//! Tracks, steps, jetties and bridges.
//!
//! All generators are deterministic (randomness comes from `Rng::at(seed, world x, z)`),
//! NaN-free, and return empty meshes for degenerate input.
//!
//! Colours come from the DESIGN.md tokens in `palette.rs`.

use crate::geom::{point_at, polyline_length};
use crate::mesh::{Marker, MarkerKind, MeshData, Rng, Surface};
use crate::model::{Bridge, BridgeKind, Hanging, Path, PathKind, Platform, SEA_LEVEL, StairKind, Stairs};
use crate::palette;
use crate::terrain::Terrain;
use glam::{Vec2, Vec3};

type Height<'a> = &'a dyn Fn(Vec2) -> f32;
type Water<'a> = &'a dyn Fn(Vec2) -> Option<f32>;

/// Ribbon height above the ground.
const LIFT: f32 = 0.03;

fn v3(p: Vec2, y: f32) -> Vec3 {
    Vec3::new(p.x, y, p.y)
}
fn flat(v: Vec2) -> Vec3 {
    Vec3::new(v.x, 0.0, v.y)
}
fn tint(c: Vec3, r: &mut Rng, amt: f32) -> Vec3 {
    c * (1.0 + (r.f() - 0.5) * amt)
}
fn rng(seed: u64, salt: u64, p: Vec2) -> Rng {
    Rng::at(seed ^ salt.wrapping_mul(0x9E37_79B9), p.x, p.y)
}
fn fin(v: f32, lo: f32, hi: f32, fallback: f32) -> f32 {
    if v.is_finite() { v.clamp(lo, hi) } else { fallback }
}
/// Every third stone of a course is lilac slate (DESIGN §A2 DryStone).
fn course_stone(k: usize, r: &mut Rng) -> Vec3 {
    if k % 3 == 2 { tint(palette::dry_stone_alt(), r, 0.08) } else { tint(palette::dry_stone(), r, 0.15) }
}
/// Thin square-section line centred on `a`→`b`.
fn thin(m: &mut MeshData, a: Vec3, b: Vec3, th: f32, c: Vec3, s: Surface) {
    let o = Vec3::Y * (th * 0.5);
    m.beam(a - o, b - o, th, th, c, s, 1.0);
}
/// Prism of `sides` facets and circumradius `r` along `a`→`b` (a horizontal-ish log), capped.
/// With `sides` divisible by 4 a flat face lies on top.
fn prism(m: &mut MeshData, a: Vec3, b: Vec3, r: f32, sides: u32, c: Vec3, s: Surface) {
    let d = b - a;
    let len = d.length();
    if len < 1e-4 || r <= 0.0 {
        return;
    }
    let d = d / len;
    let u = d.cross(Vec3::Y).normalize_or(Vec3::Z);
    let v = u.cross(d).normalize_or(Vec3::Y);
    let n = sides.max(3);
    let phi = std::f32::consts::PI / n as f32;
    let ring = |p: Vec3, i: u32| {
        let t = phi + std::f32::consts::TAU * i as f32 / n as f32;
        p + (u * t.cos() + v * t.sin()) * r
    };
    let circ = std::f32::consts::TAU * r / n as f32;
    for i in 0..n {
        let (a0, a1, b0, b1) = (ring(a, i), ring(a, i + 1), ring(b, i), ring(b, i + 1));
        m.quad_uv(
            [a0, b0, b1, a1],
            [Vec2::new(0.0, i as f32 * circ), Vec2::new(len, i as f32 * circ), Vec2::new(len, (i + 1) as f32 * circ), Vec2::new(0.0, (i + 1) as f32 * circ)],
            c,
            s,
        );
        m.tri(a, a0, a1, c * 0.9, s, 1.0);
        m.tri(b, b1, b0, c * 0.9, s, 1.0);
    }
}
/// Small hanging / standing lantern. `top` is the attachment point, `hang` the cord length.
/// Returns the glow centre.
fn lantern(m: &mut MeshData, top: Vec3, hang: f32) -> Vec3 {
    let mc = palette::metal();
    if hang > 0.01 {
        m.oriented_box(top - Vec3::Y * (hang * 0.5), Vec3::new(0.012, hang * 0.5, 0.012), Vec3::X, mc, Surface::Metal, 1.0);
    }
    let bt = top - Vec3::Y * hang;
    let hx = Vec3::new(0.09, 0.0, 0.09);
    m.aabb(bt - Vec3::Y * 0.03 - hx, bt + hx, mc, Surface::Metal, 1.0);
    m.aabb(bt - Vec3::Y * 0.24 - hx, bt - Vec3::Y * 0.21 + hx, mc, Surface::Metal, 1.0);
    let c = bt - Vec3::Y * 0.12;
    m.aabb(c - Vec3::new(0.06, 0.09, 0.06), c + Vec3::new(0.06, 0.09, 0.06), palette::glow(), Surface::Glow, 1.0);
    c
}

// ---------------------------------------------------------------------------------------------
// Tracks
// ---------------------------------------------------------------------------------------------

fn chaikin(pts: &[Vec2]) -> Vec<Vec2> {
    if pts.len() < 3 {
        return pts.to_vec();
    }
    let mut out = vec![pts[0]];
    for w in pts.windows(2) {
        out.push(w[0] * 0.75 + w[1] * 0.25);
        out.push(w[0] * 0.25 + w[1] * 0.75);
    }
    out.push(pts[pts.len() - 1]);
    out
}

/// Track draped on the terrain: a shell or pebble ribbon, slate flags set in grass, a raised
/// boardwalk on sleepers or loose stepping stones. Where it crosses water it becomes a ford
/// of boulders that stay above the high tide.
pub fn path_mesh(path: &Path, terrain: &Terrain, seed: u64) -> MeshData {
    build_path(path, seed, &|p| terrain.height(p), &|p| terrain.water_level(p))
}

/// Ford: a row of round boulders every ~0.7 m along a wet stretch, flat tops 0.32 m above the
/// water (clear of the 0.3 m tide).
#[allow(clippy::too_many_arguments)]
fn ford_stones(m: &mut MeshData, c: Vec2, s: Vec2, w: f32, level: f32, bed: f32, seed: u64) {
    let mut r = rng(seed, 11, c);
    let offs: &[f32] = if w > 1.6 { &[-0.25, 0.25] } else { &[0.0] };
    for &o in offs {
        let p = c + s * (o * w + (r.f() - 0.5) * 0.2) + Vec2::new(r.f() - 0.5, r.f() - 0.5) * 0.15;
        let rad = 0.2 + 0.1 * r.f();
        let base = bed.min(level) - 0.05;
        let top = level + 0.32;
        let col = tint(palette::rock(), &mut r, 0.2);
        m.cylinder(v3(p, base), rad * 1.15, rad, top - base, 7, true, false, col, Surface::Rock, 1.0);
    }
}

