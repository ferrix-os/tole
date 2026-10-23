//! Focus and keys (`docs/UI-FRAMEWORK.md` §4.7), as Qt Quick has them.
//!
//! Within a focus scope -- a `FocusScope`, or a root -- one item at a time
//! holds `focus`; giving it to another takes it from the one that held
//! it. Keys go to the *active focus* item, reached from the root down
//! through the scopes, each scope's focused item in turn. An item and the
//! scopes above it have `activeFocus`.
//!
//! A key goes first to the active focus item's `Keys`: its handler for
//! that key (`Keys.onReturnPressed`), which accepts it, then
//! `Keys.onPressed`, which accepts it only by `event.accepted = true`.
//! Then its `KeyNavigation`, then the same on each item above it. A key
//! nobody accepted that is Tab or Shift+Tab moves the focus to the next
//! or previous item with `activeFocusOnTab`, in tree order.

use std::rc::Rc;

use crate::item::{ItemId, Kind};
use crate::signal::SignalId;
use crate::ui::Ui;
use crate::value::Value;

/// A key as `Keys` handlers read it: `event.key`, `event.text`, ...
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct KeyEvent {
    /// The keysym's name, as xkb names it: `"a"`, `"A"`, `"Return"`,
    /// `"Escape"`, `"Tab"`, `"ISO_Left_Tab"`, `"Up"`, `"space"`.
    pub key: Rc<str>,
    /// What it types, if it types anything printable.
    pub text: Rc<str>,
    /// Shift was held.
    pub shift: bool,
    /// Control was held.
    pub control: bool,
    /// Alt was held.
    pub alt: bool,
    /// The key is held and repeating.
    pub repeat: bool,
}

impl KeyEvent {
    /// The key `key` with no modifiers, typing `text` when it is one
    /// printable character.
    #[must_use]
    pub fn named(key: &str) -> Self {
        let printable = key.chars().count() == 1 && !key.chars().any(char::is_control);
        KeyEvent {
            key: Rc::from(key),
            text: Rc::from(if printable { key } else { "" }),
            ..KeyEvent::default()
        }
    }

    /// The same with Shift held.
    #[must_use]
    pub fn shifted(self) -> Self {
        KeyEvent {
            shift: true,
            ..self
        }
    }

    /// Whether this is Shift+Tab, which xkb names `ISO_Left_Tab`.
    fn is_backtab(&self) -> bool {
        &*self.key == "ISO_Left_Tab" || (&*self.key == "Tab" && self.shift)
    }
}

impl crate::PropType for KeyEvent {
    fn into_value(self) -> Value {
        Value::custom(self)
    }
    fn from_value(value: &Value) -> Option<Self> {
        value.downcast::<Self>().cloned()
    }
    fn fallback() -> Self {
        KeyEvent::default()
    }
}

/// Where `KeyNavigation` sends the focus.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Nav {
    /// `KeyNavigation.tab`.
    Tab,
    /// `KeyNavigation.backtab`.
    Backtab,
    /// `KeyNavigation.up`.
    Up,
    /// `KeyNavigation.down`.
    Down,
    /// `KeyNavigation.left`.
    Left,
    /// `KeyNavigation.right`.
    Right,
}

impl Nav {
    /// The direction `event` asks for.
    fn of(event: &KeyEvent) -> Option<Nav> {
        if event.is_backtab() {
            return Some(Nav::Backtab);
        }
        Some(match &*event.key {
            "Tab" => Nav::Tab,
            "Up" => Nav::Up,
            "Down" => Nav::Down,
            "Left" => Nav::Left,
            "Right" => Nav::Right,
            _ => return None,
        })
    }
}

/// An item's `Keys` and `KeyNavigation`.
#[derive(Debug, Default)]
pub(crate) struct KeysAttached {
    /// `Keys.onPressed`.
    pressed: Option<SignalId>,
    /// `Keys.onReleased`.
    released: Option<SignalId>,
    /// `Keys.onReturnPressed` and the rest: the keysym and its signal.
    by_key: Vec<(Rc<str>, SignalId)>,
    /// `KeyNavigation.*`.
    nav: Vec<(Nav, ItemId)>,
}

