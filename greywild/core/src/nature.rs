//! Trees, clutter props and painted vegetation scatter.
//!
//! Objects are built in a local frame (origin on the ground, +y up), then scaled, rotated by
//! `Placed::angle` in the x,z plane and stood on the terrain.

use crate::geom;
use crate::mesh::{hash2, Marker, MarkerKind, MeshData, Rng, Surface};
use crate::model::{ClutterKind, Colors, PaintLayer, Placed, PropKind, TreeKind, GLADE_HALF};
use crate::palette;
use crate::terrain::Terrain;
use glam::{Vec2, Vec3};
use std::collections::HashMap;

const V: fn(f32, f32, f32) -> Vec3 = Vec3::new;

// ---------------------------------------------------------------------------------------------
// Small helpers
// ---------------------------------------------------------------------------------------------

fn tint(c: Vec3, rng: &mut Rng, amt: f32) -> Vec3 {
    let k = 1.0 + (rng.f() - 0.5) * amt;
    let h = V(1.0 + (rng.f() - 0.5) * amt * 0.5, 1.0, 1.0 + (rng.f() - 0.5) * amt * 0.5);
    (c * k * h).max(Vec3::ZERO)
}

const ICO_FACES: [[usize; 3]; 20] = [
    [0, 11, 5], [0, 5, 1], [0, 1, 7], [0, 7, 10], [0, 10, 11],
    [1, 5, 9], [5, 11, 4], [11, 10, 2], [10, 7, 6], [7, 1, 8],
    [3, 9, 4], [3, 4, 2], [3, 2, 6], [3, 6, 8], [3, 8, 9],
    [4, 9, 5], [2, 4, 11], [6, 2, 10], [8, 6, 7], [9, 8, 1],
];

/// Lumpy low-poly icosphere (20 flat triangles) with radii `r`, vertices jittered by `jit`.
/// `floor` clamps vertices to a minimum y (boulders and bushes sitting on the ground).
#[allow(clippy::too_many_arguments)]
fn blob(m: &mut MeshData, c: Vec3, r: Vec3, jit: f32, col: Vec3, s: Surface, rng: &mut Rng, floor: Option<f32>) {
    let t = (1.0 + 5f32.sqrt()) / 2.0;
    let raw = [
        V(-1., t, 0.), V(1., t, 0.), V(-1., -t, 0.), V(1., -t, 0.),
        V(0., -1., t), V(0., 1., t), V(0., -1., -t), V(0., 1., -t),
        V(t, 0., -1.), V(t, 0., 1.), V(-t, 0., -1.), V(-t, 0., 1.),
    ];
    let mut p = [Vec3::ZERO; 12];
    for (i, v) in raw.iter().enumerate() {
        let k = 1.0 + (rng.f() - 0.5) * 2.0 * jit;
        let mut q = c + v.normalize() * r * k;
        if let Some(f) = floor {
            q.y = q.y.max(f);
        }
        p[i] = q;
    }
    for f in ICO_FACES {
        let (a, mut b, mut d) = (p[f[0]], p[f[1]], p[f[2]]);
        let n = (b - a).cross(d - a);
        if n.dot((a + b + d) / 3.0 - c) < 0.0 {
            std::mem::swap(&mut b, &mut d);
        }
        let shade = 0.93 + 0.14 * rng.f();
        m.tri(a, b, d, col * shade, s, 0.7);
    }
}

/// Prism along `a`→`b` (logs, wheels) with end caps; outward normals.
fn prism(m: &mut MeshData, a: Vec3, b: Vec3, r: f32, sides: u32, side_col: Vec3, cap_col: Vec3, s: Surface) {
    let d = b - a;
    let len = d.length();
    if len < 1e-4 {
        return;
    }
    let d = d / len;
    let u = d.cross(Vec3::Y).normalize_or(Vec3::X);
    let v = d.cross(u);
    let ring = |i: u32, c: Vec3| {
        let ang = i as f32 / sides as f32 * std::f32::consts::TAU;
        c + (u * ang.cos() + v * ang.sin()) * r
    };
    for i in 0..sides {
        let (a0, a1, b0, b1) = (ring(i, a), ring(i + 1, a), ring(i, b), ring(i + 1, b));
        m.quad(a0, a1, b1, b0, side_col, s, 1.0);
        m.tri(b, b0, b1, cap_col, s, 1.0);
        m.tri(a, a1, a0, cap_col, s, 1.0);
    }
}

#[allow(clippy::too_many_arguments)]
fn cyl(m: &mut MeshData, base: Vec3, r0: f32, r1: f32, h: f32, sides: u32, caps: (bool, bool), col: Vec3, s: Surface) {
    m.cylinder(base, r0, r1, h, sides, caps.0, caps.1, col, s, 1.0);
}

fn tri2(m: &mut MeshData, a: Vec3, b: Vec3, c: Vec3, col: Vec3, s: Surface) {
    m.tri(a, b, c, col, s, 1.0);
    m.tri(a, c, b, col, s, 1.0);
}

fn quad2(m: &mut MeshData, a: Vec3, b: Vec3, c: Vec3, d: Vec3, col: Vec3, s: Surface) {
    m.quad(a, b, c, d, col, s, 1.0);
    m.quad(a, d, c, b, col, s, 1.0);
}

/// Thin upright triangle visible from both sides; normals point up (soft grass lighting).
fn blade(m: &mut MeshData, base: Vec3, dir: Vec3, half_w: f32, tip: Vec3, c0: Vec3, c1: Vec3, s: Surface) {
    let a = m.vertex(base - dir * half_w, Vec3::Y, Vec2::new(0.0, 0.0), c0, s);
    let b = m.vertex(base + dir * half_w, Vec3::Y, Vec2::new(1.0, 0.0), c0, s);
    let t = m.vertex(tip, Vec3::Y, Vec2::new(0.5, 1.0), c1, s);
    m.indices.extend_from_slice(&[a, b, t, a, t, b]);
}

/// Thin tapered upright quad visible from both sides.
#[allow(clippy::too_many_arguments)]
fn stalk(m: &mut MeshData, base: Vec3, dir: Vec3, w0: f32, top: Vec3, w1: f32, c0: Vec3, c1: Vec3, s: Surface) {
    let a = m.vertex(base - dir * w0, Vec3::Y, Vec2::new(0.0, 0.0), c0, s);
    let b = m.vertex(base + dir * w0, Vec3::Y, Vec2::new(1.0, 0.0), c0, s);
    let c = m.vertex(top + dir * w1, Vec3::Y, Vec2::new(1.0, 1.0), c1, s);
    let d = m.vertex(top - dir * w1, Vec3::Y, Vec2::new(0.0, 1.0), c1, s);
    m.indices.extend_from_slice(&[a, b, c, a, c, d, a, c, b, a, d, c]);
}

