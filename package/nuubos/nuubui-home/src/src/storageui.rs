/*
 * Settings → System → Storage of nuubUI, on the generic rows of home.slint
 * (see gameui.rs), and the games card's moments outside it
 * (docs/dev/platform.md "SD2 = games card"):
 *
 *   64 Storage          SD1, the games card (SD2) and its actions, then
 *                       BACKUP TO SD2
 *   59 Backup Content   what the backup to SD2 includes
 *
 * The state comes from the nuubos-systemd snapshot (storage.*), pushed on
 * every change; commands go to nuubos-systemd (eject, ignore, use, backup
 * settings) or are jobs (sd2-format, sd2-prepare, sd2-backup, sd2-restore).
 * A card seen for the first time that needs a choice (no games: create the
 * folders?; unreadable: format?) asks once, over the Storage page, never
 * over a game. No storage policy lives here.
 */

use crate::gameui::{
    apply_rows_split, arm_or, armed, armed_detail, days_ago, focused_key, move_selection, nav, notice, row, run_tool,
    show, toggle,
};
use crate::{navigate_settings_view, open_settings_choice, tr, tr_arg, GenRow, HomeWindow};
use slint::ComponentHandle;
use std::cell::RefCell;
use std::sync::atomic::Ordering;
use std::thread;

pub const STORAGE_VIEW: i32 = 64;
pub const CONTENT_VIEW: i32 = 59;

const SYSTEMCTL: &str = "/usr/bin/nuubos-systemctl";
const JOBCTL: &str = "/usr/bin/nuubos-jobctl";
const BACKUPCTL: &str = "/usr/sbin/nuubos-sd2-backup";

/* Backup items, in nuubos-sd2-backup order. */
const ITEMS: [&str; 6] = ["saves", "states", "library", "screenshots", "bios", "covers"];

/* storage.* of the nuubos-systemd snapshot. */
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Storage {
    pub health: String,
    pub action: String,
    pub sd1_total: u64,
    pub sd1_free: u64,
    pub slot: bool,
    pub state: String,
    pub cid: String,
    pub fs: String,
    pub label: String,
    pub prompt: String,
    pub total: u64,
    pub free: u64,
    pub backup_enabled: bool,
    pub backup_items: String,
    pub backup_last: i64,
    pub backup_result: String,
    pub backup_card: String,
}

impl Storage {
    pub fn parse(&mut self, key: &str, value: &str) {
        match key {
            "storage.health" => self.health = value.to_owned(),
            "storage.action_required" => self.action = value.to_owned(),
            "storage.userdata_total" => self.sd1_total = value.parse().unwrap_or(0),
            "storage.userdata_free" => self.sd1_free = value.parse().unwrap_or(0),
            "storage.sd2.slot" => self.slot = value == "1",
            "storage.sd2.state" => self.state = value.to_owned(),
            "storage.sd2.cid" => self.cid = value.to_owned(),
            "storage.sd2.fs" => self.fs = value.to_owned(),
            "storage.sd2.label" => self.label = value.to_owned(),
            "storage.sd2.prompt" => self.prompt = value.to_owned(),
            "storage.sd2.total" => self.total = value.parse().unwrap_or(0),
            "storage.sd2.free" => self.free = value.parse().unwrap_or(0),
            "storage.backup.enabled" => self.backup_enabled = value == "1",
            "storage.backup.items" => self.backup_items = value.to_owned(),
            "storage.backup.last" => self.backup_last = value.parse().unwrap_or(0),
            "storage.backup.result" => self.backup_result = value.to_owned(),
            "storage.backup.card" => self.backup_card = value.to_owned(),
            _ => {}
        }
    }

    fn mounted(&self) -> bool {
        self.state == "ready" || self.state == "readonly"
    }

    fn has_item(&self, item: &str) -> bool {
        self.backup_items.split(',').any(|i| i == item)
    }
}

#[derive(Default)]
struct State {
    storage: Storage,
    /* A job of this page runs (format, prepare, backup, restore). */
    busy: String,
    /* The card whose first-seen question was already asked. */
    asked: String,
    /* Backups on the card: (dir, label), this console's first. */
    backups: Vec<(String, String)>,
    /* The card the format confirmation is for. */
    format_cid: String,
}

thread_local! {
    static STATE: RefCell<State> = RefCell::new(State::default());
}

