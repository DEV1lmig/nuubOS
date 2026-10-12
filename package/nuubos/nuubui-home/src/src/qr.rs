/*
 * Minimal QR Code encoder (ISO/IEC 18004) for the Syncthing device ID a
 * phone scans: alphanumeric or byte mode, error correction level M,
 * versions 1-10, the mask with the lowest penalty. Returns the modules
 * (true = dark), without the quiet zone.
 *
 * The structure follows the public specification; the Reed-Solomon and
 * placement steps are the well-known reference algorithms.
 */

/* Error correction level M, versions 1..=10. */
const ECC_PER_BLOCK: [usize; 11] = [0, 10, 16, 26, 18, 24, 16, 18, 22, 22, 26];
const BLOCKS: [usize; 11] = [0, 1, 1, 1, 2, 2, 4, 4, 4, 5, 5];
const ALPHANUMERIC: &str = "0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZ $%*+-./:";

struct Bits(Vec<bool>);

impl Bits {
    fn push(&mut self, value: u32, len: usize) {
        for i in (0..len).rev() {
            self.0.push((value >> i) & 1 != 0);
        }
    }
}

fn raw_modules(ver: usize) -> usize {
    let mut n = (16 * ver + 128) * ver + 64;
    if ver >= 2 {
        let align = ver / 7 + 2;
        n -= (25 * align - 10) * align - 55;
        if ver >= 7 {
            n -= 36;
        }
    }
    n
}

fn data_codewords(ver: usize) -> usize {
    raw_modules(ver) / 8 - ECC_PER_BLOCK[ver] * BLOCKS[ver]
}

fn gf_mul(x: u8, y: u8) -> u8 {
    let mut z: u8 = 0;
    for i in (0..8).rev() {
        z = (z << 1) ^ ((z >> 7) * 0x1d);
        z ^= ((y >> i) & 1) * x;
    }
    z
}

fn rs_divisor(degree: usize) -> Vec<u8> {
    let mut d = vec![0u8; degree];
    d[degree - 1] = 1;
    let mut root: u8 = 1;
    for _ in 0..degree {
        for j in 0..degree {
            d[j] = gf_mul(d[j], root);
            if j + 1 < degree {
                d[j] ^= d[j + 1];
            }
        }
        root = gf_mul(root, 0x02);
    }
    d
}

fn rs_remainder(data: &[u8], divisor: &[u8]) -> Vec<u8> {
    let mut r = vec![0u8; divisor.len()];
    for &b in data {
        let factor = b ^ r.remove(0);
        r.push(0);
        for (x, &y) in r.iter_mut().zip(divisor) {
            *x ^= gf_mul(y, factor);
        }
    }
    r
}

/* Data codewords split in blocks, each with its error correction, then
 * interleaved. */
fn with_ecc(data: &[u8], ver: usize) -> Vec<u8> {
    let blocks = BLOCKS[ver];
    let ecc_len = ECC_PER_BLOCK[ver];
    let raw = raw_modules(ver) / 8;
    let short = blocks - raw % blocks;
    let short_len = raw / blocks;
    let divisor = rs_divisor(ecc_len);
    let mut all = Vec::new();
    let mut k = 0;
    for i in 0..blocks {
        let len = short_len - ecc_len + usize::from(i >= short);
        let mut b = data[k..k + len].to_vec();
        k += len;
        let ecc = rs_remainder(&b, &divisor);
        if i < short {
            b.push(0);
        }
        b.extend_from_slice(&ecc);
        all.push(b);
    }
    let mut out = Vec::with_capacity(raw);
    for i in 0..all[0].len() {
        for (j, b) in all.iter().enumerate() {
            if i != short_len - ecc_len || j >= short {
                out.push(b[i]);
            }
        }
    }
    out
}

struct Matrix {
    size: usize,
    dark: Vec<bool>,
    function: Vec<bool>,
}

impl Matrix {
    fn set(&mut self, x: usize, y: usize, dark: bool) {
        self.dark[y * self.size + x] = dark;
        self.function[y * self.size + x] = true;
    }

