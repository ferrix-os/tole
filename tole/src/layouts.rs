//! `Grid`, `Flow` and the layouts -- `RowLayout`, `ColumnLayout`,
//! `GridLayout` with `Layout.*` -- as bindings (`docs/UI-FRAMEWORK.md`
//! §4.3).
//!
//! Where a `Row` places each child from the one before it, these place
//! their children together: one binding, the container's `solve`, reads
//! every child's size and `Layout.*` and the container's own size, and
//! answers each child's rectangle; each child's `x` and `y` (and, in a
//! layout, its `width` and `height`) read their own from it. The
//! container's implicit size is a binding of its own that reads only the
//! children, so a container sized by its content is no loop.
//!
//! The layouts follow Qt Quick Layouts: a child's preferred size is its
//! `Layout.preferredWidth`, else its implicit width, clamped to its
//! minimum and maximum; room left over goes to the children that fill,
//! and room short is taken from them first; across, a child fills or
//! keeps its preferred size, centred unless `Layout.alignment` says.

use std::rc::Rc;

use crate::item::{Common, ItemId, Kind};
use crate::render::Rect;
use crate::ui::{Prop, PropId, Ui};

/// Where a container put its children, by their place among those it
/// manages.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Solved(Rc<Vec<Rect>>);

impl crate::PropType for Solved {
    fn into_value(self) -> crate::Value {
        crate::Value::custom(self)
    }
    fn from_value(value: &crate::Value) -> Option<Self> {
        value.downcast::<Self>().cloned()
    }
    fn fallback() -> Self {
        Solved::default()
    }
}

/// `Qt.AlignLeft` and the rest, as `Layout.alignment` takes them.
pub mod align {
    /// `Qt.AlignLeft`.
    pub const LEFT: i32 = 0x1;
    /// `Qt.AlignRight`.
    pub const RIGHT: i32 = 0x2;
    /// `Qt.AlignHCenter`.
    pub const H_CENTER: i32 = 0x4;
    /// `Qt.AlignTop`.
    pub const TOP: i32 = 0x20;
    /// `Qt.AlignBottom`.
    pub const BOTTOM: i32 = 0x40;
    /// `Qt.AlignVCenter`.
    pub const V_CENTER: i32 = 0x80;
    /// `Qt.AlignCenter`.
    pub const CENTER: i32 = H_CENTER | V_CENTER;
}

/// An item's `Layout.*`: what a layout reads of it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LayoutProps {
    /// `Layout.fillWidth`.
    pub fill_width: Prop<bool>,
    /// `Layout.fillHeight`.
    pub fill_height: Prop<bool>,
    /// `Layout.preferredWidth`; below 0 for the implicit width.
    pub preferred_width: Prop<f32>,
    /// `Layout.preferredHeight`; below 0 for the implicit height.
    pub preferred_height: Prop<f32>,
    /// `Layout.minimumWidth`.
    pub minimum_width: Prop<f32>,
    /// `Layout.minimumHeight`.
    pub minimum_height: Prop<f32>,
    /// `Layout.maximumWidth`.
    pub maximum_width: Prop<f32>,
    /// `Layout.maximumHeight`.
    pub maximum_height: Prop<f32>,
    /// `Layout.margins`.
    pub margins: Prop<f32>,
    /// `Layout.leftMargin`; below 0 for `margins`.
    pub left_margin: Prop<f32>,
    /// `Layout.rightMargin`; below 0 for `margins`.
    pub right_margin: Prop<f32>,
    /// `Layout.topMargin`; below 0 for `margins`.
    pub top_margin: Prop<f32>,
    /// `Layout.bottomMargin`; below 0 for `margins`.
    pub bottom_margin: Prop<f32>,
    /// `Layout.alignment`: [`align`]'s flags, 0 for the layout's own.
    pub alignment: Prop<i32>,
}

impl LayoutProps {
    /// Its cells.
    pub(crate) fn props(&self) -> [PropId; 14] {
        [
            self.fill_width.id,
            self.fill_height.id,
            self.preferred_width.id,
            self.preferred_height.id,
            self.minimum_width.id,
            self.minimum_height.id,
            self.maximum_width.id,
            self.maximum_height.id,
            self.margins.id,
            self.left_margin.id,
            self.right_margin.id,
            self.top_margin.id,
            self.bottom_margin.id,
            self.alignment.id,
        ]
    }
}

