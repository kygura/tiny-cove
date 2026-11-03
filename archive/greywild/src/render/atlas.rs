//! Procedural texture atlas: one 32×32 tile per [`Surface`], 8 tiles per row.
//!
//! Texels are mostly light neutral detail (stone joints, plank gaps, grain…) that the
//! shader multiplies by the vertex colour, so palette tints stay in charge of hue.
//! Alpha marks special texels: `1.0` = glass/flame (emissive mask, drawn untinted),
//! `0.5`.. = regular. Tile `i` = `Surface as u8`. Sampled with `textureLoad` (nearest).

use bevy::{
    asset::RenderAssetUsages,
    image::{Image, ImageSampler},
    render::render_resource::{Extent3d, TextureDimension, TextureFormat},
};
use greywild_core::Surface;

pub const TILE: u32 = 32;
pub const COLS: u32 = 8;
pub const ROWS: u32 = (Surface::COUNT as u32).div_ceil(COLS);

const ALL: [Surface; Surface::COUNT] = [
    Surface::Grass,
    Surface::Dirt,
    Surface::Rock,
    Surface::Gravel,
    Surface::Concrete,
    Surface::Plinth,
    Surface::Earth,
    Surface::Cladding,
    Surface::Steel,
    Surface::Sedum,
    Surface::Seam,
    Surface::Precast,
    Surface::Pavers,
    Surface::Slabs,
    Surface::Poured,
    Surface::Window,
    Surface::Door,
    Surface::Bark,
    Surface::Leaves,
    Surface::Fiber,
    Surface::Petal,
    Surface::Metal,
    Surface::Glow,
    Surface::Water,
];

fn hash(x: i32, y: i32, seed: u32) -> f32 {
    let mut h = (x as u32).wrapping_mul(0x27d4_eb2d)
        ^ (y as u32).wrapping_mul(0x1656_67b1)
        ^ seed.wrapping_mul(0x9e37_79b9);
    h ^= h >> 15;
    h = h.wrapping_mul(0x85eb_ca6b);
    h ^= h >> 13;
    h = h.wrapping_mul(0xc2b2_ae35);
    h ^= h >> 16;
    (h & 0xffff) as f32 / 65535.0
}

/// Tileable blocky value noise (period 32) in 0..1, cell size `c` px.
fn vnoise(x: i32, y: i32, c: i32, seed: u32) -> f32 {
    let n = TILE as i32 / c;
    let (cx, cy) = (x.div_euclid(c), y.div_euclid(c));
    let (fx, fy) = ((x.rem_euclid(c)) as f32 / c as f32, (y.rem_euclid(c)) as f32 / c as f32);
    let h = |i: i32, j: i32| hash(i.rem_euclid(n), j.rem_euclid(n), seed);
    let a = h(cx, cy) + (h(cx + 1, cy) - h(cx, cy)) * fx;
    let b = h(cx, cy + 1) + (h(cx + 1, cy + 1) - h(cx, cy + 1)) * fx;
    a + (b - a) * fy
}

/// Tileable Voronoi: (distance to nearest, distance to second nearest, cell id hash).
fn voronoi(x: i32, y: i32, cells: i32, seed: u32) -> (f32, f32, f32) {
    let size = TILE as f32 / cells as f32;
    let (px, py) = (x as f32 + 0.5, y as f32 + 0.5);
    let (cx, cy) = ((px / size).floor() as i32, (py / size).floor() as i32);
    let (mut d1, mut d2, mut id) = (1e9f32, 1e9f32, 0.0);
    for j in -1..=1 {
        for i in -1..=1 {
            let (gx, gy) = (cx + i, cy + j);
            let (wx, wy) = (gx.rem_euclid(cells), gy.rem_euclid(cells));
            let ox = (gx as f32 + 0.15 + 0.7 * hash(wx, wy, seed)) * size;
            let oy = (gy as f32 + 0.15 + 0.7 * hash(wx, wy, seed + 7)) * size;
            let d = ((ox - px).powi(2) + (oy - py).powi(2)).sqrt();
            if d < d1 {
                d2 = d1;
                d1 = d;
                id = hash(wx, wy, seed + 13);
            } else if d < d2 {
                d2 = d;
            }
        }
    }
    (d1, d2, id)
}