fn build_path(path: &Path, seed: u64, h: Height, wet: Water) -> MeshData {
    let mut m = MeshData::default();
    let mut clean: Vec<Vec2> = Vec::new();
    for p in path.points.iter().copied().filter(|p| p.is_finite()) {
        if clean.last().is_none_or(|q| q.distance(p) > 1e-3) {
            clean.push(p);
        }
    }
    if clean.len() < 2 {
        return m;
    }
    for _ in 0..2 {
        clean = chaikin(&clean);
    }
    let total = polyline_length(&clean);
    if total < 0.05 || total > 2000.0 {
        return m;
    }
    let w = fin(path.width, 0.3, 6.0, 1.5);
    let hw = w * 0.5;
    let n = ((total / 0.5).ceil() as usize).max(1);
    let cs: Vec<Vec2> = (0..=n).map(|i| point_at(&clean, total * i as f32 / n as f32).0).collect();
    let ts: Vec<Vec2> = (0..=n)
        .map(|i| (cs[(i + 1).min(n)] - cs[i.saturating_sub(1)]).normalize_or(Vec2::X))
        .collect();
    let side = |t: Vec2| Vec2::new(t.y, -t.x);
    let wets: Vec<Option<f32>> = cs.iter().map(|&c| wet(c)).collect();
    let (surf, base_col) = match path.kind {
        PathKind::Shell => (Surface::Shell, palette::shell()),
        PathKind::Pebble => (Surface::Pebble, palette::pebble_lilac()),
        PathKind::Flagstone => (Surface::Grass, palette::grass()),
        PathKind::Boardwalk | PathKind::Stepping => (Surface::Boards, palette::boards()),
    };
    // Shell, pebble and the grass bed under flagstones are ribbons; the rest are loose pieces.
    let ribbon = matches!(path.kind, PathKind::Shell | PathKind::Pebble | PathKind::Flagstone);

    // Ribbon columns (left edge, middle, right edge) per sample, edges jittered.
    let cols: Vec<[Vec3; 3]> = (0..=n)
        .map(|i| {
            let (c, s) = (cs[i], side(ts[i]));
            let mut r = rng(seed, 1, c);
            let j = |r: &mut Rng, amt: f32| if path.kind == PathKind::Shell { (r.f() - 0.5) * amt * 0.5 } else { (r.f() - 0.5) * amt };
            let (ja, jb, jm) = (j(&mut r, 0.2), j(&mut r, 0.2), j(&mut r, 0.1));
            let pa = c - s * (hw + ja).max(hw * 0.8);
            let pm = c + s * jm;
            let pb = c + s * (hw + jb).max(hw * 0.8);
            [pa, pm, pb].map(|p| v3(p, h(p) + LIFT))
        })
        .collect();

    let mut since = 10.0_f32;
    for i in 0..=n {
        let c = cs[i];
        let dry_next = i < n && wets[i].is_none() && wets[i + 1].is_none();
        if dry_next && ribbon {
            let mut r = rng(seed, 2, c);
            for k in 0..2 {
                let col = match path.kind {
                    // Pebbles alternate lilac and rose; joints show the honey wet sand.
                    PathKind::Pebble => {
                        let pick = r.f();
                        let base = if pick < 0.45 { palette::pebble_lilac() } else if pick < 0.9 { palette::pebble_rose() } else { palette::wet_sand() };
                        tint(base, &mut r, 0.1)
                    }
                    // Shell fades to saffron-cream sand at the edge.
                    PathKind::Shell => tint(base_col.lerp(palette::sand(), if k == 0 { 0.12 } else { 0.2 }), &mut r, 0.08),
                    _ => tint(base_col, &mut r, 0.12),
                };
                m.quad(cols[i][k], cols[i + 1][k], cols[i + 1][k + 1], cols[i][k + 1], col, surf, 1.0);
            }
        }
        if let Some(level) = wets[i] {
            if i > 0 {
                since += cs[i].distance(cs[i - 1]);
            }
            if since >= 0.7 {
                since = 0.0;
                ford_stones(&mut m, c, side(ts[i]), w, level, h(c), seed);
            }
            continue;
        }
        since = 10.0;
        let s = side(ts[i]);
        let mut r = rng(seed, 3, c);
        match path.kind {
            PathKind::Pebble => {
                // Raised beach pebbles standing proud of the ribbon.
                let count = (w.round() as usize).clamp(1, 3);
                for _ in 0..count {
                    if i >= n {
                        break;
                    }
                    let p = c + s * (r.range(-0.9, 0.9) * hw) + Vec2::new(r.f() - 0.5, r.f() - 0.5) * 0.3;
                    let rad = r.range(0.07, 0.13);
                    let pick = r.f();
                    let base = if pick < 0.4 { palette::pebble_lilac() } else if pick < 0.8 { palette::pebble_rose() } else { palette::wet_sand() };
                    let col = tint(base, &mut r, 0.2);
                    m.cylinder(v3(p, h(p) + LIFT - 0.01), rad, rad * 0.75, rad * 0.8, 5, true, false, col, Surface::Pebble, 1.0);
                }
            }
            PathKind::Shell => {
                if r.f() < 0.35 && i < n {
                    let p = c + s * (r.range(-0.8, 0.8) * hw);
                    let sz = r.range(0.03, 0.06);
                    let y = h(p) + LIFT;
                    m.aabb(v3(p, y), v3(p, y) + Vec3::new(sz, sz * 0.7, sz), tint(palette::shell_nacre(), &mut r, 0.2), Surface::Shell, 1.0);
                }
                // Pebble kerb stones along both edges, alternating lilac and rose.
                if i < n && wets[i + 1].is_none() {
                    for sg in [-1.0, 1.0] {
                        let p = c + s * (sg * (hw + 0.06));
                        let base = if (i + (sg > 0.0) as usize) % 2 == 0 { palette::pebble_lilac() } else { palette::pebble_rose() };
                        let col = tint(base, &mut r, 0.2);
                        let half = Vec3::new(r.range(0.14, 0.2), r.range(0.05, 0.08), r.range(0.07, 0.1));
                        m.oriented_box(v3(p, h(p) + half.y - 0.01), half, flat(ts[i]), col, Surface::Pebble, 1.0);
                    }
                }
            }
            PathKind::Flagstone => {
                // Irregular slate slabs, about 0.9 m, in rows with grass joints between.
                if i % 2 == 0 && i < n {
                    let cols = ((w / 0.9).round() as usize).max(1);
                    let pitch = w / cols as f32;
                    for k in 0..cols {
                        let o = (k as f32 + 0.5) * pitch - hw;
                        let mut rr = rng(seed, 5, c + s * o);
                        let ang = (rr.f() - 0.5) * 0.4;
                        let sx = Vec2::from_angle(ang).rotate(s);
                        let p = c + s * o + ts[i] * ((rr.f() - 0.5) * 0.15);
                        let col = tint(palette::flagstone().lerp(palette::flagstone_edge(), rr.f() * 0.45), &mut rr, 0.1);
                        let half = Vec3::new(pitch * 0.5 - 0.06 - rr.f() * 0.04, 0.045, rr.range(0.3, 0.42));
                        m.oriented_box(v3(p, h(p) + LIFT + 0.03), half, flat(sx), col, Surface::Flagstone, 1.0);
                    }
                }
            }
            PathKind::Stepping => {
                // Round slate stepping stones ~1 m apart, one per ~0.9 m of width, set into the ground.
                if i % 2 == 0 {
                    let cols = ((w / 0.9).round() as usize).max(1);
                    let pitch = w / cols as f32;
                    for k in 0..cols {
                        let o = (k as f32 + 0.5) * pitch - hw;
                        let mut rr = rng(seed, 6, c + s * o);
                        let p = c + s * (o + (rr.f() - 0.5) * 0.1 * pitch);
                        let rad = (pitch * 0.45).clamp(0.15, 0.45) * rr.range(0.85, 1.0);
                        let col = tint(palette::flagstone().lerp(palette::flagstone_edge(), rr.f() * 0.4), &mut rr, 0.12);
                        m.cylinder(v3(p, h(p) - 0.04), rad * 1.08, rad * 0.95, 0.16, 7, true, false, col, Surface::Flagstone, 1.0);
                    }
                }
            }
            PathKind::Boardwalk => {
                // Deck: two cross planks per sample on sleepers, clear of the highest ground
                // across the width. Slightly raised so it reads on sand.
                let ground = cols[i].iter().map(|v| v.y - LIFT).fold(f32::MIN, f32::max);
                let top = ground + 0.12;
                for o in [-0.125, 0.125] {
                    let p = c + ts[i] * o;
                    let col = tint(if r.f() < 0.25 { palette::boards_hi() } else { palette::boards() }, &mut r, 0.14);
                    m.oriented_box(v3(p, top - 0.025), Vec3::new(hw, 0.025, 0.11), flat(s), col, Surface::Boards, 1.0);
                }
                if i % 3 == 0 {
                    m.oriented_box(v3(c, top - 0.11), Vec3::new(hw + 0.05, 0.06, 0.07), flat(s), tint(palette::wood(), &mut r, 0.12), Surface::Timber, 1.0);
                }
            }
        }
        // Steep slope: little risers across the ribbon.
        if ribbon && i > 0 && i % 2 == 0 && wets[i - 1].is_none() {
            let step = cs[i].distance(cs[i - 1]).max(1e-3);
            if ((h(cs[i]) - h(cs[i - 1])) / step).abs() > 0.577 {
                m.oriented_box(v3(c, h(c) + LIFT + 0.05), Vec3::new(hw, 0.05, 0.06), flat(s), base_col * 0.85, surf, 1.0);
            }
        }
    }
    m
}

// ---------------------------------------------------------------------------------------------
// Steps
// ---------------------------------------------------------------------------------------------

/// Number of steps for a flight: ~0.25 m rise each, but never less than 0.3 m tread.
pub fn step_count(rise: f32, run: f32) -> usize {
    if !(rise > 0.0) || !run.is_finite() {
        return 0;
    }
    let n = (rise / 0.25).round().max(1.0);
    let max_n = (run / 0.3).floor().max(1.0);
    n.min(max_n) as usize
}

/// Rope ladder from `from` (low) to `to` (high): two ropes, driftwood rungs every 0.3 m and a
/// painted crossbar at the top.
fn rope_ladder(m: &mut MeshData, from: Vec3, to: Vec3, w: f32, side: Vec3, trim: Vec3) {
    let len = from.distance(to);
    if len < 0.05 {
        return;
    }
    let off = side * (w * 0.5);
    for sg in [-1.0, 1.0] {
        thin(m, from + off * sg, to + off * sg, 0.045, palette::rope(), Surface::Fiber);
    }
    let rungs = ((len / 0.3).ceil() as usize).clamp(1, 200);
    for k in 1..rungs {
        let p = from.lerp(to, k as f32 / rungs as f32);
        m.beam(p - off * 1.08, p + off * 1.08, 0.06, 0.05, palette::driftwood(), Surface::Timber, 1.0);
    }
    m.beam(to - off * 1.2 + Vec3::Y * 0.02, to + off * 1.2 + Vec3::Y * 0.02, 0.1, 0.08, trim, Surface::Carving, 1.0);
}

