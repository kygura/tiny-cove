//! Blocks, silo towers and roofs (eco-brutalist).
//!
//! A block is a monolith built from horizontal *bands* of wall (bush-hammered plinth, one band
//! per storey, upper storeys cantilevered on tall blocks) separated by proud concrete floor
//! slabs. Each band is a shell of flat quads following a (possibly offset) footprint polygon,
//! with rectangular *holes* cut for windows and doors; every hole gets deep reveals and a
//! recessed glass / door panel. Details (brise-soleil fins, silo ribs, entry canopies,
//! balconies, rain chains, planters, ivy, moss, water streaks, vent stacks) are small boxes on
//! top. Roofs are generic over any simple CCW polygon (`roof_mesh`).
//!
//! Roof covering: Pitched and Sawtooth slabs are sedum when the roof colour index is 0 or 1
//! (the palette's greens) and standing-seam metal otherwise; Shell is always precast
//! concrete; Green is always a sedum mat behind a parapet.
//!
//! All randomness hashes `(seed, world position)` so previews equal committed results.

use crate::geom::{closed, closest_on_polyline, contains, footprint_polygon, outward, signed_area};
use crate::mesh::{Marker, MarkerKind, MeshData, Rng, Surface};
use crate::model::{Colors, Footprint, House, Material, OpeningKind, RoofStyle, ROOF_COLORS};
use crate::palette;
use crate::terrain::Terrain;
use crate::walls::Cut;
use glam::{Vec2, Vec3};
use std::f32::consts::PI;

/// Height of one storey (floor to floor).
const STOREY: f32 = 3.0;
/// Roof slab overhang beyond the wall line.
const OVERHANG: f32 = 0.8;
/// Walls reach this far below the lowest ground corner.
const SINK: f32 = 0.3;
/// Projection of a plain floor-slab band beyond the wall below / above it.
const SLAB: f32 = 0.12;
/// Depth of brise-soleil fins (and of the slab bands that frame them).
const FIN: f32 = 0.6;
/// Floor-slab band extent below / above the storey line.
const SLAB_LO: f32 = 0.12;
const SLAB_HI: f32 = 0.13;

/// Block / silo mesh (walls, openings, roof, vent stack, details) plus markers
/// (vent steam, lit windows, door spots, entry lights, roof perches).
/// `cuts` are passages through the block walls (paths) and become doors / portals.
pub fn house_mesh(house: &House, cuts: &[Cut], terrain: &Terrain, seed: u64) -> (MeshData, Vec<Marker>) {
    let fp = sane_footprint(house.footprint);
    let poly = footprint_polygon(&fp);
    let h = fin(house.wall_height, 3.0).clamp(0.6, 24.0);
    let pitch = fin(house.roof_pitch, 0.6).clamp(0.0, 4.0);
    let og = fin(house.overgrowth, 0.0).clamp(0.0, 1.0);
    let y0 = {
        let m = poly.iter().map(|p| terrain.height(*p)).filter(|v| v.is_finite()).fold(f32::INFINITY, f32::min);
        if m.is_finite() { m } else { 0.0 }
    };
    let center = poly.iter().copied().sum::<Vec2>() / poly.len() as f32;
    let round = matches!(fp, Footprint::Round { .. });
    let tower = round && h >= 6.0;
    let colors = house.colors;
    let trim = palette::trim(colors);
    let (msurf, mcol) = material_look(house.material, colors);
    let slab = palette::wall(colors).lerp(palette::stone(), 0.6);
    let mut style = house.roof;
    if pitch < 0.05 {
        style = RoofStyle::Green;
    }
    if round && matches!(style, RoofStyle::Pitched | RoofStyle::Sawtooth) {
        style = RoofStyle::Shell;
    }
    let mut b = B::default();
    let mut rng = Rng::at(seed ^ 0x4805_E000, center.x, center.y);

    if h < 1.5 {
        pavilion(&mut b, &poly, y0, h, style, pitch, colors, terrain, seed, center);
        return (b.m, b.mk);
    }

    let foundation = fin(house.foundation, 0.0).clamp(0.0, (h * 0.5).min(2.0));
    let has_plinth = foundation > 0.02;
    let n = poly.len();
    let lens: Vec<f32> = (0..n).map(|i| poly[i].distance(poly[(i + 1) % n])).collect();
    let cant = if !round && h >= 6.0 { rng.range(0.3, 0.6) } else { 0.0 };
    let holes = plan_openings(house, cuts, &lens, h, round, tower, &mut rng);
    let fins_on = house.fins && !round;

    // ---- wall bands: plinth, one band per storey, slab bands between them --------------
    let mut bands: Vec<Band> = Vec::new();
    let body_lo = if has_plinth { foundation } else { -SINK };
    if has_plinth {
        bands.push(Band::new(-SINK, foundation, 0.06, Surface::Plinth, palette::foundation()));
    }
    let mut courses: Vec<(f32, f32, f32)> = Vec::new(); // slab line y, offset below, offset above
    let mut lo = body_lo;
    let mut k = 0;
    loop {
        let mut hi = ((k + 1) as f32 * STOREY).min(h);
        if h - hi < 1.2 || k >= 7 {
            hi = h;
        }
        let off = if k >= 1 { cant } else { 0.0 };
        if hi > lo + 0.01 {
            bands.push(Band::new(lo, hi, off, msurf, mcol));
        }
        if hi >= h - 1e-3 {
            break;
        }
        courses.push((hi, off, cant));
        lo = hi;
        k += 1;
    }
    let top_off = bands.last().map_or(0.0, |bd| bd.off);
    for band in &bands {
        wall_band(&mut b, &poly, y0, band, &holes, trim);
    }
    if has_plinth {
        annulus(&mut b, &poly, y0, foundation, 0.06, 0.0, true, &holes, palette::foundation(), Surface::Plinth);
    }
    let ledge_out = if fins_on { FIN } else { SLAB };
    for &(y, below, above) in &courses {
        let out = below.max(above) + ledge_out;
        let ledge = Band::new(y - SLAB_LO, y + SLAB_HI, out, Surface::Concrete, slab);
        wall_band(&mut b, &poly, y0, &ledge, &holes, trim);
        annulus(&mut b, &poly, y0, y - SLAB_LO, out, below, false, &holes, slab * 0.8, Surface::Concrete);
        annulus(&mut b, &poly, y0, y + SLAB_HI, out, above, true, &holes, slab, Surface::Concrete);
    }

    // ---- brise-soleil fins (blocks) / ribs (silos) -------------------------------------
    if round {
        let depth = if house.fins { 0.45 } else if tower { 0.22 } else { 0.0 };
        if depth > 0.0 {
            ribs(&mut b, &poly, &lens, y0, body_lo, h, depth, &holes, slab);
        }
    } else if fins_on {
        let (fc, fs) = if house.material == Material::Timber { (trim, Surface::Steel) } else { (slab, Surface::Concrete) };
        for band in bands.iter().filter(|bd| bd.surf == msurf) {
            fins(&mut b, &poly, y0, band, &holes, fc, fs);
        }
    }

    // ---- openings: windows, doors ------------------------------------------------------
    for hole in &holes {
        let vm = (hole.v0 + hole.v1) * 0.5;
        let band = bands.iter().find(|bd| vm >= bd.lo && vm < bd.hi).or(bands.last()).copied().unwrap_or(Band::new(0.0, h, 0.0, msurf, mcol));
        let wf = Wf::at_off(&poly, band.off, hole.edge, y0);
        if hole.kind.is_door() {
            door_details(&mut b, &wf, hole, h, slab, terrain);
        } else {
            window_details(&mut b, &wf, hole, &band, slab, og, seed, !tower && !fins_on);
        }
    }

    // ---- overgrowth: ivy, moss on ledges, water streaks --------------------------------
    if og > 0.01 {
        weathering(&mut b, &poly, y0, &bands, &courses, ledge_out, &holes, og, seed);
    }

    // ---- roof --------------------------------------------------------------------------
    let roof_poly = offset_poly(&poly, top_off);
    let eave = y0 + h;
    let opts = RoofOpts {
        overhang: if style == RoofStyle::Green { 0.0 } else { OVERHANG },
        gable: Some((msurf, mcol)),
        planters: house.planters || tower,
        parapet: true,
        solar: style == RoofStyle::Green && !round && !house.planters && rng.f() < 0.6,
        trim,
        seed,
    };
    let roof_pitch = if tower && style == RoofStyle::Shell { pitch.max(0.5) } else { pitch };
    let geom = roof_impl(&mut b, &roof_poly, eave, style, roof_pitch, colors, &opts);

    // ---- ventilation stack (rect blocks) -----------------------------------------------
    if let (false, Some(rf)) = (round, rect_frame(&roof_poly)) {
        vent_stack(&mut b, &rf, &geom, eave, slab, trim, seed);
    }

    // ---- balconies, rain chains, planters ----------------------------------------------
    decor(&mut b, &poly, &lens, y0, h, cant, fins_on, &holes, slab, trim, og, terrain, seed);

    (b.m, b.mk)
}

