//! The Vulkan device: made once, with the one pipeline every drawing goes
//! through, and the images and buffers a frame needs.
//!
//! Every call into Vulkan is `unsafe`: its rules are the specification's
//! valid-usage statements, and each block says which ones it keeps. The
//! handles are made and destroyed only here and in [`super::target`], and
//! each lives inside the [`Gpu`] that made it; [`Gpu`]'s `Drop` waits for
//! the device before it destroys anything.

use std::collections::HashMap;
use std::ffi::CStr;

use ash::vk;

use super::atlas::Atlas;

/// The format every frame, layer and picture is drawn in: the byte order
/// `wl_shm`'s ARGB8888 and DRM's `AR24` have in memory.
pub(crate) const FORMAT: vk::Format = vk::Format::B8G8R8A8_UNORM;

/// The atlas's side, in texels.
pub(crate) const ATLAS_SIZE: u32 = 4096;

/// The SPIR-V of the two shaders, compiled from the GLSL beside them.
const VERT: &[u8] = include_bytes!("shaders/quad.vert.spv");
/// The fragment shader's.
const FRAG: &[u8] = include_bytes!("shaders/quad.frag.spv");

/// An image on the device, its view, and the descriptor set that samples
/// it.
#[derive(Debug)]
pub(crate) struct Texture {
    /// The image.
    pub(crate) image: vk::Image,
    /// Its memory.
    pub(crate) memory: vk::DeviceMemory,
    /// Its view.
    pub(crate) view: vk::ImageView,
    /// The set that binds it to the shader; null when it is only drawn
    /// into.
    pub(crate) set: vk::DescriptorSet,
    /// Its width and height.
    pub(crate) size: (u32, u32),
    /// The layout it was last left in.
    pub(crate) layout: vk::ImageLayout,
    /// The frame it was last drawn with: pictures not drawn for a while
    /// are let go.
    pub(crate) used: u64,
    /// The picture it holds, kept alive so its address names no other
    /// while the texture is cached; `None` for what is not a picture.
    pub(crate) keep: Option<crate::image::Image>,
}

/// A buffer the processor writes or reads, mapped for as long as it lives.
#[derive(Debug)]
pub(crate) struct HostBuffer {
    /// The buffer.
    pub(crate) buffer: vk::Buffer,
    /// Its memory.
    pub(crate) memory: vk::DeviceMemory,
    /// Where it is mapped.
    pub(crate) ptr: *mut u8,
    /// Its size in bytes.
    pub(crate) size: u64,
}

/// The device.
pub(crate) struct Gpu {
    /// The loader, kept loaded.
    _entry: ash::Entry,
    /// The instance.
    pub(crate) instance: ash::Instance,
    /// The logical device.
    pub(crate) device: ash::Device,
    /// Its graphics queue.
    pub(crate) queue: vk::Queue,
    /// The device's name.
    pub(crate) name: String,
    /// Whether it draws on the processor.
    pub(crate) software: bool,
    /// Its memory types.
    memory: vk::PhysicalDeviceMemoryProperties,
    /// `VK_EXT_external_memory_host`, when it has it.
    pub(crate) host_import: Option<ash::ext::external_memory_host::Device>,
    /// The alignment an imported pointer and size need.
    pub(crate) host_align: u64,
    /// `VK_KHR_external_memory_fd`, when it also has dmabufs and DRM
    /// format modifiers.
    pub(crate) dmabuf: Option<ash::khr::external_memory_fd::Device>,
    /// The command pool.
    pool: vk::CommandPool,
    /// The one command buffer: one frame is in flight at a time.
    pub(crate) cmd: vk::CommandBuffer,
    /// Signalled when it has run.
    fence: vk::Fence,
    /// The one set layout: a sampled image.
    set_layout: vk::DescriptorSetLayout,
    /// The pipeline's layout.
    pub(crate) pipeline_layout: vk::PipelineLayout,
    /// The pipeline.
    pub(crate) pipeline: vk::Pipeline,
    /// Bilinear, clamped.
    sampler: vk::Sampler,
    /// Where sets come from.
    descriptors: vk::DescriptorPool,
    /// Glyphs and masks.
    pub(crate) atlas: Atlas,
    /// The atlas's texture.
    pub(crate) atlas_texture: Texture,
    /// Uploads on their way to the device.
    pub(crate) staging: Option<HostBuffer>,
    /// The instances of a frame.
    pub(crate) instances: Option<HostBuffer>,
    /// A headless frame read back.
    pub(crate) readback: Option<HostBuffer>,
    /// Pictures and large masks, by their key.
    pub(crate) textures: HashMap<u64, Texture>,
    /// Opacity groups' layers, one each depth, the frame's size.
    pub(crate) layers: Vec<Texture>,
    /// Frames drawn.
    pub(crate) frame: u64,
}

impl std::fmt::Debug for Gpu {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Gpu")
            .field("name", &self.name)
            .finish_non_exhaustive()
    }
}

/// A Vulkan result as words.
pub(crate) fn said(what: &str, result: vk::Result) -> String {
    format!("{what}: {result}")
}

