//! Filesystem listing and mutation behind a testable `FsOps` trait. The real
//! `StdFs` impl talks to disk; the file manager only ever calls through the
//! trait, so its logic is unit-testable and disk I/O is isolated here.

use crate::openwith::{classify, Role};
use std::io;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// A single directory entry the file manager displays.
#[derive(Clone, Debug)]
pub struct Entry {
    pub name: String,
    pub path: PathBuf,
    pub is_dir: bool,
    pub size: u64,
    pub modified: Option<SystemTime>,
    pub role: Role,
}

/// All filesystem effects the file manager needs, behind a trait so the UI is
/// testable with a fake and the real impl is the only thing touching disk.
pub trait FsOps {
    /// List `dir`, directories first then files, each group sorted case-insensitively
    /// by name. Hidden (dot) entries are included only when `show_hidden`.
    fn list(&self, dir: &Path, show_hidden: bool) -> io::Result<Vec<Entry>>;
    /// Create `name` under `parent`; returns the new path.
    fn mkdir(&self, parent: &Path, name: &str) -> io::Result<PathBuf>;
    /// Rename `path` to `new_name` (same parent); returns the new path.
    fn rename(&self, path: &Path, new_name: &str) -> io::Result<PathBuf>;
    /// Copy `src` into `dst_dir` (recursive for directories), de-duping the name.
    fn copy(&self, src: &Path, dst_dir: &Path) -> io::Result<PathBuf>;
    /// Move `src` into `dst_dir` (rename, falling back to copy+remove across devices).
    fn move_to(&self, src: &Path, dst_dir: &Path) -> io::Result<PathBuf>;
    /// Move `path` to the OS Trash. Never hard-deletes.
    fn trash(&self, path: &Path) -> io::Result<()>;
    /// Set Unix permission bits on `path` (no-op on non-unix).
    fn set_mode(&self, path: &Path, mode: u32) -> io::Result<()>;
    /// Whether paths are on this machine, so local tools (tar, 7z) can run on them.
    fn is_local(&self) -> bool {
        true
    }
}

/// The production filesystem implementation.
pub struct StdFs;

impl FsOps for StdFs {
    fn list(&self, dir: &Path, show_hidden: bool) -> io::Result<Vec<Entry>> {
        let mut out = Vec::new();
        let read = std::fs::read_dir(dir).map_err(|e| {
            // A missing folder (no ~/Desktop on a fresh user) is an empty
            // view, not news: debug only. Anything else is a warning.
            let msg = format!("fileops::list {}: {}", dir.display(), e);
            if e.kind() == io::ErrorKind::NotFound { crate::log_debug(&msg) } else { crate::log_warn(&msg) }
            e
        })?;
        for ent in read {
            let ent = ent?;
            let name = ent.file_name().to_string_lossy().to_string();
            if !show_hidden && name.starts_with('.') {
                continue;
            }
            let meta = ent.metadata().ok();
            let is_dir = meta.as_ref().map(|m| m.is_dir()).unwrap_or(false);
            let size = meta.as_ref().map(|m| m.len()).unwrap_or(0);
            let modified = meta.as_ref().and_then(|m| m.modified().ok());
            let path = ent.path();
            let role = classify(&path, is_dir);
            out.push(Entry { name, path, is_dir, size, modified, role });
        }
        out.sort_by(|a, b| {
            b.is_dir
                .cmp(&a.is_dir) // dirs first (true > false)
                .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
        });
        Ok(out)
    }

    fn mkdir(&self, parent: &Path, name: &str) -> io::Result<PathBuf> {
        let p = parent.join(name);
        std::fs::create_dir(&p)?;
        Ok(p)
    }

    fn rename(&self, path: &Path, new_name: &str) -> io::Result<PathBuf> {
        let parent = path.parent().unwrap_or(Path::new("."));
        let dst = parent.join(new_name);
        std::fs::rename(path, &dst)?;
        Ok(dst)
    }