    fn finder(&mut self, cx: i32, cy: i32) {
        for dy in -4..=4i32 {
            for dx in -4..=4i32 {
                let (x, y) = (cx + dx, cy + dy);
                if x >= 0 && y >= 0 && (x as usize) < self.size && (y as usize) < self.size {
                    let d = dx.abs().max(dy.abs());
                    self.set(x as usize, y as usize, d != 2 && d != 4);
                }
            }
        }
    }

    fn alignment(&mut self, cx: usize, cy: usize) {
        for dy in -2..=2i32 {
            for dx in -2..=2i32 {
                self.set((cx as i32 + dx) as usize, (cy as i32 + dy) as usize, dx.abs().max(dy.abs()) != 1);
            }
        }
    }

    fn format(&mut self, mask: u32) {
        /* Level M = 00. */
        let data = mask;
        let mut rem = data;
        for _ in 0..10 {
            rem = (rem << 1) ^ ((rem >> 9) * 0x537);
        }
        let bits = ((data << 10) | rem) ^ 0x5412;
        let bit = |i: u32| (bits >> i) & 1 != 0;
        let s = self.size;
        for i in 0..6 {
            self.set(8, i, bit(i as u32));
        }
        self.set(8, 7, bit(6));
        self.set(8, 8, bit(7));
        self.set(7, 8, bit(8));
        for i in 9..15 {
            self.set(14 - i, 8, bit(i as u32));
        }
        for i in 0..8 {
            self.set(s - 1 - i, 8, bit(i as u32));
        }
        for i in 8..15 {
            self.set(8, s - 15 + i, bit(i as u32));
        }
        self.set(8, s - 8, true);
    }

    fn version(&mut self, ver: usize) {
        if ver < 7 {
            return;
        }
        let mut rem = ver as u32;
        for _ in 0..12 {
            rem = (rem << 1) ^ ((rem >> 11) * 0x1f25);
        }
        let bits = ((ver as u32) << 12) | rem;
        for i in 0..18 {
            let dark = (bits >> i) & 1 != 0;
            let a = self.size - 11 + i % 3;
            let b = i / 3;
            self.set(a, b, dark);
            self.set(b, a, dark);
        }
    }
}

fn alignment_positions(ver: usize, size: usize) -> Vec<usize> {
    if ver == 1 {
        return Vec::new();
    }
    let n = ver / 7 + 2;
    let step = (ver * 4 + n * 2 + 1) / (n * 2 - 2) * 2;
    let mut p: Vec<usize> = (0..n - 1).map(|i| size - 7 - i * step).collect();
    p.push(6);
    p.reverse();
    p
}

fn masked(mask: u32, x: usize, y: usize) -> bool {
    match mask {
        0 => (x + y) % 2 == 0,
        1 => y % 2 == 0,
        2 => x % 3 == 0,
        3 => (x + y) % 3 == 0,
        4 => (x / 3 + y / 2) % 2 == 0,
        5 => x * y % 2 + x * y % 3 == 0,
        6 => (x * y % 2 + x * y % 3) % 2 == 0,
        _ => ((x + y) % 2 + x * y % 3) % 2 == 0,
    }
}

/* The four penalty rules of the specification. */
fn penalty(m: &Matrix) -> usize {
    let s = m.size;
    let at = |x: usize, y: usize| m.dark[y * s + x];
    let mut score = 0;
    for horizontal in [true, false] {
        for a in 0..s {
            let line: Vec<bool> = (0..s).map(|b| if horizontal { at(b, a) } else { at(a, b) }).collect();
            let mut run = 1;
            for b in 1..=s {
                if b < s && line[b] == line[b - 1] {
                    run += 1;
                } else {
                    if run >= 5 {
                        score += run - 2;
                    }
                    run = 1;
                }
            }
            /* 1:1:3:1:1 with four light modules on one side. */
            const PAT: [bool; 11] = [true, false, true, true, true, false, true, false, false, false, false];
            for b in 0..s.saturating_sub(10) {
                let w = &line[b..b + 11];
                if w.iter().zip(PAT.iter()).all(|(p, q)| p == q) || w.iter().zip(PAT.iter().rev()).all(|(p, q)| p == q) {
                    score += 40;
                }
            }
        }
    }
    for y in 0..s - 1 {
        for x in 0..s - 1 {
            let c = at(x, y);
            if c == at(x + 1, y) && c == at(x, y + 1) && c == at(x + 1, y + 1) {
                score += 3;
            }
        }
    }
    let dark = m.dark.iter().filter(|d| **d).count();
    let percent = dark * 100 / (s * s);
    score + (percent.abs_diff(50) / 5) * 10
}

