//! The pointer: which `MouseArea` or text field it is over, presses,
//! holds and turns the wheel on (`docs/UI-FRAMEWORK.md` §4.7).
//!
//! The topmost enabled area or text field under the pointer, in paint
//! order and inside every clip around it, takes a press and holds the
//! pointer until the release, as Qt's grab does; the release is a click
//! when the pointer is still over it. A press soon after another in the
//! same place counts on (a double click, a triple). Hover goes to the
//! topmost area that asks for it. A text field pressed takes the active
//! focus and the caret; dragged, it selects. A window host feeds the
//! pointer in logical pixels; tests feed it the same way.

use crate::item::{ItemId, Kind, MouseAreaProps};
use crate::render::Rect;
use crate::ui::Ui;
use crate::value::Value;

/// How long after a press another counts on from it, in milliseconds of
/// the `Ui`'s clock.
const MULTI_CLICK_MS: f64 = 400.0;

/// How far from a press another may be and still count on from it.
const MULTI_CLICK_SLOP: f32 = 4.0;

/// The pointer's shape, as the item under it asks.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Cursor {
    /// The arrow.
    #[default]
    Default,
    /// A hand, over something to click.
    Pointer,
    /// The I-beam, over text to type into.
    Text,
    /// Across, over something that moves a boundary sideways.
    ColResize,
    /// Down, over something that moves a boundary up and down.
    RowResize,
}

/// `cursorShape`'s values, as Qt numbers them.
pub mod cursor_shape {
    /// `Qt.ArrowCursor`.
    pub const ARROW: i32 = 0;
    /// `Qt.IBeamCursor`.
    pub const IBEAM: i32 = 4;
    /// `Qt.SplitVCursor`.
    pub const SPLIT_V: i32 = 11;
    /// `Qt.SplitHCursor`.
    pub const SPLIT_H: i32 = 12;
    /// `Qt.PointingHandCursor`.
    pub const POINTING_HAND: i32 = 13;
}

/// What is under the pointer and takes it.
#[derive(Clone, Copy, Debug)]
enum Hit {
    /// A `MouseArea`.
    Area(MouseAreaProps),
    /// A `TextInput` or `TextEdit`.
    Text,
    /// A `SplitView`'s handle: the child before it, and its size then.
    Split(ItemId, f32),
}

/// The pointer's state.
#[derive(Debug, Default)]
pub(crate) struct Input {
    /// Where it is, in the window; `None` outside.
    at: Option<(f32, f32)>,
    /// What holds it since a press.
    grab: Option<ItemId>,
    /// The area it hovers.
    hover: Option<ItemId>,
    /// The last press: when, where, and how many in a row.
    last_press: Option<(f64, f32, f32, u32)>,
    /// Whether Shift is held, as the window says.
    shift: bool,
    /// Whether Control is held, as the window says.
    control: bool,
    /// The popup being hit-tested now, over the rest.
    pub(crate) overlay: Option<ItemId>,
    /// The shape it has now.
    cursor: Cursor,
    /// Whether the shape changed since the window last asked.
    cursor_changed: bool,
    /// A `SplitView` handle being dragged: the child before it, its size
    /// when the press came, and where the press was along the split.
    split: Option<(ItemId, f32, f32)>,
    /// A view gave a delegate back to its pool since the pointer was last
    /// looked under: the area it hovers may now show another row, or be
    /// hidden, while the pointer has not moved ([`Ui::delegate_released`]).
    recycled: bool,
}

impl Ui {
    /// The pointer moved to (`x`, `y`) in the window, logical pixels.
    pub fn pointer_move(&mut self, x: f32, y: f32) {
        self.update();
        self.input.at = Some((x, y));
        if let Some(grab) = self.input.grab {
            if let Some((child, start, from)) = self.input.split {
                let along = if self.split_vertical(grab) { y } else { x };
                self.split_drag(child, start + along - from);
            }
            self.follow(grab, x, y, true);
        }
        self.rehover();
    }

    /// Where the pointer is in the window, logical pixels; `None` while it
    /// is not over it: what a panel that opens under the pointer and closes
    /// when it goes asks.
    #[must_use]
    pub fn pointer_at(&self) -> Option<(f32, f32)> {
        self.input.at
    }

    /// The pointer left the window.
    pub fn pointer_leave(&mut self) {
        self.update();
        self.input.at = None;
        self.rehover();
    }

