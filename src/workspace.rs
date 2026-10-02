//! The notes folder: listing, tree state and file operations.

use std::collections::{HashMap, HashSet};
use std::fs::{self, OpenOptions};
use std::io;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum EntryKind {
    Dir,
    Note,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub path: PathBuf,
    pub name: String,
    pub kind: EntryKind,
}

pub fn is_note(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("md") || e.eq_ignore_ascii_case("markdown"))
}

/// Walker shared by the tree, quick open and search: honours .gitignore, skips hidden.
fn walker(dir: &Path) -> ignore::WalkBuilder {
    let mut builder = ignore::WalkBuilder::new(dir);
    builder
        .hidden(true)
        .git_ignore(true)
        .git_exclude(true)
        .require_git(false)
        .follow_links(false);
    builder
}

/// Notes and folders in `dir`. Symlinked notes are kept, symlinked folders skipped.
fn classify(path: &Path) -> Option<EntryKind> {
    let meta = fs::symlink_metadata(path).ok()?;
    if meta.file_type().is_symlink() {
        let target = fs::metadata(path).ok()?;
        return (target.is_file() && is_note(path)).then_some(EntryKind::Note);
    }
    if meta.is_dir() {
        Some(EntryKind::Dir)
    } else {
        (meta.is_file() && is_note(path)).then_some(EntryKind::Note)
    }
}

/// Visible children of `dir`: folders first, then notes, case-insensitive.
pub fn list_dir(dir: &Path) -> io::Result<Vec<Entry>> {
    if !fs::metadata(dir)?.is_dir() {
        return Err(io::Error::new(io::ErrorKind::NotADirectory, "not a folder"));
    }
    let mut entries = Vec::new();
    for result in walker(dir).max_depth(Some(1)).build() {
        let entry = match result {
            Ok(entry) => entry,
            Err(err) => {
                tracing::debug!(%err, "skipping entry");
                continue;
            }
        };
        if entry.depth() == 0 {
            continue;
        }
        let path = entry.into_path();
        if let Some(kind) = classify(&path) {
            let name = path
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned();
            entries.push(Entry { path, name, kind });
        }
    }
    entries.sort_by_cached_key(|e| (e.kind, e.name.to_lowercase()));
    Ok(entries)
}

/// Every note under `root`, in walk order.
pub fn walk_notes(root: &Path) -> Vec<PathBuf> {
    walker(root)
        .build()
        .filter_map(Result::ok)
        .map(ignore::DirEntry::into_path)
        .filter(|p| classify(p) == Some(EntryKind::Note))
        .collect()
}

pub struct Row<'a> {
    pub depth: usize,
    pub entry: &'a Entry,
    pub expanded: bool,
}

/// Explorer state: loaded listings, expanded folders, selection.
#[derive(Debug, Default)]
pub struct Tree {
    root: PathBuf,
    listings: HashMap<PathBuf, Vec<Entry>>,
    expanded: HashSet<PathBuf>,
    pub selected: Option<PathBuf>,
}

impl Tree {
    pub fn new(root: PathBuf) -> Self {
        Self {
            root,
            ..Self::default()
        }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn set_listing(&mut self, dir: PathBuf, entries: Vec<Entry>) {
        self.listings.insert(dir, entries);
    }

    pub fn has_listing(&self, dir: &Path) -> bool {
        self.listings.contains_key(dir)
    }

    pub fn is_expanded(&self, dir: &Path) -> bool {
        self.expanded.contains(dir)
    }

    /// Expands or collapses `dir`. True if it is now expanded and needs listing.
    pub fn toggle(&mut self, dir: &Path) -> bool {
        if self.expanded.remove(dir) {
            return false;
        }
        self.expand(dir.to_path_buf())
    }

    /// Expands `dir`. True if it needs listing.
    pub fn expand(&mut self, dir: PathBuf) -> bool {
        let needs_listing = !self.has_listing(&dir);
        self.expanded.insert(dir);
        needs_listing
    }

    pub fn expanded(&self) -> Vec<PathBuf> {
        let mut dirs: Vec<_> = self.expanded.iter().cloned().collect();
        dirs.sort();
        dirs
    }

    /// Visible rows, depth first, starting under the root.
    pub fn rows(&self) -> Vec<Row<'_>> {
        let mut rows = Vec::new();
        self.push_rows(&self.root, 0, &mut rows);
        rows
    }

