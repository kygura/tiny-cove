//! Trees, clutter props and painted ground scatter.
//!
//! Objects are built in a local frame (origin on the ground, +y up), then scaled, rotated by
//! `Placed::angle` in the x,z plane and stood on the terrain. Colours come from the
//! DESIGN.md tokens in `palette.rs`.

use crate::geom;
use crate::mesh::{hash2, Marker, MarkerKind, MeshData, Rng, Surface};
use crate::model::{ClutterKind, Colors, PaintLayer, Placed, PropKind, TreeKind, GLADE_HALF};
use crate::palette;
use crate::terrain::Terrain;
use glam::{Vec2, Vec3};
use std::f32::consts::{PI, TAU};

const V: fn(f32, f32, f32) -> Vec3 = Vec3::new;


// ---------------------------------------------------------------------------------------------
// Small helpers
// ---------------------------------------------------------------------------------------------

fn tint(c: Vec3, rng: &mut Rng, amt: f32) -> Vec3 {
    let k = 1.0 + (rng.f() - 0.5) * amt;
    let h = V(1.0 + (rng.f() - 0.5) * amt * 0.5, 1.0, 1.0 + (rng.f() - 0.5) * amt * 0.5);
    (c * k * h).max(Vec3::ZERO)
}

/// Darken toward indigo-plum instead of grey (shadow sides, undersides, ends).
fn dim(c: Vec3, k: f32) -> Vec3 {
    c * k + V(0.06, 0.02, 0.12) * (1.0 - k)
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

/// Prism along `a`→`b` (logs, poles) with end caps; outward normals.
#[allow(clippy::too_many_arguments)]
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
        let ang = i as f32 / sides as f32 * TAU;
        c + (u * ang.cos() + v * ang.sin()) * r
    };
    for i in 0..sides {
        let (a0, a1, b0, b1) = (ring(i, a), ring(i + 1, a), ring(i, b), ring(i + 1, b));
        m.quad(a0, a1, b1, b0, side_col, s, 1.0);
        m.tri(b, b0, b1, cap_col, s, 1.0);
        m.tri(a, a1, a0, cap_col, s, 1.0);
    }
}

/// A pole or log: a five-sided prism with a slightly darker end cap.
fn log(m: &mut MeshData, a: Vec3, b: Vec3, r: f32, col: Vec3, s: Surface) {
    prism(m, a, b, r, 5, col, dim(col, 0.85), s);
}

#[allow(clippy::too_many_arguments)]
fn cyl(m: &mut MeshData, base: Vec3, r0: f32, r1: f32, h: f32, sides: u32, caps: (bool, bool), col: Vec3, s: Surface) {
    m.cylinder(base, r0, r1, h, sides, caps.0, caps.1, col, s, 1.0);
}

