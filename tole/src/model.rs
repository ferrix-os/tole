//! Models (`docs/UI-FRAMEWORK.md` §3.4): what `Repeater` and the views
//! make their delegates from.
//!
//! A program's model is Rust: a type implementing [`Model`], shared as a
//! [`ModelRc`], which says what changed through its [`ModelTracker`]. The
//! views poll the tracker's revision when the `Ui` updates, so a model
//! needs no `Ui` to change. [`VecModel`] covers the small cases. The
//! markup's `list<T>` values and a plain `int` count are models too: all
//! three reach a view as one [`ModelSource`].

use std::cell::{Cell, RefCell};
use std::fmt;
use std::rc::Rc;

use crate::value::{PropType, Value};

/// What changed in a model, as a view reads it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Change {
    /// Rows `start..start + count` hold new values.
    Changed {
        /// The first.
        start: usize,
        /// How many.
        count: usize,
    },
    /// Rows were added at `start`.
    Inserted {
        /// Where.
        start: usize,
        /// How many.
        count: usize,
    },
    /// Rows `start..start + count` went.
    Removed {
        /// The first.
        start: usize,
        /// How many.
        count: usize,
    },
    /// Anything may have changed.
    Reset,
}

/// How many changes a tracker keeps for views that have not read them;
/// a view further behind starts again from the rows.
const CHANGES_KEPT: usize = 64;

/// What a model says changed, for the views showing it.
#[derive(Default)]
pub struct ModelTracker {
    /// Bumped at each change.
    revision: Cell<u64>,
    /// The last changes, each with the revision it made.
    changes: RefCell<Vec<(u64, Change)>>,
}

impl fmt::Debug for ModelTracker {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ModelTracker")
            .field("revision", &self.revision.get())
            .finish_non_exhaustive()
    }
}

impl ModelTracker {
    /// Rows `start..start + count` hold new values.
    pub fn rows_changed(&self, start: usize, count: usize) {
        self.note(Change::Changed { start, count });
    }

    /// `count` rows were added at `start`.
    pub fn rows_inserted(&self, start: usize, count: usize) {
        self.note(Change::Inserted { start, count });
    }

    /// Rows `start..start + count` went.
    pub fn rows_removed(&self, start: usize, count: usize) {
        self.note(Change::Removed { start, count });
    }

    /// Anything may have changed.
    pub fn reset(&self) {
        self.note(Change::Reset);
    }

    /// The revision: bumped at each change.
    #[must_use]
    pub fn revision(&self) -> u64 {
        self.revision.get()
    }

    /// The changes after `revision`, oldest first; `[Reset]` when they
    /// are no longer kept.
    #[must_use]
    pub fn changes_since(&self, revision: u64) -> Vec<Change> {
        if revision == self.revision.get() {
            return Vec::new();
        }
        let changes = self.changes.borrow();
        match changes.first() {
            Some((first, _)) if *first <= revision + 1 => changes
                .iter()
                .filter(|(r, _)| *r > revision)
                .map(|(_, c)| *c)
                .collect(),
            _ => vec![Change::Reset],
        }
    }

    /// Record a change.
    fn note(&self, change: Change) {
        let revision = self.revision.get().wrapping_add(1);
        self.revision.set(revision);
        let mut changes = self.changes.borrow_mut();
        if changes.len() >= CHANGES_KEPT {
            let _ = changes.remove(0);
        }
        changes.push((revision, change));
    }
}

/// A model of rows of `Row`, written in Rust.
pub trait Model {
    /// A row: a type the markup declared (`struct LogLine`), or any other
    /// property type.
    type Row: PropType;
    /// How many rows.
    fn row_count(&self) -> usize;
    /// Row `index`.
    fn row(&self, index: usize) -> Option<Self::Row>;
    /// Row `index`'s height, when the view may know it without making the
    /// row: rows of several heights in a long list.
    fn row_height(&self, _index: usize) -> Option<f32> {
        None
    }
    /// What says it changed.
    fn tracker(&self) -> &ModelTracker;
}

/// A model of any row type, as a view holds it.
pub trait AnyModel {
    /// How many rows.
    fn row_count(&self) -> usize;
    /// Row `index`, as a property value.
    fn row_value(&self, index: usize) -> Value;
    /// Row `index`'s height, if the model knows it.
    fn row_height(&self, index: usize) -> Option<f32>;
    /// The tracker's revision; 0 for a model that never changes.
    fn revision(&self) -> u64;
    /// The changes after `revision`.
    fn changes_since(&self, revision: u64) -> Vec<Change>;
}

