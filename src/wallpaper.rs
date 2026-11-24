//! PROTOTYPE image wallpaper via chafa (cell-art). Experimental / throwaway:
//! gated behind the `TETRON_WM_WALLPAPER=<image>` env var so it changes nothing
//! unless asked. Converts an image to a w×h grid of colored cells (works on any
//! terminal incl. a kmscon console, since the output is cells, not graphics).

use crate::buffer::CellBuffer;
use crate::cell::{Cell, Rgba};
use std::path::PathBuf;

/// The wallpapers bundled in the binary, one per built-in theme (426x240 JPEGs
/// from `themes/`, each under 100 KB; a test checks). `default` draws the
/// active theme's picture, and a configured file that is missing falls back to
/// it, so the desktop never goes blank by accident.
const EMBEDDED: &[(&str, &[u8])] = &[
    ("midnight", include_bytes!("../themes/midnight.jpg")),
    ("gruvbox", include_bytes!("../themes/gruvbox.jpg")),
    ("pastoral", include_bytes!("../themes/pastoral.jpg")),
    ("windows", include_bytes!("../themes/windows.jpg")),
];

/// The picture for a theme with none of its own, and the plain default.
const FALLBACK_IMAGE: &str = "midnight";

/// What decides the picture `default` shows for `theme`: its file's picture, or
/// the bundled one. Equal keys mean the same picture, so a theme change that
/// keeps the key needs no redraw.
pub fn theme_key(theme: &str) -> (Option<String>, &'static str) {
    (crate::theme::wallpaper_for(theme), theme_image(theme))
}

/// Which bundled picture `default` shows for `theme` (a theme name, any case):
/// its own when it has one, else midnight's.
pub fn theme_image(theme: &str) -> &'static str {
    let t = theme.trim().to_lowercase();
    let t = if t == "light" { "pastoral" } else { t.as_str() };
    EMBEDDED.iter().map(|(n, _)| *n).find(|n| *n == t).unwrap_or(FALLBACK_IMAGE)
}

fn embedded(name: &str) -> &'static [u8] {
    EMBEDDED.iter().find(|(n, _)| *n == name).map_or(EMBEDDED[0].1, |(_, b)| b)
}

/// Config value that means "the built-in wallpaper". An empty value means the
/// same; Settings writes `default` back so the name is always visible.
pub const DEFAULT_NAME: &str = "default";

/// The config default before `default` existed. Configs still carrying it get
/// the built-in image without a "not found" note when no such file exists.
const LEGACY_NAME: &str = "wallpaper.jpg";

/// Does config value `spec` ask for the built-in wallpaper (unset, empty, or
/// the `default` keyword)?
pub fn is_default(spec: Option<&str>) -> bool {
    spec.map(clean_spec).is_none_or(|s| s.is_empty() || s == DEFAULT_NAME)
}

/// What a wallpaper value asks for, before any file is looked at.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Source {
    /// An image: the built-in, a bundled one, or a file.
    Image,
    /// `none`: no wallpaper, the plain theme `desktop_bg`.
    Plain,
    /// `black` or `#rrggbb`: a flat colour, drawn without chafa.
    Solid(Rgba),
}

/// Classify wallpaper value `spec` (see [`Source`]). Only `none`, `black` and
/// a `#rrggbb` colour are special; anything else, including an unset value, is
/// an image (and a malformed `#…` stays one, so the "not found" note shows).
pub fn source(spec: Option<&str>) -> Source {
    let Some(spec) = spec else { return Source::Image };
    let s = clean_spec(spec);
    if s.eq_ignore_ascii_case("none") {
        return Source::Plain;
    }
    if s.eq_ignore_ascii_case("black") {
        return Source::Solid(Rgba::rgb(0, 0, 0));
    }
    match s.strip_prefix('#') {
        Some(hex) if hex.len() == 6 => crate::badge::parse_color(&s).map_or(Source::Image, |c| Source::Solid(Rgba { a: 255, ..c })),
        _ => Source::Image,
    }
}

/// What to put behind the icons for `spec` at colour `depth`, before any file
/// is read. Below 256 colours a theme's picture turns to noise, so `default`
/// is the theme's own desktop colour there; a file the user chose is drawn at
/// any depth.
///
/// `themed` is true when the value is not the user's own pick: the config says
/// `default`, so the theme's picture (or the built-in one) is shown.
pub fn draw_for(spec: Option<&str>, themed: bool, depth: crate::display::ColorDepth) -> Option<Source> {
    match source(spec) {
        Source::Plain => None,
        s @ Source::Solid(_) => Some(s),
        Source::Image if (themed || is_default(spec)) && depth < crate::display::ColorDepth::Ansi256 => None,
        Source::Image => Some(Source::Image),
    }
}

