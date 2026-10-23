//! `Shape` and `Canvas` (`docs/UI-FRAMEWORK.md` §3.5): figures beyond
//! rounded boxes.
//!
//! A `Shape` holds `ShapePath`s written in the markup, each a path of
//! lines and curves with a fill and a stroke, bound like any property:
//! the boot picker's triangle, a skip mark. A `Canvas` is painted by a
//! Rust function the program gives it ([`CanvasPainter`]), handed a
//! [`Painter`] in the item's own logical coordinates: charts,
//! sparklines, a minimap, which are loops over data, Rust's work. A
//! canvas is painted again whenever a frame is made after something
//! changed, its size among them; a program whose data changed says so
//! with [`Ui::update_canvas`].

use std::fmt;
use std::rc::Rc;

use crate::item::{ItemId, Kind};
use crate::render::{DrawOp, Rect};
use crate::text::TextEngine;
use crate::ui::Ui;
use crate::value::{Color, Font, PropType, Value};

/// One step of a path, in logical pixels.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PathCmd {
    /// Start a new figure at (`x`, `y`).
    Move(f32, f32),
    /// A line to (`x`, `y`).
    Line(f32, f32),
    /// A quadratic curve: its control point, then its end.
    Quad(f32, f32, f32, f32),
    /// A cubic curve: its two control points, then its end.
    Cubic(f32, f32, f32, f32, f32, f32),
    /// Back to the figure's start.
    Close,
    /// A circle of its own: its centre and radius.
    Circle(f32, f32, f32),
}

/// How a line's ends are drawn.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum LineCap {
    /// Cut square at the end.
    Butt,
    /// Rounded past the end by half the width.
    #[default]
    Round,
    /// Squared past the end by half the width.
    Square,
}

/// How a stroke is drawn: its ends, and a dash.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct LineStyle {
    /// Its ends.
    pub cap: LineCap,
    /// Whether its corners are mitred rather than rounded.
    pub miter: bool,
    /// Dashes and gaps this long each, in logical pixels; 0 for a solid
    /// line.
    pub dash: f32,
}

/// A `ShapePath`'s `capStyle` and `joinStyle`, Qt's numbers
/// (`ShapePath.FlatCap` 0, `SquareCap` 1, `RoundCap` 2; `MiterJoin` 0,
/// `RoundJoin` 128; a bevel is drawn mitred), as a [`LineStyle`].
#[must_use]
pub fn shape_style(cap: i32, join: i32) -> LineStyle {
    LineStyle {
        cap: match cap {
            0 => LineCap::Butt,
            1 => LineCap::Square,
            _ => LineCap::Round,
        },
        miter: join != 128,
        dash: 0.0,
    }
}

/// A path: figures of lines and curves, and how a stroke of it is drawn.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Path {
    /// Its steps.
    cmds: Vec<PathCmd>,
    /// How it is stroked.
    style: LineStyle,
}

impl Path {
    /// An empty path.
    #[must_use]
    pub fn new() -> Self {
        Path::default()
    }

    /// Start a figure at (`x`, `y`).
    pub fn move_to(&mut self, x: f32, y: f32) -> &mut Self {
        self.cmds.push(PathCmd::Move(x, y));
        self
    }

    /// A line to (`x`, `y`).
    pub fn line_to(&mut self, x: f32, y: f32) -> &mut Self {
        self.cmds.push(PathCmd::Line(x, y));
        self
    }

    /// A quadratic curve through control (`cx`, `cy`) to (`x`, `y`).
    pub fn quad_to(&mut self, cx: f32, cy: f32, x: f32, y: f32) -> &mut Self {
        self.cmds.push(PathCmd::Quad(cx, cy, x, y));
        self
    }

    /// A cubic curve through two controls to (`x`, `y`).
    pub fn cubic_to(&mut self, c1: (f32, f32), c2: (f32, f32), x: f32, y: f32) -> &mut Self {
        self.cmds.push(PathCmd::Cubic(c1.0, c1.1, c2.0, c2.1, x, y));
        self
    }

    /// Close the figure.
    pub fn close(&mut self) -> &mut Self {
        self.cmds.push(PathCmd::Close);
        self
    }

    /// A circle of its own around (`cx`, `cy`).
    pub fn circle(&mut self, cx: f32, cy: f32, radius: f32) -> &mut Self {
        self.cmds.push(PathCmd::Circle(cx, cy, radius));
        self
    }

