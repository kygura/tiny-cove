//! Structures + seed -> voxel grid. Pure and deterministic.
//!
//! All randomness is hashed from the world seed and voxel/geometry coordinates only,
//! never from structure ids or placement order, so a live preview of a structure
//! rasterizes exactly like the committed one.

use crate::structure::{Structure, World, polyline_len};
use crate::voxel::{GROUND_Y, SIZE_X, SIZE_Y, SIZE_Z, VOXEL_SIZE, Voxel, VoxelGrid, voxel_to_world};
use bevy::math::{IVec3, Vec2, Vec3};

/// Wall thickness in voxels (normal, dam).
const WALL_T: f32 = 3.0;
const DAM_T: f32 = 5.0;
/// Road width in voxels and clearance height of the carved opening.
const ROAD_W: f32 = 6.0;
const ROAD_CLEAR: i32 = 8;
/// Tallest block body; leaves room for antennas and stacks above the roof.
const MAX_BLOCK_H: i32 = SIZE_Y - GROUND_Y - 14;

// Hash salts, one per decision so features don't correlate.
const S_SCORCH: u64 = 0x5c0;
const S_CRATER: u64 = 0xc7a;
const S_TREE: u64 = 0x7ee;
const S_RUBBLE: u64 = 0x7bb;
const S_WALL: u64 = 0xa11;
const S_BLOCK: u64 = 0xb10;

/// Builds the full voxel grid for a world: terrain, walls/blocks in order, roads last.
pub fn generate(world: &World) -> VoxelGrid {
    let mut grid = terrain(world.seed);
    let structures = world.placed.iter().map(|p| &p.structure);
    for s in structures.clone().filter(|s| !matches!(s, Structure::Road { .. })) {
        apply(&mut grid, s, world.seed);
    }
    for s in structures.filter(|s| matches!(s, Structure::Road { .. })) {
        apply(&mut grid, s, world.seed);
    }
    grid
}

/// True if the ground-plane point `p` (world x/z) is within `radius` of the
/// structure's footprint. Used by the demolish tool.
pub fn hits(structure: &Structure, p: Vec2, radius: f32) -> bool {
    match structure {
        Structure::Wall { points, dam, .. } => {
            let t = if *dam { DAM_T } else { WALL_T };
            polyline_len(points) >= 2.0 * VOXEL_SIZE
                && polyline_dist(points, p) <= t * VOXEL_SIZE / 2.0 + radius
        }
        Structure::Block { min, max, .. } => {
            let (lo, hi) = (min.min(*max) - radius, min.max(*max) + radius);
            p.cmpge(lo).all() && p.cmple(hi).all()
        }
        Structure::Road { points } => {
            polyline_dist(points, p) <= ROAD_W * VOXEL_SIZE / 2.0 + radius
        }
    }
}

/// Rasterizes one structure into `grid`.
pub fn apply(grid: &mut VoxelGrid, s: &Structure, seed: u64) {
    match s {
        Structure::Wall { points, height, dam } => wall(grid, points, *height as i32, *dam, seed),
        Structure::Block { min, max, height } => block(grid, *min, *max, *height as i32, seed),
        Structure::Road { points } => road(grid, points),
    }
}

// ---------------------------------------------------------------- hashing / noise

fn mix(mut z: u64) -> u64 {
    z = z.wrapping_add(0x9e37_79b9_7f4a_7c15);
    z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    z ^ (z >> 31)
}

fn hash(seed: u64, a: i32, b: i32, c: i32) -> u64 {
    mix(seed ^ mix(a as u32 as u64 ^ mix((b as u32 as u64) << 32 | c as u32 as u64)))
}

/// Uniform in [0, 1).
fn unit(h: u64) -> f32 {
    (h >> 40) as f32 / (1u64 << 24) as f32
}

fn rnd(seed: u64, a: i32, b: i32, c: i32) -> f32 {
    unit(hash(seed, a, b, c))
}

/// Smooth 2D value noise in [0, 1), lattice spacing 1.
fn value_noise(seed: u64, x: f32, z: f32) -> f32 {
    let (xi, zi) = (x.floor() as i32, z.floor() as i32);
    let (fx, fz) = (x - xi as f32, z - zi as f32);
    let (sx, sz) = (fx * fx * (3.0 - 2.0 * fx), fz * fz * (3.0 - 2.0 * fz));
    let v = |dx, dz| rnd(seed, xi + dx, zi + dz, 0);
    let a = v(0, 0) + (v(1, 0) - v(0, 0)) * sx;
    let b = v(0, 1) + (v(1, 1) - v(0, 1)) * sx;
    a + (b - a) * sz
}

// ---------------------------------------------------------------- grid helpers

fn at(x: i32, y: i32, z: i32) -> IVec3 {
    IVec3::new(x, y, z)
}

/// Prepares a footprint column: removes trees/rubble above ground and fills
/// any crater hollow below ground with a concrete foundation (y >= 1).
fn site_column(grid: &mut VoxelGrid, x: i32, z: i32) {
    for y in 1..GROUND_Y {
        if grid.get(at(x, y, z)) == Voxel::Air {
            grid.set(at(x, y, z), Voxel::ConcreteDark);
        }
    }
    for y in GROUND_Y..SIZE_Y {
        if matches!(grid.get(at(x, y, z)), Voxel::DeadWood | Voxel::Rubble) {
            grid.set(at(x, y, z), Voxel::Air);
        }
    }
}

fn fill_column(grid: &mut VoxelGrid, x: i32, z: i32, y0: i32, y1: i32, v: Voxel) {
    for y in y0..y1 {
        grid.set(at(x, y, z), v);
    }
}

// ---------------------------------------------------------------- polylines

/// World point -> continuous voxel-space x/z (voxel (x, z) spans [x, x+1)).
fn to_voxel_space(p: Vec2) -> Vec2 {
    p / VOXEL_SIZE + Vec2::new((SIZE_X / 2) as f32, (SIZE_Z / 2) as f32)
}

/// Distance from `p` to segment ab and the clamped parameter along it.
fn seg_dist(p: Vec2, a: Vec2, b: Vec2) -> (f32, f32) {
    let ab = b - a;
    let l2 = ab.length_squared();
    let t = if l2 > 0.0 { ((p - a).dot(ab) / l2).clamp(0.0, 1.0) } else { 0.0 };
    (p.distance(a + ab * t), t)
}

