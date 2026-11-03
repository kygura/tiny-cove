//! `build`: document → terrain + meshes + markers, resolving interactions between structures
//! (paths cut portals through walls / doors through houses / gaps in fences; houses flatten the
//! ground; closed wall rings can be roofed). Also ray picking against the result.

use crate::buildings::{house_mesh, needs_stilts, roof_mesh};
use crate::geom::{closed, closest_on_polyline, footprint_polygon, is_closed, polyline_crossings, signed_area};
use crate::mesh::{hash2, Marker, MarkerKind, MeshData};
use crate::model::{Document, Footprint, House, Structure, StructureId};
use crate::nature::{placed_mesh, scatter_mesh};
use crate::paths::{bridge_mesh, path_mesh, platform_mesh, stairs_mesh};
use crate::terrain::{FlattenRegion, Terrain};
use crate::walls::{Cut, MAX_GAP, fence_mesh, wall_mesh};
use glam::{Vec2, Vec3};

/// Mesh of one structure.
#[derive(Clone, Debug)]
pub struct Part {
    pub id: StructureId,
    pub mesh: MeshData,
}

/// Everything the renderer needs for one document.
#[derive(Clone, Debug)]
pub struct Scene {
    pub terrain: Terrain,
    pub ground: MeshData,
    pub water: MeshData,
    pub scatter: MeshData,
    pub parts: Vec<Part>,
    pub markers: Vec<Marker>,
}

/// Builds the whole scene. Pure and deterministic.
pub fn build(doc: &Document) -> Scene {
    let seed = doc.seed;

    // Ground under houses is levelled unless the house stands on stilts (explicit, or
    // automatic over water / steep ground judged on the unlevelled ground), whose natural
    // ground no neighbour's levelling margin may touch either. Paths follow it.
    let houses: Vec<&House> = doc.structures.iter().filter_map(|(_, s)| if let Structure::House(h) = s { Some(h) } else { None }).collect();
    let flatten: Vec<FlattenRegion> = if houses.is_empty() {
        Vec::new()
    } else {
        let raw = Terrain::build(seed, &doc.terrain, &[]);
        houses
            .iter()
            .map(|h| {
                let keep = needs_stilts(h, &raw);
                // A kept footprint grows by more than a grid cell so its edges sample raw ground.
                FlattenRegion { poly: footprint_polygon(&h.footprint), height: None, margin: if keep { 1.0 } else { 1.5 }, keep }
            })
            .collect()
    };
    let terrain = Terrain::build(seed, &doc.terrain, &flatten);

    let paths: Vec<(&[Vec2], f32)> = doc
        .structures
        .iter()
        .filter_map(|(_, s)| match s {
            Structure::Path(p) if p.points.len() >= 2 => Some((p.points.as_slice(), p.width)),
            _ => None,
        })
        .collect();
    // `thick`: the host's thickness, for a gap sized to the crossing angle (`None` keeps the
    // square-on width, as for hut doors).
    let cuts_through = |host: &[Vec2], extra: f32, thick: Option<f32>, height: f32| -> Vec<Cut> {
        let mut cuts: Vec<Cut> = Vec::new();
        for (pts, width) in &paths {
            for (along, at) in polyline_crossings(host, pts) {
                let w = (width + extra).max(1.4);
                // A track crossing at angle `a` takes w / sin(a) of the centre line, plus
                // t / tan(a) to get through the thickness `t`.
                let width = thick.filter(|t| t.is_finite()).map_or(w, |t| {
                    let (h, d) = (dir_at(host, along), dir_at(pts, at));
                    let (sin, cos) = (h.perp_dot(d).abs().max(0.05), h.dot(d).abs());
                    (w / sin + t.clamp(0.0, 2.5) * cos / sin).min(MAX_GAP.max(w))
                });
                cuts.push(Cut { along, width, height });
            }
        }
        cuts.sort_by(|a, b| a.along.total_cmp(&b.along));
        // Merge overlapping passages into one.
        let mut merged: Vec<Cut> = Vec::new();
        for c in cuts {
            if let Some(last) = merged.last_mut() {
                if c.along - c.width * 0.5 <= last.along + last.width * 0.5 {
                    let lo = (last.along - last.width * 0.5).min(c.along - c.width * 0.5);
                    let hi = (last.along + last.width * 0.5).max(c.along + c.width * 0.5);
                    *last = Cut { along: (lo + hi) * 0.5, width: hi - lo, height: last.height.max(c.height) };
                    continue;
                }
            }
            merged.push(c);
        }
        merged
    };

    let mut parts = Vec::new();
    let mut markers = Vec::new();
    let mut blockers: Vec<Vec<Vec2>> = Vec::new();

    for (id, s) in &doc.structures {
        let (mesh, mk) = match s {
            Structure::Wall(w) => {
                let mut w = w.clone();
                let mut roof = None;
                if w.roofed && is_closed(&w.points) && w.height.is_finite() && w.height >= 0.3 {
                    let mut ring: Vec<Vec2> = w.points[..w.points.len() - 1].to_vec();
                    if signed_area(&ring) < 0.0 {
                        ring.reverse();
                    }
                    // Same flat top as `wall_mesh`: highest ground-or-water + height (capped at 40).
                    let eff = |p: &Vec2| {
                        let g = terrain.height(*p);
                        let g = if g.is_finite() { g } else { 0.0 };
                        terrain.water_level(*p).filter(|l| l.is_finite()).map_or(g, |l| l.max(g))
                    };
                    let base = ring.iter().map(eff).fold(f32::MIN, f32::max) + w.height.min(40.0);
                    let mut r = roof_mesh(&ring, base, w.roof, w.roof_pitch, w.colors, seed);
                    // Tracks walk under the roof: a low ring is raised (walls and roof together)
                    // until its eaves clear walking height over every track gap.
                    let lift = ROOF_HEADROOM - headroom(&r.0, &ring, &paths, &terrain);
                    if lift > 0.0 && w.height + lift <= 40.0 {
                        w.height += lift;
                        r = roof_mesh(&ring, base + lift, w.roof, w.roof_pitch, w.colors, seed);
                    }
                    roof = Some(r);
                }
                let cuts = cuts_through(&w.points, 0.6, Some(w.thickness), (w.height - 0.6).clamp(2.2, 3.4));
                let (mut mesh, mut mk) = wall_mesh(&w, &cuts, &terrain, seed);
                if let Some((r, rmk)) = roof {
                    mesh.append(&r);
                    mk.extend(rmk);
                }
                blockers.push(ribbon(&w.points, w.thickness + 0.6));
                (mesh, mk)
            }
            Structure::House(h) => {
                let poly = footprint_polygon(&h.footprint);
                let ring = closed(&poly);
                let cuts = cuts_through(&ring, 0.2, None, 2.4);
                blockers.push(grow(&poly, 0.4));
                house_mesh(h, &cuts, &terrain, seed)
            }
            Structure::Fence(f) => {
                let cuts = cuts_through(&f.points, 0.4, Some(FENCE_DEPTH), 3.0);
                blockers.push(ribbon(&f.points, 0.6));
                fence_mesh(f, &cuts, &terrain, seed)
            }
            Structure::Path(p) => {
                blockers.push(ribbon(&p.points, p.width + 0.3));
                (path_mesh(p, &terrain, seed), Vec::new())
            }
            Structure::Stairs(st) => stairs_mesh(st, &terrain, seed),
            Structure::Platform(pl) => {
                blockers.push(footprint_polygon(&Footprint::Rect { center: pl.center, half: pl.half, angle: pl.angle }));
                platform_mesh(pl, &terrain, seed)
            }
            Structure::Bridge(b) => bridge_mesh(b, &terrain, seed),
            Structure::Placed(p) => placed_mesh(p, &terrain, seed),
        };
        // Never hand the renderer a non-finite or out-of-range mesh.
        let (mesh, mk) = if mesh.is_valid() { (mesh, mk) } else { (MeshData::default(), Vec::new()) };
        markers.extend(mk);
        parts.push(Part { id: *id, mesh });
    }

    // Birds swim on open water: lagoons first, then the sea inside the buildable area.
    let mut spots: Vec<(bool, Vec2, f32)> = Vec::new();
    for zi in (-30..=30).step_by(5) {
        for xi in (-30..=30).step_by(5) {
            let p = Vec2::new(xi as f32, zi as f32);
            if let Some(level) = terrain.water_level(p) {
                if terrain.height(p) < level - 0.3 {
                    spots.push((terrain.is_sea(p), p, level));
                }
            }
        }
    }
    spots.sort_by_key(|s| (s.0, hash2(seed, s.1.x, s.1.y)));
    for (_, p, level) in spots.into_iter().take(6) {
        markers.push(Marker { kind: MarkerKind::Pond, pos: Vec3::new(p.x, level, p.y), dir: Vec3::Y });
    }

    Scene {
        ground: terrain.ground_mesh(),
        water: terrain.water_mesh(),
        scatter: scatter_mesh(&terrain, seed, &blockers),
        terrain,
        parts,
        markers,
    }
}

