//! The types the markup knows: values, and the kinds of item with their
//! properties as the runtime (`tole`) names them.

use std::collections::BTreeMap;

/// A value's type.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Ty {
    /// `bool`.
    Bool,
    /// `int`.
    Int,
    /// `real`, `length`.
    Real,
    /// `string`.
    Str,
    /// `color`.
    Color,
    /// `font`.
    Font,
    /// `Text.elide`'s.
    Elide,
    /// `Text.horizontalAlignment`'s.
    HAlign,
    /// `Text.verticalAlignment`'s.
    VAlign,
    /// A declared `enum`.
    Enum(String),
    /// A declared `struct`.
    Struct(String),
    /// A key, as `Keys` handlers are given it.
    KeyEvent,
    /// `list<T>`: a small list of values.
    List(Box<Ty>),
    /// `model<T>`: a program's model of rows.
    Model(Box<Ty>),
    /// What a `Repeater` or view takes as its `model`: a count, a list or
    /// a model, made into one when bound.
    ModelSource,
}

impl Ty {
    /// The type a markup name stands for, given the declared types.
    pub(crate) fn named(
        name: &str,
        structs: &BTreeMap<String, Vec<(String, Ty)>>,
        enums: &BTreeMap<String, Vec<String>>,
    ) -> Option<Ty> {
        Some(match name {
            "bool" => Ty::Bool,
            "int" => Ty::Int,
            "real" | "length" | "double" => Ty::Real,
            "string" => Ty::Str,
            "color" => Ty::Color,
            "font" => Ty::Font,
            "KeyEvent" => Ty::KeyEvent,
            other if other.starts_with("list<") && other.ends_with('>') => {
                let inner = other.get(5..other.len() - 1)?;
                Ty::List(Box::new(Ty::named(inner, structs, enums)?))
            }
            other if other.starts_with("model<") && other.ends_with('>') => {
                let inner = other.get(6..other.len() - 1)?;
                Ty::Model(Box::new(Ty::named(inner, structs, enums)?))
            }
            other if structs.contains_key(other) => Ty::Struct(other.to_owned()),
            other if enums.contains_key(other) => Ty::Enum(other.to_owned()),
            _ => return None,
        })
    }

    /// Its Rust type.
    pub(crate) fn rust(&self) -> String {
        match self {
            Ty::Bool => "bool".to_owned(),
            Ty::Int => "i32".to_owned(),
            Ty::Real => "f32".to_owned(),
            Ty::Str => "::std::rc::Rc<str>".to_owned(),
            Ty::Color => "::tole::Color".to_owned(),
            Ty::Font => "::tole::Font".to_owned(),
            Ty::Elide => "::tole::Elide".to_owned(),
            Ty::HAlign => "::tole::HAlign".to_owned(),
            Ty::VAlign => "::tole::VAlign".to_owned(),
            Ty::KeyEvent => "::tole::KeyEvent".to_owned(),
            Ty::List(inner) => format!("::std::vec::Vec<{}>", inner.rust()),
            Ty::Model(inner) => format!("::tole::ModelRc<{}>", inner.rust()),
            Ty::ModelSource => "::tole::ModelSource".to_owned(),
            Ty::Enum(name) | Ty::Struct(name) => name.clone(),
        }
    }

    /// How the markup writes it, for diagnostics.
    pub(crate) fn name(&self) -> String {
        match self {
            Ty::Bool => "bool".to_owned(),
            Ty::Int => "int".to_owned(),
            Ty::Real => "real".to_owned(),
            Ty::Str => "string".to_owned(),
            Ty::Color => "color".to_owned(),
            Ty::Font => "font".to_owned(),
            Ty::Elide => "Text.elide".to_owned(),
            Ty::HAlign => "Text.horizontalAlignment".to_owned(),
            Ty::VAlign => "Text.verticalAlignment".to_owned(),
            Ty::KeyEvent => "KeyEvent".to_owned(),
            Ty::List(inner) => format!("list<{}>", inner.name()),
            Ty::Model(inner) => format!("model<{}>", inner.name()),
            Ty::ModelSource => "count, list or model".to_owned(),
            Ty::Enum(name) | Ty::Struct(name) => name.clone(),
        }
    }

