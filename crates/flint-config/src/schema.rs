//! Every setting, declared once: its key in `settings.toml`, what it
//! takes, its default and what it's for. Loading, checking and the table
//! in docs/CONFIGURATION.md all read this list, so they can't disagree.
//!
//! Only settings that do something are declared. Settings the design
//! names for later milestones join the list with the
//! behaviour, so `settings.toml` never accepts a key that silently does
//! nothing.
//!
//! A list of records (`[[quicklinks]]`) is checked entry by entry: a bad
//! entry is dropped with its reason and the others are kept, the same
//! "a bad value costs only itself" rule scalars follow.

use toml::Value;

use crate::chord::Chord;
use crate::{ProviderConfig, Quicklink, Settings, THEMES};

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
    /// An array of tables (`[[quicklinks]]`) with these fields.
    Records(&'static [&'static str]),
    /// One of these words, in any case; stored as written here.
    Choice(&'static [&'static str]),
    /// Any text; empty means "not set".
    Text,
    /// A table of key sequences (`[leader]`).
    Leader,
}

impl Kind {
    fn describe(self) -> String {
        match self {
            Kind::Bool => "`true` or `false`".into(),
            Kind::Int { min, max } => format!("a whole number, {min}–{max}"),
            Kind::Chord => "a key chord, like `\"Ctrl+Shift+Space\"`".into(),
            Kind::Records(fields) => {
                let fields: Vec<String> = fields.iter().map(|f| format!("`{f}`")).collect();
                format!("entries with {}", fields.join(", "))
            }
            Kind::Choice(choices) => {
                let choices: Vec<String> = choices.iter().map(|c| format!("`\"{c}\"`")).collect();
                format!("one of {}", choices.join(", "))
            }
            Kind::Text => "text".into(),
            Kind::Leader => "a table of key sequences and actions".into(),
        }
    }
}