/// Whether `name` is among `extensions`.
fn has(extensions: &[vk::ExtensionProperties], name: &CStr) -> bool {
    extensions
        .iter()
        .any(|e| e.extension_name_as_c_str().is_ok_and(|n| n == name))
}

/// How much a device of `kind` is wanted: the lower the better.
fn rank(kind: vk::PhysicalDeviceType) -> u32 {
    match kind {
        vk::PhysicalDeviceType::DISCRETE_GPU => 0,
        vk::PhysicalDeviceType::INTEGRATED_GPU => 1,
        vk::PhysicalDeviceType::VIRTUAL_GPU => 2,
        vk::PhysicalDeviceType::CPU => 4,
        _ => 3,
    }
}

impl Gpu {
    /// Load Vulkan and make the device ([`super::Gpu::new`]).
    pub(crate) fn new(hardware_only: bool) -> Result<Gpu, String> {
        Self::filtered(hardware_only, std::env::var("TOLE_GPU_DEVICE").ok())
    }

    /// [`Gpu::new`] in a static program, which cannot load Vulkan.
    #[cfg(target_env = "musl")]
    pub(crate) fn filtered(_hardware_only: bool, _wanted: Option<String>) -> Result<Gpu, String> {
        Err("no Vulkan library: a static program cannot load one".to_owned())
    }

    /// [`Gpu::new`] on the device whose name holds `wanted`, when given.
    #[cfg(not(target_env = "musl"))]
    pub(crate) fn filtered(hardware_only: bool, wanted: Option<String>) -> Result<Gpu, String> {
        // SAFETY: loading the system's Vulkan loader runs its initialisers;
        // it is the library every Vulkan program loads, and nothing of it
        // is used after the entry is dropped, which is last.
        let entry =
            unsafe { ash::Entry::load() }.map_err(|error| format!("no Vulkan library: {error}"))?;
        let app = vk::ApplicationInfo::default()
            .application_name(c"tole")
            .engine_name(c"tole")
            .api_version(vk::API_VERSION_1_3);
        let info = vk::InstanceCreateInfo::default().application_info(&app);
        // SAFETY: `info` and what it points to live across the call.
        // A loader with no driver behind it refuses the instance with
        // ERROR_INCOMPATIBLE_DRIVER: a machine without Vulkan as much as
        // one without the library, and said as such for the fallback.
        let instance = unsafe { entry.create_instance(&info, None) }.map_err(|e| {
            if e == vk::Result::ERROR_INCOMPATIBLE_DRIVER {
                format!("no Vulkan driver: {}", said("vkCreateInstance", e))
            } else {
                said("vkCreateInstance", e)
            }
        })?;
        match Self::with_instance(entry, instance.clone(), hardware_only, wanted) {
            Ok(gpu) => Ok(gpu),
            Err(error) => {
                // SAFETY: nothing was made from the instance that still
                // lives; `with_instance` destroyed what it made.
                unsafe { instance.destroy_instance(None) };
                Err(error)
            }
        }
    }