    fn copy(&self, src: &Path, dst_dir: &Path) -> io::Result<PathBuf> {
        let name = src
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("item")
            .to_string();
        let dst = unique_destination(dst_dir, &name);
        copy_recursive(src, &dst)?;
        Ok(dst)
    }

    fn move_to(&self, src: &Path, dst_dir: &Path) -> io::Result<PathBuf> {
        let name = src
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("item")
            .to_string();
        let dst = unique_destination(dst_dir, &name);
        match std::fs::rename(src, &dst) {
            Ok(()) => Ok(dst),
            Err(_) => {
                // Cross-device: copy then remove the source.
                copy_recursive(src, &dst)?;
                if src.is_dir() {
                    std::fs::remove_dir_all(src)?;
                } else {
                    std::fs::remove_file(src)?;
                }
                Ok(dst)
            }
        }
    }

    fn trash(&self, path: &Path) -> io::Result<()> {
        let dir = trash_dir().ok_or_else(|| io::Error::other("no trash directory"))?;
        std::fs::create_dir_all(&dir)?;
        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("item")
            .to_string();
        let dst = unique_destination(&dir, &name);
        match std::fs::rename(path, &dst) {
            Ok(()) => Ok(()),
            Err(_) => {
                copy_recursive(path, &dst)?;
                if path.is_dir() {
                    std::fs::remove_dir_all(path)?;
                } else {
                    std::fs::remove_file(path)?;
                }
                Ok(())
            }
        }
    }

    fn set_mode(&self, path: &Path, mode: u32) -> io::Result<()> {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode))?;
        }
        #[cfg(not(unix))]
        {
            let _ = (path, mode);
        }
        Ok(())
    }
}

/// Recursively copy a file or directory tree from `src` to `dst`.
fn copy_recursive(src: &Path, dst: &Path) -> io::Result<()> {
    if src.is_dir() {
        std::fs::create_dir_all(dst)?;
        for ent in std::fs::read_dir(src)? {
            let ent = ent?;
            copy_recursive(&ent.path(), &dst.join(ent.file_name()))?;
        }
        Ok(())
    } else {
        if let Some(parent) = dst.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::copy(src, dst).map(|_| ())
    }
}

/// A non-colliding destination path for `name` inside `dir`: `name`, then
/// `name copy`, `name copy 2`, … (preserving the extension for files).
pub fn unique_destination(dir: &Path, name: &str) -> PathBuf {
    let candidate = dir.join(name);
    if !candidate.exists() {
        return candidate;
    }
    let (stem, ext) = split_name(name);
    for n in 1..1_000_000 {
        let suffix = if n == 1 { " copy".to_string() } else { format!(" copy {n}") };
        let trial = if ext.is_empty() {
            format!("{stem}{suffix}")
        } else {
            format!("{stem}{suffix}.{ext}")
        };
        let p = dir.join(trial);
        if !p.exists() {
            return p;
        }
    }
    // Astronomically unlikely fallback: append the process id to disambiguate.
    dir.join(format!("{stem}-{}", std::process::id()))
}

/// Split a filename into (stem, extension-without-dot). Dotfiles with no further
/// extension keep their whole name as the stem.
fn split_name(name: &str) -> (String, String) {
    match name.rsplit_once('.') {
        Some((stem, ext)) if !stem.is_empty() => (stem.to_string(), ext.to_string()),
        _ => (name.to_string(), String::new()),
    }
}

/// Files copied or cut on this machine, shared by the desktop and every local
/// file manager window (one per daemon), so a copy in one pastes in another.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileClip {
    pub paths: Vec<PathBuf>,
    pub cut: bool,
}

static FILE_CLIP: std::sync::Mutex<Option<FileClip>> = std::sync::Mutex::new(None);

/// Remember `paths` for Paste (`cut`: Paste moves them, once).
pub fn clip_set(paths: Vec<PathBuf>, cut: bool) {
    if paths.is_empty() {
        return;
    }
    *FILE_CLIP.lock().unwrap_or_else(|e| e.into_inner()) = Some(FileClip { paths, cut });
}

/// What Paste would paste, if anything.
pub fn clip_get() -> Option<FileClip> {
    FILE_CLIP.lock().unwrap_or_else(|e| e.into_inner()).clone()
}

