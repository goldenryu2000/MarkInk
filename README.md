# MarkInk

A fast, reliable Markdown note-taking editor for Linux, written in Rust.

Status: early development.

## Features

- Open a folder of notes and browse it in a sidebar
- Tabs and a toggleable split preview
- List continuation and common editing shortcuts
- Autosave with atomic writes; outside edits reload or raise a conflict banner
- Quick open and full-text search
- Session restore per folder

## Shortcuts

Press F1 in the app to see these.

| Action | Keys |
|---|---|
| Quick open | Ctrl+P |
| Search notes | Ctrl+Shift+F |
| Toggle preview | Ctrl+E |
| Toggle sidebar | Ctrl+B |
| New note | Ctrl+N |
| New folder | Ctrl+Shift+N |
| Rename | F2 |
| Close tab | Ctrl+W |
| Next / previous tab | Ctrl+Tab / Ctrl+Shift+Tab |
| Undo | Ctrl+Z |
| Redo | Ctrl+Shift+Z / Ctrl+Y |
| Delete word | Ctrl+Backspace / Ctrl+Delete |
| Jump by word | Ctrl+Left / Ctrl+Right |
| Toggle checkbox | Ctrl+L |
| Indent / outdent | Tab / Shift+Tab |
| Newline without list marker | Shift+Enter |
| Shortcuts | F1 |

Enter continues bullet, numbered, task and quote lines. Enter on an empty item ends the list.

Logs: `~/.local/state/markink/markink.log` (`MARKINK_LOG=debug` for more).

## Install

Download from the [latest release](https://github.com/goldenryu2000/MarkInk/releases/latest).

| Platform | File |
|---|---|
| Debian, Ubuntu | `markink_<version>_amd64.deb` (or `arm64`), then `sudo apt install ./markink_*.deb` |
| Other Linux | `markink-<version>-linux-x86_64.tar.gz` (or `aarch64`): extract and put `markink` on your PATH |
| macOS | `MarkInk-<version>-macos-apple-silicon.dmg` or `-intel.dmg` |
| Windows | `markink_<version>_x64-setup.exe`, or `MarkInk-<version>-windows-x86_64-portable.exe` |

macOS and Windows builds are not signed yet:

- macOS: open MarkInk once, then go to System Settings > Privacy & Security and click Open Anyway.
- Windows: on the SmartScreen prompt, choose More info > Run anyway.

On macOS, use Cmd where this README says Ctrl, and Option for word jumps and deletes. Switching tabs stays on Ctrl+Tab.

## Build from source

    cargo install --git https://github.com/goldenryu2000/MarkInk
    markink ~/notes

## License

MIT or Apache-2.0, at your option.