fn grey(v: f32) -> [f32; 4] {
    [v, v, v, 0.5]
}

fn rgb(r: f32, g: f32, b: f32) -> [f32; 4] {
    [r, g, b, 0.5]
}

fn texel(s: Surface, x: i32, y: i32) -> [f32; 4] {
    let n = |c: i32, seed: u32| vnoise(x, y, c, seed);
    let r = hash(x, y, s as u32 * 31 + 5);
    match s {
        Surface::Grass => {
            // Short vertical blades: light tips, dark roots, clumpy value.
            let col = hash(x, 0, 11);
            let phase = (y + (col * 32.0) as i32).rem_euclid(8);
            let clump = 0.82 + 0.18 * n(8, 1);
            let v = match phase {
                0 => 1.0,
                1 | 2 => 0.95,
                7 => 0.8,
                _ => 0.9,
            };
            let v = if r > 0.95 { 1.06 } else { v };
            [v * clump * 0.97, v * clump, v * clump * 0.92, 0.5]
        }
        Surface::Dirt => {
            let mut v = 0.8 + 0.15 * n(8, 2) + (r - 0.5) * 0.08;
            if r > 0.94 {
                v *= 0.7;
            } else if r < 0.04 {
                v *= 1.15;
            }
            grey(v)
        }
        Surface::Rock => {
            let (d1, d2, id) = voronoi(x, y, 3, 21);
            let crack = d2 - d1 < 1.2;
            let v = 0.72 + 0.22 * id + 0.08 * n(4, 3);
            grey(if crack { 0.5 } else { v + if y % 11 == 0 { -0.06 } else { 0.0 } })
        }
        Surface::Gravel => {
            // Pea gravel: small round stones with darker gaps, a few dark/light speckles.
            let (d1, d2, id) = voronoi(x, y, 8, 4);
            let v = if d2 - d1 < 0.9 { 0.74 } else { 0.86 + 0.12 * id - 0.04 * (d1 / 2.0).min(1.0) };
            let v = if r > 0.93 { v * 0.82 } else if r < 0.05 { v * 1.08 } else { v };
            rgb(v, v * 0.98, v * 0.95)
        }
        Surface::Concrete => {
            // Board-formed concrete: horizontal boards (5–6 px), each with its own tone and
            // a faint grain, 1 px seam lines, tie-hole dots on a 16 px grid with short drips.
            const EDGES: [i32; 7] = [0, 6, 11, 16, 22, 27, 32];
            let b = EDGES.iter().rposition(|e| *e <= y).unwrap_or(0).min(5) as i32;
            let top = y == EDGES[b as usize];
            let seam = y == EDGES[b as usize + 1] - 1;
            let grain = vnoise(x, y * 8 + b * 3, 8, 50 + b as u32);
            let mut v = 0.86 + 0.07 * hash(b, 0, 31) + 0.05 * (grain - 0.5) + 0.03 * (n(16, 32) - 0.5);
            if seam {
                v -= 0.07;
            } else if top {
                v += 0.025;
            }
            let (hx, hy) = ((x - 4).rem_euclid(16), (y - 8).rem_euclid(16));
            if hx == 0 && hy == 0 {
                v = 0.58; // tie hole
            } else if (hx == 0 || (hx == 1 && r > 0.5)) && (1..5).contains(&hy) {
                v -= 0.05 * (5 - hy) as f32 / 4.0; // drip stain under the hole
            }
            if r > 0.97 {
                v -= 0.05; // blowholes
            }
            grey(v)
        }
        Surface::Plinth => {
            // Bush-hammered: dense stipple, chiselled 2 px margin around 32×16 courses.
            let ry = y % 16;
            let margin = ry < 2 || x < 2 || ry == 15 || x == 31;
            let joint = ry == 15 || x == 31;
            let stipple = hash(x, y, 41) * 0.5 + hash(x / 2, y / 2, 42) * 0.5;
            let v = if joint {
                0.6
            } else if margin {
                0.9 + 0.03 * (r - 0.5)
            } else {
                0.78 + 0.14 * stipple + 0.04 * (n(8, 43) - 0.5)
            };
            grey(v)
        }
        Surface::Earth => {
            // Rammed earth: wavy horizontal strata of warm tones, thin darker lifts, pebbles.
            let wave = (3.0 * (vnoise(x, 0, 16, 44) - 0.5)) as i32;
            let yy = (y + wave).rem_euclid(32);
            const BANDS: [(i32, [f32; 3]); 6] = [
                (5, [1.0, 0.93, 0.82]),
                (9, [0.93, 0.83, 0.72]),
                (16, [0.99, 0.9, 0.77]),
                (19, [0.86, 0.75, 0.65]),
                (26, [0.97, 0.88, 0.74]),
                (32, [0.9, 0.8, 0.69]),
            ];
            let i = BANDS.iter().position(|(e, _)| yy < *e).unwrap_or(5);
            let c = BANDS[i].1;
            let k = 1.0 + 0.04 * (n(4, 45) - 0.5) - if yy == BANDS[i].0 - 1 && i % 2 == 1 { 0.06 } else { 0.0 };
            let k = if r > 0.95 { k * 0.85 } else if r < 0.03 { k * 1.06 } else { k };
            rgb(c[0] * k, c[1] * k, c[2] * k)
        }
        Surface::Cladding => {
            // Charred timber (shou sugi ban): vertical 8 px boards, 1 px shadow gap,
            // alligator-crack texture in blocky cells, a lighter batten edge.
            let px = x % 8;
            let plank = x / 8;
            let crack = (y + plank * 5).rem_euclid(5) == 0 && hash(plank, y / 5, 9) > 0.35;
            let grain = vnoise(x * 4, y, 4, 8 + plank as u32);
            let mut v = 0.84 + 0.08 * hash(plank, 0, 9) + 0.07 * (grain - 0.5);
            if px == 7 {
                v = 0.5;
            } else if px == 0 {
                v += 0.07;
            } else if crack {
                v -= 0.12;
            }
            grey(v)
        }
        Surface::Steel => {
            // Flat steel plate with faint vertical mill streaks.
            let streak = vnoise(x, y / 8, 2, 10);
            let v = 0.88 + 0.07 * (streak - 0.5) + if r > 0.96 { -0.06 } else { 0.0 };
            rgb(v, v, v * 1.02)
        }
        Surface::Sedum => {
            // Sedum mat: dense green-tinted speckle of rosettes, with rare flower dots.
            let (d1, _, id) = voronoi(x, y, 8, 13);
            let mut v = 0.95 - (d1 / 2.4).min(1.0) * 0.3 + 0.08 * id;
            if r > 0.985 {
                return rgb(1.0, 0.82, 0.62);
            } else if r > 0.97 {
                return rgb(1.0, 1.0, 0.72);
            }
            v += 0.04 * (n(8, 14) - 0.5);
            rgb(v * 0.95, v, v * 0.88)
        }
        Surface::Seam => {
            // Standing seam: vertical ribs every 8 px (lit side, shadow side), flat pans.
            let px = x % 8;
            let pan = 0.84 + 0.04 * hash(x / 8, 0, 15) + 0.03 * (n(16, 14) - 0.5);
            let v = match px {
                0 => 1.0,
                1 => 0.68,
                _ => pan,
            };
            rgb(v * 0.98, v, v * 1.03)
        }
        Surface::Precast => {
            // Coffered precast: 16 px panels, raised 3 px ribs, recessed centre with a
            // shadowed top/left inner edge and a lit bottom/right one.
            let (cx, cy) = (x % 16, y % 16);
            let rib = cx < 3 || cy < 3;
            let v = if rib {
                if cx == 0 || cy == 0 { 0.8 } else { 0.94 }
            } else if cx == 3 || cy == 3 {
                0.7
            } else if cx == 15 || cy == 15 {
                0.92
            } else {
                0.84 + 0.04 * (n(4, 16) - 0.5)
            };
            grey(v + if r > 0.96 { -0.04 } else { 0.0 })
        }
        Surface::Pavers => {
            // Small square pavers in a grid (8 px), 1 px joints, per-paver tone.
            let joint = x % 8 == 7 || y % 8 == 7;
            if joint {
                grey(0.6)
            } else {
                let id = hash(x / 8, y / 8, 17);
                let edge = x % 8 == 0 || y % 8 == 0;
                grey(0.82 + 0.12 * id + if edge { 0.04 } else { 0.0 } + 0.03 * (r - 0.5))
            }
        }
        Surface::Slabs => {
            // Large slabs (16 px, offset rows) with grassy joints.
            let row = y / 16;
            let xs = (x + if row % 2 == 1 { 8 } else { 0 }).rem_euclid(32);
            let (jx, jy) = (xs % 16, y % 16);
            if jx >= 14 || jy >= 14 {
                let g = 0.7 + 0.2 * hash(x, y, 18);
                rgb(g * 0.62, g * 0.9, g * 0.45)
            } else {
                let id = hash(xs / 16, row, 19);
                grey(0.84 + 0.1 * id + 0.04 * (n(4, 19) - 0.5) + if jx == 0 || jy == 0 { 0.03 } else { 0.0 })
            }
        }
        Surface::Poured => {
            // Poured concrete: smooth, saw-cut joints every 16 px, faint trowel arcs, pits.
            if x % 16 == 15 || y == 31 {
                grey(0.66)
            } else {
                let arc = ((x as f32 * 0.4).sin() + (y as f32 * 0.3).cos() * 0.6) * 0.012;
                grey(0.88 + arc + 0.05 * (n(8, 20) - 0.5) - if r > 0.97 { 0.08 } else { 0.0 })
            }
        }
        Surface::Window => {
            // Deep glazing: 2 px frame, two vertical mullions and a transom; glass untinted
            // (alpha 1) — dark blue-grey with a diagonal sheen and a dim interior.
            let frame = !(2..30).contains(&x) || !(2..30).contains(&y);
            let mullion = x == 15 || x == 16 || y == 10;
            if frame {
                grey(if x == 0 || y == 0 || x == 31 || y == 31 { 0.6 } else { 0.9 })
            } else if mullion {
                grey(0.82)
            } else {
                let sheen = (x + (31 - y)).rem_euclid(17) < 2;
                let mut c = [0.13, 0.16, 0.21, 1.0];
                if sheen {
                    c = [0.38, 0.45, 0.52, 1.0];
                } else if y >= 22 && (19..27).contains(&x) {
                    c = [0.09, 0.09, 0.11, 0.95];
                } else if y < 10 {
                    c = [0.2, 0.24, 0.3, 1.0];
                }
                c
            }
        }
        Surface::Door => {
            // Flush door: vertical boards, recessed frame, long horizontal pull bar.
            let border = !(2..30).contains(&x) || y < 2;
            if border {
                grey(0.62)
            } else if (20..23).contains(&x) && (12..20).contains(&y) {
                grey(if x == 20 { 0.95 } else { 0.4 })
            } else if x % 7 == 1 {
                grey(0.72)
            } else {
                grey(0.84 + 0.06 * hash(x / 7, 3, 21) + 0.05 * (vnoise(x * 4, y, 4, 22) - 0.5))
            }
        }
        Surface::Bark => {
            let ridge = vnoise(x * 2, y / 2, 4, 23);
            let v = if ridge < 0.35 { 0.55 } else { 0.8 + 0.2 * (ridge - 0.35) };
            grey(v)
        }
        Surface::Leaves => {
            let (d1, _, id) = voronoi(x, y, 4, 24);
            let rim = d1 / (TILE as f32 / 4.0);
            let mut v = 0.95 - rim * 0.45 + 0.12 * id;
            if r > 0.9 {
                v += 0.12;
            }
            [v * 0.95, v, v * 0.85, 0.5]
        }
        Surface::Fiber => {
            // Moss / ivy: soft clumps of small leaves with dark gaps.
            let (d1, d2, id) = voronoi(x, y, 6, 25);
            let gap = d2 - d1 < 0.8;
            let v = if gap { 0.68 } else { 0.98 - (d1 / 3.0).min(1.0) * 0.18 + 0.06 * (id - 0.5) };
            rgb(v * 0.96, v, v * 0.9)
        }
        Surface::Petal => grey(0.95 - if r > 0.9 { 0.1 } else { 0.0 } + 0.04 * (n(8, 26) - 0.5)),
        Surface::Metal => {
            let rivet = x % 16 == 4 && y % 16 == 4;
            let edge = x % 16 == 0 || y % 16 == 0;
            grey(if rivet { 1.0 } else if edge { 0.55 } else { 0.78 + 0.08 * n(8, 27) })
        }
        Surface::Glow => {
            let v = 1.0 - 0.08 * n(8, 28);
            [v, v, v, 1.0]
        }
        Surface::Water => {
            let band = ((y as f32 + 3.0 * vnoise(x, 0, 8, 29)) * 0.4).sin();
            grey(if band > 0.9 { 1.0 } else { 0.85 })
        }
    }
}

