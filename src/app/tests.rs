use std::path::PathBuf;

use iced::widget::text_editor::{Action, Edit};

use super::*;
use crate::workspace::EntryKind;

pub(super) fn state() -> State {
    State::new(PathBuf::from("/notes"))
}

pub(super) fn load(state: &mut State, name: &str, text: &str) -> DocId {
    let path = state.root.join(name);
    let _ = state.update(Message::NoteLoaded(
        path.clone(),
        Ok(LoadedNote::from_text(text)),
    ));
    state.tabs.find(&path).expect("note opened")
}

pub(super) fn type_char(state: &mut State, id: DocId, c: char) {
    let _ = state.update(Message::Edit(id, Action::Edit(Edit::Insert(c))));
}

fn text_of(state: &State, id: DocId) -> String {
    state.tabs.get(id).unwrap().text().to_owned()
}

#[test]
fn loaded_note_opens_and_activates() {
    let mut s = state();
    let a = load(&mut s, "a.md", "x");
    assert_eq!(s.tabs.active_id(), Some(a));
    assert_eq!(s.title(), "a.md - notes - MarkInk");
}

#[test]
fn unreadable_note_shows_notice() {
    let mut s = state();
    let _ = s.update(Message::NoteLoaded(
        "/notes/bin.md".into(),
        Err(ReadError::NotUtf8),
    ));
    assert!(s.tabs.is_empty());
    assert_eq!(
        s.notice.as_deref(),
        Some("Cannot open bin.md: not valid UTF-8 text")
    );
}

#[test]
fn clicking_open_note_activates_it() {
    let mut s = state();
    let a = load(&mut s, "a.md", "");
    load(&mut s, "b.md", "");
    let entry = Entry {
        path: "/notes/a.md".into(),
        name: "a.md".into(),
        kind: EntryKind::Note,
    };
    let _ = s.update(Message::EntryClicked(entry));
    assert_eq!(s.tabs.active_id(), Some(a));
    assert_eq!(s.tree.selected, Some(PathBuf::from("/notes/a.md")));
}

#[test]
fn clicking_folder_toggles_it() {
    let mut s = state();
    let entry = Entry {
        path: "/notes/work".into(),
        name: "work".into(),
        kind: EntryKind::Dir,
    };
    let _ = s.update(Message::EntryClicked(entry.clone()));
    assert!(s.tree.is_expanded(&entry.path));
    let _ = s.update(Message::EntryClicked(entry.clone()));
    assert!(!s.tree.is_expanded(&entry.path));
}

#[test]
fn edits_and_undo_reach_the_document() {
    let mut s = state();
    let a = load(&mut s, "a.md", "x");
    type_char(&mut s, a, 'z');
    assert_eq!(text_of(&s, a), "zx");
    let _ = s.update(Message::Shortcut(Command::Undo));
    assert_eq!(text_of(&s, a), "x");
    let _ = s.update(Message::Shortcut(Command::Redo));
    assert_eq!(text_of(&s, a), "zx");
}

#[test]
fn tab_shortcuts() {
    let mut s = state();
    let a = load(&mut s, "a.md", "");
    let b = load(&mut s, "b.md", "");
    let _ = s.update(Message::Shortcut(Command::NextTab));
    assert_eq!(s.tabs.active_id(), Some(a));
    let _ = s.update(Message::Shortcut(Command::CloseTab));
    assert_eq!(s.tabs.active_id(), Some(b));
    assert_eq!(s.tabs.len(), 1);
}

#[test]
fn sidebar_toggles() {
    let mut s = state();
    let _ = s.update(Message::Shortcut(Command::ToggleSidebar));
    assert!(!s.sidebar_visible);
}
use crate::document::DocStatus;
use crate::fsio::{DiskSnapshot, SaveError};
use iced::window;

fn doc(state: &State, id: DocId) -> &crate::document::Document {
    state.tabs.get(id).unwrap()
}

fn save_due(state: &mut State, id: DocId) {
    let revision = doc(state, id).revision();
    let _ = state.update(Message::SaveDue(id, revision));
}

