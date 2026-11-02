//! Document -> voxel grid. Pure and deterministic.
//!
//! All decoration randomness hashes (seed, voxel position), never structure index
//! or placement order, so a ghost preview matches what is committed.

use crate::buildings;
use crate::noise::{hash, rnd};
use crate::structure::{Document, Structure};
use crate::terrain::{self, Terrain, WATER_LEVEL, col, shard};
use crate::voxel::{NX, NY, NZ, ORIGIN, VOXEL_SIZE, Voxel, VoxelGrid, world_to_voxel};
use std::cell::RefCell;

const S_SPIRE: u64 = 0x5917;
const S_FIELD: u64 = 0xf1e1;

/// Mutable generation state: the grid plus per-column bare-ground height
/// (first non-ground y). Structures read `h`, never the decorated grid.
pub(crate) struct Ctx {
    pub g: VoxelGrid,
    pub h: Vec<i32>,
    pub seed: u64,
}

impl Ctx {
    pub fn height(&self, x: i32, z: i32) -> i32 {
        if (0..NX).contains(&x) && (0..NZ).contains(&z) { self.h[col(x, z)] } else { 0 }
    }

    #[inline]
    pub fn set(&mut self, x: i32, y: i32, z: i32, v: Voxel) {
        self.g.set(x, y, z, v);
    }

    /// Removes plants (and whole connected trees / crystals) standing in column (x, z).
    pub fn clear_column(&mut self, x: i32, z: i32) {
        if !(0..NX).contains(&x) || !(0..NZ).contains(&z) {
            return;
        }
        for y in self.h[col(x, z)]..NY {
            match self.g.get(x, y, z) {
                Voxel::Meadow | Voxel::Crop => self.g.set(x, y, z, Voxel::Air),
                Voxel::Wood | Voxel::Leaves | Voxel::Blossom | Voxel::Crystal => self.flood_clear(x, y, z),
                _ => {}
            }
        }
    }

    fn flood_clear(&mut self, x: i32, y: i32, z: i32) {
        let plant = |v: Voxel| matches!(v, Voxel::Wood | Voxel::Leaves | Voxel::Blossom | Voxel::Crystal);
        let mut stack = vec![(x, y, z)];
        while let Some((x, y, z)) = stack.pop() {
            if !plant(self.g.get(x, y, z)) {
                continue;
            }
            self.g.set(x, y, z, Voxel::Air);
            for (dx, dy, dz) in [(1, 0, 0), (-1, 0, 0), (0, 1, 0), (0, -1, 0), (0, 0, 1), (0, 0, -1)] {
                stack.push((x + dx, y + dy, z + dz));
            }
        }
    }
}

thread_local! {
    static TERRAIN_CACHE: RefCell<Option<(u64, Terrain)>> = const { RefCell::new(None) };
}

/// Terrain for `seed`, cached per thread (terrain is the expensive, unchanging part).
pub fn terrain_for(seed: u64) -> Terrain {
    TERRAIN_CACHE.with(|c| {
        let mut c = c.borrow_mut();
        if c.as_ref().is_none_or(|(s, _)| *s != seed) {
            *c = Some((seed, terrain::terrain(seed)));
        }
        c.as_ref().unwrap().1.clone()
    })
}

/// Builds the full voxel grid: terrain, then fields, walls, blocks, spires in
/// placement order, then paths (which carve).
pub fn generate(doc: &Document) -> VoxelGrid {
    let doc = &doc.sanitized();
    let t = terrain_for(doc.seed);
    let mut c = Ctx { g: t.grid, h: t.heights, seed: doc.seed };
    let all = &doc.structures;
    for s in all {
        if let Structure::Field { min, max } = s {
            field(&mut c, *min, *max);
        }
    }
    for s in all {
        if let Structure::Wall { path, tall } = s {
            buildings::wall(&mut c, path, *tall);
        }
    }
    for s in all {
        if let Structure::Block { min, max, floors } = s {
            buildings::block(&mut c, *min, *max, *floors);
        }
    }
    for s in all {
        if let Structure::Spire { at, kind } = s {
            spire(&mut c, *at, *kind);
        }
    }
    for s in all {
        if let Structure::Path { path } = s {
            gravel_path(&mut c, path);
        }
    }
    c.g
}

