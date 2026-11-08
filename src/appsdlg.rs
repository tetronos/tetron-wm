//! Settings > General > Apps dialog (WMINST-011): pick a size to install
//! (Essentials, Recommended, Extras once it has apps), tick whether the apps
//! no distribution packages come too, or remove a set; every choice opens a
//! confirmation first, with focus on Cancel. The dialog only decides; the
//! session runs the suite installer or apt in a terminal window.

use crate::buffer::CellBuffer;
use crate::cell::Rgba;
use crate::packaging::{Missing, Size};

fn pal_bg() -> Rgba { crate::theme::current().window_bg }
fn pal_fg() -> Rgba { crate::theme::current().text }
fn pal_dim() -> Rgba { crate::theme::current().dim }
fn pal_sel() -> Rgba { crate::theme::current().active_bg }
fn pal_accent() -> Rgba { crate::theme::current().accent }

/// The download size line of the confirmation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Download {
    /// Not an apt host: no size shown.
    NotApt,
    Computing,
    Known(u64),
    Unknown,
}

/// What a key or click asks of Settings.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    None,
    Close,
    /// Run the suite installer for this size; `bool`: with the script apps.
    Install(Size, bool),
    /// Remove the size debs of this size and larger (apt).
    Remove(Size),
    /// Compute the download size of [`AppsDlg::download_pkgs`] off the loop.
    Download,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum What {
    Install(Size),
    Remove,
}

#[derive(Debug)]
struct Confirm {
    what: What,
    scroll: usize,
    /// Focus on the Install (or Remove) button; Cancel otherwise.
    ok: bool,
    download: Download,
}

#[derive(Debug)]
pub struct AppsDlg {
    installed: Size,
    have_curl: bool,
    apt: bool,
    /// Every missing app up to extras, read when the dialog opened.
    missing: Vec<Missing>,
    sizes: Vec<Size>,
    /// The size debs Remove set would remove (empty: no Remove set row).
    remove: Vec<&'static str>,
    sel: usize,
    scripts: bool,
    confirm: Option<Confirm>,
}

/// Lines of the app list in the confirmation that fit above the buttons.
fn list_rows(h: i32) -> usize {
    (h - 11).max(1) as usize
}

impl AppsDlg {
    /// `extras_listed`: the list has extras apps (else Extras is not offered).
    pub fn new(installed: Size, have_curl: bool, apt: bool, missing: Vec<Missing>, extras_listed: bool, remove: Vec<&'static str>) -> AppsDlg {
        let mut sizes = vec![Size::Essentials, Size::Recommended];
        if extras_listed {
            sizes.push(Size::Extras);
        }
        AppsDlg { installed, have_curl, apt, missing, sizes, remove, sel: 0, scripts: true, confirm: None }
    }

    fn rows(&self) -> usize {
        self.sizes.len() + 1 + usize::from(!self.remove.is_empty())
    }

    fn checkbox_row(&self) -> usize {
        self.sizes.len()
    }

    /// The missing apps installing `size` adds, with the checkbox applied.
    fn adding(&self, size: Size) -> Vec<&Missing> {
        self.missing.iter().filter(|m| m.size <= size && (self.scripts || !m.is_script())).collect()
    }

    /// The packages whose download size the confirmation shows.
    pub fn download_pkgs(&self) -> Vec<String> {
        match self.confirm.as_ref().map(|c| c.what) {
            Some(What::Install(s)) => self.adding(s).iter().filter(|m| !m.is_script()).map(|m| m.pkg.clone()).collect(),
            _ => Vec::new(),
        }
    }

    pub fn set_download(&mut self, bytes: Option<u64>) {
        if let Some(c) = self.confirm.as_mut() {
            c.download = bytes.map_or(Download::Unknown, Download::Known);
        }
    }

    pub fn up(&mut self) {
        match self.confirm.as_mut() {
            Some(c) => c.scroll = c.scroll.saturating_sub(1),
            None => self.sel = self.sel.saturating_sub(1),
        }
    }

