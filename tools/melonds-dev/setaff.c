#define _GNU_SOURCE
#include <sched.h>
#include <stdio.h>
#include <stdlib.h>
/* setaff HEXMASK TID... */
int main(int argc, char **argv)
{
	unsigned long m = strtoul(argv[1], NULL, 16);
	cpu_set_t s; CPU_ZERO(&s);
	for (int c = 0; c < 8; c++) if (m & (1ul << c)) CPU_SET(c, &s);
	int rc = 0;
	for (int i = 2; i < argc; i++)
		if (sched_setaffinity(atoi(argv[i]), sizeof s, &s)) { perror(argv[i]); rc = 1; }
	return rc;
}
