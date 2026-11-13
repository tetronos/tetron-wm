use tetron_wm::system::{ClockInfo, MemInfo, SystemState, VolumeInfo, WifiInfo};
use tetron_wm::tray::{tray_segments, SegmentKind};

fn sample() -> SystemState {
    SystemState {
        clock: ClockInfo {
            time: "09:41".into(),
            date: "Wed 04 Jun".into(),
            uptime_secs: 0,
            year: 2026,
            month: 6,
            day: 4,
        },
        cpu_pct: 32.0,
        mem: MemInfo { used: 6, total: 10 },
        wifi: Some(WifiInfo { ssid: "wlan".into(), signal: 3, enabled: true }),
        volume: VolumeInfo { level: 60, muted: false },
        ..Default::default()
    }
}

#[test]
fn segments_are_right_aligned_and_on_row_zero() {
    let segs = tray_segments(&sample(), 100, 9, 0);
    // Volume is the farthest-right segment (a fixed eyes-free corner knob).
    let vol = segs.iter().find(|s| s.kind == SegmentKind::Volume).unwrap();
    let max_x = segs.iter().map(|s| s.rect.x + s.rect.w).max().unwrap();
    assert_eq!(vol.rect.x + vol.rect.w, max_x);
    assert!(segs.iter().all(|s| s.rect.y == 0));
}

#[test]
fn narrow_width_drops_cpu_but_keeps_clock() {
    let wide = tray_segments(&sample(), 100, 9, 0);
    let narrow = tray_segments(&sample(), 24, 9, 0);
    assert!(narrow.iter().any(|s| s.kind == SegmentKind::Clock));
    assert!(narrow.iter().any(|s| s.kind == SegmentKind::Volume), "volume is always kept");
    assert!(!narrow.iter().any(|s| s.kind == SegmentKind::Cpu));
    assert!(wide.iter().any(|s| s.kind == SegmentKind::Cpu));
}

#[test]
fn segments_do_not_overlap() {
    let segs = tray_segments(&sample(), 100, 9, 0);
    for pair in segs.windows(2) {
        assert!(pair[0].rect.x + pair[0].rect.w <= pair[1].rect.x);
    }
}

use tetron_wm::geometry::Point;
use tetron_wm::system::ControlIntent;
use tetron_wm::tray::Tray;

#[test]
fn clicking_a_segment_opens_then_closes_its_popover() {
    let mut tray = Tray::new();
    let segs = tray_segments(&sample(), 100, 9, 0);
    let vol = segs.iter().find(|s| s.kind == SegmentKind::Volume).unwrap();
    assert!(tray.on_menubar_click(Point::new(vol.rect.x, 0), &segs));
    assert_eq!(tray.open(), Some(SegmentKind::Volume));
    // Clicking the same segment again closes it.
    assert!(tray.on_menubar_click(Point::new(vol.rect.x, 0), &segs));
    assert_eq!(tray.open(), None);
}

#[test]
fn volume_popover_arrows_yield_intents() {
    let mut tray = Tray::new();
    tray.force_open(SegmentKind::Volume);
    let r = tray.render(100, 30, &sample());
    let up = r.hits.iter().find(|h| h.intent == ControlIntent::VolumeUp).unwrap();
    assert_eq!(tray.on_popover_click(up.rect.center(), &r), Some(ControlIntent::VolumeUp));
    let down = r.hits.iter().find(|h| h.intent == ControlIntent::VolumeDown).unwrap();
    assert_eq!(tray.on_popover_click(down.rect.center(), &r), Some(ControlIntent::VolumeDown));
}

#[test]
fn closed_tray_renders_nothing() {
    let tray = Tray::new();
    let r = tray.render(100, 30, &sample());
    assert!(r.layers.is_empty());
    assert!(r.bounds.is_none());
}

#[test]
fn clock_segment_is_time_only() {
    // The clock shows only the time now; the date/day live in the calendar popover.
    let segs = tray_segments(&sample(), 100, 9, 0);
    let clock = segs.iter().find(|s| s.kind == SegmentKind::Clock).unwrap();
    assert_eq!(clock.text, "09:41");
}