    pub fn down(&mut self) {
        let n = self.confirm.as_ref().map(|c| match c.what {
            What::Install(s) => self.adding(s).len(),
            What::Remove => self.remove.len(),
        });
        match (self.confirm.as_mut(), n) {
            (Some(c), Some(n)) => c.scroll = (c.scroll + 1).min(n.saturating_sub(1)),
            _ => self.sel = (self.sel + 1).min(self.rows() - 1),
        }
    }

    /// Left / Right: switch the confirmation's button; flip the checkbox.
    pub fn side(&mut self) {
        let on_box = self.sel == self.checkbox_row();
        match self.confirm.as_mut() {
            Some(c) => c.ok = !c.ok,
            None if on_box => self.scripts = !self.scripts,
            None => {}
        }
    }

    /// Esc: back from the confirmation, else close.
    pub fn esc(&mut self) -> Outcome {
        if self.confirm.take().is_some() { Outcome::None } else { Outcome::Close }
    }

    /// Enter (and Space on the choice list).
    pub fn enter(&mut self) -> Outcome {
        if let Some(c) = self.confirm.take() {
            if !c.ok {
                return Outcome::None;
            }
            return match c.what {
                What::Install(s) => Outcome::Install(s, self.scripts),
                What::Remove => Outcome::Remove(self.installed),
            };
        }
        let row = self.sel;
        if row == self.checkbox_row() {
            self.scripts = !self.scripts;
            return Outcome::None;
        }
        let what = match self.sizes.get(row) {
            Some(_) if !self.have_curl => return Outcome::None,
            Some(&s) => What::Install(s),
            None => What::Remove,
        };
        let mut c = Confirm { what, scroll: 0, ok: false, download: Download::NotApt };
        let fetch = self.apt && matches!(what, What::Install(_));
        if fetch {
            c.download = Download::Computing;
        }
        self.confirm = Some(c);
        if fetch && !self.download_pkgs().is_empty() {
            Outcome::Download
        } else {
            if let Some(c) = self.confirm.as_mut().filter(|_| fetch) {
                c.download = Download::Known(0);
            }
            Outcome::None
        }
    }

    /// A click at content row `y` (0 = the section title line) and column
    /// `x` relative to the content's left edge, for content height `h`.
    pub fn click(&mut self, x: i32, y: i32, h: i32) -> Outcome {
        if self.confirm.is_some() {
            if y == h - 2 {
                if let Some(c) = self.confirm.as_mut() {
                    c.ok = x < 14;
                }
                return self.enter();
            }
            return Outcome::None;
        }
        let row = y - 5;
        if row >= 0 && (row as usize) < self.rows() {
            self.sel = row as usize;
            return self.enter();
        }
        Outcome::None
    }

