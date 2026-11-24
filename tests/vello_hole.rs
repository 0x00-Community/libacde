use std::sync::Arc;

use acde::gpu::kurbo::{Affine, Rect};
use acde::gpu::peniko::{Color, Fill, ImageQuality};
use acde::gpu::{Gpu, GpuImage, Paint, PaintScene, Scene, wgpu};
use acde::vello::{Composer, Hole, Op, Overrides, Vello};

const W: u32 = 96;
const H: u32 = 64;

struct Bench {
    gpu: Arc<Gpu>,
    vello: Vello,
    overrides: Overrides,
    composer: Composer,
}

impl Bench {
    fn new() -> Self {
        let (_, _, gpu) = Gpu::open("acde hole test").expect("a GPU");
        let vello = Vello::new(&gpu.device, &gpu.queue).expect("vello");
        let overrides = Overrides::from(gpu.shared.images.clone());
        let composer = Composer::new(&gpu.device, &gpu.queue);
        Self { gpu, vello, overrides, composer }
    }

    fn texture(&self, (w, h): (u32, u32)) -> wgpu::Texture {
        let usage = wgpu::TextureUsages::RENDER_ATTACHMENT
            | wgpu::TextureUsages::STORAGE_BINDING
            | wgpu::TextureUsages::TEXTURE_BINDING
            | wgpu::TextureUsages::COPY_SRC
            | wgpu::TextureUsages::COPY_DST;
        acde::gpu::texture(&self.gpu.device, (w, h), usage, "hole test")
    }

    fn image(&self, (w, h): (u32, u32)) -> Arc<GpuImage> {
        let texture = self.texture((w, h));
        let bytes: Vec<u8> = (0..w * h).flat_map(|i| [(i * 37 % 251) as u8, (i * 11 % 241) as u8, (i * 101 % 239) as u8, 255]).collect();
        self.gpu.queue.write_texture(
            texture.as_image_copy(),
            &bytes,
            wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(w * 4), rows_per_image: Some(h) },
            wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
        );
        let image = GpuImage::new(texture, &self.gpu.shared.images);
        image.set_opaque(true);
        image
    }

    fn by_vello(&mut self, scene: &Scene) -> Vec<[u8; 4]> {
        let target = self.texture((W, H));
        let view = target.create_view(&Default::default());
        self.vello.render([(scene.clone(), Affine::IDENTITY)], &mut self.overrides, &view, (W, H), Color::TRANSPARENT, None).expect("vello renders");
        self.read(&target)
    }

    fn by_hole(&mut self, hole: &Hole) -> Vec<[u8; 4]> {
        let target = self.texture((W, H));
        let view = target.create_view(&Default::default());
        let source = hole.image.texture.create_view(&Default::default());
        let extent = [hole.image.texture.width() as f32, hole.image.texture.height() as f32];
        let mut encoder = self.gpu.device.create_command_encoder(&Default::default());
        self.composer.clear(&mut encoder, &view);
        self.composer.opaque_from(&mut encoder, &view, &source, hole.map.map(|v| v as f32), extent, hole.smooth, hole.rect);
        self.composer.flush();
        self.gpu.submit([encoder.finish()]);
        self.composer.end_frame();
        self.read(&target)
    }

    fn read(&self, texture: &wgpu::Texture) -> Vec<[u8; 4]> {
        self.gpu.read(texture, [0, 0, W, H]).expect("read back").as_chunks::<4>().0.to_vec()
    }
}

fn picture(image: &Arc<GpuImage>, rect: Rect, place: Affine, quality: ImageQuality) -> Scene {
    let mut brush = image.brush();
    brush.sampler.quality = quality;
    let mut scene = Scene::new();
    scene.fill(Fill::NonZero, Affine::IDENTITY, &Paint::Image(brush), Some(place), &rect);
    scene
}

fn differ(a: &[[u8; 4]], b: &[[u8; 4]]) -> u8 {
    a.iter().zip(b).flat_map(|(p, q)| p.iter().zip(q).map(|(x, y)| x.abs_diff(*y))).max().unwrap_or(0)
}

