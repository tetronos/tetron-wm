//! Chrome glyphs (window buttons, menubar toggles, markers) per `icon_style`.
//! Together with `iconset.rs` (file-type and status icons) this is the one
//! place glyph choices live; widgets ask [`get`] / [`ch`] and never embed a
//! symbol themselves (a source-scan test enforces it).
//!
//! Under `ascii` every glyph comes from [`CONSOLE_SAFE`]: the characters present
//! in the Linux kernel console fonts (Lat15/Uni2/Uni3 Fixed, Terminus, VGA) and
//! DejaVu Sans Mono, which kmscon and most terminals fall back to. The set is
//! computed by `scripts/fontcov.py`; a source-scan test keeps stray fancy
//! glyphs from creeping back in. `unicode` uses [`UNICODE_SAFE`], richer
//! symbols all present in DejaVu Sans Mono (kmscon's default font and the
//! usual GUI fallback). `nerd` and `emoji` keep the same symbols as `unicode`
//! for chrome (they assume a GUI terminal or a patched font).

use crate::iconset::{self, IconStyle};

/// Non-ASCII characters every console font tested can draw (plus all of ASCII).
/// `≡` is included: all five console fonts have it (checked with `psfgettable`).
pub const CONSOLE_SAFE: &str =
    "§±²³µ·¹×–—“”•…←↑→↓−≤≥⊙─│╭╮╯╰█■▮▲▶▸▼▾◂●☐✗¶≈¤¦¬°«»‹›░▒╱╲┌┐└┘├┤┬┴┼═║♦‰†‡∞≠≡";

/// [`CONSOLE_SAFE`] plus the extra single-cell symbols the `unicode` tier uses,
/// each checked present in DejaVu Sans Mono (fontTools cmap).
/// Absent there, so never used by `unicode`: ⓘ ℹ ⏻ ⎘ ⍾ ☰ ᛒ.
pub const UNICODE_SAFE: &str = concat!(
    "§±²³µ·¹×–—“”•…←↑→↓−≤≥⊙─│╭╮╯╰█■▮▲▶▸▼▾◂●☐✗¶≈¤¦¬°«»‹›░▒╱╲┌┐└┘├┤┬┴┼═║♦‰†‡∞≠≡",
    "✦⊞▦✕▢❐★☆⏎⌕⌘○✓▣‖▯⌨▤⚙◇◉♪♫▫↯⇄⚠◢",
);

/// Upper half block, for image art only (two pixels per cell), never chrome.
/// The kernel VT fonts lack it, so callers use coloured spaces on a VT; DejaVu
/// (kmscon) and GUI terminals have it, as the chafa wallpaper already assumes.
pub const HALF_BLOCK_UPPER: char = '\u{2580}';

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Glyph {
    Assistant,
    ModeDesktop,
    ModeSimple,
    Close,
    Maximize,
    Restore,
    Pinned,
    Unpinned,
    Newline,
    Search,
    Prompt,
    Off,
    Check,
    Revoke,
    Paused,
    Info,
    BarOn,
    BarOff,
    Terminal,
    Plus,
    /// The window's bottom right corner: the resize grip.
    ResizeGrip,
}

pub const ALL: [Glyph; 21] = [
    Glyph::Assistant, Glyph::ModeDesktop, Glyph::ModeSimple, Glyph::Close,
    Glyph::Maximize, Glyph::Restore, Glyph::Pinned, Glyph::Unpinned,
    Glyph::Newline, Glyph::Search, Glyph::Prompt, Glyph::Off, Glyph::Check,
    Glyph::Revoke, Glyph::Paused, Glyph::Info, Glyph::BarOn, Glyph::BarOff,
    Glyph::Terminal, Glyph::Plus, Glyph::ResizeGrip,
];

/// The glyph in a specific `style`.
pub fn glyph_for(g: Glyph, style: IconStyle) -> &'static str {
    use Glyph::*;
    match style {
        IconStyle::Ascii | IconStyle::Auto => match g {
            Assistant => "*",
            ModeDesktop => "\u{25A0}", // ■
            ModeSimple => "\u{2588}",  // █
            Close => "\u{00D7}",       // ×
            Maximize => "\u{25B2}",    // ▲
            Restore => "\u{25BC}",     // ▼
            Pinned => "*",
            Unpinned => "\u{00B7}",    // ·
            Newline => "\u{00B6}",     // ¶
            Search => "/",
            Prompt => ">",
            Off => "-",
            Check => "+",
            Revoke => "\u{25A0}",      // ■
            Paused => "=",
            Info => "\u{2261}",       // ≡ (a plain `?` reads as a missing glyph)
            BarOn => "\u{25AE}",       // ▮
            BarOff => "\u{00B7}",      // ·
            Terminal => ">",
            Plus => "+",
            ResizeGrip => "\u{2518}", // ┘ (◢ is missing from the console fonts)
        },
        // ⓘ is missing from DejaVu Sans Mono (kmscon would draw tofu).
        IconStyle::Unicode if g == Info => "\u{2261}", // ≡
        _ => match g {
            Assistant => "\u{2726}",   // ✦
            ModeDesktop => "\u{229E}", // ⊞
            ModeSimple => "\u{25A6}",  // ▦
            Close => "\u{2715}",       // ✕
            Maximize => "\u{25A2}",    // ▢
            Restore => "\u{2750}",     // ❐
            Pinned => "\u{2605}",      // ★
            Unpinned => "\u{2606}",    // ☆
            Newline => "\u{23CE}",     // ⏎
            Search => "\u{2315}",      // ⌕
            Prompt => "\u{2318}",      // ⌘
            Off => "\u{25CB}",         // ○
            Check => "\u{2713}",       // ✓
            Revoke => "\u{25A3}",      // ▣
            Paused => "\u{2016}",      // ‖
            Info => "\u{24D8}",        // ⓘ
            BarOn => "\u{25AE}",       // ▮
            BarOff => "\u{25AF}",      // ▯
            Terminal => "\u{2328}",    // ⌨
            Plus => "+",
            ResizeGrip => "\u{25E2}",  // ◢
        },
    }
}

