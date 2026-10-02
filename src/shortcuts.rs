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
}

impl Command {
    /// Whether the focused editor should hand this key to the app.
    pub fn from_editor(self) -> bool {
        !matches!(self, Command::Escape | Command::Rename)
    }
}

/// Maps a key press to a command. `latin` is the layout-independent letter.
pub fn command_for(key: &Key, latin: Option<char>, modifiers: Modifiers) -> Option<Command> {
    if let Key::Named(named) = key {
        return match (named, modifiers.command(), modifiers.shift()) {
            (Named::Tab, true, false) => Some(Command::NextTab),
            (Named::Tab, true, true) => Some(Command::PrevTab),
            (Named::F2, false, _) => Some(Command::Rename),
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
    fn editor_keeps_escape_and_f2() {
        assert!(!Command::Escape.from_editor());
        assert!(!Command::Rename.from_editor());
        assert!(Command::Undo.from_editor());
    }
}