    /// Whether Shift is held: a press in a text field with it stretches
    /// the selection.
    pub fn set_shift(&mut self, held: bool) {
        self.input.shift = held;
    }

    /// Whether Shift and Control are held, as the window says.
    pub fn set_modifiers(&mut self, shift: bool, control: bool) {
        self.input.shift = shift;
        self.input.control = control;
    }

    /// Whether Shift and Control were held, as last said: what a program
    /// reads when a click comes, to add to a selection or stretch it.
    #[must_use]
    pub fn modifiers(&self) -> (bool, bool) {
        (self.input.shift, self.input.control)
    }

    /// The button went down.
    pub fn pointer_press(&mut self) {
        self.update();
        let Some((x, y)) = self.input.at else {
            return;
        };
        let now = self.clock_now();
        let clicks = match self.input.last_press {
            Some((then, px, py, count))
                if now - then <= MULTI_CLICK_MS
                    && (px - x).abs() <= MULTI_CLICK_SLOP
                    && (py - y).abs() <= MULTI_CLICK_SLOP =>
            {
                count + 1
            }
            _ => 1,
        };
        self.input.last_press = Some((now, x, y, clicks));
        if self.press_outside_popups(x, y) {
            self.rehover();
            return;
        }
        let Some((item, hit)) = self.hits_at(x, y).into_iter().last() else {
            return;
        };
        self.input.grab = Some(item);
        match hit {
            Hit::Area(area) => {
                area.pressed.set(self, true);
                area.contains_mouse.set(self, true);
                self.follow(item, x, y, false);
                self.emit(area.on_pressed, Vec::new());
                if clicks == 2 {
                    self.emit(area.double_clicked, Vec::new());
                }
            }
            Hit::Split(child, start) => {
                let along = if self.split_vertical(item) { y } else { x };
                self.input.split = Some((child, start, along));
            }
            Hit::Text => {
                self.force_active_focus(item);
                if let Some(rect) = self.rect_of(item) {
                    let shift = self.input.shift;
                    self.text_press(item, (x - rect.x, y - rect.y), clicks, shift);
                }
            }
        }
    }

