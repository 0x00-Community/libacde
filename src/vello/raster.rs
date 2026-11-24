use std::collections::{HashMap, VecDeque};
use std::os::fd::OwnedFd;
use std::sync::{Arc, Weak};
use std::time::{Duration, Instant};

use super::{Composer, Device, GpuImage, Hole, Kept, Op, Overrides, Shared, Vello};
use crate::gpu::{Backdrop, Content, Dmabuf, Paint, Part, Pass, Px, RenderCommand, Scene, Submitted};
use kurbo::{Affine, Rect, Shape};
use peniko::Color;

const ATLAS: u32 = 4096;
const LEAST_HOLE: i64 = 64 * 64;
const MOST_AREAS: usize = 32;
const KEEP_FRAMES: u64 = 8;

type Key = (usize, usize, Px);

enum Step<'p, C> {
    Clear {
        canvas: wgpu::TextureView,
        regions: Option<Vec<Px>>,
    },
    Backdrop {
        canvas: wgpu::TextureView,
        backdrop: &'p Backdrop,
    },
    Piece {
        canvas: wgpu::TextureView,
        part: &'p Part<C>,
        drawn: Px,
        clip: Px,
        at: [u32; 2],
        scene: Option<Scene>,
        above: Vec<(Scene, Px)>,
        cover: bool,
        keep: Option<Key>,
    },
    Blit {
        canvas: wgpu::TextureView,
        hole: Hole,
        clip: Px,
    },
    Kept {
        canvas: wgpu::TextureView,
        key: Key,
    },
}

struct Cache {
    kept: Kept,
    image: Option<(wgpu::Texture, wgpu::TextureView)>,
    bounds: Px,
    areas: Vec<Px>,
    used: u64,
}

#[derive(Default)]
struct Plan {
    bases: Vec<(usize, Px)>,
    between: Vec<Px>,
    written: Vec<Px>,
    reads: Vec<Px>,
    covered: Vec<Px>,
}

#[derive(Default)]
struct Spent {
    pieces: usize,
    merged: usize,
    batches: usize,
    area: u64,
    extent: u64,
    bound: (u32, u32),
    blits: usize,
    kept: usize,
    covers: usize,
    scenes: Duration,
    vello: Duration,
    compose: Duration,
}

impl<C> Step<'_, C> {
    fn size(drawn: Px) -> (u32, u32) {
        (drawn.w.max(0) as u32, drawn.h.max(0) as u32)
    }
}

struct Atlas {
    bound: (u32, u32),
    used: (u32, u32),
    x: u32,
    y: u32,
    row: u32,
}

impl Atlas {
    fn new(bound: (u32, u32)) -> Self {
        Self { bound, used: (0, 0), x: 0, y: 0, row: 0 }
    }

    fn place(&mut self, (w, h): (u32, u32)) -> Option<[u32; 2]> {
        if self.x + w > self.bound.0 {
            (self.x, self.y, self.row) = (0, self.y + self.row, 0);
        }
        if self.x + w > self.bound.0 || self.y + h > self.bound.1 {
            return None;
        }
        let at = [self.x, self.y];
        self.x += w;
        self.row = self.row.max(h);
        self.used = (self.used.0.max(self.x), self.used.1.max(self.y + h));
        Some(at)
    }
}

pub struct Raster {
    pub device: Device,
    vello: Vello,
    composer: Composer,
    canvases: HashMap<usize, Vec<Option<(wgpu::Texture, wgpu::TextureView)>>>,
    submits: VecDeque<(wgpu::SubmissionIndex, Option<OwnedFd>)>,
    poll: bool,
    spent: Spent,
    reads_layer: bool,
    kept: HashMap<Key, Cache>,
    views: HashMap<u64, (Weak<GpuImage>, wgpu::TextureView)>,
    blitted: Vec<(Arc<GpuImage>, u64)>,
    frame: u64,
}

