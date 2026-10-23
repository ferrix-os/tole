//! What the file chooser and a file manager know about a file without
//! drawing it: its kind (a private key, a picture, an archive ...), its
//! size and age as a person reads them, the patterns that pick files
//! (`*.png | my?.txt | [image]`), the search over names in its three
//! modes, and what a preview may show of it -- never a private key's
//! contents.
//!
//! All of it is plain functions over paths and strings, so it is tested
//! without a `Ui`; `folder.rs` puts it behind `FolderListModel` and
//! `FileInfo`.

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

/// What a file is, as the chooser shows and filters it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FileKind {
    /// A folder, or a link to one.
    Dir,
    /// A private key: PEM or OpenSSH, by its first line.
    Private,
    /// A public key: `.pub`, or a line starting `ssh-`.
    Public,
    /// A picture, by its name.
    Image,
    /// A video, by its name.
    Video,
    /// Sound, by its name.
    Audio,
    /// An archive, by its name.
    Archive,
    /// A document to read (PDF and the office formats), by its name.
    Document,
    /// Text: by its name, or a small file whose start is UTF-8 with no
    /// NUL in it.
    Text,
    /// Anything else.
    Other,
}

impl FileKind {
    /// Its name in patterns and in `modelData.kind`.
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            FileKind::Dir => "folder",
            FileKind::Private => "private",
            FileKind::Public => "public",
            FileKind::Image => "image",
            FileKind::Video => "video",
            FileKind::Audio => "audio",
            FileKind::Archive => "archive",
            FileKind::Document => "document",
            FileKind::Text => "text",
            FileKind::Other => "other",
        }
    }

    /// The three letters on its badge.
    #[must_use]
    pub fn badge(self) -> &'static str {
        match self {
            FileKind::Dir => "DIR",
            FileKind::Private => "KEY",
            FileKind::Public => "PUB",
            FileKind::Image => "IMG",
            FileKind::Video => "VID",
            FileKind::Audio => "AUD",
            FileKind::Archive => "ZIP",
            FileKind::Document => "DOC",
            FileKind::Text => "TXT",
            FileKind::Other => "BIN",
        }
    }

    /// Its name in a sentence: "Private key".
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            FileKind::Dir => "Folder",
            FileKind::Private => "Private key",
            FileKind::Public => "Public key",
            FileKind::Image => "Image",
            FileKind::Video => "Video",
            FileKind::Audio => "Audio",
            FileKind::Archive => "Archive",
            FileKind::Document => "Document",
            FileKind::Text => "Text",
            FileKind::Other => "File",
        }
    }

    /// The kind a pattern's `[word]` names.
    #[must_use]
    pub fn from_word(word: &str) -> Option<FileKind> {
        Some(match word.to_lowercase().as_str() {
            "folder" | "folders" | "dir" => FileKind::Dir,
            "private" | "key" | "keys" => FileKind::Private,
            "public" => FileKind::Public,
            "image" | "images" | "picture" | "pictures" => FileKind::Image,
            "video" | "videos" => FileKind::Video,
            "audio" | "music" | "sound" => FileKind::Audio,
            "archive" | "archives" => FileKind::Archive,
            "document" | "documents" | "doc" => FileKind::Document,
            "text" | "file" => FileKind::Text,
            "other" => FileKind::Other,
            _ => return None,
        })
    }
}

/// The largest file whose start is read to tell its kind: keys and small
/// text files are below it; a large file is told by its name alone.
const SNIFF_LIMIT: u64 = 1 << 20;

fn extension(name: &str) -> String {
    match name.rsplit_once('.') {
        Some((stem, ext)) if !stem.is_empty() => ext.to_lowercase(),
        _ => String::new(),
    }
}

/// Whether `name` ends in one of the picture formats the preview draws.
#[must_use]
pub fn is_drawable_image(name: &str) -> bool {
    matches!(extension(name).as_str(), "png" | "jpg" | "jpeg" | "svg")
}

