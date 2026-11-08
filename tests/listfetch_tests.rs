//! Store > Update list end to end (WMCAT-004): curl fetches the catalog from a
//! file:// lists directory laid out like the published one; a good list is
//! used and kept, a bad one changes nothing. Own test binary: it sets
//! XDG_DATA_HOME and TETRON_WM_LISTS_URL for the whole process.

use tetron_wm::{catalog, listfetch};

#[test]
fn update_list_uses_and_keeps_a_good_list_and_refuses_a_bad_one() {
    let tmp = std::env::temp_dir().join(format!("tetron-wm-listfetch-{}", std::process::id()));
    let minor = listfetch::minor_of(env!("CARGO_PKG_VERSION"));
    let dir = tmp.join("lists").join(&minor);
    std::fs::create_dir_all(&dir).unwrap();
    // SAFETY: this test binary has one test, so no other thread reads the environment.
    unsafe {
        std::env::set_var("XDG_DATA_HOME", tmp.join("data"));
        std::env::set_var("TETRON_WM_LISTS_URL", format!("file://{}", tmp.join("lists").display()));
    }
    let builtin = include_str!("../assets/catalog.json");
    let mut doc: serde_json::Value = serde_json::from_str(builtin).unwrap();
    doc["apps"].as_array_mut().unwrap().truncate(3);
    std::fs::write(dir.join("catalog.json"), doc.to_string()).unwrap();

    assert_eq!(listfetch::run(), "List updated: 3 apps");
    assert_eq!(catalog::catalog().len(), 3, "the download is in use");
    let kept = catalog::cache_path().unwrap();
    assert!(kept.starts_with(tmp.join("data")) && kept.is_file());

    doc["schema"] = (catalog::SCHEMA + 1).into();
    std::fs::write(dir.join("catalog.json"), doc.to_string()).unwrap();
    let msg = listfetch::run();
    assert!(msg.contains("not used") && msg.contains("schema"), "{msg}");
    assert_eq!(catalog::catalog().len(), 3, "the list in use stays");

    std::fs::remove_file(dir.join("catalog.json")).unwrap();
    assert!(listfetch::run().contains("download failed"));
    assert_eq!(catalog::parse_count(&std::fs::read_to_string(&kept).unwrap()), Ok(3), "the kept copy is untouched");
    std::fs::remove_dir_all(&tmp).unwrap();
}
