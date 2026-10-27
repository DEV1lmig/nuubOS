/*
 * Terminal application of nuubUI (RC4, user decision 2026-10-27). The tile
 * asks nuubos-termd to open root's login shell in foot; there is no page.
 * While the terminal runs it covers nuubUI and the controller drives it
 * through nuubos-inputd (Home ignores input, Home Music stops like for a
 * game); the Quick Menu has the TERMINAL section and the keyboard.
 */

use crate::library::show_notice;
use crate::{on_game_session, tr, HomeWindow};
use slint::ComponentHandle;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::Duration;

const TERM_SOCKET: &str = "/run/nuubos/termd.sock";

/* The terminal runs: it covers nuubUI. */
pub static RUNNING: AtomicBool = AtomicBool::new(false);

fn request(line: &str) -> String {
    let Ok(mut stream) = UnixStream::connect(TERM_SOCKET) else { return String::new() };
    let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
    let _ = stream.write_all(line.as_bytes());
    let _ = stream.write_all(b"\n");
    let mut reply = String::new();
    let _ = BufReader::new(stream).read_line(&mut reply);
    reply
}

pub fn open(ui: &HomeWindow) {
    let weak = ui.as_weak();
    thread::spawn(move || {
        let reply = request("OPEN");
        if !reply.starts_with("OK") {
            eprintln!("home: terminal OPEN failed={}", reply.trim());
            let _ = slint::invoke_from_event_loop(move || {
                if let Some(ui) = weak.upgrade() {
                    show_notice(&ui, tr(&ui, 1387, "The terminal could not be opened"));
                }
            });
        }
    });
}

/* Follows nuubos-termd: Home input and Home Music follow the terminal
 * like a game. */
pub fn start_listener(ui: &HomeWindow) {
    let weak = ui.as_weak();
    thread::spawn(move || loop {
        if let Ok(mut stream) = UnixStream::connect(TERM_SOCKET) {
            if stream.write_all(b"SUBSCRIBE\n").is_ok() {
                let mut running = false;
                for line in BufReader::new(stream).lines() {
                    let Ok(line) = line else { break };
                    if let Some(v) = line.strip_prefix("state=") {
                        running = v != "idle";
                    } else if line == "end=1" {
                        if RUNNING.swap(running, Ordering::SeqCst) == running {
                            continue;
                        }
                        let weak = weak.clone();
                        let _ = slint::invoke_from_event_loop(move || {
                            if let Some(ui) = weak.upgrade() {
                                on_game_session(&ui, running);
                            }
                        });
                    }
                }
            }
        }
        if RUNNING.swap(false, Ordering::SeqCst) {
            let weak = weak.clone();
            let _ = slint::invoke_from_event_loop(move || {
                if let Some(ui) = weak.upgrade() {
                    on_game_session(&ui, false);
                }
            });
        }
        thread::sleep(Duration::from_millis(500));
    });
}
