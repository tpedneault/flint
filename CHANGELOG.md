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