impl Ui {
    /// `item`'s `Layout.*`, made the first time it is asked for.
    pub fn layout_of(&mut self, item: ItemId) -> Option<LayoutProps> {
        if let Some(layout) = self.item(item)?.layout {
            return Some(layout);
        }
        let layout = LayoutProps {
            fill_width: self.prop(false),
            fill_height: self.prop(false),
            preferred_width: self.prop(-1.0_f32),
            preferred_height: self.prop(-1.0_f32),
            minimum_width: self.prop(0.0_f32),
            minimum_height: self.prop(0.0_f32),
            maximum_width: self.prop(f32::INFINITY),
            maximum_height: self.prop(f32::INFINITY),
            margins: self.prop(0.0_f32),
            left_margin: self.prop(-1.0_f32),
            right_margin: self.prop(-1.0_f32),
            top_margin: self.prop(-1.0_f32),
            bottom_margin: self.prop(-1.0_f32),
            alignment: self.prop(0_i32),
        };
        self.item_mut(item)?.layout = Some(layout);
        // The parent's layout read no `Layout.*` of it before: bind again.
        if let Some(parent) = self.parent(item) {
            self.position_children(parent);
        }
        Some(layout)
    }

    /// The children a container places: every child but a `Repeater`,
    /// which makes its delegates beside itself instead, a `Popup`, which
    /// is placed where it says over everything, and a model.
    pub(crate) fn placed_children(&self, parent: ItemId) -> Vec<(ItemId, Common)> {
        self.children(parent)
            .iter()
            .filter(|&&child| {
                !matches!(
                    self.kind(child),
                    Some(
                        Kind::Repeater(_)
                            | Kind::Popup(_)
                            | Kind::FolderListModel(_)
                            | Kind::FileInfo(_)
                    )
                )
            })
            .filter_map(|&child| Some((child, self.common(child)?)))
            .collect()
    }

    /// Bind the places of a `Grid`'s, `Flow`'s or layout's children;
    /// `false` when `parent` is none of them.
    pub(crate) fn place_together(&mut self, parent: ItemId) -> bool {
        let Some(own) = self.common(parent) else {
            return false;
        };
        let (solve, cross, shape) = match self.kind(parent) {
            Some(Kind::Grid(p)) => (p.solve, p.cross, Shape::Grid(p, false)),
            Some(Kind::GridLayout(p)) => (p.solve, p.cross, Shape::Grid(p, true)),
            Some(Kind::Flow(p)) => (p.solve, p.cross, Shape::Flow(p.spacing)),
            Some(Kind::RowLayout(p)) => (p.solve, p.cross, Shape::Line(p.spacing, true, false)),
            Some(Kind::ColumnLayout(p)) => (p.solve, p.cross, Shape::Line(p.spacing, false, false)),
            Some(Kind::SplitView(p)) => (
                p.flow.solve,
                p.flow.cross,
                Shape::Line(p.flow.spacing, !p.vertical, true),
            ),
            _ => return false,
        };
        let kids: Rc<Vec<(Common, Option<LayoutProps>)>> = Rc::new(
            self.placed_children(parent)
                .into_iter()
                .map(|(id, common)| (common, self.item(id).and_then(|data| data.layout)))
                .collect(),
        );
        let sizes = shape.sizes_children();
        if let Shape::Flow(spacing) = shape {
            self.bind_flow(own, solve, spacing, &kids);
            return true;
        }
        // Each way on its own: across reads only widths, down only heights.
        for (prop, horizontal) in [(solve, true), (cross, false)] {
            let k = Rc::clone(&kids);
            prop.bind(self, move |ui| {
                let room = ui.get(if horizontal { own.width } else { own.height });
                Solved(Rc::new(shape.solve_axis(ui, &k, horizontal, Some(room)).0))
            });
            let k = Rc::clone(&kids);
            let implicit = if horizontal {
                own.implicit_width
            } else {
                own.implicit_height
            };
            implicit.bind(self, move |ui| shape.solve_axis(ui, &k, horizontal, None).1);
        }
        for (index, (child, _)) in kids.iter().enumerate() {
            let pick = move |ui: &mut Ui, from: Prop<Solved>, f: fn(&Rect) -> f32| {
                ui.get(from).0.get(index).map_or(0.0, f)
            };
            child.x.bind(self, move |ui| pick(ui, solve, |r| r.x));
            child.y.bind(self, move |ui| pick(ui, cross, |r| r.x));
            if sizes {
                child.width.bind(self, move |ui| pick(ui, solve, |r| r.w));
                child.height.bind(self, move |ui| pick(ui, cross, |r| r.w));
            }
        }
        true
    }

