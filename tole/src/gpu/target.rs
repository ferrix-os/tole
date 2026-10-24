//! What a frame is drawn into: an image of the device's own, read back
//! for a headless frame, or the buffer the compositor is handed -- a
//! dmabuf the device exported, or a `wl_shm` buffer the device draws into
//! where it lies.

use std::os::fd::{FromRawFd as _, OwnedFd};

use ash::vk;

use super::device::{self, FORMAT, Gpu, Texture};

/// How a target's pixels reach whoever reads them.
#[derive(Debug)]
pub(crate) enum Kind {
    /// The device's own image; read back by copying it out.
    Own,
    /// A linear image bound to memory the program mapped (a `wl_shm`
    /// buffer): drawn where it lies, no copy at all.
    Host,
    /// The device's own image, its drawn rectangles copied by the device
    /// into a buffer bound to memory the program mapped.
    HostCopy {
        /// The buffer over the program's memory.
        buffer: vk::Buffer,
        /// That memory, imported.
        memory: vk::DeviceMemory,
    },
    /// A linear image the device exported as a dmabuf.
    Dmabuf {
        /// The descriptor.
        fd: OwnedFd,
        /// Bytes from one row to the next.
        stride: u32,
        /// Where the first row is.
        offset: u32,
    },
}

/// A frame's pixels on the device.
#[derive(Debug)]
pub struct Target {
    /// The image.
    pub(crate) texture: Texture,
    /// How its pixels are read.
    pub(crate) kind: Kind,
}

/// The layout a target is drawn in: a linear image whose memory others
/// read stays `GENERAL`, so its rows are where its layout says.
pub(crate) fn drawing_layout(kind: &Kind) -> vk::ImageLayout {
    match kind {
        Kind::Host | Kind::Dmabuf { .. } => vk::ImageLayout::GENERAL,
        Kind::Own | Kind::HostCopy { .. } => vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL,
    }
}

impl Target {
    /// Its size in pixels.
    #[must_use]
    pub fn size(&self) -> (u32, u32) {
        self.texture.size
    }

