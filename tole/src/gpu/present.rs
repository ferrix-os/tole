//! A window's frames drawn on the GPU and handed to the compositor with
//! no copy by the program: into a dmabuf the device exported, given over
//! with `zwp_linux_dmabuf_v1` where the compositor offers it (hyprix does
//! beside an Intel or NVIDIA GPU, and where it draws on the GPU itself),
//! else into the `wl_shm` buffer itself, imported into the device
//! (`VK_EXT_external_memory_host`) -- lavapipe draws straight into the
//! memory the compositor reads.
//!
//! Damage is kept as the CPU's window keeps it: each buffer knows what
//! changed since it was last drawn into (EGL's buffer-age rule), only
//! that is drawn again, and only this frame's damage is reported with
//! `wl_surface.damage_buffer`. Requests go out on the toolkit's surface
//! through its generic requests, so the toolkit is unchanged.

use compositor_toolkit::protocol::core::{WL_BUFFER, wl_buffer, wl_surface};
use compositor_toolkit::protocol::linux_dmabuf::{
    ZWP_LINUX_BUFFER_PARAMS_V1, ZWP_LINUX_DMABUF_V1, zwp_linux_buffer_params_v1 as params,
    zwp_linux_dmabuf_v1 as dmabuf,
};
use compositor_toolkit::{Client, Event, ObjectId, SurfaceId, Value};

use super::Gpu;
use super::frame::After;
use super::target::{Kind, Target};
use crate::ui::Ui;
use crate::value::Color;

/// `wl_shm`'s ARGB8888.
const SHM_ARGB8888: u32 = 0;
/// DRM's `AR24`: ARGB8888, the same bytes.
const DRM_ARGB8888: u32 = 0x3432_5241;
/// How many buffers a window keeps at most.
const SLOTS: usize = 3;
/// How many rectangles a buffer's owed damage is kept apart in.
const RECTS: usize = 8;

/// One buffer the compositor is handed.
#[derive(Debug)]
struct Slot {
    /// Its pixels on the device.
    target: Target,
    /// Its `wl_buffer`.
    buffer: ObjectId,
    /// Whether the toolkit made it (`wl_shm`), rather than the dmabuf
    /// protocol.
    shm: bool,
    /// The compositor holds it.
    busy: bool,
    /// What changed since it was last drawn into; `None` for all of it.
    owed: Option<Vec<(u32, u32, u32, u32)>>,
}

/// A window's GPU frames.
#[derive(Debug)]
pub(crate) struct Presenter {
    /// The device.
    gpu: Gpu,
    /// The buffers.
    slots: Vec<Slot>,
    /// What the last frame showed, and at which scale.
    scene: Option<(crate::damage::Scene, f32)>,
    /// The compositor's `zwp_linux_dmabuf_v1`, when it offers one and the
    /// device can export.
    dmabuf: Option<(ObjectId, u32)>,
    /// The buffer scale last committed.
    sent_scale: u32,
    /// How the frames reach the compositor, said once known.
    how: Option<&'static str>,
    /// Events read while waiting for the compositor's answer about a
    /// dmabuf, which belong to the program.
    deferred: Vec<Event>,
}

/// Write to standard error, where a program's warnings go.
pub(crate) fn warn(line: &str) {
    use std::io::Write as _;
    let _ = writeln!(std::io::stderr(), "{line}");
}

impl Presenter {
    /// Frames on `gpu` for a window on `client`.
    pub(crate) fn new(gpu: Gpu, client: &mut Client) -> Self {
        // `TOLE_GPU_PRESENT=shm` keeps to `wl_shm`, to try that path where
        // the compositor would take a dmabuf.
        let shm_only = std::env::var("TOLE_GPU_PRESENT").is_ok_and(|v| v == "shm");
        let dmabuf = if gpu.can_export_dmabuf() && !shm_only {
            client.bind(&ZWP_LINUX_DMABUF_V1, 3, None).ok()
        } else {
            None
        };
        Presenter {
            gpu,
            slots: Vec::new(),
            scene: None,
            dmabuf,
            sent_scale: 0,
            how: None,
            deferred: Vec::new(),
        }
    }

