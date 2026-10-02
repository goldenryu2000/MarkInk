//! App-wide keyboard shortcuts.

use iced::keyboard::key::Named;
use iced::keyboard::{Key, Modifiers};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Command {
    QuickOpen,
    Search,
    TogglePreview,
    ToggleSidebar,
    NewNote,
    NewFolder,
    CloseTab,
    NextTab,
    PrevTab,
    Undo,
    Redo,
    Rename,
    Escape,
    Help,
}

/// Shortcut reference shown in the app and kept in sync with the README.
pub const HELP: &[(&str, &str)] = &[
    ("Quick open", "Ctrl+P"),
    ("Search notes", "Ctrl+Shift+F"),
    ("Toggle preview", "Ctrl+E"),
    ("Toggle sidebar", "Ctrl+B"),
    ("New note", "Ctrl+N"),
    ("New folder", "Ctrl+Shift+N"),
    ("Rename", "F2"),
    ("Close tab", "Ctrl+W"),
    ("Next / previous tab", "Ctrl+Tab / Ctrl+Shift+Tab"),
    ("Undo", "Ctrl+Z"),
    ("Redo", "Ctrl+Shift+Z / Ctrl+Y"),
    ("Delete word", "Ctrl+Backspace / Ctrl+Delete"),
    ("Jump by word", "Ctrl+Left / Ctrl+Right"),
    ("Toggle checkbox", "Ctrl+L"),
    ("Indent / outdent", "Tab / Shift+Tab"),
    ("Newline without list marker", "Shift+Enter"),
    ("Shortcuts", "F1"),
];

/// Display hint for a command, e.g. `"New note (Ctrl+N)"`.
pub fn hint(action: &str) -> String {
    match HELP.iter().find(|(a, _)| *a == action) {
        Some((_, keys)) => format!("{action} ({keys})"),
        None => action.to_owned(),
    }
}

impl Command {
    /// Whether the focused editor should hand this key to the app.
    pub fn from_editor(self) -> bool {
        !matches!(self, Command::Escape | Command::Rename)
    }
}

/// Editing keys the editor widget does not provide itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditorKey {
    DeleteWordBack,
    DeleteWordForward,
    ToggleTask,
    Indent,
    Unindent,
    /// Enter with list continuation.
    Enter,
}

/// Maps a key press in the focused editor to an editing action.
pub fn editor_key(key: &Key, latin: Option<char>, modifiers: Modifiers) -> Option<EditorKey> {
    if modifiers.alt() {
        return None;
    }
    let ctrl = modifiers.command();
    match key {
        Key::Named(Named::Backspace) if ctrl => Some(EditorKey::DeleteWordBack),
        Key::Named(Named::Delete) if ctrl => Some(EditorKey::DeleteWordForward),
        Key::Named(Named::Tab) if !ctrl => Some(if modifiers.shift() {
            EditorKey::Unindent
        } else {
            EditorKey::Indent
        }),
        Key::Named(Named::Enter) if !ctrl && !modifiers.shift() => Some(EditorKey::Enter),
        _ if ctrl && !modifiers.shift() && latin == Some('l') => Some(EditorKey::ToggleTask),
        _ => None,
    }
}

