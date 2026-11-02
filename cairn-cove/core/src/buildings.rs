//! Huts, roundhouses, beacons and their roofs.
//!
//! A **hut** is a ring of thick walls around its [`Footprint`] under a chunky roof:
//! - Walls by [`Material`]: `Wattle` (daub panels in the wall colour between timber corner and
//!   mid posts, a sill beam and a wall plate), `Plank` (vertical planks with painted horizontal
//!   bands in the trim colour, corner boards), `Stone` (dry-stone courses on a boulder footing,
//!   corner quoins and proud through-stones).
//! - Roofs by [`RoofStyle`] on rectangles: `Thatch` (thick hipped thatch, visible thickness at a
//!   slightly ragged eave, bound ridge roll), `Turf` (low grassy gable, sod edge strips, tiny
//!   flowers), `Hide` (hide stretched over rafter poles that poke out past the ridge, sagging
//!   between the poles, painted zigzag hem) and `Boat` (an upturned hull: curved keel rising at
//!   both ends, strakes alternating roof colour and boards, carved stems). Round footprints get a
//!   thatch cone with a topknot (`Thatch` and `Boat`), a low turf dome or a conical hide tent
//!   with its poles through the apex. `roof_pitch` sets the rise; only turf may be flat.
//! - `carved`: crossed carved gable horns (or an apex finial) plus a painted band under the
//!   eaves and carved door posts. `hangings`: drying fish, net bundles and shell strings on a
//!   rail under the eaves.
//! - **Stilts** ([`needs_stilts`]): an explicit stilt height, a footprint over the sea or a
//!   lagoon, or ground varying more than 0.8 m raise the floor onto a plank deck on log posts
//!   that run down to the ground / seabed (floor ≥ water + 1.2 m over water), with a ladder (or
//!   log steps when the drop is small) down from the door.
//! - With no explicit openings a hut gets a door on its longest side (hide curtain, or planks
//!   on plank huts), 1–3 small shuttered windows, a smoke hole and a lantern by the door.
//!   Explicit openings: `Window` (shuttered), `Door` (hide-curtain doorway), `WideDoor` (plank
//!   boat door). Paths through the wall (`cuts`) become doorways too.
//!
//! A tall round house ([`is_beacon`]) is a **beacon**: a tapered dry-stone tower with wind holes,
//! a stone top platform and an iron fire basket (`Fire` marker), optionally under a small thatch
//! cone on posts.
//!
//! All randomness hashes `(seed ^ salt, footprint centre)` so previews equal committed results.

use crate::geom::{footprint_polygon, outward, signed_area};
use crate::mesh::{Marker, MarkerKind, MeshData, Rng, Surface, tangent_frame};
use crate::model::{Colors, Footprint, House, Material, OpeningKind, RoofStyle, SEA_LEVEL};
use crate::palette;
use crate::terrain::Terrain;
use crate::walls::Cut;
use glam::{Vec2, Vec3, Vec3Swizzles};
use std::f32::consts::{FRAC_PI_2, FRAC_PI_4, TAU};

/// Texture repeats per metre.
const UV: f32 = 0.5;
/// Walls on the ground reach this far below the lowest ground sample.
const SINK: f32 = 0.3;
/// Ground height range across a footprint above which a hut goes up on stilts.
const STEEP: f32 = 0.8;
/// Minimum floor height above the water surface for huts over water.
const WATER_CLEAR: f32 = 1.2;
/// Minimum floor height above the highest ground sample for automatic stilts.
const MIN_STILT: f32 = 0.5;
/// Stilt deck extends this far beyond the wall line.
const DECK_OUT: f32 = 0.35;
const DECK_T: f32 = 0.2;
/// Beacon top radius / base radius.
const BEACON_TAPER: f32 = 0.74;
const Y: Vec3 = Vec3::Y;

// Salts of the independent decision streams (see [`pick`]).
const SALT_DOOR: u64 = 0xCA12_0100;
const SALT_WIN: u64 = 0xCA12_0200;
const SALT_HANG: u64 = 0xCA12_0300;
const SALT_EAVE: u64 = 0xCA12_0400;
const SALT_SCATTER: u64 = 0xCA12_0500;
const SALT_STONE: u64 = 0xCA12_0600;

/// One RNG stream per decision, seeded from `(seed ^ salt, footprint centre)`.
fn pick(seed: u64, salt: u64, at: Vec2) -> Rng {
    Rng::at(seed ^ salt, at.x, at.y)
}

// =============================================================================================
// Public API
// =============================================================================================

/// Hut / roundhouse / beacon mesh plus markers (doors, lantern, lit windows, smoke hole,
/// perches, beacon fire). `cuts` are paths through the walls and become doorways.
pub fn house_mesh(house: &House, cuts: &[Cut], terrain: &Terrain, seed: u64) -> (MeshData, Vec<Marker>) {
    let fp = sane_footprint(house.footprint);
    let poly = footprint_polygon(&fp);
    let center = area_centroid(&poly);
    let site = site(house, &poly, terrain);
    let look = Look::new(house);
    let mut b = B::default();
    match fp {
        Footprint::Round { radius, .. } if is_beacon(house) => {
            beacon(&mut b, house, &poly, center, radius, &look, &site, cuts, seed)
        }
        _ => hut(&mut b, house, &poly, center, &look, &site, cuts, terrain, seed),
    }
    (b.m, b.mk)
}

/// A round house whose wall height is at least 2.5 × its radius is a beacon tower: a tapered
/// dry-stone tower with a fire basket on top (a `Fire` marker). Beacons never stand on stilts.
pub fn is_beacon(house: &House) -> bool {
    match sane_footprint(house.footprint) {
        Footprint::Round { radius, .. } => wall_h(house) >= 2.5 * radius,
        Footprint::Rect { .. } => false,
    }
}

/// True if the hut is raised on stilts: an explicit `stilts` height, any part of the footprint
/// over the sea or a lagoon, or a ground range across the footprint above 0.8 m. Beacons never
/// are. The scene skips flattening the ground under such huts.
pub fn needs_stilts(house: &House, terrain: &Terrain) -> bool {
    site(house, &footprint_polygon(&sane_footprint(house.footprint)), terrain).stilts
}

/// Roof over an arbitrary simple polygon `poly` (counter-clockwise, as `geom::footprint_polygon`)
/// with eaves at height `base_y`. Used for roofed dyke rings. `pitch` as in [`House::roof_pitch`].
///
/// Rectangles get the hut roofs (hipped thatch, turf / hide gables, boat hull); other polygons
/// whose centroid sees every edge get the round roofs (thatch cone, turf dome, hide tent);
/// concave rings and a flat turf pitch get a flat roof of the style's covering.
pub fn roof_mesh(poly: &[Vec2], base_y: f32, style: RoofStyle, pitch: f32, colors: Colors, seed: u64) -> (MeshData, Vec<Marker>) {
    let mut b = B::default();
    // Ring roofs sit on the dyke's centre line: reach a little further to cover the wall top.
    let ctx = RoofCtx::new(style, pitch, colors, 0.0, false, (Surface::DryStone, palette::dry_stone()), seed, 0.35);
    let out = roof_impl(&mut b, poly, fin(base_y, 0.0), &ctx);
    for p in out.perches {
        b.marker(MarkerKind::Perch, p, Y);
    }
    (b.m, b.mk)
}

// =============================================================================================
// Small helpers
// =============================================================================================

fn fin(x: f32, d: f32) -> f32 {
    if x.is_finite() { x } else { d }
}

fn p3(v: Vec2, y: f32) -> Vec3 {
    Vec3::new(v.x, y, v.y)
}

fn wall_h(house: &House) -> f32 {
    fin(house.wall_height, 3.0).clamp(0.6, 24.0)
}

fn sane_footprint(fp: Footprint) -> Footprint {
    let v = |c: Vec2| Vec2::new(fin(c.x, 0.0).clamp(-1e4, 1e4), fin(c.y, 0.0).clamp(-1e4, 1e4));
    match fp {
        Footprint::Rect { center, half, angle } => Footprint::Rect {
            center: v(center),
            half: Vec2::new(fin(half.x, 2.0).abs().clamp(0.5, 30.0), fin(half.y, 2.0).abs().clamp(0.5, 30.0)),
            angle: fin(angle, 0.0),
        },
        Footprint::Round { center, radius } => Footprint::Round { center: v(center), radius: fin(radius, 2.0).abs().clamp(0.6, 30.0) },
    }
}

/// Miter offset of a CCW polygon (positive = outward). Miter length is clamped.
fn offset_poly(poly: &[Vec2], d: f32) -> Vec<Vec2> {
    let n = poly.len();
    if d == 0.0 || n < 3 {
        return poly.to_vec();
    }
    (0..n)
        .map(|i| {
            let p = poly[i];
            let a = poly[(i + n - 1) % n];
            let c = poly[(i + 1) % n];
            let n0 = outward((p - a).normalize_or_zero());
            let n1 = outward((c - p).normalize_or_zero());
            let m = (n0 + n1).normalize_or(n1);
            let cos = m.dot(n1).max(0.5);
            p + m * (d / cos)
        })
        .collect()
}

/// Removes non-finite, duplicate and collinear points; returns a CCW polygon.
fn clean_poly(poly: &[Vec2]) -> Vec<Vec2> {
    let mut v: Vec<Vec2> = poly.iter().copied().filter(|p| p.is_finite() && p.abs().max_element() < 1e5).collect();
    v.dedup_by(|a, b| a.distance(*b) < 1e-3);
    while v.len() > 1 && v[0].distance(v[v.len() - 1]) < 1e-3 {
        v.pop();
    }
    let mut changed = true;
    while changed && v.len() >= 3 {
        changed = false;
        let n = v.len();
        for i in 0..n {
            let (a, p, c) = (v[(i + n - 1) % n], v[i], v[(i + 1) % n]);
            let cross = (p - a).perp_dot(c - p);
            if cross.abs() < 1e-5 * (p - a).length().max(1e-3) * (c - p).length().max(1e-3) + 1e-9 {
                v.remove(i);
                changed = true;
                break;
            }
        }
    }
    if v.len() >= 3 && signed_area(&v) < 0.0 {
        v.reverse();
    }
    v
}

fn area_centroid(poly: &[Vec2]) -> Vec2 {
    let n = poly.len();
    let mut a = 0.0;
    let mut c = Vec2::ZERO;
    for i in 0..n {
        let (p, q) = (poly[i], poly[(i + 1) % n]);
        let cr = p.perp_dot(q);
        a += cr;
        c += (p + q) * cr;
    }
    if a.abs() < 1e-6 { poly.iter().copied().sum::<Vec2>() / n.max(1) as f32 } else { c / (3.0 * a) }
}

/// Distance from `c` to the nearest edge line of `poly`.
fn inradius(poly: &[Vec2], c: Vec2) -> f32 {
    let n = poly.len();
    (0..n)
        .map(|i| {
            let (a, q) = (poly[i], poly[(i + 1) % n]);
            (c - a).perp_dot((q - a).normalize_or_zero()).abs()
        })
        .fold(f32::INFINITY, f32::min)
        .min(1e4)
}

/// True if `c` sees every edge of the CCW polygon from its inner side.
fn star_shaped(poly: &[Vec2], c: Vec2) -> bool {
    let n = poly.len();
    (0..n).all(|i| (poly[(i + 1) % n] - poly[i]).perp_dot(c - poly[i]) > 1e-4)
}

/// Ear-clipping triangulation of a simple CCW polygon (falls back to a fan).
fn triangulate(poly: &[Vec2]) -> Vec<[usize; 3]> {
    let n = poly.len();
    let mut out = Vec::new();
    if n < 3 {
        return out;
    }
    let mut idx: Vec<usize> = (0..n).collect();
    let inside = |p: Vec2, a: Vec2, b: Vec2, c: Vec2| {
        (b - a).perp_dot(p - a) >= -1e-7 && (c - b).perp_dot(p - b) >= -1e-7 && (a - c).perp_dot(p - c) >= -1e-7
    };
    let mut guard = 0;
    while idx.len() > 3 && guard < n * n + 8 {
        guard += 1;
        let m = idx.len();
        let mut found = false;
        for k in 0..m {
            let (i0, i1, i2) = (idx[(k + m - 1) % m], idx[k], idx[(k + 1) % m]);
            let (a, b, c) = (poly[i0], poly[i1], poly[i2]);
            if (b - a).perp_dot(c - b) <= 1e-9 {
                continue;
            }
            if idx.iter().any(|&j| j != i0 && j != i1 && j != i2 && inside(poly[j], a, b, c)) {
                continue;
            }
            out.push([i0, i1, i2]);
            idx.remove(k);
            found = true;
            break;
        }
        if !found {
            break;
        }
    }
    for k in 1..idx.len().saturating_sub(1) {
        out.push([idx[0], idx[k], idx[k + 1]]);
    }
    out
}

/// Position on the perimeter: (edge index, distance along that edge).
fn locate(lens: &[f32], along: f32) -> (usize, f32) {
    let per: f32 = lens.iter().sum();
    if per <= 1e-4 || !along.is_finite() {
        return (0, lens.first().copied().unwrap_or(0.0) * 0.5);
    }
    let mut rest = along.rem_euclid(per);
    for (i, &l) in lens.iter().enumerate() {
        if rest <= l {
            return (i, rest);
        }
        rest -= l;
    }
    (lens.len() - 1, *lens.last().unwrap_or(&0.0))
}

/// Index of the longest edge (the first one on ties).
fn longest(lens: &[f32]) -> usize {
    let mut best = 0;
    for (i, &l) in lens.iter().enumerate() {
        if l > lens[best] + 1e-4 {
            best = i;
        }
    }
    best
}

/// Complement of the union of `block` intervals within `[0, len]`.
fn spans(len: f32, block: impl Iterator<Item = (f32, f32)>) -> Vec<(f32, f32)> {
    let mut iv: Vec<(f32, f32)> = block.filter(|(a, b)| a.is_finite() && b.is_finite() && b > a).collect();
    iv.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut out = Vec::new();
    let mut x = 0.0f32;
    for (a, b) in iv {
        if a > x + 1e-4 {
            out.push((x, a.min(len)));
        }
        x = x.max(b);
        if x >= len {
            break;
        }
    }
    if x < len - 1e-4 {
        out.push((x, len));
    }
    out.retain(|(a, b)| b - a > 1e-4);
    out
}