/// The kind of the file `name` at `path`; `is_dir` and `len` from its
/// metadata. A small file's first bytes are read.
#[must_use]
pub fn classify(path: &Path, name: &str, is_dir: bool, len: u64) -> FileKind {
    if is_dir {
        return FileKind::Dir;
    }
    let ext = extension(name);
    match ext.as_str() {
        "pub" => return FileKind::Public,
        "png" | "jpg" | "jpeg" | "gif" | "webp" | "svg" | "bmp" | "avif" | "ico" | "tif"
        | "tiff" => return FileKind::Image,
        "mp4" | "mkv" | "webm" | "mov" | "avi" | "m4v" | "ivf" => return FileKind::Video,
        "mp3" | "flac" | "ogg" | "opus" | "wav" | "m4a" | "aac" => return FileKind::Audio,
        "zip" | "tar" | "gz" | "tgz" | "xz" | "txz" | "zst" | "bz2" | "7z" | "rar" | "cpio"
        | "deb" | "rpm" => return FileKind::Archive,
        "pdf" | "odt" | "ods" | "odp" | "doc" | "docx" | "xls" | "xlsx" | "ppt" | "pptx"
        | "epub" => return FileKind::Document,
        _ => {}
    }
    if len > SNIFF_LIMIT {
        return FileKind::Other;
    }
    let head = read_head(path, 512);
    if head.starts_with(b"-----BEGIN ") {
        let first = head.split(|&b| b == b'\n').next().unwrap_or_default();
        if contains(first, b"PRIVATE KEY") {
            return FileKind::Private;
        }
    }
    if head.starts_with(b"ssh-") || head.starts_with(b"ecdsa-sha2-") {
        return FileKind::Public;
    }
    let text_ext = matches!(
        ext.as_str(),
        "txt"
            | "md"
            | "toml"
            | "yaml"
            | "yml"
            | "json"
            | "conf"
            | "cfg"
            | "ini"
            | "log"
            | "rs"
            | "c"
            | "h"
            | "py"
            | "sh"
            | "tole"
            | "csv"
            | "xml"
            | "html"
            | "css"
            | "js"
    );
    if text_ext || (!head.contains(&0) && std::str::from_utf8(utf8_prefix(&head)).is_ok()) {
        if head.is_empty() && !text_ext {
            return FileKind::Other;
        }
        return FileKind::Text;
    }
    FileKind::Other
}

/// `bytes` without a character cut off at its end.
fn utf8_prefix(bytes: &[u8]) -> &[u8] {
    match std::str::from_utf8(bytes) {
        Ok(_) => bytes,
        Err(error) if error.error_len().is_none() => {
            bytes.get(..error.valid_up_to()).unwrap_or(bytes)
        }
        Err(_) => bytes,
    }
}

fn contains(hay: &[u8], needle: &[u8]) -> bool {
    hay.windows(needle.len()).any(|w| w == needle)
}

fn read_head(path: &Path, limit: usize) -> Vec<u8> {
    use std::io::Read as _;
    let mut out = Vec::new();
    if let Ok(file) = std::fs::File::open(path) {
        let _ = file.take(limit as u64).read_to_end(&mut out);
    }
    out
}

/// A size as a person reads it: `411 B`, `2.6 KB`, `412 KB`, `2.1 MB`.
#[must_use]
pub fn size_text(bytes: u64) -> String {
    if bytes < 1024 {
        return format!("{bytes} B");
    }
    let units = ["KB", "MB", "GB", "TB", "PB"];
    let mut value = bytes as f64 / 1024.0;
    let mut unit = 0;
    while value >= 1024.0 && unit + 1 < units.len() {
        value /= 1024.0;
        unit += 1;
    }
    let name = units.get(unit).copied().unwrap_or("PB");
    if value >= 100.0 {
        format!("{value:.0} {name}")
    } else {
        let text = format!("{value:.1}");
        format!("{} {name}", text.strip_suffix(".0").unwrap_or(&text))
    }
}

thread_local! {
    /// What `home()` and `now()` answer instead of the machine's, for a
    /// drawing that must not depend on the machine it is made on.
    static FIXED: RefCell<(Option<PathBuf>, Option<i64>)> = const { RefCell::new((None, None)) };
}

/// Make this thread's home folder `home` and its clock `now` (seconds
/// since 1970, UTC) for everything here, or the machine's again with
/// `None`: a screenshot test's folders and dates.
pub fn fix_home_and_clock(home: Option<PathBuf>, now: Option<i64>) {
    FIXED.with(|fixed| *fixed.borrow_mut() = (home, now));
}

/// The home folder: `$HOME`, or the one a test fixed.
#[must_use]
pub fn home() -> PathBuf {
    if let Some(home) = FIXED.with(|fixed| fixed.borrow().0.clone()) {
        return home;
    }
    std::env::var_os("HOME").map_or_else(|| PathBuf::from("/"), PathBuf::from)
}

