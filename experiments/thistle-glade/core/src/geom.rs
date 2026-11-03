//! 2-D helpers on the ground plane (x, z as `Vec2`).

use crate::model::Footprint;
use glam::Vec2;

/// Total length of a polyline.
pub fn polyline_length(pts: &[Vec2]) -> f32 {
    pts.windows(2).map(|w| w[0].distance(w[1])).sum()
}

/// Point and unit direction at distance `along` on a polyline (clamped to its ends).
pub fn point_at(pts: &[Vec2], along: f32) -> (Vec2, Vec2) {
    let mut rest = along.max(0.0);
    for w in pts.windows(2) {
        let len = w[0].distance(w[1]);
        if len < 1e-6 {
            continue;
        }
        let dir = (w[1] - w[0]) / len;
        if rest <= len {
            return (w[0] + dir * rest, dir);
        }
        rest -= len;
    }
    match pts {
        [] => (Vec2::ZERO, Vec2::X),
        [p] => (*p, Vec2::X),
        _ => {
            let n = pts.len();
            (pts[n - 1], (pts[n - 1] - pts[n - 2]).normalize_or(Vec2::X))
        }
    }
}

/// Closest point on a polyline to `p`: (distance along, distance away).
pub fn closest_on_polyline(pts: &[Vec2], p: Vec2) -> (f32, f32) {
    let mut best = (0.0, f32::INFINITY);
    let mut acc = 0.0;
    for w in pts.windows(2) {
        let d = w[1] - w[0];
        let len2 = d.length_squared();
        let t = if len2 > 1e-12 { ((p - w[0]).dot(d) / len2).clamp(0.0, 1.0) } else { 0.0 };
        let q = w[0] + d * t;
        let dist = q.distance(p);
        if dist < best.1 {
            best = (acc + t * len2.sqrt(), dist);
        }
        acc += len2.sqrt();
    }
    best
}

/// Intersection of segments `a0-a1` and `b0-b1`: parameters (ta, tb) in 0..=1.
pub fn segment_intersect(a0: Vec2, a1: Vec2, b0: Vec2, b1: Vec2) -> Option<(f32, f32)> {
    let r = a1 - a0;
    let s = b1 - b0;
    let denom = r.perp_dot(s);
    if denom.abs() < 1e-9 {
        return None;
    }
    let q = b0 - a0;
    let ta = q.perp_dot(s) / denom;
    let tb = q.perp_dot(r) / denom;
    ((0.0..=1.0).contains(&ta) && (0.0..=1.0).contains(&tb)).then_some((ta, tb))
}

/// Every crossing of polyline `a` by polyline `b`: (distance along `a`, distance along `b`).
pub fn polyline_crossings(a: &[Vec2], b: &[Vec2]) -> Vec<(f32, f32)> {
    let mut out = Vec::new();
    let mut acc_a = 0.0;
    for wa in a.windows(2) {
        let la = wa[0].distance(wa[1]);
        let mut acc_b = 0.0;
        for wb in b.windows(2) {
            let lb = wb[0].distance(wb[1]);
            if let Some((ta, tb)) = segment_intersect(wa[0], wa[1], wb[0], wb[1]) {
                out.push((acc_a + ta * la, acc_b + tb * lb));
            }
            acc_b += lb;
        }
        acc_a += la;
    }
    out
}

/// True if the polyline's ends meet (within 0.5 m) and it has at least 4 points.
pub fn is_closed(pts: &[Vec2]) -> bool {
    pts.len() >= 4 && pts[0].distance(pts[pts.len() - 1]) < 0.5
}

/// Signed area (positive = counter-clockwise when x right, z "up" in the 2-D plane).
pub fn signed_area(poly: &[Vec2]) -> f32 {
    let n = poly.len();
    (0..n).map(|i| poly[i].perp_dot(poly[(i + 1) % n])).sum::<f32>() * 0.5
}