    fn push_rows<'a>(&'a self, dir: &Path, depth: usize, rows: &mut Vec<Row<'a>>) {
        let Some(entries) = self.listings.get(dir) else {
            return;
        };
        for entry in entries {
            let expanded = entry.kind == EntryKind::Dir && self.expanded.contains(&entry.path);
            rows.push(Row {
                depth,
                entry,
                expanded,
            });
            if expanded {
                self.push_rows(&entry.path, depth + 1, rows);
            }
        }
    }

    /// The loaded listing that must be refreshed after `path` changed.
    pub fn affected_listing(&self, path: &Path) -> Option<PathBuf> {
        let parent = path.parent()?;
        self.has_listing(parent).then(|| parent.to_path_buf())
    }

    /// Drops state for a removed path and everything under it.
    pub fn forget(&mut self, path: &Path) {
        self.listings.retain(|dir, _| !dir.starts_with(path));
        self.expanded.retain(|dir| !dir.starts_with(path));
        if self
            .selected
            .as_deref()
            .is_some_and(|s| s.starts_with(path))
        {
            self.selected = None;
        }
    }

    /// Folder for new items: the selected folder, the selected note's folder, or the root.
    pub fn target_dir(&self) -> PathBuf {
        match &self.selected {
            Some(path) if path.is_dir() => path.clone(),
            Some(path) => path
                .parent()
                .map_or_else(|| self.root.clone(), Path::to_path_buf),
            None => self.root.clone(),
        }
    }
}

fn validate_name(name: &str) -> io::Result<&str> {
    let name = name.trim();
    if name.is_empty() || name == "." || name == ".." || name.contains(['/', '\0']) {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "invalid name"));
    }
    Ok(name)
}

fn with_note_extension(name: &str) -> String {
    if is_note(Path::new(name)) {
        name.to_owned()
    } else {
        format!("{name}.md")
    }
}

/// Creates an empty note; `.md` is added if missing.
pub fn create_note(dir: &Path, name: &str) -> io::Result<PathBuf> {
    let path = dir.join(with_note_extension(validate_name(name)?));
    OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)?;
    Ok(path)
}

pub fn create_dir(dir: &Path, name: &str) -> io::Result<PathBuf> {
    let path = dir.join(validate_name(name)?);
    fs::create_dir(&path)?;
    Ok(path)
}

/// Renames within the same folder. Never overwrites.
pub fn rename(from: &Path, new_name: &str) -> io::Result<PathBuf> {
    let name = validate_name(new_name)?;
    let name = if is_note(from) && from.is_file() {
        with_note_extension(name)
    } else {
        name.to_owned()
    };
    let to = from.with_file_name(name);
    if to == from {
        return Ok(to);
    }
    if fs::symlink_metadata(&to).is_ok() {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "a file with that name exists",
        ));
    }
    fs::rename(from, &to)?;
    Ok(to)
}

