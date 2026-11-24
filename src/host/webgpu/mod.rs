mod canvas;
mod desc;
mod pass;

use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::mpsc::Sender;

use crate::gpu::Gpu;
use crate::gpu::wgpu;

use crate::host::bind;
use crate::host::rt::{self, Msg};

pub(crate) type Id = u32;

pub(crate) enum Obj {
    Device(Device),
    Buffer(Buffer),
    Texture(Texture),
    View(View),
    Sampler(wgpu::Sampler),
    BindGroupLayout(wgpu::BindGroupLayout),
    PipelineLayout(wgpu::PipelineLayout),
    BindGroup(BindGroup),
    Shader(Shader),
    RenderPipeline(wgpu::RenderPipeline),
    ComputePipeline(wgpu::ComputePipeline),
    Encoder(Option<wgpu::CommandEncoder>),
    CommandBuffer(Option<wgpu::CommandBuffer>),
    RenderBundle(wgpu::RenderBundle),
    QuerySet(QuerySet),
}

pub(crate) struct Device {
    on_error: v8::Global<v8::Function>,
    on_lost: v8::Global<v8::Function>,
}

pub(crate) struct Buffer {
    pub buffer: wgpu::Buffer,
    device: Id,
    map: Map,
    request: u32,
}

#[derive(Clone, Copy, PartialEq)]
enum Map {
    Unmapped,
    Pending,
    Mapped { write: bool, created: bool, offset: u64, size: u64 },
}

pub(crate) struct Texture {
    pub texture: wgpu::Texture,
    device: Id,
    pub canvas: Option<Id>,
}

pub(crate) struct View {
    pub view: wgpu::TextureView,
    pub texture: Id,
}

pub(crate) struct BindGroup {
    pub group: wgpu::BindGroup,
    pub canvas_textures: Vec<Id>,
}

pub(crate) struct QuerySet {
    set: wgpu::QuerySet,
    device: Id,
}

pub(crate) struct Shader {
    module: wgpu::ShaderModule,
    messages: Vec<wgpu::CompilationMessage>,
}

#[derive(Default)]
pub(crate) struct State {
    gpu: Option<Option<Arc<Gpu>>>,
    pub objects: HashMap<Id, Obj>,
    next: Id,
    adapter_waiting: Vec<v8::Global<v8::PromiseResolver>>,
    lost_listener: Option<u64>,
    pub canvases: HashMap<Id, canvas::Context>,
}

impl State {
    pub fn id(&mut self) -> Id {
        self.next += 1;
        self.next
    }

    pub fn insert(&mut self, obj: Obj) -> Id {
        let id = self.id();
        self.objects.insert(id, obj);
        id
    }

    pub fn gpu(&self) -> Result<Arc<Gpu>, Throw> {
        self.gpu.clone().flatten().ok_or_else(|| Throw::Error("OperationError: no GPU adapter".into()))
    }
}

macro_rules! getters {
    ($($name:ident: $variant:ident => $ty:ty, $what:literal;)*) => {
        impl State {
            $(
                pub fn $name(&self, device: Id, id: Id) -> Option<&$ty> {
                    match self.objects.get(&id) {
                        Some(Obj::$variant(x)) => Some(x),
                        _ => {
                            report(device, Kind::Validation, format!(concat!("invalid ", $what, " ({})"), id));
                            None
                        }
                    }
                }
            )*
        }
    };
}

getters! {
    buffer: Buffer => Buffer, "GPUBuffer";
    texture: Texture => Texture, "GPUTexture";
    view: View => View, "GPUTextureView";
    sampler: Sampler => wgpu::Sampler, "GPUSampler";
    bind_group_layout: BindGroupLayout => wgpu::BindGroupLayout, "GPUBindGroupLayout";
    pipeline_layout: PipelineLayout => wgpu::PipelineLayout, "GPUPipelineLayout";
    bind_group: BindGroup => BindGroup, "GPUBindGroup";
    shader: Shader => Shader, "GPUShaderModule";
    render_pipeline: RenderPipeline => wgpu::RenderPipeline, "GPURenderPipeline";
    compute_pipeline: ComputePipeline => wgpu::ComputePipeline, "GPUComputePipeline";
    render_bundle: RenderBundle => wgpu::RenderBundle, "GPURenderBundle";
    query_set: QuerySet => QuerySet, "GPUQuerySet";
}

thread_local! {
    static STATE: RefCell<State> = RefCell::new(State::default());
    static ERRORS: RefCell<Errors> = RefCell::new(Errors::default());
}

pub(crate) fn with_state<R>(f: impl FnOnce(&mut State) -> R) -> R {
    STATE.with(|s| f(&mut s.borrow_mut()))
}

#[derive(Clone, Copy, PartialEq)]
pub(crate) enum Kind {
    Validation,
    OutOfMemory,
    Internal,
}

impl Kind {
    fn name(self) -> &'static str {
        match self {
            Kind::Validation => "validation",
            Kind::OutOfMemory => "out-of-memory",
            Kind::Internal => "internal",
        }
    }
}

pub(crate) struct ErrorInfo {
    kind: Kind,
    message: String,
}

#[derive(Default)]
struct DeviceErrors {
    scopes: Vec<(Kind, Option<ErrorInfo>)>,
    lost: bool,
}

#[derive(Default)]
struct Errors {
    current: Id,
    devices: HashMap<Id, DeviceErrors>,
    uncaptured: Vec<(Id, ErrorInfo)>,
    tx: Option<Sender<Msg>>,
    capture: Option<Option<ErrorInfo>>,
}

pub(crate) enum Throw {
    Type(String),
    Error(String),
}

impl Throw {
    fn from_message(message: String) -> Self {
        match message.strip_prefix("TypeError: ") {
            Some(rest) => Throw::Type(rest.to_owned()),
            None => Throw::Error(message),
        }
    }
}

fn enter(device: Id) -> bool {
    ERRORS.with(|e| e.borrow_mut().current = device);
    alive(device)
}

fn alive(device: Id) -> bool {
    ERRORS.with(|e| e.borrow().devices.get(&device).is_some_and(|d| !d.lost))
}

fn live(device: Id) -> Result<Option<Arc<Gpu>>, Throw> {
    if !enter(device) {
        return Ok(None);
    }
    with_state(|s| s.gpu()).map(Some)
}

pub(crate) fn maybe<T>(id: Option<Id>, get: impl FnOnce(Id) -> Option<T>) -> Option<Option<T>> {
    id.map_or(Some(None), |id| get(id).map(Some))
}

pub(crate) fn current_device() -> Id {
    ERRORS.with(|e| e.borrow().current)
}

pub(crate) fn report(device: Id, kind: Kind, message: String) {
    ERRORS.with(|e| {
        let mut e = e.borrow_mut();
        if let Some(capture) = e.capture.as_mut() {
            capture.get_or_insert(ErrorInfo { kind, message });
            return;
        }
        let Some(errors) = e.devices.get_mut(&device) else { return };
        if errors.lost {
            return;
        }
        if let Some((_, caught)) = errors.scopes.iter_mut().rev().find(|(filter, _)| *filter == kind) {
            caught.get_or_insert(ErrorInfo { kind, message });
            return;
        }
        let first = e.uncaptured.is_empty();
        e.uncaptured.push((device, ErrorInfo { kind, message }));
        if first && let Some(tx) = &e.tx {
            _ = tx.send(Msg::Task(Box::new(dispatch_errors)));
        }
    });
}

