//! Background poller that refreshes a shared [`SystemState`] on throttled
//! cadences. Clock + CPU/memory update ~every second; the shell-backed metrics
//! (WiFi/volume/Bluetooth) refresh ~every three seconds. All shell-outs are
//! timeout-guarded, so the poller can never hang the desktop.

use crate::protocol::DaemonStatus;
use crate::system::{backend, BatteryInfo, ClockInfo, Interest, SystemState, Want};
use std::sync::atomic::Ordering::Relaxed;
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};

/// Lets the poller threads pause while no client is attached (nobody can see
/// the tray, so the shell-outs, probes and file reads are wasted work) and wake
/// at once when one attaches.
#[derive(Clone)]
struct Gate(Arc<DaemonStatus>, Arc<Interest>);

/// How often a paused poller re-checks for an attach.
const PAUSE_STEP: Duration = Duration::from_millis(100);

impl Gate {
    /// Block while detached. Returns whether it had to wait, so the caller can
    /// refresh everything immediately on resume.
    fn wait_attached(&self) -> bool {
        let mut waited = false;
        while !self.0.attached.load(Relaxed) {
            waited = true;
            std::thread::sleep(PAUSE_STEP);
        }
        waited
    }

    /// Sleep up to `d`, waking early when a client attaches or a part of the
    /// UI that was hidden becomes visible (so its data is read at once).
    fn sleep(&self, d: Duration) {
        let gen = (self.0.attach_gen.load(Relaxed), self.1.kick_gen());
        let start = Instant::now();
        while start.elapsed() < d {
            std::thread::sleep(PAUSE_STEP.min(d));
            if (self.0.attach_gen.load(Relaxed), self.1.kick_gen()) != gen {
                return;
            }
        }
    }
}

/// Owns a background thread that periodically refreshes a shared snapshot.
pub struct SystemPoller {
    state: Arc<RwLock<SystemState>>,
}

impl SystemPoller {
    /// Spawn the poller thread and return a handle whose `state()` is updated in
    /// place. The thread runs for the lifetime of the process (daemon).
    ///
    /// While `status.attached` is false (the session is detached) every poll is
    /// paused; the next attach refreshes everything straight away. `interest`
    /// says which shell-backed parts the UI currently shows; unseen ones are not
    /// read.
    pub fn start(status: Arc<DaemonStatus>, interest: Arc<Interest>) -> Self {
        let state = Arc::new(RwLock::new(SystemState::default()));
        let worker = state.clone();
        let gate = Gate(status, interest);
        std::thread::spawn(move || run(worker, gate));
        SystemPoller { state }
    }

    /// The shared snapshot handle (cloned into the session).
    pub fn state(&self) -> Arc<RwLock<SystemState>> {
        self.state.clone()
    }
}

