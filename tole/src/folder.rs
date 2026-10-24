//! `FolderListModel`: a folder's entries as a model a `ListView` shows,
//! after Qt's `Qt.labs.folderlistmodel` -- what the file chooser and a
//! file manager list; and `FileInfo`, one file as a preview pane shows it.
//!
//! Neither draws anything. A `FolderListModel` reads its folder when
//! `folder`, `showHidden`, `showFiles` or `revision` change, and filters
//! and sorts what it read when the search, the accepted pattern or the
//! sort change, without reading the disk again: folders first, then
//! files, by name (case aside) unless `sortField` says otherwise. A
//! folder that cannot be read lists nothing.

use std::path::Path;
use std::rc::Rc;

use crate::files::{self, FileKind, Pattern, Search};
use crate::item::{FileInfoProps, FolderProps};
use crate::model::{ModelRc, VecModel};
use crate::ui::Ui;
use crate::value::{PropType, Value};

/// One entry of a folder, as a delegate reads it: `modelData.name`,
/// `modelData.kind`, `modelData.sizeText` and the rest.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct FileEntry {
    /// Its name.
    pub name: Rc<str>,
    /// Its whole path.
    pub path: Rc<str>,
    /// Whether it is a folder (or a link to one).
    pub is_dir: bool,
    /// Its size in bytes; 0 for a folder.
    pub size: f32,
    /// Its kind's name: `folder`, `private`, `public`, `image`, `video`,
    /// `audio`, `archive`, `document`, `text` or `other`.
    pub kind: Rc<str>,
    /// The three letters on its badge: `DIR`, `KEY`, `IMG` ...
    pub badge: Rc<str>,
    /// Its kind in a sentence: `Private key`.
    pub kind_label: Rc<str>,
    /// Its size as read: `2.6 KB`; a folder's, its count of entries.
    pub size_text: Rc<str>,
    /// When it changed, as read: `Today`, `Sep 12`.
    pub modified: Rc<str>,
    /// Whether its name starts with `.`.
    pub hidden: bool,
    /// Whether the model's `accept` pattern takes it; folders always.
    pub accepted: bool,
    /// Whether it is a picture the `Image` item draws.
    pub is_image: bool,
    /// For a folder, how many entries it holds.
    pub items: i32,
    /// Seconds since 1970 it changed, for sorting.
    pub(crate) modified_secs: i64,
    /// Its kind.
    pub(crate) file_kind: Option<FileKind>,
}

impl FileEntry {
    /// An entry that is only a name and a path, for a program that lists
    /// places itself: a `Breadcrumbs` crumb that is not a folder read
    /// from the disk (a folder inside an archive, a tag).
    #[must_use]
    pub fn named(name: &str, path: &str) -> Self {
        FileEntry {
            name: name.into(),
            path: path.into(),
            ..FileEntry::default()
        }
    }
}

impl PropType for FileEntry {
    fn into_value(self) -> Value {
        Value::custom(self)
    }
    fn from_value(value: &Value) -> Option<Self> {
        value.downcast::<Self>().cloned()
    }
    fn fallback() -> Self {
        FileEntry::default()
    }
}

/// What a folder read gave: its entries, shared, compared by identity.
#[derive(Clone, Debug, Default)]
pub(crate) struct Entries(Rc<Vec<FileEntry>>);

impl PartialEq for Entries {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.0, &other.0)
    }
}

impl PropType for Entries {
    fn into_value(self) -> Value {
        Value::custom(self)
    }
    fn from_value(value: &Value) -> Option<Self> {
        value.downcast::<Self>().cloned()
    }
    fn fallback() -> Self {
        Entries::default()
    }
}

/// What a `FolderListModel`'s `sortField` may be.
pub mod sort_field {
    /// By name, case aside.
    pub const NAME: i32 = 0;
    /// By size, largest first.
    pub const SIZE: i32 = 1;
    /// By when it changed, newest first.
    pub const TIME: i32 = 2;
    /// By kind, then name.
    pub const TYPE: i32 = 3;
}

