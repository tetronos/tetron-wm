//! The app store: a browseable, searchable view over the bundled catalog
//! ([`crate::catalog`]). Renders itself into a window's content [`CellBuffer`]
//! and handles keyboard navigation. Installing an app is delegated to the
//! session (it spawns the install command in a PTY window).

use crate::buffer::CellBuffer;
use crate::catalog::{self, CatalogApp, Packages};
use crate::cell::{Cell, Rgba};
use crate::geometry::Point;

// Chrome colors follow the active desktop theme (were hardcoded midnight, which
// left the Store dark under every theme). Named `pal_*` to avoid colliding with
// local `bg`/`fg` bindings in the render code.
fn pal_bg() -> Rgba { crate::theme::current().window_bg }
fn pal_fg() -> Rgba { crate::theme::current().text }
fn pal_dim() -> Rgba { crate::theme::current().dim }
fn pal_sel() -> Rgba { crate::theme::current().active_bg }
fn pal_accent() -> Rgba { crate::theme::current().accent }
fn pal_panel() -> Rgba { crate::theme::current().menubar_bg }
// Semantic success color (e.g. "installed" markers): the theme's `success`.
fn green() -> Rgba { crate::theme::current().success }

const SIDEBAR_W: i32 = 16;
const LIST_W: i32 = 30;

/// Tag shown on apps flagged `cli` — a tool that prints output and exits (or
/// needs subcommands) rather than opening a persistent full-screen TUI.
const CLI_BADGE: &str = "CLI";

/// The Update list button on row 0 (WMCAT-004; F5 does the same).
const UPDATE_BTN: &str = "[ Update list ]";

/// What a click in the Store asks the session to do.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StoreClick {
    None,
    /// Install or launch the selected app.
    Activate,
    /// Fetch the catalog ([`crate::listfetch`]).
    UpdateList,
}

/// State of the store browser.
pub struct Store {
    categories: Vec<String>,
    cat_index: usize,
    query: String,
    selected: usize,
    scroll: usize,
    /// The Update list result line, shown on row 0 in place of the count.
    status: String,
}

impl Default for Store {
    fn default() -> Self {
        Self::new()
    }
}

impl Store {
    /// Create a store browser (category list derived from the catalog).
    pub fn new() -> Self {
        Self { categories: Self::category_list(), cat_index: 0, query: String::new(), selected: 0, scroll: 0, status: String::new() }
    }

    fn category_list() -> Vec<String> {
        let mut cats: Vec<String> = Vec::new();
        for c in catalog::catalog() {
            if !cats.iter().any(|x| x == &c.category) {
                cats.push(c.category.clone());
            }
        }
        cats.sort();
        cats.insert(0, "All".to_string());
        cats
    }

    /// An Update list press started.
    pub fn update_started(&mut self) {
        self.status = "Updating list\u{2026}".into();
    }

    /// An Update list finished with `msg`: show it and re-read the catalog in
    /// use, keeping the selected category when it still exists.
    pub fn update_done(&mut self, msg: &str) {
        self.status = msg.to_string();
        let cat = self.categories.get(self.cat_index).cloned();
        self.categories = Self::category_list();
        self.cat_index = cat.and_then(|c| self.categories.iter().position(|x| *x == c)).unwrap_or(0);
        self.reset_list();
    }

    fn update_btn_x(w: i32) -> i32 {
        (w - UPDATE_BTN.len() as i32 - 1).max(0)
    }

