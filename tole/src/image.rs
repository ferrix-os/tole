//! Pictures a painter draws: pixels a program has (a screenshot, a
//! decoded file), kept premultiplied as tiny-skia draws them, and shared,
//! so a frame drawn again does not copy them.

use std::fmt;
use std::rc::Rc;

/// A picture.
#[derive(Clone)]
pub struct Image(Rc<tiny_skia::Pixmap>);

impl Image {
    /// A picture `width` by `height` of RGBA pixels, not premultiplied,
    /// a row after another; `None` when the size and the pixels disagree.
    #[must_use]
    pub fn from_rgba(width: u32, height: u32, rgba: &[u8]) -> Option<Self> {
        let mut pixmap = tiny_skia::Pixmap::new(width, height)?;
        if rgba.len() != pixmap.data().len() {
            return None;
        }
        for (out, pixel) in pixmap.pixels_mut().iter_mut().zip(rgba.chunks_exact(4)) {
            if let [r, g, b, a] = *pixel {
                *out = tiny_skia::ColorU8::from_rgba(r, g, b, a).premultiply();
            }
        }
        Some(Image(Rc::new(pixmap)))
    }

    /// Its width and height in pixels.
    #[must_use]
    pub fn size(&self) -> (u32, u32) {
        (self.0.width(), self.0.height())
    }

    /// Its pixels.
    #[must_use]
    pub fn pixmap(&self) -> &tiny_skia::Pixmap {
        &self.0
    }
}

impl fmt::Debug for Image {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("Image").field(&self.size()).finish()
    }
}

impl PartialEq for Image {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.0, &other.0)
    }
}
