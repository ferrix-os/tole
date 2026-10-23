//! States, transitions and animations (`docs/UI-FRAMEWORK.md` §2.1),
//! driven by a clock the window ticks from its frames and tests tick by
//! hand ([`Ui::tick`]).
//!
//! * An animation is a tree of [`Step`]s -- tweens of a property from one
//!   value to another along an [`Easing`] curve, pauses, sequences and
//!   parallels -- run once, a number of times, or for ever.
//! * A `Behavior` puts a cell in front of a property ([`Ui::divert`]):
//!   what the property is given goes there, and each time that changes
//!   the property is animated from where it is to it.
//! * `states` choose a property's value by the item's active state:
//!   the first state whose `when` holds, else the one `state` names. A
//!   `Transition` from one state to another animates what the change of
//!   state moved, as a `Behavior` that only acts when the state changed.
//!
//! Real (`real`, `length`) and colour properties animate; a colour is
//! interpolated per channel, not premultiplied.

use std::cell::RefCell;
use std::f32::consts::PI;
use std::rc::Rc;

use crate::item::ItemId;
use crate::ui::{Prop, PropId, Ui};
use crate::value::{Color, Value};

/// Qt's easing curves, those the controls and the viewer use.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Easing {
    /// Straight.
    #[default]
    Linear,
    /// Quadratic, slow at the start.
    InQuad,
    /// Quadratic, slow at the end.
    OutQuad,
    /// Quadratic, slow at both ends.
    InOutQuad,
    /// Cubic, slow at the start.
    InCubic,
    /// Cubic, slow at the end.
    OutCubic,
    /// Cubic, slow at both ends.
    InOutCubic,
    /// Sine, slow at both ends.
    InOutSine,
    /// Past the end and back.
    OutBack,
    /// Bouncing at the end.
    OutBounce,
    /// A spring settling at the end.
    OutElastic,
}

impl Easing {
    /// Progress `t` (0 to 1) along the curve.
    #[must_use]
    pub fn apply(self, t: f32) -> f32 {
        let t = t.clamp(0.0, 1.0);
        match self {
            Easing::Linear => t,
            Easing::InQuad => t * t,
            Easing::OutQuad => t * (2.0 - t),
            Easing::InOutQuad => {
                if t < 0.5 {
                    2.0 * t * t
                } else {
                    -1.0 + (4.0 - 2.0 * t) * t
                }
            }
            Easing::InCubic => t * t * t,
            Easing::OutCubic => {
                let u = t - 1.0;
                u * u * u + 1.0
            }
            Easing::InOutCubic => {
                if t < 0.5 {
                    4.0 * t * t * t
                } else {
                    let u = 2.0 * t - 2.0;
                    0.5 * u * u * u + 1.0
                }
            }
            Easing::InOutSine => -(f32::cos(PI * t) - 1.0) / 2.0,
            Easing::OutBack => {
                let (c1, c3) = (1.701_58_f32, 2.701_58_f32);
                let u = t - 1.0;
                1.0 + c3 * u * u * u + c1 * u * u
            }
            Easing::OutBounce => {
                let (n, d) = (7.5625_f32, 2.75_f32);
                if t < 1.0 / d {
                    n * t * t
                } else if t < 2.0 / d {
                    let u = t - 1.5 / d;
                    n * u * u + 0.75
                } else if t < 2.5 / d {
                    let u = t - 2.25 / d;
                    n * u * u + 0.9375
                } else {
                    let u = t - 2.625 / d;
                    n * u * u + 0.984_375
                }
            }
            Easing::OutElastic => {
                if t <= 0.0 || t >= 1.0 {
                    t
                } else {
                    2.0_f32.powf(-10.0 * t) * f32::sin((t * 10.0 - 0.75) * (2.0 * PI / 3.0)) + 1.0
                }
            }
        }
    }
}

impl crate::PropType for Easing {
    fn into_value(self) -> Value {
        Value::custom(self)
    }
    fn from_value(value: &Value) -> Option<Self> {
        value.downcast::<Self>().copied()
    }
    fn fallback() -> Self {
        Easing::Linear
    }
}

/// How long and along which curve.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Tween {
    /// Milliseconds.
    pub duration: f32,
    /// The curve.
    pub easing: Easing,
}

