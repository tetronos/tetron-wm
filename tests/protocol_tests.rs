use tetron_wm::session::ClientMsg;
use tetron_wm::protocol::{FrameMsg, Flags};
use tetron_wm::compositor::CellChange;
use tetron_wm::cell::{Cell, Rgba};
use tetron_wm::geometry::Point;

#[test]
fn client_msg_roundtrips_json() {
    for msg in [
        ClientMsg::MouseDown(Point::new(3, 4)),
        ClientMsg::Key(vec![1, 2, 3]),
        ClientMsg::Resize { w: 80, h: 24, takeover: false },
        ClientMsg::OpenStore,
        ClientMsg::Shutdown,
    ] {
        let s = serde_json::to_string(&msg).unwrap();
        let _back: ClientMsg = serde_json::from_str(&s).unwrap();
    }
}

#[test]
fn frame_msg_roundtrips_json() {
    let f = FrameMsg {
        changes: vec![CellChange { x: 1, y: 2, cell: Cell { ch: 'A', fg: Rgba::rgb(1,2,3), bg: Rgba::rgb(4,5,6), attrs: Default::default() } }],
        spans: Vec::new(),
        cursor: Some(Point::new(5, 6)),
        flags: Flags { launcher_open: true, ..Default::default() },
        images: Vec::new(),
        image_data: Vec::new(),
        clear: true,
        clipboard: Some("copied text".into()),
        notice: None,
        fullscreen: None,
        switch_to: Some(tetron_wm::systems::SwitchSpec {
            name: "pi".into(),
            host: "pi@10.0.0.2".into(),
            port: Some(2222),
            theme: Some("nord".into()),
            setup: true,
            password: None,
        }),
    };
    let s = serde_json::to_string(&f).unwrap();
    let back: FrameMsg = serde_json::from_str(&s).unwrap();
    assert_eq!(back.changes.len(), 1);
    assert_eq!(back.changes[0].cell.ch, 'A');
    assert!(back.flags.launcher_open);
    assert!(back.clear);
    let spec = back.switch_to.expect("switch spec survives the round trip");
    assert_eq!(spec.host, "pi@10.0.0.2");
    assert!(spec.setup);
}

#[test]
fn frame_msg_clear_defaults_off_for_older_daemons() {
    // A frame from a daemon that predates `clear` (and `switch_to`) must still
    // parse, with the wipe defaulted off and no switch requested.
    let s = r#"{"changes":[],"cursor":null,"flags":{}}"#;
    let f: FrameMsg = serde_json::from_str(s).unwrap();
    assert!(!f.clear);
    assert!(f.switch_to.is_none());
    assert!(!f.flags.power_editing);
}

#[test]
fn new_mouse_messages_roundtrip() {
    use tetron_wm::session::ClientMsg;
    use tetron_wm::geometry::Point;
    for msg in [ClientMsg::MouseDouble(Point::new(3, 4)), ClientMsg::MouseRightDown(Point::new(5, 6))] {
        let s = serde_json::to_string(&msg).unwrap();
        let back: ClientMsg = serde_json::from_str(&s).unwrap();
        assert_eq!(serde_json::to_string(&back).unwrap(), s);
    }
}

#[test]
fn launcher_left_right_roundtrip() {
    use tetron_wm::session::ClientMsg;
    for msg in [ClientMsg::LauncherLeft, ClientMsg::LauncherRight] {
        let s = serde_json::to_string(&msg).unwrap();
        let back: ClientMsg = serde_json::from_str(&s).unwrap();
        assert_eq!(serde_json::to_string(&back).unwrap(), s);
    }
}

