/* Minimal HTTP/1.x for the lighttpd proxy hop (one request per
 * connection). lighttpd terminates TLS, so only the local socket is read
 * here; limits still apply because the request content comes from the
 * network unchanged. */

use serde_json::Value;
use std::io::{self, BufRead, BufReader, Read, Write};
use std::os::unix::net::UnixStream;

const MAX_HEAD: usize = 32 * 1024;
const MAX_JSON_BODY: u64 = 256 * 1024;

pub struct Request {
    pub method: String,
    pub path: String,
    pub query: Vec<(String, String)>,
    headers: Vec<(String, String)>,
    reader: BufReader<UnixStream>,
    remaining: Option<u64>,
    chunked: bool,
    chunk_left: u64,
    chunk_done: bool,
}

impl Request {
    pub fn read(stream: UnixStream) -> io::Result<Request> {
        let mut reader = BufReader::new(stream);
        let mut line = String::new();
        let mut total = 0usize;
        let mut read_line = |reader: &mut BufReader<UnixStream>, line: &mut String| -> io::Result<()> {
            line.clear();
            let n = reader.by_ref().take((MAX_HEAD - total) as u64 + 1).read_line(line)?;
            total += n;
            if n == 0 || total > MAX_HEAD {
                return Err(io::Error::new(io::ErrorKind::InvalidData, "head"));
            }
            Ok(())
        };
        read_line(&mut reader, &mut line)?;
        let mut parts = line.trim_end().split(' ');
        let method = parts.next().unwrap_or("").to_owned();
        let target = parts.next().unwrap_or("/").to_owned();
        let mut headers = Vec::new();
        loop {
            read_line(&mut reader, &mut line)?;
            let l = line.trim_end_matches(['\r', '\n']);
            if l.is_empty() {
                break;
            }
            if let Some((k, v)) = l.split_once(':') {
                headers.push((k.trim().to_ascii_lowercase(), v.trim().to_owned()));
            }
        }
        let (path, qs) = target.split_once('?').unwrap_or((&target, ""));
        let query = parse_query(qs);
        let chunked = headers.iter().any(|(k, v)| k == "transfer-encoding" && v.to_ascii_lowercase().contains("chunked"));
        let remaining = if chunked {
            None
        } else {
            headers.iter().find(|(k, _)| k == "content-length").and_then(|(_, v)| v.parse().ok()).or(Some(0))
        };
        Ok(Request {
            method,
            path: percent_decode(path),
            query,
            headers,
            reader,
            remaining,
            chunked,
            chunk_left: 0,
            chunk_done: false,
        })
    }

    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers.iter().find(|(k, _)| k == name).map(|(_, v)| v.as_str())
    }

    pub fn q(&self, name: &str) -> &str {
        self.query.iter().find(|(k, _)| k == name).map(|(_, v)| v.as_str()).unwrap_or("")
    }

    pub fn content_length(&self) -> Option<u64> {
        if self.chunked { None } else { self.remaining }
    }

    /* The client address lighttpd added last to X-Forwarded-For. */
    pub fn client(&self) -> String {
        self.header("x-forwarded-for")
            .and_then(|v| v.rsplit(',').next())
            .map(|v| v.trim().to_owned())
            .unwrap_or_default()
    }

    pub fn cookie(&self, name: &str) -> Option<String> {
        let all = self.header("cookie")?;
        all.split(';').find_map(|c| {
            let (k, v) = c.trim().split_once('=')?;
            (k == name).then(|| v.to_owned())
        })
    }

    pub fn json(&mut self) -> io::Result<Value> {
        let mut body = Vec::new();
        self.by_ref().take(MAX_JSON_BODY + 1).read_to_end(&mut body)?;
        if body.len() as u64 > MAX_JSON_BODY {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "body"));
        }
        if body.is_empty() {
            return Ok(Value::Object(Default::default()));
        }
        serde_json::from_slice(&body).map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "json"))
    }

    pub fn stream(&mut self) -> &mut UnixStream {
        self.reader.get_mut()
    }

    fn next_chunk(&mut self) -> io::Result<()> {
        let mut line = String::new();
        if self.chunk_left == 0 && !self.chunk_done {
            self.reader.by_ref().take(64).read_line(&mut line)?;
            if line.trim().is_empty() {
                line.clear();
                self.reader.by_ref().take(64).read_line(&mut line)?;
            }
            let size = u64::from_str_radix(line.trim().split(';').next().unwrap_or(""), 16)
                .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "chunk"))?;
            if size == 0 {
                self.chunk_done = true;
            }
            self.chunk_left = size;
        }
        Ok(())
    }
}