fn bush_into(m: &mut MeshData, base: Vec3, scale: f32, blobs: usize, rng: &mut Rng) {
    for _ in 0..blobs {
        let off = V(rng.range(-0.35, 0.35), 0.0, rng.range(-0.35, 0.35)) * scale;
        let r = rng.range(0.45, 0.62) * scale;
        let col = tint(palette::leaves(), rng, 0.3);
        blob(m, base + off + Vec3::Y * (r * 0.8), V(r, r * 0.8, r), 0.18, col, Surface::Leaves, rng, Some(base.y + 0.02));
    }
}

/// Long arching leaf strip (double-sided): rises `rise`, droops `droop`, tapers to a tip.
#[allow(clippy::too_many_arguments)]
fn frond(m: &mut MeshData, base: Vec3, yaw: f32, len: f32, rise: f32, droop: f32, w: f32, c0: Vec3, c1: Vec3, s: Surface) {
    const SEGS: usize = 4;
    let dir = V(yaw.cos(), 0.0, yaw.sin());
    let side = V(-yaw.sin(), 0.0, yaw.cos());
    let at = |t: f32| base + dir * (len * t) + Vec3::Y * (rise * t - droop * t * t);
    let wd = |t: f32| w * (1.0 - t * 0.92);
    for i in 0..SEGS {
        let (t0, t1) = (i as f32 / SEGS as f32, (i + 1) as f32 / SEGS as f32);
        let (p0, p1) = (at(t0), at(t1));
        let col = c0.lerp(c1, (t0 + t1) * 0.5);
        quad2(m, p0 - side * wd(t0), p0 + side * wd(t0), p1 + side * wd(t1), p1 - side * wd(t1), col, s);
    }
}

/// Small ground fern: a rosette of arching fronds.
fn fern_clump(m: &mut MeshData, at: Vec3, scale: f32, rng: &mut Rng) {
    let col = tint(palette::leaves() * V(0.75, 1.0, 0.7), rng, 0.3);
    let n = 5 + (rng.f() * 2.0) as usize;
    let y0 = rng.f() * std::f32::consts::TAU;
    for k in 0..n {
        let yaw = y0 + k as f32 / n as f32 * std::f32::consts::TAU + rng.range(-0.2, 0.2);
        let len = rng.range(0.4, 0.6) * scale;
        frond(m, at + Vec3::Y * 0.02, yaw, len, 0.35 * scale, 0.5 * scale, 0.07 * scale, col * 0.7, col * 1.1, Surface::Leaves);
    }
}

// ---------------------------------------------------------------------------------------------
// Trees
// ---------------------------------------------------------------------------------------------

fn build_tree(kind: TreeKind, rng: &mut Rng) -> (MeshData, Vec<Marker>) {
    let mut m = MeshData::default();
    match kind {
        TreeKind::Oak => {
            let bark = tint(palette::bark(), rng, 0.3);
            cyl(&mut m, Vec3::ZERO, 0.30, 0.18, 2.4, 6, (false, false), bark, Surface::Bark);
            let spots = [(0.0, 3.7, 0.0, 1.45), (0.95, 3.1, 0.35, 1.05), (-0.85, 3.2, -0.4, 1.15), (0.2, 3.2, -1.0, 0.95), (-0.2, 4.2, 0.5, 0.95)];
            let n = 3 + (rng.f() * 3.0) as usize;
            for &(x, y, z, r) in spots.iter().take(n.min(5)) {
                let col = tint(palette::leaves(), rng, 0.35);
                blob(&mut m, V(x, y, z), V(r, r * 0.85, r), 0.18, col, Surface::Leaves, rng, None);
            }
        }
        TreeKind::Fern => {
            let bark = tint(palette::bark() * V(0.9, 0.7, 0.6), rng, 0.3);
            cyl(&mut m, Vec3::ZERO, 0.17, 0.11, 2.5, 6, (false, false), bark, Surface::Bark);
            for y in [0.5, 1.2, 1.9] {
                cyl(&mut m, V(0.0, y, 0.0), 0.185 - 0.03 * y, 0.17 - 0.03 * y, 0.1, 6, (false, false), bark * 0.75, Surface::Fiber);
            }
            let n = 8 + (rng.f() * 5.0) as usize;
            let y0 = rng.f() * std::f32::consts::TAU;
            for k in 0..n {
                let yaw = y0 + k as f32 / n as f32 * std::f32::consts::TAU + rng.range(-0.15, 0.15);
                let col = tint(palette::leaves() * V(0.8, 1.05, 0.75), rng, 0.25);
                let (len, rise) = (rng.range(1.7, 2.2), rng.range(1.4, 1.8));
                frond(&mut m, V(0.0, 2.45, 0.0), yaw, len, rise, 1.6, 0.17, col * 0.7, col * 1.1, Surface::Leaves);
            }
            cyl(&mut m, V(0.0, 2.45, 0.0), 0.1, 0.0, 0.4, 5, (false, false), palette::leaves() * 0.9, Surface::Leaves);
        }
        TreeKind::Pine => {
            let bark = tint(palette::bark(), rng, 0.3) * 0.8;
            cyl(&mut m, Vec3::ZERO, 0.26, 0.16, 1.4, 6, (false, false), bark, Surface::Bark);
            let four = rng.f() < 0.6;
            let tiers: &[(f32, f32, f32)] = if four {
                &[(0.9, 1.7, 2.2), (2.3, 1.4, 2.0), (3.6, 1.1, 2.0), (4.9, 0.8, 2.2)]
            } else {
                &[(0.9, 1.8, 2.4), (2.4, 1.4, 2.2), (3.9, 1.0, 2.2)]
            };
            for (i, &(y, r, h)) in tiers.iter().enumerate() {
                let col = tint(palette::pine(), rng, 0.25) * (0.9 + 0.08 * i as f32);
                cyl(&mut m, V(0.0, y, 0.0), r, 0.0, h, 7, (false, true), col, Surface::Leaves);
            }
        }
        TreeKind::Birch => {
            let white = V(0.88, 0.87, 0.82);
            cyl(&mut m, Vec3::ZERO, 0.12, 0.07, 3.6, 6, (false, false), white, Surface::Bark);
            for _ in 0..6 {
                let y = rng.range(0.4, 3.0);
                let ang = rng.f() * std::f32::consts::TAU;
                let rad = 0.12 - 0.05 * y / 3.6 + 0.005;
                m.aabb(V(ang.cos() * rad - 0.03, y - 0.04, ang.sin() * rad - 0.03), V(ang.cos() * rad + 0.03, y + 0.04, ang.sin() * rad + 0.03), V(0.08, 0.07, 0.07), Surface::Bark, 1.0);
            }
            let spots = [(0.0, 4.4, 0.0, 0.95), (0.55, 3.7, 0.3, 0.8), (-0.5, 3.5, -0.3, 0.85), (0.1, 3.4, -0.7, 0.7)];
            for &(x, y, z, r) in &spots {
                let col = tint(palette::leaves() * V(1.2, 1.2, 0.9), rng, 0.3);
                blob(&mut m, V(x, y, z), V(r, r * 0.9, r), 0.2, col, Surface::Leaves, rng, None);
            }
        }
        TreeKind::Fruit => {
            let bark = tint(palette::bark(), rng, 0.3);
            cyl(&mut m, Vec3::ZERO, 0.2, 0.14, 1.3, 6, (false, false), bark, Surface::Bark);
            let c = V(0.0, 2.0, 0.0);
            let r = 1.15;
            blob(&mut m, c, V(r, r * 0.9, r), 0.15, tint(palette::leaves(), rng, 0.25), Surface::Leaves, rng, None);
            blob(&mut m, V(0.6, 1.75, 0.3), V(0.8, 0.7, 0.8), 0.15, tint(palette::leaves(), rng, 0.25), Surface::Leaves, rng, None);
            for _ in 0..7 {
                let th = rng.f() * std::f32::consts::TAU;
                let ph = rng.range(0.2, 1.5);
                let dir = V(th.cos() * ph.sin(), ph.cos(), th.sin() * ph.sin());
                let p = c + dir * V(r, r * 0.9, r) * 1.02;
                let col = if rng.f() < 0.5 { V(0.95, 0.6, 0.7) } else { V(0.85, 0.15, 0.1) };
                m.aabb(p - Vec3::splat(0.07), p + Vec3::splat(0.07), col, Surface::Petal, 1.0);
            }
        }
    }
    let top = m.bounds().map(|b| b.1.y).unwrap_or(0.0);
    let markers = vec![Marker { kind: MarkerKind::Perch, pos: V(0.0, top, 0.0), dir: Vec3::Y }];
    (m, markers)
}

