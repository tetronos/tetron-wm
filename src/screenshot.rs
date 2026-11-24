//! Screenshot of the composited cell grid as a PNG, with no privileges and no
//! graphics stack: glyphs come from a Terminus PSF console font (the one the
//! bare-console setup installs). Mirrors what kmscon shows, since kmscon has no
//! graphics layer, but not its exact font or cursor.

use crate::buffer::CellBuffer;
use crate::cell::Rgba;
use std::collections::HashMap;

/// A parsed PSF2 bitmap font.
pub struct Psf {
    pub w: usize,
    pub h: usize,
    row_bytes: usize,
    glyphs: Vec<u8>,
    charsize: usize,
    map: HashMap<char, usize>,
}

impl Psf {
    pub fn parse(d: &[u8]) -> Option<Psf> {
        if d.len() < 32 || d[..4] != [0x72, 0xb5, 0x4a, 0x86] {
            return None;
        }
        let u = |i: usize| u32::from_le_bytes([d[i], d[i + 1], d[i + 2], d[i + 3]]) as usize;
        let (hdr, flags, n, charsize, h, w) = (u(8), u(12), u(16), u(20), u(24), u(28));
        let row_bytes = w.div_ceil(8);
        let gend = hdr.checked_add(n.checked_mul(charsize)?)?;
        if d.len() < gend || charsize < row_bytes * h {
            return None;
        }
        let mut map = HashMap::new();
        if flags & 1 != 0 {
            let mut idx = 0;
            let mut in_seq = false;
            let mut i = gend;
            while i < d.len() && idx < n {
                match d[i] {
                    0xFF => { idx += 1; in_seq = false; i += 1; }
                    0xFE => { in_seq = true; i += 1; }
                    b => {
                        let len = match b { 0..=0x7f => 1, 0xc0..=0xdf => 2, 0xe0..=0xef => 3, _ => 4 };
                        let end = (i + len).min(d.len());
                        if !in_seq {
                            if let Some(c) = std::str::from_utf8(&d[i..end]).ok().and_then(|s| s.chars().next()) {
                                map.entry(c).or_insert(idx);
                            }
                        }
                        i = end;
                    }
                }
            }
        } else {
            for i in 0..n.min(256) {
                map.insert(i as u8 as char, i);
            }
        }
        Some(Psf { w, h, row_bytes, glyphs: d[hdr..gend].to_vec(), charsize, map })
    }

    /// Whether `n` names a PSF console font (console-setup: .psf, kbd: .psfu).
    fn is_psf_name(n: &str) -> bool {
        [".psf", ".psf.gz", ".psfu", ".psfu.gz"].iter().any(|e| n.ends_with(e))
    }

    /// Load the first usable Terminus console font found on this system.
    pub fn load_system() -> Option<Psf> {
        use std::io::Read;
        // Every console font directory (WMNIX-005); the first one that has a
        // font wins, as kbd and console-setup each ship a full set.
        let (dir, mut names) = crate::paths::consolefont_dirs().into_iter().find_map(|dir| {
            let names: Vec<String> = std::fs::read_dir(&dir).ok()?.flatten().map(|e| e.file_name().to_string_lossy().into_owned())
                .filter(|n| Self::is_psf_name(n)).collect();
            (!names.is_empty()).then_some((dir, names))
        })?;
        let rank = |n: &str| -> i32 {
            let mut r = 0;
            if n.contains("Terminus") { r -= 10; }
            if n.starts_with("Uni2") { r -= 5; }
            if n.contains("16") { r -= 3; }
            if n.contains("Bold") { r += 2; }
            r
        };
        names.sort_by_key(|n| (rank(n), n.clone()));
        for n in names {
            let Ok(raw) = std::fs::read(dir.join(&n)) else { continue };
            let data = if n.ends_with(".gz") {
                let mut out = Vec::new();
                if flate2::read::GzDecoder::new(&raw[..]).read_to_end(&mut out).is_err() { continue; }
                out
            } else { raw };
            if let Some(f) = Psf::parse(&data) { return Some(f); }
        }
        None
    }

    fn glyph(&self, c: char) -> Option<&[u8]> {
        let i = *self.map.get(&c).or_else(|| self.map.get(&'?'))?;
        let o = i * self.charsize;
        self.glyphs.get(o..o + self.charsize)
    }

    fn on(&self, g: &[u8], x: usize, y: usize) -> bool {
        g[y * self.row_bytes + x / 8] & (0x80 >> (x % 8)) != 0
    }
}

