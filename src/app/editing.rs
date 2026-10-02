//! Opening notes, switching tabs, editing and saving.

use std::collections::{BTreeSet, HashSet};
use std::path::PathBuf;
use std::time::Instant;

use iced::widget::{operation, text_editor};
use iced::{Task, window};

use super::{EDITOR_ID, Message, State, blocking, delayed, list_dir, load_note, read_disk};
use crate::autosave::{AUTOSAVE_DELAY, retry_delay};
use crate::document::{DocId, DocStatus, Document};
use crate::fsio::{DiskSnapshot, LoadedNote, ReadError, SaveError};
use crate::shortcuts::EditorKey;
use crate::watcher::{self, ExternalAction, FsChange};

impl State {
    pub(super) fn open(&mut self, path: PathBuf) -> Task<Message> {
        match self.tabs.find(&path) {
            Some(id) => self.activate(id),
            None => load_note(path),
        }
    }

    pub(super) fn note_loaded(
        &mut self,
        path: PathBuf,
        result: Result<LoadedNote, ReadError>,
    ) -> Task<Message> {
        match result {
            Ok(note) => {
                let previous = self.tabs.active_id();
                let id = self.tabs.open(path.clone(), note);
                if let Some((_, line)) = self.pending_cursor.take_if(|(p, _)| *p == path)
                    && let Some(doc) = self.tabs.get_mut(id)
                {
                    doc.set_cursor(line, 0);
                }
                self.refresh_active_preview();
                Task::batch([
                    self.flush_previous(previous),
                    focus_editor(),
                    self.save_session(),
                ])
            }
            Err(err) => {
                self.notice = Some(format!(
                    "Cannot open {}: {err}",
                    self.relative(&path).display()
                ));
                Task::none()
            }
        }
    }

    /// Opens a note with the cursor at `line` (0-based).
    pub(super) fn open_at(&mut self, path: PathBuf, line: usize) -> Task<Message> {
        if let Some(id) = self.tabs.find(&path) {
            let task = self.activate(id);
            if let Some(doc) = self.tabs.get_mut(id) {
                doc.set_cursor(line, 0);
            }
            return Task::batch([task, focus_editor()]);
        }
        self.pending_cursor = Some((path.clone(), line));
        load_note(path)
    }

    pub(super) fn activate(&mut self, id: DocId) -> Task<Message> {
        let previous = self.tabs.active_id();
        if previous == Some(id) || !self.tabs.activate(id) {
            return Task::none();
        }
        self.refresh_active_preview();
        Task::batch([
            self.flush_previous(previous),
            focus_editor(),
            self.sync_preview(id),
        ])
    }

    pub(super) fn cycle_tab(&mut self, forward: bool) -> Task<Message> {
        let previous = self.tabs.active_id();
        self.tabs.cycle(forward);
        self.refresh_active_preview();
        Task::batch([self.flush_previous(previous), focus_editor()])
    }

    /// Closes a tab once its edits are on disk; never drops unsaved text.
    pub(super) fn close_tab(&mut self, id: DocId) -> Task<Message> {
        let Some(doc) = self.tabs.get_mut(id) else {
            return Task::none();
        };
        if !doc.is_dirty() && !doc.saving {
            self.tabs.close(id);
            return self.save_session();
        }
        if doc.saving || doc.can_autosave() {
            doc.close_requested = true;
            return self.flush(id);
        }
        self.notice = Some(format!(
            "{} has unsaved changes. Resolve its banner first.",
            doc.title()
        ));
        Task::none()
    }

    pub(super) fn edit(&mut self, id: DocId, action: text_editor::Action) -> Task<Message> {
        let Some(doc) = self.tabs.get_mut(id) else {
            return Task::none();
        };
        if let text_editor::Action::Scroll { lines } = action {
            doc.view.scrolled(lines as f32, doc.line_count());
            doc.apply(action, Instant::now());
            return self.sync_preview(id);
        }
        let changed = doc.apply(action, Instant::now());
        self.after_move(id, changed)
    }

    /// Keeps the cursor in the estimated view, syncs the preview and, if the text changed, restarts timers.
    fn after_move(&mut self, id: DocId, changed: bool) -> Task<Message> {
        let visible = self.visible_lines();
        if let Some(doc) = self.tabs.get_mut(id) {
            doc.view.follow_cursor(doc.cursor().0, visible);
        }
        let sync = self.sync_preview(id);
        if changed {
            Task::batch([self.after_change(id), sync])
        } else {
            sync
        }
    }