    fn size_note(&self, s: Size) -> &'static str {
        match s {
            Size::Essentials => "the tools tetron-wm uses and one good app per need",
            Size::Recommended => "plus a curated pick in every category",
            Size::Extras => "plus the extra apps",
            Size::Minimal => "",
        }
    }

    /// Draw into the content area starting at column `cx`; `w`, `h` are the
    /// whole panel's size.
    pub fn render(&self, buf: &mut CellBuffer, cx: i32, w: i32, h: i32) {
        let width = (w - cx - 1).max(10) as usize;
        let put = |buf: &mut CellBuffer, y: i32, s: &str, fg: Rgba, bg: Rgba| {
            let s: String = s.chars().take(width).collect();
            buf.write_str(cx, y, &s, fg, bg);
        };
        let missing_now = self.missing.iter().filter(|m| m.size <= self.installed).count();
        let have = match missing_now {
            0 => self.installed.name().to_string(),
            n => format!("{}, {n} missing", self.installed.name()),
        };
        let Some(c) = self.confirm.as_ref() else {
            put(buf, 1, "General \u{203A} Apps", pal_accent(), pal_bg());
            put(buf, 3, &format!("Installed: {have}"), pal_fg(), pal_bg());
            for i in 0..self.rows() {
                let y = 5 + i as i32;
                let sel = i == self.sel;
                let line = if let Some(&s) = self.sizes.get(i) {
                    let add = self.adding(s).len();
                    let state = if !self.have_curl {
                        "needs curl".to_string()
                    } else if add == 0 {
                        "all installed".to_string()
                    } else {
                        format!("{add} to add")
                    };
                    format!("{:<12} {:<52} {state}", s.title(), self.size_note(s))
                } else if i == self.checkbox_row() {
                    let tick = if self.scripts { "[x]" } else { "[ ]" };
                    format!("{tick} also the apps no distribution packages (opencode, fresh, uv, catt, casttui, Nerd Font)")
                } else {
                    format!("Remove set: {}", self.installed.name())
                };
                let (fg, bg) = if sel { (pal_fg(), pal_sel()) } else { (pal_dim(), pal_bg()) };
                put(buf, y, &format!("{} {line}", if sel { "\u{25B8}" } else { " " }), fg, bg);
            }
            let y = 6 + self.rows() as i32;
            put(buf, y, "Enter chooses (a confirmation follows). Space or Left/Right ticks the box.", pal_dim(), pal_bg());
            put(buf, y + 1, "Installs in a terminal window, where sudo asks for your password. Esc closes.", pal_dim(), pal_bg());
            return;
        };
        let (title, lines): (String, Vec<String>) = match c.what {
            What::Install(s) => {
                let add = self.adding(s);
                let dl = match c.download {
                    Download::NotApt => String::new(),
                    Download::Computing => ", download size: computing\u{2026}".into(),
                    Download::Known(0) => String::new(),
                    Download::Known(b) => format!(", download {}", crate::packaging::fmt_download(b)),
                    Download::Unknown => ", download size unknown".into(),
                };
                let title = format!("Install {}: {} apps to add{dl}", s.title(), add.len());
                let lines = add
                    .iter()
                    .map(|m| if m.is_script() { format!("{} (its own installer)", m.name) } else { m.name.clone() })
                    .collect();
                (title, lines)
            }
            What::Remove => (format!("Remove set: {}", self.installed.name()), self.remove.iter().map(|p| p.to_string()).collect()),
        };
        put(buf, 1, "General \u{203A} Apps", pal_accent(), pal_bg());
        put(buf, 3, &title, pal_fg(), pal_bg());
        let (l1, l2) = match c.what {
            What::Install(_) => (
                "Needs the network and your sudo password. Apps already here are kept.",
                if self.apt { "Undo: Remove set here, then apt lists what it removes and asks." } else { "Undo: remove the apps one by one with your package manager." },
            ),
            What::Remove => (
                "Removes these packages. apt then lists the apps no other package needs",
                "and asks before removing them. Apps you installed yourself stay.",
            ),
        };
        put(buf, 4, l1, pal_dim(), pal_bg());
        put(buf, 5, l2, pal_dim(), pal_bg());
        let rows = list_rows(h);
        for (i, l) in lines.iter().skip(c.scroll).take(rows).enumerate() {
            put(buf, 7 + i as i32, &format!("  {l}"), pal_fg(), pal_bg());
        }
        if lines.len() > rows {
            put(buf, h - 3, &format!("  \u{2191}\u{2193} {} more", lines.len().saturating_sub(c.scroll + rows)), pal_dim(), pal_bg());
        }
        let ok = match c.what {
            What::Install(_) => "[ Install ]",
            What::Remove => "[ Remove ]",
        };
        let (of, ob) = if c.ok { (pal_fg(), pal_sel()) } else { (pal_dim(), pal_bg()) };
        let (cf, cb) = if c.ok { (pal_dim(), pal_bg()) } else { (pal_fg(), pal_sel()) };
        buf.write_str(cx, h - 2, ok, of, ob);
        buf.write_str(cx + 14, h - 2, "[ Cancel ]", cf, cb);
        buf.write_str(cx + 26, h - 2, "Left/Right switch, Enter, Esc back", pal_dim(), pal_bg());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn m(name: &str, pkg: &str, size: Size) -> Missing {
        Missing { name: name.into(), pkg: pkg.into(), size }
    }

    fn dlg(apt: bool, remove: Vec<&'static str>) -> AppsDlg {
        let missing = vec![m("tmux", "tmux", Size::Essentials), m("opencode", "", Size::Essentials), m("lazygit", "lazygit", Size::Recommended)];
        AppsDlg::new(Size::Essentials, true, apt, missing, false, remove)
    }

    #[test]
    fn choose_a_size_confirm_then_install_with_the_script_apps() {
        let mut d = dlg(false, vec![]);
        assert_eq!(d.rows(), 3, "two sizes and the checkbox; no extras, no remove");
        d.down();
        assert_eq!(d.enter(), Outcome::None, "opens the confirmation");
        assert_eq!(d.adding(Size::Recommended).len(), 3);
        assert_eq!(d.enter(), Outcome::None, "focus starts on Cancel");
        assert!(d.confirm.is_none(), "Cancel closes the confirmation");
        d.enter();
        d.side();
        assert_eq!(d.enter(), Outcome::Install(Size::Recommended, true));
    }

    #[test]
    fn the_checkbox_leaves_out_the_script_apps() {
        let mut d = dlg(false, vec![]);
        d.sel = d.checkbox_row();
        d.enter();
        assert!(!d.scripts);
        assert_eq!(d.adding(Size::Essentials).len(), 1);
        d.side();
        assert!(d.scripts, "Left/Right flip it too");
        d.side();
        d.sel = 0;
        d.enter();
        d.side();
        assert_eq!(d.enter(), Outcome::Install(Size::Essentials, false));
    }

    #[test]
    fn apt_host_asks_for_the_download_size_of_packages_only() {
        let mut d = dlg(true, vec![]);
        assert_eq!(d.enter(), Outcome::Download);
        assert_eq!(d.download_pkgs(), ["tmux"]);
        assert_eq!(d.confirm.as_ref().unwrap().download, Download::Computing);
        d.set_download(Some(5_000_000));
        assert_eq!(d.confirm.as_ref().unwrap().download, Download::Known(5_000_000));
        assert_eq!(d.esc(), Outcome::None, "Esc goes back to the choices");
        assert_eq!(d.esc(), Outcome::Close);
    }

    #[test]
    fn remove_set_is_offered_only_with_debs_to_remove_and_confirmed() {
        let mut d = dlg(true, vec!["tetron-wm-essentials"]);
        assert_eq!(d.rows(), 4);
        d.sel = 3;
        assert_eq!(d.enter(), Outcome::None, "confirmation, no download size");
        d.side();
        assert_eq!(d.enter(), Outcome::Remove(Size::Essentials));
    }

    #[test]
    fn extras_only_when_listed_and_no_install_without_curl() {
        let d = AppsDlg::new(Size::Minimal, true, false, vec![], true, vec![]);
        assert_eq!(d.sizes, [Size::Essentials, Size::Recommended, Size::Extras]);
        let mut d = AppsDlg::new(Size::Minimal, false, false, vec![], false, vec![]);
        assert_eq!(d.enter(), Outcome::None);
        assert!(d.confirm.is_none(), "without curl a size does nothing");
    }

    #[test]
    fn renders_choices_and_confirmation() {
        let text = |d: &AppsDlg| {
            let mut b = CellBuffer::new(120, 30);
            d.render(&mut b, 16, 120, 30);
            (0..30).map(|y| (0..120).filter_map(|x| b.get(x, y).map(|c| c.ch)).collect::<String>()).collect::<Vec<_>>().join("\n")
        };
        let mut d = dlg(true, vec!["tetron-wm-essentials"]);
        let s = text(&d);
        assert!(s.contains("Installed: essentials, 2 missing") && s.contains("Remove set: essentials") && s.contains("[x] also"), "{s}");
        assert!(s.contains("3 to add"), "{s}");
        d.down();
        d.enter();
        d.set_download(Some(42_000_000));
        let s = text(&d);
        assert!(s.contains("Install Recommended: 3 apps to add, download about 42 MB"), "{s}");
        assert!(s.contains("opencode (its own installer)") && s.contains("[ Cancel ]"), "{s}");
    }
}
