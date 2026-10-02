//! Reading and atomically writing note files.

use std::fmt;
use std::fs::{self, File, OpenOptions};
use std::hash::{DefaultHasher, Hash, Hasher};
use std::io::{self, Write};
use std::path::Path;

const BOM: &str = "\u{feff}";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LineEnding {
    #[default]
    Lf,
    CrLf,
}

/// How a note's text is encoded on disk.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct TextFormat {
    pub line_ending: LineEnding,
    pub bom: bool,
}

impl TextFormat {
    /// Splits raw file text into editor text and its on-disk format.
    pub fn decode(raw: &str) -> (String, TextFormat) {
        let (bom, body) = match raw.strip_prefix(BOM) {
            Some(rest) => (true, rest),
            None => (false, raw),
        };
        let crlf = body.matches("\r\n").count();
        let lf = body.matches('\n').count() - crlf;
        if crlf > lf {
            let format = TextFormat {
                line_ending: LineEnding::CrLf,
                bom,
            };
            (body.replace("\r\n", "\n"), format)
        } else {
            let format = TextFormat {
                line_ending: LineEnding::Lf,
                bom,
            };
            (body.to_owned(), format)
        }
    }

    /// Inverse of [`TextFormat::decode`].
    pub fn encode(&self, text: &str) -> Vec<u8> {
        let mut out = String::with_capacity(text.len() + BOM.len());
        if self.bom {
            out.push_str(BOM);
        }
        match self.line_ending {
            LineEnding::Lf => out.push_str(text),
            LineEnding::CrLf => out.push_str(&text.replace('\n', "\r\n")),
        }
        out.into_bytes()
    }
}

/// Identity of a file's bytes, used to detect outside changes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DiskSnapshot {
    hash: u64,
    len: u64,
}

impl DiskSnapshot {
    pub fn of(bytes: &[u8]) -> Self {
        let mut hasher = DefaultHasher::new();
        bytes.hash(&mut hasher);
        Self {
            hash: hasher.finish(),
            len: bytes.len() as u64,
        }
    }
}

/// A note as read from disk.
#[derive(Debug, Clone)]
pub struct LoadedNote {
    pub text: String,
    pub format: TextFormat,
    pub snapshot: DiskSnapshot,
    pub read_only: bool,
}

impl LoadedNote {
    /// A writable note whose file content is exactly `text`.
    pub fn from_text(text: &str) -> Self {
        Self {
            text: text.to_owned(),
            format: TextFormat::default(),
            snapshot: DiskSnapshot::of(text.as_bytes()),
            read_only: false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReadError {
    NotFound,
    NotUtf8,
    Io(String),
}

impl fmt::Display for ReadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotFound => f.write_str("file not found"),
            Self::NotUtf8 => f.write_str("not valid UTF-8 text"),
            Self::Io(err) => f.write_str(err),
        }
    }
}

impl From<io::Error> for ReadError {
    fn from(err: io::Error) -> Self {
        match err.kind() {
            io::ErrorKind::NotFound => Self::NotFound,
            _ => Self::Io(err.to_string()),
        }
    }
}

/// Reads a note. Refuses non-UTF-8 content instead of guessing.
pub fn read_note(path: &Path) -> Result<LoadedNote, ReadError> {
    let bytes = fs::read(path)?;
    let snapshot = DiskSnapshot::of(&bytes);
    let raw = String::from_utf8(bytes).map_err(|_| ReadError::NotUtf8)?;
    let (text, format) = TextFormat::decode(&raw);
    let read_only = OpenOptions::new()
        .append(true)
        .open(path)
        .is_err_and(|e| e.kind() == io::ErrorKind::PermissionDenied);
    Ok(LoadedNote {
        text,
        format,
        snapshot,
        read_only,
    })
}

/// What a save expects to find on disk before writing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Expect {
    Snapshot(DiskSnapshot),
    Anything,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SaveError {
    /// The file changed on disk since we last read or wrote it.
    Conflict,
    /// The file was deleted on disk.
    Missing,
    Io(String),
}