#[test]
fn clock_popover_is_a_calendar_with_month_nav() {
    let mut tray = Tray::new();
    tray.force_open(SegmentKind::Clock);
    let r = tray.render(100, 30, &sample());
    assert!(!r.layers.is_empty());
    let prev = r.hits.iter().find(|h| h.intent == ControlIntent::CalendarPrev);
    let next = r.hits.iter().find(|h| h.intent == ControlIntent::CalendarNext);
    assert!(prev.is_some() && next.is_some(), "calendar has ◂ ▸ month navigation");
    // Stepping a month changes the rendered grid (bounds stay a calendar box).
    tray.calendar_step(1);
    let r2 = tray.render(100, 30, &sample());
    assert!(r2.bounds.is_some());
}

#[test]
fn calendar_grid_marks_today() {
    // June 2026: the 4th is a Thursday — the calendar must place it in column 3.
    let weeks = tetron_wm::calendar::month_grid(2026, 6);
    assert_eq!(weeks[0][3], Some(4));
}

#[test]
fn bell_segment_appears_with_notifications() {
    let none = tray_segments(&sample(), 100, 9, 0);
    assert!(!none.iter().any(|s| s.kind == SegmentKind::Bell));
    let some = tray_segments(&sample(), 100, 9, 3);
    let bell = some.iter().find(|s| s.kind == SegmentKind::Bell).unwrap();
    assert!(bell.text.contains('3'));
}

#[test]
fn bell_popover_rows_focus_and_clear() {
    let mut tray = Tray::new();
    tray.notify(7, "claude".into(), "09:41".into());
    tray.notify(9, "btop".into(), "09:42".into());
    assert_eq!(tray.notif_count(), 2);
    tray.force_open(SegmentKind::Bell);
    let r = tray.render(100, 30, &sample());
    let focus = r.hits.iter().find(|h| h.intent == ControlIntent::NotifFocus(9));
    assert!(focus.is_some(), "newest notification row focuses its window");
    let clear = r.hits.iter().find(|h| h.intent == ControlIntent::NotifClear);
    assert!(clear.is_some(), "clear-all row present");
    tray.clear_notifs_for(9);
    assert!(!tray.has_notif_for(9));
    assert!(tray.has_notif_for(7));
    tray.clear_notifs();
    assert_eq!(tray.notif_count(), 0);
}

#[test]
fn calendar_lists_upcoming_events() {
    let mut st = sample();
    st.events = vec![
        tetron_wm::system::CalEvent { year: 2026, month: 6, day: 10, text: "09:30 Standup".into() },
    ];
    let mut tray = Tray::new();
    tray.force_open(SegmentKind::Clock);
    let r = tray.render(100, 30, &st);
    let buf = &r.layers[0].buf;
    let mut found = false;
    for y in 0..buf.height() {
        let row: String = (0..buf.width()).filter_map(|x| buf.get(x, y).map(|c| c.ch)).collect();
        if row.contains("Standup") {
            found = true;
        }
    }
    assert!(found, "event line rendered in the calendar popover");
}

#[test]
fn network_label_is_wifi_or_net_without_the_ssid() {
    use tetron_wm::system::NetInfo;
    let mut st = sample();
    st.wifi = Some(WifiInfo { ssid: "HomeNetwork5G".into(), signal: 3, enabled: true });
    let label = |st: &SystemState| tray_segments(st, 100, 9, 0).into_iter().find(|s| s.kind == SegmentKind::Wifi).map(|s| s.text);
    let t = label(&st).unwrap();
    assert!(t.ends_with(" wifi") && !t.contains("Home"), "label: {t:?}");
    // Wired default route wins over an idle WiFi radio.
    st.net = Some(NetInfo { iface: "eth0".into(), ip: "10.0.0.5".into(), wifi: false });
    assert!(label(&st).unwrap().ends_with(" net"));
    // Wired only, no WiFi hardware.
    st.wifi = None;
    assert!(label(&st).unwrap().ends_with(" net"));
    // No link and no WiFi: segment hidden.
    st.net = None;
    assert!(label(&st).is_none());
}

