//! `Popup` (`docs/UI-FRAMEWORK.md` §4.10): an item drawn over everything
//! else in the window and given the pointer first while it is shown, as
//! Qt Quick Controls' popups are, placed from its parent but outside any
//! cut around it. Menus and combo boxes open one; this first cut keeps
//! them inside the window rather than in a surface of their own.
//!
//! What closes one is its `closePolicy`: a press outside it (which it
//! then takes, so the press that closes a menu does nothing else),
//! Escape, and -- a flag of tole's own, which a menu sets -- a click
//! released inside it. A `modal` one takes every press while open and
//! dims the window behind it.

use crate::item::{ItemId, Kind, PopupProps};
use crate::render::Rect;
use crate::ui::Ui;

/// `closePolicy`'s flags, as Qt numbers them, and one of tole's own.
pub mod close_policy {
    /// Nothing closes it but `close()`.
    pub const NO_AUTO_CLOSE: i32 = 0;
    /// A press outside it.
    pub const CLOSE_ON_PRESS_OUTSIDE: i32 = 0x1;
    /// Escape.
    pub const CLOSE_ON_ESCAPE: i32 = 0x20;
    /// A click released inside it, on something that takes clicks: what
    /// a menu does once an item is chosen. Not in Qt.
    pub const CLOSE_ON_RELEASE_INSIDE: i32 = 0x1000;
    /// Escape or a press outside, as Qt's popups have.
    pub const DEFAULT: i32 = CLOSE_ON_ESCAPE | CLOSE_ON_PRESS_OUTSIDE;
}

impl Ui {
    /// A `Popup`'s own properties.
    #[must_use]
    pub fn popup(&self, item: ItemId) -> Option<PopupProps> {
        match self.kind(item)? {
            Kind::Popup(props) => Some(props),
            _ => None,
        }
    }

    /// Show the `Popup` `item`. A modal one, or one given `focus`, takes
    /// the keys, as a dialog does; whichever it is, the keys go back to
    /// where they were when it closes, if they are inside it then.
    pub fn open_popup(&mut self, item: ItemId) {
        let Some(props) = self.popup(item) else {
            return;
        };
        let Some(common) = self.common(item) else {
            return;
        };
        let was_shown = self.get(common.visible);
        common.visible.set(self, true);
        if was_shown {
            return;
        }
        let before = self.active_focus_item();
        self.focus.before_popup.retain(|(p, _)| *p != item);
        self.focus.before_popup.push((item, before));
        if self.get(props.modal) || self.get(common.focus) {
            self.force_active_focus(item);
        }
    }

    /// Hide the `Popup` `item`.
    pub fn close_popup(&mut self, item: ItemId) {
        if let Some(common) = self.common(item).filter(|_| self.popup(item).is_some()) {
            common.visible.set(self, false);
        }
        self.give_focus_back(item);
    }

    /// Popups hidden some other way than [`Ui::close_popup`] -- their
    /// `visible` set, or destroyed -- give the keys back too: run as the
    /// `Ui` updates.
    pub(crate) fn popups_hidden(&mut self) {
        let open: Vec<ItemId> = self.focus.before_popup.iter().map(|(p, _)| *p).collect();
        let gone: Vec<ItemId> = open
            .into_iter()
            .filter(|p| !self.common(*p).is_some_and(|c| self.get(c.visible)))
            .collect();
        for popup in gone {
            self.give_focus_back(popup);
        }
    }

    /// `item` closed: the keys go back to where they were when it opened,
    /// but only when they are still inside it -- a popup closed after
    /// something else took them leaves them there.
    fn give_focus_back(&mut self, item: ItemId) {
        let Some(at) = self.focus.before_popup.iter().position(|(p, _)| *p == item) else {
            return;
        };
        let (_, before) = self.focus.before_popup.remove(at);
        let mut inside = false;
        let mut walk = self.active_focus_item();
        while let Some(here) = walk {
            if here == item {
                inside = true;
                break;
            }
            walk = self.parent(here);
        }
        // A popup destroyed with the keys in it leaves no chain to walk.
        let lost = self.active_focus_item().is_none_or(|f| !self.exists(f));
        if inside || (lost && !self.exists(item)) {
            match before {
                Some(before) if self.exists(before) => self.force_active_focus(before),
                _ if self.exists(item) => self.set_focus(item, false),
                _ => {}
            }
        }
    }

    /// The popups open now, in tree order, the topmost last.
    #[must_use]
    pub fn open_popups(&mut self) -> Vec<ItemId> {
        self.popups_shown().into_iter().map(|(p, _)| p).collect()
    }

    /// The popups shown, in tree order (topmost last), each with where
    /// its parent is in the window.
    pub(crate) fn popups_shown(&mut self) -> Vec<(ItemId, (f32, f32))> {
        let mut out = Vec::new();
        for root in self.roots.clone() {
            self.popups_in(root, (0.0, 0.0), &mut out);
        }
        out
    }

    /// [`Ui::popups_shown`] under `item`, its parent at `origin`.
    fn popups_in(&mut self, item: ItemId, origin: (f32, f32), out: &mut Vec<(ItemId, (f32, f32))>) {
        let Some(common) = self.common(item) else {
            return;
        };
        if !self.get(common.visible) {
            return;
        }
        if self.popup(item).is_some() {
            out.push((item, origin));
        }
        let here = (origin.0 + self.get(common.x), origin.1 + self.get(common.y));
        for child in self.children(item).to_vec() {
            self.popups_in(child, here, out);
        }
    }

    /// A press at (`x`, `y`): close the popups it is outside of that say
    /// so. Answers whether a popup takes the press: it closed one, or a
    /// modal one is open and it fell outside it.
    pub(crate) fn press_outside_popups(&mut self, x: f32, y: f32) -> bool {
        let shown = self.popups_shown();
        let mut taken = false;
        for (popup, _) in shown.iter().rev() {
            let Some(props) = self.popup(*popup) else {
                continue;
            };
            let inside = self.rect_of(*popup).is_some_and(|r| r.contains(x, y));
            if inside {
                break;
            }
            if self.get(props.close_policy) & close_policy::CLOSE_ON_PRESS_OUTSIDE != 0 {
                self.close_popup(*popup);
                taken = true;
            } else if self.get(props.modal) {
                taken = true;
            }
        }
        taken
    }

    /// Escape: close the topmost popup that closes on it. Whether one did.
    pub(crate) fn escape_popup(&mut self) -> bool {
        for (popup, _) in self.popups_shown().into_iter().rev() {
            if let Some(props) = self.popup(popup)
                && self.get(props.close_policy) & close_policy::CLOSE_ON_ESCAPE != 0
            {
                self.close_popup(popup);
                return true;
            }
        }
        false
    }

    /// A click released on `item`: close the popup around it if it
    /// closes on that.
    pub(crate) fn released_inside_popup(&mut self, item: ItemId) {
        let mut at = self.parent(item);
        while let Some(above) = at {
            if let Some(props) = self.popup(above) {
                if self.get(props.close_policy) & close_policy::CLOSE_ON_RELEASE_INSIDE != 0 {
                    self.close_popup(above);
                }
                return;
            }
            at = self.parent(above);
        }
    }

    /// The window's whole area: the first root's.
    pub(crate) fn window_rect(&mut self) -> Rect {
        match self
            .roots
            .first()
            .copied()
            .and_then(|root| self.common(root))
        {
            Some(c) => Rect::new(0.0, 0.0, self.get(c.width), self.get(c.height)),
            None => Rect::default(),
        }
    }
}
