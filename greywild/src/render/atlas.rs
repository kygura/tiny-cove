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
use glade_core::Surface;

pub const TILE: u32 = 32;
pub const COLS: u32 = 8;
pub const ROWS: u32 = (Surface::COUNT as u32).div_ceil(COLS);

const ALL: [Surface; Surface::COUNT] = [
    Surface::Grass,
    Surface::Dirt,
    Surface::Rock,
    Surface::Sand,
    Surface::Stone,
    Surface::Foundation,
    Surface::Plaster,
    Surface::Planks,
    Surface::Timber,
    Surface::RoofTile,
    Surface::Slate,
    Surface::Thatch,
    Surface::Cobble,
    Surface::Flagstone,
    Surface::Setts,
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

/// Running-bond blocks: returns (is_joint, block id hash, top-edge highlight).
fn blocks(x: i32, y: i32, row_h: i32, min_w: i32, seed: u32) -> (bool, f32, bool) {
    let row = y / row_h;
    let ry = y % row_h;
    // Joint positions along the row: deterministic widths summing to 32.
    let shift = (hash(row, 0, seed) * TILE as f32) as i32;
    let xs = (x + shift).rem_euclid(TILE as i32);
    let mut start = 0;
    let mut idx = 0;
    loop {
        let w = min_w + (hash(row, idx + 1, seed) * min_w as f32 * 0.8) as i32;
        let end = if start + w + min_w > TILE as i32 { TILE as i32 } else { start + w };
        if xs < end {
            let joint = ry == row_h - 1 || xs == end - 1;
            let id = hash(row, idx, seed + 3);
            return (joint, id, ry == 0);
        }
        start = end;
        idx += 1;
    }
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
        Surface::Sand => grey(0.92 + (r - 0.5) * 0.1 + 0.04 * n(16, 4)),
        Surface::Stone => {
            let (joint, id, top) = blocks(x, y, 8, 9, 31);
            if joint {
                grey(0.62)
            } else {
                let v = 0.8 + 0.18 * id + 0.06 * (n(4, 5) - 0.5) + if top { 0.07 } else { 0.0 };
                grey(v)
            }
        }
        Surface::Foundation => {
            let (joint, id, top) = blocks(x, y, 11, 13, 41);
            if joint {
                grey(0.45)
            } else {
                grey(0.74 + 0.2 * id + 0.08 * (n(4, 6) - 0.5) + if top { 0.08 } else { 0.0 })
            }
        }
        Surface::Plaster => {
            let mut v = 0.96 + 0.05 * (n(16, 7) - 0.5);
            if r > 0.95 {
                v -= 0.07;
            }
            grey(v)
        }
        Surface::Planks => {
            let px = x % 8;
            let plank = x / 8;
            let grain = vnoise(x * 4, y, 4, 8 + plank as u32);
            let mut v = 0.82 + 0.12 * hash(plank, 0, 9) + 0.08 * (grain - 0.5);
            if px == 7 {
                v = 0.45;
            } else if px == 0 {
                v += 0.06;
            }
            if (px == 3) && (y % 16 == 3) {
                v = 0.4; // nail
            }
            grey(v)
        }
        Surface::Timber => {
            let grain = vnoise(x, y * 4, 4, 10);
            let ring = ((y as f32 + 3.0 * vnoise(x, 0, 16, 12)) * 0.7).sin();
            grey(0.82 + 0.1 * (grain - 0.5) + if ring > 0.85 { -0.12 } else { 0.0 })
        }
        Surface::RoofTile => {
            // Rows of 8 px; each tile 8 px wide with rounded lower edge.
            let row = y / 8;
            let ry = y % 8;
            let xs = (x + if row % 2 == 1 { 4 } else { 0 }).rem_euclid(32);
            let tx = xs % 8;
            let id = hash(xs / 8, row, 13);
            let edge = (tx as f32 - 3.5).abs() / 4.0; // 0 centre .. 1 edge
            let bottom = 7 - (edge * edge * 2.0) as i32;
            let mut v = 0.82 + 0.15 * id + 0.12 * (1.0 - edge);
            if ry >= bottom || tx == 0 {
                v = 0.5;
            } else if ry == 0 {
                v -= 0.1;
            }
            grey(v)
        }
        Surface::Slate => {
            let row = y / 6;
            let ry = y % 6;
            let shift = (hash(row, 1, 14) * 32.0) as i32;
            let xs = (x + shift).rem_euclid(32);
            let w = 8;
            let id = hash(xs / w, row, 15);
            let v = if ry == 5 || xs % w == 0 { 0.48 } else { 0.78 + 0.18 * id - 0.03 * ry as f32 };
            rgb(v * 0.97, v, v * 1.04)
        }
        Surface::Thatch => {
            let layer = y / 10;
            let ly = y % 10;
            let strand = vnoise(x * 3 + layer * 7, y, 3, 16);
            let mut v = 0.78 + 0.25 * strand;
            if ly == 9 {
                v = 0.5;
            } else if ly == 8 {
                v *= 0.8;
            }
            if r > 0.9 {
                v *= 1.1;
            }
            rgb(v, v * 0.96, v * 0.86)
        }
        Surface::Cobble => {
            let (d1, d2, id) = voronoi(x, y, 5, 17);
            if d2 - d1 < 1.6 {
                grey(0.45)
            } else {
                let dome = (1.0 - d1 / 5.0).clamp(0.0, 1.0);
                grey(0.68 + 0.18 * id + 0.18 * dome)
            }
        }
        Surface::Flagstone => {
            let (d1, d2, id) = voronoi(x, y, 2, 18);
            if d2 - d1 < 1.4 {
                grey(0.5)
            } else {
                grey(0.78 + 0.16 * id + 0.06 * (n(4, 19) - 0.5))
            }
        }
        Surface::Setts => {
            let row = y / 6;
            let xs = (x + if row % 2 == 1 { 4 } else { 0 }).rem_euclid(32);
            let id = hash(xs / 8, row, 20);
            if y % 6 == 5 || xs % 8 == 7 {
                grey(0.42)
            } else {
                grey(0.7 + 0.22 * id + if y % 6 == 0 { 0.06 } else { 0.0 })
            }
        }
        Surface::Window => {
            // Frame 3 px, mullion cross 2 px; glass untinted (alpha 1).
            let frame = !(3..29).contains(&x) || !(3..29).contains(&y);
            let mullion = x == 15 || x == 16 || y == 15 || y == 16;
            if frame {
                let v = if x == 0 || y == 0 || x == 31 || y == 31 { 0.55 } else { 0.95 };
                grey(v)
            } else if mullion {
                grey(0.82)
            } else {
                // Dark glass with a diagonal sheen; interior silhouette lower right.
                let sheen = (x + (31 - y)).rem_euclid(14) < 2;
                let mut c = [0.16, 0.2, 0.26, 1.0];
                if sheen {
                    c = [0.42, 0.5, 0.58, 1.0];
                } else if y >= 20 && (19..26).contains(&x) {
                    c = [0.1, 0.11, 0.14, 0.95];
                }
                c
            }
        }
        Surface::Door => {
            let px = x % 8;
            let strap = (5..8).contains(&y) || (24..27).contains(&y);
            let border = !(2..30).contains(&x) || y < 2;
            if strap && (2..30).contains(&x) {
                let rivet = y % 3 == 1 && x % 6 == 3;
                grey(if rivet { 0.55 } else { 0.28 })
            } else if border {
                grey(0.6)
            } else if px == 7 {
                grey(0.45)
            } else {
                grey(0.8 + 0.12 * hash(x / 8, 3, 21) + 0.06 * (vnoise(x * 4, y, 4, 22) - 0.5))
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
            let a = (x + y).rem_euclid(4) == 0;
            let b = (x - y).rem_euclid(6) == 0;
            grey(0.85 + 0.1 * (n(4, 25) - 0.5) - if a { 0.12 } else { 0.0 } - if b { 0.06 } else { 0.0 })
        }
        Surface::Petal => grey(0.95 - if r > 0.9 { 0.1 } else { 0.0 } + 0.04 * (n(8, 26) - 0.5)),
        Surface::Metal => {
            let rivet = x % 16 == 4 && y % 16 == 4;
            let edge = x % 16 == 0 || y % 16 == 0;
            grey(if rivet { 1.0 } else if edge { 0.5 } else { 0.75 + 0.1 * n(8, 27) })
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
        assert!(texel(Surface::Stone, 8, 8)[3] < 0.9);
    }
}
