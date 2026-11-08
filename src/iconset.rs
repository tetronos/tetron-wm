//! File-type icon glyphs for the list view and the no-graphics fallback.
//!
//! The graphical icons (`icons.rs` PNGs via the Kitty graphics protocol) are
//! used when the terminal supports them. Where it does not -- kmscon, a bare
//! Linux VT, plain SSH to a non-graphics terminal -- the file manager and the
//! desktop fall back to a single glyph per file type. The `icon_style` config
//! key picks which glyph set, set process-globally on the daemon (like the
//! theme). This module and `glyphs.rs` are the only places glyph choices live.
//!   - `auto` (default): `ascii` on a bare kernel VT, `unicode` everywhere
//!     else. Resolved from the attached client's display (`crate::display`)
//!     and re-resolved when another client attaches.
//!   - `ascii`: only characters in `glyphs::CONSOLE_SAFE` (the kernel console
//!     PSF fonts and DejaVu Sans Mono). Renders everywhere.
//!   - `unicode`: richer single-cell symbols from `glyphs::UNICODE_SAFE`, all
//!     present in DejaVu Sans Mono (kmscon's font and the usual GUI fallback).
//!   - `nerd`:  Nerd Font file-type glyphs -- the nicest console result, but
//!     Private-Use-Area, so tofu without a Nerd Font installed.
//!     tetron-os ships one and sets this.
//!   - `emoji`: color emoji -- nice on a GUI terminal, tofu on a console.

use crate::openwith::Role;
use serde::{Deserialize, Serialize};
use std::sync::{OnceLock, RwLock};

/// Which glyph set the file manager / desktop use for the non-graphics icon.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum IconStyle {
    /// `unicode` unless the client is a bare VT, then `ascii`. The default.
    /// Only a preference: [`style`] never returns it.
    #[default]
    Auto,
    /// Console-safe shapes -- renders in every font, including kernel PSFs.
    Ascii,
    /// Richer symbols present in DejaVu Sans Mono (kmscon, GUI terminals).
    Unicode,
    /// Nerd Font file-type glyphs -- needs a Nerd Font (tetron-os ships one).
    Nerd,
    /// Color emoji -- GUI terminals only.
    Emoji,
}

/// The glyph for `role` in a specific `style`.
pub fn glyph_for(role: Role, style: IconStyle) -> char {
    match style {
        IconStyle::Unicode => match role {
            // Only glyphs in `glyphs::UNICODE_SAFE`.
            Role::Directory => '\u{25A4}',  // ▤ square with lines
            Role::Executable => '\u{2699}', // ⚙ gear
            Role::Text => '\u{2610}',       // ☐ ballot box
            Role::Code => '\u{25C7}',       // ◇ diamond
            Role::Image => '\u{25C9}',      // ◉ fisheye
            Role::Audio => '\u{266A}',      // ♪ note
            Role::Video => '\u{25B6}',      // ▶ play
            Role::Archive => '\u{25A3}',    // ▣ boxed square
            Role::Pdf => '\u{25A6}',        // ▦ crosshatch
            Role::Other => '\u{25AB}',      // ▫ small square
        },
        IconStyle::Ascii | IconStyle::Auto => match role {
            // Only glyphs in `glyphs::CONSOLE_SAFE` (kernel console fonts have few shapes).
            Role::Directory => '\u{25A0}',  // ■ filled square
            Role::Executable => '\u{25CF}', // ● filled circle
            Role::Text => '\u{2610}',       // ☐ ballot box
            Role::Code => '\u{25B8}',       // ▸ small triangle
            Role::Image => '\u{25B2}',      // ▲ up triangle
            Role::Audio => '\u{25AE}',      // ▮ vertical bar
            Role::Video => '\u{25B6}',      // ▶ play
            Role::Archive => '\u{25BC}',    // ▼ down triangle
            Role::Pdf => '\u{2588}',        // █ full block
            Role::Other => '\u{00B7}',      // · middle dot
        },
        IconStyle::Nerd => match role {
            Role::Directory => '\u{F07B}',  //  folder
            Role::Executable => '\u{F120}', //  terminal
            Role::Text => '\u{F0F6}',       //  file-text
            Role::Code => '\u{F1C9}',       //  file-code
            Role::Image => '\u{F1C5}',      //  file-image
            Role::Audio => '\u{F1C7}',      //  file-audio
            Role::Video => '\u{F1C8}',      //  file-video
            Role::Archive => '\u{F1C6}',    //  file-archive
            Role::Pdf => '\u{F1C1}',        //  file-pdf
            Role::Other => '\u{F016}',      //  file
        },
        IconStyle::Emoji => match role {
            Role::Directory => '\u{1F4C1}',  // 📁
            Role::Image => '\u{1F5BC}',      // 🖼
            Role::Audio => '\u{1F3B5}',      // 🎵
            Role::Video => '\u{1F3AC}',      // 🎬
            Role::Archive => '\u{1F4E6}',    // 📦
            Role::Pdf => '\u{1F4D5}',        // 📕
            Role::Executable => '\u{2699}',  // ⚙
            _ => '\u{1F4C4}',                // 📄
        },
    }
}

