//! Every setting, declared once: its key in `settings.toml`, what it
//! takes, its default and what it's for. Loading, checking and the table
//! in docs/CONFIGURATION.md all read this list, so they can't disagree.
//!
//! Only settings that do something are declared. Settings the design
//! names for later milestones (`behaviour.escape_hides` in M2,
//! `appearance.theme` in M3, `[[quicklinks]]`, `[leader]`, ...) join the
//! list with the behaviour, so `settings.toml` never accepts a key that
//! silently does nothing.

use toml::Value;

use crate::chord::Chord;
use crate::Settings;

/// What a setting takes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// `true` or `false`.
    Bool,
    /// A whole number in `min..=max`.
    Int {
        /// Smallest allowed.
        min: i64,
        /// Largest allowed.
        max: i64,
    },
    /// A key chord (`"Alt+Space"`).
    Chord,
}

impl Kind {
    fn describe(self) -> String {
        match self {
            Kind::Bool => "`true` or `false`".into(),
            Kind::Int { min, max } => format!("a whole number, {min}–{max}"),
            Kind::Chord => "a key chord, like `\"Ctrl+Shift+Space\"`".into(),
        }
    }
}

type Apply = fn(&mut Settings, &Value) -> Result<(), String>;

/// One setting.
pub struct Setting {
    /// Its dotted key: `appearance.width`.
    pub key: &'static str,
    /// What it takes.
    pub kind: Kind,
    /// Its default as it would be written in `settings.toml`.
    pub default: &'static str,
    /// What it's for, one or two sentences, for the docs.
    pub help: &'static str,
    apply: Apply,
}

impl std::fmt::Debug for Setting {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Setting").field("key", &self.key).finish()
    }
}

impl Setting {
    /// Checks `value` against the kind and stores it in `settings`. The
    /// error says what was wrong, without the key (the caller adds it).
    pub fn apply(&self, settings: &mut Settings, value: &Value) -> Result<(), String> {
        match (self.kind, value) {
            (Kind::Bool, Value::Boolean(_)) | (Kind::Chord, Value::String(_)) => {}
            (Kind::Int { min, max }, Value::Integer(n)) => {
                if !(min..=max).contains(n) {
                    return Err(format!("{n} is out of range ({min}–{max})"));
                }
            }
            (kind, value) => {
                return Err(format!(
                    "expected {}, found {}",
                    kind.describe(),
                    value.type_str()
                ))
            }
        }
        (self.apply)(settings, value)
    }
}

fn int(value: &Value) -> u32 {
    // `Setting::apply` has checked the range, which is within `u32`.
    value
        .as_integer()
        .and_then(|n| u32::try_from(n).ok())
        .unwrap_or_default()
}

/// Every setting, in the order the docs list them.
pub static SETTINGS: &[Setting] = &[
    Setting {
        key: "hotkey.toggle",
        kind: Kind::Chord,
        default: "\"Alt+Space\"",
        help: "The global hotkey that shows and hides Flint (Windows, and X11 \
               best effort). On Wayland there are no global hotkeys: bind \
               `flint toggle` in the desktop's shortcut settings instead \
               (docs/KDE.md). `Alt+Space` shadows Windows' own window-menu \
               shortcut while Flint runs.",
        apply: |s, v| {
            let text = v.as_str().unwrap_or_default();
            s.hotkey = text.parse::<Chord>()?;
            Ok(())
        },
    },
    Setting {
        key: "appearance.width",
        kind: Kind::Int {
            min: 400,
            max: 2000,
        },
        default: "720",
        help: "The window's width, in logical pixels (scaled with the display).",
        apply: |s, v| {
            s.width = int(v);
            Ok(())
        },
    },
    Setting {
        key: "appearance.rows",
        kind: Kind::Int { min: 1, max: 30 },
        default: "9",
        help: "How many results are shown at once; the window's height follows.",
        apply: |s, v| {
            s.rows = int(v);
            Ok(())
        },
    },
    Setting {
        key: "behaviour.hide_on_blur",
        kind: Kind::Bool,
        default: "true",
        help: "Hide Flint when it loses focus, as when you click another window.",
        apply: |s, v| {
            s.hide_on_blur = v.as_bool().unwrap_or(true);
            Ok(())
        },
    },
];

/// The setting with this dotted key.
pub fn find(key: &str) -> Option<&'static Setting> {
    SETTINGS.iter().find(|s| s.key == key)
}

/// Every setting as a Markdown table: the copy in docs/CONFIGURATION.md,
/// which a test keeps current.
pub fn markdown_table() -> String {
    let mut out = String::from("| Setting | Takes | Default | What it does |\n|---|---|---|---|\n");
    for s in SETTINGS {
        out.push_str(&format!(
            "| `{}` | {} | `{}` | {} |\n",
            s.key,
            s.kind.describe(),
            s.default,
            s.help
        ));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_default_is_accepted_and_is_what_settings_default_to() {
        for s in SETTINGS {
            let doc: toml::Table = format!("v = {}", s.default).parse().unwrap();
            let mut applied = Settings::default();
            s.apply(&mut applied, &doc["v"])
                .unwrap_or_else(|e| panic!("{}: {e}", s.key));
            assert_eq!(
                applied,
                Settings::default(),
                "{}'s default disagrees",
                s.key
            );
        }
    }

    #[test]
    fn keys_are_unique_and_dotted() {
        let mut keys: Vec<_> = SETTINGS.iter().map(|s| s.key).collect();
        keys.sort_unstable();
        keys.dedup();
        assert_eq!(keys.len(), SETTINGS.len());
        assert!(SETTINGS.iter().all(|s| s.key.split('.').count() == 2));
    }

    /// docs/CONFIGURATION.md lists every setting; this keeps it current.
    #[test]
    fn the_docs_table_is_current() {
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../docs/CONFIGURATION.md");
        let docs = std::fs::read_to_string(path)
            .unwrap_or_default()
            .replace('\r', "");
        let table = markdown_table();
        assert!(
            docs.contains(&table),
            "docs/CONFIGURATION.md's settings table is out of date; replace it with:\n\n{table}"
        );
    }
}
