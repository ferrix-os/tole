//! Damage and partial redraw (`docs/UI-FRAMEWORK.md` §4.4).
//!
//! Each frame's scene remembers, for each item, the operations it drew,
//! where they fell and under which cut and opacity. The next frame's is
//! compared with it item by item: an item whose operations, place, cut or
//! opacity changed, or that came or went, damages where it was and where
//! it is. The damage, merged into a few rectangles, is all that is drawn
//! again -- every operation played, cut to each rectangle -- into a frame
//! the program keeps, and all that is handed to the compositor.

use std::collections::HashMap;

use crate::item::ItemId;
use crate::render::{DrawOp, Rect};

/// What one item drew.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Entry {
    /// The item.
    item: ItemId,
    /// Where its operations fell, in logical pixels, inside its cut.
    bounds: Rect,
    /// Its operations: the scene's `ops[start..end]`.
    start: usize,
    /// One past its last.
    end: usize,
    /// The cut it drew under.
    clip: Option<Rect>,
    /// The opacity groups around it, outermost first.
    opacity: Vec<f32>,
}

/// A frame's operations and who drew which.
#[derive(Clone, Debug, Default)]
pub(crate) struct Scene {
    /// The operations, in paint order.
    pub(crate) ops: Vec<DrawOp>,
    /// Each item's part.
    entries: Vec<Entry>,
    /// The cuts in force while building, innermost last.
    pub(crate) clips: Vec<Rect>,
    /// The opacity groups open while building.
    pub(crate) opacity: Vec<f32>,
}

impl Scene {
    /// Open a cut.
    pub(crate) fn push_clip(&mut self, rect: Rect) {
        let rect = match self.clips.last() {
            Some(outer) => outer.intersect(&rect),
            None => rect,
        };
        self.clips.push(rect);
    }

    /// Record that `item` drew `ops[start..]`, at `scale`.
    pub(crate) fn record(&mut self, item: ItemId, start: usize, scale: f32) {
        let end = self.ops.len();
        let mut bounds: Option<Rect> = None;
        for op in self.ops.get(start..end).unwrap_or(&[]) {
            let Some(rect) = op_bounds(op, scale) else {
                continue;
            };
            bounds = Some(bounds.map_or(rect, |b| union(&b, &rect)));
        }
        let Some(mut bounds) = bounds else {
            return;
        };
        let clip = self.clips.last().copied();
        if let Some(clip) = clip {
            bounds = bounds.intersect(&clip);
        }
        self.entries.push(Entry {
            item,
            bounds,
            start,
            end,
            clip,
            opacity: self.opacity.clone(),
        });
    }

    /// Where this scene differs from `before`: rectangles in logical
    /// pixels, not yet merged.
    pub(crate) fn damage_since(&self, before: &Scene) -> Vec<Rect> {
        let mut out = Vec::new();
        let old: HashMap<ItemId, &Entry> = before.entries.iter().map(|e| (e.item, e)).collect();
        let mut seen: HashMap<ItemId, ()> = HashMap::new();
        for entry in &self.entries {
            let _ = seen.insert(entry.item, ());
            match old.get(&entry.item) {
                Some(was)
                    if was.bounds == entry.bounds
                        && was.clip == entry.clip
                        && was.opacity == entry.opacity
                        && before.ops.get(was.start..was.end)
                            == self.ops.get(entry.start..entry.end) => {}
                Some(was) => {
                    out.push(was.bounds);
                    out.push(entry.bounds);
                }
                None => out.push(entry.bounds),
            }
        }
        for entry in &before.entries {
            if !seen.contains_key(&entry.item) {
                out.push(entry.bounds);
            }
        }
        // Paint order matters too: the same items in another order (a
        // child raised) redraw where they overlap. Compare the order of
        // what both frames hold.
        let order_now: Vec<ItemId> = self
            .entries
            .iter()
            .map(|e| e.item)
            .filter(|i| old.contains_key(i))
            .collect();
        let order_then: Vec<ItemId> = before
            .entries
            .iter()
            .map(|e| e.item)
            .filter(|i| seen.contains_key(i))
            .collect();
        if order_now != order_then {
            out.extend(self.entries.iter().map(|e| e.bounds));
        }
        out.retain(|r| r.w > 0.0 && r.h > 0.0);
        out
    }
}

/// Where `op` draws, in logical pixels, a pixel wider all round for
/// antialiasing; `None` for an operation that draws nothing itself.
pub(crate) fn op_bounds(op: &DrawOp, scale: f32) -> Option<Rect> {
    let grow = |r: Rect, by: f32| Rect::new(r.x - by, r.y - by, r.w + 2.0 * by, r.h + 2.0 * by);
    match op {
        DrawOp::Fill { rect, .. } | DrawOp::Stroke { rect, .. } => Some(grow(*rect, 1.0)),
        DrawOp::Text {
            run, x, baseline, ..
        } => {
            let s = scale.max(f32::EPSILON);
            let r = Rect::new(
                x / s,
                (baseline - run.ascent) / s,
                run.width / s,
                (run.ascent + run.descent) / s,
            );
            Some(grow(r, 2.0))
        }
        _ => None,
    }
}

/// The smallest rectangle holding both.
pub(crate) fn union(a: &Rect, b: &Rect) -> Rect {
    let x = a.x.min(b.x);
    let y = a.y.min(b.y);
    Rect::new(
        x,
        y,
        a.right().max(b.right()) - x,
        a.bottom().max(b.bottom()) - y,
    )
}

/// `rects` merged: overlapping or touching ones joined, and all of them
/// into one when there are more than `most`.
pub(crate) fn merge(mut rects: Vec<Rect>, most: usize) -> Vec<Rect> {
    let touches = |a: &Rect, b: &Rect| {
        a.x <= b.right() && b.x <= a.right() && a.y <= b.bottom() && b.y <= a.bottom()
    };
    let mut merged = true;
    while merged {
        merged = false;
        'outer: for i in 0..rects.len() {
            for j in i + 1..rects.len() {
                if let (Some(a), Some(b)) = (rects.get(i), rects.get(j))
                    && touches(a, b)
                {
                    let joined = union(a, b);
                    if let Some(slot) = rects.get_mut(i) {
                        *slot = joined;
                    }
                    let _ = rects.swap_remove(j);
                    merged = true;
                    break 'outer;
                }
            }
        }
    }
    if rects.len() > most {
        let all = rects
            .iter()
            .skip(1)
            .fold(rects.first().copied().unwrap_or_default(), |acc, r| {
                union(&acc, r)
            });
        return vec![all];
    }
    rects
}

/// `ops` played through `renderer`, but for the drawing whose bounds
/// miss `cull`: what a part of a frame needs, every pixel inside `cull`
/// as the whole frame has it.
pub(crate) fn play_culled(
    ops: &[DrawOp],
    renderer: &mut dyn crate::Renderer,
    cull: Rect,
    scale: f32,
) {
    for op in ops {
        if let Some(bounds) = op_bounds(op, scale) {
            let meet = bounds.intersect(&cull);
            if meet.w <= 0.0 || meet.h <= 0.0 {
                continue;
            }
        }
        crate::render::play(std::slice::from_ref(op), renderer);
    }
}
