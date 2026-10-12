# Working on Flint

Flint is a keyboard-first launcher for Windows and Linux, the front door
to Fenix (https://github.com/tpedneault/fenix) and the tools built around
it. The design is settled; M0 (workspace), M1 (daemon, endpoint, CLI,
hotkey, window), M2 (search, modes, apps, quicklinks, frecency), M3
(Fenix level 0: projects, recent files, theme), M4 (action menu,
which-key leader, footer messages) and M5 (external providers, two
examples) are in place, and the milestones from M6 fill in the rest.

## Read first, in this order

1. `docs/DESIGN.md` — what Flint is, the architecture, crates, ranking,
   platform notes, budgets, and the milestones (M0–M8).
2. `docs/PROTOCOL.md` — the provider/client wire protocol. `flint-protocol`
   implements exactly this; change both together.
3. `docs/FENIX-INTEGRATION.md` — what Flint uses from Fenix, how it
   degrades, and the Fenix changes F1–F16.

## How to work

- **Go milestone by milestone, in order.** Each milestone is one pull
  request that leaves Flint usable, with tests, docs and a `CHANGELOG.md`
  line. Don't start M(n+1) inside M(n)'s pull request.
- **M0–M6 need nothing from Fenix.** If you find yourself needing a Fenix
  change before M7, stop and record it in `docs/FENIX-INTEGRATION.md`
  instead of working around it silently.
- **Never modify the Fenix repository from a Flint session**, and never
  write Fenix's files at runtime (principle 2 of FENIX-INTEGRATION.md).
  Fenix work happens in its own repository and its own pull requests.
- **Depend on Fenix crates by git and pinned `rev`** (DESIGN.md,
  "Dependencies on Fenix crates"). Only `fenix-keymap`, `fenix-rpc`,
  `fenix-storage` now; `fenix-jira`/`fenix-gitlab` in M8. Not
  `fenix-project`, not `fenix-gui`, not `fenix-picker`.
- **Windows is the primary platform**, Linux (KDE Plasma 6, Wayland)
  second. CI runs both. Code that can't be tested in CI (hotkeys, focus,
  window placement) lives in `flint-platform` behind traits, with the
  logic around it tested through fakes.
- **Budgets are requirements.** The timing log arrives in M1 and the
  index benchmark gate in M2 (DESIGN.md, "Performance budget").

## Conventions (the same as Fenix's)

- A Cargo workspace of small crates. Logic lives in host-agnostic crates
  with unit tests; `flint-gui` and `flint-platform` are the only crates
  that touch the window system or the OS.
- Doc comments explain *why*, including the tradeoff taken, as Fenix's
  do. A reader should learn from a module's header what it is for and
  what it deliberately doesn't do.
- `cargo fmt`, `cargo clippy --workspace --all-targets -- -D warnings`,
  `cargo test --workspace` must pass. One CI entry point script
  (`scripts/ci.ps1`, PowerShell 7) runs the same gates locally and in CI,
  like Fenix's.
- Commit subjects name the area, then what changed for the user:
  `Index: frecency decays with a 7-day half-life`,
  `Platform: the hotkey is registered without admin rights`.
- Errors reach the user as one line in the footer, naming what failed and
  why. No dialogs. No panics on bad input from config, files or peers.
- User-facing docs (`docs/CONFIGURATION.md`, `docs/KEYBINDINGS.md`,
  `docs/PROVIDERS.md` once they exist) are updated in the same pull
  request as the behaviour. The settings table is generated from the
  schema and a test fails when it is stale.
- Licence: MIT OR Apache-2.0, as Fenix.

## What only Thomas can verify

Some behaviour can't be checked from CI or a cloud session. When a
milestone depends on one of these, say so in the pull request description
under "Needs checking on a desktop" with exact steps, rather than
reporting it as done:

- the hotkey on Windows, and `Alt+Space` against other apps that want it;
- focus: Flint coming to the front on the hotkey, and the app it hands off
  to (Fenix, a launched program) coming to the front afterwards;
- KDE Wayland: the `flint toggle` shortcut, activation tokens, the KWin
  window rule in `contrib/kde/`;
- the fallback GPU adapter in an RDP session;
- hotkey-to-frame latency on real hardware (`flint status --timings`).

## Where things are

```
docs/DESIGN.md              the design
docs/PROTOCOL.md            the wire protocol
docs/FENIX-INTEGRATION.md   Fenix: what's used, what's needed (F1–F16)
docs/PROVIDERS.md           writing a provider
examples/                   the example providers (Python, Rust)
docs/ARCHITECTURE.md        the crates, the rules tests enforce, the CI gates
docs/CONFIGURATION.md       settings (table generated from flint-config's schema)
docs/KEYBINDINGS.md         keys; docs/KDE.md: the Plasma/Wayland setup
contrib/kde/                the KWin window rule
crates/                     one folder per crate; each lib.rs header says why
scripts/ci.ps1              every gate, locally and in CI
CHANGELOG.md                Keep a Changelog; add under Unreleased
```
