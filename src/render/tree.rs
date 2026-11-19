use std::collections::{HashMap, HashSet};

use crate::render::error::{Error, Result};
use blitz_dom::{DocumentMutator, LocalName, Namespace, NodeId, QualName, ns};

#[derive(Default)]
pub(crate) struct Ids {
    nodes: Vec<NodeId>,
    slots: Vec<Slot>,
    live: u32,
    pub(crate) canvases: HashMap<u32, crate::render::canvas::Canvas>,
    pub(crate) gpu_canvases: HashSet<u32>,
    pub(crate) images: HashMap<u32, (String, u8, u32)>,
    pub(crate) templates: HashMap<NodeId, NodeId>,
    pub(crate) doctypes: HashMap<NodeId, Doctype>,
    pub(crate) pis: HashMap<NodeId, String>,
    pub(crate) cdata: HashSet<NodeId>,
    pub(crate) owners: HashMap<NodeId, NodeId>,
    pub(crate) documents: HashMap<NodeId, DocumentInfo>,
    pub(crate) page: Option<DocumentInfo>,
    pub(crate) forms: HashMap<NodeId, Form>,
    pub(crate) masked: HashSet<NodeId>,
    pub(crate) scrolls: HashMap<NodeId, (f64, f64)>,
    pub(crate) fresh: Vec<NodeId>,
    pub(crate) unpinned: Vec<NodeId>,
}

#[derive(Clone, Copy)]
struct Slot {
    node: NodeId,
    id: u32,
    pinned: bool,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct Doctype {
    pub(crate) name: String,
    pub(crate) public_id: String,
    pub(crate) system_id: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct DocumentInfo {
    pub(crate) xml: bool,
    pub(crate) interface: &'static str,
    pub(crate) content_type: String,
    pub(crate) url: String,
}

impl DocumentInfo {
    pub(crate) fn new(interface: &str, content_type: &str, url: &str) -> Self {
        let interface = match interface {
            "html" => "html",
            "xml" => "xml",
            _ => "document",
        };
        Self { xml: interface != "html", interface, content_type: content_type.to_owned(), url: url.to_owned() }
    }
}

#[derive(Clone, Debug, Default)]
pub(crate) struct Form {
    pub(crate) value: Option<String>,
    pub(crate) checked: Option<bool>,
    pub(crate) selected: Option<bool>,
    pub(crate) dirty: bool,
}

const NO_ID: u32 = u32::MAX;

impl Ids {
    fn track(&mut self, node: NodeId) -> u32 {
        let id = self.nodes.len() as u32;
        self.nodes.push(node);
        let index = Self::index(node);
        if self.slots.len() <= index {
            self.slots.resize(index + 1, Slot { node: NodeId::default(), id: NO_ID, pinned: false });
        }
        self.slots[index] = Slot { node, id, pinned: false };
        self.live += 1;
        id
    }

    pub(crate) fn id_of(&mut self, node: NodeId) -> u32 {
        match self.handle(node) {
            Some(id) => id,
            None => self.track(node),
        }
    }

    pub(crate) fn node(&self, id: u32) -> Result<NodeId> {
        match self.nodes.get(id as usize) {
            Some(&node) if node != NodeId::default() => Ok(node),
            Some(_) => Err(Error::new(format!("node {id} was released"))),
            None => Err(Error::new(format!("unknown node {id}"))),
        }
    }

    fn slot(&self, node: NodeId) -> Option<&Slot> {
        self.slots.get(Self::index(node)).filter(|s| s.node == node && s.id != NO_ID)
    }

    fn index(node: NodeId) -> usize {
        node.as_u64() as u32 as usize
    }

    pub(crate) fn forget(&mut self, node: NodeId) {
        self.templates.remove(&node);
        self.doctypes.remove(&node);
        self.pis.remove(&node);
        self.cdata.remove(&node);
        self.owners.remove(&node);
        self.documents.remove(&node);
        self.forms.remove(&node);
        self.masked.remove(&node);
        self.scrolls.remove(&node);
        let Some(id) = self.handle(node) else { return };
        self.slots[Self::index(node)] = Slot { node: NodeId::default(), id: NO_ID, pinned: false };
        self.live -= 1;
        self.nodes[id as usize] = NodeId::default();
        self.canvases.remove(&id);
        self.gpu_canvases.remove(&id);
        self.images.remove(&id);
    }

    pub(crate) fn live(&self) -> u32 {
        self.live
    }

    pub(crate) fn handle(&self, node: NodeId) -> Option<u32> {
        self.slot(node).map(|s| s.id)
    }

    pub(crate) fn pin(&mut self, node: NodeId, pinned: bool) {
        if let Some(slot) = self.slots.get_mut(Self::index(node)).filter(|s| s.node == node && s.id != NO_ID) {
            slot.pinned = pinned;
        }
    }

    pub(crate) fn pinned(&self, node: NodeId) -> bool {
        self.slot(node).is_some_and(|s| s.pinned)
    }

    pub(crate) fn xml_owned(&self, owner: Option<NodeId>) -> bool {
        match owner {
            Some(document) => self.documents.get(&document).is_some_and(|info| info.xml),
            None => self.page.as_ref().is_some_and(|info| info.xml),
        }
    }
}

pub(crate) struct Tree<'d> {
    pub(crate) m: DocumentMutator<'d>,
    pub(crate) ids: &'d mut Ids,
}

impl Tree<'_> {
    pub(crate) fn create_element(&mut self, ns: Namespace, tag: &str) -> NodeId {
        let html = ns == ns!(html);
        let tag = match html {
            true => tag.to_ascii_lowercase(),
            false => tag.to_owned(),
        };
        let node = self.m.create_element(QualName::new(None, ns, tag.as_str().into()), Vec::new());
        let id = self.ids.track(node);
        if html && tag == "img" {
            self.ids.images.insert(id, (String::new(), u8::MAX, 0));
        }
        node
    }