    /// The rest of [`Gpu::new`], once there is an instance.
    fn with_instance(
        entry: ash::Entry,
        instance: ash::Instance,
        hardware_only: bool,
        wanted: Option<String>,
    ) -> Result<Gpu, String> {
        // SAFETY: the instance is valid.
        let devices = unsafe { instance.enumerate_physical_devices() }
            .map_err(|e| said("vkEnumeratePhysicalDevices", e))?;
        let mut found: Vec<(u32, vk::PhysicalDevice, String, vk::PhysicalDeviceType)> = Vec::new();
        for physical in devices {
            // SAFETY: `physical` came from this instance.
            let props = unsafe { instance.get_physical_device_properties(physical) };
            let name = props
                .device_name_as_c_str()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            if props.api_version < vk::API_VERSION_1_3 {
                continue;
            }
            if hardware_only && props.device_type == vk::PhysicalDeviceType::CPU {
                continue;
            }
            if let Some(wanted) = &wanted
                && !name.contains(wanted.as_str())
            {
                continue;
            }
            found.push((rank(props.device_type), physical, name, props.device_type));
        }
        found.sort_by_key(|(rank, ..)| *rank);
        let Some((_, physical, name, kind)) = found.into_iter().next() else {
            let kind = if hardware_only {
                "no Vulkan 1.3 GPU (only a software one, or none)"
            } else {
                "no Vulkan 1.3 device"
            };
            return Err(match &wanted {
                Some(wanted) => format!("{kind} named like `{wanted}`"),
                None => kind.to_owned(),
            });
        };
        // SAFETY: `physical` came from this instance.
        let families = unsafe { instance.get_physical_device_queue_family_properties(physical) };
        let family = families
            .iter()
            .position(|f| f.queue_flags.contains(vk::QueueFlags::GRAPHICS))
            .and_then(|f| u32::try_from(f).ok())
            .ok_or_else(|| format!("{name}: no graphics queue"))?;
        // SAFETY: `physical` came from this instance.
        let extensions = unsafe { instance.enumerate_device_extension_properties(physical) }
            .map_err(|e| said("vkEnumerateDeviceExtensionProperties", e))?;
        let host = has(&extensions, ash::ext::external_memory_host::NAME);
        let dmabuf = has(&extensions, ash::khr::external_memory_fd::NAME)
            && has(&extensions, ash::ext::external_memory_dma_buf::NAME)
            && has(&extensions, ash::ext::image_drm_format_modifier::NAME);
        let mut enabled: Vec<*const std::ffi::c_char> = Vec::new();
        if host {
            enabled.push(ash::ext::external_memory_host::NAME.as_ptr());
        }
        if dmabuf {
            enabled.push(ash::khr::external_memory_fd::NAME.as_ptr());
            enabled.push(ash::ext::external_memory_dma_buf::NAME.as_ptr());
            enabled.push(ash::ext::image_drm_format_modifier::NAME.as_ptr());
        }
        let priorities = [1.0];
        let queues = [vk::DeviceQueueCreateInfo::default()
            .queue_family_index(family)
            .queue_priorities(&priorities)];
        let mut v13 = vk::PhysicalDeviceVulkan13Features::default()
            .dynamic_rendering(true)
            .synchronization2(true);
        let info = vk::DeviceCreateInfo::default()
            .queue_create_infos(&queues)
            .enabled_extension_names(&enabled)
            .push_next(&mut v13);
        // SAFETY: `info` and everything it points to live across the
        // call; the extensions were all listed by the device.
        let device = unsafe { instance.create_device(physical, &info, None) }
            .map_err(|e| format!("{name}: {}", said("vkCreateDevice", e)))?;
        // SAFETY: queue 0 of `family` was asked for above.
        let queue = unsafe { device.get_device_queue(family, 0) };
        // SAFETY: `physical` came from this instance.
        let memory = unsafe { instance.get_physical_device_memory_properties(physical) };
        let mut host_align = 4096;
        let host_import = host.then(|| {
            let mut props = vk::PhysicalDeviceExternalMemoryHostPropertiesEXT::default();
            let mut all = vk::PhysicalDeviceProperties2::default().push_next(&mut props);
            // SAFETY: `all` and its chain live across the call.
            unsafe { instance.get_physical_device_properties2(physical, &mut all) };
            host_align = props.min_imported_host_pointer_alignment.max(1);
            ash::ext::external_memory_host::Device::new(&instance, &device)
        });
        let dmabuf = dmabuf.then(|| ash::khr::external_memory_fd::Device::new(&instance, &device));
        let mut parts = Parts::new(&device, family)
            .map_err(|error| format!("{name}: {error}"))
            .inspect_err(|_| {
                // SAFETY: nothing made from the device lives; `Parts::new`
                // destroyed what it made.
                unsafe { device.destroy_device(None) };
            })?;
        let mut gpu = Gpu {
            _entry: entry,
            instance,
            queue,
            name,
            software: kind == vk::PhysicalDeviceType::CPU,
            memory,
            host_import,
            host_align,
            dmabuf,
            pool: parts.pool,
            cmd: parts.cmd,
            fence: parts.fence,
            set_layout: parts.set_layout,
            pipeline_layout: parts.pipeline_layout,
            pipeline: parts.pipeline,
            sampler: parts.sampler,
            descriptors: parts.descriptors,
            atlas: Atlas::new(ATLAS_SIZE, ATLAS_SIZE),
            atlas_texture: Texture {
                image: vk::Image::null(),
                memory: vk::DeviceMemory::null(),
                view: vk::ImageView::null(),
                set: vk::DescriptorSet::null(),
                size: (0, 0),
                layout: vk::ImageLayout::UNDEFINED,
                used: 0,
                keep: None,
            },
            staging: None,
            instances: None,
            readback: None,
            textures: HashMap::new(),
            layers: Vec::new(),
            frame: 0,
            device,
        };
        parts.forget();
        gpu.atlas_texture = gpu.texture(
            (ATLAS_SIZE, ATLAS_SIZE),
            vk::Format::R8_UNORM,
            vk::ImageUsageFlags::SAMPLED | vk::ImageUsageFlags::TRANSFER_DST,
        )?;
        Ok(gpu)
    }

    /// A memory type among `bits` with `flags`.
    pub(crate) fn memory_type(&self, bits: u32, flags: vk::MemoryPropertyFlags) -> Option<u32> {
        let count = usize::try_from(self.memory.memory_type_count).unwrap_or(0);
        self.memory
            .memory_types
            .iter()
            .take(count)
            .enumerate()
            .find(|(index, kind)| bits & (1 << index) != 0 && kind.property_flags.contains(flags))
            .and_then(|(index, _)| u32::try_from(index).ok())
    }

