//! The right-click menu shared by the desktop and the file manager: which
//! actions a target offers ([`actions_for`], [`open_with_actions`]) and one menu
//! widget ([`ContextMenu`]) that both surfaces draw and hit-test with the same
//! geometry. A new action is added here once and appears in both places.

use std::path::Path;

use crate::buffer::CellBuffer;
use crate::cell::Cell;
use crate::geometry::{Point, Rect};
use crate::openwith::Role;

/// Where the menu was opened.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Surface {
    Desktop,
    Files,
}

/// What the menu is about.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Target {
    /// A file or folder (`pinned`: a desktop shortcut, not a real file).
    Entry { is_dir: bool, pinned: bool, role: Role },
    /// Empty space (the desktop background).
    Background,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MenuAction {
    Open,
    /// Replace the menu with the apps that can open the entry.
    OpenWith,
    /// Open the entry with this handler (`@image` = the built-in viewer).
    OpenWithApp(String),
    Rename,
    Copy,
    Cut,
    /// Paste the copied or cut files here (the folder shown, or the desktop).
    Paste,
    /// Paste the copied or cut files into the folder under the click.
    PasteInto,
    /// Delete everything in the Trash, after a confirmation.
    EmptyTrash,
    Trash,
    GetInfo,
    Unpin,
    NewFolder,
    CleanUp,
    /// Hand the screen to fbi / fim (plain console only), with the folder's images.
    ViewFullscreen,
    SetWallpaper,
    CopyPath,
    /// A shell in the folder (the entry's folder for a file, the folder itself).
    OpenTerminalHere,
    /// Hand the screen to mpv / cvlc for a video (plain console only).
    PlayFullscreen,
    ExtractHere,
    ListContents,
    /// Cast a video, audio or image file to a Chromecast with catt, in a window.
    CastTo,
    /// Replace the menu with the saved Systems to copy the entry to.
    SendTo,
    /// Copy the entry to this Systems entry (by name) with scp.
    SendToSystem(String),
    /// Replace the menu with the plain archive formats ([`pack_actions`]).
    Compress,
    /// Replace the menu with the password formats ([`pack_actions`]).
    Encrypt,
    /// Pack the entry into this format beside it, in a window.
    PackAs(Pack),
    // Window (title bar) actions.
    Minimize,
    Maximize,
    MoveWindow,
    RenameWindow,
    CloseWindow,
}

impl MenuAction {
    pub fn label(&self) -> String {
        match self {
            MenuAction::Open => "Open".into(),
            MenuAction::OpenWith => "Open with\u{2026}".into(),
            MenuAction::OpenWithApp(cmd) if cmd == "@image" => "Image viewer".into(),
            MenuAction::OpenWithApp(cmd) if cmd == "@pdf" => "PDF viewer".into(),
            MenuAction::OpenWithApp(cmd) if cmd == "@pdftext" => "Text (pdftotext)".into(),
            MenuAction::OpenWithApp(cmd) if cmd == "@timg" => "timg".into(),
            MenuAction::OpenWithApp(cmd) => cmd.clone(),
            MenuAction::Rename => "Rename".into(),
            MenuAction::Copy => "Copy".into(),
            MenuAction::Cut => "Cut".into(),
            MenuAction::Paste => "Paste".into(),
            MenuAction::PasteInto => "Paste into folder".into(),
            MenuAction::EmptyTrash => "Empty Trash\u{2026}".into(),
            MenuAction::Trash => "Move to Trash".into(),
            MenuAction::GetInfo => "Get Info".into(),
            MenuAction::Unpin => "Unpin".into(),
            MenuAction::NewFolder => "New Folder".into(),
            MenuAction::CleanUp => "Clean Up".into(),
            MenuAction::ViewFullscreen => "View full screen".into(),
            MenuAction::SetWallpaper => "Set as wallpaper".into(),
            MenuAction::CopyPath => "Copy path".into(),
            MenuAction::OpenTerminalHere => "Open terminal here".into(),
            MenuAction::PlayFullscreen => "Play full screen".into(),
            MenuAction::ExtractHere => "Extract here".into(),
            MenuAction::ListContents => "List contents".into(),
            MenuAction::CastTo => "Cast to Chromecast".into(),
            MenuAction::SendTo => "Send to\u{2026}".into(),
            MenuAction::SendToSystem(name) => name.clone(),
            MenuAction::Compress => "Compress\u{2026}".into(),
            MenuAction::Encrypt => "Encrypt\u{2026}".into(),
            MenuAction::PackAs(p) => p.label().into(),
            MenuAction::Minimize => "Minimize".into(),
            MenuAction::Maximize => "Maximize".into(),
            MenuAction::MoveWindow => "Move".into(),
            MenuAction::RenameWindow => "Rename".into(),
            MenuAction::CloseWindow => "Close".into(),
        }
    }
}

/// What the client and machine can do, for the actions that depend on it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Abilities {
    /// A route to show images full screen exists (see `fullscreen::plan`): "View full screen".
    pub fullscreen_image: bool,
    /// A route to play video full screen exists: "Play full screen".
    pub fullscreen_video: bool,
    /// A route to show a PDF full screen exists: "View full screen".
    pub fullscreen_pdf: bool,
    /// Saved Systems exist: "Send to…".
    pub send: bool,
    /// catt is installed: "Cast to Chromecast" for video, audio and images.
    pub cast: bool,
    /// tar is installed and the entry is on this machine: "Compress…".
    pub compress: bool,
    /// 7z, age or gpg is installed and the entry is on this machine: "Encrypt…".
    pub encrypt: bool,
    /// Files were copied or cut on this machine: "Paste".
    pub paste: bool,
    /// The Trash holds something: "Empty Trash…" on the background.
    pub trash: bool,
}

