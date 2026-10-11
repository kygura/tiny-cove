//! Seeded meadow terrain: rolling hills, ponds, a ruined highway, rock outcrops,
//! crystals, trees and tufts. Pure and deterministic.

use crate::noise::{hash, rnd, smoothstep, value_noise};
use crate::voxel::{NX, NY, NZ, SIZE_X, SIZE_Z, Voxel, VoxelGrid};

/// Ponds are filled with water up to (not including) this voxel y.
pub const WATER_LEVEL: i32 = 9;
/// Voxel-space centre of the map.
const CX: f32 = (SIZE_X / 2) as f32;
const CZ: f32 = (SIZE_Z / 2) as f32;
/// Radius (voxels) of the calm, flat-ish central area.
const CALM: f32 = 42.0;

const S_HILL: u64 = 0x1111;
const S_POND: u64 = 0x2222;
const S_HW: u64 = 0x3333;
const S_ROCK: u64 = 0x4444;
const S_CRYS: u64 = 0x5555;
const S_TREE: u64 = 0x6666;
const S_TUFT: u64 = 0x7777;

/// Terrain voxels plus the bare-ground surface height per column (the first
/// non-ground y; ignores plants and decorations).
#[derive(Clone)]
pub struct Terrain {
    pub grid: VoxelGrid,
    pub heights: Vec<i32>,
}

#[derive(Clone, Copy, PartialEq)]
enum Surf {
    Grass,
    Sand,
    Rock,
}

pub(crate) fn col(x: i32, z: i32) -> usize {
    x as usize + z as usize * SIZE_X
}

fn hill(seed: u64, fx: f32, fz: f32) -> f32 {
    let d = ((fx - CX).powi(2) + (fz - CZ).powi(2)).sqrt();
    let m = smoothstep(CALM - 4.0, CALM + 30.0, d);
    let n = value_noise(seed ^ S_HILL, fx / 48.0, fz / 48.0) * 0.65 + value_noise(seed ^ S_HILL ^ 1, fx / 21.0, fz / 21.0) * 0.35;
    let gentle = (value_noise(seed ^ S_HILL ^ 2, fx / 26.0, fz / 26.0) - 0.5) * 3.0;
    11.0 + gentle * (1.0 - m) + (n - 0.5) * 26.0 * m
}

struct Highway {
    t: (f32, f32),
    n: (f32, f32),
    off: f32,
}

impl Highway {
    fn new(seed: u64) -> Self {
        let th = rnd(seed ^ S_HW, 0, 0, 0) * std::f32::consts::PI;
        let sign = if rnd(seed ^ S_HW, 1, 0, 0) < 0.5 { -1.0 } else { 1.0 };
        Highway { t: (th.cos(), th.sin()), n: (-th.sin(), th.cos()), off: sign * (54.0 + rnd(seed ^ S_HW, 2, 0, 0) * 8.0) }
    }
    /// (signed distance to the centre line, arc position).
    fn at(&self, fx: f32, fz: f32) -> (f32, f32) {
        let (dx, dz) = (fx - CX, fz - CZ);
        (dx * self.n.0 + dz * self.n.1 - self.off, dx * self.t.0 + dz * self.t.1)
    }
    fn point(&self, u: f32, d: f32) -> (f32, f32) {
        (CX + self.t.0 * u + self.n.0 * (self.off + d), CZ + self.t.1 * u + self.n.1 * (self.off + d))
    }
}