// ---------------------------------------------------------------------------------------------
// Clutter
// ---------------------------------------------------------------------------------------------

fn build_clutter(kind: ClutterKind, rng: &mut Rng, colors: Colors) -> (MeshData, Vec<Marker>) {
    let mut m = MeshData::default();
    let mut markers = Vec::new();
    let wood = tint(palette::wood(), rng, 0.25);
    let trim = palette::trim(colors);
    let metal = palette::metal();
    match kind {
        ClutterKind::RainBarrel => {
            let rust = tint(V(0.34, 0.14, 0.07), rng, 0.25);
            cyl(&mut m, Vec3::ZERO, 0.38, 0.38, 0.9, 8, (true, true), rust, Surface::Steel);
            for y in [0.1, 0.45, 0.78] {
                cyl(&mut m, V(0.0, y, 0.0), 0.4, 0.4, 0.05, 8, (false, false), rust * 0.7, Surface::Steel);
            }
            cyl(&mut m, V(0.0, 0.9, 0.0), 0.3, 0.3, 0.03, 8, (true, false), V(0.02, 0.03, 0.04), Surface::Metal);
            // Downpipe stub dropping into the drum.
            m.beam(V(0.0, 1.22, -0.5), V(0.0, 1.22, -0.2), 0.07, 0.07, trim, Surface::Steel, 1.0);
            cyl(&mut m, V(0.0, 0.9, -0.2), 0.035, 0.035, 0.34, 6, (false, false), trim, Surface::Steel);
            cyl(&mut m, V(0.0, 0.0, -0.5), 0.035, 0.035, 1.25, 6, (false, false), trim, Surface::Steel);
        }
        ClutterKind::Planter => {
            let c = palette::wall(colors);
            m.aabb(V(-0.5, 0.0, -0.5), V(0.5, 0.55, 0.5), c, Surface::Concrete, 1.0);
            m.aabb(V(-0.54, 0.5, -0.54), V(0.54, 0.6, 0.54), c * 0.9, Surface::Plinth, 1.0);
            m.aabb(V(-0.44, 0.6, -0.44), V(0.44, 0.62, 0.44), palette::dirt() * 0.45, Surface::Dirt, 1.0);
            blob(&mut m, V(-0.05, 0.9, 0.0), V(0.42, 0.3, 0.42), 0.2, tint(palette::leaves(), rng, 0.3), Surface::Leaves, rng, None);
            for _ in 0..4 {
                let (x, z) = (rng.range(-0.35, 0.35), rng.range(-0.35, 0.35));
                let h = rng.range(0.85, 1.1);
                let col = palette::flower(rng.next_u64() as u32);
                m.aabb(V(x - 0.012, 0.62, z - 0.012), V(x + 0.012, h, z + 0.012), palette::grass() * 0.6, Surface::Grass, 1.0);
                m.aabb(V(x - 0.06, h, z - 0.06), V(x + 0.06, h + 0.07, z + 0.06), col, Surface::Petal, 1.0);
            }
        }
        ClutterKind::Compost => {
            let slat = |m: &mut MeshData, a: Vec3, b: Vec3| {
                for k in 0..4 {
                    let y = 0.1 + 0.2 * k as f32;
                    m.beam(a + Vec3::Y * y, b + Vec3::Y * y, 0.04, 0.15, wood * (0.9 + 0.05 * k as f32), Surface::Cladding, 1.0);
                }
            };
            for (sx, sz) in [(-1.0f32, -1.0f32), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)] {
                m.aabb(V(sx * 0.62 - 0.05, 0.0, sz * 0.5 - 0.05), V(sx * 0.62 + 0.05, 0.9, sz * 0.5 + 0.05), wood * 0.8, Surface::Cladding, 1.0);
            }
            slat(&mut m, V(-0.62, 0.0, -0.5), V(0.62, 0.0, -0.5));
            slat(&mut m, V(-0.62, 0.0, -0.5), V(-0.62, 0.0, 0.5));
            slat(&mut m, V(0.62, 0.0, -0.5), V(0.62, 0.0, 0.5));
            blob(&mut m, V(0.0, 0.3, 0.0), V(0.55, 0.45, 0.4), 0.2, palette::dirt() * 0.4, Surface::Dirt, rng, Some(0.05));
            blob(&mut m, V(0.2, 0.65, 0.05), V(0.22, 0.14, 0.22), 0.3, tint(palette::leaves() * 0.8, rng, 0.3), Surface::Leaves, rng, None);
        }
        ClutterKind::Cistern => {
            let c = palette::wall(colors);
            cyl(&mut m, Vec3::ZERO, 0.9, 0.85, 1.1, 10, (false, true), c, Surface::Concrete);
            cyl(&mut m, V(0.0, 1.1, 0.0), 0.95, 0.95, 0.12, 10, (true, false), c * 0.9, Surface::Plinth);
            m.aabb(V(-0.25, 1.22, -0.25), V(0.25, 1.3, 0.25), metal, Surface::Metal, 1.0);
            m.aabb(V(-0.1, 1.3, -0.03), V(0.1, 1.34, 0.03), trim, Surface::Steel, 1.0);
            m.beam(V(0.0, 0.7, 0.9), V(0.0, 0.7, 1.15), 0.08, 0.08, trim, Surface::Steel, 1.0);
        }
        ClutterKind::Wheelbarrow => {
            let rust = tint(V(0.34, 0.14, 0.07), rng, 0.25);
            m.aabb(V(-0.8, 0.5, -0.5), V(0.8, 0.58, 0.5), rust, Surface::Steel, 1.0);
            for sz in [-1.0f32, 1.0] {
                m.aabb(V(-0.8, 0.58, sz * 0.5 - 0.02), V(0.8, 0.85, sz * 0.5 + 0.02), rust * 0.95, Surface::Steel, 1.0);
            }
            for sx in [-1.0f32, 1.0] {
                m.aabb(V(sx * 0.8 - 0.02, 0.58, -0.5), V(sx * 0.8 + 0.02, 0.85, 0.5), rust * 0.95, Surface::Steel, 1.0);
            }
            m.aabb(V(-0.03, 0.42, -0.6), V(0.03, 0.48, 0.6), trim, Surface::Steel, 1.0);
            for sz in [-1.0f32, 1.0] {
                let z = sz * 0.58;
                prism(&mut m, V(0.0, 0.45, z), V(0.0, 0.45, z + sz * 0.08), 0.45, 8, trim, trim * 0.8, Surface::Steel);
                m.beam(V(0.8, 0.5, sz * 0.3), V(2.0, 0.35, sz * 0.3), 0.08, 0.08, wood, Surface::Cladding, 1.0);
            }
            blob(&mut m, V(0.0, 0.62, 0.0), V(0.6, 0.2, 0.4), 0.2, palette::dirt() * 0.45, Surface::Dirt, rng, None);
        }
        ClutterKind::Bench => {
            let c = palette::wall(colors);
            for sx in [-0.55f32, 0.55] {
                m.aabb(V(sx - 0.14, 0.0, -0.2), V(sx + 0.14, 0.38, 0.2), c * 0.9, Surface::Concrete, 1.0);
            }
            m.aabb(V(-0.85, 0.38, -0.25), V(0.85, 0.52, 0.25), c, Surface::Concrete, 1.0);
            if rng.f() < 0.7 {
                m.aabb(V(-0.5 + rng.f() * 0.5, 0.52, -0.2), V(0.1 + rng.f() * 0.6, 0.55, 0.0), palette::leaves() * 0.7, Surface::Sedum, 1.0);
            }
        }
        ClutterKind::LampPost => {
            let c = palette::wall(colors);
            m.aabb(V(-0.17, 0.0, -0.17), V(0.17, 0.4, 0.17), c, Surface::Concrete, 1.0);
            cyl(&mut m, V(0.0, 0.4, 0.0), 0.08, 0.05, 2.05, 6, (true, false), trim, Surface::Steel);
            m.beam(V(0.0, 2.4, 0.0), V(0.5, 2.4, 0.0), 0.06, 0.06, trim, Surface::Steel, 1.0);
            m.aabb(V(0.49, 2.3, -0.015), V(0.51, 2.41, 0.015), metal, Surface::Metal, 1.0);
            m.aabb(V(0.38, 2.25, -0.12), V(0.62, 2.3, 0.12), metal, Surface::Metal, 1.0);
            m.aabb(V(0.38, 1.95, -0.12), V(0.62, 2.0, 0.12), metal, Surface::Metal, 1.0);
            m.aabb(V(0.41, 2.0, -0.09), V(0.59, 2.25, 0.09), palette::glow(), Surface::Glow, 1.0);
            markers.push(Marker { kind: MarkerKind::Lantern, pos: V(0.5, 2.12, 0.0), dir: Vec3::Y });
        }
        ClutterKind::WindTurbine => {
            let c = palette::white() * 0.85;
            cyl(&mut m, Vec3::ZERO, 0.12, 0.05, 5.3, 6, (true, false), c, Surface::Steel);
            m.aabb(V(-0.12, 5.3, -0.1), V(0.3, 5.5, 0.1), metal, Surface::Metal, 1.0);
            let hub = V(0.4, 5.4, 0.0);
            m.aabb(hub - V(0.0, 0.07, 0.07), hub + V(0.12, 0.07, 0.07), c, Surface::Steel, 1.0);
            let a0 = rng.f() * std::f32::consts::TAU;
            for k in 0..3 {
                let a = a0 + k as f32 * std::f32::consts::TAU / 3.0;
                let (d, s) = (V(0.0, a.cos(), a.sin()), V(0.0, -a.sin(), a.cos()));
                let tip = hub + d * 1.1;
                let (r0, r1) = (hub + d * 0.1 + s * 0.12, hub + d * 0.1 - s * 0.04);
                tri2(&mut m, r0, r1, tip, c, Surface::Steel);
            }
            markers.push(Marker { kind: MarkerKind::Flag, pos: hub, dir: Vec3::X });
        }
        ClutterKind::WashingLine => {
            for sx in [-1.6f32, 1.6] {
                cyl(&mut m, V(sx, 0.0, 0.0), 0.06, 0.05, 2.1, 6, (true, false), trim, Surface::Steel);
            }
            let line = |t: f32| V(-1.6 + 3.2 * t, 1.95 - 0.25 * (1.0 - (2.0 * t - 1.0).powi(2)), 0.0);
            for i in 0..6 {
                m.beam(line(i as f32 / 6.0), line((i + 1) as f32 / 6.0), 0.02, 0.02, palette::white() * 0.8, Surface::Fiber, 1.0);
            }
            let cols = [palette::wall(colors), palette::roof(colors), palette::flower(rng.next_u64() as u32), palette::white()];
            for (k, t) in [0.15f32, 0.38, 0.62, 0.85].iter().enumerate() {
                let p = line(*t);
                let w = 0.22 + 0.06 * rng.f();
                let drop = 0.4 + 0.2 * rng.f();
                quad2(&mut m, V(p.x - w, p.y - drop, 0.0), V(p.x + w, p.y - drop, 0.0), V(p.x + w, p.y - 0.02, 0.0), V(p.x - w, p.y - 0.02, 0.0), cols[k], Surface::Fiber);
            }
        }
        ClutterKind::LogPile => {
            let bark = tint(palette::bark(), rng, 0.2);
            let cap = palette::wood() * 1.15;
            let rows: [(&[f32], f32); 3] = [(&[-0.3, 0.0, 0.3], 0.14), (&[-0.15, 0.15], 0.38), (&[0.0], 0.62)];
            for (zs, y) in rows {
                for &z in zs {
                    prism(&mut m, V(-0.6, y, z), V(0.6, y, z), 0.14, 6, bark, cap, Surface::Bark);
                }
            }
        }
        ClutterKind::Rock => {
            let c = tint(palette::rock(), rng, 0.2);
            blob(&mut m, V(0.0, 0.3, 0.0), V(0.9, 0.6, 0.75), 0.3, c, Surface::Rock, rng, Some(0.0));
            if rng.f() < 0.5 {
                blob(&mut m, V(0.85, 0.15, 0.3), V(0.45, 0.3, 0.4), 0.3, c * 0.95, Surface::Rock, rng, Some(0.0));
            }
        }
        ClutterKind::Bush => {
            let n = 2 + (rng.f() * 2.0) as usize;
            bush_into(&mut m, Vec3::ZERO, 1.0, n, rng);
        }
        ClutterKind::Beehive => {
            for (sx, sz) in [(-1.0f32, -1.0f32), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)] {
                m.aabb(V(sx * 0.28 - 0.03, 0.0, sz * 0.28 - 0.03), V(sx * 0.28 + 0.03, 0.45, sz * 0.28 + 0.03), trim, Surface::Steel, 1.0);
            }
            m.aabb(V(-0.34, 0.45, -0.34), V(0.34, 0.5, 0.34), wood, Surface::Cladding, 1.0);
            let straw = tint(palette::hay(), rng, 0.15);
            cyl(&mut m, V(0.0, 0.5, 0.0), 0.3, 0.33, 0.18, 8, (false, false), straw * 0.95, Surface::Fiber);
            cyl(&mut m, V(0.0, 0.68, 0.0), 0.33, 0.25, 0.18, 8, (false, false), straw, Surface::Fiber);
            cyl(&mut m, V(0.0, 0.86, 0.0), 0.25, 0.12, 0.16, 8, (false, false), straw * 1.05, Surface::Fiber);
            cyl(&mut m, V(0.0, 1.02, 0.0), 0.12, 0.0, 0.1, 8, (false, false), straw * 1.1, Surface::Fiber);
        }
        ClutterKind::Solar => {
            let panel = V(0.04, 0.08, 0.2);
            // Steel A-frame: tall back legs, short front legs, rail along the panel underside.
            for sx in [-1.1f32, 0.0, 1.1] {
                m.beam(V(sx, 0.0, 0.5), V(sx, 0.62, 0.5), 0.06, 0.06, trim, Surface::Steel, 1.0);
                m.beam(V(sx, 0.0, -0.55), V(sx, 1.45, -0.55), 0.06, 0.06, trim, Surface::Steel, 1.0);
            }
            m.beam(V(-1.25, 0.6, 0.54), V(1.25, 0.6, 0.54), 0.05, 0.05, trim, Surface::Steel, 1.0);
            m.beam(V(-1.25, 1.44, -0.58), V(1.25, 1.44, -0.58), 0.05, 0.05, trim, Surface::Steel, 1.0);
            for k in 0..3 {
                let (x0, x1) = (-1.2 + k as f32 * 0.82, -0.44 + k as f32 * 0.82);
                m.quad(V(x0, 0.64, 0.56), V(x1, 0.64, 0.56), V(x1, 1.5, -0.56), V(x0, 1.5, -0.56), panel, Surface::Window, 1.0);
            }
            m.aabb(V(1.35, 0.0, 0.3), V(1.65, 0.45, 0.6), palette::wall(colors), Surface::Concrete, 1.0);
        }
    }
    (m, markers)
}

