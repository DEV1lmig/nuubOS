/*
 * Syncthing in nuubUI (EPIC-034): Settings → Gaming, section SYNCTHING
 * (user request 2026-10-12, it was in Connectivity), its page
 * (settings-view 66) and the full-screen setup guide.
 *
 * nuubos-syncd owns the capability: this module subscribes to its snapshot
 * (/run/nuubos/syncd.sock, SUBSCRIBE), shows it and sends its commands. No
 * polling: devices asking to connect, consoles appearing on the network
 * and sync progress arrive as new snapshots.
 *
 * The guide (user request 2026-10-12) walks through one pairing at a time:
 *   0 what to sync with (another console | a computer or server | a phone)
 *   1 what to sync (turns Syncthing on)
 *   2 prepare the other device
 *   3 pair: console = pick it among the nearby devices (or accept its
 *     request); computer = it adds this console (nearby list, or the full
 *     ID), accept its request; phone = it scans the QR code, accept
 *   4 console: wait for the other console to accept (then 5 by itself);
 *     computer/phone: accept the nuubOS folders there
 *   5 done
 * plus a single screen with this console's ID and QR code, and the same
 * full screen for other modules (show_screen: Tailscale's sign-in code).
 *
 * Existing shares (user report 2026-10-21: the other devices already share
 * saves and states): section AVAILABLE FOLDERS lists the folders paired
 * devices share that this console does not sync yet (name, folder ID,
 * device); pressing one asks what it syncs as here: saves, states or
 * screenshots not linked to such a folder yet, or ignore it.
 */

use crate::gameui::{apply_rows_sections3, nav, notice, row, toggle};
use crate::{move_model_selection, open_settings_choice, tr, tr_arg, GenRow, HomeWindow};
use slint::{ComponentHandle, Image, ModelRc, Rgba8Pixel, SharedPixelBuffer, VecModel};
use std::cell::RefCell;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::rc::Rc;
use std::thread;
use std::time::Duration;

const SOCKET: &str = "/run/nuubos/syncd.sock";
pub const VIEW: i32 = 66;
/* Kinds that may use an existing folder (syncd custom_kind). */
const CUSTOM_KINDS: [(&str, usize, &str); 3] = [("saves", 673, "Saves"), ("states", 674, "Save States"), ("screenshots", 675, "Screenshots")];
/* What can be synchronized: (kind, title, fallback, detail, fallback). */
const KINDS: [(&str, usize, &str, usize, &str); 5] = [
    ("saves", 673, "Saves", 0, ""),
    ("states", 674, "Save States", 0, ""),
    ("screenshots", 675, "Screenshots", 0, ""),
    ("library", 1089, "Covers & Metadata", 1090, "Shared by every profile on this console"),
    ("history", 1091, "Play Activity", 1092, "Time played and Recently Played, added up across your devices"),
];

fn kind_detail(ui: &HomeWindow, idx: usize, fallback: &str) -> String {
    if idx == 0 { String::new() } else { tr(ui, idx, fallback) }
}

/* Guide kinds. */
const CONSOLE: u8 = 1;
const COMPUTER: u8 = 2;
const PHONE: u8 = 3;
const SHOW_ID: u8 = 4;
/* A screen another module fills (show_screen). */
const EXTERNAL: u8 = 5;

/* What another module shows on the full screen: a QR code of qr_text (also
 * printed under it), or waiting while it has none. */
#[derive(Clone, Default)]
pub struct ExternalScreen {
    pub title: String,
    pub body: String,
    pub qr_text: String,
    pub waiting: String,
}

#[derive(Clone, Default)]
struct Device {
    id: String,
    name: String,
    connected: bool,
    /* 0..100, -1 unknown */
    pct: i32,
    addr: String,
}

/* A folder a paired device shares that this console does not use. */
#[derive(Clone, Default)]
struct Offer {
    folder: String,
    label: String,
    device: String,
    device_name: String,
}

#[derive(Clone, Default)]
struct Snapshot {
    enabled: bool,
    ready: bool,
    failed: bool,
    internet: bool,
    relays: bool,
    id: String,
    name: String,
    /* (kind, on, Syncthing state) */
    folders: Vec<(String, bool, String)>,
    devices: Vec<Device>,
    pending: Vec<Device>,
    nearby: Vec<Device>,
    /* (kind, Syncthing folder id, an existing folder chosen) */
    folder_ids: Vec<(String, String, bool)>,
    offers: Vec<Offer>,
}

#[derive(Default)]
struct Guide {
    open: bool,
    kind: u8,
    step: u8,
    /* The device paired in this guide. */
    target: String,
    target_name: String,
    error: String,
    busy: bool,
    /* Item keys on screen; "" = not actionable. */
    keys: Vec<String>,
    qr_for: String,
    external: ExternalScreen,
}

