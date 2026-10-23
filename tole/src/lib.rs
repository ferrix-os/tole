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
mod damage;
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
#[cfg(feature = "wayland")]
pub mod window;

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

/// A window's frame, kept between draws so that only what changed is
/// drawn again ([`Ui::render_into`]).
#[derive(Debug)]
pub struct Frame {
    /// The pixels, at the scale they were drawn at.
    pixmap: tiny_skia::Pixmap,
    /// Where damaged parts are drawn before they are copied in: the
    /// frame's size, so nothing drawn there is cut by an edge the whole
    /// frame does not have.
    scratch: tiny_skia::Pixmap,
    /// The scale.
    scale: f32,
    /// What the pixels show.
    scene: Option<damage::Scene>,
    /// What is under everything.
    background: Color,
}

impl Frame {
    /// A frame of `width` × `height` logical pixels at `scale`, cleared to
    /// `background`; `None` when no pixmap that large can be made.
    #[must_use]
    pub fn new((width, height): (u32, u32), scale: f32, background: Color) -> Option<Self> {
        let pixels = |logical: u32| (logical as f32 * scale).round().max(1.0) as u32;
        let pixmap = tiny_skia::Pixmap::new(pixels(width), pixels(height))?;
        let scratch = pixmap.clone();
        Some(Frame {
            pixmap,
            scratch,
            scale,
            scene: None,
            background,
        })
    }

    /// The pixels: premultiplied RGBA, tiny-skia's order.
    #[must_use]
    pub fn pixmap(&self) -> &tiny_skia::Pixmap {
        &self.pixmap
    }

    /// Its size in buffer pixels.
    #[must_use]
    pub fn pixel_size(&self) -> (u32, u32) {
        (self.pixmap.width(), self.pixmap.height())
    }
}

/// How many rectangles of damage a frame keeps apart before it draws
/// their bounding box instead.
const DAMAGE_RECTS: usize = 8;

impl Ui {
    /// Draw into `frame` what changed since it was last drawn into, and
    /// answer those places in buffer pixels (`x`, `y`, `width`, `height`)
    /// -- empty when nothing changed. A frame drawn into for the first
    /// time is drawn whole, and answers the whole of it. The frame must be
    /// at the `Ui`'s scale; make a new one when the size or scale changes.
    pub fn render_into(&mut self, frame: &mut Frame) -> Vec<(u32, u32, u32, u32)> {
        let scale = self.scale();
        let scene = self.scene_with_items();
        let (width, height) = frame.pixel_size();
        let s = frame.scale.max(f32::EPSILON);
        let whole = Rect::new(0.0, 0.0, width as f32 / s, height as f32 / s);
        let logical = match (&frame.scene, (scale - frame.scale).abs() < f32::EPSILON) {
            (Some(before), true) => damage::merge(scene.damage_since(before), DAMAGE_RECTS),
            _ => vec![whole],
        };
        let mut pixels = Vec::new();
        for rect in logical {
            let rect = rect.intersect(&whole);
            let x0 = (rect.x * s).floor().max(0.0) as u32;
            let y0 = (rect.y * s).floor().max(0.0) as u32;
            let x1 = ((rect.right() * s).ceil() as u32).min(width);
            let y1 = ((rect.bottom() * s).ceil() as u32).min(height);
            if x1 > x0 && y1 > y0 {
                pixels.push((x0, y0, x1 - x0, y1 - y0));
            }
        }
        let background = frame.background;
        let fill =
            tiny_skia::Color::from_rgba8(background.r, background.g, background.b, background.a);
        for &(x, y, w, h) in &pixels {
            // The part is drawn on the scratch frame, of the whole frame's
            // size, with only the drawing that reaches it: nothing is cut
            // by an edge the whole frame lacks, so its pixels are the
            // whole frame's (tiny-skia draws a curve cut by a pixmap's
            // edge a little differently). Then the part alone is copied.
            let (Some(area), Some(cut)) = (
                tiny_skia::Rect::from_xywh(x as f32, y as f32, w as f32, h as f32),
                tiny_skia::IntRect::from_xywh(
                    i32::try_from(x).unwrap_or(0),
                    i32::try_from(y).unwrap_or(0),
                    w,
                    h,
                ),
            ) else {
                continue;
            };
            let mut paint = tiny_skia::Paint::default();
            paint.set_color(fill);
            paint.blend_mode = tiny_skia::BlendMode::Source;
            frame
                .scratch
                .fill_rect(area, &paint, tiny_skia::Transform::identity(), None);
            {
                let mut target = frame.scratch.as_mut();
                let mut renderer = CpuRenderer::new(&mut target, &mut self.text.fonts, s);
                let logical = Rect::new(x as f32 / s, y as f32 / s, w as f32 / s, h as f32 / s);
                damage::play_culled(&scene.ops, &mut renderer, logical, s);
            }
            let Some(part) = frame.scratch.clone_rect(cut) else {
                continue;
            };
            let copy = tiny_skia::PixmapPaint {
                blend_mode: tiny_skia::BlendMode::Source,
                ..tiny_skia::PixmapPaint::default()
            };
            frame.pixmap.draw_pixmap(
                cut.x(),
                cut.y(),
                part.as_ref(),
                &copy,
                tiny_skia::Transform::identity(),
                None,
            );
        }
        frame.scene = Some(scene);
        pixels
    }
}

#[cfg(test)]
mod tests;