fn run(state: Arc<RwLock<SystemState>>, gate: Gate) {
    let backend = backend();
    let mut sys = sysinfo::System::new_all();
    let mut last_slow = Instant::now()
        .checked_sub(Duration::from_secs(10))
        .unwrap_or_else(Instant::now);
    let mut seen_kick = gate.1.kick_gen();
    let caps = backend.caps();
    // The network and Bluetooth reads (nmcli, bluetoothctl) start D-Bus clients
    // and cost far more than the volume read, so they refresh less often.
    let mut last_radio = last_slow;
    let mut clock = Clock::default();

    // Remote reachability + calendar events refresh on their own (slowest)
    // cadence, on a separate thread so a slow ssh-port probe or `khal` run can
    // never delay the clock/CPU updates.
    {
        let state = state.clone();
        let gate = gate.clone();
        std::thread::spawn(move || loop {
            gate.wait_attached();
            let online = probe_remotes();
            let events = read_khal_events();
            if let Ok(mut s) = state.write() {
                s.remotes_online = online;
                s.events = events;
            }
            gate.sleep(Duration::from_secs(20));
        });
    }

    // tetron status on its own thread + cadence: the probe is a Unix-socket
    // round-trip (timeout-guarded in `system::tetron`), kept off the clock/CPU
    // path so a slow/absent daemon can never delay the rest of the desktop.
    {
        let state = state.clone();
        let gate = gate.clone();
        std::thread::spawn(move || loop {
            gate.wait_attached();
            let tetron = crate::system::tetron::poll();
            if let Ok(mut s) = state.write() {
                s.tetron = tetron;
            }
            gate.sleep(Duration::from_secs(4));
        });
    }

    loop {
        // Detached: nobody can see the tray, so do nothing until a client
        // attaches, then refresh everything (including the slow reads) at once.
        if gate.wait_attached() {
            last_slow = Instant::now().checked_sub(Duration::from_secs(10)).unwrap_or_else(Instant::now);
            last_radio = Instant::now().checked_sub(Duration::from_secs(60)).unwrap_or_else(Instant::now);
            clock = Clock::default();
        }
        sys.refresh_cpu_usage();
        sys.refresh_memory();
        let cpu_pct = sys.global_cpu_usage();
        let (used, total) = (sys.used_memory(), sys.total_memory());
        let clock_now = clock.now();
        let battery = read_battery();
        // A few sysfs reads, like the battery: cheap enough every second.
        let brightness = if cfg!(target_os = "linux") { crate::brightness::read_pct() } else { None };

        // The shell-backed metrics refresh less often than CPU/clock, or at once
        // when a hidden part becomes visible. Only the parts the UI shows are read.
        let kick = gate.1.kick_gen();
        let kicked = kick != seen_kick;
        let slow = last_slow.elapsed() >= SLOW_EVERY || kicked;
        let radio = last_radio.elapsed() >= RADIO_EVERY || kicked;
        seen_kick = kick;
        let shown = gate.1.want();
        let want = Want { volume: shown.volume, net: shown.net && radio, bluetooth: shown.bluetooth && radio };
        let readout = if slow {
            last_slow = Instant::now();
            if radio {
                last_radio = last_slow;
            }
            Some(backend.read_parts(want))
        } else {
            None
        };

        if let Ok(mut s) = state.write() {
            s.clock = clock_now;
            s.cpu_pct = cpu_pct;
            s.mem.used = used;
            s.mem.total = total;
            s.battery = battery;
            s.brightness = brightness;
            // Capabilities are cheap and fixed; always set so segments that depend
            // on them (Bluetooth) exist even when their data is not being read.
            s.caps = caps;
            if let Some(r) = readout {
                if want.net {
                    s.wifi = r.wifi;
                    s.net = r.net;
                    s.known_networks = r.known_networks;
                }
                if want.bluetooth {
                    s.bluetooth = r.bluetooth;
                }
                if want.volume {
                    s.volume = r.volume;
                }
            }
        }

        gate.sleep(Duration::from_secs(1));
    }
}

/// Volume refresh interval (and the cadence of the slow-read check).
const SLOW_EVERY: Duration = Duration::from_secs(3);
/// WiFi, link, known networks and Bluetooth refresh interval. Showing a hidden
/// part, attaching, or a tray action (a kick) refreshes them at once.
const RADIO_EVERY: Duration = Duration::from_secs(10);

/// The menubar clock. The shown fields change only at a minute boundary (time
/// zone offsets are whole minutes), so `date` runs once a minute, not every
/// second; uptime comes from sysinfo each call.
#[derive(Default)]
struct Clock {
    minute: Option<u64>,
    info: ClockInfo,
}

impl Clock {
    fn now(&mut self) -> ClockInfo {
        let minute = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() / 60)
            .unwrap_or(0);
        if self.minute != Some(minute) {
            // Local time via `date` (cheap, avoids a chrono dependency), one call.
            let out = crate::system::run_capped("date", &["+%H:%M%n%a %d %b%n%Y-%m-%d"], 1).unwrap_or_default();
            self.info = parse_date_lines(&out);
            // A failed run is retried next second instead of kept for a minute.
            self.minute = (!out.is_empty()).then_some(minute);
        }
        ClockInfo { uptime_secs: sysinfo::System::uptime(), ..self.info.clone() }
    }
}