/// `folder` with a leading `~` made the home folder.
#[must_use]
pub fn expand_home(folder: &str) -> String {
    match folder.strip_prefix('~') {
        Some(rest) if rest.is_empty() || rest.starts_with('/') => {
            let home = files::home();
            format!("{}{rest}", home.to_string_lossy().trim_end_matches('/'))
        }
        _ => folder.to_owned(),
    }
}

/// One entry of a folder, from its path and name.
#[must_use]
pub fn entry_of(path: &Path, name: &str, show_hidden: bool) -> FileEntry {
    // Followed through a link, as a person means it.
    let meta = std::fs::metadata(path).ok();
    let is_dir = meta.as_ref().is_some_and(std::fs::Metadata::is_dir);
    let len = if is_dir {
        0
    } else {
        meta.as_ref().map_or(0, std::fs::Metadata::len)
    };
    let kind = files::classify(path, name, is_dir, len);
    let modified_secs = meta.as_ref().map_or(0, files::modified_secs);
    let items = if is_dir {
        i32::try_from(files::item_count(path, show_hidden)).unwrap_or(i32::MAX)
    } else {
        0
    };
    FileEntry {
        name: Rc::from(name),
        path: Rc::from(path.to_string_lossy().as_ref()),
        is_dir,
        size: len as f32,
        kind: Rc::from(kind.name()),
        badge: Rc::from(kind.badge()),
        kind_label: Rc::from(kind.label()),
        size_text: Rc::from(if is_dir {
            format!("{items} item{}", if items == 1 { "" } else { "s" })
        } else {
            files::size_text(len)
        }),
        modified: Rc::from(if meta.is_some() {
            files::date_text(modified_secs)
        } else {
            String::new()
        }),
        hidden: name.starts_with('.'),
        accepted: true,
        is_image: !is_dir && files::is_drawable_image(name),
        items,
        modified_secs,
        file_kind: Some(kind),
    }
}

/// The entries of `dir`, unsorted and unfiltered but for hidden names
/// and, without `files`, everything but folders.
fn list(dir: &str, hidden: bool, files: bool) -> Vec<FileEntry> {
    let Ok(read) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    read.filter_map(Result::ok)
        .filter_map(|entry| {
            let name = entry.file_name().to_string_lossy().into_owned();
            if !hidden && name.starts_with('.') {
                return None;
            }
            let row = entry_of(&entry.path(), &name, hidden);
            (row.is_dir || files).then_some(row)
        })
        .collect()
}

/// What a `FolderListModel` shows of what it read.
struct Shown {
    search: Result<Search, String>,
    accept: Result<Pattern, String>,
    accepted_only: bool,
    sort: i32,
    reversed: bool,
}

impl Shown {
    fn rows(&self, entries: &[FileEntry]) -> Vec<FileEntry> {
        let Ok(search) = &self.search else {
            return Vec::new();
        };
        let accept = self.accept.as_ref().ok();
        // A pattern that cannot be read takes nothing, but hides nothing
        // unless only what it takes is asked for.
        let mut rows: Vec<FileEntry> = entries
            .iter()
            .filter_map(|row| {
                let kind = row.file_kind.unwrap_or(FileKind::Other);
                if !search.matches(&row.name, kind) {
                    return None;
                }
                let accepted = row.is_dir || accept.is_some_and(|p| p.matches(&row.name, kind));
                if self.accepted_only && !accepted {
                    return None;
                }
                let mut row = row.clone();
                row.accepted = accepted;
                Some(row)
            })
            .collect();
        let by_name = |a: &FileEntry, b: &FileEntry| {
            a.name
                .to_lowercase()
                .cmp(&b.name.to_lowercase())
                .then_with(|| a.name.cmp(&b.name))
        };
        rows.sort_by(|a, b| {
            let within = match self.sort {
                sort_field::SIZE => b
                    .size
                    .total_cmp(&a.size)
                    .then_with(|| b.items.cmp(&a.items))
                    .then_with(|| by_name(a, b)),
                sort_field::TIME => b
                    .modified_secs
                    .cmp(&a.modified_secs)
                    .then_with(|| by_name(a, b)),
                sort_field::TYPE => a.kind.cmp(&b.kind).then_with(|| by_name(a, b)),
                _ => by_name(a, b),
            };
            b.is_dir.cmp(&a.is_dir).then(if self.reversed {
                within.reverse()
            } else {
                within
            })
        });
        rows
    }
}

