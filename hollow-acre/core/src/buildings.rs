//! Walls and blocks.

use crate::generate::{Ctx, P2, Poly, vs_rect};
use crate::noise::{hash, rnd};
use crate::terrain::line3;
use crate::voxel::{NX, NY, NZ, Voxel};

const S_WALL: u64 = 0xa11;
const S_BLOCK: u64 = 0xb10;

fn band(yr: i32) -> Voxel {
    if yr >= 0 && (yr / 4) % 2 == 0 { Voxel::Concrete } else { Voxel::ConcreteDark }
}

/// Board-formed wall along a polyline, 3 voxels thick, 12 high (22 tall, battered).
pub(crate) fn wall(c: &mut Ctx, path: &[P2], tall: bool) {
    let Some(poly) = Poly::new(path, 2.0) else { return };
    let hh = if tall { 22 } else { 12 };
    let seed = c.seed ^ S_WALL;
    let len = poly.len;
    poly.for_columns(3.6, |x, z, hit| {
        let (d, s) = (hit.d, hit.s);
        let batter = if !tall {
            0
        } else if d <= 2.0 {
            6
        } else if d <= 2.5 {
            4
        } else if d <= 3.0 {
            2
        } else {
            0
        };
        // Buttress every ~10 voxels, sticking out 1-2.
        let k = ((s - 5.0) / 10.0).round() as i32;
        let sk = 5.0 + 10.0 * k as f32;
        let mut butt = 0;
        if k >= 0 && sk <= len - 1.0 && (s - sk).abs() <= 1.0 && d > 1.5 {
            let ext = 1 + (hash(seed, k, 1, 0) % 2) as i32;
            let ok = match hash(seed, k, 2, 0) % 4 {
                0 => hit.side > 0.0,
                1 => hit.side < 0.0,
                _ => true,
            };
            if ok && d <= 1.5 + ext as f32 {
                butt = if d <= 2.5 { hh - 3 } else { hh * 5 / 9 };
            }
        }
        let body = d <= 1.5;
        let top = if body { hh } else { batter.max(butt) };
        if top == 0 {
            return;
        }
        c.clear_column(x, z);
        let h = c.height(x, z);
        for y in (h - 3).max(1)..h {
            c.set(x, y, z, Voxel::ConcreteDark);
        }
        for yr in 0..top {
            let mut v = band(yr);
            if body && yr == hh - 1 {
                v = Voxel::ConcreteDark; // walkway
            }
            c.set(x, h + yr, z, v);
        }
        if !body {
            return;
        }
        // Slit windows (1 wide, 3 tall) every 7 voxels.
        let kw = ((s - 3.5) / 7.0).round() as i32;
        let sw = 3.5 + 7.0 * kw as f32;
        if kw >= 0 && sw <= len - 2.0 && (s - sw).abs() < 0.5 {
            for (g, base) in [4, 13].into_iter().enumerate() {
                if g == 1 && !tall {
                    break;
                }
                let lit = hash(seed, kw, g as i32, 3) % 100 < 22;
                for yr in base..base + 3 {
                    c.set(x, h + yr, z, if lit { Voxel::WindowLit } else { Voxel::Glass });
                }
            }
        }
        // Walkway parapet (crenellated) and lamps.
        if d > 0.5 && (s.floor() as i32).rem_euclid(3) != 2 {
            c.set(x, h + hh, z, Voxel::Concrete);
        }
        let kl = ((s - 8.5) / 16.0).round() as i32;
        if kl >= 0 && 8.5 + 16.0 * kl as f32 <= len - 2.0 && d < 0.6 && (s - 8.5 - 16.0 * kl as f32).abs() < 0.5 {
            c.set(x, h + hh, z, Voxel::Rust);
            c.set(x, h + hh + 1, z, Voxel::Lamp);
        }
        // Ivy creeping up from the ground in irregular patches.
        if d > 0.5 {
            let side = if hit.side > 0.0 { 1 } else { -1 };
            let hv = rnd(seed ^ 1, (s / 4.0).floor() as i32, side, 0);
            if hv >= 0.5 {
                let iv = 2 + ((hv - 0.5) * 2.0 * (hh - 6) as f32) as i32 + (hash(seed, x, z, 7) % 3) as i32 - 1;
                for yr in 0..iv.min(hh - 2) {
                    if !matches!(c.g.get(x, h + yr, z), Voxel::Glass | Voxel::WindowLit) {
                        c.set(x, h + yr, z, Voxel::Ivy);
                    }
                }
            }
        }
    });
}

/// Evenly spaced 2-wide column starts covering [lo, hi), including both ends.
fn starts(lo: i32, hi: i32) -> Vec<i32> {
    let len = hi - lo;
    let n = (((len - 2) as f32) / 6.0).round().max(1.0) as i32;
    (0..=n).map(|k| lo + k * (len - 2) / n).collect()
}

