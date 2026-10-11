# Changelog

All notable changes to Flint are recorded here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and Flint uses
[Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- The design: [docs/DESIGN.md](docs/DESIGN.md),
  [docs/PROTOCOL.md](docs/PROTOCOL.md) and
  [docs/FENIX-INTEGRATION.md](docs/FENIX-INTEGRATION.md).
- The Cargo workspace: nine crates with headers saying what each is for,
  `flint --version`, and [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md).
- CI on Windows and Linux through one script, `scripts/ci.ps1`: format,
  Clippy with warnings denied, tests, and a build. Workspace tests keep the
  architecture document, the OS-crate layering and the Fenix pins honest.
- Flint runs: `flint` starts a resident daemon per user (or shows the
  running one); `flint toggle`, `show [--query]`, `hide`, `status
  [--timings]`, `reload` and `quit` drive it from a terminal, a script or
  a desktop shortcut.
- The global hotkey, `Alt+Space` by default (`hotkey.toggle`), on Windows
  and X11. If another app owns the chord, Flint says so in its footer and
  in `flint status`, and `flint toggle` still works.
- KDE Plasma on Wayland: bind `flint toggle` to a shortcut, with the
  activation token passed on so Flint gets focus, and a KWin window rule in
  `contrib/kde/` ([docs/KDE.md](docs/KDE.md)).
- The window: a query line, a list (facts about the running Flint until
  search arrives in M2) and a one-line footer, in Fenix's Orbit Dark
  colours and monospace font. `Esc` hides it, and so does clicking away.
- `settings.toml` with `hotkey.toggle`, `appearance.width`,
  `appearance.rows` and `behaviour.hide_on_blur`; a bad value costs only
  that setting and is reported in one line
  ([docs/CONFIGURATION.md](docs/CONFIGURATION.md),
  [docs/KEYBINDINGS.md](docs/KEYBINDINGS.md)).
- The timing log: every show is logged with its time from request to first
  frame, and `flint status --timings` prints the last 100 with p50/p95.
- A software GPU fallback (WARP, llvmpipe), so Flint starts in VMs and RDP
  sessions without a usable GPU.
- Search: type to find installed apps (Start Menu shortcuts on Windows,
  `.desktop` files on Linux), quicklinks and Flint's own commands, with
  matched letters highlighted and `Enter` to launch or open. Results are
  ranked by match and by what you picked before for queries starting the
  same way, and don't move under the selection when new ones arrive.
- Fenix's Insert/Normal modes in the window: `Esc` goes to Normal mode
  (`j`/`k`, `gg`/`G`, `y` to copy, `q` to hide); `behaviour.escape_hides`
  makes one `Esc` hide instead.
- Scope keywords: `a ` for apps, `flint ` for Flint's commands, and each
  quicklink's keyword, shown as a chip.
- `[[quicklinks]]` in `settings.toml`: `j ABC-123` opens
  `https://jira.example.com/browse/ABC-123`.
- A keystroke benchmark over 100k candidates gates CI.