/// The folders from the top to `path`, as breadcrumbs: each with its
/// name (`~` for the home folder, `/` for the root) and its path.
fn crumbs(path: &str) -> Vec<FileEntry> {
    let home = files::home();
    let home = home.to_string_lossy().trim_end_matches('/').to_owned();
    let mut out = Vec::new();
    let rest = if !home.is_empty() && (path == home || path.starts_with(&format!("{home}/"))) {
        out.push((Rc::from("~"), home.clone()));
        path.get(home.len()..).unwrap_or("")
    } else {
        out.push((Rc::from("/"), "/".to_owned()));
        path
    };
    let mut at = out.last().map(|(_, p)| p.clone()).unwrap_or_default();
    for part in rest.split('/').filter(|p| !p.is_empty()) {
        if !at.ends_with('/') {
            at.push('/');
        }
        at.push_str(part);
        out.push((Rc::from(part), at.clone()));
    }
    out.into_iter()
        .map(|(name, path)| FileEntry {
            name,
            path: Rc::from(path.as_str()),
            is_dir: true,
            kind: Rc::from("folder"),
            badge: Rc::from("DIR"),
            accepted: true,
            ..FileEntry::default()
        })
        .collect()
}

impl Ui {
    /// A new `FolderListModel`'s properties.
    pub(crate) fn folder_props(&mut self) -> FolderProps {
        let props = FolderProps {
            folder: self.prop(Rc::<str>::from("~")),
            show_hidden: self.prop(false),
            show_files: self.prop(true),
            files: self.prop(ModelRc::<FileEntry>::default()),
            parent_folder: self.prop(Rc::<str>::from("")),
            path: self.prop(Rc::<str>::from("")),
            display_path: self.prop(Rc::<str>::from("")),
            crumbs: self.prop(ModelRc::<FileEntry>::default()),
            name_filter: self.prop(Rc::<str>::from("")),
            filter_mode: self.prop(files::search_mode::BASIC),
            filter_error: self.prop(Rc::<str>::from("")),
            accept: self.prop(Rc::<str>::from("")),
            accept_error: self.prop(Rc::<str>::from("")),
            accepted_only: self.prop(false),
            sort_field: self.prop(sort_field::NAME),
            sort_reversed: self.prop(false),
            revision: self.prop(0_i32),
            count: self.prop(0_i32),
            total_size: self.prop(0.0_f32),
            current_index: self.prop(-1_i32),
            current: self.prop(FileEntry::default()),
            entries: self.prop(Entries::default()),
            rows: self.prop(Entries::default()),
        };
        let folder = props.folder;
        props
            .path
            .bind(self, move |ui| Rc::from(expand_home(&ui.get(folder))));
        let path = props.path;
        props
            .display_path
            .bind(self, move |ui| Rc::from(files::tilde(&ui.get(path))));
        props.crumbs.bind(self, move |ui| {
            ModelRc::new(VecModel::new(crumbs(&ui.get(path))))
        });
        props.parent_folder.bind(self, move |ui| {
            let path = ui.get(path);
            Rc::from(
                Path::new(&*path)
                    .parent()
                    .map_or(String::new(), |p| p.to_string_lossy().into_owned()),
            )
        });
        let (hidden, files_too, revision) = (props.show_hidden, props.show_files, props.revision);
        props.entries.bind(self, move |ui| {
            let _ = ui.get(revision);
            let path = ui.get(path);
            Entries(Rc::new(list(&path, ui.get(hidden), ui.get(files_too))))
        });
        let (filter, mode) = (props.name_filter, props.filter_mode);
        props.filter_error.bind(self, move |ui| {
            let (text, mode) = (ui.get(filter), ui.get(mode));
            Rc::from(Search::new(&text, mode).err().unwrap_or_default().as_str())
        });
        let accept = props.accept;
        props.accept_error.bind(self, move |ui| {
            Rc::from(
                Pattern::parse(&ui.get(accept))
                    .err()
                    .unwrap_or_default()
                    .as_str(),
            )
        });
        let (entries, only) = (props.entries, props.accepted_only);
        let (sort, reversed) = (props.sort_field, props.sort_reversed);
        let rows = props.rows;
        rows.bind(self, move |ui| {
            let shown = Shown {
                search: Search::new(&ui.get(filter), ui.get(mode)),
                accept: Pattern::parse(&ui.get(accept)),
                accepted_only: ui.get(only),
                sort: ui.get(sort),
                reversed: ui.get(reversed),
            };
            Entries(Rc::new(shown.rows(&ui.get(entries).0)))
        });
        props.files.bind(self, move |ui| {
            ModelRc::new(VecModel::new(ui.get(rows).0.to_vec()))
        });
        props.count.bind(self, move |ui| {
            i32::try_from(ui.get(rows).0.len()).unwrap_or(i32::MAX)
        });
        let index = props.current_index;
        props.current.bind(self, move |ui| {
            let at = ui.get(index);
            usize::try_from(at)
                .ok()
                .and_then(|at| ui.get(rows).0.get(at).cloned())
                .unwrap_or_default()
        });
        props
            .total_size
            .bind(self, move |ui| ui.get(rows).0.iter().map(|r| r.size).sum());
        props
    }