/// Status-bar / tray indicator icons. Styled the same way as file-type glyphs so
/// the whole UI follows one `icon_style` (no stray emoji under ascii/nerd). Wifi
/// uses `system::bars_glyph` instead — it is a level meter, not a single icon.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    Cpu,
    Mem,
    BatteryCharging,
    Battery,
    VolMute,
    VolLow,
    VolHigh,
    Bluetooth,
    Net,
    Bell,
    Clipboard,
    Brightness,
}

/// The glyph for a status indicator in a specific `style`.
pub fn status_for(s: Status, style: IconStyle) -> char {
    use Status::*;
    match style {
        // Only glyphs in `glyphs::UNICODE_SAFE`.
        IconStyle::Unicode => match s {
            Cpu => '\u{2699}',             // ⚙
            Mem => '\u{25A4}',             // ▤
            BatteryCharging => '\u{21AF}', // ↯
            Battery => '\u{25AE}',         // ▮
            VolMute => '\u{2715}',         // ✕
            VolLow => '\u{266A}',          // ♪
            VolHigh => '\u{266B}',         // ♫
            Bluetooth => 'B',
            Net => '\u{21C4}',             // ⇄
            Bell => '\u{2022}',            // •
            Clipboard => '\u{00A7}',       // §
            Brightness => '\u{00A4}',      // ¤ (a small sun)
        },
        // BMP symbols — render in essentially every console/monospace font.
        IconStyle::Ascii | IconStyle::Auto => match s {
            Cpu => '\u{2299}',             // ⊙
            Mem => '\u{2592}',             // ▒
            BatteryCharging => '\u{25B2}', // ▲
            Battery => '\u{25AE}',         // ▮
            VolMute => '\u{2717}',         // ✗
            VolLow => '\u{25B8}',          // ▸
            VolHigh => '\u{25B6}',         // ▶
            Bluetooth => 'B',
            Net => '\u{2193}',             // ↓
            Bell => '\u{2022}',            // •
            Clipboard => '\u{00A7}',       // §
            Brightness => '\u{00A4}',      // ¤ (a small sun)
        },
        // Nerd Font (FontAwesome) glyphs — needs a Nerd Font installed.
        IconStyle::Nerd => match s {
            Cpu => '\u{F2DB}',             //  microchip
            Mem => '\u{F1C0}',             //  database
            BatteryCharging => '\u{F0E7}', //  bolt
            Battery => '\u{F240}',         //  battery-full
            VolMute => '\u{F026}',         //  volume-off
            VolLow => '\u{F027}',          //  volume-down
            VolHigh => '\u{F028}',         //  volume-up
            Bluetooth => '\u{F293}',       //  bluetooth
            Net => '\u{F0EC}',             //  exchange
            Bell => '\u{F0F3}',            //  bell
            Clipboard => '\u{F0EA}',       //  paste
            Brightness => '\u{F185}',      //  sun-o
        },
        IconStyle::Emoji => match s {
            Cpu => '\u{1F5A5}',            // 🖥
            Mem => '\u{1F4BE}',            // 💾
            BatteryCharging => '\u{26A1}', // ⚡
            Battery => '\u{1F50B}',        // 🔋
            VolMute => '\u{1F507}',        // 🔇
            VolLow => '\u{1F509}',         // 🔉
            VolHigh => '\u{1F50A}',        // 🔊
            Bluetooth => '\u{224B}',       // ≋ (no bluetooth emoji; the "bt" label carries it)
            Net => '\u{1F517}',            // 🔗
            Bell => '\u{1F514}',           // 🔔
            Clipboard => '\u{1F4CB}',      // 📋
            Brightness => '\u{1F506}',     // 🔆
        },
    }
}

