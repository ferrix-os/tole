//! The item tree (`docs/UI-FRAMEWORK.md` §4.2): items in a generational
//! arena, each a kind and its properties, a parent and children.
//!
//! An item is made with [`Ui::create`], which gives every property its
//! default -- `width` and `height` bound to the implicit size, as in QML --
//! and, under a positioner, binds its place. A component of the markup is
//! not a kind: the compiler makes it as a subtree of these.

use std::rc::Rc;

use crate::ui::{Prop, Ui};
use crate::value::{Color, Elide, Font, HAlign, VAlign};

/// An item's id: an index and a generation, so the id of a destroyed item
/// (a recycled delegate's, say) is refused rather than followed.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ItemId {
    /// The index.
    index: u32,
    /// Its generation.
    generation: u32,
}

/// What every item has.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Common {
    /// Left, from the parent's left.
    pub x: Prop<f32>,
    /// Top, from the parent's top.
    pub y: Prop<f32>,
    /// Width; bound to `implicit_width` until set.
    pub width: Prop<f32>,
    /// Height; bound to `implicit_height` until set.
    pub height: Prop<f32>,
    /// The width its content asks for.
    pub implicit_width: Prop<f32>,
    /// The height its content asks for.
    pub implicit_height: Prop<f32>,
    /// Whether it and its children are drawn (and take room in a
    /// positioner).
    pub visible: Prop<bool>,
    /// 0 to 1, for it and its children together.
    pub opacity: Prop<f32>,
    /// Whether its children are cut at its edges.
    pub clip: Prop<bool>,
    /// Whether it and its children take input: a disabled item's
    /// `MouseArea`s take no pointer and it takes no focus by Tab.
    pub enabled: Prop<bool>,
    /// Whether it holds the focus within its focus scope. Set through
    /// [`Ui::set_focus`], which takes it from the one that held it.
    pub focus: Prop<bool>,
    /// Whether keys come to it: it, and every focus scope around it,
    /// holds the focus (read only: [`Ui::force_active_focus`]).
    pub active_focus: Prop<bool>,
    /// Whether Tab and Shift+Tab stop at it.
    pub active_focus_on_tab: Prop<bool>,
    /// The name of the state it is in: `""` for its own.
    pub state: Prop<Rc<str>>,
}

/// A `Rectangle`'s own properties.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RectangleProps {
    /// The fill.
    pub color: Prop<Color>,
    /// The corners' radius.
    pub radius: Prop<f32>,
    /// The border's width, inside the edge; 0 for none.
    pub border_width: Prop<f32>,
    /// The border's colour.
    pub border_color: Prop<Color>,
    /// The border's dashes and gaps, this long each; 0 for a solid
    /// border.
    pub border_dash: Prop<f32>,
}

/// A `Text`'s own properties.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TextProps {
    /// The text: one line.
    pub text: Prop<Rc<str>>,
    /// Its colour.
    pub color: Prop<Color>,
    /// Its font.
    pub font: Prop<Font>,
    /// Where it is cut when wider than the item.
    pub elide: Prop<Elide>,
    /// Where it sits across the item.
    pub horizontal_alignment: Prop<HAlign>,
    /// Where it sits down the item.
    pub vertical_alignment: Prop<VAlign>,
}

/// A `TextInput`'s or `TextEdit`'s own properties and signals.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TextInputProps {
    /// The text.
    pub text: Prop<Rc<str>>,
    /// Its colour.
    pub color: Prop<Color>,
    /// Its font.
    pub font: Prop<Font>,
    /// What is under selected text.
    pub selection_color: Prop<Color>,
    /// The caret's colour.
    pub cursor_color: Prop<Color>,
    /// Where the caret is: a byte offset into the text.
    pub cursor_position: Prop<i32>,
    /// The selection's other end; the caret's place when nothing is
    /// selected.
    pub anchor: Prop<i32>,
    /// The selection's first end (read only).
    pub selection_start: Prop<i32>,
    /// Its last end (read only).
    pub selection_end: Prop<i32>,
    /// What is selected (read only).
    pub selected_text: Prop<Rc<str>>,
    /// Whether keys may change it.
    pub read_only: Prop<bool>,
    /// `crate::echo`'s: a password shows dots.
    pub echo_mode: Prop<i32>,
    /// `crate::wrap`'s: whether a `TextEdit`'s lines break at its width.
    pub wrap_mode: Prop<i32>,
    /// Return or Enter in a `TextInput`.
    pub accepted: crate::signal::SignalId,
    /// The text was changed by keys, a paste or a cut.
    pub text_edited: crate::signal::SignalId,
    /// A `TextEdit`: lines, and Return breaks one.
    pub multiline: bool,
}

/// A `Canvas`'s own properties.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CanvasProps {
    /// The Rust function it is painted by.
    pub painter: Prop<crate::shape::CanvasPainter>,
    /// Bumped by [`Ui::update_canvas`]: its data changed.
    pub revision: Prop<i32>,
}

/// A `Shape`'s own properties.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ShapeProps {
    /// Its `ShapePath`s, as the markup binds them.
    pub paths: Prop<crate::shape::ShapePaths>,
}

/// A `Popup`'s own properties.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PopupProps {
    /// `crate::close_policy`'s flags: what closes it.
    pub close_policy: Prop<i32>,
    /// Whether it takes every press while open, and dims the window
    /// behind it.
    pub modal: Prop<bool>,
    /// What a modal one lays over the window behind it.
    pub dim_color: Prop<Color>,
}

