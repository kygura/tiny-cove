//! Dykes and windbreaks.
//!
//! A dyke (`Wall`) is sliced along its centre polyline into *spans*: pieces between polyline
//! vertices, openings, gateposts and breakwater ends. Each span knows which height bands are
//! solid (all of it, below a wind hole and above its lintel, above a door lintel, or nothing).
//! The body is then laid per kind, PS1-chunky, with vertex colours carrying the variation:
//! - `DryStone`: irregular courses of flat stones (one box each) over a recessed joint core,
//!   with a slight batter. Crest: upright copestones.
//! - `Turf`: stone base courses under lumpy alternating sod / turf bands and a rounded grassy
//!   cap. Crest: magenta thrift tufts.
//! - `Palisade`: sharpened driftwood stakes of varied height, a lashed rope rail and a driftwood
//!   rail. Crest: stake tips painted in the trim colour.
//!
//! `tumbled` dips collapsed sections and spills stones at the foot; `footing` adds a boulder row;
//! `weathering` salt-bleaches wood and blotches stone with orange / yellow lichen. Over the sea
//! the dyke becomes a breakwater: a wide rubble mole of big boulders with a flat walkable top.
//! Track cuts leave a gap between two standing-stone gateposts, lintelled when the dyke is at
//! least 2.2 m tall. Roofed rings keep a flat top; the scene puts `roof_mesh` on it.
//!
//! A windbreak (`Fence`) is a row of posts with kind-specific infill, broken where tracks cross.

use crate::geom::is_closed;
use crate::mesh::{Marker, MarkerKind, MeshData, Rng, Surface};
use crate::model::{Colors, Fence, FenceKind, OpeningKind, SEA_LEVEL, Wall, WallKind};
use crate::palette;
use crate::terrain::Terrain;
use glam::{Vec2, Vec3};
use std::f32::consts::{PI, TAU};

/// A passage the scene asks a wall/fence/house to leave open (where a path crosses it).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Cut {
    /// Distance along the host polyline / perimeter, at the passage centre.
    pub along: f32,
    /// Clear width of the passage in metres.
    pub width: f32,
    /// Clear height (lintel underside) in metres above the ground.
    pub height: f32,
}

/// Default texture scale (tiles per metre).
const UV: f32 = 0.5;
/// Stones carry their own course texture at a finer scale.
const STONE_UV: f32 = 1.0;
/// Stones and stakes are sunk this far into the ground.
const SINK: f32 = 0.08;
/// Batter: half-width lost per metre of height.
const BATTER: f32 = 0.06;
/// Gatepost length along the dyke.
const GATE_W: f32 = 0.55;
/// A track gap gets a lintel stone when the dyke is at least this tall.
const LINTEL_MIN: f32 = 2.2;
/// ... and the gap at most this wide (merged gaps of several tracks stay open).
const LINTEL_MAX: f32 = 6.0;
/// Widest gap (several merged track crossings).
const MAX_GAP: f32 = 16.0;
/// Square wind hole size.
const HOLE_W: f32 = 0.5;
/// Sea sampling step along the dyke.
const SEA_STEP: f32 = 0.5;
/// Station spacing for bands, rails and the joint core.
const STATION: f32 = 1.0;

// ---------------------------------------------------------------------------------------------
// Small helpers
// ---------------------------------------------------------------------------------------------

fn v3(p: Vec2, y: f32) -> Vec3 {
    Vec3::new(p.x, y, p.y)
}

/// Left normal of a ground direction.
fn nlv(d: Vec2) -> Vec2 {
    Vec2::new(-d.y, d.x)
}

fn ground(t: &Terrain, p: Vec2) -> f32 {
    let h = t.height(p);
    if h.is_finite() { h } else { 0.0 }
}

/// Ground or water surface, whichever is higher.
fn ground_eff(t: &Terrain, p: Vec2) -> f32 {
    let g = ground(t, p);
    t.water_level(p).filter(|w| w.is_finite()).map_or(g, |l| l.max(g))
}

/// Brightness jitter that keeps hue and saturation.
fn jit(c: Vec3, r: &mut Rng, amt: f32) -> Vec3 {
    c * (1.0 + r.range(-amt, amt))
}

/// Stone weathering: a little salt bleach plus lichen blotches on `0.35·w` of the stones.
fn weather_stone(c: Vec3, w: f32, r: &mut Rng) -> Vec3 {
    let c = c.lerp(palette::salt_bleach(), 0.2 * w);
    let (roll, which) = (r.f(), r.f());
    if roll < 0.35 * w {
        c.lerp(if which < 0.67 { palette::lichen() } else { palette::lichen_yellow() }, 0.75)
    } else {
        c
    }
}

/// Wood weathering: salt bleach by `0.6·w`, occasional lichen.
fn weather_wood(c: Vec3, w: f32, r: &mut Rng) -> Vec3 {
    let c = c.lerp(palette::salt_bleach(), 0.6 * w);
    let (roll, which) = (r.f(), r.f());
    if roll < 0.2 * w {
        c.lerp(if which < 0.67 { palette::lichen() } else { palette::lichen_yellow() }, 0.45)
    } else {
        c
    }
}

/// Quad whose winding is flipped if needed so its normal points away from `inside`.
fn out_quad(m: &mut MeshData, q: [Vec3; 4], inside: Vec3, col: Vec3, s: Surface, uv: f32) {
    let n = (q[1] - q[0]).cross(q[3] - q[0]);
    let fc = (q[0] + q[1] + q[2] + q[3]) * 0.25;
    if n.dot(fc - inside) >= 0.0 {
        m.quad(q[0], q[1], q[2], q[3], col, s, uv);
    } else {
        m.quad(q[0], q[3], q[2], q[1], col, s, uv);
    }
}

#[allow(clippy::too_many_arguments)]
fn out_tri(m: &mut MeshData, a: Vec3, b: Vec3, c: Vec3, inside: Vec3, col: Vec3, s: Surface, uv: f32) {
    let n = (b - a).cross(c - a);
    if n.dot((a + b + c) / 3.0 - inside) >= 0.0 {
        m.tri(a, b, c, col, s, uv);
    } else {
        m.tri(a, c, b, col, s, uv);
    }
}

const F_LEFT: u8 = 1;
const F_RIGHT: u8 = 2;
const F_TOP: u8 = 4;
const F_START: u8 = 8;
const F_END: u8 = 16;
const F_BOTTOM: u8 = 32;
const F_SIDES: u8 = F_LEFT | F_RIGHT | F_TOP;
const F_ALL: u8 = F_SIDES | F_START | F_END | F_BOTTOM;

/// Convex hexahedron. `b` / `t` are the bottom / top corners in order left-start, right-start,
/// right-end, left-end. Only the faces in `faces` are emitted, all facing outward.
fn hexa(m: &mut MeshData, b: [Vec3; 4], t: [Vec3; 4], faces: u8, col: Vec3, s: Surface, uv: f32) {
    let c = (b.iter().chain(t.iter()).copied().sum::<Vec3>()) / 8.0;
    let mut f = |bit: u8, q: [Vec3; 4]| {
        if faces & bit != 0 {
            out_quad(m, q, c, col, s, uv);
        }
    };
    f(F_LEFT, [b[0], b[3], t[3], t[0]]);
    f(F_RIGHT, [b[1], b[2], t[2], t[1]]);
    f(F_TOP, [t[0], t[1], t[2], t[3]]);
    f(F_START, [b[0], b[1], t[1], t[0]]);
    f(F_END, [b[3], b[2], t[2], t[3]]);
    f(F_BOTTOM, [b[0], b[1], b[2], b[3]]);
}

/// Unit frame perpendicular to `axis`.
fn frame(axis: Vec3) -> (Vec3, Vec3) {
    let helper = if axis.y.abs() < 0.9 { Vec3::Y } else { Vec3::X };
    let u = axis.cross(helper).normalize_or(Vec3::X);
    (u, axis.cross(u).normalize_or(Vec3::Z))
}

/// Tapered prism from `a` to `b` (posts, stakes, reed bundles), radii `r0` → `r1`.
#[allow(clippy::too_many_arguments)]
fn post(m: &mut MeshData, a: Vec3, b: Vec3, r0: f32, r1: f32, sides: u32, rot: f32, cap: bool, col: Vec3, s: Surface) {
    let ax = (b - a).normalize_or(Vec3::Y);
    let (u, v) = frame(ax);
    let sides = sides.max(3);
    let ring = |c: Vec3, r: f32, i: u32| {
        let t = rot + i as f32 * TAU / sides as f32;
        c + (u * t.cos() + v * t.sin()) * r
    };
    let mid = (a + b) * 0.5;
    for i in 0..sides {
        out_quad(m, [ring(a, r0, i), ring(a, r0, i + 1), ring(b, r1, i + 1), ring(b, r1, i)], mid, col, s, UV);
    }
    if cap {
        for i in 1..sides - 1 {
            out_tri(m, ring(b, r1, 0), ring(b, r1, i), ring(b, r1, i + 1), mid, col, s, UV);
        }
    }
}

/// Cone from a ring of radius `r` around `a` up to `apex` (stake points, reed tips).
#[allow(clippy::too_many_arguments)]
fn cone(m: &mut MeshData, a: Vec3, apex: Vec3, r: f32, sides: u32, rot: f32, col: Vec3, s: Surface) {
    let ax = (apex - a).normalize_or(Vec3::Y);
    let (u, v) = frame(ax);
    let sides = sides.max(3);
    let ring = |i: u32| {
        let t = rot + i as f32 * TAU / sides as f32;
        a + (u * t.cos() + v * t.sin()) * r
    };
    let inside = a + (apex - a) * 0.25;
    for i in 0..sides {
        out_tri(m, ring(i), ring(i + 1), apex, inside, col, s, UV);
    }
}

/// Chunky low-poly boulder: a jittered bipyramid over `k` equator points (2·k tris).
#[allow(clippy::too_many_arguments)]
fn boulder(m: &mut MeshData, c: Vec3, r: f32, squash: f32, k: u32, rng: &mut Rng, col: Vec3, s: Surface) {
    let k = k.max(3);
    let rot = rng.range(0.0, TAU);
    let eq: Vec<Vec3> = (0..k)
        .map(|i| {
            let a = rot + i as f32 * TAU / k as f32 + rng.range(-0.25, 0.25);
            let rr = r * rng.range(0.8, 1.15);
            c + Vec3::new(a.cos() * rr, rng.range(-0.12, 0.12) * r, a.sin() * rr)
        })
        .collect();
    let top = c + Vec3::new(rng.range(-0.2, 0.2) * r, r * squash, rng.range(-0.2, 0.2) * r);
    let bot = c - Vec3::Y * (r * squash * 0.6);
    for i in 0..k as usize {
        let j = (i + 1) % k as usize;
        out_tri(m, eq[i], eq[j], top, c, col, s, STONE_UV);
        out_tri(m, eq[j], eq[i], bot, c, col, s, STONE_UV);
    }
}

/// Small regular octahedron bead (net floats).
fn bead(m: &mut MeshData, c: Vec3, r: f32, col: Vec3, s: Surface) {
    let eq = [Vec3::X, Vec3::Z, -Vec3::X, -Vec3::Z].map(|d| c + d * r);
    let (top, bot) = (c + Vec3::Y * r * 1.2, c - Vec3::Y * r * 1.2);
    for i in 0..4 {
        let j = (i + 1) % 4;
        out_tri(m, eq[i], eq[j], top, c, col, s, UV);
        out_tri(m, eq[j], eq[i], bot, c, col, s, UV);
    }
}

// ---------------------------------------------------------------------------------------------
// Polyline
// ---------------------------------------------------------------------------------------------

struct Line {
    p: Vec<Vec2>,
    cum: Vec<f32>,
    closed: bool,
}