    /// Apps matching the current category + query.
    pub fn filtered(&self) -> Vec<&'static CatalogApp> {
        let cat = &self.categories[self.cat_index];
        let q = self.query.to_lowercase();
        catalog::catalog()
            .iter()
            .filter(|c| {
                catalog::runs_on_current_os(&c.name)
                    && (cat == "All" || &c.category == cat)
                    && (q.is_empty()
                        || c.name.to_lowercase().contains(&q)
                        || c.description.to_lowercase().contains(&q))
            })
            .collect()
    }

    /// The currently highlighted app.
    pub fn selected_app(&self) -> Option<&'static CatalogApp> {
        self.filtered().into_iter().nth(self.selected)
    }

    pub fn move_up(&mut self) {
        self.selected = self.selected.saturating_sub(1);
    }
    pub fn move_down(&mut self) {
        let n = self.filtered().len();
        if n > 0 && self.selected + 1 < n {
            self.selected += 1;
        }
    }
    pub fn prev_category(&mut self) {
        if self.cat_index > 0 {
            self.cat_index -= 1;
            self.reset_list();
        }
    }
    pub fn next_category(&mut self) {
        if self.cat_index + 1 < self.categories.len() {
            self.cat_index += 1;
            self.reset_list();
        }
    }
    pub fn type_char(&mut self, c: char) {
        self.query.push(c);
        self.reset_list();
    }
    pub fn backspace(&mut self) {
        self.query.pop();
        self.reset_list();
    }
    fn reset_list(&mut self) {
        self.selected = 0;
        self.scroll = 0;
    }

    /// Render the store into a `w × h` content buffer.
    pub fn render(&self, w: i32, h: i32) -> CellBuffer {
        let mut buf = CellBuffer::new(w, h);
        buf.fill(Cell { ch: ' ', fg: pal_fg(), bg: pal_bg(), attrs: Default::default() });

        // Search bar (row 0).
        buf.write_str(1, 0, &format!("{} {}\u{2588}", crate::glyphs::get(crate::glyphs::Glyph::Search), self.query), pal_accent(), pal_bg());
        let bx = Self::update_btn_x(w);
        buf.write_str(bx, 0, UPDATE_BTN, pal_accent(), pal_bg());
        let meta = if self.status.is_empty() { format!("{} apps", self.filtered().len()) } else { self.status.clone() };
        let room = (bx - 2 - SIDEBAR_W).max(0) as usize;
        let meta = truncate(&meta, room);
        buf.write_str((bx - 2 - meta.chars().count() as i32).max(0), 0, meta, pal_dim(), pal_bg());
        hline(&mut buf, w, 1);

        // Sidebar (categories).
        for (i, cat) in self.categories.iter().enumerate() {
            let y = 2 + i as i32;
            if y >= h {
                break;
            }
            let sel = i == self.cat_index;
            let (fg, bg) = if sel { (pal_accent(), pal_sel()) } else { (pal_dim(), pal_bg()) };
            for x in 0..SIDEBAR_W {
                buf.set(x, y, Cell { ch: ' ', fg, bg, attrs: Default::default() });
            }
            buf.write_str(1, y, truncate(cat, SIDEBAR_W as usize - 2), fg, bg);
        }
        vline(&mut buf, SIDEBAR_W, 2, h);

        // App list.
        let list_x = SIDEBAR_W + 1;
        let rows = (h - 2).max(0) as usize;
        let apps = self.filtered();
        let scroll = self.scroll_for(rows);
        for (row, app) in apps.iter().skip(scroll).take(rows).enumerate() {
            let idx = scroll + row;
            let y = 2 + row as i32;
            let sel = idx == self.selected;
            let installed = catalog::is_installed(&app.bin);
            let flags = ListRowFlags { installed, cli: app.cli, sel };
            draw_list_row(&mut buf, list_x, y, LIST_W, &app.name, flags);
        }
        vline(&mut buf, list_x + LIST_W, 2, h);

        // Detail pane.
        let dx = list_x + LIST_W + 1;
        let dw = (w - dx - 1).max(0);
        if let Some(app) = self.selected_app() {
            for y in 2..h {
                for x in dx..w {
                    buf.set(x, y, Cell { ch: ' ', fg: pal_fg(), bg: pal_panel(), attrs: Default::default() });
                }
            }
            buf.write_str(dx + 1, 3, truncate(&app.name, dw as usize - 2), pal_accent(), pal_panel());
            let cat_str = truncate(&app.category, dw as usize - 2);
            buf.write_str(dx + 1, 4, cat_str, pal_dim(), pal_panel());
            if app.cli {
                let bx = dx + 1 + cat_str.chars().count() as i32 + 2;
                if bx + CLI_BADGE.len() as i32 <= dx + dw {
                    buf.write_str(bx, 4, CLI_BADGE, pal_accent(), pal_panel());
                }
            }
            // The suite size that brings it (WMCAT-003).
            if let Some(set) = &app.set {
                buf.write_str(dx + 1, 5, truncate(&format!("In the {set} set (Settings > General > Apps)"), dw as usize - 2), pal_dim(), pal_panel());
            }
            let mut y = 6;
            for line in wrap(&app.description, dw as usize - 2) {
                if y >= h - 5 {
                    break;
                }
                buf.write_str(dx + 1, y, &line, pal_fg(), pal_panel());
                y += 1;
            }
            // Setup tip (e.g. "add models/providers with `hermes model` first").
            if let Some(r) = catalog::recipe(&app.name) {
                if !r.tip.is_empty() {
                    y += 1;
                    for line in wrap(&format!("Setup: {}", r.tip), dw as usize - 2) {
                        if y >= h - 5 {
                            break;
                        }
                        buf.write_str(dx + 1, y, &line, pal_accent(), pal_panel());
                        y += 1;
                    }
                }
            }
            buf.write_str(dx + 1, h - 4, truncate(&app.homepage, dw as usize - 2), pal_dim(), pal_panel());
            // Verified-recipe badge.
            if let Some(r) = catalog::recipe(&app.name) {
                if r.verified {
                    buf.write_str(dx + 1, h - 3, &format!("{} verified \u{00B7} {}", crate::glyphs::get(crate::glyphs::Glyph::Check), r.method), green(), pal_panel());
                }
            }
            // Action hint.
            let installed = catalog::is_installed(&app.bin);
            let action = if installed { "[ Enter: Launch ]" } else { "[ Enter: Install ]" };
            let acol = if installed { green() } else { pal_accent() };
            buf.write_str(dx + 1, h - 2, action, acol, pal_panel());
        }

        buf
    }

    /// Handle a click at content-local point `p` (within a `w × h` content area).
    ///
    /// Activate (install/launch the selected app) on the action button or a
    /// second click on the selected row; UpdateList on the row 0 button.
    pub fn handle_click(&mut self, p: Point, w: i32, h: i32) -> StoreClick {
        if p.y == 0 && p.x >= Self::update_btn_x(w) && p.x < Self::update_btn_x(w) + UPDATE_BTN.len() as i32 {
            return StoreClick::UpdateList;
        }
        if p.y < 2 {
            return StoreClick::None; // search bar / divider
        }
        // Category sidebar.
        if p.x < SIDEBAR_W {
            let i = (p.y - 2) as usize;
            if i < self.categories.len() {
                self.cat_index = i;
                self.reset_list();
            }
            return StoreClick::None;
        }
        // App list.
        let list_x = SIDEBAR_W + 1;
        if p.x >= list_x && p.x < list_x + LIST_W {
            let rows = (h - 2).max(0) as usize;
            let idx = self.scroll_for(rows) + (p.y - 2) as usize;
            if idx < self.filtered().len() {
                if idx == self.selected {
                    return StoreClick::Activate; // second click on the selected row activates
                }
                self.selected = idx;
            }
            return StoreClick::None;
        }
        // Detail pane action button (bottom).
        let dx = list_x + LIST_W + 1;
        if p.x >= dx && p.y == h - 2 {
            return StoreClick::Activate;
        }
        StoreClick::None
    }

    /// Scroll offset that keeps the selection visible given `rows` visible lines.
    fn scroll_for(&self, rows: usize) -> usize {
        if rows == 0 {
            return 0;
        }
        if self.selected < self.scroll {
            self.selected
        } else if self.selected >= self.scroll + rows {
            self.selected + 1 - rows
        } else {
            self.scroll
        }
    }
}