/// Parse `date +%H:%M%n%a %d %b%n%Y-%m-%d`: time, date, then the civil date for
/// the menubar calendar.
fn parse_date_lines(out: &str) -> ClockInfo {
    let mut lines = out.lines().map(str::trim);
    let time = lines.next().unwrap_or_default().to_string();
    let date = lines.next().unwrap_or_default().to_string();
    let mut parts = lines.next().unwrap_or_default().splitn(3, '-');
    let year = parts.next().and_then(|s| s.parse().ok()).unwrap_or(0);
    let month = parts.next().and_then(|s| s.parse().ok()).unwrap_or(0);
    let day = parts.next().and_then(|s| s.parse().ok()).unwrap_or(0);
    ClockInfo { time, date, uptime_secs: 0, year, month, day }
}

/// Battery is reported only on hosts that have one; `None` hides the segment.
/// Linux reads sysfs directly; macOS shells out to `pmset` (timeout-guarded).
fn read_battery() -> Option<BatteryInfo> {
    #[cfg(target_os = "linux")]
    {
        let rd = std::fs::read_dir("/sys/class/power_supply").ok()?;
        for e in rd.flatten() {
            let name = e.file_name().to_string_lossy().to_string();
            if !name.starts_with("BAT") {
                continue;
            }
            let p = e.path();
            let cap = std::fs::read_to_string(p.join("capacity"))
                .ok()
                .and_then(|s| s.trim().parse::<u8>().ok());
            if let Some(pct) = cap {
                let charging = std::fs::read_to_string(p.join("status"))
                    .map(|s| matches!(s.trim(), "Charging" | "Full"))
                    .unwrap_or(false);
                let rd = |f: &str| {
                    std::fs::read_to_string(p.join(f)).ok().and_then(|s| s.trim().parse::<u64>().ok())
                };
                // energy_* (µWh) with power_now (µW), or charge_* (µAh) with current_now (µA).
                let (now, full, rate) = if rd("energy_now").is_some() {
                    (rd("energy_now"), rd("energy_full"), rd("power_now"))
                } else {
                    (rd("charge_now"), rd("charge_full"), rd("current_now"))
                };
                let full_state = std::fs::read_to_string(p.join("status")).map(|s| s.trim() == "Full").unwrap_or(false);
                let mins = if full_state { None } else { battery_mins(now, full, rate, charging) };
                return Some(BatteryInfo { pct: pct.min(100), charging, mins });
            }
        }
        None
    }
    #[cfg(target_os = "macos")]
    {
        let out = crate::system::run_capped("pmset", &["-g", "batt"], 1)?;
        parse_pmset_batt(&out)
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        None
    }
}

/// Minutes to empty (discharging) or to full (charging) from sysfs counters.
/// `None` when a counter is missing or the rate is zero.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn battery_mins(now: Option<u64>, full: Option<u64>, rate: Option<u64>, charging: bool) -> Option<u32> {
    let (now, full, rate) = (now?, full?, rate?);
    if rate == 0 {
        return None;
    }
    let left = if charging { full.saturating_sub(now) } else { now };
    Some((left as u128 * 60 / rate as u128).min(u32::MAX as u128) as u32)
}

/// Probe each saved remote system's ssh port (TCP connect, 400ms cap) so the
/// Systems menu can show a live ●/○ reachability dot.
fn probe_remotes() -> Vec<(String, bool)> {
    let mut out = Vec::new();
    for sys in crate::systems::load() {
        let host = sys.host.rsplit('@').next().unwrap_or(&sys.host).to_string();
        let port = sys.port.unwrap_or(22);
        let online = std::net::ToSocketAddrs::to_socket_addrs(&(host.as_str(), port))
            .ok()
            .and_then(|mut addrs| addrs.next())
            .map(|addr| {
                std::net::TcpStream::connect_timeout(&addr, Duration::from_millis(400)).is_ok()
            })
            .unwrap_or(false);
        out.push((sys.name, online));
    }
    out
}