impl Line {
    fn new(pts: &[Vec2]) -> Option<Line> {
        let mut p: Vec<Vec2> = Vec::new();
        for &q in pts {
            if q.is_finite() && q.abs().max_element() < 1.0e4 && p.last().is_none_or(|l| l.distance(q) >= 0.05) {
                p.push(q);
            }
        }
        if p.len() < 2 {
            return None;
        }
        let closed = is_closed(&p);
        if closed {
            let f = p[0];
            *p.last_mut()? = f;
        }
        let mut cum = vec![0.0];
        for w in p.windows(2) {
            cum.push(cum[cum.len() - 1] + w[0].distance(w[1]));
        }
        if cum[cum.len() - 1] < 0.1 {
            return None;
        }
        Some(Line { p, cum, closed })
    }
    fn segs(&self) -> usize {
        self.p.len() - 1
    }
    fn total(&self) -> f32 {
        self.cum[self.cum.len() - 1]
    }
    fn dir(&self, i: usize) -> Vec2 {
        (self.p[i + 1] - self.p[i]).normalize_or(Vec2::X)
    }
    /// Point and segment direction at distance `s`.
    fn at(&self, s: f32) -> (Vec2, Vec2) {
        let s = s.clamp(0.0, self.total());
        let i = self.cum.partition_point(|&c| c <= s).saturating_sub(1).min(self.segs() - 1);
        let d = self.dir(i);
        (self.p[i] + d * (s - self.cum[i]), d)
    }
    /// Directions before / after vertex `i`.
    fn vdirs(&self, i: usize) -> (Vec2, Vec2) {
        let n = self.segs();
        let next = if i < n {
            self.dir(i)
        } else if self.closed {
            self.dir(0)
        } else {
            self.dir(n - 1)
        };
        let prev = if i > 0 {
            self.dir(i - 1)
        } else if self.closed {
            self.dir(n - 1)
        } else {
            next
        };
        (prev, next)
    }
    /// Left offset per metre of half-width at `s`: mitred at interior vertices, plain normal of
    /// `d` elsewhere.
    fn normal_at(&self, s: f32, d: Vec2) -> Vec2 {
        for (i, &c) in self.cum.iter().enumerate() {
            if (c - s).abs() < 1e-3 && (self.closed || (i > 0 && i < self.segs())) {
                let (d0, d1) = self.vdirs(i);
                let (n0, n1) = (nlv(d0), nlv(d1));
                let k = 1.0 + n0.dot(n1);
                return if k < 0.15 { n1 } else { (n0 + n1) / k.max(0.3) };
            }
        }
        nlv(d)
    }
    /// The same closed ring, restarted at distance `s` (on a vertex or a segment).
    fn rotated(&self, s: f32) -> Option<Line> {
        let i = self.cum.partition_point(|&c| c <= s).saturating_sub(1).min(self.segs() - 1);
        let (q, _) = self.at(s);
        let mut pts = vec![q];
        pts.extend_from_slice(&self.p[i + 1..]);
        pts.extend_from_slice(&self.p[1..=i]);
        pts.push(q);
        Line::new(&pts)
    }
    /// True if a polyline vertex lies strictly inside `(a, b)`.
    fn vertex_in(&self, a: f32, b: f32) -> bool {
        self.cum.iter().any(|&c| c > a + 0.02 && c < b - 0.02)
    }
}

/// Keeps a band end at least 2 cm tall; an end whose top falls below its bottom is pulled down
/// under the top instead, so the piece wedges along the top line rather than poking above it.
fn lift(lo: f32, hi: f32) -> (f32, f32) {
    if hi < lo + 0.02 { (hi - 0.02, hi) } else { (lo, hi) }
}

/// `n` evenly spaced stations covering `[a, b]` at roughly `step` spacing (both ends included).
fn stations(a: f32, b: f32, step: f32) -> Vec<f32> {
    let n = ((b - a) / step).ceil().clamp(1.0, 4096.0) as usize;
    (0..=n).map(|i| a + (b - a) * i as f32 / n as f32).collect()
}

// ---------------------------------------------------------------------------------------------
// Dyke
// ---------------------------------------------------------------------------------------------

/// What a span of the dyke keeps (absolute world heights).
#[derive(Clone, Copy, Debug, PartialEq)]
enum Hole {
    /// Full body.
    Solid,
    /// Nothing at all (open gap, gatepost slot without lintel).
    Open,
    /// Only above this height (over a lintel).
    Above(f32),
    /// Below the sill and above the lintel top (wind hole).
    Window(f32, f32),
    /// Breakwater instead of a wall.
    Sea,
}

impl Hole {
    /// Solid height bands `(lo, hi)`: `None` lo = from the base, `None` hi = to the top.
    fn bands(self) -> Vec<(Option<f32>, Option<f32>)> {
        match self {
            Hole::Solid => vec![(None, None)],
            Hole::Above(y) => vec![(Some(y), None)],
            Hole::Window(sill, top) => vec![(None, Some(sill)), (Some(top), None)],
            Hole::Open | Hole::Sea => Vec::new(),
        }
    }
}

struct Span {
    s0: f32,
    s1: f32,
    d: Vec2,
    hole: Hole,
}

struct Feat {
    /// Hole extent along the line.
    a: f32,
    b: f32,
    /// Extent including lintel overhang / gateposts (for overlap tests).
    ea: f32,
    eb: f32,
    hole: Hole,
}

struct Lintel {
    a: f32,
    b: f32,
    y: f32,
    th: f32,
    timber: bool,
}

struct Gate {
    a: f32,
    b: f32,
    top: f32,
    lintel: bool,
}

/// One station sample along the dyke.
#[derive(Clone, Copy)]
struct Smp {
    p: Vec2,
    /// Stone base (ground minus sink).
    base: f32,
    /// Full body height above the base.
    full: f32,
    /// Body height above the base after collapses.
    ht: f32,
}

struct Dyke<'a> {
    w: &'a Wall,
    line: Line,
    terr: &'a Terrain,
    seed: u64,
    hgt: f32,
    half: f32,
    weather: f32,
    flat_top: Option<f32>,
    collapses: Vec<(f32, f32, f32)>,
}

/// Wall mesh plus markers (perches on top, lanterns on lintelled gateposts, doors).
pub fn wall_mesh(wall: &Wall, cuts: &[Cut], terrain: &Terrain, seed: u64) -> (MeshData, Vec<Marker>) {
    let mut m = MeshData::default();
    let mut mk = Vec::new();
    if let Some(d) = Dyke::new(wall, terrain, seed) {
        d.build(cuts, &mut m, &mut mk);
    }
    if m.is_empty() || !m.is_valid() || mk.iter().any(|k| !k.pos.is_finite() || !k.dir.is_finite()) {
        return (MeshData::default(), Vec::new());
    }
    (m, mk)
}

impl<'a> Dyke<'a> {
    fn new(w: &'a Wall, terr: &'a Terrain, seed: u64) -> Option<Self> {
        let line = Line::new(&w.points)?;
        if !w.height.is_finite() || w.height < 0.3 {
            return None;
        }
        let hgt = w.height.min(40.0);
        let th = if w.thickness.is_finite() { w.thickness.clamp(0.2, 2.5) } else { 0.6 };
        let weather = if w.weathering.is_finite() { w.weathering.clamp(0.0, 1.0) } else { 0.0 };
        // Roofed rings keep a flat top (the scene roofs at max ground-or-water + height).
        let flat_top = (line.closed && w.roofed)
            .then(|| line.p.iter().map(|&p| ground_eff(terr, p)).fold(f32::MIN, f32::max) + hgt);
        Some(Dyke { w, line, terr, seed, hgt, half: th * 0.5, weather, flat_top, collapses: Vec::new() })
    }

    fn salt(&self, k: u64) -> u64 {
        self.seed ^ k.wrapping_mul(0x9E37_79B9_7F4A_7C15)
    }

    fn rng(&self, k: u64, p: Vec2) -> Rng {
        Rng::at(self.salt(k), p.x, p.y)
    }

    fn profile(&self, s: f32) -> f32 {
        let mut p: f32 = 1.0;
        for &(c, hw, d) in &self.collapses {
            let u = (s - c) / hw;
            if u.abs() < 1.0 {
                p = p.min(1.0 - d * 0.5 * (1.0 + (PI * u).cos()));
            }
        }
        p
    }

    fn smp(&self, s: f32) -> Smp {
        let (p, _) = self.line.at(s);
        let base = ground(self.terr, p) - SINK;
        let top = self.flat_top.unwrap_or_else(|| ground_eff(self.terr, p) + self.hgt);
        let full = (top - base).max(0.0);
        Smp { p, base, full, ht: full * self.profile(s) }
    }

    fn half_at(&self, h: f32) -> f32 {
        (self.half - BATTER * h.max(0.0)).max(self.half * 0.65)
    }

    fn stone_col(&self, r: &mut Rng) -> Vec3 {
        let c = if r.f() < 0.33 { palette::dry_stone_alt() } else { palette::dry_stone() };
        weather_stone(jit(c, r, 0.06), self.weather, r)
    }

    fn wood_col(&self, r: &mut Rng) -> Vec3 {
        weather_wood(jit(palette::driftwood(), r, 0.05), self.weather, r)
    }

    /// Contiguous stretches over the sea, as `(a, b)` distances.
    fn sea_runs(&self) -> Vec<(f32, f32)> {
        let total = self.line.total();
        let n = ((total / SEA_STEP).ceil() as usize).clamp(1, 20_000);
        let step = total / n as f32;
        let wet: Vec<bool> = (0..=n).map(|i| self.terr.is_sea(self.line.at(i as f32 * step).0)).collect();
        let mut runs = Vec::new();
        let mut i = 0;
        while i <= n {
            if !wet[i] {
                i += 1;
                continue;
            }
            let mut j = i;
            while j < n && wet[j + 1] {
                j += 1;
            }
            let mut a = ((i as f32 - 0.5) * step).max(0.0);
            let mut b = ((j as f32 + 0.5) * step).min(total);
            if a < 0.6 {
                a = 0.0;
            }
            if total - b < 0.6 {
                b = total;
            }
            runs.push((a, b));
            i = j + 1;
        }
        runs
    }

    /// Track cuts in this dyke's own `along` (see `build`), widths clamped, sorted, and merged
    /// wherever two gaps would leave less than a pair of gateposts of wall between them.
    fn prepare_cuts(&self, cuts: &[Cut], loc: impl Fn(f32) -> f32) -> Vec<Cut> {
        let mut v: Vec<Cut> = cuts
            .iter()
            .filter(|c| c.along.is_finite() && c.width.is_finite() && c.height.is_finite())
            .map(|c| Cut { along: loc(c.along), width: c.width.clamp(0.8, MAX_GAP), height: c.height })
            .collect();
        v.sort_by(|a, b| a.along.total_cmp(&b.along));
        let mut merged: Vec<Cut> = Vec::new();
        for c in v {
            if let Some(l) = merged.last_mut() {
                let (lo, hi) = (c.along - c.width * 0.5, l.along + l.width * 0.5);
                if lo - hi < 2.0 * GATE_W + 0.15 {
                    let (a, b) = ((l.along - l.width * 0.5).min(lo), hi.max(c.along + c.width * 0.5));
                    *l = Cut { along: (a + b) * 0.5, width: (b - a).min(MAX_GAP), height: l.height.max(c.height) };
                    continue;
                }
            }
            merged.push(c);
        }
        merged
    }