/// A `w`×`h` buffer of one background colour.
pub fn solid(color: Rgba, w: i32, h: i32) -> CellBuffer {
    let mut buf = CellBuffer::new(w.max(0), h.max(0));
    buf.fill(Cell { ch: ' ', fg: color, bg: color, attrs: Default::default() });
    buf
}

/// Normalise a wallpaper value the way people paste it: surrounding quotes
/// (`"/a b.jpg"`, `'/a b.jpg'`, from a shell or a file manager's "copy
/// path"), a `file://` URL (percent-decoded, so `%20` becomes a space), and a
/// leading `$HOME` / `${HOME}` (becomes `~`, which [`resolve_path`] expands).
pub fn clean_spec(spec: &str) -> String {
    let mut s = spec.trim();
    for q in ['"', '\''] {
        if s.len() >= 2 && s.starts_with(q) && s.ends_with(q) {
            s = s[1..s.len() - 1].trim();
        }
    }
    if let Some(rest) = s.strip_prefix("file://") {
        // `file://localhost/x` and `file:///x` both name `/x`.
        let rest = rest.strip_prefix("localhost").unwrap_or(rest);
        return percent_decode(rest);
    }
    for var in ["$HOME", "${HOME}"] {
        if let Some(rest) = s.strip_prefix(var) {
            if rest.is_empty() || rest.starts_with('/') {
                return format!("~{rest}");
            }
        }
    }
    s.to_string()
}

/// Decode `%XX` escapes (as bytes, so multi-byte UTF-8 survives); a malformed
/// escape is kept as typed.
fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let hex = |c: u8| (c as char).to_digit(16);
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            if let (Some(h), Some(l)) = (hex(b[i + 1]), hex(b[i + 2])) {
                out.push((h * 16 + l) as u8);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// What to draw for a config value: the image file, plus the configured file
/// when it was missing (the built-in image stands in; the caller logs it).
pub struct Choice {
    /// File to hand to chafa. `None` only if the built-in could not be written.
    pub path: Option<PathBuf>,
    /// The configured file that does not exist, when that is why `path` is
    /// the built-in image.
    pub missing: Option<PathBuf>,
}

/// Pick the file to draw for config value `spec` (see [`Choice`]).
pub fn choose(spec: Option<&str>, theme: &str) -> Choice {
    if is_default(spec) {
        return Choice { path: ensure_default(theme), missing: None };
    }
    let spec = spec.unwrap_or_default();
    let p = resolve_path(spec);
    if p.exists() {
        return Choice { path: Some(p), missing: None };
    }
    let legacy = clean_spec(spec) == LEGACY_NAME;
    Choice { path: ensure_default(theme), missing: (!legacy).then_some(p) }
}

/// A one-line problem with config value `spec` for Settings to show under the
/// wallpaper rows, or `None` when it will draw as asked. Only looks at the
/// disk and `$PATH`; never writes.
pub fn note(spec: Option<&str>) -> Option<String> {
    if source(spec) != Source::Image {
        return None; // a flat colour or no wallpaper needs neither chafa nor a file
    }
    if !chafa_installed() {
        return Some("chafa is not installed, so the desktop is plain.".into());
    }
    if is_default(spec) {
        return None;
    }
    let spec = spec.unwrap_or_default();
    let p = resolve_path(spec);
    if p.exists() || clean_spec(spec) == LEGACY_NAME {
        return None;
    }
    Some(format!("Not found, showing the built-in: {}", p.display()))
}

/// Is `chafa` (the wallpaper renderer) on `$PATH`?
fn chafa_installed() -> bool {
    crate::paths::on_path("chafa")
}

/// Write the bundled picture for `theme` to `<data>/tetron-wm/wallpaper-<name>.jpg`
/// (`$XDG_DATA_HOME` aware) unless the file there already holds exactly these
/// bytes, and return its path. `chafa` needs a real file, hence the stamp.
/// Comparing contents (not size) means a new picture always replaces the old
/// copy. The copies older versions wrote (`wallpaper.jpg`,
/// `default-wallpaper.jpg`) are removed.
pub fn ensure_default(theme: &str) -> Option<PathBuf> {
    let base = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|| dirs::home_dir().map(|h| h.join(".local/share")))?;
    let dir = base.join("tetron-wm");
    let name = theme_image(theme);
    let bytes = embedded(name);
    let path = dir.join(format!("wallpaper-{name}.jpg"));
    let current = std::fs::read(&path).map(|b| b == bytes).unwrap_or(false);
    if !current {
        std::fs::create_dir_all(&dir).ok()?;
        std::fs::write(&path, bytes).ok()?;
        for old in ["wallpaper.jpg", "default-wallpaper.jpg"] {
            let _ = std::fs::remove_file(dir.join(old));
        }
        crate::dbg_log(&format!("wallpaper: stamped the {name} picture to {}", path.display()));
    }
    Some(path)
}