/// Stores a checked value. `Ok` carries problems with single entries of
/// a list (each already worded, the rest of the list kept); `Err` rejects
/// the whole value, which then keeps its default.
type Apply = fn(&mut Settings, &Value) -> Result<Vec<String>, String>;

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
    /// error says what was wrong, without the key (the caller adds it);
    /// `Ok` lists entries of a list that were dropped, and why.
    pub fn apply(&self, settings: &mut Settings, value: &Value) -> Result<Vec<String>, String> {
        match (self.kind, value) {
            (Kind::Bool, Value::Boolean(_)) | (Kind::Chord, Value::String(_)) => {}
            (Kind::Records(_), Value::Array(_))
            | (Kind::Text, Value::String(_))
            | (Kind::Leader, Value::Table(_)) => {}
            (Kind::Choice(choices), Value::String(text)) => {
                if !choices.iter().any(|c| c.eq_ignore_ascii_case(text)) {
                    return Err(format!("`{text}` isn't {}", self.kind.describe()));
                }
            }
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

/// The choice `value` names, spelled as the setting lists it.
fn choice(value: &Value, choices: &[&'static str]) -> &'static str {
    let text = value.as_str().unwrap_or_default();
    choices
        .iter()
        .find(|c| c.eq_ignore_ascii_case(text))
        .copied()
        .unwrap_or(choices[0])
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
            Ok(vec![])
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
            Ok(vec![])
        },
    },
    Setting {
        key: "appearance.rows",
        kind: Kind::Int { min: 1, max: 30 },
        default: "9",
        help: "How many results are shown at once; the window's height follows.",
        apply: |s, v| {
            s.rows = int(v);
            Ok(vec![])
        },
    },
    Setting {
        key: "appearance.theme",
        kind: Kind::Choice(&THEMES),
        default: "\"fenix\"",
        help: "The colours. `fenix` follows Fenix's `editor.theme` (and \
               `editor.font_family`), changing when Fenix's does; a theme's \
               name pins it. Orbit Dark when Fenix isn't installed or names a \
               theme Flint doesn't have.",
        apply: |s, v| {
            s.theme = choice(v, &THEMES).to_owned();
            Ok(vec![])
        },
    },
    Setting {
        key: "behaviour.hide_on_blur",
        kind: Kind::Bool,
        default: "true",
        help: "Hide Flint when it loses focus, as when you click another window.",
        apply: |s, v| {
            s.hide_on_blur = v.as_bool().unwrap_or(true);
            Ok(vec![])
        },
    },
    Setting {
        key: "behaviour.escape_hides",
        kind: Kind::Bool,
        default: "false",
        help: "Make `Esc` hide Flint from either mode, as Raycast does. By \
               default `Esc` in Insert mode goes to Normal mode, where a second \
               `Esc` (or `q`) hides.",
        apply: |s, v| {
            s.escape_hides = v.as_bool().unwrap_or(false);
            Ok(vec![])
        },
    },
    Setting {
        key: "fenix.executable",
        kind: Kind::Text,
        default: "\"\"",
        help: "The `fenix` program Flint starts when no Fenix is running to \
               ask (a running one is reached over its control channel). Empty: \
               `fenix` on the `PATH`, else beside the `flint` program.",
        apply: |s, v| {
            s.fenix_executable = v.as_str().unwrap_or_default().trim().to_owned();
            Ok(vec![])
        },
    },
    Setting {
        key: "shell.program",
        kind: Kind::Text,
        default: "\"\"",
        help: "The shell that runs what you type after `> ` (`pwsh`, `cmd`, `bash`, a full \
               path). Empty: `pwsh` if it is on the `PATH`, else Windows PowerShell; on \
               Linux, `$SHELL`, else `/bin/sh`.",
        apply: |s, v| {
            s.shell_program = v.as_str().unwrap_or_default().trim().to_owned();
            Ok(vec![])
        },
    },
    Setting {
        key: "leader",
        kind: Kind::Leader,
        default: "see below",
        help: "What `SPC` sequences do in Normal mode: `\"f r\" = \"scope:fenix.recent\"`. \
               An action is `scope:<provider>`, `flint:<command>`, `fenix:<command>` or \
               `query:<text>`; a table `{ action = ..., label = ... }` adds a label, and \
               `{ label = ... }` names a group. Without the table, the defaults below apply; \
               with it, the table is the whole map.",
        apply: |s, v| {
            let (bindings, problems) = crate::leader::parse(v)?;
            s.leader = bindings;
            Ok(problems)
        },
    },
    Setting {
        key: "providers",
        kind: Kind::Records(&["name", "command", "connect", "prefix", "credential"]),
        default: "[]",
        help: "External providers (docs/PROVIDERS.md). `command` is the program and its \
               arguments, as a list, started by Flint in the settings folder and talked \
               to over its stdin and stdout; `connect = true` instead waits for a running \
               program to connect to Flint itself. `prefix` overrides the provider's own \
               scope keyword. `credential` names a secret in the OS credential store \
               (`jira:https://jira.example.com`, stored with `flint credential set`) that \
               Flint gives the provider when it starts it. Only providers named here ever \
               run.",
        apply: |s, v| {
            let (providers, problems) = providers(v);
            s.providers = providers;
            Ok(problems)
        },
    },
    Setting {
        key: "quicklinks",
        kind: Kind::Records(&["keyword", "name", "url"]),
        default: "[]",
        help: "Links with a keyword: typing the keyword and a space scopes the \
               query to the link, and `Enter` opens `url` with `{query}` \
               replaced by what you typed, percent-encoded. A link without \
               `{query}` just opens. Written as `[[quicklinks]]` tables (see \
               the example above).",
        apply: |s, v| {
            let (links, problems) = quicklinks(v);
            s.quicklinks = links;
            Ok(problems)
        },
    },
];

/// Reads `[[quicklinks]]`, keeping the good entries and explaining the
/// others.
fn quicklinks(value: &Value) -> (Vec<Quicklink>, Vec<String>) {
    let mut links: Vec<Quicklink> = Vec::new();
    let mut problems = Vec::new();
    for (n, entry) in value.as_array().into_iter().flatten().enumerate() {
        let field = |name: &str| entry.get(name).and_then(Value::as_str).map(str::trim);
        let checked = (|| -> Result<Quicklink, String> {
            let keyword = field("keyword")
                .filter(|k| !k.is_empty())
                .ok_or("no `keyword`")?;
            if keyword.contains(char::is_whitespace) {
                return Err(format!("keyword `{keyword}` has a space in it"));
            }
            if links.iter().any(|l| l.keyword == keyword) {
                return Err(format!("keyword `{keyword}` is used by an earlier entry"));
            }
            let name = field("name").filter(|n| !n.is_empty()).ok_or("no `name`")?;
            let url = field("url").filter(|u| !u.is_empty()).ok_or("no `url`")?;
            if !has_scheme(url) {
                return Err(format!(
                    "url `{url}` doesn't start with a scheme like `https:`"
                ));
            }
            Ok(Quicklink {
                keyword: keyword.to_owned(),
                name: name.to_owned(),
                url: url.to_owned(),
            })
        })();
        match checked {
            Ok(link) => links.push(link),
            Err(why) => problems.push(format!("entry {}: {why}", n + 1)),
        }
    }
    (links, problems)
}

/// Reads `[[providers]]`, keeping the good entries and explaining the
/// others.
fn providers(value: &Value) -> (Vec<ProviderConfig>, Vec<String>) {
    let mut providers: Vec<ProviderConfig> = Vec::new();
    let mut problems = Vec::new();
    for (n, entry) in value.as_array().into_iter().flatten().enumerate() {
        let checked = (|| -> Result<ProviderConfig, String> {
            let name = entry
                .get("name")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|n| !n.is_empty())
                .ok_or("no `name`")?;
            if !name.chars().all(|c| {
                c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '.' | '_' | '-')
            }) {
                return Err(format!(
                    "name `{name}` may use only a-z, 0-9, `.`, `_` and `-`"
                ));
            }
            if providers.iter().any(|p| p.name == name) {
                return Err(format!("name `{name}` is used by an earlier entry"));
            }
            let command: Option<Vec<String>> =
                match entry.get("command") {
                    None => None,
                    Some(Value::Array(parts)) => Some(
                        parts
                            .iter()
                            .map(|p| p.as_str().map(str::to_owned))
                            .collect::<Option<Vec<_>>>()
                            .filter(|c| c.first().is_some_and(|p| !p.trim().is_empty()))
                            .ok_or("`command` must be a list of strings, the program first")?,
                    ),
                    Some(_) => return Err(
                        "`command` must be a list like [\"python\", \"jira.py\"], never one string"
                            .into(),
                    ),
                };
            let connect = entry
                .get("connect")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            match (&command, connect) {
                (Some(_), true) => {
                    return Err("has both `command` and `connect = true`; pick one".into())
                }
                (None, false) => {
                    return Err(
                        "needs `command` (Flint starts it) or `connect = true` (it connects)"
                            .into(),
                    )
                }
                _ => {}
            }
            let prefix = match entry.get("prefix") {
                None => None,
                Some(Value::String(p))
                    if !p.trim().is_empty() && !p.trim().contains(char::is_whitespace) =>
                {
                    Some(p.trim().to_owned())
                }
                Some(_) => return Err("`prefix` must be one word".into()),
            };
            let credential = match entry.get("credential") {
                None => None,
                Some(Value::String(c)) => {
                    crate::check_credential_name(c)?;
                    if connect {
                        return Err(
                            "`credential` is for a provider Flint starts; one that connects \
                             has its own"
                                .into(),
                        );
                    }
                    Some(c.clone())
                }
                Some(_) => return Err("`credential` must be a name like \"jira:https://…\"".into()),
            };
            Ok(ProviderConfig {
                name: name.to_owned(),
                command: command.unwrap_or_default(),
                connect,
                prefix,
                credential,
            })
        })();
        match checked {
            Ok(provider) => providers.push(provider),
            Err(why) => problems.push(format!("entry {}: {why}", n + 1)),
        }
    }
    (providers, problems)
}

