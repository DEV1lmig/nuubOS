/* SPDX-License-Identifier: MIT */
/*
 * nuubos-splash — the lifecycle splash on the kernel framebuffer.
 *
 *   nuubos-splash [--prepare] [boot|reboot|poweroff]
 *   nuubos-splash --kms boot        S00: KMS splash on every connected output
 *   nuubos-splash --hold            lifecycle-finalize: keep the last frame
 *   nuubos-splash --ppm FILE WIDTH HEIGHT MODE [PALETTE]   (host tests)
 *
 * boot: --kms draws the raster through KMS at each connected output's own
 * mode (HDMI: the UI's output policy, at most 1080 lines; the panel: its
 * native mode), then keeps the DRM device open without being master until
 * nuubUI is ready (/run/nuubos/ui-ready, inotify): the kernel's fbdev
 * emulation restores its own 640x480 framebuffer on the device's last
 * close, which on HDMI showed a small splash (user report 2026-10-10).
 * Without KMS the fbdev path below is used.
 * --hold: started before labwc is killed at restart/power off; holding the
 * device keeps labwc's last frame (nuubUI's full-size final splash) on
 * screen instead of the fbdev restore. It ends with the system.
 * reboot/poweroff: --prepare draws the final raster into fbdev without
 * unblanking, for the very last moment and HDMI-less fallbacks.
 * The raster itself: nuubos-splash-render.h.
 */

#include <fcntl.h>
#include <linux/fb.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/ioctl.h>
#include <sys/mman.h>
#include <unistd.h>
#include <signal.h>
#include <sys/inotify.h>
#include <xf86drm.h>
#include <xf86drmMode.h>

#include "nuubos-splash-render.h"
#include "nuubos-splash-kms.h"

/* The splash on a w x h scan-out buffer seen through transform t: drawn
 * upright at the logical size and rotated into place. */
static void render_oriented(uint32_t *dst, int stride, int w, int h, int t,
			    const char *mode, const struct nuubos_splash_palette *palette)
{
	int lw = (t & 1) ? h : w;
	int lh = (t & 1) ? w : h;
	uint32_t *buf;

	if (!t || !(buf = malloc((size_t)lw * (size_t)lh * 4))) {
		nuubos_splash_render(dst, stride, w, h, mode, palette);
		return;
	}
	nuubos_splash_render(buf, lw, lw, lh, mode, palette);
	nuubos_kms_rotate_copy(dst, stride, w, h, t, buf);
	free(buf);
}

static int kms_splash(const char *mode)
{
	struct nuubos_splash_palette palette;
	drmModeRes *res;
	uint32_t used = 0;
	int shown = 0;
	int fd = open("/dev/dri/card0", O_RDWR | O_CLOEXEC);

	if (fd < 0)
		return 1;
	res = drmModeGetResources(fd);
	if (!res) {
		close(fd);
		return 1;
	}
	nuubos_splash_load_palette(&palette, NULL);
	for (int i = 0; i < res->count_connectors; i++) {
		drmModeConnector *c = drmModeGetConnector(fd, res->connectors[i]);
		struct drm_mode_create_dumb cd = { 0 };
		struct drm_mode_map_dumb md = { 0 };
		drmModeModeInfo *m;
		uint32_t fb = 0, crtc_index;
		uint8_t *map;

		if (!c)
			continue;
		if (c->connection != DRM_MODE_CONNECTED || !(m = pick_mode(c)) ||
		    !(crtc_index = pick_crtc(fd, res, c, used))) {
			drmModeFreeConnector(c);
			continue;
		}
		cd.width = m->hdisplay;
		cd.height = m->vdisplay;
		cd.bpp = 32;
		if (drmIoctl(fd, DRM_IOCTL_MODE_CREATE_DUMB, &cd) == 0 &&
		    drmModeAddFB(fd, cd.width, cd.height, 24, 32, cd.pitch, cd.handle, &fb) == 0) {
			md.handle = cd.handle;
			if (drmIoctl(fd, DRM_IOCTL_MODE_MAP_DUMB, &md) == 0) {
				map = mmap(NULL, cd.size, PROT_READ | PROT_WRITE, MAP_SHARED, fd, md.offset);
				if (map != MAP_FAILED) {
					render_oriented((uint32_t *)map, (int)(cd.pitch / 4), m->hdisplay, m->vdisplay,
							panel_transform(fd, c->connector_id), mode, &palette);
					munmap(map, cd.size);
					if (drmModeSetCrtc(fd, res->crtcs[crtc_index - 1], fb, 0, 0, &c->connector_id, 1, m) == 0) {
						used |= 1u << (crtc_index - 1);
						shown++;
					}
				}
			}
		}
		drmModeFreeConnector(c);
	}
	drmModeFreeResources(res);
	if (!shown) {
		close(fd);
		return 1;
	}
	/* labwc must become master; the frame stays while fd is open. */
	drmDropMaster(fd);
	if (fork() == 0) {
		setsid();
		hold_device("/run/nuubos/ui-ready");
		_exit(0);
	}
	return 0;
}

