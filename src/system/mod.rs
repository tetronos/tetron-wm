//! Host system state (clock, CPU/mem, battery, WiFi, Bluetooth, volume) and the
//! traits that read and control it. Portable metrics come from the `sysinfo`
//! crate; WiFi/volume/Bluetooth go through a per-OS backend.

/// A full snapshot of host state, refreshed by the poller and read each frame.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SystemState {
    pub clock: ClockInfo,
    pub cpu_pct: f32,
    pub mem: MemInfo,
    pub battery: Option<BatteryInfo>,
    pub wifi: Option<WifiInfo>,
    /// The primary (default-route) network link, WiFi or not.
    pub net: Option<NetInfo>,
    pub bluetooth: BluetoothInfo,
    pub volume: VolumeInfo,
    /// Panel backlight in percent; `None` without a backlight device
    /// (desktops, VMs), which hides the brightness controls.
    pub brightness: Option<u8>,
    pub known_networks: Vec<String>,
    pub caps: BackendCaps,
    /// Reachability of the saved remote systems (name → ssh port answered),
    /// refreshed by the poller so the Systems menu can show ●/○ dots.
    pub remotes_online: Vec<(String, bool)>,
    /// Upcoming calendar events (from `khal` when installed), soonest first.
    pub events: Vec<CalEvent>,
    /// tetron (P2P mesh VPN) status, polled from its daemon socket. Default =
    /// daemon not reachable (tetron may not be installed/running); the tray
    /// segment is hidden unless `tetron.reachable`.
    pub tetron: TetronInfo,
}

/// tetron daemon status for the menubar tray.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TetronInfo {
    /// The daemon's IPC socket answered a `Status` request this poll. When
    /// false, tetron is not installed/running and the tray segment is hidden.
    pub reachable: bool,
    /// The VPN is active (TUN up, networks connected), vs. reachable standby.
    pub active: bool,
    /// One entry per joined network.
    pub networks: Vec<TetronNetwork>,
}

/// One tetron network's summary for the tray.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TetronNetwork {
    /// Local display name of the network.
    pub name: String,
    /// Total members in the network.
    pub members: usize,
    /// Peers with a live connection right now.
    pub connected: usize,
    /// Any connected peer is a direct (not relayed) link.
    pub any_direct: bool,
    /// This network's data plane is up (vs. on standby). Drives the per-network
    /// connect/disconnect toggle.
    pub active: bool,
}

/// One upcoming calendar event for the menubar calendar popover.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CalEvent {
    /// Civil date of the event (local), for marking calendar days.
    pub year: i32,
    pub month: u32,
    pub day: u32,
    /// Display line, e.g. "09:30 Standup".
    pub text: String,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct ClockInfo {
    pub time: String,
    pub date: String,
    pub uptime_secs: u64,
    /// Today's civil date (local time) for the menubar calendar; all zero until
    /// the first poll succeeds.
    pub year: i32,
    pub month: u32,
    pub day: u32,
}
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct MemInfo {
    pub used: u64,
    pub total: u64,
}
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct BatteryInfo {
    pub pct: u8,
    pub charging: bool,
    /// Minutes to empty (discharging) or to full (charging), when known.
    pub mins: Option<u32>,
}
/// The primary network link: the interface carrying the default route.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct NetInfo {
    pub iface: String,
    pub ip: String,
    /// True for a WiFi interface; false for ethernet, tether, VPN, etc.
    pub wifi: bool,
}

/// Parse `ip -4 route get <addr>` output (`... dev wlan0 src 192.168.1.42 ...`)
/// into `(interface, ip)`.
pub fn parse_ip_route_get(out: &str) -> Option<(String, String)> {
    let w: Vec<&str> = out.split_whitespace().collect();
    let after = |key: &str| w.iter().position(|t| *t == key).and_then(|i| w.get(i + 1)).map(|s| s.to_string());
    Some((after("dev")?, after("src")?))
}

