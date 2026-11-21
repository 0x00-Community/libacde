use std::os::fd::{AsFd, AsRawFd, FromRawFd, IntoRawFd, OwnedFd};
use std::path::Path;

use crate::gpu::DeviceRequest;
use crate::gpu::Dmabuf;

use ash::vk;
use wgpu::hal::vulkan::Api as Vk;

const EXTENSIONS: [&std::ffi::CStr; 4] = [
    ash::khr::external_memory_fd::NAME,
    ash::ext::external_memory_dma_buf::NAME,
    ash::ext::image_drm_format_modifier::NAME,
    ash::khr::external_semaphore_fd::NAME,
];

const SEMAPHORES: usize = 8;
const WAITS: usize = 64;
const VELLO_STORAGE_BUFFERS: u32 = 8;
const VELLO_INVOCATIONS: u32 = 256;

#[derive(Clone, Copy)]
pub struct Fence(vk::Semaphore);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Backend {
    Vulkan,
    Gles,
}

impl Backend {
    fn wgpu(self) -> wgpu::Backends {
        match self {
            Backend::Vulkan => wgpu::Backends::VULKAN,
            Backend::Gles => wgpu::Backends::GL,
        }
    }
}

type Opened = (wgpu::Device, wgpu::Queue, &'static str);

pub struct Device {
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
    pub info: wgpu::AdapterInfo,
    pub backend: Backend,
    pub tier: &'static str,
    downlevel: wgpu::DownlevelCapabilities,
    vello: Result<(), String>,
    semaphores: Vec<vk::Semaphore>,
    next: usize,
    waits: Vec<vk::Semaphore>,
    next_wait: usize,
}

impl Device {
    pub fn open(backend: Backend, node: &Path) -> Result<Self, String> {
        let (adapter, (device, queue, tier)) = Self::on_node(backend, node)?;
        let vello = match backend {
            Backend::Vulkan => Ok(()),
            Backend::Gles => Self::try_vello(&device, &queue, &adapter.get_downlevel_capabilities()),
        };
        Ok(Self::assemble(&adapter, (device, queue, tier), backend, vello))
    }

    pub fn probe(backend: Backend, node: &Path) -> Result<String, String> {
        let (adapter, opened) = Self::on_node(backend, node)?;
        Ok(Self::assemble(&adapter, opened, backend, Err("not tried by the probe".into())).describe())
    }

    pub fn open_hardware() -> Result<Self, String> {
        let mut errors = Vec::new();
        let mut adapters: Vec<_> = Self::adapters(Backend::Vulkan).into_iter().filter(|a| a.get_info().device_type != wgpu::DeviceType::Cpu).collect();
        adapters.sort_by_key(|a| match a.get_info().device_type {
            wgpu::DeviceType::DiscreteGpu | wgpu::DeviceType::IntegratedGpu => 0,
            _ => 1,
        });
        let (adapter, opened) = adapters
            .into_iter()
            .find_map(|adapter| match Self::open_adapter(Backend::Vulkan, &adapter) {
                Ok(opened) => Some((adapter, opened)),
                Err(e) => {
                    errors.push(format!("{}: {e}", adapter.get_info().name));
                    None
                }
            })
            .ok_or_else(|| if errors.is_empty() { "no GPU (Vulkan)".to_string() } else { errors.join("; ") })?;
        Ok(Self::assemble(&adapter, opened, Backend::Vulkan, Ok(())))
    }

    pub fn fits_vello(&self) -> bool {
        self.vello.is_ok()
    }

    pub fn describe(&self) -> String {
        let (info, limits, flags) = (&self.info, self.device.limits(), self.downlevel.flags);
        let flag = |name: &str, f: wgpu::DownlevelFlags| format!("{name}={}", flags.contains(f));
        let vello = match &self.vello {
            Ok(()) => "vello=runs".to_owned(),
            Err(e) => format!("vello=no ({e})"),
        };
        let workgroup = (limits.max_compute_workgroup_size_x, limits.max_compute_workgroup_size_y, limits.max_compute_workgroup_size_z);
        [
            format!("{} ({:?}, {:?}, driver {} {})", info.name, self.backend, info.device_type, info.driver, info.driver_info),
            format!("device tier {}", self.tier),
            format!("max_storage_buffers_per_shader_stage={}", limits.max_storage_buffers_per_shader_stage),
            format!("max_compute_workgroup_size={}x{}x{}", workgroup.0, workgroup.1, workgroup.2),
            format!("max_compute_invocations_per_workgroup={}", limits.max_compute_invocations_per_workgroup),
            format!("max_compute_workgroup_storage_size={}", limits.max_compute_workgroup_storage_size),
            format!("max_storage_buffer_binding_size={}", limits.max_storage_buffer_binding_size),
            flag("COMPUTE_SHADERS", wgpu::DownlevelFlags::COMPUTE_SHADERS),
            flag("FRAGMENT_WRITABLE_STORAGE", wgpu::DownlevelFlags::FRAGMENT_WRITABLE_STORAGE),
            flag("VERTEX_STORAGE", wgpu::DownlevelFlags::VERTEX_STORAGE),
            flag("INDIRECT_EXECUTION", wgpu::DownlevelFlags::INDIRECT_EXECUTION),
            vello,
        ]
        .join(", ")
    }

    fn assemble(adapter: &wgpu::Adapter, (device, queue, tier): Opened, backend: Backend, vello: Result<(), String>) -> Self {
        let downlevel = adapter.get_downlevel_capabilities();
        let mut opened =
            Self { device, queue, info: adapter.get_info(), backend, tier, downlevel, vello, semaphores: Vec::new(), next: 0, waits: Vec::new(), next_wait: 0 };
        if backend == Backend::Vulkan {
            opened.semaphores = opened.create_semaphores();
            opened.waits = opened.create_waits();
        }
        opened
    }

    fn adapters(backend: Backend) -> Vec<wgpu::Adapter> {
        let backends = backend.wgpu();
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor { backends, ..wgpu::InstanceDescriptor::new_without_display_handle() });
        pollster::block_on(instance.enumerate_adapters(backends))
    }

    fn on_node(backend: Backend, node: &Path) -> Result<(wgpu::Adapter, Opened), String> {
        let adapter = Self::pick(Self::adapters(backend), Self::pci_ids(node), backend, node)?;
        let opened = Self::open_adapter(backend, &adapter).map_err(|e| format!("{}: {e}", adapter.get_info().name))?;
        Ok((adapter, opened))
    }

    fn pci_ids(node: &Path) -> Option<(u32, u32)> {
        let name = node.file_name()?.to_str()?;
        let read = |what: &str| {
            let text = std::fs::read_to_string(format!("/sys/class/drm/{name}/device/{what}")).ok()?;
            u32::from_str_radix(text.trim().trim_start_matches("0x"), 16).ok()
        };
        Some((read("vendor")?, read("device")?))
    }

    fn pick(adapters: Vec<wgpu::Adapter>, ids: Option<(u32, u32)>, backend: Backend, node: &Path) -> Result<wgpu::Adapter, String> {
        let names: Vec<String> = adapters.iter().map(|a| format!("{} ({:?})", a.get_info().name, a.get_info().device_type)).collect();
        let mut hardware: Vec<_> = adapters.into_iter().filter(|a| a.get_info().device_type != wgpu::DeviceType::Cpu).collect();
        if let Some((vendor, device)) = ids
            && let Some(at) = hardware.iter().position(|a| (a.get_info().vendor, a.get_info().device) == (vendor, device))
        {
            return Ok(hardware.swap_remove(at));
        }
        match hardware.len() {
            1 => Ok(hardware.remove(0)),
            0 if names.is_empty() => Err(format!("no {backend:?} adapter")),
            0 => Err(format!("only software {backend:?} adapters: {}", names.join(", "))),
            _ => Err(format!("{} matches none of the {backend:?} adapters, and there is more than one: {}", node.display(), names.join(", "))),
        }
    }

    fn open_adapter(backend: Backend, adapter: &wgpu::Adapter) -> Result<Opened, String> {
        if backend == Backend::Vulkan {
            let hal_adapter = unsafe { adapter.as_hal::<Vk>() }.ok_or("not a Vulkan adapter")?;
            let caps = hal_adapter.physical_device_capabilities();
            let missing: Vec<_> = EXTENSIONS.iter().filter(|e| !caps.supports_extension(e)).map(|e| e.to_string_lossy()).collect();
            if !missing.is_empty() {
                return Err(format!("missing Vulkan extensions {}", missing.join(", ")));
            }
        }
        let tiers = DeviceRequest::tiers(adapter.features() & wgpu::Features::all_webgpu_mask(), adapter.limits());
        let ((device, queue), tier) = DeviceRequest::open("acde", &tiers, |request| match backend {
            Backend::Vulkan => Self::open_tier(adapter, request),
            Backend::Gles => pollster::block_on(adapter.request_device(&request.descriptor("acde"))).map_err(|e| e.to_string()),
        })?;
        Ok((device, queue, tier))
    }

    fn try_vello(device: &wgpu::Device, queue: &wgpu::Queue, downlevel: &wgpu::DownlevelCapabilities) -> Result<(), String> {
        let limits = device.limits();
        if !downlevel.flags.contains(wgpu::DownlevelFlags::COMPUTE_SHADERS) {
            return Err("no compute shaders".into());
        }
        if limits.max_storage_buffers_per_shader_stage < VELLO_STORAGE_BUFFERS {
            return Err(format!("{} storage buffers per stage, vello needs {VELLO_STORAGE_BUFFERS}", limits.max_storage_buffers_per_shader_stage));
        }
        if limits.max_compute_invocations_per_workgroup < VELLO_INVOCATIONS || limits.max_compute_workgroup_size_x < VELLO_INVOCATIONS {
            return Err(format!("workgroups of {} invocations, vello needs {VELLO_INVOCATIONS}", limits.max_compute_invocations_per_workgroup));
        }
        let started = std::time::Instant::now();
        let validation = device.push_error_scope(wgpu::ErrorFilter::Validation);
        let internal = device.push_error_scope(wgpu::ErrorFilter::Internal);
        let build = || crate::vello::Vello::on_this_thread(device, queue).map(drop);
        let built = std::panic::catch_unwind(std::panic::AssertUnwindSafe(build)).unwrap_or_else(|_| Err("building vello's pipelines panicked".into()));
        let internal = pollster::block_on(internal.pop());
        let validation = pollster::block_on(validation.pop());
        let errors: Vec<String> = [built.err(), internal.map(|e| e.to_string()), validation.map(|e| e.to_string())].into_iter().flatten().collect();
        if !errors.is_empty() {
            return Err(format!("vello's pipelines do not build: {}", errors.join("; ")));
        }
        log::info!("acde: vello's pipelines built on the GLES device in {:.1}ms", started.elapsed().as_secs_f64() * 1000.0);
        Ok(())
    }

    fn open_tier(adapter: &wgpu::Adapter, request: &DeviceRequest) -> Result<(wgpu::Device, wgpu::Queue), String> {
        let hal = unsafe {
            let hal_adapter = adapter.as_hal::<Vk>().ok_or("not a Vulkan adapter")?;
            hal_adapter
                .open_with_callback(
                    request.features,
                    &request.limits,
                    &wgpu::MemoryHints::MemoryUsage,
                    Some(Box::new(|args| args.extensions.extend(EXTENSIONS))),
                )
                .map_err(|e| format!("opening the Vulkan device: {e}"))?
        };
        unsafe { adapter.create_device_from_hal::<Vk>(hal, &request.descriptor("acde")) }.map_err(|e| format!("wgpu device: {e}"))
    }

    fn create_semaphores(&self) -> Vec<vk::Semaphore> {
        unsafe {
            let Some(hal_device) = self.device.as_hal::<Vk>() else { return Vec::new() };
            let raw = hal_device.raw_device();
            (0..SEMAPHORES)
                .map_while(|_| {
                    let mut export = vk::ExportSemaphoreCreateInfo::default().handle_types(vk::ExternalSemaphoreHandleTypeFlags::SYNC_FD);
                    raw.create_semaphore(&vk::SemaphoreCreateInfo::default().push_next(&mut export), None).ok()
                })
                .collect()
        }
    }

    fn create_waits(&self) -> Vec<vk::Semaphore> {
        unsafe {
            let Some(hal_device) = self.device.as_hal::<Vk>() else { return Vec::new() };
            let raw = hal_device.raw_device();
            (0..WAITS).map_while(|_| raw.create_semaphore(&vk::SemaphoreCreateInfo::default(), None).ok()).collect()
        }
    }

    pub fn wait_for(&mut self, sync_file: OwnedFd) -> Result<(), OwnedFd> {
        let Some(&semaphore) = self.waits.get(self.next_wait) else { return Err(sync_file) };
        let imported = unsafe {
            let Some(hal_device) = self.device.as_hal::<Vk>() else { return Err(sync_file) };
            let ext = ash::khr::external_semaphore_fd::Device::new(hal_device.shared_instance().raw_instance(), hal_device.raw_device());
            let info = vk::ImportSemaphoreFdInfoKHR::default()
                .semaphore(semaphore)
                .flags(vk::SemaphoreImportFlags::TEMPORARY)
                .handle_type(vk::ExternalSemaphoreHandleTypeFlags::SYNC_FD)
                .fd(sync_file.as_raw_fd());
            ext.import_semaphore_fd(&info).is_ok()
        };
        if !imported {
            return Err(sync_file);
        }
        let _ = sync_file.into_raw_fd();
        if let Some(queue) = unsafe { self.queue.as_hal::<Vk>() } {
            queue.add_wait_semaphore(semaphore, None, vk::PipelineStageFlags::ALL_COMMANDS);
        }
        self.next_wait = (self.next_wait + 1) % self.waits.len();
        Ok(())
    }

    pub fn arm(&mut self) -> Option<Fence> {
        let semaphore = *self.semaphores.get(self.next)?;
        let queue = unsafe { self.queue.as_hal::<Vk>() }?;
        queue.add_signal_semaphore(semaphore, None);
        self.next = (self.next + 1) % self.semaphores.len();
        Some(Fence(semaphore))
    }

    pub fn sync_file(&self, Fence(semaphore): Fence) -> Option<OwnedFd> {
        let fd = unsafe {
            let hal_device = self.device.as_hal::<Vk>()?;
            let ext = ash::khr::external_semaphore_fd::Device::new(hal_device.shared_instance().raw_instance(), hal_device.raw_device());
            let info = vk::SemaphoreGetFdInfoKHR::default().semaphore(semaphore).handle_type(vk::ExternalSemaphoreHandleTypeFlags::SYNC_FD);
            ext.get_semaphore_fd(&info).ok()?
        };
        (fd >= 0).then(|| unsafe { OwnedFd::from_raw_fd(fd) })
    }

    pub fn import(&self, dmabuf: Dmabuf<'_>, (width, height): (u32, u32)) -> Result<wgpu::Texture, String> {
        unsafe {
            let hal_device = self.device.as_hal::<Vk>().ok_or("not a Vulkan device")?;
            let raw = hal_device.raw_device();
            let layouts = [vk::SubresourceLayout { offset: dmabuf.offset.into(), size: 0, row_pitch: dmabuf.stride.into(), array_pitch: 0, depth_pitch: 0 }];
            let mut explicit = vk::ImageDrmFormatModifierExplicitCreateInfoEXT::default().drm_format_modifier(dmabuf.modifier).plane_layouts(&layouts);
            let mut external = vk::ExternalMemoryImageCreateInfo::default().handle_types(vk::ExternalMemoryHandleTypeFlags::DMA_BUF_EXT);
            let info = vk::ImageCreateInfo::default()
                .image_type(vk::ImageType::TYPE_2D)
                .format(vk::Format::R8G8B8A8_UNORM)
                .extent(vk::Extent3D { width, height, depth: 1 })
                .mip_levels(1)
                .array_layers(1)
                .samples(vk::SampleCountFlags::TYPE_1)
                .tiling(vk::ImageTiling::DRM_FORMAT_MODIFIER_EXT)
                .usage(
                    vk::ImageUsageFlags::STORAGE
                        | vk::ImageUsageFlags::SAMPLED
                        | vk::ImageUsageFlags::TRANSFER_DST
                        | vk::ImageUsageFlags::TRANSFER_SRC
                        | vk::ImageUsageFlags::COLOR_ATTACHMENT,
                )
                .sharing_mode(vk::SharingMode::EXCLUSIVE)
                .initial_layout(vk::ImageLayout::UNDEFINED)
                .push_next(&mut external)
                .push_next(&mut explicit);
            let image = raw.create_image(&info, None).map_err(|e| format!("VkImage: {e}"))?;
            let requirements = raw.get_image_memory_requirements(image);
            let fd_ext = ash::khr::external_memory_fd::Device::new(hal_device.shared_instance().raw_instance(), raw);
            let mut fd_props = vk::MemoryFdPropertiesKHR::default();
            if let Err(e) = fd_ext.get_memory_fd_properties(vk::ExternalMemoryHandleTypeFlags::DMA_BUF_EXT, dmabuf.fd.as_raw_fd(), &mut fd_props) {
                raw.destroy_image(image, None);
                return Err(format!("dma-buf memory properties: {e}"));
            }
            let types = requirements.memory_type_bits & fd_props.memory_type_bits;
            if types == 0 {
                raw.destroy_image(image, None);
                return Err("no memory type can import this dma-buf".into());
            }
            let fd = dmabuf.fd.as_fd().try_clone_to_owned().map_err(|e| e.to_string())?;
            let mut import = vk::ImportMemoryFdInfoKHR::default().handle_type(vk::ExternalMemoryHandleTypeFlags::DMA_BUF_EXT).fd(fd.as_raw_fd());
            let mut dedicated = vk::MemoryDedicatedAllocateInfo::default().image(image);
            let alloc = vk::MemoryAllocateInfo::default()
                .allocation_size(requirements.size)
                .memory_type_index(types.trailing_zeros())
                .push_next(&mut import)
                .push_next(&mut dedicated);
            let memory = match raw.allocate_memory(&alloc, None) {
                Ok(memory) => {
                    let _ = fd.into_raw_fd();
                    memory
                }
                Err(e) => {
                    raw.destroy_image(image, None);
                    return Err(format!("importing the dma-buf's memory: {e}"));
                }
            };
            if let Err(e) = raw.bind_image_memory(image, memory, 0) {
                raw.free_memory(memory, None);
                raw.destroy_image(image, None);
                return Err(format!("binding the memory: {e}"));
            }
            let size = wgpu::Extent3d { width, height, depth_or_array_layers: 1 };
            let hal_desc = wgpu::hal::TextureDescriptor {
                label: Some("acde canvas"),
                size,
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba8Unorm,
                usage: wgpu::TextureUses::STORAGE_READ_WRITE
                    | wgpu::TextureUses::RESOURCE
                    | wgpu::TextureUses::COPY_DST
                    | wgpu::TextureUses::COPY_SRC
                    | wgpu::TextureUses::COLOR_TARGET,
                memory_flags: wgpu::hal::MemoryFlags::empty(),
                view_formats: vec![],
            };
            let hal_texture = hal_device.texture_from_raw(image, &hal_desc, None, wgpu::hal::vulkan::TextureMemory::Dedicated(memory));
            drop(hal_device);
            Ok(self.device.create_texture_from_hal::<Vk>(
                hal_texture,
                &wgpu::TextureDescriptor {
                    label: Some("acde canvas"),
                    size,
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format: wgpu::TextureFormat::Rgba8Unorm,
                    usage: wgpu::TextureUsages::STORAGE_BINDING
                        | wgpu::TextureUsages::TEXTURE_BINDING
                        | wgpu::TextureUsages::COPY_DST
                        | wgpu::TextureUsages::COPY_SRC
                        | wgpu::TextureUsages::RENDER_ATTACHMENT,
                    view_formats: &[],
                },
                wgpu::TextureUses::UNINITIALIZED,
            ))
        }
    }
}

