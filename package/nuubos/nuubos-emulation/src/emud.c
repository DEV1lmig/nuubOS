/* SPDX-License-Identifier: MIT */
/*
 * nuubos-emud — Emulation Service (EPIC-013 RetroArch Integration,
 * EPIC-019 Saves & Save States, EPIC-020 sessions, EPIC-005 game actions).
 *
 * Owns the running game session. A Library game is launched in RetroArch
 * (the emulation backend) with:
 *   - the qualified core for its system (cores.conf: bundled cores first,
 *     then cores the user downloaded with the RetroArch Online Updater);
 *   - the active user's RetroArch configuration, seeded once from the
 *     nuubOS H700 defaults and freely editable in RetroArch Advanced;
 *   - a session configuration written at every launch with what nuubOS
 *     must control: per-user/per-system save and state directories, shared
 *     BIOS and cores, drivers, the nuubOS game gamepads, the stdin command
 *     interface and the UI language.
 *
 * The session is controlled through RetroArch's stdin command interface
 * (never the UDP network interface). Every client (Quick Menu, controller
 * hotkeys, lifecycle hooks) uses this service, so pause, save/load state,
 * slot, reset, quit and RetroArch Advanced have a single implementation.
 * Controllers reach the game only through nuubos-inputd game gamepads,
 * switched on for the lifetime of the session connection.
 *
 * Protocol (/run/nuubos/emud.sock, one command per line, tab separated
 * arguments): STATUS and SUBSCRIBE return the session snapshot (SUBSCRIBE
 * pushes a new one on every change); LAUNCH <game> [resume|new|slot:N],
 * STATES [game], SETTINGS, SET <key> <value>, PAUSE, RESUME,
 * SAVE_STATE, LOAD_STATE, SLOT <n|+1|-1>, RESET, FAST_FORWARD, ADVANCED,
 * QUIT, SCREENSHOT, OVERLAY_TEXT <line> reply OK or ERR <reason>; PRE_POWER <sleep|restart|poweroff>
 * replies once the game state is safe. STORAGE_LOST <mount point> (from
 * nuubos-storagectl when the games card is pulled out): a game whose ROM
 * was on it is saved in the automatic state and closed, with a notice.
 *
 * LAUNCH takes a third argument for netplay on the local network: "host",
 * or "connect:<address>:<port>:<core>" (a host found by NETPLAY_HOSTS, the
 * LAN discovery of RetroArch hosts, with the host's emulator). Over a
 * running game NETPLAY_HOST restarts it as a host from its saved progress,
 * NETPLAY_STOP ends netplay (the game goes on); the snapshot follows
 * RetroArch's netplay events (netplay=off|starting|host|client,
 * netplay_peers=, netplay_peer=). CHEATS lists the running game's cheat
 * lists and cheats, CHEAT_SET <k> <0|1> and CHEAT_FILE <path> change them
 * (kept per user and game, applied at once; never in hardcore mode or with
 * netplay). Cores the user downloaded for a system (its libretro databases,
 * libretro.conf) are emulators of that system like the cores.conf ones.
 *
 * Per-game configuration (EPIC-018): GAME_SETTINGS <game> <system> lists the
 * effective values and the user's explicit overrides; GAME_SET <game>
 * <system> <key> <value> (empty value = inherit) and GAME_RESET <game>
 * change them. Only overrides are stored, per user, in
 * /userdata/users/<id>/appdata/nuubos-emulation/games/<game>.conf. They
 * apply at the next launch: core choice, aspect ratio and video filter on
 * top of the user's RetroArch configuration. RetroArch saves its whole
 * configuration on exit, so the keys a session overrode are put back to
 * the user's own values when it ends.
 *
 * RetroAchievements (EPIC-017): a user logged in with
 * nuubos-achievementsctl has /state/users/<id>/secrets/retroachievements.conf
 * (user name and API token, never the password); the session enables
 * RetroArch's achievements with that token, and hardcore mode when chosen.
 * In hardcore mode (user request 2026-10-11) the session has no save
 * states at all: SAVE_STATE, LOAD_STATE, SLOT and FAST_FORWARD (and their
 * hotkeys) are refused with game.hardcore, the game starts from the
 * beginning and nothing is saved when it closes. Outcomes the user must
 * see (state saved, empty slot, launch failure...) are typed notifications
 * (EPIC-006).
 *
 * Idle: poll() without timeout. A timeout is armed only while a save or a
 * quit is pending.
 */

#include <arpa/inet.h>
#include <ctype.h>
#include <dirent.h>
#include <errno.h>
#include <fcntl.h>
#include <limits.h>
#include <netinet/in.h>
#include <poll.h>
#include <signal.h>
#include <stdarg.h>
#include <stdbool.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <strings.h>
#include <sys/inotify.h>
#include <sys/socket.h>
#include <sys/stat.h>
#include <sys/types.h>
#include <sys/un.h>
#include <sys/wait.h>
#include <time.h>
#include <unistd.h>

#include <nuubos/notify.h>

/* Roots are overridable at build time for host tests. */
#ifndef RUN_ROOT
#define RUN_ROOT "/run/nuubos"
#endif
#define SOCKET_PATH RUN_ROOT "/emud.sock"
#define PIDFILE RUN_ROOT "/emud.pid"
#define SESSION_DIR RUN_ROOT "/emulation"
#define SESSION_CFG SESSION_DIR "/session.cfg"
#define SESSION_SHADER SESSION_DIR "/session.glslp"
#define RETROARCH_LOG RUN_ROOT "/retroarch.log"
#define LIBRARY_SOCKET RUN_ROOT "/libraryd.sock"
#define INPUT_SOCKET RUN_ROOT "/inputd.sock"
#define ACTIVE_USER_FILE RUN_ROOT "/user/active"
#define WAYLAND_RUNTIME RUN_ROOT "/wayland-runtime"
#define PIPEWIRE_RUNTIME RUN_ROOT "/pipewire"

#ifndef RETROARCH_BIN
#define RETROARCH_BIN "/usr/bin/retroarch"
#endif
#ifndef SHARE_DIR
#define SHARE_DIR "/usr/share/nuubos/emulation"
#endif
#define CORES_CONF SHARE_DIR "/cores.conf"
/* nuubOS systems in the libretro databases (core-info "database"). */
#define LIBRETRO_CONF SHARE_DIR "/libretro.conf"
#define DEFAULT_RA_CFG SHARE_DIR "/retroarch.cfg"
#define DEFAULT_CORE_OPTIONS SHARE_DIR "/retroarch-core-options.cfg"
#define CORE_OPTION_UPDATES SHARE_DIR "/core-option-updates.conf"
#define DEFAULT_SHADER_PRESET SHARE_DIR "/shaders/global.glslp"
#define SHADER_DIR SHARE_DIR "/shaders"
/* Performance overlay font (RetroArch patch 0004 draws with the OSD font). */
#define OSD_FONT "/usr/share/fonts/cantarell/Cantarell-Bold.otf"
#define DISPLAY_CONF SHARE_DIR "/display.conf"
/* Game render height on HDMI: RetroArch (patch 0005) draws a buffer of
 * this many lines and the display engine scales it to the 1080p mode on
 * its plane (kernel patch 0031), so the frontend's per-pixel cost stays
 * near the handheld panel's (N64 at a 1080p buffer: ~1/3 of full speed).
 * 360 keeps Majora's Mask PAL at full speed; 240p content then fits at
 * 1.5x, below the CRT look's 2x scanlines. display.conf field 3 overrides
 * it per system (0 = native resolution). The panel never uses it. */
#define GAME_HDMI_LINES 360
#ifndef HDMI_ENABLED
#define HDMI_ENABLED "/sys/class/drm/card0-HDMI-A-1/enabled"
#endif
#ifndef DRM_CLASS
#define DRM_CLASS "/sys/class/drm"
#endif
#ifndef SYSTEM_ICONS
#define SYSTEM_ICONS "/usr/share/nuubos/systems"
#endif
#ifndef SYSTEMS_CONF
#define SYSTEMS_CONF "/usr/share/nuubos/systems.conf"
#endif
/* Support files some cores load from the system directory (PPSSPP fonts
 * and UI atlas, ScummVM engine data and themes). */
#define SYSTEM_FILES SHARE_DIR "/system"
#ifndef BUNDLED_CORES
#define BUNDLED_CORES "/usr/lib/libretro"
#endif
#ifndef CORE_INFO_DIR
#define CORE_INFO_DIR "/usr/share/libretro/info"
#endif
#define RA_ASSETS_DIR "/usr/share/retroarch/assets"
#define RA_AUTOCONFIG_DIR "/usr/share/retroarch/autoconfig"

#ifndef USERDATA
#define USERDATA "/userdata"
#endif
#define DOWNLOADED_CORES USERDATA "/retroarch/cores"
#define BIOS_DIR USERDATA "/bios"
#define CHEATS_DIR USERDATA "/cheats"
/* The libretro cheat collection (nuubos-libretroctl cheats update): an
 * archive with an index of its files; the files of a game played are
 * extracted into one folder per database. */
#define CHEATS_LIBRETRO CHEATS_DIR "/libretro"
#define CHEATS_ARCHIVE CHEATS_LIBRETRO "/cheats.zip"
#define CHEATS_INDEX CHEATS_LIBRETRO "/index.txt"
#define SESSION_CHEATS SESSION_DIR "/session.cht"
#define CACHE_DIR USERDATA "/cache/retroarch"
#define USERS_DIR USERDATA "/users"
#ifndef STATE_USERS
#define STATE_USERS "/state/users"
#endif
#define GAME_SETTINGS_SUBDIR "appdata/nuubos-emulation/games"
#define EMU_SETTINGS_FILE "appdata/nuubos-emulation/settings.conf"
#define ACHIEVEMENTS_FILE "secrets/retroachievements.conf"
#define PROFILE_FILE "profile.conf"

#define MAX_CLIENTS 16
#define MAX_LINE 1024
#define MAX_PLAYERS 8
/* Save state slots per game: user setting (default 10, at most 99). */
#define DEFAULT_SLOTS 10
#define MAX_SLOTS 99
/* RetroArch's automatic slot (.state.auto), used before power actions. */
#define AUTO_SLOT -1
#define SAVE_TIMEOUT_MS 10000
#define QUIT_TIMEOUT_MS 10000
#define KILL_TIMEOUT_MS 4000
/* A session ending this early with an error never really started. */
#define START_FAILURE_MS 5000
#define LIBRARY_TIMEOUT_MS 3000
#define SCREENSHOT_TIMEOUT_MS 5000
#define MAX_RESTORE_KEYS 48
/* Performance overlay: RetroArch frame rate sampled once a second. */
#define OVERLAY_TICK_MS 1000
/* Netplay (RetroArch's protocol): TCP game port and LAN discovery port,
 * how long a discovery listens for hosts. */
#define NETPLAY_PORT 55435
#define DISCOVERY_MS 1500
/* Cheat lists offered for a game (its own files, the collection's). */
#define MAX_CHEAT_LISTS 12

enum session_state {
	SESSION_IDLE = 0,
	SESSION_RUNNING,
	SESSION_EXITING,
};

enum pending_kind {
	PENDING_NONE = 0,
	PENDING_SAVE,
	PENDING_LOAD,
};

/*
 * The user's emulation settings (Settings -> Gaming), stored in
 * /userdata/users/<id>/appdata/nuubos-emulation/settings.conf:
 *   boot_screens  start with the console's own boot sequence (BIOS logo)
 *   slots         save state slots per game
 *   slot_rotation each save goes to the next slot, wrapping to 0
 *   bezels, crt   picture presentation (bezel.glsl, crt.glsl)
 *   overlay       performance overlay over games, overlay_items its fields
 *   hotkey_<action> chord control, held with the Quick Menu button
 */
enum hotkey_index {
	HK_SAVE,
	HK_LOAD,
	HK_SLOT_NEXT,
	HK_SLOT_PREV,
	HK_FAST_FORWARD,
	HK_QUIT,
	HK_SCREENSHOT,
	HK_COUNT,
};

static const char *const hotkey_actions[HK_COUNT] = {
	"save_state", "load_state", "slot_next", "slot_prev",
	"fast_forward", "quit", "screenshot",
};

/* nuubos-controllersd control ids (the user's mapping applies). */
static const char *const hotkey_defaults[HK_COUNT] = {
	"r1", "l1", "menu_up", "menu_down", "face_north", "settings", "l2",
};

static const char *const hotkey_controls[] = {
	"menu_confirm", "menu_back", "face_north", "face_west",
	"menu_up", "menu_down", "menu_left", "menu_right",
	"l1", "r1", "l2", "r2", "l3", "r3", "settings", "select", NULL,
};

static const char *const overlay_fields[] = {
	"fps", "cpu", "gpu", "ram", "temp", "power", "battery", "clock", NULL,
};
#define DEFAULT_OVERLAY_ITEMS "fps,cpu,ram,temp,battery,clock"

struct emu_settings {
	bool boot_screens;
	int slots;
	bool slot_rotation;
	bool bezels;
	bool crt;
	bool overlay;
	char overlay_items[96];
	char hotkey[HK_COUNT][16];
};

struct client {
	int fd;
	bool subscribed;
	char buf[MAX_LINE];
	size_t used;
};

/* Netplay of the session, from RetroArch's events (patch 0008). */
enum netplay_mode {
	NET_OFF = 0,
	NET_HOST,
	NET_CLIENT,
};

struct session {
	enum session_state state;
	pid_t pid;
	int stdin_fd;
	int stdout_fd;
	int input_fd;
	char out_buf[MAX_LINE];
	size_t out_used;
	char user[64];
	char game[32];
	char system[64];
	char core[64];
	char content[PATH_MAX];
	/* The ROM file itself (content may be a generated MAME .cmd). */
	char rom[PATH_MAX];
	char states_dir[PATH_MAX];
	char state_base[PATH_MAX];
	int slot;
	bool paused;
	bool fast_forward;
	/* RetroAchievements hardcore: no save states, no fast-forward. */
	bool hardcore;
	long long started_ms;
	long long quit_deadline_ms;
	bool term_sent;
	bool kill_sent;
	/* One state operation at a time: RetroArch replies once the state is
	 * serialized; a save is complete when its file is closed. */
	enum pending_kind pending;
	int pending_slot;
	bool pending_replied;
	bool pending_quiet;
	char pending_name[NAME_MAX + 1];
	long long pending_deadline_ms;
	int inotify_fd;
	int inotify_wd;
	char input_buf[128];
	size_t input_used;
	/* Slot rotation: the progress of a closing game goes into the next
	 * slot, saved by emud before RetroArch is asked to quit. */
	bool quit_requested;
	bool quit_saved;
	/* A lifecycle hook waiting for PRE_POWER completion. */
	int power_client;
	char power_action[16];
	/* Screenshot confirmation: a new PNG closed in the screenshot folder. */
	int shot_fd;
	long long shot_deadline_ms;
	char screenshots_dir[PATH_MAX];
	/* User RetroArch keys this session overrode, restored at exit
	 * (value NULL = the key was absent). */
	char ra_cfg[PATH_MAX];
	int restore_count;
	char restore_key[MAX_RESTORE_KEYS][48];
	char *restore_value[MAX_RESTORE_KEYS];
	/* The user's settings when the game started (hotkeys and overlay
	 * follow later changes). */
	struct emu_settings settings;
	/* Performance overlay: last RetroArch frame rate, next sample. */
	double fps;
	long long overlay_tick_ms;
	/* Cheats turned on in the session (0 = none loaded). */
	int cheats_on;
	/* Netplay: what the launch asked for ("host", "connect"), the state
	 * RetroArch reports, the players connected to this host, the other
	 * side's nickname; host again once the game has closed (Host Game
	 * over a running game restarts it as a host from its saved
	 * progress). */
	char net_launch[16];
	enum netplay_mode netplay;
	int net_peers;
	char net_peer[40];
	bool relaunch_host;
};

/* LAN discovery of netplay hosts (NETPLAY_HOSTS): one at a time, every
 * client asking meanwhile gets the same answer. */
struct discovery {
	int fd;
	long long deadline_ms;
	bool waiting[MAX_CLIENTS];
	int count;
	struct {
		char address[INET_ADDRSTRLEN];
		int port;
		bool password;
		char nick[32];
		char core[32];
		char content[256];
	} hosts[16];
};

/* Explicit per-game overrides; empty = inherited. The cheats are the
 * user's choice for the game: the list (a .cht file) and the indices of
 * the cheats turned on in it ("0,4,7"). */
struct game_overrides {
	char core[64];
	char aspect[16];
	char filter[16];
	char cheat_file[PATH_MAX];
	char cheats[512];
};


static volatile sig_atomic_t stop_requested;
static int sigchld_pipe[2] = { -1, -1 };
static struct client clients[MAX_CLIENTS];
static struct session session;
static struct discovery discovery = { .fd = -1 };

static long long monotonic_ms(void)
{
	struct timespec ts;

	clock_gettime(CLOCK_MONOTONIC, &ts);
	return (long long)ts.tv_sec * 1000LL + ts.tv_nsec / 1000000LL;
}

static void log_msg(const char *fmt, ...)
{
	va_list ap;

	va_start(ap, fmt);
	vfprintf(stderr, fmt, ap);
	va_end(ap);
	fputc('\n', stderr);
	fflush(stderr);
}

static void handle_stop(int sig)
{
	(void)sig;
	stop_requested = 1;
}

static void handle_sigchld(int sig)
{
	int saved = errno;

	(void)sig;
	if (sigchld_pipe[1] >= 0 && write(sigchld_pipe[1], "c", 1) < 0) {
		/* Pipe full: a wakeup is already pending. */
	}
	errno = saved;
}

static void copy_text(char *dst, size_t size, const char *src)
{
	snprintf(dst, size, "%s", src ? src : "");
}

static void trim(char *text)
{
	size_t len = strlen(text);

	while (len > 0 && (text[len - 1] == '\n' || text[len - 1] == '\r' ||
			   isspace((unsigned char)text[len - 1])))
		text[--len] = '\0';
}

static int write_all(int fd, const char *data, size_t len)
{
	while (len > 0) {
		ssize_t n = send(fd, data, len, MSG_NOSIGNAL);

		if (n < 0 && errno == ENOTSOCK)
			n = write(fd, data, len);
		if (n < 0) {
			if (errno == EINTR)
				continue;
			return -1;
		}
		data += n;
		len -= (size_t)n;
	}
	return 0;
}

static int mkdir_p(const char *path)
{
	char tmp[PATH_MAX];
	char *p;

	copy_text(tmp, sizeof(tmp), path);
	for (p = tmp + 1; *p; p++) {
		if (*p != '/')
			continue;
		*p = '\0';
		if (mkdir(tmp, 0755) != 0 && errno != EEXIST)
			return -1;
		*p = '/';
	}
	if (mkdir(tmp, 0755) != 0 && errno != EEXIST)
		return -1;
	return 0;
}

static bool file_exists(const char *path)
{
	struct stat st;

	return stat(path, &st) == 0 && S_ISREG(st.st_mode);
}

/* Copies a nuubOS default into the user's configuration once; the user's
 * own changes (RetroArch Advanced) are never overwritten. */
static int seed_file(const char *src, const char *dst)
{
	char tmp[PATH_MAX];
	char buf[8192];
	int in;
	int out;
	ssize_t n;
	int rc = 0;

	if (file_exists(dst))
		return 0;
	in = open(src, O_RDONLY | O_CLOEXEC);
	if (in < 0)
		return -1;
	snprintf(tmp, sizeof(tmp), "%s.tmp", dst);
	out = open(tmp, O_WRONLY | O_CREAT | O_TRUNC | O_CLOEXEC, 0644);
	if (out < 0) {
		close(in);
		return -1;
	}
	while ((n = read(in, buf, sizeof(buf))) > 0) {
		if (write(out, buf, (size_t)n) != n) {
			rc = -1;
			break;
		}
	}
	if (n < 0 || fsync(out) != 0)
		rc = -1;
	close(in);
	if (close(out) != 0)
		rc = -1;
	if (rc == 0 && rename(tmp, dst) != 0)
		rc = -1;
	if (rc != 0)
		unlink(tmp);
	return rc;
}

/* Appends to dst the "key = value" lines of src whose key dst does not
 * have yet: options of cores added by a newer image reach existing users,
 * values the user already has are never changed. */
