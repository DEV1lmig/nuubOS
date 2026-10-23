/* nuubos-dram-blob DISK: keep the SPL's trained DRAM setup on the boot card
 * (U-Boot patch 0007, docs/dev/platform.md "Cached DRAM training").
 *
 * After every DRAM init that passed its tests the SPL leaves a blob in
 * BL31's reserved page (not System RAM, so /dev/mem may map it): magic,
 * version, the SoC's SID, RTC GP1-GP4/GP7 and the whole trained struct
 * dram_para, checksum. A cold boot with the trained mark, and a resume
 * from universal standby, restore the DRAM delays from sector 2040 of the
 * boot card; this writes the blob there when it differs (after a full
 * training), never into a partition. */
#include <fcntl.h>
#include <stdint.h>
#include <stdio.h>
#include <string.h>
#include <sys/mman.h>
#include <unistd.h>

#define BLOB_DRAM 0x4003f000UL
#define BLOB_MAGIC 0x4e445442u /* "NDTB" */
#define BLOB_VERSION 1u
#define BLOB_SECTOR 2040u
#define BLOB_WORDS 44 /* magic, version, sid[4], rtc[5], para[32], sum */

static uint32_t blob_sum(const uint32_t *w)
{
	uint32_t sum = 0x5a5a5a5a;
	int i;

	for (i = 0; i < BLOB_WORDS - 1; i++)
		sum = (sum << 5 | sum >> 27) ^ w[i];
	return sum;
}

int main(int argc, char **argv)
{
	uint32_t blob[BLOB_WORDS];
	unsigned char sector[512], mbr[512];
	const volatile uint32_t *page;
	uint32_t p1_start;
	int fd, i;

	if (argc != 2) {
		fprintf(stderr, "Usage: nuubos-dram-blob DISK\n");
		return 2;
	}
	fd = open("/dev/mem", O_RDONLY | O_SYNC);
	if (fd < 0)
		return perror("/dev/mem"), 1;
	page = mmap(NULL, 4096, PROT_READ, MAP_SHARED, fd, BLOB_DRAM);
	close(fd);
	if (page == MAP_FAILED)
		return perror("mmap"), 1;
	for (i = 0; i < BLOB_WORDS; i++)
		blob[i] = page[i];
	if (blob[0] != BLOB_MAGIC || blob[1] != BLOB_VERSION ||
	    blob[BLOB_WORDS - 1] != blob_sum(blob)) {
		printf("none\n");
		return 0;
	}

	fd = open(argv[1], O_RDWR | O_SYNC);
	if (fd < 0)
		return perror(argv[1]), 1;
	/* Partition 1 must start after the sector (MBR entry 1, LBA start). */
	if (pread(fd, mbr, sizeof(mbr), 0) != sizeof(mbr) ||
	    mbr[510] != 0x55 || mbr[511] != 0xaa)
		return fprintf(stderr, "no MBR on %s\n", argv[1]), 1;
	memcpy(&p1_start, mbr + 446 + 8, 4);
	if (p1_start <= BLOB_SECTOR)
		return fprintf(stderr, "partition 1 at %u, not after %u\n",
			       p1_start, BLOB_SECTOR), 1;
	if (pread(fd, sector, sizeof(sector), BLOB_SECTOR * 512ul) != sizeof(sector))
		return perror("read"), 1;
	if (!memcmp(sector, blob, sizeof(blob))) {
		printf("unchanged\n");
		return 0;
	}
	memset(sector, 0, sizeof(sector));
	memcpy(sector, blob, sizeof(blob));
	if (pwrite(fd, sector, sizeof(sector), BLOB_SECTOR * 512ul) != sizeof(sector) ||
	    fsync(fd))
		return perror("write"), 1;
	close(fd);
	printf("saved\n");
	return 0;
}
