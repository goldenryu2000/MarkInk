//! Command-line arguments.

use std::path::{Path, PathBuf};

use clap::Parser;

/// A fast, reliable Markdown note-taking editor.
#[derive(Debug, Parser)]
#[command(name = "markink", version, about)]
pub struct Cli {
    /// Folder or note to open. Defaults to the current directory.
    pub path: Option<PathBuf>,
}

/// What to open at startup.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Launch {
    pub root: PathBuf,
    pub open: Option<PathBuf>,
}

impl Cli {
    /// Resolves the workspace. A note path opens its parent folder plus the note.
    pub fn launch(&self, cwd: &Path) -> Result<Launch, String> {
        let path = cwd.join(self.path.as_deref().unwrap_or(Path::new(".")));
        let path = path
            .canonicalize()
            .map_err(|e| format!("cannot open {}: {e}", path.display()))?;
        if path.is_dir() {
            return Ok(Launch {
                root: path,
                open: None,
            });
        }
        match path.parent() {
            Some(parent) => Ok(Launch {
                root: parent.to_path_buf(),
                open: Some(path),
            }),
            None => Err(format!("{} is not a folder", path.display())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn cli(path: Option<&str>) -> Cli {
        Cli {
            path: path.map(PathBuf::from),
        }
    }

    #[test]
    fn defaults_to_cwd() {
        let dir = tempfile::tempdir().unwrap();
        let launch = cli(None).launch(dir.path()).unwrap();
        assert_eq!(launch.root, dir.path().canonicalize().unwrap());
        assert_eq!(launch.open, None);
    }

    #[test]
    fn resolves_relative_folder() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir(dir.path().join("notes")).unwrap();
        let launch = cli(Some("notes")).launch(dir.path()).unwrap();
        assert_eq!(
            launch.root,
            dir.path().join("notes").canonicalize().unwrap()
        );
    }

    #[test]
    fn note_path_opens_parent_and_note() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("a.md"), "x").unwrap();
        let launch = cli(Some("a.md")).launch(dir.path()).unwrap();
        let root = dir.path().canonicalize().unwrap();
        assert_eq!(launch.open, Some(root.join("a.md")));
        assert_eq!(launch.root, root);
    }

    #[test]
    fn missing_path_is_an_error() {
        let dir = tempfile::tempdir().unwrap();
        assert!(cli(Some("nope")).launch(dir.path()).is_err());
    }
}