pub fn terrain(seed: u64) -> Terrain {
    let hw = Highway::new(seed);
    let ponds = ponds(seed, &hw);
    let n = SIZE_X * SIZE_Z;
    let mut hs = vec![0i32; n];
    let mut surf = vec![Surf::Grass; n];

    for z in 0..NZ {
        for x in 0..NX {
            let (fx, fz) = (x as f32 + 0.5, z as f32 + 0.5);
            let mut base = hill(seed, fx, fz);
            let (dl, u) = hw.at(fx, fz);
            if dl.abs() < 6.5 {
                let (qx, qz) = hw.point(u, 0.0);
                let w = smoothstep(6.5, 3.8, dl.abs());
                base += (hill(seed, qx, qz) - base) * w;
            }
            for &(px, pz, r) in &ponds {
                let dd = ((fx - px).powi(2) + (fz - pz).powi(2)).sqrt() / r;
                if dd < 2.0 {
                    let w = smoothstep(2.0, 1.0, dd);
                    let target = if dd < 1.0 { (WATER_LEVEL + 1) as f32 - (1.0 - dd * dd) * 4.0 } else { (WATER_LEVEL + 1) as f32 };
                    base += (target - base) * w;
                    if dd < 1.3 {
                        surf[col(x, z)] = Surf::Sand;
                    }
                }
            }
            hs[col(x, z)] = (base.round() as i32).clamp(3, 40);
        }
    }

    // Rock outcrops toward the edges.
    for i in 0..8 {
        let ang = rnd(seed ^ S_ROCK, i, 0, 0) * std::f32::consts::TAU;
        let dist = 64.0 + rnd(seed ^ S_ROCK, i, 1, 0) * 24.0;
        let (ox, oz) = (CX + ang.cos() * dist, CZ + ang.sin() * dist);
        let r = 3.0 + rnd(seed ^ S_ROCK, i, 2, 0) * 3.5;
        let hh = 3.0 + rnd(seed ^ S_ROCK, i, 3, 0) * 4.0;
        for z in (oz - r * 1.4) as i32..=(oz + r * 1.4) as i32 {
            for x in (ox - r * 1.4) as i32..=(ox + r * 1.4) as i32 {
                if !(0..NX).contains(&x) || !(0..NZ).contains(&z) {
                    continue;
                }
                let dd = ((x as f32 + 0.5 - ox).powi(2) + (z as f32 + 0.5 - oz).powi(2)).sqrt() / r;
                let c = col(x, z);
                if dd < 1.3 && hs[c] > WATER_LEVEL + 1 {
                    let jitter = 0.75 + 0.5 * rnd(seed ^ S_ROCK, x, z, 9);
                    let add = (hh * (1.0 - (dd / 1.3).powi(2)).max(0.0).powf(0.7) * jitter).round() as i32;
                    if add > 0 {
                        hs[c] += add;
                        surf[c] = Surf::Rock;
                    }
                }
            }
        }
    }

    // Fill columns.
    let mut grid = VoxelGrid::new();
    for z in 0..NZ {
        for x in 0..NX {
            let c = col(x, z);
            let h = hs[c].min(NY - 1);
            for y in 0..h {
                let v = match surf[c] {
                    Surf::Rock => Voxel::Rock,
                    Surf::Sand if y >= h - 2 => Voxel::Sand,
                    _ if y < h - 4 => Voxel::Rock,
                    _ if y == h - 1 => if surf[c] == Surf::Sand { Voxel::Sand } else { Voxel::Grass },
                    _ => Voxel::Soil,
                };
                grid.set(x, y, z, v);
            }
            for y in h..WATER_LEVEL {
                grid.set(x, y, z, Voxel::Water);
            }
        }
    }

    // Cracked highway.
    let mut road = vec![false; n];
    for z in 0..NZ {
        for x in 0..NX {
            let (fx, fz) = (x as f32 + 0.5, z as f32 + 0.5);
            let (dl, u) = hw.at(fx, fz);
            let c = col(x, z);
            let edge = 3.5 + (value_noise(seed ^ S_HW ^ 5, fx / 3.0, fz / 3.0) - 0.5) * 1.4;
            if dl.abs() > edge || hs[c] <= WATER_LEVEL || surf[c] != Surf::Grass {
                continue;
            }
            road[c] = true;
            let patch = value_noise(seed ^ S_HW ^ 1, fx / 6.0, fz / 6.0);
            if patch > 0.76 {
                continue; // grass has reclaimed the road
            }
            let crack = value_noise(seed ^ S_HW ^ 2, fx / 2.6, fz / 2.6);
            let top = if (crack - 0.5).abs() < 0.025 {
                Voxel::Gravel
            } else if dl.abs() < 0.6 && ((u / 4.0).floor() as i32).rem_euclid(2) == 0 && patch > 0.3 {
                Voxel::Concrete
            } else {
                Voxel::Asphalt
            };
            let h = hs[c];
            grid.set(x, h - 1, z, top);
            grid.set(x, h - 2, z, Voxel::Asphalt);
        }
    }

    pylons(&mut grid, &hs, seed, &hw);
    crystals(&mut grid, &hs, &road, seed);
    trees(&mut grid, &hs, &road, seed);

    // Meadow tufts.
    for z in 0..NZ {
        for x in 0..NX {
            let c = col(x, z);
            let h = hs[c];
            if grid.get(x, h - 1, z) != Voxel::Grass || grid.get(x, h, z) != Voxel::Air {
                continue;
            }
            let (fx, fz) = (x as f32, z as f32);
            let p = 0.04 + 0.16 * smoothstep(0.45, 0.75, value_noise(seed ^ S_TUFT, fx / 9.0, fz / 9.0));
            if rnd(seed ^ S_TUFT, x, z, 0) < p {
                grid.set(x, h, z, Voxel::Meadow);
            }
        }
    }
    Terrain { grid, heights: hs }
}

