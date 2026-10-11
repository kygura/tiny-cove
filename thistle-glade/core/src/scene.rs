//! `build`: document → terrain + meshes + markers, resolving interactions between structures
//! (paths cut gates through walls / doors through houses / gaps in fences; houses flatten the
//! ground; closed wall rings can be roofed). Also ray picking against the result.

use crate::buildings::{house_mesh, roof_mesh};
use crate::geom::{closed, closest_on_polyline, footprint_polygon, is_closed, polyline_crossings, signed_area};
use crate::mesh::{Marker, MarkerKind, MeshData};
use crate::model::{Document, Footprint, Structure, StructureId, TerrainDab};
use crate::nature::{placed_mesh, scatter_mesh};
use crate::paths::{bridge_mesh, path_mesh, platform_mesh, stairs_mesh};
use crate::terrain::{FlattenRegion, Terrain};
use crate::walls::{Cut, fence_mesh, wall_mesh};
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
    /// Inputs of the last build, so the next one can reuse what did not change.
    cache: Cache,
}

#[derive(Clone, Debug, Default)]
struct Cache {
    terrain_key: Option<(u64, Vec<TerrainDab>, Vec<FlattenRegion>)>,
    /// Per part: the structure and the cuts it was built with, and its markers.
    parts: Vec<(Structure, Vec<Cut>, Vec<Marker>)>,
    blockers: Vec<Vec<Vec2>>,
}

/// Builds the whole scene. Pure and deterministic.
pub fn build(doc: &Document) -> Scene {
    build_incremental(doc, None)
}