/// A shared [`Model`]: what a `model<T>` property holds.
pub struct ModelRc<T> {
    /// The model.
    inner: Rc<dyn Model<Row = T>>,
}

impl<T> Clone for ModelRc<T> {
    fn clone(&self) -> Self {
        ModelRc {
            inner: Rc::clone(&self.inner),
        }
    }
}

impl<T: PropType> fmt::Debug for ModelRc<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ModelRc")
            .field("rows", &self.inner.row_count())
            .finish()
    }
}

impl<T> PartialEq for ModelRc<T> {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.inner, &other.inner)
    }
}

impl<T: PropType> ModelRc<T> {
    /// `model`, shared.
    pub fn new(model: impl Model<Row = T> + 'static) -> Self {
        ModelRc {
            inner: Rc::new(model),
        }
    }

    /// An `Rc` the program already holds, so it keeps a handle to change
    /// the model through.
    pub fn from_rc(model: Rc<dyn Model<Row = T>>) -> Self {
        ModelRc { inner: model }
    }

    /// The model.
    #[must_use]
    pub fn model(&self) -> &dyn Model<Row = T> {
        &*self.inner
    }
}

impl<T: PropType> Default for ModelRc<T> {
    fn default() -> Self {
        ModelRc::new(VecModel::<T>::default())
    }
}

impl<T: PropType> PropType for ModelRc<T> {
    fn into_value(self) -> Value {
        Value::custom(self)
    }
    fn from_value(value: &Value) -> Option<Self> {
        value.downcast::<Self>().cloned()
    }
    fn fallback() -> Self {
        ModelRc::default()
    }
}

impl<T: PropType> AnyModel for ModelRc<T> {
    fn row_count(&self) -> usize {
        self.inner.row_count()
    }
    fn row_value(&self, index: usize) -> Value {
        self.inner
            .row(index)
            .map_or(Value::Void, PropType::into_value)
    }
    fn row_height(&self, index: usize) -> Option<f32> {
        self.inner.row_height(index)
    }
    fn revision(&self) -> u64 {
        self.inner.tracker().revision()
    }
    fn changes_since(&self, revision: u64) -> Vec<Change> {
        self.inner.tracker().changes_since(revision)
    }
}

/// A model over a `Vec`, changed through its methods.
#[derive(Debug)]
pub struct VecModel<T> {
    /// The rows.
    rows: RefCell<Vec<T>>,
    /// What changed.
    tracker: ModelTracker,
}

impl<T> Default for VecModel<T> {
    fn default() -> Self {
        VecModel {
            rows: RefCell::new(Vec::new()),
            tracker: ModelTracker::default(),
        }
    }
}

impl<T: Clone> VecModel<T> {
    /// A model of `rows`.
    #[must_use]
    pub fn new(rows: Vec<T>) -> Self {
        VecModel {
            rows: RefCell::new(rows),
            tracker: ModelTracker::default(),
        }
    }

    /// Add a row at the end.
    pub fn push(&self, row: T) {
        let at = {
            let mut rows = self.rows.borrow_mut();
            rows.push(row);
            rows.len() - 1
        };
        self.tracker.rows_inserted(at, 1);
    }

    /// Put a row in at `index`.
    pub fn insert(&self, index: usize, row: T) {
        let at = {
            let mut rows = self.rows.borrow_mut();
            let at = index.min(rows.len());
            rows.insert(at, row);
            at
        };
        self.tracker.rows_inserted(at, 1);
    }

    /// Take row `index` out.
    pub fn remove(&self, index: usize) -> Option<T> {
        let row = {
            let mut rows = self.rows.borrow_mut();
            (index < rows.len()).then(|| rows.remove(index))
        }?;
        self.tracker.rows_removed(index, 1);
        Some(row)
    }

    /// Set row `index`.
    pub fn set_row(&self, index: usize, row: T) {
        let set = match self.rows.borrow_mut().get_mut(index) {
            Some(slot) => {
                *slot = row;
                true
            }
            None => false,
        };
        if set {
            self.tracker.rows_changed(index, 1);
        }
    }

    /// Every row anew.
    pub fn set_vec(&self, rows: Vec<T>) {
        *self.rows.borrow_mut() = rows;
        self.tracker.reset();
    }
}

