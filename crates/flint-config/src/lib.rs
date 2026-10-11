//! Flint's `settings.toml` and folders: every setting declared once in a
//! schema with its default, its allowed range or choices and a description;
//! reading and validating the file; and resolving Flint's settings, state
//! and log folders (`FLINT_HOME` included).
//!
//! It mirrors `fenix-config`'s conventions so someone who edits one file
//! can edit the other. A bad value costs only that setting: it falls back
//! to its default and produces one line of explanation for the footer and
//! `flint reload`, never an error that stops Flint starting. The settings
//! table in `docs/CONFIGURATION.md` is generated from the schema, and a test
//! fails when the two drift.
//!
//! This crate reads; it never writes `settings.toml`, so there is nothing
//! to preserve and plain `toml` is enough (Fenix needs `toml_edit` because
//! its settings page saves in place). Noticing that the file changed is the
//! daemon's job: it calls [`load`] again on `flint reload` and when the
//! window is shown with a newer file, which costs one `stat` and needs no
//! watcher thread or polling timer while Flint is hidden.
//!
//! Folder resolution copies `fenix_storage::paths`'s rules for `flint`
//! instead of `fenix` (a dozen lines) because that module hard-codes the
//! app name; F14 in `docs/FENIX-INTEGRATION.md` would let both share one
//! implementation. This crate never reads Fenix's settings; following
//! Fenix's theme is `flint-fenix`'s job.

pub mod chord;
pub mod paths;
pub mod schema;

use std::path::Path;
use std::time::SystemTime;

pub use chord::{Chord, Key};
pub use paths::Folders;

/// Every setting's value, after checking.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Settings {
    /// `hotkey.toggle`.
    pub hotkey: Chord,
    /// `appearance.width`, in logical pixels.
    pub width: u32,
    /// `appearance.rows`.
    pub rows: u32,
    /// `behaviour.hide_on_blur`.
    pub hide_on_blur: bool,
    /// `behaviour.escape_hides`.
    pub escape_hides: bool,
    /// `[[quicklinks]]`, in file order, keywords unique.
    pub quicklinks: Vec<Quicklink>,
}

/// One `[[quicklinks]]` entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Quicklink {
    /// Typed with a space to scope the query to this link.
    pub keyword: String,
    /// Shown as the item's title.
    pub name: String,
    /// Opened, with `{query}` replaced by the query, percent-encoded.
    pub url: String,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            hotkey: Chord::ALT_SPACE,
            width: 720,
            rows: 9,
            hide_on_blur: true,
            escape_hides: false,
            quicklinks: Vec::new(),
        }
    }
}

/// The result of reading `settings.toml`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Loaded {
    /// Every setting: from the file where it was valid, the default where
    /// it wasn't set or wasn't valid.
    pub settings: Settings,
    /// One line per problem, naming the key, what was wrong and what Flint
    /// used instead. Empty when the file is fine or absent.
    pub errors: Vec<String>,
    /// The file's modification time when it was read, to notice changes.
    /// `None` when there is no file.
    pub modified: Option<SystemTime>,
}

/// Reads `settings.toml` at `path`. A missing file is the defaults with
/// no errors: Flint needs no settings file to run.
pub fn load(path: &Path) -> Loaded {
    let modified = std::fs::metadata(path).and_then(|m| m.modified()).ok();
    match std::fs::read_to_string(path) {
        Ok(text) => Loaded {
            modified,
            ..parse(&text)
        },
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Loaded {
            settings: Settings::default(),
            errors: Vec::new(),
            modified: None,
        },
        Err(e) => Loaded {
            settings: Settings::default(),
            errors: vec![format!(
                "settings.toml couldn't be read ({e}); using the defaults"
            )],
            modified,
        },
    }
}

/// Reads settings from TOML text.
pub fn parse(text: &str) -> Loaded {
    let mut settings = Settings::default();
    let mut errors = Vec::new();
    let table: toml::Table = match text.parse() {
        Ok(table) => table,
        Err(e) => {
            let line = e
                .span()
                .map(|span| text[..span.start.min(text.len())].lines().count().max(1));
            let place = line.map(|n| format!(" line {n}")).unwrap_or_default();
            errors.push(format!(
                "settings.toml{place}: {}; using the defaults",
                e.message()
            ));
            return Loaded {
                settings,
                errors,
                modified: None,
            };
        }
    };
    let mut leaves = Vec::new();
    collect_leaves(&table, "", &mut leaves);
    for (key, value) in leaves {
        let Some(setting) = schema::find(&key) else {
            errors.push(format!("`{key}` isn't a setting; it is ignored"));
            continue;
        };
        match setting.apply(&mut settings, value) {
            Ok(dropped) => errors.extend(
                dropped
                    .into_iter()
                    .map(|why| format!("`{key}` {why}; it is ignored")),
            ),
            Err(why) => errors.push(format!("`{key}`: {why}; using {}", setting.default)),
        }
    }
    Loaded {
        settings,
        errors,
        modified: None,
    }
}