/// Same result as [`build`], reusing terrain, structure meshes and scatter from `prev` where
/// their inputs are unchanged (live previews only touch one structure).
pub fn build_incremental(doc: &Document, prev: Option<&Scene>) -> Scene {
    let seed = doc.seed;

    // Ground under houses and platforms is levelled.
    let flatten: Vec<FlattenRegion> = doc
        .structures
        .iter()
        .filter_map(|(_, s)| match s {
            Structure::House(h) => Some(FlattenRegion { poly: footprint_polygon(&h.footprint), height: None, margin: 1.5 }),
            _ => None,
        })
        .collect();
    let terrain_key = (seed, doc.terrain.clone(), flatten);
    let reuse_terrain = prev.filter(|p| p.cache.terrain_key.as_ref() == Some(&terrain_key));
    let terrain = match reuse_terrain {
        Some(p) => p.terrain.clone(),
        None => Terrain::build(seed, &terrain_key.1, &terrain_key.2),
    };

    let paths: Vec<(&[Vec2], f32)> = doc
        .structures
        .iter()
        .filter_map(|(_, s)| match s {
            Structure::Path(p) if p.points.len() >= 2 => Some((p.points.as_slice(), p.width)),
            _ => None,
        })
        .collect();
    let cuts_through = |host: &[Vec2], extra: f32, height: f32| -> Vec<Cut> {
        let mut cuts: Vec<Cut> = Vec::new();
        for (pts, width) in &paths {
            for (along, _) in polyline_crossings(host, pts) {
                cuts.push(Cut { along, width: (width + extra).max(1.4), height });
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

    let mut part_cache = Vec::new();
    for (id, s) in &doc.structures {
        let cuts = match s {
            Structure::Wall(w) => cuts_through(&w.points, 0.6, (w.height - 0.6).clamp(2.2, 3.4)),
            Structure::House(h) => cuts_through(&closed(&footprint_polygon(&h.footprint)), 0.2, 2.4),
            Structure::Fence(f) => cuts_through(&f.points, 0.4, 3.0),
            _ => Vec::new(),
        };
        match s {
            Structure::Wall(w) => blockers.push(ribbon(&w.points, w.thickness + 0.6)),
            Structure::House(h) => blockers.push(grow(&footprint_polygon(&h.footprint), 0.4)),
            Structure::Fence(f) => blockers.push(ribbon(&f.points, 0.6)),
            Structure::Path(p) => blockers.push(ribbon(&p.points, p.width + 0.3)),
            Structure::Platform(pl) => {
                blockers.push(footprint_polygon(&Footprint::Rect { center: pl.center, half: pl.half, angle: pl.angle }))
            }
            _ => {}
        }
        let cached = reuse_terrain.and_then(|p| {
            let i = p.parts.iter().position(|part| part.id == *id)?;
            let (ps, pc, pm) = p.cache.parts.get(i)?;
            (ps == s && *pc == cuts).then(|| (p.parts[i].mesh.clone(), pm.clone()))
        });
        let (mesh, mk) = match cached {
            Some(hit) => hit,
            None => build_part(s, &cuts, &terrain, seed),
        };
        markers.extend(mk.iter().copied());
        part_cache.push((s.clone(), cuts, mk));
        parts.push(Part { id: *id, mesh });
    }

    // Ducks: a few spots on open water.
    let mut ponds = 0;
    'outer: for zi in (-30..=30).step_by(5) {
        for xi in (-30..=30).step_by(5) {
            let p = Vec2::new(xi as f32, zi as f32);
            if let Some(level) = terrain.water_level(p) {
                if terrain.height(p) < level - 0.3 {
                    markers.push(Marker { kind: MarkerKind::Pond, pos: Vec3::new(p.x, level, p.y), dir: Vec3::Y });
                    ponds += 1;
                    if ponds >= 6 {
                        break 'outer;
                    }
                }
            }
        }
    }

    let (ground, water) = match reuse_terrain {
        Some(p) => (p.ground.clone(), p.water.clone()),
        None => (terrain.ground_mesh(), terrain.water_mesh()),
    };
    let scatter = match reuse_terrain.filter(|p| p.cache.blockers == blockers) {
        Some(p) => p.scatter.clone(),
        None => scatter_mesh(&terrain, seed, &blockers),
    };
    Scene {
        ground,
        water,
        scatter,
        terrain,
        parts,
        markers,
        cache: Cache { terrain_key: Some(terrain_key), parts: part_cache, blockers },
    }
}

/// Mesh and markers of one structure, given the passages cut through it.
fn build_part(s: &Structure, cuts: &[Cut], terrain: &Terrain, seed: u64) -> (MeshData, Vec<Marker>) {
    match s {
        Structure::Wall(w) => {
            let (mut mesh, mut mk) = wall_mesh(w, cuts, terrain, seed);
            if w.roofed && is_closed(&w.points) {
                let mut ring: Vec<Vec2> = w.points[..w.points.len() - 1].to_vec();
                if signed_area(&ring) < 0.0 {
                    ring.reverse();
                }
                let base = ring.iter().map(|p| terrain.height(*p)).fold(f32::MIN, f32::max) + w.height;
                let (roof, rmk) = roof_mesh(&ring, base, w.roof, w.roof_pitch, w.colors, seed);
                mesh.append(&roof);
                mk.extend(rmk);
            }
            (mesh, mk)
        }
        Structure::House(h) => house_mesh(h, cuts, terrain, seed),
        Structure::Fence(f) => fence_mesh(f, cuts, terrain, seed),
        Structure::Path(p) => (path_mesh(p, terrain, seed), Vec::new()),
        Structure::Stairs(st) => stairs_mesh(st, terrain, seed),
        Structure::Platform(pl) => platform_mesh(pl, terrain, seed),
        Structure::Bridge(b) => bridge_mesh(b, terrain, seed),
        Structure::Placed(p) => placed_mesh(p, terrain, seed),
    }
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

    #[test]
    fn path_through_wall_makes_one_cut() {
        let mut doc = Document::new(3);
        doc.add(Structure::Wall(Wall {
            points: vec![Vec2::new(-6.0, 0.0), Vec2::new(6.0, 0.0)],
            height: 4.0,
            thickness: 1.0,
            material: Material::Stone,
            battlements: true,
            ruined: false,
            foundation: 0.0,
            roofed: false,
            roof: RoofStyle::Gable,
            roof_pitch: 0.6,
            openings: vec![],
            colors: Colors::default(),
        }));
        doc.add(Structure::Path(Path {
            points: vec![Vec2::new(0.0, -5.0), Vec2::new(0.0, 5.0)],
            width: 1.5,
            kind: PathKind::Cobble,
        }));
        let s = build(&doc);
        assert_eq!(s.parts.len(), 2);
        assert_eq!(build(&doc).markers, s.markers);
    }

    #[test]
    fn incremental_build_matches_full_build() {
        let a = crate::demo::demo_document(5);
        let full_a = build(&a);
        let mut b = a.clone();
        if let Some((_, Structure::Placed(p))) = b.structures.iter_mut().find(|(_, s)| matches!(s, Structure::Placed(_))) {
            p.at += Vec2::new(1.0, 0.5);
        }
        b.add(Structure::Path(Path { points: vec![Vec2::new(-20.0, 20.0), Vec2::new(-10.0, 24.0)], width: 1.2, kind: PathKind::Dirt }));
        let same = |x: &Scene, y: &Scene| {
            assert_eq!(x.parts.len(), y.parts.len());
            for (p, q) in x.parts.iter().zip(&y.parts) {
                assert_eq!(p.id, q.id);
                assert_eq!(p.mesh, q.mesh, "part {}", p.id);
            }
            assert_eq!(x.markers, y.markers);
            assert_eq!(x.ground, y.ground);
            assert_eq!(x.water, y.water);
            assert_eq!(x.scatter, y.scatter);
        };
        let inc = build_incremental(&b, Some(&full_a));
        same(&inc, &build(&b));
        // A house changes the flattened terrain and the scatter blockers.
        let mut c = b.clone();
        c.add(Structure::House(House {
            footprint: Footprint::Rect { center: Vec2::new(20.0, 20.0), half: Vec2::new(2.0, 2.0), angle: 0.0 },
            wall_height: 3.0,
            roof: RoofStyle::Gable,
            roof_pitch: 0.8,
            material: Material::Stone,
            half_timber: false,
            foundation: 0.0,
            battlements: false,
            openings: vec![],
            colors: Colors::default(),
        }));
        same(&build_incremental(&c, Some(&inc)), &build(&c));
    }

    #[test]
    fn pick_hits_terrain() {
        let doc = Document::new(1);
        let s = build(&doc);
        let h = pick(&doc, &s, Vec3::new(0.0, 50.0, 0.0), -Vec3::Y).expect("hit");
        assert!(h.id.is_none());
    }
}