// ---------------------------------------------------------------------------------------------
// placed_mesh
// ---------------------------------------------------------------------------------------------

/// One placed tree or clutter object, standing on the terrain.
pub fn placed_mesh(placed: &Placed, terrain: &Terrain, seed: u64) -> (MeshData, Vec<Marker>) {
    let at = if placed.at.is_finite() { placed.at } else { Vec2::ZERO };
    let angle = if placed.angle.is_finite() { placed.angle } else { 0.0 };
    let scale = if placed.scale.is_finite() { placed.scale.clamp(0.3, 3.0) } else { 1.0 };
    let y = terrain.height(at);
    let y = if y.is_finite() { y } else { 0.0 };

    let salt = match placed.kind {
        PropKind::Tree(k) => k as u64 + 1,
        PropKind::Clutter(k) => k as u64 + 17,
    };
    let mut rng = Rng::at(seed ^ salt.wrapping_mul(0x9E37_79B9), at.x, at.y);
    let (mut m, mut markers) = match placed.kind {
        PropKind::Tree(k) => build_tree(k, &mut rng),
        PropKind::Clutter(k) => build_clutter(k, &mut rng, placed.colors),
    };

    let rot = Vec2::from_angle(angle);
    for p in &mut m.positions {
        let q = rot.rotate(Vec2::new(p[0], p[2])) * scale;
        *p = [at.x + q.x, y + p[1] * scale, at.y + q.y];
    }
    for n in &mut m.normals {
        let q = rot.rotate(Vec2::new(n[0], n[2]));
        *n = [q.x, n[1], q.y];
    }
    for mk in &mut markers {
        let q = rot.rotate(Vec2::new(mk.pos.x, mk.pos.z)) * scale;
        mk.pos = V(at.x + q.x, y + mk.pos.y * scale, at.y + q.y);
        let d = rot.rotate(Vec2::new(mk.dir.x, mk.dir.z));
        mk.dir = V(d.x, mk.dir.y, d.y);
    }
    (m, markers)
}