fn sink(error: wgpu::Error) {
    let (kind, message) = match error {
        wgpu::Error::OutOfMemory { .. } => (Kind::OutOfMemory, "out of memory".to_owned()),
        wgpu::Error::Validation { description, .. } => (Kind::Validation, description),
        wgpu::Error::Internal { description, .. } => (Kind::Internal, description),
    };
    report(current_device(), kind, message);
}

fn dispatch_errors(scope: &mut v8::PinScope) {
    let errors = ERRORS.with(|e| std::mem::take(&mut e.borrow_mut().uncaptured));
    for (device, error) in errors {
        let Some(callback) = STATE.with(|s| match s.borrow().objects.get(&device) {
            Some(Obj::Device(d)) if alive(device) => Some(d.on_error.clone()),
            _ => None,
        }) else {
            continue;
        };
        let kind = bind::string(scope, error.kind.name());
        let message = bind::string(scope, &error.message);
        bind::call(scope, callback, &[kind.into(), message.into()]);
    }
}

fn capturing<R>(f: impl FnOnce() -> R) -> (R, Option<ErrorInfo>) {
    ERRORS.with(|e| e.borrow_mut().capture = Some(None));
    let result = f();
    let error = ERRORS.with(|e| e.borrow_mut().capture.take().flatten());
    (result, error)
}

pub(crate) enum Ret<'s> {
    Undefined,
    Num(f64),
    Val(v8::Local<'s, v8::Value>),
}

impl From<()> for Ret<'_> {
    fn from(_: ()) -> Self {
        Ret::Undefined
    }
}

impl From<Id> for Ret<'_> {
    fn from(id: Id) -> Self {
        Ret::Num(id.into())
    }
}

type Out<'s> = Result<Ret<'s>, Throw>;

macro_rules! natives {
    (methods = $methods:ident; $($js:literal => fn $name:ident($scope:ident, $args:ident) $body:block)*) => {
        $(
            fn $name<'s>($scope: &mut v8::PinScope<'s, '_>, $args: v8::FunctionCallbackArguments<'s>, mut rv: v8::ReturnValue) {
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| -> Out<'s> { $body }));
                finish($scope, &mut rv, result);
            }
        )*
        pub(crate) fn $methods(scope: &mut v8::PinScope, obj: v8::Local<v8::Object>) {
            $( crate::host::bind::method(scope, obj, $js, $name); )*
        }
    };
}
pub(crate) use natives;

pub(crate) fn finish<'s>(scope: &mut v8::PinScope<'s, '_>, rv: &mut v8::ReturnValue, result: std::thread::Result<Out<'s>>) {
    match result {
        Ok(Ok(Ret::Undefined)) => {}
        Ok(Ok(Ret::Num(n))) => rv.set_double(n),
        Ok(Ok(Ret::Val(v))) => rv.set(v),
        Ok(Err(Throw::Type(message))) => bind::throw_type_error(scope, &message),
        Ok(Err(Throw::Error(message))) => bind::throw_error(scope, &message),
        Err(panic) => {
            let message = panic.downcast_ref::<String>().cloned().or_else(|| panic.downcast_ref::<&str>().map(|s| s.to_string())).unwrap_or_default();
            report(current_device(), Kind::Internal, format!("libacde: {message}"));
        }
    }
}

pub(crate) fn json<T: serde::de::DeserializeOwned>(scope: &mut v8::PinScope, args: &v8::FunctionCallbackArguments, i: i32) -> Result<T, Throw> {
    let text = bind::arg_string(scope, args, i);
    serde_json::from_str(&text).map_err(|e| Throw::Type(e.to_string()))
}

fn promise<'s>(scope: &mut v8::PinScope<'s, '_>) -> Option<(u32, v8::Local<'s, v8::Value>)> {
    bind::promise(scope).map(|(token, promise)| (token, promise.into()))
}

fn settle(scope: &mut v8::PinScope, token: u32, value: Option<&str>) {
    resolve(scope, token, |scope| match value {
        Some(text) => bind::string(scope, text).into(),
        None => v8::null(scope).into(),
    });
}

fn resolve(scope: &mut v8::PinScope, token: u32, value: impl for<'s> FnOnce(&mut v8::PinScope<'s, '_>) -> v8::Local<'s, v8::Value>) {
    let Some(resolver) = bind::resolver(scope, token) else { return };
    let value = value(scope);
    resolver.resolve(scope, value);
}

pub fn install(scope: &mut v8::PinScope, natives: v8::Local<v8::Object>) {
    let gpu = v8::Object::new(scope);
    methods(scope, gpu);
    canvas::methods(scope, gpu);
    bind::set(scope, natives, "gpu", gpu.into());
}

pub fn clear() {
    let state = STATE.with(|s| std::mem::take(&mut *s.borrow_mut()));
    if let (Some(Some(gpu)), Some(listener)) = (&state.gpu, state.lost_listener) {
        gpu.forget_lost(listener);
    }
    drop(state);
    ERRORS.with(|e| *e.borrow_mut() = Errors::default());
    crate::gpu::set_error_sink(None);
}

fn adapter_ready(scope: &mut v8::PinScope, gpu: Option<Arc<Gpu>>) {
    let lost = gpu.as_ref().is_some_and(|gpu| gpu.gone());
    let waiting = with_state(|s| {
        if s.gpu.is_none() && !lost {
            s.gpu = Some(gpu.clone());
            if let Some(gpu) = &gpu {
                let tx = rt::current().sender();
                ERRORS.with(|e| e.borrow_mut().tx = Some(tx.clone()));
                crate::gpu::set_error_sink(Some(Box::new(sink)));
                s.lost_listener = Some(gpu.on_lost(move |message| {
                    let message = message.to_owned();
                    _ = tx.send(Msg::Task(Box::new(move |scope| all_lost(scope, &message))));
                }));
            }
        }
        std::mem::take(&mut s.adapter_waiting)
    });
    let info = adapter_info(scope);
    for resolver in waiting {
        v8::Local::new(scope, resolver).resolve(scope, info);
    }
}

/// After a device loss the page lets go of the lost device, so its next requestAdapter() finds the host window's new one.
fn let_go_of_lost_gpu() {
    let Some(Some(held)) = with_state(|s| s.gpu.clone()) else { return };
    if !held.gone() {
        return;
    }
    let listener = with_state(|s| {
        s.gpu = None;
        s.lost_listener.take()
    });
    if let Some(listener) = listener {
        held.forget_lost(listener);
    }
}

/// Whether an adapter can be given now: not while the published device is the lost one (the host window is opening a new
/// device); requestAdapter() then resolves null and the page asks again, as when a browser has no adapter.
fn follow_replacement() -> bool {
    let_go_of_lost_gpu();
    !Gpu::get().flatten().is_some_and(|now| now.gone())
}

fn all_lost(scope: &mut v8::PinScope, message: &str) {
    let_go_of_lost_gpu();
    let devices: Vec<Id> = ERRORS.with(|e| {
        e.borrow_mut()
            .devices
            .iter_mut()
            .filter(|(_, d)| !d.lost)
            .map(|(&id, d)| {
                d.lost = true;
                id
            })
            .collect()
    });
    let callbacks: Vec<_> = with_state(|s| {
        devices
            .iter()
            .filter_map(|id| match s.objects.get(id) {
                Some(Obj::Device(d)) => Some(d.on_lost.clone()),
                _ => None,
            })
            .collect()
    });
    for callback in callbacks {
        let reason = bind::string(scope, "unknown");
        let message = bind::string(scope, message);
        bind::call(scope, callback, &[reason.into(), message.into()]);
    }
}