impl KeysAttached {
    /// Every signal it made, to be freed with its item.
    pub(crate) fn signals(&self) -> Vec<SignalId> {
        let mut out: Vec<SignalId> = self.pressed.into_iter().chain(self.released).collect();
        out.extend(self.by_key.iter().map(|(_, signal)| *signal));
        out
    }
}

/// The focus's state.
#[derive(Debug)]
pub(crate) struct Focus {
    /// The items with `activeFocus`, outermost first; the last is where
    /// keys go.
    chain: Vec<ItemId>,
    /// The root the focus is under; the first root when `None`.
    root: Option<ItemId>,
    /// Whether the window has the keyboard.
    window_active: bool,
    /// Whether a handler accepted the key being delivered.
    accepted: bool,
    /// The modal popups open that took the focus, each with the item
    /// that had it before, to have it back when the popup closes.
    pub(crate) before_popup: Vec<(ItemId, Option<ItemId>)>,
}

impl Default for Focus {
    fn default() -> Self {
        Focus {
            chain: Vec::new(),
            root: None,
            window_active: true,
            accepted: false,
            before_popup: Vec::new(),
        }
    }
}

impl Ui {
    /// Give `item` the focus within its focus scope (`on`), taking it
    /// from the item that held it, or take it away. It has the active
    /// focus too when its scopes do; [`Ui::force_active_focus`] gives
    /// them it as well.
    pub fn set_focus(&mut self, item: ItemId, on: bool) {
        self.focus_within_scope(item, on);
        self.refocus();
    }

    /// Give `item` the active focus: the focus within its scope, and each
    /// scope around it the focus within its own.
    pub fn force_active_focus(&mut self, item: ItemId) {
        if !self.exists(item) {
            return;
        }
        let mut at = item;
        loop {
            match self.scope_of(at) {
                Some(scope) => {
                    self.focus_within_scope(at, true);
                    if self.parent(scope).is_none() {
                        self.focus.root = Some(scope);
                        break;
                    }
                    at = scope;
                }
                None => {
                    self.focus.root = Some(at);
                    break;
                }
            }
        }
        self.refocus();
    }

    /// The item keys go to, if any has the active focus.
    #[must_use]
    pub fn active_focus_item(&self) -> Option<ItemId> {
        self.focus.chain.last().copied()
    }

    /// Whether the window has the keyboard: without it nothing has the
    /// active focus, though every scope remembers its focused item.
    pub fn set_window_active(&mut self, active: bool) {
        self.focus.window_active = active;
        self.refocus();
    }

    /// The `Keys.onPressed` signal of `item`, made the first time it is
    /// asked for.
    pub fn keys_pressed(&mut self, item: ItemId) -> Option<SignalId> {
        if let Some(signal) = self.item(item)?.keys.as_ref().and_then(|k| k.pressed) {
            return Some(signal);
        }
        let signal = self.new_signal();
        self.attached(item)?.pressed = Some(signal);
        Some(signal)
    }

    /// The `Keys.onReleased` signal of `item`.
    pub fn keys_released(&mut self, item: ItemId) -> Option<SignalId> {
        if let Some(signal) = self.item(item)?.keys.as_ref().and_then(|k| k.released) {
            return Some(signal);
        }
        let signal = self.new_signal();
        self.attached(item)?.released = Some(signal);
        Some(signal)
    }