fn saved(state: &mut State, id: DocId, result: Result<DiskSnapshot, SaveError>) {
    let revision = doc(state, id).revision();
    let _ = state.update(Message::Saved(id, revision, result));
}

#[test]
fn save_due_starts_save_for_current_revision() {
    let mut s = state();
    let a = load(&mut s, "a.md", "x");
    type_char(&mut s, a, 'y');
    let stale = doc(&s, a).revision() - 1;
    let _ = s.update(Message::SaveDue(a, stale));
    assert!(!doc(&s, a).saving);
    save_due(&mut s, a);
    assert!(doc(&s, a).saving);
}

#[test]
fn save_due_on_clean_doc_does_nothing() {
    let mut s = state();
    let a = load(&mut s, "a.md", "x");
    save_due(&mut s, a);
    assert!(!doc(&s, a).saving);
}

#[test]
fn successful_save_marks_clean() {
    let mut s = state();
    let a = load(&mut s, "a.md", "x");
    type_char(&mut s, a, 'y');
    save_due(&mut s, a);
    saved(&mut s, a, Ok(DiskSnapshot::of(b"yx")));
    assert!(!doc(&s, a).is_dirty());
    assert!(!doc(&s, a).saving);
}

#[test]
fn conflict_pauses_autosave() {
    let mut s = state();
    let a = load(&mut s, "a.md", "x");
    type_char(&mut s, a, 'y');
    save_due(&mut s, a);
    saved(&mut s, a, Err(SaveError::Conflict));
    assert_eq!(doc(&s, a).status, DocStatus::Conflict);
    save_due(&mut s, a);
    assert!(!doc(&s, a).saving);
}

#[test]
fn io_failures_count_attempts() {
    let mut s = state();
    let a = load(&mut s, "a.md", "x");
    type_char(&mut s, a, 'y');
    save_due(&mut s, a);
    saved(&mut s, a, Err(SaveError::Io("disk full".into())));
    let _ = s.update(Message::RetrySave(a));
    assert!(doc(&s, a).saving);
    saved(&mut s, a, Err(SaveError::Io("disk full".into())));
    assert_eq!(
        doc(&s, a).status,
        DocStatus::SaveFailed {
            error: "disk full".into(),
            attempt: 1
        }
    );
}

#[test]
fn switching_tabs_saves_previous() {
    let mut s = state();
    let a = load(&mut s, "a.md", "x");
    type_char(&mut s, a, 'y');
    load(&mut s, "b.md", "");
    assert!(doc(&s, a).saving);
}

#[test]
fn losing_focus_saves_everything() {
    let mut s = state();
    let a = load(&mut s, "a.md", "x");
    type_char(&mut s, a, 'y');
    let _ = s.update(Message::Window(window::Event::Unfocused));
    assert!(doc(&s, a).saving);
}

#[test]
fn closing_dirty_tab_waits_for_save() {
    let mut s = state();
    let a = load(&mut s, "a.md", "x");
    type_char(&mut s, a, 'y');
    let _ = s.update(Message::CloseTab(a));
    assert!(doc(&s, a).saving);
    saved(&mut s, a, Ok(DiskSnapshot::of(b"yx")));
    assert!(s.tabs.get(a).is_none());
}

#[test]
fn closing_conflicted_tab_keeps_it_open() {
    let mut s = state();
    let a = load(&mut s, "a.md", "x");
    type_char(&mut s, a, 'y');
    s.tabs.get_mut(a).unwrap().status = DocStatus::Conflict;
    let _ = s.update(Message::CloseTab(a));
    assert!(s.tabs.get(a).is_some());
    assert!(s.notice.is_some());
}

#[test]
fn quit_waits_for_saves() {
    let mut s = state();
    let a = load(&mut s, "a.md", "x");
    type_char(&mut s, a, 'y');
    let _ = s.update(Message::Window(window::Event::CloseRequested));
    assert!(s.quitting && doc(&s, a).saving);
    saved(&mut s, a, Ok(DiskSnapshot::of(b"yx")));
    assert!(!s.quitting && !s.confirm_quit);
}