#[derive(Default)]
struct State {
    snap: Snapshot,
    guide: Guide,
}

thread_local! {
    static STATE: RefCell<State> = RefCell::new(State::default());
}

fn parse(text: &str) -> Snapshot {
    let mut s = Snapshot::default();
    for line in text.lines() {
        let Some((k, v)) = line.split_once('=') else { continue };
        let f: Vec<&str> = v.split('\t').collect();
        let field = |i: usize| f.get(i).copied().unwrap_or("").to_owned();
        match k {
            "enabled" => s.enabled = v == "1",
            "ready" => s.ready = v == "1",
            "failed" => s.failed = v == "1",
            "internet" => s.internet = v == "1",
            "relays" => s.relays = v == "1",
            "device_id" => s.id = v.to_owned(),
            "device_name" => s.name = v.to_owned(),
            "folder" => s.folders.push((field(0), field(1) == "1", field(2))),
            "device" => s.devices.push(Device {
                id: field(0),
                name: field(1),
                connected: field(2) == "1",
                pct: field(3).parse().unwrap_or(-1),
                addr: field(4),
            }),
            "pending" => s.pending.push(Device { id: field(0), name: field(1), addr: field(2), pct: -1, ..Default::default() }),
            "nearby" => s.nearby.push(Device { id: field(0), addr: field(1), pct: -1, ..Default::default() }),
            "folderid" => s.folder_ids.push((field(0), field(1), field(2) == "1")),
            "offer" => s.offers.push(Offer { folder: field(0), label: field(1), device: field(2), device_name: field(3) }),
            _ => {}
        }
    }
    s
}

/* The 7-character code Syncthing itself shows for a device. */
fn short(id: &str) -> String {
    id.chars().take(7).collect()
}

fn device_label(ui: &HomeWindow, d: &Device) -> String {
    if d.name.is_empty() { tr_arg(ui, 1059, "Device {0}", &short(&d.id)) } else { d.name.clone() }
}

/* "AAAAAAA-BBBBBBB-CCCCCCC-DDDDDDD-\nEEEEEEE-...": two lines. */
fn split_id(id: &str) -> String {
    let groups: Vec<&str> = id.split('-').collect();
    if groups.len() < 8 {
        return id.to_owned();
    }
    format!("{}-\n{}", groups[..4].join("-"), groups[4..].join("-"))
}

/* ---------------------------------------------------------------- */
/* Service                                                          */
/* ---------------------------------------------------------------- */

/* Follows the snapshot for the whole session (nuubos-syncd only writes
 * when something changed). Reconnects with a growing pause if syncd is
 * absent. */
pub fn start_listener(ui: &HomeWindow) {
    let weak = ui.as_weak();
    thread::spawn(move || {
        let mut pause = 500;
        loop {
            if let Ok(mut s) = UnixStream::connect(SOCKET) {
                if s.write_all(b"SUBSCRIBE\n").is_ok() {
                    pause = 500;
                    let mut buf = String::new();
                    for line in BufReader::new(s).lines() {
                        let Ok(l) = line else { break };
                        if l == "end=1" {
                            let snap = parse(&buf);
                            buf.clear();
                            let w = weak.clone();
                            let _ = slint::invoke_from_event_loop(move || {
                                if let Some(ui) = w.upgrade() {
                                    apply(&ui, snap);
                                }
                            });
                        } else {
                            buf.push_str(&l);
                            buf.push('\n');
                        }
                    }
                }
            }
            thread::sleep(Duration::from_millis(pause));
            pause = (pause * 2).min(30_000);
        }
    });
}

fn request(line: &str) -> String {
    let Ok(mut s) = UnixStream::connect(SOCKET) else { return "ERR service".into() };
    /* ADD and IGNORE reconfigure Syncthing. */
    s.set_read_timeout(Some(Duration::from_secs(45))).ok();
    if s.write_all(line.as_bytes()).and_then(|_| s.write_all(b"\n")).is_err() {
        return "ERR service".into();
    }
    s.shutdown(std::net::Shutdown::Write).ok();
    let mut reply = String::new();
    let _ = BufReader::new(s).read_line(&mut reply);
    if reply.is_empty() { "ERR service".into() } else { reply.trim().to_owned() }
}

fn command(ui: &HomeWindow, line: String, done: fn(&HomeWindow, String)) {
    let weak = ui.as_weak();
    thread::spawn(move || {
        let reply = request(&line);
        let _ = slint::invoke_from_event_loop(move || {
            if let Some(ui) = weak.upgrade() {
                done(&ui, reply);
            }
        });
    });
}

fn error_text(ui: &HomeWindow, reply: &str) -> String {
    if reply.contains("not-running") {
        tr(ui, 1085, "Syncthing is still starting. Try again in a moment.")
    } else {
        tr(ui, 93, "Failed")
    }
}