impl Read for Request {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if self.chunked {
            self.next_chunk()?;
            if self.chunk_done {
                return Ok(0);
            }
            let want = buf.len().min(self.chunk_left as usize);
            let n = self.reader.read(&mut buf[..want])?;
            if n == 0 {
                return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "chunk"));
            }
            self.chunk_left -= n as u64;
            return Ok(n);
        }
        let left = self.remaining.unwrap_or(0);
        if left == 0 {
            return Ok(0);
        }
        let want = buf.len().min(left as usize);
        let n = self.reader.read(&mut buf[..want])?;
        if n == 0 {
            return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "body"));
        }
        self.remaining = Some(left - n as u64);
        Ok(n)
    }
}

pub fn parse_query(qs: &str) -> Vec<(String, String)> {
    qs.split('&')
        .filter(|p| !p.is_empty())
        .map(|p| {
            let (k, v) = p.split_once('=').unwrap_or((p, ""));
            (percent_decode(&k.replace('+', " ")), percent_decode(&v.replace('+', " ")))
        })
        .collect()
}

pub fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            if let Some(v) = std::str::from_utf8(&b[i + 1..i + 3]).ok().and_then(|h| u8::from_str_radix(h, 16).ok()) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

pub struct Response {
    pub status: u16,
    pub ctype: &'static str,
    pub body: Vec<u8>,
    pub headers: Vec<(String, String)>,
}

impl Response {
    pub fn json(status: u16, v: Value) -> Response {
        Response { status, ctype: "application/json; charset=utf-8", body: v.to_string().into_bytes(), headers: Vec::new() }
    }

    pub fn error(status: u16, code: &str) -> Response {
        Response::json(status, serde_json::json!({ "error": code }))
    }

    pub fn ok() -> Response {
        Response::json(200, serde_json::json!({ "ok": true }))
    }

    pub fn with(mut self, k: &str, v: String) -> Response {
        self.headers.push((k.to_owned(), v));
        self
    }
}

pub fn reason(status: u16) -> &'static str {
    match status {
        200 => "OK",
        400 => "Bad Request",
        401 => "Unauthorized",
        403 => "Forbidden",
        404 => "Not Found",
        405 => "Method Not Allowed",
        409 => "Conflict",
        413 => "Payload Too Large",
        429 => "Too Many Requests",
        500 => "Internal Server Error",
        503 => "Service Unavailable",
        507 => "Insufficient Storage",
        _ => "Status",
    }
}

pub fn head(w: &mut impl Write, status: u16, ctype: &str, len: Option<u64>, extra: &[(String, String)]) -> io::Result<()> {
    let mut h = format!("HTTP/1.1 {} {}\r\nContent-Type: {}\r\nCache-Control: no-store\r\nConnection: close\r\n", status, reason(status), ctype);
    if let Some(n) = len {
        h.push_str(&format!("Content-Length: {n}\r\n"));
    }
    for (k, v) in extra {
        if v.contains(['\r', '\n']) {
            continue;
        }
        h.push_str(&format!("{k}: {v}\r\n"));
    }
    h.push_str("\r\n");
    w.write_all(h.as_bytes())
}

pub fn send(w: &mut impl Write, r: Response) -> io::Result<()> {
    head(w, r.status, r.ctype, Some(r.body.len() as u64), &r.headers)?;
    w.write_all(&r.body)?;
    w.flush()
}
