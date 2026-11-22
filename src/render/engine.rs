use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Sender;
use std::sync::{Arc, Weak};
use std::time::Instant;

use blitz_dom::{BaseDocument, DocumentConfig, NodeId};
use blitz_traits::net::Url;
use blitz_traits::shell::{ColorScheme, Viewport};

use crate::render::canvas::Canvas;
use crate::render::commands::Commands;
use crate::render::damage::Damage;
use crate::render::net::LocalNet;
use crate::render::tree::{Ids, Tree};
use crate::render::verify::Mismatch;
use crate::render::{Error, Frame, Options, Panel, Plane, Rect, Result, Stats};
use blitz_paint::PaintCache;

mod highlight;

const FRAME: f64 = 1.0 / 60.0;

const UA_EXTRA: &str = "button, select, input:is([type=button], [type=submit], [type=reset], [type=checkbox], [type=radio], \
    [type=color], [type=file], [type=image], [type=range]) { box-sizing: border-box; } \
    input[type=range] { appearance: auto; width: 129px; height: 16px; margin: 2px; padding: 0; border: 0; background: transparent; cursor: default; }";

pub(crate) struct Engine {
    pub(crate) doc: BaseDocument,
    pub(crate) ids: Ids,
    clock: Clock,
    resolved_at: f64,
    pub(crate) in_task: Option<u64>,
    task_time: Option<(u64, f64)>,
    arrived: Arc<AtomicBool>,
    pub(crate) stale: bool,
    pub(crate) dirty: bool,
    stats: Stats,
    damage: Damage,
    glass: acde_scene::glass::Preference,
    paint_cache: PaintCache,
    panels: Vec<(NodeId, u32, kurbo::Rect)>,
    panel_damage: std::collections::HashMap<u32, Damage>,
    verify: bool,
    from_cache: bool,
    pub(crate) committed: u64,
    pub(crate) uncommitted: bool,
    pub(crate) want_present: bool,
    pub(crate) active: Option<blitz_dom::NodeId>,
    pub(crate) picker: Option<blitz_dom::node::SelectPicker>,
    pub(crate) highlight: Option<NodeId>,
    pub(crate) name: String,
    pub(crate) trace: Option<crate::log::Trace>,
    pub(crate) input: Option<crate::log::Trace>,
    pub(crate) held: Vec<crate::render::Cmd>,
}

unsafe impl Send for Engine {}

impl Engine {
    pub(crate) fn new(width: u32, height: u32, scale: f32, options: &Options, wake: Weak<Sender<()>>) -> Result<Self> {
        let base = options.base_url.as_deref().unwrap_or("http://acde.local/");
        let base = Url::parse(base).map_err(|e| Error::new(format!("base_url {base}: {e}")))?;
        let resources = options.resources.as_deref().unwrap_or_default();
        let arrived = Arc::new(AtomicBool::new(false));
        let net = LocalNet::new(base.clone(), resources, arrived.clone(), options.loader.clone(), wake);
        let mut this = Self {
            doc: BaseDocument::new(DocumentConfig {
                viewport: Some(Viewport::new(width, height, scale, if options.dark { ColorScheme::Dark } else { ColorScheme::Light })),
                base_url: Some(base.into()),
                net_provider: Some(Arc::new(net)),
                skip_iframe_content: true,
                default_lang: Some(options.default_lang.clone().unwrap_or_else(|| "zh-Hans".into())),
                svg_text_only: options.svg_text_path.unwrap_or(false),
                ..Default::default()
            }),
            ids: Ids::default(),
            clock: Clock::default(),
            resolved_at: 0.0,
            in_task: None,
            task_time: None,
            arrived,
            stale: true,
            dirty: true,
            stats: Stats::default(),
            damage: Damage::default(),
            glass: acde_scene::glass::preference(),
            paint_cache: PaintCache::default(),
            panels: Vec::new(),
            panel_damage: Default::default(),
            verify: options.verify_paint.unwrap_or_else(|| std::env::var("ACDE_PAINT_VERIFY").is_ok_and(|v| v == "1")),
            from_cache: false,
            committed: 0,
            uncommitted: false,
            want_present: false,
            active: None,
            picker: None,
            highlight: None,
            name: options.name.clone().unwrap_or_default(),
            trace: None,
            input: None,
            held: Vec::new(),
        };
        let document = this.doc.root_node().id;
        {
            let mut tree = this.tree();
            let html = tree.create_element(blitz_dom::ns!(html), "html");
            tree.m.append_children(document, &[html]);
        }
        this.doc.add_user_agent_stylesheet(blitz_dom::UserAgentStylesheet::parse(UA_EXTRA));
        Ok(this)
    }