/// Seconds since 1970: the clock, or the one a test fixed.
#[must_use]
pub fn now() -> i64 {
    if let Some(now) = FIXED.with(|fixed| fixed.borrow().1) {
        return now;
    }
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX))
}

/// `path` with the home folder written `~`.
#[must_use]
pub fn tilde(path: &str) -> String {
    let home = home();
    let home = home.to_string_lossy();
    let home = home.trim_end_matches('/');
    if home.is_empty() {
        return path.to_owned();
    }
    match path.strip_prefix(home) {
        Some("") => "~".to_owned(),
        Some(rest) if rest.starts_with('/') => format!("~{rest}"),
        _ => path.to_owned(),
    }
}

/// The days since 1970-01-01 of a civil date's start, and back.
fn civil(days: i64) -> (i64, u32, u32) {
    // Howard Hinnant's days-from-civil, inverted.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

/// The local zone's offset from UTC in seconds, as the standard time of
/// `/etc/localtime`'s POSIX footer or `$TZ` gives it (summer time aside:
/// an hour's error at midnight, in "Today" against "Yesterday" only).
fn zone_offset() -> i64 {
    thread_local! {
        static OFFSET: std::cell::Cell<Option<i64>> = const { std::cell::Cell::new(None) };
    }
    if FIXED.with(|fixed| fixed.borrow().1.is_some()) {
        return 0;
    }
    if let Some(offset) = OFFSET.with(std::cell::Cell::get) {
        return offset;
    }
    let posix = std::env::var("TZ")
        .ok()
        .filter(|tz| !tz.starts_with(':'))
        .or_else(|| {
            let bytes = std::fs::read("/etc/localtime").ok()?;
            if bytes.get(..4)? != b"TZif" {
                return None;
            }
            let text = String::from_utf8_lossy(&bytes);
            let trimmed = text.trim_end_matches('\n');
            trimmed.rsplit('\n').next().map(str::to_owned)
        });
    let offset = posix.as_deref().and_then(posix_offset).unwrap_or(0);
    OFFSET.with(|cell| cell.set(Some(offset)));
    offset
}

/// The local zone's offset from UTC in seconds (its standard time; 0
/// while a test fixed the clock): what a program writing a local date
/// adds to the clock.
#[must_use]
pub fn local_offset() -> i64 {
    zone_offset()
}

/// The year, month and day of `days` since 1970-01-01.
#[must_use]
pub fn civil_date(days: i64) -> (i64, u32, u32) {
    civil(days)
}

/// The standard offset of a POSIX `TZ` string (`CET-1CEST,...`): east
/// of Greenwich positive, as seconds to add to UTC.
fn posix_offset(tz: &str) -> Option<i64> {
    let rest = if let Some(quoted) = tz.strip_prefix('<') {
        quoted.split_once('>')?.1
    } else {
        tz.trim_start_matches(|c: char| c.is_ascii_alphabetic())
    };
    let end = rest
        .find(|c: char| !(c.is_ascii_digit() || c == ':' || c == '+' || c == '-'))
        .unwrap_or(rest.len());
    let number = rest.get(..end)?;
    if number.is_empty() {
        return None;
    }
    let (sign, digits) = match number.strip_prefix('-') {
        Some(d) => (1, d),
        None => (-1, number.trim_start_matches('+')),
    };
    let mut parts = digits.split(':').map(|p| p.parse::<i64>().unwrap_or(0));
    let hours = parts.next().unwrap_or(0);
    let minutes = parts.next().unwrap_or(0);
    Some(sign * (hours * 3600 + minutes * 60))
}

const MONTHS: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];

/// When a file was changed, as a person reads it: `Today`, `Yesterday`,
/// `Sep 12`, or `Sep 12 2024` in another year.
#[must_use]
pub fn date_text(secs: i64) -> String {
    let offset = zone_offset();
    let day = (secs + offset).div_euclid(86_400);
    let today = (now() + offset).div_euclid(86_400);
    if day == today {
        return "Today".to_owned();
    }
    if day + 1 == today {
        return "Yesterday".to_owned();
    }
    let (year, month, date) = civil(day);
    let (this_year, _, _) = civil(today);
    let name = MONTHS.get(month as usize - 1).copied().unwrap_or("?");
    if year == this_year {
        format!("{name} {date}")
    } else {
        format!("{name} {date} {year}")
    }
}