/// A `SplitView`'s own properties.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SplitProps {
    /// It places its children as a `RowLayout` (`ColumnLayout` when
    /// vertical) does, `spacing` apart: the handles' width.
    pub flow: FlowProps,
    /// The line drawn in each handle.
    pub handle_color: Prop<Color>,
    /// Whether its children go down rather than across.
    pub vertical: bool,
}

/// A `FolderListModel`'s own properties: a folder's entries as a model.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FolderProps {
    /// The folder; `~` at its start is the home folder.
    pub folder: Prop<Rc<str>>,
    /// Whether names starting with `.` are listed.
    pub show_hidden: Prop<bool>,
    /// Whether files are listed, or folders only.
    pub show_files: Prop<bool>,
    /// The entries shown, folders first, sorted (read only).
    pub files: Prop<crate::model::ModelRc<crate::folder::FileEntry>>,
    /// The folder above, `""` at the top (read only).
    pub parent_folder: Prop<Rc<str>>,
    /// The folder, `~` worked out (read only).
    pub path: Prop<Rc<str>>,
    /// The folder with the home folder written `~` (read only).
    pub display_path: Prop<Rc<str>>,
    /// The folders from `~` or `/` down to it, as breadcrumbs: each
    /// entry's `name` and `path` (read only).
    pub crumbs: Prop<crate::model::ModelRc<crate::folder::FileEntry>>,
    /// The search over names, read as `filterMode` says.
    pub name_filter: Prop<Rc<str>>,
    /// `FolderListModel.Basic`, `Advanced` or `Regex`.
    pub filter_mode: Prop<i32>,
    /// Why the search cannot be read, or `""` (read only); while it
    /// cannot, nothing is listed.
    pub filter_error: Prop<Rc<str>>,
    /// The pattern a file must match to be `accepted`
    /// (`*.png | [image]`); `""` takes every file.
    pub accept: Prop<Rc<str>>,
    /// Why `accept` cannot be read, or `""` (read only); while it cannot,
    /// no file is accepted.
    pub accept_error: Prop<Rc<str>>,
    /// Whether files `accept` does not take are left out.
    pub accepted_only: Prop<bool>,
    /// `FolderListModel.Name`, `Size`, `Time` or `Type`.
    pub sort_field: Prop<i32>,
    /// Whether the sort is turned round (folders stay first).
    pub sort_reversed: Prop<bool>,
    /// Changed to read the folder again.
    pub revision: Prop<i32>,
    /// How many entries are shown (read only).
    pub count: Prop<i32>,
    /// The bytes of the files shown (read only).
    pub total_size: Prop<f32>,
    /// The row chosen, -1 for none: set by the program, as a view's.
    pub current_index: Prop<i32>,
    /// The row at `currentIndex`, or an empty entry (read only).
    pub current: Prop<crate::folder::FileEntry>,
    /// What the folder read gave.
    pub(crate) entries: Prop<crate::folder::Entries>,
    /// What is shown of it.
    pub(crate) rows: Prop<crate::folder::Entries>,
}

/// A `FileInfo`'s own properties: one file, as a preview shows it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FileInfoProps {
    /// The file; `~` at its start is the home folder.
    pub path: Prop<Rc<str>>,
    /// The pattern the file must match, as a `FolderListModel`'s.
    pub accept: Prop<Rc<str>>,
    /// Whether a folder's hidden entries are counted and shown.
    pub show_hidden: Prop<bool>,
    /// Changed to read the file again.
    pub revision: Prop<i32>,
    /// The file as a folder's entry (read only).
    pub entry: Prop<crate::folder::FileEntry>,
    /// Whether it is there (read only).
    pub exists: Prop<bool>,
    /// The folder it is in, `~` written for home (read only).
    pub location: Prop<Rc<str>>,
    /// What a preview shows of it in text (read only): the start of a
    /// text file, a private key's first and last lines with dots for
    /// the rest, a folder's names.
    pub text: Prop<Rc<str>>,
    /// Why `accept` does not take it, or `""` (read only).
    pub refusal: Prop<Rc<str>>,
}

/// An `Image`'s own properties: a picture file drawn.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ImageProps {
    /// The file: PNG, JPEG or SVG; `~` at its start is the home folder.
    pub source: Prop<Rc<str>>,
    /// `Image.Stretch`, `Image.PreserveAspectFit` or
    /// `Image.PreserveAspectCrop`.
    pub fill_mode: Prop<i32>,
    /// `Image.Null` (no source), `Image.Ready` (a picture this draws) or
    /// `Image.Error` (none there, or not PNG, JPEG or SVG) (read only).
    pub status: Prop<i32>,
}

/// A `Row`'s or `Column`'s own properties.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PositionerProps {
    /// Between two children.
    pub spacing: Prop<f32>,
    /// Around them all.
    pub padding: Prop<f32>,
}

/// A `Grid`'s or `GridLayout`'s own properties.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GridProps {
    /// How many columns: 0 for as many as there are children.
    pub columns: Prop<i32>,
    /// Between rows and between columns, unless given apart.
    pub spacing: Prop<f32>,
    /// Between rows; below 0 for `spacing`.
    pub row_spacing: Prop<f32>,
    /// Between columns; below 0 for `spacing`.
    pub column_spacing: Prop<f32>,
    /// Where each child goes across (a `Flow`: both ways), worked out
    /// together.
    pub(crate) solve: Prop<crate::layouts::Solved>,
    /// Where each child goes down, worked out apart from across so a
    /// child's height may follow its width.
    pub(crate) cross: Prop<crate::layouts::Solved>,
}