/// Resolve a configured wallpaper path to a file on disk (after [`clean_spec`]).
///
/// - An absolute path (`/…`) is used as-is.
/// - A `~`-path is expanded against the home directory.
/// - A relative path is looked up in the theme folders, in order
///   (`~/.config/tetron-wm/themes/`, `~/.local/share/tetron-wm/themes/`, the
///   system data dirs), then the config dir root, then the old
///   `wallpapers/` folder (kept so existing configs keep working). It is
///   **never** resolved against the process cwd (the daemon's cwd is
///   unpredictable — a service, a reload, launched from anywhere), so a bare
///   filename always means "next to your config".
///
/// Returns the first candidate that exists, or the best-effort candidate when
/// none exist (so the caller's own "missing file" handling / log still fires).
pub fn resolve_path(path: &str) -> PathBuf {
    let cleaned = clean_spec(path);
    let path = cleaned.as_str();
    if let Some(rest) = path.strip_prefix("~/") {
        if let Some(home) = dirs::home_dir() {
            return home.join(rest);
        }
    }
    let p = PathBuf::from(path);
    if p.is_absolute() {
        return p;
    }
    resolve_relative(path, &crate::theme::themes_dirs(), crate::config::config_dir().as_deref())
}

/// A relative wallpaper path: the first theme directory that has it (in
/// [`crate::theme::themes_dirs`] order), then the config dir root, then the old
/// `wallpapers/` folder. When none has it, the best-effort candidate in the
/// first theme directory, so the caller's "missing file" log names it.
fn resolve_relative(path: &str, theme_dirs: &[PathBuf], cfg: Option<&std::path::Path>) -> PathBuf {
    if let Some(t) = theme_dirs.iter().map(|d| d.join(path)).find(|t| t.exists()) {
        return t;
    }
    if let Some(cfg) = cfg {
        for c in [cfg.join(path), cfg.join("wallpapers").join(path)] {
            if c.exists() {
                return c;
            }
        }
    }
    theme_dirs.first().map(|d| d.join(path)).or_else(|| cfg.map(|c| c.join(path))).unwrap_or_else(|| PathBuf::from(path))
}

/// chafa symbol classes the console font can draw. `ascii` (the default style)
/// must never emit a glyph a console font lacks: kernel VT fonts have only the
/// space, full block and the ░ ▒ shades (no half blocks at all), while DejaVu
/// (kmscon) has every half block and quadrant.
/// Sextants and braille are absent from those fonts, and as kmscon fallbacks they
/// are wider than a cell, so only the GUI styles (`nerd`/`emoji`) use `all-wide`.
/// Whether the client is a bare VT comes from its reported caps (`display`),
/// not the daemon's own `TERM`, which may belong to a different terminal.
fn symbol_set() -> &'static str {
    use crate::iconset::IconStyle;
    match crate::iconset::style() {
        IconStyle::Ascii if crate::display::get().vt => "space+solid+0x2591..0x2592",
        // DejaVu Sans Mono (kmscon) has every half block and quadrant.
        IconStyle::Ascii | IconStyle::Unicode | IconStyle::Auto => "half+quad+space+solid",
        _ => "all-wide",
    }
}

/// chafa's colour mode for the client's display. With a small palette chafa
/// picks symbols and colours together (far better art than quantizing a
/// truecolor render afterwards); a bare VT has only 8 background colours.
/// Mono never gets here (`SessionCore::build_wallpaper` skips the wallpaper).
fn color_mode(caps: crate::display::ClientCaps) -> &'static str {
    use crate::display::ColorDepth;
    match caps.colors {
        ColorDepth::Mono => "2",
        ColorDepth::Ansi16 if caps.vt => "8",
        ColorDepth::Ansi16 => "16",
        ColorDepth::Ansi256 => "240",
        ColorDepth::TrueColor => "full",
    }
}

