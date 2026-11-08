//! The menubar status tray: indicator segments + click-through popovers.

use crate::geometry::Rect;
use crate::system::{bars_glyph, mem_pct, SystemState};

/// Map a volume reading to its status icon (muted/low/high).
fn volume_status(v: &crate::system::VolumeInfo) -> crate::iconset::Status {
    use crate::iconset::Status;
    if v.muted || v.level == 0 {
        Status::VolMute
    } else if v.level < 66 {
        Status::VolLow
    } else {
        Status::VolHigh
    }
}

/// Which indicator a tray segment represents (used for hit-testing + drop order).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SegmentKind {
    Cpu,
    Mem,
    Battery,
    Volume,
    /// Screen brightness: only on a machine with a backlight.
    Brightness,
    Bluetooth,
    Wifi,
    Tetron,
    Select,
    Clipboard,
    Bell,
    /// The `ⓘ` menu: session info, links, and the segments placed in (or
    /// overflowed into) it.
    Info,
    Clock,
}

impl SegmentKind {
    /// Config / intent key of a segment that can live in the Info menu
    /// (`[tray_placement]`); `None` for the always-on-bar kinds.
    pub fn key(self) -> Option<&'static str> {
        Some(match self {
            SegmentKind::Cpu => "cpu",
            SegmentKind::Mem => "mem",
            SegmentKind::Battery => "battery",
            SegmentKind::Bluetooth => "bluetooth",
            SegmentKind::Wifi => "wifi",
            SegmentKind::Tetron => "tetron",
            SegmentKind::Bell => "bell",
            _ => return None,
        })
    }

    /// Inverse of [`SegmentKind::key`].
    pub fn from_key(key: &str) -> Option<SegmentKind> {
        Some(match key {
            "cpu" => SegmentKind::Cpu,
            "mem" => SegmentKind::Mem,
            "battery" => SegmentKind::Battery,
            "bluetooth" => SegmentKind::Bluetooth,
            "wifi" => SegmentKind::Wifi,
            "tetron" => SegmentKind::Tetron,
            "bell" => SegmentKind::Bell,
            _ => return None,
        })
    }

    /// Human label for the segment's row in the Info menu.
    fn label(self) -> &'static str {
        match self {
            SegmentKind::Cpu => "CPU",
            SegmentKind::Mem => "Memory",
            SegmentKind::Battery => "Battery",
            SegmentKind::Bluetooth => "Bluetooth",
            SegmentKind::Wifi => "Network",
            SegmentKind::Tetron => "Tetron",
            SegmentKind::Bell => "Alerts",
            _ => "",
        }
    }
}

/// Where a segment is shown: pinned to the bar, in the Info menu, or on the bar
/// but allowed to overflow into the Info menu when space runs out.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Place {
    /// On the bar; overflows into Info only after every `Auto` segment has.
    Bar,
    /// Always in the Info menu (a hot CPU / memory is shown on the bar anyway).
    Info,
    /// On the bar, overflows into the Info menu when space runs out.
    Auto,
}

/// A CPU or memory reading at or above this percentage is shown on the bar even
/// when placed in Info, so a problem is visible without opening the menu.
pub const HOT_PCT: u32 = 90;

/// Per-segment placement (`[tray_placement]` in the config).
#[derive(Clone, Debug, Default)]
pub struct Placement(std::collections::BTreeMap<String, String>);

impl Placement {
    /// Build from the config table (`cpu = "info"`, ...). Unknown values fall
    /// back to the segment's default.
    pub fn from_config(map: &std::collections::BTreeMap<String, String>) -> Self {
        Placement(map.clone())
    }

    /// Every segment on the bar (what the plain `tray_segments*` helpers use).
    pub fn all_bar() -> Self {
        let mut m = std::collections::BTreeMap::new();
        for k in ["cpu", "mem", "battery", "bluetooth", "wifi", "tetron", "bell"] {
            m.insert(k.to_string(), "bar".to_string());
        }
        Placement(m)
    }

    /// The effective placement of `kind`. Defaults: CPU, memory and Bluetooth in
    /// Info; everything else `auto`.
    pub fn get(&self, kind: SegmentKind) -> Place {
        let Some(key) = kind.key() else { return Place::Bar };
        match self.0.get(key).map(|v| v.trim().to_ascii_lowercase()).as_deref() {
            Some("bar") => Place::Bar,
            Some("info") => Place::Info,
            Some("auto") => Place::Auto,
            _ => match kind {
                SegmentKind::Cpu | SegmentKind::Mem | SegmentKind::Bluetooth => Place::Info,
                _ => Place::Auto,
            },
        }
    }
}

/// A segment that lives in the Info menu: placed there, or overflowed into it.
#[derive(Clone, Debug, PartialEq)]
pub struct InfoRow {
    pub kind: SegmentKind,
    pub label: &'static str,
    /// The segment's bar text (icon + value), shown as the row's readout.
    pub text: String,
}

/// The tray layout: what is on the bar and what lives in the Info menu.
#[derive(Clone, Debug, Default)]
pub struct TrayLayout {
    pub segments: Vec<Segment>,
    pub in_info: Vec<InfoRow>,
}

pub use crate::traytools::ToolAction;

/// The `[tray_tools]` item key for a segment, if its popover has an Open row.
fn tool_item(kind: SegmentKind) -> Option<&'static str> {
    match kind {
        SegmentKind::Cpu => Some("cpu"),
        SegmentKind::Mem => Some("mem"),
        SegmentKind::Battery => Some("battery"),
        SegmentKind::Wifi => Some("network"),
        SegmentKind::Bluetooth => Some("bluetooth"),
        SegmentKind::Volume => Some("volume"),
        _ => None,
    }
}

/// The tool a segment's popover offers, or `None` for no action row. `installed`
/// answers "is this binary on `$PATH`" so the choice is testable without
/// touching the host; `cfg` is the user's `[tray_tools]` override.
pub fn segment_app(
    kind: SegmentKind,
    cfg: &crate::traytools::TrayTools,
    installed: &dyn Fn(&str) -> bool,
) -> Option<ToolAction> {
    crate::traytools::pick(tool_item(kind)?, cfg, installed)
}

/// A laid-out tray segment: its kind, display text, and screen rect (row 0).
#[derive(Clone, Debug, PartialEq)]
pub struct Segment {
    pub kind: SegmentKind,
    pub text: String,
    pub rect: Rect,
}

/// Columns of space between adjacent segments.
const GAP: i32 = 2;

/// Build the right-aligned, ordered list of tray segments for a `width`-wide
/// menubar, leaving `reserve` columns free on the right for the power button
/// (whose label is the host name, so its width varies per machine). Lowest-
/// priority segments (CPU, then Memory, then Battery) drop out first when
/// space is tight; the clock is always kept.
pub fn tray_segments(state: &SystemState, width: i32, reserve: i32, bells: usize) -> Vec<Segment> {
    tray_segments_with(state, width, reserve, bells, None, false)
}