pub fn move_to_trash(path: &Path) -> io::Result<()> {
    trash::delete(path).map_err(io::Error::other)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(entries: &[Entry]) -> Vec<&str> {
        entries.iter().map(|e| e.name.as_str()).collect()
    }

    fn setup() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        fs::create_dir_all(root.join("Work/deep")).unwrap();
        fs::create_dir_all(root.join(".git")).unwrap();
        fs::create_dir_all(root.join("build")).unwrap();
        fs::write(root.join("b.md"), "").unwrap();
        fs::write(root.join("A.markdown"), "").unwrap();
        fs::write(root.join("image.png"), "").unwrap();
        fs::write(root.join(".hidden.md"), "").unwrap();
        fs::write(root.join(".gitignore"), "build/\nignored.md\n").unwrap();
        fs::write(root.join("ignored.md"), "").unwrap();
        fs::write(root.join("Work/w.md"), "").unwrap();
        fs::write(root.join("Work/deep/d.md"), "").unwrap();
        dir
    }

    #[test]
    fn lists_folders_then_notes_respecting_ignores() {
        let dir = setup();
        let entries = list_dir(dir.path()).unwrap();
        assert_eq!(names(&entries), ["Work", "A.markdown", "b.md"]);
        assert_eq!(entries[0].kind, EntryKind::Dir);
    }

    #[test]
    fn symlinked_notes_kept_and_folders_skipped() {
        let dir = setup();
        std::os::unix::fs::symlink(dir.path().join("b.md"), dir.path().join("link.md")).unwrap();
        std::os::unix::fs::symlink(dir.path().join("Work"), dir.path().join("loop")).unwrap();
        let entries = list_dir(dir.path()).unwrap();
        assert_eq!(names(&entries), ["Work", "A.markdown", "b.md", "link.md"]);
    }

    #[test]
    fn walks_all_notes() {
        let dir = setup();
        let mut notes: Vec<_> = walk_notes(dir.path())
            .into_iter()
            .map(|p| p.strip_prefix(dir.path()).unwrap().to_path_buf())
            .collect();
        notes.sort();
        let expected: Vec<PathBuf> = ["A.markdown", "Work/deep/d.md", "Work/w.md", "b.md"]
            .iter()
            .map(PathBuf::from)
            .collect();
        assert_eq!(notes, expected);
    }

    #[test]
    fn tree_rows_follow_expansion() {
        let dir = setup();
        let root = dir.path().to_path_buf();
        let work = root.join("Work");
        let mut tree = Tree::new(root.clone());
        tree.set_listing(root.clone(), list_dir(&root).unwrap());
        assert_eq!(tree.rows().len(), 3);
        assert!(tree.toggle(&work));
        tree.set_listing(work.clone(), list_dir(&work).unwrap());
        let rows: Vec<_> = tree
            .rows()
            .iter()
            .map(|r| (r.depth, r.entry.name.clone()))
            .collect();
        assert_eq!(rows[0], (0, "Work".to_string()));
        assert_eq!(rows[1], (1, "deep".to_string()));
        assert_eq!(rows[2], (1, "w.md".to_string()));
        assert!(!tree.toggle(&work));
        assert_eq!(tree.rows().len(), 3);
        assert!(!tree.toggle(&work), "listing is cached");
    }

    #[test]
    fn forget_drops_nested_state() {
        let mut tree = Tree::new("/r".into());
        tree.set_listing("/r/a".into(), vec![]);
        tree.set_listing("/r/a/b".into(), vec![]);
        tree.expand("/r/a".into());
        tree.selected = Some("/r/a/b/n.md".into());
        tree.forget(Path::new("/r/a"));
        assert!(!tree.has_listing(Path::new("/r/a/b")));
        assert!(!tree.is_expanded(Path::new("/r/a")));
        assert_eq!(tree.selected, None);
    }

    #[test]
    fn target_dir_uses_selection() {
        let dir = setup();
        let root = dir.path().to_path_buf();
        let mut tree = Tree::new(root.clone());
        assert_eq!(tree.target_dir(), root);
        tree.selected = Some(root.join("Work"));
        assert_eq!(tree.target_dir(), root.join("Work"));
        tree.selected = Some(root.join("Work/w.md"));
        assert_eq!(tree.target_dir(), root.join("Work"));
    }

    #[test]
    fn create_note_adds_extension_and_never_overwrites() {
        let dir = tempfile::tempdir().unwrap();
        let path = create_note(dir.path(), "ideas").unwrap();
        assert_eq!(path, dir.path().join("ideas.md"));
        assert!(create_note(dir.path(), "ideas.md").is_err());
        assert!(create_note(dir.path(), "a/b").is_err());
        assert!(create_note(dir.path(), "  ").is_err());
    }

    #[test]
    fn rename_keeps_extension_and_refuses_overwrite() {
        let dir = tempfile::tempdir().unwrap();
        let a = create_note(dir.path(), "a").unwrap();
        create_note(dir.path(), "b").unwrap();
        assert!(rename(&a, "b").is_err());
        let renamed = rename(&a, "c").unwrap();
        assert_eq!(renamed, dir.path().join("c.md"));
        assert!(renamed.exists() && !a.exists());
        let folder = create_dir(dir.path(), "f").unwrap();
        assert_eq!(rename(&folder, "g").unwrap(), dir.path().join("g"));
    }
}
