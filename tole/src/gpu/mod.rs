//! The GPU renderer: the same [`crate::Renderer`] as the CPU one, drawn by
//! Vulkan (`docs/UI-FRAMEWORK.md` §4.6, "The GPU renderer").
//!
//! Vulkan is loaded at run time (`libvulkan.so.1`, through `ash`), so a
//! program built with the `gpu` feature runs where there is none -- a
//! static build, a machine without a driver -- and draws on the CPU
//! instead: [`Gpu::new`] says why, and the window falls back. Nothing is
//! drawn on the GPU unless asked for: `TOLE_RENDERER=gpu` (any device,
//! lavapipe included) or `TOLE_RENDERER=auto` (a real GPU only), and the
//! CPU renderer stays the default and what the screenshots are compared
//! against.
//!
//! What the GPU draws itself: rectangles, filled and outlined, rounded or
//! not, antialiased by their distance in the fragment shader; text, from
//! `compositor-text`'s own glyph masks kept in an atlas, so a glyph's
//! pixels are the CPU's; pictures, sampled bilinearly; opacity groups, as
//! layers; cuts, as scissors -- every cut tole makes is a rectangle on
//! whole pixels. Paths and dashed outlines are filled into a coverage mask
//! by tiny-skia, as the CPU draws them, once for each shape and scale, and
//! the mask is drawn by the GPU from the atlas.
//!
//! A frame is recorded first ([`GpuFrame`] is the [`crate::Renderer`]),
//! then encoded into one command buffer and submitted, and the call waits
//! for it: one frame in flight, so the buffers it wrote are the
//! compositor's to read when it returns.

// A static program has the renderer's types but never a device, so most
// of it is never reached there; without a window (`wayland`), the targets
// a window draws into are never made.
#![cfg_attr(
    target_env = "musl",
    allow(
        dead_code,
        unused_imports,
        reason = "a static program never has a device"
    )
)]
#![cfg_attr(
    not(feature = "wayland"),
    allow(dead_code, reason = "without a window, its targets are never made")
)]

mod atlas;
mod device;
mod frame;
#[cfg(feature = "wayland")]
pub(crate) mod present;
mod target;

use std::cell::RefCell;
use std::rc::Rc;

use compositor_text::Fonts;

use crate::render::Rect;
use crate::value::Color;

pub use device::{Core, Node};
pub use frame::GpuRenderer;
pub use target::Target;

/// Which renderer a window draws with.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Choice {
    /// What `TOLE_RENDERER` says: `gpu`, `auto` or `cpu`; the CPU when it
    /// is unset or says anything else.
    #[default]
    FromEnv,
    /// The CPU renderer.
    Cpu,
    /// The GPU renderer on any Vulkan device, lavapipe included; the CPU
    /// when there is none.
    Gpu,
    /// The GPU renderer on a hardware device only; the CPU otherwise.
    Auto,
}

impl Choice {
    /// The choice `TOLE_RENDERER` makes, for [`Choice::FromEnv`].
    #[must_use]
    pub fn resolve(self) -> Choice {
        match self {
            Choice::FromEnv => match std::env::var("TOLE_RENDERER").as_deref() {
                Ok("gpu" | "vulkan") => Choice::Gpu,
                Ok("auto") => Choice::Auto,
                _ => Choice::Cpu,
            },
            other => other,
        }
    }
}

/// A Vulkan device tole draws with: the instance, the device, the
/// pipeline, the glyph atlas and the pictures uploaded. Cloning it shares
/// the device.
#[derive(Clone)]
pub struct Gpu {
    /// The device and everything made on it, shared with the frames
    /// made on it, which let their images go when dropped.
    pub(crate) inner: Rc<RefCell<device::Gpu>>,
}

impl std::fmt::Debug for Gpu {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Gpu")
            .field("device", &self.name())
            .finish_non_exhaustive()
    }
}

impl Gpu {
    /// Load Vulkan and make a device on the first GPU (a discrete one,
    /// then an integrated one, then any), or on the device whose name
    /// holds `TOLE_GPU_DEVICE`. `hardware_only` refuses a CPU device
    /// (lavapipe).
    ///
    /// # Errors
    ///
    /// Why there is no device: no Vulkan library, no device, or one
    /// without what tole needs (Vulkan 1.3's dynamic rendering).
    pub fn new(hardware_only: bool) -> Result<Gpu, String> {
        device::Gpu::new(hardware_only).map(|inner| Gpu {
            inner: Rc::new(RefCell::new(inner)),
        })
    }