    /// A `Flow`: both ways at once, its height what it wraps to at its
    /// width.
    fn bind_flow(
        &mut self,
        own: Common,
        solve: Prop<Solved>,
        spacing: Prop<f32>,
        kids: &Rc<Vec<(Common, Option<LayoutProps>)>>,
    ) {
        let sizes =
            |ui: &mut Ui, kids: &[(Common, Option<LayoutProps>)]| -> Vec<Option<(f32, f32)>> {
                kids.iter()
                    .map(|(c, _)| {
                        ui.get(c.visible)
                            .then(|| (ui.get(c.width), ui.get(c.height)))
                    })
                    .collect()
            };
        let k = Rc::clone(kids);
        solve.bind(self, move |ui| {
            let width = ui.get(own.width);
            let gap = ui.get(spacing);
            let items = sizes(ui, &k);
            Solved(Rc::new(flow(&items, gap, width).0))
        });
        let k = Rc::clone(kids);
        own.implicit_width.bind(self, move |ui| {
            let gap = ui.get(spacing);
            let items = sizes(ui, &k);
            flow(&items, gap, f32::INFINITY).1.0
        });
        let k = Rc::clone(kids);
        own.implicit_height.bind(self, move |ui| {
            let width = ui.get(own.width);
            let gap = ui.get(spacing);
            let items = sizes(ui, &k);
            flow(&items, gap, width).1.1
        });
        for (index, (child, _)) in kids.iter().enumerate() {
            let pick =
                move |ui: &mut Ui, f: fn(&Rect) -> f32| ui.get(solve).0.get(index).map_or(0.0, f);
            child.x.bind(self, move |ui| pick(ui, |r| r.x));
            child.y.bind(self, move |ui| pick(ui, |r| r.y));
        }
    }
}

/// Which way a container places.
#[derive(Clone, Copy, Debug)]
enum Shape {
    /// A `Grid` (`false`) or a `GridLayout` (`true`).
    Grid(crate::item::GridProps, bool),
    /// A `Flow`, with its spacing.
    Flow(Prop<f32>),
    /// A `RowLayout` (`true`) or `ColumnLayout`, with its spacing, and
    /// whether every child fills it across (a `SplitView`'s do).
    Line(Prop<f32>, bool, bool),
}

/// One child as a layout sees it, along one axis.
#[derive(Clone, Copy, Debug, Default)]
struct Span {
    /// Its minimum.
    min: f32,
    /// What it would have.
    pref: f32,
    /// Its maximum.
    max: f32,
    /// Whether it takes room left over.
    fill: bool,
    /// The margin before it.
    before: f32,
    /// The margin after it.
    after: f32,
}

impl Span {
    /// The room it takes with its margins, at size `size`.
    fn outer(&self, size: f32) -> f32 {
        size + self.before + self.after
    }
}