/// One test of a pattern.
#[derive(Clone, Debug, PartialEq)]
enum Test {
    /// `[image]`.
    Kind(FileKind),
    /// `*.png`, lower-cased.
    Glob(Vec<char>),
}

/// A pattern of names and kinds: `*.png | my?.txt | [image]`. Parts are
/// separated by `|` or `,`; a file matches when any part does. `*` is any
/// text, `?` one character, case aside, and the whole name must match;
/// `[kind]` matches by kind. An empty pattern matches everything.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Pattern {
    tests: Vec<Test>,
}

impl Pattern {
    /// Read `text`.
    ///
    /// # Errors
    ///
    /// A sentence about the first part that could not be read, as the
    /// chooser shows it under the field.
    pub fn parse(text: &str) -> Result<Pattern, String> {
        let mut tests = Vec::new();
        for part in text
            .split(['|', ','])
            .map(str::trim)
            .filter(|p| !p.is_empty())
        {
            if let Some(word) = part.strip_prefix('[').and_then(|p| p.strip_suffix(']'))
                && !word.is_empty()
                && word.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
            {
                let kind = FileKind::from_word(word).ok_or_else(|| {
                    format!(
                        "Unknown kind \u{201c}{word}\u{201d}. Use image, private, public, text, archive, video, audio or folder."
                    )
                })?;
                tests.push(Test::Kind(kind));
                continue;
            }
            if part.contains(['[', ']']) {
                return Err(format!(
                    "\u{201c}{part}\u{201d} has an unmatched bracket. Kinds look like [image]."
                ));
            }
            tests.push(Test::Glob(part.to_lowercase().chars().collect()));
        }
        Ok(Pattern { tests })
    }

    /// Whether it has no parts, and so matches everything.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.tests.is_empty()
    }

    /// Whether a file named `name` of `kind` matches.
    #[must_use]
    pub fn matches(&self, name: &str, kind: FileKind) -> bool {
        if self.tests.is_empty() {
            return true;
        }
        let lower: Vec<char> = name.to_lowercase().chars().collect();
        self.tests.iter().any(|test| match test {
            Test::Kind(want) => *want == kind,
            Test::Glob(glob) => glob_matches(glob, &lower),
        })
    }

    /// Whether one of its parts names `kind`.
    #[must_use]
    pub fn names_kind(&self, kind: FileKind) -> bool {
        self.tests.contains(&Test::Kind(kind))
    }
}

/// `*` and `?` against the whole of `text`, without backtracking blowing
/// up: the classic two-pointer walk.
fn glob_matches(glob: &[char], text: &[char]) -> bool {
    let (mut g, mut t) = (0, 0);
    let mut star: Option<(usize, usize)> = None;
    while t < text.len() {
        match glob.get(g) {
            Some('*') => {
                star = Some((g, t));
                g += 1;
            }
            Some(&c) if c == '?' || Some(&c) == text.get(t) => {
                g += 1;
                t += 1;
            }
            _ => match star {
                Some((sg, st)) => {
                    g = sg + 1;
                    t = st + 1;
                    star = Some((sg, st + 1));
                }
                None => return false,
            },
        }
    }
    glob.get(g..)
        .is_some_and(|rest| rest.iter().all(|&c| c == '*'))
}

/// How the search field reads its text.
pub mod search_mode {
    /// The name contains the text, case aside.
    pub const BASIC: i32 = 0;
    /// A [`super::Pattern`].
    pub const ADVANCED: i32 = 1;
    /// A regular expression found in the name, case aside: the
    /// compositor's RE2-like syntax (`^id_.*$`).
    pub const REGEX: i32 = 2;
}

/// A search over names.
#[derive(Clone, Debug)]
pub enum Search {
    /// Everything.
    All,
    /// Names containing this, lower-cased.
    Contains(String),
    /// A pattern.
    Pattern(Pattern),
    /// A regular expression, over the lower-cased name.
    Regex(compositor_regex::Regex),
}