impl Abilities {
    /// For the attached client and this machine.
    pub fn current() -> Self {
        use crate::fullscreen::{offered, Kind};
        let caps = crate::display::get();
        let have = crate::catalog::is_installed;
        Abilities {
            fullscreen_image: offered(Kind::Images, &caps, &have),
            fullscreen_video: offered(Kind::Video, &caps, &have),
            fullscreen_pdf: offered(Kind::Pdf, &caps, &have),
            send: !crate::systems::load().is_empty() && have("scp"),
            cast: have("catt"),
            compress: have("tar"),
            encrypt: PackTools::current().choices(true).next().is_some(),
            paste: crate::fileops::clip_get().is_some(),
            trash: crate::fileops::trash_count() > 0,
        }
    }

    /// Without the local-only actions, for a folder on a remote system.
    pub fn remote(self) -> Self {
        Abilities { compress: false, encrypt: false, paste: false, trash: false, ..self }
    }
}

/// The actions `target` offers on `surface`, top to bottom.
pub fn actions_for(surface: Surface, target: Target, can: Abilities) -> Vec<MenuAction> {
    use MenuAction as A;
    match (surface, target) {
        (Surface::Desktop, Target::Entry { pinned: true, .. }) => vec![A::Open, A::Unpin],
        (_, Target::Entry { is_dir, role, .. }) => {
            let mut v = vec![A::Open];
            if !is_dir {
                v.push(A::OpenWith);
            }
            match role {
                Role::Image => {
                    if can.fullscreen_image {
                        v.push(A::ViewFullscreen);
                    }
                    v.push(A::SetWallpaper);
                }
                Role::Video if can.fullscreen_video => v.push(A::PlayFullscreen),
                Role::Pdf if can.fullscreen_pdf => v.push(A::ViewFullscreen),
                Role::Archive => v.extend([A::ExtractHere, A::ListContents]),
                _ => {}
            }
            if can.cast && !is_dir && matches!(role, Role::Video | Role::Audio | Role::Image) {
                v.push(A::CastTo);
            }
            v.extend([A::Rename, A::Copy, A::Cut]);
            if is_dir && can.paste {
                v.push(A::PasteInto);
            }
            if can.compress {
                v.push(A::Compress);
            }
            if can.encrypt {
                v.push(A::Encrypt);
            }
            v.extend([A::Trash, A::CopyPath, A::OpenTerminalHere, A::GetInfo]);
            if can.send {
                v.push(A::SendTo);
            }
            v
        }
        (_, Target::Background) => {
            let mut v = Vec::new();
            if can.paste {
                v.push(A::Paste);
            }
            v.extend([A::NewFolder, A::OpenTerminalHere]);
            if surface == Surface::Desktop {
                v.push(A::CleanUp);
            }
            if can.trash {
                v.push(A::EmptyTrash);
            }
            v
        }
    }
}

/// The title bar menu of a window.
pub fn window_actions() -> Vec<MenuAction> {
    use MenuAction as A;
    vec![A::Minimize, A::Maximize, A::MoveWindow, A::RenameWindow, A::CloseWindow]
}

/// The "Send to…" choices: one per saved Systems entry.
pub fn send_to_actions() -> Vec<MenuAction> {
    crate::systems::load().into_iter().map(|s| MenuAction::SendToSystem(s.name)).collect()
}

/// The shell command that casts `file` to a Chromecast with catt. `device` is
/// catt's `-d` value (a name or an IP); empty means catt's default device.
/// catt serves the file itself and keeps running until playback ends, so this
/// belongs in a window, where closing it stops the cast.
pub fn cast_command(file: &Path, device: &str) -> String {
    let q = |s: &str| crate::systems::sh_quote(s);
    let device = device.trim();
    let dev = if device.is_empty() { String::new() } else { format!("-d {} ", q(device)) };
    format!("catt {dev}cast {}", q(&file.to_string_lossy()))
}

/// Prefix for a script with pipelines: report the first failing stage where
/// the shell knows `pipefail` (bash, busybox, recent dash), else the last.
const PIPEFAIL: &str = "(set -o pipefail) 2>/dev/null && set -o pipefail; ";

/// gpg asks for the passphrase on the window's tty (not a desktop pinentry)
/// and does not cache it in the agent. `GPG_TTY` is set before the pipeline:
/// inside it, `tty` would see the pipe.
const GPG: &str = "gpg --pinentry-mode loopback --no-symkey-cache";
const GPG_TTY: &str = "export GPG_TTY=$(tty); ";

/// The shell command that extracts (`list` false) or lists `archive` into
/// or from `dir`, chosen by its name; `None` for an unknown kind.
pub fn archive_command(archive: &Path, dir: &Path, list: bool) -> Option<String> {
    archive_command_with(archive, dir, list, PackTools::current().sevenz)
}