fn eave_drop(rise: f32, run: f32, overhang: f32) -> f32 {
    if run > 0.05 { (rise / run * overhang).min(0.6) } else { 0.0 }
}

/// Mesh + markers under construction, plus the roof's top-surface triangles (for scattering
/// flowers and lichen on them).
#[derive(Default)]
struct B {
    m: MeshData,
    mk: Vec<Marker>,
    tops: Vec<[Vec3; 3]>,
}

impl B {
    fn marker(&mut self, kind: MarkerKind, pos: Vec3, dir: Vec3) {
        if pos.is_finite() && dir.is_finite() {
            self.mk.push(Marker { kind, pos, dir });
        }
    }

    /// Triangle whose front faces `want`. Degenerate triangles are skipped.
    fn tri(&mut self, a: Vec3, b: Vec3, c: Vec3, want: Vec3, col: Vec3, s: Surface) {
        let n = (b - a).cross(c - a);
        if n.length_squared() < 1e-12 || !n.is_finite() {
            return;
        }
        if n.dot(want) < 0.0 { self.m.tri(a, c, b, col, s, UV) } else { self.m.tri(a, b, c, col, s, UV) }
    }

    /// Quad whose front faces `want`, UVs from its edge lengths. Near-degenerate quads (two
    /// coincident corners) fall back to two triangles.
    fn quad(&mut self, p: [Vec3; 4], want: Vec3, col: Vec3, s: Surface) {
        let [a, b, c, d] = p;
        let n = (b - a).cross(d - a);
        if n.length_squared() < 1e-10 || !n.is_finite() {
            self.tri(a, b, c, want, col, s);
            self.tri(a, c, d, want, col, s);
            return;
        }
        if n.dot(want) < 0.0 { self.m.quad(a, d, c, b, col, s, UV) } else { self.m.quad(a, b, c, d, col, s, UV) }
    }

    /// Quad with explicit UVs whose front faces `want`.
    fn quad_uv(&mut self, p: [Vec3; 4], uv: [Vec2; 4], want: Vec3, col: Vec3, s: Surface) {
        let n = (p[1] - p[0]).cross(p[3] - p[0]);
        if n.length_squared() < 1e-10 || !n.is_finite() {
            return;
        }
        if n.dot(want) < 0.0 {
            self.m.quad_uv([p[0], p[3], p[2], p[1]], [uv[0], uv[3], uv[2], uv[1]], col, s);
        } else {
            self.m.quad_uv(p, uv, col, s);
        }
    }

    /// Oriented box (centre, half extents along `x_axis`, up, and their cross).
    fn bx(&mut self, c: Vec3, half: Vec3, x_axis: Vec3, col: Vec3, s: Surface) {
        if c.is_finite() && half.is_finite() && x_axis.is_finite() && half.min_element() > 1e-4 {
            self.m.oriented_box(c, half, x_axis, col, s, UV);
        }
    }

    /// Square bar of side `w` centred on the segment `a`→`b`.
    fn bar(&mut self, a: Vec3, b: Vec3, w: f32, col: Vec3, s: Surface) {
        let d = b - a;
        if !d.is_finite() || d.length() < 1e-4 {
            return;
        }
        let dir = d.normalize();
        let side = dir.cross(Y).normalize_or(Vec3::X);
        let up = side.cross(dir).normalize_or(Y);
        let off = -up * (w * 0.5);
        self.m.beam(a + off, b + off, w, w, col, s, UV);
    }

    /// Hexagonal log post from `base` up to height `top`.
    fn post(&mut self, base: Vec3, top: f32, r: f32, col: Vec3, s: Surface) {
        if base.is_finite() && top.is_finite() && top - base.y > 0.02 {
            self.m.cylinder(base, r, r * 0.85, top - base.y, 6, true, false, col, s, UV);
        }
    }
}

/// Colours and wall material of one hut (weathering already applied).
struct Look {
    w: f32,
    wall_s: Surface,
    wall_c: Vec3,
    thick: f32,
    trim: Vec3,
    wood: Vec3,
    drift: Vec3,
    boards: Vec3,
    rope: Vec3,
    stone: Vec3,
    hide: Vec3,
}

/// Lerp toward salt bleach (DESIGN: `0.6·w` on wood, daub, thatch and hide).
fn bleach(c: Vec3, w: f32) -> Vec3 {
    c.lerp(palette::salt_bleach(), 0.6 * w)
}

impl Look {
    fn new(house: &House) -> Look {
        let w = fin(house.weathering, 0.0).clamp(0.0, 1.0);
        let c = house.colors;
        let (wall_s, wall_c, thick) = match house.material {
            Material::Wattle => (Surface::Daub, bleach(palette::daub(c), w), 0.24),
            Material::Plank => (Surface::Plank, bleach(palette::boards(), w), 0.18),
            // Dry stone with a thin mortar wash in the wall colour.
            Material::Stone => (Surface::DryStone, palette::dry_stone().lerp(palette::wall(c), 0.15), 0.42),
        };
        Look {
            w,
            wall_s,
            wall_c,
            thick,
            trim: palette::trim(c),
            wood: bleach(palette::wood(), w),
            drift: bleach(palette::driftwood(), w),
            boards: bleach(palette::boards(), w),
            rope: palette::rope(),
            stone: palette::stone(),
            hide: bleach(palette::hide(), w),
        }
    }
}

// =============================================================================================
// Site: ground, water, stilts
// =============================================================================================

struct Site {
    /// Lowest ground sample under the footprint.
    lo: f32,
    stilts: bool,
    /// Floor level (top of the stilt deck, or just above the lowest and highest ground).
    floor: f32,
}

fn site(house: &House, poly: &[Vec2], terrain: &Terrain) -> Site {
    let c = area_centroid(poly);
    let n = poly.len();
    let mut pts: Vec<Vec2> = poly.to_vec();
    for i in 0..n {
        pts.push((poly[i] + poly[(i + 1) % n]) * 0.5);
        pts.push((poly[i] + c) * 0.5);
    }
    pts.push(c);
    let (mut lo, mut hi, mut water) = (f32::INFINITY, f32::NEG_INFINITY, None::<f32>);
    for p in pts {
        let g = fin(terrain.height(p), 0.0);
        lo = lo.min(g);
        hi = hi.max(g);
        if let Some(l) = terrain.water_level(p).filter(|l| l.is_finite() && g < *l) {
            water = Some(water.map_or(l, |w| w.max(l)));
        }
    }
    if !lo.is_finite() || !hi.is_finite() {
        (lo, hi) = (0.0, 0.0);
    }
    let explicit = fin(house.stilts, 0.0).clamp(0.0, 8.0);
    let stilts = !is_beacon(house) && (explicit > 0.01 || water.is_some() || hi - lo > STEEP);
    let floor = if stilts {
        let f = hi + explicit.max(MIN_STILT);
        water.map_or(f, |l| f.max(l.max(SEA_LEVEL) + WATER_CLEAR))
    } else {
        // Levelled ground is flat to a few cm; where a neighbour's pad leaves a lip the grid
        // cannot smooth out, the floor still clears every bit of ground under it.
        (lo + 0.1).max(hi + 0.02)
    };
    Site { lo, stilts, floor }
}

// =============================================================================================
// Walls
// =============================================================================================

/// One wall facet. Bottom outer edge `ob`, top outer edge `ot` (equal for vertical walls,
/// pulled in for the tapered beacon), inner edges `ib` / `it`. `u` is measured in metres along
/// the bottom outer edge; points lerp proportionally on the other edges.
#[derive(Clone, Copy, Debug)]
struct Edge {
    ob: [Vec2; 2],
    ot: [Vec2; 2],
    ib: [Vec2; 2],
    it: [Vec2; 2],
    y_lo: f32,
    y_hi: f32,
    len: f32,
    d: Vec3,
    n: Vec3,
}

impl Edge {
    fn pt(&self, u: f32, y: f32, inner: bool) -> Vec3 {
        let f = if self.len > 1e-5 { (u / self.len).clamp(-1.0, 2.0) } else { 0.0 };
        let k = ((y - self.y_lo) / (self.y_hi - self.y_lo).max(1e-4)).clamp(-1.0, 2.0);
        let (b, t) = if inner { (self.ib, self.it) } else { (self.ob, self.ot) };
        let bot = b[0].lerp(b[1], f);
        let top = t[0].lerp(t[1], f);
        p3(bot.lerp(top, k), y)
    }

    /// Point on (or `out` metres proud of) the outer face.
    fn at(&self, u: f32, y: f32, out: f32) -> Vec3 {
        self.pt(u, y, false) + self.n * out
    }
}

