//! tole's demos: each a small `.tole` program under `ui/`, compiled by
//! `build.rs`, and drawn here headless at its own size. `tole-examples
//! --all docs/screenshots` writes the README's screenshots; the tests fail
//! when one of them no longer matches what the demo draws.

use std::path::PathBuf;

use tole::{Color, Element, Fonts, Ui};

tole::include_ui!();

/// A demo: its name, and how to make it in a `Ui`.
#[derive(Clone, Copy, Debug)]
pub struct Demo {
    /// Its name: the screenshot's file name.
    pub name: &'static str,
    /// What it shows.
    pub about: &'static str,
    /// Make it, as a root, and do to it what its screenshot shows;
    /// answers its root's common properties.
    pub make: fn(&mut Ui) -> Option<tole::Common>,
}

/// Press and release the pointer on the item whose text is `label`, or
/// only move over it when `click` is false: how a demo's script works its
/// buttons without knowing where they are.
pub fn press(ui: &mut Ui, label: &str, click: bool) -> Option<()> {
    let item = find_text(ui, label)?;
    let rect = ui.rect_of(item)?;
    ui.pointer_move(rect.x + rect.w / 2.0, rect.y + rect.h / 2.0);
    if click {
        ui.pointer_press();
        ui.pointer_release();
    }
    Some(())
}

/// Press and release the key xkb names `key` (`"Tab"`, `"space"`,
/// `"Right"`), as a window hands it over.
pub fn key(ui: &mut Ui, key: &str) {
    let event = tole::KeyEvent::named(key);
    let _ = ui.key_press(&event);
    let _ = ui.key_release(&event);
}

/// The first `Text` item, anywhere, saying `label`.
pub fn find_text(ui: &mut Ui, label: &str) -> Option<tole::ItemId> {
    let mut stack: Vec<tole::ItemId> = ui.roots().to_vec();
    while let Some(item) = stack.pop() {
        if let Some(text) = ui.text(item)
            && &*ui.get(text.text) == label
        {
            return Some(item);
        }
        stack.extend(ui.children(item).iter().rev());
    }
    None
}

/// Every demo, in the README's order.
pub const DEMOS: [Demo; 12] = [
    Demo {
        name: "hello",
        about: "the smallest program",
        make: |ui| Hello::new(ui, None).map(|root| root.common()),
    },
    Demo {
        name: "layouts",
        about: "anchors, a Row and a Column",
        make: |ui| Layouts::new(ui, None).map(|root| root.common()),
    },
    Demo {
        name: "counter",
        about: "signals and handlers: three clicks, and the pointer over Reset",
        make: |ui| {
            let root = Counter::new(ui, None)?;
            for _ in 0..3 {
                press(ui, "+1", true)?;
            }
            press(ui, "Reset", false)?;
            Some(root.common())
        },
    },
    Demo {
        name: "focus",
        about: "focus and keys: Tab, Space, Right, Right, Space, Down",
        make: |ui| {
            let root = Focus::new(ui, None)?;
            for name in ["Tab", "space", "Right", "Right", "space", "Down"] {
                key(ui, name);
            }
            Some(root.common())
        },
    },
    Demo {
        name: "form",
        about: "RowLayout, ColumnLayout, GridLayout, Flow, Grid and Repeater",
        make: |ui| Form::new(ui, None).map(|root| root.common()),
    },
    Demo {
        name: "journal",
        about: "a ListView of 100,000 rows, scrolled to row 50,000",
        make: |ui| {
            let journal = Journal::new(ui, None)?;
            journal
                .entries
                .set(ui, tole::ModelRc::new(JournalRows::default()));
            let list = find_kind(ui, |kind| matches!(kind, tole::Kind::ListView(_)))?;
            ui.position_view_at_index(list, 50_000);
            Some(journal.common())
        },
    },
    Demo {
        name: "transitions",
        about: "states, a Transition, Behaviors and a looping animation, a second after the switch",
        make: |ui| {
            let root = Transitions::new(ui, None)?;
            ui.tick(0.0);
            click_at(ui, 430.0, 39.0);
            ui.update();
            ui.tick(1000.0);
            Some(root.common())
        },
    },
    Demo {
        name: "controls-light",
        about: "Fx.Controls in the light palette, a word double-clicked in a field",
        make: |ui| controls(ui, false),
    },
    Demo {
        name: "controls-dark",
        about: "the same, Theme.dark set",
        make: |ui| controls(ui, true),
    },
    Demo {
        name: "popups",
        about: "a ComboBox's list and a Menu open over a SplitView with a scrolled list",
        make: popups,
    },
    Demo {
        name: "file-chooser",
        about: "the FileDialog over a window, dark, on a folder kept for it",
        make: chooser,
    },
    Demo {
        name: "charts",
        about: "Canvas painters in Rust (histogram, sparkline, area) and Shapes in markup",
        make: charts,
    },
];