    /// The path through `points`, closed when `closed`.
    #[must_use]
    pub fn through(points: &[(f32, f32)], closed: bool) -> Self {
        let mut path = Path::new();
        for (i, &(x, y)) in points.iter().enumerate() {
            let _ = if i == 0 {
                path.move_to(x, y)
            } else {
                path.line_to(x, y)
            };
        }
        if closed && !points.is_empty() {
            let _ = path.close();
        }
        path
    }

    /// Stroke it so.
    pub fn set_style(&mut self, style: LineStyle) -> &mut Self {
        self.style = style;
        self
    }

    /// How it is stroked.
    #[must_use]
    pub fn style(&self) -> LineStyle {
        self.style
    }

    /// Its steps.
    #[must_use]
    pub fn cmds(&self) -> &[PathCmd] {
        &self.cmds
    }

    /// Whether it has no steps.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.cmds.is_empty()
    }

    /// The same moved by (`dx`, `dy`).
    #[must_use]
    pub fn translated(&self, dx: f32, dy: f32) -> Path {
        let cmds = self
            .cmds
            .iter()
            .map(|cmd| match *cmd {
                PathCmd::Move(x, y) => PathCmd::Move(x + dx, y + dy),
                PathCmd::Line(x, y) => PathCmd::Line(x + dx, y + dy),
                PathCmd::Quad(cx, cy, x, y) => PathCmd::Quad(cx + dx, cy + dy, x + dx, y + dy),
                PathCmd::Cubic(a, b, c, d, x, y) => {
                    PathCmd::Cubic(a + dx, b + dy, c + dx, d + dy, x + dx, y + dy)
                }
                PathCmd::Close => PathCmd::Close,
                PathCmd::Circle(x, y, r) => PathCmd::Circle(x + dx, y + dy, r),
            })
            .collect();
        Path {
            cmds,
            style: self.style,
        }
    }

    /// The smallest rectangle holding every point of it, control points
    /// included; `None` when it is empty.
    #[must_use]
    pub fn bounds(&self) -> Option<Rect> {
        let mut points = Vec::new();
        for cmd in &self.cmds {
            match *cmd {
                PathCmd::Move(x, y) | PathCmd::Line(x, y) => points.push((x, y)),
                PathCmd::Quad(cx, cy, x, y) => points.extend([(cx, cy), (x, y)]),
                PathCmd::Cubic(a, b, c, d, x, y) => points.extend([(a, b), (c, d), (x, y)]),
                PathCmd::Circle(x, y, r) => points.extend([(x - r, y - r), (x + r, y + r)]),
                PathCmd::Close => {}
            }
        }
        let (first, rest) = points.split_first()?;
        let (mut x0, mut y0, mut x1, mut y1) = (first.0, first.1, first.0, first.1);
        for &(x, y) in rest {
            x0 = x0.min(x);
            y0 = y0.min(y);
            x1 = x1.max(x);
            y1 = y1.max(y);
        }
        Some(Rect::new(x0, y0, x1 - x0, y1 - y0))
    }
}

/// One `ShapePath`: a path, its fill and its stroke.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ShapePathValue {
    /// The path, in the shape's coordinates.
    pub path: Path,
    /// What fills it; transparent for nothing.
    pub fill: Color,
    /// What strokes it; transparent for nothing.
    pub stroke: Color,
    /// The stroke's width.
    pub width: f32,
}

/// A `Shape`'s paths, as its `paths` property holds them.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ShapePaths(pub Rc<Vec<ShapePathValue>>);

impl ShapePaths {
    /// `paths`.
    #[must_use]
    pub fn new(paths: Vec<ShapePathValue>) -> Self {
        ShapePaths(Rc::new(paths))
    }
}

impl PropType for ShapePaths {
    fn into_value(self) -> Value {
        Value::custom(self)
    }
    fn from_value(value: &Value) -> Option<Self> {
        value.downcast::<Self>().cloned()
    }
    fn fallback() -> Self {
        ShapePaths::default()
    }
}

/// What a `Canvas` paints with: drawing calls in the canvas's own
/// logical coordinates.
pub struct Painter<'a> {
    /// The operations made, in the window's coordinates.
    ops: Vec<DrawOp>,
    /// Where the canvas is in the window.
    origin: (f32, f32),
    /// Its size.
    size: (f32, f32),
    /// Text, for labels.
    text: &'a mut TextEngine,
    /// Buffer pixels to a logical one.
    scale: f32,
}

