/* SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0 */
/*
 * nuubos-updatectl — OTA updates, network and verification side (EPIC-048).
 *
 *   status        provider=, channel=, auto=, current=, build=, available=,
 *                 version=, notes= (release notes, escaped), downloaded=
 *   channel [NAME] show or choose the release channel (stable|rc|nightly)
 *   auto [on|off] show or set the daily automatic check (default on;
 *                 run by nuubos-update-auto)
 *   check         fetch and verify the release manifest of the channel:
 *                 OK available V | OK current | OK none (nothing published
 *                 on the channel) | ERR network | server | signature
 *   download      fetch the payload (resumable), verify its SHA-256 (a job)
 *   verify        re-check the downloaded payload against the manifest
 *   write DEVICE  unpack the verified system image onto DEVICE (the
 *                 inactive system slot), then read it back and check it
 *
 * Releases are GitHub Releases. Each channel has a fixed release tag
 * "channel-<name>" whose assets are the current manifest and signature
 * (replaced at every publication); the manifest names the payload by
 * "<release tag>/<file>", a versioned release of its own.
 *
 * Trust comes from the signature, never from the transport: the manifest
 * (a small key=value file) is signed with the nuubOS release key (Ed25519)
 * and carries the payload size and SHA-256 and the SHA-256 of the system
 * image inside it. Its channel must be the one asked for, and only a
 * higher build serial than the running one is offered (an old signed
 * manifest can never downgrade). The public key is compiled in from
 * board/nuubos/common/ota/update-key.pub; a build without it reports
 * "unavailable". nuubos-update-apply installs into the inactive A/B slot.
 *
 * Files: /userdata/.nuubos-update/{manifest,manifest.sig,payload.part,payload}
 * (USERDATA has room; the hidden folder is not shown in Files); the
 * channel in /state/update/channel, auto-off when the daily check is off.
 */

#include <ctype.h>
#include <errno.h>
#include <fcntl.h>
#include <limits.h>
#include <signal.h>
#include <stdbool.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <linux/fs.h>
#include <sys/ioctl.h>
#include <sys/stat.h>
#include <sys/statvfs.h>
#include <unistd.h>

#include <curl/curl.h>
#include <sodium.h>
#include <zlib.h>

#ifndef UPDATE_URL
#define UPDATE_URL "https://github.com/baf93/nuubOS/releases/download"
#endif
#ifndef UPDATE_PUBKEY_HEX
#define UPDATE_PUBKEY_HEX ""
#endif
#ifndef UPDATE_DIR
#define UPDATE_DIR "/userdata/.nuubos-update"
#endif
#ifndef OS_RELEASE
#define OS_RELEASE "/etc/os-release"
#endif
#ifndef BUILD_ID_FILE
#define BUILD_ID_FILE "/usr/share/nuubos/build-id"
#endif
#ifndef BUILD_SERIAL_FILE
#define BUILD_SERIAL_FILE "/usr/share/nuubos/build-serial"
#endif
#ifndef CHANNEL_FILE
#define CHANNEL_FILE "/state/update/channel"
#endif
#ifndef AUTO_FILE
#define AUTO_FILE "/state/update/auto-off"
#endif
#ifndef UPDATE_DEFAULT_CHANNEL
#define UPDATE_DEFAULT_CHANNEL "stable"
#endif
#define MANIFEST_FORMAT 2
/* notes= : the release notes, one line ("\n" = new line, "\\" = \). */
#define NOTES_MAX 4096

static const char *const channels[] = { "stable", "rc", "nightly" };
/* Names before 2026-10-14, still found in CHANNEL_FILE: old -> new. */
static const char *const renamed_channels[][2] = { { "beta", "rc" }, { "alpha", "nightly" } };

static volatile sig_atomic_t cancelled;
static bool as_job;

static void on_term(int sig)
{
	(void)sig;
	cancelled = 1;
}

static int fail(const char *reason)
{
	printf(as_job ? "@error %s\n" : "ERR %s\n", reason);
	return 1;
}

static void trim(char *s)
{
	size_t n = strlen(s);

	while (n && isspace((unsigned char)s[n - 1]))
		s[--n] = '\0';
}