#[test]
fn network_popover_shows_ip_and_copies_it() {
    use tetron_wm::system::{ControlIntent, NetInfo};
    use tetron_wm::tray::Tray;
    let mut st = sample();
    st.net = Some(NetInfo { iface: "wlan0".into(), ip: "192.168.1.42".into(), wifi: true });
    let mut tray = Tray::new();
    tray.force_open(SegmentKind::Wifi);
    let r = tray.render(100, 30, &st);
    let text: String = r.layers.iter().flat_map(|l| {
        let b = &l.buf;
        (0..b.height()).flat_map(move |y| (0..b.width()).filter_map(move |x| b.get(x, y).map(|c| c.ch)).chain(std::iter::once('\n')))
    }).collect();
    assert!(text.contains("192.168.1.42 (wlan0)"), "{text}");
    assert!(text.contains("SSID  wlan"), "{text}");
    assert!(r.hits.iter().any(|h| h.intent == ControlIntent::CopyText("192.168.1.42".into())));
    // No link: Offline, no copy row.
    st.net = None;
    st.wifi = None;
    let r = tray.render(100, 30, &st);
    assert!(r.hits.iter().all(|h| !matches!(h.intent, ControlIntent::CopyText(_))));
}

#[test]
fn popover_offers_the_right_tool() {
    use tetron_wm::traytools::TrayTools;
    use tetron_wm::tray::segment_app;
    let cfg = TrayTools::new();
    let none = |_: &str| false;
    let all = |_: &str| true;
    // CPU: htop when present, else top.
    let cpu = segment_app(SegmentKind::Cpu, &cfg, &all).unwrap();
    assert_eq!((cpu.command.as_str(), cpu.label.as_str()), ("htop", "Open htop"));
    assert_eq!(segment_app(SegmentKind::Cpu, &cfg, &none).unwrap().command, "top");
    // Memory: free -h (held open), vm_stat without free, no action with neither.
    let mem = segment_app(SegmentKind::Mem, &cfg, &all).unwrap();
    assert_eq!((mem.command.as_str(), mem.label.as_str()), ("sh", "Open free -h"));
    assert!(mem.args[1].starts_with("free -h;"), "{:?}", mem.args);
    let only_vm = |b: &str| b == "vm_stat";
    assert!(segment_app(SegmentKind::Mem, &cfg, &only_vm).unwrap().args[1].starts_with("vm_stat;"));
    assert!(segment_app(SegmentKind::Mem, &cfg, &none).is_none());
    // Battery: jolt, else upower, else pmset, else no row.
    assert_eq!(segment_app(SegmentKind::Battery, &cfg, &all).unwrap().command, "jolt");
    let only_up = |b: &str| b == "upower";
    assert!(segment_app(SegmentKind::Battery, &cfg, &only_up).unwrap().args[1].starts_with("upower -i"));
    assert!(segment_app(SegmentKind::Battery, &cfg, &none).is_none());
    // Network: wlctl first, ip as the held fallback.
    assert_eq!(segment_app(SegmentKind::Wifi, &cfg, &all).unwrap().command, "wlctl");
    let only_ip = |b: &str| b == "ip";
    assert!(segment_app(SegmentKind::Wifi, &cfg, &only_ip).unwrap().args[1].starts_with("ip -br addr"));
    // Bluetooth: bluetuith first, bluetoothctl next.
    assert_eq!(segment_app(SegmentKind::Bluetooth, &cfg, &all).unwrap().command, "bluetuith");
    let only_ctl = |b: &str| b == "bluetoothctl";
    assert_eq!(segment_app(SegmentKind::Bluetooth, &cfg, &only_ctl).unwrap().command, "bluetoothctl");
    // The clock (calendar) opens calcurse.
    assert_eq!(segment_app(SegmentKind::Clock, &cfg, &all).unwrap().command, "calcurse");
}