static void merge_missing_keys(const char *src, const char *dst)
{
	char line[512];
	char key[128];
	FILE *in = fopen(src, "r");
	FILE *cur;
	FILE *out;

	if (!in)
		return;
	while (fgets(line, sizeof(line), in)) {
		bool present = false;
		char other[512];

		if (sscanf(line, " %127[^ =#] =", key) != 1)
			continue;
		cur = fopen(dst, "r");
		while (cur && !present && fgets(other, sizeof(other), cur)) {
			char k[128];

			present = sscanf(other, " %127[^ =#] =", k) == 1 && !strcmp(k, key);
		}
		if (cur)
			fclose(cur);
		if (present)
			continue;
		out = fopen(dst, "a");
		if (!out)
			break;
		fputs(line, out);
		fclose(out);
	}
	fclose(in);
}

/* Sets key to new_value in the core options file at path when it holds
 * old_value; true when the file was readable. */
static bool update_core_option(const char *path, const char *key,
			       const char *old_value, const char *new_value)
{
	char tmp[PATH_MAX + 8];
	char line[1024];
	bool changed = false;
	FILE *in = fopen(path, "r");
	FILE *out;

	if (!in)
		return false;
	snprintf(tmp, sizeof(tmp), "%s.tmp", path);
	out = fopen(tmp, "w");
	if (!out) {
		fclose(in);
		return false;
	}
	while (fgets(line, sizeof(line), in)) {
		char k[128], v[256];

		if (sscanf(line, " %127[^ =#] = \"%255[^\"]\"", k, v) == 2 &&
		    !strcmp(k, key) && !strcmp(v, old_value)) {
			fprintf(out, "%s = \"%s\"\n", key, new_value);
			changed = true;
		} else {
			fputs(line, out);
		}
	}
	fclose(in);
	if (fclose(out) != 0 || (changed && rename(tmp, path) != 0)) {
		unlink(tmp);
		return false;
	}
	if (!changed)
		unlink(tmp);
	else
		log_msg("emud: core option %s %s -> %s", key, old_value, new_value);
	return true;
}

/* One-time updates of core option defaults (core-option-updates.conf: id|
 * key|old|new): a key still holding the old default gets the new one, a
 * value the user chose is kept; applied ids are listed in done_path so each
 * runs once per user. */
static void apply_core_option_updates(const char *path, const char *done_path)
{
	char line[512];
	FILE *in = fopen(CORE_OPTION_UPDATES, "r");

	if (!in)
		return;
	while (fgets(line, sizeof(line), in)) {
		char id[96], key[128], old_value[128], new_value[128];
		char done[128];
		bool applied = false;
		FILE *fp;

		if (line[0] == '#' ||
		    sscanf(line, "%95[^|\n]|%127[^|\n]|%127[^|\n]|%127[^|\n]",
			   id, key, old_value, new_value) != 4)
			continue;
		fp = fopen(done_path, "r");
		while (fp && !applied && fgets(done, sizeof(done), fp)) {
			trim(done);
			applied = !strcmp(done, id);
		}
		if (fp)
			fclose(fp);
		if (applied || !update_core_option(path, key, old_value, new_value))
			continue;
		fp = fopen(done_path, "a");
		if (fp) {
			fprintf(fp, "%s\n", id);
			fclose(fp);
		}
	}
	fclose(in);
}

/* Copies every file of src missing in dst (recursively). Existing files are
 * the user's and are never replaced. */
static void seed_tree(const char *src, const char *dst)
{
	DIR *dir = opendir(src);
	struct dirent *entry;

	if (!dir)
		return;
	if (mkdir_p(dst) != 0) {
		closedir(dir);
		return;
	}
	while ((entry = readdir(dir)) != NULL) {
		char from[PATH_MAX];
		char to[PATH_MAX];
		struct stat st;

		if (entry->d_name[0] == '.')
			continue;
		snprintf(from, sizeof(from), "%s/%s", src, entry->d_name);
		snprintf(to, sizeof(to), "%s/%s", dst, entry->d_name);
		if (stat(from, &st) != 0)
			continue;
		if (S_ISDIR(st.st_mode))
			seed_tree(from, to);
		else if (S_ISREG(st.st_mode) && seed_file(from, to) != 0)
			log_msg("emud: could not provide %s", to);
	}
	closedir(dir);
}

/* ------------------------------------------------------------------ */
/* Notifications (EPIC-006): typed events, the renderer localizes.     */
/* ------------------------------------------------------------------ */

static void notify_state(const char *event, int slot, const char *op)
{
	struct nuubos_notify n;

	nuubos_notify_begin(&n, "POST", "game.state", event);
	if (slot != AUTO_SLOT)
		nuubos_notify_int(&n, "slot", slot);
	if (op)
		nuubos_notify_str(&n, "op", op);
	(void)nuubos_notify_send(&n);
}

static void notify_failed(const char *reason)
{
	struct nuubos_notify n;

	nuubos_notify_begin(&n, "POST", "game.session", "game.failed");
	nuubos_notify_str(&n, "reason", reason);
	(void)nuubos_notify_send(&n);
}

static void notify_storage_lost(void)
{
	struct nuubos_notify n;

	nuubos_notify_begin(&n, "POST", "game.session", "game.storage_lost");
	(void)nuubos_notify_send(&n);
}

static void notify_slot(int slot)
{
	struct nuubos_notify n;

	nuubos_notify_begin(&n, "POST", "game.state", "game.slot");
	nuubos_notify_int(&n, "slot", slot);
	(void)nuubos_notify_send(&n);
}

static void notify_screenshot(bool ok)
{
	struct nuubos_notify n;

	nuubos_notify_begin(&n, "POST", "game.screenshot",
			    ok ? "game.screenshot.saved" : "game.screenshot.failed");
	(void)nuubos_notify_send(&n);
}

/* An action hardcore mode does not allow (save/load state, slot,
 * fast-forward). */
static void notify_hardcore(void)
{
	struct nuubos_notify n;

	nuubos_notify_begin(&n, "POST", "game.state", "game.hardcore");
	(void)nuubos_notify_send(&n);
}

static void notify_fast_forward(bool on)
{
	struct nuubos_notify n;

	nuubos_notify_begin(&n, "POST", "game.fastforward", "game.fastforward");
	nuubos_notify_str(&n, "state", on ? "on" : "off");
	(void)nuubos_notify_send(&n);
}

/* ------------------------------------------------------------------ */
/* Snapshot                                                            */
/* ------------------------------------------------------------------ */

static const char *state_name(void)
{
	switch (session.state) {
	case SESSION_RUNNING:
		return "running";
	case SESSION_EXITING:
		return "exiting";
	default:
		return "idle";
	}
}

static size_t build_status(char *out, size_t size)
{
	int n = snprintf(out, size,
			 "state=%s\n"
			 "game=%s\n"
			 "system=%s\n"
			 "core=%s\n"
			 "paused=%d\n"
			 "slot=%d\n"
			 "fast_forward=%d\n"
			 "state_busy=%d\n"
			 "slots=%d\n"
			 "overlay=%d\n"
			 "overlay_items=%s\n"
			 "fps=%.1f\n"
			 "hardcore=%d\n"
			 "cheats_on=%d\n"
			 "netplay=%s\n"
			 "netplay_peers=%d\n"
			 "netplay_peer=%s\n"
			 "quitting=%d\n"
			 "end=1\n",
			 state_name(),
			 session.state == SESSION_IDLE ? "" : session.game,
			 session.state == SESSION_IDLE ? "" : session.system,
			 session.state == SESSION_IDLE ? "" : session.core,
			 session.paused ? 1 : 0,
			 session.slot,
			 session.fast_forward ? 1 : 0,
			 session.pending != PENDING_NONE ? 1 : 0,
			 session.state == SESSION_IDLE ? DEFAULT_SLOTS : session.settings.slots,
			 session.state != SESSION_IDLE && session.settings.overlay ? 1 : 0,
			 session.state == SESSION_IDLE ? "" : session.settings.overlay_items,
			 session.fps,
			 session.state != SESSION_IDLE && session.hardcore ? 1 : 0,
			 session.cheats_on,
			 session.state == SESSION_IDLE ? "off"
			 : session.netplay == NET_HOST ? "host"
			 : session.netplay == NET_CLIENT ? "client"
			 : session.net_launch[0] ? "starting" : "off",
			 session.net_peers, session.net_peer,
			 /* The game is closing: quit asked (its automatic save may
			 * still run) or RetroArch exiting. The Quick Menu covers it
			 * with the closing screen; not when it restarts as netplay
			 * host. */
			 !session.relaunch_host &&
			 (session.state == SESSION_EXITING ||
			  (session.state == SESSION_RUNNING && session.quit_requested)) ? 1 : 0);

	return n < 0 ? 0 : (size_t)n < size ? (size_t)n : size - 1;
}

static void close_client(struct client *c)
{
	if (c->fd >= 0)
		close(c->fd);
	c->fd = -1;
	c->subscribed = false;
	c->used = 0;
	if (session.power_client >= 0 && &clients[session.power_client] == c)
		session.power_client = -1;
	discovery.waiting[c - clients] = false;
}

static void reply(struct client *c, const char *text)
{
	if (c->fd >= 0 && write_all(c->fd, text, strlen(text)) != 0)
		close_client(c);
}

/* Low-latency Wi-Fi (radio power save off) while a netplay session runs:
 * nuubos-wifid watches this directory (battery audit 2026-10-14). */
#define LOW_LATENCY_MARK RUN_ROOT "/low-latency/emud"

static void sync_low_latency(void)
{
	static int marked = -1;
	int want = session.state != SESSION_IDLE &&
		   (session.net_launch[0] || session.netplay != NET_OFF);

	if (want == marked)
		return;
	if (want) {
		int fd;

		(void)mkdir(RUN_ROOT "/low-latency", 0755);
		fd = open(LOW_LATENCY_MARK, O_WRONLY | O_CREAT | O_CLOEXEC, 0644);
		if (fd >= 0)
			close(fd);
	} else {
		(void)unlink(LOW_LATENCY_MARK);
	}
	marked = want;
}

static void notify_subscribers(void)
{
	char status[768];

	sync_low_latency();
	size_t len = build_status(status, sizeof(status));
	int i;

	for (i = 0; i < MAX_CLIENTS; i++) {
		if (clients[i].fd < 0 || !clients[i].subscribed)
			continue;
		if (write_all(clients[i].fd, status, len) != 0)
			close_client(&clients[i]);
	}
}

/* ------------------------------------------------------------------ */
/* Other services                                                      */
/* ------------------------------------------------------------------ */

static int connect_socket(const char *path, int timeout_ms)
{
	struct sockaddr_un addr;
	struct timeval tv;
	int fd = socket(AF_UNIX, SOCK_STREAM | SOCK_CLOEXEC, 0);

	if (fd < 0)
		return -1;
	memset(&addr, 0, sizeof(addr));
	addr.sun_family = AF_UNIX;
	copy_text(addr.sun_path, sizeof(addr.sun_path), path);
	tv.tv_sec = timeout_ms / 1000;
	tv.tv_usec = (timeout_ms % 1000) * 1000;
	(void)setsockopt(fd, SOL_SOCKET, SO_RCVTIMEO, &tv, sizeof(tv));
	(void)setsockopt(fd, SOL_SOCKET, SO_SNDTIMEO, &tv, sizeof(tv));
	if (connect(fd, (struct sockaddr *)&addr, sizeof(addr)) != 0) {
		close(fd);
		return -1;
	}
	return fd;
}

static int read_line_fd(int fd, char *out, size_t size)
{
	size_t used = 0;

	while (used + 1 < size) {
		char c;
		ssize_t n = read(fd, &c, 1);

		if (n < 0 && errno == EINTR)
			continue;
		if (n <= 0)
			return -1;
		if (c == '\n')
			break;
		out[used++] = c;
	}
	out[used] = '\0';
	return 0;
}

/* One request/one line reply to nuubos-libraryd. */
static int library_request(const char *command, char *reply_out, size_t size)
{
	int fd = connect_socket(LIBRARY_SOCKET, LIBRARY_TIMEOUT_MS);
	int rc;

	if (fd < 0)
		return -1;
	if (write_all(fd, command, strlen(command)) != 0 ||
	    write_all(fd, "\n", 1) != 0) {
		close(fd);
		return -1;
	}
	rc = read_line_fd(fd, reply_out, size);
	close(fd);
	return rc;
}

static void library_session(const char *verb, const char *game)
{
	char command[128];
	char answer[128];

	snprintf(command, sizeof(command), "%s\t%s", verb, game);
	if (library_request(command, answer, sizeof(answer)) != 0 ||
	    strcmp(answer, "OK") != 0)
		log_msg("emud: libraryd %s %s failed: %s", verb, game,
			answer[0] ? answer : "no reply");
}

/* Game gamepads stay on for as long as this connection is open: if emud
 * dies, inputd gives the controllers back to the nuubUI by itself. */
static int input_gamepads_on(void)
{
	char answer[64];
	int fd = connect_socket(INPUT_SOCKET, 1000);

	if (fd < 0)
		return -1;
	if (write_all(fd, "GAMEPADS ON\n", 12) != 0 ||
	    read_line_fd(fd, answer, sizeof(answer)) != 0 ||
	    strcmp(answer, "OK") != 0) {
		close(fd);
		return -1;
	}
	(void)fcntl(fd, F_SETFL, O_NONBLOCK);
	return fd;
}

static bool read_active_user(char *out, size_t size)
{
	FILE *fp = fopen(ACTIVE_USER_FILE, "r");

	out[0] = '\0';
	if (!fp)
		return false;
	if (!fgets(out, (int)size, fp))
		out[0] = '\0';
	fclose(fp);
	trim(out);
	/* Users are UUIDs: never build paths from anything else. */
	for (const char *p = out; *p; p++)
		if (!isxdigit((unsigned char)*p) && *p != '-')
			return false;
	return out[0] != '\0';
}

/* RetroArch user_language for the user's nuubOS language. */
static int retroarch_language(const char *user)
{
	static const struct {
		const char *code;
		int retro;
	} langs[] = {
		{ "en", 0 }, { "fr", 2 }, { "es", 3 }, { "de", 4 },
		{ "it", 5 }, { "nl", 6 }, { "pt", 8 }, { "ja", 1 },
		{ "ru", 9 }, { "ko", 10 }, { "zh-tw", 11 }, { "zh-cn", 12 },
		{ "pl", 14 }, { "el", 17 }, { "tr", 18 }, { "fi", 23 },
		{ "sv", 25 }, { "uk", 26 }, { "cs", 27 }, { "hu", 31 },
		{ "nb", 34 }, { "es-419", 3 }, { "pt-br", 7 }, { "id", 24 },
		/* RetroArch has no Danish or Romanian: English. */
	};
	char path[PATH_MAX];
	char line[128];
	FILE *fp;
	int result = 0;

	snprintf(path, sizeof(path), STATE_USERS "/%s/localization.conf", user);
	fp = fopen(path, "r");
	if (!fp)
		return 0;
	while (fgets(line, sizeof(line), fp)) {
		trim(line);
		if (strncmp(line, "LANGUAGE=", 9) != 0)
			continue;
		for (size_t i = 0; i < sizeof(langs) / sizeof(langs[0]); i++)
			if (strcmp(line + 9, langs[i].code) == 0)
				result = langs[i].retro;
	}
	fclose(fp);
	return result;
}

/*
 * cores.conf: "<system>|<core>[,<core>...]" in order of preference. A
 * bundled (qualified) core wins over a downloaded one; a system without a
 * bundled core uses a core the user downloaded from RetroArch.
 */
static void content_base_name(const char *content, char *out, size_t size);
static bool list_has(const char *list, const char *name);

/* A field ("key = "value"") of a core's libretro core-info file; "" when
 * the file or the field is missing. */
static void core_info_value(const char *core, const char *key, char *out, size_t size)
{
	char path[PATH_MAX];
	char line[1024];
	size_t klen = strlen(key);
	FILE *fp;

	out[0] = '\0';
	snprintf(path, sizeof(path), CORE_INFO_DIR "/%s_libretro.info", core);
	fp = fopen(path, "r");
	if (!fp)
		return;
	while (fgets(line, sizeof(line), fp)) {
		char *p = line;
		char *end;

		if (strncmp(p, key, klen))
			continue;
		p += klen;
		while (*p == ' ')
			p++;
		if (*p++ != '=')
			continue;
		while (*p == ' ')
			p++;
		if (*p == '"')
			p++;
		trim(p);
		if ((end = strrchr(p, '"')))
			*end = '\0';
		copy_text(out, size, p);
		break;
	}
	fclose(fp);
}

/* libretro.conf: the libretro databases of a system, "|" separated ("" =
 * none). */
static void system_databases(const char *system, char *out, size_t size)
{
	char line[1024];
	FILE *fp = fopen(LIBRETRO_CONF, "r");
	size_t len = strlen(system);

	out[0] = '\0';
	if (!fp)
		return;
	while (fgets(line, sizeof(line), fp)) {
		trim(line);
		if (line[0] == '#' || strncmp(line, system, len) || line[len] != '|')
			continue;
		copy_text(out, size, line + len + 1);
		break;
	}
	fclose(fp);
}

/* "a|b|c" has name. */
static bool pipe_list_has(const char *list, const char *name)
{
	size_t len = strlen(name);

	for (const char *p = list; *p;) {
		const char *bar = strchr(p, '|');
		size_t n = bar ? (size_t)(bar - p) : strlen(p);

		if (n == len && !strncmp(p, name, len))
			return true;
		if (!bar)
			break;
		p = bar + 1;
	}
	return false;
}

/* The core's core-info names one of the databases dbs ("a|b"). */
static bool core_for_databases(const char *core, const char *dbs)
{
	char info[1024];
	char *save = NULL;

	if (!dbs[0])
		return false;
	core_info_value(core, "database", info, sizeof(info));
	for (char *db = strtok_r(info, "|", &save); db; db = strtok_r(NULL, "|", &save))
		if (pipe_list_has(dbs, db))
			return true;
	return false;
}

static int core_file_filter(const struct dirent *e)
{
	size_t len = strlen(e->d_name);

	return e->d_name[0] != '.' && len > 12 && !strcmp(e->d_name + len - 12, "_libretro.so");
}

/* Cores the user downloaded for the system beyond cores.conf (Get More
 * Emulators, or RetroArch's own Online Updater): a core in
 * /userdata/retroarch/cores whose core-info names one of the system's
 * libretro databases, appended to out ("a,b") in name order. */
static void downloaded_cores(const char *system, char *out, size_t size)
{
	struct dirent **names = NULL;
	char dbs[512];
	int n;

	system_databases(system, dbs, sizeof(dbs));
	if (!dbs[0])
		return;
	n = scandir(DOWNLOADED_CORES, &names, core_file_filter, alphasort);
	for (int i = 0; i < n; i++) {
		char core[NAME_MAX + 1];
		char bundled[PATH_MAX];

		copy_text(core, sizeof(core), names[i]->d_name);
		core[strlen(core) - 12] = '\0';
		snprintf(bundled, sizeof(bundled), BUNDLED_CORES "/%s_libretro.so", core);
		if (!list_has(out, core) && !file_exists(bundled) &&
		    core_for_databases(core, dbs) && strlen(out) + strlen(core) + 2 < size) {
			if (out[0])
				strcat(out, ",");
			strcat(out, core);
		}
		free(names[i]);
	}
	free(names);
}

/* MAME machine and media switches of a system ("apple2e",
 * "dsk:-flop1,woz:-flop1"), from the optional cores.conf fields 3-4. */
static char resolved_machine[64];
static char resolved_media[256];

static bool resolve_core(const char *system, char *core, size_t core_size,
			 char *path, size_t path_size)
{
	static const char *const roots[] = { BUNDLED_CORES, DOWNLOADED_CORES };
	char line[512];
	FILE *fp = fopen(CORES_CONF, "r");
	bool found = false;

	if (!fp)
		return false;
	while (!found && fgets(line, sizeof(line), fp)) {
		char *sep;
		char *save = NULL;
		char *name;
		char *extra;
		char list[512];

		trim(line);
		if (!line[0] || line[0] == '#')
			continue;
		sep = strchr(line, '|');
		if (!sep)
			continue;
		*sep = '\0';
		if (strcmp(line, system) != 0)
			continue;
		resolved_machine[0] = '\0';
		resolved_media[0] = '\0';
		extra = strchr(sep + 1, '|');
		if (extra) {
			char *media;

			*extra++ = '\0';
			media = strchr(extra, '|');
			if (media)
				*media++ = '\0';
			copy_text(resolved_machine, sizeof(resolved_machine), extra);
			copy_text(resolved_media, sizeof(resolved_media), media ? media : "");
		}
		for (size_t r = 0; r < 2 && !found; r++) {
			copy_text(list, sizeof(list), sep + 1);
			for (name = strtok_r(list, ",", &save); name && !found;
			     name = strtok_r(NULL, ",", &save)) {
				snprintf(path, path_size, "%s/%s_libretro.so",
					 roots[r], name);
				if (file_exists(path)) {
					copy_text(core, core_size, name);
					found = true;
				}
			}
		}
	}
	fclose(fp);
	/* No core of cores.conf: one the user downloaded for the system. */
	if (!found) {
		char extra[512] = "";
		char *comma;

		downloaded_cores(system, extra, sizeof(extra));
		if ((comma = strchr(extra, ',')))
			*comma = '\0';
		if (extra[0]) {
			copy_text(core, core_size, extra);
			snprintf(path, path_size, DOWNLOADED_CORES "/%s_libretro.so", extra);
			resolved_machine[0] = '\0';
			resolved_media[0] = '\0';
			found = true;
		}
	}
	return found;
}