    pub(crate) fn set_panel(&mut self, node: NodeId, id: u32, rect: Option<[f64; 4]>) {
        let scale = self.doc.viewport().scale_f64();
        let had = self.panels.iter().any(|p| p.0 == node);
        self.panels.retain(|p| p.0 != node);
        match rect {
            Some([x, y, w, h]) => self.panels.push((node, id, kurbo::Rect::new(x * scale, y * scale, (x + w) * scale, (y + h) * scale))),
            None => _ = self.panel_damage.remove(&id),
        }
        if had != rect.is_some() {
            self.paint_cache.clear();
            self.damage = Damage::default();
        }
    }

    pub(crate) fn image_pixels(&self, id: u32, node: NodeId) -> Option<crate::render::Pixels> {
        let element = self.doc.get_node(node)?.element_data()?;
        match &*element.name.local {
            "img" => {
                if let Some(blitz_dom::node::ImageData::Svg(svg)) = element.image_data() {
                    let size = svg.tree.size();
                    let (w, h) = (size.width().ceil() as u32, size.height().ceil() as u32);
                    let picture = blitz_paint::svg_picture(svg, w as f32, h as f32);
                    return crate::render::gpu::draw(picture, (w, h), kurbo::Affine::IDENTITY).map(crate::render::Pixels::Gpu);
                }
                let image = element.raster_image_data()?;
                Some(crate::render::Pixels::Cpu { width: image.width, height: image.height, rgba: image.data.data().to_vec() })
            }
            "canvas" if self.ids.canvases.contains_key(&id) => self.ids.canvases.get(&id)?.bitmap().map(crate::render::Pixels::Gpu),
            _ => {
                let picture = self.doc.canvas_picture(node)?;
                picture.commands.iter().find_map(|c| match c {
                    crate::gpu::RenderCommand::Fill(f) => match &f.brush {
                        crate::gpu::Paint::Image(brush) => match crate::gpu::Gpu::shared().and_then(|g| g.images.find(brush.image.data.id())) {
                            Some(image) => Some(crate::render::Pixels::Gpu(image)),
                            None => Some(crate::render::Pixels::Cpu {
                                width: brush.image.width,
                                height: brush.image.height,
                                rgba: brush.image.data.data().to_vec(),
                            }),
                        },
                        _ => None,
                    },
                    _ => None,
                })
            }
        }
    }

    pub(crate) fn with_canvas<T>(&self, id: u32, node: NodeId, f: impl FnOnce(&Canvas) -> T) -> T {
        match self.ids.canvases.get(&id) {
            Some(c) => f(c),
            None => f(&Canvas::new(Canvas::size_of(&self.doc, node))),
        }
    }

    pub(crate) fn resize(&mut self, width: u32, height: u32, scale: f32) {
        let mut viewport = self.doc.viewport().clone();
        viewport.window_size = (width, height);
        viewport.hidpi_scale = scale;
        self.doc.set_viewport(viewport);
        self.touch();
    }

    pub(crate) fn set_reduced_motion(&mut self, reduced: bool) {
        let preset = if reduced { acde_scene::motion::Preset::Reduced } else { acde_scene::motion::Preset::Standard };
        if acde_scene::motion::set_system(preset) {
            self.touch();
        }
    }

    pub(crate) fn set_glass(&mut self, preference: acde_scene::glass::Preference) {
        acde_scene::glass::set_preference(preference);
        if self.glass == preference {
            return;
        }
        self.glass = preference;
        self.paint_cache.clear();
        self.damage = Damage::default();
        self.touch();
    }

    pub(crate) fn set_dark(&mut self, on: bool) {
        let scheme = if on { ColorScheme::Dark } else { ColorScheme::Light };
        if self.doc.viewport().color_scheme == scheme {
            return;
        }
        let mut viewport = self.doc.viewport().clone();
        viewport.color_scheme = scheme;
        self.doc.set_viewport(viewport);
        self.touch();
    }

