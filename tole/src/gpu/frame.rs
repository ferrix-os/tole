//! A frame on the GPU: the scene's operations recorded as instances of
//! one quad, then encoded into the command buffer and run.
//!
//! [`GpuRenderer`] is the [`Renderer`]. Each call becomes an instance
//! (where the quad goes, the colour, the shape, its kind, its texture
//! coordinates) and a draw of the instances that share a texture and a
//! scissor. A part of a frame ([`GpuRenderer::area`]) is cleared to the
//! background and everything after it is cut to it, so a frame drawn in
//! parts is the whole frame where the parts are, as the CPU's is.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash as _, Hasher as _};

use ash::vk;
use compositor_text::{Fonts, Run};

use super::device::{FORMAT, Gpu};
use super::target::{Kind, Target, drawing_layout};
use crate::render::{Rect, Renderer};
use crate::value::Color;

/// One instance's floats: rect, colour, shape, params, uv.
const FLOATS: usize = 20;

/// One instance's size in bytes, the vertex binding's stride.
pub(crate) const INSTANCE_BYTES: u32 = (FLOATS * 4) as u32;

/// What a draw samples.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Tex {
    /// The glyph atlas.
    Atlas,
    /// A cached picture or large mask.
    Texture(u64),
}

/// A pixel rectangle: left, top, right, bottom.
type Area = [i32; 4];

/// One recorded step.
#[derive(Clone, Copy, Debug)]
enum Cmd {
    /// Instances `first..first + count`.
    Draw {
        /// What they sample.
        tex: Tex,
        /// Where they may draw.
        scissor: Area,
        /// The first.
        first: u32,
        /// How many.
        count: u32,
    },
    /// Clear `area` to `color`.
    Clear {
        /// Where.
        area: Area,
        /// The premultiplied colour.
        color: [f32; 4],
    },
    /// Draw what follows into the layer at `depth`, its `area` cleared.
    PushLayer {
        /// The layer.
        depth: usize,
        /// The part drawn in.
        area: Area,
    },
    /// End the layer at `depth`, and draw instance `instance` with it.
    PopLayer {
        /// The layer.
        depth: usize,
        /// The instance that lays it down.
        instance: u32,
        /// Where.
        area: Area,
    },
}

/// Pixels on their way to an image: `offset` into the frame's upload
/// bytes, rows `w` texels long.
#[derive(Clone, Copy, Debug)]
struct Upload {
    /// Into the atlas, or a cached texture.
    to: Tex,
    /// Where the bytes start.
    offset: u64,
    /// Where they go.
    place: (u32, u32, u32, u32),
}

/// What happens after the frame is drawn.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum After {
    /// Nothing: a host or dmabuf target, made visible to the host.
    Present,
    /// These rectangles (`x`, `y`, `w`, `h`) copied out to the host:
    /// into a `HostCopy` target's buffer, or the read-back buffer.
    Copy(Vec<(u32, u32, u32, u32)>),
}

/// Records a scene for the GPU: the [`Renderer`] the GPU draws through.
pub struct GpuRenderer<'a> {
    /// The device.
    gpu: &'a mut Gpu,
    /// The glyphs.
    fonts: &'a mut Fonts,
    /// What is drawn into.
    target: &'a mut Target,
    /// Buffer pixels to a logical one.
    scale: f32,
    /// The instances.
    instances: Vec<f32>,
    /// The steps.
    cmds: Vec<Cmd>,
    /// Pixels to upload.
    uploads: Vec<Upload>,
    /// Their bytes.
    bytes: Vec<u8>,
    /// The part being drawn; the whole target until [`GpuRenderer::area`].
    area: Area,
    /// The cuts, innermost last: the logical rectangle and its pixels.
    clips: Vec<(Rect, Area)>,
    /// Layers open now.
    depth: usize,
    /// Their opacities, innermost last.
    opacity: Vec<f32>,
    /// Layers open when the last batch was encoded, innermost last.
    open: Vec<usize>,
    /// The first error; the frame is then not shown.
    error: Option<String>,
}

impl std::fmt::Debug for GpuRenderer<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GpuRenderer")
            .field("scale", &self.scale)
            .field("instances", &(self.instances.len() / FLOATS))
            .finish_non_exhaustive()
    }
}

/// A colour premultiplied, as floats.
fn premultiplied(color: Color) -> [f32; 4] {
    let a = f32::from(color.a) / 255.0;
    [
        f32::from(color.r) / 255.0 * a,
        f32::from(color.g) / 255.0 * a,
        f32::from(color.b) / 255.0 * a,
        a,
    ]
}

/// Two areas' common part.
fn meet(a: Area, b: Area) -> Area {
    [
        a[0].max(b[0]),
        a[1].max(b[1]),
        a[2].min(b[2]),
        a[3].min(b[3]),
    ]
}

/// Whether an area holds no pixel.
fn empty(a: Area) -> bool {
    a[2] <= a[0] || a[3] <= a[1]
}

/// A key from what `hash` writes.
fn key(hash: impl FnOnce(&mut DefaultHasher)) -> u64 {
    let mut hasher = DefaultHasher::new();
    hash(&mut hasher);
    hasher.finish()
}

