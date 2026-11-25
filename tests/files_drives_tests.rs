//! Files left pane: the Drives section, its menus and keyboard focus (Tab).
//! The drives are set by hand; mounting itself is covered by the VM test.

use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;
use tetron_drives::Drive;
use tetron_wm::contextmenu::MenuAction as A;
use tetron_wm::filemanager::{FileManager, Overlay, Target, SIDEBAR_W, SIDEBAR_W_DRIVES};
use tetron_wm::geometry::Point;

fn tmp(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("tetron-wm-fmd-{}-{}", tag, std::process::id()));
    let _ = fs::remove_dir_all(&d);
    fs::create_dir_all(&d).unwrap();
    d
}

fn drive(name: &str, label: Option<&str>, mounted: Option<PathBuf>, removable: bool, locked: bool) -> Drive {
    Drive {
        dev: format!("/dev/{name}"),
        disk: format!("/dev/{}", name.trim_end_matches(char::is_numeric)),
        name: name.into(),
        label: label.map(str::to_string),
        uuid: Some("1234-ABCD".into()),
        size: 31_000_000_000,
        fstype: Some(if locked { "crypto_LUKS" } else { "exfat" }.into()),
        mountpoint: mounted,
        removable,
        bus: Some(if removable { "usb" } else { "sata" }.into()),
        model: Some("Cruzer".into()),
        locked,
        luks: None,
    }
}

fn row(buf: &tetron_wm::buffer::CellBuffer, y: i32, w: i32) -> String {
    (0..w).map(|x| buf.get(x, y).map_or(' ', |c| c.ch)).collect::<String>().trim_end().to_string()
}

fn menu_items(fm: &FileManager) -> Vec<A> {
    match fm.overlay() {
        Some(Overlay::DriveMenu { menu, .. }) => menu.items.clone(),
        other => panic!("no drive menu: {other:?}"),
    }
}

/// Places (6 rows), a blank, "Drives", then the drives: drive j sits at top + 8 + j.
const DRIVE0: i32 = 8;

#[test]
fn drives_section_lists_drives_and_widens_the_pane() {
    let d = tmp("render");
    let mut fm = FileManager::new(d.clone(), BTreeMap::new());
    assert_eq!(fm.sidebar_w(), SIDEBAR_W, "no drives: the pane keeps its width");
    let top = fm.listing_top();
    assert!(!row(&fm.render(80, 24), top + DRIVE0 - 1, 20).contains("Drives"));

    fm.set_drives(vec![drive("sdb1", Some("STICK"), Some(d.clone()), true, false), drive("sda2", None, None, false, false), drive("sdc1", Some("vault"), None, true, true)]);
    assert_eq!(fm.sidebar_w(), SIDEBAR_W_DRIVES);
    let buf = fm.render(80, 24);
    assert_eq!(row(&buf, top + DRIVE0 - 1, SIDEBAR_W_DRIVES), "Drives");
    let stick = row(&buf, top + DRIVE0, SIDEBAR_W_DRIVES - 1);
    assert!(stick.starts_with(" STICK") && stick.ends_with("29G\u{25B2}"), "mounted removable: name, size, eject mark: {stick:?}");
    let internal = row(&buf, top + DRIVE0 + 1, SIDEBAR_W_DRIVES - 1);
    assert!(internal.starts_with(" sda2") && internal.ends_with("29G"), "no label: the device name, no mark: {internal:?}");
    assert!(row(&buf, top + DRIVE0 + 2, SIDEBAR_W_DRIVES - 1).ends_with("locked"));
    // Not mounted is dimmer than mounted.
    let fg = |y| buf.get(2, y).unwrap().fg;
    assert_ne!(fg(top + DRIVE0), fg(top + DRIVE0 + 1));
    let _ = fs::remove_dir_all(&d);
}

#[test]
fn hit_test_maps_drive_rows_and_keeps_places() {
    let d = tmp("hit");
    let mut fm = FileManager::new(d.clone(), BTreeMap::new());
    fm.set_drives(vec![drive("sdb1", Some("STICK"), None, true, false)]);
    let top = fm.listing_top();
    assert_eq!(fm.hit_test(Point::new(3, top), 80, 24), Some(Target::Sidebar(0)));
    assert_eq!(fm.hit_test(Point::new(3, top + DRIVE0 - 1), 80, 24), None, "the header is not a target");
    assert_eq!(fm.hit_test(Point::new(18, top + DRIVE0), 80, 24), Some(Target::Drive(0)));
    let _ = fs::remove_dir_all(&d);
}

#[test]
fn click_on_a_not_mounted_drive_offers_mount_and_does_not_mount() {
    let d = tmp("click");
    let mut fm = FileManager::new(d.clone(), BTreeMap::new());
    fm.set_drives(vec![drive("sdb1", Some("STICK"), None, true, false)]);
    let top = fm.listing_top();
    assert!(fm.handle_click(Point::new(3, top + DRIVE0), 80, 24, false, false));
    assert_eq!(menu_items(&fm), [A::DriveMountOpen, A::DriveMount, A::DriveEject, A::DriveProperties]);
    assert_eq!(fm.cwd(), d.as_path(), "nothing opened yet");
    assert_eq!(fm.status(), "", "no job started");
    let _ = fs::remove_dir_all(&d);
}

