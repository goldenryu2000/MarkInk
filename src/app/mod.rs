//! The iced application: state, messages and update dispatch.

mod editing;
mod explorer;
mod palette;
mod preview;
mod session;
#[cfg(test)]
mod tests;
mod view;

use std::path::{Path, PathBuf};

use iced::keyboard::key::Named;
use iced::theme;
use iced::widget::{markdown, text_editor};
use iced::{Subscription, Task, Theme, keyboard, system, window};

use crate::cli::Launch;
use crate::document::DocId;
use crate::fsio::{self, DiskSnapshot, LoadedNote, ReadError, SaveError};
use crate::quick_open::QuickOpen;
use crate::shortcuts::{self, Command, EditorKey};
use crate::tabs::Tabs;
use crate::watcher::{self, FsChange};
use crate::workspace::{self, Entry, Tree};
use explorer::{FileOp, Prompt};
use palette::{Palette, SearchEvent, build_index};
use session::RestoredTabs;

const EDITOR_ID: &str = "editor";
const PREVIEW_ID: &str = "preview";
/// Editor line height in pixels (15px text, iced's default 1.3 line height).
const EDITOR_LINE_HEIGHT: f32 = 15.0 * 1.3;
/// Vertical space taken by the tab bar and editor padding.
const EDITOR_CHROME: f32 = 60.0;

fn window_settings() -> window::Settings {
    let icon =
        window::icon::from_rgba(include_bytes!("../../assets/icon-64.rgba").to_vec(), 64, 64);
    window::Settings {
        size: iced::Size::new(1200.0, 800.0),
        icon: icon.ok(),
        exit_on_close_request: false,
        #[cfg(target_os = "linux")]
        platform_specific: window::settings::PlatformSpecific {
            application_id: "markink".into(),
            ..Default::default()
        },
        ..window::Settings::default()
    }
}

pub fn run(launch: Launch) -> iced::Result {
    iced::application(
        move || State::boot(launch.clone()),
        State::update,
        State::view,
    )
    .title(State::title)
    .subscription(State::subscription)
    .theme(State::theme)
    .window(window_settings())
    .run()
}

pub struct State {
    root: PathBuf,
    tree: Tree,
    tabs: Tabs,
    sidebar_visible: bool,
    notice: Option<String>,
    quitting: bool,
    confirm_quit: bool,
    theme_mode: theme::Mode,
    quick_open: QuickOpen,
    palette: Option<Palette>,
    search_generation: u64,
    pending_cursor: Option<(PathBuf, usize)>,
    prompt: Option<Prompt>,
    session_path: Option<PathBuf>,
    launch_open: Option<PathBuf>,
    help_visible: bool,
    window_height: f32,
    session_save_in_flight: bool,
    session_save_pending: bool,
    sidebar_offset: f32,
}

#[derive(Debug, Clone)]
pub enum Message {
    DirListed(PathBuf, Result<Vec<Entry>, String>),
    EntryClicked(Entry),
    NoteLoaded(PathBuf, Result<LoadedNote, ReadError>),
    ActivateTab(DocId),
    CloseTab(DocId),
    Edit(DocId, text_editor::Action),
    EditorKey(DocId, EditorKey),
    Key(keyboard::Event),
    Shortcut(Command),
    DismissNotice,
    SaveDue(DocId, u64),
    Saved(DocId, u64, Result<DiskSnapshot, SaveError>),
    RetrySave(DocId),
    Window(window::Event),
    QuitAnyway,
    CancelQuit,
    PreviewDue(DocId, u64),
    LinkClicked(markdown::Uri),
    ThemeChanged(theme::Mode),
    FsChanges(Vec<FsChange>),
    DiskRead(DocId, Result<LoadedNote, ReadError>),
    KeepMine(DocId),
    LoadDiskVersion(DocId),
    DiskVersionLoaded(DocId, Result<LoadedNote, ReadError>),
    Recreate(DocId),
    Discard(DocId),
    IndexBuilt(Vec<PathBuf>),
    PaletteQuery(String),
    PaletteSubmit,
    PalettePick(usize),
    ClosePalette,
    SearchToggleRegex(bool),
    SearchDue(u64),
    Search(u64, SearchEvent),
    PromptInput(String),
    PromptSubmit,
    FileOpDone(Result<FileOp, String>),
    TrashSelected,
    SessionTabsLoaded(RestoredTabs),
    SessionSaved(Result<(), String>),
    SidebarScrolled(f32),
}