/* key=value from a file into out; false when absent. */
static bool file_value(const char *path, const char *key, char *out, size_t size)
{
	/* Release notes are one escaped line of up to NOTES_MAX bytes. */
	static char line[NOTES_MAX + 64];
	size_t klen = strlen(key);
	FILE *fp = fopen(path, "r");
	bool found = false;

	out[0] = '\0';
	if (!fp)
		return false;
	while (fgets(line, sizeof(line), fp)) {
		trim(line);
		if (!strncmp(line, key, klen) && line[klen] == '=') {
			snprintf(out, size, "%s", line + klen + 1);
			/* os-release values may be quoted. */
			if (out[0] == '"') {
				memmove(out, out + 1, strlen(out));
				if (out[0] && out[strlen(out) - 1] == '"')
					out[strlen(out) - 1] = '\0';
			}
			found = true;
		}
	}
	fclose(fp);
	return found;
}

static void current_version(char *version, size_t vsize, char *build, size_t bsize)
{
	FILE *fp;

	if (!file_value(OS_RELEASE, "VERSION_ID", version, vsize))
		snprintf(version, vsize, "0");
	build[0] = '\0';
	fp = fopen(BUILD_ID_FILE, "r");
	if (fp) {
		if (fgets(build, (int)bsize, fp))
			trim(build);
		fclose(fp);
	}
}

/* Build serial (seconds since the epoch at image build time): releases are
 * ordered by it, never by the version text. */
static long long current_serial(void)
{
	char text[32] = "";
	FILE *fp = fopen(BUILD_SERIAL_FILE, "r");

	if (fp) {
		if (!fgets(text, sizeof(text), fp))
			text[0] = '\0';
		fclose(fp);
	}
	return atoll(text);
}

static bool valid_channel(const char *name)
{
	for (size_t i = 0; i < sizeof(channels) / sizeof(channels[0]); i++)
		if (!strcmp(name, channels[i]))
			return true;
	return false;
}

static void current_channel(char *out, size_t size)
{
	FILE *fp = fopen(CHANNEL_FILE, "r");

	out[0] = '\0';
	if (fp) {
		if (fgets(out, (int)size, fp))
			trim(out);
		fclose(fp);
	}
	for (size_t i = 0; i < sizeof(renamed_channels) / sizeof(renamed_channels[0]); i++)
		if (!strcmp(out, renamed_channels[i][0]))
			snprintf(out, size, "%s", renamed_channels[i][1]);
	if (!valid_channel(out))
		snprintf(out, size, "%s", UPDATE_DEFAULT_CHANNEL);
}

static bool public_key(unsigned char pk[crypto_sign_PUBLICKEYBYTES])
{
	size_t len = 0;

	return strlen(UPDATE_PUBKEY_HEX) == 2 * crypto_sign_PUBLICKEYBYTES &&
	       sodium_hex2bin(pk, crypto_sign_PUBLICKEYBYTES, UPDATE_PUBKEY_HEX,
			      strlen(UPDATE_PUBKEY_HEX), NULL, &len, NULL) == 0 &&
	       len == crypto_sign_PUBLICKEYBYTES;
}

static long read_file(const char *path, unsigned char *buf, size_t size)
{
	FILE *fp = fopen(path, "rb");
	size_t n;

	if (!fp)
		return -1;
	n = fread(buf, 1, size, fp);
	if (ferror(fp) || !feof(fp)) {
		fclose(fp);
		return -1;
	}
	fclose(fp);
	return (long)n;
}

/* The manifest is trusted only when its Ed25519 signature verifies. */
static bool manifest_verified(void)
{
	static unsigned char manifest[16384];
	unsigned char sig[crypto_sign_BYTES + 1];
	unsigned char pk[crypto_sign_PUBLICKEYBYTES];
	long n, s;

	if (!public_key(pk))
		return false;
	n = read_file(UPDATE_DIR "/manifest", manifest, sizeof(manifest));
	s = read_file(UPDATE_DIR "/manifest.sig", sig, sizeof(sig));
	if (n <= 0 || s != crypto_sign_BYTES)
		return false;
	return crypto_sign_verify_detached(sig, manifest, (unsigned long long)n, pk) == 0;
}

