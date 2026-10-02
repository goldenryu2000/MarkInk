//! One open note: editor buffer, undo history and save state.

use std::path::{Path, PathBuf};
use std::time::Instant;

use iced::widget::markdown;
use std::sync::Arc;

use iced::widget::text_editor::{Action, Content, Cursor, Edit, Motion, Position};

use crate::fsio::{self, DiskSnapshot, Expect, LoadedNote, SaveError, TextFormat};
use crate::lists::{self, OnEnter};
use crate::scroll_sync::ViewEstimate;
use crate::undo::{Change, EditKind, UndoStack};

/// Notes larger than this open with the preview disabled.
pub const LARGE_NOTE_BYTES: usize = 5 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct DocId(pub u64);

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DocStatus {
    Normal,
    /// Disk changed while we had unsaved edits.
    Conflict,
    DeletedOnDisk,
    SaveFailed {
        error: String,
        attempt: u32,
    },
}

/// A copy of a document's text to write in the background.
#[derive(Debug, Clone)]
pub struct SaveRequest {
    pub path: PathBuf,
    pub text: String,
    pub format: TextFormat,
    pub expect: Expect,
    pub revision: u64,
}

impl SaveRequest {
    pub fn run(self) -> (u64, Result<DiskSnapshot, SaveError>) {
        let result = fsio::save_note(&self.path, &self.text, self.format, self.expect);
        (self.revision, result)
    }
}

pub struct Document {
    id: DocId,
    path: PathBuf,
    content: Content,
    /// Mirror of `content`, so diffs and saves never rebuild the text.
    text: String,
    undo: UndoStack,
    revision: u64,
    saved_revision: u64,
    disk: DiskSnapshot,
    format: TextFormat,
    read_only: bool,
    preview: markdown::Content,
    preview_revision: Option<u64>,
    pub status: DocStatus,
    pub saving: bool,
    /// A disk change arrived mid-save; re-read once the save lands.
    pub recheck_after_save: bool,
    /// Close the tab once pending edits are saved.
    pub close_requested: bool,
    pub preview_visible: bool,
    /// Estimated scroll position, used to sync the preview.
    pub view: ViewEstimate,
}

impl Document {
    pub fn new(id: DocId, path: PathBuf, note: LoadedNote) -> Self {
        Self {
            id,
            path,
            content: Content::with_text(&note.text),
            text: note.text,
            undo: UndoStack::default(),
            revision: 0,
            saved_revision: 0,
            disk: note.snapshot,
            format: note.format,
            read_only: note.read_only,
            preview: markdown::Content::new(),
            preview_revision: None,
            status: DocStatus::Normal,
            saving: false,
            recheck_after_save: false,
            close_requested: false,
            preview_visible: false,
            view: ViewEstimate::default(),
        }
    }