impl<T: PropType> Model for VecModel<T> {
    type Row = T;
    fn row_count(&self) -> usize {
        self.rows.borrow().len()
    }
    fn row(&self, index: usize) -> Option<T> {
        self.rows.borrow().get(index).cloned()
    }
    fn tracker(&self) -> &ModelTracker {
        &self.tracker
    }
}

/// The rows of a `list<T>` value: a model that never changes, compared
/// by its rows.
#[derive(Debug, PartialEq)]
struct ListRows<T>(Vec<T>);

impl<T: PropType> AnyModel for ListRows<T> {
    fn row_count(&self) -> usize {
        self.0.len()
    }
    fn row_value(&self, index: usize) -> Value {
        self.0
            .get(index)
            .cloned()
            .map_or(Value::Void, PropType::into_value)
    }
    fn row_height(&self, _index: usize) -> Option<f32> {
        None
    }
    fn revision(&self) -> u64 {
        0
    }
    fn changes_since(&self, _revision: u64) -> Vec<Change> {
        Vec::new()
    }
}

/// A count as a model: `model: 5`, each row its index.
#[derive(Debug, PartialEq)]
struct Count(usize);

impl AnyModel for Count {
    fn row_count(&self) -> usize {
        self.0
    }
    fn row_value(&self, index: usize) -> Value {
        Value::Int(i32::try_from(index).unwrap_or(i32::MAX))
    }
    fn row_height(&self, _index: usize) -> Option<f32> {
        None
    }
    fn revision(&self) -> u64 {
        0
    }
    fn changes_since(&self, _revision: u64) -> Vec<Change> {
        Vec::new()
    }
}

/// What a `Repeater` or a view is given as its `model`: a count, a
/// `list<T>` or a [`ModelRc`], with its row type kept in the compiler's
/// hands rather than here.
#[derive(Clone)]
pub struct ModelSource {
    /// The rows.
    inner: Rc<dyn AnyModel>,
    /// What it was made from, to compare two sources: the same `Rc` for a
    /// [`ModelRc`], the same rows for the others.
    key: SourceKey,
}

/// How two sources compare.
#[derive(Clone, Debug, PartialEq)]
enum SourceKey {
    /// A count.
    Count(usize),
    /// A list's rows, as values.
    List(Vec<Value>),
    /// A program's model, by its address.
    Model(*const ()),
}

impl fmt::Debug for ModelSource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ModelSource")
            .field("rows", &self.inner.row_count())
            .finish_non_exhaustive()
    }
}

impl PartialEq for ModelSource {
    fn eq(&self, other: &Self) -> bool {
        self.key == other.key
    }
}

impl Default for ModelSource {
    fn default() -> Self {
        ModelSource::count(0)
    }
}

impl ModelSource {
    /// `model: 5`.
    #[must_use]
    pub fn count(count: i32) -> Self {
        let count = usize::try_from(count).unwrap_or(0);
        ModelSource {
            inner: Rc::new(Count(count)),
            key: SourceKey::Count(count),
        }
    }

    /// A `list<T>`.
    #[must_use]
    pub fn list<T: PropType + PartialEq + fmt::Debug>(rows: Vec<T>) -> Self {
        let key = SourceKey::List(rows.iter().cloned().map(PropType::into_value).collect());
        ModelSource {
            inner: Rc::new(ListRows(rows)),
            key,
        }
    }

    /// A program's model.
    #[must_use]
    pub fn model<T: PropType>(model: ModelRc<T>) -> Self {
        let key = SourceKey::Model(Rc::as_ptr(&model.inner).cast::<()>());
        ModelSource {
            inner: Rc::new(model),
            key,
        }
    }

    /// The rows.
    #[must_use]
    pub fn rows(&self) -> &dyn AnyModel {
        &*self.inner
    }
}

impl PropType for ModelSource {
    fn into_value(self) -> Value {
        Value::custom(self)
    }
    fn from_value(value: &Value) -> Option<Self> {
        value.downcast::<Self>().cloned()
    }
    fn fallback() -> Self {
        ModelSource::default()
    }
}

impl<T: PropType + PartialEq + fmt::Debug> PropType for Vec<T> {
    fn into_value(self) -> Value {
        Value::custom(self)
    }
    fn from_value(value: &Value) -> Option<Self> {
        value.downcast::<Self>().cloned()
    }
    fn fallback() -> Self {
        Vec::new()
    }
}
