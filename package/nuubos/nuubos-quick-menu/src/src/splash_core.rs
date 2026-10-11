//! nuubOS lifecycle splash raster (dot-matrix logo + track): a line-by-line port of
//! package/nuubos/nuubos-splash/src/nuubos-splash-render.h. It MUST give
//! the same pixels (tools/splash-check compares the two); integer
//! arithmetic only. Identical copy in nuubos-quick-menu/src/src.

pub const PALETTE_PATH: &str = "/etc/nuubos/splash.palette";

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Palette {
    pub background: u32,
    pub text: u32,
    pub accent: u32,
    pub surface: u32,
}

impl Default for Palette {
    fn default() -> Self {
        Palette { background: 0x0d0e10, text: 0xf5f6f8, accent: 0x3a86ff, surface: 0x24272d }
    }
}

fn hex(s: &str, fallback: u32) -> u32 {
    let b = s.as_bytes();
    if b.len() < 7 || b[0] != b'#' {
        return fallback;
    }
    let mut v = 0u32;
    for &c in &b[1..7] {
        let d = match c {
            b'0'..=b'9' => c - b'0',
            b'a'..=b'f' => c - b'a' + 10,
            b'A'..=b'F' => c - b'A' + 10,
            _ => return fallback,
        };
        v = v * 16 + d as u32;
    }
    v
}

pub fn parse_palette(text: &str) -> Palette {
    let mut p = Palette::default();
    for line in text.lines() {
        let Some((k, v)) = line.split_once('=') else { continue };
        match k {
            "background" => p.background = hex(v, p.background),
            "text" => p.text = hex(v, p.text),
            "accent" => p.accent = hex(v, p.accent),
            "surface" => p.surface = hex(v, p.surface),
            _ => {}
        }
    }
    p
}

pub fn load_palette() -> Palette {
    std::fs::read_to_string(PALETTE_PATH).map(|t| parse_palette(&t)).unwrap_or_default()
}

#[derive(Clone, Copy, Debug)]
pub struct Geometry {
    pub cell: i64,
    pub cap: i64,
    pub word_x8: i64,
    pub word_y8: i64,
    pub word_w8: i64,
    pub track_x8: i64,
    pub track_y8: i64,
    pub track_w8: i64,
    pub track_h8: i64,
    pub motto_pitch: i64,
    pub motto_x8: i64,
    pub motto_y8: i64,
}

/// The motto "Small System. Big Adventures." (5x7 letters), boot only.
pub const MOTTO: [&str; 7] = [
    " ####              ##    ##          ####              #                            ####    #                ###      #                    #                                 ",
    "#                   #     #         #                  #                            #   #        ####       #   #     #                    #                                 ",
    "#     ## ##  ###    #     #         #     #   #  #### ###    ###  ## ##             #   #  ##   #   #       #   #  #### #   #  ###  ####  ###   #   # # ##   ###   ####      ",
    " ###  # # #     #   #     #          ###  #   # #      #    #   # # # #             ####    #   #   #       ##### #   # #   # #   # #   #  #    #   # ##  # #   # #          ",
    "    # # # #  ####   #     #             #  ####  ###   #    ##### # # #             #   #   #    ####       #   # #   # #   # ##### #   #  #    #   # #     #####  ###       ",
    "    # #   # #   #   #     #             #     #     #  #  # #     #   #  ##         #   #   #       #       #   # #   #  # #  #     #   #  #  # #   # #     #         #  ##  ",
    "####  #   #  ####  ###   ###        ####   ###  ####    ##   #### #   #  ##         ####   ###   ###        #   #  ####   #    #### #   #   ##   #### #      #### ####   ##  ",
];
pub const MOTTO_COLS: i64 = 173;

/// The logo dot table (copies: nuubos-splash-render.h, home.slint
/// NuubLogo, www/app.js LOGO): 7 rows x 37 columns, '#' = a dot; columns
/// from LOGO_ACCENT_COL ("OS") take the accent.
pub const LOGO: [&str; 7] = [
    "                  #       ####   ####",
    "                  #      ##  ## #    ",
    "####  #   # #   # ####   #    # #    ",
    "#   # #   # #   # #   #  #    #  ### ",
    "#   # #   # #   # #   #  #    #     #",
    "#   # #   # #   # #   #  ##  ##     #",
    "#   #  ####  #### ####    ####  #### ",
];
pub const LOGO_COLS: i64 = 37;
pub const LOGO_ROWS: i64 = 7;
pub const LOGO_ACCENT_COL: i64 = 25;

