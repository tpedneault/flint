# Flint — design

Flint is a keyboard-first launcher for Windows and Linux. One hotkey from
anywhere, a few characters, and an action: open an app, a file, a project
in Fenix, a Jira issue, an internal URL, a command another tool registered.
It is the front door to Fenix and to every tool built after it.

This document is the design the first implementation follows. The wire
protocol is in [PROTOCOL.md](PROTOCOL.md); what Flint needs from Fenix,
and what Fenix has to add, is in [FENIX-INTEGRATION.md](FENIX-INTEGRATION.md).

Status: M2 implemented (see [ARCHITECTURE.md](ARCHITECTURE.md)). Pinned against Fenix
`f05780740b7d45e41eedc35b846dba7c832f4d49` (2026-10-11).

## Why the name

A flint is what you strike to get the spark that lights the fire. Fenix is
the fire. The launcher is small, hard, and does one thing on contact.

## Goals

1. **Fast enough to be a reflex.** Visible within 50 ms of the hotkey;
   results updated within one frame (16 ms) of a keystroke for local
   providers. These are requirements, measured, not aspirations.
2. **One grammar with Fenix.** The same Insert/Normal split, the same `SPC`
   leader with which-key, the same theme, the same settings conventions.
   Someone fluent in Fenix is fluent in Flint on day one.
3. **The suite's front door.** Other tools contribute items and actions
   through a small, documented protocol and get search, ranking, keys and
   UI for free.
4. **Works on a locked-down corporate machine.** No admin rights, no
   installer, no cloud service, single binary, honours the system proxy
   and the corporate CA store.
5. **No tool requires another.** Flint works with Fenix missing, closed or
   out of date, and Fenix never requires Flint. Each integration degrades
   to something sensible and says so.

## Non-goals

- Editing anything. Flint finds and dispatches; heavier work is handed to
  a tool that does it (Fenix, the browser, the file manager). If a feature
  needs a text area larger than the query line, it belongs elsewhere.
- A plugin marketplace, plugin auto-discovery, or in-process plugins.
  Providers are separate processes the user declares in config.
- macOS. Thomas has a Mac, but Raycast and Alfred exist there and the
  platform layer is the expensive part. Keep platform code behind a trait
  so a port stays possible; don't build it now.
- Replacing the OS shell, the window manager or the Start menu.

## The experience

**Summon.** The hotkey (default `Alt+Space`, configurable) shows a centred
window, query line focused, in Insert mode. If the window is already
visible, the hotkey hides it. Losing focus hides it.

