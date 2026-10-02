//! Layout and widgets.

use std::ops::Range;
use std::path::Path;

use iced::highlighter;
use iced::widget::text_editor::{self, Binding, KeyPress};
use iced::widget::{
    Column, Space, button, checkbox, column, container, markdown, mouse_area, opaque, row, rule,
    scrollable, space, stack, text, text_input, tooltip,
};
use iced::{Center, Color, Element, Fill, Font, Padding, Theme};

use super::explorer::{PROMPT_INPUT_ID, PromptKind};
use super::palette::SearchPalette;
use super::palette::{PALETTE_INPUT_ID, Palette};
use super::{EDITOR_ID, Message, PREVIEW_ID, State};
use crate::document::{DocId, DocStatus, Document};
use crate::search::MAX_HITS;
use crate::shortcuts::{self, Command};
use crate::workspace::{EntryKind, Row};

const SIDEBAR_WIDTH: f32 = 240.0;
const UI_TEXT: f32 = 13.0;
const EDITOR_TEXT: f32 = 15.0;

impl State {
    pub(super) fn view(&self) -> Element<'_, Message> {
        let overlay = if self.confirm_quit {
            Some(quit_dialog())
        } else if self.help_visible {
            Some(help_dialog())
        } else {
            self.palette.as_ref().map(|p| palette_view(p, &self.root))
        };
        match overlay {
            Some(overlay) => stack![self.base(), overlay].into(),
            None => self.base(),
        }
    }

    fn base(&self) -> Element<'_, Message> {
        let main = column![self.tab_bar(), self.editor_area()]
            .width(Fill)
            .height(Fill);
        let body: Element<'_, Message> = if self.sidebar_visible {
            row![self.sidebar(), rule::vertical(1), main]
                .height(Fill)
                .into()
        } else {
            main.into()
        };
        match &self.notice {
            Some(notice) => column![body, notice_bar(notice)].into(),
            None => body,
        }
    }

    fn sidebar(&self) -> Element<'_, Message> {
        let action = |label, hint: &str, message| {
            let button = button(text(label).size(11))
                .on_press(message)
                .padding([2, 6])
                .style(button::text);
            with_hint(button, shortcuts::hint(hint))
        };
        let header = row![
            action("+ Note", "New note", Message::Shortcut(Command::NewNote)),
            action(
                "+ Folder",
                "New folder",
                Message::Shortcut(Command::NewFolder)
            ),
            action("Rename", "Rename", Message::Shortcut(Command::Rename)),
            action("Trash", "Move to trash", Message::TrashSelected),
            space::horizontal(),
            action("?", "Shortcuts", Message::Shortcut(Command::Help)),
        ]
        .spacing(2);
        let selected = self.tree.selected.as_deref();
        let rows = self.tree.rows();
        let total = rows.len();
        let range = visible_range(
            total,
            self.sidebar_offset,
            self.window_height,
            TREE_ROW_HEIGHT,
        );
        let above = range.start as f32 * TREE_ROW_HEIGHT;
        let below = (total - range.end) as f32 * TREE_ROW_HEIGHT;
        let visible = rows
            .into_iter()
            .skip(range.start)
            .take(range.len())
            .map(|row| tree_row(row, selected));
        let list = column![Space::new().height(above)]
            .extend(visible)
            .push(Space::new().height(below));
        let mut content = column![header].spacing(4).padding(4);
        if let Some(prompt) = &self.prompt {
            let placeholder = match prompt.kind {
                PromptKind::NewNote(_) => "New note name",
                PromptKind::NewFolder(_) => "New folder name",
                PromptKind::Rename(_) => "New name",
            };
            content = content.push(
                text_input(placeholder, &prompt.value)
                    .id(PROMPT_INPUT_ID)
                    .on_input(Message::PromptInput)
                    .on_submit(Message::PromptSubmit)
                    .size(UI_TEXT)
                    .padding(4),
            );
        }
        let list = scrollable(list)
            .on_scroll(|viewport| Message::SidebarScrolled(viewport.absolute_offset().y))
            .height(Fill);
        content = content.push(list);
        container(content).width(SIDEBAR_WIDTH).height(Fill).into()
    }

    fn tab_bar(&self) -> Element<'_, Message> {
        let active = self.tabs.active_id();
        let tabs = self
            .tabs
            .iter()
            .map(|doc| tab(doc, Some(doc.id()) == active));
        scrollable(row(tabs).spacing(2).padding(2))
            .direction(scrollable::Direction::Horizontal(
                scrollable::Scrollbar::new().width(2).scroller_width(2),
            ))
            .into()
    }

    fn editor_area(&self) -> Element<'_, Message> {
        match self.tabs.active() {
            Some(doc) => self.document_view(doc),
            None => {
                let intro = text("Open a note from the sidebar, or:").size(UI_TEXT);
                container(column![intro, shortcut_table()].spacing(12))
                    .center(Fill)
                    .into()
            }
        }
    }
}