fn polyline_dist(points: &[Vec2], p: Vec2) -> f32 {
    match points {
        [] => f32::INFINITY,
        [a] => p.distance(*a),
        _ => points.windows(2).map(|w| seg_dist(p, w[0], w[1]).0).fold(f32::INFINITY, f32::min),
    }
}

/// Converts a freehand world path to a clean voxel-space polyline: drops points
/// closer than one voxel, then Douglas-Peucker removes mouse jitter.
fn clean_path(points: &[Vec2]) -> Vec<Vec2> {
    let mut out: Vec<Vec2> = Vec::with_capacity(points.len());
    for p in points.iter().map(|&p| to_voxel_space(p)) {
        if out.last().is_none_or(|l| l.distance(p) >= 1.0) {
            out.push(p);
        }
    }
    if let (Some(&last), Some(&end)) = (out.last(), points.last())
        && out.len() > 1
        && last != to_voxel_space(end)
    {
        // Keep the true end point so the wall reaches where the drag stopped.
        *out.last_mut().unwrap() = to_voxel_space(end);
    }
    if out.len() <= 2 {
        return out;
    }
    let mut keep = vec![false; out.len()];
    keep[0] = true;
    keep[out.len() - 1] = true;
    let mut stack = vec![(0, out.len() - 1)];
    while let Some((a, b)) = stack.pop() {
        let (mut best, mut bi) = (0.0, 0);
        for i in a + 1..b {
            let d = seg_dist(out[i], out[a], out[b]).0;
            if d > best {
                (best, bi) = (d, i);
            }
        }
        if best > 0.75 {
            keep[bi] = true;
            stack.push((a, bi));
            stack.push((bi, b));
        }
    }
    out.into_iter().zip(keep).filter_map(|(p, k)| k.then_some(p)).collect()
}

/// Per-column query against a voxel-space polyline: distance and arc length
/// of the nearest point.
struct Path {
    pts: Vec<Vec2>,
    cum: Vec<f32>,
    len: f32,
}

impl Path {
    fn new(pts: Vec<Vec2>) -> Self {
        let mut cum = vec![0.0];
        for w in pts.windows(2) {
            cum.push(cum.last().unwrap() + w[0].distance(w[1]));
        }
        let len = *cum.last().unwrap();
        Self { pts, cum, len }
    }

    fn nearest(&self, p: Vec2) -> (f32, f32) {
        if self.pts.len() == 1 {
            return (p.distance(self.pts[0]), 0.0);
        }
        let mut best = (f32::INFINITY, 0.0);
        for (i, w) in self.pts.windows(2).enumerate() {
            let (d, t) = seg_dist(p, w[0], w[1]);
            if d < best.0 {
                best = (d, self.cum[i] + t * (self.cum[i + 1] - self.cum[i]));
            }
        }
        best
    }

    fn point_at(&self, s: f32) -> Vec2 {
        for (i, w) in self.pts.windows(2).enumerate() {
            let l = self.cum[i + 1] - self.cum[i];
            if s <= self.cum[i + 1] && l > 0.0 {
                return w[0].lerp(w[1], ((s - self.cum[i]) / l).clamp(0.0, 1.0));
            }
        }
        *self.pts.last().unwrap()
    }

    /// Visits every grid column whose centre is within `r` of the path.
    fn for_columns(&self, r: f32, mut f: impl FnMut(i32, i32, f32, f32)) {
        let lo = self.pts.iter().fold(Vec2::INFINITY, |a, &p| a.min(p)) - r;
        let hi = self.pts.iter().fold(Vec2::NEG_INFINITY, |a, &p| a.max(p)) + r;
        let (x0, z0) = ((lo.x.floor() as i32).max(0), (lo.y.floor() as i32).max(0));
        let (x1, z1) = ((hi.x.ceil() as i32).min(SIZE_X - 1), (hi.y.ceil() as i32).min(SIZE_Z - 1));
        for z in z0..=z1 {
            for x in x0..=x1 {
                let (d, s) = self.nearest(Vec2::new(x as f32 + 0.5, z as f32 + 0.5));
                if d <= r {
                    f(x, z, d, s);
                }
            }
        }
    }
}

// ---------------------------------------------------------------- terrain

/// Ash plain with scorch patches, craters, dead trees and rubble. y = 0 is always solid.
pub fn terrain(seed: u64) -> VoxelGrid {
    let mut g = VoxelGrid::new();
    for z in 0..SIZE_Z {
        for x in 0..SIZE_X {
            let (fx, fz) = (x as f32, z as f32);
            let n = value_noise(seed ^ S_SCORCH, fx / 14.0, fz / 14.0) * 0.7
                + value_noise(seed ^ S_SCORCH ^ 1, fx / 4.0, fz / 4.0) * 0.3;
            for y in 0..GROUND_Y {
                let scorch = n > 0.66 && (y == GROUND_Y - 1 || n > 0.75);
                g.set(at(x, y, z), if scorch { Voxel::Scorch } else { Voxel::Ash });
            }
        }
    }

    let (cx, cz) = ((SIZE_X / 2) as f32, (SIZE_Z / 2) as f32);
    let craters = 3 + hash(seed ^ S_CRATER, 0, 0, 0) % 3;
    for i in 0..craters as i32 {
        let r = 5.0 + rnd(seed ^ S_CRATER, i, 1, 0) * 7.0;
        let ang = rnd(seed ^ S_CRATER, i, 2, 0) * std::f32::consts::TAU;
        // Rim edge at least 62 voxels (31 world units) from the centre.
        let dist = 62.0 + r + rnd(seed ^ S_CRATER, i, 3, 0) * 12.0;
        let c = Vec2::new(cx + ang.cos() * dist, cz + ang.sin() * dist);
        crater(&mut g, seed, c, r);
    }

    let trees = 18 + hash(seed ^ S_TREE, 0, 0, 0) % 12;
    for i in 0..trees as i32 {
        let x = (rnd(seed ^ S_TREE, i, 1, 0) * SIZE_X as f32) as i32;
        let z = (rnd(seed ^ S_TREE, i, 2, 0) * SIZE_Z as f32) as i32;
        dead_tree(&mut g, seed ^ S_TREE ^ (i as u64) << 8, x, z);
    }

    let piles = 12 + hash(seed ^ S_RUBBLE, 0, 0, 0) % 10;
    for i in 0..piles as i32 {
        let c = Vec2::new(rnd(seed ^ S_RUBBLE, i, 1, 0), rnd(seed ^ S_RUBBLE, i, 2, 0)) * SIZE_X as f32;
        let r = 1.0 + rnd(seed ^ S_RUBBLE, i, 3, 0) * 2.5;
        rubble_pile(&mut g, seed, c, r);
    }
    g
}

