/*
 * nuubos-admind — the nuubOS Web administration service (EPIC-038).
 *
 * lighttpd serves the web application over HTTPS and passes /api/ to this
 * daemon on a local socket. Every API request but sign-in needs a session
 * opened with the device credential (user root and the password shared
 * with SSH and SMB, EPIC-039). State-changing requests also need the
 * X-NuubOS header and a same-origin Origin (a cross-site page can send
 * neither), on top of the SameSite=Strict session cookie.
 *
 * The daemon owns no product state: it reads the services' snapshots and
 * sends them their commands (see api.rs), streams files in and out of the
 * user storage roots (files.rs) and relays change notifications to open
 * pages (events.rs). It runs only while Web administration is enabled
 * (nuubos-remotectl) and sleeps in accept() when nobody uses it.
 *
 *   nuubos-admind [--socket PATH] [--owner USER]
 */

mod api;
mod auth;
mod events;
mod files;
mod http;
mod svc;
mod thumbs;

use http::{Request, Response};
use serde_json::json;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::{UnixListener, UnixStream};
use std::sync::OnceLock;
use std::thread;
use std::time::Duration;

static ROOT: OnceLock<String> = OnceLock::new();

/* NUUBOS_ROOT prefixes the system paths for host tests. */
pub fn root() -> &'static str {
    ROOT.get_or_init(|| std::env::var("NUUBOS_ROOT").unwrap_or_default())
}

fn same_origin(req: &Request) -> bool {
    match (req.header("origin"), req.header("host")) {
        (None, _) => true,
        (Some(o), Some(h)) => o == format!("https://{h}"),
        _ => false,
    }
}

fn route(req: &mut Request) -> Option<Response> {
    let method = req.method.clone();
    let path = req.path.clone();
    let writes = method != "GET" && method != "HEAD";
    if writes && (req.header("x-nuubos") != Some("1") || !same_origin(req)) {
        return Some(Response::error(403, "origin"));
    }
    match (method.as_str(), path.as_str()) {
        /* Before sign-in the page only learns how to look. */
        ("GET", "/api/public") => {
            return Some(Response::json(200, json!({ "lang": api::language(), "mood": api::mood() })));
        }
        ("POST", "/api/login") => {
            let body = match req.json() {
                Ok(b) => b,
                Err(_) => return Some(Response::error(400, "body")),
            };
            let user = body["user"].as_str().unwrap_or("");
            let password = body["password"].as_str().unwrap_or("");
            return Some(match auth::login(&req.client(), user, password) {
                auth::Login::Ok(token) => Response::ok().with("Set-Cookie", auth::cookie(&token)),
                auth::Login::Wrong => {
                    thread::sleep(Duration::from_millis(400));
                    Response::error(401, "credentials")
                }
                auth::Login::Locked(secs) => Response::json(429, json!({ "error": "locked", "retry": secs })),
            });
        }
        _ => {}
    }
    let token = req.cookie(auth::COOKIE).unwrap_or_default();
    if !auth::check(&token) {
        return Some(Response::error(401, "session"));
    }
    match (method.as_str(), path.as_str()) {
        ("POST", "/api/logout") => {
            auth::logout(&token);
            Some(Response::ok().with("Set-Cookie", auth::clear_cookie()))
        }
        ("GET", "/api/session") => Some(Response::json(200, json!({ "user": auth::USER }))),
        ("GET", "/api/i18n") => {
            let lang = api::language();
            Some(Response::json(200, json!({ "lang": lang, "strings": api::catalog(&lang) })))
        }
        ("GET", "/api/events") => {
            events::serve(req.stream());
            None
        }
        ("GET", "/api/get") => {
            let query = req.query.clone();
            let q = move |k: &str| query.iter().find(|(n, _)| n == k).map(|(_, v)| v.clone()).unwrap_or_default();
            Some(match api::topic(req.q("t"), &q) {
                Ok(v) => Response::json(200, v),
                Err(e) => Response::error(if e == "topic" { 404 } else { 503 }, &e),
            })
        }
        ("POST", "/api/do") => {
            let body = match req.json() {
                Ok(b) => b,
                Err(_) => return Some(Response::error(400, "body")),
            };
            Some(match api::action(&body) {
                Ok(v) => Response::json(200, v),
                Err(e) => Response::error(if e == "op" { 404 } else { 409 }, &e),
            })
        }
        ("GET", "/api/img") => files::image(req),
        ("GET", "/api/download") => files::download(req),
        ("PUT", "/api/upload") => Some(upload(req)),
        _ => Some(Response::error(404, "path")),
    }
}

/* PUT /api/upload, then what the destination needs: a Library scan for
 * games, a BIOS check for BIOS files. */
fn upload(req: &mut Request) -> Response {
    let after = req.q("after").to_owned();
    match files::upload(req) {
        Err(r) => r,
        Ok(path) => match after.as_str() {
            "scan" => {
                let _ = svc::sock_ok("libraryd", "SCAN");
                Response::json(200, json!({ "ok": true, "path": path }))
            }
            "bios" => {
                events::changed("bios");
                Response::json(200, json!({ "ok": true, "path": path }))
            }
            _ => Response::json(200, json!({ "ok": true, "path": path })),
        },
    }
}

fn handle(stream: UnixStream) {
    stream.set_read_timeout(Some(Duration::from_secs(120))).ok();
    let Ok(mut req) = Request::read(stream) else { return };
    if let Some(r) = route(&mut req) {
        let _ = http::send(req.stream(), r);
    }
}

fn main() {
    let mut path = format!("{}/run/nuubos/admin.sock", root());
    let mut owner = String::new();
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--socket" => path = args.next().unwrap_or(path),
            "--owner" => owner = args.next().unwrap_or_default(),
            _ => {
                eprintln!("Usage: nuubos-admind [--socket PATH] [--owner USER]");
                std::process::exit(2);
            }
        }
    }
    let _ = std::fs::remove_file(&path);
    let listener = match UnixListener::bind(&path) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("nuubos-admind: {path}: {e}");
            std::process::exit(1);
        }
    };
    /* Only the web server may connect. */
    let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));
    if !owner.is_empty() {
        let _ = std::process::Command::new("chown").arg(&owner).arg(&path).status();
    }
    for conn in listener.incoming() {
        match conn {
            Ok(s) => {
                thread::spawn(move || handle(s));
            }
            Err(_) => thread::sleep(Duration::from_millis(50)),
        }
    }
}