#[test]
fn cpu_popover_has_a_clickable_open_row() {
    use tetron_wm::system::ControlIntent;
    use tetron_wm::tray::Tray;
    let mut tray = Tray::new();
    tray.force_open(SegmentKind::Cpu);
    let r = tray.render(100, 30, &sample());
    // top is always available, htop when installed: either way one OpenTool row.
    let hit = r.hits.iter().find(|h| matches!(h.intent, ControlIntent::OpenTool { .. }));
    let hit = hit.expect("CPU popover has an Open row");
    match &hit.intent {
        ControlIntent::OpenTool { title, command, .. } => {
            assert_eq!(title, "CPU");
            assert!(command == "htop" || command == "top", "{command}");
        }
        _ => unreachable!(),
    }
    // Clicking that row maps back to the same intent.
    let p = tetron_wm::geometry::Point::new(hit.rect.x + 1, hit.rect.y);
    assert!(matches!(tray.on_popover_click(p, &r), Some(ControlIntent::OpenTool { .. })));
}

#[test]
fn tray_never_covers_the_left_chrome_while_items_can_be_shed() {
    use tetron_wm::chrome::TASKBAR_X;
    // Every width from roomy down to just past what the always-kept segments
    // (volume + clock) need: no segment may start left of the taskbar.
    for width in 45..=120 {
        let segs = tray_segments(&sample(), width, 0, 0);
        let left = segs.iter().map(|s| s.rect.x).min().unwrap();
        assert!(left >= TASKBAR_X, "width {width}: tray starts at {left}, covers the launcher");
    }
}

#[test]
fn shedding_goes_in_order_cpu_first() {
    // Just too narrow for cpu: it goes before anything else the sample has.
    let full = tray_segments(&sample(), 120, 0, 0);
    let w = full.iter().map(|s| s.rect.w).sum::<i32>() + 2 * (full.len() as i32 - 1);
    let tight = tray_segments(&sample(), tetron_wm::chrome::TASKBAR_X + w - 1, 0, 0);
    assert!(!tight.iter().any(|s| s.kind == SegmentKind::Cpu));
    assert!(tight.iter().any(|s| s.kind == SegmentKind::Mem));
}

// ── Info menu, placement, overflow ───────────────────────────────────────────

use tetron_wm::system::BatteryInfo;
use tetron_wm::tray::{tray_layout, InfoView, Place, Placement, HOT_PCT};

fn layout(state: &SystemState, width: i32, bells: usize, p: &Placement) -> tetron_wm::tray::TrayLayout {
    tray_layout(state, width, 0, bells, Some((3, false)), false, p)
}

fn kinds(l: &tetron_wm::tray::TrayLayout) -> Vec<SegmentKind> {
    l.segments.iter().map(|s| s.kind).collect()
}

fn with_battery(mut s: SystemState) -> SystemState {
    s.battery = Some(BatteryInfo { pct: 50, charging: false, mins: None });
    s
}

#[test]
fn default_placement_puts_cpu_mem_and_bluetooth_in_info() {
    let mut s = with_battery(sample());
    s.caps.bluetooth = true;
    let l = layout(&s, 120, 0, &Placement::default());
    let bar = kinds(&l);
    for k in [SegmentKind::Cpu, SegmentKind::Mem, SegmentKind::Bluetooth] {
        assert!(!bar.contains(&k), "{k:?} is not on the bar by default");
        assert!(l.in_info.iter().any(|r| r.kind == k), "{k:?} is an Info row");
    }
    assert!(bar.contains(&SegmentKind::Battery) && bar.contains(&SegmentKind::Wifi));
    // Info rows follow the bar order.
    let order: Vec<_> = l.in_info.iter().map(|r| r.kind).collect();
    assert_eq!(order, vec![SegmentKind::Cpu, SegmentKind::Mem, SegmentKind::Bluetooth]);
}

#[test]
fn config_can_pin_cpu_back_to_the_bar() {
    let mut cfg = std::collections::BTreeMap::new();
    cfg.insert("cpu".to_string(), "bar".to_string());
    let p = Placement::from_config(&cfg);
    assert_eq!(p.get(SegmentKind::Cpu), Place::Bar);
    assert!(kinds(&layout(&sample(), 120, 0, &p)).contains(&SegmentKind::Cpu));
}