/// `scheme:` per RFC 3986: a letter, then letters, digits, `+`, `-`, `.`.
fn has_scheme(url: &str) -> bool {
    let Some((scheme, _)) = url.split_once(':') else {
        return false;
    };
    let mut chars = scheme.chars();
    chars.next().is_some_and(|c| c.is_ascii_alphabetic())
        && chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'))
}

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
            if s.kind == Kind::Leader {
                continue; // its default is a list in the docs, not a value
            }
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
        assert!(SETTINGS
            .iter()
            .filter(|s| !matches!(s.kind, Kind::Records(_) | Kind::Leader))
            .all(|s| s.key.split('.').count() == 2));
    }

    #[test]
    fn quicklinks_keep_good_entries_and_explain_bad_ones() {
        let doc: toml::Table = r#"
            [[quicklinks]]
            keyword = "j"
            name = "Jira issue"
            url = "https://jira.example.com/browse/{query}"
            [[quicklinks]]
            keyword = "j"
            name = "Again"
            url = "https://x"
            [[quicklinks]]
            keyword = "my space"
            name = "x"
            url = "https://x"
            [[quicklinks]]
            keyword = "w"
            name = "Outlook"
            url = "outlook.office.com"
            [[quicklinks]]
            keyword = "mr"
            url = "https://gitlab/merge_requests?search={query}"
            [[quicklinks]]
            keyword = "m"
            name = "Mail"
            url = "mailto:me@example.com"
        "#
        .parse()
        .unwrap();
        let (links, problems) = quicklinks(&doc["quicklinks"]);
        let keywords: Vec<&str> = links.iter().map(|l| l.keyword.as_str()).collect();
        assert_eq!(keywords, ["j", "m"]);
        assert_eq!(problems.len(), 4, "{problems:?}");
        assert!(problems[0].starts_with("entry 2: keyword `j` is used"));
        assert!(problems[1].contains("space"));
        assert!(problems[2].contains("scheme"));
        assert_eq!(problems[3], "entry 5: no `name`");
    }

    #[test]
    fn choices_are_matched_in_any_case_and_stored_as_listed() {
        let theme = find("appearance.theme").unwrap();
        let mut s = Settings::default();
        theme
            .apply(&mut s, &Value::String("visual studio DARK".into()))
            .unwrap();
        assert_eq!(s.theme, "Visual Studio Dark");
        let err = theme
            .apply(&mut s, &Value::String("Solarized Light".into()))
            .unwrap_err();
        assert!(
            err.starts_with("`Solarized Light` isn't one of `\"fenix\"`"),
            "{err}"
        );
    }

    #[test]
    fn providers_need_a_way_to_run_and_a_clean_name() {
        let doc: toml::Table = r#"
            [[providers]]
            name = "jira"
            command = ["flint-jira", "--verbose"]
            prefix = "j"
            credential = "jira:https://jira.example.com"
            [[providers]]
            name = "fenix-live"
            connect = true
            [[providers]]
            name = "Bad Name"
            command = ["x"]
            [[providers]]
            name = "shell"
            command = "python jira.py"
            [[providers]]
            name = "both"
            command = ["x"]
            connect = true
            [[providers]]
            name = "neither"
            [[providers]]
            name = "jira"
            command = ["again"]
            [[providers]]
            name = "badcred"
            command = ["x"]
            credential = "just-a-token"
        "#
        .parse()
        .unwrap();
        let (providers, problems) = providers(&doc["providers"]);
        let names: Vec<&str> = providers.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(names, ["jira", "fenix-live"]);
        assert_eq!(providers[0].command, ["flint-jira", "--verbose"]);
        assert!(providers[1].connect && providers[1].command.is_empty());
        assert_eq!(
            providers[0].credential.as_deref(),
            Some("jira:https://jira.example.com")
        );
        assert_eq!(problems.len(), 6, "{problems:?}");
        assert!(
            problems[5].contains("isn't a credential name"),
            "{problems:?}"
        );
        assert!(problems[1].contains("never one string"), "no shell strings");
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
