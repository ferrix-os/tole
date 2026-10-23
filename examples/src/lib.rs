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
pub const DEMOS: [Demo; 7] = [
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
];

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
/// what the machine has installed.
#[must_use]
pub fn fonts() -> Fonts {
    let mut fonts = Fonts::new();
    let dir = std::env::var_os("TOLE_FONTS").map_or_else(
        || PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../../../../assets/fonts/inter"),
        PathBuf::from,
    );
    let _ = fonts.add_dir(&dir);
    fonts
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
    };
    let mut window =
        tole::window::Window::open(root, options).map_err(|error| error.to_string())?;
    while !window.is_closed() {
        let _ = window
            .dispatch(&mut ui, None)
            .map_err(|error| error.to_string())?;
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
    fn every_demo_draws_something() {
        for demo in &DEMOS {
            let png = render(demo, 1.0).expect("drawn");
            assert!(png.len() > 1000, "{}", demo.name);
        }
    }
}
