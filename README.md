# tole

Ferrix's UI framework, after Qt Quick and QML: a retained tree of items
with typed properties and bindings, anchors and layouts, drawn on the CPU
with tiny-skia and Ferrix's own text, with a GPU path to come. Its markup,
`.tole` files, is compiled into Rust when a program is built, so a program
carries no interpreter.

The design, the plan and where it stands are in Ferrix's
[`docs/UI-FRAMEWORK.md`](https://github.com/ferrix-os/ferrix/blob/develop/docs/UI-FRAMEWORK.md).

## Demos

Each is a `.tole` file in [`examples/ui`](examples/ui), compiled by the
examples' `build.rs` and drawn headless on the CPU at scale 2. The
pictures below are those drawings: `cargo run -p tole-examples -- --all
docs/screenshots` writes them, and a test fails (in CI too) when one no
longer matches what its demo draws, so they cannot go stale.

### Hello

```qml
// The smallest program: a card with two lines of text, centred.
import Fx

component Hello : Rectangle {
    width: 360; height: 160
    color: "#fafaf8"

    Rectangle {
        anchors.fill: parent
        anchors.margins: 16
        radius: 12
        color: "white"
        border { width: 1; color: "#e6e6e2" }

        Column {
            anchors.centerIn: parent
            spacing: 6
            Text { text: "Hello from tole"; font: font("Inter", 22, 600); color: "#1a1a1a" }
            Text { text: "a .tole file, compiled into Rust"; font: font("Inter", 13, 400); color: "#888888" }
        }
    }
}
```

![hello](docs/screenshots/hello.png)

### Layouts

Every place is a binding: anchors to the window and to a sibling, a `Row`
and a `Column`. Resize the window and everything follows.

```qml
// Anchors, a Row and a Column: every place here is a binding, so resizing
// the window moves everything with it.
import Fx

component Swatch : Rectangle {
    property string label
    width: 64; height: 40
    radius: 6
    Text {
        anchors.centerIn: parent
        text: label
        font: font("Inter", 12, 500)
        color: "white"
    }
}

component Layouts : Rectangle {
    width: 480; height: 300
    color: "#fafaf8"

    // A corner each, anchored to the window.
    Swatch { label: "left"; color: "#2a62c9"; anchors.left: parent.left; anchors.top: parent.top; anchors.margins: 12 }
    Swatch { label: "right"; color: "#2fa35a"; anchors.right: parent.right; anchors.top: parent.top; anchors.margins: 12 }
    Swatch { id: middle; label: "centre"; color: "#d2382b"; anchors.centerIn: parent }
    // Beside the middle one, its right edge on the middle's left edge.
    Swatch { label: "beside"; color: "#e9a91c"; anchors.right: middle.left; anchors.rightMargin: 8; anchors.verticalCenter: middle.verticalCenter }

    Row {
        anchors.left: parent.left; anchors.bottom: parent.bottom; anchors.margins: 12
        spacing: 6
        Swatch { label: "Row"; color: "#555555"; width: 48 }
        Swatch { label: "of"; color: "#777777"; width: 36 }
        Swatch { label: "three"; color: "#999999"; width: 56 }
    }

    Column {
        anchors.right: parent.right; anchors.bottom: parent.bottom; anchors.margins: 12
        spacing: 4
        Swatch { label: "a"; color: "#7ea6f5"; height: 22 }
        Swatch { label: "Column"; color: "#2a62c9"; height: 22 }
        Swatch { label: "${2 + 1} high"; color: "#14306a"; height: 22 }
    }
}
```

![layouts](docs/screenshots/layouts.png)

### Counter

Signals and handlers. A button is a component with a `clicked` signal,
and each use of it says what a click does; its colour is a binding on its
`MouseArea`'s `pressed` and `containsMouse`. The picture is after three
clicks on +1, with the pointer resting on Reset; the demo's script works
the buttons through the same pointer calls a window feeds.

```qml
// Signals and handlers: a button is a component with a `clicked`
// signal, and each use says what a click does. Its colour is a binding on
// its MouseArea's `pressed` and `containsMouse`.
import Fx

component Button : Rectangle {
    id: button
    property string label
    signal clicked()

    width: caption.implicitWidth + 32
    height: 36
    radius: 8
    color: area.pressed ? "#14306a" : area.containsMouse ? "#2a62c9" : "#555555"

    Text {
        id: caption
        anchors.centerIn: parent
        text: label
        color: "white"
        font: font("Inter", 14, 500)
    }
    MouseArea {
        id: area
        anchors.fill: parent
        hoverEnabled: true
        // `clicked()` alone would be the MouseArea's own, as in QML.
        onClicked: button.clicked()
    }
}

component Counter : Rectangle {
    property int count
    width: 420; height: 180
    color: "#fafaf8"

    Column {
        anchors.centerIn: parent
        spacing: 16
        Text {
            text: count == 1 ? "Clicked once" : "Clicked ${count} times"
            font: font("Inter", 20, 600)
            color: count >= 3 ? "#d2382b" : "#1a1a1a"
        }
        Row {
            spacing: 8
            Button { label: "+1"; onClicked: count = count + 1 }
            Button { label: "−1"; onClicked: if (count > 0) { count = count - 1 } }
            Button { label: "Reset"; onClicked: count = 0 }
        }
    }
}
```

![counter](docs/screenshots/counter.png)

The program's Rust is not called back: it reads what was emitted, after
it hands the `Ui` an event, through an enum the compiler writes for each
component that declares signals.

```rust
let button = Button::new(&mut ui, None)?;
ui.pointer_move(x, y);
ui.pointer_press();
ui.pointer_release();
for emitted in ui.take_signals() {
    if let Some(ButtonSignal::Clicked) = button.signal(&emitted) {
        // ...
    }
}
```

### Focus and keys

Focus as Qt Quick has it: one item at a time holds `focus` within its
focus scope, keys go to the item with `activeFocus` and bubble up until a
handler takes them, Tab and Shift+Tab walk the items with
`activeFocusOnTab` in tree order, and `KeyNavigation` sends the arrows
where it says. `Keys.onSpacePressed` takes its key; `Keys.onPressed` takes
one by `event.accepted = true`. Keys are named as xkb names them
(`event.key == "Return"`). The picture is after Tab, Space, Right, Right,
Space and Down.

```qml
// Focus and keys: Tab and Shift+Tab walk the tiles in tree order, the
// arrows move as KeyNavigation says, and Space or a click toggles a tile.
// The ring is a binding on `activeFocus`.
import Fx

component Tile : Rectangle {
    id: tile
    property string label
    property bool on

    activeFocusOnTab: true
    width: 128; height: 64
    radius: 10
    color: on ? "#2a62c9" : "white"
    border { width: activeFocus ? 3 : 1; color: activeFocus ? "#e9a91c" : "#d6d6d0" }

    Text {
        anchors.centerIn: parent
        text: label
        color: tile.on ? "white" : "#1a1a1a"
        font: font("Inter", 14, 600)
    }
    Keys.onSpacePressed: on = !on
    MouseArea {
        anchors.fill: parent
        onClicked: { tile.forceActiveFocus(); tile.on = !tile.on }
    }
}

component Focus : Rectangle {
    width: 460; height: 236
    color: "#fafaf8"

    Column {
        anchors.centerIn: parent
        spacing: 12
        Row {
            spacing: 12
            Tile { id: wifi; label: "Wi-Fi"; KeyNavigation.right: bluetooth; KeyNavigation.down: dark }
            Tile { id: bluetooth; label: "Bluetooth"; KeyNavigation.left: wifi; KeyNavigation.right: vpn; KeyNavigation.down: night }
            Tile { id: vpn; label: "VPN"; KeyNavigation.left: bluetooth; KeyNavigation.down: quiet }
        }
        Row {
            spacing: 12
            Tile { id: dark; label: "Dark"; KeyNavigation.up: wifi; KeyNavigation.right: night }
            Tile { id: night; label: "Night light"; KeyNavigation.up: bluetooth; KeyNavigation.left: dark; KeyNavigation.right: quiet }
            Tile { id: quiet; label: "Quiet"; KeyNavigation.up: vpn; KeyNavigation.left: night }
        }
        Text {
            text: "Tab and the arrows move the focus; Space toggles"
            font: font("Inter", 12, 400)
            color: "#888888"
        }
    }
}
```

![focus](docs/screenshots/focus.png)

### Layouts and Repeater

Qt Quick Layouts: a `ColumnLayout` holds a heading `RowLayout` (a
filling `Item` pushes the button right), a `GridLayout` whose fields
take the width left over, a `Flow` of chips that wraps, and a `Grid`.
`Layout.fillWidth`, `preferredWidth`, `minimum*`, `maximum*`, the margins
and `alignment` share the room as Qt shares it. Each way is worked out
on its own, so a `Flow`'s height can follow the width its layout gives
it. The chips and swatches are a `Repeater`'s delegates, made from a
`list<string>` and from a count, with `modelData` and `index`; a
program's own `model<T>` (`tole::VecModel`, or any `tole::Model`) works
the same way and is followed as it changes.

```qml
// Layouts and Repeater: a ColumnLayout holding a heading row, a
// GridLayout of labelled fields that fill the width, a Flow of chips a
// Repeater makes from a list, and a Grid of swatches made from a count.
import Fx

component Field : Rectangle {
    property string value
    implicitHeight: 30
    radius: 6
    color: "white"
    border { width: 1; color: "#d6d6d0" }
    Text {
        anchors.left: parent.left; anchors.leftMargin: 10
        anchors.verticalCenter: parent.verticalCenter
        text: value
        font: font("Inter", 13, 400)
        color: "#1a1a1a"
    }
}

component Chip : Rectangle {
    property string label
    property bool on
    implicitWidth: caption.implicitWidth + 24
    implicitHeight: 26
    radius: 13
    color: on ? "#2a62c9" : "#ececE8"
    Text {
        id: caption
        anchors.centerIn: parent
        text: label
        font: font("Inter", 12, 500)
        color: on ? "white" : "#1a1a1a"
    }
}

component Label : Text {
    font: font("Inter", 13, 400)
    color: "#777777"
}

component Form : Rectangle {
    width: 520; height: 404
    color: "#fafaf8"

    ColumnLayout {
        anchors.fill: parent
        anchors.margins: 20
        spacing: 12

        RowLayout {
            Layout.fillWidth: true
            Text { text: "Network"; font: font("Inter", 20, 600); color: "#1a1a1a" }
            Item { Layout.fillWidth: true }
            Rectangle {
                implicitWidth: 76; implicitHeight: 30
                radius: 8
                color: "#2a62c9"
                Text { anchors.centerIn: parent; text: "Apply"; color: "white"; font: font("Inter", 13, 600) }
            }
        }

        GridLayout {
            Layout.fillWidth: true
            columns: 2
            columnSpacing: 12
            rowSpacing: 8
            Label { text: "Name" }
            Field { Layout.fillWidth: true; value: "home" }
            Label { text: "Address" }
            Field { Layout.fillWidth: true; value: "192.168.9.193/24" }
            Label { text: "Gateway" }
            Field { Layout.fillWidth: true; value: "192.168.9.1" }
        }

        Label { text: "Interfaces"; Layout.topMargin: 6 }
        Flow {
            Layout.fillWidth: true
            spacing: 8
            Repeater {
                model: ["eth0", "wlan0", "wg0", "docker0", "br-lan", "tailscale0", "virbr0", "lo"]
                Chip { label: modelData; on: index < 3 }
            }
        }

        Label { text: "Accent"; Layout.topMargin: 6 }
        Grid {
            columns: 12
            spacing: 6
            Repeater {
                model: 24
                Rectangle {
                    width: 30; height: 22
                    radius: 4
                    color: rgba(0.10 + index * 0.035, 0.38 + (index % 12) * 0.02, 0.85 - index * 0.025, 1)
                }
            }
        }

        Item { Layout.fillHeight: true }
    }
}
```

![form](docs/screenshots/form.png)

### A list of 100,000 rows

A `ListView` over a Rust model of 100,000 rows, two kinds of row chosen
by a `DelegateChooser` on the rows' `kind`. Only the rows in view have
delegates; scrolled out, a delegate goes to a pool of its kind and is
given the next row that needs one, so a screenful of them serves the
whole list (a test scrolls a thousand wheel steps and counts). Row
heights live in a Fenwick tree, so finding the row at an offset and
correcting a row measured differently both take `log n` steps. The
model makes a row only when it is shown; this one computes them, and
says its rows' heights so the view never guesses. The picture is
scrolled to row 50,000 with `positionViewAtIndex`.

```qml
// A ListView of 100,000 rows from a Rust model, two kinds of row chosen
// by a DelegateChooser. Only the rows in view have delegates, recycled
// as it scrolls; the model makes a row only when it is shown.
import Fx

enum EntryKind { Record, Header }

struct Entry {
    kind: EntryKind
    level: int
    time: string
    source: string
    text: string
}

component Journal : Rectangle {
    property model<Entry> entries
    width: 600; height: 420
    color: "#fafaf8"

    Rectangle {
        id: bar
        width: parent.width; height: 46
        color: "white"
        Text { x: 18; anchors.verticalCenter: parent.verticalCenter; text: "Journal"; font: font("Inter", 17, 600); color: "#1a1a1a" }
        Text {
            anchors.right: parent.right; anchors.rightMargin: 18
            anchors.verticalCenter: parent.verticalCenter
            text: "${group(list.count)} entries"
            font: font("Inter", 12, 400)
            color: "#888888"
        }
        Rectangle { y: parent.height - 1; width: parent.width; height: 1; color: "#e6e6e2" }
    }

    ListView {
        id: list
        anchors.top: bar.bottom; anchors.bottom: parent.bottom
        anchors.left: parent.left; anchors.right: parent.right
        clip: true
        model: entries
        delegate: DelegateChooser {
            role: "kind"
            DelegateChoice {
                roleValue: EntryKind.Header
                Rectangle {
                    width: parent.width; height: 34
                    color: "#f0f0ec"
                    Text { x: 18; anchors.verticalCenter: parent.verticalCenter; text: modelData.text; font: font("Inter", 12, 600); color: "#555555" }
                }
            }
            DelegateChoice {
                roleValue: EntryKind.Record
                Rectangle {
                    width: parent.width; height: 24
                    color: modelData.level <= 3 ? "#fbe9e7" : modelData.level == 4 ? "#fdf4e0" : "transparent"
                    Text { x: 18; anchors.verticalCenter: parent.verticalCenter; text: modelData.time; font: font("Inter", 12, 400); color: "#999999" }
                    Text {
                        x: 92; width: 96; anchors.verticalCenter: parent.verticalCenter
                        text: modelData.source; elide: Text.ElideRight
                        font: font("Inter", 12, 500)
                        color: modelData.level <= 3 ? "#c62828" : "#2a62c9"
                    }
                    Text {
                        x: 196; width: parent.width - 212; anchors.verticalCenter: parent.verticalCenter
                        text: modelData.text; elide: Text.ElideRight
                        font: font("Inter", 12, 400)
                        color: "#1a1a1a"
                    }
                }
            }
        }
    }
}
```

The model, in the demo's Rust:

```rust
impl tole::Model for JournalRows {
    type Row = Entry;                       // the markup's struct
    fn row_count(&self) -> usize { 100_000 }
    fn row(&self, index: usize) -> Option<Entry> { /* made on demand */ }
    fn row_height(&self, index: usize) -> Option<f32> {
        Some(if index.is_multiple_of(40) { 34.0 } else { 24.0 })
    }
    fn tracker(&self) -> &tole::ModelTracker { &self.tracker }
}
```

![journal](docs/screenshots/journal.png)

### States, transitions and animations

`states` with `when` and `PropertyChanges`, a `Transition` animating
what a change of state moves, `Behavior on x` animating every change,
and `SequentialAnimation`/`ParallelAnimation`/`PauseAnimation` with
`NumberAnimation` and `ColorAnimation` on a property, `loops` and
`running`, along Qt's easing curves. The clock is the window's frames;
the tests and these pictures tick it by hand, so they are exact. The
picture is a second after the switch was clicked; below it, the demo
played (an animated PNG, drawn the same way: the switch on, then off).

```qml
// States, transitions and animations. The switch's knob and colour
// follow it through Behaviors; the card has a state, "open", that the
// switch turns on, and a Transition slides its panel in and tints the
// card when the state changes. The dot pulses for ever: a
// SequentialAnimation on its opacity.
import Fx

component Switch : Rectangle {
    id: switcher
    property bool on
    width: 52; height: 30
    radius: 15
    color: on ? "#2a62c9" : "#c9c9c4"
    Behavior on color { ColorAnimation { duration: 180 } }
    Rectangle {
        x: switcher.on ? 25 : 3; y: 3
        width: 24; height: 24
        radius: 12
        color: "white"
        Behavior on x { NumberAnimation { duration: 180; easing.type: Easing.OutCubic } }
    }
    MouseArea { anchors.fill: parent; onClicked: switcher.on = !switcher.on }
}

component Transitions : Rectangle {
    width: 480; height: 300
    color: "#fafaf8"

    Text { x: 24; y: 28; text: "Details"; font: font("Inter", 18, 600); color: "#1a1a1a" }
    Switch { id: toggle; x: 404; y: 24 }

    Rectangle {
        id: card
        x: 24; y: 76; width: 432; height: 200
        radius: 12
        color: "white"
        border { width: 1; color: "#e6e6e2" }
        clip: true

        Text { x: 20; y: 22; text: toggle.on ? "Shown in full" : "Folded"; font: font("Inter", 15, 600); color: "#1a1a1a" }
        Rectangle {
            x: 20; y: 60; width: 12; height: 12
            radius: 6
            color: "#2fa35a"
            SequentialAnimation on opacity {
                loops: Animation.Infinite
                NumberAnimation { to: 0.2; duration: 700; easing.type: Easing.InOutSine }
                NumberAnimation { to: 1; duration: 700; easing.type: Easing.InOutSine }
            }
        }
        Text { x: 40; y: 57; text: "live"; font: font("Inter", 13, 400); color: "#777777" }

        Rectangle {
            id: panel
            x: 440; y: 12; width: 200; height: 176
            radius: 10
            color: "#14306a"
            Text { x: 18; y: 18; text: "The panel"; font: font("Inter", 15, 600); color: "white" }
            Text { x: 18; y: 44; text: "slid in by a Transition"; font: font("Inter", 12, 400); color: "#b8c8ec" }
        }
    }

    states: [
        State {
            name: "open"
            when: toggle.on
            PropertyChanges { target: panel; x: 220 }
            PropertyChanges { target: card; color: "#eef3fc" }
        }
    ]
    transitions: [
        Transition {
            NumberAnimation { properties: "x"; duration: 500; easing.type: Easing.OutBack }
            ColorAnimation { duration: 500 }
        }
    ]
}
```

![transitions](docs/screenshots/transitions.png)

![transitions, played](docs/screenshots/transitions-animated.png)

### In a window

Every demo also runs in a window on the Wayland desktop it is started
on -- Ferrix's hyprix, or any other: tole's `wayland` feature opens a
toplevel through Ferrix's own Wayland client runtime, pure Rust over the
socket, with no libwayland.

```sh
cargo run -p tole-examples -- counter --window
```

Only what changed is drawn again: a frame is kept between draws, each
item's drawing is compared with the last frame's, and the damaged parts
alone are drawn and handed to the compositor. A test holds the kept
frame to what drawing the whole scene gives, pixel for pixel, as a
rounded box wanders at scales 1, 1.5 and 2.

More come as the framework reaches them: the controls and the theme in
light and dark, a chart, and the lode viewer ported onto tole.

## Crates

* **`tole`**: the runtime. Property cells and bindings with dependency
  tracking, the item tree (`Item`, `Rectangle`, `Text`, `Row`, `Column`,
  `MouseArea`, `FocusScope`), `Grid`, `Flow`, `RowLayout`,
  `ColumnLayout`, `GridLayout` with `Layout.*`, `Repeater` and models,
  `Flickable`, `ListView` with virtualization, recycling and
  `DelegateChooser`, states, transitions, `Behavior` and animations on
  a frame clock,
  signals and handlers, the pointer's routing, focus, keys and the Tab
  order, damage and
  partial redraw into a kept `Frame`, the window (`wayland` feature),
  anchors, the scene as drawing operations, the `Renderer` trait and the
  CPU renderer, typed handles for each kind of item, and `include_ui!` for
  the compiler's output.
* **`tole-compiler`**: `.tole` files parsed, type-checked and written out
  as Rust from a program's `build.rs` (`tole_compiler::build("ui")`), put
  in place with `tole::include_ui!()`. Errors name the file, line and
  column. It is a build dependency only: nothing of it is in a program.
* **`tole-examples`** (`examples/`): the demos above.

## Building

tole is a component of [Ferrix](https://github.com/ferrix-os/ferrix): a
Ferrix tree checks it out at `src/user/system/linux/tole`
(`components.toml`), beside the compositor's workspace, and tole reaches
two of the compositor's crates by path from there: `text` (fonts, shaping
and glyphs, pure Rust over ttf-parser and rustybuzz) and, for the window
backend behind its `wayland` feature, `toolkit` (the Wayland client
runtime, pure Rust over the socket). Its tests draw with the tree's own
Inter from `assets/fonts`.

So to build it alone, put it where a Ferrix tree would:

```sh
git clone --filter=blob:none --sparse https://github.com/ferrix-os/ferrix
cd ferrix
git sparse-checkout set src/user/system/linux/compositor assets/fonts/inter
git clone https://github.com/ferrix-os/tole src/user/system/linux/tole
cd src/user/system/linux/tole
cargo test
```

which is what CI does. Inside a Ferrix tree, `cargo xtask check` runs
tole's formatting, clippy and tests with everything else.

## Changing it

A change to tole is two landings, as for every Ferrix component (Ferrix's
`docs/CONVENTIONS.md`, "Components live in repositories of their own"):
commit and push here, then pin the commit in Ferrix with `cargo xtask
pin-components` and land that. Commits name one author: no
`Co-authored-by:` trailer and no tool signature; CI refuses them.

## Licence

MIT, as Ferrix's compositor.