/// Parse `route -n get default` (macOS) into the interface name.
pub fn parse_route_n_get(out: &str) -> Option<String> {
    out.lines()
        .find_map(|l| l.trim().strip_prefix("interface:"))
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

/// Format a minutes count as `h:mm`.
pub fn fmt_mins(m: u32) -> String {
    format!("{}:{:02}", m / 60, m % 60)
}
#[derive(Clone, Debug, Default, PartialEq)]
pub struct WifiInfo {
    pub ssid: String,
    pub signal: u8,
    pub enabled: bool,
}
#[derive(Clone, Debug, Default, PartialEq)]
pub struct BtDevice {
    pub name: String,
    pub addr: String,
    pub connected: bool,
}
#[derive(Clone, Debug, Default, PartialEq)]
pub struct BluetoothInfo {
    pub enabled: bool,
    pub devices: Vec<BtDevice>,
}
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct VolumeInfo {
    pub level: u8,
    pub muted: bool,
}
/// Which controls the active backend can actually perform (a missing helper tool
/// flips a bit off, so its popover renders read-only).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct BackendCaps {
    pub volume: bool,
    pub wifi: bool,
    pub bluetooth: bool,
}

/// Four-segment signal bar, filled left-to-right for `signal` in 0..=4 (clamped).
pub fn bars_glyph(signal: u8) -> String {
    let n = signal.min(4) as usize;
    (0..4).map(|i| if i < n { '▮' } else { '·' }).collect()
}

/// Integer percentage of `used`/`total`, guarding division by zero.
pub fn mem_pct(used: u64, total: u64) -> u8 {
    if total == 0 {
        0
    } else {
        ((used as u128 * 100) / total as u128) as u8
    }
}

/// A control action requested by a popover click, dispatched by the session to
/// the active backend.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ControlIntent {
    VolumeUp,
    VolumeDown,
    VolumeSet(u8),
    ToggleMute,
    /// Screen brightness one step (`brightness::STEP`) up or down, or to a
    /// percent. Linux backlight only.
    BrightnessUp,
    BrightnessDown,
    BrightnessSet(u8),
    WifiSetEnabled(bool),
    WifiConnectKnown(String),
    BtSetEnabled(bool),
    BtConnect { addr: String, connect: bool },
    /// Activate (`Resume`) or deactivate (`Standby`) tetron. `network = None`
    /// toggles every joined network (daemon-wide); `Some(name)` toggles just
    /// that network. Handled by the session (sends to the tetron daemon),
    /// not the OS backend.
    TetronSetActive { network: Option<String>, active: bool },
    /// Open a terminal window showing `tetron status`, then an interactive
    /// shell where the full tetron CLI (invite/join/admin/…) is available.
    /// Handled by the session (launches a shell), not the OS backend.
    TetronOpenTerminal,
    /// Open `command args` in a new window (a tray popover's "Open <tool>"
    /// row: htop/top, free -h, jolt). Handled by the session, not the OS backend.
    OpenTool { title: String, command: String, args: Vec<String> },
    /// Step the menubar calendar popover a month back/forward. Handled by the
    /// session's tray state, not the OS backend.
    CalendarPrev,
    CalendarNext,
    /// Focus the window a notification came from / clear all notifications.
    /// Handled by the session, not the OS backend.
    NotifFocus(u64),
    NotifClear,
    /// Copy text to the clipboard (the network popover's IP row). Handled by
    /// the session, not the OS backend.
    CopyText(String),
    /// Info menu: open another tray segment's own popover in place (one level;
    /// the key is `SegmentKind::key()`). Handled by the session's tray state.
    OpenSegment(String),
    /// Info menu links. Handled by the session, not the OS backend.
    OpenSessions,
    OpenLogs,
    OpenHelp,
    OpenSettings,
    /// Info menu: start the screensaver.
    StartScreensaver,
    /// Make this audio output (PipeWire sink id) the system default, for the
    /// Settings Sound section's output picker (HDMI, headphones, speakers).
    SetOutput(u32),
    /// Open the Store with this search typed in (the volume popover's "Get
    /// wiremix" row when no mixer is installed). Handled by the session.
    OpenStoreSearch(String),
}

/// One audio output (a PipeWire sink) for the Settings output picker.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AudioOutput {
    pub id: u32,
    /// Display name with the device prefix all outputs share removed
    /// ("HDMI / DisplayPort 3 Output", not "Raptor Lake-P/U/H cAVS HDMI ...").
    pub name: String,
    /// This is the current default output.
    pub default: bool,
}