/* Page commands: an error shows as the page notice; the change itself
 * comes back as a snapshot. */
fn page_done(ui: &HomeWindow, reply: String) {
    if reply.starts_with("ERR") {
        notice(ui, error_text(ui, &reply));
    }
}

fn apply(ui: &HomeWindow, snap: Snapshot) {
    STATE.with(|st| st.borrow_mut().snap = snap);
    if guide_open() {
        guide_follow(ui);
        render_guide(ui);
    }
    let view = ui.get_settings_view();
    if ui.get_settings_open() && (view == VIEW || view == crate::gameui::GAMING_VIEW || view == 0) {
        crate::gameui::render(ui);
    }
}

/* ---------------------------------------------------------------- */
/* Status                                                           */
/* ---------------------------------------------------------------- */

fn status_line(ui: &HomeWindow, s: &Snapshot) -> String {
    let syncing = s.folders.iter().any(|f| f.1 && matches!(f.2.as_str(), "syncing" | "sync-preparing" | "sync-waiting"))
        || s.devices.iter().any(|d| d.connected && (0..100).contains(&d.pct));
    if s.failed {
        tr(ui, 677, "Syncthing stopped unexpectedly")
    } else if !s.enabled {
        tr(ui, 670, "Keeps your saves in sync with your other devices. It is not a backup.")
    } else if !s.ready {
        tr(ui, 1025, "Starting…")
    } else if s.devices.is_empty() {
        tr(ui, 1027, "No devices yet")
    } else if s.folders.iter().any(|f| f.1 && f.2 == "error") {
        tr(ui, 1084, "Cannot sync")
    } else if !s.devices.iter().any(|d| d.connected) {
        tr(ui, 1026, "Waiting for your devices")
    } else if syncing {
        tr(ui, 323, "Syncing…")
    } else {
        tr(ui, 489, "Up to date")
    }
}

/* The Syncthing row of Settings → Gaming: (value, detail, enabled). */
pub fn summary(ui: &HomeWindow) -> (String, String, bool) {
    STATE.with(|st| {
        let s = &st.borrow().snap;
        (if s.enabled { tr(ui, 222, "On") } else { tr(ui, 54, "Off") }, status_line(ui, s), s.enabled)
    })
}

fn device_value(ui: &HomeWindow, d: &Device) -> String {
    if !d.connected {
        tr(ui, 159, "Not connected")
    } else if (0..100).contains(&d.pct) {
        tr_arg(ui, 1039, "{0}% synced", &d.pct.to_string())
    } else {
        tr(ui, 198, "Connected")
    }
}

/* ---------------------------------------------------------------- */
/* Settings page 66                                                 */
/* ---------------------------------------------------------------- */

pub fn render_page(ui: &HomeWindow) {
    let s = STATE.with(|st| st.borrow().snap.clone());
    let mut rows: Vec<(String, GenRow, bool)> = Vec::new();
    rows.push(("sync-enabled".into(), toggle(tr(ui, 669, "Syncthing"), s.enabled, status_line(ui, &s)), true));
    let (mut split, mut split2, mut split3) = (0, 0, 0);
    if s.enabled {
        let mut add = nav(tr(ui, 1028, "Add a Device"), String::new());
        add.detail = tr(ui, 1029, "Another console, a computer or a phone").into();
        rows.push(("sync-add".into(), add, true));
        rows.push(("sync-me".into(), row(tr(ui, 1037, "This Console"), s.name.clone(),
            if s.ready { tr_arg(ui, 1038, "Code {0} • Press to show the full ID", &short(&s.id)) } else { String::new() }),
            s.ready));
        split = rows.len() as i32;
        for p in &s.pending {
            rows.push((format!("sync-req:{}", p.id), row(device_label(ui, p), String::new(),
                tr(ui, 1032, "Wants to sync with you")), true));
        }
        for d in &s.devices {
            rows.push((format!("sync-dev:{}", d.id), row(device_label(ui, d), device_value(ui, d), d.addr.clone()), true));
        }
        if s.pending.is_empty() && s.devices.is_empty() {
            let mut r = row(tr(ui, 1027, "No devices yet"), String::new(), String::new());
            r.enabled = false;
            rows.push(("info".into(), r, false));
        }
        split2 = rows.len() as i32;
        for (kind, idx, fallback, didx, dfallback) in KINDS {
            let (on, state) = s.folders.iter().find(|f| f.0 == kind).map(|f| (f.1, f.2.clone())).unwrap_or_default();
            /* The Syncthing folder ID, to recognize it on the other devices. */
            let folder_id = s.folder_ids.iter().find(|f| f.0 == kind).map(|f| f.1.clone());
            let detail = match state.as_str() {
                _ if !on => kind_detail(ui, didx, dfallback),
                "idle" | "" if folder_id.is_some() => tr_arg(ui, 1324, "Folder ID {0}", folder_id.as_deref().unwrap_or("")),
                "syncing" | "sync-preparing" | "sync-waiting" => tr(ui, 323, "Syncing…"),
                "scanning" | "scan-waiting" => tr(ui, 488, "Checking…"),
                "error" => tr(ui, 1084, "Cannot sync"),
                _ => kind_detail(ui, didx, dfallback),
            };
            rows.push((format!("sync-folder:{kind}"), toggle(tr(ui, idx, fallback), on, detail), true));
        }
        rows.push(("sync-internet".into(), toggle(tr(ui, 1035, "Sync Away from Home"), s.internet,
            tr(ui, 1036, "Finds your devices over the Internet; your data still goes straight between them")), true));
        /* Relays carry the encrypted data: a separate choice, only
         * meaningful away from home. */
        let mut relays = toggle(tr(ui, 1087, "Allow Relays"), s.internet && s.relays,
            tr(ui, 1088, "If a direct connection is impossible, the encrypted data may pass through public Syncthing servers"));
        relays.enabled = s.internet;
        rows.push(("sync-relays".into(), relays, s.internet));
        /* Folders the other devices share and this console does not sync
         * yet; none: no section. */
        if !s.offers.is_empty() {
            split3 = rows.len() as i32;
            for o in &s.offers {
                rows.push((format!("sync-offer:{}", o.folder), row(offer_label(o), o.folder.clone(),
                    tr_arg(ui, 1325, "Shared by {0}", &offer_device(ui, o))), true));
            }
        }
    }
    apply_rows_sections3(ui, rows, tr(ui, 669, "Syncthing").to_uppercase(),
        (split, tr(ui, 1030, "DEVICES")), (split2, tr(ui, 1031, "WHAT TO SYNC")),
        (split3, tr(ui, 1343, "AVAILABLE FOLDERS")), &[]);
}