/// Like [`tray_segments`], plus the clipboard segment when `clip` is
/// `Some((entries, paused))` (clipboard history enabled). Every segment is on
/// the bar (see [`Placement::all_bar`]); the session uses [`tray_layout`].
pub fn tray_segments_with(
    state: &SystemState,
    width: i32,
    reserve: i32,
    bells: usize,
    clip: Option<(usize, bool)>,
    select_mode: bool,
) -> Vec<Segment> {
    tray_layout(state, width, reserve, bells, clip, select_mode, &Placement::all_bar()).segments
}

/// Lay out the tray: the right-aligned bar segments and the rows of the Info
/// menu. Segments placed in Info, and segments that do not fit the bar (shed,
/// lowest priority first), become Info rows instead of disappearing. Volume,
/// the clock, the clipboard, the SELECT marker and Info itself are never shed.
pub fn tray_layout(
    state: &SystemState,
    width: i32,
    reserve: i32,
    bells: usize,
    clip: Option<(usize, bool)>,
    select_mode: bool,
    placement: &Placement,
) -> TrayLayout {
    // Display order, left→right.
    use crate::iconset::{status, Status};
    let mut texts: Vec<(SegmentKind, String)> = Vec::new();
    let mut info: Vec<(SegmentKind, String)> = Vec::new();
    let cpu = state.cpu_pct.round() as u32;
    let mem = mem_pct(state.mem.used, state.mem.total) as u32;
    // A hot CPU / memory is pinned to the bar even when placed in Info.
    fn put(
        texts: &mut Vec<(SegmentKind, String)>,
        info: &mut Vec<(SegmentKind, String)>,
        placement: &Placement,
        kind: SegmentKind,
        text: String,
        hot: bool,
    ) {
        if placement.get(kind) == Place::Info && !hot {
            info.push((kind, text));
        } else {
            texts.push((kind, text));
        }
    }
    // A space follows each glyph so a double-width Nerd Font icon does not overlap
    // the metric (same convention as the file manager's list view).
    put(&mut texts, &mut info, placement, SegmentKind::Cpu, format!("{} {}%", status(Status::Cpu), cpu), cpu >= HOT_PCT);
    put(&mut texts, &mut info, placement, SegmentKind::Mem, format!("{} {}%", status(Status::Mem), mem), mem >= HOT_PCT);
    if let Some(b) = &state.battery {
        let g = status(if b.charging { Status::BatteryCharging } else { Status::Battery });
        put(&mut texts, &mut info, placement, SegmentKind::Battery, format!("{} {}%", g, b.pct), false);
    }
    if state.caps.bluetooth || state.bluetooth.enabled {
        put(&mut texts, &mut info, placement, SegmentKind::Bluetooth, format!("{} bt", status(Status::Bluetooth)), false);
    }
    // Network: `wifi` (with signal bars) when the primary link is WiFi, `net`
    // for any other link (ethernet, tether, VPN). The SSID lives in the popover.
    if state.wifi.is_some() || state.net.is_some() {
        let on_wifi = state.net.as_ref().map(|n| n.wifi).unwrap_or(true);
        if on_wifi {
            let sig = state.wifi.as_ref().map(|w| w.signal).unwrap_or(0);
            put(&mut texts, &mut info, placement, SegmentKind::Wifi, format!("{} wifi", bars_glyph(sig)), false);
        } else {
            put(&mut texts, &mut info, placement, SegmentKind::Wifi, format!("{} net", status(Status::Net)), false);
        }
    }
    // tetron: only when the daemon answered (installed + running). `⇄{peers}`
    // with a direct(●)/relay(○) dot, or `off` on reachable standby.
    if state.tetron.reachable {
        let connected: usize = state.tetron.networks.iter().map(|n| n.connected).sum();
        let any_direct = state.tetron.networks.iter().any(|n| n.any_direct);
        let dot = if !state.tetron.active {
            "off"
        } else if any_direct {
            "●"
        } else {
            crate::glyphs::get(crate::glyphs::Glyph::Off)
        };
        put(&mut texts, &mut info, placement, SegmentKind::Tetron, format!("{} {connected} {dot}", status(Status::Net)), false);
    }
    if select_mode {
        texts.push((SegmentKind::Select, "SELECT".to_string()));
    }
    if let Some((n, paused)) = clip {
        let tail = if paused { crate::glyphs::get(crate::glyphs::Glyph::Paused).to_string() } else { n.to_string() }; // ‖ when paused
        texts.push((SegmentKind::Clipboard, format!("{} {tail}", status(Status::Clipboard))));
    }
    if bells > 0 {
        put(&mut texts, &mut info, placement, SegmentKind::Bell, format!("{} {bells}", status(Status::Bell)), false);
    }
    // Brightness, left of Info: a laptop control, never moved into the Info
    // menu; fixed width so nothing shifts as it changes.
    if let Some(p) = state.brightness {
        texts.push((SegmentKind::Brightness, format!("{} {p:>3}", status(Status::Brightness))));
    }
    // The Info menu: fixed width, always present, a fixed distance from the right
    // corner (only the fixed-width clock and volume are right of it).
    texts.push((SegmentKind::Info, format!("{} ", crate::glyphs::get(crate::glyphs::Glyph::Info))));
    // Clock is time-only ("09:41"); one click on it drops down the full calendar.
    texts.push((SegmentKind::Clock, state.clock.time.clone()));
    // Volume sits at the far right, past the clock — a fixed corner the user can
    // shove the pointer into and scroll to change volume without looking.
    texts.push((
        SegmentKind::Volume,
        // Fixed 3-wide level so the segment width never changes (5 → 100): the
        // corner stays put and nothing to its left shifts as the volume moves.
        format!("{} {:>3}", status(volume_status(&state.volume)), state.volume.level),
    ));

    // When out of space, move low-priority segments into the Info menu. Volume,
    // the clock, the clipboard, SELECT and Info are never in this list. The tray
    // must never reach into the left-hand chrome (brand, mode, assistant, "+"):
    // a segment is shed as soon as the tray's FIRST (leftmost) cell would land
    // left of `TASKBAR_X`, not only once the whole item has been pushed off.
    let avail = width - reserve - crate::chrome::TASKBAR_X;
    let total = |v: &[(SegmentKind, String)]| -> i32 {
        let widths: i32 = v.iter().map(|(_, t)| t.chars().count() as i32).sum();
        widths + GAP * (v.len() as i32 - 1).max(0)
    };
    // `Bar` segments go last, after every `Auto` one.
    let mut order = [
        SegmentKind::Cpu,
        SegmentKind::Mem,
        SegmentKind::Battery,
        SegmentKind::Bell,
        SegmentKind::Tetron,
        SegmentKind::Wifi,
        SegmentKind::Bluetooth,
    ];
    order.sort_by_key(|k| placement.get(*k) == Place::Bar);
    for k in order {
        if total(&texts) <= avail {
            break;
        }
        if let Some(i) = texts.iter().position(|(sk, _)| *sk == k) {
            info.push(texts.remove(i));
        }
    }

    // Right-align: lay out from the right edge leftward, then return left→right.
    // The rightmost segment reaches the final column (no trailing gap) so a scroll
    // at the very edge still lands on Volume.
    let mut x = width - reserve;
    let mut out: Vec<Segment> = Vec::new();
    for (kind, text) in texts.iter().rev() {
        let w = text.chars().count() as i32;
        x -= w;
        out.push(Segment { kind: *kind, text: text.clone(), rect: Rect::new(x, 0, w, 1) });
        x -= GAP;
    }
    out.reverse();
    // Info rows in the bar's left→right order (CPU, memory, battery, …).
    let rank = |k: SegmentKind| [
        SegmentKind::Cpu,
        SegmentKind::Mem,
        SegmentKind::Battery,
        SegmentKind::Bluetooth,
        SegmentKind::Wifi,
        SegmentKind::Tetron,
        SegmentKind::Bell,
    ].iter().position(|x| *x == k).unwrap_or(usize::MAX);
    info.sort_by_key(|(k, _)| rank(*k));
    let in_info = info
        .into_iter()
        .map(|(kind, text)| InfoRow { kind, label: kind.label(), text })
        .collect();
    TrayLayout { segments: out, in_info }
}

