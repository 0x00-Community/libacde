use std::sync::Arc;

use acde::gpu::{Backdrop, ColorMatrix, Effect, Gpu, Shadow, wgpu};
use acde::vello::{Composer, Op};

const W: u32 = 128;
const H: u32 = 64;
const PHASES: [u32; 8] = [0, 1, 2, 3, 4, 5, 6, 7];

struct Bench {
    gpu: Arc<Gpu>,
    composer: Composer,
    canvas: wgpu::Texture,
}

impl Bench {
    fn new() -> Self {
        let (_, _, gpu) = Gpu::open("acde compose test").expect("a GPU");
        let composer = Composer::new(&gpu.device, &gpu.queue);
        let usage =
            wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_SRC | wgpu::TextureUsages::COPY_DST;
        let canvas = acde::gpu::texture(&gpu.device, (W, H), usage, "test canvas");
        Self { gpu, composer, canvas }
    }

    fn fill(&self, texture: &wgpu::Texture, pixels: &[[u8; 4]]) {
        let bytes: Vec<u8> = pixels.iter().flatten().copied().collect();
        self.gpu.queue.write_texture(
            texture.as_image_copy(),
            &bytes,
            wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(W * 4), rows_per_image: Some(H) },
            wgpu::Extent3d { width: W, height: H, depth_or_array_layers: 1 },
        );
    }

    fn layer(&mut self, pixels: &[[u8; 4]]) {
        let view = self.composer.layer_view((W, H));
        self.fill(view.texture(), pixels);
    }

    fn run(&mut self, draw: impl FnOnce(&mut Composer, &mut wgpu::CommandEncoder, &wgpu::TextureView)) -> Vec<[u8; 4]> {
        let view = self.canvas.create_view(&wgpu::TextureViewDescriptor::default());
        let mut encoder = self.gpu.device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("test") });
        draw(&mut self.composer, &mut encoder, &view);
        self.composer.flush();
        self.gpu.submit([encoder.finish()]);
        self.composer.end_frame();
        let bytes = self.gpu.read(&self.canvas, [0, 0, W, H]).expect("read back");
        bytes.as_chunks::<4>().0.to_vec()
    }

    fn blurred(&mut self, sigma: f32, at: u32) -> Vec<[u8; 4]> {
        self.layer(&edge([255, 255, 255, 255], [0, 0, 0, 255], at));
        self.fill(&self.canvas, &vec![[0; 4]; (W * H) as usize]);
        let mut effect = Effect::default();
        effect.blur(sigma);
        self.run(|c, e, canvas| c.filtered(e, canvas, &effect, [0, 0, W, H], [0, 0], [0, 0, W, H]))
    }
}

fn edge(left: [u8; 4], right: [u8; 4], at: u32) -> Vec<[u8; 4]> {
    (0..W * H).map(|i| if i % W < at { left } else { right }).collect()
}

fn row(pixels: &[[u8; 4]], y: u32) -> Vec<f64> {
    pixels[(y * W) as usize..((y + 1) * W) as usize].iter().map(|p| f64::from(p[0]) / 255.0).collect()
}

fn erf(x: f64) -> f64 {
    let t = 1.0 / (1.0 + 0.327_591_1 * x.abs());
    let y = 1.0 - (((((1.061_405_429 * t - 1.453_152_027) * t) + 1.421_413_741) * t - 0.284_496_736) * t + 0.254_829_592) * t * (-x * x).exp();
    y.copysign(x)
}

fn gaussian_edge(sigma: f64, at: u32) -> Vec<f64> {
    let middle = f64::from(at);
    (0..W).map(|x| 0.5 * (1.0 - erf((f64::from(x) + 0.5 - middle) / (sigma * std::f64::consts::SQRT_2)))).collect()
}

fn spread(profile: &[f64]) -> f64 {
    let slope: Vec<f64> = profile.windows(2).map(|w| (w[0] - w[1]).max(0.0)).collect();
    let total: f64 = slope.iter().sum();
    let mean = slope.iter().enumerate().map(|(i, s)| i as f64 * s).sum::<f64>() / total;
    (slope.iter().enumerate().map(|(i, s)| (i as f64 - mean).powi(2) * s).sum::<f64>() / total).sqrt()
}

fn worst(a: &[f64], b: &[f64]) -> f64 {
    a.iter().zip(b).map(|(a, b)| (a - b).abs()).fold(0.0, f64::max)
}