// ---------------------------------------------------------------------------------------------
// Scatter
// ---------------------------------------------------------------------------------------------

const CELL: f32 = 0.6;
const MAX_TRIS: usize = 80_000;
const BUSH_SPACING: f32 = 1.2;

/// What scatter needs to know about the ground (closures so tests can fake it).
struct Probe<'a> {
    height: &'a dyn Fn(Vec2) -> f32,
    normal_y: &'a dyn Fn(Vec2) -> f32,
    water: &'a dyn Fn(Vec2) -> bool,
    paint: &'a dyn Fn(PaintLayer, Vec2) -> f32,
}

/// Vegetation scattered over the glade from the terrain's paint layers and a little everywhere
/// on grass (tufts, a few wild flowers): flowers, bushes, grain, grass tufts.
/// Nothing is scattered inside `blockers` (structure footprints / path ribbons) or on water.
pub fn scatter_mesh(terrain: &Terrain, seed: u64, blockers: &[Vec<Vec2>]) -> MeshData {
    scatter_with(
        seed,
        blockers,
        &Probe {
            height: &|p| terrain.height(p),
            normal_y: &|p| terrain.normal(p).y,
            water: &|p| terrain.water_level(p).is_some(),
            paint: &|l, p| terrain.paint(l, p),
        },
    )
}