fn find(list: &[Device], id: &str) -> Option<Device> {
    list.iter().find(|d| d.id == id).cloned()
}

fn offer_label(o: &Offer) -> String {
    if o.label.is_empty() { o.folder.clone() } else { o.label.clone() }
}

fn offer_device(ui: &HomeWindow, o: &Offer) -> String {
    if o.device_name.is_empty() { tr_arg(ui, 1059, "Device {0}", &short(&o.device)) } else { o.device_name.clone() }
}

/* The existing folder chosen for a kind, "" = nuubOS's own. */
fn custom_id(s: &Snapshot, kind: &str) -> String {
    s.folder_ids.iter().find(|f| f.0 == kind && f.2).map(|f| f.1.clone()).unwrap_or_default()
}

pub fn page_confirm(ui: &HomeWindow, key: &str) {
    let s = STATE.with(|st| st.borrow().snap.clone());
    match key {
        "sync-enabled" if s.enabled => command(ui, "DISABLE".into(), page_done),
        /* The first time: the guide (it turns Syncthing on at step 1). */
        "sync-enabled" if s.devices.is_empty() => open_guide(ui),
        "sync-enabled" => command(ui, "ENABLE".into(), page_done),
        "sync-add" => open_guide(ui),
        "sync-me" => open_id(ui),
        "sync-internet" => command(ui, format!("INTERNET\t{}", u8::from(!s.internet)), page_done),
        "sync-relays" if s.internet => command(ui, format!("RELAYS\t{}", u8::from(!s.relays)), page_done),
        k if k.starts_with("sync-folder:") => {
            let kind = &k[12..];
            let on = s.folders.iter().any(|f| f.0 == kind && f.1);
            if custom_id(&s, kind).is_empty() {
                command(ui, format!("FOLDER\t{kind}\t{}", u8::from(!on)), page_done);
            } else {
                /* Linked to another device's folder: on/off, or back to
                 * nuubOS's own folder (user request 2026-10-21). */
                let title = KINDS.iter().find(|x| x.0 == kind).map(|x| tr(ui, x.1, x.2)).unwrap_or_default();
                let options = vec![
                    ("toggle".to_owned(), if on { tr(ui, 1345, "Turn Off") } else { tr(ui, 909, "Turn On") }),
                    ("own".to_owned(), tr(ui, 1346, "Use the nuubOS Folder")),
                ];
                open_settings_choice(ui, &format!("sync-linked:{kind}"), &title, options, "toggle");
            }
        }
        k if k.starts_with("sync-req:") => {
            if let Some(p) = find(&s.pending, &k[9..]) {
                open_settings_choice(ui, k, &device_label(ui, &p),
                    vec![("accept".into(), tr(ui, 1033, "Accept")), ("ignore".into(), tr(ui, 1034, "Ignore"))], "accept");
            }
        }
        k if k.starts_with("sync-dev:") => {
            if let Some(d) = find(&s.devices, &k[9..]) {
                open_settings_choice(ui, k, &device_label(ui, &d), vec![("remove".into(), tr(ui, 644, "Remove"))], "remove");
            }
        }
        /* What the offered folder syncs as here: a kind not linked to
         * another device's folder yet, or ignore it. */
        k if k.starts_with("sync-offer:") => {
            if let Some(o) = s.offers.iter().find(|o| o.folder == k[11..]) {
                let mut options: Vec<(String, String)> = CUSTOM_KINDS.iter()
                    .filter(|(kind, _, _)| custom_id(&s, kind).is_empty())
                    .map(|(kind, idx, fallback)| ((*kind).to_owned(), tr(ui, *idx, fallback))).collect();
                options.push(("decline".into(), tr(ui, 1034, "Ignore")));
                let first = options[0].0.clone();
                open_settings_choice(ui, k, &tr_arg(ui, 1344, "Sync {0} as", &offer_label(o)), options, &first);
            }
        }
        _ => {}
    }
}