/* A resumed request answered with the whole file (200 instead of 206)
 * restarts the file from the beginning instead of appending to it. */
struct sink {
	FILE *fp;
	bool resuming;
	bool restart;
};

static size_t on_header(char *buf, size_t size, size_t n, void *userdata)
{
	struct sink *s = userdata;
	size_t len = size * n;
	int code;

	if (len > 9 && !strncmp(buf, "HTTP/", 5) && sscanf(strchr(buf, ' ') ? strchr(buf, ' ') : buf, "%d", &code) == 1)
		s->restart = s->resuming && code == 200;
	return len;
}

static size_t to_file(char *ptr, size_t size, size_t n, void *userdata)
{
	struct sink *s = userdata;

	if (s->restart) {
		s->restart = false;
		s->resuming = false;
		if (ftruncate(fileno(s->fp), 0) != 0 || fseek(s->fp, 0, SEEK_SET) != 0)
			return 0;
	}
	return fwrite(ptr, size, n, s->fp);
}

static int progress_cb(void *p, curl_off_t dltotal, curl_off_t dlnow, curl_off_t a, curl_off_t b)
{
	static int last = -1;
	long long base = *(long long *)p;
	long long total;
	int percent;

	(void)a;
	(void)b;
	if (cancelled)
		return 1;
	total = base + dltotal;
	if (as_job && dltotal > 0 && total > 0) {
		percent = (int)((base + dlnow) * 100 / total);
		if (percent != last) {
			last = percent;
			printf("@progress %d\n", percent);
			fflush(stdout);
		}
	}
	return 0;
}

/* GET url into path; with resume, append from the current size. */
static long fetch(const char *url, const char *path, bool resume, long timeout)
{
	CURL *curl = curl_easy_init();
	struct stat st;
	long long have = 0;
	long status = -1;
	FILE *fp;
	CURLcode res;
	struct sink sink;
	char range[32];

	if (!curl)
		return -1;
	if (resume && stat(path, &st) == 0)
		have = (long long)st.st_size;
	fp = fopen(path, resume && have ? "ab" : "wb");
	if (!fp) {
		curl_easy_cleanup(curl);
		return -1;
	}
	curl_easy_setopt(curl, CURLOPT_URL, url);
	curl_easy_setopt(curl, CURLOPT_FOLLOWLOCATION, 1L);
	curl_easy_setopt(curl, CURLOPT_USERAGENT, "nuubOS-update");
	sink = (struct sink){ .fp = fp, .resuming = have > 0, .restart = false };
	curl_easy_setopt(curl, CURLOPT_WRITEFUNCTION, to_file);
	curl_easy_setopt(curl, CURLOPT_WRITEDATA, &sink);
	curl_easy_setopt(curl, CURLOPT_HEADERFUNCTION, on_header);
	curl_easy_setopt(curl, CURLOPT_HEADERDATA, &sink);
	curl_easy_setopt(curl, CURLOPT_NOSIGNAL, 1L);
	curl_easy_setopt(curl, CURLOPT_FAILONERROR, 1L);
	curl_easy_setopt(curl, CURLOPT_CONNECTTIMEOUT, 15L);
	if (timeout)
		curl_easy_setopt(curl, CURLOPT_TIMEOUT, timeout);
	/* A stalled connection fails instead of hanging. */
	curl_easy_setopt(curl, CURLOPT_LOW_SPEED_LIMIT, 1L);
	curl_easy_setopt(curl, CURLOPT_LOW_SPEED_TIME, 60L);
	/* An explicit range (not RESUME_FROM): a server that ignores it answers
	 * 200 with the whole file, which the sink restarts from zero. */
	if (have) {
		snprintf(range, sizeof(range), "%lld-", have);
		curl_easy_setopt(curl, CURLOPT_RANGE, range);
	}
	curl_easy_setopt(curl, CURLOPT_NOPROGRESS, 0L);
	curl_easy_setopt(curl, CURLOPT_XFERINFOFUNCTION, progress_cb);
	curl_easy_setopt(curl, CURLOPT_XFERINFODATA, &have);
	res = curl_easy_perform(curl);
	if (res == CURLE_OK)
		curl_easy_getinfo(curl, CURLINFO_RESPONSE_CODE, &status);
	else if (res == CURLE_HTTP_RETURNED_ERROR)
		curl_easy_getinfo(curl, CURLINFO_RESPONSE_CODE, &status);
	curl_easy_cleanup(curl);
	if (fclose(fp) != 0)
		return -1;
	return status;
}