/// A `Flow`'s, `RowLayout`'s or `ColumnLayout`'s own properties.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FlowProps {
    /// Between two children.
    pub spacing: Prop<f32>,
    /// Where each child goes across (a `Flow`: both ways), worked out
    /// together.
    pub(crate) solve: Prop<crate::layouts::Solved>,
    /// Where each child goes down, worked out apart from across so a
    /// child's height may follow its width.
    pub(crate) cross: Prop<crate::layouts::Solved>,
}

/// A `Repeater`'s own properties.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RepeaterProps {
    /// What it makes a delegate for each row of.
    pub model: Prop<crate::model::ModelSource>,
    /// How many delegates it made.
    pub count: Prop<i32>,
}

/// A `Flickable`'s own properties: what a `ListView` has too.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FlickProps {
    /// How far its content is scrolled across.
    pub content_x: Prop<f32>,
    /// How far down.
    pub content_y: Prop<f32>,
    /// Its content's width; below 0 for its own width.
    pub content_width: Prop<f32>,
    /// Its content's height; below 0 for the bottom of its content's
    /// lowest child. A `ListView` sets it from its rows.
    pub content_height: Prop<f32>,
    /// Whether the wheel scrolls it.
    pub interactive: Prop<bool>,
}

/// A `ListView`'s own properties.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ListViewProps {
    /// It scrolls as a `Flickable`.
    pub flick: FlickProps,
    /// What it makes a delegate for each row of.
    pub model: Prop<crate::model::ModelSource>,
    /// Between two rows.
    pub spacing: Prop<f32>,
    /// How many rows its model has.
    pub count: Prop<i32>,
    /// The row a program or the keys made current; -1 for none.
    pub current_index: Prop<i32>,
    /// How far past its edges delegates are kept made, in logical pixels.
    pub cache_buffer: Prop<f32>,
    /// Whether it keeps its end in view as rows come, as `tail -f` does.
    /// On when it is scrolled to its end (within a row), by whatever
    /// scrolled it, or when set; off when scrolled away from the end,
    /// never because rows came.
    pub follow_end: Prop<bool>,
}

/// What an item is, with its own properties.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// A plain `Item`: geometry and children, nothing drawn.
    Item,
    /// A `Rectangle`.
    Rectangle(RectangleProps),
    /// A `Text`.
    Text(TextProps),
    /// A `Row`: its children side by side.
    Row(PositionerProps),
    /// A `Column`: its children one under another.
    Column(PositionerProps),
    /// A `MouseArea`: where the pointer presses, hovers and turns the
    /// wheel.
    MouseArea(MouseAreaProps),
    /// A `FocusScope`: an `Item` within which one item at a time holds
    /// the focus, kept while the focus is elsewhere.
    FocusScope,
    /// A `Grid`: its children in cells, row by row.
    Grid(GridProps),
    /// A `Flow`: its children side by side, wrapping at its width.
    Flow(FlowProps),
    /// A `RowLayout`: its children side by side, sized by `Layout.*`.
    RowLayout(FlowProps),
    /// A `ColumnLayout`: the same, down.
    ColumnLayout(FlowProps),
    /// A `GridLayout`: in cells, sized by `Layout.*`.
    GridLayout(GridProps),
    /// A `Repeater`: a delegate for each row of its model, made beside
    /// it in its parent.
    Repeater(RepeaterProps),
    /// A `Flickable`: its children on a content item it scrolls.
    Flickable(FlickProps),
    /// A `ListView`: a `Flickable` making delegates only for the rows in
    /// view.
    ListView(ListViewProps),
    /// A `TextInput` or `TextEdit`: text typed into.
    TextInput(TextInputProps),
    /// A `Popup`: drawn over everything and given the pointer first while
    /// shown, outside any cut around it.
    Popup(PopupProps),
    /// A `SplitView`: its children side by side (or one under another),
    /// the boundaries between them dragged.
    SplitView(SplitProps),
    /// A `FolderListModel`: draws nothing, lists a folder.
    FolderListModel(FolderProps),
    /// A `FileInfo`: draws nothing, tells about one file.
    FileInfo(FileInfoProps),
    /// An `Image`: a picture file, drawn.
    Image(ImageProps),
    /// A `Canvas`: painted by a Rust function.
    Canvas(CanvasProps),
    /// A `Shape`: paths of lines and curves, filled and stroked.
    Shape(ShapeProps),
}

/// A `MouseArea`'s own properties and signals.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MouseAreaProps {
    /// Whether it follows the pointer when no button is down.
    pub hover_enabled: Prop<bool>,
    /// Whether a press it took is held.
    pub pressed: Prop<bool>,
    /// Whether the pointer is over it (while hovering is enabled, or
    /// while pressed).
    pub contains_mouse: Prop<bool>,
    /// The pointer's place across it.
    pub mouse_x: Prop<f32>,
    /// The pointer's place down it.
    pub mouse_y: Prop<f32>,
    /// A press and a release on it.
    pub clicked: crate::signal::SignalId,
    /// A press.
    pub on_pressed: crate::signal::SignalId,
    /// The release of a press it took.
    pub released: crate::signal::SignalId,
    /// The pointer came over it.
    pub entered: crate::signal::SignalId,
    /// The pointer left it.
    pub exited: crate::signal::SignalId,
    /// The wheel turned over it: the vertical delta, in logical pixels,
    /// positive down.
    pub wheel: crate::signal::SignalId,
    /// A second press soon after one, in the same place.
    pub double_clicked: crate::signal::SignalId,
    /// The pointer moved while a press it took is held.
    pub position_changed: crate::signal::SignalId,
    /// The pointer's shape over it: `crate::cursor_shape`'s.
    pub cursor_shape: Prop<i32>,
}

