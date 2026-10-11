# Architecture

Flint is a Cargo workspace of small crates, the same shape as Fenix's.
The logic lives in host-agnostic crates that know nothing about windows,
the GPU or the operating system, and each is unit-tested on its own
(`cargo test --workspace`). Two crates touch the outside world:
`flint-platform` (the OS) and `flint-gui` (the window and the GPU). The
`flint` binary wires everything together.

This document is the map of the code. *Why* Flint is built this way (the
resident daemon, the budgets, ranking, the platform constraints) is in
[DESIGN.md](DESIGN.md); the wire protocol is in [PROTOCOL.md](PROTOCOL.md);
what comes from Fenix is in [FENIX-INTEGRATION.md](FENIX-INTEGRATION.md).

Status: M0. Every crate exists with a header saying what it is for and
what it deliberately doesn't do; none has behaviour yet. The *Filled in*
column says which milestone gives each one its first code.

## Crates

| Crate | Role | Filled in |
|---|---|---|
| `flint-protocol` | Message types, handshake, error codes and size limits of [PROTOCOL.md](PROTOCOL.md); `Content-Length` framing compatible with `fenix-rpc`, with a bounded reader. No threads, no sockets. | M1 (client messages), M5 |
| `flint-index` | Candidate store, query and scope parsing, matching with `nucleo-matcher`, frecency, the stable merge of streamed results. Pure; carries the 16 ms benchmark gate. | M2 |
| `flint-config` | `settings.toml`: the schema declared once, reading, per-setting validation, live reload; Flint's folders and `FLINT_HOME`. | M1 |
| `flint-platform` | Everything OS-specific behind traits: hotkey, control endpoint, foreground window, focus hand-off, open/reveal/terminal, autostart, app discovery, window list. | M1 |
| `flint-providers` | Built-in providers: apps, quicklinks, Flint's commands, then calculator, shell and windows. | M2, M6 |
| `flint-fenix` | Everything Fenix: locating it, its state files and theme, the control-protocol client with capability detection, the legacy hand-off. | M3, M7 |
| `flint-host` | The provider host: spawning and accepting providers, handshakes, routing, timeouts, crash isolation and restart. Owns the threads that talk to peers. | M5 |
| `flint-gui` | winit + wgpu + glyphon: the pre-created window, query line, list, chips, action menu, which-key popup, footer, theming. | M1 |
| `flint` | The executable: command line, daemon or client role, `--console` on Windows. | M0 (`--version`, `--help`), M1 |

Each crate's `lib.rs` (or `main.rs`) opens with a doc comment that is the
longer version of its row here, including the tradeoffs it takes. Read
that before changing a crate.

## How the crates depend on each other

The intended edges, added to a crate's `Cargo.toml` when it first uses the
other crate rather than ahead of time (except the binary, which depends on
every crate from M0 so building it builds the whole workspace):

```
flint ──► every crate below

flint-gui ──────► flint-index, flint-config          (+ winit, wgpu, glyphon, fenix-keymap)
flint-host ─────► flint-protocol, flint-platform, flint-config
flint-fenix ────► flint-protocol, flint-platform     (+ fenix-storage, fenix-rpc)
flint-providers ► flint-protocol, flint-platform
flint-protocol ─►                                    (+ fenix-rpc)
flint-index, flint-config, flint-platform ─► no flint crates
```

Two rules hold whatever the edges become, and
[`crates/flint/tests/workspace.rs`](../crates/flint/tests/workspace.rs)
fails the build when either is broken:

- **Only `flint-gui` and `flint-platform` depend on OS or window-system
  crates** (`winit`, `wgpu`, `glyphon`, `windows`/`windows-sys`,
  `global-hotkey`, `interprocess`, `libc`/`nix`/`rustix`, `zbus`, ...).
  Everything else reaches the OS through `flint-platform`'s traits, which
  is what lets CI test the logic around hotkeys, focus and window
  placement with fakes.
- **Fenix crates come from one pinned git revision**, declared once in
  the workspace's `[workspace.dependencies]` and taken from there with
  `{ workspace = true }`. Only `fenix-keymap`, `fenix-rpc` and
  `fenix-storage` are allowed until M8 adds `fenix-jira` and
  `fenix-gitlab`. Never `fenix-project` (it pulls in Fenix's MIB and CCSDS
  crates), `fenix-gui` or `fenix-picker` (DESIGN.md explains each).

The same test checks that this document's crate table matches `crates/`,
that every crate inherits the workspace's version, edition, licence and
lints, and that the licence is MIT OR Apache-2.0.

## Conventions

The same as Fenix's, so code moves between the two without translation:

- Doc comments explain *why*, including the tradeoff taken. The
  workspace enables the `missing_docs` lint, and CI denies warnings, so a
  crate without a header fails the build.
- No panics on bad input from config, files or peers; errors reach the
  user as one line in the footer naming what failed and why.
- User-facing docs (`CONFIGURATION.md`, `KEYBINDINGS.md`, `PROVIDERS.md`
  once they exist) change in the same pull request as the behaviour.
- `CHANGELOG.md` gets a line under *Unreleased* for every user-visible
  change.

## Checking a change

One script runs every gate, locally and in CI, like Fenix's:

```powershell
pwsh -NoProfile -File ./scripts/ci.ps1
```

It needs Rust stable with `rustfmt` and `clippy`, and PowerShell 7. The
gates, in order:

| Gate | Command |
|---|---|
| `fmt` | `cargo fmt --all --check` |
| `clippy` | `cargo clippy --locked --workspace --all-targets -- -D warnings` |
| `tests` | `cargo test --locked --workspace --no-fail-fast` |
| `build` | `cargo build --locked -p flint` |

The index benchmark gate joins them in M2 (DESIGN.md, "Performance
budget"). Each run writes one log per gate and a `summary.json` to a fresh
folder under `target/ci/`, and stops at the first failing gate.

`--locked` means a change to dependencies must include the updated
`Cargo.lock`, so CI never silently builds a different dependency graph.

The GitHub Actions workflow ([`.github/workflows/ci.yml`](../.github/workflows/ci.yml))
runs the script on Windows (`windows-2022`, primary, checked out to a path
containing a space) and on Linux (`ubuntu-24.04`), and uploads the logs.
Actions are pinned to commit hashes and the token is read-only.

Some behaviour can't be checked in CI at all: the hotkey, focus, KWin
placement, the fallback GPU adapter in an RDP session, and real-hardware
latency. Pull requests that depend on them list exact steps under "Needs
checking on a desktop".