impl State {
    fn document_view<'a>(&'a self, doc: &'a Document) -> Element<'a, Message> {
        let body = self.panes(doc);
        match banner(doc) {
            Some(banner) => column![banner, body].into(),
            None => body,
        }
    }

    fn panes<'a>(&'a self, doc: &'a Document) -> Element<'a, Message> {
        let theme = self.theme();
        let highlight = if theme == Theme::Light {
            highlighter::Theme::InspiredGitHub
        } else {
            highlighter::Theme::Base16Ocean
        };
        let editor = container(editor(doc, highlight)).width(Fill);
        if !doc.preview_visible {
            return editor.into();
        }
        let preview = markdown::view(doc.preview_items(), markdown::Settings::from(&theme))
            .map(Message::LinkClicked);
        let preview = scrollable(container(preview).padding(16).width(Fill))
            .id(PREVIEW_ID)
            .width(Fill)
            .height(Fill);
        row![editor, rule::vertical(1), preview].height(Fill).into()
    }
}

fn editor(doc: &Document, highlight: highlighter::Theme) -> Element<'_, Message> {
    let id = doc.id();
    iced::widget::text_editor(doc.content())
        .id(EDITOR_ID)
        .on_action(move |action| Message::Edit(id, action))
        .key_binding(editor_binding(id))
        .font(Font::MONOSPACE)
        .size(EDITOR_TEXT)
        .padding(12)
        .height(Fill)
        .highlight("md", highlight)
        .into()
}

/// Routes app shortcuts and extra editing keys out of the focused editor.
fn editor_binding(id: DocId) -> impl Fn(KeyPress) -> Option<Binding<Message>> {
    move |press| {
        if !matches!(press.status, text_editor::Status::Focused { .. }) {
            return Binding::from_key_press(press);
        }
        let latin = press.key.to_latin(press.physical_key);
        if let Some(key) = shortcuts::editor_key(&press.key, latin, press.modifiers) {
            return Some(Binding::Custom(Message::EditorKey(id, key)));
        }
        match shortcuts::command_for(&press.key, latin, press.modifiers) {
            Some(command) if command.from_editor() => {
                Some(Binding::Custom(Message::Shortcut(command)))
            }
            _ => Binding::from_key_press(press),
        }
    }
}

fn tab(doc: &Document, active: bool) -> Element<'_, Message> {
    let label = format!("{}{}", doc.title(), status_suffix(doc));
    let close = with_hint(
        button(text("×").size(UI_TEXT))
            .on_press(Message::CloseTab(doc.id()))
            .padding([0, 4])
            .style(button::text),
        shortcuts::hint("Close tab"),
    );
    button(
        row![text(label).size(UI_TEXT), close]
            .spacing(6)
            .align_y(Center),
    )
    .on_press(Message::ActivateTab(doc.id()))
    .padding([4, 10])
    .style(if active {
        button::secondary
    } else {
        button::text
    })
    .into()
}

fn status_suffix(doc: &Document) -> &'static str {
    match &doc.status {
        _ if doc.is_read_only() => " (read-only)",
        DocStatus::Conflict => " (conflict)",
        DocStatus::DeletedOnDisk => " (deleted)",
        DocStatus::SaveFailed { .. } => " (not saved)",
        DocStatus::Normal => "",
    }
}

/// Sidebar rows have a fixed height so only the visible ones need drawing.
const TREE_ROW_HEIGHT: f32 = 22.0;
/// Extra rows drawn above and below the viewport.
const OVERSCAN: usize = 5;