    /// The device's name, as Vulkan gives it.
    #[must_use]
    pub fn name(&self) -> String {
        self.inner.borrow().name.clone()
    }

    /// Whether the device draws on the CPU (lavapipe).
    #[must_use]
    pub fn is_software(&self) -> bool {
        self.inner.borrow().software
    }

    /// Whether the device can render into memory the program already has
    /// mapped (`VK_EXT_external_memory_host`): a `wl_shm` buffer drawn
    /// into where it lies.
    #[must_use]
    pub fn can_import_host_memory(&self) -> bool {
        self.inner.borrow().host_import.is_some()
    }

    /// Whether the device can export a linear image as a dmabuf
    /// (`VK_EXT_external_memory_dma_buf` and
    /// `VK_EXT_image_drm_format_modifier`).
    #[must_use]
    pub fn can_export_dmabuf(&self) -> bool {
        self.inner.borrow().dmabuf.is_some()
    }
}

/// A device being made on a thread of its own (a driver's first start can
/// take over a second), while the window draws on the CPU.
///
/// The thread sends what it made, then wakes the window's loop: the loop
/// asks the channel, never whether the thread has ended, since a thread
/// that has woken it can still be returning -- and the loop, finding it
/// not done, then slept until something else woke it (the demo's GPU boot
/// on a loaded host, 2026-10-25).
#[derive(Debug)]
pub struct Pending {
    /// What the thread made, once it sends it.
    made: std::sync::mpsc::Receiver<Result<Core, String>>,
    /// What came, kept between [`Pending::ready`] and [`Pending::finish`].
    got: Option<Result<Core, String>>,
    /// Whether the thread could be started at all.
    started: bool,
}

impl Gpu {
    /// Start making a device on a thread, as [`Gpu::new`] would make it,
    /// preferring the one on DRM node `near` (the compositor's own) when
    /// it names one; `wake` is called once it is made or refused.
    #[must_use]
    pub fn start(
        hardware_only: bool,
        near: Option<(u32, u32)>,
        wake: Box<dyn FnOnce() + Send>,
    ) -> Pending {
        let wanted = std::env::var("TOLE_GPU_DEVICE").ok();
        let (send, made) = std::sync::mpsc::channel();
        let started = std::thread::Builder::new()
            .name("tole-gpu".to_owned())
            .spawn(move || {
                let core = Core::new(hardware_only, wanted, near);
                let _ = send.send(core);
                wake();
            })
            .is_ok();
        Pending {
            made,
            got: None,
            started,
        }
    }
}

impl Pending {
    /// Whether the device is made, or refused.
    #[must_use]
    pub fn ready(&mut self) -> bool {
        if !self.started {
            return true;
        }
        if self.got.is_none() {
            match self.made.try_recv() {
                Ok(made) => self.got = Some(made),
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    self.got = Some(Err("the thread making the device failed".to_owned()));
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => {}
            }
        }
        self.got.is_some()
    }

    /// The device, finished on this thread, or why there is none.
    ///
    /// # Errors
    ///
    /// The device could not be made.
    pub fn finish(mut self) -> Result<Gpu, String> {
        if !self.started {
            return Err("no thread to make the device".to_owned());
        }
        let core = match self.got.take() {
            Some(made) => made,
            None => self
                .made
                .recv()
                .map_err(|_| "the thread making the device failed".to_owned())?,
        }?;
        device::Gpu::from_core(core).map(|inner| Gpu {
            inner: Rc::new(RefCell::new(inner)),
        })
    }
}

/// A frame kept on the GPU between draws, so only what changed is drawn
/// again ([`Ui::render_into_gpu`]): the GPU's [`crate::Frame`].
pub struct GpuFrame {
    /// The device it lives on.
    gpu: Rc<RefCell<device::Gpu>>,
    /// Its image; `None` only while it is dropped.
    target: Option<Target>,
    /// What it shows.
    scene: Option<crate::damage::Scene>,
    /// Its scale.
    scale: f32,
    /// What is under everything.
    background: Color,
}