/// Tapered tube along a polyline (bent trunks, branches, floats). `r(i)` is the radius at point
/// `i` and `col(i)` the colour of the segment `i..i+1`. Ends are left open.
fn tube(m: &mut MeshData, pts: &[Vec3], r: &dyn Fn(usize) -> f32, sides: u32, col: &dyn Fn(usize) -> Vec3, s: Surface) {
    if pts.len() < 2 {
        return;
    }
    let frame = |i: usize| {
        let d = (pts[(i + 1).min(pts.len() - 1)] - pts[i.saturating_sub(1)]).normalize_or(Vec3::Y);
        let reference = if d.y.abs() > 0.6 { Vec3::X } else { Vec3::Y };
        let u = (reference - d * reference.dot(d)).normalize_or(Vec3::X);
        (u, d.cross(u))
    };
    let ring = |i: usize, k: u32| {
        let (u, v) = frame(i);
        let a = k as f32 / sides as f32 * TAU;
        pts[i] + (u * a.cos() + v * a.sin()) * r(i)
    };
    for i in 0..pts.len() - 1 {
        for k in 0..sides {
            m.quad(ring(i, k), ring(i, k + 1), ring(i + 1, k + 1), ring(i + 1, k), col(i), s, 1.0);
        }
    }
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

/// Small pointed speck (flower, fruit, shell): a cone with `sides` facets.
fn speck(m: &mut MeshData, at: Vec3, r: f32, h: f32, sides: u32, col: Vec3, s: Surface) {
    cyl(m, at, r, 0.0, h, sides, (false, false), col, s);
}

/// Triangle lying flat, normal forced upwards.
fn up_tri(m: &mut MeshData, a: Vec3, b: Vec3, c: Vec3, col: Vec3, s: Surface) {
    if (b - a).cross(c - a).y >= 0.0 {
        m.tri(a, b, c, col, s, 1.0);
    } else {
        m.tri(a, c, b, col, s, 1.0);
    }
}

/// Flat painted ribbon on a vertical face looking +z (carved spirals, zigzags).
fn face_ribbon(m: &mut MeshData, pts: &[Vec2], z: &dyn Fn(f32) -> f32, w: f32, col: Vec3) {
    for seg in pts.windows(2) {
        let t = (seg[1] - seg[0]).normalize_or(Vec2::X);
        let n = Vec2::new(-t.y, t.x) * w;
        let p = |q: Vec2, k: f32| V(q.x + n.x * k, q.y + n.y * k, z(q.y));
        m.quad(p(seg[0], -1.0), p(seg[1], -1.0), p(seg[1], 1.0), p(seg[0], 1.0), col, Surface::Carving, 1.0);
    }
}

// ---------------------------------------------------------------------------------------------
// Trees
// ---------------------------------------------------------------------------------------------

/// Flat-topped foliage pad (umbrella layer): a wide frustum with an underside.
fn pad(m: &mut MeshData, c: Vec3, r: f32, h: f32, col: Vec3) {
    cyl(m, c, r, r * 0.6, h, 8, (true, true), col, Surface::Leaves);
}

fn sea_pine(m: &mut MeshData, rng: &mut Rng) {
    let bark = tint(palette::bark(), rng, 0.2);
    // Trunk bent downwind (+x): the wind-sculpted lean of a coastal pine.
    let lean = rng.range(0.8, 1.3);
    let pts: Vec<Vec3> = (0..=6)
        .map(|i| {
            let t = i as f32 / 6.0;
            V(lean * 1.2 * t * t, 3.4 * t, 0.18 * (t * 5.0).sin())
        })
        .collect();
    tube(m, &pts, &|i| 0.30 - 0.19 * (i as f32 / 6.0), 6, &|i| if i % 2 == 0 { bark } else { palette::bark_hi() }, Surface::Bark);
    let top = pts[6];
    // Branches reaching the lower pads.
    let low = V(top.x * 0.35 - 0.7, 2.2, 0.3);
    let side = V(top.x + 0.9, 3.0, -0.4);
    log(m, pts[3], low, 0.07, bark, Surface::Bark);
    log(m, pts[4], side, 0.07, bark, Surface::Bark);
    let pine = palette::leaf_sea_pine();
    // Three umbrella layers, lopsided downwind, each lit a little brighter than the one below.
    pad(m, low + V(0.0, 0.0, 0.0), 1.15, 0.36, tint(dim(pine, 0.85), rng, 0.2));
    pad(m, side + V(0.0, 0.0, 0.0), 1.2, 0.38, tint(pine, rng, 0.2));
    pad(m, top + V(0.2, 0.0, 0.0), 1.75, 0.42, tint(pine.lerp(palette::grass_tip(), 0.12), rng, 0.2));
}

fn palm(m: &mut MeshData, rng: &mut Rng) {
    let trunk = tint(palette::palm_trunk(), rng, 0.2);
    let lean = rng.range(0.9, 1.4);
    let n = 9usize;
    let pts: Vec<Vec3> = (0..=n)
        .map(|i| {
            let t = i as f32 / n as f32;
            V(lean * t.powf(1.8), 3.8 * t, 0.12 * (t * 3.0).sin())
        })
        .collect();
    // Alternating fat and thin segments read as the palm's growth rings.
    let ringed = |i: usize| (0.21 - 0.09 * (i as f32 / n as f32)) * if i % 2 == 0 { 1.0 } else { 0.86 };
    tube(m, &pts, &ringed, 6, &|i| if i % 2 == 0 { trunk } else { dim(trunk, 0.82) }, Surface::Bark);
    let crown = pts[n] - V(0.0, 0.05, 0.0);
    let leaf = palette::leaf_palm();
    let k = 9 + (rng.f() * 2.0) as usize;
    let y0 = rng.f() * TAU;
    for i in 0..k {
        let yaw = y0 + i as f32 / k as f32 * TAU + rng.range(-0.12, 0.12);
        let col = tint(leaf, rng, 0.2);
        frond(m, crown, yaw, rng.range(1.8, 2.3), rng.range(1.0, 1.4), 2.1, 0.2, dim(col, 0.85), col * 1.08, Surface::Leaves);
    }
    // Young upright fronds in the middle of the crown.
    for i in 0..2 {
        let yaw = y0 + i as f32 * PI;
        frond(m, crown, yaw, 0.9, 1.1, 0.35, 0.12, leaf, palette::grass_tip(), Surface::Leaves);
    }
    // Coconuts under the crown.
    for i in 0..3 {
        let a = y0 + i as f32 * TAU / 3.0;
        let c = crown + V(a.cos() * 0.17, -0.2, a.sin() * 0.17);
        blob(m, c, Vec3::splat(0.12), 0.1, tint(palette::bark(), rng, 0.2), Surface::Bark, rng, None);
    }
}

fn cypress(m: &mut MeshData, rng: &mut Rng) {
    let bark = palette::bark();
    cyl(m, Vec3::ZERO, 0.16, 0.1, 0.7, 6, (false, false), bark, Surface::Bark);
    // Six stacked cones: fat in the lower third, narrowing to a flame tip that sways a little.
    const R: [f32; 6] = [0.95, 1.0, 0.86, 0.68, 0.47, 0.28];
    let sway = rng.range(-0.15, 0.15);
    for (k, r) in R.iter().enumerate() {
        let t = k as f32 / 5.0;
        let col = tint(palette::leaf_cypress(), rng, 0.15).lerp(palette::leaf_palm(), 0.1 * t);
        cyl(m, V(sway * t * t, 0.45 + 0.88 * k as f32, 0.0), *r, 0.0, 1.45, 7, (false, true), col, Surface::Leaves);
    }
}

fn fig(m: &mut MeshData, rng: &mut Rng) {
    let bark = tint(palette::bark(), rng, 0.2);
    let pts = [V(0.0, 0.0, 0.0), V(0.05, 0.6, 0.0), V(0.0, 1.2, 0.05)];
    tube(m, &pts, &|i| 0.3 - 0.07 * i as f32, 6, &|_| bark, Surface::Bark);
    // Multi-lobe canopy: a crown lobe and four around it, wide rather than tall.
    let leaf = palette::leaf_fig();
    let mut lobes = vec![(V(0.0, 2.55, 0.0), 1.25)];
    let a0 = rng.f() * TAU;
    for i in 0..4 {
        let a = a0 + i as f32 * TAU / 4.0;
        let r = rng.range(0.85, 1.05);
        lobes.push((V(a.cos() * 1.3, 2.05 + rng.f() * 0.3, a.sin() * 1.3), r));
        log(m, V(0.0, 1.1, 0.0), V(a.cos() * 0.9, 1.9, a.sin() * 0.9), 0.07, bark, Surface::Bark);
    }
    for &(c, r) in &lobes {
        blob(m, c, V(r, r * 0.78, r), 0.14, tint(leaf, rng, 0.25), Surface::Leaves, rng, None);
    }
    // Purple fruit dots hang from the lobes.
    for _ in 0..7 {
        let (c, r) = lobes[(rng.next_u64() % lobes.len() as u64) as usize];
        let (th, ph) = (rng.f() * TAU, rng.range(0.6, 1.9));
        let d = V(th.cos() * ph.sin(), ph.cos(), th.sin() * ph.sin());
        let p = c + d * V(r, r * 0.78, r) * 0.98;
        m.aabb(p - Vec3::splat(0.07), p + Vec3::splat(0.07), palette::fig_fruit(), Surface::Petal, 1.0);
    }
}

fn snag(m: &mut MeshData, rng: &mut Rng) {
    // Bleached driftwood: lilac-grained cream, twisted trunk and bare branches.
    let wood = tint(palette::snag(), rng, 0.15);
    let grain = palette::grain().lerp(wood, 0.5);
    let n = 8usize;
    let (ph, amp) = (rng.f() * TAU, rng.range(0.18, 0.3));
    let pts: Vec<Vec3> = (0..=n)
        .map(|i| {
            let t = i as f32 / n as f32;
            V(amp * (t * 5.0 + ph).sin(), 3.7 * t, amp * (t * 4.0 + ph).cos() - amp)
        })
        .collect();
    tube(m, &pts, &|i| 0.30 - 0.22 * (i as f32 / n as f32), 6, &|i| if i % 2 == 0 { wood } else { grain }, Surface::Bark);
    // Splayed roots.
    for i in 0..3 {
        let a = i as f32 * TAU / 3.0 + ph;
        log(m, V(0.0, 0.35, 0.0), V(a.cos() * 0.55, -0.05, a.sin() * 0.55), 0.09, grain, Surface::Bark);
    }
    // Twisted branches, thinning outwards.
    let k = 4 + (rng.f() * 2.0) as usize;
    for i in 0..k {
        let t = 0.42 + 0.5 * i as f32 / k as f32;
        let o = pts[(t * n as f32) as usize];
        let a = ph + i as f32 * 2.4 + rng.range(-0.3, 0.3);
        let (dx, dz) = (a.cos(), a.sin());
        let len = rng.range(0.9, 1.5) * (1.1 - 0.5 * t);
        let bend = rng.range(0.3, 0.7);
        let b = [o, o + V(dx * len * 0.5, len * 0.35, dz * len * 0.5), o + V(dx * len, len * 0.35 + bend, dz * len)];
        tube(m, &b, &|j| 0.1 - 0.045 * j as f32, 4, &|_| wood, Surface::Bark);
        // A short twig at the elbow.
        log(m, b[1], b[1] + V(-dz * 0.35, 0.3, dx * 0.35), 0.03, grain, Surface::Bark);
    }
}

fn build_tree(kind: TreeKind, rng: &mut Rng) -> (MeshData, Vec<Marker>) {
    let mut m = MeshData::default();
    match kind {
        TreeKind::SeaPine => sea_pine(&mut m, rng),
        TreeKind::Palm => palm(&mut m, rng),
        TreeKind::Cypress => cypress(&mut m, rng),
        TreeKind::Fig => fig(&mut m, rng),
        TreeKind::Snag => snag(&mut m, rng),
    }
    // Gulls perch on the highest point of the tree.
    let top = m.positions.iter().map(|p| Vec3::from_array(*p)).max_by(|a, b| a.y.total_cmp(&b.y)).unwrap_or(Vec3::ZERO);
    (m, vec![Marker { kind: MarkerKind::Perch, pos: top, dir: Vec3::Y }])
}

// ---------------------------------------------------------------------------------------------
// Clutter
// ---------------------------------------------------------------------------------------------

/// Outrigger canoe on the +x axis (bow at +x): a long planked hull, a float on two booms to +z
/// and a painted bow band.
fn canoe(m: &mut MeshData, rng: &mut Rng, trim: Vec3) {
    const N: usize = 8;
    const L: f32 = 1.75;
    let hull = tint(palette::wood(), rng, 0.15);
    let inner = tint(palette::timber().lerp(palette::rope(), 0.35), rng, 0.1);
    let half = |x: f32| 0.02 + 0.33 * (1.0 - (x / L).abs().powf(2.2)).max(0.0).sqrt();
    let gunwale = |x: f32| 0.36 + 0.38 * (x / L).abs().powi(3);
    let keel = |x: f32| 0.06 + 0.3 * (x / L).abs().powi(3);
    let st: Vec<f32> = (0..=N).map(|i| -L + 2.0 * L * i as f32 / N as f32).collect();
    for i in 0..N {
        let (x0, x1) = (st[i], st[i + 1]);
        let bow = i >= N - 2;
        let outer = if bow { trim } else { hull };
        let surf = if bow { Surface::Carving } else { Surface::Plank };
        let p = |x: f32, y: f32, z: f32| V(x, y, z);
        let (w0, w1) = (half(x0), half(x1));
        let (iw0, iw1) = ((w0 - 0.04).max(0.0), (w1 - 0.04).max(0.0));
        let (g0, g1, k0, k1) = (gunwale(x0), gunwale(x1), keel(x0), keel(x1));
        // Outer planking (left = -z, right = +z).
        m.quad(p(x1, k1, 0.0), p(x0, k0, 0.0), p(x0, g0, -w0), p(x1, g1, -w1), outer, surf, 1.0);
        m.quad(p(x0, k0, 0.0), p(x1, k1, 0.0), p(x1, g1, w1), p(x0, g0, w0), outer, surf, 1.0);
        // Inner hollow, a little lighter so it reads from above.
        m.quad(p(x0, k0 + 0.05, 0.0), p(x1, k1 + 0.05, 0.0), p(x1, g1 - 0.02, -iw1), p(x0, g0 - 0.02, -iw0), inner, Surface::Boards, 1.0);
        m.quad(p(x1, k1 + 0.05, 0.0), p(x0, k0 + 0.05, 0.0), p(x0, g0 - 0.02, iw0), p(x1, g1 - 0.02, iw1), inner, Surface::Boards, 1.0);
        // Gunwale rims.
        m.quad(p(x1, g1, -w1), p(x0, g0, -w0), p(x0, g0 - 0.02, -iw0), p(x1, g1 - 0.02, -iw1), palette::timber(), Surface::Timber, 1.0);
        m.quad(p(x0, g0, w0), p(x1, g1, w1), p(x1, g1 - 0.02, iw1), p(x0, g0 - 0.02, iw0), palette::timber(), Surface::Timber, 1.0);
    }
    // Carved prow post with a painted tip, thwarts, a bundle of paddles.
    let tipx = L + 0.02;
    log(m, V(tipx - 0.1, gunwale(L) - 0.05, 0.0), V(tipx + 0.1, gunwale(L) + 0.32, 0.0), 0.05, trim, Surface::Carving);
    for x in [-0.7f32, 0.0, 0.7] {
        let w = half(x) - 0.02;
        m.beam(V(x, gunwale(x) - 0.06, -w), V(x, gunwale(x) - 0.06, w), 0.1, 0.05, palette::timber(), Surface::Timber, 1.0);
    }
    // Outrigger: spindle float on +z, joined to the hull by two booms.
    let fz = 1.35;
    let float: Vec<Vec3> = [-1.15f32, -0.7, 0.0, 0.7, 1.15].iter().map(|&x| V(x, 0.14 + 0.05 * (x / 1.15f32).abs(), fz)).collect();
    let radii = [0.02f32, 0.09, 0.1, 0.09, 0.02];
    tube(m, &float, &|i| radii[i], 6, &|_| inner, Surface::Timber);
    for x in [-0.55f32, 0.55] {
        log(m, V(x, gunwale(x) - 0.02, half(x) - 0.03), V(x, 0.22, fz), 0.035, palette::timber(), Surface::Timber);
    }
}

/// A fish hanging by the head from `top`: a diamond body and a split tail, silver-cyan.
fn fish(m: &mut MeshData, top: Vec3, len: f32, yaw: f32, col: Vec3) {
    let dx = V(yaw.cos(), 0.0, yaw.sin());
    let w = len * 0.2;
    let p = |x: f32, y: f32| top + dx * x + Vec3::Y * y;
    quad2(m, p(0.0, 0.0), p(-w, -len * 0.4), p(0.0, -len * 0.85), p(w, -len * 0.4), col, Surface::Petal);
    tri2(m, p(0.0, -len * 0.8), p(-w * 1.1, -len * 1.1), p(w * 1.1, -len * 1.1), dim(col, 0.85), Surface::Petal);
}

fn drying_rack(m: &mut MeshData, rng: &mut Rng, trim: Vec3) {
    let pole = tint(palette::driftwood(), rng, 0.15);
    for sx in [-1.5f32, 1.5] {
        // A-frame: two legs meeting at the ridge, lashed in trim colour.
        log(m, V(sx, 0.0, -0.55), V(sx, 1.9, 0.0), 0.06, pole, Surface::Timber);
        log(m, V(sx, 0.0, 0.55), V(sx, 1.9, 0.0), 0.06, pole, Surface::Timber);
        m.aabb(V(sx - 0.08, 1.7, -0.08), V(sx + 0.08, 1.78, 0.08), trim, Surface::Carving, 1.0);
    }
    log(m, V(-1.85, 1.9, 0.0), V(1.85, 1.9, 0.0), 0.05, pole, Surface::Timber);
    // Lower crossbars on both slopes, where legs are at z = ±0.246.
    for sz in [-1.0f32, 1.0] {
        log(m, V(-1.5, 1.05, sz * 0.246), V(1.5, 1.05, sz * 0.246), 0.04, pole, Surface::Timber);
    }
    // Silver fish hang from the ridge and the bars.
    for i in 0..9usize {
        let x = -1.3 + i as f32 * 0.325;
        let (top, len) = match i % 3 {
            0 => (V(x, 1.88, 0.0), rng.range(0.55, 0.7)),
            1 => (V(x, 1.03, 0.246), rng.range(0.45, 0.6)),
            _ => (V(x, 1.03, -0.246), rng.range(0.45, 0.6)),
        };
        let col = if i % 2 == 0 { palette::fish() } else { palette::shell() };
        fish(m, top, len, rng.range(-0.5, 0.5), col * rng.range(0.92, 1.0));
    }
}

fn firepit(m: &mut MeshData, rng: &mut Rng) -> Vec<Marker> {
    // Ring of eight stones, every third lilac slate.
    for i in 0..8usize {
        let a = i as f32 / 8.0 * TAU;
        let col = match i % 3 {
            2 => palette::stone_alt(),
            0 => palette::rock(),
            _ => palette::dry_stone(),
        };
        blob(m, V(a.cos() * 0.62, 0.13, a.sin() * 0.62), V(0.2, 0.15, 0.2), 0.22, tint(col, rng, 0.15), Surface::Rock, rng, Some(0.0));
    }
    // Ember bed and crossed logs.
    cyl(m, V(0.0, 0.0, 0.0), 0.5, 0.45, 0.07, 8, (true, false), palette::ember(), Surface::Dirt);
    let wood = palette::driftwood();
    for i in 0..3usize {
        let a = i as f32 * PI / 3.0 + rng.range(-0.15, 0.15);
        let d = V(a.cos(), 0.0, a.sin());
        log(m, d * -0.5 + V(0.0, 0.1, 0.0), d * 0.5 + V(0.0, 0.3 + 0.03 * i as f32, 0.0), 0.075, tint(wood, rng, 0.15), Surface::Bark);
    }
    vec![Marker { kind: MarkerKind::Fire, pos: V(0.0, 0.4, 0.0), dir: Vec3::Y }]
}

fn cairn(m: &mut MeshData, rng: &mut Rng) {
    // 5-9 rounded stones, tapering as they climb.
    let n = 5 + (rng.next_u64() % 5) as usize;
    let cols = [palette::rock(), palette::stone_alt(), palette::dry_stone(), palette::pebble_rose(), palette::pebble_lilac()];
    let mut y = 0.0f32;
    let shift = rng.f() * TAU;
    for k in 0..n {
        let t = k as f32 / n as f32;
        let r = 0.45 * (1.0 - 0.72 * t) * rng.range(0.9, 1.1);
        let ry = r * 0.5;
        let off = if k == 0 { Vec2::ZERO } else { Vec2::from_angle(shift + k as f32 * 2.1) * 0.05 };
        let floor = if k == 0 { Some(0.0) } else { None };
        let c = V(off.x, y + ry * if k == 0 { 0.9 } else { 0.85 }, off.y);
        blob(m, c, V(r, ry, r * rng.range(0.85, 1.0)), 0.12, tint(cols[(k + (shift * 3.0) as usize) % cols.len()], rng, 0.15), Surface::Rock, rng, floor);
        y += ry * 1.3;
    }
}

fn standing_stone(m: &mut MeshData, rng: &mut Rng, trim: Vec3) {
    let stone = tint(palette::rock(), rng, 0.15);
    let (wb, db, wt, dt, h) = (0.5f32, 0.22f32, 0.36f32, 0.17f32, rng.range(2.5, 2.8));
    let tops = [h, h * 0.94, h * 0.9, h * 0.97]; // (-,+) (+,+) (+,-) (-,-)
    let b = |sx: f32, sz: f32| V(sx * wb, 0.0, sz * db);
    let t = |sx: f32, sz: f32, y: f32| V(sx * wt, y, sz * dt);
    m.quad(b(-1.0, 1.0), b(1.0, 1.0), t(1.0, 1.0, tops[1]), t(-1.0, 1.0, tops[0]), stone, Surface::Rock, 0.8);
    m.quad(b(1.0, -1.0), b(-1.0, -1.0), t(-1.0, -1.0, tops[3]), t(1.0, -1.0, tops[2]), dim(stone, 0.9), Surface::Rock, 0.8);
    m.quad(b(1.0, 1.0), b(1.0, -1.0), t(1.0, -1.0, tops[2]), t(1.0, 1.0, tops[1]), stone, Surface::Rock, 0.8);
    m.quad(b(-1.0, -1.0), b(-1.0, 1.0), t(-1.0, 1.0, tops[0]), t(-1.0, -1.0, tops[3]), dim(stone, 0.9), Surface::Rock, 0.8);
    m.quad(t(-1.0, -1.0, tops[3]), t(-1.0, 1.0, tops[0]), t(1.0, 1.0, tops[1]), t(1.0, -1.0, tops[2]), stone * 1.05, Surface::Rock, 0.8);
    // Wedge stones at the foot.
    blob(m, V(0.4, 0.1, 0.25), V(0.28, 0.2, 0.22), 0.2, tint(palette::dry_stone(), rng, 0.15), Surface::Rock, rng, Some(0.0));
    blob(m, V(-0.35, 0.09, -0.2), V(0.22, 0.17, 0.2), 0.2, tint(palette::stone_alt(), rng, 0.15), Surface::Rock, rng, Some(0.0));
    // Carved spiral (about 1.5 turns, outwards) and a zigzag below, painted in pigment.
    let zf = move |y: f32| db + (dt - db) * (y / h) + 0.012;
    let (cx, cy) = (0.0, h * 0.62);
    let spiral: Vec<Vec2> = (0..=16)
        .map(|i| {
            let (a, r) = (i as f32 * 0.55, 0.03 + 0.2 * i as f32 / 16.0);
            Vec2::new(cx + r * a.cos(), cy + r * a.sin())
        })
        .collect();
    face_ribbon(m, &spiral, &zf, 0.022, trim);
    let zig: Vec<Vec2> = (0..=6).map(|i| Vec2::new(-0.21 + 0.07 * i as f32, h * 0.3 + if i % 2 == 0 { 0.0 } else { 0.09 })).collect();
    face_ribbon(m, &zig, &zf, 0.018, trim);
}

fn carved_pole(m: &mut MeshData, rng: &mut Rng, colors: Colors) {
    let c0 = colors.trim as u32;
    let band = |i: u32| palette::pigment(c0 + i);
    // Footing stones.
    blob(m, V(0.0, 0.12, 0.0), V(0.42, 0.18, 0.42), 0.18, tint(palette::dry_stone(), rng, 0.15), Surface::Rock, rng, Some(0.0));
    // Six painted bands, alternately fat and slim.
    let bh = 0.55f32;
    for k in 0..6u32 {
        let y = 0.2 + k as f32 * bh;
        let r = if k % 2 == 0 { 0.2 } else { 0.17 };
        cyl(m, V(0.0, y, 0.0), r, r, bh, 6, (false, false), band(k), Surface::Carving);
        // Carved faces on bands 1 and 4: brow, nose and mouth in a second pigment.
        if k == 1 || k == 4 {
            let f = band(k + 2);
            m.aabb(V(-0.14, y + 0.34, r - 0.02), V(0.14, y + 0.4, r + 0.07), f, Surface::Carving, 1.0);
            m.aabb(V(-0.04, y + 0.15, r - 0.02), V(0.04, y + 0.33, r + 0.09), f, Surface::Carving, 1.0);
            m.aabb(V(-0.1, y + 0.05, r - 0.02), V(0.1, y + 0.1, r + 0.05), f, Surface::Carving, 1.0);
        }
    }
    // Bird finial: body, head, beak, swept wings and tail, all painted.
    let y = 0.2 + 6.0 * bh + 0.12;
    cyl(m, V(0.0, y - 0.12, 0.0), 0.14, 0.1, 0.12, 6, (true, false), band(1), Surface::Carving);
    prism(m, V(-0.28, y + 0.05, 0.0), V(0.24, y + 0.1, 0.0), 0.13, 6, band(0), band(0), Surface::Carving);
    blob(m, V(0.32, y + 0.2, 0.0), Vec3::splat(0.11), 0.1, band(0), Surface::Carving, rng, None);
    tri2(m, V(0.4, y + 0.24, 0.05), V(0.4, y + 0.24, -0.05), V(0.58, y + 0.18, 0.0), band(3), Surface::Carving);
    for sz in [-1.0f32, 1.0] {
        let w = [V(0.12, y + 0.14, sz * 0.1), V(-0.2, y + 0.14, sz * 0.12), V(-0.5, y + 0.3, sz * 0.85), V(0.05, y + 0.32, sz * 0.8)];
        quad2(m, w[0], w[1], w[2], w[3], band(2), Surface::Carving);
        tri2(m, w[2], w[3], V(-0.2, y + 0.4, sz * 1.05), band(3), Surface::Carving);
    }
    tri2(m, V(-0.26, y + 0.08, 0.1), V(-0.26, y + 0.08, -0.1), V(-0.62, y + 0.2, 0.0), band(4), Surface::Carving);
}

fn net_rack(m: &mut MeshData, rng: &mut Rng, trim: Vec3) {
    let pole = tint(palette::driftwood(), rng, 0.15);
    for sx in [-1.4f32, 1.4] {
        log(m, V(sx, 0.0, 0.0), V(sx * 1.02, 2.0, 0.0), 0.07, pole, Surface::Timber);
        log(m, V(sx * 1.35, 0.0, 0.0), V(sx, 1.4, 0.0), 0.04, pole, Surface::Timber);
    }
    log(m, V(-1.5, 1.95, 0.0), V(1.5, 1.95, 0.0), 0.05, pole, Surface::Timber);
    // The net drapes in a sagging belly from the top rope: 6 x 2 cells.
    const C: usize = 6;
    let net = palette::rope();
    let at = |i: usize, j: usize| {
        let u = i as f32 / C as f32;
        let sag = (u * PI).sin();
        let y = match j {
            0 => 1.9,
            1 => 1.25 - 0.08 * sag,
            _ => 0.55 + 0.3 * (1.0 - sag) + 0.1 * (i % 2) as f32,
        };
        V(-1.35 + 2.7 * u, y, 0.05 + 0.2 * sag * j as f32 * 0.5)
    };
    for j in 0..2 {
        for i in 0..C {
            quad2(m, at(i, j), at(i + 1, j), at(i + 1, j + 1), at(i, j + 1), net, Surface::Fiber);
        }
    }
    // Bright floats along the head rope and a few on the foot.
    for i in 0..8usize {
        let x = -1.2 + i as f32 * (2.4 / 7.0);
        let col = if i % 2 == 0 { trim } else { palette::pigment(i as u32 / 2 + 3) };
        cyl(m, V(x, 1.9, 0.02), 0.08, 0.06, 0.14, 5, (true, false), col, Surface::Carving);
    }
    blob(m, V(0.2, 0.1, 0.55), V(0.55, 0.12, 0.3), 0.2, net * 0.95, Surface::Fiber, rng, Some(0.0));
}

fn baskets(m: &mut MeshData, rng: &mut Rng, trim: Vec3) {
    let reed = palette::reed();
    // Woven baskets: flaring frusta with a dark open mouth and a trim-coloured rim band.
    for &(x, z, r, h) in &[(-0.35f32, 0.0f32, 0.34f32, 0.5f32), (0.28, 0.32, 0.26, 0.38)] {
        let col = tint(reed, rng, 0.15);
        cyl(m, V(x, 0.0, z), r * 0.8, r, h, 8, (false, false), col, Surface::Fiber);
        cyl(m, V(x, h - 0.04, z), r * 1.02, r * 1.02, 0.07, 8, (false, false), trim, Surface::Carving);
        cyl(m, V(x, h - 0.01, z), r * 0.9, r * 0.9, 0.0, 8, (true, false), dim(col, 0.55), Surface::Fiber);
    }
    // Clay jars: belly, shoulder, neck, with a painted band.
    for &(x, z, s) in &[(0.45f32, -0.3f32, 1.0f32), (-0.05, -0.5, 0.8), (0.0, 0.6, 0.65)] {
        let clay = tint(palette::clay(), rng, 0.15);
        cyl(m, V(x, 0.0, z), 0.13 * s, 0.26 * s, 0.22 * s, 7, (false, false), clay, Surface::Dirt);
        cyl(m, V(x, 0.22 * s, z), 0.26 * s, 0.17 * s, 0.2 * s, 7, (false, false), clay, Surface::Dirt);
        cyl(m, V(x, 0.42 * s, z), 0.1 * s, 0.1 * s, 0.14 * s, 7, (false, false), dim(clay, 0.92), Surface::Dirt);
        cyl(m, V(x, 0.56 * s, z), 0.14 * s, 0.14 * s, 0.03 * s, 7, (true, false), clay, Surface::Dirt);
        cyl(m, V(x, 0.2 * s, z), 0.265 * s, 0.255 * s, 0.05 * s, 7, (false, false), trim, Surface::Carving);
    }
}

fn lantern_post(m: &mut MeshData, rng: &mut Rng, trim: Vec3) -> Vec<Marker> {
    let post = tint(palette::driftwood(), rng, 0.12);
    blob(m, V(0.0, 0.12, 0.0), V(0.3, 0.16, 0.3), 0.18, tint(palette::dry_stone(), rng, 0.15), Surface::Rock, rng, Some(0.0));
    cyl(m, V(0.0, 0.1, 0.0), 0.1, 0.06, 2.3, 6, (true, false), post, Surface::Timber);
    cyl(m, V(0.0, 0.7, 0.0), 0.115, 0.115, 0.08, 6, (false, false), trim, Surface::Carving);
    // Crossarm with a diagonal brace, carved tip.
    log(m, V(0.0, 2.3, 0.0), V(0.62, 2.3, 0.0), 0.05, post, Surface::Timber);
    log(m, V(0.0, 1.95, 0.0), V(0.42, 2.27, 0.0), 0.03, post, Surface::Timber);
    m.aabb(V(0.6, 2.26, -0.04), V(0.68, 2.34, 0.04), trim, Surface::Carving, 1.0);
    // Hanging lantern: cord, iron cap and base, glowing glass.
    let iron = palette::metal();
    m.aabb(V(0.6 - 0.01, 2.14, -0.01), V(0.62 + 0.01, 2.3, 0.01), palette::rope(), Surface::Fiber, 1.0);
    m.aabb(V(0.5, 2.1, -0.12), V(0.72, 2.15, 0.12), iron, Surface::Metal, 1.0);
    m.aabb(V(0.5, 1.8, -0.12), V(0.72, 1.85, 0.12), iron, Surface::Metal, 1.0);
    m.aabb(V(0.53, 1.85, -0.09), V(0.69, 2.1, 0.09), palette::glow(), Surface::Glow, 1.0);
    vec![Marker { kind: MarkerKind::Lantern, pos: V(0.61, 1.97, 0.0), dir: Vec3::Y }]
}

fn pennant(m: &mut MeshData, rng: &mut Rng, colors: Colors) -> Vec<Marker> {
    let pole = tint(palette::driftwood(), rng, 0.12);
    let trim = palette::trim(colors);
    cyl(m, Vec3::ZERO, 0.13, 0.06, 5.5, 6, (true, false), pole, Surface::Timber);
    for y in [0.5f32, 1.4, 4.2] {
        cyl(m, V(0.0, y, 0.0), 0.14 - 0.012 * y, 0.14 - 0.012 * y, 0.07, 6, (false, false), palette::rope(), Surface::Fiber);
    }
    speck(m, V(0.0, 5.5, 0.0), 0.08, 0.3, 5, trim, Surface::Carving);
    // Long streaming pennant in sailcloth with bands of pigment, tapering to a swallow tail.
    let top = 5.25f32;
    let seg = 7;
    let len = 2.9;
    let wave = |t: f32| V(0.1 + len * t, 0.0, 0.2 * (t * 7.0).sin() * t);
    let band = |i: usize| if i % 2 == 0 { palette::sailcloth() } else { palette::pigment(colors.trim as u32 + (i as u32 / 2)) };
    for i in 0..seg {
        let (t0, t1) = (i as f32 / seg as f32, (i + 1) as f32 / seg as f32);
        let (p0, p1) = (wave(t0), wave(t1));
        let (h0, h1) = (0.62 * (1.0 - 0.9 * t0), 0.62 * (1.0 - 0.9 * t1));
        let (y0, y1) = (top - 0.05 * t0 * 4.0, top - 0.05 * t1 * 4.0);
        quad2(m, p0 + V(0.0, y0, 0.0), p1 + V(0.0, y1, 0.0), p1 + V(0.0, y1 - h1, 0.0), p0 + V(0.0, y0 - h0, 0.0), band(i), Surface::Hide);
    }
    let end = wave(1.0);
    tri2(m, end + V(0.0, top - 0.2 - 0.06, 0.0), end + V(0.0, top - 0.06, 0.0), end + V(0.25, top - 0.06 - 0.03, 0.0), trim, Surface::Hide);
    vec![Marker { kind: MarkerKind::Flag, pos: V(0.1, top, 0.0), dir: Vec3::X }]
}

fn rock(m: &mut MeshData, rng: &mut Rng) {
    let c = tint(palette::rock(), rng, 0.2);
    blob(m, V(0.0, 0.34, 0.0), V(1.0, 0.7, 0.85), 0.28, c, Surface::Rock, rng, Some(0.0));
    if rng.f() < 0.6 {
        blob(m, V(0.9, 0.17, 0.35), V(0.48, 0.34, 0.42), 0.25, tint(palette::stone_alt().lerp(c, 0.6), rng, 0.15), Surface::Rock, rng, Some(0.0));
    }
    if rng.f() < 0.5 {
        blob(m, V(-0.7, 0.14, -0.55), V(0.38, 0.28, 0.34), 0.25, c * 0.97, Surface::Rock, rng, Some(0.0));
    }
}

fn bush(m: &mut MeshData, rng: &mut Rng, trim: Vec3) {
    // Flowering gorse: green lumps studded with saffron (and the odd flax or trim) flower specks.
    let n = 3 + (rng.f() * 2.0) as usize;
    let leaf = palette::leaf_fig();
    for _ in 0..n {
        let c = V(rng.range(-0.4, 0.4), 0.0, rng.range(-0.4, 0.4));
        let r = rng.range(0.45, 0.62);
        let ry = r * 0.8;
        let cc = c + V(0.0, ry * 0.8, 0.0);
        blob(m, cc, V(r, ry, r), 0.18, tint(dim(leaf, 0.9), rng, 0.3), Surface::Leaves, rng, Some(0.02));
        for _ in 0..4 {
            let (th, ph) = (rng.f() * TAU, rng.range(0.2, 1.2));
            let d = V(th.cos() * ph.sin(), ph.cos(), th.sin() * ph.sin());
            let col = match rng.next_u64() % 8 {
                0 => palette::flax(),
                1 => trim,
                _ => palette::gorse(),
            };
            speck(m, cc + d * V(r, ry, r) * 0.92, 0.075, 0.09, 4, col, Surface::Petal);
        }
    }
}

fn crab_pots(m: &mut MeshData, rng: &mut Rng, trim: Vec3) {
    let reed = palette::reed();
    let mut pot = |m: &mut MeshData, c: Vec3| {
        let col = tint(reed, rng, 0.15);
        cyl(m, c, 0.4, 0.3, 0.5, 8, (false, true), col, Surface::Fiber);
        // Timber hoops and the funnel mouth.
        cyl(m, c + V(0.0, 0.04, 0.0), 0.405, 0.405, 0.05, 8, (false, false), palette::timber(), Surface::Timber);
        cyl(m, c + V(0.0, 0.4, 0.0), 0.325, 0.31, 0.05, 8, (false, false), palette::timber(), Surface::Timber);
        cyl(m, c + V(0.0, 0.5, 0.0), 0.18, 0.18, 0.0, 6, (true, false), dim(col, 0.5), Surface::Fiber);
    };
    pot(m, V(-0.42, 0.0, 0.0));
    pot(m, V(0.42, 0.0, 0.1));
    pot(m, V(0.0, 0.5, 0.04));
    // Rope from the top pot to a trim-coloured buoy, clay weight on the ground.
    let rope = palette::rope();
    m.beam(V(0.1, 0.95, 0.05), V(0.65, 0.5, 0.65), 0.04, 0.04, rope, Surface::Fiber, 1.0);
    m.beam(V(0.65, 0.5, 0.65), V(0.75, 0.16, 0.95), 0.04, 0.04, rope, Surface::Fiber, 1.0);
    cyl(m, V(0.75, 0.0, 0.95), 0.12, 0.1, 0.2, 6, (true, false), trim, Surface::Carving);
    cyl(m, V(-0.2, 0.0, -0.55), 0.1, 0.08, 0.1, 6, (true, false), palette::clay(), Surface::Dirt);
}

fn woodpile(m: &mut MeshData, rng: &mut Rng) {
    // Driftwood stack: 4 + 3 + 2 + 1 logs, lilac-grained ends.
    let rows: [(&[f32], f32); 4] = [(&[-0.45, -0.15, 0.15, 0.45], 0.14), (&[-0.3, 0.0, 0.3], 0.38), (&[-0.15, 0.15], 0.61), (&[0.0], 0.83)];
    for (zs, y) in rows {
        for &z in zs {
            let len = rng.range(0.5, 0.65);
            let sh = rng.range(-0.1, 0.1);
            let col = tint(palette::driftwood(), rng, 0.2);
            prism(m, V(-len + sh, y, z), V(len + sh, y, z), 0.14 + rng.range(-0.02, 0.01), 5, col, palette::grain(), Surface::Bark);
        }
    }
}

fn build_clutter(kind: ClutterKind, rng: &mut Rng, colors: Colors) -> (MeshData, Vec<Marker>) {
    let mut m = MeshData::default();
    let mut markers = Vec::new();
    let trim = palette::trim(colors);
    match kind {
        ClutterKind::Canoe => canoe(&mut m, rng, trim),
        ClutterKind::DryingRack => drying_rack(&mut m, rng, trim),
        ClutterKind::Firepit => markers = firepit(&mut m, rng),
        ClutterKind::Cairn => cairn(&mut m, rng),
        ClutterKind::StandingStone => standing_stone(&mut m, rng, trim),
        ClutterKind::CarvedPole => carved_pole(&mut m, rng, colors),
        ClutterKind::NetRack => net_rack(&mut m, rng, trim),
        ClutterKind::Baskets => baskets(&mut m, rng, trim),
        ClutterKind::LanternPost => markers = lantern_post(&mut m, rng, trim),
        ClutterKind::Pennant => markers = pennant(&mut m, rng, colors),
        ClutterKind::Rock => rock(&mut m, rng),
        ClutterKind::Bush => bush(&mut m, rng, trim),
        ClutterKind::CrabPots => crab_pots(&mut m, rng, trim),
        ClutterKind::Woodpile => woodpile(&mut m, rng),
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

    // A canoe on water floats: the Bevy side spawns it as a bobbing entity.
    if placed.kind == PropKind::Clutter(ClutterKind::Canoe) {
        if let Some(level) = terrain.water_level(at).filter(|l| l.is_finite()) {
            let d = Vec2::from_angle(angle);
            return (MeshData::default(), vec![Marker { kind: MarkerKind::Boat, pos: V(at.x, level, at.y), dir: V(d.x, 0.0, d.y) }]);
        }
    }

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
/// Ground below this height above sea level is beach sand.
const SAND_TOP: f32 = 0.55;
/// Beach above this height is the dry upper beach where marram grows by itself.
const UPPER_BEACH: f32 = 0.25;

/// What scatter needs to know about the ground (closures so tests can fake it).
struct Probe<'a> {
    height: &'a dyn Fn(Vec2) -> f32,
    normal_y: &'a dyn Fn(Vec2) -> f32,
    water: &'a dyn Fn(Vec2) -> bool,
    paint: &'a dyn Fn(PaintLayer, Vec2) -> f32,
}

/// Ground cover scattered over the island from the terrain's paint layers (sea-pink cushions,
/// marram, wrack, pebbles) plus a light automatic layer: grass tufts and wild flowers on grass,
/// marram on the upper beach and shells on sand. Nothing is scattered inside `blockers`
/// (structure footprints / path ribbons), on water, or on rock (steep ground).
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
        // Over budget: thin every cell uniformly (slightly under-shoot).
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
            let layer = |l| (probe.paint)(l, p).clamp(0.0, 1.0);
            let (pink, dune, wrack, pebbles) = (layer(PaintLayer::SeaPinks), layer(PaintLayer::DuneGrass), layer(PaintLayer::Wrack), layer(PaintLayer::Pebbles));
            let sand = y < SAND_TOP;

            // Painted layers.
            if pink > 0.05 && rng.f() < 0.3 * pink {
                thrift(&mut m, at, &mut rng);
            }
            if dune > 0.05 && rng.f() < 0.3 * dune {
                marram(&mut m, at, &mut rng);
            }
            if wrack > 0.05 && rng.f() < 0.35 * wrack {
                wrack_strands(&mut m, at, &mut rng);
            }
            if pebbles > 0.05 && rng.f() < 0.12 * pebbles {
                pebble(&mut m, at, &mut rng);
            }

            if sand {
                // Beach: marram on the dry upper beach, shells anywhere on the sand.
                if y >= UPPER_BEACH && dune < 0.05 && rng.f() < 0.1 {
                    marram(&mut m, at, &mut rng);
                }
                if rng.f() < 0.05 {
                    shell(&mut m, at, &mut rng);
                }
            } else {
                // Grass: tufts (suppressed on pebbles and in dense thrift) and wild flowers.
                let ptuft = 0.4 * (1.0 - (pebbles * 1.5).min(1.0)) * (1.0 - pink * 0.5);
                if rng.f() < ptuft {
                    tuft(&mut m, at, &mut rng);
                }
                if rng.f() < 0.04 * (1.0 - pebbles * 0.8) {
                    let patch = hash2(seed, (p.x / 3.0).floor(), (p.y / 3.0).floor());
                    let idx = if rng.f() < 0.85 { patch % 5 } else { rng.next_u64() % 5 } as usize;
                    flower(&mut m, at, idx, &mut rng);
                }
            }
        }
    }
    m
}