/// The glyph in the active style.
pub fn get(g: Glyph) -> &'static str {
    glyph_for(g, iconset::style())
}

/// First char of [`get`] (for cell-based drawing).
pub fn ch(g: Glyph) -> char {
    get(g).chars().next().unwrap_or(' ')
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::iconset::{glyph_for as file_glyph, status_for, Status};
    use crate::openwith::Role;

    fn safe(c: char) -> bool {
        c.is_ascii() || CONSOLE_SAFE.contains(c)
    }

    #[test]
    fn ascii_chrome_is_console_safe_and_one_cell() {
        for g in ALL {
            let s = glyph_for(g, IconStyle::Ascii);
            assert_eq!(s.chars().count(), 1, "{g:?}");
            assert!(s.chars().all(safe), "{g:?} {s:?}");
        }
    }

    #[test]
    fn every_glyph_exists_in_every_style() {
        for style in [IconStyle::Auto, IconStyle::Ascii, IconStyle::Unicode, IconStyle::Nerd, IconStyle::Emoji] {
            for g in ALL {
                assert!(!glyph_for(g, style).is_empty(), "{g:?}/{style:?}");
            }
        }
    }

    #[test]
    fn unicode_tier_is_dejavu_safe_and_one_cell() {
        let ok = |c: char| c.is_ascii() || UNICODE_SAFE.contains(c);
        for g in ALL {
            let s = glyph_for(g, IconStyle::Unicode);
            assert_eq!(s.chars().count(), 1, "{g:?}");
            assert!(s.chars().all(ok), "{g:?} {s:?}");
        }
        for role in [
            Role::Directory, Role::Image, Role::Video, Role::Audio, Role::Text,
            Role::Code, Role::Archive, Role::Pdf, Role::Executable, Role::Other,
        ] {
            assert!(ok(file_glyph(role, IconStyle::Unicode)), "{role:?}");
        }
        for s in [
            Status::Cpu, Status::Mem, Status::BatteryCharging, Status::Battery,
            Status::VolMute, Status::VolLow, Status::VolHigh, Status::Bluetooth,
            Status::Net, Status::Bell, Status::Clipboard,
        ] {
            assert!(ok(status_for(s, IconStyle::Unicode)), "{s:?}");
        }
        assert!(crate::iconset::warning_label(IconStyle::Unicode).chars().all(ok));
    }

    #[test]
    fn ascii_file_and_status_icons_are_console_safe() {
        for role in [
            Role::Directory, Role::Image, Role::Video, Role::Audio, Role::Text,
            Role::Code, Role::Archive, Role::Pdf, Role::Executable, Role::Other,
        ] {
            assert!(safe(file_glyph(role, IconStyle::Ascii)), "{role:?}");
        }
        for s in [
            Status::Cpu, Status::Mem, Status::BatteryCharging, Status::Battery,
            Status::VolMute, Status::VolLow, Status::VolHigh, Status::Bluetooth,
            Status::Net, Status::Bell, Status::Clipboard,
        ] {
            assert!(safe(status_for(s, IconStyle::Ascii)), "{s:?}");
        }
    }

    /// Regression guard: production code outside the glyph tables must only
    /// contain console-safe characters (comments and `#[cfg(test)]` tails are
    /// exempt). Use `glyphs::get` / `iconset` for anything fancier.
    #[test]
    fn source_has_no_console_unsafe_glyphs() {
        fn walk(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
            for e in std::fs::read_dir(dir).unwrap().flatten() {
                let p = e.path();
                if p.is_dir() { walk(&p, out) } else if p.extension().is_some_and(|x| x == "rs") { out.push(p) }
            }
        }
        let mut files = vec![];
        walk(&std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src"), &mut files);
        let mut bad = vec![];
        for f in files {
            let name = f.file_name().unwrap().to_string_lossy().to_string();
            // catalog.rs matches the recipe data suffix "⚠️" (input, never drawn).
            if matches!(name.as_str(), "glyphs.rs" | "iconset.rs" | "catalog.rs") { continue; }
            let text = std::fs::read_to_string(&f).unwrap();
            for (n, line) in text.lines().enumerate() {
                if line.trim_start().starts_with("#[cfg(test)]") { break; }
                let code = match line.trim_start() {
                    l if l.starts_with("//") => continue,
                    _ => line,
                };
                let code = code.split(" // ").next().unwrap_or(code);
                let mut esc = code.to_string();
                while let Some(i) = esc.find("\\u{") {
                    let Some(j) = esc[i..].find('}') else { break };
                    let hex = &esc[i + 3..i + j];
                    let ch = u32::from_str_radix(hex, 16).ok().and_then(char::from_u32).unwrap_or('?');
                    esc.replace_range(i..i + j + 1, &ch.to_string());
                }
                for c in esc.chars() {
                    if !safe(c) && c != '\u{FE0F}' {
                        bad.push(format!("{name}:{} {c:?}", n + 1));
                    }
                }
            }
        }
        assert!(bad.is_empty(), "console-unsafe glyphs (use glyphs::get): {bad:#?}");
    }
}