    pub(super) fn editor_key(&mut self, id: DocId, key: EditorKey) -> Task<Message> {
        let Some(doc) = self.tabs.get_mut(id) else {
            return Task::none();
        };
        let now = Instant::now();
        let changed = match key {
            EditorKey::DeleteWordBack => doc.delete_word(false, now),
            EditorKey::DeleteWordForward => doc.delete_word(true, now),
            EditorKey::Enter => doc.enter(now),
            EditorKey::Indent => {
                doc.apply(text_editor::Action::Edit(text_editor::Edit::Indent), now)
            }
            EditorKey::Unindent => {
                doc.apply(text_editor::Action::Edit(text_editor::Edit::Unindent), now)
            }
            EditorKey::ToggleTask => doc.toggle_task(now),
        };
        self.after_move(id, changed)
    }

    /// Undo (`true`) or redo in the active note.
    pub(super) fn undo(&mut self, undo: bool) -> Task<Message> {
        let Some(doc) = self.tabs.active_mut() else {
            return Task::none();
        };
        let id = doc.id();
        let changed = if undo { doc.undo() } else { doc.redo() };
        self.after_move(id, changed)
    }

    /// Timers to restart after a note's text changed.
    pub(super) fn after_change(&mut self, id: DocId) -> Task<Message> {
        Task::batch([self.schedule_save(id), self.schedule_preview(id)])
    }

    fn schedule_save(&self, id: DocId) -> Task<Message> {
        let Some(doc) = self.tabs.get(id) else {
            return Task::none();
        };
        let revision = doc.revision();
        delayed(AUTOSAVE_DELAY, Message::SaveDue(id, revision))
    }

    pub(super) fn save_due(&mut self, id: DocId, revision: u64) -> Task<Message> {
        let due = self
            .tabs
            .get(id)
            .is_some_and(|d| d.revision() == revision && d.can_autosave());
        if due {
            self.save(id, false)
        } else {
            Task::none()
        }
    }

    /// Starts a background save. `force` overwrites whatever is on disk.
    pub(super) fn save(&mut self, id: DocId, force: bool) -> Task<Message> {
        let Some(doc) = self.tabs.get_mut(id) else {
            return Task::none();
        };
        let request = doc.begin_save(force);
        Task::perform(
            blocking(move || request.run()),
            move |(revision, result)| Message::Saved(id, revision, result),
        )
    }

    /// Saves now if autosave is allowed.
    pub(super) fn flush(&mut self, id: DocId) -> Task<Message> {
        if self.tabs.get(id).is_some_and(Document::can_autosave) {
            self.save(id, false)
        } else {
            Task::none()
        }
    }

    fn flush_previous(&mut self, previous: Option<DocId>) -> Task<Message> {
        match previous {
            Some(id) if self.tabs.active_id() != Some(id) => self.flush(id),
            _ => Task::none(),
        }
    }

    pub(super) fn flush_all(&mut self) -> Task<Message> {
        let ids: Vec<DocId> = self
            .tabs
            .iter()
            .filter(|d| d.can_autosave())
            .map(Document::id)
            .collect();
        Task::batch(ids.into_iter().map(|id| self.save(id, false)))
    }

    pub(super) fn saved(
        &mut self,
        id: DocId,
        revision: u64,
        result: Result<DiskSnapshot, SaveError>,
    ) -> Task<Message> {
        let Some(doc) = self.tabs.get_mut(id) else {
            return Task::none();
        };
        doc.saving = false;
        let (mut resave, mut close, mut retry, mut recheck) = (false, false, None, false);
        match result {
            Ok(snapshot) => {
                doc.mark_saved(revision, snapshot);
                recheck = std::mem::take(&mut doc.recheck_after_save);
                resave = doc.is_dirty();
                close = !resave && doc.close_requested;
            }
            Err(SaveError::Conflict) => doc.status = DocStatus::Conflict,
            Err(SaveError::Missing) => doc.status = DocStatus::DeletedOnDisk,
            Err(SaveError::Io(error)) => {
                let attempt = match &doc.status {
                    DocStatus::SaveFailed { attempt, .. } => attempt + 1,
                    _ => 0,
                };
                tracing::warn!(path = %doc.path().display(), %error, attempt, "save failed");
                doc.status = DocStatus::SaveFailed { error, attempt };
                retry = Some(retry_delay(attempt));
            }
        }
        if !matches!(doc.status, DocStatus::Normal) {
            doc.close_requested = false;
        }
        let mut tasks = Vec::new();
        if resave {
            tasks.push(if self.quitting {
                self.flush(id)
            } else {
                self.schedule_save(id)
            });
        }
        if close {
            self.tabs.close(id);
            tasks.push(self.save_session());
        }
        if let Some(delay) = retry {
            tasks.push(delayed(delay, Message::RetrySave(id)));
        }
        if recheck {
            tasks.push(self.recheck(id));
        }
        tasks.push(self.try_exit());
        Task::batch(tasks)
    }