fn tuft(m: &mut MeshData, at: Vec3, rng: &mut Rng) {
    let base_col = if rng.f() < 0.25 { palette::grass_dry() } else { palette::grass() } * rng.range(0.8, 1.2);
    let blades = 2 + (rng.f() * 2.0) as usize;
    let yaw0 = rng.f() * PI;
    for k in 0..blades {
        let yaw = yaw0 + k as f32 * PI / blades as f32 + rng.range(-0.2, 0.2);
        let dir = V(yaw.cos(), 0.0, yaw.sin());
        let h = rng.range(0.22, 0.45);
        let lean = V(rng.range(-0.08, 0.08), 0.0, rng.range(-0.08, 0.08));
        let base = at + V(rng.range(-0.05, 0.05), -0.02, rng.range(-0.05, 0.05));
        blade(m, base, dir, 0.035, base + lean + Vec3::Y * h, dim(base_col, 0.75), base_col.lerp(palette::grass_tip(), 0.4), Surface::Grass);
    }
}

fn flower(m: &mut MeshData, at: Vec3, idx: usize, rng: &mut Rng) {
    let h = rng.range(0.25, 0.45);
    let yaw = rng.f() * PI;
    let dir = V(yaw.cos(), 0.0, yaw.sin());
    let stem = palette::grass();
    let top = at + V(rng.range(-0.03, 0.03), h, rng.range(-0.03, 0.03));
    blade(m, at - Vec3::Y * 0.02, dir, 0.012, top, dim(stem, 0.7), stem, Surface::Grass);
    cyl(m, top - Vec3::Y * 0.02, 0.09, 0.0, 0.06, 4, (false, false), palette::wildflower(idx) * rng.range(0.92, 1.0), Surface::Petal);
}