/// Clear height a roofed ring keeps over the tracks that cross it.
const ROOF_HEADROOM: f32 = 2.0;

/// Unit direction of polyline `pts` at distance `s` along it.
fn dir_at(pts: &[Vec2], s: f32) -> Vec2 {
    let mut acc = 0.0;
    let mut d = Vec2::X;
    for w in pts.windows(2) {
        let l = w[0].distance(w[1]);
        if l > 1e-6 {
            d = (w[1] - w[0]) / l;
        }
        acc += l;
        if s <= acc {
            break;
        }
    }
    d
}

/// Least height above the ground of any part of `roof` (eave lips and fascias included) that
/// lies over a track crossing `ring`, its full width (∞ if none): each roof triangle is
/// clipped to each track segment's strip.
fn headroom(roof: &MeshData, ring: &[Vec2], paths: &[(&[Vec2], f32)], terrain: &Terrain) -> f32 {
    let Some((lo, hi)) = roof.bounds() else { return f32::INFINITY };
    // Keeps the part of a convex polygon where `n · xz <= c`.
    let clip = |poly: Vec<Vec3>, n: Vec2, c: f32| -> Vec<Vec3> {
        let f = |p: Vec3| n.dot(Vec2::new(p.x, p.z)) - c;
        let mut out = Vec::new();
        for (i, &p) in poly.iter().enumerate() {
            let q = poly[(i + 1) % poly.len()];
            let (fp, fq) = (f(p), f(q));
            if fp <= 0.0 {
                out.push(p);
            }
            if (fp < 0.0) != (fq < 0.0) && (fp - fq).abs() > 1e-9 {
                out.push(p.lerp(q, fp / (fp - fq)));
            }
        }
        out
    };
    let ring = closed(ring);
    let mut best = f32::INFINITY;
    for (pts, width) in paths {
        if polyline_crossings(&ring, pts).is_empty() {
            continue;
        }
        let half = (width.max(0.0) * 0.5).min(5.0);
        for w in pts.windows(2) {
            let (a, b) = (w[0], w[1]);
            let (smin, smax) = (a.min(b) - Vec2::splat(half), a.max(b) + Vec2::splat(half));
            if smax.x < lo.x || smin.x > hi.x || smax.y < lo.z || smin.y > hi.z || a.distance(b) < 1e-4 {
                continue;
            }
            let d = (b - a).normalize();
            let n = d.perp();
            let planes = [(-d, -d.dot(a)), (d, d.dot(b)), (n, n.dot(a) + half), (-n, -n.dot(a) + half)];
            for tri in roof.indices.chunks_exact(3) {
                let mut poly: Vec<Vec3> = tri.iter().map(|&i| Vec3::from_array(roof.positions[i as usize])).collect();
                for &(pn, c) in &planes {
                    if poly.is_empty() {
                        break;
                    }
                    poly = clip(poly, pn, c);
                }
                for p in poly {
                    best = best.min(p.y - terrain.height(Vec2::new(p.x, p.z)));
                }
            }
        }
    }
    best
}

/// Rectangles around each segment of a polyline (as one polygon per segment would be more
/// precise; a hull of offsets is good enough for blocking scatter). Returned as a closed
/// polygon around the whole polyline: left offsets forward, right offsets back.
fn ribbon(pts: &[Vec2], width: f32) -> Vec<Vec2> {
    if pts.len() < 2 {
        return Vec::new();
    }
    let h = width * 0.5;
    let mut left = Vec::new();
    let mut right = Vec::new();
    for i in 0..pts.len() {
        let d = if i + 1 < pts.len() { pts[i + 1] - pts[i] } else { pts[i] - pts[i - 1] };
        let n = d.normalize_or(Vec2::X).perp() * h;
        left.push(pts[i] + n);
        right.push(pts[i] - n);
    }
    right.reverse();
    left.extend(right);
    left
}

/// Polygon pushed outward from its centroid by `d` metres (approximate offset).
fn grow(poly: &[Vec2], d: f32) -> Vec<Vec2> {
    let c = poly.iter().copied().sum::<Vec2>() / poly.len().max(1) as f32;
    poly.iter().map(|p| *p + (*p - c).normalize_or_zero() * d).collect()
}


