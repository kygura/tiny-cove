//! Colour palette: DESIGN.md §Art "Folk Pigment", implemented literally.
//!
//! Every token is a 5-bit-exact sRGB hex (DESIGN A0). The named fns return **linear** RGB
//! (`srgb(hex)`), which is what meshes write as vertex tint; the `*_HEX` consts and the
//! `*_SWATCHES` tables keep the raw sRGB for the HUD and for [`shade`].
//!
//! [`shade`] is the CPU port of the cel band function in `src/render/glade.wesl` (DESIGN A3).
//! The `ROOF_SHADES` / `WALL_SHADES` / `TRIM_SHADES` tables (noon mid / noon shadow, read by the
//! HUD's split swatch chips) and the calibration goldens are unit-tested against it.

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

/// sRGB hex → sRGB-encoded components in 0..1.
pub fn hex_to_unit(hex: u32) -> Vec3 {
    Vec3::new(((hex >> 16) & 0xff) as f32, ((hex >> 8) & 0xff) as f32, (hex & 0xff) as f32) / 255.0
}

/// sRGB components in 0..1 → nearest hex (no quantise to 5 bits).
pub fn unit_to_hex(c: Vec3) -> u32 {
    let b = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u32;
    (b(c.x) << 16) | (b(c.y) << 8) | b(c.z)
}

// ---- swatches (DESIGN A1) ----------------------------------------------------------------

/// Roof swatches (name, sRGB hex), index = `Colors::roof`.
pub const ROOF_SWATCHES: [(&str, u32); ROOF_COLORS as usize] = [
    ("GOLDEN STRAW", 0xEFB53A),
    ("SUN-BLEACHED", 0xFFDE84),
    ("MADDER DYED", 0xE6523A),
    ("WOAD DYED", 0x426BFF),
    ("MALACHITE", 0x21CE8C),
    ("COCHINEAL", 0xD63A8C),
];

/// Wall swatches (name, sRGB hex), index = `Colors::wall`: daub limewash, painted plank
/// bands, mortar wash.
pub const WALL_SWATCHES: [(&str, u32); WALL_COLORS as usize] = [
    ("CHALK LIME", 0xF7EFDE),
    ("OCHRE WASH", 0xF7B53A),
    ("TERRACOTTA", 0xE6633A),
    ("SKY WASH", 0x5AC5FF),
    ("ROSE LIME", 0xFF8CBD),
];

/// Trim / pigment swatches (name, sRGB hex), index = `Colors::trim`.
pub const TRIM_SWATCHES: [(&str, u32); TRIM_COLORS as usize] = [
    ("MADDER", 0xFF5A4A),
    ("COCHINEAL", 0xE63A9C),
    ("INDIGO", 0x3152FF),
    ("SAFFRON", 0xFFC519),
    ("SEA-GLASS", 0x31DEA5),
];

/// Noon (mid, shadow) band colours of each roof swatch (DESIGN A1), parallel to
/// [`ROOF_SWATCHES`]. The HUD's split chips read these.
pub const ROOF_SHADES: [(u32, u32); ROOF_COLORS as usize] = [
    (0xCE7B3A, 0xAD3A29),
    (0xD6A57B, 0xB56B63),
    (0xC5314A, 0xA51952),
    (0x523ADE, 0x3119C5),
    (0x21ADB5, 0x0873A5),
    (0xB5319C, 0x9C19A5),
];
/// Noon (mid, shadow) of each wall swatch, parallel to [`WALL_SWATCHES`].
pub const WALL_SHADES: [(u32, u32); WALL_COLORS as usize] = [
    (0xA59CD6, 0x847BBD),
    (0xCE733A, 0xAD3A29),
    (0xC5313A, 0xA51942),
    (0x527BDE, 0x3131C5),
    (0xD67BBD, 0xB552B5),
];
/// Noon (mid, shadow) of each trim swatch, parallel to [`TRIM_SWATCHES`].
pub const TRIM_SHADES: [(u32, u32); TRIM_COLORS as usize] = [
    (0xD64263, 0xB52163),
    (0xC531AD, 0xA519AD),
    (0x4229DE, 0x2910C5),
    (0xD67B21, 0xB54219),
    (0x31BDC5, 0x1973AD),
];