#[test]
fn click_on_a_mounted_drive_opens_it() {
    let d = tmp("open");
    let mnt = d.join("STICK");
    fs::create_dir(&mnt).unwrap();
    fs::write(mnt.join("photo.jpg"), b"x").unwrap();
    let mut fm = FileManager::new(d.clone(), BTreeMap::new());
    fm.set_drives(vec![drive("sdb1", Some("STICK"), Some(mnt.clone()), true, false)]);
    fm.handle_click(Point::new(3, fm.listing_top() + DRIVE0), 80, 24, false, false);
    assert_eq!(fm.cwd(), mnt.as_path());
    assert_eq!(fm.entries()[0].name, "photo.jpg");
    let _ = fs::remove_dir_all(&d);
}

#[test]
fn menus_for_mounted_internal_and_locked_drives() {
    let d = tmp("menus");
    let mut fm = FileManager::new(d.clone(), BTreeMap::new());
    fm.set_drives(vec![drive("sda2", None, Some(d.clone()), false, false), drive("sdc1", None, None, true, true)]);
    fm.begin_drive_menu(0, Point::new(1, 1));
    assert_eq!(menu_items(&fm), [A::Open, A::DriveUnmount, A::CopyPath, A::DriveProperties], "internal: no eject");
    fm.begin_drive_menu(1, Point::new(1, 1));
    assert_eq!(menu_items(&fm), [A::DriveUnlockOpen, A::DriveEject, A::DriveProperties]);
    let _ = fs::remove_dir_all(&d);
}

#[test]
fn unlock_asks_a_masked_passphrase() {
    let d = tmp("pass");
    let mut fm = FileManager::new(d.clone(), BTreeMap::new());
    fm.set_drives(vec![drive("sdc1", Some("vault"), None, true, true)]);
    fm.begin_drive_menu(0, Point::new(1, 1));
    fm.context_menu_commit(); // Unlock and open…
    assert!(fm.is_editing(), "typed keys go to the passphrase box");
    for c in "s3cret".chars() {
        fm.overlay_char(c);
    }
    let buf = fm.render(80, 24);
    let all: String = (0..24).map(|y| row(&buf, y, 80)).collect::<Vec<_>>().join("\n");
    assert!(all.contains("Unlock vault"));
    assert!(all.contains("\u{2022}\u{2022}\u{2022}\u{2022}\u{2022}\u{2022}"), "six dots");
    assert!(!all.contains("s3cret"), "never shown");
    fm.cancel_overlay();
    let _ = fs::remove_dir_all(&d);
}

#[test]
fn properties_show_device_filesystem_and_state() {
    let d = tmp("props");
    let mut fm = FileManager::new(d.clone(), BTreeMap::new());
    fm.set_drives(vec![drive("sdb1", Some("STICK"), None, true, false)]);
    fm.begin_drive_menu(0, Point::new(1, 1));
    fm.context_menu_down();
    fm.context_menu_down();
    fm.context_menu_down(); // Properties
    fm.context_menu_commit();
    assert!(fm.dialog_open());
    let buf = fm.render(80, 24);
    let all: String = (0..24).map(|y| row(&buf, y, 80)).collect::<Vec<_>>().join("\n");
    for want in ["Device: /dev/sdb1", "Disk: Cruzer (/dev/sdb)", "Filesystem: exfat", "Connected by: usb, removable", "Not mounted"] {
        assert!(all.contains(want), "{want} missing:\n{all}");
    }
    let _ = fs::remove_dir_all(&d);
}

#[test]
fn tab_moves_focus_to_the_pane_and_enter_works_there() {
    let d = tmp("tab");
    let home = dirs::home_dir().unwrap();
    let mut fm = FileManager::new(d.clone(), BTreeMap::new());
    fm.set_drives(vec![drive("sdb1", Some("STICK"), None, true, false)]);
    fm.toggle_pane_focus();
    assert!(fm.pane_focused());
    fm.activate(); // Home, the first place
    assert_eq!(fm.cwd(), home.as_path());
    assert!(!fm.pane_focused(), "opening a place hands focus back to the listing");

    fm.toggle_pane_focus();
    for _ in 0..20 {
        fm.move_cursor(0, 1); // past the places onto the drive, clamped there
    }
    fm.activate();
    assert_eq!(menu_items(&fm)[0], A::DriveMountOpen, "Enter on a not mounted drive offers mounting");
    fm.cancel_overlay();
    fm.begin_context(); // the menu key does the same in the pane
    assert!(fm.context_menu_open());
    fm.cancel_overlay();
    fm.toggle_pane_focus();
    assert!(!fm.pane_focused(), "Tab again: back to the listing");
    let _ = fs::remove_dir_all(&d);
}

#[test]
fn a_drive_that_went_away_closes_nothing_and_clamps_the_pane_cursor() {
    let d = tmp("gone");
    let mut fm = FileManager::new(d.clone(), BTreeMap::new());
    fm.set_drives(vec![drive("sdb1", None, None, true, false), drive("sdc1", None, None, true, false)]);
    fm.toggle_pane_focus();
    for _ in 0..20 {
        fm.move_cursor(0, 1);
    }
    fm.set_drives(vec![]);
    assert_eq!(fm.sidebar_w(), SIDEBAR_W);
    fm.activate(); // the cursor is clamped onto the last place: no panic
    let _ = fs::remove_dir_all(&d);
}