/// Result of a pick.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Hit {
    /// `None` = terrain.
    pub id: Option<StructureId>,
    pub point: Vec3,
    pub normal: Vec3,
    /// Distance along the host's centre polyline (walls/fences/paths) or perimeter (houses)
    /// closest to the hit; 0 for other structures and terrain.
    pub along: f32,
}

/// Depth of a windbreak for sizing a track gap by its crossing angle (posts and panels).
const FENCE_DEPTH: f32 = 0.3;

/// How close (metres, on the water plane) a ray must pass to a mesh-less floating prop.
const FLOAT_PICK_RADIUS: f32 = 1.5;

/// Nearest hit of a world-space ray against structures and terrain.
pub fn pick(doc: &Document, scene: &Scene, origin: Vec3, dir: Vec3) -> Option<Hit> {
    let dir = dir.normalize_or_zero();
    let mut best: Option<(f32, Hit)> = None;
    for part in &scene.parts {
        let Some((lo, hi)) = part.mesh.bounds() else { continue };
        if !ray_aabb(origin, dir, lo - Vec3::splat(0.05), hi + Vec3::splat(0.05)) {
            continue;
        }
        if let Some((t, n)) = ray_mesh(origin, dir, &part.mesh) {
            if best.as_ref().is_none_or(|(bt, _)| t < *bt) {
                let point = origin + dir * t;
                let along = doc.get(part.id).map_or(0.0, |s| along_of(s, Vec2::new(point.x, point.z)));
                best = Some((t, Hit { id: Some(part.id), point, normal: n, along }));
            }
        }
    }
    // Props with no mesh (a canoe afloat is only a `Boat` marker): hit them within
    // `FLOAT_PICK_RADIUS` of their spot on the water plane.
    for part in &scene.parts {
        let Some(Structure::Placed(p)) = doc.get(part.id) else { continue };
        if !part.mesh.is_empty() || !p.at.is_finite() || dir.y.abs() < 1e-6 {
            continue;
        }
        let y = scene.terrain.water_level(p.at).unwrap_or_else(|| scene.terrain.height(p.at));
        let t = (y - origin.y) / dir.y;
        let point = origin + dir * t;
        if t > 1e-4 && Vec2::new(point.x, point.z).distance(p.at) <= FLOAT_PICK_RADIUS && best.as_ref().is_none_or(|(bt, _)| t < *bt) {
            best = Some((t, Hit { id: Some(part.id), point, normal: Vec3::Y, along: 0.0 }));
        }
    }
    if let Some((t, n)) = ray_mesh(origin, dir, &scene.ground) {
        if best.as_ref().is_none_or(|(bt, _)| t < *bt) {
            best = Some((t, Hit { id: None, point: origin + dir * t, normal: n, along: 0.0 }));
        }
    }
    best.map(|(_, h)| h).or_else(|| {
        scene.terrain.raycast(origin, dir).map(|p| Hit { id: None, point: p, normal: Vec3::Y, along: 0.0 })
    })
}

/// `along` for a point near a structure.
pub fn along_of(s: &Structure, p: Vec2) -> f32 {
    match s {
        Structure::Wall(w) => closest_on_polyline(&w.points, p).0,
        Structure::Fence(f) => closest_on_polyline(&f.points, p).0,
        Structure::Path(pa) => closest_on_polyline(&pa.points, p).0,
        Structure::House(h) => closest_on_polyline(&closed(&footprint_polygon(&h.footprint)), p).0,
        _ => 0.0,
    }
}

fn ray_aabb(o: Vec3, d: Vec3, lo: Vec3, hi: Vec3) -> bool {
    let inv = d.recip();
    let t0 = (lo - o) * inv;
    let t1 = (hi - o) * inv;
    let tmin = t0.min(t1).max_element();
    let tmax = t0.max(t1).min_element();
    tmax >= tmin.max(0.0)
}