#[test]
fn quit_with_unsaved_asks() {
    let mut s = state();
    let a = load(&mut s, "a.md", "x");
    type_char(&mut s, a, 'y');
    s.tabs.get_mut(a).unwrap().status = DocStatus::Conflict;
    let _ = s.update(Message::Window(window::Event::CloseRequested));
    assert!(s.confirm_quit);
    let _ = s.update(Message::CancelQuit);
    assert!(!s.confirm_quit);
}
use super::preview::{Link, resolve_link};
use std::path::Path;

#[test]
fn toggling_preview_parses_once() {
    let mut s = state();
    let a = load(&mut s, "a.md", "# Title");
    let _ = s.update(Message::Shortcut(Command::TogglePreview));
    assert!(doc(&s, a).preview_visible);
    assert!(!doc(&s, a).preview_is_stale());
    let _ = s.update(Message::Shortcut(Command::TogglePreview));
    assert!(!doc(&s, a).preview_visible);
}

#[test]
fn preview_refreshes_only_for_current_revision() {
    let mut s = state();
    let a = load(&mut s, "a.md", "x");
    let _ = s.update(Message::Shortcut(Command::TogglePreview));
    type_char(&mut s, a, 'y');
    let stale = doc(&s, a).revision() - 1;
    let _ = s.update(Message::PreviewDue(a, stale));
    assert!(doc(&s, a).preview_is_stale());
    let current = doc(&s, a).revision();
    let _ = s.update(Message::PreviewDue(a, current));
    assert!(!doc(&s, a).preview_is_stale());
}

#[test]
fn resolves_links() {
    let note = Path::new("/notes/work/a.md");
    assert_eq!(
        resolve_link(Some(note), "https://x.org"),
        Link::External("https://x.org".into())
    );
    assert_eq!(
        resolve_link(Some(note), "b.md#top"),
        Link::Note("/notes/work/b.md".into())
    );
    assert_eq!(
        resolve_link(Some(note), "my%20note.md"),
        Link::Note("/notes/work/my note.md".into())
    );
    assert_eq!(
        resolve_link(Some(note), "img.png"),
        Link::External(
            Path::new("/notes/work")
                .join("img.png")
                .to_string_lossy()
                .into()
        )
    );
    assert_eq!(resolve_link(Some(note), "#heading"), Link::Unsupported);
    assert_eq!(resolve_link(None, "b.md"), Link::Unsupported);
}
use crate::watcher::FsChange;

fn disk_read(state: &mut State, id: DocId, result: Result<LoadedNote, ReadError>) {
    let _ = state.update(Message::DiskRead(id, result));
}

#[test]
fn outside_edit_reloads_clean_note() {
    let mut s = state();
    let a = load(&mut s, "a.md", "x");
    disk_read(&mut s, a, Ok(LoadedNote::from_text("new")));
    assert_eq!(text_of(&s, a), "new");
    assert!(!doc(&s, a).is_dirty());
}

#[test]
fn outside_edit_on_dirty_note_conflicts() {
    let mut s = state();
    let a = load(&mut s, "a.md", "x");
    type_char(&mut s, a, 'y');
    disk_read(&mut s, a, Ok(LoadedNote::from_text("new")));
    assert_eq!(doc(&s, a).status, DocStatus::Conflict);
    assert_eq!(text_of(&s, a), "yx");
}

#[test]
fn unchanged_disk_is_ignored() {
    let mut s = state();
    let a = load(&mut s, "a.md", "x");
    type_char(&mut s, a, 'y');
    disk_read(&mut s, a, Ok(LoadedNote::from_text("x")));
    assert_eq!(doc(&s, a).status, DocStatus::Normal);
    assert_eq!(text_of(&s, a), "yx");
}

#[test]
fn deleted_file_marks_note() {
    let mut s = state();
    let a = load(&mut s, "a.md", "x");
    disk_read(&mut s, a, Err(ReadError::NotFound));
    assert_eq!(doc(&s, a).status, DocStatus::DeletedOnDisk);
}

#[test]
fn disk_read_during_save_is_deferred() {
    let mut s = state();
    let a = load(&mut s, "a.md", "x");
    type_char(&mut s, a, 'y');
    save_due(&mut s, a);
    disk_read(&mut s, a, Ok(LoadedNote::from_text("yx")));
    assert_eq!(doc(&s, a).status, DocStatus::Normal);
    assert!(doc(&s, a).recheck_after_save);
}