pub fn human(bytes: u64) -> String {
    let b = bytes as f64;
    if b >= 1e9 {
        format!("{:.1} GB", b / 1e9)
    } else if b >= 1e6 {
        format!("{:.0} MB", b / 1e6)
    } else {
        format!("{:.0} KB", b / 1e3)
    }
}

fn space(ui: &HomeWindow, free: u64, total: u64) -> String {
    tr(ui, 914, "{0} free of {1}").replace("{0}", &human(free)).replace("{1}", &human(total))
}

/* Value of the System → Storage row. */
pub fn summary(ui: &HomeWindow, s: &Storage) -> String {
    let sd1 = tr(ui, 912, "SD1");
    match s.state.as_str() {
        "ready" | "readonly" => format!("{sd1} • {}", tr(ui, 64, "SD2")),
        _ => sd1,
    }
}

fn sd2_value(ui: &HomeWindow, s: &Storage) -> (String, String) {
    let name = if s.label.is_empty() { tr(ui, 915, "Games card") } else { s.label.clone() };
    match s.state.as_str() {
        "ready" => (space(ui, s.free, s.total), name),
        "readonly" => (tr(ui, 918, "Read-only"),
            tr(ui, 919, "The card has errors: games can be played, nothing can be written to it")),
        "checking" => (tr(ui, 917, "Checking…"), String::new()),
        "unreadable" => (tr(ui, 920, "Unreadable"), tr(ui, 921, "No supported file system (exFAT, FAT32, ext4)")),
        "ejected" => (tr(ui, 922, "Ejected"), tr(ui, 627, "Safe to remove")),
        "ignored" => (tr(ui, 978, "Not in use"), tr(ui, 980, "nuubOS leaves this card alone until you choose to use it")),
        _ => (tr(ui, 916, "No card"), String::new()),
    }
}

fn last_backup(ui: &HomeWindow, s: &Storage) -> String {
    if s.backup_last > 0 {
        tr_arg(ui, 933, "Last backup: {0}", &days_ago(ui, s.backup_last))
    } else {
        tr(ui, 934, "Never backed up")
    }
}