fn crater(g: &mut VoxelGrid, seed: u64, c: Vec2, r: f32) {
    let outer = r * 1.7;
    for z in (c.y - outer) as i32..=(c.y + outer) as i32 {
        for x in (c.x - outer) as i32..=(c.x + outer) as i32 {
            let d = Vec2::new(x as f32 + 0.5, z as f32 + 0.5).distance(c) / r;
            let h = rnd(seed ^ S_CRATER, x, z, 7);
            if d < 1.0 {
                let depth = ((1.0 - d * d) * (GROUND_Y - 1) as f32).round() as i32;
                if depth > 0 {
                    fill_column(g, x, z, GROUND_Y - depth, GROUND_Y, Voxel::Air);
                    g.set(at(x, GROUND_Y - depth - 1, z), Voxel::Scorch);
                }
            } else if d < 1.7 {
                if h < 1.6 - d {
                    g.set(at(x, GROUND_Y - 1, z), Voxel::Scorch);
                }
                // Thrown-up rim, densest right at the lip.
                if d < 1.35 && h < 0.75 - (d - 1.0) {
                    g.set(at(x, GROUND_Y, z), Voxel::Rubble);
                    if h < 0.25 {
                        g.set(at(x, GROUND_Y + 1, z), Voxel::Rubble);
                    }
                }
            }
        }
    }
}

fn dead_tree(g: &mut VoxelGrid, seed: u64, mut x: i32, mut z: i32) {
    let h = 4 + (hash(seed, 0, 0, 1) % 7) as i32;
    let y0 = g.top(x, z) + 1;
    if y0 < 1 {
        return;
    }
    const DIRS: [(i32, i32); 8] = [(1, 0), (-1, 0), (0, 1), (0, -1), (1, 1), (-1, 1), (1, -1), (-1, -1)];
    for k in 0..h {
        let y = y0 + k;
        g.set(at(x, y, z), Voxel::DeadWood);
        // Crooked lean: occasionally step sideways (kept face-connected).
        if k > 1 && k < h - 1 && rnd(seed, k, 1, 0) < 0.25 {
            let (dx, dz) = DIRS[(hash(seed, k, 2, 0) % 4) as usize];
            x += dx;
            z += dz;
            g.set(at(x, y, z), Voxel::DeadWood);
        }
        // Branches in the upper half: short crooked arms climbing outward.
        if k >= h / 2 && rnd(seed, k, 3, 0) < 0.45 {
            let (dx, dz) = DIRS[(hash(seed, k, 4, 0) % 8) as usize];
            let (mut bx, mut by, mut bz) = (x, y, z);
            for j in 0..1 + (hash(seed, k, 5, 0) % 3) as i32 {
                bx += dx;
                bz += dz;
                if j > 0 || rnd(seed, k, 6, j) < 0.5 {
                    by += 1;
                }
                g.set(at(bx, by, bz), Voxel::DeadWood);
            }
        }
    }
}

fn rubble_pile(g: &mut VoxelGrid, seed: u64, c: Vec2, r: f32) {
    for z in (c.y - r) as i32..=(c.y + r) as i32 {
        for x in (c.x - r) as i32..=(c.x + r) as i32 {
            let d = Vec2::new(x as f32 + 0.5, z as f32 + 0.5).distance(c);
            let h = rnd(seed ^ S_RUBBLE, x, z, 9);
            if d > r || h < 0.2 {
                continue;
            }
            let top = g.top(x, z);
            let n = 1 + ((r - d) * 0.8 + h) as i32;
            for y in top + 1..top + 1 + n {
                let v = if h > 0.93 { Voxel::ConcreteDark } else { Voxel::Rubble };
                g.set(at(x, y, z), v);
            }
            if h > 0.97 {
                // Bent rebar poking out.
                g.set(at(x, top + 1 + n, z), Voxel::Rust);
            }
        }
    }
}

// ---------------------------------------------------------------- walls