pub fn stairs_mesh(stairs: &Stairs, terrain: &Terrain, seed: u64) -> (MeshData, Vec<Marker>) {
    let mut m = MeshData::default();
    let mk = Vec::new();
    if !(stairs.from.is_finite() && stairs.to.is_finite()) {
        return (m, mk);
    }
    let (from, to) = if stairs.from.y <= stairs.to.y { (stairs.from, stairs.to) } else { (stairs.to, stairs.from) };
    let d = Vec3::new(to.x - from.x, 0.0, to.z - from.z);
    let (run, rise) = (d.length(), to.y - from.y);
    if (run < 0.05 && rise < 0.05) || run > 100.0 || rise > 50.0 {
        return (m, mk);
    }
    let dir = if run > 0.05 { d / run } else { Vec3::X };
    let side = dir.cross(Vec3::Y);
    let trim = palette::trim(stairs.colors);

    // Anything steeper than 1.5 rise/run cannot be a flight of steps: hang a rope ladder.
    if stairs.kind == StairKind::RopeLadder || run < 0.05 || rise > run * 1.5 {
        let w = fin(stairs.width, 0.3, 0.6, 0.6).min(0.6);
        rope_ladder(&mut m, from, to, w, side, trim);
        return (m, mk);
    }

    let w = fin(stairs.width, 0.5, 4.0, 1.2);
    let n = if rise < 0.12 { 1 } else { step_count(rise, run) };
    let (tr, rs) = (run / n as f32, rise / n as f32);
    let at = |s: f32| from + dir * s;
    for i in 0..n {
        let top = from.y + (i + 1) as f32 * rs;
        let c = at((i as f32 + 0.5) * tr);
        let mut r = rng(seed, 21, Vec2::new(c.x, c.z));
        // Lowest ground across the tread, so the step always reaches the earth.
        let g = [0.0, 1.0, -1.0]
            .iter()
            .map(|o| {
                let p = c + side * (o * w * 0.5);
                terrain.height(Vec2::new(p.x, p.z))
            })
            .fold(f32::MAX, f32::min);
        let base = (g - 0.05).min(top - 0.2);
        match stairs.kind {
            StairKind::Stone => {
                // Cheek walls either side, a cut block and a slightly proud nosing slab.
                let ground = terrain.height(Vec2::new(c.x, c.z));
                let wb = (ground - 0.05).min(top - 0.2);
                for sg in [-1.0, 1.0] {
                    let p = c + side * (sg * (w * 0.5 + 0.06));
                    m.oriented_box(
                        Vec3::new(p.x, (top + 0.3 + wb) * 0.5, p.z),
                        Vec3::new(0.06, (top + 0.3 - wb) * 0.5, tr * 0.5),
                        side,
                        course_stone(i + (sg > 0.0) as usize, &mut r) * 0.9,
                        Surface::DryStone,
                        1.0,
                    );
                }
                let col = course_stone(i, &mut r);
                m.oriented_box(Vec3::new(c.x, (top - 0.05 + base) * 0.5, c.z), Vec3::new(w * 0.5, (top - 0.05 - base) * 0.5, tr * 0.5), side, col, Surface::DryStone, 1.0);
                m.oriented_box(Vec3::new(c.x, top - 0.025, c.z), Vec3::new(w * 0.5 + 0.02, 0.025, tr * 0.5 + 0.025), side, col * 1.06, Surface::DryStone, 1.0);
            }
            StairKind::Log => {
                // Earth tread held back by a log riser and two driven stakes.
                let front = at(i as f32 * tr + 0.1);
                let col = tint(palette::bark(), &mut r, 0.15);
                m.oriented_box(
                    Vec3::new(c.x, (top - 0.03 + base) * 0.5, c.z),
                    Vec3::new(w * 0.5, (top - 0.03 - base) * 0.5, tr * 0.5),
                    side,
                    tint(palette::dirt(), &mut r, 0.12),
                    Surface::Dirt,
                    1.0,
                );
                let lo = Vec3::new(front.x, top - 0.09, front.z);
                prism(&mut m, lo - side * (w * 0.5 + 0.1), lo + side * (w * 0.5 + 0.1), 0.1, 6, col, Surface::Timber);
                for sg in [-1.0, 1.0] {
                    let p = front + side * (sg * (w * 0.5 + 0.12));
                    let b = base.min(top - 0.5);
                    m.cylinder(Vec3::new(p.x, b, p.z), 0.055, 0.035, top + 0.12 - b, 6, true, false, tint(palette::wood(), &mut r, 0.15), Surface::Timber, 1.0);
                }
            }
            StairKind::RopeLadder => {}
        }
    }
    (m, mk)
}

// ---------------------------------------------------------------------------------------------
// Jetty
// ---------------------------------------------------------------------------------------------

/// Points around a local rectangle's perimeter, ~`spacing` apart, corners included.
fn perimeter(hx: f32, hz: f32, spacing: f32) -> Vec<Vec2> {
    let c = [Vec2::new(-hx, -hz), Vec2::new(hx, -hz), Vec2::new(hx, hz), Vec2::new(-hx, hz)];
    let mut out = Vec::new();
    for i in 0..4 {
        let (a, b) = (c[i], c[(i + 1) % 4]);
        let n = ((a.distance(b) / spacing).ceil() as usize).max(1);
        for k in 0..n {
            out.push(a.lerp(b, k as f32 / n as f32));
        }
    }
    out
}

/// One edge of the jetty rectangle.
struct Edge {
    /// Outward unit direction (world).
    out: Vec2,
    /// Unit direction along the edge (world).
    along: Vec2,
    /// Centre-to-edge distance and half length of the edge.
    dist: f32,
    half_len: f32,
    sea: bool,
    /// Where a canoe moors off this edge: the nearest spot out from it that stays afloat at
    /// low tide (`Terrain::boat_water`), if any within reach.
    boat: Option<Vec2>,
}

/// A jetty: plank deck on log piles that run down to the seabed, mooring posts with rope coils
/// at the corners, a ladder to the water and one or two canoes alongside. On land (no sea
/// beside it) it is a raised stage with a railing and a gap in front.
pub fn platform_mesh(platform: &Platform, terrain: &Terrain, seed: u64) -> (MeshData, Vec<Marker>) {
    let mut m = MeshData::default();
    let mut mk = Vec::new();
    let p = platform;
    if !(p.center.is_finite() && p.half.is_finite() && p.angle.is_finite() && p.height.is_finite()) {
        return (m, mk);
    }
    let (hx, hz) = (p.half.x.abs().clamp(0.25, 30.0), p.half.y.abs().clamp(0.25, 30.0));
    let height = p.height.clamp(0.15, 12.0);
    let ax = Vec2::from_angle(p.angle);
    let az = Vec2::new(-ax.y, ax.x);
    let (ax3, az3) = (flat(ax), flat(az));
    let world = |l: Vec2| p.center + ax * l.x + az * l.y;
    // `height` is measured from the highest ground or water under the deck, so the deck never
    // sinks into the beach at the landward end and the Raise phase always lifts it.
    let mut gmax = f32::MIN;
    let mut over_sea = false;
    for i in 0..=4 {
        for j in 0..=4 {
            let q = world(Vec2::new(-hx + 2.0 * hx * i as f32 / 4.0, -hz + 2.0 * hz * j as f32 / 4.0));
            let g = terrain.height(q);
            gmax = gmax.max(terrain.water_level(q).map_or(g, |l| l.max(g)));
            over_sea |= terrain.is_sea(q);
        }
    }
    let mut top = gmax + height;
    if over_sea {
        // The deck always clears the water, high tide included.
        top = top.max(SEA_LEVEL + 0.45);
    }
    let trim = palette::trim(p.colors);
    let mut r = rng(seed, 31, p.center);

    // Edges: +x, -x, +z, -z in local space; sea if the water just beyond them is sea.
    let edges: Vec<Edge> = [(ax, az, hx, hz), (-ax, az, hx, hz), (az, ax, hz, hx), (-az, ax, hz, hx)]
        .into_iter()
        .map(|(out, along, dist, half_len)| {
            let sea = terrain.is_sea(p.center + out * (dist + 1.1));
            let boat = [1.1, 1.6, 2.2, 2.8]
                .into_iter()
                .map(|off| p.center + out * (dist + off))
                .find(|q| terrain.is_sea(*q) && terrain.boat_water(*q).is_some());
            Edge { out, along, dist, half_len, sea, boat }
        })
        .collect();
    let jetty = over_sea || edges.iter().any(|e| e.sea);

    // Deck on two bearers running the long way.
    m.oriented_box(v3(p.center, top - 0.07), Vec3::new(hx, 0.07, hz), ax3, tint(palette::boards(), &mut r, 0.08), Surface::Boards, 1.0);
    let by = top - 0.14 - 0.06;
    let bearer = tint(palette::wood(), &mut r, 0.1);
    if hx >= hz {
        for sg in [-1.0, 1.0] {
            m.oriented_box(v3(world(Vec2::new(0.0, sg * (hz - 0.15))), by), Vec3::new(hx, 0.06, 0.07), ax3, bearer, Surface::Timber, 1.0);
        }
    } else {
        for sg in [-1.0, 1.0] {
            m.oriented_box(v3(world(Vec2::new(sg * (hx - 0.15), 0.0)), by), Vec3::new(hz, 0.06, 0.07), az3, bearer, Surface::Timber, 1.0);
        }
    }

    // Log piles down to the terrain (seabed over water), with a waler tying them when deep.
    let under = top - 0.2;
    let per = perimeter((hx - 0.18).max(0.1), (hz - 0.18).max(0.1), (4.0 * (hx + hz) / 90.0).max(1.8));
    let bases: Vec<f32> = per.iter().map(|l| terrain.height(world(*l)) - 0.2).collect();
    for (l, base) in per.iter().zip(&bases) {
        if under - base > 0.1 {
            let q = world(*l);
            let rad = r.range(0.1, 0.13);
            m.cylinder(v3(q, *base), rad * 1.1, rad, under - base, 6, false, false, tint(palette::wood(), &mut r, 0.2), Surface::Timber, 1.0);
        }
    }
    let wy = under - 0.45;
    for i in 0..per.len() {
        let j = (i + 1) % per.len();
        if bases[i] < wy - 0.3 && bases[j] < wy - 0.3 {
            thin(&mut m, v3(world(per[i]), wy), v3(world(per[j]), wy), 0.07, palette::wood() * 0.92, Surface::Timber);
        }
    }

    // Corner mooring posts with rope coils and painted caps (jetty), or plain perches (stage).
    let ins = 0.18_f32.min(hx.min(hz) * 0.5);
    let corners = [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)];
    let mut perch_y = top;
    let mut railing = false;
    if jetty {
        let post_top = top + 0.6;
        perch_y = post_top + 0.07;
        for (sx, sz) in corners {
            let q = world(Vec2::new(sx * (hx - ins), sz * (hz - ins)));
            let base = terrain.height(q) - 0.2;
            m.cylinder(v3(q, base), 0.14, 0.12, post_top - base, 7, true, false, tint(palette::wood(), &mut r, 0.15), Surface::Timber, 1.0);
            m.cylinder(v3(q, post_top), 0.16, 0.16, 0.07, 7, true, false, trim, Surface::Carving, 1.0);
            for (k, rad) in [0.21, 0.19, 0.21].into_iter().enumerate() {
                m.cylinder(v3(q, top + 0.1 + 0.05 * k as f32), rad, rad, 0.045, 8, true, false, tint(palette::rope(), &mut r, 0.1), Surface::Fiber, 1.0);
            }
        }
    } else if height > 1.2 {
        railing = true;
        perch_y = top + 0.9;
    }
    if railing {
        let per = perimeter(hx - 0.06, hz - 0.06, 1.5);
        let gap = |l: Vec2| l.y < 0.0 && (l.y + hz - 0.06).abs() < 1e-3 && l.x.abs() <= 1.0;
        for (i, l) in per.iter().enumerate() {
            let l2 = per[(i + 1) % per.len()];
            if gap(*l) || gap(l2) {
                continue;
            }
            let (q, q2) = (world(*l), world(l2));
            m.cylinder(v3(q, top), 0.025, 0.025, 0.9, 6, true, false, trim, Surface::Timber, 1.0);
            for hgt in [0.9, 0.45] {
                m.beam(v3(q, top + hgt - 0.05), v3(q2, top + hgt - 0.05), 0.04, 0.04, trim, Surface::Timber, 1.0);
            }
        }
    }
    for (sx, sz) in corners {
        mk.push(Marker { kind: MarkerKind::Perch, pos: v3(world(Vec2::new(sx * (hx - ins), sz * (hz - ins))), perch_y), dir: Vec3::Y });
    }

    // Canoes alongside: prefer the long sea edges, one or two. `dir` runs along the edge.
    let mut order: Vec<usize> = (0..4).filter(|&i| edges[i].sea && edges[i].boat.is_some()).collect();
    order.sort_by(|&a, &b| edges[b].half_len.total_cmp(&edges[a].half_len));
    let mut boats = vec![false; 4];
    for (rank, &i) in order.iter().take(2).enumerate() {
        if rank == 0 || r.f() < 0.5 {
            let e = &edges[i];
            boats[i] = true;
            if let Some(q) = e.boat {
                mk.push(Marker { kind: MarkerKind::Boat, pos: v3(q, SEA_LEVEL), dir: flat(e.along) });
            }
        }
    }

    // Ladder down to the water on a sea edge, preferring one without a canoe (then the short end).
    let ladder = (0..4).filter(|&i| edges[i].sea).min_by(|&a, &b| {
        (boats[a], edges[a].half_len).partial_cmp(&(boats[b], edges[b].half_len)).unwrap_or(std::cmp::Ordering::Equal)
    });
    if let Some(i) = ladder {
        let e = &edges[i];
        let along = if boats[i] { e.half_len * 0.7 } else { 0.0 };
        let foot = p.center + e.out * (e.dist + 0.06) + e.along * along;
        let low = (terrain.height(foot) + 0.1).max(SEA_LEVEL - 0.8).min(top - 0.6);
        let rail_top = top + 0.35;
        for sg in [-1.0, 1.0] {
            let q = foot + e.along * (sg * 0.22);
            m.oriented_box(v3(q, (low + rail_top) * 0.5), Vec3::new(0.025, (rail_top - low) * 0.5, 0.025), flat(e.along), palette::wood(), Surface::Timber, 1.0);
        }
        let rungs = (((top - 0.2 - low) / 0.3).floor() as usize).min(40);
        for k in 0..=rungs {
            m.oriented_box(v3(foot, top - 0.2 - 0.3 * k as f32), Vec3::new(0.24, 0.02, 0.02), flat(e.along), palette::driftwood(), Surface::Timber, 1.0);
        }
    }
    (m, mk)
}

