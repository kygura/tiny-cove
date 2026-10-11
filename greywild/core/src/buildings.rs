//! Houses, towers and roofs.
//!
//! A house is built from horizontal *bands* of wall (foundation plinth, stone ground storey,
//! jettied half-timbered upper storey, tapering tower drums…). Each band is a shell of flat
//! quads following a (possibly offset) footprint polygon, with rectangular *holes* cut for
//! windows and doors; every hole gets reveal faces and a recessed window / door panel.
//! Details (quoins, timbers, shutters, window boxes, arches, lanterns, ivy, chimney, props)
//! are small boxes on top. Roofs are generic over any simple CCW polygon (`roof_mesh`).
//!
//! All randomness hashes `(seed, world position)` so previews equal committed results.

use crate::geom::{contains, footprint_polygon, outward, signed_area};
use crate::mesh::{Marker, MarkerKind, MeshData, Rng, Surface};
use crate::model::{Colors, Footprint, House, Material, OpeningKind, RoofStyle};
use crate::palette;
use crate::terrain::Terrain;
use crate::walls::Cut;
use glam::{Vec2, Vec3};
use std::f32::consts::PI;

/// Height of one storey (floor to floor).
const STOREY: f32 = 2.6;
/// Roof overhang beyond the wall line.
const OVERHANG: f32 = 0.35;
/// Outward step of a jettied upper storey.
const JETTY: f32 = 0.2;
/// Walls reach this far below the lowest ground corner.
const SINK: f32 = 0.3;
/// Tower drum height between string courses.
const TOWER_SEG: f32 = 3.0;
/// Inward step of each tower drum.
const TAPER: f32 = 0.07;

/// House / tower mesh (walls, openings, roof, chimney, details) plus markers
/// (chimney smoke, lit windows, door spots, roof perches).
/// `cuts` are passages through the house walls (paths) and become doors / arches.
pub fn house_mesh(house: &House, cuts: &[Cut], terrain: &Terrain, seed: u64) -> (MeshData, Vec<Marker>) {
    let fp = sane_footprint(house.footprint);
    let poly = footprint_polygon(&fp);
    let h = fin(house.wall_height, 3.0).clamp(0.6, 24.0);
    let pitch = fin(house.roof_pitch, 0.6).clamp(0.0, 4.0);
    let y0 = {
        let m = poly.iter().map(|p| terrain.height(*p)).filter(|v| v.is_finite()).fold(f32::INFINITY, f32::min);
        if m.is_finite() { m } else { 0.0 }
    };
    let center = poly.iter().copied().sum::<Vec2>() / poly.len() as f32;
    let (round, inradius) = match fp {
        Footprint::Round { radius, .. } => (true, radius),
        Footprint::Rect { half, .. } => (false, half.x.min(half.y)),
    };
    let tower = round && h >= 6.0;
    let colors = house.colors;
    let trim = palette::trim(colors);
    let (msurf, mcol) = material_look(house.material, colors);
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
        shelter(&mut b, &poly, y0, h, style, pitch, inradius, round, colors, terrain, seed, center);
        return (b.m, b.mk);
    }

    let foundation = fin(house.foundation, 0.0).clamp(0.0, (h * 0.5).min(2.0));
    let has_plinth = foundation > 0.02;
    let timbered = house.fins && house.material != Material::Concrete && h >= 3.5 && !tower;
    let n = poly.len();
    let lens: Vec<f32> = (0..n).map(|i| poly[i].distance(poly[(i + 1) % n])).collect();
    let holes = plan_openings(house, cuts, &lens, h, round, tower, &mut rng);

    // ---- wall bands -------------------------------------------------------------------
    let stone = palette::stone();
    let mut bands: Vec<Band> = Vec::new();
    let body_lo = if has_plinth { foundation } else { -SINK };
    if has_plinth {
        bands.push(Band::new(-SINK, foundation, 0.06, Surface::Plinth, palette::foundation()));
    }
    let mut top_off = 0.0;
    let mut courses: Vec<(f32, f32, f32)> = Vec::new(); // tower string courses: y, off below, off above
    if timbered {
        bands.push(Band::new(body_lo, STOREY, 0.0, Surface::Concrete, stone));
        bands.push(Band::new(STOREY, h, JETTY, msurf, mcol));
        top_off = JETTY;
    } else if tower {
        // Slight taper, never more than ~15 % of the radius in total.
        let taper = TAPER.min(inradius * 0.15 / (h / TOWER_SEG).ceil().max(1.0));
        let mut lo = body_lo;
        let mut k = 0;
        loop {
            let hi = ((k + 1) as f32 * TOWER_SEG).min(h);
            let off = -taper * k as f32;
            if hi > lo + 0.01 {
                bands.push(Band::new(lo, hi, off, msurf, mcol));
                top_off = off;
            }
            if hi >= h - 1e-3 {
                break;
            }
            courses.push((hi, off, off - taper));
            lo = hi;
            k += 1;
        }
    } else {
        bands.push(Band::new(body_lo, h, 0.0, msurf, mcol));
    }
    for band in &bands {
        wall_band(&mut b, &poly, y0, band, &holes, trim);
    }
    if has_plinth {
        let above = bands.iter().find(|bd| bd.lo >= foundation - 1e-3 && bd.off < 0.06).map_or(0.0, |bd| bd.off);
        annulus(&mut b, &poly, y0, foundation, 0.06, above, true, &holes, palette::foundation(), Surface::Plinth);
    }
    if timbered {
        annulus(&mut b, &poly, y0, STOREY, 0.0, JETTY, false, &holes, trim, Surface::Cladding);
        timber_frame(&mut b, &poly, y0, h, &holes, trim, seed);
    }
    for &(y, below, above) in &courses {
        let out = below + 0.1;
        let ledge = Band::new(y - 0.12, y + 0.1, out, Surface::Concrete, stone * 0.9);
        wall_band(&mut b, &poly, y0, &ledge, &holes, trim);
        annulus(&mut b, &poly, y0, y - 0.12, out, below, false, &holes, stone * 0.9, Surface::Concrete);
        annulus(&mut b, &poly, y0, y + 0.1, out, above, true, &holes, stone * 0.9, Surface::Concrete);
    }

    // ---- corner quoins (rect houses, stone / plaster corners) --------------------------
    if !round && (timbered || house.material != Material::Timber) {
        let lo = if has_plinth { foundation } else { 0.0 };
        let hi = if timbered { STOREY } else { h };
        let qcol = if bands.iter().any(|bd| bd.lo <= lo + 1e-3 && bd.surf == Surface::Earth) { stone } else { stone * 0.86 };
        quoins(&mut b, &poly, y0, lo, hi, qcol, seed);
    }

    // ---- openings: windows, doors ------------------------------------------------------
    let quoin_margin = if round { 0.05 } else { 0.55 };
    for hole in &holes {
        let vm = (hole.v0 + hole.v1) * 0.5;
        let band = bands.iter().find(|bd| vm >= bd.lo && vm < bd.hi).or(bands.last()).copied().unwrap_or(Band::new(0.0, h, 0.0, msurf, mcol));
        let wf = Wf::at_off(&poly, band.off, hole.edge, y0);
        match hole.kind {
            Hk::Window | Hk::Slit => window_details(&mut b, &wf, hole, &band, quoin_margin, trim, seed, !tower),
            Hk::Door | Hk::Wide => door_details(&mut b, &wf, hole, &band, h, trim, terrain, seed),
        }
    }

    // ---- ivy on stone walls ------------------------------------------------------------
    for band in bands.iter().filter(|bd| bd.surf == Surface::Concrete && bd.lo <= 0.5) {
        ivy(&mut b, &poly, y0, band, &holes, round, seed);
    }

    // ---- roof --------------------------------------------------------------------------
    let roof_poly = offset_poly(&poly, top_off);
    let eave = y0 + h;
    let opts = RoofOpts {
        overhang: if style == RoofStyle::Green { 0.0 } else { OVERHANG },
        gable: Some((msurf, mcol)),
        timber: timbered.then_some(trim),
        planters: house.planters || tower,
        corbels: tower,
        parapet: true,
        finial: round,
        dormers: !round,
        trim,
        seed,
    };
    let roof_pitch = if tower && style == RoofStyle::Shell { pitch.max(1.4) } else { pitch };
    let geom = roof_impl(&mut b, &roof_poly, eave, style, roof_pitch, colors, &opts);

    // ---- chimney (rect houses) ---------------------------------------------------------
    if let (false, Some(rf)) = (round, rect_frame(&roof_poly)) {
        chimney(&mut b, &rf, &geom, eave, seed);
    }

    // ---- props beside the house --------------------------------------------------------
    let door_edge = holes.iter().find(|ho| ho.kind.is_door()).map_or(usize::MAX, |ho| ho.edge);
    yard_decor(&mut b, &poly, y0, h, door_edge, round, tower, &bands, terrain, seed, &holes);

    (b.m, b.mk)
}

