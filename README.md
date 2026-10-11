# Flint

A keyboard-first launcher for Windows and Linux, written in Rust.

One hotkey from anywhere, a few characters, and an action: open an app, a
file, a project in [Fenix](https://github.com/tpedneault/fenix), a Jira
issue, an internal URL, or a command another tool registered. Flint uses
Fenix's grammar (Vim's Insert/Normal modes and a `SPC` leader with
which-key) and Fenix's themes, and is built to run on a locked-down
corporate machine: no admin rights, no installer, no cloud service.

**Status:** designed, not yet implemented.

- [Design](docs/DESIGN.md): the experience, architecture, milestones
- [Provider protocol](docs/PROTOCOL.md): how other tools add items and actions
- [Flint and Fenix](docs/FENIX-INTEGRATION.md): the integration, and what Fenix needs to add

## Licence

Flint is licensed under either of

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE))
- MIT license ([LICENSE-MIT](LICENSE-MIT))

at your option.
