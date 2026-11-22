use std::sync::Arc;
use std::time::{Duration, Instant};

use acde::gpu::kurbo::{Affine, Circle, Rect};
use acde::gpu::peniko::{Color, Fill};
use acde::gpu::{Gpu, GpuImage, PaintScene as _, Scene, wgpu};
use acde::vello::{Overrides, Vello};

const W: u32 = 1280;
const H: u32 = 800;
const SIDE: u32 = 64;

struct Bench {
    gpu: Arc<Gpu>,
    vello: Vello,
    overrides: Overrides,
    target: wgpu::Texture,
    image: Arc<GpuImage>,
}

impl Bench {
    fn new() -> Self {
        let (_, _, gpu) = Gpu::open("acde trim test").expect("a GPU");
        let vello = Vello::new(&gpu.device, &gpu.queue).expect("vello");
        let overrides = Overrides::from(gpu.images.clone());
        let usage = wgpu::TextureUsages::STORAGE_BINDING | wgpu::TextureUsages::COPY_SRC;
        let target = acde::gpu::texture(&gpu.device, (W, H), usage, "trim test target");
        let usage = wgpu::TextureUsages::STORAGE_BINDING | wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_SRC | wgpu::TextureUsages::COPY_DST;
        let image = GpuImage::new(acde::gpu::texture(&gpu.device, (SIDE, SIDE), usage, "trim test image"), &gpu.images);
        let pixels: Vec<u8> = (0..SIDE * SIDE).flat_map(|i| [(i % SIDE * 4) as u8, (i / SIDE * 4) as u8, 160, 255]).collect();
        gpu.write(&image, &pixels, (SIDE, SIDE), [0, 0], [0, 0, SIDE, SIDE]);
        Self { gpu, vello, overrides, target, image }
    }

    fn scene(&self) -> Scene {
        let mut scene = Scene::new();
        scene.fill(Fill::NonZero, Affine::IDENTITY, Color::from_rgb8(232, 238, 248), None, &Rect::new(0.0, 0.0, f64::from(W), f64::from(H)));
        scene.fill(Fill::NonZero, Affine::IDENTITY, Color::from_rgb8(51, 102, 204), None, &Rect::new(40.0, 40.0, 440.0, 280.0));
        scene.fill(Fill::NonZero, Affine::IDENTITY, Color::from_rgba8(255, 204, 0, 200), None, &Circle::new((240.0, 160.0), 96.0));
        scene.draw_image(self.image.brush().as_ref(), Affine::translate((600.0, 100.0)) * Affine::scale(3.0));
        scene
    }

    fn render(&mut self) -> (Vec<u8>, Duration) {
        let started = Instant::now();
        let view = self.target.create_view(&wgpu::TextureViewDescriptor::default());
        let scene = self.scene();
        self.vello.render([(scene, Affine::IDENTITY)], &mut self.overrides, &view, (W, H), Color::TRANSPARENT, Some(self.gpu.shared.as_ref())).expect("render");
        let _ = self.gpu.device.poll(wgpu::PollType::wait_indefinitely());
        let spent = started.elapsed();
        (self.gpu.read(&self.target, [0, 0, W, H]).expect("read back"), spent)
    }
}

#[test]
fn a_trimmed_renderer_draws_the_same_pixels() {
    let mut bench = Bench::new();
    let (first, cold) = bench.render();
    let (again, warm) = bench.render();
    assert!(first == again, "two renders of the same scene differ");
    let started = Instant::now();
    assert_eq!(bench.vello.trim(), Ok(true), "a renderer that drew is rebuilt");
    let trim = started.elapsed();
    assert_eq!(bench.vello.trim(), Ok(false), "a renderer that has not drawn since is left alone");
    let (after, refill) = bench.render();
    assert!(first == after, "the rebuilt renderer draws different pixels (or lost the GPU image)");
    let ms = |d: Duration| d.as_secs_f64() * 1000.0;
    eprintln!("first render {:.2} ms, again {:.2} ms, trim {:.2} ms, first after the trim {:.2} ms", ms(cold), ms(warm), ms(trim), ms(refill));
}