#[test]
fn keep_mine_forces_save() {
    let mut s = state();
    let a = load(&mut s, "a.md", "x");
    type_char(&mut s, a, 'y');
    disk_read(&mut s, a, Ok(LoadedNote::from_text("new")));
    let _ = s.update(Message::KeepMine(a));
    assert_eq!(doc(&s, a).status, DocStatus::Normal);
    assert!(doc(&s, a).saving);
}

#[test]
fn load_disk_version_replaces_text() {
    let mut s = state();
    let a = load(&mut s, "a.md", "x");
    type_char(&mut s, a, 'y');
    disk_read(&mut s, a, Ok(LoadedNote::from_text("new")));
    let _ = s.update(Message::DiskVersionLoaded(
        a,
        Ok(LoadedNote::from_text("new")),
    ));
    assert_eq!(text_of(&s, a), "new");
    assert_eq!(doc(&s, a).status, DocStatus::Normal);
}

#[test]
fn discard_closes_without_saving() {
    let mut s = state();
    let a = load(&mut s, "a.md", "x");
    type_char(&mut s, a, 'y');
    let _ = s.update(Message::Discard(a));
    assert!(s.tabs.get(a).is_none());
}

#[test]
fn rename_on_disk_moves_tab() {
    let mut s = state();
    let a = load(&mut s, "a.md", "x");
    let change = FsChange::Renamed {
        from: "/notes/a.md".into(),
        to: "/notes/b.md".into(),
    };
    let _ = s.update(Message::FsChanges(vec![change]));
    assert_eq!(doc(&s, a).path(), Path::new("/notes/b.md"));
}
use super::palette::Palette;

fn with_index(state: &mut State, notes: &[&str]) {
    let paths = notes.iter().map(|n| state.root.join(n)).collect();
    let _ = state.update(Message::IndexBuilt(paths));
}

fn quick_open_results(state: &State) -> Vec<String> {
    match &state.palette {
        Some(Palette::QuickOpen { results, .. }) => results.clone(),
        _ => panic!("quick open not shown"),
    }
}

#[test]
fn quick_open_filters_and_toggles() {
    let mut s = state();
    with_index(&mut s, &["work/todo.md", "ideas.md"]);
    let _ = s.update(Message::Shortcut(Command::QuickOpen));
    assert_eq!(quick_open_results(&s), ["ideas.md", "work/todo.md"]);
    let _ = s.update(Message::PaletteQuery("todo".into()));
    assert_eq!(quick_open_results(&s)[0], "work/todo.md");
    let _ = s.update(Message::Shortcut(Command::QuickOpen));
    assert!(s.palette.is_none());
}

#[test]
fn quick_open_pick_opens_note() {
    let mut s = state();
    let a = load(&mut s, "a.md", "");
    load(&mut s, "b.md", "");
    with_index(&mut s, &["a.md", "b.md"]);
    let _ = s.update(Message::Shortcut(Command::QuickOpen));
    let _ = s.update(Message::PalettePick(0));
    assert!(s.palette.is_none());
    assert_eq!(s.tabs.active_id(), Some(a));
}

#[test]
fn arrows_move_selection_and_escape_closes() {
    let mut s = state();
    with_index(&mut s, &["a.md", "b.md"]);
    let _ = s.update(Message::Shortcut(Command::QuickOpen));
    s.palette_move(1);
    s.palette_move(5);
    assert!(matches!(
        s.palette,
        Some(Palette::QuickOpen { selected: 1, .. })
    ));
    let _ = s.update(Message::Shortcut(Command::Escape));
    assert!(s.palette.is_none());
}
use super::palette::SearchEvent;
use crate::search::SearchHit;

fn search_palette(state: &State) -> &super::palette::SearchPalette {
    match &state.palette {
        Some(Palette::Search(search)) => search,
        _ => panic!("search not shown"),
    }
}

