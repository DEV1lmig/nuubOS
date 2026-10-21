/* Live updates for open pages (Server-Sent Events).
 *
 * While at least one page listens, one watcher per service follows its
 * change notifications (SUBSCRIBE on the product sockets, the typed D-Bus
 * signals) and every change is announced as {"t": topic}; the page then
 * reads the topic again. A watcher stops at the first notification that
 * finds nobody listening, so with no page open the daemon holds no
 * subscriptions and sleeps. Pages get a comment every HEARTBEAT to notice
 * a lost connection; nothing else is timed. */

use crate::svc;
use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, RecvTimeoutError, Sender};
use std::sync::Mutex;
use std::thread;
use std::time::Duration;

const HEARTBEAT: Duration = Duration::from_secs(25);

static CLIENTS: Mutex<Vec<Sender<String>>> = Mutex::new(Vec::new());

pub fn listeners() -> usize {
    CLIENTS.lock().unwrap().len()
}

pub fn publish(v: Value) {
    let msg = v.to_string();
    CLIENTS.lock().unwrap().retain(|c| c.send(msg.clone()).is_ok());
}

pub fn changed(topic: &str) {
    publish(json!({ "t": topic }));
}

/* Socket services: (topic, socket name, line that ends a snapshot).
 * statusd only says "changed". Audio has few subscriber slots (the Quick
 * Menu OSD needs one): its D-Bus signal is used instead. */
const SOCKETS: [(&str, &str, &str); 10] = [
    ("status", "statusd", "changed"),
    ("system", "systemd", "end=1"),
    ("users", "usersd", "end=1"),
    ("localization", "localizationd", "available="),
    ("regional", "regionald", "local_time="),
    ("library", "libraryd", "end=1"),
    ("jobs", "jobd", "end=1"),
    ("emu", "emud", "end=1"),
    ("sync", "syncd", "end=1"),
    ("tailscale", "tsd", "end=1"),
];
const BUSES: [(&str, &str); 6] = [
    ("audio", "Audio"),
    ("wifi", "Wifi"),
    ("bluetooth", "Bluetooth"),
    ("controllers", "Controllers"),
    ("lighting", "Lighting"),
    ("rumble", "Rumble"),
];
static RUNNING: [AtomicBool; 16] = [const { AtomicBool::new(false) }; 16];


fn start_watchers() {
    for (i, (topic, name, last)) in SOCKETS.iter().enumerate() {
        if !RUNNING[i].swap(true, Ordering::SeqCst) {
            thread::spawn(move || {
                watch_socket(topic, name, last);
                RUNNING[i].store(false, Ordering::SeqCst);
            });
        }
    }
    for (j, (topic, svc)) in BUSES.iter().enumerate() {
        let i = SOCKETS.len() + j;
        if !RUNNING[i].swap(true, Ordering::SeqCst) {
            thread::spawn(move || {
                watch_bus(topic, svc);
                RUNNING[i].store(false, Ordering::SeqCst);
            });
        }
    }
}

/* Every snapshot is one change (regionald pushes one a minute, for the
 * clock). */
fn watch_socket(topic: &str, name: &str, last: &str) {
    let Ok(mut s) = UnixStream::connect(svc::socket(name)) else { return };
    if s.write_all(b"SUBSCRIBE\n").is_err() {
        return;
    }
    let mut first = true;
    let mut reader = BufReader::new(s);
    let mut line = String::new();
    loop {
        line.clear();
        match reader.read_line(&mut line) {
            Ok(0) | Err(_) => return,
            Ok(_) => {}
        }
        if !line.starts_with(last) {
            continue;
        }
        if first {
            /* The snapshot answering SUBSCRIBE itself. */
            first = false;
            if name != "statusd" {
                continue;
            }
        }
        if listeners() == 0 {
            return;
        }
        changed(topic);
    }
}

fn watch_bus(topic: &str, name: &str) {
    let Ok(p) = svc::proxy(name) else { return };
    let Ok(signals) = p.receive_all_signals() else { return };
    for msg in signals {
        if listeners() == 0 {
            return;
        }
        let member = msg.header().member().map(|m| m.as_str().to_owned()).unwrap_or_default();
        match member.as_str() {
            /* Raw controller input and remap capture are not for the web. */
            "InputEvent" | "RemapCaptured" | "RemapCancelled" | "TesterExitRequested" => continue,
            "OperationFailed" => {
                let (op, message): (String, String) = msg.body().deserialize().unwrap_or_default();
                publish(json!({ "t": topic, "failed": op, "message": message }));
            }
            "ScanStateChanged" | "DiscoveryStateChanged" | "WpsStateChanged" => {
                let (active,): (bool,) = msg.body().deserialize().unwrap_or_default();
                publish(json!({ "t": topic, "member": member, "active": active }));
            }
            _ => changed(topic),
        }
    }
}

/* The event stream of one page; returns when the page goes away. */
pub fn serve(stream: &mut UnixStream) {
    let (tx, rx) = channel::<String>();
    CLIENTS.lock().unwrap().push(tx);
    start_watchers();
    let ok = crate::http::head(stream, 200, "text/event-stream", None, &[("X-Accel-Buffering".into(), "no".into())])
        .and_then(|_| stream.write_all(b": nuubOS\n\n"))
        .and_then(|_| stream.flush());
    if ok.is_err() {
        return;
    }
    loop {
        let r = match rx.recv_timeout(HEARTBEAT) {
            Ok(msg) => stream.write_all(format!("data: {msg}\n\n").as_bytes()),
            Err(RecvTimeoutError::Timeout) => stream.write_all(b": .\n\n"),
            Err(RecvTimeoutError::Disconnected) => return,
        };
        if r.and_then(|_| stream.flush()).is_err() {
            return;
        }
    }
}