#[test]
fn blur_follows_a_gaussian_of_its_sigma() {
    let mut bench = Bench::new();
    let mut profiles = Vec::new();
    for sigma in [1.0, 3.0, 6.0] {
        let got = row(&bench.blurred(sigma, W / 2), H / 2);
        let want = gaussian_edge(f64::from(sigma), W / 2);
        let (got_spread, want_spread) = (spread(&got), spread(&want));
        assert!((got_spread - want_spread).abs() < 0.2 * want_spread + 0.15, "sigma {sigma}: the edge spreads {got_spread:.2} px, a Gaussian {want_spread:.2}");
        assert!(worst(&got, &want) < 0.08, "sigma {sigma}: off the Gaussian edge by {:.3}\n{got:.2?}\n{want:.2?}", worst(&got, &want));
        profiles.push(got);
    }
    assert!(worst(&profiles[0], &profiles[1]) > 0.1, "blur(1px) and blur(3px) look the same");
    assert!(worst(&profiles[1], &profiles[2]) > 0.1, "blur(3px) and blur(6px) look the same");
}

#[test]
fn blur_grows_without_steps() {
    let mut bench = Bench::new();
    let mut last = 0.0;
    for tenth in 5..=80 {
        let sigma = tenth as f32 / 10.0;
        let got = PHASES.map(|p| spread(&row(&bench.blurred(sigma, W / 2 + p), H / 2))).iter().sum::<f64>() / PHASES.len() as f64;
        assert!(got > last - 0.05, "sigma {sigma} spreads {got:.3} px, less than {last:.3} before");
        last = got;
    }
}

#[test]
fn shadow_without_blur_is_sharp() {
    let mut bench = Bench::new();
    let square = |x: i64, y: i64| (16..48).contains(&x) && (16..48).contains(&y);
    let layer: Vec<[u8; 4]> = (0..i64::from(W * H)).map(|i| if square(i % i64::from(W), i / i64::from(W)) { [255; 4] } else { [0; 4] }).collect();
    bench.layer(&layer);
    bench.fill(&bench.canvas, &vec![[0; 4]; (W * H) as usize]);
    let shadow = Shadow { dx: 2.0, dy: 3.0, blur: 0.0, color: [0.0, 0.0, 1.0, 1.0] };
    let got = bench.run(|c, e, canvas| c.shadow(e, canvas, &shadow, [0, 0, 64, 64], [0, 0], [0, 0, W, H]));
    for y in 0..i64::from(H) {
        for x in 0..i64::from(W) {
            let want = if square(x - 2, y - 3) { [0, 0, 255, 255] } else { [0; 4] };
            assert_eq!(got[(y * i64::from(W) + x) as usize], want, "at ({x}, {y})");
        }
    }
}

fn filtered(matrices: &[ColorMatrix], p: [u8; 4]) -> [u8; 4] {
    let mut c = p.map(|v| f32::from(v) / 255.0);
    for m in matrices {
        let m = m.0;
        c = std::array::from_fn(|i| (0..4).map(|j| m[i * 5 + j] * c[j]).sum::<f32>() + m[i * 5 + 4]);
    }
    c.map(|v| (v.clamp(0.0, 1.0) * 255.0).round() as u8)
}

fn backdrop(bench: &mut Bench, matrices: &[ColorMatrix]) -> Vec<[u8; 4]> {
    let mut effect = Effect::default();
    for m in matrices {
        effect.matrix(m.0);
    }
    let backdrop = Backdrop { rect: [8.0, 8.0, 112.0, 48.0], radius: 0.0, exponent: 2.0, glass: None, effect };
    bench.run(|c, e, canvas| c.backdrop(e, canvas, &backdrop))
}

const LEFT: [u8; 4] = [200, 120, 80, 255];
const RIGHT: [u8; 4] = [60, 90, 160, 255];

fn check_row(got: &[[u8; 4]], matrices: &[ColorMatrix]) {
    for x in 8..120u32 {
        let want = filtered(matrices, if x < W / 2 { LEFT } else { RIGHT });
        let p = got[(32 * W + x) as usize];
        assert!(p.iter().zip(want).all(|(a, b)| a.abs_diff(b) <= 3), "at x {x}: {p:?}, want {want:?}");
    }
}

#[test]
fn backdrop_without_blur_keeps_edges() {
    let mut bench = Bench::new();
    bench.fill(&bench.canvas, &edge(LEFT, RIGHT, W / 2));
    let matrices = [ColorMatrix::saturate(2.0)];
    check_row(&backdrop(&mut bench, &matrices), &matrices);
}

#[test]
fn backdrop_applies_every_color_function_in_order() {
    let mut bench = Bench::new();
    let cases = [
        vec![ColorMatrix::contrast(1.5)],
        vec![ColorMatrix::grayscale(1.0)],
        vec![ColorMatrix::hue_rotate(std::f32::consts::FRAC_PI_2)],
        vec![ColorMatrix::sepia(0.5), ColorMatrix::invert(0.25)],
        vec![ColorMatrix::hue_rotate(std::f32::consts::FRAC_PI_2), ColorMatrix::grayscale(0.5), ColorMatrix::brightness(1.2)],
    ];
    for matrices in cases {
        bench.fill(&bench.canvas, &edge(LEFT, RIGHT, W / 2));
        check_row(&backdrop(&mut bench, &matrices), &matrices);
    }
}