    /// How its pixels reach the compositor, in words.
    pub(crate) fn describe(&self) -> &'static str {
        match self.kind {
            Kind::Own => "read back",
            Kind::Host => "drawn into the wl_shm buffer itself",
            Kind::HostCopy { .. } => "copied by the device into the wl_shm buffer",
            Kind::Dmabuf { .. } => "a dmabuf through zwp_linux_dmabuf_v1",
        }
    }

    /// An image of the device's own, `size` pixels.
    pub(crate) fn own(gpu: &mut Gpu, size: (u32, u32)) -> Result<Target, String> {
        let texture = gpu.texture(
            size,
            FORMAT,
            vk::ImageUsageFlags::COLOR_ATTACHMENT
                | vk::ImageUsageFlags::TRANSFER_SRC
                | vk::ImageUsageFlags::TRANSFER_DST,
        )?;
        Ok(Target {
            texture,
            kind: Kind::Own,
        })
    }

    /// A target over `len` bytes the program mapped at `ptr`: `size`
    /// pixels of four bytes, rows `size.0 * 4` bytes apart. Drawn where
    /// they lie when the device can bind a linear image there, else into
    /// an image of its own whose drawn parts the device copies there.
    ///
    /// # Safety
    ///
    /// `ptr` must stay mapped, readable and writable, for `len` bytes
    /// rounded up to the device's import alignment, until the target is
    /// let go ([`Target::release`]), and be aligned to that alignment.
    pub(crate) unsafe fn host(
        gpu: &mut Gpu,
        ptr: *mut u8,
        len: usize,
        size: (u32, u32),
    ) -> Result<Target, String> {
        let Some(host) = gpu.host_import.clone() else {
            return Err("no VK_EXT_external_memory_host".to_owned());
        };
        let align = gpu.host_align;
        if !(ptr as u64).is_multiple_of(align) {
            return Err(format!("the buffer is not aligned to {align} bytes"));
        }
        let len = u64::try_from(len).unwrap_or(u64::MAX);
        let import_len = len.div_ceil(align).saturating_mul(align);
        let ty = vk::ExternalMemoryHandleTypeFlags::HOST_ALLOCATION_EXT;
        let mut props = vk::MemoryHostPointerPropertiesEXT::default();
        // SAFETY: `ptr` is a host allocation the caller keeps mapped, and
        // the device handle is the one the extension was loaded for.
        let result = unsafe {
            (host.fp().get_memory_host_pointer_properties_ext)(
                gpu.device.handle(),
                ty,
                ptr.cast(),
                &mut props,
            )
        };
        result
            .result()
            .map_err(|e| device::said("vkGetMemoryHostPointerPropertiesEXT", e))?;
        // SAFETY: as this function's own contract.
        match unsafe { linear_on_host(gpu, ptr, import_len, size, props.memory_type_bits) } {
            Ok(texture) => {
                return Ok(Target {
                    texture,
                    kind: Kind::Host,
                });
            }
            Err(why) => {
                if std::env::var_os("TOLE_GPU_TRACE").is_some() {
                    super::present_warn(&format!("tole: no linear image over wl_shm: {why}"));
                }
            }
        }
        let info = vk::BufferCreateInfo::default()
            .size(len)
            .usage(vk::BufferUsageFlags::TRANSFER_DST);
        let mut external = vk::ExternalMemoryBufferCreateInfo::default().handle_types(ty);
        let info = info.push_next(&mut external);
        // SAFETY: a plain transfer buffer for external host memory.
        let buffer = unsafe { gpu.device.create_buffer(&info, None) }
            .map_err(|e| device::said("vkCreateBuffer", e))?;
        // SAFETY: `buffer` is this device's.
        let needs = unsafe { gpu.device.get_buffer_memory_requirements(buffer) };
        let bits = needs.memory_type_bits & props.memory_type_bits;
        let memory = import(gpu, ptr, import_len, bits, None);
        let memory = match memory {
            Ok(memory) => memory,
            Err(error) => {
                // SAFETY: unused.
                unsafe { gpu.device.destroy_buffer(buffer, None) };
                return Err(error);
            }
        };
        // SAFETY: imported memory of a type the buffer takes, at least as
        // large as it needs, bound once.
        if let Err(e) = unsafe { gpu.device.bind_buffer_memory(buffer, memory, 0) } {
            // SAFETY: unused.
            unsafe { gpu.device.destroy_buffer(buffer, None) };
            // SAFETY: nothing is bound to it.
            unsafe { gpu.device.free_memory(memory, None) };
            return Err(device::said("vkBindBufferMemory", e));
        }
        let fail = |gpu: &mut Gpu, error: String| {
            // SAFETY: unused.
            unsafe { gpu.device.destroy_buffer(buffer, None) };
            // SAFETY: the buffer bound to it is gone.
            unsafe { gpu.device.free_memory(memory, None) };
            error
        };
        let texture = match gpu.texture(
            size,
            FORMAT,
            vk::ImageUsageFlags::COLOR_ATTACHMENT | vk::ImageUsageFlags::TRANSFER_SRC,
        ) {
            Ok(texture) => texture,
            Err(error) => return Err(fail(gpu, error)),
        };
        Ok(Target {
            texture,
            kind: Kind::HostCopy { buffer, memory },
        })
    }

    /// A linear image the device exports as a dmabuf, `size` pixels.
    pub(crate) fn dmabuf(gpu: &mut Gpu, size: (u32, u32)) -> Result<Target, String> {
        let Some(fd_ext) = gpu.dmabuf.clone() else {
            return Err("no dmabuf export".to_owned());
        };
        let ty = vk::ExternalMemoryHandleTypeFlags::DMA_BUF_EXT;
        let modifiers = [0u64];
        let mut list =
            vk::ImageDrmFormatModifierListCreateInfoEXT::default().drm_format_modifiers(&modifiers);
        let mut external = vk::ExternalMemoryImageCreateInfo::default().handle_types(ty);
        let info = vk::ImageCreateInfo::default()
            .image_type(vk::ImageType::TYPE_2D)
            .format(FORMAT)
            .extent(vk::Extent3D {
                width: size.0.max(1),
                height: size.1.max(1),
                depth: 1,
            })
            .mip_levels(1)
            .array_layers(1)
            .samples(vk::SampleCountFlags::TYPE_1)
            .tiling(vk::ImageTiling::DRM_FORMAT_MODIFIER_EXT)
            .usage(vk::ImageUsageFlags::COLOR_ATTACHMENT | vk::ImageUsageFlags::TRANSFER_SRC)
            .initial_layout(vk::ImageLayout::UNDEFINED)
            .push_next(&mut list)
            .push_next(&mut external);
        // SAFETY: a linear 2D image for export, as the extensions enabled
        // allow.
        let image = unsafe { gpu.device.create_image(&info, None) }
            .map_err(|e| device::said("vkCreateImage (dmabuf)", e))?;
        // SAFETY: `image` is this device's.
        let needs = unsafe { gpu.device.get_image_memory_requirements(image) };
        let Some(kind) = gpu.memory_type(needs.memory_type_bits, vk::MemoryPropertyFlags::empty())
        else {
            // SAFETY: unused.
            unsafe { gpu.device.destroy_image(image, None) };
            return Err("no memory type for a dmabuf".to_owned());
        };
        let mut export = vk::ExportMemoryAllocateInfo::default().handle_types(ty);
        let mut dedicated = vk::MemoryDedicatedAllocateInfo::default().image(image);
        let alloc = vk::MemoryAllocateInfo::default()
            .allocation_size(needs.size)
            .memory_type_index(kind)
            .push_next(&mut export)
            .push_next(&mut dedicated);
        // SAFETY: the image's own requirements, dedicated to it.
        let memory = match unsafe { gpu.device.allocate_memory(&alloc, None) } {
            Ok(memory) => memory,
            Err(e) => {
                // SAFETY: unused.
                unsafe { gpu.device.destroy_image(image, None) };
                return Err(device::said("vkAllocateMemory (dmabuf)", e));
            }
        };
        let mut texture = Texture {
            image,
            memory,
            view: vk::ImageView::null(),
            set: vk::DescriptorSet::null(),
            size,
            layout: vk::ImageLayout::UNDEFINED,
            used: gpu.frame,
            keep: None,
        };
        // SAFETY: dedicated memory for this image, bound once.
        if let Err(e) = unsafe { gpu.device.bind_image_memory(image, memory, 0) } {
            gpu.destroy_texture(texture);
            return Err(device::said("vkBindImageMemory (dmabuf)", e));
        }
        let plane = vk::ImageSubresource {
            aspect_mask: vk::ImageAspectFlags::MEMORY_PLANE_0_EXT,
            mip_level: 0,
            array_layer: 0,
        };
        // SAFETY: a DRM-modifier image's plane 0, which it has.
        let layout = unsafe { gpu.device.get_image_subresource_layout(image, plane) };
        let get = vk::MemoryGetFdInfoKHR::default()
            .memory(memory)
            .handle_type(ty);
        // SAFETY: the memory was allocated for export as a dmabuf.
        let raw = match unsafe { fd_ext.get_memory_fd(&get) } {
            Ok(raw) => raw,
            Err(e) => {
                gpu.destroy_texture(texture);
                return Err(device::said("vkGetMemoryFdKHR", e));
            }
        };
        // SAFETY: the descriptor is new and ours alone (the call hands a
        // new reference to the caller).
        let fd = unsafe { OwnedFd::from_raw_fd(raw) };
        match gpu.view_and_set(image, FORMAT, false) {
            Ok((view, _)) => texture.view = view,
            Err(error) => {
                gpu.destroy_texture(texture);
                return Err(error);
            }
        }
        let (Ok(stride), Ok(offset)) = (
            u32::try_from(layout.row_pitch),
            u32::try_from(layout.offset),
        ) else {
            gpu.destroy_texture(texture);
            return Err("a dmabuf's layout out of range".to_owned());
        };
        Ok(Target {
            texture,
            kind: Kind::Dmabuf { fd, stride, offset },
        })
    }

    /// The dmabuf's descriptor, stride and offset, for a `Dmabuf` target.
    pub(crate) fn dmabuf_plane(&self) -> Option<(i32, u32, u32)> {
        use std::os::fd::AsRawFd as _;
        match &self.kind {
            Kind::Dmabuf { fd, stride, offset } => Some((fd.as_raw_fd(), *stride, *offset)),
            _ => None,
        }
    }

    /// Let it go. The device must be done with it.
    pub(crate) fn release(self, gpu: &mut Gpu) {
        if let Kind::HostCopy { buffer, memory } = self.kind {
            // SAFETY: nothing pending uses it (one frame in flight).
            unsafe { gpu.device.destroy_buffer(buffer, None) };
            // SAFETY: the buffer bound to it is gone.
            unsafe { gpu.device.free_memory(memory, None) };
        }
        gpu.destroy_texture(self.texture);
    }
}

