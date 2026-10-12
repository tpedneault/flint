//! Checks on the workspace itself, so the rules in `CLAUDE.md` and
//! `docs/ARCHITECTURE.md` fail a build when broken instead of relying on a
//! reviewer noticing: the crate list matches the architecture document,
//! every crate inherits the shared package settings and lints, only
//! `flint-gui` and `flint-platform` depend on window-system or OS crates,
//! and Fenix crates come from one pinned revision and only the allowed ones.
//!
//! They read manifests rather than `cargo metadata` so they need no network
//! and no extra tool; the price is that they see direct dependencies only,
//! which is where the rules apply.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use toml::Table;

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn read(path: &Path) -> String {
    fs::read_to_string(path).unwrap_or_else(|e| panic!("reading {}: {e}", path.display()))
}

fn manifest(path: &Path) -> Table {
    read(path)
        .parse()
        .unwrap_or_else(|e| panic!("parsing {}: {e}", path.display()))
}

/// Every crate directory under `crates/`, by name.
fn crate_dirs() -> BTreeSet<String> {
    fs::read_dir(root().join("crates"))
        .expect("crates/ exists")
        .map(|entry| entry.expect("readable entry"))
        .filter(|entry| entry.path().join("Cargo.toml").is_file())
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .collect()
}

/// The names of every direct dependency in a manifest: `[dependencies]`,
/// `[dev-dependencies]` and their `[target.'cfg(..)'.*]` forms.
fn dependency_names(manifest: &Table) -> BTreeSet<String> {
    const KINDS: [&str; 2] = ["dependencies", "dev-dependencies"];
    let mut tables: Vec<&Table> = KINDS
        .iter()
        .filter_map(|kind| manifest.get(*kind).and_then(|v| v.as_table()))
        .collect();
    if let Some(targets) = manifest.get("target").and_then(|v| v.as_table()) {
        for target in targets.values().filter_map(|v| v.as_table()) {
            tables.extend(
                KINDS
                    .iter()
                    .filter_map(|kind| target.get(*kind).and_then(|v| v.as_table())),
            );
        }
    }
    tables
        .into_iter()
        .flat_map(|table| table.keys().cloned())
        .collect()
}

#[test]
fn every_crate_is_a_member_and_documented_in_architecture() {
    let dirs = crate_dirs();

    let workspace = manifest(&root().join("Cargo.toml"));
    let members: BTreeSet<String> = workspace["workspace"]["members"]
        .as_array()
        .expect("workspace.members is an array")
        .iter()
        .filter_map(|m| {
            let m = m.as_str().expect("member is a string");
            // Example providers live in `examples/`, outside the crate table.
            m.strip_prefix("crates/").map(str::to_owned)
        })
        .collect();
    assert_eq!(members, dirs, "workspace members and crates/ differ");

    // Crate-table rows in ARCHITECTURE.md start with "| `flint...` |".
    let architecture = read(&root().join("docs/ARCHITECTURE.md"));
    let documented: BTreeSet<String> = architecture
        .lines()
        .filter_map(|line| line.strip_prefix("| `"))
        .filter_map(|rest| rest.split_once('`').map(|(name, _)| name.to_owned()))
        .filter(|name| name == "flint" || name.starts_with("flint-"))
        .collect();
    assert_eq!(
        documented, dirs,
        "the crate table in docs/ARCHITECTURE.md and crates/ differ"
    );
}

#[test]
fn every_crate_inherits_package_settings_and_lints() {
    for name in crate_dirs() {
        let manifest = manifest(&root().join("crates").join(&name).join("Cargo.toml"));
        let package = manifest["package"].as_table().expect("[package]");
        assert_eq!(package["name"].as_str(), Some(name.as_str()));
        for key in [
            "version",
            "edition",
            "license",
            "repository",
            "authors",
            "publish",
        ] {
            let inherited = package
                .get(key)
                .and_then(|v| v.as_table())
                .and_then(|t| t.get("workspace"))
                .and_then(|v| v.as_bool());
            assert_eq!(inherited, Some(true), "{name}: `{key}.workspace = true`");
        }
        let lints = manifest
            .get("lints")
            .and_then(|v| v.get("workspace"))
            .and_then(|v| v.as_bool());
        assert_eq!(lints, Some(true), "{name}: `[lints] workspace = true`");
    }
}