impl Raster {
    pub fn new(device: Device, poll: bool) -> Result<Self, String> {
        let mut vello = Vello::new(&device.device, &device.queue)?;
        let composer = Composer::new(&device.device, &device.queue);
        let view = super::texture(&device.device, (64, 64), wgpu::TextureUsages::STORAGE_BINDING, "acde warm-up").create_view(&Default::default());
        let _ = vello.render([], &mut Overrides::from(Arc::default()), &view, (64, 64), Color::TRANSPARENT, None);
        let _ = device.device.poll(wgpu::PollType::wait_indefinitely());
        Ok(Self {
            device,
            vello,
            composer,
            canvases: HashMap::new(),
            submits: VecDeque::new(),
            poll,
            spent: Spent::default(),
            reads_layer: false,
            kept: HashMap::new(),
            views: HashMap::new(),
            blitted: Vec::new(),
            frame: 0,
        })
    }

    pub fn canvases(&mut self, plane: usize, dmabufs: &[Dmabuf<'_>], size: (u32, u32)) -> Vec<Result<(), String>> {
        let mut results = Vec::new();
        let canvases = dmabufs
            .iter()
            .map(|d| {
                let imported = self.device.import(*d, size).map(|t| {
                    let view = t.create_view(&wgpu::TextureViewDescriptor::default());
                    (t, view)
                });
                results.push(imported.as_ref().map(|_| ()).map_err(Clone::clone));
                imported.ok()
            })
            .collect();
        self.canvases.insert(plane, canvases);
        results
    }

    pub fn release(&mut self, plane: usize) {
        self.canvases.remove(&plane);
        self.kept.retain(|(p, ..), _| *p != plane);
    }

    pub fn idle(&mut self) {
        self.kept.clear();
        self.views.clear();
        self.composer.trim();
        if let Err(e) = self.vello.trim() {
            log::warn!("web layer: dropping vello's buffers: {e}");
        }
    }

    pub fn draw<C: Content>(&mut self, passes: Vec<Pass<C>>, images: &mut Overrides, lock: Option<&Shared>) -> Result<Submitted, String> {
        let started = Instant::now();
        let oldest = if self.submits.len() > 1 { self.submits.pop_front() } else { None };
        let poll = match oldest {
            Some((_, Some(fence))) => {
                let mut fds = [rustix::event::PollFd::new(&fence, rustix::event::PollFlags::IN)];
                let _ = rustix::event::poll(&mut fds, Some(&rustix::event::Timespec { tv_sec: 1, tv_nsec: 0 }));
                wgpu::PollType::Poll
            }
            Some((index, None)) => wgpu::PollType::Wait { submission_index: Some(index), timeout: Some(Duration::from_secs(1)) },
            None => wgpu::PollType::Poll,
        };
        let _ = self.device.device.poll(poll);
        let waited = started.elapsed();
        let mut encoder = self.device.device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("acde page") });
        self.spent = Spent::default();
        self.frame += 1;
        if let Err(e) = self.draw_passes(&mut encoder, &passes, images) {
            self.kept.clear();
            self.blitted.clear();
            return Err(e);
        }
        let guard = lock.map(Shared::submit_lock);
        let fence = if self.poll { None } else { self.device.arm() };
        let index = self.submit(&mut encoder);
        drop(guard);
        for (image, generation) in self.blitted.drain(..) {
            image.mark_copied(generation);
        }
        let frame = self.frame;
        self.kept.retain(|_, c| c.used + KEEP_FRAMES > frame);
        self.views.retain(|_, (image, _)| image.strong_count() > 0);
        let recorded = started.elapsed() - waited;
        let ms = |d: Duration| d.as_secs_f64() * 1000.0;
        let spent = &self.spent;
        log::debug!(
            target: "perf::raster",
            "pieces={} merged={} blits={} kept={} covers={} area={} extent={} bound={:?} batches={} scenes={:.3} vello={:.3} compose={:.3}",
            spent.pieces,
            spent.merged,
            spent.blits,
            spent.kept,
            spent.covers,
            spent.area,
            spent.extent,
            spent.bound,
            spent.batches,
            ms(spent.scenes),
            ms(spent.vello),
            ms(spent.compose)
        );
        self.composer.end_frame();
        let fence = match fence.and_then(|f| self.device.sync_file(f)) {
            Some(fd) => {
                self.submits.push_back((index, fd.try_clone().ok()));
                Some(fd)
            }
            None => {
                self.submits.push_back((index, None));
                self.device.device.poll(wgpu::PollType::wait_indefinitely()).map_err(|e| format!("waiting for the GPU: {e}"))?;
                None
            }
        };
        Ok(Submitted { fence, waited, recorded })
    }

    fn submit(&mut self, encoder: &mut wgpu::CommandEncoder) -> wgpu::SubmissionIndex {
        self.reads_layer = false;
        self.composer.flush();
        let done = std::mem::replace(encoder, self.device.device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("acde page") }));
        self.device.queue.submit([done.finish()])
    }

    fn draw_passes<C: Content>(&mut self, encoder: &mut wgpu::CommandEncoder, passes: &[Pass<C>], images: &mut Overrides) -> Result<(), String> {
        let most = self.device.device.limits().max_texture_dimension_2d;
        let bound = passes.iter().fold((ATLAS, ATLAS), |(w, h), p| (w.max(p.size.0), h.max(p.size.1)));
        let bound = (bound.0.min(most), bound.1.min(most));
        self.spent.bound = bound;
        let mut steps = Vec::new();
        for pass in passes {
            let canvas = self.canvases.get(&pass.plane).and_then(|c| c.get(pass.next)?.clone()).ok_or("the canvas is not imported yet")?.1;
            let full = Px::sized(pass.size);
            let cleared = steps.len();
            steps.push(Step::Clear { canvas: canvas.clone(), regions: None });
            let whole = [full];
            let targets = pass.targets.as_deref().unwrap_or(&whole);
            let mut plan = Plan::default();
            for (k, part) in pass.parts.iter().enumerate() {
                let plain = part.backdrop.is_none() && part.shadows.is_empty() && part.effect.is_none();
                if let Some(backdrop) = part.backdrop.as_ref().filter(|b| targets.iter().any(|t| Px::covering(b.rect).overlaps(*t))) {
                    let reach = Px::covering(backdrop.rect).grown((3.0 * backdrop.effect.blur).ceil() as i32);
                    plan.between.push(reach);
                    plan.written.push(reach.grown(backdrop.glass.map_or(0.0, |g| g.reach()).ceil() as i32));
                    plan.reads.push(*plan.written.last().expect("just pushed"));
                    steps.push(Step::Backdrop { canvas: canvas.clone(), backdrop });
                }
                for (drawn, clip) in part.pieces(targets, full) {
                    let built = Instant::now();
                    let scene = part.content.scene_for(drawn);
                    self.spent.scenes += built.elapsed();
                    if scene.commands.is_empty() {
                        continue;
                    }
                    if plain && let Some(hole) = Hole::largest(&scene, images.images()) {
                        let inner = Px::of(hole.rect)
                            .intersection(clip)
                            .and_then(|i| i.intersection(drawn))
                            .filter(|i| i.area() >= LEAST_HOLE && i.area() * 2 >= drawn.area());
                        if let Some(inner) = inner.filter(|_| hole.blends_alone(&scene)) {
                            self.cut_hole(&mut steps, &mut plan, &canvas, (pass.plane, k, part), (drawn, clip, inner), scene, hole, images);
                            continue;
                        }
                    }
                    self.piece(&mut steps, &mut plan, &canvas, part, (drawn, clip), scene, None);
                }
            }
            let regions: Vec<Px> = plan.covered.iter().fold(targets.to_vec(), |left, c| left.into_iter().flat_map(|r| subtract(r, *c)).collect());
            if !plan.covered.is_empty() || pass.targets.is_some() {
                steps[cleared] = Step::Clear { canvas: canvas.clone(), regions: Some(regions) };
            }
        }
        if let Some(used) = Self::pack_tallest_first(&mut steps, bound) {
            return self.run(encoder, &mut steps, used, images);
        }
        let mut atlas = Atlas::new(bound);
        let mut batch = Vec::new();
        for mut step in steps {
            if let Step::Piece { drawn, at, .. } = &mut step {
                let size = Step::<C>::size(*drawn);
                *at = match atlas.place(size) {
                    Some(placed) => placed,
                    None => {
                        self.run(encoder, &mut batch, atlas.used, images)?;
                        atlas = Atlas::new(bound);
                        atlas.place(size).ok_or_else(|| format!("a piece of {}x{} is larger than the layer", size.0, size.1))?
                    }
                };
            }
            batch.push(step);
        }
        self.run(encoder, &mut batch, atlas.used, images)
    }

    #[allow(clippy::too_many_arguments, reason = "a piece of a part, as the plan files it")]
    fn piece<'p, C>(
        &mut self,
        steps: &mut Vec<Step<'p, C>>,
        plan: &mut Plan,
        canvas: &wgpu::TextureView,
        part: &'p Part<C>,
        (drawn, clip): (Px, Px),
        scene: Scene,
        keep: Option<Key>,
    ) {
        let plain = part.backdrop.is_none() && part.shadows.is_empty() && part.effect.is_none();
        if keep.is_none() {
            let base = plan.bases.iter().rposition(|(_, b)| b.intersection(drawn) == Some(drawn));
            let free = |at: usize| plain && !plan.between.iter().any(|e| e.overlaps(drawn)) && !plan.bases[at + 1..].iter().any(|(_, b)| b.overlaps(drawn));
            if let Some(Step::Piece { above, .. }) = base.filter(|at| free(*at)).map(|at| &mut steps[plan.bases[at].0]) {
                above.push((scene, clip));
                self.spent.merged += 1;
                return;
            }
        }
        let cover = plain && !plan.written.iter().any(|w| w.overlaps(clip));
        if cover {
            plan.covered.push(clip);
            self.spent.covers += 1;
        }
        if plain && plan.between.is_empty() && keep.is_none() {
            plan.bases.push((steps.len(), drawn));
        } else {
            plan.between.push(drawn);
        }
        plan.written.push(drawn);
        self.spent.area += u64::from(Step::<C>::size(drawn).0) * u64::from(Step::<C>::size(drawn).1);
        steps.push(Step::Piece { canvas: canvas.clone(), part, drawn, clip, at: [0, 0], scene: Some(scene), above: Vec::new(), cover, keep });
    }

    #[allow(clippy::too_many_arguments, reason = "the piece, the hole cut into it and where it goes")]
    fn cut_hole<'p, C: Content>(
        &mut self,
        steps: &mut Vec<Step<'p, C>>,
        plan: &mut Plan,
        canvas: &wgpu::TextureView,
        (plane, index, part): (usize, usize, &'p Part<C>),
        (drawn, clip, inner): (Px, Px, Px),
        scene: Scene,
        hole: Hole,
        images: &Overrides,
    ) {
        let key = (plane, index, inner);
        let frame = self.frame;
        let set = images.images();
        let hit = self
            .kept
            .get_mut(&key)
            .filter(|c| (c.image.is_some() || c.areas.is_empty()) && c.kept.matches(&scene, &hole, set))
            .map(|c| c.used = frame)
            .is_some();
        let above = match hit {
            true => None,
            false => {
                let above = hole.above(&scene);
                let areas = above.as_ref().map_or_else(Vec::new, |a| reach(a, inner));
                let bounds = areas.iter().copied().reduce(Px::union).unwrap_or_default();
                let kept = hole.keep(&scene, set);
                let image = self.kept.remove(&key).and_then(|c| c.image).filter(|(t, _)| (t.width(), t.height()) == Step::<C>::size(bounds));
                self.kept.insert(key, Cache { kept, image, bounds, areas, used: frame });
                above.filter(|_| bounds.area() > 0).map(|a| (a, bounds))
            }
        };
        let image = hole.image.clone();
        if !plan.reads.iter().any(|r| r.overlaps(inner)) {
            plan.covered.push(inner);
        }
        plan.between.push(inner);
        plan.written.push(inner);
        self.spent.blits += 1;
        steps.push(Step::Blit { canvas: canvas.clone(), hole, clip: inner });
        if hit && self.kept.get(&key).is_some_and(|c| !c.areas.is_empty()) {
            self.spent.kept += 1;
            steps.push(Step::Kept { canvas: canvas.clone(), key });
        }
        if let Some((above, bounds)) = above {
            self.piece(steps, plan, canvas, part, (bounds, bounds), above, Some(key));
        }
        for ring in subtract(drawn, inner) {
            let Some(ring_clip) = ring.intersection(clip) else { continue };
            let mut rest = part.content.scene_for(ring);
            rest.commands.retain(|c| !matches!(c, RenderCommand::Fill(f) if matches!(&f.brush, Paint::Image(b) if b.image.data.id() == image.id())));
            if !rest.commands.is_empty() {
                self.piece(steps, plan, canvas, part, (ring, ring_clip), rest, None);
            }
        }
    }

    fn view(&mut self, image: &Arc<GpuImage>) -> wgpu::TextureView {
        self.views.entry(image.id()).or_insert_with(|| (Arc::downgrade(image), image.texture.create_view(&wgpu::TextureViewDescriptor::default()))).1.clone()
    }

    fn pack_tallest_first<C>(steps: &mut [Step<'_, C>], bound: (u32, u32)) -> Option<(u32, u32)> {
        let mut order: Vec<(usize, (u32, u32))> = steps
            .iter()
            .enumerate()
            .filter_map(|(i, step)| match step {
                Step::Piece { drawn, .. } => Some((i, Step::<C>::size(*drawn))),
                _ => None,
            })
            .collect();
        order.sort_by_key(|(_, (w, h))| std::cmp::Reverse((*h, *w)));
        let mut atlas = Atlas::new(bound);
        let mut placed = Vec::with_capacity(order.len());
        for (i, size) in order {
            placed.push((i, atlas.place(size)?));
        }
        for (i, spot) in placed {
            if let Step::Piece { at, .. } = &mut steps[i] {
                *at = spot;
            }
        }
        Some(atlas.used)
    }

    fn run<C: Content>(
        &mut self,
        encoder: &mut wgpu::CommandEncoder,
        steps: &mut Vec<Step<'_, C>>,
        used: (u32, u32),
        images: &mut Overrides,
    ) -> Result<(), String> {
        let layer = self.composer.layer_view((used.0.max(1), used.1.max(1)));
        let mut scenes = Vec::new();
        for step in steps.iter_mut() {
            let Step::Piece { drawn, at, scene, above, .. } = step else { continue };
            let (dx, dy) = (f64::from(at[0]) - f64::from(drawn.x), f64::from(at[1]) - f64::from(drawn.y));
            let cell = |r: Px| Rect::new(f64::from(r.x) + dx, f64::from(r.y) + dy, f64::from(r.x + r.w) + dx, f64::from(r.y + r.h) + dy);
            if let Some(scene) = scene.take() {
                scenes.push((scene, Affine::translate((dx, dy)), Some(cell(*drawn))));
            }
            for (scene, clip) in above.drain(..) {
                scenes.push((scene, Affine::translate((dx, dy)), Some(cell(clip))));
            }
        }
        if let [(_, _, clip)] = &mut scenes[..]
            && clip.is_some_and(|c| c.x0 <= 0.0 && c.y0 <= 0.0 && c.x1 >= f64::from(used.0) && c.y1 >= f64::from(used.1))
        {
            *clip = None;
        }
        let scenes_empty = scenes.is_empty();
        if !scenes_empty {
            let rendered = Instant::now();
            self.spent.pieces += scenes.len();
            self.spent.batches += 1;
            self.spent.extent += u64::from(used.0) * u64::from(used.1);
            if self.reads_layer {
                self.submit(encoder);
            }
            self.vello.render_clipped(scenes, images, &layer, used, Color::TRANSPARENT, None)?;
            self.spent.vello += rendered.elapsed();
        }
        let composed = Instant::now();
        self.reads_layer |= !scenes_empty;
        let mut places: Vec<Op> = Vec::new();
        let mut on: Option<wgpu::TextureView> = None;
        for step in steps.drain(..) {
            let mergeable =
                matches!(&step, Step::Piece { part, canvas, .. } if part.shadows.is_empty() && part.effect.is_none() && on.as_ref() == Some(canvas));
            if !mergeable && let Some(canvas) = on.take() {
                self.composer.draw(encoder, &canvas, &places);
                places.clear();
            }
            match step {
                Step::Clear { canvas, regions: Some(regions) } => self.composer.clear_rects(encoder, &canvas, regions.iter().map(|r| r.xywh())),
                Step::Clear { canvas, regions: None } => self.composer.clear(encoder, &canvas),
                Step::Backdrop { canvas, backdrop } => self.composer.backdrop(encoder, &canvas, backdrop),
                Step::Blit { canvas, hole, clip } => {
                    let view = self.view(&hole.image);
                    let extent = [hole.image.texture.width() as f32, hole.image.texture.height() as f32];
                    let map = hole.map.map(|v| v as f32);
                    self.composer.opaque_from(encoder, &canvas, &view, map, extent, hole.smooth, clip.xywh());
                    self.blitted.push((hole.image.clone(), hole.image.generation()));
                }
                Step::Kept { canvas, key } => {
                    if let Some(Cache { image: Some((_, view)), bounds, areas, .. }) = self.kept.get(&key) {
                        let clips: Vec<[u32; 4]> = areas.iter().map(|a| a.xywh()).collect();
                        let view = view.clone();
                        self.composer.place_areas_from(encoder, &canvas, &view, [bounds.x.max(0) as u32, bounds.y.max(0) as u32], &clips);
                    }
                }
                Step::Piece { canvas, part, drawn, clip, at, cover, keep, .. } => {
                    if let Some(key) = keep {
                        self.keep(encoder, key, at, drawn);
                    }
                    for shadow in &part.shadows {
                        self.composer.shadow(encoder, &canvas, shadow, drawn.xywh(), at, clip.xywh());
                    }
                    if part.effect.is_none() {
                        let (origin, clip) = ([drawn.x.max(0) as u32, drawn.y.max(0) as u32], clip.xywh());
                        places.push(if cover { Op::Cover { origin, at, clip } } else { Op::Place { origin, at, clip } });
                        on = Some(canvas);
                    } else {
                        self.composer.filtered(encoder, &canvas, &part.effect, drawn.xywh(), at, clip.xywh());
                    }
                }
            }
        }
        if let Some(canvas) = on {
            self.composer.draw(encoder, &canvas, &places);
        }
        self.spent.compose += composed.elapsed();
        Ok(())
    }

    fn keep(&mut self, encoder: &mut wgpu::CommandEncoder, key: Key, at: [u32; 2], drawn: Px) {
        let Some(layer) = self.composer.layer_texture() else { return };
        let Some(cache) = self.kept.get_mut(&key) else { return };
        let size = Step::<()>::size(drawn);
        let (texture, _) = cache.image.get_or_insert_with(|| {
            let usage = wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST;
            let texture = super::texture(&self.device.device, size, usage, "acde kept above");
            let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
            (texture, view)
        });
        encoder.copy_texture_to_texture(
            wgpu::TexelCopyTextureInfo { texture: &layer, mip_level: 0, origin: wgpu::Origin3d { x: at[0], y: at[1], z: 0 }, aspect: wgpu::TextureAspect::All },
            texture.as_image_copy(),
            wgpu::Extent3d { width: size.0, height: size.1, depth_or_array_layers: 1 },
        );
    }

    pub fn icon(&mut self, dmabuf: Dmabuf<'_>, (width, height): (u32, u32), scene: Scene, images: &mut Overrides) -> Result<(), String> {
        let texture = self.device.import(dmabuf, (width, height))?;
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let layer = self.composer.layer_view((width, height));
        self.vello.render([(scene, Affine::IDENTITY)], images, &layer, (width, height), Color::TRANSPARENT, None)?;
        let mut encoder = self.device.device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("acde icon") });
        self.composer.clear(&mut encoder, &view);
        self.composer.place(&mut encoder, &view, [0, 0], [0, 0], [0, 0, width, height]);
        let index = self.submit(&mut encoder);
        self.composer.end_frame();
        self.device
            .device
            .poll(wgpu::PollType::Wait { submission_index: Some(index), timeout: Some(Duration::from_secs(2)) })
            .map(|_| ())
            .map_err(|e| format!("waiting for the GPU: {e}"))
    }
}

