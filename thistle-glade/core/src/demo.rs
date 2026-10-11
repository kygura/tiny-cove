//! A scripted demo glade that uses every primitive (for `--demo-shot` and tests).

use crate::model::*;
use glam::{Vec2, Vec3};

fn v(x: f32, z: f32) -> Vec2 {
    Vec2::new(x, z)
}

fn colors(roof: u8, wall: u8, trim: u8) -> Colors {
    Colors { roof, wall, trim }
}

fn wall(points: Vec<Vec2>, height: f32) -> Wall {
    Wall {
        points,
        height,
        thickness: 1.0,
        material: Material::Stone,
        battlements: true,
        ruined: false,
        foundation: 0.6,
        roofed: false,
        roof: RoofStyle::Gable,
        roof_pitch: 0.7,
        openings: Vec::new(),
        colors: Colors::default(),
    }
}

fn house(footprint: Footprint, wall_height: f32, roof: RoofStyle, material: Material, half_timber: bool, c: Colors) -> House {
    House {
        footprint,
        wall_height,
        roof,
        roof_pitch: 0.8,
        material,
        half_timber,
        foundation: 0.4,
        battlements: false,
        openings: Vec::new(),
        colors: c,
    }
}

fn placed(at: Vec2, angle: f32, kind: PropKind) -> Structure {
    Structure::Placed(Placed { at, angle, scale: 1.0, kind, colors: colors(0, 0, 1) })
}

