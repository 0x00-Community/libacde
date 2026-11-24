use std::path::PathBuf;
use std::sync::Mutex;

struct Kept {
    device: wgpu::Device,
    cache: wgpu::PipelineCache,
    path: Option<PathBuf>,
    saved: usize,
}

static CACHES: Mutex<Vec<Kept>> = Mutex::new(Vec::new());

#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn folder() -> Option<PathBuf> {
    let base = std::env::var_os("XDG_CACHE_HOME")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").filter(|v| !v.is_empty()).map(|h| PathBuf::from(h).join(".cache")))?;
    Some(base.join("acde").join("wgpu"))
}

#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
pub(crate) fn open(device: &wgpu::Device, info: &wgpu::AdapterInfo) {
    if !device.features().contains(wgpu::Features::PIPELINE_CACHE) {
        return;
    }
    let path = wgpu::util::pipeline_cache_key(info).and_then(|key| Some(folder()?.join(key)));
    let data = path.as_ref().and_then(|p| std::fs::read(p).ok());
    let descriptor = wgpu::PipelineCacheDescriptor { label: Some("acde pipelines"), data: data.as_deref(), fallback: true };
    let cache = unsafe { device.create_pipeline_cache(&descriptor) };
    let saved = data.map_or(0, |d| d.len());
    let mut caches = CACHES.lock().unwrap_or_else(|e| e.into_inner());
    caches.retain(|k| k.device != *device);
    caches.push(Kept { device: device.clone(), cache, path, saved });
}

pub(crate) fn of(device: &wgpu::Device) -> Option<wgpu::PipelineCache> {
    CACHES.lock().unwrap_or_else(|e| e.into_inner()).iter().find(|k| k.device == *device).map(|k| k.cache.clone())
}

pub(crate) fn save(device: &wgpu::Device) {
    let mut caches = CACHES.lock().unwrap_or_else(|e| e.into_inner());
    let Some(kept) = caches.iter_mut().find(|k| k.device == *device) else { return };
    let (Some(path), Some(data)) = (kept.path.clone(), kept.cache.get_data()) else { return };
    if data.len() == kept.saved {
        return;
    }
    kept.saved = data.len();
    drop(caches);
    let written = path.parent().map_or(Ok(()), std::fs::create_dir_all).and_then(|()| {
        let partial = path.with_extension(format!("{}.part", std::process::id()));
        std::fs::write(&partial, &data).and_then(|()| std::fs::rename(&partial, &path))
    });
    if let Err(e) = written {
        log::debug!("wgpu pipeline cache {}: {e}", path.display());
    }
}
