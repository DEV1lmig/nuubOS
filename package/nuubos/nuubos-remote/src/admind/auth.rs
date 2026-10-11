/* Sign-in with the device credential (user root, the password shared by
 * SSH and SMB, EPIC-039) and the sessions it opens.
 *
 * Sessions live only in memory: a daemon restart (Web administration off
 * and on, reboot) signs everybody out. A session ends after IDLE_SECS
 * without requests, after MAX_SECS in any case, on sign-out, and as soon
 * as the device credential changes (Regenerate Password). Failed sign-ins
 * lock the client address out for an exponentially growing time. */

use std::collections::HashMap;
use std::fs;
use std::io::Read;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

pub const COOKIE: &str = "nuubos_session";
const IDLE_SECS: u64 = 4 * 3600;
const MAX_SECS: u64 = 7 * 24 * 3600;
const MAX_SESSIONS: usize = 16;
const FREE_FAILURES: u32 = 4;
const MAX_LOCK_SECS: u64 = 15 * 60;
pub const USER: &str = "root";

struct Session {
    created: u64,
    last: u64,
    credential: String,
}

struct Failures {
    count: u32,
    until: u64,
}

static SESSIONS: Mutex<Option<HashMap<String, Session>>> = Mutex::new(None);
static FAILURES: Mutex<Option<HashMap<String, Failures>>> = Mutex::new(None);

pub fn credential_path() -> String {
    format!("{}/state/remote/credential", crate::root())
}

pub fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

fn credential() -> String {
    fs::read_to_string(credential_path()).map(|s| s.trim().to_owned()).unwrap_or_default()
}

pub fn random_hex(bytes: usize) -> String {
    let mut buf = vec![0u8; bytes];
    let ok = fs::File::open("/dev/urandom").and_then(|mut f| f.read_exact(&mut buf)).is_ok();
    if !ok {
        return String::new();
    }
    buf.iter().map(|b| format!("{b:02x}")).collect()
}

fn same(a: &[u8], b: &[u8]) -> bool {
    /* Constant time for equal lengths; the length itself is not secret
     * (the credential format is public). */
    if a.len() != b.len() {
        return false;
    }
    a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

pub enum Login {
    Ok(String),
    Wrong,
    Locked(u64),
}

pub fn login(client: &str, user: &str, password: &str) -> Login {
    let t = now();
    let mut fails = FAILURES.lock().unwrap();
    let fails = fails.get_or_insert_with(HashMap::new);
    fails.retain(|_, f| f.until + MAX_LOCK_SECS > t);
    if let Some(f) = fails.get(client) {
        if f.until > t {
            return Login::Locked(f.until - t);
        }
    }
    let cred = credential();
    let good = !cred.is_empty() && same(user.as_bytes(), USER.as_bytes()) & same(password.trim().as_bytes(), cred.as_bytes());
    if !good {
        let f = fails.entry(client.to_owned()).or_insert(Failures { count: 0, until: 0 });
        f.count += 1;
        if f.count > FREE_FAILURES {
            let lock = (15u64 << (f.count - FREE_FAILURES - 1).min(6)).min(MAX_LOCK_SECS);
            f.until = t + lock;
        }
        return Login::Wrong;
    }
    fails.remove(client);
    let token = random_hex(32);
    if token.is_empty() {
        return Login::Wrong;
    }
    let mut s = SESSIONS.lock().unwrap();
    let s = s.get_or_insert_with(HashMap::new);
    expire(s, t, &cred);
    if s.len() >= MAX_SESSIONS {
        if let Some(oldest) = s.iter().min_by_key(|(_, v)| v.last).map(|(k, _)| k.clone()) {
            s.remove(&oldest);
        }
    }
    s.insert(token.clone(), Session { created: t, last: t, credential: cred });
    Login::Ok(token)
}

fn expire(s: &mut HashMap<String, Session>, t: u64, cred: &str) {
    s.retain(|_, v| t < v.last + IDLE_SECS && t < v.created + MAX_SECS && v.credential == cred);
}

/* A valid session token; refreshes its idle time. */
pub fn check(token: &str) -> bool {
    if token.len() != 64 {
        return false;
    }
    let t = now();
    let cred = credential();
    let mut s = SESSIONS.lock().unwrap();
    let s = s.get_or_insert_with(HashMap::new);
    expire(s, t, &cred);
    match s.get_mut(token) {
        Some(v) => {
            v.last = t;
            true
        }
        None => false,
    }
}

pub fn logout(token: &str) {
    if let Some(s) = SESSIONS.lock().unwrap().as_mut() {
        s.remove(token);
    }
}

pub fn cookie(token: &str) -> String {
    format!("{COOKIE}={token}; Path=/; HttpOnly; Secure; SameSite=Strict; Max-Age={MAX_SECS}")
}

pub fn clear_cookie() -> String {
    format!("{COOKIE}=; Path=/; HttpOnly; Secure; SameSite=Strict; Max-Age=0")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compare() {
        assert!(same(b"abcd-efgh-jkmn", b"abcd-efgh-jkmn"));
        assert!(!same(b"abcd-efgh-jkmn", b"abcd-efgh-jkmm"));
        assert!(!same(b"abc", b"abcd"));
    }
}