/// Thrift cushion: a low pink dome with deeper magenta centres.
fn thrift(m: &mut MeshData, at: Vec3, rng: &mut Rng) {
    let r = rng.range(0.22, 0.34);
    let base = at - Vec3::Y * 0.02;
    cyl(m, base, r, r * 0.5, 0.15, 6, (true, false), palette::sea_pink() * rng.range(0.9, 1.0), Surface::Petal);
    for _ in 0..3 {
        let o = V(rng.range(-0.6, 0.6), 0.0, rng.range(-0.6, 0.6)) * r * 0.6;
        speck(m, base + o + Vec3::Y * 0.12, 0.05, 0.07, 3, palette::sea_pink_deep(), Surface::Petal);
    }
}

/// Marram tuft: a fan of tall arching blades.
fn marram(m: &mut MeshData, at: Vec3, rng: &mut Rng) {
    let col = palette::marram() * rng.range(0.88, 1.0);
    let n = 5;
    let y0 = rng.f() * TAU;
    for k in 0..n {
        let a = y0 + k as f32 * TAU / n as f32 + rng.range(-0.3, 0.3);
        let out = V(a.cos(), 0.0, a.sin());
        let side = V(-a.sin(), 0.0, a.cos());
        let (h, lean) = (rng.range(0.4, 0.75), rng.range(0.1, 0.3));
        let base = at + out * 0.03 - Vec3::Y * 0.02;
        blade(m, base, side, 0.03, base + out * lean + Vec3::Y * h, dim(col, 0.72), col, Surface::Petal);
    }
}