/// (centre x, centre z, radius) in voxel space. Always clear of the highway and the calm centre.
fn ponds(seed: u64, hw: &Highway) -> Vec<(f32, f32, f32)> {
    let count = 1 + (hash(seed ^ S_POND, 0, 0, 0) % 2) as i32;
    (0..count)
        .map(|i| {
            let ang = rnd(seed ^ S_POND, i, 1, 0) * std::f32::consts::TAU;
            let dist = 58.0 + rnd(seed ^ S_POND, i, 2, 0) * 14.0;
            let r = 8.0 + rnd(seed ^ S_POND, i, 3, 0) * 5.0;
            let mut c = (CX + ang.cos() * dist, CZ + ang.sin() * dist);
            if hw.at(c.0, c.1).0.abs() < r * 2.0 + 8.0 {
                c = (CX - ang.cos() * dist, CZ - ang.sin() * dist);
            }
            (c.0, c.1, r)
        })
        .collect()
}

/// Straight voxel line (inclusive), 1 voxel thick.
pub(crate) fn line3(grid: &mut VoxelGrid, a: [f32; 3], b: [f32; 3], v: Voxel) {
    let steps = (0..3).map(|i| (b[i] - a[i]).abs()).fold(0.0, f32::max).ceil().max(1.0) as i32;
    for s in 0..=steps {
        let t = s as f32 / steps as f32;
        let p = |i: usize| (a[i] + (b[i] - a[i]) * t).round() as i32;
        grid.set(p(0), p(1), p(2), v);
    }
}