fn make_edges(ob: &[Vec2], ot: &[Vec2], ib: &[Vec2], it: &[Vec2], y_lo: f32, y_hi: f32) -> Vec<Edge> {
    let n = ob.len();
    (0..n)
        .map(|i| {
            let j = (i + 1) % n;
            let d2 = (ob[j] - ob[i]).normalize_or(Vec2::X);
            Edge {
                ob: [ob[i], ob[j]],
                ot: [ot[i], ot[j]],
                ib: [ib[i], ib[j]],
                it: [it[i], it[j]],
                y_lo,
                y_hi,
                len: ob[i].distance(ob[j]),
                d: p3(d2, 0.0),
                n: p3(outward(d2), 0.0),
            }
        })
        .collect()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Hk {
    /// Small shuttered window.
    Window,
    /// Doorway (hide curtain or plank door).
    Door,
    /// Wide plank boat door.
    Wide,
    /// Beacon wind hole.
    Wind,
}

/// A hole through one wall facet: `u` along the edge, absolute `y`.
#[derive(Clone, Copy, Debug)]
struct Hole {
    edge: usize,
    u0: f32,
    u1: f32,
    y0: f32,
    y1: f32,
    kind: Hk,
}

impl Hole {
    fn is_door(&self) -> bool {
        matches!(self.kind, Hk::Door | Hk::Wide)
    }
    fn mid(&self) -> f32 {
        (self.u0 + self.u1) * 0.5
    }
}

/// Places a hole of width `w` centred at `u` (clamped into the edge); rejects it when it does
/// not fit or overlaps an existing hole.
#[allow(clippy::too_many_arguments)]
fn place(holes: &mut Vec<Hole>, lens: &[f32], edge: usize, u: f32, w: f32, y0: f32, y1: f32, kind: Hk) -> bool {
    let Some(&len) = lens.get(edge) else { return false };
    let margin = if len < 1.6 { 0.06 } else { 0.35 };
    let w = w.min(len - 2.0 * margin);
    if w < 0.2 || y1 - y0 < 0.25 || !(y0.is_finite() && y1.is_finite()) {
        return false;
    }
    let lo = margin + w * 0.5;
    let hi = (len - margin - w * 0.5).max(lo);
    let u = fin(u, len * 0.5).clamp(lo, hi);
    let h = Hole { edge, u0: u - w * 0.5, u1: u + w * 0.5, y0, y1, kind };
    if holes.iter().any(|o| o.edge == edge && h.u0 < o.u1 + 0.15 && o.u0 < h.u1 + 0.15 && h.y0 < o.y1 + 0.1 && o.y0 < h.y1 + 0.1) {
        return false;
    }
    holes.push(h);
    true
}

/// Doors, windows and wind holes. Paths (`cuts`) first, then explicit openings or, when there
/// are none, a door on the longest side plus 1–3 windows.
#[allow(clippy::too_many_arguments)]
fn plan_openings(house: &House, cuts: &[Cut], lens: &[f32], floor: f32, h: f32, round: bool, tower: bool, seed: u64, c: Vec2) -> Vec<Hole> {
    let mut holes = Vec::new();
    let n = lens.len();
    if n == 0 {
        return holes;
    }
    let dh = {
        let d = (h - 0.3).min(1.9);
        if d < 0.5 { h * 0.8 } else { d }
    };
    let (wy0, wy1) = {
        let y0 = floor + (h * 0.42).clamp(0.45, 1.05);
        (y0, (y0 + 0.55).min(floor + h - 0.15))
    };
    for cut in cuts {
        let (e, u) = locate(lens, cut.along);
        let ch = fin(cut.height, dh).max(0.5).min(dh);
        place(&mut holes, lens, e, u, fin(cut.width, 1.0).clamp(0.9, 2.0), floor, floor + ch, Hk::Door);
    }
    if !house.openings.is_empty() {
        for o in &house.openings {
            let (e, u) = locate(lens, o.along);
            match o.kind {
                OpeningKind::Window => place(&mut holes, lens, e, u, 0.6, wy0, wy1, Hk::Window),
                OpeningKind::Door => place(&mut holes, lens, e, u, 1.0, floor, floor + dh, Hk::Door),
                OpeningKind::WideDoor => {
                    let wh = (h - 0.2).min(2.3).max(dh);
                    place(&mut holes, lens, e, u, 2.0, floor, floor + wh.min(h - 0.05), Hk::Wide)
                }
            };
        }
    } else {
        if !holes.iter().any(Hole::is_door) {
            let mut rng = pick(seed, SALT_DOOR, c);
            let first = if round { (rng.next_u64() % n as u64) as usize } else { longest(lens) };
            for k in 0..n {
                let e = (first + k) % n;
                if place(&mut holes, lens, e, lens[e] * 0.5, 1.0, floor, floor + dh, Hk::Door) {
                    break;
                }
            }
        }
        if !tower && h >= 1.3 {
            let mut rng = pick(seed, SALT_WIN, c);
            let want = 1 + (rng.next_u64() % 3) as usize;
            let door_edge = holes.iter().find(|h| h.is_door()).map_or(0, |h| h.edge);
            let step = (n / (want + 1)).max(1);
            let mut got = 0;
            for k in 1..=2 * n {
                if got == want {
                    break;
                }
                let e = (door_edge + k * step) % n;
                let u = lens[e] * rng.range(0.3, 0.7);
                if place(&mut holes, lens, e, u, 0.6, wy0, wy1, Hk::Window) {
                    got += 1;
                }
            }
        }
    }
    if tower {
        // Wind holes in three tiers, four per tier, staggered between tiers.
        let step = (n / 4).max(2);
        for (t, f) in [0.4f32, 0.62, 0.82].into_iter().enumerate() {
            let y0 = floor + h * f;
            for e in (t * step / 2..n).step_by(step) {
                place(&mut holes, lens, e, lens[e] * 0.5, 0.3, y0, (y0 + 0.42).min(floor + h - 0.2), Hk::Wind);
            }
        }
    }
    holes
}

/// Outer and inner faces of every facet, split around the holes, plus reveals (jambs, sills,
/// heads) and an optional top cap.
fn walls(b: &mut B, edges: &[Edge], holes: &[Hole], s: Surface, col: Vec3, cap: bool) {
    for (i, e) in edges.iter().enumerate() {
        let hs: Vec<&Hole> = holes.iter().filter(|h| h.edge == i).collect();
        let mut ys = vec![e.y_lo, e.y_hi];
        for h in &hs {
            ys.push(h.y0.clamp(e.y_lo, e.y_hi));
            ys.push(h.y1.clamp(e.y_lo, e.y_hi));
        }
        ys.sort_by(f32::total_cmp);
        ys.dedup_by(|a, b| (*a - *b).abs() < 1e-4);
        for w in ys.windows(2) {
            let (ya, yb) = (w[0], w[1]);
            let blocked = hs.iter().filter(|h| h.y0 <= ya + 1e-4 && h.y1 >= yb - 1e-4).map(|h| (h.u0, h.u1));
            for (u0, u1) in spans(e.len, blocked) {
                let uv = [Vec2::new(u0, ya), Vec2::new(u1, ya), Vec2::new(u1, yb), Vec2::new(u0, yb)].map(|v| v * UV);
                b.quad_uv([e.pt(u0, ya, false), e.pt(u1, ya, false), e.pt(u1, yb, false), e.pt(u0, yb, false)], uv, e.n, col, s);
                b.quad_uv([e.pt(u0, ya, true), e.pt(u1, ya, true), e.pt(u1, yb, true), e.pt(u0, yb, true)], uv, -e.n, col, s);
            }
        }
        for h in &hs {
            let (y0, y1) = (h.y0.max(e.y_lo), h.y1.min(e.y_hi));
            if y1 <= y0 {
                continue;
            }
            let (u0, u1) = (h.u0, h.u1);
            b.quad([e.pt(u0, y0, false), e.pt(u0, y0, true), e.pt(u0, y1, true), e.pt(u0, y1, false)], e.d, col, s);
            b.quad([e.pt(u1, y0, false), e.pt(u1, y0, true), e.pt(u1, y1, true), e.pt(u1, y1, false)], -e.d, col, s);
            if y0 > e.y_lo + 0.01 {
                b.quad([e.pt(u0, y0, false), e.pt(u1, y0, false), e.pt(u1, y0, true), e.pt(u0, y0, true)], Y, col, s);
            }
            if y1 < e.y_hi - 0.01 {
                b.quad([e.pt(u0, y1, false), e.pt(u1, y1, false), e.pt(u1, y1, true), e.pt(u0, y1, true)], -Y, col, s);
            }
        }
        if cap {
            b.quad([e.pt(0.0, e.y_hi, false), e.pt(e.len, e.y_hi, false), e.pt(e.len, e.y_hi, true), e.pt(0.0, e.y_hi, true)], Y, col, s);
        }
    }
}

/// A horizontal band `out` metres proud of a facet between `ya` and `yb`, interrupted by the
/// `block` intervals (sills, painted bands, carved bands, footings).
#[allow(clippy::too_many_arguments)]
fn strip(b: &mut B, e: &Edge, ya: f32, yb: f32, out: f32, block: &[(f32, f32)], col: Vec3, s: Surface) {
    if yb <= ya + 1e-3 {
        return;
    }
    for (u0, u1) in spans(e.len, block.iter().copied()) {
        if u1 - u0 < 0.05 {
            continue;
        }
        b.quad([e.at(u0, ya, out), e.at(u1, ya, out), e.at(u1, yb, out), e.at(u0, yb, out)], e.n, col, s);
        if out > 0.025 {
            b.quad([e.at(u0, yb, 0.0), e.at(u1, yb, 0.0), e.at(u1, yb, out), e.at(u0, yb, out)], Y, col, s);
            b.quad([e.at(u0, ya, 0.0), e.at(u1, ya, 0.0), e.at(u1, ya, out), e.at(u0, ya, out)], -Y, col, s);
        }
    }
}

/// Holes on facet `i` overlapping `[ya, yb]` (doors only if `doors_only`), padded.
fn blocks(holes: &[Hole], i: usize, ya: f32, yb: f32, doors_only: bool) -> Vec<(f32, f32)> {
    holes
        .iter()
        .filter(|h| h.edge == i && h.y0 < yb && h.y1 > ya && (!doors_only || h.is_door()))
        .map(|h| (h.u0 - 0.06, h.u1 + 0.06))
        .collect()
}

fn in_hole(holes: &[Hole], i: usize, u: f32, pad: f32) -> bool {
    holes.iter().any(|h| h.edge == i && u > h.u0 - pad && u < h.u1 + pad)
}

/// Material dressing: timber frame on wattle, corner boards and painted bands on planks,
/// footing, quoins and through-stones on stone, carved band and lichen.
#[allow(clippy::too_many_arguments)]
fn dress(b: &mut B, house: &House, edges: &[Edge], holes: &[Hole], look: &Look, floor: f32, round: bool, seed: u64, c: Vec2) {
    let n = edges.len();
    let mut rng = pick(seed, SALT_STONE, c);
    for (i, e) in edges.iter().enumerate() {
        let prev = &edges[(i + n - 1) % n];
        let inward = -(e.n + prev.n).normalize_or(e.n);
        let corner = e.ob[0];
        let ymid = (e.y_lo + e.y_hi) * 0.5;
        let hh = (e.y_hi - e.y_lo) * 0.5;
        let h = e.y_hi - floor;
        match house.material {
            Material::Wattle => {
                if !round || i % 2 == 0 {
                    b.bx(p3(corner, ymid) + inward * 0.08, Vec3::new(0.14, hh, 0.14), e.d, look.wood, Surface::Timber);
                }
                if e.len > 2.0 {
                    let k = (e.len / 1.7).floor() as usize;
                    for j in 1..k {
                        let u = e.len * j as f32 / k as f32;
                        if !in_hole(holes, i, u, 0.14) {
                            b.bx(e.at(u, ymid, 0.02), Vec3::new(0.1, hh, 0.08), e.d, look.wood, Surface::Timber);
                        }
                    }
                }
                strip(b, e, floor - 0.04, floor + 0.14, 0.05, &blocks(holes, i, floor - 0.04, floor + 0.14, true), look.wood, Surface::Timber);
                strip(b, e, e.y_hi - 0.2, e.y_hi, 0.05, &blocks(holes, i, e.y_hi - 0.2, e.y_hi, false), look.wood, Surface::Timber);
            }
            Material::Plank => {
                if !round {
                    b.bx(p3(corner, ymid) + inward * 0.05, Vec3::new(0.1, hh, 0.1), e.d, look.wood, Surface::Timber);
                }
                for f in [0.3, 0.72] {
                    let ya = floor + h * f;
                    strip(b, e, ya, ya + 0.16, 0.02, &blocks(holes, i, ya, ya + 0.16, false), look.trim, Surface::Plank);
                }
            }
            Material::Stone => {
                let top = floor + 0.35;
                strip(b, e, e.y_lo, top, 0.08, &blocks(holes, i, e.y_lo, top, true), palette::foundation(), Surface::DryStone);
                if !round {
                    let k = (h / 0.45).floor() as usize;
                    for j in 0..k {
                        let y = floor + 0.35 + j as f32 * 0.45 + 0.2;
                        if y + 0.2 > e.y_hi {
                            break;
                        }
                        let axis = if j % 2 == 0 { e.d } else { prev.d };
                        b.bx(p3(corner, y) + inward * 0.12 + axis * if j % 2 == 0 { 0.14 } else { -0.14 }, Vec3::new(0.3, 0.2, 0.2), axis, look.stone, Surface::DryStone);
                    }
                }
                let count = (e.len * h * 0.25) as usize;
                for _ in 0..count {
                    let u = rng.range(0.3, (e.len - 0.3).max(0.31));
                    let y = rng.range(floor + 0.5, (e.y_hi - 0.2).max(floor + 0.51));
                    if !holes.iter().any(|o| o.edge == i && u > o.u0 - 0.25 && u < o.u1 + 0.25 && y > o.y0 - 0.15 && y < o.y1 + 0.15) {
                        b.bx(e.at(u, y, 0.03), Vec3::new(0.16, 0.08, 0.1), e.d, look.stone, Surface::DryStone);
                    }
                }
            }
        }
        if house.carved {
            let (ya, yb) = (e.y_hi - 0.45, e.y_hi - 0.27);
            strip(b, e, ya, yb, 0.04, &blocks(holes, i, ya, yb, false), look.trim, Surface::Carving);
        }
    }
    if house.material == Material::Stone {
        wall_lichen(b, edges, holes, look.w, &mut rng);
    }
}

/// Orange / yellow lichen clumps on stone faces, `0.35·w` coverage-ish.
fn wall_lichen(b: &mut B, edges: &[Edge], holes: &[Hole], w: f32, rng: &mut Rng) {
    if w <= 0.01 {
        return;
    }
    for (i, e) in edges.iter().enumerate() {
        let count = (w * e.len * (e.y_hi - e.y_lo) * 0.35) as usize;
        for _ in 0..count {
            let u = rng.range(0.15, (e.len - 0.15).max(0.16));
            let y = rng.range(e.y_lo + 0.4, (e.y_hi - 0.1).max(e.y_lo + 0.41));
            let s = rng.range(0.1, 0.22);
            let col = if rng.f() < 0.67 { palette::lichen() } else { palette::flower(2) };
            if holes.iter().any(|o| o.edge == i && u > o.u0 - s && u < o.u1 + s && y > o.y0 - s && y < o.y1 + s) {
                continue;
            }
            b.quad([e.at(u - s, y - s * 0.6, 0.03), e.at(u + s, y - s * 0.6, 0.03), e.at(u + s, y + s * 0.6, 0.03), e.at(u - s, y + s * 0.6, 0.03)], e.n, col, Surface::Petal);
        }
    }
}

// =============================================================================================
// Openings
// =============================================================================================

/// Panels, shutters, lintels and markers of every hole. Returns the index of the main door.
#[allow(clippy::too_many_arguments)]
fn openings(b: &mut B, edges: &[Edge], holes: &[Hole], look: &Look, carved: bool, plank_door: bool, floor: f32, thick: f32, stilts: bool) -> Option<usize> {
    let inset = -thick * 0.5;
    let full = [Vec2::new(0.0, 0.0), Vec2::new(0.999, 0.0), Vec2::new(0.999, 0.999), Vec2::new(0.0, 0.999)];
    let mut main = None;
    for (k, h) in holes.iter().enumerate() {
        let e = &edges[h.edge];
        let (u0, u1, y0, y1) = (h.u0, h.u1, h.y0, h.y1.min(e.y_hi));
        let (um, w, ym) = (h.mid(), u1 - u0, (y0 + y1) * 0.5);
        let panel = |b: &mut B, a: f32, c: f32, col: Vec3, s: Surface| {
            b.quad_uv([e.at(a, y0, inset), e.at(c, y0, inset), e.at(c, y1, inset), e.at(a, y1, inset)], full, e.n, col, s);
        };
        match h.kind {
            Hk::Window => {
                panel(b, u0, u1, look.trim, Surface::Window);
                // Shutters swung open flat against the wall, sill below.
                let sw = w * 0.5;
                for uc in [u0 - sw * 0.5 - 0.04, u1 + sw * 0.5 + 0.04] {
                    b.bx(e.at(uc, ym, 0.04), Vec3::new(sw * 0.5, (y1 - y0) * 0.5 + 0.03, 0.025), e.d, look.trim, Surface::Plank);
                }
                b.bx(e.at(um, y0 - 0.04, 0.06), Vec3::new(w * 0.5 + 0.1, 0.04, 0.09), e.d, look.wood, Surface::Timber);
                b.marker(MarkerKind::WindowLight, e.at(um, ym, 0.05), e.n);
            }
            Hk::Door | Hk::Wide => {
                if h.kind == Hk::Wide {
                    // Boat door: two plank leaves split by a painted stile.
                    panel(b, u0, um - 0.02, look.boards, Surface::Plank);
                    panel(b, um + 0.02, u1, look.boards, Surface::Plank);
                    b.bx(e.at(um, ym, inset + 0.03), Vec3::new(0.08, (y1 - y0) * 0.5, 0.03), e.d, look.trim, Surface::Carving);
                } else if plank_door {
                    panel(b, u0, u1, look.boards, Surface::Plank);
                    let yb = y0 + (y1 - y0) * 0.55;
                    b.quad([e.at(u0, yb, inset + 0.01), e.at(u1, yb, inset + 0.01), e.at(u1, yb + 0.14, inset + 0.01), e.at(u0, yb + 0.14, inset + 0.01)], e.n, look.trim, Surface::Plank);
                } else {
                    panel(b, u0, u1, look.hide, Surface::Door);
                }
                let fancy = carved || h.kind == Hk::Wide;
                let (lc, ls) = if fancy { (look.trim, Surface::Carving) } else { (look.wood, Surface::Timber) };
                if y1 + 0.18 <= e.y_hi + 0.05 {
                    b.bx(e.at(um, y1 + 0.09, 0.05), Vec3::new(w * 0.5 + 0.22, 0.09, 0.13), e.d, lc, ls);
                }
                if carved {
                    for uc in [u0 - 0.07, u1 + 0.07] {
                        b.bx(e.at(uc, ym, 0.05), Vec3::new(0.07, (y1 - y0) * 0.5, 0.07), e.d, look.trim, Surface::Carving);
                    }
                }
                b.marker(MarkerKind::Door, e.at(um, floor, if stilts { 0.2 } else { 0.7 }), e.n);
                main.get_or_insert(k);
            }
            Hk::Wind => {}
        }
    }
    main
}

/// Hanging lantern on a bracket beside the main door.
fn lantern(b: &mut B, e: &Edge, h: &Hole, look: &Look) {
    let mut u = h.u1 + 0.3;
    if u > e.len - 0.1 {
        u = h.u0 - 0.3;
    }
    if u < 0.1 {
        u = h.mid();
    }
    let y = (h.y1 + 0.15).min(e.y_hi - 0.2);
    b.bar(e.at(u, y, 0.0), e.at(u, y, 0.4), 0.06, look.wood, Surface::Timber);
    b.bx(e.at(u, y - 0.08, 0.38), Vec3::new(0.11, 0.04, 0.11), e.d, palette::metal(), Surface::Metal);
    let glass = e.at(u, y - 0.26, 0.38);
    b.bx(glass, Vec3::new(0.08, 0.13, 0.08), e.d, palette::glow(), Surface::Glow);
    b.marker(MarkerKind::Lantern, glass, e.n);
}

// =============================================================================================
// Hut
// =============================================================================================

#[allow(clippy::too_many_arguments)]
fn hut(b: &mut B, house: &House, poly: &[Vec2], c: Vec2, look: &Look, site: &Site, cuts: &[Cut], terrain: &Terrain, seed: u64) {
    let round = poly.len() > 4;
    let h = wall_h(house);
    let floor = site.floor;
    let y_lo = if site.stilts { floor - 0.05 } else { site.lo - SINK };
    let y_hi = floor + h;
    let thick = look.thick.min(inradius(poly, c) * 0.5);
    let inner = offset_poly(poly, -thick);
    let edges = make_edges(poly, poly, &inner, &inner, y_lo, y_hi);
    let lens: Vec<f32> = edges.iter().map(|e| e.len).collect();
    let holes = plan_openings(house, cuts, &lens, floor, h, round, false, seed, c);

    walls(b, &edges, &holes, look.wall_s, look.wall_c, true);
    // Earth floor inside (seen through the door).
    for [i, j, k] in triangulate(&inner) {
        b.tri(p3(inner[i], floor), p3(inner[j], floor), p3(inner[k], floor), Y, palette::dirt(), Surface::Dirt);
    }
    dress(b, house, &edges, &holes, look, floor, round, seed, c);
    let main = openings(b, &edges, &holes, look, house.carved, house.material == Material::Plank, floor, thick, site.stilts);
    if let Some(k) = main {
        lantern(b, &edges[holes[k].edge], &holes[k], look);
    }
    if site.stilts {
        let (ei, u) = main.map_or_else(|| (longest(&lens), lens[longest(&lens)] * 0.5), |k| (holes[k].edge, holes[k].mid()));
        stilts(b, poly, c, floor, terrain, &edges[ei], u, look);
    } else if let Some(k) = main {
        let (e, hl) = (&edges[holes[k].edge], &holes[k]);
        b.bx(e.at(hl.mid(), floor - 0.08, 0.35), Vec3::new((hl.u1 - hl.u0) * 0.5 + 0.12, 0.1, 0.3), e.d, look.stone, Surface::DryStone);
    }

    let ctx = RoofCtx::new(house.roof, house.roof_pitch, house.colors, look.w, house.carved, (look.wall_s, look.wall_c), seed, 0.0);
    let out = roof_impl(b, poly, y_hi, &ctx);
    b.marker(MarkerKind::ChimneySmoke, out.smoke, Y);
    for p in out.perches {
        b.marker(MarkerKind::Perch, p, Y);
    }
    if house.hangings {
        hangings(b, &edges, &holes, look, rect_frame(poly).map(|rf| rf.ax), seed, c);
    }
}

/// Plank deck on log posts down to the ground / seabed, bearers, braces and a ladder (or log
/// steps) at the door.
#[allow(clippy::too_many_arguments)]
fn stilts(b: &mut B, poly: &[Vec2], c: Vec2, floor: f32, terrain: &Terrain, door: &Edge, door_u: f32, look: &Look) {
    let deck = offset_poly(poly, DECK_OUT);
    slab(b, &deck, floor, DECK_T, look.boards, Surface::Boards, look.wood, Surface::Boards, false);
    let ring = offset_poly(poly, DECK_OUT - 0.15);
    let n = ring.len();
    let under = floor - DECK_T;
    let ground = |p: Vec2| fin(terrain.height(p), under - 1.0).min(under - 0.05);
    let mut posts = Vec::new();
    if n > 4 {
        let m = n.min(8);
        posts.extend((0..m).map(|k| ring[k * n / m]));
    } else {
        for i in 0..n {
            let (a, q) = (ring[i], ring[(i + 1) % n]);
            let k = ((a.distance(q) / 1.8).ceil() as usize).max(1);
            posts.extend((0..k).map(|j| a.lerp(q, j as f32 / k as f32)));
        }
    }
    let perimeter = posts.len();
    if inradius(poly, c) > 2.0 {
        posts.push(c);
    }
    for p in &posts {
        b.post(p3(*p, ground(*p) - 0.3), under, 0.13, look.wood, Surface::Timber);
    }
    for i in 0..n {
        b.bar(p3(ring[i], under - 0.08), p3(ring[(i + 1) % n], under - 0.08), 0.16, look.wood, Surface::Timber);
    }
    for k in 0..perimeter {
        let (pa, pq) = (posts[k], posts[(k + 1) % perimeter]);
        let gq = ground(pq);
        if under - gq > 1.2 {
            b.bar(p3(pa, under - 0.25), p3(pq, gq + 0.35), 0.07, look.drift, Surface::Timber);
        }
    }
    ladder(b, door, door_u, floor, terrain, look);
}

/// Ladder from the deck edge in front of the door down to the ground (or the water), or a few
/// log steps when the drop is small.
fn ladder(b: &mut B, e: &Edge, um: f32, floor: f32, terrain: &Terrain, look: &Look) {
    let top2 = e.at(um, floor, DECK_OUT - 0.05).xz();
    let (n2, d2) = (e.n.xz(), e.d.xz());
    let bottom_at = |p: Vec2| {
        let g = fin(terrain.height(p), floor - 1.0);
        terrain.water_level(p).filter(|l| l.is_finite()).map_or(g, |l| g.max(l - 0.4)).min(floor - 0.05)
    };
    let drop = floor - bottom_at(top2 + n2 * 0.8);
    if drop < 0.75 {
        let k = (drop / 0.25).ceil().max(1.0) as usize;
        let step = drop / k as f32;
        for j in 0..k {
            let y = floor - step * (j as f32 + 1.0);
            b.bx(p3(top2 + n2 * (0.2 + 0.3 * j as f32), y + step * 0.5 - 0.05), Vec3::new(0.5, step * 0.5 + 0.05, 0.15), e.d, look.wood, Surface::Timber);
        }
        return;
    }
    let run = (drop * 0.35).clamp(0.3, 1.6);
    let foot2 = top2 + n2 * run;
    let bottom = bottom_at(foot2);
    for side in [-0.25f32, 0.25] {
        let t = p3(top2 + d2 * side, floor + 0.7);
        let f = p3(foot2 + d2 * side + n2 * 0.0, bottom - 0.1);
        b.bar(f, t, 0.08, look.drift, Surface::Timber);
    }
    let rungs = ((floor - bottom) / 0.32).floor() as usize;
    for j in 1..=rungs {
        let t = j as f32 / (rungs as f32 + 1.0);
        let y = bottom + (floor - bottom) * t;
        let p = foot2.lerp(top2, (y - bottom + 0.1) / (floor + 0.8 - bottom));
        b.bar(p3(p - d2 * 0.29, y), p3(p + d2 * 0.29, y), 0.06, look.wood, Surface::Timber);
    }
}

/// Flat slab: top face, underside and edge faces.
#[allow(clippy::too_many_arguments)]
fn slab(b: &mut B, poly: &[Vec2], top: f32, thick: f32, top_c: Vec3, top_s: Surface, edge_c: Vec3, edge_s: Surface, record: bool) {
    let n = poly.len();
    if n < 3 {
        return;
    }
    let bot = top - thick;
    for [i, j, k] in triangulate(poly) {
        let (a, q, r) = (p3(poly[i], top), p3(poly[j], top), p3(poly[k], top));
        b.tri(a, q, r, Y, top_c, top_s);
        if record {
            b.tops.push([a, q, r]);
        }
        b.tri(p3(poly[i], bot), p3(poly[j], bot), p3(poly[k], bot), -Y, edge_c, edge_s);
    }
    for i in 0..n {
        let (a, q) = (poly[i], poly[(i + 1) % n]);
        let out = p3(outward((q - a).normalize_or_zero()), 0.0);
        b.quad([p3(a, bot), p3(q, bot), p3(q, top), p3(a, top)], out, edge_c, edge_s);
    }
}

/// Drying fish, net bundles with floats and shell strings hanging from a rail under the eaves
/// of the long sides (every third facet on round huts).
#[allow(clippy::too_many_arguments)]
fn hangings(b: &mut B, edges: &[Edge], holes: &[Hole], look: &Look, ax: Option<Vec2>, seed: u64, c: Vec2) {
    let mut rng = pick(seed, SALT_HANG, c);
    let fish = palette::fish();
    for (i, e) in edges.iter().enumerate() {
        let eave_side = match ax {
            Some(a) => e.d.xz().dot(a).abs() > 0.7,
            None => i % 3 == 0,
        };
        if !eave_side || e.len < 1.0 {
            continue;
        }
        let (y, out) = (e.y_hi - 0.08, 0.3);
        b.bar(e.at(0.15, y, out), e.at(e.len - 0.15, y, out), 0.06, look.drift, Surface::Timber);
        for u in [0.3, e.len - 0.3] {
            b.bar(e.at(u, y, 0.0), e.at(u, y, out), 0.05, look.drift, Surface::Timber);
        }
        let mut u = 0.45;
        let mut kind = (rng.next_u64() % 3) as u32;
        while u < e.len - 0.35 {
            let blocked = holes.iter().any(|h| h.edge == i && u > h.u0 - 0.2 && u < h.u1 + 0.2 && h.y1 > y - 1.0);
            if !blocked {
                let top = e.at(u, y, out);
                match kind {
                    0 => {
                        // Two drying fish on a string.
                        b.bar(top, top - Y * 0.15, 0.02, look.rope, Surface::Fiber);
                        for f in 0..2 {
                            let yf = y - 0.32 - 0.38 * f as f32;
                            let p = |du: f32, dy: f32| e.at(u + du, yf + dy, out);
                            for want in [e.n, -e.n] {
                                b.tri(p(0.0, 0.17), p(-0.07, 0.0), p(0.0, -0.12), want, fish, Surface::Hide);
                                b.tri(p(0.0, 0.17), p(0.0, -0.12), p(0.07, 0.0), want, fish, Surface::Hide);
                                b.tri(p(0.0, -0.12), p(-0.08, -0.22), p(0.08, -0.22), want, fish, Surface::Hide);
                            }
                        }
                    }
                    1 => {
                        // Net bundle with two painted floats.
                        b.bar(top, top - Y * 0.12, 0.02, look.rope, Surface::Fiber);
                        b.bx(top - Y * 0.38, Vec3::new(0.13, 0.25, 0.1), e.d, look.rope, Surface::Fiber);
                        for s in [-1.0, 1.0] {
                            b.bx(top - Y * (0.3 + 0.12 * s) + e.d * 0.14 * s + e.n * 0.04, Vec3::splat(0.055), e.d, look.trim, Surface::Carving);
                        }
                    }
                    _ => {
                        // Shell string: shells alternating shell white and a pigment.
                        b.bar(top, top - Y * 0.75, 0.02, look.rope, Surface::Fiber);
                        for k in 0..4 {
                            let col = if k % 2 == 0 { palette::shell() } else { palette::pigment(k + i as u32) };
                            b.bx(top - Y * (0.15 + 0.17 * k as f32), Vec3::splat(0.045), e.d, col, Surface::Shell);
                        }
                    }
                }
                kind = (kind + 1 + (rng.next_u64() % 2) as u32) % 3;
            }
            u += 0.5 + rng.f() * 0.25;
        }
    }
}

// =============================================================================================
// Beacon
// =============================================================================================

#[allow(clippy::too_many_arguments)]
fn beacon(b: &mut B, house: &House, poly: &[Vec2], c: Vec2, r: f32, look: &Look, site: &Site, cuts: &[Cut], seed: u64) {
    let h = wall_h(house);
    let base = site.lo;
    let top = base + h;
    let rt = r * BEACON_TAPER;
    let t = (r * 0.35).min(0.5).min(rt * 0.5);
    let scale = |k: f32| poly.iter().map(|p| c + (*p - c) * k).collect::<Vec<Vec2>>();
    let (top_poly, inner_b, inner_t) = (scale(BEACON_TAPER), scale((r - t) / r), scale((rt - t) / r));
    let edges = make_edges(poly, &top_poly, &inner_b, &inner_t, base - SINK, top);
    let lens: Vec<f32> = edges.iter().map(|e| e.len).collect();
    let holes = plan_openings(house, cuts, &lens, base, h, true, true, seed, c);
    let stone = palette::dry_stone().lerp(palette::wall(house.colors), 0.15);
    walls(b, &edges, &holes, Surface::DryStone, stone, true);
    for (i, e) in edges.iter().enumerate() {
        strip(b, e, e.y_lo, base + 0.4, 0.08, &blocks(&holes, i, e.y_lo, base + 0.4, true), palette::foundation(), Surface::DryStone);
        if house.carved {
            strip(b, e, top - 0.75, top - 0.5, 0.04, &blocks(&holes, i, top - 0.75, top - 0.5, false), look.trim, Surface::Carving);
        }
    }
    wall_lichen(b, &edges, &holes, look.w, &mut pick(seed, SALT_STONE, c));
    openings(b, &edges, &holes, look, house.carved, false, base, t, false);

    // Top platform with copestones round the rim.
    let plat = offset_poly(&top_poly, 0.3);
    let py = top + 0.25;
    slab(b, &plat, py, 0.35, look.stone, Surface::DryStone, palette::foundation(), Surface::DryStone, false);
    for (i, v) in plat.iter().enumerate().step_by(2) {
        let tang = (plat[(i + 1) % plat.len()] - *v).normalize_or(Vec2::X);
        b.bx(p3(c + (*v - c) * 0.92, py + 0.14), Vec3::new(0.2, 0.14, 0.13), p3(tang, 0.0), look.stone, Surface::DryStone);
    }

    // Iron fire basket on a stone plinth, with a few logs in it.
    let iron = palette::metal();
    b.m.cylinder(p3(c, py), 0.42, 0.36, 0.3, 6, true, false, palette::foundation(), Surface::DryStone, UV);
    let yb = py + 0.3;
    b.m.cylinder(p3(c, yb), 0.24, 0.24, 0.08, 6, true, false, iron, Surface::Metal, UV);
    for k in 0..6 {
        let d = Vec2::from_angle(k as f32 / 6.0 * TAU);
        b.bar(p3(c + d * 0.22, yb), p3(c + d * 0.5, yb + 0.75), 0.06, iron, Surface::Metal);
    }
    for f in [0.12f32, 0.92] {
        let (rr, y) = (0.22 + 0.28 * f, yb + 0.75 * f);
        for k in 0..8 {
            let (a0, a1) = (k as f32 / 8.0 * TAU, (k + 1) as f32 / 8.0 * TAU);
            b.bar(p3(c + Vec2::from_angle(a0) * rr, y), p3(c + Vec2::from_angle(a1) * rr, y), 0.05, iron, Surface::Metal);
        }
    }
    for a in [0.3f32, 1.9] {
        let d = Vec2::from_angle(a) * 0.3;
        b.bar(p3(c - d, yb + 0.12), p3(c + d, yb + 0.2), 0.1, look.wood, Surface::Timber);
    }
    b.marker(MarkerKind::Fire, p3(c, yb + 0.4), Y);

    if house.roof == RoofStyle::Thatch {
        // Small thatch cone on four posts above the fire.
        let ry = py + 2.2;
        let rad = (rt * 0.62).max(0.75);
        for k in 0..4 {
            let d = Vec2::from_angle(FRAC_PI_4 + k as f32 * FRAC_PI_2);
            b.post(p3(c + d * rad, py), ry, 0.08, look.wood, Surface::Timber);
        }
        let oct: Vec<Vec2> = (0..8).map(|k| c + Vec2::from_angle(k as f32 / 8.0 * TAU) * (rad + 0.08)).collect();
        let ctx = RoofCtx::new(RoofStyle::Thatch, house.roof_pitch, house.colors, look.w, house.carved, (Surface::DryStone, look.stone), seed, 0.0);
        let rise = 1.0f32.max(ctx.pitch * rad);
        let out = round_roof(b, &oct, c, ry, rise, &ctx);
        for p in out.perches {
            b.marker(MarkerKind::Perch, p, Y);
        }
    } else {
        b.marker(MarkerKind::Perch, p3(c + (plat[0] - c) * 0.92, py + 0.3), Y);
    }
}

// =============================================================================================
// Roofs
// =============================================================================================

/// Everything a roof builder needs.
#[derive(Clone, Copy, Debug)]
struct RoofCtx {
    style: RoofStyle,
    /// Effective pitch (style minimum applied).
    pitch: f32,
    overhang: f32,
    /// Roof swatch (bleached on thatch / hide).
    roof: Vec3,
    /// Thatch binding / trimmed eave.
    bind: Vec3,
    turf: Vec3,
    trim: Vec3,
    wood: Vec3,
    drift: Vec3,
    boards: Vec3,
    rope: Vec3,
    /// Gable-end infill material.
    gable: (Surface, Vec3),
    carved: bool,
    weathering: f32,
    seed: u64,
}

impl RoofCtx {
    #[allow(clippy::too_many_arguments)]
    fn new(style: RoofStyle, pitch: f32, colors: Colors, w: f32, carved: bool, gable: (Surface, Vec3), seed: u64, extra_overhang: f32) -> Self {
        let p = fin(pitch, 0.7).clamp(0.0, 3.0);
        // Thatch, hull and hide need some rise to exist; only turf may lie flat.
        let (pitch, overhang) = match style {
            RoofStyle::Thatch => (p.max(0.3), 0.5),
            RoofStyle::Turf => (p, 0.3),
            RoofStyle::Hide => (p.max(0.15), 0.35),
            RoofStyle::Boat => (p.max(0.25), 0.4),
        };
        let roof = palette::roof(colors);
        RoofCtx {
            style,
            pitch,
            overhang: overhang + extra_overhang,
            roof: if style == RoofStyle::Turf { roof } else { bleach(roof, w) },
            bind: bleach(roof.lerp(palette::wood(), 0.35), w),
            turf: palette::turf(),
            trim: palette::trim(colors),
            wood: bleach(palette::wood(), w),
            drift: bleach(palette::driftwood(), w),
            boards: bleach(palette::boards(), w),
            rope: palette::rope(),
            gable,
            carved,
            weathering: w,
            seed,
        }
    }
}

/// Where the roof's smoke hole is and where gulls may land.
struct RoofOut {
    smoke: Vec3,
    perches: Vec<Vec3>,
}

/// Oriented rectangle: centre, ridge axis, cross axis, half length (≥) and half width.
#[derive(Clone, Copy, Debug)]
struct Rf {
    c: Vec2,
    ax: Vec2,
    ac: Vec2,
    a: f32,
    b: f32,
}

impl Rf {
    fn w(&self, s: f32, t: f32, y: f32) -> Vec3 {
        p3(self.c + self.ax * s + self.ac * t, y)
    }
    fn ax3(&self) -> Vec3 {
        p3(self.ax, 0.0)
    }
    fn ac3(&self) -> Vec3 {
        p3(self.ac, 0.0)
    }
}

fn rect_frame(p: &[Vec2]) -> Option<Rf> {
    if p.len() != 4 {
        return None;
    }
    for i in 0..4 {
        let e0 = (p[(i + 1) % 4] - p[i]).normalize_or_zero();
        let e1 = (p[(i + 2) % 4] - p[(i + 1) % 4]).normalize_or_zero();
        if e0 == Vec2::ZERO || e1 == Vec2::ZERO || e0.dot(e1).abs() > 0.05 {
            return None;
        }
    }
    let c = p.iter().copied().sum::<Vec2>() * 0.25;
    let (e0, e1) = (p[1] - p[0], p[2] - p[1]);
    let (l0, l1) = (e0.length(), e1.length());
    Some(if l0 >= l1 - 1e-4 {
        Rf { c, ax: e0 / l0, ac: e1 / l1, a: l0 * 0.5, b: l1 * 0.5 }
    } else {
        Rf { c, ax: e1 / l1, ac: e0 / l0, a: l1 * 0.5, b: l0 * 0.5 }
    })
}

fn roof_impl(b: &mut B, poly: &[Vec2], base: f32, ctx: &RoofCtx) -> RoofOut {
    let poly = clean_poly(poly);
    if poly.len() < 3 {
        let c = poly.iter().copied().sum::<Vec2>() / poly.len().max(1) as f32;
        return RoofOut { smoke: p3(c, base + 0.5), perches: Vec::new() };
    }
    let c = area_centroid(&poly);
    let start = b.tops.len();
    let out = if ctx.pitch < 0.05 {
        flat_roof(b, &poly, c, base, ctx)
    } else if let Some(rf) = rect_frame(&poly) {
        let rise = ctx.pitch * rf.b;
        match ctx.style {
            RoofStyle::Thatch => hip_roof(b, &rf, base, rise, ctx),
            RoofStyle::Turf => turf_gable(b, &rf, base, rise, ctx),
            RoofStyle::Hide => hide_gable(b, &rf, base, rise, ctx),
            RoofStyle::Boat => boat_roof(b, &rf, base, rise, ctx),
        }
    } else if star_shaped(&poly, c) {
        let rise = ctx.pitch * inradius(&poly, c);
        round_roof(b, &poly, c, base, rise, ctx)
    } else {
        flat_roof(b, &poly, c, base, ctx)
    };
    let tops: Vec<[Vec3; 3]> = b.tops[start..].to_vec();
    let area: f32 = tops.iter().map(|t| (t[1] - t[0]).cross(t[2] - t[0]).length() * 0.5).sum();
    let mut rng = pick(ctx.seed, SALT_SCATTER, c);
    if ctx.style == RoofStyle::Turf {
        // Tiny flowers in the turf (never the reserved white).
        let count = ((area * 1.2) as usize).clamp(4, 60);
        scatter_on(b, &tops, count, &mut rng, 0.09, |r| palette::flower([0, 1, 2, 4, 5][(r.next_u64() % 5) as usize]));
    }
    if ctx.weathering > 0.01 {
        let count = ((area * 0.6 * ctx.weathering) as usize).min(40);
        scatter_on(b, &tops, count, &mut rng, 0.2, |r| if r.f() < 0.67 { palette::lichen() } else { palette::flower(2) });
    }
    out
}

/// Small flat triangles (flowers, lichen) on random points of the given top triangles.
fn scatter_on(b: &mut B, tris: &[[Vec3; 3]], count: usize, rng: &mut Rng, size: f32, col: impl Fn(&mut Rng) -> Vec3) {
    let areas: Vec<f32> = tris.iter().map(|t| (t[1] - t[0]).cross(t[2] - t[0]).length() * 0.5).collect();
    let total: f32 = areas.iter().sum();
    if tris.is_empty() || total.is_nan() || total <= 1e-4 {
        return;
    }
    for _ in 0..count {
        let mut x = rng.f() * total;
        let mut k = 0;
        while k + 1 < tris.len() && x > areas[k] {
            x -= areas[k];
            k += 1;
        }
        let t = tris[k];
        let (mut u, mut v) = (rng.f(), rng.f());
        if u + v > 1.0 {
            (u, v) = (1.0 - u, 1.0 - v);
        }
        let p = t[0] + (t[1] - t[0]) * u + (t[2] - t[0]) * v;
        let mut n = (t[1] - t[0]).cross(t[2] - t[0]).normalize_or(Y);
        if n.y < 0.0 {
            n = -n;
        }
        let (tu, tv) = tangent_frame(n);
        let a = rng.f() * TAU;
        let d1 = tu * a.cos() + tv * a.sin();
        let d2 = n.cross(d1);
        let q = p + n * 0.03;
        let cc = col(rng);
        b.tri(q + d1 * size, q - d1 * size * 0.5 + d2 * size * 0.87, q - d1 * size * 0.5 - d2 * size * 0.87, n, cc, Surface::Petal);
    }
}

/// Thick roof skin: each face (3 or 4 points, underside level) gets an upper surface lifted by
/// `t` (recorded as a top) and an underside facing down.
fn skin(b: &mut B, faces: &[Vec<Vec3>], t: f32, col: Vec3, s: Surface) {
    for f in faces {
        let up: Vec<Vec3> = f.iter().map(|p| *p + Y * t).collect();
        match f.len() {
            3 => {
                b.tri(up[0], up[1], up[2], Y, col, s);
                b.tri(f[0], f[1], f[2], -Y, col, s);
                b.tops.push([up[0], up[1], up[2]]);
            }
            4 => {
                b.quad([up[0], up[1], up[2], up[3]], Y, col, s);
                b.quad([f[0], f[1], f[2], f[3]], -Y, col, s);
                b.tops.push([up[0], up[1], up[2]]);
                b.tops.push([up[0], up[2], up[3]]);
            }
            _ => {}
        }
    }
}

/// Visible roof thickness along an eave / rake from `a` to `c`: segments ~0.5 m whose bottom
/// edge drops by up to `rag` (a slightly ragged thatch eave).
#[allow(clippy::too_many_arguments)]
fn fascia(b: &mut B, a: Vec3, c: Vec3, t: f32, out: Vec3, rag: f32, rng: &mut Rng, col: Vec3, s: Surface) {
    let k = ((a.distance(c) / 0.5).ceil() as usize).clamp(1, 200);
    let mut dp = 0.0;
    for i in 0..k {
        let p = a.lerp(c, i as f32 / k as f32);
        let q = a.lerp(c, (i + 1) as f32 / k as f32);
        let dq = if i + 1 == k || rag <= 0.0 { 0.0 } else { rng.range(0.0, rag) };
        b.quad([p - Y * dp, q - Y * dq, q + Y * t, p + Y * t], out, col, s);
        dp = dq;
    }
}

/// Crossed carved horns: two boards crossing at `at` in the plane across `along`, each running
/// down one rake and up past the crossing.
fn horns(b: &mut B, at: Vec3, along: Vec3, cross: Vec3, slope: f32, col: Vec3) {
    for sgn in [-1.0f32, 1.0] {
        let d = (cross * sgn - Y * slope.clamp(0.4, 2.0)).normalize_or(-Y);
        b.bar(at + d * 0.8 + along * 0.03 * sgn, at - d * 0.75 + along * 0.03 * sgn, 0.11, col, Surface::Carving);
    }
}

/// Gable-end infill triangles in the wall material at both ends of a rect roof.
fn gable_infill(b: &mut B, rf: &Rf, base: f32, rise: f32, gable: (Surface, Vec3)) {
    for sgn in [-1.0f32, 1.0] {
        let (p, q, r) = (rf.w(sgn * rf.a, -rf.b, base), rf.w(sgn * rf.a, rf.b, base), rf.w(sgn * rf.a, 0.0, base + rise));
        b.tri(p, q, r, rf.ax3() * sgn, gable.1, gable.0);
        b.tri(p, q, r, -rf.ax3() * sgn, gable.1, gable.0);
    }
}

/// Thick steep hipped thatch: visible thickness at a ragged eave, bound ridge roll.
fn hip_roof(b: &mut B, rf: &Rf, base: f32, rise: f32, ctx: &RoofCtx) -> RoofOut {
    const T: f32 = 0.35;
    let o = ctx.overhang;
    let (aa, bb) = (rf.a + o, rf.b + o);
    let drop = eave_drop(rise, rf.b, o);
    let (ey, ry) = (base - drop, base + rise);
    let r = (rf.a - rf.b * 0.85).max(0.0);
    let e = |s: f32, t: f32| rf.w(s, t, ey);
    let k = |s: f32| rf.w(s, 0.0, ry);
    let faces = vec![
        vec![e(-aa, -bb), e(aa, -bb), k(r), k(-r)],
        vec![e(aa, bb), e(-aa, bb), k(-r), k(r)],
        vec![e(aa, -bb), e(aa, bb), k(r)],
        vec![e(-aa, bb), e(-aa, -bb), k(-r)],
    ];
    skin(b, &faces, T, ctx.roof, Surface::Thatch);
    let (ax, ac) = (rf.ax3(), rf.ac3());
    let mut rng = pick(ctx.seed, SALT_EAVE, rf.c);
    for (p, q, out) in [(e(-aa, -bb), e(aa, -bb), -ac), (e(aa, -bb), e(aa, bb), ax), (e(aa, bb), e(-aa, bb), ac), (e(-aa, bb), e(-aa, -bb), -ax)] {
        fascia(b, p, q, T, out, 0.14, &mut rng, ctx.bind, Surface::Thatch);
    }
    // Bound ridge roll with rope bindings.
    let roll = k(0.0) + Y * (T - 0.02);
    let half = r + 0.3;
    b.bx(roll, Vec3::new(half, 0.15, 0.2), ax, ctx.bind, Surface::Thatch);
    let nb = ((2.0 * half) / 0.6).floor() as i32;
    for i in 0..=nb {
        let s = -half + 0.1 + (2.0 * half - 0.2) * i as f32 / nb.max(1) as f32;
        b.bx(roll + ax * s, Vec3::new(0.035, 0.17, 0.22), ax, ctx.rope, Surface::Fiber);
    }
    let slope = (rise + drop) / bb.max(0.1);
    if ctx.carved {
        for sgn in [-1.0f32, 1.0] {
            horns(b, k(sgn * (r + 0.2)) + Y * (T + 0.1), ax, ac, slope, ctx.trim);
        }
    }
    RoofOut { smoke: k(r * 0.5 + 0.1) + Y * (T + 0.35), perches: vec![k(-(r + 0.2)) + Y * (T + 0.18), k(r + 0.2) + Y * (T + 0.18)] }
}

/// Low grassy gable: turf top, sod edge strips (roof swatch), ridge binding with tufts.
fn turf_gable(b: &mut B, rf: &Rf, base: f32, rise: f32, ctx: &RoofCtx) -> RoofOut {
    const T: f32 = 0.3;
    let o = ctx.overhang;
    let (aa, bb) = (rf.a + o * 0.7, rf.b + o);
    let drop = eave_drop(rise, rf.b, o);
    let (ey, ry) = (base - drop, base + rise);
    let e = |s: f32, t: f32| rf.w(s, t, ey);
    let k = |s: f32| rf.w(s, 0.0, ry);
    let faces = vec![vec![e(-aa, -bb), e(aa, -bb), k(aa), k(-aa)], vec![e(aa, bb), e(-aa, bb), k(-aa), k(aa)]];
    skin(b, &faces, T, ctx.turf, Surface::Turf);
    let (ax, ac) = (rf.ax3(), rf.ac3());
    let mut rng = pick(ctx.seed, SALT_EAVE, rf.c);
    let sod = ctx.roof;
    fascia(b, e(-aa, -bb), e(aa, -bb), T, -ac, 0.0, &mut rng, sod, Surface::Turf);
    fascia(b, e(aa, bb), e(-aa, bb), T, ac, 0.0, &mut rng, sod, Surface::Turf);
    for sgn in [-1.0f32, 1.0] {
        fascia(b, e(sgn * aa, -bb), k(sgn * aa), T, ax * sgn, 0.0, &mut rng, sod, Surface::Turf);
        fascia(b, k(sgn * aa), e(sgn * aa, bb), T, ax * sgn, 0.0, &mut rng, sod, Surface::Turf);
    }
    gable_infill(b, rf, base, rise, ctx.gable);
    b.bx(k(0.0) + Y * (T + 0.06), Vec3::new(aa, 0.08, 0.16), ax, sod, Surface::Turf);
    let nt = (2.0 * aa / 0.6).floor() as i32;
    for i in 0..nt {
        let s = -aa + 0.3 + 0.6 * i as f32;
        b.m.cylinder(k(s) + Y * (T + 0.12), 0.14, 0.0, 0.2, 4, false, false, ctx.turf, Surface::Turf, UV);
    }
    // Smoke hole: a little timber frame in the turf near the ridge.
    let hole = rf.w(aa * 0.45, 0.0, ry + T - 0.02);
    b.bx(hole + Y * 0.08, Vec3::new(0.25, 0.08, 0.25), ax, ctx.wood, Surface::Timber);
    if ctx.carved {
        for sgn in [-1.0f32, 1.0] {
            horns(b, k(sgn * aa) + Y * (T + 0.05), ax, ac, (rise + drop) / bb.max(0.1), ctx.trim);
        }
    }
    RoofOut { smoke: hole + Y * 0.4, perches: vec![k(-aa) + Y * (T + 0.15), k(aa) + Y * (T + 0.15)] }
}

/// Hide stretched over rafter poles: sags between the poles, poles poke out past the ridge,
/// a painted zigzag hem along the eaves, lashings at the ridge.
fn hide_gable(b: &mut B, rf: &Rf, base: f32, rise: f32, ctx: &RoofCtx) -> RoofOut {
    const T: f32 = 0.05;
    const SAG: f32 = 0.07;
    let o = ctx.overhang;
    let (aa, bb) = (rf.a + o * 0.5, rf.b + o);
    let drop = eave_drop(rise, rf.b, o);
    let (ey, ry) = (base - drop, base + rise);
    let n = ((2.0 * aa / 1.3).ceil() as usize).clamp(2, 40);
    let st: Vec<f32> = (0..=n).map(|k| -aa + 2.0 * aa * k as f32 / n as f32).collect();
    let mut faces = Vec::new();
    for side in [-1.0f32, 1.0] {
        let pe = |s: f32, dy: f32| rf.w(s, side * bb, ey - dy);
        let pr = |s: f32, dy: f32| rf.w(s, 0.0, ry - dy);
        for k in 0..n {
            let (s0, s1) = (st[k], st[k + 1]);
            let sm = (s0 + s1) * 0.5;
            faces.push(vec![pe(s0, 0.0), pe(sm, SAG), pr(sm, SAG), pr(s0, 0.0)]);
            faces.push(vec![pe(sm, SAG), pe(s1, 0.0), pr(s1, 0.0), pr(sm, SAG)]);
        }
        // Painted zigzag hem.
        let teeth = ((2.0 * aa) / 0.45).ceil() as usize;
        for j in 0..teeth {
            let (sa, sb) = (-aa + 2.0 * aa * j as f32 / teeth as f32, -aa + 2.0 * aa * (j + 1) as f32 / teeth as f32);
            let at = |s: f32, f: f32| pe(s, 0.0).lerp(pr(s, 0.0), f) + Y * 0.09;
            b.tri(at(sa, 0.04), at(sb, 0.04), at((sa + sb) * 0.5, 0.2), Y, ctx.trim, Surface::Hide);
        }
    }
    skin(b, &faces, T, ctx.roof, Surface::Hide);
    gable_infill(b, rf, base, rise, (Surface::Hide, ctx.roof));
    let (ax, ac) = (rf.ax3(), rf.ac3());
    let k = |s: f32| rf.w(s, 0.0, ry);
    b.bar(k(-aa - 0.5) + Y * 0.06, k(aa + 0.5) + Y * 0.06, 0.15, ctx.drift, Surface::Timber);
    for &s in &st {
        for side in [-1.0f32, 1.0] {
            let e0 = rf.w(s, side * (bb + 0.05), ey - 0.06);
            let r0 = k(s) + Y * 0.06;
            let d = (r0 - e0).normalize_or(Y);
            b.bar(e0, r0 + d * 0.5, 0.09, ctx.drift, Surface::Timber);
        }
        b.bx(k(s) + Y * 0.08, Vec3::splat(0.1), ax, ctx.rope, Surface::Fiber);
    }
    if ctx.carved {
        for sgn in [-1.0f32, 1.0] {
            horns(b, k(sgn * (aa + 0.2)) + Y * 0.12, ax, ac, (rise + drop) / bb.max(0.1), ctx.trim);
        }
    }
    RoofOut { smoke: k(aa * 0.55) + Y * 0.35, perches: vec![k(-(aa + 0.4)) + Y * 0.15, k(aa + 0.4) + Y * 0.15] }
}

/// Upturned hull: strakes along a curved section, keel rising at both ends, gunwales, carved
/// stems, gable ends filled under the hull curve.
fn boat_roof(b: &mut B, rf: &Rf, base: f32, rise: f32, ctx: &RoofCtx) -> RoofOut {
    const K: usize = 4;
    let o = ctx.overhang;
    let (ll, bw) = (rf.a + o, rf.b + o);
    let drop = eave_drop(rise, rf.b, o);
    let ey = base - drop;
    let h0 = base + rise - ey;
    let end = 0.3 + 0.3 * rise;
    let hs = |s: f32| h0 + end * (s / ll) * (s / ll);
    let hull = |s: f32, f: f32, side: f32| {
        let a = f * FRAC_PI_2;
        rf.w(s, side * bw * a.cos(), ey + hs(s) * a.sin())
    };
    let ns = ((2.0 * ll / 0.8).ceil() as usize).clamp(6, 60);
    let st: Vec<f32> = (0..=ns).map(|k| -ll + 2.0 * ll * k as f32 / ns as f32).collect();
    let (ax, ac) = (rf.ax3(), rf.ac3());
    for side in [-1.0f32, 1.0] {
        for k in 0..K {
            let (f0, f1) = (k as f32 / K as f32, (k + 1) as f32 / K as f32);
            let fm = (f0 + f1) * 0.5 * FRAC_PI_2;
            let want = ac * side * fm.cos() + Y * (fm.sin() + 0.2);
            let col = if k % 2 == 0 { ctx.boards } else { ctx.roof };
            for i in 0..ns {
                let p = [hull(st[i], f0, side), hull(st[i + 1], f0, side), hull(st[i + 1], f1, side), hull(st[i], f1, side)];
                b.quad(p, want, col, Surface::Boards);
                b.quad(p, -want, col, Surface::Boards);
                b.tops.push([p[0], p[1], p[2]]);
                b.tops.push([p[0], p[2], p[3]]);
            }
        }
        b.bar(hull(-ll, 0.0, side), hull(ll, 0.0, side), 0.14, ctx.wood, Surface::Timber);
    }
    for i in 0..ns {
        b.bar(hull(st[i], 1.0, 1.0) + Y * 0.05, hull(st[i + 1], 1.0, 1.0) + Y * 0.05, 0.16, ctx.wood, Surface::Timber);
    }
    let mut perches = Vec::new();
    for sgn in [-1.0f32, 1.0] {
        let p = hull(sgn * ll, 1.0, 1.0);
        let tip = p + ax * sgn * 0.3 + Y * 0.6;
        b.bar(p, tip, 0.14, ctx.trim, Surface::Carving);
        perches.push(tip + Y * 0.1);
        if ctx.carved {
            horns(b, p + Y * 0.15, ax, ac, 1.0, ctx.trim);
        }
        // Gable end under the hull curve.
        let s = sgn * rf.a;
        let hh = hs(s);
        let mid = rf.w(s, 0.0, base);
        let mut pts = vec![rf.w(s, -rf.b, base)];
        for j in 0..=6 {
            let lat = -rf.b + 2.0 * rf.b * j as f32 / 6.0;
            let q = (1.0 - (lat / bw) * (lat / bw)).max(0.0).sqrt();
            pts.push(rf.w(s, lat, (ey + hh * q).max(base)));
        }
        pts.push(rf.w(s, rf.b, base));
        for w in pts.windows(2) {
            b.tri(mid, w[0], w[1], ax * sgn, ctx.gable.1, ctx.gable.0);
            b.tri(mid, w[0], w[1], -ax * sgn, ctx.gable.1, ctx.gable.0);
        }
    }
    RoofOut { smoke: hull(rf.a * 0.5, 1.0, 1.0) + Y * 0.35, perches }
}

/// Round / star-shaped roofs: thatch cone with a bound topknot (Thatch, Boat), low turf dome
/// (Turf), conical hide tent with poles through the apex (Hide).
fn round_roof(b: &mut B, poly: &[Vec2], c: Vec2, base: f32, rise: f32, ctx: &RoofCtx) -> RoofOut {
    let n = poly.len();
    let rin = inradius(poly, c);
    let o = ctx.overhang;
    let drop = eave_drop(rise, rin, o);
    let ey = base - drop;
    let apex = p3(c, base + rise);
    let mut rng = pick(ctx.seed, SALT_EAVE, c);
    let mut faces = Vec::new();
    match ctx.style {
        RoofStyle::Thatch | RoofStyle::Boat => {
            const T: f32 = 0.35;
            let eave = offset_poly(poly, o);
            for i in 0..n {
                faces.push(vec![p3(eave[i], ey), p3(eave[(i + 1) % n], ey), apex]);
            }
            skin(b, &faces, T, ctx.roof, Surface::Thatch);
            for i in 0..n {
                let (p, q) = (eave[i], eave[(i + 1) % n]);
                fascia(b, p3(p, ey), p3(q, ey), T, p3(outward((q - p).normalize_or_zero()), 0.0), 0.14, &mut rng, ctx.bind, Surface::Thatch);
            }
            // Bound topknot.
            let top = apex + Y * (T - 0.15);
            b.m.cylinder(top, 0.3, 0.26, 0.35, 6, false, false, ctx.bind, Surface::Thatch, UV);
            b.m.cylinder(top + Y * 0.12, 0.32, 0.32, 0.08, 6, false, false, ctx.rope, Surface::Fiber, UV);
            b.m.cylinder(top + Y * 0.35, 0.38, 0.0, 0.42, 6, false, true, ctx.roof, Surface::Thatch, UV);
            let tip = top + Y * 0.77;
            if ctx.carved {
                b.post(tip - Y * 0.1, tip.y + 0.7, 0.06, ctx.trim, Surface::Carving);
                horns(b, tip + Y * 0.45, Vec3::X, Vec3::Z, 1.0, ctx.trim);
            }
            RoofOut { smoke: tip + Y * 0.1, perches: vec![tip + Y * 0.05] }
        }
        RoofStyle::Turf => {
            const T: f32 = 0.28;
            let eave = offset_poly(poly, o);
            let h = base + rise - ey;
            let ring = |j: usize, i: usize| {
                let a = j as f32 / 3.0 * FRAC_PI_2;
                p3(c + (eave[i % n] - c) * a.cos(), ey + h * a.sin())
            };
            for j in 0..3 {
                for i in 0..n {
                    if j < 2 {
                        faces.push(vec![ring(j, i), ring(j, i + 1), ring(j + 1, i + 1), ring(j + 1, i)]);
                    } else {
                        faces.push(vec![ring(j, i), ring(j, i + 1), apex]);
                    }
                }
            }
            skin(b, &faces, T, ctx.turf, Surface::Turf);
            for i in 0..n {
                let (p, q) = (eave[i], eave[(i + 1) % n]);
                fascia(b, p3(p, ey), p3(q, ey), T, p3(outward((q - p).normalize_or_zero()), 0.0), 0.0, &mut rng, ctx.roof, Surface::Turf);
            }
            let hole = apex + Y * (T - 0.06);
            b.m.cylinder(hole, 0.32, 0.3, 0.16, 6, false, false, ctx.wood, Surface::Timber, UV);
            if ctx.carved {
                b.post(hole + p3(Vec2::new(0.4, 0.0), 0.0), hole.y + 0.9, 0.06, ctx.trim, Surface::Carving);
            }
            RoofOut { smoke: hole + Y * 0.4, perches: vec![hole + Y * 0.2] }
        }
        RoofStyle::Hide => {
            const T: f32 = 0.04;
            let eave = offset_poly(poly, o * 0.6);
            for i in 0..n {
                faces.push(vec![p3(eave[i], ey), p3(eave[(i + 1) % n], ey), apex]);
                // Painted zigzag hem near the eave.
                let (p, q) = (p3(eave[i], ey), p3(eave[(i + 1) % n], ey));
                let teeth = ((p.distance(q) / 0.45).ceil() as usize).max(1);
                for j in 0..teeth {
                    let at = |f: f32, up: f32| p.lerp(q, f).lerp(apex, up) + Y * (T + 0.03);
                    let (fa, fb) = (j as f32 / teeth as f32, (j + 1) as f32 / teeth as f32);
                    b.tri(at(fa, 0.04), at(fb, 0.04), at((fa + fb) * 0.5, 0.2), Y, ctx.trim, Surface::Hide);
                }
            }
            skin(b, &faces, T, ctx.roof, Surface::Hide);
            let step = (n / 8).max(1);
            let mut tips = Vec::new();
            for i in (0..n).step_by(step) {
                let e0 = p3(eave[i], ey - 0.05);
                let d = (apex - e0).normalize_or(Y);
                let tip = apex + d * 0.7;
                b.bar(e0, tip, 0.08, ctx.drift, Surface::Timber);
                tips.push(tip);
            }
            b.bx(apex + Y * 0.06, Vec3::new(0.12, 0.1, 0.12), Vec3::X, ctx.rope, Surface::Fiber);
            if ctx.carved {
                horns(b, apex + Y * 0.3, Vec3::X, Vec3::Z, 1.0, ctx.trim);
            }
            let perch = tips.iter().copied().fold(apex, |a, t| if t.y > a.y { t } else { a });
            RoofOut { smoke: apex + Y * 0.5, perches: vec![perch + Y * 0.05] }
        }
    }
}

/// Flat roof (flat turf, concave rings): a slab of the style's covering with an edge strip.
fn flat_roof(b: &mut B, poly: &[Vec2], c: Vec2, base: f32, ctx: &RoofCtx) -> RoofOut {
    const T: f32 = 0.3;
    let p = offset_poly(poly, ctx.overhang * 0.5);
    let (top_s, top_c, edge_c) = match ctx.style {
        RoofStyle::Thatch => (Surface::Thatch, ctx.roof, ctx.bind),
        RoofStyle::Turf => (Surface::Turf, ctx.turf, ctx.roof),
        RoofStyle::Hide => (Surface::Hide, ctx.roof, ctx.trim),
        RoofStyle::Boat => (Surface::Boards, ctx.roof, ctx.wood),
    };
    slab(b, &p, base + T, T, top_c, top_s, edge_c, top_s, true);
    let hole = p3(c, base + T);
    b.bx(hole + Y * 0.08, Vec3::new(0.25, 0.08, 0.25), Vec3::X, ctx.wood, Surface::Timber);
    RoofOut { smoke: hole + Y * 0.4, perches: vec![p3(p[0], base + T + 0.1)] }
}

// =============================================================================================
// Tests
// =============================================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Opening, TerrainDab, TerrainOp};

    /// Level dry ground 1 m above the sea across the whole island, so geometry tests do not
    /// depend on the island layout.
    fn ground() -> Terrain {
        let poly = vec![Vec2::new(-40.0, -40.0), Vec2::new(40.0, -40.0), Vec2::new(40.0, 40.0), Vec2::new(-40.0, 40.0)];
        Terrain::build(1, &[], &[crate::terrain::FlattenRegion { poly, height: Some(1.0), margin: 1.0, keep: false }])
    }

    fn rect(hx: f32, hz: f32) -> Footprint {
        Footprint::Rect { center: Vec2::new(2.0, -3.0), half: Vec2::new(hx, hz), angle: 0.4 }
    }

    fn round(r: f32) -> Footprint {
        Footprint::Round { center: Vec2::new(-4.0, 1.0), radius: r }
    }

    fn house(fp: Footprint, h: f32, roof: RoofStyle, material: Material) -> House {
        House {
            footprint: fp,
            wall_height: h,
            roof,
            roof_pitch: 0.8,
            material,
            carved: false,
            hangings: false,
            stilts: 0.0,
            openings: vec![],
            colors: Colors { roof: 0, wall: 1, trim: 2 },
            weathering: 0.0,
        }
    }

    fn count(mk: &[Marker], k: MarkerKind) -> usize {
        mk.iter().filter(|m| m.kind == k).count()
    }

    fn has(m: &MeshData, s: Surface) -> bool {
        m.surfaces.contains(&(s as u8))
    }

    fn check(m: &MeshData, mk: &[Marker]) {
        assert!(m.is_valid(), "invalid mesh");
        assert!(!m.is_empty(), "empty mesh");
        assert!(m.colors.iter().flatten().all(|v| v.is_finite()));
        let (lo, hi) = m.bounds().unwrap();
        assert!(lo.y >= -12.0 && hi.y < 60.0, "bounds {lo:?} {hi:?}");
        assert!(mk.iter().all(|k| k.pos.is_finite() && k.dir.is_finite()));
    }

    const ROOFS: [RoofStyle; 4] = [RoofStyle::Thatch, RoofStyle::Turf, RoofStyle::Hide, RoofStyle::Boat];
    const MATS: [Material; 3] = [Material::Wattle, Material::Plank, Material::Stone];

    /// Terrain with a deep hole sunk below sea level around the origin.
    fn sea() -> Terrain {
        Terrain::build(1, &[TerrainDab { center: Vec2::ZERO, radius: 14.0, op: TerrainOp::Sculpt { amount: -12.0 } }], &[])
    }

    #[test]
    fn every_combination_is_valid() {
        let t = ground();
        let fps = [rect(3.0, 2.0), rect(0.6, 0.6), round(1.8), round(3.0), round(0.8)];
        let mut rc = 0u8;
        for fp in fps {
            for h in [0.6, 2.4, 5.0, 9.0] {
                for roof in ROOFS {
                    for mat in MATS {
                        for flags in 0..4 {
                            for pitch in [0.0, 0.8, 2.5] {
                                let mut hs = house(fp, h, roof, mat);
                                hs.carved = flags & 1 != 0;
                                hs.hangings = flags & 2 != 0;
                                hs.weathering = if flags == 3 { 1.0 } else { 0.0 };
                                hs.stilts = if flags == 1 { 1.5 } else { 0.0 };
                                hs.roof_pitch = pitch;
                                rc = (rc + 1) % crate::model::ROOF_COLORS;
                                hs.colors.roof = rc;
                                let (m, mk) = house_mesh(&hs, &[], &t, 9);
                                check(&m, &mk);
                            }
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn deterministic() {
        let t = ground();
        let mut hs = house(rect(3.0, 2.0), 3.0, RoofStyle::Turf, Material::Stone);
        hs.hangings = true;
        hs.weathering = 0.7;
        hs.carved = true;
        assert_eq!(house_mesh(&hs, &[], &t, 5), house_mesh(&hs, &[], &t, 5));
        let rh = house(round(2.5), 2.5, RoofStyle::Hide, Material::Wattle);
        assert_eq!(house_mesh(&rh, &[], &t, 5), house_mesh(&rh, &[], &t, 5));
        let ring = footprint_polygon(&round(3.0));
        assert_eq!(roof_mesh(&ring, 2.0, RoofStyle::Turf, 0.5, Colors::default(), 3), roof_mesh(&ring, 2.0, RoofStyle::Turf, 0.5, Colors::default(), 3));
    }

    #[test]
    fn auto_details_door_on_longest_side() {
        let t = ground();
        let fp = Footprint::Rect { center: Vec2::new(1.0, 2.0), half: Vec2::new(3.0, 1.5), angle: 0.0 };
        for seed in 0..6 {
            let (m, mk) = house_mesh(&house(fp, 2.8, RoofStyle::Thatch, Material::Wattle), &[], &t, seed);
            check(&m, &mk);
            assert_eq!(count(&mk, MarkerKind::Door), 1);
            assert_eq!(count(&mk, MarkerKind::Lantern), 1);
            assert!((1..=3).contains(&count(&mk, MarkerKind::WindowLight)), "1-3 windows");
            assert_eq!(count(&mk, MarkerKind::ChimneySmoke), 1);
            assert!(count(&mk, MarkerKind::Perch) >= 1);
            let door = mk.iter().find(|k| k.kind == MarkerKind::Door).unwrap();
            assert!(door.dir.z.abs() > 0.99, "door faces off a long side: {:?}", door.dir);
            assert!(has(&m, Surface::Door) && has(&m, Surface::Window) && has(&m, Surface::Glow) && has(&m, Surface::Daub) && has(&m, Surface::Timber));
            let smoke = mk.iter().find(|k| k.kind == MarkerKind::ChimneySmoke).unwrap();
            assert!(smoke.pos.y > t.height(Vec2::new(1.0, 2.0)) + 2.8 + 0.5, "smoke above the roof");
        }
        // Plank huts get a plank door instead of a hide curtain.
        let (m, _) = house_mesh(&house(fp, 2.8, RoofStyle::Thatch, Material::Plank), &[], &t, 1);
        assert!(!has(&m, Surface::Door) && has(&m, Surface::Plank));
    }

    #[test]
    fn manual_openings_cycle_kinds() {
        let t = ground();
        let mut hs = house(Footprint::Rect { center: Vec2::ZERO, half: Vec2::new(4.0, 2.5), angle: 0.0 }, 3.2, RoofStyle::Boat, Material::Stone);
        hs.openings = vec![
            Opening { along: 2.0, kind: OpeningKind::Window },
            Opening { along: 6.0, kind: OpeningKind::Door },
            Opening { along: 13.0, kind: OpeningKind::WideDoor },
        ];
        let (m, mk) = house_mesh(&hs, &[], &t, 2);
        check(&m, &mk);
        assert_eq!(count(&mk, MarkerKind::WindowLight), 1, "no automatic windows");
        assert_eq!(count(&mk, MarkerKind::Door), 2);
        assert_eq!(count(&mk, MarkerKind::Lantern), 1);
        assert!(has(&m, Surface::Door), "hide curtain doorway");
        assert!(has(&m, Surface::Carving), "boat door is painted");
        assert!(has(&m, Surface::Window));
    }

    #[test]
    fn cut_becomes_door() {
        let t = ground();
        let hs = house(Footprint::Rect { center: Vec2::ZERO, half: Vec2::new(3.0, 2.0), angle: 0.0 }, 3.0, RoofStyle::Hide, Material::Plank);
        let poly = footprint_polygon(&hs.footprint);
        let cut = Cut { along: 13.0, width: 1.6, height: 2.4 };
        let (m, mk) = house_mesh(&hs, &[cut], &t, 1);
        check(&m, &mk);
        assert_eq!(count(&mk, MarkerKind::Door), 1, "the path's doorway replaces the automatic door");
        let (along, _) = crate::geom::closest_on_polyline(&crate::geom::closed(&poly), mk.iter().find(|k| k.kind == MarkerKind::Door).unwrap().pos.xz());
        assert!((along - 13.0).abs() < 0.3, "door at the cut: {along}");
    }

    #[test]
    fn roof_styles_are_distinct() {
        let t = ground();
        let fp = rect(3.0, 2.0);
        let surf = |roof: RoofStyle, fp: Footprint| house_mesh(&house(fp, 2.5, roof, Material::Stone), &[], &t, 4).0;
        let th = surf(RoofStyle::Thatch, fp);
        assert!(has(&th, Surface::Thatch) && has(&th, Surface::Fiber), "thatch with bound ridge");
        let tu = surf(RoofStyle::Turf, fp);
        assert!(has(&tu, Surface::Turf) && has(&tu, Surface::Petal), "turf with flowers");
        assert!(!has(&tu, Surface::Thatch));
        let hi = surf(RoofStyle::Hide, fp);
        assert!(has(&hi, Surface::Hide) && has(&hi, Surface::Timber));
        let bo = surf(RoofStyle::Boat, fp);
        assert!(has(&bo, Surface::Boards) && has(&bo, Surface::Carving), "hull strakes and carved stems");
        // Round: boat falls back to a thatch cone; turf dome; hide tent.
        let rb = surf(RoofStyle::Boat, round(2.5));
        assert!(has(&rb, Surface::Thatch) && !has(&rb, Surface::Boards));
        assert!(has(&surf(RoofStyle::Turf, round(2.5)), Surface::Turf));
        assert!(has(&surf(RoofStyle::Hide, round(2.5)), Surface::Hide));
        // Boat strakes alternate the roof swatch and boards.
        let mut hs = house(fp, 2.5, RoofStyle::Boat, Material::Stone);
        hs.colors.roof = 3;
        let (m, _) = house_mesh(&hs, &[], &t, 4);
        let swatch = palette::roof(hs.colors);
        let boards = palette::boards();
        let strake = |c: Vec3| m.colors.iter().zip(&m.surfaces).any(|(k, s)| *s == Surface::Boards as u8 && (Vec3::new(k[0], k[1], k[2]) - c).length() < 1e-4);
        assert!(strake(swatch) && strake(boards));
    }

    #[test]
    fn pitch_is_honoured() {
        let t = ground();
        for roof in ROOFS {
            for fp in [rect(3.0, 2.0), round(2.5)] {
                let top = |p: f32| {
                    let mut hs = house(fp, 2.5, roof, Material::Wattle);
                    hs.roof_pitch = p;
                    house_mesh(&hs, &[], &t, 1).0.bounds().unwrap().1.y
                };
                assert!(top(1.6) > top(0.6) + 0.5, "{roof:?} {fp:?}");
            }
        }
    }

    #[test]
    fn carved_and_hangings_add_detail() {
        let t = ground();
        for roof in ROOFS {
            for fp in [rect(3.0, 2.0), round(2.2)] {
                let plain = house(fp, 2.6, roof, Material::Wattle);
                let mut carved = plain.clone();
                carved.carved = true;
                let (pm, _) = house_mesh(&plain, &[], &t, 1);
                let (cm, _) = house_mesh(&carved, &[], &t, 1);
                assert!(!has(&pm, Surface::Carving) || roof == RoofStyle::Boat, "{roof:?}");
                assert!(has(&cm, Surface::Carving) && cm.vertex_count() > pm.vertex_count(), "{roof:?} {fp:?}");
                let mut hung = plain.clone();
                hung.hangings = true;
                let (hm, _) = house_mesh(&hung, &[], &t, 1);
                assert!(hm.vertex_count() > pm.vertex_count() + 50, "{roof:?} {fp:?}");
                assert!(has(&hm, Surface::Fiber));
            }
        }
    }

    #[test]
    fn materials() {
        let t = ground();
        let fp = rect(3.0, 2.0);
        let (w, _) = house_mesh(&house(fp, 2.5, RoofStyle::Thatch, Material::Wattle), &[], &t, 1);
        assert!(has(&w, Surface::Daub) && has(&w, Surface::Timber));
        let (p, _) = house_mesh(&house(fp, 2.5, RoofStyle::Thatch, Material::Plank), &[], &t, 1);
        assert!(has(&p, Surface::Plank) && !has(&p, Surface::Daub));
        // Painted bands in the trim colour.
        let trim = palette::trim(Colors { roof: 0, wall: 1, trim: 2 });
        assert!(p.colors.iter().zip(&p.surfaces).any(|(k, s)| *s == Surface::Plank as u8 && (Vec3::new(k[0], k[1], k[2]) - trim).length() < 1e-4));
        let mut st = house(fp, 2.5, RoofStyle::Thatch, Material::Stone);
        st.weathering = 1.0;
        let (s, _) = house_mesh(&st, &[], &t, 1);
        assert!(has(&s, Surface::DryStone) && !has(&s, Surface::Daub));
        assert!(has(&s, Surface::Petal), "lichen on weathered stone");
    }

    #[test]
    fn stilts_over_the_sea() {
        let t = sea();
        let c = Vec2::ZERO;
        assert!(t.is_sea(c));
        let hs = house(Footprint::Rect { center: c, half: Vec2::new(2.5, 2.0), angle: 0.3 }, 2.5, RoofStyle::Thatch, Material::Plank);
        assert!(needs_stilts(&hs, &t));
        let (m, mk) = house_mesh(&hs, &[], &t, 1);
        check(&m, &mk);
        let door = mk.iter().find(|k| k.kind == MarkerKind::Door).unwrap();
        assert!(door.pos.y >= SEA_LEVEL + WATER_CLEAR - 1e-3, "floor clears the water: {}", door.pos.y);
        // Posts reach down to the seabed, the deck is boards.
        let (lo, _) = m.bounds().unwrap();
        assert!(lo.y < t.height(c) + 0.5, "posts reach the seabed: {} vs {}", lo.y, t.height(c));
        assert!(has(&m, Surface::Boards) && has(&m, Surface::Timber));
        // Round huts too.
        let rh = house(Footprint::Round { center: c, radius: 2.0 }, 2.0, RoofStyle::Hide, Material::Wattle);
        assert!(needs_stilts(&rh, &t));
        check(&house_mesh(&rh, &[], &t, 1).0, &house_mesh(&rh, &[], &t, 1).1);
    }

    #[test]
    fn stilts_over_a_lagoon_and_steep_ground() {
        let lagoon = Terrain::build(1, &[TerrainDab { center: Vec2::new(4.0, 4.0), radius: 7.0, op: TerrainOp::Water { depth: 1.5 } }], &[]);
        let p = Vec2::new(4.0, 4.0);
        let level = lagoon.water_level(p).expect("lagoon");
        let hs = house(Footprint::Rect { center: p, half: Vec2::new(1.5, 1.2), angle: 0.0 }, 2.4, RoofStyle::Turf, Material::Wattle);
        assert!(needs_stilts(&hs, &lagoon));
        let (m, mk) = house_mesh(&hs, &[], &lagoon, 1);
        check(&m, &mk);
        let door = mk.iter().find(|k| k.kind == MarkerKind::Door).unwrap();
        assert!(door.pos.y >= level.max(SEA_LEVEL) + WATER_CLEAR - 1e-3);

        let steep = Terrain::build(1, &[TerrainDab { center: Vec2::new(3.0, 0.0), radius: 5.0, op: TerrainOp::Sculpt { amount: 4.0 } }], &[]);
        let hs = house(Footprint::Rect { center: Vec2::ZERO, half: Vec2::new(2.0, 1.5), angle: 0.0 }, 2.4, RoofStyle::Thatch, Material::Stone);
        assert!(needs_stilts(&hs, &steep));
        check(&house_mesh(&hs, &[], &steep, 1).0, &house_mesh(&hs, &[], &steep, 1).1);

        // Flat dry ground: no stilts unless asked for.
        let flat = Terrain::build(1, &[TerrainDab { center: Vec2::ZERO, radius: 12.0, op: TerrainOp::Flatten { height: 1.0, strength: 1.0 } }], &[]);
        let mut hs = house(Footprint::Rect { center: Vec2::ZERO, half: Vec2::new(2.0, 1.5), angle: 0.0 }, 2.4, RoofStyle::Thatch, Material::Wattle);
        assert!(!needs_stilts(&hs, &flat));
        let (plain, mk) = house_mesh(&hs, &[], &flat, 1);
        assert!((mk.iter().find(|k| k.kind == MarkerKind::Door).unwrap().pos.y - 1.1).abs() < 0.05);
        hs.stilts = 1.8;
        assert!(needs_stilts(&hs, &flat));
        let (raised, mk) = house_mesh(&hs, &[], &flat, 1);
        assert!(mk.iter().find(|k| k.kind == MarkerKind::Door).unwrap().pos.y > 1.0 + 1.8 - 0.05);
        assert!(raised.bounds().unwrap().1.y > plain.bounds().unwrap().1.y + 1.5);
        assert!(has(&raised, Surface::Boards) && !has(&plain, Surface::Boards));
    }

    #[test]
    fn beacons() {
        let t = ground();
        for roof in ROOFS {
            for mat in MATS {
                let hs = house(Footprint::Round { center: Vec2::new(3.0, 3.0), radius: 1.5 }, 6.0, roof, mat);
                assert!(is_beacon(&hs));
                let mut sea_hs = hs.clone();
                sea_hs.footprint = Footprint::Round { center: Vec2::ZERO, radius: 1.5 };
                assert!(!needs_stilts(&sea_hs, &sea()), "beacons stand on the seabed, not stilts");
                let (m, mk) = house_mesh(&hs, &[], &t, 1);
                check(&m, &mk);
                assert_eq!(count(&mk, MarkerKind::Fire), 1);
                assert_eq!(count(&mk, MarkerKind::ChimneySmoke), 0);
                assert!(count(&mk, MarkerKind::Perch) >= 1);
                assert_eq!(count(&mk, MarkerKind::Door), 1);
                assert!(has(&m, Surface::DryStone) && has(&m, Surface::Metal));
                assert_eq!(has(&m, Surface::Thatch), roof == RoofStyle::Thatch, "thatch cone over the fire, else open");
                let fire = mk.iter().find(|k| k.kind == MarkerKind::Fire).unwrap();
                let g = t.height(Vec2::new(3.0, 3.0));
                assert!((fire.pos.xz() - Vec2::new(3.0, 3.0)).length() < 0.05 && fire.pos.y > g + 6.0);
                // Tapered: the top of the tower is narrower than its foot.
                let width_at = |y0: f32, y1: f32| {
                    m.positions.iter().zip(&m.surfaces).filter(|(p, s)| p[1] > y0 && p[1] < y1 && **s == Surface::DryStone as u8).map(|(p, _)| Vec2::new(p[0] - 3.0, p[2] - 3.0).length()).fold(0.0f32, f32::max)
                };
                let lo_y = t.height(Vec2::new(3.0, 3.0));
                assert!(width_at(lo_y + 3.5, lo_y + 5.6) < width_at(lo_y - 1.0, lo_y + 0.45) - 0.2);
            }
        }
        // A low round house is a roundhouse, never a beacon.
        let rh = house(Footprint::Round { center: Vec2::ZERO, radius: 3.0 }, 2.5, RoofStyle::Thatch, Material::Wattle);
        assert!(!is_beacon(&rh));
        let (_, mk) = house_mesh(&rh, &[], &t, 1);
        assert_eq!(count(&mk, MarkerKind::Fire), 0);
        assert_eq!(count(&mk, MarkerKind::ChimneySmoke), 1);
        assert!(!is_beacon(&house(rect(1.0, 1.0), 20.0, RoofStyle::Thatch, Material::Stone)));
    }

    #[test]
    fn roof_mesh_on_rings() {
        let rect_ring = footprint_polygon(&rect(4.0, 3.0));
        let round_ring = footprint_polygon(&round(3.5));
        let l_ring = vec![Vec2::new(0.0, 0.0), Vec2::new(6.0, 0.0), Vec2::new(6.0, 2.0), Vec2::new(2.0, 2.0), Vec2::new(2.0, 6.0), Vec2::new(0.0, 6.0)];
        let cw: Vec<Vec2> = round_ring.iter().rev().copied().collect();
        for ring in [&rect_ring, &round_ring, &l_ring, &cw] {
            for style in ROOFS {
                for pitch in [0.0, 0.6, f32::NAN] {
                    let (m, mk) = roof_mesh(ring, 2.0, style, pitch, Colors { roof: 4, wall: 0, trim: 1 }, 7);
                    check(&m, &mk);
                    assert!(m.bounds().unwrap().0.y > 1.0, "roof stays near its eaves");
                }
            }
        }
        // Degenerate rings never panic and stay valid.
        for ring in [vec![], vec![Vec2::ZERO], vec![Vec2::ZERO, Vec2::X], vec![Vec2::NAN; 5]] {
            let (m, mk) = roof_mesh(&ring, f32::NAN, RoofStyle::Hide, 1.0, Colors::default(), 0);
            assert!(m.is_valid());
            assert!(mk.iter().all(|k| k.pos.is_finite()));
        }
    }

    #[test]
    fn triangle_budget() {
        let t = ground();
        let mut hs = house(Footprint::Rect { center: Vec2::ZERO, half: Vec2::new(6.0, 4.0), angle: 0.0 }, 4.0, RoofStyle::Boat, Material::Stone);
        hs.carved = true;
        hs.hangings = true;
        hs.weathering = 1.0;
        hs.stilts = 2.0;
        let (m, _) = house_mesh(&hs, &[], &t, 1);
        assert!(m.indices.len() / 3 < 12_000, "{} tris", m.indices.len() / 3);
        let (m, _) = house_mesh(&house(Footprint::Round { center: Vec2::ZERO, radius: 6.0 }, 15.0, RoofStyle::Thatch, Material::Stone), &[], &t, 1);
        assert!(m.indices.len() / 3 < 12_000, "{} tris", m.indices.len() / 3);
    }

    #[test]
    fn degenerate_inputs() {
        let t = ground();
        let fps = [
            Footprint::Rect { center: Vec2::new(f32::NAN, 0.0), half: Vec2::new(0.0, f32::INFINITY), angle: f32::NAN },
            Footprint::Rect { center: Vec2::ZERO, half: Vec2::new(30.0, 0.01), angle: 0.0 },
            Footprint::Round { center: Vec2::ZERO, radius: 0.0 },
            Footprint::Round { center: Vec2::ZERO, radius: 1000.0 },
        ];
        for fp in fps {
            for h in [0.0, f32::NAN, 1.49, 24.0, 100.0] {
                for roof in ROOFS {
                    let mut hs = house(fp, h, roof, Material::Stone);
                    hs.roof_pitch = f32::NAN;
                    hs.carved = true;
                    hs.hangings = true;
                    hs.stilts = f32::NAN;
                    hs.weathering = f32::NAN;
                    hs.openings = vec![Opening { along: f32::NAN, kind: OpeningKind::Door }, Opening { along: -5.0, kind: OpeningKind::Window }, Opening { along: 1e9, kind: OpeningKind::WideDoor }];
                    let cuts = [Cut { along: f32::INFINITY, width: f32::NAN, height: 0.0 }, Cut { along: 1e6, width: 9.0, height: 2.0 }];
                    let (m, mk) = house_mesh(&hs, &cuts, &t, 0);
                    check(&m, &mk);
                }
            }
        }
    }
}