/// The kinds, to make one.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum KindName {
    /// [`Kind::Item`].
    Item,
    /// [`Kind::Rectangle`].
    Rectangle,
    /// [`Kind::Text`].
    Text,
    /// [`Kind::Row`].
    Row,
    /// [`Kind::Column`].
    Column,
    /// [`Kind::MouseArea`].
    MouseArea,
    /// [`Kind::FocusScope`].
    FocusScope,
    /// [`Kind::Grid`].
    Grid,
    /// [`Kind::Flow`].
    Flow,
    /// [`Kind::RowLayout`].
    RowLayout,
    /// [`Kind::ColumnLayout`].
    ColumnLayout,
    /// [`Kind::GridLayout`].
    GridLayout,
    /// [`Kind::Repeater`].
    Repeater,
    /// [`Kind::Flickable`].
    Flickable,
    /// [`Kind::ListView`].
    ListView,
    /// [`Kind::TextInput`], one line.
    TextInput,
    /// [`Kind::TextInput`], lines.
    TextEdit,
    /// [`Kind::Popup`].
    Popup,
    /// [`Kind::SplitView`], across.
    SplitView,
    /// [`Kind::SplitView`], down.
    SplitViewVertical,
    /// [`Kind::FolderListModel`].
    FolderListModel,
    /// [`Kind::FileInfo`].
    FileInfo,
    /// [`Kind::Image`].
    Image,
    /// [`Kind::Canvas`].
    Canvas,
    /// [`Kind::Shape`].
    Shape,
}

/// One item.
#[derive(Debug)]
pub(crate) struct ItemData {
    /// Its kind and own properties.
    pub(crate) kind: Kind,
    /// Its common properties.
    pub(crate) common: Common,
    /// Its parent.
    pub(crate) parent: Option<ItemId>,
    /// Its children, in paint order.
    pub(crate) children: Vec<ItemId>,
    /// Properties and signals a component declared on it, freed with it.
    pub(crate) owned: Vec<Owned>,
    /// For a focus scope (or a root): which item within it holds the
    /// focus.
    pub(crate) focus_child: Option<ItemId>,
    /// Its `Keys` and `KeyNavigation`, once the markup gave it either.
    pub(crate) keys: Option<Box<crate::focus::KeysAttached>>,
    /// Its `Layout.*`, once the markup gave any.
    pub(crate) layout: Option<crate::layouts::LayoutProps>,
    /// A `Flickable`'s or `ListView`'s content item: where its children
    /// go.
    pub(crate) content: Option<ItemId>,
    /// A `ListView`'s rows and delegates.
    pub(crate) view: Option<Rc<std::cell::RefCell<crate::view::ViewState>>>,
    /// How far a `TextInput`'s text is scrolled across to keep the caret
    /// in view, in logical pixels.
    pub(crate) text_scroll: f32,
}

/// What an item holds for a component.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Owned {
    /// A declared property.
    Prop(crate::ui::PropId),
    /// A declared signal.
    Signal(crate::signal::SignalId),
    /// An item it made beside itself (a `Repeater`'s delegates), which
    /// goes with it.
    Item(ItemId),
}

/// A place in the arena.
#[derive(Debug)]
pub(crate) struct Slot {
    /// Bumped each time the slot is freed.
    generation: u32,
    /// The item, while it lives.
    pub(crate) item: Option<ItemData>,
}