// ---------------------------------------------------------------------------------------------
// Bridges
// ---------------------------------------------------------------------------------------------

struct Fr {
    a: Vec3,
    b: Vec3,
    dir: Vec3,
    side: Vec3,
    len: f32,
    w: f32,
}

impl Fr {
    fn new(br: &Bridge) -> Option<Fr> {
        if !(br.a.is_finite() && br.b.is_finite()) {
            return None;
        }
        let d = Vec3::new(br.b.x - br.a.x, 0.0, br.b.z - br.a.z);
        let len = d.length();
        if !(0.5..=300.0).contains(&len) {
            return None;
        }
        let dir = d / len;
        Some(Fr { a: br.a, b: br.b, dir, side: dir.cross(Vec3::Y), len, w: fin(br.width, 0.6, 6.0, 1.5) })
    }
    /// Straight deck line at `t` in 0..=1.
    fn base(&self, t: f32) -> Vec3 {
        self.a.lerp(self.b, t)
    }
    fn xz(&self, v: Vec3) -> Vec2 {
        Vec2::new(v.x, v.z)
    }
}

/// Four posts, one per deck corner at each end, with Perch markers on top and an optional
/// painted cap.
#[allow(clippy::too_many_arguments)]
fn end_posts(m: &mut MeshData, mk: &mut Vec<Marker>, f: &Fr, lat: f32, out: f32, top_h: f32, half: f32, surf: Surface, col: Vec3, cap: Option<Vec3>, terrain: &Terrain) {
    for (t, o) in [(0.0, -1.0), (1.0, 1.0)] {
        for sg in [-1.0, 1.0] {
            let b = f.base(t);
            let p = b + f.dir * (o * out) + f.side * (sg * lat);
            let bot = (terrain.height(f.xz(p)) - 0.2).min(b.y - 0.3);
            let top = b.y + top_h;
            m.oriented_box(Vec3::new(p.x, (bot + top) * 0.5, p.z), Vec3::new(half, (top - bot) * 0.5, half), f.side, col, surf, 1.0);
            let mut pt = top;
            if let Some(c) = cap {
                m.oriented_box(Vec3::new(p.x, top + 0.04, p.z), Vec3::new(half + 0.03, 0.04, half + 0.03), f.side, c, Surface::Carving, 1.0);
                pt += 0.08;
            }
            mk.push(Marker { kind: MarkerKind::Perch, pos: Vec3::new(p.x, pt, p.z), dir: Vec3::Y });
        }
    }
}

pub fn bridge_mesh(bridge: &Bridge, terrain: &Terrain, seed: u64) -> (MeshData, Vec<Marker>) {
    let mut m = MeshData::default();
    let mut mk = Vec::new();
    let Some(f) = Fr::new(bridge) else { return (m, mk) };
    match bridge.kind {
        BridgeKind::Rope { railing } => rope_bridge(&mut m, &mut mk, &f, bridge, railing.min(3), terrain, seed),
        BridgeKind::Causeway => causeway(&mut m, &mut mk, &f, terrain, seed),
        BridgeKind::Log => log_bridge(&mut m, &mut mk, &f, bridge, terrain, seed),
    }
    (m, mk)
}

