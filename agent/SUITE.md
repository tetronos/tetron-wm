
# The tetron suite

tetron-wm is one part of the tetron suite, published by the tetronos GitHub
organisation (https://github.com/tetronos) and packaged in the tetronos apt
repository (https://tetronos.github.io/apt/). These are the shipped parts:

| Part | What it is |
|---|---|
| tetron | P2P mesh VPN daemon and CLI; the base the other tetron parts talk to |
| tetron-wm | this desktop: window manager for the terminal (daemon, client, app server) |
| tetron-browser | text-mode web browser (maintained carbonyl continuation); Start Page uses its bookmarks.html |
| tetron-tui | terminal UI for the tetron VPN |
| tetron-webui | browser dashboard and admin console for the tetron VPN |
| tetron-systray | tray status and quick actions for the tetron VPN |
| tetron-messageboard | message board hosted on one tetron mesh |
| tetron-hosts | syncs tetron peer names into /etc/hosts |
| tetron-sync-receiver | photo backup destination for the tetron-mobile-sync Android app |
| tetron-veilid | installs a veilid-server companion daemon for tetron |
| tetron-os | TetronOS: the Debian-based OS images that ship all of the above |
| casttui | Chromecast caster TUI used by Files > Cast |

Each repository is `https://github.com/tetronos/<part>`.

## On this machine

tetron-wm is installed as: {{INSTALL}}.

{{SUITE}}