// ── Interactive tray: open popover state + rendering + hit-testing ─────────────

use crate::buffer::CellBuffer;
use crate::cell::Cell;
use crate::compositor::Layer;
use crate::geometry::Point;
use crate::system::{ControlIntent, SystemState as St};

/// A clickable hot-zone inside an open popover: its screen rect and the intent
/// it produces when clicked.
#[derive(Clone, Debug, PartialEq)]
pub struct PopoverHit {
    pub rect: Rect,
    pub intent: ControlIntent,
}

/// A rendered popover: compositor layers, clickable hits, and the popover's outer
/// bounds (so the session can detect clicks outside it).
#[derive(Default)]
pub struct Rendered {
    pub layers: Vec<Layer>,
    pub hits: Vec<PopoverHit>,
    pub bounds: Option<Rect>,
}

/// What the Info menu shows besides the tray state: the session it belongs to
/// and the segments living in it.
#[derive(Clone, Debug, Default)]
pub struct InfoView {
    pub session: String,
    pub windows: usize,
    pub version: &'static str,
    pub rows: Vec<InfoRow>,
}

/// One pending notification: a background app rang the bell.
#[derive(Clone, Debug, PartialEq)]
pub struct Notif {
    /// Raw window id (`WindowId.0`) — clicking the row focuses this window.
    pub win: u64,
    pub text: String,
    pub time: String,
}

/// Keep at most this many notifications (oldest dropped).
const MAX_NOTIFS: usize = 50;

/// The interactive menubar tray: tracks which segment's popover is open (and the
/// x it is anchored under), renders the popover, and maps clicks to intents.
#[derive(Default)]
pub struct Tray {
    open: Option<(SegmentKind, i32)>,
    /// Months the calendar popover is offset from the current month (◂ ▸ nav).
    cal_offset: i32,
    /// Pending bell notifications, newest first.
    notifs: Vec<Notif>,
    /// `[tray_tools]` overrides for the popovers' Open rows.
    tools: crate::traytools::TrayTools,
    /// Volume popover keyboard row: 0 the level bar, 1 the mixer link.
    vol_sel: usize,
    /// Info menu keyboard row: an index into its rows (segments, then links).
    info_sel: usize,
}

/// The mixer row shared by the volume popover and Settings → Sound: the first
/// installed program of `[tray_tools] volume` (wiremix gets `-s extracompat`
/// when `ascii`, the bare-console glyph tier), else on Linux a Store search
/// for wiremix. An empty `volume` list in the config removes the row.
pub fn volume_mixer(
    tools: &crate::traytools::TrayTools,
    installed: &dyn Fn(&str) -> bool,
    ascii: bool,
) -> Option<(String, ControlIntent)> {
    if let Some(mut a) = segment_app(SegmentKind::Volume, tools, installed) {
        if ascii && a.command == "wiremix" && a.args.is_empty() {
            a.args = vec!["-s".into(), "extracompat".into()];
        }
        return Some((a.label, ControlIntent::OpenTool { title: a.title, command: a.command, args: a.args }));
    }
    let removed = tools.get("volume").is_some_and(|s| s.lines().is_empty());
    (cfg!(target_os = "linux") && !removed)
        .then(|| ("Get wiremix (Store)".to_string(), ControlIntent::OpenStoreSearch("wiremix".into())))
}

/// The volume mixer row for the live config and glyph tier.
pub fn volume_mixer_now(tools: &crate::traytools::TrayTools) -> Option<(String, ControlIntent)> {
    volume_mixer(tools, &crate::catalog::is_installed, crate::iconset::style() == crate::iconset::IconStyle::Ascii)
}

/// A key routed to the open volume popover (the client sends these while the
/// `tray_volume` flag is set, however the popover was opened).
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum TrayKey {
    Left,
    Right,
    Up,
    Down,
    Enter,
    Mute,
    Esc,
}

impl Tray {
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the `[tray_tools]` config (call on startup and config change).
    pub fn set_tools(&mut self, tools: crate::traytools::TrayTools) {
        self.tools = tools;
    }

    /// The kind whose popover is currently open, if any.
    pub fn open(&self) -> Option<SegmentKind> {
        self.open.map(|(k, _)| k)
    }

    /// Open `kind`'s popover at a default anchor (used by tests).
    pub fn force_open(&mut self, kind: SegmentKind) {
        self.open_at(kind, 20);
    }

    /// Open `kind`'s popover anchored under column `anchor_x` (the keyboard
    /// path: leader `=` opens Volume under its segment).
    pub fn open_at(&mut self, kind: SegmentKind, anchor_x: i32) {
        self.open = Some((kind, anchor_x));
        self.cal_offset = 0;
        self.vol_sel = 0;
        self.info_sel = 0;
    }

    /// A key while the Info menu is open, with `rows` its selectable rows:
    /// Up / Down move (wrapping), Enter returns the row to act on, Esc closes.
    pub fn info_key(&mut self, key: TrayKey, rows: usize) -> Option<usize> {
        let n = rows.max(1);
        match key {
            TrayKey::Up | TrayKey::Left => self.info_sel = (self.info_sel + n - 1) % n,
            TrayKey::Down | TrayKey::Right => self.info_sel = (self.info_sel + 1) % n,
            TrayKey::Enter => return rows.checked_sub(1).map(|last| self.info_sel.min(last)),
            TrayKey::Esc => self.close(),
            TrayKey::Mute => {}
        }
        None
    }

    /// The highlighted Info menu row, for tests.
    pub fn info_row(&self) -> usize {
        self.info_sel
    }

    /// The volume popover's link row: the first installed mixer from
    /// `[tray_tools] volume`, or on Linux a Store search for wiremix when none
    /// is installed (an empty `volume` list in the config removes the row).
    /// On the ascii icon tier (bare console) wiremix gets its ASCII char set.
    pub fn volume_link(&self) -> Option<(String, ControlIntent)> {
        volume_mixer_now(&self.tools)
    }

    /// The highlighted volume popover row (0 level, 1 link), for tests.
    pub fn volume_row(&self) -> usize {
        self.vol_sel
    }