/// Paste `clip` into `dir` through `fs`: a copy, or a move for a cut. Name
/// clashes get " copy", " copy 2", ... ([`unique_destination`]); a cut into
/// the folder it came from is left alone; a folder is never pasted into
/// itself. Returns how many were pasted and the first error, if any.
pub fn paste_into(fs: &dyn FsOps, clip: &FileClip, dir: &Path) -> (usize, Option<String>) {
    let mut done = 0;
    let mut err = None;
    for src in &clip.paths {
        if clip.cut && src.parent() == Some(dir) {
            continue;
        }
        if dir.starts_with(src) {
            err.get_or_insert_with(|| format!("can not paste {} into itself", src.display()));
            continue;
        }
        let r = if clip.cut { fs.move_to(src, dir) } else { fs.copy(src, dir) };
        match r {
            Ok(_) => done += 1,
            Err(e) => {
                err.get_or_insert_with(|| e.to_string());
            }
        }
    }
    (done, err)
}

/// Paste the shared clipboard into `dir` on this machine; a cut is used up.
pub fn paste_clip_into(dir: &Path) -> (usize, Option<String>) {
    let Some(clip) = clip_get() else { return (0, None) };
    let r = paste_into(&StdFs, &clip, dir);
    if clip.cut {
        *FILE_CLIP.lock().unwrap_or_else(|e| e.into_inner()) = None;
    }
    r
}

/// How many items the Trash holds (0 without one).
pub fn trash_count() -> usize {
    trash_dir().and_then(|d| std::fs::read_dir(d).ok()).map_or(0, |rd| rd.flatten().count())
}

/// Delete everything in the Trash for good: its `files` and, on Linux, the
/// matching `info` records (freedesktop Trash). Returns how many items went.
pub fn empty_trash() -> io::Result<usize> {
    let Some(dir) = trash_dir() else { return Ok(0) };
    let n = empty_dir(&dir)?;
    if let Some(info) = dir.parent().map(|t| t.join("info")).filter(|i| !cfg!(target_os = "macos") && i.is_dir()) {
        empty_dir(&info)?;
    }
    Ok(n)
}

/// Remove every entry of `dir`, keeping `dir`. Symlinks are removed, never followed.
fn empty_dir(dir: &Path) -> io::Result<usize> {
    let mut n = 0;
    let Ok(rd) = std::fs::read_dir(dir) else { return Ok(0) };
    for e in rd.flatten() {
        let p = e.path();
        if e.file_type().is_ok_and(|t| t.is_dir()) {
            std::fs::remove_dir_all(&p)?;
        } else {
            std::fs::remove_file(&p)?;
        }
        n += 1;
    }
    Ok(n)
}

/// The OS Trash directory for moved-not-deleted files.
pub fn trash_dir() -> Option<PathBuf> {
    let home = dirs::home_dir()?;
    if cfg!(target_os = "macos") {
        Some(home.join(".Trash"))
    } else {
        Some(home.join(".local/share/Trash/files"))
    }
}

/// Detailed metadata for the Get-Info panel.
#[derive(Clone, Debug)]
pub struct FileInfo {
    pub path: PathBuf,
    pub size: u64,
    pub modified: Option<SystemTime>,
    pub is_dir: bool,
    pub is_symlink: bool,
    pub mode: u32,
    pub link_target: Option<PathBuf>,
}

/// Gather metadata for `path` (following the link for size/mode, but recording
/// whether the path itself is a symlink and where it points).
pub fn info(path: &Path) -> io::Result<FileInfo> {
    let lmeta = std::fs::symlink_metadata(path)?;
    let is_symlink = lmeta.file_type().is_symlink();
    let link_target = if is_symlink { std::fs::read_link(path).ok() } else { None };
    // Follow for the real size/mode where possible; fall back to the link's own.
    let meta = std::fs::metadata(path).unwrap_or(lmeta);
    let mode = mode_of(&meta);
    Ok(FileInfo {
        path: path.to_path_buf(),
        size: meta.len(),
        modified: meta.modified().ok(),
        is_dir: meta.is_dir(),
        is_symlink,
        mode,
        link_target,
    })
}