/// Roof over an arbitrary simple polygon `poly` (counter-clockwise, as `geom::footprint_polygon`)
/// with eaves at height `base_y`. Used for houses and for roofed wall rings.
/// `pitch` as in [`House::roof_pitch`]. Non-rectangular polygons: hip/cone style roofs rise to
/// the centroid; gable falls back to hip.
///
/// The roof plane passes through `base_y` at the polygon outline and overhangs it by ~0.35 m.
/// Rectangles get true gable / hip / pyramid roofs (gable ends in stone); other star-shaped
/// polygons get a pyramid to the centroid; concave rings whose centroid cannot see every edge
/// fall back to a flat roof with a parapet.
pub fn roof_mesh(poly: &[Vec2], base_y: f32, style: RoofStyle, pitch: f32, colors: Colors, seed: u64) -> (MeshData, Vec<Marker>) {
    let mut b = B::default();
    let opts = RoofOpts {
        overhang: OVERHANG,
        gable: Some((Surface::Concrete, palette::stone())),
        timber: None,
        planters: false,
        corbels: false,
        parapet: true,
        finial: false,
        dormers: false,
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
    // Ground-storey window size.
    let win_h = 1.0f32.min(h - 0.6);
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
        // Arrow slits spiralling up the drums.
        let mut k = 1;
        while (k as f32) * TOWER_SEG + 2.1 <= h - 0.2 {
            for j in 0..2 {
                let e = (k * 3 + j * n / 2 + n / 4) % n;
                holes.extend(place_hole(&holes, lens, e, lens[e] * 0.5, Hk::Slit, k as f32 * TOWER_SEG + 0.9, 1.1, 0.22, true));
            }
            k += 1;
        }
        return holes;
    }

    // Windows every ~2.2 m per storey.
    let mut k = 0;
    loop {
        let (sill, wh) = if k == 0 { (win_sill, win_h) } else { (k as f32 * STOREY + 0.8, 1.0) };
        if wh < 0.4 || sill + wh > h - 0.25 {
            break;
        }
        for e in 0..n {
            if round {
                if (e + k) % 3 == 0 {
                    holes.extend(place_hole(&holes, lens, e, lens[e] * 0.5, Hk::Window, sill, wh, 0.8, true));
                }
                continue;
            }
            let usable = lens[e] - 1.4;
            let mut count = (usable / 2.2).floor() as i32;
            if count == 0 && usable >= 0.9 {
                count = 1;
            }
            for j in 0..count.max(0) {
                let u = 0.7 + usable * (j as f32 + 0.5) / count as f32;
                holes.extend(place_hole(&holes, lens, e, u, Hk::Window, sill, wh, 0.8, false));
            }
        }
        k += 1;
        if k > 12 {
            break;
        }
    }
    holes
}

