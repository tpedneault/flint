# Flint

A keyboard-first launcher for Windows and Linux, written in Rust.

One hotkey from anywhere, a few characters, and an action: open an app, a
file, a project in [Fenix](https://github.com/tpedneault/fenix), a Jira
issue, an internal URL, or a command another tool registered. Flint uses
Fenix's grammar (Vim's Insert/Normal modes and a `SPC` leader with
which-key) and Fenix's themes, and is built to run on a locked-down
corporate machine: no admin rights, no installer, no cloud service.

**Status:** M2. Flint runs in the background, comes up on its hotkey (or
`flint toggle`), and finds and launches apps, quicklinks and its own
commands, with Fenix's Insert/Normal modes. Fenix integration arrives in
M3.

- [Design](docs/DESIGN.md): the experience, architecture, milestones
- [Architecture](docs/ARCHITECTURE.md): the crates, their rules, and how to check a change
- [Configuration](docs/CONFIGURATION.md), [keybindings](docs/KEYBINDINGS.md), and [KDE Plasma setup](docs/KDE.md)
- [Provider protocol](docs/PROTOCOL.md): how other tools add items and actions
- [Flint and Fenix](docs/FENIX-INTEGRATION.md): the integration, and what Fenix needs to add

## Licence

Flint is licensed under either of

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE))
- MIT license ([LICENSE-MIT](LICENSE-MIT))

at your option.