    /// The device's name and how frames reach the compositor.
    pub(crate) fn describe(&self) -> String {
        format!(
            "gpu: {}{}",
            self.gpu.name(),
            self.how.map(|how| format!(", {how}")).unwrap_or_default()
        )
    }

    /// Events read while waiting, for the window to handle.
    pub(crate) fn take_deferred(&mut self) -> Vec<Event> {
        std::mem::take(&mut self.deferred)
    }

    /// An event for one of the presenter's objects; whether it was one.
    pub(crate) fn event(&mut self, object: ObjectId, opcode: u16) -> bool {
        if let Some(slot) = self.slots.iter_mut().find(|s| s.buffer == object) {
            if opcode == wl_buffer::event::RELEASE {
                slot.busy = false;
            }
            return true;
        }
        self.dmabuf.is_some_and(|(d, _)| d == object)
    }

    /// Let every buffer go.
    pub(crate) fn close(mut self, client: &mut Client) {
        self.drop_slots(client);
    }

    /// Let every buffer go, the device done with them first.
    fn drop_slots(&mut self, client: &mut Client) {
        let mut gpu = self.gpu.inner.borrow_mut();
        for slot in self.slots.drain(..) {
            if slot.shm {
                client.destroy_buffer(slot.buffer);
            } else {
                let _ = client.request(slot.buffer, wl_buffer::request::DESTROY, &[]);
            }
            slot.target.release(&mut gpu);
        }
        self.scene = None;
    }

    /// A new buffer of `size` pixels.
    fn make_slot(&mut self, client: &mut Client, size: (u32, u32)) -> Result<Slot, String> {
        if let Some((manager, version)) = self.dmabuf {
            match self.dmabuf_slot(client, (manager, version), size) {
                Ok(slot) => return Ok(slot),
                Err(why) => {
                    warn(&format!(
                        "tole: the compositor did not take a dmabuf ({why}); frames go through wl_shm"
                    ));
                    self.dmabuf = None;
                }
            }
        }
        let (buffer, _) = client
            .new_buffer(size.0, size.1, SHM_ARGB8888)
            .map_err(|e| format!("a wl_shm buffer: {e}"))?;
        let (ptr, len) = match client.buffer_bytes(buffer) {
            Some(bytes) => (bytes.as_mut_ptr(), bytes.len()),
            None => return Err("a wl_shm buffer with no bytes".to_owned()),
        };
        let mut gpu = self.gpu.inner.borrow_mut();
        let made = if gpu.host_align <= 4096 {
            // SAFETY: the toolkit keeps the buffer's bytes mapped,
            // page-aligned and whole pages long, until `destroy_buffer`,
            // which comes only after the target is let go (`drop_slots`);
            // `Target::host` refuses an alignment the mapping lacks.
            unsafe { Target::host(&mut gpu, ptr, len, size) }
        } else {
            Err("the device imports host memory only on larger pages".to_owned())
        };
        let target = match made {
            Ok(target) => target,
            Err(why) => {
                warn(&format!(
                    "tole: the device cannot draw into wl_shm memory ({why}); each frame is read back"
                ));
                match Target::own(&mut gpu, size) {
                    Ok(target) => target,
                    Err(error) => {
                        drop(gpu);
                        client.destroy_buffer(buffer);
                        return Err(error);
                    }
                }
            }
        };
        Ok(Slot {
            target,
            buffer,
            shm: true,
            busy: false,
            owed: None,
        })
    }

    /// A buffer the device exports as a dmabuf, given to the compositor.
    fn dmabuf_slot(
        &mut self,
        client: &mut Client,
        (manager, version): (ObjectId, u32),
        size: (u32, u32),
    ) -> Result<Slot, String> {
        let target = Target::dmabuf(&mut self.gpu.inner.borrow_mut(), size)?;
        let answer = Self::offer(
            client,
            (manager, version),
            &target,
            size,
            &mut self.deferred,
        );
        match answer {
            Ok(buffer) => Ok(Slot {
                target,
                buffer,
                shm: false,
                busy: false,
                owed: None,
            }),
            Err(why) => {
                target.release(&mut self.gpu.inner.borrow_mut());
                Err(why)
            }
        }
    }

