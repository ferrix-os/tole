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
            Ty::Enum(name) | Ty::Struct(name) => name.clone(),
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
            _ => None,
        }
    }

    /// Only the common properties, which any item has: what `parent` at a
    /// component's root reaches.
    pub(crate) fn common_property(path: &[&str]) -> Option<(&'static str, Ty)> {
        Kind::Item.property(path)
    }
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