    /// A key while the volume popover is open. Left / Right step the level on
    /// any row, Up / Down move between the level and the link row, Enter opens
    /// the link (or closes on the level row), `m` / Space mute, Esc closes.
    /// Returns the intent for the session to apply.
    pub fn volume_key(&mut self, key: TrayKey) -> Option<ControlIntent> {
        let link = self.volume_link();
        match key {
            TrayKey::Left => Some(ControlIntent::VolumeDown),
            TrayKey::Right => Some(ControlIntent::VolumeUp),
            TrayKey::Mute => Some(ControlIntent::ToggleMute),
            TrayKey::Up => {
                self.vol_sel = 0;
                None
            }
            TrayKey::Down => {
                self.vol_sel = if link.is_some() { 1 } else { 0 };
                None
            }
            TrayKey::Enter => {
                let pick = if self.vol_sel == 1 { link.map(|(_, i)| i) } else { None };
                self.close();
                pick
            }
            TrayKey::Esc => {
                self.close();
                None
            }
        }
    }

    /// Open `kind`'s popover in place of the open one, keeping its anchor (the
    /// Info menu's rows open the segment's own popover; one level only, a
    /// segment popover never opens another).
    pub fn open_nested(&mut self, kind: SegmentKind) {
        let anchor = self.open.map(|(_, x)| x).unwrap_or(20);
        self.open = Some((kind, anchor));
        self.cal_offset = 0;
    }

    /// Close any open popover.
    pub fn close(&mut self) {
        self.open = None;
    }

    /// Step the calendar popover by `delta` months (◂ ▸ clicks).
    pub fn calendar_step(&mut self, delta: i32) {
        self.cal_offset += delta;
    }

    /// Record a bell notification (newest first, capped).
    pub fn notify(&mut self, win: u64, text: String, time: String) {
        self.notifs.insert(0, Notif { win, text, time });
        self.notifs.truncate(MAX_NOTIFS);
    }

    pub fn notif_count(&self) -> usize {
        self.notifs.len()
    }

    pub fn has_notif_for(&self, win: u64) -> bool {
        self.notifs.iter().any(|n| n.win == win)
    }

    pub fn clear_notifs(&mut self) {
        self.notifs.clear();
    }

    /// Drop the notifications for one window (clicked / focused).
    pub fn clear_notifs_for(&mut self, win: u64) {
        self.notifs.retain(|n| n.win != win);
    }

    /// Handle a click on the menubar row: toggle the popover of the clicked
    /// segment. Returns `true` if the click hit a segment (handled).
    pub fn on_menubar_click(&mut self, p: Point, segments: &[Segment]) -> bool {
        if p.y != 0 {
            return false;
        }
        if let Some(seg) = segments.iter().find(|s| s.rect.contains(p)) {
            self.open = match self.open {
                Some((k, _)) if k == seg.kind => None,
                _ => {
                    self.cal_offset = 0; // calendar always opens on the current month
                    self.vol_sel = 0;
                    Some((seg.kind, seg.rect.x))
                }
            };
            return true;
        }
        false
    }

    /// Map a click inside the open popover to a `ControlIntent`, if it landed on
    /// a hot-zone.
    pub fn on_popover_click(&self, p: Point, r: &Rendered) -> Option<ControlIntent> {
        r.hits.iter().find(|h| h.rect.contains(p)).map(|h| h.intent.clone())
    }

    /// Render the open popover (if any) into layers + hits. The Info menu shows
    /// nothing session-specific here; the session uses [`Tray::render_with`].
    pub fn render(&self, w: i32, h: i32, state: &St) -> Rendered {
        self.render_with(w, h, state, &InfoView::default())
    }

    /// Like [`Tray::render`], with the session facts the Info menu displays.
    pub fn render_with(&self, w: i32, h: i32, state: &St, info: &InfoView) -> Rendered {
        let Some((kind, anchor_x)) = self.open else { return Rendered::default() };
        match kind {
            SegmentKind::Info => self.render_info(w, h, anchor_x, state, info),
            SegmentKind::Volume => self.render_volume(w, h, anchor_x, state),
            SegmentKind::Brightness => self.render_brightness(w, anchor_x, state),
            SegmentKind::Wifi => self.render_wifi(w, h, anchor_x, state),
            SegmentKind::Bluetooth => self.render_bluetooth(w, h, anchor_x, state),
            SegmentKind::Tetron => self.render_tetron(w, h, anchor_x, state),
            // The clipboard popover is the session's `ClipMenu` overlay, not a tray popover.
            SegmentKind::Clipboard | SegmentKind::Select => Rendered::default(),
            SegmentKind::Clock => self.render_calendar(w, h, anchor_x, state),
            SegmentKind::Bell => self.render_bell(w, anchor_x),
            SegmentKind::Cpu => self.render_lines(w, h, anchor_x, "CPU", SegmentKind::Cpu, &[
                format!("{:.0}% load", state.cpu_pct),
            ]),
            SegmentKind::Mem => self.render_lines(w, h, anchor_x, "Memory", SegmentKind::Mem, &[
                format!("{}% used", mem_pct(state.mem.used, state.mem.total)),
            ]),
            SegmentKind::Battery => {
                let mut lines = vec![state.battery.map(|b| format!("{}%{}", b.pct, if b.charging { format!(" {}", crate::iconset::status(crate::iconset::Status::BatteryCharging)) } else { String::new() }))
                    .unwrap_or_else(|| "no battery".into())];
                if let Some(b) = state.battery {
                    if let Some(m) = b.mins {
                        lines.push(format!("{} {}", crate::system::fmt_mins(m), if b.charging { "to full" } else { "left" }));
                    }
                }
                self.render_lines(w, h, anchor_x, "Battery", SegmentKind::Battery, &lines)
            }
        }
    }

