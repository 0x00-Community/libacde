use kurbo::{Arc, Point, Vec2};

pub const ROUND: f64 = 1.0;

pub const WGSL: &str = include_str!("corner.wgsl");

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Corners {
    pub radius: f64,
    pub exponent: f64,
}

pub fn exponent(k: f64) -> f64 {
    k.exp2()
}

pub fn is_round(k: f64) -> bool {
    (k - ROUND).abs() < 1e-6
}

pub fn arc_points(arc: &Arc, k: f64) -> Vec<Point> {
    let Arc { center, radii, start_angle, sweep_angle, .. } = *arc;
    if k.is_infinite() {
        return limit_points(center, radii, start_angle, sweep_angle, k > 0.0);
    }
    let power = 2.0 / exponent(k);
    let steps = steps(radii, sweep_angle);
    (0..=steps)
        .map(|i| {
            let angle = start_angle + sweep_angle * i as f64 / steps as f64;
            let (sin, cos) = angle.sin_cos();
            center + Vec2::new(radii.x * shaped(cos, power), radii.y * shaped(sin, power))
        })
        .collect()
}

pub fn equivalent_radius(radius: f64, k: f64) -> f64 {
    if is_round(k) || radius <= 0.0 {
        return radius;
    }
    radius * (outside(exponent(k)) / outside(2.0)).sqrt()
}

pub fn coverage(point: (f64, f64), half: (f64, f64), radius: f64, exponent: f64) -> f32 {
    let (x, y) = (point.0.abs(), point.1.abs());
    let radius = radius.clamp(0.0, half.0.min(half.1));
    let (cx, cy) = (half.0 - radius, half.1 - radius);
    let distance = if radius > 0.0 && x > cx && y > cy {
        let n = exponent.max(1.0);
        let (u, v) = ((x - cx) / radius, (y - cy) / radius);
        ((u.powf(n) + v.powf(n)).powf(1.0 / n) - 1.0) * radius
    } else {
        (x - half.0).max(y - half.1)
    };
    (0.5 - distance).clamp(0.0, 1.0) as f32
}

fn outside(n: f64) -> f64 {
    if n == f64::INFINITY {
        return 0.0;
    }
    if n <= 0.0 {
        return 1.0;
    }
    const STEPS: usize = 256;
    let height = |x: f64| (1.0 - x.powf(n)).max(0.0).powf(1.0 / n);
    let h = 1.0 / STEPS as f64;
    let inside = (0..STEPS).map(|i| height((i as f64 + 0.5) * h)).sum::<f64>() * h;
    1.0 - inside
}

fn shaped(value: f64, power: f64) -> f64 {
    let value = snap(value);
    value.signum() * value.abs().powf(power)
}

fn steps(radii: Vec2, sweep: f64) -> usize {
    let quarter = (sweep.abs() / core::f64::consts::FRAC_PI_2).max(0.0);
    let per_quarter = (4.0 * radii.x.max(radii.y).max(1.0).sqrt()).ceil().clamp(8.0, 96.0);
    ((per_quarter * quarter).ceil() as usize).max(2)
}

fn limit_points(center: Point, radii: Vec2, start: f64, sweep: f64, square: bool) -> Vec<Point> {
    let end = start + sweep;
    let at = |angle: f64| {
        let (sin, cos) = angle.sin_cos();
        center + Vec2::new(radii.x * snap(cos), radii.y * snap(sin))
    };
    let middle = start + sweep / 2.0;
    let (sin, cos) = middle.sin_cos();
    let turn = match square {
        true => center + Vec2::new(radii.x * cos.signum(), radii.y * sin.signum()),
        false => center,
    };
    vec![at(start), turn, at(end)]
}

fn snap(value: f64) -> f64 {
    if value.abs() < 1e-9 { 0.0 } else { value }
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::f64::consts::{FRAC_PI_2, PI};

    fn on_curve(p: Point, center: Point, r: f64, n: f64) -> f64 {
        (((p.x - center.x) / r).abs().powf(n) + ((p.y - center.y) / r).abs().powf(n)).powf(1.0 / n)
    }

    #[test]
    fn the_exponent_of_k() {
        assert_eq!(exponent(1.0), 2.0);
        assert_eq!(exponent(2.0), 4.0);
        assert_eq!(exponent(0.0), 1.0);
        assert_eq!(exponent(f64::INFINITY), f64::INFINITY);
        assert_eq!(exponent(f64::NEG_INFINITY), 0.0);
    }

    #[test]
    fn points_lie_on_the_superellipse_and_meet_the_ends() {
        let arc = Arc::new((10.0, 10.0), (10.0, 10.0), PI, FRAC_PI_2, 0.0);
        for k in [1.0, 2.0, 2.5, 0.0, -1.0] {
            let points = arc_points(&arc, k);
            let n = exponent(k);
            assert!((points[0] - Point::new(0.0, 10.0)).hypot() < 1e-9, "k {k} start {:?}", points[0]);
            assert!((points[points.len() - 1] - Point::new(10.0, 0.0)).hypot() < 1e-9, "k {k} end");
            for p in &points {
                assert!((on_curve(*p, Point::new(10.0, 10.0), 10.0, n) - 1.0).abs() < 1e-9, "k {k} point {p:?}");
            }
        }
    }

    #[test]
    fn a_squircle_bulges_past_the_circle() {
        let arc = Arc::new((10.0, 10.0), (10.0, 10.0), PI, FRAC_PI_2, 0.0);
        let middle = |k: f64| {
            let points = arc_points(&arc, k);
            points[points.len() / 2]
        };
        assert!(middle(2.0).x < middle(1.0).x);
        assert!(middle(-1.0).x > middle(0.0).x);
    }

    #[test]
    fn square_and_notch_are_the_limits() {
        let arc = Arc::new((10.0, 10.0), (10.0, 10.0), PI, FRAC_PI_2, 0.0);
        assert_eq!(arc_points(&arc, f64::INFINITY), vec![Point::new(0.0, 10.0), Point::new(0.0, 0.0), Point::new(10.0, 0.0)]);
        assert_eq!(arc_points(&arc, f64::NEG_INFINITY), vec![Point::new(0.0, 10.0), Point::new(10.0, 10.0), Point::new(10.0, 0.0)]);
    }

    #[test]
    fn the_equivalent_radius_keeps_the_area_cut_off_the_corner() {
        assert_eq!(equivalent_radius(10.0, 1.0), 10.0);
        let squircle = equivalent_radius(10.0, 2.0);
        assert!(squircle > 5.0 && squircle < 6.5, "{squircle}");
        assert_eq!(equivalent_radius(10.0, f64::INFINITY), 0.0);
    }

    #[test]
    fn coverage_is_inside_outside_and_soft_at_the_edge() {
        let half = (50.0, 30.0);
        assert_eq!(coverage((0.0, 0.0), half, 10.0, 4.0), 1.0);
        assert_eq!(coverage((60.0, 0.0), half, 10.0, 4.0), 0.0);
        assert_eq!(coverage((49.9, 29.9), half, 10.0, 4.0), 0.0);
        let edge = coverage((50.0, 0.0), half, 10.0, 4.0);
        assert!((edge - 0.5).abs() < 1e-6);
        let corner_round = coverage((47.5, 27.5), half, 10.0, 2.0);
        let corner_squircle = coverage((47.5, 27.5), half, 10.0, 4.0);
        assert!(corner_squircle > corner_round);
    }
}
