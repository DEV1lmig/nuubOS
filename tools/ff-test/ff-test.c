/*
 * Development aid: plays one rumble effect on an evdev device, straight
 * through the kernel's force feedback (no nuubOS service in between), to
 * tell a driver problem from a routing one. Never shipped.
 *
 * Usage: ff-test /dev/input/eventN [strong 0-65535] [weak 0-65535] [ms]
 *        ff-test game|game2 [strong] [weak] [ms]
 *   game: turns nuubos-inputd's game gamepads on (as nuubos-emud does),
 *   plays on the first "nuubOS Gamepad" (game2: the second, player 2)
 *   (player 1: the effect goes through
 *   inputd to that player's controllers, as in a game) and turns them off.
 *   A direct play on a controller is dropped by the kernel while inputd
 *   grabs it (EVIOCGRAB: only the grabbing handle may inject events).
 */
#include <fcntl.h>
#include <linux/input.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/ioctl.h>
#include <sys/socket.h>
#include <sys/un.h>
#include <unistd.h>

/* The event node of the first nuubOS Gamepad, after GAMEPADS ON. */
static int find_vpad(char *path, size_t size, int skip)
{
	for (int i = 0; i < 64; i++) {
		char name[128] = "";
		int fd;

		snprintf(path, size, "/dev/input/event%d", i);
		fd = open(path, O_RDWR);
		if (fd < 0)
			continue;
		ioctl(fd, EVIOCGNAME(sizeof(name)), name);
		close(fd);
		if (!strcmp(name, "nuubOS Gamepad") && skip-- == 0)
			return 0;
	}
	return -1;
}

int main(int argc, char **argv)
{
	struct ff_effect e;
	struct input_event play;
	int fd, ms;

	char path[64];
	int ctl = -1;

	if (argc < 2)
		return fprintf(stderr, "Usage: ff-test /dev/input/eventN|game [strong] [weak] [ms]\n"), 2;
	snprintf(path, sizeof(path), "%s", argv[1]);
	if (!strncmp(argv[1], "game", 4)) {
		struct sockaddr_un a;
		char reply[16] = "";

		ctl = socket(AF_UNIX, SOCK_STREAM, 0);
		memset(&a, 0, sizeof(a));
		a.sun_family = AF_UNIX;
		snprintf(a.sun_path, sizeof(a.sun_path), "/run/nuubos/inputd.sock");
		if (ctl < 0 || connect(ctl, (struct sockaddr *)&a, sizeof(a)) ||
		    write(ctl, "GAMEPADS ON\n", 12) != 12 || read(ctl, reply, sizeof(reply) - 1) <= 0)
			return perror("inputd"), 1;
		usleep(1500000);
		if (find_vpad(path, sizeof(path), !strcmp(argv[1], "game2")))
			return fprintf(stderr, "no nuubOS Gamepad\n"), 1;
		printf("game gamepad %s\n", path);
	}
	fd = open(path, O_RDWR);
	if (fd < 0)
		return perror("open"), 1;
	ms = argc > 4 ? atoi(argv[4]) : 1000;
	memset(&e, 0, sizeof(e));
	e.type = FF_RUMBLE;
	e.id = -1;
	e.u.rumble.strong_magnitude = (unsigned short)(argc > 2 ? atoi(argv[2]) : 0xffff);
	e.u.rumble.weak_magnitude = (unsigned short)(argc > 3 ? atoi(argv[3]) : 0xffff);
	e.replay.length = (unsigned short)ms;
	if (ioctl(fd, EVIOCSFF, &e) < 0)
		return perror("EVIOCSFF"), 1;
	printf("uploaded effect id=%d\n", e.id);
	memset(&play, 0, sizeof(play));
	play.type = EV_FF;
	play.code = (unsigned short)e.id;
	play.value = 1;
	if (write(fd, &play, sizeof(play)) != sizeof(play))
		return perror("play"), 1;
	printf("playing %d ms\n", ms);
	fflush(stdout);
	usleep((useconds_t)(ms + 200) * 1000);
	ioctl(fd, EVIOCRMFF, e.id);
	close(fd);
	if (ctl >= 0)
		close(ctl); /* inputd turns the gamepads off with their owner */
	return 0;
}
