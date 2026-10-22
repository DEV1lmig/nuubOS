/* pcsample TID SECONDS [HZ]: poor man's profiler for the device (no perf
 * there). Interrupts thread TID with ptrace HZ times a second (default
 * 300), prints its program counter, link register and x1 (hex, "pc lr x1" per
 * line: lr names the caller of a leaf function such as memcpy) and lets it
 * go on; x1 is the request of an ioctl).
 * Resolve on the host with pcsample-report.py against /proc/PID/maps and
 * the unstripped libraries of output/h700/target. Test only.
 * Build: aarch64-...-gcc -O2 -o pcsample pcsample.c */
#define _GNU_SOURCE
#include <elf.h>
#include <errno.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <sys/ptrace.h>
#include <sys/uio.h>
#include <sys/wait.h>
#include <time.h>
#include <unistd.h>

struct regs {
	uint64_t x[31];
	uint64_t sp;
	uint64_t pc;
	uint64_t pstate;
};

int main(int argc, char **argv)
{
	pid_t tid;
	long n, i;
	int hz;

	if (argc < 3)
		return 2;
	tid = atoi(argv[1]);
	hz = argc > 3 ? atoi(argv[3]) : 300;
	n = (long)atoi(argv[2]) * hz;
	if (ptrace(PTRACE_SEIZE, tid, 0, 0) != 0) {
		perror("seize");
		return 1;
	}
	for (i = 0; i < n; i++) {
		struct timespec ts = { 0, 1000000000L / hz };
		struct regs r;
		struct iovec io = { &r, sizeof(r) };
		int st;

		nanosleep(&ts, NULL);
		if (ptrace(PTRACE_INTERRUPT, tid, 0, 0) != 0)
			break;
		if (waitpid(tid, &st, __WALL) < 0)
			break;
		if (ptrace(PTRACE_GETREGSET, tid, (void *)NT_PRSTATUS, &io) == 0)
			printf("%llx %llx %llx\n", (unsigned long long)r.pc,
					(unsigned long long)r.x[30], (unsigned long long)r.x[1]);
		ptrace(PTRACE_CONT, tid, 0, 0);
	}
	ptrace(PTRACE_DETACH, tid, 0, 0);
	return 0;
}
