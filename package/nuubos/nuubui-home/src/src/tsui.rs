/*
 * Tailscale in nuubUI (user request 2026-10-21): Settings → Connectivity,
 * SERVICES row Tailscale and its page (settings-view 73, generic rows).
 *
 * nuubos-tsd owns the capability: this module subscribes to its snapshot
 * (/run/nuubos/tsd.sock, SUBSCRIBE), shows it and sends its commands. No
 * polling: download progress, the sign-in address and the connection
 * arrive as new snapshots.
 *
 * Rows: not downloaded → Download (progress, A cancels); downloaded →
 * Tailscale on/off (status), Sign In (needs login: the full-screen QR code
 * of Tailscale's sign-in address, syncui::show_screen, closed by itself once
 * connected) or the console's tailnet name and address + Sign Out, Update /
 * Check for Updates, Remove (second press).
 */

use crate::gameui::{apply_rows, arm_or, armed, armed_detail, nav, notice, row, toggle};
use crate::syncui::{close_external, external_open, show_screen, ExternalScreen};
use crate::{tr, tr_arg, GenRow, HomeWindow};
use slint::ComponentHandle;
use std::cell::RefCell;
use std::cmp::Ordering as CmpOrdering;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::thread;
use std::time::Duration;

const SOCKET: &str = "/run/nuubos/tsd.sock";
pub const VIEW: i32 = 73;

#[derive(Clone, Default)]
struct Snapshot {
    installed: bool,
    version: String,
    latest: String,
    job: String,
    progress: i32,
    error: String,
    enabled: bool,
    ready: bool,
    failed: bool,
    state: String,
    auth_url: String,
    name: String,
    ip: String,
    tailnet: String,
}

#[derive(Default)]
struct State {
    snap: Snapshot,
    /* The sign-in screen was asked for on this console. */
    signing_in: bool,
    /* The last job error already shown. */
    shown_error: String,
}

thread_local! {
    static STATE: RefCell<State> = RefCell::new(State::default());
}

fn parse(text: &str) -> Snapshot {
    let mut s = Snapshot { progress: -1, ..Default::default() };
    for line in text.lines() {
        let Some((k, v)) = line.split_once('=') else { continue };
        let v = v.to_owned();
        match k {
            "installed" => s.installed = v == "1",
            "version" => s.version = v,
            "latest" => s.latest = v,
            "job" => s.job = v,
            "progress" => s.progress = v.parse().unwrap_or(-1),
            "error" => s.error = v,
            "enabled" => s.enabled = v == "1",
            "ready" => s.ready = v == "1",
            "failed" => s.failed = v == "1",
            "state" => s.state = v,
            "auth_url" => s.auth_url = v,
            "name" => s.name = v,
            "ip" => s.ip = v,
            "tailnet" => s.tailnet = v,
            _ => {}
        }
    }
    s
}

/* "1.104.2" < "1.110.0", numerically per component. */
fn newer(latest: &str, version: &str) -> bool {
    let parse = |v: &str| v.split('.').map(|p| p.parse::<u64>().unwrap_or(0)).collect::<Vec<_>>();
    !latest.is_empty() && parse(latest).cmp(&parse(version)) == CmpOrdering::Greater
}

/* ---------------------------------------------------------------- */
/* Service                                                          */
/* ---------------------------------------------------------------- */

/* Follows the snapshot for the whole session (nuubos-tsd only writes when
 * something changed). Reconnects with a growing pause if tsd is absent. */
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
    /* LOGIN, LOGOUT and REMOVE wait for tailscaled. */
    s.set_read_timeout(Some(Duration::from_secs(45))).ok();
    if s.write_all(line.as_bytes()).and_then(|_| s.write_all(b"\n")).is_err() {
        return "ERR service".into();
    }
    s.shutdown(std::net::Shutdown::Write).ok();
    let mut reply = String::new();
    let _ = BufReader::new(s).read_line(&mut reply);
    if reply.is_empty() { "ERR service".into() } else { reply.trim().to_owned() }
}

/* Errors show as the page notice; the change itself comes back as a
 * snapshot. */