/// The machine's audio outputs. Shells out (timeout-guarded), so call it on a
/// user action (opening the Sound section, switching output), never per frame.
/// Empty where unsupported (macOS, no PipeWire).
pub fn audio_outputs() -> Vec<AudioOutput> {
    #[cfg(target_os = "linux")]
    {
        linux::read_outputs()
    }
    #[cfg(not(target_os = "linux"))]
    {
        Vec::new()
    }
}

/// The OS-specific slice of [`SystemState`] produced by a [`SystemMonitor`].
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SystemReadout {
    pub wifi: Option<WifiInfo>,
    pub net: Option<NetInfo>,
    pub bluetooth: BluetoothInfo,
    pub volume: VolumeInfo,
    pub known_networks: Vec<String>,
}

/// Which OS-specific parts a [`SystemMonitor::read_parts`] call fetches. Each
/// part costs a subprocess or two, so unseen data is not read.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Want {
    pub volume: bool,
    /// WiFi, the primary link (`net`) and the known-network list.
    pub net: bool,
    pub bluetooth: bool,
}

impl Want {
    pub const ALL: Want = Want { volume: true, net: true, bluetooth: true };
}

/// What the UI currently shows, published by the session for the poller: parts
/// nobody can see (a segment in the Info menu with the menu closed) are not
/// polled. Everything is wanted until a session says otherwise.
#[derive(Debug)]
pub struct Interest {
    net: std::sync::atomic::AtomicBool,
    bluetooth: std::sync::atomic::AtomicBool,
    /// Bumped when a part becomes wanted, so the poller reads it at once.
    kick: std::sync::atomic::AtomicU64,
}

impl Default for Interest {
    fn default() -> Self {
        Interest {
            net: std::sync::atomic::AtomicBool::new(true),
            bluetooth: std::sync::atomic::AtomicBool::new(true),
            kick: std::sync::atomic::AtomicU64::new(0),
        }
    }
}

impl Interest {
    /// Publish what is visible now. Volume is always on the bar, so always wanted.
    pub fn set(&self, net: bool, bluetooth: bool) {
        use std::sync::atomic::Ordering::Relaxed;
        let was_net = self.net.swap(net, Relaxed);
        let was_bt = self.bluetooth.swap(bluetooth, Relaxed);
        if (net && !was_net) || (bluetooth && !was_bt) {
            self.kick.fetch_add(1, Relaxed);
        }
    }

    pub fn want(&self) -> Want {
        use std::sync::atomic::Ordering::Relaxed;
        Want { volume: true, net: self.net.load(Relaxed), bluetooth: self.bluetooth.load(Relaxed) }
    }

    /// Changes whenever a part newly becomes wanted.
    pub fn kick_gen(&self) -> u64 {
        self.kick.load(std::sync::atomic::Ordering::Relaxed)
    }
}

/// Reads the OS-specific parts of the snapshot. Portable metrics (CPU/mem/
/// battery) are filled in by the poller via `sysinfo`.
pub trait SystemMonitor: Send + Sync {
    /// Read only the parts in `want`; the rest come back at their defaults.
    fn read_parts(&self, want: Want) -> SystemReadout;
    /// Read everything.
    fn read(&self) -> SystemReadout {
        self.read_parts(Want::ALL)
    }
    fn caps(&self) -> BackendCaps;
}

/// Applies a [`ControlIntent`] to the host (best-effort, timeout-guarded).
pub trait SystemControl: Send + Sync {
    fn apply(&self, intent: &ControlIntent);
}

/// A backend is both a monitor and a controller.
pub trait Backend: SystemMonitor + SystemControl {}
impl<T: SystemMonitor + SystemControl> Backend for T {}

/// Fallback backend on unsupported targets: no caps, no-op control.
pub struct StubBackend;
impl SystemMonitor for StubBackend {
    fn read_parts(&self, _want: Want) -> SystemReadout {
        SystemReadout::default()
    }
    fn caps(&self) -> BackendCaps {
        BackendCaps::default()
    }
}
impl SystemControl for StubBackend {
    fn apply(&self, _intent: &ControlIntent) {}
}

