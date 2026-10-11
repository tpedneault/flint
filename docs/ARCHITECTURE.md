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

Status: M1. The daemon, the control endpoint, the CLI, the hotkey and the
window with its query line and a static list work; matching, providers
and actions don't exist yet. The *Filled in* column says which milestone
gives each crate its first code, or its next.

## Crates

| Crate | Role | Filled in |
|---|---|---|
| `flint-protocol` | Message types, handshake, error codes and size limits of [PROTOCOL.md](PROTOCOL.md); `Content-Length` framing compatible with `fenix-rpc`, with a bounded reader; the host-side handshake state machine and a blocking client. No threads, no sockets. | M1 (envelope, `hello`, client messages), M5 (provider messages) |
| `flint-index` | Candidate store, query and scope parsing, matching with `nucleo-matcher`, frecency, the stable merge of streamed results. Pure; carries the 16 ms benchmark gate. | M2 |
| `flint-config` | `settings.toml`: the schema declared once, reading, per-setting validation; the hotkey `Chord`; Flint's folders and `FLINT_HOME`. | M1 |
| `flint-platform` | Everything OS-specific: the control endpoint, the hotkey (behind the `Hotkey` trait), focus hand-off, the console, process facts; later open/reveal/terminal, autostart, app discovery, window list. | M1, M2, M6 |
| `flint-providers` | Built-in providers: apps, quicklinks, Flint's commands, then calculator, shell and windows. | M2, M6 |
| `flint-fenix` | Everything Fenix: locating it, its state files and theme, the control-protocol client with capability detection, the legacy hand-off. | M3, M7 |
| `flint-host` | The control server for CLI clients (M1); the provider host (spawning and accepting providers, routing, timeouts, crash isolation) in M5. Owns the threads that talk to peers. | M1, M5 |
| `flint-gui` | winit + wgpu + glyphon: the window, query line, list and footer; GPU with fallback adapter; later chips, highlights, action menu, which-key popup, theming. | M1 |
| `flint` | The executable: command line, daemon or client role, the daemon's controller, its log and timing log. | M1 |

Each crate's `lib.rs` (or `main.rs`) opens with a doc comment that is the
longer version of its row here, including the tradeoffs it takes. Read
that before changing a crate.

## How the crates depend on each other

The edges as they are, each added when a crate first used the other
(the binary depends on every crate so building it builds the workspace):

```
flint ──► every crate below

flint-gui ──────► flint-config                       (+ winit, wgpu, glyphon)
flint-host ─────► flint-protocol
flint-platform ─► flint-config                       (+ windows-sys, global-hotkey, libc)
flint-protocol ─►                                    (+ fenix-rpc, serde_json)
flint-config ───►                                    (+ toml, dirs)
```

Still to come: `flint-gui` → `flint-index` and `fenix-keymap` (M2, M4),
`flint-fenix` → `flint-protocol`, `flint-platform`, `fenix-storage` (M3),
`flint-providers` → `flint-protocol`, `flint-platform` (M2), `flint-host`
→ `flint-platform` for spawning providers (M5).

`flint-platform` depends on `flint-config` for one type, `Chord`: the
hotkey is parsed and validated with the other settings, and the platform
maps a parsed chord onto what the OS registers. `flint-host` doesn't
depend on `flint-platform`: it serves any `Read`/`Write` pair, so its tests
run over in-memory pipes and the binary hands it the endpoint's streams.

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

## The daemon at run time

```
 event-loop thread (flint-gui's winit loop, flint's Daemon controller)
   owns: the window and GPU surface, the hotkey registration, settings,
         the timing log; does all the work
   ▲ Event::Hotkey             ▲ Event::Request { request, reply }
   │ (global-hotkey callback)  │
   │                     control threads (flint-host), one per connection:
   │                       read a frame → HostSession → hand the request to
   │                       the loop → wait ≤ 5 s for the answer → write it
   │                     accept thread: blocks on the endpoint
```

While hidden, all three wait on the OS (`ControlFlow::Wait`, a blocking
accept, a blocking read); there are no timers, so the daemon uses no CPU.
`quit` is answered before the daemon exits: the control thread writes the
answer and only then sends `Event::Exit`.

Single instance: `flint` connects to the endpoint; if nobody answers it
binds it and becomes the daemon; if binding finds it taken, another
daemon is starting that instant, and it connects again.

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
