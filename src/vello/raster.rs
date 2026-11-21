use std::collections::{HashMap, VecDeque};
use std::os::fd::OwnedFd;
use std::sync::Arc;
use std::time::{Duration, Instant};

use super::{Composer, Device, Op, Overrides, Shared, Vello};
use crate::gpu::{Backdrop, Content, Dmabuf, Part, Pass, Px, Scene, Submitted};
use kurbo::{Affine, Rect};
use peniko::Color;

const ATLAS: u32 = 4096;

enum Step<'p, C> {
    Clear { canvas: wgpu::TextureView, regions: Option<&'p [Px]> },
    Backdrop { canvas: wgpu::TextureView, backdrop: &'p Backdrop },
    Piece { canvas: wgpu::TextureView, part: &'p Part<C>, drawn: Px, clip: Px, at: [u32; 2], scene: Option<Scene>, above: Vec<(Scene, Px)> },
}

#[derive(Default)]
struct Spent {
    pieces: usize,
    merged: usize,
    batches: usize,
    area: u64,
    extent: u64,
    bound: (u32, u32),
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
}

impl Raster {
    pub fn new(device: Device, poll: bool) -> Result<Self, String> {
        let mut vello = Vello::new(&device.device, &device.queue)?;
        let composer = Composer::new(&device.device, &device.queue);
        let view = super::texture(&device.device, (64, 64), wgpu::TextureUsages::STORAGE_BINDING, "acde warm-up").create_view(&Default::default());
        let _ = vello.render([], &mut Overrides::from(Arc::default()), &view, (64, 64), Color::TRANSPARENT, None);
        let _ = device.device.poll(wgpu::PollType::wait_indefinitely());
        Ok(Self { device, vello, composer, canvases: HashMap::new(), submits: VecDeque::new(), poll, spent: Spent::default(), reads_layer: false })
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
    }

    pub fn idle(&mut self) {
        self.composer.trim();
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
        self.draw_passes(&mut encoder, &passes, images)?;
        let guard = lock.map(Shared::submit_lock);
        let fence = if self.poll { None } else { self.device.arm() };
        let index = self.submit(&mut encoder);
        drop(guard);
        let recorded = started.elapsed() - waited;
        let ms = |d: Duration| d.as_secs_f64() * 1000.0;
        let spent = &self.spent;
        log::debug!(
            target: "perf::raster",
            "pieces={} merged={} area={} extent={} bound={:?} batches={} scenes={:.3} vello={:.3} compose={:.3}",
            spent.pieces,
            spent.merged,
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
            steps.push(Step::Clear { canvas: canvas.clone(), regions: pass.targets.as_deref() });
            let whole = [full];
            let targets = pass.targets.as_deref().unwrap_or(&whole);
            let mut bases: Vec<(usize, Px)> = Vec::new();
            let mut between: Vec<Px> = Vec::new();
            for part in &pass.parts {
                let plain = part.backdrop.is_none() && part.shadows.is_empty() && part.effect.is_none();
                if let Some(backdrop) = part.backdrop.as_ref().filter(|b| targets.iter().any(|t| Px::covering(b.rect).overlaps(*t))) {
                    between.push(Px::covering(backdrop.rect).grown((3.0 * backdrop.effect.blur).ceil() as i32));
                    steps.push(Step::Backdrop { canvas: canvas.clone(), backdrop });
                }
                for (drawn, clip) in part.pieces(targets, full) {
                    let built = Instant::now();
                    let scene = part.content.scene_for(drawn);
                    self.spent.scenes += built.elapsed();
                    if scene.commands.is_empty() {
                        continue;
                    }
                    let base = bases.iter().rposition(|(_, b)| b.intersection(drawn) == Some(drawn));
                    let free = |at: usize| plain && !between.iter().any(|e| e.overlaps(drawn)) && !bases[at + 1..].iter().any(|(_, b)| b.overlaps(drawn));
                    if let Some(Step::Piece { above, .. }) = base.filter(|at| free(*at)).map(|at| &mut steps[bases[at].0]) {
                        above.push((scene, clip));
                        self.spent.merged += 1;
                        continue;
                    }
                    if plain && between.is_empty() {
                        bases.push((steps.len(), drawn));
                    } else {
                        between.push(drawn);
                    }
                    self.spent.area += u64::from(Step::<C>::size(drawn).0) * u64::from(Step::<C>::size(drawn).1);
                    steps.push(Step::Piece { canvas: canvas.clone(), part, drawn, clip, at: [0, 0], scene: Some(scene), above: Vec::new() });
                }
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
                Step::Piece { canvas, part, drawn, clip, at, .. } => {
                    for shadow in &part.shadows {
                        self.composer.shadow(encoder, &canvas, shadow, drawn.xywh(), at, clip.xywh());
                    }
                    if part.effect.is_none() {
                        places.push(Op::Place { origin: [drawn.x.max(0) as u32, drawn.y.max(0) as u32], at, clip: clip.xywh() });
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
