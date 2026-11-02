//! Walls and fences, eco-brutalist.
//!
//! A wall is cut into "stations" along its centre polyline (vertices with mitred offsets, regular
//! samples, opening subdivisions, window edges). Pieces between stations are extruded slabs whose
//! underside (portals over cuts, arches over water), recesses (slot windows) and top (ruins) are
//! per-piece. On top sits a thick concrete coping; planters, rebar, buttress fins and overgrowth
//! (ivy, moss, water stains) are added as separate chunky low-poly bits.

use crate::geom::is_closed;
use crate::mesh::{Marker, MarkerKind, MeshData, Rng, Surface, hash_f};
use crate::model::{Fence, FenceKind, OpeningKind, Wall, WallKind};
use crate::palette;
use crate::terrain::Terrain;
use glam::{Vec2, Vec3};

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

const UV: f32 = 0.5;
/// Slot window width (matches the block windows in buildings.rs and `OpeningKind::Window`).
const WINDOW_W: f32 = 0.8;
/// Minimum thickness for a walkable wall top.
const WALK_MIN_TH: f32 = 0.8;
/// Walkway parapet upstand height / width.
const UPSTAND_H: f32 = 0.4;
const UPSTAND_W: f32 = 0.16;
/// Rusty rebar tint.
const RUST: Vec3 = Vec3::new(0.30, 0.12, 0.05);

// ---------------------------------------------------------------------------------------------
// Small helpers
// ---------------------------------------------------------------------------------------------

fn v3(p: Vec2, y: f32) -> Vec3 {
    Vec3::new(p.x, y, p.y)
}

/// Left normal of a ground direction (matches `dir × Y` in world space).
fn nlv(d: Vec2) -> Vec2 {
    Vec2::new(-d.y, d.x)
}

fn ground(t: &Terrain, p: Vec2) -> f32 {
    let h = t.height(p);
    if h.is_finite() { h } else { 0.0 }
}

fn wlevel(t: &Terrain, p: Vec2) -> Option<f32> {
    t.water_level(p).filter(|w| w.is_finite())
}

/// Vertical face from `p0` to `p1`; `left` faces the left normal side.
#[allow(clippy::too_many_arguments)]
fn side(m: &mut MeshData, p0: Vec2, p1: Vec2, lo0: f32, lo1: f32, hi0: f32, hi1: f32, left: bool, col: Vec3, s: Surface, uv: f32) {
    let (hi0, hi1) = (hi0.max(lo0), hi1.max(lo1));
    if hi0 - lo0 < 1e-3 && hi1 - lo1 < 1e-3 {
        return;
    }
    let (a, b, c, d) = (v3(p0, lo0), v3(p1, lo1), v3(p1, hi1), v3(p0, hi0));
    if left {
        m.quad(a, b, c, d, col, s, uv);
    } else {
        m.quad(b, a, d, c, col, s, uv);
    }
}

/// End face across the wall at a station. `l`/`r` are the left/right points; `f` > 0 faces +dir.
#[allow(clippy::too_many_arguments)]
fn cap(m: &mut MeshData, l: Vec2, r: Vec2, lo: f32, hi: f32, f: f32, col: Vec3, s: Surface, uv: f32) {
    if hi - lo < 1e-3 {
        return;
    }
    if f > 0.0 {
        m.quad(v3(l, lo), v3(r, lo), v3(r, hi), v3(l, hi), col, s, uv);
    } else {
        m.quad(v3(r, lo), v3(l, lo), v3(l, hi), v3(r, hi), col, s, uv);
    }
}