/// A package name the Store may put in a shell command: letters, digits and
/// `. _ + - / @` only (the catalog is downloaded; anything else is ignored).
fn pkg_name(n: &str) -> Option<&str> {
    let ok = !n.is_empty() && !n.starts_with('-') && n.chars().all(|c| c.is_ascii_alphanumeric() || "._+-/@".contains(c));
    ok.then_some(n)
}

/// `sudo` unless already root, as `$S` for the package manager branches.
const SUDO_VAR: &str = "S=sudo; [ \"$(id -u)\" -eq 0 ] && S=; ";

/// The host package manager branches for `p` (WMCAT-005), host order: Homebrew
/// on macOS, then apt, dnf, pacman. Each is `(condition, command)`; a condition
/// holds when that manager exists and has the package, so a name missing on
/// this release falls through to the next way.
fn native_branches(p: &Packages) -> Vec<(String, String)> {
    let mut v = Vec::new();
    if let Some(n) = pkg_name(&p.brew) {
        v.push((
            format!("[ \"$(uname -s)\" = Darwin ] && {{ {}; command -v brew >/dev/null 2>&1; }}", crate::toolchain::BREW_ENV),
            format!("brew install {n}"),
        ));
    }
    if let Some(n) = pkg_name(&p.apt) {
        v.push((format!("command -v apt-get >/dev/null 2>&1 && apt-cache show {n} >/dev/null 2>&1"), format!("$S apt-get install -y {n}")));
    }
    if let Some(n) = pkg_name(&p.dnf) {
        v.push((format!("command -v dnf >/dev/null 2>&1 && dnf -q info {n} >/dev/null 2>&1"), format!("$S dnf install -y {n}")));
    }
    if let Some(n) = pkg_name(&p.pacman) {
        v.push((format!("command -v pacman >/dev/null 2>&1 && pacman -Si {n} >/dev/null 2>&1"), format!("$S pacman -S --needed --noconfirm {n}")));
    }
    v
}