fn build_rows(ui: &HomeWindow, st: &State, view: i32) -> (Vec<(String, GenRow, bool)>, i32) {
    let s = &st.storage;
    let mut rows: Vec<(String, GenRow, bool)> = Vec::new();
    let armed = armed();
    if view == CONTENT_VIEW {
        let mut profiles = toggle(tr(ui, 945, "Profiles & Settings"), true,
            tr(ui, 946, "Always included. Passwords and accounts are never copied."));
        profiles.enabled = false;
        rows.push(("profiles".into(), profiles, false));
        for (item, title, detail) in [
            ("saves", tr(ui, 947, "Saves"), String::new()),
            ("states", tr(ui, 948, "Save States"), String::new()),
            ("library", tr(ui, 949, "Game Library"), tr(ui, 950, "Play history and collections")),
            ("screenshots", tr(ui, 951, "Screenshots & Recordings"), String::new()),
            ("bios", tr(ui, 576, "BIOS Files"), String::new()),
            ("covers", tr(ui, 953, "Covers & Metadata"), tr(ui, 954, "Downloaded game information; it can take a lot of space")),
        ] {
            rows.push((format!("item:{item}"), toggle(title, s.has_item(item), detail), true));
        }
        return (rows, 0);
    }

    let busy = !st.busy.is_empty();
    let mut sd1 = row(tr(ui, 912, "SD1"), space(ui, s.sd1_free, s.sd1_total),
        tr(ui, 913, "System and saves"));
    if s.health == "DEGRADED" || s.health == "FAILED" {
        sd1.detail = tr(ui, 976, "Errors found: open Recovery").into();
    }
    rows.push(("sd1".into(), sd1, false));
    if s.slot {
        let (value, detail) = sd2_value(ui, s);
        rows.push(("sd2".into(), row(tr(ui, 64, "SD2"), value, detail), false));
        match s.state.as_str() {
            "ready" | "readonly" => {
                rows.push(("eject".into(), row(tr(ui, 924, "Eject SD2"), String::new(), String::new()), !busy));
            }
            "ejected" => {
                rows.push(("use".into(), row(tr(ui, 925, "Use SD2 Again"), String::new(), String::new()), !busy));
            }
            "ignored" => {
                rows.push(("use".into(), row(tr(ui, 979, "Use SD2"), String::new(), String::new()), !busy));
            }
            _ => {}
        }
        if s.state == "ready" {
            let mut prepare = row(tr(ui, 926, "Create Game Folders"), String::new(),
                tr(ui, 927, "Adds a folder for each system. Nothing is deleted."));
            if st.busy == "prepare" {
                prepare.value = tr(ui, 91, "Running").into();
            }
            rows.push(("prepare".into(), prepare, !busy));
        }
        if matches!(s.state.as_str(), "ready" | "readonly" | "unreadable" | "ignored") {
            let mut format = row(tr(ui, 928, "Format SD2"),
                if st.busy == "format" { tr(ui, 970, "Formatting…") } else { String::new() },
                armed_detail(&armed, "format", tr(ui, 930, "Press again • everything on SD2 will be erased"),
                    tr(ui, 929, "Erases everything on the card")));
            format.enabled = !busy;
            rows.push(("format".into(), format, !busy));
        }
    }
    rows.push(("media".into(), nav(tr(ui, 625, "Removable Media"), String::new()), true));

    /* BACKUP TO SD2 */
    let split = rows.len() as i32;
    if !s.slot {
        return (rows, 0);
    }
    let writable = s.state == "ready";
    let mut auto = toggle(tr(ui, 69, "Automatic Backup"), s.backup_enabled,
        if s.mounted() { last_backup(ui, s) } else { tr(ui, 935, "Insert an SD2 card to back up") });
    if s.backup_result == "failed" {
        auto.detail = tr(ui, 968, "The backup could not be completed").into();
    }
    rows.push(("auto".into(), auto, true));
    let on = ITEMS.iter().filter(|i| s.has_item(i)).count();
    rows.push(("content".into(), nav(tr(ui, 936, "Backup Content"),
        if on == ITEMS.len() { tr(ui, 937, "All") }
        else { tr(ui, 938, "{0} of {1}").replace("{0}", &on.to_string()).replace("{1}", &ITEMS.len().to_string()) }), true));
    if s.backup_card == "conflict" {
        rows.push(("archive".into(), row(tr(ui, 942, "Backup of a previous SD1"),
            if st.busy == "archive" { tr(ui, 91, "Running") } else { String::new() },
            armed_detail(&armed, "archive", tr(ui, 191, "Press again to confirm"),
                tr(ui, 943, "Restore it, or keep it aside and start a new backup"))), writable && !busy));
    } else {
        let mut now = row(tr(ui, 88, "Back Up Now"),
            if st.busy == "backup" { tr(ui, 966, "Backing up…") } else { String::new() }, String::new());
        now.enabled = writable;
        rows.push(("backup".into(), now, writable && !busy));
    }
    if matches!(s.backup_card.as_str(), "linked" | "conflict" | "restorable") {
        let mut restore = nav(tr(ui, 940, "Restore from SD2"),
            if st.busy == "restore" { tr(ui, 965, "Restoring…") } else { String::new() });
        restore.detail = tr(ui, 941, "Replaces the data on SD1 with a backup").into();
        rows.push(("restore".into(), restore, s.mounted() && !busy));
    }
    (rows, split)
}

pub fn render(ui: &HomeWindow) {
    let view = ui.get_settings_view();
    if view != STORAGE_VIEW && view != CONTENT_VIEW {
        return;
    }
    let (rows, split) = STATE.with(|st| build_rows(ui, &st.borrow(), view));
    if view == CONTENT_VIEW {
        apply_rows_split(ui, rows, tr(ui, 936, "Backup Content").to_uppercase(), 0, String::new());
    } else {
        apply_rows_split(ui, rows, tr(ui, 19, "Storage").to_uppercase(), split, tr(ui, 931, "BACKUP TO SD2"));
    }
}

/* System → Storage. */
pub fn open(ui: &HomeWindow) {
    show(ui, STORAGE_VIEW);
    render(ui);
}

/* A new nuubos-systemd snapshot. */
pub fn update(ui: &HomeWindow, s: &Storage) {
    let changed = STATE.with(|st| {
        let mut st = st.borrow_mut();
        let changed = st.storage != *s;
        st.storage = s.clone();
        changed
    });
    if changed {
        render(ui);
    }
    maybe_ask(ui);
}