fn pylons(grid: &mut VoxelGrid, hs: &[i32], seed: u64, hw: &Highway) {
    for i in 0..2 {
        let u = (if i == 0 { -1.0 } else { 1.0 }) * (14.0 + rnd(seed ^ S_HW, 10 + i, 0, 0) * 30.0);
        let side = if rnd(seed ^ S_HW, 20 + i, 0, 0) < 0.5 { -5.5 } else { 5.5 };
        let (px, pz) = hw.point(u, side);
        let (cx, cz) = (px.round() as i32, pz.round() as i32);
        if !(5..NX - 5).contains(&cx) || !(5..NZ - 5).contains(&cz) || hs[col(cx, cz)] <= WATER_LEVEL {
            continue;
        }
        let hmax = (-2..=2).flat_map(|dz| (-2..=2).map(move |dx| (dx, dz))).map(|(dx, dz)| hs[col(cx + dx, cz + dz)]).max().unwrap_or(0);
        let height = 24.0 - 7.0 * (i as f32); // second pylon is shorter / broken
        let leg = |sx: f32, sz: f32, y: f32| {
            let w = if y <= 14.0 { 2.0 - y / 14.0 } else { 1.0 };
            [cx as f32 + sx * w, hmax as f32 + y, cz as f32 + sz * w]
        };
        let corners = [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)];
        let levels = [0.0, 5.0, 10.0, 14.0, 20.0];
        for &(sx, sz) in &corners {
            let ground = hs[col(cx + 2 * sx as i32, cz + 2 * sz as i32)] - 1;
            line3(grid, [cx as f32 + 2.0 * sx, ground as f32, cz as f32 + 2.0 * sz], leg(sx, sz, 0.0), Voxel::Rust);
            for w in levels.windows(2) {
                if w[1] <= height {
                    line3(grid, leg(sx, sz, w[0]), leg(sx, sz, w[1]), Voxel::Rust);
                }
            }
            line3(grid, leg(sx, sz, 14.0), leg(sx, sz, height), Voxel::Rust);
        }
        for (k, &y) in levels.iter().enumerate() {
            if y > height {
                break;
            }
            for j in 0..4 {
                let (a, b) = (corners[j], corners[(j + 1) % 4]);
                line3(grid, leg(a.0, a.1, y), leg(b.0, b.1, y), Voxel::Rust);
                if let Some(&y2) = levels.get(k + 1).filter(|&&y2| y2 <= height) {
                    line3(grid, leg(a.0, a.1, y), leg(b.0, b.1, y2), Voxel::Rust);
                    line3(grid, leg(b.0, b.1, y), leg(a.0, a.1, y2), Voxel::Rust);
                }
            }
        }
        // Cross arms.
        if i == 0 {
            for (y, half) in [(height - 1.0, 5.0), (height - 6.0, 4.0)] {
                let yy = hmax as f32 + y;
                line3(grid, [cx as f32 - half, yy, cz as f32], [cx as f32 + half, yy, cz as f32], Voxel::Rust);
                for s in [-half, 0.0, half] {
                    line3(grid, [cx as f32 + s, yy, cz as f32], [cx as f32 + s, yy - 2.0, cz as f32], Voxel::Rust);
                }
            }
        }
    }
}

fn crystals(grid: &mut VoxelGrid, hs: &[i32], road: &[bool], seed: u64) {
    let clusters = 5 + (hash(seed ^ S_CRYS, 0, 0, 0) % 3) as i32;
    for i in 0..clusters {
        let ang = rnd(seed ^ S_CRYS, i, 1, 0) * std::f32::consts::TAU;
        let dist = 60.0 + rnd(seed ^ S_CRYS, i, 2, 0) * 24.0;
        let (cx, cz) = ((CX + ang.cos() * dist) as i32, (CZ + ang.sin() * dist) as i32);
        if !(4..NX - 4).contains(&cx) || !(4..NZ - 4).contains(&cz) {
            continue;
        }
        let c = col(cx, cz);
        if hs[c] <= WATER_LEVEL + 1 || road[c] {
            continue;
        }
        let shards = 2 + (hash(seed ^ S_CRYS, i, 3, 0) % 4) as i32;
        for s in 0..shards {
            let r = |k| rnd(seed ^ S_CRYS, i * 16 + s, 10 + k, 0);
            let (bx, bz) = (cx as f32 + (r(0) - 0.5) * 4.0, cz as f32 + (r(1) - 0.5) * 4.0);
            shard(grid, hs, bx, bz, 3 + (r(2) * 6.0) as i32, (r(3) - 0.5) * 0.9, (r(4) - 0.5) * 0.9);
        }
    }
}