    pub(crate) fn create_element_named(&mut self, name: QualName) -> NodeId {
        let img = name.ns == ns!(html) && &*name.local == "img";
        let node = self.m.create_element(name, Vec::new());
        let id = self.ids.track(node);
        if img {
            self.ids.images.insert(id, (String::new(), u8::MAX, 0));
        }
        node
    }

    pub(crate) fn set_attribute(&mut self, id: u32, node: NodeId, name: QualName, value: Option<&str>) {
        self.reset_canvas(id, node, &name.local);
        match value {
            Some(value) => self.m.set_attribute(node, name, value),
            None => self.m.clear_attribute(node, name),
        }
    }

    fn reset_canvas(&mut self, id: u32, node: NodeId, name: &LocalName) {
        if matches!(&**name, "width" | "height") && (self.ids.canvases.remove(&id).is_some() | self.ids.gpu_canvases.remove(&id)) {
            self.m.doc.set_canvas_picture(node, None);
        }
    }

    fn canvas_node(&self, id: u32) -> Result<NodeId> {
        let node = self.ids.node(id)?;
        match self.m.doc.get_node(node).and_then(|n| n.element_data()).is_some_and(|e| &*e.name.local == "canvas") {
            true => Ok(node),
            false => Err(Error::new(format!("node {id} is not a <canvas>"))),
        }
    }

    pub(crate) fn canvas_image(&mut self, id: u32, image: Option<peniko::ImageData>) -> Result<()> {
        use crate::gpu::PaintScene;
        let node = self.canvas_node(id)?;
        self.ids.canvases.remove(&id);
        let Some(image) = image.filter(|i| i.width > 0 && i.height > 0) else {
            self.ids.gpu_canvases.remove(&id);
            self.m.doc.set_canvas_picture(node, None);
            return Ok(());
        };
        let (width, height) = crate::render::canvas::Canvas::size_of(self.m.doc, node);
        let fit = kurbo::Affine::scale_non_uniform(width / f64::from(image.width), height / f64::from(image.height));
        let mut picture = crate::gpu::Scene::new();
        let brush = peniko::ImageBrush { image, sampler: peniko::ImageSampler::default() };
        picture.fill(peniko::Fill::NonZero, kurbo::Affine::IDENTITY, &crate::gpu::Paint::Image(brush), Some(fit), &kurbo::Rect::new(0.0, 0.0, width, height));
        self.ids.gpu_canvases.insert(id);
        self.m.doc.set_canvas_picture(node, Some(std::sync::Arc::new(picture)));
        Ok(())
    }

