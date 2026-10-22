/* nuubos-boot-stages: how the time before Linux splits, for the diagnostic
 * mode (S04nuubos-diagnostics, docs/dev/platform.md "Boot time").
 *
 * The arm64 system counter runs since the SoC reset; the kernel's clock
 * starts at its own entry, so counter - uptime is the time before Linux.
 * The SPL leaves the counter's milliseconds at the start and the end of the
 * cold-boot DRAM init in RTC GP5 (U-Boot patch 0005, low/high 16 bits);
 * nuubos-suspend overwrites GP5 before every sleep, so read it early. */
#include <fcntl.h>
#include <stdint.h>
#include <stdio.h>
#include <sys/mman.h>
#include <time.h>
#include <unistd.h>

#define RTC_PAGE 0x07000000UL
#define RTC_GP5 0x114

int main(void)
{
	uint64_t count, freq;
	struct timespec ts;
	double before;
	int fd;

	__asm__ volatile("isb; mrs %0, cntvct_el0" : "=r"(count));
	__asm__ volatile("mrs %0, cntfrq_el0" : "=r"(freq));
	clock_gettime(CLOCK_BOOTTIME, &ts);
	before = (double)count / (double)freq - (ts.tv_sec + ts.tv_nsec / 1e9);
	printf("before_linux_s=%.3f\n", before);

	fd = open("/dev/mem", O_RDONLY | O_SYNC | O_CLOEXEC);
	if (fd >= 0) {
		volatile uint32_t *rtc = mmap(NULL, 4096, PROT_READ, MAP_SHARED, fd, RTC_PAGE);

		if (rtc != MAP_FAILED) {
			uint32_t gp5 = rtc[RTC_GP5 / 4];
			unsigned int start = gp5 & 0xffff, end = gp5 >> 16;

			/* Anything else there is not ours (a resume, an old SPL). */
			if (end > start && end < before * 1000 + 1000) {
				printf("spl_dram_start_s=%.3f\n", start / 1000.0);
				printf("spl_dram_end_s=%.3f\n", end / 1000.0);
				printf("dram_init_s=%.3f\n", (end - start) / 1000.0);
				printf("after_dram_s=%.3f\n", before - end / 1000.0);
			} else {
				printf("rtc_gp5=0x%08x\n", gp5);
			}
			munmap((void *)rtc, 4096);
		}
		close(fd);
	}
	return 0;
}