fn wall(g: &mut VoxelGrid, points: &[Vec2], height: i32, dam: bool, seed: u64) {
    let path = Path::new(clean_path(points));
    if path.pts.len() < 2 || path.len < 2.0 {
        return;
    }
    let seed = seed ^ S_WALL;
    let h = if dam { height.max(32) } else { height }.clamp(4, SIZE_Y - GROUND_Y - 6);
    let t = if dam { DAM_T } else { WALL_T } / 2.0;
    let out = if dam { 3.0 } else { 2.0 };
    let lip_h = if dam { 2 } else { 1 };
    let top = GROUND_Y + h; // first voxel above the walkway
    let win_y = GROUND_Y + h / 2 - 1;
    let len = path.len;

    path.for_columns(t + out + 0.5, |x, z, d, s| {
        site_column(g, x, z);
        let k = d - t;
        if k > 0.0 {
            // Outside the wall body: footing and buttresses.
            if matches!((s.floor() as i32).rem_euclid(10), 4 | 5) && s > 3.0 && s < len - 3.0 && k <= out {
                let step = k.ceil();
                let bh = ((h as f32) * (out + 1.0 - step) / (out + 1.0)) as i32;
                fill_column(g, x, z, GROUND_Y, GROUND_Y + bh - 1, Voxel::Concrete);
                g.set(at(x, GROUND_Y + bh - 1, z), Voxel::ConcreteDark);
            } else if k <= 1.0 {
                fill_column(g, x, z, GROUND_Y, GROUND_Y + 2, Voxel::ConcreteDark);
            }
            return;
        }
        let outer = d > t - 1.0;
        let r = |salt| rnd(seed, x, z, salt);
        let streak = if dam && r(0) < 0.3 { 3 + (r(1) * (h / 2) as f32) as i32 } else { 0 };
        for y in GROUND_Y..top {
            let yy = y - GROUND_Y;
            let band = yy < 2 || yy == h - 2 || (dam && yy % 6 == 5) || (y >= top - streak && outer);
            g.set(at(x, y, z), if band { Voxel::ConcreteDark } else { Voxel::Concrete });
        }
        if outer {
            // Slit windows at mid height on the faces; dams get a second row.
            if (s.floor() as i32).rem_euclid(6) == 3 && s > 2.0 && s < len - 2.0 {
                let rows = if dam { [win_y - 8, win_y] } else { [win_y, win_y] };
                for wy in rows {
                    let lit = rnd(seed, x, z, wy) < 0.15;
                    let v = if lit { Voxel::WindowLit } else { Voxel::WindowDark };
                    fill_column(g, x, z, wy, wy + 3, v);
                }
            }
            // Parapet lips with crenel gaps; posts and rebar now and then.
            if (s.floor() as i32).rem_euclid(4) != 0 {
                fill_column(g, x, z, top, top + lip_h, Voxel::Concrete);
                if r(2) < 0.04 {
                    fill_column(g, x, z, top + lip_h, top + lip_h + 2, Voxel::Rust);
                }
            } else if r(3) < 0.3 {
                g.set(at(x, top, z), Voxel::Rust);
            }
        }
    });

    // Pier towers at both ends make even a short stroke look deliberate.
    if len >= 8.0 {
        let r = t + 1.0;
        for e in [path.pts[0], *path.pts.last().unwrap()] {
            for z in (e.y - r).floor() as i32..=(e.y + r).floor() as i32 {
                for x in (e.x - r).floor() as i32..=(e.x + r).floor() as i32 {
                    site_column(g, x, z);
                    fill_column(g, x, z, GROUND_Y, top + 2, Voxel::Concrete);
                    fill_column(g, x, z, GROUND_Y, GROUND_Y + 2, Voxel::ConcreteDark);
                    g.set(at(x, top + 2, z), Voxel::ConcreteDark);
                }
            }
            let (ex, ez) = (e.x.floor() as i32, e.y.floor() as i32);
            fill_column(g, ex, ez, top + 3, top + 5, Voxel::Rust);
            g.set(at(ex, top + 5, ez), Voxel::Lamp);
        }
    }

    // Floodlights on short rust poles along the walkway.
    let mut s = 8.0;
    while s < len - 4.0 {
        let p = path.point_at(s);
        let (x, z) = (p.x.floor() as i32, p.y.floor() as i32);
        let y = g.top(x, z) + 1;
        fill_column(g, x, z, y, y + 2, Voxel::Rust);
        g.set(at(x, y + 2, z), Voxel::Lamp);
        s += 16.0;
    }
}

// ---------------------------------------------------------------- blocks

struct Rect {
    x0: i32,
    z0: i32,
    x1: i32, // exclusive
    z1: i32,
}

impl Rect {
    fn contains(&self, x: i32, z: i32) -> bool {
        x >= self.x0 && x < self.x1 && z >= self.z0 && z < self.z1
    }

    /// Nearest facade of a cell: (face, index along face, face length, depth),
    /// or None for corners/pilasters and deep interior.
    fn face(&self, x: i32, z: i32) -> Option<(i32, i32, i32, i32)> {
        let ds = [z - self.z0, self.x1 - 1 - x, self.z1 - 1 - z, x - self.x0];
        let (f, &depth) = ds.iter().enumerate().min_by_key(|e| *e.1)?;
        let others = ds.iter().enumerate().filter(|&(i, _)| i != f).map(|e| *e.1).min()?;
        if depth > 1 || others <= depth.max(0) || (depth == 1 && others <= 1) {
            return None;
        }
        let (i, len) = if f % 2 == 0 { (x - self.x0, self.x1 - self.x0) } else { (z - self.z0, self.z1 - self.z0) };
        Some((f as i32, i, len, depth))
    }
}

/// Footprint, hash key and stair core of a block; None if under 4x4 voxels.
struct BlockLayout {
    r: Rect,
    core: Rect,
    key: u64,
    cs: i32,
    side: i32,
}

fn block_layout(min: Vec2, max: Vec2, seed: u64) -> Option<BlockLayout> {
    let (a, b) = (to_voxel_space(min.min(max)), to_voxel_space(min.max(max)));
    let r = Rect {
        x0: (a.x.round() as i32).clamp(0, SIZE_X),
        z0: (a.y.round() as i32).clamp(0, SIZE_Z),
        x1: (b.x.round() as i32).clamp(0, SIZE_X),
        z1: (b.y.round() as i32).clamp(0, SIZE_Z),
    };
    let (w, d) = (r.x1 - r.x0, r.z1 - r.z0);
    if w < 4 || d < 4 {
        return None;
    }
    let key = hash(seed ^ S_BLOCK, r.x0, r.z0, r.x1 * 1000 + r.z1);
    // Stair/elevator core straddling one facade.
    let cs = if w.min(d) >= 10 { 3 } else { 2 };
    let side = (key % 4) as i32;
    let (cx, cz) = (r.x0 + (w - cs) / 2, r.z0 + (d - cs) / 2);
    let core = match side {
        0 => Rect { x0: cx, z0: r.z0 - 1, x1: cx + cs, z1: r.z0 - 1 + cs },
        1 => Rect { x0: r.x1 + 1 - cs, z0: cz, x1: r.x1 + 1, z1: cz + cs },
        2 => Rect { x0: cx, z0: r.z1 + 1 - cs, x1: cx + cs, z1: r.z1 + 1 },
        _ => Rect { x0: r.x0 - 1, z0: cz, x1: r.x0 - 1 + cs, z1: cz + cs },
    };
    Some(BlockLayout { r, core, key, cs, side })
}

/// Roof height (voxel y of the roof slab) for a requested block height.
fn block_top(height: i32) -> i32 {
    GROUND_Y + height.clamp(6, MAX_BLOCK_H) - 1
}

const STACK_H: i32 = 12;

/// Min-corner voxel x/z of the 2x2 rooftop smokestack, if this block gets one.
fn stack_cell(l: &BlockLayout) -> Option<(i32, i32)> {
    let (r, core) = (&l.r, &l.core);
    let (ix0, iz0, ix1, iz1) = (r.x0 + 2, r.z0 + 2, r.x1 - 2, r.z1 - 2);
    let (iw, id) = (ix1 - ix0, iz1 - iz0);
    if iw < 2 || id < 2 || (r.x1 - r.x0) * (r.z1 - r.z0) <= 200 {
        return None;
    }
    let pick = |k: i32, n: i32, m: i32| (hash(l.key, k, n, 0) % m.max(1) as u64) as i32;
    let (sx, sz) = (ix0 + pick(90, 0, iw - 1), iz0 + pick(90, 1, id - 1));
    (roof_free(core, sx, sz) && roof_free(core, sx + 1, sz + 1)).then_some((sx, sz))
}

