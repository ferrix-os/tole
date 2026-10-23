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

/// What an `Image`'s `fillMode` may be, and its `status`.
pub mod fill_mode {
    /// Stretched to the item's size.
    pub const STRETCH: i32 = 0;
    /// Its shape kept, as large as fits, centred.
    pub const PRESERVE_ASPECT_FIT: i32 = 1;
    /// Its shape kept, covering the item, the overflow cut off evenly.
    pub const PRESERVE_ASPECT_CROP: i32 = 2;
}

/// An `Image`'s `status`.
pub mod image_status {
    /// No source.
    pub const NULL: i32 = 0;
    /// A picture it draws.
    pub const READY: i32 = 1;
    /// None there, or not one it reads.
    pub const ERROR: i32 = 3;
}

/// The pictures `Image` items drew, decoded and scaled to the pixels they
/// covered, so a frame drawn again decodes nothing. Keyed by the file, its
/// modification time, the size and the fill; dropped all at once when it
/// holds too many.
#[derive(Default)]
pub(crate) struct Pictures {
    held: std::collections::HashMap<PictureKey, Option<(Image, crate::Rect)>>,
}

/// A picture's file, modification time, size in pixels and fill.
type PictureKey = (Rc<str>, i64, u32, u32, i32);

impl fmt::Debug for Pictures {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Pictures")
            .field("held", &self.held.len())
            .finish()
    }
}

/// The most pictures kept.
const KEPT: usize = 256;

/// `pixmap` drawn into a new one `w` by `h`, scaled by `sx`, `sy` and
/// moved by `dx`, `dy`: halved first while it is more than twice as large,
/// so a photograph made small does not shimmer.
fn scaled(
    pixmap: &tiny_skia::Pixmap,
    w: u32,
    h: u32,
    sx: f32,
    sy: f32,
    dx: f32,
    dy: f32,
) -> Option<tiny_skia::Pixmap> {
    let mut source = pixmap.clone();
    let (mut sx, mut sy) = (sx, sy);
    while sx < 0.5 && sy < 0.5 && source.width() > 1 && source.height() > 1 {
        let (hw, hh) = (source.width().div_ceil(2), source.height().div_ceil(2));
        let mut half = tiny_skia::Pixmap::new(hw, hh)?;
        half.draw_pixmap(
            0,
            0,
            source.as_ref(),
            &tiny_skia::PixmapPaint {
                quality: tiny_skia::FilterQuality::Bilinear,
                ..tiny_skia::PixmapPaint::default()
            },
            tiny_skia::Transform::from_scale(
                hw as f32 / source.width() as f32,
                hh as f32 / source.height() as f32,
            ),
            None,
        );
        sx *= source.width() as f32 / hw as f32;
        sy *= source.height() as f32 / hh as f32;
        source = half;
    }
    let mut out = tiny_skia::Pixmap::new(w, h)?;
    out.draw_pixmap(
        0,
        0,
        source.as_ref(),
        &tiny_skia::PixmapPaint {
            quality: tiny_skia::FilterQuality::Bicubic,
            ..tiny_skia::PixmapPaint::default()
        },
        tiny_skia::Transform::from_row(sx, 0.0, 0.0, sy, dx, dy),
        None,
    );
    Some(out)
}

/// The picture at `path` for a place `w` by `h` device pixels, and where
/// in that place it goes (in the same pixels).
fn picture(path: &str, w: u32, h: u32, mode: i32) -> Option<(Image, crate::Rect)> {
    let decoded = compositor_image::load(
        std::path::Path::new(path),
        compositor_image::Fit::Within(w, h),
    )
    .ok()?;
    let (iw, ih) = (decoded.width() as f32, decoded.height() as f32);
    let (fw, fh) = (w as f32, h as f32);
    match mode {
        fill_mode::PRESERVE_ASPECT_FIT => {
            let s = (fw / iw).min(fh / ih);
            let (tw, th) = ((iw * s).round().max(1.0), (ih * s).round().max(1.0));
            let pixmap = scaled(&decoded, tw as u32, th as u32, tw / iw, th / ih, 0.0, 0.0)?;
            let rect =
                crate::Rect::new(((fw - tw) / 2.0).floor(), ((fh - th) / 2.0).floor(), tw, th);
            Some((Image(Rc::new(pixmap)), rect))
        }
        fill_mode::PRESERVE_ASPECT_CROP => {
            let s = (fw / iw).max(fh / ih);
            let (dx, dy) = ((fw - iw * s) / 2.0, (fh - ih * s) / 2.0);
            let pixmap = scaled(&decoded, w, h, s, s, dx, dy)?;
            Some((Image(Rc::new(pixmap)), crate::Rect::new(0.0, 0.0, fw, fh)))
        }
        _ => {
            let pixmap = scaled(&decoded, w, h, fw / iw, fh / ih, 0.0, 0.0)?;
            Some((Image(Rc::new(pixmap)), crate::Rect::new(0.0, 0.0, fw, fh)))
        }
    }
}

impl crate::Ui {
    /// A new `Image`'s properties.
    pub(crate) fn image_props(&mut self) -> crate::item::ImageProps {
        let props = crate::item::ImageProps {
            source: self.prop(Rc::<str>::from("")),
            fill_mode: self.prop(fill_mode::STRETCH),
            status: self.prop(image_status::NULL),
        };
        let source = props.source;
        props.status.bind(self, move |ui| {
            let path = crate::folder::expand_home(&ui.get(source));
            if path.is_empty() {
                return image_status::NULL;
            }
            let mut head = [0_u8; 512];
            let got = std::fs::File::open(&path)
                .and_then(|mut f| std::io::Read::read(&mut f, &mut head))
                .unwrap_or(0);
            if compositor_image::sniff(head.get(..got).unwrap_or_default()).is_some() {
                image_status::READY
            } else {
                image_status::ERROR
            }
        });
        props
    }

    /// An `Image`'s own properties.
    #[must_use]
    pub fn image_item(&self, item: crate::ItemId) -> Option<crate::item::ImageProps> {
        match self.kind(item)? {
            crate::Kind::Image(props) => Some(props),
            _ => None,
        }
    }

    /// An `Image`'s picture, in `place`.
    pub(crate) fn image_ops(
        &mut self,
        props: crate::item::ImageProps,
        place: crate::Rect,
        ops: &mut Vec<crate::DrawOp>,
    ) {
        let source = self.get(props.source);
        let mode = self.get(props.fill_mode);
        let _ = self.get(props.status);
        if source.is_empty() || place.w <= 0.0 || place.h <= 0.0 {
            return;
        }
        let scale = self.scale();
        let (w, h) = (
            (place.w * scale).round() as u32,
            (place.h * scale).round() as u32,
        );
        if w == 0 || h == 0 {
            return;
        }
        let path = crate::folder::expand_home(&source);
        let mtime = std::fs::metadata(&path).map_or(0, |meta| crate::files::modified_secs(&meta));
        let key = (Rc::from(path.as_str()), mtime, w, h, mode);
        if !self.pictures.held.contains_key(&key) {
            if self.pictures.held.len() >= KEPT {
                self.pictures.held.clear();
            }
            let made = picture(&path, w, h, mode);
            let _ = self.pictures.held.insert(key.clone(), made);
        }
        if let Some(Some((image, at))) = self.pictures.held.get(&key) {
            ops.push(crate::DrawOp::Image {
                rect: crate::Rect::new(
                    place.x + at.x / scale,
                    place.y + at.y / scale,
                    at.w / scale,
                    at.h / scale,
                ),
                image: image.clone(),
            });
        }
    }
}