    /// A `FolderListModel`'s own properties.
    #[must_use]
    pub fn folder_list(&self, item: crate::ItemId) -> Option<FolderProps> {
        match self.kind(item)? {
            crate::Kind::FolderListModel(props) => Some(props),
            _ => None,
        }
    }

    /// A new `FileInfo`'s properties.
    pub(crate) fn file_info_props(&mut self) -> FileInfoProps {
        let text = |ui: &mut Ui| ui.prop(Rc::<str>::from(""));
        let props = FileInfoProps {
            path: text(self),
            accept: text(self),
            show_hidden: self.prop(false),
            revision: self.prop(0_i32),
            entry: self.prop(FileEntry::default()),
            exists: self.prop(false),
            location: text(self),
            text: text(self),
            refusal: text(self),
        };
        let (path, hidden, revision) = (props.path, props.show_hidden, props.revision);
        props.entry.bind(self, move |ui| {
            let _ = ui.get(revision);
            let path = expand_home(&ui.get(path));
            if path.is_empty() {
                return FileEntry::default();
            }
            let at = Path::new(&path);
            let name = at
                .file_name()
                .map_or_else(|| path.clone(), |n| n.to_string_lossy().into_owned());
            entry_of(at, &name, ui.get(hidden))
        });
        let entry = props.entry;
        props.exists.bind(self, move |ui| {
            let path = ui.get(entry).path;
            !path.is_empty() && Path::new(&*path).symlink_metadata().is_ok()
        });
        props.location.bind(self, move |ui| {
            let path = ui.get(entry).path;
            Rc::from(
                Path::new(&*path)
                    .parent()
                    .map_or(String::new(), |p| files::tilde(&p.to_string_lossy())),
            )
        });
        props.text.bind(self, move |ui| {
            let row = ui.get(entry);
            if row.path.is_empty() {
                return Rc::from("");
            }
            let kind = row.file_kind.unwrap_or(FileKind::Other);
            Rc::from(files::preview_text(Path::new(&*row.path), kind, ui.get(hidden)).as_str())
        });
        let accept = props.accept;
        props.refusal.bind(self, move |ui| {
            let row = ui.get(entry);
            if row.path.is_empty() || row.is_dir {
                return Rc::from("");
            }
            let kind = row.file_kind.unwrap_or(FileKind::Other);
            match Pattern::parse(&ui.get(accept)) {
                Ok(pattern) if pattern.matches(&row.name, kind) => Rc::from(""),
                Ok(pattern)
                    if kind == FileKind::Public && pattern.names_kind(FileKind::Private) =>
                {
                    Rc::from("Public key: use the file without .pub")
                }
                _ => Rc::from("Not an accepted file type"),
            }
        });
        props
    }

    /// A `FileInfo`'s own properties.
    #[must_use]
    pub fn file_info(&self, item: crate::ItemId) -> Option<FileInfoProps> {
        match self.kind(item)? {
            crate::Kind::FileInfo(props) => Some(props),
            _ => None,
        }
    }
}