/// A child along one axis, read inside a binding: whether it is shown,
/// its span, and its `Layout.alignment`.
fn read_axis(
    ui: &mut Ui,
    common: Common,
    layout: Option<LayoutProps>,
    (horizontal, actual): (bool, bool),
) -> (bool, Span, i32) {
    let shown = ui.get(common.visible);
    // A layout reads what a child asks for; a positioner, what it is.
    let implicit = ui.get(match (horizontal, actual) {
        (true, false) => common.implicit_width,
        (false, false) => common.implicit_height,
        (true, true) => common.width,
        (false, true) => common.height,
    });
    let Some(l) = layout else {
        let span = Span {
            min: 0.0,
            pref: implicit,
            max: f32::INFINITY,
            ..Span::default()
        };
        return (shown, span, 0);
    };
    let (pref, min, max, fill, before, after) = if horizontal {
        (
            l.preferred_width,
            l.minimum_width,
            l.maximum_width,
            l.fill_width,
            l.left_margin,
            l.right_margin,
        )
    } else {
        (
            l.preferred_height,
            l.minimum_height,
            l.maximum_height,
            l.fill_height,
            l.top_margin,
            l.bottom_margin,
        )
    };
    let margins = ui.get(l.margins);
    let side = |ui: &mut Ui, prop: Prop<f32>| {
        let own = ui.get(prop);
        if own >= 0.0 { own } else { margins }
    };
    let (min, max) = (ui.get(min), ui.get(max));
    let max = max.max(min);
    let given = ui.get(pref);
    let pref = if given >= 0.0 { given } else { implicit };
    let span = Span {
        min,
        pref: pref.clamp(min, max),
        max,
        fill: ui.get(fill),
        before: side(ui, before),
        after: side(ui, after),
    };
    (shown, span, ui.get(l.alignment))
}

/// Share out `room` among `spans` along one axis: each starts at its
/// preferred size; room over goes to those that fill, up to their
/// maximum; room short comes from those that fill, then the others, down
/// to their minimum.
fn share(spans: &[Span], room: f32) -> Vec<f32> {
    let mut sizes: Vec<f32> = spans.iter().map(|s| s.pref).collect();
    let used: f32 = spans
        .iter()
        .zip(&sizes)
        .map(|(s, size)| s.outer(*size))
        .sum();
    let mut left = room - used;
    let rounds = [true, false];
    if left > 0.0 {
        // Growing: only fillers, evenly, those at their maximum dropping out.
        for _ in 0..spans.len() + 1 {
            let open: Vec<usize> = (0..spans.len())
                .filter(|&i| spans.get(i).is_some_and(|s| s.fill))
                .filter(|&i| match (spans.get(i), sizes.get(i)) {
                    (Some(s), Some(size)) => *size < s.max,
                    _ => false,
                })
                .collect();
            if open.is_empty() || left <= 0.0 {
                break;
            }
            let each = left / open.len() as f32;
            for i in open {
                if let (Some(s), Some(size)) = (spans.get(i), sizes.get_mut(i)) {
                    let grown = (*size + each).min(s.max);
                    left -= grown - *size;
                    *size = grown;
                }
            }
        }
    } else if left < 0.0 {
        for fillers in rounds {
            for _ in 0..spans.len() + 1 {
                let open: Vec<usize> = (0..spans.len())
                    .filter(|&i| spans.get(i).is_some_and(|s| s.fill == fillers))
                    .filter(|&i| match (spans.get(i), sizes.get(i)) {
                        (Some(s), Some(size)) => *size > s.min,
                        _ => false,
                    })
                    .collect();
                if open.is_empty() || left >= 0.0 {
                    break;
                }
                let each = left / open.len() as f32;
                for i in open {
                    if let (Some(s), Some(size)) = (spans.get(i), sizes.get_mut(i)) {
                        let shrunk = (*size + each).max(s.min);
                        left -= shrunk - *size;
                        *size = shrunk;
                    }
                }
            }
        }
    }
    sizes
}

/// Where in `room` a child of `size` goes, by its alignment flags:
/// `start`, `end` and `middle` are the flags for its three places, and
/// `default` the flag used when none is given.
fn align_in(
    room: f32,
    size: f32,
    flags: i32,
    (start, end, middle): (i32, i32, i32),
    default: i32,
) -> f32 {
    let flags = if flags & (start | end | middle) == 0 {
        default
    } else {
        flags
    };
    if flags & end != 0 {
        room - size
    } else if flags & middle != 0 {
        (room - size) / 2.0
    } else {
        0.0
    }
}

impl Shape {
    /// Whether it sets its children's sizes, not only their places.
    fn sizes_children(self) -> bool {
        matches!(self, Shape::Grid(_, true) | Shape::Line(..))
    }