/// Every key that isn't a table, with its dotted path. Arrays of tables
/// (`[[quicklinks]]`) are leaves: the setting checks its entries.
fn collect_leaves<'a>(
    table: &'a toml::Table,
    prefix: &str,
    out: &mut Vec<(String, &'a toml::Value)>,
) {
    for (key, value) in table {
        let path = if prefix.is_empty() {
            key.clone()
        } else {
            format!("{prefix}.{key}")
        };
        match value {
            toml::Value::Table(inner) => collect_leaves(inner, &path, out),
            other => out.push((path, other)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_empty_file_is_the_defaults() {
        let loaded = parse("");
        assert_eq!(loaded.settings, Settings::default());
        assert!(loaded.errors.is_empty());
    }

    #[test]
    fn valid_values_are_used() {
        let loaded = parse(
            "[hotkey]\ntoggle = \"Ctrl+Shift+Space\"\n\
             [appearance]\nwidth = 900\nrows = 5\n\
             [behaviour]\nhide_on_blur = false\n",
        );
        assert!(loaded.errors.is_empty(), "{:?}", loaded.errors);
        assert_eq!(loaded.settings.hotkey.to_string(), "Ctrl+Shift+Space");
        assert_eq!(loaded.settings.width, 900);
        assert_eq!(loaded.settings.rows, 5);
        assert!(!loaded.settings.hide_on_blur);
    }

    #[test]
    fn a_bad_value_costs_only_that_setting() {
        let loaded =
            parse("appearance.width = 5000\nappearance.rows = 4\nhotkey.toggle = \"Alt+Nope\"\n");
        assert_eq!(loaded.settings.width, 720);
        assert_eq!(loaded.settings.rows, 4);
        assert_eq!(loaded.settings.hotkey, Chord::ALT_SPACE);
        assert_eq!(loaded.errors.len(), 2, "{:?}", loaded.errors);
        assert!(loaded
            .errors
            .iter()
            .any(|e| e.contains("`appearance.width`")
                && e.contains("out of range")
                && e.contains("using 720")));
        assert!(loaded
            .errors
            .iter()
            .any(|e| e.contains("`hotkey.toggle`") && e.contains("Nope")));
        assert!(
            loaded.errors.iter().all(|e| !e.contains('\n')),
            "one line each"
        );
    }

    #[test]
    fn a_wrong_type_is_named() {
        let loaded = parse("[behaviour]\nhide_on_blur = \"yes\"\n");
        assert!(
            loaded.errors[0].contains("expected `true` or `false`, found string"),
            "{:?}",
            loaded.errors
        );
    }

    #[test]
    fn unknown_keys_are_reported_not_fatal() {
        let loaded = parse("[appearance]\nwidht = 800\n[[leader]]\nkeys = \"p\"\n");
        assert_eq!(loaded.settings, Settings::default());
        assert!(loaded
            .errors
            .iter()
            .any(|e| e.contains("`appearance.widht`")));
        assert!(loaded.errors.iter().any(|e| e.contains("`leader`")));
    }

    #[test]
    fn a_bad_quicklink_is_dropped_with_its_reason() {
        let loaded = parse(
            "[[quicklinks]]
keyword = \"j\"
name = \"Jira\"
url = \"https://j/{query}\"
             [[quicklinks]]
keyword = \"w\"
url = \"https://w\"
",
        );
        assert_eq!(loaded.settings.quicklinks.len(), 1);
        assert_eq!(
            loaded.errors,
            ["`quicklinks` entry 2: no `name`; it is ignored"]
        );
    }

    #[test]
    fn a_syntax_error_names_the_line_and_keeps_the_defaults() {
        let loaded = parse("[appearance]\nwidth = 800\nrows = = 3\n");
        assert_eq!(loaded.settings, Settings::default());
        assert_eq!(loaded.errors.len(), 1);
        assert!(
            loaded.errors[0].starts_with("settings.toml line 3:"),
            "{}",
            loaded.errors[0]
        );
        assert!(!loaded.errors[0].contains('\n'));
    }

    #[test]
    fn a_missing_file_is_silent() {
        let loaded = load(Path::new("definitely/not/here/settings.toml"));
        assert_eq!(loaded.settings, Settings::default());
        assert!(loaded.errors.is_empty());
        assert!(loaded.modified.is_none());
    }
}
