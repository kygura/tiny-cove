//! Island heightfield, brush edits, paint layers and the ground / lagoon meshes.
//!
//! Grid: `GRID` x `GRID` samples over `[-EXTENT, EXTENT]²` where `EXTENT = GLADE_HALF + 16`
//! (the buildable area plus a 16 m sea skirt each side), spacing `SPACING = 2*EXTENT/(GRID-1)`
//! = 0.6 m. Index = `j * GRID + i`, `i` along +x, `j` along +z.
//!
//! Base shape ([`IslandPlan`], a function of the seed): a grassy interior 0.5–3 m above sea
//! level, a sand beach ring sloping gently into the sea, a sheltered cove bay cutting in on one
//! side, a rocky headland with cliffs up to ~7 m on its flank, 2–4 offshore sea stacks, and a
//! seabed falling away towards [`FLOOR`], so the sea surrounds the island inside the grid.
//! The sea itself is not meshed here: the renderer draws a sea plane at [`SEA_LEVEL`].
//!
//! Lagoons: each water dab records, per cell, `ground height before carving - 0.2`
//! (`carve` tracks how much water dabs lowered a cell, so overlapping dabs do not creep
//! downwards). After all edits, every connected blob of wet cells (mask > 0.5) gets ONE level:
//! the minimum over the blob, so each lagoon is exactly flat, and its bed is forced below it.

use crate::geom;
use crate::mesh::{hash_f, MeshData, Surface};
use crate::model::{PaintLayer, TerrainDab, TerrainOp, GLADE_HALF, SEA_LEVEL, TIDE_RANGE};
use crate::palette;
use glam::{Vec2, Vec3};
use std::f32::consts::{PI, TAU};

/// Samples per side of the heightfield grid (covers the buildable area and a sea skirt).
pub const GRID: usize = 161;
/// Half extent of the grid in metres (buildable area + 16 m of sea).
pub const EXTENT: f32 = GLADE_HALF + 16.0;
/// Distance between grid samples in metres.
pub const SPACING: f32 = 2.0 * EXTENT / (GRID as f32 - 1.0);
/// Furthest a footprint sharing a terrace with its neighbours is levelled from its own ground.
pub const TERRACE_DEV: f32 = 0.3;
/// Bottom of the diorama block (the seabed far offshore rests just above it).
pub const FLOOR: f32 = -8.0;

/// Mean shoreline radius of the island before the cove, headland and wobble.
const ISLAND_R: f32 = 24.0;
/// How far the cove bay cuts into the island at its centre.
const COVE_DEPTH: f32 = 10.0;
/// How far the headland juts out beyond the mean shoreline.
const HEADLAND_JUT: f32 = 4.0;
/// Hash salt of the island layout. Chosen so the default demo seed (7) opens the cove
/// towards the default camera (front-right) with the headland on its left flank.
const PLAN_SALT: u64 = 0x25D;
/// Ground below this (near the shore) is beach sand.
const BEACH_TOP: f32 = 0.6;

/// Least water depth (at low tide) a floating boat needs under it.
pub const BOAT_DEPTH: f32 = 0.5;

const NO_LEVEL: f32 = f32::INFINITY;
const LAYERS: usize = 4;

/// A region the scene flattens the ground under (house footprints, path beds…), applied
/// after the player's dabs. `poly` is a closed counter-clockwise polygon on the ground plane.
#[derive(Clone, Debug, PartialEq)]
pub struct FlattenRegion {
    pub poly: Vec<Vec2>,
    /// Target height; `None` = average of the ground inside the polygon (after the dabs,
    /// before any region).
    pub height: Option<f32>,
    /// Blend distance outside the polygon in metres.
    pub margin: f32,
    /// Instead of levelling: keep the natural ground inside the polygon and within `margin`
    /// of it untouched by every other region (stilted huts).
    pub keep: bool,
}

