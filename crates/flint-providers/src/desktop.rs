//! Freedesktop `.desktop` files: the parts Flint needs to list and launch
//! applications on Linux, per the Desktop Entry Specification 1.5.
//!
//! Only the `[Desktop Entry]` group is read. An entry is skipped when it
//! isn't `Type=Application`, is `Hidden` or `NoDisplay`, is limited by
//! `OnlyShowIn`/`NotShowIn` to other desktops, or names a `TryExec`
//! program that isn't installed, which is what the spec asks of a menu.
//!
//! `Exec` becomes an argv here, never a shell string: the spec's quoting
//! rules are applied, field codes for files and URLs are dropped (Flint
//! launches without files), `%i`, `%c` and `%k` expand to the icon, name
//! and file path, and `%%` is a literal percent. Nothing here runs it.

use std::collections::HashMap;
use std::path::Path;

/// What Flint uses from a desktop entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DesktopEntry {
    /// `Name`, localised when a translation for the locale exists.
    pub name: String,
    /// `GenericName`, else `Comment`.
    pub description: Option<String>,
    /// `Keywords`, localised like the name.
    pub keywords: Vec<String>,
    /// `Exec`, as an argument vector.
    pub argv: Vec<String>,
    /// `Path`, the working directory.
    pub cwd: Option<String>,
    /// `Terminal=true`.
    pub terminal: bool,
}

/// Parses the text of a desktop file. `None` when the entry shouldn't be
/// listed; `locale` is `LANG`'s language part (`de_DE`), `desktops` is
/// `XDG_CURRENT_DESKTOP` split on `:`, and `installed` says whether a
/// `TryExec` program exists.
pub fn parse(
    text: &str,
    path: &Path,
    locale: Option<&str>,
    desktops: &[String],
    installed: impl Fn(&str) -> bool,
) -> Option<DesktopEntry> {
    let group = main_group(text);
    let get = |key: &str| group.get(key).map(|v| unescape(v));
    let localised = |key: &str| -> Option<String> {
        if let Some(locale) = locale {
            let lang = locale.split(['_', '.', '@']).next().unwrap_or(locale);
            let country = locale.split('.').next().unwrap_or(locale);
            for candidate in [country, lang] {
                if let Some(v) = group.get(&format!("{key}[{candidate}]")) {
                    return Some(unescape(v));
                }
            }
        }
        get(key)
    };
    let truthy = |key: &str| get(key).is_some_and(|v| v == "true");

    if get("Type").as_deref() != Some("Application") || truthy("Hidden") || truthy("NoDisplay") {
        return None;
    }
    let list = |key: &str| -> Vec<String> {
        get(key)
            .map(|v| {
                v.split(';')
                    .filter(|s| !s.is_empty())
                    .map(str::to_owned)
                    .collect()
            })
            .unwrap_or_default()
    };
    let only = list("OnlyShowIn");
    if !only.is_empty() && !only.iter().any(|d| desktops.contains(d)) {
        return None;
    }
    if list("NotShowIn").iter().any(|d| desktops.contains(d)) {
        return None;
    }
    if let Some(program) = get("TryExec").filter(|p| !p.is_empty()) {
        if !installed(&program) {
            return None;
        }
    }
    let name = localised("Name").filter(|n| !n.is_empty())?;
    let icon = get("Icon");
    let argv = parse_exec(&get("Exec")?, &name, icon.as_deref(), path).ok()?;
    if argv.is_empty() {
        return None;
    }
    Some(DesktopEntry {
        description: localised("GenericName")
            .or_else(|| localised("Comment"))
            .filter(|d| !d.is_empty()),
        keywords: localised("Keywords")
            .map(|v| {
                v.split(';')
                    .filter(|s| !s.is_empty())
                    .map(str::to_owned)
                    .collect()
            })
            .unwrap_or_default(),
        name,
        argv,
        cwd: get("Path").filter(|p| !p.is_empty()),
        terminal: truthy("Terminal"),
    })
}

/// The `[Desktop Entry]` group's keys, raw.
fn main_group(text: &str) -> HashMap<String, String> {
    let mut group = HashMap::new();
    let mut inside = false;
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if line.starts_with('[') {
            inside = line == "[Desktop Entry]";
            continue;
        }
        if inside {
            if let Some((key, value)) = line.split_once('=') {
                group
                    .entry(key.trim().to_owned())
                    .or_insert_with(|| value.trim().to_owned());
            }
        }
    }
    group
}

/// The spec's escapes for string values: `\s \n \t \r \\`.
fn unescape(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    let mut chars = value.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('s') => out.push(' '),
            Some('n') => out.push('\n'),
            Some('t') => out.push('\t'),
            Some('r') => out.push('\r'),
            Some('\\') => out.push('\\'),
            // Not a string-value escape: keep it for `Exec`'s own quoting.
            Some(other) => {
                out.push('\\');
                out.push(other);
            }
            None => out.push('\\'),
        }
    }
    out
}