/// Player roof colour.
pub fn roof(c: Colors) -> Vec3 {
    srgb(ROOF_SWATCHES[(c.roof % ROOF_COLORS) as usize].1)
}
/// Player wall colour (daub limewash, painted plank bands, mortar wash).
pub fn wall(c: Colors) -> Vec3 {
    srgb(WALL_SWATCHES[(c.wall % WALL_COLORS) as usize].1)
}
/// Player trim / pigment colour.
pub fn trim(c: Colors) -> Vec3 {
    srgb(TRIM_SWATCHES[(c.trim % TRIM_COLORS) as usize].1)
}
/// Wattle-and-daub limewash in the player's wall colour.
pub fn daub(c: Colors) -> Vec3 {
    wall(c)
}
/// Pigment `i` (wraps): madder, cochineal, indigo, saffron, sea-glass.
pub fn pigment(i: u32) -> Vec3 {
    srgb(TRIM_SWATCHES[(i as usize) % TRIM_SWATCHES.len()].1)
}

// ---- tokens (DESIGN A1). One fn per token; the hex lives in the matching const. ----------

macro_rules! tokens {
    ($($(#[$m:meta])* $name:ident = $hex:literal;)*) => {
        /// Every token as `(name, sRGB hex)`, for tests and tools.
        pub const TOKENS: &[(&str, u32)] = &[$((stringify!($name), $hex)),*];
        $( $(#[$m])* pub fn $name() -> Vec3 { srgb($hex) } )*
    };
}

tokens! {
    // Ground and stone.
    /// Grass and lawn albedo.
    grass = 0x6BD642;
    /// Sunlit blade ticks, rim on turf crests.
    grass_tip = 0xADEF4A;
    /// DuneGrass paint layer.
    marram = 0xDEE65A;
    /// Dry beach, saffron-cream.
    sand = 0xF7D68C;
    /// Honey band above the current tide line.
    wet_sand = 0xDEA54A;
    /// Wrack line at the high-tide mark.
    tide_mark = 0xCE7B31;
    /// Red-ochre earth.
    dirt = 0xD6733A;
    /// Rose-granite boulders, sea stacks.
    rock = 0xCE9C84;
    /// Red-sandstone headland faces.
    cliff = 0xB5635A;
    /// Ochre strata stripe on cliffs and stacks.
    cliff_band = 0xEFA55A;
    /// Dyke, beacon and gatepost stones (warm buff).
    dry_stone = 0xE6C594;
    /// Every third dyke stone: lilac slate.
    dry_stone_alt = 0xBDA5EF;
    /// Joints and undercuts in coursed stone.
    stone_joint = 0x84529C;
    /// Crushed-shell track.
    shell = 0xF7EFE6;
    /// Shell chips, shell strings, pink sand specks.
    shell_nacre = 0xFFADA5;
    pebble_lilac = 0xA59CEF;
    pebble_rose = 0xEFB59C;
    /// Blue slate slabs.
    flagstone = 0x5A73CE;
    flagstone_edge = 0x7B8CEF;
    /// Weathering clumps, orange.
    lichen = 0xFF9C19;
    lichen_yellow = 0xEFE63A;
    /// Weathering lerp target for wood, daub and thatch.
    salt_bleach = 0xF7EFD6;
    /// Wrack paint layer (amber kelp).
    wrack = 0xCE9C10;

    // Building materials.
    /// Golden straw (equals roof swatch 0).
    thatch = 0xEFB53A;
    thatch_tip = 0xFFCE6B;
    thatch_bind = 0xD67B19;
    /// Turf roof and turf dyke top.
    turf = 0x5ADE4A;
    /// Sod-band edges.
    sod = 0x9C4A29;
    /// Stretched hide, door curtains.
    hide = 0xEFA56B;
    hide_seam = 0xDE8C4A;
    /// Pennant ground, hide-roof light panels.
    sailcloth = 0xF7E6AD;
    /// Bleached posts, palisade stakes, driftwood windbreak, woodpile.
    driftwood = 0xEFD6B5;
    /// Grain lines on driftwood (lilac).
    driftwood_grain = 0xC5A5C5;
    /// Oiled corner posts, jetty piles, sleepers, mooring posts.
    timber = 0xBD6329;
    palm_trunk = 0xCE8C4A;
    /// Sea pine, fig and cypress trunks, log steps.
    bark = 0x8C4231;
    bark_hi = 0xB55A42;
    /// Jetty deck, boardwalk, plank walls, bridge planks.
    boards = 0xDE9C5A;
    boards_hi = 0xEFBD7B;
    board_gap = 0x7B3129;
    /// Lashings, rope coils, rope bridges, nets.
    rope = 0xEFC56B;
    /// Reed screens, baskets, wattle weave.
    reed = 0xD6C54A;
    fiber_dark = 0xB58C29;
    /// Jars, firepit daub, crab-pot weights.
    clay = 0xDE7342;
    /// Beacon basket, hoops (plum-indigo, never black).
    iron = 0x4A3A73;
    iron_hi = 0x6B5AA5;
    iron_hot = 0xFF6B29;

    // Plants.
    leaf_sea_pine = 0x21AD73;
    leaf_palm = 0x8CDE3A;
    leaf_cypress = 0x108C63;
    leaf_fig = 0x42C54A;
    fig_fruit = 0xB5317B;
    /// Bleached dead tree.
    snag = 0xEFE6CE;
    petal_sea_pink = 0xFF5AB5;
    petal_sea_pink_deep = 0xE63A9C;
    petal_gorse = 0xFFD619;
    petal_flax = 0x5A8CFF;

    // Fire and light.
    flame_white = 0xFFF7AD;
    glow_core = 0xFFE65A;
    glow_fire = 0xFF7B19;
    glow_ember = 0xFF3A31;
    ember_fade = 0x84195A;
    window_light = 0xFFB542;
    /// Point light colour of a `Fire` marker.
    fire_light = 0xFF8C29;
    /// Point light colour of a `Lantern` / `WindowLight`.
    lantern_light = 0xFFA542;

    // Water, sky, ink and life.
    /// Depth < 1.5 m.
    sea_shallow = 0x19DECE;
    /// Depth 1.5–4 m.
    sea_mid = 0x08ADD6;
    /// Depth 4–10 m.
    sea_deep = 0x1052C5;
    /// Depth > 10 m and the fogged horizon.
    sea_abyss = 0x1929A5;
    /// Shore foam bands, whitecaps (the only pure white).
    foam = 0xFFFFFF;
    foam_shade = 0x8CEFEF;
    /// Hard cel sun specular.
    glint = 0xFFFFDE;
    lagoon = 0x42EFC5;
    lagoon_deep = 0x19C5BD;
    lagoon_rim = 0xCEFFF7;
    outline_day = 0x31217B;
    outline_night = 0x10104A;
    /// Noon shadow tint.
    shadow_day = 0x4A3AAD;
    shadow_night = 0x21197B;
    smoke = 0xE6DEF7;
    smoke_shade = 0xA59CDE;
    smoke_night = 0x5A4A9C;
    rain = 0xCEF7FF;
    firefly = 0xDEFF5A;
    gull = 0xF7EFDE;
    gull_tip = 0x31217B;
    crab = 0xFF5A31;
    fish = 0x9CEFFF;
    moon = 0xDEEFFF;
    moon_mark = 0xA5BDF7;
    star = 0xF7EFDE;
    star_pink = 0xFFADD6;
    star_cyan = 0x9CDEFF;
}

/// Daytime shadow tint (alias of [`shadow_day`]).
pub fn shadow() -> Vec3 {
    shadow_day()
}

// ---- greywild leftovers (DESIGN A1 mapping) ----------------------------------------------

/// Generic dressed stone → `dry_stone`.
pub fn stone() -> Vec3 {
    dry_stone()
}
/// Boulder footing → `rock`.
pub fn foundation() -> Vec3 {
    rock()
}
/// Timber posts, beams, piles → `timber`.
pub fn wood() -> Vec3 {
    timber()
}
/// Dune grass → `marram`.
pub fn grass_dry() -> Vec3 {
    marram()
}
/// Broadleaf foliage → `leaf_fig`.
pub fn leaves() -> Vec3 {
    leaf_fig()
}
/// Needles → `leaf_sea_pine`.
pub fn pine() -> Vec3 {
    leaf_sea_pine()
}
/// Straw bundles → `reed`.
pub fn hay() -> Vec3 {
    reed()
}
/// Iron → `iron`.
pub fn metal() -> Vec3 {
    iron()
}
/// Lantern glass and flame core → `glow_core`.
pub fn glow() -> Vec3 {
    glow_core()
}
/// Painted / bleached white → `salt_bleach` (white is reserved for foam).
pub fn white() -> Vec3 {
    salt_bleach()
}
/// Flower colours: sea pink, gorse, flax, deep sea pink.
pub fn flower(i: u32) -> Vec3 {
    [petal_sea_pink, petal_gorse, petal_flax, petal_sea_pink_deep][(i % 4) as usize]()
}

// ---- cel band function (DESIGN A3), CPU port of glade.wesl `shade_band` -----------------

/// Light band of a cel-shaded pixel.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Band {
    Lit,
    Mid,
    Shadow,
}

/// Lit / mid / shadow thresholds on `light = max(N·L, 0) × shadowmap`.
pub fn band_of(light: f32) -> Band {
    if light >= 0.30 {
        Band::Lit
    } else if light >= 0.08 {
        Band::Mid
    } else {
        Band::Shadow
    }
}

/// sRGB (0..1) → HSV (hue in degrees 0..360).
pub fn rgb_to_hsv(c: Vec3) -> Vec3 {
    let mx = c.max_element();
    let mn = c.min_element();
    let d = mx - mn;
    let h = if d <= 1e-6 {
        0.0
    } else if mx == c.x {
        60.0 * ((c.y - c.z) / d).rem_euclid(6.0)
    } else if mx == c.y {
        60.0 * ((c.z - c.x) / d + 2.0)
    } else {
        60.0 * ((c.x - c.y) / d + 4.0)
    };
    Vec3::new(h, if mx <= 1e-6 { 0.0 } else { d / mx }, mx)
}

/// HSV (hue degrees) → sRGB 0..1.
pub fn hsv_to_rgb(hsv: Vec3) -> Vec3 {
    let (h, s, v) = (hsv.x.rem_euclid(360.0), hsv.y.clamp(0.0, 1.0), hsv.z.max(0.0));
    let c = v * s;
    let x = c * (1.0 - ((h / 60.0).rem_euclid(2.0) - 1.0).abs());
    let m = v - c;
    let (r, g, b) = match (h / 60.0) as u32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    Vec3::new(r + m, g + m, b + m)
}

/// Rotates hue `h` toward `t` by up to `amount` degrees along the DESIGN A3 direction rule:
/// positive when `60 ≤ h < t` (greens/cyans through teal and blue), else negative (the warm
/// path through orange, red, magenta). Never rotates past `t`. Returns (new hue, warm path).
pub fn rotate_toward(h: f32, t: f32, amount: f32) -> (f32, bool) {
    if (60.0..t).contains(&h) {
        (h + amount.min(t - h), false)
    } else {
        let dist = (h - t).rem_euclid(360.0);
        ((h - amount.min(dist)).rem_euclid(360.0), true)
    }
}

/// Hue rotation of the lit band toward `tint` by `deg` (dusk 10°, night 30°).
pub fn rot(c: Vec3, tint: Vec3, deg: f32) -> Vec3 {
    if deg <= 0.0 {
        return c;
    }
    let (mut a, t) = (rgb_to_hsv(c), rgb_to_hsv(tint).x);
    if a.y < 0.15 {
        return c;
    }
    a.x = rotate_toward(a.x, t, deg).0;
    hsv_to_rgb(a)
}

/// DESIGN A3 `shade(c, band, tint)` in sRGB 0..1: hue-shifted, saturated mid / shadow
/// colour of albedo `c` toward the shadow `tint`. `Lit` returns `c` unchanged.
pub fn shade(c: Vec3, band: Band, tint: Vec3) -> Vec3 {
    let (rot_cool, rot_warm, sat_k, val_k, mix_k, floor, floor_white) = match band {
        Band::Lit => return c,
        Band::Mid => (20.0, 14.0, 1.0, 0.90, 0.08, 0.0, 0.25),
        Band::Shadow => (36.0, 25.0, 1.2, 0.78, 0.15, 0.40, 0.30),
    };
    let t = rgb_to_hsv(tint).x;
    let mut a = rgb_to_hsv(c);
    let white = a.y < 0.15;
    if white {
        a.x = t;
        a.y = a.y.max(floor_white);
    } else {
        let warm = !(60.0..t).contains(&a.x);
        a.x = rotate_toward(a.x, t, if warm { rot_warm } else { rot_cool }).0;
    }
    a.y = (a.y * sat_k).min(1.0);
    a.z *= val_k;
    let mixed = hsv_to_rgb(a).lerp(tint, mix_k);
    let mut m = rgb_to_hsv(mixed);
    let f = if white { floor_white } else { floor };
    if m.y < f {
        m.y = f;
        return hsv_to_rgb(m);
    }
    mixed
}

/// DESIGN A3 anti-mud floor (after lighting): low-chroma darks are pulled toward the tint
/// hue and given S = 0.35; nothing drops below V = 0.22.
pub fn anti_mud(c: Vec3, tint: Vec3) -> Vec3 {
    let mut a = rgb_to_hsv(c);
    if a.z < 0.80 && a.y < 0.35 {
        let t = rgb_to_hsv(tint).x;
        let k = (0.35 - a.y) / 0.35;
        let dist = if (60.0..t).contains(&a.x) { t - a.x } else { (a.x - t).rem_euclid(360.0) };
        a.x = rotate_toward(a.x, t, dist * k).0;
        a.y = 0.35;
    }
    a.z = a.z.max(0.22);
    hsv_to_rgb(a)
}

/// The post pass's 15-bit quantise (5 bits per channel, no dither) of an sRGB colour.
pub fn quantise15(c: Vec3) -> Vec3 {
    (c.clamp(Vec3::ZERO, Vec3::ONE) * 31.0).round() / 31.0
}

/// Full noon-clear band colour of albedo `hex`, quantised to 15 bits, as a hex. Mirrors the
/// shader at noon (sun = fill = white × 1.0, tint = `shadow_day`).
pub fn noon_band(hex: u32, band: Band) -> u32 {
    let tint = hex_to_unit(0x4A3AAD);
    let c = shade(hex_to_unit(hex), band, tint);
    let c = if band == Band::Lit { c } else { anti_mud(c, tint) };
    unit_to_hex(quantise15(c))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: u32, b: u32, lsb: i32) -> bool {
        (0..3).all(|k| {
            let (x, y) = (((a >> (k * 8)) & 0xff) as i32, ((b >> (k * 8)) & 0xff) as i32);
            // One 5-bit LSB is ~8.2 byte steps.
            (x - y).abs() <= lsb * 9
        })
    }

    /// DESIGN A0: every token hex uses only 5-bit-exact channel bytes.
    #[test]
    fn tokens_are_5bit_exact() {
        let ok = |b: u32| (0..32).any(|k: u32| (k as f32 * 255.0 / 31.0).round() as u32 == b);
        let all = TOKENS.iter().chain(&ROOF_SWATCHES).chain(&WALL_SWATCHES).chain(&TRIM_SWATCHES);
        for (name, hex) in all {
            for k in 0..3 {
                assert!(ok((hex >> (k * 8)) & 0xff), "{name} #{hex:06X} not 5-bit exact");
            }
        }
    }

    /// DESIGN A0 rule 2: pure white is reserved for foam.
    #[test]
    fn white_is_reserved() {
        for (name, hex) in TOKENS.iter().chain(&WALL_SWATCHES) {
            assert!(*hex != 0xFFFFFF || *name == "foam", "{name} uses reserved white");
        }
    }

    /// Vibrant: every swatch is saturated (no greys), except Chalk Lime.
    #[test]
    fn swatches_are_vibrant() {
        for (name, hex) in ROOF_SWATCHES.iter().chain(&WALL_SWATCHES).chain(&TRIM_SWATCHES) {
            let c = hex_to_unit(*hex);
            let hsv = rgb_to_hsv(c);
            assert!(hsv.z > 0.5, "{name} too dark");
            assert!(*name == "CHALK LIME" || hsv.y > 0.4, "{name} not saturated");
        }
    }

    #[test]
    fn hsv_round_trips() {
        for (_, hex) in TOKENS {
            let c = hex_to_unit(*hex);
            assert!((hsv_to_rgb(rgb_to_hsv(c)) - c).abs().max_element() < 1e-4);
        }
    }

    /// DESIGN A3 calibration goldens (noon clear, after the 15-bit quantise, ±1 LSB).
    #[test]
    fn shade_matches_calibration_goldens() {
        const GOLDENS: [(&str, u32, u32, u32, u32); 12] = [
            ("grass", 0x6BD642, 0x6BD642, 0x3AB54A, 0x21945A),
            ("sand", 0xF7D68C, 0xF7D68C, 0xCEA584, 0xAD6B6B),
            ("wet_sand", 0xDEA54A, 0xDEA54A, 0xBD734A, 0x9C3A3A),
            ("thatch", 0xEFB53A, 0xEFB53A, 0xCE7B3A, 0xAD3A29),
            ("dry_stone", 0xE6C594, 0xE6C594, 0xC58484, 0xA56B73),
            ("driftwood", 0xEFD6B5, 0xEFD6B5, 0xCE84B5, 0xAD6B7B),
            ("turf", 0x5ADE4A, 0x5ADE4A, 0x42BD63, 0x299C73),
            ("leaf_palm", 0x8CDE3A, 0x8CDE3A, 0x4ABD3A, 0x199C3A),
            ("leaf_cypress", 0x108C63, 0x108C63, 0x107B84, 0x084A73),
            ("boards", 0xDE9C5A, 0xDE9C5A, 0xBD6B5A, 0x9C3A42),
            ("flagstone", 0x5A73CE, 0x5A73CE, 0x5A52B5, 0x4231A5),
            ("iron", 0x4A3A73, 0x4A3A73, 0x3A316B, 0x312963),
        ];
        let mut bad = Vec::new();
        for (name, albedo, lit, mid, shadow) in GOLDENS {
            for (band, want) in [(Band::Lit, lit), (Band::Mid, mid), (Band::Shadow, shadow)] {
                let got = noon_band(albedo, band);
                if !close(got, want, 1) {
                    bad.push(format!("{name} {band:?}: got #{got:06X} want #{want:06X}"));
                }
            }
            // The token fn and the golden albedo agree.
            let tok = TOKENS.iter().find(|t| t.0 == name).expect("golden names a token");
            assert_eq!(tok.1, albedo, "{name}");
        }
        assert!(bad.is_empty(), "{bad:#?}");
    }

    /// DESIGN A1: the `*_SHADES` tables are what `shade()` produces for each swatch.
    #[test]
    fn swatch_shades_match_shade_fn() {
        let tables: [(&[(&str, u32)], &[(u32, u32)]); 3] =
            [(&ROOF_SWATCHES, &ROOF_SHADES), (&WALL_SWATCHES, &WALL_SHADES), (&TRIM_SWATCHES, &TRIM_SHADES)];
        let mut bad = Vec::new();
        for (sw, sh) in tables {
            assert_eq!(sw.len(), sh.len());
            for ((name, hex), (mid, shadow)) in sw.iter().zip(sh) {
                for (band, want) in [(Band::Mid, *mid), (Band::Shadow, *shadow)] {
                    let got = noon_band(*hex, band);
                    if !close(got, want, 1) {
                        bad.push(format!("{name} {band:?}: got #{got:06X} want #{want:06X}"));
                    }
                }
            }
        }
        assert!(bad.is_empty(), "{bad:#?}");
    }

    /// Shadows are never grey or black: every shaded token keeps chroma and value.
    #[test]
    fn shadows_never_grey() {
        let tint = hex_to_unit(0x4A3AAD);
        for (name, hex) in TOKENS {
            let s = anti_mud(shade(hex_to_unit(*hex), Band::Shadow, tint), tint);
            let hsv = rgb_to_hsv(s);
            assert!(hsv.y >= 0.299 && hsv.z >= 0.219, "{name} shadow muddy: {hsv:?}");
        }
    }

    #[test]
    fn direction_rule() {
        // Grass (green) goes positive toward blue, thatch (orange) goes the warm way.
        assert_eq!(rotate_toward(100.0, 248.0, 36.0), (136.0, false));
        assert_eq!(rotate_toward(40.0, 248.0, 25.0), (15.0, true));
        // Never past the tint.
        assert_eq!(rotate_toward(240.0, 248.0, 36.0), (248.0, false));
        assert_eq!(rotate_toward(250.0, 248.0, 25.0), (248.0, true));
        assert_eq!(band_of(0.5), Band::Lit);
        assert_eq!(band_of(0.1), Band::Mid);
        assert_eq!(band_of(0.0), Band::Shadow);
    }

    #[test]
    fn leftovers_map_to_tokens() {
        assert_eq!(stone(), dry_stone());
        assert_eq!(foundation(), rock());
        assert_eq!(wood(), timber());
        assert_eq!(metal(), iron());
        assert_eq!(white(), salt_bleach());
        assert_eq!(flower(1), petal_gorse());
        assert_eq!(roof(Colors::default()), thatch());
    }
}