/* ------------------------------------------------------------------ */
/* Per-game overrides (EPIC-018)                                       */
/* ------------------------------------------------------------------ */

static bool valid_game(const char *game)
{
	size_t len = strlen(game);

	if (!len || len >= 32)
		return false;
	for (const char *p = game; *p; p++)
		if (!isxdigit((unsigned char)*p))
			return false;
	return true;
}

static void overrides_path(const char *user, const char *game, char *out, size_t size)
{
	snprintf(out, size, USERS_DIR "/%s/" GAME_SETTINGS_SUBDIR "/%s.conf", user, game);
}

static void load_overrides(const char *user, const char *game, struct game_overrides *o)
{
	char path[PATH_MAX];
	char line[PATH_MAX + 32];
	FILE *fp;

	memset(o, 0, sizeof(*o));
	overrides_path(user, game, path, sizeof(path));
	fp = fopen(path, "r");
	if (!fp)
		return;
	while (fgets(line, sizeof(line), fp)) {
		char *eq;

		trim(line);
		if (!(eq = strchr(line, '=')))
			continue;
		*eq++ = '\0';
		if (!strcmp(line, "core"))
			copy_text(o->core, sizeof(o->core), eq);
		else if (!strcmp(line, "aspect"))
			copy_text(o->aspect, sizeof(o->aspect), eq);
		else if (!strcmp(line, "filter"))
			copy_text(o->filter, sizeof(o->filter), eq);
		else if (!strcmp(line, "cheat_file"))
			copy_text(o->cheat_file, sizeof(o->cheat_file), eq);
		else if (!strcmp(line, "cheats"))
			copy_text(o->cheats, sizeof(o->cheats), eq);
	}
	fclose(fp);
}

static int save_overrides(const char *user, const char *game, const struct game_overrides *o)
{
	char path[PATH_MAX];
	char tmp[PATH_MAX + 8];
	char dir[PATH_MAX];
	FILE *fp;
	int rc = 0;

	overrides_path(user, game, path, sizeof(path));
	/* Nothing overridden: no file, the game inherits everything. */
	if (!o->core[0] && !o->aspect[0] && !o->filter[0] && !o->cheat_file[0] && !o->cheats[0])
		return unlink(path) == 0 || errno == ENOENT ? 0 : -1;
	snprintf(dir, sizeof(dir), USERS_DIR "/%s/" GAME_SETTINGS_SUBDIR, user);
	if (mkdir_p(dir) != 0)
		return -1;
	snprintf(tmp, sizeof(tmp), "%s.tmp", path);
	fp = fopen(tmp, "w");
	if (!fp)
		return -1;
	if (o->core[0])
		fprintf(fp, "core=%s\n", o->core);
	if (o->aspect[0])
		fprintf(fp, "aspect=%s\n", o->aspect);
	if (o->filter[0])
		fprintf(fp, "filter=%s\n", o->filter);
	if (o->cheat_file[0])
		fprintf(fp, "cheat_file=%s\n", o->cheat_file);
	if (o->cheats[0])
		fprintf(fp, "cheats=%s\n", o->cheats);
	if (ferror(fp))
		rc = -1;
	if (fclose(fp) != 0)
		rc = -1;
	if (rc == 0 && rename(tmp, path) != 0)
		rc = -1;
	if (rc != 0)
		unlink(tmp);
	return rc;
}

static bool valid_aspect(const char *v)
{
	return !strcmp(v, "core") || !strcmp(v, "4:3") || !strcmp(v, "16:9") ||
	       !strcmp(v, "full") || !strcmp(v, "integer");
}

static bool valid_filter(const char *v)
{
	return !strcmp(v, "sharp") || !strcmp(v, "smooth") || !strcmp(v, "pixel");
}

/* Cores of a system that can be loaded (bundled or downloaded), in
 * cores.conf preference order, then the other cores the user downloaded
 * for it, as "a,b,c". */
static void available_cores(const char *system, char *out, size_t size)
{
	static const char *const roots[] = { BUNDLED_CORES, DOWNLOADED_CORES };
	char line[512];
	FILE *fp = fopen(CORES_CONF, "r");

	out[0] = '\0';
	if (!fp)
		return;
	while (fgets(line, sizeof(line), fp)) {
		char *sep, *end, *save = NULL;

		trim(line);
		if (!line[0] || line[0] == '#' || !(sep = strchr(line, '|')))
			continue;
		*sep++ = '\0';
		if (strcmp(line, system) != 0)
			continue;
		if ((end = strchr(sep, '|')))
			*end = '\0';
		for (char *name = strtok_r(sep, ",", &save); name; name = strtok_r(NULL, ",", &save)) {
			char path[PATH_MAX];
			bool ok = false;

			for (size_t r = 0; r < 2 && !ok; r++) {
				snprintf(path, sizeof(path), "%s/%s_libretro.so", roots[r], name);
				ok = file_exists(path);
			}
			if (ok && strlen(out) + strlen(name) + 2 < size) {
				if (out[0])
					strcat(out, ",");
				strcat(out, name);
			}
		}
		break;
	}
	fclose(fp);
	downloaded_cores(system, out, size);
}

static bool list_has(const char *list, const char *name)
{
	size_t len = strlen(name);

	for (const char *p = list; *p;) {
		const char *comma = strchr(p, ',');
		size_t n = comma ? (size_t)(comma - p) : strlen(p);

		if (n == len && !strncmp(p, name, len))
			return true;
		if (!comma)
			break;
		p = comma + 1;
	}
	return false;
}

static void core_file(const char *core, char *out, size_t size)
{
	snprintf(out, size, BUNDLED_CORES "/%s_libretro.so", core);
	if (!file_exists(out))
		snprintf(out, size, DOWNLOADED_CORES "/%s_libretro.so", core);
}

/* ------------------------------------------------------------------ */
/* User emulation settings (Settings -> Gaming)                        */
/* ------------------------------------------------------------------ */

static bool word_in(const char *const *list, const char *word)
{
	for (; *list; list++)
		if (!strcmp(*list, word))
			return true;
	return false;
}

/* "a,b,c" made only of known overlay fields (empty allowed). */
static bool valid_overlay_items(const char *v)
{
	char copy[96];
	char *save = NULL;

	if (strlen(v) >= sizeof(copy))
		return false;
	copy_text(copy, sizeof(copy), v);
	for (char *w = strtok_r(copy, ",", &save); w; w = strtok_r(NULL, ",", &save))
		if (!word_in(overlay_fields, w))
			return false;
	return true;
}

static void default_settings(struct emu_settings *e)
{
	memset(e, 0, sizeof(*e));
	e->slots = DEFAULT_SLOTS;
	e->slot_rotation = true;
	e->bezels = true; /* user decision 2026-10-15 */
	e->crt = true;
	copy_text(e->overlay_items, sizeof(e->overlay_items), DEFAULT_OVERLAY_ITEMS);
	for (int i = 0; i < HK_COUNT; i++)
		copy_text(e->hotkey[i], sizeof(e->hotkey[i]), hotkey_defaults[i]);
}

/* Applies key=value to e; false when the key or value is not valid. */
static bool settings_set(struct emu_settings *e, const char *key, const char *v)
{
	bool flag = !strcmp(v, "1");

	if (!strcmp(key, "boot_screens") || !strcmp(key, "slot_rotation") ||
	    !strcmp(key, "bezels") || !strcmp(key, "crt") || !strcmp(key, "overlay")) {
		if (strcmp(v, "0") && strcmp(v, "1"))
			return false;
		if (!strcmp(key, "boot_screens"))
			e->boot_screens = flag;
		else if (!strcmp(key, "slot_rotation"))
			e->slot_rotation = flag;
		else if (!strcmp(key, "bezels"))
			e->bezels = flag;
		else if (!strcmp(key, "crt"))
			e->crt = flag;
		else
			e->overlay = flag;
		return true;
	}
	if (!strcmp(key, "slots")) {
		char *end;
		long n = strtol(v, &end, 10);

		if (*end || n < 1 || n > MAX_SLOTS)
			return false;
		e->slots = (int)n;
		return true;
	}
	if (!strcmp(key, "overlay_items")) {
		if (!valid_overlay_items(v))
			return false;
		copy_text(e->overlay_items, sizeof(e->overlay_items), v);
		return true;
	}
	if (!strncmp(key, "hotkey_", 7)) {
		for (int i = 0; i < HK_COUNT; i++) {
			if (strcmp(key + 7, hotkey_actions[i]))
				continue;
			if (!word_in(hotkey_controls, v))
				return false;
			copy_text(e->hotkey[i], sizeof(e->hotkey[i]), v);
			return true;
		}
	}
	return false;
}

static void settings_path(const char *user, char *out, size_t size)
{
	snprintf(out, size, USERS_DIR "/%s/" EMU_SETTINGS_FILE, user);
}

static void load_settings(const char *user, struct emu_settings *e)
{
	char path[PATH_MAX];
	char line[256];
	FILE *fp;

	default_settings(e);
	settings_path(user, path, sizeof(path));
	fp = fopen(path, "r");
	if (!fp)
		return;
	while (fgets(line, sizeof(line), fp)) {
		char *eq;

		trim(line);
		if (!(eq = strchr(line, '=')))
			continue;
		*eq++ = '\0';
		(void)settings_set(e, line, eq);
	}
	fclose(fp);
}

static size_t format_settings(const struct emu_settings *e, char *out, size_t size)
{
	size_t len;
	int n = snprintf(out, size,
			 "boot_screens=%d\nslots=%d\nslot_rotation=%d\nbezels=%d\ncrt=%d\n"
			 "overlay=%d\noverlay_items=%s\n",
			 e->boot_screens, e->slots, e->slot_rotation, e->bezels, e->crt,
			 e->overlay, e->overlay_items);

	len = n < 0 ? 0 : (size_t)n < size ? (size_t)n : size - 1;
	for (int i = 0; i < HK_COUNT && len < size; i++) {
		n = snprintf(out + len, size - len, "hotkey_%s=%s\n",
			     hotkey_actions[i], e->hotkey[i]);
		if (n > 0)
			len = (size_t)n < size - len ? len + (size_t)n : size - 1;
	}
	return len;
}

static int save_settings(const char *user, const struct emu_settings *e)
{
	char path[PATH_MAX];
	char tmp[PATH_MAX + 8];
	char dir[PATH_MAX];
	char text[1024];
	size_t len = format_settings(e, text, sizeof(text));
	FILE *fp;
	int rc = 0;

	snprintf(dir, sizeof(dir), USERS_DIR "/%s/appdata/nuubos-emulation", user);
	if (mkdir_p(dir) != 0)
		return -1;
	settings_path(user, path, sizeof(path));
	snprintf(tmp, sizeof(tmp), "%s.tmp", path);
	fp = fopen(tmp, "w");
	if (!fp)
		return -1;
	if (fwrite(text, 1, len, fp) != len)
		rc = -1;
	if (fclose(fp) != 0)
		rc = -1;
	if (rc == 0 && rename(tmp, path) != 0)
		rc = -1;
	if (rc != 0)
		unlink(tmp);
	return rc;
}

/* The chords inputd applies while this session holds the gamepads. */
static void send_hotkeys(void)
{
	char line[512] = "HOTKEYS";
	size_t len = strlen(line);

	if (session.input_fd < 0)
		return;
	for (int i = 0; i < HK_COUNT; i++) {
		int n = snprintf(line + len, sizeof(line) - len, " %s=%s",
				 hotkey_actions[i], session.settings.hotkey[i]);

		if (n > 0 && (size_t)n < sizeof(line) - len)
			len += (size_t)n;
	}
	line[len++] = '\n';
	if (write_all(session.input_fd, line, len) != 0)
		log_msg("emud: hotkeys to inputd failed");
}

/* ------------------------------------------------------------------ */
/* Display: screen type, bezel colour and the session shader           */
/* ------------------------------------------------------------------ */

/* display.conf field (1 = screen type, 2 = HDMI lines) of a system; ""
 * when the system or the field is missing. */
static void display_field(const char *system, int field, char *out, size_t size)
{
	char line[128];
	FILE *fp = fopen(DISPLAY_CONF, "r");

	copy_text(out, size, "");
	if (!fp)
		return;
	while (fgets(line, sizeof(line), fp)) {
		char *sep;

		trim(line);
		if (line[0] == '#' || !(sep = strchr(line, '|')))
			continue;
		*sep++ = '\0';
		if (strcmp(line, system))
			continue;
		for (int i = 1; i < field && sep; i++)
			if ((sep = strchr(sep, '|')))
				sep++;
		if (sep) {
			char *end = strchr(sep, '|');

			if (end)
				*end = '\0';
			copy_text(out, size, sep);
		}
		break;
	}
	fclose(fp);
}

/* display.conf screen type of a system: "crt", "lcd" or "monitor". */
static void system_screen(const char *system, char *out, size_t size)
{
	display_field(system, 1, out, size);
	if (!out[0])
		copy_text(out, size, "monitor");
}

/* Render height of a game of the system on the current output: the system's
 * HDMI lines while HDMI is the active output (the output policy turns the
 * panel off then), else 0 = native. */
static int game_render_height(const char *system)
{
	char value[16] = "";
	FILE *fp = fopen(HDMI_ENABLED, "r");

	if (!fp)
		return 0;
	if (!fgets(value, sizeof(value), fp))
		value[0] = '\0';
	fclose(fp);
	trim(value);
	if (strcmp(value, "enabled") != 0)
		return 0;
	display_field(system, 2, value, sizeof(value));
	return value[0] ? atoi(value) : GAME_HDMI_LINES;
}

/* The active output is a square built-in panel (RG CubeXX 720x720): an
 * enabled non-HDMI connector whose first (preferred) mode is WxW. */
static bool square_panel(void)
{
	DIR *dir = opendir(DRM_CLASS);
	struct dirent *e;
	bool square = false;

	if (!dir)
		return false;
	while (!square && (e = readdir(dir)) != NULL) {
		char path[PATH_MAX];
		char value[32] = "";
		unsigned w = 0, h = 0;
		FILE *fp;

		if (strncmp(e->d_name, "card0-", 6) || strstr(e->d_name, "HDMI"))
			continue;
		snprintf(path, sizeof(path), DRM_CLASS "/%s/enabled", e->d_name);
		if (!(fp = fopen(path, "r")))
			continue;
		if (!fgets(value, sizeof(value), fp))
			value[0] = '\0';
		fclose(fp);
		trim(value);
		if (strcmp(value, "enabled"))
			continue;
		snprintf(path, sizeof(path), DRM_CLASS "/%s/modes", e->d_name);
		if (!(fp = fopen(path, "r")))
			continue;
		if (fscanf(fp, "%ux%u", &w, &h) == 2 && w && w == h)
			square = true;
		fclose(fp);
	}
	closedir(dir);
	return square;
}

/* A TV system's picture fills a square handheld screen's height as far as
 * at most a share of its width (a TV's overscan) is cropped (RetroArch patch
 * 0006, NUUBOS_RA_OVERSCAN_X; bezel.glsl OVERSCAN_X): with the default 8 %
 * (4 % a side) a 4:3 game on the 720x720 RG CubeXX is 783x587, not an
 * integer 2x with large margins (user decisions 2026-10-11: square screens
 * only, never HDMI or 4:3/3:2 panels; integer 3x cropped PS1 too much).
 * display.conf field 4 sets the share per system (PS1 5 %, user request
 * 2026-10-14: 8 % cut its borders a little too much). Only with the default
 * presentation (or the per-game Integer Scaling). Returns the percentage of
 * the width that may be cropped, 0 = off. */
#define TV_OVERSCAN_DEFAULT_PCT 8

static int tv_overscan(const char *system, const struct game_overrides *o)
{
	char screen[16], pct[16];
	int value;

	if (o->aspect[0] && strcmp(o->aspect, "integer"))
		return 0;
	if (!square_panel()) /* HDMI turns the panel off */
		return 0;
	system_screen(system, screen, sizeof(screen));
	if (strcmp(screen, "crt"))
		return 0;
	display_field(system, 3, pct, sizeof(pct));
	value = atoi(pct);
	return value > 0 && value < 50 ? value : TV_OVERSCAN_DEFAULT_PCT;
}

/* systems.conf colour (field 6, #rrggbb) as 0..1 components. */
static void system_colour(const char *system, float rgb[3])
{
	char line[512];
	FILE *fp = fopen(SYSTEMS_CONF, "r");

	rgb[0] = 0.25f;
	rgb[1] = 0.45f;
	rgb[2] = 0.90f;
	if (!fp)
		return;
	while (fgets(line, sizeof(line), fp)) {
		char *field[7] = { line };
		int n = 1;
		unsigned int r, g, b;

		if (line[0] == '#')
			continue;
		for (char *p = line; *p && n < 7; p++)
			if (*p == '|') {
				*p = '\0';
				field[n++] = p + 1;
			}
		if (n < 6 || strcmp(field[0], system))
			continue;
		if (sscanf(field[5], "#%02x%02x%02x", &r, &g, &b) == 3) {
			rgb[0] = (float)r / 255.0f;
			rgb[1] = (float)g / 255.0f;
			rgb[2] = (float)b / 255.0f;
		}
		break;
	}
	fclose(fp);
}

/* The Mood colours of the bezel (logo, matte): the lifecycle splash cache
 * /etc/nuubos/splash.palette, written by nuubos-themectl for the active
 * user; built-in nuubOS colours without it. */
struct mood_colours {
	float background[3], text[3], accent[3];
};

static void set_hex_rgb(float out[3], const char *v)
{
	unsigned int r, g, b;

	if (sscanf(v, "#%02x%02x%02x", &r, &g, &b) == 3) {
		out[0] = (float)r / 255.0f;
		out[1] = (float)g / 255.0f;
		out[2] = (float)b / 255.0f;
	}
}

static void mood_colours(struct mood_colours *m)
{
	char line[128];
	FILE *fp;

	set_hex_rgb(m->background, "#0d0e10");
	set_hex_rgb(m->text, "#f5f6f8");
	set_hex_rgb(m->accent, "#3a86ff");
	fp = fopen("/etc/nuubos/splash.palette", "r");
	if (!fp)
		return;
	while (fgets(line, sizeof(line), fp)) {
		if (!strncmp(line, "background=", 11))
			set_hex_rgb(m->background, line + 11);
		else if (!strncmp(line, "text=", 5))
			set_hex_rgb(m->text, line + 5);
		else if (!strncmp(line, "accent=", 7))
			set_hex_rgb(m->accent, line + 7);
	}
	fclose(fp);
}

/* Boot sequence: the core options that skip or show the console's own
 * start-up (BIOS logo), set at every launch from the user's setting. */
static const struct {
	const char *key;
	const char *on;
	const char *off;
} boot_options[] = {
	{ "pcsx_rearmed_show_bios_bootlogo", "enabled", "disabled" },
	{ "mgba_skip_bios", "OFF", "ON" },
	{ "gambatte_gb_bootloader", "enabled", "disabled" },
	{ "genesis_plus_gx_bios", "enabled", "disabled" },
	{ "melonds_boot_directly", "disabled", "enabled" },
};

/* Rewrites the boot keys of the user's core options file. */
static void apply_boot_options(const char *path, bool boot)
{
	char tmp[PATH_MAX + 8];
	char line[1024];
	bool done[sizeof(boot_options) / sizeof(boot_options[0])] = { false };
	FILE *in = fopen(path, "r");
	FILE *out;

	if (!in)
		return;
	snprintf(tmp, sizeof(tmp), "%s.tmp", path);
	out = fopen(tmp, "w");
	if (!out) {
		fclose(in);
		return;
	}
	while (fgets(line, sizeof(line), in)) {
		char key[128];
		size_t i;

		for (i = 0; i < sizeof(boot_options) / sizeof(boot_options[0]); i++)
			if (sscanf(line, " %127[^ =#] =", key) == 1 &&
			    !strcmp(key, boot_options[i].key))
				break;
		if (i == sizeof(boot_options) / sizeof(boot_options[0])) {
			fputs(line, out);
			continue;
		}
		if (!done[i])
			fprintf(out, "%s = \"%s\"\n", boot_options[i].key,
				boot ? boot_options[i].on : boot_options[i].off);
		done[i] = true;
	}
	for (size_t i = 0; i < sizeof(boot_options) / sizeof(boot_options[0]); i++)
		if (!done[i])
			fprintf(out, "%s = \"%s\"\n", boot_options[i].key,
				boot ? boot_options[i].on : boot_options[i].off);
	fclose(in);
	if (fclose(out) != 0 || rename(tmp, path) != 0)
		unlink(tmp);
}

