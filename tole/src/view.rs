//! `ListView` (`docs/UI-FRAMEWORK.md` §4.3): delegates only for the rows
//! in view, recycled as it scrolls.
//!
//! Every row has a height: the model's, when it says ([`crate::Model::
//! row_height`]), else the first delegate's, corrected as each row is
//! made. The heights are kept in a Fenwick tree, so the row at a scroll
//! offset and the offset of a row are found in `log n` steps however
//! many rows there are, and a row measured differently costs `log n`
//! too. A delegate scrolled out of view goes to a pool of its kind
//! (`DelegateChooser` makes several kinds) and is given the next row
//! that needs one of that kind; a screenful of delegates serves a model
//! of a million rows.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::rc::Rc;

use crate::item::ItemId;
use crate::model::{Change, ModelSource};
use crate::repeater::{Delegate, DelegateFactory};
use crate::ui::Ui;
use crate::value::Value;

/// Which of a view's delegates a row takes: an index into its factories.
pub type Chooser = Rc<dyn Fn(&Value) -> usize>;

/// How many delegates of each kind wait in a pool before more are
/// dropped.
const POOL_MAX: usize = 64;

/// Sums of row heights, by prefix.
#[derive(Debug, Default)]
struct Fenwick {
    /// The tree, from 1.
    tree: Vec<f64>,
    /// Each row's own height.
    values: Vec<f64>,
}

impl Fenwick {
    /// A tree of `values`.
    fn new(values: Vec<f64>) -> Self {
        let mut tree = vec![0.0; values.len() + 1];
        for (i, v) in values.iter().enumerate() {
            let at = i + 1;
            if let Some(slot) = tree.get_mut(at) {
                *slot += v;
            }
            let up = at + (at & at.wrapping_neg());
            let here = tree.get(at).copied().unwrap_or(0.0);
            if let Some(slot) = tree.get_mut(up) {
                *slot += here;
            }
        }
        Fenwick { tree, values }
    }

    /// How many rows.
    fn len(&self) -> usize {
        self.values.len()
    }

    /// A row more at the end.
    fn push(&mut self, value: f64) {
        let at = self.values.len() + 1;
        let low = at & at.wrapping_neg();
        // The new node covers (at - low, at]: its own value and the sums
        // of the rows before it that it covers.
        let covered = self.prefix(at - 1) - self.prefix(at - low);
        self.values.push(value);
        self.tree.push(value + covered);
    }

    /// Row `index`'s height, now `value`.
    fn set(&mut self, index: usize, value: f64) {
        let Some(old) = self.values.get_mut(index) else {
            return;
        };
        let delta = value - *old;
        *old = value;
        let mut at = index + 1;
        while let Some(slot) = self.tree.get_mut(at) {
            *slot += delta;
            at += at & at.wrapping_neg();
        }
    }

    /// The heights of the rows before `index`.
    fn prefix(&self, index: usize) -> f64 {
        let mut at = index.min(self.values.len());
        let mut sum = 0.0;
        while at > 0 {
            sum += self.tree.get(at).copied().unwrap_or(0.0);
            at -= at & at.wrapping_neg();
        }
        sum
    }

    /// All of them.
    fn total(&self) -> f64 {
        self.prefix(self.values.len())
    }

    /// The row at `offset`: the most rows whose heights sum to no more.
    fn find(&self, offset: f64) -> usize {
        let n = self.values.len();
        let mut step = n.next_power_of_two();
        let (mut at, mut left) = (0_usize, offset);
        while step > 0 {
            let next = at + step;
            if let Some(&sum) = self.tree.get(next)
                && next <= n
                && sum <= left
            {
                at = next;
                left -= sum;
            }
            step /= 2;
        }
        at.min(n.saturating_sub(1))
    }
}

/// A view's state between updates.
#[derive(Debug, Default)]
pub(crate) struct ViewState {
    /// The model it last made from.
    source: Option<ModelSource>,
    /// Its revision then.
    revision: u64,
    /// Each row's height and the spacing after it.
    heights: Fenwick,
    /// Which rows' heights are known rather than estimated.
    known: Vec<bool>,
    /// The height a row is taken to have before it is made.
    estimate: Option<f32>,
    /// The spacing the heights were taken with.
    spacing: f32,
    /// The rows with a delegate: its kind and itself.
    shown: BTreeMap<usize, (usize, Delegate)>,
    /// Delegates waiting for a row, by kind.
    pool: Vec<(usize, Delegate)>,
    /// How many delegates were ever made: for tests.
    made: usize,
}