/// Coverage of `c` at pixel (x, y) of a w*h cell, 0..=255, for glyphs drawn
/// geometrically: block elements, quadrants, braille and sextants. Wallpaper
/// cell art (chafa) is made of these, and console fonts rarely carry them all.
fn block_cov(c: char, x: usize, y: usize, w: usize, h: usize) -> Option<u8> {
    let on = |b: bool| Some(if b { 255 } else { 0 });
    let (top, left) = (y < h / 2, x < w / 2);
    let u = c as u32;
    match u {
        0x2580 => on(top),
        0x2581..=0x2587 => on(y >= h - h * (u - 0x2580) as usize / 8),
        0x2588 => on(true),
        0x2589..=0x258f => on(x < w * (0x2590 - u) as usize / 8),
        0x2590 => on(!left),
        0x2591 => Some(64),
        0x2592 => Some(128),
        0x2593 => Some(192),
        0x2594 => on(y < h / 8),
        0x2595 => on(x >= w - w / 8),
        // Diagonal mosaic wedges: about half a cell, approximated as a blend.
        0x1fb3c..=0x1fb6f | 0x25e2..=0x25e5 => Some(128),
        0x2596..=0x259f => {
            // bits: 1 UL, 2 UR, 4 LL, 8 LR
            const Q: [u8; 10] = [4, 8, 1, 1 | 4 | 8, 1 | 8, 1 | 2 | 4, 1 | 2 | 8, 2, 2 | 4, 2 | 4 | 8];
            let m = Q[(u - 0x2596) as usize];
            let bit = match (top, left) { (true, true) => 1, (true, false) => 2, (false, true) => 4, (false, false) => 8 };
            on(m & bit != 0)
        }
        0x2800..=0x28ff => {
            let (col, row) = (x * 2 / w, y * 4 / h);
            // dot bit positions: column-major for rows 0-2, then row 3
            let bit = match (col, row) { (0, 0) => 0, (0, 1) => 1, (0, 2) => 2, (1, 0) => 3, (1, 1) => 4, (1, 2) => 5, (0, _) => 6, _ => 7 };
            on(u & (1 << bit) != 0)
        }
        0x1fb00..=0x1fb3b => {
            let mut v = u - 0x1fb00 + 1;
            if v >= 21 { v += 1; }
            if v >= 42 { v += 1; }
            let bit = (y * 3 / h) * 2 + x * 2 / w;
            on(v & (1 << bit) != 0)
        }
        _ => None,
    }
}

fn mix(bg: Rgba, fg: Rgba, a: u8) -> [u8; 3] {
    let m = |b: u8, f: u8| ((b as u32 * (255 - a as u32) + f as u32 * a as u32) / 255) as u8;
    [m(bg.r, fg.r), m(bg.g, fg.g), m(bg.b, fg.b)]
}

/// Render the buffer to an RGB image.
pub fn render(buf: &CellBuffer, font: &Psf) -> image::RgbImage {
    let (cw, ch) = (font.w, font.h);
    let mut img = image::RgbImage::new(buf.width() as u32 * cw as u32, buf.height() as u32 * ch as u32);
    let black = Rgba::rgb(0, 0, 0);
    for cy in 0..buf.height() {
        for cx in 0..buf.width() {
            let Some(cell) = buf.get(cx, cy) else { continue };
            let (mut fg, mut bg) = (cell.fg.over(black), cell.bg.over(black));
            if cell.attrs.inverse { std::mem::swap(&mut fg, &mut bg); }
            let glyph = if cell.ch == ' ' { None } else { font.glyph(cell.ch) };
            for py in 0..ch {
                for px in 0..cw {
                    let a = match block_cov(cell.ch, px, py, cw, ch) {
                        Some(a) => a,
                        None => match glyph {
                            Some(g) if font.on(g, px, py) => 255,
                            _ => 0,
                        },
                    };
                    let a = if cell.attrs.underline && py == ch - 1 { 255 } else { a };
                    img.put_pixel((cx as usize * cw + px) as u32, (cy as usize * ch + py) as u32, image::Rgb(mix(bg, fg, a)));
                }
            }
        }
    }
    img
}

/// Write a PNG of `buf` to `path`. Errors are returned as text for the log.
/// Why a screenshot fails on a host with no console fonts. Debian and Ubuntu ship
/// the PSF fonts in console-setup-linux; fonts-terminus there has none.
pub const NO_FONT: &str = "no PSF console font found in /usr/share/consolefonts \
(Debian and Ubuntu: sudo apt install console-setup-linux; Fedora: kbd; Arch: terminus-font)";

pub fn save_png(buf: &CellBuffer, path: &std::path::Path) -> Result<(), String> {
    let font = Psf::load_system().ok_or(NO_FONT)?;
    render(buf, &font).save(path).map_err(|e| e.to_string())
}