impl Ui {
    /// A new item of `kind` under `parent`, or a root when `None`. Answers
    /// `None` only when `parent` is gone.
    pub fn create(&mut self, kind: KindName, parent: Option<ItemId>) -> Option<ItemId> {
        if let Some(parent) = parent
            && self.item(parent).is_none()
        {
            return None;
        }
        // A Flickable's children go on its content item.
        let parent = parent.map(|p| self.item(p).and_then(|data| data.content).unwrap_or(p));
        let common = Common {
            x: self.prop(0.0_f32),
            y: self.prop(0.0_f32),
            width: self.prop(0.0_f32),
            height: self.prop(0.0_f32),
            implicit_width: self.prop(0.0_f32),
            implicit_height: self.prop(0.0_f32),
            visible: self.prop(true),
            opacity: self.prop(1.0_f32),
            clip: self.prop(false),
            enabled: self.prop(true),
            focus: self.prop(false),
            active_focus: self.prop(false),
            active_focus_on_tab: self.prop(false),
            state: self.prop(Rc::<str>::from("")),
        };
        let (implicit_width, implicit_height) = (common.implicit_width, common.implicit_height);
        common.width.bind(self, move |ui| ui.get(implicit_width));
        common.height.bind(self, move |ui| ui.get(implicit_height));
        let kind = match kind {
            KindName::Item => Kind::Item,
            KindName::FocusScope => Kind::FocusScope,
            KindName::Grid | KindName::GridLayout => {
                let layout = kind == KindName::GridLayout;
                let props = GridProps {
                    columns: self.prop(if layout { 0 } else { 4 }),
                    spacing: self.prop(if layout { 5.0_f32 } else { 0.0 }),
                    row_spacing: self.prop(-1.0_f32),
                    column_spacing: self.prop(-1.0_f32),
                    solve: self.prop(crate::layouts::Solved::default()),
                    cross: self.prop(crate::layouts::Solved::default()),
                };
                if layout {
                    Kind::GridLayout(props)
                } else {
                    Kind::Grid(props)
                }
            }
            KindName::Flow | KindName::RowLayout | KindName::ColumnLayout => {
                let props = FlowProps {
                    spacing: self.prop(if kind == KindName::Flow { 0.0_f32 } else { 5.0 }),
                    solve: self.prop(crate::layouts::Solved::default()),
                    cross: self.prop(crate::layouts::Solved::default()),
                };
                match kind {
                    KindName::Flow => Kind::Flow(props),
                    KindName::RowLayout => Kind::RowLayout(props),
                    _ => Kind::ColumnLayout(props),
                }
            }
            KindName::Flickable => Kind::Flickable(self.flick_props()),
            KindName::ListView => Kind::ListView(ListViewProps {
                flick: self.flick_props(),
                model: self.prop(crate::model::ModelSource::default()),
                spacing: self.prop(0.0_f32),
                count: self.prop(0_i32),
                current_index: self.prop(-1_i32),
                cache_buffer: self.prop(0.0_f32),
                follow_end: self.prop(false),
            }),
            KindName::Repeater => Kind::Repeater(RepeaterProps {
                model: self.prop(crate::model::ModelSource::default()),
                count: self.prop(0_i32),
            }),
            KindName::Rectangle => Kind::Rectangle(RectangleProps {
                color: self.prop(Color::hex(0xff_ff_ff)),
                radius: self.prop(0.0_f32),
                border_width: self.prop(0.0_f32),
                border_color: self.prop(Color::hex(0)),
                border_dash: self.prop(0.0_f32),
            }),
            KindName::Text => {
                let props = TextProps {
                    text: self.prop(Rc::<str>::from("")),
                    color: self.prop(Color::hex(0)),
                    font: self.prop(Font::default()),
                    elide: self.prop(Elide::None),
                    horizontal_alignment: self.prop(HAlign::Left),
                    vertical_alignment: self.prop(VAlign::Center),
                };
                implicit_width.bind(self, move |ui| {
                    let (text, font, scale) = (ui.get(props.text), ui.get(props.font), ui.scale());
                    ui.text.run(&font, scale, &text).width / scale
                });
                implicit_height.bind(self, move |ui| {
                    let (font, scale) = (ui.get(props.font), ui.scale());
                    let (ascent, descent) = ui.text.extent(&font, scale);
                    (ascent + descent) / scale
                });
                Kind::Text(props)
            }
            KindName::Popup => {
                common.visible.set(self, false);
                Kind::Popup(PopupProps {
                    close_policy: self.prop(crate::popup::close_policy::DEFAULT),
                    modal: self.prop(false),
                    dim_color: self.prop(Color::rgba(0, 0, 0, 72)),
                })
            }
            KindName::SplitView | KindName::SplitViewVertical => Kind::SplitView(SplitProps {
                flow: FlowProps {
                    spacing: self.prop(1.0_f32),
                    solve: self.prop(crate::layouts::Solved::default()),
                    cross: self.prop(crate::layouts::Solved::default()),
                },
                handle_color: self.prop(Color::rgba(0, 0, 0, 40)),
                vertical: kind == KindName::SplitViewVertical,
            }),
            KindName::FolderListModel => Kind::FolderListModel(self.folder_props()),
            KindName::FileInfo => Kind::FileInfo(self.file_info_props()),
            KindName::Image => Kind::Image(self.image_props()),
            KindName::Canvas => Kind::Canvas(CanvasProps {
                painter: self.prop(crate::shape::CanvasPainter::default()),
                revision: self.prop(0_i32),
            }),
            KindName::Shape => {
                let props = ShapeProps {
                    paths: self.prop(crate::shape::ShapePaths::default()),
                };
                implicit_width.bind(self, move |ui| ui.shape_extent(props).0);
                implicit_height.bind(self, move |ui| ui.shape_extent(props).1);
                Kind::Shape(props)
            }
            KindName::TextInput | KindName::TextEdit => {
                let text = self.prop(Rc::<str>::from(""));
                let cursor_position = self.prop(0_i32);
                let anchor = self.prop(0_i32);
                let props = TextInputProps {
                    text,
                    color: self.prop(Color::hex(0)),
                    font: self.prop(Font::default()),
                    selection_color: self.prop(Color::rgba(42, 98, 201, 64)),
                    cursor_color: self.prop(Color::hex(0)),
                    cursor_position,
                    anchor,
                    selection_start: self.prop(0_i32),
                    selection_end: self.prop(0_i32),
                    selected_text: self.prop(Rc::<str>::from("")),
                    read_only: self.prop(false),
                    echo_mode: self.prop(0_i32),
                    wrap_mode: self.prop(0_i32),
                    accepted: self.new_signal(),
                    text_edited: self.new_signal(),
                    multiline: kind == KindName::TextEdit,
                };
                props
                    .selection_start
                    .bind(self, move |ui| ui.get(cursor_position).min(ui.get(anchor)));
                props
                    .selection_end
                    .bind(self, move |ui| ui.get(cursor_position).max(ui.get(anchor)));
                props.selected_text.bind(self, move |ui| {
                    let text = ui.get(text);
                    let at = |v: i32| usize::try_from(v).unwrap_or(0).min(text.len());
                    let (a, b) = (at(ui.get(cursor_position)), at(ui.get(anchor)));
                    Rc::from(text.get(a.min(b)..a.max(b)).unwrap_or(""))
                });
                implicit_width.bind(self, move |ui| ui.text_input_implicit(props, None).0);
                let width = common.width;
                implicit_height.bind(self, move |ui| {
                    let wraps = props.multiline && ui.get(props.wrap_mode) != 0;
                    let at = if wraps { Some(ui.get(width)) } else { None };
                    ui.text_input_implicit(props, at).1
                });
                Kind::TextInput(props)
            }
            KindName::Row | KindName::Column => {
                let props = PositionerProps {
                    spacing: self.prop(0.0_f32),
                    padding: self.prop(0.0_f32),
                };
                if kind == KindName::Row {
                    Kind::Row(props)
                } else {
                    Kind::Column(props)
                }
            }
            KindName::MouseArea => Kind::MouseArea(MouseAreaProps {
                hover_enabled: self.prop(false),
                pressed: self.prop(false),
                contains_mouse: self.prop(false),
                mouse_x: self.prop(0.0_f32),
                mouse_y: self.prop(0.0_f32),
                clicked: self.new_signal(),
                on_pressed: self.new_signal(),
                released: self.new_signal(),
                entered: self.new_signal(),
                exited: self.new_signal(),
                wheel: self.new_signal(),
                double_clicked: self.new_signal(),
                position_changed: self.new_signal(),
                cursor_shape: self.prop(0_i32),
            }),
        };
        let data = ItemData {
            kind,
            common,
            parent,
            children: Vec::new(),
            owned: Vec::new(),
            focus_child: None,
            keys: None,
            layout: None,
            content: None,
            view: None,
            text_scroll: 0.0,
        };
        let id = self.place(data);
        match parent {
            Some(parent) => {
                if let Some(item) = self.item_mut(parent) {
                    item.children.push(id);
                }
                self.position_children(parent);
            }
            None => self.roots.push(id),
        }
        if matches!(kind, Kind::Flickable(_) | Kind::ListView(_)) {
            self.make_content(id);
        }
        Some(id)
    }