/// The demo document. Deterministic.
pub fn demo_document(seed: u64) -> Document {
    let mut d = Document::new(seed);

    // Terrain: a pond with a stream, a raised knoll for the keep, painted meadows.
    for i in 0..10 {
        let t = i as f32 / 9.0;
        d.terrain.push(TerrainDab { center: v(6.0 + t * 14.0, 8.0 + t * 6.0), radius: 3.2, op: TerrainOp::Water { depth: 1.2 } });
    }
    d.terrain.push(TerrainDab { center: v(16.0, 14.0), radius: 6.0, op: TerrainOp::Water { depth: 1.5 } });
    d.terrain.push(TerrainDab { center: v(-14.0, -12.0), radius: 9.0, op: TerrainOp::Sculpt { amount: 2.5 } });
    for (c, layer) in [
        (v(-4.0, 16.0), PaintLayer::Flowers),
        (v(-20.0, 10.0), PaintLayer::Flowers),
        (v(22.0, -10.0), PaintLayer::Grain),
        (v(18.0, -18.0), PaintLayer::Grain),
        (v(-24.0, 22.0), PaintLayer::Bushes),
        (v(4.0, -4.0), PaintLayer::Dirt),
    ] {
        d.terrain.push(TerrainDab { center: c, radius: 6.0, op: TerrainOp::Paint { layer, strength: 1.0 } });
    }

    // Curtain wall around the keep knoll with a tower at each corner.
    let ring = vec![v(-22.0, -20.0), v(-6.0, -20.0), v(-6.0, -4.0), v(-22.0, -4.0), v(-22.0, -20.0)];
    let mut curtain = wall(ring, 4.5);
    curtain.openings.push(Opening { along: 6.0, kind: OpeningKind::Window });
    d.add(Structure::Wall(curtain));
    for (i, c) in [v(-22.0, -20.0), v(-6.0, -20.0), v(-6.0, -4.0), v(-22.0, -4.0)].into_iter().enumerate() {
        let roof = if i % 2 == 0 { RoofStyle::Cone } else { RoofStyle::Flat };
        let mut t = house(Footprint::Round { center: c, radius: 2.2 }, 8.0, roof, Material::Stone, false, colors(1, 0, 0));
        t.battlements = roof == RoofStyle::Flat;
        d.add(Structure::House(t));
    }
    // Keep inside.
    d.add(Structure::House(house(
        Footprint::Rect { center: v(-14.0, -12.0), half: v(3.5, 3.0), angle: 0.0 },
        9.0,
        RoofStyle::Hip,
        Material::Stone,
        false,
        colors(1, 4, 0),
    )));

    // A ruined wall on the far side.
    let mut ruin = wall(vec![v(10.0, -26.0), v(24.0, -22.0)], 3.5);
    ruin.ruined = true;
    ruin.battlements = false;
    d.add(Structure::Wall(ruin));

    // Village: houses of different materials and roofs.
    d.add(Structure::House(house(Footprint::Rect { center: v(2.0, 4.0), half: v(3.0, 2.2), angle: 0.3 }, 5.5, RoofStyle::Gable, Material::Plaster, true, colors(0, 0, 0))));
    d.add(Structure::House(house(Footprint::Rect { center: v(-6.0, 8.0), half: v(2.5, 2.0), angle: -0.2 }, 3.2, RoofStyle::Gable, Material::Stone, false, colors(4, 0, 1))));
    d.add(Structure::House(house(Footprint::Rect { center: v(10.0, -6.0), half: v(2.8, 2.4), angle: 0.8 }, 4.0, RoofStyle::Hip, Material::Wood, false, colors(3, 0, 2))));
    d.add(Structure::House(house(Footprint::Rect { center: v(-2.0, -8.0), half: v(2.0, 1.6), angle: 1.2 }, 1.0, RoofStyle::Gable, Material::Wood, false, colors(5, 0, 3))));
    d.add(Structure::House(house(Footprint::Round { center: v(16.0, 2.0), radius: 2.0 }, 3.5, RoofStyle::Cone, Material::Plaster, false, colors(2, 2, 0))));

    // Roofed courtyard wall ring (roof tool).
    let mut barn = wall(vec![v(20.0, -8.0), v(26.0, -8.0), v(26.0, -2.0), v(20.0, -2.0), v(20.0, -8.0)], 3.0);
    barn.material = Material::Wood;
    barn.battlements = false;
    barn.roofed = true;
    barn.colors = colors(4, 0, 1);
    d.add(Structure::Wall(barn));

    // Paths: cobble road through the curtain wall gate, dirt lanes.
    d.add(Structure::Path(Path { points: vec![v(-14.0, -12.0), v(-10.0, 0.0), v(-2.0, 6.0), v(6.0, 4.0), v(14.0, 0.0)], width: 2.2, kind: PathKind::Cobble }));
    d.add(Structure::Path(Path { points: vec![v(-4.0, 6.0), v(-6.0, 14.0), v(-4.0, 24.0)], width: 1.4, kind: PathKind::Dirt }));
    d.add(Structure::Path(Path { points: vec![v(6.0, 4.0), v(10.0, 12.0), v(12.0, 20.0)], width: 1.6, kind: PathKind::Flagstone }));
    d.add(Structure::Path(Path { points: vec![v(14.0, 0.0), v(20.0, -2.0)], width: 1.8, kind: PathKind::Setts }));

    // Fences around the grain field.
    d.add(Structure::Fence(Fence { points: vec![v(16.0, -14.0), v(28.0, -14.0), v(28.0, -24.0)], kind: FenceKind::WoodRail, colors: colors(0, 0, 1) }));
    d.add(Structure::Fence(Fence { points: vec![v(-10.0, 12.0), v(-16.0, 14.0), v(-20.0, 20.0)], kind: FenceKind::Picket, colors: colors(0, 0, 2) }));
    d.add(Structure::Fence(Fence { points: vec![v(-26.0, 4.0), v(-18.0, 4.0)], kind: FenceKind::Stone, colors: Colors::default() }));
    d.add(Structure::Fence(Fence { points: vec![v(0.0, 20.0), v(6.0, 24.0)], kind: FenceKind::Wattle, colors: Colors::default() }));

    // Bridges over the stream and stairs onto the curtain wall.
    d.add(Structure::Bridge(Bridge { a: Vec3::new(8.0, 1.2, 14.0), b: Vec3::new(13.0, 1.2, 7.0), width: 1.6, kind: BridgeKind::Stone, hanging: Hanging::None, colors: Colors::default() }));
    d.add(Structure::Bridge(Bridge { a: Vec3::new(13.0, 2.0, 21.0), b: Vec3::new(21.0, 2.0, 10.0), width: 1.2, kind: BridgeKind::Rope { railing: 1 }, hanging: Hanging::Lanterns, colors: colors(0, 0, 3) }));
    d.add(Structure::Bridge(Bridge { a: Vec3::new(3.0, 1.0, 10.0), b: Vec3::new(7.0, 1.0, 16.0), width: 1.4, kind: BridgeKind::Wood, hanging: Hanging::None, colors: colors(0, 0, 1) }));
    d.add(Structure::Stairs(Stairs { from: Vec3::new(-4.0, 0.5, -10.0), to: Vec3::new(-5.6, 4.8, -14.0), width: 1.0, kind: StairKind::Wood, colors: colors(0, 0, 0) }));
    d.add(Structure::Stairs(Stairs { from: Vec3::new(-24.0, 0.5, -8.0), to: Vec3::new(-22.6, 4.6, -10.0), width: 0.6, kind: StairKind::Ladder, colors: colors(0, 0, 1) }));
    d.add(Structure::Platform(Platform { center: v(22.0, 18.0), half: v(2.0, 1.5), angle: 0.4, height: 1.6, colors: colors(0, 0, 1) }));

    // Trees and clutter.
    for (i, (x, z)) in [(-26.0, 12.0), (-20.0, 26.0), (24.0, 24.0), (26.0, 6.0), (-12.0, 22.0), (0.0, -24.0), (6.0, -20.0)].into_iter().enumerate() {
        let k = [TreeKind::Oak, TreeKind::Pine, TreeKind::Birch, TreeKind::Fruit][i % 4];
        d.add(placed(v(x, z), i as f32, PropKind::Tree(k)));
    }
    let clutter = [
        (v(5.0, 7.5), ClutterKind::Well),
        (v(-1.0, 1.0), ClutterKind::Cart),
        (v(-3.5, 3.0), ClutterKind::Barrel),
        (v(-3.0, 3.8), ClutterKind::Crate),
        (v(20.0, -16.0), ClutterKind::Haystack),
        (v(22.0, -18.0), ClutterKind::Scarecrow),
        (v(0.0, 10.0), ClutterKind::Bench),
        (v(-8.0, 0.0), ClutterKind::LanternPost),
        (v(8.0, 2.0), ClutterKind::LanternPost),
        (v(-14.0, -12.0 + 4.0), ClutterKind::FlagPole),
        (v(-9.0, 10.0), ClutterKind::WashingLine),
        (v(12.0, -4.0), ClutterKind::Woodpile),
        (v(-28.0, -26.0), ClutterKind::Rock),
        (v(26.0, 26.0), ClutterKind::Rock),
        (v(-24.0, 16.0), ClutterKind::Bush),
        (v(-18.0, 18.0), ClutterKind::Beehive),
    ];
    for (i, (at, k)) in clutter.into_iter().enumerate() {
        d.add(placed(at, i as f32 * 0.7, PropKind::Clutter(k)));
    }
    d
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scene::build;

    #[test]
    fn demo_builds_valid_meshes() {
        let doc = demo_document(42);
        let s = build(&doc);
        assert_eq!(s.parts.len(), doc.structures.len());
        for p in &s.parts {
            assert!(p.mesh.is_valid(), "part {}", p.id);
            assert!(!p.mesh.is_empty(), "empty part {} {:?}", p.id, doc.get(p.id));
        }
        assert!(s.ground.is_valid() && !s.ground.is_empty());
        assert!(s.water.is_valid() && !s.water.is_empty());
        assert!(s.scatter.is_valid());
        assert!(s.markers.iter().any(|m| m.kind == crate::MarkerKind::ChimneySmoke));
        assert!(s.markers.iter().any(|m| m.kind == crate::MarkerKind::Pond));
    }

    /// `cargo test -p glade_core --release -- --ignored --nocapture demo_timing`
    #[test]
    #[ignore]
    fn demo_timing() {
        let doc = demo_document(42);
        let t = std::time::Instant::now();
        let s = build(&doc);
        let tris: usize = s.parts.iter().map(|p| p.mesh.indices.len() / 3).sum::<usize>()
            + (s.ground.indices.len() + s.water.indices.len() + s.scatter.indices.len()) / 3;
        println!("build: {:?}, triangles: {tris}, scatter tris: {}", t.elapsed(), s.scatter.indices.len() / 3);
        // Live preview of one moved tree.
        let mut moved = doc.clone();
        if let Some((_, Structure::Placed(p))) = moved.structures.iter_mut().rev().find(|(_, s)| matches!(s, Structure::Placed(_))) {
            p.at.x += 0.5;
        }
        let t = std::time::Instant::now();
        let _ = crate::scene::build_incremental(&moved, Some(&s));
        println!("incremental (one prop moved): {:?}", t.elapsed());
    }
}

