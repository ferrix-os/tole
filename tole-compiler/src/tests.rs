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
    assert!(rust.contains("let e1 = ::tole::handles::Text::new(ui, ::core::option::Option::Some(::tole::Element::content(&base)))?;"), "{rust}");
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
        rust.contains(
            "let receiver = this; let arg0: f32 = ((1_i32) as f32); receiver.percent(ui, arg0)"
        ),
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

#[test]
fn signals_handlers_and_their_statements() {
    let rust = accepted(
        r##"
component Chip : Rectangle {
    property string text
    property int clicks
    signal removed()
    signal picked(int index, string name)
    MouseArea {
        id: area
        hoverEnabled: true
        onClicked: {
            clicks = clicks + 1
            if (clicks > 2) { removed() } else { picked(clicks, text) }
        }
        onWheel: parent.radius = delta
    }
    color: area.containsMouse ? "#eeeeee" : "white"
}
component Bar : Row {
    property string last
    Chip { id: chip; onPicked: last = name + index }
}
"##,
    );
    assert!(rust.contains("pub removed: ::tole::SignalId,"), "{rust}");
    assert!(rust.contains("removed: ui.own_signal(::tole::Element::id(&base)),"));
    assert!(rust.contains("ui.connect(e1.props.clicked, ::std::rc::Rc::new(move |ui: &mut ::tole::Ui, args: &[::tole::Value]|"), "{rust}");
    assert!(rust.contains("{ let value = (ui.get(this.clicks)).wrapping_add(1_i32); ::tole::Prop::set(this.clicks, ui, value); }"), "{rust}");
    assert!(rust.contains("{ let args = ::std::vec![]; ui.emit(this.removed, args); }"));
    assert!(rust.contains("{ let args = ::std::vec![<i32 as ::tole::PropType>::into_value(ui.get(this.clicks)), <::std::rc::Rc<str> as ::tole::PropType>::into_value(ui.get(this.text))]; ui.emit(this.picked, args); }"), "{rust}");
    assert!(rust.contains("let delta: f32 = args.get(0)"), "{rust}");
    assert!(
        rust.contains(
            "{ let value = delta; ::tole::Prop::set(this.base.props.radius, ui, value); }"
        ),
        "{rust}"
    );
    // The program's side: which signal, with its arguments.
    assert!(
        rust.contains(
            "pub enum ChipSignal {\n    Removed,\n    Picked(i32, ::std::rc::Rc<str>),\n}"
        ),
        "{rust}"
    );
    assert!(rust.contains(
        "pub fn signal(&self, emitted: &::tole::Emitted) -> ::core::option::Option<ChipSignal>"
    ));
    // A handler on an instance, its parameters by name.
    assert!(rust.contains("ui.connect(e1.picked,"), "{rust}");
    assert!(rust.contains("let name: ::std::rc::Rc<str> = args.get(1)"));
}

#[test]
fn bad_handlers_are_refused() {
    assert_eq!(
        refused("component A : Item { onClicked: x = 1 }"),
        "1:22: A has no signal `clicked`"
    );
    assert_eq!(
        refused("component A : Item { MouseArea { onClicked: 3 } }"),
        "1:45: a statement is an assignment, a call or an `if`"
    );
    assert_eq!(
        refused("component A : Item { MouseArea { onClicked: width = \"x\" } }"),
        "1:53: a real was expected, and this is a string"
    );
    assert_eq!(
        refused("component A : Item { signal s(int n)\n MouseArea { onClicked: s() } }"),
        "2:25: this signal takes 1 arguments"
    );
    assert_eq!(
        refused("component A : Item { signal clicked()\n MouseArea { onClicked: clicked() } }"),
        "2:25: this handler emits the `clicked` it handles, again and again: name the component's root by its id (`root.clicked()`)"
    );
}