/* Nothing in front that a question would interrupt. */
fn can_ask(ui: &HomeWindow) -> bool {
    !crate::library::GAME_RUNNING.load(Ordering::SeqCst)
        && !crate::oob_active()
        && !crate::mediaui::PLAYING.load(Ordering::SeqCst)
        && !crate::moonlight::STREAM_RUNNING.load(Ordering::SeqCst)
        && !crate::webui::RUNNING.load(Ordering::SeqCst)
        && !ui.get_settings_choice_open()
}

/* The first-seen question for a card: once per card and session, on the
 * Storage page. Called on every snapshot and when a game ends. */
pub fn maybe_ask(ui: &HomeWindow) {
    let (prompt, cid, label) = STATE.with(|st| {
        let st = st.borrow();
        (st.storage.prompt.clone(), st.storage.cid.clone(), st.storage.label.clone())
    });
    if prompt == "none" || prompt.is_empty() || cid.is_empty() || !can_ask(ui) {
        return;
    }
    if STATE.with(|st| st.borrow().asked == cid) {
        return;
    }
    STATE.with(|st| st.borrow_mut().asked = cid.clone());
    if ui.get_settings_view() != STORAGE_VIEW {
        crate::open_settings_at(ui, STORAGE_VIEW);
        open(ui);
    }
    let name = if label.is_empty() { tr(ui, 64, "SD2") } else { label };
    if prompt == "prepare" {
        open_settings_choice(ui, "sd2-prompt", &format!("{} • {}", tr(ui, 955, "New SD2 card: no games found"), name),
            vec![("prepare".to_owned(), tr(ui, 926, "Create Game Folders")),
                 ("ignore".to_owned(), tr(ui, 956, "Don't Use"))], "prepare");
    } else {
        open_settings_choice(ui, "sd2-prompt", &tr(ui, 957, "SD2 can't be read"),
            vec![("format".to_owned(), tr(ui, 928, "Format SD2")),
                 ("ignore".to_owned(), tr(ui, 956, "Don't Use"))], "ignore");
    }
}

fn system(ui: &HomeWindow, args: Vec<String>, done: fn(&HomeWindow, String)) {
    let weak = ui.as_weak();
    thread::spawn(move || {
        let refs: Vec<&str> = args.iter().map(String::as_str).collect();
        let out = run_tool(SYSTEMCTL, &refs, None);
        let _ = slint::invoke_from_event_loop(move || {
            if let Some(ui) = weak.upgrade() {
                done(&ui, out);
            }
        });
    });
}

/* A job of this page: busy while it runs, then its outcome as a notice. */
fn job(ui: &HomeWindow, busy: &'static str, args: Vec<String>, done: fn(&HomeWindow, bool, String)) {
    STATE.with(|st| st.borrow_mut().busy = busy.to_owned());
    render(ui);
    let weak = ui.as_weak();
    thread::spawn(move || {
        let mut all = vec!["run".to_owned()];
        all.extend(args);
        let refs: Vec<&str> = all.iter().map(String::as_str).collect();
        let out = run_tool(JOBCTL, &refs, None);
        let ok = !out.starts_with("ERR") && !out.is_empty();
        let _ = slint::invoke_from_event_loop(move || {
            let Some(ui) = weak.upgrade() else { return };
            STATE.with(|st| st.borrow_mut().busy.clear());
            done(&ui, ok, out.trim().to_owned());
            render(&ui);
        });
    });
}

fn start_format(ui: &HomeWindow, cid: String) {
    job(ui, "format", vec!["sd2-format".into(), cid], |ui, ok, _| {
        notice(ui, if ok { tr(ui, 971, "SD2 formatted") } else { tr(ui, 972, "The card could not be formatted") });
    });
}

fn start_prepare(ui: &HomeWindow) {
    job(ui, "prepare", vec!["sd2-prepare".into()], |ui, ok, _| {
        notice(ui, if ok { tr(ui, 92, "Completed") } else { tr(ui, 93, "Failed") });
    });
}

fn start_backup(ui: &HomeWindow) {
    job(ui, "backup", vec!["sd2-backup".into()], |ui, ok, _| {
        notice(ui, if ok { tr(ui, 73, "Backup completed") } else { tr(ui, 968, "The backup could not be completed") });
    });
}