    /// A field of a built-in struct: its Rust name and type.
    pub(crate) fn builtin_field(&self, field: &str) -> Option<(&'static str, Ty)> {
        match (self, field) {
            (Ty::KeyEvent, "key") => Some(("key", Ty::Str)),
            (Ty::KeyEvent, "text") => Some(("text", Ty::Str)),
            (Ty::KeyEvent, "shift") => Some(("shift", Ty::Bool)),
            (Ty::KeyEvent, "control") => Some(("control", Ty::Bool)),
            (Ty::KeyEvent, "alt") => Some(("alt", Ty::Bool)),
            (Ty::KeyEvent, "isAutoRepeat") => Some(("repeat", Ty::Bool)),
            _ => None,
        }
    }

    /// Whether `format!("{}")` shows it, for strings built from it.
    pub(crate) fn shows(&self) -> bool {
        matches!(self, Ty::Bool | Ty::Int | Ty::Real | Ty::Str)
    }
}

/// A kind of item the runtime makes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Kind {
    /// `Item`.
    Item,
    /// `Rectangle`.
    Rectangle,
    /// `Text`.
    Text,
    /// `Row`.
    Row,
    /// `Column`.
    Column,
    /// `MouseArea`.
    MouseArea,
    /// `FocusScope`.
    FocusScope,
    /// `Grid`.
    Grid,
    /// `Flow`.
    Flow,
    /// `RowLayout`.
    RowLayout,
    /// `ColumnLayout`.
    ColumnLayout,
    /// `GridLayout`.
    GridLayout,
    /// `Repeater`.
    Repeater,
}

impl Kind {
    /// The kind a markup name stands for.
    pub(crate) fn named(name: &str) -> Option<Kind> {
        Some(match name {
            "Item" => Kind::Item,
            "Rectangle" => Kind::Rectangle,
            "Text" => Kind::Text,
            "Row" => Kind::Row,
            "Column" => Kind::Column,
            "MouseArea" => Kind::MouseArea,
            "FocusScope" => Kind::FocusScope,
            "Grid" => Kind::Grid,
            "Flow" => Kind::Flow,
            "RowLayout" => Kind::RowLayout,
            "ColumnLayout" => Kind::ColumnLayout,
            "GridLayout" => Kind::GridLayout,
            "Repeater" => Kind::Repeater,
            _ => return None,
        })
    }