pub struct Exported {
    pub fd: OwnedFd,
    pub offset: u32,
    pub stride: u32,
}

impl Exported {
    pub fn dmabuf(&self) -> Dmabuf<'_> {
        Dmabuf { fd: self.fd.as_fd(), offset: self.offset, stride: self.stride, modifier: Dmabuf::LINEAR }
    }

    const IMAGE: wgpu::TextureUsages = wgpu::TextureUsages::STORAGE_BINDING
        .union(wgpu::TextureUsages::TEXTURE_BINDING)
        .union(wgpu::TextureUsages::COPY_DST)
        .union(wgpu::TextureUsages::COPY_SRC);

    pub fn texture(device: &wgpu::Device, size: (u32, u32)) -> Result<(wgpu::Texture, Exported), String> {
        Self::with_usage(device, size, Self::IMAGE)
    }

    pub fn with_usage(device: &wgpu::Device, (width, height): (u32, u32), usage: wgpu::TextureUsages) -> Result<(wgpu::Texture, Exported), String> {
        use wgpu::{TextureUsages as U, TextureUses as H};
        const USES: [(U, vk::ImageUsageFlags, H); 5] = [
            (U::COPY_SRC, vk::ImageUsageFlags::TRANSFER_SRC, H::COPY_SRC),
            (U::COPY_DST, vk::ImageUsageFlags::TRANSFER_DST, H::COPY_DST),
            (U::TEXTURE_BINDING, vk::ImageUsageFlags::SAMPLED, H::RESOURCE),
            (U::STORAGE_BINDING, vk::ImageUsageFlags::STORAGE, H::STORAGE_READ_WRITE),
            (U::RENDER_ATTACHMENT, vk::ImageUsageFlags::COLOR_ATTACHMENT, H::COLOR_TARGET),
        ];
        if !USES.iter().fold(U::empty(), |all, (u, ..)| all | *u).contains(usage) {
            return Err(format!("an exportable image with usage {usage:?}"));
        }
        let (vk_usage, hal_usage) =
            USES.iter().filter(|(u, ..)| usage.contains(*u)).fold((vk::ImageUsageFlags::empty(), H::empty()), |(v, h), (_, vu, hu)| (v | *vu, h | *hu));
        unsafe {
            let hal_device = device.as_hal::<Vk>().ok_or("not a Vulkan device")?;
            let raw = hal_device.raw_device();
            let modifiers = [Dmabuf::LINEAR];
            let mut list = vk::ImageDrmFormatModifierListCreateInfoEXT::default().drm_format_modifiers(&modifiers);
            let mut external = vk::ExternalMemoryImageCreateInfo::default().handle_types(vk::ExternalMemoryHandleTypeFlags::DMA_BUF_EXT);
            let info = vk::ImageCreateInfo::default()
                .image_type(vk::ImageType::TYPE_2D)
                .format(vk::Format::R8G8B8A8_UNORM)
                .extent(vk::Extent3D { width, height, depth: 1 })
                .mip_levels(1)
                .array_layers(1)
                .samples(vk::SampleCountFlags::TYPE_1)
                .tiling(vk::ImageTiling::DRM_FORMAT_MODIFIER_EXT)
                .usage(vk_usage)
                .sharing_mode(vk::SharingMode::EXCLUSIVE)
                .initial_layout(vk::ImageLayout::UNDEFINED)
                .push_next(&mut external)
                .push_next(&mut list);
            let image = raw.create_image(&info, None).map_err(|e| format!("VkImage: {e}"))?;
            let requirements = raw.get_image_memory_requirements(image);
            let types = requirements.memory_type_bits;
            if types == 0 {
                raw.destroy_image(image, None);
                return Err("no memory type for an exportable image".into());
            }
            let mut export = vk::ExportMemoryAllocateInfo::default().handle_types(vk::ExternalMemoryHandleTypeFlags::DMA_BUF_EXT);
            let mut dedicated = vk::MemoryDedicatedAllocateInfo::default().image(image);
            let alloc = vk::MemoryAllocateInfo::default()
                .allocation_size(requirements.size)
                .memory_type_index(types.trailing_zeros())
                .push_next(&mut export)
                .push_next(&mut dedicated);
            let memory = match raw.allocate_memory(&alloc, None) {
                Ok(m) => m,
                Err(e) => {
                    raw.destroy_image(image, None);
                    return Err(format!("exportable memory: {e}"));
                }
            };
            let fail = |what: String| {
                raw.free_memory(memory, None);
                raw.destroy_image(image, None);
                what
            };
            raw.bind_image_memory(image, memory, 0).map_err(|e| fail(format!("binding the memory: {e}")))?;
            let fd_ext = ash::khr::external_memory_fd::Device::new(hal_device.shared_instance().raw_instance(), raw);
            let fd = fd_ext
                .get_memory_fd(&vk::MemoryGetFdInfoKHR::default().memory(memory).handle_type(vk::ExternalMemoryHandleTypeFlags::DMA_BUF_EXT))
                .map_err(|e| fail(format!("exporting the memory: {e}")))?;
            let fd = OwnedFd::from_raw_fd(fd);
            let layout = raw.get_image_subresource_layout(
                image,
                vk::ImageSubresource { aspect_mask: vk::ImageAspectFlags::MEMORY_PLANE_0_EXT, mip_level: 0, array_layer: 0 },
            );
            let (offset, stride) = (u32::try_from(layout.offset), u32::try_from(layout.row_pitch));
            let (Ok(offset), Ok(stride)) = (offset, stride) else { return Err(fail("an image layout past 4 GiB".into())) };
            let size = wgpu::Extent3d { width, height, depth_or_array_layers: 1 };
            let hal_desc = wgpu::hal::TextureDescriptor {
                label: Some("acde exported image"),
                size,
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba8Unorm,
                usage: hal_usage,
                memory_flags: wgpu::hal::MemoryFlags::empty(),
                view_formats: vec![],
            };
            let hal_texture = hal_device.texture_from_raw(image, &hal_desc, None, wgpu::hal::vulkan::TextureMemory::Dedicated(memory));
            drop(hal_device);
            let texture = device.create_texture_from_hal::<Vk>(
                hal_texture,
                &wgpu::TextureDescriptor {
                    label: Some("acde exported image"),
                    size,
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format: wgpu::TextureFormat::Rgba8Unorm,
                    usage,
                    view_formats: &[],
                },
                wgpu::TextureUses::UNINITIALIZED,
            );
            Ok((texture, Exported { fd, offset, stride }))
        }
    }
}