    /// Hand `target`'s dmabuf to the compositor and wait for its answer:
    /// the `wl_buffer`, or why not.
    fn offer(
        client: &mut Client,
        (manager, version): (ObjectId, u32),
        target: &Target,
        (w, h): (u32, u32),
        deferred: &mut Vec<Event>,
    ) -> Result<ObjectId, String> {
        let Some((fd, stride, offset)) = target.dmabuf_plane() else {
            return Err("not a dmabuf".to_owned());
        };
        let made = client.new_object(&ZWP_LINUX_BUFFER_PARAMS_V1, version);
        let said = |e: compositor_toolkit::Error| e.to_string();
        client
            .request(
                manager,
                dmabuf::request::CREATE_PARAMS,
                &[Value::NewId(made)],
            )
            .map_err(said)?;
        client.adopt_new_ids(made, params::event::CREATED, &WL_BUFFER, 1);
        client
            .request(
                made,
                params::request::ADD,
                &[
                    Value::Fd(fd),
                    Value::Uint(0),
                    Value::Uint(offset),
                    Value::Uint(stride),
                    Value::Uint(0),
                    Value::Uint(0),
                ],
            )
            .map_err(said)?;
        let (Ok(wi), Ok(hi)) = (i32::try_from(w), i32::try_from(h)) else {
            return Err("too large".to_owned());
        };
        client
            .request(
                made,
                params::request::CREATE,
                &[
                    Value::Int(wi),
                    Value::Int(hi),
                    Value::Uint(DRM_ARGB8888),
                    Value::Uint(0),
                ],
            )
            .map_err(said)?;
        let mut answer = None;
        for _ in 0..4 {
            for event in client.roundtrip().map_err(said)? {
                match event {
                    Event::Object {
                        object,
                        opcode,
                        ref args,
                        ..
                    } if object == made => {
                        answer = Some(if opcode == params::event::CREATED {
                            args.first()
                                .and_then(Value::as_object)
                                .ok_or_else(|| "created, but no buffer".to_owned())
                        } else {
                            Err("failed".to_owned())
                        });
                    }
                    other => deferred.push(other),
                }
            }
            if answer.is_some() {
                break;
            }
        }
        let _ = client.request(made, params::request::DESTROY, &[]);
        answer.unwrap_or_else(|| Err("no answer".to_owned()))
    }

    /// Draw what changed in `ui` into a free buffer of `size` logical
    /// pixels at `scale` and commit it to `surface`, a frame callback
    /// asked for with it. `Ok(None)` when every buffer is held: draw
    /// again once one is released. `Ok(Some(false))` when nothing changed.
    ///
    /// # Errors
    ///
    /// The device or the connection failed: the window draws on the CPU
    /// from then on.
    pub(crate) fn draw(
        &mut self,
        ui: &mut Ui,
        client: &mut Client,
        surface: SurfaceId,
        (size, scale): ((u32, u32), u32),
        background: Color,
    ) -> Result<Option<bool>, String> {
        let s = scale.max(1);
        let pixel = (size.0.saturating_mul(s), size.1.saturating_mul(s));
        if self
            .slots
            .first()
            .is_some_and(|slot| slot.target.size() != pixel)
        {
            self.drop_slots(client);
        }
        let at = match self.slots.iter().position(|slot| !slot.busy) {
            Some(at) => at,
            None if self.slots.len() < SLOTS => {
                let slot = self.make_slot(client, pixel)?;
                self.slots.push(slot);
                self.slots.len() - 1
            }
            None => return Ok(None),
        };
        let (before, before_scale) = match &self.scene {
            Some((scene, scale)) => (Some(scene), *scale),
            None => (None, s as f32),
        };
        let (scene, damage) = ui.damage_since(before, before_scale, pixel);
        if damage.is_empty() {
            self.scene = Some((scene, s as f32));
            return Ok(Some(false));
        }
        let Some(slot) = self.slots.get_mut(at) else {
            return Err("no buffer".to_owned());
        };
        let rects = match &slot.owed {
            Some(owed) => merge(owed.iter().chain(&damage).copied().collect()),
            None => Vec::new(),
        };
        let after = match slot.target.kind {
            Kind::Host | Kind::Dmabuf { .. } => After::Present,
            Kind::HostCopy { .. } | Kind::Own => After::Copy(rects.clone()),
        };
        {
            let mut gpu = self.gpu.inner.borrow_mut();
            super::draw(
                &mut gpu,
                &mut ui.text.fonts,
                &mut slot.target,
                (&scene.ops, s as f32),
                &rects,
                background,
                &after,
            )?;
            if matches!(slot.target.kind, Kind::Own) {
                let bytes = client
                    .buffer_bytes(slot.buffer)
                    .ok_or_else(|| "a wl_shm buffer with no bytes".to_owned())?;
                super::copy_read_back(&gpu, pixel, &rects, bytes)?;
            }
        }
        if self.how.is_none() {
            self.how = Some(slot.target.describe());
        }
        let buffer = slot.buffer;
        slot.busy = true;
        slot.owed = Some(Vec::new());
        for (index, other) in self.slots.iter_mut().enumerate() {
            if index != at
                && let Some(owed) = &mut other.owed
            {
                owed.extend_from_slice(&damage);
                if owed.len() > RECTS * 4 {
                    *owed = merge(std::mem::take(owed));
                }
            }
        }
        self.commit(client, surface, buffer, s, &damage)?;
        self.scene = Some((scene, s as f32));
        Ok(Some(true))
    }