#[cfg(unix)]
fn mode_of(meta: &std::fs::Metadata) -> u32 {
    use std::os::unix::fs::PermissionsExt;
    meta.permissions().mode()
}
#[cfg(not(unix))]
fn mode_of(_meta: &std::fs::Metadata) -> u32 {
    0
}

/// Render the low 9 mode bits as `rwxr-xr-x`.
pub fn mode_rwx(mode: u32) -> String {
    let bit = |shift: u32, ch: char| if mode & (1 << shift) != 0 { ch } else { '-' };
    let mut s = String::with_capacity(9);
    for (r, w, x) in [(8, 7, 6), (5, 4, 3), (2, 1, 0)] {
        s.push(bit(r, 'r'));
        s.push(bit(w, 'w'));
        s.push(bit(x, 'x'));
    }
    s
}

// ── Remote filesystem over ssh (the Systems feature's file browser) ─────────────

/// Forward `FsOps` through a boxed backend so one `FileManager` type can browse
/// either the local disk (`StdFs`) or a saved remote system (`SshFs`).
impl FsOps for Box<dyn FsOps + Send> {
    fn list(&self, dir: &Path, show_hidden: bool) -> io::Result<Vec<Entry>> {
        (**self).list(dir, show_hidden)
    }
    fn mkdir(&self, parent: &Path, name: &str) -> io::Result<PathBuf> {
        (**self).mkdir(parent, name)
    }
    fn rename(&self, path: &Path, new_name: &str) -> io::Result<PathBuf> {
        (**self).rename(path, new_name)
    }
    fn copy(&self, src: &Path, dst_dir: &Path) -> io::Result<PathBuf> {
        (**self).copy(src, dst_dir)
    }
    fn move_to(&self, src: &Path, dst_dir: &Path) -> io::Result<PathBuf> {
        (**self).move_to(src, dst_dir)
    }
    fn trash(&self, path: &Path) -> io::Result<()> {
        (**self).trash(path)
    }
    fn set_mode(&self, path: &Path, mode: u32) -> io::Result<()> {
        (**self).set_mode(path, mode)
    }
}

/// `FsOps` on a remote machine, via non-interactive ssh commands (`BatchMode`,
/// so a system without key auth fails fast instead of hanging on a prompt).
/// Every call is timeout-guarded by [`crate::system::run_capped`].
pub struct SshFs {
    /// `user@host` (or bare host).
    pub target: String,
    pub port: Option<u16>,
}

impl SshFs {
    pub fn new(target: String, port: Option<u16>) -> Self {
        SshFs { target, port }
    }

    /// Run `cmd` through `sh -c` on the remote; `None` on failure/timeout.
    ///
    /// NOTE: these calls currently run synchronously on the daemon's render
    /// loop (via `FileManager` inside `core.apply`), so an unreachable remote
    /// briefly freezes the desktop — bounded by `ConnectTimeout` (3s) plus the
    /// per-op `secs` budget. The frequent navigation ops (list/home) use short
    /// budgets to keep that hitch small; copy/move keep a generous budget for
    /// large transfers. The proper fix is to move remote FsOps off the render
    /// loop (like the scp paste already is) — tracked as a follow-up.
    fn ssh(&self, cmd: &str, secs: u64) -> Option<String> {
        let port = self.port.map(|p| p.to_string());
        let mut args: Vec<&str> = vec!["-o", "BatchMode=yes", "-o", "ConnectTimeout=3"];
        if let Some(p) = port.as_deref() {
            args.extend(["-p", p]);
        }
        args.push(&self.target);
        args.push(cmd);
        let out = crate::system::run_capped("ssh", &args, secs);
        if out.is_none() {
            crate::dbg_log(&format!("sshfs {}: FAILED: {}", self.target, cmd));
        }
        out
    }