#[test]
fn old_resize_without_takeover_still_parses() {
    // A pre-`attach -d` client sends Resize with no `takeover` field.
    let msg: ClientMsg = serde_json::from_str(r#"{"Resize":{"w":80,"h":24}}"#).unwrap();
    assert!(matches!(msg, ClientMsg::Resize { w: 80, h: 24, takeover: false }));
}

#[test]
fn old_frame_without_notice_still_parses() {
    let f: FrameMsg = serde_json::from_str(r#"{"changes":[],"cursor":null,"flags":{}}"#).unwrap();
    assert!(f.notice.is_none());
}

#[test]
fn session_names_are_validated() {
    use tetron_wm::protocol::valid_session_name as ok;
    assert!(ok("work") && ok("a-b_C9") && ok("default"));
    assert!(!ok("") && !ok("a/b") && !ok("..") && !ok("a b") && !ok(&"x".repeat(33)));
}

#[test]
fn default_session_keeps_legacy_dir_named_nests() {
    use tetron_wm::protocol::{session_dir, socket_root};
    assert_eq!(session_dir("default"), socket_root());
    assert_eq!(session_dir("work"), socket_root().join("sessions").join("work"));
}

// ── Packed cell spans (daemon → client) ─────────────────────────────────────

/// A noisy 200x50 diff: every 3rd row fully changed in one style (text
/// output), the rest every other cell in random colours, with wide glyphs,
/// continuation cells, escapes and attributes mixed in. Deterministic (an LCG).
fn noisy_changes() -> Vec<CellChange> {
    use tetron_wm::cell::CellAttrs;
    let mut seed = 0x2545_f491_u32;
    let mut rnd = || { seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223); seed >> 8 };
    let mut out = Vec::new();
    for y in 0..50 {
        for x in 0..200 {
            if y % 3 != 0 && x % 2 == 1 { continue; }
            let r = rnd();
            let text = y % 3 == 0;
            let (fg, bg) = if text {
                (Rgba::rgb(200, 208, 220), Rgba::rgb(20, 22, 30))
            } else {
                (Rgba::new(r as u8, (r >> 8) as u8, 7, 255), Rgba::new(1, (r >> 4) as u8, 9, (r >> 12) as u8))
            };
            let ch = match r % 40 { 0 => '\u{1F4C1}', 1 => '\0', 2 => '"', 3 => '\\', _ => (b'a' + (r % 26) as u8) as char };
            let attrs = if text { CellAttrs::default() } else {
                CellAttrs { bold: r & 64 != 0, italic: r % 7 == 0, underline: r % 5 == 0, inverse: r % 11 == 0 }
            };
            out.push(CellChange { x, y, cell: Cell { ch, fg, bg, attrs } });
        }
    }
    out
}

#[test]
fn spans_round_trip_every_change_in_order() {
    use tetron_wm::protocol::{pack_changes, unpack_spans, Span};
    let changes = noisy_changes();
    let spans = pack_changes(&changes);
    assert!(spans.len() < changes.len(), "same-style runs merge");
    let json = serde_json::to_string(&spans).unwrap();
    let spans: Vec<Span> = serde_json::from_str(&json).unwrap();
    let mut back = Vec::new();
    unpack_spans(&spans, &mut back);
    assert_eq!(back, changes);
    // A gap splits a run; nothing is invented in between.
    let gap = [changes[0].clone(), CellChange { x: 5, ..changes[0].clone() }];
    assert_eq!(pack_changes(&gap).len(), 2);
}

#[test]
fn packed_frame_to_new_client_and_plain_frame_to_old_client() {
    use tetron_wm::protocol::{pack_changes, unpack_spans};
    let changes = noisy_changes();
    let frame = |changes, spans| FrameMsg {
        changes, spans, cursor: None, flags: Flags::default(), images: Vec::new(),
        image_data: Vec::new(), clear: false, switch_to: None, clipboard: None, notice: None, fullscreen: None,
    };
    // Unpacked (the client did not ask): the old format, no `spans` key.
    let plain = serde_json::to_string(&frame(changes.clone(), Vec::new())).unwrap();
    assert!(!plain.contains("\"spans\""));
    // Packed: `changes` stays present (empty), so an older parser still accepts it.
    let packed = serde_json::to_string(&frame(Vec::new(), pack_changes(&changes))).unwrap();
    assert!(packed.contains("\"changes\":[]"));
    let mut back: FrameMsg = serde_json::from_str(&packed).unwrap();
    unpack_spans(&back.spans, &mut back.changes);
    assert_eq!(back.changes, changes);
    // A frame from an older daemon (no `spans`), and one with neither field.
    let old: FrameMsg = serde_json::from_str(r#"{"changes":[],"cursor":null,"flags":{}}"#).unwrap();
    assert!(old.spans.is_empty());
    let bare: FrameMsg = serde_json::from_str(r#"{"cursor":null,"flags":{}}"#).unwrap();
    assert!(bare.changes.is_empty());
}

#[test]
fn packed_cells_cap_defaults_off_for_older_clients() {
    use tetron_wm::display::ClientCaps;
    // An older client's caps lack the field: the daemon must not pack.
    let old: ClientCaps = serde_json::from_str(r#"{"vt":false,"kitty_graphics":true}"#).unwrap();
    assert!(!old.packed_cells);
    let new = ClientCaps { packed_cells: true, ..Default::default() };
    let back: ClientCaps = serde_json::from_str(&serde_json::to_string(&new).unwrap()).unwrap();
    assert!(back.packed_cells);
}

/// Wire size and encode+decode time, per-cell objects vs spans, for a noisy
/// 200x50 frame. `cargo test --release --test protocol_tests -- --ignored --nocapture`
#[test]
#[ignore]
fn spans_size_and_speed() {
    use std::time::Instant;
    use tetron_wm::protocol::{pack_changes, unpack_spans, Span};
    let changes = noisy_changes();
    let n = 200;
    let t = Instant::now();
    let mut bytes_a = 0;
    for _ in 0..n {
        let s = serde_json::to_string(&changes).unwrap();
        bytes_a = s.len();
        let back: Vec<CellChange> = serde_json::from_str(&s).unwrap();
        assert_eq!(back.len(), changes.len());
    }
    let a = t.elapsed() / n;
    let t = Instant::now();
    let mut bytes_b = 0;
    for _ in 0..n {
        let s = serde_json::to_string(&pack_changes(&changes)).unwrap();
        bytes_b = s.len();
        let spans: Vec<Span> = serde_json::from_str(&s).unwrap();
        let mut back = Vec::new();
        unpack_spans(&spans, &mut back);
        assert_eq!(back.len(), changes.len());
    }
    let b = t.elapsed() / n;
    println!("{} cells: objects {bytes_a} B {a:?}, spans {bytes_b} B {b:?}", changes.len());
}