impl Default for Tween {
    fn default() -> Self {
        // Qt's default duration.
        Tween {
            duration: 250.0,
            easing: Easing::Linear,
        }
    }
}

/// One step of an animation.
#[derive(Clone, Debug, PartialEq)]
pub enum Step {
    /// A property from a value (where it is, when `None`) to a value
    /// (where it is, when `None`).
    Tween {
        /// The property.
        target: PropId,
        /// From.
        from: Option<Value>,
        /// To.
        to: Option<Value>,
        /// How.
        tween: Tween,
    },
    /// Nothing, for milliseconds.
    Pause(f32),
    /// One after another.
    Sequential(Vec<Step>),
    /// All at once, as long as the longest.
    Parallel(Vec<Step>),
}

/// A tween with its times and values worked out.
#[derive(Clone, Debug)]
struct Segment {
    /// The property.
    target: PropId,
    /// When it starts, in ms from the animation's start.
    start: f64,
    /// When it ends.
    end: f64,
    /// From.
    from: Value,
    /// To.
    to: Value,
    /// The curve.
    easing: Easing,
}

/// `step` laid out in time from `at`, each property starting where the
/// step before left it (`last`). Answers when it ends.
fn lay_out(
    ui: &mut Ui,
    step: &Step,
    at: f64,
    last: &mut Vec<(PropId, Value)>,
    out: &mut Vec<Segment>,
) -> f64 {
    match step {
        Step::Pause(ms) => at + f64::from(ms.max(0.0)),
        Step::Sequential(steps) => steps
            .iter()
            .fold(at, |t, step| lay_out(ui, step, t, last, out)),
        Step::Parallel(steps) => steps
            .iter()
            .map(|step| lay_out(ui, step, at, last, out))
            .fold(at, f64::max),
        Step::Tween {
            target,
            from,
            to,
            tween,
        } => {
            let now = match last.iter().find(|(p, _)| p == target) {
                Some((_, v)) => v.clone(),
                None => ui.read(*target),
            };
            let from = from.clone().unwrap_or_else(|| now.clone());
            let to = to.clone().unwrap_or(now);
            let end = at + f64::from(tween.duration.max(0.0));
            last.retain(|(p, _)| p != target);
            last.push((*target, to.clone()));
            out.push(Segment {
                target: *target,
                start: at,
                end,
                from,
                to,
                easing: tween.easing,
            });
            end
        }
    }
}

/// `from` to `to` at progress `t`: reals and colours; anything else
/// jumps at the end.
fn mix(from: &Value, to: &Value, t: f32) -> Value {
    match (from, to) {
        (Value::Real(a), Value::Real(b)) => Value::Real(a + (b - a) * t),
        (Value::Int(a), Value::Int(b)) => {
            Value::Int((*a as f32 + (*b - *a) as f32 * t).round() as i32)
        }
        (Value::Color(a), Value::Color(b)) => {
            let ch = |x: u8, y: u8| {
                (f32::from(x) + (f32::from(y) - f32::from(x)) * t)
                    .round()
                    .clamp(0.0, 255.0) as u8
            };
            Value::Color(Color::rgba(
                ch(a.r, b.r),
                ch(a.g, b.g),
                ch(a.b, b.b),
                ch(a.a, b.a),
            ))
        }
        _ if t >= 1.0 => to.clone(),
        _ => from.clone(),
    }
}

/// An animation running.
#[derive(Debug)]
struct Running {
    /// Its segments.
    segments: Vec<Segment>,
    /// How long one run takes.
    length: f64,
    /// When it started, on the clock.
    started: f64,
    /// How many runs; below 0 for ever.
    loops: i32,
    /// Its id, to stop it.
    id: u64,
}

/// The clock and what runs on it.
#[derive(Debug, Default)]
pub(crate) struct Clock {
    /// Now, in ms, as last ticked.
    now: f64,
    /// What runs.
    running: Vec<Running>,
    /// The next id.
    next: u64,
    /// How many times it was ticked: a `Behavior` animates only what
    /// changes after the tick that followed its making, so a tree being
    /// set up shows its values at once, as QML's do before the component
    /// is complete.
    ticks: u64,
}

