"""tetron-wm installed from Nix (WMNIX-*): a NixOS system, a Nix profile or
home-manager. The binary lives in the read-only, versioned /nix/store and is
run through a wrapper that sets PATH; nothing is under /usr. These
requirements make tetron-wm find what it needs there, write no store path to
disk, and leave upgrades to Nix. Installing and removing apps through Nix is
not here: it belongs to tetron-appstore."""
from libspec import Requirement


class NixInstallMethod(Requirement):
    """REQUIREMENT-ID: WMNIX-001

    Depends on WMINST-005. tetron-wm knows it was installed by Nix when the
    running binary, after resolving symlinks, is under `/nix/store/`. That
    check comes first: the install method is then "nix" (Settings > About
    shows it), and the dpkg checks (package ownership, diversion, the size
    packages) are skipped and read as not packaged. Any other location keeps
    the WMINST-005 methods unchanged. Decided once per process, without a
    shell-out, like `running_is_packaged`.
    """


class NixNoSelfUpdate(Requirement):
    """REQUIREMENT-ID: WMNIX-002

    Depends on WMNIX-001 and WMDEB-005. With method "nix", tetron-wm never
    writes or replaces its own binary: Settings > Updates and the update
    action do not run install.sh or cargo; they show "tetron-wm is managed
    by Nix: update the system (nixos-rebuild switch) or the profile (nix
    profile upgrade)". The update check may still report that a newer
    release exists. The dev channel refuses with the same message. Reload
    still works (it restarts the same binary, it does not install).
    """


class StableSelfPath(Requirement):
    """REQUIREMENT-ID: WMNIX-003

    Any path to tetron-wm that is written to disk and used later (the
    apphost service unit or launchd plist, the mc clipboard commands in
    mc.ini, the command agents use to start `tetron-wm mcp`) is a stable
    self path, chosen in this order:
    1. `$TETRON_WM_EXE` when set to an absolute path of an executable file;
    2. when the running binary is under `/nix/store/`: the first
       `tetron-wm` on PATH that is outside `/nix/store/` and resolves to a
       tetron-wm in the store (a profile link such as
       `/run/current-system/sw/bin`, `/etc/profiles/per-user/<user>/bin` or
       `~/.nix-profile/bin`), so the written path survives upgrades and
       garbage collection;
    3. else the running binary (today's behaviour, including the
       "(deleted)" handling of `self_exe`).
    A wrapped binary is never written: when the running file is named
    `.tetron-wm-wrapped` (the Nix wrapper's inner binary, which lacks the
    wrapper's PATH), its sibling `tetron-wm` is used in step 3. If step 2
    finds nothing, step 3 applies and the debug log says the written path
    will break after a Nix upgrade. Spawning a child process now (daemon,
    apphost, shims) may keep using the running binary.
    """


class HelpersOnPath(Requirement):
    """REQUIREMENT-ID: WMNIX-004

    Helper programs tetron-wm checks for or runs are found on PATH, not at a
    fixed /usr path. `tetronos-ssh-temp` (Launcher > Remote Access and
    Settings) is available when it is on PATH; the status file
    `/run/tetronos/ssh-temp.status` is unchanged (the helper writes it on
    every system). Remote Access runs the helper under sudo by the full
    path PATH gave, since sudo may reset PATH. The apps-dialog check for apt (`/usr/bin/apt-get`) stays
    as it is, but with method "nix" the dialog offers no install or removal
    and says the apps are managed by Nix.
    """


class DataFromXdgDataDirs(Requirement):
    """REQUIREMENT-ID: WMNIX-005

    Depends on WMDEB-002. Shared data is looked up in each `$XDG_DATA_DIRS`
    entry (default `/usr/local/share:/usr/share`), as the themes already
    are:
    - console fonts (screenshots, the "consolefonts" app check):
      `<dir>/consolefonts` for each entry, then `/usr/lib/kbd/consolefonts`;
    - font fallbacks when fc-list gives no answer (Nerd Font symbols,
      DejaVu Sans Mono): `<dir>/fonts` searched for the same file prefixes.
    A Nix package that ships console fonts puts their share directory on
    `XDG_DATA_DIRS` (wrapper or system profile).
    """


class PackagedUnitAnyUnitDir(Requirement):
    """REQUIREMENT-ID: WMNIX-006

    The packaged apphost user unit (tetron-wm-apphost.service, installed for
    every user by a package or a system configuration) is found in any
    systemd user unit directory a package or system can use:
    `/etc/systemd/user` (where NixOS writes user units from its
    configuration), `/usr/lib/systemd/user`, and `<dir>/systemd/user` for
    each `$XDG_DATA_DIRS` entry. Whether it is enabled is decided as today.
    No shell-out to systemctl for the lookup.
    """


class NixLiveSession(Requirement):
    """REQUIREMENT-ID: WMNIX-007

    The live-session check (free space warning before installing apps)
    also knows a NixOS live ISO: `/iso` mounted and `/nix/.ro-store`
    present, with `/` or `/nix/store` on an overlay or tmpfs.
    """


class NoNewUsrPaths(Requirement):
    """REQUIREMENT-ID: WMNIX-008

    Fixed /usr, /var/lib/dpkg and /sbin paths in non-test code are limited
    to an allowlist: the dpkg and apt support (WMDEB-005, WMINST-005), the
    defaults of the XDG lists (WMDEB-002, WMNIX-005) and the fallbacks
    named in WMNIX-005 and WMNIX-006. reconcile.py fails on a new one
    outside the allowlist, so new code uses PATH or XDG lookups.
    """
