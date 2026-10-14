/* Files over the web: uploads, downloads and images. The roots are the
 * ones the Files application uses (nuubos-filesctl): user storage,
 * removable media and mounted network shares; hidden names, the upload
 * staging folder and lost+found are refused. Images additionally come from
 * the system's own artwork (covers, system icons, avatars). Paths are
 * resolved (realpath) before the check, so a link cannot leave a root. */

use crate::http::{self, Request, Response};
use std::fs;
use std::io::{self, Read, Write};
use std::os::unix::fs::MetadataExt;
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};

fn roots() -> Vec<String> {
    let r = crate::root();
    vec![format!("{r}/userdata"), format!("{r}/media"), format!("{r}/run/nuubos/shares")]
}

fn image_roots() -> Vec<String> {
    let r = crate::root();
    vec![
        format!("{r}/userdata"),
        format!("{r}/media"),
        format!("{r}/usr/share/nuubos/systems"),
        format!("{r}/usr/share/nuubos/avatars"),
        format!("{r}/usr/share/nuubos/applications"),
        format!("{r}/usr/share/nuubos/themes"),
        format!("{r}/run/nuubos/userdata"),
    ]
}

fn hidden_part(p: &Path, root: &str) -> bool {
    let rel = p.strip_prefix(root).unwrap_or(p);
    rel.components().any(|c| {
        let s = c.as_os_str().to_string_lossy();
        s.starts_with('.') || s == "lost+found"
    })
}

/* A resolved path inside one of the roots, or None. */
fn inside(path: &str, roots: &[String], allow_hidden: bool) -> Option<PathBuf> {
    if path.is_empty() || !path.starts_with('/') || path.contains('\0') {
        return None;
    }
    let real = fs::canonicalize(path).ok()?;
    for root in roots {
        let Ok(rr) = fs::canonicalize(root) else { continue };
        if real.starts_with(&rr) {
            if !allow_hidden && hidden_part(&real, &rr.to_string_lossy()) {
                return None;
            }
            return Some(real);
        }
    }
    None
}

/* A file name for a new entry: printable, no separators, not hidden. */
pub fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 255
        && !name.starts_with('.')
        && !name.contains('/')
        && !name.contains('\\')
        && !name.chars().any(|c| c.is_control())
}

fn mime(path: &Path) -> &'static str {
    match path.extension().map(|e| e.to_string_lossy().to_ascii_lowercase()).as_deref() {
        Some("png") => "image/png",
        Some("jpg") | Some("jpeg") => "image/jpeg",
        Some("webp") => "image/webp",
        Some("gif") => "image/gif",
        Some("svg") => "image/svg+xml",
        Some("txt") | Some("log") | Some("cfg") | Some("conf") | Some("ini") => "text/plain; charset=utf-8",
        Some("json") => "application/json",
        Some("pdf") => "application/pdf",
        Some("mp3") => "audio/mpeg",
        Some("ogg") => "audio/ogg",
        Some("mp4") => "video/mp4",
        _ => "application/octet-stream",
    }
}

fn is_image(path: &Path) -> bool {
    mime(path).starts_with("image/")
}

fn send_file(w: &mut UnixStream, path: &Path, attachment: bool) -> io::Result<()> {
    send_with(w, path, attachment, Vec::new())
}

fn send_with(w: &mut UnixStream, path: &Path, attachment: bool, mut extra: Vec<(String, String)>) -> io::Result<()> {
    let mut f = fs::File::open(path)?;
    let len = f.metadata()?.len();
    if attachment {
        let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let ascii: String = name.chars().map(|c| if c.is_ascii_graphic() && c != '"' && c != '\\' || c == ' ' { c } else { '_' }).collect();
        extra.push(("Content-Disposition".into(), format!("attachment; filename=\"{ascii}\"; filename*=UTF-8''{}", encode(&name))));
    }
    let ctype = mime(path);
    /* Never let a downloaded file run as a page of this site. */
    let ctype = if attachment && (ctype.starts_with("text/") || ctype.contains("svg")) { "application/octet-stream" } else { ctype };
    http::head(w, 200, ctype, Some(len), &extra)?;
    io::copy(&mut f, w)?;
    w.flush()
}

fn encode(s: &str) -> String {
    s.bytes()
        .map(|b| if b.is_ascii_alphanumeric() || b"-._~".contains(&b) { (b as char).to_string() } else { format!("%{b:02X}") })
        .collect()
}

/* GET /api/img?p=ABS[&w=WIDTH]: the image, or a scaled copy of it no
 * wider than WIDTH (thumbs.rs). Pages show the same images again at every
 * refresh: they may keep them, and ask again with the validator. */
pub fn image(req: &mut Request) -> Option<Response> {
    let Some(p) = inside(req.q("p"), &image_roots(), false).filter(|p| p.is_file() && is_image(p)) else {
        return Some(Response::error(404, "path"));
    };
    let w = crate::thumbs::width(req.q("w"));
    let tag = fs::metadata(&p).ok().map(|m| format!("\"{:x}-{:x}-{}\"", m.len(), m.mtime(), w.unwrap_or(0)));
    if tag.is_some() && req.header("if-none-match") == tag.as_deref() {
        let _ = http::head(req.stream(), 304, "image/*", None, &[("Cache-Control".into(), CACHE_IMAGES.into()), ("ETag".into(), tag.unwrap_or_default())]);
        return None;
    }
    let file = w.and_then(|w| crate::thumbs::get(&p, w)).unwrap_or(p);
    let extra = vec![("Cache-Control".into(), CACHE_IMAGES.into()), ("ETag".into(), tag.unwrap_or_default())];
    match send_with(req.stream(), &file, false, extra) {
        Ok(()) => None,
        Err(_) => Some(Response::error(404, "path")),
    }
}

