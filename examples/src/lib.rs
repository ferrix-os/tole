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
    /// Make it, as a root; answers its root's common properties.
    pub make: fn(&mut Ui) -> Option<tole::Common>,
}

/// Every demo, in the README's order.
pub const DEMOS: [Demo; 2] = [
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
    fn every_demo_draws_something() {
        for demo in &DEMOS {
            let png = render(demo, 1.0).expect("drawn");
            assert!(png.len() > 1000, "{}", demo.name);
        }
    }
}
