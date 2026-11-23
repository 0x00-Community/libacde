use serde::Serialize;

use crate::gpu::Gpu;
use crate::render::layers::Backdrop;
use crate::render::{Frame, Plane};

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct BackdropFact {
    pub panel: Option<u32>,
    pub rect: [f64; 4],
    pub glass: bool,
    pub refracts: bool,
    pub group: Option<String>,
    pub pieces: usize,
    pub left_out: bool,
}

impl BackdropFact {
    pub(super) fn of_frame(frame: &Frame) -> Vec<Self> {
        let panels = frame.panels.iter().flat_map(|p| Self::of_plane(&p.plane, Some(p.id), frame.scale));
        Self::of_plane(&frame.plane, None, frame.scale).into_iter().chain(panels).collect()
    }

    fn of_plane(plane: &Plane, panel: Option<u32>, scale: f64) -> Vec<Self> {
        plane.layers.iter().filter_map(|l| l.backdrop.as_ref()).map(|b| Self::of(b, panel, scale)).collect()
    }

    fn of(b: &Backdrop, panel: Option<u32>, scale: f64) -> Self {
        let r = b.bounds.intersect(b.clip);
        let css = |v: f64| (v / scale * 100.0).round() / 100.0;
        Self {
            panel,
            rect: [css(r.x0), css(r.y0), css(r.width().max(0.0)), css(r.height().max(0.0))],
            glass: b.glass().is_some(),
            refracts: b.glass().is_some_and(|g| g.refracts()),
            group: b.group_name.clone(),
            pieces: b.group().map_or(1, |g| g.len()),
            left_out: b.left_out_of_its_group(),
        }
    }
}

#[derive(Debug, PartialEq, Serialize)]
#[serde(tag = "path", rename_all = "lowercase")]
enum Raster {
    Gpu { adapter: String, driver: String, backend: String, software: bool },
    Cpu { canvases: Option<String> },
    Lost { adapter: String },
    None,
    Pending,
}

impl Raster {
    fn now() -> Self {
        let published = Gpu::get();
        if crate::render::gpu::cpu_raster_registered() {
            return Raster::Cpu { canvases: published.flatten().map(|g| g.info.name.clone()) };
        }
        match published {
            Some(Some(gpu)) if gpu.gone() => Raster::Lost { adapter: gpu.info.name.clone() },
            Some(Some(gpu)) => Raster::of(&gpu.info),
            Some(None) => Raster::None,
            None => Raster::Pending,
        }
    }

    fn of(info: &wgpu::AdapterInfo) -> Self {
        let driver = [info.driver.as_str(), info.driver_info.as_str()].into_iter().filter(|s| !s.is_empty()).collect::<Vec<_>>().join(" ");
        Raster::Gpu {
            adapter: info.name.clone(),
            driver,
            backend: format!("{:?}", info.backend).to_lowercase(),
            software: info.device_type == wgpu::DeviceType::Cpu,
        }
    }
}

pub(super) fn report(backdrops: &[BackdropFact]) -> String {
    serde_json::json!({ "raster": Raster::now(), "backdrops": backdrops }).to_string()
}

#[cfg(test)]
mod tests {
    use super::Raster;

    #[test]
    fn a_page_without_any_device_says_none() {
        assert_eq!(serde_json::to_value(Raster::None).unwrap(), serde_json::json!({ "path": "none" }));
    }
}
