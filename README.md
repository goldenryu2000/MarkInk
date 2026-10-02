# MarkInk

A fast, reliable Markdown note-taking editor for Linux, written in Rust.

Status: early development.

## Features

- Open a folder of notes and browse it in a sidebar
- Tabs and a toggleable split preview
- Autosave with atomic writes; outside edits reload or raise a conflict banner
- Quick open and full-text search
- Session restore per folder

## Shortcuts

| Action | Keys |
|---|---|
| Quick open | Ctrl+P |
| Search notes | Ctrl+Shift+F |
| Toggle preview | Ctrl+E |
| Toggle sidebar | Ctrl+B |
| New note / folder | Ctrl+N / Ctrl+Shift+N |
| Rename | F2 |
| Close tab | Ctrl+W |
| Next / previous tab | Ctrl+Tab / Ctrl+Shift+Tab |
| Undo / redo | Ctrl+Z / Ctrl+Shift+Z |

Logs: `~/.local/state/markink/markink.log` (`MARKINK_LOG=debug` for more).

## Build

    cargo build --release
    ./target/release/markink ~/notes

## License

MIT or Apache-2.0, at your option.