/// Rows to draw for a list of `total` fixed-height rows scrolled to `offset`.
pub(super) fn visible_range(total: usize, offset: f32, viewport: f32, row: f32) -> Range<usize> {
    let visible = (viewport / row).ceil() as usize;
    let first = (offset.max(0.0) / row).floor() as usize;
    let end = first.saturating_add(visible + OVERSCAN).min(total);
    let start = first
        .saturating_sub(OVERSCAN)
        .min(end.saturating_sub(visible + 2 * OVERSCAN));
    start..end
}

fn tree_row<'a>(row: Row<'a>, selected: Option<&Path>) -> Element<'a, Message> {
    let marker = match (row.entry.kind, row.expanded) {
        (EntryKind::Dir, true) => "▾ ",
        (EntryKind::Dir, false) => "▸ ",
        (EntryKind::Note, _) => "   ",
    };
    let indent = 6.0 + row.depth as f32 * 14.0;
    let is_selected = selected == Some(row.entry.path.as_path());
    let label = text(format!("{marker}{}", row.entry.name))
        .size(UI_TEXT)
        .wrapping(text::Wrapping::None);
    button(label)
        .on_press(Message::EntryClicked(row.entry.clone()))
        .width(Fill)
        .height(TREE_ROW_HEIGHT)
        .padding(Padding {
            top: 2.0,
            bottom: 2.0,
            left: indent,
            right: 6.0,
        })
        .style(if is_selected {
            button::secondary
        } else {
            button::text
        })
        .into()
}

fn notice_bar(notice: &str) -> Element<'_, Message> {
    let dismiss = button(text("Dismiss").size(UI_TEXT))
        .on_press(Message::DismissNotice)
        .style(button::text);
    container(row![text(notice).size(UI_TEXT).width(Fill), dismiss].align_y(Center))
        .padding([4, 10])
        .width(Fill)
        .style(container::bordered_box)
        .into()
}

fn quit_dialog<'a>() -> Element<'a, Message> {
    let buttons = row![
        space::horizontal(),
        button(text("Keep editing"))
            .on_press(Message::CancelQuit)
            .style(button::secondary),
        button(text("Quit without saving"))
            .on_press(Message::QuitAnyway)
            .style(button::danger),
    ]
    .spacing(8);
    let dialog = column![
        text("Some notes have unsaved changes.").size(EDITOR_TEXT),
        text("They could not be saved. Check the banners on their tabs.").size(UI_TEXT),
        buttons,
    ]
    .spacing(12);
    modal(
        container(dialog)
            .padding(20)
            .max_width(460)
            .style(container::bordered_box),
    )
}

/// Centers `content` over a dimmed backdrop that blocks clicks underneath.
fn modal<'a>(content: impl Into<Element<'a, Message>>) -> Element<'a, Message> {
    let backdrop = container(content)
        .center(Fill)
        .style(|_: &Theme| container::Style {
            background: Some(
                Color {
                    a: 0.5,
                    ..Color::BLACK
                }
                .into(),
            ),
            ..container::Style::default()
        });
    opaque(backdrop)
}

fn banner(doc: &Document) -> Option<Element<'_, Message>> {
    let id = doc.id();
    let (message, actions) = match &doc.status {
        DocStatus::Normal => return None,
        DocStatus::Conflict => (
            "This note changed on disk while you were editing.".to_owned(),
            vec![
                ("Keep mine", Message::KeepMine(id)),
                ("Load disk version", Message::LoadDiskVersion(id)),
            ],
        ),
        DocStatus::DeletedOnDisk => (
            "This note was deleted on disk.".to_owned(),
            vec![
                ("Recreate", Message::Recreate(id)),
                ("Close", Message::Discard(id)),
            ],
        ),
        DocStatus::SaveFailed { error, .. } => {
            (format!("Could not save: {error}. Retrying."), vec![])
        }
    };
    let buttons = actions.into_iter().map(|(label, message)| {
        button(text(label).size(UI_TEXT))
            .on_press(message)
            .style(button::secondary)
            .into()
    });
    let content = row![text(message).size(UI_TEXT).width(Fill)]
        .extend(buttons)
        .spacing(8)
        .align_y(Center);
    Some(
        container(content)
            .padding([6, 12])
            .width(Fill)
            .style(container::bordered_box)
            .into(),
    )
}

const SEARCH_ROWS: usize = 200;