    /// A device-local image of `size` in `format`, with a view and, when
    /// it is sampled, a set.
    pub(crate) fn texture(
        &mut self,
        size: (u32, u32),
        format: vk::Format,
        usage: vk::ImageUsageFlags,
    ) -> Result<Texture, String> {
        let info = vk::ImageCreateInfo::default()
            .image_type(vk::ImageType::TYPE_2D)
            .format(format)
            .extent(vk::Extent3D {
                width: size.0.max(1),
                height: size.1.max(1),
                depth: 1,
            })
            .mip_levels(1)
            .array_layers(1)
            .samples(vk::SampleCountFlags::TYPE_1)
            .tiling(vk::ImageTiling::OPTIMAL)
            .usage(usage)
            .initial_layout(vk::ImageLayout::UNDEFINED);
        // SAFETY: `info` describes a 2D image the device must support for
        // these common formats and usages.
        let image = unsafe { self.device.create_image(&info, None) }
            .map_err(|e| said("vkCreateImage", e))?;
        // SAFETY: `image` was just made on this device.
        let needs = unsafe { self.device.get_image_memory_requirements(image) };
        let Some(kind) = self.memory_type(
            needs.memory_type_bits,
            vk::MemoryPropertyFlags::DEVICE_LOCAL,
        ) else {
            // SAFETY: `image` is unused.
            unsafe { self.device.destroy_image(image, None) };
            return Err("no device-local memory for an image".to_owned());
        };
        let alloc = vk::MemoryAllocateInfo::default()
            .allocation_size(needs.size)
            .memory_type_index(kind);
        // SAFETY: the size and type are the image's own requirements.
        let memory = match unsafe { self.device.allocate_memory(&alloc, None) } {
            Ok(memory) => memory,
            Err(e) => {
                // SAFETY: `image` is unused.
                unsafe { self.device.destroy_image(image, None) };
                return Err(said("vkAllocateMemory", e));
            }
        };
        let mut texture = Texture {
            image,
            memory,
            view: vk::ImageView::null(),
            set: vk::DescriptorSet::null(),
            size,
            layout: vk::ImageLayout::UNDEFINED,
            used: self.frame,
            keep: None,
        };
        // SAFETY: the memory meets the image's requirements and is bound
        // once, at offset 0.
        if let Err(e) = unsafe { self.device.bind_image_memory(image, memory, 0) } {
            self.destroy_texture(texture);
            return Err(said("vkBindImageMemory", e));
        }
        match self.view_and_set(image, format, usage.contains(vk::ImageUsageFlags::SAMPLED)) {
            Ok((view, set)) => {
                texture.view = view;
                texture.set = set;
                Ok(texture)
            }
            Err(error) => {
                self.destroy_texture(texture);
                Err(error)
            }
        }
    }

    /// A view of `image`, and a set sampling it when `sampled`.
    pub(crate) fn view_and_set(
        &mut self,
        image: vk::Image,
        format: vk::Format,
        sampled: bool,
    ) -> Result<(vk::ImageView, vk::DescriptorSet), String> {
        let info = vk::ImageViewCreateInfo::default()
            .image(image)
            .view_type(vk::ImageViewType::TYPE_2D)
            .format(format)
            .subresource_range(color_range());
        // SAFETY: `image` is a 2D colour image of `format` on this device.
        let view = unsafe { self.device.create_image_view(&info, None) }
            .map_err(|e| said("vkCreateImageView", e))?;
        if !sampled {
            return Ok((view, vk::DescriptorSet::null()));
        }
        let layouts = [self.set_layout];
        let alloc = vk::DescriptorSetAllocateInfo::default()
            .descriptor_pool(self.descriptors)
            .set_layouts(&layouts);
        // SAFETY: the pool and layout are this device's.
        let set = match unsafe { self.device.allocate_descriptor_sets(&alloc) } {
            Ok(sets) => sets.first().copied().unwrap_or_default(),
            Err(e) => {
                // SAFETY: `view` is unused.
                unsafe { self.device.destroy_image_view(view, None) };
                return Err(said("vkAllocateDescriptorSets", e));
            }
        };
        let images = [vk::DescriptorImageInfo::default()
            .sampler(self.sampler)
            .image_view(view)
            .image_layout(vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL)];
        let write = [vk::WriteDescriptorSet::default()
            .dst_set(set)
            .dst_binding(0)
            .descriptor_type(vk::DescriptorType::COMBINED_IMAGE_SAMPLER)
            .image_info(&images)];
        // SAFETY: `set` was just allocated and is not in use by any
        // command buffer.
        unsafe { self.device.update_descriptor_sets(&write, &[]) };
        Ok((view, set))
    }

    /// Let a texture go. The device must be done with it.
    pub(crate) fn destroy_texture(&mut self, texture: Texture) {
        if texture.set != vk::DescriptorSet::null() {
            // SAFETY: the set came from this pool, which frees sets, and no
            // pending command uses it (one frame in flight, waited for).
            let _ = unsafe {
                self.device
                    .free_descriptor_sets(self.descriptors, &[texture.set])
            };
        }
        if texture.view != vk::ImageView::null() {
            // SAFETY: as above, nothing uses the view.
            unsafe { self.device.destroy_image_view(texture.view, None) };
        }
        if texture.image != vk::Image::null() {
            // SAFETY: as above.
            unsafe { self.device.destroy_image(texture.image, None) };
        }
        if texture.memory != vk::DeviceMemory::null() {
            // SAFETY: the image bound to it is gone.
            unsafe { self.device.free_memory(texture.memory, None) };
        }
    }

