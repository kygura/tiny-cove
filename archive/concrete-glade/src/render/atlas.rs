//! Procedural 64x64 texture atlas: 4x4 tiles of 16 px, tile i = `Voxel as u32`.

use crate::voxel::Voxel;
use bevy::{
    asset::RenderAssetUsages,
    image::{Image, ImageSampler},
    render::render_resource::{Extent3d, TextureDimension, TextureFormat},
};

pub const TILE: u32 = 16;
pub const ATLAS_TILES: u32 = 4;
pub const ATLAS_SIZE: u32 = TILE * ATLAS_TILES;

/// Atlas UV rectangle (min, max) of a voxel's tile, inset by half a texel so nearest
/// sampling never bleeds into the neighbouring tile.
pub fn tile_uv_rect(v: Voxel) -> ([f32; 2], [f32; 2]) {
    let i = v.atlas_tile();
    let s = 1.0 / ATLAS_TILES as f32;
    let inset = 0.5 / ATLAS_SIZE as f32;
    let (u, w) = ((i % ATLAS_TILES) as f32 * s, (i / ATLAS_TILES) as f32 * s);
    ([u + inset, w + inset], [u + s - inset, w + s - inset])
}

fn hash(x: u32, y: u32, seed: u32) -> f32 {
    let mut h =
        x.wrapping_mul(0x27d4_eb2d) ^ y.wrapping_mul(0x1656_67b1) ^ seed.wrapping_mul(0x9e37_79b9);
    h ^= h >> 15;
    h = h.wrapping_mul(0x85eb_ca6b);
    h ^= h >> 13;
    h = h.wrapping_mul(0xc2b2_ae35);
    h ^= h >> 16;
    (h & 0xffff) as f32 / 65535.0
}

fn hex(c: u32) -> [f32; 3] {
    [
        (c >> 16) as f32,
        ((c >> 8) & 0xff) as f32,
        (c & 0xff) as f32,
    ]
}

fn shade(c: [f32; 3], k: f32) -> [f32; 3] {
    c.map(|v| v * k)
}

fn mix(a: [f32; 3], b: [f32; 3], t: f32) -> [f32; 3] {
    [
        a[0] + (b[0] - a[0]) * t,
        a[1] + (b[1] - a[1]) * t,
        a[2] + (b[2] - a[2]) * t,
    ]
}

/// Blocky 2x2 value noise in roughly -1..1.
fn noise(x: u32, y: u32, seed: u32) -> f32 {
    (hash(x / 2, y / 2, seed) * 0.6 + hash(x, y, seed + 1) * 0.4) * 2.0 - 1.0
}

/// Window: 1 px frame, mullion cross, glass in between.
fn window(x: u32, y: u32, frame: [f32; 3], glass: [f32; 3], lit: bool) -> [f32; 3] {
    if x == 0 || y == 0 || x == 15 || y == 15 || x == 7 || y == 8 {
        return frame;
    }
    if lit {
        // Warmer at the bottom, a dim figure-shaped blot in one pane.
        let g = shade(glass, 0.92 + y as f32 * 0.008);
        if (10..13).contains(&x) && y >= 11 {
            shade(glass, 0.7)
        } else {
            g
        }
    } else if (x + 15 - y).is_multiple_of(9) {
        mix(glass, hex(0x3a4048), 0.6)
    } else {
        shade(glass, 1.0 + noise(x, y, 7) * 0.08)
    }
}