/// Build a `w`×`h` cell wallpaper from `path` using chafa's symbol output,
/// stretched to fill exactly. Returns `None` if chafa is missing/fails.
///
/// The chafa call is **bounded** (stdin closed, hard timeout). This runs on the
/// daemon's input/render loop (via `SessionCore::build_wallpaper`), so a chafa
/// that ever blocked — an inherited stdin, a slow or malformed file, a wedged
/// process — would otherwise freeze the whole UI until restart.
pub fn from_image(path: &str, w: i32, h: i32) -> Option<CellBuffer> {
    if w <= 0 || h <= 0 {
        return None;
    }
    let size = format!("{w}x{h}");
    let args = [
        "-f", "symbols",
        "-c", color_mode(crate::display::get()),
        // `all-wide`: every symbol EXCEPT the 2-column-wide ones. A wide glyph
        // (e.g. a CJK char chafa picks) would occupy two cells but our grid is
        // strictly one glyph per column, so it shifts the row and leaves black
        // gaps. Excluding wide keeps exactly w×h single-column cells.
        "--symbols", symbol_set(),
        "--stretch",
        "-s", &size,
        path,
    ];
    let out = run_chafa_capped(&args, std::time::Duration::from_secs(8))?;
    Some(parse_ansi(&String::from_utf8_lossy(&out), w, h))
}

/// Render `path` into exactly `w`×`h` cells for the image viewer, with the same
/// symbols and palette as the wallpaper. The caller already fitted the aspect
/// ratio, hence `--stretch`. Small palettes are dithered, which chafa only
/// does when asked and which keeps photos readable on a VT. `None` when chafa
/// is missing or fails.
pub fn viewer_cells(path: &std::path::Path, w: i32, h: i32) -> Option<CellBuffer> {
    if w <= 0 || h <= 0 {
        return None;
    }
    let size = format!("{w}x{h}");
    let colors = color_mode(crate::display::get());
    let path = path.to_string_lossy();
    let mut args = vec!["-f", "symbols", "-c", colors, "--symbols", symbol_set(), "--stretch", "-s", &size];
    if matches!(colors, "2" | "8" | "16") {
        args.extend(["--dither", "diffusion"]);
    }
    args.push(&path);
    let out = run_chafa_capped(&args, std::time::Duration::from_secs(8))?;
    Some(parse_ansi(&String::from_utf8_lossy(&out), w, h))
}

/// Run chafa with stdin closed and a hard timeout, draining stdout on a thread
/// so a full pipe can never deadlock the child. Returns its stdout on success,
/// or `None` if chafa is missing, fails, or outruns `timeout` (then killed).
fn run_chafa_capped(args: &[&str], timeout: std::time::Duration) -> Option<Vec<u8>> {
    use std::io::Read;
    use std::process::{Command, Stdio};
    use std::time::Instant;
    let mut child = Command::new("chafa")
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    // Drain stdout concurrently: chafa's full-screen output is far larger than
    // the OS pipe buffer, so reading only after exit could deadlock.
    let mut stdout = child.stdout.take()?;
    let reader = std::thread::spawn(move || {
        let mut buf = Vec::new();
        let _ = stdout.read_to_end(&mut buf);
        buf
    });
    let deadline = Instant::now() + timeout;
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                let buf = reader.join().unwrap_or_default();
                return if status.success() { Some(buf) } else { None };
            }
            Ok(None) => {
                if Instant::now() >= deadline {
                    let _ = child.kill();
                    let _ = child.wait();
                    let _ = reader.join();
                    crate::dbg_log("wallpaper: chafa timed out — killed (solid fallback)");
                    return None;
                }
                std::thread::sleep(std::time::Duration::from_millis(15));
            }
            Err(_) => {
                let _ = reader.join();
                return None;
            }
        }
    }
}

