#![allow(dead_code)]
// Renders the splash with the Rust port (splash_core.rs) as PPM.
#[path = "../../package/nuubos/nuubui-home/src/src/splash_core.rs"]
mod splash_core;
use std::io::Write;

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let (path, w, h, mode) = (&a[1], a[2].parse().unwrap(), a[3].parse().unwrap(), &a[4]);
    let p = a.get(5).map(|f| splash_core::parse_palette(&std::fs::read_to_string(f).unwrap())).unwrap_or_default();
    let px = splash_core::render(w, h, mode, &p);
    let mut out = format!("P6\n{w} {h}\n255\n").into_bytes();
    for v in px {
        out.extend_from_slice(&[(v >> 16) as u8, (v >> 8) as u8, v as u8]);
    }
    std::fs::File::create(path).unwrap().write_all(&out).unwrap();
}