    #[allow(clippy::too_many_lines)]
    fn build(mut self, cuts: &[Cut], m: &mut MeshData, mk: &mut Vec<Marker>) {
        let total = self.line.total();
        let palisade = self.w.kind == WallKind::Palisade;
        // A closed ring has no real ends: if a track crosses near its seam, restart the ring
        // at the corner (or segment middle) farthest from every track so no gap straddles it.
        let mut off = 0.0;
        if self.line.closed {
            let reach = |c: &Cut| c.width.clamp(0.8, MAX_GAP) * 0.5 + GATE_W + 0.15;
            let ok = |c: &&Cut| c.along.is_finite() && c.width.is_finite();
            if cuts.iter().filter(ok).any(|c| c.along - reach(c) < 0.0 || c.along + reach(c) > total) {
                let clear = |s: f32| {
                    cuts.iter().filter(ok).map(|c| {
                        let d = (s - c.along).rem_euclid(total);
                        d.min(total - d) - reach(c)
                    })
                    .fold(f32::MAX, f32::min)
                };
                let segs = self.line.cum.windows(2).filter(|w| w[1] - w[0] > 0.2).map(|w| (w[0] + w[1]) * 0.5);
                let best = self.line.cum[1..self.line.segs()].iter().copied().chain(segs).max_by(|a, b| clear(*a).total_cmp(&clear(*b)));
                if let Some(line) = best.filter(|s| clear(*s) > 0.0).and_then(|s| self.line.rotated(s).map(|l| (s, l))) {
                    (off, self.line) = line;
                }
            }
        }
        let total = self.line.total();
        let loc = |s: f32| if off == 0.0 { s } else { (s - off).rem_euclid(total) };
        // A roofed ring keeps its flat-topped wall over the water too, so the roof rests on it
        // all round instead of floating over a low breakwater.
        let sea = if self.flat_top.is_some() { Vec::new() } else { self.sea_runs() };
        let in_sea = |a: f32, b: f32| sea.iter().any(|&(x, y)| a < y && b > x);
        let busy = |feats: &[Feat], a: f32, b: f32| feats.iter().any(|f| a < f.eb + 0.1 && b > f.ea - 0.1);
        let mut feats: Vec<Feat> = Vec::new();
        let mut lintels: Vec<Lintel> = Vec::new();
        let mut gates: Vec<Gate> = Vec::new();

        // ---- track cuts: gap between two standing-stone gateposts ---------------------------
        for c in self.prepare_cuts(cuts, loc) {
            let wd = c.width;
            let posts = total >= wd + 2.0 * GATE_W + 0.2;
            let margin = wd * 0.5 + if posts { GATE_W } else { 0.0 } + 0.02;
            if margin * 2.0 > total {
                continue;
            }
            let along = c.along.clamp(0.0, total);
            let sc = along.clamp(margin, total - margin);
            let (mut a, mut b) = (sc - wd * 0.5, sc + wd * 0.5);
            let (mut post_a, mut post_b) = (posts, posts);
            // Near an open end the gap (and a gatepost) would slide over the track: run the gap
            // out to that end instead, with a post on the inner side only.
            if !self.line.closed && (sc - along).abs() > 0.05 {
                if sc > along {
                    (a, b, post_a) = (0.0, along + wd * 0.5, false);
                } else {
                    (a, b, post_b) = (along - wd * 0.5, total, false);
                }
            }
            let ea = if post_a { a - GATE_W } else { a };
            let eb = if post_b { b + GATE_W } else { b };
            if busy(&feats, ea, eb) || in_sea(ea, eb) {
                continue;
            }
            let sm = self.smp((a + b) * 0.5);
            let above_ground = sm.full - SINK;
            if post_a && post_b && above_ground >= LINTEL_MIN && wd <= LINTEL_MAX {
                let clear = c.height.clamp(1.8, (above_ground - 0.32).max(1.8));
                let y = sm.base + SINK + clear;
                let top = y + 0.3;
                feats.push(Feat { a: ea, b: eb, ea, eb, hole: Hole::Above(top) });
                gates.push(Gate { a: ea, b: a, top: y, lintel: true });
                gates.push(Gate { a: b, b: eb, top: y, lintel: true });
                lintels.push(Lintel { a: ea, b: eb, y, th: 0.3, timber: false });
            } else {
                feats.push(Feat { a: ea, b: eb, ea, eb, hole: Hole::Open });
                let top = sm.base + SINK + (above_ground + 0.45).max(1.3);
                if post_a {
                    gates.push(Gate { a: ea, b: a, top, lintel: false });
                }
                if post_b {
                    gates.push(Gate { a: b, b: eb, top, lintel: false });
                }
            }
        }

        // ---- player openings ----------------------------------------------------------------
        for o in &self.w.openings {
            if !o.along.is_finite() {
                continue;
            }
            let along = loc(o.along);
            let (wd, pad) = match o.kind {
                OpeningKind::Window => (HOLE_W, 0.2),
                OpeningKind::Door => (1.1, 0.25),
                OpeningKind::WideDoor => (2.4, 0.3),
            };
            let (a, b) = (along - wd * 0.5, along + wd * 0.5);
            let (ea, eb) = (a - pad, b + pad);
            if ea < 0.05 || eb > total - 0.05 || busy(&feats, ea, eb) || in_sea(ea, eb) || self.line.vertex_in(ea, eb) {
                continue;
            }
            let sm = self.smp(along);
            let gy = sm.base + SINK;
            let hg = sm.full - SINK;
            let (p, d) = self.line.at(along);
            let n = nlv(d);
            let door = |mk: &mut Vec<Marker>| {
                mk.push(Marker { kind: MarkerKind::Door, pos: v3(p + n * (self.half + 0.5), gy), dir: v3(n, 0.0) });
            };
            let hole = match o.kind {
                OpeningKind::Window => {
                    let sill = (hg * 0.42).clamp(0.5, 1.0);
                    if sill + HOLE_W + 0.22 + 0.1 > hg {
                        continue;
                    }
                    let head = gy + sill + HOLE_W;
                    lintels.push(Lintel { a: ea, b: eb, y: head, th: 0.22, timber: palisade });
                    Hole::Window(gy + sill, head + 0.22)
                }
                OpeningKind::Door => {
                    door(mk);
                    if hg >= 2.3 {
                        lintels.push(Lintel { a: ea, b: eb, y: gy + 1.9, th: 0.26, timber: palisade });
                        Hole::Above(gy + 1.9 + 0.26)
                    } else {
                        Hole::Open
                    }
                }
                OpeningKind::WideDoor => {
                    door(mk);
                    if hg >= 2.75 {
                        lintels.push(Lintel { a: ea, b: eb, y: gy + 2.3, th: 0.24, timber: true });
                        Hole::Above(gy + 2.3 + 0.24)
                    } else {
                        Hole::Open
                    }
                }
            };
            feats.push(Feat { a, b, ea, eb, hole });
        }

        // ---- collapses (tumbled) --------------------------------------------------------------
        if self.w.tumbled {
            self.collapses = self.make_collapses(&feats);
        }

        // ---- spans ------------------------------------------------------------------------------
        let mut bp: Vec<f32> = self.line.cum.clone();
        for f in &feats {
            bp.extend([f.a, f.b]);
        }
        for &(a, b) in &sea {
            bp.extend([a, b]);
        }
        let mut bp: Vec<f32> = bp.into_iter().filter(|s| s.is_finite()).map(|s| s.clamp(0.0, total)).collect();
        bp.sort_by(f32::total_cmp);
        bp.dedup_by(|x, y| (*x - *y).abs() < 1e-3);
        let spans: Vec<Span> = bp
            .windows(2)
            .filter(|w| w[1] - w[0] >= 1e-3)
            .map(|w| {
                let mid = (w[0] + w[1]) * 0.5;
                let hole = if sea.iter().any(|&(a, b)| mid >= a && mid <= b) {
                    Hole::Sea
                } else {
                    feats.iter().find(|f| mid >= f.a && mid <= f.b).map_or(Hole::Solid, |f| f.hole)
                };
                Span { s0: w[0], s1: w[1], d: self.line.at(mid).1, hole }
            })
            .collect();

        // ---- body ---------------------------------------------------------------------------
        match self.w.kind {
            WallKind::DryStone => {
                self.core(&spans, None, m);
                self.stones(&spans, None, m);
                if self.w.crest {
                    self.copestones(&spans, m);
                }
            }
            WallKind::Turf => {
                let stone_h = (self.hgt * 0.38).clamp(0.3, 0.8) + SINK;
                self.core(&spans, Some(stone_h), m);
                self.stones(&spans, Some(stone_h), m);
                self.turf(&spans, stone_h, m);
            }
            WallKind::Palisade => {
                self.stakes(&spans, m);
                self.rails(&spans, m);
            }
        }
        if self.w.footing.is_finite() && self.w.footing > 0.0 {
            self.footing(&spans, self.w.footing.min(self.hgt), m);
        }
        for g in &gates {
            self.gatepost(g, m, mk);
        }
        for l in &lintels {
            self.lintel(l, m);
        }
        self.spill(&sea, m);
        for &(a, b) in &sea {
            self.breakwater(a, b, m, mk);
        }
        self.perches(&spans, mk);
    }

    fn make_collapses(&self, feats: &[Feat]) -> Vec<(f32, f32, f32)> {
        let total = self.line.total();
        let clear = |c: f32, hw: f32| !feats.iter().any(|f| c - hw < f.eb + 0.2 && c + hw > f.ea - 0.2);
        let mut v = Vec::new();
        let mut j = 0;
        while (j as f32) * 4.0 < total && j < 2000 {
            let s0 = j as f32 * 4.0;
            let mut r = self.rng(11, self.line.at(s0).0);
            if r.f() < 0.5 {
                let c = s0 + r.range(1.0, 3.0);
                let hw = r.range(0.8, 1.7);
                let d = r.range(0.55, 0.85);
                if c < total - 0.3 && clear(c, hw) {
                    v.push((c, hw, d));
                }
            }
            j += 1;
        }
        if v.is_empty() && total >= 2.0 {
            let hw = (total * 0.25).min(1.4);
            if let Some(c) = [0.5, 0.3, 0.7, 0.15, 0.85].iter().map(|f| total * f).find(|&c| clear(c, hw)) {
                v.push((c, hw, 0.7));
            }
        }
        v
    }

    /// Height band `(lo, hi)` evaluated at a sample (heights above the base, `hi` limited to the
    /// collapsed top); `cap` limits the top further (turf stone base).
    fn band_at(&self, band: (Option<f32>, Option<f32>), sm: &Smp, cap: Option<f32>) -> (f32, f32) {
        let lo = band.0.map_or(0.0, |y| y - sm.base);
        let hi = band.1.map_or(sm.ht, |y| (y - sm.base).min(sm.ht));
        (lo, cap.map_or(hi, |c| hi.min(c)))
    }

    /// Course boundaries (heights above the base).
    fn courses(&self) -> Vec<f32> {
        let total = self.line.total();
        let hmax = stations(0.0, total, 2.0).iter().map(|&s| self.smp(s).full).fold(0.0, f32::max).min(41.0) + 0.3;
        let mut r = self.rng(21, self.line.p[0]);
        let mut y = vec![0.0];
        while y[y.len() - 1] < hmax {
            let last = y[y.len() - 1];
            y.push(last + r.range(0.28, 0.46));
        }
        y
    }

    /// Stone joints along course `k`.
    fn breaks(&self, k: usize) -> Vec<f32> {
        let total = self.line.total();
        let mut r = self.rng(31 + k as u64, self.line.p[0]);
        let mut s = -r.range(0.0, 0.9);
        let mut v = Vec::new();
        while s < total && v.len() < 20_000 {
            if s > 0.0 {
                v.push(s);
            }
            s += r.range(0.55, 1.25);
        }
        v
    }

    /// Recessed joint core behind the stones: shows through joints, dips and slits.
    fn core(&self, spans: &[Span], cap: Option<f32>, m: &mut MeshData) {
        let col = palette::stone_joint();
        for sp in spans {
            let bands = sp.hole.bands();
            if bands.is_empty() {
                continue;
            }
            let st = stations(sp.s0, sp.s1, STATION);
            for w in st.windows(2) {
                let (a, b) = (self.smp(w[0]), self.smp(w[1]));
                let (na, nb) = (self.line.normal_at(w[0], sp.d), self.line.normal_at(w[1], sp.d));
                let hw = self.half * 0.6;
                for &band in &bands {
                    let (l0, t0) = self.band_at(band, &a, cap);
                    let (l1, t1) = self.band_at(band, &b, cap);
                    if (t0 - l0).max(t1 - l1) < 0.05 {
                        continue;
                    }
                    let ((l0, t0), (l1, t1)) = (lift(l0, t0 - 0.02), lift(l1, t1 - 0.02));
                    let bt = [v3(a.p + na * hw, a.base + l0), v3(a.p - na * hw, a.base + l0), v3(b.p - nb * hw, b.base + l1), v3(b.p + nb * hw, b.base + l1)];
                    let tp = [v3(a.p + na * hw, a.base + t0), v3(a.p - na * hw, a.base + t0), v3(b.p - nb * hw, b.base + t1), v3(b.p + nb * hw, b.base + t1)];
                    hexa(m, bt, tp, F_SIDES, col, Surface::DryStone, STONE_UV);
                }
            }
        }
    }