impl<'a> GpuRenderer<'a> {
    /// A renderer into `target` at `scale`, with `fonts`' glyphs.
    pub(crate) fn new(
        gpu: &'a mut Gpu,
        fonts: &'a mut Fonts,
        target: &'a mut Target,
        scale: f32,
    ) -> Self {
        let (w, h) = target.size();
        gpu.frame += 1;
        let whole = [
            0,
            0,
            i32::try_from(w).unwrap_or(i32::MAX),
            i32::try_from(h).unwrap_or(i32::MAX),
        ];
        GpuRenderer {
            gpu,
            fonts,
            target,
            scale,
            instances: Vec::new(),
            cmds: Vec::new(),
            uploads: Vec::new(),
            bytes: Vec::new(),
            area: whole,
            clips: Vec::new(),
            depth: 0,
            opacity: Vec::new(),
            open: Vec::new(),
            error: None,
        }
    }

    /// The whole target.
    fn whole(&self) -> Area {
        let (w, h) = self.target.size();
        [
            0,
            0,
            i32::try_from(w).unwrap_or(i32::MAX),
            i32::try_from(h).unwrap_or(i32::MAX),
        ]
    }

    /// Draw what follows into the part `x`, `y`, `w`, `h` (buffer pixels),
    /// cleared to `background` first.
    pub(crate) fn area(&mut self, (x, y, w, h): (u32, u32, u32, u32), background: Color) {
        let int = |v: u32| i32::try_from(v).unwrap_or(i32::MAX);
        self.area = meet(
            [
                int(x),
                int(y),
                int(x.saturating_add(w)),
                int(y.saturating_add(h)),
            ],
            self.whole(),
        );
        self.clips.clear();
        self.cmds.push(Cmd::Clear {
            area: self.area,
            color: premultiplied(background),
        });
    }

    /// Where drawing may fall now.
    fn scissor(&self) -> Area {
        match self.clips.last() {
            Some((_, bounds)) => meet(*bounds, self.area),
            None => self.area,
        }
    }

    /// Whether a shape covering `rect` (logical) is cut away entirely.
    fn cut_away(&self, rect: Rect) -> bool {
        self.clips.last().is_some_and(|(clip, _)| {
            let inside = clip.intersect(&rect);
            inside.w <= 0.0 || inside.h <= 0.0
        })
    }

    /// Add an instance drawn with `tex`.
    fn push(&mut self, tex: Tex, instance: [f32; FLOATS]) {
        let scissor = self.scissor();
        if empty(scissor) {
            return;
        }
        let index = u32::try_from(self.instances.len() / FLOATS).unwrap_or(u32::MAX);
        self.instances.extend_from_slice(&instance);
        if let Some(Cmd::Draw {
            tex: last,
            scissor: area,
            first,
            count,
        }) = self.cmds.last_mut()
            && *last == tex
            && *area == scissor
            && *first + *count == index
        {
            *count += 1;
            return;
        }
        self.cmds.push(Cmd::Draw {
            tex,
            scissor,
            first: index,
            count: 1,
        });
    }

    /// Where the next upload's bytes start: the end of those before,
    /// padded to 16 bytes, since a copy into an image must start on a
    /// multiple of its texel's size (`vkCmdCopyBufferToImage`'s
    /// `bufferOffset`), and 16 is one for every format.
    fn aligned_offset(&mut self) -> u64 {
        let padded = self.bytes.len().next_multiple_of(16);
        self.bytes.resize(padded, 0);
        padded as u64
    }

    /// Note the first error.
    fn fail(&mut self, error: String) {
        if self.error.is_none() {
            self.error = Some(error);
        }
    }

    /// A mask's place in the atlas for `key`, uploading `coverage` when
    /// it is not there; the batch so far is drawn and the atlas emptied
    /// when it is full. `None` for a mask too large for the atlas.
    fn atlas_place(
        &mut self,
        key: u64,
        (w, h): (u32, u32),
        coverage: impl FnOnce(&mut Self) -> Option<Vec<u8>>,
    ) -> Option<(u32, u32)> {
        if let Some(place) = self.gpu.atlas.get(key) {
            return Some((place.0, place.1));
        }
        if !self.gpu.atlas.fits(w, h) {
            return None;
        }
        let bytes = coverage(self)?;
        let place = match self.gpu.atlas.put(key, w, h) {
            Some(place) => place,
            None => {
                if std::env::var_os("TOLE_GPU_TRACE").is_some() {
                    #[allow(clippy::print_stderr, reason = "a trace asked for")]
                    {
                        eprintln!("tole gpu: atlas full, flushing");
                    }
                }
                if let Err(error) = self.flush() {
                    self.fail(error);
                    return None;
                }
                self.gpu.atlas.clear();
                self.gpu.atlas.put(key, w, h)?
            }
        };
        let offset = self.aligned_offset();
        self.bytes.extend_from_slice(&bytes);
        self.uploads.push(Upload {
            to: Tex::Atlas,
            offset,
            place,
        });
        Some((place.0, place.1))
    }

