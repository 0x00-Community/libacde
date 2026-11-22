use std::collections::HashMap;

use super::Name;
use super::group::{GlassGroup, Piece};
use crate::motion::Curve;

#[derive(Clone, Copy, Debug, PartialEq)]
enum Phase {
    Shown,
    Growing { since: f64, from: [f32; 2] },
    Leaving { since: f64 },
}

#[derive(Clone, Copy, Debug)]
struct Track {
    piece: Piece,
    phase: Phase,
}

#[derive(Debug, Default)]
pub struct Appearance {
    tracks: HashMap<Name, Track>,
    seen: bool,
}

impl Appearance {
    pub fn step(&mut self, groups: &mut [&mut GlassGroup], now: f64, curve: Curve) -> bool {
        let present = named(groups);
        if !std::mem::replace(&mut self.seen, true) {
            self.tracks = present.iter().map(|&(id, at)| (id, Track { piece: piece(groups, at), phase: Phase::Shown })).collect();
            return false;
        }
        for &(id, at) in &present {
            let full = piece(groups, at);
            let phase = match self.tracks.get(&id).map(|t| t.phase) {
                Some(Phase::Shown) => Phase::Shown,
                Some(Phase::Growing { since, from }) if curve.at(now - since) < 1.0 => Phase::Growing { since, from },
                Some(Phase::Growing { .. }) => Phase::Shown,
                None | Some(Phase::Leaving { .. }) => match nearest(groups[at.0].pieces(), full.center, Some(at.1)) {
                    Some(from) => Phase::Growing { since: now, from },
                    None => Phase::Shown,
                },
            };
            if let Phase::Growing { since, from } = phase {
                groups[at.0].pieces_mut()[at.1] = scaled(&full, from, curve.at(now - since) as f32);
            }
            self.tracks.insert(id, Track { piece: full, phase });
        }
        let gone: Vec<Name> = self.tracks.keys().filter(|id| !present.iter().any(|(p, _)| p == *id)).copied().collect();
        for id in gone {
            self.leave(id, groups, now, curve);
        }
        self.tracks.values().any(|t| t.phase != Phase::Shown)
    }

    fn leave(&mut self, id: Name, groups: &mut [&mut GlassGroup], now: f64, curve: Curve) {
        let Some(track) = self.tracks.get_mut(&id) else { return };
        let since = match track.phase {
            Phase::Leaving { since } => since,
            _ => now,
        };
        track.phase = Phase::Leaving { since };
        let t = curve.at(now - since) as f32;
        let target = groups
            .iter()
            .enumerate()
            .filter_map(|(g, group)| Some((g, nearest(group.pieces(), track.piece.center, None)?)))
            .min_by(|(_, a), (_, b)| distance(*a, track.piece.center).total_cmp(&distance(*b, track.piece.center)));
        match target {
            Some((g, toward)) if t < 1.0 => _ = groups[g].push(scaled(&track.piece, toward, 1.0 - t)),
            _ => _ = self.tracks.remove(&id),
        }
    }
}

fn named(groups: &[&mut GlassGroup]) -> Vec<(Name, (usize, usize))> {
    groups.iter().enumerate().flat_map(|(g, group)| group.pieces().iter().enumerate().filter_map(move |(i, p)| Some((p.id?, (g, i))))).collect()
}

fn piece(groups: &[&mut GlassGroup], (g, i): (usize, usize)) -> Piece {
    groups[g].pieces()[i]
}

fn nearest(pieces: &[Piece], to: [f32; 2], except: Option<usize>) -> Option<[f32; 2]> {
    pieces.iter().enumerate().filter(|(i, _)| Some(*i) != except).map(|(_, p)| p.center).min_by(|a, b| distance(*a, to).total_cmp(&distance(*b, to)))
}

fn distance(a: [f32; 2], b: [f32; 2]) -> f32 {
    (a[0] - b[0]).hypot(a[1] - b[1])
}