    /// Stacked flat stones in irregular courses with a slight batter.
    fn stones(&self, spans: &[Span], cap: Option<f32>, m: &mut MeshData) {
        let ys = self.courses();
        for k in 0..ys.len() - 1 {
            let (y0, y1) = (ys[k], ys[k + 1]);
            if cap.is_some_and(|c| y0 >= c - 0.05) {
                break;
            }
            let br = self.breaks(k);
            for sp in spans {
                let bands = sp.hole.bands();
                if bands.is_empty() {
                    continue;
                }
                let mut cuts = vec![sp.s0];
                cuts.extend(br.iter().copied().filter(|&s| s > sp.s0 + 0.08 && s < sp.s1 - 0.08));
                cuts.push(sp.s1);
                for w in cuts.windows(2) {
                    self.stone(m, sp, w[0], w[1], k, (y0, y1), &bands, cap);
                }
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn stone(&self, m: &mut MeshData, sp: &Span, u0: f32, u1: f32, k: usize, (y0, y1): (f32, f32), bands: &[(Option<f32>, Option<f32>)], cap: Option<f32>) {
        let (at0, at1) = (u0 <= sp.s0, u1 >= sp.s1);
        let mid = self.smp((u0 + u1) * 0.5);
        let mut r = self.rng(41 + k as u64, mid.p);
        let ragged = self.w.tumbled && mid.ht < mid.full * 0.97;
        let dip = r.range(0.0, 0.07);
        let col = self.stone_col(&mut r);
        let (jl, jr) = (r.range(-0.01, 0.05), r.range(-0.01, 0.05));
        let drop = r.range(0.0, 0.3);
        // Interior joints shrink a little so the joint core shows through.
        let (e0, e1) = (if at0 { 0.0 } else { 0.015 }, if at1 { 0.0 } else { 0.015 });
        let (a, b) = (self.smp(u0), self.smp(u1));
        let (pa, pb) = (a.p + sp.d * e0, b.p - sp.d * e1);
        let (na, nb) = (self.line.normal_at(u0, sp.d), self.line.normal_at(u1, sp.d));
        let yc = cap.map_or(y1, |c| y1.min(c)) - dip;
        for &band in bands {
            if ragged && band.1.is_none() && y0 + 0.08 + drop > mid.ht {
                continue;
            }
            let ev = |s: &Smp| {
                let mut s2 = *s;
                if ragged {
                    s2.ht = s2.full;
                }
                let (lo, hi) = self.band_at(band, &s2, cap);
                (lo.max(y0), hi.min(yc))
            };
            let (l0, t0) = ev(&a);
            let (l1, t1) = ev(&b);
            if (t0 - l0).max(t1 - l1) < 0.05 {
                continue;
            }
            let ((l0, t0), (l1, t1)) = (lift(l0, t0), lift(l1, t1));
            let hw = self.half_at((l0 + l1) * 0.5);
            let (wl, wr) = (hw + jl, hw + jr);
            let bt = [v3(pa + na * wl, a.base + l0), v3(pa - na * wr, a.base + l0), v3(pb - nb * wr, b.base + l1), v3(pb + nb * wl, b.base + l1)];
            let tp = [v3(pa + na * wl, a.base + t0), v3(pa - na * wr, a.base + t0), v3(pb - nb * wr, b.base + t1), v3(pb + nb * wl, b.base + t1)];
            let faces = F_SIDES | if at0 { F_START } else { 0 } | if at1 { F_END } else { 0 };
            hexa(m, bt, tp, faces, col, Surface::DryStone, STONE_UV);
        }
    }

    /// Upright copestones on edge along the crest.
    fn copestones(&self, spans: &[Span], m: &mut MeshData) {
        for sp in spans {
            if !sp.hole.bands().iter().any(|b| b.1.is_none()) {
                continue;
            }
            let n = ((sp.s1 - sp.s0) / 0.3).floor() as usize;
            for i in 0..n {
                let s = sp.s0 + (sp.s1 - sp.s0) * (i as f32 + 0.5) / n as f32;
                let sm = self.smp(s);
                if sm.ht < sm.full * 0.95 {
                    continue;
                }
                let mut r = self.rng(51, sm.p);
                let d = sp.d;
                let nn = nlv(d);
                let ta = r.range(0.12, 0.2) * 0.5;
                let hc = r.range(0.26, 0.42);
                let ac = self.half_at(sm.ht) + 0.02;
                let tilt = d * r.range(-0.05, 0.05);
                let y = sm.base + sm.ht - 0.02;
                let c = sm.p;
                let foot = [c - d * ta + nn * ac, c - d * ta - nn * ac, c + d * ta - nn * ac, c + d * ta + nn * ac];
                let bt = foot.map(|q| v3(q, y));
                let tp = [0, 1, 2, 3].map(|i| v3(foot[i] + tilt, y + hc + r.range(-0.04, 0.04)));
                let col = self.stone_col(&mut r);
                hexa(m, bt, tp, F_SIDES | F_START | F_END, col, Surface::DryStone, STONE_UV);
            }
        }
    }

    /// Lumpy alternating sod / turf bands over the stone base, a rounded grassy cap and
    /// (crest) thrift tufts.
    fn turf(&self, spans: &[Span], stone_h: f32, m: &mut MeshData) {
        const BAND: f32 = 0.3;
        const CAP: f32 = 0.32;
        let lump = |p: Vec2, j: usize| self.rng(61 + j as u64, p).range(-0.03, 0.07);
        for sp in spans {
            let bands = sp.hole.bands();
            if bands.is_empty() {
                continue;
            }
            let st = stations(sp.s0, sp.s1, STATION);
            for (wi, w) in st.windows(2).enumerate() {
                let (a, b) = (self.smp(w[0]), self.smp(w[1]));
                let (na, nb) = (self.line.normal_at(w[0], sp.d), self.line.normal_at(w[1], sp.d));
                let ends = (if wi == 0 { F_START } else { 0 }) | (if wi + 2 == st.len() { F_END } else { 0 });
                for &band in &bands {
                    let to_top = band.1.is_none();
                    let lim = |s: &Smp| {
                        let (lo, hi) = self.band_at(band, s, None);
                        (lo, if to_top { hi - CAP } else { hi })
                    };
                    let (lo0, hi0) = lim(&a);
                    let (lo1, hi1) = lim(&b);
                    let mut j = 0;
                    loop {
                        let y0 = stone_h + j as f32 * BAND;
                        if y0 >= hi0.max(hi1) || j > 200 {
                            break;
                        }
                        let y1 = y0 + BAND;
                        let (l0, t0) = (y0.max(lo0), y1.min(hi0));
                        let (l1, t1) = (y0.max(lo1), y1.min(hi1));
                        if (t0 - l0).max(t1 - l1) >= 0.04 {
                            let ((l0, t0), (l1, t1)) = (lift(l0, t0), lift(l1, t1));
                            let (wa, wb) = (self.half_at(l0) + lump(a.p, j), self.half_at(l1) + lump(b.p, j));
                            let bt = [v3(a.p + na * wa, a.base + l0), v3(a.p - na * wa, a.base + l0), v3(b.p - nb * wb, b.base + l1), v3(b.p + nb * wb, b.base + l1)];
                            let tp = [v3(a.p + na * wa, a.base + t0), v3(a.p - na * wa, a.base + t0), v3(b.p - nb * wb, b.base + t1), v3(b.p + nb * wb, b.base + t1)];
                            let mut r = self.rng(71 + j as u64, (a.p + b.p) * 0.5);
                            let col = jit(if j % 2 == 0 { palette::sod() } else { palette::turf() }, &mut r, 0.06);
                            hexa(m, bt, tp, F_SIDES | ends, col, Surface::Turf, UV);
                        }
                        j += 1;
                    }
                    if !to_top {
                        continue;
                    }
                    // Rounded grassy cap.
                    let c0 = (a.ht - CAP).max(lo0).max(0.0);
                    let c1 = (b.ht - CAP).max(lo1).max(0.0);
                    if (a.ht - c0).max(b.ht - c1) < 0.04 {
                        continue;
                    }
                    let (wa, wb) = (self.half_at(c0) + lump(a.p, 999), self.half_at(c1) + lump(b.p, 999));
                    let (ta, tb) = (wa * 0.55, wb * 0.55);
                    let bt = [v3(a.p + na * wa, a.base + c0), v3(a.p - na * wa, a.base + c0), v3(b.p - nb * wb, b.base + c1), v3(b.p + nb * wb, b.base + c1)];
                    let tp = [v3(a.p + na * ta, a.base + a.ht), v3(a.p - na * ta, a.base + a.ht), v3(b.p - nb * tb, b.base + b.ht), v3(b.p + nb * tb, b.base + b.ht)];
                    hexa(m, bt, tp, F_SIDES | ends, palette::turf(), Surface::Grass, UV);
                    if self.w.crest {
                        self.thrift(&a, &b, sp.d, m);
                    }
                }
            }
        }
    }

    /// Magenta thrift tufts along a turf crest piece.
    fn thrift(&self, a: &Smp, b: &Smp, d: Vec2, m: &mut MeshData) {
        let len = a.p.distance(b.p);
        let n = ((len / 0.45).round() as usize).max(1);
        for i in 0..n {
            let t = (i as f32 + 0.5) / n as f32;
            let p = a.p.lerp(b.p, t);
            let y = a.base + a.ht + (b.base + b.ht - a.base - a.ht) * t;
            let mut r = self.rng(81, p);
            let c = p + nlv(d) * r.range(-0.15, 0.15) * self.half;
            let cushion = r.range(0.1, 0.16);
            boulder(m, v3(c, y + 0.02), cushion, 0.6, 4, &mut r, palette::grass_tip(), Surface::Grass);
            let col = if r.f() < 0.6 { palette::flower(0) } else { palette::flower(1) };
            cone(m, v3(c, y + cushion * 0.5), v3(c, y + cushion * 0.5 + r.range(0.12, 0.2)), cushion * 0.8, 4, r.range(0.0, TAU), col, Surface::Petal);
        }
    }

    /// Stake rows: offsets across the dyke (fractions of the half-width) and stake radius.
    fn stake_layout(&self) -> (Vec<f32>, f32) {
        let r = (self.half * 0.45).clamp(0.08, 0.15);
        let rows = if self.half >= 0.35 { vec![0.45, -0.45] } else { vec![0.0] };
        (rows, r)
    }

    /// Sharpened driftwood stakes of varied height (crest: painted tips).
    fn stakes(&self, spans: &[Span], m: &mut MeshData) {
        let (rows, r) = self.stake_layout();
        let step = r * 2.15;
        let tip_col = palette::trim(self.w.colors);
        for sp in spans {
            let bands = sp.hole.bands();
            if bands.is_empty() {
                continue;
            }
            let n = ((sp.s1 - sp.s0) / step).floor().max(1.0) as usize;
            let len = sp.s1 - sp.s0;
            for (ri, &off) in rows.iter().enumerate() {
                for i in 0..n {
                    let shift = if ri == 1 { 0.5 } else { 0.0 };
                    let s = (sp.s0 + len * (i as f32 + 0.5 + shift) / n as f32).min(sp.s1 - r * 0.5);
                    if s - sp.s0 < r * 0.5 && n > 1 {
                        continue;
                    }
                    let sm = self.smp(s);
                    let nn = nlv(sp.d);
                    let c = sm.p + nn * off * self.half;
                    let mut rr = self.rng(91 + ri as u64, c);
                    let var = rr.range(0.85, 1.1);
                    let rot = rr.range(0.0, TAU);
                    let col = self.wood_col(&mut rr);
                    let ragged = self.w.tumbled && sm.ht < sm.full * 0.9;
                    if ragged && rr.f() < 0.45 {
                        // Fallen stake lying at the foot.
                        let ang = rr.range(-0.6, 0.6);
                        let side = if rr.f() < 0.5 { 1.0 } else { -1.0 };
                        let dir = (nn * side * ang.cos() + sp.d * ang.sin()).normalize_or(nn);
                        let q0 = c + nn * side * (self.half + 0.2);
                        let q1 = q0 + dir * (sm.full * rr.range(0.5, 0.8)).min(3.0);
                        let (g0, g1) = (ground(self.terr, q0), ground(self.terr, q1));
                        if !self.terr.is_sea(q1) {
                            m.beam(v3(q0, g0), v3(q1, g1), r * 1.6, r * 1.4, col, Surface::Timber, UV);
                        }
                        continue;
                    }
                    let topv = if ragged { sm.ht * rr.range(0.6, 1.0) } else { sm.full * var };
                    for &band in &bands {
                        let lo = band.0.map_or(-0.12, |y| y - sm.base);
                        let hi = band.1.map_or(topv, |y| (y - sm.base).min(topv));
                        if hi - lo < 0.1 {
                            continue;
                        }
                        let pointed = band.1.is_none() && !ragged;
                        post(m, v3(c, sm.base + lo), v3(c, sm.base + hi), r, r * 0.92, 5, rot, !pointed, col, Surface::Timber);
                        if pointed {
                            let (tc, ts) = if self.w.crest { (tip_col, Surface::Carving) } else { (col, Surface::Timber) };
                            let apex = v3(c + Vec2::new(rr.range(-0.02, 0.02), rr.range(-0.02, 0.02)), sm.base + hi + r * 2.6);
                            cone(m, v3(c, sm.base + hi), apex, r * 0.92, 5, rot, tc, ts);
                        }
                    }
                }
            }
        }
    }

    /// Lashed rope rail (both faces) and a driftwood rail (left face) across the stakes.
    fn rails(&self, spans: &[Span], m: &mut MeshData) {
        let (rows, r) = self.stake_layout();
        let reach = rows.iter().fold(0.0_f32, |a, &o| a.max(o.abs())) * self.half + r + 0.03;
        let rope = palette::rope();
        for sp in spans {
            let bands = sp.hole.bands();
            if bands.is_empty() {
                continue;
            }
            let st = stations(sp.s0, sp.s1, STATION);
            for w in st.windows(2) {
                let (a, b) = (self.smp(w[0]), self.smp(w[1]));
                let (na, nb) = (self.line.normal_at(w[0], sp.d), self.line.normal_at(w[1], sp.d));
                let ok = |s: &Smp, y: f32| y < s.ht - 0.05 && bands.iter().any(|&bd| {
                    let (lo, hi) = self.band_at(bd, s, None);
                    y > lo + 0.05 && y < hi - 0.05
                });
                for (frac, sides, timber) in [(0.33, &[1.0][..], true), (0.72, &[1.0, -1.0][..], false)] {
                    let (ya, yb) = (a.full * frac, b.full * frac);
                    if !(ok(&a, ya) && ok(&b, yb)) {
                        continue;
                    }
                    for &side in sides {
                        let (q0, q1) = (a.p + na * side * reach, b.p + nb * side * reach);
                        if timber {
                            let mut rr = self.rng(101, q0);
                            let col = self.wood_col(&mut rr);
                            m.beam(v3(q0, a.base + ya - 0.06), v3(q1, b.base + yb - 0.06), 0.1, 0.12, col, Surface::Timber, UV);
                        } else {
                            m.beam(v3(q0, a.base + ya - 0.03), v3(q1, b.base + yb - 0.03), 0.05, 0.06, rope, Surface::Fiber, UV);
                        }
                    }
                }
            }
        }
    }

    /// Boulder row along both feet.
    fn footing(&self, spans: &[Span], footing: f32, m: &mut MeshData) {
        let rad = (footing * 0.75).clamp(0.2, 0.75);
        for sp in spans {
            if !sp.hole.bands().first().is_some_and(|b| b.0.is_none()) {
                continue;
            }
            let n = ((sp.s1 - sp.s0) / (rad * 1.5)).ceil().max(1.0) as usize;
            for i in 0..n {
                let s = sp.s0 + (sp.s1 - sp.s0) * (i as f32 + 0.5) / n as f32;
                let (p, _) = self.line.at(s);
                for side in [1.0_f32, -1.0] {
                    let mut r = self.rng(111, p + Vec2::splat(side * 0.37));
                    let c = p + nlv(sp.d) * side * (self.half_at(0.0) + rad * 0.45) + sp.d * r.range(-0.1, 0.1);
                    if self.terr.is_sea(c) {
                        continue;
                    }
                    let rr = rad * r.range(0.8, 1.15);
                    let col = weather_stone(jit(palette::rock(), &mut r, 0.08), self.weather, &mut r);
                    boulder(m, v3(c, ground(self.terr, c) + rr * 0.25), rr, 0.75, 5, &mut r, col, Surface::Rock);
                }
            }
        }
    }

    /// Standing-stone gatepost occupying `[g.a, g.b]`.
    fn gatepost(&self, g: &Gate, m: &mut MeshData, mk: &mut Vec<Marker>) {
        let mid = (g.a + g.b) * 0.5;
        let (p, d) = self.line.at(mid);
        let n = nlv(d);
        let gy = ground(self.terr, p);
        let mut r = self.rng(121, p);
        let ha = (g.b - g.a) * 0.5;
        let hw = self.half + 0.08;
        let bot = gy - SINK - 0.05;
        let foot = [p - d * ha + n * hw, p - d * ha - n * hw, p + d * ha - n * hw, p + d * ha + n * hw];
        let bt = foot.map(|q| v3(q, bot));
        let tp = [0, 1, 2, 3].map(|i| {
            let q = p + (foot[i] - p) * 0.88;
            let jy = if g.lintel { 0.0 } else { r.range(-0.12, 0.12) };
            v3(q, g.top + jy)
        });
        let col = self.stone_col(&mut r);
        hexa(m, bt, tp, F_SIDES | F_START | F_END, col, Surface::Rock, STONE_UV);
        if g.lintel {
            mk.push(Marker { kind: MarkerKind::Lantern, pos: v3(p + n * (hw + 0.15), gy + 1.7), dir: v3(n, 0.0) });
        } else {
            mk.push(Marker { kind: MarkerKind::Perch, pos: v3(p, g.top + 0.12), dir: Vec3::Y });
        }
    }

    /// Straight lintel (stone or timber) across `[l.a, l.b]`.
    fn lintel(&self, l: &Lintel, m: &mut MeshData) {
        let (pa, _) = self.line.at(l.a);
        let (pb, _) = self.line.at(l.b);
        let d = (pb - pa).normalize_or(Vec2::X);
        let n = nlv(d);
        let hw = self.half + if l.timber { 0.02 } else { 0.06 };
        let mut r = self.rng(131, (pa + pb) * 0.5);
        let (col, s) = if l.timber {
            (weather_wood(jit(palette::wood(), &mut r, 0.05), self.weather, &mut r), Surface::Timber)
        } else {
            (self.stone_col(&mut r), Surface::DryStone)
        };
        let foot = [pa + n * hw, pa - n * hw, pb - n * hw, pb + n * hw];
        hexa(m, foot.map(|q| v3(q, l.y)), foot.map(|q| v3(q, l.y + l.th)), F_ALL, col, s, STONE_UV);
    }

    /// Spilled stones (and fallen stakes) at the foot of collapsed sections.
    fn spill(&self, sea: &[(f32, f32)], m: &mut MeshData) {
        let total = self.line.total();
        for &(c, hw, depth) in &self.collapses {
            let n = 3 + (depth * hw * 4.0) as usize;
            let pc = self.line.at(c).0;
            for i in 0..n {
                let mut r = self.rng(141 + i as u64, pc);
                let s = (c + r.range(-1.1, 1.1) * hw).clamp(0.0, total);
                if sea.iter().any(|&(a, b)| s >= a && s <= b) {
                    continue;
                }
                let (p, d) = self.line.at(s);
                let side = if r.f() < 0.5 { 1.0 } else { -1.0 };
                let q = p + nlv(d) * side * (self.half_at(0.0) + r.range(0.1, 0.9));
                if self.terr.is_sea(q) {
                    continue;
                }
                let rad = r.range(0.14, 0.3);
                let col = self.stone_col(&mut r);
                boulder(m, v3(q, ground(self.terr, q) + rad * 0.3), rad, 0.6, 4, &mut r, col, Surface::Rock);
            }
        }
    }

    /// Breakwater over `[a, b]`: a wide rubble mole of big boulders with a flat walkable top.
    #[allow(clippy::too_many_lines)]
    fn breakwater(&self, a: f32, b: f32, m: &mut MeshData, mk: &mut Vec<Marker>) {
        let total = self.line.total();
        if b - a < 1e-3 {
            return;
        }
        let deck = SEA_LEVEL + (self.hgt * 0.5).clamp(0.9, 2.0);
        let wd = (self.half * 2.0).clamp(1.2, 3.0);
        const SLOPE: f32 = 0.8;
        let mut keys = stations(a, b, 1.25);
        keys.extend(self.line.cum.iter().copied().filter(|&c| c > a + 0.05 && c < b - 0.05));
        keys.sort_by(f32::total_cmp);
        keys.dedup_by(|x, y| (*x - *y).abs() < 0.05);
        let whole_ring = self.line.closed && a <= 1e-3 && b >= total - 1e-3;
        let rock = palette::rock();
        let nk = keys.len();
        for (i, w) in keys.windows(2).enumerate() {
            let mid = (w[0] + w[1]) * 0.5;
            let d = self.line.at(mid).1;
            let pt = |s: f32| {
                let (p, _) = self.line.at(s);
                let g = ground(self.terr, p).min(deck - 0.4);
                (p, self.line.normal_at(s, d), g)
            };
            let (p0, n0, g0) = pt(w[0]);
            let (p1, n1, g1) = pt(w[1]);
            let (wb0, wb1) = (wd + (deck - g0) * SLOPE, wd + (deck - g1) * SLOPE);
            let bt = [v3(p0 + n0 * wb0, g0), v3(p0 - n0 * wb0, g0), v3(p1 - n1 * wb1, g1), v3(p1 + n1 * wb1, g1)];
            let tp = [v3(p0 + n0 * wd, deck), v3(p0 - n0 * wd, deck), v3(p1 - n1 * wd, deck), v3(p1 + n1 * wd, deck)];
            let mut r = self.rng(151, (p0 + p1) * 0.5);
            let ends = (if i == 0 && !whole_ring { F_START } else { 0 }) | (if i + 2 == nk && !whole_ring { F_END } else { 0 });
            let flank = weather_stone(jit(rock, &mut r, 0.06), self.weather, &mut r);
            hexa(m, bt, tp, F_LEFT | F_RIGHT | ends, flank, Surface::Rock, STONE_UV);
            let top = self.stone_col(&mut r);
            hexa(m, bt, tp, F_TOP, top, Surface::DryStone, STONE_UV);
            // Big boulders along the waterline, smaller ones along the deck edge.
            let len = p0.distance(p1);
            let k = ((len / 0.9).ceil() as usize).max(1);
            for j in 0..k {
                let t = (j as f32 + 0.5) / k as f32;
                let p = p0.lerp(p1, t);
                let n = n0.lerp(n1, t).normalize_or(nlv(d));
                for side in [1.0_f32, -1.0] {
                    let mut rr = self.rng(161, p + n * side);
                    let y = SEA_LEVEL + rr.range(-0.2, 0.25);
                    let lat = wd + (deck - y) * SLOPE;
                    let rad = rr.range(0.45, 0.8);
                    let col = weather_stone(jit(rock, &mut rr, 0.08), self.weather, &mut rr);
                    let c = p + n * side * lat + d * rr.range(-0.15, 0.15);
                    boulder(m, v3(c, y), rad, 0.75, 5, &mut rr, col, Surface::Rock);
                    if j % 2 == 0 {
                        let r2 = rr.range(0.3, 0.5);
                        let c2 = p + n * side * (wd + 0.05);
                        let col2 = weather_stone(jit(rock, &mut rr, 0.08), self.weather, &mut rr);
                        boulder(m, v3(c2, deck - 0.1), r2, 0.7, 5, &mut rr, col2, Surface::Rock);
                    }
                }
            }
        }
        // Rounded head where the mole ends in open water, with a lantern.
        for (s, out) in [(a, -1.0_f32), (b, 1.0)] {
            let at_end = if out < 0.0 { s <= 1e-3 } else { s >= total - 1e-3 };
            if !at_end || self.line.closed {
                continue;
            }
            let (p, d) = self.line.at(s);
            let fwd = d * out;
            let mut r = self.rng(171, p);
            for i in 0..5 {
                let ang = (i as f32 / 4.0 - 0.5) * PI * 0.9;
                let dir = fwd * ang.cos() + nlv(d) * ang.sin();
                let rad = r.range(0.5, 0.85);
                let c = p + dir * (wd + 0.4);
                let col = weather_stone(jit(rock, &mut r, 0.08), self.weather, &mut r);
                boulder(m, v3(c, SEA_LEVEL + r.range(-0.1, 0.3)), rad, 0.75, 5, &mut r, col, Surface::Rock);
            }
            mk.push(Marker { kind: MarkerKind::Lantern, pos: v3(p, deck + 0.4), dir: v3(fwd, 0.0) });
        }
        for s in stations(a, b, 4.0) {
            let (p, _) = self.line.at(s);
            mk.push(Marker { kind: MarkerKind::Perch, pos: v3(p, deck), dir: Vec3::Y });
        }
    }

    /// Perches along the crest every ~3 m.
    fn perches(&self, spans: &[Span], mk: &mut Vec<Marker>) {
        let crest = match (self.w.kind, self.w.crest) {
            (WallKind::DryStone, true) => 0.34,
            (WallKind::Palisade, _) => self.stake_layout().1 * 2.6,
            _ => 0.0,
        };
        for sp in spans {
            if sp.hole != Hole::Solid {
                continue;
            }
            let mut s = (sp.s0 / 3.0).floor() * 3.0 + 1.5;
            while s < sp.s1 {
                if s > sp.s0 {
                    let sm = self.smp(s);
                    mk.push(Marker { kind: MarkerKind::Perch, pos: v3(sm.p, sm.base + sm.ht + crest), dir: Vec3::Y });
                }
                s += 3.0;
            }
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Windbreak
// ---------------------------------------------------------------------------------------------

/// Windbreak mesh plus perch markers on some post tops.
pub fn fence_mesh(fence: &Fence, cuts: &[Cut], terrain: &Terrain, seed: u64) -> (MeshData, Vec<Marker>) {
    let mut m = MeshData::default();
    let mut mk = Vec::new();
    build_fence(fence, cuts, terrain, seed, &mut m, &mut mk);
    if m.is_empty() || !m.is_valid() || mk.iter().any(|k| !k.pos.is_finite()) {
        return (MeshData::default(), Vec::new());
    }
    (m, mk)
}

/// A planted post: base and (leaning) top.
#[derive(Clone, Copy)]
struct Post {
    base: Vec3,
    top: Vec3,
}

impl Post {
    /// Point on the post axis at `frac` of its height above the ground (`g`).
    fn at(&self, g: f32, frac: f32) -> Vec3 {
        let y = g + (self.top.y - g) * frac;
        let t = ((y - self.base.y) / (self.top.y - self.base.y).max(1e-3)).clamp(0.0, 1.0);
        self.base.lerp(self.top, t)
    }
}

fn build_fence(f: &Fence, cuts: &[Cut], terr: &Terrain, seed: u64, m: &mut MeshData, mk: &mut Vec<Marker>) {
    let Some(line) = Line::new(&f.points) else { return };
    let total = line.total();
    let (bay, pr, ph, lean) = match f.kind {
        FenceKind::Wattle => (1.5, 0.06, 1.25, 0.02),
        FenceKind::Driftwood => (2.0, 0.08, 1.25, 0.09),
        FenceKind::NetLine => (2.2, 0.08, 1.8, 0.04),
        FenceKind::Reed => (2.0, 0.06, 1.7, 0.02),
    };
    // Gaps where tracks cross, merged.
    let mut gaps: Vec<(f32, f32)> = cuts
        .iter()
        .filter(|c| c.along.is_finite() && c.width.is_finite())
        .map(|c| {
            let w = c.width.clamp(0.6, 8.0);
            (c.along - w * 0.5, c.along + w * 0.5)
        })
        .collect();
    gaps.sort_by(|x, y| x.0.total_cmp(&y.0));
    let mut runs = Vec::new();
    let mut s = 0.0_f32;
    for (a, b) in gaps {
        if a > s {
            runs.push((s, a.min(total), true));
        }
        s = s.max(b);
    }
    if s < total {
        runs.push((s, total, false));
    }
    let inset = pr + lean + 0.08;
    let mut post_i = 0usize;
    for (r0, r1, gap_after) in runs {
        let a = if r0 > 0.0 { r0 + inset } else { r0 };
        let b = if gap_after { r1 - inset } else { r1 };
        if b - a < 0.3 {
            continue;
        }
        let mut keys = vec![a];
        keys.extend(line.cum.iter().copied().filter(|&c| c > a + 0.3 && c < b - 0.3));
        keys.push(b);
        let mut ss = Vec::new();
        for w in keys.windows(2) {
            let st = stations(w[0], w[1], bay);
            ss.extend_from_slice(&st[..st.len() - 1]);
        }
        ss.push(b);
        // Posts.
        let posts: Vec<(Post, f32)> = ss
            .iter()
            .map(|&s| {
                let (p, _) = line.at(s);
                let g = ground(terr, p);
                let mut r = Rng::at(seed ^ 0xF3_4CE5, p.x, p.y);
                let off = Vec2::new(r.range(-1.0, 1.0), r.range(-1.0, 1.0)).clamp_length_max(1.0) * lean;
                (Post { base: v3(p, g - 0.1), top: v3(p + off, g + ph * r.range(0.92, 1.08)) }, g)
            })
            .collect();
        for &(pt, _) in &posts {
            let mut r = Rng::at(seed ^ 0x9057, pt.base.x, pt.base.z);
            let col = match f.kind {
                FenceKind::Driftwood => jit(palette::driftwood().lerp(palette::salt_bleach(), r.range(0.0, 0.35)), &mut r, 0.05),
                _ => jit(palette::wood(), &mut r, 0.06),
            };
            post(m, pt.base, pt.top, pr, pr * 0.85, 5, r.range(0.0, TAU), true, col, Surface::Timber);
            if post_i % 3 == 0 {
                mk.push(Marker { kind: MarkerKind::Perch, pos: pt.top, dir: Vec3::Y });
            }
            post_i += 1;
        }
        // Infill per bay.
        for (bi, w) in posts.windows(2).enumerate() {
            let ((p0, g0), (p1, g1)) = (w[0], w[1]);
            let (q0, q1) = (Vec2::new(p0.base.x, p0.base.z), Vec2::new(p1.base.x, p1.base.z));
            if q0.distance(q1) < 0.1 {
                continue;
            }
            let d = (q1 - q0).normalize_or(Vec2::X);
            let n = nlv(d);
            let mut r = Rng::at(seed ^ 0xBA7, (q0.x + q1.x) * 0.5, (q0.y + q1.y) * 0.5);
            match f.kind {
                FenceKind::Wattle => wattle(m, (q0, g0), (q1, g1), n, bi, &mut r),
                FenceKind::Driftwood => {
                    for frac in [0.42, 0.86] {
                        let (ra, rb) = (p0.at(g0, frac), p1.at(g1, frac));
                        let kink = (ra + rb) * 0.5 + v3(n * r.range(-0.07, 0.07), r.range(-0.06, 0.04));
                        let col = jit(palette::driftwood().lerp(palette::salt_bleach(), r.range(0.0, 0.35)), &mut r, 0.05);
                        m.beam(ra - Vec3::Y * 0.04, kink - Vec3::Y * 0.04, 0.09, 0.08, col, Surface::Timber, UV);
                        m.beam(kink - Vec3::Y * 0.04, rb - Vec3::Y * 0.04, 0.09, 0.08, col, Surface::Timber, UV);
                    }
                }
                FenceKind::NetLine => net(m, (p0, g0), (p1, g1), f.colors, bi, &mut r),
                FenceKind::Reed => reed(m, terr, (q0, g0), (q1, g1), n, ph, &mut r),
            }
        }
    }
}

/// Woven wattle panel: strips that pass alternately in front of and behind a middle sail rod.
fn wattle(m: &mut MeshData, (q0, g0): (Vec2, f32), (q1, g1): (Vec2, f32), n: Vec2, bay: usize, r: &mut Rng) {
    const STRIPS: usize = 6;
    let qm = (q0 + q1) * 0.5;
    let gm = (g0 + g1) * 0.5;
    post(m, v3(qm, gm - 0.08), v3(qm, gm + 1.12), 0.035, 0.03, 4, r.range(0.0, TAU), true, jit(palette::wood(), r, 0.06), Surface::Timber);
    let th = 0.045;
    for j in 0..STRIPS {
        let (y0, y1) = (0.12 + j as f32 * 0.17, 0.12 + j as f32 * 0.17 + 0.15);
        let col = jit(if (j + bay) % 2 == 0 { palette::hay() } else { palette::fiber_dark() }, r, 0.05);
        for (half, (a, ga, b, gb)) in [(q0, g0, qm, gm), (qm, gm, q1, g1)].into_iter().enumerate() {
            let off = if (j + bay + half) % 2 == 0 { 0.035 } else { -0.035 };
            let (la, ra) = (a + n * (off + th), a + n * (off - th));
            let (lb, rb) = (b + n * (off + th), b + n * (off - th));
            let bt = [v3(la, ga + y0), v3(ra, ga + y0), v3(rb, gb + y0), v3(lb, gb + y0)];
            let tp = [v3(la, ga + y1), v3(ra, ga + y1), v3(rb, gb + y1), v3(lb, gb + y1)];
            hexa(m, bt, tp, F_SIDES, col, Surface::Fiber, 1.0);
        }
    }
}

/// Sagging fishing net between two posts, a top rope and bright float beads.
fn net(m: &mut MeshData, (p0, g0): (Post, f32), (p1, g1): (Post, f32), colors: Colors, bay: usize, r: &mut Rng) {
    const K: usize = 4;
    let (ta, tb) = (p0.at(g0, 0.95), p1.at(g1, 0.95));
    let sag = r.range(0.14, 0.24);
    let top = |i: usize| {
        let t = i as f32 / K as f32;
        ta.lerp(tb, t) - Vec3::Y * (sag * 4.0 * t * (1.0 - t))
    };
    let bot = |i: usize| {
        let t = i as f32 / K as f32;
        let b = Vec3::new(ta.x + (tb.x - ta.x) * t, g0 + (g1 - g0) * t + 0.38, ta.z + (tb.z - ta.z) * t);
        b - Vec3::Y * (0.12 * 4.0 * t * (1.0 - t))
    };
    let rope = palette::rope();
    for i in 0..K {
        let (a, b, c, d) = (bot(i), bot(i + 1), top(i + 1), top(i));
        let from = m.vertex_count();
        m.quad(a, b, c, d, rope, Surface::Fiber, 1.0);
        m.quad(b, a, d, c, rope, Surface::Fiber, 1.0);
        // Hangs from the top rope: the loose foot sways a little.
        m.flutter(from, |v| if v.distance(a) < 1e-4 || v.distance(b) < 1e-4 { 0.3 } else { 0.0 });
        m.beam(top(i) - Vec3::Y * 0.02, top(i + 1) - Vec3::Y * 0.02, 0.04, 0.04, palette::fiber_dark(), Surface::Fiber, UV);
    }
    for i in 1..K {
        let col = if (i + bay) % 2 == 0 { palette::trim(colors) } else { palette::pigment(colors.trim as u32 + 3) };
        bead(m, top(i) - Vec3::Y * 0.06, 0.07, col, Surface::Carving);
    }
}

/// Bundled reed screen with binding bands on both faces.
fn reed(m: &mut MeshData, terr: &Terrain, (q0, g0): (Vec2, f32), (q1, g1): (Vec2, f32), n: Vec2, ph: f32, r: &mut Rng) {
    let len = q0.distance(q1);
    let k = ((len / 0.24).round() as usize).max(1);
    for i in 0..k {
        let q = q0.lerp(q1, (i as f32 + 0.5) / k as f32);
        let g = ground(terr, q);
        let h = ph * r.range(0.82, 0.97);
        let rot = r.range(0.0, TAU);
        let col = jit(if r.f() < 0.6 { palette::hay() } else { palette::grass_dry() }, r, 0.05);
        let top = v3(q, g + h);
        post(m, v3(q, g - 0.05), top, 0.1, 0.075, 4, rot, false, col, Surface::Fiber);
        cone(m, top, top + Vec3::new(r.range(-0.03, 0.03), r.range(0.12, 0.2), r.range(-0.03, 0.03)), 0.075, 4, rot, col, Surface::Fiber);
    }
    for y in [0.45, 1.1] {
        for side in [1.0_f32, -1.0] {
            let (a, b) = (q0 + n * side * 0.11, q1 + n * side * 0.11);
            m.beam(v3(a, g0 + y), v3(b, g1 + y), 0.04, 0.07, palette::fiber_dark(), Surface::Fiber, UV);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Colors, Opening};
    use crate::terrain::FlattenRegion;

    fn wall(points: Vec<Vec2>) -> Wall {
        Wall {
            points,
            height: 3.0,
            thickness: 0.8,
            kind: WallKind::DryStone,
            crest: false,
            tumbled: false,
            footing: 0.0,
            roofed: false,
            roof: Default::default(),
            roof_pitch: 0.5,
            openings: vec![],
            colors: Colors::default(),
            weathering: 0.0,
        }
    }
    fn square(poly: [(f32, f32); 4], height: f32) -> FlattenRegion {
        FlattenRegion { poly: poly.iter().map(|&(x, z)| Vec2::new(x, z)).collect(), height: Some(height), margin: 1.0, keep: false }
    }
    /// Flat land at 1 m above the sea.
    fn flat_terrain() -> Terrain {
        Terrain::build(1, &[], &[square([(-30.0, -30.0), (30.0, -30.0), (30.0, 30.0), (-30.0, 30.0)], 1.0)])
    }
    /// Land at 1 m for x < 0, sea 3 m deep for x > 2.
    fn coast() -> Terrain {
        Terrain::build(
            1,
            &[],
            &[square([(-30.0, -30.0), (0.0, -30.0), (0.0, 30.0), (-30.0, 30.0)], 1.0), square([(2.0, -30.0), (30.0, -30.0), (30.0, 30.0), (2.0, 30.0)], -3.0)],
        )
    }
    fn line20() -> Vec<Vec2> {
        vec![Vec2::new(-10.0, 0.0), Vec2::new(10.0, 0.0)]
    }
    fn fence(kind: FenceKind) -> Fence {
        Fence { points: line20(), kind, colors: Colors::default() }
    }
    fn tris(m: &MeshData) -> usize {
        m.indices.len() / 3
    }
    fn has(m: &MeshData, s: Surface) -> bool {
        m.surfaces.contains(&(s as u8))
    }
    fn has_col(m: &MeshData, c: Vec3) -> bool {
        m.colors.iter().any(|k| (Vec3::new(k[0], k[1], k[2]) - c).abs().max_element() < 1e-4)
    }
    /// Highest vertex y within `dx` of `x` (optionally of one surface).
    fn top_near(m: &MeshData, x: f32, dx: f32, s: Option<Surface>) -> f32 {
        m.positions
            .iter()
            .zip(&m.surfaces)
            .filter(|(p, k)| (p[0] - x).abs() < dx && s.is_none_or(|s| **k == s as u8))
            .map(|(p, _)| p[1])
            .fold(f32::MIN, f32::max)
    }
    const KINDS: [WallKind; 3] = [WallKind::DryStone, WallKind::Turf, WallKind::Palisade];
    const FENCES: [FenceKind; 4] = [FenceKind::Wattle, FenceKind::Driftwood, FenceKind::NetLine, FenceKind::Reed];

    #[test]
    fn all_combinations_valid_and_deterministic() {
        let t = flat_terrain();
        for kind in KINDS {
            for crest in [false, true] {
                for tumbled in [false, true] {
                    for footing in [0.0, 0.6] {
                        for weathering in [0.0, 1.0] {
                            for h in [1.2, 3.0, 7.0] {
                                let mut w = wall(vec![Vec2::new(-10.0, 0.0), Vec2::new(0.0, 4.0), Vec2::new(10.0, 0.0)]);
                                (w.kind, w.crest, w.tumbled, w.footing, w.weathering, w.height) = (kind, crest, tumbled, footing, weathering, h);
                                w.openings = vec![
                                    Opening { along: 4.0, kind: OpeningKind::Window },
                                    Opening { along: 16.0, kind: OpeningKind::WideDoor },
                                    Opening { along: 19.5, kind: OpeningKind::Door },
                                ];
                                let cuts = [Cut { along: 10.0, width: 2.0, height: 2.6 }];
                                let (m, mk) = wall_mesh(&w, &cuts, &t, 7);
                                let tag = format!("{kind:?} crest {crest} tumbled {tumbled} footing {footing} w {weathering} h {h}");
                                assert!(m.is_valid() && !m.is_empty(), "{tag}");
                                assert!(mk.iter().any(|k| k.kind == MarkerKind::Perch), "{tag}");
                                assert!(tris(&m) < 6000, "{tag}: {} tris", tris(&m));
                                assert_eq!((m, mk), wall_mesh(&w, &cuts, &t, 7), "{tag} deterministic");
                            }
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn seed_changes_decoration_only() {
        let t = flat_terrain();
        let mut w = wall(line20());
        w.tumbled = true;
        w.crest = true;
        assert_eq!(wall_mesh(&w, &[], &t, 9), wall_mesh(&w, &[], &t, 9));
        assert_ne!(wall_mesh(&w, &[], &t, 9).0, wall_mesh(&w, &[], &t, 10).0);
        let f = fence(FenceKind::Driftwood);
        assert_eq!(fence_mesh(&f, &[], &t, 1), fence_mesh(&f, &[], &t, 1));
        assert_ne!(fence_mesh(&f, &[], &t, 1).0, fence_mesh(&f, &[], &t, 2).0);
    }

    #[test]
    fn dry_stone_courses_batter_and_height() {
        let t = flat_terrain();
        let (m, _) = wall_mesh(&wall(line20()), &[], &t, 3);
        // Many individual stones: lilac slate among the buff, joint core behind.
        assert!(tris(&m) > 400, "{}", tris(&m));
        assert!(has_col(&m, palette::stone_joint()));
        assert!(m.colors.iter().any(|k| k[2] > k[0]), "lilac slate stones");
        // Honours height: top at ground (1.0) + 3.0.
        let top = m.positions.iter().map(|p| p[1]).fold(f32::MIN, f32::max);
        assert!((top - 4.0).abs() < 0.05, "top {top}");
        // Honours thickness, with a batter: narrower at the top than at the foot.
        let width = |lo: f32, hi: f32| m.positions.iter().filter(|p| p[1] >= lo && p[1] <= hi).map(|p| p[2].abs()).fold(0.0, f32::max);
        let (foot, crest) = (width(0.9, 1.3), width(3.6, 4.0));
        assert!(foot > 0.39 && foot < 0.5, "foot {foot}");
        assert!(crest < foot - 0.1, "batter {crest} vs {foot}");
    }

    #[test]
    fn crest_per_kind() {
        let t = flat_terrain();
        let mut w = wall(line20());
        // Dry stone: upright copestones above the top course.
        let plain = wall_mesh(&w, &[], &t, 3).0;
        w.crest = true;
        let cope = wall_mesh(&w, &[], &t, 3).0;
        assert!(top_near(&cope, 0.0, 10.0, None) > top_near(&plain, 0.0, 10.0, None) + 0.2);
        // Turf: magenta thrift tufts.
        w.kind = WallKind::Turf;
        w.crest = false;
        assert!(!has(&wall_mesh(&w, &[], &t, 3).0, Surface::Petal));
        w.crest = true;
        let thrift = wall_mesh(&w, &[], &t, 3).0;
        assert!(has(&thrift, Surface::Petal) && (has_col(&thrift, palette::flower(0)) || has_col(&thrift, palette::flower(1))));
        // Palisade: stake tips painted in the trim colour.
        w.kind = WallKind::Palisade;
        w.crest = false;
        assert!(!has(&wall_mesh(&w, &[], &t, 3).0, Surface::Carving));
        w.crest = true;
        w.colors.trim = 2;
        let tips = wall_mesh(&w, &[], &t, 3).0;
        assert!(has(&tips, Surface::Carving) && has_col(&tips, palette::trim(w.colors)));
    }

    #[test]
    fn turf_has_stone_base_turf_bands_and_grassy_top() {
        let t = flat_terrain();
        let mut w = wall(line20());
        w.kind = WallKind::Turf;
        let (m, _) = wall_mesh(&w, &[], &t, 4);
        assert!(has(&m, Surface::DryStone) && has(&m, Surface::Turf) && has(&m, Surface::Grass));
        let near = |c: Vec3| m.colors.iter().any(|k| (Vec3::new(k[0], k[1], k[2]) - c).abs().max_element() < 0.07 * c.max_element());
        assert!(near(palette::sod()) && near(palette::turf()));
        let stone_top = top_near(&m, 0.0, 10.0, Some(Surface::DryStone));
        assert!(stone_top < 1.0 + 1.0, "stone base only at the foot: {stone_top}");
        let grass_top = top_near(&m, 0.0, 10.0, Some(Surface::Grass));
        assert!((grass_top - 4.0).abs() < 0.05, "grassy cap on top: {grass_top}");
    }

    #[test]
    fn palisade_stakes_vary_and_have_rails() {
        let t = flat_terrain();
        let mut w = wall(line20());
        w.kind = WallKind::Palisade;
        let (m, _) = wall_mesh(&w, &[], &t, 4);
        assert!(has(&m, Surface::Timber) && has(&m, Surface::Fiber), "stakes and rope rail");
        assert!(has_col(&m, palette::rope()));
        // Varied stake heights.
        let tops: Vec<f32> = (-9..=9).map(|x| top_near(&m, x as f32, 0.2, None)).collect();
        let (lo, hi) = tops.iter().fold((f32::MAX, f32::MIN), |(l, h), &y| (l.min(y), h.max(y)));
        assert!(hi - lo > 0.2, "{lo}..{hi}");
        assert!(lo > 1.0 + 3.0 * 0.8, "{lo}");
    }

    #[test]
    fn tumbled_collapses_and_spills() {
        let t = flat_terrain();
        for kind in KINDS {
            let mut w = wall(line20());
            w.kind = kind;
            let whole = wall_mesh(&w, &[], &t, 5).0;
            w.tumbled = true;
            let (m, _) = wall_mesh(&w, &[], &t, 5);
            assert!(m.is_valid());
            // Somewhere along the dyke the crest dips well below full height.
            let crest = |m: &MeshData| {
                (-95..=95)
                    .filter_map(|i| {
                        let x = i as f32 * 0.1;
                        let ys = m.positions.iter().filter(|p| (p[0] - x).abs() < 0.55 && p[2].abs() < 0.6).map(|p| p[1]);
                        ys.reduce(f32::max)
                    })
                    .fold(f32::MAX, f32::min)
            };
            assert!(crest(&whole) > 3.9, "{kind:?} intact crest {}", crest(&whole));
            assert!(crest(&m) < 1.0 + 3.0 * 0.85, "{kind:?} lowest crest {}", crest(&m));
            // Spilled stones / fallen stakes outside the footprint at the foot.
            let outside = |m: &MeshData| m.positions.iter().filter(|p| p[2].abs() > 0.75 && p[1] < 1.6).count();
            assert!(outside(&m) > outside(&whole), "{kind:?} spill");
        }
    }

    #[test]
    fn footing_is_a_boulder_row() {
        let t = flat_terrain();
        let mut w = wall(line20());
        let bare = wall_mesh(&w, &[], &t, 6).0;
        assert!(!has(&bare, Surface::Rock));
        w.footing = 0.5;
        let (m, _) = wall_mesh(&w, &[], &t, 6);
        let rocks: Vec<&[f32; 3]> = m.positions.iter().zip(&m.surfaces).filter(|(_, s)| **s == Surface::Rock as u8).map(|(p, _)| p).collect();
        assert!(rocks.len() > 100);
        assert!(rocks.iter().all(|p| p[1] < 1.0 + 0.8), "boulders stay at the foot");
        assert!(rocks.iter().any(|p| p[2] > 0.5) && rocks.iter().any(|p| p[2] < -0.5), "both sides");
    }

    #[test]
    fn weathering_bleaches_and_grows_lichen() {
        let t = flat_terrain();
        let lichen = |m: &MeshData| {
            m.colors.iter().filter(|k| {
                let c = Vec3::new(k[0], k[1], k[2]);
                c.distance(palette::lichen()) < 0.25 || c.distance(palette::lichen_yellow()) < 0.25
            })
            .count()
        };
        let mut w = wall(line20());
        let clean = wall_mesh(&w, &[], &t, 8).0;
        w.weathering = 1.0;
        let old = wall_mesh(&w, &[], &t, 8).0;
        assert_eq!(lichen(&clean), 0);
        assert!(lichen(&old) > 50, "{}", lichen(&old));
        // Palisade wood drifts toward salt bleach.
        let bleach = |m: &MeshData| {
            let d: f32 = m.colors.iter().zip(&m.surfaces).filter(|(_, s)| **s == Surface::Timber as u8).map(|(k, _)| Vec3::new(k[0], k[1], k[2]).distance(palette::salt_bleach())).sum();
            d / m.surfaces.iter().filter(|s| **s == Surface::Timber as u8).count().max(1) as f32
        };
        w.kind = WallKind::Palisade;
        w.weathering = 0.0;
        let fresh = wall_mesh(&w, &[], &t, 8).0;
        w.weathering = 1.0;
        let salted = wall_mesh(&w, &[], &t, 8).0;
        assert!(bleach(&salted) < bleach(&fresh));
    }

    #[test]
    fn breakwater_over_the_sea() {
        let t = coast();
        assert!(t.is_sea(Vec2::new(8.0, 0.0)) && !t.is_sea(Vec2::new(-8.0, 0.0)));
        for kind in KINDS {
            let mut w = wall(line20());
            w.kind = kind;
            let (m, mk) = wall_mesh(&w, &[], &t, 2);
            assert!(m.is_valid());
            let sea_pts: Vec<&[f32; 3]> = m.positions.iter().filter(|p| p[0] > 4.0).collect();
            // Wide rubble mole: boulders well beyond the dyke thickness, around the waterline.
            assert!(sea_pts.iter().any(|p| p[2].abs() > 2.0 && p[1].abs() < 0.6), "{kind:?} wide mole");
            // Low and flat: nothing as tall as the land wall out there.
            let sea_top = sea_pts.iter().map(|p| p[1]).fold(f32::MIN, f32::max);
            assert!(sea_top < SEA_LEVEL + 2.6, "{kind:?} {sea_top}");
            // Flat walkable deck: up-facing stone quads at one level.
            let deck = m.positions.iter().zip(&m.normals).zip(&m.surfaces).filter(|((p, n), s)| p[0] > 4.0 && n[1] > 0.99 && **s == Surface::DryStone as u8).count();
            assert!(deck >= 8, "{kind:?} deck {deck}");
            // Still a full wall on land; perches and a lantern on the mole head.
            assert!(top_near(&m, -8.0, 1.0, None) > 1.0 + 2.5, "{kind:?} land wall");
            assert!(mk.iter().any(|k| k.kind == MarkerKind::Perch && k.pos.x > 4.0));
            assert!(mk.iter().any(|k| k.kind == MarkerKind::Lantern && k.pos.x > 9.0));
        }
    }

    #[test]
    fn track_cut_gap_with_gateposts_and_lintel() {
        let t = flat_terrain();
        let cut = [Cut { along: 10.0, width: 2.0, height: 2.4 }];
        for kind in KINDS {
            let mut w = wall(line20());
            w.kind = kind;
            w.height = 4.0;
            let (m, mk) = wall_mesh(&w, &cut, &t, 5);
            assert!(m.is_valid());
            // Clear passage under the lintel.
            assert!(!m.positions.iter().any(|p| p[0].abs() < 0.99 && p[1] > 1.05 && p[1] < 1.0 + 2.35), "{kind:?} passage");
            // Lintel stone across the gap, gateposts (standing stones) on both sides.
            assert!(m.positions.iter().zip(&m.surfaces).any(|(p, s)| *s == Surface::DryStone as u8 && (p[0].abs() - 1.55).abs() < 0.02 && (p[1] - 3.4).abs() < 0.01), "{kind:?} lintel");
            assert!(m.positions.iter().any(|p| p[0].abs() < 0.9 && p[1] > 3.75), "{kind:?} dyke continues over the lintel");
            let posts = |side: f32| m.positions.iter().zip(&m.surfaces).any(|(p, s)| *s == Surface::Rock as u8 && (p[0] * side) > 1.0 && (p[0] * side) < 1.6);
            assert!(posts(1.0) && posts(-1.0), "{kind:?} gateposts");
            assert_eq!(mk.iter().filter(|k| k.kind == MarkerKind::Lantern).count(), 2);
            // Below 2.2 m: no lintel, the gateposts stand proud of the dyke.
            w.height = 1.5;
            let (low, lmk) = wall_mesh(&w, &cut, &t, 5);
            assert!(!low.positions.iter().any(|p| p[0].abs() < 0.99 && p[1] > 1.0), "{kind:?} open gap");
            assert!(top_near(&low, 1.27, 0.25, Some(Surface::Rock)) > 1.0 + 1.5 + 0.25);
            assert!(!lmk.iter().any(|k| k.kind == MarkerKind::Lantern));
        }
    }

    #[test]
    fn openings_hole_doorway_and_wide_door() {
        let t = flat_terrain();
        for kind in KINDS {
            let mut w = wall(vec![Vec2::ZERO, Vec2::new(12.0, 0.0)]);
            w.kind = kind;
            w.openings = vec![
                Opening { along: 3.0, kind: OpeningKind::Window },
                Opening { along: 6.0, kind: OpeningKind::Door },
                Opening { along: 9.0, kind: OpeningKind::WideDoor },
            ];
            let (m, mk) = wall_mesh(&w, &[], &t, 2);
            assert!(m.is_valid());
            assert_eq!(mk.iter().filter(|k| k.kind == MarkerKind::Door).count(), 2);
            // Wind hole: sill at 1.0 + 0.5 m (3 m dyke: 0.42 · 3 clamps to 1.0 → 1.0 m).
            let (sill, head) = (1.0 + 1.0, 1.0 + 1.0 + HOLE_W);
            assert!(!m.positions.iter().any(|p| (p[0] - 3.0).abs() < 0.2 && p[1] > sill + 0.03 && p[1] < head - 0.03), "{kind:?} wind hole");
            // Doorway clear to 1.9 m, wide door to 2.3 m; both lintelled on a 3 m dyke.
            assert!(!m.positions.iter().any(|p| (p[0] - 6.0).abs() < 0.5 && p[1] > 1.05 && p[1] < 1.0 + 1.85), "{kind:?} door");
            assert!(!m.positions.iter().any(|p| (p[0] - 9.0).abs() < 1.15 && p[1] > 1.05 && p[1] < 1.0 + 2.25), "{kind:?} wide door");
            let timber_lintel = m.positions.iter().zip(&m.surfaces).any(|(p, s)| *s == Surface::Timber as u8 && ((p[0] - 9.0).abs() - 1.5).abs() < 0.02 && (p[1] - 3.3).abs() < 0.01);
            assert!(timber_lintel, "{kind:?} wide door timber lintel");
        }
    }

    #[test]
    fn roofed_ring_keeps_a_flat_top_and_follows_hills_otherwise() {
        use crate::model::{TerrainDab, TerrainOp};
        let level = TerrainDab { center: Vec2::ZERO, radius: 40.0, op: TerrainOp::Flatten { height: 1.0, strength: 1.0 } };
        let hill = TerrainDab { center: Vec2::new(5.0, 0.0), radius: 8.0, op: TerrainOp::Sculpt { amount: 3.0 } };
        let t = Terrain::build(1, &[level, hill], &[]);
        let a = Vec2::new(-5.0, -5.0);
        let mut w = wall(vec![a, Vec2::new(5.0, -5.0), Vec2::new(5.0, 5.0), Vec2::new(-5.0, 5.0), a]);
        w.roofed = true;
        let (m, _) = wall_mesh(&w, &[], &t, 1);
        let want = w.points.iter().map(|&p| ground_eff(&t, p)).fold(f32::MIN, f32::max) + 3.0;
        for x in [-4.5_f32, 4.5] {
            assert!((top_near(&m, x, 0.4, None) - want).abs() < 0.05, "flat top at x {x}: {} vs {want}", top_near(&m, x, 0.4, None));
        }
        w.roofed = false;
        let (open, _) = wall_mesh(&w, &[], &t, 1);
        let (gl, gr) = (t.height(Vec2::new(-5.0, 0.0)), t.height(Vec2::new(5.0, 0.0)));
        assert!(gr > gl + 1.0);
        assert!((top_near(&open, -5.0, 0.3, None) - (gl + 3.0)).abs() < 0.3);
        assert!((top_near(&open, 5.0, 0.3, None) - (gr + 3.0)).abs() < 0.3);
    }

    /// A roofed ring straddling the shore keeps its flat-topped wall over the water (the roof
    /// rests on it all round); the same ring unroofed turns into a low breakwater out there.
    #[test]
    fn roofed_ring_over_the_sea_keeps_its_wall() {
        let t = coast();
        let a = Vec2::new(-5.0, -4.0);
        let mut w = wall(vec![a, Vec2::new(7.0, -4.0), Vec2::new(7.0, 4.0), Vec2::new(-5.0, 4.0), a]);
        w.height = 2.5;
        w.roofed = true;
        assert!(t.is_sea(Vec2::new(7.0, 0.0)));
        let want = w.points.iter().map(|&p| ground_eff(&t, p)).fold(f32::MIN, f32::max) + 2.5;
        let (m, _) = wall_mesh(&w, &[], &t, 1);
        assert!(m.is_valid());
        assert!((top_near(&m, 7.0, 0.4, None) - want).abs() < 0.05, "sea side {} vs {want}", top_near(&m, 7.0, 0.4, None));
        assert!(m.positions.iter().all(|p| p[2].abs() < 4.0 + 1.2), "no wide rubble mole");
        w.roofed = false;
        let (open, _) = wall_mesh(&w, &[], &t, 1);
        assert!(top_near(&open, 7.0, 0.4, None) < want - 0.8, "unroofed: breakwater");
    }

    #[test]
    fn triangle_budget() {
        let t = flat_terrain();
        for kind in KINDS {
            let mut w = wall(line20());
            (w.kind, w.crest, w.footing, w.tumbled) = (kind, true, 0.5, true);
            let (m, _) = wall_mesh(&w, &[], &t, 3);
            assert!(tris(&m) <= 3500, "{kind:?} {}", tris(&m));
        }
        for k in FENCES {
            let (m, _) = fence_mesh(&fence(k), &[], &t, 3);
            assert!(tris(&m) <= 2200, "{k:?} {}", tris(&m));
        }
    }

    #[test]
    fn fences_valid_with_gaps_and_kind_parts() {
        let t = flat_terrain();
        for k in FENCES {
            let (m, mk) = fence_mesh(&fence(k), &[], &t, 4);
            assert!(m.is_valid() && !m.is_empty(), "{k:?}");
            assert!(mk.iter().any(|x| x.kind == MarkerKind::Perch));
            let bent = Fence { points: vec![Vec2::new(-5.0, 0.0), Vec2::new(0.0, 3.0), Vec2::new(5.0, -1.0)], kind: k, colors: Colors::default() };
            assert!(fence_mesh(&bent, &[], &t, 4).0.is_valid());
            let (g, _) = fence_mesh(&fence(k), &[Cut { along: 10.0, width: 2.0, height: 2.0 }], &t, 4);
            assert!(g.is_valid() && !g.is_empty());
            assert!(!g.positions.iter().any(|p| p[0].abs() < 0.95), "{k:?} gap");
            assert!(g.positions.iter().any(|p| p[0].abs() < 1.4), "{k:?} end posts");
            assert!(has(&m, Surface::Timber), "{k:?} posts");
            match k {
                FenceKind::Wattle => assert!(has(&m, Surface::Fiber) && has_col(&m, palette::fiber_dark())),
                FenceKind::Driftwood => assert!(!has(&m, Surface::Fiber)),
                FenceKind::NetLine => {
                    assert!(has(&m, Surface::Fiber) && has(&m, Surface::Carving), "net + floats");
                    assert!(has_col(&m, palette::trim(Colors::default())));
                }
                FenceKind::Reed => assert!(has(&m, Surface::Fiber)),
            }
        }
    }

    #[test]
    fn wattle_strips_alternate_sides() {
        let t = flat_terrain();
        let (m, _) = fence_mesh(&fence(FenceKind::Wattle), &[], &t, 4);
        let fiber_z: Vec<f32> = m.positions.iter().zip(&m.surfaces).filter(|(_, s)| **s == Surface::Fiber as u8).map(|(p, _)| p[2]).collect();
        assert!(fiber_z.iter().any(|&z| z > 0.07) && fiber_z.iter().any(|&z| z < -0.07));
    }

    #[test]
    fn degenerate_inputs() {
        let t = flat_terrain();
        let p = Vec2::new(1.0, 1.0);
        let nan = Vec2::new(f32::NAN, 0.0);
        for pts in [vec![], vec![p], vec![p, p, p], vec![p, nan], vec![nan, nan]] {
            let (m, mk) = wall_mesh(&wall(pts.clone()), &[], &t, 1);
            assert!(m.is_empty() && mk.is_empty());
            for k in FENCES {
                let f = Fence { points: pts.clone(), kind: k, colors: Colors::default() };
                assert!(fence_mesh(&f, &[], &t, 1).0.is_empty());
            }
        }
        for kind in KINDS {
            let mut w = wall(vec![Vec2::ZERO, Vec2::ZERO, Vec2::new(3.0, 0.0), Vec2::new(3.0, 0.0), Vec2::new(3.0, 3.0)]);
            w.kind = kind;
            w.thickness = 1.0e9;
            w.crest = true;
            w.tumbled = true;
            w.footing = 50.0;
            w.weathering = f32::NAN;
            w.openings = vec![Opening { along: f32::NAN, kind: OpeningKind::Door }, Opening { along: 1.0, kind: OpeningKind::Window }];
            let cuts = [
                Cut { along: f32::NAN, width: 1.0, height: 1.0 },
                Cut { along: 2.0, width: 1.0e9, height: -3.0 },
                Cut { along: -5.0, width: 0.0, height: 2.0 },
            ];
            let (m, mk) = wall_mesh(&w, &cuts, &t, 1);
            assert!(m.is_valid() && mk.iter().all(|k| k.pos.is_finite()));
            w.height = f32::NAN;
            assert!(wall_mesh(&w, &[], &t, 1).0.is_empty());
            w.height = 1.0e9;
            assert!(wall_mesh(&w, &[], &t, 1).0.is_valid());
            let (f, _) = fence_mesh(&fence(FenceKind::Wattle), &cuts, &t, 1);
            assert!(f.is_valid());
        }
    }
}
