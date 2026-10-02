//! Full-text search across notes, built on ripgrep's libraries.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use grep_regex::RegexMatcherBuilder;
use grep_searcher::Searcher;
use grep_searcher::sinks::UTF8;

use crate::workspace;

pub const MAX_HITS: usize = 1000;
const BATCH: usize = 64;
const MAX_LINE_CHARS: usize = 200;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchHit {
    pub path: PathBuf,
    /// 1-based line number.
    pub line: usize,
    pub text: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SearchSummary {
    pub hits: usize,
    pub truncated: bool,
    pub cancelled: bool,
}

/// Case-insensitive search. Plain text unless `regex`. Hits arrive in batches.
pub fn search(
    root: &Path,
    query: &str,
    regex: bool,
    cancel: &AtomicBool,
    mut emit: impl FnMut(Vec<SearchHit>),
) -> Result<SearchSummary, String> {
    let matcher = RegexMatcherBuilder::new()
        .case_insensitive(true)
        .fixed_strings(!regex)
        .build(query)
        .map_err(|e| e.to_string())?;
    let mut searcher = Searcher::new();
    let mut summary = SearchSummary::default();
    let mut batch = Vec::new();
    for path in workspace::walk_notes(root) {
        if cancel.load(Ordering::Relaxed) {
            summary.cancelled = true;
            break;
        }
        let result = searcher.search_path(
            &matcher,
            &path,
            UTF8(|line, text| {
                if summary.hits == MAX_HITS {
                    summary.truncated = true;
                    return Ok(false);
                }
                summary.hits += 1;
                batch.push(SearchHit {
                    path: path.clone(),
                    line: line as usize,
                    text: text.trim_end().chars().take(MAX_LINE_CHARS).collect(),
                });
                Ok(!cancel.load(Ordering::Relaxed))
            }),
        );
        if let Err(err) = result {
            tracing::debug!(path = %path.display(), %err, "search skipped file");
        }
        if batch.len() >= BATCH {
            emit(std::mem::take(&mut batch));
        }
        if summary.truncated {
            break;
        }
    }
    if !batch.is_empty() {
        emit(batch);
    }
    Ok(summary)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn run(root: &Path, query: &str, regex: bool) -> (Vec<SearchHit>, SearchSummary) {
        let mut hits = Vec::new();
        let summary = search(root, query, regex, &AtomicBool::new(false), |b| {
            hits.extend(b)
        })
        .unwrap();
        hits.sort_by(|a, b| (&a.path, a.line).cmp(&(&b.path, b.line)));
        (hits, summary)
    }

    #[test]
    fn finds_case_insensitive_plain_text() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("a.md"), "Hello\nworld\nhello.*\n").unwrap();
        fs::write(dir.path().join("b.txt"), "hello").unwrap();
        let (hits, summary) = run(dir.path(), "HELLO", false);
        assert_eq!(hits.iter().map(|h| h.line).collect::<Vec<_>>(), [1, 3]);
        assert_eq!(summary.hits, 2);
        let (hits, _) = run(dir.path(), "hello.*", false);
        assert_eq!(hits.len(), 1, "plain text treats .* literally");
    }

    #[test]
    fn supports_regex_and_reports_bad_patterns() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("a.md"), "todo 1\ndone\ntodo 22\n").unwrap();
        let (hits, _) = run(dir.path(), r"todo \d+", true);
        assert_eq!(hits.len(), 2);
        assert!(search(dir.path(), "(", true, &AtomicBool::new(false), |_| {}).is_err());
    }

    #[test]
    fn caps_results() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("a.md"), "x\n".repeat(MAX_HITS + 10)).unwrap();
        let (hits, summary) = run(dir.path(), "x", false);
        assert_eq!(hits.len(), MAX_HITS);
        assert!(summary.truncated);
    }

    #[test]
    fn cancelled_search_stops() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("a.md"), "x\n").unwrap();
        let summary = search(dir.path(), "x", false, &AtomicBool::new(true), |_| {}).unwrap();
        assert!(summary.cancelled);
        assert_eq!(summary.hits, 0);
    }

    #[test]
    fn skips_non_utf8_files() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("bad.md"), [b'x', 0xff, b'\n']).unwrap();
        fs::write(dir.path().join("good.md"), "x\n").unwrap();
        let (hits, _) = run(dir.path(), "x", false);
        assert_eq!(hits.len(), 1);
    }
}