    /// Destroy `item` and everything under it.
    pub fn destroy(&mut self, item: ItemId) {
        let Some(data) = self.item(item) else {
            return;
        };
        let parent = data.parent;
        let mut doomed = vec![item];
        let mut at = 0;
        while let Some(&next) = doomed.get(at) {
            if let Some(data) = self.item(next) {
                doomed.extend(data.children.iter().copied());
                for owned in &data.owned {
                    if let Owned::Item(made) = *owned
                        && !doomed.contains(&made)
                    {
                        doomed.push(made);
                    }
                }
            }
            at += 1;
        }
        let mut parents: Vec<ItemId> = Vec::new();
        for id in &doomed {
            if let Some(above) = self.parent(*id)
                && !parents.contains(&above)
            {
                parents.push(above);
            }
        }
        for id in doomed {
            let Some(data) = self.slot_mut(id).and_then(|slot| {
                slot.generation = slot.generation.wrapping_add(1);
                slot.item.take()
            }) else {
                continue;
            };
            for prop in props_of(&data) {
                self.free(prop);
            }
            for owned in &data.owned {
                match *owned {
                    Owned::Prop(prop) => self.free(prop),
                    Owned::Signal(signal) => self.free_signal(signal),
                    Owned::Item(_) => {}
                }
            }
            if let Some(layout) = data.layout {
                for prop in layout.props() {
                    self.free(prop);
                }
            }
            if let Kind::MouseArea(m) = data.kind {
                for signal in [
                    m.clicked,
                    m.on_pressed,
                    m.released,
                    m.entered,
                    m.exited,
                    m.wheel,
                    m.double_clicked,
                    m.position_changed,
                ] {
                    self.free_signal(signal);
                }
            }
            if let Some(keys) = &data.keys {
                for signal in keys.signals() {
                    self.free_signal(signal);
                }
            }
            if let Kind::TextInput(t) = data.kind {
                self.free_signal(t.accepted);
                self.free_signal(t.text_edited);
            }
            self.forget_input(id);
            self.free_items.push(id.index);
        }
        if parent.is_none() {
            self.roots.retain(|&root| root != item);
        }
        for above in parents {
            let Some(data) = self.item(above) else {
                continue;
            };
            let kept: Vec<ItemId> = data
                .children
                .iter()
                .copied()
                .filter(|&child| self.item(child).is_some())
                .collect();
            if let Some(data) = self.item_mut(above) {
                data.children = kept;
            }
            self.position_children(above);
        }
        self.forget_focus();
    }

    /// Whether `item` lives.
    #[must_use]
    pub fn exists(&self, item: ItemId) -> bool {
        self.item(item).is_some()
    }

    /// `item`'s kind and own properties.
    #[must_use]
    pub fn kind(&self, item: ItemId) -> Option<Kind> {
        self.item(item).map(|data| data.kind)
    }

    /// `item`'s common properties.
    #[must_use]
    pub fn common(&self, item: ItemId) -> Option<Common> {
        self.item(item).map(|data| data.common)
    }

    /// A `Rectangle`'s own properties.
    #[must_use]
    pub fn rectangle(&self, item: ItemId) -> Option<RectangleProps> {
        match self.kind(item)? {
            Kind::Rectangle(props) => Some(props),
            _ => None,
        }
    }