    /// The remote home directory (the browser's starting point), or `None`
    /// when the system is unreachable / key auth is not set up.
    pub fn remote_home(&self) -> Option<PathBuf> {
        let home = self.ssh("pwd", 5)?;
        let home = home.trim();
        (!home.is_empty()).then(|| PathBuf::from(home))
    }

    fn q(p: &Path) -> String {
        crate::systems::sh_quote(&p.to_string_lossy())
    }

    fn err(what: &str) -> io::Error {
        io::Error::other(format!("ssh {what} failed (is the system online / key installed?)"))
    }
}

impl FsOps for SshFs {
    fn is_local(&self) -> bool {
        false
    }

    fn list(&self, dir: &Path, show_hidden: bool) -> io::Result<Vec<Entry>> {
        let cmd = format!("cd {} && LC_ALL=C ls -lA", Self::q(dir));
        let out = self.ssh(&cmd, 5).ok_or_else(|| Self::err("list"))?;
        let mut entries = Vec::new();
        for line in out.lines() {
            let Some((kind, size, name)) = parse_ls_line(line) else { continue };
            if !show_hidden && name.starts_with('.') {
                continue;
            }
            let is_dir = kind == 'd';
            let path = dir.join(&name);
            let role = classify(&path, is_dir);
            entries.push(Entry { name, path, is_dir, size, modified: None, role });
        }
        entries.sort_by(|a, b| {
            b.is_dir
                .cmp(&a.is_dir)
                .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
        });
        Ok(entries)
    }

    fn mkdir(&self, parent: &Path, name: &str) -> io::Result<PathBuf> {
        let p = parent.join(name);
        self.ssh(&format!("mkdir {}", Self::q(&p)), 8).ok_or_else(|| Self::err("mkdir"))?;
        Ok(p)
    }

    fn rename(&self, path: &Path, new_name: &str) -> io::Result<PathBuf> {
        let dst = path.parent().unwrap_or(Path::new("/")).join(new_name);
        self.ssh(&format!("mv {} {}", Self::q(path), Self::q(&dst)), 8)
            .ok_or_else(|| Self::err("rename"))?;
        Ok(dst)
    }

    fn copy(&self, src: &Path, dst_dir: &Path) -> io::Result<PathBuf> {
        self.ssh(&format!("cp -r {} {}/", Self::q(src), Self::q(dst_dir)), 20)
            .ok_or_else(|| Self::err("copy"))?;
        Ok(dst_dir.join(src.file_name().unwrap_or_default()))
    }

    fn move_to(&self, src: &Path, dst_dir: &Path) -> io::Result<PathBuf> {
        self.ssh(&format!("mv {} {}/", Self::q(src), Self::q(dst_dir)), 20)
            .ok_or_else(|| Self::err("move"))?;
        Ok(dst_dir.join(src.file_name().unwrap_or_default()))
    }

    fn trash(&self, path: &Path) -> io::Result<()> {
        // Move to the remote's trash (mac ~/.Trash, else XDG) — never a hard rm.
        let cmd = format!(
            "if [ -d \"$HOME/.Trash\" ]; then mv {p} \"$HOME/.Trash/\"; \
else mkdir -p \"$HOME/.local/share/Trash/files\" && mv {p} \"$HOME/.local/share/Trash/files/\"; fi",
            p = Self::q(path),
        );
        self.ssh(&cmd, 10).ok_or_else(|| Self::err("trash"))?;
        Ok(())
    }

    fn set_mode(&self, path: &Path, mode: u32) -> io::Result<()> {
        self.ssh(&format!("chmod {:o} {}", mode & 0o7777, Self::q(path)), 8)
            .ok_or_else(|| Self::err("chmod"))?;
        Ok(())
    }
}