/* ------------------------------------------------------------------ */
/* RetroAchievements (EPIC-017)                                        */
/* ------------------------------------------------------------------ */

struct achievements {
	bool enabled;
	bool hardcore;
	char username[64];
	char token[128];
};

static bool cfg_value_safe(const char *v)
{
	return strpbrk(v, "\"\\\r\n") == NULL;
}

static void load_achievements(const char *user, struct achievements *a)
{
	char path[PATH_MAX];
	char line[256];
	FILE *fp;

	memset(a, 0, sizeof(*a));
	snprintf(path, sizeof(path), STATE_USERS "/%s/" ACHIEVEMENTS_FILE, user);
	fp = fopen(path, "r");
	if (!fp)
		return;
	while (fgets(line, sizeof(line), fp)) {
		char *eq;

		trim(line);
		if (!(eq = strchr(line, '=')))
			continue;
		*eq++ = '\0';
		if (!strcmp(line, "USERNAME"))
			copy_text(a->username, sizeof(a->username), eq);
		else if (!strcmp(line, "TOKEN"))
			copy_text(a->token, sizeof(a->token), eq);
		else if (!strcmp(line, "ENABLED"))
			a->enabled = !strcmp(eq, "1");
		else if (!strcmp(line, "HARDCORE"))
			a->hardcore = !strcmp(eq, "1");
	}
	fclose(fp);
	if (!a->username[0] || !a->token[0] || !cfg_value_safe(a->username) ||
	    !cfg_value_safe(a->token))
		a->enabled = false;
}

/* ------------------------------------------------------------------ */
/* User RetroArch keys restored after a session                         */
/* ------------------------------------------------------------------ */

static void free_restore(void)
{
	for (int i = 0; i < session.restore_count; i++)
		free(session.restore_value[i]);
	session.restore_count = 0;
}

/* Remember the user's value of key before the session overrides it. */
static void remember_key(const char *key)
{
	char line[1024];
	FILE *fp;
	size_t klen = strlen(key);
	int i;

	for (i = 0; i < session.restore_count; i++)
		if (!strcmp(session.restore_key[i], key))
			return;
	if (session.restore_count >= MAX_RESTORE_KEYS)
		return;
	i = session.restore_count++;
	copy_text(session.restore_key[i], sizeof(session.restore_key[i]), key);
	session.restore_value[i] = NULL;
	fp = fopen(session.ra_cfg, "r");
	if (!fp)
		return;
	while (fgets(line, sizeof(line), fp)) {
		char *p = line;

		if (strncmp(p, key, klen) != 0)
			continue;
		p += klen;
		while (*p == ' ')
			p++;
		if (*p != '=')
			continue;
		free(session.restore_value[i]);
		session.restore_value[i] = strdup(line);
	}
	fclose(fp);
}

/* Put the remembered keys back into the user's retroarch.cfg. */
static void restore_user_keys(void)
{
	char tmp[PATH_MAX + 8];
	char line[2048];
	bool written[MAX_RESTORE_KEYS] = { false };
	FILE *in, *out;

	if (!session.restore_count || !session.ra_cfg[0])
		return;
	in = fopen(session.ra_cfg, "r");
	if (!in) {
		free_restore();
		return;
	}
	snprintf(tmp, sizeof(tmp), "%s.tmp", session.ra_cfg);
	out = fopen(tmp, "w");
	if (!out) {
		fclose(in);
		free_restore();
		return;
	}
	while (fgets(line, sizeof(line), in)) {
		int match = -1;

		for (int i = 0; i < session.restore_count && match < 0; i++) {
			size_t klen = strlen(session.restore_key[i]);
			const char *p = line + klen;

			if (strncmp(line, session.restore_key[i], klen) != 0)
				continue;
			while (*p == ' ')
				p++;
			if (*p == '=')
				match = i;
		}
		if (match < 0) {
			fputs(line, out);
		} else if (session.restore_value[match] && !written[match]) {
			fputs(session.restore_value[match], out);
			written[match] = true;
		}
	}
	fclose(in);
	if (fclose(out) == 0 && rename(tmp, session.ra_cfg) == 0)
		log_msg("emud: restored %d user RetroArch keys", session.restore_count);
	else
		unlink(tmp);
	free_restore();
}

/* Shader preset of the session: one pass, its parameters. */
/* The session preset: one pass of shader, or for the bezel (cached = true)
 * that pass into a feedback FBO and bezel-picture.glsl to the screen (bezel.glsl;
 * the last pass has no scale: with one RetroArch would add a pass after it). */
static int write_session_shader(const char *shader, bool cached, const char *params)
{
	char tmp[] = SESSION_SHADER ".tmp";
	FILE *fp = fopen(tmp, "w");
	int rc;

	if (!fp)
		return -1;
	fprintf(fp, "shaders = \"%d\"\nshader0 = \"%s\"\nfilter_linear0 = \"false\"\n",
		cached ? 2 : 1, shader);
	if (cached)
		fputs("scale_type0 = \"viewport\"\nscale0 = \"1.000000\"\n"
		      "feedback_pass = \"0\"\n"
		      "shader1 = \"" SHADER_DIR "/bezel-picture.glsl\"\n"
		      "filter_linear1 = \"false\"\n", fp);
	fputs(params, fp);
	rc = ferror(fp) ? -1 : 0;
	if (fclose(fp) != 0)
		rc = -1;
	if (rc == 0 && rename(tmp, SESSION_SHADER) != 0)
		rc = -1;
	return rc;
}

/*
 * Picture presentation (A, B, I, L). Without a per-game override the game
 * gets integer scaling with the core's aspect (a PS1 game is 4:3 whatever
 * its resolution): whole vertical scale, width from the aspect; content
 * that would leave more than 12 % of the screen unused (GBA or PSP on
 * 640x480) is scaled to fit with its aspect instead (stock smart mode),
 * through sharp bilinear so no pixel is uneven. The picture is centred
 * horizontally and at the top of the screen (RetroArch patch 0003); the
 * per-game "integer" aspect is the same picture without bezels. Bezels
 * make the viewport the whole screen and bezel.glsl places the picture
 * the same way; the CRT look applies to TV systems (display.conf) unless
 * the game has its own filter. Returns the --set-shader argument ("" =
 * none).
 */
static const char *write_presentation(FILE *fp, const struct game_overrides *o)
{
	static char shader[PATH_MAX];
	static const char *const keys[] = {
		"aspect_ratio_index", "video_scale_integer",
		"video_scale_integer_scaling", "video_scale_integer_axis",
		"video_viewport_bias_x", "video_viewport_bias_y",
		"video_viewport_bias_portrait_x", "video_viewport_bias_portrait_y",
		"video_shader_enable", "video_smooth", NULL,
	};
	char screen[16];
	bool crt;
	bool bezel = false;
	bool integer = true;
	int index = 22; /* the core's aspect */

	for (int i = 0; keys[i]; i++)
		remember_key(keys[i]);
	system_screen(session.system, screen, sizeof(screen));
	crt = session.settings.crt && !strcmp(screen, "crt") && !o->filter[0];
	if (o->aspect[0] && strcmp(o->aspect, "integer")) {
		index = !strcmp(o->aspect, "4:3") ? 0
		      : !strcmp(o->aspect, "16:9") ? 1
		      : !strcmp(o->aspect, "full") ? 24
		      : 22; /* core */
		integer = false;
	} else if (!strcmp(o->aspect, "integer")) {
		/* the default presentation, without bezels */
	} else if (session.settings.bezels) {
		index = 24;
		integer = false;
		bezel = true;
	}
	fprintf(fp,
		"aspect_ratio_index = \"%d\"\n"
		"video_scale_integer = \"%s\"\n"
		"video_scale_integer_scaling = \"2\"\n"
		"video_scale_integer_axis = \"0\"\n"
		"video_viewport_bias_x = \"0.500000\"\n"
		"video_viewport_bias_y = \"0.000000\"\n"
		"video_viewport_bias_portrait_x = \"0.500000\"\n"
		"video_viewport_bias_portrait_y = \"0.000000\"\n"
		"video_smooth = \"%s\"\n",
		index, integer ? "true" : "false",
		!strcmp(o->filter, "smooth") ? "true" : "false");

	shader[0] = '\0';
	if (bezel) {
		char params[2048];
		char icon[PATH_MAX];
		struct mood_colours mood;
		float rgb[3];
		bool has_icon;

		system_colour(session.system, rgb);
		mood_colours(&mood);
		snprintf(icon, sizeof(icon), SYSTEM_ICONS "/%s.png", session.system);
		has_icon = file_exists(icon);
		/* An unused sampler still needs an image: the logo map. */
		if (!has_icon)
			copy_text(icon, sizeof(icon), SHADER_DIR "/bezel-logo.png");
		snprintf(params, sizeof(params),
			 "textures = \"SYSICON;BRAND\"\n"
			 "SYSICON = \"%s\"\nSYSICON_linear = \"true\"\n"
			 "SYSICON_mipmap = \"true\"\n"
			 "BRAND = \"" SHADER_DIR "/bezel-logo.png\"\n"
			 "BRAND_linear = \"false\"\n"
			 "parameters = \"BEZEL_R;BEZEL_G;BEZEL_B;BEZEL_STYLE;HAS_ICON;CRT;OVERSCAN_X;"
			 "MOOD_BG_R;MOOD_BG_G;MOOD_BG_B;MOOD_TEXT_R;MOOD_TEXT_G;MOOD_TEXT_B;"
			 "MOOD_ACCENT_R;MOOD_ACCENT_G;MOOD_ACCENT_B\"\n"
			 "BEZEL_R = \"%.3f\"\nBEZEL_G = \"%.3f\"\nBEZEL_B = \"%.3f\"\n"
			 "BEZEL_STYLE = \"%d\"\nHAS_ICON = \"%d\"\nCRT = \"%d\"\nOVERSCAN_X = \"%d\"\n"
			 "MOOD_BG_R = \"%.3f\"\nMOOD_BG_G = \"%.3f\"\nMOOD_BG_B = \"%.3f\"\n"
			 "MOOD_TEXT_R = \"%.3f\"\nMOOD_TEXT_G = \"%.3f\"\nMOOD_TEXT_B = \"%.3f\"\n"
			 "MOOD_ACCENT_R = \"%.3f\"\nMOOD_ACCENT_G = \"%.3f\"\nMOOD_ACCENT_B = \"%.3f\"\n",
			 icon, rgb[0], rgb[1], rgb[2], !strcmp(screen, "lcd") ? 1 : 0,
			 has_icon ? 1 : 0, crt ? 1 : 0, tv_overscan(session.system, o),
			 mood.background[0], mood.background[1], mood.background[2],
			 mood.text[0], mood.text[1], mood.text[2],
			 mood.accent[0], mood.accent[1], mood.accent[2]);
		if (write_session_shader(SHADER_DIR "/bezel.glsl", true, params) == 0)
			copy_text(shader, sizeof(shader), SESSION_SHADER);
	} else if (crt) {
		if (write_session_shader(SHADER_DIR "/crt.glsl", false, "") == 0)
			copy_text(shader, sizeof(shader), SESSION_SHADER);
	} else if (!o->filter[0] || !strcmp(o->filter, "sharp")) {
		copy_text(shader, sizeof(shader), DEFAULT_SHADER_PRESET);
	}
	fprintf(fp, "video_shader_enable = \"%s\"\n", shader[0] ? "true" : "false");
	return shader;
}

/* Achievements, appended to the session config. */
static void write_session_overrides(FILE *fp, const struct achievements *a)
{
	/* The token is written for every session of a logged-in user and
	 * removed from the user's file afterwards: USERDATA is exFAT and has
	 * no permissions, the token lives only in STATE. */
	remember_key("cheevos_enable");
	remember_key("cheevos_username");
	remember_key("cheevos_token");
	remember_key("cheevos_password");
	remember_key("cheevos_hardcore_mode_enable");
	if (a->enabled) {
		fprintf(fp,
			"cheevos_enable = \"true\"\n"
			"cheevos_username = \"%s\"\n"
			"cheevos_token = \"%s\"\n"
			"cheevos_password = \"\"\n"
			"cheevos_hardcore_mode_enable = \"%s\"\n",
			a->username, a->token, a->hardcore ? "true" : "false");
	} else {
		fprintf(fp, "cheevos_enable = \"false\"\n"
			    "cheevos_token = \"\"\n"
			    "cheevos_password = \"\"\n");
	}
}

/* Netplay: the user's nuubOS name, the local network only (no public
 * lobby, relay or UPnP), the default port, everyone plays. */
static void write_session_netplay(FILE *fp)
{
	static const char *const keys[] = {
		"netplay_nickname", "netplay_public_announce", "netplay_use_mitm_server",
		"netplay_nat_traversal", "netplay_start_as_spectator", "netplay_password",
		"netplay_spectate_password", "netplay_ip_port", NULL,
	};
	char path[PATH_MAX];
	char line[256];
	char nick[32] = "nuubOS";
	FILE *profile;

	snprintf(path, sizeof(path), STATE_USERS "/%s/" PROFILE_FILE, session.user);
	if ((profile = fopen(path, "r"))) {
		while (fgets(line, sizeof(line), profile)) {
			trim(line);
			if (strncmp(line, "USER_NAME=", 10) || !line[10] || !cfg_value_safe(line + 10))
				continue;
			copy_text(nick, sizeof(nick), line + 10);
			/* A name cut to RetroArch's 31 bytes never ends in half a
			 * UTF-8 character. */
			if (strlen(line + 10) >= sizeof(nick)) {
				size_t n = strlen(nick);

				while (n > 0 && ((unsigned char)nick[n - 1] & 0xc0) == 0x80)
					n--;
				if (n > 0 && ((unsigned char)nick[n - 1] & 0xc0) == 0xc0)
					n--;
				nick[n] = '\0';
			}
		}
		fclose(profile);
	}
	for (int i = 0; keys[i]; i++)
		remember_key(keys[i]);
	fprintf(fp,
		"netplay_nickname = \"%s\"\n"
		"netplay_public_announce = \"false\"\n"
		"netplay_use_mitm_server = \"false\"\n"
		"netplay_nat_traversal = \"false\"\n"
		"netplay_start_as_spectator = \"false\"\n"
		"netplay_password = \"\"\n"
		"netplay_spectate_password = \"\"\n"
		"netplay_ip_port = \"%d\"\n",
		nick, NETPLAY_PORT);
}

/* ------------------------------------------------------------------ */
/* RetroArch session                                                   */
/* ------------------------------------------------------------------ */

static void ra_command(const char *fmt, ...)
{
	char line[256];
	va_list ap;
	int n;

	if (session.stdin_fd < 0)
		return;
	va_start(ap, fmt);
	n = vsnprintf(line, sizeof(line) - 1, fmt, ap);
	va_end(ap);
	if (n <= 0)
		return;
	if ((size_t)n > sizeof(line) - 2)
		n = (int)sizeof(line) - 2;
	line[n++] = '\n';
	if (write(session.stdin_fd, line, (size_t)n) != n)
		log_msg("emud: command to RetroArch failed: %s", strerror(errno));
}

/* RetroArch's state file names: <base>.state, .stateN, .state.auto. */
static void state_path(const char *base, int slot, char *out, size_t size)
{
	if (slot == AUTO_SLOT)
		snprintf(out, size, "%s.state.auto", base);
	else if (slot == 0)
		snprintf(out, size, "%s.state", base);
	else
		snprintf(out, size, "%s.state%d", base, slot);
}

static void state_file_name(int slot, char *out, size_t size)
{
	state_path(session.state_base, slot, out, size);
}

/* Modification time of a state, -1 when there is none. */
static long long state_mtime(const char *base, int slot)
{
	char path[PATH_MAX];
	struct stat st;

	state_path(base, slot, path, sizeof(path));
	if (stat(path, &st) != 0 || !S_ISREG(st.st_mode))
		return -1;
	return (long long)st.st_mtime;
}

/* The most recent state (a slot or AUTO_SLOT), NO_STATE when none; the
 * most recent numbered slot in *manual (0 when none). */
#define NO_STATE -2
static int newest_state(const char *base, int slots, int *manual)
{
	long long best = -1;
	long long best_manual = -1;
	int newest = NO_STATE;

	*manual = 0;
	for (int slot = AUTO_SLOT; slot < slots; slot++) {
		long long t = state_mtime(base, slot);

		if (t < 0)
			continue;
		if (t > best) {
			best = t;
			newest = slot;
		}
		if (slot != AUTO_SLOT && t > best_manual) {
			best_manual = t;
			*manual = slot;
		}
	}
	return newest;
}

/* Where the next save goes: the selected slot, or with rotation the slot
 * after it once it holds a state (wrapping to 0 after the last). */
static int save_target(void)
{
	if (!session.settings.slot_rotation || state_mtime(session.state_base, session.slot) < 0)
		return session.slot;
	return (session.slot + 1) % session.settings.slots;
}

/* Saves into save_target() and selects that slot. */
static bool begin_save(int slot, bool quiet);
static bool save_next(bool quiet)
{
	int target = save_target();

	if (!begin_save(target, quiet))
		return false;
	session.slot = target;
	return true;
}

static void stop_watch(void)
{
	if (session.inotify_fd >= 0)
		close(session.inotify_fd);
	session.inotify_fd = -1;
	session.inotify_wd = -1;
}

static void finish_power_wait(void);
static void finish_screenshot(bool ok);
static void request_quit(void);

static void finish_pending(bool ok)
{
	enum pending_kind kind = session.pending;
	int slot = session.pending_slot;
	bool quiet = session.pending_quiet;

	session.pending = PENDING_NONE;
	stop_watch();
	if (!quiet) {
		if (kind == PENDING_SAVE)
			notify_state(ok ? "game.state.saved" : "game.state.failed",
				     slot, ok ? NULL : "save");
		else if (kind == PENDING_LOAD)
			notify_state(ok ? "game.state.loaded" : "game.state.failed",
				     slot, ok ? NULL : "load");
	}
	if (kind == PENDING_SAVE)
		log_msg("emud: save slot %d %s", slot, ok ? "written" : "failed");
	finish_power_wait();
	if (session.quit_requested)
		request_quit();
	notify_subscribers();
}

static bool begin_save(int slot, bool quiet)
{
	char path[PATH_MAX];

	if (session.state != SESSION_RUNNING || session.pending != PENDING_NONE)
		return false;
	state_file_name(slot, path, sizeof(path));
	session.inotify_fd = inotify_init1(IN_NONBLOCK | IN_CLOEXEC);
	if (session.inotify_fd >= 0)
		session.inotify_wd = inotify_add_watch(session.inotify_fd,
						       session.states_dir,
						       IN_CLOSE_WRITE | IN_MOVED_TO);
	if (session.inotify_fd < 0 || session.inotify_wd < 0) {
		stop_watch();
		return false;
	}
	copy_text(session.pending_name, sizeof(session.pending_name),
		  strrchr(path, '/') + 1);
	session.pending = PENDING_SAVE;
	session.pending_slot = slot;
	session.pending_quiet = quiet;
	session.pending_replied = false;
	session.pending_deadline_ms = monotonic_ms() + SAVE_TIMEOUT_MS;
	ra_command("SAVE_STATE_SLOT %d", slot);
	notify_subscribers();
	return true;
}

static void begin_load(int slot)
{
	char path[PATH_MAX];

	state_file_name(slot, path, sizeof(path));
	if (!file_exists(path)) {
		notify_state("game.state.empty", slot, NULL);
		return;
	}
	session.pending = PENDING_LOAD;
	session.pending_slot = slot;
	session.pending_quiet = false;
	session.pending_replied = false;
	session.pending_deadline_ms = monotonic_ms() + SAVE_TIMEOUT_MS;
	ra_command("LOAD_STATE_SLOT %d", slot);
	notify_subscribers();
}