impl std::fmt::Debug for GpuFrame {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GpuFrame")
            .field("size", &self.pixel_size())
            .field("scale", &self.scale)
            .finish_non_exhaustive()
    }
}

impl GpuFrame {
    /// A frame of `width` × `height` logical pixels at `scale`, cleared to
    /// `background` when first drawn.
    ///
    /// # Errors
    ///
    /// The device could not make the image.
    pub fn new(
        gpu: &Gpu,
        (width, height): (u32, u32),
        scale: f32,
        background: Color,
    ) -> Result<Self, String> {
        let pixels = |logical: u32| (logical as f32 * scale).round().max(1.0) as u32;
        let target = Target::own(&mut gpu.inner.borrow_mut(), (pixels(width), pixels(height)))?;
        Ok(GpuFrame {
            gpu: Rc::clone(&gpu.inner),
            target: Some(target),
            scene: None,
            scale,
            background,
        })
    }

    /// Its size in buffer pixels.
    #[must_use]
    pub fn pixel_size(&self) -> (u32, u32) {
        self.target.as_ref().map_or((0, 0), Target::size)
    }

    /// Its pixels, read back: premultiplied RGBA, tiny-skia's order.
    ///
    /// # Errors
    ///
    /// The device failed.
    pub fn read(&mut self, fonts: &mut Fonts) -> Result<tiny_skia::Pixmap, String> {
        let Some(target) = self.target.as_mut() else {
            return Err("no image".to_owned());
        };
        let mut gpu = self.gpu.borrow_mut();
        let renderer = GpuRenderer::new(&mut gpu, fonts, target, self.scale);
        renderer.finish(&frame::After::Copy(Vec::new()))?;
        read_back(&gpu, target.size())
    }
}

impl Drop for GpuFrame {
    fn drop(&mut self) {
        if let (Some(target), Ok(mut gpu)) = (self.target.take(), self.gpu.try_borrow_mut()) {
            target.release(&mut gpu);
        }
    }
}

/// The read-back buffer's `size` pixels, BGRA, as a tiny-skia pixmap.
fn read_back(gpu: &device::Gpu, (w, h): (u32, u32)) -> Result<tiny_skia::Pixmap, String> {
    let mut pixmap = tiny_skia::Pixmap::new(w, h).ok_or_else(|| format!("no pixmap of {w}x{h}"))?;
    let Some(buffer) = &gpu.readback else {
        return Err("nothing was read back".to_owned());
    };
    let len = pixmap.data().len();
    if (buffer.size as usize) < len {
        return Err("the read-back buffer is too small".to_owned());
    }
    // SAFETY: the buffer is mapped for `buffer.size` bytes, at least `len`,
    // and the device finished writing it (the submission was waited for
    // and a barrier made the writes visible to the host).
    let bytes = unsafe { std::slice::from_raw_parts(buffer.ptr.cast_const(), len) };
    for (out, bgra) in pixmap
        .data_mut()
        .chunks_exact_mut(4)
        .zip(bytes.chunks_exact(4))
    {
        if let ([r, g, b, a], [bb, gg, rr, aa]) = (out, bgra) {
            (*r, *g, *b, *a) = (*rr, *gg, *bb, *aa);
        }
    }
    Ok(pixmap)
}

/// The rectangles `rects` (all of it when empty) of the read-back buffer,
/// a frame of `size` pixels, copied into `out`, a buffer of the same size
/// and byte order: the last resort when the device cannot draw into the
/// compositor's memory.
#[cfg(feature = "wayland")]
pub(crate) fn copy_read_back(
    gpu: &device::Gpu,
    (w, h): (u32, u32),
    rects: &[(u32, u32, u32, u32)],
    out: &mut [u8],
) -> Result<(), String> {
    let Some(buffer) = &gpu.readback else {
        return Err("nothing was read back".to_owned());
    };
    let len = w as usize * h as usize * 4;
    if (buffer.size as usize) < len || out.len() < len {
        return Err("the read-back buffer is too small".to_owned());
    }
    // SAFETY: as in `read_back`: mapped for at least `len` bytes, written
    // by a finished submission and made visible to the host.
    let bytes = unsafe { std::slice::from_raw_parts(buffer.ptr.cast_const(), len) };
    let whole = [(0, 0, w, h)];
    let rects = if rects.is_empty() { &whole[..] } else { rects };
    let stride = w as usize * 4;
    for &(x, y, rw, rh) in rects {
        for row in y as usize..(y + rh).min(h) as usize {
            let start = row * stride + x as usize * 4;
            let end = start + (rw.min(w.saturating_sub(x))) as usize * 4;
            if let (Some(from), Some(to)) = (bytes.get(start..end), out.get_mut(start..end)) {
                to.copy_from_slice(from);
            }
        }
    }
    Ok(())
}