    /// A `Text`'s own properties.
    #[must_use]
    pub fn text(&self, item: ItemId) -> Option<TextProps> {
        match self.kind(item)? {
            Kind::Text(props) => Some(props),
            _ => None,
        }
    }

    /// A `MouseArea`'s own properties and signals.
    #[must_use]
    pub fn mouse_area(&self, item: ItemId) -> Option<MouseAreaProps> {
        match self.kind(item)? {
            Kind::MouseArea(props) => Some(props),
            _ => None,
        }
    }

    /// A property a component declared on `item`: made here, and freed
    /// with the item.
    pub fn own_prop<T: crate::PropType>(&mut self, item: ItemId, value: T) -> Prop<T> {
        let prop = self.prop(value);
        match self.item_mut(item) {
            Some(data) => data.owned.push(Owned::Prop(prop.id)),
            None => self.free(prop.id),
        }
        prop
    }

    /// A signal a component declared on `item`, freed with the item.
    pub fn own_signal(&mut self, item: ItemId) -> crate::signal::SignalId {
        let signal = self.new_signal();
        match self.item_mut(item) {
            Some(data) => data.owned.push(Owned::Signal(signal)),
            None => self.free_signal(signal),
        }
        signal
    }

    /// A `Grid`'s or `GridLayout`'s own properties.
    #[must_use]
    pub fn grid(&self, item: ItemId) -> Option<GridProps> {
        match self.kind(item)? {
            Kind::Grid(props) | Kind::GridLayout(props) => Some(props),
            _ => None,
        }
    }

    /// A `Flow`'s, `RowLayout`'s or `ColumnLayout`'s own properties.
    #[must_use]
    pub fn flow(&self, item: ItemId) -> Option<FlowProps> {
        match self.kind(item)? {
            Kind::Flow(props) | Kind::RowLayout(props) | Kind::ColumnLayout(props) => Some(props),
            _ => None,
        }
    }

    /// A `Repeater`'s own properties.
    #[must_use]
    pub fn repeater(&self, item: ItemId) -> Option<RepeaterProps> {
        match self.kind(item)? {
            Kind::Repeater(props) => Some(props),
            _ => None,
        }
    }

    /// A `Flickable`'s or `ListView`'s scrolling properties.
    #[must_use]
    pub fn flickable(&self, item: ItemId) -> Option<FlickProps> {
        match self.kind(item)? {
            Kind::Flickable(props) => Some(props),
            Kind::ListView(props) => Some(props.flick),
            _ => None,
        }
    }

    /// A `ListView`'s own properties.
    #[must_use]
    pub fn list_view(&self, item: ItemId) -> Option<ListViewProps> {
        match self.kind(item)? {
            Kind::ListView(props) => Some(props),
            _ => None,
        }
    }

    /// A `Flickable`'s or `ListView`'s content item.
    #[must_use]
    pub fn content_item(&self, item: ItemId) -> Option<ItemId> {
        self.item(item)?.content
    }

    /// Make the cell `prop` go with `owner`.
    pub(crate) fn own_cell(&mut self, owner: ItemId, prop: crate::ui::PropId) {
        match self.item_mut(owner) {
            Some(data) => data.owned.push(Owned::Prop(prop)),
            None => self.free(prop),
        }
    }

    /// Make `item` go with `owner` when `owner` is destroyed.
    pub(crate) fn own_item(&mut self, owner: ItemId, item: ItemId) {
        if let Some(data) = self.item_mut(owner) {
            data.owned.push(Owned::Item(item));
        }
    }

    /// Put `items`, children of `anchor`'s parent, straight after
    /// `anchor` in this order, and place them again.
    pub(crate) fn place_after(&mut self, anchor: ItemId, items: &[ItemId]) {
        let Some(parent) = self.parent(anchor) else {
            return;
        };
        let Some(data) = self.item_mut(parent) else {
            return;
        };
        let mut rest: Vec<ItemId> = data
            .children
            .iter()
            .copied()
            .filter(|c| !items.contains(c))
            .collect();
        let at = rest
            .iter()
            .position(|&c| c == anchor)
            .map_or(rest.len(), |p| p + 1);
        let tail = rest.split_off(at);
        rest.extend_from_slice(items);
        rest.extend(tail);
        if data.children == rest {
            return;
        }
        data.children = rest;
        self.position_children(parent);
    }

    /// Put `child` at `index` among its parent's children, and place
    /// them again.
    pub fn move_child(&mut self, child: ItemId, index: usize) {
        let Some(parent) = self.parent(child) else {
            return;
        };
        if let Some(data) = self.item_mut(parent) {
            data.children.retain(|&c| c != child);
            let at = index.min(data.children.len());
            data.children.insert(at, child);
        }
        self.position_children(parent);
    }

    /// A `Row`'s or `Column`'s own properties.
    #[must_use]
    pub fn positioner(&self, item: ItemId) -> Option<PositionerProps> {
        match self.kind(item)? {
            Kind::Row(props) | Kind::Column(props) => Some(props),
            _ => None,
        }
    }

    /// `item`'s parent.
    #[must_use]
    pub fn parent(&self, item: ItemId) -> Option<ItemId> {
        self.item(item)?.parent
    }

    /// `item`'s children, in paint order.
    #[must_use]
    pub fn children(&self, item: ItemId) -> &[ItemId] {
        self.item(item).map_or(&[], |data| data.children.as_slice())
    }

    /// The items with no parent.
    #[must_use]
    pub fn roots(&self) -> &[ItemId] {
        &self.roots
    }