fn command(ui: &HomeWindow, line: &'static str) {
    let weak = ui.as_weak();
    thread::spawn(move || {
        let reply = request(line);
        if reply.starts_with("ERR") {
            let _ = slint::invoke_from_event_loop(move || {
                if let Some(ui) = weak.upgrade() {
                    if line == "LOGIN" {
                        STATE.with(|st| st.borrow_mut().signing_in = false);
                        close_external(&ui);
                    }
                    notice(&ui, tr(&ui, 93, "Failed"));
                }
            });
        }
    });
}

fn apply(ui: &HomeWindow, snap: Snapshot) {
    let (new_error, signing_in) = STATE.with(|st| {
        let mut st = st.borrow_mut();
        /* A download failure is told once. */
        let new_error = (!snap.error.is_empty() && snap.error != st.shown_error).then(|| snap.error.clone());
        st.shown_error = snap.error.clone();
        if snap.state == "Running" || !snap.enabled {
            st.signing_in = false;
        }
        st.snap = snap;
        (new_error, st.signing_in)
    });
    if signing_in {
        show_sign_in(ui);
    } else if external_open() && STATE.with(|st| st.borrow().snap.state == "Running") {
        close_external(ui);
    }
    if ui.get_settings_open() && ui.get_settings_view() == VIEW {
        if let Some(e) = new_error {
            notice(ui, error_text(ui, &e));
        }
        render(ui);
    } else if ui.get_settings_open() && (ui.get_settings_view() == 1 || ui.get_settings_view() == 0) {
        crate::gameui::render(ui);
    }
}

fn error_text(ui: &HomeWindow, error: &str) -> String {
    match error {
        "network" => tr(ui, 494, "Check the network connection"),
        "space" => tr(ui, 493, "Not enough free space"),
        _ => tr(ui, 1333, "Tailscale could not be downloaded"),
    }
}

/* ---------------------------------------------------------------- */
/* Status                                                           */
/* ---------------------------------------------------------------- */

fn status_line(ui: &HomeWindow, s: &Snapshot) -> String {
    if s.failed {
        tr(ui, 93, "Failed")
    } else if !s.installed || !s.enabled {
        tr(ui, 1329, "Reach this console from anywhere through your tailnet")
    } else if !s.ready {
        tr(ui, 1025, "Starting…")
    } else {
        match s.state.as_str() {
            "Running" => tr(ui, 198, "Connected"),
            "NeedsLogin" => tr(ui, 1331, "Sign in to your tailnet"),
            "NeedsMachineAuth" => tr(ui, 1334, "Waiting for approval in your tailnet"),
            "Stopped" => tr(ui, 159, "Not connected"),
            _ => tr(ui, 1025, "Starting…"),
        }
    }
}

/* The Tailscale row of Connectivity → SERVICES: (value, detail). */
pub fn summary(ui: &HomeWindow) -> (String, String) {
    STATE.with(|st| {
        let s = &st.borrow().snap;
        let value = if !s.installed { String::new() } else if s.enabled { tr(ui, 222, "On") } else { tr(ui, 54, "Off") };
        let detail = if s.enabled && s.state == "Running" && !s.ip.is_empty() { s.ip.clone() } else { status_line(ui, s) };
        (value, detail)
    })
}

/* ---------------------------------------------------------------- */
/* Page                                                             */
/* ---------------------------------------------------------------- */

/* A row whose operation is running: a bar in place of its detail line. */
fn progress_row(title: String, percent: i32) -> GenRow {
    GenRow {
        progress_shown: true,
        progress: percent.clamp(0, 100) as f32 / 100.0,
        ..row(title, format!("{}%", percent.clamp(0, 100)), String::new())
    }
}

