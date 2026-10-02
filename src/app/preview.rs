//! Preview toggling and link handling.

use std::path::{Path, PathBuf};

use iced::Task;

use super::{Message, State, delayed};
use crate::autosave::PREVIEW_DELAY;
use crate::document::{DocId, LARGE_NOTE_BYTES};
use crate::workspace;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Link {
    External(String),
    Note(PathBuf),
    Unsupported,
}

/// Classifies a clicked link relative to the note at `current`.
pub fn resolve_link(current: Option<&Path>, uri: &str) -> Link {
    if let Some((scheme, _)) = uri.split_once(':')
        && !scheme.is_empty()
        && scheme
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "+-.".contains(c))
    {
        let allowed = ["http", "https", "mailto"]
            .iter()
            .any(|s| scheme.eq_ignore_ascii_case(s));
        return if allowed {
            Link::External(uri.to_owned())
        } else {
            Link::Unsupported
        };
    }
    let target = uri
        .split('#')
        .next()
        .unwrap_or_default()
        .replace("%20", " ");
    let Some(dir) = current.and_then(Path::parent) else {
        return Link::Unsupported;
    };
    if target.is_empty() {
        return Link::Unsupported;
    }
    let path = dir.join(target);
    if workspace::is_note(&path) {
        Link::Note(path)
    } else if path
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("desktop"))
    {
        Link::Unsupported
    } else {
        Link::External(path.to_string_lossy().into_owned())
    }
}

impl State {
    pub(super) fn toggle_preview(&mut self) -> Task<Message> {
        let Some(doc) = self.tabs.active_mut() else {
            return Task::none();
        };
        if !doc.preview_allowed() {
            let limit = LARGE_NOTE_BYTES / (1024 * 1024);
            self.notice = Some(format!("Preview is off for notes over {limit} MB."));
            return Task::none();
        }
        doc.preview_visible = !doc.preview_visible;
        self.refresh_active_preview();
        Task::none()
    }

    pub(super) fn schedule_preview(&self, id: DocId) -> Task<Message> {
        match self.tabs.get(id) {
            Some(doc) if doc.preview_visible => {
                let revision = doc.revision();
                delayed(PREVIEW_DELAY, Message::PreviewDue(id, revision))
            }
            _ => Task::none(),
        }
    }

    pub(super) fn preview_due(&mut self, id: DocId, revision: u64) -> Task<Message> {
        if let Some(doc) = self.tabs.get_mut(id)
            && doc.revision() == revision
            && doc.preview_visible
            && doc.preview_allowed()
        {
            doc.refresh_preview();
        }
        Task::none()
    }

    /// Parses the active note's preview if it is shown and out of date.
    pub(super) fn refresh_active_preview(&mut self) {
        if let Some(doc) = self.tabs.active_mut()
            && doc.preview_visible
            && doc.preview_is_stale()
            && doc.preview_allowed()
        {
            doc.refresh_preview();
        }
    }

    pub(super) fn link_clicked(&mut self, uri: String) -> Task<Message> {
        let current = self.tabs.active().map(|d| d.path().to_path_buf());
        match resolve_link(current.as_deref(), &uri) {
            Link::Note(path) => self.open(path),
            Link::External(target)
                if target.starts_with('/') && !safe_to_open(Path::new(&target)) =>
            {
                self.notice = Some(format!("Not opening {target}: executable or missing."));
                Task::none()
            }
            Link::External(target) => {
                std::thread::spawn(move || {
                    if let Err(err) = std::process::Command::new("xdg-open").arg(&target).status() {
                        tracing::warn!(%err, %target, "cannot open link");
                    }
                });
                Task::none()
            }
            Link::Unsupported => Task::none(),
        }
    }
}

/// Local files we hand to `xdg-open`: never executables or desktop entries.
pub fn safe_to_open(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    let is_desktop_entry = path
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("desktop"));
    !is_desktop_entry
        && std::fs::metadata(path).is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 == 0)
}
