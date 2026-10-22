/* frametime.so: LD_PRELOAD into RetroArch (test only, never shipped).
 * Writes the time between presented frames in microseconds, one u32 per
 * frame, taken when eglSwapBuffers (or eglSwapBuffersWithDamage*) RETURNS:
 * Mesa's Wayland swap first waits for the frame callback of the swap
 * before and then commits, so the return is the commit; the time a swap is
 * entered only says when the CPU finished the frame (it showed 2/30 ms
 * pairs with perfectly regular commits, 2026-10-22). Output to
 * $FRAMETIME_OUT (default /tmp/frametime.bin), flushed every 64 frames.
 * Build: aarch64-...-gcc -O2 -shared -fPIC -o frametime.so frametime.c -ldl
 * Read with frametime-report.sh. */
#define _GNU_SOURCE
#include <dlfcn.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <time.h>

typedef unsigned int EGLBoolean;
typedef void *EGLDisplay;
typedef void *EGLSurface;
typedef int32_t EGLint;

static FILE *out;
static uint64_t last;
static uint32_t buf[64];
static int used;

static void note_frame(void)
{
	struct timespec ts;
	uint64_t now;

	clock_gettime(CLOCK_MONOTONIC, &ts);
	now = (uint64_t)ts.tv_sec * 1000000u + ts.tv_nsec / 1000;
	if (!out) {
		const char *path = getenv("FRAMETIME_OUT");
		out = fopen(path ? path : "/tmp/frametime.bin", "wb");
		if (!out)
			return;
	}
	if (last) {
		buf[used++] = (uint32_t)(now - last);
		if (used == 64) {
			fwrite(buf, sizeof(buf[0]), used, out);
			fflush(out);
			used = 0;
		}
	}
	last = now;
}

EGLBoolean eglSwapBuffers(EGLDisplay dpy, EGLSurface surface)
{
	static EGLBoolean (*real)(EGLDisplay, EGLSurface);

	EGLBoolean ret;

	if (!real)
		real = dlsym(RTLD_NEXT, "eglSwapBuffers");
	ret = real(dpy, surface);
	note_frame();
	return ret;
}

EGLBoolean eglSwapBuffersWithDamageKHR(EGLDisplay dpy, EGLSurface surface,
		const EGLint *rects, EGLint n)
{
	static EGLBoolean (*real)(EGLDisplay, EGLSurface, const EGLint *, EGLint);

	EGLBoolean ret;

	if (!real)
		real = dlsym(RTLD_NEXT, "eglSwapBuffersWithDamageKHR");
	ret = real(dpy, surface, rects, n);
	note_frame();
	return ret;
}

EGLBoolean eglSwapBuffersWithDamageEXT(EGLDisplay dpy, EGLSurface surface,
		const EGLint *rects, EGLint n)
{
	static EGLBoolean (*real)(EGLDisplay, EGLSurface, const EGLint *, EGLint);

	EGLBoolean ret;

	if (!real)
		real = dlsym(RTLD_NEXT, "eglSwapBuffersWithDamageEXT");
	ret = real(dpy, surface, rects, n);
	note_frame();
	return ret;
}