static bool sha256_matches(const char *path, const char *expected_hex, long long expected_size)
{
	crypto_hash_sha256_state st;
	unsigned char digest[crypto_hash_sha256_BYTES];
	char hex[2 * crypto_hash_sha256_BYTES + 1];
	static unsigned char buf[1 << 20];
	long long total = 0;
	size_t n;
	FILE *fp = fopen(path, "rb");

	if (!fp)
		return false;
	crypto_hash_sha256_init(&st);
	while ((n = fread(buf, 1, sizeof(buf), fp)) > 0 && !cancelled) {
		crypto_hash_sha256_update(&st, buf, n);
		total += (long long)n;
	}
	fclose(fp);
	if (cancelled || total != expected_size)
		return false;
	crypto_hash_sha256_final(&st, digest);
	sodium_bin2hex(hex, sizeof(hex), digest, sizeof(digest));
	return !strcasecmp(hex, expected_hex);
}

static bool safe_name(const char *s)
{
	if (!s[0] || strlen(s) > 128)
		return false;
	for (; *s; s++)
		if (!isalnum((unsigned char)*s) && *s != '.' && *s != '-' && *s != '_')
			return false;
	return true;
}

/* "<tag>/<file>": two plain names. */
static bool safe_payload(const char *s)
{
	char tag[160];
	const char *slash = strchr(s, '/');

	if (!slash || strchr(slash + 1, '/') || (size_t)(slash - s) >= sizeof(tag))
		return false;
	snprintf(tag, sizeof(tag), "%.*s", (int)(slash - s), s);
	return safe_name(tag) && safe_name(slash + 1) && strcmp(tag, "..") && strcmp(slash + 1, "..");
}

/* A verified manifest of a format this build understands, for the channel
 * in use, newer than the running build. */
static bool manifest_offers_update(void)
{
	char format[16], channel[32], wanted[32], serial[32];

	file_value(UPDATE_DIR "/manifest", "format", format, sizeof(format));
	file_value(UPDATE_DIR "/manifest", "channel", channel, sizeof(channel));
	file_value(UPDATE_DIR "/manifest", "serial", serial, sizeof(serial));
	current_channel(wanted, sizeof(wanted));
	return atoi(format) == MANIFEST_FORMAT && !strcmp(channel, wanted) &&
	       atoll(serial) > current_serial();
}

/* No answer at all (DNS, connection, timeout) = the network; an HTTP error
 * (GitHub rate limit, 5xx) = the update service. */
static const char *http_failure(long status)
{
	return status > 0 ? "server" : "network";
}

static int cmd_check(void)
{
	unsigned char pk[crypto_sign_PUBLICKEYBYTES];
	long status;
	char url[1024], channel[32], version[64];

	if (!public_key(pk))
		return fail("unavailable");
	if (mkdir(UPDATE_DIR, 0700) != 0 && errno != EEXIST)
		return fail("storage");
	current_channel(channel, sizeof(channel));
	snprintf(url, sizeof(url), "%s/channel-%s/nuubos-update.manifest", UPDATE_URL, channel);
	status = fetch(url, UPDATE_DIR "/manifest.new", false, 30);
	/* Nothing published on this channel yet: nothing to offer. */
	if (status == 404) {
		unlink(UPDATE_DIR "/manifest.new");
		puts("OK none");
		return 0;
	}
	if (status != 200)
		return fail(http_failure(status));
	snprintf(url, sizeof(url), "%s/channel-%s/nuubos-update.manifest.sig", UPDATE_URL, channel);
	status = fetch(url, UPDATE_DIR "/manifest.sig.new", false, 30);
	if (status != 200)
		return fail(http_failure(status));
	/* Keep a previous good manifest until the new one verifies. */
	rename(UPDATE_DIR "/manifest", UPDATE_DIR "/manifest.old");
	rename(UPDATE_DIR "/manifest.sig", UPDATE_DIR "/manifest.sig.old");
	rename(UPDATE_DIR "/manifest.new", UPDATE_DIR "/manifest");
	rename(UPDATE_DIR "/manifest.sig.new", UPDATE_DIR "/manifest.sig");
	if (!manifest_verified()) {
		rename(UPDATE_DIR "/manifest.old", UPDATE_DIR "/manifest");
		rename(UPDATE_DIR "/manifest.sig.old", UPDATE_DIR "/manifest.sig");
		return fail("signature");
	}
	unlink(UPDATE_DIR "/manifest.old");
	unlink(UPDATE_DIR "/manifest.sig.old");
	file_value(UPDATE_DIR "/manifest", "version", version, sizeof(version));
	if (manifest_offers_update())
		printf("OK available %s\n", version);
	else
		printf("OK current\n");
	return 0;
}