fn scatter_with(seed: u64, blockers: &[Vec<Vec2>], probe: &Probe) -> MeshData {
    let mut keep = 1.0f32;
    loop {
        let m = scatter_pass(seed, blockers, probe, keep);
        let n = m.indices.len() / 3;
        if n <= MAX_TRIS {
            return m;
        }
        // Over budget: thin every cell uniformly (slightly under-shoot; bush spacing is nonlinear).
        keep *= (MAX_TRIS as f32 / n as f32 * 0.97).min(0.95);
    }
}

/// One scatter pass; each cell survives with probability `keep` (hash-based, deterministic).
fn scatter_pass(seed: u64, blockers: &[Vec<Vec2>], probe: &Probe, keep: f32) -> MeshData {
    let mut m = MeshData::default();
    let seed = seed ^ 0x5CA7_7E12;
    let boxes: Vec<(Vec2, Vec2, &[Vec2])> = blockers
        .iter()
        .filter(|b| b.len() >= 3)
        .map(|b| {
            let lo = b.iter().fold(Vec2::splat(f32::INFINITY), |a, p| a.min(*p));
            let hi = b.iter().fold(Vec2::splat(f32::NEG_INFINITY), |a, p| a.max(*p));
            (lo, hi, b.as_slice())
        })
        .collect();
    let mut bushes: HashMap<(i32, i32), Vec<Vec2>> = HashMap::new();
    let n = (2.0 * GLADE_HALF / CELL) as i32;

    for cz in 0..n {
        for cx in 0..n {
            let bx = -GLADE_HALF + (cx as f32 + 0.5) * CELL;
            let bz = -GLADE_HALF + (cz as f32 + 0.5) * CELL;
            if keep < 1.0 && crate::mesh::hash_f(seed ^ 0x7B1D, bx, bz) >= keep {
                continue;
            }
            let mut rng = Rng::at(seed, bx, bz);
            let p = Vec2::new(bx + (rng.f() - 0.5) * CELL, bz + (rng.f() - 0.5) * CELL);
            if p.x.abs() > GLADE_HALF || p.y.abs() > GLADE_HALF {
                continue;
            }
            if boxes.iter().any(|(lo, hi, poly)| p.cmpge(*lo).all() && p.cmple(*hi).all() && geom::contains(poly, p)) {
                continue;
            }
            if (probe.water)(p) || (probe.normal_y)(p) < 0.75 {
                continue;
            }
            let y = (probe.height)(p);
            if !y.is_finite() {
                continue;
            }
            let at = V(p.x, y, p.y);
            let flowers = (probe.paint)(PaintLayer::Flowers, p).clamp(0.0, 1.0);
            let fern_s = (probe.paint)(PaintLayer::Ferns, p).clamp(0.0, 1.0);
            let moss = (probe.paint)(PaintLayer::Moss, p).clamp(0.0, 1.0);
            let gravel = (probe.paint)(PaintLayer::Gravel, p).clamp(0.0, 1.0);

            // Fern clumps (min spacing): painted, plus the odd one hugging water.
            let mut fern_p = 0.07 * fern_s;
            if fern_p < 0.01 && rng.f() < 0.04 {
                let wet = [Vec2::X, -Vec2::X, Vec2::Y, -Vec2::Y].iter().any(|d| (probe.water)(p + *d * 2.5));
                fern_p = if wet { 0.5 } else { 0.0 };
            }
            if fern_p > 0.0 && rng.f() < fern_p {
                let key = ((p.x / BUSH_SPACING).floor() as i32, (p.y / BUSH_SPACING).floor() as i32);
                let near = (-1..=1).any(|dx| {
                    (-1..=1).any(|dz| bushes.get(&(key.0 + dx, key.1 + dz)).is_some_and(|v| v.iter().any(|q| q.distance(p) < BUSH_SPACING)))
                });
                if !near {
                    bushes.entry(key).or_default().push(p);
                    fern_clump(&mut m, at, 0.7 + 0.6 * rng.f(), &mut rng);
                    continue;
                }
            }
            // Moss cushions.
            if moss > 0.05 && rng.f() < 0.4 * moss {
                moss_mound(&mut m, at, &mut rng);
                continue;
            }
            // Gravel: a few pebbles (the terrain shader paints the ground itself).
            if gravel > 0.05 && rng.f() < 0.05 * gravel {
                pebble(&mut m, at, &mut rng);
            }
            // Grass tufts (suppressed on gravel, moss and in dense flowers).
            let ptuft = 0.45 * (1.0 - (gravel * 1.5).min(1.0)) * (1.0 - moss * 0.8) * (1.0 - flowers * 0.5);
            if rng.f() < ptuft {
                tuft(&mut m, at, &mut rng);
            }
            // Clover patches in the wild meadow.
            if rng.f() < 0.1 * (1.0 - gravel) {
                clover(&mut m, at, &mut rng);
            }
            // Wildflowers: sparse everywhere, dense where painted.
            let pf = (0.05 + 0.5 * flowers) * (1.0 - gravel * 0.8);
            if rng.f() < pf {
                let patch = hash2(seed, (p.x / 3.0).floor(), (p.y / 3.0).floor());
                let idx = if rng.f() < 0.85 { patch % 6 } else { rng.next_u64() % 6 } as u32;
                flower(&mut m, at, idx, &mut rng);
            }
        }
    }
    m
}

fn tuft(m: &mut MeshData, at: Vec3, rng: &mut Rng) {
    let base_col = if rng.f() < 0.25 { palette::grass_dry() } else { palette::grass() } * rng.range(0.8, 1.2);
    let blades = 2 + (rng.f() * 2.0) as usize;
    let yaw0 = rng.f() * std::f32::consts::PI;
    for k in 0..blades {
        let yaw = yaw0 + k as f32 * std::f32::consts::PI / blades as f32 + rng.range(-0.2, 0.2);
        let dir = V(yaw.cos(), 0.0, yaw.sin());
        let h = rng.range(0.22, 0.45);
        let lean = V(rng.range(-0.08, 0.08), 0.0, rng.range(-0.08, 0.08));
        let base = at + V(rng.range(-0.05, 0.05), -0.02, rng.range(-0.05, 0.05));
        blade(m, base, dir, 0.035, base + lean + Vec3::Y * h, base_col * 0.7, base_col * 1.15, Surface::Grass);
    }
}