impl Ui {
    /// The clock reads `now` milliseconds: animations move to where they
    /// are then, and those that ended are dropped.
    pub fn tick(&mut self, now: f64) {
        self.clock.now = now;
        self.clock.ticks = self.clock.ticks.wrapping_add(1);
        let running = core::mem::take(&mut self.clock.running);
        let mut kept = Vec::new();
        for run in running {
            let elapsed = (now - run.started).max(0.0);
            let total = if run.loops < 0 {
                f64::INFINITY
            } else {
                run.length * f64::from(run.loops.max(1))
            };
            let done = elapsed >= total;
            let t = if done {
                run.length
            } else if run.length > 0.0 {
                elapsed % run.length
            } else {
                0.0
            };
            // Each property: its latest segment begun by now.
            let mut targets: Vec<PropId> = Vec::new();
            for segment in &run.segments {
                if !targets.contains(&segment.target) {
                    targets.push(segment.target);
                }
            }
            for target in targets {
                let segment = run
                    .segments
                    .iter()
                    .rev()
                    .find(|s| s.target == target && s.start <= t)
                    .or_else(|| run.segments.iter().find(|s| s.target == target));
                let Some(segment) = segment else { continue };
                let span = segment.end - segment.start;
                let progress = if span <= 0.0 {
                    1.0
                } else {
                    ((t - segment.start) / span).clamp(0.0, 1.0) as f32
                };
                let value = mix(&segment.from, &segment.to, segment.easing.apply(progress));
                self.write(target, value);
            }
            if !done {
                kept.push(run);
            }
        }
        // Animations started while these ran come after them.
        kept.append(&mut self.clock.running);
        self.clock.running = kept;
    }

    /// The clock's time.
    #[must_use]
    pub fn now(&self) -> f64 {
        self.clock.now
    }

    /// Whether anything is animating: a window keeps drawing frames
    /// while it is.
    #[must_use]
    pub fn animating(&self) -> bool {
        !self.clock.running.is_empty()
    }

    /// Run `step` now, `loops` times (below 0 for ever); answers its id.
    pub fn start_animation(&mut self, step: &Step, loops: i32) -> u64 {
        let mut segments = Vec::new();
        let length = lay_out(self, step, 0.0, &mut Vec::new(), &mut segments);
        let id = self.clock.next;
        self.clock.next += 1;
        // A property animated anew stops being animated by the others.
        let targets: Vec<PropId> = segments.iter().map(|s| s.target).collect();
        for run in &mut self.clock.running {
            run.segments.retain(|s| !targets.contains(&s.target));
        }
        self.clock.running.retain(|run| !run.segments.is_empty());
        self.clock.running.push(Running {
            segments,
            length,
            started: self.clock.now,
            loops,
            id,
        });
        id
    }

    /// Stop the animation `id`, leaving its properties where they are.
    pub fn stop_animation(&mut self, id: u64) {
        self.clock.running.retain(|run| run.id != id);
    }

    /// `NumberAnimation on x { ... }` and the rest: run `step` while
    /// `running` holds, `loops` times, from when it starts to hold; the
    /// animation goes with `owner`.
    pub fn animation_on(&mut self, owner: ItemId, step: Step, loops: i32, running: Prop<bool>) {
        let state: Rc<RefCell<Option<u64>>> = Rc::new(RefCell::new(None));
        let effect = Rc::new(move |ui: &mut Ui| {
            let want = ui.get(running);
            let id = *state.borrow();
            match (want, id) {
                (true, None) => {
                    let id = ui.start_animation(&step, loops);
                    *state.borrow_mut() = Some(id);
                }
                (false, Some(id)) => {
                    ui.stop_animation(id);
                    *state.borrow_mut() = None;
                }
                _ => {}
            }
        });
        self.add_effect(owner, effect);
    }

