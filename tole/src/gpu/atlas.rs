//! The atlas: where glyph and path masks are kept on the device, packed
//! on shelves, and found again by their key. When it is full it is
//! emptied and filled again from the next mask on.

use std::collections::HashMap;

/// A place in the atlas: left, top, width, height in texels.
pub(crate) type Place = (u32, u32, u32, u32);

/// A row of masks of about one height.
#[derive(Clone, Copy, Debug)]
struct Shelf {
    /// Its top.
    y: u32,
    /// Its height.
    height: u32,
    /// Where its next mask goes.
    x: u32,
}

/// The atlas's book-keeping; its texels are the device's.
#[derive(Debug)]
pub(crate) struct Atlas {
    /// Its width.
    width: u32,
    /// Its height.
    height: u32,
    /// The shelves, top first.
    shelves: Vec<Shelf>,
    /// Where each key's mask is.
    places: HashMap<u64, Place>,
    /// How many times it was emptied.
    pub(crate) resets: u64,
}

/// Texels left empty around each mask.
const GAP: u32 = 1;

impl Atlas {
    /// An empty atlas of `width` × `height`.
    pub(crate) fn new(width: u32, height: u32) -> Self {
        Atlas {
            width,
            height,
            shelves: Vec::new(),
            places: HashMap::new(),
            resets: 0,
        }
    }

    /// Where `key`'s mask is, if it is in.
    pub(crate) fn get(&self, key: u64) -> Option<Place> {
        self.places.get(&key).copied()
    }

    /// Whether a mask of `w` × `h` could ever be put in.
    pub(crate) fn fits(&self, w: u32, h: u32) -> bool {
        w.saturating_add(GAP) <= self.width && h.saturating_add(GAP) <= self.height
    }

    /// A place for `key`'s mask of `w` × `h`; `None` when the atlas is
    /// full (empty it with [`Atlas::clear`] and ask again).
    pub(crate) fn put(&mut self, key: u64, w: u32, h: u32) -> Option<Place> {
        if !self.fits(w, h) {
            return None;
        }
        let (gw, gh) = (w + GAP, h + GAP);
        // The lowest shelf it fits without wasting more than half of it.
        let best = self
            .shelves
            .iter()
            .enumerate()
            .filter(|(_, s)| s.height >= gh && s.height <= gh.saturating_mul(2).max(8))
            .filter(|(_, s)| s.x.saturating_add(gw) <= self.width)
            .min_by_key(|(_, s)| s.height)
            .map(|(i, _)| i);
        let at = match best {
            Some(i) => i,
            None => {
                let top = self.shelves.last().map_or(0, |s| s.y + s.height);
                if top.saturating_add(gh) > self.height {
                    return None;
                }
                self.shelves.push(Shelf {
                    y: top,
                    height: gh,
                    x: 0,
                });
                self.shelves.len() - 1
            }
        };
        let shelf = self.shelves.get_mut(at)?;
        let place = (shelf.x, shelf.y, w, h);
        shelf.x += gw;
        let _ = self.places.insert(key, place);
        Some(place)
    }

    /// Empty it.
    pub(crate) fn clear(&mut self) {
        self.shelves.clear();
        self.places.clear();
        self.resets += 1;
    }

    /// How many masks it holds.
    #[cfg(test)]
    pub(crate) fn len(&self) -> usize {
        self.places.len()
    }
}

#[cfg(test)]
mod tests {
    use super::Atlas;

    #[test]
    fn masks_are_packed_apart_and_found_again() {
        let mut atlas = Atlas::new(64, 64);
        let mut placed = Vec::new();
        for key in 0..20u64 {
            let w = 3 + (key % 5) as u32;
            let h = 4 + (key % 3) as u32;
            let place = atlas.put(key, w, h).expect("room");
            placed.push(place);
            assert_eq!(atlas.get(key), Some(place));
        }
        for (i, a) in placed.iter().enumerate() {
            for b in placed.iter().skip(i + 1) {
                let apart =
                    a.0 + a.2 <= b.0 || b.0 + b.2 <= a.0 || a.1 + a.3 <= b.1 || b.1 + b.3 <= a.1;
                assert!(apart, "{a:?} and {b:?} overlap");
            }
            assert!(a.0 + a.2 <= 64 && a.1 + a.3 <= 64);
        }
    }

    #[test]
    fn a_full_atlas_says_so_and_takes_more_once_emptied() {
        let mut atlas = Atlas::new(32, 32);
        let mut key = 0;
        while atlas.put(key, 15, 15).is_some() {
            key += 1;
        }
        assert_eq!(key, 4);
        assert!(atlas.put(99, 40, 2).is_none(), "wider than the atlas");
        atlas.clear();
        assert_eq!(atlas.len(), 0);
        assert!(atlas.put(100, 15, 15).is_some());
        assert_eq!(atlas.resets, 1);
    }
}