    /// A host-visible, coherent buffer of at least `size` bytes, mapped.
    pub(crate) fn host_buffer(
        &self,
        size: u64,
        usage: vk::BufferUsageFlags,
        cached: bool,
    ) -> Result<HostBuffer, String> {
        let size = size.max(256);
        let info = vk::BufferCreateInfo::default().size(size).usage(usage);
        // SAFETY: `info` is a plain exclusive buffer.
        let buffer = unsafe { self.device.create_buffer(&info, None) }
            .map_err(|e| said("vkCreateBuffer", e))?;
        // SAFETY: `buffer` is this device's.
        let needs = unsafe { self.device.get_buffer_memory_requirements(buffer) };
        let visible =
            vk::MemoryPropertyFlags::HOST_VISIBLE | vk::MemoryPropertyFlags::HOST_COHERENT;
        let kind = cached
            .then(|| {
                self.memory_type(
                    needs.memory_type_bits,
                    visible | vk::MemoryPropertyFlags::HOST_CACHED,
                )
            })
            .flatten()
            .or_else(|| self.memory_type(needs.memory_type_bits, visible));
        let Some(kind) = kind else {
            // SAFETY: `buffer` is unused.
            unsafe { self.device.destroy_buffer(buffer, None) };
            return Err("no host-visible memory".to_owned());
        };
        let alloc = vk::MemoryAllocateInfo::default()
            .allocation_size(needs.size)
            .memory_type_index(kind);
        // SAFETY: the buffer's own requirements.
        let memory = match unsafe { self.device.allocate_memory(&alloc, None) } {
            Ok(memory) => memory,
            Err(e) => {
                // SAFETY: `buffer` is unused.
                unsafe { self.device.destroy_buffer(buffer, None) };
                return Err(said("vkAllocateMemory", e));
            }
        };
        let fail = |e: vk::Result, what: &str| {
            // SAFETY: neither is in use.
            unsafe { self.device.destroy_buffer(buffer, None) };
            // SAFETY: the buffer bound to it is gone.
            unsafe { self.device.free_memory(memory, None) };
            said(what, e)
        };
        // SAFETY: bound once at offset 0, as required.
        if let Err(e) = unsafe { self.device.bind_buffer_memory(buffer, memory, 0) } {
            return Err(fail(e, "vkBindBufferMemory"));
        }
        // SAFETY: host-visible memory, mapped whole once.
        let ptr = match unsafe {
            self.device
                .map_memory(memory, 0, vk::WHOLE_SIZE, vk::MemoryMapFlags::empty())
        } {
            Ok(ptr) => ptr.cast::<u8>(),
            Err(e) => return Err(fail(e, "vkMapMemory")),
        };
        Ok(HostBuffer {
            buffer,
            memory,
            ptr,
            size,
        })
    }

    /// Let a buffer go. The device must be done with it.
    pub(crate) fn destroy_buffer(&self, buffer: HostBuffer) {
        // SAFETY: nothing pending uses it (one frame in flight).
        unsafe { self.device.destroy_buffer(buffer.buffer, None) };
        // SAFETY: the buffer bound to it is gone; freeing unmaps.
        unsafe { self.device.free_memory(buffer.memory, None) };
    }

    /// `slot`, grown to hold `size` bytes.
    pub(crate) fn ensure(
        &self,
        slot: &mut Option<HostBuffer>,
        size: u64,
        usage: vk::BufferUsageFlags,
        cached: bool,
    ) -> Result<(), String> {
        if slot.as_ref().is_some_and(|b| b.size >= size) {
            return Ok(());
        }
        if let Some(old) = slot.take() {
            self.destroy_buffer(old);
        }
        // Room to grow, so a frame a little larger does not make another.
        let grown = size.saturating_add(size / 2).next_power_of_two();
        *slot = Some(self.host_buffer(grown, usage, cached)?);
        Ok(())
    }

    /// Start recording the command buffer.
    pub(crate) fn begin(&self) -> Result<(), String> {
        // SAFETY: the buffer is not pending: the last submission was
        // waited for.
        unsafe {
            self.device
                .reset_command_buffer(self.cmd, vk::CommandBufferResetFlags::empty())
        }
        .map_err(|e| said("vkResetCommandBuffer", e))?;
        let info = vk::CommandBufferBeginInfo::default()
            .flags(vk::CommandBufferUsageFlags::ONE_TIME_SUBMIT);
        // SAFETY: the buffer was just reset.
        unsafe { self.device.begin_command_buffer(self.cmd, &info) }
            .map_err(|e| said("vkBeginCommandBuffer", e))
    }

