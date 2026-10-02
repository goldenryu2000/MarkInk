//! Undo history built from text diffs, independent of the editor widget.

use std::time::{Duration, Instant};

const GROUP_WINDOW: Duration = Duration::from_secs(1);
const MAX_GROUPS: usize = 1000;

/// Replace `removed` with `inserted` at byte `offset`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Change {
    pub offset: usize,
    pub removed: String,
    pub inserted: String,
}

impl Change {
    /// The single contiguous change turning `old` into `new`.
    pub fn between(old: &str, new: &str) -> Option<Change> {
        if old == new {
            return None;
        }
        let prefix = common_prefix(old, new);
        let suffix = common_suffix(&old[prefix..], &new[prefix..]);
        Some(Change {
            offset: prefix,
            removed: old[prefix..old.len() - suffix].to_owned(),
            inserted: new[prefix..new.len() - suffix].to_owned(),
        })
    }

    pub fn apply(&self, text: &mut String) {
        text.replace_range(
            self.offset..self.offset + self.removed.len(),
            &self.inserted,
        );
    }

    pub fn inverse(&self) -> Change {
        Change {
            offset: self.offset,
            removed: self.inserted.clone(),
            inserted: self.removed.clone(),
        }
    }
}

/// How an edit groups with its neighbours.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditKind {
    Insert,
    Delete,
    /// Paste, newline, indent: always its own step.
    Other,
}

#[derive(Debug)]
struct Group {
    kind: EditKind,
    changes: Vec<Change>,
}

#[derive(Debug, Default)]
pub struct UndoStack {
    undo: Vec<Group>,
    redo: Vec<Group>,
    last_edit: Option<Instant>,
}

impl UndoStack {
    pub fn record(&mut self, change: Change, kind: EditKind, now: Instant) {
        self.redo.clear();
        let recent = self
            .last_edit
            .is_some_and(|t| now.duration_since(t) < GROUP_WINDOW);
        self.last_edit = Some(now);
        if recent
            && let Some(group) = self.undo.last_mut()
            && group.kind == kind
            && group
                .changes
                .last()
                .is_some_and(|prev| continues(prev, &change, kind))
        {
            group.changes.push(change);
            return;
        }
        self.undo.push(Group {
            kind,
            changes: vec![change],
        });
        if self.undo.len() > MAX_GROUPS {
            self.undo.remove(0);
        }
    }

    /// Makes the next edit start a new undo step.
    pub fn break_group(&mut self) {
        self.last_edit = None;
    }

    /// Changes that revert the latest step, in application order.
    pub fn undo(&mut self) -> Option<Vec<Change>> {
        let group = self.undo.pop()?;
        let changes = group.changes.iter().rev().map(Change::inverse).collect();
        self.redo.push(group);
        self.last_edit = None;
        Some(changes)
    }

    /// Changes that reapply the latest undone step, in application order.
    pub fn redo(&mut self) -> Option<Vec<Change>> {
        let group = self.redo.pop()?;
        let changes = group.changes.clone();
        self.undo.push(group);
        self.last_edit = None;
        Some(changes)
    }
}

/// Whether `next` extends the run of edits ending with `prev`.
fn continues(prev: &Change, next: &Change, kind: EditKind) -> bool {
    match kind {
        EditKind::Insert => {
            next.removed.is_empty()
                && !next.inserted.contains('\n')
                && next.offset == prev.offset + prev.inserted.len()
        }
        EditKind::Delete => {
            next.inserted.is_empty()
                && (next.offset + next.removed.len() == prev.offset || next.offset == prev.offset)
        }
        EditKind::Other => false,
    }
}

fn common_prefix(a: &str, b: &str) -> usize {
    let mut n = a.bytes().zip(b.bytes()).take_while(|(x, y)| x == y).count();
    while !a.is_char_boundary(n) {
        n -= 1;
    }
    n
}

