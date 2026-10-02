//! Watching the notes folder for outside changes.

use std::path::{Path, PathBuf};
use std::time::Duration;

use iced::Subscription;
use iced::futures::channel::mpsc;
use iced::futures::{SinkExt, Stream, StreamExt};
use notify::event::{ModifyKind, RenameMode};
use notify::{Event, EventKind, RecommendedWatcher, RecursiveMode};
use notify_debouncer_full::{DebounceEventResult, Debouncer, RecommendedCache, new_debouncer};

use crate::fsio::DiskSnapshot;

const DEBOUNCE: Duration = Duration::from_millis(100);

pub type Handle = Debouncer<RecommendedWatcher, RecommendedCache>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FsChange {
    Created(PathBuf),
    Modified(PathBuf),
    Removed(PathBuf),
    Renamed { from: PathBuf, to: PathBuf },
}

impl FsChange {
    pub fn paths(&self) -> Vec<&Path> {
        match self {
            Self::Created(p) | Self::Modified(p) | Self::Removed(p) => vec![p],
            Self::Renamed { from, to } => vec![from, to],
        }
    }
}

/// False inside hidden folders (relative to `root`) and for our temp files.
pub fn is_relevant(root: &Path, path: &Path) -> bool {
    let relative = path.strip_prefix(root).unwrap_or(path);
    !relative
        .components()
        .any(|c| c.as_os_str().to_string_lossy().starts_with('.'))
}

pub fn changes_from_event(root: &Path, event: &Event) -> Vec<FsChange> {
    let relevant = |p: &PathBuf| is_relevant(root, p);
    let each = |make: fn(PathBuf) -> FsChange| -> Vec<FsChange> {
        event
            .paths
            .iter()
            .filter(|p| relevant(p))
            .cloned()
            .map(make)
            .collect()
    };
    match event.kind {
        EventKind::Create(_) => each(FsChange::Created),
        EventKind::Remove(_) => each(FsChange::Removed),
        EventKind::Modify(ModifyKind::Name(RenameMode::Both)) => match event.paths.as_slice() {
            [from, to] => match (relevant(from), relevant(to)) {
                (true, true) => vec![FsChange::Renamed {
                    from: from.clone(),
                    to: to.clone(),
                }],
                (false, true) => vec![FsChange::Created(to.clone())],
                (true, false) => vec![FsChange::Removed(from.clone())],
                (false, false) => vec![],
            },
            _ => vec![],
        },
        EventKind::Modify(ModifyKind::Name(RenameMode::From)) => each(FsChange::Removed),
        EventKind::Modify(ModifyKind::Name(_)) => each(FsChange::Created),
        EventKind::Modify(_) => each(FsChange::Modified),
        EventKind::Access(_) | EventKind::Any | EventKind::Other => vec![],
    }
}

/// Starts watching `root` recursively. Dropping the handle stops it.
pub fn start(
    root: PathBuf,
    mut on_changes: impl FnMut(Vec<FsChange>) + Send + 'static,
) -> notify::Result<Handle> {
    let watch_root = root.clone();
    let mut debouncer =
        new_debouncer(
            DEBOUNCE,
            None,
            move |result: DebounceEventResult| match result {
                Ok(events) => {
                    let changes: Vec<_> = events
                        .iter()
                        .flat_map(|e| changes_from_event(&root, &e.event))
                        .collect();
                    if !changes.is_empty() {
                        on_changes(changes);
                    }
                }
                Err(errors) => {
                    for err in errors {
                        tracing::warn!(%err, "file watcher error");
                    }
                }
            },
        )?;
    debouncer.watch(&watch_root, RecursiveMode::Recursive)?;
    Ok(debouncer)
}

pub fn subscription(root: PathBuf) -> Subscription<Vec<FsChange>> {
    Subscription::run_with(root, stream)
}

