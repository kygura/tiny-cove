//! A scripted showcase valley that uses every primitive (for `--demo-shot` and tests).
//!
//! Composed in screen-aligned coordinates for the default camera (yaw 45°, pitch 35°,
//! zoom 1): `u` runs left→right across the screen, `w` from the back (top) to the front
//! (bottom). Everything sits within |u| ≤ 19, -16 ≤ w ≤ 19 so a single default shot shows it.
//! A stream runs diagonally from the back-right into a pond at the front-left; the civic
//! walls stand back-left, the blocks front-right.

use crate::model::*;
use crate::terrain::Terrain;
use glam::{Vec2, Vec3};
use std::f32::consts::FRAC_1_SQRT_2;

/// Screen-aligned (u right, w towards the viewer) → world (x, z).
/// Depth is squeezed slightly (and shifted forward) so tall back-row structures stay in frame.
fn s(u: f32, w: f32) -> Vec2 {
    let w = w * 0.9 + 1.5;
    Vec2::new((u + w) * FRAC_1_SQRT_2, (w - u) * FRAC_1_SQRT_2)
}

fn colors(roof: u8, wall: u8, trim: u8) -> Colors {
    Colors { roof, wall, trim }
}

fn wall(points: Vec<Vec2>, height: f32, weathering: f32) -> Wall {
    Wall {
        points,
        height,
        thickness: 1.0,
        kind: WallKind::DryStone,
        crest: false,
        tumbled: false,
        footing: 0.6,
        roofed: false,
        roof: RoofStyle::Boat,
        roof_pitch: 0.7,
        openings: Vec::new(),
        colors: Colors::default(),
        weathering,
    }
}

#[allow(clippy::too_many_arguments)]
fn block(at: Vec2, half: Vec2, height: f32, roof: RoofStyle, material: Material, carved: bool, c: Colors, weathering: f32) -> Structure {
    Structure::House(House {
        footprint: Footprint::Rect { center: at, half, angle: 0.0 },
        wall_height: height,
        roof,
        roof_pitch: 0.7,
        material,
        carved,
        stilts: 0.0,
        hangings: false,
        openings: Vec::new(),
        colors: c,
        weathering,
    })
}

fn placed(at: Vec2, angle: f32, scale: f32, kind: PropKind, c: Colors) -> Structure {
    Structure::Placed(Placed { at, angle, scale, kind, colors: c })
}

/// Stream centre line `u + w = 3` at screen column `u`, and its crossing direction.
fn stream_at(u: f32) -> Vec2 {
    s(u, 3.0 - u)
}