    /// Draw a coverage mask whose top-left is at pixel (`left`, `top`),
    /// found by `key` or made by `make`, in `color`.
    fn mask(
        &mut self,
        key: u64,
        (left, top, w, h): (i64, i64, u32, u32),
        color: [f32; 4],
        make: impl FnOnce() -> Option<Vec<u8>>,
    ) {
        if w == 0 || h == 0 {
            return;
        }
        let mut make = Some(make);
        let at = self.atlas_place(key, (w, h), |_| make.take().and_then(|m| m()));
        let (tex, (u, v)) = match at {
            Some(at) => (Tex::Atlas, at),
            None => {
                // Larger than the atlas: a texture of its own.
                let Some(make) = make.take() else {
                    return;
                };
                if !self.texture_with(key, (w, h), vk::Format::R8_UNORM, make) {
                    return;
                }
                (Tex::Texture(key), (0, 0))
            }
        };
        let (x, y) = (left as f32, top as f32);
        self.push(
            tex,
            [
                x,
                y,
                x + w as f32,
                y + h as f32,
                color[0],
                color[1],
                color[2],
                color[3],
                x,
                y,
                x + w as f32,
                y + h as f32,
                0.0,
                0.0,
                3.0,
                0.0,
                u as f32,
                v as f32,
                0.0,
                0.0,
            ],
        );
    }

    /// Make sure the cached texture `key` exists, made of `size` texels in
    /// `format` from `bytes` when it does not. Whether it does now.
    fn texture_with(
        &mut self,
        key: u64,
        size: (u32, u32),
        format: vk::Format,
        bytes: impl FnOnce() -> Option<Vec<u8>>,
    ) -> bool {
        let frame = self.gpu.frame;
        if let Some(texture) = self.gpu.textures.get_mut(&key) {
            texture.used = frame;
            return true;
        }
        let Some(bytes) = bytes() else {
            return false;
        };
        match self.gpu.texture(
            size,
            format,
            vk::ImageUsageFlags::SAMPLED | vk::ImageUsageFlags::TRANSFER_DST,
        ) {
            Ok(texture) => {
                let _ = self.gpu.textures.insert(key, texture);
            }
            Err(error) => {
                self.fail(error);
                return false;
            }
        }
        let offset = self.aligned_offset();
        self.bytes.extend_from_slice(&bytes);
        self.uploads.push(Upload {
            to: Tex::Texture(key),
            offset,
            place: (0, 0, size.0, size.1),
        });
        true
    }

    /// A tiny-skia path's coverage drawn by the CPU, `fill` or stroked
    /// by `stroke`, at its own place in pixels, then laid down by the GPU
    /// in `color`.
    fn skia_mask(
        &mut self,
        path: &tiny_skia::Path,
        stroke: Option<&tiny_skia::Stroke>,
        color: Color,
    ) {
        let grow = stroke.map_or(1.0, |s| s.width * 2.0 + 2.0);
        let b = path.bounds();
        let (left, top) = ((b.left() - grow).floor(), (b.top() - grow).floor());
        let (right, bottom) = ((b.right() + grow).ceil(), (b.bottom() + grow).ceil());
        let (w, h) = (
            (right - left).max(0.0) as u32,
            (bottom - top).max(0.0) as u32,
        );
        if w == 0 || h == 0 || w > 16_384 || h > 16_384 {
            return;
        }
        // The same shape anywhere else whole pixels away has the same
        // coverage, and the same key.
        let shifted = path
            .clone()
            .transform(tiny_skia::Transform::from_translate(-left, -top));
        let Some(shifted) = shifted else {
            return;
        };
        let k = key(|hasher| {
            1u8.hash(hasher);
            for p in shifted.points() {
                p.x.to_bits().hash(hasher);
                p.y.to_bits().hash(hasher);
            }
            for v in shifted.verbs() {
                (*v as u8).hash(hasher);
            }
            if let Some(s) = stroke {
                s.width.to_bits().hash(hasher);
                (s.line_cap as u8).hash(hasher);
                (s.line_join as u8).hash(hasher);
                format!("{:?}", s.dash).hash(hasher);
            }
            (w, h).hash(hasher);
        });
        let stroke = stroke.cloned();
        self.mask(
            k,
            (left as i64, top as i64, w, h),
            premultiplied(color),
            move || coverage_of(&shifted, stroke.as_ref(), (w, h)),
        );
    }

    /// Draw everything recorded so far, and go on from there.
    fn flush(&mut self) -> Result<(), String> {
        let after = After::Copy(Vec::new());
        self.encode(&after, false)
    }

    /// Draw everything recorded and run `after`.
    ///
    /// # Errors
    ///
    /// The device failed; the target then holds no good frame.
    pub(crate) fn finish(mut self, after: &After) -> Result<(), String> {
        if let Some(error) = self.error.take() {
            return Err(error);
        }
        // Layers left open are laid down, as the CPU leaves none open.
        while self.depth > 0 {
            self.pop_opacity();
        }
        self.encode(after, true)?;
        if self.gpu.frame.is_multiple_of(64) {
            self.gpu.forget_unused(240);
        }
        Ok(())
    }

