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
pub const DEMOS: [Demo; 3] = [
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
];

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
    fn every_demo_draws_something() {
        for demo in &DEMOS {
            let png = render(demo, 1.0).expect("drawn");
            assert!(png.len() > 1000, "{}", demo.name);
        }
    }
}