/// Upcoming events for the calendar popover, via `khal` when installed (the
/// catalog's terminal calendar). Missing khal → empty, popover shows none.
fn read_khal_events() -> Vec<crate::system::CalEvent> {
    let Some(out) = crate::system::run_capped(
        "khal",
        &["list", "--format", "{start-date-full}|{start-time} {title}", "today", "30d"],
        4,
    ) else {
        return Vec::new();
    };
    parse_khal_events(&out)
}

/// Parse khal `--format "{start-date-full}|{start-time} {title}"` lines. Day
/// headers and anything without the `|` or a parseable Y-M-D date are skipped,
/// so an unexpected khal config degrades to fewer events, never garbage.
fn parse_khal_events(out: &str) -> Vec<crate::system::CalEvent> {
    let mut events = Vec::new();
    for line in out.lines() {
        let Some((date, text)) = line.split_once('|') else { continue };
        let Some((y, m, d)) = parse_ymd(date.trim()) else { continue };
        let text = text.trim();
        if text.is_empty() {
            continue;
        }
        events.push(crate::system::CalEvent { year: y, month: m, day: d, text: text.to_string() });
        if events.len() >= 30 {
            break;
        }
    }
    events
}

/// Parse a date as `YYYY-MM-DD` or `DD.MM.YYYY` (khal's `start-date-full`
/// honours the user's locale dateformat; these cover the common defaults).
fn parse_ymd(s: &str) -> Option<(i32, u32, u32)> {
    let parts: Vec<&str> = if s.contains('-') {
        s.splitn(3, '-').collect()
    } else if s.contains('.') {
        let mut v: Vec<&str> = s.splitn(3, '.').collect();
        v.reverse(); // DD.MM.YYYY → YYYY MM DD
        v
    } else {
        return None;
    };
    if parts.len() != 3 {
        return None;
    }
    let y: i32 = parts[0].trim().parse().ok()?;
    let m: u32 = parts[1].trim().parse().ok()?;
    let d: u32 = parts[2].trim().parse().ok()?;
    if !(1..=12).contains(&m) || !(1..=31).contains(&d) || y < 1970 {
        return None;
    }
    Some((y, m, d))
}

/// Parse `pmset -g batt` output, e.g.
/// `Now drawing from 'AC Power'\n -InternalBattery-0 (id=…)  85%; charging; …`.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn parse_pmset_batt(out: &str) -> Option<BatteryInfo> {
    let pct = out
        .split_whitespace()
        .find_map(|w| w.strip_suffix("%;").or_else(|| w.strip_suffix('%')))
        .and_then(|n| n.parse::<u8>().ok())?;
    // "discharging" contains "charging", so match the delimited token.
    let charging = out.contains("AC Power") || out.contains("; charging") || out.contains("; charged");
    // "3:10 remaining" / "0:42 until full" (absent while "(no estimate)").
    let mins = out.split_whitespace().find_map(|w| {
        let (h, m) = w.split_once(':')?;
        Some(h.parse::<u32>().ok()? * 60 + m.parse::<u32>().ok()?)
    });
    Some(BatteryInfo { pct: pct.min(100), charging, mins })
}

#[cfg(test)]
mod gate_tests {
    use super::*;

    fn gate(attached: bool) -> (Gate, Arc<DaemonStatus>) {
        let st = Arc::new(DaemonStatus::default());
        st.attached.store(attached, Relaxed);
        (Gate(st.clone(), Arc::new(Interest::default())), st)
    }

    #[test]
    fn attached_does_not_wait() {
        let (g, _) = gate(true);
        assert!(!g.wait_attached());
    }