static void handle_ra_reply(char *line)
{
	int slot;
	char result[16];

	if (sscanf(line, "SAVE_STATE_SLOT %d %15s", &slot, result) == 2) {
		if (session.pending != PENDING_SAVE || slot != session.pending_slot)
			return;
		if (strcmp(result, "OK") != 0)
			finish_pending(false);
		else
			session.pending_replied = true;
		return;
	}
	if (sscanf(line, "LOAD_STATE_SLOT %d %15s", &slot, result) == 2) {
		if (session.pending == PENDING_LOAD && slot == session.pending_slot)
			finish_pending(strcmp(result, "OK") == 0);
		return;
	}
	if (!strcmp(line, "CHEATS_LOAD FAILED") || !strcmp(line, "NETPLAY_STOP FAILED")) {
		log_msg("emud: retroarch refused %s", line);
		return;
	}
	if (!strncmp(line, "NETPLAY ", 8)) {
		/* Netplay state (RetroArch patch 0008). */
		const char *ev = line + 8;
		const char *nick = strchr(ev, ' ');

		nick = nick ? nick + 1 : "";
		if (!strcmp(ev, "hosting")) {
			session.netplay = NET_HOST;
			session.net_peers = 0;
			session.net_peer[0] = '\0';
		} else if (!strncmp(ev, "joined", 6)) {
			session.net_peers++;
			copy_text(session.net_peer, sizeof(session.net_peer), nick);
		} else if (!strncmp(ev, "left", 4)) {
			if (session.net_peers > 0)
				session.net_peers--;
		} else if (!strncmp(ev, "connected", 9)) {
			session.netplay = NET_CLIENT;
			session.net_peers = 1;
			copy_text(session.net_peer, sizeof(session.net_peer), nick);
		} else if (!strcmp(ev, "disconnected") && session.netplay == NET_CLIENT) {
			session.netplay = NET_OFF;
			session.net_launch[0] = '\0';
		} else if (!strcmp(ev, "off") || !strcmp(ev, "failed")) {
			session.netplay = NET_OFF;
			session.net_launch[0] = '\0';
			session.net_peers = 0;
		}
		log_msg("emud: netplay %s", ev);
		notify_subscribers();
		return;
	}
	if (!strncmp(line, "GET_FPS ", 8)) {
		session.fps = atof(line + 8);
		notify_subscribers();
		return;
	}
	if (sscanf(line, "SET_PAUSED %d", &slot) == 1) {
		bool paused = slot != 0;

		if (paused != session.paused) {
			session.paused = paused;
			notify_subscribers();
		}
	}
}

static void process_ra_output(void)
{
	char buf[512];
	ssize_t n = read(session.stdout_fd, buf, sizeof(buf));

	if (n <= 0) {
		if (n < 0 && (errno == EINTR || errno == EAGAIN))
			return;
		close(session.stdout_fd);
		session.stdout_fd = -1;
		return;
	}
	for (ssize_t i = 0; i < n; i++) {
		if (buf[i] == '\n') {
			session.out_buf[session.out_used] = '\0';
			handle_ra_reply(session.out_buf);
			session.out_used = 0;
		} else if (session.out_used + 1 < sizeof(session.out_buf)) {
			session.out_buf[session.out_used++] = buf[i];
		} else {
			session.out_used = 0;
		}
	}
}

static void process_watch(void)
{
	char buf[4096] __attribute__((aligned(__alignof__(struct inotify_event))));
	ssize_t n;
	bool done = false;

	while ((n = read(session.inotify_fd, buf, sizeof(buf))) > 0) {
		for (char *p = buf; p < buf + n;) {
			struct inotify_event *ev = (struct inotify_event *)p;

			if (ev->len && strcmp(ev->name, session.pending_name) == 0)
				done = true;
			p += sizeof(*ev) + ev->len;
		}
	}
	if (done && session.pending == PENDING_SAVE)
		finish_pending(true);
}

/* ------------------------------------------------------------------ */
/* Cheats                                                              */
/* ------------------------------------------------------------------ */

/*
 * A game's cheats are a .cht file of the user's (/userdata/cheats) or of
 * the libretro cheat collection (nuubos-libretroctl: an archive and its
 * index; the game's files are extracted when its cheats are listed into
 * /userdata/cheats/libretro/<database>). The user picks the list and turns cheats on per
 * game (game overrides cheat_file/cheats); the session gets a copy of the
 * list with those cheats on (session.cht) through RetroArch's CHEATS_LOAD
 * (patch 0008), at launch and at every change. No cheats in hardcore mode
 * or with netplay (RetroArch refuses them there).
 */
struct cheat_list {
	char path[PATH_MAX];
	char name[NAME_MAX + 1];
	int rank;
};

/* A file name's title for matching: letters and digits in lower case,
 * without its (...) [...] {...} tags. */
static void title_key(const char *name, char *out, size_t size)
{
	int depth = 0;
	size_t n = 0;

	for (const char *p = name; *p && n + 1 < size; p++) {
		if (*p == '(' || *p == '[' || *p == '{')
			depth++;
		else if ((*p == ')' || *p == ']' || *p == '}') && depth)
			depth--;
		else if (!depth && isalnum((unsigned char)*p))
			out[n++] = (char)tolower((unsigned char)*p);
	}
	out[n] = '\0';
}

/* First word of a name's first (...) tag: the region ("USA"). */
static void region_tag(const char *name, char *out, size_t size)
{
	const char *open = strchr(name, '(');
	size_t n = 0;

	out[0] = '\0';
	if (!open)
		return;
	for (const char *p = open + 1; *p && *p != ')' && *p != ',' && n + 1 < size; p++)
		out[n++] = *p;
	out[n] = '\0';
}

/* Inserts a list by rank, then name, keeping at most MAX_CHEAT_LISTS. */
static void add_cheat_list(struct cheat_list *lists, int *count, const char *path,
			   const char *name, int rank)
{
	int at = *count;

	for (int i = 0; i < *count; i++)
		if (!strcmp(lists[i].path, path))
			return;
	while (at > 0 && (lists[at - 1].rank > rank ||
			  (lists[at - 1].rank == rank && strcasecmp(lists[at - 1].name, name) > 0)))
		at--;
	if (at >= MAX_CHEAT_LISTS)
		return;
	if (*count < MAX_CHEAT_LISTS)
		(*count)++;
	memmove(&lists[at + 1], &lists[at], (size_t)(*count - 1 - at) * sizeof(*lists));
	copy_text(lists[at].path, sizeof(lists[at].path), path);
	copy_text(lists[at].name, sizeof(lists[at].name), name);
	lists[at].rank = rank;
}

/* Rank of a list for the ROM: the one named exactly like the ROM first,
 * then those of its region, then the others (Game Genie, Action
 * Replay...); -1 when its title is another. */
static int cheat_rank(const char *name, const char *stem, const char *key, const char *region)
{
	char other[NAME_MAX + 1];

	title_key(name, other, sizeof(other));
	if (strcmp(other, key))
		return -1;
	return !strcasecmp(name, stem) ? 0 : region[0] && strstr(name, region) ? 1 : 2;
}

/* The collection's files of the ROM's title in the system's databases
 * (index.txt lines "<database>/<name>.cht"). */
static void scan_cheat_index(const char *dbs, const char *stem, const char *key,
			     const char *region, int base, struct cheat_list *lists, int *count)
{
	char line[PATH_MAX];
	FILE *fp = fopen(CHEATS_INDEX, "r");

	if (!fp)
		return;
	while (fgets(line, sizeof(line), fp)) {
		char name[NAME_MAX + 1];
		char path[PATH_MAX];
		char *slash;
		size_t len;
		int rank;

		trim(line);
		len = strlen(line);
		if (!(slash = strchr(line, '/')) || len < 5 || strcasecmp(line + len - 4, ".cht"))
			continue;
		*slash = '\0';
		if (!pipe_list_has(dbs, line))
			continue;
		copy_text(name, sizeof(name), slash + 1);
		name[strlen(name) - 4] = '\0';
		if ((rank = cheat_rank(name, stem, key, region)) < 0)
			continue;
		snprintf(path, sizeof(path), CHEATS_LIBRETRO "/%s/%s", line, slash + 1);
		add_cheat_list(lists, count, path, name, base + rank);
	}
	fclose(fp);
}

/* Extracts collection files (paths under CHEATS_LIBRETRO) from the
 * archive, all in one run of unzip; its names are patterns there. */
static void extract_cheats(const char *const *paths, int n)
{
	char names[MAX_CHEAT_LISTS][PATH_MAX];
	char *argv[8 + MAX_CHEAT_LISTS];
	int argc = 0;
	int status;
	pid_t pid;

	argv[argc++] = (char *)"unzip";
	argv[argc++] = (char *)"-o";
	argv[argc++] = (char *)"-q";
	argv[argc++] = (char *)CHEATS_ARCHIVE;
	for (int i = 0; i < n && i < MAX_CHEAT_LISTS; i++) {
		const char *entry = paths[i] + strlen(CHEATS_LIBRETRO "/");
		size_t len = 0;

		for (const char *p = entry; *p && len + 2 < sizeof(names[i]); p++) {
			if (strchr("[]*?\\", *p))
				names[i][len++] = '\\';
			names[i][len++] = *p;
		}
		names[i][len] = '\0';
		argv[argc++] = names[i];
	}
	argv[argc++] = (char *)"-d";
	argv[argc++] = (char *)CHEATS_LIBRETRO;
	argv[argc] = NULL;
	if (argc == 6 || !file_exists(CHEATS_ARCHIVE))
		return;
	pid = fork();
	if (pid == 0) {
		int null = open("/dev/null", O_RDWR | O_CLOEXEC);

		if (null >= 0) {
			dup2(null, STDIN_FILENO);
			dup2(null, STDOUT_FILENO);
			dup2(null, STDERR_FILENO);
		}
		execvp("unzip", argv);
		_exit(127);
	}
	/* A few small files: waited for here (the main loop's reaper never
	 * sees this child first). */
	while (pid > 0 && waitpid(pid, &status, 0) < 0 && errno == EINTR)
		;
}

/* The .cht files of a folder with the ROM's title. */
static void scan_cheat_dir(const char *dir, const char *stem, const char *key,
			   const char *region, int base, struct cheat_list *lists, int *count)
{
	DIR *d = opendir(dir);
	struct dirent *e;

	if (!d)
		return;
	while ((e = readdir(d)) != NULL) {
		char name[NAME_MAX + 1];
		char path[PATH_MAX];
		size_t len = strlen(e->d_name);
		int rank;

		if (e->d_name[0] == '.' || len < 5 || strcasecmp(e->d_name + len - 4, ".cht"))
			continue;
		copy_text(name, sizeof(name), e->d_name);
		name[len - 4] = '\0';
		if ((rank = cheat_rank(name, stem, key, region)) < 0)
			continue;
		snprintf(path, sizeof(path), "%s/%s", dir, e->d_name);
		add_cheat_list(lists, count, path, name, base + rank);
	}
	closedir(d);
}

/* The cheat lists for a game, best first: the user's own files
 * (/userdata/cheats/<system>/<rom>.cht, /userdata/cheats/<rom>.cht, then
 * files of the same title in /userdata/cheats/<system>), then the libretro
 * collection's files of the same title in the system's databases. */
static int cheat_lists(const char *system, const char *rom, struct cheat_list *lists)
{
	char stem[NAME_MAX + 1];
	char key[NAME_MAX + 1];
	char region[32];
	char dbs[512];
	char path[PATH_MAX];
	int count = 0;

	const char *missing[MAX_CHEAT_LISTS];
	int n = 0;

	content_base_name(rom, stem, sizeof(stem));
	title_key(stem, key, sizeof(key));
	region_tag(stem, region, sizeof(region));
	snprintf(path, sizeof(path), CHEATS_DIR "/%s/%s.cht", system, stem);
	if (file_exists(path))
		add_cheat_list(lists, &count, path, stem, 0);
	snprintf(path, sizeof(path), CHEATS_DIR "/%s.cht", stem);
	if (file_exists(path))
		add_cheat_list(lists, &count, path, stem, 0);
	if (!key[0])
		return count;
	snprintf(path, sizeof(path), CHEATS_DIR "/%s", system);
	scan_cheat_dir(path, stem, key, region, 1, lists, &count);
	system_databases(system, dbs, sizeof(dbs));
	if (dbs[0])
		scan_cheat_index(dbs, stem, key, region, 10, lists, &count);
	/* The collection's lists are extracted the first time they are
	 * offered; one that could not be is left out. */
	for (int i = 0; i < count; i++)
		if (!file_exists(lists[i].path))
			missing[n++] = lists[i].path;
	if (n)
		extract_cheats(missing, n);
	for (int i = 0; i < count;) {
		if (file_exists(lists[i].path)) {
			i++;
			continue;
		}
		memmove(&lists[i], &lists[i + 1], (size_t)(count - i - 1) * sizeof(*lists));
		count--;
	}
	return count;
}

/* The game's list: the user's choice while it exists, else the best one
 * ("" when there is none). */
static void current_cheat_file(const struct game_overrides *o, const struct cheat_list *lists,
			       int count, char *out, size_t size)
{
	if (o->cheat_file[0] && !file_exists(o->cheat_file) &&
	    !strncmp(o->cheat_file, CHEATS_LIBRETRO "/", strlen(CHEATS_LIBRETRO "/"))) {
		/* A new collection replaced the extracted files. */
		const char *path = o->cheat_file;

		extract_cheats(&path, 1);
	}
	if (o->cheat_file[0] && file_exists(o->cheat_file))
		copy_text(out, size, o->cheat_file);
	else
		copy_text(out, size, count > 0 ? lists[0].path : "");
}

/* Cheat k of a .cht line "cheat<k>_<field> = ...": the field, NULL when
 * the line is something else. */
static char *cheat_line(char *line, int *k)
{
	char *p = line;
	char *end;
	long n;

	while (*p == ' ')
		p++;
	if (strncmp(p, "cheat", 5) || !isdigit((unsigned char)p[5]))
		return NULL;
	n = strtol(p + 5, &end, 10);
	if (*end != '_' || n < 0 || n > 100000)
		return NULL;
	*k = (int)n;
	return end + 1;
}

/* The session's copy of the list with the user's cheats on, loaded by
 * RetroArch; nothing on clears the session's cheats. */
static void apply_session_cheats(const struct game_overrides *o, const char *file)
{
	char tmp[] = SESSION_CHEATS ".tmp";
	char line[2048];
	char list[sizeof(o->cheats)];
	char *save = NULL;
	FILE *in;
	FILE *out;
	int on = 0;

	if (!file[0] || !o->cheats[0] || mkdir_p(SESSION_DIR) != 0 || !(in = fopen(file, "r"))) {
		ra_command("CHEATS_LOAD");
		session.cheats_on = 0;
		notify_subscribers();
		return;
	}
	out = fopen(tmp, "w");
	if (!out) {
		fclose(in);
		return;
	}
	while (fgets(line, sizeof(line), in)) {
		int k;
		char *field = cheat_line(line, &k);

		/* Every cheat off, then the user's on. */
		if (field && !strncmp(field, "enable", 6) && (field[6] == ' ' || field[6] == '='))
			continue;
		fputs(line, out);
	}
	fclose(in);
	copy_text(list, sizeof(list), o->cheats);
	fputc('\n', out);
	for (char *k = strtok_r(list, ",", &save); k; k = strtok_r(NULL, ",", &save)) {
		fprintf(out, "cheat%d_enable = \"true\"\n", atoi(k));
		on++;
	}
	if (fclose(out) != 0 || rename(tmp, SESSION_CHEATS) != 0) {
		unlink(tmp);
		return;
	}
	ra_command("CHEATS_LOAD %s", SESSION_CHEATS);
	session.cheats_on = on;
	notify_subscribers();
}

/* Writes the settings nuubOS enforces at every launch; RetroArch appends
 * them over the user's configuration (--appendconfig). */
/* RetroArch notifications switched off for every session. */
static const char *const ra_notification_keys[] = {
	"notification_show_autoconfig", "notification_show_autoconfig_fails",
	"notification_show_cheats_applied", "notification_show_patch_applied",
	"notification_show_remap_load", "notification_show_config_override_load",
	"notification_show_set_initial_disk", "notification_show_disk_control",
	"notification_show_save_state", "notification_show_fast_forward",
	"notification_show_screenshot", "notification_show_refresh_rate",
	"notification_show_netplay_extra", "menu_show_load_content_animation",
	NULL,
};

static int write_session_cfg(const char *ra_dir, const char *saves,
			     const char *states, const char *screenshots,
			     const struct game_overrides *o, const struct achievements *a,
			     bool auto_load, const char **shader)
{
	char tmp[] = SESSION_CFG ".tmp";
	bool auto_save = !session.settings.slot_rotation && !session.hardcore;
	FILE *fp;
	int player;
	int rc;
	int i;

	if (mkdir_p(SESSION_DIR) != 0)
		return -1;
	fp = fopen(tmp, "w");
	if (!fp)
		return -1;
	fprintf(fp,
		"# Written by nuubos-emud for the running session.\n"
		"libretro_directory = \"" DOWNLOADED_CORES "\"\n"
		"libretro_info_path = \"" CORE_INFO_DIR "\"\n"
		"assets_directory = \"" RA_ASSETS_DIR "\"\n"
		"joypad_autoconfig_dir = \"" RA_AUTOCONFIG_DIR "\"\n"
		"system_directory = \"" BIOS_DIR "\"\n"
		"cheat_database_path = \"" CHEATS_DIR "\"\n"
		"cache_directory = \"" CACHE_DIR "\"\n"
		"video_shader_dir = \"" SHADER_DIR "\"\n"
		"savefile_directory = \"%s\"\n"
		"savestate_directory = \"%s\"\n"
		"screenshot_directory = \"%s\"\n"
		"sort_savefiles_enable = \"false\"\n"
		"sort_savestates_enable = \"false\"\n"
		"sort_savefiles_by_content_enable = \"false\"\n"
		"sort_savestates_by_content_enable = \"false\"\n"
		"savefiles_in_content_dir = \"false\"\n"
		"savestates_in_content_dir = \"false\"\n"
		"systemfiles_in_content_dir = \"false\"\n"
		"global_core_options = \"true\"\n"
		"core_options_path = \"%s/retroarch-core-options.cfg\"\n"
		"rgui_config_directory = \"%s/config\"\n"
		"input_remapping_directory = \"%s/config/remaps\"\n"
		"playlist_directory = \"%s/playlists\"\n"
		"content_database_path = \"%s/database\"\n"
		"thumbnails_directory = \"%s/thumbnails\"\n"
		"log_dir = \"%s/logs\"\n"
		"video_driver = \"gl\"\n"
		"video_fullscreen = \"true\"\n"
		"input_driver = \"wayland\"\n"
		"input_joypad_driver = \"udev\"\n"
		"input_autodetect_enable = \"true\"\n"
		"stdin_cmd_enable = \"true\"\n"
		"network_cmd_enable = \"false\"\n"
		"pause_nonactive = \"false\"\n"
		"quit_on_close_content = \"1\"\n"
		"input_menu_toggle_gamepad_combo = \"0\"\n"
		"input_quit_gamepad_combo = \"0\"\n"
		"quit_press_twice = \"false\"\n"
		"menu_swap_ok_cancel_buttons = \"false\"\n"
		"config_save_on_exit = \"true\"\n"
		"user_language = \"%d\"\n",
		saves, states, screenshots, ra_dir, ra_dir, ra_dir, ra_dir,
		ra_dir, ra_dir, ra_dir, retroarch_language(session.user));
	/* Port N is reserved for nuubOS game gamepad N (product id N), so
	 * player order always follows nuubOS player assignment. */
	for (player = 1; player <= MAX_PLAYERS; player++)
		fprintf(fp,
			"input_player%d_device_reservation_type = \"2\"\n"
			"input_player%d_reserved_device = \"0000:%04x\"\n",
			player, player, player);
	*shader = write_presentation(fp, o);
	/* The performance overlay line is drawn into the frame with the OSD
	 * font (RetroArch notifications use widgets, not this font). */
	remember_key("video_font_enable");
	remember_key("video_font_path");
	remember_key("video_font_size");
	fprintf(fp, "video_font_enable = \"true\"\nvideo_font_path = \"%s\"\n"
		"video_font_size = \"32.000000\"\n", OSD_FONT);
	/* RetroArch's own notifications are off (nuubOS notifies save/load/
	 * slot/fast-forward/screenshot itself); RetroAchievements popups are
	 * separate widgets and stay (user request 2026-10-11). */
	for (i = 0; ra_notification_keys[i]; i++) {
		remember_key(ra_notification_keys[i]);
		fprintf(fp, "%s = \"false\"\n", ra_notification_keys[i]);
	}
	remember_key("notification_show_screenshot_flash");
	fprintf(fp, "notification_show_screenshot_flash = \"2\"\n");
	/* Progress is kept when the game closes (.state.auto, or with slot
	 * rotation the next slot, saved by emud: request_quit); auto_load
	 * resumes from it when it is the most recent state. */
	remember_key("savestate_auto_save");
	remember_key("savestate_auto_load");
	fprintf(fp, "savestate_auto_save = \"%s\"\nsavestate_auto_load = \"%s\"\n",
		auto_save ? "true" : "false", auto_load ? "true" : "false");
	write_session_overrides(fp, a);
	write_session_netplay(fp);
	rc = ferror(fp) ? -1 : 0;
	if (fclose(fp) != 0)
		rc = -1;
	if (rc == 0 && rename(tmp, SESSION_CFG) != 0)
		rc = -1;
	return rc;
}

