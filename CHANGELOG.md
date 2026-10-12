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
- Fenix's projects (`p `, pinned first) and recent files (`f `, most
  recent first) in the results: `Enter` opens a project's folder, and
  opens a recent file in Fenix through its existing hand-off.
- Flint follows Fenix's theme and font (all eight of Fenix's themes),
  picking up a change the next time it is shown; `appearance.theme` pins
  one instead.
- `fenix.executable` for a Fenix that isn't on the `PATH`.
- The action menu: `Tab` (or `.` in Normal mode) lists what the selected
  result can do, each with its key (`o` open, `e` open in Fenix, `f` show
  in folder, `t` terminal here, `c` copy). Actions Flint can't run yet are
  dimmed and say why.
- The leader: `SPC` in Normal mode opens a which-key list over the
  `[leader]` map from `settings.toml` (Fenix's key trie, Fenix's grammar),
  with defaults for apps, projects, recent files, Flint's commands,
  reload and settings.
- "Show in folder" selects the file in Explorer, or in the file manager on
  Linux.
- External providers: `[[providers]]` in `settings.toml` runs a program
  that adds items and actions to Flint over the provider protocol, either
  started by Flint (`command`) or connecting to it (`connect = true`). A
  provider that crashes or misbehaves is restarted, then disabled with
  its reason in `flint status`, without slowing Flint down; each has its
  own log. How to write one, with examples in Python and Rust:
  [docs/PROVIDERS.md](docs/PROVIDERS.md).
- The windows provider (Windows): `w ` lists the open windows, the one
  you were in first; `Enter` switches to one, `Tab` `x` closes it. `SPC w`
  in Normal mode. Linux says it can't list windows yet.
- The calculator: a sum (`2 * 21`, `sqrt(2)/2`, `0xff + 1`) is answered
  above everything else as you type, or with `= `; `Enter` copies it.
- The shell prefix: `> ` runs what follows with your shell
  (`shell.program`), in a terminal or hidden with how it ended in the
  footer.
- `flint autostart on|off`: start Flint, hidden, when you log in (the
  `Run` key on Windows, an autostart entry on Linux); `flint autostart`
  says whether it does.
- Fenix level 1: with Fenix running, Flint asks it over Fenix's control
  channel what it can do and enables each Fenix action accordingly (a
  dimmed action says why). `Enter` on a project opens it in Fenix, as
  `SPC p p` would; a typed path like `C:\src\main.rs:12:3` opens there;
  Fenix comes to the front reliably. When Fenix is the window you came
  from, providers get its project and file as context. With no Fenix
  running, Flint starts one with the request (`--project`, `--goto`).
  `flint status` says which level and what Fenix offers.

### Fixed

- The `Space` of `Alt+Space` is no longer typed into the query when Flint
  appears (keys already held when the window takes focus are ignored), so
  a scope keyword typed straight after the hotkey (`b `) scopes the query
  instead of searching for ` b `.
