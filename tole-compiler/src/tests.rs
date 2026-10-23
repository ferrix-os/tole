//! Host tests: what is accepted and how it is written out, and what is
//! refused and where. That the written Rust compiles and draws is the
//! examples crate's test (`examples/`), which builds real `.tole` files.

use crate::compile;

/// The diagnostic `source` gives, as `line:column: message`.
fn refused(source: &str) -> String {
    let error = compile(&[("t.tole", source)]).expect_err("refused");
    format!("{}:{}: {}", error.pos.line, error.pos.column, error.message)
}

/// The Rust `source` gives.
fn accepted(source: &str) -> String {
    compile(&[("t.tole", source)]).unwrap_or_else(|error| panic!("{error}"))
}

#[test]
fn a_component_becomes_a_struct_with_new() {
    let rust = accepted(
        r#"
import Fx
component Badge : Rectangle {
    property string label: "E"
    property int level
    width: 16; height: 16
    radius: 3
    color: level <= 3 ? #d2382b : "transparent"
    Text {
        id: letter
        anchors.centerIn: parent
        text: label
        color: "white"
    }
}
"#,
    );
    assert!(rust.contains("pub struct Badge {"), "{rust}");
    assert!(rust.contains("pub base: ::tole::handles::Rectangle,"));
    assert!(rust.contains("pub label: ::tole::Prop<::std::rc::Rc<str>>,"));
    assert!(rust.contains("pub level: ::tole::Prop<i32>,"));
    // Constants are set, expressions bound.
    assert!(
        rust.contains("::tole::Prop::set(this.base.common.width, ui, ((16_i32) as f32));"),
        "{rust}"
    );
    assert!(
        rust.contains("::tole::Prop::bind(this.base.props.color, ui"),
        "{rust}"
    );
    assert!(rust.contains("::tole::Color::rgba(210, 56, 43, 255)"));
    assert!(rust.contains("::tole::Color::rgba(0, 0, 0, 0)"));
    // The child under the root, its text bound to the root's property.
    assert!(rust.contains("let e1 = ::tole::handles::Text::new(ui, ::core::option::Option::Some(::tole::Element::id(&this)))?;"));
    assert!(rust.contains("::tole::Prop::bind(e1.props.text, ui, move |ui: &mut ::tole::Ui| -> ::std::rc::Rc<str> { ui.get(this.label) });"), "{rust}");
    assert!(rust.contains("center_in: ::core::option::Option::Some(::tole::Element::id(&this))"));
}

#[test]
fn declared_types_and_their_values() {
    let rust = accepted(
        r#"
enum Kind { Record, Fold }
struct Line { kind: Kind; level: int, text: string }
component Row1 : Item {
    property Line line
    property bool folded: line.kind == Kind.Fold
    property string shown: "${line.level}: ${line.text}"
}
"#,
    );
    assert!(
        rust.contains("pub enum Kind {\n    #[default] Record,\n    Fold,\n}"),
        "{rust}"
    );
    assert!(rust.contains("pub struct Line {\n    pub kind: Kind,\n    pub level: i32,\n    pub text: ::std::rc::Rc<str>,\n}"));
    assert!(rust.contains("impl ::tole::PropType for Line"));
    assert!(
        rust.contains("((ui.get(this.line)).kind == Kind::Fold)"),
        "{rust}"
    );
    assert!(
        rust.contains(
            r#"::std::format!("{}: {}", (ui.get(this.line)).level, (ui.get(this.line)).text)"#
        ),
        "{rust}"
    );
}

#[test]
fn functions_and_built_ins() {
    let rust = accepted(
        r#"
component Gauge : Rectangle {
    property real used
    function percent(of: real): int { return round(used * 100 / of) }
    width: max(4, used * 200)
    Text { text: "${percent(1)} %"; font: font("Inter", 12, 500); elide: Text.ElideRight }
}
"#,
    );
    assert!(
        rust.contains("pub fn percent(&self, ui: &mut ::tole::Ui, of: f32) -> i32 {"),
        "{rust}"
    );
    assert!(
        rust.contains("this.percent(ui, ((1_i32) as f32))"),
        "{rust}"
    );
    assert!(rust.contains("::tole::Font::new(&"));
    assert!(rust.contains("::tole::Elide::Right"));
}

#[test]
fn components_use_components_and_reach_their_properties() {
    let rust = accepted(
        r#"
component Chip : Rectangle { property string text; Text { text: parent.text } }
component Bar : Row {
    spacing: 8
    Chip { id: first; text: "a" }
    Chip { text: first.text + "b"; width: first.width * 2 }
}
"#,
    );
    assert!(rust.contains("let e1 = Chip::new(ui"), "{rust}");
    assert!(rust.contains(
        "::tole::Prop::bind(e2.text, ui, move |ui: &mut ::tole::Ui| -> ::std::rc::Rc<str> {"
    ));
    assert!(rust.contains("ui.get(e1.base.common.width)"), "{rust}");
    // Inside Chip, `parent` of the Text is the Chip's root.
    assert!(rust.contains("ui.get(this.text)"), "{rust}");
}

#[test]
fn bad_markup_is_refused_where_it_is() {
    assert_eq!(
        refused("component A : Item {\n    widht: 3\n}"),
        "2:5: A has no property `widht`"
    );
    assert_eq!(
        refused("component A : Item { width: \"wide\" }"),
        "1:22: a real was expected, and this is a string"
    );
    assert_eq!(
        refused("component A : Item { visible: 1 + true }"),
        "1:33: `+` does not join a int and a bool"
    );
    assert_eq!(
        refused("component A : Item {\n  Txet { }\n}"),
        "2:3: `Txet` is neither a kind of item nor a component"
    );
    assert_eq!(
        refused("component A : Item { Item { id: a }\n Item { id: a } }"),
        "2:13: the id `a` is given twice"
    );
    assert_eq!(
        refused("component A : Item { B { } }\ncomponent B : Item { A { } }"),
        "2:22: `A` makes itself, through `B`"
    );
    assert_eq!(
        refused("component A : Item { property foo x }"),
        "1:31: `foo` is not a type"
    );
    assert_eq!(
        refused("component A : Item { x: nothing }"),
        "1:25: `nothing` is not an id, a property or a type here"
    );
    assert_eq!(
        refused("component A : Item { signal clicked() }"),
        "1:29: signals come with input, in the next step (U3)"
    );
    assert_eq!(
        refused("component A : Item {\n  x: \"open"),
        "2:6: a string is not closed on its line"
    );
    assert_eq!(
        refused("enum A { X }\ncomponent A : Item { }"),
        "2:11: `A` is declared twice, or is a kind of item"
    );
    assert_eq!(
        refused("component A : Item { x: 1 ? 2 : 3 }"),
        "1:27: a condition is a bool"
    );
}

#[test]
fn an_expression_may_go_on_over_lines() {
    let _ = accepted(
        "component A : Item {\n    property bool wide\n    width: wide\n        ? 200\n        : 100\n    height: 10 +\n        20\n}",
    );
}
