#![allow(dead_code)]

use std::ops::Deref;

use std::time::{Duration, Instant};

use acde::render::{Document, Dom, Error, Frame, Metrics, NONE, Ns, Options, Rect, RenderHandle, Result};

pub struct Doc {
    document: Document,
    pub dom: Dom,
}

#[derive(Clone)]
pub struct Handle {
    handle: RenderHandle,
    dom: Dom,
}

impl Deref for Handle {
    type Target = RenderHandle;

    fn deref(&self) -> &RenderHandle {
        &self.handle
    }
}

impl Handle {
    pub fn commit(&self) -> u64 {
        self.dom.commit();
        self.handle.commit()
    }
}

impl Deref for Doc {
    type Target = Dom;

    fn deref(&self) -> &Dom {
        &self.dom
    }
}

pub trait WaitFrame {
    fn wait_frame(&self) -> Frame;
    fn wait_frame_after(&self, commit: u64) -> Frame;
}

impl WaitFrame for RenderHandle {
    fn wait_frame(&self) -> Frame {
        let start = Instant::now();
        loop {
            if let Some(frame) = self.frame() {
                return frame;
            }
            assert!(start.elapsed() < Duration::from_secs(5), "no frame");
            std::thread::sleep(Duration::from_millis(1));
        }
    }

    fn wait_frame_after(&self, commit: u64) -> Frame {
        let start = Instant::now();
        loop {
            if let Some(frame) = self.frame().filter(|f| f.commit > commit) {
                return frame;
            }
            assert!(start.elapsed() < Duration::from_secs(5), "no frame after commit {commit}");
            std::thread::sleep(Duration::from_millis(1));
        }
    }
}

pub fn vello() {
    if let Some(gpu) = acde::gpu::Gpu::shared() {
        gpu.set_compositor(std::sync::Arc::new(acde::vello::VelloCompositor));
    }
}

pub fn rasterize(scene: acde::gpu::Scene, size: (u32, u32)) -> Vec<u8> {
    vello();
    acde::render::gpu::rasterize(scene, size)
}

pub fn near(a: f64, b: f64) -> bool {
    (a - b).abs() < 0.6
}

fn rect([x, y, width, height]: [f64; 4]) -> Rect {
    Rect { x, y, width, height }
}

impl Doc {
    pub fn new(width: u32, height: u32, scale: f32, options: &Options) -> Result<Self> {
        vello();
        let document = Document::new(width, height, scale, options)?;
        let dom = document.dom();
        Ok(Self { document, dom })
    }

    pub fn handle(&self) -> Handle {
        Handle { handle: self.document.handle(), dom: Clone::clone(&self.dom) }
    }

    pub fn commit(&self) -> u64 {
        self.handle().commit()
    }

    pub fn sync(&self) {
        self.document.sync()
    }

    pub fn scene(&self) -> acde::gpu::Scene {
        self.document.scene()
    }

    fn pinned(&self, id: Result<u32>) -> Result<u32> {
        let id = id?;
        self.dom.pin(id)?;
        Ok(id)
    }

    pub fn create_element(&self, tag: &str) -> Result<u32> {
        self.create_element_ns(0, tag)
    }

    pub fn create_element_ns(&self, ns: u8, tag: &str) -> Result<u32> {
        let ns = match ns {
            1 => Ns::Svg,
            2 => Ns::MathMl,
            _ => Ns::Html,
        };
        self.pinned(self.dom.create_element(ns, tag))
    }

    pub fn create_text(&self, text: &str) -> Result<u32> {
        self.pinned(self.dom.create_text(text))
    }

    pub fn append_child(&self, parent: u32, child: u32) -> Result<()> {
        self.dom.insert_before(parent, child, NONE)
    }

    pub fn insert_before(&self, parent: u32, child: u32, reference: Option<u32>) -> Result<()> {
        self.dom.insert_before(parent, child, reference.unwrap_or(NONE))
    }

    pub fn release(&self, id: u32) -> Result<()> {
        if id == 0 {
            return Err(Error("cannot release <html>".into()));
        }
        let pinned = match self.dom.pinned_in(id) {
            Ok(pinned) => pinned,
            Err(e) if e.0.contains("released") || e.0.contains("gone") => return Ok(()),
            Err(e) => return Err(e),
        };
        self.dom.remove(id)?;
        for node in pinned {
            self.dom.unpin(node)?;
        }
        self.dom.commit();
        Ok(())
    }

    pub fn add_stylesheet(&self, css: &str) -> Result<()> {
        let style = self.create_element("style")?;
        let text = self.create_text(css)?;
        self.append_child(style, text)?;
        self.append_child(0, style)
    }

    pub fn rect(&self, id: u32) -> Result<Rect> {
        self.dom.rect(id).map(rect)
    }

    pub fn metrics(&self, id: u32) -> Result<Metrics> {
        Ok(self.dom.metrics(id)?.unwrap_or_default())
    }

    pub fn caret_rect(&self, id: u32, offset: u32) -> Result<Rect> {
        self.dom.caret_rect(id, offset).map(rect)
    }

    pub fn text_rects(&self, id: u32, start: u32, end: u32) -> Result<Vec<Rect>> {
        Ok(self.dom.text_rects(id, start, end)?.into_iter().map(rect).collect())
    }

    pub fn scroll_by(&self, id: u32, dx: f64, dy: f64) -> Result<()> {
        let m = self.metrics(id)?;
        self.dom.scroll_to(id, m.scroll_left + dx, m.scroll_top + dy, false)
    }
}