    /// Encode the batch, submit it and wait; then `after` when `last`.
    fn encode(&mut self, after: &After, last: bool) -> Result<(), String> {
        let gpu = &mut *self.gpu;
        let floats = self.instances.len();
        let mut slot = gpu.instances.take();
        gpu.ensure(
            &mut slot,
            (floats * 4) as u64,
            vk::BufferUsageFlags::VERTEX_BUFFER,
            false,
        )?;
        gpu.instances = slot;
        let mut slot = gpu.staging.take();
        gpu.ensure(
            &mut slot,
            self.bytes.len() as u64,
            vk::BufferUsageFlags::TRANSFER_SRC,
            false,
        )?;
        gpu.staging = slot;
        let (Some(instances), Some(staging)) = (&gpu.instances, &gpu.staging) else {
            return Err("no buffers".to_owned());
        };
        // SAFETY: the instance buffer is mapped, holds at least `floats`
        // floats, and the device is not reading it (one frame in flight,
        // waited for).
        unsafe {
            std::ptr::copy_nonoverlapping(
                self.instances.as_ptr().cast::<u8>(),
                instances.ptr,
                floats * 4,
            );
        }
        // SAFETY: as above, for the upload bytes.
        unsafe {
            std::ptr::copy_nonoverlapping(self.bytes.as_ptr(), staging.ptr, self.bytes.len());
        }
        let (instance_buffer, staging_buffer) = (instances.buffer, staging.buffer);
        gpu.begin()?;
        let d = gpu.device.clone();
        let cmd = gpu.cmd;
        // The uploads.
        for upload in std::mem::take(&mut self.uploads) {
            let texture = match upload.to {
                Tex::Atlas => &mut gpu.atlas_texture,
                Tex::Texture(k) => match gpu.textures.get_mut(&k) {
                    Some(texture) => texture,
                    None => continue,
                },
            };
            let (image, from) = (texture.image, texture.layout);
            texture.layout = vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL;
            let shader = (
                vk::PipelineStageFlags2::FRAGMENT_SHADER,
                vk::AccessFlags2::SHADER_SAMPLED_READ,
            );
            let copy = (
                vk::PipelineStageFlags2::COPY,
                vk::AccessFlags2::TRANSFER_WRITE,
            );
            gpu.barrier(
                image,
                from,
                vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                shader,
                copy,
            );
            let (x, y, w, h) = upload.place;
            let region = [vk::BufferImageCopy {
                buffer_offset: upload.offset,
                buffer_row_length: 0,
                buffer_image_height: 0,
                image_subresource: vk::ImageSubresourceLayers {
                    aspect_mask: vk::ImageAspectFlags::COLOR,
                    mip_level: 0,
                    base_array_layer: 0,
                    layer_count: 1,
                },
                image_offset: vk::Offset3D {
                    x: i32::try_from(x).unwrap_or(0),
                    y: i32::try_from(y).unwrap_or(0),
                    z: 0,
                },
                image_extent: vk::Extent3D {
                    width: w,
                    height: h,
                    depth: 1,
                },
            }];
            // SAFETY: recording; the region lies inside both the staging
            // buffer (the bytes were put there) and the image.
            unsafe {
                d.cmd_copy_buffer_to_image(
                    cmd,
                    staging_buffer,
                    image,
                    vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                    &region,
                );
            }
            gpu.barrier(
                image,
                vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL,
                copy,
                shader,
            );
        }
        // The target, then any layer open from the last batch.
        let attach = (
            vk::PipelineStageFlags2::COLOR_ATTACHMENT_OUTPUT,
            vk::AccessFlags2::COLOR_ATTACHMENT_READ | vk::AccessFlags2::COLOR_ATTACHMENT_WRITE,
        );
        let target_layout = drawing_layout(&self.target.kind);
        let from = self.target.texture.layout;
        gpu.barrier(
            self.target.texture.image,
            from,
            target_layout,
            (
                vk::PipelineStageFlags2::ALL_COMMANDS,
                vk::AccessFlags2::MEMORY_READ | vk::AccessFlags2::MEMORY_WRITE,
            ),
            attach,
        );
        self.target.texture.layout = target_layout;
        let size = self.target.size();
        let begin = |gpu: &Gpu, view: vk::ImageView, layout: vk::ImageLayout| {
            let color = [vk::RenderingAttachmentInfo::default()
                .image_view(view)
                .image_layout(layout)
                .load_op(vk::AttachmentLoadOp::LOAD)
                .store_op(vk::AttachmentStoreOp::STORE)];
            let info = vk::RenderingInfo::default()
                .render_area(vk::Rect2D {
                    offset: vk::Offset2D { x: 0, y: 0 },
                    extent: vk::Extent2D {
                        width: size.0,
                        height: size.1,
                    },
                })
                .layer_count(1)
                .color_attachments(&color);
            // SAFETY: recording, outside any rendering; the view is of an
            // image in `layout`, the frame's size.
            unsafe { gpu.device.cmd_begin_rendering(gpu.cmd, &info) };
            let viewport = [vk::Viewport {
                x: 0.0,
                y: 0.0,
                width: size.0 as f32,
                height: size.1 as f32,
                min_depth: 0.0,
                max_depth: 1.0,
            }];
            // SAFETY: recording; the pipeline takes a dynamic viewport.
            unsafe { gpu.device.cmd_set_viewport(gpu.cmd, 0, &viewport) };
        };
        let level_view = |gpu: &Gpu, open: &[usize], target: &Target| match open.last() {
            Some(&depth) => gpu
                .layers
                .get(depth)
                .map_or((target.texture.view, target_layout), |l| {
                    (l.view, vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL)
                }),
            None => (target.texture.view, target_layout),
        };
        let (view, layout) = level_view(gpu, &self.open, self.target);
        begin(gpu, view, layout);
        // SAFETY: recording; the pipeline is this device's.
        unsafe { d.cmd_bind_pipeline(cmd, vk::PipelineBindPoint::GRAPHICS, gpu.pipeline) };
        // SAFETY: recording; the buffer holds the instances.
        unsafe { d.cmd_bind_vertex_buffers(cmd, 0, &[instance_buffer], &[0]) };
        let mut push = [0u8; 8];
        push[..4].copy_from_slice(&(size.0 as f32).to_le_bytes());
        push[4..].copy_from_slice(&(size.1 as f32).to_le_bytes());
        // SAFETY: recording; eight bytes, as the layout's range.
        unsafe {
            d.cmd_push_constants(
                cmd,
                gpu.pipeline_layout,
                vk::ShaderStageFlags::VERTEX,
                0,
                &push,
            );
        }
        let rect = |a: Area| vk::Rect2D {
            offset: vk::Offset2D {
                x: a[0].max(0),
                y: a[1].max(0),
            },
            extent: vk::Extent2D {
                width: u32::try_from(a[2] - a[0].max(0)).unwrap_or(0),
                height: u32::try_from(a[3] - a[1].max(0)).unwrap_or(0),
            },
        };
        let mut bound = None;
        for step in std::mem::take(&mut self.cmds) {
            match step {
                Cmd::Draw {
                    tex,
                    scissor,
                    first,
                    count,
                } => {
                    if empty(scissor) {
                        continue;
                    }
                    let set = match tex {
                        Tex::Atlas => gpu.atlas_texture.set,
                        Tex::Texture(k) => match gpu.textures.get(&k) {
                            Some(t) => t.set,
                            None => continue,
                        },
                    };
                    if bound != Some(set) {
                        // SAFETY: recording; the set samples an image now
                        // in SHADER_READ_ONLY_OPTIMAL.
                        unsafe {
                            d.cmd_bind_descriptor_sets(
                                cmd,
                                vk::PipelineBindPoint::GRAPHICS,
                                gpu.pipeline_layout,
                                0,
                                &[set],
                                &[],
                            );
                        }
                        bound = Some(set);
                    }
                    // SAFETY: recording; the scissor lies in the frame.
                    unsafe { d.cmd_set_scissor(cmd, 0, &[rect(scissor)]) };
                    // SAFETY: recording, inside rendering, with the
                    // pipeline, buffer and set bound; the instances exist.
                    unsafe { d.cmd_draw(cmd, 6, count, 0, first) };
                }
                Cmd::Clear { area, color } => {
                    if empty(area) {
                        continue;
                    }
                    let clear = [vk::ClearAttachment {
                        aspect_mask: vk::ImageAspectFlags::COLOR,
                        color_attachment: 0,
                        clear_value: vk::ClearValue {
                            color: vk::ClearColorValue { float32: color },
                        },
                    }];
                    let rects = [vk::ClearRect {
                        rect: rect(area),
                        base_array_layer: 0,
                        layer_count: 1,
                    }];
                    // SAFETY: recording, inside rendering; the rectangle
                    // lies in the frame.
                    unsafe { d.cmd_clear_attachments(cmd, &clear, &rects) };
                }
                Cmd::PushLayer { depth, area } => {
                    // SAFETY: recording, inside rendering.
                    unsafe { d.cmd_end_rendering(cmd) };
                    let Some(layer) = gpu.layers.get_mut(depth) else {
                        return Err("a layer was not made".to_owned());
                    };
                    let (image, view) = (layer.image, layer.view);
                    layer.layout = vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL;
                    gpu.barrier(
                        image,
                        vk::ImageLayout::UNDEFINED,
                        vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL,
                        (
                            vk::PipelineStageFlags2::FRAGMENT_SHADER,
                            vk::AccessFlags2::SHADER_SAMPLED_READ,
                        ),
                        attach,
                    );
                    self.open.push(depth);
                    begin(gpu, view, vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL);
                    let clear = [vk::ClearAttachment {
                        aspect_mask: vk::ImageAspectFlags::COLOR,
                        color_attachment: 0,
                        clear_value: vk::ClearValue {
                            color: vk::ClearColorValue { float32: [0.0; 4] },
                        },
                    }];
                    if !empty(area) {
                        let rects = [vk::ClearRect {
                            rect: rect(area),
                            base_array_layer: 0,
                            layer_count: 1,
                        }];
                        // SAFETY: as for a clear above.
                        unsafe { d.cmd_clear_attachments(cmd, &clear, &rects) };
                    }
                }
                Cmd::PopLayer {
                    depth,
                    instance,
                    area,
                } => {
                    // SAFETY: recording, inside rendering.
                    unsafe { d.cmd_end_rendering(cmd) };
                    let _ = self.open.pop();
                    let Some(layer) = gpu.layers.get_mut(depth) else {
                        return Err("a layer was not made".to_owned());
                    };
                    let (image, set) = (layer.image, layer.set);
                    layer.layout = vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL;
                    gpu.barrier(
                        image,
                        vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL,
                        vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL,
                        attach,
                        (
                            vk::PipelineStageFlags2::FRAGMENT_SHADER,
                            vk::AccessFlags2::SHADER_SAMPLED_READ,
                        ),
                    );
                    let (view, layout) = level_view(gpu, &self.open, self.target);
                    let under = match self.open.last() {
                        Some(&d) => gpu.layers.get(d).map(|l| l.image),
                        None => Some(self.target.texture.image),
                    };
                    if let Some(under) = under {
                        gpu.barrier(under, layout, layout, attach, attach);
                    }
                    begin(gpu, view, layout);
                    if empty(area) {
                        continue;
                    }
                    // SAFETY: recording; the set samples the layer, now
                    // in SHADER_READ_ONLY_OPTIMAL.
                    unsafe {
                        d.cmd_bind_descriptor_sets(
                            cmd,
                            vk::PipelineBindPoint::GRAPHICS,
                            gpu.pipeline_layout,
                            0,
                            &[set],
                            &[],
                        );
                    }
                    bound = Some(set);
                    // SAFETY: as for a draw above.
                    unsafe { d.cmd_set_scissor(cmd, 0, &[rect(area)]) };
                    // SAFETY: as for a draw above.
                    unsafe { d.cmd_draw(cmd, 6, 1, 0, instance) };
                }
            }
        }
        // SAFETY: recording, inside rendering.
        unsafe { d.cmd_end_rendering(cmd) };
        self.instances.clear();
        self.bytes.clear();
        if last {
            self.after(after)?;
        }
        self.gpu.submit_and_wait()
    }

