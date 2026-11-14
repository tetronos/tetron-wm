use tetron_wm::store::Store;

#[test]
fn store_has_all_category_first_and_lists_apps() {
    let s = Store::new();
    // "All" shows the whole catalog
    assert!(s.filtered().len() > 400);
    assert!(s.selected_app().is_some());
}

#[test]
fn category_filter_narrows_results() {
    let mut s = Store::new();
    let all = s.filtered().len();
    s.next_category(); // move off "All" to the first real category
    let narrowed = s.filtered().len();
    assert!(narrowed > 0 && narrowed < all);
}

#[test]
fn query_filters_by_name() {
    let mut s = Store::new();
    for c in "btop".chars() { s.type_char(c); }
    assert!(s.filtered().iter().any(|a| a.name.to_lowercase().contains("btop")));
    assert!(s.filtered().len() < 50);
    s.backspace();
    s.backspace();
    s.backspace();
    s.backspace();
    assert!(s.filtered().len() > 400); // query cleared
}

#[test]
fn render_returns_sized_buffer() {
    let s = Store::new();
    let buf = s.render(80, 24);
    assert_eq!(buf.width(), 80);
    assert_eq!(buf.height(), 24);
}

#[test]
fn click_selects_category_and_row() {
    use tetron_wm::geometry::Point;
    let mut s = Store::new();
    // sidebar: category at y = 2 + index; click "Dashboards" (index 1)
    let _ = s.handle_click(Point::new(2, 3), 96, 22);
    assert!(s.filtered().len() < 500 && !s.filtered().is_empty());
    // list column starts at x=17; first visible row at y=2 selects index 0
    let _ = s.handle_click(Point::new(20, 2), 96, 22);
    assert_eq!(s.selected_app().map(|a| a.name.clone()), s.filtered().first().map(|a| a.name.clone()));
    // second click on the same (selected) row activates
    assert_eq!(s.handle_click(Point::new(20, 2), 96, 22), tetron_wm::store::StoreClick::Activate);
}

/// WMCAT-006: the "Prebuilt" line is drawn and clickable only for an app with
/// a releases page, on the same row render and hit-test share.
#[test]
fn prebuilt_line_click_opens_releases() {
    use tetron_wm::geometry::Point;
    use tetron_wm::store::StoreClick;
    let (w, h) = (110, 30);
    let row = |s: &Store, y: i32| -> String { let b = s.render(w, h); (0..w).map(|x| b.get(x, y).map(|c| c.ch).unwrap_or(' ')).collect() };
    let with = tetron_wm::catalog::catalog().iter().find(|a| tetron_wm::catalog::releases_url(a).is_some() && a.name.len() > 5).unwrap();
    let mut s = Store::new();
    for c in with.name.chars() { s.type_char(c); }
    assert_eq!(s.selected_app().map(|a| a.name.as_str()), Some(with.name.as_str()));
    let y = (0..h).find(|&y| row(&s, y).contains("Prebuilt (Alt+O): https://github.com/")).expect("prebuilt line");
    let x = (0..w).find(|&x| s.render(w, h).get(x, y).map(|c| c.ch) == Some('P')).unwrap();
    assert_eq!(s.handle_click(Point::new(x + 2, y), w, h), StoreClick::OpenReleases);
    let without = tetron_wm::catalog::catalog().iter().find(|a| !a.releases && a.name.len() > 5 && !a.name.contains(' ')).unwrap();
    let mut s = Store::new();
    for c in without.name.chars() { s.type_char(c); }
    if s.selected_app().map(|a| a.releases) == Some(false) {
        assert_eq!(s.handle_click(Point::new(x + 2, y), w, h), StoreClick::None);
    }
}