impl Ui {
    /// Make `view`'s delegates with `factories`, the one `chooser` picks
    /// for each row (the first when `None`), from now on.
    pub fn set_view_delegates(
        &mut self,
        view: ItemId,
        factories: Vec<DelegateFactory>,
        chooser: Option<Chooser>,
    ) {
        let state = Rc::new(RefCell::new(ViewState::default()));
        let views = Rc::clone(&state);
        let effect = Rc::new(move |ui: &mut Ui| {
            let mut s = core::mem::take(&mut *views.borrow_mut());
            update_view(ui, view, &factories, chooser.as_ref(), &mut s);
            *views.borrow_mut() = s;
        });
        self.add_effect(view, effect);
        if let Some(data) = self.item_mut(view) {
            data.view = Some(state);
        }
    }

    /// How many delegates `view` holds, shown or pooled, and how many it
    /// ever made: what virtualization keeps small.
    #[must_use]
    pub fn view_delegates(&self, view: ItemId) -> Option<(usize, usize)> {
        let state = self.item(view)?.view.as_ref()?.borrow();
        Some((state.shown.len() + state.pool.len(), state.made))
    }

    /// Scroll `view` so row `index` is at its top (as far as its rows
    /// reach), as `positionViewAtIndex(index, ListView.Beginning)`.
    pub fn position_view_at_index(&mut self, view: ItemId, index: usize) {
        let (Some(props), Some(own)) = (self.list_view(view), self.common(view)) else {
            return;
        };
        self.update();
        let at = match self.item(view).and_then(|d| d.view.as_ref()) {
            Some(state) => state.borrow().heights.prefix(index) as f32,
            None => return,
        };
        let total = self.get(props.flick.content_height);
        let most = (total - self.get(own.height)).max(0.0);
        props.flick.content_y.set(self, at.clamp(0.0, most));
        self.update();
    }

    /// Scroll `view` to its end.
    pub fn position_view_at_end(&mut self, view: ItemId) {
        self.position_view_at_index(view, usize::MAX);
    }
}

/// One update of a view.
fn update_view(
    ui: &mut Ui,
    view: ItemId,
    factories: &[DelegateFactory],
    chooser: Option<&Chooser>,
    state: &mut ViewState,
) {
    let (Some(props), Some(own), Some(content)) =
        (ui.list_view(view), ui.common(view), ui.content_item(view))
    else {
        return;
    };
    let source = ui.get(props.model);
    let spacing = ui.get(props.spacing);
    let rows_now = source.rows().row_count();
    let changes = match &state.source {
        Some(before) if *before == source && state.spacing == spacing => {
            source.rows().changes_since(state.revision)
        }
        _ => vec![Change::Reset],
    };
    for change in changes {
        match change {
            Change::Changed { start, count } => {
                let rows: Vec<usize> = state
                    .shown
                    .range(start..start.saturating_add(count))
                    .map(|(r, _)| *r)
                    .collect();
                for row in rows {
                    release(ui, state, row);
                }
            }
            Change::Inserted { start, count } if start == state.heights.len() => {
                for _ in 0..count {
                    let h = state.estimate.unwrap_or(0.0) + spacing;
                    state.heights.push(f64::from(h));
                    state.known.push(false);
                }
            }
            _ => {
                let rows: Vec<usize> = state.shown.keys().copied().collect();
                for row in rows {
                    release(ui, state, row);
                }
                state.heights = Fenwick::default();
                state.known.clear();
            }
        }
    }
    state.source = Some(source.clone());
    state.revision = source.rows().revision();
    state.spacing = spacing;
    let model = source.rows();
    // Rows the tree has not taken yet (after a reset): their heights.
    if state.heights.len() != rows_now {
        let rows: Vec<usize> = state.shown.keys().copied().collect();
        for row in rows {
            release(ui, state, row);
        }
        let mut known = Vec::with_capacity(rows_now);
        let values: Vec<f64> = (0..rows_now)
            .map(|i| {
                let given = model.row_height(i);
                known.push(given.is_some());
                f64::from(given.or(state.estimate).unwrap_or(0.0) + spacing)
            })
            .collect();
        state.heights = Fenwick::new(values);
        state.known = known;
    }
    props
        .count
        .set(ui, i32::try_from(rows_now).unwrap_or(i32::MAX));
    // The rows in view, and as far past as the cache buffer.
    let cache = ui.get(props.cache_buffer).max(0.0);
    let top = f64::from(ui.get(props.flick.content_y) - cache).max(0.0);
    let bottom = f64::from(ui.get(props.flick.content_y) + ui.get(own.height) + cache);
    // Before any row is measured every height is 0: start at the top.
    // Heights the model gave place the rows already, scrolled anywhere.
    let placed = state.heights.prefix(rows_now) > 0.0;
    let first = if rows_now == 0 || (state.estimate.is_none() && !placed) {
        0
    } else {
        state.heights.find(top)
    };
    let stale: Vec<usize> = state
        .shown
        .keys()
        .copied()
        .filter(|&row| row < first || row >= rows_now || state.heights.prefix(row) >= bottom)
        .collect();
    for row in stale {
        release(ui, state, row);
    }
    let mut row = first;
    while row < rows_now && state.heights.prefix(row) < bottom {
        if !state.shown.contains_key(&row) {
            let value = model.row_value(row);
            let kind = chooser.map_or(0, |choose| choose(&value));
            let delegate = match state.pool.iter().position(|(k, _)| *k == kind) {
                Some(at) => Some(state.pool.swap_remove(at).1),
                None => factories.get(kind).and_then(|make| {
                    state.made += 1;
                    make(ui, content)
                }),
            };
            let Some(delegate) = delegate else {
                break;
            };
            ui.set_row(&delegate, value, row);
            if let Some(c) = ui.common(delegate.root) {
                c.visible.set(ui, true);
            }
            let _ = state.shown.insert(row, (kind, delegate));
        }
        measure(ui, state, row, spacing);
        row += 1;
    }
    // Each shown row at its offset, now the heights are known.
    let shown: Vec<(usize, ItemId)> = state.shown.iter().map(|(r, (_, d))| (*r, d.root)).collect();
    for (row, root) in shown {
        measure(ui, state, row, spacing);
        let y = state.heights.prefix(row) as f32;
        if let Some(c) = ui.common(root) {
            c.y.set(ui, y);
        }
    }
    let total = (state.heights.total() as f32 - if rows_now > 0 { spacing } else { 0.0 }).max(0.0);
    props.flick.content_height.set(ui, total);
}