    /// The Info menu: session facts, the segments living in it (a click opens
    /// the segment's popover; CPU and memory run their tool directly), and links.
    fn render_info(&self, w: i32, _h: i32, anchor_x: i32, _st: &St, info: &InfoView) -> Rendered {
        let t = crate::theme::current();
        let box_w = 38;
        // Interior rows: title, 3 facts, [blank, segment rows], blank, links;
        // plus the top and bottom border.
        let links: [(&str, ControlIntent); 5] = [
            ("Sessions", ControlIntent::OpenSessions),
            ("Screensaver", ControlIntent::StartScreensaver),
            ("Logs", ControlIntent::OpenLogs),
            ("Shortcuts", ControlIntent::OpenHelp),
            ("Settings", ControlIntent::OpenSettings),
        ];
        let seg_rows = info.rows.len() as i32;
        let box_h = 2 + 1 + 3 + if seg_rows > 0 { 1 + seg_rows } else { 0 } + 1 + links.len() as i32;
        let origin = self.box_origin(w, anchor_x, box_w);
        let mut buf = CellBuffer::new(box_w, box_h);
        fill_box(&mut buf, box_w, box_h);
        let cap = (box_w - 4) as usize;
        let clip = |s: String| -> String { s.chars().take(cap).collect() };
        buf.write_str(2, 1, "Info", t.accent, t.window_bg);
        let mut y = 2;
        for (k, v) in [
            ("Session", info.session.clone()),
            ("Windows", info.windows.to_string()),
            ("Version", info.version.to_string()),
        ] {
            buf.write_str(2, y, &clip(format!("{k:<8}{v}")), t.text, t.window_bg);
            y += 1;
        }
        let mut hits = Vec::new();
        if seg_rows > 0 {
            y += 1; // blank line
            for row in &info.rows {
                // CPU / memory open their tool straight away; everything else
                // opens its own popover.
                let action = segment_app(row.kind, &self.tools, &crate::catalog::is_installed);
                let tail = if action.is_some() { " \u{25B8}" } else { "" };
                let line = clip(format!("{:<10}{}{}", row.label, row.text, tail));
                let bg = if hits.len() == self.info_sel { t.active_bg } else { t.window_bg };
                for x in 1..box_w - 1 {
                    buf.set(x, y, crate::cell::Cell { ch: ' ', fg: t.text, bg, attrs: Default::default() });
                }
                buf.write_str(2, y, &line, t.text, bg);
                let intent = match (row.kind, action) {
                    (SegmentKind::Cpu | SegmentKind::Mem, Some(a)) => {
                        ControlIntent::OpenTool { title: a.title, command: a.command, args: a.args }
                    }
                    _ => ControlIntent::OpenSegment(row.kind.key().unwrap_or("").to_string()),
                };
                hits.push(PopoverHit { rect: Rect::new(origin.x + 1, origin.y + y, box_w - 2, 1), intent });
                y += 1;
            }
        }
        y += 1; // blank line
        for (label, intent) in links {
            // The keyboard row highlight: the whole inner width of the selected row.
            let bg = if hits.len() == self.info_sel { t.active_bg } else { t.window_bg };
            for x in 1..box_w - 1 {
                buf.set(x, y, crate::cell::Cell { ch: ' ', fg: t.accent, bg, attrs: Default::default() });
            }
            buf.write_str(2, y, &format!("\u{25B8} {label}"), t.accent, bg);
            hits.push(PopoverHit { rect: Rect::new(origin.x + 1, origin.y + y, box_w - 2, 1), intent });
            y += 1;
        }
        Rendered { layers: vec![layer(origin, buf)], hits, bounds: Some(Rect::new(origin.x, origin.y, box_w, box_h)) }
    }

    fn box_origin(&self, w: i32, anchor_x: i32, box_w: i32) -> Point {
        let x = anchor_x.min(w - box_w - 1).max(0);
        Point::new(x, 1)
    }

    /// Brightness popover: title with the percent, a level bar with ◂ ▸.
    fn render_brightness(&self, w: i32, anchor_x: i32, st: &St) -> Rendered {
        let box_w = 26;
        let box_h = 5;
        let origin = self.box_origin(w, anchor_x, box_w);
        let mut buf = CellBuffer::new(box_w, box_h);
        fill_box(&mut buf, box_w, box_h);
        let t = crate::theme::current();
        let Some(p) = st.brightness else {
            buf.write_str(2, 1, "No backlight", t.text, t.window_bg);
            buf.write_str(2, 3, "Esc", t.dim, t.window_bg);
            return Rendered { layers: vec![layer(origin, buf)], hits: Vec::new(), bounds: Some(Rect::new(origin.x, origin.y, box_w, box_h)) };
        };
        buf.write_str(2, 1, &format!("Brightness {p}%"), t.text, t.window_bg);
        for x in 1..box_w - 1 {
            buf.set(x, 2, crate::cell::Cell { ch: ' ', fg: t.text, bg: t.active_bg, attrs: Default::default() });
        }
        let filled = (p as i32 * 10 / 100).clamp(0, 10) as usize;
        let bar: String = (0..10).map(|i| if i < filled { crate::glyphs::ch(crate::glyphs::Glyph::BarOn) } else { crate::glyphs::ch(crate::glyphs::Glyph::BarOff) }).collect();
        buf.write_str(2, 2, "◂", t.accent, t.active_bg);
        buf.write_str(5, 2, &bar, t.text, t.active_bg);
        buf.write_str(17, 2, "▸", t.accent, t.active_bg);
        let hit = |lx: i32, intent: ControlIntent| PopoverHit { rect: Rect::new(origin.x + lx, origin.y + 2, 1, 1), intent };
        let hits = vec![hit(2, ControlIntent::BrightnessDown), hit(17, ControlIntent::BrightnessUp)];
        buf.write_str(2, box_h - 2, "←→ level  Esc", t.dim, t.window_bg);
        Rendered { layers: vec![layer(origin, buf)], hits, bounds: Some(Rect::new(origin.x, origin.y, box_w, box_h)) }
    }

    /// A key while the brightness popover is open: Left / Right step the
    /// level, Enter and Esc close. Returns the intent for the session to apply.
    pub fn brightness_key(&mut self, key: TrayKey) -> Option<ControlIntent> {
        match key {
            TrayKey::Left => Some(ControlIntent::BrightnessDown),
            TrayKey::Right => Some(ControlIntent::BrightnessUp),
            TrayKey::Enter | TrayKey::Esc => {
                self.close();
                None
            }
            TrayKey::Up | TrayKey::Down | TrayKey::Mute => None,
        }
    }

    fn render_volume(&self, w: i32, _h: i32, anchor_x: i32, st: &St) -> Rendered {
        let link = self.volume_link();
        let box_w = 26;
        // Rows: title, level bar, [link], key hint, plus the two border rows.
        let box_h = 5 + link.is_some() as i32;
        let origin = self.box_origin(w, anchor_x, box_w);
        let mut buf = CellBuffer::new(box_w, box_h);
        fill_box(&mut buf, box_w, box_h);
        let t = crate::theme::current();
        let v = &st.volume;
        buf.write_str(2, 1, &format!("Volume {}{}", v.level, if v.muted { " (muted)" } else { "" }), t.text, t.window_bg);
        // The keyboard row highlight: the whole inner width of the selected row.
        let sel_y = 2 + self.vol_sel as i32;
        let bg_at = |y: i32| if y == sel_y { t.active_bg } else { t.window_bg };
        for x in 1..box_w - 1 {
            buf.set(x, sel_y, crate::cell::Cell { ch: ' ', fg: t.text, bg: t.active_bg, attrs: Default::default() });
        }
        // Row 2: ◂  bar  ▸   speaker
        let filled = (v.level as i32 * 6 / 100).clamp(0, 6) as usize;
        let bar: String = (0..6).map(|i| if i < filled { crate::glyphs::ch(crate::glyphs::Glyph::BarOn) } else { crate::glyphs::ch(crate::glyphs::Glyph::BarOff) }).collect();
        buf.write_str(2, 2, "◂", t.accent, bg_at(2));
        buf.write_str(5, 2, &bar, t.text, bg_at(2));
        buf.write_str(13, 2, "▸", t.accent, bg_at(2));
        buf.write_str(17, 2, &crate::iconset::status(volume_status(v)).to_string(), t.text, bg_at(2));
        let hit = |lx: i32, lw: i32, intent: ControlIntent| PopoverHit {
            rect: Rect::new(origin.x + lx, origin.y + 2, lw, 1),
            intent,
        };
        let mut hits = vec![
            hit(2, 1, ControlIntent::VolumeDown),
            hit(13, 1, ControlIntent::VolumeUp),
            hit(17, 2, ControlIntent::ToggleMute),
        ];
        if let Some((label, intent)) = link {
            let row: String = format!("\u{25B8} {label}").chars().take((box_w - 3) as usize).collect();
            buf.write_str(2, 3, &row, t.accent, bg_at(3));
            hits.push(PopoverHit { rect: Rect::new(origin.x + 1, origin.y + 3, box_w - 2, 1), intent });
        }
        buf.write_str(2, box_h - 2, "←→ level  m mute  Esc", t.dim, t.window_bg);
        Rendered { layers: vec![layer(origin, buf)], hits, bounds: Some(Rect::new(origin.x, origin.y, box_w, box_h)) }
    }