    /// The signal of `item`'s handler for one key, by its keysym
    /// (`Keys.onReturnPressed` is `"Return"`): a key with a handler is
    /// accepted.
    pub fn keys_key_pressed(&mut self, item: ItemId, keysym: &str) -> Option<SignalId> {
        if let Some(signal) = self.item(item)?.keys.as_ref().and_then(|k| {
            k.by_key
                .iter()
                .find(|(name, _)| &**name == keysym)
                .map(|(_, signal)| *signal)
        }) {
            return Some(signal);
        }
        let signal = self.new_signal();
        self.attached(item)?.by_key.push((Rc::from(keysym), signal));
        Some(signal)
    }

    /// `KeyNavigation`: the key `nav` names moves the active focus from
    /// `item` to `target`.
    pub fn set_key_navigation(&mut self, item: ItemId, nav: Nav, target: ItemId) {
        if let Some(keys) = self.attached(item) {
            keys.nav.retain(|(n, _)| *n != nav);
            keys.nav.push((nav, target));
        }
    }

    /// Accept the key being delivered: a `Keys.onPressed` handler's
    /// `event.accepted = true`.
    pub fn accept_key(&mut self, accepted: bool) {
        self.focus.accepted = accepted;
    }

    /// A key went down: delivered as the module says. Answers whether
    /// anything took it; a program may use one nobody took itself.
    pub fn key_press(&mut self, event: &KeyEvent) -> bool {
        self.update();
        if &*event.key == "Escape" && self.escape_popup() {
            return true;
        }
        let start = self
            .active_focus_item()
            .or_else(|| self.focus_root())
            .filter(|_| self.focus.window_active);
        let mut at = start;
        while let Some(item) = at {
            if self.deliver(item, event, true) {
                return true;
            }
            at = self.parent(item);
        }
        let step = match Nav::of(event) {
            Some(Nav::Tab) => 1,
            Some(Nav::Backtab) => -1,
            _ => return false,
        };
        self.tab(step)
    }

    /// A key came up: `Keys.onReleased`, bubbling.
    pub fn key_release(&mut self, event: &KeyEvent) -> bool {
        self.update();
        let mut at = self
            .active_focus_item()
            .or_else(|| self.focus_root())
            .filter(|_| self.focus.window_active);
        while let Some(item) = at {
            if self.deliver(item, event, false) {
                return true;
            }
            at = self.parent(item);
        }
        false
    }

    /// Move the active focus `step` stops along the Tab order (1 forward,
    /// -1 back), wrapping. Answers whether there was anywhere to go.
    pub fn tab(&mut self, step: i32) -> bool {
        let Some(root) = self.focus_root() else {
            return false;
        };
        let mut stops = Vec::new();
        self.tab_stops(root, &mut stops);
        if stops.is_empty() {
            return false;
        }
        let count = stops.len() as i64;
        let next = match self
            .active_focus_item()
            .and_then(|current| stops.iter().position(|&s| s == current))
        {
            Some(at) => (at as i64 + i64::from(step)).rem_euclid(count),
            None if step >= 0 => 0,
            None => count - 1,
        };
        let Some(&target) = stops.get(next as usize) else {
            return false;
        };
        self.force_active_focus(target);
        true
    }

    /// `item`'s `Keys` and `KeyNavigation`, made when first asked for.
    fn attached(&mut self, item: ItemId) -> Option<&mut KeysAttached> {
        let data = self.item_mut(item)?;
        Some(data.keys.get_or_insert_with(Box::default))
    }

