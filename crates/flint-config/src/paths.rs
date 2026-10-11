//! Where Flint keeps things. Two kinds, mirroring Fenix's layout under
//! `flint` instead of `fenix`:
//!
//! - **settings**: what you chose (`settings.toml`). Roams with your
//!   profile on Windows (`%AppData%\flint`); `~/.config/flint` on Linux.
//! - **local**: what Flint noticed about this machine (`state\`, from M2's
//!   frecency) and its logs (`logs\`). Never roams
//!   (`%LocalAppData%\flint`, `~/.local/state/flint`).
//!
//! `FLINT_HOME` puts both in one folder: a portable install, or a test run
//! that mustn't touch the real ones.
//!
//! These are `fenix_storage::paths::Roots`'s rules, copied, because that
//! module hard-codes `fenix` and `FENIX_HOME`; F14 in
//! FENIX-INTEGRATION.md would let both apps share one implementation.
//! Fenix's `data` folder has no Flint counterpart yet: Flint makes nothing
//! a person would call their data.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

/// The environment variable that makes Flint portable.
pub const HOME_VAR: &str = "FLINT_HOME";

/// Flint's folders, resolved.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Folders {
    /// Holds `settings.toml`.
    pub settings: PathBuf,
    /// Holds `state` and `logs`.
    pub local: PathBuf,
}

impl Folders {
    /// Both under `home`.
    pub fn portable(home: &Path) -> Self {
        Folders {
            settings: home.to_path_buf(),
            local: home.to_path_buf(),
        }
    }

    /// The platform's own folders; `None` where it has none (no home
    /// directory at all).
    pub fn platform() -> Option<Self> {
        let settings = dirs::config_dir()?.join("flint");
        let local = dirs::state_dir()
            .or_else(dirs::data_local_dir)
            .map(|d| d.join("flint"))
            .unwrap_or_else(|| settings.clone());
        // Where the local folder is the settings folder (macOS), keep them
        // apart by a subfolder, as Fenix does.
        let local = if local == settings {
            settings.join("local")
        } else {
            local
        };
        Some(Folders { settings, local })
    }

    /// `FLINT_HOME` when set and non-empty, else the platform's folders.
    pub fn current() -> Option<Self> {
        Self::resolve(std::env::var_os(HOME_VAR))
    }

    /// [`Folders::current`] with the variable's value passed in, so it
    /// can be tested without touching the process environment.
    pub fn resolve(home: Option<OsString>) -> Option<Self> {
        match home.filter(|v| !v.is_empty()) {
            Some(home) => Some(Self::portable(Path::new(&home))),
            None => Self::platform(),
        }
    }

    /// `settings.toml`.
    pub fn settings_file(&self) -> PathBuf {
        self.settings.join("settings.toml")
    }

    /// A state file, like `frecency.json`.
    pub fn state(&self, name: &str) -> PathBuf {
        self.local.join("state").join(name)
    }

    /// The log folder.
    pub fn logs(&self) -> PathBuf {
        self.local.join("logs")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_portable_home_holds_everything() {
        let f = Folders::resolve(Some("/p".into())).unwrap();
        assert_eq!(f.settings_file(), Path::new("/p/settings.toml"));
        assert_eq!(
            f.state("frecency.json"),
            Path::new("/p/state/frecency.json")
        );
        assert_eq!(f.logs(), Path::new("/p/logs"));
    }

    #[test]
    fn an_empty_home_means_the_platform_folders() {
        assert_eq!(Folders::resolve(Some("".into())), Folders::platform());
    }

    #[test]
    fn the_platform_keeps_settings_apart_from_this_machines_state() {
        let Some(f) = Folders::platform() else { return };
        assert!(f.settings.ends_with("flint"), "{}", f.settings.display());
        assert_ne!(f.local, f.settings);
        if cfg!(windows) {
            assert!(
                f.settings.to_string_lossy().contains("Roaming"),
                "{}",
                f.settings.display()
            );
            assert!(
                f.local.to_string_lossy().contains("Local"),
                "{}",
                f.local.display()
            );
        }
    }
}