static int write_ppm(const char *path, int w, int h, const char *mode, const char *palette)
{
	struct nuubos_splash_palette p;
	uint32_t *px = malloc((size_t)w * (size_t)h * 4);
	FILE *fp;

	if (!px)
		return 1;
	nuubos_splash_load_palette(&p, palette ? palette : "/nonexistent");
	nuubos_splash_render(px, w, w, h, mode, &p);
	fp = fopen(path, "wb");
	if (!fp)
		return 1;
	fprintf(fp, "P6\n%d %d\n255\n", w, h);
	for (int i = 0; i < w * h; i++) {
		unsigned char rgb[3] = { (unsigned char)(px[i] >> 16), (unsigned char)(px[i] >> 8), (unsigned char)px[i] };

		fwrite(rgb, 1, 3, fp);
	}
	fclose(fp);
	free(px);
	return 0;
}

int main(int argc, char **argv)
{
	const char *mode = "boot";
	int prepare_only = 0;
	struct fb_fix_screeninfo fix;
	struct fb_var_screeninfo var;
	struct nuubos_splash_palette palette;
	uint8_t *map;
	size_t map_size;
	int fd;

	if (argc >= 6 && !strcmp(argv[1], "--ppm"))
		return write_ppm(argv[2], atoi(argv[3]), atoi(argv[4]), argv[5], argc > 6 ? argv[6] : NULL);
	if (argc >= 2 && !strcmp(argv[1], "--hold")) {
		/* Not master, no drawing: only keeps the device open. */
		int hfd = open("/dev/dri/card0", O_RDWR | O_CLOEXEC);

		if (hfd < 0)
			return 1;
		drmDropMaster(hfd);
		if (fork() == 0) {
			setsid();
			hold_device(NULL);
			_exit(0);
		}
		return 0;
	}
	if (argc >= 2 && !strcmp(argv[1], "--kms"))
		return kms_splash(argc >= 3 ? argv[2] : "boot");

	for (int i = 1; i < argc; i++) {
		if (!strcmp(argv[i], "--prepare"))
			prepare_only = 1;
		else
			mode = argv[i];
	}
	if (!strcmp(mode, "restart"))
		mode = "reboot";
	if (!strcmp(mode, "shutdown"))
		mode = "poweroff";

	memset(&fix, 0, sizeof(fix));
	memset(&var, 0, sizeof(var));
	fd = open("/dev/fb0", O_RDWR | O_CLOEXEC);
	if (fd < 0) {
		perror("open /dev/fb0");
		return 1;
	}
	if (ioctl(fd, FBIOGET_FSCREENINFO, &fix) < 0 || ioctl(fd, FBIOGET_VSCREENINFO, &var) < 0) {
		perror("fb0 info");
		close(fd);
		return 1;
	}
	if (var.bits_per_pixel != 32) {
		fprintf(stderr, "unsupported fb0 bpp: %u\n", var.bits_per_pixel);
		close(fd);
		return 1;
	}
	map_size = (size_t)fix.line_length * (size_t)var.yres_virtual;
	map = mmap(NULL, map_size, PROT_READ | PROT_WRITE, MAP_SHARED, fd, 0);
	if (map == MAP_FAILED) {
		perror("mmap fb0");
		close(fd);
		return 1;
	}
	if (!prepare_only)
		ioctl(fd, FBIOBLANK, FB_BLANK_UNBLANK);

	nuubos_splash_load_palette(&palette, NULL);
	render_oriented((uint32_t *)map, (int)(fix.line_length / 4), (int)var.xres, (int)var.yres,
			builtin_panel_transform(), mode, &palette);

	if (msync(map, map_size, MS_SYNC) < 0)
		perror("msync fb0");
	if (!prepare_only)
		ioctl(fd, FBIOBLANK, FB_BLANK_UNBLANK);
	munmap(map, map_size);
	close(fd);
	return 0;
}