    pub(super) fn fs_changes(&mut self, changes: Vec<FsChange>) -> Task<Message> {
        let mut listings = BTreeSet::new();
        let mut touched = HashSet::new();
        for change in &changes {
            match change {
                FsChange::Renamed { from, to } => {
                    self.tabs.rename_path(from, to);
                    self.tree.forget(from);
                }
                FsChange::Removed(path) => self.tree.forget(path),
                FsChange::Created(_) | FsChange::Modified(_) => {}
            }
            for path in change.paths() {
                listings.extend(self.tree.affected_listing(path));
                touched.insert(path.to_path_buf());
            }
        }
        let structural = changes.iter().any(|c| !matches!(c, FsChange::Modified(_)));
        let mut tasks: Vec<_> = listings.into_iter().map(list_dir).collect();
        if structural {
            tasks.push(super::palette::build_index(self.root.clone()));
        }
        let ids: Vec<DocId> = self
            .tabs
            .iter()
            .filter(|d| touched.contains(d.path()))
            .map(Document::id)
            .collect();
        tasks.extend(ids.into_iter().map(|id| self.recheck(id)));
        Task::batch(tasks)
    }

    /// Compares a note with disk, or defers until its save lands.
    pub(super) fn recheck(&mut self, id: DocId) -> Task<Message> {
        let Some(doc) = self.tabs.get_mut(id) else {
            return Task::none();
        };
        if doc.saving {
            doc.recheck_after_save = true;
            return Task::none();
        }
        read_disk(id, doc.path().to_path_buf(), Message::DiskRead)
    }

    pub(super) fn disk_read(
        &mut self,
        id: DocId,
        result: Result<LoadedNote, ReadError>,
    ) -> Task<Message> {
        let Some(doc) = self.tabs.get_mut(id) else {
            return Task::none();
        };
        if doc.saving {
            doc.recheck_after_save = true;
            return Task::none();
        }
        let on_disk = match &result {
            Ok(note) => Some(note.snapshot),
            Err(ReadError::NotFound) => None,
            Err(err) => {
                tracing::warn!(path = %doc.path().display(), %err, "cannot re-read note");
                return Task::none();
            }
        };
        match watcher::decide(doc.is_dirty(), doc.disk(), on_disk) {
            ExternalAction::Ignore => {
                if matches!(doc.status, DocStatus::Conflict | DocStatus::DeletedOnDisk) {
                    doc.status = DocStatus::Normal;
                    return self.flush(id);
                }
            }
            ExternalAction::Reload => {
                if let Ok(note) = result {
                    doc.reload(note);
                    self.refresh_active_preview();
                }
            }
            ExternalAction::Conflict => doc.status = DocStatus::Conflict,
            ExternalAction::Deleted => doc.status = DocStatus::DeletedOnDisk,
        }
        Task::none()
    }

    /// Writes our version regardless of disk (Keep mine, Recreate).
    pub(super) fn overwrite(&mut self, id: DocId) -> Task<Message> {
        let Some(doc) = self.tabs.get_mut(id) else {
            return Task::none();
        };
        doc.status = DocStatus::Normal;
        self.save(id, true)
    }

    pub(super) fn disk_version_loaded(
        &mut self,
        id: DocId,
        result: Result<LoadedNote, ReadError>,
    ) -> Task<Message> {
        match (self.tabs.get_mut(id), result) {
            (Some(doc), Ok(note)) => {
                doc.reload(note);
                self.refresh_active_preview();
            }
            (Some(doc), Err(err)) => {
                self.notice = Some(format!("Cannot load {}: {err}", doc.title()));
            }
            (None, _) => {}
        }
        Task::none()
    }

    pub(super) fn window_event(&mut self, event: window::Event) -> Task<Message> {
        match event {
            window::Event::Unfocused => self.flush_all(),
            window::Event::Resized(size) | window::Event::Opened { size, .. } => {
                self.window_height = size.height;
                Task::none()
            }
            window::Event::CloseRequested => {
                self.quitting = true;
                let flush = self.flush_all();
                Task::batch([flush, self.try_exit()])
            }
            _ => Task::none(),
        }
    }

    /// Exits once no save is in flight; asks first if edits would be lost.
    pub(super) fn try_exit(&mut self) -> Task<Message> {
        if !self.quitting || self.tabs.iter().any(|d| d.saving) {
            return Task::none();
        }
        self.quitting = false;
        if self.tabs.iter().any(Document::is_dirty) {
            self.confirm_quit = true;
            return Task::none();
        }
        self.exit()
    }

    pub(super) fn exit(&mut self) -> Task<Message> {
        self.save_session_now();
        iced::exit()
    }
}

pub(super) fn focus_editor() -> Task<Message> {
    operation::focus(EDITOR_ID)
}