pub fn geometry(width: i32, height: i32) -> Geometry {
    let (width, height) = (width as i64, height as i64);
    let mut cell = height * 11 / 100 / LOGO_ROWS;
    if cell > width * 80 / 100 / LOGO_COLS {
        cell = width * 80 / 100 / LOGO_COLS;
    }
    if cell < 3 {
        cell = 3;
    }
    let cap = cell * LOGO_ROWS;
    let word_w = cell * LOGO_COLS;
    let word_x8 = ((width - word_w) / 2) * 8;
    let top = height * 43 / 100 - cap / 2;
    let mut th = cell * 3 / 4;
    if th < 3 {
        th = 3;
    }
    let mut motto_pitch = cell / 3;
    if motto_pitch * MOTTO_COLS > width * 90 / 100 {
        motto_pitch = width * 90 / 100 / MOTTO_COLS;
    }
    if motto_pitch < 1 {
        motto_pitch = 1;
    }
    Geometry {
        motto_pitch,
        motto_x8: ((width - motto_pitch * MOTTO_COLS) / 2) * 8,
        motto_y8: (top + cap + cell * 3 + th + cell * 2) * 8,
        cell,
        cap,
        word_x8,
        word_y8: top * 8,
        word_w8: word_w * 8,
        track_x8: word_x8,
        track_y8: (top + cap + cell * 3) * 8,
        track_w8: word_w * 8,
        track_h8: th * 8,
    }
}

/// 0 outside, 1 "nuub", 2 "OS" (1/8 px sample).
fn word_hit(g: &Geometry, sx: i64, sy: i64) -> u8 {
    let (x, y) = (sx - g.word_x8, sy - g.word_y8);
    let cell8 = g.cell * 8;
    let gap = if g.cell / 6 > 1 { g.cell / 6 } else { 1 };
    let dot8 = (g.cell - gap) * 8;
    let r = dot8 / 4;
    if x < 0 || y < 0 || x >= g.word_w8 || y >= cell8 * LOGO_ROWS {
        return 0;
    }
    let (col, row) = (x / cell8, y / cell8);
    if LOGO[row as usize].as_bytes()[col as usize] != b'#' {
        return 0;
    }
    let (dx, dy) = (x - col * cell8, y - row * cell8);
    if dx >= dot8 || dy >= dot8 {
        return 0;
    }
    let cx = if dx < r { r } else if dx > dot8 - r { dot8 - r } else { dx };
    let cy = if dy < r { r } else if dy > dot8 - r { dot8 - r } else { dy };
    if (dx - cx) * (dx - cx) + (dy - cy) * (dy - cy) > r * r {
        return 0;
    }
    if col >= LOGO_ACCENT_COL { 2 } else { 1 }
}

fn motto_hit(g: &Geometry, sx: i64, sy: i64) -> bool {
    let p8 = g.motto_pitch * 8;
    let dot8 = (g.motto_pitch - g.motto_pitch / 4) * 8;
    let r = dot8 / 4;
    let (x, y) = (sx - g.motto_x8, sy - g.motto_y8);
    if x < 0 || y < 0 || x >= p8 * MOTTO_COLS || y >= p8 * LOGO_ROWS {
        return false;
    }
    let (col, row) = (x / p8, y / p8);
    if MOTTO[row as usize].as_bytes()[col as usize] != b'#' {
        return false;
    }
    let (dx, dy) = (x - col * p8, y - row * p8);
    if dx >= dot8 || dy >= dot8 {
        return false;
    }
    let cx = if dx < r { r } else if dx > dot8 - r { dot8 - r } else { dx };
    let cy = if dy < r { r } else if dy > dot8 - r { dot8 - r } else { dy };
    (dx - cx) * (dx - cx) + (dy - cy) * (dy - cy) <= r * r
}