/// [`archive_command`] with the 7-Zip binary given (`None`: not installed).
/// Encrypted files decrypt here: `.tar.gz.gpg` / `.tar.gz.age` straight into
/// `dir`, a single `x.gpg` / `x.age` to `x` beside it.
fn archive_command_with(archive: &Path, dir: &Path, list: bool, sevenz: Option<&str>) -> Option<String> {
    let q = |p: &Path| crate::systems::sh_quote(&p.to_string_lossy());
    let (a, d) = (q(archive), q(dir));
    let name = archive.file_name()?.to_string_lossy().to_ascii_lowercase();
    let tar = [".tar", ".tar.gz", ".tgz", ".tar.xz", ".txz", ".tar.bz2", ".tbz2", ".tar.zst"];
    let decrypt = if name.ends_with(".gpg") || name.ends_with(".pgp") {
        Some((GPG_TTY, format!("{GPG} -d")))
    } else if name.ends_with(".age") {
        Some(("", "age -d".to_string()))
    } else {
        None
    };
    if let Some((env, dec)) = decrypt {
        let inner = &name[..name.len() - 4];
        if tar.iter().any(|e| inner.ends_with(e)) {
            // tar only detects the compression of a file, not of a pipe.
            let z = [("gz", "-z "), ("xz", "-J "), ("bz2", "-j "), ("zst", "--zstd ")]
                .iter()
                .find(|(e, _)| inner.ends_with(e))
                .map_or("", |(_, f)| f);
            let t = if list { format!("{z}-tvf -") } else { format!("{z}-xf - -C {d}") };
            return Some(format!("{PIPEFAIL}{env}{dec} {a} | tar {t}"));
        }
        if list {
            return None;
        }
        let orig = archive.file_name()?.to_string_lossy();
        let out = unique_with_ext(dir, &orig[..orig.len() - 4], "");
        let o = q(&out);
        return Some(format!("{env}{dec} -o {o} {a} || {{ s=$?; rm -f {o}; (exit $s); }}"));
    }
    Some(if tar.iter().any(|e| name.ends_with(e)) {
        if list { format!("tar -tvf {a}") } else { format!("tar -xf {a} -C {d}") }
    } else if name.ends_with(".zip") {
        // unzip can not open AES zips; 7-Zip can, and asks for the password.
        match (list, sevenz) {
            (true, _) => format!("unzip -l {a}"),
            (false, Some(z)) => format!("{z} x {a} -o{d}"),
            (false, None) => format!("unzip {a} -d {d}"),
        }
    } else if name.ends_with(".7z") {
        let z = sevenz.unwrap_or("7z");
        if list { format!("{z} l {a}") } else { format!("{z} x {a} -o{d}") }
    } else if name.ends_with(".rar") {
        if list { format!("unrar l {a}") } else { format!("unrar x {a} {d}/") }
    } else {
        // A single compressed file: keep the original, write it next to it.
        let tool = [(".gz", "gzip"), (".xz", "xz"), (".bz2", "bzip2"), (".zst", "zstd")]
            .iter()
            .find(|(e, _)| name.ends_with(e))?
            .1;
        if list { format!("{tool} -l {a}") } else { format!("{tool} -dk {a}") }
    })
}

/// Whether working on `archive` may ask for a password (7z, AES zip, gpg,
/// age), so it must own the window's tty rather than share it with `less`.
pub fn archive_prompts(archive: &Path) -> bool {
    let name = archive.file_name().map(|n| n.to_string_lossy().to_ascii_lowercase()).unwrap_or_default();
    [".7z", ".zip", ".gpg", ".pgp", ".age"].iter().any(|e| name.ends_with(e))
}

/// An archive format for "Compress…" (plain) and "Encrypt…" (asks for a
/// password in the window). `.7z` is the default encrypted format: AES-256,
/// names hidden, opens on every OS.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pack {
    TarGz,
    Zip,
    SevenZ,
    SevenZAes,
    ZipAes,
    TarGzAge,
    TarGzGpg,
}

impl Pack {
    /// Plain formats first, then the encrypted ones, each in menu order.
    pub const ALL: [Pack; 7] = [Pack::SevenZ, Pack::Zip, Pack::TarGz, Pack::SevenZAes, Pack::ZipAes, Pack::TarGzAge, Pack::TarGzGpg];

    pub fn encrypted(self) -> bool {
        matches!(self, Pack::SevenZAes | Pack::ZipAes | Pack::TarGzAge | Pack::TarGzGpg)
    }

    /// The extension appended to the entry's name.
    pub fn ext(self) -> &'static str {
        match self {
            Pack::TarGz => ".tar.gz",
            Pack::Zip | Pack::ZipAes => ".zip",
            Pack::SevenZ | Pack::SevenZAes => ".7z",
            Pack::TarGzAge => ".tar.gz.age",
            Pack::TarGzGpg => ".tar.gz.gpg",
        }
    }

    /// The menu row, with a hint for choosing.
    pub fn label(self) -> &'static str {
        match self {
            Pack::SevenZ => ".7z (any OS, smallest)",
            Pack::Zip => ".zip (any OS)",
            Pack::TarGz => ".tar.gz (Linux, keeps permissions)",
            Pack::SevenZAes => ".7z AES-256 (any OS, hides names)",
            Pack::ZipAes => ".zip AES-256 (any OS, shows names)",
            Pack::TarGzAge => ".tar.gz.age (Linux, keeps permissions)",
            Pack::TarGzGpg => ".tar.gz.gpg AES-256 (Linux, keeps permissions)",
        }
    }
}

