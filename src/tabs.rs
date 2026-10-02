//! The ordered set of open documents.

use std::path::{Path, PathBuf};

use crate::document::{DocId, Document};
use crate::fsio::LoadedNote;

#[derive(Default)]
pub struct Tabs {
    docs: Vec<Document>,
    active: Option<DocId>,
    next_id: u64,
}

impl Tabs {
    /// Opens a note after the active tab, or activates it if already open.
    pub fn open(&mut self, path: PathBuf, note: LoadedNote) -> DocId {
        if let Some(id) = self.find(&path) {
            self.active = Some(id);
            return id;
        }
        let id = DocId(self.next_id);
        self.next_id += 1;
        let at = self
            .active
            .and_then(|a| self.index(a))
            .map_or(self.docs.len(), |i| i + 1);
        self.docs.insert(at, Document::new(id, path, note));
        self.active = Some(id);
        id
    }

    pub fn find(&self, path: &Path) -> Option<DocId> {
        self.docs
            .iter()
            .find(|d| d.path() == path)
            .map(Document::id)
    }

    pub fn get(&self, id: DocId) -> Option<&Document> {
        self.docs.iter().find(|d| d.id() == id)
    }

    pub fn get_mut(&mut self, id: DocId) -> Option<&mut Document> {
        self.docs.iter_mut().find(|d| d.id() == id)
    }

    pub fn active_id(&self) -> Option<DocId> {
        self.active
    }

    pub fn active(&self) -> Option<&Document> {
        self.get(self.active?)
    }

    pub fn active_mut(&mut self) -> Option<&mut Document> {
        self.get_mut(self.active?)
    }

    pub fn activate(&mut self, id: DocId) -> bool {
        let exists = self.get(id).is_some();
        if exists {
            self.active = Some(id);
        }
        exists
    }

    /// Removes a tab, activating its right neighbour (or left at the end).
    pub fn close(&mut self, id: DocId) -> Option<Document> {
        let index = self.index(id)?;
        let doc = self.docs.remove(index);
        if self.active == Some(id) {
            let neighbour = self
                .docs
                .get(index)
                .or_else(|| index.checked_sub(1).and_then(|i| self.docs.get(i)));
            self.active = neighbour.map(Document::id);
        }
        Some(doc)
    }

    /// Activates the next or previous tab, wrapping around.
    pub fn cycle(&mut self, forward: bool) {
        let len = self.docs.len();
        let Some(index) = self.active.and_then(|a| self.index(a)) else {
            return;
        };
        let next = if forward {
            (index + 1) % len
        } else {
            (index + len - 1) % len
        };
        self.active = Some(self.docs[next].id());
    }

    pub fn iter(&self) -> impl Iterator<Item = &Document> {
        self.docs.iter()
    }

    pub fn iter_mut(&mut self) -> impl Iterator<Item = &mut Document> {
        self.docs.iter_mut()
    }

    pub fn len(&self) -> usize {
        self.docs.len()
    }

    pub fn is_empty(&self) -> bool {
        self.docs.is_empty()
    }

    /// Updates paths after a file or folder rename.
    pub fn rename_path(&mut self, from: &Path, to: &Path) {
        for doc in &mut self.docs {
            if let Ok(rest) = doc.path().strip_prefix(from) {
                let path = if rest.as_os_str().is_empty() {
                    to.to_path_buf()
                } else {
                    to.join(rest)
                };
                doc.set_path(path);
            }
        }
    }

    /// Ids of tabs at or under `path`.
    pub fn under(&self, path: &Path) -> Vec<DocId> {
        self.docs
            .iter()
            .filter(|d| d.path().starts_with(path))
            .map(Document::id)
            .collect()
    }

    fn index(&self, id: DocId) -> Option<usize> {
        self.docs.iter().position(|d| d.id() == id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn open(tabs: &mut Tabs, path: &str) -> DocId {
        tabs.open(PathBuf::from(path), LoadedNote::from_text(""))
    }

    fn titles(tabs: &Tabs) -> Vec<String> {
        tabs.iter().map(Document::title).collect()
    }

    #[test]
    fn opening_inserts_after_active_and_activates() {
        let mut tabs = Tabs::default();
        let a = open(&mut tabs, "/n/a.md");
        open(&mut tabs, "/n/b.md");
        tabs.activate(a);
        let c = open(&mut tabs, "/n/c.md");
        assert_eq!(titles(&tabs), ["a.md", "c.md", "b.md"]);
        assert_eq!(tabs.active_id(), Some(c));
    }

    #[test]
    fn opening_same_path_reuses_tab() {
        let mut tabs = Tabs::default();
        let a = open(&mut tabs, "/n/a.md");
        open(&mut tabs, "/n/b.md");
        assert_eq!(open(&mut tabs, "/n/a.md"), a);
        assert_eq!(tabs.len(), 2);
        assert_eq!(tabs.active_id(), Some(a));
    }

    #[test]
    fn closing_activates_neighbour() {
        let mut tabs = Tabs::default();
        let a = open(&mut tabs, "/n/a.md");
        let b = open(&mut tabs, "/n/b.md");
        let c = open(&mut tabs, "/n/c.md");
        tabs.activate(b);
        tabs.close(b);
        assert_eq!(tabs.active_id(), Some(c));
        tabs.close(c);
        assert_eq!(tabs.active_id(), Some(a));
        tabs.close(a);
        assert_eq!(tabs.active_id(), None);
    }

    #[test]
    fn closing_inactive_keeps_active() {
        let mut tabs = Tabs::default();
        let a = open(&mut tabs, "/n/a.md");
        let b = open(&mut tabs, "/n/b.md");
        tabs.close(a);
        assert_eq!(tabs.active_id(), Some(b));
    }

    #[test]
    fn cycle_wraps() {
        let mut tabs = Tabs::default();
        let a = open(&mut tabs, "/n/a.md");
        let b = open(&mut tabs, "/n/b.md");
        tabs.cycle(true);
        assert_eq!(tabs.active_id(), Some(a));
        tabs.cycle(false);
        assert_eq!(tabs.active_id(), Some(b));
    }

    #[test]
    fn rename_moves_files_and_folders() {
        let mut tabs = Tabs::default();
        let a = open(&mut tabs, "/n/a.md");
        let b = open(&mut tabs, "/n/dir/b.md");
        tabs.rename_path(Path::new("/n/a.md"), Path::new("/n/z.md"));
        tabs.rename_path(Path::new("/n/dir"), Path::new("/n/other"));
        assert_eq!(tabs.get(a).unwrap().path(), Path::new("/n/z.md"));
        assert_eq!(tabs.get(b).unwrap().path(), Path::new("/n/other/b.md"));
    }

    #[test]
    fn under_matches_whole_components() {
        let mut tabs = Tabs::default();
        let a = open(&mut tabs, "/n/dir/a.md");
        open(&mut tabs, "/n/dir2/b.md");
        assert_eq!(tabs.under(Path::new("/n/dir")), vec![a]);
    }
}
