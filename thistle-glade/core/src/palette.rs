//! Colour palette (linear RGB tints multiplied with the atlas textures).
//!
//! Warm, cosy, storybook: terracotta and slate roofs, cream plaster, honey stone,
//! saturated meadow green. Values are linear (the renderer works in linear space).

use crate::model::{Colors, ROOF_COLORS, TRIM_COLORS, WALL_COLORS};
use glam::Vec3;

fn srgb(hex: u32) -> Vec3 {
    let c = |v: u32| {
        let s = v as f32 / 255.0;
        if s <= 0.04045 { s / 12.92 } else { ((s + 0.055) / 1.055).powf(2.4) }
    };
    Vec3::new(c((hex >> 16) & 0xff), c((hex >> 8) & 0xff), c(hex & 0xff))
}

const ROOFS: [u32; ROOF_COLORS as usize] = [
    0xc0563a, // terracotta
    0x4f6f8f, // slate blue
    0x3f8a7a, // teal
    0x6c8a3a, // moss green
    0xd0a040, // ochre / thatch gold
    0x7a4a6a, // plum
];

const WALLS: [u32; WALL_COLORS as usize] = [
    0xefe4cc, // cream
    0xf7f3ea, // lime white
    0xe8c9b8, // pale pink
    0xd6c08e, // honey
    0xb9c4c9, // cool grey
];

const TRIMS: [u32; TRIM_COLORS as usize] = [
    0x6b4a2e, // walnut
    0x8c6a43, // oak
    0x3d5a6c, // blue-painted
    0x8a3b2e, // oxblood
    0x4e5e3a, // olive
];

pub fn roof(c: Colors) -> Vec3 {
    srgb(ROOFS[(c.roof % ROOF_COLORS) as usize])
}

pub fn wall(c: Colors) -> Vec3 {
    srgb(WALLS[(c.wall % WALL_COLORS) as usize])
}

pub fn trim(c: Colors) -> Vec3 {
    srgb(TRIMS[(c.trim % TRIM_COLORS) as usize])
}

/// Neutral tints for surfaces that are not player-coloured.
pub fn stone() -> Vec3 {
    srgb(0xcfc4ae)
}
pub fn foundation() -> Vec3 {
    srgb(0x8e8a80)
}
pub fn wood() -> Vec3 {
    srgb(0x9a7550)
}
pub fn grass() -> Vec3 {
    srgb(0x7fb24a)
}
pub fn grass_dry() -> Vec3 {
    srgb(0xa9b85a)
}
pub fn dirt() -> Vec3 {
    srgb(0xa98a62)
}
pub fn rock() -> Vec3 {
    srgb(0x9a958c)
}
pub fn sand() -> Vec3 {
    srgb(0xd8c79a)
}
pub fn leaves() -> Vec3 {
    srgb(0x5e9a3c)
}
pub fn pine() -> Vec3 {
    srgb(0x3e6e4a)
}
pub fn bark() -> Vec3 {
    srgb(0x6e5440)
}
pub fn hay() -> Vec3 {
    srgb(0xe0c060)
}
pub fn grain() -> Vec3 {
    srgb(0xe8c25a)
}
pub fn metal() -> Vec3 {
    srgb(0x3a3a40)
}
pub fn glow() -> Vec3 {
    srgb(0xffd27a)
}
pub fn white() -> Vec3 {
    Vec3::ONE
}

/// Meadow flower colours.
pub fn flower(i: u32) -> Vec3 {
    const F: [u32; 6] = [0xf2f0e6, 0xf2d24a, 0xe86a8a, 0x9a7ae0, 0xe8603a, 0x6aa0f0];
    srgb(F[(i as usize) % F.len()])
}