/// A short warning marker for labels (e.g. launcher variant entries) in `style`:
/// emoji only when the style is `emoji`, so ascii consoles never see tofu.
pub fn warning_label(style: IconStyle) -> &'static str {
    match style {
        IconStyle::Ascii | IconStyle::Auto => "[!]",
        IconStyle::Unicode => "\u{26A0}",
        IconStyle::Nerd => "\u{F071}",
        IconStyle::Emoji => "\u{26A0}\u{FE0F}",
    }
}

/// The status glyph in the active style.
pub fn status(s: Status) -> char {
    status_for(s, style())
}

/// (configured preference, resolved style). The preference may be `Auto`; the
/// resolved style never is.
fn slot() -> &'static RwLock<(IconStyle, IconStyle)> {
    static STYLE: OnceLock<RwLock<(IconStyle, IconStyle)>> = OnceLock::new();
    STYLE.get_or_init(|| RwLock::new((IconStyle::Ascii, IconStyle::Ascii)))
}

/// The concrete style for preference `pref` on display `caps`: `auto` is
/// `ascii` on a bare VT (PSF fonts) and `unicode` elsewhere.
pub fn resolve(pref: IconStyle, caps: crate::display::ClientCaps) -> IconStyle {
    match pref {
        IconStyle::Auto if caps.vt => IconStyle::Ascii,
        IconStyle::Auto => IconStyle::Unicode,
        s => s,
    }
}

/// The warning label in the active style.
pub fn warning() -> &'static str {
    warning_label(style())
}

/// The active (resolved) icon style. Never `Auto`.
pub fn style() -> IconStyle {
    slot().read().unwrap().1
}

/// Set the configured icon style (daemon startup, config change) and resolve
/// it against the attached client's display.
pub fn set(pref: IconStyle) {
    *slot().write().unwrap() = (pref, resolve(pref, crate::display::get()));
}

/// Re-resolve the configured style after the client's display changed.
/// Returns whether the resolved style changed.
pub fn refresh() -> bool {
    let mut s = slot().write().unwrap();
    let next = resolve(s.0, crate::display::get());
    let changed = next != s.1;
    s.1 = next;
    changed
}

/// The glyph for `role` in the active style.
pub fn glyph(role: Role) -> char {
    glyph_for(role, style())
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALL_STYLES: [IconStyle; 5] =
        [IconStyle::Auto, IconStyle::Ascii, IconStyle::Unicode, IconStyle::Nerd, IconStyle::Emoji];

    #[test]
    fn auto_is_ascii_on_a_vt_and_unicode_elsewhere() {
        use crate::display::ClientCaps;
        let vt = ClientCaps { vt: true, ..Default::default() };
        assert_eq!(resolve(IconStyle::Auto, vt), IconStyle::Ascii);
        assert_eq!(resolve(IconStyle::Auto, ClientCaps::default()), IconStyle::Unicode);
        // An explicit choice is never overridden.
        assert_eq!(resolve(IconStyle::Nerd, vt), IconStyle::Nerd);
        assert_eq!(resolve(IconStyle::Ascii, ClientCaps::default()), IconStyle::Ascii);
    }

    #[test]
    fn every_role_has_a_glyph_in_every_style() {
        for style in ALL_STYLES {
            for role in [
                Role::Directory, Role::Image, Role::Video, Role::Audio, Role::Text,
                Role::Code, Role::Archive, Role::Pdf, Role::Executable, Role::Other,
            ] {
                let g = glyph_for(role, style);
                assert_ne!(g, '\0', "{style:?}/{role:?}");
            }
        }
    }

    #[test]
    fn ascii_glyphs_are_bmp() {
        // The point of the default style: no astral-plane / PUA codepoints.
        for role in [Role::Directory, Role::Other, Role::Executable, Role::Audio, Role::Video] {
            assert!((glyph_for(role, IconStyle::Ascii) as u32) <= 0xFFFF);
        }
    }

    const STATUSES: [Status; 10] = [
        Status::Cpu, Status::Mem, Status::BatteryCharging, Status::Battery,
        Status::VolMute, Status::VolLow, Status::VolHigh, Status::Bluetooth,
        Status::Net, Status::Bell,
    ];

    #[test]
    fn every_status_has_a_glyph_in_every_style() {
        for style in ALL_STYLES {
            for s in STATUSES {
                assert_ne!(status_for(s, style), '\0', "{style:?}/{s:?}");
            }
        }
    }

    #[test]
    fn ascii_status_glyphs_are_bmp_no_emoji() {
        // Consistency fix: the tray must not use astral-plane emoji under ascii.
        for s in STATUSES {
            assert!((status_for(s, IconStyle::Ascii) as u32) <= 0xFFFF, "{s:?}");
        }
    }
}