#[test]
fn a_hot_cpu_shows_on_the_bar_even_when_placed_in_info() {
    let mut s = sample();
    s.cpu_pct = HOT_PCT as f32;
    let l = layout(&s, 120, 0, &Placement::default());
    assert!(kinds(&l).contains(&SegmentKind::Cpu), "hot CPU is pinned to the bar");
    assert!(!l.in_info.iter().any(|r| r.kind == SegmentKind::Cpu));
    s.cpu_pct = HOT_PCT as f32 - 1.0;
    assert!(!kinds(&layout(&s, 120, 0, &Placement::default())).contains(&SegmentKind::Cpu));
}

#[test]
fn info_sits_between_bell_and_clock_at_a_fixed_distance_from_the_corner() {
    let p = Placement::default();
    let quiet = layout(&sample(), 100, 0, &p);
    let busy = layout(&with_battery(sample()), 100, 4, &p);
    let info_x = |l: &tetron_wm::tray::TrayLayout| l.segments.iter().find(|s| s.kind == SegmentKind::Info).unwrap().rect.x;
    assert_eq!(info_x(&quiet), info_x(&busy), "bells and batteries come and go without moving it");
    let k = kinds(&busy);
    let pos = |x: SegmentKind| k.iter().position(|y| *y == x).unwrap();
    assert!(pos(SegmentKind::Bell) < pos(SegmentKind::Info));
    assert_eq!(pos(SegmentKind::Info) + 1, pos(SegmentKind::Clock));
    assert_eq!(pos(SegmentKind::Clock) + 1, pos(SegmentKind::Volume));
}

#[test]
fn shed_segments_overflow_into_info_instead_of_vanishing() {
    let mut s = with_battery(sample());
    s.caps.bluetooth = true;
    let p = Placement::all_bar();
    let roomy = layout(&s, 120, 2, &p);
    assert!(roomy.in_info.is_empty());
    // Narrow: segments leave the bar in priority order, all into Info.
    let tight = layout(&s, 44, 2, &p);
    for k in [SegmentKind::Clipboard, SegmentKind::Volume, SegmentKind::Clock, SegmentKind::Info] {
        assert!(kinds(&tight).contains(&k), "{k:?} is never shed");
        assert!(!tight.in_info.iter().any(|r| r.kind == k));
    }
    assert!(!tight.in_info.is_empty());
    let on_bar = kinds(&tight);
    for r in &tight.in_info {
        assert!(!on_bar.contains(&r.kind), "{:?} is in Info, not on the bar too", r.kind);
    }
    // Nothing is lost: every segment is on the bar or in Info.
    for k in kinds(&roomy) {
        assert!(on_bar.contains(&k) || tight.in_info.iter().any(|r| r.kind == k), "{k:?} vanished");
    }
}

#[test]
fn bar_placement_is_shed_after_auto() {
    let mut s = with_battery(sample());
    s.caps.bluetooth = true;
    let mut cfg = std::collections::BTreeMap::new();
    cfg.insert("battery".to_string(), "bar".to_string());
    cfg.insert("cpu".to_string(), "auto".to_string());
    cfg.insert("mem".to_string(), "auto".to_string());
    cfg.insert("bluetooth".to_string(), "auto".to_string());
    let p = Placement::from_config(&cfg);
    // Whenever the pinned battery has been shed, every `auto` segment is already
    // in Info; and a width exists where autos are shed but battery stays.
    let autos = [SegmentKind::Cpu, SegmentKind::Mem, SegmentKind::Bluetooth, SegmentKind::Wifi];
    let mut kept_battery_while_shedding = false;
    for width in 40..120 {
        let l = layout(&s, width, 0, &p);
        let battery_shed = l.in_info.iter().any(|r| r.kind == SegmentKind::Battery);
        if battery_shed {
            assert!(!kinds(&l).iter().any(|k| autos.contains(k)), "width {width}: battery shed while an auto segment stayed");
        } else if !l.in_info.is_empty() {
            kept_battery_while_shedding = true;
        }
    }
    assert!(kept_battery_while_shedding);
}

fn info_view(l: &tetron_wm::tray::TrayLayout) -> InfoView {
    InfoView { session: "work".into(), windows: 3, version: "9.9.9", rows: l.in_info.clone() }
}

