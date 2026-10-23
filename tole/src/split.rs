//! `SplitView`: its children side by side (or one under another), placed
//! as a `RowLayout` (`ColumnLayout`) places them with `Layout.*`, a
//! handle between each two whose boundary the pointer drags.
//!
//! Dragging a handle gives the child before it that size as its
//! `Layout.preferredWidth` (`Height`), within its minimum and maximum,
//! and the child that fills takes what is left, as Qt Quick Controls'
//! `SplitView` does with `SplitView.fillWidth`. The handle is a line
//! `spacing` wide in `handleColor`, and takes the pointer a few pixels
//! either side of it.

use crate::item::{ItemId, Kind, SplitProps};
use crate::render::{DrawOp, Rect};
use crate::ui::Ui;

/// How far either side of a handle's line it takes the pointer.
const GRIP: f32 = 4.0;

/// The smallest a child is dragged to.
const SMALLEST: f32 = 8.0;

impl Ui {
    /// A `SplitView`'s own properties.
    #[must_use]
    pub fn split(&self, item: ItemId) -> Option<SplitProps> {
        match self.kind(item)? {
            Kind::SplitView(props) => Some(props),
            _ => None,
        }
    }

    /// Whether `item` is a `SplitView` whose children go down.
    pub(crate) fn split_vertical(&self, item: ItemId) -> bool {
        matches!(self.kind(item), Some(Kind::SplitView(p)) if p.vertical)
    }

    /// The handles of `split`: for each, the child before it, the line's
    /// start along the split (from the split's own start) and the child's
    /// size.
    fn split_handles(&mut self, split: ItemId) -> Vec<(ItemId, f32, f32)> {
        let vertical = self.split_vertical(split);
        let mut shown: Vec<(ItemId, f32, f32)> = Vec::new();
        for (id, c) in self.placed_children(split) {
            if !self.get(c.visible) {
                continue;
            }
            shown.push(if vertical {
                (id, self.get(c.y), self.get(c.height))
            } else {
                (id, self.get(c.x), self.get(c.width))
            });
        }
        shown
            .windows(2)
            .filter_map(|pair| match pair {
                [(before, at, size), _] => Some((*before, at + size, *size)),
                _ => None,
            })
            .collect()
    }

    /// The handle of `split` (at `rect` in the window) under `point`: the
    /// child before it and its size.
    pub(crate) fn split_handle_at(
        &mut self,
        split: ItemId,
        rect: Rect,
        (x, y): (f32, f32),
    ) -> Option<(ItemId, f32)> {
        let Some(Kind::SplitView(props)) = self.kind(split) else {
            return None;
        };
        let along = if props.vertical {
            y - rect.y
        } else {
            x - rect.x
        };
        let width = self.get(props.flow.spacing);
        self.split_handles(split)
            .into_iter()
            .find(|(_, at, _)| along >= at - GRIP && along <= at + width + GRIP)
            .map(|(child, _, size)| (child, size))
    }

    /// A handle dragged: `child`, the one before it, is to be `size`.
    pub(crate) fn split_drag(&mut self, child: ItemId, size: f32) {
        let Some(split) = self.parent(child) else {
            return;
        };
        let vertical = self.split_vertical(split);
        let Some(layout) = self.layout_of(child) else {
            return;
        };
        let (min, max, preferred, fill) = if vertical {
            (
                layout.minimum_height,
                layout.maximum_height,
                layout.preferred_height,
                layout.fill_height,
            )
        } else {
            (
                layout.minimum_width,
                layout.maximum_width,
                layout.preferred_width,
                layout.fill_width,
            )
        };
        let low = self.get(min).max(SMALLEST);
        let high = self.get(max).max(low);
        preferred.set(self, size.clamp(low, high));
        fill.set(self, false);
    }

    /// A `SplitView`'s handles' lines.
    pub(crate) fn split_ops(
        &mut self,
        item: ItemId,
        props: SplitProps,
        place: Rect,
        ops: &mut Vec<DrawOp>,
    ) {
        let color = self.get(props.handle_color);
        let width = self.get(props.flow.spacing);
        if color.is_transparent() || width <= 0.0 {
            return;
        }
        for (_, at, _) in self.split_handles(item) {
            let rect = if props.vertical {
                Rect::new(place.x, place.y + at, place.w, width)
            } else {
                Rect::new(place.x + at, place.y, width, place.h)
            };
            ops.push(DrawOp::Fill {
                rect,
                radius: 0.0,
                color,
            });
        }
    }
}