/*
 * MAME computers/consoles: the core starts "<machine> <media switch> file"
 * from a .cmd file. It is named after the game so states and saves stay
 * per game. The switch is picked by extension from the system's media map
 * ("dsk:-flop1,cas:-cass"); the first entry is the default.
 */
static int write_mame_cmd(const char *content, char *cmd_path, size_t size)
{
	char base[NAME_MAX + 1];
	char media[256];
	char chosen[32] = "";
	const char *ext = strrchr(content, '.');
	char *save = NULL;
	char *item;
	FILE *fp;

	content_base_name(content, base, sizeof(base));
	copy_text(media, sizeof(media), resolved_media);
	for (item = strtok_r(media, ",", &save); item; item = strtok_r(NULL, ",", &save)) {
		char *colon = strchr(item, ':');

		if (!colon)
			continue;
		*colon = '\0';
		if (!chosen[0])
			copy_text(chosen, sizeof(chosen), colon + 1);
		if (ext && !strcasecmp(ext + 1, item)) {
			copy_text(chosen, sizeof(chosen), colon + 1);
			break;
		}
	}
	if (!chosen[0] || strchr(content, '"'))
		return -1;
	snprintf(cmd_path, size, SESSION_DIR "/%s.cmd", base);
	fp = fopen(cmd_path, "w");
	if (!fp)
		return -1;
	fprintf(fp, "%s %s \"%s\"\n", resolved_machine, chosen, content);
	return fclose(fp) == 0 ? 0 : -1;
}

static void content_base_name(const char *content, char *out, size_t size)
{
	const char *base = strrchr(content, '/');
	char *dot;

	copy_text(out, size, base ? base + 1 : content);
	dot = strrchr(out, '.');
	if (dot && dot != out)
		*dot = '\0';
}

static void reset_session(void)
{
	stop_watch();
	if (session.shot_fd > 0)
		close(session.shot_fd);
	free_restore();
	if (session.stdin_fd >= 0)
		close(session.stdin_fd);
	if (session.stdout_fd >= 0)
		close(session.stdout_fd);
	if (session.input_fd >= 0)
		close(session.input_fd);
	memset(&session, 0, sizeof(session));
	session.state = SESSION_IDLE;
	session.pid = -1;
	session.stdin_fd = -1;
	session.stdout_fd = -1;
	session.input_fd = -1;
	session.inotify_fd = -1;
	session.inotify_wd = -1;
	session.power_client = -1;
	session.shot_fd = -1;
}

/*
 * mode: "resume" (default) starts from the most recent state (a slot or the
 * automatic state written when the game last closed), "new" from the
 * beginning, "slot:N" from slot N ("slot:-1" = the automatic state).
 * net: "" alone, "host" hosts a netplay game on the local network,
 * "connect:<address>:<port>:<core>" joins one (from the beginning: the
 * host sends its game; with the host's emulator, NETPLAY_HOSTS).
 */
static const char *cheats_blocked(void);

static const char *launch(const char *game, const char *mode, const char *net)
{
	char answer[PATH_MAX + 128];
	char command[64];
	char core_path[PATH_MAX];
	char base[160];
	char ra_dir[256];
	char saves[PATH_MAX];
	char screenshots[PATH_MAX];
	char path[PATH_MAX];
	char *tab;
	int in_pipe[2];
	int out_pipe[2];
	pid_t pid;
	struct game_overrides overrides;
	struct achievements achievements;
	const char *shader = "";
	char entry_arg[16] = "";
	bool auto_load = false;
	int manual = 0;
	int start = NO_STATE;
	int render_height;
	int overscan;
	char net_address[INET_ADDRSTRLEN] = "";
	char net_port[8] = "";
	char net_core[64] = "";

	if (session.state != SESSION_IDLE)
		return "busy";
	if (!net)
		net = "";
	if (!strncmp(net, "connect:", 8)) {
		struct in_addr ip;
		char *p;
		char *q;
		long port;

		copy_text(net_address, sizeof(net_address), net + 8);
		if (!(p = strchr(net_address, ':')))
			return "net";
		*p = '\0';
		port = strtol(strchr(net + 8, ':') + 1, &q, 10);
		if (inet_pton(AF_INET, net_address, &ip) != 1 || *q != ':' || port < 1 || port > 65535)
			return "net";
		snprintf(net_port, sizeof(net_port), "%ld", port);
		copy_text(net_core, sizeof(net_core), q + 1);
		mode = "new";
	} else if (net[0] && strcmp(net, "host")) {
		return "net";
	}
	if (!mode || !mode[0])
		mode = "resume";
	if (strcmp(mode, "resume") && strcmp(mode, "new") && strncmp(mode, "slot:", 5))
		return "mode";
	for (const char *p = game; *p; p++)
		if (!isxdigit((unsigned char)*p))
			return "game";
	if (!game[0] || strlen(game) >= sizeof(session.game))
		return "game";
	if (!read_active_user(session.user, sizeof(session.user)))
		return "no-user";
	load_settings(session.user, &session.settings);

	snprintf(command, sizeof(command), "RESOLVE\t%s", game);
	if (library_request(command, answer, sizeof(answer)) != 0)
		return "library";
	if (strncmp(answer, "ERR ", 4) == 0)
		return strcmp(answer + 4, "unavailable") == 0 ? "unavailable" : "game";
	if (strncmp(answer, "OK ", 3) != 0 || !(tab = strchr(answer + 3, '\t')))
		return "library";
	*tab = '\0';
	copy_text(session.game, sizeof(session.game), game);
	copy_text(session.system, sizeof(session.system), answer + 3);
	copy_text(session.content, sizeof(session.content), tab + 1);
	copy_text(session.rom, sizeof(session.rom), tab + 1);
	if (!file_exists(session.content))
		return "unavailable";
	if (!resolve_core(session.system, session.core, sizeof(session.core),
			  core_path, sizeof(core_path)))
		return "no-core";
	load_overrides(session.user, game, &overrides);
	load_achievements(session.user, &achievements);
	session.hardcore = achievements.enabled && achievements.hardcore;
	if (session.hardcore && !strncmp(mode, "slot:", 5))
		return "hardcore";
	/* Joining: the host's emulator, whatever the game's choice. */
	if (net_core[0]) {
		char cores[512];

		available_cores(session.system, cores, sizeof(cores));
		if (!list_has(cores, net_core))
			return "no-core";
		copy_text(overrides.core, sizeof(overrides.core), net_core);
	}
	if (net[0] && session.hardcore)
		return "hardcore";
	if (overrides.core[0] && strcmp(overrides.core, session.core)) {
		char cores[512];

		/* An override naming a core that is gone falls back to the
		 * system default instead of failing the launch. */
		available_cores(session.system, cores, sizeof(cores));
		if (list_has(cores, overrides.core)) {
			copy_text(session.core, sizeof(session.core), overrides.core);
			core_file(session.core, core_path, sizeof(core_path));
		} else {
			log_msg("emud: core override %s unavailable", overrides.core);
		}
	}
	if (!strcmp(session.core, "mame") && resolved_machine[0]) {
		char cmd[PATH_MAX];

		if (mkdir_p(SESSION_DIR) != 0 ||
		    write_mame_cmd(session.content, cmd, sizeof(cmd)) != 0)
			return "storage";
		copy_text(session.content, sizeof(session.content), cmd);
	}

	/* Per-user configuration and progress; device-global cores, BIOS
	 * and cache (EPIC-003 ownership model). */
	snprintf(base, sizeof(base), USERS_DIR "/%s", session.user);
	snprintf(ra_dir, sizeof(ra_dir), "%s/appdata/retroarch", base);
	snprintf(saves, sizeof(saves), "%s/saves/%s", base, session.system);
	snprintf(session.states_dir, sizeof(session.states_dir), "%s/states/%s",
		 base, session.system);
	snprintf(screenshots, sizeof(screenshots), "%s/screenshots", base);
	snprintf(path, sizeof(path), "%s/config/remaps", ra_dir);
	if (mkdir_p(path) != 0 || mkdir_p(saves) != 0 ||
	    mkdir_p(session.states_dir) != 0 || mkdir_p(screenshots) != 0 ||
	    mkdir_p(DOWNLOADED_CORES) != 0 || mkdir_p(CACHE_DIR) != 0 ||
	    mkdir_p(BIOS_DIR) != 0)
		return "storage";
	/* Once per service start: a new image brings new support files. */
	{
		static bool system_seeded;

		if (!system_seeded) {
			seed_tree(SYSTEM_FILES, BIOS_DIR);
			system_seeded = true;
		}
	}
	snprintf(path, sizeof(path), "%s/retroarch.cfg", ra_dir);
	if (seed_file(DEFAULT_RA_CFG, path) != 0)
		return "storage";
	copy_text(session.ra_cfg, sizeof(session.ra_cfg), path);
	copy_text(session.screenshots_dir, sizeof(session.screenshots_dir), screenshots);
	snprintf(path, sizeof(path), "%s/retroarch-core-options.cfg", ra_dir);
	if (seed_file(DEFAULT_CORE_OPTIONS, path) == 0) {
		char done[PATH_MAX];

		merge_missing_keys(DEFAULT_CORE_OPTIONS, path);
		snprintf(done, sizeof(done), "%s/core-option-updates", ra_dir);
		apply_core_option_updates(path, done);
	}
	apply_boot_options(path, session.settings.boot_screens);
	snprintf(path, sizeof(path), "%s/config/global.glslp", ra_dir);
	(void)seed_file(DEFAULT_SHADER_PRESET, path);
	content_base_name(session.content, path, sizeof(path));
	snprintf(session.state_base, sizeof(session.state_base), "%s/%s",
		 session.states_dir, path);

	/* Starting point: RetroArch loads an entry slot (--entryslot) or the
	 * automatic state (savestate_auto_load). Hardcore always starts from
	 * the beginning. */
	if (session.hardcore) {
		(void)newest_state(session.state_base, session.settings.slots, &manual);
	} else if (!strcmp(mode, "resume")) {
		start = newest_state(session.state_base, session.settings.slots, &manual);
	} else if (!strncmp(mode, "slot:", 5)) {
		start = atoi(mode + 5);
		if (start < AUTO_SLOT || start >= session.settings.slots ||
		    state_mtime(session.state_base, start) < 0)
			return "state";
		(void)newest_state(session.state_base, session.settings.slots, &manual);
	}
	if (start == AUTO_SLOT)
		auto_load = true;
	else if (start >= 0)
		snprintf(entry_arg, sizeof(entry_arg), "%d", start);
	if (write_session_cfg(ra_dir, saves, session.states_dir, screenshots,
			      &overrides, &achievements, auto_load, &shader) != 0)
		return "storage";

	session.input_fd = input_gamepads_on();
	if (session.input_fd < 0)
		log_msg("emud: inputd game gamepads unavailable");
	send_hotkeys();

	if (pipe2(in_pipe, O_CLOEXEC) != 0)
		return "spawn";
	if (pipe2(out_pipe, O_CLOEXEC) != 0) {
		close(in_pipe[0]);
		close(in_pipe[1]);
		return "spawn";
	}

	render_height = game_render_height(session.system);
	overscan = tv_overscan(session.system, &overrides);
	pid = fork();
	if (pid < 0) {
		close(in_pipe[0]);
		close(in_pipe[1]);
		close(out_pipe[0]);
		close(out_pipe[1]);
		return "spawn";
	}
	if (pid == 0) {
		char cfg[PATH_MAX];
		char *argv[24] = {
			(char *)RETROARCH_BIN, (char *)"--config", cfg,
			(char *)"--appendconfig", (char *)SESSION_CFG,
			/* "" disables RetroArch's automatic shader presets. */
			(char *)"--set-shader", (char *)shader,
		};
		int argc = 7;

		if (entry_arg[0]) {
			argv[argc++] = (char *)"--entryslot";
			argv[argc++] = entry_arg;
		}
		if (!strcmp(net, "host")) {
			argv[argc++] = (char *)"--host";
		} else if (net_address[0]) {
			argv[argc++] = (char *)"--connect";
			argv[argc++] = net_address;
			argv[argc++] = (char *)"--port";
			argv[argc++] = net_port;
		}
		argv[argc++] = (char *)"-L";
		argv[argc++] = core_path;
		argv[argc++] = session.content;
		argv[argc] = NULL;
		int log_fd;
		sigset_t none;

		sigemptyset(&none);
		sigprocmask(SIG_SETMASK, &none, NULL);
		signal(SIGCHLD, SIG_DFL);
		signal(SIGPIPE, SIG_DFL);
		setsid();
		dup2(in_pipe[0], STDIN_FILENO);
		dup2(out_pipe[1], STDOUT_FILENO);
		log_fd = open(RETROARCH_LOG, O_WRONLY | O_CREAT | O_TRUNC | O_CLOEXEC, 0644);
		if (log_fd >= 0)
			dup2(log_fd, STDERR_FILENO);
		clearenv();
		setenv("PATH", "/usr/bin:/bin:/usr/sbin:/sbin", 1);
		setenv("HOME", ra_dir, 1);
		/* RetroArch's own default directories ($XDG_CONFIG_HOME/
		 * retroarch) are the user's RetroArch directory. */
		snprintf(cfg, sizeof(cfg), "%s/appdata", base);
		setenv("XDG_CONFIG_HOME", cfg, 1);
		snprintf(cfg, sizeof(cfg), "%s/retroarch.cfg", ra_dir);
		setenv("XDG_RUNTIME_DIR", WAYLAND_RUNTIME, 1);
		setenv("WAYLAND_DISPLAY", "wayland-0", 1);
		setenv("PIPEWIRE_RUNTIME_DIR", PIPEWIRE_RUNTIME, 1);
		setenv("LANG", "C.UTF-8", 1);
		if (overscan > 0) {
			char pct[16];

			snprintf(pct, sizeof(pct), "%d", overscan);
			setenv("NUUBOS_RA_OVERSCAN_X", pct, 1);
		}
		if (render_height > 0) {
			char lines[16];

			snprintf(lines, sizeof(lines), "%d", render_height);
			setenv("NUUBOS_RA_RENDER_HEIGHT", lines, 1);
		}
		execv(RETROARCH_BIN, argv);
		_exit(127);
	}

	close(in_pipe[0]);
	close(out_pipe[1]);
	session.pid = pid;
	session.stdin_fd = in_pipe[1];
	session.stdout_fd = out_pipe[0];
	(void)fcntl(session.stdin_fd, F_SETFL, O_NONBLOCK);
	(void)fcntl(session.stdout_fd, F_SETFL, O_NONBLOCK);
	session.state = SESSION_RUNNING;
	/* Load State and the slot shown follow the state the game started
	 * from, else the most recent numbered slot. */
	session.slot = start >= 0 ? start : manual;
	session.paused = false;
	session.fps = 0;
	session.overlay_tick_ms = monotonic_ms() + OVERLAY_TICK_MS;
	session.fast_forward = false;
	session.started_ms = monotonic_ms();
	copy_text(session.net_launch, sizeof(session.net_launch), net_address[0] ? "connect" : net);
	/* The user's cheats for the game (never with netplay or hardcore). */
	if (!cheats_blocked() && overrides.cheats[0]) {
		struct cheat_list lists[MAX_CHEAT_LISTS];
		int count = cheat_lists(session.system, session.rom, lists);
		char file[PATH_MAX];

		current_cheat_file(&overrides, lists, count, file, sizeof(file));
		apply_session_cheats(&overrides, file);
	}
	log_msg("emud: launch game=%s system=%s core=%s pid=%d user=%s start=%d shader=%s render=%d hardcore=%d net=%s cheats=%d",
		session.game, session.system, session.core, (int)pid,
		session.user, start, shader[0] ? shader : "none", render_height,
		session.hardcore ? 1 : 0, net[0] ? net : "-", session.cheats_on);
	library_session("SESSION_BEGIN", session.game);
	notify_subscribers();
	return NULL;
}

static void request_quit(void)
{
	if (session.state != SESSION_RUNNING)
		return;
	/* With slot rotation RetroArch's automatic slot is off: the progress
	 * goes into the next slot first (once; a failed save still quits). */
	if (session.settings.slot_rotation && !session.hardcore && !session.quit_saved) {
		session.quit_requested = true;
		notify_subscribers();
		if (session.pending != PENDING_NONE)
			return;
		session.quit_saved = true;
		if (save_next(true))
			return;
	}
	session.state = SESSION_EXITING;
	session.quit_deadline_ms = monotonic_ms() + QUIT_TIMEOUT_MS;
	/* Unpause first: a paused RetroArch still reads commands, but the
	 * quit must also flush SRAM through the normal content close. */
	ra_command("SET_PAUSED 0");
	ra_command("QUIT");
	notify_subscribers();
}

/* A PRE_POWER hook waits for the automatic state (and, before
 * restart/poweroff, for RetroArch to exit and write its save files). */
static void finish_power_wait(void)
{
	struct client *c;

	if (session.power_client < 0)
		return;
	if (session.pending != PENDING_NONE)
		return;
	if (strcmp(session.power_action, "sleep") != 0) {
		if (session.state == SESSION_RUNNING)
			request_quit();
		if (session.state != SESSION_IDLE)
			return;
	}
	c = &clients[session.power_client];
	session.power_client = -1;
	reply(c, "OK\n");
}

/* Saves and states just changed: the automatic backup to the games card
 * runs now if it is on (docs/dev/platform.md "SD2 = games card"). The
 * helper only submits a job; it is reaped like any child. */
static void trigger_backup(void)
{
	pid_t pid = fork();

	if (pid == 0) {
		int null = open("/dev/null", O_RDWR | O_CLOEXEC);

		if (null >= 0) {
			dup2(null, STDIN_FILENO);
			dup2(null, STDOUT_FILENO);
			dup2(null, STDERR_FILENO);
		}
		execl("/usr/sbin/nuubos-sd2-backup", "nuubos-sd2-backup", "trigger",
		      "session-end", (char *)NULL);
		_exit(127);
	}
}

static void child_exited(int status)
{
	long long ran = monotonic_ms() - session.started_ms;
	bool failed = (WIFEXITED(status) && WEXITSTATUS(status) != 0) ||
		      WIFSIGNALED(status);
	int power_client = session.power_client;
	/* Host Game: the same game again, from the progress its quit just
	 * saved, as a netplay host. */
	bool relaunch = session.relaunch_host && !failed && power_client < 0;
	char game[sizeof(session.game)];

	log_msg("emud: retroarch exited pid=%d status=%d after %lld ms",
		(int)session.pid, status, ran);
	if (session.pending != PENDING_NONE && !session.pending_quiet)
		notify_state("game.state.failed", session.pending_slot,
			     session.pending == PENDING_SAVE ? "save" : "load");
	library_session("SESSION_END", session.game);
	if (failed && session.state == SESSION_RUNNING)
		notify_failed(ran < START_FAILURE_MS ? "start" : "crash");
	if (session.shot_fd > 0)
		notify_screenshot(false);
	restore_user_keys();
	copy_text(game, sizeof(game), session.game);
	reset_session();
	if (relaunch) {
		const char *error = launch(game, "resume", "host");

		if (!error)
			return;
		log_msg("emud: host relaunch of %s refused: %s", game, error);
		reset_session();
		notify_failed("start");
	}
	notify_subscribers();
	if (power_client >= 0 && clients[power_client].fd >= 0)
		reply(&clients[power_client], "OK\n");
	else
		trigger_backup();
}

static void reap_children(void)
{
	char drain[32];
	int status;
	pid_t pid;

	while (read(sigchld_pipe[0], drain, sizeof(drain)) > 0)
		;
	while ((pid = waitpid(-1, &status, WNOHANG)) > 0) {
		if (pid == session.pid)
			child_exited(status);
	}
}