/// Draw `ops` into `target` in the parts `rects` (buffer pixels; the
/// whole target when empty), each cleared to `background` first, then do
/// `after`; wait for the device when `wait`, else leave it running.
pub(crate) fn draw(
    gpu: &mut device::Gpu,
    fonts: &mut Fonts,
    target: &mut Target,
    (ops, scale): (&[crate::DrawOp], f32),
    rects: &[(u32, u32, u32, u32)],
    background: Color,
    (after, wait): (&frame::After, bool),
) -> Result<(), String> {
    let (w, h) = target.size();
    let whole = [(0, 0, w, h)];
    let rects = if rects.is_empty() { &whole[..] } else { rects };
    let mut renderer = GpuRenderer::new(gpu, fonts, target, scale);
    let s = scale.max(f32::EPSILON);
    for &(x, y, rw, rh) in rects {
        renderer.area((x, y, rw, rh), background);
        let logical = Rect::new(x as f32 / s, y as f32 / s, rw as f32 / s, rh as f32 / s);
        crate::damage::play_culled(ops, &mut renderer, logical, s);
    }
    if wait {
        renderer.finish(after)
    } else {
        renderer.finish_in_flight(after)
    }
}

impl crate::Ui {
    /// One whole frame of `width` × `height` logical pixels drawn on the
    /// GPU at the current scale, cleared to `background`, and read back:
    /// [`crate::Ui::render`]'s GPU form.
    ///
    /// # Errors
    ///
    /// The device failed.
    pub fn render_gpu(
        &mut self,
        gpu: &Gpu,
        size: (u32, u32),
        background: Color,
    ) -> Result<tiny_skia::Pixmap, String> {
        let scale = self.scale();
        let mut frame = GpuFrame::new(gpu, size, scale, background)?;
        let _ = self.render_into_gpu(&mut frame)?;
        let ops = frame.target.as_ref().map(Target::size);
        let Some(size) = ops else {
            return Err("no image".to_owned());
        };
        let device = frame.gpu.borrow();
        read_back(&device, size)
    }

    /// Draw into `frame` what changed since it was last drawn into, on
    /// the GPU, and answer those places in buffer pixels, as
    /// [`crate::Ui::render_into`] does; the drawn parts are copied back
    /// to the host, as a window's `wl_shm` buffer would take them.
    ///
    /// # Errors
    ///
    /// The device failed.
    pub fn render_into_gpu(
        &mut self,
        frame: &mut GpuFrame,
    ) -> Result<Vec<(u32, u32, u32, u32)>, String> {
        let size = frame.pixel_size();
        let (scene, pixels) = self.damage_since(frame.scene.as_ref(), frame.scale, size);
        if pixels.is_empty() {
            frame.scene = Some(scene);
            return Ok(pixels);
        }
        let Some(target) = frame.target.as_mut() else {
            return Err("no image".to_owned());
        };
        let mut gpu = frame.gpu.borrow_mut();
        draw(
            &mut gpu,
            &mut self.text.fonts,
            target,
            (&scene.ops, frame.scale),
            &pixels,
            frame.background,
            (&frame::After::Copy(pixels.clone()), true),
        )?;
        drop(gpu);
        frame.scene = Some(scene);
        Ok(pixels)
    }
}

/// Say `line` on standard error, where a program's warnings go.
pub(crate) fn present_warn(line: &str) {
    use std::io::Write as _;
    let _ = writeln!(std::io::stderr(), "{line}");
}

/// A device whose name holds `wanted`, for the tests of the fallback.
#[cfg(test)]
pub(crate) fn device_named(hardware_only: bool, wanted: &str) -> Result<Gpu, String> {
    device::Gpu::filtered(hardware_only, Some(wanted.to_owned())).map(|inner| Gpu {
        inner: Rc::new(RefCell::new(inner)),
    })
}
