//! The app catalog for tetron-wm: everything from the shared `tetron-catalog`
//! crate (WMCAT-008), plus what is tetron-wm's own: launcher entries for the
//! catalogued apps that are installed. The Update list fetch is
//! tetron-appstore (WMCAT-004).

pub use tetron_catalog::*;

use crate::config::AppEntry;

/// Return catalog apps whose binary is present on the current `$PATH`, plus
/// one extra entry per declared [`Variant`] of each installed app (e.g. the
/// Claude Code "⚠️" skip-permissions flavor) — data-driven, no per-app
/// special-casing here.
pub fn detect_installed() -> Vec<AppEntry> {
    let bins = installed_bins();
    let mut entries = Vec::new();
    for c in catalog().iter().filter(|c| bins.contains(&c.bin) || bins.contains(&c.name.to_lowercase())) {
        let requires_cwd = Some(recipe(&c.name).map(|r| r.requires_cwd).unwrap_or(false));
        entries.push(AppEntry {
            name: c.name.clone(),
            command: c.bin.clone(),
            args: Vec::new(),
            category: Some(c.category.clone()),
            requires_cwd,
            cwd: None,
            cli: Some(c.cli),
            warn: None,
        });
        for v in &c.variants {
            entries.push(AppEntry {
                name: format!("{} {}", c.name, if v.suffix == "\u{26A0}\u{FE0F}" { crate::iconset::warning() } else { v.suffix.as_str() }),
                command: c.bin.clone(),
                args: v.args.clone(),
                category: Some(c.category.clone()),
                requires_cwd,
                cwd: None,
                // Variants launch exactly as declared, not through the CLI
                // help-then-shell wrapper.
                cli: None,
                warn: v.warn.clone(),
            });
        }
    }
    entries
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    /// `detect_installed` emits a base entry plus one extra entry per declared
    /// variant, carrying the variant's args/warn through and leaving `cli`
    /// unset (variants launch exactly as declared). Fakes the `$PATH` scan via
    /// [`set_installed_bins`] — real `is_installed`/`detect_installed`
    /// callers elsewhere have no inline tests, so this can't race them.
    #[test]
    fn detect_installed_emits_variant_entry_for_claude_code() {
        let mut fake = HashSet::new();
        fake.insert("claude".to_string());
        set_installed_bins(Some(fake));

        let entries = detect_installed();

        let base = entries.iter().find(|e| e.name == "Claude Code").expect("base entry present");
        assert_eq!(base.command, "claude");
        assert!(base.args.is_empty());
        assert_eq!(base.warn, None, "the plain entry carries no warning");

        let variant = entries.iter().find(|e| e.name == format!("Claude Code {}", crate::iconset::warning())).expect("variant entry present");
        assert_eq!(variant.command, "claude");
        assert_eq!(variant.args, vec!["--dangerously-skip-permissions".to_string()]);
        assert_eq!(variant.cli, None, "variants launch as given, not through the CLI wrapper");
        assert_eq!(variant.category, base.category);
        assert_eq!(variant.requires_cwd, base.requires_cwd);
        assert!(variant.warn.as_deref().unwrap_or("").contains("--dangerously-skip-permissions"));

        // Leave the cache empty so a later real scan repopulates it lazily.
        set_installed_bins(None);
    }

    /// tetron-wm's own helper: the launcher finds it by a prefixed name on PATH (never a
    /// generic `startpage`), and its Store recipe fetches a script that exists in `scripts/`
    /// under that same name. AI Mode (`tetron-aimode`) is its own repository and package:
    /// its recipe fetches the script from tetronos/tetron-aimode.
    #[test]
    fn own_helpers_use_prefixed_names_backed_by_scripts() {
        let app = catalog().iter().find(|a| a.name == "Start Page").expect("Start Page in the catalog");
        assert_eq!(app.bin, "tetron-wm-startpage");
        let r = recipe("Start Page").expect("Start Page recipe");
        assert!(r.install.contains("/scripts/tetron-wm-startpage "), "{}", r.install);
        assert!(r.install.contains("~/.local/bin/tetron-wm-startpage"), "{}", r.install);
        assert!(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("scripts").join("tetron-wm-startpage").is_file());

        let app = catalog().iter().find(|a| a.name == "AI Mode").expect("AI Mode in the catalog");
        assert_eq!(app.bin, "tetron-aimode");
        let r = recipe("AI Mode").expect("AI Mode recipe");
        assert!(r.install.contains("tetronos/tetron-aimode/main/tetron-aimode"), "{}", r.install);
        assert!(r.install.contains("~/.local/bin/tetron-aimode"), "{}", r.install);
        assert!(!std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("scripts").join("tetron-aimode").exists(), "moved to its own repository");
    }

    /// CON-WD07: the catalog tetron-wm is built with carries exactly this
    /// repository's suite list (its app sets), so Settings > General > Apps
    /// and the suite installer agree with the packages.
    #[test]
    fn built_in_catalog_carries_our_suite_list() {
        assert_eq!(suite_apps(), include_str!("../assets/suite-apps.txt"));
    }
}