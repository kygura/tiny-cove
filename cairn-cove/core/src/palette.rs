//! Colour palette (linear RGB tints multiplied with the atlas textures).
//!
//! Vibrant and saturated, like a hand-painted travel poster run through a PS1: electric
//! turquoise shallows deepening to cobalt, saffron sand, lime-to-emerald grass, golden thatch
//! and pigment-painted carvings. No greys, no mud. Player swatches mirror DESIGN.md §Art; the
//! HUD reads the `*_SWATCHES` tables from here. Values returned by the fns are linear.

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

/// Roof swatches (name, sRGB hex), index = `Colors::roof`: thatch, turf, hide and hull tints.
pub const ROOF_SWATCHES: [(&str, u32); ROOF_COLORS as usize] = [
    ("GOLDEN STRAW", 0xF2B33D),
    ("SUNLIT TURF", 0x5BD13F),
    ("SAFFRON", 0xFFC21A),
    ("CORAL", 0xFF6F59),
    ("SEA GLASS", 0x2FE0A0),
    ("INDIGO", 0x3555FF),
];

/// Daub limewash swatches (name, sRGB hex), index = `Colors::wall`.
pub const WALL_SWATCHES: [(&str, u32); WALL_COLORS as usize] = [
    ("CHALK WHITE", 0xFFF4DC),
    ("OCHRE", 0xF5A623),
    ("TERRACOTTA", 0xE8643C),
    ("SKY BLUE", 0x5EC8FF),
    ("SHELL PINK", 0xFF9EC0),
];

/// Pigment swatches (name, sRGB hex), index = `Colors::trim`: carvings, shutters, painted
/// bands, doors, railings.
pub const TRIM_SWATCHES: [(&str, u32); TRIM_COLORS as usize] = [
    ("MADDER", 0xFF5A4E),
    ("MAGENTA", 0xE8399A),
    ("INDIGO", 0x3555FF),
    ("SAFFRON", 0xFFC21A),
    ("SEA GLASS", 0x2FE0A0),
];

/// Player roof colour.
pub fn roof(c: Colors) -> Vec3 {
    srgb(ROOF_SWATCHES[(c.roof % ROOF_COLORS) as usize].1)
}

/// Player daub limewash colour.
pub fn wall(c: Colors) -> Vec3 {
    srgb(WALL_SWATCHES[(c.wall % WALL_COLORS) as usize].1)
}

/// Player trim / pigment colour (carvings, shutters, doors, railings).
pub fn trim(c: Colors) -> Vec3 {
    srgb(TRIM_SWATCHES[(c.trim % TRIM_COLORS) as usize].1)
}

/// Wattle-and-daub limewash in the player's wall colour.
pub fn daub(c: Colors) -> Vec3 {
    wall(c)
}

/// Pigment `i` (wraps): madder, magenta, indigo, saffron, sea glass.
pub fn pigment(i: u32) -> Vec3 {
    srgb(TRIM_SWATCHES[(i as usize) % TRIM_SWATCHES.len()].1)
}

// ---- kit materials -----------------------------------------------------------------------

/// Golden straw thatch.
pub fn thatch() -> Vec3 {
    srgb(0xF2B33D)
}
/// Bright green turf / sod roofs and dyke tops.
pub fn turf() -> Vec3 {
    srgb(0x4FD03A)
}
/// Sun-bleached driftwood: warm cream.
pub fn driftwood() -> Vec3 {
    srgb(0xEED6AE)
}
/// Dry-stone courses: warm honey sandstone.
pub fn dry_stone() -> Vec3 {
    srgb(0xDDB77C)
}
/// Stretched hide / sailcloth.
pub fn hide() -> Vec3 {
    srgb(0xF0A65A)
}
/// Crushed white shell.
pub fn shell() -> Vec3 {
    srgb(0xFFF1E0)
}
/// Warm saffron-cream beach sand.
pub fn sand() -> Vec3 {
    srgb(0xF6D58A)
}
/// Wet sand at the waterline: deeper honey.
pub fn wet_sand() -> Vec3 {
    srgb(0xDDA040)
}
/// Electric turquoise shallows.
pub fn sea_shallow() -> Vec3 {
    srgb(0x19E0D0)
}
/// Saturated cobalt offshore.
pub fn sea_deep() -> Vec3 {
    srgb(0x1050C8)
}
/// Shoreline foam bands.
pub fn foam() -> Vec3 {
    Vec3::ONE
}
/// Orange / yellow lichen (weathering).
pub fn lichen() -> Vec3 {
    srgb(0xFFB21E)
}
/// Daytime shadow tint: saturated violet-blue, never grey.
pub fn shadow() -> Vec3 {
    srgb(0x4A3AA8)
}

// ---- generic tints (shared by every generator) -------------------------------------------

/// Generic cut / dressed stone (steps, kerbs, gateposts).
pub fn stone() -> Vec3 {
    srgb(0xE2C08A)
}
/// Boulder footing: a deeper warm stone.
pub fn foundation() -> Vec3 {
    srgb(0xB98A5C)
}
/// Timber posts, beams and log piles: warm sunlit brown.
pub fn wood() -> Vec3 {
    srgb(0xC07A3A)
}
/// Lush lime-green island grass (lit band).
pub fn grass() -> Vec3 {
    srgb(0x6CD443)
}
/// Sun-dried grass on the dunes.
pub fn grass_dry() -> Vec3 {
    srgb(0xC2DC4A)
}
/// Bare earth: warm terracotta.
pub fn dirt() -> Vec3 {
    srgb(0xC9773E)
}
/// Headland rock: rosy ochre.
pub fn rock() -> Vec3 {
    srgb(0xCC8C66)
}
/// Broadleaf foliage: vivid green.
pub fn leaves() -> Vec3 {
    srgb(0x34C24E)
}
/// Pine / cypress needles: emerald.
pub fn pine() -> Vec3 {
    srgb(0x1F9E5A)
}
pub fn bark() -> Vec3 {
    srgb(0xA25A30)
}
/// Dry marram, straw bundles.
pub fn hay() -> Vec3 {
    srgb(0xF0C85A)
}
/// Iron (fire baskets, lantern frames): deep indigo rather than grey.
pub fn metal() -> Vec3 {
    srgb(0x34308A)
}
/// Lanterns and beacon fires: hot orange.
pub fn glow() -> Vec3 {
    srgb(0xFF8A1E)
}
pub fn white() -> Vec3 {
    Vec3::ONE
}

/// Flower colours (sea pinks first).
pub fn flower(i: u32) -> Vec3 {
    const F: [u32; 6] = [0xFF5FA8, 0xE8399A, 0xFFC21A, 0xFFF4F0, 0xFF5A4E, 0x8A6BFF];
    srgb(F[(i as usize) % F.len()])
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Vibrant: every swatch is saturated (no greys), except deliberate whites.
    #[test]
    fn swatches_are_vibrant() {
        for (name, hex) in ROOF_SWATCHES.iter().chain(&WALL_SWATCHES).chain(&TRIM_SWATCHES) {
            let c = srgb(*hex);
            let (mx, mn) = (c.max_element(), c.min_element());
            assert!(mx > 0.3, "{name} too dark");
            assert!(*name == "CHALK WHITE" || (mx - mn) / mx > 0.4, "{name} not saturated");
        }
    }
}