#[allow(clippy::too_many_arguments)]
fn window_details(b: &mut B, wf: &Wf, h: &Hole, band: &Band, margin: f32, trim: Vec3, seed: u64, allow_box: bool) {
    let (u0, u1, v0, v1) = (h.u0, h.u1, h.v0, h.v1);
    let w = u1 - u0;
    let mid = (u0 + u1) * 0.5;
    let vm = (v0 + v1) * 0.5;
    let centre = wf.at(mid, vm, 0.0);
    let mut rng = Rng::at(seed ^ 0x5157_0000, centre.x, centre.z + centre.y * 0.37);
    b.marker(MarkerKind::WindowLight, wf.at(mid, vm, -h.kind.depth() + 0.02), wf.n);
    let stone_wall = matches!(band.surf, Surface::Concrete | Surface::Plinth);
    if h.kind == Hk::Slit {
        b.lbox(wf, mid, v1 + 0.07, 0.02, Vec3::new(w * 0.5 + 0.1, 0.07, 0.05), palette::stone() * 0.9, Surface::Concrete);
        b.lbox(wf, mid, v0 - 0.05, 0.02, Vec3::new(w * 0.5 + 0.08, 0.05, 0.05), palette::stone() * 0.9, Surface::Concrete);
        return;
    }
    let (lc, ls) = if stone_wall { (palette::stone() * 0.92, Surface::Concrete) } else { (trim, Surface::Steel) };
    b.lbox(wf, mid, v1 + 0.07, 0.02, Vec3::new(w * 0.5 + 0.1, 0.07, 0.05), lc, ls);
    b.lbox(wf, mid, v0 - 0.05, 0.05, Vec3::new(w * 0.5 + 0.09, 0.05, 0.08), lc, ls);
    // Open shutters, flat against the wall.
    let sw = w * 0.5;
    if u0 - sw - 0.05 >= wf.u_start() + margin && u1 + sw + 0.05 <= wf.u_end() - margin {
        let tilt = rng.range(-0.02, 0.02);
        for (u, sd) in [(u0 - sw * 0.5 - 0.03, -1.0f32), (u1 + sw * 0.5 + 0.03, 1.0)] {
            b.lbox(wf, u, vm + tilt * sd, 0.035, Vec3::new(sw * 0.5, (v1 - v0) * 0.5, 0.025), trim, Surface::Cladding);
        }
    }
    // Window box with flowers.
    if allow_box && rng.f() < 0.4 {
        b.lbox(wf, mid, v0 - 0.21, 0.2, Vec3::new(w * 0.5 + 0.04, 0.1, 0.12), trim * 0.85, Surface::Cladding);
        b.lbox(wf, mid, v0 - 0.07, 0.2, Vec3::new(w * 0.5, 0.05, 0.1), palette::leaves(), Surface::Leaves);
        let ci = (rng.f() * 6.0) as u32;
        let nfl = 4;
        for j in 0..nfl {
            let u = u0 + w * (j as f32 + 0.5) / nfl as f32 + rng.range(-0.05, 0.05);
            let c = palette::flower(ci + (j % 2) as u32 * 2);
            b.lbox(wf, u, v0 + rng.range(0.0, 0.07), 0.2 + rng.range(-0.05, 0.05), Vec3::splat(0.06), c, Surface::Petal);
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn door_details(b: &mut B, wf: &Wf, h: &Hole, band: &Band, wall_h: f32, trim: Vec3, terrain: &Terrain, seed: u64) {
    let (u0, u1, v1) = (h.u0, h.u1, h.v1);
    let w = u1 - u0;
    let mid = (u0 + u1) * 0.5;
    let stone_wall = matches!(band.surf, Surface::Concrete | Surface::Plinth);
    let at_door = wf.at(mid, 0.0, 0.0);
    let mut rng = Rng::at(seed ^ 0xD00_0000, at_door.x, at_door.z);
    if stone_wall {
        // Arched stone frame; spandrels fill the corners above the arch.
        let rx = w * 0.5;
        let ry = (rx * 0.7).min((h.v1 - h.v0) * 0.35);
        let spring = v1 - ry;
        let th = 0.2;
        let out = 0.07;
        let segs = 6;
        let pt = |t: f32, r: f32| (mid + t.cos() * (rx + r), spring + t.sin() * (ry + r));
        for k in 0..segs {
            let t0 = PI * (1.0 - k as f32 / segs as f32);
            let t1 = PI * (1.0 - (k + 1) as f32 / segs as f32);
            let (i0, i1, o0, o1) = (pt(t0, 0.0), pt(t1, 0.0), pt(t0, th), pt(t1, th));
            let col = palette::stone() * rng.range(0.8, 0.95);
            let a = |p: (f32, f32), o: f32| wf.at(p.0, p.1, o);
            let uv = [Vec2::ZERO, Vec2::new(0.3, 0.0), Vec2::new(0.3, 0.2), Vec2::new(0.0, 0.2)];
            b.face([a(i0, out), a(i1, out), a(o1, out), a(o0, out)], uv, wf.n, col, Surface::Concrete);
            let tm = (t0 + t1) * 0.5;
            let radial = wf.d * tm.cos() + Vec3::Y * tm.sin();
            b.face([a(o0, 0.0), a(o1, 0.0), a(o1, out), a(o0, out)], uv, radial, col, Surface::Concrete);
            b.face([a(i0, 0.0), a(i1, 0.0), a(i1, out), a(i0, out)], uv, -radial, col, Surface::Concrete);
            let corner = if k < segs / 2 { (u0, v1) } else { (u1, v1) };
            b.tri(a(corner, 0.003), a(i0, 0.003), a(i1, 0.003), wf.n, band.color, band.surf);
        }
    } else {
        for u in [u0 - 0.07, u1 + 0.07] {
            b.lbox(wf, u, (h.v0 + v1) * 0.5 + 0.04, 0.02, Vec3::new(0.07, (v1 - h.v0) * 0.5 + 0.04, 0.05), trim, Surface::Steel);
        }
        b.lbox(wf, mid, v1 + 0.1, 0.03, Vec3::new(w * 0.5 + 0.2, 0.08, 0.06), trim, Surface::Steel);
    }
    // Step stone.
    b.lbox(wf, mid, 0.0, 0.24, Vec3::new(w * 0.5 + 0.12, 0.09, 0.22), palette::foundation(), Surface::Plinth);
    // Lantern on a bracket beside the door.
    let mut side = if rng.f() < 0.5 { -1.0 } else { 1.0 };
    let lu = |s: f32| mid + s * (w * 0.5 + 0.4);
    if !(0.25..=wf.len - 0.25).contains(&lu(side)) {
        side = -side;
    }
    let fits = |u: f32| (0.25..=wf.len - 0.25).contains(&u);
    // Narrow walls (round houses): hang the lantern above the door instead.
    let (u, yl) = if fits(lu(side)) { (lu(side), (v1 + 0.2).min(wall_h - 0.35)) } else { (mid, (v1 + 0.5).min(wall_h - 0.3)) };
    if fits(u) && yl > 0.8 {
        b.lbox(wf, u, yl + 0.12, 0.15, Vec3::new(0.025, 0.025, 0.15), palette::metal(), Surface::Metal);
        b.lbox(wf, u, yl + 0.05, 0.3, Vec3::new(0.09, 0.025, 0.09), palette::metal(), Surface::Metal);
        b.lbox(wf, u, yl - 0.09, 0.3, Vec3::new(0.065, 0.11, 0.065), palette::glow(), Surface::Glow);
        b.marker(MarkerKind::Lantern, wf.at(u, yl - 0.09, 0.3), Vec3::Y);
    }
    let spot = wf.at(mid, 0.0, 0.8);
    let gy = fin(terrain.height(Vec2::new(spot.x, spot.z)), spot.y);
    b.marker(MarkerKind::Door, Vec3::new(spot.x, gy, spot.z), wf.n);
}

// =============================================================================================
// Wall dressing
// =============================================================================================

/// Alternating long/short stone blocks at every corner.
fn quoins(b: &mut B, poly: &[Vec2], y0: f32, lo: f32, hi: f32, col: Vec3, seed: u64) {
    let ch = 0.38;
    let count = ((hi - lo) / ch).floor().max(0.0) as usize;
    let n = poly.len();
    for j in 0..n {
        let wo = Wf::new(poly, poly, j, y0);
        let wi = Wf::new(poly, poly, (j + n - 1) % n, y0);
        if wo.len < 1.2 || wi.len < 1.2 {
            continue;
        }
        for k in 0..count.min(64) {
            let y = lo + (k as f32 + 0.5) * ch;
            let (lo_len, li_len) = if (k + j) % 2 == 0 { (0.55, 0.3) } else { (0.3, 0.55) };
            let mut rng = Rng::at(seed ^ 0x0071_0000, poly[j].x + y, poly[j].y);
            let c = col * rng.range(0.85, 1.02);
            b.lbox(&wo, (lo_len - 0.05) * 0.5, y, 0.0, Vec3::new((lo_len + 0.05) * 0.5, ch * 0.46, 0.05), c, Surface::Concrete);
            b.lbox(&wi, wi.len - (li_len - 0.05) * 0.5, y, 0.0, Vec3::new((li_len + 0.05) * 0.5, ch * 0.46, 0.05), c, Surface::Concrete);
        }
    }
}

/// Exposed timber framing of the jettied upper storey: plates, posts, braces.
fn timber_frame(b: &mut B, base: &[Vec2], y0: f32, h: f32, holes: &[Hole], trim: Vec3, seed: u64) {
    let offp = offset_poly(base, JETTY);
    let (lo, hi) = (STOREY, h);
    let mid_y = (lo + hi) * 0.5;
    let half_h = (hi - lo) * 0.5;
    for i in 0..base.len() {
        let wf = Wf::new(base, &offp, i, y0);
        if wf.len < 0.3 {
            continue;
        }
        let (us, ue) = (wf.u_start(), wf.u_end());
        for y in [lo + 0.09, hi - 0.09] {
            b.lbox(&wf, (us + ue) * 0.5, y, 0.0, Vec3::new((ue - us) * 0.5 + 0.03, 0.09, 0.07), trim, Surface::Steel);
        }
        let spans: Vec<(f32, f32)> =
            holes.iter().filter(|o| o.edge == i && o.v1 > lo && o.v0 < hi).map(|o| (o.u0 - 0.07, o.u1 + 0.07)).collect();
        let mut posts = vec![us + 0.07, ue - 0.07];
        for (a, c) in &spans {
            posts.push(*a);
            posts.push(*c);
        }
        posts.sort_by(f32::total_cmp);
        posts.dedup_by(|a, b| (*a - *b).abs() < 0.16);
        let in_span = |u: f32| spans.iter().any(|(a, c)| u > *a && u < *c);
        let mut all = posts.clone();
        for w in posts.windows(2) {
            let (a, c) = (w[0], w[1]);
            if in_span((a + c) * 0.5) {
                continue;
            }
            let k = ((c - a) / 1.5).round().max(1.0) as usize;
            for j in 1..k {
                all.push(a + (c - a) * j as f32 / k as f32);
            }
        }
        all.sort_by(f32::total_cmp);
        for &u in &all {
            b.lbox(&wf, u, mid_y, 0.0, Vec3::new(0.07, half_h, 0.06), trim, Surface::Steel);
        }
        for w in all.windows(2) {
            let (a, c) = (w[0], w[1]);
            if c - a < 0.5 || in_span((a + c) * 0.5) {
                continue;
            }
            let pm = wf.at((a + c) * 0.5, mid_y, 0.0);
            let mut rng = Rng::at(seed ^ 0x00B7_ACE0, pm.x, pm.z);
            if rng.f() < 0.55 {
                let (ya, yc) = if rng.f() < 0.5 { (lo + 0.18, hi - 0.3) } else { (hi - 0.3, lo + 0.18) };
                b.m.beam(wf.at(a + 0.07, ya, 0.0), wf.at(c - 0.07, yc, 0.0), 0.1, 0.12, trim, Surface::Steel, 1.0);
            }
        }
    }
}

/// Ivy patches climbing from the ground on stone bands.
fn ivy(b: &mut B, poly: &[Vec2], y0: f32, band: &Band, holes: &[Hole], round: bool, seed: u64) {
    let offp = offset_poly(poly, band.off);
    let chance = if round { 0.22 } else { 0.45 };
    for i in 0..poly.len() {
        let wf = Wf::new(poly, &offp, i, y0);
        if wf.len < 1.0 {
            continue;
        }
        let m = wf.at(wf.len * 0.5, 0.0, 0.0);
        let mut rng = Rng::at(seed ^ 0x1717_0000, m.x, m.z);
        if rng.f() >= chance {
            continue;
        }
        let uc = rng.range(0.4, (wf.len - 0.4).max(0.41));
        let top = rng.range(1.2, 3.4).min(band.hi - 0.1);
        if holes.iter().any(|o| o.edge == i && o.u0 < uc + 0.6 && uc - 0.6 < o.u1 && o.v0 < top) {
            continue;
        }
        let rows = ((top - band.lo.max(-0.1)) / 0.42).ceil().clamp(1.0, 10.0) as usize;
        for r in 0..rows {
            let y = band.lo.max(-0.1) + r as f32 * 0.42;
            let w = rng.range(0.4, 0.8) * (1.0 - r as f32 / rows as f32 * 0.5);
            let u = uc + rng.range(-0.18, 0.18);
            let c = palette::leaves() * rng.range(0.8, 1.1);
            b.wquad(&wf, u - w * 0.5, u + w * 0.5, y, y + 0.55, 0.05, false, c, Surface::Leaves);
        }
    }
}

// =============================================================================================
// Roofs
// =============================================================================================

#[derive(Clone, Copy, Debug)]
struct RoofOpts {
    overhang: f32,
    /// Material of gable-end triangles (and dormer walls); `None` leaves gables open.
    gable: Option<(Surface, Vec3)>,
    /// Half-timber pattern on gables in this colour.
    timber: Option<Vec3>,
    planters: bool,
    corbels: bool,
    parapet: bool,
    finial: bool,
    dormers: bool,
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

/// What was built, so the chimney can find the roof surface.
#[derive(Clone, Debug)]
enum RoofGeom {
    None,
    Flat { top: f32 },
    Ridge { rf: Rf, base: f32, rise: f32, hip: bool, tv: f32 },
    Pyramid { c: Vec2, base: f32, rise: f32, edges: Vec<(Vec2, f32)>, tv: f32 },
}

impl RoofGeom {
    /// Height of the roof's top surface at `p` (inside the footprint).
    fn top_at(&self, p: Vec2) -> Option<f32> {
        match self {
            RoofGeom::None => None,
            RoofGeom::Flat { top } => Some(*top),
            RoofGeom::Ridge { rf, base, rise, hip, tv } => {
                let s = (p - rf.c).dot(rf.ax).abs();
                let t = (p - rf.c).dot(rf.ac).abs();
                let k = rise / rf.b.max(0.05);
                let mut d = t;
                if *hip {
                    d = d.max(s - (rf.a - rf.b).max(0.0));
                }
                Some(base + rise - k * d.min(rf.b) + tv)
            }
            RoofGeom::Pyramid { c, base, rise, edges, tv } => {
                let r = edges.iter().map(|(n, d)| (p - *c).dot(*n) / d.max(0.05)).fold(0.0f32, f32::max).min(1.0);
                Some(base + rise * (1.0 - r) + tv)
            }
        }
    }
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
        return flat_roof(b, &poly, base, o);
    }
    let (surf, thick) = roof_cover(colors);
    let col = palette::roof(colors);
    match (rf, style) {
        (Some(rf), RoofStyle::Pitched) | (Some(rf), RoofStyle::Sawtooth) => {
            let rise = (pitch * rf.b).min(30.0);
            ridge_roof(b, &rf, base, rise, style == RoofStyle::Sawtooth, surf, thick, col, o)
        }
        _ => {
            let rise = (pitch * dmin).min(30.0);
            pyramid_roof(b, &poly, c, &edges, base, rise, surf, thick, col, o)
        }
    }
}

/// One roof plane: top surface (raised by `tv`), boarded underside, and fascia faces on the
/// edges that have an outward direction in `fascia`.
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
    let under = palette::wood() * 0.6;
    for (side, normal) in [(true, n), (false, -n)] {
        let ord: Vec<usize> = if side { order.clone() } else { order.iter().rev().copied().collect() };
        let base_i = b.m.positions.len() as u32;
        for &i in &ord {
            let p = if side { pts[i] + up } else { pts[i] };
            let (c, sf) = if side { (col, surf) } else { (under, Surface::Cladding) };
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
            b.face([a, c, c + up, a + up], uv, want, col * 0.8, surf);
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn ridge_roof(b: &mut B, rf: &Rf, base: f32, rise: f32, hip: bool, surf: Surface, tv: f32, col: Vec3, o: &RoofOpts) -> RoofGeom {
    let k = rise / rf.b.max(0.05);
    let oe = o.overhang.min(0.8 / k.max(1e-3));
    let ye = base - oe * k;
    let ridge = base + rise;
    let (bg, ag) = (rf.b + oe, rf.a + if hip { oe } else { o.overhang });
    let (ax3, ac3) = (p3(rf.ax, 0.0), p3(rf.ac, 0.0));
    let w = |s: f32, t: f32, y: f32| rf.w(s, t, y);
    let cap = col * 0.75;
    if !hip {
        for sg in [1.0f32, -1.0] {
            let pts = [w(-ag, sg * bg, ye), w(ag, sg * bg, ye), w(ag, 0.0, ridge), w(-ag, 0.0, ridge)];
            roof_face(b, &pts, &[Some(ac3 * sg), Some(ax3), None, Some(-ax3)], tv, ax3, col, surf);
        }
        b.m.beam(w(-ag - 0.05, 0.0, ridge + tv - 0.07), w(ag + 0.05, 0.0, ridge + tv - 0.07), 0.3, 0.16, cap, surf, 1.0);
        for ss in [1.0f32, -1.0] {
            b.marker(MarkerKind::Perch, w(ss * ag * 0.92, 0.0, ridge + tv + 0.12), Vec3::Y);
        }
        if let Some((gs, gc)) = o.gable {
            for ss in [1.0f32, -1.0] {
                let sa = ss * rf.a;
                b.tri(w(sa, -rf.b, base), w(sa, rf.b, base), w(sa, 0.0, ridge), ax3 * ss, gc, gs);
                if let Some(tc) = o.timber {
                    gable_timber(b, rf, ss, base, rise, tc);
                }
            }
            if o.dormers && rise > 2.5 && rf.a >= 2.4 {
                let count = if rf.a * 2.0 >= 7.0 { 2 } else { 1 };
                for sg in [1.0f32, -1.0] {
                    for j in 0..count {
                        let s0 = if count == 1 { 0.0 } else { (j as f32 * 2.0 - 1.0) * rf.a * 0.42 };
                        dormer(b, rf, s0, sg, base, rise, tv, (gs, gc), (surf, col), o.trim);
                    }
                }
            }
        }
    } else {
        let r = (rf.a - rf.b).max(0.0);
        for sg in [1.0f32, -1.0] {
            let pts = [w(-ag, sg * bg, ye), w(ag, sg * bg, ye), w(r, 0.0, ridge), w(-r, 0.0, ridge)];
            roof_face(b, &pts, &[Some(ac3 * sg), None, None, None], tv, ax3, col, surf);
            let end = [w(sg * ag, -bg, ye), w(sg * ag, bg, ye), w(sg * r, 0.0, ridge)];
            roof_face(b, &end, &[Some(ax3 * sg), None, None], tv, ac3, col, surf);
        }
        if r > 0.1 {
            b.m.beam(w(-r, 0.0, ridge + tv - 0.07), w(r, 0.0, ridge + tv - 0.07), 0.28, 0.15, cap, surf, 1.0);
        }
        for ss in [1.0f32, -1.0] {
            for sg in [1.0f32, -1.0] {
                b.m.beam(w(ss * ag, sg * bg, ye + tv - 0.05), w(ss * r, 0.0, ridge + tv - 0.07), 0.2, 0.12, cap, surf, 1.0);
            }
            b.marker(MarkerKind::Perch, w(ss * r, 0.0, ridge + tv + 0.12), Vec3::Y);
        }
    }
    RoofGeom::Ridge { rf: *rf, base, rise, hip, tv }
}

/// Collar, king post and struts on a gable triangle.
fn gable_timber(b: &mut B, rf: &Rf, ss: f32, base: f32, rise: f32, tc: Vec3) {
    let s = ss * (rf.a + 0.03);
    let w = |t: f32, y: f32| rf.w(s, t, y);
    let yc = base + rise * 0.45;
    let hw = rf.b * 0.55 - 0.05;
    b.m.beam(w(-rf.b + 0.05, base), w(rf.b - 0.05, base), 0.1, 0.14, tc, Surface::Steel, 1.0);
    b.m.beam(w(-hw, yc - 0.07), w(hw, yc - 0.07), 0.1, 0.13, tc, Surface::Steel, 1.0);
    let post_h = rise * 0.9;
    b.m.oriented_box(w(0.0, base + post_h * 0.5), Vec3::new(0.07, post_h * 0.5, 0.05), p3(rf.ac, 0.0), tc, Surface::Steel, 1.0);
    for sd in [-1.0f32, 1.0] {
        b.m.beam(w(sd * rf.b * 0.75, base + 0.12), w(sd * 0.12, yc - 0.1), 0.09, 0.11, tc, Surface::Steel, 1.0);
    }
}

/// Small gabled dormer window sitting on the slope on side `sg` at `s0` along the ridge.
#[allow(clippy::too_many_arguments)]
fn dormer(b: &mut B, rf: &Rf, s0: f32, sg: f32, base: f32, rise: f32, tv: f32, wall: (Surface, Vec3), roof: (Surface, Vec3), trim: Vec3) {
    let k = rise / rf.b.max(0.05);
    let ridge_top = base + rise + tv;
    let top = |t: f32| ridge_top - k * t;
    let w = |s: f32, t: f32, y: f32| rf.w(s0 + s, t * sg, y);
    let (hw, hd, rd) = (0.6, 0.9, 0.45);
    let tf = 0.62 * rf.b;
    let yb = top(tf);
    let ye = yb + hd;
    let ya = ye + rd;
    if ya > ridge_top - 0.15 {
        return;
    }
    let te = (ridge_top - ye) / k;
    let ta = (ridge_top - ya) / k;
    let out = p3(rf.ac * sg, 0.0);
    let ax = p3(rf.ax, 0.0);
    let uv = [Vec2::ZERO, Vec2::new(1.2, 0.0), Vec2::new(1.2, hd), Vec2::new(0.0, hd)];
    b.face([w(-hw, tf, yb), w(hw, tf, yb), w(hw, tf, ye), w(-hw, tf, ye)], uv, out, wall.1, wall.0);
    b.tri(w(-hw, tf, ye), w(hw, tf, ye), w(0.0, tf, ya), out, wall.1, wall.0);
    for sd in [-1.0f32, 1.0] {
        b.tri(w(sd * hw, tf, yb), w(sd * hw, tf, ye), w(sd * hw, te, ye), ax * sd, wall.1, wall.0);
    }
    let wo = tf + 0.02;
    let wuv = [Vec2::ZERO, Vec2::X, Vec2::ONE, Vec2::Y];
    b.face([w(-0.3, wo, yb + 0.15), w(0.3, wo, yb + 0.15), w(0.3, wo, yb + 0.75), w(-0.3, wo, yb + 0.75)], wuv, out, palette::white(), Surface::Window);
    b.m.beam(w(-0.36, wo, yb + 0.06), w(0.36, wo, yb + 0.06), 0.08, 0.08, trim, Surface::Steel, 1.0);
    b.marker(MarkerKind::WindowLight, w(0.0, wo, yb + 0.45), out);
    let tfo = tf + 0.18;
    let yev = ye - 0.06;
    let tev = (ridge_top - yev) / k;
    let dtv = 0.08;
    for sd in [-1.0f32, 1.0] {
        let pts = [w(sd * (hw + 0.12), tfo, yev), w(sd * (hw + 0.12), tev, yev), w(0.0, ta, ya), w(0.0, tfo, ya)];
        roof_face(b, &pts, &[Some(ax * sd), None, None, Some(out)], dtv, out, roof.1, roof.0);
    }
    b.m.beam(w(0.0, tfo + 0.03, ya + dtv - 0.05), w(0.0, ta, ya + dtv - 0.05), 0.16, 0.1, roof.1 * 0.75, roof.0, 1.0);
}

#[allow(clippy::too_many_arguments)]
fn pyramid_roof(b: &mut B, poly: &[Vec2], c: Vec2, edges: &[(Vec2, f32)], base: f32, rise: f32, surf: Surface, tv: f32, col: Vec3, o: &RoofOpts) -> RoofGeom {
    let dmin = edges.iter().map(|e| e.1).fold(f32::INFINITY, f32::min).max(0.05);
    let k = rise / dmin;
    let oe = o.overhang.min(0.8 / k.max(1e-3));
    let ye = base - oe * k;
    let g = offset_poly(poly, oe);
    let apex = p3(c, base + rise);
    let n = g.len();
    for i in 0..n {
        let (a, q) = (g[i], g[(i + 1) % n]);
        let e = p3((q - a).normalize_or(Vec2::X), 0.0);
        let pts = [p3(a, ye), p3(q, ye), apex];
        roof_face(b, &pts, &[Some(p3(edges[i].0, 0.0)), None, None], tv, e, col, surf);
    }
    let top = apex + Vec3::Y * tv;
    if o.finial {
        b.m.cylinder(top - Vec3::Y * 0.1, 0.09, 0.0, 1.0, 4, false, false, palette::metal(), Surface::Metal, 1.0);
        b.m.aabb(top + Vec3::new(-0.09, 0.3, -0.09), top + Vec3::new(0.09, 0.48, 0.09), palette::glow() * 0.9, Surface::Metal, 1.0);
        b.marker(MarkerKind::Perch, top + Vec3::Y * 0.95, Vec3::Y);
    } else {
        b.m.aabb(top - Vec3::new(0.15, 0.12, 0.15), top + Vec3::new(0.15, 0.08, 0.15), col * 0.75, surf, 1.0);
        b.marker(MarkerKind::Perch, top + Vec3::Y * 0.15, Vec3::Y);
    }
    RoofGeom::Pyramid { c, base, rise, edges: edges.to_vec(), tv }
}

fn flat_roof(b: &mut B, poly: &[Vec2], base: f32, o: &RoofOpts) -> RoofGeom {
    let stone = palette::stone();
    let slab_y = base + 0.15;
    let top = offset_poly(poly, 0.03);
    for t in triangulate(&top) {
        b.tri(p3(top[t[0]], slab_y), p3(top[t[1]], slab_y), p3(top[t[2]], slab_y), Vec3::Y, stone * 0.8, Surface::Slabs);
    }
    wall_band(b, poly, 0.0, &Band::new(base - 0.05, slab_y, 0.03, Surface::Concrete, stone * 0.9), &[], Vec3::ONE);
    if !o.parapet {
        b.marker(MarkerKind::Perch, p3(poly[0], slab_y + 0.05), Vec3::Y);
        return RoofGeom::Flat { top: slab_y };
    }
    let (out, inn) = if o.corbels { (0.3, 0.0) } else { (0.05, -0.25) };
    let p_lo = if o.corbels { base - 0.1 } else { base - 0.05 };
    let p_hi = slab_y + 0.55;
    wall_band(b, poly, 0.0, &Band::new(p_lo, p_hi, out, Surface::Concrete, stone), &[], Vec3::ONE);
    let mut inner = Band::new(slab_y, p_hi, inn, Surface::Concrete, stone * 0.85);
    inner.inward = true;
    wall_band(b, poly, 0.0, &inner, &[], Vec3::ONE);
    annulus(b, poly, 0.0, p_hi, out, inn, true, &[], stone * 0.95, Surface::Concrete);
    if o.corbels {
        annulus(b, poly, 0.0, p_lo, out, 0.0, false, &[], stone * 0.8, Surface::Concrete);
        for i in 0..poly.len() {
            let wf = Wf::new(poly, poly, i, 0.0);
            let count = ((wf.len / 0.7) as usize).clamp(1, 64);
            for j in 0..count {
                let u = wf.len * (j as f32 + 0.5) / count as f32;
                b.lbox(&wf, u, p_lo - 0.2, 0.15, Vec3::new(0.09, 0.2, 0.15), stone * 0.85, Surface::Concrete);
                b.lbox(&wf, u, p_lo - 0.48, 0.07, Vec3::new(0.08, 0.08, 0.07), stone * 0.85, Surface::Concrete);
            }
        }
    }
    let mut perch_y = p_hi;
    if o.planters {
        let mid = (out + inn) * 0.5;
        let offm = offset_poly(poly, mid);
        for i in 0..poly.len() {
            let wf = Wf::new(poly, &offm, i, 0.0);
            let l = wf.u_end() - wf.u_start();
            if l < 0.3 {
                continue;
            }
            let count = ((l / 1.1).round() as usize).clamp(1, 64);
            let seg = l / count as f32;
            for j in 0..count {
                let u = wf.u_start() + seg * (j as f32 + 0.5);
                b.lbox(&wf, u, p_hi + 0.22, 0.0, Vec3::new((seg * 0.27).min(0.3), 0.22, (out - inn) * 0.5), stone, Surface::Concrete);
            }
        }
        perch_y += 0.44;
    }
    let mut rng = Rng::at(o.seed ^ 0xF1A7, poly[0].x, poly[0].y);
    let k = (rng.f() * poly.len() as f32) as usize % poly.len();
    b.marker(MarkerKind::Perch, p3(offset_poly(poly, (out + inn) * 0.5)[k], perch_y + 0.02), Vec3::Y);
    RoofGeom::Flat { top: slab_y }
}

// =============================================================================================
// Chimney, shelters, yard props
// =============================================================================================

fn chimney(b: &mut B, rf: &Rf, geom: &RoofGeom, eave: f32, seed: u64) {
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
    b.m.oriented_box(p3(p, (bot + top) * 0.5), Vec3::new(0.32, (top - bot) * 0.5, 0.32), ax, palette::stone() * 0.92, Surface::Concrete, 1.0);
    b.m.oriented_box(p3(p, top + 0.06), Vec3::new(0.42, 0.07, 0.42), ax, palette::foundation(), Surface::Plinth, 1.0);
    let pot = palette::roof(Colors { roof: 0, wall: 0, trim: 0 }) * 0.85;
    b.m.cylinder(p3(p, top + 0.12), 0.13, 0.11, 0.28, 6, true, false, pot, Surface::Sedum, 1.0);
    b.marker(MarkerKind::ChimneySmoke, p3(p, top + 0.45), Vec3::Y);
}

/// Very low "walls": an open shelter on posts whose roof nearly reaches the ground.
#[allow(clippy::too_many_arguments)]
fn shelter(
    b: &mut B,
    poly: &[Vec2],
    y0: f32,
    h: f32,
    style: RoofStyle,
    pitch: f32,
    inradius: f32,
    round: bool,
    colors: Colors,
    terrain: &Terrain,
    seed: u64,
    center: Vec2,
) {
    let trim = palette::trim(colors);
    let n = poly.len();
    let step = if n <= 4 { 1 } else { (n / 6).max(1) };
    let mut tops = Vec::new();
    for i in (0..n).step_by(step) {
        let v = poly[i];
        let q = v + (center - v).normalize_or_zero() * 0.15;
        let top = y0 + h;
        let gy = fin(terrain.height(q), y0).min(top - 0.2);
        let bot = gy - SINK;
        b.m.oriented_box(p3(q, (bot + top) * 0.5), Vec3::new(0.08, (top - bot) * 0.5, 0.08), Vec3::X, trim, Surface::Steel, 1.0);
        tops.push(p3(q, top - 0.16));
    }
    if tops.len() >= 2 {
        for i in 0..tops.len() {
            b.m.beam(tops[i], tops[(i + 1) % tops.len()], 0.12, 0.16, trim, Surface::Steel, 1.0);
        }
    }
    let need = (2.4 - h).max(0.0) / inradius.max(0.3);
    let pitch = pitch.max(need).max(0.3);
    let style = if style == RoofStyle::Green { if round { RoofStyle::Shell } else { RoofStyle::Pitched } } else { style };
    let opts = RoofOpts {
        overhang: OVERHANG,
        gable: None,
        timber: None,
        planters: false,
        corbels: false,
        parapet: false,
        finial: round,
        dormers: false,
        trim,
        seed,
    };
    roof_impl(b, poly, y0 + h, style, pitch, colors, &opts);
    let mut rng = Rng::at(seed ^ 0x7E47, center.x, center.y);
    if rng.f() < 0.6 {
        let gy = fin(terrain.height(center), y0);
        let ang = rng.range(0.0, PI);
        let x = Vec3::new(ang.cos(), 0.0, ang.sin());
        b.m.oriented_box(Vec3::new(center.x, gy + 0.22, center.y), Vec3::new(0.45, 0.25, 0.3), x, palette::hay(), Surface::Fiber, 1.0);
        if rng.f() < 0.5 {
            b.m.oriented_box(Vec3::new(center.x, gy + 0.68, center.y) + x * 0.1, Vec3::new(0.42, 0.22, 0.28), x, palette::hay() * 0.95, Surface::Fiber, 1.0);
        }
    }
    // Folk gather under the shelter's front.
    let mut best = 0;
    for i in 1..n {
        if poly[i].distance(poly[(i + 1) % n]) > poly[best].distance(poly[(best + 1) % n]) + 1e-3 {
            best = i;
        }
    }
    let (a, q) = (poly[best], poly[(best + 1) % n]);
    let spot = (a + q) * 0.5 + outward((q - a).normalize_or(Vec2::X)) * 0.8;
    b.marker(MarkerKind::Door, p3(spot, fin(terrain.height(spot), y0)), p3(outward((q - a).normalize_or(Vec2::X)), 0.0));
}

/// A haystack, barrels, a woodpile or a washing line beside some houses.
#[allow(clippy::too_many_arguments)]
fn yard_decor(
    b: &mut B,
    poly: &[Vec2],
    y0: f32,
    h: f32,
    door_edge: usize,
    round: bool,
    tower: bool,
    bands: &[Band],
    terrain: &Terrain,
    seed: u64,
    holes: &[Hole],
) {
    let c = poly.iter().copied().sum::<Vec2>() / poly.len() as f32;
    let mut rng = Rng::at(seed ^ 0xDEC0_0000, c.x, c.y);
    let r = rng.f();
    let n = poly.len();
    // A wall that is not the door wall.
    let mut e = ((rng.f() * n as f32) as usize).min(n - 1);
    if e == door_edge {
        e = (e + n / 2).min(n - 1) % n;
    }
    let wf = Wf::new(poly, poly, e, y0);
    if wf.len < 1.5 {
        return;
    }
    let u = wf.len * rng.range(0.3, 0.7);
    if holes.iter().any(|o| o.edge == e && o.kind.is_door() && (o.u0 - 1.2..o.u1 + 1.2).contains(&u)) {
        return;
    }
    let plinth = bands.first().map_or(0.0, |bd| if bd.off > 0.0 { bd.off } else { 0.0 });
    let spot = |out: f32| {
        let p = wf.at(u, 0.0, out + plinth);
        let g = fin(terrain.height(Vec2::new(p.x, p.z)), y0);
        Vec3::new(p.x, g, p.z)
    };
    let wood = palette::wood();
    if tower {
        if r < 0.35 {
            barrels(b, spot(0.5), wf.d, &mut rng, wood);
        }
        return;
    }
    if r < 0.22 {
        let p = spot(1.0);
        b.m.cylinder(p - Vec3::Y * 0.05, 0.62, 0.5, 0.85, 7, false, false, palette::hay(), Surface::Fiber, 1.0);
        b.m.cylinder(p + Vec3::Y * 0.8, 0.5, 0.0, 0.55, 7, false, false, palette::hay() * 0.95, Surface::Fiber, 1.0);
    } else if r < 0.42 {
        barrels(b, spot(0.45), wf.d, &mut rng, wood);
    } else if r < 0.62 {
        let p = spot(0.35);
        let bark = palette::bark();
        for row in 0..3 {
            let logs = 4 - row;
            for j in 0..logs {
                let off = (j as f32 - (logs - 1) as f32 * 0.5) * 0.2;
                let base = p + Vec3::Y * (0.1 + row as f32 * 0.17) + wf.n * off;
                b.m.beam(base - wf.d * 0.55, base + wf.d * 0.55, 0.17, 0.16, bark * rng.range(0.85, 1.1), Surface::Bark, 1.0);
            }
        }
    } else if r < 0.78 && !round && h >= 2.4 {
        // Washing line from the wall to a post.
        let attach = wf.at(u, 1.9, 0.05 + plinth);
        let post = spot(3.2);
        b.m.oriented_box(post + Vec3::Y * 0.85, Vec3::new(0.06, 1.15, 0.06), wf.d, palette::wood(), Surface::Steel, 1.0);
        let ptop = post + Vec3::Y * 1.9;
        b.m.beam(attach, ptop, 0.03, 0.03, palette::white() * 0.9, Surface::Fiber, 1.0);
        let dir = (ptop - attach).normalize_or(Vec3::X);
        let side = dir.cross(Vec3::Y).normalize_or(Vec3::X);
        for (j, f) in [0.3f32, 0.52, 0.75].iter().enumerate() {
            let top = attach.lerp(ptop, *f) - Vec3::Y * 0.02;
            let hw = rng.range(0.15, 0.24);
            let ht = rng.range(0.35, 0.55);
            let a = top - dir * hw;
            let d = top + dir * hw;
            let col = if j % 2 == 0 { palette::white() } else { palette::flower(rng.f().mul_add(6.0, 0.0) as u32) };
            let quad = [a, d, d - Vec3::Y * ht, a - Vec3::Y * ht];
            let uv = [Vec2::ZERO, Vec2::X, Vec2::ONE, Vec2::Y];
            b.face(quad, uv, side, col, Surface::Fiber);
            b.face(quad, uv, -side, col, Surface::Fiber);
        }
    }
}

fn barrels(b: &mut B, p: Vec3, along: Vec3, rng: &mut Rng, wood: Vec3) {
    let count = 1 + (rng.f() * 3.0) as usize;
    for j in 0..count.min(3) {
        let q = p + along * (j as f32 * 0.62 - 0.6);
        b.m.cylinder(q - Vec3::Y * 0.03, 0.27, 0.27, 0.75, 7, true, false, wood * rng.range(0.8, 1.05), Surface::Cladding, 1.0);
        b.m.cylinder(q + Vec3::Y * 0.2, 0.285, 0.285, 0.06, 7, false, false, palette::metal(), Surface::Metal, 1.0);
    }
}

// =============================================================================================
// Tests
// =============================================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Opening, ROOF_COLORS};

    fn ground() -> Terrain {
        Terrain::build(1, &[], &[])
    }

    fn rect(hx: f32, hz: f32) -> Footprint {
        Footprint::Rect { center: Vec2::new(2.0, -3.0), half: Vec2::new(hx, hz), angle: 0.4 }
    }

    fn house(fp: Footprint, h: f32, roof: RoofStyle, pitch: f32, material: Material, ht: bool) -> House {
        House {
            footprint: fp,
            wall_height: h,
            roof,
            roof_pitch: pitch,
            material,
            fins: ht,
            foundation: 0.4,
            planters: true,
            openings: vec![],
            colors: Colors { roof: 0, wall: 1, trim: 2 },
        }
    }

    fn count(mk: &[Marker], k: MarkerKind) -> usize {
        mk.iter().filter(|m| m.kind == k).count()
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
        for fp in fps {
            for h in [0.8, 2.0, 3.6, 5.5, 10.0] {
                for roof in roofs {
                    for mat in mats {
                        for ht in [false, true] {
                            for pitch in [0.0, 0.7, 2.0] {
                                for rc in [0, 1, 4] {
                                    let mut hs = house(fp, h, roof, pitch, mat, ht);
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

    #[test]
    fn rect_house_has_door_chimney_windows_lantern() {
        let t = ground();
        let (m, mk) = house_mesh(&house(rect(3.0, 4.0), 3.5, RoofStyle::Pitched, 0.8, Material::RammedEarth, false), &[], &t, 3);
        check(&m, &mk);
        assert_eq!(count(&mk, MarkerKind::Door), 1);
        assert_eq!(count(&mk, MarkerKind::ChimneySmoke), 1);
        assert!(count(&mk, MarkerKind::WindowLight) >= 4);
        assert!(count(&mk, MarkerKind::Lantern) >= 1);
        assert!(count(&mk, MarkerKind::Perch) >= 2);
        // Smoke comes out above the ridge-ish area, well above the eaves.
        let smoke = mk.iter().find(|k| k.kind == MarkerKind::ChimneySmoke).unwrap();
        assert!(smoke.pos.y > 3.5 + 1.0);
    }

    #[test]
    fn towers() {
        let t = ground();
        let fp = Footprint::Round { center: Vec2::new(0.0, 0.0), radius: 2.5 };
        let (m, mk) = house_mesh(&house(fp, 11.0, RoofStyle::Shell, 1.5, Material::Concrete, false), &[], &t, 4);
        check(&m, &mk);
        assert!(count(&mk, MarkerKind::Perch) >= 1);
        assert!(count(&mk, MarkerKind::WindowLight) >= 4, "arrow slits");
        assert_eq!(count(&mk, MarkerKind::Door), 1);
        assert_eq!(count(&mk, MarkerKind::ChimneySmoke), 0);
        let perch = mk.iter().find(|k| k.kind == MarkerKind::Perch).unwrap();
        assert!(perch.pos.y > 11.0 + 2.5);
        // Flat tower: crenellated top, perch on the parapet.
        let (m, mk) = house_mesh(&house(fp, 9.0, RoofStyle::Green, 0.0, Material::Concrete, false), &[], &t, 4);
        check(&m, &mk);
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
        let mut hs = house(rect(3.0, 4.0), 3.0, RoofStyle::Sawtooth, 0.6, Material::RammedEarth, false);
        hs.openings = vec![Opening { along: 2.0, kind: OpeningKind::Window }];
        let (_, mk) = house_mesh(&hs, &[], &t, 5);
        assert_eq!(count(&mk, MarkerKind::Door), 0);
        let cuts = [Cut { along: 15.0, width: 1.6, height: 2.4 }, Cut { along: 25.0, width: 3.0, height: 2.4 }];
        let (m, mk) = house_mesh(&hs, &cuts, &t, 5);
        check(&m, &mk);
        assert_eq!(count(&mk, MarkerKind::Door), 2);
        assert_eq!(count(&mk, MarkerKind::WindowLight), 1);
        // Auto houses: the cut replaces the automatic door.
        hs.openings.clear();
        let (_, mk) = house_mesh(&hs, &cuts[..1], &t, 5);
        assert_eq!(count(&mk, MarkerKind::Door), 1);
    }

    #[test]
    fn tent_shelter() {
        let t = ground();
        for fp in [rect(2.0, 3.0), Footprint::Round { center: Vec2::ZERO, radius: 2.0 }] {
            for roof in [RoofStyle::Pitched, RoofStyle::Green, RoofStyle::Sawtooth, RoofStyle::Shell] {
                let (m, mk) = house_mesh(&house(fp, 0.7, roof, 0.3, Material::Timber, false), &[], &t, 6);
                check(&m, &mk);
                let (_, hi) = m.bounds().unwrap();
                assert!(hi.y > 2.0, "tent roof should rise to head height");
                assert_eq!(count(&mk, MarkerKind::WindowLight), 0);
            }
        }
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
        // Gable on a rectangle: ridge above the eaves.
        let (m, mk) = roof_mesh(&square, 4.0, RoofStyle::Pitched, 1.0, Colors::default(), 1);
        assert!(m.bounds().unwrap().1.y > 4.0 + 2.9);
        assert!(count(&mk, MarkerKind::Perch) >= 2);
    }

    #[test]
    fn yard_decor_stands_on_ground() {
        use crate::model::{TerrainDab, TerrainOp};
        let slope = [TerrainDab { center: Vec2::new(2.0, -3.0), radius: 9.0, op: TerrainOp::Sculpt { amount: 2.0 } }];
        let t = Terrain::build(5, &slope, &[]);
        let fp = rect(3.0, 2.2);
        let poly = footprint_polygon(&fp);
        let mut washing = 0;
        for seed in 0..80 {
            let hs = house(fp, 5.5, RoofStyle::Pitched, 0.8, Material::RammedEarth, true);
            let (m, _) = house_mesh(&hs, &[], &t, seed);
            // Decor-material triangles clear of the house outline (> 0.6 m outside the walls).
            let mut lowest = f32::INFINITY;
            let mut any = false;
            for tri in m.indices.chunks(3) {
                let s = m.surfaces[tri[0] as usize];
                if ![Surface::Fiber, Surface::Steel, Surface::Bark, Surface::Cladding].iter().any(|k| *k as u8 == s) {
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
                washing += 1;
                assert!(lowest < 0.3 && lowest > -0.6, "seed {seed}: decor lowest point is {lowest} m from the ground");
            }
        }
        assert!(washing > 5, "decor expected on several seeds");
    }

    #[test]
    fn deterministic() {
        let t = ground();
        let hs = house(rect(3.0, 4.0), 5.5, RoofStyle::Pitched, 1.4, Material::RammedEarth, true);
        let a = house_mesh(&hs, &[], &t, 11);
        let b = house_mesh(&hs, &[], &t, 11);
        assert_eq!(a.0, b.0);
        assert_eq!(a.1, b.1);
        // Different seeds may decorate differently, but stay valid.
        let c = house_mesh(&hs, &[], &t, 12);
        check(&c.0, &c.1);
    }

    #[test]
    fn bounds_sanity() {
        let t = ground();
        for roof in [RoofStyle::Pitched, RoofStyle::Sawtooth, RoofStyle::Shell] {
            for ht in [false, true] {
                let h = 4.0;
                let (m, _) = house_mesh(&house(rect(3.0, 4.0), h, roof, 0.8, Material::RammedEarth, ht), &[], &t, 2);
                let (lo, hi) = m.bounds().unwrap();
                assert!(lo.y >= -1.0);
                assert!(hi.y > h + 0.8 * 3.0 * 0.9, "{roof:?}: apex {hi:?}");
            }
        }
        let (m, _) = house_mesh(&house(rect(3.0, 4.0), 4.0, RoofStyle::Green, 0.0, Material::Concrete, false), &[], &t, 2);
        let (_, hi) = m.bounds().unwrap();
        assert!(hi.y > 4.0 && hi.y < 7.0);
    }

    #[test]
    fn triangle_budget() {
        let t = ground();
        for roof in [RoofStyle::Pitched, RoofStyle::Sawtooth] {
            for seed in 0..8 {
                let mut hs = house(rect(3.0, 4.0), 5.6, roof, 1.0, Material::RammedEarth, true);
                hs.colors.roof = (seed % ROOF_COLORS as u64) as u8;
                let (m, _) = house_mesh(&hs, &[], &t, seed);
                let tris = m.indices.len() / 3;
                assert!(tris <= 5000, "{tris} triangles");
            }
        }
    }

    #[test]
    fn half_timber_jetty_and_dormers() {
        let t = ground();
        let hs = house(rect(3.0, 4.5), 5.6, RoofStyle::Pitched, 1.2, Material::RammedEarth, true);
        let (m, mk) = house_mesh(&hs, &[], &t, 8);
        check(&m, &mk);
        assert!(m.surfaces.contains(&(Surface::Steel as u8)));
        assert!(m.surfaces.contains(&(Surface::Concrete as u8)));
        // Upper storey windows plus dormers (rise 3.6 m > 2.5 m).
        assert!(count(&mk, MarkerKind::WindowLight) >= 8);
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
                hs.openings = vec![Opening { along: f32::NAN, kind: OpeningKind::Door }, Opening { along: -5.0, kind: OpeningKind::Window }];
                let cuts = [Cut { along: f32::INFINITY, width: f32::NAN, height: 0.0 }, Cut { along: 1e6, width: 9.0, height: 2.0 }];
                let (m, mk) = house_mesh(&hs, &cuts, &t, 0);
                check(&m, &mk);
            }
        }
    }
}