    pub fn id(&self) -> DocId {
        self.id
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn set_path(&mut self, path: PathBuf) {
        self.path = path;
    }

    pub fn title(&self) -> String {
        self.path.file_name().map_or_else(
            || self.path.display().to_string(),
            |n| n.to_string_lossy().into_owned(),
        )
    }

    pub fn text(&self) -> &str {
        &self.text
    }

    pub fn content(&self) -> &Content {
        &self.content
    }

    pub fn line_count(&self) -> usize {
        self.content.line_count()
    }

    pub fn revision(&self) -> u64 {
        self.revision
    }

    pub fn is_dirty(&self) -> bool {
        self.revision != self.saved_revision
    }

    pub fn disk(&self) -> DiskSnapshot {
        self.disk
    }

    pub fn is_read_only(&self) -> bool {
        self.read_only
    }

    /// Cursor as (line, byte column).
    pub fn cursor(&self) -> (usize, usize) {
        let position = self.content.cursor().position;
        (position.line, position.column)
    }

    /// Moves the cursor, clamped to the text.
    pub fn set_cursor(&mut self, line: usize, column: usize) {
        let line = line.min(self.content.line_count().saturating_sub(1));
        let text = self
            .content
            .line(line)
            .map(|l| l.text.into_owned())
            .unwrap_or_default();
        let mut column = column.min(text.len());
        while !text.is_char_boundary(column) {
            column -= 1;
        }
        self.content.move_to(Cursor {
            position: Position { line, column },
            selection: None,
        });
    }

    /// Applies an editor action. Returns true if the text changed.
    pub fn apply(&mut self, action: Action, now: Instant) -> bool {
        let kind = match &action {
            Action::Edit(_) if self.read_only => return false,
            Action::Edit(edit) => Some(edit_kind(edit)),
            _ => None,
        };
        self.content.perform(action);
        let Some(kind) = kind else {
            self.undo.break_group();
            return false;
        };
        let new_text = self.content.text();
        match Change::between(&self.text, &new_text) {
            Some(change) => {
                self.undo.record(change, kind, now);
                self.text = new_text;
                self.revision += 1;
                true
            }
            None => false,
        }
    }

    pub fn undo(&mut self) -> bool {
        match self.undo.undo() {
            Some(changes) if !self.read_only => self.apply_history(changes),
            _ => false,
        }
    }

    pub fn redo(&mut self) -> bool {
        match self.undo.redo() {
            Some(changes) if !self.read_only => self.apply_history(changes),
            _ => false,
        }
    }

    /// Replays history changes on both the mirror and the editor.
    fn apply_history(&mut self, changes: Vec<Change>) -> bool {
        for change in &changes {
            let start = position_at(&self.text, change.offset);
            let end = position_at(&self.text, change.offset + change.removed.len());
            let selection = (!change.removed.is_empty()).then_some(start);
            let position = if change.removed.is_empty() {
                start
            } else {
                end
            };
            self.content.move_to(Cursor {
                position,
                selection,
            });
            let edit = if change.inserted.is_empty() {
                Edit::Backspace
            } else {
                Edit::Paste(Arc::new(change.inserted.clone()))
            };
            self.content.perform(Action::Edit(edit));
            change.apply(&mut self.text);
        }
        if self.content.text() != self.text {
            tracing::warn!("editor diverged from history, rebuilding buffer");
            let offset = changes.last().map_or(0, |c| c.offset + c.inserted.len());
            self.content = Content::with_text(&self.text);
            let position = position_at(&self.text, offset);
            self.content.move_to(Cursor {
                position,
                selection: None,
            });
        }
        self.revision += 1;
        true
    }

    /// Ctrl+Backspace / Ctrl+Delete: deletes the selection, or the word before or after.
    pub fn delete_word(&mut self, forward: bool, now: Instant) -> bool {
        if self.content.selection().is_none() {
            let motion = if forward {
                Motion::WordRight
            } else {
                Motion::WordLeft
            };
            self.apply(Action::Select(motion), now);
        }
        let edit = if forward {
            Edit::Delete
        } else {
            Edit::Backspace
        };
        self.apply(Action::Edit(edit), now)
    }

    /// Ctrl+L: adds or toggles a checkbox on the cursor's line.
    pub fn toggle_task(&mut self, now: Instant) -> bool {
        if self.read_only {
            return false;
        }
        let Position { line, column } = self.content.cursor().position;
        let old = self
            .content
            .line(line)
            .map(|l| l.text.into_owned())
            .unwrap_or_default();
        let new = lists::toggle_task(&old);
        self.content.move_to(Cursor {
            position: Position {
                line,
                column: old.len(),
            },
            selection: Some(Position { line, column: 0 }),
        });
        let changed = self.apply(Action::Edit(Edit::Paste(Arc::new(new.clone()))), now);
        let column = (column + new.len()).saturating_sub(old.len());
        self.set_cursor(line, column);
        changed
    }

    /// Enter with Markdown list and quote continuation.
    pub fn enter(&mut self, now: Instant) -> bool {
        if self.read_only {
            return false;
        }
        let Position { line, column } = self.content.cursor().position;
        let text = self
            .content
            .line(line)
            .map(|l| l.text.into_owned())
            .unwrap_or_default();
        let (before, after) = text.split_at(column.min(text.len()));
        let action = match (self.content.selection(), lists::on_enter(before, after)) {
            (None, OnEnter::Continue(insert)) => Edit::Paste(Arc::new(insert)),
            (None, OnEnter::EndList) => {
                let start = Position { line, column: 0 };
                let end = Position { line, column };
                self.content.move_to(Cursor {
                    position: end,
                    selection: Some(start),
                });
                Edit::Backspace
            }
            _ => Edit::Enter,
        };
        self.apply(Action::Edit(action), now)
    }

    /// Marks a save as in flight and returns what to write.
    pub fn begin_save(&mut self, force: bool) -> SaveRequest {
        self.saving = true;
        SaveRequest {
            path: self.path.clone(),
            text: self.text.clone(),
            format: self.format,
            expect: if force {
                Expect::Anything
            } else {
                Expect::Snapshot(self.disk)
            },
            revision: self.revision,
        }
    }

    /// Records a successful save of `revision`.
    pub fn mark_saved(&mut self, revision: u64, snapshot: DiskSnapshot) {
        self.saving = false;
        self.disk = snapshot;
        self.saved_revision = self.saved_revision.max(revision);
        self.status = DocStatus::Normal;
    }

    /// Whether autosave may write this document now.
    pub fn can_autosave(&self) -> bool {
        self.is_dirty()
            && !self.saving
            && !self.read_only
            && matches!(
                self.status,
                DocStatus::Normal | DocStatus::SaveFailed { .. }
            )
    }

    /// Replaces the text with the disk version, keeping the cursor line.
    pub fn reload(&mut self, note: LoadedNote) {
        let line = self.content.cursor().position.line;
        self.content = Content::with_text(&note.text);
        self.text = note.text;
        self.format = note.format;
        self.disk = note.snapshot;
        self.read_only = note.read_only;
        self.undo = UndoStack::default();
        self.revision += 1;
        self.saved_revision = self.revision;
        self.status = DocStatus::Normal;
        self.set_cursor(line, 0);
    }

    pub fn preview_allowed(&self) -> bool {
        self.text.len() <= LARGE_NOTE_BYTES
    }

    pub fn preview_is_stale(&self) -> bool {
        self.preview_revision != Some(self.revision)
    }

    pub fn refresh_preview(&mut self) {
        self.preview = markdown::Content::parse(&self.text);
        self.preview_revision = Some(self.revision);
    }

    pub fn preview_items(&self) -> &[markdown::Item] {
        self.preview.items()
    }
}

fn edit_kind(edit: &Edit) -> EditKind {
    match edit {
        Edit::Insert(_) => EditKind::Insert,
        Edit::Backspace | Edit::Delete => EditKind::Delete,
        Edit::Paste(_) | Edit::Enter | Edit::Indent | Edit::Unindent => EditKind::Other,
    }
}

/// Line and byte column of byte `offset`.
fn position_at(text: &str, offset: usize) -> Position {
    let before = &text[..offset];
    let line = before.matches('\n').count();
    let column = offset - before.rfind('\n').map_or(0, |i| i + 1);
    Position { line, column }
}

#[cfg(test)]
mod tests {
    use super::*;
    use iced::widget::text_editor::Motion;
    use std::fs;
    use std::sync::Arc;

