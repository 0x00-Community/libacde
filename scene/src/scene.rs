use std::sync::Arc;

use kurbo::{Affine, BezPath, Rect, Shape, Stroke, Vec2};
use peniko::{BlendMode, Brush, BrushRef, Color, Fill, FontData, Gradient, ImageBrush, ImageBrushRef, Style, StyleRef};

use crate::Filter;

pub type NormalizedCoord = i16;

#[derive(Copy, Clone, Debug, PartialEq)]
pub struct Glyph {
    pub id: u32,
    pub x: f32,
    pub y: f32,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Paint<I = ImageBrush, G = Gradient> {
    Solid(Color),
    Gradient(G),
    Image(I),
}

pub type PaintRef<'a> = Paint<ImageBrushRef<'a>, &'a Gradient>;

impl<'a> From<&'a Paint> for PaintRef<'a> {
    fn from(paint: &'a Paint) -> Self {
        match paint {
            Paint::Solid(color) => Paint::Solid(*color),
            Paint::Gradient(gradient) => Paint::Gradient(gradient),
            Paint::Image(image) => Paint::Image(image.as_ref()),
        }
    }
}

impl From<PaintRef<'_>> for Paint {
    fn from(paint: PaintRef<'_>) -> Self {
        match paint {
            Paint::Solid(color) => Paint::Solid(color),
            Paint::Gradient(gradient) => Paint::Gradient(gradient.clone()),
            Paint::Image(image) => Paint::Image(image.to_owned()),
        }
    }
}

impl<'a> From<PaintRef<'a>> for BrushRef<'a> {
    fn from(paint: PaintRef<'a>) -> Self {
        match paint {
            Paint::Solid(color) => Brush::Solid(color),
            Paint::Gradient(gradient) => Brush::Gradient(gradient),
            Paint::Image(image) => Brush::Image(image),
        }
    }
}

impl<'a> From<BrushRef<'a>> for PaintRef<'a> {
    fn from(brush: BrushRef<'a>) -> Self {
        match brush {
            BrushRef::Solid(color) => Paint::Solid(color),
            BrushRef::Gradient(gradient) => Paint::Gradient(gradient),
            BrushRef::Image(image) => Paint::Image(image),
        }
    }
}

impl<I, G> From<Color> for Paint<I, G> {
    fn from(color: Color) -> Self {
        Paint::Solid(color)
    }
}

impl<'a, I> From<&'a Gradient> for Paint<I, &'a Gradient> {
    fn from(gradient: &'a Gradient) -> Self {
        Paint::Gradient(gradient)
    }
}

impl<'a, G> From<ImageBrushRef<'a>> for Paint<ImageBrushRef<'a>, G> {
    fn from(image: ImageBrushRef<'a>) -> Self {
        Paint::Image(image)
    }
}

pub trait PaintScene {
    fn reset(&mut self);

    #[allow(clippy::too_many_arguments, reason = "every argument is part of a CSS layer")]
    fn push_layer(
        &mut self,
        fill: Fill,
        blend: impl Into<BlendMode>,
        alpha: f32,
        transform: Affine,
        clip: &impl Shape,
        filter: Option<Arc<Filter>>,
        backdrop_filter: Option<Arc<Filter>>,
    );

    fn push_clip_layer(&mut self, fill: Fill, transform: Affine, clip: &impl Shape);

    fn pop_layer(&mut self);

