/*
 * Development aid: turns nuubos-inputd's game gamepads on (as nuubos-emud
 * does) and prints what every "nuubOS Gamepad" (player 1, 2...) sends to a game, so a
 * physical button can be traced to the game's button. Never shipped.
 *
 * Usage: pad-watch [seconds] [passive] (default 20 s). passive: only read
 * the gamepads of a game already running (no GAMEPADS ON).
 */
#include <fcntl.h>
#include <linux/input.h>
#include <poll.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/ioctl.h>
#include <sys/socket.h>
#include <sys/un.h>
#include <time.h>
#include <unistd.h>

int main(int argc, char **argv)
{
	struct sockaddr_un a;
	char reply[16] = "", path[64];
	struct pollfd p[8];
	int ctl = socket(AF_UNIX, SOCK_STREAM, 0), n = 0, seconds = argc > 1 ? atoi(argv[1]) : 20;
	time_t end;

	memset(&a, 0, sizeof(a));
	a.sun_family = AF_UNIX;
	snprintf(a.sun_path, sizeof(a.sun_path), "/run/nuubos/inputd.sock");
	if (argc > 2 && !strcmp(argv[2], "passive")) {
		close(ctl);
		ctl = -1;
	} else if (ctl < 0 || connect(ctl, (struct sockaddr *)&a, sizeof(a)) ||
		   write(ctl, "GAMEPADS ON\n", 12) != 12 || read(ctl, reply, sizeof(reply) - 1) <= 0) {
		return perror("inputd"), 1;
	} else {
		usleep(1500000);
	}
	for (int i = 0; i < 64 && n < 8; i++) {
		char name[128] = "";
		int fd;

		snprintf(path, sizeof(path), "/dev/input/event%d", i);
		fd = open(path, O_RDONLY | O_NONBLOCK);
		if (fd < 0)
			continue;
		ioctl(fd, EVIOCGNAME(sizeof(name)), name);
		if (strcmp(name, "nuubOS Gamepad")) {
			close(fd);
			continue;
		}
		p[n++] = (struct pollfd){ fd, POLLIN, 0 };
	}
	if (!n)
		return fprintf(stderr, "no nuubOS Gamepad\n"), 1;
	printf("watching %d game gamepads for %d s (pad 1 = player 1)\n", n, seconds);
	fflush(stdout);
	end = time(NULL) + seconds;
	while (time(NULL) < end) {
		struct input_event ev;

		if (poll(p, (nfds_t)n, 500) <= 0)
			continue;
		for (int k = 0; k < n; k++)
			while (read(p[k].fd, &ev, sizeof(ev)) == sizeof(ev))
				if (ev.type == EV_KEY || (ev.type == EV_ABS && (ev.value > 8000 || ev.value < -8000 || ev.code >= ABS_Z)))
					printf("pad %d %s code=%d value=%d\n", k + 1, ev.type == EV_KEY ? "KEY" : "ABS", ev.code, ev.value);
		fflush(stdout);
	}
	for (int k = 0; k < n; k++)
		close(p[k].fd);
	if (ctl >= 0)
		close(ctl);
	return 0;
}