fn track_hit(g: &Geometry, sx: i64, sy: i64) -> bool {
    let r = g.track_h8 / 2;
    let (x0, x1, y0) = (g.track_x8, g.track_x8 + g.track_w8, g.track_y8);
    if sy < y0 || sy >= y0 + g.track_h8 || sx < x0 || sx >= x1 {
        return false;
    }
    if sx >= x0 + r && sx < x1 - r {
        return true;
    }
    let cx = if sx < x0 + r { x0 + r } else { x1 - r };
    let dy = sy - (y0 + r);
    (sx - cx) * (sx - cx) + dy * dy <= r * r
}

fn mix(from: u32, to: u32, k: i64, n: i64) -> u32 {
    let mut out = 0u32;
    for s in [0, 8, 16] {
        let (a, b) = (((from >> s) & 0xff) as i64, ((to >> s) & 0xff) as i64);
        out |= (((a * (n - k) + b * k + n / 2) / n) as u32) << s;
    }
    out
}

/// 0x00RRGGBB pixels, width x height, for "boot" | "reboot" | "poweroff" | "full".
pub fn render(width: i32, height: i32, mode: &str, p: &Palette) -> Vec<u32> {
    let (w, h) = (width.max(1), height.max(1));
    let g = geometry(w, h);
    let dim = mode == "poweroff";
    let full = mode == "full";
    let fg = if dim { mix(p.background, p.text, 35, 100) } else { p.text };
    let ac = if dim { mix(p.background, p.accent, 35, 100) } else { p.accent };
    let track = if full { p.accent } else { p.surface };
    let mut px = vec![p.background; (w as usize) * (h as usize)];
    let y0 = (g.word_y8 / 8).max(0) as i32;
    let y1 = (((g.track_y8 + g.track_h8) / 8) as i32 + 1).min(h);
    let x0 = (g.word_x8 / 8).max(0) as i32;
    let x1 = (((g.word_x8 + g.word_w8) / 8) as i32 + 1).min(w);
    for y in y0..y1 {
        for x in x0..x1 {
            let (mut nw, mut no, mut nt) = (0i64, 0i64, 0i64);
            for j in 0..4i64 {
                for i in 0..4i64 {
                    let sx = x as i64 * 8 + 1 + 2 * i;
                    let sy = y as i64 * 8 + 1 + 2 * j;
                    match word_hit(&g, sx, sy) {
                        1 => nw += 1,
                        2 => no += 1,
                        _ => {
                            if !dim && track_hit(&g, sx, sy) {
                                nt += 1;
                            }
                        }
                    }
                }
            }
            if nw + no + nt > 0 {
                let mut c = 0u32;
                for s in [0, 8, 16] {
                    let ch = |v: u32| ((v >> s) & 0xff) as i64;
                    let v = ch(p.background) * (16 - nw - no - nt) + ch(fg) * nw + ch(ac) * no + ch(track) * nt;
                    c |= (((v + 8) / 16) as u32) << s;
                }
                px[(y * w + x) as usize] = c;
            }
        }
    }
    if mode == "boot" {
        let mc = mix(p.background, p.text, 55, 100);
        let pitch = g.motto_pitch as i32;
        let y0 = ((g.motto_y8 / 8) as i32).max(0);
        let y1 = ((g.motto_y8 / 8) as i32 + pitch * LOGO_ROWS as i32).min(h);
        let x0 = ((g.motto_x8 / 8) as i32).max(0);
        let x1 = ((g.motto_x8 / 8) as i32 + pitch * MOTTO_COLS as i32).min(w);
        for y in y0..y1 {
            for x in x0..x1 {
                let mut nm = 0i64;
                for j in 0..4i64 {
                    for i in 0..4i64 {
                        if motto_hit(&g, x as i64 * 8 + 1 + 2 * i, y as i64 * 8 + 1 + 2 * j) {
                            nm += 1;
                        }
                    }
                }
                if nm > 0 {
                    px[(y * w + x) as usize] = mix(p.background, mc, nm, 16);
                }
            }
        }
    }
    px
}