/// `len` bytes at `ptr` imported as memory of a type among `bits`,
/// dedicated to `image` when given.
fn import(
    gpu: &Gpu,
    ptr: *mut u8,
    len: u64,
    bits: u32,
    image: Option<vk::Image>,
) -> Result<vk::DeviceMemory, String> {
    let Some(kind) = gpu.memory_type(bits, vk::MemoryPropertyFlags::empty()) else {
        return Err("no memory type takes the buffer".to_owned());
    };
    let mut from = vk::ImportMemoryHostPointerInfoEXT::default()
        .handle_type(vk::ExternalMemoryHandleTypeFlags::HOST_ALLOCATION_EXT)
        .host_pointer(ptr.cast());
    let mut dedicated = vk::MemoryDedicatedAllocateInfo::default();
    let mut alloc = vk::MemoryAllocateInfo::default()
        .allocation_size(len)
        .memory_type_index(kind)
        .push_next(&mut from);
    if let Some(image) = image {
        dedicated = dedicated.image(image);
        let _ = &mut dedicated;
        alloc = alloc.push_next(&mut dedicated);
    }
    // SAFETY: the caller keeps `ptr` mapped for `len` bytes, aligned as
    // the device asks, and the type is one the pointer may be imported as.
    unsafe { gpu.device.allocate_memory(&alloc, None) }
        .map_err(|e| device::said("vkAllocateMemory (host)", e))
}