/// Take row `row`'s height from its delegate, if it has one and it
/// differs from what the tree holds; the first one measured is the
/// estimate for every row not yet made.
fn measure(ui: &mut Ui, state: &mut ViewState, row: usize, spacing: f32) {
    let Some((_, delegate)) = state.shown.get(&row) else {
        return;
    };
    let Some(c) = ui.common(delegate.root) else {
        return;
    };
    let height = ui.get(c.height);
    if state.estimate.is_none() {
        state.estimate = Some(height);
        let known = core::mem::take(&mut state.known);
        let values: Vec<f64> = known
            .iter()
            .enumerate()
            .map(|(i, &k)| {
                if k {
                    state.heights.values.get(i).copied().unwrap_or(0.0)
                } else {
                    f64::from(height + spacing)
                }
            })
            .collect();
        state.heights = Fenwick::new(values);
        state.known = known;
    }
    let want = f64::from(height + spacing);
    if state
        .heights
        .values
        .get(row)
        .is_some_and(|&h| (h - want).abs() > 1e-3)
    {
        state.heights.set(row, want);
    }
    if let Some(k) = state.known.get_mut(row) {
        *k = true;
    }
}

/// Put row `row`'s delegate in the pool, hidden, or drop it when the
/// pool is full.
fn release(ui: &mut Ui, state: &mut ViewState, row: usize) {
    let Some((kind, delegate)) = state.shown.remove(&row) else {
        return;
    };
    if state.pool.iter().filter(|(k, _)| *k == kind).count() >= POOL_MAX {
        ui.destroy(delegate.root);
        return;
    }
    if let Some(c) = ui.common(delegate.root) {
        c.visible.set(ui, false);
    }
    state.pool.push((kind, delegate));
}

#[cfg(test)]
mod tests {
    use super::Fenwick;

    #[test]
    fn a_fenwick_tree_sums_finds_and_grows() {
        let mut tree = Fenwick::new(vec![10.0, 20.0, 30.0]);
        assert_eq!(tree.prefix(0), 0.0);
        assert_eq!(tree.prefix(2), 30.0);
        assert_eq!(tree.total(), 60.0);
        assert_eq!(tree.find(0.0), 0);
        assert_eq!(tree.find(9.9), 0);
        assert_eq!(tree.find(10.0), 1);
        assert_eq!(tree.find(59.0), 2);
        assert_eq!(tree.find(1e9), 2);
        tree.set(1, 5.0);
        assert_eq!(tree.prefix(2), 15.0);
        for v in [1.0, 2.0, 3.0, 4.0, 5.0] {
            tree.push(v);
        }
        let direct: Vec<f64> = (0..=8).map(|i| tree.values.iter().take(i).sum()).collect();
        let summed: Vec<f64> = (0..=8).map(|i| tree.prefix(i)).collect();
        assert_eq!(direct, summed);
    }
}