/// The demo document. Deterministic for a given seed.
pub fn demo_document(seed: u64) -> Document {
    let mut d = Document::new(seed);

    // ------------------------------------------------------------------ terrain
    // Level the stage, then carve a diagonal stream into a pond and paint the ground.
    for i in -2..=2 {
        for j in -2..=2 {
            let c = Vec2::new(i as f32 * 8.0, j as f32 * 8.0);
            d.terrain.push(TerrainDab { center: c, radius: 7.5, op: TerrainOp::Flatten { height: 0.8, strength: 0.85 } });
        }
    }
    for i in 0..=22 {
        let u = 18.0 - i as f32 * 1.0;
        d.terrain.push(TerrainDab { center: stream_at(u), radius: 2.4, op: TerrainOp::Water { depth: 1.3 } });
    }
    d.terrain.push(TerrainDab { center: s(-7.0, 9.0), radius: 4.4, op: TerrainOp::Water { depth: 1.4 } });
    d.terrain.push(TerrainDab { center: s(-9.5, 11.0), radius: 3.2, op: TerrainOp::Water { depth: 1.2 } });
    d.terrain.push(TerrainDab { center: s(-19.0, -12.0), radius: 5.0, op: TerrainOp::Sculpt { amount: 1.2 } });
    for (u, w, r, layer) in [
        (12.0, -4.5, 3.0, PaintLayer::SeaPinks),
        (2.5, 15.5, 3.0, PaintLayer::SeaPinks),
        (-15.0, 10.5, 3.0, PaintLayer::SeaPinks),
        (17.0, 16.0, 2.5, PaintLayer::SeaPinks),
        (-4.5, 6.5, 2.5, PaintLayer::DuneGrass),
        (-12.5, 13.0, 3.0, PaintLayer::DuneGrass),
        (7.0, -14.5, 2.5, PaintLayer::DuneGrass),
        (15.0, 15.0, 3.5, PaintLayer::Wrack),
        (-8.5, -9.5, 3.0, PaintLayer::Wrack),
        (-18.0, -2.0, 2.5, PaintLayer::Wrack),
        (14.5, 9.0, 2.5, PaintLayer::Pebbles),
        (-2.0, -2.5, 2.0, PaintLayer::Pebbles),
    ] {
        d.terrain.push(TerrainDab { center: s(u, w), radius: r, op: TerrainOp::Paint { layer, strength: 1.0 } });
    }
    let ground = Terrain::build(seed, &d.terrain, &[]);
    let h = |p: Vec2| ground.height(p);
    let at = |u: f32, w: f32, up: f32| {
        let p = s(u, w);
        Vec3::new(p.x, h(p) + up, p.y)
    };

    // ------------------------------------------------------------------ civic walls (back-left)
    // Roofed wall ring: a courtyard hall the pavers path enters through a portal.
    let mut ring = wall(vec![s(-18.0, -12.0), s(-12.0, -12.0), s(-12.0, -7.0), s(-18.0, -7.0), s(-18.0, -12.0)], 3.2, 0.35);
    ring.roofed = true;
    ring.roof = RoofStyle::Boat;
    ring.colors = colors(3, 1, 0);
    ring.openings.push(Opening { along: 2.5, kind: OpeningKind::Window });
    d.add(Structure::Wall(ring));
    // Long board-formed wall; the pavers path cuts a portal through it.
    let mut long = wall(vec![s(-18.0, -3.0), s(-6.0, -3.0)], 4.0, 0.5);
    long.openings.push(Opening { along: 3.5, kind: OpeningKind::Window });
    d.add(Structure::Wall(long));
    let top_a = h(s(-16.5, -3.0)) + 4.0;
    let top_b = h(s(-8.0, -3.0)) + 4.0;
    d.add(Structure::Stairs(Stairs { from: at(-16.5, 1.6, 0.0), to: { let p = s(-16.5, -3.0); Vec3::new(p.x, top_a, p.y) }, width: 1.0, kind: StairKind::Log, colors: colors(0, 0, 0) }));
    d.add(Structure::Stairs(Stairs { from: at(-8.0, 2.4, 0.0), to: { let p = s(-8.0, -3.0); Vec3::new(p.x, top_b, p.y) }, width: 1.3, kind: StairKind::Stone, colors: colors(0, 0, 2) }));
    // Silo tower behind the wall's end.
    d.add(Structure::House(House {
        footprint: Footprint::Round { center: s(-4.0, -8.0), radius: 2.0 },
        wall_height: 8.0,
        roof: RoofStyle::Thatch,
        roof_pitch: 0.8,
        material: Material::Wattle,
        carved: false,
        stilts: 0.0,
        hangings: false,
        openings: Vec::new(),
        colors: colors(2, 0, 0),
        weathering: 0.3,
    }));
    // Sawtooth workshop with fins.
    d.add(block(s(2.0, -11.0), Vec2::new(3.2, 2.4), 4.5, RoofStyle::Hide, Material::Wattle, true, colors(4, 1, 2), 0.2));

    // ------------------------------------------------------------------ stream (back-right → pond)
    // Aqueduct: a wall across the stream.
    let mut aq = wall(vec![s(9.0, -14.0), s(17.0, -6.0)], 3.0, 0.6);
    aq.thickness = 0.8;
    aq.footing = 0.0;
    d.add(Structure::Wall(aq));
    let aq_top = h(s(15.5, -7.5)) + 3.0;
    d.add(Structure::Stairs(Stairs { from: at(16.6, -8.6, 0.0), to: { let p = s(15.5, -7.5); Vec3::new(p.x, aq_top, p.y) }, width: 0.6, kind: StairKind::RopeLadder, colors: colors(0, 0, 2) }));
    // One bridge of each kind across the stream, back to front.
    let bridge = |u: f32, kind: BridgeKind, hanging: Hanging, width: f32, c: Colors| {
        let off = 3.6 * FRAC_1_SQRT_2;
        let (a, b) = (s(u - off, 3.0 - u - off), s(u + off, 3.0 - u + off));
        let y = h(a).max(h(b)) + 0.15;
        Structure::Bridge(Bridge { a: Vec3::new(a.x, y, a.y), b: Vec3::new(b.x, y, b.y), width, kind, hanging, colors: c })
    };
    d.add(bridge(9.0, BridgeKind::Rope { railing: 1 }, Hanging::Lanterns, 1.3, colors(0, 0, 2)));
    d.add(bridge(5.0, BridgeKind::Causeway, Hanging::None, 2.0, colors(0, 0, 0)));
    d.add(bridge(1.0, BridgeKind::Log, Hanging::None, 1.8, colors(0, 0, 0)));
    d.add(bridge(-2.5, BridgeKind::Rope { railing: 3 }, Hanging::Pennants, 1.2, colors(0, 0, 1)));

    // ------------------------------------------------------------------ blocks (front-right)
    d.add(block(s(8.0, 6.0), Vec2::new(3.0, 2.2), 4.5, RoofStyle::Boat, Material::Wattle, false, colors(3, 0, 0), 0.1));
    let mut green = House {
        footprint: Footprint::Rect { center: s(16.0, 1.0), half: Vec2::new(2.2, 2.2), angle: 0.0 },
        wall_height: 3.6,
        roof: RoofStyle::Turf,
        roof_pitch: 0.0,
        material: Material::Plank,
        carved: false,
        stilts: 0.0,
        hangings: true,
        openings: Vec::new(),
        colors: colors(0, 0, 1),
        weathering: 0.5,
    };
    green.openings.push(Opening { along: 1.5, kind: OpeningKind::WideDoor });
    d.add(Structure::House(green));
    d.add(block(s(7.0, 15.0), Vec2::new(2.4, 2.4), 3.8, RoofStyle::Thatch, Material::Stone, false, colors(2, 2, 4), 0.45));
    // Pavilion: very low walls → canopy on pilotis.
    d.add(block(s(15.0, 10.0), Vec2::new(2.4, 1.8), 0.8, RoofStyle::Boat, Material::Wattle, false, colors(5, 0, 2), 0.25));
    // Fins block with a green roof (front-left, by the long wall).
    d.add(block(s(-15.0, 5.0), Vec2::new(2.4, 2.0), 6.0, RoofStyle::Turf, Material::Wattle, true, colors(1, 4, 3), 0.65));
    // Ruined wall with exposed rebar, deep in moss.
    let mut ruin = wall(vec![s(12.5, 17.0), s(15.5, 15.5), s(18.0, 12.5)], 2.8, 0.9);
    ruin.tumbled = true;
    ruin.footing = 0.3;
    d.add(Structure::Wall(ruin));
    // Rammed-earth planter wall wrapping the pond.
    let mut planters = wall(vec![s(-19.0, 9.5), s(-15.5, 14.5), s(-11.0, 17.5)], 2.0, 0.75);
    planters.kind = WallKind::Turf;
    planters.crest = true;
    planters.colors = colors(0, 2, 4);
    d.add(Structure::Wall(planters));
    d.add(Structure::Platform(Platform { center: s(-0.8, 12.0), half: Vec2::new(2.2, 1.6), angle: 0.0, height: 1.2, colors: colors(0, 0, 0) }));

    // ------------------------------------------------------------------ paths
    let path = |pts: &[(f32, f32)], width: f32, kind: PathKind| {
        Structure::Path(Path { points: pts.iter().map(|&(u, w)| s(u, w)).collect(), width, kind })
    };
    d.add(path(&[(-15.0, -9.5), (-10.0, -6.0), (-10.0, 3.0), (-12.0, 6.0)], 2.0, PathKind::Pebble));
    d.add(path(&[(-12.0, 6.0), (-7.0, 10.0), (-3.0, 11.0)], 1.2, PathKind::Stepping));
    d.add(path(&[(-4.0, -5.4), (2.3, -4.7)], 1.4, PathKind::Shell));
    d.add(path(&[(-5.0, 1.2), (-1.7, -0.7)], 1.6, PathKind::Flagstone));
    d.add(path(&[(7.7, 0.7), (12.0, 2.5)], 1.6, PathKind::Flagstone));
    d.add(path(&[(3.7, 4.7), (3.0, 10.0), (11.8, 10.2)], 1.8, PathKind::Boardwalk));

    // ------------------------------------------------------------------ fences
    let fence = |pts: &[(f32, f32)], kind: FenceKind, c: Colors| Structure::Fence(Fence { points: pts.iter().map(|&(u, w)| s(u, w)).collect(), kind, colors: c });
    d.add(fence(&[(1.5, 15.0), (1.5, 18.5)], FenceKind::Driftwood, colors(0, 0, 0)));
    d.add(fence(&[(-10.5, 15.2), (-5.0, 15.6)], FenceKind::Wattle, Colors::default()));
    d.add(fence(&[(18.2, 4.6), (16.2, 6.6)], FenceKind::NetLine, Colors::default()));
    d.add(fence(&[(-1.0, 16.5), (4.5, 18.5)], FenceKind::Reed, Colors::default()));

    // ------------------------------------------------------------------ trees
    for (u, w, k, sc) in [
        (-19.0, -15.0, TreeKind::Snag, 1.0),
        (-19.0, -7.5, TreeKind::SeaPine, 0.9),
        (-1.0, -14.5, TreeKind::Cypress, 1.0),
        (8.0, -16.5, TreeKind::SeaPine, 1.1),
        (18.5, -11.0, TreeKind::Snag, 0.9),
        (-13.0, 0.5, TreeKind::Fig, 1.0),
        (11.5, 18.0, TreeKind::Cypress, 0.9),
        (18.0, 8.5, TreeKind::Fig, 0.9),
        (-4.0, 7.6, TreeKind::Palm, 1.0),
        (-12.5, 12.8, TreeKind::Palm, 1.1),
        (-2.5, 3.0, TreeKind::Palm, 0.8),
        (-19.0, 17.0, TreeKind::Cypress, 1.0),
    ] {
        d.add(placed(s(u, w), u * 0.37 + w, sc, PropKind::Tree(k), Colors::default()));
    }

    // ------------------------------------------------------------------ clutter (every kind)
    for (i, (u, w, k)) in [
        (5.5, 2.8, ClutterKind::Baskets),
        (-8.4, 1.4, ClutterKind::CrabPots),
        (-0.8, -7.8, ClutterKind::Firepit),
        (-6.6, -12.0, ClutterKind::CarvedPole),
        (-5.2, -1.0, ClutterKind::NetRack),
        (-0.5, -2.6, ClutterKind::Canoe),
        (-10.8, -1.0, ClutterKind::LanternPost),
        (2.4, 8.6, ClutterKind::LanternPost),
        (10.5, 12.6, ClutterKind::LanternPost),
        (19.0, -3.0, ClutterKind::Pennant),
        (-9.0, -12.5, ClutterKind::DryingRack),
        (-7.5, -6.6, ClutterKind::Woodpile),
        (18.5, -7.5, ClutterKind::Rock),
        (-17.0, 16.5, ClutterKind::Rock),
        (-12.5, 17.8, ClutterKind::Bush),
        (13.0, 14.0, ClutterKind::Bush),
        (6.5, -14.0, ClutterKind::Cairn),
        (11.0, -1.2, ClutterKind::StandingStone),
    ]
    .into_iter()
    .enumerate()
    {
        d.add(placed(s(u, w), i as f32 * 0.7, 1.0, PropKind::Clutter(k), colors(0, 0, (i % 5) as u8)));
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

    #[test]
    fn demo_shows_every_kind() {
        let doc = demo_document(7);
        let props: Vec<PropKind> = doc.structures.iter().filter_map(|(_, s)| if let Structure::Placed(p) = s { Some(p.kind) } else { None }).collect();
        for k in [TreeKind::Snag, TreeKind::SeaPine, TreeKind::Cypress, TreeKind::Fig, TreeKind::Palm] {
            assert!(props.contains(&PropKind::Tree(k)), "{k:?}");
        }
        use ClutterKind::*;
        for k in [Canoe, DryingRack, Firepit, Cairn, StandingStone, CarvedPole, NetRack, Baskets, LanternPost, Pennant, Rock, Bush, CrabPots, Woodpile] {
            assert!(props.contains(&PropKind::Clutter(k)), "{k:?}");
        }
        let has = |f: &dyn Fn(&Structure) -> bool| doc.structures.iter().any(|(_, s)| f(s));
        for k in [PathKind::Shell, PathKind::Pebble, PathKind::Flagstone, PathKind::Boardwalk, PathKind::Stepping] {
            assert!(has(&|s| matches!(s, Structure::Path(p) if p.kind == k)), "{k:?}");
        }
        for k in [FenceKind::Driftwood, FenceKind::Wattle, FenceKind::NetLine, FenceKind::Reed] {
            assert!(has(&|s| matches!(s, Structure::Fence(f) if f.kind == k)), "{k:?}");
        }
        for k in [StairKind::Log, StairKind::Stone, StairKind::RopeLadder] {
            assert!(has(&|s| matches!(s, Structure::Stairs(t) if t.kind == k)), "{k:?}");
        }
        for r in [RoofStyle::Boat, RoofStyle::Hide, RoofStyle::Thatch, RoofStyle::Turf] {
            assert!(has(&|s| matches!(s, Structure::House(h) if h.roof == r)), "{r:?}");
        }
        assert!(has(&|s| matches!(s, Structure::Bridge(b) if b.kind == BridgeKind::Causeway)));
        assert!(has(&|s| matches!(s, Structure::Bridge(b) if b.kind == BridgeKind::Log)));
        assert!(has(&|s| matches!(s, Structure::Bridge(b) if b.hanging == Hanging::Lanterns)));
        assert!(has(&|s| matches!(s, Structure::Bridge(b) if b.hanging == Hanging::Pennants)));
        assert!(has(&|s| matches!(s, Structure::Wall(w) if w.tumbled)));
        assert!(has(&|s| matches!(s, Structure::Wall(w) if w.crest)));
        assert!(has(&|s| matches!(s, Structure::Wall(w) if w.roofed)));
        assert!(has(&|s| matches!(s, Structure::Platform(_))));
        // Same seed, same document.
        assert_eq!(demo_document(7), doc);
    }

    /// `cargo test -p cairn_cove_core --release -- --ignored --nocapture demo_timing`
    #[test]
    #[ignore]
    fn demo_timing() {
        let doc = demo_document(42);
        let t = std::time::Instant::now();
        let s = build(&doc);
        let tris: usize = s.parts.iter().map(|p| p.mesh.indices.len() / 3).sum::<usize>()
            + (s.ground.indices.len() + s.water.indices.len() + s.scatter.indices.len()) / 3;
        println!("build: {:?}, triangles: {tris}, scatter tris: {}", t.elapsed(), s.scatter.indices.len() / 3);
    }
}