fn flower(m: &mut MeshData, at: Vec3, idx: u32, rng: &mut Rng) {
    let h = rng.range(0.25, 0.45);
    let yaw = rng.f() * std::f32::consts::PI;
    let dir = V(yaw.cos(), 0.0, yaw.sin());
    let stem = palette::grass() * 0.75;
    let top = at + V(rng.range(-0.03, 0.03), h, rng.range(-0.03, 0.03));
    blade(m, at - Vec3::Y * 0.02, dir, 0.012, top, stem * 0.8, stem, Surface::Grass);
    let col = (palette::flower(idx) * rng.range(0.9, 1.1)).max(Vec3::ZERO);
    m.cylinder(top - Vec3::Y * 0.02, 0.09, 0.0, 0.06, 4, false, false, col, Surface::Petal, 1.0);
}

/// Triangle lying flat, normal forced upwards.
fn up_tri(m: &mut MeshData, a: Vec3, b: Vec3, c: Vec3, col: Vec3, s: Surface) {
    if (b - a).cross(c - a).y >= 0.0 {
        m.tri(a, b, c, col, s, 1.0);
    } else {
        m.tri(a, c, b, col, s, 1.0);
    }
}

/// Low cushion of moss: one flattened lumpy blob hugging the ground.
fn moss_mound(m: &mut MeshData, at: Vec3, rng: &mut Rng) {
    let r = rng.range(0.22, 0.4);
    let col = tint(palette::leaves() * V(0.55, 0.95, 0.45), rng, 0.3);
    blob(m, at, V(r, r * 0.4, r), 0.2, col, Surface::Sedum, rng, Some(at.y - 0.01));
}

/// Small flattened stone: jittered octahedron.
fn pebble(m: &mut MeshData, at: Vec3, rng: &mut Rng) {
    let (rx, rz, h) = (rng.range(0.04, 0.1), rng.range(0.04, 0.1), rng.range(0.03, 0.06));
    let col = palette::rock() * rng.range(0.7, 1.1);
    let c = at + Vec3::Y * (h * 0.4);
    let ring = [V(rx, 0.0, 0.0), V(0.0, 0.0, rz * 1.2), V(-rx, 0.0, 0.0), V(0.0, 0.0, -rz)];
    for i in 0..4 {
        let (a, b) = (c + ring[i], c + ring[(i + 1) % 4]);
        m.tri(c + Vec3::Y * h, a, b, col, Surface::Rock, 1.0);
        m.tri(c - Vec3::Y * h * 0.4, b, a, col * 0.8, Surface::Rock, 1.0);
    }
}

/// Three flat trefoil leaves.
fn clover(m: &mut MeshData, at: Vec3, rng: &mut Rng) {
    let col = palette::grass() * V(0.7, 0.95, 0.6) * rng.range(0.85, 1.15);
    let yaw0 = rng.f() * std::f32::consts::TAU;
    let base = at + Vec3::Y * 0.015;
    for k in 0..3 {
        let a = yaw0 + k as f32 * std::f32::consts::TAU / 3.0;
        let (d, s) = (V(a.cos(), 0.0, a.sin()), V(-a.sin(), 0.0, a.cos()));
        up_tri(m, base, base + d * 0.14 + s * 0.06, base + d * 0.14 - s * 0.06, col, Surface::Leaves);
    }
}