    fn doc(text: &str) -> Document {
        Document::new(
            DocId(1),
            PathBuf::from("/notes/a.md"),
            LoadedNote::from_text(text),
        )
    }

    fn type_str(doc: &mut Document, s: &str) {
        for c in s.chars() {
            let edit = if c == '\n' {
                Edit::Enter
            } else {
                Edit::Insert(c)
            };
            doc.apply(Action::Edit(edit), Instant::now());
        }
    }

    fn assert_in_sync(doc: &Document) {
        assert_eq!(doc.content().text(), doc.text());
    }

    #[test]
    fn editor_text_round_trips() {
        for text in ["", "a", "a\n", "a\n\n", "# T\n\n- x\n"] {
            assert_eq!(doc(text).content().text(), text);
        }
    }

    #[test]
    fn typing_marks_dirty_and_stays_in_sync() {
        let mut d = doc("");
        assert!(!d.is_dirty());
        type_str(&mut d, "hi");
        assert_eq!(d.text(), "hi");
        assert_in_sync(&d);
        assert!(d.is_dirty());
    }

    #[test]
    fn moving_does_not_dirty() {
        let mut d = doc("abc");
        d.apply(Action::Move(Motion::Right), Instant::now());
        assert!(!d.is_dirty());
    }

    #[test]
    fn undo_and_redo_restore_text_and_editor() {
        let mut d = doc("");
        type_str(&mut d, "abc\nd");
        assert!(d.undo());
        assert_eq!(d.text(), "abc\n");
        assert_in_sync(&d);
        assert!(d.undo());
        assert!(d.undo());
        assert_eq!(d.text(), "");
        assert_in_sync(&d);
        assert!(!d.undo());
        assert!(d.redo());
        assert_eq!(d.text(), "abc");
        assert_in_sync(&d);
    }

    #[test]
    fn undo_restores_deleted_selection() {
        let mut d = doc("one\ntwo\n");
        d.apply(Action::SelectAll, Instant::now());
        d.apply(Action::Edit(Edit::Backspace), Instant::now());
        assert_eq!(d.text(), "");
        assert!(d.undo());
        assert_eq!(d.text(), "one\ntwo\n");
        assert_in_sync(&d);
    }

    #[test]
    fn paste_is_undoable() {
        let mut d = doc("");
        d.apply(
            Action::Edit(Edit::Paste(Arc::new("x\ny".into()))),
            Instant::now(),
        );
        assert_eq!(d.text(), "x\ny");
        assert!(d.undo());
        assert_eq!(d.text(), "");
        assert_in_sync(&d);
    }

    #[test]
    fn read_only_ignores_edits() {
        let mut note = LoadedNote::from_text("a");
        note.read_only = true;
        let mut d = Document::new(DocId(1), PathBuf::from("/n/a.md"), note);
        type_str(&mut d, "b");
        assert_eq!(d.text(), "a");
        assert!(!d.is_dirty());
    }