/// Wrack: a few strands of dark olive and amber seaweed lying on the shore.
fn wrack_strands(m: &mut MeshData, at: Vec3, rng: &mut Rng) {
    let strands = 2 + (rng.f() * 2.0) as usize;
    for k in 0..strands {
        let col = palette::wrack_strand((rng.next_u64() % 3) as usize);
        let mut yaw = rng.f() * TAU;
        let w = rng.range(0.03, 0.05);
        let mut p = at + V(rng.range(-0.15, 0.15), 0.025 + 0.002 * k as f32, rng.range(-0.15, 0.15));
        for _ in 0..3 {
            let d = V(yaw.cos(), 0.0, yaw.sin());
            let s = V(-yaw.sin(), 0.0, yaw.cos()) * w;
            let q = p + d * rng.range(0.13, 0.2);
            up_tri(m, p - s, p + s, q + s * 0.7, col, Surface::Petal);
            up_tri(m, p - s, q + s * 0.7, q - s * 0.7, col, Surface::Petal);
            p = q;
            yaw += rng.range(-0.6, 0.6);
        }
    }
}

/// Small flattened stone: jittered octahedron in lilac or rose.
fn pebble(m: &mut MeshData, at: Vec3, rng: &mut Rng) {
    let (rx, rz, h) = (rng.range(0.04, 0.1), rng.range(0.04, 0.1), rng.range(0.03, 0.06));
    let col = if rng.f() < 0.5 { palette::pebble_lilac() } else { palette::pebble_rose() } * rng.range(0.9, 1.05);
    let c = at + Vec3::Y * (h * 0.4);
    let ring = [V(rx, 0.0, 0.0), V(0.0, 0.0, rz * 1.2), V(-rx, 0.0, 0.0), V(0.0, 0.0, -rz)];
    for i in 0..4 {
        let (a, b) = (c + ring[i], c + ring[(i + 1) % 4]);
        m.tri(c + Vec3::Y * h, a, b, col, Surface::Pebble, 1.0);
        m.tri(c - Vec3::Y * h * 0.4, b, a, dim(col, 0.8), Surface::Pebble, 1.0);
    }
}