fn adapter_info<'s>(scope: &mut v8::PinScope<'s, '_>) -> v8::Local<'s, v8::Value> {
    let Some(Some(gpu)) = with_state(|s| s.gpu.clone()) else { return v8::null(scope).into() };
    let vendor = match gpu.info.vendor {
        0x8086 => "intel",
        0x1002 | 0x1022 => "amd",
        0x10de => "nvidia",
        0x13b5 => "arm",
        0x5143 => "qualcomm",
        0x1010 => "imgtec",
        0x10005 => "mesa",
        _ => "",
    };
    let features: Vec<&str> = (gpu.device.features() & wgpu::Features::all_webgpu_mask()).iter().filter_map(|f| f.as_str()).collect();
    let limits: serde_json::Map<String, serde_json::Value> =
        limit_values(&gpu.device.limits()).into_iter().map(|(name, value)| (name.into(), value.into())).collect();
    crate::host::offload::json(
        scope,
        &serde_json::json!({
            "vendor": vendor,
            "architecture": "",
            "device": "",
            "description": format!("{} ({} {})", gpu.info.name, gpu.info.driver, gpu.info.driver_info),
            "isFallbackAdapter": gpu.info.device_type == wgpu::DeviceType::Cpu,
            "features": features,
            "limits": limits,
        }),
    )
}

fn limit_values(l: &wgpu::Limits) -> Vec<(&'static str, f64)> {
    macro_rules! limits {
        ($($name:literal => $field:ident),* $(,)?) => { vec![$(($name, l.$field as f64)),*] };
    }
    let mut values = limits! {
        "maxTextureDimension1D" => max_texture_dimension_1d,
        "maxTextureDimension2D" => max_texture_dimension_2d,
        "maxTextureDimension3D" => max_texture_dimension_3d,
        "maxTextureArrayLayers" => max_texture_array_layers,
        "maxBindGroups" => max_bind_groups,
        "maxBindingsPerBindGroup" => max_bindings_per_bind_group,
        "maxDynamicUniformBuffersPerPipelineLayout" => max_dynamic_uniform_buffers_per_pipeline_layout,
        "maxDynamicStorageBuffersPerPipelineLayout" => max_dynamic_storage_buffers_per_pipeline_layout,
        "maxSampledTexturesPerShaderStage" => max_sampled_textures_per_shader_stage,
        "maxSamplersPerShaderStage" => max_samplers_per_shader_stage,
        "maxStorageBuffersPerShaderStage" => max_storage_buffers_per_shader_stage,
        "maxStorageTexturesPerShaderStage" => max_storage_textures_per_shader_stage,
        "maxUniformBuffersPerShaderStage" => max_uniform_buffers_per_shader_stage,
        "maxUniformBufferBindingSize" => max_uniform_buffer_binding_size,
        "maxStorageBufferBindingSize" => max_storage_buffer_binding_size,
        "minUniformBufferOffsetAlignment" => min_uniform_buffer_offset_alignment,
        "minStorageBufferOffsetAlignment" => min_storage_buffer_offset_alignment,
        "maxVertexBuffers" => max_vertex_buffers,
        "maxBufferSize" => max_buffer_size,
        "maxVertexAttributes" => max_vertex_attributes,
        "maxVertexBufferArrayStride" => max_vertex_buffer_array_stride,
        "maxInterStageShaderVariables" => max_inter_stage_shader_variables,
        "maxColorAttachments" => max_color_attachments,
        "maxColorAttachmentBytesPerSample" => max_color_attachment_bytes_per_sample,
        "maxComputeWorkgroupStorageSize" => max_compute_workgroup_storage_size,
        "maxComputeInvocationsPerWorkgroup" => max_compute_invocations_per_workgroup,
        "maxComputeWorkgroupSizeX" => max_compute_workgroup_size_x,
        "maxComputeWorkgroupSizeY" => max_compute_workgroup_size_y,
        "maxComputeWorkgroupSizeZ" => max_compute_workgroup_size_z,
        "maxComputeWorkgroupsPerDimension" => max_compute_workgroups_per_dimension,
    };
    values.push(("maxBindGroupsPlusVertexBuffers", f64::from(l.max_bind_groups + l.max_vertex_buffers)));
    values.push(("maxStorageBuffersInVertexStage", f64::from(l.max_storage_buffers_per_shader_stage)));
    values.push(("maxStorageBuffersInFragmentStage", f64::from(l.max_storage_buffers_per_shader_stage)));
    values.push(("maxStorageTexturesInVertexStage", f64::from(l.max_storage_textures_per_shader_stage)));
    values.push(("maxStorageTexturesInFragmentStage", f64::from(l.max_storage_textures_per_shader_stage)));
    values
}