#[test]
fn focus_keys_and_key_navigation() {
    let rust = accepted(
        "component A : FocusScope {
            Item { id: a; focus: true; activeFocusOnTab: true; KeyNavigation.down: b
                Keys.onPressed: { if (event.text == \"q\") { event.accepted = true; b.forceActiveFocus() } }
                Keys.onReturnPressed: b.focus = true }
            Item { id: b; enabled: !a.activeFocus }
        }",
    );
    for want in [
        "ui.set_focus(::tole::Element::id(&e1), true);",
        "ui.set_key_navigation(::tole::Element::id(&e1), ::tole::Nav::Down, ::tole::Element::id(&e2));",
        "ui.keys_pressed(::tole::Element::id(&e1))",
        "ui.keys_key_pressed(::tole::Element::id(&e1), \"Return\")",
        "ui.accept_key(true);",
        "ui.force_active_focus(::tole::Element::id(&e2));",
        "ui.set_focus(::tole::Element::id(&e2), on);",
        "((event).text == ::std::rc::Rc::<str>::from(\"q\"))",
        "ui.get(e1.common.active_focus)",
        "::tole::handles::FocusScope::new(ui, parent)",
    ] {
        assert!(rust.contains(want), "{want}\n{rust}");
    }
}

#[test]
fn bad_focus_and_keys_are_refused() {
    assert_eq!(
        refused("component A : Item { property bool f\n focus: f }"),
        "2:9: `focus` is given as `true` or `false`; a handler moves it (`other.focus = true`, `forceActiveFocus()`)"
    );
    assert_eq!(
        refused("component A : Item { Keys.onHelloPressed: x = 1 }"),
        "1:27: `Keys` has no signal `helloPressed`"
    );
    assert_eq!(
        refused("component A : Item { Mouse.onPressed: x = 1 }"),
        "1:22: `Mouse` has no signals: `Keys` has"
    );
    assert_eq!(
        refused("component A : Item { KeyNavigation.tab: 3 }"),
        "1:41: `KeyNavigation` names an id"
    );
    assert_eq!(
        refused("component A : Item { KeyNavigation.sideways: a }"),
        "1:36: `KeyNavigation` has `tab`, `backtab`, `up`, `down`, `left` and `right`"
    );
}

#[test]
fn layouts_lists_and_repeaters() {
    let rust = accepted(
        "struct Tag { label: string; on: bool }
        component Chip : Rectangle { property string label; property Tag modelData }
        component A : Item {
            property list<Tag> tags
            property list<string> names: [\"a\", \"b\"]
            property int first: names.length
            property string second: names[1]
            property bool on: tags[0].on
            RowLayout {
                Layout.fillWidth: true
                Item { Layout.preferredWidth: 30; Layout.alignment: Qt.AlignTop | Qt.AlignRight }
                Repeater { model: tags; Chip { label: modelData.label + \"${index}\" } }
                Repeater { model: 3; Rectangle { width: index * 10 } }
            }
        }",
    );
    for want in [
        "pub tags: ::tole::Prop<::std::vec::Vec<Tag>>,",
        "::std::vec![::std::rc::Rc::<str>::from(\"a\"), ::std::rc::Rc::<str>::from(\"b\")]",
        "(i32::try_from((ui.get(this.names)).len()).unwrap_or(i32::MAX))",
        "let layout_e1 = ui.layout_of(::tole::Element::id(&e1))?;",
        "::tole::Prop::set(layout_e2.alignment, ui, (::tole::align::TOP | ::tole::align::RIGHT));",
        "::tole::ModelSource::list(ui.get(this.tags))",
        "::tole::ModelSource::count(3_i32)",
        "let row_1 = d1_0.model_data.id;",
        "let model_data_1: ::tole::Prop<Tag> = ::tole::Prop::from_id(row_1);",
        "let model_data_1: ::tole::Prop<i32> = ::tole::Prop::from_id(row_1);",
        "ui.set_delegate(::tole::Element::id(&e3), factory);",
        "(ui.get(model_data_1)).label",
        "ui.update();",
    ] {
        assert!(rust.contains(want), "{want}\n{rust}");
    }
}

#[test]
fn bad_layouts_and_repeaters_are_refused() {
    assert_eq!(
        refused("component A : Item { Layout.fillSideways: true }"),
        "1:22: `Layout.fillSideways` is not one of `Layout`'s"
    );
    assert_eq!(
        refused("component A : Item { Repeater { model: 2; Item {} Item {} } }"),
        "1:51: a `Repeater` has one delegate: put the rest inside it"
    );
    assert_eq!(
        refused("component A : Item { Repeater { model: \"x\"; Item {} } }"),
        "1:33: a count, list or model was expected, and this is a string"
    );
    assert_eq!(
        refused(
            "component C : Item { property string modelData }\ncomponent A : Item { Repeater { model: 2; C {} } }"
        ),
        "2:43: `C`'s `modelData` is a string, and the model's rows here are a int"
    );
    assert_eq!(
        refused("component A : Item { property list<int> n: [1, \"a\"] }"),
        "1:44: a list's items are of one type"
    );
    assert_eq!(
        refused("component A : Item { property int n: 3[0] }"),
        "1:39: a int cannot be indexed"
    );
}

#[test]
fn list_views_choose_their_delegates() {
    let rust = accepted(
        "enum K { A, B }
        struct R { kind: K; text: string }
        component V : Item {
            property model<R> rows
            Flickable { id: f; contentHeight: 900; Item { height: f.contentY } }
            ListView {
                id: list
                model: rows
                delegate: DelegateChooser {
                    role: \"kind\"
                    DelegateChoice { roleValue: K.B; Text { text: modelData.text; width: parent.width } }
                    DelegateChoice { Item { height: index } }
                }
            }
            ListView { model: 10; Item { height: list.count } }
        }",
    );
    for want in [
        "::tole::Prop::set(e1.props.content_height, ui, ((900_i32) as f32));",
        "ui.get(e1.props.content_y)",
        "let factories: ::std::vec::Vec<::tole::DelegateFactory> = ::std::vec![",
        "let role = row.kind;",
        "if role == K::B { return 0; } 1 });",
        "ui.set_view_delegates(::tole::Element::id(&e3), factories, ::core::option::Option::Some(chooser));",
        "ui.get(e3.common.width)",
        "ui.set_view_delegates(::tole::Element::id(&e4), ::std::vec![",
        "::core::option::Option::None);",
        "ui.get(e3.props.count)",
    ] {
        assert!(rust.contains(want), "{want}\n{rust}");
    }
}

#[test]
fn bad_choosers_are_refused() {
    assert_eq!(
        refused(
            "struct R { kind: int }\ncomponent V : Item { property model<R> rows\n ListView { model: rows; delegate: DelegateChooser { role: \"sort\" } } }"
        ),
        "3:60: the rows, a R, have no field `sort`"
    );
    assert_eq!(
        refused(
            "component V : Item { property int k\n ListView { model: 3; DelegateChooser { DelegateChoice { roleValue: k; Item {} } } } }"
        ),
        "2:69: a `roleValue` is a constant"
    );
    assert_eq!(
        refused("component V : Item { Repeater { model: 3; DelegateChooser { } } }"),
        "1:43: a `DelegateChooser` chooses a `ListView`'s delegates; a `Repeater` has one"
    );
}

#[test]
fn states_transitions_and_animations() {
    let rust = accepted(
        "component A : Rectangle {
            id: root
            property bool open
            Rectangle {
                id: panel
                Behavior on opacity { NumberAnimation { duration: 90; easing.type: Easing.OutCubic } }
                SequentialAnimation on x { loops: Animation.Infinite; running: root.open
                    NumberAnimation { to: 10; duration: 100 } PauseAnimation { duration: 50 } }
            }
            states: [ State { name: \"open\"; when: open; PropertyChanges { target: panel; width: 200; color: \"red\" } } ]
            transitions: Transition { from: \"*\"; to: \"open\"; NumberAnimation { properties: \"width\"; duration: 300 } }
        }",
    );
    for want in [
        "ui.behavior(::tole::Element::id(&e1), e1.common.opacity.id, ::tole::Animate::Always(::tole::Tween { duration: ((90_i32) as f32), easing: ::tole::Easing::OutCubic }));",
        "::tole::Prop::bind(running, ui, move |ui: &mut ::tole::Ui| -> bool { ui.get(this.open) });",
        "::tole::Step::Sequential(::std::vec![::tole::Step::Tween { target: e1.common.x.id, from: ::core::option::Option::None, to: ::core::option::Option::Some(<f32 as ::tole::PropType>::into_value(((10_i32) as f32))),",
        "::tole::Step::Pause((::tole::Tween { duration: ((50_i32) as f32), easing: ::tole::Easing::Linear }).duration)",
        "ui.animation_on(::tole::Element::id(&e1), step, -1_i32, running);",
        "let active_this = ui.active_state(::tole::Element::id(&this), ::std::vec![(::std::rc::Rc::<str>::from(\"open\"),",
        "ui.bind_states(::tole::Element::id(&this), e1.common.width.id, active_this,",
        "ui.bind_states(::tole::Element::id(&this), e1.props.color.id, active_this,",
        "::tole::Animate::Transitions { active: active_this, rules: ::std::vec![::tole::Transition { from: ::std::rc::Rc::from(\"*\"), to: ::std::rc::Rc::from(\"open\"),",
    ] {
        assert!(rust.contains(want), "{want}\n{rust}");
    }
    // States are bound before the animations that read them.
    let states = rust.find("let active_this").expect("states");
    let behavior = rust
        .find("::tole::Animate::Transitions")
        .expect("transitions");
    assert!(states < behavior);
}

#[test]
fn bad_animations_are_refused() {
    assert_eq!(
        refused("component A : Text { Behavior on text { NumberAnimation {} } }"),
        "1:34: a string does not animate: reals and colours do"
    );
    assert_eq!(
        refused("component A : Rectangle { Behavior on color { NumberAnimation {} } }"),
        "1:47: a NumberAnimation animates a real, and this is a color"
    );
    assert_eq!(
        refused("component A : Item { states: [ State { when: true } ] }"),
        "1:32: a `State` has a `name`"
    );
    assert_eq!(
        refused("component A : Item { transitions: Transition { } }"),
        "1:22: `transitions` animate changes of `states`, and there are none here"
    );
    assert_eq!(
        refused(
            "component A : Item { states: State { name: \"a\"; PropertyChanges { wings: 2 } } }"
        ),
        "1:67: A has no property `wings`"
    );
}

#[test]
fn modules_singletons_and_the_controls() {
    let rust = accepted(
        r#"
import Fx
import Fx.Controls
component Page : Rectangle {
    color: Theme.bg
    Button { id: save; text: "Save"; highlighted: true; onClicked: Theme.dark = !Theme.dark }
    Text { text: save.text; font: Theme.sans(); color: Theme.ink }
}
"#,
    );
    // The controls are a Rust module of their own, which sees the top.
    assert!(
        rust.contains("pub mod fx_controls {\nuse super::*;"),
        "{rust}"
    );
    assert!(rust.contains("pub struct Button {"));
    assert!(
        rust.contains("let e1 = fx_controls::Button::new(ui, "),
        "{rust}"
    );
    // A singleton: made once per Ui, its properties read and set by name.
    assert!(
        rust.contains(
            "pub fn instance(ui: &mut ::tole::Ui) -> Self {\n        ui.global(Self::make)"
        )
    );
    assert!(
        rust.contains("::tole::Prop::get(fx_controls::Theme::instance(ui).bg, ui)"),
        "{rust}"
    );
    assert!(
        rust.contains("::tole::Prop::set(fx_controls::Theme::instance(ui).dark, ui, value)"),
        "{rust}"
    );
    // A function's missing arguments are its defaults.
    assert!(rust.contains("let receiver = fx_controls::Theme::instance(ui); let arg0: f32 = ((13_i32) as f32); let arg1: i32 = 400_i32; receiver.sans(ui, arg0, arg1)"), "{rust}");
    // Without the import, the controls are not there.
    assert_eq!(
        refused("import Fx\ncomponent P : Item { Button { } }"),
        "2:22: `Button` is neither a kind of item nor a component"
    );
    assert_eq!(
        refused("import Fx.Widgets\ncomponent P : Item { }"),
        "1:8: there is no module `Fx.Widgets`: `Fx` and `Fx.Controls` are"
    );
}

#[test]
fn a_file_sees_its_own_then_its_imports_then_the_program() {
    // Two files: one declares its own Button; the other imports the
    // controls', and gets theirs.
    let rust = compile(&[
        (
            "own.tole",
            "component Button : Rectangle { property string label }\ncomponent Mine : Item { Button { label: \"x\" } }",
        ),
        (
            "uses.tole",
            "import Fx.Controls\ncomponent Theirs : Item { Button { text: \"y\" } }",
        ),
    ])
    .unwrap_or_else(|error| panic!("{error}"));
    assert!(rust.contains("let e1 = Button::new(ui"), "{rust}");
    assert!(
        rust.contains("let e1 = fx_controls::Button::new(ui"),
        "{rust}"
    );
}

#[test]
fn aliases_and_where_a_uses_children_go() {
    let rust = accepted(
        r#"
component Card : Rectangle {
    property alias title: heading.text
    property alias titleColor: heading.color
    default property alias content: body
    Text { id: heading; text: "Card" }
    Column { id: body; y: 30 }
}
component Page : Item {
    Card {
        title: "Mail"
        Text { text: "inside"; anchors.left: parent.left }
    }
}
"#,
    );
    // The alias is the item's own cell, so a change either way is seen
    // both ways.
    assert!(rust.contains("            title: e1.props.text,"), "{rust}");
    assert!(rust.contains("            title_color: e1.props.color,"));
    assert!(rust.contains("            content: ::tole::Element::content(&e2),"));
    assert!(
        rust.contains("    fn content(&self) -> ::tole::ItemId {\n        self.content\n    }")
    );
    // A use's children go where the default alias says; `parent` there is
    // found when it runs.
    assert!(
        rust.contains("let e2 = ::tole::handles::Text::new(ui, ::core::option::Option::Some(::tole::Element::content(&e1)))?;"),
        "{rust}"
    );
    assert!(
        rust.contains("left: ui.parent(::tole::Element::id(&e2))"),
        "{rust}"
    );
    // The alias is set like any property of the component.
    assert!(
        rust.contains("::tole::Prop::set(e1.title, ui, ::std::rc::Rc::<str>::from(\"Mail\"));"),
        "{rust}"
    );
}

#[test]
fn bad_modules_aliases_and_singletons_are_refused() {
    assert_eq!(
        refused("component C : Item { property alias t: nothing.text }"),
        "1:40: `nothing.text` names no property this alias can stand for"
    );
    assert_eq!(
        refused("component C : Item { property alias t: 4 }"),
        "1:37: an alias names what it stands for: `property alias text: label.text`, `default property alias content: box`"
    );
    assert_eq!(
        refused("component C : Item { default property alias c: a.b; Item { id: a } }"),
        "1:45: a default alias names the id of the item a use's children go in"
    );
    assert_eq!(
        refused("singleton S { property int a: 1; Item { } }"),
        "1:34: a singleton holds properties and functions"
    );
    assert_eq!(
        refused("singleton S { property int a: 1 }\ncomponent C : Item { width: S.b }"),
        "2:29: `S` has no property `b`"
    );
    assert_eq!(
        refused(
            "component C : Item { function f(a: int, b: int = 2): int { return a + b }\n width: f() }"
        ),
        "2:9: `f` takes 2 arguments"
    );
    assert_eq!(
        refused("component C : Item { property bool on\n Exclusive.checked: 3 }"),
        "2:2: `Exclusive.checked: checked` names a bool property"
    );
}

#[test]
fn popups_splits_folders_and_item_methods() {
    let rust = accepted(
        r#"
component P : Item {
    SplitView {
        orientation: Qt.Vertical
        Item { id: top; Layout.preferredHeight: 100 }
        Popup { id: pop; closePolicy: Popup.CloseOnEscape | Popup.CloseOnReleaseInside; modal: true }
    }
    FolderListModel { id: dir; folder: "~/.ssh"; showHidden: true }
    ListView {
        model: dir.files
        delegate: Text { text: modelData.isDir ? modelData.name + "/" : modelData.name }
    }
    MouseArea { cursorShape: Qt.PointingHandCursor; onDoubleClicked: pop.open(); onPositionChanged: pop.close() }
}
"#,
    );
    assert!(
        rust.contains("::tole::handles::SplitViewVertical::new("),
        "{rust}"
    );
    assert!(
        rust.contains(
            "::tole::close_policy::CLOSE_ON_ESCAPE | ::tole::close_policy::CLOSE_ON_RELEASE_INSIDE"
        ),
        "{rust}"
    );
    assert!(
        rust.contains("ui.open_popup(::tole::Element::id(&e3));"),
        "{rust}"
    );
    assert!(rust.contains("ui.close_popup(::tole::Element::id(&e3));"));
    assert!(
        rust.contains("::tole::ModelSource::model(ui.get(e4.props.files))"),
        "{rust}"
    );
    assert!(rust.contains(".is_dir"));
    assert_eq!(
        refused("component P : Item { SplitView { orientation: o }\n property int o }"),
        "1:47: a SplitView's `orientation` is `Qt.Horizontal` or `Qt.Vertical`, and stays"
    );
}

#[test]
fn shapes_and_canvases() {
    let rust = accepted(
        r#"
component P : Item {
    property painter chart
    Canvas { painter: chart }
    Shape {
        ShapePath {
            fillColor: "red"
            startX: 0; startY: 0
            PathLine { x: width; y: 10 }
            PathQuad { controlX: 4; controlY: 4; x: 0; y: 20 }
            closed: true
        }
    }
}
"#,
    );
    assert!(
        rust.contains("pub chart: ::tole::Prop<::tole::CanvasPainter>,"),
        "{rust}"
    );
    assert!(
        rust.contains("::tole::Prop::bind(e1.props.painter, ui"),
        "{rust}"
    );
    assert!(
        rust.contains("let _ = path.line_to(ui.get(e2.common.width), ((10_i32) as f32));"),
        "{rust}"
    );
    assert!(rust.contains(
        "path.quad_to(((4_i32) as f32), ((4_i32) as f32), ((0_i32) as f32), ((20_i32) as f32))"
    ));
    assert!(rust.contains("if true { let _ = path.close(); }"));
    assert_eq!(
        refused("component P : Shape { Rectangle { } }"),
        "1:23: a `Shape` holds `ShapePath`s"
    );
}
