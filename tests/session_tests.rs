use tetron_wm::chrome::DockKind;
use tetron_wm::session::{SessionCore, ClientMsg};
use tetron_wm::config::{AppEntry, Config};
use tetron_wm::geometry::Point;

#[test]
fn launching_app_creates_window_and_dock_entry() {
    let mut core = SessionCore::new(80, 24, Config::default());
    core.apply(ClientMsg::Launch { name: "shell".into(), command: "sh".into(), args: vec!["-c".into(), "sleep 2".into()] });
    let frame = core.build_frame();
    // a window layer + menubar + dock present
    assert!(frame.layers.len() >= 3);
    assert_eq!(core.window_count(), 1);
    core.shutdown();
}

#[test]
fn click_dock_focuses_window() {
    let mut core = SessionCore::new(80, 24, Config::default());
    core.apply(ClientMsg::Launch { name: "a".into(), command: "sh".into(), args: vec!["-c".into(),"sleep 2".into()] });
    core.apply(ClientMsg::Launch { name: "b".into(), command: "sh".into(), args: vec!["-c".into(),"sleep 2".into()] });
    // dock_regions returns (pill_index, Rect); apps "a" and "b" each get their own pill
    let items = core.dock_items_for_test();
    let regions = core.dock_regions();
    let (idx, r) = regions[0];
    // Extract the WindowId from the first pill (must be a Single since names differ)
    let first_id = match &items[idx].kind {
        DockKind::Single(id) => *id,
        DockKind::Group(_, ids) => ids[0],
    };
    core.apply(ClientMsg::MouseDown(Point::new(r.x, r.y)));
    assert_eq!(core.focused(), Some(first_id));
    core.shutdown();
}

#[test]
fn far_right_menubar_click_does_not_quit() {
    let mut core = SessionCore::new(80, 24, Config::default());
    assert!(!core.quit_requested());
    // The power button is gone (Exit/Restart/Shutdown moved into the launcher's
    // system section). A far-right menubar click now lands on the tray and must
    // never quit the session.
    core.apply(ClientMsg::MouseDown(Point::new(79, 0)));
    assert!(!core.quit_requested(), "a menubar click must not quit");
    core.shutdown();
}

#[test]
fn spotlight_launches_selected_app() {
    let mut core = SessionCore::new(80, 24, Config::default());
    assert!(!core.spotlight_open());
    core.apply(ClientMsg::ToggleSpotlight);
    assert!(core.spotlight_open());
    let before = core.window_count();
    core.apply(ClientMsg::LauncherEnter); // launch highlighted (the default shell)
    assert!(!core.launcher_open());
    assert_eq!(core.window_count(), before + 1);
    core.shutdown();
}

#[test]
fn open_store_creates_focused_store_window() {
    let mut core = SessionCore::new(120, 40, Config::default());
    assert!(!core.focused_is_store());
    core.apply(ClientMsg::OpenStore);
    assert!(core.focused_is_store());
    let n = core.window_count();
    // opening again focuses the same window, doesn't make a second
    core.apply(ClientMsg::OpenStore);
    assert_eq!(core.window_count(), n);
    core.apply(ClientMsg::StoreClose);
    assert!(!core.focused_is_store());
    core.shutdown();
}

#[test]
fn open_settings_creates_focused_settings_window() {
    let mut core = SessionCore::new(120, 40, Config::default());
    assert!(!core.focused_is_settings());
    core.apply(ClientMsg::OpenSettings);
    assert!(core.focused_is_settings());
    core.apply(ClientMsg::SettingsClose);
    assert!(!core.focused_is_settings());
    core.shutdown();
}