/// The charts demo: its three painters, Rust over made-up data, in the
/// theme's colours.
fn charts(ui: &mut Ui) -> Option<tole::Common> {
    use tole::{CanvasPainter, Path, Rect};
    let theme = fx_controls::Theme::instance(ui);
    theme.dark.set(ui, false);
    let (accent, fill, line, ink, red) = (
        theme.accent.get(ui),
        theme.accent_bg.get(ui),
        theme.line.get(ui),
        theme.ink4.get(ui),
        theme.red.get(ui),
    );
    let font = theme.sans(ui, 10.0, 400);
    let root = Charts::new(ui, None)?;
    // A wave with a bump: the same every run.
    let series = |n: usize, seed: f32| -> Vec<f32> {
        (0..n)
            .map(|i| {
                let t = i as f32 / n as f32;
                let wave = 0.55 + 0.25 * (t * 9.0 + seed).sin() + 0.12 * (t * 23.0 + seed).cos();
                let bump = (-((t - 0.7) * 9.0).powi(2)).exp() * 0.35;
                (wave + bump).clamp(0.05, 1.0)
            })
            .collect()
    };
    let bars = series(30, 0.3);
    root.histogram.set(
        ui,
        CanvasPainter::new(move |p| {
            let (w, h) = p.size();
            let base = h - 16.0;
            for k in 0..4 {
                let y = base - base * k as f32 / 3.0;
                p.line((0.0, y), (w, y), 1.0, line);
            }
            let step = w / bars.len() as f32;
            for (i, v) in bars.iter().enumerate() {
                let bar = (base - 4.0) * v;
                let color = if *v > 0.95 { red } else { accent };
                p.fill_rect(
                    Rect::new(i as f32 * step + 1.0, base - bar, step - 2.0, bar),
                    1.5,
                    color,
                );
            }
            for (i, label) in ["−30", "−20", "−10", "now"].iter().enumerate() {
                let x = (w - 18.0) * i as f32 / 3.0;
                p.text(label, &font, (x, h - 2.0), ink);
            }
        }),
    );
    let spark = series(40, 2.0);
    root.sparkline.set(
        ui,
        CanvasPainter::new(move |p| {
            let (w, h) = p.size();
            let mut path = Path::new();
            for (i, v) in spark.iter().enumerate() {
                let x = w * i as f32 / (spark.len() - 1) as f32;
                let y = h - 2.0 - (h - 4.0) * v;
                if i == 0 {
                    let _ = path.move_to(x, y);
                } else {
                    let _ = path.line_to(x, y);
                }
            }
            p.path(&path, Color::TRANSPARENT, red, 1.5);
        }),
    );
    let memory = series(60, 4.0);
    root.area.set(
        ui,
        CanvasPainter::new(move |p| {
            let (w, h) = p.size();
            let point =
                |i: usize, v: f32| (w * i as f32 / (memory.len() - 1) as f32, h - (h - 6.0) * v);
            let mut top = Path::new();
            let mut under = Path::new();
            let _ = under.move_to(0.0, h);
            for (i, v) in memory.iter().enumerate() {
                let (x, y) = point(i, *v);
                if i == 0 {
                    let _ = top.move_to(x, y);
                } else {
                    let _ = top.line_to(x, y);
                }
                let _ = under.line_to(x, y);
            }
            let _ = under.line_to(w, h).close();
            p.path(&under, fill, Color::TRANSPARENT, 0.0);
            p.path(&top, Color::TRANSPARENT, accent, 1.5);
        }),
    );
    Some(root.common())
}

/// The popups demo: the combo box opened by a click, the menu by the
/// program, an item of it under the pointer, and the list scrolled.
fn popups(ui: &mut Ui) -> Option<tole::Common> {
    let theme = fx_controls::Theme::instance(ui);
    theme.dark.set(ui, false);
    let root = Popups::new(ui, None)?;
    let list = find_kind(ui, |k| matches!(k, tole::Kind::ListView(_)))?;
    if let Some(flick) = ui.flickable(list) {
        flick.content_y.set(ui, 64.0);
    }
    press(ui, "pixel7", true)?;
    let menu = find_text(ui, "Copy record").and_then(|t| popup_of(ui, t))?;
    ui.open_popup(menu);
    press(ui, "Copy cursor", false)?;
    Some(root.common())
}