    /// Every child's place and size along one axis (as a [`Rect`]'s `x`
    /// and `w`; hidden ones empty), and the implicit size that way;
    /// `room` is the container's size that way, `None` to measure only.
    fn solve_axis(
        self,
        ui: &mut Ui,
        kids: &[(Common, Option<LayoutProps>)],
        horizontal: bool,
        room: Option<f32>,
    ) -> (Vec<Rect>, f32) {
        let actual = matches!(self, Shape::Grid(_, false));
        let children: Vec<(bool, Span, i32)> = kids
            .iter()
            .map(|(common, layout)| read_axis(ui, *common, *layout, (horizontal, actual)))
            .collect();
        let flags = if horizontal {
            (align::LEFT, align::RIGHT, align::H_CENTER)
        } else {
            (align::TOP, align::BOTTOM, align::V_CENTER)
        };
        match self {
            Shape::Line(spacing, across, _) if across == horizontal => {
                along(&children, ui.get(spacing), room)
            }
            Shape::Line(_, across, fills) => {
                // Across a line: a row's children centred down, a
                // column's at its left, unless they say.
                let default = if across { align::V_CENTER } else { align::LEFT };
                let children: Vec<(bool, Span, i32)> = children
                    .into_iter()
                    .map(|(shown, span, align)| {
                        (
                            shown,
                            Span {
                                fill: span.fill || fills,
                                ..span
                            },
                            align,
                        )
                    })
                    .collect();
                beside(&children, flags, default, room)
            }
            Shape::Grid(props, layout) => {
                let spacing = ui.get(props.spacing);
                let given = ui.get(if horizontal {
                    props.column_spacing
                } else {
                    props.row_spacing
                });
                let gap = if given >= 0.0 { given } else { spacing };
                let columns = usize::try_from(ui.get(props.columns)).unwrap_or(0);
                let default = if horizontal {
                    align::LEFT
                } else {
                    align::V_CENTER
                };
                grid_axis(
                    &children,
                    columns,
                    horizontal,
                    gap,
                    layout,
                    (flags, default),
                    room,
                )
            }
            Shape::Flow(_) => (Vec::new(), 0.0),
        }
    }
}

/// Along a `RowLayout` or `ColumnLayout`: one after another, sharing the
/// room.
fn along(children: &[(bool, Span, i32)], spacing: f32, room: Option<f32>) -> (Vec<Rect>, f32) {
    let shown: Vec<usize> = (0..children.len())
        .filter(|&i| children.get(i).is_some_and(|c| c.0))
        .collect();
    let gaps = shown.len().saturating_sub(1) as f32 * spacing;
    let spans: Vec<Span> = shown
        .iter()
        .filter_map(|&i| children.get(i))
        .map(|c| c.1)
        .collect();
    let implicit = spans.iter().map(|s| s.outer(s.pref)).sum::<f32>() + gaps;
    let Some(room) = room else {
        return (Vec::new(), implicit);
    };
    let sizes = share(&spans, room - gaps);
    let mut out = vec![Rect::default(); children.len()];
    let mut at = 0.0;
    for (k, &i) in shown.iter().enumerate() {
        let (Some(span), Some(&size)) = (spans.get(k), sizes.get(k)) else {
            continue;
        };
        let start = at + span.before;
        if let Some(slot) = out.get_mut(i) {
            *slot = Rect::new(start, 0.0, size, 0.0);
        }
        at = start + size + span.after + spacing;
    }
    (out, implicit)
}

/// Across a `RowLayout` or `ColumnLayout`: each child alone in the room.
fn beside(
    children: &[(bool, Span, i32)],
    flags: (i32, i32, i32),
    default: i32,
    room: Option<f32>,
) -> (Vec<Rect>, f32) {
    let implicit = children
        .iter()
        .filter(|c| c.0)
        .map(|c| c.1.outer(c.1.pref))
        .fold(0.0, f32::max);
    let Some(room) = room else {
        return (Vec::new(), implicit);
    };
    let out = children
        .iter()
        .map(|&(shown, span, align)| {
            if !shown {
                return Rect::default();
            }
            let inner = room - span.before - span.after;
            let size = if span.fill {
                inner.clamp(span.min, span.max)
            } else {
                span.pref
            };
            let at = span.before + align_in(inner, size, align, flags, default);
            Rect::new(at, 0.0, size, 0.0)
        })
        .collect();
    (out, implicit)
}