/* Restore, from Settings or the OOB: every service reloads the restored
 * settings through the central restart path. */
pub fn start_restore(ui: &HomeWindow, dir: String) {
    job(ui, "restore", vec!["sd2-restore".into(), dir], |ui, ok, out| {
        if ok && out == "restart-required" {
            notice(ui, tr(ui, 638, "Restore complete"));
            thread::spawn(|| { let _ = run_tool(SYSTEMCTL, &["restart"], None); });
        } else {
            notice(ui, tr(ui, 640, "The backup could not be restored"));
        }
    });
}

/* Backups on the card, this console's first, then the newest. */
fn load_backups(ui: &HomeWindow, then: fn(&HomeWindow)) {
    let weak = ui.as_weak();
    thread::spawn(move || {
        let out = run_tool(BACKUPCTL, &["list"], None);
        let mut list: Vec<(bool, i64, String, String, String)> = out.lines()
            .filter_map(|l| l.strip_prefix("backup="))
            .map(|l| l.split('\t').collect::<Vec<_>>())
            .filter(|f| f.len() >= 6)
            .map(|f| (f[5] == "1", f[3].parse().unwrap_or(0), f[0].to_owned(), f[2].to_owned(), f[4].replace(',', ", ")))
            .collect();
        list.sort_by(|a, b| b.0.cmp(&a.0).then(b.1.cmp(&a.1)));
        let _ = slint::invoke_from_event_loop(move || {
            let Some(ui) = weak.upgrade() else { return };
            let backups = list.into_iter().map(|(this, created, dir, model, users)| {
                let who = if this { tr(&ui, 962, "This console") } else { model };
                let mut label = format!("{who} • {}", days_ago(&ui, created));
                if !users.is_empty() {
                    label.push_str(&format!(" • {users}"));
                }
                (dir, label)
            }).collect();
            STATE.with(|st| st.borrow_mut().backups = backups);
            then(&ui);
        });
    });
}

fn choose_backup(ui: &HomeWindow) {
    let options = STATE.with(|st| st.borrow().backups.clone());
    if options.is_empty() {
        return;
    }
    let first = options[0].0.clone();
    open_settings_choice(ui, "sd2-restore", &tr(ui, 940, "Restore from SD2"), options, &first);
}

/* OOB, after the language: a backup on the card can bring everything back
 * instead of a new setup. */
pub fn offer_restore(ui: &HomeWindow) -> bool {
    let card = STATE.with(|st| st.borrow().storage.backup_card.clone());
    if !matches!(card.as_str(), "linked" | "conflict" | "restorable") {
        return false;
    }
    load_backups(ui, |ui| {
        let mut options = STATE.with(|st| st.borrow().backups.clone());
        if options.is_empty() {
            return;
        }
        let first = options[0].0.clone();
        options.push(("scratch".to_owned(), tr(ui, 964, "Start from Scratch")));
        open_settings_choice(ui, "sd2-oob-restore", &tr(ui, 963, "Backup found on SD2"), options, &first);
    });
    true
}

fn set_item(ui: &HomeWindow, item: &str, on: bool) {
    /* Shown at once; the snapshot confirms. */
    STATE.with(|st| {
        let mut st = st.borrow_mut();
        let mut items: Vec<String> = st.storage.backup_items.split(',').filter(|i| !i.is_empty() && *i != item)
            .map(str::to_owned).collect();
        if on {
            items.push(item.to_owned());
        }
        st.storage.backup_items = ITEMS.iter().filter(|i| items.iter().any(|x| x == *i)).copied()
            .collect::<Vec<_>>().join(",");
    });
    render(ui);
    system(ui, vec!["set-storage-backup-item".into(), item.to_owned(), if on { "1" } else { "0" }.into()], |_, _| {});
}