/* Free space on USERDATA for what is still missing of the payload. */
static bool room_for(long long size)
{
	struct statvfs vfs;
	struct stat st;
	long long have = 0;

	if (stat(UPDATE_DIR "/payload.part", &st) == 0)
		have = (long long)st.st_size;
	if (statvfs(UPDATE_DIR, &vfs) != 0)
		return true;
	return (long long)vfs.f_bavail * (long long)vfs.f_frsize > size - have + (64LL << 20);
}

static int cmd_download(void)
{
	char url[1024], payload[160], sha[80], size_text[32];
	long long size;
	long status;

	if (!manifest_verified())
		return fail("signature");
	if (!manifest_offers_update())
		return fail("current");
	if (!file_value(UPDATE_DIR "/manifest", "payload", payload, sizeof(payload)) || !safe_payload(payload) ||
	    !file_value(UPDATE_DIR "/manifest", "sha256", sha, sizeof(sha)) ||
	    !file_value(UPDATE_DIR "/manifest", "size", size_text, sizeof(size_text)))
		return fail("manifest");
	size = atoll(size_text);
	if (size <= 0)
		return fail("manifest");
	if (access(UPDATE_DIR "/payload", F_OK) == 0 && sha256_matches(UPDATE_DIR "/payload", sha, size)) {
		printf(as_job ? "@result downloaded\n" : "OK downloaded\n");
		return 0;
	}
	unlink(UPDATE_DIR "/payload");
	if (!room_for(size))
		return fail("space");
	snprintf(url, sizeof(url), "%s/%s", UPDATE_URL, payload);
	status = fetch(url, UPDATE_DIR "/payload.part", true, 0);
	if (cancelled)
		return fail("cancelled");
	/* 206 partial content or 200 for a fresh download; 416 means the
	 * part already holds everything. */
	if (status != 200 && status != 206 && status != 416)
		return fail(http_failure(status));
	if (!sha256_matches(UPDATE_DIR "/payload.part", sha, size)) {
		/* A corrupt or tampered part is never kept for resume. */
		unlink(UPDATE_DIR "/payload.part");
		return fail(cancelled ? "cancelled" : "checksum");
	}
	if (rename(UPDATE_DIR "/payload.part", UPDATE_DIR "/payload") != 0)
		return fail("storage");
	sync();
	printf(as_job ? "@result downloaded\n" : "OK downloaded\n");
	return 0;
}

static int cmd_verify(void)
{
	char sha[80], size_text[32];

	if (!manifest_verified())
		return fail("signature");
	if (!file_value(UPDATE_DIR "/manifest", "sha256", sha, sizeof(sha)) ||
	    !file_value(UPDATE_DIR "/manifest", "size", size_text, sizeof(size_text)))
		return fail("manifest");
	if (!sha256_matches(UPDATE_DIR "/payload", sha, atoll(size_text)))
		return fail("checksum");
	puts("OK");
	return 0;
}

static int cmd_status(void)
{
	unsigned char pk[crypto_sign_PUBLICKEYBYTES];
	static char notes[NOTES_MAX + 1];
	char cur[64], build[128], channel[32], version[64] = "";
	bool available = manifest_verified() && manifest_offers_update();

	current_version(cur, sizeof(cur), build, sizeof(build));
	current_channel(channel, sizeof(channel));
	if (available) {
		file_value(UPDATE_DIR "/manifest", "version", version, sizeof(version));
		file_value(UPDATE_DIR "/manifest", "notes", notes, sizeof(notes));
	}
	printf("provider=%s\nchannel=%s\nauto=%d\ncurrent=%s\nbuild=%s\navailable=%d\nversion=%s\nnotes=%s\ndownloaded=%d\nend=1\n",
	       public_key(pk) ? "available" : "unavailable", channel, access(AUTO_FILE, F_OK) != 0, cur, build, available,
	       version, notes, available && access(UPDATE_DIR "/payload", F_OK) == 0);
	return 0;
}