/// Möller–Trumbore over all triangles (both sides). Returns (t, face normal facing the ray).
fn ray_mesh(o: Vec3, d: Vec3, m: &MeshData) -> Option<(f32, Vec3)> {
    let mut best: Option<(f32, Vec3)> = None;
    for tri in m.indices.chunks_exact(3) {
        let a = Vec3::from_array(m.positions[tri[0] as usize]);
        let b = Vec3::from_array(m.positions[tri[1] as usize]);
        let c = Vec3::from_array(m.positions[tri[2] as usize]);
        let e1 = b - a;
        let e2 = c - a;
        let p = d.cross(e2);
        let det = e1.dot(p);
        if det.abs() < 1e-9 {
            continue;
        }
        let inv = 1.0 / det;
        let s = o - a;
        let u = s.dot(p) * inv;
        if !(0.0..=1.0).contains(&u) {
            continue;
        }
        let q = s.cross(e1);
        let v = d.dot(q) * inv;
        if v < 0.0 || u + v > 1.0 {
            continue;
        }
        let t = e2.dot(q) * inv;
        if t > 1e-4 && best.is_none_or(|(bt, _)| t < bt) {
            let mut n = e1.cross(e2).normalize_or(Vec3::Y);
            if n.dot(d) > 0.0 {
                n = -n;
            }
            best = Some((t, n));
        }
    }
    best
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::*;

    /// Flat dry land at 1 m over the whole buildable area.
    fn flat_doc(seed: u64) -> Document {
        let mut doc = Document::new(seed);
        doc.terrain.push(TerrainDab { center: Vec2::ZERO, radius: 60.0, op: TerrainOp::Flatten { height: 1.0, strength: 1.0 } });
        doc
    }

    fn track(doc: &mut Document, a: Vec2, b: Vec2) {
        doc.add(Structure::Path(Path { points: vec![a, b], width: 1.4, kind: PathKind::Shell }));
    }

    /// True if a horizontal ray 1 m above the ground along `a → b` hits structure `id`.
    fn blocked(doc: &Document, s: &Scene, id: StructureId, a: Vec2, b: Vec2) -> bool {
        let d = (b - a).normalize();
        let o = Vec3::new(a.x, s.terrain.height(a) + 1.0, a.y);
        pick(doc, s, o, Vec3::new(d.x, 0.0, d.y)).is_some_and(|h| h.id == Some(id))
    }

    /// Gatepost stones (Rock) of a dyke within `r` of `p`.
    fn posts_near(s: &Scene, id: StructureId, p: Vec2, r: f32) -> usize {
        let m = &s.parts.iter().find(|q| q.id == id).unwrap().mesh;
        m.positions.iter().zip(&m.surfaces).filter(|(q, k)| **k == crate::mesh::Surface::Rock as u8 && Vec2::new(q[0], q[2]).distance(p) < r).count()
    }

    /// A track through a dyke cuts a walk-through gap between gateposts; next to it the dyke
    /// still blocks.
    #[test]
    fn path_through_wall_cuts_a_gap_with_gateposts() {
        let mut doc = flat_doc(3);
        let mut w = dyke(vec![Vec2::new(-10.0, 0.0), Vec2::new(10.0, 0.0)], WallKind::DryStone);
        w.height = 1.5;
        let wall = doc.add(Structure::Wall(w));
        track(&mut doc, Vec2::new(0.0, -5.0), Vec2::new(0.0, 5.0));
        let s = build(&doc);
        assert_eq!(s.parts.len(), 2);
        assert_eq!(build(&doc).markers, s.markers);
        assert!(!blocked(&doc, &s, wall, Vec2::new(0.0, -5.0), Vec2::new(0.0, 5.0)), "gap along the track");
        assert!(blocked(&doc, &s, wall, Vec2::new(4.0, -5.0), Vec2::new(4.0, 5.0)), "solid beside it");
        // Gateposts either side of the gap (track 1.4 m + 0.6 m clearance, then a 0.55 m post).
        assert!(posts_near(&s, wall, Vec2::new(-1.3, 0.0), 0.7) > 0 && posts_near(&s, wall, Vec2::new(1.3, 0.0), 0.7) > 0);
        assert_eq!(posts_near(&s, wall, Vec2::ZERO, 0.9), 0, "no post in the track");
    }

    /// Two tracks crossing a dyke close together both get through (their gaps merge when
    /// there is no room for gateposts between them), whatever their spacing.
    #[test]
    fn close_tracks_through_a_dyke_both_pass() {
        for gap in [2.0, 2.5, 3.0, 3.5, 4.5] {
            let mut doc = flat_doc(3);
            let mut w = dyke(vec![Vec2::new(-10.0, 0.0), Vec2::new(10.0, 0.0)], WallKind::DryStone);
            w.height = 1.5;
            let wall = doc.add(Structure::Wall(w));
            for x in [0.0, gap] {
                track(&mut doc, Vec2::new(x, -5.0), Vec2::new(x, 5.0));
            }
            let s = build(&doc);
            for x in [0.0, gap] {
                assert!(!blocked(&doc, &s, wall, Vec2::new(x, -5.0), Vec2::new(x, 5.0)), "track at {x}, spacing {gap}");
            }
            assert!(blocked(&doc, &s, wall, Vec2::new(-5.0, -5.0), Vec2::new(-5.0, 5.0)));
        }
    }

    /// True if a horizontal ray `lift` m above the ground along `a → b`, shifted `lat` m to
    /// its left, hits structure `id`.
    fn blocked_at(doc: &Document, s: &Scene, id: StructureId, a: Vec2, b: Vec2, lift: f32, lat: f32) -> bool {
        let d = (b - a).normalize();
        let a = a + d.perp() * lat;
        let o = Vec3::new(a.x, s.terrain.height(a) + lift, a.y);
        pick(doc, s, o, Vec3::new(d.x, 0.0, d.y)).is_some_and(|h| h.id == Some(id))
    }

    /// A track crossing a dyke at a shallow angle, or a thick dyke, still gets a gap wide enough
    /// to walk through the whole thickness, across the track's full width.
    #[test]
    fn shallow_crossings_of_thick_dykes_pass() {
        for thickness in [1.0, 1.6, 2.5] {
            for deg in [20.0_f32, 30.0, 45.0] {
                let mut doc = flat_doc(3);
                let mut w = dyke(vec![Vec2::new(-20.0, 0.0), Vec2::new(20.0, 0.0)], WallKind::DryStone);
                w.height = 1.5;
                w.thickness = thickness;
                let wall = doc.add(Structure::Wall(w));
                let d = Vec2::from_angle(deg.to_radians());
                track(&mut doc, -d * 9.0, d * 9.0);
                let s = build(&doc);
                for lift in [0.4, 1.0] {
                    for lat in [-0.49, 0.0, 0.49] {
                        let what = format!("{deg} deg, {thickness} m thick, {lift} m up, {lat} m aside");
                        assert!(!blocked_at(&doc, &s, wall, -d * 9.0, d * 9.0, lift, lat), "{what}");
                        assert!(!blocked_at(&doc, &s, wall, d * 9.0, -d * 9.0, lift, lat), "{what}, back");
                    }
                }
                assert!(blocked(&doc, &s, wall, Vec2::new(-17.0, -5.0), Vec2::new(-17.0, 5.0)), "{deg} {thickness}");
            }
        }
    }

    /// A windbreak crossed at a shallow angle opens wide enough for the track too.
    #[test]
    fn shallow_crossings_of_windbreaks_pass() {
        for kind in [FenceKind::Wattle, FenceKind::Driftwood, FenceKind::NetLine, FenceKind::Reed] {
            for deg in [15.0_f32, 20.0, 45.0] {
                let mut doc = flat_doc(3);
                let fence = doc.add(Structure::Fence(Fence { points: vec![Vec2::new(-20.0, 0.0), Vec2::new(20.0, 0.0)], kind, colors: Colors::default() }));
                let d = Vec2::from_angle(deg.to_radians());
                track(&mut doc, -d * 9.0, d * 9.0);
                let s = build(&doc);
                for lift in [0.4, 1.0] {
                    for lat in [-0.4, 0.0, 0.4] {
                        let what = format!("{kind:?} {deg} deg, {lift} m up, {lat} m aside");
                        assert!(!blocked_at(&doc, &s, fence, -d * 9.0, d * 9.0, lift, lat), "{what}");
                        assert!(!blocked_at(&doc, &s, fence, d * 9.0, -d * 9.0, lift, lat), "{what}, back");
                    }
                }
            }
        }
    }

    /// However many tracks cross a dyke close together, the merged gap is never cut short:
    /// every track gets through, and the dyke stands beyond them.
    #[test]
    fn many_close_tracks_through_a_dyke_all_pass() {
        for (n, pitch) in [(8, 2.6), (12, 1.6), (12, 2.0), (16, 2.0), (16, 2.6)] {
            let mut doc = flat_doc(3);
            let mut w = dyke(vec![Vec2::new(-30.0, 0.0), Vec2::new(30.0, 0.0)], WallKind::DryStone);
            w.height = 1.5;
            let wall = doc.add(Structure::Wall(w));
            let xs: Vec<f32> = (0..n).map(|i| (i as f32 - (n - 1) as f32 * 0.5) * pitch).collect();
            for &x in &xs {
                track(&mut doc, Vec2::new(x, -5.0), Vec2::new(x, 5.0));
            }
            let s = build(&doc);
            for &x in &xs {
                for lift in [0.4, 1.0] {
                    assert!(!blocked_at(&doc, &s, wall, Vec2::new(x, -5.0), Vec2::new(x, 5.0), lift, 0.0), "{n} tracks {pitch} m apart: x={x}");
                }
            }
            assert!(blocked(&doc, &s, wall, Vec2::new(28.5, -5.0), Vec2::new(28.5, 5.0)), "{n} x {pitch}");
        }
    }

    /// A low roofed ring with a track through it raises its walls and roof together until the
    /// eaves clear walking height over the gap; without a track it keeps its height.
    #[test]
    fn roofed_low_ring_clears_walking_height_over_its_track() {
        let v = |x: f32, z: f32| Vec2::new(x, z);
        for kind in [WallKind::Turf, WallKind::DryStone] {
            for roof in [RoofStyle::Thatch, RoofStyle::Turf] {
                // Seam mid-side and at a corner.
                for seam in [0, 3] {
                    let mut pts = vec![v(0.0, -4.0), v(4.0, -4.0), v(4.0, 4.0), v(-4.0, 4.0), v(-4.0, -4.0)];
                    pts.rotate_left(seam);
                    pts.push(pts[0]);
                    let mut w = dyke(pts, kind);
                    w.height = 1.5;
                    w.roofed = true;
                    w.roof = roof;
                    let mut doc = flat_doc(3);
                    let ring = doc.add(Structure::Wall(w));
                    let bare = build(&doc);
                    assert!(blocked_at(&doc, &bare, ring, v(0.0, -9.0), v(0.0, 9.0), 1.0, 0.0));
                    let top = |s: &Scene| s.parts.iter().find(|p| p.id == ring).unwrap().mesh.bounds().unwrap().1.y;
                    for x in [-2.0, -0.4, 0.0, 1.2, 2.0] {
                        track(&mut doc, v(x, -9.0), v(x, 9.0));
                        let s = build(&doc);
                        assert!(top(&s) > top(&bare), "the roof is raised");
                        for lift in [0.4, 1.0, 1.8] {
                            for (a, b) in [(v(x, -9.0), v(x, 9.0)), (v(x, 9.0), v(x, -9.0))] {
                                assert!(!blocked_at(&doc, &s, ring, a, b, lift, 0.0), "{kind:?} {roof:?} seam {seam}: x={x}, {lift} m up");
                            }
                        }
                        assert!(blocked(&doc, &s, ring, v(6.0, 3.0), v(-6.0, 3.0)), "the ring still stands");
                        doc.structures.retain(|(_, st)| !matches!(st, Structure::Path(_)));
                    }
                }
            }
        }
    }

    /// A track crossing right by a dyke's end, or at a closed ring's seam, still gets through.
    #[test]
    fn tracks_at_dyke_ends_and_ring_seams_pass() {
        for (end, w_h) in [(0.2, 1.5), (0.5, 1.5), (1.0, 1.5), (0.3, 3.0)] {
            for flip in [false, true] {
                let mut doc = flat_doc(3);
                let mut w = dyke(vec![Vec2::new(-10.0, 0.0), Vec2::new(10.0, 0.0)], WallKind::DryStone);
                w.height = w_h;
                if flip {
                    w.points.reverse();
                }
                let wall = doc.add(Structure::Wall(w));
                let x = -10.0 + end;
                track(&mut doc, Vec2::new(x, -5.0), Vec2::new(x, 5.0));
                let s = build(&doc);
                assert!(!blocked(&doc, &s, wall, Vec2::new(x, -5.0), Vec2::new(x, 5.0)), "{end} m from the end (flip {flip})");
                assert!(blocked(&doc, &s, wall, Vec2::new(0.0, -5.0), Vec2::new(0.0, 5.0)));
            }
        }
        // A ring whose first point sits mid-side: a track crossing at that seam and the far side.
        for roofed in [false, true] {
            let mut doc = flat_doc(3);
            let v = |x: f32, z: f32| Vec2::new(x, z);
            let mut w = dyke(vec![v(0.0, -4.0), v(4.0, -4.0), v(4.0, 4.0), v(-4.0, 4.0), v(-4.0, -4.0), v(0.0, -4.0)], WallKind::Turf);
            w.roofed = roofed;
            let wall = doc.add(Structure::Wall(w));
            for x in [0.2, -0.4] {
                track(&mut doc, v(x, -9.0), v(x, 9.0));
                let s = build(&doc);
                assert!(!blocked(&doc, &s, wall, v(x, -9.0), v(x, 9.0)), "ring seam, track at {x}, roofed {roofed}");
                assert!(!blocked(&doc, &s, wall, v(x, 9.0), v(x, -9.0)), "ring seam, track at {x}, from the far side");
                assert!(blocked(&doc, &s, wall, v(2.5, -9.0), v(2.5, 9.0)));
                doc.structures.retain(|(_, st)| !matches!(st, Structure::Path(_)));
            }
        }
    }

    use glam::Vec3Swizzles;

    fn dyke(points: Vec<Vec2>, kind: WallKind) -> Wall {
        Wall {
            points,
            height: 2.0,
            thickness: 1.0,
            kind,
            crest: false,
            tumbled: false,
            footing: 0.0,
            roofed: false,
            roof: RoofStyle::Thatch,
            roof_pitch: 0.6,
            openings: vec![],
            colors: Colors::default(),
            weathering: 0.0,
        }
    }

    fn hut(footprint: Footprint, wall_height: f32, roof: RoofStyle, material: Material) -> House {
        House {
            footprint,
            wall_height,
            roof,
            roof_pitch: 0.8,
            material,
            carved: false,
            hangings: false,
            stilts: 0.0,
            openings: vec![],
            colors: Colors::default(),
            weathering: 0.0,
        }
    }

    fn prop(at: Vec2, kind: PropKind) -> Structure {
        Structure::Placed(Placed { at, angle: 0.3, scale: 1.0, kind, colors: Colors::default() })
    }

    /// Every variant of every structure kind (on dry land) builds a non-empty, finite mesh.
    #[test]
    fn every_variant_builds_a_valid_mesh() {
        let mut doc = Document::new(11);
        // Raise the whole buildable area to dry land first.
        for i in -3..=3 {
            for j in -3..=3 {
                let center = Vec2::new(i as f32 * 8.0, j as f32 * 8.0);
                doc.terrain.push(TerrainDab { center, radius: 7.5, op: TerrainOp::Flatten { height: 1.0, strength: 1.0 } });
            }
        }
        let v = |x: f32, z: f32| Vec2::new(x, z);
        let at = |x: f32, z: f32| Vec3::new(x, 2.0, z);
        for (i, kind) in [WallKind::DryStone, WallKind::Turf, WallKind::Palisade].into_iter().enumerate() {
            for (j, (crest, tumbled, footing)) in [(false, false, 0.0), (true, true, 0.5)].into_iter().enumerate() {
                let z = -20.0 + 3.0 * (2 * i + j) as f32;
                let mut w = dyke(vec![v(-24.0, z), v(-18.0, z)], kind);
                (w.crest, w.tumbled, w.footing) = (crest, tumbled, footing);
                doc.add(Structure::Wall(w));
            }
        }
        let roofs = [RoofStyle::Thatch, RoofStyle::Turf, RoofStyle::Hide, RoofStyle::Boat];
        let mats = [Material::Wattle, Material::Plank, Material::Stone];
        for (i, roof) in roofs.into_iter().enumerate() {
            for (j, material) in mats.into_iter().enumerate() {
                let c = v(-12.0 + 7.0 * j as f32, -20.0 + 7.0 * i as f32);
                let mut h = hut(Footprint::Rect { center: c, half: v(2.0, 1.5), angle: 0.2 }, 2.5, roof, material);
                (h.carved, h.hangings, h.stilts) = (j == 0, j == 1, if j == 2 { 1.0 } else { 0.0 });
                doc.add(Structure::House(h));
            }
            // Roundhouse and beacon.
            doc.add(Structure::House(hut(Footprint::Round { center: v(10.0, -20.0 + 7.0 * i as f32), radius: 2.0 }, 2.0, roof, Material::Wattle)));
            doc.add(Structure::House(hut(Footprint::Round { center: v(16.0, -20.0 + 7.0 * i as f32), radius: 1.5 }, 6.0, roof, Material::Stone)));
        }
        for (i, kind) in [FenceKind::Wattle, FenceKind::Driftwood, FenceKind::NetLine, FenceKind::Reed].into_iter().enumerate() {
            let z = 8.0 + 1.5 * i as f32;
            doc.add(Structure::Fence(Fence { points: vec![v(-24.0, z), v(-18.0, z)], kind, colors: Colors::default() }));
        }
        let paths = [PathKind::Shell, PathKind::Pebble, PathKind::Flagstone, PathKind::Boardwalk, PathKind::Stepping];
        for (i, kind) in paths.into_iter().enumerate() {
            let x = -14.0 + 2.5 * i as f32;
            doc.add(Structure::Path(Path { points: vec![v(x, 8.0), v(x, 14.0)], width: 1.4, kind }));
        }
        for (i, kind) in [StairKind::Stone, StairKind::Log, StairKind::RopeLadder].into_iter().enumerate() {
            let x = 0.0 + 2.5 * i as f32;
            doc.add(Structure::Stairs(Stairs { from: at(x, 8.0), to: at(x, 11.0) + Vec3::Y * 1.5, width: 1.0, kind, colors: Colors::default() }));
        }
        doc.add(Structure::Platform(Platform { center: v(10.0, 10.0), half: v(2.0, 1.5), angle: 0.0, height: 1.0, colors: Colors::default() }));
        let bridges = [
            BridgeKind::Rope { railing: 0 },
            BridgeKind::Rope { railing: 1 },
            BridgeKind::Rope { railing: 2 },
            BridgeKind::Rope { railing: 3 },
            BridgeKind::Log,
            BridgeKind::Causeway,
        ];
        let hangings = [Hanging::None, Hanging::Lanterns, Hanging::Pennants];
        for (i, kind) in bridges.into_iter().enumerate() {
            let z = 16.0 + 2.5 * i as f32;
            let hanging = hangings[i % hangings.len()];
            doc.add(Structure::Bridge(Bridge { a: at(-24.0, z), b: at(-17.0, z), width: 1.4, kind, hanging, colors: Colors::default() }));
        }
        let trees = [TreeKind::SeaPine, TreeKind::Palm, TreeKind::Cypress, TreeKind::Fig, TreeKind::Snag];
        for (i, k) in trees.into_iter().enumerate() {
            doc.add(prop(v(-10.0 + 4.0 * i as f32, 18.0), PropKind::Tree(k)));
        }
        use ClutterKind::*;
        let clutter = [Canoe, DryingRack, Firepit, Cairn, StandingStone, CarvedPole, NetRack, Baskets, LanternPost, Pennant, Rock, Bush, CrabPots, Woodpile];
        for (i, k) in clutter.into_iter().enumerate() {
            doc.add(prop(v(-10.0 + 2.5 * i as f32, 24.0), PropKind::Clutter(k)));
        }
        let scene = build(&doc);
        assert_eq!(scene.parts.len(), doc.structures.len());
        for (part, (_, s)) in scene.parts.iter().zip(&doc.structures) {
            assert!(part.mesh.is_valid() && !part.mesh.is_empty(), "{s:?}");
            // Vertex alpha is the flutter weight: only cloth, nets and hanging fish move.
            let cloth = match s {
                Structure::Placed(p) => matches!(p.kind, PropKind::Clutter(Pennant | DryingRack)),
                Structure::Fence(f) => f.kind == FenceKind::NetLine,
                Structure::Bridge(b) => b.hanging == Hanging::Pennants && matches!(b.kind, BridgeKind::Rope { .. }),
                _ => false,
            };
            let lo = part.mesh.colors.iter().map(|c| c[3]).fold(1.0, f32::min);
            assert_eq!(lo < 1.0, cloth, "{s:?} flutter {lo}");
        }
        for (part, (_, s)) in scene.parts.iter().zip(&doc.structures) {
            if matches!(s, Structure::Placed(p) if p.kind == PropKind::Clutter(Pennant)) || matches!(s, Structure::Bridge(b) if b.hanging == Hanging::Pennants && matches!(b.kind, BridgeKind::Rope { .. })) {
                let lo = part.mesh.colors.iter().map(|c| c[3]).fold(1.0, f32::min);
                assert!(lo < 0.05, "pennant tips flutter freely: {s:?} {lo}");
            }
        }
        assert!(scene.markers.iter().all(|m| m.pos.is_finite() && m.dir.is_finite()));
    }

    /// Firepits and beacons burn; a canoe on water floats; a jetty over the sea moors canoes.
    #[test]
    fn fire_and_boat_markers() {
        let count = |s: &Scene, k: MarkerKind| s.markers.iter().filter(|m| m.kind == k).count();
        let mut doc = Document::new(4);
        doc.add(prop(Vec2::new(0.0, 0.0), PropKind::Clutter(ClutterKind::Firepit)));
        let s = build(&doc);
        assert_eq!(count(&s, MarkerKind::Fire), 1);

        let mut doc = Document::new(4);
        doc.add(Structure::House(hut(Footprint::Round { center: Vec2::ZERO, radius: 1.5 }, 6.0, RoofStyle::Thatch, Material::Stone)));
        doc.add(Structure::House(hut(Footprint::Round { center: Vec2::new(10.0, 0.0), radius: 3.0 }, 2.5, RoofStyle::Thatch, Material::Wattle)));
        let s = build(&doc);
        let fires: Vec<&Marker> = s.markers.iter().filter(|m| m.kind == MarkerKind::Fire).collect();
        assert_eq!(fires.len(), 1, "only the beacon burns, not the low roundhouse");
        assert!(fires[0].pos.xz().length() < 0.1 && fires[0].pos.y > 6.0);

        // Canoes: dug into the sea vs on land.
        let sea = Vec2::new(-12.0, 6.0);
        let mut doc = Document::new(4);
        doc.terrain.push(TerrainDab { center: sea, radius: 7.0, op: TerrainOp::Sculpt { amount: -6.0 } });
        let afloat = doc.add(prop(sea, PropKind::Clutter(ClutterKind::Canoe)));
        let ashore = doc.add(prop(Vec2::new(12.0, 6.0), PropKind::Clutter(ClutterKind::Canoe)));
        let s = build(&doc);
        assert!(s.terrain.is_sea(sea));
        let boats: Vec<&Marker> = s.markers.iter().filter(|m| m.kind == MarkerKind::Boat).collect();
        assert_eq!(boats.len(), 1);
        assert!((boats[0].pos.y - SEA_LEVEL).abs() < 1e-6 && (boats[0].pos.xz() - sea).length() < 1e-4);
        assert!((boats[0].dir.length() - 1.0).abs() < 1e-4);
        let mesh = |id| &s.parts.iter().find(|p| p.id == id).unwrap().mesh;
        assert!(mesh(afloat).is_empty(), "a floating canoe is a marker, not a static mesh");
        assert!(!mesh(ashore).is_empty());

        // A jetty out over the sea gets canoes alongside; one on land gets none.
        let mut doc = Document::new(4);
        doc.terrain.push(TerrainDab { center: sea, radius: 9.0, op: TerrainOp::Sculpt { amount: -6.0 } });
        doc.add(Structure::Platform(Platform { center: sea, half: Vec2::new(2.0, 1.0), angle: 0.4, height: 7.0, colors: Colors::default() }));
        doc.add(Structure::Platform(Platform { center: Vec2::new(12.0, -8.0), half: Vec2::new(2.0, 1.0), angle: 0.0, height: 1.0, colors: Colors::default() }));
        let s = build(&doc);
        assert!((1..=4).contains(&count(&s, MarkerKind::Boat)));
        assert!(s.markers.iter().filter(|m| m.kind == MarkerKind::Boat).all(|m| (m.pos.xz() - sea).length() < 4.0));
    }

    #[test]
    fn pick_hits_terrain() {
        let doc = Document::new(1);
        let s = build(&doc);
        let h = pick(&doc, &s, Vec3::new(0.0, 50.0, 0.0), -Vec3::Y).expect("hit");
        assert!(h.id.is_none());
        assert!((h.point.y - s.terrain.height(Vec2::ZERO)).abs() < 0.05);
        // Out over the sea the ray still finds the seabed.
        let off = s.terrain.plan().cove * 30.0;
        let h = pick(&doc, &s, Vec3::new(off.x, 50.0, off.y), -Vec3::Y).expect("hit");
        assert!(h.id.is_none() && h.point.y < SEA_LEVEL);
    }

    /// A canoe afloat has no mesh (only a Boat marker), yet Erase / Dye can still click it.
    #[test]
    fn pick_hits_floating_canoe() {
        let mut doc = Document::new(4);
        let sea = Vec2::new(-6.0, 6.0);
        doc.terrain.push(TerrainDab { center: sea, radius: 6.0, op: TerrainOp::Sculpt { amount: -6.0 } });
        let canoe = doc.add(prop(sea, PropKind::Clutter(ClutterKind::Canoe)));
        let s = build(&doc);
        assert!(s.parts.iter().find(|p| p.id == canoe).unwrap().mesh.is_empty());
        let dir = Vec3::new(-1.0, -1.2, -1.0).normalize();
        let shot = |at: Vec2| pick(&doc, &s, Vec3::new(at.x, SEA_LEVEL, at.y) - dir * 60.0, dir);
        for off in [Vec2::ZERO, Vec2::new(1.0, 0.0), Vec2::new(0.0, -1.2)] {
            let h = shot(sea + off).expect("hit");
            assert_eq!(h.id, Some(canoe), "offset {off}");
            assert!((h.point.y - SEA_LEVEL).abs() < 1e-3);
        }
        let miss = shot(sea + Vec2::new(2.0, 0.0)).expect("hit");
        assert_ne!(miss.id, Some(canoe));
        // A structure in front of the canoe (on the shore, towards the camera) still wins.
        let mut doc2 = doc.clone();
        let front = sea - dir.xz().normalize() * 6.0;
        let hut_id = doc2.add(Structure::House(hut(Footprint::Rect { center: front, half: Vec2::new(1.5, 1.5), angle: 0.0 }, 6.0, RoofStyle::Thatch, Material::Stone)));
        let s2 = build(&doc2);
        let h = pick(&doc2, &s2, Vec3::new(sea.x, SEA_LEVEL, sea.y) - dir * 60.0, dir).expect("hit");
        assert_eq!(h.id, Some(hut_id));
    }

    /// Ground under a hut is levelled, except under a stilted hut (over the water, steep
    /// ground or explicit stilts), which keeps the natural ground; beacons always level.
    #[test]
    fn stilted_huts_are_not_flattened() {
        let mut doc = Document::new(7);
        let bare = Terrain::build(7, &[], &[]);
        let cove = bare.plan().cove;
        let shore = (0..200).map(|i| i as f32 * 0.25).find(|r| bare.is_sea(cove * *r)).unwrap();
        let rect = |c: Vec2| Footprint::Rect { center: c, half: Vec2::new(2.2, 1.6), angle: cove.to_angle() };
        let over_water = rect(cove * (shore + 0.5));
        let on_land = rect(-cove * 4.0);
        let mut explicit = hut(rect(cove.perp() * 7.0), 2.4, RoofStyle::Turf, Material::Plank);
        explicit.stilts = 1.2;
        doc.add(Structure::House(hut(over_water, 2.4, RoofStyle::Thatch, Material::Wattle)));
        doc.add(Structure::House(hut(on_land, 2.4, RoofStyle::Hide, Material::Wattle)));
        doc.add(Structure::House(explicit));
        let s = build(&doc);
        let raw = Terrain::build(7, &[], &[]);
        let ring = |fp: Footprint| -> Vec<Vec2> {
            let poly = footprint_polygon(&fp);
            let c = poly.iter().copied().sum::<Vec2>() / poly.len() as f32;
            poly.iter().flat_map(|p| [*p, p.lerp(c, 0.5)]).chain([c]).collect()
        };
        for (_, st) in &doc.structures {
            let Structure::House(h) = st else { continue };
            let pts = ring(h.footprint);
            if needs_stilts(h, &raw) {
                assert!(pts.iter().all(|p| s.terrain.height(*p) == raw.height(*p)), "{:?}", h.footprint);
            } else {
                let hs: Vec<f32> = pts.iter().map(|p| s.terrain.height(*p)).collect();
                let (lo, hi) = hs.iter().fold((f32::MAX, f32::MIN), |(a, b), y| (a.min(*y), b.max(*y)));
                assert!(hi - lo < 0.05, "levelled {lo}..{hi}");
            }
        }
        assert!(needs_stilts(&hut(over_water, 2.4, RoofStyle::Thatch, Material::Wattle), &raw));
        assert!(!needs_stilts(&hut(on_land, 2.4, RoofStyle::Thatch, Material::Wattle), &raw));
        // Steep ground: the headland's cliff edge.
        let head = raw.plan().headland;
        let cliff = (40..150).map(|i| head * i as f32 * 0.25).find(|p| raw.normal(*p).y < 0.5).unwrap();
        assert!(needs_stilts(&hut(rect(cliff), 2.4, RoofStyle::Thatch, Material::Wattle), &raw));
        let beacon = hut(Footprint::Round { center: cliff, radius: 1.5 }, 6.0, RoofStyle::Thatch, Material::Stone);
        assert!(!needs_stilts(&beacon, &raw));
    }

    /// Site sample points of a footprint (as `buildings::site` takes them).
    fn site_points(fp: Footprint) -> Vec<Vec2> {
        let poly = footprint_polygon(&fp);
        let c = poly.iter().copied().sum::<Vec2>() / poly.len() as f32;
        let n = poly.len();
        (0..n).flat_map(|i| [poly[i], (poly[i] + poly[(i + 1) % n]) * 0.5, (poly[i] + c) * 0.5]).chain([c]).collect()
    }

    /// Huts side by side on a slope each keep level ground under their own floor (whatever
    /// order they were placed in), and a stilted neighbour's natural ground is untouched.
    #[test]
    fn neighbouring_huts_keep_their_own_ground() {
        let bare = Terrain::build(7, &[], &[]);
        let base = -bare.plan().cove * 6.0;
        let mut doc = Document::new(7);
        doc.terrain.push(TerrainDab { center: base + Vec2::new(16.0, 0.0), radius: 30.0, op: TerrainOp::Sculpt { amount: 2.0 } });
        let raw = Terrain::build(7, &doc.terrain, &[]);
        let rect = |x: f32| Footprint::Rect { center: base + Vec2::new(x, 0.0), half: Vec2::new(2.0, 1.5), angle: 0.0 };
        let range = |t: &Terrain, fp: Footprint| {
            let hs: Vec<f32> = site_points(fp).iter().map(|p| t.height(*p)).collect();
            hs.iter().fold((f32::MAX, f32::MIN), |(a, b), y| (a.min(*y), b.max(*y)))
        };
        // Two plain huts 0.2 m apart, then the second one on stilts 0.5 m away.
        for (gap, stilts) in [(0.2, 0.0), (0.5, 1.0)] {
            let (fa, fb) = (rect(0.0), rect(4.0 + gap));
            let mut b = hut(fb, 2.4, RoofStyle::Thatch, Material::Wattle);
            b.stilts = stilts;
            let mut doc = doc.clone();
            doc.add(Structure::House(hut(fa, 2.4, RoofStyle::Thatch, Material::Wattle)));
            doc.add(Structure::House(b.clone()));
            let s = build(&doc);
            let (lo, hi) = range(&raw, fa);
            assert!(hi - lo > 0.15 && !raw.is_sea(base), "a real slope: {lo}..{hi}");
            let (alo, ahi) = range(&s.terrain, fa);
            // The first hut's floor stands on (and clears) its own ground.
            let floor = |id| {
                let m = &s.parts.iter().find(|p| p.id == id).unwrap().mesh;
                m.positions.iter().zip(&m.surfaces).filter(|(_, s)| **s == crate::mesh::Surface::Dirt as u8).map(|(p, _)| p[1]).fold(f32::MIN, f32::max)
            };
            let ids: Vec<StructureId> = doc.structures.iter().map(|(id, _)| *id).collect();
            let fl = floor(ids[ids.len() - 2]);
            assert!(fl > ahi && fl < alo + 0.25, "floor {fl} over ground {alo}..{ahi} (gap {gap})");
            if stilts > 0.0 {
                assert!(needs_stilts(&b, &raw));
                assert!(site_points(fb).iter().all(|p| s.terrain.height(*p) == raw.height(*p)), "stilted ground untouched");
            } else {
                // Side by side, both stand on one level terrace.
                for fp in [fa, fb] {
                    let (lo, hi) = range(&s.terrain, fp);
                    assert!(hi - lo < 0.08, "level: {lo}..{hi}");
                }
                assert!((floor(ids[ids.len() - 1]) - fl).abs() < 0.05);
            }
        }
    }

    /// Swimming birds go to lagoons first, then to the open sea.
    #[test]
    fn pond_markers_prefer_lagoons() {
        let mut doc = Document::new(3);
        doc.terrain.push(TerrainDab { center: Vec2::new(5.0, 5.0), radius: 4.0, op: TerrainOp::Water { depth: 1.2 } });
        let s = build(&doc);
        let ponds: Vec<&Marker> = s.markers.iter().filter(|m| m.kind == MarkerKind::Pond).collect();
        assert_eq!(ponds.len(), 6);
        let lv = s.terrain.water_level(Vec2::new(5.0, 5.0)).unwrap();
        assert!(lv > SEA_LEVEL + 0.1);
        assert!((ponds[0].pos.y - lv).abs() < 1e-5 && ponds[0].pos.xz() == Vec2::new(5.0, 5.0));
        assert!(ponds[1..].iter().all(|m| m.pos.y == SEA_LEVEL && s.terrain.is_sea(m.pos.xz())));
        assert_eq!(build(&doc).markers, s.markers);
    }
}