#[test]
fn search_ignores_stale_generations() {
    let mut s = state();
    let _ = s.update(Message::Shortcut(Command::Search));
    let _ = s.update(Message::PaletteQuery("a".into()));
    let first = search_palette(&s).generation;
    let _ = s.update(Message::PaletteQuery("ab".into()));
    let hit = SearchHit {
        path: "/notes/a.md".into(),
        line: 2,
        text: "ab".into(),
    };
    let _ = s.update(Message::Search(
        first,
        SearchEvent::Batch(vec![hit.clone()]),
    ));
    assert!(search_palette(&s).hits.is_empty());
    let current = search_palette(&s).generation;
    let _ = s.update(Message::Search(current, SearchEvent::Batch(vec![hit])));
    assert_eq!(search_palette(&s).hits.len(), 1);
}

#[test]
fn new_query_cancels_running_search() {
    let mut s = state();
    let _ = s.update(Message::Shortcut(Command::Search));
    let _ = s.update(Message::PaletteQuery("a".into()));
    let generation = search_palette(&s).generation;
    let _ = s.update(Message::SearchDue(generation));
    let cancel = search_palette(&s).cancel.clone();
    let _ = s.update(Message::PaletteQuery("ab".into()));
    assert!(cancel.load(std::sync::atomic::Ordering::Relaxed));
}

#[test]
fn picking_hit_in_open_note_moves_cursor() {
    let mut s = state();
    let a = load(&mut s, "a.md", "one\ntwo\nthree");
    let _ = s.update(Message::Shortcut(Command::Search));
    let _ = s.update(Message::PaletteQuery("three".into()));
    let generation = search_palette(&s).generation;
    let hit = SearchHit {
        path: "/notes/a.md".into(),
        line: 3,
        text: "three".into(),
    };
    let _ = s.update(Message::Search(generation, SearchEvent::Batch(vec![hit])));
    let _ = s.update(Message::PalettePick(0));
    assert_eq!(doc(&s, a).cursor(), (2, 0));
}

#[test]
fn picking_hit_in_closed_note_positions_after_load() {
    let mut s = state();
    let task = s.open_at("/notes/b.md".into(), 1);
    drop(task);
    let b = load(&mut s, "b.md", "x\ny");
    assert_eq!(doc(&s, b).cursor(), (1, 0));
}
use super::explorer::{FileOp, PromptKind};

#[test]
fn new_note_prompt_targets_selected_folder() {
    let mut s = state();
    s.tree.selected = Some("/notes/work/a.md".into());
    let _ = s.update(Message::Shortcut(Command::NewNote));
    let prompt = s.prompt.as_ref().unwrap();
    assert!(matches!(&prompt.kind, PromptKind::NewNote(dir) if dir == Path::new("/notes/work")));
    let _ = s.update(Message::Shortcut(Command::Escape));
    assert!(s.prompt.is_none());
}

#[test]
fn rename_prompt_prefills_name_and_skips_root() {
    let mut s = state();
    let _ = s.update(Message::Shortcut(Command::Rename));
    assert!(s.prompt.is_none());
    s.tree.selected = Some("/notes/a.md".into());
    let _ = s.update(Message::Shortcut(Command::Rename));
    assert_eq!(s.prompt.as_ref().unwrap().value, "a.md");
}

#[test]
fn rename_done_moves_tabs_and_selection() {
    let mut s = state();
    let a = load(&mut s, "a.md", "");
    let op = FileOp::Renamed {
        from: "/notes/a.md".into(),
        to: "/notes/b.md".into(),
    };
    let _ = s.update(Message::FileOpDone(Ok(op)));
    assert_eq!(doc(&s, a).path(), Path::new("/notes/b.md"));
    assert_eq!(s.tree.selected, Some(PathBuf::from("/notes/b.md")));
}

#[test]
fn trashing_closes_tabs_under_path() {
    let mut s = state();
    let a = load(&mut s, "work/a.md", "");
    s.tree.selected = Some("/notes/work".into());
    let _ = s.update(Message::TrashSelected);
    assert!(s.tabs.get(a).is_none());
}