    /// Record what `after` asks once the frame is drawn.
    fn after(&mut self, after: &After) -> Result<(), String> {
        let gpu = &mut *self.gpu;
        let d = gpu.device.clone();
        let image = self.target.texture.image;
        let layout = self.target.texture.layout;
        let attach = (
            vk::PipelineStageFlags2::COLOR_ATTACHMENT_OUTPUT,
            vk::AccessFlags2::COLOR_ATTACHMENT_WRITE,
        );
        let host = (vk::PipelineStageFlags2::HOST, vk::AccessFlags2::HOST_READ);
        let rects = match after {
            After::Present => {
                gpu.barrier(image, layout, layout, attach, host);
                return Ok(());
            }
            After::Copy(rects) => rects,
        };
        let (w, h) = self.target.size();
        let whole = [(0, 0, w, h)];
        let rects: &[(u32, u32, u32, u32)] = if rects.is_empty() { &whole } else { rects };
        let buffer = match &self.target.kind {
            Kind::HostCopy { buffer, .. } => *buffer,
            Kind::Own => {
                let mut slot = gpu.readback.take();
                gpu.ensure(
                    &mut slot,
                    u64::from(w) * u64::from(h) * 4,
                    vk::BufferUsageFlags::TRANSFER_DST,
                    true,
                )?;
                gpu.readback = slot;
                match &gpu.readback {
                    Some(b) => b.buffer,
                    None => return Err("no read-back buffer".to_owned()),
                }
            }
            Kind::Host | Kind::Dmabuf { .. } => {
                gpu.barrier(image, layout, layout, attach, host);
                return Ok(());
            }
        };
        let copy = (
            vk::PipelineStageFlags2::COPY,
            vk::AccessFlags2::TRANSFER_READ,
        );
        gpu.barrier(
            image,
            layout,
            vk::ImageLayout::TRANSFER_SRC_OPTIMAL,
            attach,
            copy,
        );
        self.target.texture.layout = vk::ImageLayout::TRANSFER_SRC_OPTIMAL;
        let regions: Vec<vk::BufferImageCopy> = rects
            .iter()
            .filter(|r| r.2 > 0 && r.3 > 0 && r.0 + r.2 <= w && r.1 + r.3 <= h)
            .map(|&(x, y, rw, rh)| vk::BufferImageCopy {
                buffer_offset: (u64::from(y) * u64::from(w) + u64::from(x)) * 4,
                buffer_row_length: w,
                buffer_image_height: h,
                image_subresource: vk::ImageSubresourceLayers {
                    aspect_mask: vk::ImageAspectFlags::COLOR,
                    mip_level: 0,
                    base_array_layer: 0,
                    layer_count: 1,
                },
                image_offset: vk::Offset3D {
                    x: i32::try_from(x).unwrap_or(0),
                    y: i32::try_from(y).unwrap_or(0),
                    z: 0,
                },
                image_extent: vk::Extent3D {
                    width: rw,
                    height: rh,
                    depth: 1,
                },
            })
            .collect();
        if !regions.is_empty() {
            // SAFETY: recording, outside rendering; the image is in
            // TRANSFER_SRC_OPTIMAL and every region lies in it and in the
            // buffer, which is the frame's size.
            unsafe {
                d.cmd_copy_image_to_buffer(
                    gpu.cmd,
                    image,
                    vk::ImageLayout::TRANSFER_SRC_OPTIMAL,
                    buffer,
                    &regions,
                );
            }
        }
        let barrier = [vk::MemoryBarrier2::default()
            .src_stage_mask(vk::PipelineStageFlags2::COPY)
            .src_access_mask(vk::AccessFlags2::TRANSFER_WRITE)
            .dst_stage_mask(vk::PipelineStageFlags2::HOST)
            .dst_access_mask(vk::AccessFlags2::HOST_READ)];
        let info = vk::DependencyInfo::default().memory_barriers(&barrier);
        // SAFETY: recording.
        unsafe { d.cmd_pipeline_barrier2(gpu.cmd, &info) };
        Ok(())
    }
}

