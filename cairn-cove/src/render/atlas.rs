//! Procedural texture atlas (DESIGN.md A2): one 32×32 tile per [`Surface`], 8 tiles per row.
//!
//! The atlas is **tint-space**: most texels are a lavender-biased near-white ramp that the
//! shader multiplies by the vertex colour (the token or swatch), so recesses shade cool and
//! never grey: `M0 #FFFFFF` face · `M1 #E6E6F7` soft detail · `M2 #C5C5EF` joint/seam ·
//! `M3 #9C9CDE` deep groove.
//!
//! Texel alpha classes (read by `glade.wesl`):
//! - `1.0` emissive, untinted (flame, lantern glass, night window slot);
//! - `0.85` hot accent (`iron_hot` basket rim: glows at night, plain iron by day);
//! - `0.75` accent: absolute colour (a palette token), lit and shaded but not tinted. RGB
//!   `#000000` is the **trim sentinel**: the shader paints the trim pigment there;
//! - `0.5` regular tinted ramp;
//! - `0.0` cutout (discarded).
//!
//! Tile `i` = `Surface as u8`. Sampled with `textureLoad` (nearest). Every tile keeps to a
//! handful of distinct texel values so it survives affine warping at 240p.

use bevy::{
    asset::RenderAssetUsages,
    image::{Image, ImageSampler},
    render::render_resource::{Extent3d, TextureDimension, TextureFormat},
};
use cairn_cove_core::Surface;
use std::f32::consts::TAU;

pub const TILE: u32 = 32;
pub const COLS: u32 = 8;
pub const ROWS: u32 = (Surface::COUNT as u32).div_ceil(COLS);