/// The file chooser demo: the dialog open on `fixtures/machines`, a key
/// chosen; the path field shows it as a person's home would.
fn chooser(ui: &mut Ui) -> Option<tole::Common> {
    let theme = fx_controls::Theme::instance(ui);
    theme.dark.set(ui, true);
    let root = Chooser::new(ui, None)?;
    let model = find_kind(ui, |k| matches!(k, tole::Kind::FolderListModel(_)))?;
    let folder = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("fixtures/machines/keys");
    let props = ui.folder_list(model)?;
    props
        .folder
        .set(ui, folder.to_string_lossy().as_ref().into());
    let dialog = popup_of(ui, model)?;
    ui.open_popup(dialog);
    ui.update();
    let field = find_input(ui, &folder.to_string_lossy())?;
    let text = ui.text_input(field)?;
    text.text.set(ui, "~/machines/keys".into());
    press(ui, "id_ed25519", true)?;
    ui.pointer_leave();
    Some(root.common())
}

/// The `Popup` around `item`.
pub fn popup_of(ui: &Ui, item: tole::ItemId) -> Option<tole::ItemId> {
    let mut at = ui.parent(item);
    while let Some(above) = at {
        if ui.popup(above).is_some() {
            return Some(above);
        }
        at = ui.parent(above);
    }
    None
}

/// The controls demo, light or dark, with "nazuna" double-clicked in its
/// first field.
fn controls(ui: &mut Ui, dark: bool) -> Option<tole::Common> {
    let theme = fx_controls::Theme::instance(ui);
    theme.dark.set(ui, dark);
    let root = Controls::new(ui, None)?;
    let field = find_input(ui, "nazuna.local")?;
    let rect = ui.rect_of(field)?;
    for _ in 0..2 {
        ui.pointer_move(rect.x + 12.0, rect.y + rect.h / 2.0);
        ui.pointer_press();
        ui.pointer_release();
    }
    ui.pointer_leave();
    Some(root.common())
}

/// The first `TextInput` or `TextEdit`, anywhere, holding `text`.
pub fn find_input(ui: &mut Ui, text: &str) -> Option<tole::ItemId> {
    let mut stack: Vec<tole::ItemId> = ui.roots().to_vec();
    while let Some(item) = stack.pop() {
        if let Some(input) = ui.text_input(item)
            && &*ui.get(input.text) == text
        {
            return Some(item);
        }
        stack.extend(ui.children(item).iter().rev());
    }
    None
}

/// Press and release the pointer at (`x`, `y`).
pub fn click_at(ui: &mut Ui, x: f32, y: f32) {
    ui.pointer_move(x, y);
    ui.pointer_press();
    ui.pointer_release();
    ui.pointer_leave();
}

/// The transitions demo as an animated PNG at scale 1: the switch turned
/// on, then off, 25 frames a second.
#[must_use]
pub fn transitions_animated() -> Option<Vec<u8>> {
    let mut ui = Ui::new(fonts());
    let root = Transitions::new(&mut ui, None)?;
    let (w, h) = (
        ui.get(root.common().width).round() as u32,
        ui.get(root.common().height).round() as u32,
    );
    let mut frames = Vec::new();
    for frame in 0..60_u32 {
        let t = f64::from(frame) * 40.0;
        if frame == 5 || frame == 35 {
            click_at(&mut ui, 430.0, 39.0);
        }
        ui.update();
        ui.tick(t);
        frames.push(ui.render((w, h), Color::hex(0xfa_fa_f8))?.data().to_vec());
    }
    Some(tole::png::encode_animated(w, h, &frames, 40))
}

/// The journal demo's rows: 100,000 of them, each made when asked for
/// and never kept.
#[derive(Debug, Default)]
pub struct JournalRows {
    /// Says nothing ever changes.
    tracker: tole::ModelTracker,
}

/// A row of the journal: a heading every 40 rows, records between.
const JOURNAL_ROWS: usize = 100_000;

impl tole::Model for JournalRows {
    type Row = Entry;

    fn row_count(&self) -> usize {
        JOURNAL_ROWS
    }