fn roof_free(core: &Rect, x: i32, z: i32) -> bool {
    !(x >= core.x0 - 1 && x <= core.x1 && z >= core.z0 - 1 && z <= core.z1)
}

/// World-space top centre of a block's smokestack, if it has one.
pub fn stack_pos(min: Vec2, max: Vec2, height: u32, seed: u64) -> Option<Vec3> {
    let l = block_layout(min, max, seed)?;
    let (sx, sz) = stack_cell(&l)?;
    let top = (block_top(height as i32) + 1 + STACK_H).min(SIZE_Y);
    Some(voxel_to_world(at(sx + 1, top, sz + 1)))
}

fn block(g: &mut VoxelGrid, min: Vec2, max: Vec2, height: i32, seed: u64) {
    let Some(layout) = block_layout(min, max, seed) else { return };
    let (r, core, key, cs, side) = (&layout.r, &layout.core, layout.key, layout.cs, layout.side);
    let (w, d) = (r.x1 - r.x0, r.z1 - r.z0);
    let seed = seed ^ S_BLOCK;
    let ytop = block_top(height); // roof slab
    let pilotis = w >= 8 && d >= 8;
    // Bands sit at rel % 3 == 0; rel 0 is the transfer slab / plinth top.
    let base = if pilotis { GROUND_Y + 3 } else { GROUND_Y + 1 };

    for z in r.z0 - 1..=r.z1 {
        for x in r.x0 - 1..=r.x1 {
            let inside = r.contains(x, z);
            if inside {
                site_column(g, x, z);
            }
            for y in GROUND_Y..=ytop + 1 {
                let rel = y - base;
                let v = if !inside {
                    // Protruding slab edges.
                    (y >= base && y <= ytop && (rel % 3 == 0 || y == ytop)).then_some(Voxel::ConcreteDark)
                } else if y > ytop {
                    let ring = x == r.x0 || z == r.z0 || x == r.x1 - 1 || z == r.z1 - 1;
                    ring.then_some(Voxel::Concrete)
                } else if y < base {
                    let on_x = (x - r.x0) % 4 == 0 || x == r.x1 - 1;
                    let on_z = (z - r.z0) % 4 == 0 || z == r.z1 - 1;
                    let edge_x = x == r.x0 || x == r.x1 - 1;
                    let edge_z = z == r.z0 || z == r.z1 - 1;
                    let column = (edge_x && on_z) || (edge_z && on_x);
                    (!pilotis || column).then_some(Voxel::ConcreteDark)
                } else if rel % 3 == 0 || y == ytop {
                    Some(Voxel::ConcreteDark)
                } else {
                    facade(r, seed, x, z, rel)
                };
                if let Some(v) = v {
                    g.set(at(x, y, z), v);
                }
            }
        }
    }

    let ext = 4 + (key >> 8) as i32 % 3;
    for z in core.z0..core.z1 {
        for x in core.x0..core.x1 {
            site_column(g, x, z);
            fill_column(g, x, z, GROUND_Y, ytop + ext + 1, Voxel::ConcreteDark);
        }
    }
    let mid = cs / 2;
    let (sx, sz) = match side {
        0 => (core.x0 + mid, core.z0),
        1 => (core.x1 - 1, core.z0 + mid),
        2 => (core.x0 + mid, core.z1 - 1),
        _ => (core.x0, core.z0 + mid),
    };
    fill_column(g, sx, sz, base + 1, ytop + ext - 1, Voxel::WindowLit);

    roof(g, &layout, ytop + 1);
}

/// Facade cell between floor bands: windows, balconies, or plain concrete.
fn facade(r: &Rect, seed: u64, x: i32, z: i32, rel: i32) -> Option<Voxel> {
    let Some((f, i, len, depth)) = r.face(x, z) else {
        return Some(Voxel::Concrete);
    };
    let floor = rel / 3;
    let bay = i / 4;
    let balcony = i % 4 != 0
        && bay * 4 + 3 <= len - 2
        && rnd(seed, f * 7919 + bay, floor, r.x0 * 400 + r.z0 + 1) < 0.1;
    let lit = |wi: i32| rnd(seed, x * 7 + z * 13 + wi, floor, f) < 0.25;
    let window = |wi| Some(if lit(wi) { Voxel::WindowLit } else { Voxel::WindowDark });
    match (depth, balcony) {
        (0, true) if rel % 3 == 1 => Some(Voxel::Rust),
        (0, true) => None,
        (1, true) if i % 4 == 2 => window(1),
        (0, false) if i % 2 == 1 && i < len - 1 => window(0),
        _ => Some(Voxel::Concrete),
    }
}

/// Machinery, tanks, antennas and (for large blocks) a smokestack on the roof at `y`.
fn roof(g: &mut VoxelGrid, l: &BlockLayout, y: i32) {
    let (r, core, key) = (&l.r, &l.core, l.key);
    let (ix0, iz0, ix1, iz1) = (r.x0 + 2, r.z0 + 2, r.x1 - 2, r.z1 - 2);
    if ix1 - ix0 < 2 || iz1 - iz0 < 2 {
        return;
    }
    let (iw, id) = (ix1 - ix0, iz1 - iz0);
    let area = (r.x1 - r.x0) * (r.z1 - r.z0);
    let stack = stack_cell(l);
    // Machinery keeps clear of the core and the smokestack.
    let free = |x: i32, z: i32| {
        roof_free(core, x, z) && stack.is_none_or(|(sx, sz)| !((sx - 1..=sx + 2).contains(&x) && (sz - 1..=sz + 2).contains(&z)))
    };
    let pick = |k: i32, n: i32, m: i32| (hash(key, k, n, 0) % m.max(1) as u64) as i32;

    if let Some((sx, sz)) = stack {
        for (dx, dz) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
            fill_column(g, sx + dx, sz + dz, y, (y + STACK_H).min(SIZE_Y), Voxel::ConcreteDark);
        }
    }

    let n = (area / 60).clamp(1, 8);
    for k in 0..n {
        let (x, z) = (ix0 + pick(k, 1, iw), iz0 + pick(k, 2, id));
        if !free(x, z) {
            continue;
        }
        match pick(k, 3, 3) {
            0 => {
                let (bw, bd, bh) = (2 + pick(k, 4, 3), 2 + pick(k, 5, 2), 1 + pick(k, 6, 2));
                let v = if pick(k, 7, 2) == 0 { Voxel::ConcreteDark } else { Voxel::Rust };
                for bz in z..(z + bd).min(iz1) {
                    for bx in x..(x + bw).min(ix1) {
                        if free(bx, bz) {
                            fill_column(g, bx, bz, y, y + bh, v);
                        }
                    }
                }
            }
            1 => {
                let th = 2 + pick(k, 4, 2);
                let c = Vec2::new(x as f32 + 1.0, z as f32 + 1.0);
                for tz in z - 1..z + 3 {
                    for tx in x - 1..x + 3 {
                        let inside = Vec2::new(tx as f32 + 0.5, tz as f32 + 0.5).distance(c) < 1.3;
                        if inside && tx >= ix0 && tx < ix1 && tz >= iz0 && tz < iz1 && free(tx, tz) {
                            g.set(at(tx, y, tz), Voxel::ConcreteDark);
                            fill_column(g, tx, tz, y + 1, y + 1 + th, Voxel::Rust);
                        }
                    }
                }
            }
            _ => {
                let ah = 4 + pick(k, 4, 7);
                fill_column(g, x, z, y, (y + ah).min(SIZE_Y - 1), Voxel::Rust);
                g.set(at(x, (y + ah).min(SIZE_Y - 1), z), Voxel::Lamp);
            }
        }
    }
}