#[test]
fn image_window_draws_cells_then_a_fitted_kitty_placement() {
    // A real 40x20 PNG in a temp folder.
    let dir = std::env::temp_dir().join(format!("tetron-wm-img-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("x.png");
    let img = image::RgbaImage::from_pixel(40, 20, image::Rgba([10, 200, 30, 255]));
    image::DynamicImage::ImageRgba8(img).save(&path).unwrap();

    // Disable the desktop so the only image placement is the viewer's (desktop
    // icons emit their own image placements).
    let mut core = SessionCore::new(80, 24, Config { apps: vec![], desktop_enabled: false, ..Config::default() });
    core.apply(ClientMsg::OpenImage(path.to_string_lossy().to_string()));
    assert!(core.focused_is_image());

    // No Kitty graphics: the image arrives as cells, with no placement.
    let green = |c: &tetron_wm::cell::Cell| c.bg.g > 150 && c.bg.r < 60;
    let mut drawn = false;
    for _ in 0..500 {
        core.pump_images();
        let frame = core.build_frame();
        assert!(frame.images.is_empty(), "no placement without graphics support");
        if frame.layers.iter().any(|l| (0..l.buf.height()).any(|y| (0..l.buf.width()).any(|x| green(l.buf.get(x, y).unwrap())))) {
            drawn = true;
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    assert!(drawn, "image drawn as cells");

    // A Kitty-capable client: a visible placement, aspect-fitted (not the whole
    // window) with the source crop.
    let before = tetron_wm::display::get();
    core.apply(ClientMsg::Caps(tetron_wm::display::ClientCaps { kitty_graphics: true, ..before }));
    core.pump_images();
    let frame = core.build_frame();
    tetron_wm::display::set(before);
    assert_eq!(frame.images.len(), 1, "one image placement");
    let p = &frame.images[0];
    assert!(p.visible, "unobstructed: visible");
    assert_eq!(p.src, Some([0, 0, 40, 20]));
    // A 2:1 image, in cells twice as tall as wide, spans 4 columns per row.
    assert_eq!(p.cols as i32, 4 * p.rows as i32, "aspect kept: {p:?}");

    // Zoomed in, a press on the image and a drag pan the shown crop.
    core.apply(ClientMsg::Caps(tetron_wm::display::ClientCaps { kitty_graphics: true, ..before }));
    for _ in 0..3 {
        core.apply(ClientMsg::ImageKey(tetron_wm::imageview::ImageKey::ZoomIn));
    }
    core.pump_images();
    let src_x = |core: &mut SessionCore| {
        core.pump_images();
        core.build_frame().images[0].src.unwrap()[0]
    };
    let x0 = src_x(&mut core);
    let r = core.build_frame().images[0].rect;
    let mid = tetron_wm::geometry::Point::new(r.x + r.w / 2, r.y + r.h / 2);
    core.apply(ClientMsg::MouseDown(mid));
    core.apply(ClientMsg::MouseDrag(tetron_wm::geometry::Point::new(mid.x - 4, mid.y)));
    core.apply(ClientMsg::MouseUp(tetron_wm::geometry::Point::new(mid.x - 4, mid.y)));
    let x1 = src_x(&mut core);
    tetron_wm::display::set(before);
    assert!(x1 > x0, "dragging left moves the crop right: {x0} -> {x1}");
    assert!(core.focused_is_image(), "the press did not close or move the window");

    // Full screen is only handed over from a plain console: elsewhere `f` does nothing.
    core.apply(ClientMsg::ImageKey(tetron_wm::imageview::ImageKey::Fullscreen));
    assert!(!core.quit_requested() && core.fullscreen_spec().is_none(), "not on a plain console: no handoff");

    // Esc closes the viewer.
    core.apply(ClientMsg::ImageKey(tetron_wm::imageview::ImageKey::Close));
    assert!(!core.focused_is_image());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn file_manager_emits_thumbnail_placement_for_image() {
    let dir = std::env::temp_dir().join(format!("tetron-wm-fmthumb-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let img = image::RgbaImage::from_pixel(8, 8, image::Rgba([1, 2, 3, 255]));
    image::DynamicImage::ImageRgba8(img).save(dir.join("p.png")).unwrap();

    let mut core = SessionCore::new(100, 30, Config::default());
    core.apply(ClientMsg::OpenFileManager);
    // navigate the FM into our temp dir by faking it: open at temp via a second open
    core.open_filemanager_at(dir.clone()); // test helper below
    // Thumbnails load on a background thread now; pump until it arrives.
    let frame = pump_until_image(&mut core);
    assert!(frame.images.iter().any(|p| p.cols >= 1), "expected a thumbnail placement");
    core.shutdown();
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn file_manager_emits_role_icon_placement_for_non_image_entry() {
    // A folder with only non-image entries (a text file and a subfolder) —
    // these have no real thumbnail, so the Icon view should place the shared
    // per-role tile (same image id for every entry of that role) instead of
    // falling back to a bare glyph.
    let dir = std::env::temp_dir().join(format!("tetron-wm-fmrole-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("note.txt"), b"hello").unwrap();
    std::fs::create_dir(dir.join("sub")).unwrap();

    let mut core = SessionCore::new(100, 30, Config::default());
    core.apply(ClientMsg::OpenFileManager);
    core.open_filemanager_at(dir.clone());
    // Role icons are pre-generated (no background loader involved), so a
    // single build_frame right after opening should already carry them.
    let frame = core.build_frame();
    assert!(
        frame.images.iter().any(|p| p.cols >= 1),
        "expected a role-icon placement for the non-image entries"
    );
    core.shutdown();
    let _ = std::fs::remove_dir_all(&dir);
}

/// Drive the background thumbnail loader until `build_frame` emits an image
/// placement (or a timeout), returning that frame.
fn pump_until_image(core: &mut SessionCore) -> tetron_wm::session::Frame {
    for _ in 0..200 {
        core.pump_thumbnails();
        let frame = core.build_frame();
        if frame.images.iter().any(|p| p.cols >= 1) {
            return frame;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    core.build_frame()
}

#[test]
fn desktop_click_selects_and_double_click_opens_files() {
    let dir = std::env::temp_dir().join(format!("tetron-wm-deskwire-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::create_dir(dir.join("proj")).unwrap();

    // Point the desktop at our temp dir via a test helper.
    let mut core = SessionCore::new(100, 30, Config { desktop_pins: vec![], ..Config::default() });
    core.set_desktop_dir_for_test(dir.clone()); // reloads desktop at `dir`
    // "proj" is the only icon (idx 0); derive a point inside its tile (the layout
    // is right-aligned, so don't assume top-left).
    let tr = core.desktop_icon_tile_for_test(0);
    let p = tetron_wm::geometry::Point::new(tr.x + 1, tr.y + 1);
    core.apply(ClientMsg::MouseDown(p));
    assert_eq!(core.desktop_selection_len_for_test(), 1);
    let before = core.window_count();
    core.apply(ClientMsg::MouseDouble(p));
    assert_eq!(core.window_count(), before + 1); // a Files window opened on the folder
    core.shutdown();
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn desktop_new_folder_via_menu_creates_dir() {
    let dir = std::env::temp_dir().join(format!("tetron-wm-deskmk-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let mut core = SessionCore::new(100, 30, Config { desktop_pins: vec![], ..Config::default() });
    core.set_desktop_dir_for_test(dir.clone());
    core.apply(ClientMsg::MouseRightDown(tetron_wm::geometry::Point::new(60, 20))); // empty desktop menu
    // Drive new-folder directly via the editing messages (menu click tested in unit tests):
    core.begin_desktop_new_folder_for_test();
    for c in "Stuff".chars() {
        core.apply(ClientMsg::DesktopChar(c));
    }
    core.apply(ClientMsg::DesktopCommit);
    assert!(dir.join("Stuff").is_dir());
    core.shutdown();
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn right_click_fm_entry_opens_context_menu() {
    let dir = std::env::temp_dir().join(format!("tetron-wm-fmctx-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("entry.txt"), b"hi").unwrap();

    let mut core = SessionCore::new(120, 40, Config::default());
    core.apply(ClientMsg::OpenFileManager);
    core.open_filemanager_at(dir.clone());
    assert!(!core.focused_fm_context_open_for_test());
    // The FM window opens at rect (15, 2, 90, 30) for a 120x40 screen, so its
    // content rect is (16, 3, 88, 28). Icon view's first tile sits at local
    // (16, 2)..(30, 8); (18, 2) lands inside it (row 0, col 0 → entry 0).
    let p = Point::new(16 + 18, 3 + 2);
    core.apply(ClientMsg::MouseRightDown(p));
    assert!(core.focused_fm_context_open_for_test(), "right-click on the entry opens the context menu");
    core.shutdown();
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn right_click_fm_toolbar_does_not_open_context_menu() {
    let dir = std::env::temp_dir().join(format!("tetron-wm-fmctx2-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("entry.txt"), b"hi").unwrap();

    let mut core = SessionCore::new(120, 40, Config::default());
    core.apply(ClientMsg::OpenFileManager);
    core.open_filemanager_at(dir.clone());
    // The toolbar row (local y = 0) is a non-entry hit — v1 does nothing.
    let p = Point::new(16 + 18, 3);
    core.apply(ClientMsg::MouseRightDown(p));
    assert!(!core.focused_fm_context_open_for_test());
    core.shutdown();
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn open_file_manager_creates_focused_window_and_is_single_instance() {
    let mut core = SessionCore::new(120, 40, Config::default());
    assert!(!core.focused_is_filemanager());
    core.apply(ClientMsg::OpenFileManager);
    assert!(core.focused_is_filemanager());
    let n = core.window_count();
    core.apply(ClientMsg::OpenFileManager);
    assert_eq!(core.window_count(), n); // re-focus, not a second window
    core.apply(ClientMsg::FileManagerClose);
    assert!(!core.focused_is_filemanager());
    core.shutdown();
}

#[test]
fn desktop_image_icon_emits_thumbnail_placement() {
    let dir = std::env::temp_dir().join(format!("tetron-wm-deskthumb-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let img = image::RgbaImage::from_pixel(8, 8, image::Rgba([9, 9, 9, 255]));
    image::DynamicImage::ImageRgba8(img).save(dir.join("p.png")).unwrap();

    let mut core = SessionCore::new(100, 30, Config { desktop_pins: vec![], ..Config::default() });
    core.set_desktop_dir_for_test(dir.clone());
    // Thumbnails load on a background thread now; pump until it arrives.
    let frame = pump_until_image(&mut core);
    assert!(frame.images.iter().any(|pl| pl.cols >= 1), "expected a desktop thumbnail placement");
    core.shutdown();
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn app_graphics_placement_is_emitted_in_frame() {
    // Drive an AppInstance's GraphicsState directly, then assert build_frame emits it.
    let mut core = SessionCore::new(100, 30, Config::default());
    core.apply(ClientMsg::Launch { name: "sh".into(), command: "sh".into(), args: vec!["-c".into(), "sleep 5".into()] });
    // Inject a placement+image into the launched app's graphics state via a test helper.
    let png = {
        let i = image::RgbaImage::from_pixel(2, 2, image::Rgba([1, 2, 3, 255]));
        let mut b = std::io::Cursor::new(Vec::new());
        image::DynamicImage::ImageRgba8(i).write_to(&mut b, image::ImageFormat::Png).unwrap();
        b.into_inner()
    };
    core.inject_app_graphics_for_test(&png);
    let frame = core.build_frame();
    assert!(frame.images.iter().any(|p| p.cols >= 1), "expected an app graphics placement");
    core.shutdown();
}

#[test]
fn restore_rebuilds_app_window_from_meta() {
    // Launch an app, push its meta, then simulate a fresh frontend over the SAME
    // in-process host by constructing a new SessionCore around a host that already
    // owns the app. We approximate this by reusing the same core: drop its window
    // mapping, then restore.
    let mut core = SessionCore::new(100, 30, Config::default());
    core.apply(ClientMsg::Launch { name: "shell".into(), command: "sh".into(), args: vec!["-c".into(), "sleep 5".into()] });
    core.sync_app_meta();
    let before = core.window_count();
    assert_eq!(before, 1);
    // Forget the window (as a fresh frontend would) but keep the app in the host.
    core.forget_windows_for_test();
    assert_eq!(core.window_count(), 0);
    let restored = core.restore_windows_from_host();
    assert_eq!(restored, 1);
    assert_eq!(core.window_count(), 1);
    core.shutdown();
}

#[test]
fn sync_app_meta_records_window_state() {
    let mut core = SessionCore::new(100, 30, Config::default());
    core.apply(ClientMsg::Launch { name: "shell".into(), command: "sh".into(), args: vec!["-c".into(), "sleep 5".into()] });
    core.sync_app_meta();
    // The app's meta is now populated (non-empty) for restore.
    assert!(core.app_meta_count_for_test() > 0);
    core.shutdown();
}

#[test]
fn cascade_keyboard_launches_app_from_submenu() {
    let mut core = SessionCore::new(120, 40, Config::default());
    core.apply(ClientMsg::ToggleMenu);
    assert!(core.launcher_open_for_test());
    let before = core.window_count();
    core.apply(ClientMsg::LauncherRight); // descend into the first category
    core.apply(ClientMsg::LauncherEnter); // launch the first app in it
    assert!(!core.launcher_open_for_test()); // menu closed after launch
    assert_eq!(core.window_count(), before + 1);
    core.shutdown();
}

#[test]
fn mode_toggle_switches_view() {
    use tetron_wm::chrome::menubar_mode_region;
    let mut core = SessionCore::new(120, 40, Config::default());
    assert!(!core.simple_mode());
    let r = menubar_mode_region();
    core.apply(ClientMsg::MouseDown(Point::new(r.x, 0)));
    assert!(core.simple_mode(), "clicking the toggle enters simple mode");
    core.apply(ClientMsg::MouseDown(Point::new(r.x, 0)));
    assert!(!core.simple_mode(), "clicking again returns to desktop");
    core.shutdown();
}

#[test]
fn simple_mode_renders_focused_app_fullscreen_without_desktop() {
    use tetron_wm::chrome::menubar_mode_region;
    let mut core = SessionCore::new(120, 40, Config::default());
    core.apply(ClientMsg::Launch { name: "shell".into(), command: "sh".into(), args: vec!["-c".into(), "sleep 5".into()] });
    core.apply(ClientMsg::MouseDown(Point::new(menubar_mode_region().x, 0)));
    assert!(core.simple_mode());
    let frame = core.build_frame();
    // The focused app's content layer originates at the work area top (row 1),
    // i.e. there is a content layer at y=1 spanning the width.
    assert!(frame.layers.iter().any(|l| l.origin.y == 1 && l.buf.width() == 120),
        "focused app should fill the work-area width at row 1");
    core.shutdown();
}

#[test]
fn app_mouse_area_none_without_mouse_mode() {
    let mut core = SessionCore::new(120, 40, Config::default());
    core.apply(ClientMsg::Launch { name: "shell".into(), command: "sh".into(), args: vec!["-c".into(), "sleep 5".into()] });
    // A bare shell hasn't enabled mouse reporting → no app area, passthrough off.
    assert!(core.app_mouse_area().is_none());
    core.shutdown();
}

#[test]
fn tray_popover_click_launches_over_a_covering_window() {
    // Regression: an open tray popover must own the click even when an app
    // window sits under it. A maximized shell covers the whole work area, so
    // the popover's "Open terminal" row is over app content. Before the fix,
    // mouse_select (which runs first on MouseDown) did not count the tray as an
    // overlay, so it swallowed the click as a text-selection on the window
    // beneath and the launch never fired.
    use tetron_wm::tray::SegmentKind;
    let cfg = Config { launch_maximized: true, ..Config::default() };
    let mut core = SessionCore::new(120, 40, cfg);
    core.apply(ClientMsg::Launch { name: "shell".into(), command: "sh".into(), args: vec!["-c".into(), "sleep 5".into()] });
    assert_eq!(core.window_count(), 1);
    let rect = core
        .tray_openterm_rect_for_test(SegmentKind::Tetron)
        .expect("tetron popover has an Open terminal row");
    core.apply(ClientMsg::MouseDown(Point::new(rect.x, rect.y)));
    assert_eq!(core.window_count(), 2, "Open terminal launched despite the window under the popover");
    core.shutdown();
}

#[test]
fn app_mouse_area_suppressed_while_launcher_open() {
    // An open overlay (the Go launcher) must take the mouse — even over a focused
    // app — so clicking an app in the menu is never swallowed by passthrough.
    let mut core = SessionCore::new(120, 40, Config::default());
    core.apply(ClientMsg::Launch { name: "shell".into(), command: "sh".into(), args: vec!["-c".into(), "sleep 5".into()] });
    core.apply(ClientMsg::ToggleMenu); // open the Go launcher
    assert!(core.launcher_open());
    assert!(core.app_mouse_area().is_none(), "no app passthrough area while the launcher is open");
    core.shutdown();
}

#[test]
fn two_same_app_windows_group_in_dock() {
    let mut core = SessionCore::new(120, 40, Config::default());
    let launch = |c: &mut SessionCore| c.apply(ClientMsg::Launch { name: "Claude".into(), command: "sh".into(), args: vec!["-c".into(), "sleep 5".into()] });
    launch(&mut core);
    assert_eq!(core.dock_pill_count_for_test(), 1); // one window → one pill
    launch(&mut core);
    assert_eq!(core.dock_pill_count_for_test(), 1); // two Claude → still ONE grouped pill
    assert_eq!(core.window_count(), 2);
    core.shutdown();
}

#[test]
fn rename_changes_label_not_grouping() {
    let mut core = SessionCore::new(120, 40, Config::default());
    core.apply(ClientMsg::Launch { name: "Claude".into(), command: "sh".into(), args: vec!["-c".into(), "sleep 5".into()] });
    core.apply(ClientMsg::RenameFocused);
    for c in "appname".chars() { core.apply(ClientMsg::RenameChar(c)); }
    core.apply(ClientMsg::RenameCommit);
    assert_eq!(core.focused_label_for_test(), "appname");
    // a second Claude still groups with the renamed one (same app_key)
    core.apply(ClientMsg::Launch { name: "Claude".into(), command: "sh".into(), args: vec!["-c".into(), "sleep 5".into()] });
    assert_eq!(core.dock_pill_count_for_test(), 1);
    core.shutdown();
}

#[test]
fn clicking_titlebar_does_not_move_tiled_window() {
    // Regression: a plain click on a tiled window's titlebar (e.g. to rename it)
    // must NOT untile/move the window — only a real drag should.
    let mut core = SessionCore::new(120, 40, Config::default());
    core.apply(ClientMsg::Launch { name: "a".into(), command: "sh".into(), args: vec!["-c".into(), "sleep 5".into()] });
    core.apply(ClientMsg::SendToCell(1)); // tile into a grid cell
    let before = core.focused_window_rect_for_test().unwrap();
    // Click (down then up at the SAME titlebar point — no drag motion).
    let p = Point::new(before.x + 2, before.y);
    core.apply(ClientMsg::MouseDown(p));
    core.apply(ClientMsg::MouseUp(p));
    let after = core.focused_window_rect_for_test().unwrap();
    assert_eq!(before, after, "a plain titlebar click must not move/untile a tiled window");
    core.shutdown();
}

#[test]
fn double_click_titlebar_full_sequence_keeps_tiled_window_put() {
    // Faithful repro of a real double-click on a tiled window's titlebar with
    // all-motion mouse: Down, Drag(same p), Up, Double, Up.
    let mut core = SessionCore::new(120, 40, Config::default());
    core.apply(ClientMsg::Launch { name: "a".into(), command: "sh".into(), args: vec!["-c".into(), "sleep 5".into()] });
    core.apply(ClientMsg::SendToCell(1));
    let before = core.focused_window_rect_for_test().unwrap();
    let p = Point::new(before.x + 2, before.y);
    core.apply(ClientMsg::MouseDown(p));
    core.apply(ClientMsg::MouseDrag(p));   // all-motion Moved at same cell
    core.apply(ClientMsg::MouseUp(p));
    core.apply(ClientMsg::MouseDouble(p)); // second click → rename
    core.apply(ClientMsg::MouseUp(p));
    let after = core.focused_window_rect_for_test().unwrap();
    assert_eq!(before, after, "double-click titlebar must not move the tiled window (before={before:?} after={after:?})");
    core.shutdown();
}

#[test]
fn spurious_teleport_drag_does_not_fling_window() {
    // Repro of the real bug: while dragging, a single stray mouse report jumped
    // >half the screen (a click at the top yielded a drag to the bottom),
    // flinging the window off-screen. Such teleports must be ignored.
    let mut core = SessionCore::new(120, 40, Config::default());
    core.apply(ClientMsg::Launch { name: "a".into(), command: "sh".into(), args: vec!["-c".into(), "sleep 5".into()] });
    core.apply(ClientMsg::SendToCell(1));
    let before = core.focused_window_rect_for_test().unwrap();
    let p = Point::new(before.x + 2, before.y);
    core.apply(ClientMsg::MouseDown(p));
    // Stray report teleporting to the BOTTOM edge row (the real garbage signature:
    // a same-ms row-1 → last-row "drag" that flings the window off-screen). A big
    // jump that lands on a screen edge is dropped.
    core.apply(ClientMsg::MouseDrag(Point::new(p.x, 39)));
    core.apply(ClientMsg::MouseUp(Point::new(p.x, 39)));
    let after = core.focused_window_rect_for_test().unwrap();
    assert_eq!(before, after, "a spurious teleport drag must not move the window (before={before:?} after={after:?})");
    core.shutdown();
}

#[test]
fn fast_vertical_drag_moves_the_window() {
    // A fast legitimate drag reports a large single vertical step (terminals
    // coalesce motion). As long as it lands mid-screen — not on an edge row — it
    // must move the window, not be dropped as a spurious teleport. Guards against
    // the over-broad magnitude-only filter that froze fast drags / biased them to
    // one direction.
    let mut core = SessionCore::new(120, 40, Config::default());
    core.apply(ClientMsg::Launch { name: "a".into(), command: "sh".into(), args: vec!["-c".into(), "sleep 5".into()] });
    core.apply(ClientMsg::SendToCell(1));
    let before = core.focused_window_rect_for_test().unwrap();
    let p = Point::new(before.x + 2, before.y);
    core.apply(ClientMsg::MouseDown(p));
    // Jump of 24 rows (> h/2 = 20) to mid-screen row 25 — a fast, legitimate drag.
    core.apply(ClientMsg::MouseDrag(Point::new(p.x, 25)));
    core.apply(ClientMsg::MouseUp(Point::new(p.x, 25)));
    let after = core.focused_window_rect_for_test().unwrap();
    assert!(after.y > before.y, "a fast mid-screen drag must move the window down (before={before:?} after={after:?})");
    core.shutdown();
}

#[test]
fn bare_move_during_drag_ends_it() {
    // A no-button move while a window drag is active means the release was lost
    // (a real drag only arrives as Drag events). The drag must end, not keep the
    // window stuck to the pointer.
    let mut core = SessionCore::new(120, 40, Config::default());
    core.apply(ClientMsg::Launch { name: "a".into(), command: "sh".into(), args: vec!["-c".into(), "sleep 5".into()] });
    core.apply(ClientMsg::SendToCell(1));
    let before = core.focused_window_rect_for_test().unwrap();
    let p = Point::new(before.x + 2, before.y);
    core.apply(ClientMsg::MouseDown(p));
    core.apply(ClientMsg::MouseDrag(Point::new(p.x, 10))); // real drag down
    core.apply(ClientMsg::MouseMove(Point::new(p.x, 10))); // button released, Up lost
    let moved = core.focused_window_rect_for_test().unwrap();
    // A further bare move must NOT drag the window any more.
    core.apply(ClientMsg::MouseMove(Point::new(p.x, 20)));
    let after = core.focused_window_rect_for_test().unwrap();
    assert_eq!(moved, after, "window must not keep following the pointer after the drag ended (moved={moved:?} after={after:?})");
    core.shutdown();
}

#[test]
fn taskbar_plus_button_opens_a_shell_window() {
    use tetron_wm::chrome::menubar_new_shell_region;
    let mut core = SessionCore::new(120, 40, Config::default());
    let before = core.window_count();
    let r = menubar_new_shell_region(); // the "+" in the top menubar row
    assert_eq!(r.y, 0);
    core.apply(ClientMsg::MouseDown(Point::new(r.x, r.y)));
    assert_eq!(core.window_count(), before + 1, "clicking + should open a new shell window");
    core.shutdown();
}

#[test]
fn closing_launcher_by_clicking_brand_does_not_launch_shell() {
    // Open the launcher (click the brand), then click the brand again to close it.
    // Closing must NOT activate the auto-selected first row (now "Shell").
    use tetron_wm::chrome::menubar_brand_region;
    let mut core = SessionCore::new(120, 40, Config::default());
    let before = core.window_count();
    let p = Point::new(menubar_brand_region().x + 1, 0);
    core.apply(ClientMsg::MouseDown(p)); // open
    assert!(core.launcher_open());
    core.apply(ClientMsg::MouseDown(p)); // click brand again → close
    assert!(!core.launcher_open(), "clicking the brand again should close the launcher");
    assert_eq!(core.window_count(), before, "closing the launcher must not open a shell");
    core.shutdown();
}

#[test]
fn closing_app_window_requires_confirmation() {
    // Clicking the titlebar ✕ on an app window opens a modal confirm dialog and
    // does NOT kill the app until the user confirms (it kills the process).
    let mut core = SessionCore::new(
        80,
        24,
        Config { apps: vec![], desktop_enabled: false, ..Config::default() },
    );
    core.apply(ClientMsg::Launch {
        name: "shell".into(),
        command: "sh".into(),
        args: vec!["-c".into(), "sleep 5".into()],
    });
    assert_eq!(core.window_count(), 1);
    let r = core.focused_window_rect_for_test().unwrap();
    let close = Point::new(r.x + r.w - 3, r.y); // close glyph column

    // First click: dialog opens, window stays.
    core.apply(ClientMsg::MouseDown(close));
    assert!(core.confirm_close_open(), "closing an app window opens the confirm dialog");
    assert_eq!(core.window_count(), 1, "window must not close until confirmed");

    // Cancel keeps the window.
    core.apply(ClientMsg::ConfirmCloseNo);
    assert!(!core.confirm_close_open());
    assert_eq!(core.window_count(), 1);

    // Click again, then confirm: the window closes.
    core.apply(ClientMsg::MouseDown(close));
    assert!(core.confirm_close_open());
    core.apply(ClientMsg::ConfirmCloseYes);
    assert!(!core.confirm_close_open());
    assert_eq!(core.window_count(), 0, "confirming closes the app window");
    core.shutdown();
}

#[test]
fn closing_filemanager_does_not_confirm() {
    // Built-in panels (File Manager / Store / Settings) have no process to lose,
    // so the titlebar ✕ closes them immediately with no confirm dialog.
    let mut core = SessionCore::new(
        100,
        30,
        Config { apps: vec![], desktop_enabled: false, ..Config::default() },
    );
    core.apply(ClientMsg::OpenFileManager);
    assert_eq!(core.window_count(), 1);
    let r = core.focused_window_rect_for_test().unwrap();
    core.apply(ClientMsg::MouseDown(Point::new(r.x + r.w - 3, r.y)));
    assert!(!core.confirm_close_open(), "built-in panels close without a prompt");
    assert_eq!(core.window_count(), 0, "file manager closes immediately");
    core.shutdown();
}

#[test]
fn dock_right_click_ignored_while_another_overlay_is_open() {
    use tetron_wm::session::dock_ctx_row_rect;
    // With the launcher open, right-clicking a dock pill must NOT open the dock
    // context menu underneath it: its click-capture runs before the launcher's,
    // so a hidden menu would silently eat the next click.
    let mut core = SessionCore::new(120, 40, Config::default());
    core.apply(ClientMsg::Launch { name: "shell".into(), command: "sh".into(), args: vec!["-c".into(), "sleep 5".into()] });
    let before = core.focused_window_rect_for_test().unwrap();
    let items = core.dock_items_for_test();
    let (_, pill) = tetron_wm::chrome::menubar_taskbar_regions(120, &items, &[])[0];
    core.apply(ClientMsg::ToggleMenu); // open the launcher (a blocking overlay)
    assert!(core.launcher_open());
    core.apply(ClientMsg::MouseRightDown(Point::new(pill.x, 0)));
    // The menu never opened, so clicking its Reset row does nothing — the click
    // routes to the launcher instead, and the window is untouched.
    let row = dock_ctx_row_rect(pill.x, 120, 40, 3);
    core.apply(ClientMsg::MouseDown(Point::new(row.x + 1, row.y)));
    assert_eq!(core.focused_window_rect_for_test().unwrap(), before);
    core.shutdown();
}

#[test]
fn dock_ctx_menu_drops_down_from_taskbar() {
    use tetron_wm::session::dock_ctx_rect;
    // The pill context menu drops down from the menubar/taskbar row, so its top
    // sits at row 1 (just below the bar) regardless of terminal height — render
    // and hit-test share this fn, so they can never drift.
    let d = dock_ctx_rect(0, 80, 40);
    assert_eq!(d.y, 1, "menu drops down from the taskbar row: {d:?}");
    assert!(d.x >= 0 && d.x + d.w <= 80, "fits horizontally: {d:?}");
}

#[test]
fn dock_right_click_reset_centres_at_half_size() {
    use tetron_wm::session::dock_ctx_row_rect;
    let mut core = SessionCore::new(120, 40, Config::default());
    core.apply(ClientMsg::Launch { name: "shell".into(), command: "sh".into(), args: vec!["-c".into(), "sleep 5".into()] });
    let items = core.dock_items_for_test();
    let (_, pill) = tetron_wm::chrome::menubar_taskbar_regions(120, &items, &[])[0];
    core.apply(ClientMsg::MouseRightDown(Point::new(pill.x, 0)));
    // Click row 3 (Reset size) using the same geometry the session renders with.
    let row = dock_ctx_row_rect(pill.x, 120, 40, 3);
    core.apply(ClientMsg::MouseDown(Point::new(row.x + 1, row.y)));
    let r = core.focused_window_rect_for_test().unwrap();
    assert_eq!((r.w, r.h), (60, 19), "half of the 120x39 work area");
    assert_eq!((r.x, r.y), ((120 - 60) / 2, 1 + (39 - 19) / 2), "centred: {r:?}");
    core.shutdown();
}

#[test]
fn dock_right_click_elsewhere_dismisses_without_acting() {
    use tetron_wm::session::dock_ctx_row_rect;
    let mut core = SessionCore::new(120, 40, Config::default());
    core.apply(ClientMsg::Launch { name: "shell".into(), command: "sh".into(), args: vec!["-c".into(), "sleep 5".into()] });
    let before = core.focused_window_rect_for_test().unwrap();
    let items = core.dock_items_for_test();
    let (_, pill) = tetron_wm::chrome::menubar_taskbar_regions(120, &items, &[])[0];
    core.apply(ClientMsg::MouseRightDown(Point::new(pill.x, 0)));
    // A click far from the menu dismisses it and must not move/close anything.
    core.apply(ClientMsg::MouseDown(Point::new(2, 5)));
    assert_eq!(core.focused_window_rect_for_test().unwrap(), before);
    assert_eq!(core.window_count(), 1);
    // And the menu really is gone: clicking a row rect now does nothing either.
    let row = dock_ctx_row_rect(pill.x, 120, 40, 3);
    core.apply(ClientMsg::MouseDown(Point::new(row.x + 1, row.y)));
    assert_eq!(core.focused_window_rect_for_test().unwrap(), before);
    core.shutdown();
}

/// Launcher entries flagged `cli` (prints-and-exits tools like himalaya, gum,
/// khard) spawn through the `sh -lc '<bin> --help; exec "${SHELL:-sh}"'`
/// wrapper instead of the bare binary, so the user sees usage then lands in a
/// normal shell with the tool on `$PATH`.
#[test]
fn cli_flagged_launcher_app_wraps_in_shell() {
    let mut cfg = Config::default();
    cfg.launcher.push(AppEntry {
        name: "himalaya".into(),
        command: "himalaya".into(),
        args: vec![],
        category: None,
        requires_cwd: None,
        cwd: None,
        cli: Some(true),
        warn: None,
    });
    let mut core = SessionCore::new(80, 24, cfg);
    core.apply(ClientMsg::ToggleSpotlight);
    for c in "himalaya".chars() {
        core.apply(ClientMsg::LauncherChar(c));
    }
    core.apply(ClientMsg::LauncherEnter);
    let (cmd, args) = core.focused_app_launch_cmd_for_test().expect("app launched");
    assert_eq!(cmd, "sh");
    assert_eq!(args.first().map(String::as_str), Some("-lc"));
    let script = args.get(1).cloned().unwrap_or_default();
    assert!(script.contains("'himalaya' --help"), "script: {script}");
    assert!(script.contains("exec \"${SHELL:-sh}\""), "script: {script}");
    core.shutdown();
}

/// A launcher entry with no `cli` flag (the common TUI case) launches the bare
/// binary unchanged — no shell wrapper.
#[test]
fn non_cli_launcher_app_launches_bare_binary() {
    let mut cfg = Config::default();
    cfg.launcher.push(AppEntry {
        name: "true".into(),
        command: "true".into(),
        args: vec![],
        category: None,
        requires_cwd: None,
        cwd: None,
        cli: Some(false),
        warn: None,
    });
    let mut core = SessionCore::new(80, 24, cfg);
    core.apply(ClientMsg::ToggleSpotlight);
    for c in "true".chars() {
        core.apply(ClientMsg::LauncherChar(c));
    }
    core.apply(ClientMsg::LauncherEnter);
    let (cmd, args) = core.focused_app_launch_cmd_for_test().expect("app launched");
    assert_eq!(cmd, "true");
    assert!(args.is_empty());
    core.shutdown();
}

/// A launcher entry flagged `warn` opens the launch-warning dialog instead of
/// launching immediately; confirming (`LaunchWarnYes`) then runs the exact
/// same launch.
#[test]
fn warn_flagged_launcher_app_opens_dialog_then_launches_on_confirm() {
    let mut cfg = Config::default();
    cfg.launcher.push(AppEntry {
        name: "true".into(),
        command: "true".into(),
        args: vec![],
        category: None,
        requires_cwd: None,
        cwd: None,
        cli: None,
        warn: Some("careful?".into()),
    });
    let mut core = SessionCore::new(80, 24, cfg);
    assert!(!core.launch_warn_open());
    core.apply(ClientMsg::ToggleSpotlight);
    for c in "true".chars() {
        core.apply(ClientMsg::LauncherChar(c));
    }
    core.apply(ClientMsg::LauncherEnter);
    assert!(core.launch_warn_open(), "a warn-flagged entry opens the dialog first");
    assert!(core.focused_app_launch_cmd_for_test().is_none(), "not launched yet");

    core.apply(ClientMsg::LaunchWarnYes);
    assert!(!core.launch_warn_open());
    let (cmd, args) = core.focused_app_launch_cmd_for_test().expect("confirming launches the pending entry");
    assert_eq!(cmd, "true");
    assert!(args.is_empty());
    core.shutdown();
}

/// Cancelling the launch-warning dialog (`LaunchWarnNo`) never launches.
#[test]
fn warn_flagged_launcher_app_cancel_does_not_launch() {
    let mut cfg = Config::default();
    cfg.launcher.push(AppEntry {
        name: "true".into(),
        command: "true".into(),
        args: vec![],
        category: None,
        requires_cwd: None,
        cwd: None,
        cli: None,
        warn: Some("careful?".into()),
    });
    let mut core = SessionCore::new(80, 24, cfg);
    core.apply(ClientMsg::ToggleSpotlight);
    for c in "true".chars() {
        core.apply(ClientMsg::LauncherChar(c));
    }
    core.apply(ClientMsg::LauncherEnter);
    assert!(core.launch_warn_open());
    core.apply(ClientMsg::LaunchWarnNo);
    assert!(!core.launch_warn_open());
    assert!(core.focused_app_launch_cmd_for_test().is_none(), "cancelling never launches");
    core.shutdown();
}

/// The `tetron-wm launch` escape hatch (ClientMsg::Launch) applies the same
/// help-then-shell wrapper to a bare launch of a catalog-flagged CLI tool
/// (gum is `"cli": true` in the embedded catalog) — previously it spawned the
/// bare binary, which printed usage and instantly died (the gap flagged by
/// PR #46).
#[test]
fn bare_cli_launch_via_client_msg_wraps_in_shell() {
    let mut core = SessionCore::new(80, 24, Config::default());
    core.apply(ClientMsg::Launch {
        name: "gum".into(),
        command: "gum".into(),
        args: vec![],
    });
    let (cmd, args) = core.focused_app_launch_cmd_for_test().expect("app launched");
    assert_eq!(cmd, "sh");
    assert_eq!(args.first().map(String::as_str), Some("-lc"));
    let script = args.get(1).cloned().unwrap_or_default();
    assert!(script.contains("'gum' --help"), "script: {script}");
    assert!(script.contains("exec \"${SHELL:-sh}\""), "script: {script}");
    core.shutdown();
}

/// Explicit args mean an intentional invocation — `tetron-wm launch gum choose a`
/// must run exactly as given, no wrapper. (The window is named after the
/// catalog CLI app "gum" but runs `true` so the spawn succeeds on any
/// machine — the point is that the CLI flag must NOT rewrite an invocation
/// that carries args.)
#[test]
fn cli_launch_with_args_via_client_msg_runs_bare() {
    let mut core = SessionCore::new(80, 24, Config::default());
    core.apply(ClientMsg::Launch {
        name: "gum".into(),
        command: "true".into(),
        args: vec!["choose".into(), "a".into()],
    });
    let (cmd, args) = core.focused_app_launch_cmd_for_test().expect("app launched");
    assert_eq!(cmd, "true");
    assert_eq!(args, vec!["choose".to_string(), "a".to_string()]);
    core.shutdown();
}

/// Text of every row of every layer, one string per layer row (for asserting
/// what a popup shows without caring about its exact position).
fn all_layer_text(core: &mut SessionCore) -> Vec<String> {
    let frame = core.build_frame();
    let mut out = Vec::new();
    for l in &frame.layers {
        for y in 0..l.buf.height() {
            out.push((0..l.buf.width()).map(|x| l.buf.get(x, y).map(|c| c.ch).unwrap_or(' ')).collect());
        }
    }
    out
}

#[test]
fn clicking_the_taskbar_overflow_marker_lists_the_hidden_windows() {
    // 8 distinctly named windows in a 60-col bar: the taskbar cannot show all
    // pills, so a "…+N" marker appears. Clicking it must list the hidden windows.
    let mut core = SessionCore::new(60, 24, Config::default());
    for i in 1..=8 {
        core.apply(ClientMsg::Launch { name: format!("win{i}"), command: "sh".into(), args: vec!["-c".into(), "sleep 3".into()] });
    }
    assert!(!core.dock_popup_open_for_test());
    // Find the marker on the menubar row.
    let rows = all_layer_text(&mut core);
    let bar = rows.iter().find(|r| r.contains("\u{2026}+")).expect("overflow marker is drawn").clone();
    let col = bar.chars().position(|c| c == '\u{2026}').unwrap() as i32;
    let after: String = bar.chars().skip(col as usize + 2).take_while(|c| c.is_ascii_digit()).collect();
    let hidden: usize = after.parse().unwrap();
    assert!(hidden >= 1);

    core.apply(ClientMsg::MouseDown(Point::new(col, 0)));
    assert!(core.dock_popup_open_for_test(), "clicking …+N opens the overflow list");
    let text = all_layer_text(&mut core).join("\n");
    // The last-launched window is hidden behind the marker, so it is listed.
    assert!(text.contains("win8"), "overflow list shows hidden windows:\n{text}");

    // Picking a row focuses that window and closes the list.
    let frame = core.build_frame();
    let mut target = None;
    for l in &frame.layers {
        for y in 0..l.buf.height() {
            let row: String = (0..l.buf.width()).map(|x| l.buf.get(x, y).map(|c| c.ch).unwrap_or(' ')).collect();
            if l.z == 5200 {
                if let Some(i) = row.find("win7") {
                    target = Some(Point::new(l.origin.x + row[..i].chars().count() as i32, l.origin.y + y));
                }
            }
        }
    }
    let target = target.expect("a row for win7 is drawn");
    core.apply(ClientMsg::MouseDown(target));
    assert_eq!(core.focused_label_for_test(), "win7", "clicking the row focuses the window");
    assert!(!core.dock_popup_open_for_test());

    // Re-open, then a second click on the marker dismisses it.
    core.apply(ClientMsg::MouseDown(Point::new(col, 0)));
    assert!(core.dock_popup_open_for_test());
    core.apply(ClientMsg::MouseDown(Point::new(col, 0)));
    assert!(!core.dock_popup_open_for_test());
    core.shutdown();
}

/// Screen position of the first cell showing `needle` on a layer of z `z`.
fn find_on_layer(core: &mut SessionCore, z: i32, needle: &str) -> Option<Point> {
    let frame = core.build_frame();
    for l in frame.layers.iter().filter(|l| l.z == z) {
        for y in 0..l.buf.height() {
            let row: String = (0..l.buf.width()).map(|x| l.buf.get(x, y).map(|c| c.ch).unwrap_or(' ')).collect();
            if let Some(i) = row.find(needle) {
                return Some(Point::new(l.origin.x + row[..i].chars().count() as i32, l.origin.y + y));
            }
        }
    }
    None
}

#[test]
fn info_menu_opens_from_the_bar_and_its_links_work() {
    let mut core = SessionCore::new(100, 30, Config::default());
    let icon = find_on_layer(&mut core, 1000, tetron_wm::glyphs::get(tetron_wm::glyphs::Glyph::Info)).expect("the ⓘ segment is on the menubar");
    assert_eq!(icon.y, 0);
    core.apply(ClientMsg::MouseDown(icon));
    // The popover shows the session facts and the links.
    let session = tetron_wm::protocol::session_name();
    assert!(find_on_layer(&mut core, 5000, "Session").is_some());
    assert!(find_on_layer(&mut core, 5000, &session).is_some());
    // "Sessions" opens a window running the live `ls` loop.
    let link = find_on_layer(&mut core, 5000, "Sessions").expect("Sessions link");
    core.apply(ClientMsg::MouseDown(Point::new(link.x + 2, link.y)));
    assert_eq!(core.window_count(), 1);
    assert_eq!(core.focused_label_for_test(), "sessions");
    // The popover closed.
    assert!(find_on_layer(&mut core, 5000, "Shortcuts").is_none());
    core.shutdown();
}

#[test]
fn info_menu_settings_link_opens_settings() {
    let mut core = SessionCore::new(100, 30, Config::default());
    let icon = find_on_layer(&mut core, 1000, tetron_wm::glyphs::get(tetron_wm::glyphs::Glyph::Info)).unwrap();
    core.apply(ClientMsg::MouseDown(icon));
    let link = find_on_layer(&mut core, 5000, "Settings").expect("Settings link");
    core.apply(ClientMsg::MouseDown(Point::new(link.x + 2, link.y)));
    assert_eq!(core.window_count(), 1, "settings opened");
    core.shutdown();
}

#[test]
fn poller_interest_follows_what_the_ui_shows() {
    use std::sync::{Arc, RwLock};
    use tetron_wm::system::{Interest, SystemState, WifiInfo};
    let mut core = SessionCore::new(100, 30, Config::default());
    let interest = Arc::new(Interest::default());
    core.attach_interest(interest.clone());
    let mut st = SystemState::default();
    st.caps.bluetooth = true;
    st.wifi = Some(WifiInfo { ssid: "wlan".into(), signal: 3, enabled: true });
    core.attach_tray_state(Arc::new(RwLock::new(st)));

    // Wi-Fi is on the bar (auto); Bluetooth lives in the Info menu (default),
    // closed, so nobody can see it: it is not polled.
    core.build_frame();
    let w = interest.want();
    assert!(w.net && w.volume && !w.bluetooth, "{w:?}");
    let kick0 = interest.kick_gen();

    // Open the Info menu, then the Bluetooth row: its popover needs the data.
    let icon = find_on_layer(&mut core, 1000, tetron_wm::glyphs::get(tetron_wm::glyphs::Glyph::Info)).unwrap();
    core.apply(ClientMsg::MouseDown(icon));
    core.build_frame();
    assert!(!interest.want().bluetooth, "the Info row alone needs no Bluetooth data");
    let row = find_on_layer(&mut core, 5000, "Bluetooth").expect("Bluetooth row in Info");
    core.apply(ClientMsg::MouseDown(Point::new(row.x + 2, row.y)));
    core.build_frame();
    assert!(interest.want().bluetooth, "its own popover is open");
    assert!(interest.kick_gen() > kick0, "the poller is told to read it now");

    // Closing it stops the polling again.
    core.apply(ClientMsg::MouseDown(Point::new(0, 20)));
    core.build_frame();
    assert!(!interest.want().bluetooth);
    core.shutdown();
}

#[test]
fn wifi_data_is_read_until_the_segment_exists() {
    use std::sync::{Arc, RwLock};
    use tetron_wm::system::{Interest, SystemState, WifiInfo};
    let mut core = SessionCore::new(100, 30, Config::default());
    let interest = Arc::new(Interest::default());
    core.attach_interest(interest.clone());
    // Nothing read yet: no Wi-Fi or network data, so there is no segment to show.
    let state = Arc::new(RwLock::new(SystemState::default()));
    core.attach_tray_state(state.clone());
    core.build_frame();
    assert!(interest.want().net, "the Wi-Fi/net segment cannot appear unless it is read");
    // Data arrives: the segment exists on the bar, still wanted.
    state.write().unwrap().wifi = Some(WifiInfo { ssid: "w".into(), signal: 3, enabled: true });
    core.build_frame();
    assert!(interest.want().net);
    let bar = find_on_layer(&mut core, 1000, "wifi");
    assert!(bar.is_some(), "the wifi segment is on the bar");
    core.shutdown();
}

#[test]
fn help_is_a_window_that_stays_open_while_you_use_everything_else() {
    let mut core = SessionCore::new(100, 40, Config::default());
    core.apply(ClientMsg::ToggleHelp);
    assert!(core.help_open() && core.focused_is_help());
    assert_eq!(core.window_count(), 1, "a real window");
    // The launcher (and every other click) still works while it is open.
    core.apply(ClientMsg::MouseDown(Point::new(2, 0)));
    assert!(core.launcher_open_for_test(), "the launcher opens with the shortcuts window up");
    assert!(core.help_open(), "and the shortcuts window stays");
    core.apply(ClientMsg::LauncherEsc);
    // Clicking the desktop does not dismiss it either.
    core.apply(ClientMsg::MouseDown(Point::new(2, 38)));
    assert!(core.help_open());
    // With another window focused, ToggleHelp refocuses it (no second copy);
    // from inside the help window, ToggleHelp closes it.
    core.apply(ClientMsg::Launch { name: "sh".into(), command: "sh".into(), args: vec!["-c".into(), "sleep 3".into()] });
    assert!(!core.focused_is_help());
    core.apply(ClientMsg::ToggleHelp);
    assert!(core.focused_is_help());
    assert_eq!(core.window_count(), 2, "no second copy");
    core.apply(ClientMsg::ToggleHelp);
    assert!(!core.help_open());
    assert_eq!(core.window_count(), 1);
    core.shutdown();
}

#[test]
fn configured_launcher_label_is_drawn_on_the_menubar() {
    let cfg = Config { launcher_label: "work".into(), ..Config::default() };
    let mut core = SessionCore::new(100, 30, cfg);
    assert!(find_on_layer(&mut core, 1000, "work").is_some(), "the label is on the bar");
    assert!(find_on_layer(&mut core, 1000, "tetron").is_none());
    // The brand button still opens the launcher.
    core.apply(ClientMsg::MouseDown(Point::new(2, 0)));
    assert!(core.launcher_open());
    core.shutdown();
}

// ── Screensaver ─────────────────────────────────────────────────────────────

fn saver_msg(mode: tetron_wm::screensaver::SaverMode, effect: Option<&str>) -> ClientMsg {
    ClientMsg::Screensaver { mode, effect: effect.map(String::from) }
}

/// Whether any cell of the frame's layers is not plain black.
fn frame_has_lit_cells(core: &SessionCore) -> bool {
    let frame = core.build_frame();
    frame.layers.iter().any(|l| {
        (0..l.buf.height()).any(|y| {
            (0..l.buf.width()).any(|x| {
                let c = l.buf.get(x, y).unwrap();
                c.bg.r != 0 || c.bg.g != 0 || c.bg.b != 0 || c.ch != ' '
            })
        })
    })
}

#[test]
fn screensaver_covers_everything_and_hides_cursor_and_images() {
    use tetron_wm::screensaver::SaverMode::*;
    let mut core = SessionCore::new(80, 24, Config::default());
    core.apply(ClientMsg::Launch { name: "a".into(), command: "sh".into(), args: vec!["-c".into(), "sleep 3".into()] });
    assert!(core.build_frame().layers.len() > 1, "a normal desktop has several layers");
    assert!(!core.screensaver_active());

    core.apply(saver_msg(On, None));
    assert!(core.screensaver_active());
    let f = core.build_frame();
    assert_eq!(f.layers.len(), 1, "the saver is the only layer, nothing leaks through");
    assert_eq!((f.layers[0].buf.width(), f.layers[0].buf.height()), (80, 24));
    assert!(f.cursor.is_none() && f.images.is_empty());
    assert!(!frame_has_lit_cells(&core), "the default effect is plain black");

    core.apply(saver_msg(Off, None));
    assert!(!core.screensaver_active());
    assert!(core.build_frame().layers.len() > 1, "the desktop is back");
    core.shutdown();
}

#[test]
fn screensaver_toggle_and_named_effects() {
    use tetron_wm::screensaver::SaverMode::*;
    let mut core = SessionCore::new(100, 30, Config::default());
    core.apply(saver_msg(Toggle, Some("clock")));
    assert!(core.screensaver_active() && frame_has_lit_cells(&core), "the clock draws lit cells");
    core.apply(saver_msg(Toggle, None));
    assert!(!core.screensaver_active(), "toggle again turns it off");
    // An unknown effect falls back to black instead of failing.
    core.apply(saver_msg(On, Some("no-such-effect")));
    assert!(core.screensaver_active() && !frame_has_lit_cells(&core));
    core.shutdown();
}

#[test]
fn screensaver_effect_defaults_to_the_config_key() {
    use tetron_wm::screensaver::SaverMode::*;
    let cfg = Config { screensaver: "clock".into(), ..Config::default() };
    let mut core = SessionCore::new(100, 30, cfg);
    core.apply(saver_msg(On, None));
    assert!(frame_has_lit_cells(&core), "screensaver = \"clock\" is used when none is named");
    core.shutdown();
}

#[test]
fn screensaver_follows_a_resize_and_repaints_when_due() {
    use tetron_wm::screensaver::SaverMode::*;
    let mut core = SessionCore::new(100, 30, Config::default());
    core.apply(saver_msg(On, Some("clock")));
    let first = core.build_frame().layers[0].buf.clone();
    core.apply(ClientMsg::Resize { w: 60, h: 20, takeover: false });
    core.tick_screensaver();
    let f = core.build_frame();
    assert_eq!((f.layers[0].buf.width(), f.layers[0].buf.height()), (60, 20), "repainted for the new size");
    // The clock is animated: after a second a tick paints something different.
    std::thread::sleep(std::time::Duration::from_millis(1100));
    core.apply(ClientMsg::Resize { w: 100, h: 30, takeover: false });
    core.tick_screensaver();
    let later = core.build_frame().layers[0].buf.clone();
    let differs = (0..30).any(|y| (0..100).any(|x| first.get(x, y).unwrap().bg != later.get(x, y).unwrap().bg));
    assert!(differs, "the clock moved or ticked");
    core.shutdown();
}

#[test]
fn screensaver_is_started_from_the_launcher_and_the_info_menu() {
    let mut core = SessionCore::new(100, 30, Config::default());
    // Info menu link.
    let icon = find_on_layer(&mut core, 1000, tetron_wm::glyphs::get(tetron_wm::glyphs::Glyph::Info)).unwrap();
    core.apply(ClientMsg::MouseDown(icon));
    let link = find_on_layer(&mut core, 5000, "Screensaver").expect("Screensaver link in the Info menu");
    core.apply(ClientMsg::MouseDown(Point::new(link.x + 2, link.y)));
    assert!(core.screensaver_active(), "the Info menu link starts it");
    assert!(core.build_frame().cursor.is_none());
    core.shutdown();
}

/// The mouse wheel over the working-directory picker moves its selection
/// (wheel down = further down the tree, wheel up = back).
#[test]
fn wheel_scrolls_the_working_directory_picker() {
    let mut cfg = Config::default();
    cfg.launcher.push(AppEntry {
        name: "needsdir".into(),
        command: "needsdir".into(),
        args: vec![],
        category: None,
        requires_cwd: Some(true),
        cwd: None,
        cli: None,
        warn: None,
    });
    let mut core = SessionCore::new(80, 24, cfg);
    core.apply(ClientMsg::ToggleSpotlight);
    for c in "needsdir".chars() {
        core.apply(ClientMsg::LauncherChar(c));
    }
    core.apply(ClientMsg::LauncherEnter);
    assert!(core.dirpicker_open());
    let top = core.dirpicker_selected().unwrap();
    let p = Point { x: 40, y: 12 };
    core.apply(ClientMsg::ScrollAt { p, lines: -2 });
    let down = core.dirpicker_selected().unwrap();
    assert_ne!(down, top, "wheel down moved the selection");
    core.apply(ClientMsg::ScrollAt { p, lines: 2 });
    assert_eq!(core.dirpicker_selected().unwrap(), top, "wheel up came back");
    core.shutdown();
}

#[test]
fn pointer_follows_mouse_passed_through_to_an_app() {
    use tetron_wm::mouse::{MouseAction, MouseButton, MouseInput, MouseMods};
    let mut core = SessionCore::new(80, 24, Config::default());
    core.apply(ClientMsg::MouseInput(MouseInput {
        col: 30, row: 10, button: MouseButton::None, action: MouseAction::Move, mods: MouseMods::default(),
    }));
    assert_eq!(core.build_frame().cursor, Some(Point::new(30, 10)));
}

/// Launch three sleeping shells named a, b, c; returns their ids in order.
fn three_windows(core: &mut SessionCore) -> Vec<tetron_wm::window::WindowId> {
    let mut ids = Vec::new();
    for n in ["a", "b", "c"] {
        core.apply(ClientMsg::Launch { name: n.into(), command: "sh".into(), args: vec!["-c".into(), "sleep 2".into()] });
        ids.push(core.focused().unwrap());
    }
    ids
}

#[test]
fn focus_next_and_prev_cycle_in_taskbar_order_and_wrap() {
    let mut core = SessionCore::new(120, 40, Config::default());
    let ids = three_windows(&mut core);
    assert_eq!(core.focused(), Some(ids[2]));
    core.apply(ClientMsg::FocusNext);
    assert_eq!(core.focused(), Some(ids[0]), "wraps from last to first");
    core.apply(ClientMsg::FocusNext);
    assert_eq!(core.focused(), Some(ids[1]));
    core.apply(ClientMsg::FocusPrev);
    core.apply(ClientMsg::FocusPrev);
    assert_eq!(core.focused(), Some(ids[2]), "wraps from first to last");
    core.shutdown();
}

#[test]
fn focus_next_skips_minimized_unless_all_are() {
    let mut core = SessionCore::new(120, 40, Config::default());
    let ids = three_windows(&mut core);
    // Minimize b: cycling from c goes a, c, a ... never b.
    core.apply(ClientMsg::FocusPrev);
    assert_eq!(core.focused(), Some(ids[1]));
    core.apply(ClientMsg::MinimizeFocused);
    for _ in 0..4 {
        core.apply(ClientMsg::FocusNext);
        assert_ne!(core.focused(), Some(ids[1]));
    }
    // Minimize everything: the step then restores a window.
    for _ in 0..3 {
        core.apply(ClientMsg::MinimizeFocused);
    }
    core.apply(ClientMsg::FocusNext);
    assert!(core.focused().is_some(), "all minimized: Tab restores one");
    core.shutdown();
}

#[test]
fn window_picker_lists_all_and_restores_the_pick() {
    let mut core = SessionCore::new(120, 40, Config::default());
    let ids = three_windows(&mut core);
    core.apply(ClientMsg::FocusNext); // a
    core.apply(ClientMsg::MinimizeFocused);
    core.apply(ClientMsg::WindowPickerOpen);
    assert!(core.window_picker_open());
    // Digit 1 is a, minimized: picking restores and focuses it.
    core.apply(ClientMsg::WindowPickerNumber(1));
    assert!(!core.window_picker_open());
    assert_eq!(core.focused(), Some(ids[0]));
    // Arrow + Enter: open highlights the focused row (a), one down is b.
    core.apply(ClientMsg::WindowPickerOpen);
    core.apply(ClientMsg::WindowPickerStep(1));
    core.apply(ClientMsg::WindowPickerEnter);
    assert_eq!(core.focused(), Some(ids[1]));
    // Esc closes without changing focus.
    core.apply(ClientMsg::WindowPickerOpen);
    core.apply(ClientMsg::WindowPickerStep(1));
    core.apply(ClientMsg::WindowPickerEsc);
    assert!(!core.window_picker_open());
    assert_eq!(core.focused(), Some(ids[1]));
    core.shutdown();
}

#[test]
fn move_mode_moves_resizes_and_esc_undoes() {
    let mut core = SessionCore::new(120, 40, Config::default());
    core.apply(ClientMsg::Launch { name: "a".into(), command: "sh".into(), args: vec!["-c".into(), "sleep 2".into()] });
    let start = core.focused_window_rect_for_test().unwrap();
    core.apply(ClientMsg::MoveModeStart);
    assert!(core.move_mode_open());
    core.apply(ClientMsg::MoveModeStep { dx: 5, dy: 0 });
    core.apply(ClientMsg::MoveModeStep { dx: 0, dy: 1 });
    let moved = core.focused_window_rect_for_test().unwrap();
    assert_eq!((moved.x, moved.y, moved.w, moved.h), (start.x + 5, start.y + 1, start.w, start.h));
    core.apply(ClientMsg::MoveModeToggleResize);
    assert!(core.move_mode_resizing());
    core.apply(ClientMsg::MoveModeStep { dx: 2, dy: -1 });
    let sized = core.focused_window_rect_for_test().unwrap();
    assert_eq!((sized.x, sized.w, sized.h), (moved.x, start.w + 2, start.h - 1));
    // Esc puts everything back.
    core.apply(ClientMsg::MoveModeEnd { cancel: true });
    assert!(!core.move_mode_open());
    assert_eq!(core.focused_window_rect_for_test(), Some(start));
    core.shutdown();
}

#[test]
fn move_mode_enter_keeps_and_restores_maximized_on_esc() {
    let mut core = SessionCore::new(120, 40, Config::default());
    core.apply(ClientMsg::Launch { name: "a".into(), command: "sh".into(), args: vec!["-c".into(), "sleep 2".into()] });
    let start = core.focused_window_rect_for_test().unwrap();
    core.apply(ClientMsg::MoveModeStart);
    core.apply(ClientMsg::MoveModeStep { dx: 3, dy: 0 });
    core.apply(ClientMsg::MoveModeEnd { cancel: false });
    assert!(!core.move_mode_open());
    assert_eq!(core.focused_window_rect_for_test().unwrap().x, start.x + 3);
    // A maximized window moved then Esc'd comes back maximized.
    core.apply(ClientMsg::MaximizeFocused);
    let max = core.focused_window_rect_for_test().unwrap();
    core.apply(ClientMsg::MoveModeStart);
    core.apply(ClientMsg::MoveModeStep { dx: 0, dy: 4 });
    core.apply(ClientMsg::MoveModeEnd { cancel: true });
    assert_eq!(core.focused_window_rect_for_test(), Some(max));
    core.apply(ClientMsg::MaximizeFocused);
    assert_eq!(core.focused_window_rect_for_test().unwrap().x, start.x + 3, "restore still works");
    core.shutdown();
}

#[test]
fn leader_volume_toggles_the_volume_popover() {
    let mut core = SessionCore::new(120, 40, Config::default());
    core.apply(ClientMsg::VolumeOpen);
    assert!(core.volume_popover_open());
    core.apply(ClientMsg::TrayKey(tetron_wm::tray::TrayKey::Esc));
    assert!(!core.volume_popover_open());
    core.apply(ClientMsg::VolumeOpen);
    core.apply(ClientMsg::VolumeOpen);
    assert!(!core.volume_popover_open(), "leader = again closes it");
    core.shutdown();
}

#[test]
fn colors_setting_overrides_detection_unless_the_env_forced_it() {
    use tetron_wm::display::{ClientCaps, ColorDepth};
    let before = tetron_wm::display::get();
    let mut core = SessionCore::new(80, 24, Config { apps: vec![], desktop_enabled: false, colors: "mono".into(), ..Config::default() });
    core.apply(ClientMsg::Caps(ClientCaps { colors: ColorDepth::TrueColor, ..Default::default() }));
    assert_eq!(core.colors_override(), Some(ColorDepth::Mono), "the client is told to render black and white");
    assert_eq!(tetron_wm::display::get().colors, ColorDepth::Mono, "and the theme adapts to it");
    assert!(core.take_repaint(), "a depth change re-baselines the screen");
    // TETRON_WM_COLORS on the client wins over the setting.
    core.apply(ClientMsg::Caps(ClientCaps { colors: ColorDepth::Ansi256, colors_forced: true, ..Default::default() }));
    assert_eq!(core.colors_override(), None);
    assert_eq!(tetron_wm::display::get().colors, ColorDepth::Ansi256);
    // Auto: the client's own detection.
    core.set_config_for_test(Config { colors: "auto".into(), ..Config::default() });
    core.apply(ClientMsg::Caps(ClientCaps { colors: ColorDepth::Ansi16, ..Default::default() }));
    assert_eq!(core.colors_override(), None);
    tetron_wm::display::set(before);
}

#[test]
fn cli_launch_of_a_builtin_opens_the_builtin_window() {
    // `tetron-wm launch @settings` used to spawn "@settings" as a program.
    let mut core = SessionCore::new(100, 30, Config::default());
    core.apply(ClientMsg::Launch { name: "@settings".into(), command: "@settings".into(), args: vec![] });
    assert_eq!(core.window_count(), 1, "Settings window");
    let dir = std::env::temp_dir();
    core.apply(ClientMsg::Launch { name: "@files".into(), command: "@files".into(), args: vec![dir.display().to_string()] });
    assert_eq!(core.window_count(), 2, "Files window");
    core.shutdown();
}

/// WMCAT-006: Alt+O in the Store copies the selected app's releases page to
/// the clipboard (a terminal browser, when installed, opens it too).
#[test]
fn store_open_releases_copies_the_link() {
    let app = tetron_wm::catalog::catalog().iter().find(|a| tetron_wm::catalog::releases_url(a).is_some() && a.name.len() > 5).expect("an app with releases");
    let mut core = SessionCore::new(120, 40, Config::default());
    core.apply(ClientMsg::OpenStore);
    for c in app.name.chars() {
        core.apply(ClientMsg::StoreChar(c));
    }
    let _ = core.take_clipboard();
    core.apply(ClientMsg::StoreOpenReleases);
    let got = core.take_clipboard().expect("link copied");
    assert!(got.ends_with("/releases/latest") && got.starts_with("https://github.com/"), "{got}");
    core.shutdown();
}