fn subtract(r: Px, cut: Px) -> Vec<Px> {
    let Some(i) = r.intersection(cut) else { return vec![r] };
    [
        Px { x: r.x, y: r.y, w: r.w, h: i.y - r.y },
        Px { x: r.x, y: i.y + i.h, w: r.w, h: r.y + r.h - (i.y + i.h) },
        Px { x: r.x, y: i.y, w: i.x - r.x, h: i.h },
        Px { x: i.x + i.w, y: i.y, w: r.x + r.w - (i.x + i.w), h: i.h },
    ]
    .into_iter()
    .filter(|p| p.w > 0 && p.h > 0)
    .collect()
}

fn reach(scene: &Scene, within: Px) -> Vec<Px> {
    let bound = Rect::new(f64::from(within.x), f64::from(within.y), f64::from(within.x + within.w), f64::from(within.y + within.h));
    let mut stack = vec![bound];
    let mut found = Vec::new();
    for command in &scene.commands {
        let top = *stack.last().expect("the bottom is never popped");
        match command {
            RenderCommand::PushLayer(l) => stack.push(l.transform.transform_rect_bbox(l.clip.bounding_box()).intersect(top)),
            RenderCommand::PushClipLayer(c) => stack.push(c.transform.transform_rect_bbox(c.clip.bounding_box()).intersect(top)),
            RenderCommand::PopLayer => {
                if stack.len() > 1 {
                    stack.pop();
                }
            }
            drawn => {
                let r = crate::render::draw_bounds(drawn, Affine::IDENTITY).inflate(2.0, 2.0).intersect(top).expand();
                if r.area() > 0.0 {
                    found.extend(Px::covering([r.x0 as f32, r.y0 as f32, r.width() as f32, r.height() as f32]).intersection(within));
                }
            }
        }
    }
    disjoint(found)
}