    /// Network popover: WiFi toggle + SSID (when WiFi is present), the primary
    /// link's IP and interface (click copies the IP), known networks, and the
    /// Open row. With no link at all it reads "Offline".
    fn render_wifi(&self, w: i32, _h: i32, anchor_x: i32, st: &St) -> Rendered {
        let t = crate::theme::current();
        let wifi = st.wifi.as_ref();
        let enabled = wifi.map(|x| x.enabled).unwrap_or(false);
        let cur = wifi.map(|x| x.ssid.clone()).unwrap_or_default();
        let nets: Vec<String> = if wifi.is_some() { st.known_networks.iter().take(6).cloned().collect() } else { Vec::new() };
        let action = segment_app(SegmentKind::Wifi, &self.tools, &crate::catalog::is_installed);
        let show_ssid = wifi.is_some() && !cur.is_empty() && st.net.as_ref().map(|n| n.wifi).unwrap_or(true);
        let box_w = 34;
        let box_h = 3 + show_ssid as i32 + 1 + nets.len() as i32 + action.is_some() as i32;
        let origin = self.box_origin(w, anchor_x, box_w);
        let mut buf = CellBuffer::new(box_w, box_h);
        fill_box(&mut buf, box_w, box_h);
        let mut hits = Vec::new();
        if wifi.is_some() {
            buf.write_str(2, 1, &format!("Wi-Fi  [{}]", if enabled { "on" } else { "off" }), t.accent, t.window_bg);
            hits.push(PopoverHit {
                rect: Rect::new(origin.x + 9, origin.y + 1, 4, 1),
                intent: ControlIntent::WifiSetEnabled(!enabled),
            });
        } else {
            buf.write_str(2, 1, "Network", t.accent, t.window_bg);
        }
        let cap = (box_w - 4) as usize;
        let mut y = 2;
        if show_ssid {
            let line: String = format!("SSID  {cur}").chars().take(cap).collect();
            buf.write_str(2, y, &line, t.text, t.window_bg);
            y += 1;
        }
        match &st.net {
            Some(n) if !n.ip.is_empty() => {
                let line: String = format!("IP    {} ({})", n.ip, n.iface).chars().take(cap).collect();
                buf.write_str(2, y, &line, t.text, t.window_bg);
                hits.push(PopoverHit {
                    rect: Rect::new(origin.x + 1, origin.y + y, box_w - 2, 1),
                    intent: ControlIntent::CopyText(n.ip.clone()),
                });
            }
            Some(n) => buf.write_str(2, y, &format!("{} (no IP)", n.iface), t.dim, t.window_bg),
            None => buf.write_str(2, y, "Offline", t.dim, t.window_bg),
        }
        y += 1;
        for ssid in &nets {
            let mark = if *ssid == cur { "●" } else { " " };
            let line: String = format!("{} {}", mark, ssid).chars().take(cap).collect();
            buf.write_str(2, y, &line, t.text, t.window_bg);
            hits.push(PopoverHit {
                rect: Rect::new(origin.x + 1, origin.y + y, box_w - 2, 1),
                intent: ControlIntent::WifiConnectKnown(ssid.clone()),
            });
            y += 1;
        }
        if let Some(a) = action {
            buf.write_str(2, y, &format!("\u{25B8} {}", a.label), t.accent, t.window_bg);
            hits.push(PopoverHit {
                rect: Rect::new(origin.x + 1, origin.y + y, box_w - 2, 1),
                intent: ControlIntent::OpenTool { title: a.title, command: a.command, args: a.args },
            });
        }
        Rendered { layers: vec![layer(origin, buf)], hits, bounds: Some(Rect::new(origin.x, origin.y, box_w, box_h)) }
    }

    fn render_bluetooth(&self, w: i32, _h: i32, anchor_x: i32, st: &St) -> Rendered {
        let t = crate::theme::current();
        let bt = &st.bluetooth;
        let devs: Vec<_> = bt.devices.iter().take(6).cloned().collect();
        let box_w = 30;
        let action = segment_app(SegmentKind::Bluetooth, &self.tools, &crate::catalog::is_installed);
        let box_h = 3 + devs.len().max(1) as i32 + action.is_some() as i32;
        let origin = self.box_origin(w, anchor_x, box_w);
        let mut buf = CellBuffer::new(box_w, box_h);
        fill_box(&mut buf, box_w, box_h);
        buf.write_str(2, 1, &format!("Bluetooth  [{}]", if bt.enabled { "on" } else { "off" }), t.accent, t.window_bg);
        let mut hits = vec![PopoverHit {
            rect: Rect::new(origin.x + 13, origin.y + 1, 4, 1),
            intent: ControlIntent::BtSetEnabled(!bt.enabled),
        }];
        if devs.is_empty() && st.caps.bluetooth {
            buf.write_str(2, 2, "(no paired devices)", t.dim, t.window_bg);
        } else if !st.caps.bluetooth {
            buf.write_str(2, 2, "install blueutil to control", t.dim, t.window_bg);
        }
        for (i, d) in devs.iter().enumerate() {
            let y = 2 + i as i32;
            let mark = if d.connected { "●" } else { crate::glyphs::get(crate::glyphs::Glyph::Off) };
            buf.write_str(2, y, &format!("{} {}", mark, d.name), t.text, t.window_bg);
            hits.push(PopoverHit {
                rect: Rect::new(origin.x + 1, origin.y + y, box_w - 2, 1),
                intent: ControlIntent::BtConnect { addr: d.addr.clone(), connect: !d.connected },
            });
        }
        if let Some(a) = action {
            let y = 2 + devs.len().max(1) as i32;
            buf.write_str(2, y, &format!("\u{25B8} {}", a.label), t.accent, t.window_bg);
            hits.push(PopoverHit {
                rect: Rect::new(origin.x + 1, origin.y + y, box_w - 2, 1),
                intent: ControlIntent::OpenTool { title: a.title, command: a.command, args: a.args },
            });
        }
        Rendered { layers: vec![layer(origin, buf)], hits, bounds: Some(Rect::new(origin.x, origin.y, box_w, box_h)) }
    }