    fn stroke<'a>(&mut self, style: &Stroke, transform: Affine, paint: impl Into<PaintRef<'a>>, paint_transform: Option<Affine>, shape: &impl Shape);

    fn fill<'a>(&mut self, fill: Fill, transform: Affine, paint: impl Into<PaintRef<'a>>, paint_transform: Option<Affine>, shape: &impl Shape);

    #[allow(clippy::too_many_arguments, reason = "a glyph run's parameters, as vello takes them")]
    fn draw_glyphs<'a, 's: 'a>(
        &'s mut self,
        font: &'a FontData,
        font_size: f32,
        hint: bool,
        normalized_coords: &'a [NormalizedCoord],
        embolden: Vec2,
        style: impl Into<StyleRef<'a>>,
        paint: impl Into<PaintRef<'a>>,
        paint_alpha: f32,
        transform: Affine,
        glyph_transform: Option<Affine>,
        glyphs: impl Iterator<Item = Glyph> + Clone,
    );

    fn draw_box_shadow(&mut self, transform: Affine, rect: Rect, color: Color, radius: f64, std_dev: f64);

    fn append_scene(&mut self, scene: Scene, transform: Affine) {
        for command in scene.commands {
            command.replay(self, transform);
        }
    }

    fn draw_image(&mut self, image: ImageBrushRef<'_>, transform: Affine) {
        let rect = Rect::new(0.0, 0.0, f64::from(image.image.width), f64::from(image.image.height));
        self.fill(Fill::NonZero, transform, image, None, &rect);
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum RenderCommand {
    PushLayer(LayerCommand),
    PushClipLayer(ClipCommand),
    PopLayer,
    Stroke(StrokeCommand),
    Fill(FillCommand),
    GlyphRun(GlyphRunCommand),
    BoxShadow(BoxShadowCommand),
}

#[derive(Clone, Debug, PartialEq)]
pub struct LayerCommand {
    pub fill: Fill,
    pub blend: BlendMode,
    pub alpha: f32,
    pub transform: Affine,
    pub clip: BezPath,
    pub filter: Option<Arc<Filter>>,
    pub backdrop_filter: Option<Arc<Filter>>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ClipCommand {
    pub fill: Fill,
    pub transform: Affine,
    pub clip: BezPath,
}

#[derive(Clone, Debug, PartialEq)]
pub struct StrokeCommand {
    pub style: Stroke,
    pub transform: Affine,
    pub brush: Paint,
    pub brush_transform: Option<Affine>,
    pub shape: BezPath,
}

#[derive(Clone, Debug, PartialEq)]
pub struct FillCommand {
    pub fill: Fill,
    pub transform: Affine,
    pub brush: Paint,
    pub brush_transform: Option<Affine>,
    pub shape: BezPath,
}

#[derive(Clone, Debug, PartialEq)]
pub struct GlyphRunCommand {
    pub font_data: FontData,
    pub font_size: f32,
    pub hint: bool,
    pub normalized_coords: Vec<NormalizedCoord>,
    pub embolden: Vec2,
    pub style: Style,
    pub brush: Paint,
    pub brush_alpha: f32,
    pub transform: Affine,
    pub glyph_transform: Option<Affine>,
    pub glyphs: Vec<Glyph>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct BoxShadowCommand {
    pub transform: Affine,
    pub rect: Rect,
    pub brush: Color,
    pub radius: f64,
    pub std_dev: f64,
}

impl RenderCommand {
    pub fn transformed(mut self, by: Affine) -> Self {
        match &mut self {
            Self::PushLayer(c) => c.transform = by * c.transform,
            Self::PushClipLayer(c) => c.transform = by * c.transform,
            Self::PopLayer => {}
            Self::Stroke(c) => c.transform = by * c.transform,
            Self::Fill(c) => c.transform = by * c.transform,
            Self::GlyphRun(c) => c.transform = by * c.transform,
            Self::BoxShadow(c) => c.transform = by * c.transform,
        }
        self
    }

    pub fn replay<P: PaintScene + ?Sized>(self, to: &mut P, by: Affine) {
        match self {
            Self::PushLayer(c) => to.push_layer(c.fill, c.blend, c.alpha, by * c.transform, &c.clip, c.filter, c.backdrop_filter),
            Self::PushClipLayer(c) => to.push_clip_layer(c.fill, by * c.transform, &c.clip),
            Self::PopLayer => to.pop_layer(),
            Self::Stroke(c) => to.stroke(&c.style, by * c.transform, &c.brush, c.brush_transform, &c.shape),
            Self::Fill(c) => to.fill(c.fill, by * c.transform, &c.brush, c.brush_transform, &c.shape),
            Self::GlyphRun(c) => to.draw_glyphs(
                &c.font_data,
                c.font_size,
                c.hint,
                &c.normalized_coords,
                c.embolden,
                &c.style,
                &c.brush,
                c.brush_alpha,
                by * c.transform,
                c.glyph_transform,
                c.glyphs.into_iter(),
            ),
            Self::BoxShadow(c) => to.draw_box_shadow(by * c.transform, c.rect, c.brush, c.radius, c.std_dev),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Scene {
    pub tolerance: f64,
    pub commands: Vec<RenderCommand>,
}

impl Scene {
    pub const TOLERANCE: f64 = 0.1;

    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_tolerance(tolerance: f64) -> Self {
        Self { tolerance, commands: Vec::new() }
    }
}

impl Default for Scene {
    fn default() -> Self {
        Self::with_tolerance(Self::TOLERANCE)
    }
}

impl PaintScene for Scene {
    fn reset(&mut self) {
        self.commands.clear();
    }

    fn push_layer(
        &mut self,
        fill: Fill,
        blend: impl Into<BlendMode>,
        alpha: f32,
        transform: Affine,
        clip: &impl Shape,
        filter: Option<Arc<Filter>>,
        backdrop_filter: Option<Arc<Filter>>,
    ) {
        let clip = clip.into_path(self.tolerance);
        self.commands.push(RenderCommand::PushLayer(LayerCommand { fill, blend: blend.into(), alpha, transform, clip, filter, backdrop_filter }));
    }

    fn push_clip_layer(&mut self, fill: Fill, transform: Affine, clip: &impl Shape) {
        let clip = clip.into_path(self.tolerance);
        self.commands.push(RenderCommand::PushClipLayer(ClipCommand { fill, transform, clip }));
    }

    fn pop_layer(&mut self) {
        self.commands.push(RenderCommand::PopLayer);
    }

    fn stroke<'a>(&mut self, style: &Stroke, transform: Affine, paint: impl Into<PaintRef<'a>>, paint_transform: Option<Affine>, shape: &impl Shape) {
        let shape = shape.into_path(self.tolerance);
        let brush = paint.into().into();
        self.commands.push(RenderCommand::Stroke(StrokeCommand { style: style.clone(), transform, brush, brush_transform: paint_transform, shape }));
    }

    fn fill<'a>(&mut self, fill: Fill, transform: Affine, paint: impl Into<PaintRef<'a>>, paint_transform: Option<Affine>, shape: &impl Shape) {
        let shape = shape.into_path(self.tolerance);
        let brush = paint.into().into();
        self.commands.push(RenderCommand::Fill(FillCommand { fill, transform, brush, brush_transform: paint_transform, shape }));
    }

    fn draw_glyphs<'a, 's: 'a>(
        &'s mut self,
        font: &'a FontData,
        font_size: f32,
        hint: bool,
        normalized_coords: &'a [NormalizedCoord],
        embolden: Vec2,
        style: impl Into<StyleRef<'a>>,
        paint: impl Into<PaintRef<'a>>,
        paint_alpha: f32,
        transform: Affine,
        glyph_transform: Option<Affine>,
        glyphs: impl Iterator<Item = Glyph> + Clone,
    ) {
        self.commands.push(RenderCommand::GlyphRun(GlyphRunCommand {
            font_data: font.clone(),
            font_size,
            hint,
            normalized_coords: normalized_coords.to_vec(),
            embolden,
            style: style.into().to_owned(),
            brush: paint.into().into(),
            brush_alpha: paint_alpha,
            transform,
            glyph_transform,
            glyphs: glyphs.collect(),
        }));
    }

    fn draw_box_shadow(&mut self, transform: Affine, rect: Rect, color: Color, radius: f64, std_dev: f64) {
        self.commands.push(RenderCommand::BoxShadow(BoxShadowCommand { transform, rect, brush: color, radius, std_dev }));
    }

    fn append_scene(&mut self, scene: Scene, transform: Affine) {
        self.commands.extend(scene.commands.into_iter().map(|c| c.transformed(transform)));
    }
}
