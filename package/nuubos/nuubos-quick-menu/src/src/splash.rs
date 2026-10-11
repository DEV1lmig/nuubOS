//! nuubOS lifecycle splash for Slint: the rasters of splash_core.rs (the
//! port of nuubos-splash-render.h, pixel-identical to the kernel splash and
//! labwc's first frame) as images, plus the track geometry the boot,
//! restart and power-off animations draw on (docs/dev/ui.md "Lifecycle
//! splash"). Identical copy in nuubui-home and nuubos-quick-menu (src/src/splash.rs).

pub use crate::splash_core::{load_palette, Palette};
use slint::{Image, Rgba8Pixel, SharedPixelBuffer};

/// Animation timing shared by nuubUI, the Quick Menu and nuubos-lightingd
/// (its Lifecycle shapes run for the same time).
pub const BOOT_FILL_MS: u64 = 700;
pub const BOOT_FADE_MS: u64 = 320;
pub const OUT_FADE_MS: u64 = 280;
pub const RESTART_SWEEP_MS: u64 = 900;
pub const RESTART_SWEEPS: u64 = 2;
pub const POWEROFF_DRAIN_MS: u64 = 650;

/// The raster for "boot" | "reboot" | "poweroff" | "full" at physical size.
pub fn image(mode: &str, width: u32, height: u32, palette: &Palette) -> Image {
    let (w, h) = (width.max(1), height.max(1));
    let px = crate::splash_core::render(w as i32, h as i32, mode, palette);
    let mut buffer = SharedPixelBuffer::<Rgba8Pixel>::new(w, h);
    for (dst, v) in buffer.make_mut_slice().iter_mut().zip(px) {
        *dst = Rgba8Pixel { r: (v >> 16) as u8, g: (v >> 8) as u8, b: v as u8, a: 255 };
    }
    Image::from_rgba8(buffer)
}

/// Track rectangle in physical pixels: (x, y, width, height).
pub fn track(width: u32, height: u32) -> (f32, f32, f32, f32) {
    let g = crate::splash_core::geometry(width.max(1) as i32, height.max(1) as i32);
    (g.track_x8 as f32 / 8.0, g.track_y8 as f32 / 8.0, g.track_w8 as f32 / 8.0, g.track_h8 as f32 / 8.0)
}

/// 0xRRGGBB as a Slint colour.
pub fn color(v: u32) -> slint::Color {
    slint::Color::from_rgb_u8((v >> 16) as u8, (v >> 8) as u8, v as u8)
}

/// One frame of a lifecycle animation, over the rasters:
/// `top` = "boot"/"reboot" raster, `base` = "poweroff" raster (power off
/// only), `fill` = accent pill on the track between `from` and `to`
/// (fractions of the track width).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Frame {
    pub top_alpha: f32,
    pub base_alpha: f32,
    pub fill_from: f32,
    pub fill_to: f32,
    pub fill_alpha: f32,
    pub done: bool,
}

/* M3 emphasized deceleration and a symmetric ease (no trigonometry). */
fn ease_out(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    1.0 - (1.0 - t) * (1.0 - t) * (1.0 - t)
}

fn ease_in_out(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Total length of an animation in milliseconds.
pub fn duration_ms(kind: &str) -> u64 {
    match kind {
        "boot" => BOOT_FILL_MS + BOOT_FADE_MS,
        "reboot" => OUT_FADE_MS + RESTART_SWEEP_MS * RESTART_SWEEPS,
        _ => OUT_FADE_MS + POWEROFF_DRAIN_MS,
    }
}

/// The frame of `kind` ("boot" | "reboot" | "poweroff") at `t` ms.
/// boot: the track fills from its centre, then the splash fades out over
/// nuubUI. reboot: the splash fades in, an accent segment sweeps the track
/// twice, the reboot raster stays. poweroff: the splash fades in with a
/// full track, which drains to its centre while the wordmark dims; the
/// power-off raster stays. nuubos-lightingd plays the same shapes on the
/// LEDs for the same times.
pub fn frame(kind: &str, t: u64) -> Frame {
    let f = |v: u64, d: u64| v as f32 / d as f32;
    let mut out = Frame { top_alpha: 1.0, base_alpha: 0.0, fill_from: 0.0, fill_to: 0.0, fill_alpha: 0.0, done: false };
    match kind {
        "boot" => {
            if t < BOOT_FILL_MS {
                let e = ease_out(f(t, BOOT_FILL_MS));
                out.fill_from = 0.5 - 0.5 * e;
                out.fill_to = 0.5 + 0.5 * e;
                out.fill_alpha = 1.0;
            } else {
                let a = 1.0 - ease_in_out(f(t - BOOT_FILL_MS, BOOT_FADE_MS));
                out.top_alpha = a;
                out.fill_to = 1.0;
                out.fill_alpha = a;
                out.done = t >= duration_ms("boot");
                if out.done {
                    out.top_alpha = 0.0;
                    out.fill_alpha = 0.0;
                }
            }
        }
        "reboot" => {
            if t < OUT_FADE_MS {
                out.top_alpha = ease_in_out(f(t, OUT_FADE_MS));
            } else if t < duration_ms("reboot") {
                /* One ease over both sweeps, like the LED laps
                 * (nuubos-lightingd SHAPE_RESTART): no stop between them. */
                let whole = ease_in_out(f(t - OUT_FADE_MS, RESTART_SWEEP_MS * RESTART_SWEEPS));
                let p = (whole * RESTART_SWEEPS as f32).fract();
                let seg = 0.32;
                let head = p * (1.0 + seg);
                out.fill_from = (head - seg).max(0.0);
                out.fill_to = head.min(1.0);
                out.fill_alpha = 1.0;
            } else {
                out.done = true;
            }
        }
        _ => {
            if t < OUT_FADE_MS {
                let a = ease_in_out(f(t, OUT_FADE_MS));
                out.top_alpha = a;
                out.fill_to = 1.0;
                out.fill_alpha = a;
            } else if t < duration_ms("poweroff") {
                let k = ease_in_out(f(t - OUT_FADE_MS, POWEROFF_DRAIN_MS));
                out.base_alpha = 1.0;
                out.top_alpha = 1.0 - k;
                out.fill_from = 0.5 * k;
                out.fill_to = 1.0 - 0.5 * k;
                out.fill_alpha = 1.0;
            } else {
                out.base_alpha = 1.0;
                out.top_alpha = 0.0;
                out.done = true;
            }
        }
    }
    out
}