/// `path`'s coverage, filled or stroked, in a `w` × `h` mask, as
/// tiny-skia draws it for the CPU renderer.
fn coverage_of(
    path: &tiny_skia::Path,
    stroke: Option<&tiny_skia::Stroke>,
    (w, h): (u32, u32),
) -> Option<Vec<u8>> {
    let mut pixmap = tiny_skia::Pixmap::new(w, h)?;
    let mut paint = tiny_skia::Paint::default();
    paint.set_color(tiny_skia::Color::WHITE);
    paint.anti_alias = true;
    match stroke {
        Some(stroke) => {
            pixmap.stroke_path(path, &paint, stroke, tiny_skia::Transform::identity(), None);
        }
        None => pixmap.fill_path(
            path,
            &paint,
            tiny_skia::FillRule::Winding,
            tiny_skia::Transform::identity(),
            None,
        ),
    }
    Some(
        pixmap
            .data()
            .chunks_exact(4)
            .map(|p| p.get(3).copied().unwrap_or(0))
            .collect(),
    )
}

impl Renderer for GpuRenderer<'_> {
    fn fill(&mut self, rect: Rect, radius: f32, color: Color) {
        if color.is_transparent() || self.cut_away(rect) {
            return;
        }
        let s = self.scale;
        let (x, y, w, h) = (rect.x * s, rect.y * s, rect.w * s, rect.h * s);
        if w <= 0.0 || h <= 0.0 {
            return;
        }
        let r = (radius * s).min(w / 2.0).min(h / 2.0);
        let c = premultiplied(color);
        let (kind, grow) = if r > 0.0 { (1.0, 1.0) } else { (0.0, 0.0) };
        // A square rectangle is not antialiased, and tiny-skia's scan
        // converter rounds its edges half up (Skia's `SkFDot6Round`): an
        // edge on a pixel's centre leaves that pixel out on top and left,
        // where the GPU's top-left rule would take it in.
        let (x, y, w, h) = if r > 0.0 {
            (x, y, w, h)
        } else {
            let snap = |v: f32| (v + 0.5).floor();
            let (x0, y0) = (snap(x), snap(y));
            (x0, y0, snap(x + w) - x0, snap(y + h) - y0)
        };
        if w <= 0.0 || h <= 0.0 {
            return;
        }
        self.push(
            Tex::Atlas,
            [
                x - grow,
                y - grow,
                x + w + grow,
                y + h + grow,
                c[0],
                c[1],
                c[2],
                c[3],
                x,
                y,
                x + w,
                y + h,
                r.max(0.0),
                0.0,
                kind,
                0.0,
                0.0,
                0.0,
                0.0,
                0.0,
            ],
        );
    }

    fn stroke(&mut self, rect: Rect, radius: f32, width: f32, color: Color, dash: f32) {
        if color.is_transparent() || width <= 0.0 || self.cut_away(rect) {
            return;
        }
        let half = width / 2.0;
        let inner = Rect::new(rect.x + half, rect.y + half, rect.w - width, rect.h - width);
        let corner = if dash > 0.0 {
            radius
        } else {
            (radius - half).max(0.0)
        };
        let s = self.scale;
        if dash > 0.0 {
            let Some(path) = crate::cpu::rounded_path(inner, corner, s) else {
                return;
            };
            let stroke = tiny_skia::Stroke {
                width: width * s,
                dash: tiny_skia::StrokeDash::new(vec![dash * s, dash * s], 0.0),
                ..tiny_skia::Stroke::default()
            };
            self.skia_mask(&path, Some(&stroke), color);
            return;
        }
        let (x, y, w, h) = (inner.x * s, inner.y * s, inner.w * s, inner.h * s);
        if w <= 0.0 || h <= 0.0 {
            return;
        }
        let r = (corner * s).min(w / 2.0).min(h / 2.0);
        let lw = width * s;
        let grow = lw / 2.0 + 1.0;
        let c = premultiplied(color);
        self.push(
            Tex::Atlas,
            [
                x - grow,
                y - grow,
                x + w + grow,
                y + h + grow,
                c[0],
                c[1],
                c[2],
                c[3],
                x,
                y,
                x + w,
                y + h,
                r.max(0.0),
                lw,
                2.0,
                0.0,
                0.0,
                0.0,
                0.0,
                0.0,
            ],
        );
    }

    fn text(&mut self, run: &Run, x: f32, baseline: f32, color: Color) {
        if color.is_transparent() || empty(self.scissor()) {
            return;
        }
        let c = premultiplied(color);
        for glyph in &run.glyphs {
            // Placed as compositor-text places it: the pen on a quarter
            // of a pixel, the baseline on a whole one.
            let pen = x + glyph.x;
            let whole = pen.floor();
            let mut quarter = ((pen - whole) * 4.0).round();
            let mut column = whole as i64;
            if quarter >= 4.0 {
                quarter = 0.0;
                column += 1;
            }
            let row = (baseline - glyph.y).round() as i64;
            let k = key(|h| {
                0u8.hash(h);
                glyph.face.0.hash(h);
                glyph.id.hash(h);
                run.px.to_bits().hash(h);
                (quarter as u8).hash(h);
            });
            let Some(mask) = self
                .fonts
                .glyph(glyph.face, glyph.id, run.px, quarter / 4.0)
            else {
                continue;
            };
            let (left, top, w, h) = (
                i64::from(mask.left),
                i64::from(mask.top),
                mask.width,
                mask.height,
            );
            let bytes = mask.coverage.clone();
            self.mask(k, (column + left, row - top, w, h), c, move || Some(bytes));
        }
    }

    fn path(&mut self, path: &crate::shape::Path, fill: Color, stroke: Color, width: f32) {
        let Some(bounds) = path.bounds() else {
            return;
        };
        let grown = Rect::new(
            bounds.x - width,
            bounds.y - width,
            bounds.w + 2.0 * width,
            bounds.h + 2.0 * width,
        );
        if self.cut_away(grown) {
            return;
        }
        let s = self.scale;
        let Some(skia) = crate::cpu::skia_path(path, s) else {
            return;
        };
        if !fill.is_transparent() {
            self.skia_mask(&skia, None, fill);
        }
        if !stroke.is_transparent() && width > 0.0 {
            let line = crate::cpu::line_stroke(path.style(), width, s);
            self.skia_mask(&skia, Some(&line), stroke);
        }
    }

    fn image(&mut self, rect: Rect, image: &crate::image::Image) {
        let (w, h) = image.size();
        if w == 0 || h == 0 || rect.w <= 0.0 || rect.h <= 0.0 || self.cut_away(rect) {
            return;
        }
        let pixmap = image.pixmap();
        let k = key(|hh| {
            2u8.hash(hh);
            (std::ptr::from_ref(pixmap) as usize).hash(hh);
        });
        let made = self.texture_with(k, (w, h), vk::Format::R8G8B8A8_UNORM, || {
            Some(pixmap.data().to_vec())
        });
        if !made {
            return;
        }
        if let Some(texture) = self.gpu.textures.get_mut(&k)
            && texture.keep.is_none()
        {
            texture.keep = Some(image.clone());
        }
        let s = self.scale;
        let (x, y, rw, rh) = (rect.x * s, rect.y * s, rect.w * s, rect.h * s);
        self.push(
            Tex::Texture(k),
            [
                x,
                y,
                x + rw,
                y + rh,
                1.0,
                1.0,
                1.0,
                1.0,
                x,
                y,
                x + rw,
                y + rh,
                0.0,
                0.0,
                4.0,
                0.0,
                0.0,
                0.0,
                1.0,
                1.0,
            ],
        );
    }

    fn push_clip(&mut self, rect: Rect) {
        let rect = match self.clips.last() {
            Some((outer, _)) => outer.intersect(&rect),
            None => rect,
        };
        let s = self.scale;
        let px = |v: f32| (v * s).round() as i32;
        let bounds = [px(rect.x), px(rect.y), px(rect.right()), px(rect.bottom())];
        self.clips.push((rect, bounds));
    }

    fn pop_clip(&mut self) {
        let _ = self.clips.pop();
    }

    fn push_opacity(&mut self, opacity: f32) {
        let depth = self.depth;
        let size = self.target.size();
        let fits = self.gpu.layers.get(depth).is_some_and(|l| l.size == size);
        if !fits {
            if depth < self.gpu.layers.len() {
                // Only at a frame's start can a layer in use be remade;
                // a frame's layers are all its size, so this is a resize.
                let old = self.gpu.layers.remove(depth);
                self.gpu.destroy_texture(old);
            }
            match self.gpu.texture(
                size,
                FORMAT,
                vk::ImageUsageFlags::COLOR_ATTACHMENT | vk::ImageUsageFlags::SAMPLED,
            ) {
                Ok(layer) => {
                    let at = depth.min(self.gpu.layers.len());
                    self.gpu.layers.insert(at, layer);
                }
                Err(error) => {
                    self.fail(error);
                    return;
                }
            }
        }
        self.cmds.push(Cmd::PushLayer {
            depth,
            area: self.area,
        });
        self.depth += 1;
        self.opacity.push(opacity);
    }

    fn pop_opacity(&mut self) {
        let Some(opacity) = self.opacity.pop() else {
            return;
        };
        self.depth = self.depth.saturating_sub(1);
        let a = self.area;
        let (x0, y0, x1, y1) = (a[0] as f32, a[1] as f32, a[2] as f32, a[3] as f32);
        let instance = u32::try_from(self.instances.len() / FLOATS).unwrap_or(u32::MAX);
        self.instances.extend_from_slice(&[
            x0, y0, x1, y1, opacity, opacity, opacity, opacity, x0, y0, x1, y1, 0.0, 0.0, 5.0, 0.0,
            0.0, 0.0, 0.0, 0.0,
        ]);
        self.cmds.push(Cmd::PopLayer {
            depth: self.depth,
            instance,
            area: a,
        });
    }
}
