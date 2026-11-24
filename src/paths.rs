//! Where things are on this system: programs on `PATH`, shared data in the
//! XDG data directories, and how tetron-wm itself is installed (a Nix store
//! or not). Code that needs a program, a data file or the path of tetron-wm
//! asks here instead of using a fixed /usr path, so the same binary works on
//! Debian, NixOS and anything else (WMNIX-001, WMNIX-003 to WMNIX-005,
//! WMNIX-008).

use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

/// The Nix store. Files under it are read-only and versioned: a path into
/// it changes with every upgrade and is deleted by garbage collection.
pub const NIX_STORE: &str = "/nix/store";

/// `$XDG_DATA_DIRS` when unset or empty (XDG Base Directory spec).
pub const DEFAULT_DATA_DIRS: &str = "/usr/local/share:/usr/share";

/// The name of the inner binary a Nix wrapper (makeWrapper) runs.
const WRAPPED: &str = ".tetron-wm-wrapped";

/// Whether `p` is a regular file with an execute bit.
pub fn is_executable(p: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(p).is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
}

/// The first executable `name` in the directories of `path` (a `PATH`
/// value). A name with a slash, or an empty one, is never looked up.
pub fn which_in(name: &str, path: &OsStr) -> Option<PathBuf> {
    if name.is_empty() || name.contains('/') {
        return None;
    }
    std::env::split_paths(path).filter(|d| d.is_absolute()).map(|d| d.join(name)).find(|p| is_executable(p))
}

/// The first executable `name` on `$PATH`.
pub fn which(name: &str) -> Option<PathBuf> {
    which_in(name, &std::env::var_os("PATH")?)
}

/// Whether an executable `name` is on `$PATH`.
pub fn on_path(name: &str) -> bool {
    which(name).is_some()
}

/// The XDG data directories from a `$XDG_DATA_DIRS` value: the default list
/// when unset or empty, relative entries dropped.
pub fn data_dirs_from(v: Option<OsString>) -> Vec<PathBuf> {
    let v = v.filter(|v| !v.is_empty()).unwrap_or_else(|| DEFAULT_DATA_DIRS.into());
    std::env::split_paths(&v).filter(|d| d.is_absolute()).collect()
}

/// The XDG data directories of this process.
pub fn data_dirs() -> Vec<PathBuf> {
    data_dirs_from(std::env::var_os("XDG_DATA_DIRS"))
}

/// `rel` under each XDG data directory, in order (`consolefonts`,
/// `fonts`, `systemd/user`, ...). Missing directories are included; callers
/// skip what does not exist.
pub fn data_subdirs(rel: &str) -> Vec<PathBuf> {
    data_dirs().into_iter().map(|d| d.join(rel)).collect()
}

/// Console font directories: `consolefonts` under each XDG data directory,
/// then kbd's own directory on distributions that keep it there (WMNIX-005).
pub fn consolefont_dirs() -> Vec<PathBuf> {
    let mut d = data_subdirs("consolefonts");
    d.push(PathBuf::from("/usr/lib/kbd/consolefonts"));
    d
}

/// Font directories: `fonts` under each XDG data directory (WMNIX-005).
pub fn font_dirs() -> Vec<PathBuf> {
    data_subdirs("fonts")
}

/// Whether a file whose name starts with `prefix` is in one of `dirs` or
/// their subdirectories, at most `depth` levels down (font trees nest
/// `truetype/<family>/`).
pub fn has_file_with_prefix(dirs: &[PathBuf], prefix: &str, depth: u8) -> bool {
    dirs.iter().any(|d| {
        std::fs::read_dir(d).is_ok_and(|rd| {
            rd.flatten().any(|e| {
                let is_dir = e.file_type().is_ok_and(|t| t.is_dir());
                if is_dir {
                    depth > 0 && has_file_with_prefix(&[e.path()], prefix, depth - 1)
                } else {
                    e.file_name().to_string_lossy().starts_with(prefix)
                }
            })
        })
    })
}

/// Whether `p` is inside the store directory `store`.
pub fn in_store(p: &Path, store: &Path) -> bool {
    p.starts_with(store)
}

/// Whether the running tetron-wm was installed by Nix: its binary, with
/// symlinks resolved, is in the Nix store (WMNIX-001). Decided once.
pub fn running_from_nix() -> bool {
    static NIX: OnceLock<bool> = OnceLock::new();
    *NIX.get_or_init(|| {
        let nix = std::env::current_exe()
            .ok()
            .and_then(|e| std::fs::canonicalize(&e).ok().or(Some(e)))
            .is_some_and(|e| in_store(&e, Path::new(NIX_STORE)));
        if nix {
            crate::dbg_log("paths: the running tetron-wm is in the Nix store (install method nix)");
        }
        nix
    })
}

/// How the stable self path was found (WMNIX-003), for the debug log.
#[derive(Debug, PartialEq, Eq)]
pub enum SelfPathSource {
    /// `$TETRON_WM_EXE`.
    Env,
    /// A profile link on `PATH` into the store.
    Profile,
    /// The running binary (or the wrapper next to it).
    Running,
    /// The running binary is in the store and no profile link was found:
    /// a written path breaks after an upgrade.
    StoreOnly,
}