    /// Its handle's Rust type.
    pub(crate) fn handle(self) -> &'static str {
        match self {
            Kind::Item => "::tole::handles::Item",
            Kind::Rectangle => "::tole::handles::Rectangle",
            Kind::Text => "::tole::handles::Text",
            Kind::Row => "::tole::handles::Row",
            Kind::Column => "::tole::handles::Column",
            Kind::MouseArea => "::tole::handles::MouseArea",
            Kind::FocusScope => "::tole::handles::FocusScope",
            Kind::Grid => "::tole::handles::Grid",
            Kind::Flow => "::tole::handles::Flow",
            Kind::RowLayout => "::tole::handles::RowLayout",
            Kind::ColumnLayout => "::tole::handles::ColumnLayout",
            Kind::GridLayout => "::tole::handles::GridLayout",
            Kind::Repeater => "::tole::handles::Repeater",
        }
    }

    /// The property `path` names on it: the Rust path from the handle,
    /// and its type. `path` is one name, or two for a group
    /// (`border.width`).
    pub(crate) fn property(self, path: &[&str]) -> Option<(&'static str, Ty)> {
        let common = match path {
            ["x"] => Some((".common.x", Ty::Real)),
            ["y"] => Some((".common.y", Ty::Real)),
            ["width"] => Some((".common.width", Ty::Real)),
            ["height"] => Some((".common.height", Ty::Real)),
            ["implicitWidth"] => Some((".common.implicit_width", Ty::Real)),
            ["implicitHeight"] => Some((".common.implicit_height", Ty::Real)),
            ["visible"] => Some((".common.visible", Ty::Bool)),
            ["opacity"] => Some((".common.opacity", Ty::Real)),
            ["clip"] => Some((".common.clip", Ty::Bool)),
            ["enabled"] => Some((".common.enabled", Ty::Bool)),
            ["focus"] => Some((".common.focus", Ty::Bool)),
            ["activeFocus"] => Some((".common.active_focus", Ty::Bool)),
            ["activeFocusOnTab"] => Some((".common.active_focus_on_tab", Ty::Bool)),
            _ => None,
        };
        if common.is_some() {
            return common;
        }
        match (self, path) {
            (Kind::Rectangle, ["color"]) => Some((".props.color", Ty::Color)),
            (Kind::Rectangle, ["radius"]) => Some((".props.radius", Ty::Real)),
            (Kind::Rectangle, ["border", "width"]) => Some((".props.border_width", Ty::Real)),
            (Kind::Rectangle, ["border", "color"]) => Some((".props.border_color", Ty::Color)),
            (Kind::Text, ["text"]) => Some((".props.text", Ty::Str)),
            (Kind::Text, ["color"]) => Some((".props.color", Ty::Color)),
            (Kind::Text, ["font"]) => Some((".props.font", Ty::Font)),
            (Kind::Text, ["elide"]) => Some((".props.elide", Ty::Elide)),
            (Kind::Text, ["horizontalAlignment"]) => {
                Some((".props.horizontal_alignment", Ty::HAlign))
            }
            (Kind::Text, ["verticalAlignment"]) => Some((".props.vertical_alignment", Ty::VAlign)),
            (Kind::Row | Kind::Column, ["spacing"]) => Some((".props.spacing", Ty::Real)),
            (Kind::Row | Kind::Column, ["padding"]) => Some((".props.padding", Ty::Real)),
            (Kind::Grid | Kind::GridLayout, ["columns"]) => Some((".props.columns", Ty::Int)),
            (
                Kind::Grid | Kind::GridLayout | Kind::Flow | Kind::RowLayout | Kind::ColumnLayout,
                ["spacing"],
            ) => Some((".props.spacing", Ty::Real)),
            (Kind::Grid | Kind::GridLayout, ["rowSpacing"]) => {
                Some((".props.row_spacing", Ty::Real))
            }
            (Kind::Grid | Kind::GridLayout, ["columnSpacing"]) => {
                Some((".props.column_spacing", Ty::Real))
            }
            (Kind::Repeater, ["model"]) => Some((".props.model", Ty::ModelSource)),
            (Kind::Repeater, ["count"]) => Some((".props.count", Ty::Int)),
            (Kind::MouseArea, ["hoverEnabled"]) => Some((".props.hover_enabled", Ty::Bool)),
            (Kind::MouseArea, ["pressed"]) => Some((".props.pressed", Ty::Bool)),
            (Kind::MouseArea, ["containsMouse"]) => Some((".props.contains_mouse", Ty::Bool)),
            (Kind::MouseArea, ["mouseX"]) => Some((".props.mouse_x", Ty::Real)),
            (Kind::MouseArea, ["mouseY"]) => Some((".props.mouse_y", Ty::Real)),
            _ => None,
        }
    }

    /// The signal `name` on it: the Rust path from the handle, and its
    /// parameters.
    pub(crate) fn signal(
        self,
        name: &str,
    ) -> Option<(&'static str, &'static [(&'static str, Ty)])> {
        match (self, name) {
            (Kind::MouseArea, "clicked") => Some((".props.clicked", &[])),
            (Kind::MouseArea, "pressed") => Some((".props.on_pressed", &[])),
            (Kind::MouseArea, "released") => Some((".props.released", &[])),
            (Kind::MouseArea, "entered") => Some((".props.entered", &[])),
            (Kind::MouseArea, "exited") => Some((".props.exited", &[])),
            (Kind::MouseArea, "wheel") => Some((".props.wheel", &[("delta", Ty::Real)])),
            _ => None,
        }
    }

    /// Only the common properties, which any item has: what `parent` at a
    /// component's root reaches.
    pub(crate) fn common_property(path: &[&str]) -> Option<(&'static str, Ty)> {
        Kind::Item.property(path)
    }
}

/// `Layout.fillWidth` and the rest: the field of `tole::LayoutProps`
/// and its type.
pub(crate) fn layout_attached(name: &str) -> Option<(&'static str, Ty)> {
    Some(match name {
        "fillWidth" => ("fill_width", Ty::Bool),
        "fillHeight" => ("fill_height", Ty::Bool),
        "preferredWidth" => ("preferred_width", Ty::Real),
        "preferredHeight" => ("preferred_height", Ty::Real),
        "minimumWidth" => ("minimum_width", Ty::Real),
        "minimumHeight" => ("minimum_height", Ty::Real),
        "maximumWidth" => ("maximum_width", Ty::Real),
        "maximumHeight" => ("maximum_height", Ty::Real),
        "margins" => ("margins", Ty::Real),
        "leftMargin" => ("left_margin", Ty::Real),
        "rightMargin" => ("right_margin", Ty::Real),
        "topMargin" => ("top_margin", Ty::Real),
        "bottomMargin" => ("bottom_margin", Ty::Real),
        "alignment" => ("alignment", Ty::Int),
        _ => return None,
    })
}

/// The keysym a `Keys.on<Key>Pressed` handler is for, as QML names the
/// handlers.
pub(crate) fn keys_handler_keysym(signal: &str) -> Option<&'static str> {
    Some(match signal {
        "returnPressed" => "Return",
        "enterPressed" => "KP_Enter",
        "escapePressed" => "Escape",
        "spacePressed" => "space",
        "tabPressed" => "Tab",
        "backtabPressed" => "ISO_Left_Tab",
        "upPressed" => "Up",
        "downPressed" => "Down",
        "leftPressed" => "Left",
        "rightPressed" => "Right",
        "deletePressed" => "Delete",
        "backPressed" => "BackSpace",
        "homePressed" => "Home",
        "endPressed" => "End",
        "pageUpPressed" => "Prior",
        "pageDownPressed" => "Next",
        _ => return None,
    })
}

