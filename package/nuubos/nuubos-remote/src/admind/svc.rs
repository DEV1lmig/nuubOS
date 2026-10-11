/* Access to the nuubOS product services: line protocols on their sockets,
 * their command-line tools and the typed D-Bus services. Replies are
 * passed on as generic JSON; the services stay the only policy. */

use serde_json::{Map, Value};
use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
use std::process::{Command, Stdio};
use std::sync::OnceLock;
use std::time::Duration;

pub fn run_dir() -> String {
    format!("{}/run/nuubos", crate::root())
}

pub fn socket(name: &str) -> String {
    format!("{}/{name}.sock", run_dir())
}

/* One command on a service socket, the whole reply. */
pub fn sock_timeout(name: &str, cmd: &str, timeout: Duration) -> Result<String, String> {
    let mut s = UnixStream::connect(socket(name)).map_err(|_| "service".to_owned())?;
    s.set_read_timeout(Some(timeout)).ok();
    s.set_write_timeout(Some(Duration::from_secs(2))).ok();
    s.write_all(cmd.as_bytes()).and_then(|_| s.write_all(b"\n")).map_err(|_| "service".to_owned())?;
    s.shutdown(std::net::Shutdown::Write).ok();
    let mut out = String::new();
    s.read_to_string(&mut out).map_err(|_| "timeout".to_owned())?;
    Ok(out)
}

pub fn sock(name: &str, cmd: &str) -> Result<String, String> {
    sock_timeout(name, cmd, Duration::from_secs(4))
}

/* A command whose reply is OK / ERR <reason>. */
pub fn sock_ok(name: &str, cmd: &str) -> Result<(), String> {
    reply_ok(&sock(name, cmd)?)
}

pub fn reply_ok(out: &str) -> Result<(), String> {
    let first = out.lines().next().unwrap_or("").trim();
    if let Some(r) = first.strip_prefix("ERR") {
        let r = r.trim();
        return Err(if r.is_empty() { "failed".into() } else { r.to_owned() });
    }
    Ok(())
}

/* A product tool; stdin carries secrets (never the command line). */
pub fn tool(bin: &str, args: &[&str], stdin: Option<&str>) -> Result<String, String> {
    let mut c = Command::new(bin);
    c.args(args).stdout(Stdio::piped()).stderr(Stdio::null());
    c.stdin(if stdin.is_some() { Stdio::piped() } else { Stdio::null() });
    let mut child = c.spawn().map_err(|_| "tool".to_owned())?;
    if let (Some(text), Some(mut w)) = (stdin, child.stdin.take()) {
        let _ = w.write_all(text.as_bytes());
    }
    let out = child.wait_with_output().map_err(|_| "tool".to_owned())?;
    let text = String::from_utf8_lossy(&out.stdout).into_owned();
    if out.status.success() {
        Ok(text)
    } else {
        let first = text.lines().find(|l| l.starts_with("ERR")).unwrap_or("").trim_start_matches("ERR").trim().to_owned();
        Err(if first.is_empty() { "failed".into() } else { first })
    }
}

pub fn bin(name: &str) -> String {
    for dir in ["/usr/bin", "/usr/sbin"] {
        let p = format!("{dir}/{name}");
        if std::path::Path::new(&p).exists() {
            return p;
        }
    }
    format!("/usr/bin/{name}")
}

pub fn tool_ok(name: &str, args: &[&str]) -> Result<String, String> {
    tool(&bin(name), args, None)
}

/* key=value lines as JSON: tab-separated values become field lists and a
 * repeated key collects its values in order (callers treat a single value
 * and a list alike). Space-separated key=value pairs on one line (rgbctl)
 * are not split. */
pub fn kv(text: &str) -> Value {
    let mut map = Map::new();
    let mut lists = std::collections::HashSet::new();
    for line in text.lines() {
        let Some((k, v)) = line.split_once('=') else { continue };
        if k.is_empty() || k == "end" {
            continue;
        }
        let rows = v.contains('\t');
        let val = if rows {
            Value::Array(v.split('\t').map(|f| Value::String(f.to_owned())).collect())
        } else {
            Value::String(v.to_owned())
        };
        match map.get_mut(k) {
            Some(Value::Array(list)) if lists.contains(k) => list.push(val),
            Some(old) => {
                let prev = old.take();
                *old = Value::Array(vec![prev, val]);
                lists.insert(k.to_owned());
            }
            None if rows => {
                map.insert(k.to_owned(), Value::Array(vec![val]));
                lists.insert(k.to_owned());
            }
            None => {
                map.insert(k.to_owned(), val);
            }
        }
    }
    Value::Object(map)
}

pub fn active_user() -> String {
    std::fs::read_to_string(format!("{}/user/active", run_dir())).map(|s| s.trim().to_owned()).unwrap_or_default()
}

/* ------------------------------------------------------------- D-Bus */

static BUS: OnceLock<Option<zbus::blocking::Connection>> = OnceLock::new();

pub fn bus() -> Result<&'static zbus::blocking::Connection, String> {
    BUS.get_or_init(|| zbus::blocking::Connection::system().ok()).as_ref().ok_or_else(|| "bus".to_owned())
}

pub fn proxy(svc: &str) -> Result<zbus::blocking::Proxy<'static>, String> {
    let c = bus()?;
    zbus::blocking::Proxy::new(c, format!("org.nuubOS.{svc}"), format!("/org/nuubOS/{svc}"), format!("org.nuubOS.{svc}1"))
        .map_err(|_| "bus".to_owned())
}

pub fn dbus_err(e: zbus::Error) -> String {
    match e {
        zbus::Error::MethodError(_, Some(msg), _) => msg,
        zbus::Error::MethodError(name, None, _) => name.as_str().rsplit('.').next().unwrap_or("failed").to_owned(),
        _ => "service".to_owned(),
    }
}

pub fn call<B, R>(svc: &str, method: &str, body: &B) -> Result<R, String>
where
    B: serde::ser::Serialize + zbus::zvariant::DynamicType,
    R: for<'d> zbus::zvariant::DynamicDeserialize<'d>,
{
    proxy(svc)?.call(method, body).map_err(dbus_err)
}

pub fn call0(svc: &str, method: &str, body: &(impl serde::ser::Serialize + zbus::zvariant::DynamicType)) -> Result<(), String> {
    let p = proxy(svc)?;
    p.call_method(method, body).map(|_| ()).map_err(dbus_err)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kv_scalars_rows_and_repeats() {
        let v = kv("a=1\nuser=x\tBaf\t1\nuser=y\tEli\t0\nzone=Europe/Rome\nzone=Europe/London\nend=1\n");
        assert_eq!(v["a"], "1");
        assert_eq!(v["user"].as_array().unwrap().len(), 2);
        assert_eq!(v["user"][1][1], "Eli");
        assert_eq!(v["zone"][1], "Europe/London");
        let one = kv("game=id\tsnes\tMario\n");
        assert_eq!(one["game"][0][2], "Mario");
        let three = kv("z=a\nz=b\nz=c\n");
        assert_eq!(three["z"].as_array().unwrap().len(), 3);
        assert!(v.get("end").is_none());
    }
}