    /// End the command buffer, submit it and wait until it has run.
    pub(crate) fn submit_and_wait(&self) -> Result<(), String> {
        // SAFETY: the buffer is recording.
        unsafe { self.device.end_command_buffer(self.cmd) }
            .map_err(|e| said("vkEndCommandBuffer", e))?;
        let cmds = [self.cmd];
        let submit = [vk::SubmitInfo::default().command_buffers(&cmds)];
        // SAFETY: the fence is unsignalled (reset after each wait).
        unsafe { self.device.queue_submit(self.queue, &submit, self.fence) }
            .map_err(|e| said("vkQueueSubmit", e))?;
        // SAFETY: the fence was just submitted with the buffer.
        unsafe {
            self.device
                .wait_for_fences(&[self.fence], true, 10_000_000_000)
        }
        .map_err(|e| said("vkWaitForFences", e))?;
        // SAFETY: the fence is signalled and no submission names it.
        unsafe { self.device.reset_fences(&[self.fence]) }.map_err(|e| said("vkResetFences", e))
    }

    /// Let go of the pictures not drawn in the last `frames` frames.
    pub(crate) fn forget_unused(&mut self, frames: u64) {
        let now = self.frame;
        let old: Vec<u64> = self
            .textures
            .iter()
            .filter(|(_, t)| now.saturating_sub(t.used) > frames)
            .map(|(key, _)| *key)
            .collect();
        for key in old {
            if let Some(texture) = self.textures.remove(&key) {
                self.destroy_texture(texture);
            }
        }
    }

    /// An image barrier from `from` to `to` on the command buffer.
    pub(crate) fn barrier(
        &self,
        image: vk::Image,
        from: vk::ImageLayout,
        to: vk::ImageLayout,
        (src_stage, src_access): (vk::PipelineStageFlags2, vk::AccessFlags2),
        (dst_stage, dst_access): (vk::PipelineStageFlags2, vk::AccessFlags2),
    ) {
        let barriers = [vk::ImageMemoryBarrier2::default()
            .src_stage_mask(src_stage)
            .src_access_mask(src_access)
            .dst_stage_mask(dst_stage)
            .dst_access_mask(dst_access)
            .old_layout(from)
            .new_layout(to)
            .src_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
            .dst_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
            .image(image)
            .subresource_range(color_range())];
        let info = vk::DependencyInfo::default().image_memory_barriers(&barriers);
        // SAFETY: the command buffer is recording and `image` is this
        // device's, whole, in layout `from`.
        unsafe { self.device.cmd_pipeline_barrier2(self.cmd, &info) };
    }
}

/// The whole of a colour image with one level and one layer.
pub(crate) fn color_range() -> vk::ImageSubresourceRange {
    vk::ImageSubresourceRange {
        aspect_mask: vk::ImageAspectFlags::COLOR,
        base_mip_level: 0,
        level_count: 1,
        base_array_layer: 0,
        layer_count: 1,
    }
}

/// What the device is made with beside itself, made in order and let go
/// in reverse when a later one fails.
struct Parts {
    /// The device they are made on.
    device: ash::Device,
    /// The command pool.
    pool: vk::CommandPool,
    /// Its one buffer.
    cmd: vk::CommandBuffer,
    /// The fence.
    fence: vk::Fence,
    /// The set layout.
    set_layout: vk::DescriptorSetLayout,
    /// The pipeline layout.
    pipeline_layout: vk::PipelineLayout,
    /// The pipeline.
    pipeline: vk::Pipeline,
    /// The sampler.
    sampler: vk::Sampler,
    /// The descriptor pool.
    descriptors: vk::DescriptorPool,
    /// Whether they now belong to a [`Gpu`].
    kept: bool,
}

impl Drop for Parts {
    fn drop(&mut self) {
        if self.kept {
            return;
        }
        let d = &self.device;
        // SAFETY: made on `d`, never submitted or used (each below too).
        unsafe { d.destroy_descriptor_pool(self.descriptors, None) };
        // SAFETY: as above.
        unsafe { d.destroy_sampler(self.sampler, None) };
        // SAFETY: as above.
        unsafe { d.destroy_pipeline(self.pipeline, None) };
        // SAFETY: as above.
        unsafe { d.destroy_pipeline_layout(self.pipeline_layout, None) };
        // SAFETY: as above.
        unsafe { d.destroy_descriptor_set_layout(self.set_layout, None) };
        // SAFETY: as above.
        unsafe { d.destroy_fence(self.fence, None) };
        // SAFETY: as above; destroying the pool frees its buffer.
        unsafe { d.destroy_command_pool(self.pool, None) };
    }
}

