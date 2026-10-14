/* Small copies of images for the web pages. Covers are scraped at full
 * size (often 0.5-1 MB PNGs) and a library page shows dozens of them: the
 * page asks for a width (w=) and gets a scaled copy, made once and kept in
 * CACHE under a name derived from the image's path, size and mtime (a new
 * cover gets a new name; the old copy is simply never asked for again).
 * Opaque images become JPEG, images with transparency stay PNG. At most
 * three images are decoded at a time: a grid asks for dozens at once and a
 * decoded cover takes a few MB on a 1 GB console. */

use image::codecs::jpeg::JpegEncoder;
use image::imageops::FilterType;
use image::{DynamicImage, ImageFormat, ImageReader, Limits};
use std::collections::hash_map::DefaultHasher;
use std::fs;
use std::hash::{Hash, Hasher};
use std::io::Cursor;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::sync::{Condvar, Mutex};

const CACHE: &str = "/userdata/cache/web-admin/thumbs";
/* The widths the pages use (cards, cover, avatars); anything else is
 * rounded up to one of them so the cache stays small. */
const WIDTHS: [u32; 4] = [96, 192, 320, 480];
const DECODERS: usize = 3;

static SLOTS: Mutex<usize> = Mutex::new(0);
static FREED: Condvar = Condvar::new();

struct Slot;

impl Slot {
    fn take() -> Slot {
        let mut n = SLOTS.lock().unwrap_or_else(|e| e.into_inner());
        while *n >= DECODERS {
            n = FREED.wait(n).unwrap_or_else(|e| e.into_inner());
        }
        *n += 1;
        Slot
    }
}

impl Drop for Slot {
    fn drop(&mut self) {
        let mut n = SLOTS.lock().unwrap_or_else(|e| e.into_inner());
        *n -= 1;
        FREED.notify_one();
    }
}

pub fn width(asked: &str) -> Option<u32> {
    let w: u32 = asked.parse().ok()?;
    WIDTHS.iter().copied().find(|&x| x >= w).or(Some(WIDTHS[WIDTHS.len() - 1]))
}

/* The scaled copy of SRC (a raster image inside the image roots) at width
 * W, made if needed; None when SRC cannot be decoded (the caller then
 * sends the original). */
pub fn get(src: &Path, w: u32) -> Option<PathBuf> {
    let ext = src.extension()?.to_string_lossy().to_ascii_lowercase();
    if !matches!(ext.as_str(), "png" | "jpg" | "jpeg") {
        return None;
    }
    let meta = fs::metadata(src).ok()?;
    let mut h = DefaultHasher::new();
    (src, meta.len(), meta.mtime(), meta.mtime_nsec(), w).hash(&mut h);
    let base = format!("{}/{}/{:016x}", crate::root(), CACHE, h.finish());
    for e in ["jpg", "png"] {
        let p = PathBuf::from(format!("{base}.{e}"));
        if p.is_file() {
            return Some(p);
        }
    }
    let _slot = Slot::take();
    /* Another request may have made it while this one waited. */
    for e in ["jpg", "png"] {
        let p = PathBuf::from(format!("{base}.{e}"));
        if p.is_file() {
            return Some(p);
        }
    }
    let img = decode(src)?;
    if img.width() <= w {
        return None;
    }
    let small = img.resize(w, u32::MAX, FilterType::Triangle);
    drop(img);
    let opaque = !small.color().has_alpha() || small.to_rgba8().pixels().all(|p| p.0[3] == 255);
    let (out, ext) = if opaque { (encode_jpeg(&small)?, "jpg") } else { (encode_png(&small)?, "png") };
    let dest = PathBuf::from(format!("{base}.{ext}"));
    fs::create_dir_all(dest.parent()?).ok()?;
    let part = PathBuf::from(format!("{base}.{}.part", std::process::id()));
    fs::write(&part, out).ok()?;
    fs::rename(&part, &dest).ok()?;
    Some(dest)
}

fn decode(src: &Path) -> Option<DynamicImage> {
    let mut r = ImageReader::open(src).ok()?.with_guessed_format().ok()?;
    let mut limits = Limits::default();
    limits.max_image_width = Some(8192);
    limits.max_image_height = Some(8192);
    limits.max_alloc = Some(96 * 1024 * 1024);
    r.limits(limits);
    r.decode().ok()
}

fn encode_jpeg(img: &DynamicImage) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    JpegEncoder::new_with_quality(&mut out, 84).encode_image(&img.to_rgb8()).ok()?;
    Some(out)
}

fn encode_png(img: &DynamicImage) -> Option<Vec<u8>> {
    let mut out = Cursor::new(Vec::new());
    img.to_rgba8().write_to(&mut out, ImageFormat::Png).ok()?;
    Some(out.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn widths_round_up() {
        assert_eq!(width("100"), Some(192));
        assert_eq!(width("96"), Some(96));
        assert_eq!(width("4000"), Some(480));
        assert_eq!(width("x"), None);
    }
}