pub fn owns_choice(context: &str) -> bool {
    context.starts_with("sync-req:") || context.starts_with("sync-dev:") || context.starts_with("sync-offer:")
        || context.starts_with("sync-linked:")
}

pub fn apply_choice(ui: &HomeWindow, context: &str, value: String) {
    if let Some(kind) = context.strip_prefix("sync-linked:") {
        let on = STATE.with(|st| st.borrow().snap.folders.iter().any(|f| f.0 == kind && f.1));
        let line = if value == "own" { format!("FOLDER_ID\t{kind}\t-") } else { format!("FOLDER\t{kind}\t{}", u8::from(!on)) };
        command(ui, line, page_done);
        return;
    }
    if let Some(folder) = context.strip_prefix("sync-offer:") {
        let line = if value == "decline" { format!("DECLINE\t{folder}") } else { format!("USE\t{folder}\t{value}") };
        command(ui, line, page_done);
        return;
    }
    let id = &context[9..];
    let line = match (&context[..9], value.as_str()) {
        ("sync-req:", "accept") => {
            let name = STATE.with(|st| find(&st.borrow().snap.pending, id).map(|p| p.name).unwrap_or_default());
            format!("ADD\t{id}\t{name}")
        }
        ("sync-req:", "ignore") => format!("IGNORE\t{id}"),
        ("sync-dev:", "remove") => format!("REMOVE\t{id}"),
        _ => return,
    };
    command(ui, line, page_done);
}

/* ---------------------------------------------------------------- */
/* Guide                                                            */
/* ---------------------------------------------------------------- */

pub fn guide_open() -> bool {
    STATE.with(|st| st.borrow().guide.open)
}

pub fn open_guide(ui: &HomeWindow) {
    STATE.with(|st| st.borrow_mut().guide = Guide { open: true, ..Default::default() });
    ui.set_sync_guide_index(0);
    ui.set_sync_guide_open(true);
    render_guide(ui);
}

fn open_id(ui: &HomeWindow) {
    STATE.with(|st| st.borrow_mut().guide = Guide { open: true, kind: SHOW_ID, ..Default::default() });
    ui.set_sync_guide_index(-1);
    ui.set_sync_guide_open(true);
    render_guide(ui);
}

fn close_guide(ui: &HomeWindow) {
    STATE.with(|st| st.borrow_mut().guide = Guide::default());
    ui.set_sync_guide_open(false);
    if ui.get_settings_open() {
        crate::gameui::render(ui);
    }
}

/* What the QR code of the open screen encodes. */
fn qr_text(st: &State) -> String {
    if st.guide.kind == EXTERNAL { st.guide.external.qr_text.clone() } else { st.snap.id.clone() }
}

/* Opens (or updates, when it is already showing) another module's screen. */
pub fn show_screen(ui: &HomeWindow, screen: ExternalScreen) {
    let opening = STATE.with(|st| {
        let mut st = st.borrow_mut();
        let opening = !(st.guide.open && st.guide.kind == EXTERNAL);
        if opening {
            st.guide = Guide { open: true, kind: EXTERNAL, ..Default::default() };
        }
        st.guide.external = screen;
        opening
    });
    if opening {
        ui.set_sync_guide_index(-1);
        ui.set_sync_guide_open(true);
    }
    render_guide(ui);
}

pub fn external_open() -> bool {
    STATE.with(|st| {
        let g = &st.borrow().guide;
        g.open && g.kind == EXTERNAL
    })
}

pub fn close_external(ui: &HomeWindow) {
    if external_open() {
        close_guide(ui);
    }
}

fn go(ui: &HomeWindow, step: u8) {
    STATE.with(|st| {
        let mut st = st.borrow_mut();
        st.guide.step = step;
        st.guide.error.clear();
        if step == 0 {
            st.guide.kind = 0;
        }
    });
    ui.set_sync_guide_index(0);
    render_guide(ui);
}