const CACHE_IMAGES: &str = "private, max-age=300";

/* GET /api/download?p=ABS */
pub fn download(req: &mut Request) -> Option<Response> {
    let Some(p) = inside(req.q("p"), &roots(), false).filter(|p| p.is_file()) else {
        return Some(Response::error(404, "path"));
    };
    match send_file(req.stream(), &p, true) {
        Ok(()) => None,
        Err(_) => Some(Response::error(404, "path")),
    }
}

/* PUT /api/upload?dir=ABS&name=NAME[&replace=1]: the body is the file.
 * Written as a hidden .part file and renamed when complete, so a broken
 * upload never leaves a half file a scan could pick up. Existing files are
 * kept unless replace=1 (BIOS files are replaced on purpose). Returns the
 * final path. */
pub fn upload(req: &mut Request) -> Result<String, Response> {
    let dir = match inside(req.q("dir"), &roots(), false).filter(|d| d.is_dir()) {
        Some(d) => d,
        None if req.q("mkdirs") == "1" => make_dirs(req.q("dir")).ok_or_else(|| Response::error(400, "dir"))?,
        None => return Err(Response::error(400, "dir")),
    };
    let name = req.q("name").to_owned();
    if !valid_name(&name) {
        return Err(Response::error(400, "name"));
    }
    let dest = dir.join(&name);
    let replace = req.q("replace") == "1";
    if dest.exists() && (!replace || !dest.is_file()) {
        return Err(Response::error(409, "exists"));
    }
    let expected = req.content_length();
    if let Some(n) = expected {
        if free_bytes(&dir).map(|f| f < n + 1024 * 1024).unwrap_or(false) {
            return Err(Response::error(507, "space"));
        }
    }
    let part = dir.join(format!(".{}.part.{}", name, std::process::id()));
    let result = (|| -> io::Result<u64> {
        let mut f = fs::OpenOptions::new().write(true).create_new(true).open(&part)?;
        let mut buf = vec![0u8; 256 * 1024];
        let mut total = 0u64;
        loop {
            let n = req.read(&mut buf)?;
            if n == 0 {
                break;
            }
            f.write_all(&buf[..n])?;
            total += n as u64;
        }
        f.sync_all()?;
        Ok(total)
    })();
    match result {
        Ok(total) if expected.map(|n| n == total).unwrap_or(true) => {
            if fs::rename(&part, &dest).is_err() {
                let _ = fs::remove_file(&part);
                return Err(Response::error(507, "storage"));
            }
            Ok(dest.to_string_lossy().into_owned())
        }
        Ok(_) => {
            let _ = fs::remove_file(&part);
            Err(Response::error(400, "incomplete"))
        }
        Err(e) => {
            let _ = fs::remove_file(&part);
            let space = e.raw_os_error() == Some(28);
            Err(Response::error(if space { 507 } else { 400 }, if space { "space" } else { "incomplete" }))
        }
    }
}

/* Missing folders below an existing allowed one (a BIOS in a system
 * subfolder), each a valid name. */
fn make_dirs(dir: &str) -> Option<PathBuf> {
    let mut missing = Vec::new();
    let mut cur = Path::new(dir);
    while !cur.exists() {
        missing.push(cur.file_name()?.to_str()?.to_owned());
        cur = cur.parent()?;
    }
    let mut d = inside(cur.to_str()?, &roots(), false).filter(|d| d.is_dir())?;
    if missing.is_empty() || missing.len() > 4 || missing.iter().any(|m| !valid_name(m)) {
        return None;
    }
    for m in missing.iter().rev() {
        d = d.join(m);
        fs::create_dir(&d).ok()?;
    }
    Some(d)
}

fn free_bytes(dir: &Path) -> Option<u64> {
    let out = std::process::Command::new("df").arg("-k").arg(dir).output().ok()?;
    let text = String::from_utf8_lossy(&out.stdout);
    let line = text.lines().nth(1)?;
    let avail: u64 = line.split_whitespace().nth(3)?.parse().ok()?;
    Some(avail * 1024)
}

/* A path argument for an action on user files (filesctl checks again). */
pub fn user_path(p: &str) -> Option<String> {
    inside(p, &roots(), false).map(|p| p.to_string_lossy().into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names() {
        assert!(valid_name("Super Mario World (USA).sfc"));
        assert!(!valid_name(".hidden"));
        assert!(!valid_name("a/b"));
        assert!(!valid_name("a\nb"));
        assert!(!valid_name(""));
    }

    #[test]
    fn roots_and_links() {
        let t = std::env::temp_dir().join(format!("admind-test-{}", std::process::id()));
        let ud = t.join("userdata");
        fs::create_dir_all(ud.join("roms/snes")).unwrap();
        fs::create_dir_all(ud.join(".nuubos-upload")).unwrap();
        fs::create_dir_all(t.join("etc")).unwrap();
        fs::write(t.join("etc/shadow"), "x").unwrap();
        std::os::unix::fs::symlink(t.join("etc"), ud.join("link")).unwrap();
        let roots = vec![ud.to_string_lossy().into_owned()];
        assert!(inside(&ud.join("roms/snes").to_string_lossy(), &roots, false).is_some());
        assert!(inside(&ud.join("link/shadow").to_string_lossy(), &roots, false).is_none());
        assert!(inside(&ud.join("roms/../../etc/shadow").to_string_lossy(), &roots, false).is_none());
        assert!(inside(&ud.join(".nuubos-upload").to_string_lossy(), &roots, false).is_none());
        assert!(inside("relative", &roots, false).is_none());
        fs::remove_dir_all(&t).unwrap();
    }
}