impl fmt::Display for SaveError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Conflict => f.write_str("file changed on disk"),
            Self::Missing => f.write_str("file was deleted"),
            Self::Io(err) => f.write_str(err),
        }
    }
}

/// Saves `text` unless the file on disk no longer matches `expect`.
pub fn save_note(
    path: &Path,
    text: &str,
    format: TextFormat,
    expect: Expect,
) -> Result<DiskSnapshot, SaveError> {
    if let Expect::Snapshot(known) = expect {
        match fs::read(path) {
            Ok(current) if DiskSnapshot::of(&current) != known => {
                return Err(SaveError::Conflict);
            }
            Ok(_) => {}
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Err(SaveError::Missing),
            Err(e) => return Err(SaveError::Io(e.to_string())),
        }
    }
    let bytes = format.encode(text);
    write_atomic(path, &bytes).map_err(|e| SaveError::Io(e.to_string()))?;
    Ok(DiskSnapshot::of(&bytes))
}

/// Writes through a temp file and rename, so readers never see a partial file.
pub fn write_atomic(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let target = match fs::symlink_metadata(path) {
        Ok(meta) if meta.file_type().is_symlink() => fs::canonicalize(path)?,
        _ => path.to_path_buf(),
    };
    let dir = target
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let name = target
        .file_name()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "path has no file name"))?;
    let tmp = dir.join(format!(".{}.markink-tmp", name.to_string_lossy()));
    let permissions = fs::metadata(&target).ok().map(|m| m.permissions());
    let result = (|| -> io::Result<()> {
        let mut file = File::create(&tmp)?;
        file.write_all(bytes)?;
        if let Some(permissions) = permissions {
            file.set_permissions(permissions)?;
        }
        file.sync_all()?;
        fs::rename(&tmp, &target)?;
        File::open(dir)?.sync_all()
    })();
    if result.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn lf_text_is_untouched() {
        let (text, format) = TextFormat::decode("a\nb\n");
        assert_eq!(text, "a\nb\n");
        assert_eq!(format, TextFormat::default());
    }

    #[test]
    fn crlf_is_normalized_and_restored() {
        let (text, format) = TextFormat::decode("a\r\nb\r\n");
        assert_eq!(text, "a\nb\n");
        assert_eq!(format.line_ending, LineEnding::CrLf);
        assert_eq!(format.encode(&text), b"a\r\nb\r\n");
    }

    #[test]
    fn bom_is_stripped_and_restored() {
        let (text, format) = TextFormat::decode("\u{feff}# hi\n");
        assert_eq!(text, "# hi\n");
        assert!(format.bom);
        assert_eq!(format.encode(&text), "\u{feff}# hi\n".as_bytes());
    }

    #[test]
    fn minority_crlf_is_kept_verbatim() {
        let raw = "a\nb\nc\r\n";
        let (text, format) = TextFormat::decode(raw);
        assert_eq!(text, raw);
        assert_eq!(format.encode(&text), raw.as_bytes());
    }

    #[test]
    fn missing_final_newline_is_kept() {
        let (text, format) = TextFormat::decode("a\r\nb");
        assert_eq!(format.encode(&text), b"a\r\nb");
    }

    #[test]
    fn reads_note() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.md");
        fs::write(&path, "hello\n").unwrap();
        let note = read_note(&path).unwrap();
        assert_eq!(note.text, "hello\n");
        assert_eq!(note.snapshot, DiskSnapshot::of(b"hello\n"));
        assert!(!note.read_only);
    }

    #[test]
    fn refuses_non_utf8() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("bin.md");
        fs::write(&path, [0xff, 0xfe, 0x00]).unwrap();
        assert_eq!(read_note(&path).unwrap_err(), ReadError::NotUtf8);
    }

    #[test]
    fn missing_file_is_not_found() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(
            read_note(&dir.path().join("x.md")).unwrap_err(),
            ReadError::NotFound
        );
    }

    #[test]
    fn detects_read_only() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("ro.md");
        fs::write(&path, "x").unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o444)).unwrap();
        if OpenOptions::new().append(true).open(&path).is_ok() {
            return; // running as root
        }
        assert!(read_note(&path).unwrap().read_only);
    }

    fn snap(text: &str) -> Expect {
        Expect::Snapshot(DiskSnapshot::of(text.as_bytes()))
    }

    #[test]
    fn saves_and_returns_snapshot() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.md");
        fs::write(&path, "old").unwrap();
        let saved = save_note(&path, "new", TextFormat::default(), snap("old")).unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "new");
        assert_eq!(saved, DiskSnapshot::of(b"new"));
    }

    #[test]
    fn refuses_when_changed_on_disk() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.md");
        fs::write(&path, "external").unwrap();
        let err = save_note(&path, "mine", TextFormat::default(), snap("old")).unwrap_err();
        assert_eq!(err, SaveError::Conflict);
        assert_eq!(fs::read_to_string(&path).unwrap(), "external");
    }

    #[test]
    fn reports_missing_file_without_recreating() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.md");
        let err = save_note(&path, "mine", TextFormat::default(), snap("old")).unwrap_err();
        assert_eq!(err, SaveError::Missing);
        assert!(!path.exists());
    }

    #[test]
    fn anything_overwrites_and_recreates() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.md");
        save_note(&path, "one", TextFormat::default(), Expect::Anything).unwrap();
        fs::write(&path, "external").unwrap();
        save_note(&path, "two", TextFormat::default(), Expect::Anything).unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "two");
    }

    #[test]
    fn crlf_and_bom_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.md");
        let raw = "\u{feff}a\r\nb\r\n";
        fs::write(&path, raw).unwrap();
        let note = read_note(&path).unwrap();
        save_note(
            &path,
            &note.text,
            note.format,
            Expect::Snapshot(note.snapshot),
        )
        .unwrap();
        assert_eq!(fs::read(&path).unwrap(), raw.as_bytes());
    }

    #[test]
    fn leaves_no_temp_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.md");
        save_note(&path, "x", TextFormat::default(), Expect::Anything).unwrap();
        let names: Vec<_> = fs::read_dir(dir.path())
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        assert_eq!(names, vec![std::ffi::OsString::from("a.md")]);
    }

    #[test]
    fn keeps_permissions() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.md");
        fs::write(&path, "x").unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
        save_note(&path, "y", TextFormat::default(), snap("x")).unwrap();
        let mode = fs::metadata(&path).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600);
    }

    #[test]
    fn follows_symlinks() {
        let dir = tempfile::tempdir().unwrap();
        let real = dir.path().join("real.md");
        let link = dir.path().join("link.md");
        fs::write(&real, "x").unwrap();
        std::os::unix::fs::symlink(&real, &link).unwrap();
        save_note(&link, "y", TextFormat::default(), snap("x")).unwrap();
        assert!(
            fs::symlink_metadata(&link)
                .unwrap()
                .file_type()
                .is_symlink()
        );
        assert_eq!(fs::read_to_string(&real).unwrap(), "y");
    }

    #[test]
    fn failed_write_keeps_original() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.md");
        fs::write(&path, "x").unwrap();
        fs::set_permissions(dir.path(), fs::Permissions::from_mode(0o555)).unwrap();
        let privileged = fs::File::create(dir.path().join("probe")).is_ok();
        let result = save_note(&path, "y", TextFormat::default(), snap("x"));
        fs::set_permissions(dir.path(), fs::Permissions::from_mode(0o755)).unwrap();
        if privileged {
            return;
        }
        assert!(matches!(result, Err(SaveError::Io(_))));
        assert_eq!(fs::read_to_string(&path).unwrap(), "x");
    }

    proptest! {
        #[test]
        fn decode_encode_round_trips(text in "[a-z \n]{0,40}", crlf: bool, bom: bool) {
            let line_ending = if crlf { LineEnding::CrLf } else { LineEnding::Lf };
            let format = TextFormat { line_ending, bom };
            let raw = String::from_utf8(format.encode(&text)).unwrap();
            let (decoded, _) = TextFormat::decode(&raw);
            prop_assert_eq!(decoded, text);
        }
    }
}