impl Parts {
    /// Make them all on `device`.
    fn new(device: &ash::Device, family: u32) -> Result<Self, String> {
        let mut parts = Parts {
            device: device.clone(),
            pool: vk::CommandPool::null(),
            cmd: vk::CommandBuffer::null(),
            fence: vk::Fence::null(),
            set_layout: vk::DescriptorSetLayout::null(),
            pipeline_layout: vk::PipelineLayout::null(),
            pipeline: vk::Pipeline::null(),
            sampler: vk::Sampler::null(),
            descriptors: vk::DescriptorPool::null(),
            kept: false,
        };
        let info = vk::CommandPoolCreateInfo::default()
            .queue_family_index(family)
            .flags(vk::CommandPoolCreateFlags::RESET_COMMAND_BUFFER);
        // SAFETY: `family` is one of the device's queue families.
        parts.pool = unsafe { device.create_command_pool(&info, None) }
            .map_err(|e| said("vkCreateCommandPool", e))?;
        let alloc = vk::CommandBufferAllocateInfo::default()
            .command_pool(parts.pool)
            .level(vk::CommandBufferLevel::PRIMARY)
            .command_buffer_count(1);
        // SAFETY: the pool was just made.
        parts.cmd = unsafe { device.allocate_command_buffers(&alloc) }
            .map_err(|e| said("vkAllocateCommandBuffers", e))?
            .first()
            .copied()
            .unwrap_or_default();
        // SAFETY: a plain fence.
        parts.fence = unsafe { device.create_fence(&vk::FenceCreateInfo::default(), None) }
            .map_err(|e| said("vkCreateFence", e))?;
        let bindings = [vk::DescriptorSetLayoutBinding::default()
            .binding(0)
            .descriptor_type(vk::DescriptorType::COMBINED_IMAGE_SAMPLER)
            .descriptor_count(1)
            .stage_flags(vk::ShaderStageFlags::FRAGMENT)];
        let info = vk::DescriptorSetLayoutCreateInfo::default().bindings(&bindings);
        // SAFETY: one sampled-image binding.
        parts.set_layout = unsafe { device.create_descriptor_set_layout(&info, None) }
            .map_err(|e| said("vkCreateDescriptorSetLayout", e))?;
        let sizes = [vk::DescriptorPoolSize {
            ty: vk::DescriptorType::COMBINED_IMAGE_SAMPLER,
            descriptor_count: 1024,
        }];
        let info = vk::DescriptorPoolCreateInfo::default()
            .flags(vk::DescriptorPoolCreateFlags::FREE_DESCRIPTOR_SET)
            .max_sets(1024)
            .pool_sizes(&sizes);
        // SAFETY: a pool of sampled-image sets.
        parts.descriptors = unsafe { device.create_descriptor_pool(&info, None) }
            .map_err(|e| said("vkCreateDescriptorPool", e))?;
        let info = vk::SamplerCreateInfo::default()
            .mag_filter(vk::Filter::LINEAR)
            .min_filter(vk::Filter::LINEAR)
            .mipmap_mode(vk::SamplerMipmapMode::NEAREST)
            .address_mode_u(vk::SamplerAddressMode::CLAMP_TO_EDGE)
            .address_mode_v(vk::SamplerAddressMode::CLAMP_TO_EDGE)
            .address_mode_w(vk::SamplerAddressMode::CLAMP_TO_EDGE)
            .max_lod(0.0);
        // SAFETY: a bilinear clamped sampler, no anisotropy.
        parts.sampler = unsafe { device.create_sampler(&info, None) }
            .map_err(|e| said("vkCreateSampler", e))?;
        let layouts = [parts.set_layout];
        let push = [vk::PushConstantRange {
            stage_flags: vk::ShaderStageFlags::VERTEX,
            offset: 0,
            size: 8,
        }];
        let info = vk::PipelineLayoutCreateInfo::default()
            .set_layouts(&layouts)
            .push_constant_ranges(&push);
        // SAFETY: the set layout was just made.
        parts.pipeline_layout = unsafe { device.create_pipeline_layout(&info, None) }
            .map_err(|e| said("vkCreatePipelineLayout", e))?;
        parts.pipeline = pipeline(device, parts.pipeline_layout)?;
        Ok(parts)
    }

    /// They belong to the [`Gpu`] now.
    fn forget(&mut self) {
        self.kept = true;
    }
}

/// SPIR-V bytes as the words a shader module takes.
fn words(bytes: &[u8]) -> Vec<u32> {
    bytes
        .chunks_exact(4)
        .filter_map(|w| <[u8; 4]>::try_from(w).ok())
        .map(u32::from_le_bytes)
        .collect()
}