/// The installed programs the [`Pack`] formats need.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PackTools {
    pub tar: bool,
    pub zip: bool,
    /// The 7-Zip binary: `7z` (p7zip-full), `7zz` (7-Zip) or `7za`.
    pub sevenz: Option<&'static str>,
    pub age: bool,
    pub gpg: bool,
}

impl PackTools {
    pub fn current() -> Self {
        let have = crate::catalog::is_installed;
        PackTools {
            tar: have("tar"),
            zip: have("zip"),
            sevenz: ["7z", "7zz", "7za"].into_iter().find(|b| have(b)),
            age: have("age"),
            gpg: have("gpg"),
        }
    }

    /// The formats these tools can write, plain (`encrypted` false) or not.
    pub fn choices(self, encrypted: bool) -> impl Iterator<Item = Pack> {
        Pack::ALL.into_iter().filter(move |p| p.encrypted() == encrypted).filter(move |p| match p {
            Pack::TarGz => self.tar,
            Pack::Zip => self.zip || self.sevenz.is_some(),
            Pack::SevenZ | Pack::SevenZAes | Pack::ZipAes => self.sevenz.is_some(),
            Pack::TarGzAge => self.tar && self.age,
            Pack::TarGzGpg => self.tar && self.gpg,
        })
    }

    /// The command, run in the entry's folder, that packs `name` (a file or
    /// folder there) into `out` (a new name there). The password, if any, is
    /// asked by the tool itself on the window's tty: never argv, disk or log.
    /// A failed or cancelled run removes the partial `out`.
    pub fn command(self, pack: Pack, name: &str, out: &str) -> String {
        // A leading '-' would read as an option.
        let arg = |s: &str| crate::systems::sh_quote(&if s.starts_with('-') { format!("./{s}") } else { s.to_string() });
        let (n, o) = (arg(name), arg(out));
        let z = self.sevenz.unwrap_or("7z");
        let cmd = match pack {
            Pack::TarGz => format!("tar -czf {o} {n}"),
            Pack::Zip if self.zip => format!("zip -r {o} {n}"),
            Pack::Zip => format!("{z} a -tzip {o} {n}"),
            Pack::SevenZ => format!("{z} a {o} {n}"),
            Pack::SevenZAes => format!("{z} a -t7z -mhe=on -p {o} {n}"),
            Pack::ZipAes => format!("{z} a -tzip -mem=AES256 -p {o} {n}"),
            Pack::TarGzAge => format!("{PIPEFAIL}tar -czf - {n} | age -p -o {o}"),
            // gpg asks only once on a tty, so a typo would lock the data: confirm by
            // decrypting it again (which also checks the archive).
            Pack::TarGzGpg => format!(
                "{PIPEFAIL}{GPG_TTY}tar -czf - {n} | {GPG} --symmetric --cipher-algo AES256 -o {o} \
                 && echo 'Confirm: enter the passphrase again.' && {GPG} -d {o} | tar -tzf - > /dev/null"
            ),
        };
        format!("{cmd} || {{ s=$?; rm -f {o}; (exit $s); }}")
    }
}

/// The "Compress…" (`encrypted` false) or "Encrypt…" choices installed here.
pub fn pack_actions(encrypted: bool) -> Vec<MenuAction> {
    PackTools::current().choices(encrypted).map(MenuAction::PackAs).collect()
}

/// `stem` + `ext` in `dir`, or `stem copy` + `ext`, `stem copy 2` + `ext`, …
/// when taken (like [`crate::fileops::unique_destination`], but keeping a
/// compound extension such as `.tar.gz.gpg` whole).
pub fn unique_with_ext(dir: &Path, stem: &str, ext: &str) -> std::path::PathBuf {
    (1..1_000_000)
        .map(|n| match n {
            1 => format!("{stem}{ext}"),
            2 => format!("{stem} copy{ext}"),
            n => format!("{stem} copy {}{ext}", n - 1),
        })
        .map(|name| dir.join(name))
        .find(|p| !p.exists())
        .unwrap_or_else(|| dir.join(format!("{stem}-{}{ext}", std::process::id())))
}

/// The "Open with…" choices for `path`: the built-in viewer for images, then
/// the installed terminal apps from the Default Apps candidates. A web page
/// lists the installed text browsers first, then the editors.
pub fn open_with_actions(path: &Path) -> Vec<MenuAction> {
    let role = crate::openwith::classify(path, false);
    let mut cmds = if crate::openwith::is_web_page(path) { crate::openwith::browsers() } else { Vec::new() };
    for c in crate::openwith::candidates(role) {
        if !cmds.contains(&c) {
            cmds.push(c);
        }
    }
    cmds.into_iter()
        .filter(|c| !c.is_empty() && c != "@navigate" && (c != "@image" || role == Role::Image))
        .map(MenuAction::OpenWithApp)
        .collect()
}

/// An open menu: its items, the highlighted row and where it was opened.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ContextMenu {
    pub anchor: Point,
    pub items: Vec<MenuAction>,
    pub sel: usize,
}

impl ContextMenu {
    pub fn new(anchor: Point, items: Vec<MenuAction>) -> Self {
        Self { anchor, items, sel: 0 }
    }

    /// Replace the items in place (the Open-with submenu), highlight the first.
    pub fn replace(&mut self, items: Vec<MenuAction>) {
        self.items = items;
        self.sel = 0;
    }

