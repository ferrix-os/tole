//! `Flickable` (`docs/UI-FRAMEWORK.md` §4.7): children on a content item
//! that moves by `-contentX`, `-contentY`, and the wheel that scrolls it.
//!
//! The content item is made with the `Flickable` and every child made
//! under the `Flickable` goes on it. Its size is `contentWidth` and
//! `contentHeight`, or, while they are below 0, the `Flickable`'s width
//! and the bottom of its lowest child. The wheel goes to the innermost
//! `Flickable` under the pointer that can still move that way, and to a
//! `MouseArea` only when none can.

use crate::item::{Common, FlickProps, ItemId, KindName};
use crate::render::Rect;
use crate::ui::Ui;

/// How many logical pixels a unit of the wheel scrolls.
const WHEEL_STEP: f32 = 3.0;

impl FlickProps {
    /// Its cells.
    pub(crate) fn props(&self) -> [crate::ui::PropId; 5] {
        [
            self.content_x.id,
            self.content_y.id,
            self.content_width.id,
            self.content_height.id,
            self.interactive.id,
        ]
    }
}

impl Ui {
    /// A new `Flickable`'s properties.
    pub(crate) fn flick_props(&mut self) -> FlickProps {
        FlickProps {
            content_x: self.prop(0.0_f32),
            content_y: self.prop(0.0_f32),
            content_width: self.prop(-1.0_f32),
            content_height: self.prop(-1.0_f32),
            interactive: self.prop(true),
        }
    }

    /// Make `flick`'s content item and bind it to the scroll.
    pub(crate) fn make_content(&mut self, flick: ItemId) {
        let (Some(props), Some(own)) = (self.flickable(flick), self.common(flick)) else {
            return;
        };
        let Some(content) = self.create(KindName::Item, Some(flick)) else {
            return;
        };
        if let Some(data) = self.item_mut(flick) {
            data.content = Some(content);
        }
        let Some(c) = self.common(content) else {
            return;
        };
        c.x.bind(self, move |ui| -ui.get(props.content_x));
        c.y.bind(self, move |ui| -ui.get(props.content_y));
        c.width.bind(self, move |ui| {
            let given = ui.get(props.content_width);
            if given >= 0.0 {
                given
            } else {
                ui.get(own.width)
            }
        });
        self.bind_content_height(flick);
    }

    /// Bind `flick`'s content item's height: `contentHeight`, or the
    /// bottom of its lowest child. Bound again when its children change.
    pub(crate) fn bind_content_height(&mut self, flick: ItemId) {
        let (Some(props), Some(content)) = (self.flickable(flick), self.content_item(flick)) else {
            return;
        };
        let Some(c) = self.common(content) else {
            return;
        };
        let children: Vec<Common> = self
            .children(content)
            .iter()
            .filter_map(|&child| self.common(child))
            .collect();
        c.height.bind(self, move |ui| {
            let given = ui.get(props.content_height);
            if given >= 0.0 {
                return given;
            }
            let mut bottom = 0.0_f32;
            for child in &children {
                if ui.get(child.visible) {
                    bottom = bottom.max(ui.get(child.y) + ui.get(child.height));
                }
            }
            bottom
        });
    }

    /// The `Flickable` whose content item `item` is, if it is one.
    pub(crate) fn flick_of_content(&self, item: ItemId) -> Option<ItemId> {
        let above = self.parent(item)?;
        (self.content_item(above) == Some(item)).then_some(above)
    }

    /// Scroll the innermost `Flickable` under the pointer that can move
    /// by `dy` (and `dx`); whether one did.
    pub(crate) fn wheel_flick(&mut self, (x, y): (f32, f32), dx: f32, dy: f32) -> bool {
        let mut found = Vec::new();
        for root in self.roots.clone() {
            self.flicks_in(root, (0.0, 0.0), None, (x, y), &mut found);
        }
        for flick in found.into_iter().rev() {
            let (Some(props), Some(own), Some(content)) = (
                self.flickable(flick),
                self.common(flick),
                self.content_item(flick).and_then(|c| self.common(c)),
            ) else {
                continue;
            };
            if !self.get(props.interactive) {
                continue;
            }
            let mut moved = false;
            for (delta, at, view, extent) in [
                (dy, props.content_y, own.height, content.height),
                (dx, props.content_x, own.width, content.width),
            ] {
                if delta == 0.0 {
                    continue;
                }
                let most = (self.get(extent) - self.get(view)).max(0.0);
                let now = self.get(at);
                let next = (now + delta * WHEEL_STEP).clamp(0.0, most);
                if (next - now).abs() > f32::EPSILON {
                    at.set(self, next);
                    moved = true;
                }
            }
            if moved {
                return true;
            }
        }
        false
    }

    /// The shown, enabled `Flickable`s under the point, outermost first.
    fn flicks_in(
        &mut self,
        item: ItemId,
        origin: (f32, f32),
        clip: Option<Rect>,
        point: (f32, f32),
        out: &mut Vec<ItemId>,
    ) {
        let Some(common) = self.common(item) else {
            return;
        };
        if !self.get(common.visible) || !self.get(common.enabled) {
            return;
        }
        let rect = Rect::new(
            origin.0 + self.get(common.x),
            origin.1 + self.get(common.y),
            self.get(common.width),
            self.get(common.height),
        );
        let inside = clip.is_none_or(|c| c.contains(point.0, point.1));
        if self.flickable(item).is_some() && inside && rect.contains(point.0, point.1) {
            out.push(item);
        }
        let clip = if self.get(common.clip) {
            Some(clip.map_or(rect, |outer| outer.intersect(&rect)))
        } else {
            clip
        };
        for child in self.children(item).to_vec() {
            self.flicks_in(child, (rect.x, rect.y), clip, point, out);
        }
    }
}