#[test]
fn a_hole_blit_draws_what_vello_draws() {
    let mut bench = Bench::new();
    let cases = [
        ((W, H), Rect::new(0.0, 0.0, 96.0, 64.0), Affine::IDENTITY, ImageQuality::Medium, 0),
        ((64, 48), Rect::new(8.0, 4.0, 72.0, 52.0), Affine::translate((8.0, 4.0)), ImageQuality::Medium, 0),
        ((48, 32), Rect::new(0.0, 0.0, 96.0, 64.0), Affine::scale(2.0), ImageQuality::Medium, 1),
        ((48, 32), Rect::new(0.0, 0.0, 96.0, 64.0), Affine::scale(2.0), ImageQuality::Low, 0),
        ((40, 30), Rect::new(4.0, 2.0, 64.0, 47.0), Affine::translate((4.0, 2.0)) * Affine::scale(1.5), ImageQuality::Medium, 1),
    ];
    for (size, rect, place, quality, slack) in cases {
        let image = bench.image(size);
        let scene = picture(&image, rect, place, quality);
        let hole = Hole::largest(&scene, &bench.gpu.shared.images).expect("an opaque, axis-aligned image is a hole");
        assert_eq!(hole.rect, [rect.x0 as u32, rect.y0 as u32, rect.width() as u32, rect.height() as u32]);
        let (want, got) = (bench.by_vello(&scene), bench.by_hole(&hole));
        assert!(differ(&want, &got) <= slack, "{size:?} {place:?} {quality:?}: off by {}", differ(&want, &got));
    }
}

#[test]
fn only_one_to_one_images_are_found_for_the_host_window() {
    let bench = Bench::new();
    let image = bench.image((48, 32));
    let scaled = picture(&image, Rect::new(0.0, 0.0, 96.0, 64.0), Affine::scale(2.0), ImageQuality::Medium);
    assert!(Hole::find(&scaled, &bench.gpu.shared.images).is_none());
    assert!(Hole::largest(&scaled, &bench.gpu.shared.images).is_some());
    let mut two = picture(&image, Rect::new(0.0, 0.0, 48.0, 32.0), Affine::IDENTITY, ImageQuality::Medium);
    let other = bench.image((64, 48));
    two.commands.extend(picture(&other, Rect::new(10.0, 10.0, 74.0, 58.0), Affine::translate((10.0, 10.0)), ImageQuality::Medium).commands);
    assert!(Hole::find(&two, &bench.gpu.shared.images).is_none());
    let largest = Hole::largest(&two, &bench.gpu.shared.images).expect("two holes");
    assert!(Arc::ptr_eq(&largest.image, &other));
}

#[test]
fn a_blend_mode_above_the_hole_keeps_it_from_being_cut() {
    let bench = Bench::new();
    let image = bench.image((48, 32));
    let mut scene = picture(&image, Rect::new(0.0, 0.0, 48.0, 32.0), Affine::IDENTITY, ImageQuality::Medium);
    let hole = Hole::largest(&scene, &bench.gpu.shared.images).expect("a hole");
    assert!(hole.blends_alone(&scene));
    let square = Rect::new(0.0, 0.0, 8.0, 8.0);
    scene.push_layer(Fill::NonZero, acde::gpu::peniko::Mix::Normal, 0.5, Affine::IDENTITY, &square, None, None);
    scene.push_layer(Fill::NonZero, acde::gpu::peniko::Mix::Multiply, 1.0, Affine::IDENTITY, &square, None, None);
    scene.pop_layer();
    scene.pop_layer();
    assert!(hole.blends_alone(&scene), "a blend inside a group above stays in the group");
    scene.push_layer(Fill::NonZero, acde::gpu::peniko::Mix::Multiply, 1.0, Affine::IDENTITY, &square, None, None);
    scene.pop_layer();
    assert!(!hole.blends_alone(&scene), "a blend right above the hole would mix with it");
}