/// Heightfield plus lagoon mask and paint layers; a pure function of `(seed, dabs, regions)`.
#[derive(Clone, Debug)]
pub struct Terrain {
    pub seed: u64,
    plan: IslandPlan,
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

/// Signed distance from `p` to the buildable area's rounded-square edge (corner radius 8 m);
/// negative inside, positive outside.
pub fn glade_dist(p: Vec2) -> f32 {
    let q = p.abs() - Vec2::splat(GLADE_HALF - 8.0);
    q.max(Vec2::ZERO).length() + q.x.max(q.y).min(0.0) - 8.0
}

/// The seed's island layout: which way the cove and the headland face, and where the sea
/// stacks stand. Directions are unit vectors on the ground plane `(x, z)` from the origin.
#[derive(Clone, Debug, PartialEq)]
pub struct IslandPlan {
    seed: u64,
    /// Towards the middle of the cove bay.
    pub cove: Vec2,
    /// Towards the rocky headland, on one flank of the cove so it shelters it.
    pub headland: Vec2,
    /// Towards the low sandy spit on the cove's other flank.
    pub spit: Vec2,
    /// Offshore sea stacks `(centre, radius, top height)`. The first stands nearest the
    /// headland cliffs (close enough to bridge).
    pub stacks: Vec<(Vec2, f32, f32)>,
}

impl IslandPlan {
    /// Layout for `seed`.
    pub fn new(seed: u64) -> Self {
        let r = |k: f32| hash_f(seed ^ PLAN_SALT, k, 7.0);
        let cove_a = r(1.0) * TAU;
        let side = if r(2.0) < 0.5 { 1.0 } else { -1.0 };
        let head_a = cove_a + side * PI * (0.42 + 0.1 * r(3.0));
        let mut plan = IslandPlan {
            seed,
            cove: Vec2::from_angle(cove_a),
            headland: Vec2::from_angle(head_a),
            spit: Vec2::from_angle(cove_a - side * 0.8),
            stacks: Vec::new(),
        };
        let count = 2 + ((r(4.0) * 3.0) as usize).min(2);
        for k in 0..count {
            // Alternate sides of the headland, each further round and further out.
            let s = if k % 2 == 0 { -side } else { side };
            let a = head_a + s * (0.14 + 0.2 * k as f32 + 0.08 * r(10.0 + k as f32));
            let dir = Vec2::from_angle(a);
            let gap = if k == 0 { 4.0 + r(20.0) } else { 5.0 + 4.0 * r(20.0 + k as f32) };
            let radius = 1.7 + 0.7 * r(30.0 + k as f32);
            let top = if k == 0 { 5.5 + r(40.0) } else { 3.5 + 3.5 * r(40.0 + k as f32) };
            plan.stacks.push((dir * (plan.shore_radius(dir) + gap + radius), radius, top));
        }
        plan
    }

    /// Distance from the origin to the base shoreline in direction `dir` (unit).
    pub fn shore_radius(&self, dir: Vec2) -> f32 {
        let wob = (vnoise(self.seed ^ 101, dir.x * 1.7 + 9.0, dir.y * 1.7 + 9.0) - 0.5) * 5.0;
        let cove = smooth(0.55f32.cos(), 0.05f32.cos(), dir.dot(self.cove));
        let head = smooth(0.45f32.cos(), 0.1f32.cos(), dir.dot(self.headland));
        let spit = smooth(0.35f32.cos(), 1.0, dir.dot(self.spit));
        ISLAND_R + wob - COVE_DEPTH * cove + HEADLAND_JUT * head + 3.0 * spit
    }

    /// Signed horizontal distance from `p` to the base shoreline (negative inland).
    pub fn shore_dist(&self, p: Vec2) -> f32 {
        p.length() - self.shore_radius(p.normalize_or(Vec2::X))
    }