/// Ground height in world units at world (x, z): the top of the highest solid
/// voxel in that column, ignoring plants. Out-of-map points clamp to the edge.
pub fn ground_height(grid: &VoxelGrid, x: f32, z: f32) -> f32 {
    let v = world_to_voxel([x, 0.0, z]);
    let (vx, vz) = (v[0].clamp(0, NX - 1), v[2].clamp(0, NZ - 1));
    let top = (0..NY).rev().find(|&y| {
        let k = grid.get(vx, y, vz);
        k != Voxel::Air && !k.is_vegetation()
    });
    (top.unwrap_or(-1) + 1) as f32 * VOXEL_SIZE
}

// ---------------------------------------------------------------- geometry

pub(crate) type P2 = [f32; 2];

/// World x/z -> continuous voxel-space x/z. Non-finite input stays non-finite.
pub(crate) fn to_vs(p: P2) -> P2 {
    [(p[0] - ORIGIN) / VOXEL_SIZE, (p[1] - ORIGIN) / VOXEL_SIZE]
}

fn dist(a: P2, b: P2) -> f32 {
    ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2)).sqrt()
}

/// (distance, parameter t in 0..1, signed side) from p to segment ab.
fn seg(p: P2, a: P2, b: P2) -> (f32, f32, f32) {
    let ab = [b[0] - a[0], b[1] - a[1]];
    let l2 = ab[0] * ab[0] + ab[1] * ab[1];
    let ap = [p[0] - a[0], p[1] - a[1]];
    let t = if l2 > 0.0 { ((ap[0] * ab[0] + ap[1] * ab[1]) / l2).clamp(0.0, 1.0) } else { 0.0 };
    let q = [a[0] + ab[0] * t, a[1] + ab[1] * t];
    (dist(p, q), t, ab[0] * (p[1] - q[1]) - ab[1] * (p[0] - q[0]))
}

/// A cleaned voxel-space polyline with arc lengths.
pub(crate) struct Poly {
    pts: Vec<P2>,
    cum: Vec<f32>,
    pub len: f32,
}

pub(crate) struct Hit {
    pub d: f32,
    /// Arc length of the nearest point.
    pub s: f32,
    /// Positive on the left of the direction of travel.
    pub side: f32,
}