/* Snapshot-driven steps: the other console accepted. */
fn guide_follow(ui: &HomeWindow) {
    let next = STATE.with(|st| {
        let st = st.borrow();
        let g = &st.guide;
        (g.kind == CONSOLE && (g.step == 3 || g.step == 4) && !g.target.is_empty()
            && st.snap.devices.iter().any(|d| d.id == g.target && d.connected))
            .then_some(5u8)
    });
    if let Some(step) = next {
        go(ui, step);
    }
}

fn numbered(lines: &[String]) -> String {
    lines.iter().enumerate().map(|(i, l)| format!("{}. {l}", i + 1)).collect::<Vec<_>>().join("\n")
}

/* The image of the QR code: one pixel per module, a 4-module quiet zone. */
fn qr_image(text: &str) -> Option<Image> {
    let (size, dark) = crate::qr::encode(text)?;
    let side = size + 8;
    let mut pixels = SharedPixelBuffer::<Rgba8Pixel>::new(side as u32, side as u32);
    let buf = pixels.make_mut_slice();
    for (i, px) in buf.iter_mut().enumerate() {
        let (x, y) = (i % side, i / side);
        let on = x >= 4 && y >= 4 && x < size + 4 && y < size + 4 && dark[(y - 4) * size + (x - 4)];
        let v = if on { 0 } else { 255 };
        *px = Rgba8Pixel { r: v, g: v, b: v, a: 255 };
    }
    Some(Image::from_rgba8(pixels))
}

struct Screen {
    title: String,
    body: String,
    code: String,
    id: String,
    qr: bool,
    waiting: String,
    /* (key, row); key "" = not actionable */
    items: Vec<(String, GenRow)>,
}