/// The install command for an app (run in a shell window), WMCAT-005: the
/// host's own package manager when it has the app (Homebrew on macOS; apt, dnf
/// or pacman on Linux), else the verified upstream recipe (binary, cargo, go,
/// uv, …), else Homebrew on Linux as the last resort, else a best-effort chain.
/// It always ends by naming the homepage so the user can finish by hand.
pub fn install_command(app: &CatalogApp) -> String {
    let done = format!(
        "echo; echo '────────'; echo 'Done. If it succeeded, {name} is now in your launcher.'; \
echo 'Close this window (x) when finished.'; exec \"$SHELL\"",
        name = app.name
    );
    let recipe = app.recipe.as_ref().filter(|r| r.verified && !r.install.is_empty());
    // The upstream way: a verified recipe (its toolchain offered first when
    // missing), or Homebrew by name; None leaves the best-effort chain.
    let brew_by_name = pkg_name(&app.pkg.brew).map(|n| format!("brew install {n}"));
    let upstream = match (recipe, brew_by_name) {
        (Some(r), _) => Some((r.method.clone(), r.install.clone())),
        (None, Some(b)) => Some(("brew".to_string(), b)),
        (None, None) => None,
    };
    let upstream = upstream.map(|(method, cmd)| {
        let pre = crate::toolchain::preamble(&app.name, &method, &app.homepage);
        format!("{pre}{cmd}")
    });
    let native = native_branches(&app.pkg);
    if upstream.is_none() && native.is_empty() {
        return fallback_command(app);
    }
    let mut sh = format!("clear; {SUDO_VAR}echo 'Installing {name} …'; echo; ", name = app.name);
    if native.is_empty() {
        sh.push_str(&format!("{}; ", upstream.unwrap()));
    } else {
        for (i, (cond, cmd)) in native.iter().enumerate() {
            sh.push_str(&format!("{} {cond}; then {cmd}; ", if i == 0 { "if" } else { "elif" }));
        }
        let rest = upstream.unwrap_or_else(|| {
            format!("echo 'No package for {name} here. Get it from:'; echo '  {home}'", name = app.name, home = app.homepage)
        });
        sh.push_str(&format!("else {rest}; fi; "));
    }
    sh.push_str(&done);
    sh
}

/// The best-effort chain for an app with no package names and no verified
/// recipe: the host's manager by command name, then `cargo install`, then
/// `go install <repo>@latest`, then Homebrew when installed.
fn fallback_command(app: &CatalogApp) -> String {
    let gopath = app
        .homepage
        .trim_start_matches("https://")
        .trim_start_matches("http://")
        .trim_end_matches('/');
    let bin = pkg_name(&app.bin).unwrap_or("");
    let native: String = if bin.is_empty() {
        String::new()
    } else {
        native_branches(&Packages { apt: bin.into(), dnf: bin.into(), pacman: bin.into(), brew: String::new() })
            .into_iter()
            .map(|(c, i)| format!("( {c} && {i} ) || "))
            .collect()
    };
    format!(
        "clear; {SUDO_VAR}echo 'Installing {name} — trying the package manager, then cargo, go and brew…'; echo; \
{native}cargo install {bin} \
|| go install {gopath}@latest \
|| ( {brew_env}; command -v brew >/dev/null 2>&1 && brew install {bin} ) \
|| true; \
echo; echo '────────'; \
echo 'If nothing installed automatically, get {name} from:'; echo '  {home}'; \
echo; echo 'Close this window (x) when done.'; exec \"$SHELL\"",
        name = app.name,
        brew_env = crate::toolchain::BREW_ENV,
        home = app.homepage,
    )
}

/// Per-row flags for [`draw_list_row`], bundled into one argument to keep the
/// function's parameter count clippy-clean.
struct ListRowFlags {
    /// Whether the app is on `$PATH` (shows the green checkmark).
    installed: bool,
    /// Whether a right-aligned `CLI` tag is drawn (app flagged as a CLI tool).
    cli: bool,
    /// Whether this row is the current selection (swaps to the highlight bg).
    sel: bool,
}