    /// The live item `id` names.
    pub(crate) fn item(&self, id: ItemId) -> Option<&ItemData> {
        self.items
            .get(id.index as usize)
            .filter(|slot| slot.generation == id.generation)
            .and_then(|slot| slot.item.as_ref())
    }

    /// The same, to change.
    pub(crate) fn item_mut(&mut self, id: ItemId) -> Option<&mut ItemData> {
        self.items
            .get_mut(id.index as usize)
            .filter(|slot| slot.generation == id.generation)
            .and_then(|slot| slot.item.as_mut())
    }

    /// The slot `id` names, live or not, if its generation matches.
    fn slot_mut(&mut self, id: ItemId) -> Option<&mut Slot> {
        self.items
            .get_mut(id.index as usize)
            .filter(|slot| slot.generation == id.generation)
    }

    /// Put `data` in a free slot.
    fn place(&mut self, data: ItemData) -> ItemId {
        if let Some(index) = self.free_items.pop()
            && let Some(slot) = self.items.get_mut(index as usize)
        {
            slot.item = Some(data);
            return ItemId {
                index,
                generation: slot.generation,
            };
        }
        let index = u32::try_from(self.items.len()).unwrap_or(u32::MAX);
        self.items.push(Slot {
            generation: 0,
            item: Some(data),
        });
        ItemId {
            index,
            generation: 0,
        }
    }
}

/// Every property cell an item holds.
fn props_of(data: &ItemData) -> Vec<crate::ui::PropId> {
    let c = data.common;
    let mut ids = vec![
        c.x.id,
        c.y.id,
        c.width.id,
        c.height.id,
        c.implicit_width.id,
        c.implicit_height.id,
        c.visible.id,
        c.opacity.id,
        c.clip.id,
        c.enabled.id,
        c.focus.id,
        c.active_focus.id,
        c.active_focus_on_tab.id,
        c.state.id,
    ];
    match data.kind {
        Kind::Item | Kind::FocusScope => {}
        Kind::Rectangle(p) => ids.extend([
            p.color.id,
            p.radius.id,
            p.border_width.id,
            p.border_color.id,
            p.border_dash.id,
        ]),
        Kind::Text(p) => ids.extend([
            p.text.id,
            p.color.id,
            p.font.id,
            p.elide.id,
            p.horizontal_alignment.id,
            p.vertical_alignment.id,
        ]),
        Kind::Row(p) | Kind::Column(p) => ids.extend([p.spacing.id, p.padding.id]),
        Kind::Grid(p) | Kind::GridLayout(p) => ids.extend([
            p.columns.id,
            p.spacing.id,
            p.row_spacing.id,
            p.column_spacing.id,
            p.solve.id,
            p.cross.id,
        ]),
        Kind::Flow(p) | Kind::RowLayout(p) | Kind::ColumnLayout(p) => {
            ids.extend([p.spacing.id, p.solve.id, p.cross.id]);
        }
        Kind::Repeater(p) => ids.extend([p.model.id, p.count.id]),
        Kind::Flickable(f) => ids.extend(f.props()),
        Kind::ListView(p) => {
            ids.extend(p.flick.props());
            ids.extend([
                p.model.id,
                p.spacing.id,
                p.count.id,
                p.current_index.id,
                p.cache_buffer.id,
                p.follow_end.id,
            ]);
        }
        Kind::Popup(p) => ids.extend([p.close_policy.id, p.modal.id, p.dim_color.id]),
        Kind::Canvas(p) => ids.extend([p.painter.id, p.revision.id]),
        Kind::Shape(p) => ids.push(p.paths.id),
        Kind::SplitView(p) => ids.extend([
            p.flow.spacing.id,
            p.flow.solve.id,
            p.flow.cross.id,
            p.handle_color.id,
        ]),
        Kind::FolderListModel(p) => ids.extend([
            p.folder.id,
            p.show_hidden.id,
            p.show_files.id,
            p.files.id,
            p.parent_folder.id,
            p.path.id,
            p.display_path.id,
            p.crumbs.id,
            p.name_filter.id,
            p.filter_mode.id,
            p.filter_error.id,
            p.accept.id,
            p.accept_error.id,
            p.accepted_only.id,
            p.sort_field.id,
            p.sort_reversed.id,
            p.revision.id,
            p.count.id,
            p.total_size.id,
            p.current_index.id,
            p.current.id,
            p.entries.id,
            p.rows.id,
        ]),
        Kind::FileInfo(p) => ids.extend([
            p.path.id,
            p.accept.id,
            p.show_hidden.id,
            p.revision.id,
            p.entry.id,
            p.exists.id,
            p.location.id,
            p.text.id,
            p.refusal.id,
        ]),
        Kind::Image(p) => ids.extend([p.source.id, p.fill_mode.id, p.status.id]),
        Kind::TextInput(p) => ids.extend([
            p.text.id,
            p.color.id,
            p.font.id,
            p.selection_color.id,
            p.cursor_color.id,
            p.cursor_position.id,
            p.anchor.id,
            p.selection_start.id,
            p.selection_end.id,
            p.selected_text.id,
            p.read_only.id,
            p.echo_mode.id,
            p.wrap_mode.id,
        ]),
        Kind::MouseArea(p) => ids.extend([
            p.hover_enabled.id,
            p.pressed.id,
            p.contains_mouse.id,
            p.mouse_x.id,
            p.mouse_y.id,
            p.cursor_shape.id,
        ]),
    }
    ids
}
