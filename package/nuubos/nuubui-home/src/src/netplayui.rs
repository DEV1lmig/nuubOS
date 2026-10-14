/*
 * Netplay (Home application "Netplay"): RetroArch netplay games hosted on
 * the local network, to join from Home without RetroArch's menu.
 *
 * nuubos-emud owns netplay: NETPLAY_HOSTS is its LAN discovery (RetroArch's
 * protocol, so a RetroArch on a PC is found too), matched to this library
 * (libraryd MATCH) and to the host's emulator; joining is a LAUNCH with
 * "connect:<address>:<port>:<core>". Hosting is the Quick Menu's GAME →
 * Netplay → Host Game over a running game. The page is the application
 * page (AppPage, apppage.rs look) at settings-view 72: the games found
 * (entries), then Search Again; it searches when it opens.
 */

use crate::{
    move_model_selection, navigate_settings_view, play_ui_sound, tr, tr_arg, write_ui_context, AppEntry,
    DetailAction, HomeWindow,
};
use slint::{ComponentHandle, ModelRc, SharedString, VecModel};
use std::cell::RefCell;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::Duration;

pub const VIEW: i32 = 72;

/* The Netplay page owns Home input. */
pub static ACTIVE: AtomicBool = AtomicBool::new(false);

#[derive(Clone, Default)]
struct Host {
    address: String,
    port: String,
    nick: String,
    emulator: String,
    content: String,
    game: String,
    core: String,
    password: bool,
    title: String,
    system_name: String,
}

impl Host {
    fn joinable(&self) -> bool {
        !self.game.is_empty() && !self.core.is_empty() && !self.password
    }
}

#[derive(Default)]
struct State {
    hosts: Vec<Host>,
    searching: bool,
    searched: bool,
}

thread_local! {
    static STATE: RefCell<State> = RefCell::new(State::default());
}

fn request(socket: &str, line: &str, timeout: u64) -> String {
    let Ok(mut stream) = UnixStream::connect(socket) else { return String::new() };
    let _ = stream.set_read_timeout(Some(Duration::from_secs(timeout)));
    if stream.write_all(format!("{line}\n").as_bytes()).is_err() {
        return String::new();
    }
    let mut reply = String::new();
    for line in BufReader::new(stream).lines().map_while(Result::ok) {
        let done = line == "end=1" || line.starts_with("ERR") || line.starts_with("OK");
        reply.push_str(&line);
        reply.push('\n');
        if done {
            break;
        }
    }
    reply
}

/* emud's discovery, then each matched game's title from libraryd. */
fn discover() -> Vec<Host> {
    let reply = request("/run/nuubos/emud.sock", "NETPLAY_HOSTS", 6);
    reply
        .lines()
        .filter_map(|l| l.strip_prefix("host="))
        .filter_map(|v| {
            let f: Vec<&str> = v.split('\t').collect();
            if f.len() < 9 {
                return None;
            }
            let mut h = Host {
                address: f[0].into(),
                port: f[1].into(),
                nick: f[2].into(),
                emulator: f[3].into(),
                content: f[4].into(),
                game: f[5].into(),
                core: f[7].into(),
                password: f[8] == "1",
                ..Default::default()
            };
            if !h.game.is_empty() {
                let details = request("/run/nuubos/libraryd.sock", &format!("DETAILS\t{}", h.game), 3);
                for line in details.lines() {
                    if let Some(g) = line.strip_prefix("game=") {
                        h.title = g.split('\t').nth(2).unwrap_or("").to_owned();
                    } else if let Some(n) = line.strip_prefix("system_name=") {
                        h.system_name = n.to_owned();
                    }
                }
            }
            if h.title.is_empty() {
                /* Not in this library: the host's file name. */
                h.title = h.content.rsplit_once('.').map(|(s, _)| s).unwrap_or(&h.content).to_owned();
            }
            Some(h)
        })
        .collect()
}

fn search(ui: &HomeWindow) {
    if STATE.with(|st| std::mem::replace(&mut st.borrow_mut().searching, true)) {
        return;
    }
    ui.set_gen_notice("".into());
    refresh(ui);
    let weak = ui.as_weak();
    thread::spawn(move || {
        let hosts = discover();
        let _ = slint::invoke_from_event_loop(move || {
            STATE.with(|st| {
                let mut st = st.borrow_mut();
                st.hosts = hosts;
                st.searching = false;
                st.searched = true;
            });
            if let Some(ui) = weak.upgrade() {
                if ACTIVE.load(Ordering::SeqCst) {
                    let count = row_count();
                    ui.set_gen_index(ui.get_gen_index().clamp(0, (count - 1).max(0)));
                    refresh(&ui);
                }
            }
        });
    });
}

fn row_count() -> i32 {
    STATE.with(|st| st.borrow().hosts.len() as i32) + 1
}