fn texel(v: Voxel, x: u32, y: u32) -> [f32; 3] {
    match v {
        Voxel::Air => hex(0x6b6258),
        Voxel::Ash => shade(hex(0x7a766e), 1.0 + noise(x, y, 1) * 0.07),
        Voxel::Scorch => {
            let n = noise(x, y, 2);
            shade(
                hex(0x3d3833),
                1.0 + n * 0.12 - if hash(x, y, 3) > 0.9 { 0.15 } else { 0.0 },
            )
        }
        Voxel::Asphalt => {
            // Wandering crack down the tile plus a short branch.
            let cx = 5 + (0..=y).map(|i| hash(0, i, 4) > 0.6).filter(|&b| b).count() as u32 / 2;
            let crack = x == cx.min(15) || (y == 10 && (cx..cx + 5).contains(&x));
            // Cooler and darker than scorch (#3d3833) so roads read as roads.
            if crack {
                hex(0x45484c)
            } else {
                shade(hex(0x222428), 1.0 + noise(x, y, 5) * 0.05)
            }
        }
        Voxel::Rubble => {
            let gap = x % 5 == 4 && hash(x / 5, y / 4, 6) > 0.4 || y % 4 == 3;
            let k = 0.85 + hash(x / 5, y / 4, 7) * 0.3;
            if gap {
                hex(0x3d3833)
            } else {
                shade(hex(0x5e5a54), k + noise(x, y, 8) * 0.04)
            }
        }
        Voxel::Concrete => concrete(x, y, hex(0x9a958b), hex(0x87827a), 9),
        Voxel::ConcreteDark => concrete(x, y, hex(0x6a665f), hex(0x58544e), 10),
        Voxel::WindowDark => window(x, y, hex(0x45423d), hex(0x1d1f22), false),
        Voxel::WindowLit => window(x, y, hex(0x5a4a33), hex(0xe3b35a), true),
        Voxel::Rust => {
            let h = hash(x, y, 11);
            let base = shade(hex(0x7b4a2e), 1.0 + noise(x, y, 12) * 0.08);
            if h > 0.92 {
                hex(0x9a6038)
            } else if h < 0.1 {
                hex(0x55311f)
            } else {
                base
            }
        }
        Voxel::DeadWood => {
            let grain = (x + (hash(0, y / 3, 13) * 2.0) as u32).is_multiple_of(4);
            shade(
                hex(0x3b3029),
                if grain {
                    0.78
                } else {
                    1.0 + noise(x, y, 14) * 0.05
                },
            )
        }
        Voxel::Lamp => {
            let d = (x as f32 - 7.5).abs().max((y as f32 - 7.5).abs());
            if d > 6.0 {
                hex(0x4a2a20)
            } else {
                mix(hex(0xf0a070), hex(0xd8452f), (d / 6.0).powf(0.7))
            }
        }
    }
}

/// Board-form concrete: horizontal lines every 4 px, tie holes, drip streaks.
fn concrete(x: u32, y: u32, base: [f32; 3], line: [f32; 3], seed: u32) -> [f32; 3] {
    let streak_len = |col: u32| (hash(col, 0, seed) * 14.0) as u32;
    let mut c = shade(base, 1.0 + noise(x, y, seed + 20) * 0.03);
    if y % 4 == 3 {
        c = line;
    }
    if (x == 3 || x == 12) && y % 8 == 1 {
        c = shade(line, 0.85);
    }
    for col in [2, 9, 13] {
        if x == col && y <= streak_len(col) {
            c = mix(c, shade(line, 0.8), 0.55 * (1.0 - y as f32 / 16.0));
        }
    }
    c
}

const ALL: [Voxel; 12] = [
    Voxel::Air,
    Voxel::Ash,
    Voxel::Scorch,
    Voxel::Asphalt,
    Voxel::Rubble,
    Voxel::Concrete,
    Voxel::ConcreteDark,
    Voxel::WindowDark,
    Voxel::WindowLit,
    Voxel::Rust,
    Voxel::DeadWood,
    Voxel::Lamp,
];

/// Raw sRGB RGBA8 pixels of the atlas, row-major.
pub fn atlas_pixels() -> Vec<u8> {
    let n = ATLAS_SIZE as usize;
    let mut data = vec![0u8; n * n * 4];
    for i in 0..ATLAS_TILES * ATLAS_TILES {
        let v = ALL.get(i as usize).copied().unwrap_or(Voxel::Air);
        debug_assert!(i as usize >= ALL.len() || v.atlas_tile() == i);
        for y in 0..TILE {
            for x in 0..TILE {
                let c = texel(v, x, y);
                let px = (i % ATLAS_TILES * TILE + x) as usize;
                let py = (i / ATLAS_TILES * TILE + y) as usize;
                let o = (py * n + px) * 4;
                for k in 0..3 {
                    data[o + k] = c[k].round().clamp(0.0, 255.0) as u8;
                }
                data[o + 3] = 255;
            }
        }
    }
    data
}

/// The atlas as a nearest-sampled sRGB image without mipmaps.
pub fn atlas_image() -> Image {
    let mut image = Image::new(
        Extent3d {
            width: ATLAS_SIZE,
            height: ATLAS_SIZE,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        atlas_pixels(),
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::RENDER_WORLD,
    );
    image.sampler = ImageSampler::nearest();
    image
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tiles_match_voxel_indices() {
        for (i, v) in ALL.iter().enumerate() {
            assert_eq!(v.atlas_tile(), i as u32);
        }
        let ([u0, v0], [u1, v1]) = tile_uv_rect(Voxel::Concrete);
        assert!(u0 > 0.25 && u1 < 0.5 && v0 > 0.25 && v1 < 0.5);
    }
}
