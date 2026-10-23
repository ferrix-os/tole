//! Signals: what an item says happened, its handlers, and the queue the
//! program reads (`docs/UI-FRAMEWORK.md` §3.4).
//!
//! A handler written in the markup is a closure connected to its signal
//! and run when the signal is emitted. The program's Rust is not called
//! back: every emission is also queued, and the program takes the queue
//! after it hands the `Ui` an event ([`Ui::take_signals`]), matching each
//! against the signals of the components it holds -- pull rather than
//! callbacks, as `compositor-toolkit`'s events are, so the program's state
//! needs no `RefCell`.

use std::rc::Rc;

use crate::ui::Ui;
use crate::value::Value;

/// A signal's id: an index and its generation.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct SignalId {
    /// The index.
    index: u32,
    /// Its generation.
    generation: u32,
}

/// A handler: given the `Ui` and the signal's arguments.
pub type Handler = Rc<dyn Fn(&mut Ui, &[Value])>;

/// One emission, as the program reads it.
#[derive(Clone, Debug, PartialEq)]
pub struct Emitted {
    /// Which signal.
    pub signal: SignalId,
    /// Its arguments.
    pub args: Vec<Value>,
}

/// A signal's place.
#[derive(Default)]
struct Slot {
    /// Bumped each time it is freed.
    generation: u32,
    /// Whether a signal holds it.
    live: bool,
    /// Its handlers, in the order connected.
    handlers: Vec<Handler>,
}

/// The signals of a `Ui`.
#[derive(Default)]
pub(crate) struct Signals {
    /// Their places.
    slots: Vec<Slot>,
    /// Places free to be taken again.
    free: Vec<u32>,
    /// Emissions the program has not taken.
    queue: Vec<Emitted>,
    /// How deep handlers emitting signals have gone.
    depth: u32,
}

impl std::fmt::Debug for Signals {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Signals")
            .field("slots", &self.slots.len())
            .field("queued", &self.queue.len())
            .finish_non_exhaustive()
    }
}

/// How deep handlers may emit signals that run handlers: deeper is a
/// loop in the markup, and stops there.
const DEPTH_MAX: u32 = 64;

impl Ui {
    /// A new signal.
    pub fn new_signal(&mut self) -> SignalId {
        let signals = &mut self.signals;
        if let Some(index) = signals.free.pop()
            && let Some(slot) = signals.slots.get_mut(index as usize)
        {
            slot.live = true;
            return SignalId {
                index,
                generation: slot.generation,
            };
        }
        let index = u32::try_from(signals.slots.len()).unwrap_or(u32::MAX);
        signals.slots.push(Slot {
            generation: 0,
            live: true,
            handlers: Vec::new(),
        });
        SignalId {
            index,
            generation: 0,
        }
    }

    /// Free `signal` and its handlers.
    pub(crate) fn free_signal(&mut self, signal: SignalId) {
        if let Some(slot) = self.slot_of(signal) {
            slot.live = false;
            slot.generation = slot.generation.wrapping_add(1);
            slot.handlers.clear();
            self.signals.free.push(signal.index);
        }
    }

    /// The live place of `signal`.
    fn slot_of(&mut self, signal: SignalId) -> Option<&mut Slot> {
        self.signals
            .slots
            .get_mut(signal.index as usize)
            .filter(|slot| slot.live && slot.generation == signal.generation)
    }

    /// Run `handler` each time `signal` is emitted.
    pub fn connect(&mut self, signal: SignalId, handler: Handler) {
        if let Some(slot) = self.slot_of(signal) {
            slot.handlers.push(handler);
        }
    }

    /// Emit `signal` with `args`: queued for the program, and its
    /// handlers run, in the order connected.
    pub fn emit(&mut self, signal: SignalId, args: Vec<Value>) {
        let Some(handlers) = self.slot_of(signal).map(|slot| slot.handlers.clone()) else {
            return;
        };
        if self.signals.depth >= DEPTH_MAX {
            return;
        }
        self.signals.queue.push(Emitted {
            signal,
            args: args.clone(),
        });
        self.signals.depth += 1;
        for handler in handlers {
            handler(self, &args);
        }
        self.signals.depth -= 1;
    }

    /// Every emission since the last call, oldest first.
    pub fn take_signals(&mut self) -> Vec<Emitted> {
        core::mem::take(&mut self.signals.queue)
    }
}