#[allow(clippy::ptr_arg)] // `Subscription::run_with` passes `&PathBuf`.
fn stream(root: &PathBuf) -> impl Stream<Item = Vec<FsChange>> + use<> {
    let root = root.clone();
    iced::stream::channel(16, async move |mut output: mpsc::Sender<Vec<FsChange>>| {
        let (tx, mut rx) = mpsc::unbounded();
        let _handle = match start(root, move |changes| {
            let _ = tx.unbounded_send(changes);
        }) {
            Ok(handle) => handle,
            Err(err) => {
                tracing::error!(%err, "file watching disabled");
                return;
            }
        };
        while let Some(changes) = rx.next().await {
            if output.send(changes).await.is_err() {
                break;
            }
        }
    })
}

/// What to do with an open note after its file changed on disk.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExternalAction {
    Ignore,
    Reload,
    Conflict,
    Deleted,
}

pub fn decide(dirty: bool, known: DiskSnapshot, on_disk: Option<DiskSnapshot>) -> ExternalAction {
    match on_disk {
        None => ExternalAction::Deleted,
        Some(snapshot) if snapshot == known => ExternalAction::Ignore,
        Some(_) if dirty => ExternalAction::Conflict,
        Some(_) => ExternalAction::Reload,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use notify::event::{CreateKind, DataChange, RemoveKind};
    use std::fs;
    use std::sync::mpsc as std_mpsc;

    fn event(kind: EventKind, paths: &[&str]) -> Event {
        paths
            .iter()
            .fold(Event::new(kind), |e, p| e.add_path(PathBuf::from(p)))
    }

    fn changes(kind: EventKind, paths: &[&str]) -> Vec<FsChange> {
        changes_from_event(Path::new("/n"), &event(kind, paths))
    }

    #[test]
    fn maps_basic_events() {
        assert_eq!(
            changes(EventKind::Create(CreateKind::File), &["/n/a.md"]),
            [FsChange::Created("/n/a.md".into())]
        );
        assert_eq!(
            changes(EventKind::Remove(RemoveKind::File), &["/n/a.md"]),
            [FsChange::Removed("/n/a.md".into())]
        );
        assert_eq!(
            changes(
                EventKind::Modify(ModifyKind::Data(DataChange::Content)),
                &["/n/a.md"]
            ),
            [FsChange::Modified("/n/a.md".into())]
        );
        assert!(
            changes(
                EventKind::Access(notify::event::AccessKind::Any),
                &["/n/a.md"]
            )
            .is_empty()
        );
    }

    #[test]
    fn maps_renames() {
        let both = EventKind::Modify(ModifyKind::Name(RenameMode::Both));
        assert_eq!(
            changes(both, &["/n/a.md", "/n/b.md"]),
            [FsChange::Renamed {
                from: "/n/a.md".into(),
                to: "/n/b.md".into()
            }]
        );
        assert_eq!(
            changes(both, &["/n/.a.md.markink-tmp", "/n/a.md"]),
            [FsChange::Created("/n/a.md".into())]
        );
        assert_eq!(
            changes(both, &["/n/a.md", "/n/.trash/a.md"]),
            [FsChange::Removed("/n/a.md".into())]
        );
    }

    #[test]
    fn hidden_paths_are_relative_to_root() {
        assert!(!is_relevant(Path::new("/n"), Path::new("/n/.git/index")));
        assert!(is_relevant(
            Path::new("/home/u/.notes"),
            Path::new("/home/u/.notes/a.md")
        ));
    }

    #[test]
    fn decision_table() {
        let a = DiskSnapshot::of(b"a");
        let b = DiskSnapshot::of(b"b");
        assert_eq!(decide(false, a, Some(a)), ExternalAction::Ignore);
        assert_eq!(decide(true, a, Some(a)), ExternalAction::Ignore);
        assert_eq!(decide(false, a, Some(b)), ExternalAction::Reload);
        assert_eq!(decide(true, a, Some(b)), ExternalAction::Conflict);
        assert_eq!(decide(false, a, None), ExternalAction::Deleted);
    }

    #[test]
    fn reports_real_changes() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        let (tx, rx) = std_mpsc::channel();
        let _handle = start(root.clone(), move |c| tx.send(c).unwrap()).unwrap();
        fs::write(root.join("a.md"), "x").unwrap();
        let got = rx.recv_timeout(Duration::from_secs(3)).unwrap();
        assert!(
            got.iter()
                .any(|c| c.paths().contains(&root.join("a.md").as_path())),
            "{got:?}"
        );
    }
}