    /// tetron popover: a global on/off toggle plus one row per network that
    /// toggles that network's data plane (Resume/Standby). Clicking the `[on]`/
    /// `[off]` title label toggles all networks; clicking a network row toggles
    /// just it. (Invites/join live in the tetron-tui, not here.)
    fn render_tetron(&self, w: i32, _h: i32, anchor_x: i32, st: &St) -> Rendered {
        let t = crate::theme::current();
        let ti = &st.tetron;
        let nets: Vec<_> = ti.networks.iter().take(8).cloned().collect();
        let rows = nets.len().max(1) as i32; // network rows (or the "(no networks)" line)
        let box_w = 40;
        let box_h = 3 + rows + 1; // +1 for the "Open terminal" action row
        let origin = self.box_origin(w, anchor_x, box_w);
        let mut buf = CellBuffer::new(box_w, box_h);
        fill_box(&mut buf, box_w, box_h);
        buf.write_str(2, 1, &format!("Tetron  [{}]", if ti.active { "on" } else { "off" }), t.accent, t.window_bg);
        // Global toggle on the `[on]`/`[off]` label.
        let mut hits = vec![PopoverHit {
            rect: Rect::new(origin.x + 9, origin.y + 1, 5, 1),
            intent: ControlIntent::TetronSetActive { network: None, active: !ti.active },
        }];
        if nets.is_empty() {
            buf.write_str(2, 2, "(no networks)", t.dim, t.window_bg);
        }
        for (i, n) in nets.iter().enumerate() {
            let y = 2 + i as i32;
            // `●`/`○` = this network's data plane up/standby; direct/relay = how
            // its connected peers are reached.
            let mark = if n.active { "●" } else { crate::glyphs::get(crate::glyphs::Glyph::Off) };
            let via = if n.any_direct { "direct" } else { "relay" };
            let line = format!("{} {}  {}/{} · {}", mark, n.name, n.connected, n.members, via);
            buf.write_str(2, y, &line, t.text, t.window_bg);
            hits.push(PopoverHit {
                rect: Rect::new(origin.x + 1, origin.y + y, box_w - 2, 1),
                intent: ControlIntent::TetronSetActive { network: Some(n.name.clone()), active: !n.active },
            });
        }
        // Action row: open a terminal on `tetron status` + an interactive shell
        // (the full tetron CLI). The thin dashboard until the tetron-tui lands.
        let term_y = 2 + rows;
        buf.write_str(2, term_y, &format!("{} Open terminal", crate::glyphs::get(crate::glyphs::Glyph::Terminal)), t.accent, t.window_bg);
        hits.push(PopoverHit {
            rect: Rect::new(origin.x + 1, origin.y + term_y, box_w - 2, 1),
            intent: ControlIntent::TetronOpenTerminal,
        });
        Rendered { layers: vec![layer(origin, buf)], hits, bounds: Some(Rect::new(origin.x, origin.y, box_w, box_h)) }
    }

    /// The bell popover: pending notifications (click → focus that window) plus
    /// a clear-all row.
    fn render_bell(&self, w: i32, anchor_x: i32) -> Rendered {
        let t = crate::theme::current();
        let box_w = 36;
        let n = self.notifs.len().min(8);
        let box_h = n.max(1) as i32 + 3; // title + rows + clear + borders
        let origin = self.box_origin(w, anchor_x, box_w);
        let mut buf = CellBuffer::new(box_w, box_h);
        fill_box(&mut buf, box_w, box_h);
        buf.write_str(2, 1, "Notifications", t.accent, t.window_bg);
        let mut hits = Vec::new();
        if self.notifs.is_empty() {
            buf.write_str(2, 2, "(none)", t.dim, t.window_bg);
        }
        for (i, nf) in self.notifs.iter().take(8).enumerate() {
            let y = 2 + i as i32;
            let line: String = format!("{} {}", nf.time, nf.text)
                .chars()
                .take((box_w - 4) as usize)
                .collect();
            buf.write_str(2, y, &line, t.text, t.window_bg);
            hits.push(PopoverHit {
                rect: Rect::new(origin.x + 1, origin.y + y, box_w - 2, 1),
                intent: ControlIntent::NotifFocus(nf.win),
            });
        }
        let clear_y = box_h - 2;
        buf.write_str(2, clear_y, "clear all", t.accent, t.window_bg);
        hits.push(PopoverHit {
            rect: Rect::new(origin.x + 1, origin.y + clear_y, 11, 1),
            intent: ControlIntent::NotifClear,
        });
        Rendered { layers: vec![layer(origin, buf)], hits, bounds: Some(Rect::new(origin.x, origin.y, box_w, box_h)) }
    }

    /// The clock popover: a month calendar with ◂ ▸ navigation, today highlighted,
    /// and a time/uptime footer. Falls back to plain text until the first poll
    /// has produced a civil date.
    fn render_calendar(&self, w: i32, h: i32, anchor_x: i32, st: &St) -> Rendered {
        let c = &st.clock;
        if c.year == 0 {
            return self.render_lines(w, h, anchor_x, "Clock", SegmentKind::Clock, &[
                c.date.clone(),
                format!("up {}h", c.uptime_secs / 3600),
            ]);
        }
        let t = crate::theme::current();
        let (year, month) = crate::calendar::add_months(c.year, c.month, self.cal_offset);
        let weeks = crate::calendar::month_grid(year, month);
        // Upcoming events (khal): mark their days in the grid and list a few.
        let listed: Vec<&crate::system::CalEvent> = st.events.iter().take(3).collect();
        let box_w = 24;
        let box_h = weeks.len() as i32 + 5 + listed.len() as i32;
        let origin = self.box_origin(w, anchor_x, box_w);
        let mut buf = CellBuffer::new(box_w, box_h);
        fill_box(&mut buf, box_w, box_h);
        // Month header with prev/next arrows.
        let title = format!("{} {}", crate::calendar::month_name(month), year);
        let tx = (box_w - title.chars().count() as i32) / 2;
        buf.write_str(2, 1, "◂", t.accent, t.window_bg);
        buf.write_str(tx, 1, &title, t.accent, t.window_bg);
        buf.write_str(box_w - 3, 1, "▸", t.accent, t.window_bg);
        buf.write_str(2, 2, "Mo Tu We Th Fr Sa Su", t.dim, t.window_bg);
        for (wi, week) in weeks.iter().enumerate() {
            let y = 3 + wi as i32;
            for (di, day) in week.iter().enumerate() {
                if let Some(d) = day {
                    let x = 2 + di as i32 * 3;
                    let today = self.cal_offset == 0 && *d == c.day;
                    let has_event = st
                        .events
                        .iter()
                        .any(|e| e.year == year && e.month == month && e.day == *d);
                    let (fg, bg) = if today {
                        (t.window_bg, t.accent)
                    } else if has_event {
                        (t.accent, t.window_bg)
                    } else {
                        (t.text, t.window_bg)
                    };
                    buf.write_str(x, y, &format!("{d:>2}"), fg, bg);
                    if has_event && !today {
                        // Underline event days so they read as "something here".
                        for ex in x..x + 2 {
                            if let Some(cell) = buf.get(ex, y) {
                                let mut cell = *cell;
                                cell.attrs.underline = true;
                                buf.set(ex, y, cell);
                            }
                        }
                    }
                }
            }
        }
        // Next events (soonest first) under the grid.
        for (i, ev) in listed.iter().enumerate() {
            let y = 3 + weeks.len() as i32 + i as i32;
            let line: String = format!("{:>2}/{} {}", ev.day, ev.month, ev.text)
                .chars()
                .take((box_w - 4) as usize)
                .collect();
            buf.write_str(2, y, &line, t.text, t.window_bg);
        }
        let footer = format!("{}  up {}h", c.time, c.uptime_secs / 3600);
        buf.write_str(2, box_h - 2, &footer, t.dim, t.window_bg);
        let hits = vec![
            PopoverHit {
                rect: Rect::new(origin.x + 1, origin.y + 1, 3, 1),
                intent: ControlIntent::CalendarPrev,
            },
            PopoverHit {
                rect: Rect::new(origin.x + box_w - 4, origin.y + 1, 3, 1),
                intent: ControlIntent::CalendarNext,
            },
        ];
        Rendered { layers: vec![layer(origin, buf)], hits, bounds: Some(Rect::new(origin.x, origin.y, box_w, box_h)) }
    }