/// A slanted crystal shard of height `h` rising from the ground at (bx, bz),
/// leaning (lx, lz) voxels per layer. Wide at the base, single voxel above.
pub(crate) fn shard(grid: &mut VoxelGrid, hs: &[i32], bx: f32, bz: f32, h: i32, lx: f32, lz: f32) {
    for t in 0..h {
        let (x, z) = ((bx + lx * t as f32).round() as i32, (bz + lz * t as f32).round() as i32);
        if !(0..NX).contains(&x) || !(0..NZ).contains(&z) {
            break;
        }
        let y = hs[col(x, z)] + t - 1;
        let wide = t < (h + 2) / 3;
        for (dx, dz) in [(0, 0), (1, 0), (0, 1)] {
            if (dx, dz) == (0, 0) || wide {
                let g = grid.get(x + dx, y, z + dz);
                if matches!(g, Voxel::Air | Voxel::Meadow | Voxel::Grass) {
                    grid.set(x + dx, y, z + dz, Voxel::Crystal);
                }
            }
        }
    }
}

fn trees(grid: &mut VoxelGrid, hs: &[i32], road: &[bool], seed: u64) {
    let mut placed: Vec<(i32, i32)> = Vec::new();
    for i in 0..600 {
        if placed.len() >= 70 {
            break;
        }
        let x = 4 + (rnd(seed ^ S_TREE, i, 0, 0) * (NX - 8) as f32) as i32;
        let z = 4 + (rnd(seed ^ S_TREE, i, 1, 0) * (NZ - 8) as f32) as i32;
        let (fx, fz) = (x as f32, z as f32);
        if ((fx - CX).powi(2) + (fz - CZ).powi(2)).sqrt() < CALM - 2.0 {
            continue;
        }
        let grove = value_noise(seed ^ S_TREE ^ 1, fx / 18.0, fz / 18.0);
        let p = rnd(seed ^ S_TREE, i, 2, 0);
        if !(grove > 0.52 && p < 0.4 || p < 0.025) {
            continue;
        }
        let c = col(x, z);
        if road[c] || hs[c] <= WATER_LEVEL + 1 || grid.get(x, hs[c] - 1, z) != Voxel::Grass || grid.get(x, hs[c], z) == Voxel::Crystal {
            continue;
        }
        if placed.iter().any(|&(px, pz)| (px - x).pow(2) + (pz - z).pow(2) < 20) {
            continue;
        }
        placed.push((x, z));
        tree(grid, hs[c], x, z, seed, i, 4 + (hash(seed ^ S_TREE, i, 3, 0) % 3) as i32, 2.6 + rnd(seed ^ S_TREE, i, 4, 0) * 0.8, rnd(seed ^ S_TREE, i, 5, 0) < 0.3);
    }
}

/// A round-canopy tree whose trunk starts at `ground` (first air y).
#[allow(clippy::too_many_arguments)]
pub(crate) fn tree(grid: &mut VoxelGrid, ground: i32, x: i32, z: i32, seed: u64, id: i32, trunk: i32, r: f32, blossom: bool) {
    for y in ground..ground + trunk {
        grid.set(x, y, z, Voxel::Wood);
    }
    let cy = ground + trunk;
    let ri = r.ceil() as i32;
    for dy in -2..=ri {
        for dz in -ri..=ri {
            for dx in -ri..=ri {
                let d2 = dx * dx + dz * dz + ((dy as f32) * 1.35).powi(2) as i32;
                let jitter = rnd(seed ^ S_TREE ^ 9, x + dx, cy + dy + id * 64, z + dz) * 1.6;
                if d2 as f32 <= r * r + jitter - 0.5 && grid.get(x + dx, cy + dy, z + dz) == Voxel::Air {
                    let b = blossom && rnd(seed ^ S_TREE ^ 8, x + dx, cy + dy, z + dz) < 0.6;
                    grid.set(x + dx, cy + dy, z + dz, if b { Voxel::Blossom } else { Voxel::Leaves });
                }
            }
        }
    }
}