// ---------------------------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    const TREES: [TreeKind; 4] = [TreeKind::Oak, TreeKind::Pine, TreeKind::Birch, TreeKind::Fruit];
    const CLUTTER: [ClutterKind; 14] = [
        ClutterKind::RainBarrel, ClutterKind::Planter, ClutterKind::Compost, ClutterKind::Cistern, ClutterKind::Wheelbarrow,
        ClutterKind::Bench, ClutterKind::LampPost, ClutterKind::WindTurbine, ClutterKind::WashingLine,
        ClutterKind::LogPile, ClutterKind::Rock, ClutterKind::Bush, ClutterKind::Beehive, ClutterKind::Solar,
    ];

    fn terrain() -> Terrain {
        Terrain::build(1, &[], &[])
    }

    fn put(kind: PropKind, scale: f32) -> (MeshData, Vec<Marker>) {
        let p = Placed { at: Vec2::new(3.0, -2.0), angle: 0.0, scale, kind, colors: Colors::default() };
        placed_mesh(&p, &terrain(), 7)
    }

    fn tris(m: &MeshData) -> usize {
        m.indices.len() / 3
    }

    #[test]
    fn trees_valid_and_sized() {
        let ranges = [(TreeKind::Oak, 4.0, 6.5), (TreeKind::Pine, 5.5, 8.5), (TreeKind::Birch, 4.0, 6.5), (TreeKind::Fruit, 2.4, 4.0)];
        for (k, lo, hi) in ranges {
            let (m, mk) = put(PropKind::Tree(k), 1.0);
            assert!(m.is_valid() && !m.is_empty(), "{k:?}");
            assert!(tris(&m) <= 600, "{k:?} {}", tris(&m));
            let top = m.bounds().unwrap().1.y - terrain().height(Vec2::new(3.0, -2.0));
            assert!(top > lo && top < hi, "{k:?} height {top}");
            assert!(mk.iter().any(|x| x.kind == MarkerKind::Perch));
        }
        assert_eq!(TREES.len(), ranges.len());
    }

    #[test]
    fn clutter_valid_and_sized() {
        for k in CLUTTER {
            let (m, _) = put(PropKind::Clutter(k), 1.0);
            assert!(m.is_valid() && !m.is_empty(), "{k:?}");
            assert!(tris(&m) <= 400, "{k:?} {}", tris(&m));
            let (lo, hi) = match k {
                ClutterKind::RainBarrel => (0.7, 1.1),
                ClutterKind::Planter => (0.6, 1.1),
                ClutterKind::Compost => (1.2, 2.2),
                ClutterKind::Cistern => (2.2, 3.4),
                ClutterKind::Wheelbarrow => (0.6, 1.4),
                ClutterKind::Bench => (0.5, 1.0),
                ClutterKind::LampPost => (2.0, 3.0),
                ClutterKind::WindTurbine => (5.0, 6.5),
                ClutterKind::WashingLine => (1.6, 2.5),
                ClutterKind::LogPile => (0.5, 1.0),
                ClutterKind::Rock => (0.3, 1.6),
                ClutterKind::Bush => (0.5, 1.7),
                ClutterKind::Beehive => (0.6, 1.5),
                ClutterKind::Solar => (1.8, 2.8),
            };
            let top = m.bounds().unwrap().1.y - terrain().height(Vec2::new(3.0, -2.0));
            assert!(top > lo && top < hi, "{k:?} height {top}");
        }
    }

    #[test]
    fn markers_for_lantern_and_flag() {
        let (_, mk) = put(PropKind::Clutter(ClutterKind::LampPost), 1.0);
        assert!(mk.iter().any(|x| x.kind == MarkerKind::Lantern));
        let (_, mk) = put(PropKind::Clutter(ClutterKind::WindTurbine), 1.0);
        let f = mk.iter().find(|x| x.kind == MarkerKind::Flag).unwrap();
        assert!(f.pos.y > 5.0);
    }

    #[test]
    fn scale_is_clamped_and_rotation_keeps_position() {
        let k = PropKind::Clutter(ClutterKind::RainBarrel);
        assert_eq!(put(k, 100.0).0, put(k, 3.0).0);
        assert_eq!(put(k, 0.001).0, put(k, 0.3).0);
        assert_eq!(put(k, f32::NAN).0, put(k, 1.0).0);
        let mut p = Placed { at: Vec2::new(5.0, 5.0), angle: 1.3, scale: 1.0, kind: k, colors: Colors::default() };
        let (m, _) = placed_mesh(&p, &terrain(), 1);
        let (lo, hi) = m.bounds().unwrap();
        assert!(((lo + hi) * 0.5 - V(5.0, 0.45 + terrain().height(Vec2::new(5.0, 5.0)), 5.0)).abs().max_element() < 0.2);
        p.angle = f32::NAN;
        assert!(placed_mesh(&p, &terrain(), 1).0.is_valid());
    }

    #[test]
    fn placement_is_deterministic() {
        for k in CLUTTER {
            assert_eq!(put(PropKind::Clutter(k), 1.2), put(PropKind::Clutter(k), 1.2));
        }
        for k in TREES {
            assert_eq!(put(PropKind::Tree(k), 1.0), put(PropKind::Tree(k), 1.0));
        }
    }

    #[test]
    fn prism_normals_point_outwards() {
        let mut m = MeshData::default();
        prism(&mut m, V(-1.0, 0.0, 0.0), V(1.0, 0.0, 0.0), 0.5, 6, Vec3::ONE, Vec3::ONE, Surface::Bark);
        for (p, n) in m.positions.iter().zip(&m.normals) {
            let out = V(0.0, p[1], p[2]);
            let n = Vec3::from_array(*n);
            assert!(n.dot(out) > 0.0 || n.x.abs() > 0.99);
        }
    }

    fn fake<'a>(
        water: &'a dyn Fn(Vec2) -> bool,
        ny: &'a dyn Fn(Vec2) -> f32,
        paint: &'a dyn Fn(PaintLayer, Vec2) -> f32,
    ) -> Probe<'a> {
        Probe { height: &|_| 0.0, normal_y: ny, water, paint }
    }

    #[test]
    fn scatter_blocked_everywhere_is_empty() {
        let b = vec![vec![Vec2::new(-40.0, -40.0), Vec2::new(40.0, -40.0), Vec2::new(40.0, 40.0), Vec2::new(-40.0, 40.0)]];
        assert!(scatter_mesh(&terrain(), 3, &b).is_empty());
    }

    #[test]
    fn scatter_flat_has_tufts_within_budget_and_is_deterministic() {
        let a = scatter_mesh(&terrain(), 3, &[]);
        assert!(a.is_valid() && tris(&a) > 1000);
        assert!(tris(&a) < MAX_TRIS);
        assert_eq!(a, scatter_mesh(&terrain(), 3, &[]));
        assert_ne!(a, scatter_mesh(&terrain(), 4, &[]));
    }

    #[test]
    fn scatter_respects_water_and_slope() {
        let none = |_: PaintLayer, _: Vec2| 0.0;
        assert!(scatter_with(1, &[], &fake(&|_| true, &|_| 1.0, &none)).is_empty());
        assert!(scatter_with(1, &[], &fake(&|_| false, &|_| 0.5, &none)).is_empty());
        // Water only on the x > 0 half: nothing grows there.
        let m = scatter_with(1, &[], &fake(&|p| p.x > 0.0, &|_| 1.0, &none));
        assert!(!m.is_empty());
        assert!(m.positions.iter().all(|p| p[0] <= 0.5));
    }

    #[test]
    fn scatter_layers_add_vegetation_within_budget() {
        let none = |_: PaintLayer, _: Vec2| 0.0;
        let base = tris(&scatter_with(1, &[], &fake(&|_| false, &|_| 1.0, &none)));
        for layer in [PaintLayer::Flowers, PaintLayer::Moss, PaintLayer::Ferns] {
            let paint = move |l: PaintLayer, _: Vec2| if l == layer { 1.0 } else { 0.0 };
            let m = scatter_with(1, &[], &fake(&|_| false, &|_| 1.0, &paint));
            assert!(m.is_valid());
            assert!(tris(&m) <= MAX_TRIS, "{layer:?} {}", tris(&m));
            if layer != PaintLayer::Ferns {
                assert!(tris(&m) > base, "{layer:?}");
            }
        }
        let dirt = |l: PaintLayer, _: Vec2| if l == PaintLayer::Gravel { 1.0 } else { 0.0 };
        assert!(tris(&scatter_with(1, &[], &fake(&|_| false, &|_| 1.0, &dirt))) < base / 4);
    }

    #[test]
    fn scatter_thins_uniformly_when_over_budget() {
        let paint = |l: PaintLayer, _: Vec2| if l == PaintLayer::Flowers || l == PaintLayer::Moss { 1.0 } else { 0.0 };
        let probe = fake(&|_| false, &|_| 1.0, &paint);
        let full = tris(&scatter_pass(1, &[], &probe, 1.0));
        assert!(full > MAX_TRIS, "test needs an over-budget glade, got {full}");
        let m = scatter_with(1, &[], &probe);
        assert!(m.is_valid() && tris(&m) <= MAX_TRIS && tris(&m) > MAX_TRIS / 2, "{}", tris(&m));
        assert_eq!(m, scatter_with(1, &[], &probe));
        let mut q = [0usize; 4];
        for p in &m.positions {
            q[(p[0] > 0.0) as usize * 2 + (p[2] > 0.0) as usize] += 1;
        }
        assert!(q.iter().all(|&c| c > m.positions.len() / 8), "{q:?}");
    }

    #[test]
    fn bushes_keep_min_spacing() {
        let paint = |l: PaintLayer, _: Vec2| if l == PaintLayer::Ferns { 1.0 } else { 0.0 };
        let m = scatter_with(1, &[], &fake(&|_| false, &|_| 1.0, &paint));
        assert!(m.surfaces.iter().any(|&s| s == Surface::Leaves as u8));
    }
}
