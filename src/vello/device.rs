use std::os::fd::{AsFd, AsRawFd, FromRawFd, IntoRawFd, OwnedFd};

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

#[derive(Clone, Copy)]
pub struct Fence(vk::Semaphore);

pub struct Device {
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
    pub info: wgpu::AdapterInfo,
    semaphores: Vec<vk::Semaphore>,
    next: usize,
}

impl Device {
    pub fn open() -> Result<Self, String> {
        Self::open_where(true)
    }

    pub fn open_hardware() -> Result<Self, String> {
        Self::open_where(false)
    }

    fn open_where(cpu: bool) -> Result<Self, String> {
        let mut errors = Vec::new();
        let instance =
            wgpu::Instance::new(wgpu::InstanceDescriptor { backends: wgpu::Backends::VULKAN, ..wgpu::InstanceDescriptor::new_without_display_handle() });
        let mut adapters: Vec<_> = pollster::block_on(instance.enumerate_adapters(wgpu::Backends::VULKAN))
            .into_iter()
            .filter(|a| cpu || a.get_info().device_type != wgpu::DeviceType::Cpu)
            .collect();
        adapters.sort_by_key(|a| match a.get_info().device_type {
            wgpu::DeviceType::DiscreteGpu | wgpu::DeviceType::IntegratedGpu => 0,
            wgpu::DeviceType::Cpu => 2,
            _ => 1,
        });
        let (adapter, device, queue) = adapters
            .into_iter()
            .find_map(|adapter| match Self::open_adapter(&adapter) {
                Ok((device, queue)) => Some((adapter, device, queue)),
                Err(e) => {
                    errors.push(format!("{}: {e}", adapter.get_info().name));
                    None
                }
            })
            .ok_or_else(|| if errors.is_empty() { "no GPU (Vulkan)".to_string() } else { errors.join("; ") })?;
        let mut device = Self { device, queue, info: adapter.get_info(), semaphores: Vec::new(), next: 0 };
        device.semaphores = device.create_semaphores();
        Ok(device)
    }

    fn open_adapter(adapter: &wgpu::Adapter) -> Result<(wgpu::Device, wgpu::Queue), String> {
        let hal_adapter = unsafe { adapter.as_hal::<Vk>() }.ok_or("not a Vulkan adapter")?;
        let caps = hal_adapter.physical_device_capabilities();
        let missing: Vec<_> = EXTENSIONS.iter().filter(|e| !caps.supports_extension(e)).map(|e| e.to_string_lossy()).collect();
        if !missing.is_empty() {
            return Err(format!("missing Vulkan extensions {}", missing.join(", ")));
        }
        drop(hal_adapter);
        let tiers = DeviceRequest::tiers(adapter.features() & wgpu::Features::all_webgpu_mask(), adapter.limits());
        DeviceRequest::open("acde", &tiers, |request| Self::open_tier(adapter, request)).map(|(device, _)| device)
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