**Type.** One list mixes results from every provider, ranked by match
quality and by frecency (how often and how recently you chose this item for
a query starting this way). Results from slow providers append below what
is already visible; the list never reorders under the cursor once shown
(see [Ranking](#ranking)).

**Prefixes.** A keyword followed by a space scopes the query to one
provider: `j ABC-123`, `p flight`, `w outlook`, `= 3*1024`, `> cargo test`.
Keywords come from config and from provider registrations; the active
scope is shown as a chip at the start of the query line, and `Backspace` on
an empty query removes it.

**Modes.**

| Mode | Keys |
|---|---|
| Insert | typing edits the query; `Enter` primary action; `Tab` action menu; `Ctrl-n`/`Ctrl-p` or `Down`/`Up` move; `Ctrl-w` delete word; `Ctrl-u` clear; `Esc` to Normal |
| Normal | `j`/`k` move, `gg`/`G` first/last, `i`/`a`/`/` back to Insert, `Enter` primary, `Tab` or `.` action menu, `y` copy the item's primary text, `SPC` leader, `Esc` or `q` hide |

`Esc` from Insert goes to Normal rather than hiding, because that is what
the hands of a Vim user expect. `flint.escape_hides = true` makes a single
`Esc` hide from either mode, for people who prefer Raycast's behaviour.

**Action menu.** `Tab` opens the selected item's actions in a small list
(open in Fenix / reveal in file manager / copy path / open terminal here).
The first action is the `Enter` action. Each action may carry a one-key
accelerator shown in the menu.

**Leader.** `SPC` in Normal mode opens the which-key popup over a
`fenix-keymap` trie built from config, e.g. `SPC p` → projects scope,
`SPC f r` → Fenix recent files, `SPC j` → Jira scope. It shows the same
hints Fenix's popup shows.

**Context.** Flint records the foreground window (process, title) before
it shows itself. When that window is Fenix and Fenix supports the control
protocol, Flint asks it for the current project and file, and
project-scoped results rank first. Context is passed to every provider in
each query.

**Feedback.** Errors and confirmations appear in a one-line footer (the
same role as Fenix's status line), never as dialogs. An action that fails
leaves the window open with the reason.

## Architecture

```
            hotkey (Windows)          `flint toggle` (KDE shortcut, scripts)
                 │                                  │
                 ▼                                  ▼
  ┌───────────────────────────────────────────────────────────────┐
  │ flint daemon (one per user)                                   │
  │  flint-gui ─ winit + wgpu + glyphon; window pre-created,      │
  │              shown/hidden, never destroyed                    │
  │  flint-index ─ candidates, nucleo matching, frecency          │
  │  providers (in-process): apps, quicklinks, Fenix state, Flint │
  │  provider host ─ JSON-RPC (PROTOCOL.md) to external providers │
  │  control endpoint ─ per-user named pipe / Unix socket         │
  └───────┬─────────────────────────────┬─────────────────────────┘
          │ stdio (spawned)             │ local socket (they connect)
          ▼                             ▼
   jira, gitlab, your scripts     long-running tools (Fenix, later)

          flint-fenix ──── Fenix control protocol (FENIX-INTEGRATION.md)
                     └──── Fenix's state files, read-only (fenix-storage)
```

### One resident process per user

`flint` with no arguments starts the daemon if none is running for this
user, otherwise asks the running one to show itself and exits. The daemon
creates its window and GPU surface at startup, hidden, and never destroys
them: showing is a `set_visible(true)`, a redraw and a focus request.
Startup cost (GPU init, font loading, provider indexing) is paid once,
behind the user's back, at login.

The daemon owns a **control endpoint**: a named pipe on Windows, a Unix
domain socket on Linux, private to the user (see
[Security](#security)). The same endpoint serves:

- the CLI client (`flint toggle`, `flint show --query "j "`, `flint quit`,
  `flint status`, `flint reload`);
- external providers that are long-running applications and connect in
  rather than being spawned.

Single-instance negotiation is "try to connect; if nothing answers, bind";
binding uses `FILE_FLAG_FIRST_PIPE_INSTANCE` on Windows so a second daemon
cannot squat the name. Do **not** copy Fenix's fixed loopback TCP port —
[FENIX-INTEGRATION.md](FENIX-INTEGRATION.md#f2) explains why it is wrong
on shared machines.

### Workspace layout

Same shape as Fenix: small host-agnostic crates, unit-tested on their own,
and one GUI crate that wires them.

| Crate | Role |
|---|---|
| `flint-protocol` | Message types (serde), handshake, error codes, limits. Framing compatible with `fenix-rpc` (its writer; a bounded reader of its own until F16). No threads, no sockets. |
| `flint-index` | Candidate store, query parsing (prefix, scope), matching with `nucleo-matcher`, frecency, the stable-ordering merge of streamed results. Pure. |
| `flint-config` | `settings.toml`: schema declared once, read, validated per setting (a bad value costs only that setting), live-reloaded. Mirrors `fenix-config`'s conventions. |
| `flint-platform` | Everything OS-specific behind traits: global hotkey, control endpoint, foreground-window info, focus hand-off, open/reveal/terminal, autostart, app discovery, window list. `cfg(windows)` / `cfg(unix)`. |
| `flint-providers` | Built-in providers: apps, quicklinks, Flint's own commands, calculator, shell. |
| `flint-fenix` | Everything Fenix: locating it, reading its state files, its settings (theme), the control-protocol client with capability detection, the legacy hand-off fallback. Exposes Fenix as a provider. |
| `flint-host` | Provider host: spawning, the socket listener, handshakes, routing queries and executes, timeouts, crash isolation and restart. Threads live here. |
| `flint-gui` | winit + wgpu + glyphon: window, query line, list, chips, action menu, which-key popup, footer, theming. |
| `flint` | The binary: CLI parsing, daemon vs client role, `--console` on Windows (same as Fenix). |

### Dependencies on Fenix crates

Fenix's crates are not on crates.io (`publish = false`). Depend on them as
git dependencies pinned to a revision:

```toml
[workspace.dependencies]
fenix-keymap  = { git = "https://github.com/tpedneault/fenix", rev = "f05780740b7d45e41eedc35b846dba7c832f4d49" }
fenix-rpc     = { git = "https://github.com/tpedneault/fenix", rev = "f05780740b7d45e41eedc35b846dba7c832f4d49" }
fenix-storage = { git = "https://github.com/tpedneault/fenix", rev = "f05780740b7d45e41eedc35b846dba7c832f4d49" }
```

| Crate | Why | Weight |
|---|---|---|
| `fenix-keymap` | The leader trie and `KeyPress` model, so key handling is identical to Fenix | no dependencies |
| `fenix-rpc` | `Content-Length` framing, already proven against LSP/DAP (writer now; reader once it is bounded, F16) | no dependencies |
| `fenix-storage` | Fenix's folder resolution (`FENIX_HOME` included), its keyed state files, atomic writes | `dirs`, `serde_json`, `tempfile`, `toml_edit` |
| `fenix-jira` | Later (milestone 8), for the Jira provider | `ureq` with native certs |

Do **not** depend on `fenix-project`: it pulls in `fenix-mib` and
`fenix-ccsds` to read two JSON keys. Read `state/projects.json` and
`state/recent.json` through `fenix_storage::state::read` with the keys
documented in [FENIX-INTEGRATION.md](FENIX-INTEGRATION.md#what-flint-reads-today),
and treat that as a stopgap until Fenix exposes a light crate (F7).

Do not use `fenix-picker` for matching. Its scorer is a deliberately simple
greedy one (its own doc comment says so) and returns no match positions;
Flint needs highlighting and must stay fast at 100k candidates. Use
`nucleo-matcher` (the matcher Helix uses).

### Rendering

wgpu + winit + glyphon, the same stack and versions as Fenix
(`wgpu 30`, `winit 0.30`, `glyphon 0.12` at the pinned revision), so
fonts and themes look the same. Two things to take from Fenix's `gpu.rs`
and one to do better:

- Force a non-sRGB surface format, as Fenix does, so colours written by
  the shaders match Fenix's exactly.
- Reconfigure the surface at most once per frame, not per resize event.
- Fenix requests a high-performance adapter and `expect`s one, with no
  fallback. Flint must retry with `force_fallback_adapter: true` (WARP on
  DX12, llvmpipe on Linux) before giving up, because VMs and RDP sessions
  without a usable GPU are normal in corporate settings and a launcher
  that fails to start at login is worse than a slower one. Log which
  adapter was chosen in `flint status`. (Fenix would benefit from the same
  fallback; see F15.)
- Unlike Fenix, request a low-power adapter, and use DX12 on Windows
  (`WGPU_BACKEND` overrides). Measured in M1 on one machine (GTX 1660 Ti,
  release build, two runs each): DX12 was ready in 524–649 ms using
  98–120 MB resident, Vulkan in 951 ms using 139 MB; and WARP, the
  fallback, is DX12's.

Layout is fixed and simple: query line, result rows (title, subtitle,
match highlights, provider badge, an icon glyph), footer. Use Nerd Font
glyphs for icons when a Nerd Font is installed, as Fenix's explorer does
(`icon::ICON_FONT_FAMILY`); fall back to letters. Real app icons
(extracting `.lnk`/`.ico`/`.desktop` icons) are a later nicety, loaded
lazily and cached; never block a frame on one.

### Theming

Flint follows Fenix's theme by default: it reads `editor.theme` and
`editor.font_family` from Fenix's `settings.toml` (through
`fenix_storage::paths::settings_file()`), watches the file, and applies a
change live. `appearance.theme = "fenix"` (default) means "follow Fenix";
any theme name pins one.

Fenix's palettes live in `fenix-gui` and are built from `glyphon` types, so
Flint cannot depend on them. Until Fenix extracts them (F9), Flint carries
a copy of the eight palettes' handful of colours it needs (background,
foreground, accent, selection, muted text, border), in one file with a
comment naming the Fenix revision they were copied from and a test that
the set of names matches Fenix's `theme::ALL` list in this document:
Orbit Dark, TempleOS, Gruvbox Dark, Nord, Dracula, Solarized Dark,
One Dark, Visual Studio Dark. An unknown name falls back to Orbit Dark.

## Ranking

Each candidate gets `score = match_score × w_match + frecency(item, query_prefix) × w_frec + provider_boost + context_boost`.

- **Match**: `nucleo-matcher` with smart case and path-aware bonuses for
  file items, fuzzily on the title. Keywords and subtitles match by
  substring at half weight (decided in M2: fuzzy matching across a
  subtitle's many words matched nearly anything).
- **Frecency**: keyed by `(item id, first two characters of the query)`;
  each pick adds 1, values decay with a 7-day half-life. Stored in Flint's
  state folder through `fenix_storage::state` (a small JSON file, atomic
  writes), capped at 5,000 entries, least-recent pruned.
- **Context**: items from the project Fenix reports as current, or whose
  provider declares relevance to the foreground app, get a boost.
- **Stable merge**: once a row has been on screen for more than one frame,
  later results are inserted only below the selection, never above it. A
  new keystroke re-ranks everything.

Item ids must be stable across sessions (a path, a URL, a Fenix command
id, a provider-defined key), or frecency cannot work. The protocol says so.

## Providers

### Built in

| Provider | Prefix | Items | Actions |
|---|---|---|---|
| Apps | `a` | Windows: `.lnk` under the user's and all-users' Start Menu `Programs` folders (Scoop's "Scoop Apps" folder included). Linux: `.desktop` files in the XDG data dirs. | launch; open containing folder; copy path |
| Quicklinks | per link | one item per `[[quicklinks]]` entry; the query after the keyword fills `{query}` | open URL; copy URL |
| Fenix projects | `p` | Fenix's known projects, pinned first, group as subtitle | open in Fenix (needs F1); open folder; terminal here; copy path |
| Fenix recent files | `f` | Fenix's recent files | open in Fenix; reveal; copy path |
| Fenix commands | `:` | Fenix's command registry, with key sequences shown (needs F1/F6) | run in Fenix |
| Flint | `flint` | reload config, open settings, show logs, quit | — |
| Calculator | `=` | evaluates the query (a small expression parser; no shelling out) | copy result |
| Shell | `>` | the query as a command | run in the user's terminal; run hidden and show exit status |
| Windows | `w` | open top-level windows (Windows: `EnumWindows`; KDE: KWin scripting over D-Bus, later) | focus; close |

### External

Anything else is a provider process speaking [PROTOCOL.md](PROTOCOL.md):

```toml
[[providers]]
name = "jira"
command = ["flint-jira"]          # spawned, stdio
prefix = "j"

[[providers]]
name = "fenix"
connect = true                    # a long-running app connects to Flint itself
```

- A provider is spawned only if declared in config. No auto-discovery.
- A provider that crashes is restarted with backoff (1 s, 5 s, 30 s) and
  disabled after three crashes in a minute, with the reason in
  `flint status` and in the footer when its prefix is used.
- Results arriving after a newer keystroke, or more than 2 s after the
  query, are dropped.
- Provider stderr goes to Flint's log, one file per provider, rotated.

Ship two example providers in `examples/`: one in Rust and one in Python
(standard library only), both under 150 lines, to prove the protocol is
language-agnostic and to serve as templates.

## Configuration

Flint's folders mirror Fenix's four kinds, under `flint` instead of
`fenix`: settings in `%AppData%\flint` / `~/.config/flint`, state in
`%LocalAppData%\flint\state` / `~/.local/state/flint/state`, logs beside
the state. `FLINT_HOME` puts everything in one folder, like `FENIX_HOME`.
`fenix_storage::paths` hard-codes `fenix`; until it is generalised (F14),
`flint-config` implements the same rules in a dozen lines.

`settings.toml`, hand-editable, live-reloaded, every key declared once
in a schema with a default, a range or choice set, and a description (the
same discipline as `fenix-config`; generate the settings table in
`docs/CONFIGURATION.md` from the schema and fail a test when it is stale):

```toml
[hotkey]
toggle = "Alt+Space"

[appearance]
theme = "fenix"            # follow Fenix; or a theme name
width = 720                # logical pixels
rows = 9

[behaviour]
escape_hides = false
hide_on_blur = true

[fenix]
executable = ""            # empty: find `fenix` on PATH, then next to Flint

[[quicklinks]]
keyword = "j"
name = "Jira issue"
url = "https://jira.example.com/browse/{query}"

[[quicklinks]]
keyword = "mr"
name = "GitLab merge requests"
url = "https://gitlab.example.com/dashboard/merge_requests?search={query}"

[leader]
"p"   = "scope:fenix.projects"
"f r" = "scope:fenix.recent"
"j"   = "scope:quicklink.j"
"t"   = "fenix:agenda.toggle_clock"
```

`{query}` is percent-encoded in URLs. In `run` actions it is passed as one
argument, never interpolated into a shell string.

`fenix-keymap`'s labels are `&'static str`; leader labels from config are
not. Until F10 lands, intern config labels with a small arena that lives as
long as the daemon and is rebuilt (not grown) on reload.

## Platform notes

### Windows (primary)

- **Hotkey**: `RegisterHotKey` via the `global-hotkey` crate; no admin.
  If registration fails (another app owns the chord), say so in the footer
  and in `flint status`, and keep running so `flint toggle` still works.
  `Alt+Space` shadows the system window menu shortcut; document it.
- **Focus**: Windows only lets the foreground process give away focus.
  When the hotkey fires, Flint is allowed to take focus. When Flint hands
  off to another app (Fenix, a launched program), call
  `AllowSetForegroundWindow` with the target's PID *before* sending the
  request, which is why Fenix's control protocol has to report its PID
  (F3). For processes Flint launches itself, use the PID from the spawn.
- **Apps**: resolve `.lnk` targets lazily (only when needed for display or
  launch); launch through `ShellExecuteExW` on the `.lnk` itself so
  arguments, working directory and "run as" settings are honoured.
  (M2 never resolves them: the name, folder and the shortcut itself are
  enough to list and launch.) Packaged apps (Microsoft Store / MSIX:
  Calculator, Windows Terminal, Settings, Photos) have no `.lnk` in those
  folders, so the Apps provider doesn't list them yet; listing them means
  enumerating the shell's `AppsFolder` (COM) and launching by
  AppUserModelID. Found in M2 testing; a follow-up for `flint-platform`.
- **Autostart**: opt-in, `flint autostart on|off`, writing
  `HKCU\Software\Microsoft\Windows\CurrentVersion\Run`. No scheduled task,
  no service.
- **Console**: `#![windows_subsystem = "windows"]` and `--console`, as Fenix.

### Linux, KDE Plasma 6 on Wayland

- **Hotkey**: Wayland gives applications no global hotkeys. Document
  binding a KDE custom shortcut to `flint toggle`. The client forwards
  `XDG_ACTIVATION_TOKEN` from its environment to the daemon, which uses
  winit's startup-notify support to activate its window with it (without a
  token KWin's focus-stealing prevention may leave the window behind). The
  xdg-desktop-portal GlobalShortcuts portal is a later improvement.
- **No hidden windows**: winit can neither hide nor focus a window on
  Wayland (`set_visible` and `focus_window` do nothing there), so the
  pre-created hidden window is a Windows/X11 strategy. On Wayland the
  window is created on each show, with the activation token, and dropped
  on hide; the GPU device, fonts and glyph atlas stay loaded, so only the
  surface is made again (decided in M1).
- **Placement**: Wayland clients cannot position their windows. Accept
  KWin's placement, and document a KWin window rule (centre, keep above,
  skip taskbar) shipped as a file in `contrib/kde/`. A layer-shell surface
  is out of scope for now (winit does not support it).
- **Windows provider**: not available to ordinary Wayland clients; on KDE
  it goes through KWin scripting/D-Bus later. Keep it behind the platform
  trait and leave it unimplemented on Linux in the first milestones.
- **X11**: should work through winit with a hotkey via `global-hotkey`;
  supported on a best-effort basis.

## Security

- **Control endpoint is per-user and private.** Windows: pipe name
  includes the user's SID and the login session id (pipe names are
  machine-wide, and one user logged in twice on a Remote Desktop host
  needs two daemons), created with a DACL granting access to that SID
  only, with `FILE_FLAG_FIRST_PIPE_INSTANCE` and
  `PIPE_REJECT_REMOTE_CLIENTS`; clients connect with identification-level
  impersonation only. With `FLINT_HOME` set, the endpoint's name carries a
  hash of that folder, so a portable or test Flint runs beside the
  installed one. Linux:
  `$XDG_RUNTIME_DIR/flint/control.sock` in a `0700` directory; if
  `XDG_RUNTIME_DIR` is unset, `/tmp/flint-$UID/` created `0700` and
  checked to be owned by the user before use. A lock file beside the
  socket, held for the daemon's life, decides who the daemon is, so a
  socket left by a crash is replaced safely.
- **Providers run as the user and only if declared in config.**
- **No shell interpolation.** `run` actions take an argv array.
- **Secrets**: provider tokens go in the OS credential store (`keyring`
  crate: Windows Credential Manager, Secret Service), never in
  `settings.toml`. Flint never reads Fenix's tokens out of Fenix's
  `settings.toml`; see F12 for the shared credential naming.
- **Network**: providers that call HTTP use the system proxy and the OS
  certificate store (`ureq` with `native-certs`, as `fenix-jira` does), so
  a TLS-intercepting corporate proxy works without configuration.

## Performance budget and how it is checked

| Budget | Target | How it is measured |
|---|---|---|
| hotkey → first presented frame | ≤ 50 ms p95 | timestamps in the daemon (hotkey received → `present` returned), logged; `flint status --timings` prints the last 100 |
| keystroke → list updated (local providers, 100k candidates) | ≤ 16 ms p95 | `flint-index` benchmark in CI on synthetic data; fails the build when exceeded by 50% |
| idle CPU while hidden | ~0 | event loop in `ControlFlow::Wait`; no polling timers while hidden |
| resident memory | < 120 MB | noted in `flint status` |
| daemon cold start to ready | ≤ 1.5 s | logged; not on the hot path but watched |

Fenix already has a launch profiler (`profile.rs`) and a UI-thread
watchdog (`watchdog.rs`); copy their approach rather than inventing one.

## Testing

- Host-agnostic crates carry the logic and the unit tests:
  query parsing, ranking, stable merge, frecency decay, config validation,
  protocol encode/decode and handshake state machine, Fenix state parsing
  against fixture files copied from a real Fenix state folder.
- `flint-host` gets integration tests that spawn the example providers and
  a deliberately misbehaving one (crashes, hangs, floods, sends garbage).
- `flint-fenix` gets tests against a fake Fenix control server, for both
  the modern protocol and the legacy hand-off fallback.
- CI on Windows and Linux: `cargo fmt --check`, `cargo clippy --workspace
  --all-targets -- -D warnings`, `cargo test --workspace`, and the index
  benchmark gate. Mirror Fenix's `scripts/ci.ps1` single-entry-point style.

## Milestones

Each milestone ends usable, with docs and changelog updated.

| # | Delivers | Fenix changes needed |
|---|---|---|
| M0 | Workspace, crates as empty shells with their doc comments, licences, CI on both OSes, `docs/ARCHITECTURE.md` | none |
| M1 | Daemon, per-user control endpoint, `flint`/`flint toggle|show|hide|quit|status`, hotkey on Windows, KDE shortcut docs, pre-created hidden window, query line, static list, timing log | none |
| M2 | `flint-index` with nucleo, Insert/Normal modes, Apps + Quicklinks + Flint providers, frecency, stable merge, benchmark gate | none |
| M3 | Fenix level 0: projects and recent files from Fenix's state files, open files in Fenix through the existing hand-off (spawning `fenix <file>`), follow Fenix's theme | none (works today) |
| M4 | Action menu, which-key leader from config, footer messages | none |
| M5 | Provider protocol: stdio and socket transports, crash isolation, both example providers, `docs/PROVIDERS.md` | none |
| M6 | Windows provider (Windows only), calculator, shell prefix, autostart | none |
| M7 | Fenix level 1: control-protocol client with capability detection; open project; `file:line:col`; Fenix commands with their keys; context from Fenix; agenda clock in/out and notebook capture | F1–F6 (F8 for agenda writes) |
| M8 | Jira and GitLab providers (reusing `fenix-jira`/`fenix-gitlab`), credentials in the OS store | F12 to share credentials |

M0–M6 need nothing from Fenix. M7 is the point where Fenix work must have
landed; [FENIX-INTEGRATION.md](FENIX-INTEGRATION.md) orders that work so
the first Fenix pull request unblocks the most.

## Risks

| Risk | Mitigation |
|---|---|
| Windows focus rules make hand-offs land behind other windows | `AllowSetForegroundWindow` with the target PID; test on a real desktop early in M3, not at the end |
| KWin focus-stealing prevention and placement on Wayland | activation-token forwarding in M1; window rule in `contrib/kde/` |
| wgpu on GPU-less VMs and RDP | fallback adapter retry; test in an RDP session |
| Scope creep into a second Fenix | the non-goals above; a feature that needs editing is a provider handing off |
| Fenix's state file format changes under Flint | read defensively (missing or unreadable means empty, as Fenix does), fixture tests, and F7 to make it a contract |
| Latency regresses silently | benchmark gate in CI and the timing log from M1 |