    #[test]
    fn detached_blocks_until_attach_then_reports_it_waited() {
        let (g, st) = gate(false);
        let t = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(250));
            st.attached.store(true, Relaxed);
        });
        let start = Instant::now();
        assert!(g.wait_attached());
        assert!(start.elapsed() >= Duration::from_millis(200), "it really paused");
        t.join().unwrap();
    }

    #[test]
    fn sleep_wakes_early_on_attach() {
        let (g, st) = gate(true);
        let t = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(150));
            st.attach_gen.fetch_add(1, Relaxed);
        });
        let start = Instant::now();
        g.sleep(Duration::from_secs(10));
        assert!(start.elapsed() < Duration::from_secs(2), "woke early");
        t.join().unwrap();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pmset_output_parses_pct_and_charging() {
        let charging = "Now drawing from 'AC Power'\n -InternalBattery-0 (id=123)\t85%; charging; 0:42 remaining present: true";
        let b = parse_pmset_batt(charging).unwrap();
        assert_eq!(b.pct, 85);
        assert!(b.charging);

        let discharging = "Now drawing from 'Battery Power'\n -InternalBattery-0 (id=123)\t47%; discharging; 3:10 remaining present: true";
        let b = parse_pmset_batt(discharging).unwrap();
        assert_eq!(b.pct, 47);
        assert!(!b.charging, "'discharging' must not read as charging");

        assert_eq!(parse_pmset_batt(charging).unwrap().mins, Some(42));
        assert_eq!(parse_pmset_batt(discharging).unwrap().mins, Some(190));
        assert!(parse_pmset_batt("Now drawing from 'AC Power'").is_none(), "no battery line → None");
    }

    #[test]
    fn battery_minutes_from_counters() {
        assert_eq!(battery_mins(Some(30_000), Some(50_000), Some(15_000), false), Some(120));
        assert_eq!(battery_mins(Some(30_000), Some(50_000), Some(20_000), true), Some(60));
        assert_eq!(battery_mins(Some(1), Some(2), Some(0), false), None);
        assert_eq!(battery_mins(None, Some(2), Some(1), false), None);
    }

    #[test]
    fn khal_lines_parse_and_headers_skip() {
        let out = "Today, 2026-06-10\n2026-06-10|09:30 Standup\n2026-06-11| All-day thing\nnot an event\n15.06.2026|14:00 Dentist\n";
        let ev = parse_khal_events(out);
        assert_eq!(ev.len(), 3);
        assert_eq!((ev[0].year, ev[0].month, ev[0].day), (2026, 6, 10));
        assert_eq!(ev[0].text, "09:30 Standup");
        assert_eq!((ev[2].year, ev[2].month, ev[2].day), (2026, 6, 15), "DD.MM.YYYY accepted");
    }

    #[test]
    fn bad_dates_rejected() {
        assert!(parse_ymd("garbage").is_none());
        assert!(parse_ymd("2026-13-01").is_none());
        assert_eq!(parse_ymd("2026-06-10"), Some((2026, 6, 10)));
        assert_eq!(parse_ymd("10.06.2026"), Some((2026, 6, 10)));
    }

    #[test]
    fn date_lines_parse_time_date_and_civil_date() {
        let c = parse_date_lines("14:05\nThu 22 Oct\n2026-10-22\n");
        assert_eq!((c.time.as_str(), c.date.as_str()), ("14:05", "Thu 22 Oct"));
        assert_eq!((c.year, c.month, c.day), (2026, 10, 22));
        let empty = parse_date_lines("");
        assert_eq!((empty.time.as_str(), empty.year), ("", 0), "a failed `date` leaves the clock blank");
    }

    #[test]
    fn clock_runs_date_once_per_minute() {
        let mut c = Clock::default();
        let first = c.now();
        if first.time.is_empty() {
            return; // no `date` on this host
        }
        let minute = c.minute;
        assert!(minute.is_some());
        // Within the same minute the cached fields are reused, not re-read.
        c.info.time = "cached".into();
        if c.minute == minute && c.now().time != "cached" {
            // The minute rolled over between the two calls; the re-read is correct.
            assert_ne!(c.minute, minute);
        }
    }
}
