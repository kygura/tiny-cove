//! Paths, stairs, platforms and bridges.
//!
//! All generators are deterministic (randomness comes from `Rng::at(seed, world x, z)`),
//! NaN-free, and return empty meshes for degenerate input.

use crate::geom::{point_at, polyline_length};
use crate::mesh::{Marker, MarkerKind, MeshData, Rng, Surface};
use crate::model::{Bridge, BridgeKind, Hanging, Path, PathKind, Platform, StairKind, Stairs};
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
fn rope_color() -> Vec3 {
    palette::hay() * 0.75
}
/// Thin square-section line centred on `a`→`b`.
fn thin(m: &mut MeshData, a: Vec3, b: Vec3, th: f32, c: Vec3, s: Surface) {
    let o = Vec3::Y * (th * 0.5);
    m.beam(a - o, b - o, th, th, c, s, 1.0);
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
// Paths
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

/// Path ribbon draped on the terrain (slightly above it), with individual stones / setts /
/// flagstones as geometry, stepping stones (a ford) where it crosses water.
pub fn path_mesh(path: &Path, terrain: &Terrain, seed: u64) -> MeshData {
    build_path(path, seed, &|p| terrain.height(p), &|p| terrain.water_level(p))
}

/// Stepping stones along a wet stretch: one row every ~0.7 m, flat tops 0.1 m above water.
#[allow(clippy::too_many_arguments)]
fn ford_stones(m: &mut MeshData, c: Vec2, s: Vec2, w: f32, level: f32, bed: f32, seed: u64) {
    let mut r = rng(seed, 11, c);
    let offs: &[f32] = if w > 1.6 { &[-0.25, 0.25] } else { &[0.0] };
    for &o in offs {
        let p = c + s * (o * w + (r.f() - 0.5) * 0.2) + Vec2::new(r.f() - 0.5, r.f() - 0.5) * 0.15;
        let rad = 0.22 + 0.12 * r.f();
        let base = bed.min(level) - 0.05;
        let top = level + 0.1;
        let col = tint(palette::rock(), &mut r, 0.2);
        m.cylinder(v3(p, base), rad * 1.15, rad * 0.9, top - base, 6, true, false, col, Surface::Rock, 1.0);
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
    let neat = path.kind == PathKind::Poured;
    let (surf, base_col) = match path.kind {
        PathKind::Gravel => (Surface::Dirt, palette::dirt()),
        PathKind::Pavers => (Surface::Pavers, palette::stone()),
        PathKind::Slabs | PathKind::Stepping => (Surface::Slabs, palette::stone() * 0.95),
        PathKind::Poured => (Surface::Poured, palette::rock()),
    };

    // Ribbon columns (left edge, middle, right edge) per sample, edges jittered.
    let cols: Vec<[Vec3; 3]> = (0..=n)
        .map(|i| {
            let (c, s) = (cs[i], side(ts[i]));
            let mut r = rng(seed, 1, c);
            let j = |r: &mut Rng, amt: f32| if neat { 0.0 } else { (r.f() - 0.5) * amt };
            let (ja, jb, jm) = (j(&mut r, 0.3), j(&mut r, 0.3), j(&mut r, 0.1));
            let pa = c - s * (hw + ja);
            let pm = c + s * jm;
            let pb = c + s * (hw + jb);
            [pa, pm, pb].map(|p| v3(p, h(p) + LIFT))
        })
        .collect();

    let mut since = 10.0_f32;
    for i in 0..=n {
        let c = cs[i];
        let dry_next = i < n && wets[i].is_none() && wets[i + 1].is_none();
        if dry_next {
            let mut r = rng(seed, 2, c);
            let col = tint(base_col, &mut r, 0.12);
            for k in 0..2 {
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
        let x_axis = flat(ts[i]);
        match path.kind {
            PathKind::Stepping => {}
            PathKind::Gravel => {
                if r.f() < 0.3 {
                    let p = c + s * (r.range(-0.8, 0.8) * hw);
                    let sz = r.range(0.04, 0.09);
                    let y = h(p) + LIFT;
                    m.aabb(v3(p, y), v3(p, y) + Vec3::new(sz, sz * 0.7, sz), tint(palette::rock(), &mut r, 0.3), Surface::Rock, 1.0);
                }
            }
            PathKind::Pavers => {
                if i < n && wets[i + 1].is_none() {
                    for f in [0.0, 0.5] {
                        let cc = cs[i].lerp(cs[i + 1], f);
                        for sg in [-1.0, 1.0] {
                            let mut rr = rng(seed, 4, cc + s * sg);
                            let p = cc + s * (sg * (hw - 0.06 + (rr.f() - 0.5) * 0.06));
                            let hy = 0.04 + rr.f() * 0.03;
                            let col = tint(palette::stone(), &mut rr, 0.25);
                            m.oriented_box(v3(p, h(p) + LIFT + hy), Vec3::new(0.14, hy, 0.1), x_axis, col, Surface::Pavers, 1.0);
                        }
                    }
                }
            }
            PathKind::Slabs => {
                if i % 2 == 0 {
                    for o in [-0.55_f32, 0.0, 0.55] {
                        let mut rr = rng(seed, 5, c + s * o);
                        let p = c + s * (o * hw + (rr.f() - 0.5) * 0.2 * hw);
                        let ang = (rr.f() - 0.5) * 0.7;
                        let ax = Vec2::from_angle(ang).rotate(ts[i]);
                        let half = Vec3::new(rr.range(0.25, 0.42).min(hw * 0.5 + 0.1), 0.03, rr.range(0.2, 0.35).min(hw * 0.5 + 0.1));
                        let col = tint(palette::stone(), &mut rr, 0.3);
                        m.oriented_box(v3(p, h(p) + LIFT + 0.02), half, flat(ax), col, Surface::Slabs, 1.0);
                    }
                }
            }
            PathKind::Poured => {
                if i < n && wets[i + 1].is_none() {
                    for sg in [-1.0, 1.0] {
                        let e = |q: Vec2, t: Vec2| {
                            let p = q + side(t) * (sg * (hw - 0.07));
                            v3(p, h(p) + LIFT - 0.01)
                        };
                        m.beam(e(cs[i], ts[i]), e(cs[i + 1], ts[i + 1]), 0.14, 0.08, palette::foundation() * 1.1, Surface::Poured, 1.0);
                    }
                }
            }
        }
        // Steep slope: little risers across the ribbon.
        if i > 0 && i % 2 == 0 && wets[i - 1].is_none() {
            let step = cs[i].distance(cs[i - 1]).max(1e-3);
            if ((h(cs[i]) - h(cs[i - 1])) / step).abs() > 0.577 {
                m.oriented_box(
                    v3(c, h(c) + LIFT + 0.05),
                    Vec3::new(hw, 0.05, 0.06),
                    flat(s),
                    base_col * 0.85,
                    surf,
                    1.0,
                );
            }
        }
    }
    m
}

// ---------------------------------------------------------------------------------------------
// Stairs
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

pub fn stairs_mesh(stairs: &Stairs, terrain: &Terrain, seed: u64) -> (MeshData, Vec<Marker>) {
    let mut m = MeshData::default();
    let mut mk = Vec::new();
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

    if stairs.kind == StairKind::Ladder || run < 0.05 {
        let w = fin(stairs.width, 0.3, 0.6, 0.6).min(0.6);
        let off = side * (w * 0.5);
        for sg in [-1.0, 1.0] {
            m.beam(from + off * sg, to + off * sg, 0.07, 0.07, trim, Surface::Steel, 1.0);
        }
        let len = from.distance(to);
        for k in 1..((len / 0.3).floor() as usize + 1) {
            let p = from.lerp(to, k as f32 * 0.3 / len);
            m.beam(p - off, p + off, 0.05, 0.05, palette::wood(), Surface::Steel, 1.0);
        }
        return (m, mk);
    }

    let w = fin(stairs.width, 0.5, 4.0, 1.2);
    let n = if rise < 0.12 { 1 } else { step_count(rise, run) };
    let (tr, rs) = (run / n as f32, rise / n as f32);
    let at = |s: f32| from + dir * s;
    let x_axis = side;
    for i in 0..n {
        let top = from.y + (i + 1) as f32 * rs;
        let c = at((i as f32 + 0.5) * tr);
        let mut r = rng(seed, 21, Vec2::new(c.x, c.z));
        match stairs.kind {
            StairKind::Concrete => {
                let g = [0.0, 1.0, -1.0]
                    .iter()
                    .map(|o| {
                        let p = c + side * (o * w * 0.5);
                        terrain.height(Vec2::new(p.x, p.z))
                    })
                    .fold(f32::MAX, f32::min);
                let base = (g - 0.05).min(top - 0.2);
                m.oriented_box(
                    Vec3::new(c.x, (top + base) * 0.5, c.z),
                    Vec3::new(w * 0.5, (top - base) * 0.5, tr * 0.5),
                    x_axis,
                    tint(palette::stone(), &mut r, 0.15),
                    Surface::Concrete,
                    1.0,
                );
            }
            _ => {
                m.oriented_box(
                    Vec3::new(c.x, top - 0.03, c.z),
                    Vec3::new(w * 0.5, 0.03, tr * 0.5 + 0.02),
                    x_axis,
                    tint(palette::wood(), &mut r, 0.15),
                    Surface::Cladding,
                    1.0,
                );
            }
        }
    }
    if stairs.kind != StairKind::Steel {
        return (m, mk);
    }
    // Stringers.
    for sg in [-1.0, 1.0] {
        let o = side * (sg * (w * 0.5 - 0.04));
        m.beam(at(0.0) + o - Vec3::Y * 0.15, at(run) + Vec3::new(0.0, rise - 0.15, 0.0) + o, 0.08, 0.3, trim, Surface::Steel, 1.0);
    }
    // Railings.
    let lamp = rise > 3.0;
    let rail_y = |s: f32| from.y + rise * s / run + rs * 0.5 + 0.9;
    let np = ((run / 1.2).ceil() as usize + 1).max(2);
    for sg in [-1.0_f32, 1.0] {
        let o = side * (sg * (w * 0.5 + 0.04));
        for k in 0..np {
            let s = run * k as f32 / (np - 1) as f32;
            let idx = ((s / tr).floor() as usize).min(n - 1);
            let base = from.y + (idx + 1) as f32 * rs - 0.02;
            let newel = lamp && k == np - 1;
            let top = rail_y(s) + if newel { 0.12 } else { 0.0 };
            let p = at(s) + o;
            m.oriented_box(Vec3::new(p.x, (base + top) * 0.5, p.z), Vec3::new(0.04, (top - base) * 0.5, 0.04), x_axis, trim, Surface::Steel, 1.0);
            if newel && sg > 0.0 {
                let c = lantern(&mut m, Vec3::new(p.x, top + 0.24, p.z), 0.0);
                mk.push(Marker { kind: MarkerKind::Lantern, pos: c, dir: Vec3::Y });
            }
        }
        for drop in [0.03, 0.5] {
            let a = at(0.0) + o + Vec3::Y * (rail_y(0.0) - drop - 0.03);
            let b = at(run) + o + Vec3::Y * (rail_y(run) - drop - 0.03);
            m.beam(a, b, 0.07, 0.06, trim, Surface::Steel, 1.0);
        }
    }
    (m, mk)
}

// ---------------------------------------------------------------------------------------------
// Platform
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
    let ax3 = flat(ax);
    let world = |l: Vec2| p.center + ax * l.x + az * l.y;
    let mut gmin = f32::MAX;
    for i in 0..=4 {
        for j in 0..=4 {
            let l = Vec2::new(-hx + 2.0 * hx * i as f32 / 4.0, -hz + 2.0 * hz * j as f32 / 4.0);
            gmin = gmin.min(terrain.height(world(l)));
        }
    }
    let top = gmin + height;
    let under = top - 0.1;
    let trim = palette::trim(p.colors);
    let mut r = rng(seed, 31, p.center);
    m.oriented_box(v3(p.center, top - 0.05), Vec3::new(hx, 0.05, hz), ax3, tint(palette::wood(), &mut r, 0.1), Surface::Cladding, 1.0);
    // Joists along local x, rim beams along local z.
    let nj = ((2.0 * hz / 1.0).ceil() as usize + 1).max(2);
    for k in 0..nj {
        let z = -hz + 2.0 * hz * k as f32 / (nj - 1) as f32;
        m.beam(v3(world(Vec2::new(-hx, z)), under - 0.12), v3(world(Vec2::new(hx, z)), under - 0.12), 0.1, 0.12, trim, Surface::Steel, 1.0);
    }
    for sx in [-1.0, 1.0] {
        let x = sx * (hx - 0.06);
        m.beam(v3(world(Vec2::new(x, -hz)), under - 0.16), v3(world(Vec2::new(x, hz)), under - 0.16), 0.12, 0.16, trim, Surface::Steel, 1.0);
    }
    // Posts down to the terrain at each post.
    for l in perimeter((hx - 0.15).max(0.1), (hz - 0.15).max(0.1), 2.0) {
        let q = world(l);
        let base = terrain.height(q) - 0.15;
        if under - base > 0.1 {
            m.oriented_box(v3(q, (base + under) * 0.5), Vec3::new(0.09, (under - base) * 0.5, 0.09), ax3, trim, Surface::Steel, 1.0);
        }
    }
    let railing = height > 1.2;
    let mut perch_y = top;
    if railing {
        perch_y = top + 0.9;
        let per = perimeter(hx - 0.06, hz - 0.06, 1.5);
        for (i, l) in per.iter().enumerate() {
            let q = world(*l);
            m.oriented_box(v3(q, top + 0.45), Vec3::new(0.04, 0.45, 0.04), ax3, trim, Surface::Steel, 1.0);
            let q2 = world(per[(i + 1) % per.len()]);
            for hgt in [0.9, 0.45] {
                m.beam(v3(q, top + hgt - 0.05), v3(q2, top + hgt - 0.05), 0.06, 0.05, trim, Surface::Steel, 1.0);
            }
        }
    }
    for (sx, sz) in [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)] {
        mk.push(Marker { kind: MarkerKind::Perch, pos: v3(world(Vec2::new(sx * (hx - 0.06), sz * (hz - 0.06))), perch_y), dir: Vec3::Y });
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

/// Four posts, one per deck corner at each end, with Perch markers on top.
#[allow(clippy::too_many_arguments)]
fn end_posts(m: &mut MeshData, mk: &mut Vec<Marker>, f: &Fr, lat: f32, out: f32, top_h: f32, half: f32, surf: Surface, col: Vec3, terrain: &Terrain) {
    for (t, o) in [(0.0, -1.0), (1.0, 1.0)] {
        for sg in [-1.0, 1.0] {
            let b = f.base(t);
            let p = b + f.dir * (o * out) + f.side * (sg * lat);
            let bot = (terrain.height(f.xz(p)) - 0.2).min(b.y - 0.3);
            let top = b.y + top_h;
            m.oriented_box(Vec3::new(p.x, (bot + top) * 0.5, p.z), Vec3::new(half, (top - bot) * 0.5, half), f.side, col, surf, 1.0);
            mk.push(Marker { kind: MarkerKind::Perch, pos: Vec3::new(p.x, top, p.z), dir: Vec3::Y });
        }
    }
}

pub fn bridge_mesh(bridge: &Bridge, terrain: &Terrain, seed: u64) -> (MeshData, Vec<Marker>) {
    let mut m = MeshData::default();
    let mut mk = Vec::new();
    let Some(f) = Fr::new(bridge) else { return (m, mk) };
    match bridge.kind {
        BridgeKind::Cable { railing } => rope_bridge(&mut m, &mut mk, &f, bridge, railing.min(3), terrain, seed),
        BridgeKind::Arch => stone_bridge(&mut m, &mut mk, &f, terrain, seed),
        BridgeKind::Girder => wood_bridge(&mut m, &mut mk, &f, bridge, terrain, seed),
    }
    (m, mk)
}

fn rope_bridge(m: &mut MeshData, mk: &mut Vec<Marker>, f: &Fr, br: &Bridge, railing: u8, terrain: &Terrain, seed: u64) {
    let trim = palette::trim(br.colors);
    let rc = rope_color();
    let sag = (f.len * 0.06).clamp(0.25, 3.0);
    let bell = |t: f32| 4.0 * t * (1.0 - t);
    let deck = |t: f32| f.base(t) - Vec3::Y * (sag * bell(t));
    let hand = |t: f32| f.base(t) + Vec3::Y * (1.1 - sag * 0.7 * bell(t));
    let lat = f.w * 0.5 + 0.02;
    let nseg = (f.len.ceil() as usize).max(2);
    let tt = |k: usize| k as f32 / nseg as f32;

    // Planks across, with gaps.
    let pitch = 0.33;
    for k in 0..(f.len / pitch).floor() as usize {
        let (s0, s1) = (k as f32 * pitch, k as f32 * pitch + 0.25);
        let mut r = rng(seed, 41, f.xz(f.base(s0 / f.len)));
        m.beam(deck(s0 / f.len), deck((s1 / f.len).min(1.0)), f.w, 0.05, tint(palette::wood(), &mut r, 0.2), Surface::Cladding, 1.0);
    }
    for sg in [-1.0_f32, 1.0] {
        let o = f.side * (sg * lat);
        for k in 0..nseg {
            // Carrying rope under the deck edge, and the hand rope.
            let so = f.side * (sg * (f.w * 0.5 - 0.05));
            thin(m, deck(tt(k)) + so - Vec3::Y * 0.05, deck(tt(k + 1)) + so - Vec3::Y * 0.05, 0.07, rc, Surface::Fiber);
            thin(m, hand(tt(k)) + o, hand(tt(k + 1)) + o, 0.06, rc, Surface::Fiber);
            let (e0, e1) = (deck(tt(k)) + o + Vec3::Y * 0.05, deck(tt(k + 1)) + o + Vec3::Y * 0.05);
            let (h0, h1) = (hand(tt(k)) + o, hand(tt(k + 1)) + o);
            match railing {
                1 => {
                    let hang = hand(tt(k)) + o;
                    let base = e0;
                    m.oriented_box((hang + base) * 0.5, Vec3::new(0.015, (hang.y - base.y) * 0.5, 0.015), f.side, rc, Surface::Fiber, 1.0);
                }
                2 => {
                    thin(m, e0, h1, 0.025, rc, Surface::Fiber);
                    thin(m, h0, e1, 0.025, rc, Surface::Fiber);
                }
                3 => {
                    for fr in [0.3, 0.6, 0.9] {
                        let (a, b) = (e0.lerp(h0, fr), e1.lerp(h1, fr));
                        m.beam(a, b, 0.03, 0.12, trim, Surface::Cladding, 1.0);
                    }
                    if k % 2 == 0 {
                        m.oriented_box((e0 + h0) * 0.5, Vec3::new(0.025, (h0.y - e0.y) * 0.5, 0.025), f.side, trim, Surface::Steel, 1.0);
                    }
                }
                _ => {}
            }
        }
    }
    end_posts(m, mk, f, lat + 0.03, 0.2, 1.3, 0.1, Surface::Steel, trim, terrain);

    match br.hanging {
        Hanging::Lamps => {
            let n = ((f.len / 3.0).floor() as usize).max(1);
            for k in 0..n {
                let t = (k as f32 + 0.5) / n as f32;
                for sg in [-1.0_f32, 1.0] {
                    let c = lantern(m, hand(t) + f.side * (sg * lat) - Vec3::Y * 0.04, 0.15);
                    mk.push(Marker { kind: MarkerKind::Lantern, pos: c, dir: Vec3::Y });
                }
            }
        }
        Hanging::Vines => {
            let n = ((f.len / 2.0).floor() as usize).max(2);
            for k in 0..n {
                let t = (k as f32 + 0.5) / n as f32;
                let sg = if k % 2 == 0 { 1.0 } else { -1.0 };
                let p = hand(t) + f.side * (sg * lat);
                let q = hand((t + 0.6 / f.len).min(1.0)) + f.side * (sg * lat);
                let tip = p.lerp(q, 0.5) - Vec3::Y * 0.45;
                let col = palette::flower(k as u32);
                m.tri(p, tip, q, col, Surface::Fiber, 1.0);
                m.tri(p, q, tip, col, Surface::Fiber, 1.0);
                mk.push(Marker { kind: MarkerKind::Flag, pos: p, dir: Vec3::Y });
            }
        }
        Hanging::None => {}
    }
}

fn stone_bridge(m: &mut MeshData, mk: &mut Vec<Marker>, f: &Fr, terrain: &Terrain, seed: u64) {
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
        let col = tint(palette::stone(), &mut r, 0.15);
        m.beam(p0 - Vec3::Y * TH, p1 - Vec3::Y * TH, f.w, TH, col, Surface::Concrete, 1.0);
        for sg in [-1.0_f32, 1.0] {
            let o = f.side * (sg * f.w * 0.5);
            let pc = f.side * (sg * (f.w * 0.5 - 0.12));
            m.beam(p0 + pc, p1 + pc, 0.24, 0.5, col * 1.05, Surface::Concrete, 1.0);
            let (t0, t1) = (p0.y - TH, p1.y - TH);
            let (b0, b1) = (bottom(j, sg).min(t0 - 0.01), bottom(j + 1, sg).min(t1 - 0.01));
            if t0 - b0 > 0.02 || t1 - b1 > 0.02 {
                let q = |p: Vec3, y: f32| p + o + Vec3::Y * (y - p.y);
                let (a, b, c, d) = (q(p0, b0), q(p1, b1), q(p1, t1), q(p0, t0));
                if sg > 0.0 { m.quad(a, b, c, d, col * 0.95, Surface::Concrete, 1.0) } else { m.quad(b, a, d, c, col * 0.95, Surface::Concrete, 1.0) }
            }
        }
        if in_arch(j) {
            // Soffit (underside of the arch), facing down.
            let (c0, c1) = (bottom(j, 0.0), bottom(j + 1, 0.0));
            if c0 < p0.y - TH && c1 < p1.y - TH {
                let q = |p: Vec3, y: f32, sg: f32| p + f.side * (sg * f.w * 0.5) + Vec3::Y * (y - p.y);
                m.quad(q(p0, c0, -1.0), q(p1, c1, -1.0), q(p1, c1, 1.0), q(p0, c0, 1.0), col * 0.8, Surface::Concrete, 1.0);
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
                if facing > 0.0 { m.quad(a, b, c, d, palette::stone() * 0.85, Surface::Concrete, 1.0) } else { m.quad(a, d, c, b, palette::stone() * 0.85, Surface::Concrete, 1.0) }
            }
        }
    }
    end_posts(m, mk, f, f.w * 0.5 - 0.12, 0.0, 0.9, 0.2, Surface::Concrete, palette::stone() * 1.1, terrain);
}

fn wood_bridge(m: &mut MeshData, mk: &mut Vec<Marker>, f: &Fr, br: &Bridge, terrain: &Terrain, seed: u64) {
    let trim = palette::trim(br.colors);
    let nb = ((f.len / 3.0).ceil() as usize).max(1);
    let at = |t: f32| f.base(t);
    let lat = f.w * 0.5 - 0.1;
    for k in 0..nb {
        let (t0, t1) = (k as f32 / nb as f32, (k + 1) as f32 / nb as f32);
        let mut r = rng(seed, 61, f.xz(at(t0)));
        m.beam(at(t0) - Vec3::Y * 0.08, at(t1) - Vec3::Y * 0.08, f.w, 0.08, tint(palette::wood(), &mut r, 0.15), Surface::Cladding, 1.0);
        for sg in [-1.0_f32, 1.0] {
            let o = f.side * (sg * f.w * 0.3);
            m.beam(at(t0) + o - Vec3::Y * 0.26, at(t1) + o - Vec3::Y * 0.26, 0.15, 0.18, trim, Surface::Steel, 1.0);
        }
    }
    // Trestle bents.
    for k in 0..=nb {
        let c = at(k as f32 / nb as f32);
        let under = c.y - 0.26;
        let mut feet = [Vec3::ZERO; 2];
        for (i, sg) in [-1.0_f32, 1.0].iter().enumerate() {
            let p = c + f.side * (sg * lat);
            let base = terrain.height(f.xz(p)) - 0.15;
            feet[i] = Vec3::new(p.x, base, p.z);
            if under - base > 0.1 {
                m.oriented_box(Vec3::new(p.x, (base + under) * 0.5, p.z), Vec3::new(0.1, (under - base) * 0.5, 0.1), f.side, trim, Surface::Steel, 1.0);
            }
        }
        m.beam(Vec3::new(feet[0].x, under - 0.1, feet[0].z), Vec3::new(feet[1].x, under - 0.1, feet[1].z), 0.14, 0.14, trim, Surface::Steel, 1.0);
        let low = feet[0].y.max(feet[1].y);
        if under - low > 0.8 {
            m.beam(Vec3::new(feet[0].x, low + 0.3, feet[0].z), Vec3::new(feet[1].x, under - 0.3, feet[1].z), 0.07, 0.07, trim, Surface::Steel, 1.0);
        }
    }
    // Railings: posts every ~1.5 m with two rails.
    let np = ((f.len / 1.5).ceil() as usize).max(1);
    for sg in [-1.0_f32, 1.0] {
        let o = f.side * (sg * lat);
        for k in 0..=np {
            let p = at(k as f32 / np as f32) + o;
            m.oriented_box(p + Vec3::Y * 0.5, Vec3::new(0.04, 0.5, 0.04), f.side, trim, Surface::Steel, 1.0);
        }
        for hgt in [0.95, 0.5] {
            m.beam(at(0.0) + o + Vec3::Y * (hgt - 0.05), at(1.0) + o + Vec3::Y * (hgt - 0.05), 0.07, 0.06, trim, Surface::Steel, 1.0);
        }
    }
    end_posts(m, mk, f, lat, 0.12, 1.15, 0.09, Surface::Steel, trim, terrain);
}

// ---------------------------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Colors;

    fn path(kind: PathKind, len: f32, width: f32) -> Path {
        Path { points: vec![Vec2::new(0.0, 0.0), Vec2::new(len * 0.5, 3.0), Vec2::new(len, 0.0)], width, kind }
    }
    fn tris(m: &MeshData) -> usize {
        m.indices.len() / 3
    }
    fn t() -> Terrain {
        Terrain::build(1, &[], &[])
    }
    fn stairs(kind: StairKind, rise: f32) -> Stairs {
        Stairs { from: Vec3::new(0.0, 0.0, 0.0), to: Vec3::new(6.0, rise, 1.0), width: 1.2, kind, colors: Colors::default() }
    }
    fn bridge(kind: BridgeKind, hanging: Hanging) -> Bridge {
        Bridge { a: Vec3::new(0.0, 1.0, 0.0), b: Vec3::new(14.0, 1.5, 3.0), width: 1.6, kind, hanging, colors: Colors::default() }
    }

    #[test]
    fn path_kinds_valid_and_deterministic() {
        for kind in [PathKind::Gravel, PathKind::Pavers, PathKind::Slabs, PathKind::Poured] {
            let p = path(kind, 12.0, 2.0);
            let a = path_mesh(&p, &t(), 5);
            assert!(!a.is_empty() && a.is_valid(), "{kind:?}");
            assert_eq!(a, path_mesh(&p, &t(), 5));
            assert_ne!(a, path_mesh(&p, &t(), 6));
            // Ribbon faces up.
            let up = a.normals.iter().zip(&a.surfaces).filter(|(n, s)| **s != Surface::Rock as u8 && n[1] > 0.9).count();
            assert!(up > 20, "{kind:?} ribbon should face up");
        }
    }

    #[test]
    fn cobble_budget() {
        let m = path_mesh(&path(PathKind::Pavers, 30.0, 2.0), &t(), 1);
        assert!(tris(&m) <= 4000, "{} tris", tris(&m));
    }

    #[test]
    fn ford_places_stepping_stones() {
        let p = Path { points: vec![Vec2::new(0.0, 0.0), Vec2::new(14.0, 0.0)], width: 1.5, kind: PathKind::Pavers };
        let m = build_path(&p, 3, &|_| 0.0, &|q| (q.x > 5.0 && q.x < 10.0).then_some(0.5));
        assert!(m.is_valid());
        let rock_top = m.positions.iter().zip(&m.surfaces).filter(|(_, s)| **s == Surface::Rock as u8).map(|(p, _)| p[1]).fold(f32::MIN, f32::max);
        assert!((rock_top - 0.6).abs() < 1e-4, "{rock_top}");
        let rocks = m.surfaces.iter().filter(|s| **s == Surface::Rock as u8).count();
        assert!(rocks > 20);
        // No ribbon over the water.
        assert!(m.positions.iter().zip(&m.surfaces).all(|(p, s)| *s == Surface::Rock as u8 || !(p[0] > 5.6 && p[0] < 9.4)));
    }

    #[test]
    fn steep_path_gets_risers() {
        let p = Path { points: vec![Vec2::new(0.0, 0.0), Vec2::new(10.0, 0.0)], width: 1.5, kind: PathKind::Gravel };
        let flat = build_path(&p, 3, &|_| 0.0, &|_| None);
        let steep = build_path(&p, 3, &|q| q.x * 0.8, &|_| None);
        assert!(steep.is_valid() && tris(&steep) > tris(&flat) + 40);
    }

    #[test]
    fn degenerate_paths() {
        let nan = Vec2::new(f32::NAN, 0.0);
        for pts in [vec![], vec![Vec2::ZERO], vec![Vec2::ZERO, Vec2::ZERO], vec![nan, Vec2::ONE], vec![Vec2::ZERO, nan, Vec2::new(5.0, 0.0)]] {
            for width in [0.0, f32::NAN, 100.0] {
                let m = path_mesh(&Path { points: pts.clone(), width, kind: PathKind::Pavers }, &t(), 1);
                assert!(m.is_valid());
            }
        }
    }

    #[test]
    fn stairs_all_kinds() {
        for kind in [StairKind::Steel, StairKind::Concrete, StairKind::Ladder] {
            let (m, _) = stairs_mesh(&stairs(kind, 4.0), &t(), 1);
            assert!(!m.is_empty() && m.is_valid(), "{kind:?}");
            let (lo, hi) = m.bounds().unwrap();
            assert!(hi.y >= 3.9 && lo.y > -1.0, "{kind:?} {lo} {hi}");
        }
        assert_eq!(step_count(4.0, 8.0), 16);
        assert_eq!(step_count(4.0, 3.0), 10); // tread floor of 0.3 m wins
        assert_eq!(step_count(0.0, 3.0), 0);
        // Reversed endpoints give the same flight.
        let s = stairs(StairKind::Concrete, 2.0);
        let r = Stairs { from: s.to, to: s.from, ..s.clone() };
        assert_eq!(stairs_mesh(&s, &t(), 1).0, stairs_mesh(&r, &t(), 1).0);
    }

    #[test]
    fn tall_wood_stairs_have_lantern() {
        let (m, mk) = stairs_mesh(&stairs(StairKind::Steel, 4.0), &t(), 1);
        assert!(m.is_valid());
        assert_eq!(mk.iter().filter(|k| k.kind == MarkerKind::Lantern).count(), 1);
        let (_, mk) = stairs_mesh(&stairs(StairKind::Steel, 2.0), &t(), 1);
        assert!(mk.is_empty());
    }

    #[test]
    fn degenerate_stairs() {
        let z = Stairs { from: Vec3::ONE, to: Vec3::ONE, width: 1.0, kind: StairKind::Steel, colors: Colors::default() };
        assert!(stairs_mesh(&z, &t(), 1).0.is_empty());
        let nan = Stairs { to: Vec3::new(f32::NAN, 0.0, 0.0), ..z.clone() };
        assert!(stairs_mesh(&nan, &t(), 1).0.is_empty());
        let flat = Stairs { to: Vec3::new(3.0, 1.0, 1.0), ..z.clone() };
        for kind in [StairKind::Steel, StairKind::Concrete, StairKind::Ladder] {
            assert!(stairs_mesh(&Stairs { kind, ..flat.clone() }, &t(), 1).0.is_valid());
        }
        let vertical = Stairs { to: Vec3::new(1.0, 4.0, 1.0), ..z.clone() };
        let (m, _) = stairs_mesh(&vertical, &t(), 1);
        assert!(m.is_valid() && !m.is_empty());
    }

    #[test]
    fn platforms() {
        for h in [0.5, 1.0, 2.5] {
            let p = Platform { center: Vec2::new(2.0, 1.0), half: Vec2::new(2.5, 1.5), angle: 0.7, height: h, colors: Colors::default() };
            let (m, mk) = platform_mesh(&p, &t(), 1);
            assert!(!m.is_empty() && m.is_valid());
            assert_eq!(mk.len(), 4);
            assert!(mk.iter().all(|k| k.kind == MarkerKind::Perch));
            let (_, hi) = m.bounds().unwrap();
            assert!(hi.y >= h - 1e-4);
        }
        let bad = Platform { center: Vec2::ZERO, half: Vec2::new(f32::NAN, 1.0), angle: 0.0, height: 1.0, colors: Colors::default() };
        assert!(platform_mesh(&bad, &t(), 1).0.is_empty());
        let tiny = Platform { half: Vec2::ZERO, height: -3.0, ..bad.clone() };
        let tiny = Platform { half: Vec2::ZERO, ..tiny };
        assert!(platform_mesh(&tiny, &t(), 1).0.is_valid());
    }

    #[test]
    fn bridges_all_kinds() {
        let mut kinds = vec![BridgeKind::Arch, BridgeKind::Girder];
        kinds.extend((0..4).map(|r| BridgeKind::Cable { railing: r }));
        kinds.push(BridgeKind::Cable { railing: 9 });
        for kind in kinds {
            for hanging in [Hanging::None, Hanging::Lamps, Hanging::Vines] {
                let (m, mk) = bridge_mesh(&bridge(kind, hanging), &t(), 2);
                assert!(!m.is_empty() && m.is_valid(), "{kind:?} {hanging:?}");
                assert!(tris(&m) < 12000, "{kind:?} {} tris", tris(&m));
                assert!(mk.iter().filter(|k| k.kind == MarkerKind::Perch).count() >= 2);
                let lanterns = mk.iter().filter(|k| k.kind == MarkerKind::Lantern).count();
                let flags = mk.iter().filter(|k| k.kind == MarkerKind::Flag).count();
                if matches!(kind, BridgeKind::Cable { .. }) && hanging == Hanging::Lamps {
                    assert!(lanterns >= 4, "{kind:?}");
                } else {
                    assert_eq!(lanterns, 0);
                }
                assert_eq!(flags > 0, matches!(kind, BridgeKind::Cable { .. }) && hanging == Hanging::Vines);
                assert_eq!((m.clone(), mk.clone()), bridge_mesh(&bridge(kind, hanging), &t(), 2));
            }
        }
    }

    #[test]
    fn long_stone_bridge_has_several_arches() {
        let b = Bridge { b: Vec3::new(30.0, 6.0, 0.0), ..bridge(BridgeKind::Arch, Hanging::None) };
        let (m, _) = bridge_mesh(&b, &t(), 1);
        assert!(m.is_valid() && tris(&m) > 600);
    }

    #[test]
    fn degenerate_bridges() {
        for kind in [BridgeKind::Arch, BridgeKind::Girder, BridgeKind::Cable { railing: 1 }] {
            let mut b = bridge(kind, Hanging::Lamps);
            b.b = b.a;
            assert!(bridge_mesh(&b, &t(), 1).0.is_empty());
            b.b = b.a + Vec3::new(0.7, 0.0, 0.0);
            assert!(bridge_mesh(&b, &t(), 1).0.is_valid());
            b.b = Vec3::new(f32::NAN, 0.0, 0.0);
            assert!(bridge_mesh(&b, &t(), 1).0.is_empty());
            b = bridge(kind, Hanging::Vines);
            b.width = 0.0;
            assert!(bridge_mesh(&b, &t(), 1).0.is_valid());
            b.width = f32::NAN;
            assert!(bridge_mesh(&b, &t(), 1).0.is_valid());
        }
    }
}
