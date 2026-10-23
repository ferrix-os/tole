//! Typed handles for the kinds of item: what the generated code (and a
//! program building a tree by hand) holds for each item it made.
//!
//! Each is `Copy`, so a binding closure captures the handles it reads by
//! value.

use crate::item::{Common, ItemId, KindName, PositionerProps, RectangleProps, TextProps};
use crate::ui::Ui;

/// What every handle, and every component the compiler generates, is.
pub trait Element: Copy {
    /// The item.
    fn id(&self) -> ItemId;
    /// Its common properties.
    fn common(&self) -> Common;
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
    /// A `Column`.
    Column, Column, PositionerProps, positioner
);

/// A plain `Item`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Item {
    /// The item.
    pub id: ItemId,
    /// Its common properties.
    pub common: Common,
}

impl Item {
    /// A new one under `parent`, or a root.
    pub fn new(ui: &mut Ui, parent: Option<ItemId>) -> Option<Self> {
        let id = ui.create(KindName::Item, parent)?;
        Some(Item {
            id,
            common: ui.common(id)?,
        })
    }
}

impl Element for Item {
    fn id(&self) -> ItemId {
        self.id
    }
    fn common(&self) -> Common {
        self.common
    }
}