#[test]
fn rename_refused_while_saving() {
    let mut s = state();
    let a = load(&mut s, "a.md", "x");
    type_char(&mut s, a, 'y');
    save_due(&mut s, a);
    s.tree.selected = Some("/notes/a.md".into());
    let _ = s.update(Message::Shortcut(Command::Rename));
    let _ = s.update(Message::PromptInput("b".into()));
    let _ = s.update(Message::PromptSubmit);
    assert!(s.notice.is_some());
}

#[test]
fn failed_file_op_shows_notice() {
    let mut s = state();
    let _ = s.update(Message::FileOpDone(Err("exists".into())));
    assert_eq!(s.notice.as_deref(), Some("exists"));
}
use super::session::{RestoredTab, RestoredTabs, load_tabs};
use crate::session::{Session, TabState};

#[test]
fn captures_relative_session() {
    let mut s = state();
    load(&mut s, "a.md", "x\ny");
    let b = load(&mut s, "work/b.md", "");
    s.tabs.get_mut(b).unwrap().preview_visible = true;
    s.tree.expand("/notes/work".into());
    let session = s.capture_session();
    let paths: Vec<_> = session.tabs.iter().map(|t| t.path.clone()).collect();
    assert_eq!(paths, [PathBuf::from("a.md"), PathBuf::from("work/b.md")]);
    assert_eq!(session.active_tab, Some(1));
    assert!(session.tabs[1].preview);
    assert_eq!(session.expanded_dirs, [PathBuf::from("work")]);
}

#[test]
fn restores_tabs_in_order_with_cursor_and_active() {
    let mut s = state();
    let tab = |name: &str, cursor| RestoredTab {
        path: s.root.join(name),
        note: LoadedNote::from_text("l0\nl1\nl2"),
        cursor,
        preview: false,
    };
    let restored = RestoredTabs {
        tabs: vec![tab("a.md", (2, 1)), tab("b.md", (0, 0))],
        active: Some(s.root.join("a.md")),
    };
    let _ = s.update(Message::SessionTabsLoaded(restored));
    let titles: Vec<_> = s.tabs.iter().map(|d| d.title()).collect();
    assert_eq!(titles, ["a.md", "b.md"]);
    let active = s.tabs.active().unwrap();
    assert_eq!(active.title(), "a.md");
    assert_eq!(active.cursor(), (2, 1));
}

#[test]
fn load_tabs_skips_missing_files() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("a.md"), "x").unwrap();
    let tabs = vec![
        TabState {
            path: "gone.md".into(),
            cursor: (0, 0),
            preview: false,
        },
        TabState {
            path: "a.md".into(),
            cursor: (0, 0),
            preview: false,
        },
    ];
    let restored = load_tabs(dir.path(), tabs, Some(0));
    assert_eq!(restored.tabs.len(), 1);
    assert_eq!(restored.active, Some(dir.path().join("gone.md")));
}

#[test]
fn restore_expands_folders_and_sidebar() {
    let mut s = state();
    let session = Session {
        expanded_dirs: vec!["work".into()],
        sidebar_visible: false,
        ..Session::default()
    };
    let _ = s.restore_session(session);
    assert!(s.tree.is_expanded(Path::new("/notes/work")));
    assert!(!s.sidebar_visible);
}

use iced::keyboard::key::{Code, Physical};
use iced::{event, keyboard::Modifiers};

fn escape_event() -> iced::Event {
    let key = keyboard::Key::Named(keyboard::key::Named::Escape);
    iced::Event::Keyboard(keyboard::Event::KeyPressed {
        key: key.clone(),
        modified_key: key,
        physical_key: Physical::Code(Code::Escape),
        location: keyboard::Location::Standard,
        modifiers: Modifiers::empty(),
        text: None,
        repeat: false,
    })
}

#[test]
fn captured_escape_still_closes_palette() {
    let mut s = state();
    let _ = s.update(Message::Shortcut(Command::QuickOpen));
    let message = super::escape_filter(
        escape_event(),
        event::Status::Captured,
        window::Id::unique(),
    );
    let _ = s.update(message.expect("escape mapped"));
    assert!(s.palette.is_none());
}

