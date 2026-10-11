//! Terrain heightfield, brush edits, paint layers and the ground/water meshes.
//!
//! Grid: `GRID` x `GRID` samples over `[-EXTENT, EXTENT]²` where `EXTENT = GLADE_HALF + 8`
//! (the glade plus an 8 m skirt each side), spacing `SPACING = 2*EXTENT/(GRID-1)` = 0.625 m.
//! Index = `j * GRID + i`, `i` along +x, `j` along +z.
//!
//! Water: each water dab records, per cell, `ground height before carving - 0.2`
//! (`carve` tracks how much water dabs lowered a cell, so overlapping dabs do not creep
//! downwards). After all edits, every connected blob of wet cells (mask > 0.5) gets ONE level:
//! the minimum over the blob, so each pond is exactly flat, and its bed is forced below it.

use crate::geom;
use crate::mesh::{hash_f, MeshData, Surface};
use crate::model::{PaintLayer, TerrainDab, TerrainOp, GLADE_HALF};
use crate::palette;
use glam::{Vec2, Vec3};

/// Samples per side of the heightfield grid (covers `[-GLADE_HALF, GLADE_HALF]` and a skirt).
pub const GRID: usize = 129;
/// Half extent of the grid in metres (glade + 8 m skirt).
pub const EXTENT: f32 = GLADE_HALF + 8.0;
/// Distance between grid samples in metres.
pub const SPACING: f32 = 2.0 * EXTENT / (GRID as f32 - 1.0);
/// Bottom of the diorama block.
pub const FLOOR: f32 = -8.0;

const NO_LEVEL: f32 = f32::INFINITY;
const LAYERS: usize = 4;

/// A region the scene flattens the ground under (house footprints, path beds…), applied
/// after the player's dabs. `poly` is a closed counter-clockwise polygon on the ground plane.
#[derive(Clone, Debug, PartialEq)]
pub struct FlattenRegion {
    pub poly: Vec<Vec2>,
    /// Target height; `None` = average of the current ground inside the polygon.
    pub height: Option<f32>,
    /// Blend distance outside the polygon in metres.
    pub margin: f32,
}

/// Heightfield plus water mask and paint layers; a pure function of `(seed, dabs, regions)`.
#[derive(Clone, Debug)]
pub struct Terrain {
    pub seed: u64,
    h: Vec<f32>,
    wet: Vec<f32>,
    level: Vec<f32>,
    paint: [Vec<f32>; LAYERS],
    max_h: f32,
}

