//! Locations of MarkInk's own files.

#[cfg(all(unix, not(target_os = "macos")))]
use std::ffi::OsString;
use std::path::PathBuf;

/// Linux and BSD: `$XDG_STATE_HOME/markink`, falling back to `~/.local/state/markink`.
#[cfg(all(unix, not(target_os = "macos")))]
pub fn state_dir() -> PathBuf {
    state_dir_from(std::env::var_os("XDG_STATE_HOME"), std::env::var_os("HOME"))
}

/// macOS: `~/Library/Application Support/MarkInk`. Windows: `%LOCALAPPDATA%\MarkInk`.
#[cfg(not(all(unix, not(target_os = "macos"))))]
pub fn state_dir() -> PathBuf {
    dirs::data_local_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("MarkInk")
}

#[cfg(all(unix, not(target_os = "macos")))]
fn state_dir_from(xdg: Option<OsString>, home: Option<OsString>) -> PathBuf {
    let base = match xdg.map(PathBuf::from) {
        Some(dir) if dir.is_absolute() => dir,
        _ => PathBuf::from(home.unwrap_or_else(|| ".".into())).join(".local/state"),
    };
    base.join("markink")
}

#[cfg(all(test, unix, not(target_os = "macos")))]
mod tests {
    use super::*;

    #[test]
    fn uses_xdg_state_home() {
        let dir = state_dir_from(Some("/x/state".into()), Some("/home/u".into()));
        assert_eq!(dir, PathBuf::from("/x/state/markink"));
    }

    #[test]
    fn falls_back_to_home() {
        let dir = state_dir_from(None, Some("/home/u".into()));
        assert_eq!(dir, PathBuf::from("/home/u/.local/state/markink"));
    }

    #[test]
    fn ignores_relative_xdg() {
        let dir = state_dir_from(Some("rel".into()), Some("/home/u".into()));
        assert_eq!(dir, PathBuf::from("/home/u/.local/state/markink"));
    }
}

#[cfg(all(test, not(all(unix, not(target_os = "macos")))))]
mod tests {
    #[test]
    fn uses_platform_data_dir() {
        assert!(super::state_dir().ends_with("MarkInk"));
    }
}
