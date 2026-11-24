use std::sync::Arc;

use crate::gpu::RenderCommand;
use crate::gpu::Scene;
use blitz_paint::Piece;
use kurbo::Affine;

#[derive(Clone)]
struct Seg {
    commands: Arc<[RenderCommand]>,
    start: usize,
    end: usize,
    transform: Affine,
    at: usize,
}

#[derive(Clone, Default)]
pub struct Commands {
    segs: Vec<Seg>,
    len: usize,
}

impl Commands {
    pub(crate) fn new(loose: Vec<RenderCommand>, pieces: Vec<Piece>) -> Self {
        let mut out = Self::owned(loose);
        for piece in pieces {
            out.push(piece.commands, piece.range.start, piece.range.end, piece.transform);
        }
        out
    }

    pub(crate) fn extend(&mut self, commands: Vec<RenderCommand>) {
        let end = commands.len();
        self.push(commands.into(), 0, end, Affine::IDENTITY);
    }

    pub(crate) fn owned(commands: Vec<RenderCommand>) -> Self {
        let mut out = Self::default();
        let end = commands.len();
        out.push(commands.into(), 0, end, Affine::IDENTITY);
        out
    }

    fn push(&mut self, commands: Arc<[RenderCommand]>, start: usize, end: usize, transform: Affine) {
        if start >= end {
            return;
        }
        if let Some(last) = self.segs.last_mut()
            && last.end == start
            && last.transform == transform
            && Arc::ptr_eq(&last.commands, &commands)
        {
            last.end = end;
        } else {
            self.segs.push(Seg { commands, start, end, transform, at: self.len });
        }
        self.len += end - start;
    }

    pub fn len(&self) -> usize {
        self.len
    }

    pub(crate) fn same(&self, other: &Commands) -> bool {
        self.len == other.len
            && self.segs.len() == other.segs.len()
            && self
                .segs
                .iter()
                .zip(&other.segs)
                .all(|(a, b)| Arc::ptr_eq(&a.commands, &b.commands) && (a.start, a.end, a.transform) == (b.start, b.end, b.transform))
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    pub fn iter(&self) -> impl Iterator<Item = (&RenderCommand, Affine)> + '_ {
        self.segs.iter().flat_map(|s| s.commands[s.start..s.end].iter().map(move |c| (c, s.transform)))
    }

    pub(crate) fn iter_range(&self, range: std::ops::Range<usize>) -> impl Iterator<Item = (&RenderCommand, Affine)> + '_ {
        let first = self.segs.partition_point(|s| s.at + (s.end - s.start) <= range.start);
        self.segs[first..].iter().take_while(move |s| s.at < range.end).flat_map(move |s| {
            let from = s.start + range.start.saturating_sub(s.at);
            let to = s.end.min(s.start + (range.end - s.at));
            s.commands[from..to].iter().map(move |c| (c, s.transform))
        })
    }

    pub fn get(&self, i: usize) -> Option<(&RenderCommand, Affine)> {
        let k = self.segs.partition_point(|s| s.at <= i).checked_sub(1)?;
        let s = &self.segs[k];
        s.commands.get(s.start + (i - s.at)).filter(|_| i - s.at < s.end - s.start).map(|c| (c, s.transform))
    }

    pub(crate) fn placed(command: &RenderCommand, transform: Affine) -> RenderCommand {
        match transform == Affine::IDENTITY {
            true => command.clone(),
            false => blitz_paint::placed(command.clone(), transform),
        }
    }

    pub fn to_vec(&self) -> Vec<RenderCommand> {
        self.iter().map(|(c, t)| Self::placed(c, t)).collect()
    }

    pub fn to_scene(&self, tolerance: f64) -> Scene {
        Scene { tolerance, commands: self.to_vec() }
    }

    pub(crate) fn builder(&self) -> Builder<'_> {
        Builder { source: self, out: Commands::default(), owned: Vec::new() }
    }
}

pub(crate) struct Builder<'a> {
    source: &'a Commands,
    out: Commands,
    owned: Vec<RenderCommand>,
}

impl Builder<'_> {
    pub(crate) fn source(&mut self, i: usize) {
        self.flush();
        let k = self.source.segs.partition_point(|s| s.at <= i) - 1;
        let s = &self.source.segs[k];
        let local = s.start + (i - s.at);
        self.out.push(s.commands.clone(), local, local + 1, s.transform);
    }

    pub(crate) fn owned(&mut self, command: RenderCommand) {
        self.owned.push(command);
    }

    pub(crate) fn len(&self) -> usize {
        self.out.len + self.owned.len()
    }

    pub(crate) fn finish(mut self) -> Commands {
        self.flush();
        self.out
    }

    fn flush(&mut self) {
        if !self.owned.is_empty() {
            let owned = std::mem::take(&mut self.owned);
            let end = owned.len();
            self.out.push(owned.into(), 0, end, Affine::IDENTITY);
        }
    }
}