/// The stable self path (WMNIX-003) from its inputs: `env` is
/// `$TETRON_WM_EXE`, `running` the running binary, `path` the `PATH` value,
/// `store` the store directory.
pub fn stable_self_path_from(
    env: Option<&OsStr>,
    running: &Path,
    path: Option<&OsStr>,
    store: &Path,
) -> (PathBuf, SelfPathSource) {
    if let Some(e) = env.map(PathBuf::from).filter(|e| e.is_absolute() && is_executable(e)) {
        return (e, SelfPathSource::Env);
    }
    let real = std::fs::canonicalize(running).unwrap_or_else(|_| running.to_path_buf());
    let from_store = in_store(&real, store);
    if from_store {
        let link = path.into_iter().flat_map(std::env::split_paths).filter(|d| d.is_absolute()).find_map(|d| {
            let c = d.join("tetron-wm");
            let target = std::fs::canonicalize(&c).ok()?;
            (!in_store(&c, store) && in_store(&target, store) && is_executable(&c)).then_some(c)
        });
        if let Some(l) = link {
            return (l, SelfPathSource::Profile);
        }
    }
    let unwrapped = match running.file_name() {
        Some(n) if n == WRAPPED => {
            running.parent().map(|d| d.join("tetron-wm")).filter(|w| is_executable(w)).unwrap_or_else(|| running.to_path_buf())
        }
        _ => running.to_path_buf(),
    };
    (unwrapped, if from_store { SelfPathSource::StoreOnly } else { SelfPathSource::Running })
}

/// The path of tetron-wm to write into files that are used later (service
/// unit, mc.ini, shims, agent configs): survives upgrades and garbage
/// collection where the install allows it (WMNIX-003). Falls back to the
/// bare name when the running binary is unknown.
pub fn stable_self_path() -> PathBuf {
    let Ok(running) = crate::self_exe() else {
        return PathBuf::from("tetron-wm");
    };
    // A replaced binary that is not back yet: the path is what counts.
    let running = match running.to_str().and_then(|s| s.strip_suffix(" (deleted)")) {
        Some(s) => PathBuf::from(s),
        None => running,
    };
    let env = std::env::var_os("TETRON_WM_EXE");
    let path = std::env::var_os("PATH");
    let (p, src) = stable_self_path_from(env.as_deref(), &running, path.as_deref(), Path::new(NIX_STORE));
    if src == SelfPathSource::StoreOnly {
        crate::dbg_log(&format!(
            "paths: no profile link to tetron-wm on PATH; writing the store path {} (breaks after a Nix upgrade)",
            p.display()
        ));
    }
    p
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("twm-paths-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn exe(p: &Path) {
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, "#!/bin/sh\n").unwrap();
        std::fs::set_permissions(p, std::fs::Permissions::from_mode(0o755)).unwrap();
    }

    #[test]
    fn which_finds_only_executables_in_absolute_dirs() {
        let t = tmp("which");
        exe(&t.join("b/tool"));
        std::fs::create_dir_all(t.join("a")).unwrap();
        std::fs::write(t.join("a/tool"), "not executable").unwrap();
        let path = std::env::join_paths([t.join("a"), t.join("b")]).unwrap();
        assert_eq!(which_in("tool", &path), Some(t.join("b/tool")));
        assert_eq!(which_in("missing", &path), None);
        assert_eq!(which_in("b/tool", &path), None, "a name with a slash is not looked up");
        assert_eq!(which_in("", &path), None);
        assert_eq!(which_in("tool", OsStr::new("relative")), None);
        let _ = std::fs::remove_dir_all(&t);
    }

    #[test]
    fn data_dirs_default_and_relative_entries() {
        let p = |s: &str| PathBuf::from(s);
        assert_eq!(data_dirs_from(None), vec![p("/usr/local/share"), p("/usr/share")]);
        assert_eq!(data_dirs_from(Some("".into())), vec![p("/usr/local/share"), p("/usr/share")]);
        assert_eq!(data_dirs_from(Some("/run/current-system/sw/share:rel:/x".into())), vec![p("/run/current-system/sw/share"), p("/x")]);
    }

    #[test]
    fn stable_self_path_prefers_env_then_profile_link() {
        let t = tmp("self");
        let store = t.join("store");
        let pkg = store.join("abc-tetron-wm-1/bin");
        exe(&pkg.join(".tetron-wm-wrapped"));
        exe(&pkg.join("tetron-wm"));
        let prof = t.join("profile/bin");
        std::fs::create_dir_all(&prof).unwrap();
        std::os::unix::fs::symlink(pkg.join("tetron-wm"), prof.join("tetron-wm")).unwrap();
        let other = t.join("other/bin");
        exe(&other.join("tetron-wm"));
        let running = pkg.join(".tetron-wm-wrapped");

        let path = std::env::join_paths([other.clone(), pkg.clone(), prof.clone()]).unwrap();
        let (p, s) = stable_self_path_from(None, &running, Some(&path), &store);
        assert_eq!((p, s), (prof.join("tetron-wm"), SelfPathSource::Profile), "skips a non-store tetron-wm and the store dir itself");

        let env = other.join("tetron-wm");
        let (p, s) = stable_self_path_from(Some(env.as_os_str()), &running, Some(&path), &store);
        assert_eq!((p, s), (env, SelfPathSource::Env));
        let (_, s) = stable_self_path_from(Some(OsStr::new("relative")), &running, Some(&path), &store);
        assert_eq!(s, SelfPathSource::Profile, "a relative TETRON_WM_EXE is ignored");

        let no_link = std::env::join_paths([other.clone()]).unwrap();
        let (p, s) = stable_self_path_from(None, &running, Some(&no_link), &store);
        assert_eq!((p, s), (pkg.join("tetron-wm"), SelfPathSource::StoreOnly), "the wrapper, never the wrapped binary");
        let _ = std::fs::remove_dir_all(&t);
    }

    #[test]
    fn stable_self_path_outside_a_store_is_the_running_binary() {
        let t = tmp("plain");
        let bin = t.join("bin/tetron-wm");
        exe(&bin);
        let (p, s) = stable_self_path_from(None, &bin, None, &t.join("store"));
        assert_eq!((p, s), (bin, SelfPathSource::Running));
        let _ = std::fs::remove_dir_all(&t);
    }
}