impl State {
    pub fn new(root: PathBuf) -> Self {
        Self {
            tree: Tree::new(root.clone()),
            quick_open: QuickOpen::new(root.clone()),
            root,
            tabs: Tabs::default(),
            sidebar_visible: true,
            notice: None,
            quitting: false,
            confirm_quit: false,
            theme_mode: theme::Mode::Dark,
            palette: None,
            search_generation: 0,
            pending_cursor: None,
            prompt: None,
            session_path: None,
            launch_open: None,
            help_visible: false,
            window_height: 800.0,
            session_save_in_flight: false,
            session_save_pending: false,
            sidebar_offset: 0.0,
        }
    }

    fn boot(launch: Launch) -> (Self, Task<Message>) {
        let mut state = Self::new(launch.root.clone());
        let session_path = crate::session::session_file(&crate::paths::state_dir(), &launch.root);
        let session = crate::session::load(&session_path);
        state.session_path = Some(session_path);
        let mut tasks = vec![
            list_dir(launch.root.clone()),
            build_index(launch.root),
            system::theme().map(Message::ThemeChanged),
        ];
        match session {
            Some(session) if !session.tabs.is_empty() => {
                state.launch_open = launch.open;
                tasks.push(state.restore_session(session));
            }
            Some(session) => {
                tasks.push(state.restore_session(session));
                tasks.extend(launch.open.map(load_note));
            }
            None => tasks.extend(launch.open.map(load_note)),
        }
        (state, Task::batch(tasks))
    }

    fn title(&self) -> String {
        let folder = self.root.file_name().map_or_else(
            || self.root.display().to_string(),
            |n| n.to_string_lossy().into_owned(),
        );
        match self.tabs.active() {
            Some(doc) => format!("{} - {folder} - MarkInk", doc.title()),
            None => format!("{folder} - MarkInk"),
        }
    }