fn rope_bridge(m: &mut MeshData, mk: &mut Vec<Marker>, f: &Fr, br: &Bridge, railing: u8, terrain: &Terrain, seed: u64) {
    let trim = palette::trim(br.colors);
    let (rope, knot) = (palette::rope(), palette::fiber_dark());
    let sag = (f.len * 0.06).clamp(0.25, 3.0);
    let bell = |t: f32| 4.0 * t * (1.0 - t);
    let deck = |t: f32| f.base(t) - Vec3::Y * (sag * bell(t));
    // Hand lines run parallel to the sagging deck.
    let hand = |t: f32| deck(t) + Vec3::Y * 1.0;
    let lat = f.w * 0.5 + 0.02;
    let nseg = (f.len.ceil() as usize).max(2);
    let tt = |k: usize| k as f32 / nseg as f32;

    // Sagging plank deck: a plank every 0.34 m.
    let pitch = 0.34;
    for k in 0..(f.len / pitch).floor() as usize {
        let (s0, s1) = (k as f32 * pitch, k as f32 * pitch + 0.27);
        let mut r = rng(seed, 41, f.xz(f.base(s0 / f.len)));
        let base = if r.f() < 0.2 { palette::boards_hi() } else { palette::boards() };
        m.beam(deck(s0 / f.len), deck((s1 / f.len).min(1.0)), f.w, 0.05, tint(base, &mut r, 0.16), Surface::Boards, 1.0);
    }
    for sg in [-1.0_f32, 1.0] {
        let o = f.side * (sg * lat);
        for k in 0..nseg {
            // Carrying rope under the deck edge, and the hand line.
            let so = f.side * (sg * (f.w * 0.5 - 0.05));
            thin(m, deck(tt(k)) + so - Vec3::Y * 0.05, deck(tt(k + 1)) + so - Vec3::Y * 0.05, 0.06, rope, Surface::Fiber);
            thin(m, hand(tt(k)) + o, hand(tt(k + 1)) + o, 0.06, rope, Surface::Fiber);
            let (e0, e1) = (deck(tt(k)) + o + Vec3::Y * 0.05, deck(tt(k + 1)) + o + Vec3::Y * 0.05);
            let (h0, h1) = (hand(tt(k)) + o, hand(tt(k + 1)) + o);
            let hh = h0.y - e0.y;
            match railing {
                // Plain: a lower rope and a hanger every other metre.
                0 => {
                    thin(m, e0.lerp(h0, 0.45), e1.lerp(h1, 0.45), 0.04, rope, Surface::Fiber);
                    if k % 2 == 0 {
                        thin(m, e0, h0, 0.03, rope, Surface::Fiber);
                    }
                }
                // Netting: crossing diagonals make the diamond mesh.
                1 => {
                    thin(m, e0, h1, 0.03, rope, Surface::Fiber);
                    thin(m, h0, e1, 0.03, rope, Surface::Fiber);
                }
                // Plank balusters under a lower rope.
                2 => {
                    thin(m, e0.lerp(h0, 0.5), e1.lerp(h1, 0.5), 0.04, rope, Surface::Fiber);
                    for q in [0.0, 0.5] {
                        let b = e0.lerp(e1, q);
                        m.oriented_box(b + Vec3::Y * (hh * 0.5), Vec3::new(0.03, hh * 0.5, 0.02), f.side, tint(palette::boards(), &mut rng(seed, 43, f.xz(b)), 0.15), Surface::Boards, 1.0);
                    }
                }
                // Knotted rope: a knot on every span, painted floats hanging from every other.
                _ => {
                    thin(m, e0.lerp(h0, 0.5), e1.lerp(h1, 0.5), 0.04, rope, Surface::Fiber);
                    m.aabb(h0 - Vec3::splat(0.045), h0 + Vec3::splat(0.045), knot, Surface::Fiber, 1.0);
                    if k % 2 == 0 {
                        let c = h0.lerp(h1, 0.5);
                        thin(m, c, c - Vec3::Y * 0.14, 0.02, knot, Surface::Fiber);
                        m.cylinder(c - Vec3::Y * 0.31, 0.07, 0.07, 0.17, 6, true, true, trim, Surface::Carving, 1.0);
                    }
                }
            }
        }
    }
    end_posts(m, mk, f, lat + 0.03, 0.2, 1.3, 0.1, Surface::Timber, palette::wood(), Some(trim), terrain);

    match br.hanging {
        Hanging::Lanterns => {
            let n = ((f.len / 3.0).floor() as usize).max(1);
            for k in 0..n {
                let t = (k as f32 + 0.5) / n as f32;
                for sg in [-1.0_f32, 1.0] {
                    let c = lantern(m, hand(t) + f.side * (sg * lat) - Vec3::Y * 0.04, 0.15);
                    mk.push(Marker { kind: MarkerKind::Lantern, pos: c, dir: Vec3::Y });
                }
            }
        }
        Hanging::Pennants => {
            // Small cloth triangles on the hand lines, painted in the trim and pigment colours.
            let n = ((f.len / 2.0).floor() as usize).max(2);
            for k in 0..n {
                let t = (k as f32 + 0.5) / n as f32;
                let sg = if k % 2 == 0 { 1.0 } else { -1.0 };
                let p = hand(t) + f.side * (sg * lat);
                let q = hand((t + 0.6 / f.len).min(1.0)) + f.side * (sg * lat);
                let tip = p.lerp(q, 0.5) - Vec3::Y * 0.45;
                let base = if k % 3 == 0 { trim } else { palette::pigment(k as u32) };
                let col = tint(base, &mut rng(seed, 42, f.xz(p)), 0.1);
                let from = m.vertex_count();
                m.tri(p, tip, q, col, Surface::Hide, 1.0);
                m.tri(p, q, tip, col, Surface::Hide, 1.0);
                // Tied along the hand line, free at the tip.
                m.flutter(from, |v| if v.distance(tip) < 1e-4 { 1.0 } else { 0.0 });
                // A sailcloth heading strip along the line.
                m.beam(p, q, 0.02, 0.05, palette::sailcloth(), Surface::Hide, 1.0);
                mk.push(Marker { kind: MarkerKind::Flag, pos: p, dir: Vec3::Y });
            }
        }
        Hanging::None => {}
    }
}

/// Stacked-stone causeway on a gentle hump, with semicircular culvert arches the water runs
/// through, a low parapet and copestones.
fn causeway(m: &mut MeshData, mk: &mut Vec<Marker>, f: &Fr, terrain: &Terrain, seed: u64) {
    const TH: f32 = 0.4;
    let hump = (f.len * 0.04).clamp(0.3, 1.0);
    let n = (f.len / 0.5).ceil() as usize;
    let dx = f.len / n as f32;
    let top = |j: usize| {
        let t = j as f32 / n as f32;
        f.base(t) + Vec3::Y * (hump * 4.0 * t * (1.0 - t))
    };
    let ground = |j: usize, sg: f32| {
        let p = top(j) + f.side * (sg * f.w * 0.5);
        terrain.height(Vec2::new(p.x, p.z))
    };
    // Arches, snapped to the column grid: (centre column, half width in columns, springline y).
    let na = (f.len / 8.0).ceil() as usize;
    let cell = f.len / na as f32;
    let mut arches: Vec<(usize, usize, f32)> = Vec::new();
    for k in 0..na {
        let cj = (((k as f32 + 0.5) * cell) / dx).round() as usize;
        let crown = top(cj).y - TH - 0.3;
        let gmin = (0..=4)
            .map(|i| {
                let j = (((k as f32 + i as f32 / 4.0) * cell) / dx).round() as usize;
                ground(j.min(n), 0.0)
            })
            .fold(f32::MAX, f32::min);
        let r = (cell * 0.4).min(4.0).min((crown - gmin).max(0.5));
        let rc = (r / dx).floor() as usize;
        if rc >= 1 && cj >= rc && cj + rc <= n {
            arches.push((cj, rc, crown - rc as f32 * dx));
        }
    }
    let bottom = |j: usize, sg: f32| -> f32 {
        let g = ground(j, sg);
        for &(cj, rc, spring) in &arches {
            if j >= cj - rc && j <= cj + rc {
                let r = rc as f32 * dx;
                let x = (j as f32 - cj as f32) * dx;
                return (spring + (r * r - x * x).max(0.0).sqrt()).max(g);
            }
        }
        g - 0.25
    };
    let in_arch = |j: usize| arches.iter().any(|&(cj, rc, _)| j >= cj - rc && j < cj + rc);
    for j in 0..n {
        let (p0, p1) = (top(j), top(j + 1));
        let mut r = rng(seed, 51, Vec2::new(p0.x, p0.z));
        let col = tint(palette::dry_stone(), &mut r, 0.15);
        m.beam(p0 - Vec3::Y * TH, p1 - Vec3::Y * TH, f.w, TH, col, Surface::DryStone, 1.0);
        for sg in [-1.0_f32, 1.0] {
            let o = f.side * (sg * f.w * 0.5);
            let pc = f.side * (sg * (f.w * 0.5 - 0.12));
            m.beam(p0 + pc, p1 + pc, 0.24, 0.5, col * 1.05, Surface::DryStone, 1.0);
            // Copestones on every other span, every third one lilac slate.
            if j % 2 == 0 {
                let mid = (p0 + p1) * 0.5 + pc + Vec3::Y * 0.54;
                m.oriented_box(mid, Vec3::new(0.17, 0.06, dx * 0.45), f.side, course_stone(j / 2 + (sg > 0.0) as usize, &mut r), Surface::DryStone, 1.0);
            }
            let (t0, t1) = (p0.y - TH, p1.y - TH);
            let (b0, b1) = (bottom(j, sg).min(t0 - 0.01), bottom(j + 1, sg).min(t1 - 0.01));
            if t0 - b0 > 0.02 || t1 - b1 > 0.02 {
                let q = |p: Vec3, y: f32| p + o + Vec3::Y * (y - p.y);
                let (a, b, c, d) = (q(p0, b0), q(p1, b1), q(p1, t1), q(p0, t0));
                if sg > 0.0 { m.quad(a, b, c, d, col * 0.95, Surface::DryStone, 1.0) } else { m.quad(b, a, d, c, col * 0.95, Surface::DryStone, 1.0) }
            }
        }
        if in_arch(j) {
            // Soffit (underside of the arch), facing down.
            let (c0, c1) = (bottom(j, 0.0), bottom(j + 1, 0.0));
            if c0 < p0.y - TH && c1 < p1.y - TH {
                let q = |p: Vec3, y: f32, sg: f32| p + f.side * (sg * f.w * 0.5) + Vec3::Y * (y - p.y);
                m.quad(q(p0, c0, -1.0), q(p1, c1, -1.0), q(p1, c1, 1.0), q(p0, c0, 1.0), col * 0.8, Surface::DryStone, 1.0);
            }
        }
    }
    // Pier walls inside each arch opening.
    for &(cj, rc, spring) in &arches {
        for (j, facing) in [(cj - rc, 1.0), (cj + rc, -1.0)] {
            let p = top(j);
            let bot = ground(j, 0.0) - 0.25;
            if spring - bot > 0.05 {
                let at = |sg: f32, y: f32| p + f.side * (sg * f.w * 0.5) + Vec3::Y * (y - p.y);
                let (a, b, c, d) = (at(-1.0, bot), at(-1.0, spring), at(1.0, spring), at(1.0, bot));
                let col = palette::dry_stone() * 0.85;
                if facing > 0.0 { m.quad(a, b, c, d, col, Surface::DryStone, 1.0) } else { m.quad(a, d, c, b, col, Surface::DryStone, 1.0) }
            }
        }
    }
    end_posts(m, mk, f, f.w * 0.5 - 0.12, 0.0, 0.9, 0.2, Surface::DryStone, palette::dry_stone() * 1.1, None, terrain);
}