impl Poly {
    /// Converts a freehand world path to a clean voxel-space polyline: drops
    /// non-finite points and points closer than a voxel, then Douglas-Peucker
    /// removes mouse jitter. Returns None when degenerate (< `min_len` voxels).
    pub fn new(world: &[P2], min_len: f32) -> Option<Poly> {
        let mut out: Vec<P2> = Vec::with_capacity(world.len());
        for p in world.iter().filter(|p| p[0].is_finite() && p[1].is_finite()).map(|&p| to_vs(p)) {
            if out.last().is_none_or(|l| dist(*l, p) >= 1.0) {
                out.push(p);
            }
        }
        if let Some(end) = world.iter().rev().find(|p| p[0].is_finite() && p[1].is_finite()).map(|&p| to_vs(p))
            && out.len() > 1
        {
            *out.last_mut().unwrap() = end;
        }
        if out.len() > 2 {
            let mut keep = vec![false; out.len()];
            keep[0] = true;
            keep[out.len() - 1] = true;
            let mut stack = vec![(0, out.len() - 1)];
            while let Some((a, b)) = stack.pop() {
                let (mut best, mut bi) = (0.0, 0);
                for i in a + 1..b {
                    let d = seg(out[i], out[a], out[b]).0;
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
            out = out.into_iter().zip(keep).filter_map(|(p, k)| k.then_some(p)).collect();
        }
        if out.len() < 2 {
            return None;
        }
        let mut cum = vec![0.0];
        for w in out.windows(2) {
            cum.push(cum.last().unwrap() + dist(w[0], w[1]));
        }
        let len = *cum.last().unwrap();
        (len >= min_len).then_some(Poly { pts: out, cum, len })
    }

    pub fn point_at(&self, s: f32) -> P2 {
        for (i, w) in self.pts.windows(2).enumerate() {
            let l = self.cum[i + 1] - self.cum[i];
            if s <= self.cum[i + 1] && l > 0.0 {
                let t = ((s - self.cum[i]) / l).clamp(0.0, 1.0);
                return [w[0][0] + (w[1][0] - w[0][0]) * t, w[0][1] + (w[1][1] - w[0][1]) * t];
            }
        }
        *self.pts.last().unwrap()
    }

    /// Visits every in-map column whose centre is within `r` of the path, once,
    /// in raster order, with its nearest-point hit. Cost is per-segment bbox.
    pub fn for_columns(&self, r: f32, mut f: impl FnMut(i32, i32, &Hit)) {
        let cl = |v: f32, max: i32| v.clamp(-1.0, max as f32 + 1.0) as i32;
        let mut cells: std::collections::BTreeMap<(i32, i32), Hit> = std::collections::BTreeMap::new();
        for (i, w) in self.pts.windows(2).enumerate() {
            let lo = [w[0][0].min(w[1][0]) - r, w[0][1].min(w[1][1]) - r];
            let hi = [w[0][0].max(w[1][0]) + r, w[0][1].max(w[1][1]) + r];
            let (x0, z0) = (cl(lo[0].floor(), NX).max(0), cl(lo[1].floor(), NZ).max(0));
            let (x1, z1) = (cl(hi[0].ceil(), NX).min(NX - 1), cl(hi[1].ceil(), NZ).min(NZ - 1));
            for z in z0..=z1 {
                for x in x0..=x1 {
                    let (d, t, side) = seg([x as f32 + 0.5, z as f32 + 0.5], w[0], w[1]);
                    if d <= r && cells.get(&(z, x)).is_none_or(|h| d < h.d) {
                        let s = self.cum[i] + t * (self.cum[i + 1] - self.cum[i]);
                        cells.insert((z, x), Hit { d, s, side });
                    }
                }
            }
        }
        for (&(z, x), h) in &cells {
            f(x, z, h);
        }
    }
}

/// Integer voxel rectangle [x0, x1) x [z0, z1) from a world rectangle; None if
/// non-finite or smaller than `min` voxels on either side.
pub(crate) fn vs_rect(a: P2, b: P2, min: i32) -> Option<(i32, i32, i32, i32)> {
    if !(a[0].is_finite() && a[1].is_finite() && b[0].is_finite() && b[1].is_finite()) {
        return None;
    }
    let (a, b) = (to_vs(a), to_vs(b));
    let lim = |v: f32| v.clamp(-4096.0, 4096.0);
    let (x0, x1) = (lim(a[0].min(b[0])).floor() as i32, lim(a[0].max(b[0])).ceil() as i32);
    let (z0, z1) = (lim(a[1].min(b[1])).floor() as i32, lim(a[1].max(b[1])).ceil() as i32);
    (x1 - x0 >= min && z1 - z0 >= min && x1 > 0 && z1 > 0 && x0 < NX && z0 < NZ).then_some((x0, z0, x1, z1))
}

// ---------------------------------------------------------------- field

fn field(c: &mut Ctx, min: P2, max: P2) {
    let Some((x0, z0, x1, z1)) = vs_rect(min, max, 5) else { return };
    let (w, d) = (x1 - x0, z1 - z0);
    let (cx0, cx1, cz0, cz1) = (x0.max(0), x1.min(NX), z0.max(0), z1.min(NZ));
    let (mut sum, mut n) = (0i64, 0i64);
    for z in cz0..cz1 {
        for x in cx0..cx1 {
            sum += c.height(x, z) as i64;
            n += 1;
        }
    }
    if n == 0 {
        return;
    }
    let f = ((sum as f32 / n as f32).round() as i32).clamp(WATER_LEVEL + 1, NY - 8);
    let long_x = w >= d;
    for z in cz0..cz1 {
        for x in cx0..cx1 {
            c.clear_column(x, z);
            let old = c.height(x, z);
            for y in f..old {
                c.set(x, y, z, Voxel::Air);
            }
            for y in old..f {
                c.set(x, y, z, Voxel::Soil);
            }
            c.set(x, f - 1, z, Voxel::Soil);
            c.set(x, f - 2, z, Voxel::Soil);
            c.h[col(x, z)] = f;
            let (lx, lz) = (x - x0, z - z0);
            if lx == 0 || lz == 0 || lx == w - 1 || lz == d - 1 {
                c.set(x, f - 1, z, Voxel::ConcreteDark);
                c.set(x, f, z, Voxel::ConcreteDark);
                continue;
            }
            let across = if long_x { lz } else { lx };
            match across {
                1 => c.set(x, f - 1, z, Voxel::Water),
                2 => {}
                a if (a - 3) % 2 == 1 => {
                    c.set(x, f - 1, z, Voxel::Air); // furrow
                }
                _ => {
                    let hh = hash(c.seed ^ S_FIELD, x, z, 5);
                    let tall = match hh % 8 {
                        0 => 0,
                        1..=4 => 1,
                        _ => 2,
                    };
                    for y in f..f + tall {
                        c.set(x, y, z, Voxel::Crop);
                    }
                }
            }
        }
    }
}

// ---------------------------------------------------------------- spire

fn spire(c: &mut Ctx, at: P2, kind: u8) {
    if !(at[0].is_finite() && at[1].is_finite()) {
        return;
    }
    let v = to_vs(at);
    if v[0] < 0.0 || v[1] < 0.0 || v[0] >= NX as f32 || v[1] >= NZ as f32 {
        return;
    }
    let (cx, cz) = (v[0] as i32, v[1] as i32);
    let h0 = c.height(cx, cz);
    let seed = c.seed ^ S_SPIRE;
    let clear = |c: &mut Ctx, r: i32| {
        for z in cz - r..=cz + r {
            for x in cx - r..=cx + r {
                if (x - cx).pow(2) + (z - cz).pow(2) <= r * r {
                    c.clear_column(x, z);
                }
            }
        }
    };
    match kind % 3 {
        0 => {
            clear(c, 3);
            let shards = 3 + (hash(seed, cx, cz, 0) % 5) as i32;
            for i in 0..shards {
                let r = |k| rnd(seed, cx * 8 + i, cz, k + 1);
                let (bx, bz) = (cx as f32 + (r(0) - 0.5) * 4.0, cz as f32 + (r(1) - 0.5) * 4.0);
                let height = if i == 0 { 13 + (r(2) * 4.0) as i32 } else { 5 + (r(2) * 8.0) as i32 };
                let (lx, lz) = ((r(3) - 0.5) * 0.7, (r(4) - 0.5) * 0.7);
                shard(&mut c.g, &c.h, bx, bz, height, lx, lz);
            }
        }
        1 => {
            clear(c, 5);
            let mut b = h0;
            for z in cz - 4..=cz + 4 {
                for x in cx - 4..=cx + 4 {
                    if (x - cx).pow(2) + (z - cz).pow(2) <= 17 {
                        b = b.max(c.height(x, z));
                    }
                }
            }
            for z in cz - 4..=cz + 4 {
                for x in cx - 4..=cx + 4 {
                    let d2 = (x - cx).pow(2) + (z - cz).pow(2);
                    if d2 > 17 {
                        continue;
                    }
                    for y in c.height(x, z)..b + 1 {
                        c.set(x, y, z, Voxel::ConcreteDark);
                    }
                    let ring = (7..=10).contains(&d2);
                    c.set(x, b + 1, z, if ring { Voxel::Rune } else { Voxel::Concrete });
                }
            }
            // The floating monolith is a separate bobbing entity (see `life`): plinth and
            // rune ring only here.
        }
        _ => {
            clear(c, 6);
            terrain::tree(&mut c.g, h0, cx, cz, seed, 0, 8, 5.0, true);
            for (dx, dz) in [(1, 0), (0, 1), (1, 1)] {
                for y in c.height(cx + dx, cz + dz).min(h0)..h0 + 4 {
                    c.set(cx + dx, y, cz + dz, Voxel::Wood);
                }
            }
        }
    }
}

// ---------------------------------------------------------------- path

/// Gravel path set flush into the ground; carves arches through structures.
fn gravel_path(c: &mut Ctx, pts: &[P2]) {
    let Some(poly) = Poly::new(pts, 1.5) else { return };
    poly.for_columns(2.6, |x, z, hit| {
        let h = c.height(x, z);
        let mut carved = false;
        if (h..h + 8).any(|y| c.g.get(x, y, z).is_structure()) {
            let u = hit.d / 2.6;
            let top = (5.0 + 2.0 * (1.0 - u * u).max(0.0).sqrt()).floor() as i32;
            for y in h..h + top {
                if c.g.get(x, y, z) != Voxel::Water {
                    c.set(x, y, z, Voxel::Air);
                }
            }
            carved = true;
        }
        if hit.d <= 1.5 {
            for y in h..h + 3 {
                if matches!(c.g.get(x, y, z), Voxel::Crop | Voxel::Meadow) {
                    c.set(x, y, z, Voxel::Air);
                }
            }
            let below = c.g.get(x, h - 1, z);
            if c.g.get(x, h, z) != Voxel::Water && (carved || !below.is_structure()) && below != Voxel::Water {
                c.set(x, h - 1, z, Voxel::Gravel);
            }
        }
    });
}