    /// Base ground height at `p` before any player edits.
    pub fn height(&self, p: Vec2) -> f32 {
        let seed = self.seed;
        let f = |s: f32, o: u64| vnoise(seed ^ o, p.x / s, p.y / s);
        let n = 0.6 * f(14.0, 11) + 0.3 * f(6.0, 23) + 0.1 * f(2.5, 37);
        // Grassy interior with one soft hill near the middle.
        let hc = Vec2::new(hash_f(seed, 1.0, 2.0) - 0.5, hash_f(seed, 3.0, 4.0) - 0.5) * 14.0;
        let hill = 0.8 * (-(p - hc).length_squared() / (2.0 * 6.0 * 6.0)).exp();
        let land = (0.5 + 1.8 * (n - 0.3).max(0.0) + hill).clamp(0.5, 3.0);
        // Beach ring sloping into the shallows, then the shelf dropping towards the floor.
        let r = p.length();
        let dir = p.normalize_or(Vec2::X);
        let d = r - self.shore_radius(dir);
        let coast = if d < 0.0 {
            -0.11 * d
        } else if d < 6.0 {
            -0.22 * d
        } else {
            -1.32 - 0.75 * (d - 6.0)
        };
        let mut h = land + (coast - land) * smooth(-11.0, -5.0, d);
        // Rocky headland: a turf-topped plateau rising seawards to ~7 m, ending in cliffs.
        let hw = smooth(0.55f32.cos(), 0.22f32.cos(), dir.dot(self.headland)) * smooth(6.0, 15.0, r);
        if hw > 0.0 {
            let dh = d + (vnoise(seed ^ 61, p.x * 0.45, p.y * 0.45) - 0.5) * 3.0;
            let plateau = 2.5 + 4.3 * smooth(8.0, 24.0, r) + (n - 0.5) * 0.8;
            let head = if dh < 1.5 { plateau + (-2.5 - plateau) * smooth(-1.0, 1.5, dh) } else { -2.5 - 0.7 * (dh - 1.5) };
            h += (head - h) * hw;
        }
        // Sea stacks: steep ragged rock columns standing in the sea.
        for &(c, rad, top) in &self.stacks {
            let q = p - c;
            let ragged = 1.0 + (vnoise(seed ^ 83, q.x * 0.9 + c.x, q.y * 0.9 + c.y) - 0.5) * 0.35;
            let dist = q.length() * ragged;
            let s = 1.0 - smooth(rad * 0.7, rad * 1.1, dist);
            if s > 0.0 {
                let crown = top - 0.25 * (dist / rad).powi(2) + (n - 0.5) * 0.5;
                h += (crown - h).max(0.0) * s;
            }
        }
        h.max(FLOOR + 0.5)
    }
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
    /// Builds the base island from `seed`, applies `dabs` in order, then `flatten` regions.
    pub fn build(seed: u64, dabs: &[TerrainDab], flatten: &[FlattenRegion]) -> Self {
        let n = GRID * GRID;
        let plan = IslandPlan::new(seed);
        let mut t = Terrain {
            seed,
            h: (0..n).map(|k| plan.height(world(k % GRID, k / GRID))).collect(),
            plan,
            wet: vec![0.0; n],
            level: vec![NO_LEVEL; n],
            paint: std::array::from_fn(|_| vec![0.0; n]),
            max_h: 0.0,
        };
        let mut carve = vec![0.0f32; n];
        for d in dabs {
            t.apply_dab(d, &mut carve);
        }
        t.apply_regions(flatten);
        t.finish();
        t
    }