/// A big driftwood log (more side by side on a wide bridge) with a hewn walking top, branch
/// stubs, A-frame trestle legs under it and a hand rope between the end posts.
fn log_bridge(m: &mut MeshData, mk: &mut Vec<Marker>, f: &Fr, br: &Bridge, terrain: &Terrain, seed: u64) {
    let trim = palette::trim(br.colors);
    let r = (f.w * 0.4).clamp(0.25, 0.8);
    let apothem = r * (std::f32::consts::PI / 8.0).cos();
    let logs = ((f.w / (2.0 * r * 0.95)).ceil() as usize).clamp(1, 4);
    let lane = |i: usize| if logs == 1 { 0.0 } else { (i as f32 / (logs - 1) as f32 - 0.5) * (f.w - 2.0 * r) };
    let drop = Vec3::Y * apothem;
    let trestles = ((f.len / 4.0).ceil() as usize).saturating_sub(1).max(1);
    for i in 0..logs {
        let o = f.side * lane(i);
        let mut rr = rng(seed, 61, f.xz(f.base(0.5)) + Vec2::splat(i as f32));
        let col = tint(palette::driftwood(), &mut rr, 0.1);
        let (a, b) = (f.a + o - drop, f.b + o - drop);
        prism(m, a, b, r, 8, col, Surface::Timber);
        // Hewn flat top.
        m.beam(f.a + o - Vec3::Y * 0.01, f.b + o - Vec3::Y * 0.01, r * 0.8, 0.03, tint(palette::boards(), &mut rr, 0.1), Surface::Boards, 1.0);
        // Branch stubs poking out of the sides.
        let stubs = (f.len / 1.6).floor() as usize;
        for k in 0..stubs {
            let t = (k as f32 + rr.f()) / stubs.max(1) as f32;
            let sg = if rr.f() < 0.5 { -1.0 } else { 1.0 };
            let c = f.base(t) - drop + f.side * lane(i);
            thin(m, c + f.side * (sg * r * 0.8), c + f.side * (sg * r * 1.35) + Vec3::Y * 0.12, 0.08, col * 0.9, Surface::Timber);
        }
        // Trestles: two splayed legs, a cross brace and a cap beam.
        for k in 1..=trestles {
            let t = k as f32 / (trestles + 1) as f32;
            let top = f.base(t) + o - drop - Vec3::Y * (apothem * 0.5);
            let g = terrain.height(f.xz(top)) - 0.2;
            if top.y - g < 0.5 {
                continue;
            }
            let splay = (0.2 + (top.y - g) * 0.12).min(1.2);
            let leg = tint(palette::wood(), &mut rr, 0.15);
            let mut feet = [Vec3::ZERO; 2];
            for (n, sg) in [-1.0, 1.0].into_iter().enumerate() {
                let foot_p = f.xz(top) + Vec2::new(f.side.x, f.side.z) * (sg * (r * 0.5 + splay));
                feet[n] = v3(foot_p, terrain.height(foot_p) - 0.2);
                thin(m, top + f.side * (sg * r * 0.5), feet[n], 0.14, leg, Surface::Timber);
            }
            thin(m, feet[0].lerp(top, 0.4), feet[1].lerp(top, 0.4), 0.08, leg * 0.9, Surface::Timber);
            thin(m, top - f.side * (r * 0.9), top + f.side * (r * 0.9), 0.12, leg, Surface::Timber);
        }
    }
    // Hand rope on both sides between the end posts.
    let lat = f.w * 0.5 + 0.05;
    for sg in [-1.0_f32, 1.0] {
        let o = f.side * (sg * lat);
        let sagging = |t: f32| Vec3::Y * (1.0 - 0.08 * 4.0 * t * (1.0 - t));
        let nseg = ((f.len / 1.5).ceil() as usize).max(1);
        for k in 0..nseg {
            let (t0, t1) = (k as f32 / nseg as f32, (k + 1) as f32 / nseg as f32);
            thin(m, f.base(t0) + o + sagging(t0), f.base(t1) + o + sagging(t1), 0.05, palette::rope(), Surface::Fiber);
        }
    }
    end_posts(m, mk, f, lat, 0.12, 1.15, 0.09, Surface::Timber, palette::wood(), Some(trim), terrain);
}