/// A `Grid` or `GridLayout` (`layout`) along one axis: its columns
/// (`horizontal`) or its rows, `columns` to a row (all in one row when 0).
fn grid_axis(
    children: &[(bool, Span, i32)],
    columns: usize,
    horizontal: bool,
    gap: f32,
    layout: bool,
    (flags, default): ((i32, i32, i32), i32),
    room: Option<f32>,
) -> (Vec<Rect>, f32) {
    let shown: Vec<usize> = (0..children.len())
        .filter(|&i| children.get(i).is_some_and(|c| c.0))
        .collect();
    let columns = if columns == 0 {
        shown.len().max(1)
    } else {
        columns
    };
    let lanes = if horizontal {
        columns.min(shown.len().max(1))
    } else {
        shown.len().div_ceil(columns)
    };
    let lane_of = |k: usize| if horizontal { k % columns } else { k / columns };
    // Each column (or row) as one span: its largest child's.
    let mut spans = vec![
        Span {
            max: f32::INFINITY,
            ..Span::default()
        };
        lanes
    ];
    for (k, &i) in shown.iter().enumerate() {
        let (Some(child), Some(lane)) = (children.get(i), spans.get_mut(lane_of(k))) else {
            continue;
        };
        lane.pref = lane.pref.max(child.1.outer(child.1.pref));
        lane.min = lane.min.max(child.1.outer(child.1.min));
        lane.fill |= child.1.fill;
    }
    let gaps = lanes.saturating_sub(1) as f32 * gap;
    let implicit = spans.iter().map(|s| s.pref).sum::<f32>() + gaps;
    let Some(room) = room else {
        return (Vec::new(), implicit);
    };
    let sizes: Vec<f32> = if layout {
        share(&spans, room - gaps)
    } else {
        spans.iter().map(|s| s.pref).collect()
    };
    let mut starts = Vec::with_capacity(sizes.len());
    let mut at = 0.0;
    for size in &sizes {
        starts.push(at);
        at += size + gap;
    }
    let mut out = vec![Rect::default(); children.len()];
    for (k, &i) in shown.iter().enumerate() {
        let lane = lane_of(k);
        let (Some(&(_, span, align)), Some(&start), Some(&cell)) =
            (children.get(i), starts.get(lane), sizes.get(lane))
        else {
            continue;
        };
        let rect = if layout {
            let inner = cell - span.before - span.after;
            let size = if span.fill {
                inner.clamp(span.min, span.max)
            } else {
                span.pref.min(inner.max(span.min))
            };
            let place = start + span.before + align_in(inner, size, align, flags, default);
            Rect::new(place, 0.0, size, 0.0)
        } else {
            Rect::new(start, 0.0, span.pref, 0.0)
        };
        if let Some(slot) = out.get_mut(i) {
            *slot = rect;
        }
    }
    (out, implicit)
}

/// A `Flow`: across, wrapping where the next child would pass `width`;
/// each child its size, `None` when hidden. The rectangles, and the
/// size it takes.
fn flow(items: &[Option<(f32, f32)>], spacing: f32, width: f32) -> (Vec<Rect>, (f32, f32)) {
    let mut rects = vec![Rect::default(); items.len()];
    let (mut x, mut y, mut line_height, mut widest) = (0.0_f32, 0.0_f32, 0.0_f32, 0.0_f32);
    let mut any_on_line = false;
    for (i, item) in items.iter().enumerate() {
        let Some((w, h)) = *item else {
            continue;
        };
        if any_on_line && x + w > width {
            x = 0.0;
            y += line_height + spacing;
            line_height = 0.0;
        }
        if let Some(slot) = rects.get_mut(i) {
            *slot = Rect::new(x, y, w, h);
        }
        widest = widest.max(x + w);
        x += w + spacing;
        line_height = line_height.max(h);
        any_on_line = true;
    }
    (
        rects,
        (widest, if any_on_line { y + line_height } else { 0.0 }),
    )
}
