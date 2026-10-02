//! Saving and restoring the workspace session.

use std::path::{Path, PathBuf};

use iced::Task;

use super::editing::focus_editor;
use super::{Message, State, blocking, list_dir};
use crate::fsio::{self, LoadedNote};
use crate::session::{self, Session, TabState};

#[derive(Debug, Clone)]
pub struct RestoredTab {
    pub path: PathBuf,
    pub note: LoadedNote,
    pub cursor: (usize, usize),
    pub preview: bool,
}

#[derive(Debug, Clone)]
pub struct RestoredTabs {
    pub tabs: Vec<RestoredTab>,
    pub active: Option<PathBuf>,
}

/// Reads session tabs in order, skipping notes that can no longer be opened.
pub fn load_tabs(root: &Path, tabs: Vec<TabState>, active: Option<usize>) -> RestoredTabs {
    let active = active.and_then(|i| tabs.get(i)).map(|t| root.join(&t.path));
    let tabs = tabs
        .into_iter()
        .filter_map(|tab| {
            let path = root.join(&tab.path);
            match fsio::read_note(&path) {
                Ok(note) => Some(RestoredTab {
                    path,
                    note,
                    cursor: tab.cursor,
                    preview: tab.preview,
                }),
                Err(err) => {
                    tracing::info!(path = %path.display(), %err, "not restoring tab");
                    None
                }
            }
        })
        .collect();
    RestoredTabs { tabs, active }
}

impl State {
    pub(super) fn capture_session(&self) -> Session {
        let mut tabs = Vec::new();
        let mut active_tab = None;
        for doc in self.tabs.iter() {
            let Ok(path) = doc.path().strip_prefix(&self.root) else {
                continue;
            };
            if Some(doc.id()) == self.tabs.active_id() {
                active_tab = Some(tabs.len());
            }
            tabs.push(TabState {
                path: path.to_path_buf(),
                cursor: doc.cursor(),
                preview: doc.preview_visible,
            });
        }
        let expanded_dirs = self
            .tree
            .expanded()
            .into_iter()
            .filter_map(|d| d.strip_prefix(&self.root).ok().map(Path::to_path_buf))
            .collect();
        Session {
            tabs,
            active_tab,
            expanded_dirs,
            sidebar_visible: self.sidebar_visible,
            ..Session::default()
        }
    }

    pub(super) fn restore_session(&mut self, session: Session) -> Task<Message> {
        self.sidebar_visible = session.sidebar_visible;
        let mut tasks = Vec::new();
        for dir in session.expanded_dirs {
            let dir = self.root.join(dir);
            if self.tree.expand(dir.clone()) {
                tasks.push(list_dir(dir));
            }
        }
        if !session.tabs.is_empty() {
            let root = self.root.clone();
            let (tabs, active) = (session.tabs, session.active_tab);
            tasks.push(Task::perform(
                blocking(move || load_tabs(&root, tabs, active)),
                Message::SessionTabsLoaded,
            ));
        }
        Task::batch(tasks)
    }

    pub(super) fn session_tabs_loaded(&mut self, restored: RestoredTabs) -> Task<Message> {
        for tab in restored.tabs {
            let id = self.tabs.open(tab.path, tab.note);
            if let Some(doc) = self.tabs.get_mut(id) {
                doc.set_cursor(tab.cursor.0, tab.cursor.1);
                doc.preview_visible = tab.preview && doc.preview_allowed();
            }
        }
        if let Some(id) = restored.active.and_then(|path| self.tabs.find(&path)) {
            self.tabs.activate(id);
        }
        let open = self
            .launch_open
            .take()
            .map_or_else(Task::none, |path| self.open(path));
        self.refresh_active_preview();
        Task::batch([open, focus_editor()])
    }

    /// Writes the session in the background.
    pub(super) fn save_session(&self) -> Task<Message> {
        let Some(path) = self.session_path.clone() else {
            return Task::none();
        };
        let session = self.capture_session();
        Task::perform(
            blocking(move || session::save(&path, &session).map_err(|e| e.to_string())),
            Message::SessionSaved,
        )
    }

    /// Writes the session synchronously; used on exit.
    pub(super) fn save_session_now(&self) {
        if let Some(path) = &self.session_path
            && let Err(err) = session::save(path, &self.capture_session())
        {
            tracing::warn!(%err, "cannot save session");
        }
    }
}