/// `Text.ElideRight` and the rest: the runtime's values for the names
/// QML gives them.
pub(crate) fn builtin_constant(space: &str, name: &str) -> Option<(&'static str, Ty)> {
    Some(match (space, name) {
        ("Text", "ElideNone") => ("::tole::Elide::None", Ty::Elide),
        ("Text", "ElideRight") => ("::tole::Elide::Right", Ty::Elide),
        ("Text", "AlignLeft") => ("::tole::HAlign::Left", Ty::HAlign),
        ("Text", "AlignHCenter") => ("::tole::HAlign::Center", Ty::HAlign),
        ("Text", "AlignRight") => ("::tole::HAlign::Right", Ty::HAlign),
        ("Text", "AlignTop") => ("::tole::VAlign::Top", Ty::VAlign),
        ("Text", "AlignVCenter") => ("::tole::VAlign::Center", Ty::VAlign),
        ("Text", "AlignBottom") => ("::tole::VAlign::Bottom", Ty::VAlign),
        ("Qt", "AlignLeft") => ("::tole::align::LEFT", Ty::Int),
        ("Qt", "AlignRight") => ("::tole::align::RIGHT", Ty::Int),
        ("Qt", "AlignHCenter") => ("::tole::align::H_CENTER", Ty::Int),
        ("Qt", "AlignTop") => ("::tole::align::TOP", Ty::Int),
        ("Qt", "AlignBottom") => ("::tole::align::BOTTOM", Ty::Int),
        ("Qt", "AlignVCenter") => ("::tole::align::V_CENTER", Ty::Int),
        ("Qt", "AlignCenter") => ("::tole::align::CENTER", Ty::Int),
        _ => return None,
    })
}

/// A colour name a string may give, as `#rrggbbaa`.
pub(crate) fn named_color(text: &str) -> Option<u32> {
    let hex = |digits: &str| u32::from_str_radix(digits, 16).ok();
    match text {
        "transparent" => Some(0),
        "white" => Some(0xffff_ffff),
        "black" => Some(0x0000_00ff),
        "red" => Some(0xff00_00ff),
        "green" => Some(0x0080_00ff),
        "blue" => Some(0x0000_ffff),
        _ => {
            let digits = text.strip_prefix('#')?;
            match digits.len() {
                6 => Some((hex(digits)? << 8) | 0xff),
                8 => hex(digits),
                3 => {
                    let v = hex(digits)?;
                    let (r, g, b) = ((v >> 8) & 0xf, (v >> 4) & 0xf, v & 0xf);
                    Some((r * 0x11) << 24 | (g * 0x11) << 16 | (b * 0x11) << 8 | 0xff)
                }
                _ => None,
            }
        }
    }
}

/// `camelCase` as `snake_case`, and a Rust keyword made a raw name.
pub(crate) fn snake(name: &str) -> String {
    let mut out = String::new();
    for (index, c) in name.chars().enumerate() {
        if c.is_uppercase() {
            if index > 0 {
                out.push('_');
            }
            out.extend(c.to_lowercase());
        } else {
            out.push(c);
        }
    }
    const KEYWORDS: [&str; 38] = [
        "as", "break", "const", "continue", "crate", "else", "enum", "extern", "false", "fn",
        "for", "if", "impl", "in", "let", "loop", "match", "mod", "move", "mut", "pub", "ref",
        "return", "self", "static", "struct", "super", "trait", "true", "type", "unsafe", "use",
        "where", "while", "async", "await", "dyn", "gen",
    ];
    if KEYWORDS.contains(&out.as_str()) {
        format!("r#{out}")
    } else {
        out
    }
}
