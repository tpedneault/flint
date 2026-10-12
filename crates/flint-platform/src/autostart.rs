//! Starting Flint at login: `flint autostart on|off`.
//!
//! Opt-in, per user, no admin rights, and nothing that outlives turning it
//! off (DESIGN.md, "Autostart"):
//!
//! - Windows: a `Flint` value under
//!   `HKCU\Software\Microsoft\Windows\CurrentVersion\Run` with the quoted
//!   path of this `flint.exe`. No scheduled task and no service: the `Run`
//!   key is what Task Manager's Startup page lists and lets the user
//!   disable, so Flint shows up where people look for it.
//! - Linux: `flint.desktop` in `$XDG_CONFIG_HOME/autostart` (else
//!   `~/.config/autostart`), the XDG autostart spec Plasma follows; it is
//!   what System Settings → Autostart lists.
//!
//! Either way the command is `flint` with no arguments, which starts the
//! daemon hidden. The path recorded is this executable's, so turning it on
//! from a different build (a development `target\debug\flint.exe`) points
//! login at that build; `flint autostart` reports the recorded path.

use std::path::{Path, PathBuf};

/// Where autostart stands.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum State {
    /// Flint starts at login, with this command.
    On(String),
    /// It doesn't.
    Off,
}

/// Makes `exe` start at login, replacing any earlier entry.
pub fn enable(exe: &Path) -> Result<(), String> {
    #[cfg(windows)]
    {
        windows::set(&run_value(exe))
    }
    #[cfg(not(windows))]
    {
        let path = desktop_file().ok_or("no home folder to put the autostart entry in")?;
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)
                .map_err(|e| format!("couldn't create {}: {e}", dir.display()))?;
        }
        std::fs::write(&path, desktop_entry(exe))
            .map_err(|e| format!("couldn't write {}: {e}", path.display()))
    }
}

/// Stops Flint starting at login. Already off is fine.
pub fn disable() -> Result<(), String> {
    #[cfg(windows)]
    {
        windows::delete()
    }
    #[cfg(not(windows))]
    {
        let Some(path) = desktop_file() else {
            return Ok(());
        };
        match std::fs::remove_file(&path) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(format!("couldn't remove {}: {e}", path.display())),
        }
    }
}