/// Pick the backend for the current OS. Returns a stub on unsupported targets.
pub fn backend() -> Box<dyn Backend> {
    #[cfg(target_os = "macos")]
    {
        Box::new(macos::MacBackend::new())
    }
    #[cfg(target_os = "linux")]
    {
        Box::new(linux::LinuxBackend::new())
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        Box::new(StubBackend)
    }
}

/// Kill a child and reap it on a background thread. Dropping a `Child` (even
/// after `kill`) never calls `wait`, so the dead process stays a zombie until
/// this process exits; a command that hangs every poll (nmcli with a wedged
/// NetworkManager) leaked one per cap. The thread also covers a child stuck in
/// uninterruptible sleep, which would block a plain `wait` here.
pub fn kill_and_reap(mut child: std::process::Child) {
    let _ = child.kill();
    std::thread::spawn(move || {
        let _ = child.wait();
    });
}

/// Run a command with a hard timeout, returning trimmed stdout on success.
/// Never blocks longer than `secs`; kills the child on timeout. This is the
/// **only** sanctioned way to shell out — it keeps a hung tool from freezing
/// any caller (the poller or a control dispatch).
pub fn run_capped(program: &str, args: &[&str], secs: u64) -> Option<String> {
    use std::io::Read;
    use std::process::{Command, Stdio};
    use std::time::{Duration, Instant};
    let mut child = Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    // Drain stdout on a thread: a child writing more than the pipe buffer (64 KB
    // on Linux) blocks until someone reads, so reading only after exit stalled it
    // until the cap and lost the output.
    let mut stdout = child.stdout.take()?;
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let mut buf = Vec::new();
        let ok = stdout.read_to_end(&mut buf).is_ok();
        let _ = tx.send(ok.then_some(buf));
    });
    let deadline = Instant::now() + Duration::from_secs(secs);
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                // A background grandchild can hold the pipe open after the child
                // exits; wait for EOF only until the cap.
                let left = deadline.saturating_duration_since(Instant::now());
                let out = rx.recv_timeout(left).ok().flatten()?;
                return status.success().then(|| String::from_utf8(out).ok()).flatten();
            }
            Ok(None) => {
                if Instant::now() >= deadline {
                    kill_and_reap(child);
                    return None;
                }
                std::thread::sleep(Duration::from_millis(20));
            }
            Err(_) => {
                kill_and_reap(child);
                return None;
            }
        }
    }
}

/// How a [`run_capped_status`] command ended.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Capped {
    /// Exited with status 0.
    Done,
    /// Exited non-zero (or was killed); the text is its first line of stderr.
    Failed(String),
    /// Still running when the cap hit. It is left running (reaped on a thread),
    /// not killed: for power actions that means the request was accepted and the
    /// command is waiting on the system (a suspend returns only after resume).
    StillRunning,
    /// The program could not be started (not installed, not executable).
    Missing(String),
}