    fn malloc_bytes() -> f64 {
        #[cfg(all(target_os = "linux", target_env = "gnu"))]
        {
            #[repr(C)]
            struct MallInfo2 {
                arena: usize,
                ordblks: usize,
                smblks: usize,
                hblks: usize,
                hblkhd: usize,
                usmblks: usize,
                fsmblks: usize,
                uordblks: usize,
                fordblks: usize,
                keepcost: usize,
            }
            unsafe extern "C" {
                fn mallinfo2() -> MallInfo2;
            }
            let m = unsafe { mallinfo2() };
            (m.uordblks + m.hblkhd) as f64
        }
        #[cfg(not(all(target_os = "linux", target_env = "gnu")))]
        0.0
    }

    pub(crate) fn stats(&self) -> Stats {
        let m = self.doc.memory();
        Stats {
            nodes: m.nodes as u32,
            images: m.images as u32,
            image_bytes: m.image_bytes as f64,
            fonts: m.fonts as u32,
            live_ids: self.ids.live(),
            animating: self.doc.is_animating(),
            malloc_bytes: Self::malloc_bytes(),
            ..self.stats
        }
    }

    pub(crate) fn scene(&mut self) -> crate::gpu::Scene {
        self.resolved();
        self.paint().0.to_scene(crate::gpu::Scene::TOLERANCE)
    }

    fn picker_scene(&mut self, scale: f64) -> Option<crate::gpu::Scene> {
        self.picker.as_ref()?;
        if !self.place_picker() {
            return None;
        }
        let mut scene = crate::gpu::Scene::new();
        blitz_paint::paint_select_picker(&mut scene, &self.doc, self.picker.as_ref()?, scale);
        Some(scene)
    }

    fn paint(&mut self) -> (Commands, Vec<(u32, kurbo::Rect, Commands)>) {
        let viewport = self.doc.viewport().clone();
        let (width, height) = viewport.window_size;
        let scale = viewport.scale_f64();
        let start = Instant::now();
        let mut loose = crate::gpu::Scene::new();
        let mut pieces = Vec::new();
        let pinned: std::collections::HashSet<NodeId> = self.panels.iter().map(|p| p.0).collect();
        let painted = blitz_paint::paint_scene_cached(&mut loose, &mut pieces, &mut self.doc, (scale, width, height), &mut self.paint_cache, true, &pinned);
        let mut commands = Commands::new(loose.commands, pieces);
        let mut picker = self.overlay(scale);
        if let Some(picker) = &mut picker {
            commands.extend(match self.verify {
                true => picker.clone(),
                false => std::mem::take(picker),
            });
        }
        let panels = self
            .panels
            .iter()
            .map(|&(node, id, rect)| {
                let shift = kurbo::Affine::translate((-rect.x0, -rect.y0));
                let pieces = painted
                    .iter()
                    .filter(|(n, _)| *n == node)
                    .flat_map(|(_, p)| p.iter().cloned())
                    .map(|p| blitz_paint::Piece { transform: shift * p.transform, ..p });
                (id, rect, Commands::new(Vec::new(), pieces.collect()))
            })
            .collect();
        let cache = self.paint_cache.stats;
        self.stats.fragments_recorded = cache.recorded;
        self.stats.fragments_reused = cache.reused;
        self.stats.paint_ms = crate::log::since_ms(start);
        let mut from_cache = true;
        if self.verify {
            let mut reference = crate::gpu::Scene::new();
            blitz_paint::paint_scene_reference(&mut reference, &mut self.doc, (scale, width, height), true, &pinned);
            if let Some(picker) = picker {
                reference.commands.extend(picker);
            }
            let scene = commands.to_scene(reference.tolerance);
            if let Some(m) = Mismatch::find(&scene, &reference, &self.paint_cache) {
                self.stats.paint_mismatches += 1;
                let node = m.node.map(|n| (n, self.ids.handle(n)));
                log::warn!(
                    "incremental and full-page recording differ: command {} (fragment {node:?}, of {} / {})\n  incremental: {}\n  full page: {}",
                    m.index,
                    scene.commands.len(),
                    reference.commands.len(),
                    m.cached,
                    m.reference,
                );
                commands = Commands::owned(reference.commands);
                from_cache = false;
            }
        }
        self.from_cache = from_cache;
        self.dirty = false;
        (commands, panels)
    }