/// A linear image bound to the program's memory at `ptr`, when the device
/// lays its rows out as the `wl_shm` buffer has them.
///
/// # Safety
///
/// As [`Target::host`].
unsafe fn linear_on_host(
    gpu: &mut Gpu,
    ptr: *mut u8,
    len: u64,
    size: (u32, u32),
    host_bits: u32,
) -> Result<Texture, String> {
    // Where the device pads a linear image's rows (llvmpipe to 64 bytes)
    // and the window's width does not fill them, the rows do not line up
    // and the device copies instead. An explicit linear DRM layout with
    // the buffer's own row pitch was tried: llvmpipe 26 crashes drawing
    // into one over imported host memory.
    let mut external = vk::ExternalMemoryImageCreateInfo::default()
        .handle_types(vk::ExternalMemoryHandleTypeFlags::HOST_ALLOCATION_EXT);
    let info = vk::ImageCreateInfo::default()
        .image_type(vk::ImageType::TYPE_2D)
        .format(FORMAT)
        .extent(vk::Extent3D {
            width: size.0.max(1),
            height: size.1.max(1),
            depth: 1,
        })
        .mip_levels(1)
        .array_layers(1)
        .samples(vk::SampleCountFlags::TYPE_1)
        .tiling(vk::ImageTiling::LINEAR)
        .usage(vk::ImageUsageFlags::COLOR_ATTACHMENT | vk::ImageUsageFlags::TRANSFER_SRC)
        .initial_layout(vk::ImageLayout::UNDEFINED)
        .push_next(&mut external);
    // SAFETY: a linear 2D image over external host memory.
    let image = unsafe { gpu.device.create_image(&info, None) }
        .map_err(|e| device::said("vkCreateImage (linear)", e))?;
    let sub = vk::ImageSubresource {
        aspect_mask: vk::ImageAspectFlags::COLOR,
        mip_level: 0,
        array_layer: 0,
    };
    // SAFETY: a linear image's colour subresource.
    let layout = unsafe { gpu.device.get_image_subresource_layout(image, sub) };
    // SAFETY: `image` is this device's.
    let needs = unsafe { gpu.device.get_image_memory_requirements(image) };
    let rows = u64::from(size.0) * 4;
    if layout.offset != 0 || layout.row_pitch != rows || needs.size > len {
        // SAFETY: unused.
        unsafe { gpu.device.destroy_image(image, None) };
        return Err(format!(
            "the device lays a linear image out with rows {} bytes apart, not {rows}",
            layout.row_pitch
        ));
    }
    let memory = match import(
        gpu,
        ptr,
        len,
        needs.memory_type_bits & host_bits,
        Some(image),
    ) {
        Ok(memory) => memory,
        Err(error) => {
            // SAFETY: unused.
            unsafe { gpu.device.destroy_image(image, None) };
            return Err(error);
        }
    };
    let mut texture = Texture {
        image,
        memory,
        view: vk::ImageView::null(),
        set: vk::DescriptorSet::null(),
        size,
        layout: vk::ImageLayout::UNDEFINED,
        used: gpu.frame,
        keep: None,
    };
    // SAFETY: imported memory the image fits in, bound once.
    if let Err(e) = unsafe { gpu.device.bind_image_memory(image, memory, 0) } {
        gpu.destroy_texture(texture);
        return Err(device::said("vkBindImageMemory (linear)", e));
    }
    match gpu.view_and_set(image, FORMAT, false) {
        Ok((view, _)) => {
            texture.view = view;
            Ok(texture)
        }
        Err(error) => {
            gpu.destroy_texture(texture);
            Err(error)
        }
    }
}