fn screen(ui: &HomeWindow, s: &Snapshot, g: &Guide) -> Screen {
    let mut sc = Screen { title: String::new(), body: String::new(), code: String::new(), id: String::new(), qr: false,
        waiting: String::new(), items: Vec::new() };
    let next = |label: usize, fallback: &str| (String::from("next"), row(tr(ui, label, fallback), String::new(), String::new()));
    let starting = tr(ui, 1025, "Starting…");
    let request_rows = |sc: &mut Screen| {
        for p in &s.pending {
            sc.items.push((format!("add:{}", p.id), row(device_label(ui, p), p.addr.clone(), tr(ui, 1032, "Wants to sync with you"))));
        }
    };
    let target_row = |sc: &mut Screen| {
        if let Some(d) = s.devices.iter().find(|d| d.id == g.target) {
            sc.items.push((String::new(), row(device_label(ui, d), device_value(ui, d), String::new())));
        }
    };
    if g.kind == EXTERNAL {
        let e = &g.external;
        sc.title = e.title.clone();
        sc.body = e.body.clone();
        sc.qr = !e.qr_text.is_empty();
        sc.id = e.qr_text.clone();
        sc.waiting = if sc.qr { String::new() } else { e.waiting.clone() };
        return sc;
    }
    if g.kind == SHOW_ID {
        sc.title = tr(ui, 671, "This Console's ID");
        sc.body = tr(ui, 1082, "Add it on your other device, or scan the code with your phone.");
        if s.ready {
            sc.code = short(&s.id);
            sc.id = split_id(&s.id);
            sc.qr = true;
        } else {
            sc.waiting = starting;
        }
        return sc;
    }
    match (g.step, g.kind) {
        (0, _) => {
            sc.title = tr(ui, 1040, "Sync Your Saves");
            sc.body = format!("{}\n\n{}", tr(ui, 1041, "Syncthing keeps your saves the same on all your devices."),
                tr(ui, 1042, "What do you want to sync with?"));
            for (kind, t, tf, d, df) in [(CONSOLE, 1043, "Another Console", 1044, "Two nuubOS consoles share the same saves"),
                (COMPUTER, 1045, "Computer or Server", 1046, "A PC or NAS that stays on: where all your consoles meet"),
                (PHONE, 1047, "Android Phone", 1048, "With the Syncthing-Fork app")] {
                let mut r = nav(tr(ui, t, tf), String::new());
                r.detail = tr(ui, d, df).into();
                sc.items.push((format!("kind:{kind}"), r));
            }
        }
        (1, _) => {
            sc.title = tr(ui, 1049, "What to Sync");
            sc.body = format!("{} {}", tr(ui, 1050, "Choose what this profile shares."),
                tr(ui, 1093, "Saves match when the game file has the same name on every device."));
            for (kind, idx, fallback, didx, dfallback) in KINDS {
                let on = s.folders.iter().any(|f| f.0 == kind && f.1);
                sc.items.push((format!("folder:{kind}"), toggle(tr(ui, idx, fallback), on, kind_detail(ui, didx, dfallback))));
            }
            sc.items.push(next(391, "Continue"));
        }
        (2, CONSOLE) => {
            sc.title = tr(ui, 1051, "On the Other Console");
            let path = [tr(ui, 3, "Settings"), tr(ui, 702, "Gaming"), tr(ui, 669, "Syncthing"),
                tr(ui, 1028, "Add a Device"), tr(ui, 1043, "Another Console")].join(" → ");
            sc.body = format!("{}\n\n{}", numbered(&[tr(ui, 1052, "Turn it on and connect it to the same Wi-Fi network."),
                tr(ui, 1053, "Choose a profile with the same name."), tr_arg(ui, 1054, "Open {0}.", &path)]),
                tr_arg(ui, 1055, "This console: {0}", &s.name));
            if s.ready {
                sc.code = short(&s.id);
            }
            sc.items.push(next(391, "Continue"));
        }
        (3, CONSOLE) => {
            sc.title = tr(ui, 1056, "Choose the Other Console");
            sc.body = tr(ui, 1057, "Pick the code the other console shows.");
            request_rows(&mut sc);
            for n in &s.nearby {
                sc.items.push((format!("add:{}", n.id), row(device_label(ui, n), n.addr.clone(), String::new())));
            }
            sc.waiting = if s.ready { tr(ui, 1058, "Looking for consoles on your network…") } else { starting };
        }
        (4, CONSOLE) => {
            sc.title = tr(ui, 1060, "Confirm on the Other Console");
            sc.body = tr_arg(ui, 1061, "On the other console, accept the request from {0}.", &s.name);
            sc.waiting = tr(ui, 1062, "Waiting for the other device…");
            target_row(&mut sc);
        }
        (2, COMPUTER) => {
            sc.title = tr(ui, 1063, "Install Syncthing on the Computer");
            sc.body = numbered(&[tr(ui, 1064, "Download Syncthing from syncthing.net."),
                tr(ui, 1065, "Start it: its page opens in the browser."),
                tr(ui, 1066, "Keep it running: it is where all your consoles meet.")]);
            sc.items.push(next(391, "Continue"));
        }
        (3, COMPUTER) => {
            sc.title = tr(ui, 1067, "Add This Console");
            sc.body = tr_arg(ui, 1068, "In Syncthing on the computer choose Add Remote Device (code {0}), or type its ID:", &short(&s.id));
            if s.ready {
                sc.id = split_id(&s.id);
            }
            request_rows(&mut sc);
            sc.waiting = if s.ready { tr(ui, 1069, "Waiting for the request from the computer…") } else { starting };
        }
        (2, PHONE) => {
            sc.title = tr(ui, 1073, "Install Syncthing-Fork");
            sc.body = numbered(&[tr(ui, 1074, "On your Android phone install Syncthing-Fork and open it."),
                tr(ui, 1075, "Connect the phone to the same Wi-Fi network.")]);
            sc.items.push(next(391, "Continue"));
        }
        (3, PHONE) => {
            sc.title = tr(ui, 1076, "Scan This Code");
            sc.body = tr(ui, 1077, "In Syncthing-Fork open Devices, tap + and scan this code.");
            if s.ready {
                sc.qr = true;
                sc.code = short(&s.id);
            }
            request_rows(&mut sc);
            sc.waiting = if s.ready { tr(ui, 1078, "Waiting for the request from the phone…") } else { starting };
        }
        (4, kind) => {
            sc.title = tr(ui, 1070, "Accept the Folders");
            sc.body = if kind == PHONE {
                tr(ui, 1079, "The phone now asks to add the nuubOS folders: accept them.")
            } else {
                format!("{}\n\n{}", tr(ui, 1071, "The computer now asks to add the nuubOS folders: accept them."),
                    tr(ui, 1072, "For more consoles, repeat this guide on each one."))
            };
            target_row(&mut sc);
            sc.items.push(next(391, "Continue"));
        }
        _ => {
            sc.title = tr(ui, 1080, "You're All Set");
            let name = s.devices.iter().find(|d| d.id == g.target).map(|d| device_label(ui, d))
                .unwrap_or_else(|| g.target_name.clone());
            sc.body = tr_arg(ui, 1081, "{0} is connected.", &name);
            sc.items.push(("done".into(), row(tr(ui, 697, "Done"), String::new(), String::new())));
        }
    }
    sc
}

