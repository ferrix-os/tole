//! `Repeater` (`docs/UI-FRAMEWORK.md` §2.1): a delegate for each row of its
//! model, made beside it in its parent, so a `Row`, a `Flow` or a layout
//! places them as its own children.
//!
//! The delegates are made, given their rows and dropped when the `Ui`
//! updates ([`Ui::update`]): a row that stays keeps its delegate and is
//! given its new value; rows over are made, rows short are dropped from
//! the end.

use std::cell::RefCell;
use std::rc::Rc;

use crate::item::ItemId;
use crate::model::ModelSource;
use crate::ui::{Prop, PropId, Ui};

/// One delegate a factory made: its root, and the cells its row and
/// index are put in (`modelData`, `index`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Delegate {
    /// Its root item.
    pub root: ItemId,
    /// `modelData`: holds the row's value.
    pub row: PropId,
    /// `index`.
    pub index: Prop<i32>,
}

/// What makes a delegate under a parent: the compiler writes one for each
/// `Repeater` and view.
pub type DelegateFactory = Rc<dyn Fn(&mut Ui, ItemId) -> Option<Delegate>>;

/// A `Repeater`'s state between updates.
#[derive(Debug, Default)]
struct Made {
    /// The model it last made from.
    source: Option<ModelSource>,
    /// Its revision then.
    revision: u64,
    /// The delegates, by row.
    delegates: Vec<Delegate>,
}

impl Ui {
    /// Make `repeater`'s delegates with `factory`, from now on.
    pub fn set_delegate(&mut self, repeater: ItemId, factory: DelegateFactory) {
        let state = Rc::new(RefCell::new(Made::default()));
        let effect = Rc::new(move |ui: &mut Ui| repeat(ui, repeater, &factory, &state));
        self.add_effect(repeater, effect);
    }

    /// The cells for a delegate's `modelData` and `index` when its root
    /// declares neither: owned by `root`.
    pub fn delegate_cells(&mut self, root: ItemId) -> (PropId, Prop<i32>) {
        let row = self.own_prop(root, 0_i32).id;
        (row, self.own_prop(root, 0_i32))
    }

    /// Put a row's value in a delegate's `modelData`.
    pub(crate) fn set_row(&mut self, delegate: &Delegate, value: crate::Value, index: usize) {
        self.write(delegate.row, value);
        delegate
            .index
            .set(self, i32::try_from(index).unwrap_or(i32::MAX));
    }
}

/// One update of a `Repeater`.
fn repeat(ui: &mut Ui, repeater: ItemId, factory: &DelegateFactory, state: &RefCell<Made>) {
    let (Some(props), Some(parent)) = (ui.repeater(repeater), ui.parent(repeater)) else {
        return;
    };
    let source = ui.get(props.model);
    let revision = source.rows().revision();
    let mut made = core::mem::take(&mut *state.borrow_mut());
    if made.source.as_ref() == Some(&source) && made.revision == revision {
        *state.borrow_mut() = made;
        return;
    }
    let rows = source.rows();
    let count = rows.row_count();
    let keep = made.delegates.len().min(count);
    for gone in made.delegates.split_off(keep) {
        ui.destroy(gone.root);
    }
    for (index, delegate) in made.delegates.iter().enumerate() {
        ui.set_row(delegate, rows.row_value(index), index);
    }
    for index in keep..count {
        let Some(delegate) = factory(ui, parent) else {
            break;
        };
        ui.own_item(repeater, delegate.root);
        ui.set_row(&delegate, rows.row_value(index), index);
        made.delegates.push(delegate);
    }
    // The delegates go straight after the repeater, in their rows' order.
    let roots: Vec<ItemId> = made.delegates.iter().map(|d| d.root).collect();
    ui.place_after(repeater, &roots);
    props
        .count
        .set(ui, i32::try_from(made.delegates.len()).unwrap_or(i32::MAX));
    made.source = Some(source);
    made.revision = revision;
    *state.borrow_mut() = made;
}
