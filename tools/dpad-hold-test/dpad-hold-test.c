/*
 * Development aid: a direction held on a controller while another input
 * device appears (Reddit report 2026-10-21, RG SP: "a D-pad direction
 * stops responding and takes some time to recover"). Needs a game running
 * (game gamepads on). A virtual pad holds D-pad Right; a second device is
 * created ("pad" as the argument: a second controller, assigned a player,
 * so the PADMAP really changes; else a remote) (inputd rescans,
 * controllersd sends PADMAP); the ABS_HAT0X of
 * every "nuubOS Gamepad" is printed: Right must still be held (1). Then
 * Right is released right after a third device appears: the hat must go
 * back to 0. Never shipped.
 *
 * Usage: dpad-hold-test [ready_s] [pad]
 */
#include <dirent.h>
#include <fcntl.h>
#include <linux/uinput.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/ioctl.h>
#include <unistd.h>

static void emit(int fd, int type, int code, int value)
{
	struct input_event ev;

	memset(&ev, 0, sizeof(ev));
	ev.type = type;
	ev.code = code;
	ev.value = value;
	if (write(fd, &ev, sizeof(ev)) != sizeof(ev))
		perror("dpad-hold-test: write");
}

static int make_device(const char *name, unsigned short product, int pad)
{
	static const int keys[] = { BTN_SOUTH, BTN_EAST, BTN_NORTH, BTN_WEST, BTN_START, BTN_SELECT, BTN_MODE,
				    BTN_DPAD_UP, BTN_DPAD_DOWN, BTN_DPAD_LEFT, BTN_DPAD_RIGHT, BTN_TL, BTN_TR };
	struct uinput_setup setup;
	int fd = open("/dev/uinput", O_WRONLY | O_NONBLOCK);

	if (fd < 0)
		return perror("open /dev/uinput"), -1;
	ioctl(fd, UI_SET_EVBIT, EV_KEY);
	if (pad)
		for (size_t i = 0; i < sizeof(keys) / sizeof(keys[0]); i++)
			ioctl(fd, UI_SET_KEYBIT, keys[i]);
	else
		ioctl(fd, UI_SET_KEYBIT, KEY_PLAYPAUSE);
	memset(&setup, 0, sizeof(setup));
	setup.id.bustype = BUS_VIRTUAL;
	setup.id.vendor = 0x1209;
	setup.id.product = product;
	snprintf(setup.name, sizeof(setup.name), "%s", name);
	ioctl(fd, UI_DEV_SETUP, &setup);
	if (ioctl(fd, UI_DEV_CREATE) < 0)
		return perror("UI_DEV_CREATE"), close(fd), -1;
	return fd;
}

static void print_hats(const char *when)
{
	DIR *dir = opendir("/dev/input");
	struct dirent *e;

	printf("%s:", when);
	while (dir && (e = readdir(dir)) != NULL) {
		char path[300], name[128] = "";
		struct input_absinfo info;
		int fd;

		if (strncmp(e->d_name, "event", 5))
			continue;
		snprintf(path, sizeof(path), "/dev/input/%s", e->d_name);
		fd = open(path, O_RDONLY | O_NONBLOCK);
		if (fd < 0)
			continue;
		ioctl(fd, EVIOCGNAME(sizeof(name)), name);
		if (!strncmp(name, "nuubOS Gamepad", 14) &&
		    ioctl(fd, EVIOCGABS(ABS_HAT0X), &info) == 0)
			printf(" [%s hat_x=%d]", name, info.value);
		close(fd);
	}
	if (dir)
		closedir(dir);
	printf("\n");
	fflush(stdout);
}

int main(int argc, char **argv)
{
	int pad, other, third;

	pad = make_device("nuubOS hold test pad", 0x0d71, 1);
	if (pad < 0)
		return 1;
	/* inputd opens it on hotplug, controllersd maps it. */
	sleep(argc > 1 ? atoi(argv[1]) : 6);
	emit(pad, EV_KEY, BTN_DPAD_RIGHT, 1);
	emit(pad, EV_SYN, SYN_REPORT, 0);
	usleep(500000);
	print_hats("held");

	other = make_device("nuubOS hold test remote", 0x0d72,
			    argc > 2 && !strcmp(argv[2], "pad"));
	sleep(3);
	print_hats("held, after a device appeared (want 1)");

	third = make_device("nuubOS hold test remote 2", 0x0d73, 0);
	usleep(2000);
	emit(pad, EV_KEY, BTN_DPAD_RIGHT, 0);
	emit(pad, EV_SYN, SYN_REPORT, 0);
	sleep(3);
	print_hats("released as a device appeared (want 0)");

	if (third >= 0)
		ioctl(third, UI_DEV_DESTROY), close(third);
	if (other >= 0)
		ioctl(other, UI_DEV_DESTROY), close(other);
	ioctl(pad, UI_DEV_DESTROY);
	close(pad);
	return 0;
}