fn smooth(a: f32, b: f32, x: f32) -> f32 {
    let t = ((x - a) / (b - a)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Smooth value noise in 0..1 (hash lattice, smoothstep interpolation).
fn vnoise(seed: u64, x: f32, z: f32) -> f32 {
    let (ix, iz) = (x.floor(), z.floor());
    let (fx, fz) = (x - ix, z - iz);
    let (u, v) = (fx * fx * (3.0 - 2.0 * fx), fz * fz * (3.0 - 2.0 * fz));
    let n = |dx: f32, dz: f32| hash_f(seed, ix + dx, iz + dz);
    let a = n(0., 0.) + (n(1., 0.) - n(0., 0.)) * u;
    let b = n(0., 1.) + (n(1., 1.) - n(0., 1.)) * u;
    a + (b - a) * v
}

/// Signed distance from `p` to the glade's rounded-square edge (corner radius 8 m);
/// negative inside the glade, positive on the cliff side.
pub fn glade_dist(p: Vec2) -> f32 {
    let q = p.abs() - Vec2::splat(GLADE_HALF - 8.0);
    q.max(Vec2::ZERO).length() + q.x.max(q.y).min(0.0) - 8.0
}

/// Procedural base ground height before any edits.
fn base_height(seed: u64, p: Vec2) -> f32 {
    let f = |s: f32, o: u64| vnoise(seed ^ o, p.x / s, p.y / s);
    let n = 0.6 * f(16.0, 11) + 0.3 * f(7.0, 23) + 0.1 * f(3.0, 37);
    let mut g = (n * 1.8 - 0.1).clamp(0.0, 1.5);
    // one soft low hill
    let hc = Vec2::new((hash_f(seed, 1.0, 2.0) - 0.5) * 36.0, (hash_f(seed, 3.0, 4.0) - 0.5) * 36.0);
    g += 1.3 * (-(p - hc).length_squared() / (2.0 * 7.0 * 7.0)).exp();
    let d = glade_dist(p);
    if d <= 0.0 {
        return g;
    }
    let s = smooth(0.0, 3.0, d);
    let jag = (vnoise(seed ^ 77, p.x * 0.8, p.y * 0.8) - 0.5) * 1.2 * s;
    (g * (1.0 - s) - 5.0 * s - 0.6 * (d - 3.0).max(0.0) + jag).max(FLOOR + 0.5)
}

fn bilerp(v: &[f32], p: Vec2) -> f32 {
    let (i, j, fx, fz) = cell_of(p);
    let at = |a: usize, b: usize| v[b * GRID + a];
    let x0 = at(i, j) + (at(i + 1, j) - at(i, j)) * fx;
    let x1 = at(i, j + 1) + (at(i + 1, j + 1) - at(i, j + 1)) * fx;
    x0 + (x1 - x0) * fz
}

/// Cell containing `p` (clamped) and fractional position in it.
fn cell_of(p: Vec2) -> (usize, usize, f32, f32) {
    let g = ((p + Vec2::splat(EXTENT)) / SPACING).clamp(Vec2::ZERO, Vec2::splat(GRID as f32 - 1.0));
    let i = (g.x.floor() as usize).min(GRID - 2);
    let j = (g.y.floor() as usize).min(GRID - 2);
    (i, j, g.x - i as f32, g.y - j as f32)
}

fn world(i: usize, j: usize) -> Vec2 {
    Vec2::new(i as f32, j as f32) * SPACING - Vec2::splat(EXTENT)
}

/// Inclusive sample index ranges touched by a disc.
fn range(c: Vec2, r: f32) -> (usize, usize, usize, usize) {
    let max = Vec2::splat(GRID as f32 - 1.0);
    let lo = ((c - Vec2::splat(r) + Vec2::splat(EXTENT)) / SPACING).floor().clamp(Vec2::ZERO, max);
    let hi = ((c + Vec2::splat(r) + Vec2::splat(EXTENT)) / SPACING).ceil().clamp(Vec2::ZERO, max);
    (lo.x as usize, hi.x as usize, lo.y as usize, hi.y as usize)
}

fn seg_dist(p: Vec2, a: Vec2, b: Vec2) -> f32 {
    let ab = b - a;
    let t = if ab.length_squared() > 1e-9 { ((p - a).dot(ab) / ab.length_squared()).clamp(0.0, 1.0) } else { 0.0 };
    (p - a - ab * t).length()
}

impl Terrain {
    /// Builds the base glade from `seed`, applies `dabs` in order, then `flatten` regions.
    pub fn build(seed: u64, dabs: &[TerrainDab], flatten: &[FlattenRegion]) -> Self {
        let n = GRID * GRID;
        let mut t = Terrain {
            seed,
            h: (0..n).map(|k| base_height(seed, world(k % GRID, k / GRID))).collect(),
            wet: vec![0.0; n],
            level: vec![NO_LEVEL; n],
            paint: std::array::from_fn(|_| vec![0.0; n]),
            max_h: 0.0,
        };
        let mut carve = vec![0.0f32; n];
        for d in dabs {
            t.apply_dab(d, &mut carve);
        }
        for r in flatten {
            t.apply_region(r);
        }
        t.finish();
        t
    }

    fn apply_dab(&mut self, d: &TerrainDab, carve: &mut [f32]) {
        if !(d.radius > 0.0) || !d.center.is_finite() {
            return;
        }
        let (i0, i1, j0, j1) = range(d.center, d.radius);
        for j in j0..=j1 {
            for i in i0..=i1 {
                let dist = (world(i, j) - d.center).length();
                if dist >= d.radius {
                    continue;
                }
                let f = 0.5 * (1.0 + (std::f32::consts::PI * dist / d.radius).cos());
                let k = j * GRID + i;
                match d.op {
                    TerrainOp::Sculpt { amount } => self.h[k] += amount * f,
                    TerrainOp::Flatten { height, strength } => {
                        self.h[k] += (height - self.h[k]) * (strength * f).clamp(0.0, 1.0)
                    }
                    TerrainOp::Water { depth } => {
                        self.level[k] = self.level[k].min(self.h[k] + carve[k] - 0.2);
                        self.h[k] -= depth * f;
                        carve[k] += depth * f;
                        self.wet[k] = self.wet[k].max(f);
                    }
                    TerrainOp::Paint { layer, strength } => match layer {
                        PaintLayer::Clear => self.paint.iter_mut().for_each(|p| p[k] *= 1.0 - f),
                        _ => {
                            let p = &mut self.paint[layer as usize][k];
                            *p = (*p + strength * f).clamp(0.0, 1.0);
                        }
                    },
                }
            }
        }
    }

    fn apply_region(&mut self, r: &FlattenRegion) {
        if r.poly.len() < 3 {
            return;
        }
        let m = r.margin.max(0.0);
        let (lo, hi) = r.poly.iter().fold((Vec2::splat(f32::MAX), Vec2::splat(f32::MIN)), |(l, h), p| (l.min(*p), h.max(*p)));
        let (i0, i1, j0, j1) = range((lo + hi) * 0.5, ((hi - lo) * 0.5).max_element() + m + SPACING);
        let inside = |p: Vec2| geom::contains(&r.poly, p);
        let target = r.height.unwrap_or_else(|| {
            let (mut s, mut c) = (0.0, 0);
            for j in j0..=j1 {
                for i in i0..=i1 {
                    if inside(world(i, j)) {
                        s += self.h[j * GRID + i];
                        c += 1;
                    }
                }
            }
            if c > 0 { s / c as f32 } else { self.height((lo + hi) * 0.5) }
        });
        for j in j0..=j1 {
            for i in i0..=i1 {
                let p = world(i, j);
                let inn = inside(p);
                let w = if inn {
                    1.0
                } else if m > 0.0 {
                    let d = (0..r.poly.len()).map(|e| seg_dist(p, r.poly[e], r.poly[(e + 1) % r.poly.len()])).fold(f32::MAX, f32::min);
                    1.0 - smooth(0.0, m, d)
                } else {
                    0.0
                };
                if w > 0.0 {
                    let k = j * GRID + i;
                    self.h[k] += (target - self.h[k]) * w;
                    self.wet[k] *= 1.0 - w;
                }
            }
        }
    }

    /// Flattens every wet blob to its lowest level, sinks beds below it, caches the max height.
    fn finish(&mut self) {
        let n = GRID * GRID;
        let mut seen = vec![false; n];
        let mut stack = Vec::new();
        for s in 0..n {
            if seen[s] || self.wet[s] <= 0.5 {
                continue;
            }
            let mut cells = vec![s];
            seen[s] = true;
            stack.push(s);
            while let Some(k) = stack.pop() {
                let (i, j) = ((k % GRID) as i32, (k / GRID) as i32);
                for (a, b) in [(i - 1, j), (i + 1, j), (i, j - 1), (i, j + 1)] {
                    if a < 0 || b < 0 || a >= GRID as i32 || b >= GRID as i32 {
                        continue;
                    }
                    let q = b as usize * GRID + a as usize;
                    if !seen[q] && self.wet[q] > 0.5 {
                        seen[q] = true;
                        cells.push(q);
                        stack.push(q);
                    }
                }
            }
            let lv = cells.iter().map(|&k| self.level[k]).fold(NO_LEVEL, f32::min);
            for &k in &cells {
                self.level[k] = lv;
                self.h[k] = self.h[k].min(lv - 0.05);
            }
        }
        for k in 0..n {
            if self.wet[k] <= 0.5 {
                self.level[k] = NO_LEVEL;
            }
        }
        self.max_h = self.h.iter().cloned().fold(FLOOR, f32::max);
    }

    /// Ground height at `p` (bilinear). Outside the grid: clamps.
    pub fn height(&self, p: Vec2) -> f32 {
        bilerp(&self.h, p)
    }

    /// Ground normal at `p` (central differences).
    pub fn normal(&self, p: Vec2) -> Vec3 {
        let e = SPACING;
        let dx = self.height(p + Vec2::X * e) - self.height(p - Vec2::X * e);
        let dz = self.height(p + Vec2::Y * e) - self.height(p - Vec2::Y * e);
        Vec3::new(-dx, 2.0 * e, -dz).normalize()
    }

    /// Wet mask (bilinear), 0..1.
    fn wetness(&self, p: Vec2) -> f32 {
        bilerp(&self.wet, p)
    }

    /// Water surface height at `p` if `p` is under water.
    pub fn water_level(&self, p: Vec2) -> Option<f32> {
        if self.wetness(p) <= 0.5 {
            return None;
        }
        let (i, j, _, _) = cell_of(p);
        self.cell_level(i, j)
    }

    /// Mean level of the wet corners of cell `(i, j)`.
    fn cell_level(&self, i: usize, j: usize) -> Option<f32> {
        let (mut s, mut c) = (0.0, 0);
        for (a, b) in [(i, j), (i + 1, j), (i, j + 1), (i + 1, j + 1)] {
            let k = b * GRID + a;
            if self.wet[k] > 0.5 {
                s += self.level[k];
                c += 1;
            }
        }
        (c > 0).then(|| s / c as f32)
    }

    /// Painted layer strength at `p` in 0..=1 (`Clear` is always 0).
    pub fn paint(&self, layer: PaintLayer, p: Vec2) -> f32 {
        match layer {
            PaintLayer::Clear => 0.0,
            l => bilerp(&self.paint[l as usize], p),
        }
    }

    /// Ray–terrain intersection (for picking). Returns the hit point.
    pub fn raycast(&self, origin: Vec3, dir: Vec3) -> Option<Vec3> {
        let dir = dir.try_normalize()?;
        let (lo, hi) = (Vec3::new(-EXTENT, FLOOR, -EXTENT), Vec3::new(EXTENT, self.max_h + 0.01, EXTENT));
        let (mut t0, mut t1) = (0.0f32, f32::MAX);
        let mut inside = true;
        for a in 0..3 {
            if dir[a].abs() < 1e-9 {
                inside &= origin[a] >= lo[a] && origin[a] <= hi[a];
            } else {
                let (u, v) = ((lo[a] - origin[a]) / dir[a], (hi[a] - origin[a]) / dir[a]);
                t0 = t0.max(u.min(v));
                t1 = t1.min(u.max(v));
            }
        }
        if inside && t0 <= t1 {
            let f = |t: f32| {
                let p = origin + dir * t;
                p.y - self.height(Vec2::new(p.x, p.z))
            };
            if f(t0) <= 0.0 {
                return Some(origin + dir * t0);
            }
            let mut t = t0;
            while t < t1 {
                let nt = (t + 0.25).min(t1);
                if f(nt) <= 0.0 {
                    let (mut a, mut b) = (t, nt);
                    for _ in 0..24 {
                        let m = 0.5 * (a + b);
                        if f(m) > 0.0 { a = m } else { b = m }
                    }
                    return Some(origin + dir * b);
                }
                t = nt;
            }
        }
        if dir.y.abs() < 1e-6 {
            return None;
        }
        let t = -origin.y / dir.y;
        (t > 0.0).then(|| origin + dir * t)
    }

    /// Ground mesh (grass/dirt/rock/sand by slope, paint and water, cliff skirt and bottom).
    pub fn ground_mesh(&self) -> MeshData {
        let mut m = MeshData::default();
        let rock_cos = 40f32.to_radians().cos();
        for j in 0..GRID - 1 {
            for i in 0..GRID - 1 {
                let corner = |a: usize, b: usize| {
                    let w = world(a, b);
                    Vec3::new(w.x, self.h[b * GRID + a], w.y)
                };
                let (p00, p10, p01, p11) = (corner(i, j), corner(i + 1, j), corner(i, j + 1), corner(i + 1, j + 1));
                // Counter-clockwise seen from above; alternate the diagonal.
                let tris = if (i + j) % 2 == 0 { [[p00, p01, p11], [p00, p11, p10]] } else { [[p00, p01, p10], [p10, p01, p11]] };
                for t in tris {
                    let n = (t[1] - t[0]).cross(t[2] - t[0]).normalize_or_zero();
                    let c = (t[0] + t[1] + t[2]) / 3.0;
                    let cp = Vec2::new(c.x, c.z);
                    let jit = hash_f(self.seed ^ 5, c.x, c.z);
                    let wet = self.wetness(cp);
                    let (s, col) = if glade_dist(cp) > 0.3 || n.y < rock_cos {
                        (Surface::Rock, palette::rock() * (0.85 + 0.2 * jit))
                    } else if wet > 0.5 {
                        (Surface::Dirt, palette::dirt() * 0.6)
                    } else if wet > 0.02 {
                        (Surface::Gravel, palette::sand() * (0.95 + 0.1 * jit))
                    } else if self.paint(PaintLayer::Gravel, cp) > 0.5 {
                        (Surface::Dirt, palette::dirt() * (0.93 + 0.14 * jit))
                    } else {
                        let k = vnoise(self.seed ^ 9, c.x * 0.12, c.z * 0.12);
                        (Surface::Grass, palette::grass().lerp(palette::grass_dry(), k) * (0.95 + 0.1 * jit))
                    };
                    for p in t {
                        m.vertex(p, n, Vec2::new(p.x, p.z) * 0.5, col, s);
                    }
                    let b = m.positions.len() as u32;
                    m.indices.extend_from_slice(&[b - 3, b - 2, b - 1]);
                }
            }
        }
        // Skirt walls (two strata bands) and bottom, so the block reads as solid.
        let g = GRID - 1;
        let mut edge = |pts: Vec<(usize, usize)>| {
            for w in pts.windows(2) {
                let top = |q: (usize, usize)| {
                    let p = world(q.0, q.1);
                    Vec3::new(p.x, self.h[q.1 * GRID + q.0], p.y)
                };
                let (a, b) = (top(w[0]), top(w[1]));
                let mid = |p: Vec3| Vec3::new(p.x, (p.y + FLOOR) * 0.5, p.z);
                let low = |p: Vec3| Vec3::new(p.x, FLOOR, p.z);
                let r = palette::rock();
                m.quad(low(a), low(b), mid(b), mid(a), r * 0.55, Surface::Rock, 0.5);
                m.quad(mid(a), mid(b), b, a, r * 0.8, Surface::Rock, 0.5);
            }
        };
        edge((0..=g).map(|i| (i, g)).collect()); // +z, increasing x
        edge((0..=g).rev().map(|i| (i, 0)).collect()); // -z
        edge((0..=g).rev().map(|j| (g, j)).collect()); // +x, decreasing z
        edge((0..=g).map(|j| (0, j)).collect()); // -x
        let e = EXTENT;
        m.quad(
            Vec3::new(-e, FLOOR, -e),
            Vec3::new(-e, FLOOR, e),
            Vec3::new(e, FLOOR, e),
            Vec3::new(e, FLOOR, -e),
            palette::rock() * 0.4,
            Surface::Rock,
            0.5,
        );
        m
    }

    /// Water surface mesh (Surface::Water): marching squares on the wet mask (threshold 0.5),
    /// the clipped polygon of each cell at its pond level, so shorelines are smooth.
    pub fn water_mesh(&self) -> MeshData {
        let mut m = MeshData::default();
        let col = Vec3::new(0.08, 0.38, 0.45);
        for j in 0..GRID - 1 {
            for i in 0..GRID - 1 {
                let Some(y) = self.cell_level(i, j) else { continue };
                // Corners counter-clockwise seen from below the +y axis order used by quad_uv.
                let c = [(i, j), (i, j + 1), (i + 1, j + 1), (i + 1, j)];
                let w = c.map(|(a, b)| self.wet[b * GRID + a]);
                let mut poly: Vec<Vec2> = Vec::with_capacity(6);
                for k in 0..4 {
                    let l = (k + 1) % 4;
                    let (pk, pl) = (world(c[k].0, c[k].1), world(c[l].0, c[l].1));
                    if w[k] > 0.5 {
                        poly.push(pk);
                    }
                    if (w[k] > 0.5) != (w[l] > 0.5) {
                        poly.push(pk.lerp(pl, (0.5 - w[k]) / (w[l] - w[k])));
                    }
                }
                for k in 1..poly.len().saturating_sub(1) {
                    let v = |p: Vec2| Vec3::new(p.x, y, p.y);
                    let t = [poly[0], poly[k], poly[k + 1]];
                    let n = Vec3::Y;
                    let b = m.positions.len() as u32;
                    for p in t {
                        m.vertex(v(p), n, p, col, Surface::Water);
                    }
                    m.indices.extend_from_slice(&[b, b + 1, b + 2]);
                }
            }
        }
        m
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dab(x: f32, z: f32, r: f32, op: TerrainOp) -> TerrainDab {
        TerrainDab { center: Vec2::new(x, z), radius: r, op }
    }

    #[test]
    fn deterministic() {
        let d = [dab(1.0, 2.0, 5.0, TerrainOp::Sculpt { amount: 1.0 }), dab(-5.0, 3.0, 4.0, TerrainOp::Water { depth: 1.0 })];
        let (a, b) = (Terrain::build(7, &d, &[]), Terrain::build(7, &d, &[]));
        assert_eq!(a.h, b.h);
        assert_eq!(a.ground_mesh(), b.ground_mesh());
        assert_eq!(a.water_mesh(), b.water_mesh());
        assert_ne!(a.h, Terrain::build(8, &d, &[]).h);
    }

    #[test]
    fn base_shape() {
        let t = Terrain::build(3, &[], &[]);
        assert!(t.height(Vec2::ZERO) > -0.01 && t.height(Vec2::ZERO) < 3.0);
        assert!(t.height(Vec2::new(GLADE_HALF + 3.0, 0.0)) < -4.0);
        let g = t.ground_mesh();
        assert!(g.is_valid() && !g.is_empty());
        // ground triangles face up
        assert!(g.normals[..9].iter().all(|n| n[1] > 0.0));
        assert!(t.water_mesh().is_empty());
    }

    #[test]
    fn sculpt_local() {
        let base = Terrain::build(1, &[], &[]);
        let t = Terrain::build(1, &[dab(0.0, 0.0, 4.0, TerrainOp::Sculpt { amount: 1.5 })], &[]);
        assert!((t.height(Vec2::ZERO) - base.height(Vec2::ZERO) - 1.5).abs() < 0.1);
        let far = Vec2::new(15.0, 15.0);
        assert_eq!(t.height(far), base.height(far));
    }

    #[test]
    fn water_pond() {
        let t = Terrain::build(1, &[dab(0.0, 0.0, 5.0, TerrainOp::Water { depth: 1.0 })], &[]);
        let lv = t.water_level(Vec2::ZERO).expect("wet");
        assert!(t.height(Vec2::ZERO) < lv);
        assert!(t.water_level(Vec2::new(20.0, 0.0)).is_none());
        let w = t.water_mesh();
        assert!(w.is_valid() && !w.is_empty());
        assert!(w.positions.iter().all(|p| (p[1] - lv).abs() < 1e-4));
        assert!(w.normals.iter().all(|n| n[1] > 0.99));
        assert!(t.ground_mesh().is_valid());
    }

    #[test]
    fn water_mesh_hugs_wet_region() {
        let d = [dab(0.0, 0.0, 5.0, TerrainOp::Water { depth: 1.0 }), dab(9.0, 3.0, 3.0, TerrainOp::Water { depth: 1.0 })];
        let t = Terrain::build(7, &d, &[]);
        let m = t.water_mesh();
        assert!(m.is_valid() && !m.is_empty());
        for p in &m.positions {
            let q = Vec2::new(p[0], p[2]);
            // On the 0.5 contour or inside it; never more than half a cell out.
            assert!((0..8).any(|k| {
                let a = k as f32 * 0.785;
                t.wetness(q + Vec2::new(a.cos(), a.sin()) * 0.5 * SPACING) > 0.5
            }) || t.wetness(q) > 0.45, "{q}");
            assert!(m.normals[0][1] > 0.99);
        }
    }

    #[test]
    fn flatten_region_levels() {
        let sq = vec![Vec2::new(-3.0, -3.0), Vec2::new(3.0, -3.0), Vec2::new(3.0, 3.0), Vec2::new(-3.0, 3.0)];
        let r = FlattenRegion { poly: sq.clone(), height: None, margin: 2.0 };
        let d = [dab(0.0, 0.0, 8.0, TerrainOp::Sculpt { amount: 2.0 }), dab(0.0, 0.0, 4.0, TerrainOp::Water { depth: 1.0 })];
        let t = Terrain::build(2, &d, &[r]);
        let hs: Vec<f32> = (-4..=4).flat_map(|x| (-4..=4).map(move |z| (x, z))).map(|(x, z)| t.height(Vec2::new(x as f32 * 0.7, z as f32 * 0.7))).collect();
        let (mn, mx) = hs.iter().fold((f32::MAX, f32::MIN), |(a, b), h| (a.min(*h), b.max(*h)));
        assert!(mx - mn < 0.05, "{mn} {mx}");
        assert!(t.water_level(Vec2::ZERO).is_none());
        let r2 = FlattenRegion { poly: sq, height: Some(1.25), margin: 0.0 };
        assert!((Terrain::build(2, &[], &[r2]).height(Vec2::ZERO) - 1.25).abs() < 1e-4);
    }

    #[test]
    fn paint_and_clear() {
        let d = [
            dab(0.0, 0.0, 4.0, TerrainOp::Paint { layer: PaintLayer::Flowers, strength: 2.0 }),
            dab(0.0, 0.0, 4.0, TerrainOp::Paint { layer: PaintLayer::Gravel, strength: 0.6 }),
        ];
        let t = Terrain::build(1, &d, &[]);
        assert_eq!(t.paint(PaintLayer::Flowers, Vec2::ZERO), 1.0);
        assert!(t.paint(PaintLayer::Gravel, Vec2::ZERO) > 0.5);
        assert_eq!(t.paint(PaintLayer::Flowers, Vec2::new(10.0, 0.0)), 0.0);
        assert!(t.ground_mesh().surfaces.contains(&(Surface::Dirt as u8)));
        let mut d2 = d.to_vec();
        d2.push(dab(0.0, 0.0, 4.0, TerrainOp::Paint { layer: PaintLayer::Flowers, strength: -0.4 }));
        assert!((Terrain::build(1, &d2, &[]).paint(PaintLayer::Flowers, Vec2::ZERO) - 0.6).abs() < 1e-5);
        d2.push(dab(0.0, 0.0, 4.0, TerrainOp::Paint { layer: PaintLayer::Clear, strength: 1.0 }));
        let c = Terrain::build(1, &d2, &[]);
        assert_eq!(c.paint(PaintLayer::Flowers, Vec2::ZERO), 0.0);
        assert_eq!(c.paint(PaintLayer::Gravel, Vec2::ZERO), 0.0);
    }

    #[test]
    fn raycast_hits() {
        let t = Terrain::build(1, &[dab(5.0, 5.0, 3.0, TerrainOp::Flatten { height: 1.0, strength: 1.0 })], &[]);
        let target = Vec3::new(5.0, 1.0, 5.0);
        let dir = Vec3::new(-1.0, -1.0, -1.0).normalize();
        let hit = t.raycast(target - dir * 30.0, dir).expect("hit");
        assert!((hit - target).length() < 0.1, "{hit}");
        assert!(t.raycast(Vec3::new(0.0, 5.0, 0.0), Vec3::Y).is_none());
    }

    #[test]
    fn degenerate() {
        let d = [
            dab(0.0, 0.0, 0.0, TerrainOp::Sculpt { amount: 1.0 }),
            dab(0.0, 0.0, 0.0, TerrainOp::Water { depth: 1.0 }),
            dab(0.0, 0.0, -1.0, TerrainOp::Flatten { height: 0.0, strength: 1.0 }),
            dab(f32::NAN, 0.0, 2.0, TerrainOp::Sculpt { amount: 1.0 }),
        ];
        let t = Terrain::build(1, &d, &[]);
        assert!(t.h.iter().all(|h| h.is_finite()));
        assert!(t.ground_mesh().is_valid());
        let e = Terrain::build(1, &[], &[]);
        assert!(e.normal(Vec2::new(100.0, -100.0)).is_finite());
        assert!(e.height(Vec2::new(1e6, 0.0)).is_finite());
    }

    #[test]
    fn many_dabs() {
        let d: Vec<_> = (0..1000)
            .map(|k| {
                let op = if k % 3 == 0 { TerrainOp::Sculpt { amount: 0.1 } } else { TerrainOp::Paint { layer: PaintLayer::Moss, strength: 0.2 } };
                dab((k % 40) as f32 - 20.0, (k / 40) as f32 - 12.0, 3.0, op)
            })
            .collect();
        assert!(Terrain::build(4, &d, &[]).ground_mesh().is_valid());
    }
}
