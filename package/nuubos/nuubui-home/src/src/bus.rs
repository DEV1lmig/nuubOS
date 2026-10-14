//! One system bus connection for the whole process (battery audit
//! 2026-10-14): every `Connection::system()` used to open its own socket,
//! authenticate and start its own executor thread, ~20 of them kept alive
//! by the signal listeners. Clones share the connection; a cheap local
//! GetId checks it before reuse (the bus policy denies Peer.Ping), so a
//! restarted dbus-daemon gets a new one.

use std::sync::Mutex;

static SYSTEM: Mutex<Option<zbus::blocking::Connection>> = Mutex::new(None);

fn alive(connection: &zbus::blocking::Connection) -> bool {
    connection
        .call_method(
            Some("org.freedesktop.DBus"),
            "/org/freedesktop/DBus",
            Some("org.freedesktop.DBus"),
            "GetId",
            &(),
        )
        .is_ok()
}

/// The shared system bus connection, opened on first use.
pub fn system() -> zbus::Result<zbus::blocking::Connection> {
    let mut cached = SYSTEM.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    if let Some(connection) = cached.as_ref() {
        if alive(connection) {
            return Ok(connection.clone());
        }
    }
    let connection = zbus::blocking::Connection::system()?;
    *cached = Some(connection.clone());
    Ok(connection)
}