    pub fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::DirListed(dir, result) => self.dir_listed(dir, result),
            Message::EntryClicked(entry) => self.entry_clicked(entry),
            Message::NoteLoaded(path, result) => self.note_loaded(path, result),
            Message::ActivateTab(id) => self.activate(id),
            Message::CloseTab(id) => self.close_tab(id),
            Message::Edit(id, action) => self.edit(id, action),
            Message::EditorKey(id, key) => self.editor_key(id, key),
            Message::Key(event) => self.on_key(event),
            Message::Shortcut(command) => self.command(command),
            Message::DismissNotice => {
                self.notice = None;
                Task::none()
            }
            Message::SaveDue(id, revision) => self.save_due(id, revision),
            Message::Saved(id, revision, result) => self.saved(id, revision, result),
            Message::RetrySave(id) => self.flush(id),
            Message::Window(event) => self.window_event(event),
            Message::QuitAnyway => self.exit(),
            Message::CancelQuit => {
                self.confirm_quit = false;
                Task::none()
            }
            Message::PreviewDue(id, revision) => self.preview_due(id, revision),
            Message::LinkClicked(uri) => self.link_clicked(uri),
            Message::ThemeChanged(mode) => {
                self.theme_mode = mode;
                Task::none()
            }
            Message::FsChanges(changes) => self.fs_changes(changes),
            Message::DiskRead(id, result) => self.disk_read(id, result),
            Message::KeepMine(id) | Message::Recreate(id) => self.overwrite(id),
            Message::LoadDiskVersion(id) => match self.tabs.get(id) {
                Some(doc) => read_disk(id, doc.path().to_path_buf(), Message::DiskVersionLoaded),
                None => Task::none(),
            },
            Message::DiskVersionLoaded(id, result) => self.disk_version_loaded(id, result),
            Message::Discard(id) => {
                self.tabs.close(id);
                Task::none()
            }
            Message::IndexBuilt(notes) => self.index_built(notes),
            Message::PaletteQuery(query) => self.palette_query(query),
            Message::PaletteSubmit => self.palette_submit(),
            Message::PalettePick(index) => self.palette_pick(index),
            Message::ClosePalette => self.close_palette(),
            Message::SearchToggleRegex(regex) => self.search_toggle_regex(regex),
            Message::SearchDue(generation) => self.search_due(generation),
            Message::Search(generation, event) => self.search_event(generation, event),
            Message::PromptInput(value) => self.prompt_input(value),
            Message::PromptSubmit => self.prompt_submit(),
            Message::FileOpDone(result) => self.file_op_done(result),
            Message::TrashSelected => self.trash_selected(),
            Message::SessionTabsLoaded(restored) => self.session_tabs_loaded(restored),
            Message::SessionSaved(result) => self.session_saved(result),
            Message::SidebarScrolled(offset) => {
                self.sidebar_offset = offset;
                Task::none()
            }
        }
    }

    fn subscription(&self) -> Subscription<Message> {
        Subscription::batch([
            keyboard::listen().map(Message::Key),
            iced::event::listen_with(escape_filter),
            window::events().map(|(_, event)| Message::Window(event)),
            system::theme_changes().map(Message::ThemeChanged),
            watcher::subscription(self.root.clone()).map(Message::FsChanges),
        ])
    }

    fn on_key(&mut self, event: keyboard::Event) -> Task<Message> {
        let keyboard::Event::KeyPressed {
            key,
            physical_key,
            modifiers,
            ..
        } = event
        else {
            return Task::none();
        };
        if self.palette.is_some()
            && let keyboard::Key::Named(named @ (Named::ArrowUp | Named::ArrowDown)) = &key
        {
            self.palette_move(if *named == Named::ArrowUp { -1 } else { 1 });
            return Task::none();
        }
        match shortcuts::command_for(&key, key.to_latin(physical_key), modifiers) {
            // Escape comes through `escape_filter`, which also sees captured presses.
            Some(Command::Escape) | None => Task::none(),
            Some(command) => self.command(command),
        }
    }

    fn command(&mut self, command: Command) -> Task<Message> {
        match command {
            Command::Undo => self.undo(true),
            Command::Redo => self.undo(false),
            Command::CloseTab => match self.tabs.active_id() {
                Some(id) => self.close_tab(id),
                None => Task::none(),
            },
            Command::NextTab => self.cycle_tab(true),
            Command::PrevTab => self.cycle_tab(false),
            Command::ToggleSidebar => {
                self.sidebar_visible = !self.sidebar_visible;
                Task::none()
            }
            Command::Escape => {
                self.notice = None;
                if self.palette.is_some() {
                    self.close_palette()
                } else if self.help_visible {
                    self.help_visible = false;
                    Task::none()
                } else {
                    self.prompt = None;
                    Task::none()
                }
            }
            Command::Help => {
                self.help_visible = !self.help_visible;
                Task::none()
            }
            Command::TogglePreview => self.toggle_preview(),
            Command::QuickOpen => self.open_quick_open(),
            Command::Search => self.open_search(),
            Command::NewNote => self.new_note(),
            Command::NewFolder => self.new_folder(),
            Command::Rename => self.rename_selected(),
        }
    }

    fn theme(&self) -> Theme {
        match self.theme_mode {
            theme::Mode::Light => Theme::Light,
            _ => Theme::Dark,
        }
    }

    /// Editor lines that fit on screen.
    fn visible_lines(&self) -> f32 {
        ((self.window_height - EDITOR_CHROME) / EDITOR_LINE_HEIGHT).max(1.0)
    }

    /// `path` relative to the workspace root, for display.
    fn relative<'a>(&self, path: &'a Path) -> &'a Path {
        path.strip_prefix(&self.root).unwrap_or(path)
    }
}

/// Sends `message` after `delay`. The timer is created lazily, inside the runtime.
fn delayed(delay: std::time::Duration, message: Message) -> Task<Message> {
    Task::perform(async move { tokio::time::sleep(delay).await }, move |()| {
        message
    })
}

/// Runs blocking work off the UI thread.
async fn blocking<T: Send + 'static>(work: impl FnOnce() -> T + Send + 'static) -> T {
    tokio::task::spawn_blocking(work)
        .await
        .expect("background task panicked")
}

fn list_dir(dir: PathBuf) -> Task<Message> {
    let target = dir.clone();
    Task::perform(
        blocking(move || workspace::list_dir(&target).map_err(|e| e.to_string())),
        move |result| Message::DirListed(dir, result),
    )
}

fn load_note(path: PathBuf) -> Task<Message> {
    let target = path.clone();
    Task::perform(blocking(move || fsio::read_note(&target)), move |result| {
        Message::NoteLoaded(path, result)
    })
}

fn read_disk(
    id: DocId,
    path: PathBuf,
    wrap: fn(DocId, Result<LoadedNote, ReadError>) -> Message,
) -> Task<Message> {
    Task::perform(blocking(move || fsio::read_note(&path)), move |result| {
        wrap(id, result)
    })
}

/// Escape arrives here even when a text input captured it.
fn escape_filter(
    event: iced::Event,
    _status: iced::event::Status,
    _window: window::Id,
) -> Option<Message> {
    match event {
        iced::Event::Keyboard(keyboard::Event::KeyPressed {
            key: keyboard::Key::Named(Named::Escape),
            ..
        }) => Some(Message::Shortcut(Command::Escape)),
        _ => None,
    }
}