    pub(crate) fn recover(&mut self) {
        self.stats.panics += 1;
        if catch_unwind(AssertUnwindSafe(|| self.doc.repair_after_panic())).is_err() {
            log::error!("panicked while repairing the document after a panic");
        }
        self.paint_cache.clear();
        self.damage = Damage::default();
        self.from_cache = false;
        self.stale = true;
        self.dirty = true;
    }

    pub(crate) fn is_dirty(&self) -> bool {
        self.dirty || self.stale || self.doc.is_animating()
    }

    pub(crate) fn invalidate(&mut self) {
        self.damage = Damage::default();
        self.touch();
    }

    pub(crate) fn drop_frame(&mut self) {
        self.stale = false;
        self.dirty = false;
    }

    pub(crate) fn tick(&mut self, now_ms: f64) {
        self.clock.tick(now_ms);
        if self.doc.is_animating() {
            self.touch();
        }
    }

    pub(crate) fn scroll_at(&mut self, x: f32, y: f32, dx: f64, dy: f64, at: f64) {
        let scale = self.doc.viewport().scale_f64();
        if self.picker.is_some() {
            match self.picker_option_at(f64::from(x) / scale, f64::from(y) / scale) {
                -2 => self.close_picker(),
                _ => return self.scroll_picker(dy / scale),
            }
        }
        let (x, y) = (x / scale as f32, y / scale as f32);
        let hit = self.resolved().hit(x, y).map(|h| h.node_id);
        let viewport = self.doc.viewport_scroll();
        let mut scrolled = Vec::new();
        let changed = match hit {
            Some(node) => self.doc.scroll_node_by_has_changed(node, -dx / scale, -dy / scale, |event| scrolled.push(event.target)),
            None => self.doc.scroll_viewport_by_has_changed(-dx / scale, -dy / scale),
        };
        if changed {
            self.dirty = true;
            self.trace = crate::log::Trace::earliest(self.trace, Some(crate::log::Trace::input("scroll", at)));
            let document = self.doc.root_node().id;
            self.ids.scrolls.entry(document).or_insert((viewport.x, viewport.y));
            let root = self.doc.try_root_element().map(|r| r.id);
            for node in scrolled.into_iter().filter(|&n| Some(n) != root) {
                self.ids.scrolls.entry(node).or_insert((f64::NAN, f64::NAN));
            }
        }
    }

    pub(crate) fn pointer(&mut self, x: f32, y: f32) {
        let scale = self.doc.viewport().scale_f64() as f32;
        self.resolved();
        if self.doc.set_hover_to(x / scale, y / scale) {
            self.touch();
        }
    }

    fn plane(&mut self, commands: Commands, scale: f64, viewport: kurbo::Rect) -> Plane {
        let start = Instant::now();
        let (rects, by_fragment, missed) =
            self.damage.update(&commands, crate::gpu::Scene::TOLERANCE, viewport, self.from_cache.then_some(&self.paint_cache), self.verify);
        self.stats.damage_mismatches += u32::from(missed);
        self.stats.damage_by_fragment += u32::from(by_fragment);
        self.stats.damage_ms = crate::log::since_ms(start);
        let start = Instant::now();
        let plane = self.damage.plane(commands, rects, scale, viewport);
        self.stats.layers_ms = crate::log::since_ms(start);
        plane
    }