/* `text` uppercase alphanumeric (a Syncthing device ID) or any bytes.
 * None when it does not fit in version 10. */
pub fn encode(text: &str) -> Option<(usize, Vec<bool>)> {
    let alnum = text.chars().all(|c| ALPHANUMERIC.contains(c));
    let (ver, mut bits) = (1..=10).find_map(|ver| {
        let mut b = Bits(Vec::new());
        if alnum {
            b.push(0b0010, 4);
            b.push(text.len() as u32, if ver <= 9 { 9 } else { 11 });
            let v: Vec<u32> = text.chars().map(|c| ALPHANUMERIC.find(c).unwrap_or(0) as u32).collect();
            for pair in v.chunks(2) {
                if pair.len() == 2 {
                    b.push(pair[0] * 45 + pair[1], 11);
                } else {
                    b.push(pair[0], 6);
                }
            }
        } else {
            b.push(0b0100, 4);
            b.push(text.len() as u32, if ver <= 9 { 8 } else { 16 });
            for byte in text.bytes() {
                b.push(byte as u32, 8);
            }
        }
        (b.0.len() <= data_codewords(ver) * 8).then_some((ver, b))
    })?;
    let capacity = data_codewords(ver) * 8;
    let terminator = (capacity - bits.0.len()).min(4);
    bits.push(0, terminator);
    let pad = (8 - bits.0.len() % 8) % 8;
    bits.push(0, pad);
    let mut data: Vec<u8> = bits.0.chunks(8).map(|c| c.iter().fold(0u8, |a, &b| (a << 1) | u8::from(b))).collect();
    let mut filler = [0xecu8, 0x11].into_iter().cycle();
    while data.len() < data_codewords(ver) {
        data.push(filler.next().unwrap_or(0));
    }
    let codewords = with_ecc(&data, ver);

    let size = ver * 4 + 17;
    let mut m = Matrix { size, dark: vec![false; size * size], function: vec![false; size * size] };
    for i in 0..size {
        m.set(6, i, i % 2 == 0);
        m.set(i, 6, i % 2 == 0);
    }
    m.finder(3, 3);
    m.finder(size as i32 - 4, 3);
    m.finder(3, size as i32 - 4);
    let pos = alignment_positions(ver, size);
    let n = pos.len();
    for i in 0..n {
        for j in 0..n {
            if !((i == 0 && j == 0) || (i == 0 && j == n - 1) || (i == n - 1 && j == 0)) {
                m.alignment(pos[i], pos[j]);
            }
        }
    }
    m.format(0);
    m.version(ver);

    /* Zig-zag placement, two columns at a time from the right. */
    let mut i = 0;
    let mut right = size as i32 - 1;
    while right >= 1 {
        if right == 6 {
            right = 5;
        }
        for vert in 0..size {
            for j in 0..2 {
                let x = (right - j) as usize;
                let upward = (right + 1) & 2 == 0;
                let y = if upward { size - 1 - vert } else { vert };
                if !m.function[y * size + x] && i < codewords.len() * 8 {
                    m.dark[y * size + x] = (codewords[i >> 3] >> (7 - (i & 7))) & 1 != 0;
                    i += 1;
                }
            }
        }
        right -= 2;
    }

    let base = m.dark.clone();
    let mut best: Option<(usize, u32)> = None;
    for mask in 0..8 {
        m.dark.copy_from_slice(&base);
        for y in 0..size {
            for x in 0..size {
                if !m.function[y * size + x] && masked(mask, x, y) {
                    m.dark[y * size + x] ^= true;
                }
            }
        }
        m.format(mask);
        let p = penalty(&m);
        if best.map_or(true, |(b, _)| p < b) {
            best = Some((p, mask));
        }
    }
    let mask = best.map(|b| b.1).unwrap_or(0);
    m.dark.copy_from_slice(&base);
    for y in 0..size {
        for x in 0..size {
            if !m.function[y * size + x] && masked(mask, x, y) {
                m.dark[y * size + x] ^= true;
            }
        }
    }
    m.format(mask);
    Some((size, m.dark))
}