// ---------------------------------------------------------------- roads

/// Asphalt strip with kerbs. Where it cuts through structures it carves a gate:
/// dark jambs with a lamp on top and a lintel above.
fn road(g: &mut VoxelGrid, points: &[Vec2]) {
    let path = Path::new(clean_path(points));
    if path.pts.len() < 2 || path.len < 2.0 {
        return;
    }
    let half = ROAD_W / 2.0;
    let lintel = GROUND_Y + ROAD_CLEAR;
    let mut carved_cols = Vec::new();
    let mut edges = Vec::new();
    path.for_columns(half + 1.0, |x, z, d, _| {
        if d > half {
            edges.push((x, z));
            return;
        }
        for y in 1..GROUND_Y - 1 {
            if g.get(at(x, y, z)) == Voxel::Air {
                g.set(at(x, y, z), Voxel::Ash);
            }
        }
        g.set(at(x, GROUND_Y - 1, z), Voxel::Asphalt);
        let mut carved = false;
        for y in GROUND_Y..lintel {
            carved |= g.get(at(x, y, z)).is_structure();
            g.set(at(x, y, z), Voxel::Air);
        }
        if carved {
            carved_cols.push((x, z));
            if g.get(at(x, lintel, z)).is_structure() {
                g.set(at(x, lintel, z), Voxel::ConcreteDark);
            }
        }
    });
    for (x, z) in edges {
        let mut jamb = false;
        for y in GROUND_Y..lintel {
            if g.get(at(x, y, z)).is_structure() {
                g.set(at(x, y, z), Voxel::ConcreteDark);
                jamb = true;
            }
        }
        let beside_gate = carved_cols.iter().any(|&(cx, cz)| (cx - x).abs() + (cz - z).abs() == 1);
        if jamb && beside_gate && g.get(at(x, lintel, z)).is_structure() {
            g.set(at(x, lintel - 1, z), Voxel::Lamp);
        } else if !jamb && g.get(at(x, GROUND_Y - 1, z)) != Voxel::Asphalt {
            // Kerb, except where it would cross another road.
            g.set(at(x, GROUND_Y, z), Voxel::ConcreteDark);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::voxel::world_to_voxel;
    use bevy::math::Vec3;

    const SEED: u64 = 42;

    fn wall_s(points: &[(f32, f32)]) -> Structure {
        Structure::Wall { points: points.iter().map(|&(x, z)| Vec2::new(x, z)).collect(), height: 16, dam: false }
    }

    fn block_s(a: (f32, f32), b: (f32, f32), height: u32) -> Structure {
        Structure::Block { min: Vec2::new(a.0, a.1), max: Vec2::new(b.0, b.1), height }
    }

    fn road_s(points: &[(f32, f32)]) -> Structure {
        Structure::Road { points: points.iter().map(|&(x, z)| Vec2::new(x, z)).collect() }
    }

    /// Voxel at world x/z and voxel height y.
    fn vox(g: &VoxelGrid, x: f32, z: f32, y: i32) -> Voxel {
        let p = world_to_voxel(Vec3::new(x, 0.0, z));
        g.get(IVec3::new(p.x, y, p.z))
    }

    fn sample_world() -> World {
        let mut w = World::new(SEED);
        w.add(road_s(&[(-20.0, 5.0), (0.0, 5.0), (20.0, 12.0)]));
        w.add(wall_s(&[(-25.0, -15.0), (-5.0, -10.0), (10.0, -18.0), (25.0, 0.0)]));
        w.add(block_s((5.0, 5.0), (15.0, 15.0), 30));
        w.add(Structure::Wall { points: vec![Vec2::new(-30.0, 20.0), Vec2::new(-10.0, 30.0)], height: 16, dam: true });
        w
    }

    #[test]
    fn generate_is_deterministic() {
        let w = sample_world();
        assert!(generate(&w) == generate(&w));
        assert!(generate(&w) != terrain(SEED));
    }

    #[test]
    fn generate_apply_is_order_independent() {
        // Same structure into grids with different history: identical in its region.
        let s = wall_s(&[(-10.0, 0.0), (5.0, 3.0), (12.0, 10.0)]);
        let mut a = terrain(SEED);
        apply(&mut a, &block_s((-35.0, -35.0), (-28.0, -28.0), 12), SEED);
        apply(&mut a, &s, SEED);
        let mut b = terrain(SEED);
        apply(&mut b, &s, SEED);
        for z in 70..120 {
            for x in 50..120 {
                for y in 0..SIZE_Y {
                    let p = IVec3::new(x, y, z);
                    assert_eq!(a.get(p), b.get(p), "at {p}");
                }
            }
        }
    }

    #[test]
    fn generate_wall_along_path_only() {
        let t = terrain(SEED);
        let mut g = t.clone();
        apply(&mut g, &wall_s(&[(-10.0, 0.0), (10.0, 0.0)]), SEED);
        for x in [-8.0, -3.0, 0.0, 4.0, 8.0] {
            for y in GROUND_Y..GROUND_Y + 14 {
                assert!(vox(&g, x + 0.1, 0.1, y).is_solid(), "gap at x={x} y={y}");
            }
            let p = world_to_voxel(Vec3::new(x + 0.1, 0.0, 0.1));
            assert!((GROUND_Y + 15..GROUND_Y + 20).contains(&g.top(p.x, p.z)), "walkway at x={x}");
        }
        for (x, z) in [(0.0, 10.0), (0.0, -10.0), (20.0, 0.0), (-25.0, 5.0)] {
            for y in 0..SIZE_Y {
                let p = world_to_voxel(Vec3::new(x, 0.0, z));
                let p = IVec3::new(p.x, y, p.z);
                assert_eq!(g.get(p), t.get(p));
            }
        }
    }

    #[test]
    fn generate_short_wall_is_noop() {
        let t = terrain(SEED);
        for pts in [vec![(0.0, 0.0)], vec![(0.0, 0.0), (0.4, 0.3)], vec![]] {
            let mut g = t.clone();
            apply(&mut g, &wall_s(&pts), SEED);
            assert!(g == t);
        }
        let mut g = t.clone();
        apply(&mut g, &block_s((0.0, 0.0), (1.0, 5.0), 20), SEED);
        assert!(g == t, "footprint under 4x4 voxels is ignored");
    }

    #[test]
    fn generate_block_height_and_pilotis() {
        let h = 20;
        let mut g = VoxelGrid::new();
        apply(&mut g, &block_s((-10.0, -10.0), (10.0, 10.0), h as u32), SEED);
        let (x0, x1) = (60, 100);
        // Parapet corner sits exactly one above the roof slab.
        assert_eq!(g.top(x0, x0), GROUND_Y + h);
        let mut max_top = 0;
        for z in x0 - 2..x1 + 2 {
            for x in x0 - 2..x1 + 2 {
                max_top = max_top.max(g.top(x, z));
            }
        }
        assert!(max_top > GROUND_Y + h, "roof stuff or core above roof");
        assert!(max_top <= GROUND_Y + h + 12, "top {max_top}");
        // Pilotis: open under the interior, transfer slab above.
        for y in GROUND_Y..GROUND_Y + 3 {
            assert_eq!(g.get(IVec3::new(70, y, 73)), Voxel::Air);
        }
        assert_eq!(g.get(IVec3::new(70, GROUND_Y + 3, 73)), Voxel::ConcreteDark);
        assert!(g.get(IVec3::new(x0, GROUND_Y, x0)).is_solid(), "corner column");
        // Facade has windows.
        let windows = (0..SIZE_Y)
            .flat_map(|y| (x0..x1).map(move |x| (x, y)))
            .filter(|&(x, y)| matches!(g.get(IVec3::new(x, y, x0)), Voxel::WindowDark | Voxel::WindowLit))
            .count();
        assert!(windows > 20, "{windows}");
    }

    #[test]
    fn generate_blocks_union() {
        let a = block_s((-10.0, -10.0), (4.0, 4.0), 20);
        let b = block_s((0.0, -6.0), (12.0, 10.0), 30);
        let (mut ga, mut gb, mut gab) = (VoxelGrid::new(), VoxelGrid::new(), VoxelGrid::new());
        apply(&mut ga, &a, SEED);
        apply(&mut gb, &b, SEED);
        apply(&mut gab, &a, SEED);
        apply(&mut gab, &b, SEED);
        for z in 0..SIZE_Z {
            for x in 0..SIZE_X {
                for y in 0..SIZE_Y {
                    let p = IVec3::new(x, y, z);
                    if ga.get(p).is_solid() || gb.get(p).is_solid() {
                        assert!(gab.get(p).is_solid(), "hole at {p}");
                    }
                }
            }
        }
    }

    #[test]
    fn generate_road_carves_gate() {
        let mut w = World::new(SEED);
        w.add(road_s(&[(0.0, -10.0), (0.0, 10.0)]));
        w.add(wall_s(&[(-10.0, 0.0), (10.0, 0.0)]));
        let g = generate(&w);
        for y in GROUND_Y..GROUND_Y + ROAD_CLEAR {
            assert_eq!(vox(&g, 0.1, 0.1, y), Voxel::Air);
        }
        assert_eq!(vox(&g, 0.1, 0.1, GROUND_Y - 1), Voxel::Asphalt);
        assert_eq!(vox(&g, 0.1, 0.1, GROUND_Y + ROAD_CLEAR), Voxel::ConcreteDark, "lintel");
        assert!(vox(&g, 0.1, 0.1, GROUND_Y + ROAD_CLEAR + 2).is_solid(), "wall above gate");
        assert!(vox(&g, 5.0, 0.1, GROUND_Y + 1).is_solid(), "wall beside gate");
        let asphalt = (-18..18)
            .filter(|&i| vox(&g, 0.1, i as f32 * 0.5, GROUND_Y - 1) == Voxel::Asphalt)
            .count();
        assert_eq!(asphalt, 36, "no speckle");
        for x in [-1.8, 1.8] {
            assert_eq!(vox(&g, x, -5.0, GROUND_Y), Voxel::ConcreteDark, "kerb at x={x}");
            assert_eq!(vox(&g, x, -5.0, GROUND_Y + 1), Voxel::Air);
            let lamp = (-4..4).any(|i| vox(&g, x, i as f32 * 0.5 + 0.25, GROUND_Y + ROAD_CLEAR - 1) == Voxel::Lamp);
            assert!(lamp, "lamp on jamb at x={x}");
        }
    }

    #[test]
    fn generate_crossing_roads_have_no_kerb_across() {
        let mut w = World::new(SEED);
        w.add(road_s(&[(0.0, -10.0), (0.0, 10.0)]));
        w.add(road_s(&[(-10.0, 0.0), (10.0, 0.0)]));
        let g = generate(&w);
        // Inside the other road's lane (|c| < 1.5) neither kerb survives.
        for i in -3..3 {
            let c = i as f32 * 0.5 + 0.25;
            assert_eq!(vox(&g, c, 1.8, GROUND_Y), Voxel::Air, "x={c}");
            assert_eq!(vox(&g, 1.8, c, GROUND_Y), Voxel::Air, "z={c}");
        }
    }

    #[test]
    fn stack_pos_matches_rooftop_stack() {
        let mut found = 0;
        for i in 0..12 {
            let (min, max, h) = (Vec2::new(-12.0 + i as f32, -10.0), Vec2::new(4.0, 6.0 + i as f32 * 0.5), 20);
            let mut g = VoxelGrid::new();
            apply(&mut g, &Structure::Block { min, max, height: h }, SEED);
            let Some(p) = stack_pos(min, max, h, SEED) else { continue };
            found += 1;
            let v = world_to_voxel(p - Vec3::splat(0.25));
            assert_eq!(g.get(v), Voxel::ConcreteDark, "stack top at {v}");
            assert_eq!(g.get(v + IVec3::Y), Voxel::Air);
            assert_eq!(g.get(v - IVec3::Y * 11), Voxel::ConcreteDark, "stack base");
        }
        assert!(found > 3, "{found}");
        assert_eq!(stack_pos(Vec2::ZERO, Vec2::splat(4.0), 20, SEED), None, "small blocks have none");
    }

    #[test]
    fn generate_hits() {
        let w = wall_s(&[(0.0, 0.0), (10.0, 0.0)]);
        assert!(hits(&w, Vec2::new(5.0, 0.5), 0.0));
        assert!(hits(&w, Vec2::new(5.0, 1.5), 1.0));
        assert!(!hits(&w, Vec2::new(5.0, 3.0), 0.5));
        assert!(!hits(&wall_s(&[(0.0, 0.0), (0.2, 0.0)]), Vec2::ZERO, 1.0));
        let b = block_s((4.0, 4.0), (-2.0, -2.0), 10);
        assert!(hits(&b, Vec2::new(0.0, 0.0), 0.0));
        assert!(hits(&b, Vec2::new(4.5, 0.0), 0.6));
        assert!(!hits(&b, Vec2::new(5.0, 0.0), 0.5));
        let r = road_s(&[(0.0, 0.0), (0.0, 10.0)]);
        assert!(hits(&r, Vec2::new(0.9, 5.0), 0.0));
        assert!(!hits(&r, Vec2::new(2.5, 5.0), 0.5));
    }

    #[test]
    fn generate_terrain_shape() {
        for seed in [1, 2, 3, SEED, 0xdead_beef] {
            let t = terrain(seed);
            let (mut hollow, mut wood) = (0, 0);
            for z in 0..SIZE_Z {
                for x in 0..SIZE_X {
                    assert!(t.get(IVec3::new(x, 0, z)).is_solid());
                    let ground = t.get(IVec3::new(x, GROUND_Y - 1, z)).is_solid();
                    let c = Vec2::new(x as f32 + 0.5 - 80.0, z as f32 + 0.5 - 80.0) * VOXEL_SIZE;
                    if c.length() < 30.0 {
                        assert!(ground, "crater in centre at {x},{z}");
                    }
                    hollow += !ground as i32;
                    wood += (GROUND_Y..SIZE_Y).filter(|&y| t.get(IVec3::new(x, y, z)) == Voxel::DeadWood).count();
                }
            }
            assert!(hollow > 50, "craters exist");
            assert!(wood > 50, "trees exist");
        }
    }

    #[test]
    #[ignore]
    fn generate_timing() {
        use std::time::Instant;
        let t0 = Instant::now();
        let t = terrain(SEED);
        let terrain_ms = t0.elapsed().as_secs_f64() * 1e3;
        let mut w = World::new(SEED);
        for i in 0..20 {
            let f = i as f32 * 3.7 - 35.0;
            match i % 4 {
                0 => w.add(wall_s(&[(f, -30.0), (f + 10.0, 0.0), (f - 5.0, 30.0)])),
                1 => w.add(block_s((f, f * 0.5), (f + 10.0, f * 0.5 + 14.0), 40)),
                2 => w.add(Structure::Wall { points: vec![Vec2::new(-35.0, f), Vec2::new(35.0, -f)], height: 32, dam: true }),
                _ => w.add(road_s(&[(-38.0, f), (0.0, -f), (38.0, f)])),
            };
        }
        let t1 = Instant::now();
        let g = generate(&w);
        let gen_ms = t1.elapsed().as_secs_f64() * 1e3;
        std::hint::black_box((t, g));
        println!("terrain {terrain_ms:.2} ms, generate(terrain + 20) {gen_ms:.2} ms");
    }

    #[test]
    #[ignore]
    fn generate_ascii() {
        let mut w = World::new(SEED);
        w.add(wall_s(&[(-30.0, -10.0), (-10.0, -5.0), (5.0, -15.0)]));
        w.add(block_s((5.0, 0.0), (20.0, 12.0), 24));
        w.add(road_s(&[(-35.0, 2.0), (-20.0, -8.0), (35.0, -8.0)]));
        w.add(wall_s(&[(-35.0, -28.0), (0.0, -28.0)]));
        w.add(road_s(&[(-20.0, -38.0), (-18.0, -20.0)]));
        let g = generate(&w);
        let ch = |v: Voxel| match v {
            Voxel::Air => ' ',
            Voxel::Ash => '.',
            Voxel::Scorch => ',',
            Voxel::Asphalt => '=',
            Voxel::Rubble => '%',
            Voxel::Concrete => '#',
            Voxel::ConcreteDark => 'D',
            Voxel::WindowDark => 'o',
            Voxel::WindowLit => '*',
            Voxel::Rust => 'r',
            Voxel::DeadWood => 'T',
            Voxel::Lamp => '!',
        };
        println!("top-down (top voxel):");
        for z in (0..SIZE_Z).step_by(2) {
            let row: String = (0..SIZE_X).step_by(1).map(|x| ch(g.get(IVec3::new(x, g.top(x, z).max(0), z)))).collect();
            println!("{row}");
        }
        // Elevation seen from -z: first solid voxel scanning z0..z1.
        let elevation = |z0: i32, z1: i32, xs: std::ops::Range<i32>| {
            for y in (0..SIZE_Y).rev() {
                let row: String = xs
                    .clone()
                    .map(|x| (z0..z1).map(|z| g.get(IVec3::new(x, y, z))).find(|v| v.is_solid()).map_or(' ', ch))
                    .collect();
                println!("{row}");
            }
        };
        println!("block elevation:");
        elevation(70, 110, 85..125);
        println!("straight wall elevation:");
        elevation(20, 40, 5..80);
    }
}