/// Roof over an arbitrary simple polygon `poly` (counter-clockwise, as `geom::footprint_polygon`)
/// with eaves at height `base_y`. Used for blocks and for roofed wall rings.
/// `pitch` as in [`House::roof_pitch`].
///
/// Rectangles get a sharp slab gable (Pitched) or north-light bays (Sawtooth); Shell gives a
/// folded-plate pyramid on polygons with up to six sides and a shallow dome on rounder ones;
/// other star-shaped polygons fall back to Shell for Pitched / Sawtooth; concave rings whose
/// centroid cannot see every edge, zero pitch and Green give a flat sedum roof with a parapet.
pub fn roof_mesh(poly: &[Vec2], base_y: f32, style: RoofStyle, pitch: f32, colors: Colors, seed: u64) -> (MeshData, Vec<Marker>) {
    let mut b = B::default();
    let opts = RoofOpts {
        overhang: OVERHANG,
        gable: Some((Surface::Concrete, palette::stone())),
        planters: false,
        parapet: true,
        solar: false,
        trim: palette::trim(colors),
        seed,
    };
    let base = fin(base_y, 0.0);
    roof_impl(&mut b, poly, base, style, pitch, colors, &opts);
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

fn material_look(m: Material, c: Colors) -> (Surface, Vec3) {
    match m {
        Material::Concrete => (Surface::Concrete, palette::stone()),
        Material::RammedEarth => (Surface::Earth, palette::wall(c)),
        Material::Timber => (Surface::Cladding, palette::wood().lerp(palette::trim(c), 0.3)),
    }
}

/// Roof covering by palette entry: slate blue → slate, gold → thatch, else clay tiles.
/// Returns (surface, thickness).
fn roof_cover(c: Colors) -> (Surface, f32) {
    match c.roof % crate::model::ROOF_COLORS {
        1 => (Surface::Seam, 0.12),
        4 => (Surface::Precast, 0.3),
        _ => (Surface::Sedum, 0.14),
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
    let mut v: Vec<Vec2> = poly.iter().copied().filter(|p| p.is_finite()).collect();
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

/// Mesh + markers under construction.
#[derive(Default)]
struct B {
    m: MeshData,
    mk: Vec<Marker>,
}

impl B {
    fn marker(&mut self, kind: MarkerKind, pos: Vec3, dir: Vec3) {
        if pos.is_finite() && dir.is_finite() {
            self.mk.push(Marker { kind, pos, dir });
        }
    }

    /// Quad whose front faces `want` (vertex order fixed up as needed).
    fn face(&mut self, p: [Vec3; 4], uv: [Vec2; 4], want: Vec3, c: Vec3, s: Surface) {
        if (p[1] - p[0]).cross(p[3] - p[0]).dot(want) < 0.0 {
            self.m.quad_uv([p[0], p[3], p[2], p[1]], [uv[0], uv[3], uv[2], uv[1]], c, s);
        } else {
            self.m.quad_uv(p, uv, c, s);
        }
    }

    /// Triangle whose front faces `want`. Degenerate triangles are skipped.
    fn tri(&mut self, a: Vec3, b: Vec3, c: Vec3, want: Vec3, col: Vec3, s: Surface) {
        let n = (b - a).cross(c - a);
        if n.length_squared() < 1e-12 {
            return;
        }
        if n.dot(want) < 0.0 { self.m.tri(a, c, b, col, s, 1.0) } else { self.m.tri(a, b, c, col, s, 1.0) }
    }

    /// Wall-plane quad in a wall frame (u along the wall, y up, `out` off the plane).
    #[allow(clippy::too_many_arguments)]
    fn wquad(&mut self, wf: &Wf, u0: f32, u1: f32, y0: f32, y1: f32, out: f32, inward: bool, c: Vec3, s: Surface) {
        let p = [wf.at(u0, y0, out), wf.at(u1, y0, out), wf.at(u1, y1, out), wf.at(u0, y1, out)];
        let uv = [Vec2::new(u0, y0), Vec2::new(u1, y0), Vec2::new(u1, y1), Vec2::new(u0, y1)];
        self.face(p, uv, if inward { -wf.n } else { wf.n }, c, s);
    }

    /// Box in a wall frame: centre (u, y, out), half extents (along, up, out).
    #[allow(clippy::too_many_arguments)]
    fn lbox(&mut self, wf: &Wf, u: f32, y: f32, out: f32, half: Vec3, c: Vec3, s: Surface) {
        self.m.oriented_box(wf.at(u, y, out), half, wf.d, c, s, 1.0);
    }
}

/// Frame of one wall edge (possibly offset from the footprint). `u` is measured in footprint
/// edge coordinates (0 at the edge's first corner); offset edges extend to `-shift..len+shift`.
#[derive(Clone, Copy, Debug)]
struct Wf {
    o: Vec3,
    d: Vec3,
    n: Vec3,
    len: f32,
    shift: f32,
}

impl Wf {
    fn new(base: &[Vec2], offp: &[Vec2], i: usize, y0: f32) -> Wf {
        let n = base.len();
        let j = (i + 1) % n;
        let len = base[i].distance(base[j]);
        let (a, b) = (offp[i], offp[j]);
        let l2 = a.distance(b);
        let dir = if l2 > 1e-6 { (b - a) / l2 } else { (base[j] - base[i]).normalize_or(Vec2::X) };
        let shift = (l2 - len) * 0.5;
        Wf { o: p3(a + dir * shift, y0), d: p3(dir, 0.0), n: p3(outward(dir), 0.0), len, shift }
    }

    fn at_off(base: &[Vec2], off: f32, i: usize, y0: f32) -> Wf {
        Wf::new(base, &offset_poly(base, off), i, y0)
    }

    fn at(&self, u: f32, y: f32, out: f32) -> Vec3 {
        self.o + self.d * u + Vec3::Y * y + self.n * out
    }

    fn u_start(&self) -> f32 {
        -self.shift
    }

    fn u_end(&self) -> f32 {
        self.len + self.shift
    }
}

// =============================================================================================
// Walls: bands and holes
// =============================================================================================

#[derive(Clone, Copy, Debug)]
struct Band {
    /// Heights relative to the floor level `y0`.
    lo: f32,
    hi: f32,
    /// Offset from the footprint (positive = outward).
    off: f32,
    surf: Surface,
    color: Vec3,
    /// Faces point inward (parapet backs).
    inward: bool,
}

impl Band {
    fn new(lo: f32, hi: f32, off: f32, surf: Surface, color: Vec3) -> Band {
        Band { lo, hi, off, surf, color, inward: false }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Hk {
    Window,
    Slit,
    Door,
    Wide,
}

impl Hk {
    fn depth(self) -> f32 {
        match self {
            Hk::Window => 0.12,
            Hk::Slit => 0.2,
            Hk::Door | Hk::Wide => 0.16,
        }
    }
    fn is_door(self) -> bool {
        matches!(self, Hk::Door | Hk::Wide)
    }
}

/// A rectangular hole in a wall edge. `u` in footprint edge coordinates, `v` above floor.
#[derive(Clone, Copy, Debug)]
struct Hole {
    edge: usize,
    u0: f32,
    u1: f32,
    v0: f32,
    v1: f32,
    kind: Hk,
    along: f32,
}

/// One band of wall around the whole polygon, with holes cut out (plus reveals and panels).
fn wall_band(b: &mut B, base: &[Vec2], y0: f32, band: &Band, holes: &[Hole], trim: Vec3) {
    if band.hi - band.lo < 1e-3 {
        return;
    }
    let offp = offset_poly(base, band.off);
    for i in 0..base.len() {
        let wf = Wf::new(base, &offp, i, y0);
        if wf.len < 1e-3 {
            continue;
        }
        let (ua, ub) = (wf.u_start(), wf.u_end());
        let hs: Vec<(Hole, f32, f32)> = if band.inward {
            Vec::new()
        } else {
            holes
                .iter()
                .filter(|h| h.edge == i)
                .filter_map(|h| {
                    let c0 = h.v0.max(band.lo);
                    let c1 = h.v1.min(band.hi);
                    (c1 - c0 > 1e-3).then_some((*h, c0, c1))
                })
                .collect()
        };
        let mut us = vec![ua, ub];
        for (h, _, _) in &hs {
            us.push(h.u0.clamp(ua, ub));
            us.push(h.u1.clamp(ua, ub));
        }
        us.sort_by(f32::total_cmp);
        us.dedup_by(|a, b| (*a - *b).abs() < 1e-4);
        for w in us.windows(2) {
            let (s0, s1) = (w[0], w[1]);
            if s1 - s0 < 1e-4 {
                continue;
            }
            let mut cover: Vec<(f32, f32)> =
                hs.iter().filter(|(h, _, _)| h.u0 <= s0 + 1e-4 && h.u1 >= s1 - 1e-4).map(|(_, c0, c1)| (*c0, *c1)).collect();
            cover.sort_by(|a, b| a.0.total_cmp(&b.0));
            let mut y = band.lo;
            for (c0, c1) in cover {
                if c0 > y + 1e-4 {
                    b.wquad(&wf, s0, s1, y, c0, 0.0, band.inward, band.color, band.surf);
                }
                y = y.max(c1);
            }
            if band.hi > y + 1e-4 {
                b.wquad(&wf, s0, s1, y, band.hi, 0.0, band.inward, band.color, band.surf);
            }
        }
        for (h, c0, c1) in &hs {
            reveal(b, &wf, h, *c0, *c1, band, trim);
        }
    }
}

/// Jambs, sill/head reveals and the recessed panel of one hole (clipped to a band).
fn reveal(b: &mut B, wf: &Wf, h: &Hole, c0: f32, c1: f32, band: &Band, trim: Vec3) {
    let d = h.kind.depth();
    let (u0, u1) = (h.u0, h.u1);
    let p = |u: f32, y: f32| wf.at(u, y, 0.0);
    let q = |u: f32, y: f32| wf.at(u, y, -d);
    let uvj = [Vec2::new(0.0, c0), Vec2::new(d, c0), Vec2::new(d, c1), Vec2::new(0.0, c1)];
    let rc = band.color * 0.9;
    b.face([p(u0, c0), q(u0, c0), q(u0, c1), p(u0, c1)], uvj, wf.d, rc, band.surf);
    b.face([p(u1, c0), q(u1, c0), q(u1, c1), p(u1, c1)], uvj, -wf.d, rc, band.surf);
    let uvh = [Vec2::new(u0, 0.0), Vec2::new(u1, 0.0), Vec2::new(u1, d), Vec2::new(u0, d)];
    if (c0 - h.v0).abs() < 1e-4 {
        b.face([p(u0, c0), p(u1, c0), q(u1, c0), q(u0, c0)], uvh, Vec3::Y, rc, band.surf);
    }
    if (c1 - h.v1).abs() < 1e-4 {
        b.face([p(u0, c1), p(u1, c1), q(u1, c1), q(u0, c1)], uvh, -Vec3::Y, rc * 0.8, band.surf);
    }
    let (surf, col) = match h.kind {
        Hk::Window | Hk::Slit => (Surface::Window, palette::white()),
        Hk::Door | Hk::Wide => (Surface::Door, trim),
    };
    let w = (u1 - u0).max(1e-3);
    let ht = (h.v1 - h.v0).max(1e-3);
    let uv = |u: f32, y: f32| Vec2::new((u - u0) / w, (y - h.v0) / ht);
    b.face(
        [q(u0, c0), q(u1, c0), q(u1, c1), q(u0, c1)],
        [uv(u0, c0), uv(u1, c0), uv(u1, c1), uv(u0, c1)],
        wf.n,
        col,
        surf,
    );
}

/// Horizontal ring between two offsets of `base` at height `y` (ledge tops, jetty soffit…),
/// interrupted where holes pass through that height.
#[allow(clippy::too_many_arguments)]
fn annulus(b: &mut B, base: &[Vec2], y0: f32, y: f32, off_a: f32, off_b: f32, up: bool, holes: &[Hole], c: Vec3, s: Surface) {
    if (off_a - off_b).abs() < 1e-4 {
        return;
    }
    let pa = offset_poly(base, off_a);
    let pb = offset_poly(base, off_b);
    for i in 0..base.len() {
        let wa = Wf::new(base, &pa, i, y0);
        let wb = Wf::new(base, &pb, i, y0);
        if wa.len < 1e-3 {
            continue;
        }
        let mut gaps: Vec<(f32, f32)> =
            holes.iter().filter(|h| h.edge == i && h.v0 < y - 1e-4 && h.v1 > y + 1e-4).map(|h| (h.u0, h.u1)).collect();
        gaps.sort_by(|a, b| a.0.total_cmp(&b.0));
        let mut segs: Vec<(Option<f32>, Option<f32>)> = Vec::new();
        let mut prev: Option<f32> = None;
        for (g0, g1) in gaps {
            segs.push((prev, Some(g0)));
            prev = Some(prev.map_or(g1, |p: f32| p.max(g1)));
        }
        segs.push((prev, None));
        for (s0, s1) in segs {
            let (aa, ab) = (s0.unwrap_or(wa.u_start()), s0.unwrap_or(wb.u_start()));
            let (ba, bb) = (s1.unwrap_or(wa.u_end()), s1.unwrap_or(wb.u_end()));
            if ba - aa < 1e-4 || bb - ab < 1e-4 {
                continue;
            }
            let p = [wa.at(aa, y, 0.0), wa.at(ba, y, 0.0), wb.at(bb, y, 0.0), wb.at(ab, y, 0.0)];
            let uv = p.map(|v| Vec2::new(v.x, v.z));
            b.face(p, uv, if up { Vec3::Y } else { -Vec3::Y }, c, s);
        }
    }
}

// =============================================================================================
// Openings
// =============================================================================================

fn locate(lens: &[f32], along: f32) -> (usize, f32) {
    let per: f32 = lens.iter().sum();
    if per <= 1e-4 || !along.is_finite() {
        return (0, 0.0);
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

/// Builds one hole of `kind` centred at `u` on `edge` (clamped into the edge), rejecting it if
/// it would overlap an existing hole.
#[allow(clippy::too_many_arguments)]
fn place_hole(holes: &[Hole], lens: &[f32], edge: usize, u: f32, kind: Hk, v0: f32, ht: f32, w: f32, round: bool) -> Option<Hole> {
    let len = lens[edge];
    let mut margin = if round { 0.1 } else { 0.55 };
    if len - 2.0 * margin < 0.3 {
        margin = 0.05;
    }
    let w = w.min(len - 2.0 * margin);
    if w < 0.18 || ht < 0.25 {
        return None;
    }
    let lo = margin + w * 0.5;
    let u = fin(u, len * 0.5).clamp(lo, (len - margin - w * 0.5).max(lo));
    let h = Hole {
        edge,
        u0: u - w * 0.5,
        u1: u + w * 0.5,
        v0,
        v1: v0 + ht,
        kind,
        along: lens[..edge].iter().sum::<f32>() + u,
    };
    let clash = holes.iter().any(|o| {
        o.edge == edge && h.u0 < o.u1 + 0.12 && o.u0 < h.u1 + 0.12 && h.v0 < o.v1 + 0.1 && o.v0 < h.v1 + 0.1
    });
    (!clash).then_some(h)
}

fn door_dims(kind: Hk, h: f32) -> (f32, f32) {
    match kind {
        Hk::Wide => (2.4, 2.4f32.min(h - 0.3)),
        _ => (1.1, 2.0f32.min(h - 0.3)),
    }
}

fn plan_openings(house: &House, cuts: &[Cut], lens: &[f32], h: f32, round: bool, tower: bool, rng: &mut Rng) -> Vec<Hole> {
    let mut holes: Vec<Hole> = Vec::new();
    let n = lens.len();
    let per: f32 = lens.iter().sum();
    // Ground-storey slot window size.
    let win_h = 1.4f32.min(h - 0.6);
    let win_sill = 1.0f32.min(h - 0.3 - win_h).max(0.3);

    for op in &house.openings {
        let (e, u) = locate(lens, op.along);
        let hole = match op.kind {
            OpeningKind::Window => place_hole(&holes, lens, e, u, Hk::Window, win_sill, win_h, 0.8, round),
            OpeningKind::Door => {
                let (w, ht) = door_dims(Hk::Door, h);
                place_hole(&holes, lens, e, u, Hk::Door, 0.0, ht, w, round)
            }
            OpeningKind::WideDoor => {
                let (w, ht) = door_dims(Hk::Wide, h);
                place_hole(&holes, lens, e, u, Hk::Wide, 0.0, ht, w, round)
            }
        };
        holes.extend(hole);
    }

    // Paths through the walls become doors (unless a door is already close by).
    for cut in cuts {
        if !cut.along.is_finite() {
            continue;
        }
        let along = cut.along.rem_euclid(per.max(1e-3));
        let width = fin(cut.width, 1.4);
        let near = holes.iter().any(|o| {
            let d = (o.along - along).abs();
            o.kind.is_door() && d.min(per - d) < width * 0.5 + 1.0
        });
        if near {
            continue;
        }
        let kind = if width > 2.0 { Hk::Wide } else { Hk::Door };
        let (e, u) = locate(lens, along);
        let (w, ht) = door_dims(kind, h);
        let w = if kind == Hk::Wide { w.max(width.min(4.0)) } else { w };
        // Windows in the way give way to the passage.
        holes.retain(|o| !(o.edge == e && o.v0 < ht + 0.1 && o.u0 < u + w * 0.5 + 0.15 && u - w * 0.5 - 0.15 < o.u1));
        holes.extend(place_hole(&holes, lens, e, u, kind, 0.0, ht, w, round));
    }

    if !house.openings.is_empty() {
        return holes;
    }

    // Automatic door on the longest wall.
    if !holes.iter().any(|o| o.kind.is_door()) {
        let mut best = 0;
        for i in 1..n {
            if lens[i] > lens[best] + 1e-3 {
                best = i;
            }
        }
        let jitter = if round { 0.0 } else { (rng.f() - 0.5) * 0.3 * lens[best] };
        let (w, ht) = door_dims(Hk::Door, h);
        holes.extend(place_hole(&holes, lens, best, lens[best] * 0.5 + jitter, Hk::Door, 0.0, ht, w, round));
    }

    if tower {
        // Slit windows spiralling up the silo, two per storey.
        let mut k = 1;
        while (k as f32) * STOREY + 2.4 <= h - 0.2 {
            for j in 0..2 {
                let e = (k * 3 + j * n / 2 + n / 4) % n;
                holes.extend(place_hole(&holes, lens, e, lens[e] * 0.5, Hk::Slit, k as f32 * STOREY + 0.8, 1.5, 0.3, true));
            }
            k += 1;
        }
        return holes;
    }

    // Per storey: either one ribbon window per façade or a row of deep slots.
    let ribbon = !round && rng.f() < 0.45;
    let mut k = 0;
    loop {
        let (sill, wh) = if k == 0 {
            (win_sill, if ribbon { win_h.min(1.0) } else { win_h })
        } else {
            (k as f32 * STOREY + 0.9, if ribbon { 1.0 } else { 1.5 })
        };
        if wh < 0.4 || sill + wh > h - 0.25 {
            break;
        }
        for e in 0..n {
            if round {
                if (e + k) % 3 == 0 {
                    holes.extend(place_hole(&holes, lens, e, lens[e] * 0.5, Hk::Window, sill, wh, 0.6, true));
                }
                continue;
            }
            let usable = lens[e] - 1.4;
            if ribbon && usable >= 1.2 {
                if let Some(hole) = place_hole(&holes, lens, e, lens[e] * 0.5, Hk::Window, sill, wh, usable, false) {
                    holes.push(hole);
                    continue;
                }
            }
            let mut count = (usable / 1.6).floor() as i32;
            if count == 0 && usable >= 0.7 {
                count = 1;
            }
            for j in 0..count.max(0) {
                let u = 0.7 + usable * (j as f32 + 0.5) / count as f32;
                holes.extend(place_hole(&holes, lens, e, u, Hk::Window, sill, wh, 0.6, false));
            }
        }
        k += 1;
        if k > 12 {
            break;
        }
    }
    holes
}

/// Mullions, drip sill, water streak and sometimes a planter under a recessed window.
#[allow(clippy::too_many_arguments)]
fn window_details(b: &mut B, wf: &Wf, h: &Hole, band: &Band, slab: Vec3, og: f32, seed: u64, allow_box: bool) {
    let (u0, u1, v0, v1) = (h.u0, h.u1, h.v0, h.v1);
    let w = u1 - u0;
    let mid = (u0 + u1) * 0.5;
    let vm = (v0 + v1) * 0.5;
    let d = h.kind.depth();
    let centre = wf.at(mid, vm, 0.0);
    let mut rng = Rng::at(seed ^ 0x5157_0000, centre.x, centre.z + centre.y * 0.37);
    b.marker(MarkerKind::WindowLight, wf.at(mid, vm, -d + 0.02), wf.n);
    // Ribbon windows: dark steel mullions just in front of the glass.
    if w > 1.5 {
        let k = ((w / 1.3).round() as usize).clamp(2, 24);
        for j in 1..k {
            let u = u0 + w * j as f32 / k as f32;
            b.lbox(wf, u, vm, -d + 0.05, Vec3::new(0.035, (v1 - v0) * 0.5, 0.035), palette::metal(), Surface::Metal);
        }
    }
    // Projecting drip sill.
    b.lbox(wf, mid, v0 - 0.06, 0.05, Vec3::new(w * 0.5 + 0.08, 0.06, 0.09), slab * 0.92, Surface::Concrete);
    // Dark water streak running down from the sill.
    if og > 0.02 && rng.f() < 0.25 + 0.6 * og {
        let top = v0 - 0.12;
        let bot = (top - rng.range(0.4, 0.5 + 1.8 * og)).max(band.lo + 0.05);
        if top - bot > 0.15 {
            let sw = (w * 0.5 * rng.range(0.4, 0.9)).min(0.5);
            let su = mid + rng.range(-0.1, 0.1);
            b.wquad(wf, su - sw, su + sw, bot, top, 0.02, false, band.color * 0.55, band.surf);
        }
    }
    // Concrete planter box with trailing greenery.
    if allow_box && h.kind == Hk::Window && rng.f() < 0.2 + 0.3 * og {
        let leaves = palette::leaves();
        b.lbox(wf, mid, v0 - 0.32, 0.25, Vec3::new(w * 0.5 + 0.05, 0.16, 0.2), slab * 0.9, Surface::Concrete);
        b.lbox(wf, mid, v0 - 0.13, 0.25, Vec3::new(w * 0.5, 0.04, 0.17), leaves, Surface::Leaves);
        let drop = rng.range(0.2, 0.4 + 0.6 * og);
        b.wquad(wf, mid - w * 0.4, mid + w * 0.4, v0 - 0.48 - drop, v0 - 0.2, 0.46, false, leaves * 0.8, Surface::Fiber);
    }
}

/// Cantilevered canopy slab with a downlight over the recessed entry, threshold slab.
fn door_details(b: &mut B, wf: &Wf, h: &Hole, wall_h: f32, slab: Vec3, terrain: &Terrain) {
    let (u0, u1, v1) = (h.u0, h.u1, h.v1);
    let w = u1 - u0;
    let mid = (u0 + u1) * 0.5;
    let cy = v1 + 0.3;
    if cy + 0.12 <= wall_h {
        b.lbox(wf, mid, cy, 0.65, Vec3::new(w * 0.5 + 0.6, 0.12, 0.7), slab, Surface::Concrete);
        b.lbox(wf, mid, cy - 0.14, 0.8, Vec3::new(0.12, 0.02, 0.12), palette::glow(), Surface::Glow);
        b.marker(MarkerKind::Lantern, wf.at(mid, cy - 0.25, 0.8), Vec3::Y);
    }
    b.lbox(wf, mid, 0.0, 0.3, Vec3::new(w * 0.5 + 0.3, 0.09, 0.3), palette::foundation(), Surface::Plinth);
    let spot = wf.at(mid, 0.0, 0.8);
    let gy = fin(terrain.height(Vec2::new(spot.x, spot.z)), spot.y);
    b.marker(MarkerKind::Door, Vec3::new(spot.x, gy, spot.z), wf.n);
}

// =============================================================================================
// Wall dressing
// =============================================================================================

/// Brise-soleil: deep vertical fins across every façade of one storey band, clear of doors.
fn fins(b: &mut B, base: &[Vec2], y0: f32, band: &Band, holes: &[Hole], col: Vec3, s: Surface) {
    let lo = band.lo.max(-SINK);
    if band.hi - lo < 0.3 {
        return;
    }
    let offp = offset_poly(base, band.off);
    for i in 0..base.len() {
        let wf = Wf::new(base, &offp, i, y0);
        let (us, ue) = (wf.u_start() + 0.12, wf.u_end() - 0.12);
        if ue - us < 0.5 {
            continue;
        }
        let k = (((ue - us) / 1.1).round() as usize).clamp(1, 64);
        for j in 0..=k {
            let u = us + (ue - us) * j as f32 / k as f32;
            let blocked =
                holes.iter().any(|o| o.edge == i && o.kind.is_door() && o.v1 > lo && o.v0 < band.hi && u > o.u0 - 0.25 && u < o.u1 + 0.25);
            if !blocked {
                b.lbox(&wf, u, (lo + band.hi) * 0.5, FIN * 0.5, Vec3::new(0.07, (band.hi - lo) * 0.5, FIN * 0.5), col, s);
            }
        }
    }
}

/// Silo ribs: full-height pilasters on every other facet corner, clear of doors.
#[allow(clippy::too_many_arguments)]
fn ribs(b: &mut B, poly: &[Vec2], lens: &[f32], y0: f32, lo: f32, hi: f32, depth: f32, holes: &[Hole], col: Vec3) {
    let n = poly.len();
    for i in (0..n).step_by(2) {
        let prev = (i + n - 1) % n;
        let blocked =
            holes.iter().any(|o| o.kind.is_door() && ((o.edge == i && o.u0 < 0.35) || (o.edge == prev && o.u1 > lens[prev] - 0.35)));
        if blocked {
            continue;
        }
        let n0 = outward((poly[i] - poly[prev]).normalize_or_zero());
        let n1 = outward((poly[(i + 1) % n] - poly[i]).normalize_or_zero());
        let m = (n0 + n1).normalize_or(n1);
        let c = poly[i] + m * depth * 0.5;
        let tangent = p3(Vec2::new(-m.y, m.x), 0.0);
        b.m.oriented_box(p3(c, y0 + (lo + hi) * 0.5), Vec3::new(0.12, (hi - lo) * 0.5, depth * 0.5), tangent, col, Surface::Concrete, 1.0);
    }
}

/// Overgrowth: ivy climbing from the ground, moss on the slab ledges, dark water streaks
/// under them. Amount and reach scale with `og` (0..=1).
#[allow(clippy::too_many_arguments)]
fn weathering(b: &mut B, poly: &[Vec2], y0: f32, bands: &[Band], courses: &[(f32, f32, f32)], ledge_out: f32, holes: &[Hole], og: f32, seed: u64) {
    let leaves = palette::leaves();
    let n = poly.len();
    if let Some(band) = bands.iter().find(|bd| bd.surf != Surface::Plinth) {
        let offp = offset_poly(poly, band.off);
        for i in 0..n {
            let wf = Wf::new(poly, &offp, i, y0);
            if wf.len < 1.0 {
                continue;
            }
            let m = wf.at(wf.len * 0.5, 0.0, 0.0);
            let mut rng = Rng::at(seed ^ 0x1717_0000, m.x, m.z);
            if rng.f() >= 0.1 + 0.75 * og {
                continue;
            }
            let uc = rng.range(0.4, (wf.len - 0.4).max(0.41));
            let top = rng.range(1.0, 1.4 + 5.0 * og).min(band.hi - 0.15);
            let w0 = rng.range(0.5, 0.9 + og);
            if holes.iter().any(|o| o.edge == i && o.u0 < uc + w0 * 0.5 + 0.2 && uc - w0 * 0.5 - 0.2 < o.u1 && o.v0 < top + 0.6) {
                continue;
            }
            let base_y = band.lo.max(-0.1);
            let rows = ((top - base_y) / 0.45).ceil().clamp(1.0, 12.0) as usize;
            for r in 0..rows {
                let y = base_y + r as f32 * 0.45;
                let w = w0 * (1.0 - r as f32 / rows as f32 * 0.6) * rng.range(0.7, 1.1);
                let u = uc + rng.range(-0.15, 0.15);
                b.wquad(&wf, u - w * 0.5, u + w * 0.5, y, y + 0.6, 0.04, false, leaves * rng.range(0.7, 1.05), Surface::Fiber);
            }
        }
    }
    for (ci, &(y, below, above)) in courses.iter().enumerate() {
        let out = below.max(above) + ledge_out;
        let Some(wall) = bands.iter().find(|bd| bd.surf != Surface::Plinth && bd.lo < y - 0.5 && bd.hi > y - 0.5).copied() else {
            continue;
        };
        let (pa, pb) = (offset_poly(poly, above), offset_poly(poly, below));
        for i in 0..n {
            let wa = Wf::new(poly, &pa, i, y0);
            let wb = Wf::new(poly, &pb, i, y0);
            if wa.len < 1.0 {
                continue;
            }
            let m = wa.at(wa.len * 0.5, y, 0.0);
            let mut rng = Rng::at(seed ^ 0x3055_0000 ^ ci as u64, m.x, m.z);
            for _ in 0..((wa.len / 2.0) as usize).clamp(1, 16) {
                let u = rng.range(0.3, wa.len - 0.3);
                if rng.f() < og * 0.8 {
                    let l = rng.range(0.4, 1.4);
                    let d = out - above;
                    let c = leaves * rng.range(0.55, 0.8);
                    b.lbox(&wa, u, y + SLAB_HI + 0.025, d * 0.5, Vec3::new(l * 0.5, 0.03, d * 0.5 - 0.01), c, Surface::Fiber);
                }
                if rng.f() < og * 0.7 {
                    let top = y - SLAB_LO;
                    let bot = (top - rng.range(0.5, 0.6 + 2.0 * og)).max(wall.lo + 0.05);
                    let sw = rng.range(0.1, 0.35);
                    let clear = !holes.iter().any(|o| o.edge == i && o.u0 < u + sw && u - sw < o.u1 && o.v1 > bot && o.v0 < top);
                    if clear && top - bot > 0.15 {
                        b.wquad(&wb, u - sw, u + sw, bot, top, 0.02, false, wall.color * 0.55, wall.surf);
                    }
                }
            }
        }
    }
}

// =============================================================================================
// Roofs
// =============================================================================================

#[derive(Clone, Copy, Debug)]
struct RoofOpts {
    overhang: f32,
    /// Material of gable-end infill (and sawtooth bay ends); `None` leaves them open.
    gable: Option<(Surface, Vec3)>,
    /// Flat roofs: crenellated planter boxes on the parapet.
    planters: bool,
    /// Flat roofs: thick parapet (else a thin overhanging slab with an underside).
    parapet: bool,
    /// Flat roofs on rectangles: rows of tilted solar panels.
    solar: bool,
    trim: Vec3,
    seed: u64,
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

/// What was built, so the vent stack can find the roof surface.
#[derive(Clone, Debug)]
enum RoofGeom {
    None,
    Flat { top: f32 },
    Ridge { rf: Rf, base: f32, rise: f32, tv: f32 },
    Saw { rf: Rf, base: f32, rise: f32, bw: f32, tv: f32 },
    Pyramid { c: Vec2, base: f32, rise: f32, edges: Vec<(Vec2, f32)>, tv: f32 },
}

impl RoofGeom {
    /// Height of the roof's top surface at `p` (inside the footprint).
    fn top_at(&self, p: Vec2) -> Option<f32> {
        match self {
            RoofGeom::None => None,
            RoofGeom::Flat { top } => Some(*top),
            RoofGeom::Ridge { rf, base, rise, tv } => {
                let t = (p - rf.c).dot(rf.ac).abs();
                Some(base + rise - rise / rf.b.max(0.05) * t.min(rf.b) + tv)
            }
            RoofGeom::Saw { rf, base, rise, bw, tv } => {
                let s = (p - rf.c).dot(rf.ax) + rf.a;
                Some(base + rise * (s.rem_euclid(bw.max(0.05)) / bw.max(0.05)).min(1.0) + tv)
            }
            RoofGeom::Pyramid { c, base, rise, edges, tv } => {
                let r = edges.iter().map(|(n, d)| (p - *c).dot(*n) / d.max(0.05)).fold(0.0f32, f32::max).min(1.0);
                Some(base + rise * (1.0 - r) + tv)
            }
        }
    }
}

/// Roof covering of slab roofs by palette entry: greens (0, 1) → sedum, else standing seam.
/// Returns (surface, slab thickness).
fn roof_cover(c: Colors) -> (Surface, f32) {
    if c.roof % ROOF_COLORS < 2 { (Surface::Sedum, 0.3) } else { (Surface::Seam, 0.24) }
}

/// Tint of a sedum mat: the roof colour if it is one of the greens, else a leaf green.
fn sedum_color(c: Colors) -> Vec3 {
    if c.roof % ROOF_COLORS < 2 { palette::roof(c) } else { palette::leaves() * 0.9 }
}

fn roof_impl(b: &mut B, poly: &[Vec2], base: f32, style: RoofStyle, pitch: f32, colors: Colors, o: &RoofOpts) -> RoofGeom {
    let poly = clean_poly(poly);
    if poly.len() < 3 || signed_area(&poly) < 1e-3 || !base.is_finite() {
        return RoofGeom::None;
    }
    let pitch = fin(pitch, 0.6).clamp(0.0, 4.0);
    let n = poly.len();
    let c = area_centroid(&poly);
    let edges: Vec<(Vec2, f32)> = (0..n)
        .map(|i| {
            let (a, q) = (poly[i], poly[(i + 1) % n]);
            let nrm = outward((q - a).normalize_or_zero());
            (nrm, (a - c).dot(nrm))
        })
        .collect();
    let dmin = edges.iter().map(|e| e.1).fold(f32::INFINITY, f32::min);
    let star = contains(&poly, c) && dmin > 0.15;
    let rf = rect_frame(&poly);
    let flat = style == RoofStyle::Green || pitch < 0.05 || (rf.is_none() && !star);
    if flat {
        return flat_roof(b, &poly, base, sedum_color(colors), o);
    }
    let col = palette::roof(colors);
    match (rf, style) {
        (Some(rf), RoofStyle::Pitched) => {
            let (surf, tv) = roof_cover(colors);
            ridge_roof(b, &rf, base, (pitch * rf.b).min(30.0), surf, tv, col, o)
        }
        (Some(rf), RoofStyle::Sawtooth) => {
            let (surf, tv) = roof_cover(colors);
            sawtooth_roof(b, &rf, base, pitch, surf, tv, col, o)
        }
        _ => {
            let shell = palette::stone().lerp(col, 0.15);
            shell_roof(b, &poly, c, &edges, base, (pitch * dmin).min(30.0), shell, o)
        }
    }
}

/// One roof slab: top surface (raised by `tv`), concrete soffit, and concrete edge faces on
/// the edges that have an outward direction in `fascia`.
#[allow(clippy::too_many_arguments)]
fn roof_face(b: &mut B, pts: &[Vec3], fascia: &[Option<Vec3>], tv: f32, e: Vec3, col: Vec3, surf: Surface) {
    let len = pts.len();
    if len < 3 {
        return;
    }
    let mut nn = Vec3::ZERO;
    for i in 0..len {
        nn += pts[i].cross(pts[(i + 1) % len]);
    }
    let mut n = nn.normalize_or(Vec3::Y);
    let mut order: Vec<usize> = (0..len).collect();
    if n.y < 0.0 {
        n = -n;
        order.reverse();
    }
    let mut s = n.cross(e).normalize_or(Vec3::Z);
    if s.y < 0.0 {
        s = -s;
    }
    let up = Vec3::Y * tv;
    let concrete = palette::stone();
    for (side, normal) in [(true, n), (false, -n)] {
        let ord: Vec<usize> = if side { order.clone() } else { order.iter().rev().copied().collect() };
        let base_i = b.m.positions.len() as u32;
        for &i in &ord {
            let p = if side { pts[i] + up } else { pts[i] };
            let (c, sf) = if side { (col, surf) } else { (concrete * 0.7, Surface::Concrete) };
            b.m.vertex(p, normal, Vec2::new(p.dot(e), p.dot(s)), c, sf);
        }
        for k in 1..len - 1 {
            let (p0, p1, p2) = (pts[ord[0]], pts[ord[k]], pts[ord[k + 1]]);
            if (p1 - p0).cross(p2 - p0).length_squared() > 1e-10 {
                b.m.indices.extend_from_slice(&[base_i, base_i + k as u32, base_i + k as u32 + 1]);
            }
        }
    }
    for i in 0..len {
        if let Some(want) = fascia.get(i).copied().flatten() {
            let (a, c) = (pts[i], pts[(i + 1) % len]);
            let l = a.distance(c);
            let uv = [Vec2::ZERO, Vec2::new(l, 0.0), Vec2::new(l, tv), Vec2::new(0.0, tv)];
            b.face([a, c, c + up, a + up], uv, want, concrete * 0.85, Surface::Concrete);
        }
    }
}

/// Sharp slab gable: two thick planes meeting at a knife ridge, deep overhang all round, the
/// slab thickness showing at the open gable ends.
#[allow(clippy::too_many_arguments)]
fn ridge_roof(b: &mut B, rf: &Rf, base: f32, rise: f32, surf: Surface, tv: f32, col: Vec3, o: &RoofOpts) -> RoofGeom {
    let k = rise / rf.b.max(0.05);
    let oe = o.overhang.min(0.9 / k.max(1e-3));
    let ye = base - oe * k;
    let ridge = base + rise;
    let (bg, ag) = (rf.b + oe, rf.a + o.overhang);
    let (ax3, ac3) = (p3(rf.ax, 0.0), p3(rf.ac, 0.0));
    let w = |s: f32, t: f32, y: f32| rf.w(s, t, y);
    for sg in [1.0f32, -1.0] {
        let pts = [w(-ag, sg * bg, ye), w(ag, sg * bg, ye), w(ag, 0.0, ridge), w(-ag, 0.0, ridge)];
        roof_face(b, &pts, &[Some(ac3 * sg), Some(ax3), None, Some(-ax3)], tv, ax3, col * if sg > 0.0 { 1.0 } else { 0.94 }, surf);
    }
    for ss in [1.0f32, -1.0] {
        b.marker(MarkerKind::Perch, w(ss * ag * 0.92, 0.0, ridge + tv + 0.05), Vec3::Y);
    }
    if let Some((gs, gc)) = o.gable {
        for ss in [1.0f32, -1.0] {
            let sa = ss * rf.a;
            b.tri(w(sa, -rf.b, base), w(sa, rf.b, base), w(sa, 0.0, ridge), ax3 * ss, gc, gs);
        }
    }
    RoofGeom::Ridge { rf: *rf, base, rise, tv }
}

/// North-light sawtooth: monopitch slab bays along the long axis, each rising to a vertical
/// glazed strip (with steel mullions) facing +ridge axis.
#[allow(clippy::too_many_arguments)]
fn sawtooth_roof(b: &mut B, rf: &Rf, base: f32, pitch: f32, surf: Surface, tv: f32, col: Vec3, o: &RoofOpts) -> RoofGeom {
    let nb = ((rf.a * 2.0 / 3.0).round() as usize).clamp(1, 12);
    let bw = rf.a * 2.0 / nb as f32;
    let rise = (pitch * rf.b * 0.5).max(0.4).min(bw * 0.9).min(4.0);
    let ov = o.overhang.min(0.6);
    let bg = rf.b + ov;
    let (ax3, ac3) = (p3(rf.ax, 0.0), p3(rf.ac, 0.0));
    let w = |s: f32, t: f32, y: f32| rf.w(s, t, y);
    let top = base + rise;
    for j in 0..nb {
        let s0 = -rf.a + bw * j as f32;
        let s1 = s0 + bw;
        let s0e = if j == 0 { s0 - ov } else { s0 };
        let ye = base - (s0 - s0e) * rise / bw;
        let pts = [w(s0e, -bg, ye), w(s1, -bg, top), w(s1, bg, top), w(s0e, bg, ye)];
        roof_face(b, &pts, &[Some(-ac3), Some(ax3), Some(ac3), Some(-ax3)], tv, ac3, col, surf);
        let gy = if j + 1 < nb { base + tv } else { base };
        if top - gy > 0.1 {
            let (l, hgt) = (rf.b * 2.0, top - gy);
            let uv = [Vec2::ZERO, Vec2::new(l, 0.0), Vec2::new(l, hgt), Vec2::new(0.0, hgt)];
            b.face([w(s1, -rf.b, gy), w(s1, rf.b, gy), w(s1, rf.b, top), w(s1, -rf.b, top)], uv, ax3, Vec3::ONE, Surface::Window);
            let m = ((l / 1.2).round() as usize).clamp(1, 24);
            for i in 1..m {
                let t = -rf.b + l * i as f32 / m as f32;
                b.m.oriented_box(w(s1 + 0.03, t, (gy + top) * 0.5), Vec3::new(0.03, hgt * 0.5, 0.04), ax3, o.trim, Surface::Steel, 1.0);
            }
            b.marker(MarkerKind::WindowLight, w(s1 - 0.1, 0.0, (gy + top) * 0.5), ax3);
        }
        if let Some((gs, gc)) = o.gable {
            for sg in [1.0f32, -1.0] {
                b.tri(w(s0, sg * rf.b, base), w(s1, sg * rf.b, base), w(s1, sg * rf.b, top), ac3 * sg, gc, gs);
            }
        }
    }
    for s in [-rf.a + bw, rf.a] {
        b.marker(MarkerKind::Perch, w(s, 0.0, top + tv + 0.05), Vec3::Y);
    }
    RoofGeom::Saw { rf: *rf, base, rise, bw, tv }
}

/// Precast shell: a folded-plate pyramid (eave edges kink up at their midpoints) on polygons
/// with up to six sides, a shallow dome with a concrete oculus ring on rounder ones.
#[allow(clippy::too_many_arguments)]
fn shell_roof(b: &mut B, poly: &[Vec2], c: Vec2, edges: &[(Vec2, f32)], base: f32, rise: f32, col: Vec3, o: &RoofOpts) -> RoofGeom {
    let dmin = edges.iter().map(|e| e.1).fold(f32::INFINITY, f32::min).max(0.05);
    let k = rise / dmin;
    let oe = o.overhang.min(0.9 / k.max(1e-3));
    let ye = base - oe * k;
    let g = offset_poly(poly, oe);
    let n = g.len();
    let tv = 0.25;
    let surf = Surface::Precast;
    let apex = p3(c, base + rise);
    let mut top = apex.y + tv;
    if n <= 6 {
        let fold = rise * 0.22;
        for i in 0..n {
            let (a, q) = (g[i], g[(i + 1) % n]);
            let e = p3((q - a).normalize_or(Vec2::X), 0.0);
            let m = p3((a + q) * 0.5, ye + fold);
            let out = Some(p3(edges[i].0, 0.0));
            roof_face(b, &[p3(a, ye), m, apex], &[out, None, None], tv, e, col, surf);
            roof_face(b, &[m, p3(q, ye), apex], &[out, None, None], tv, e, col * 0.9, surf);
        }
    } else {
        let ring = |f: f32| -> Vec<Vec2> { g.iter().map(|p| c + (*p - c) * f).collect() };
        let levels = [(1.0f32, ye), (0.6, base + rise * 0.66), (0.2, base + rise)];
        for l in 0..2 {
            let ((fa, ya), (fb, yb)) = (levels[l], levels[l + 1]);
            let (ra, rb) = (ring(fa), ring(fb));
            for i in 0..n {
                let j = (i + 1) % n;
                let e = p3((g[j] - g[i]).normalize_or(Vec2::X), 0.0);
                let out = if l == 0 { Some(p3(edges[i].0, 0.0)) } else { None };
                roof_face(b, &[p3(ra[i], ya), p3(ra[j], ya), p3(rb[j], yb), p3(rb[i], yb)], &[out, None, None, None], tv, e, col, surf);
            }
        }
        let cap: Vec<Vec3> = ring(0.2).iter().map(|p| p3(*p, top)).collect();
        b.m.polygon(&cap, true, col * 0.95, surf, 1.0);
        let r = (dmin * 0.1).max(0.15);
        b.m.cylinder(p3(c, top - 0.02), r, r, 0.35, 8, true, false, palette::stone() * 0.9, Surface::Concrete, 1.0);
        top += 0.33;
    }
    b.marker(MarkerKind::Perch, p3(c, top + 0.05), Vec3::Y);
    RoofGeom::Pyramid { c, base, rise, edges: edges.to_vec(), tv }
}

/// Flat sedum roof with grass clumps: thick parapet (optionally crenellated with planter boxes,
/// optionally with solar panels) or, without parapet, a thin overhanging slab.
fn flat_roof(b: &mut B, poly: &[Vec2], base: f32, sedum: Vec3, o: &RoofOpts) -> RoofGeom {
    let stone = palette::stone();
    let leaves = palette::leaves();
    let slab_y = base + 0.2;
    let ov = if o.parapet { 0.03 } else { o.overhang.max(0.03) };
    let top = offset_poly(poly, ov);
    let tris = triangulate(&top);
    for t in &tris {
        b.tri(p3(top[t[0]], slab_y), p3(top[t[1]], slab_y), p3(top[t[2]], slab_y), Vec3::Y, sedum, Surface::Sedum);
    }
    wall_band(b, poly, 0.0, &Band::new(base - 0.1, slab_y, ov, Surface::Concrete, stone * 0.9), &[], Vec3::ONE);
    if !o.parapet {
        for t in &tris {
            b.tri(p3(top[t[0]], base - 0.1), p3(top[t[1]], base - 0.1), p3(top[t[2]], base - 0.1), -Vec3::Y, stone * 0.7, Surface::Concrete);
        }
        clumps(b, poly, slab_y, sedum, o.seed, 0.3);
        b.marker(MarkerKind::Perch, p3(poly[0], slab_y + 0.05), Vec3::Y);
        return RoofGeom::Flat { top: slab_y };
    }
    let (out, inn) = (0.1, -0.4);
    let p_lo = base - 0.1;
    let p_hi = slab_y + 0.6;
    wall_band(b, poly, 0.0, &Band::new(p_lo, p_hi, out, Surface::Concrete, stone), &[], Vec3::ONE);
    let mut inner = Band::new(slab_y, p_hi, inn, Surface::Concrete, stone * 0.85);
    inner.inward = true;
    wall_band(b, poly, 0.0, &inner, &[], Vec3::ONE);
    annulus(b, poly, 0.0, p_hi, out, inn, true, &[], stone * 0.95, Surface::Concrete);
    clumps(b, poly, slab_y, sedum, o.seed, 0.8);
    let mut perch_y = p_hi;
    if o.planters {
        let offm = offset_poly(poly, (out + inn) * 0.5);
        let hd = (out - inn) * 0.5;
        let mut rng = Rng::at(o.seed ^ 0x9A47_0000, poly[0].x, poly[0].y);
        for i in 0..poly.len() {
            let wf = Wf::new(poly, &offm, i, 0.0);
            let l = wf.u_end() - wf.u_start();
            if l < 0.3 {
                continue;
            }
            let count = ((l / 1.2).round() as usize).clamp(1, 64);
            let seg = l / count as f32;
            for j in 0..count {
                let u = wf.u_start() + seg * (j as f32 + 0.5);
                let hw = (seg * 0.3).min(0.35);
                b.lbox(&wf, u, p_hi + 0.2, 0.0, Vec3::new(hw, 0.2, hd), stone * 0.95, Surface::Concrete);
                b.lbox(&wf, u, p_hi + 0.43, 0.0, Vec3::new(hw * 0.9, 0.05, hd * 0.85), leaves, Surface::Leaves);
                let sw = hw * rng.range(0.4, 0.9);
                let drop = rng.range(0.3, 1.1);
                b.wquad(&wf, u - sw, u + sw, p_hi + 0.35 - drop, p_hi + 0.44, hd + 0.02, false, leaves * 0.8, Surface::Fiber);
            }
        }
        perch_y += 0.5;
    }
    if o.solar {
        solar_panels(b, poly, slab_y);
    }
    let mut rng = Rng::at(o.seed ^ 0xF1A7, poly[0].x, poly[0].y);
    let k = (rng.f() * poly.len() as f32) as usize % poly.len();
    b.marker(MarkerKind::Perch, p3(offset_poly(poly, (out + inn) * 0.5)[k], perch_y + 0.02), Vec3::Y);
    RoofGeom::Flat { top: slab_y }
}

/// Grass and shrub clumps scattered on a sedum mat, at least `margin` inside the outline.
fn clumps(b: &mut B, poly: &[Vec2], y: f32, col: Vec3, seed: u64, margin: f32) {
    let c = area_centroid(poly);
    let mut rng = Rng::at(seed ^ 0xC1A4_0000, c.x, c.y);
    let (lo, hi) = poly.iter().fold((Vec2::splat(f32::INFINITY), Vec2::splat(f32::NEG_INFINITY)), |(l, h), p| (l.min(*p), h.max(*p)));
    let want = ((signed_area(poly).abs() / 6.0) as usize).clamp(2, 24);
    let ring = closed(poly);
    let mut placed = 0;
    for _ in 0..want * 3 {
        if placed >= want {
            break;
        }
        let p = Vec2::new(rng.range(lo.x, hi.x), rng.range(lo.y, hi.y));
        if !contains(poly, p) || closest_on_polyline(&ring, p).1 < margin {
            continue;
        }
        let hw = rng.range(0.2, 0.45);
        let ht = rng.range(0.08, 0.3);
        let ang = rng.range(0.0, PI);
        let shade = col.lerp(palette::leaves(), 0.5) * rng.range(0.75, 1.1);
        let half = Vec3::new(hw, ht * 0.5, hw * rng.range(0.6, 1.0));
        b.m.oriented_box(p3(p, y + ht * 0.5), half, Vec3::new(ang.cos(), 0.0, ang.sin()), shade, Surface::Leaves, 1.0);
        placed += 1;
    }
}

/// Rows of tilted solar panels on a flat rectangular roof.
fn solar_panels(b: &mut B, poly: &[Vec2], y: f32) {
    let Some(rf) = rect_frame(poly) else { return };
    let pw = (rf.a - 1.2).min(2.5);
    let rows = ((rf.b * 2.0 - 1.6) / 1.6).floor().clamp(0.0, 6.0) as usize;
    if pw < 0.4 {
        return;
    }
    let (panel, frame) = (palette::metal() * 1.3, palette::metal());
    for r in 0..rows {
        let t = -rf.b + 0.9 + r as f32 * 1.6;
        let q = [rf.w(-pw, t, y + 0.25), rf.w(pw, t, y + 0.25), rf.w(pw, t + 0.9, y + 0.75), rf.w(-pw, t + 0.9, y + 0.75)];
        let uv = [Vec2::ZERO, Vec2::new(pw * 2.0, 0.0), Vec2::new(pw * 2.0, 1.0), Vec2::new(0.0, 1.0)];
        b.face(q, uv, Vec3::Y, panel, Surface::Metal);
        b.face(q, uv, -Vec3::Y, frame, Surface::Metal);
        for s in [-pw + 0.15, pw - 0.15] {
            b.m.oriented_box(rf.w(s, t + 0.75, y + 0.35), Vec3::new(0.03, 0.3, 0.03), p3(rf.ax, 0.0), frame, Surface::Steel, 1.0);
        }
    }
}

// =============================================================================================
// Vent stack, pavilion, decor
// =============================================================================================

/// Ventilation stack near a roof corner: a square concrete shaft under a cap slab on steel
/// posts, or a steel flue with a collar. Emits steam (`ChimneySmoke`).
fn vent_stack(b: &mut B, rf: &Rf, geom: &RoofGeom, eave: f32, slab: Vec3, trim: Vec3, seed: u64) {
    let mut rng = Rng::at(seed ^ 0xC417_0000, rf.c.x, rf.c.y);
    let s1 = if rng.f() < 0.5 { -1.0 } else { 1.0 };
    let s2 = if rng.f() < 0.5 { -1.0 } else { 1.0 };
    let s = s1 * (rf.a - 0.75).max(0.0);
    let flat = matches!(geom, RoofGeom::Flat { .. });
    let t = s2 * if flat { (rf.b - 0.7).max(0.0) } else { (rf.b * 0.3).min(rf.b - 0.45).max(0.0) };
    let p = rf.c + rf.ax * s + rf.ac * t;
    let roof_y = geom.top_at(p).unwrap_or(eave);
    let top = (roof_y + if flat { 1.9 } else { 1.0 }).max(eave + 1.2);
    let bot = eave - 0.5;
    let ax = p3(rf.ax, 0.0);
    if rng.f() < 0.5 {
        b.m.oriented_box(p3(p, (bot + top) * 0.5), Vec3::new(0.3, (top - bot) * 0.5, 0.3), ax, slab * 0.95, Surface::Concrete, 1.0);
        b.m.oriented_box(p3(p, top + 0.3), Vec3::new(0.42, 0.06, 0.42), ax, slab, Surface::Concrete, 1.0);
        let ac = p3(rf.ac, 0.0);
        for (sx, sz) in [(-1.0f32, -1.0f32), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)] {
            b.m.oriented_box(p3(p, top + 0.12) + ax * 0.26 * sx + ac * 0.26 * sz, Vec3::new(0.03, 0.12, 0.03), ax, trim, Surface::Steel, 1.0);
        }
    } else {
        b.m.cylinder(p3(p, bot), 0.22, 0.22, top - bot, 8, false, false, trim, Surface::Steel, 1.0);
        b.m.cylinder(p3(p, top - 0.15), 0.27, 0.27, 0.15, 8, true, true, palette::metal(), Surface::Metal, 1.0);
    }
    b.marker(MarkerKind::ChimneySmoke, p3(p, top + 0.5), Vec3::Y);
}

/// Very low walls: a pavilion, i.e. a roof slab on slender concrete pilotis.
#[allow(clippy::too_many_arguments)]
fn pavilion(b: &mut B, poly: &[Vec2], y0: f32, h: f32, style: RoofStyle, pitch: f32, colors: Colors, terrain: &Terrain, seed: u64, center: Vec2) {
    let slab = palette::stone();
    let canopy = y0 + 2.2 + h * 0.6;
    let n = poly.len();
    let step = if n <= 4 { 1 } else { (n / 6).max(1) };
    let corners: Vec<Vec2> = (0..n).step_by(step).map(|i| poly[i] + (center - poly[i]).normalize_or_zero() * 0.35).collect();
    let mut cols = corners.clone();
    if n <= 4 {
        for i in 0..corners.len() {
            let (a, q) = (corners[i], corners[(i + 1) % corners.len()]);
            let k = ((a.distance(q) / 3.5) as usize).min(8);
            cols.extend((1..=k).map(|j| a.lerp(q, j as f32 / (k + 1) as f32)));
        }
    }
    for q in &cols {
        let gy = fin(terrain.height(*q), y0).min(canopy - 0.3);
        b.m.cylinder(p3(*q, gy - SINK), 0.11, 0.11, canopy - gy + SINK, 6, false, false, slab * 0.95, Surface::Concrete, 1.0);
    }
    let opts = RoofOpts { overhang: 0.6, gable: None, planters: false, parapet: false, solar: false, trim: palette::trim(colors), seed };
    roof_impl(b, poly, canopy, style, pitch, colors, &opts);
    // A cast bench under the canopy.
    let mut rng = Rng::at(seed ^ 0x7E47, center.x, center.y);
    if rng.f() < 0.6 {
        let gy = fin(terrain.height(center), y0);
        let ang = rng.range(0.0, PI);
        let x = Vec3::new(ang.cos(), 0.0, ang.sin());
        b.m.oriented_box(Vec3::new(center.x, gy + 0.2, center.y), Vec3::new(0.8, 0.22, 0.22), x, slab * 0.85, Surface::Concrete, 1.0);
    }
    // Folk gather under the pavilion's front.
    let mut best = 0;
    for i in 1..n {
        if poly[i].distance(poly[(i + 1) % n]) > poly[best].distance(poly[(best + 1) % n]) + 1e-3 {
            best = i;
        }
    }
    let (a, q) = (poly[best], poly[(best + 1) % n]);
    let nrm = outward((q - a).normalize_or(Vec2::X));
    let spot = (a + q) * 0.5 + nrm * 0.8;
    b.marker(MarkerKind::Door, p3(spot, fin(terrain.height(spot), y0)), p3(nrm, 0.0));
}

/// Rain chain from an eave corner into a gravel basin, a planter trough with shrubs, and a
/// balcony slab with steel railings on multi-storey blocks.
#[allow(clippy::too_many_arguments)]
fn decor(
    b: &mut B,
    poly: &[Vec2],
    lens: &[f32],
    y0: f32,
    h: f32,
    cant: f32,
    fins: bool,
    holes: &[Hole],
    slab: Vec3,
    trim: Vec3,
    og: f32,
    terrain: &Terrain,
    seed: u64,
) {
    let n = poly.len();
    let c = poly.iter().copied().sum::<Vec2>() / n as f32;
    let mut rng = Rng::at(seed ^ 0xDEC0_0000, c.x, c.y);
    let leaves = palette::leaves();
    let metal = palette::metal();
    let door_edge = holes.iter().find(|o| o.kind.is_door()).map_or(usize::MAX, |o| o.edge);
    let door_near = |e: usize, u0: f32, u1: f32| holes.iter().any(|o| o.edge == e && o.kind.is_door() && o.u0 - 1.4 < u1 && u0 < o.u1 + 1.4);
    let reach = if fins { FIN + 0.15 } else { 0.0 };
    let ground = |p: Vec2| fin(terrain.height(p), y0);

    // Rain chain at a corner of a rectangular block.
    if n == 4 && rng.f() < 0.75 {
        let k = ((rng.f() * n as f32) as usize).min(n - 1);
        let prev = (k + n - 1) % n;
        if !door_near(k, 0.0, 0.3) && !door_near(prev, lens[prev] - 0.3, lens[prev]) {
            let n0 = outward((poly[k] - poly[prev]).normalize_or_zero());
            let n1 = outward((poly[(k + 1) % n] - poly[k]).normalize_or_zero());
            let p = poly[k] + (n0 + n1) * (cant + reach + 0.35);
            let top = y0 + h - 0.12;
            let g = ground(p);
            b.m.beam(p3(poly[k] + (n0 + n1) * cant, top - 0.05), p3(p, top - 0.05), 0.08, 0.08, metal, Surface::Metal, 1.0);
            let links = ((top - g - 0.12) / 0.3).max(0.0) as usize;
            for j in 0..links.min(90) {
                let ax = if j % 2 == 0 { p3(n0, 0.0) } else { p3(n1, 0.0) };
                b.m.oriented_box(p3(p, g + 0.12 + j as f32 * 0.3), Vec3::new(0.05, 0.08, 0.025), ax, metal, Surface::Metal, 1.0);
            }
            b.m.oriented_box(p3(p, g + 0.02), Vec3::new(0.32, 0.1, 0.32), p3(n1, 0.0), slab * 0.8, Surface::Plinth, 1.0);
        }
    }

    // Planter trough with shrubs along a wall without a door.
    let mut e = ((rng.f() * n as f32) as usize).min(n - 1);
    if e == door_edge {
        e = (e + n / 2) % n;
    }
    let wf = Wf::new(poly, poly, e, y0);
    if rng.f() < 0.65 && wf.len >= 2.2 {
        let hl = rng.range(0.6, 1.2).min(wf.len * 0.5 - 0.4);
        let u = rng.range(hl + 0.3, wf.len - hl - 0.3);
        if !door_near(e, u - hl, u + hl) {
            let out = reach + 0.4;
            let gp = wf.at(u, 0.0, out);
            let gy = ground(Vec2::new(gp.x, gp.z)) - y0;
            b.lbox(&wf, u, gy + 0.2, out, Vec3::new(hl, 0.26, 0.3), slab * 0.85, Surface::Plinth);
            b.lbox(&wf, u, gy + 0.47, out, Vec3::new(hl - 0.05, 0.03, 0.26), leaves * 0.8, Surface::Leaves);
            for _ in 0..2 + (rng.f() * 2.0) as usize {
                let su = u + rng.range(-hl + 0.25, hl - 0.25);
                let r = rng.range(0.18, 0.32);
                b.lbox(&wf, su, gy + 0.5 + r, out + rng.range(-0.08, 0.08), Vec3::new(r, r, r * 0.9), leaves * rng.range(0.75, 1.1), Surface::Leaves);
            }
            let sw = hl * rng.range(0.3, 0.8);
            b.wquad(&wf, u - sw, u + sw, gy + 0.1 + rng.range(0.0, 0.2), gy + 0.47, out + 0.31, false, leaves * 0.7, Surface::Fiber);
        }
    }

    // Balcony on the first floor slab, opposite the door.
    if n == 4 && h >= 5.5 && rng.f() < 0.65 {
        let e = if door_edge < n { (door_edge + 2) % n } else { e };
        let wf = Wf::at_off(poly, cant, e, y0);
        if wf.len >= 3.6 {
            let u = wf.len * 0.5 + rng.range(-0.15, 0.15) * (wf.len - 3.4);
            let y = STOREY + SLAB_HI;
            let depth = 1.4;
            b.lbox(&wf, u, y - 0.12, depth * 0.5, Vec3::new(1.3, 0.12, depth * 0.5), slab, Surface::Concrete);
            let corners = [wf.at(u - 1.25, y, 0.0), wf.at(u - 1.25, y, depth - 0.05), wf.at(u + 1.25, y, depth - 0.05), wf.at(u + 1.25, y, 0.0)];
            for pair in corners.windows(2) {
                for ry in [0.5, 1.0] {
                    b.m.beam(pair[0] + Vec3::Y * ry, pair[1] + Vec3::Y * ry, 0.05, 0.05, trim, Surface::Steel, 1.0);
                }
                let k = ((pair[0].distance(pair[1]) / 0.6).ceil() as usize).clamp(1, 16);
                for j in 0..=k {
                    let p = pair[0].lerp(pair[1], j as f32 / k as f32);
                    b.m.oriented_box(p + Vec3::Y * 0.5, Vec3::new(0.025, 0.5, 0.025), wf.d, trim, Surface::Steel, 1.0);
                }
            }
            if rng.f() < 0.4 + 0.5 * og {
                b.lbox(&wf, u + 0.95, y + 0.25, depth - 0.35, Vec3::new(0.22, 0.25, 0.22), slab * 0.9, Surface::Concrete);
                b.lbox(&wf, u + 0.95, y + 0.65, depth - 0.35, Vec3::new(0.28, 0.2, 0.26), leaves, Surface::Fiber);
            }
        }
    }
}

// =============================================================================================
// Tests
// =============================================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Opening;

    fn ground() -> Terrain {
        Terrain::build(1, &[], &[])
    }

    fn rect(hx: f32, hz: f32) -> Footprint {
        Footprint::Rect { center: Vec2::new(2.0, -3.0), half: Vec2::new(hx, hz), angle: 0.4 }
    }

    fn house(fp: Footprint, h: f32, roof: RoofStyle, pitch: f32, material: Material, fins: bool) -> House {
        House {
            footprint: fp,
            wall_height: h,
            roof,
            roof_pitch: pitch,
            material,
            fins,
            foundation: 0.4,
            planters: true,
            openings: vec![],
            colors: Colors { roof: 0, wall: 1, trim: 2 },
            overgrowth: 0.5,
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
        let (lo, _) = m.bounds().unwrap();
        assert!(lo.y >= -1.0, "below ground: {lo:?}");
        assert!(mk.iter().all(|k| k.pos.is_finite() && k.dir.is_finite()));
    }

    #[test]
    fn every_combination_is_valid() {
        let t = ground();
        let fps = [
            rect(3.0, 4.0),
            rect(0.6, 0.6),
            Footprint::Round { center: Vec2::new(-4.0, 1.0), radius: 1.8 },
            Footprint::Round { center: Vec2::new(5.0, 5.0), radius: 3.0 },
        ];
        let mats = [Material::Concrete, Material::RammedEarth, Material::Timber];
        let roofs = [RoofStyle::Pitched, RoofStyle::Sawtooth, RoofStyle::Shell, RoofStyle::Green];
        let mut rc = 0u8;
        for fp in fps {
            for h in [0.8, 3.6, 7.0, 12.0] {
                for roof in roofs {
                    for mat in mats {
                        for fins in [false, true] {
                            for planters in [false, true] {
                                for og in [0.0, 1.0] {
                                    for pitch in [0.0, 0.8] {
                                        let mut hs = house(fp, h, roof, pitch, mat, fins);
                                        hs.planters = planters;
                                        hs.overgrowth = og;
                                        rc = (rc + 1) % ROOF_COLORS;
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
        }
    }

    #[test]
    fn block_has_door_vent_windows_light() {
        let t = ground();
        let (m, mk) = house_mesh(&house(rect(3.0, 4.0), 3.5, RoofStyle::Pitched, 0.8, Material::RammedEarth, false), &[], &t, 3);
        check(&m, &mk);
        assert_eq!(count(&mk, MarkerKind::Door), 1);
        assert_eq!(count(&mk, MarkerKind::ChimneySmoke), 1);
        assert!(count(&mk, MarkerKind::WindowLight) >= 4);
        assert!(count(&mk, MarkerKind::Lantern) >= 1, "entry canopy light");
        assert!(count(&mk, MarkerKind::Perch) >= 2);
        assert!(has(&m, Surface::Earth) && has(&m, Surface::Plinth) && has(&m, Surface::Window));
        // Steam comes out well above the eaves.
        let steam = mk.iter().find(|k| k.kind == MarkerKind::ChimneySmoke).unwrap();
        assert!(steam.pos.y > 3.5 + 1.0);
    }

    #[test]
    fn materials_and_covers() {
        let t = ground();
        let fp = rect(3.0, 4.0);
        let (m, _) = house_mesh(&house(fp, 7.0, RoofStyle::Pitched, 0.8, Material::Concrete, false), &[], &t, 1);
        assert!(has(&m, Surface::Concrete) && has(&m, Surface::Sedum), "roof colour 0 → sedum slab");
        let mut hs = house(fp, 7.0, RoofStyle::Pitched, 0.8, Material::Timber, false);
        hs.colors.roof = 3;
        let (m, _) = house_mesh(&hs, &[], &t, 1);
        assert!(has(&m, Surface::Cladding) && has(&m, Surface::Seam) && has(&m, Surface::Concrete), "timber + slab bands + seam");
        let (m, _) = house_mesh(&house(fp, 4.0, RoofStyle::Shell, 0.8, Material::Concrete, false), &[], &t, 1);
        assert!(has(&m, Surface::Precast));
    }

    #[test]
    fn silo_towers() {
        let t = ground();
        let fp = Footprint::Round { center: Vec2::new(0.0, 0.0), radius: 2.5 };
        let (m, mk) = house_mesh(&house(fp, 11.0, RoofStyle::Shell, 1.5, Material::Concrete, false), &[], &t, 4);
        check(&m, &mk);
        assert!(has(&m, Surface::Precast), "dome shell");
        assert!(count(&mk, MarkerKind::Perch) >= 1);
        assert!(count(&mk, MarkerKind::WindowLight) >= 4, "slit windows");
        assert_eq!(count(&mk, MarkerKind::Door), 1);
        assert_eq!(count(&mk, MarkerKind::ChimneySmoke), 0);
        let perch = mk.iter().find(|k| k.kind == MarkerKind::Perch).unwrap();
        assert!(perch.pos.y > 11.0 + 1.0);
        // Ribs stand proud of the shell.
        let (lo, hi) = m.bounds().unwrap();
        assert!(hi.x > 2.6 && lo.x < -2.6);
        // Green silo: parapet planters, perch on top.
        let (m, mk) = house_mesh(&house(fp, 9.0, RoofStyle::Green, 0.0, Material::Concrete, false), &[], &t, 4);
        check(&m, &mk);
        assert!(has(&m, Surface::Sedum));
        let perch = mk.iter().find(|k| k.kind == MarkerKind::Perch).unwrap();
        assert!(perch.pos.y > 9.5);
    }

    #[test]
    fn manual_openings_respected() {
        let t = ground();
        let mut hs = house(rect(3.0, 4.0), 3.0, RoofStyle::Pitched, 0.6, Material::Concrete, false);
        hs.openings = vec![
            Opening { along: 3.0, kind: OpeningKind::Window },
            Opening { along: 11.0, kind: OpeningKind::Window },
            Opening { along: 17.0, kind: OpeningKind::Door },
            Opening { along: 24.0, kind: OpeningKind::Window },
        ];
        let (m, mk) = house_mesh(&hs, &[], &t, 5);
        check(&m, &mk);
        assert_eq!(count(&mk, MarkerKind::WindowLight), 3);
        assert_eq!(count(&mk, MarkerKind::Door), 1);
        hs.openings = vec![Opening { along: 5.0, kind: OpeningKind::WideDoor }];
        let (m, mk) = house_mesh(&hs, &[], &t, 5);
        check(&m, &mk);
        assert_eq!(count(&mk, MarkerKind::WindowLight), 0);
        assert_eq!(count(&mk, MarkerKind::Door), 1);
    }

    #[test]
    fn cut_becomes_door() {
        let t = ground();
        let mut hs = house(rect(3.0, 4.0), 3.0, RoofStyle::Pitched, 0.6, Material::RammedEarth, false);
        hs.openings = vec![Opening { along: 2.0, kind: OpeningKind::Window }];
        let (_, mk) = house_mesh(&hs, &[], &t, 5);
        assert_eq!(count(&mk, MarkerKind::Door), 0);
        let cuts = [Cut { along: 15.0, width: 1.6, height: 2.4 }, Cut { along: 25.0, width: 3.0, height: 2.4 }];
        let (m, mk) = house_mesh(&hs, &cuts, &t, 5);
        check(&m, &mk);
        assert_eq!(count(&mk, MarkerKind::Door), 2);
        assert_eq!(count(&mk, MarkerKind::WindowLight), 1);
        // Auto blocks: the cut replaces the automatic door.
        hs.openings.clear();
        let (_, mk) = house_mesh(&hs, &cuts[..1], &t, 5);
        assert_eq!(count(&mk, MarkerKind::Door), 1);
    }

    #[test]
    fn pavilion_on_pilotis() {
        let t = ground();
        for fp in [rect(2.0, 3.0), Footprint::Round { center: Vec2::ZERO, radius: 2.0 }] {
            for roof in [RoofStyle::Pitched, RoofStyle::Green, RoofStyle::Sawtooth, RoofStyle::Shell] {
                let (m, mk) = house_mesh(&house(fp, 0.7, roof, 0.3, Material::Timber, false), &[], &t, 6);
                check(&m, &mk);
                let (_, hi) = m.bounds().unwrap();
                assert!(hi.y > 2.4, "canopy above head height");
                assert!(!has(&m, Surface::Cladding) && !has(&m, Surface::Door), "no walls, just pilotis");
                assert_eq!(count(&mk, MarkerKind::Door), 1);
                if roof != RoofStyle::Sawtooth {
                    assert_eq!(count(&mk, MarkerKind::WindowLight), 0);
                }
            }
        }
    }

    #[test]
    fn sawtooth_has_glazing() {
        let t = ground();
        let (m, mk) = house_mesh(&house(rect(3.0, 6.0), 4.0, RoofStyle::Sawtooth, 1.0, Material::Concrete, false), &[], &t, 2);
        check(&m, &mk);
        assert!(has(&m, Surface::Window));
        let glazing = m.surfaces.iter().zip(&m.positions).filter(|(s, p)| **s == Surface::Window as u8 && p[1] > 4.0).count();
        assert!(glazing >= 4 * 4, "north-light strips above the eaves");
        let square = vec![Vec2::new(0.0, 0.0), Vec2::new(6.0, 0.0), Vec2::new(6.0, 9.0), Vec2::new(0.0, 9.0)];
        let (m, _) = roof_mesh(&square, 3.0, RoofStyle::Sawtooth, 0.8, Colors::default(), 1);
        assert!(has(&m, Surface::Window));
    }

    #[test]
    fn roof_mesh_on_rings() {
        // Concave L-shaped ring (centroid sees every edge? no → flat), and a star-shaped one.
        let l_ring = vec![
            Vec2::new(0.0, 0.0),
            Vec2::new(10.0, 0.0),
            Vec2::new(10.0, 3.0),
            Vec2::new(3.0, 3.0),
            Vec2::new(3.0, 10.0),
            Vec2::new(0.0, 10.0),
        ];
        let blob: Vec<Vec2> =
            (0..9).map(|i| Vec2::from_angle(i as f32 / 9.0 * std::f32::consts::TAU) * (5.0 + (i % 3) as f32)).collect();
        let square = vec![Vec2::new(0.0, 0.0), Vec2::new(6.0, 0.0), Vec2::new(6.0, 9.0), Vec2::new(0.0, 9.0)];
        for ring in [&l_ring, &blob, &square] {
            for style in [RoofStyle::Pitched, RoofStyle::Sawtooth, RoofStyle::Shell, RoofStyle::Green] {
                for pitch in [0.0, 0.6, 3.0, f32::NAN] {
                    let (m, mk) = roof_mesh(ring, 4.0, style, pitch, Colors { roof: 4, wall: 0, trim: 0 }, 7);
                    assert!(m.is_valid() && !m.is_empty(), "{style:?} {pitch}");
                    assert!(!mk.is_empty());
                    let (lo, hi) = m.bounds().unwrap();
                    assert!(lo.y > 2.5 && hi.y > 4.0);
                }
            }
        }
        // Clockwise and degenerate input.
        let mut cw = square.clone();
        cw.reverse();
        let (m, _) = roof_mesh(&cw, 2.0, RoofStyle::Pitched, 0.6, Colors::default(), 1);
        assert!(m.is_valid() && !m.is_empty());
        let (m, _) = roof_mesh(&[Vec2::ZERO, Vec2::X], 2.0, RoofStyle::Pitched, 0.6, Colors::default(), 1);
        assert!(m.is_valid());
        let (m, _) = roof_mesh(&[Vec2::ZERO, Vec2::X, Vec2::new(2.0, 0.0)], 2.0, RoofStyle::Sawtooth, 0.6, Colors::default(), 1);
        assert!(m.is_valid());
        // Slab gable on a rectangle: ridge above the eaves.
        let (m, mk) = roof_mesh(&square, 4.0, RoofStyle::Pitched, 1.0, Colors::default(), 1);
        assert!(m.bounds().unwrap().1.y > 4.0 + 2.9);
        assert!(count(&mk, MarkerKind::Perch) >= 2);
    }

    #[test]
    fn decor_stands_on_ground() {
        use crate::model::{TerrainDab, TerrainOp};
        let slope = [TerrainDab { center: Vec2::new(2.0, -3.0), radius: 9.0, op: TerrainOp::Sculpt { amount: 2.0 } }];
        let t = Terrain::build(5, &slope, &[]);
        let fp = rect(3.0, 2.2);
        let poly = footprint_polygon(&fp);
        let mut seen = 0;
        for seed in 0..80 {
            let hs = house(fp, 5.5, RoofStyle::Pitched, 0.8, Material::RammedEarth, seed % 2 == 0);
            let (m, _) = house_mesh(&hs, &[], &t, seed);
            // Ground-decor triangles (planters, rain chains, basins) clear of the walls.
            let mut lowest = f32::INFINITY;
            let mut any = false;
            for tri in m.indices.chunks(3) {
                let s = m.surfaces[tri[0] as usize];
                if ![Surface::Leaves, Surface::Metal, Surface::Plinth].iter().any(|k| *k as u8 == s) {
                    continue;
                }
                let ps: Vec<Vec3> = tri.iter().map(|&i| Vec3::from_array(m.positions[i as usize])).collect();
                let c = (ps[0] + ps[1] + ps[2]) / 3.0;
                let c2 = Vec2::new(c.x, c.z);
                if crate::geom::contains(&poly, c2) || crate::geom::closest_on_polyline(&crate::geom::closed(&poly), c2).1 < 0.6 {
                    continue;
                }
                any = true;
                lowest = lowest.min(ps.iter().map(|p| p.y - t.height(Vec2::new(p.x, p.z))).fold(f32::INFINITY, f32::min));
            }
            if any {
                seen += 1;
                assert!(lowest < 0.3 && lowest > -0.6, "seed {seed}: decor lowest point is {lowest} m from the ground");
            }
        }
        assert!(seen > 5, "decor expected on several seeds");
    }

    #[test]
    fn deterministic() {
        let t = ground();
        let mut hs = house(rect(3.0, 4.0), 7.5, RoofStyle::Green, 0.0, Material::RammedEarth, true);
        hs.overgrowth = 0.8;
        let a = house_mesh(&hs, &[], &t, 11);
        let b = house_mesh(&hs, &[], &t, 11);
        assert_eq!(a.0, b.0);
        assert_eq!(a.1, b.1);
        // Different seeds may decorate differently, but stay valid.
        let c = house_mesh(&hs, &[], &t, 12);
        check(&c.0, &c.1);
    }

    #[test]
    fn overgrowth_adds_greenery() {
        let t = ground();
        let green = |og: f32| {
            let mut hs = house(rect(3.0, 4.0), 7.0, RoofStyle::Pitched, 0.8, Material::Concrete, false);
            hs.overgrowth = og;
            (0..6).map(|s| house_mesh(&hs, &[], &t, s).0.surfaces.iter().filter(|v| **v == Surface::Fiber as u8).count()).sum::<usize>()
        };
        assert!(green(1.0) > green(0.0) + 100);
    }

    #[test]
    fn bounds_sanity() {
        let t = ground();
        for (roof, min_rise) in [(RoofStyle::Pitched, 2.1), (RoofStyle::Sawtooth, 0.6), (RoofStyle::Shell, 2.1)] {
            for fins in [false, true] {
                let h = 4.0;
                let (m, _) = house_mesh(&house(rect(3.0, 4.0), h, roof, 0.8, Material::RammedEarth, fins), &[], &t, 2);
                let (lo, hi) = m.bounds().unwrap();
                assert!(lo.y >= -1.0);
                assert!(hi.y > h + min_rise, "{roof:?}: apex {hi:?}");
            }
        }
        let (m, _) = house_mesh(&house(rect(3.0, 4.0), 4.0, RoofStyle::Green, 0.0, Material::Concrete, false), &[], &t, 2);
        let (_, hi) = m.bounds().unwrap();
        assert!(hi.y > 4.0 && hi.y < 7.0);
    }

    #[test]
    fn triangle_budget() {
        let t = ground();
        for roof in [RoofStyle::Pitched, RoofStyle::Sawtooth, RoofStyle::Green] {
            for seed in 0..8 {
                let mut hs = house(rect(3.0, 4.0), 9.0, roof, 1.0, Material::RammedEarth, true);
                hs.colors.roof = (seed % ROOF_COLORS as u64) as u8;
                hs.overgrowth = 1.0;
                let (m, _) = house_mesh(&hs, &[], &t, seed);
                let tris = m.indices.len() / 3;
                assert!(tris <= 6000, "{tris} triangles");
            }
        }
    }

    #[test]
    fn fins_slabs_and_cantilever() {
        let t = ground();
        let plain = house(rect(3.0, 4.5), 7.0, RoofStyle::Pitched, 1.0, Material::Concrete, false);
        let (m0, mk) = house_mesh(&plain, &[], &t, 8);
        check(&m0, &mk);
        assert!(count(&mk, MarkerKind::WindowLight) >= 8, "two storeys of windows");
        let mut finned = plain.clone();
        finned.fins = true;
        let (m1, mk) = house_mesh(&finned, &[], &t, 8);
        check(&m1, &mk);
        assert!(m1.vertex_count() > m0.vertex_count() + 24 * 20, "fin grid");
        // Upper storey cantilevers beyond the footprint: walls reach outside the ground outline.
        let poly = footprint_polygon(&plain.footprint);
        let ring = crate::geom::closed(&poly);
        let out = m0
            .positions
            .iter()
            .zip(&m0.surfaces)
            .filter(|(p, s)| **s == Surface::Concrete as u8 && p[1] > 4.0 && p[1] < 6.5)
            .map(|(p, _)| crate::geom::closest_on_polyline(&ring, Vec2::new(p[0], p[2])).1)
            .fold(0.0f32, f32::max);
        assert!(out > 0.25, "cantilever {out}");
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
            for h in [0.0, f32::NAN, 1.49, 1.5, 24.0, 100.0] {
                let mut hs = house(fp, h, RoofStyle::Pitched, f32::NAN, Material::RammedEarth, true);
                hs.foundation = f32::NAN;
                hs.overgrowth = f32::NAN;
                hs.openings = vec![Opening { along: f32::NAN, kind: OpeningKind::Door }, Opening { along: -5.0, kind: OpeningKind::Window }];
                let cuts = [Cut { along: f32::INFINITY, width: f32::NAN, height: 0.0 }, Cut { along: 1e6, width: 9.0, height: 2.0 }];
                let (m, mk) = house_mesh(&hs, &cuts, &t, 0);
                check(&m, &mk);
            }
        }
    }
}