#[test]
fn backdrop_blurs_summed_in_squares() {
    let mut bench = Bench::new();
    let mut twice = Effect::default();
    twice.blur(3.0);
    twice.blur(4.0);
    assert!((twice.blur - 5.0).abs() < 1e-5);
    let mut once = Effect::default();
    once.blur(5.0);
    let mut run = |effect: Effect| {
        bench.fill(&bench.canvas, &edge(LEFT, RIGHT, W / 2));
        let backdrop = Backdrop { rect: [8.0, 8.0, 112.0, 48.0], radius: 0.0, exponent: 2.0, glass: None, effect };
        bench.run(|c, e, canvas| c.backdrop(e, canvas, &backdrop))
    };
    assert_eq!(run(twice), run(once));
}

type DrawAt<'a> = dyn Fn(&mut Composer, &mut wgpu::CommandEncoder, &wgpu::TextureView, [u32; 2]) + 'a;

#[test]
fn a_layer_read_from_an_offset_lands_where_it_belongs() {
    let mut bench = Bench::new();
    let square = |x: i64, y: i64, at: i64| (16 + at..32 + at).contains(&x) && (8 + at..24 + at).contains(&y);
    let mut draw = |at: u32, with: &DrawAt<'_>| {
        let layer: Vec<[u8; 4]> =
            (0..i64::from(W * H)).map(|i| if square(i % i64::from(W), i / i64::from(W), i64::from(at)) { [255; 4] } else { [0; 4] }).collect();
        bench.layer(&layer);
        bench.fill(&bench.canvas, &vec![[0; 4]; (W * H) as usize]);
        bench.run(|c, e, canvas| with(c, e, canvas, [at, at]))
    };
    let shadow = Shadow { dx: 2.0, dy: 3.0, blur: 2.0, color: [0.0, 0.0, 1.0, 1.0] };
    let mut effect = Effect::default();
    effect.blur(1.5);
    let place = |c: &mut Composer, e: &mut wgpu::CommandEncoder, v: &wgpu::TextureView, at: [u32; 2]| c.place(e, v, [4, 4], at, [0, 0, W, H]);
    let shadowed =
        |c: &mut Composer, e: &mut wgpu::CommandEncoder, v: &wgpu::TextureView, at: [u32; 2]| c.shadow(e, v, &shadow, [4, 4, 48, 32], at, [0, 0, W, H]);
    let filtered =
        |c: &mut Composer, e: &mut wgpu::CommandEncoder, v: &wgpu::TextureView, at: [u32; 2]| c.filtered(e, v, &effect, [4, 4, 48, 32], at, [0, 0, W, H]);
    for (name, with) in [("place", &place as &DrawAt<'_>), ("shadow", &shadowed), ("filtered", &filtered)] {
        assert_eq!(draw(0, with), draw(20, with), "{name}: the layer read at (20, 20) differs from the one at (0, 0)");
    }
}

#[test]
fn a_shadow_from_the_blurred_canvas_matches_one_blurred_alone() {
    let mut bench = Bench::new();
    let shadow = Shadow { dx: 2.0, dy: 3.0, blur: 2.0, color: [0.0, 0.0, 1.0, 1.0] };
    let square = |at: i64| move |x: i64, y: i64| (16 + at..32 + at).contains(&x) && (8 + at..24 + at).contains(&y);
    let layer = |bench: &mut Bench, inside: &dyn Fn(i64, i64) -> bool| {
        let pixels: Vec<[u8; 4]> = (0..i64::from(W * H)).map(|i| if inside(i % i64::from(W), i / i64::from(W)) { [255; 4] } else { [0; 4] }).collect();
        bench.layer(&pixels);
        bench.fill(&bench.canvas, &vec![[0; 4]; (W * H) as usize]);
    };
    layer(&mut bench, &square(0));
    let alone = bench.run(|c, e, canvas| c.shadow(e, canvas, &shadow, [4, 4, 48, 32], [0, 0], [0, 0, W, H]));
    layer(&mut bench, &square(32));
    let shared = bench.run(|c, e, canvas| {
        let blurred = c.blur_layers(e, (W, H), shadow.blur, 0).expect("a layer canvas");
        c.draw(e, canvas, &[Op::Shadow { shadow, rect: [4, 4, 48, 32], at: [32, 32], clip: [0, 0, W, H], blurred }]);
    });
    let worst = alone.iter().zip(&shared).map(|(a, b)| a.iter().zip(b).map(|(x, y)| x.abs_diff(*y)).max().unwrap_or(0)).max().unwrap_or(0);
    assert!(worst <= 2, "the shadow differs by up to {worst}");
}