#[test]
fn escape_is_not_handled_twice() {
    let mut s = state();
    let _ = s.update(Message::Shortcut(Command::QuickOpen));
    let iced::Event::Keyboard(key_event) = escape_event() else {
        unreachable!()
    };
    let _ = s.update(Message::Key(key_event));
    assert!(s.palette.is_some(), "Escape arrives via escape_filter only");
}

#[test]
fn links_only_open_safe_targets() {
    let note = Path::new("/notes/a.md");
    assert_eq!(
        resolve_link(Some(note), "file:///etc/passwd"),
        Link::Unsupported
    );
    assert_eq!(
        resolve_link(Some(note), "javascript:alert(1)"),
        Link::Unsupported
    );
    assert_eq!(
        resolve_link(Some(note), "HTTPS://x.org"),
        Link::External("HTTPS://x.org".into())
    );
    assert_eq!(
        resolve_link(Some(note), "mailto:a@b.c"),
        Link::External("mailto:a@b.c".into())
    );
    assert_eq!(resolve_link(Some(note), "run.desktop"), Link::Unsupported);
}

#[test]
#[cfg(unix)]
fn executable_local_files_are_not_opened() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let script = dir.path().join("run.sh");
    let image = dir.path().join("pic.png");
    std::fs::write(&script, "#!/bin/sh").unwrap();
    std::fs::write(&image, "png").unwrap();
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
    assert!(!super::preview::safe_to_open(&script));
    assert!(super::preview::safe_to_open(&image));
}

use crate::shortcuts::EditorKey;

#[test]
fn editor_keys_edit_the_document() {
    let mut s = state();
    let a = load(&mut s, "a.md", "- one");
    s.tabs.get_mut(a).unwrap().set_cursor(0, 5);
    let _ = s.update(Message::EditorKey(a, EditorKey::Enter));
    assert_eq!(text_of(&s, a), "- one\n- ");
    let _ = s.update(Message::EditorKey(a, EditorKey::DeleteWordBack));
    assert!(doc(&s, a).text().len() < "- one\n- ".len());
    assert!(doc(&s, a).is_dirty());
}

#[test]
fn help_toggles_and_escape_closes_it() {
    let mut s = state();
    let _ = s.update(Message::Shortcut(Command::Help));
    assert!(s.help_visible);
    let _ = s.update(Message::Shortcut(Command::Help));
    assert!(!s.help_visible);
    let _ = s.update(Message::Shortcut(Command::Help));
    let _ = s.update(Message::Shortcut(Command::Escape));
    assert!(!s.help_visible);
}

use iced::widget::text_editor::Motion;

#[test]
fn editor_scrolling_moves_the_view_estimate() {
    let mut s = state();
    let a = load(&mut s, "a.md", &"x\n".repeat(200));
    let _ = s.update(Message::Edit(a, Action::Scroll { lines: 50 }));
    assert_eq!(doc(&s, a).view.top(), 50.0);
    let _ = s.update(Message::Edit(a, Action::Move(Motion::DocumentEnd)));
    assert!(doc(&s, a).view.top() > 150.0);
}

#[test]
fn window_height_sets_visible_lines() {
    let mut s = state();
    let tall = s.visible_lines();
    let _ = s.update(Message::Window(window::Event::Resized(iced::Size::new(
        800.0, 400.0,
    ))));
    assert!(s.visible_lines() < tall);
}

#[test]
fn opened_window_size_sets_visible_lines() {
    let mut s = state();
    let tall = s.visible_lines();
    let opened = window::Event::Opened {
        position: None,
        size: iced::Size::new(800.0, 400.0),
    };
    let _ = s.update(Message::Window(opened));
    assert!(s.visible_lines() < tall);
}

#[test]
#[cfg(windows)]
fn windows_executables_are_not_opened() {
    let dir = tempfile::tempdir().unwrap();
    for name in ["run.exe", "run.BAT", "x.ps1", "s.lnk"] {
        let path = dir.path().join(name);
        std::fs::write(&path, "x").unwrap();
        assert!(!super::preview::safe_to_open(&path), "{name}");
    }
    let image = dir.path().join("pic.png");
    std::fs::write(&image, "png").unwrap();
    assert!(super::preview::safe_to_open(&image));
}