/// Point-in-polygon (even-odd).
pub fn contains(poly: &[Vec2], p: Vec2) -> bool {
    let n = poly.len();
    let mut inside = false;
    let mut j = n.wrapping_sub(1);
    for i in 0..n {
        let (a, b) = (poly[i], poly[j]);
        if (a.y > p.y) != (b.y > p.y) && p.x < (b.x - a.x) * (p.y - a.y) / (b.y - a.y) + a.x {
            inside = !inside;
        }
        j = i;
    }
    inside
}

/// Number of facets used for round footprints of radius `r`.
pub fn round_sides(r: f32) -> u32 {
    ((r * 4.0).ceil() as u32).clamp(8, 24)
}

/// Footprint outline, counter-clockwise (positive signed area), first corner first.
/// Rect corners: (-hx,-hz), (hx,-hz), (hx,hz), (-hx,hz) rotated by angle.
pub fn footprint_polygon(fp: &Footprint) -> Vec<Vec2> {
    match *fp {
        Footprint::Rect { center, half, angle } => {
            let rot = Vec2::from_angle(angle);
            let mut v: Vec<Vec2> = [(-1., -1.), (1., -1.), (1., 1.), (-1., 1.)]
                .iter()
                .map(|&(sx, sz)| center + rot.rotate(Vec2::new(sx * half.x, sz * half.y)))
                .collect();
            if signed_area(&v) < 0.0 {
                v.reverse();
            }
            v
        }
        Footprint::Round { center, radius } => {
            let n = round_sides(radius);
            let mut v: Vec<Vec2> = (0..n)
                .map(|i| center + Vec2::from_angle(i as f32 / n as f32 * std::f32::consts::TAU) * radius)
                .collect();
            if signed_area(&v) < 0.0 {
                v.reverse();
            }
            v
        }
    }
}

/// Closed perimeter of a polygon as a polyline (first point repeated at the end).
pub fn closed(poly: &[Vec2]) -> Vec<Vec2> {
    let mut v = poly.to_vec();
    if let Some(&f) = poly.first() {
        v.push(f);
    }
    v
}

/// Outward unit normal of a counter-clockwise polygon edge direction `dir`.
pub fn outward(dir: Vec2) -> Vec2 {
    // CCW polygon in (x, z): interior is on the left of travel, so outward is to the right.
    Vec2::new(dir.y, -dir.x)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn polyline_basics() {
        let p = [Vec2::ZERO, Vec2::new(4.0, 0.0), Vec2::new(4.0, 3.0)];
        assert_eq!(polyline_length(&p), 7.0);
        let (q, d) = point_at(&p, 5.0);
        assert!((q - Vec2::new(4.0, 1.0)).length() < 1e-5);
        assert!((d - Vec2::Y).length() < 1e-5);
        let (along, dist) = closest_on_polyline(&p, Vec2::new(2.0, 1.0));
        assert!((along - 2.0).abs() < 1e-5 && (dist - 1.0).abs() < 1e-5);
    }

    #[test]
    fn crossings_found() {
        let a = [Vec2::new(-5.0, 0.0), Vec2::new(5.0, 0.0)];
        let b = [Vec2::new(0.0, -5.0), Vec2::new(0.0, 5.0)];
        let c = polyline_crossings(&a, &b);
        assert_eq!(c.len(), 1);
        assert!((c[0].0 - 5.0).abs() < 1e-5 && (c[0].1 - 5.0).abs() < 1e-5);
    }

    #[test]
    fn footprints_are_ccw_and_contain_centre() {
        for fp in [
            Footprint::Rect { center: Vec2::new(1.0, 2.0), half: Vec2::new(3.0, 2.0), angle: 0.7 },
            Footprint::Round { center: Vec2::new(-3.0, 0.0), radius: 2.5 },
        ] {
            let poly = footprint_polygon(&fp);
            assert!(signed_area(&poly) > 0.0);
            let c = match fp {
                Footprint::Rect { center, .. } | Footprint::Round { center, .. } => center,
            };
            assert!(contains(&poly, c));
            // Outward normal of first edge points away from the centre.
            let d = (poly[1] - poly[0]).normalize();
            let mid = (poly[0] + poly[1]) * 0.5;
            assert!((mid + outward(d) - c).length() > (mid - c).length());
        }
    }
}
