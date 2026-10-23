//! `FolderListModel`: a folder's entries as a model a `ListView` shows,
//! after Qt's `Qt.labs.folderlistmodel` -- what the file chooser lists.
//!
//! It draws nothing. Its `files` are worked out when `folder`,
//! `showHidden` or `showFiles` change: folders first, then files, each
//! by name, case aside. A folder that cannot be read lists nothing.

use std::rc::Rc;

use crate::item::FolderProps;
use crate::model::{ModelRc, VecModel};
use crate::ui::Ui;
use crate::value::{PropType, Value};

/// One entry of a folder, as a delegate reads it: `modelData.name`,
/// `modelData.path`, `modelData.isDir`, `modelData.size`.
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

/// `folder` with a leading `~` made the home folder.
#[must_use]
pub fn expand_home(folder: &str) -> String {
    match folder.strip_prefix('~') {
        Some(rest) if rest.is_empty() || rest.starts_with('/') => {
            let home = std::env::var("HOME").unwrap_or_default();
            format!("{home}{rest}")
        }
        _ => folder.to_owned(),
    }
}

/// The entries of `dir`: folders first, then files, each by name.
fn list(dir: &str, hidden: bool, files: bool) -> Vec<FileEntry> {
    let Ok(read) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut out: Vec<FileEntry> = read
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let name = entry.file_name().to_string_lossy().into_owned();
            if !hidden && name.starts_with('.') {
                return None;
            }
            let path = entry.path();
            // Followed through a link, as a person means it.
            let meta = std::fs::metadata(&path).ok();
            let is_dir = meta.as_ref().is_some_and(std::fs::Metadata::is_dir);
            if !is_dir && !files {
                return None;
            }
            Some(FileEntry {
                name: Rc::from(name),
                path: Rc::from(path.to_string_lossy().as_ref()),
                is_dir,
                size: if is_dir {
                    0.0
                } else {
                    meta.map_or(0.0, |m| m.len() as f32)
                },
            })
        })
        .collect();
    out.sort_by(|a, b| {
        b.is_dir
            .cmp(&a.is_dir)
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
    out
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
        };
        let folder = props.folder;
        props
            .path
            .bind(self, move |ui| Rc::from(expand_home(&ui.get(folder))));
        let path = props.path;
        props.parent_folder.bind(self, move |ui| {
            let path = ui.get(path);
            Rc::from(
                std::path::Path::new(&*path)
                    .parent()
                    .map_or(String::new(), |p| p.to_string_lossy().into_owned()),
            )
        });
        let (hidden, files) = (props.show_hidden, props.show_files);
        props.files.bind(self, move |ui| {
            let path = ui.get(path);
            let rows = list(&path, ui.get(hidden), ui.get(files));
            ModelRc::new(VecModel::new(rows))
        });
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
}