/// Splits an `Exec` value into arguments and expands its field codes.
pub fn parse_exec(
    exec: &str,
    name: &str,
    icon: Option<&str>,
    path: &Path,
) -> Result<Vec<String>, String> {
    let mut args = Vec::new();
    let mut current = String::new();
    let mut started = false;
    let mut quoted = false;
    let mut chars = exec.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '"' => {
                quoted = !quoted;
                started = true;
            }
            '\\' if quoted => match chars.next() {
                Some(e @ ('"' | '`' | '$' | '\\')) => current.push(e),
                _ => return Err("a backslash in quotes must escape \" ` $ or \\".into()),
            },
            ' ' | '\t' if !quoted => {
                if started {
                    args.push(Arg::Text(std::mem::take(&mut current)));
                    started = false;
                }
            }
            '%' if !quoted => {
                let code = chars.next().ok_or("`%` at the end of Exec")?;
                if code == '%' {
                    current.push('%');
                    started = true;
                } else if started {
                    // A code inside a word, like `--file=%f`: only `%%`
                    // is meaningful there; drop the code.
                } else {
                    // Standalone: it is the whole argument.
                    if chars.peek().is_some_and(|c| *c != ' ' && *c != '\t') {
                        return Err(format!("field code `%{code}` must stand alone"));
                    }
                    args.push(Arg::Code(code));
                }
            }
            other => {
                current.push(other);
                started = true;
            }
        }
    }
    if quoted {
        return Err("unterminated quote in Exec".into());
    }
    if started {
        args.push(Arg::Text(current));
    }
    let mut argv = Vec::new();
    for arg in args {
        match arg {
            Arg::Text(text) => argv.push(text),
            Arg::Code('i') => {
                if let Some(icon) = icon.filter(|i| !i.is_empty()) {
                    argv.push("--icon".into());
                    argv.push(icon.into());
                }
            }
            Arg::Code('c') => argv.push(name.into()),
            Arg::Code('k') => argv.push(path.display().to_string()),
            // Files, URLs and deprecated codes: launched without any.
            Arg::Code(_) => {}
        }
    }
    Ok(argv)
}

enum Arg {
    Text(String),
    Code(char),
}

#[cfg(test)]
mod tests {
    use super::*;

    fn exec(text: &str) -> Result<Vec<String>, String> {
        parse_exec(
            text,
            "Firefox",
            Some("firefox"),
            Path::new("/apps/firefox.desktop"),
        )
    }

    fn entry(text: &str) -> Option<DesktopEntry> {
        parse(
            text,
            Path::new("/a.desktop"),
            Some("de_DE.UTF-8"),
            &["KDE".into()],
            |p| p != "missing",
        )
    }

    #[test]
    fn exec_splits_on_spaces_and_honours_quotes() {
        assert_eq!(exec("firefox %u").unwrap(), ["firefox"]);
        assert_eq!(
            exec("\"/opt/My App/run\" --x").unwrap(),
            ["/opt/My App/run", "--x"]
        );
        assert_eq!(
            exec(r#"sh -c "echo \"hi\" \$HOME""#).unwrap(),
            ["sh", "-c", r#"echo "hi" $HOME"#]
        );
        assert_eq!(exec("a  b\tc").unwrap(), ["a", "b", "c"]);
        assert_eq!(exec("prog \"\"").unwrap(), ["prog", ""]);
    }

    #[test]
    fn field_codes_expand_or_vanish() {
        assert_eq!(exec("app %F %U %f").unwrap(), ["app"]);
        assert_eq!(
            exec("app %i %c %k").unwrap(),
            [
                "app",
                "--icon",
                "firefox",
                "Firefox",
                "/apps/firefox.desktop"
            ]
        );
        assert_eq!(exec("app 100%%").unwrap(), ["app", "100%"]);
        assert_eq!(exec("app --file=%f").unwrap(), ["app", "--file="]);
    }

    #[test]
    fn broken_exec_lines_are_refused() {
        assert!(exec("app \"unterminated").is_err());
        assert!(exec("app %").is_err());
        assert!(exec(r#"app "\q""#).is_err());
        assert!(exec("app %fx").is_err());
    }

    #[test]
    fn a_full_entry_with_translations() {
        let e = entry(
            "# comment\n[Desktop Entry]\nType=Application\nName=Files\nName[de]=Dateien\n\
             GenericName=File Manager\nKeywords=folder;manager;\nExec=dolphin %u\nPath=/home\nTerminal=false\n\
             [Desktop Action new]\nName=New Window\nExec=dolphin --new\n",
        )
        .unwrap();
        assert_eq!(e.name, "Dateien");
        assert_eq!(e.description.as_deref(), Some("File Manager"));
        assert_eq!(e.keywords, ["folder", "manager"]);
        assert_eq!(e.argv, ["dolphin"]);
        assert_eq!(e.cwd.as_deref(), Some("/home"));
        assert!(!e.terminal);
    }

    #[test]
    fn entries_a_menu_wouldnt_show_are_skipped() {
        let base = "[Desktop Entry]\nType=Application\nName=X\nExec=x\n";
        assert!(entry(base).is_some());
        for extra in [
            "NoDisplay=true",
            "Hidden=true",
            "OnlyShowIn=GNOME;",
            "NotShowIn=KDE;",
            "TryExec=missing",
        ] {
            assert!(entry(&format!("{base}{extra}\n")).is_none(), "{extra}");
        }
        assert!(entry(&format!("{base}OnlyShowIn=GNOME;KDE;\n")).is_some());
        assert!(entry("[Desktop Entry]\nType=Link\nName=X\nURL=https://x\n").is_none());
        assert!(
            entry("[Desktop Entry]\nType=Application\nName=X\n").is_none(),
            "no Exec"
        );
    }

    #[test]
    fn string_escapes_apply_before_exec_quoting() {
        let e = entry(
            "[Desktop Entry]\nType=Application\nName=A\\sB\nExec=\"/opt/x y/bin\"\nTerminal=true\n",
        )
        .unwrap();
        assert_eq!(e.name, "A B");
        assert_eq!(e.argv, ["/opt/x y/bin"]);
        assert!(e.terminal);
    }
}