impl Search {
    /// The search `text` means in `mode` (`search_mode`).
    ///
    /// # Errors
    ///
    /// Why the text cannot be searched with.
    pub fn new(text: &str, mode: i32) -> Result<Search, String> {
        let text = text.trim();
        if text.is_empty() {
            return Ok(Search::All);
        }
        match mode {
            search_mode::ADVANCED => Pattern::parse(text).map(Search::Pattern),
            search_mode::REGEX => {
                let lower = text.to_lowercase();
                let (start, core) = match lower.strip_prefix('^') {
                    Some(rest) => ("", rest),
                    None => (".*", lower.as_str()),
                };
                let ends = core.ends_with('$') && !core.ends_with("\\$");
                let core = if ends {
                    core.get(..core.len() - 1).unwrap_or(core)
                } else {
                    core
                };
                let end = if ends { "" } else { ".*" };
                compositor_regex::Regex::new(&format!("{start}({core}){end}"))
                    .map(Search::Regex)
                    .map_err(|why| format!("Invalid regex: {}", why.trim_end_matches('.')))
            }
            _ => Ok(Search::Contains(text.to_lowercase())),
        }
    }

    /// Whether a file named `name` of `kind` is found.
    #[must_use]
    pub fn matches(&self, name: &str, kind: FileKind) -> bool {
        match self {
            Search::All => true,
            Search::Contains(text) => name.to_lowercase().contains(text.as_str()),
            Search::Pattern(pattern) => pattern.matches(name, kind),
            Search::Regex(regex) => regex.matches(&name.to_lowercase()),
        }
    }
}

/// What a preview shows of a file, in text: a private key's first and
/// last lines with dots between (its contents are never read out), the
/// start of a text file or public key, a folder's names. Empty for
/// anything else.
#[must_use]
pub fn preview_text(path: &Path, kind: FileKind, show_hidden: bool) -> String {
    const LINES: usize = 40;
    match kind {
        FileKind::Private => {
            let head = read_head(path, 64 * 1024);
            let text = String::from_utf8_lossy(&head);
            let first = text.lines().next().unwrap_or("-----BEGIN PRIVATE KEY-----");
            let last = text
                .lines()
                .rev()
                .find(|l| l.starts_with("-----END "))
                .unwrap_or("-----END PRIVATE KEY-----");
            let dots = "\u{2022}".repeat(32);
            format!("{first}\n{dots}\n{dots}\n{dots}\n{last}")
        }
        FileKind::Text | FileKind::Public => {
            let head = read_head(path, 4096);
            let text = String::from_utf8_lossy(utf8_prefix(&head)).into_owned();
            text.lines().take(LINES).collect::<Vec<_>>().join("\n")
        }
        FileKind::Dir => {
            let mut names: Vec<String> = std::fs::read_dir(path)
                .map(|read| {
                    read.filter_map(Result::ok)
                        .map(|e| e.file_name().to_string_lossy().into_owned())
                        .filter(|n| show_hidden || !n.starts_with('.'))
                        .collect()
                })
                .unwrap_or_default();
            names.sort_by_key(|n| n.to_lowercase());
            if names.is_empty() {
                return "Empty folder".to_owned();
            }
            let more = names.len().saturating_sub(LINES);
            names.truncate(LINES);
            let mut out = names.join("\n");
            if more > 0 {
                out.push_str(&format!("\n\u{2026} {more} more"));
            }
            out
        }
        _ => String::new(),
    }
}

/// How many entries a folder holds (hidden ones aside unless asked).
#[must_use]
pub fn item_count(path: &Path, show_hidden: bool) -> usize {
    std::fs::read_dir(path).map_or(0, |read| {
        read.filter_map(Result::ok)
            .filter(|e| show_hidden || !e.file_name().to_string_lossy().starts_with('.'))
            .count()
    })
}