/* The page from the state (AppPage). */
pub fn refresh(ui: &HomeWindow) {
    if !ACTIVE.load(Ordering::SeqCst) {
        return;
    }
    let (hosts, searching, searched) = STATE.with(|st| {
        let st = st.borrow();
        (st.hosts.clone(), st.searching, st.searched)
    });
    let entries: Vec<AppEntry> = hosts
        .iter()
        .map(|h| {
            let state = if h.game.is_empty() {
                tr(ui, 1266, "Not in your library")
            } else if h.core.is_empty() {
                tr_arg(ui, 1267, "Needs the emulator {0}", &h.emulator)
            } else if h.password {
                tr(ui, 1274, "Needs a password")
            } else {
                tr(ui, 1269, "Join")
            };
            let hosted = tr_arg(ui, 1268, "Hosted by {0}", &h.nick);
            let detail = if h.system_name.is_empty() { hosted.clone() } else { format!("{hosted} • {}", h.system_name) };
            AppEntry {
                title: h.title.clone().into(),
                detail: detail.clone().into(),
                value: state.clone().into(),
                icon: 11,
                good: h.joinable(),
                caption: format!("{detail} • {}:{}", h.address, h.port).into(),
            }
        })
        .collect();
    let mut again = DetailAction {
        icon: 17,
        label: tr(ui, 1263, "Search Again").into(),
        primary: entries.is_empty(),
        on: false,
        enabled: !searching,
        armed: false,
        busy: searching,
    };
    if searching {
        again.label = tr(ui, 1264, "Searching for netplay games…").into();
    }
    let description = if searching && entries.is_empty() {
        tr(ui, 1264, "Searching for netplay games…")
    } else if searched && entries.is_empty() {
        tr(ui, 1265, "No games found. On the other console, start a game and choose Host Game in its Quick Menu.")
    } else {
        tr(ui, 1262, "Play together with consoles on the same network running nuubOS or RetroArch.")
    };
    ui.set_app_page_kicker(tr(ui, 1239, "Netplay").to_uppercase().into());
    ui.set_app_page_title(tr(ui, 1239, "Netplay").into());
    ui.set_app_page_meta("".into());
    ui.set_app_page_description(description.into());
    ui.set_app_page_chips(ModelRc::default());
    ui.set_app_page_entries(ModelRc::from(Rc::new(VecModel::from(entries))));
    ui.set_app_page_actions(ModelRc::from(Rc::new(VecModel::from(vec![again]))));
    ui.set_app_page_captions(ModelRc::from(Rc::new(VecModel::from(vec![SharedString::new()]))));
    ui.set_app_page_pin("".into());
    ui.set_app_page_progress(-1.0);
    ui.set_app_page_progress_label("".into());
}

pub fn open(ui: &HomeWindow) {
    ACTIVE.store(true, Ordering::SeqCst);
    ui.set_netplay_active(true);
    ui.set_gen_index(0);
    ui.set_gen_notice("".into());
    navigate_settings_view(ui, VIEW);
    ui.set_settings_open(true);
    write_ui_context("netplay");
    refresh(ui);
    search(ui);
}

/* Back to Home (also when the user picker takes over). */
pub fn leave(ui: &HomeWindow) {
    ACTIVE.store(false, Ordering::SeqCst);
    navigate_settings_view(ui, 0);
    ui.set_settings_selected_index(0);
    ui.set_settings_open(false);
    ui.set_netplay_active(false);
    write_ui_context("home");
}

fn join(ui: &HomeWindow, h: &Host) {
    if !h.joinable() {
        play_ui_sound("error");
        ui.set_gen_notice(tr(ui, 1270, "The game could not be joined").into());
        return;
    }
    let mode = format!("new\tconnect:{}:{}:{}", h.address, h.port, h.core);
    let game = h.game.clone();
    leave(ui);
    crate::library::launch_with_mode(ui, &game, true, &mode);
}

pub fn handle_action(ui: &HomeWindow, action: &str) {
    let count = row_count();
    let index = ui.get_gen_index();
    match action {
        "menu_up" | "menu_down" | "menu_left" | "menu_right" => {
            play_ui_sound(action);
            let back = action == "menu_up" || action == "menu_left";
            ui.set_gen_index(move_model_selection(index, count, if back { -1 } else { 1 }));
            ui.set_gen_notice("".into());
        }
        "menu_back" => {
            play_ui_sound(action);
            leave(ui);
        }
        "menu_confirm" => {
            let host = STATE.with(|st| st.borrow().hosts.get(index.max(0) as usize).cloned());
            match host {
                Some(h) => {
                    if h.joinable() {
                        play_ui_sound(action);
                    }
                    join(ui, &h);
                }
                None => {
                    play_ui_sound(action);
                    search(ui);
                }
            }
        }
        _ => {}
    }
}