/// Draw one app-list row spanning `w` cells from `x` per `flags` (see
/// [`ListRowFlags`]): the install checkmark, the (possibly truncated) name,
/// and — for CLI-flagged apps (prints output and exits / needs subcommands,
/// rather than a persistent TUI) — a right-aligned `CLI` tag.
fn draw_list_row(buf: &mut CellBuffer, x: i32, y: i32, w: i32, name: &str, flags: ListRowFlags) {
    let ListRowFlags { installed, cli, sel } = flags;
    let bg = if sel { pal_sel() } else { pal_bg() };
    for dx in 0..w {
        buf.set(x + dx, y, Cell { ch: ' ', fg: pal_fg(), bg, attrs: Default::default() });
    }
    let mark = if installed { format!("{} ", crate::glyphs::get(crate::glyphs::Glyph::Check)) } else { "  ".to_string() };
    buf.write_str(x, y, &mark, green(), bg);
    let badge_w = if cli { CLI_BADGE.len() as i32 + 1 } else { 0 };
    let name_w = (w - 3 - badge_w).max(1) as usize;
    buf.write_str(x + 2, y, truncate(name, name_w), pal_fg(), bg);
    if cli {
        buf.write_str(x + w - CLI_BADGE.len() as i32, y, CLI_BADGE, pal_accent(), bg);
    }
}