fn screen_text(r: &tetron_wm::tray::Rendered) -> String {
    let mut out = String::new();
    for l in &r.layers {
        for y in 0..l.buf.height() {
            out.extend((0..l.buf.width()).map(|x| l.buf.get(x, y).map(|c| c.ch).unwrap_or(' ')));
            out.push('\n');
        }
    }
    out
}

#[test]
fn info_popover_shows_session_facts_rows_and_links() {
    let mut s = sample();
    s.caps.bluetooth = true;
    let l = layout(&s, 100, 0, &Placement::default());
    let mut tray = Tray::new();
    tray.force_open(SegmentKind::Info);
    let r = tray.render_with(100, 30, &s, &info_view(&l));
    let text = screen_text(&r);
    assert!(text.contains("work") && text.contains("9.9.9"), "{text}");
    assert!(text.contains("Session") && text.contains("Windows") && text.contains("Version"), "{text}");
    for label in ["CPU", "Memory", "Bluetooth", "Sessions", "Logs", "Shortcuts", "Settings"] {
        assert!(text.contains(label), "missing {label}:\n{text}");
    }
    let intents: Vec<_> = r.hits.iter().map(|h| h.intent.clone()).collect();
    assert!(intents.contains(&ControlIntent::OpenSessions));
    assert!(intents.contains(&ControlIntent::OpenLogs));
    assert!(intents.contains(&ControlIntent::OpenHelp));
    assert!(intents.contains(&ControlIntent::OpenSettings));
    // Bluetooth has no direct tool: its row opens its own popover.
    assert!(intents.contains(&ControlIntent::OpenSegment("bluetooth".into())));
    // CPU runs its tool straight away (top is always a candidate).
    assert!(intents.iter().any(|i| matches!(i, ControlIntent::OpenTool { .. })));
}

#[test]
fn info_row_opens_the_segment_popover_in_place() {
    let mut s = sample();
    s.caps.bluetooth = true;
    let l = layout(&s, 100, 0, &Placement::default());
    let mut tray = Tray::new();
    tray.force_open(SegmentKind::Info);
    let r = tray.render_with(100, 30, &s, &info_view(&l));
    // Click the Bluetooth row.
    let hit = r.hits.iter().find(|h| h.intent == ControlIntent::OpenSegment("bluetooth".into())).unwrap();
    let intent = tray.on_popover_click(Point::new(hit.rect.x + 2, hit.rect.y), &r).unwrap();
    assert_eq!(intent, ControlIntent::OpenSegment("bluetooth".into()));
    tray.open_nested(SegmentKind::from_key("bluetooth").unwrap());
    assert_eq!(tray.open(), Some(SegmentKind::Bluetooth));
    let nested = screen_text(&tray.render_with(100, 30, &s, &info_view(&l)));
    assert!(nested.contains("Bluetooth"), "{nested}");
    assert!(!nested.contains("Sessions"), "the segment popover is not the Info menu");
}

#[test]
fn info_popover_keeps_every_row_inside_its_border() {
    for with_rows in [false, true] {
        let mut s = sample();
        s.caps.bluetooth = with_rows;
        let l = layout(&s, 100, 0, &Placement::default());
        let mut tray = Tray::new();
        tray.force_open(SegmentKind::Info);
        let r = tray.render_with(100, 30, &s, &info_view(&l));
        let layer = &r.layers[0];
        let h = layer.buf.height();
        // The bottom row is pure border, and the last row of text sits above it.
        let bottom: String = (0..layer.buf.width()).map(|x| layer.buf.get(x, h - 1).unwrap().ch).collect();
        assert!(bottom.chars().all(|c| "╰─╯".contains(c)), "bottom row is the border: {bottom:?}");
        let last: String = (0..layer.buf.width()).map(|x| layer.buf.get(x, h - 2).unwrap().ch).collect();
        assert!(last.contains("Settings"), "the last link sits just above the border: {last:?}");
        // Every hit rect is inside the box.
        let b = r.bounds.unwrap();
        assert!(r.hits.iter().all(|h| h.rect.y > b.y && h.rect.y < b.y + b.h - 1));
    }
}
