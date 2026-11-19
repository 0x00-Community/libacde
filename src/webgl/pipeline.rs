use crate::webgl::FastMap;
use std::rc::Rc;

use crate::webgl::shader::Sampled;

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Binding {
    Texture { binding: u32, dim: wgpu::TextureViewDimension, sample: SampleKind, stages: u8 },
    Sampler { binding: u32, kind: SamplerKind, stages: u8 },
    Uniform { binding: u32, size: u32, stages: u8 },
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum SampleKind {
    Float,
    Unfilterable,
    Sint,
    Uint,
    Depth,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum SamplerKind {
    Filtering,
    NonFiltering,
    Comparison,
}

impl SampleKind {
    pub fn of(sampled: Sampled, filterable: bool) -> Self {
        match sampled {
            Sampled::Float if filterable => SampleKind::Float,
            Sampled::Float => SampleKind::Unfilterable,
            Sampled::Sint => SampleKind::Sint,
            Sampled::Uint => SampleKind::Uint,
            Sampled::Depth => SampleKind::Depth,
        }
    }
}

#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub struct VertexSlot {
    pub stride: u64,
    pub instance: bool,
    pub attributes: Vec<(u32, u64, wgpu::VertexFormat)>,
}

#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub struct PipelineKey {
    pub program: u64,
    pub layout: Vec<Binding>,
    pub vertex: Vec<VertexSlot>,
    pub topology: wgpu::PrimitiveTopology,
    pub strip: Option<wgpu::IndexFormat>,
    pub front_face: wgpu::FrontFace,
    pub cull: Option<wgpu::Face>,
    pub depth: Option<DepthKey>,
    pub samples: u32,
    pub alpha_to_coverage: bool,
    pub mask: u64,
    pub targets: Vec<Option<(wgpu::TextureFormat, Option<BlendKey>, u32)>>,
    pub discard: bool,
    pub points: bool,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct BlendKey {
    pub color: (wgpu::BlendFactor, wgpu::BlendFactor, wgpu::BlendOperation),
    pub alpha: (wgpu::BlendFactor, wgpu::BlendFactor, wgpu::BlendOperation),
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct DepthKey {
    pub format: wgpu::TextureFormat,
    pub write: bool,
    pub compare: wgpu::CompareFunction,
    pub stencil: [(wgpu::CompareFunction, wgpu::StencilOperation, wgpu::StencilOperation, wgpu::StencilOperation); 2],
    pub read_mask: u32,
    pub write_mask: u32,
    pub bias: (i32, u32, u32),
}

pub struct Caches {
    pub uniforms: wgpu::BindGroupLayout,
    pub layouts: FastMap<Vec<Binding>, Rc<(wgpu::BindGroupLayout, wgpu::PipelineLayout)>>,
    pub empty: Rc<(wgpu::BindGroupLayout, wgpu::PipelineLayout)>,
    pub pipelines: FastMap<PipelineKey, wgpu::RenderPipeline>,
    pub samplers: FastMap<SamplerKey, wgpu::Sampler>,
    pub groups: FastMap<GroupKey, wgpu::BindGroup>,
    pub helpers: FastMap<String, wgpu::RenderPipeline>,
    pub compute: FastMap<&'static str, (wgpu::ComputePipeline, wgpu::BindGroupLayout)>,
    pub dummies: FastMap<(wgpu::TextureViewDimension, SampleKind), (wgpu::Texture, wgpu::TextureView)>,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct SamplerKey {
    pub address: [wgpu::AddressMode; 3],
    pub mag: wgpu::FilterMode,
    pub min: wgpu::FilterMode,
    pub mip: wgpu::MipmapFilterMode,
    pub lod: (u32, u32),
    pub compare: Option<wgpu::CompareFunction>,
    pub anisotropy: u16,
}

#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub struct GroupKey {
    pub layout: Vec<Binding>,
    pub entries: Vec<(u64, u64, u64)>,
}

impl Caches {
    pub fn new(gpu: &crate::gpu::Gpu) -> Self {
        let device = &gpu.device;
        let entry = |binding| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
            ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: true, min_binding_size: None },
            count: None,
        };
        let uniforms = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor { label: Some("acde webgl uniforms"), entries: &[entry(0), entry(1)] });
        let empty_group = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor { label: Some("acde webgl resources"), entries: &[] });
        let empty_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("acde webgl"),
            bind_group_layouts: &[Some(&uniforms), Some(&empty_group)],
            immediate_size: 0,
        });
        Caches {
            uniforms,
            layouts: FastMap::default(),
            empty: Rc::new((empty_group, empty_layout)),
            pipelines: FastMap::default(),
            samplers: FastMap::default(),
            groups: FastMap::default(),
            helpers: FastMap::default(),
            compute: FastMap::default(),
            dummies: FastMap::default(),
        }
    }

    pub fn layout(&mut self, device: &wgpu::Device, bindings: &[Binding]) -> Rc<(wgpu::BindGroupLayout, wgpu::PipelineLayout)> {
        if bindings.is_empty() {
            return self.empty.clone();
        }
        if let Some(l) = self.layouts.get(bindings) {
            return l.clone();
        }
        let stages = |s: u8| {
            let mut v = wgpu::ShaderStages::NONE;
            if s & 1 != 0 {
                v |= wgpu::ShaderStages::VERTEX;
            }
            if s & 2 != 0 {
                v |= wgpu::ShaderStages::FRAGMENT;
            }
            v
        };
        let entries: Vec<wgpu::BindGroupLayoutEntry> = bindings
            .iter()
            .map(|b| match *b {
                Binding::Texture { binding, dim, sample, stages: s } => wgpu::BindGroupLayoutEntry {
                    binding,
                    visibility: stages(s),
                    ty: wgpu::BindingType::Texture {
                        sample_type: match sample {
                            SampleKind::Float => wgpu::TextureSampleType::Float { filterable: true },
                            SampleKind::Unfilterable => wgpu::TextureSampleType::Float { filterable: false },
                            SampleKind::Sint => wgpu::TextureSampleType::Sint,
                            SampleKind::Uint => wgpu::TextureSampleType::Uint,
                            SampleKind::Depth => wgpu::TextureSampleType::Depth,
                        },
                        view_dimension: dim,
                        multisampled: false,
                    },
                    count: None,
                },
                Binding::Sampler { binding, kind, stages: s } => wgpu::BindGroupLayoutEntry {
                    binding,
                    visibility: stages(s),
                    ty: wgpu::BindingType::Sampler(match kind {
                        SamplerKind::Filtering => wgpu::SamplerBindingType::Filtering,
                        SamplerKind::NonFiltering => wgpu::SamplerBindingType::NonFiltering,
                        SamplerKind::Comparison => wgpu::SamplerBindingType::Comparison,
                    }),
                    count: None,
                },
                Binding::Uniform { binding, size, stages: s } => wgpu::BindGroupLayoutEntry {
                    binding,
                    visibility: stages(s),
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: wgpu::BufferSize::new(u64::from(size)),
                    },
                    count: None,
                },
            })
            .collect();
        let group = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor { label: Some("acde webgl resources"), entries: &entries });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("acde webgl"),
            bind_group_layouts: &[Some(&self.uniforms), Some(&group)],
            immediate_size: 0,
        });
        let l = Rc::new((group, layout));
        self.layouts.insert(bindings.to_vec(), l.clone());
        l
    }

    pub fn sampler(&mut self, device: &wgpu::Device, key: SamplerKey) -> wgpu::Sampler {
        self.samplers
            .entry(key)
            .or_insert_with(|| {
                device.create_sampler(&wgpu::SamplerDescriptor {
                    label: Some("acde webgl"),
                    address_mode_u: key.address[0],
                    address_mode_v: key.address[1],
                    address_mode_w: key.address[2],
                    mag_filter: key.mag,
                    min_filter: key.min,
                    mipmap_filter: key.mip,
                    lod_min_clamp: key.lod.0 as f32 / 256.0,
                    lod_max_clamp: key.lod.1 as f32 / 256.0,
                    compare: key.compare,
                    anisotropy_clamp: key.anisotropy,
                    border_color: None,
                })
            })
            .clone()
    }
}