// ---------------------------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Colors, TerrainDab, TerrainOp};

    fn path(kind: PathKind, len: f32, width: f32) -> Path {
        Path { points: vec![Vec2::new(0.0, 0.0), Vec2::new(len * 0.5, 3.0), Vec2::new(len, 0.0)], width, kind }
    }
    fn tris(m: &MeshData) -> usize {
        m.indices.len() / 3
    }
    fn t() -> Terrain {
        Terrain::build(1, &[], &[])
    }
    /// Terrain with a deep sea hole at `c`.
    fn sea_at(c: Vec2) -> Terrain {
        Terrain::build(4, &[TerrainDab { center: c, radius: 9.0, op: TerrainOp::Sculpt { amount: -6.0 } }], &[])
    }
    fn stairs(kind: StairKind, rise: f32) -> Stairs {
        Stairs { from: Vec3::new(0.0, 0.0, 0.0), to: Vec3::new(6.0, rise, 1.0), width: 1.2, kind, colors: Colors::default() }
    }
    fn bridge(kind: BridgeKind, hanging: Hanging) -> Bridge {
        Bridge { a: Vec3::new(0.0, 1.0, 0.0), b: Vec3::new(14.0, 1.5, 3.0), width: 1.6, kind, hanging, colors: Colors::default() }
    }
    fn count(m: &MeshData, s: Surface) -> usize {
        m.surfaces.iter().filter(|x| **x == s as u8).count()
    }
    fn straight(kind: PathKind, w: f32) -> Path {
        Path { points: vec![Vec2::new(0.0, 0.0), Vec2::new(12.0, 0.0)], width: w, kind }
    }
    const KINDS: [PathKind; 5] = [PathKind::Shell, PathKind::Pebble, PathKind::Flagstone, PathKind::Boardwalk, PathKind::Stepping];

    #[test]
    fn path_kinds_valid_and_deterministic() {
        for kind in KINDS {
            let p = path(kind, 12.0, 2.0);
            let a = path_mesh(&p, &t(), 5);
            assert!(!a.is_empty() && a.is_valid(), "{kind:?}");
            assert_eq!(a, path_mesh(&p, &t(), 5));
            assert_ne!(a, path_mesh(&p, &t(), 6));
        }
    }

    #[test]
    fn path_kinds_use_their_own_surfaces() {
        let m = |k| build_path(&straight(k, 2.0), 3, &|_| 0.0, &|_| None);
        let shell = m(PathKind::Shell);
        assert!(count(&shell, Surface::Shell) > 0 && count(&shell, Surface::Pebble) > 0, "shell track has a pebble kerb");
        let pebble = m(PathKind::Pebble);
        assert!(count(&pebble, Surface::Pebble) > count(&pebble, Surface::Shell));
        let flag = m(PathKind::Flagstone);
        assert!(count(&flag, Surface::Flagstone) > 0 && count(&flag, Surface::Grass) > 0, "slabs with grass joints");
        let board = m(PathKind::Boardwalk);
        assert!(count(&board, Surface::Boards) > 0 && count(&board, Surface::Timber) > 0, "planks on sleepers");
        let step = m(PathKind::Stepping);
        assert!(count(&step, Surface::Flagstone) > 0 && count(&step, Surface::Grass) == 0, "loose stones, no ribbon");
        // Pebbles are multi-coloured.
        let cols: std::collections::HashSet<[u32; 3]> =
            pebble.colors.iter().map(|c| [(c[0] * 20.0) as u32, (c[1] * 20.0) as u32, (c[2] * 20.0) as u32]).collect();
        assert!(cols.len() > 5);
    }

    #[test]
    fn ribbons_face_up() {
        for kind in [PathKind::Shell, PathKind::Pebble, PathKind::Flagstone] {
            let a = path_mesh(&path(kind, 12.0, 2.0), &t(), 5);
            let up = a.normals.iter().filter(|n| n[1] > 0.9).count();
            assert!(up > 20, "{kind:?} ribbon should face up");
        }
    }

    #[test]
    fn pavers_budget() {
        for kind in KINDS {
            let m = path_mesh(&path(kind, 30.0, 2.0), &t(), 1);
            assert!(tris(&m) <= 4500, "{kind:?} {} tris", tris(&m));
        }
    }

    #[test]
    fn path_width_is_honoured() {
        for kind in KINDS {
            for w in [1.0_f32, 2.0, 3.0] {
                let m = build_path(&straight(kind, w), 2, &|_| 0.0, &|_| None);
                let (lo, hi) = m.bounds().unwrap();
                let half = lo.z.abs().max(hi.z);
                assert!(half >= w * 0.5 * 0.75 && half <= w * 0.5 + 0.45, "{kind:?} w={w} half={half}");
            }
        }
    }

    #[test]
    fn tracks_follow_the_ground() {
        let slope = |q: Vec2| q.x * 0.3;
        for kind in KINDS {
            let m = build_path(&straight(kind, 1.6), 3, &slope, &|_| None);
            assert!(m.is_valid() && !m.is_empty());
            for p in &m.positions {
                let g = slope(Vec2::new(p[0], p[2]));
                assert!(p[1] > g - 0.5 && p[1] < g + 0.6, "{kind:?} y={} ground={g}", p[1]);
            }
        }
    }

    #[test]
    fn boardwalk_is_raised_on_sleepers() {
        let m = build_path(&straight(PathKind::Boardwalk, 1.6), 3, &|_| 0.0, &|_| None);
        let deck_lo = m.positions.iter().zip(&m.surfaces).filter(|(_, s)| **s == Surface::Boards as u8).map(|(p, _)| p[1]).fold(f32::MAX, f32::min);
        assert!(deck_lo > 0.05, "planks sit above the sand ({deck_lo})");
        let sleeper_lo = m.positions.iter().zip(&m.surfaces).filter(|(_, s)| **s == Surface::Timber as u8).map(|(p, _)| p[1]).fold(f32::MAX, f32::min);
        assert!(sleeper_lo < deck_lo, "sleepers go below the deck");
    }

    #[test]
    fn ford_places_stepping_stones_above_the_tide() {
        for kind in KINDS {
            let p = Path { points: vec![Vec2::new(0.0, 0.0), Vec2::new(14.0, 0.0)], width: 1.5, kind };
            let m = build_path(&p, 3, &|_| 0.0, &|q| (q.x > 5.0 && q.x < 10.0).then_some(0.5));
            assert!(m.is_valid(), "{kind:?}");
            let rock_top = m.positions.iter().zip(&m.surfaces).filter(|(_, s)| **s == Surface::Rock as u8).map(|(p, _)| p[1]).fold(f32::MIN, f32::max);
            assert!((rock_top - 0.82).abs() < 1e-4, "{kind:?} {rock_top}");
            assert!(count(&m, Surface::Rock) > 20);
            // No ribbon, planks or slabs over the water.
            assert!(m.positions.iter().zip(&m.surfaces).all(|(p, s)| *s == Surface::Rock as u8 || !(p[0] > 5.6 && p[0] < 9.4)), "{kind:?}");
        }
    }

    #[test]
    fn steep_path_gets_risers() {
        let p = Path { points: vec![Vec2::new(0.0, 0.0), Vec2::new(10.0, 0.0)], width: 1.5, kind: PathKind::Shell };
        let flat = build_path(&p, 3, &|_| 0.0, &|_| None);
        let steep = build_path(&p, 3, &|q| q.x * 0.8, &|_| None);
        assert!(steep.is_valid() && tris(&steep) > tris(&flat) + 40);
    }

    #[test]
    fn degenerate_paths() {
        let nan = Vec2::new(f32::NAN, 0.0);
        for pts in [vec![], vec![Vec2::ZERO], vec![Vec2::ZERO, Vec2::ZERO], vec![nan, Vec2::ONE], vec![Vec2::ZERO, nan, Vec2::new(5.0, 0.0)]] {
            for width in [0.0, f32::NAN, 100.0] {
                for kind in KINDS {
                    let m = path_mesh(&Path { points: pts.clone(), width, kind }, &t(), 1);
                    assert!(m.is_valid());
                }
            }
        }
    }

    #[test]
    fn prism_normals_point_outward() {
        let mut m = MeshData::default();
        prism(&mut m, Vec3::new(0.0, 1.0, 0.0), Vec3::new(4.0, 2.0, 1.0), 0.5, 8, Vec3::ONE, Surface::Timber);
        assert!(m.is_valid() && !m.is_empty());
        let axis = |p: Vec3| Vec3::new(0.0, 1.0, 0.0) + (Vec3::new(4.0, 1.0, 1.0)) * ((p - Vec3::new(0.0, 1.0, 0.0)).dot(Vec3::new(4.0, 1.0, 1.0)) / 18.0);
        for (p, n) in m.positions.iter().zip(&m.normals) {
            let p = Vec3::from_array(*p);
            let n = Vec3::from_array(*n);
            let rel = p - axis(p);
            // Side faces point away from the axis; caps point along it.
            assert!(rel.dot(n) > -1e-3 || n.dot(Vec3::new(4.0, 1.0, 1.0).normalize()).abs() > 0.99, "{p} {n}");
        }
    }

    #[test]
    fn stairs_all_kinds() {
        for kind in [StairKind::Log, StairKind::Stone, StairKind::RopeLadder] {
            let (m, _) = stairs_mesh(&stairs(kind, 4.0), &t(), 1);
            assert!(!m.is_empty() && m.is_valid(), "{kind:?}");
            let (lo, hi) = m.bounds().unwrap();
            assert!(hi.y >= 3.9 && lo.y > -1.0, "{kind:?} {lo} {hi}");
        }
        assert_eq!(step_count(4.0, 8.0), 16);
        assert_eq!(step_count(4.0, 3.0), 10); // tread floor of 0.3 m wins
        assert_eq!(step_count(0.0, 3.0), 0);
        // Reversed endpoints give the same flight.
        let s = stairs(StairKind::Stone, 2.0);
        let r = Stairs { from: s.to, to: s.from, ..s.clone() };
        assert_eq!(stairs_mesh(&s, &t(), 1).0, stairs_mesh(&r, &t(), 1).0);
    }

    #[test]
    fn stair_kinds_use_their_own_materials() {
        let (stone, _) = stairs_mesh(&stairs(StairKind::Stone, 2.0), &t(), 1);
        assert!(count(&stone, Surface::DryStone) > 0 && count(&stone, Surface::Dirt) == 0);
        let (log, _) = stairs_mesh(&stairs(StairKind::Log, 2.0), &t(), 1);
        assert!(count(&log, Surface::Timber) > 0 && count(&log, Surface::Dirt) > 0, "log risers and earth treads");
        assert_eq!(count(&log, Surface::DryStone), 0);
        let (rope, _) = stairs_mesh(&stairs(StairKind::RopeLadder, 2.0), &t(), 1);
        assert!(count(&rope, Surface::Fiber) > 0 && count(&rope, Surface::Timber) > 0, "ropes and rungs");
        assert_eq!(count(&rope, Surface::Dirt), 0);
        // One log riser and two stakes per step.
        let n = step_count(2.0, 6.08);
        assert!(count(&log, Surface::Timber) >= n * 40);
    }

    #[test]
    fn very_steep_stairs_become_ladder() {
        let s = Stairs { from: Vec3::ZERO, to: Vec3::new(0.5, 4.0, 0.0), width: 1.2, kind: StairKind::Stone, colors: Colors::default() };
        let (m, _) = stairs_mesh(&s, &t(), 1);
        let (l, _) = stairs_mesh(&Stairs { kind: StairKind::RopeLadder, ..s }, &t(), 1);
        assert!(m.is_valid() && !m.is_empty());
        assert_eq!(m, l);
    }

    #[test]
    fn degenerate_stairs() {
        let z = Stairs { from: Vec3::ONE, to: Vec3::ONE, width: 1.0, kind: StairKind::Log, colors: Colors::default() };
        assert!(stairs_mesh(&z, &t(), 1).0.is_empty());
        let nan = Stairs { to: Vec3::new(f32::NAN, 0.0, 0.0), ..z.clone() };
        assert!(stairs_mesh(&nan, &t(), 1).0.is_empty());
        let flat = Stairs { to: Vec3::new(3.0, 1.0, 1.0), ..z.clone() };
        for kind in [StairKind::Log, StairKind::Stone, StairKind::RopeLadder] {
            assert!(stairs_mesh(&Stairs { kind, ..flat.clone() }, &t(), 1).0.is_valid());
        }
        let vertical = Stairs { to: Vec3::new(1.0, 4.0, 1.0), ..z.clone() };
        let (m, _) = stairs_mesh(&vertical, &t(), 1);
        assert!(m.is_valid() && !m.is_empty());
    }

    fn jetty(c: Vec2, h: f32) -> Platform {
        Platform { center: c, half: Vec2::new(2.5, 1.2), angle: 0.4, height: h, colors: Colors::default() }
    }

    #[test]
    fn jetty_over_sea() {
        let c = Vec2::new(-12.0, 6.0);
        let terr = sea_at(c);
        assert!(terr.is_sea(c));
        let seabed = terr.height(c);
        let p = jetty(c, 7.0);
        let (m, mk) = platform_mesh(&p, &terr, 1);
        assert!(!m.is_empty() && m.is_valid());
        assert_eq!((m.clone(), mk.clone()), platform_mesh(&p, &terr, 1));
        // Deck planks, log piles to the seabed, rope coils on mooring posts.
        assert!(count(&m, Surface::Boards) > 0 && count(&m, Surface::Timber) > 0 && count(&m, Surface::Fiber) > 0);
        let (lo, hi) = m.bounds().unwrap();
        assert!(lo.y <= seabed + 0.05, "piles reach the seabed: {} vs {seabed}", lo.y);
        // Mooring posts rise above the deck; four of them are perches.
        let perches: Vec<&Marker> = mk.iter().filter(|k| k.kind == MarkerKind::Perch).collect();
        assert_eq!(perches.len(), 4);
        assert!(perches.iter().all(|k| k.pos.y > hi.y - 0.2));
        // 1-2 canoes on the sea beside the jetty, hull parallel to an edge.
        let boats: Vec<&Marker> = mk.iter().filter(|k| k.kind == MarkerKind::Boat).collect();
        assert!((1..=2).contains(&boats.len()));
        let (ax, az) = (Vec3::new(0.4_f32.cos(), 0.0, 0.4_f32.sin()), Vec3::new(-0.4_f32.sin(), 0.0, 0.4_f32.cos()));
        for b in &boats {
            assert!(terr.is_sea(Vec2::new(b.pos.x, b.pos.z)) && (b.pos.y - SEA_LEVEL).abs() < 1e-6);
            assert!((b.dir.length() - 1.0).abs() < 1e-4 && b.dir.y == 0.0);
            assert!(b.dir.dot(ax).abs() > 0.999 || b.dir.dot(az).abs() > 0.999);
            assert!((Vec2::new(b.pos.x, b.pos.z) - c).length() < 4.0);
        }
        // A ladder reaches below the waterline.
        let rung_low = m.positions.iter().zip(&m.surfaces).filter(|(p, s)| **s == Surface::Timber as u8 && p[1] < SEA_LEVEL - 0.1 && p[1] > seabed + 1.0).count();
        assert!(rung_low > 0, "ladder rungs under water");
    }

    #[test]
    fn jetty_deck_clears_the_water_and_land_stage_is_plain() {
        let c = Vec2::new(-12.0, 6.0);
        let terr = sea_at(c);
        // Deck height is relative to the highest ground or water; a tiny value must still clear
        // the tide.
        let (m, _) = platform_mesh(&jetty(c, 0.2), &terr, 1);
        let deck_top = m.positions.iter().zip(&m.surfaces).filter(|(_, s)| **s == Surface::Boards as u8).map(|(p, _)| p[1]).fold(f32::MIN, f32::max);
        assert!(deck_top >= SEA_LEVEL + 0.45 - 1e-4, "{deck_top}");
        // On land: raised stage, no canoes, no rope coils.
        let land = Vec2::new(12.0, -8.0);
        let lt = Terrain::build(4, &[], &[]);
        assert!(!lt.is_sea(land));
        for h in [0.5, 1.0, 2.5] {
            let (m, mk) = platform_mesh(&jetty(land, h), &lt, 1);
            assert!(!m.is_empty() && m.is_valid());
            assert!(mk.iter().all(|k| k.kind == MarkerKind::Perch) && mk.len() == 4, "h={h}");
            assert_eq!(count(&m, Surface::Fiber), 0);
            let (_, hi) = m.bounds().unwrap();
            assert!(hi.y >= lt.height(land) - 1.0 + h - 1e-4 || hi.y >= h - 1e-4);
        }
    }

    /// A jetty from the beach out to sea: its height is measured from the highest ground or
    /// water under it (the deck clears the sand and rises with the setting), and its canoes
    /// moor only where they stay afloat at low tide.
    #[test]
    fn shore_jetty_rises_with_height_and_moors_in_deep_water() {
        use crate::model::TIDE_RANGE;
        use crate::terrain::BOAT_DEPTH;
        let terr = Terrain::build(7, &[], &[]);
        let cove = terr.plan().cove;
        let shore = (0..400).map(|i| i as f32 * 0.1).find(|r| terr.is_sea(cove * *r)).unwrap();
        let deck = |m: &MeshData| m.positions.iter().zip(&m.surfaces).filter(|(_, s)| **s == Surface::Boards as u8).map(|(p, _)| p[1]).fold(f32::MIN, f32::max);
        let mut moored = 0;
        for hx in [5.0, 6.0, 8.0] {
            let p = Platform { center: cove * shore, half: Vec2::new(hx, 1.0), angle: cove.to_angle(), height: 1.5, colors: Colors::default() };
            let land_end = terr.height(cove * (shore - hx));
            assert!(land_end > SEA_LEVEL);
            let (m, mk) = platform_mesh(&p, &terr, 1);
            let top = deck(&m);
            assert!(top >= land_end + 1.5 - 1e-3, "deck {top} clears the beach at {land_end}");
            let (raised, _) = platform_mesh(&Platform { height: 2.5, ..p.clone() }, &terr, 1);
            assert!((deck(&raised) - top - 1.0).abs() < 1e-3, "raising lifts the deck");
            for b in mk.iter().filter(|k| k.kind == MarkerKind::Boat) {
                let q = Vec2::new(b.pos.x, b.pos.z);
                assert!(SEA_LEVEL - TIDE_RANGE - terr.height(q) >= BOAT_DEPTH - 1e-4, "afloat at low tide: bed {}", terr.height(q));
                moored += 1;
            }
        }
        assert!(moored > 0);
        // Over shallows that dry out at low tide no canoe moors at all.
        let flat = Terrain::build(7, &[crate::model::TerrainDab { center: cove * (shore + 6.0), radius: 40.0, op: crate::model::TerrainOp::Flatten { height: -0.4, strength: 1.0 } }], &[]);
        let c = cove * (shore + 6.0);
        assert!(flat.is_sea(c) && flat.boat_water(c).is_none());
        let (_, mk) = platform_mesh(&jetty(c, 1.0), &flat, 1);
        assert!(!mk.iter().any(|k| k.kind == MarkerKind::Boat));
    }

    #[test]
    fn degenerate_platforms() {
        let bad = Platform { center: Vec2::ZERO, half: Vec2::new(f32::NAN, 1.0), angle: 0.0, height: 1.0, colors: Colors::default() };
        assert!(platform_mesh(&bad, &t(), 1).0.is_empty());
        let tiny = Platform { half: Vec2::ZERO, height: -3.0, ..bad.clone() };
        assert!(platform_mesh(&tiny, &t(), 1).0.is_valid());
        let huge = Platform { half: Vec2::splat(100.0), height: 100.0, ..bad.clone() };
        assert!(platform_mesh(&huge, &t(), 1).0.is_valid());
    }

    #[test]
    fn bridges_all_kinds() {
        let mut kinds = vec![BridgeKind::Causeway, BridgeKind::Log];
        kinds.extend((0..4).map(|r| BridgeKind::Rope { railing: r }));
        kinds.push(BridgeKind::Rope { railing: 9 });
        for kind in kinds {
            for hanging in [Hanging::None, Hanging::Lanterns, Hanging::Pennants] {
                let (m, mk) = bridge_mesh(&bridge(kind, hanging), &t(), 2);
                assert!(!m.is_empty() && m.is_valid(), "{kind:?} {hanging:?}");
                assert!(tris(&m) < 12000, "{kind:?} {} tris", tris(&m));
                assert!(mk.iter().filter(|k| k.kind == MarkerKind::Perch).count() >= 2);
                let lanterns = mk.iter().filter(|k| k.kind == MarkerKind::Lantern).count();
                let flags = mk.iter().filter(|k| k.kind == MarkerKind::Flag).count();
                if matches!(kind, BridgeKind::Rope { .. }) && hanging == Hanging::Lanterns {
                    assert!(lanterns >= 4, "{kind:?}");
                } else {
                    assert_eq!(lanterns, 0);
                }
                assert_eq!(flags > 0, matches!(kind, BridgeKind::Rope { .. }) && hanging == Hanging::Pennants);
                assert_eq!((m.clone(), mk.clone()), bridge_mesh(&bridge(kind, hanging), &t(), 2));
            }
        }
    }

    #[test]
    fn rope_railings_are_distinct_and_deck_sags() {
        let meshes: Vec<MeshData> = (0..4).map(|r| bridge_mesh(&bridge(BridgeKind::Rope { railing: r }, Hanging::None), &t(), 2).0).collect();
        for i in 0..4 {
            for j in i + 1..4 {
                assert_ne!(meshes[i], meshes[j], "railing {i} vs {j}");
            }
        }
        // Design 3 hangs painted floats (Carving); the others carry none; 2 has plank balusters.
        assert!(count(&meshes[3], Surface::Carving) > count(&meshes[0], Surface::Carving));
        assert!(count(&meshes[2], Surface::Boards) > count(&meshes[0], Surface::Boards));
        let b = bridge(BridgeKind::Rope { railing: 0 }, Hanging::None);
        let (m, _) = bridge_mesh(&b, &t(), 2);
        let deck_lo = m.positions.iter().zip(&m.surfaces).filter(|(_, s)| **s == Surface::Boards as u8).map(|(p, _)| p[1]).fold(f32::MAX, f32::min);
        assert!(deck_lo < b.a.y - 0.4, "deck sags between the ends ({deck_lo})");
    }

    #[test]
    fn pennants_are_cloth_in_pigments() {
        let (m, mk) = bridge_mesh(&bridge(BridgeKind::Rope { railing: 0 }, Hanging::Pennants), &t(), 2);
        let (plain, _) = bridge_mesh(&bridge(BridgeKind::Rope { railing: 0 }, Hanging::None), &t(), 2);
        assert!(count(&m, Surface::Hide) > 0 && count(&plain, Surface::Hide) == 0);
        assert_eq!(mk.iter().filter(|k| k.kind == MarkerKind::Flag).count(), 7);
    }

    #[test]
    fn log_bridge_is_a_log_on_trestles() {
        let high = Bridge { a: Vec3::new(0.0, 4.0, 0.0), b: Vec3::new(14.0, 4.0, 0.0), ..bridge(BridgeKind::Log, Hanging::None) };
        let (m, mk) = bridge_mesh(&high, &t(), 1);
        assert!(m.is_valid() && count(&m, Surface::Timber) > 200);
        let (lo, hi) = m.bounds().unwrap();
        assert!(hi.y > 4.0 && lo.y < 3.0, "trestle legs run down to the ground ({lo}, {hi})");
        assert!(mk.iter().filter(|k| k.kind == MarkerKind::Perch).count() == 4);
        // A wide bridge lays more logs side by side.
        let wide = Bridge { width: 5.0, ..high.clone() };
        let (w, _) = bridge_mesh(&wide, &t(), 1);
        assert!(tris(&w) > tris(&bridge_mesh(&high, &t(), 1).0));
    }

    #[test]
    fn causeway_has_culvert_arches() {
        let b = Bridge { a: Vec3::new(0.0, 3.0, 0.0), b: Vec3::new(30.0, 3.0, 0.0), ..bridge(BridgeKind::Causeway, Hanging::None) };
        let (m, _) = bridge_mesh(&b, &t(), 1);
        assert!(m.is_valid() && tris(&m) > 600 && count(&m, Surface::DryStone) == m.surfaces.len());
        // Arches open the underside: soffit faces point down.
        assert!(m.normals.iter().any(|n| n[1] < -0.9));
    }

    #[test]
    fn degenerate_bridges() {
        for kind in [BridgeKind::Causeway, BridgeKind::Log, BridgeKind::Rope { railing: 1 }] {
            let mut b = bridge(kind, Hanging::Lanterns);
            b.b = b.a;
            assert!(bridge_mesh(&b, &t(), 1).0.is_empty());
            b.b = b.a + Vec3::new(0.7, 0.0, 0.0);
            assert!(bridge_mesh(&b, &t(), 1).0.is_valid());
            b.b = Vec3::new(f32::NAN, 0.0, 0.0);
            assert!(bridge_mesh(&b, &t(), 1).0.is_empty());
            b = bridge(kind, Hanging::Pennants);
            b.width = 0.0;
            assert!(bridge_mesh(&b, &t(), 1).0.is_valid());
            b.width = f32::NAN;
            assert!(bridge_mesh(&b, &t(), 1).0.is_valid());
        }
    }
}