/// The one pipeline: instanced quads, premultiplied source-over, drawn
/// into [`FORMAT`] by dynamic rendering.
fn pipeline(device: &ash::Device, layout: vk::PipelineLayout) -> Result<vk::Pipeline, String> {
    let module = |bytes: &[u8]| {
        let code = words(bytes);
        let info = vk::ShaderModuleCreateInfo::default().code(&code);
        // SAFETY: the code is SPIR-V compiled for Vulkan 1.3 and checked by
        // spirv-val when it was made.
        unsafe { device.create_shader_module(&info, None) }
            .map_err(|e| said("vkCreateShaderModule", e))
    };
    let vert = module(VERT)?;
    let frag = match module(FRAG) {
        Ok(frag) => frag,
        Err(error) => {
            // SAFETY: unused.
            unsafe { device.destroy_shader_module(vert, None) };
            return Err(error);
        }
    };
    let stages = [
        vk::PipelineShaderStageCreateInfo::default()
            .stage(vk::ShaderStageFlags::VERTEX)
            .module(vert)
            .name(c"main"),
        vk::PipelineShaderStageCreateInfo::default()
            .stage(vk::ShaderStageFlags::FRAGMENT)
            .module(frag)
            .name(c"main"),
    ];
    let bindings = [vk::VertexInputBindingDescription {
        binding: 0,
        stride: super::frame::INSTANCE_BYTES,
        input_rate: vk::VertexInputRate::INSTANCE,
    }];
    let attributes: Vec<vk::VertexInputAttributeDescription> = (0..5)
        .map(|location| vk::VertexInputAttributeDescription {
            location,
            binding: 0,
            format: vk::Format::R32G32B32A32_SFLOAT,
            offset: location * 16,
        })
        .collect();
    let vertex = vk::PipelineVertexInputStateCreateInfo::default()
        .vertex_binding_descriptions(&bindings)
        .vertex_attribute_descriptions(&attributes);
    let assembly = vk::PipelineInputAssemblyStateCreateInfo::default()
        .topology(vk::PrimitiveTopology::TRIANGLE_LIST);
    let viewport = vk::PipelineViewportStateCreateInfo::default()
        .viewport_count(1)
        .scissor_count(1);
    let raster = vk::PipelineRasterizationStateCreateInfo::default()
        .polygon_mode(vk::PolygonMode::FILL)
        .cull_mode(vk::CullModeFlags::NONE)
        .front_face(vk::FrontFace::COUNTER_CLOCKWISE)
        .line_width(1.0);
    let multisample = vk::PipelineMultisampleStateCreateInfo::default()
        .rasterization_samples(vk::SampleCountFlags::TYPE_1);
    let blend = [vk::PipelineColorBlendAttachmentState::default()
        .blend_enable(true)
        .src_color_blend_factor(vk::BlendFactor::ONE)
        .dst_color_blend_factor(vk::BlendFactor::ONE_MINUS_SRC_ALPHA)
        .color_blend_op(vk::BlendOp::ADD)
        .src_alpha_blend_factor(vk::BlendFactor::ONE)
        .dst_alpha_blend_factor(vk::BlendFactor::ONE_MINUS_SRC_ALPHA)
        .alpha_blend_op(vk::BlendOp::ADD)
        .color_write_mask(vk::ColorComponentFlags::RGBA)];
    let blending = vk::PipelineColorBlendStateCreateInfo::default().attachments(&blend);
    let dynamic = [vk::DynamicState::VIEWPORT, vk::DynamicState::SCISSOR];
    let dynamic = vk::PipelineDynamicStateCreateInfo::default().dynamic_states(&dynamic);
    let formats = [FORMAT];
    let mut rendering =
        vk::PipelineRenderingCreateInfo::default().color_attachment_formats(&formats);
    let info = [vk::GraphicsPipelineCreateInfo::default()
        .stages(&stages)
        .vertex_input_state(&vertex)
        .input_assembly_state(&assembly)
        .viewport_state(&viewport)
        .rasterization_state(&raster)
        .multisample_state(&multisample)
        .color_blend_state(&blending)
        .dynamic_state(&dynamic)
        .layout(layout)
        .push_next(&mut rendering)];
    // SAFETY: every state the pipeline needs is given, the formats match
    // what it draws into, and the modules live across the call.
    let made = unsafe { device.create_graphics_pipelines(vk::PipelineCache::null(), &info, None) };
    // SAFETY: the modules are no longer needed once the pipeline is made.
    unsafe { device.destroy_shader_module(vert, None) };
    // SAFETY: as above.
    unsafe { device.destroy_shader_module(frag, None) };
    made.map_err(|(_, e)| said("vkCreateGraphicsPipelines", e))
        .map(|pipelines| pipelines.first().copied().unwrap_or_default())
}

impl Drop for Gpu {
    fn drop(&mut self) {
        // SAFETY: waiting on the device's own queue.
        let _ = unsafe { self.device.device_wait_idle() };
        for (_, texture) in std::mem::take(&mut self.textures) {
            self.destroy_texture(texture);
        }
        for texture in std::mem::take(&mut self.layers) {
            self.destroy_texture(texture);
        }
        let atlas = std::mem::replace(
            &mut self.atlas_texture,
            Texture {
                image: vk::Image::null(),
                memory: vk::DeviceMemory::null(),
                view: vk::ImageView::null(),
                set: vk::DescriptorSet::null(),
                size: (0, 0),
                layout: vk::ImageLayout::UNDEFINED,
                used: 0,
                keep: None,
            },
        );
        self.destroy_texture(atlas);
        for buffer in [
            self.staging.take(),
            self.instances.take(),
            self.readback.take(),
        ]
        .into_iter()
        .flatten()
        {
            self.destroy_buffer(buffer);
        }
        let parts = Parts {
            device: self.device.clone(),
            pool: self.pool,
            cmd: self.cmd,
            fence: self.fence,
            set_layout: self.set_layout,
            pipeline_layout: self.pipeline_layout,
            pipeline: self.pipeline,
            sampler: self.sampler,
            descriptors: self.descriptors,
            kept: false,
        };
        drop(parts);
        // SAFETY: everything made on the device is gone.
        unsafe { self.device.destroy_device(None) };
        // SAFETY: the device made from it is gone.
        unsafe { self.instance.destroy_instance(None) };
    }
}