    pub(crate) fn video_image(&mut self, id: u32, image: Option<peniko::ImageData>) -> Result<bool> {
        use crate::gpu::PaintScene;
        let node = self.ids.node(id)?;
        if !self.m.doc.get_node(node).and_then(|n| n.element_data()).is_some_and(|e| &*e.name.local == "video") {
            return Err(Error::new(format!("node {id} is not a <video>")));
        }
        let before = self.m.doc.video_size(node);
        let Some(image) = image.filter(|i| i.width > 0 && i.height > 0) else {
            self.m.doc.set_canvas_picture(node, None);
            self.m.doc.set_video_size(node, None);
            return Ok(before.is_some());
        };
        let size = (image.width as f32, image.height as f32);
        let rect = kurbo::Rect::new(0.0, 0.0, f64::from(image.width), f64::from(image.height));
        let mut picture = crate::gpu::Scene::new();
        let brush = peniko::ImageBrush { image, sampler: peniko::ImageSampler::default() };
        picture.fill(peniko::Fill::NonZero, kurbo::Affine::IDENTITY, &crate::gpu::Paint::Image(brush), None, &rect);
        self.m.doc.set_canvas_picture(node, Some(std::sync::Arc::new(picture)));
        self.m.doc.set_video_size(node, Some(size));
        Ok(before != Some(size))
    }

    pub(crate) fn canvas(&mut self, id: u32, ops: &[u8]) -> Result<()> {
        use crate::render::canvas::{Canvas, Source};
        let node = self.canvas_node(id)?;
        let size = Canvas::size_of(self.m.doc, node);
        let mut canvas = self.ids.canvases.remove(&id).filter(|c| c.size() == size).unwrap_or_else(|| Canvas::new(size));
        let ids = &*self.ids;
        let source = |doc: &blitz_dom::BaseDocument, kind: u8, source: u32| -> Option<Source> {
            match kind {
                0 => {
                    let element = doc.get_node(ids.node(source).ok()?)?.element_data()?;
                    if let Some(blitz_dom::node::ImageData::Svg(svg)) = element.image_data() {
                        let size = svg.tree.size();
                        let picture = blitz_paint::svg_picture(svg, size.width(), size.height());
                        return Some(Source::Picture(std::sync::Arc::new(picture), (f64::from(size.width()), f64::from(size.height()))));
                    }
                    let image = element.raster_image_data()?;
                    Some(Source::Raster(peniko::ImageBrush {
                        image: peniko::ImageData {
                            data: image.data.clone(),
                            format: peniko::ImageFormat::Rgba8,
                            width: image.width,
                            height: image.height,
                            alpha_type: peniko::ImageAlphaType::Alpha,
                        },
                        sampler: peniko::ImageSampler::default(),
                    }))
                }
                _ => ids.canvases.get(&source).map(Canvas::source).or_else(|| {
                    let node = ids.node(source).ok()?;
                    let picture = doc.canvas_picture(node)?.clone();
                    let (w, h) = Canvas::size_of(doc, node);
                    Some(Source::Picture(picture, (w, h)))
                }),
            }
        };
        self.m.doc.set_canvas_picture(node, None);
        let result = canvas.run(self.m.doc, ops, &source);
        let picture = canvas.picture();
        self.ids.canvases.insert(id, canvas);
        self.m.doc.set_canvas_picture(node, Some(picture));
        result
    }
}