const ALL: [Surface; Surface::COUNT] = [
    Surface::Grass,
    Surface::Dirt,
    Surface::Rock,
    Surface::Sand,
    Surface::DryStone,
    Surface::Daub,
    Surface::Plank,
    Surface::Timber,
    Surface::Thatch,
    Surface::Turf,
    Surface::Hide,
    Surface::Carving,
    Surface::Shell,
    Surface::Pebble,
    Surface::Flagstone,
    Surface::Boards,
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

/// One texel: sRGB rgb in 0..1 + alpha class.
pub type Tx = [f32; 4];

fn unit(hex: u32, a: f32) -> Tx {
    let c = |s: u32| ((hex >> s) & 0xff) as f32 / 255.0;
    [c(16), c(8), c(0), a]
}
/// Tinted ramp texel.
fn ramp(hex: u32) -> Tx {
    unit(hex, 0.5)
}
/// Accent texel: absolute token colour.
fn accent(hex: u32) -> Tx {
    unit(hex, 0.75)
}
/// Hot accent texel (emissive at night).
fn hot(hex: u32) -> Tx {
    unit(hex, 0.85)
}
/// Emissive, untinted texel.
fn emit(hex: u32) -> Tx {
    unit(hex, 1.0)
}

const TRIM: Tx = [0.0, 0.0, 0.0, 0.75];
const CUT: Tx = [0.0, 0.0, 0.0, 0.0];

// Ramp (DESIGN A2).
const M0: u32 = 0xFFFFFF;
const M1: u32 = 0xE6E6F7;
const M2: u32 = 0xC5C5EF;
const M3: u32 = 0x9C9CDE;

// Accent tokens (DESIGN A1; equal to the `palette` fns of the same name).
const GRASS: u32 = 0x6BD642;
const GRASS_TIP: u32 = 0xADEF4A;
const WET_SAND: u32 = 0xDEA54A;
const CLIFF_BAND: u32 = 0xEFA55A;
const DRY_STONE_ALT: u32 = 0xBDA5EF;
const STONE_JOINT: u32 = 0x84529C;
const SHELL_NACRE: u32 = 0xFFADA5;
const PEBBLE_LILAC: u32 = 0xA59CEF;
const PEBBLE_ROSE: u32 = 0xEFB59C;
const FLAGSTONE_EDGE: u32 = 0x7B8CEF;
const SALT_BLEACH: u32 = 0xF7EFD6;
const SOD: u32 = 0x9C4A29;
const HIDE_SEAM: u32 = 0xDE8C4A;
const BARK: u32 = 0x8C4231;
const BARK_HI: u32 = 0xB55A42;
const BOARDS_HI: u32 = 0xEFBD7B;
const BOARD_GAP: u32 = 0x7B3129;
const ROPE: u32 = 0xEFC56B;
const FIBER_DARK: u32 = 0xB58C29;
const IRON_HI: u32 = 0x6B5AA5;
const IRON_HOT: u32 = 0xFF6B29;
const PETAL_SEA_PINK: u32 = 0xFF5AB5;
const FLAME_WHITE: u32 = 0xFFF7AD;
const GLOW_CORE: u32 = 0xFFE65A;
const GLOW_FIRE: u32 = 0xFF7B19;
const GLOW_EMBER: u32 = 0xFF3A31;
const WINDOW_LIGHT: u32 = 0xFFB542;
const OUTLINE_DAY: u32 = 0x31217B;

fn hash(x: i32, y: i32, seed: u32) -> f32 {
    let mut h = (x as u32).wrapping_mul(0x27d4_eb2d) ^ (y as u32).wrapping_mul(0x1656_67b1) ^ seed.wrapping_mul(0x9e37_79b9);
    h ^= h >> 15;
    h = h.wrapping_mul(0x85eb_ca6b);
    h ^= h >> 13;
    h = h.wrapping_mul(0xc2b2_ae35);
    h ^= h >> 16;
    (h & 0xffff) as f32 / 65536.0
}

/// Hashed integer in `0..n`.
fn hi(i: i32, k: i32, seed: u32, n: i32) -> i32 {
    (hash(i, k, seed) * n as f32) as i32
}

/// Wrapped (toroidal) distance between texel coordinates.
fn wrap_d(a: f32, b: f32) -> f32 {
    let d = (a - b).rem_euclid(TILE as f32);
    d.min(TILE as f32 - d)
}

/// Tileable Voronoi with `cells` jittered sites per side: (nearest, second nearest, cell id).
fn voronoi(x: i32, y: i32, cells: i32, seed: u32) -> (f32, f32, f32) {
    let size = TILE as f32 / cells as f32;
    let (px, py) = (x as f32 + 0.5, y as f32 + 0.5);
    let (mut d1, mut d2, mut id) = (1e9f32, 1e9f32, 0.0);
    for j in 0..cells {
        for i in 0..cells {
            let ox = (i as f32 + 0.2 + 0.6 * hash(i, j, seed)) * size;
            let oy = (j as f32 + 0.2 + 0.6 * hash(i, j, seed + 7)) * size;
            let d = wrap_d(ox, px).hypot(wrap_d(oy, py));
            if d < d1 {
                d2 = d1;
                d1 = d;
                id = hash(i, j, seed + 13);
            } else if d < d2 {
                d2 = d;
            }
        }
    }
    (d1, d2, id)
}

/// A 32×32 tile with wrapping access.
#[derive(Clone)]
pub struct Tile(pub Vec<Tx>);

impl Tile {
    fn fill(v: Tx) -> Self {
        Self(vec![v; (TILE * TILE) as usize])
    }
    fn idx(x: i32, y: i32) -> usize {
        (y.rem_euclid(TILE as i32) * TILE as i32 + x.rem_euclid(TILE as i32)) as usize
    }
    pub fn get(&self, x: i32, y: i32) -> Tx {
        self.0[Self::idx(x, y)]
    }
    fn set(&mut self, x: i32, y: i32, v: Tx) {
        self.0[Self::idx(x, y)] = v;
    }
    fn rect(&mut self, x: i32, y: i32, w: i32, h: i32, v: Tx) {
        for j in 0..h {
            for i in 0..w {
                self.set(x + i, y + j, v);
            }
        }
    }
    /// Fills every texel from `f(x, y)`.
    fn from_fn(f: impl Fn(i32, i32) -> Tx) -> Self {
        let mut t = Self::fill(CUT);
        for y in 0..TILE as i32 {
            for x in 0..TILE as i32 {
                t.set(x, y, f(x, y));
            }
        }
        t
    }
}

/// 7×7 square spiral (painted motif on hides).
const SPIRAL7: [&str; 7] = ["#######", "......#", ".####.#", ".#..#.#", ".#.##.#", ".#....#", ".######"];

/// Random walk of `len` steps from (x, y) with a drift `(dx, dy)` per step.
fn walk(t: &mut Tile, mut x: i32, mut y: i32, len: i32, (dx, dy): (i32, i32), seed: u32, v: Tx) {
    for k in 0..len {
        t.set(x, y, v);
        match hi(k, 0, seed, 3) {
            0 => x += dx,
            1 => y += dy,
            _ => {
                x += dx;
                y += dy;
            }
        }
    }
}

/// Builds the tile of surface `s`. Pure and deterministic.
pub fn tile(s: Surface) -> Tile {
    match s {
        Surface::Grass => {
            // Hashed 1×3 blade ticks (M1) and 2×2 clumps (M2) on M0, plus grass_tip flecks.
            let mut t = Tile::fill(ramp(M0));
            for i in 0..34 {
                t.rect(hi(i, 1, 101, 32), hi(i, 2, 101, 32), 1, 3, ramp(M1));
            }
            for i in 0..9 {
                t.rect(hi(i, 1, 102, 32), hi(i, 2, 102, 32), 2, 2, ramp(M2));
            }
            for i in 0..7 {
                t.set(hi(i, 1, 103, 32), hi(i, 2, 103, 32), accent(GRASS_TIP));
            }
            t
        }
        Surface::Dirt => {
            // M0 field, 2×2 M1 pebble dots, one meandering M3 crack, 3 pebble_rose dots.
            let mut t = Tile::fill(ramp(M0));
            for i in 0..10 {
                t.rect(hi(i, 1, 111, 32), hi(i, 2, 111, 32), 2, 2, ramp(M1));
            }
            walk(&mut t, 4, 6, 22, (1, 1), 112, ramp(M3));
            for i in 0..3 {
                t.rect(hi(i, 1, 113, 32), hi(i, 2, 113, 32), 2, 2, accent(PEBBLE_ROSE));
            }
            t
        }
        Surface::Rock => {
            // Big angular facets (M0 / M1), 1-px M3 cracks, a cliff_band sun-side edge.
            let crack = |x: i32, y: i32| {
                let (d1, d2, _) = voronoi(x, y, 2, 121);
                d2 - d1 < 1.1
            };
            Tile::from_fn(|x, y| {
                let (_, _, id) = voronoi(x, y, 2, 121);
                if crack(x, y) {
                    ramp(M3)
                } else if crack(x, y - 1) {
                    accent(CLIFF_BAND)
                } else if id < 0.5 {
                    ramp(M0)
                } else {
                    ramp(M1)
                }
            })
        }
        Surface::Sand => {
            // Sparse M1 grains, M2 ripple arcs every ~6 px on a diagonal, 2 shell_nacre specks.
            let mut t = Tile::from_fn(|x, y| {
                let w = (1.6 * (x as f32 * TAU / 32.0).sin()).round() as i32;
                let d = (x + y + w).rem_euclid(32);
                let arc = [0, 6, 13, 19, 26].contains(&d) && hash(x / 4, d, 132) > 0.3;
                if arc {
                    ramp(M2)
                } else if hash(x, y, 131) > 0.93 {
                    ramp(M1)
                } else {
                    ramp(M0)
                }
            });
            for i in 0..2 {
                t.set(hi(i, 1, 133, 32), hi(i, 2, 133, 32), accent(SHELL_NACRE));
            }
            t
        }
        Surface::DryStone => {
            // Flat stones 8–12 × 4–5 px in staggered courses: M0 faces with an M1 top edge,
            // 1-px stone_joint joints, every third stone lilac slate (dry_stone_alt).
            const HEIGHTS: [i32; 6] = [4, 4, 5, 4, 5, 4];
            const LENS: [[i32; 3]; 6] = [[10, 11, 11], [12, 10, 10], [9, 11, 12], [11, 12, 9], [10, 10, 12], [12, 9, 11]];
            let mut t = Tile::fill(accent(STONE_JOINT));
            let (mut y0, mut n) = (0, 0);
            for (c, h) in HEIGHTS.iter().enumerate() {
                let mut x0 = hi(c as i32, 0, 141, 32);
                for len in LENS[c] {
                    let alt = n % 3 == 2;
                    for dx in 0..len - 1 {
                        for dy in 0..*h {
                            let v = if alt {
                                accent(DRY_STONE_ALT)
                            } else if dy == 0 {
                                ramp(M1)
                            } else {
                                ramp(M0)
                            };
                            t.set(x0 + dx, y0 + dy, v);
                        }
                    }
                    x0 += len;
                    n += 1;
                }
                y0 += h + 1;
            }
            t
        }
        Surface::Daub => {
            // Limewash in horizontal brush strokes (M0 / M1), wattle hints (M2), one hairline.
            let mut t = Tile::from_fn(|x, y| {
                let stroke = hash(((x + hi(y, 0, 151, 32)) / 7).rem_euclid(5), y, 152) > 0.55;
                ramp(if stroke { M1 } else { M0 })
            });
            for i in 0..4 {
                t.set(hi(i, 1, 153, 32), hi(i, 2, 153, 32), ramp(M2));
            }
            walk(&mut t, 20, 3, 9, (1, 1), 154, ramp(M3));
            t
        }
        Surface::Plank => {
            // Vertical planks ~6 px wide, 1-px board_gap gaps, M1 grain ticks and a painted
            // trim band (rows 12–17) with a 1-texel zigzag top edge.
            const WIDTHS: [i32; 5] = [6, 6, 7, 6, 7];
            let mut t = Tile::fill(ramp(M0));
            for i in 0..14 {
                t.rect(hi(i, 1, 161, 32), hi(i, 2, 161, 32), 1, 2 + hi(i, 3, 161, 2), ramp(M1));
            }
            for x in 0..32 {
                for y in 12..=17 {
                    if y >= 12 + (x / 2) % 2 {
                        t.set(x, y, TRIM);
                    }
                }
            }
            let mut x = 0;
            for w in WIDTHS {
                x += w;
                for y in 0..32 {
                    t.set(x - 1, y, accent(BOARD_GAP));
                }
            }
            t
        }
        Surface::Timber => {
            // Long vertical grain streaks (M0 / M1 / M2) with a 3-px rope lashing ring.
            Tile::from_fn(|x, y| {
                if (4..7).contains(&y) {
                    return accent(ROPE);
                }
                let k = hash(x, (y + hi(x, 0, 171, 32)).div_euclid(9), 172);
                ramp(if k < 0.55 { M0 } else if k < 0.85 { M1 } else { M2 })
            })
        }
        Surface::Thatch => {
            // Staggered 4-px straw tufts as downward chevrons: M2 root row, M1 body, M0 tips;
            // every 8th row a bound course in M3.
            Tile::from_fn(|x, y| {
                if y % 8 == 0 {
                    return ramp(M3);
                }
                let (r, v) = (y / 4, y % 4);
                let u = (x + (r % 2) * 2).rem_euclid(4);
                ramp(match (v, u) {
                    (0, _) => M2,
                    (1, _) => M1,
                    (2, 0) => M2,
                    (2, _) => M1,
                    (_, 1 | 2) => M0,
                    _ => M2,
                })
            })
        }
        Surface::Turf => {
            // Grass fringe (M0 with M1 tufts) with a ragged lower edge over a sod band with
            // M2 earth lines; two sea-pink dots.
            let mut t = Tile::from_fn(|x, y| {
                let edge = 25 + hi(x / 2, 0, 181, 3);
                if y >= edge {
                    if y == 29 && hash(x / 3, 0, 182) > 0.3 { ramp(M2) } else { accent(SOD) }
                } else if hash(x, y / 2, 183) > 0.82 {
                    ramp(M1)
                } else {
                    ramp(M0)
                }
            });
            for i in 0..2 {
                t.set(hi(i, 1, 184, 32), hi(i, 2, 184, 20), accent(PETAL_SEA_PINK));
            }
            t
        }
        Surface::Hide => {
            // Smooth M0 hide, a soft M1 fold, a dashed hide_seam stitch and a painted trim
            // spiral (7×7).
            let mut t = Tile::from_fn(|x, y| {
                let fold = (x * 2 + y).rem_euclid(32) < 4;
                ramp(if fold { M1 } else { M0 })
            });
            for x in 0..32 {
                if x % 4 != 3 {
                    t.set(x, 23, accent(HIDE_SEAM));
                }
            }
            for (j, row) in SPIRAL7.iter().enumerate() {
                for (i, c) in row.bytes().enumerate() {
                    if c == b'#' {
                        t.set(18 + i as i32, 6 + j as i32, TRIM);
                    }
                }
            }
            t
        }
        Surface::Carving => {
            // Deep-cut spirals (top half) over a zigzag frieze (bottom half): raised M0, M1
            // bevels, bark recesses, a 1-px salt_bleach highlight on top edges.
            let raised = |x: i32, y: i32| {
                let (x, y) = (x.rem_euclid(32), y.rem_euclid(32));
                if y == 15 || y == 31 {
                    return false;
                }
                if y < 15 {
                    let (dx, dy) = ((x % 16) as f32 - 7.5, y as f32 - 7.0);
                    let r = dx.hypot(dy);
                    if r > 6.8 {
                        return true;
                    }
                    let a = r - (dy.atan2(dx) / TAU + 0.5) * 4.0;
                    return a.rem_euclid(4.0) < 2.0;
                }
                let tri = ((x % 8) - 4).abs();
                (y - 16 + tri).rem_euclid(6) >= 2
            };
            Tile::from_fn(|x, y| {
                if !raised(x, y) {
                    accent(BARK)
                } else if !raised(x, y - 1) {
                    accent(SALT_BLEACH)
                } else if !raised(x - 1, y) || !raised(x, y + 1) {
                    ramp(M1)
                } else {
                    ramp(M0)
                }
            })
        }
        Surface::Shell => {
            // Irregular 2–3 px chips (M0, 1 in 5 shell_nacre) on M1 with pebble_lilac shadows.
            let mut t = Tile::fill(ramp(M1));
            for j in 0..8 {
                for i in 0..8 {
                    let (x, y) = (i * 4 + hi(i, j, 191, 3), j * 4 + hi(i, j, 192, 3));
                    let (w, h) = (2 + hi(i, j, 193, 2), 2);
                    t.rect(x + 1, y + h, w, 1, accent(PEBBLE_LILAC));
                    let v = if hash(i, j, 194) < 0.2 { accent(SHELL_NACRE) } else { ramp(M0) };
                    t.rect(x, y, w, h, v);
                }
            }
            t
        }
        Surface::Pebble => {
            // Packed 5–7 px rounds: M1 body with an M0 top-left highlight, half pebble_rose,
            // wet_sand joints.
            let joint = |x: i32, y: i32| {
                let (d1, d2, _) = voronoi(x, y, 5, 201);
                d2 - d1 < 1.1 || d1 > 3.4
            };
            Tile::from_fn(|x, y| {
                let (_, _, id) = voronoi(x, y, 5, 201);
                if joint(x, y) {
                    accent(WET_SAND)
                } else if id < 0.5 {
                    accent(PEBBLE_ROSE)
                } else if joint(x - 1, y - 1) {
                    ramp(M0)
                } else {
                    ramp(M1)
                }
            })
        }
        Surface::Flagstone => {
            // 4 irregular slabs (M0 / M1), a flagstone_edge sun edge, 1–2 px grass joints.
            let joint = |x: i32, y: i32| {
                let (d1, d2, _) = voronoi(x, y, 2, 211);
                d2 - d1 < 1.7
            };
            Tile::from_fn(|x, y| {
                let (_, _, id) = voronoi(x, y, 2, 211);
                if joint(x, y) {
                    accent(GRASS)
                } else if joint(x, y - 1) {
                    accent(FLAGSTONE_EDGE)
                } else {
                    ramp(if id < 0.5 { M0 } else { M1 })
                }
            })
        }
        Surface::Boards => {
            // Horizontal 8-px deck planks: boards_hi bleached top edge, board_gap gaps and
            // butt joints, 2 M3 nail dots per plank.
            Tile::from_fn(|x, y| {
                let (row, v) = (y / 8, y % 8);
                let butt = hi(row, 0, 221, 32);
                if v == 7 || x == butt {
                    accent(BOARD_GAP)
                } else if v == 0 {
                    accent(BOARDS_HI)
                } else if x == (butt + 2) % 32 && (v == 2 || v == 5) {
                    ramp(M3)
                } else {
                    ramp(M0)
                }
            })
        }
        Surface::Window => {
            // 2-px outline_day recess frame, two slatted shutters (M0 slats, M2 gaps) with a
            // chevron cut at the top, a 6×10 centre slot: emissive window_light (the shader
            // shows it as outline ink by day).
            Tile::from_fn(|x, y| {
                let frame = !(2..30).contains(&x) || !(2..30).contains(&y);
                if frame {
                    return accent(OUTLINE_DAY);
                }
                if (13..19).contains(&x) {
                    return if (11..21).contains(&y) { emit(WINDOW_LIGHT) } else { accent(OUTLINE_DAY) };
                }
                let cx = if x < 13 { 7 } else { 24 };
                let notch = (3 - (x - cx).abs()).max(0);
                if y < 2 + notch {
                    accent(OUTLINE_DAY)
                } else {
                    ramp(if y % 3 == 0 { M2 } else { M0 })
                }
            })
        }
        Surface::Door => {
            // Hide curtain in 3 folds (M0 / M1 / M2) in a 2-px outline recess with a dot-row
            // hem in trim colour.
            Tile::from_fn(|x, y| {
                if !(2..30).contains(&x) || y < 2 {
                    return accent(OUTLINE_DAY);
                }
                if y == 27 && x % 3 == 1 {
                    return TRIM;
                }
                let k = ((x - 2) * 9 / 28) % 3;
                ramp([M0, M1, M2][k as usize])
            })
        }
        Surface::Bark => {
            // Vertical fissured plates: M0 plates, bark_hi ridges, board_gap fissures.
            const EDGES: [i32; 6] = [0, 6, 11, 17, 23, 28];
            Tile::from_fn(|x, y| {
                for (k, e) in EDGES.iter().enumerate() {
                    let w = (1.2 * (y as f32 * TAU / 16.0 + k as f32 * 1.7).sin()).round() as i32;
                    let d = (x - e - w).rem_euclid(32);
                    if d == 0 {
                        return accent(BOARD_GAP);
                    }
                    if d == 1 {
                        return accent(BARK_HI);
                    }
                }
                let plate = EDGES.iter().rposition(|e| *e <= x).unwrap_or(0) as i32;
                if (y + hi(plate, 0, 231, 32)).rem_euclid(11) == 0 {
                    accent(BOARD_GAP)
                } else {
                    ramp(M0)
                }
            })
        }
        Surface::Leaves => {
            // Scalloped clumps: M0 lit upper edge, M1 body, M2 under-notch; ~25 % cutout.
            let mut blobs: Vec<(f32, f32)> = (0..36)
                .map(|k| {
                    let (i, j) = (k % 6, k / 6);
                    let size = 32.0 / 6.0;
                    ((i as f32 + 0.2 + 0.6 * hash(i, j, 241)) * size, (j as f32 + 0.2 + 0.6 * hash(i, j, 242)) * size)
                })
                .collect();
            blobs.sort_by(|a, b| a.1.total_cmp(&b.1));
            const R: f32 = 2.85;
            Tile::from_fn(|x, y| {
                let (px, py) = (x as f32 + 0.5, y as f32 + 0.5);
                // The lowest clump covering the texel is in front.
                let hit = blobs.iter().rev().find(|(cx, cy)| wrap_d(*cx, px).hypot(wrap_d(*cy, py)) <= R);
                let Some((_, cy)) = hit else { return CUT };
                let mut dy = py - cy;
                if dy > 16.0 {
                    dy -= 32.0;
                } else if dy < -16.0 {
                    dy += 32.0;
                }
                ramp(if dy < -R + 1.6 { M0 } else if dy > R - 1.3 { M2 } else { M1 })
            })
        }
        Surface::Fiber => {
            // 2-px diagonal rope twist (M0 / fiber_dark) around the tile seam (small ropes sample
            // there), a diamond net with M3 knots on cutout ground in the middle.
            Tile::from_fn(|x, y| {
                if !(8..24).contains(&y) {
                    return if ((x + y) / 2) % 2 == 0 { ramp(M0) } else { accent(FIBER_DARK) };
                }
                let (a, b) = ((x + y).rem_euclid(8) == 0, (x - y).rem_euclid(8) == 0);
                match (a, b) {
                    (true, true) => ramp(M3),
                    (true, false) | (false, true) => ramp(M0),
                    _ => CUT,
                }
            })
        }
        Surface::Petal => {
            // Five-dot 3-px rosettes (M0 petals, M2 centres) on M1. Petal geometry is tiny
            // (flowers, fruit, lichen), so the ground stays solid instead of cutout.
            let mut t = Tile::fill(ramp(M1));
            for j in 0..4 {
                for i in 0..4 {
                    let (x, y) = (i * 8 + 1 + hi(i, j, 251, 5), j * 8 + 1 + hi(i, j, 252, 5));
                    for (dx, dy) in [(0, 1), (2, 1), (1, 0), (1, 2)] {
                        t.set(x + dx, y + dy, ramp(M0));
                    }
                    t.set(x + 1, y + 1, ramp(M2));
                }
            }
            t
        }
        Surface::Metal => {
            // Hammered plates (M0 / M1), iron_hi rivets, a hot iron_hot top-edge row.
            Tile::from_fn(|x, y| {
                if y == 0 {
                    return hot(IRON_HOT);
                }
                let (u, v) = (x % 8, y % 8);
                if (u == 1 || u == 6) && v == 2 {
                    return accent(IRON_HI);
                }
                let plate = ((x / 8 + y / 8) % 2 == 0) ^ (hash(x / 2, y / 2, 261) > 0.82);
                ramp(if plate { M0 } else { M1 })
            })
        }
        Surface::Glow => {
            // A flame tongue in hard bands (no dither): glow_core centre with a flame_white
            // heart, glow_fire body, glow_ember tips; a window_light rim for lantern glass.
            Tile::from_fn(|x, y| {
                if !(2..30).contains(&x) || !(2..30).contains(&y) {
                    return emit(WINDOW_LIGHT);
                }
                let dx = (x as f32 + 0.5 - 16.0).abs();
                let t = (y as f32 - 4.0) / 24.0;
                if !(0.0..=1.0).contains(&t) {
                    return emit(GLOW_FIRE);
                }
                let w = 11.0 * t.sqrt() * (1.0 - 0.25 * t);
                if dx > w {
                    emit(GLOW_FIRE)
                } else if t < 0.25 || dx > w - 1.5 {
                    emit(GLOW_EMBER)
                } else if dx < 1.0 && (0.55..0.85).contains(&t) {
                    emit(FLAME_WHITE)
                } else if dx < w * 0.55 && t > 0.35 {
                    emit(GLOW_CORE)
                } else {
                    emit(GLOW_FIRE)
                }
            })
        }
        Surface::Water => {
            // 1-px hard caustic squiggles (M0) over an M1 field.
            Tile::from_fn(|x, y| {
                let (d1, d2, _) = voronoi(x, y, 4, 271);
                ramp(if d2 - d1 < 0.9 { M0 } else { M1 })
            })
        }
    }
}

/// Texel of surface `s` at (x, y) (wrapping).
pub fn texel(s: Surface, x: i32, y: i32) -> Tx {
    tile(s).get(x, y)
}

fn srgb_byte(v: f32) -> u8 {
    (v.clamp(0.0, 1.0) * 255.0).round() as u8
}

/// The atlas as RGBA8 sRGB (alpha linear), nearest sampling.
pub fn atlas_image() -> Image {
    let (w, h) = (COLS * TILE, ROWS * TILE);
    let mut data = vec![0u8; (w * h * 4) as usize];
    for (i, s) in ALL.iter().enumerate() {
        let t = tile(*s);
        let (tx, ty) = (i as u32 % COLS * TILE, i as u32 / COLS * TILE);
        for y in 0..TILE {
            for x in 0..TILE {
                let c = t.get(x as i32, y as i32);
                let o = (((ty + y) * w + tx + x) * 4) as usize;
                for k in 0..4 {
                    data[o + k] = srgb_byte(c[k]);
                }
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
    use cairn_cove_core::palette;
    use std::collections::HashSet;

    fn key(t: Tx) -> [u8; 4] {
        t.map(srgb_byte)
    }

    fn count(t: &Tile, f: impl Fn(Tx) -> bool) -> usize {
        t.0.iter().filter(|v| f(**v)).count()
    }

    #[test]
    fn atlas_covers_every_surface() {
        assert_eq!(ALL.len(), Surface::COUNT);
        for (i, s) in ALL.iter().enumerate() {
            assert_eq!(*s as usize, i);
        }
        let img = atlas_image();
        assert_eq!(img.width(), COLS * TILE);
        assert_eq!(img.height(), ROWS * TILE);
        assert_eq!(img.data.as_ref().map(|d| d.len()), Some((COLS * TILE * ROWS * TILE * 4) as usize));
    }

    #[test]
    fn deterministic() {
        for s in ALL {
            assert_eq!(key(tile(s).get(5, 9)), key(tile(s).get(5, 9)));
            assert_eq!(tile(s).0.iter().map(|t| key(*t)).collect::<Vec<_>>(), tile(s).0.iter().map(|t| key(*t)).collect::<Vec<_>>());
        }
    }

    /// DESIGN A2: few distinct texel values per tile (survives affine warping), and only the
    /// documented alpha classes.
    #[test]
    fn few_values_and_known_alpha_classes() {
        for s in ALL {
            let t = tile(s);
            let distinct: HashSet<[u8; 4]> = t.0.iter().map(|v| key(*v)).collect();
            assert!(distinct.len() <= 6, "{s:?} has {} texel values", distinct.len());
            for v in &t.0 {
                assert!(v.iter().all(|c| c.is_finite() && (0.0..=1.0).contains(c)), "{s:?}");
                assert!([0.0, 0.5, 0.75, 0.85, 1.0].contains(&v[3]), "{s:?} alpha {}", v[3]);
            }
            // Ramp texels are on the lavender ramp (never grey: b >= r = g).
            for v in t.0.iter().filter(|v| v[3] == 0.5) {
                assert!(v[2] >= v[0] && v[0] == v[1], "{s:?} ramp texel {v:?}");
            }
            // Every tile shows some solid texels.
            assert!(count(&t, |v| v[3] > 0.0) > 256, "{s:?} mostly empty");
        }
    }

    /// Accent texels are palette tokens (or the trim sentinel), never ad-hoc colours.
    #[test]
    fn accents_are_tokens() {
        let tokens: HashSet<u32> = palette::TOKENS.iter().map(|t| t.1).collect();
        for s in ALL {
            for v in tile(s).0.iter().filter(|v| v[3] >= 0.75) {
                let [r, g, b, _] = key(*v);
                let hex = (r as u32) << 16 | (g as u32) << 8 | b as u32;
                assert!(hex == 0 && v[3] == 0.75 || tokens.contains(&hex), "{s:?} accent #{hex:06X}");
            }
        }
    }

    #[test]
    fn emissive_classes() {
        let glow = tile(Surface::Glow);
        assert!(glow.0.iter().all(|v| v[3] == 1.0), "glow is all emissive");
        let has = |t: &Tile, hex: u32| t.0.iter().any(|v| key(*v) == key(emit(hex)));
        for hex in [FLAME_WHITE, GLOW_CORE, GLOW_FIRE, GLOW_EMBER, WINDOW_LIGHT] {
            assert!(has(&glow, hex), "glow band #{hex:06X}");
        }
        // Window: a 6×10 emissive slot in window_light.
        let win = tile(Surface::Window);
        assert_eq!(count(&win, |v| v[3] == 1.0), 60);
        assert_eq!(key(win.get(15, 15)), key(emit(WINDOW_LIGHT)));
        // Basket rim is hot (night-emissive), stones are not emissive.
        assert_eq!(tile(Surface::Metal).get(9, 0)[3], 0.85);
        assert!(tile(Surface::DryStone).0.iter().all(|v| v[3] < 0.95));
    }

    #[test]
    fn cutouts_only_where_designed() {
        for s in ALL {
            let cut = count(&tile(s), |v| v[3] == 0.0);
            match s {
                Surface::Leaves => assert!((150..=400).contains(&cut), "leaves ~25 % holes, got {cut}"),
                Surface::Fiber => {
                    assert!(cut > 100);
                    // Rope twist at the tile seam stays solid (small lashings sample there).
                    let t = tile(s);
                    assert!((0..32).all(|x| t.get(x, 0)[3] > 0.0 && t.get(x, 31)[3] > 0.0));
                }
                _ => assert_eq!(cut, 0, "{s:?}"),
            }
        }
    }

    /// Specific DESIGN A2 motifs.
    #[test]
    fn motifs() {
        let has = |s: Surface, v: Tx| tile(s).0.iter().any(|t| key(*t) == key(v));
        // Dry stone: joints + every third stone lilac.
        let ds = tile(Surface::DryStone);
        let alt = count(&ds, |v| key(v) == key(accent(DRY_STONE_ALT)));
        assert!((150..450).contains(&alt), "about a third lilac stones: {alt}");
        assert!(has(Surface::DryStone, accent(STONE_JOINT)));
        // Plank / hide / door carry the trim sentinel; window is trim-tinted slats.
        for s in [Surface::Plank, Surface::Hide, Surface::Door] {
            assert!(has(s, TRIM), "{s:?} trim");
        }
        let plank = tile(Surface::Plank);
        assert!((0..32).all(|x| plank.get(x, 15) == TRIM || plank.get(x, 15)[3] == 0.75));
        // Thatch bound course every 8th row.
        let th = tile(Surface::Thatch);
        assert!((0..32).all(|x| key(th.get(x, 8)) == key(ramp(M3))));
        // Turf: sod band at the bottom, grass on top.
        let tu = tile(Surface::Turf);
        assert_eq!(key(tu.get(3, 31)), key(accent(SOD)));
        assert_eq!(tu.get(3, 2)[3], 0.5);
        assert!(has(Surface::Timber, accent(ROPE)));
        assert!(has(Surface::Boards, accent(BOARDS_HI)) && has(Surface::Boards, accent(BOARD_GAP)));
        assert!(has(Surface::Flagstone, accent(GRASS)) && has(Surface::Flagstone, accent(FLAGSTONE_EDGE)));
        assert!(has(Surface::Pebble, accent(PEBBLE_ROSE)) && has(Surface::Pebble, accent(WET_SAND)));
        assert!(has(Surface::Carving, accent(BARK)) && has(Surface::Carving, accent(SALT_BLEACH)));
        assert!(has(Surface::Grass, accent(GRASS_TIP)) && has(Surface::Rock, accent(CLIFF_BAND)));
        // Water caustics: the shader keys on M0 (r > 0.97) over M1.
        let w = tile(Surface::Water);
        let m0 = count(&w, |v| v[0] > 0.97);
        assert!(m0 > 30 && m0 < 512);
    }
}