    /// `Behavior on prop { ... }`: from now on a change to what `prop` is
    /// given animates it there as `animate` says. With transitions, it
    /// animates only when the active state changed, as the first rule
    /// matching the old and new state says (`*` matches any), and jumps
    /// otherwise.
    pub fn behavior(&mut self, owner: ItemId, prop: PropId, animate: Animate) {
        let given = self.divert(prop, owner);
        let first = self.read(given);
        let shown = self.alloc(first.clone());
        self.own_cell(owner, shown);
        self.attach_direct(prop, Rc::new(move |ui: &mut Ui| ui.read(shown)));
        let last: Rc<RefCell<(Value, Option<Rc<str>>)>> = Rc::new(RefCell::new((first, None)));
        let made_at = self.clock.ticks;
        if let Animate::Transitions { active, .. } = &animate {
            let state = self.get(*active);
            last.borrow_mut().1 = Some(state);
        }
        let effect = Rc::new(move |ui: &mut Ui| {
            let target = ui.read(given);
            let state = match &animate {
                Animate::Transitions { active, .. } => Some(ui.get(*active)),
                Animate::Always(_) => None,
            };
            let (was, was_state) = last.borrow().clone();
            if target == was && state == was_state {
                return;
            }
            *last.borrow_mut() = (target.clone(), state.clone());
            if target == was {
                return;
            }
            let tween = match &animate {
                _ if ui.clock.ticks == made_at => None,
                Animate::Always(tween) => Some(*tween),
                Animate::Transitions { rules, .. } => match (&was_state, &state) {
                    (Some(from), Some(to)) if from != to => rules
                        .iter()
                        .find(|r| {
                            (&*r.from == "*" || r.from == *from) && (&*r.to == "*" || r.to == *to)
                        })
                        .map(|r| r.tween),
                    _ => None,
                },
            };
            match tween {
                Some(tween) => {
                    let step = Step::Tween {
                        target: shown,
                        from: None,
                        to: Some(target),
                        tween,
                    };
                    let _ = ui.start_animation(&step, 1);
                }
                None => {
                    ui.cancel_on(shown);
                    ui.write_direct(shown, target);
                }
            }
        });
        self.add_effect(owner, effect);
    }

    /// Stop animating `prop`.
    fn cancel_on(&mut self, prop: PropId) {
        for run in &mut self.clock.running {
            run.segments.retain(|s| s.target != prop);
        }
        self.clock.running.retain(|run| !run.segments.is_empty());
    }

    /// The state `item` is in: the first of `whens` whose condition
    /// holds, else what its `state` says. A cell owned by `item`.
    pub fn active_state(
        &mut self,
        item: ItemId,
        whens: Vec<(Rc<str>, When)>,
    ) -> Option<Prop<Rc<str>>> {
        let common = self.common(item)?;
        let active = self.prop(Rc::<str>::from(""));
        self.own_cell(item, active.id);
        active.bind(self, move |ui| {
            for (name, when) in &whens {
                if when(ui) {
                    return Rc::clone(name);
                }
            }
            ui.get(common.state)
        });
        Some(active)
    }

    /// `PropertyChanges`: `prop` is the value of the first of `values`
    /// whose state is `active`, else what it is given (diverted to a cell
    /// owned by `owner`).
    pub fn bind_states(
        &mut self,
        owner: ItemId,
        prop: PropId,
        active: Prop<Rc<str>>,
        values: Vec<(Rc<str>, StateValue)>,
    ) {
        let base = self.divert(prop, owner);
        self.attach_direct(
            prop,
            Rc::new(move |ui: &mut Ui| {
                let now = ui.get(active);
                for (name, value) in &values {
                    if *name == now {
                        return value(ui);
                    }
                }
                ui.read(base)
            }),
        );
    }
}

/// A `State`'s `when`.
pub type When = Rc<dyn Fn(&mut Ui) -> bool>;

/// A `PropertyChanges`' value for a property.
pub type StateValue = Rc<dyn Fn(&mut Ui) -> Value>;

/// How a `Behavior` or transitions animate a property.
#[derive(Clone, Debug)]
pub enum Animate {
    /// Every change, so.
    Always(Tween),
    /// Only a change of state, as the first matching rule says.
    Transitions {
        /// The active state.
        active: Prop<Rc<str>>,
        /// From, to, how.
        rules: Vec<Transition>,
    },
}

/// One `Transition`'s animation of a property.
#[derive(Clone, Debug, PartialEq)]
pub struct Transition {
    /// The state it leaves; `*` for any.
    pub from: Rc<str>,
    /// The state it enters; `*` for any.
    pub to: Rc<str>,
    /// How.
    pub tween: Tween,
}