    /// Attach `buffer` to `surface`, damage `damage` of it, ask for a
    /// frame callback and commit.
    fn commit(
        &mut self,
        client: &mut Client,
        surface: SurfaceId,
        buffer: ObjectId,
        scale: u32,
        damage: &[(u32, u32, u32, u32)],
    ) -> Result<(), String> {
        let said = |e: compositor_toolkit::Error| e.to_string();
        let object = client
            .surface_object(surface)
            .ok_or_else(|| "no surface".to_owned())?;
        let version = client.bound_version("wl_compositor").unwrap_or(1);
        // Asked before the commit, so it rides on it.
        client.request_frame(surface);
        let int = |v: u32| Value::Int(i32::try_from(v).unwrap_or(i32::MAX));
        if scale != self.sent_scale && version >= 3 {
            client
                .request(object, wl_surface::request::SET_BUFFER_SCALE, &[int(scale)])
                .map_err(said)?;
            self.sent_scale = scale;
        }
        client
            .request(
                object,
                wl_surface::request::ATTACH,
                &[Value::Object(buffer), Value::Int(0), Value::Int(0)],
            )
            .map_err(said)?;
        for &(x, y, w, h) in damage {
            if version >= 4 {
                client
                    .request(
                        object,
                        wl_surface::request::DAMAGE_BUFFER,
                        &[int(x), int(y), int(w), int(h)],
                    )
                    .map_err(said)?;
            } else {
                let (left, top) = (x / scale, y / scale);
                let (right, bottom) = (
                    x.saturating_add(w).div_ceil(scale),
                    y.saturating_add(h).div_ceil(scale),
                );
                client
                    .request(
                        object,
                        wl_surface::request::DAMAGE,
                        &[int(left), int(top), int(right - left), int(bottom - top)],
                    )
                    .map_err(said)?;
            }
        }
        client
            .request(object, wl_surface::request::COMMIT, &[])
            .map_err(said)?;
        client.flush().map_err(said)
    }
}

/// Pixel rectangles merged as the damage is: touching ones joined, and
/// all of them into one past [`RECTS`].
fn merge(rects: Vec<(u32, u32, u32, u32)>) -> Vec<(u32, u32, u32, u32)> {
    let logical = rects
        .into_iter()
        .map(|(x, y, w, h)| crate::render::Rect::new(x as f32, y as f32, w as f32, h as f32))
        .collect();
    crate::damage::merge(logical, RECTS)
        .into_iter()
        .map(|r| (r.x as u32, r.y as u32, r.w as u32, r.h as u32))
        .collect()
}