fn confirm(ui: &HomeWindow, view: i32, key: &str) {
    let s = STATE.with(|st| st.borrow().storage.clone());
    if view == CONTENT_VIEW {
        if let Some(item) = key.strip_prefix("item:") {
            set_item(ui, item, !s.has_item(item));
        }
        return;
    }
    match key {
        "eject" => system(ui, vec!["sd2".into(), "eject".into()], |ui, out| {
            notice(ui, if out.starts_with("OK") { tr(ui, 627, "Safe to remove") }
                else { tr(ui, 628, "In use; close what uses it first") });
        }),
        "use" => system(ui, vec!["sd2".into(), "use".into()], |_, _| {}),
        "prepare" => start_prepare(ui),
        "format" => {
            if arm_or(ui, "format") {
                STATE.with(|st| st.borrow_mut().format_cid = s.cid.clone());
                start_format(ui, s.cid.clone());
            } else {
                render(ui);
            }
        }
        "media" => crate::filesui::open_media(ui),
        "auto" => {
            STATE.with(|st| st.borrow_mut().storage.backup_enabled = !s.backup_enabled);
            render(ui);
            system(ui, vec!["set-storage-backup".into(), if s.backup_enabled { "0" } else { "1" }.into()], |_, _| {});
        }
        "content" => show(ui, CONTENT_VIEW),
        "backup" => start_backup(ui),
        "archive" => {
            if arm_or(ui, "archive") {
                STATE.with(|st| st.borrow_mut().busy = "archive".into());
                system(ui, vec!["storage-backup-archive".into()], |ui, out| {
                    STATE.with(|st| st.borrow_mut().busy.clear());
                    if !out.starts_with("OK") {
                        notice(ui, tr(ui, 93, "Failed"));
                    }
                    render(ui);
                });
            }
            render(ui);
        }
        "restore" => load_backups(ui, choose_backup),
        _ => {}
    }
}

/* Input on pages 64 and 59 (gameui::handle_page). */
pub fn handle_page(ui: &HomeWindow, view: i32, action: &str) {
    match action {
        "menu_up" | "menu_down" => {
            move_selection(ui, action);
            render(ui);
        }
        "menu_back" => {
            if view == CONTENT_VIEW {
                show(ui, STORAGE_VIEW);
                render(ui);
            } else {
                navigate_settings_view(ui, 19);
            }
        }
        "menu_confirm" => confirm(ui, view, &focused_key(ui)),
        _ => {}
    }
}

/* Dropdown choices opened by this module. */
pub fn apply_choice(ui: &HomeWindow, context: &str, value: String) {
    match context {
        "sd2-prompt" => match value.as_str() {
            "prepare" => start_prepare(ui),
            "format" => {
                let (cid, size) = STATE.with(|st| {
                    let st = st.borrow();
                    (st.storage.cid.clone(), human(st.storage.total))
                });
                STATE.with(|st| st.borrow_mut().format_cid = cid);
                open_settings_choice(ui, "sd2-format", &tr_arg(ui, 958, "Erase everything on SD2 ({0})?", &size),
                    vec![("format".to_owned(), tr(ui, 959, "Format")), ("cancel".to_owned(), tr(ui, 619, "Cancel"))],
                    "cancel");
            }
            _ => system(ui, vec!["sd2".into(), "ignore".into()], |_, _| {}),
        },
        "sd2-format" if value == "format" => {
            let cid = STATE.with(|st| st.borrow().format_cid.clone());
            /* The card must still be the one the user confirmed. */
            if !cid.is_empty() && STATE.with(|st| st.borrow().storage.cid == cid) {
                start_format(ui, cid);
            }
        }
        "sd2-restore" => {
            let label = STATE.with(|st| st.borrow().backups.iter().find(|b| b.0 == value).map(|b| b.1.clone()))
                .unwrap_or_default();
            STATE.with(|st| st.borrow_mut().format_cid = value);
            open_settings_choice(ui, "sd2-restore-confirm", &tr_arg(ui, 960, "Restore \"{0}\"?", &label),
                vec![("restore".to_owned(), tr(ui, 635, "Restore")), ("cancel".to_owned(), tr(ui, 619, "Cancel"))],
                "cancel");
        }
        "sd2-restore-confirm" if value == "restore" => {
            let dir = STATE.with(|st| std::mem::take(&mut st.borrow_mut().format_cid));
            if !dir.is_empty() {
                start_restore(ui, dir);
            }
        }
        "sd2-oob-restore" if value != "scratch" => {
            crate::oob_notice(ui, tr(ui, 965, "Restoring…"));
            start_restore(ui, value);
        }
        _ => {}
    }
}

/* Whether a dropdown context belongs to this module. */
pub fn owns_choice(context: &str) -> bool {
    context.starts_with("sd2-")
}
