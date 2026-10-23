//! tole: Ferrix's UI framework, after Qt Quick and QML
//! (`docs/UI-FRAMEWORK.md`).
//!
//! A [`Ui`] holds a retained tree of items (`Item`, `Rectangle`, `Text`,
//! `Row`, `Column`) whose properties are typed cells. A property is set,
//! or bound to a closure; a binding re-evaluates when anything it read
//! changes, so anchors and positioners are bindings too. A frame is the
//! tree as drawing operations ([`Ui::scene`]) played through a
//! [`Renderer`]; [`CpuRenderer`] draws with tiny-skia and the desktop's
//! own text, and [`Ui::render`] gives a whole frame as a pixmap, which is
//! what the headless tests and an app's `--render` use.
//!
//! The markup compiler (`tole-compiler`, step U2) writes Rust against this
//! crate's API: [`Ui::create`], [`Prop::set`], [`Prop::bind`],
//! [`Ui::anchor`].

mod cpu;
pub mod handles;
mod input;
mod item;
mod layout;
pub mod png;
mod render;
mod signal;
mod text;
mod ui;
mod value;

pub use cpu::CpuRenderer;
pub use handles::Element;
pub use item::{
    Common, ItemId, Kind, KindName, MouseAreaProps, PositionerProps, RectangleProps, TextProps,
};
pub use layout::{Anchors, Line};
pub use render::{DrawOp, Rect, Renderer, play};
pub use signal::{Emitted, Handler, SignalId};
pub use ui::{Prop, PropId, Ui};
pub use value::{Color, CustomValue, Elide, Font, HAlign, PropType, VAlign, Value};

/// The Rust the compiler generated from the program's `.tole` files
/// (`tole_compiler::build` in its `build.rs`), as a module of its own whose
/// items are used here.
#[macro_export]
macro_rules! include_ui {
    () => {
        #[allow(
            dead_code,
            unused,
            unreachable_pub,
            missing_docs,
            unused_qualifications,
            clippy::all,
            clippy::pedantic,
            reason = "code tole-compiler generated from the .tole files"
        )]
        mod tole_generated {
            include!(concat!(env!("OUT_DIR"), "/tole_ui.rs"));
        }
        pub use tole_generated::*;
    };
}

/// The fonts, for a program that makes its own [`Ui`].
pub use compositor_text::Fonts;
/// The rasteriser, for a program that draws its frames itself.
pub use tiny_skia;

impl Ui {
    /// How many font faces were found: none means text draws nothing.
    #[must_use]
    pub fn faces(&self) -> usize {
        self.text.faces()
    }

    /// One whole frame of `width` × `height` logical pixels at the current
    /// scale, cleared to `background`. `None` when no pixmap that large
    /// can be made.
    pub fn render(
        &mut self,
        (width, height): (u32, u32),
        background: Color,
    ) -> Option<tiny_skia::Pixmap> {
        let scale = self.scale();
        let pixels = |logical: u32| (logical as f32 * scale).round().max(1.0) as u32;
        let mut pixmap = tiny_skia::Pixmap::new(pixels(width), pixels(height))?;
        pixmap.fill(tiny_skia::Color::from_rgba8(
            background.r,
            background.g,
            background.b,
            background.a,
        ));
        let ops = self.scene();
        let mut target = pixmap.as_mut();
        let mut renderer = CpuRenderer::new(&mut target, &mut self.text.fonts, scale);
        play(&ops, &mut renderer);
        Some(pixmap)
    }
}

#[cfg(test)]
mod tests;
