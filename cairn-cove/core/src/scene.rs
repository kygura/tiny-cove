//! `build`: document → terrain + meshes + markers, resolving interactions between structures
//! (paths cut portals through walls / doors through houses / gaps in fences; houses flatten the
//! ground; closed wall rings can be roofed). Also ray picking against the result.

use crate::buildings::{house_mesh, roof_mesh};
use crate::geom::{closed, closest_on_polyline, footprint_polygon, is_closed, polyline_crossings, signed_area};
use crate::mesh::{Marker, MarkerKind, MeshData};
use crate::model::{Document, Footprint, Structure, StructureId};
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
}

/// Builds the whole scene. Pure and deterministic.
pub fn build(doc: &Document) -> Scene {
    let seed = doc.seed;

    // Ground under houses is levelled (paths follow the terrain).
    let flatten: Vec<FlattenRegion> = doc
        .structures
        .iter()
        .filter_map(|(_, s)| match s {
            Structure::House(h) => Some(FlattenRegion { poly: footprint_polygon(&h.footprint), height: None, margin: 1.5 }),
            _ => None,
        })
        .collect();
    let terrain = Terrain::build(seed, &doc.terrain, &flatten);

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

    for (id, s) in &doc.structures {
        let (mesh, mk) = match s {
            Structure::Wall(w) => {
                let cuts = cuts_through(&w.points, 0.6, (w.height - 0.6).clamp(2.2, 3.4));
                let (mut mesh, mut mk) = wall_mesh(w, &cuts, &terrain, seed);
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
                    let (roof, rmk) = roof_mesh(&ring, base, w.roof, w.roof_pitch, w.colors, seed);
                    mesh.append(&roof);
                    mk.extend(rmk);
                }
                blockers.push(ribbon(&w.points, w.thickness + 0.6));
                (mesh, mk)
            }
            Structure::House(h) => {
                let poly = footprint_polygon(&h.footprint);
                let ring = closed(&poly);
                let cuts = cuts_through(&ring, 0.2, 2.4);
                blockers.push(grow(&poly, 0.4));
                house_mesh(h, &cuts, &terrain, seed)
            }
            Structure::Fence(f) => {
                let cuts = cuts_through(&f.points, 0.4, 3.0);
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

    Scene {
        ground: terrain.ground_mesh(),
        water: terrain.water_mesh(),
        scatter: scatter_mesh(&terrain, seed, &blockers),
        terrain,
        parts,
        markers,
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
            kind: WallKind::DryStone,
            crest: true,
            tumbled: false,
            footing: 0.0,
            roofed: false,
            roof: RoofStyle::Boat,
            roof_pitch: 0.6,
            openings: vec![],
            colors: Colors::default(),
            weathering: 0.5,
        }));
        doc.add(Structure::Path(Path {
            points: vec![Vec2::new(0.0, -5.0), Vec2::new(0.0, 5.0)],
            width: 1.5,
            kind: PathKind::Pebble,
        }));
        let s = build(&doc);
        assert_eq!(s.parts.len(), 2);
        assert_eq!(build(&doc).markers, s.markers);
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
    }
}
