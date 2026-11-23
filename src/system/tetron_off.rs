//! tetron mesh probe when tetron-wm is built without the `mesh` feature
//! (WMDEP-004): same API as `tetron.rs`, no tetron core dependency. The tray
//! sees a daemon that is not reachable and hides the tetron segment.

use crate::system::TetronInfo;

/// Always the not-reachable default.
pub fn poll() -> TetronInfo {
    TetronInfo::default()
}

/// Nothing to activate without the mesh client.
pub fn set_active(_network: Option<String>, _active: bool) {}

#[cfg(test)]
mod tests {
    #[test]
    fn without_mesh_the_daemon_is_never_reachable() {
        assert!(!super::poll().reachable);
    }
}