    /// The button came up.
    pub fn pointer_release(&mut self) {
        self.update();
        let Some(item) = self.input.grab.take() else {
            return;
        };
        self.input.split = None;
        let Some(area) = self.mouse_area(item) else {
            self.rehover();
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
            self.released_inside_popup(item);
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

    /// The pointer's shape, when it changed since the last call: for the
    /// window to set.
    pub fn take_cursor(&mut self) -> Option<Cursor> {
        core::mem::take(&mut self.input.cursor_changed).then_some(self.input.cursor)
    }

    /// A view gave a delegate back to its pool: what is under the pointer
    /// is looked for again at the end of the next [`Ui::update`]. The
    /// hovered area may have been that delegate's, and is then given
    /// another row -- a click that changed the rows (a selection) left the
    /// row it now shows hovered, whatever the pointer was over, until the
    /// pointer next moved (shelf on the v0.2.0 candidate, 2026-10-28).
    pub(crate) fn delegate_released(&mut self) {
        self.input.recycled = true;
    }

    /// Look under the pointer again if a view recycled a delegate: called
    /// once the update has made and placed the rows.
    pub(crate) fn rehover_if_recycled(&mut self) {
        if core::mem::take(&mut self.input.recycled) {
            self.rehover();
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
    /// leaves and the one it enters; and take the shape of what is under
    /// the pointer.
    fn rehover(&mut self) {
        let hits = match self.input.at {
            Some((x, y)) => self.hits_at(x, y),
            None => Vec::new(),
        };
        let under = self
            .input
            .grab
            .or_else(|| hits.last().map(|(item, _)| *item));
        let on_handle =
            self.input.split.is_some() || matches!(hits.last(), Some((_, Hit::Split(..))));
        let shape = match under {
            Some(item) if on_handle && self.split_vertical(item) => Cursor::RowResize,
            Some(_) if on_handle => Cursor::ColResize,
            Some(item) => self.cursor_of(item),
            None => Cursor::Default,
        };
        if shape != self.input.cursor {
            self.input.cursor = shape;
            self.input.cursor_changed = true;
        }
        let mut next = None;
        for (item, hit) in hits.into_iter().rev() {
            if let Hit::Area(area) = hit
                && self.get(area.hover_enabled)
            {
                next = Some(item);
                break;
            }
        }
        let before = self.input.hover;
        if before == next {
            if let (Some(item), Some((x, y))) = (next, self.input.at)
                && self.input.grab != Some(item)
            {
                self.follow(item, x, y, false);
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
                self.follow(item, x, y, false);
            }
            self.emit(area.entered, Vec::new());
        }
    }

    /// The shape `item` asks for.
    fn cursor_of(&mut self, item: ItemId) -> Cursor {
        if self.text_input(item).is_some() {
            return Cursor::Text;
        }
        let Some(area) = self.mouse_area(item) else {
            return Cursor::Default;
        };
        match self.get(area.cursor_shape) {
            cursor_shape::POINTING_HAND => Cursor::Pointer,
            cursor_shape::IBEAM => Cursor::Text,
            cursor_shape::SPLIT_H => Cursor::ColResize,
            cursor_shape::SPLIT_V => Cursor::RowResize,
            _ => Cursor::Default,
        }
    }

    /// Set `item`'s `mouseX` and `mouseY` from the window's (`x`, `y`);
    /// when the pointer `moved` while a press it took is held, say so,
    /// and in a text field, select to there.
    fn follow(&mut self, item: ItemId, x: f32, y: f32, moved: bool) {
        let Some(rect) = self.rect_of(item) else {
            return;
        };
        if self.text_input(item).is_some() {
            if moved && self.input.grab == Some(item) {
                self.text_drag(item, (x - rect.x, y - rect.y));
            }
            return;
        }
        let Some(area) = self.mouse_area(item) else {
            return;
        };
        area.mouse_x.set(self, x - rect.x);
        area.mouse_y.set(self, y - rect.y);
        if moved && self.input.grab == Some(item) {
            self.emit(area.position_changed, Vec::new());
        }
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
        self.hits_at(x, y)
            .into_iter()
            .filter_map(|(item, hit)| match hit {
                Hit::Area(area) => Some((item, area)),
                Hit::Text | Hit::Split(..) => None,
            })
            .collect()
    }

    /// The enabled, visible areas and text fields under (`x`, `y`), in
    /// paint order: topmost last.
    fn hits_at(&mut self, x: f32, y: f32) -> Vec<(ItemId, Hit)> {
        let mut out = Vec::new();
        for root in self.roots.clone() {
            self.hits_in(root, (0.0, 0.0), None, (x, y), &mut out);
        }
        // The popups over it all, outside every cut.
        for (popup, origin) in self.popups_shown() {
            self.input.overlay = Some(popup);
            self.hits_in(popup, origin, None, (x, y), &mut out);
        }
        self.input.overlay = None;
        out
    }

    /// [`Ui::hits_at`] under `item`. A disabled or hidden item hides its
    /// whole subtree from the pointer.
    fn hits_in(
        &mut self,
        item: ItemId,
        origin: (f32, f32),
        clip: Option<Rect>,
        point: (f32, f32),
        out: &mut Vec<(ItemId, Hit)>,
    ) {
        let Some(common) = self.common(item) else {
            return;
        };
        if !self.get(common.visible) || !self.get(common.enabled) {
            return;
        }
        if self.popup(item).is_some() && self.input.overlay != Some(item) {
            return;
        }
        let rect = Rect::new(
            origin.0 + self.get(common.x),
            origin.1 + self.get(common.y),
            self.get(common.width),
            self.get(common.height),
        );
        let inside = clip.is_none_or(|clip| clip.contains(point.0, point.1))
            && rect.contains(point.0, point.1);
        // A modal popup hides what is under it from the pointer.
        if let Some(props) = self.popup(item)
            && self.get(props.modal)
        {
            out.clear();
        }
        match self.kind(item) {
            Some(Kind::MouseArea(area)) if inside => out.push((item, Hit::Area(area))),
            Some(Kind::TextInput(_)) if inside => out.push((item, Hit::Text)),
            _ => {}
        }
        // A split's handles are over its children's edges.
        let handle = if inside && matches!(self.kind(item), Some(Kind::SplitView(_))) {
            self.split_handle_at(item, rect, point)
        } else {
            None
        };
        let clip = if self.get(common.clip) {
            Some(clip.map_or(rect, |outer| outer.intersect(&rect)))
        } else {
            clip
        };
        for child in self.children(item).to_vec() {
            self.hits_in(child, (rect.x, rect.y), clip, point, out);
        }
        if let Some((child, size)) = handle {
            out.push((item, Hit::Split(child, size)));
        }
    }
}