static int session_timeout_ms(void)
{
	long long now = monotonic_ms();
	long long deadline = -1;

	if (session.pending != PENDING_NONE)
		deadline = session.pending_deadline_ms;
	if (session.state == SESSION_EXITING &&
	    (deadline < 0 || session.quit_deadline_ms < deadline))
		deadline = session.quit_deadline_ms;
	if (session.shot_fd > 0 && (deadline < 0 || session.shot_deadline_ms < deadline))
		deadline = session.shot_deadline_ms;
	if (discovery.fd >= 0 && (deadline < 0 || discovery.deadline_ms < deadline))
		deadline = discovery.deadline_ms;
	if (session.state == SESSION_RUNNING && session.settings.overlay &&
	    (deadline < 0 || session.overlay_tick_ms < deadline))
		deadline = session.overlay_tick_ms;
	if (deadline < 0)
		return -1;
	if (deadline <= now)
		return 0;
	return deadline - now > INT_MAX ? INT_MAX : (int)(deadline - now);
}

static void finish_discovery(void);

static void session_timers(void)
{
	long long now = monotonic_ms();

	if (session.shot_fd > 0 && now >= session.shot_deadline_ms)
		finish_screenshot(false);
	if (discovery.fd >= 0 && now >= discovery.deadline_ms)
		finish_discovery();
	/* Performance overlay: ask RetroArch for its frame rate; the reply
	 * updates the snapshot the Quick Menu draws from. Not while paused
	 * (the Quick Menu hides the overlay then). */
	if (session.state == SESSION_RUNNING && session.settings.overlay &&
	    now >= session.overlay_tick_ms) {
		if (!session.paused)
			ra_command("GET_FPS");
		session.overlay_tick_ms = now + OVERLAY_TICK_MS;
	}
	if (session.pending != PENDING_NONE && now >= session.pending_deadline_ms) {
		log_msg("emud: state operation slot %d timed out", session.pending_slot);
		finish_pending(false);
	}
	if (session.state == SESSION_EXITING && now >= session.quit_deadline_ms) {
		if (!session.term_sent) {
			log_msg("emud: retroarch did not quit, SIGTERM");
			kill(session.pid, SIGTERM);
			session.term_sent = true;
			session.quit_deadline_ms = now + KILL_TIMEOUT_MS;
		} else if (!session.kill_sent) {
			log_msg("emud: retroarch did not terminate, SIGKILL");
			kill(-session.pid, SIGKILL);
			session.kill_sent = true;
			session.quit_deadline_ms = now + KILL_TIMEOUT_MS;
		}
	}
}

/* ------------------------------------------------------------------ */
/* Screenshots (EPIC-021)                                              */
/* ------------------------------------------------------------------ */

static void finish_screenshot(bool ok)
{
	if (session.shot_fd > 0)
		close(session.shot_fd);
	session.shot_fd = -1;
	notify_screenshot(ok);
	log_msg("emud: screenshot %s", ok ? "saved" : "failed");
}

/* RetroArch writes the PNG of its own rendering (never the composited
 * screen, so the Quick Menu is not in it); success is reported only once a
 * new PNG is closed in the user's screenshot folder. */
static bool begin_screenshot(void)
{
	if (session.state != SESSION_RUNNING || session.shot_fd > 0)
		return false;
	session.shot_fd = inotify_init1(IN_NONBLOCK | IN_CLOEXEC);
	if (session.shot_fd < 0 ||
	    inotify_add_watch(session.shot_fd, session.screenshots_dir,
			      IN_CLOSE_WRITE | IN_MOVED_TO) < 0) {
		if (session.shot_fd >= 0)
			close(session.shot_fd);
		session.shot_fd = -1;
		notify_screenshot(false);
		return false;
	}
	session.shot_deadline_ms = monotonic_ms() + SCREENSHOT_TIMEOUT_MS;
	ra_command("SCREENSHOT");
	return true;
}

static void process_shot_watch(void)
{
	char buf[4096] __attribute__((aligned(__alignof__(struct inotify_event))));
	ssize_t n;
	bool done = false;

	while ((n = read(session.shot_fd, buf, sizeof(buf))) > 0) {
		for (char *p = buf; p < buf + n;) {
			struct inotify_event *ev = (struct inotify_event *)p;
			size_t len = ev->len ? strlen(ev->name) : 0;

			if (len > 4 && !strcasecmp(ev->name + len - 4, ".png"))
				done = true;
			p += sizeof(*ev) + ev->len;
		}
	}
	if (done)
		finish_screenshot(true);
}

/* ------------------------------------------------------------------ */
/* Commands                                                            */
/* ------------------------------------------------------------------ */

static int step_slot(int slot, int delta)
{
	int last = session.settings.slots - 1;

	slot += delta;
	if (slot > last)
		return 0;
	if (slot < 0)
		return last;
	return slot;
}

/* Controller hotkeys from nuubos-inputd (Quick Menu button + chord): the
 * same session actions as the Quick Menu, with the outcome as an OSD
 * notification. */
static void handle_hotkey(const char *action)
{
	if (session.state != SESSION_RUNNING)
		return;
	log_msg("emud: hotkey %s", action);
	if (session.hardcore &&
	    (!strcmp(action, "save_state") || !strcmp(action, "load_state") ||
	     !strcmp(action, "slot_next") || !strcmp(action, "slot_prev") ||
	     !strcmp(action, "fast_forward"))) {
		notify_hardcore();
		return;
	}
	if (!strcmp(action, "save_state")) {
		if (save_next(false))
			notify_subscribers();
	} else if (!strcmp(action, "quit")) {
		request_quit();
	} else if (!strcmp(action, "load_state")) {
		if (session.pending == PENDING_NONE)
			begin_load(session.slot);
	} else if (!strcmp(action, "slot_next") || !strcmp(action, "slot_prev")) {
		session.slot = step_slot(session.slot,
					 !strcmp(action, "slot_next") ? 1 : -1);
		notify_slot(session.slot);
		notify_subscribers();
	} else if (!strcmp(action, "screenshot")) {
		(void)begin_screenshot();
	} else if (!strcmp(action, "fast_forward")) {
		session.fast_forward = !session.fast_forward;
		ra_command("FAST_FORWARD");
		notify_fast_forward(session.fast_forward);
		notify_subscribers();
	}
}

static void process_input_link(void)
{
	char buf[128];
	ssize_t n = read(session.input_fd, buf, sizeof(buf));

	if (n == 0 || (n < 0 && errno != EAGAIN && errno != EINTR)) {
		/* inputd restarted: take the gamepads again. */
		close(session.input_fd);
		session.input_used = 0;
		session.input_fd = input_gamepads_on();
		send_hotkeys();
		return;
	}
	for (ssize_t i = 0; i < n; i++) {
		if (buf[i] == '\n') {
			session.input_buf[session.input_used] = '\0';
			if (!strncmp(session.input_buf, "HOTKEY ", 7))
				handle_hotkey(session.input_buf + 7);
			session.input_used = 0;
		} else if (session.input_used + 1 < sizeof(session.input_buf)) {
			session.input_buf[session.input_used++] = buf[i];
		} else {
			session.input_used = 0;
		}
	}
}

static bool running(struct client *c)
{
	if (session.state == SESSION_RUNNING)
		return true;
	reply(c, "ERR no-session\n");
	return false;
}

/* GAME_SETTINGS <game> <system> | GAME_SET <game> <system> <key> <value> |
 * GAME_RESET <game>, for the active user. */
static void handle_game_settings(struct client *c, const char *verb, char *arg)
{
	char *fields[4] = { arg, NULL, NULL, NULL };
	char user[64];
	char cores[512];
	char def_core[64];
	char def_path[PATH_MAX];
	struct game_overrides o;
	int n = 1;

	for (char *p = arg; *p && n < 4; p++)
		if (*p == '\t') {
			*p = '\0';
			fields[n++] = p + 1;
		}
	if (!valid_game(fields[0])) {
		reply(c, "ERR game\n");
		return;
	}
	if (!read_active_user(user, sizeof(user))) {
		reply(c, "ERR no-user\n");
		return;
	}
	load_overrides(user, fields[0], &o);
	if (!strcmp(verb, "GAME_RESET")) {
		memset(&o, 0, sizeof(o));
		reply(c, save_overrides(user, fields[0], &o) == 0 ? "OK\n" : "ERR storage\n");
		return;
	}
	if (!fields[1] || !fields[1][0] || strchr(fields[1], '/')) {
		reply(c, "ERR system\n");
		return;
	}
	available_cores(fields[1], cores, sizeof(cores));
	if (!strcmp(verb, "GAME_SETTINGS")) {
		char out[1024];
		const char *effective;
		struct emu_settings settings;
		char screen[16];
		int len;

		/* filter_default: what an empty filter gives (write_presentation) */
		load_settings(user, &settings);
		system_screen(fields[1], screen, sizeof(screen));
		def_core[0] = '\0';
		(void)resolve_core(fields[1], def_core, sizeof(def_core), def_path, sizeof(def_path));
		effective = o.core[0] && list_has(cores, o.core) ? o.core : def_core;
		len = snprintf(out, sizeof(out),
			       "core=%s\ncore_default=%s\ncore_override=%s\ncores=%s\n"
			       "aspect=%s\nfilter=%s\nfilter_default=%s\nend=1\n",
			       effective, def_core, o.core, cores, o.aspect, o.filter,
			       settings.crt && !strcmp(screen, "crt") ? "crt" : "sharp");
		if (len > 0 && write_all(c->fd, out, (size_t)len) != 0)
			close_client(c);
		return;
	}
	/* GAME_SET */
	if (!fields[2] || !fields[3]) {
		reply(c, "ERR args\n");
		return;
	}
	if (!strcmp(fields[2], "core") && (!fields[3][0] || list_has(cores, fields[3])))
		copy_text(o.core, sizeof(o.core), fields[3]);
	else if (!strcmp(fields[2], "aspect") && (!fields[3][0] || valid_aspect(fields[3])))
		copy_text(o.aspect, sizeof(o.aspect), fields[3]);
	else if (!strcmp(fields[2], "filter") && (!fields[3][0] || valid_filter(fields[3])))
		copy_text(o.filter, sizeof(o.filter), fields[3]);
	else {
		reply(c, "ERR value\n");
		return;
	}
	reply(c, save_overrides(user, fields[0], &o) == 0 ? "OK\n" : "ERR storage\n");
}

/* Cheats are refused in hardcore mode and with netplay. */
static const char *cheats_blocked(void)
{
	if (session.hardcore)
		return "hardcore";
	if (session.net_launch[0] || session.netplay != NET_OFF)
		return "netplay";
	return NULL;
}

/* CHEATS: the running game's cheat lists and the cheats of the current
 * one: "available=0|1", "reason=" (hardcore|netplay), "file=<path>",
 * "list=<path>\t<name>" per list, "cheat=<k>\t<0|1>\t<description>" per
 * cheat, "end=1". */
static void handle_cheats(struct client *c)
{
	struct cheat_list lists[MAX_CHEAT_LISTS];
	struct game_overrides o;
	char file[PATH_MAX];
	char line[2048];
	const char *blocked = cheats_blocked();
	int count;
	int total = 0;
	bool *seen = NULL;
	FILE *fp;

	if (!running(c))
		return;
	count = cheat_lists(session.system, session.rom, lists);
	load_overrides(session.user, session.game, &o);
	current_cheat_file(&o, lists, count, file, sizeof(file));
	snprintf(line, sizeof(line), "available=%d\nreason=%s\nfile=%s\n", blocked ? 0 : 1,
		 blocked ? blocked : "", file);
	if (write_all(c->fd, line, strlen(line)) != 0)
		goto gone;
	for (int i = 0; i < count; i++) {
		snprintf(line, sizeof(line), "list=%s\t%s\n", lists[i].path, lists[i].name);
		if (write_all(c->fd, line, strlen(line)) != 0)
			goto gone;
	}
	if (file[0] && (fp = fopen(file, "r"))) {
		char buf[1024];

		while (fgets(buf, sizeof(buf), fp))
			if (sscanf(buf, " cheats = %d", &total) == 1 || sscanf(buf, " cheats = \"%d\"", &total) == 1)
				break;
		/* A list of at most 1000 cheats is shown. */
		if (total > 1000)
			total = 1000;
		if (total > 0)
			seen = calloc((size_t)total, sizeof(*seen));
		rewind(fp);
		while (seen && fgets(buf, sizeof(buf), fp)) {
			int k;
			char *field = cheat_line(buf, &k);
			char *desc;
			char *end;
			char key[16];

			if (!field || strncmp(field, "desc", 4) || k >= total || seen[k] ||
			    !(desc = strchr(field, '"')))
				continue;
			desc++;
			trim(desc);
			if ((end = strrchr(desc, '"')))
				*end = '\0';
			for (char *q = desc; *q; q++)
				if (*q == '\t' || (unsigned char)*q < 0x20)
					*q = ' ';
			if (strlen(desc) > 120)
				desc[120] = '\0';
			seen[k] = true;
			snprintf(key, sizeof(key), "%d", k);
			snprintf(line, sizeof(line), "cheat=%d\t%d\t%s\n", k, list_has(o.cheats, key) ? 1 : 0, desc);
			if (write_all(c->fd, line, strlen(line)) != 0) {
				fclose(fp);
				free(seen);
				goto gone;
			}
		}
		fclose(fp);
		/* Cheats without a description still count. */
		for (int k = 0; seen && k < total; k++) {
			char key[16];

			if (seen[k])
				continue;
			snprintf(key, sizeof(key), "%d", k);
			snprintf(line, sizeof(line), "cheat=%d\t%d\t\n", k, list_has(o.cheats, key) ? 1 : 0);
			if (write_all(c->fd, line, strlen(line)) != 0) {
				free(seen);
				goto gone;
			}
		}
		free(seen);
	}
	if (write_all(c->fd, "end=1\n", 6) == 0)
		return;
gone:
	close_client(c);
}

/* CHEAT_SET <k> <0|1>: turns cheat k of the current list on or off for
 * the running game (kept for the user and the game), applied at once.
 * CHEAT_FILE <path>: picks another of the game's lists (its cheats start
 * off). */
static void handle_cheat_change(struct client *c, const char *verb, char *arg)
{
	struct cheat_list lists[MAX_CHEAT_LISTS];
	struct game_overrides o;
	char file[PATH_MAX];
	const char *blocked;
	int count;

	if (!running(c))
		return;
	if ((blocked = cheats_blocked())) {
		char text[32];

		snprintf(text, sizeof(text), "ERR %s\n", blocked);
		reply(c, text);
		return;
	}
	count = cheat_lists(session.system, session.rom, lists);
	load_overrides(session.user, session.game, &o);
	if (!strcmp(verb, "CHEAT_FILE")) {
		bool known = false;

		for (int i = 0; i < count && !known; i++)
			known = !strcmp(lists[i].path, arg);
		if (!known) {
			reply(c, "ERR list\n");
			return;
		}
		copy_text(o.cheat_file, sizeof(o.cheat_file), arg);
		o.cheats[0] = '\0';
	} else {
		char *value = strchr(arg, '\t');
		char *end;
		long k = strtol(arg, &end, 10);
		char list[sizeof(o.cheats)];
		char *save = NULL;
		char key[16];
		size_t len = 0;

		if (value)
			*value++ = '\0';
		current_cheat_file(&o, lists, count, file, sizeof(file));
		if (*end || k < 0 || k >= 1000 || !value || (strcmp(value, "0") && strcmp(value, "1"))) {
			reply(c, "ERR args\n");
			return;
		}
		if (!file[0]) {
			reply(c, "ERR none\n");
			return;
		}
		/* The list the cheats belong to is kept with them. */
		copy_text(o.cheat_file, sizeof(o.cheat_file), file);
		snprintf(key, sizeof(key), "%ld", k);
		copy_text(list, sizeof(list), o.cheats);
		o.cheats[0] = '\0';
		for (char *w = strtok_r(list, ",", &save); w; w = strtok_r(NULL, ",", &save)) {
			if (!strcmp(w, key))
				continue;
			len += (size_t)snprintf(o.cheats + len, sizeof(o.cheats) - len, "%s%s", len ? "," : "", w);
		}
		if (!strcmp(value, "1")) {
			if (len + strlen(key) + 2 >= sizeof(o.cheats)) {
				reply(c, "ERR full\n");
				return;
			}
			snprintf(o.cheats + len, sizeof(o.cheats) - len, "%s%s", len ? "," : "", key);
		}
	}
	if (save_overrides(session.user, session.game, &o) != 0) {
		reply(c, "ERR storage\n");
		return;
	}
	current_cheat_file(&o, lists, count, file, sizeof(file));
	apply_session_cheats(&o, file);
	reply(c, "OK\n");
}

/* SETTINGS (key, value NULL) lists the active user's settings; SET key
 * value changes one. Hotkeys and the overlay follow at once in a running
 * game of that user; the rest applies at the next launch. */
static void handle_settings(struct client *c, const char *key, const char *value)
{
	char user[64];
	char out[1100];
	struct emu_settings e;
	size_t len;

	if (!read_active_user(user, sizeof(user))) {
		reply(c, "ERR no-user\n");
		return;
	}
	load_settings(user, &e);
	if (!key) {
		len = format_settings(&e, out, sizeof(out) - 8);
		memcpy(out + len, "end=1\n", 7);
		if (write_all(c->fd, out, len + 6) != 0)
			close_client(c);
		return;
	}
	if (!settings_set(&e, key, value)) {
		reply(c, "ERR value\n");
		return;
	}
	if (save_settings(user, &e) != 0) {
		reply(c, "ERR storage\n");
		return;
	}
	reply(c, "OK\n");
	if (session.state != SESSION_RUNNING || strcmp(user, session.user))
		return;
	memcpy(session.settings.hotkey, e.hotkey, sizeof(e.hotkey));
	session.settings.overlay = e.overlay;
	copy_text(session.settings.overlay_items, sizeof(session.settings.overlay_items),
		  e.overlay_items);
	if (!strncmp(key, "hotkey_", 7))
		send_hotkeys();
	session.overlay_tick_ms = monotonic_ms();
	notify_subscribers();
}

/* STATES [game]: the save states of the running game, or of a game before
 * it starts (Home's start menu): "slots=N", then "state=<slot>\t<mtime>\t
 * <HH:MM local>" for each existing state (slot -1 = the automatic state),
 * "end=1". In hardcore mode "hardcore=1" and no states (none can be
 * loaded). */
static void handle_states(struct client *c, const char *game)
{
	char base[PATH_MAX];
	char out[4096];
	int slots;
	bool hardcore;
	size_t len;

	if (game && game[0]) {
		char user[64];
		char command[64];
		char answer[PATH_MAX + 128];
		char name[NAME_MAX + 1];
		char *tab;
		struct emu_settings e;
		struct achievements a;

		if (!valid_game(game)) {
			reply(c, "ERR game\n");
			return;
		}
		if (!read_active_user(user, sizeof(user))) {
			reply(c, "ERR no-user\n");
			return;
		}
		snprintf(command, sizeof(command), "RESOLVE\t%s", game);
		if (library_request(command, answer, sizeof(answer)) != 0 ||
		    strncmp(answer, "OK ", 3) != 0 || !(tab = strchr(answer + 3, '\t'))) {
			reply(c, "ERR game\n");
			return;
		}
		*tab = '\0';
		load_settings(user, &e);
		slots = e.slots;
		load_achievements(user, &a);
		hardcore = a.enabled && a.hardcore;
		content_base_name(tab + 1, name, sizeof(name));
		snprintf(base, sizeof(base), USERS_DIR "/%s/states/%s/%s", user, answer + 3, name);
	} else if (session.state == SESSION_RUNNING) {
		slots = session.settings.slots;
		hardcore = session.hardcore;
		copy_text(base, sizeof(base), session.state_base);
	} else {
		reply(c, "ERR no-session\n");
		return;
	}
	len = (size_t)snprintf(out, sizeof(out), "slots=%d\n%s", slots,
			       hardcore ? "hardcore=1\n" : "");
	for (int slot = AUTO_SLOT; !hardcore && slot < slots && len + 48 < sizeof(out); slot++) {
		long long t = state_mtime(base, slot);

		time_t tt = (time_t)t;
		struct tm tm;

		if (t < 0)
			continue;
		/* Local time of the save, for display. */
		if (!localtime_r(&tt, &tm))
			memset(&tm, 0, sizeof(tm));
		len += (size_t)snprintf(out + len, sizeof(out) - len, "state=%d\t%lld\t%02d:%02d\n",
					slot, t, tm.tm_hour, tm.tm_min);
	}
	len += (size_t)snprintf(out + len, sizeof(out) - len, "end=1\n");
	if (write_all(c->fd, out, len) != 0)
		close_client(c);
}

/* ------------------------------------------------------------------ */
/* Netplay hosts on the local network                                  */
/* ------------------------------------------------------------------ */

/* RetroArch's LAN discovery: a "RANQ" broadcast to the netplay port, each
 * host answers with this packet (network/netplay/netplay_frontend.c). */
