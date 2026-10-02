//! Per-workspace session state (open tabs, cursors, expanded folders).

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::fsio;

pub const SESSION_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Session {
    pub version: u32,
    pub tabs: Vec<TabState>,
    pub active_tab: Option<usize>,
    pub expanded_dirs: Vec<PathBuf>,
    pub sidebar_visible: bool,
}

impl Default for Session {
    fn default() -> Self {
        Self {
            version: SESSION_VERSION,
            tabs: Vec::new(),
            active_tab: None,
            expanded_dirs: Vec::new(),
            sidebar_visible: true,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TabState {
    pub path: PathBuf,
    pub cursor: (usize, usize),
    pub preview: bool,
}

/// Session file for a workspace, keyed by a stable hash of its path.
pub fn session_file(state_dir: &Path, root: &Path) -> PathBuf {
    let hash = fnv1a(root.as_os_str().as_encoded_bytes());
    state_dir.join("sessions").join(format!("{hash:016x}.json"))
}

/// Loads a session. Missing, corrupt or outdated files yield `None`.
pub fn load(path: &Path) -> Option<Session> {
    let bytes = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(err) if err.kind() == io::ErrorKind::NotFound => return None,
        Err(err) => {
            tracing::warn!(%err, "cannot read session");
            return None;
        }
    };
    match serde_json::from_slice::<Session>(&bytes) {
        Ok(session) if session.version == SESSION_VERSION => Some(session),
        Ok(session) => {
            tracing::warn!(
                version = session.version,
                "ignoring session from another version"
            );
            None
        }
        Err(err) => {
            tracing::warn!(%err, "ignoring corrupt session");
            None
        }
    }
}

pub fn save(path: &Path, session: &Session) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let json = serde_json::to_vec_pretty(session).map_err(io::Error::other)?;
    fsio::write_atomic(path, &json)
}

/// FNV-1a: stable across Rust versions, unlike `DefaultHasher`.
fn fnv1a(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325, |hash, &b| {
        (hash ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Session {
        Session {
            tabs: vec![TabState {
                path: "a.md".into(),
                cursor: (3, 1),
                preview: true,
            }],
            active_tab: Some(0),
            expanded_dirs: vec!["work".into()],
            ..Session::default()
        }
    }

    #[test]
    fn round_trips() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("sessions/x.json");
        save(&path, &sample()).unwrap();
        assert_eq!(load(&path), Some(sample()));
    }

    #[test]
    fn missing_or_corrupt_is_none() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("s.json");
        assert_eq!(load(&path), None);
        fs::write(&path, "{not json").unwrap();
        assert_eq!(load(&path), None);
    }

    #[test]
    fn other_version_is_none() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("s.json");
        fs::write(&path, r#"{"version": 99}"#).unwrap();
        assert_eq!(load(&path), None);
    }

    #[test]
    fn file_name_is_stable_per_root() {
        let a = session_file(Path::new("/state"), Path::new("/home/u/notes"));
        let b = session_file(Path::new("/state"), Path::new("/home/u/notes"));
        let c = session_file(Path::new("/state"), Path::new("/home/u/other"));
        assert_eq!(a, b);
        assert_ne!(a, c);
        assert!(a.starts_with("/state/sessions"));
    }
}
