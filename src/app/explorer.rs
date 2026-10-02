//! Sidebar tree interactions and file operations.

use std::path::PathBuf;

use iced::Task;
use iced::widget::operation;

use super::palette::build_index;
use super::{Message, State, blocking, list_dir};
use crate::workspace::{self, Entry, EntryKind};

pub const PROMPT_INPUT_ID: &str = "prompt-input";

pub enum PromptKind {
    NewNote(PathBuf),
    NewFolder(PathBuf),
    Rename(PathBuf),
}

pub struct Prompt {
    pub kind: PromptKind,
    pub value: String,
}

#[derive(Debug, Clone)]
pub enum FileOp {
    CreatedNote(PathBuf),
    CreatedDir(PathBuf),
    Renamed { from: PathBuf, to: PathBuf },
    Trashed(PathBuf),
}

impl State {
    pub(super) fn dir_listed(
        &mut self,
        dir: PathBuf,
        result: Result<Vec<Entry>, String>,
    ) -> Task<Message> {
        match result {
            Ok(entries) => self.tree.set_listing(dir, entries),
            Err(err) => {
                tracing::warn!(dir = %dir.display(), %err, "cannot list folder");
                self.tree.forget(&dir);
            }
        }
        Task::none()
    }

    pub(super) fn entry_clicked(&mut self, entry: Entry) -> Task<Message> {
        self.tree.selected = Some(entry.path.clone());
        match entry.kind {
            EntryKind::Dir if self.tree.toggle(&entry.path) => list_dir(entry.path),
            EntryKind::Dir => Task::none(),
            EntryKind::Note => self.open(entry.path),
        }
    }

    fn start_prompt(&mut self, kind: PromptKind) -> Task<Message> {
        let value = match &kind {
            PromptKind::Rename(path) => path
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned(),
            PromptKind::NewNote(_) | PromptKind::NewFolder(_) => String::new(),
        };
        self.prompt = Some(Prompt { kind, value });
        operation::focus(PROMPT_INPUT_ID)
    }

    /// The selected item, unless it is the root.
    fn selected_item(&self) -> Option<PathBuf> {
        self.tree.selected.clone().filter(|p| p != &self.root)
    }

    pub(super) fn new_note(&mut self) -> Task<Message> {
        self.start_prompt(PromptKind::NewNote(self.tree.target_dir()))
    }

    pub(super) fn new_folder(&mut self) -> Task<Message> {
        self.start_prompt(PromptKind::NewFolder(self.tree.target_dir()))
    }

    pub(super) fn rename_selected(&mut self) -> Task<Message> {
        match self.selected_item() {
            Some(path) => self.start_prompt(PromptKind::Rename(path)),
            None => Task::none(),
        }
    }

    pub(super) fn prompt_input(&mut self, value: String) -> Task<Message> {
        if let Some(prompt) = &mut self.prompt {
            prompt.value = value;
        }
        Task::none()
    }

    pub(super) fn prompt_submit(&mut self) -> Task<Message> {
        let Some(Prompt { kind, value }) = self.prompt.take() else {
            return Task::none();
        };
        if let PromptKind::Rename(path) = &kind
            && self
                .tabs
                .under(path)
                .iter()
                .any(|&id| self.tabs.get(id).is_some_and(|d| d.saving))
        {
            self.notice = Some("A save is in progress. Try renaming again.".into());
            return Task::none();
        }
        let work = move || -> Result<FileOp, String> {
            let result = match kind {
                PromptKind::NewNote(dir) => {
                    workspace::create_note(&dir, &value).map(FileOp::CreatedNote)
                }
                PromptKind::NewFolder(dir) => {
                    workspace::create_dir(&dir, &value).map(FileOp::CreatedDir)
                }
                PromptKind::Rename(from) => {
                    workspace::rename(&from, &value).map(|to| FileOp::Renamed { from, to })
                }
            };
            result.map_err(|e| e.to_string())
        };
        Task::perform(blocking(work), Message::FileOpDone)
    }

    /// Moves the selected item to the trash, closing its tabs first.
    pub(super) fn trash_selected(&mut self) -> Task<Message> {
        let Some(path) = self.selected_item() else {
            return Task::none();
        };
        for id in self.tabs.under(&path) {
            self.tabs.close(id);
        }
        let target = path.clone();
        let work = move || {
            workspace::move_to_trash(&target)
                .map(|()| FileOp::Trashed(path))
                .map_err(|e| e.to_string())
        };
        Task::perform(blocking(work), Message::FileOpDone)
    }

    pub(super) fn file_op_done(&mut self, result: Result<FileOp, String>) -> Task<Message> {
        let op = match result {
            Ok(op) => op,
            Err(err) => {
                self.notice = Some(err);
                return Task::none();
            }
        };
        let mut tasks = vec![build_index(self.root.clone())];
        let changed = match &op {
            FileOp::CreatedNote(path) | FileOp::CreatedDir(path) => path.clone(),
            FileOp::Renamed { from, to } => {
                self.tabs.rename_path(from, to);
                self.tree.forget(from);
                let moved = self.tabs.under(to);
                tasks.extend(moved.into_iter().map(|id| self.recheck(id)));
                to.clone()
            }
            FileOp::Trashed(path) => {
                self.tree.forget(path);
                path.clone()
            }
        };
        if let Some(parent) = changed.parent() {
            if parent != self.root {
                self.tree.expand(parent.to_path_buf());
            }
            tasks.push(list_dir(parent.to_path_buf()));
        }
        if !matches!(op, FileOp::Trashed(_)) {
            self.tree.selected = Some(changed);
        }
        if let FileOp::CreatedNote(path) = op {
            tasks.push(self.open(path));
        }
        Task::batch(tasks)
    }
}