#define DISCOVERY_QUERY 0x52414E51u
#define DISCOVERY_RESPONSE 0x52414E53u
struct ad_packet {
	uint32_t header;
	int32_t content_crc;
	int32_t port;
	uint32_t has_password;
	char nick[32];
	char frontend[32];
	char core[32];
	char core_version[32];
	char retroarch_version[32];
	char content[256];
	char subsystem_name[256];
};

static void start_discovery(struct client *c)
{
	if (discovery.fd < 0) {
		struct sockaddr_in to;
		uint32_t query = htonl(DISCOVERY_QUERY);
		int on = 1;
		int fd = socket(AF_INET, SOCK_DGRAM | SOCK_CLOEXEC | SOCK_NONBLOCK, 0);

		memset(&to, 0, sizeof(to));
		to.sin_family = AF_INET;
		to.sin_port = htons(NETPLAY_PORT);
		to.sin_addr.s_addr = htonl(INADDR_BROADCAST);
		if (fd < 0 || setsockopt(fd, SOL_SOCKET, SO_BROADCAST, &on, sizeof(on)) != 0 ||
		    sendto(fd, &query, sizeof(query), 0, (struct sockaddr *)&to, sizeof(to)) !=
			    (ssize_t)sizeof(query)) {
			if (fd >= 0)
				close(fd);
			reply(c, "ERR network\n");
			return;
		}
		memset(discovery.waiting, 0, sizeof(discovery.waiting));
		discovery.fd = fd;
		discovery.count = 0;
		discovery.deadline_ms = monotonic_ms() + DISCOVERY_MS;
	}
	discovery.waiting[c - clients] = true;
}

static void process_discovery(void)
{
	struct ad_packet ad;
	struct sockaddr_in from;
	socklen_t len = sizeof(from);
	ssize_t n;

	while ((n = recvfrom(discovery.fd, &ad, sizeof(ad), 0, (struct sockaddr *)&from, &len)) >= 0) {
		char address[INET_ADDRSTRLEN];
		int port = (int)ntohl((uint32_t)ad.port);
		bool known = false;

		len = sizeof(from);
		if (n != (ssize_t)sizeof(ad) || ntohl(ad.header) != DISCOVERY_RESPONSE ||
		    from.sin_family != AF_INET || port < 1 || port > 65535 ||
		    !inet_ntop(AF_INET, &from.sin_addr, address, sizeof(address)))
			continue;
		for (int i = 0; i < discovery.count && !known; i++)
			known = !strcmp(discovery.hosts[i].address, address) && discovery.hosts[i].port == port;
		if (known || discovery.count >= (int)(sizeof(discovery.hosts) / sizeof(discovery.hosts[0])))
			continue;
		ad.nick[sizeof(ad.nick) - 1] = '\0';
		ad.core[sizeof(ad.core) - 1] = '\0';
		ad.content[sizeof(ad.content) - 1] = '\0';
		copy_text(discovery.hosts[discovery.count].address, INET_ADDRSTRLEN, address);
		discovery.hosts[discovery.count].port = port;
		discovery.hosts[discovery.count].password = ntohl(ad.has_password) & 1;
		copy_text(discovery.hosts[discovery.count].nick, 32, ad.nick);
		copy_text(discovery.hosts[discovery.count].core, 32, ad.core);
		copy_text(discovery.hosts[discovery.count].content, 256, ad.content);
		discovery.count++;
	}
}

/* Text safe for one tab separated field. */
static void field_safe(char *text)
{
	for (char *p = text; *p; p++)
		if (*p == '\t' || (unsigned char)*p < 0x20)
			*p = ' ';
}

/* NETPLAY_HOSTS answer, once the hosts had DISCOVERY_MS to reply:
 * "host=<address>\t<port>\t<nick>\t<emulator name>\t<content file>\t
 * <game id>\t<system>\t<our core>\t<password 0|1>" per host ("" game: not
 * in this library; "" core: the host's emulator is not on this console),
 * then "end=1". */
static void finish_discovery(void)
{
	char out[8192];
	size_t len = 0;

	if (discovery.fd >= 0)
		process_discovery();
	for (int i = 0; i < discovery.count; i++) {
		char command[300];
		char answer[256] = "";
		char game[32] = "";
		char system[64] = "";
		char core[64] = "";
		char *tab;

		field_safe(discovery.hosts[i].nick);
		field_safe(discovery.hosts[i].core);
		field_safe(discovery.hosts[i].content);
		snprintf(command, sizeof(command), "MATCH\t%s", discovery.hosts[i].content);
		if (library_request(command, answer, sizeof(answer)) == 0 &&
		    !strncmp(answer, "OK ", 3) && (tab = strchr(answer + 3, '\t'))) {
			char cores[512];
			char *save = NULL;

			*tab = '\0';
			copy_text(game, sizeof(game), answer + 3);
			copy_text(system, sizeof(system), tab + 1);
			/* Netplay needs the host's emulator (its core name). */
			available_cores(system, cores, sizeof(cores));
			for (char *name = strtok_r(cores, ",", &save); name && !core[0];
			     name = strtok_r(NULL, ",", &save)) {
				char corename[64];

				core_info_value(name, "corename", corename, sizeof(corename));
				if (corename[0] && !strcasecmp(corename, discovery.hosts[i].core))
					copy_text(core, sizeof(core), name);
			}
		}
		len += (size_t)snprintf(out + len, sizeof(out) - len,
					"host=%s\t%d\t%s\t%s\t%s\t%s\t%s\t%s\t%d\n",
					discovery.hosts[i].address, discovery.hosts[i].port,
					discovery.hosts[i].nick, discovery.hosts[i].core,
					discovery.hosts[i].content, game, system, core,
					discovery.hosts[i].password ? 1 : 0);
		if (len >= sizeof(out) - 16)
			break;
	}
	len += (size_t)snprintf(out + len, sizeof(out) - len, "end=1\n");
	if (discovery.fd >= 0)
		close(discovery.fd);
	discovery.fd = -1;
	for (int i = 0; i < MAX_CLIENTS; i++) {
		if (!discovery.waiting[i])
			continue;
		discovery.waiting[i] = false;
		if (clients[i].fd >= 0 && write_all(clients[i].fd, out, len) != 0)
			close_client(&clients[i]);
	}
}

static void handle_command(struct client *c, char *line)
{
	char *arg = strchr(line, '\t');
	char status[768];

	trim(line);
	if (arg)
		*arg++ = '\0';

	if (!strcmp(line, "STATUS")) {
		size_t len = build_status(status, sizeof(status));

		if (write_all(c->fd, status, len) != 0)
			close_client(c);
	} else if (!strcmp(line, "SUBSCRIBE")) {
		size_t len = build_status(status, sizeof(status));

		c->subscribed = true;
		if (write_all(c->fd, status, len) != 0)
			close_client(c);
	} else if (!strcmp(line, "LAUNCH") && arg) {
		char *mode = strchr(arg, '\t');
		char *net = NULL;
		const char *error;

		if (mode) {
			*mode++ = '\0';
			if ((net = strchr(mode, '\t')))
				*net++ = '\0';
		}
		error = launch(arg, mode, net);

		if (error) {
			char text[64];

			log_msg("emud: launch %s refused: %s", arg, error);
			if (session.state == SESSION_IDLE)
				reset_session();
			snprintf(text, sizeof(text), "ERR %s\n", error);
			reply(c, text);
		} else {
			reply(c, "OK\n");
		}
	} else if (!strcmp(line, "OVERLAY_TEXT")) {
		/* The performance overlay line built by the Quick Menu, drawn by
		 * RetroArch into the frame ("" hides it). */
		if (!running(c))
			return;
		ra_command("OSD_TEXT %s", arg ? arg : "");
		reply(c, "OK\n");
	} else if (!strcmp(line, "PAUSE") || !strcmp(line, "RESUME")) {
		if (!running(c))
			return;
		ra_command("SET_PAUSED %d", !strcmp(line, "PAUSE") ? 1 : 0);
		reply(c, "OK\n");
	} else if (session.hardcore && session.state == SESSION_RUNNING &&
		   (!strcmp(line, "SAVE_STATE") || !strcmp(line, "LOAD_STATE") ||
		    !strcmp(line, "SLOT") || !strcmp(line, "FAST_FORWARD"))) {
		notify_hardcore();
		reply(c, "ERR hardcore\n");
	} else if (!strcmp(line, "SAVE_STATE")) {
		if (!running(c))
			return;
		if (!save_next(false)) {
			reply(c, "ERR busy\n");
			return;
		}
		reply(c, "OK\n");
		notify_subscribers();
	} else if (!strcmp(line, "LOAD_STATE")) {
		if (!running(c))
			return;
		if (session.pending != PENDING_NONE) {
			reply(c, "ERR busy\n");
			return;
		}
		begin_load(session.slot);
		reply(c, "OK\n");
	} else if (!strcmp(line, "SLOT") && arg) {
		int slot = session.slot;

		if (!running(c))
			return;
		if (!strcmp(arg, "+1"))
			slot = step_slot(slot, 1);
		else if (!strcmp(arg, "-1"))
			slot = step_slot(slot, -1);
		else if (isdigit((unsigned char)arg[0]) && atoi(arg) < session.settings.slots)
			slot = atoi(arg);
		else {
			reply(c, "ERR slot\n");
			return;
		}
		session.slot = slot;
		reply(c, "OK\n");
		notify_subscribers();
	} else if (!strcmp(line, "RESET")) {
		if (!running(c))
			return;
		ra_command("RESET");
		ra_command("SET_PAUSED 0");
		reply(c, "OK\n");
	} else if (!strcmp(line, "FAST_FORWARD")) {
		if (!running(c))
			return;
		handle_hotkey("fast_forward");
		reply(c, "OK\n");
	} else if (!strcmp(line, "ADVANCED")) {
		/* RetroArch Advanced: RetroArch's own menu over the game, for
		 * power users; it pauses the content itself. */
		if (!running(c))
			return;
		ra_command("SET_PAUSED 0");
		ra_command("MENU_TOGGLE");
		reply(c, "OK\n");
	} else if (!strcmp(line, "QUIT")) {
		if (!running(c))
			return;
		request_quit();
		reply(c, "OK\n");
	} else if (!strcmp(line, "CHEATS")) {
		handle_cheats(c);
	} else if ((!strcmp(line, "CHEAT_SET") || !strcmp(line, "CHEAT_FILE")) && arg) {
		handle_cheat_change(c, line, arg);
	} else if (!strcmp(line, "NETPLAY_HOST")) {
		/* Host Game over a running game: it closes (its progress saved
		 * like at every quit) and starts again from there as a host. */
		if (!running(c))
			return;
		if (session.hardcore) {
			notify_hardcore();
			reply(c, "ERR hardcore\n");
			return;
		}
		if (session.net_launch[0] || session.netplay != NET_OFF) {
			reply(c, "ERR busy\n");
			return;
		}
		session.relaunch_host = true;
		request_quit();
		reply(c, "OK\n");
	} else if (!strcmp(line, "NETPLAY_STOP")) {
		/* Stop hosting or leave the host; the game goes on alone. */
		if (!running(c))
			return;
		if (!session.net_launch[0] && session.netplay == NET_OFF) {
			reply(c, "ERR netplay\n");
			return;
		}
		ra_command("SET_PAUSED 0");
		ra_command("NETPLAY_STOP");
		reply(c, "OK\n");
	} else if (!strcmp(line, "NETPLAY_HOSTS")) {
		start_discovery(c);
	} else if (!strcmp(line, "SCREENSHOT")) {
		if (!running(c))
			return;
		reply(c, begin_screenshot() ? "OK\n" : "ERR busy\n");
	} else if ((!strcmp(line, "GAME_SETTINGS") || !strcmp(line, "GAME_SET") ||
		    !strcmp(line, "GAME_RESET")) && arg) {
		handle_game_settings(c, line, arg);
	} else if (!strcmp(line, "SETTINGS")) {
		handle_settings(c, NULL, NULL);
	} else if (!strcmp(line, "SET") && arg) {
		char *value = strchr(arg, '\t');

		if (value)
			*value++ = '\0';
		handle_settings(c, arg, value ? value : "");
	} else if (!strcmp(line, "STATES")) {
		handle_states(c, arg);
	} else if (!strcmp(line, "STORAGE_LOST") && arg) {
		size_t len = strlen(arg);

		reply(c, "OK\n");
		if (session.state != SESSION_RUNNING || !len ||
		    strncmp(session.rom, arg, len) || session.rom[len] != '/')
			return;
		log_msg("emud: the ROM's storage %s went away, saving and closing", arg);
		notify_storage_lost();
		/* Saves and states live on SD1: the core still has the game in
		 * memory, so the automatic state is taken first (not in
		 * hardcore), then the session ends like before a power off.
		 * A core stuck reading the missing card is killed by the quit
		 * timeout. */
		if (!session.hardcore && session.pending == PENDING_NONE) {
			if (!session.settings.slot_rotation)
				(void)begin_save(AUTO_SLOT, true);
			else if (save_next(true))
				session.quit_saved = true;
		}
		session.quit_requested = true;
		if (session.pending == PENDING_NONE)
			request_quit();
	} else if (!strcmp(line, "PRE_POWER") && arg) {
		if (strcmp(arg, "sleep") && strcmp(arg, "restart") &&
		    strcmp(arg, "poweroff")) {
			reply(c, "ERR action\n");
			return;
		}
		if (session.state == SESSION_IDLE) {
			reply(c, "OK\n");
			return;
		}
		if (session.power_client >= 0) {
			reply(c, "ERR busy\n");
			return;
		}
		session.power_client = (int)(c - clients);
		copy_text(session.power_action, sizeof(session.power_action), arg);
		/* Progress is safe before every power action (CLAUDE §20.1):
		 * in the automatic slot, or with slot rotation in the next slot
		 * (the quit before restart/poweroff then does not save again);
		 * a state operation already running completes first. Hardcore
		 * sessions have no states: SRAM is written by the quit. */
		if (session.state == SESSION_RUNNING && !session.hardcore &&
		    session.pending == PENDING_NONE) {
			if (!session.settings.slot_rotation)
				(void)begin_save(AUTO_SLOT, true);
			else if (save_next(true) && strcmp(arg, "sleep") != 0)
				session.quit_saved = true;
		}
		finish_power_wait();
	} else {
		reply(c, "ERR unknown\n");
	}
}

static void process_client(struct client *c)
{
	char buf[512];
	ssize_t n = read(c->fd, buf, sizeof(buf));

	if (n <= 0) {
		if (n < 0 && (errno == EINTR || errno == EAGAIN))
			return;
		close_client(c);
		return;
	}
	for (ssize_t i = 0; i < n && c->fd >= 0; i++) {
		if (buf[i] == '\n') {
			c->buf[c->used] = '\0';
			handle_command(c, c->buf);
			c->used = 0;
		} else if (c->used + 1 < sizeof(c->buf)) {
			c->buf[c->used++] = buf[i];
		} else {
			c->used = 0;
		}
	}
}

static int make_server(void)
{
	struct sockaddr_un addr;
	int fd = socket(AF_UNIX, SOCK_STREAM | SOCK_CLOEXEC | SOCK_NONBLOCK, 0);

	if (fd < 0)
		return -1;
	memset(&addr, 0, sizeof(addr));
	addr.sun_family = AF_UNIX;
	copy_text(addr.sun_path, sizeof(addr.sun_path), SOCKET_PATH);
	unlink(SOCKET_PATH);
	if (bind(fd, (struct sockaddr *)&addr, sizeof(addr)) != 0 ||
	    chmod(SOCKET_PATH, 0660) != 0 || listen(fd, 8) != 0) {
		close(fd);
		return -1;
	}
	return fd;
}

static void accept_client(int server)
{
	int fd = accept4(server, NULL, NULL, SOCK_CLOEXEC | SOCK_NONBLOCK);

	if (fd < 0)
		return;
	for (int i = 0; i < MAX_CLIENTS; i++) {
		if (clients[i].fd >= 0)
			continue;
		clients[i].fd = fd;
		clients[i].subscribed = false;
		clients[i].used = 0;
		return;
	}
	close(fd);
}

int main(void)
{
	struct sigaction sa;
	int server;
	FILE *pid_fp;

	for (int i = 0; i < MAX_CLIENTS; i++)
		clients[i].fd = -1;
	reset_session();

	if (mkdir_p(RUN_ROOT) != 0 || pipe2(sigchld_pipe, O_CLOEXEC | O_NONBLOCK) != 0) {
		perror("nuubos-emud: init");
		return 1;
	}
	memset(&sa, 0, sizeof(sa));
	sa.sa_handler = handle_sigchld;
	sa.sa_flags = SA_RESTART | SA_NOCLDSTOP;
	sigemptyset(&sa.sa_mask);
	sigaction(SIGCHLD, &sa, NULL);
	sa.sa_handler = handle_stop;
	sa.sa_flags = 0;
	sigaction(SIGTERM, &sa, NULL);
	sigaction(SIGINT, &sa, NULL);
	signal(SIGPIPE, SIG_IGN);

	server = make_server();
	if (server < 0) {
		perror("nuubos-emud: socket");
		return 1;
	}
	pid_fp = fopen(PIDFILE, "w");
	if (pid_fp) {
		fprintf(pid_fp, "%ld\n", (long)getpid());
		fclose(pid_fp);
	}
	log_msg("emud: started pid=%ld", (long)getpid());
	sync_low_latency(); /* a mark left by a crashed emud */

	while (!stop_requested) {
		struct pollfd pfd[7 + MAX_CLIENTS];
		int who[7 + MAX_CLIENTS];
		nfds_t count = 0;
		int rc;

		pfd[count] = (struct pollfd){ server, POLLIN, 0 };
		who[count++] = -1;
		pfd[count] = (struct pollfd){ sigchld_pipe[0], POLLIN, 0 };
		who[count++] = -2;
		if (session.stdout_fd >= 0) {
			pfd[count] = (struct pollfd){ session.stdout_fd, POLLIN, 0 };
			who[count++] = -3;
		}
		if (session.inotify_fd >= 0) {
			pfd[count] = (struct pollfd){ session.inotify_fd, POLLIN, 0 };
			who[count++] = -4;
		}
		if (session.input_fd >= 0) {
			pfd[count] = (struct pollfd){ session.input_fd, POLLIN, 0 };
			who[count++] = -5;
		}
		if (session.shot_fd > 0) {
			pfd[count] = (struct pollfd){ session.shot_fd, POLLIN, 0 };
			who[count++] = -6;
		}
		if (discovery.fd >= 0) {
			pfd[count] = (struct pollfd){ discovery.fd, POLLIN, 0 };
			who[count++] = -7;
		}
		for (int i = 0; i < MAX_CLIENTS; i++) {
			if (clients[i].fd < 0)
				continue;
			pfd[count] = (struct pollfd){ clients[i].fd, POLLIN, 0 };
			who[count++] = i;
		}

		rc = poll(pfd, count, session_timeout_ms());
		if (rc < 0) {
			if (errno == EINTR)
				continue;
			break;
		}
		session_timers();
		for (nfds_t i = 0; i < count; i++) {
			if (!pfd[i].revents)
				continue;
			switch (who[i]) {
			case -1:
				accept_client(server);
				break;
			case -2:
				reap_children();
				break;
			case -3:
				if (session.stdout_fd >= 0)
					process_ra_output();
				break;
			case -4:
				if (session.inotify_fd >= 0)
					process_watch();
				break;
			case -5:
				if (session.input_fd >= 0)
					process_input_link();
				break;
			case -6:
				if (session.shot_fd > 0)
					process_shot_watch();
				break;
			case -7:
				if (discovery.fd >= 0)
					process_discovery();
				break;
			default:
				if (clients[who[i]].fd >= 0)
					process_client(&clients[who[i]]);
				break;
			}
		}
	}

	if (session.state != SESSION_IDLE && session.pid > 0) {
		/* Service stop (shutdown): let RetroArch close the content and
		 * write its save files, woken by SIGCHLD, bounded. */
		long long deadline = monotonic_ms() + QUIT_TIMEOUT_MS;
		bool exited = false;

		ra_command("SET_PAUSED 0");
		ra_command("QUIT");
		while (!exited && monotonic_ms() < deadline) {
			struct pollfd p = { sigchld_pipe[0], POLLIN, 0 };
			char drain[32];

			(void)poll(&p, 1, (int)(deadline - monotonic_ms()));
			while (read(sigchld_pipe[0], drain, sizeof(drain)) > 0)
				;
			exited = waitpid(session.pid, NULL, WNOHANG) == session.pid;
		}
		if (!exited)
			kill(-session.pid, SIGKILL);
		library_session("SESSION_END", session.game);
	}
	unlink(SOCKET_PATH);
	unlink(PIDFILE);
	return 0;
}