natives! {
    methods = methods;

    "requestAdapter" => fn request_adapter(scope, _args) {
        let Some(resolver) = v8::PromiseResolver::new(scope) else { return Ok(().into()) };
        let promise = resolver.get_promise(scope);
        if !follow_replacement() {
            let none = v8::null(scope);
            resolver.resolve(scope, none.into());
            return Ok(Ret::Val(promise.into()));
        }
        let known = with_state(|s| s.gpu.is_some());
        if known {
            let info = adapter_info(scope);
            resolver.resolve(scope, info);
            return Ok(Ret::Val(promise.into()));
        }
        let global = v8::Global::new(scope, resolver);
        let first = with_state(|s| {
            s.adapter_waiting.push(global);
            s.adapter_waiting.len() == 1
        });
        if first {
            let tx = rt::current().sender();
            let late = tx.clone();
            Gpu::when_ready(move |gpu| _ = tx.send(Msg::Task(Box::new(move |scope| adapter_ready(scope, gpu)))));
            _ = std::thread::Builder::new().name("acde-gpu-wait".into()).spawn(move || {
                std::thread::sleep(std::time::Duration::from_secs(15));
                if Gpu::get().is_none() {
                    _ = late.send(Msg::Task(Box::new(|scope| adapter_ready(scope, None))));
                }
            });
        }
        Ok(Ret::Val(promise.into()))
    }

    "createDevice" => fn create_device(scope, args) {
        let gpu = with_state(|s| s.gpu())?;
        let features = gpu.device.features();
        if let Ok(list) = v8::Local::<v8::Array>::try_from(args.get(0)) {
            for i in 0..list.length() {
                let name = list.get_index(scope, i).map(|v| v.to_rust_string_lossy(scope)).unwrap_or_default();
                let wanted: Result<wgpu::Features, _> = name.parse();
                if !wanted.is_ok_and(|f| features.contains(f) && wgpu::Features::all_webgpu_mask().contains(f)) {
                    return Err(Throw::Type(format!("the adapter has no feature '{name}'")));
                }
            }
        }
        let limits = limit_values(&gpu.device.limits());
        if let Ok(asked) = v8::Local::<v8::Object>::try_from(args.get(1))
            && let Some(keys) = asked.get_own_property_names(scope, Default::default())
        {
            for i in 0..keys.length() {
                let Some(key) = keys.get_index(scope, i) else { continue };
                let name = key.to_rust_string_lossy(scope);
                let value = asked.get(scope, key).and_then(|v| v.number_value(scope)).unwrap_or(f64::NAN);
                if value.is_nan() {
                    continue;
                }
                let Some((_, have)) = limits.iter().find(|(n, _)| *n == name) else {
                    return Err(Throw::Error(format!("OperationError: unknown limit '{name}'")));
                };
                let better = if name.starts_with("min") { value < *have } else { value > *have };
                if better {
                    return Err(Throw::Error(format!("OperationError: limit {name} {value} is beyond the adapter's {have}")));
                }
            }
        }
        let (Ok(on_error), Ok(on_lost)) = (v8::Local::<v8::Function>::try_from(args.get(2)), v8::Local::<v8::Function>::try_from(args.get(3))) else {
            return Err(Throw::Type("createDevice: callbacks".into()));
        };
        let device = Device { on_error: v8::Global::new(scope, on_error), on_lost: v8::Global::new(scope, on_lost) };
        let id = with_state(|s| s.insert(Obj::Device(device)));
        ERRORS.with(|e| e.borrow_mut().devices.insert(id, DeviceErrors::default()));
        Ok(id.into())
    }

    "destroyDevice" => fn destroy_device(scope, args) {
        let device = bind::arg_u32(scope, &args, 0);
        with_state(|s| {
            for obj in s.objects.values() {
                match obj {
                    Obj::Buffer(b) if b.device == device => b.buffer.destroy(),
                    Obj::Texture(t) if t.device == device && t.canvas.is_none() => t.texture.destroy(),
                    Obj::QuerySet(q) if q.device == device => q.set.destroy(),
                    _ => {}
                }
            }
        });
        ERRORS.with(|e| {
            if let Some(d) = e.borrow_mut().devices.get_mut(&device) {
                d.lost = true;
            }
        });
        Ok(().into())
    }

    "pushErrorScope" => fn push_error_scope(scope, args) {
        let device = bind::arg_u32(scope, &args, 0);
        let filter = match bind::arg_string(scope, &args, 1).as_str() {
            "validation" => Kind::Validation,
            "out-of-memory" => Kind::OutOfMemory,
            "internal" => Kind::Internal,
            other => return Err(Throw::Type(format!("'{other}' is not a GPUErrorFilter"))),
        };
        ERRORS.with(|e| {
            if let Some(d) = e.borrow_mut().devices.get_mut(&device) {
                d.scopes.push((filter, None));
            }
        });
        Ok(().into())
    }

    "popErrorScope" => fn pop_error_scope(scope, args) {
        let device = bind::arg_u32(scope, &args, 0);
        let popped = ERRORS.with(|e| e.borrow_mut().devices.get_mut(&device).map(|d| (d.lost, d.scopes.pop())));
        Ok(match popped {
            Some((true, _)) => Ret::Val(v8::null(scope).into()),
            Some((false, Some((_, None)))) => Ret::Val(v8::null(scope).into()),
            Some((false, Some((_, Some(error))))) => Ret::Val(crate::host::offload::json(scope, &serde_json::json!({"kind": error.kind.name(), "message": error.message}))),
            _ => Ret::Undefined,
        })
    }

    "drop" => fn drop_object(scope, args) {
        let id = bind::arg_u32(scope, &args, 0);
        let obj = with_state(|s| s.objects.remove(&id));
        if matches!(obj, Some(Obj::Device(_))) {
            ERRORS.with(|e| e.borrow_mut().devices.remove(&id));
        }
        drop(obj);
        Ok(().into())
    }

    "createBuffer" => fn create_buffer(scope, args) {
        let device = bind::arg_u32(scope, &args, 0);
        let desc: desc::BufferDesc = json(scope, &args, 1)?;
        let Some(gpu) = live(device)? else { return Ok(0.into()) };
        let Some(usage) = wgpu::BufferUsages::from_bits(desc.usage) else {
            report(device, Kind::Validation, format!("invalid GPUBufferUsage {:#x}", desc.usage));
            return Ok(0.into());
        };
        let buffer = gpu.device.create_buffer(&wgpu::BufferDescriptor { label: desc::label(&desc.label), size: desc.size, usage, mapped_at_creation: desc.mapped_at_creation });
        let map = if desc.mapped_at_creation { Map::Mapped { write: true, created: true, offset: 0, size: desc.size } } else { Map::Unmapped };
        Ok(with_state(|s| s.insert(Obj::Buffer(Buffer { buffer, device, map, request: 0 }))).into())
    }

    "destroyBuffer" => fn destroy_buffer(scope, args) {
        let (device, id) = (bind::arg_u32(scope, &args, 0), bind::arg_u32(scope, &args, 1));
        enter(device);
        with_state(|s| {
            if let Some(Obj::Buffer(b)) = s.objects.get_mut(&id) {
                b.buffer.destroy();
                b.map = Map::Unmapped;
            }
        });
        Ok(().into())
    }

    "mapAsync" => fn map_async(scope, args) {
        let (device, id, mode) = (bind::arg_u32(scope, &args, 0), bind::arg_u32(scope, &args, 1), bind::arg_u32(scope, &args, 2));
        let (offset, size) = (bind::arg_u64(scope, &args, 3), bind::arg_u64(scope, &args, 4));
        let Some((token, promise)) = promise(scope) else { return Ok(().into()) };
        let tx = rt::current().sender();
        let failed = |scope: &mut v8::PinScope, message: &str| {
            settle(scope, token, Some(&format!("OperationError: {message}")));
        };
        let Some(gpu) = live(device)? else {
            failed(scope, "the device is lost");
            return Ok(Ret::Val(promise));
        };
        let problem = with_state(|s| {
            let Some(Obj::Buffer(b)) = s.objects.get_mut(&id) else { return Some("invalid GPUBuffer".to_owned()) };
            if b.map != Map::Unmapped {
                return Some("the buffer is not unmapped".into());
            }
            let write = match mode {
                1 => false,
                2 => true,
                _ => return Some(format!("invalid GPUMapMode {mode}")),
            };
            if offset % 8 != 0 || size % 4 != 0 || offset.checked_add(size).is_none_or(|end| end > b.buffer.size()) {
                return Some(format!("map range {offset}+{size} is not inside the buffer or not aligned"));
            }
            b.map = Map::Pending;
            b.request += 1;
            let request = b.request;
            let held = gpu.clone();
            held.hold_poll();
            let mode = if write { wgpu::MapMode::Write } else { wgpu::MapMode::Read };
            b.buffer.slice(offset..offset + size).map_async(mode, move |result| {
                let ok = result.is_ok();
                _ = tx.send(Msg::Task(Box::new(move |scope| map_done(scope, token, id, request, ok, write, offset, size))));
                held.release_poll();
            });
            None
        });
        if let Some(problem) = problem {
            report(device, Kind::Validation, problem.clone());
            failed(scope, &problem);
        }
        Ok(Ret::Val(promise))
    }

    "getMappedRange" => fn get_mapped_range(scope, args) {
        let id = bind::arg_u32(scope, &args, 0);
        let (offset, size) = (bind::arg_u64(scope, &args, 1), bind::arg_u64(scope, &args, 2));
        let bytes = with_state(|s| -> Result<Option<Vec<u8>>, Throw> {
            let Some(Obj::Buffer(b)) = s.objects.get(&id) else { return Err(Throw::Error("OperationError: invalid GPUBuffer".into())) };
            let Map::Mapped { write, created, offset: at, size: len } = b.map else {
                return Err(Throw::Error("OperationError: the buffer is not mapped".into()));
            };
            if offset % 8 != 0 || size % 4 != 0 || offset < at || offset.checked_add(size).is_none_or(|end| end > at + len) {
                return Err(Throw::Error(format!("OperationError: range {offset}+{size} is outside the mapping")));
            }
            if created || size == 0 {
                return Ok(None);
            }
            let slice = b.buffer.slice(offset..offset + size);
            Ok(Some(if write {
                let mut view = slice.get_mapped_range_mut().map_err(|e| Throw::Error(format!("OperationError: {e}")))?;
                let mut range = view.slice(..);
                let ptr = range.as_raw_ptr();
                unsafe { std::slice::from_raw_parts(ptr.as_ptr() as *const u8, size as usize) }.to_vec()
            } else {
                slice.get_mapped_range().map_err(|e| Throw::Error(format!("OperationError: {e}")))?.to_vec()
            }))
        })?;
        let buffer = match bytes {
            Some(bytes) => {
                let store = v8::ArrayBuffer::new_backing_store_from_vec(bytes).make_shared();
                v8::ArrayBuffer::with_backing_store(scope, &store)
            }
            None => v8::ArrayBuffer::new(scope, size as usize),
        };
        Ok(Ret::Val(buffer.into()))
    }

    "unmap" => fn unmap(scope, args) {
        let (device, id) = (bind::arg_u32(scope, &args, 0), bind::arg_u32(scope, &args, 1));
        enter(device);
        let mut written: Vec<(u64, v8::Local<v8::ArrayBuffer>)> = Vec::new();
        if let Ok(list) = v8::Local::<v8::Array>::try_from(args.get(2)) {
            let mut i = 0;
            while i + 1 < list.length() {
                let offset = list.get_index(scope, i).and_then(|v| v.number_value(scope)).unwrap_or(0.0) as u64;
                if let Some(buffer) = list.get_index(scope, i + 1).and_then(|v| v8::Local::<v8::ArrayBuffer>::try_from(v).ok()) {
                    written.push((offset, buffer));
                }
                i += 2;
            }
        }
        with_state(|s| {
            let Some(Obj::Buffer(b)) = s.objects.get_mut(&id) else { return };
            match b.map {
                Map::Mapped { write, .. } => {
                    if write {
                        for (offset, data) in &written {
                            let len = data.byte_length() as u64;
                            if len == 0 {
                                continue;
                            }
                            if let Ok(mut view) = b.buffer.slice(*offset..*offset + len).get_mapped_range_mut()
                                && let Some(ptr) = data.data()
                            {
                                view.slice(..).copy_from_slice(unsafe { std::slice::from_raw_parts(ptr.as_ptr() as *const u8, len as usize) });
                            }
                        }
                    }
                    b.buffer.unmap();
                }
                Map::Pending => b.buffer.unmap(),
                Map::Unmapped => {}
            }
            b.map = Map::Unmapped;
        });
        for (_, data) in written {
            _ = data.detach(None);
        }
        Ok(().into())
    }

    "writeBuffer" => fn write_buffer(scope, args) {
        let (device, id, offset) = (bind::arg_u32(scope, &args, 0), bind::arg_u32(scope, &args, 1), bind::arg_u64(scope, &args, 2));
        let Some(gpu) = live(device)? else { return Ok(().into()) };
        let data = args.get(3);
        with_state(|s| {
            let Some(b) = s.buffer(device, id) else { return };
            bind::with_bytes(data, |bytes| gpu.queue.write_buffer(&b.buffer, offset, bytes));
        });
        Ok(().into())
    }

    "createTexture" => fn create_texture(scope, args) {
        let device = bind::arg_u32(scope, &args, 0);
        let desc: desc::TextureDesc = json(scope, &args, 1)?;
        let Some(gpu) = live(device)? else { return Ok(0.into()) };
        let Some(usage) = wgpu::TextureUsages::from_bits(desc.usage) else {
            report(device, Kind::Validation, format!("invalid GPUTextureUsage {:#x}", desc.usage));
            return Ok(0.into());
        };
        let texture = gpu.device.create_texture(&wgpu::TextureDescriptor {
            label: desc::label(&desc.label),
            size: desc.size.into(),
            mip_level_count: desc.mip_level_count,
            sample_count: desc.sample_count,
            dimension: desc.dimension,
            format: desc.format,
            usage,
            view_formats: &desc.view_formats,
        });
        Ok(with_state(|s| s.insert(Obj::Texture(Texture { texture, device, canvas: None }))).into())
    }

    "destroyTexture" => fn destroy_texture(scope, args) {
        let (device, id) = (bind::arg_u32(scope, &args, 0), bind::arg_u32(scope, &args, 1));
        enter(device);
        with_state(|s| {
            match s.objects.get(&id) {
                Some(Obj::Texture(t)) if t.canvas.is_none() => t.texture.destroy(),
                _ => {}
            }
        });
        Ok(().into())
    }

    "createView" => fn create_view(scope, args) {
        let (device, texture) = (bind::arg_u32(scope, &args, 0), bind::arg_u32(scope, &args, 1));
        let desc: desc::ViewDesc = json(scope, &args, 2)?;
        if !enter(device) {
            return Ok(0.into());
        }
        Ok(with_state(|s| {
            let Some(t) = s.texture(device, texture) else { return 0 };
            let view = t.texture.create_view(&desc.wgpu());
            s.insert(Obj::View(View { view, texture }))
        })
        .into())
    }

    "createSampler" => fn create_sampler(scope, args) {
        let device = bind::arg_u32(scope, &args, 0);
        let desc: desc::SamplerDesc = json(scope, &args, 1)?;
        let Some(gpu) = live(device)? else { return Ok(0.into()) };
        let sampler = gpu.device.create_sampler(&desc.wgpu());
        Ok(with_state(|s| s.insert(Obj::Sampler(sampler))).into())
    }

    "createBindGroupLayout" => fn create_bind_group_layout(scope, args) {
        let device = bind::arg_u32(scope, &args, 0);
        let desc: desc::BindGroupLayoutDesc = json(scope, &args, 1)?;
        let entries = desc.entries.iter().map(desc::LayoutEntry::wgpu).collect::<Result<Vec<_>, _>>().map_err(Throw::from_message)?;
        let Some(gpu) = live(device)? else { return Ok(0.into()) };
        let layout = gpu.device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor { label: desc::label(&desc.label), entries: &entries });
        Ok(with_state(|s| s.insert(Obj::BindGroupLayout(layout))).into())
    }

    "createPipelineLayout" => fn create_pipeline_layout(scope, args) {
        let device = bind::arg_u32(scope, &args, 0);
        let desc: desc::PipelineLayoutDesc = json(scope, &args, 1)?;
        let Some(gpu) = live(device)? else { return Ok(0.into()) };
        Ok(with_state(|s| {
            let Some(layouts) = desc.bind_group_layouts.iter().map(|id| maybe(*id, |id| s.bind_group_layout(device, id))).collect::<Option<Vec<_>>>() else { return 0 };
            let layout = gpu.device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: desc::label(&desc.label), bind_group_layouts: &layouts, immediate_size: 0 });
            s.insert(Obj::PipelineLayout(layout))
        })
        .into())
    }

    "createBindGroup" => fn create_bind_group(scope, args) {
        let device = bind::arg_u32(scope, &args, 0);
        let desc: desc::BindGroupDesc = json(scope, &args, 1)?;
        let Some(gpu) = live(device)? else { return Ok(0.into()) };
        Ok(with_state(|s| {
            let Some(layout) = s.bind_group_layout(device, desc.layout) else { return 0 };
            let mut entries = Vec::with_capacity(desc.entries.len());
            let mut canvas_textures = Vec::new();
            for entry in &desc.entries {
                let r = &entry.resource;
                let resource = if let Some(id) = r.buffer {
                    let Some(b) = s.buffer(device, id) else { return 0 };
                    wgpu::BindingResource::Buffer(wgpu::BufferBinding { buffer: &b.buffer, offset: r.offset, size: r.size.and_then(std::num::NonZeroU64::new) })
                } else if let Some(id) = r.sampler {
                    let Some(sampler) = s.sampler(device, id) else { return 0 };
                    wgpu::BindingResource::Sampler(sampler)
                } else if let Some(id) = r.view {
                    let Some(v) = s.view(device, id) else { return 0 };
                    if let Some(Obj::Texture(t)) = s.objects.get(&v.texture)
                        && t.canvas.is_some()
                    {
                        canvas_textures.push(v.texture);
                    }
                    wgpu::BindingResource::TextureView(&v.view)
                } else {
                    report(device, Kind::Validation, format!("binding {} has no resource", entry.binding));
                    return 0;
                };
                entries.push(wgpu::BindGroupEntry { binding: entry.binding, resource });
            }
            let group = gpu.device.create_bind_group(&wgpu::BindGroupDescriptor { label: desc::label(&desc.label), layout, entries: &entries });
            s.insert(Obj::BindGroup(BindGroup { group, canvas_textures }))
        })
        .into())
    }

    "createShaderModule" => fn create_shader_module(scope, args) {
        let device = bind::arg_u32(scope, &args, 0);
        let code = bind::arg_string(scope, &args, 1);
        let label = Some(bind::arg_string(scope, &args, 2));
        let Some(gpu) = live(device)? else { return Ok(0.into()) };
        let module = gpu.device.create_shader_module(wgpu::ShaderModuleDescriptor { label: desc::label(&label), source: wgpu::ShaderSource::Wgsl(code.into()) });
        let messages = pollster::block_on(module.get_compilation_info()).messages;
        Ok(with_state(|s| s.insert(Obj::Shader(Shader { module, messages }))).into())
    }

    "compilationInfo" => fn compilation_info(scope, args) {
        let id = bind::arg_u32(scope, &args, 0);
        let messages: Vec<_> = with_state(|s| match s.objects.get(&id) {
            Some(Obj::Shader(shader)) => shader
                .messages
                .iter()
                .map(|m| {
                    let kind = match m.message_type {
                        wgpu::CompilationMessageType::Error => "error",
                        wgpu::CompilationMessageType::Warning => "warning",
                        wgpu::CompilationMessageType::Info => "info",
                    };
                    let [line, position, offset, length] = m.location.map(|l| [l.line_number, l.line_position, l.offset, l.length]).unwrap_or_default();
                    serde_json::json!({"message": m.message, "type": kind, "lineNum": line, "linePos": position, "offset": offset, "length": length})
                })
                .collect(),
            _ => Vec::new(),
        });
        Ok(Ret::Val(crate::host::offload::json(scope, &serde_json::Value::Array(messages))))
    }

    "createComputePipeline" => fn create_compute_pipeline(scope, args) {
        let device = bind::arg_u32(scope, &args, 0);
        let desc: desc::ComputePipelineDesc = json(scope, &args, 1)?;
        let is_async = bind::arg_bool(scope, &args, 2);
        let Some(gpu) = live(device)? else { return Ok(0.into()) };
        let make = || {
            with_state(|s| {
                let layout = maybe(desc.layout, |id| s.pipeline_layout(device, id))?;
                let module = &s.shader(device, desc.compute.module)?.module;
                let constants = desc.compute.constants();
                let pipeline = gpu.device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                    label: desc::label(&desc.label),
                    layout,
                    module,
                    entry_point: desc.compute.entry_point.as_deref(),
                    compilation_options: wgpu::PipelineCompilationOptions { constants: &constants, zero_initialize_workgroup_memory: true },
                    cache: None,
                });
                Some(s.insert(Obj::ComputePipeline(pipeline)))
            })
            .unwrap_or(0)
        };
        pipeline_result(scope, is_async, make)
    }

    "createRenderPipeline" => fn create_render_pipeline(scope, args) {
        let device = bind::arg_u32(scope, &args, 0);
        let desc: desc::RenderPipelineDesc = json(scope, &args, 1)?;
        let is_async = bind::arg_bool(scope, &args, 2);
        let Some(gpu) = live(device)? else { return Ok(0.into()) };
        let make = || {
            with_state(|s| {
                let layout = maybe(desc.layout, |id| s.pipeline_layout(device, id))?;
                let buffers: Vec<Option<wgpu::VertexBufferLayout>> = desc
                    .vertex
                    .buffers
                    .iter()
                    .map(|b| b.as_ref().map(|b| wgpu::VertexBufferLayout { array_stride: b.array_stride, step_mode: b.step_mode, attributes: &b.attributes }))
                    .collect();
                let vertex_module = &s.shader(device, desc.vertex.stage.module)?.module;
                let vertex_constants = desc.vertex.stage.constants();
                let fragment_module = maybe(desc.fragment.as_ref().map(|f| f.stage.module), |id| s.shader(device, id).map(|sh| &sh.module))?;
                let fragment_constants = desc.fragment.as_ref().map(|f| f.stage.constants()).unwrap_or_default();
                let targets: Vec<Option<wgpu::ColorTargetState>> = desc.fragment.iter().flat_map(|f| &f.targets).map(|t| t.as_ref().map(desc::ColorTargetDesc::wgpu)).collect();
                let fragment = desc.fragment.as_ref().zip(fragment_module).map(|(f, module)| wgpu::FragmentState {
                    module,
                    entry_point: f.stage.entry_point.as_deref(),
                    compilation_options: wgpu::PipelineCompilationOptions { constants: &fragment_constants, zero_initialize_workgroup_memory: true },
                    targets: &targets,
                });
                let pipeline = gpu.device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                    label: desc::label(&desc.label),
                    layout,
                    vertex: wgpu::VertexState {
                        module: vertex_module,
                        entry_point: desc.vertex.stage.entry_point.as_deref(),
                        compilation_options: wgpu::PipelineCompilationOptions { constants: &vertex_constants, zero_initialize_workgroup_memory: true },
                        buffers: &buffers,
                    },
                    primitive: desc.primitive.wgpu(),
                    depth_stencil: desc.depth_stencil.as_ref().map(desc::DepthStencilDesc::wgpu),
                    multisample: wgpu::MultisampleState { count: desc.multisample.count, mask: desc.multisample.mask.into(), alpha_to_coverage_enabled: desc.multisample.alpha_to_coverage_enabled },
                    fragment,
                    multiview_mask: None,
                    cache: None,
                });
                Some(s.insert(Obj::RenderPipeline(pipeline)))
            })
            .unwrap_or(0)
        };
        pipeline_result(scope, is_async, make)
    }

    "getBindGroupLayout" => fn get_bind_group_layout(scope, args) {
        let (device, pipeline, index) = (bind::arg_u32(scope, &args, 0), bind::arg_u32(scope, &args, 1), bind::arg_u32(scope, &args, 2));
        if !enter(device) {
            return Ok(0.into());
        }
        Ok(with_state(|s| {
            let layout = match s.objects.get(&pipeline) {
                Some(Obj::RenderPipeline(p)) => p.get_bind_group_layout(index),
                Some(Obj::ComputePipeline(p)) => p.get_bind_group_layout(index),
                _ => {
                    report(device, Kind::Validation, format!("invalid pipeline ({pipeline})"));
                    return 0;
                }
            };
            s.insert(Obj::BindGroupLayout(layout))
        })
        .into())
    }

    "createQuerySet" => fn create_query_set(scope, args) {
        let device = bind::arg_u32(scope, &args, 0);
        let desc: desc::QuerySetDesc = json(scope, &args, 1)?;
        let ty = match desc.kind {
            desc::QueryType::Occlusion => wgpu::QueryType::Occlusion,
            desc::QueryType::Timestamp => wgpu::QueryType::Timestamp,
        };
        let Some(gpu) = live(device)? else { return Ok(0.into()) };
        let set = gpu.device.create_query_set(&wgpu::QuerySetDescriptor { label: desc::label(&desc.label), ty, count: desc.count });
        Ok(with_state(|s| s.insert(Obj::QuerySet(QuerySet { set, device }))).into())
    }

    "destroyQuerySet" => fn destroy_query_set(scope, args) {
        let id = bind::arg_u32(scope, &args, 1);
        with_state(|s| {
            if let Some(Obj::QuerySet(q)) = s.objects.get(&id) {
                q.set.destroy();
            }
        });
        Ok(().into())
    }

    "createCommandEncoder" => fn create_command_encoder(scope, args) {
        let device = bind::arg_u32(scope, &args, 0);
        let label = Some(bind::arg_string(scope, &args, 1));
        let Some(gpu) = live(device)? else { return Ok(0.into()) };
        let encoder = gpu.device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: desc::label(&label) });
        Ok(with_state(|s| s.insert(Obj::Encoder(Some(encoder)))).into())
    }

    "copyBufferToBuffer" => fn copy_buffer_to_buffer(scope, args) {
        let (device, encoder, source) = (bind::arg_u32(scope, &args, 0), bind::arg_u32(scope, &args, 1), bind::arg_u32(scope, &args, 2));
        let (source_offset, destination, destination_offset, size) = (bind::arg_u64(scope, &args, 3), bind::arg_u32(scope, &args, 4), bind::arg_u64(scope, &args, 5), bind::arg_u64(scope, &args, 6));
        encode(device, encoder, |s, e| {
            let (Some(src), Some(dst)) = (s.buffer(device, source), s.buffer(device, destination)) else { return };
            e.copy_buffer_to_buffer(&src.buffer, source_offset, &dst.buffer, destination_offset, size);
        });
        Ok(().into())
    }

    "copyBufferToTexture" => fn copy_buffer_to_texture(scope, args) {
        let (device, encoder) = (bind::arg_u32(scope, &args, 0), bind::arg_u32(scope, &args, 1));
        let desc: desc::CopyBufferToTexture = json(scope, &args, 2)?;
        encode(device, encoder, |s, e| {
            canvas::touch(s, e, desc.destination.texture, false);
            let (Some(src), Some(dst)) = (s.buffer(device, desc.source.buffer), s.texture(device, desc.destination.texture)) else { return };
            e.copy_buffer_to_texture(wgpu::TexelCopyBufferInfo { buffer: &src.buffer, layout: desc.source.layout.wgpu() }, desc.destination.wgpu(&dst.texture), desc.size.into());
        });
        Ok(().into())
    }

    "copyTextureToBuffer" => fn copy_texture_to_buffer(scope, args) {
        let (device, encoder) = (bind::arg_u32(scope, &args, 0), bind::arg_u32(scope, &args, 1));
        let desc: desc::CopyTextureToBuffer = json(scope, &args, 2)?;
        encode(device, encoder, |s, e| {
            canvas::touch(s, e, desc.source.texture, false);
            let (Some(src), Some(dst)) = (s.texture(device, desc.source.texture), s.buffer(device, desc.destination.buffer)) else { return };
            e.copy_texture_to_buffer(desc.source.wgpu(&src.texture), wgpu::TexelCopyBufferInfo { buffer: &dst.buffer, layout: desc.destination.layout.wgpu() }, desc.size.into());
        });
        Ok(().into())
    }

    "copyTextureToTexture" => fn copy_texture_to_texture(scope, args) {
        let (device, encoder) = (bind::arg_u32(scope, &args, 0), bind::arg_u32(scope, &args, 1));
        let desc: desc::CopyTextureToTexture = json(scope, &args, 2)?;
        encode(device, encoder, |s, e| {
            canvas::touch(s, e, desc.source.texture, false);
            canvas::touch(s, e, desc.destination.texture, false);
            let (Some(src), Some(dst)) = (s.texture(device, desc.source.texture), s.texture(device, desc.destination.texture)) else { return };
            e.copy_texture_to_texture(desc.source.wgpu(&src.texture), desc.destination.wgpu(&dst.texture), desc.size.into());
        });
        Ok(().into())
    }

    "clearBuffer" => fn clear_buffer(scope, args) {
        let (device, encoder, buffer) = (bind::arg_u32(scope, &args, 0), bind::arg_u32(scope, &args, 1), bind::arg_u32(scope, &args, 2));
        let (offset, size) = (bind::arg_u64(scope, &args, 3), bind::arg_u64(scope, &args, 4));
        encode(device, encoder, |s, e| {
            let Some(b) = s.buffer(device, buffer) else { return };
            e.clear_buffer(&b.buffer, offset, Some(size));
        });
        Ok(().into())
    }

    "resolveQuerySet" => fn resolve_query_set(scope, args) {
        let (device, encoder, set) = (bind::arg_u32(scope, &args, 0), bind::arg_u32(scope, &args, 1), bind::arg_u32(scope, &args, 2));
        let (first, count, destination, offset) = (bind::arg_u32(scope, &args, 3), bind::arg_u32(scope, &args, 4), bind::arg_u32(scope, &args, 5), bind::arg_u64(scope, &args, 6));
        encode(device, encoder, |s, e| {
            let (Some(q), Some(b)) = (s.query_set(device, set), s.buffer(device, destination)) else { return };
            e.resolve_query_set(&q.set, first..first.saturating_add(count), &b.buffer, offset);
        });
        Ok(().into())
    }

    "encoderDebug" => fn encoder_debug(scope, args) {
        let (device, encoder, op) = (bind::arg_u32(scope, &args, 0), bind::arg_u32(scope, &args, 1), bind::arg_u32(scope, &args, 2));
        let label = bind::arg_string(scope, &args, 3);
        encode(device, encoder, |_, e| match op {
            0 => e.push_debug_group(&label),
            1 => e.pop_debug_group(),
            _ => e.insert_debug_marker(&label),
        });
        Ok(().into())
    }

    "renderPass" => fn render_pass(scope, args) {
        let (device, encoder) = (bind::arg_u32(scope, &args, 0), bind::arg_u32(scope, &args, 1));
        let desc: desc::RenderPassDesc = json(scope, &args, 2)?;
        let commands = pass::commands(&args, 3, bind::arg_u32(scope, &args, 4));
        let strings = pass::strings(scope, &args, 5);
        encode(device, encoder, |s, e| pass::render(s, e, device, &desc, &commands, &strings));
        Ok(().into())
    }

    "computePass" => fn compute_pass(scope, args) {
        let (device, encoder) = (bind::arg_u32(scope, &args, 0), bind::arg_u32(scope, &args, 1));
        let desc: desc::ComputePassDesc = json(scope, &args, 2)?;
        let commands = pass::commands(&args, 3, bind::arg_u32(scope, &args, 4));
        let strings = pass::strings(scope, &args, 5);
        encode(device, encoder, |s, e| pass::compute(s, e, device, &desc, &commands, &strings));
        Ok(().into())
    }

    "finishEncoder" => fn finish_encoder(scope, args) {
        let (device, encoder) = (bind::arg_u32(scope, &args, 0), bind::arg_u32(scope, &args, 1));
        if !enter(device) {
            return Ok(0.into());
        }
        Ok(with_state(|s| {
            let Some(Obj::Encoder(e)) = s.objects.get_mut(&encoder) else {
                report(device, Kind::Validation, format!("invalid GPUCommandEncoder ({encoder})"));
                return 0;
            };
            let Some(e) = e.take() else {
                report(device, Kind::Validation, "the command encoder is already finished".into());
                return 0;
            };
            let buffer = e.finish();
            s.insert(Obj::CommandBuffer(Some(buffer)))
        })
        .into())
    }

    "createRenderBundle" => fn create_render_bundle(scope, args) {
        let device = bind::arg_u32(scope, &args, 0);
        let desc: desc::RenderBundleDesc = json(scope, &args, 1)?;
        let commands = pass::commands(&args, 2, bind::arg_u32(scope, &args, 3));
        let strings = pass::strings(scope, &args, 4);
        let Some(gpu) = live(device)? else { return Ok(0.into()) };
        Ok(with_state(|s| pass::bundle(s, &gpu, device, &desc, &commands, &strings).map(|b| s.insert(Obj::RenderBundle(b))).unwrap_or(0)).into())
    }

    "submit" => fn submit(scope, args) {
        let device = bind::arg_u32(scope, &args, 0);
        let mut ids = Vec::new();
        if let Ok(list) = v8::Local::<v8::Array>::try_from(args.get(1)) {
            for i in 0..list.length() {
                ids.push(list.get_index(scope, i).and_then(|v| v.uint32_value(scope)).unwrap_or(0));
            }
        }
        let Some(gpu) = live(device)? else { return Ok(().into()) };
        let buffers = with_state(|s| {
            let usable = ids.iter().all(|id| matches!(s.objects.get(id), Some(Obj::CommandBuffer(Some(_)))));
            let buffers: Vec<_> = ids
                .iter()
                .filter_map(|id| match s.objects.get_mut(id) {
                    Some(Obj::CommandBuffer(b)) => b.take(),
                    _ => None,
                })
                .collect();
            if usable {
                return buffers;
            }
            report(device, Kind::Validation, "submit: a command buffer is invalid or was submitted already".into());
            Vec::new()
        });
        if !buffers.is_empty() {
            gpu.submit(buffers);
        }
        Ok(().into())
    }

    "writeTexture" => fn write_texture(scope, args) {
        let device = bind::arg_u32(scope, &args, 0);
        let desc: desc::WriteTexture = json(scope, &args, 1)?;
        let Some(gpu) = live(device)? else { return Ok(().into()) };
        let data = args.get(2);
        with_state(|s| {
            canvas::touch_now(s, &gpu, desc.destination.texture);
            let Some(t) = s.texture(device, desc.destination.texture) else { return };
            bind::with_bytes(data, |bytes| gpu.queue.write_texture(desc.destination.wgpu(&t.texture), bytes, desc.layout.wgpu(), desc.size.into()));
        });
        Ok(().into())
    }

    "presented" => fn presented(_scope, _args) {
        let Some(gpu) = with_state(|s| s.gpu.clone().flatten()).filter(|gpu| !gpu.gone()) else { return Ok(().into()) };
        let frame = gpu.frame();
        gpu.queue.on_submitted_work_done(move || frame.store(true, std::sync::atomic::Ordering::Release));
        Ok(().into())
    }

    "inFlight" => fn in_flight(scope, args) {
        Ok(Ret::Num(Gpu::frames_in_flight(bind::arg_u32(scope, &args, 0) as usize) as f64))
    }

    "onSubmittedWorkDone" => fn on_submitted_work_done(scope, args) {
        let device = bind::arg_u32(scope, &args, 0);
        let Some((token, promise)) = promise(scope) else { return Ok(().into()) };
        let gpu = with_state(|s| s.gpu())?;
        enter(device);
        let tx = rt::current().sender();
        let held = gpu.clone();
        held.hold_poll();
        gpu.queue.on_submitted_work_done(move || {
            _ = tx.send(Msg::Task(Box::new(move |scope| settle(scope, token, None))));
            held.release_poll();
        });
        Ok(Ret::Val(promise))
    }
}