fn srgb_byte(v: f32) -> u8 {
    (v.clamp(0.0, 1.0) * 255.0).round() as u8
}

/// The atlas as RGBA8 sRGB (alpha linear), nearest sampling.
pub fn atlas_image() -> Image {
    let (w, h) = (COLS * TILE, ROWS * TILE);
    let mut data = vec![0u8; (w * h * 4) as usize];
    for (i, s) in ALL.iter().enumerate() {
        debug_assert_eq!(*s as usize, i);
        let (tx, ty) = (i as u32 % COLS * TILE, i as u32 / COLS * TILE);
        for y in 0..TILE {
            for x in 0..TILE {
                let c = texel(*s, x as i32, y as i32);
                let o = (((ty + y) * w + tx + x) * 4) as usize;
                data[o] = srgb_byte(c[0]);
                data[o + 1] = srgb_byte(c[1]);
                data[o + 2] = srgb_byte(c[2]);
                data[o + 3] = srgb_byte(c[3]);
            }
        }
    }
    let mut img = Image::new(
        Extent3d { width: w, height: h, depth_or_array_layers: 1 },
        TextureDimension::D2,
        data,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::RENDER_WORLD,
    );
    img.sampler = ImageSampler::nearest();
    img
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn atlas_covers_every_surface() {
        assert_eq!(ALL.len(), Surface::COUNT);
        for (i, s) in ALL.iter().enumerate() {
            assert_eq!(*s as usize, i);
        }
        let img = atlas_image();
        assert_eq!(img.width(), COLS * TILE);
        assert_eq!(img.height(), ROWS * TILE);
    }

    #[test]
    fn glow_and_glass_are_emissive() {
        assert_eq!(texel(Surface::Glow, 3, 3)[3], 1.0);
        assert!(texel(Surface::Window, 8, 8)[3] > 0.9);
        assert!(texel(Surface::Concrete, 8, 8)[3] < 0.9);
    }
}