    fn row(&self, index: usize) -> Option<Entry> {
        if index >= JOURNAL_ROWS {
            return None;
        }
        let seconds = 8 * 3600 + index * 3;
        let time = format!(
            "{:02}:{:02}:{:02}",
            seconds / 3600 % 24,
            seconds / 60 % 60,
            seconds % 60
        );
        if index.is_multiple_of(40) {
            return Some(Entry {
                kind: EntryKind::Header,
                text: format!("Boot {} · {time}", index / 40 + 1).into(),
                ..Entry::default()
            });
        }
        const SOURCES: [&str; 6] = ["kernel", "hyprix", "netd", "lode", "authd", "sshd"];
        const TEXTS: [&str; 6] = [
            "link up at 1 Gbit/s, full duplex",
            "output HDMI-A-1: 2560x1440 at 120 Hz",
            "lease renewed for 192.168.9.193",
            "forwarding 1,204 records over TLS",
            "session opened for sebastian",
            "accepted publickey from 192.168.9.20 port 51022",
        ];
        let hash = index.wrapping_mul(2_654_435_761) >> 7;
        let level = match hash % 23 {
            0 => 3,
            1 | 2 => 4,
            _ => 6,
        };
        Some(Entry {
            kind: EntryKind::Record,
            level,
            time: time.into(),
            source: SOURCES
                .get(hash % SOURCES.len())
                .copied()
                .unwrap_or_default()
                .into(),
            text: format!(
                "{} (#{})",
                TEXTS
                    .get((hash / 7) % TEXTS.len())
                    .copied()
                    .unwrap_or_default(),
                tole::group(index as i32)
            )
            .into(),
        })
    }

    fn row_height(&self, index: usize) -> Option<f32> {
        Some(if index.is_multiple_of(40) { 34.0 } else { 24.0 })
    }

    fn tracker(&self) -> &tole::ModelTracker {
        &self.tracker
    }
}

/// The first item, anywhere, whose kind `matches` says.
pub fn find_kind(ui: &Ui, matches: impl Fn(&tole::Kind) -> bool) -> Option<tole::ItemId> {
    let mut stack: Vec<tole::ItemId> = ui.roots().to_vec();
    while let Some(item) = stack.pop() {
        if ui.kind(item).is_some_and(|kind| matches(&kind)) {
            return Some(item);
        }
        stack.extend(ui.children(item).iter().rev());
    }
    None
}

/// The fonts the demos draw with: `TOLE_FONTS` when set, else the Ferrix
/// tree's Inter beside this checkout, so a screenshot does not depend on
/// what the machine has installed; where there is no tree (a Ferrix image
/// running a demo), the system's, `/usr/share/fonts`.
#[must_use]
pub fn fonts() -> Fonts {
    if let Some(dir) = std::env::var_os("TOLE_FONTS") {
        let mut fonts = Fonts::new();
        let _ = fonts.add_dir(std::path::Path::new(&dir));
        return fonts;
    }
    let tree =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../../../../assets/fonts/inter");
    if tree.is_dir() {
        let mut fonts = Fonts::new();
        let _ = fonts.add_dir(&tree);
        return fonts;
    }
    Fonts::system()
}

/// `demo` in a window on the Wayland desktop (`$WAYLAND_DISPLAY`), until
/// it is closed.
///
/// # Errors
///
/// No compositor, or the connection failed.
pub fn run_window(demo: &Demo) -> Result<(), String> {
    let mut ui = Ui::new(fonts());
    let root = (demo.make)(&mut ui).ok_or("the demo was not made")?;
    let size = (
        ui.get(root.width).round() as u32,
        ui.get(root.height).round() as u32,
    );
    let options = tole::window::WindowOptions {
        title: format!("tole: {}", demo.name),
        app_id: "rocks.magical.tole-examples".to_owned(),
        size,
        background: Color::hex(0xff_ff_ff),
        ..tole::window::WindowOptions::default()
    };
    let mut window =
        tole::window::Window::open(root, options).map_err(|error| error.to_string())?;
    let mut up = false;
    while !window.is_closed() {
        let _ = window
            .dispatch(&mut ui, None)
            .map_err(|error| error.to_string())?;
        // Said once, for a boot that waits for the window.
        if !up && let Some((w, h)) = window.size() {
            up = true;
            use std::io::Write as _;
            let mut out = std::io::stdout();
            let _ = writeln!(out, "tole-examples: {} is up, {w}x{h}", demo.name);
            let _ = out.flush();
        }
    }
    Ok(())
}

/// `demo` drawn at `scale`, as a PNG file's bytes.
#[must_use]
pub fn render(demo: &Demo, scale: f32) -> Option<Vec<u8>> {
    let mut ui = Ui::new(fonts());
    ui.set_scale(scale);
    let root = (demo.make)(&mut ui)?;
    let width = ui.get(root.width).round().max(1.0) as u32;
    let height = ui.get(root.height).round().max(1.0) as u32;
    let frame = ui.render((width, height), Color::TRANSPARENT)?;
    Some(tole::png::encode(
        frame.width(),
        frame.height(),
        frame.data(),
    ))
}