/// Whether Flint starts at login, and with what.
pub fn state() -> Result<State, String> {
    #[cfg(windows)]
    {
        Ok(windows::get()?.map_or(State::Off, State::On))
    }
    #[cfg(not(windows))]
    {
        let Some(path) = desktop_file() else {
            return Ok(State::Off);
        };
        match std::fs::read_to_string(&path) {
            Ok(text) => Ok(exec_line(&text).map_or(State::Off, State::On)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(State::Off),
            Err(e) => Err(format!("couldn't read {}: {e}", path.display())),
        }
    }
}

/// The `Run` value for `exe`: its path, quoted (it may contain spaces).
pub fn run_value(exe: &Path) -> String {
    format!("\"{}\"", exe.display())
}

/// The autostart `.desktop` entry for `exe`. `Exec` quotes the path as
/// the desktop entry spec asks, escaping `"`, `` ` ``, `$` and `\`.
pub fn desktop_entry(exe: &Path) -> String {
    let mut quoted = String::from('"');
    for c in exe.display().to_string().chars() {
        if matches!(c, '"' | '`' | '$' | '\\') {
            quoted.push('\\');
        }
        quoted.push(c);
    }
    quoted.push('"');
    format!(
        "[Desktop Entry]\n\
         Type=Application\n\
         Name=Flint\n\
         Comment=Keyboard-first launcher, started hidden at login\n\
         Exec={quoted}\n\
         Terminal=false\n\
         X-GNOME-Autostart-enabled=true\n"
    )
}

/// The `Exec` line of a desktop entry, if it has one.
pub fn exec_line(entry: &str) -> Option<String> {
    entry
        .lines()
        .find_map(|line| line.strip_prefix("Exec="))
        .map(|exec| exec.trim().to_owned())
}

/// `flint.desktop` in the XDG autostart folder.
#[cfg_attr(windows, allow(dead_code))]
fn desktop_file() -> Option<PathBuf> {
    let config = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")))?;
    Some(config.join("autostart").join("flint.desktop"))
}

#[cfg(windows)]
mod windows {
    use std::ptr;

    use windows_sys::Win32::Foundation::{ERROR_FILE_NOT_FOUND, ERROR_SUCCESS};
    use windows_sys::Win32::System::Registry::{
        RegDeleteKeyValueW, RegGetValueW, RegSetKeyValueW, HKEY_CURRENT_USER, REG_SZ, RRF_RT_REG_SZ,
    };

    const KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
    const NAME: &str = "Flint";

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(Some(0)).collect()
    }

    fn error(code: u32) -> String {
        std::io::Error::from_raw_os_error(code as i32).to_string()
    }

    pub fn set(value: &str) -> Result<(), String> {
        let (key, name, data) = (wide(KEY), wide(NAME), wide(value));
        // SAFETY: every pointer is a NUL-terminated buffer that outlives
        // the call, and the size given is the data's in bytes.
        let code = unsafe {
            RegSetKeyValueW(
                HKEY_CURRENT_USER,
                key.as_ptr(),
                name.as_ptr(),
                REG_SZ,
                data.as_ptr().cast(),
                (data.len() * 2) as u32,
            )
        };
        if code != ERROR_SUCCESS {
            return Err(format!("couldn't write the Run key: {}", error(code)));
        }
        Ok(())
    }

    pub fn delete() -> Result<(), String> {
        let (key, name) = (wide(KEY), wide(NAME));
        // SAFETY: NUL-terminated buffers that outlive the call.
        let code = unsafe { RegDeleteKeyValueW(HKEY_CURRENT_USER, key.as_ptr(), name.as_ptr()) };
        match code {
            ERROR_SUCCESS | ERROR_FILE_NOT_FOUND => Ok(()),
            code => Err(format!("couldn't remove the Run value: {}", error(code))),
        }
    }

    pub fn get() -> Result<Option<String>, String> {
        let (key, name) = (wide(KEY), wide(NAME));
        let mut size: u32 = 0;
        // SAFETY: a first call with no buffer asks for the size.
        let code = unsafe {
            RegGetValueW(
                HKEY_CURRENT_USER,
                key.as_ptr(),
                name.as_ptr(),
                RRF_RT_REG_SZ,
                ptr::null_mut(),
                ptr::null_mut(),
                &mut size,
            )
        };
        match code {
            ERROR_SUCCESS => {}
            ERROR_FILE_NOT_FOUND => return Ok(None),
            code => return Err(format!("couldn't read the Run key: {}", error(code))),
        }
        let mut data = vec![0u16; (size as usize).div_ceil(2)];
        // SAFETY: `data` holds `size` bytes, as the second call is told.
        let code = unsafe {
            RegGetValueW(
                HKEY_CURRENT_USER,
                key.as_ptr(),
                name.as_ptr(),
                RRF_RT_REG_SZ,
                ptr::null_mut(),
                data.as_mut_ptr().cast(),
                &mut size,
            )
        };
        if code != ERROR_SUCCESS {
            return Err(format!("couldn't read the Run key: {}", error(code)));
        }
        let end = data.iter().position(|&c| c == 0).unwrap_or(data.len());
        Ok(Some(String::from_utf16_lossy(&data[..end])))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Turning autostart on or off for real would change the login of
    // whoever runs the tests; only the values written are tested here.

    #[test]
    fn the_run_value_is_the_quoted_path() {
        assert_eq!(
            run_value(Path::new(r"C:\Program Files\Flint\flint.exe")),
            r#""C:\Program Files\Flint\flint.exe""#
        );
    }

    #[test]
    fn the_desktop_entry_quotes_its_exec_as_the_spec_asks() {
        let entry = desktop_entry(Path::new("/home/me/my $tools/flint"));
        assert!(entry.starts_with("[Desktop Entry]\nType=Application\n"));
        assert_eq!(
            exec_line(&entry).as_deref(),
            Some(r#""/home/me/my \$tools/flint""#)
        );
        assert!(entry.contains("\nTerminal=false\n"));
        assert_eq!(exec_line("[Desktop Entry]\nName=x\n"), None);
    }
}