/// Maps a key press to a command. `latin` is the layout-independent letter.
pub fn command_for(key: &Key, latin: Option<char>, modifiers: Modifiers) -> Option<Command> {
    if let Key::Named(named) = key {
        return match (named, modifiers.command(), modifiers.shift()) {
            (Named::Tab, true, false) => Some(Command::NextTab),
            (Named::Tab, true, true) => Some(Command::PrevTab),
            (Named::F2, false, _) => Some(Command::Rename),
            (Named::F1, false, _) => Some(Command::Help),
            (Named::Escape, false, _) => Some(Command::Escape),
            _ => None,
        };
    }
    if !modifiers.command() || modifiers.alt() {
        return None;
    }
    match (latin?.to_ascii_lowercase(), modifiers.shift()) {
        ('p', false) => Some(Command::QuickOpen),
        ('f', true) => Some(Command::Search),
        ('e', false) => Some(Command::TogglePreview),
        ('b', false) => Some(Command::ToggleSidebar),
        ('n', false) => Some(Command::NewNote),
        ('n', true) => Some(Command::NewFolder),
        ('w', false) => Some(Command::CloseTab),
        ('z', false) => Some(Command::Undo),
        ('z', true) | ('y', false) => Some(Command::Redo),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn letter(c: &str) -> Key {
        Key::Character(c.into())
    }

    const CTRL_SHIFT: Modifiers = Modifiers::CTRL.union(Modifiers::SHIFT);

    #[test]
    fn maps_ctrl_letters() {
        let cases = [
            ('p', Command::QuickOpen),
            ('e', Command::TogglePreview),
            ('b', Command::ToggleSidebar),
            ('n', Command::NewNote),
            ('w', Command::CloseTab),
            ('z', Command::Undo),
            ('y', Command::Redo),
        ];
        for (c, command) in cases {
            assert_eq!(
                command_for(&letter(&c.to_string()), Some(c), Modifiers::CTRL),
                Some(command)
            );
        }
    }

    #[test]
    fn maps_ctrl_shift_letters() {
        assert_eq!(
            command_for(&letter("F"), Some('f'), CTRL_SHIFT),
            Some(Command::Search)
        );
        assert_eq!(
            command_for(&letter("Z"), Some('z'), CTRL_SHIFT),
            Some(Command::Redo)
        );
        assert_eq!(
            command_for(&letter("N"), Some('n'), CTRL_SHIFT),
            Some(Command::NewFolder)
        );
    }

    #[test]
    fn uses_latin_letter_on_other_layouts() {
        assert_eq!(
            command_for(&letter("з"), Some('p'), Modifiers::CTRL),
            Some(Command::QuickOpen)
        );
    }

    #[test]
    fn maps_named_keys() {
        let tab = Key::Named(Named::Tab);
        assert_eq!(
            command_for(&tab, None, Modifiers::CTRL),
            Some(Command::NextTab)
        );
        assert_eq!(command_for(&tab, None, CTRL_SHIFT), Some(Command::PrevTab));
        assert_eq!(
            command_for(&Key::Named(Named::F2), None, Modifiers::empty()),
            Some(Command::Rename)
        );
        assert_eq!(
            command_for(&Key::Named(Named::Escape), None, Modifiers::empty()),
            Some(Command::Escape)
        );
    }

    #[test]
    fn ignores_plain_and_alt_keys() {
        assert_eq!(
            command_for(&letter("p"), Some('p'), Modifiers::empty()),
            None
        );
        assert_eq!(
            command_for(&letter("p"), Some('p'), Modifiers::CTRL | Modifiers::ALT),
            None
        );
        assert_eq!(
            command_for(&Key::Named(Named::Tab), None, Modifiers::empty()),
            None
        );
    }

    #[test]
    fn maps_editor_keys() {
        let named = |n| Key::Named(n);
        let ctrl = Modifiers::CTRL;
        assert_eq!(
            editor_key(&named(Named::Backspace), None, ctrl),
            Some(EditorKey::DeleteWordBack)
        );
        assert_eq!(
            editor_key(&named(Named::Delete), None, ctrl),
            Some(EditorKey::DeleteWordForward)
        );
        assert_eq!(
            editor_key(&letter("l"), Some('l'), ctrl),
            Some(EditorKey::ToggleTask)
        );
        assert_eq!(
            editor_key(&named(Named::Tab), None, Modifiers::empty()),
            Some(EditorKey::Indent)
        );
        assert_eq!(
            editor_key(&named(Named::Tab), None, Modifiers::SHIFT),
            Some(EditorKey::Unindent)
        );
        assert_eq!(
            editor_key(&named(Named::Enter), None, Modifiers::empty()),
            Some(EditorKey::Enter)
        );
    }

    #[test]
    fn leaves_other_editor_keys_alone() {
        let named = |n| Key::Named(n);
        assert_eq!(
            editor_key(&named(Named::Backspace), None, Modifiers::empty()),
            None
        );
        assert_eq!(
            editor_key(
                &named(Named::Backspace),
                None,
                Modifiers::CTRL | Modifiers::ALT
            ),
            None
        );
        assert_eq!(
            editor_key(&named(Named::Enter), None, Modifiers::SHIFT),
            None
        );
        assert_eq!(editor_key(&named(Named::Tab), None, Modifiers::CTRL), None);
        assert_eq!(
            editor_key(&letter("l"), Some('l'), Modifiers::empty()),
            None
        );
    }

    #[test]
    fn f1_opens_help() {
        let f1 = Key::Named(Named::F1);
        assert_eq!(
            command_for(&f1, None, Modifiers::empty()),
            Some(Command::Help)
        );
    }

    #[test]
    fn readme_lists_every_shortcut() {
        let readme = include_str!("../README.md");
        assert!(HELP.len() >= 15);
        for (action, keys) in HELP {
            let row = format!("| {action} | {keys} |");
            assert!(readme.contains(&row), "README is missing: {row}");
        }
    }

    #[test]
    fn editor_keeps_escape_and_f2() {
        assert!(!Command::Escape.from_editor());
        assert!(!Command::Rename.from_editor());
        assert!(Command::Undo.from_editor());
    }
}