pub fn render(ui: &HomeWindow) {
    let s = STATE.with(|st| st.borrow().snap.clone());
    let armed = armed();
    let mut rows: Vec<(String, GenRow, bool)> = Vec::new();
    if s.job == "install" {
        rows.push(("ts-cancel".into(), progress_row(tr_arg(ui, 1221, "Downloading {0}%", &s.progress.max(0).to_string()),
            s.progress.max(0)), true));
    } else if s.job == "remove" {
        rows.push(("info".into(), row(tr(ui, 644, "Remove"), String::new(), tr(ui, 1025, "Starting…")), false));
    } else if !s.installed {
        let mut r = nav(tr(ui, 653, "Download"), String::new());
        r.detail = tr(ui, 1330, "Tailscale's own app, about 25 MB, downloaded from tailscale.com").into();
        rows.push(("ts-download".into(), r, true));
    } else {
        rows.push(("ts-enabled".into(), toggle(tr(ui, 1328, "Tailscale"), s.enabled, status_line(ui, &s)), true));
        if s.enabled && s.ready {
            match s.state.as_str() {
                "Running" => {
                    let mut me = row(if s.name.is_empty() { tr(ui, 177, "Address") } else { s.name.clone() }, s.ip.clone(),
                        tr(ui, 1336, "SSH, File Sharing and Web Administration, when on, answer at this address too"));
                    me.enabled = false;
                    rows.push(("info".into(), me, false));
                    rows.push(("ts-logout".into(), row(tr(ui, 567, "Sign Out"), String::new(),
                        armed_detail(&armed, "ts-logout", tr(ui, 191, "Press again to confirm"), s.tailnet.clone())), true));
                }
                "NeedsLogin" | "NeedsMachineAuth" => {
                    let mut r = nav(tr(ui, 587, "Sign In"), String::new());
                    r.detail = status_line(ui, &s).into();
                    rows.push(("ts-login".into(), r, true));
                }
                _ => {}
            }
        }
        let update = if newer(&s.latest, &s.version) {
            row(tr_arg(ui, 486, "Update to {0}", &s.latest), String::new(), String::new())
        } else if s.job == "check" {
            row(tr(ui, 487, "Check for Updates"), String::new(), tr(ui, 1025, "Starting…"))
        } else {
            row(tr(ui, 487, "Check for Updates"), String::new(),
                if s.latest == s.version { tr(ui, 489, "Up to date") } else { String::new() })
        };
        let mut update = update;
        update.value = format!("{} {}", tr(ui, 485, "Version"), s.version).into();
        rows.push(("ts-update".into(), update, true));
        rows.push(("ts-remove".into(), row(tr(ui, 644, "Remove"), String::new(),
            armed_detail(&armed, "ts-remove", tr(ui, 191, "Press again to confirm"),
                tr(ui, 1335, "Removes Tailscale and takes this console out of your tailnet"))), true));
    }
    apply_rows(ui, rows, tr(ui, 1328, "Tailscale").to_uppercase());
}

fn show_sign_in(ui: &HomeWindow) {
    let url = STATE.with(|st| st.borrow().snap.auth_url.clone());
    show_screen(ui, ExternalScreen {
        title: tr(ui, 587, "Sign In"),
        body: tr(ui, 1332, "Scan the code with your phone, or open the address on another device, and sign in to Tailscale."),
        qr_text: url,
        waiting: tr(ui, 1025, "Starting…"),
    });
}

pub fn page_confirm(ui: &HomeWindow, key: &str) {
    let s = STATE.with(|st| st.borrow().snap.clone());
    match key {
        "ts-download" => command(ui, "INSTALL"),
        "ts-cancel" => command(ui, "CANCEL"),
        "ts-enabled" => command(ui, if s.enabled { "DISABLE" } else { "ENABLE" }),
        "ts-login" => {
            STATE.with(|st| st.borrow_mut().signing_in = true);
            show_sign_in(ui);
            command(ui, "LOGIN");
        }
        "ts-logout" => {
            if arm_or(ui, "ts-logout") {
                command(ui, "LOGOUT");
            }
        }
        "ts-update" => command(ui, if newer(&s.latest, &s.version) { "INSTALL" } else { "CHECK" }),
        "ts-remove" => {
            if arm_or(ui, "ts-remove") {
                command(ui, "REMOVE");
            }
        }
        _ => {}
    }
}