    fn apply_dab(&mut self, d: &TerrainDab, carve: &mut [f32]) {
        if !(d.radius > 0.0) || !d.center.is_finite() {
            return;
        }
        let ok = match d.op {
            TerrainOp::Sculpt { amount } => amount.is_finite(),
            TerrainOp::Flatten { height, strength } => height.is_finite() && strength.is_finite(),
            TerrainOp::Water { depth } => depth.is_finite(),
            TerrainOp::Paint { strength, .. } => strength.is_finite(),
        };
        if !ok {
            return;
        }
        let d = &TerrainDab {
            op: match d.op {
                TerrainOp::Sculpt { amount } => TerrainOp::Sculpt { amount: amount.clamp(-50.0, 50.0) },
                TerrainOp::Flatten { height, strength } => {
                    TerrainOp::Flatten { height: height.clamp(-50.0, 50.0), strength: strength.clamp(0.0, 1.0) }
                }
                TerrainOp::Water { depth } => TerrainOp::Water { depth: depth.clamp(0.0, 20.0) },
                op @ TerrainOp::Paint { .. } => op,
            },
            ..*d
        };
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

    /// Applies all flatten regions together, so their order does not matter. Each footprint is
    /// levelled to its own target; footprints closer than the grid can resolve (two cells)
    /// share one target (moving none by more than `TERRACE_DEV`), so huts built side by
    /// side stand on one terrace. Margins blend by the
    /// strongest weight towards the weighted mean of the targets that reach the cell, and
    /// `keep` regions protect the ground under them (and their margin) from every margin.
    fn apply_regions(&mut self, regions: &[FlattenRegion]) {
        let regions: Vec<&FlattenRegion> = regions.iter().filter(|r| r.poly.len() >= 3 && r.poly.iter().all(|p| p.is_finite())).collect();
        if regions.is_empty() {
            return;
        }
        let margin = |r: &FlattenRegion| if r.margin.is_finite() { r.margin.clamp(0.0, 50.0) } else { 0.0 };
        let cells = |r: &FlattenRegion| {
            let (lo, hi) = r.poly.iter().fold((Vec2::splat(f32::MAX), Vec2::splat(f32::MIN)), |(l, h), p| (l.min(*p), h.max(*p)));
            range((lo + hi) * 0.5, ((hi - lo) * 0.5).max_element() + margin(r) + SPACING)
        };
        let dist = |r: &FlattenRegion, p: Vec2| (0..r.poly.len()).map(|e| seg_dist(p, r.poly[e], r.poly[(e + 1) % r.poly.len()])).fold(f32::MAX, f32::min);

        // Targets from the ground before any region, as (Σ height, cell count) for averaging.
        let mut target: Vec<(f32, f32)> = regions
            .iter()
            .map(|r| {
                if let Some(h) = r.height.filter(|h| h.is_finite()) {
                    return (h.clamp(-50.0, 50.0), 1.0);
                }
                let (i0, i1, j0, j1) = cells(r);
                let (mut s, mut c) = (0.0, 0.0);
                for j in j0..=j1 {
                    for i in i0..=i1 {
                        if geom::contains(&r.poly, world(i, j)) {
                            s += self.h[j * GRID + i];
                            c += 1.0;
                        }
                    }
                }
                if c > 0.0 { (s, c) } else { (self.height(r.poly[0]), 1.0) }
            })
            .collect();
        // Averaged footprints that (nearly) touch share their target: union-find over pairs.
        // Two groups join only if no member then ends up more than `TERRACE_DEV` from its own
        // level, so a long row of huts on a slope steps down in terraces rather than one big
        // fill. Per group root: (Σ height, cell count, lowest and highest own level).
        let mut group: Vec<usize> = (0..regions.len()).collect();
        let mut span: Vec<(f32, f32, f32, f32)> = target.iter().map(|&(s, c)| (s, c, s / c, s / c)).collect();
        fn root(g: &mut [usize], mut i: usize) -> usize {
            while g[i] != i {
                g[i] = g[g[i]];
                i = g[i];
            }
            i
        }
        let shares = |r: &FlattenRegion| !r.keep && r.height.is_none();
        for a in 0..regions.len() {
            for b in a + 1..regions.len() {
                let (ra, rb) = (regions[a], regions[b]);
                let near = |x: &FlattenRegion, y: &FlattenRegion| x.poly.iter().any(|p| geom::contains(&y.poly, *p) || dist(y, *p) < 2.0 * SPACING);
                if shares(ra) && shares(rb) && (near(ra, rb) || near(rb, ra)) {
                    let (x, y) = (root(&mut group, a), root(&mut group, b));
                    let (gx, gy) = (span[x], span[y]);
                    let joined = (gx.0 + gy.0, gx.1 + gy.1, gx.2.min(gy.2), gx.3.max(gy.3));
                    let mean = joined.0 / joined.1;
                    if x != y && (joined.3 - mean).max(mean - joined.2) <= TERRACE_DEV {
                        group[x.max(y)] = x.min(y);
                        span[x.min(y)] = joined;
                    }
                }
            }
        }
        let mut sums = vec![(0.0f32, 0.0f32); regions.len()];
        for i in 0..regions.len() {
            let g = root(&mut group, i);
            sums[g] = (sums[g].0 + target[i].0, sums[g].1 + target[i].1);
        }
        for i in 0..regions.len() {
            let g = root(&mut group, i);
            target[i] = (sums[g].0 / sums[g].1, 1.0);
        }

        let n = GRID * GRID;
        // (Σ target, count) of footprints over the cell; (Σ w·target, Σ w, max w) of margins.
        let mut inside = vec![(0.0f32, 0u32); n];
        let mut blend = vec![(0.0f32, 0.0f32, 0.0f32); n];
        // 2 = under a kept footprint, 1 = within its margin.
        let mut keep = vec![0u8; n];
        for (r, &(t, _)) in regions.iter().zip(&target) {
            let m = margin(r);
            let (i0, i1, j0, j1) = cells(r);
            for j in j0..=j1 {
                for i in i0..=i1 {
                    let (p, k) = (world(i, j), j * GRID + i);
                    if geom::contains(&r.poly, p) {
                        if r.keep {
                            keep[k] = 2;
                        } else {
                            inside[k] = (inside[k].0 + t, inside[k].1 + 1);
                        }
                    } else if m > 0.0 {
                        let d = dist(r, p);
                        if r.keep {
                            if d <= m {
                                keep[k] = keep[k].max(1);
                            }
                        } else {
                            let w = 1.0 - smooth(0.0, m, d);
                            if w > 0.0 {
                                let b = &mut blend[k];
                                *b = (b.0 + w * t, b.1 + w, b.2.max(w));
                            }
                        }
                    }
                }
            }
        }
        // Priority: kept footprint > levelled footprint > kept margin > levelling margins.
        for k in 0..n {
            let ((s, c), (wt, ws, w)) = (inside[k], blend[k]);
            let (t, w) = match keep[k] {
                2 => continue,
                _ if c > 0 => (s / c as f32, 1.0),
                0 if w > 0.0 => (wt / ws, w),
                _ => continue,
            };
            self.h[k] += (t - self.h[k]) * w;
            self.wet[k] *= 1.0 - w;
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

    /// The seed's island layout (cove, headland, sea stacks).
    pub fn plan(&self) -> &IslandPlan {
        &self.plan
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

    /// True if the ground at `p` lies below mean sea level.
    pub fn is_sea(&self, p: Vec2) -> bool {
        self.height(p) < SEA_LEVEL
    }

    /// Water surface height at `p` if `p` is under water: the lagoon's own level over a
    /// carved lagoon / tide pool, [`SEA_LEVEL`] over the sea (the higher of the two where a
    /// lagoon bed dips below sea level), `None` on dry ground.
    pub fn water_level(&self, p: Vec2) -> Option<f32> {
        let lagoon = self.lagoon_level(p);
        if self.is_sea(p) {
            return Some(lagoon.map_or(SEA_LEVEL, |l| l.max(SEA_LEVEL)));
        }
        lagoon
    }

    /// Water surface at `p` if a boat floats there all day: the water is at least
    /// [`BOAT_DEPTH`] deep at low tide (`SEA_LEVEL - TIDE_RANGE` over the sea, a lagoon's own
    /// level otherwise). `None` on land and over shallows that dry out.
    pub fn boat_water(&self, p: Vec2) -> Option<f32> {
        let level = self.water_level(p).filter(|l| l.is_finite())?;
        let low = if self.is_sea(p) && level <= SEA_LEVEL { SEA_LEVEL - TIDE_RANGE } else { level };
        (low - self.height(p) >= BOAT_DEPTH).then_some(level)
    }

    /// Level of the carved lagoon at `p`, if any.
    fn lagoon_level(&self, p: Vec2) -> Option<f32> {
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

    /// Ground surface and vertex tint of a ground triangle with centroid `c` and normal `n`:
    /// rock on cliffs and steep faces (banded red sandstone), sand on the beach ring, the
    /// seabed and lagoon beds, pebbles where painted, worn dirt on banks, lime-to-emerald grass.
    fn ground_look(&self, c: Vec3, n: Vec3) -> (Surface, Vec3) {
        let cp = Vec2::new(c.x, c.z);
        let jit = 0.95 + 0.1 * hash_f(self.seed ^ 5, c.x, c.z);
        let wet = self.wetness(cp);
        let k = vnoise(self.seed ^ 9, c.x * 0.12, c.z * 0.12);
        let near_shore = self.plan.shore_dist(cp) > -12.0;
        if n.y < 40f32.to_radians().cos() {
            // Cliff strata: an ochre stripe every 1.2 m of height.
            let band = (c.y / 1.2).rem_euclid(1.0) < 0.3;
            let col = if band { palette::cliff_band() } else { palette::cliff() };
            (Surface::Rock, col * jit)
        } else if wet > 0.5 || c.y < SEA_LEVEL - 0.1 {
            // Lagoon beds and the seabed: wet honey sand.
            (Surface::Sand, palette::wet_sand() * jit)
        } else if wet > 0.02 || (near_shore && c.y < BEACH_TOP + 0.3 * k) {
            (Surface::Sand, palette::sand() * jit)
        } else if self.paint(PaintLayer::Pebbles, cp) > 0.5 {
            (Surface::Pebble, palette::stone() * jit)
        } else if n.y < 22f32.to_radians().cos() {
            // Worn banks: red-ochre earth.
            (Surface::Dirt, palette::dirt() * jit)
        } else {
            // Lime grass deepening to emerald, drying towards the dunes.
            let dune = if near_shore { smooth(BEACH_TOP + 0.9, BEACH_TOP + 0.2, c.y) * 0.6 } else { 0.0 };
            (Surface::Grass, palette::grass().lerp(palette::leaves(), k * 0.6).lerp(palette::grass_dry(), dune) * jit)
        }
    }

    /// Ground mesh (surfaces from [`Self::ground_look`]), plus a skirt and bottom so the
    /// block reads as solid from below the sea.
    pub fn ground_mesh(&self) -> MeshData {
        let mut m = MeshData::default();
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
                    let (s, col) = self.ground_look((t[0] + t[1] + t[2]) / 3.0, n);
                    for p in t {
                        m.vertex(p, n, Vec2::new(p.x, p.z) * 0.5, col, s);
                    }
                    let b = m.positions.len() as u32;
                    m.indices.extend_from_slice(&[b - 3, b - 2, b - 1]);
                }
            }
        }
        // Skirt walls (two strata bands) and bottom.
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

    /// Lagoon surface mesh (Surface::Water): marching squares on the wet mask (threshold 0.5),
    /// the clipped polygon of each cell at its lagoon level, so shorelines are smooth. The sea
    /// is not included (the renderer draws it as a plane).
    pub fn water_mesh(&self) -> MeshData {
        let mut m = MeshData::default();
        let col = palette::sea_shallow();
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
    fn non_finite_dabs_are_ignored() {
        let ops = [
            TerrainOp::Sculpt { amount: f32::NAN },
            TerrainOp::Flatten { height: f32::INFINITY, strength: 1.0 },
            TerrainOp::Water { depth: f32::NAN },
        ];
        let base = Terrain::build(3, &[], &[]);
        for op in ops {
            let t = Terrain::build(3, &[dab(0.0, 0.0, 5.0, op)], &[]);
            assert!(t.h.iter().all(|v| v.is_finite()));
            assert_eq!(t.h, base.h);
        }
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
        assert!(t.height(Vec2::ZERO) >= 0.5 && t.height(Vec2::ZERO) <= 3.0);
        let g = t.ground_mesh();
        assert!(g.is_valid() && !g.is_empty());
        // ground triangles face up
        assert!(g.normals[..9].iter().all(|n| n[1] > 0.0));
        // The sea is drawn by the renderer: no lagoons, no water mesh.
        assert!(t.water_mesh().is_empty());
        for s in [Surface::Grass, Surface::Sand, Surface::Rock] {
            assert!(g.surfaces.contains(&(s as u8)), "{s:?}");
        }
    }

    const SEEDS: std::ops::Range<u64> = 0..24;

    #[test]
    fn island_has_sea_on_all_edges() {
        for seed in SEEDS {
            let t = Terrain::build(seed, &[], &[]);
            for k in 0..GRID {
                for (i, j) in [(k, 0), (k, GRID - 1), (0, k), (GRID - 1, k)] {
                    let h = t.h[j * GRID + i];
                    assert!(h < SEA_LEVEL - 4.0, "seed {seed} edge ({i},{j}) {h}");
                    assert!(t.is_sea(world(i, j)) && t.water_level(world(i, j)) == Some(SEA_LEVEL));
                }
            }
            // The seabed far out rests near the floor.
            assert!(t.height(Vec2::splat(EXTENT - 1.0)) < FLOOR + 1.0);
        }
    }

    #[test]
    fn grassy_interior() {
        for seed in SEEDS {
            let t = Terrain::build(seed, &[], &[]);
            for k in 0..32 {
                let p = Vec2::from_angle(k as f32 * TAU / 32.0) * (k % 4) as f32 * 2.0;
                let h = t.height(p);
                assert!((0.5..=3.0).contains(&h), "seed {seed} {p} {h}");
                assert!(t.water_level(p).is_none());
            }
        }
    }

    /// Walking out from the middle in any direction away from the headland crosses dry sand
    /// that slopes gently into the sea.
    #[test]
    fn beach_ring() {
        for seed in SEEDS {
            let t = Terrain::build(seed, &[], &[]);
            let mut beaches = 0;
            let mut dirs = 0;
            for k in 0..48 {
                let dir = Vec2::from_angle(k as f32 * TAU / 48.0);
                if dir.dot(t.plan.headland) > 0.8f32.cos() {
                    continue;
                }
                dirs += 1;
                let shore = (0..200).map(|i| i as f32 * 0.25).find(|r| t.is_sea(dir * *r)).expect("reaches the sea");
                assert!(shore > 9.0 && shore < GLADE_HALF, "seed {seed} shore at {shore}");
                // 2 m inland is low dry beach; 3 m out is shallow sea; the slope is gentle.
                let (inland, out) = (t.height(dir * (shore - 2.0)), t.height(dir * (shore + 3.0)));
                if (0.0..BEACH_TOP).contains(&inland) && (-1.5..0.0).contains(&out) {
                    beaches += 1;
                }
            }
            assert!(beaches * 10 >= dirs * 9, "seed {seed}: {beaches}/{dirs} beach");
            let g = t.ground_mesh();
            let sand = g.surfaces.iter().filter(|s| **s == Surface::Sand as u8).count();
            assert!(sand > g.surfaces.len() / 10, "seed {seed}");
        }
    }

    #[test]
    fn headland_cove_and_stacks() {
        for seed in SEEDS {
            let t = Terrain::build(seed, &[], &[]);
            let plan = t.plan.clone();
            assert_eq!(plan, IslandPlan::new(seed));
            // The cove cuts in: sea much closer to the middle than elsewhere.
            let first_sea = |dir: Vec2| (0..200).map(|i| i as f32 * 0.25).find(|r| t.is_sea(dir * *r)).unwrap();
            let cove = first_sea(plan.cove);
            assert!(cove < 17.0 && cove > 9.0, "seed {seed} cove shore {cove}");
            assert!(first_sea(-plan.cove) > cove + 5.0);
            // Headland: a high plateau (~7 m) ending in steep cliffs.
            let top = (20..120).map(|i| t.height(plan.headland * i as f32 * 0.25)).fold(f32::MIN, f32::max);
            assert!((5.0..8.5).contains(&top), "seed {seed} headland top {top}");
            let steep = (40..150).any(|i| t.normal(plan.headland * i as f32 * 0.25).y < 0.5);
            assert!(steep, "seed {seed}: no cliff");
            // 2-4 stacks, standing out of the sea with water all around.
            assert!((2..=4).contains(&plan.stacks.len()));
            for &(c, rad, top) in &plan.stacks {
                assert!(c.x.abs() < EXTENT - 4.0 && c.y.abs() < EXTENT - 4.0);
                assert!(t.height(c) > top - 1.0 && t.height(c) > 3.0, "seed {seed} stack {c}");
                for k in 0..12 {
                    let q = c + Vec2::from_angle(k as f32 * TAU / 12.0) * (rad * 1.3 + 1.5);
                    assert!(t.is_sea(q), "seed {seed} stack {c} not surrounded at {q}");
                }
            }
        }
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
        assert!(t.water_level(Vec2::new(7.0, 0.0)).is_none());
        let w = t.water_mesh();
        assert!(w.is_valid() && !w.is_empty());
        assert!(w.positions.iter().all(|p| (p[1] - lv).abs() < 1e-4));
        assert!(w.normals.iter().all(|n| n[1] > 0.99));
        assert!(t.ground_mesh().is_valid());
    }

    #[test]
    fn sea_has_sea_level_and_lagoons_keep_their_own() {
        // Dig a hole well below sea level inside the island, and a lagoon elsewhere.
        let d = [
            dab(6.0, 6.0, 4.0, TerrainOp::Sculpt { amount: -6.0 }),
            dab(-6.0, -6.0, 7.0, TerrainOp::Sculpt { amount: 2.0 }),
            dab(-6.0, -6.0, 4.0, TerrainOp::Water { depth: 1.0 }),
        ];
        let t = Terrain::build(5, &d, &[]);
        let dug = Vec2::new(6.0, 6.0);
        assert!(t.is_sea(dug));
        assert_eq!(t.water_level(dug), Some(SEA_LEVEL));
        // Beyond the island the seabed falls away: sea too.
        let off = -t.plan().headland * (GLADE_HALF + 4.0);
        assert!(t.is_sea(off) && t.water_level(off) == Some(SEA_LEVEL));
        // Dry ground is neither.
        let dry = Vec2::new(-6.0, 6.0);
        assert!(!t.is_sea(dry) && t.water_level(dry).is_none());
        // The lagoon keeps its own (higher) level.
        let lagoon = Vec2::new(-6.0, -6.0);
        let lv = t.water_level(lagoon).expect("lagoon");
        assert!(!t.is_sea(lagoon) && lv > SEA_LEVEL && t.height(lagoon) < lv);
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
        let r = FlattenRegion { poly: sq.clone(), height: None, margin: 2.0, keep: false };
        let d = [dab(0.0, 0.0, 8.0, TerrainOp::Sculpt { amount: 2.0 }), dab(0.0, 0.0, 4.0, TerrainOp::Water { depth: 1.0 })];
        let t = Terrain::build(2, &d, &[r]);
        let hs: Vec<f32> = (-4..=4).flat_map(|x| (-4..=4).map(move |z| (x, z))).map(|(x, z)| t.height(Vec2::new(x as f32 * 0.7, z as f32 * 0.7))).collect();
        let (mn, mx) = hs.iter().fold((f32::MAX, f32::MIN), |(a, b), h| (a.min(*h), b.max(*h)));
        assert!(mx - mn < 0.05, "{mn} {mx}");
        assert!(t.water_level(Vec2::ZERO).is_none());
        let r2 = FlattenRegion { poly: sq, height: Some(1.25), margin: 0.0, keep: false };
        assert!((Terrain::build(2, &[], &[r2]).height(Vec2::ZERO) - 1.25).abs() < 1e-4);
    }

    /// A row of huts side by side on a slope: neighbours whose own levels are close share one
    /// terrace, but no hut is levelled further than `TERRACE_DEV` from its own ground.
    #[test]
    fn terraces_stay_near_their_own_ground() {
        // Flat land at 1 m, then the flank of a wide mound rising to the west.
        let base = Vec2::new(-16.0, 0.0);
        for amount in [4.0, 0.8] {
            let d = [dab(0.0, 0.0, 60.0, TerrainOp::Flatten { height: 1.0, strength: 1.0 }), dab(-45.0, 0.0, 60.0, TerrainOp::Sculpt { amount })];
            let raw = Terrain::build(7, &d, &[]);
            let rect = |i: usize| {
                let c = base + Vec2::new(i as f32 * 4.3, 0.0);
                vec![c + Vec2::new(-2.0, -1.5), c + Vec2::new(2.0, -1.5), c + Vec2::new(2.0, 1.5), c + Vec2::new(-2.0, 1.5)]
            };
            let regions: Vec<FlattenRegion> = (0..8).map(|i| FlattenRegion { poly: rect(i), height: None, margin: 1.5, keep: false }).collect();
            let t = Terrain::build(7, &d, &regions);
            let mean = |t: &Terrain, i: usize| {
                let c = base + Vec2::new(i as f32 * 4.3, 0.0);
                let pts: Vec<Vec2> = (-3..=3).flat_map(|x| (-2..=2).map(move |z| c + Vec2::new(x as f32 * 0.6, z as f32 * 0.6))).collect();
                pts.iter().map(|p| t.height(*p)).sum::<f32>() / pts.len() as f32
            };
            let levels: Vec<f32> = (0..8).map(|i| mean(&t, i)).collect();
            for i in 0..8 {
                let dev = levels[i] - mean(&raw, i);
                assert!(dev.abs() < TERRACE_DEV + 0.1, "hut {i} moved {dev} (sculpt {amount})");
            }
            assert!(levels.windows(2).any(|w| (w[0] - w[1]).abs() < 0.02), "neighbours share: {levels:?}");
        }
    }

    #[test]
    fn paint_and_clear() {
        let d = [
            dab(0.0, 0.0, 4.0, TerrainOp::Paint { layer: PaintLayer::SeaPinks, strength: 2.0 }),
            dab(0.0, 0.0, 4.0, TerrainOp::Paint { layer: PaintLayer::Pebbles, strength: 0.6 }),
        ];
        let t = Terrain::build(1, &d, &[]);
        assert_eq!(t.paint(PaintLayer::SeaPinks, Vec2::ZERO), 1.0);
        assert!(t.paint(PaintLayer::Pebbles, Vec2::ZERO) > 0.5);
        assert_eq!(t.paint(PaintLayer::SeaPinks, Vec2::new(10.0, 0.0)), 0.0);
        assert!(t.ground_mesh().surfaces.contains(&(Surface::Pebble as u8)));
        let mut d2 = d.to_vec();
        d2.push(dab(0.0, 0.0, 4.0, TerrainOp::Paint { layer: PaintLayer::SeaPinks, strength: -0.4 }));
        assert!((Terrain::build(1, &d2, &[]).paint(PaintLayer::SeaPinks, Vec2::ZERO) - 0.6).abs() < 1e-5);
        d2.push(dab(0.0, 0.0, 4.0, TerrainOp::Paint { layer: PaintLayer::Clear, strength: 1.0 }));
        let c = Terrain::build(1, &d2, &[]);
        assert_eq!(c.paint(PaintLayer::SeaPinks, Vec2::ZERO), 0.0);
        assert_eq!(c.paint(PaintLayer::Pebbles, Vec2::ZERO), 0.0);
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

    /// ASCII map of an island as the default camera sees it (u right, w towards the viewer).
    /// `cargo test -p cairn_cove_core --release -- --ignored --nocapture island_map`
    #[test]
    #[ignore]
    fn island_map() {
        for seed in [7u64, 1, 42] {
            let t = Terrain::build(seed, &[], &[]);
            println!("seed {seed} cove {:?} head {:?} stacks {:?}", t.plan.cove, t.plan.headland, t.plan.stacks);
            for wi in -20..=20 {
                let row: String = (-30..=30)
                    .map(|ui| {
                        let (u, w) =(ui as f32, wi as f32 * 1.5);
                        let p = Vec2::new(u + w, w - u) * std::f32::consts::FRAC_1_SQRT_2;
                        let h = t.height(p);
                        if h < -3.0 { '~' } else if h < 0.0 { '-' } else if h < BEACH_TOP { '.' } else if h < 2.0 { ',' } else if h < 3.2 { ';' } else if h < 5.0 { '#' } else { '@' }
                    })
                    .collect();
                println!("{row}");
            }
        }
    }

    #[test]
    fn many_dabs() {
        let d: Vec<_> = (0..1000)
            .map(|k| {
                let op = if k % 3 == 0 { TerrainOp::Sculpt { amount: 0.1 } } else { TerrainOp::Paint { layer: PaintLayer::Wrack, strength: 0.2 } };
                dab((k % 40) as f32 - 20.0, (k / 40) as f32 - 12.0, 3.0, op)
            })
            .collect();
        assert!(Terrain::build(4, &d, &[]).ground_mesh().is_valid());
    }
}
