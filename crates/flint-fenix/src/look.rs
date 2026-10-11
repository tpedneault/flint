//! Fenix's look: `editor.theme` and `editor.font_family` from Fenix's
//! `settings.toml`, so Flint can follow them.
//!
//! Only those two keys are read. Fenix's settings also hold Jira, GitLab
//! and GitHub tokens, and Flint never reads tokens out of another
//! program's file (DESIGN.md, Security; F12): the file is parsed, these
//! two strings are taken, and the rest is dropped untouched. A file that
//! is missing or isn't valid TOML is "no preference": Fenix itself would
//! be showing its defaults then.

use std::path::Path;

/// What Fenix is drawn with, as its settings say.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Look {
    /// `editor.theme`, when set.
    pub theme: Option<String>,
    /// `editor.font_family`, when set.
    pub font_family: Option<String>,
}

/// Reads `editor.theme` and `editor.font_family` from Fenix's settings
/// file at `path`.
pub fn read(path: &Path) -> Look {
    let Ok(text) = std::fs::read_to_string(path) else {
        return Look::default();
    };
    let Ok(table) = text.parse::<toml::Table>() else {
        return Look::default();
    };
    let editor = table.get("editor").and_then(|e| e.as_table());
    let get = |key: &str| {
        editor
            .and_then(|e| e.get(key))
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|v| !v.is_empty())
            .map(str::to_owned)
    };
    Look {
        theme: get("theme"),
        font_family: get("font_family"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_fixture_names_its_theme_and_font() {
        let path =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/fenix-home/settings.toml");
        assert_eq!(
            read(&path),
            Look {
                theme: Some("Visual Studio Dark".into()),
                font_family: Some("Consolas".into()),
            }
        );
    }

    #[test]
    fn missing_or_broken_settings_are_no_preference() {
        assert_eq!(read(Path::new("not/here/settings.toml")), Look::default());
        let dir = std::env::temp_dir().join(format!("flint-look-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("settings.toml"), "[editor\ntheme = ").unwrap();
        assert_eq!(read(&dir.join("settings.toml")), Look::default());
        std::fs::write(
            dir.join("settings.toml"),
            "[editor]\ntheme = \"  \"\nfont_family = 3\n",
        )
        .unwrap();
        assert_eq!(read(&dir.join("settings.toml")), Look::default());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