/// Parse chafa's truecolor-SGR symbol output into a `w`×`h` [`CellBuffer`].
fn parse_ansi(s: &str, w: i32, h: i32) -> CellBuffer {
    let def = Rgba::rgb(0, 0, 0);
    let mut buf = CellBuffer::new(w, h);
    buf.fill(Cell { ch: ' ', fg: def, bg: def, attrs: Default::default() });
    let (mut fg, mut bg) = (def, def);
    // chafa writes a cell of solid foreground as inverse video plus a space,
    // typically at the start of a row; ignoring it left black notches there.
    let mut inverse = false;
    let (mut x, mut y) = (0i32, 0i32);
    let mut it = s.chars().peekable();
    while let Some(c) = it.next() {
        match c {
            '\x1b' => {
                if it.peek() == Some(&'[') {
                    it.next();
                    let mut params = String::new();
                    let mut final_byte = ' ';
                    while let Some(&p) = it.peek() {
                        it.next();
                        if p.is_ascii_alphabetic() {
                            final_byte = p;
                            break;
                        }
                        params.push(p);
                    }
                    if final_byte == 'm' {
                        apply_sgr(&params, &mut fg, &mut bg, &mut inverse, def);
                    }
                }
            }
            '\n' => {
                y += 1;
                x = 0;
            }
            '\r' => x = 0,
            _ => {
                if x < w && y < h {
                    let (fg, bg) = if inverse { (bg, fg) } else { (fg, bg) };
                    buf.set(x, y, Cell { ch: c, fg, bg, attrs: Default::default() });
                }
                x += 1;
            }
        }
        if y >= h {
            break;
        }
    }
    buf
}

/// Apply one SGR parameter string, updating `fg`/`bg` (truecolor `38;2;r;g;b` /
/// `48;2;r;g;b`, 256-colour `38;5;n`, the 16 ANSI colours `30-37/90-97` and
/// `40-47/100-107`, plus reset/default) and inverse (`7` / `27`). Other
/// attributes are ignored.
fn apply_sgr(params: &str, fg: &mut Rgba, bg: &mut Rgba, inverse: &mut bool, def: Rgba) {
    let toks: Vec<&str> = params.split(';').collect();
    let num = |t: Option<&&str>| t.and_then(|s| s.parse::<u8>().ok()).unwrap_or(0);
    let mut i = 0;
    while i < toks.len() {
        match toks[i] {
            "" | "0" => {
                *fg = def;
                *bg = def;
                *inverse = false;
                i += 1;
            }
            "7" => {
                *inverse = true;
                i += 1;
            }
            "27" => {
                *inverse = false;
                i += 1;
            }
            "39" => {
                *fg = def;
                i += 1;
            }
            "49" => {
                *bg = def;
                i += 1;
            }
            "38" if toks.get(i + 1) == Some(&"2") => {
                *fg = Rgba::rgb(num(toks.get(i + 2)), num(toks.get(i + 3)), num(toks.get(i + 4)));
                i += 5;
            }
            "48" if toks.get(i + 1) == Some(&"2") => {
                *bg = Rgba::rgb(num(toks.get(i + 2)), num(toks.get(i + 3)), num(toks.get(i + 4)));
                i += 5;
            }
            "38" if toks.get(i + 1) == Some(&"5") => {
                *fg = xterm256(num(toks.get(i + 2)));
                i += 3;
            }
            "48" if toks.get(i + 1) == Some(&"5") => {
                *bg = xterm256(num(toks.get(i + 2)));
                i += 3;
            }
            t => {
                // The 16 ANSI colours, from chafa's `-c 8` / `-c 16` modes.
                match t.parse::<u8>() {
                    Ok(n @ 30..=37) => *fg = crate::display::VGA16[(n - 30) as usize],
                    Ok(n @ 90..=97) => *fg = crate::display::VGA16[(n - 90 + 8) as usize],
                    Ok(n @ 40..=47) => *bg = crate::display::VGA16[(n - 40) as usize],
                    Ok(n @ 100..=107) => *bg = crate::display::VGA16[(n - 100 + 8) as usize],
                    _ => {}
                }
                i += 1;
            }
        }
    }
}