/// Up-facing flat quad over the ground segment `c0`→`c1`, `w` to each side, at heights `y0`/`y1`.
#[allow(clippy::too_many_arguments)]
fn flat(m: &mut MeshData, c0: Vec2, c1: Vec2, n: Vec2, w: f32, y0: f32, y1: f32, col: Vec3, s: Surface, uv: f32) {
    m.quad(v3(c0 + n * w, y0), v3(c1 + n * w, y1), v3(c1 - n * w, y1), v3(c0 - n * w, y0), col, s, uv);
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

/// Convex planar profile `prof` (3..=6 points) extruded by `±off` (a fin / slab of any outline).
fn prism(m: &mut MeshData, prof: &[Vec3], off: Vec3, col: Vec3, s: Surface, uv: f32) {
    let k = prof.len();
    if k < 3 {
        return;
    }
    let c = prof.iter().copied().sum::<Vec3>() / k as f32;
    let (fr, bk): (Vec<Vec3>, Vec<Vec3>) = (prof.iter().map(|&p| p + off).collect(), prof.iter().map(|&p| p - off).collect());
    for i in 1..k - 1 {
        out_tri(m, fr[0], fr[i], fr[i + 1], c, col, s, uv);
        out_tri(m, bk[0], bk[i], bk[i + 1], c, col, s, uv);
    }
    for i in 0..k {
        let j = (i + 1) % k;
        out_quad(m, [fr[i], fr[j], bk[j], bk[i]], c, col, s, uv);
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
            if q.is_finite() && p.last().is_none_or(|l| l.distance(q) >= 0.05) {
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
    fn at(&self, s: f32) -> (Vec2, Vec2) {
        let s = s.clamp(0.0, self.total());
        let i = self.cum.partition_point(|&c| c <= s).saturating_sub(1).min(self.segs() - 1);
        let d = self.dir(i);
        (self.p[i] + d * (s - self.cum[i]), d)
    }
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
}

#[derive(Clone, Copy)]
struct St {
    s: f32,
    pt: Vec2,
    d0: Vec2,
    d1: Vec2,
    /// Real ground height.
    g: f32,
    /// Ground or water surface, whichever is higher.
    ge: f32,
}

impl St {
    /// Mitred offset towards the left by `half`.
    fn off(&self, half: f32) -> Vec2 {
        let (n0, n1) = (nlv(self.d0), nlv(self.d1));
        let k = 1.0 + n0.dot(n1);
        if k < 0.15 { n1 * half } else { (n0 + n1) * half / k.max(0.3) }
    }
    fn l(&self, h: f32) -> Vec2 {
        self.pt + self.off(h)
    }
    fn r(&self, h: f32) -> Vec2 {
        self.pt - self.off(h)
    }
}

#[derive(Clone, Copy, PartialEq)]
enum GK {
    Cut,
    Door,
    Wide,
    Water,
}

struct Gap {
    a: f32,
    b: f32,
    apex: f32,
    rv: f32,
    lintel: bool,
    kind: GK,
}

impl Gap {
    fn sc(&self) -> f32 {
        (self.a + self.b) * 0.5
    }
    fn arch(&self, s: f32) -> f32 {
        let r = ((self.b - self.a) * 0.5).max(1e-4);
        let u = ((s - self.sc()) / r).clamp(-1.0, 1.0);
        (self.apex - self.rv) + self.rv * (1.0 - u * u).max(0.0).sqrt()
    }
}

struct Win {
    a: f32,
    b: f32,
    sill: f32,
    head: f32,
}

#[derive(Clone, Copy, PartialEq)]
enum PK {
    Solid,
    Window(usize),
    Lintel(usize),
    Open,
}

struct Pc {
    kind: PK,
    hi: f32,
}

/// Parts of interval `a` not covered by `b`.
fn exposed(a: (f32, f32), b: Option<(f32, f32)>) -> [Option<(f32, f32)>; 2] {
    match b {
        None => [Some(a), None],
        Some((lb, hb)) => [
            (a.0 < lb).then(|| (a.0, lb.min(a.1))),
            (a.1 > hb).then(|| (hb.max(a.0), a.1)),
        ],
    }
}


// ---------------------------------------------------------------------------------------------
// Wall
// ---------------------------------------------------------------------------------------------

/// Wall mesh plus markers (perches on top, lamps by portals, windows, doors).
pub fn wall_mesh(wall: &Wall, cuts: &[Cut], terrain: &Terrain, seed: u64) -> (MeshData, Vec<Marker>) {
    let mut m = MeshData::default();
    let mut mk = Vec::new();
    build_wall(wall, cuts, terrain, seed, &mut m, &mut mk);
    if m.is_empty() || !m.is_valid() || mk.iter().any(|k| !k.pos.is_finite()) {
        return (MeshData::default(), Vec::new());
    }
    (m, mk)
}

#[allow(clippy::too_many_lines)]
fn build_wall(w: &Wall, cuts: &[Cut], terr: &Terrain, seed: u64, m: &mut MeshData, mk: &mut Vec<Marker>) {
    let Some(line) = Line::new(&w.points) else { return };
    if !w.height.is_finite() || w.height < 0.3 {
        return;
    }
    let hgt = w.height.min(40.0);
    let th = if w.thickness.is_finite() { w.thickness.clamp(0.2, 2.5) } else { 0.6 };
    let half = th * 0.5;
    let wood = w.kind == WallKind::Palisade;
    let og = if w.weathering.is_finite() { w.weathering.clamp(0.0, 1.0) } else { 0.0 };
    let total = line.total();
    let fh = if w.footing.is_finite() { w.footing.clamp(0.0, hgt * 0.6) } else { 0.0 };
    // Stone details (coping, lintels, sills, buttresses) are dry stone; the body follows the kind.
    let ccol = palette::dry_stone();
    let cs = Surface::DryStone;
    let (bs, bcol) = match w.kind {
        WallKind::DryStone => (Surface::DryStone, ccol),
        WallKind::Turf => (Surface::Turf, palette::turf()),
        WallKind::Palisade => (Surface::Plank, palette::driftwood()),
    };
    let green = palette::leaves();
    let eff = |p: Vec2| {
        let g = ground(terr, p);
        (g, wlevel(terr, p).map_or(g, |l| l.max(g)))
    };
    // Roofed rings keep a flat top (the scene roofs at max ground + height); everything else follows the ground.
    let flat_top = line.closed && w.roofed;
    let top_flat = line.p.iter().map(|&p| eff(p).1).fold(f32::MIN, f32::max) + hgt;
    let topat = |ge: f32| if flat_top { top_flat } else { ge + hgt };

    // ---- gaps (cuts, doors, water arches) and windows -------------------------------------
    let mut gaps: Vec<Gap> = Vec::new();
    let mut wins: Vec<Win> = Vec::new();
    let free = |gaps: &[Gap], wins: &[Win], a: f32, b: f32| {
        !gaps.iter().any(|g| a < g.b + 0.1 && b > g.a - 0.1) && !wins.iter().any(|x| a < x.b + 0.1 && b > x.a - 0.1)
    };
    let near_vertex = |a: f32, b: f32| line.cum.iter().any(|&c| c > a - 0.05 && c < b + 0.05);

    // Path cuts: square portals under a deep lintel.
    for c in cuts {
        if !(c.along.is_finite() && c.width.is_finite() && c.height.is_finite()) {
            continue;
        }
        let wd = c.width.clamp(0.4, 8.0).min(total - 0.2);
        if wd < 0.4 {
            continue;
        }
        let sc = c.along.clamp(wd * 0.5, total - wd * 0.5);
        let (a, b) = (sc - wd * 0.5, sc + wd * 0.5);
        if !free(&gaps, &wins, a, b) {
            continue;
        }
        let ge = eff(line.at(sc).0).1;
        let top = topat(ge);
        let hmax = top - ge - 0.5;
        let (apex, lintel) = if hmax < 1.0 { (top, false) } else { (ge + c.height.max(1.0).min(hmax), true) };
        gaps.push(Gap { a, b, apex, rv: 0.0, lintel, kind: GK::Cut });
    }
    for o in &w.openings {
        if !o.along.is_finite() {
            continue;
        }
        let wd = match o.kind {
            OpeningKind::Window => WINDOW_W,
            OpeningKind::Door => 1.1,
            OpeningKind::WideDoor => 2.4,
        };
        let (a, b) = (o.along - wd * 0.5, o.along + wd * 0.5);
        if a < 0.1 || b > total - 0.1 || !free(&gaps, &wins, a, b) || near_vertex(a, b) {
            continue;
        }
        let ge = eff(line.at(o.along).0).1;
        let top = topat(ge);
        match o.kind {
            OpeningKind::Window => {
                let sill = ge + 1.0_f32.max(fh + 0.3);
                if top - ge >= 2.2 && sill + 1.0 <= top - 0.2 {
                    wins.push(Win { a, b, sill, head: (sill + 1.6).min(top - 0.35) });
                }
            }
            OpeningKind::Door => {
                if top - ge >= 2.5 {
                    gaps.push(Gap { a, b, apex: ge + 2.1, rv: 0.0, lintel: true, kind: GK::Door });
                }
            }
            OpeningKind::WideDoor => {
                let apex = (ge + 2.8).min(top - 0.5);
                if apex - ge >= 1.8 {
                    gaps.push(Gap { a, b, apex, rv: 0.0, lintel: true, kind: GK::Wide });
                }
            }
        }
    }
    // Water: aqueduct arches between piers.
    {
        let n = (total / 0.5).ceil() as usize;
        let wet: Vec<bool> = (0..=n).map(|i| wlevel(terr, line.at((i as f32 * 0.5).min(total)).0).is_some()).collect();
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
            let a = (i as f32 * 0.5 - 0.25).max(0.0);
            let b = (j as f32 * 0.5 + 0.25).min(total);
            let len = b - a;
            if len >= 1.5 {
                let nb = ((len / 3.0).floor() as usize).max(1);
                for k in 0..nb {
                    let ba = a + len * k as f32 / nb as f32 + 0.3;
                    let bb = a + len * (k + 1) as f32 / nb as f32 - 0.3;
                    if bb - ba < 0.5 || !free(&gaps, &wins, ba, bb) {
                        continue;
                    }
                    let (pt, _) = line.at((ba + bb) * 0.5);
                    let g = ground(terr, pt);
                    let wl = wlevel(terr, pt).unwrap_or(g);
                    let apex = (wl + 1.2).min(topat(wl) - 0.45);
                    if apex < wl + 0.5 {
                        continue;
                    }
                    let rv = if wood { 0.0 } else { ((bb - ba) * 0.5).min((apex - g - 0.3).max(0.0)) };
                    gaps.push(Gap { a: ba, b: bb, apex, rv, lintel: true, kind: GK::Water });
                }
            }
            i = j + 1;
        }
    }
    gaps.sort_by(|x, y| x.a.total_cmp(&y.a));
    wins.sort_by(|x, y| x.a.total_cmp(&y.a));

    // ---- stations ---------------------------------------------------------------------------
    let mut raw: Vec<(f32, u8, Option<usize>)> = Vec::new();
    for (i, &c) in line.cum.iter().enumerate() {
        raw.push((c, 3, Some(i)));
    }
    let step = if w.tumbled { 0.7 } else { 1.0 };
    for i in 0..line.segs() {
        let len = line.cum[i + 1] - line.cum[i];
        let n = (len / step).ceil().max(1.0) as usize;
        for k in 1..n {
            let s = line.cum[i] + len * k as f32 / n as f32;
            let inside = gaps.iter().any(|g| s > g.a - 0.03 && s < g.b + 0.03) || wins.iter().any(|x| s > x.a - 0.03 && s < x.b + 0.03);
            if !inside {
                raw.push((s, 1, None));
            }
        }
    }
    for g in &gaps {
        let n = if g.rv <= 0.0 { 2 } else if g.b - g.a > 1.0 { 8 } else { 4 };
        for k in 0..=n {
            raw.push((g.a + (g.b - g.a) * k as f32 / n as f32, 2, None));
        }
    }
    for x in &wins {
        raw.push((x.a, 2, None));
        raw.push((x.b, 2, None));
    }
    raw.sort_by(|x, y| x.0.total_cmp(&y.0));
    let mut ded: Vec<(f32, u8, Option<usize>)> = Vec::new();
    for r in raw {
        if let Some(l) = ded.last_mut() {
            if r.0 - l.0 < 0.03 {
                if r.1 > l.1 {
                    *l = r;
                }
                continue;
            }
        }
        ded.push(r);
    }
    let st: Vec<St> = ded
        .iter()
        .map(|&(s, _, vi)| {
            let (pt, d0, d1) = match vi {
                Some(i) => {
                    let (a, b) = line.vdirs(i);
                    (line.p[i], a, b)
                }
                None => {
                    let (p, d) = line.at(s);
                    (p, d, d)
                }
            };
            let (g, ge) = eff(pt);
            St { s, pt, d0, d1, g, ge }
        })
        .collect();
    let nst = st.len();
    if nst < 2 {
        return;
    }
    let np = nst - 1;

    // ---- pieces -----------------------------------------------------------------------------
    let mut pcs: Vec<Pc> = Vec::with_capacity(np);
    for k in 0..np {
        let (a, b) = (&st[k], &st[k + 1]);
        let sm = (a.s + b.s) * 0.5;
        let mid = (a.pt + b.pt) * 0.5;
        let top = topat((a.ge + b.ge) * 0.5);
        let mut hi = top;
        if w.tumbled {
            // Broken, stepped top: most pieces lose a random chunk, a few keep full height.
            let mut r = Rng::at(seed ^ 0x51, mid.x, mid.y);
            if r.f() >= 0.25 {
                hi = top - r.range(0.08, 0.55) * hgt;
            }
        }
        hi = hi.max(a.ge.max(b.ge) + 0.45);
        let kind = if let Some(g) = gaps.iter().position(|g| sm > g.a && sm < g.b) {
            if gaps[g].lintel {
                hi = hi.max(gaps[g].apex + 0.45);
                PK::Lintel(g)
            } else {
                PK::Open
            }
        } else if let Some(x) = wins.iter().position(|x| sm > x.a && sm < x.b) {
            hi = hi.max(wins[x].head + 0.25);
            PK::Window(x)
        } else {
            PK::Solid
        };
        pcs.push(Pc { kind, hi });
    }
    // Thick flat coping overhanging both faces (not on ruins: their tops are broken).
    let lip = !w.tumbled;
    let lip_h = 0.18;
    // Thick walls without planters get a walkable top (stairs lead up to it).
    let walk = lip && th >= WALK_MIN_TH && !w.crest;
    let lip_x = 0.08;
    let pl_x = 0.08;
    let has_pl = |pi: usize| fh > 0.0 && matches!(pcs[pi].kind, PK::Solid | PK::Window(_));
    let ph = |pi: usize, ge: f32| (ge + fh).min(pcs[pi].hi - 0.1);
    let pcol = palette::foundation();
    // Solid interval of piece `pi` at arc position `s` over ground `g`.
    let iv = |pi: usize, s: f32, g: f32| -> Option<(f32, f32)> {
        match pcs[pi].kind {
            PK::Open => None,
            PK::Lintel(gi) => Some((gaps[gi].arch(s).min(pcs[pi].hi), pcs[pi].hi)),
            _ => Some((g - 0.3, pcs[pi].hi)),
        }
    };

    // ---- boundaries (end faces / jambs / ruin steps / plinth strips / coping ends) -----------
    let last_b = if line.closed { np - 1 } else { np };
    for k in 0..=last_b {
        let a_idx = if k > 0 { Some(k - 1) } else if line.closed { Some(np - 1) } else { None };
        let b_idx = if k < np { Some(k) } else { None };
        let stn = &st[k];
        let (l, r) = (stn.l(half), stn.r(half));
        let sa = if k == 0 && line.closed { st[np].s } else { stn.s };
        let ia = a_idx.and_then(|i| iv(i, sa, stn.g));
        let ib = b_idx.and_then(|i| iv(i, stn.s, stn.g));
        if let Some(x) = ia {
            for (lo, hi) in exposed(x, ib).into_iter().flatten() {
                cap(m, l, r, lo, hi, 1.0, bcol * 0.92, bs, UV);
            }
        }
        if let Some(x) = ib {
            for (lo, hi) in exposed(x, ia).into_iter().flatten() {
                cap(m, l, r, lo, hi, -1.0, bcol * 0.92, bs, UV);
            }
        }
        let pa = a_idx.is_some_and(&has_pl);
        let pb = b_idx.is_some_and(&has_pl);
        if pa != pb {
            let (pi, f) = if pa { (a_idx.unwrap_or(0), 1.0) } else { (b_idx.unwrap_or(0), -1.0) };
            let (lp, rp) = (stn.l(half + pl_x), stn.r(half + pl_x));
            let hp = ph(pi, stn.ge);
            cap(m, lp, l, stn.g - 0.3, hp, f, pcol, Surface::Rock, UV);
            cap(m, r, rp, stn.g - 0.3, hp, f, pcol, Surface::Rock, UV);
        }
        if lip {
            let ca = ia.map(|x| (x.1, x.1 + lip_h));
            let cb = ib.map(|x| (x.1, x.1 + lip_h));
            let (cl, cr) = (stn.l(half + lip_x), stn.r(half + lip_x));
            if let Some(x) = ca {
                for (lo, hi) in exposed(x, cb).into_iter().flatten() {
                    cap(m, cl, cr, lo, hi, 1.0, ccol * 0.95, cs, UV);
                }
            }
            if let Some(x) = cb {
                for (lo, hi) in exposed(x, ca).into_iter().flatten() {
                    cap(m, cl, cr, lo, hi, -1.0, ccol * 0.95, cs, UV);
                }
            }
        }
    }

    // ---- per-piece geometry -------------------------------------------------------------------
    for k in 0..np {
        let (a, b) = (&st[k], &st[k + 1]);
        let pc = &pcs[k];
        if pc.kind == PK::Open {
            continue;
        }
        let dir = (b.pt - a.pt).normalize_or(a.d1);
        let n = nlv(dir);
        let (l0, l1, r0, r1) = (a.l(half), b.l(half), a.r(half), b.r(half));
        let mid = (a.pt + b.pt) * 0.5;
        let tint = 0.94 + 0.12 * hash_f(seed, mid.x, mid.y);
        let col = bcol * tint;
        let cc = ccol * tint;
        let hi = pc.hi;
        let (lo0, lo1) = match pc.kind {
            PK::Lintel(g) => (gaps[g].arch(a.s), gaps[g].arch(b.s)),
            _ => (a.g - 0.3, b.g - 0.3),
        };
        match pc.kind {
            PK::Window(x) => {
                // Deep slot: glazing set far back, concrete reveals.
                let win = &wins[x];
                let ins = (th * 0.35).min(0.35);
                let (l0i, l1i, r0i, r1i) = (l0 - n * ins, l1 - n * ins, r0 + n * ins, r1 + n * ins);
                let wcol = Vec3::splat(0.9);
                for (left, p0, p1, q0, q1) in [(true, l0, l1, l0i, l1i), (false, r0, r1, r0i, r1i)] {
                    side(m, p0, p1, lo0, lo1, win.sill, win.sill, left, col, bs, UV);
                    side(m, q0, q1, win.sill, win.sill, win.head, win.head, left, wcol, Surface::Window, 1.0);
                    side(m, p0, p1, win.head, win.head, hi, hi, left, col, bs, UV);
                }
                let rc = cc * 0.85;
                m.quad(v3(l0, win.sill), v3(l1, win.sill), v3(l1i, win.sill), v3(l0i, win.sill), rc, cs, UV);
                m.quad(v3(r0i, win.sill), v3(r1i, win.sill), v3(r1, win.sill), v3(r0, win.sill), rc, cs, UV);
                m.quad(v3(l0i, win.head), v3(l1i, win.head), v3(l1, win.head), v3(l0, win.head), rc * 0.8, cs, UV);
                m.quad(v3(r1i, win.head), v3(r0i, win.head), v3(r0, win.head), v3(r1, win.head), rc * 0.8, cs, UV);
                let (s, h) = (win.sill, win.head);
                m.quad(v3(l0, s), v3(l0i, s), v3(l0i, h), v3(l0, h), rc, cs, UV);
                m.quad(v3(l1i, s), v3(l1, s), v3(l1, h), v3(l1i, h), rc, cs, UV);
                m.quad(v3(r0i, s), v3(r0, s), v3(r0, h), v3(r0i, h), rc, cs, UV);
                m.quad(v3(r1, s), v3(r1i, s), v3(r1i, h), v3(r1, h), rc, cs, UV);
            }
            _ => {
                side(m, l0, l1, lo0, lo1, hi, hi, true, col, bs, UV);
                side(m, r0, r1, lo0, lo1, hi, hi, false, col, bs, UV);
            }
        }
        if matches!(pc.kind, PK::Lintel(_)) {
            m.quad(v3(r0, lo0), v3(r1, lo1), v3(l1, lo1), v3(l0, lo0), cc * 0.8, cs, UV);
        }
        if lip {
            let (p0, p1, q0, q1) = (a.l(half + lip_x), b.l(half + lip_x), a.r(half + lip_x), b.r(half + lip_x));
            side(m, p0, p1, hi, hi, hi + lip_h, hi + lip_h, true, cc, cs, UV);
            side(m, q0, q1, hi, hi, hi + lip_h, hi + lip_h, false, cc, cs, UV);
            // soffit under the overhang
            m.quad(v3(p1, hi), v3(p0, hi), v3(l0, hi), v3(l1, hi), cc * 0.7, cs, UV);
            m.quad(v3(r0, hi), v3(q0, hi), v3(q1, hi), v3(r1, hi), cc * 0.7, cs, UV);
            m.quad(v3(p0, hi + lip_h), v3(p1, hi + lip_h), v3(q1, hi + lip_h), v3(q0, hi + lip_h), cc * 1.06, cs, UV);
            if walk {
                // low upstands on both coping edges; the coping between them is the walk slab
                let (y0, y1) = (hi + lip_h, hi + lip_h + UPSTAND_H);
                let (eo, ei) = (half + lip_x, half + lip_x - UPSTAND_W);
                let prev = if k > 0 { Some(k - 1) } else if line.closed { Some(np - 1) } else { None };
                let next = if k + 1 < np { Some(k + 1) } else if line.closed { Some(0) } else { None };
                let open_end = |j: Option<usize>| j.is_none_or(|j| pcs[j].kind == PK::Open || (pcs[j].hi - hi).abs() > 1e-3);
                for sg in [1.0_f32, -1.0] {
                    let (ao, bo, ai, bi) = if sg > 0.0 { (a.l(eo), b.l(eo), a.l(ei), b.l(ei)) } else { (a.r(eo), b.r(eo), a.r(ei), b.r(ei)) };
                    side(m, ao, bo, y0, y0, y1, y1, sg > 0.0, cc, cs, UV);
                    side(m, ai, bi, y0, y0, y1, y1, sg < 0.0, cc * 0.92, cs, UV);
                    let (ll0, ll1, rr0, rr1) = if sg > 0.0 { (ao, bo, ai, bi) } else { (ai, bi, ao, bo) };
                    m.quad(v3(ll0, y1), v3(ll1, y1), v3(rr1, y1), v3(rr0, y1), cc * 1.06, cs, UV);
                    if open_end(prev) {
                        cap(m, ll0, rr0, y0, y1, -1.0, cc * 0.95, cs, UV);
                    }
                    if open_end(next) {
                        cap(m, ll1, rr1, y0, y1, 1.0, cc * 0.95, cs, UV);
                    }
                }
            }
        } else {
            let (ts, tc) = if wood { (cs, cc) } else { (bs, col) };
            m.quad(v3(l0, hi), v3(l1, hi), v3(r1, hi), v3(r0, hi), tc * 1.05, ts, UV);
        }
        // plinth: darker bush-hammered band, slightly proud
        if has_pl(k) {
            let pc2 = pcol * tint;
            let (p0, p1, q0, q1) = (a.l(half + pl_x), b.l(half + pl_x), a.r(half + pl_x), b.r(half + pl_x));
            let (h0, h1) = (ph(k, a.ge), ph(k, b.ge));
            side(m, p0, p1, a.g - 0.3, b.g - 0.3, h0, h1, true, pc2, Surface::Rock, UV);
            side(m, q0, q1, a.g - 0.3, b.g - 0.3, h0, h1, false, pc2, Surface::Rock, UV);
            m.quad(v3(p0, h0), v3(p1, h1), v3(l1, h1), v3(l0, h0), pc2 * 1.1, Surface::Rock, UV);
            m.quad(v3(r0, h0), v3(r1, h1), v3(q1, h1), v3(q0, h0), pc2 * 1.1, Surface::Rock, UV);
        }
        // exposed rebar on broken tops
        if w.tumbled {
            let full = topat((a.ge + b.ge) * 0.5);
            let mut rg = Rng::at(seed ^ 0x8EBA, mid.x, mid.y);
            if hi < full - 0.05 && rg.f() < 0.8 {
                let nr = 2 + (rg.f() * 3.0) as usize;
                for _ in 0..nr {
                    let u = rg.range(0.15, 0.85);
                    let c = a.pt.lerp(b.pt, u) + n * rg.range(-0.6, 0.6) * half;
                    let lean = Vec2::new(rg.range(-0.18, 0.18), rg.range(-0.18, 0.18));
                    let len = rg.range(0.25, 0.75);
                    let base = v3(c, hi - 0.1);
                    m.beam(base, base + Vec3::new(lean.x, len, lean.y), 0.04, 0.04, RUST * rg.range(0.8, 1.3), Surface::Metal, 1.0);
                }
            }
        }
        // overgrowth: ivy from the base, water stains under the coping, moss on top
        if og > 0.0 {
            let len = (b.pt - a.pt).length().max(1e-3);
            let mut rg = Rng::at(seed ^ 0x0E61, mid.x, mid.y);
            for sg in [1.0_f32, -1.0] {
                let (f0, f1) = if sg > 0.0 { (l0, l1) } else { (r0, r1) };
                let nn = n * sg;
                let left = sg > 0.0;
                // ivy climbing from the ground in stepped tiers
                let ivy_roll = rg.f();
                let (u0, wfrac, hfrac) = (rg.f(), rg.range(0.5, 1.0), rg.range(0.35, 1.0));
                if !matches!(pc.kind, PK::Lintel(_)) && ivy_roll < og * 0.85 {
                    let cap_y = match pc.kind {
                        PK::Window(x) => wins[x].sill - 0.05,
                        _ => hi - 0.05,
                    };
                    let gm = a.g.min(b.g);
                    let ht = (og * hfrac * (cap_y - gm)).min(cap_y - gm);
                    if ht > 0.2 {
                        for t in 0..3 {
                            let wf = wfrac * (1.0 - t as f32 * 0.28);
                            let ua = (u0 * (1.0 - wf)).clamp(0.0, 1.0);
                            let ub = (ua + wf).min(1.0);
                            let ya = gm - 0.05 + ht * t as f32 / 3.0;
                            let yb = gm + ht * (t as f32 + 1.0) / 3.0;
                            let off = if fh > 0.0 && ya < a.ge.min(b.ge) + fh { pl_x + 0.02 } else { 0.02 };
                            let c = green * (0.55 + 0.12 * t as f32) * rg.range(0.85, 1.1);
                            side(m, f0.lerp(f1, ua) + nn * off, f0.lerp(f1, ub) + nn * off, ya, ya, yb, yb, left, c, Surface::Fiber, 1.0);
                        }
                    }
                }
                // dark streaks running down from the coping
                for _ in 0..2 {
                    let (roll, u, sw, sl) = (rg.f(), rg.f(), rg.range(0.08, 0.22), rg.range(0.3, 1.5));
                    if roll >= og * 0.75 || sw >= len {
                        continue;
                    }
                    let ua = u * (1.0 - sw / len);
                    let ub = ua + sw / len;
                    let yb = hi;
                    let mut ya = yb - sl * (0.5 + og);
                    ya = match pc.kind {
                        PK::Window(x) => ya.max(wins[x].head + 0.02),
                        PK::Lintel(_) => ya.max(lo0.max(lo1) + 0.05),
                        _ => ya.max(a.g.max(b.g) + fh + 0.1),
                    };
                    if yb - ya > 0.1 {
                        side(m, f0.lerp(f1, ua) + nn * 0.01, f0.lerp(f1, ub) + nn * 0.01, ya, ya, yb, yb, left, col * 0.7, bs, UV);
                    }
                }
            }
            // moss on the top surface
            let (roll, u, ml, mwf) = (rg.f(), rg.f(), rg.range(0.3, 0.9), rg.range(0.35, 0.8));
            if roll < og {
                let ml = (ml / len).min(1.0);
                let ua = u * (1.0 - ml);
                let ty = hi + if lip { lip_h } else { 0.0 } + 0.012;
                let c = green * rg.range(0.55, 0.8) + Vec3::new(0.04, 0.03, 0.0);
                flat(m, a.pt.lerp(b.pt, ua), a.pt.lerp(b.pt, ua + ml), n, half * mwf, ty, ty, c, Surface::Fiber, 1.0);
            }
        }
    }

    // ---- window sills, lintels and door leaves ----------------------------------------------
    let trim = palette::trim(w.colors);
    let piece_at = |s: f32| st.partition_point(|x| x.s <= s).saturating_sub(1).min(np - 1);
    let hi_of = |s: f32| -> Option<f32> {
        let p = &pcs[piece_at(s)];
        (p.kind != PK::Open).then_some(p.hi)
    };
    for x in &wins {
        let sc = (x.a + x.b) * 0.5;
        let (pt, d) = line.at(sc);
        let n = nlv(d);
        let d3 = Vec3::new(d.x, 0.0, d.y);
        let yc = (x.sill + x.head) * 0.5;
        for sg in [-1.0_f32, 1.0] {
            let c = pt + n * sg * (half + 0.05);
            m.oriented_box(v3(c, x.sill - 0.05), Vec3::new((x.b - x.a) * 0.5 + 0.1, 0.06, 0.06), d3, ccol * 0.9, cs, UV);
        }
        mk.push(Marker { kind: MarkerKind::WindowLight, pos: v3(pt + n * half, yc), dir: Vec3::new(n.x, 0.0, n.y) });
    }
    for g in &gaps {
        let sc = g.sc();
        let (pt, d) = line.at(sc);
        let n = nlv(d);
        let n3 = Vec3::new(n.x, 0.0, n.y);
        let d3 = Vec3::new(d.x, 0.0, d.y);
        let ge = eff(pt).1;
        let top = topat(ge);
        // deep proud lintel beam over portals
        if g.lintel && matches!(g.kind, GK::Cut | GK::Wide) {
            if let Some(h) = hi_of(sc) {
                let (y0, y1) = (g.apex - 0.04, (g.apex + 0.55).min(h));
                if y1 - y0 > 0.1 {
                    let hl = (g.b - g.a) * 0.5 + 0.35;
                    m.oriented_box(v3(pt, (y0 + y1) * 0.5), Vec3::new(hl, (y1 - y0) * 0.5, half + 0.06), d3, ccol * 0.97, cs, UV);
                }
            }
        }
        match g.kind {
            GK::Door | GK::Wide => {
                mk.push(Marker { kind: MarkerKind::Door, pos: v3(pt + n * (half + 0.8), ge), dir: n3 });
            }
            GK::Cut => {
                if top - ge >= 1.5 {
                    for s in [g.a - 0.3, g.b + 0.3] {
                        let (p, dd) = line.at(s.clamp(0.0, total));
                        let nn = nlv(dd);
                        let y = (eff(p).1 + 2.4).min(topat(eff(p).1) - 0.35);
                        let face = p + nn * half;
                        let lamp = face + nn * 0.3;
                        m.beam(v3(face, y), v3(lamp, y), 0.05, 0.05, palette::metal(), Surface::Metal, 1.0);
                        m.aabb(v3(lamp, y) + Vec3::new(-0.09, 0.02, -0.09), v3(lamp, y) + Vec3::new(0.09, 0.2, 0.09), palette::glow(), Surface::Glow, 1.0);
                        mk.push(Marker { kind: MarkerKind::Lantern, pos: v3(lamp, y + 0.1), dir: Vec3::new(nn.x, 0.0, nn.y) });
                    }
                }
            }
            GK::Water => {}
        }
        if g.kind == GK::Door {
            // recessed flush leaf at mid-thickness
            let pts: Vec<&St> = st.iter().filter(|q| q.s >= g.a - 0.03 && q.s <= g.b + 0.03).collect();
            if pts.len() >= 2 {
                let (q0, q1) = (pts[0], pts[pts.len() - 1]);
                let (y0, y1) = (g.arch(q0.s), g.arch(q1.s));
                for (sg, left) in [(0.03_f32, true), (-0.03, false)] {
                    side(m, q0.pt + n * sg, q1.pt + n * sg, q0.g, q1.g, y0, y1, left, trim, Surface::Door, 1.0);
                }
            }
        }
    }

    // ---- top features ---------------------------------------------------------------------
    let top_extra = if lip { lip_h } else { 0.0 };
    if w.crest {
        let nm = ((total / 1.5).round() as usize).max(1);
        let sp = total / nm as f32;
        let mw = (th * 0.38).clamp(0.2, 0.45);
        let ml = (sp * 0.6).min(1.0);
        for j in 0..nm {
            let s = (j as f32 + 0.5) * sp;
            let Some(h) = hi_of(s) else { continue };
            let base = h + top_extra;
            let (pt, d) = line.at(s);
            let n = nlv(d);
            let d3 = Vec3::new(d.x, 0.0, d.y);
            for sg in [1.0_f32, -1.0] {
                let edge = half + if lip { lip_x } else { 0.0 };
                let c = pt + n * sg * (edge - mw * 0.5);
                let mut rg = Rng::at(seed ^ 0xBA77, c.x, c.y);
                let mut mh = 0.45;
                let tn = 0.92 + 0.12 * rg.f();
                if w.tumbled {
                    if rg.f() < 0.35 {
                        continue;
                    }
                    mh = rg.range(0.2, 0.45);
                }
                m.oriented_box(v3(c, base + mh * 0.5), Vec3::new(ml * 0.5, mh * 0.5, mw * 0.5), d3, ccol * tn, cs, UV);
                let lh = rg.range(0.12, 0.32);
                let lc = green * rg.range(0.8, 1.1);
                m.oriented_box(v3(c, base + mh + lh * 0.5 - 0.04), Vec3::new(ml * 0.5 - 0.03, lh * 0.5, mw * 0.5 - 0.03), d3, lc, Surface::Leaves, 1.0);
                // greenery spilling over the outer edge
                let wf = rg.range(0.35, 0.9);
                let drop = rg.range(0.25, 0.7) * (0.6 + og * 0.6);
                let u = rg.range(-0.5, 0.5) * (1.0 - wf) * ml;
                let face = c + n * sg * (mw * 0.5 + 0.02) + d * u;
                let (p0, p1) = (face - d * (ml * wf * 0.5), face + d * (ml * wf * 0.5));
                let yt = base + mh + 0.02;
                side(m, p0, p1, yt - drop, yt - drop * 0.7, yt, yt, sg > 0.0, lc * 0.8, Surface::Fiber, 1.0);
            }
        }
    }
    {
        let n = ((total / 4.0).round() as usize).max(1);
        for j in 0..n {
            let s = (j as f32 + 0.5) * total / n as f32;
            if let Some(h) = hi_of(s) {
                mk.push(Marker { kind: MarkerKind::Perch, pos: v3(line.at(s).0, h + top_extra), dir: Vec3::Y });
            }
        }
    }
    // raking buttress fins on tall walls (both faces)
    if hgt > 5.0 {
        let mut s = 2.0;
        while s < total - 1.0 {
            let ok = !gaps.iter().any(|g| s > g.a - 1.0 && s < g.b + 1.0) && !wins.iter().any(|x| s > x.a - 1.0 && s < x.b + 1.0);
            if let (true, Some(h)) = (ok, hi_of(s)) {
                let (pt, d) = line.at(s);
                let n = nlv(d);
                let off = Vec3::new(d.x, 0.0, d.y) * 0.16;
                for sg in [1.0_f32, -1.0] {
                    let nn = n * sg;
                    let f = pt + nn * half;
                    let y0 = ground(terr, f + nn * 0.8).min(ground(terr, pt)) - 0.3;
                    let y1 = h - 0.02;
                    let reach = ((y1 - y0) * 0.42).clamp(1.0, 3.5);
                    let tip = f + nn * reach;
                    let nose = ground(terr, tip).max(y0 + 0.3) + 0.35;
                    let prof = [v3(f - nn * 0.05, y0), v3(tip, y0), v3(tip, nose), v3(f - nn * 0.05, y1)];
                    prism(m, &prof, off, ccol * 0.9, cs, UV);
                }
            }
            s += 4.0;
        }
    }
    // rubble at the foot of ruins
    if w.tumbled {
        let (rs, rc) = if wood { (cs, ccol) } else { (bs, bcol) };
        let n = (total / 0.9).ceil() as usize;
        for j in 0..n {
            let s = (j as f32 + 0.5) * total / n as f32;
            let (pt, d) = line.at(s);
            let mut rg = Rng::at(seed ^ 0x2B1E, pt.x, pt.y);
            if rg.f() > 0.7 {
                continue;
            }
            let sg = if rg.f() < 0.5 { 1.0 } else { -1.0 };
            let c = pt + nlv(d) * sg * (half + rg.range(0.1, 0.6));
            let hs = Vec3::new(rg.range(0.12, 0.3), rg.range(0.08, 0.2), rg.range(0.1, 0.25));
            let ang = rg.range(0.0, std::f32::consts::TAU);
            let y = ground(terr, c) + hs.y * 0.7;
            m.oriented_box(Vec3::new(c.x, y, c.y), hs, Vec3::new(ang.cos(), 0.0, ang.sin()), rc * rg.range(0.7, 0.95), rs, UV);
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Fence
// ---------------------------------------------------------------------------------------------

/// Fence mesh. Gaps where `cuts` are.
pub fn fence_mesh(fence: &Fence, cuts: &[Cut], terrain: &Terrain, seed: u64) -> (MeshData, Vec<Marker>) {
    let mut m = MeshData::default();
    build_fence(fence, cuts, terrain, seed, &mut m);
    if m.is_empty() || !m.is_valid() {
        return (MeshData::default(), Vec::new());
    }
    (m, Vec::new())
}

#[allow(clippy::too_many_lines)]
fn build_fence(f: &Fence, cuts: &[Cut], terr: &Terrain, seed: u64, m: &mut MeshData) {
    let Some(line) = Line::new(&f.points) else { return };
    let total = line.total();
    let mut gaps: Vec<(f32, f32)> = Vec::new();
    for c in cuts {
        if c.along.is_finite() && c.width.is_finite() {
            let wd = c.width.clamp(0.3, 8.0);
            let (a, b) = ((c.along - wd * 0.5).max(0.0), (c.along + wd * 0.5).min(total));
            if b > a {
                gaps.push((a, b));
            }
        }
    }
    gaps.sort_by(|x, y| x.0.total_cmp(&y.0));
    let mut runs: Vec<(f32, f32)> = Vec::new();
    let mut cur = 0.0_f32;
    for (a, b) in gaps {
        if a - 0.08 >= cur {
            runs.push((cur, a - 0.08));
        }
        cur = cur.max(b + 0.08);
    }
    if cur <= total {
        runs.push((cur, total));
    }
    let trim = palette::trim(f.colors);
    let conc = palette::wall(f.colors);
    let g_at = |p: Vec2| ground(terr, p);
    // Vertical box standing on the ground at arc position `s`.
    let post = |m: &mut MeshData, s: f32, hw: f32, h: f32, col: Vec3, srf: Surface, uv: f32| {
        let (pt, d) = line.at(s);
        let y0 = g_at(pt) - 0.2;
        m.oriented_box(v3(pt, y0 + (h + 0.2) * 0.5), Vec3::new(hw, (h + 0.2) * 0.5, hw), Vec3::new(d.x, 0.0, d.y), col, srf, uv);
    };
    for (r0, r1) in runs {
        match f.kind {
            FenceKind::Driftwood => {
                // steel tube posts + two rails, tiny concrete footings
                let ss = samples(&line, r0, r1, 1.8);
                for &s in &ss {
                    post(m, s, 0.03, 1.05, trim * 0.85, Surface::Timber, 1.0);
                    post(m, s, 0.08, 0.06, conc * 0.85, Surface::DryStone, UV);
                }
                for pair in ss.windows(2) {
                    let (p0, p1) = (line.at(pair[0]).0, line.at(pair[1]).0);
                    let (g0, g1) = (g_at(p0), g_at(p1));
                    for hh in [0.5, 1.0] {
                        m.beam(v3(p0, g0 + hh), v3(p1, g1 + hh), 0.05, 0.05, trim, Surface::Timber, 1.0);
                    }
                }
            }
            FenceKind::Wattle => {
                // concrete base + squat balusters + chunky cap, piers at vertices / every 2.4 m
                let ss = samples(&line, r0, r1, 2.4);
                for &s in &ss {
                    post(m, s, 0.1, 0.95, conc * 0.92, Surface::DryStone, UV);
                }
                for pair in ss.windows(2) {
                    let (p0, p1) = (line.at(pair[0]).0, line.at(pair[1]).0);
                    let (g0, g1) = (g_at(p0), g_at(p1));
                    m.beam(v3(p0, g0 - 0.2), v3(p1, g1 - 0.2), 0.3, 0.38, conc * 0.9, Surface::DryStone, UV);
                    m.beam(v3(p0, g0 + 0.72), v3(p1, g1 + 0.72), 0.32, 0.14, conc * 1.05, Surface::DryStone, UV);
                    let len = p0.distance(p1);
                    let nb = (len / 0.32).floor() as usize;
                    let d = (p1 - p0).normalize_or(Vec2::X);
                    let d3 = Vec3::new(d.x, 0.0, d.y);
                    for j in 1..nb {
                        let u = j as f32 / nb as f32;
                        if len * u.min(1.0 - u) < 0.2 {
                            continue;
                        }
                        let p = p0.lerp(p1, u);
                        let g = g0 + (g1 - g0) * u;
                        let mut rg = Rng::at(seed ^ 0xBA1, p.x, p.y);
                        m.oriented_box(v3(p, g + 0.445), Vec3::new(0.065, 0.285, 0.065), d3, conc * rg.range(0.88, 1.02), Surface::DryStone, UV);
                    }
                }
            }
            FenceKind::NetLine => {
                // wire cages filled with rocks, ~1 m modules, 0.9 m tall
                let ss = samples(&line, r0, r1, 1.0);
                let wire = palette::metal() * 1.5;
                for pair in ss.windows(2) {
                    let (p0, p1) = (line.at(pair[0]).0, line.at(pair[1]).0);
                    let len = p0.distance(p1);
                    if len < 0.05 {
                        continue;
                    }
                    let d = (p1 - p0) / len;
                    let n = nlv(d);
                    let (g0, g1) = (g_at(p0), g_at(p1));
                    let mut rg = Rng::at(seed ^ 0x6AB, p0.x, p0.y);
                    for course in 0..3 {
                        let split = rg.range(0.35, 0.65);
                        for (ua, ub) in [(0.0, split), (split, 1.0)] {
                            let u = (ua + ub) * 0.5;
                            let c = p0.lerp(p1, u) + n * rg.range(-0.03, 0.03);
                            let g = g0 + (g1 - g0) * u;
                            let yc = g - 0.02 + 0.3 * course as f32 + 0.15;
                            let hs = Vec3::new(len * (ub - ua) * 0.47, rg.range(0.12, 0.15), rg.range(0.19, 0.23));
                            let yaw = rg.range(-0.15, 0.15);
                            let dd = Vec2::from_angle(yaw).rotate(d);
                            m.oriented_box(v3(c, yc), hs, Vec3::new(dd.x, 0.0, dd.y), palette::rock() * rg.range(0.72, 1.1), Surface::Rock, UV);
                        }
                    }
                    for sg in [1.0_f32, -1.0] {
                        let (q0, q1) = (p0 + n * sg * 0.25, p1 + n * sg * 0.25);
                        let left = sg > 0.0;
                        for hh in [0.0_f32, 0.3, 0.6, 0.9] {
                            side(m, q0, q1, g0 + hh - 0.015, g1 + hh - 0.015, g0 + hh + 0.015, g1 + hh + 0.015, left, wire, Surface::Metal, 1.0);
                        }
                        for u in [0.0_f32, 0.5, 1.0] {
                            let q = q0.lerp(q1, u);
                            let g = g0 + (g1 - g0) * u;
                            let (qa, qb) = (q - d * 0.015, q + d * 0.015);
                            side(m, qa, qb, g - 0.05, g - 0.05, g + 0.915, g + 0.915, left, wire, Surface::Metal, 1.0);
                        }
                    }
                    // top wires across
                    for u in [0.0_f32, 0.5, 1.0] {
                        let q = p0.lerp(p1, u);
                        let y = g0 + (g1 - g0) * u + 0.92;
                        m.quad(v3(q + n * 0.25 - d * 0.015, y), v3(q + n * 0.25 + d * 0.015, y), v3(q - n * 0.25 + d * 0.015, y), v3(q - n * 0.25 - d * 0.015, y), wire, Surface::Metal, 1.0);
                    }
                }
            }
            FenceKind::Reed => {
                // concrete trough with a boxy clipped hedge
                let ss = samples(&line, r0, r1, 1.0);
                for pair in ss.windows(2) {
                    let (p0, p1) = (line.at(pair[0]).0, line.at(pair[1]).0);
                    let (g0, g1) = (g_at(p0), g_at(p1));
                    let mut rg = Rng::at(seed ^ 0xA77, p0.x, p0.y);
                    m.beam(v3(p0, g0 - 0.2), v3(p1, g1 - 0.2), 0.62, 0.62, conc * 0.92, Surface::DryStone, UV);
                    let hh = rg.range(0.62, 0.8);
                    m.beam(v3(p0, g0 + 0.38), v3(p1, g1 + 0.38), 0.52, hh, palette::leaves() * rg.range(0.78, 1.0), Surface::Leaves, 1.0);
                }
            }
        }
    }
}

/// Sample distances over `[r0, r1]`, including polyline vertices, spaced at most `step`.
fn samples(line: &Line, r0: f32, r1: f32, step: f32) -> Vec<f32> {
    let mut keys = vec![r0];
    if r1 > r0 + 1e-4 {
        keys.extend(line.cum.iter().copied().filter(|&c| c > r0 + 0.05 && c < r1 - 0.05));
        keys.push(r1);
    }
    let mut out = Vec::new();
    for w in keys.windows(2) {
        let n = ((w[1] - w[0]) / step).ceil().max(1.0) as usize;
        for j in 0..n {
            out.push(w[0] + (w[1] - w[0]) * j as f32 / n as f32);
        }
    }
    out.push(keys[keys.len() - 1]);
    out
}


#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Colors, Opening};

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
            weathering: 0.5,
        }
    }
    fn flat_terrain() -> Terrain {
        let poly = vec![Vec2::new(-30.0, -30.0), Vec2::new(30.0, -30.0), Vec2::new(30.0, 30.0), Vec2::new(-30.0, 30.0)];
        Terrain::build(1, &[], &[crate::terrain::FlattenRegion { poly, height: Some(0.0), margin: 1.0 }])
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

    #[test]
    fn all_combinations_valid() {
        let t = flat_terrain();
        for mat in [WallKind::DryStone, WallKind::Turf, WallKind::Palisade] {
            for pl in [false, true] {
                for ruin in [false, true] {
                    for fnd in [0.0, 0.6] {
                        for og in [0.0, 1.0] {
                            for h in [3.0, 7.0] {
                                let mut w = wall(vec![Vec2::new(-10.0, 0.0), Vec2::new(0.0, 4.0), Vec2::new(10.0, 0.0)]);
                                (w.kind, w.crest, w.tumbled, w.footing, w.weathering, w.height) = (mat, pl, ruin, fnd, og, h);
                                w.openings = vec![Opening { along: 4.0, kind: OpeningKind::Window }, Opening { along: 16.0, kind: OpeningKind::WideDoor }];
                                let cuts = [Cut { along: 10.0, width: 2.0, height: 2.6 }];
                                let (m, mk) = wall_mesh(&w, &cuts, &t, 7);
                                assert!(m.is_valid() && !m.is_empty(), "{mat:?} {pl} {ruin} {fnd} {og} {h}");
                                assert!(m.positions.iter().flatten().all(|v| v.is_finite()));
                                assert!(!mk.is_empty());
                                assert_eq!((m.clone(), mk.clone()), wall_mesh(&w, &cuts, &t, 7), "deterministic");
                            }
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn brutalist_features() {
        let t = flat_terrain();
        let has = |m: &MeshData, s: Surface| m.surfaces.contains(&(s as u8));
        let mut w = wall(line20());
        w.weathering = 0.0;
        let (clean, _) = wall_mesh(&w, &[], &t, 3);
        assert!(!has(&clean, Surface::Fiber) && !has(&clean, Surface::Leaves) && !has(&clean, Surface::Metal));
        w.weathering = 1.0;
        let (grown, _) = wall_mesh(&w, &[], &t, 3);
        assert!(has(&grown, Surface::Fiber) && tris(&grown) > tris(&clean));
        w.crest = true;
        let (pl, _) = wall_mesh(&w, &[], &t, 3);
        assert!(has(&pl, Surface::Leaves));
        w.crest = false;
        w.tumbled = true;
        let (ru, _) = wall_mesh(&w, &[], &t, 3);
        assert!(has(&ru, Surface::Metal), "rebar");
        w.tumbled = false;
        w.footing = 0.5;
        let (pn, _) = wall_mesh(&w, &[], &t, 3);
        assert!(has(&pn, Surface::Rock));
        w.kind = WallKind::Turf;
        assert!(has(&wall_mesh(&w, &[], &t, 3).0, Surface::Turf));
    }

    #[test]
    fn walkway_on_thick_walls() {
        let t = flat_terrain();
        let top = |m: &MeshData| m.positions.iter().map(|p| p[1]).fold(f32::MIN, f32::max);
        let mut w = wall(line20());
        w.weathering = 0.0;
        w.thickness = 1.0;
        let (walk, mk) = wall_mesh(&w, &[], &t, 1);
        assert!((top(&walk) - (3.0 + 0.18 + UPSTAND_H)).abs() < 1e-3, "upstands {}", top(&walk));
        let perches: Vec<_> = mk.iter().filter(|k| k.kind == MarkerKind::Perch).collect();
        assert!(!perches.is_empty() && perches.iter().all(|k| (k.pos.y - 3.18).abs() < 1e-3), "perch on walk slab");
        w.thickness = 0.6;
        assert!((top(&wall_mesh(&w, &[], &t, 1).0) - 3.18).abs() < 1e-3, "thin wall keeps plain coping");
        w.thickness = 1.0;
        w.crest = true;
        let (pl, _) = wall_mesh(&w, &[], &t, 1);
        assert!(!pl.positions.iter().any(|p| (p[1] - (3.18 + UPSTAND_H)).abs() < 1e-4), "planters replace upstands");
        // portal through a walkway wall: upstands end with caps, mesh stays valid
        let (cut, _) = wall_mesh(&wall(line20()), &[Cut { along: 10.0, width: 2.0, height: 2.0 }], &t, 1);
        assert!(cut.is_valid() && !cut.is_empty());
    }

    #[test]
    fn triangle_budget() {
        let t = flat_terrain();
        let mut w = wall(line20());
        w.crest = true;
        w.footing = 0.5;
        let (m, _) = wall_mesh(&w, &[], &t, 3);
        assert!(tris(&m) <= 3000, "{}", tris(&m));
    }

    #[test]
    fn closed_square() {
        let t = flat_terrain();
        let a = Vec2::new(-5.0, -5.0);
        let w = wall(vec![a, Vec2::new(5.0, -5.0), Vec2::new(5.0, 5.0), Vec2::new(-5.0, 5.0), a]);
        let (m, mk) = wall_mesh(&w, &[], &t, 1);
        assert!(m.is_valid() && !m.is_empty());
        assert!(mk.iter().any(|k| k.kind == MarkerKind::Perch));
        // mitred outer corner reaches roughly half thickness (+ coping) beyond the corner
        let (lo, hi) = m.bounds().unwrap();
        assert!((hi.x - 5.47).abs() < 0.2 && (lo.z + 5.47).abs() < 0.2, "{lo} {hi}");
    }

    #[test]
    fn cut_leaves_passage() {
        let t = flat_terrain();
        let mut w = wall(line20());
        w.height = 4.0;
        let (m, mk) = wall_mesh(&w, &[Cut { along: 10.0, width: 2.0, height: 2.6 }], &t, 5);
        assert!(m.is_valid());
        // square portal: clear right up to the lintel at 2.6
        for p in &m.positions {
            let inside = p[0].abs() < 0.99 && p[1] > 0.05 && p[1] < 2.5;
            assert!(!inside, "vertex in passage: {p:?}");
        }
        assert!(mk.iter().filter(|k| k.kind == MarkerKind::Lantern).count() >= 2);
        assert!(m.positions.iter().any(|p| p[0].abs() < 0.5 && p[1] > 2.4 && p[1] < 2.7));
        // too low a wall: plain gap
        w.height = 1.2;
        let (m2, _) = wall_mesh(&w, &[Cut { along: 10.0, width: 2.0, height: 2.6 }], &t, 5);
        assert!(m2.is_valid());
        assert!(!m2.positions.iter().any(|p| p[0].abs() < 0.99 && p[1] > 0.05));
    }

    #[test]
    fn openings_make_markers() {
        let t = flat_terrain();
        let mut w = wall(vec![Vec2::ZERO, Vec2::new(12.0, 0.0)]);
        w.openings = vec![
            Opening { along: 3.0, kind: OpeningKind::Window },
            Opening { along: 6.0, kind: OpeningKind::Door },
            Opening { along: 9.0, kind: OpeningKind::WideDoor },
        ];
        let (m, mk) = wall_mesh(&w, &[], &t, 2);
        assert!(m.is_valid());
        let n = |k| mk.iter().filter(|x| x.kind == k).count();
        assert_eq!(n(MarkerKind::WindowLight), 1);
        assert_eq!(n(MarkerKind::Door), 2);
        assert!(m.surfaces.contains(&(Surface::Window as u8)));
        assert!(m.surfaces.contains(&(Surface::Door as u8)));
        w.height = 2.0;
        let (_, mk2) = wall_mesh(&w, &[], &t, 2);
        assert!(!mk2.iter().any(|k| k.kind == MarkerKind::WindowLight));
    }

    #[test]
    fn tall_wall_buttresses_and_wood() {
        let t = flat_terrain();
        let mut w = wall(line20());
        let (flat, _) = wall_mesh(&w, &[], &t, 2);
        w.height = 6.0;
        let (tall, _) = wall_mesh(&w, &[], &t, 2);
        assert!(tall.is_valid() && tris(&tall) > tris(&flat));
        w.kind = WallKind::Palisade;
        let (wd, _) = wall_mesh(&w, &[], &t, 2);
        assert!(wd.surfaces.contains(&(Surface::DryStone as u8)) && wd.surfaces.contains(&(Surface::Plank as u8)));
    }

    #[test]
    fn fences_valid_and_gap() {
        let t = flat_terrain();
        let has = |m: &MeshData, s: Surface| m.surfaces.contains(&(s as u8));
        for k in [FenceKind::Driftwood, FenceKind::Wattle, FenceKind::NetLine, FenceKind::Reed] {
            let (m, _) = fence_mesh(&fence(k), &[], &t, 4);
            assert!(m.is_valid() && !m.is_empty(), "{k:?}");
            assert_eq!(m, fence_mesh(&fence(k), &[], &t, 4).0, "{k:?} deterministic");
            let bent = Fence { points: vec![Vec2::new(-5.0, 0.0), Vec2::new(0.0, 3.0), Vec2::new(5.0, -1.0)], kind: k, colors: Colors::default() };
            assert!(fence_mesh(&bent, &[], &t, 4).0.is_valid());
            let (g, _) = fence_mesh(&fence(k), &[Cut { along: 10.0, width: 2.0, height: 2.0 }], &t, 4);
            assert!(g.is_valid() && !g.is_empty());
            assert!(!g.positions.iter().any(|p| p[0].abs() < 0.8), "{k:?} gap");
            assert!(g.positions.iter().any(|p| p[0].abs() < 1.3 && p[0].abs() > 0.8), "{k:?} end posts");
            let want: &[Surface] = match k {
                FenceKind::Driftwood => &[Surface::Timber],
                FenceKind::Wattle => &[Surface::DryStone],
                FenceKind::NetLine => &[Surface::Rock, Surface::Metal],
                FenceKind::Reed => &[Surface::DryStone, Surface::Leaves],
            };
            assert!(want.iter().all(|&s| has(&m, s)), "{k:?} surfaces");
        }
    }

    #[test]
    fn deterministic() {
        let t = flat_terrain();
        let mut w = wall(line20());
        w.tumbled = true;
        w.crest = true;
        assert_eq!(wall_mesh(&w, &[], &t, 9), wall_mesh(&w, &[], &t, 9));
        assert_ne!(wall_mesh(&w, &[], &t, 9).0, wall_mesh(&w, &[], &t, 10).0);
        let f = fence(FenceKind::NetLine);
        assert_eq!(fence_mesh(&f, &[], &t, 1), fence_mesh(&f, &[], &t, 1));
    }

    #[test]
    fn degenerate_inputs() {
        let t = flat_terrain();
        let p = Vec2::new(1.0, 1.0);
        let nan = Vec2::new(f32::NAN, 0.0);
        for pts in [vec![], vec![p], vec![p, p, p], vec![p, nan], vec![nan, nan]] {
            let (m, mk) = wall_mesh(&wall(pts.clone()), &[], &t, 1);
            assert!(m.is_empty() && mk.is_empty());
            for k in [FenceKind::Driftwood, FenceKind::Wattle, FenceKind::NetLine, FenceKind::Reed] {
                let f = Fence { points: pts.clone(), kind: k, colors: Colors::default() };
                assert!(fence_mesh(&f, &[], &t, 1).0.is_empty());
            }
        }
        let mut w = wall(vec![Vec2::ZERO, Vec2::ZERO, Vec2::new(3.0, 0.0), Vec2::new(3.0, 0.0), Vec2::new(3.0, 3.0)]);
        w.thickness = 1.0e9;
        w.crest = true;
        w.footing = 50.0;
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
        let (f, _) = fence_mesh(&fence(FenceKind::Wattle), &cuts, &t, 1);
        assert!(f.is_valid());
    }

    #[test]
    fn walls_follow_hills() {
        use crate::model::{TerrainDab, TerrainOp};
        let hill = TerrainDab { center: Vec2::new(10.0, 0.0), radius: 8.0, op: TerrainOp::Sculpt { amount: 4.0 } };
        let t = Terrain::build(1, &[hill], &[]);
        let (m, _) = wall_mesh(&wall(line20()), &[], &t, 1);
        assert!(m.is_valid());
        // top (with coping) = ground + 3.18 at both ends
        for x in [-9.5_f32, 9.5] {
            let g = t.height(Vec2::new(x, 0.0));
            let mx = m.positions.iter().filter(|p| (p[0] - x).abs() < 0.3).map(|p| p[1]).fold(f32::MIN, f32::max);
            assert!((mx - (g + 3.1)).abs() < 1.2, "x {x} top {mx} ground {g}");
        }
    }

    #[test]
    fn water_arches() {
        use crate::model::{TerrainDab, TerrainOp};
        let pond = TerrainDab { center: Vec2::ZERO, radius: 6.0, op: TerrainOp::Water { depth: 1.5 } };
        let t = Terrain::build(1, &[pond], &[]);
        let c = Vec2::ZERO;
        let level = t.water_level(c).expect("pond has water");
        let mut w = wall(line20());
        w.height = 4.0;
        let (m, _) = wall_mesh(&w, &[], &t, 1);
        assert!(m.is_valid());
        // Arch crowns exist above the water, and some span leaves clear space just above the level.
        let bay_vertices = m.positions.iter().filter(|p| p[0].abs() < 3.0 && p[1] > level + 0.1 && p[1] < level + 0.9).count();
        let wet_x: Vec<f32> = (-100..=100).map(|i| i as f32 * 0.1).filter(|&x| t.water_level(Vec2::new(x, 0.0)).is_some()).collect();
        assert!(wet_x.len() > 15);
        // bay centre: no vertex strictly inside the clear opening column around x=0 at level+0.3..level+0.8
        let inside = m.positions.iter().filter(|p| p[0].abs() < 0.3 && p[1] > level + 0.3 && p[1] < level + 0.8).count();
        assert_eq!(inside, 0, "bay vertices {bay_vertices}");
        // solid wall far outside the pond still reaches full height
        assert!(m.positions.iter().any(|p| p[0] < -9.0 && p[1] > t.height(Vec2::new(-9.5, 0.0)) + 2.5));
    }
}