    fn panel(&mut self, (id, rect, commands): (u32, kurbo::Rect, Commands), scale: f64) -> Panel {
        let size = (rect.width().round().max(1.0) as u32, rect.height().round().max(1.0) as u32);
        let viewport = kurbo::Rect::new(0.0, 0.0, size.0.into(), size.1.into());
        let damage = self.panel_damage.entry(id).or_default();
        let started = std::time::Instant::now();
        let (rects, _, _) = damage.update(&commands, crate::gpu::Scene::TOLERANCE, viewport, None, true);
        log::debug!(target: "perf::frame", "panel {id} commands={} damage ms={:.3}", commands.len(), crate::log::since_ms(started));
        let extent = damage
            .kinds()
            .iter()
            .zip(damage.draw())
            .filter(|(k, r)| **k == crate::render::damage::Kind::Draw && r.area() > 0.0)
            .map(|(_, r)| *r)
            .reduce(|a, b| a.union(b))
            .map(Rect::from);
        let reach = extent.map(|e| Rect { x: (rect.x0 + e.x) / scale, y: (rect.y0 + e.y) / scale, width: e.width / scale, height: e.height / scale });
        Panel { id, size, plane: damage.plane(commands, rects, scale, viewport), extent, reach }
    }

    pub(crate) fn paint_frame(&mut self) -> Painted {
        self.resolved();
        let (commands, panels) = self.paint();
        let scale = self.doc.viewport().scale_f64();
        Painted { commands, panels, scale, size: self.doc.viewport().window_size, animating: self.doc.is_animating(), commit: self.committed }
    }

    pub(crate) fn finish_frame(&mut self, painted: Painted) -> Frame {
        let Painted { commands, panels, scale, size: (width, height), animating, commit } = painted;
        let viewport = kurbo::Rect::new(0.0, 0.0, width.into(), height.into());
        let plane = self.plane(commands, scale, viewport);
        let panels = panels.into_iter().map(|p| self.panel(p, scale)).collect();
        let framed = crate::log::now_ms();
        Frame { plane, panels, size: (width, height), scale, animating, commit, trace: self.trace.take().map(|t| crate::log::Trace { framed, ..t }) }
    }

    pub(crate) fn tree(&mut self) -> Tree<'_> {
        self.touch();
        Tree { m: self.doc.mutate(), ids: &mut self.ids }
    }

    pub(crate) fn touch(&mut self) {
        self.stale = true;
        self.dirty = true;
    }

    pub(crate) fn take_arrived(&mut self) -> bool {
        let arrived = self.arrived.swap(false, Ordering::AcqRel);
        if arrived {
            self.touch();
        }
        arrived
    }

    pub(crate) fn frame_timed(&mut self, locked: f64, total: f64) {
        self.stats.frame_locked.add(locked);
        self.stats.frame.add(total);
    }

    pub(crate) fn reset_timings(&mut self) {
        self.stats.resolve = Default::default();
        self.stats.frame_locked = Default::default();
        self.stats.frame = Default::default();
    }

    fn animation_time(&mut self) -> f64 {
        let now = self.clock.now();
        let at = match (self.in_task, self.task_time) {
            (None, _) => now,
            (Some(task), Some((pinned, at))) if pinned == task => at,
            (Some(task), _) => {
                self.task_time = Some((task, now));
                now
            }
        };
        at.max(self.resolved_at)
    }

    pub(crate) fn resolved(&mut self) -> &BaseDocument {
        let now = self.animation_time();
        let animation_due = self.doc.is_animating() && now - self.resolved_at >= FRAME;
        if self.stale || animation_due {
            self.dirty |= self.doc.is_animating();
            let start = Instant::now();
            self.resolved_at = now;
            self.doc.resolve(self.resolved_at);
            self.stats.resolve_ms = crate::log::since_ms(start);
            self.stats.resolve.add(self.stats.resolve_ms);
            self.stats.resolves += 1;
            self.stale = false;
            if self.arrived.swap(false, Ordering::AcqRel) {
                self.stale = true;
                self.dirty = true;
            }
        }
        &self.doc
    }
}

pub(crate) struct Painted {
    commands: Commands,
    panels: Vec<(u32, kurbo::Rect, Commands)>,
    scale: f64,
    size: (u32, u32),
    animating: bool,
    commit: u64,
}

struct Clock {
    base: f64,
    anchor: Instant,
}

impl Default for Clock {
    fn default() -> Self {
        Self { base: 0.0, anchor: Instant::now() }
    }
}

impl Clock {
    fn now(&self) -> f64 {
        self.base + self.anchor.elapsed().as_secs_f64()
    }

    fn tick(&mut self, now_ms: f64) {
        self.base = (now_ms / 1e3).max(self.now());
        self.anchor = Instant::now();
    }
}
