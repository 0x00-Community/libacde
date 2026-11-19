use std::collections::{HashMap, VecDeque};
use std::os::fd::OwnedFd;
use std::sync::Arc;
use std::time::{Duration, Instant};

use super::{Composer, Device, Overrides, Shared, Vello};
use crate::gpu::{Content, Dmabuf, Pass, Px, Scene, Submitted};
use kurbo::Affine;
use peniko::Color;

pub struct Raster {
    pub device: Device,
    vello: Vello,
    composer: Composer,
    canvases: HashMap<usize, Vec<Option<(wgpu::Texture, wgpu::TextureView)>>>,
    submits: VecDeque<(wgpu::SubmissionIndex, Option<OwnedFd>)>,
    poll: bool,
}

impl Raster {
    pub fn new(device: Device, poll: bool) -> Result<Self, String> {
        let mut vello = Vello::new(&device.device, &device.queue)?;
        let composer = Composer::new(&device.device, &device.queue);
        let view = super::texture(&device.device, (64, 64), wgpu::TextureUsages::STORAGE_BINDING, "acde warm-up").create_view(&Default::default());
        let _ = vello.render([], &mut Overrides::from(Arc::default()), &view, (64, 64), Color::TRANSPARENT, None);
        let _ = device.device.poll(wgpu::PollType::wait_indefinitely());
        Ok(Self { device, vello, composer, canvases: HashMap::new(), submits: VecDeque::new(), poll })
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
        for pass in passes {
            self.draw_pass(&mut encoder, pass, images)?;
        }
        let guard = lock.map(Shared::submit_lock);
        let fence = if self.poll { None } else { self.device.arm() };
        let index = self.submit(&mut encoder);
        drop(guard);
        let recorded = started.elapsed() - waited;
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
        self.composer.flush();
        let done = std::mem::replace(encoder, self.device.device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("acde page") }));
        self.device.queue.submit([done.finish()])
    }

    fn draw_pass<C: Content>(
        &mut self,
        encoder: &mut wgpu::CommandEncoder,
        Pass { plane, size, parts, next, targets }: Pass<C>,
        images: &mut Overrides,
    ) -> Result<(), String> {
        let full = Px::sized(size);
        let (_, canvas) = self.canvases.get(&plane).and_then(|c| c.get(next)?.clone()).ok_or("the canvas is not imported yet")?;
        let layer = self.composer.layer_view(size);
        match &targets {
            Some(regions) => self.composer.clear_rects(encoder, &canvas, regions.iter().map(|r| r.xywh())),
            None => self.composer.clear(encoder, &canvas),
        }
        let targets = targets.unwrap_or_else(|| vec![full]);
        for part in parts {
            if let Some(backdrop) = part.backdrop.as_ref().filter(|b| targets.iter().any(|t| Px::covering(b.rect).overlaps(*t))) {
                self.composer.backdrop(encoder, &canvas, backdrop);
            }
            for (drawn, clip) in part.pieces(&targets, full) {
                let [ox, oy, width, height] = drawn.xywh();
                let scene = part.content.scene_for(drawn);
                if scene.commands.is_empty() {
                    continue;
                }
                self.submit(encoder);
                self.vello.render([(scene, Affine::translate((-f64::from(ox), -f64::from(oy))))], images, &layer, (width, height), Color::TRANSPARENT, None)?;
                for shadow in &part.shadows {
                    self.composer.shadow(encoder, &canvas, shadow, drawn.xywh(), [0, 0], clip.xywh());
                }
                match part.effect.is_none() {
                    true => self.composer.place(encoder, &canvas, [ox, oy], [0, 0], clip.xywh()),
                    false => self.composer.filtered(encoder, &canvas, &part.effect, drawn.xywh(), [0, 0], clip.xywh()),
                }
            }
        }
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
