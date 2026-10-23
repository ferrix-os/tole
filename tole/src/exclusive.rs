//! `Exclusive.checked`: among the items beside each other (one parent)
//! that say it, at most one is checked, as Qt Quick Controls' radio
//! buttons are by `autoExclusive`. The newest to become checked wins.

use std::cell::Cell;
use std::rc::Rc;

use crate::item::ItemId;
use crate::ui::{Prop, Ui};

impl Ui {
    /// Make `checked`, `item`'s, exclusive among its siblings': when it
    /// becomes true, theirs are set false at the next [`Ui::update`].
    pub fn exclusive(&mut self, item: ItemId, checked: Prop<bool>) {
        let kept: Vec<(ItemId, Prop<bool>)> = self
            .exclusives
            .iter()
            .copied()
            .filter(|(other, _)| self.exists(*other))
            .collect();
        self.exclusives = kept;
        self.exclusives.push((item, checked));
        let was = Rc::new(Cell::new(false));
        self.add_effect(
            item,
            Rc::new(move |ui: &mut Ui| {
                let now = ui.get(checked);
                if now && !was.get() {
                    let parent = ui.parent(item);
                    for (other, prop) in ui.exclusives.clone() {
                        if other != item && ui.exists(other) && ui.parent(other) == parent {
                            prop.set(ui, false);
                        }
                    }
                }
                was.set(now);
            }),
        );
    }
}