    /// The menu box: one row per item inside a one-cell border, as wide as the
    /// longest label, moved so it stays inside `bounds`. Render and hit-test
    /// both use this.
    pub fn rect(&self, bounds: Rect) -> Rect {
        let longest = self.items.iter().map(|a| a.label().chars().count()).max().unwrap_or(0) as i32;
        let w = (longest + 4).max(14).min(bounds.w.max(1));
        let h = (self.items.len() as i32 + 2).min(bounds.h.max(1));
        let x = self.anchor.x.clamp(bounds.x, (bounds.x + bounds.w - w).max(bounds.x));
        let y = self.anchor.y.clamp(bounds.y, (bounds.y + bounds.h - h).max(bounds.y));
        Rect::new(x, y, w, h)
    }

    /// The item row under `p`, if any.
    pub fn index_at(&self, p: Point, bounds: Rect) -> Option<usize> {
        let r = self.rect(bounds);
        if !r.contains(p) {
            return None;
        }
        let row = p.y - (r.y + 1);
        (row >= 0 && (row as usize) < self.items.len()).then_some(row as usize)
    }

    /// The item under `p`, if any.
    pub fn item_at(&self, p: Point, bounds: Rect) -> Option<&MenuAction> {
        self.index_at(p, bounds).map(|i| &self.items[i])
    }

    /// Highlight the row under `p` (mouse hover); off the rows keeps the highlight.
    pub fn hover(&mut self, p: Point, bounds: Rect) {
        if let Some(i) = self.index_at(p, bounds) {
            self.sel = i;
        }
    }

    pub fn up(&mut self) {
        let n = self.items.len().max(1);
        self.sel = (self.sel + n - 1) % n;
    }

    pub fn down(&mut self) {
        let n = self.items.len().max(1);
        self.sel = (self.sel + 1) % n;
    }

    pub fn selected(&self) -> Option<&MenuAction> {
        self.items.get(self.sel)
    }