/// Little scallop shell: a flat five-sided cone, white or pink nacre.
fn shell(m: &mut MeshData, at: Vec3, rng: &mut Rng) {
    let col = if rng.f() < 0.4 { palette::nacre() } else { palette::shell() };
    speck(m, at + V(0.0, -0.005, 0.0), rng.range(0.04, 0.07), 0.03, 5, col, Surface::Shell);
}

// ---------------------------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    const TREES: [TreeKind; 5] = [TreeKind::SeaPine, TreeKind::Palm, TreeKind::Cypress, TreeKind::Fig, TreeKind::Snag];
    const CLUTTER: [ClutterKind; 14] = [
        ClutterKind::Canoe, ClutterKind::DryingRack, ClutterKind::Firepit, ClutterKind::Cairn, ClutterKind::StandingStone,
        ClutterKind::CarvedPole, ClutterKind::NetRack, ClutterKind::Baskets, ClutterKind::LanternPost, ClutterKind::Pennant,
        ClutterKind::Rock, ClutterKind::Bush, ClutterKind::CrabPots, ClutterKind::Woodpile,
    ];
    /// A dry, flat spot to stand props on (the same for every test).
    fn at() -> Vec2 {
        static AT: std::sync::OnceLock<Vec2> = std::sync::OnceLock::new();
        *AT.get_or_init(|| {
            let t = terrain();
            [(3.0, -2.0), (0.0, 0.0), (5.0, 5.0), (-5.0, 5.0), (-5.0, -5.0), (8.0, 0.0)]
                .iter()
                .map(|&(x, z)| Vec2::new(x, z))
                .find(|&p| t.water_level(p).is_none() && t.height(p) > 0.8 && t.normal(p).y > 0.95)
                .unwrap_or(Vec2::new(3.0, -2.0))
        })
    }

    fn terrain() -> Terrain {
        Terrain::build(1, &[], &[])
    }

    fn put_at(kind: PropKind, scale: f32, at: Vec2, seed: u64) -> (MeshData, Vec<Marker>) {
        let p = Placed { at, angle: 0.0, scale, kind, colors: Colors::default() };
        placed_mesh(&p, &terrain(), seed)
    }

    fn put(kind: PropKind, scale: f32) -> (MeshData, Vec<Marker>) {
        put_at(kind, scale, at(), 7)
    }

    fn tris(m: &MeshData) -> usize {
        m.indices.len() / 3
    }

    /// Height of the mesh above the terrain where it stands.
    fn height_above(m: &MeshData, at: Vec2) -> f32 {
        m.bounds().unwrap().1.y - terrain().height(at)
    }

    #[test]
    fn trees_valid_sized_and_distinct() {
        let ranges = [(TreeKind::SeaPine, 3.5, 6.5), (TreeKind::Palm, 3.5, 6.0), (TreeKind::Cypress, 4.5, 8.0), (TreeKind::Fig, 3.2, 5.0), (TreeKind::Snag, 3.0, 5.5)];
        assert_eq!(TREES.len(), ranges.len());
        let mut seen: Vec<MeshData> = Vec::new();
        for (k, lo, hi) in ranges {
            let (m, mk) = put(PropKind::Tree(k), 1.0);
            assert!(m.is_valid() && !m.is_empty(), "{k:?}");
            assert!(tris(&m) <= 700, "{k:?} {}", tris(&m));
            let top = height_above(&m, at());
            assert!(top > lo && top < hi, "{k:?} height {top}");
            assert!(mk.iter().any(|x| x.kind == MarkerKind::Perch), "{k:?}");
            assert!(seen.iter().all(|o| *o != m), "{k:?} duplicates another tree");
            seen.push(m);
        }
    }

    #[test]
    fn tree_shapes_follow_their_description() {
        let lean = |k: TreeKind| {
            let (m, _) = put(PropKind::Tree(k), 1.0);
            let (lo, hi) = m.bounds().unwrap();
            // Horizontal spread relative to the tree's footprint at the trunk base.
            (lo.x.min(at().x) - at().x, hi.x - at().x, hi.y - lo.y, hi.z - lo.z)
        };
        // Palm leans downwind (+x) and its fronds droop below the crown.
        let (_, palm_x, ..) = lean(TreeKind::Palm);
        assert!(palm_x > 1.5);
        let (m, _) = put(PropKind::Tree(TreeKind::Palm), 1.0);
        let ground = terrain().height(at());
        let crown_y = ground + 3.75;
        assert!(m.positions.iter().any(|p| p[1] > crown_y + 0.1), "upright young fronds");
        let leaf = Surface::Leaves as u8;
        let drooping = m.positions.iter().zip(&m.surfaces).filter(|(p, s)| **s == leaf && p[1] < crown_y - 0.5).count();
        assert!(drooping > 6, "fronds droop below the crown");
        // Cypress is tall and narrow, fig is broad and low.
        let (_, _, ch, cz) = lean(TreeKind::Cypress);
        assert!(ch > 2.5 * cz.max(1.0) * 0.9, "cypress {ch} x {cz}");
        let (_, _, fh, fz) = lean(TreeKind::Fig);
        assert!(fz > fh, "fig should be wider than tall: {fz} vs {fh}");
        // Fig carries purple fruit (Petal surface), sea pine none.
        let petals = |k: TreeKind| put(PropKind::Tree(k), 1.0).0.surfaces.iter().filter(|&&s| s == Surface::Petal as u8).count();
        assert!(petals(TreeKind::Fig) >= 7 * 24);
        assert_eq!(petals(TreeKind::SeaPine), 0);
        // Sea pine canopy: flat pads (Leaves) wider than the trunk, bent downwind.
        let (_, sx, ..) = lean(TreeKind::SeaPine);
        assert!(sx > 1.5);
    }

    #[test]
    fn clutter_valid_sized_and_distinct() {
        let mut seen: Vec<MeshData> = Vec::new();
        for k in CLUTTER {
            let (m, _) = put(PropKind::Clutter(k), 1.0);
            assert!(m.is_valid() && !m.is_empty(), "{k:?}");
            assert!(tris(&m) <= 520, "{k:?} {}", tris(&m));
            let (lo, hi) = match k {
                ClutterKind::Canoe => (0.6, 1.3),
                ClutterKind::DryingRack => (1.8, 2.2),
                ClutterKind::Firepit => (0.3, 0.9),
                ClutterKind::Cairn => (0.7, 1.9),
                ClutterKind::StandingStone => (2.3, 3.0),
                ClutterKind::CarvedPole => (3.5, 5.0),
                ClutterKind::NetRack => (1.8, 2.3),
                ClutterKind::Baskets => (0.4, 0.9),
                ClutterKind::LanternPost => (2.2, 2.8),
                ClutterKind::Pennant => (5.2, 6.0),
                ClutterKind::Rock => (0.4, 1.5),
                ClutterKind::Bush => (0.5, 1.6),
                ClutterKind::CrabPots => (0.8, 1.4),
                ClutterKind::Woodpile => (0.7, 1.2),
            };
            let top = height_above(&m, at());
            assert!(top > lo && top < hi, "{k:?} height {top}");
            assert!(seen.iter().all(|o| *o != m), "{k:?} duplicates another prop");
            seen.push(m);
        }
    }

    #[test]
    fn every_variant_is_deterministic_and_seeded() {
        for k in CLUTTER {
            assert_eq!(put(PropKind::Clutter(k), 1.2), put(PropKind::Clutter(k), 1.2), "{k:?}");
        }
        for k in TREES {
            assert_eq!(put(PropKind::Tree(k), 1.0), put(PropKind::Tree(k), 1.0), "{k:?}");
            // Same spot, different document seed: a different tree.
            assert_ne!(put_at(PropKind::Tree(k), 1.0, at(), 1).0, put_at(PropKind::Tree(k), 1.0, at(), 2).0, "{k:?}");
        }
    }

    #[test]
    fn meshes_use_vibrant_colours() {
        // No grey and nothing near-black anywhere: every vertex tint keeps some saturation or is a
        // bright cream, and the darkest channel never collapses to black.
        for kind in TREES.map(PropKind::Tree).into_iter().chain(CLUTTER.map(PropKind::Clutter)) {
            let (m, _) = put(kind, 1.0);
            for c in &m.colors {
                let (mx, mn) = (c[0].max(c[1]).max(c[2]), c[0].min(c[1]).min(c[2]));
                assert!(mx > 0.05, "{kind:?} near-black vertex {c:?}");
                // Grey = low chroma at mid value. Creams (bleached wood, shell) stay warm or lilac.
                assert!(!(mx > 0.05 && (mx - mn) / mx < 0.04 && mx < 0.9), "{kind:?} grey vertex {c:?}");
            }
        }
    }

    #[test]
    fn markers_for_fire_lantern_flag_and_perch() {
        let (_, mk) = put(PropKind::Clutter(ClutterKind::Firepit), 1.0);
        let fire = mk.iter().find(|x| x.kind == MarkerKind::Fire).expect("fire marker");
        assert!(fire.pos.y > terrain().height(at()));
        let (_, mk) = put(PropKind::Clutter(ClutterKind::LanternPost), 1.0);
        assert!(mk.iter().any(|x| x.kind == MarkerKind::Lantern));
        let (m, _) = put(PropKind::Clutter(ClutterKind::LanternPost), 1.0);
        assert!(m.surfaces.iter().any(|&s| s == Surface::Glow as u8), "lantern glass is emissive Glow");
        let (_, mk) = put(PropKind::Clutter(ClutterKind::Pennant), 1.0);
        let f = mk.iter().find(|x| x.kind == MarkerKind::Flag).unwrap();
        assert!(f.pos.y > terrain().height(at()) + 4.5);
        for k in CLUTTER {
            if !matches!(k, ClutterKind::Firepit | ClutterKind::LanternPost | ClutterKind::Pennant) {
                assert!(put(PropKind::Clutter(k), 1.0).1.is_empty(), "{k:?} has no markers");
            }
        }
    }

    #[test]
    fn cairn_has_five_to_nine_stones() {
        let mut counts = std::collections::BTreeSet::new();
        for i in 0..60 {
            let at = Vec2::new(-20.0 + i as f32 * 0.7, 4.0 + (i % 7) as f32);
            let (m, _) = put_at(PropKind::Clutter(ClutterKind::Cairn), 1.0, at, 3);
            assert_eq!(tris(&m) % 20, 0);
            let n = tris(&m) / 20;
            assert!((5..=9).contains(&n), "{n} stones");
            counts.insert(n);
        }
        assert!(counts.len() >= 3, "stone count should vary: {counts:?}");
    }

    #[test]
    fn standing_stone_and_pole_are_painted_in_pigment() {
        let carving = |k: ClutterKind| put(PropKind::Clutter(k), 1.0).0.surfaces.iter().filter(|&&s| s == Surface::Carving as u8).count();
        assert!(carving(ClutterKind::StandingStone) >= 20, "carved spiral");
        assert!(carving(ClutterKind::CarvedPole) >= 100, "bands, faces and bird");
        // The trim colour reaches the carving.
        let p = |trim: u8| Placed {
            at: at(),
            angle: 0.0,
            scale: 1.0,
            kind: PropKind::Clutter(ClutterKind::StandingStone),
            colors: Colors { trim, ..Colors::default() },
        };
        assert_ne!(placed_mesh(&p(0), &terrain(), 1).0, placed_mesh(&p(2), &terrain(), 1).0);
    }

    #[test]
    fn canoe_floats_on_water_and_stands_on_land() {
        let t = terrain();
        // Land: a real hull with no Boat marker.
        let (m, mk) = put(PropKind::Clutter(ClutterKind::Canoe), 1.0);
        assert!(!m.is_empty() && mk.is_empty());
        // Hull is long (bow along local +x), outrigger float lies to the side.
        let (lo, hi) = m.bounds().unwrap();
        assert!(hi.x - lo.x > 3.0 && hi.z - lo.z > 1.5, "{lo:?} {hi:?}");
        // Water: Boat marker and no mesh.
        let wet = (-60..60)
            .flat_map(|x| (-60..60).map(move |z| Vec2::new(x as f32, z as f32)))
            .find(|&p| t.water_level(p).is_some());
        let Some(wet) = wet else { return };
        let p = Placed { at: wet, angle: 0.7, scale: 1.0, kind: PropKind::Clutter(ClutterKind::Canoe), colors: Colors::default() };
        let (m, mk) = placed_mesh(&p, &t, 5);
        assert!(m.is_empty());
        assert_eq!(mk.len(), 1);
        assert_eq!(mk[0].kind, MarkerKind::Boat);
        assert!((mk[0].dir - V(0.7f32.cos(), 0.0, 0.7f32.sin())).length() < 1e-5);
        assert!((mk[0].pos.y - t.water_level(wet).unwrap()).abs() < 1e-5);
        // Other props on water still get a mesh.
        let p = Placed { kind: PropKind::Clutter(ClutterKind::Rock), ..p };
        assert!(!placed_mesh(&p, &t, 5).0.is_empty());
    }

    #[test]
    fn scale_is_clamped_and_rotation_keeps_position() {
        let k = PropKind::Clutter(ClutterKind::Baskets);
        assert_eq!(put(k, 100.0).0, put(k, 3.0).0);
        assert_eq!(put(k, 0.001).0, put(k, 0.3).0);
        assert_eq!(put(k, f32::NAN).0, put(k, 1.0).0);
        let mut p = Placed { at: Vec2::new(5.0, 5.0), angle: 1.3, scale: 1.0, kind: k, colors: Colors::default() };
        let (m, _) = placed_mesh(&p, &terrain(), 1);
        let (lo, hi) = m.bounds().unwrap();
        let mid = (lo + hi) * 0.5;
        assert!((mid.x - 5.0).abs() < 0.8 && (mid.z - 5.0).abs() < 0.8);
        p.angle = f32::NAN;
        assert!(placed_mesh(&p, &terrain(), 1).0.is_valid());
        // Angle really rotates a long prop: a pole row along x is along z after a quarter turn.
        let rack = |angle: f32| {
            let p = Placed { at: Vec2::ZERO, angle, scale: 1.0, kind: PropKind::Clutter(ClutterKind::DryingRack), colors: Colors::default() };
            let (lo, hi) = placed_mesh(&p, &terrain(), 1).0.bounds().unwrap();
            (hi.x - lo.x, hi.z - lo.z)
        };
        let (w0, d0) = rack(0.0);
        let (w1, d1) = rack(PI / 2.0);
        assert!(w0 > d0 && d1 > w1);
        // Scale 2 doubles the height above the ground.
        let h = |s: f32| height_above(&put(PropKind::Clutter(ClutterKind::StandingStone), s).0, at());
        assert!((h(2.0) / h(1.0) - 2.0).abs() < 0.05);
    }

    #[test]
    fn canoe_and_slab_faces_look_outwards() {
        // Canoe: planked outside faces away from the keel line, the hollow faces up.
        let (m, _) = put(PropKind::Clutter(ClutterKind::Canoe), 1.0);
        let a = at();
        let mut checked = 0;
        for ((p, n), s) in m.positions.iter().zip(&m.normals).zip(&m.surfaces) {
            let (dz, n) = (p[2] - a.y, Vec3::from_array(*n));
            if *s == Surface::Plank as u8 && dz.abs() > 0.05 && p[0] - a.x < 1.3 {
                assert!(n.z * dz > 0.0, "planking faces inwards at {p:?} {n:?}");
                checked += 1;
            }
            if *s == Surface::Boards as u8 {
                assert!(n.y > -1e-3, "hollow faces down at {p:?} {n:?}");
            }
        }
        assert!(checked > 10);
        // Standing stone: the slab is the first five quads, every face points away from its middle.
        let (m, _) = put(PropKind::Clutter(ClutterKind::StandingStone), 1.0);
        let mid = V(a.x, terrain().height(a) + 1.2, a.y);
        for (p, n) in m.positions.iter().zip(&m.normals).take(20) {
            assert!(Vec3::from_array(*n).dot(Vec3::from_array(*p) - mid) > 0.0, "{p:?} {n:?}");
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

    #[test]
    fn tube_normals_point_outwards() {
        let mut m = MeshData::default();
        let pts = [V(0.0, 0.0, 0.0), V(0.3, 1.0, 0.0), V(1.0, 1.8, 0.2)];
        tube(&mut m, &pts, &|_| 0.2, 6, &|_| Vec3::ONE, Surface::Bark);
        assert!(m.is_valid() && !m.is_empty());
        for (p, n) in m.positions.iter().zip(&m.normals) {
            let p = Vec3::from_array(*p);
            let n = Vec3::from_array(*n);
            // Distance to the centre line (closest point on the polyline).
            let near = pts.windows(2).map(|s| {
                let d = s[1] - s[0];
                s[0] + d * ((p - s[0]).dot(d) / d.length_squared()).clamp(0.0, 1.0)
            });
            let c = near.min_by(|a, b| (p - *a).length().total_cmp(&(p - *b).length())).unwrap();
            assert!(n.dot(p - c) > 0.0, "{p:?} {n:?}");
        }
    }

    // ---- scatter -------------------------------------------------------------------------

    fn fake<'a>(
        height: &'a dyn Fn(Vec2) -> f32,
        water: &'a dyn Fn(Vec2) -> bool,
        ny: &'a dyn Fn(Vec2) -> f32,
        paint: &'a dyn Fn(PaintLayer, Vec2) -> f32,
    ) -> Probe<'a> {
        Probe { height, normal_y: ny, water, paint }
    }

    const GRASS_H: f32 = 1.5;
    const UPPER_H: f32 = 0.4;
    const WATERLINE_H: f32 = 0.1;

    fn run(h: f32, layer: Option<PaintLayer>) -> MeshData {
        let paint = move |l: PaintLayer, _: Vec2| if Some(l) == layer { 1.0 } else { 0.0 };
        scatter_with(1, &[], &fake(&move |_| h, &|_| false, &|_| 1.0, &paint))
    }

    fn count(m: &MeshData, s: Surface) -> usize {
        m.surfaces.iter().filter(|&&x| x == s as u8).count()
    }

    #[test]
    fn scatter_blocked_everywhere_is_empty() {
        let b = vec![vec![Vec2::new(-40.0, -40.0), Vec2::new(40.0, -40.0), Vec2::new(40.0, 40.0), Vec2::new(-40.0, 40.0)]];
        assert!(scatter_mesh(&terrain(), 3, &b).is_empty());
    }

    #[test]
    fn scatter_none_under_blockers() {
        let b = vec![vec![Vec2::new(-10.0, -10.0), Vec2::new(10.0, -10.0), Vec2::new(10.0, 10.0), Vec2::new(-10.0, 10.0)]];
        let paint = |l: PaintLayer, _: Vec2| if l == PaintLayer::SeaPinks { 1.0 } else { 0.0 };
        let m = scatter_with(1, &b, &fake(&|_| GRASS_H, &|_| false, &|_| 1.0, &paint));
        assert!(!m.is_empty());
        // Vertices may poke out of a cell by a blade's reach but never well inside the box.
        assert!(m.positions.iter().all(|p| p[0].abs() > 9.0 || p[2].abs() > 9.0));
    }

    #[test]
    fn scatter_real_terrain_valid_deterministic_within_budget() {
        let a = scatter_mesh(&terrain(), 3, &[]);
        assert!(a.is_valid() && !a.is_empty());
        assert!(tris(&a) <= MAX_TRIS);
        assert_eq!(a, scatter_mesh(&terrain(), 3, &[]));
        assert_ne!(a, scatter_mesh(&terrain(), 4, &[]));
    }

    #[test]
    fn scatter_skips_water_rock_and_fake_world_is_deterministic() {
        let none = |_: PaintLayer, _: Vec2| 1.0;
        assert!(scatter_with(1, &[], &fake(&|_| GRASS_H, &|_| true, &|_| 1.0, &none)).is_empty());
        assert!(scatter_with(1, &[], &fake(&|_| GRASS_H, &|_| false, &|_| 0.5, &none)).is_empty());
        // Water only on the x > 0 half: nothing grows there.
        let m = scatter_with(1, &[], &fake(&|_| GRASS_H, &|p| p.x > 0.0, &|_| 1.0, &|_, _| 0.0));
        assert!(!m.is_empty() && m.positions.iter().all(|p| p[0] <= 1.0));
        assert_eq!(run(GRASS_H, Some(PaintLayer::Wrack)), run(GRASS_H, Some(PaintLayer::Wrack)));
    }

    #[test]
    fn automatic_scatter_follows_the_ground_type() {
        let grass = run(GRASS_H, None);
        let upper = run(UPPER_H, None);
        let waterline = run(WATERLINE_H, None);
        // Grass: tufts and wild flowers, no shells, no marram.
        assert!(count(&grass, Surface::Grass) > 1000 && count(&grass, Surface::Petal) > 0);
        assert_eq!(count(&grass, Surface::Shell), 0);
        // Sand: shells, no grass tufts or flowers; marram (Petal) only on the upper beach.
        for sand in [&upper, &waterline] {
            assert!(count(sand, Surface::Shell) > 100);
            assert_eq!(count(sand, Surface::Grass), 0);
        }
        assert!(count(&upper, Surface::Petal) > 100);
        assert_eq!(count(&waterline, Surface::Petal), 0);
        for m in [&grass, &upper, &waterline] {
            assert!(m.is_valid() && tris(m) <= MAX_TRIS);
        }
    }

    #[test]
    fn painted_layers_add_their_scatter() {
        let base = run(GRASS_H, None);
        for layer in [PaintLayer::SeaPinks, PaintLayer::DuneGrass] {
            let m = run(GRASS_H, Some(layer));
            assert!(m.is_valid() && tris(&m) <= MAX_TRIS, "{layer:?}");
            assert!(count(&m, Surface::Petal) > count(&base, Surface::Petal) + 500, "{layer:?}");
        }
        // Wrack: strands appear only near the waterline in the layer's colours (olive / amber).
        let wr = run(WATERLINE_H, Some(PaintLayer::Wrack));
        assert!(count(&wr, Surface::Petal) > 500 && tris(&wr) <= MAX_TRIS);
        assert!(wr.colors.iter().zip(&wr.surfaces).filter(|(_, s)| **s == Surface::Petal as u8).all(|(c, _)| c[1] >= c[2] * 3.0));
        // Pebbles: stones on the Pebble surface, tufts suppressed.
        let pb = run(GRASS_H, Some(PaintLayer::Pebbles));
        assert!(count(&pb, Surface::Pebble) > 100);
        assert!(count(&pb, Surface::Grass) < count(&base, Surface::Grass) / 2);
        // Sea pinks are pink, not olive: red clearly above green on the cushion tops.
        let sp = run(GRASS_H, Some(PaintLayer::SeaPinks));
        let pink = sp.colors.iter().zip(&sp.surfaces).filter(|(c, s)| **s == Surface::Petal as u8 && c[0] > 0.6 && c[2] > c[1]).count();
        assert!(pink > 300, "{pink}");
        // Clear paints nothing.
        assert_eq!(run(GRASS_H, Some(PaintLayer::Clear)), base);
    }

    #[test]
    fn scatter_thins_uniformly_when_over_budget() {
        let paint = |l: PaintLayer, _: Vec2| if l == PaintLayer::SeaPinks || l == PaintLayer::DuneGrass || l == PaintLayer::Wrack { 1.0 } else { 0.0 };
        let probe = fake(&|_| WATERLINE_H, &|_| false, &|_| 1.0, &paint);
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
}