fn scaled(full: &Piece, from: [f32; 2], t: f32) -> Piece {
    let t = t.clamp(0.0, 1.0);
    let toward = |a: f32, b: f32| a + (b - a) * t;
    Piece { center: [toward(from[0], full.center[0]), toward(from[1], full.center[1])], half: full.half.map(|h| h * t), radius: full.radius * t, ..*full }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::glass::{Glass, Level, Role};
    use crate::motion::Preset;

    fn named_piece(x: f32, id: &str) -> Piece {
        Piece { id: Name::of(id), ..Piece::of_rect([x, 0.0, 40.0, 40.0], 20.0, 2.0, Glass::preset(Role::Regular, Level::Clear)) }
    }

    fn group(pieces: &[Piece]) -> GlassGroup {
        let mut group = GlassGroup::new(12.0);
        pieces.iter().for_each(|p| assert!(group.push(*p)));
        group
    }

    fn step(appearance: &mut Appearance, group: &mut GlassGroup, now: f64) -> bool {
        appearance.step(&mut [group], now, Preset::Standard.curve())
    }

    #[test]
    fn what_is_there_on_the_first_frame_does_not_animate() {
        let mut appearance = Appearance::default();
        let mut first = group(&[named_piece(0.0, "a"), named_piece(50.0, "b")]);
        let before = first;
        assert!(!step(&mut appearance, &mut first, 0.0));
        assert_eq!(first, before);
    }

    #[test]
    fn a_new_piece_grows_out_of_its_nearest_neighbour_and_settles() {
        let mut appearance = Appearance::default();
        step(&mut appearance, &mut group(&[named_piece(0.0, "a")]), 0.0);
        let mut both = group(&[named_piece(0.0, "a"), named_piece(100.0, "b")]);
        assert!(step(&mut appearance, &mut both, 1.0));
        let grown = both.pieces()[1];
        assert_eq!((grown.center, grown.half), ([20.0, 20.0], [0.0, 0.0]), "it starts as a point in the middle of a");
        let mut later = group(&[named_piece(0.0, "a"), named_piece(100.0, "b")]);
        assert!(step(&mut appearance, &mut later, 1.2));
        let half = later.pieces()[1].half[0];
        assert!(half > 0.0 && half < 20.0, "halfway it is between nothing and its size: {half}");
        let mut done = group(&[named_piece(0.0, "a"), named_piece(100.0, "b")]);
        assert!(!step(&mut appearance, &mut done, 1.0 + Preset::Standard.curve().length()));
        assert_eq!(done.pieces()[1], named_piece(100.0, "b"));
    }

    #[test]
    fn a_removed_piece_shrinks_into_its_nearest_neighbour_then_goes() {
        let mut appearance = Appearance::default();
        step(&mut appearance, &mut group(&[named_piece(0.0, "a"), named_piece(100.0, "b")]), 0.0);
        let mut left = group(&[named_piece(0.0, "a")]);
        assert!(step(&mut appearance, &mut left, 1.0));
        assert_eq!(left.pieces()[1], named_piece(100.0, "b"), "the ghost starts at its own size");
        let mut later = group(&[named_piece(0.0, "a")]);
        assert!(step(&mut appearance, &mut later, 1.2));
        let ghost = later.pieces()[1];
        assert!(ghost.half[0] < 20.0 && ghost.center[0] < 120.0 && ghost.center[0] > 20.0, "{ghost:?}");
        let mut done = group(&[named_piece(0.0, "a")]);
        assert!(!step(&mut appearance, &mut done, 1.0 + Preset::Standard.curve().length()));
        assert_eq!(done.len(), 1);
    }

    #[test]
    fn pieces_without_an_id_and_a_lone_newcomer_just_appear() {
        let mut appearance = Appearance::default();
        step(&mut appearance, &mut GlassGroup::new(12.0), 0.0);
        let plain = Piece { id: None, ..named_piece(0.0, "x") };
        let mut lone = group(&[named_piece(50.0, "a"), plain]);
        let before = lone;
        assert!(step(&mut appearance, &mut lone, 1.0));
        assert_eq!(lone.pieces()[1], before.pieces()[1]);
        let mut alone = group(&[named_piece(0.0, "solo")]);
        let mut fresh = Appearance::default();
        step(&mut fresh, &mut GlassGroup::new(12.0), 0.0);
        assert!(!step(&mut fresh, &mut alone, 1.0));
        assert_eq!(alone.pieces()[0], named_piece(0.0, "solo"));
    }
}
