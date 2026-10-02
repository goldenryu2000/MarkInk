//! Fuzzy matching over note paths.

use std::path::PathBuf;

use nucleo_matcher::pattern::{CaseMatching, Normalization, Pattern};
use nucleo_matcher::{Config, Matcher};

pub struct QuickOpen {
    root: PathBuf,
    /// Root-relative paths, sorted.
    paths: Vec<String>,
    matcher: Matcher,
}

impl QuickOpen {
    pub fn new(root: PathBuf) -> Self {
        Self {
            root,
            paths: Vec::new(),
            matcher: Matcher::new(Config::DEFAULT.match_paths()),
        }
    }

    pub fn set_notes(&mut self, notes: Vec<PathBuf>) {
        let mut paths: Vec<String> = notes
            .iter()
            .filter_map(|p| p.strip_prefix(&self.root).ok())
            .map(|p| p.to_string_lossy().replace(std::path::MAIN_SEPARATOR, "/"))
            .collect();
        paths.sort_unstable();
        self.paths = paths;
    }

    pub fn len(&self) -> usize {
        self.paths.len()
    }

    pub fn is_empty(&self) -> bool {
        self.paths.is_empty()
    }

    /// Best matches first. An empty query lists notes alphabetically.
    pub fn matches(&mut self, query: &str, limit: usize) -> Vec<String> {
        if query.trim().is_empty() {
            return self.paths.iter().take(limit).cloned().collect();
        }
        let pattern = Pattern::parse(query, CaseMatching::Smart, Normalization::Smart);
        let mut hits = pattern.match_list(self.paths.iter(), &mut self.matcher);
        hits.truncate(limit);
        hits.into_iter().map(|(path, _)| path.clone()).collect()
    }

    pub fn absolute(&self, relative: &str) -> PathBuf {
        self.root.join(relative)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn index() -> QuickOpen {
        let mut q = QuickOpen::new("/n".into());
        q.set_notes(
            [
                "/n/work/todo.md",
                "/n/ideas.md",
                "/n/journal/2026-10-02.md",
                "/n/work/meeting notes.md",
            ]
            .iter()
            .map(PathBuf::from)
            .collect(),
        );
        q
    }

    #[test]
    fn empty_query_lists_sorted() {
        assert_eq!(
            index().matches("", 2),
            ["ideas.md", "journal/2026-10-02.md"]
        );
    }

    #[test]
    fn fuzzy_matches_best_first() {
        let mut q = index();
        assert_eq!(q.matches("todo", 10)[0], "work/todo.md");
        assert_eq!(q.matches("wmn", 10)[0], "work/meeting notes.md");
        assert!(q.matches("zzz", 10).is_empty());
    }

    #[test]
    fn absolute_joins_root() {
        assert_eq!(index().absolute("ideas.md"), Path::new("/n/ideas.md"));
    }
}
