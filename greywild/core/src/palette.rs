//! Colour palette (linear RGB tints multiplied with the atlas textures).
//!
//! Eco-brutalist: cool-to-warm concrete greys, corten and copper accents, sedum and zinc
//! roofs, and lush saturated greens that read clearly against the grey (nature taking the
//! concrete back). Player swatches are the authoritative values of DESIGN.md §2.3; the HUD
//! reads the `*_SWATCHES` tables from here. Values returned by the fns are linear.

use crate::model::{Colors, ROOF_COLORS, TRIM_COLORS, WALL_COLORS};
use glam::Vec3;

/// sRGB hex (`0xRRGGBB`) → linear RGB.
pub fn srgb(hex: u32) -> Vec3 {
    let c = |v: u32| {
        let s = v as f32 / 255.0;
        if s <= 0.04045 { s / 12.92 } else { ((s + 0.055) / 1.055).powf(2.4) }
    };
    Vec3::new(c((hex >> 16) & 0xff), c((hex >> 8) & 0xff), c(hex & 0xff))
}

/// Roof swatches (name, sRGB hex), index = `Colors::roof`.
pub const ROOF_SWATCHES: [(&str, u32); ROOF_COLORS as usize] = [
    ("SEDUM", 0x6A8A3E),
    ("DRY SEDUM", 0x9A8F4E),
    ("VERDIGRIS", 0x5FA08C),
    ("ZINC", 0x8C9499),
    ("CONCRETE", 0xA3A29C),
    ("BITUMEN", 0x3E403F),
];

/// Concrete swatches (name, sRGB hex), index = `Colors::wall`.
pub const CONCRETE_SWATCHES: [(&str, u32); WALL_COLORS as usize] = [
    ("PALE BOARD-FORM", 0xC9C6BD),
    ("MID GREY", 0xA09F99),
    ("SAND", 0xC4B396),
    ("CHARCOAL", 0x5A5B57),
    ("LIME WHITE", 0xE4E1D8),
];

/// Accent swatches (name, sRGB hex), index = `Colors::trim`: frames, railings, fins, doors.
pub const ACCENT_SWATCHES: [(&str, u32); TRIM_COLORS as usize] = [
    ("CORTEN RUST", 0x9C4A26),
    ("OXIDISED COPPER", 0x4F8F7E),
    ("BLACK STEEL", 0x2B2D2E),
    ("SAFETY YELLOW", 0xE2B227),
    ("TERRACOTTA", 0xB9603F),
];

pub fn roof(c: Colors) -> Vec3 {
    srgb(ROOF_SWATCHES[(c.roof % ROOF_COLORS) as usize].1)
}

/// Player concrete / render colour.
pub fn wall(c: Colors) -> Vec3 {
    srgb(CONCRETE_SWATCHES[(c.wall % WALL_COLORS) as usize].1)
}

/// Player accent colour (steel, frames, timber stain).
pub fn trim(c: Colors) -> Vec3 {
    srgb(ACCENT_SWATCHES[(c.trim % TRIM_COLORS) as usize].1)
}

/// Neutral tints for surfaces that are not player-coloured.
/// Default board-formed concrete (slightly warm pale grey).
pub fn stone() -> Vec3 {
    srgb(0xC2BFB6)
}
/// Plinth / foundation: darker, cooler bush-hammered concrete.
pub fn foundation() -> Vec3 {
    srgb(0x7E7F7B)
}
/// Charred / weathered timber: dark warm brown-grey.
pub fn wood() -> Vec3 {
    srgb(0x6A5444)
}
/// Lush meadow green, saturated so it pops against the concrete.
pub fn grass() -> Vec3 {
    srgb(0x62A23A)
}
pub fn grass_dry() -> Vec3 {
    srgb(0x9DAE52)
}
pub fn dirt() -> Vec3 {
    srgb(0x8E7458)
}
pub fn rock() -> Vec3 {
    srgb(0x8F8E88)
}
/// Gravel / sand: warm light grey.
pub fn sand() -> Vec3 {
    srgb(0xBDB5A3)
}
/// Broadleaf foliage: deep vivid green.
pub fn leaves() -> Vec3 {
    srgb(0x4E9636)
}
pub fn pine() -> Vec3 {
    srgb(0x2F6446)
}
pub fn bark() -> Vec3 {
    srgb(0x5E4E42)
}
/// Dry grass, straw, compost tops.
pub fn hay() -> Vec3 {
    srgb(0xC8AE5A)
}
pub fn grain() -> Vec3 {
    srgb(0xD8B85A)
}
/// Dark painted steel / housings.
pub fn metal() -> Vec3 {
    srgb(0x34373A)
}
/// Sodium amber lamp glass.
pub fn glow() -> Vec3 {
    srgb(0xFFB04A)
}
/// Moss on concrete: vivid yellow-green.
pub fn moss() -> Vec3 {
    srgb(0x6E9E2E)
}
/// Corten rust (for staining / rebar).
pub fn rust() -> Vec3 {
    srgb(0x8E4524)
}
pub fn white() -> Vec3 {
    Vec3::ONE
}

/// Wildflower colours.
pub fn flower(i: u32) -> Vec3 {
    const F: [u32; 6] = [0xF2F0E6, 0xF2CC3A, 0xE0607A, 0x8E6ADA, 0xE8703A, 0x5E92E8];
    srgb(F[(i as usize) % F.len()])
}
