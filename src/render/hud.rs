use crate::gpu::{PaintScene, Scene};
use kurbo::{Affine, Line, Rect, RoundedRect, Stroke};
use parley::fontique::{Collection, CollectionOptions, SourceCache};
use parley::{FontContext, FontFamily, FontWeight, Layout, LayoutContext, LineHeight, StyleProperty};
use peniko::{Color, Fill};

use super::decoration::draw_line;

pub struct HudStats<'a> {
    pub fps: f64,
    pub mean_ms: f64,
    pub worst_ms: f64,
    pub raster_ms: f64,
    pub budget_ms: f64,
    pub intervals: &'a [f64],
}

pub struct Hud {
    fonts: FontContext,
    layouts: LayoutContext<()>,
}

impl Default for Hud {
    fn default() -> Self {
        Self::new()
    }
}

impl Hud {
    pub const SIZE: (f64, f64) = (236.0, 112.0);
    pub const BARS: usize = 120;
    const PAD: f64 = 10.0;
    const GRAPH_TOP: f64 = 52.0;
    const RADIUS: f64 = 8.0;
    const FAMILY: &'static str = "ui-monospace, SF Mono, Menlo, monospace";
    const BACKGROUND: Color = Color::from_rgba8(14, 15, 18, 214);
    const TEXT: Color = Color::from_rgba8(235, 235, 240, 255);
    const DIM: Color = Color::from_rgba8(160, 164, 176, 255);
    const GUIDE: Color = Color::from_rgba8(255, 255, 255, 64);
    const GOOD: Color = Color::from_rgb8(0x4a, 0xde, 0x80);
    const SLOW: Color = Color::from_rgb8(0xfb, 0xbf, 0x24);
    const BAD: Color = Color::from_rgb8(0xf8, 0x71, 0x71);

    pub fn new() -> Self {
        let fonts =
            FontContext { source_cache: SourceCache::new_shared(), collection: Collection::new(CollectionOptions { shared: false, system_fonts: true }) };
        Self { fonts, layouts: LayoutContext::new() }
    }

    pub fn pixels(scale: f64) -> (u32, u32) {
        ((Self::SIZE.0 * scale).ceil() as u32, (Self::SIZE.1 * scale).ceil() as u32)
    }

    pub fn scene(&mut self, stats: Option<&HudStats<'_>>, scale: f64) -> Scene {
        let mut scene = Scene::new();
        let at = Affine::scale(scale);
        let (w, h) = Self::SIZE;
        scene.fill(Fill::NonZero, at, Self::BACKGROUND, None, &RoundedRect::new(0.0, 0.0, w, h, Self::RADIUS));
        let Some(stats) = stats else {
            self.text(&mut scene, "idle", 18.0, 700.0, scale, (Self::PAD, 8.0), Self::DIM);
            self.text(&mut scene, "no new frames in the last second", 11.0, 400.0, scale, (Self::PAD, 32.0), Self::DIM);
            return scene;
        };
        let budget = stats.budget_ms.max(1.0);
        let fps = format!("{:.0} FPS", stats.fps);
        let headline = self.text(&mut scene, &fps, 18.0, 700.0, scale, (Self::PAD, 6.0), Self::grade(stats.mean_ms, budget));
        let mean = format!("{:.1} ms avg", stats.mean_ms);
        self.text(&mut scene, &mean, 11.0, 400.0, scale, (Self::PAD + headline + 10.0, 12.0), Self::TEXT);
        let detail = format!("worst {:.1} · raster {:.1} ms", stats.worst_ms, stats.raster_ms);
        self.text(&mut scene, &detail, 11.0, 400.0, scale, (Self::PAD, 31.0), Self::DIM);
        let graph = Rect::new(Self::PAD, Self::GRAPH_TOP, w - Self::PAD, h - Self::PAD);
        let top = budget * 3.0;
        let y_of = |ms: f64| graph.y1 - (ms.min(top) / top) * graph.height();
        let bar = graph.width() / Self::BARS as f64;
        let start = Self::BARS.saturating_sub(stats.intervals.len());
        for (i, &ms) in stats.intervals.iter().rev().take(Self::BARS).rev().enumerate() {
            let x = graph.x0 + (start + i) as f64 * bar;
            scene.fill(Fill::NonZero, at, Self::grade(ms, budget), None, &Rect::new(x, y_of(ms), x + bar.max(1.0), graph.y1));
        }
        for guide in [budget, budget * 2.0] {
            let y = y_of(guide).round() + 0.5;
            scene.stroke(&Stroke::new(1.0), at, Self::GUIDE, None, &Line::new((graph.x0, y), (graph.x1, y)));
            let label = format!("{guide:.1}");
            self.text(&mut scene, &label, 9.0, 400.0, scale, (graph.x0 + 2.0, y - 11.0), Self::DIM);
        }
        scene
    }

    fn grade(ms: f64, budget: f64) -> Color {
        if ms <= budget * 1.25 {
            Self::GOOD
        } else if ms <= budget * 2.0 {
            Self::SLOW
        } else {
            Self::BAD
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn text(&mut self, scene: &mut Scene, text: &str, size: f32, weight: f32, scale: f64, (x, y): (f64, f64), color: Color) -> f64 {
        let layout = self.layout(text, size, weight, scale);
        if let Some(line) = layout.lines().next() {
            draw_line(scene, &line, Affine::translate((x * scale, y * scale)), color);
        }
        f64::from(layout.width()) / scale
    }

    fn layout(&mut self, text: &str, size: f32, weight: f32, scale: f64) -> Layout<()> {
        let mut builder = self.layouts.ranged_builder(&mut self.fonts, text, scale as f32, true);
        builder.push_default(StyleProperty::FontFamily(FontFamily::from(Self::FAMILY)));
        builder.push_default(StyleProperty::FontSize(size));
        builder.push_default(StyleProperty::FontWeight(FontWeight::new(weight)));
        builder.push_default(StyleProperty::LineHeight(LineHeight::FontSizeRelative(1.2)));
        let mut layout = builder.build(text);
        layout.break_all_lines(None);
        layout
    }
}
