//! Voxel palette from DESIGN.md, indexed by the `acre_core::Voxel` discriminant (u8).

/// One palette row. Colours are sRGB bytes.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PaletteEntry {
    pub albedo: [u8; 3],
    /// Emissive colour; only meaningful when `day`/`night` strengths are non-zero.
    pub emissive: [u8; 3],
    /// Emissive strength at full day (sun factor 1).
    pub day: f32,
    /// Emissive strength at night (sun factor 0).
    pub night: f32,
}

const fn hex(v: u32) -> [u8; 3] {
    [(v >> 16) as u8, (v >> 8) as u8, v as u8]
}

const fn plain(albedo: u32) -> PaletteEntry {
    PaletteEntry { albedo: hex(albedo), emissive: [0; 3], day: 0.0, night: 0.0 }
}

const fn glow(albedo: u32, emissive: u32, day: f32, night: f32) -> PaletteEntry {
    PaletteEntry { albedo: hex(albedo), emissive: hex(emissive), day, night }
}

/// Voxel kind ids (SPEC order).
pub mod kind {
    pub const AIR: u8 = 0;
    pub const SOIL: u8 = 1;
    pub const GRASS: u8 = 2;
    pub const MEADOW: u8 = 3;
    pub const ROCK: u8 = 4;
    pub const SAND: u8 = 5;
    pub const WATER: u8 = 6;
    pub const ASPHALT: u8 = 7;
    pub const GRAVEL: u8 = 8;
    pub const CONCRETE: u8 = 9;
    pub const CONCRETE_DARK: u8 = 10;
    pub const GLASS: u8 = 11;
    pub const WINDOW_LIT: u8 = 12;
    pub const RUST: u8 = 13;
    pub const STEEL: u8 = 14;
    pub const IVY: u8 = 15;
    pub const WOOD: u8 = 16;
    pub const LEAVES: u8 = 17;
    pub const BLOSSOM: u8 = 18;
    pub const CROP: u8 = 19;
    pub const CRYSTAL: u8 = 20;
    pub const MONOLITH: u8 = 21;
    pub const RUNE: u8 = 22;
    pub const LAMP: u8 = 23;
    pub const SOLAR: u8 = 24;
}

/// Palette indexed by voxel id. Index 0 (Air) is never meshed; it maps to magenta-free
/// neutral grey so a stray Air vertex is visible but not alarming.
pub const PALETTE: [PaletteEntry; 25] = [
    plain(0x808080),                            // Air
    plain(0x7a5a43),                            // Soil
    plain(0x7fb24a),                            // Grass
    plain(0xa7c25a),                            // Meadow
    plain(0x8a8a93),                            // Rock
    plain(0xe2cf9a),                            // Sand
    plain(0x4fa3b8),                            // Water
    plain(0x4b4a52),                            // Asphalt
    plain(0xc8b89c),                            // Gravel
    plain(0xd6cbbd),                            // Concrete
    plain(0xa89d92),                            // ConcreteDark
    plain(0x5b7f99),                            // Glass
    glow(0xffd27a, 0xffc35c, 0.25, 1.4),        // WindowLit
    plain(0xb5653a),                            // Rust
    plain(0x7d8a96),                            // Steel
    plain(0x4f8a3c),                            // Ivy
    plain(0x8c6242),                            // Wood
    plain(0x5d9e45),                            // Leaves
    plain(0xf2a7b8),                            // Blossom
    glow(0xf0c75a, 0xffd86b, 0.15, 0.9),        // Crop
    glow(0x7fe3d6, 0x5cf0e0, 0.35, 1.6),        // Crystal
    plain(0x5a5670),                            // Monolith
    glow(0xb9a2ff, 0xa98bff, 0.5, 1.8),         // Rune
    glow(0xfff0c2, 0xffe2a0, 0.2, 2.0),         // Lamp
    plain(0x33486e),                            // Solar
];

/// Palette entry for a voxel id; unknown ids fall back to Air's grey.
pub fn entry(voxel: u8) -> PaletteEntry {
    PALETTE.get(voxel as usize).copied().unwrap_or(PALETTE[0])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spot_check_rows() {
        assert_eq!(entry(kind::CONCRETE).albedo, [0xd6, 0xcb, 0xbd]);
        assert_eq!(entry(kind::SOLAR).albedo, [0x33, 0x48, 0x6e]);
        assert_eq!(entry(kind::CRYSTAL).night, 1.6);
        assert_eq!(entry(200), PALETTE[0]);
        let glowing = PALETTE.iter().filter(|e| e.night > 0.0).count();
        assert_eq!(glowing, 5);
    }
}