/// Like [`run_capped`], but tells "failed" from "still running" and does not
/// kill a command that outlives the cap. Use for fire-and-forget system actions.
pub fn run_capped_status(program: &str, args: &[&str], secs: u64) -> Capped {
    use std::io::Read;
    use std::process::{Command, Stdio};
    use std::time::{Duration, Instant};
    let mut child = match Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
    {
        Ok(c) => c,
        Err(e) => return Capped::Missing(e.to_string()),
    };
    let deadline = Instant::now() + Duration::from_secs(secs);
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                if status.success() {
                    return Capped::Done;
                }
                let mut err = String::new();
                if let Some(s) = child.stderr.take() {
                    let _ = s.take(512).read_to_string(&mut err);
                }
                let first = err.lines().next().unwrap_or("").trim();
                return Capped::Failed(if first.is_empty() { format!("exit status {status}") } else { first.to_string() });
            }
            Ok(None) => {
                if Instant::now() >= deadline {
                    // Reap it in the background so it never lingers as a zombie.
                    std::thread::spawn(move || {
                        let _ = child.wait();
                    });
                    return Capped::StillRunning;
                }
                std::thread::sleep(Duration::from_millis(20));
            }
            Err(e) => return Capped::Failed(e.to_string()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[cfg(target_os = "linux")]
    fn capped_timeout_leaves_no_zombie() {
        assert_eq!(run_capped("sleep", &["31"], 1), None);
        std::thread::sleep(std::time::Duration::from_millis(400));
        let me = std::process::id().to_string();
        let zombies = std::fs::read_dir("/proc")
            .unwrap()
            .flatten()
            .filter_map(|e| std::fs::read_to_string(e.path().join("stat")).ok())
            .filter(|st| {
                // "pid (comm) S ppid ..."; comm may hold spaces, so split after the last ')'.
                let rest = st.rsplit_once(')').map(|(_, r)| r.trim_start()).unwrap_or("");
                let mut f = rest.split_whitespace();
                let (state, ppid) = (f.next(), f.next());
                st.contains("(sleep)") && state == Some("Z") && ppid == Some(me.as_str())
            })
            .count();
        assert_eq!(zombies, 0);
    }

    #[test]
    fn capped_returns_output_larger_than_the_pipe_buffer() {
        // 200 KB, three times a Linux pipe buffer: must arrive whole, well inside the cap.
        let t = std::time::Instant::now();
        let out = run_capped("sh", &["-c", "head -c 200000 /dev/zero | tr '\\0' x"], 5).expect("output");
        assert_eq!(out.len(), 200_000);
        assert!(t.elapsed() < std::time::Duration::from_secs(3), "took {:?}", t.elapsed());
        assert_eq!(run_capped("sh", &["-c", "echo hi; exit 1"], 5), None, "non-zero exit is None");
        assert_eq!(run_capped("tetron-wm-no-such-program", &[], 1), None);
    }

    #[test]
    fn capped_status_distinguishes_done_failed_running_and_missing() {
        assert_eq!(run_capped_status("true", &[], 5), Capped::Done);
        assert!(matches!(run_capped_status("sh", &["-c", "echo nope >&2; exit 3"], 5), Capped::Failed(m) if m == "nope"));
        assert!(matches!(run_capped_status("sh", &["-c", "exit 4"], 5), Capped::Failed(m) if m.contains('4')));
        assert_eq!(run_capped_status("sleep", &["3"], 1), Capped::StillRunning);
        assert!(matches!(run_capped_status("tetron-wm-no-such-program", &[], 1), Capped::Missing(_)));
    }

    #[test]
    fn route_get_parses_dev_and_src() {
        let out = "1.1.1.1 via 192.168.1.1 dev wlan0 src 192.168.1.42 uid 1000 \n    cache";
        assert_eq!(parse_ip_route_get(out), Some(("wlan0".into(), "192.168.1.42".into())));
        assert_eq!(parse_ip_route_get("RTNETLINK answers: Network is unreachable"), None);
    }

    #[test]
    fn macos_route_get_parses_interface() {
        let out = "   route to: default\n    gateway: 192.168.1.1\n  interface: en0\n";
        assert_eq!(parse_route_n_get(out).as_deref(), Some("en0"));
        assert_eq!(parse_route_n_get("route: writing to routing socket: not in table"), None);
    }

    #[test]
    fn mins_format() {
        assert_eq!(fmt_mins(125), "2:05");
        assert_eq!(fmt_mins(9), "0:09");
    }
}

#[cfg(target_os = "macos")]
pub mod macos;
#[cfg(target_os = "linux")]
pub mod linux;
/// Cross-platform tetron daemon status probe (Unix socket; any OS tetron runs on).
pub mod tetron;

#[cfg(test)]
mod interest_tests {
    use super::*;

    #[test]
    fn everything_is_wanted_until_a_session_says_otherwise() {
        let i = Interest::default();
        assert_eq!(i.want(), Want::ALL);
    }

    #[test]
    fn newly_wanted_parts_kick_the_poller_once() {
        let i = Interest::default();
        i.set(false, false);
        let k = i.kick_gen();
        i.set(false, false);
        assert_eq!(i.kick_gen(), k, "no change, no kick");
        i.set(true, false);
        assert_eq!(i.kick_gen(), k + 1, "net became wanted");
        i.set(true, false);
        assert_eq!(i.kick_gen(), k + 1);
        i.set(false, false);
        assert_eq!(i.kick_gen(), k + 1, "unwanting never kicks");
        assert_eq!(i.want(), Want { volume: true, net: false, bluetooth: false });
    }
}
