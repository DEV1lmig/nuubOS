/*
 * Development aid: allows core dumps of a running process (BusyBox has no
 * prlimit), e.g. a RetroArch session started by nuubos-emud, so a core
 * taken while one of its threads waits can be read with gdb on the host.
 * Never shipped.
 *
 * Usage: core-limit PID
 */
#define _GNU_SOURCE
#include <stdio.h>
#include <stdlib.h>
#include <sys/resource.h>

int main(int argc, char **argv)
{
	struct rlimit unlimited = { RLIM_INFINITY, RLIM_INFINITY };

	if (argc != 2)
		return fprintf(stderr, "Usage: core-limit PID\n"), 2;
	if (prlimit(atoi(argv[1]), RLIMIT_CORE, &unlimited, NULL))
		return perror("prlimit"), 1;
	return 0;
}
