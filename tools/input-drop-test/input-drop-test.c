/*
 * Development aid: reproduces a lost release (user report 2026-10-21:
 * "it starts moving by itself as if a button were held", in games and
 * menus). A virtual pad presses D-pad Down, lets inputd read it, then
 * floods axis events while nuubos-inputd is stopped (SIGSTOP), releases Down in the middle of the
 * flood and floods again: the kernel's per-client buffer overflows, drops
 * the queued events (the release with them) and queues SYN_DROPPED. Never
 * shipped.
 *
 * Usage: input-drop-test INPUTD_PID [settle_s] [ready_s] [unplug]
 *   prints "pad ready", stops inputd, does the sequence, continues inputd,
 *   keeps the pad open settle_s seconds (default 4) so inputd can read it;
 *   ready_s (default 6): the wait after creating the pad. "unplug": hold
 *   Down 1.5 s and remove the pad without a release (a Bluetooth
 *   controller dropping while a direction is held).
 */
#include <fcntl.h>
#include <linux/uinput.h>
#include <signal.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/ioctl.h>
#include <time.h>
#include <unistd.h>

static int fd = -1;

static void emit(int type, int code, int value)
{
	struct input_event ev;

	memset(&ev, 0, sizeof(ev));
	ev.type = type;
	ev.code = code;
	ev.value = value;
	if (write(fd, &ev, sizeof(ev)) != sizeof(ev))
		perror("input-drop-test: write");
}

static void flood(int n)
{
	for (int i = 0; i < n; i++) {
		emit(EV_ABS, ABS_RX, (i & 1) ? 1000 : -1000);
		emit(EV_SYN, SYN_REPORT, 0);
	}
}

int main(int argc, char **argv)
{
	struct uinput_setup setup;
	struct uinput_abs_setup abs;
	static const int keys[] = { BTN_SOUTH, BTN_EAST, BTN_NORTH, BTN_WEST, BTN_START, BTN_SELECT, BTN_MODE,
				    BTN_DPAD_UP, BTN_DPAD_DOWN, BTN_DPAD_LEFT, BTN_DPAD_RIGHT, BTN_TL, BTN_TR };
	pid_t inputd;
	int settle;

	if (argc < 2)
		return fprintf(stderr, "Usage: input-drop-test INPUTD_PID [settle_s]\n"), 2;
	inputd = (pid_t)atoi(argv[1]);
	settle = argc > 2 ? atoi(argv[2]) : 4;
	fd = open("/dev/uinput", O_WRONLY | O_NONBLOCK);
	if (fd < 0)
		return perror("open /dev/uinput"), 1;
	ioctl(fd, UI_SET_EVBIT, EV_KEY);
	ioctl(fd, UI_SET_EVBIT, EV_ABS);
	for (size_t i = 0; i < sizeof(keys) / sizeof(keys[0]); i++)
		ioctl(fd, UI_SET_KEYBIT, keys[i]);
	memset(&abs, 0, sizeof(abs));
	abs.code = ABS_RX;
	abs.absinfo.minimum = -32767;
	abs.absinfo.maximum = 32767;
	ioctl(fd, UI_ABS_SETUP, &abs);
	memset(&setup, 0, sizeof(setup));
	setup.id.bustype = BUS_VIRTUAL;
	setup.id.vendor = 0x1209;
	setup.id.product = 0x0d70;
	snprintf(setup.name, sizeof(setup.name), "nuubOS drop test pad");
	ioctl(fd, UI_DEV_SETUP, &setup);
	if (ioctl(fd, UI_DEV_CREATE) < 0)
		return perror("UI_DEV_CREATE"), 1;
	/* inputd opens it on hotplug and again once controllersd has its
	 * mapping (a reopen drops what is queued). */
	sleep(argc > 3 ? atoi(argv[3]) : 6);
	printf("pad ready\n");
	fflush(stdout);
	if (argc > 4 && !strcmp(argv[4], "unplug")) {
		emit(EV_KEY, BTN_DPAD_DOWN, 1);
		emit(EV_SYN, SYN_REPORT, 0);
		usleep(1500000);
		ioctl(fd, UI_DEV_DESTROY);
		close(fd);
		printf("unplugged while Down was held\n");
		return 0;
	}
	/* The press is read (the repeat starts), then inputd falls behind. */
	emit(EV_KEY, BTN_DPAD_DOWN, 1);
	emit(EV_SYN, SYN_REPORT, 0);
	usleep(200000);
	kill(inputd, SIGSTOP);
	flood(600);
	emit(EV_KEY, BTN_DPAD_DOWN, 0);
	emit(EV_SYN, SYN_REPORT, 0);
	flood(600);
	kill(inputd, SIGCONT);
	printf("sequence done (Down released during the flood)\n");
	fflush(stdout);
	sleep(settle);
	ioctl(fd, UI_DEV_DESTROY);
	close(fd);
	return 0;
}