/* A new channel forgets what the old one offered. */
static int cmd_channel(const char *name)
{
	char current[32];
	FILE *fp;

	current_channel(current, sizeof(current));
	if (!name) {
		puts(current);
		return 0;
	}
	if (!valid_channel(name))
		return fail("channel");
	if (!strcmp(name, current)) {
		puts("OK");
		return 0;
	}
	fp = fopen(CHANNEL_FILE ".tmp", "w");
	if (!fp)
		return fail("storage");
	if (fprintf(fp, "%s\n", name) < 0 || fflush(fp) != 0 || fsync(fileno(fp)) != 0) {
		fclose(fp);
		unlink(CHANNEL_FILE ".tmp");
		return fail("storage");
	}
	fclose(fp);
	if (rename(CHANNEL_FILE ".tmp", CHANNEL_FILE) != 0)
		return fail("storage");
	unlink(UPDATE_DIR "/manifest");
	unlink(UPDATE_DIR "/manifest.sig");
	unlink(UPDATE_DIR "/payload");
	unlink(UPDATE_DIR "/payload.part");
	puts("OK");
	return 0;
}

/* The daily check is on unless AUTO_FILE exists. */
static int cmd_auto(const char *value)
{
	if (!value) {
		puts(access(AUTO_FILE, F_OK) != 0 ? "on" : "off");
		return 0;
	}
	if (!strcmp(value, "on")) {
		if (unlink(AUTO_FILE) != 0 && errno != ENOENT)
			return fail("storage");
	} else if (!strcmp(value, "off")) {
		int fd = open(AUTO_FILE, O_WRONLY | O_CREAT | O_CLOEXEC, 0644);

		if (fd < 0)
			return fail("storage");
		close(fd);
	} else {
		return fail("value");
	}
	puts("OK");
	return 0;
}

static void progress(int percent)
{
	static int last = -1;

	if (as_job && percent != last) {
		last = percent;
		printf("@progress %d\n", percent);
		fflush(stdout);
	}
}

/* Hash the first size bytes of fd: the image as it landed on the card. */
static bool read_back(int fd, long long size, const char *expected_hex)
{
	crypto_hash_sha256_state st;
	unsigned char digest[crypto_hash_sha256_BYTES];
	char hex[2 * crypto_hash_sha256_BYTES + 1];
	static unsigned char buf[1 << 20];
	long long done = 0;

	crypto_hash_sha256_init(&st);
	while (done < size && !cancelled) {
		size_t want = size - done < (long long)sizeof(buf) ? (size_t)(size - done) : sizeof(buf);
		ssize_t n = read(fd, buf, want);

		if (n <= 0)
			return false;
		crypto_hash_sha256_update(&st, buf, (size_t)n);
		done += n;
		progress(80 + (int)(done * 20 / size));
	}
	if (cancelled)
		return false;
	crypto_hash_sha256_final(&st, digest);
	sodium_bin2hex(hex, sizeof(hex), digest, sizeof(digest));
	return !strcasecmp(hex, expected_hex);
}