fn render_guide(ui: &HomeWindow) {
    let (sc, error, step, kind, busy, qr_stale) = STATE.with(|st| {
        let st = st.borrow();
        let sc = screen(ui, &st.snap, &st.guide);
        let stale = sc.qr && st.guide.qr_for != qr_text(&st);
        (sc, st.guide.error.clone(), st.guide.step, st.guide.kind, st.guide.busy, stale)
    });
    if qr_stale {
        let id = STATE.with(|st| qr_text(&st.borrow()));
        if let Some(img) = qr_image(&id) {
            ui.set_sync_guide_qr(img);
            STATE.with(|st| st.borrow_mut().guide.qr_for = id);
        }
    }
    let keys: Vec<String> = sc.items.iter().map(|i| i.0.clone()).collect();
    let rows: Vec<GenRow> = sc.items.into_iter().map(|(key, mut r)| {
        r.enabled = !key.is_empty() && !busy;
        r
    }).collect();
    let actionable: Vec<usize> = keys.iter().enumerate().filter(|(_, k)| !k.is_empty()).map(|(i, _)| i).collect();
    let mut index = ui.get_sync_guide_index();
    if actionable.is_empty() {
        index = -1;
    } else if index < 0 || !actionable.contains(&(index as usize)) {
        index = actionable.iter().copied().find(|&i| i as i32 >= index.max(0)).unwrap_or(actionable[actionable.len() - 1]) as i32;
    }
    ui.set_sync_guide_title(sc.title.into());
    ui.set_sync_guide_body(sc.body.into());
    ui.set_sync_guide_code(sc.code.into());
    ui.set_sync_guide_id(sc.id.into());
    ui.set_sync_guide_show_qr(sc.qr);
    ui.set_sync_guide_waiting(sc.waiting.into());
    ui.set_sync_guide_error(error.into());
    ui.set_sync_guide_items(ModelRc::from(Rc::new(VecModel::from(rows))));
    ui.set_sync_guide_index(index);
    ui.set_sync_guide_step(if kind == SHOW_ID || kind == EXTERNAL { -1 } else { step as i32 });
    STATE.with(|st| st.borrow_mut().guide.keys = keys);
}

pub fn guide_action(ui: &HomeWindow, action: &str) {
    let (keys, step, kind, busy) = STATE.with(|st| {
        let g = &st.borrow().guide;
        (g.keys.clone(), g.step, g.kind, g.busy)
    });
    match action {
        "menu_up" | "menu_down" => {
            let actionable: Vec<i32> = keys.iter().enumerate().filter(|(_, k)| !k.is_empty()).map(|(i, _)| i as i32).collect();
            if actionable.is_empty() {
                return;
            }
            let pos = actionable.iter().position(|&i| i == ui.get_sync_guide_index()).unwrap_or(0) as i32;
            let next = move_model_selection(pos, actionable.len() as i32, if action == "menu_up" { -1 } else { 1 });
            ui.set_sync_guide_index(actionable[next as usize]);
        }
        "menu_back" => {
            if kind == SHOW_ID || kind == EXTERNAL || step == 0 || step == 5 {
                close_guide(ui);
            } else {
                go(ui, step - 1);
            }
        }
        "menu_confirm" if !busy => {
            let key = keys.get(ui.get_sync_guide_index().max(0) as usize).cloned().unwrap_or_default();
            guide_confirm(ui, &key);
        }
        _ => {}
    }
}

fn guide_done(ui: &HomeWindow, reply: String) {
    if reply.starts_with("ERR") {
        STATE.with(|st| {
            let mut st = st.borrow_mut();
            st.guide.busy = false;
            st.guide.error = error_text(ui, &reply);
        });
        render_guide(ui);
    }
}

fn guide_confirm(ui: &HomeWindow, key: &str) {
    let (s, step) = STATE.with(|st| (st.borrow().snap.clone(), st.borrow().guide.step));
    match key {
        k if k.starts_with("kind:") => {
            let kind = k[5..].parse().unwrap_or(CONSOLE);
            STATE.with(|st| st.borrow_mut().guide.kind = kind);
            if !s.enabled {
                command(ui, "ENABLE".into(), guide_done);
            }
            go(ui, 1);
        }
        k if k.starts_with("folder:") => {
            let kind = &k[7..];
            let on = s.folders.iter().any(|f| f.0 == kind && f.1);
            command(ui, format!("FOLDER\t{kind}\t{}", u8::from(!on)), guide_done);
        }
        "next" => go(ui, step + 1),
        "done" => close_guide(ui),
        k if k.starts_with("add:") => {
            let id = k[4..].to_owned();
            let name = find(&s.pending, &id).map(|p| p.name).unwrap_or_default();
            STATE.with(|st| {
                let mut st = st.borrow_mut();
                st.guide.busy = true;
                st.guide.error.clear();
                st.guide.target = id.clone();
                st.guide.target_name = name.clone();
            });
            render_guide(ui);
            let line = if name.is_empty() { format!("ADD\t{id}") } else { format!("ADD\t{id}\t{name}") };
            command(ui, line, |ui, reply| {
                if reply.starts_with("ERR") {
                    guide_done(ui, reply);
                    return;
                }
                STATE.with(|st| st.borrow_mut().guide.busy = false);
                go(ui, 4);
                guide_follow(ui);
            });
        }
        _ => {}
    }
}