/// Seconds since 1970 of a metadata's modification time.
#[must_use]
pub fn modified_secs(meta: &std::fs::Metadata) -> i64 {
    meta.modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map_or(0, |d| i64::try_from(d.as_secs()).unwrap_or(0))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn patterns_read_globs_kinds_and_say_what_is_wrong() {
        let p = Pattern::parse("*.png | my?.txt, [image]").expect("read");
        assert!(p.matches("Shot.PNG", FileKind::Other));
        assert!(p.matches("my1.txt", FileKind::Text));
        assert!(!p.matches("my12.txt", FileKind::Text));
        assert!(p.matches("x.jpg", FileKind::Image));
        assert!(!p.matches("notes.md", FileKind::Text));
        assert!(
            Pattern::parse("")
                .expect("read")
                .matches("a", FileKind::Other)
        );
        let bad = Pattern::parse("[pictur]").expect_err("refused");
        assert!(
            bad.starts_with("Unknown kind \u{201c}pictur\u{201d}"),
            "{bad}"
        );
        let bad = Pattern::parse("*.png | [image").expect_err("refused");
        assert!(bad.contains("unmatched bracket"), "{bad}");
        assert!(glob_matches(&['*'; 3], &[]));
        assert!(glob_matches(
            &"a*b*c".chars().collect::<Vec<_>>(),
            &"aXbYbZc".chars().collect::<Vec<_>>()
        ));
    }

    #[test]
    fn search_modes_contain_match_and_find() {
        let basic = Search::new("ED25", search_mode::BASIC).expect("read");
        assert!(basic.matches("id_ed25519", FileKind::Private));
        let regex = Search::new("^id_.*$", search_mode::REGEX).expect("read");
        assert!(regex.matches("id_rsa", FileKind::Private));
        assert!(!regex.matches("xid_rsa", FileKind::Private));
        let found = Search::new("rsa", search_mode::REGEX).expect("read");
        assert!(found.matches("staging_rsa.pub", FileKind::Public));
        assert!(Search::new("(a", search_mode::REGEX).is_err());
        let advanced = Search::new("[public]", search_mode::ADVANCED).expect("read");
        assert!(advanced.matches("a.pub", FileKind::Public));
    }

    #[test]
    fn sizes_and_dates_read_as_a_person_says_them() {
        assert_eq!(size_text(411), "411 B");
        assert_eq!(size_text(2662), "2.6 KB");
        assert_eq!(size_text(412 * 1024), "412 KB");
        assert_eq!(size_text(2 * 1024 * 1024 + 100 * 1024), "2.1 MB");
        assert_eq!(size_text(1024), "1 KB");
        // 2026-10-24 12:00 UTC.
        let noon = 1_792_843_200;
        fix_home_and_clock(Some(PathBuf::from("/home/demo")), Some(noon));
        assert_eq!(date_text(noon - 3600), "Today");
        assert_eq!(date_text(noon - 86_400), "Yesterday");
        assert_eq!(date_text(noon - 42 * 86_400), "Sep 12");
        assert_eq!(date_text(noon - 400 * 86_400), "Sep 19 2025");
        assert_eq!(tilde("/home/demo/.ssh"), "~/.ssh");
        assert_eq!(tilde("/home/demo"), "~");
        assert_eq!(tilde("/home/demonstration"), "/home/demonstration");
        fix_home_and_clock(None, None);
        assert_eq!(posix_offset("CET-1CEST,M3.5.0,M10.5.0/3"), Some(3600));
        assert_eq!(posix_offset("<-03>3"), Some(-3 * 3600));
        assert_eq!(posix_offset("UTC0"), Some(0));
    }

    #[test]
    fn a_private_key_is_told_by_its_first_line_and_never_shown() {
        let dir = std::env::temp_dir().join(format!("tole-files-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let key = dir.join("id_test");
        let _ = std::fs::write(
            &key,
            "-----BEGIN OPENSSH PRIVATE KEY-----\nsecret\n-----END OPENSSH PRIVATE KEY-----\n",
        );
        assert_eq!(classify(&key, "id_test", false, 80), FileKind::Private);
        let shown = preview_text(&key, FileKind::Private, false);
        assert!(!shown.contains("secret"), "{shown}");
        assert!(shown.starts_with("-----BEGIN OPENSSH PRIVATE KEY-----"));
        let public = dir.join("id_test.pub");
        let _ = std::fs::write(&public, "ssh-ed25519 AAAA me@here\n");
        assert_eq!(
            classify(&public, "id_test.pub", false, 24),
            FileKind::Public
        );
        let notes = dir.join("notes");
        let _ = std::fs::write(&notes, "plain words\n");
        assert_eq!(classify(&notes, "notes", false, 12), FileKind::Text);
        assert_eq!(classify(&dir, "x", true, 0), FileKind::Dir);
        assert_eq!(
            classify(&dir.join("a.tar.gz"), "a.tar.gz", false, 9),
            FileKind::Archive
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}

#[cfg(test)]
mod model_tests {
    #[test]
    fn current_follows_the_index() {
        let mut ui = crate::Ui::new(compositor_text::Fonts::new());
        let m = crate::handles::FolderListModel::new(&mut ui, None).expect("made");
        m.props.folder.set(&mut ui, "/".into());
        let n = ui.get(m.props.count);
        assert!(n > 0);
        m.props.current_index.set(&mut ui, 0);
        let cur = ui.get(m.props.current);
        assert!(!cur.path.is_empty(), "{cur:?}");
    }
}
