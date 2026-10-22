/*
 * Development aid: a stick direction held while the controllers change
 * hands between the Quick Menu and a running game (user report 2026-10-22,
 * CubeXX: menus scrolling by themselves). A virtual pad with a stick is
 * created; game gamepads are turned on (as nuubos-emud does), the menu is
 * opened, the stick is held down (navigation repeat starts), the menu is
 * closed with the stick still down and the stick returns to the centre
 * (now a game event). The menu_down presses seen by a generic subscriber
 * are counted after the close and after the menu opens again: both must be
 * 0. Needs nuubos-emuctl idle. Never shipped.
 *
 * Usage: nav-owner-test [ready_s]
 */
#include <fcntl.h>
#include <linux/uinput.h>
#include <poll.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/ioctl.h>
#include <sys/socket.h>
#include <sys/un.h>
#include <time.h>
#include <unistd.h>

#define SOCKET_PATH "/run/nuubos/inputd.sock"

static void emit(int fd, int type, int code, int value)
{
	struct input_event ev;

	memset(&ev, 0, sizeof(ev));
	ev.type = type;
	ev.code = code;
	ev.value = value;
	if (write(fd, &ev, sizeof(ev)) != sizeof(ev))
		perror("nav-owner-test: write");
}

static void stick_y(int fd, int value)
{
	emit(fd, EV_ABS, ABS_Y, value);
	emit(fd, EV_SYN, SYN_REPORT, 0);
}

static int make_pad(void)
{
	static const int keys[] = { BTN_SOUTH, BTN_EAST, BTN_NORTH, BTN_WEST, BTN_START, BTN_SELECT, BTN_MODE,
				    BTN_DPAD_UP, BTN_DPAD_DOWN, BTN_DPAD_LEFT, BTN_DPAD_RIGHT, BTN_TL, BTN_TR };
	struct uinput_setup setup;
	struct uinput_abs_setup abs;
	int fd = open("/dev/uinput", O_WRONLY | O_NONBLOCK);

	if (fd < 0)
		return perror("open /dev/uinput"), -1;
	ioctl(fd, UI_SET_EVBIT, EV_KEY);
	ioctl(fd, UI_SET_EVBIT, EV_ABS);
	for (size_t i = 0; i < sizeof(keys) / sizeof(keys[0]); i++)
		ioctl(fd, UI_SET_KEYBIT, keys[i]);
	for (int code = ABS_X; code <= ABS_Y; code++) {
		ioctl(fd, UI_SET_ABSBIT, code);
		memset(&abs, 0, sizeof(abs));
		abs.code = code;
		abs.absinfo.minimum = 0;
		abs.absinfo.maximum = 4096;
		abs.absinfo.value = 2048;
		ioctl(fd, UI_ABS_SETUP, &abs);
	}
	memset(&setup, 0, sizeof(setup));
	setup.id.bustype = BUS_VIRTUAL;
	setup.id.vendor = 0x1209;
	setup.id.product = 0x0d74;
	snprintf(setup.name, sizeof(setup.name), "nuubOS nav owner test pad");
	ioctl(fd, UI_DEV_SETUP, &setup);
	if (ioctl(fd, UI_DEV_CREATE) < 0)
		return perror("UI_DEV_CREATE"), close(fd), -1;
	return fd;
}

static int connect_inputd(const char *hello)
{
	struct sockaddr_un addr;
	int fd = socket(AF_UNIX, SOCK_STREAM, 0);

	memset(&addr, 0, sizeof(addr));
	addr.sun_family = AF_UNIX;
	snprintf(addr.sun_path, sizeof(addr.sun_path), "%s", SOCKET_PATH);
	if (fd < 0 || connect(fd, (struct sockaddr *)&addr, sizeof(addr)) < 0)
		return perror("connect inputd"), -1;
	if (write(fd, hello, strlen(hello)) != (ssize_t)strlen(hello))
		return perror("write inputd"), close(fd), -1;
	return fd;
}

static void command(int ctl, const char *line)
{
	char reply[64];

	if (write(ctl, line, strlen(line)) != (ssize_t)strlen(line) ||
	    read(ctl, reply, sizeof(reply)) <= 0)
		fprintf(stderr, "nav-owner-test: %s failed\n", line);
}

/* menu_down presses read from the subscriber within ms. */
static int count_down(int sub, int ms)
{
	struct timespec start, now;
	static char buf[8192];
	static size_t used;
	int presses = 0;

	clock_gettime(CLOCK_MONOTONIC, &start);
	for (;;) {
		struct pollfd p = { .fd = sub, .events = POLLIN };
		long elapsed;

		clock_gettime(CLOCK_MONOTONIC, &now);
		elapsed = (now.tv_sec - start.tv_sec) * 1000 + (now.tv_nsec - start.tv_nsec) / 1000000;
		if (elapsed >= ms)
			break;
		if (poll(&p, 1, (int)(ms - elapsed)) <= 0)
			continue;
		ssize_t n = read(sub, buf + used, sizeof(buf) - used - 1);
		if (n <= 0)
			break;
		used += (size_t)n;
		buf[used] = '\0';
		char *line = buf, *nl;
		while ((nl = strchr(line, '\n')) != NULL) {
			*nl = '\0';
			if (strstr(line, "menu_down pressed"))
				presses++;
			line = nl + 1;
		}
		used = strlen(line);
		memmove(buf, line, used);
	}
	return presses;
}

int main(int argc, char **argv)
{
	int pad, ctl, sub, held, after_close, after_reopen;

	pad = make_pad();
	if (pad < 0)
		return 1;
	/* inputd opens it on hotplug, controllersd maps it. */
	sleep(argc > 1 ? atoi(argv[1]) : 7);
	ctl = connect_inputd("");
	sub = connect_inputd("SUBSCRIBE\n");
	if (ctl < 0 || sub < 0)
		return 1;
	command(ctl, "GAMEPADS ON\n");
	command(ctl, "MENU OPEN\n");
	count_down(sub, 300);

	stick_y(pad, 4000);
	held = count_down(sub, 1000);
	command(ctl, "MENU CLOSE\n");
	usleep(100000);
	stick_y(pad, 2048);
	count_down(sub, 100);
	after_close = count_down(sub, 1500);
	command(ctl, "MENU OPEN\n");
	after_reopen = count_down(sub, 1500);
	command(ctl, "MENU CLOSE\n");
	command(ctl, "GAMEPADS OFF\n");

	printf("held in menu: %d presses (want >1)\n", held);
	printf("after close, stick centred: %d (want 0)\n", after_close);
	printf("after reopening the menu: %d (want 0)\n", after_reopen);
	printf("%s\n", held > 1 && !after_close && !after_reopen ? "[PASS]" : "[FAIL]");

	close(sub);
	close(ctl);
	ioctl(pad, UI_DEV_DESTROY);
	close(pad);
	return 0;
}