/// Parse one `LC_ALL=C ls -lA` line into (kind char, size, name). Skips the
/// `total` header and anything that doesn't look like a listing row. Symlink
/// names keep only the link side of `name -> target`.
fn parse_ls_line(line: &str) -> Option<(char, u64, String)> {
    let kind = line.chars().next()?;
    if !matches!(kind, 'd' | '-' | 'l' | 'c' | 'b' | 'p' | 's') {
        return None; // "total 12" or noise
    }
    // perms links owner group size month day time NAME (name may hold spaces).
    let mut rest = line;
    let mut fields: Vec<&str> = Vec::new();
    for _ in 0..8 {
        rest = rest.trim_start();
        let idx = rest.find(char::is_whitespace)?;
        fields.push(&rest[..idx]);
        rest = &rest[idx..];
    }
    let size: u64 = fields[4].parse().unwrap_or(0);
    let mut name = rest.trim_start().to_string();
    if kind == 'l' {
        if let Some(i) = name.find(" -> ") {
            name.truncate(i);
        }
    }
    (!name.is_empty()).then_some((kind, size, name))
}

#[cfg(test)]
mod ssh_tests {
    use super::*;

    fn scratch(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("tetron-wm-paste-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn paste_copies_with_clash_names_and_moves_a_cut() {
        let d = scratch("clash");
        let (a, b) = (d.join("a"), d.join("b"));
        std::fs::create_dir_all(&a).unwrap();
        std::fs::create_dir_all(&b).unwrap();
        std::fs::write(a.join("f.txt"), "x").unwrap();
        std::fs::write(b.join("f.txt"), "old").unwrap();
        let copy = FileClip { paths: vec![a.join("f.txt")], cut: false };
        assert_eq!(paste_into(&StdFs, &copy, &b), (1, None));
        assert_eq!(paste_into(&StdFs, &copy, &b), (1, None));
        assert_eq!(std::fs::read_to_string(b.join("f.txt")).unwrap(), "old", "never overwrites");
        assert!(b.join("f copy.txt").exists() && b.join("f copy 2.txt").exists());
        let cut = FileClip { paths: vec![a.join("f.txt")], cut: true };
        assert_eq!(paste_into(&StdFs, &cut, &a), (0, None), "a cut into its own folder stays as it is");
        assert!(a.join("f.txt").exists() && !a.join("f copy.txt").exists());
        assert_eq!(paste_into(&StdFs, &cut, &b), (1, None));
        assert!(!a.join("f.txt").exists() && b.join("f copy 3.txt").exists());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn a_folder_is_never_pasted_into_itself() {
        let d = scratch("self");
        let a = d.join("a");
        std::fs::create_dir_all(a.join("inner")).unwrap();
        let clip = FileClip { paths: vec![a.clone()], cut: false };
        let (done, err) = paste_into(&StdFs, &clip, &a.join("inner"));
        assert_eq!(done, 0);
        assert!(err.unwrap().contains("into itself"));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn emptying_removes_files_folders_and_links_but_not_their_targets() {
        let d = scratch("empty");
        let (t, keep) = (d.join("trash"), d.join("keep"));
        std::fs::create_dir_all(t.join("dir/sub")).unwrap();
        std::fs::create_dir_all(&keep).unwrap();
        std::fs::write(t.join("f"), "x").unwrap();
        std::fs::write(keep.join("k"), "k").unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink(&keep, t.join("link")).unwrap();
        let n = empty_dir(&t).unwrap();
        assert_eq!(n, if cfg!(unix) { 3 } else { 2 });
        assert!(t.is_dir() && std::fs::read_dir(&t).unwrap().next().is_none());
        assert!(keep.join("k").exists(), "a link's target is kept");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn ls_lines_parse_kinds_sizes_and_spaced_names() {
        assert_eq!(parse_ls_line("total 12"), None);
        let (k, sz, name) = parse_ls_line("drwxr-xr-x  2 user group 4096 Jun 10 10:00 My Documents").unwrap();
        assert_eq!((k, sz, name.as_str()), ('d', 4096, "My Documents"));
        let (k, sz, name) = parse_ls_line("-rw-r--r--  1 user group  220 Jun 10 10:00 notes.txt").unwrap();
        assert_eq!((k, sz, name.as_str()), ('-', 220, "notes.txt"));
        let (k, _, name) = parse_ls_line("lrwxrwxrwx  1 user group    7 Jun 10 10:00 link -> /target").unwrap();
        assert_eq!((k, name.as_str()), ('l', "link"));
    }
}