/// Brutalist block: pilotis, overhanging slabs, windows, balconies, stair core, roof.
pub(crate) fn block(c: &mut Ctx, min: P2, max: P2, floors: u8) {
    let Some((x0, z0, x1, z1)) = vs_rect(min, max, 5) else { return };
    if floors == 0 {
        return;
    }
    let seed = c.seed ^ S_BLOCK;
    let (w, d) = (x1 - x0, z1 - z0);
    let (cx0, cx1, cz0, cz1) = ((x0 - 1).max(0), (x1 + 1).min(NX), (z0 - 1).max(0), (z1 + 1).min(NZ));
    if cx0 >= cx1 || cz0 >= cz1 {
        return;
    }
    let mut b = 0;
    for z in z0.max(0)..z1.min(NZ) {
        for x in x0.max(0)..x1.min(NX) {
            b = b.max(c.height(x, z));
        }
    }
    let n = (floors.min(12) as i32).min((NY - 1 - b - 14) / 6);
    if n < 1 {
        return;
    }
    let lt = b + 6 * n;

    // Site prep + foundation down to the terrain.
    for z in cz0..cz1 {
        for x in cx0..cx1 {
            c.clear_column(x, z);
            if (x0..x1).contains(&x) && (z0..z1).contains(&z) {
                for y in c.height(x, z)..b {
                    c.set(x, y, z, Voxel::ConcreteDark);
                }
            }
        }
    }
    // Pilotis.
    let (xs, zs) = (starts(x0, x1), starts(z0, z1));
    let interior = w >= 15 && d >= 15;
    for (i, &sx) in xs.iter().enumerate() {
        for (j, &sz) in zs.iter().enumerate() {
            if !interior && i != 0 && i != xs.len() - 1 && j != 0 && j != zs.len() - 1 {
                continue;
            }
            for y in b..b + 6 {
                for (dx, dz) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                    c.set(sx + dx, y, sz + dz, Voxel::Concrete);
                }
            }
        }
    }
    // Slabs (overhanging 1 voxel) and roof parapet.
    for i in 1..=n {
        for z in cz0..cz1 {
            for x in cx0..cx1 {
                c.set(x, b + 6 * i, z, Voxel::ConcreteDark);
            }
        }
    }
    for z in z0 - 1..z1 + 1 {
        for x in x0 - 1..x1 + 1 {
            if x == x0 - 1 || x == x1 || z == z0 - 1 || z == z1 {
                c.set(x, lt + 1, z, Voxel::Concrete);
            }
        }
    }
    // Storeys: 2-thick walls, window grid, recessed balconies with planters.
    for i in 1..n {
        let base = b + 6 * i;
        for z in z0.max(0)..z1.min(NZ) {
            for x in x0.max(0)..x1.min(NX) {
                let (dxm, dzm) = ((x - x0).min(x1 - 1 - x), (z - z0).min(z1 - 1 - z));
                if dxm >= 2 && dzm >= 2 {
                    continue;
                }
                let (face, u, len, layer) = if dxm < 2 && dzm < 2 {
                    (-1, 0, 0, 0)
                } else if dxm < 2 {
                    (if x - x0 < 2 { 0 } else { 1 }, z - z0, d, dxm)
                } else {
                    (if z - z0 < 2 { 2 } else { 3 }, x - x0, w, dzm)
                };
                // Window slots: 4 wide (2 glass + 2 wall), 4-voxel margin at both ends.
                let slot = (face >= 0 && u >= 4 && u <= len - 5)
                    .then(|| ((u - 4) / 4, (u - 4) % 4, u - (u - 4) % 4))
                    .filter(|&(_, _, start)| start + 3 <= len - 5);
                for rel in 1..=5 {
                    let mut v = Voxel::Concrete;
                    if let Some((k, r, start)) = slot {
                        let balcony = hash(seed, face, k, i) % 100 < 20;
                        let lit = hash(seed, face * 4096 + start, i, 11) % 100 < 30;
                        let glass = if lit { Voxel::WindowLit } else { Voxel::Glass };
                        if balcony {
                            if layer == 0 {
                                v = Voxel::Air;
                            } else if r < 2 && (rel == 2 || rel == 3) {
                                v = glass;
                            }
                        } else if r < 2 && (rel == 2 || rel == 3) {
                            v = glass;
                        }
                    }
                    c.set(x, base + rel, z, v);
                }
                if let Some((k, _, _)) = slot
                    && layer == 0
                    && hash(seed, face, k, i) % 100 < 20
                {
                    let (ox, oz) = match face {
                        0 => (x0 - 1, z),
                        1 => (x1, z),
                        2 => (x, z0 - 1),
                        _ => (x, z1),
                    };
                    let plant = if rnd(seed, ox, oz, i) < 0.4 { Voxel::Blossom } else { Voxel::Leaves };
                    c.set(ox, base + 1, oz, Voxel::Concrete);
                    c.set(ox, base + 2, oz, plant);
                }
            }
        }
    }
    // Stair core in one corner, sticking above the roof.
    let corner = hash(seed, x0, z0, 3) % 4;
    let (kx, kz) = (if corner & 1 == 0 { x0 } else { x1 - 3 }, if corner & 2 == 0 { z0 } else { z1 - 3 });
    for y in b..lt + 4 {
        for z in kz..kz + 3 {
            for x in kx..kx + 3 {
                c.set(x, y, z, Voxel::Concrete);
            }
        }
    }
    for z in kz - 1..kz + 4 {
        for x in kx - 1..kx + 4 {
            c.set(x, lt + 4, z, Voxel::ConcreteDark);
        }
    }
    let ox = if corner & 1 == 0 { kx } else { kx + 2 };
    let oz = if corner & 2 == 0 { kz } else { kz + 2 };
    for y in [lt + 2, lt + 3] {
        c.set(ox, y, kz + 1, Voxel::WindowLit);
        c.set(kx + 1, y, oz, Voxel::WindowLit);
    }

    // Roof.
    let free_core = |x: i32, z: i32| !((kx - 1..kx + 4).contains(&x) && (kz - 1..kz + 4).contains(&z));
    let in_roof = |x: i32, z: i32| x > x0 && x < x1 - 1 && z > z0 && z < z1 - 1;
    let big = w >= 12 && d >= 12;
    let (tx, tz) = (if corner & 1 == 0 { x1 - 6 } else { x0 + 1 }, if corner & 2 == 0 { z1 - 6 } else { z0 + 1 });
    let free_tank = |x: i32, z: i32| !big || !((tx - 1..tx + 6).contains(&x) && (tz - 1..tz + 6).contains(&z));
    let free = |x: i32, z: i32| in_roof(x, z) && free_core(x, z) && free_tank(x, z);
    let kind = if big { hash(seed, x0, z0, 77) % 3 } else { 0 };
    if kind == 1 {
        for z in z0 + 1..z1 - 1 {
            for x in x0 + 1..x1 - 1 {
                if !in_roof(x, z) || !free_core(x, z) {
                    continue;
                }
                c.set(x, lt + 1, z, Voxel::Grass);
                let r = rnd(seed, x, z, 5);
                if r < 0.14 {
                    c.set(x, lt + 2, z, Voxel::Leaves);
                    if r < 0.06 {
                        c.set(x, lt + 3, z, if rnd(seed, x, z, 6) < 0.4 { Voxel::Blossom } else { Voxel::Leaves });
                    }
                }
            }
        }
    }
    if kind != 1 && w >= 7 && d >= 7 {
        let mut z = z0 + 2;
        while z + 1 < z1 - 1 {
            for x in x0 + 1..x1 - 1 {
                if free(x, z) && free(x, z + 1) {
                    if (x - x0) % 3 == 0 {
                        c.set(x, lt + 1, z, Voxel::Steel);
                    }
                    c.set(x, lt + 2, z, Voxel::Solar);
                    c.set(x, lt + 1, z + 1, Voxel::Solar);
                }
            }
            z += 4;
        }
    }
    if big && kind != 1 {
        let (mx, mz) = (tx + 2, tz + 2);
        if kind == 0 {
            for dz in -2..=2 {
                for dx in -2..=2 {
                    let d2 = dx * dx + dz * dz;
                    if d2 <= 5 {
                        for y in lt + 3..lt + 7 {
                            c.set(mx + dx, y, mz + dz, Voxel::Rust);
                        }
                        if d2 <= 4 {
                            c.set(mx + dx, lt + 7, mz + dz, Voxel::Steel);
                        }
                    }
                }
            }
            for (dx, dz) in [(-1, -1), (1, -1), (-1, 1), (1, 1)] {
                for y in lt + 1..lt + 3 {
                    c.set(mx + dx, y, mz + dz, Voxel::Steel);
                }
            }
        } else {
            for y in lt + 1..lt + 10 {
                c.set(mx, y, mz, Voxel::Steel);
            }
            c.set(mx, lt + 9, mz, Voxel::Rust);
            let hub = [mx as f32, (lt + 9) as f32, mz as f32];
            for (bx, by) in [(0.0, 3.4), (-3.0, -1.7), (3.0, -1.7)] {
                line3(&mut c.g, hub, [hub[0] + bx, hub[1] + by, hub[2]], Voxel::Steel);
            }
        }
    }
}
