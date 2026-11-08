//! App-managed state, kept out of the hand-edited `config.toml`.
//!
//! `desktop_positions` (icon grid cells, rewritten on every icon drag) and
//! `recent_dirs` (the working-directory MRU) used to live in the config, which
//! made the config churn constantly and mostly machine data. They now live in
//! `~/.local/share/tetron-wm/state.toml` (`$XDG_DATA_HOME` aware). Nothing here
//! needs comment preservation: nobody hand-edits this file.

use crate::config::Config;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct State {
    /// Recently chosen working directories, most recent first (capped by the session).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub recent_dirs: Vec<String>,
    /// Saved desktop icon grid positions: key (abs path or pin command) → (col, row).
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub desktop_positions: BTreeMap<String, (u16, u16)>,
}

/// `$XDG_DATA_HOME/tetron-wm/state.toml`, else `~/.local/share/tetron-wm/state.toml`.
pub fn state_path() -> Option<PathBuf> {
    let base = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|| dirs::home_dir().map(|h| h.join(".local/share")))?;
    Some(base.join("tetron-wm").join("state.toml"))
}

impl State {
    /// Load the state file, migrating the legacy `recent_dirs` / `desktop_positions`
    /// keys out of `config.toml` the first time (when no state file exists yet).
    /// The legacy keys are then removed from the config file; everything else in it
    /// is left alone. An unreadable or unparsable state file is set aside as
    /// `state.toml.bad` and the state starts empty.
    pub fn load_migrating(cfg: &Config) -> State {
        let Some(path) = state_path() else { return State::default() };
        Self::load_from(&path, cfg, config_path().as_deref())
    }

    fn load_from(path: &Path, cfg: &Config, config_file: Option<&Path>) -> State {
        match std::fs::read_to_string(path) {
            Ok(text) => match toml::from_str::<State>(&text) {
                Ok(s) => return s,
                Err(e) => {
                    crate::log_warn(&format!("state: {} failed to parse ({e}); set aside as .bad", path.display()));
                    let mut bad = path.as_os_str().to_owned();
                    bad.push(".bad");
                    let _ = std::fs::rename(path, PathBuf::from(bad));
                    return State::default();
                }
            },
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => return State::default(),
            Err(_) => {}
        }
        // No state file yet: carry over whatever the old config held.
        let migrated = State { recent_dirs: cfg.recent_dirs.clone(), desktop_positions: cfg.desktop_positions.clone() };
        if migrated != State::default() {
            match migrated.save(path) {
                Ok(()) => {
                    crate::dbg_log(&format!(
                        "state: migrated {} dirs and {} icon positions from config.toml to {}",
                        migrated.recent_dirs.len(),
                        migrated.desktop_positions.len(),
                        path.display()
                    ));
                    if let Some(cf) = config_file {
                        let _ = crate::configfile::remove_keys(cf, &["recent_dirs", "desktop_positions"]);
                    }
                }
                Err(e) => crate::log_warn(&format!("state: migration write failed: {e}")),
            }
        }
        migrated
    }

    /// Write the state to `path` (atomic). Nothing is written when the file
    /// already holds exactly this state.
    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        let text = toml::to_string_pretty(self).map_err(std::io::Error::other)?;
        if std::fs::read_to_string(path).map(|cur| cur == text).unwrap_or(false) {
            return Ok(());
        }
        crate::configfile::write_atomic(path, &text)
    }
}

fn config_path() -> Option<PathBuf> {
    crate::config::config_file_path(std::env::var_os("XDG_CONFIG_HOME"), dirs::home_dir())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dir(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("tetron-state-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn sample() -> State {
        State {
            recent_dirs: vec!["/a".into(), "/b".into()],
            desktop_positions: [("/home/x/Desktop/f".to_string(), (1, 2))].into_iter().collect(),
        }
    }

    #[test]
    fn round_trips_and_skips_identical_writes() {
        let d = dir("roundtrip");
        let p = d.join("state.toml");
        sample().save(&p).unwrap();
        let first = std::fs::metadata(&p).unwrap().modified().unwrap();
        assert_eq!(State::load_from(&p, &Config::default(), None), sample());
        sample().save(&p).unwrap();
        assert_eq!(std::fs::metadata(&p).unwrap().modified().unwrap(), first, "unchanged state is not rewritten");
    }

    #[test]
    fn first_run_migrates_from_the_config_and_retires_the_legacy_keys() {
        let d = dir("migrate");
        let cfg_file = d.join("config.toml");
        std::fs::write(
            &cfg_file,
            "# mine\nsnap_threshold = 3\nrecent_dirs = [\"/a\", \"/b\"]\n\n[desktop_positions]\n\"/home/x/Desktop/f\" = [1, 2]\n",
        )
        .unwrap();
        let cfg = Config::from_toml_str(&std::fs::read_to_string(&cfg_file).unwrap()).unwrap();
        let p = d.join("data").join("state.toml");
        let s = State::load_from(&p, &cfg, Some(&cfg_file));
        assert_eq!(s, sample());
        assert!(p.exists(), "state file written");
        let left = std::fs::read_to_string(&cfg_file).unwrap();
        assert_eq!(left, "# mine\nsnap_threshold = 3\n", "only the legacy keys left the config: {left:?}");
        // A second load reads the state file and does not touch the config again.
        assert_eq!(State::load_from(&p, &Config::default(), Some(&cfg_file)), sample());
    }

    #[test]
    fn a_broken_state_file_is_set_aside_not_overwritten_silently() {
        let d = dir("bad");
        let p = d.join("state.toml");
        std::fs::write(&p, "recent_dirs = = 1\n").unwrap();
        assert_eq!(State::load_from(&p, &Config::default(), None), State::default());
        assert!(d.join("state.toml.bad").exists());
        assert!(!p.exists());
    }

    #[test]
    fn an_empty_config_creates_no_state_file() {
        let d = dir("empty");
        let p = d.join("state.toml");
        assert_eq!(State::load_from(&p, &Config::default(), None), State::default());
        assert!(!p.exists());
    }
}