fn common_suffix(a: &str, b: &str) -> usize {
    let mut n = a
        .bytes()
        .rev()
        .zip(b.bytes().rev())
        .take_while(|(x, y)| x == y)
        .count();
    while !a.is_char_boundary(a.len() - n) {
        n -= 1;
    }
    n
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    fn apply_all(text: &mut String, changes: Vec<Change>) {
        for change in changes {
            change.apply(text);
        }
    }

    /// Applies `new` to `text`, recording the diff.
    fn edit(stack: &mut UndoStack, text: &mut String, new: &str, kind: EditKind, at: Instant) {
        let change = Change::between(text, new).unwrap();
        stack.record(change, kind, at);
        *text = new.to_owned();
    }

    #[test]
    fn between_finds_insert_delete_and_replace() {
        assert_eq!(
            Change::between("ac", "abc"),
            Some(Change {
                offset: 1,
                removed: "".into(),
                inserted: "b".into()
            })
        );
        assert_eq!(
            Change::between("abc", "ac"),
            Some(Change {
                offset: 1,
                removed: "b".into(),
                inserted: "".into()
            })
        );
        assert_eq!(
            Change::between("héllo", "hállo"),
            Some(Change {
                offset: 1,
                removed: "é".into(),
                inserted: "á".into()
            })
        );
        assert_eq!(Change::between("same", "same"), None);
    }

    #[test]
    fn between_handles_repeated_chars() {
        let change = Change::between("aaa", "aaaa").unwrap();
        let mut text = "aaa".to_owned();
        change.apply(&mut text);
        assert_eq!(text, "aaaa");
    }

    #[test]
    fn typing_is_one_step() {
        let now = Instant::now();
        let (mut stack, mut text) = (UndoStack::default(), String::new());
        for typed in ["a", "ab", "abc"] {
            edit(&mut stack, &mut text, typed, EditKind::Insert, now);
        }
        apply_all(&mut text, stack.undo().unwrap());
        assert_eq!(text, "");
        assert!(stack.undo().is_none());
    }

    #[test]
    fn pause_starts_new_step() {
        let now = Instant::now();
        let (mut stack, mut text) = (UndoStack::default(), String::new());
        edit(&mut stack, &mut text, "a", EditKind::Insert, now);
        edit(
            &mut stack,
            &mut text,
            "ab",
            EditKind::Insert,
            now + Duration::from_secs(2),
        );
        apply_all(&mut text, stack.undo().unwrap());
        assert_eq!(text, "a");
    }

    #[test]
    fn cursor_jump_starts_new_step() {
        let now = Instant::now();
        let (mut stack, mut text) = (UndoStack::default(), String::from("xy"));
        edit(&mut stack, &mut text, "axy", EditKind::Insert, now);
        edit(&mut stack, &mut text, "axyb", EditKind::Insert, now);
        apply_all(&mut text, stack.undo().unwrap());
        assert_eq!(text, "axy");
    }

    #[test]
    fn backspacing_is_one_step() {
        let now = Instant::now();
        let (mut stack, mut text) = (UndoStack::default(), String::from("abc"));
        edit(&mut stack, &mut text, "ab", EditKind::Delete, now);
        edit(&mut stack, &mut text, "a", EditKind::Delete, now);
        apply_all(&mut text, stack.undo().unwrap());
        assert_eq!(text, "abc");
    }

    #[test]
    fn other_edits_never_group() {
        let now = Instant::now();
        let (mut stack, mut text) = (UndoStack::default(), String::new());
        edit(&mut stack, &mut text, "a", EditKind::Other, now);
        edit(&mut stack, &mut text, "ab", EditKind::Other, now);
        apply_all(&mut text, stack.undo().unwrap());
        assert_eq!(text, "a");
    }

    #[test]
    fn redo_reapplies_and_new_edit_clears_it() {
        let now = Instant::now();
        let (mut stack, mut text) = (UndoStack::default(), String::new());
        edit(&mut stack, &mut text, "a", EditKind::Other, now);
        apply_all(&mut text, stack.undo().unwrap());
        apply_all(&mut text, stack.redo().unwrap());
        assert_eq!(text, "a");
        apply_all(&mut text, stack.undo().unwrap());
        edit(&mut stack, &mut text, "b", EditKind::Other, now);
        assert!(stack.redo().is_none());
    }

    #[test]
    fn history_is_capped() {
        let now = Instant::now();
        let (mut stack, mut text) = (UndoStack::default(), String::new());
        for i in 0..MAX_GROUPS + 5 {
            let next = format!("{text}{}", i % 10);
            edit(&mut stack, &mut text, &next, EditKind::Other, now);
        }
        let mut steps = 0;
        while let Some(changes) = stack.undo() {
            apply_all(&mut text, changes);
            steps += 1;
        }
        assert_eq!(steps, MAX_GROUPS);
    }

    fn arb_edits() -> impl Strategy<Value = Vec<(usize, usize, String)>> {
        prop::collection::vec((0usize..100, 0usize..4, "[ab\né]{0,3}"), 1..40)
    }

    /// Applies a random edit at a char boundary, returning the new text.
    fn random_edit(text: &str, (pos, del, ins): &(usize, usize, String)) -> String {
        let bounds: Vec<usize> = text
            .char_indices()
            .map(|(i, _)| i)
            .chain([text.len()])
            .collect();
        let start = bounds[pos % bounds.len()];
        let end_index =
            (bounds.iter().position(|&b| b == start).unwrap() + del).min(bounds.len() - 1);
        let mut out = text.to_owned();
        out.replace_range(start..bounds[end_index], ins);
        out
    }

    proptest! {
        #[test]
        fn undo_all_then_redo_all_round_trips(start in "[ab\né]{0,10}", edits in arb_edits()) {
            let now = Instant::now();
            let mut stack = UndoStack::default();
            let mut text = start.clone();
            for e in &edits {
                let next = random_edit(&text, e);
                if let Some(change) = Change::between(&text, &next) {
                    let kind = if change.removed.is_empty() { EditKind::Insert } else { EditKind::Delete };
                    stack.record(change, kind, now);
                    text = next;
                }
            }
            let end = text.clone();
            while let Some(changes) = stack.undo() { apply_all(&mut text, changes); }
            prop_assert_eq!(&text, &start);
            while let Some(changes) = stack.redo() { apply_all(&mut text, changes); }
            prop_assert_eq!(text, end);
        }
    }
}