/// The RGB of an xterm 256-colour index (16 system colours, 6x6x6 cube, grey ramp).
fn xterm256(n: u8) -> Rgba {
    match n {
        0..=15 => crate::display::VGA16[n as usize],
        16..=231 => {
            let i = n - 16;
            let v = |q: u8| if q == 0 { 0 } else { 55 + 40 * q };
            Rgba::rgb(v(i / 36), v((i / 6) % 6), v(i % 6))
        }
        _ => {
            let g = 8 + 10 * (n - 232);
            Rgba::rgb(g, g, g)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn relative_wallpaper_resolves_through_the_theme_dirs_in_order() {
        let root = std::env::temp_dir().join(format!("tetron-wp-resolve-{}", std::process::id()));
        let (user, data, cfg) = (root.join("user"), root.join("data"), root.join("cfg"));
        for d in [&user, &data, &cfg.join("wallpapers")] {
            std::fs::create_dir_all(d).unwrap();
        }
        std::fs::write(data.join("dl.jpg"), "x").unwrap();
        std::fs::write(user.join("both.jpg"), "x").unwrap();
        std::fs::write(data.join("both.jpg"), "x").unwrap();
        std::fs::write(cfg.join("wallpapers/old.jpg"), "x").unwrap();
        let dirs = [user.clone(), data.clone()];
        let r = |p: &str| resolve_relative(p, &dirs, Some(&cfg));
        assert_eq!(r("dl.jpg"), data.join("dl.jpg"), "found in the download dir");
        assert_eq!(r("both.jpg"), user.join("both.jpg"), "user dir first");
        assert_eq!(r("old.jpg"), cfg.join("wallpapers/old.jpg"), "legacy folder still works");
        assert_eq!(r("nope.jpg"), user.join("nope.jpg"), "best effort names the first dir");
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn source_recognises_none_black_and_hex() {
        assert_eq!(source(None), Source::Image);
        assert_eq!(source(Some("default")), Source::Image);
        assert_eq!(source(Some("forest.jpg")), Source::Image);
        assert_eq!(source(Some("none")), Source::Plain);
        assert_eq!(source(Some(" NONE ")), Source::Plain);
        assert_eq!(source(Some("black")), Source::Solid(Rgba::rgb(0, 0, 0)));
        assert_eq!(source(Some("\"#004040\"")), Source::Solid(Rgba::rgb(0, 0x40, 0x40)));
        assert_eq!(source(Some("#0A0b0C")), Source::Solid(Rgba::rgb(10, 11, 12)));
        // Malformed colours stay images, so the "not found" note shows them.
        assert_eq!(source(Some("#12345")), Source::Image);
        assert_eq!(source(Some("#12345678")), Source::Image);
        assert_eq!(source(Some("#gggggg")), Source::Image);
    }

    #[test]
    fn default_is_the_theme_colour_below_256_colours_but_a_chosen_file_is_drawn() {
        use crate::display::ColorDepth::*;
        for d in [Mono, Ansi16] {
            assert_eq!(draw_for(Some("default"), false, d), None, "{d:?}");
            assert_eq!(draw_for(None, false, d), None, "{d:?}");
            assert_eq!(draw_for(Some("forest.jpg"), false, d), Some(Source::Image), "{d:?}");
            assert_eq!(draw_for(Some("black"), false, d), Some(Source::Solid(Rgba::rgb(0, 0, 0))), "{d:?}");
        }
        for d in [Ansi256, TrueColor] {
            assert_eq!(draw_for(Some("default"), false, d), Some(Source::Image), "{d:?}");
        }
        assert_eq!(draw_for(Some("none"), false, TrueColor), None);
        // A picture a theme brings is the theme's default too: flat below 256 colours.
        assert_eq!(draw_for(Some("/t/dawn.jpg"), true, Ansi16), None);
        assert_eq!(draw_for(Some("/t/dawn.jpg"), true, Ansi256), Some(Source::Image));
    }

    #[test]
    fn solid_fills_every_cell_and_needs_no_chafa() {
        let c = Rgba::rgb(1, 2, 3);
        let buf = solid(c, 4, 2);
        assert_eq!((buf.get(0, 0).unwrap().bg, buf.get(3, 1).unwrap().bg), (c, c));
        assert_eq!(note(Some("black")), None);
        assert_eq!(note(Some("#102030")), None);
        assert_eq!(note(Some("none")), None);
    }

    #[test]
    fn sgr_parses_16_and_256_colour_forms() {
        let (mut fg, mut bg) = (Rgba::rgb(1, 1, 1), Rgba::rgb(1, 1, 1));
        let def = Rgba::rgb(0, 0, 0);
        let mut inv = false;
        apply_sgr("0;94;41", &mut fg, &mut bg, &mut inv, def);
        assert_eq!((fg, bg), (crate::display::VGA16[12], crate::display::VGA16[1]));
        apply_sgr("38;5;196;48;5;232", &mut fg, &mut bg, &mut inv, def);
        assert_eq!((fg, bg), (Rgba::rgb(255, 0, 0), Rgba::rgb(8, 8, 8)));
    }

    /// chafa's solid cell: inverse video plus a space, as it writes at a row
    /// start. It must come out in the foreground colour, not black.
    #[test]
    fn inverse_space_is_a_solid_foreground_cell() {
        let out = "\x1b[7m\x1b[38;2;6;9;50m \x1b[0m\x1b[38;2;1;2;3;48;2;4;5;6m\u{2580}";
        let buf = parse_ansi(out, 2, 1);
        assert_eq!(buf.get(0, 0).unwrap().bg, Rgba::rgb(6, 9, 50), "inverse space shows the fg colour");
        let c = buf.get(1, 0).unwrap();
        assert_eq!((c.fg, c.bg), (Rgba::rgb(1, 2, 3), Rgba::rgb(4, 5, 6)), "reset ends inverse");
    }

    #[test]
    fn embedded_pictures_are_small_jpegs() {
        for (name, b) in EMBEDDED {
            assert_eq!(&b[..2], &[0xFF, 0xD8], "{name}: JPEG magic");
            assert!(b.len() < 100 * 1024, "{name}: keep the embedded wallpaper small");
        }
        assert!(EMBEDDED.iter().any(|(n, _)| *n == FALLBACK_IMAGE));
    }

    #[test]
    fn default_follows_the_theme_and_falls_back_to_midnight() {
        assert_eq!(theme_image("gruvbox"), "gruvbox");
        assert_eq!(theme_image(" Windows "), "windows");
        assert_eq!(theme_image("pastoral"), "pastoral");
        assert_eq!(theme_image("light"), "pastoral", "the old name");
        assert_eq!(theme_image("midnight"), "midnight");
        for t in ["newmoon", "nord", "dracula", "some-file-theme", ""] {
            assert_eq!(theme_image(t), "midnight", "{t}");
        }
    }

    #[test]
    fn clean_spec_handles_pasted_forms() {
        assert_eq!(clean_spec("  \"/a b.jpg\" "), "/a b.jpg");
        assert_eq!(clean_spec("'/a b.jpg'"), "/a b.jpg");
        assert_eq!(clean_spec("file:///home/u/My%20Pics/%C3%A9.png"), "/home/u/My Pics/\u{e9}.png");
        assert_eq!(clean_spec("file://localhost/x.png"), "/x.png");
        assert_eq!(clean_spec("$HOME/p.png"), "~/p.png");
        assert_eq!(clean_spec("${HOME}/p.png"), "~/p.png");
        assert_eq!(clean_spec("$HOMEDIR/p.png"), "$HOMEDIR/p.png", "only the whole variable");
        assert_eq!(clean_spec("100%.png"), "100%.png");
    }

    #[test]
    fn default_keyword_empty_and_unset_mean_builtin() {
        for v in [None, Some(""), Some("  "), Some("default"), Some("\"default\"")] {
            assert!(is_default(v), "{v:?}");
        }
        assert!(!is_default(Some("wallpaper.jpg")));
        assert!(!is_default(Some("/x/default")));
    }

    #[test]
    fn missing_file_reports_missing_but_legacy_name_does_not() {
        let c = choose(Some("/no/such/dir/pic.png"), "midnight");
        assert_eq!(c.missing, Some(PathBuf::from("/no/such/dir/pic.png")));
        // Old configs naming the former default never report it missing.
        assert!(choose(Some("wallpaper.jpg"), "midnight").missing.is_none());
    }

    #[test]
    fn absolute_path_passes_through() {
        assert_eq!(resolve_path("/tmp/wall.png"), PathBuf::from("/tmp/wall.png"));
        // whitespace is trimmed
        assert_eq!(resolve_path("  /tmp/wall.png  "), PathBuf::from("/tmp/wall.png"));
    }

    #[test]
    fn tilde_expands_to_home() {
        if let Some(home) = dirs::home_dir() {
            assert_eq!(resolve_path("~/pic.png"), home.join("pic.png"));
        }
    }

    #[test]
    fn relative_resolves_under_config_dir() {
        // A bare filename resolves beneath the config dir, never the cwd.
        if let Some(cfg) = crate::config::config_dir() {
            let got = resolve_path("pic.png");
            assert!(got.starts_with(&cfg), "{got:?} should be under {cfg:?}");
            assert!(got.ends_with("pic.png"));
        }
    }
}