#[cfg(test)]
mod tests {
    use super::{DEMOS, render};

    #[test]
    fn the_screenshots_are_what_the_demos_draw() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../docs/screenshots");
        for demo in &DEMOS {
            let drawn = render(demo, 2.0).expect("drawn");
            let kept = std::fs::read(dir.join(format!("{}.png", demo.name))).unwrap_or_default();
            assert!(
                drawn == kept,
                "docs/screenshots/{}.png is not what the demo draws: run `cargo run -p tole-examples -- --all docs/screenshots`",
                demo.name
            );
        }
    }

    #[test]
    fn the_counter_counts_and_says_so() {
        use super::{Counter, press};
        let mut ui = tole::Ui::new(super::fonts());
        let counter = Counter::new(&mut ui, None).expect("made");
        press(&mut ui, "+1", true).expect("pressed");
        press(&mut ui, "+1", true).expect("pressed");
        assert_eq!(counter.count.get(&mut ui), 2);
        assert!(super::find_text(&mut ui, "Clicked 2 times").is_some());
        press(&mut ui, "−1", true).expect("pressed");
        press(&mut ui, "−1", true).expect("pressed");
        press(&mut ui, "−1", true).expect("pressed");
        assert_eq!(counter.count.get(&mut ui), 0, "the `if` keeps it at nought");
    }

    #[test]
    fn the_program_hears_a_signal_through_the_generated_enum() {
        use super::{Button, ButtonSignal, press};
        use tole::Element;
        let mut ui = tole::Ui::new(super::fonts());
        let button = Button::new(&mut ui, None).expect("made");
        button.label.set(&mut ui, "Go".into());
        press(&mut ui, "Go", true).expect("pressed");
        let heard: Vec<ButtonSignal> = ui
            .take_signals()
            .iter()
            .filter_map(|e| button.signal(e))
            .collect();
        assert_eq!(heard, [ButtonSignal::Clicked]);
        // Its width follows its label.
        let narrow = ui.get(button.common().width);
        button.label.set(&mut ui, "A longer label".into());
        assert!(ui.get(button.common().width) > narrow + 40.0);
    }

    #[test]
    fn tab_arrows_and_space_work_the_tiles() {
        use super::{Focus, key};
        let mut ui = tole::Ui::new(super::fonts());
        let _focus = Focus::new(&mut ui, None).expect("made");
        let label_of_focus = |ui: &mut tole::Ui| {
            let item = ui.active_focus_item().expect("something has the focus");
            let label = ui.children(item)[0];
            let text = ui.text(label).expect("a tile's first child is its label");
            ui.get(text.text).to_string()
        };
        key(&mut ui, "Tab");
        assert_eq!(label_of_focus(&mut ui), "Wi-Fi");
        key(&mut ui, "Tab");
        key(&mut ui, "Tab");
        key(&mut ui, "Tab");
        assert_eq!(label_of_focus(&mut ui), "Dark", "tree order");
        key(&mut ui, "Up");
        assert_eq!(label_of_focus(&mut ui), "Wi-Fi", "KeyNavigation.up");
        key(&mut ui, "ISO_Left_Tab");
        assert_eq!(label_of_focus(&mut ui), "Quiet", "Shift+Tab wraps");
        // A click gives the focus and toggles.
        super::press(&mut ui, "Bluetooth", true).expect("clicked");
        assert_eq!(label_of_focus(&mut ui), "Bluetooth");
        let on = |ui: &mut tole::Ui| {
            let item = ui.active_focus_item().expect("focus");
            let rect = ui.rectangle(item).expect("a tile is a rectangle");
            ui.get(rect.color) == tole::Color::hex(0x2a62c9)
        };
        assert!(on(&mut ui));
        key(&mut ui, "space");
        assert!(!on(&mut ui));
    }

    #[test]
    fn the_journal_makes_a_screenful_of_its_hundred_thousand_rows() {
        let mut ui = tole::Ui::new(super::fonts());
        let root = (DEMOS[5].make)(&mut ui).expect("made");
        let _ = ui.render((600, 420), tole::Color::TRANSPARENT);
        let list = super::find_kind(&ui, |k| matches!(k, tole::Kind::ListView(_))).expect("a list");
        let (held, made) = ui.view_delegates(list).expect("a view");
        assert!(held < 30 && made < 30, "{held} held, {made} made");
        assert!(super::find_text(&mut ui, "100,000 entries").is_some());
        // Scrolled through a thousand rows by the wheel, it makes no more
        // than it holds.
        ui.pointer_move(300.0, 200.0);
        for _ in 0..1000 {
            ui.pointer_wheel(8.0);
        }
        let (_, after) = ui.view_delegates(list).expect("a view");
        assert!(after < 40, "{after} made");
        let _ = root;
    }

    #[test]
    fn the_animation_is_what_the_demo_plays() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../docs/screenshots/transitions-animated.png");
        let kept = std::fs::read(path).unwrap_or_default();
        assert!(
            super::transitions_animated().expect("played") == kept,
            "docs/screenshots/transitions-animated.png is stale: run `cargo run -p tole-examples -- --all docs/screenshots`"
        );
    }

    #[test]
    fn the_switch_slides_and_the_state_follows() {
        let mut ui = tole::Ui::new(super::fonts());
        let root = super::Transitions::new(&mut ui, None).expect("made");
        let panel = super::find_text(&mut ui, "The panel")
            .and_then(|t| ui.parent(t))
            .expect("panel");
        let x = |ui: &mut tole::Ui| ui.get(ui.common(panel).expect("lives").x);
        assert_eq!(x(&mut ui), 440.0);
        ui.tick(0.0);
        super::click_at(&mut ui, 430.0, 39.0);
        ui.update();
        ui.tick(250.0);
        let mid = x(&mut ui);
        assert!(mid < 300.0 && mid > 200.0, "on its way: {mid}");
        ui.tick(600.0);
        assert_eq!(x(&mut ui), 220.0);
        assert!(ui.animating(), "the dot pulses for ever");
        let _ = root;
    }

    #[test]
    fn radio_buttons_beside_each_other_are_exclusive() {
        use super::fx_controls::RadioButton;
        use tole::Element;
        let mut ui = tole::Ui::new(super::fonts());
        let column = tole::handles::Column::new(&mut ui, None).expect("made");
        let mut radios = Vec::new();
        for label in ["Light", "Dark", "System"] {
            let radio = RadioButton::new(&mut ui, Some(column.id())).expect("made");
            radio.text.set(&mut ui, label.into());
            radios.push(radio);
        }
        let other = RadioButton::new(&mut ui, None).expect("a radio button elsewhere");
        other.checked.set(&mut ui, true);
        radios[1].checked.set(&mut ui, true);
        ui.update();
        super::press(&mut ui, "System", true).expect("clicked");
        ui.update();
        let checked: Vec<bool> = radios.iter().map(|r| r.checked.get(&mut ui)).collect();
        assert_eq!(checked, [false, false, true]);
        assert!(
            other.checked.get(&mut ui),
            "another parent's is not touched"
        );
        // Space on the focused one checks it, and the others follow.
        ui.force_active_focus(radios[0].id());
        super::key(&mut ui, "space");
        ui.update();
        let checked: Vec<bool> = radios.iter().map(|r| r.checked.get(&mut ui)).collect();
        assert_eq!(checked, [true, false, false]);
    }

    #[test]
    fn check_boxes_switches_buttons_and_tabs_answer() {
        use super::fx_controls::{Button, ButtonSignal, CheckBox, Switch, TabBar, Theme};
        use tole::Element;
        let mut ui = tole::Ui::new(super::fonts());
        let column = tole::handles::Column::new(&mut ui, None).expect("made");
        let check = CheckBox::new(&mut ui, Some(column.id())).expect("made");
        check.text.set(&mut ui, "Wrap".into());
        let switch = Switch::new(&mut ui, Some(column.id())).expect("made");
        switch.text.set(&mut ui, "Live".into());
        let button = Button::new(&mut ui, Some(column.id())).expect("made");
        button.text.set(&mut ui, "Go".into());
        let tabs = TabBar::new(&mut ui, Some(column.id())).expect("made");
        tabs.tabs
            .set(&mut ui, vec!["One".into(), "Two".into(), "Three".into()]);
        super::press(&mut ui, "Wrap", true).expect("clicked");
        assert!(check.checked.get(&mut ui));
        super::press(&mut ui, "Live", true).expect("clicked");
        assert!(switch.checked.get(&mut ui));
        let _ = ui.take_signals();
        super::press(&mut ui, "Go", true).expect("clicked");
        ui.force_active_focus(button.id());
        super::key(&mut ui, "Return");
        let heard = ui
            .take_signals()
            .iter()
            .filter(|e| button.signal(e) == Some(ButtonSignal::Clicked))
            .count();
        assert_eq!(heard, 2, "a click and Return");
        super::press(&mut ui, "Three", true).expect("clicked");
        assert_eq!(tabs.current_index.get(&mut ui), 2);
        ui.force_active_focus(tabs.id());
        super::key(&mut ui, "Left");
        super::key(&mut ui, "Left");
        super::key(&mut ui, "Left");
        assert_eq!(tabs.current_index.get(&mut ui), 0, "and not past the first");
        // The theme changes every control's colours at once.
        let light = ui.get(button.base.props.color);
        let theme = Theme::instance(&mut ui);
        theme.dark.set(&mut ui, true);
        let dark = ui.get(button.base.props.color);
        assert_ne!(light, dark);
        assert_eq!(dark, theme.bg.get(&mut ui));
        assert_eq!(Theme::instance(&mut ui), theme, "one per Ui");
    }

    #[test]
    fn a_text_field_is_typed_into_and_its_alias_follows() {
        use super::fx_controls::{TextField, TextFieldSignal};
        use tole::Element;
        let mut ui = tole::Ui::new(super::fonts());
        let field = TextField::new(&mut ui, None).expect("made");
        field.placeholder_text.set(&mut ui, "Host".into());
        let _ = ui.render((200, 30), tole::Color::TRANSPARENT);
        // A click beside the text focuses it too.
        let rect = ui.rect_of(field.id()).expect("placed");
        ui.pointer_move(rect.x + 4.0, rect.y + 4.0);
        ui.pointer_press();
        ui.pointer_release();
        for key in ["n", "a", "z", "u", "n", "a"] {
            super::key(&mut ui, key);
        }
        assert_eq!(
            &*field.text.get(&mut ui),
            "nazuna",
            "typed through the alias"
        );
        assert!(super::find_text(&mut ui, "Host").is_some());
        let shown = |ui: &mut tole::Ui| {
            let text = super::find_text(ui, "Host").expect("placeholder");
            ui.get(ui.common(text).expect("lives").visible)
        };
        assert!(!shown(&mut ui), "the placeholder hides once typed into");
        field.text.set(&mut ui, "".into());
        assert!(shown(&mut ui));
        super::key(&mut ui, "Return");
        assert!(
            ui.take_signals()
                .iter()
                .any(|e| field.signal(e) == Some(TextFieldSignal::Accepted))
        );
    }

    #[test]
    fn a_combo_box_opens_over_everything_and_a_press_outside_closes_it() {
        let mut ui = tole::Ui::new(super::fonts());
        let root = super::Popups::new(&mut ui, None).expect("made");
        let _ = ui.render((720, 420), tole::Color::TRANSPARENT);
        let shown = |ui: &mut tole::Ui, label: &str| {
            let text = super::find_text(ui, label).expect("there");
            let popup = super::popup_of(ui, text).expect("in a popup");
            ui.get(ui.common(popup).expect("lives").visible)
        };
        assert!(!shown(&mut ui, "n150"));
        super::press(&mut ui, "pixel7", true).expect("clicked");
        assert!(shown(&mut ui, "n150"));
        // Chosen: the list closes and the combo box says it.
        super::press(&mut ui, "dk1", true).expect("clicked");
        assert!(!shown(&mut ui, "n150"));
        assert!(super::find_text(&mut ui, "dk1").is_some());
        // A press outside closes it and does nothing else: "Actions"
        // under it is not clicked.
        super::press(&mut ui, "dk1", true).expect("clicked");
        assert!(shown(&mut ui, "n150"));
        super::press(&mut ui, "Actions", true).expect("clicked");
        assert!(!shown(&mut ui, "n150"));
        assert!(!shown(&mut ui, "Copy record"), "the press was the popup's");
        // Now Actions opens the menu; a click on an item closes it; Escape
        // closes it too.
        super::press(&mut ui, "Actions", true).expect("clicked");
        assert!(shown(&mut ui, "Copy record"));
        super::press(&mut ui, "Copy cursor", true).expect("clicked");
        assert!(!shown(&mut ui, "Copy record"));
        super::press(&mut ui, "Actions", true).expect("clicked");
        super::key(&mut ui, "Escape");
        assert!(!shown(&mut ui, "Copy record"));
        // Keys step the combo box.
        let combo = super::find_text(&mut ui, "dk1")
            .and_then(|t| ui.parent(t))
            .expect("the combo box");
        ui.force_active_focus(combo);
        super::key(&mut ui, "Down");
        let label = ui.children(combo)[0];
        let text = ui.text(label).expect("its label");
        assert_eq!(&*ui.get(text.text), "n150");
        let _ = root;
    }

    #[test]
    fn a_split_view_handle_drags_and_a_scroll_bar_scrolls() {
        let mut ui = tole::Ui::new(super::fonts());
        let _root = super::Popups::new(&mut ui, None).expect("made");
        let _ = ui.render((720, 420), tole::Color::TRANSPARENT);
        let split =
            super::find_kind(&ui, |k| matches!(k, tole::Kind::SplitView(_))).expect("split");
        let left = ui.children(split)[0];
        let width = |ui: &mut tole::Ui| ui.get(ui.common(left).expect("lives").width);
        assert_eq!(width(&mut ui), 220.0);
        ui.pointer_move(221.0, 300.0);
        assert_eq!(ui.take_cursor(), Some(tole::Cursor::ColResize));
        ui.pointer_press();
        ui.pointer_move(301.0, 300.0);
        ui.pointer_release();
        assert_eq!(width(&mut ui), 300.0);
        ui.pointer_move(1.0, 300.0);
        ui.pointer_press();
        ui.pointer_move(-500.0, 300.0);
        ui.pointer_release();
        // Not past the handle's own room: the press at 1 is no handle.
        assert_eq!(width(&mut ui), 300.0);
        // The bar: a drag to its foot scrolls the list to its end.
        let list = super::find_kind(&ui, |k| matches!(k, tole::Kind::ListView(_))).expect("list");
        let flick = ui.flickable(list).expect("flick");
        let bar_x = 300.0 - 6.0;
        ui.pointer_move(bar_x, 30.0);
        ui.pointer_press();
        ui.pointer_move(bar_x, 1000.0);
        ui.pointer_release();
        let end = ui.get(flick.content_y);
        let most = 40.0 * 28.0 - (420.0 - 16.0);
        assert!((end - most).abs() < 1.0, "{end} of {most}");
    }

    #[test]
    fn the_file_dialog_walks_folders_and_chooses_a_file() {
        use super::fx_controls::{FileDialog, FileDialogSignal};
        let mut ui = tole::Ui::new(super::fonts());
        let root = tole::handles::Item::new(&mut ui, None).expect("made");
        root.common.width.set(&mut ui, 640.0);
        root.common.height.set(&mut ui, 480.0);
        let dialog = FileDialog::new(&mut ui, Some(root.id)).expect("made");
        let dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("fixtures/machines");
        dialog
            .folder
            .set(&mut ui, dir.to_string_lossy().as_ref().into());
        ui.open_popup(tole::Element::id(&dialog));
        let _ = ui.render((640, 480), tole::Color::TRANSPARENT);
        assert!(
            super::find_text(&mut ui, "keys/").is_some(),
            "folders first, marked"
        );
        assert!(super::find_text(&mut ui, ".hidden").is_none());
        super::press(&mut ui, "Show hidden files", true).expect("clicked");
        let _ = ui.render((640, 480), tole::Color::TRANSPARENT);
        assert!(super::find_text(&mut ui, ".hidden").is_some());
        // A double click opens a folder.
        super::press(&mut ui, "keys/", true).expect("clicked");
        super::press(&mut ui, "keys/", true).expect("clicked");
        let _ = ui.render((640, 480), tole::Color::TRANSPARENT);
        assert!(super::find_text(&mut ui, "id_ed25519.pub").is_some());
        // Up goes back.
        super::press(&mut ui, "Up", true).expect("clicked");
        let _ = ui.render((640, 480), tole::Color::TRANSPARENT);
        assert!(super::find_text(&mut ui, "hosts.txt").is_some());
        let _ = ui.take_signals();
        ui.tick(5000.0);
        super::press(&mut ui, "hosts.txt", true).expect("clicked");
        super::press(&mut ui, "Open", true).expect("clicked");
        let chosen: Vec<FileDialogSignal> = ui
            .take_signals()
            .iter()
            .filter_map(|e| dialog.signal(e))
            .collect();
        let want = dir.join("hosts.txt").to_string_lossy().into_owned();
        assert_eq!(chosen, [FileDialogSignal::Accepted(want.into())]);
        assert!(!ui.get(tole::Element::common(&dialog).visible), "closed");
    }

    #[test]
    fn every_demo_draws_something() {
        for demo in &DEMOS {
            let png = render(demo, 1.0).expect("drawn");
            assert!(png.len() > 1000, "{}", demo.name);
        }
    }
}