#[test]
fn the_licence_is_mit_or_apache() {
    let workspace = manifest(&root().join("Cargo.toml"));
    assert_eq!(
        workspace["workspace"]["package"]["license"].as_str(),
        Some("MIT OR Apache-2.0")
    );
    for file in ["LICENSE-MIT", "LICENSE-APACHE"] {
        assert!(root().join(file).is_file(), "{file} is missing");
    }
}

#[test]
fn only_gui_and_platform_touch_the_os_or_window_system() {
    // Crates whose only purpose is talking to the window system, the GPU or
    // OS APIs. Logic crates reach the OS through `flint-platform`'s traits.
    const OS_CRATES: [&str; 15] = [
        "arboard",
        "glyphon",
        "global-hotkey",
        "interprocess",
        "libc",
        "nix",
        "raw-window-handle",
        "rustix",
        "wgpu",
        "windows",
        "windows-core",
        "windows-registry",
        "windows-sys",
        "winit",
        "zbus",
    ];
    const ALLOWED: [&str; 2] = ["flint-gui", "flint-platform"];

    for name in crate_dirs() {
        if ALLOWED.contains(&name.as_str()) {
            continue;
        }
        let manifest = manifest(&root().join("crates").join(&name).join("Cargo.toml"));
        let offending: Vec<String> = dependency_names(&manifest)
            .into_iter()
            .filter(|dep| OS_CRATES.contains(&dep.as_str()))
            .collect();
        assert!(
            offending.is_empty(),
            "{name} depends on {offending:?}; OS and window-system code belongs in \
             flint-platform or flint-gui"
        );
    }
}

#[test]
fn fenix_crates_are_pinned_once_and_only_the_allowed_ones() {
    // docs/DESIGN.md, "Dependencies on Fenix crates". `fenix-jira` and
    // `fenix-gitlab` join this list in M8.
    const ALLOWED: [&str; 3] = ["fenix-keymap", "fenix-rpc", "fenix-storage"];

    let workspace = manifest(&root().join("Cargo.toml"));
    let pinned = workspace["workspace"]["dependencies"]
        .as_table()
        .expect("[workspace.dependencies]");
    let mut revs = BTreeSet::new();
    for (name, spec) in pinned.iter().filter(|(n, _)| n.starts_with("fenix-")) {
        assert!(
            ALLOWED.contains(&name.as_str()),
            "{name} is not an allowed Fenix crate"
        );
        assert_eq!(
            spec.get("git").and_then(|v| v.as_str()),
            Some("https://github.com/tpedneault/fenix"),
            "{name} must come from Fenix's repository"
        );
        let rev = spec.get("rev").and_then(|v| v.as_str());
        assert!(
            rev.is_some_and(|r| r.len() == 40),
            "{name} must be pinned to a full commit hash"
        );
        revs.insert(rev.unwrap_or_default().to_owned());
    }
    assert!(
        revs.len() <= 1,
        "Fenix crates pinned to different revisions: {revs:?}"
    );

    // Members must take Fenix crates from the workspace pin, never their own.
    for name in crate_dirs() {
        let manifest = manifest(&root().join("crates").join(&name).join("Cargo.toml"));
        for dep in dependency_names(&manifest)
            .iter()
            .filter(|d| d.starts_with("fenix-"))
        {
            assert!(ALLOWED.contains(&dep.as_str()), "{name} depends on {dep}");
            let from_workspace = manifest
                .get("dependencies")
                .and_then(|d| d.get(dep))
                .and_then(|spec| spec.get("workspace"))
                .and_then(|v| v.as_bool());
            assert_eq!(
                from_workspace,
                Some(true),
                "{name}: `{dep} = {{ workspace = true }}`"
            );
        }
    }
}

/// docs/PROVIDERS.md promises examples short enough to read in one sitting
/// and copy; this keeps the promise as they grow.
#[test]
fn the_example_providers_stay_under_150_lines() {
    for example in [
        "examples/provider-python/bookmarks.py",
        "examples/provider-rust/src/main.rs",
    ] {
        let lines = read(&root().join(example)).lines().count();
        assert!(lines < 150, "{example} is {lines} lines; keep it under 150");
    }
}