fn disjoint(mut rects: Vec<Px>) -> Vec<Px> {
    loop {
        let mut out: Vec<Px> = Vec::with_capacity(rects.len());
        for mut r in rects {
            while let Some(k) = out.iter().position(|o| o.overlaps(r)) {
                r = r.union(out.swap_remove(k));
            }
            out.push(r);
        }
        if out.len() <= MOST_AREAS {
            return out;
        }
        let waste = |a: Px, b: Px| a.union(b).area() - a.area() - b.area();
        let (i, j) =
            (0..out.len()).flat_map(|i| (i + 1..out.len()).map(move |j| (i, j))).min_by_key(|&(i, j)| waste(out[i], out[j])).expect("more than one rectangle");
        let taken = out.swap_remove(j);
        out[i] = out[i].union(taken);
        rects = out;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_cut_leaves_what_is_outside_it() {
        let r = Px { x: 0, y: 0, w: 100, h: 100 };
        let parts = subtract(r, Px { x: 10, y: 20, w: 30, h: 40 });
        assert_eq!(parts.iter().map(|p| p.area()).sum::<i64>(), 100 * 100 - 30 * 40);
        assert!(subtract(r, r).is_empty());
        assert_eq!(subtract(r, Px { x: 200, y: 0, w: 5, h: 5 }), vec![r]);
    }

    #[test]
    fn areas_do_not_overlap() {
        let rects: Vec<Px> = (0..80).map(|i| Px { x: (i * 37) % 900, y: (i * 53) % 700, w: 60, h: 40 }).collect();
        let out = disjoint(rects.clone());
        assert!(out.len() <= MOST_AREAS);
        for (i, a) in out.iter().enumerate() {
            assert!(out[i + 1..].iter().all(|b| !a.overlaps(*b)));
        }
        assert!(rects.iter().all(|r| out.iter().any(|o| o.contains(*r))));
    }
}
