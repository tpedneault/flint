//! Finding the `fenix` program: `fenix.executable` when set, else `fenix`
//! on the `PATH`, else beside `flint` itself (a portable install keeps the
//! two side by side).

use std::path::{Path, PathBuf};

/// The program's file name on this platform.
const NAME: &str = if cfg!(windows) { "fenix.exe" } else { "fenix" };

/// Where `fenix` is. The error says what was looked for, in one line.
pub fn find(configured: &str) -> Result<PathBuf, String> {
    find_in(
        configured,
        std::env::var_os("PATH").as_deref(),
        std::env::current_exe()
            .ok()
            .as_deref()
            .and_then(Path::parent),
    )
}

/// [`find`] with the `PATH` and Flint's folder passed in, for tests.
pub fn find_in(
    configured: &str,
    path: Option<&std::ffi::OsStr>,
    beside: Option<&Path>,
) -> Result<PathBuf, String> {
    if !configured.trim().is_empty() {
        let configured = PathBuf::from(configured.trim());
        return if configured.is_file() {
            Ok(configured)
        } else {
            Err(format!(
                "fenix.executable is {}, which doesn't exist",
                configured.display()
            ))
        };
    }
    let on_path = path
        .into_iter()
        .flat_map(std::env::split_paths)
        .map(|dir| dir.join(NAME));
    let next_to_flint = beside.map(|dir| dir.join(NAME));
    on_path
        .chain(next_to_flint)
        .find(|candidate| candidate.is_file())
        .ok_or_else(|| "Fenix isn't on the PATH or beside Flint; set fenix.executable".to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("flint-locate-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn the_setting_wins_and_must_exist() {
        let dir = temp("setting");
        let exe = dir.join("my-fenix");
        std::fs::write(&exe, b"").unwrap();
        assert_eq!(find_in(exe.to_str().unwrap(), None, None).unwrap(), exe);
        let err = find_in("/no/such/fenix", None, None).unwrap_err();
        assert!(err.contains("doesn't exist"), "{err}");
    }

    #[test]
    fn the_path_comes_before_flints_own_folder() {
        let on_path = temp("path");
        let beside = temp("beside");
        std::fs::write(beside.join(NAME), b"").unwrap();
        let path = std::env::join_paths([&on_path]).unwrap();
        assert_eq!(
            find_in("", Some(&path), Some(&beside)).unwrap(),
            beside.join(NAME)
        );
        std::fs::write(on_path.join(NAME), b"").unwrap();
        assert_eq!(
            find_in("", Some(&path), Some(&beside)).unwrap(),
            on_path.join(NAME)
        );
    }

    #[test]
    fn nowhere_says_how_to_fix_it() {
        let err = find_in("", None, None).unwrap_err();
        assert!(err.ends_with("set fenix.executable"), "{err}");
    }
}