    #[test]
    fn edit_during_save_stays_dirty() {
        let mut d = doc("a");
        type_str(&mut d, "b");
        let request = d.begin_save(false);
        assert!(d.saving);
        assert!(!d.can_autosave());
        type_str(&mut d, "c");
        d.mark_saved(request.revision, DiskSnapshot::of(b"ba"));
        assert!(d.is_dirty());
        assert!(d.can_autosave());
    }

    #[test]
    fn save_request_writes_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.md");
        fs::write(&path, "a").unwrap();
        let mut d = Document::new(DocId(1), path.clone(), fsio::read_note(&path).unwrap());
        type_str(&mut d, "b");
        let (revision, result) = d.begin_save(false).run();
        d.mark_saved(revision, result.unwrap());
        assert!(!d.is_dirty());
        assert_eq!(fs::read_to_string(&path).unwrap(), "ba");
    }

    #[test]
    fn conflict_blocks_autosave() {
        let mut d = doc("a");
        type_str(&mut d, "b");
        d.status = DocStatus::Conflict;
        assert!(!d.can_autosave());
    }

    #[test]
    fn reload_replaces_text_and_clears_history() {
        let mut d = doc("a\nb\nc");
        d.set_cursor(2, 0);
        d.reload(LoadedNote::from_text("x\ny"));
        assert_eq!(d.text(), "x\ny");
        assert_in_sync(&d);
        assert!(!d.is_dirty());
        assert_eq!(d.cursor().0, 1);
        assert!(!d.undo());
    }

    #[test]
    fn set_cursor_clamps() {
        let mut d = doc("ab\nc");
        d.set_cursor(9, 9);
        assert_eq!(d.cursor(), (1, 1));
    }

    #[test]
    fn ctrl_backspace_deletes_previous_word() {
        let mut d = doc("hello world");
        d.set_cursor(0, 11);
        assert!(d.delete_word(false, Instant::now()));
        assert_eq!(d.text(), "hello ");
        assert_in_sync(&d);
        assert!(d.undo());
        assert_eq!(d.text(), "hello world");
    }

    #[test]
    fn ctrl_delete_deletes_next_word() {
        let mut d = doc("hello world");
        d.set_cursor(0, 0);
        assert!(d.delete_word(true, Instant::now()));
        assert!(
            d.text().ends_with("world") && !d.text().contains("hello"),
            "{:?}",
            d.text()
        );
    }

    #[test]
    fn word_delete_with_selection_deletes_only_selection() {
        let mut d = doc("one two three");
        d.set_cursor(0, 8);
        d.apply(Action::Select(Motion::Right), Instant::now());
        assert!(d.delete_word(false, Instant::now()));
        assert_eq!(d.text(), "one two hree");
    }

    #[test]
    fn enter_continues_lists() {
        let mut d = doc("- one");
        d.set_cursor(0, 5);
        assert!(d.enter(Instant::now()));
        assert_eq!(d.text(), "- one\n- ");
        assert_eq!(d.cursor(), (1, 2));
        assert_in_sync(&d);
    }

    #[test]
    fn enter_on_empty_item_ends_list() {
        let mut d = doc("- one\n- ");
        d.set_cursor(1, 2);
        assert!(d.enter(Instant::now()));
        assert_eq!(d.text(), "- one\n");
        assert_eq!(d.cursor(), (1, 0));
    }

    #[test]
    fn enter_on_plain_line_is_newline() {
        let mut d = doc("text");
        d.set_cursor(0, 4);
        assert!(d.enter(Instant::now()));
        assert_eq!(d.text(), "text\n");
    }

    #[test]
    fn ctrl_l_toggles_checkbox_and_keeps_cursor() {
        let mut d = doc("milk\neggs");
        d.set_cursor(1, 2);
        assert!(d.toggle_task(Instant::now()));
        assert_eq!(d.text(), "milk\n- [ ] eggs");
        assert_eq!(d.cursor(), (1, 8));
        assert!(d.toggle_task(Instant::now()));
        assert_eq!(d.text(), "milk\n- [x] eggs");
        assert_in_sync(&d);
        assert!(d.undo());
        assert_eq!(d.text(), "milk\n- [ ] eggs");
    }

    #[test]
    fn preview_tracks_revision() {
        let mut d = doc("# hi");
        assert!(d.preview_is_stale());
        d.refresh_preview();
        assert!(!d.preview_is_stale());
        assert!(!d.preview_items().is_empty());
        type_str(&mut d, "x");
        assert!(d.preview_is_stale());
    }
}