    /// Draw the menu into `buf` (whose cell (0,0) is `bounds`' coordinate space
    /// origin, i.e. screen or window-content coordinates matching `bounds`).
    /// Opaque theme colours: a translucent blank cell would let the glyphs
    /// beneath (the wallpaper) show through.
    pub fn render(&self, buf: &mut CellBuffer, bounds: Rect) {
        let t = crate::theme::current();
        let (fg, bg) = (t.text, t.active_bg);
        let r = self.rect(bounds);
        for y in r.y..r.y + r.h {
            for x in r.x..r.x + r.w {
                buf.set(x, y, Cell { ch: ' ', fg, bg, attrs: Default::default() });
            }
        }
        for (i, item) in self.items.iter().enumerate() {
            let y = r.y + 1 + i as i32;
            if y >= r.y + r.h - 1 {
                break;
            }
            let (fg, bg) = if i == self.sel { (t.window_bg, t.accent) } else { (fg, bg) };
            for x in r.x + 1..r.x + r.w - 1 {
                buf.set(x, y, Cell { ch: ' ', fg, bg, attrs: Default::default() });
            }
            let label: String = item.label().chars().take((r.w - 3).max(0) as usize).collect();
            buf.write_str(r.x + 2, y, &label, fg, bg);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn menu(n: usize) -> ContextMenu {
        ContextMenu::new(Point::new(5, 3), actions_for(Surface::Files, Target::Entry { is_dir: false, pinned: false, role: Role::Text }, Abilities::default()).into_iter().take(n).collect())
    }

    #[test]
    fn cast_is_offered_for_media_files_only_when_catt_is_there() {
        use MenuAction as A;
        let none = Abilities::default();
        let can = Abilities { cast: true, ..none };
        let t = |role, is_dir| Target::Entry { is_dir, pinned: false, role };
        for role in [Role::Video, Role::Audio, Role::Image] {
            for surface in [Surface::Desktop, Surface::Files] {
                assert!(actions_for(surface, t(role, false), can).contains(&A::CastTo), "{role:?} on {surface:?}");
                assert!(!actions_for(surface, t(role, false), none).contains(&A::CastTo), "no catt, no item");
            }
        }
        for role in [Role::Text, Role::Code, Role::Archive, Role::Pdf, Role::Executable, Role::Other] {
            assert!(!actions_for(Surface::Files, t(role, false), can).contains(&A::CastTo), "{role:?} is not media");
        }
        assert!(!actions_for(Surface::Files, t(Role::Video, true), can).contains(&A::CastTo), "a folder is not a file");
        assert!(!actions_for(Surface::Desktop, Target::Entry { is_dir: false, pinned: true, role: Role::Video }, can).contains(&A::CastTo), "pinned shortcuts only open or unpin");
        assert_eq!(A::CastTo.label(), "Cast to Chromecast");
    }

    #[test]
    fn cast_command_quotes_the_file_and_the_device() {
        let f = Path::new("/home/u/My Videos/it's here.mp4");
        assert_eq!(cast_command(f, ""), "catt cast '/home/u/My Videos/it'\\''s here.mp4'");
        assert_eq!(cast_command(f, "  "), cast_command(f, ""), "blank device means catt default");
        let c = cast_command(Path::new("/m/a.mp4"), "Living Room; rm -rf ~");
        assert_eq!(c, "catt -d 'Living Room; rm -rf ~' cast '/m/a.mp4'");
    }

    #[test]
    fn actions_differ_by_surface_and_target() {
        use MenuAction as A;
        let none = Abilities::default();
        let all = Abilities { fullscreen_image: true, fullscreen_video: true, fullscreen_pdf: true, send: true, cast: false, compress: false, encrypt: false, paste: false, trash: false };
        let file = Target::Entry { is_dir: false, pinned: false, role: Role::Text };
        let dir = Target::Entry { is_dir: true, pinned: false, role: Role::Directory };
        let img = Target::Entry { is_dir: false, pinned: false, role: Role::Image };
        let vid = Target::Entry { is_dir: false, pinned: false, role: Role::Video };
        let zip = Target::Entry { is_dir: false, pinned: false, role: Role::Archive };
        assert_eq!(actions_for(Surface::Desktop, file, none), vec![A::Open, A::OpenWith, A::Rename, A::Copy, A::Cut, A::Trash, A::CopyPath, A::OpenTerminalHere, A::GetInfo]);
        assert_eq!(actions_for(Surface::Desktop, dir, none), vec![A::Open, A::Rename, A::Copy, A::Cut, A::Trash, A::CopyPath, A::OpenTerminalHere, A::GetInfo], "no Open with for folders");
        assert_eq!(actions_for(Surface::Desktop, Target::Entry { is_dir: false, pinned: true, role: Role::Other }, all), vec![A::Open, A::Unpin]);
        assert_eq!(actions_for(Surface::Desktop, Target::Background, all), vec![A::NewFolder, A::OpenTerminalHere, A::CleanUp]);
        assert_eq!(
            actions_for(Surface::Files, file, none),
            vec![A::Open, A::OpenWith, A::Rename, A::Copy, A::Cut, A::Trash, A::CopyPath, A::OpenTerminalHere, A::GetInfo]
        );
        assert_eq!(actions_for(Surface::Files, Target::Background, all), vec![A::NewFolder, A::OpenTerminalHere]);
        // Paste once something was copied or cut; Empty Trash once it holds something.
        let full = Abilities { paste: true, trash: true, ..all };
        assert_eq!(actions_for(Surface::Desktop, Target::Background, full), vec![A::Paste, A::NewFolder, A::OpenTerminalHere, A::CleanUp, A::EmptyTrash]);
        assert_eq!(actions_for(Surface::Files, Target::Background, full), vec![A::Paste, A::NewFolder, A::OpenTerminalHere, A::EmptyTrash]);
        assert!(actions_for(Surface::Files, dir, full).contains(&A::PasteInto), "a folder takes a paste");
        assert!(!actions_for(Surface::Files, file, full).contains(&A::PasteInto), "a file does not");
        assert!(!actions_for(Surface::Files, Target::Background, full.remote()).contains(&A::Paste), "no shared paste on a remote folder");
        // Images: wallpaper always, full screen only where the client can hand over.
        let plain = actions_for(Surface::Desktop, img, none);
        assert!(plain.contains(&A::SetWallpaper) && !plain.contains(&A::ViewFullscreen));
        assert_eq!(&actions_for(Surface::Files, img, all)[..4], &[A::Open, A::OpenWith, A::ViewFullscreen, A::SetWallpaper]);
        assert!(!actions_for(Surface::Desktop, file, all).contains(&A::ViewFullscreen), "only images");
        // Video full screen and archives; Send to only with saved Systems.
        assert!(actions_for(Surface::Desktop, vid, all).contains(&A::PlayFullscreen));
        assert!(!actions_for(Surface::Desktop, vid, none).contains(&A::PlayFullscreen));
        assert!(actions_for(Surface::Files, zip, none).contains(&A::ExtractHere));
        assert_eq!(actions_for(Surface::Files, file, all).last(), Some(&A::SendTo));
        assert!(!actions_for(Surface::Files, file, none).contains(&A::SendTo));
    }

    #[test]
    fn open_with_lists_browsers_first_for_web_pages() {
        let installed = crate::openwith::browsers();
        let page = open_with_actions(Path::new("/x/Start.HTML"));
        let first: Vec<_> = page.iter().take(installed.len()).cloned().collect();
        assert_eq!(first, installed.iter().map(|b| MenuAction::OpenWithApp(b.clone())).collect::<Vec<_>>());
        // The editors stay after the browsers; nothing is listed twice.
        assert!(page.iter().enumerate().all(|(i, a)| !page[..i].contains(a)));
        // Only web pages get browsers.
        let css = open_with_actions(Path::new("/x/style.css"));
        assert!(!css.iter().any(|a| matches!(a, MenuAction::OpenWithApp(c) if installed.contains(c))));
        assert!(crate::openwith::is_web_page(Path::new("a.htm")) && !crate::openwith::is_web_page(Path::new("a.html.txt")));
    }

    #[test]
    fn archive_commands_by_kind() {
        let d = Path::new("/x");
        assert_eq!(archive_command(Path::new("/x/a.tar.gz"), d, false).as_deref(), Some("tar -xf '/x/a.tar.gz' -C '/x'"));
        assert_eq!(archive_command(Path::new("/x/a.ZIP"), d, true).as_deref(), Some("unzip -l '/x/a.ZIP'"));
        assert_eq!(archive_command(Path::new("/x/a.7z"), d, false).as_deref(), Some("7z x '/x/a.7z' -o'/x'"));
        assert_eq!(archive_command(Path::new("/x/log.gz"), d, false).as_deref(), Some("gzip -dk '/x/log.gz'"));
        assert_eq!(archive_command(Path::new("/x/a.txt"), d, false), None);
    }

    #[test]
    fn compress_and_encrypt_are_offered_for_local_files_and_folders() {
        use MenuAction as A;
        let can = Abilities { compress: true, encrypt: true, ..Abilities::default() };
        for t in [
            Target::Entry { is_dir: true, pinned: false, role: Role::Directory },
            Target::Entry { is_dir: false, pinned: false, role: Role::Text },
        ] {
            for surface in [Surface::Desktop, Surface::Files] {
                let v = actions_for(surface, t, can);
                let i = v.iter().position(|a| *a == A::Compress).expect("Compress");
                assert_eq!(v[i + 1], A::Encrypt, "Encrypt follows Compress");
                assert!(!actions_for(surface, t, can.remote()).iter().any(|a| matches!(a, A::Compress | A::Encrypt)), "remote folders: local tools only");
                assert!(!actions_for(surface, t, Abilities::default()).contains(&A::Compress), "no tools, no item");
            }
        }
        assert_eq!(actions_for(Surface::Desktop, Target::Entry { is_dir: false, pinned: true, role: Role::Other }, can), vec![A::Open, A::Unpin]);
        assert_eq!(A::Encrypt.label(), "Encrypt\u{2026}");
        assert_eq!(A::PackAs(Pack::SevenZAes).label(), ".7z AES-256 (any OS, hides names)");
    }

    #[test]
    fn pack_choices_follow_the_installed_tools_with_7z_first() {
        let all = PackTools { tar: true, zip: true, sevenz: Some("7z"), age: true, gpg: true };
        assert_eq!(all.choices(false).collect::<Vec<_>>(), vec![Pack::SevenZ, Pack::Zip, Pack::TarGz]);
        assert_eq!(all.choices(true).collect::<Vec<_>>(), vec![Pack::SevenZAes, Pack::ZipAes, Pack::TarGzAge, Pack::TarGzGpg]);
        let gpg_only = PackTools { tar: true, gpg: true, ..PackTools::default() };
        assert_eq!(gpg_only.choices(true).collect::<Vec<_>>(), vec![Pack::TarGzGpg]);
        assert_eq!(gpg_only.choices(false).collect::<Vec<_>>(), vec![Pack::TarGz], "zip needs zip or 7z");
        let seven = PackTools { sevenz: Some("7zz"), ..PackTools::default() };
        assert_eq!(seven.choices(false).collect::<Vec<_>>(), vec![Pack::SevenZ, Pack::Zip], "7z writes zip too");
        assert_eq!(PackTools::default().choices(true).count(), 0);
        assert!(Pack::ALL.iter().all(|p| p.label().starts_with(p.ext())), "the row names the extension");
    }

    #[test]
    fn pack_commands_quote_names_and_never_carry_the_password() {
        let t = PackTools { tar: true, zip: false, sevenz: Some("7zz"), age: true, gpg: true };
        let cleanup = " || { s=$?; rm -f 'it'\\''s.7z'; (exit $s); }";
        assert_eq!(t.command(Pack::SevenZAes, "it's", "it's.7z"), format!("7zz a -t7z -mhe=on -p 'it'\\''s.7z' 'it'\\''s'{cleanup}"));
        assert_eq!(t.command(Pack::Zip, "a", "a.zip"), "7zz a -tzip 'a.zip' 'a' || { s=$?; rm -f 'a.zip'; (exit $s); }", "7-Zip writes zip when zip is missing");
        assert!(t.command(Pack::TarGz, "-rf", "-rf.tar.gz").starts_with("tar -czf './-rf.tar.gz' './-rf'"), "a leading dash is not an option");
        let gpg = t.command(Pack::TarGzGpg, "d", "d.tar.gz.gpg");
        assert!(gpg.contains("export GPG_TTY=$(tty); tar -czf - 'd' | gpg --pinentry-mode loopback"), "{gpg}");
        assert!(gpg.contains("--cipher-algo AES256") && gpg.contains("-d 'd.tar.gz.gpg' | tar -tzf -"), "confirmed by decrypting: {gpg}");
        assert!(t.command(Pack::TarGzAge, "d", "d.tar.gz.age").contains("tar -czf - 'd' | age -p -o 'd.tar.gz.age'"));
        for p in Pack::ALL {
            let c = t.command(p, "d", "o");
            assert!(!c.contains("-p'") && !c.contains("--passphrase") && !c.contains("SSHPASS"), "{p:?}: the tool asks on the tty");
        }
    }

    #[test]
    fn encrypted_files_decrypt_in_extract_here() {
        let d = Path::new("/nonexistent-x");
        let x = |n: &str, list| archive_command_with(&d.join(n), d, list, Some("7z"));
        assert_eq!(
            x("a.tar.gz.gpg", false).as_deref(),
            Some("(set -o pipefail) 2>/dev/null && set -o pipefail; export GPG_TTY=$(tty); gpg --pinentry-mode loopback --no-symkey-cache -d '/nonexistent-x/a.tar.gz.gpg' | tar -z -xf - -C '/nonexistent-x'")
        );
        assert!(x("a.tar.xz.age", true).unwrap().ends_with("age -d '/nonexistent-x/a.tar.xz.age' | tar -J -tvf -"));
        assert_eq!(x("n.txt.age", false).as_deref(), Some("age -d -o '/nonexistent-x/n.txt' '/nonexistent-x/n.txt.age' || { s=$?; rm -f '/nonexistent-x/n.txt'; (exit $s); }"));
        assert_eq!(x("n.txt.gpg", true), None, "a single file has no listing");
        assert_eq!(x("a.zip", false).as_deref(), Some("7z x '/nonexistent-x/a.zip' -o'/nonexistent-x'"), "7-Zip opens AES zips");
        assert_eq!(archive_command_with(&d.join("a.zip"), d, false, None).as_deref(), Some("unzip '/nonexistent-x/a.zip' -d '/nonexistent-x'"));
        assert!(archive_prompts(Path::new("a.7z")) && archive_prompts(Path::new("a.TAR.GZ.GPG")) && !archive_prompts(Path::new("a.tar.gz")));
        assert_eq!(crate::openwith::classify(Path::new("x.tar.gz.age"), false), Role::Archive);
    }

    #[test]
    fn unique_with_ext_keeps_compound_extensions() {
        let d = std::env::temp_dir().join(format!("tetron-pack-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        assert_eq!(unique_with_ext(&d, "a", ".tar.gz.gpg"), d.join("a.tar.gz.gpg"));
        std::fs::write(d.join("a.tar.gz.gpg"), b"").unwrap();
        assert_eq!(unique_with_ext(&d, "a", ".tar.gz.gpg"), d.join("a copy.tar.gz.gpg"));
        std::fs::write(d.join("a copy.tar.gz.gpg"), b"").unwrap();
        assert_eq!(unique_with_ext(&d, "a", ".tar.gz.gpg"), d.join("a copy 2.tar.gz.gpg"));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn open_with_lists_the_viewer_only_for_images() {
        let img = open_with_actions(Path::new("/x/cat.png"));
        assert_eq!(img.first(), Some(&MenuAction::OpenWithApp("@image".into())));
        assert_eq!(img[0].label(), "Image viewer");
        let txt = open_with_actions(Path::new("/x/notes.txt"));
        assert!(!txt.contains(&MenuAction::OpenWithApp("@image".into())));
        assert!(!txt.iter().any(|a| matches!(a, MenuAction::OpenWithApp(c) if c.is_empty() || c == "@navigate")));
        // Video never offers editors; when mpv is installed it plays in the window.
        let vid = open_with_actions(Path::new("/x/clip.mp4"));
        assert!(!vid.iter().any(|a| matches!(a, MenuAction::OpenWithApp(c) if ["vi", "nano", "less"].contains(&c.as_str()))));
        if crate::catalog::is_installed("mpv") {
            assert!(vid.contains(&MenuAction::OpenWithApp("mpv --vo=tct".into())));
        }
    }

    #[test]
    fn rect_fits_the_labels_and_stays_inside_the_bounds() {
        let m = menu(7);
        let b = Rect::new(0, 1, 80, 24);
        let r = m.rect(b);
        assert_eq!((r.x, r.y, r.h), (5, 3, 9));
        assert!(r.w >= "Move to Trash".len() as i32 + 4);
        // Anchored at the bottom-right corner: moved back inside.
        let mut m = m;
        m.anchor = Point::new(79, 24);
        let r = m.rect(b);
        assert!(r.x + r.w <= 80 && r.y + r.h <= 25, "{r:?}");
        assert!(r.y >= 1, "never over the menubar row");
    }

    #[test]
    fn rows_hit_test_hover_and_keys_wrap() {
        let mut m = menu(3);
        let b = Rect::new(0, 0, 80, 24);
        assert_eq!(m.item_at(Point::new(7, 4), b), Some(&MenuAction::Open), "first row under the top border");
        assert_eq!(m.item_at(Point::new(7, 5), b), Some(&MenuAction::OpenWith));
        assert_eq!(m.item_at(Point::new(7, 3), b), None, "border row");
        assert_eq!(m.item_at(Point::new(60, 4), b), None, "outside");
        m.hover(Point::new(7, 6), b);
        assert_eq!(m.selected(), Some(&MenuAction::Rename));
        m.down();
        assert_eq!(m.sel, 0, "down wraps");
        m.up();
        assert_eq!(m.sel, 2, "up wraps");
        m.replace(vec![MenuAction::OpenWithApp("less".into())]);
        assert_eq!((m.sel, m.items.len()), (0, 1));
    }

    #[test]
    fn render_is_opaque_and_highlights_the_selection() {
        let mut m = menu(2);
        m.sel = 1;
        let b = Rect::new(0, 0, 40, 12);
        let mut buf = CellBuffer::new(40, 12);
        m.render(&mut buf, b);
        let r = m.rect(b);
        for y in r.y..r.y + r.h {
            for x in r.x..r.x + r.w {
                assert_eq!(buf.get(x, y).unwrap().bg.a, 255, "opaque at {x},{y}");
            }
        }
        let t = crate::theme::current();
        assert_eq!(buf.get(r.x + 2, r.y + 2).unwrap().bg, t.accent, "highlighted row");
        let row: String = (r.x + 2..r.x + 6).map(|x| buf.get(x, r.y + 1).unwrap().ch).collect();
        assert_eq!(row, "Open");
    }
}