impl fmt::Debug for Painter<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Painter")
            .field("ops", &self.ops.len())
            .field("size", &self.size)
            .finish_non_exhaustive()
    }
}

impl Painter<'_> {
    /// The canvas's width and height.
    #[must_use]
    pub fn size(&self) -> (f32, f32) {
        self.size
    }

    /// `rect` moved to the window.
    fn place(&self, rect: Rect) -> Rect {
        Rect::new(
            rect.x + self.origin.0,
            rect.y + self.origin.1,
            rect.w,
            rect.h,
        )
    }

    /// A rectangle filled, its corners rounded by `radius`.
    pub fn fill_rect(&mut self, rect: Rect, radius: f32, color: Color) {
        if !color.is_transparent() && rect.w > 0.0 && rect.h > 0.0 {
            let rect = self.place(rect);
            self.ops.push(DrawOp::Fill {
                rect,
                radius,
                color,
            });
        }
    }

    /// A rectangle's outline, `width` wide inside its edge.
    pub fn stroke_rect(&mut self, rect: Rect, radius: f32, width: f32, color: Color) {
        if !color.is_transparent() && width > 0.0 {
            let rect = self.place(rect);
            self.ops.push(DrawOp::Stroke {
                rect,
                radius,
                width,
                color,
                dash: 0.0,
            });
        }
    }

    /// `path` filled with `fill` and stroked `width` wide with `stroke`;
    /// a transparent colour does nothing.
    pub fn path(&mut self, path: &Path, fill: Color, stroke: Color, width: f32) {
        if path.is_empty() || (fill.is_transparent() && (stroke.is_transparent() || width <= 0.0)) {
            return;
        }
        self.ops.push(DrawOp::Path {
            path: Rc::new(path.translated(self.origin.0, self.origin.1)),
            fill,
            stroke,
            width,
        });
    }

    /// A line from (`x0`, `y0`) to (`x1`, `y1`).
    pub fn line(&mut self, (x0, y0): (f32, f32), (x1, y1): (f32, f32), width: f32, color: Color) {
        let mut path = Path::new();
        let _ = path.move_to(x0, y0).line_to(x1, y1);
        self.path(&path, Color::TRANSPARENT, color, width);
    }

    /// The line through `points`, `width` wide, drawn `style`.
    pub fn polyline(&mut self, points: &[(f32, f32)], width: f32, color: Color, style: LineStyle) {
        let mut path = Path::through(points, false);
        let _ = path.set_style(style);
        self.path(&path, Color::TRANSPARENT, color, width);
    }

    /// The polygon of `points`, filled.
    pub fn polygon(&mut self, points: &[(f32, f32)], color: Color) {
        self.path(&Path::through(points, true), color, Color::TRANSPARENT, 0.0);
    }

    /// A disc of `radius` around (`cx`, `cy`).
    pub fn circle(&mut self, (cx, cy): (f32, f32), radius: f32, color: Color) {
        let mut path = Path::new();
        let _ = path.circle(cx, cy, radius);
        self.path(&path, color, Color::TRANSPARENT, 0.0);
    }

    /// Cut what is painted next to `rect`, until [`Painter::pop_clip`].
    pub fn push_clip(&mut self, rect: Rect) {
        let rect = self.place(rect);
        self.ops.push(DrawOp::PushClip(rect));
    }

    /// End the last cut.
    pub fn pop_clip(&mut self) {
        self.ops.push(DrawOp::PopClip);
    }

    /// `text` in `font` on a line `height` tall whose top is `y`, centred
    /// down it as a `Text` is, from `x`; cut with `…` to `max` wide when
    /// given. Answers the width drawn.
    pub fn text_in(
        &mut self,
        text: &str,
        font: &Font,
        (x, y, height): (f32, f32, f32),
        color: Color,
        max: Option<f32>,
    ) -> f32 {
        if text.is_empty() {
            return 0.0;
        }
        let s = self.scale;
        let text: Rc<str> = Rc::from(text);
        let run = match max {
            Some(max) => self.text.run_within(font, s, &text, max * s),
            None => self.text.run(font, s, &text),
        };
        let width = run.width / s;
        if color.is_transparent() {
            return width;
        }
        let top = (self.origin.1 + y) * s;
        let baseline = top + (height * s - (run.ascent + run.descent)) / 2.0 + run.ascent;
        self.ops.push(DrawOp::Text {
            run,
            x: ((self.origin.0 + x) * s).round(),
            baseline: baseline.round(),
            color,
        });
        width
    }

    /// How wide `text` is in `font`.
    pub fn text_width(&mut self, text: &str, font: &Font) -> f32 {
        self.text.run(font, self.scale, &Rc::from(text)).width / self.scale
    }

    /// `text` in `font`, its baseline's start at (`x`, `baseline`).
    pub fn text(&mut self, text: &str, font: &Font, (x, baseline): (f32, f32), color: Color) {
        if text.is_empty() || color.is_transparent() {
            return;
        }
        let run = self.text.run(font, self.scale, &Rc::from(text));
        self.ops.push(DrawOp::Text {
            run,
            x: ((self.origin.0 + x) * self.scale).round(),
            baseline: ((self.origin.1 + baseline) * self.scale).round(),
            color,
        });
    }
}