    /// The key to `item`'s handlers, then its navigation: whether it was
    /// taken.
    fn deliver(&mut self, item: ItemId, event: &KeyEvent, down: bool) -> bool {
        let Some(keys) = self.item(item).and_then(|data| data.keys.as_ref()) else {
            // No handlers: a text field takes what it edits with.
            return down && self.text_key(item, event);
        };
        let args = vec![Value::custom(event.clone())];
        if !down {
            let Some(released) = keys.released else {
                return false;
            };
            self.focus.accepted = false;
            self.emit(released, args);
            return core::mem::take(&mut self.focus.accepted);
        }
        let specific = keys
            .by_key
            .iter()
            .find(|(name, _)| **name == *event.key)
            .map(|(_, signal)| *signal);
        let pressed = keys.pressed;
        let nav = Nav::of(event).and_then(|want| {
            keys.nav
                .iter()
                .find(|(n, _)| *n == want)
                .map(|(_, target)| *target)
        });
        if let Some(signal) = specific {
            self.focus.accepted = true;
            self.emit(signal, args.clone());
            if core::mem::take(&mut self.focus.accepted) {
                return true;
            }
        }
        if let Some(signal) = pressed {
            self.focus.accepted = false;
            self.emit(signal, args);
            if core::mem::take(&mut self.focus.accepted) {
                return true;
            }
        }
        if self.text_key(item, event) {
            return true;
        }
        if let Some(target) = nav
            && self.exists(target)
        {
            self.force_active_focus(target);
            return true;
        }
        false
    }

    /// The root the focus is under.
    fn focus_root(&self) -> Option<ItemId> {
        self.focus
            .root
            .filter(|&root| self.exists(root))
            .or_else(|| self.roots.first().copied())
    }

    /// The scope `item` is in: its nearest `FocusScope` above, or its
    /// root. `None` for a root.
    fn scope_of(&self, item: ItemId) -> Option<ItemId> {
        let mut at = self.parent(item);
        while let Some(above) = at {
            if self.parent(above).is_none() || matches!(self.kind(above), Some(Kind::FocusScope)) {
                return Some(above);
            }
            at = self.parent(above);
        }
        None
    }

    /// `focus` within the scope, without working out the active focus.
    fn focus_within_scope(&mut self, item: ItemId, on: bool) {
        let Some(common) = self.common(item) else {
            return;
        };
        let Some(scope) = self.scope_of(item) else {
            common.focus.set(self, on);
            return;
        };
        let held = self.item(scope).and_then(|data| data.focus_child);
        if on {
            if let Some(before) = held
                && before != item
                && let Some(c) = self.common(before)
            {
                c.focus.set(self, false);
            }
            if let Some(data) = self.item_mut(scope) {
                data.focus_child = Some(item);
            }
        } else if held == Some(item)
            && let Some(data) = self.item_mut(scope)
        {
            data.focus_child = None;
        }
        common.focus.set(self, on);
    }

    /// Work out which items have the active focus, and set their
    /// `activeFocus`.
    fn refocus(&mut self) {
        let mut chain = Vec::new();
        if self.focus.window_active
            && let Some(root) = self.focus_root()
        {
            let mut scope = root;
            while let Some(held) = self
                .item(scope)
                .and_then(|data| data.focus_child)
                .filter(|&held| self.exists(held))
            {
                chain.push(held);
                if !matches!(self.kind(held), Some(Kind::FocusScope)) || chain.len() > 4096 {
                    break;
                }
                scope = held;
            }
        }
        let before = core::mem::take(&mut self.focus.chain);
        for item in before.iter().filter(|item| !chain.contains(item)) {
            if let Some(c) = self.common(*item) {
                c.active_focus.set(self, false);
            }
        }
        for item in &chain {
            if let Some(c) = self.common(*item) {
                c.active_focus.set(self, true);
            }
        }
        self.focus.chain = chain;
    }

    /// After items went: work the active focus out again if one of them
    /// had it.
    pub(crate) fn forget_focus(&mut self) {
        if self.focus.chain.iter().any(|&item| !self.exists(item)) {
            self.refocus();
        }
    }

    /// The Tab stops under `item`, in tree order: shown, enabled, with
    /// `activeFocusOnTab`.
    fn tab_stops(&mut self, item: ItemId, out: &mut Vec<ItemId>) {
        let Some(common) = self.common(item) else {
            return;
        };
        if !self.get(common.visible) || !self.get(common.enabled) {
            return;
        }
        if self.get(common.active_focus_on_tab) {
            out.push(item);
        }
        for child in self.children(item).to_vec() {
            self.tab_stops(child, out);
        }
    }
}