    fn render_lines(&self, w: i32, _h: i32, anchor_x: i32, title: &str, kind: SegmentKind, lines: &[String]) -> Rendered {
        let t = crate::theme::current();
        let box_w = 24;
        let action = segment_app(kind, &self.tools, &crate::catalog::is_installed);
        let box_h = 2 + lines.len() as i32 + action.is_some() as i32;
        let origin = self.box_origin(w, anchor_x, box_w);
        let mut buf = CellBuffer::new(box_w, box_h);
        fill_box(&mut buf, box_w, box_h);
        buf.write_str(2, 1, title, t.accent, t.window_bg);
        for (i, l) in lines.iter().enumerate() {
            buf.write_str(2, 2 + i as i32, l, t.text, t.window_bg);
        }
        let mut hits = Vec::new();
        if let Some(a) = action {
            // Action row below the readout: click opens the tool in a window.
            let y = 2 + lines.len() as i32;
            buf.write_str(2, y, &format!("\u{25B8} {}", a.label), t.accent, t.window_bg);
            hits.push(PopoverHit {
                rect: Rect::new(origin.x + 1, origin.y + y, box_w - 2, 1),
                intent: ControlIntent::OpenTool { title: a.title, command: a.command, args: a.args },
            });
        }
        Rendered { layers: vec![layer(origin, buf)], hits, bounds: Some(Rect::new(origin.x, origin.y, box_w, box_h)) }
    }
}

fn layer(origin: Point, buf: CellBuffer) -> Layer {
    Layer { z: 5000, origin, buf, opacity: 1.0, scissor: None }
}

/// Fill a buffer with the window background and draw a rounded border.
fn fill_box(buf: &mut CellBuffer, w: i32, h: i32) {
    let t = crate::theme::current();
    buf.fill(Cell { ch: ' ', fg: t.text, bg: t.window_bg, attrs: Default::default() });
    let b = |ch: char| Cell { ch, fg: t.border, bg: t.window_bg, attrs: Default::default() };
    for x in 0..w {
        buf.set(x, 0, b('─'));
        buf.set(x, h - 1, b('─'));
    }
    for y in 0..h {
        buf.set(0, y, b('│'));
        buf.set(w - 1, y, b('│'));
    }
    buf.set(0, 0, b('╭'));
    buf.set(w - 1, 0, b('╮'));
    buf.set(0, h - 1, b('╰'));
    buf.set(w - 1, h - 1, b('╯'));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn info_keys_wrap_pick_and_close() {
        let mut t = Tray::new();
        t.force_open(SegmentKind::Info);
        assert_eq!(t.info_key(TrayKey::Up, 5), None);
        assert_eq!(t.info_row(), 4, "up wraps to the last row");
        t.info_key(TrayKey::Down, 5);
        assert_eq!(t.info_key(TrayKey::Enter, 5), Some(0));
        assert_eq!(t.info_key(TrayKey::Enter, 0), None, "nothing to pick");
        t.info_key(TrayKey::Esc, 5);
        assert_eq!(t.open(), None);
    }

    #[test]
    fn volume_link_prefers_installed_mixer_and_ascii_charset() {
        let tools = crate::traytools::TrayTools::new();
        let only = |name: &'static str| move |p: &str| p == name;
        let (label, intent) = volume_mixer(&tools, &only("pulsemixer"), false).unwrap();
        assert_eq!(label, "Open pulsemixer");
        assert!(matches!(intent, ControlIntent::OpenTool { ref command, .. } if command == "pulsemixer"));
        // wiremix wins when present; on the ascii tier it gets the ASCII char set.
        let (_, intent) = volume_mixer(&tools, &|_| true, true).unwrap();
        assert_eq!(intent, ControlIntent::OpenTool { title: "Volume".into(), command: "wiremix".into(), args: vec!["-s".into(), "extracompat".into()] });
    }

    #[test]
    fn volume_link_falls_back_to_store_and_config_can_remove_it() {
        let none = volume_mixer(&crate::traytools::TrayTools::new(), &|_| false, false);
        if cfg!(target_os = "linux") {
            assert_eq!(none.unwrap().1, ControlIntent::OpenStoreSearch("wiremix".into()));
        }
        let mut tools = crate::traytools::TrayTools::new();
        tools.insert("volume".into(), crate::traytools::ToolSpec::Many(Vec::new()));
        assert_eq!(volume_mixer(&tools, &|_| true, false), None);
    }

    #[test]
    fn brightness_segment_only_with_a_backlight_and_its_keys() {
        let mut st = SystemState::default();
        let has = |st: &SystemState| tray_segments(st, 200, 0, 0).iter().any(|s| s.kind == SegmentKind::Brightness);
        assert!(!has(&st), "no backlight: no segment");
        st.brightness = Some(7);
        let segs = tray_segments(&st, 200, 0, 0);
        let b = segs.iter().position(|s| s.kind == SegmentKind::Brightness).expect("segment");
        assert!(segs[b].text.ends_with("  7"), "fixed width: {:?}", segs[b].text);
        assert_eq!(segs[b + 1].kind, SegmentKind::Info, "left of Info");
        let mut tray = Tray::new();
        tray.open_at(SegmentKind::Brightness, 50);
        assert_eq!(tray.brightness_key(TrayKey::Left), Some(ControlIntent::BrightnessDown));
        assert_eq!(tray.brightness_key(TrayKey::Right), Some(ControlIntent::BrightnessUp));
        assert!(!tray.render(120, 30, &st).hits.is_empty());
        assert_eq!(tray.brightness_key(TrayKey::Esc), None);
        assert_eq!(tray.open(), None);
    }

    #[test]
    fn volume_keys_step_level_move_rows_and_close() {
        let mut tray = Tray::new();
        tray.open_at(SegmentKind::Volume, 50);
        assert_eq!(tray.volume_key(TrayKey::Left), Some(ControlIntent::VolumeDown));
        assert_eq!(tray.volume_key(TrayKey::Right), Some(ControlIntent::VolumeUp));
        assert_eq!(tray.volume_key(TrayKey::Mute), Some(ControlIntent::ToggleMute));
        // Enter on the level row just closes.
        assert_eq!(tray.volume_key(TrayKey::Enter), None);
        assert_eq!(tray.open(), None);
        tray.open_at(SegmentKind::Volume, 50);
        tray.volume_key(TrayKey::Down);
        tray.volume_key(TrayKey::Up);
        assert_eq!(tray.volume_row(), 0);
        tray.volume_key(TrayKey::Esc);
        assert_eq!(tray.open(), None);
    }
}
