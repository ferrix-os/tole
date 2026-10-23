//! The pointer: which `MouseArea` it is over, presses, holds and turns the
//! wheel on (`docs/UI-FRAMEWORK.md` §4.7).
//!
//! The topmost enabled area under the pointer, in paint order and inside
//! every clip around it, takes a press and holds the pointer until the
//! release, as Qt's grab does; the release is a click when the pointer is
//! still over it. Hover goes to the topmost area that asks for it. A
//! window host feeds the pointer in logical pixels; tests feed it the
//! same way.

use crate::item::{ItemId, Kind, MouseAreaProps};
use crate::render::Rect;
use crate::ui::Ui;
use crate::value::Value;

/// The pointer's state.
#[derive(Debug, Default)]
pub(crate) struct Input {
    /// Where it is, in the window; `None` outside.
    at: Option<(f32, f32)>,
    /// The area holding it since a press.
    grab: Option<ItemId>,
    /// The area it hovers.
    hover: Option<ItemId>,
}

impl Ui {
    /// The pointer moved to (`x`, `y`) in the window, logical pixels.
    pub fn pointer_move(&mut self, x: f32, y: f32) {
        self.update();
        self.input.at = Some((x, y));
        if let Some(grab) = self.input.grab {
            self.follow(grab, x, y);
        }
        self.rehover();
    }

    /// The pointer left the window.
    pub fn pointer_leave(&mut self) {
        self.update();
        self.input.at = None;
        self.rehover();
    }

    /// The button went down.
    pub fn pointer_press(&mut self) {
        self.update();
        let Some((x, y)) = self.input.at else {
            return;
        };
        let Some((item, area)) = self.areas_at(x, y).into_iter().last() else {
            return;
        };
        self.input.grab = Some(item);
        area.pressed.set(self, true);
        area.contains_mouse.set(self, true);
        self.follow(item, x, y);
        self.emit(area.on_pressed, Vec::new());
    }

    /// The button came up.
    pub fn pointer_release(&mut self) {
        self.update();
        let Some(item) = self.input.grab.take() else {
            return;
        };
        let Some(area) = self.mouse_area(item) else {
            return;
        };
        let inside = match (self.input.at, self.rect_of(item)) {
            (Some((x, y)), Some(rect)) => rect.contains(x, y),
            _ => false,
        };
        area.pressed.set(self, false);
        self.emit(area.released, Vec::new());
        if inside {
            self.emit(area.clicked, Vec::new());
        }
        if self.input.hover != Some(item) {
            area.contains_mouse.set(self, false);
        }
        self.rehover();
    }

    /// The wheel turned by `dy` logical pixels, positive down.
    pub fn pointer_wheel(&mut self, dy: f32) {
        self.update();
        let Some((x, y)) = self.input.at else {
            return;
        };
        if self.wheel_flick((x, y), 0.0, dy) {
            return;
        }
        if let Some((_, area)) = self.areas_at(x, y).into_iter().last() {
            self.emit(area.wheel, vec![Value::Real(dy)]);
        }
    }

    /// Forget `item` if the pointer held or hovered it.
    pub(crate) fn forget_input(&mut self, item: ItemId) {
        if self.input.grab == Some(item) {
            self.input.grab = None;
        }
        if self.input.hover == Some(item) {
            self.input.hover = None;
        }
    }

    /// Hover the topmost area that asks for it, and say so to the one it
    /// leaves and the one it enters.
    fn rehover(&mut self) {
        let next = match self.input.at {
            Some((x, y)) => {
                let areas = self.areas_at(x, y);
                let mut found = None;
                for (item, area) in areas.into_iter().rev() {
                    if self.get(area.hover_enabled) {
                        found = Some(item);
                        break;
                    }
                }
                found
            }
            None => None,
        };
        let before = self.input.hover;
        if before == next {
            if let (Some(item), Some((x, y))) = (next, self.input.at) {
                self.follow(item, x, y);
            }
            return;
        }
        self.input.hover = next;
        if let Some(item) = before
            && let Some(area) = self.mouse_area(item)
        {
            if self.input.grab != Some(item) {
                area.contains_mouse.set(self, false);
            }
            self.emit(area.exited, Vec::new());
        }
        if let Some(item) = next
            && let Some(area) = self.mouse_area(item)
        {
            area.contains_mouse.set(self, true);
            if let Some((x, y)) = self.input.at {
                self.follow(item, x, y);
            }
            self.emit(area.entered, Vec::new());
        }
    }

    /// Set `item`'s `mouseX` and `mouseY` from the window's (`x`, `y`).
    fn follow(&mut self, item: ItemId, x: f32, y: f32) {
        let (Some(area), Some(rect)) = (self.mouse_area(item), self.rect_of(item)) else {
            return;
        };
        area.mouse_x.set(self, x - rect.x);
        area.mouse_y.set(self, y - rect.y);
    }

    /// Where `item` is in the window.
    pub fn rect_of(&mut self, item: ItemId) -> Option<Rect> {
        let common = self.common(item)?;
        let (mut x, mut y) = (self.get(common.x), self.get(common.y));
        let (w, h) = (self.get(common.width), self.get(common.height));
        let mut at = self.parent(item);
        while let Some(parent) = at {
            let c = self.common(parent)?;
            x += self.get(c.x);
            y += self.get(c.y);
            at = self.parent(parent);
        }
        Some(Rect::new(x, y, w, h))
    }

    /// The enabled, visible areas under (`x`, `y`), in paint order:
    /// topmost last.
    fn areas_at(&mut self, x: f32, y: f32) -> Vec<(ItemId, MouseAreaProps)> {
        let mut out = Vec::new();
        for root in self.roots.clone() {
            self.areas_in(root, (0.0, 0.0), None, (x, y), &mut out);
        }
        out
    }

    /// [`Ui::areas_at`] under `item`. A disabled or hidden item hides its
    /// whole subtree from the pointer.
    fn areas_in(
        &mut self,
        item: ItemId,
        origin: (f32, f32),
        clip: Option<Rect>,
        point: (f32, f32),
        out: &mut Vec<(ItemId, MouseAreaProps)>,
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
        let inside_clip = clip.is_none_or(|clip| clip.contains(point.0, point.1));
        if let Some(Kind::MouseArea(area)) = self.kind(item)
            && inside_clip
            && rect.contains(point.0, point.1)
        {
            out.push((item, area));
        }
        let clip = if self.get(common.clip) {
            Some(clip.map_or(rect, |outer| outer.intersect(&rect)))
        } else {
            clip
        };
        for child in self.children(item).to_vec() {
            self.areas_in(child, (rect.x, rect.y), clip, point, out);
        }
    }
}