/// A `Canvas`'s painting function, as its `painter` property holds it.
#[derive(Clone)]
pub struct CanvasPainter(Rc<dyn Fn(&mut Painter<'_>)>);

impl CanvasPainter {
    /// Paint with `paint`.
    pub fn new(paint: impl Fn(&mut Painter<'_>) + 'static) -> Self {
        CanvasPainter(Rc::new(paint))
    }
}

impl fmt::Debug for CanvasPainter {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("CanvasPainter")
    }
}

impl PartialEq for CanvasPainter {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.0, &other.0)
    }
}

impl Default for CanvasPainter {
    fn default() -> Self {
        CanvasPainter::new(|_| {})
    }
}

impl PropType for CanvasPainter {
    fn into_value(self) -> Value {
        Value::custom(self)
    }
    fn from_value(value: &Value) -> Option<Self> {
        value.downcast::<Self>().cloned()
    }
    fn fallback() -> Self {
        CanvasPainter::default()
    }
}

impl Ui {
    /// A `Canvas`'s own properties.
    #[must_use]
    pub fn canvas(&self, item: ItemId) -> Option<crate::item::CanvasProps> {
        match self.kind(item)? {
            Kind::Canvas(props) => Some(props),
            _ => None,
        }
    }

    /// A `Shape`'s own properties.
    #[must_use]
    pub fn shape(&self, item: ItemId) -> Option<crate::item::ShapeProps> {
        match self.kind(item)? {
            Kind::Shape(props) => Some(props),
            _ => None,
        }
    }

    /// Paint the `Canvas` `item` again at the next frame: its data
    /// changed.
    pub fn update_canvas(&mut self, item: ItemId) {
        if let Some(Kind::Canvas(props)) = self.kind(item) {
            let next = self.get(props.revision).wrapping_add(1);
            props.revision.set(self, next);
        }
    }

    /// A `Canvas`'s painting, in `place`.
    pub(crate) fn canvas_ops(
        &mut self,
        props: crate::item::CanvasProps,
        place: Rect,
        ops: &mut Vec<DrawOp>,
    ) {
        let painter = self.get(props.painter);
        let _ = self.get(props.revision);
        let scale = self.scale();
        let mut paint = Painter {
            ops: Vec::new(),
            origin: (place.x, place.y),
            size: (place.w, place.h),
            text: &mut self.text,
            scale,
        };
        (painter.0)(&mut paint);
        ops.extend(paint.ops);
    }

    /// A `Shape`'s paths, in `place`.
    pub(crate) fn shape_ops(
        &mut self,
        props: crate::item::ShapeProps,
        place: Rect,
        ops: &mut Vec<DrawOp>,
    ) {
        let paths = self.get(props.paths);
        for one in paths.0.iter() {
            if one.path.is_empty() {
                continue;
            }
            ops.push(DrawOp::Path {
                path: Rc::new(one.path.translated(place.x, place.y)),
                fill: one.fill,
                stroke: one.stroke,
                width: one.width,
            });
        }
    }

    /// A `Shape`'s implicit size: down and across to its paths' furthest
    /// point.
    pub(crate) fn shape_extent(&mut self, props: crate::item::ShapeProps) -> (f32, f32) {
        let paths = self.get(props.paths);
        paths
            .0
            .iter()
            .filter_map(|p| p.path.bounds().map(|b| (b, p.width)))
            .fold((0.0_f32, 0.0_f32), |(w, h), (b, width)| {
                (
                    w.max(b.right() + width / 2.0),
                    h.max(b.bottom() + width / 2.0),
                )
            })
    }
}