fn hline(buf: &mut CellBuffer, w: i32, y: i32) {
    for x in 0..w {
        buf.set(x, y, Cell { ch: '\u{2500}', fg: pal_dim(), bg: pal_bg(), attrs: Default::default() });
    }
}
fn vline(buf: &mut CellBuffer, x: i32, y0: i32, h: i32) {
    for y in y0..h {
        buf.set(x, y, Cell { ch: '\u{2502}', fg: pal_dim(), bg: pal_bg(), attrs: Default::default() });
    }
}
fn truncate(s: &str, max: usize) -> &str {
    match s.char_indices().nth(max) {
        Some((i, _)) => &s[..i],
        None => s,
    }
}
fn wrap(s: &str, width: usize) -> Vec<String> {
    let mut lines = Vec::new();
    let mut line = String::new();
    for word in s.split_whitespace() {
        if line.chars().count() + word.chars().count() + 1 > width && !line.is_empty() {
            lines.push(std::mem::take(&mut line));
        }
        if !line.is_empty() {
            line.push(' ');
        }
        line.push_str(word);
    }
    if !line.is_empty() {
        lines.push(line);
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row_text(buf: &CellBuffer, y: i32, w: i32) -> String {
        (0..w).map(|x| buf.get(x, y).map(|c| c.ch).unwrap_or(' ')).collect()
    }

    /// A `cli`-flagged app's list row shows the "CLI" tag; an unflagged one
    /// doesn't (the badge shown in the store's app list — see `render`).
    #[test]
    fn cli_badge_shown_only_on_flagged_row() {
        let mut buf = CellBuffer::new(30, 2);
        draw_list_row(&mut buf, 0, 0, 30, "himalaya", ListRowFlags { installed: true, cli: true, sel: false });
        draw_list_row(&mut buf, 0, 1, 30, "btop", ListRowFlags { installed: true, cli: false, sel: false });
        assert!(row_text(&buf, 0, 30).contains("CLI"), "flagged row should show the badge");
        assert!(!row_text(&buf, 1, 30).contains("CLI"), "unflagged row should not show the badge");
    }

    /// Row 0 carries the Update list button; a click on it asks for the fetch,
    /// and the result replaces the app count (WMCAT-004).
    #[test]
    fn update_list_button_on_row_zero_and_result_shown() {
        let mut st = Store::new();
        let (w, h) = (100, 30);
        let row0 = row_text(&st.render(w, h), 0, w);
        assert!(row0.contains(UPDATE_BTN) && row0.contains(" apps"), "{row0}");
        let bx = Store::update_btn_x(w);
        assert_eq!(st.handle_click(Point { x: bx + 2, y: 0 }, w, h), StoreClick::UpdateList);
        assert_eq!(st.handle_click(Point { x: bx - 3, y: 0 }, w, h), StoreClick::None);
        st.update_started();
        assert!(row_text(&st.render(w, h), 0, w).contains("Updating list"));
        st.update_done("List updated: 688 apps");
        assert!(row_text(&st.render(w, h), 0, w).contains("List updated: 688 apps"));
    }

    /// An app in a suite set says so in the detail pane (WMCAT-003).
    #[test]
    fn detail_pane_names_the_set() {
        let mut st = Store::new();
        st.query = "fresh".into();
        st.reset_list();
        let i = st.filtered().iter().position(|a| a.bin == "fresh").expect("fresh listed");
        st.selected = i;
        let buf = st.render(110, 30);
        assert!((0..30).any(|y| row_text(&buf, y, 110).contains("In the essentials set")));
    }

    /// The command parses as POSIX shell (`sh -n`).
    fn parses(c: &str) {
        let ok = std::process::Command::new("sh").args(["-n", "-c", c]).status().expect("sh").success();
        assert!(ok, "not valid sh: {c}");
    }

    fn app(json: &str) -> CatalogApp {
        let base = r#"{"name":"demo","bin":"demo","category":"X","description":"d","homepage":"https://github.com/o/demo""#;
        serde_json::from_str(&format!("{base}{json}}}")).expect("test app")
    }

    /// WMCAT-005: the host's manager first (brew only on macOS), then the
    /// verified upstream recipe; Homebrew on Linux only when nothing else.
    #[test]
    fn host_manager_first_then_upstream_recipe() {
        let a = app(r#","pkg":{"apt":"demo-tui","dnf":"demo","pacman":"demo","brew":"demo"},
            "recipe":{"install":"cargo install demo","method":"cargo","verified":true}"#);
        let c = install_command(&a);
        let (darwin, apt, dnf, pac, up) = (
            c.find("= Darwin").unwrap(),
            c.find("apt-cache show demo-tui").unwrap(),
            c.find("dnf -q info demo").unwrap(),
            c.find("pacman -Si demo").unwrap(),
            c.find("else ").unwrap(),
        );
        parses(&c);
        assert!(darwin < apt && apt < dnf && dnf < pac && pac < up, "{c}");
        assert!(c.contains("$S apt-get install -y demo-tui") && c[up..].contains("cargo install demo"), "{c}");
        assert!(c.contains("S=sudo; [ \"$(id -u)\" -eq 0 ] && S=;"), "root needs no sudo: {c}");
        assert!(!c[up..].contains("brew install"), "Linux reaches no brew when upstream exists: {c}");
    }

    /// A brew-only app on Linux: Homebrew is offered and loaded into the
    /// shell after its installer, so the brew install in the same shell works.
    #[test]
    fn brew_last_resort_loads_brew_into_the_shell() {
        let a = app(r#","recipe":{"install":"brew install o/tap/demo","method":"brew","verified":true}"#);
        let c = install_command(&a);
        parses(&c);
        assert!(c.contains("Homebrew/install") && c.contains("brew install o/tap/demo"), "{c}");
        let boot = c.find("Homebrew/install").unwrap();
        assert!(c[boot..].contains("shellenv"), "brew put on PATH after its installer: {c}");
        assert!(c.find("shellenv").unwrap() < c.find("command -v brew").unwrap(), "an installed brew off PATH is used first: {c}");
    }

    /// Package names from the downloaded catalog never reach the shell unless
    /// they are plain names; a distro-only app without recipe names the homepage.
    #[test]
    fn unsafe_package_names_are_ignored() {
        let a = app(r#","pkg":{"apt":"x; rm -rf ~","pacman":"-Syu","dnf":"demo"}"#);
        let c = install_command(&a);
        parses(&c);
        assert!(!c.contains("rm -rf") && !c.contains("-Syu"), "{c}");
        assert!(c.contains("dnf install -y demo") && c.contains("No package for demo here"), "{c}");
    }

    /// No names, no recipe: the host manager by command name, then cargo, go,
    /// and brew only when it is installed (never bootstrapped).
    #[test]
    fn fallback_tries_host_manager_before_cargo_go_brew() {
        let c = install_command(&app(""));
        let (apt, cargo, go, brew) = (
            c.find("apt-cache show demo").unwrap(),
            c.find("cargo install demo").unwrap(),
            c.find("go install github.com/o/demo@latest").unwrap(),
            c.find("brew install demo").unwrap(),
        );
        parses(&c);
        assert!(apt < cargo && cargo < go && go < brew, "{c}");
        assert!(!c.contains("Homebrew/install"), "{c}");
    }
}