#[test]
fn a_cover_writes_what_a_place_over_nothing_writes() {
    let mut bench = Bench::new();
    let layer = bench.composer.layer_view((W, H));
    let pixels: Vec<u8> = (0..W * H).flat_map(|i| [(i % 256) as u8, (i * 3 % 256) as u8, 200, (i * 7 % 256) as u8]).collect();
    bench.gpu.queue.write_texture(
        layer.texture().as_image_copy(),
        &pixels,
        wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(W * 4), rows_per_image: Some(H) },
        wgpu::Extent3d { width: W, height: H, depth_or_array_layers: 1 },
    );
    let mut draw = |op: Op, stale: bool| {
        let target = bench.texture((W, H));
        let view = target.create_view(&Default::default());
        let mut encoder = bench.gpu.device.create_command_encoder(&Default::default());
        match stale {
            true => bench.composer.fill_rects(&mut encoder, &view, [0.3, 0.6, 0.9, 1.0], [[0, 0, W, H]]),
            false => bench.composer.clear(&mut encoder, &view),
        }
        bench.composer.draw(&mut encoder, &view, &[op]);
        bench.composer.flush();
        bench.gpu.submit([encoder.finish()]);
        bench.composer.end_frame();
        bench.read(&target)
    };
    let placed = draw(Op::Place { origin: [0, 0], at: [0, 0], clip: [0, 0, W, H] }, false);
    let covered = draw(Op::Cover { origin: [0, 0], at: [0, 0], clip: [0, 0, W, H] }, true);
    assert_eq!(differ(&placed, &covered), 0);
}

#[test]
#[ignore = "a measurement: cargo test --release --test vello_hole -- --ignored --nocapture"]
fn a_full_screen_canvas_frame_costs() {
    let (w, h) = (2560u32, 1600u32);
    let mut bench = Bench::new();
    let image = bench.image((w, h));
    let target = bench.texture((w, h));
    let view = target.create_view(&Default::default());
    let source = image.texture.create_view(&Default::default());
    let scene = picture(&image, Rect::new(0.0, 0.0, f64::from(w), f64::from(h)), Affine::IDENTITY, ImageQuality::Medium);
    let hole = Hole::largest(&scene, &bench.gpu.shared.images).expect("a hole");
    let wait = |gpu: &Gpu| _ = gpu.device.poll(wgpu::PollType::wait_indefinitely());
    let time = |label: &str, bench: &mut Bench, run: &mut dyn FnMut(&mut Bench)| {
        for _ in 0..5 {
            run(bench);
        }
        wait(&bench.gpu);
        let started = std::time::Instant::now();
        for _ in 0..30 {
            run(bench);
            wait(&bench.gpu);
        }
        println!("{label}: {:.3} ms a frame", started.elapsed().as_secs_f64() * 1000.0 / 30.0);
    };
    time("vello, clip, clear and place", &mut bench, &mut |b| {
        image.painted();
        let layer = b.composer.layer_view((w, h));
        let clip = Some(Rect::new(0.0, 0.0, f64::from(w), f64::from(h)));
        b.vello.render_clipped([(scene.clone(), Affine::IDENTITY, clip)], &mut b.overrides, &layer, (w, h), Color::TRANSPARENT, None).expect("vello");
        let mut encoder = b.gpu.device.create_command_encoder(&Default::default());
        b.composer.clear(&mut encoder, &view);
        b.composer.draw(&mut encoder, &view, &[Op::Place { origin: [0, 0], at: [0, 0], clip: [0, 0, w, h] }]);
        b.composer.flush();
        b.gpu.submit([encoder.finish()]);
        b.composer.end_frame();
    });
    time("hole blit", &mut bench, &mut |b| {
        let mut encoder = b.gpu.device.create_command_encoder(&Default::default());
        b.composer.opaque_from(&mut encoder, &view, &source, hole.map.map(|v| v as f32), [w as f32, h as f32], false, hole.rect);
        b.composer.flush();
        b.gpu.submit([encoder.finish()]);
        b.composer.end_frame();
    });
}