static int cmd_write(const char *device)
{
	char sha[80], size_text[32], image_sha[80], image_text[32];
	crypto_hash_sha256_state st;
	unsigned char digest[crypto_hash_sha256_BYTES];
	char hex[2 * crypto_hash_sha256_BYTES + 1];
	static unsigned char buf[1 << 20];
	long long image_size, done = 0;
	struct stat dst;
	gzFile gz;
	int fd, n;

	if (!manifest_verified())
		return fail("signature");
	if (!file_value(UPDATE_DIR "/manifest", "sha256", sha, sizeof(sha)) ||
	    !file_value(UPDATE_DIR "/manifest", "size", size_text, sizeof(size_text)) ||
	    !file_value(UPDATE_DIR "/manifest", "image_sha256", image_sha, sizeof(image_sha)) ||
	    !file_value(UPDATE_DIR "/manifest", "image_size", image_text, sizeof(image_text)))
		return fail("manifest");
	image_size = atoll(image_text);
	if (image_size <= 0)
		return fail("manifest");
	progress(-1);
	if (!sha256_matches(UPDATE_DIR "/payload", sha, atoll(size_text)))
		return fail(cancelled ? "cancelled" : "checksum");
	fd = open(device, O_RDWR | O_CLOEXEC);
	if (fd < 0 || fstat(fd, &dst) != 0)
		return fail("slot");
	if (S_ISBLK(dst.st_mode)) {
		unsigned long long bytes = 0;

		if (ioctl(fd, BLKGETSIZE64, &bytes) != 0 || (long long)bytes < image_size) {
			close(fd);
			return fail("slot");
		}
	}
	gz = gzopen(UPDATE_DIR "/payload", "rb");
	if (!gz) {
		close(fd);
		return fail("storage");
	}
	gzbuffer(gz, 1 << 17);
	crypto_hash_sha256_init(&st);
	progress(0);
	while (!cancelled && (n = gzread(gz, buf, sizeof(buf))) > 0) {
		if (done + n > image_size)
			break;
		for (ssize_t off = 0; off < n;) {
			ssize_t w = write(fd, buf + off, (size_t)(n - off));

			if (w <= 0) {
				gzclose(gz);
				close(fd);
				return fail("write");
			}
			off += w;
		}
		crypto_hash_sha256_update(&st, buf, (size_t)n);
		done += n;
		progress((int)(done * 80 / image_size));
	}
	gzclose(gz);
	crypto_hash_sha256_final(&st, digest);
	sodium_bin2hex(hex, sizeof(hex), digest, sizeof(digest));
	if (cancelled || done != image_size || strcasecmp(hex, image_sha)) {
		close(fd);
		return fail(cancelled ? "cancelled" : "checksum");
	}
	/* Read back from the card, not from the page cache. */
	if (fsync(fd) != 0) {
		close(fd);
		return fail("write");
	}
	if (S_ISBLK(dst.st_mode))
		ioctl(fd, BLKFLSBUF, 0);
	posix_fadvise(fd, 0, 0, POSIX_FADV_DONTNEED);
	if (lseek(fd, 0, SEEK_SET) != 0 || !read_back(fd, image_size, image_sha)) {
		close(fd);
		return fail(cancelled ? "cancelled" : "verify");
	}
	close(fd);
	printf(as_job ? "@result written\n" : "OK written\n");
	return 0;
}

int main(int argc, char **argv)
{
	struct sigaction sa;
	int rc = 2;

	memset(&sa, 0, sizeof(sa));
	sa.sa_handler = on_term;
	sigaction(SIGTERM, &sa, NULL);
	sigaction(SIGINT, &sa, NULL);
	as_job = getenv("NUUBOS_JOB_ID") != NULL;
	if (sodium_init() < 0)
		return fail("crypto");
	curl_global_init(CURL_GLOBAL_DEFAULT);
	if (argc == 2 && !strcmp(argv[1], "status"))
		rc = cmd_status();
	else if (argc == 2 && !strcmp(argv[1], "check"))
		rc = cmd_check();
	else if (argc == 2 && !strcmp(argv[1], "download"))
		rc = cmd_download();
	else if (argc == 2 && !strcmp(argv[1], "verify"))
		rc = cmd_verify();
	else if ((argc == 2 || argc == 3) && !strcmp(argv[1], "channel"))
		rc = cmd_channel(argc == 3 ? argv[2] : NULL);
	else if ((argc == 2 || argc == 3) && !strcmp(argv[1], "auto"))
		rc = cmd_auto(argc == 3 ? argv[2] : NULL);
	else if (argc == 3 && !strcmp(argv[1], "write"))
		rc = cmd_write(argv[2]);
	else
		fprintf(stderr, "Usage: nuubos-updatectl status | channel [stable|rc|nightly] | auto [on|off] | check | download | verify | write DEVICE\n");
	curl_global_cleanup();
	return rc;
}
