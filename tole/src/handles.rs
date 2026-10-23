//! Typed handles for the kinds of item: what the generated code (and a
//! program building a tree by hand) holds for each item it made.
//!
//! Each is `Copy`, so a binding closure captures the handles it reads by
//! value.

use crate::item::{CanvasProps, FolderProps, PopupProps, ShapeProps, SplitProps};
use crate::item::{
    Common, FlickProps, FlowProps, GridProps, ItemId, KindName, ListViewProps, MouseAreaProps,
    PositionerProps, RectangleProps, RepeaterProps, TextInputProps, TextProps,
};
use crate::ui::Ui;

/// What every handle, and every component the compiler generates, is.
pub trait Element: Copy {
    /// The item.
    fn id(&self) -> ItemId;
    /// Its common properties.
    fn common(&self) -> Common;
    /// Where the children a use gives it go: the item itself, or for a
    /// component with a `default property alias`, the item that names.
    fn content(&self) -> ItemId {
        self.id()
    }
}

/// A handle with its kind's own properties.
macro_rules! handle {
    ($(#[$doc:meta])* $name:ident, $kind:ident, $props:ty, $get:ident) => {
        $(#[$doc])*
        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        pub struct $name {
            /// The item.
            pub id: ItemId,
            /// Its common properties.
            pub common: Common,
            /// Its own.
            pub props: $props,
        }

        impl $name {
            /// A new one under `parent`, or a root.
            pub fn new(ui: &mut Ui, parent: Option<ItemId>) -> Option<Self> {
                let id = ui.create(KindName::$kind, parent)?;
                Some($name { id, common: ui.common(id)?, props: ui.$get(id)? })
            }
        }

        impl Element for $name {
            fn id(&self) -> ItemId {
                self.id
            }
            fn common(&self) -> Common {
                self.common
            }
        }
    };
}

handle!(
    /// A `Rectangle`.
    Rectangle, Rectangle, RectangleProps, rectangle
);
handle!(
    /// A `Text`.
    Text, Text, TextProps, text
);
handle!(
    /// A `Row`.
    Row, Row, PositionerProps, positioner
);
handle!(
    /// A `MouseArea`.
    MouseArea, MouseArea, MouseAreaProps, mouse_area
);
handle!(
    /// A `Column`.
    Column, Column, PositionerProps, positioner
);

handle!(
    /// A `Grid`.
    Grid, Grid, GridProps, grid
);
handle!(
    /// A `GridLayout`.
    GridLayout, GridLayout, GridProps, grid
);
handle!(
    /// A `Flow`.
    Flow, Flow, FlowProps, flow
);
handle!(
    /// A `RowLayout`.
    RowLayout, RowLayout, FlowProps, flow
);
handle!(
    /// A `ColumnLayout`.
    ColumnLayout, ColumnLayout, FlowProps, flow
);
handle!(
    /// A `Repeater`.
    Repeater, Repeater, RepeaterProps, repeater
);

handle!(
    /// A `Canvas`.
    Canvas, Canvas, CanvasProps, canvas
);
handle!(
    /// A `Shape`.
    Shape, Shape, ShapeProps, shape
);
handle!(
    /// A `Popup`.
    Popup, Popup, PopupProps, popup
);
handle!(
    /// A `SplitView`, its children across.
    SplitView, SplitView, SplitProps, split
);
handle!(
    /// A `SplitView`, its children down.
    SplitViewVertical, SplitViewVertical, SplitProps, split
);
handle!(
    /// A `FolderListModel`.
    FolderListModel, FolderListModel, FolderProps, folder_list
);
handle!(
    /// A `TextInput`: one line typed into.
    TextInput, TextInput, TextInputProps, text_input
);
handle!(
    /// A `TextEdit`: lines typed into.
    TextEdit, TextEdit, TextInputProps, text_input
);

handle!(
    /// A `Flickable`.
    Flickable, Flickable, FlickProps, flickable
);
handle!(
    /// A `ListView`.
    ListView, ListView, ListViewProps, list_view
);

/// A handle with no properties of its kind's own.
macro_rules! plain_handle {
    ($(#[$doc:meta])* $name:ident) => {
        $(#[$doc])*
        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        pub struct $name {
            /// The item.
            pub id: ItemId,
            /// Its common properties.
            pub common: Common,
        }

        impl $name {
            /// A new one under `parent`, or a root.
            pub fn new(ui: &mut Ui, parent: Option<ItemId>) -> Option<Self> {
                let id = ui.create(KindName::$name, parent)?;
                Some($name { id, common: ui.common(id)? })
            }
        }

        impl Element for $name {
            fn id(&self) -> ItemId {
                self.id
            }
            fn common(&self) -> Common {
                self.common
            }
        }
    };
}

plain_handle!(
    /// A plain `Item`.
    Item
);
plain_handle!(
    /// A `FocusScope`.
    FocusScope
);