fn pipeline_result<'s>(scope: &mut v8::PinScope<'s, '_>, is_async: bool, make: impl FnOnce() -> Id) -> Out<'s> {
    if !is_async {
        return Ok(make().into());
    }
    let (id, error) = capturing(make);
    Ok(match error {
        Some(error) => {
            with_state(|s| s.objects.remove(&id));
            Ret::Val(bind::string(scope, &error.message).into())
        }
        None => id.into(),
    })
}

fn encode(device: Id, encoder: Id, f: impl FnOnce(&mut State, &mut wgpu::CommandEncoder)) {
    if !enter(device) {
        return;
    }
    with_state(|s| {
        let taken = match s.objects.get_mut(&encoder) {
            Some(Obj::Encoder(e)) => e.take(),
            _ => {
                report(device, Kind::Validation, format!("invalid GPUCommandEncoder ({encoder})"));
                return;
            }
        };
        let Some(mut e) = taken else {
            report(device, Kind::Validation, "the command encoder is finished".into());
            return;
        };
        f(s, &mut e);
        if let Some(Obj::Encoder(slot)) = s.objects.get_mut(&encoder) {
            *slot = Some(e);
        }
    });
}

#[allow(clippy::too_many_arguments)]
fn map_done(scope: &mut v8::PinScope, token: u32, id: Id, request: u32, ok: bool, write: bool, offset: u64, size: u64) {
    let outcome = with_state(|s| {
        let Some(Obj::Buffer(b)) = s.objects.get_mut(&id) else { return Some("AbortError: the buffer is gone") };
        if b.map != Map::Pending || b.request != request {
            return Some("AbortError: the buffer was unmapped or destroyed before the mapping finished");
        }
        if ok {
            b.map = Map::Mapped { write, created: false, offset, size };
            None
        } else {
            b.map = Map::Unmapped;
            Some("OperationError: the mapping failed")
        }
    });
    settle(scope, token, outcome);
}
