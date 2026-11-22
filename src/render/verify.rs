use crate::gpu::RenderCommand;
use crate::gpu::Scene;
use blitz_dom::NodeId;
use blitz_paint::PaintCache;
use kurbo::Affine;

enum Token<'a> {
    Text(&'a str),
    Number(f64),
}

pub(crate) struct Mismatch {
    pub(crate) index: usize,
    pub(crate) node: Option<NodeId>,
    pub(crate) cached: String,
    pub(crate) reference: String,
}

impl Mismatch {
    pub(crate) fn find(cached: &Scene, reference: &Scene, cache: &PaintCache) -> Option<Self> {
        let n = cached.commands.len().max(reference.commands.len());
        let index = (0..n).find(|&i| match (cached.commands.get(i), reference.commands.get(i)) {
            (Some(a), Some(b)) => !Self::same(a, b),
            _ => true,
        })?;
        let show = |c: Option<&RenderCommand>| c.map_or_else(|| "(none)".into(), |c| format!("{c:?}"));
        Some(Self { index, node: cache.owner(index), cached: show(cached.commands.get(index)), reference: show(reference.commands.get(index)) })
    }

    fn same(a: &RenderCommand, b: &RenderCommand) -> bool {
        const TOLERANCE: f64 = 0.02;
        if a == b {
            return true;
        }
        let (da, db) = (format!("{:?}", Self::baked(a)), format!("{:?}", Self::baked(b)));
        let (ta, tb) = (Self::tokens(&da), Self::tokens(&db));
        ta.len() == tb.len()
            && ta.iter().zip(&tb).all(|(x, y)| match (x, y) {
                (Token::Number(p), Token::Number(q)) => (p - q).abs() <= TOLERANCE + 1e-6 * p.abs().max(q.abs()),
                (Token::Text(p), Token::Text(q)) => p == q,
                _ => false,
            })
    }

    fn baked(c: &RenderCommand) -> RenderCommand {
        let mut c = c.clone();
        match &mut c {
            RenderCommand::PushLayer(l) => {
                l.clip.apply_affine(l.transform);
                l.transform = Affine::IDENTITY;
            }
            RenderCommand::PushClipLayer(l) => {
                l.clip.apply_affine(l.transform);
                l.transform = Affine::IDENTITY;
            }
            _ => {}
        }
        c
    }

    fn tokens(text: &str) -> Vec<Token<'_>> {
        let mut out = Vec::new();
        let mut rest = text;
        while let Some(i) = rest.find(|ch: char| ch.is_ascii_digit()) {
            let prev = rest[..i].chars().last();
            let start = if prev == Some('-') { i - 1 } else { i };
            let len = rest[i..].find(|ch: char| !(ch.is_ascii_digit() || ch == '.' || ch == 'e' || ch == '-')).unwrap_or(rest.len() - i);
            let token = &rest[start..i + len];
            match (prev.is_some_and(|p| p.is_alphanumeric() || p == '_'), token.parse::<f64>()) {
                (false, Ok(v)) => {
                    out.push(Token::Text(&rest[..start]));
                    out.push(Token::Number(v));
                }
                _ => out.push(Token::Text(&rest[..i + len])),
            }
            rest = &rest[i + len..];
        }
        out.push(Token::Text(rest));
        out
    }
}

#[cfg(test)]
mod tests {
    use crate::gpu::{LayerCommand, RenderCommand};
    use kurbo::{Affine, Rect, Shape};
    use peniko::{BlendMode, Fill};

    use super::Mismatch;

    fn layer(transform: Affine, clip: Rect) -> RenderCommand {
        RenderCommand::PushLayer(LayerCommand {
            fill: Fill::NonZero,
            blend: BlendMode::default(),
            alpha: 0.0,
            transform,
            clip: clip.to_path(0.1),
            filter: None,
            backdrop_filter: None,
            corners: None,
        })
    }

    #[test]
    fn baked_layer_transform_counts_as_the_same() {
        let cached = layer(
            Affine::new([1.002670407295227, 0.0, 0.0, 1.002670407295227, 329.1721737384796, 306.7516521215439]),
            Rect::new(49.635179875940786, 59.281102644097125, 53.86006199418525, 63.50598475937482),
        );
        let reference = layer(Affine::IDENTITY, Rect::new(378.94524057545146, 366.1964002692014, 383.1707232205451, 370.42188291132027));
        assert!(Mismatch::same(&cached, &reference));
        let shifted = layer(
            Affine::translate((0.15, 0.0)),
            match &reference {
                RenderCommand::PushLayer(l) => l.clip.bounding_box(),
                _ => unreachable!(),
            },
        );
        assert!(!Mismatch::same(&shifted, &reference));
    }
}