/// Every primitive variant laid out in rows on flattened ground (visual QA).
/// Rows (z): -24 walls, -14 fences + paths, -4 houses, 8 towers + roofed rings,
/// 18 stairs/platform/bridges, 27 trees + clutter.
pub fn showcase_document(seed: u64) -> Document {
    let mut d = Document::new(seed);
    // Level the glade so heights are comparable.
    for zi in (-28..=28).step_by(8) {
        for xi in (-28..=28).step_by(8) {
            d.terrain.push(TerrainDab { center: v(xi as f32, zi as f32), radius: 9.0, op: TerrainOp::Flatten { height: 0.5, strength: 1.0 } });
        }
    }
    // A pond under the bridges.
    for x in [-2.0, 4.0, 10.0] {
        d.terrain.push(TerrainDab { center: v(x, 18.0), radius: 3.0, op: TerrainOp::Water { depth: 1.2 } });
    }

    // Walls: material × battlements × ruined × foundation, 6 m segments.
    let mut x = -28.0;
    for material in [Material::Stone, Material::Wood] {
        for (battlements, ruined, foundation) in [(false, false, 0.0), (true, false, 0.6), (false, true, 0.0), (true, false, 0.0)] {
            let mut w = wall(vec![v(x, -24.0), v(x + 5.0, -24.0)], 4.0);
            w.material = material;
            w.battlements = battlements;
            w.ruined = ruined;
            w.foundation = foundation;
            w.openings = vec![Opening { along: 1.5, kind: OpeningKind::Window }, Opening { along: 3.5, kind: OpeningKind::Door }];
            d.add(Structure::Wall(w));
            x += 7.0;
        }
    }
    // A tall wall with a gate cut by a path.
    let mut tall = wall(vec![v(-28.0, -30.0), v(-14.0, -30.0)], 7.0);
    tall.openings = vec![Opening { along: 11.0, kind: OpeningKind::WideDoor }];
    d.add(Structure::Wall(tall));
    d.add(Structure::Path(Path { points: vec![v(-21.0, -33.0), v(-21.0, -27.0)], width: 2.0, kind: PathKind::Cobble }));

    // Fences and paths.
    for (i, kind) in [FenceKind::WoodRail, FenceKind::Picket, FenceKind::Stone, FenceKind::Wattle].into_iter().enumerate() {
        let x0 = -28.0 + i as f32 * 7.0;
        d.add(Structure::Fence(Fence { points: vec![v(x0, -16.0), v(x0 + 5.0, -16.0)], kind, colors: colors(0, 0, i as u8) }));
    }
    for (i, kind) in [PathKind::Dirt, PathKind::Cobble, PathKind::Flagstone, PathKind::Setts].into_iter().enumerate() {
        let x0 = 2.0 + i as f32 * 7.0;
        d.add(Structure::Path(Path { points: vec![v(x0, -16.0), v(x0 + 2.5, -12.0), v(x0 + 5.0, -14.0)], width: 1.8, kind }));
    }

    // Houses: roof × material, plus half-timber and a tent.
    let mut x = -28.0;
    let roofs = [RoofStyle::Gable, RoofStyle::Hip, RoofStyle::Cone, RoofStyle::Flat];
    for (i, roof) in roofs.into_iter().enumerate() {
        let material = [Material::Stone, Material::Plaster, Material::Wood, Material::Plaster][i];
        let mut h = house(Footprint::Rect { center: v(x + 2.5, -4.0), half: v(2.6, 2.0), angle: 0.0 }, 4.0, roof, material, i == 3, colors(i as u8, i as u8, i as u8));
        h.battlements = roof == RoofStyle::Flat;
        d.add(Structure::House(h));
        x += 7.0;
    }
    d.add(Structure::House(house(Footprint::Rect { center: v(2.5, -4.0), half: v(3.0, 2.2), angle: 0.0 }, 5.5, RoofStyle::Gable, Material::Plaster, true, colors(1, 1, 0))));
    d.add(Structure::House(house(Footprint::Rect { center: v(10.0, -4.0), half: v(2.2, 1.8), angle: 0.0 }, 1.0, RoofStyle::Gable, Material::Wood, false, colors(5, 0, 3))));
    d.add(Structure::House(house(Footprint::Rect { center: v(17.0, -4.0), half: v(2.4, 2.0), angle: 0.0 }, 3.5, RoofStyle::Gable, Material::Stone, false, colors(4, 0, 1))));
    let mut manual = house(Footprint::Rect { center: v(24.5, -4.0), half: v(2.4, 2.0), angle: 0.0 }, 3.5, RoofStyle::Hip, Material::Plaster, false, colors(2, 2, 2));
    manual.openings = vec![Opening { along: 1.0, kind: OpeningKind::Window }, Opening { along: 3.0, kind: OpeningKind::Door }, Opening { along: 6.0, kind: OpeningKind::WideDoor }];
    d.add(Structure::House(manual));

    // Towers and round houses; roofed wall rings.
    for (i, (r, h, roof)) in [(2.2, 9.0, RoofStyle::Cone), (2.2, 9.0, RoofStyle::Flat), (2.0, 3.5, RoofStyle::Cone), (2.6, 6.0, RoofStyle::Hip)].into_iter().enumerate() {
        let mut t = house(Footprint::Round { center: v(-25.0 + i as f32 * 7.0, 8.0), radius: r }, h, roof, Material::Stone, false, colors(i as u8 + 1, 0, 0));
        t.battlements = roof == RoofStyle::Flat;
        d.add(Structure::House(t));
    }
    for (i, roof) in [RoofStyle::Gable, RoofStyle::Hip].into_iter().enumerate() {
        let x0 = 4.0 + i as f32 * 10.0;
        let mut ring = wall(vec![v(x0, 5.0), v(x0 + 7.0, 5.0), v(x0 + 7.0, 11.0), v(x0, 11.0), v(x0, 5.0)], 3.0);
        ring.roofed = true;
        ring.roof = roof;
        ring.battlements = false;
        ring.colors = colors(i as u8 * 3, 0, 1);
        d.add(Structure::Wall(ring));
    }

    // Stairs, platform, bridges over the pond.
    d.add(Structure::Stairs(Stairs { from: Vec3::new(-28.0, 0.5, 18.0), to: Vec3::new(-24.0, 3.5, 18.0), width: 1.2, kind: StairKind::Wood, colors: colors(0, 0, 0) }));
    d.add(Structure::Stairs(Stairs { from: Vec3::new(-22.0, 0.5, 18.0), to: Vec3::new(-18.0, 3.5, 18.0), width: 1.2, kind: StairKind::Stone, colors: Colors::default() }));
    d.add(Structure::Stairs(Stairs { from: Vec3::new(-16.0, 0.5, 18.0), to: Vec3::new(-15.0, 3.5, 18.0), width: 0.6, kind: StairKind::Ladder, colors: colors(0, 0, 1) }));
    d.add(Structure::Platform(Platform { center: v(-10.0, 18.0), half: v(2.0, 1.5), angle: 0.0, height: 2.0, colors: colors(0, 0, 1) }));
    let bridges = [
        (BridgeKind::Rope { railing: 0 }, Hanging::None),
        (BridgeKind::Rope { railing: 1 }, Hanging::Lanterns),
        (BridgeKind::Rope { railing: 2 }, Hanging::Flags),
        (BridgeKind::Rope { railing: 3 }, Hanging::None),
        (BridgeKind::Stone, Hanging::None),
        (BridgeKind::Wood, Hanging::None),
    ];
    for (i, (kind, hanging)) in bridges.into_iter().enumerate() {
        let x0 = -5.0 + i as f32 * 3.0;
        d.add(Structure::Bridge(Bridge { a: Vec3::new(x0, 1.5, 14.0), b: Vec3::new(x0, 1.5, 22.0), width: 1.2, kind, hanging, colors: colors(0, 0, i as u8) }));
    }

    // Trees and clutter rows.
    for (i, k) in [TreeKind::Oak, TreeKind::Pine, TreeKind::Birch, TreeKind::Fruit].into_iter().enumerate() {
        d.add(placed(v(18.0 + i as f32 * 3.5, 18.0), 0.0, PropKind::Tree(k)));
    }
    let clutter = [
        ClutterKind::Barrel, ClutterKind::Crate, ClutterKind::Haystack, ClutterKind::Well, ClutterKind::Cart,
        ClutterKind::Bench, ClutterKind::LanternPost, ClutterKind::FlagPole, ClutterKind::WashingLine,
        ClutterKind::Woodpile, ClutterKind::Rock, ClutterKind::Bush, ClutterKind::Beehive, ClutterKind::Scarecrow,
    ];
    for (i, k) in clutter.into_iter().enumerate() {
        d.add(placed(v(-28.0 + i as f32 * 4.0, 27.0), 0.0, PropKind::Clutter(k)));
    }
    d
}

#[cfg(test)]
mod showcase_tests {
    #[test]
    fn showcase_parts_are_valid_and_non_empty() {
        let doc = super::showcase_document(1);
        let s = crate::scene::build(&doc);
        for p in &s.parts {
            assert!(p.mesh.is_valid() && !p.mesh.is_empty(), "part {} {:?}", p.id, doc.get(p.id));
        }
    }
}