fn palette_view<'a>(palette: &'a Palette, root: &'a Path) -> Element<'a, Message> {
    match palette {
        Palette::QuickOpen {
            query,
            results,
            selected,
        } => {
            let input = palette_input("Open note", query);
            let rows = results
                .iter()
                .enumerate()
                .map(|(i, path)| result_row(text(path).size(UI_TEXT), i, *selected));
            palette_frame(column![input, scrollable(column(rows))])
        }
        Palette::Search(search) => {
            let regex = checkbox(search.regex)
                .label("Regex")
                .on_toggle(Message::SearchToggleRegex)
                .size(14);
            let header = row![palette_input("Search notes", &search.query), regex]
                .spacing(8)
                .align_y(Center);
            let shown = search
                .hits
                .iter()
                .take(SEARCH_ROWS)
                .enumerate()
                .map(|(i, hit)| {
                    let location = format!("{}:{}", relative_to(&hit.path, root), hit.line);
                    let label = column![text(location).size(11), text(&hit.text).size(UI_TEXT)];
                    result_row(label, i, search.selected)
                });
            palette_frame(column![
                header,
                text(search_status(search)).size(11),
                scrollable(column(shown))
            ])
        }
    }
}

fn relative_to<'a>(path: &'a Path, root: &Path) -> std::borrow::Cow<'a, str> {
    path.strip_prefix(root).unwrap_or(path).to_string_lossy()
}

fn search_status(search: &SearchPalette) -> String {
    if let Some(error) = &search.error {
        return format!("Invalid pattern: {error}");
    }
    let count = search.hits.len();
    match search.summary {
        None if search.query.trim().is_empty() => String::new(),
        None => format!("Searching... {count} found"),
        Some(summary) if summary.truncated => format!("First {MAX_HITS} matches shown"),
        Some(_) if count > SEARCH_ROWS => format!("{count} matches, first {SEARCH_ROWS} shown"),
        Some(_) => format!("{count} matches"),
    }
}

fn palette_input<'a>(placeholder: &'a str, value: &'a str) -> Element<'a, Message> {
    text_input(placeholder, value)
        .id(PALETTE_INPUT_ID)
        .on_input(Message::PaletteQuery)
        .on_submit(Message::PaletteSubmit)
        .padding(8)
        .size(14)
        .into()
}

fn result_row<'a>(
    label: impl Into<Element<'a, Message>>,
    index: usize,
    selected: usize,
) -> Element<'a, Message> {
    button(label)
        .on_press(Message::PalettePick(index))
        .width(Fill)
        .padding([4, 8])
        .style(if index == selected {
            button::secondary
        } else {
            button::text
        })
        .into()
}

/// A panel near the top; clicking outside closes it.
fn palette_frame(content: Column<'_, Message>) -> Element<'_, Message> {
    let panel = container(content.spacing(6))
        .padding(10)
        .width(640)
        .max_height(480)
        .style(container::bordered_box);
    let placed = container(opaque(panel))
        .width(Fill)
        .height(Fill)
        .align_x(Center)
        .padding(Padding::ZERO.top(60));
    mouse_area(placed).on_press(Message::ClosePalette).into()
}

/// Wraps `content` with a hover tooltip.
fn with_hint<'a>(content: impl Into<Element<'a, Message>>, hint: String) -> Element<'a, Message> {
    let label = container(text(hint).size(11))
        .padding([3, 6])
        .style(container::bordered_box);
    tooltip(content, label, tooltip::Position::Bottom)
        .gap(4)
        .into()
}

/// Two-column list of every shortcut.
fn shortcut_table<'a>() -> Element<'a, Message> {
    let rows = shortcuts::help().into_iter().map(|(action, keys)| {
        row![
            text(action).size(UI_TEXT).width(220),
            text(keys).size(UI_TEXT).font(Font::MONOSPACE),
        ]
        .into()
    });
    column(rows).spacing(4).into()
}

fn help_dialog<'a>() -> Element<'a, Message> {
    let close = button(text("Close"))
        .on_press(Message::Shortcut(Command::Help))
        .style(button::secondary);
    let dialog = column![
        text("Shortcuts").size(EDITOR_TEXT),
        shortcut_table(),
        text("Enter continues lists and quotes; Enter on an empty item ends the list.").size(11),
        row![space::horizontal(), close],
    ]
    .spacing(12);
    modal(
        container(dialog)
            .padding(20)
            .max_width(520)
            .style(container::bordered_box),
    )
}
