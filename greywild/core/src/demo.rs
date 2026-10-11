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

fn wall(points: Vec<Vec2>, height: f32, overgrowth: f32) -> Wall {
    Wall {
        points,
        height,
        thickness: 1.0,
        material: Material::Concrete,
        planters: false,
        ruined: false,
        foundation: 0.6,
        roofed: false,
        roof: RoofStyle::Pitched,
        roof_pitch: 0.7,
        openings: Vec::new(),
        colors: Colors::default(),
        overgrowth,
    }
}

#[allow(clippy::too_many_arguments)]
fn block(at: Vec2, half: Vec2, height: f32, roof: RoofStyle, material: Material, fins: bool, c: Colors, overgrowth: f32) -> Structure {
    Structure::House(House {
        footprint: Footprint::Rect { center: at, half, angle: 0.0 },
        wall_height: height,
        roof,
        roof_pitch: 0.7,
        material,
        fins,
        foundation: 0.4,
        planters: false,
        openings: Vec::new(),
        colors: c,
        overgrowth,
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
        (12.0, -4.5, 3.0, PaintLayer::Flowers),
        (2.5, 15.5, 3.0, PaintLayer::Flowers),
        (-15.0, 10.5, 3.0, PaintLayer::Flowers),
        (17.0, 16.0, 2.5, PaintLayer::Flowers),
        (-4.5, 6.5, 2.5, PaintLayer::Ferns),
        (-12.5, 13.0, 3.0, PaintLayer::Ferns),
        (7.0, -14.5, 2.5, PaintLayer::Ferns),
        (15.0, 15.0, 3.5, PaintLayer::Moss),
        (-8.5, -9.5, 3.0, PaintLayer::Moss),
        (-18.0, -2.0, 2.5, PaintLayer::Moss),
        (14.5, 9.0, 2.5, PaintLayer::Gravel),
        (-2.0, -2.5, 2.0, PaintLayer::Gravel),
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
    ring.roof = RoofStyle::Pitched;
    ring.colors = colors(3, 1, 0);
    ring.openings.push(Opening { along: 2.5, kind: OpeningKind::Window });
    d.add(Structure::Wall(ring));
    // Long board-formed wall; the pavers path cuts a portal through it.
    let mut long = wall(vec![s(-18.0, -3.0), s(-6.0, -3.0)], 4.0, 0.5);
    long.openings.push(Opening { along: 3.5, kind: OpeningKind::Window });
    d.add(Structure::Wall(long));
    let top_a = h(s(-16.5, -3.0)) + 4.0;
    let top_b = h(s(-8.0, -3.0)) + 4.0;
    d.add(Structure::Stairs(Stairs { from: at(-16.5, 1.6, 0.0), to: { let p = s(-16.5, -3.0); Vec3::new(p.x, top_a, p.y) }, width: 1.0, kind: StairKind::Steel, colors: colors(0, 0, 0) }));
    d.add(Structure::Stairs(Stairs { from: at(-8.0, 2.4, 0.0), to: { let p = s(-8.0, -3.0); Vec3::new(p.x, top_b, p.y) }, width: 1.3, kind: StairKind::Concrete, colors: colors(0, 0, 2) }));
    // Silo tower behind the wall's end.
    d.add(Structure::House(House {
        footprint: Footprint::Round { center: s(-4.0, -8.0), radius: 2.0 },
        wall_height: 8.0,
        roof: RoofStyle::Shell,
        roof_pitch: 0.8,
        material: Material::Concrete,
        fins: false,
        foundation: 0.5,
        planters: false,
        openings: Vec::new(),
        colors: colors(2, 0, 0),
        overgrowth: 0.3,
    }));
    // Sawtooth workshop with fins.
    d.add(block(s(2.0, -11.0), Vec2::new(3.2, 2.4), 4.5, RoofStyle::Sawtooth, Material::Concrete, true, colors(4, 1, 2), 0.2));

    // ------------------------------------------------------------------ stream (back-right → pond)
    // Aqueduct: a wall across the stream.
    let mut aq = wall(vec![s(9.0, -14.0), s(17.0, -6.0)], 3.0, 0.6);
    aq.thickness = 0.8;
    aq.foundation = 0.0;
    d.add(Structure::Wall(aq));
    let aq_top = h(s(15.5, -7.5)) + 3.0;
    d.add(Structure::Stairs(Stairs { from: at(16.6, -8.6, 0.0), to: { let p = s(15.5, -7.5); Vec3::new(p.x, aq_top, p.y) }, width: 0.6, kind: StairKind::Ladder, colors: colors(0, 0, 2) }));
    // One bridge of each kind across the stream, back to front.
    let bridge = |u: f32, kind: BridgeKind, hanging: Hanging, width: f32, c: Colors| {
        let off = 3.6 * FRAC_1_SQRT_2;
        let (a, b) = (s(u - off, 3.0 - u - off), s(u + off, 3.0 - u + off));
        let y = h(a).max(h(b)) + 0.15;
        Structure::Bridge(Bridge { a: Vec3::new(a.x, y, a.y), b: Vec3::new(b.x, y, b.y), width, kind, hanging, colors: c })
    };
    d.add(bridge(9.0, BridgeKind::Cable { railing: 1 }, Hanging::Lamps, 1.3, colors(0, 0, 2)));
    d.add(bridge(5.0, BridgeKind::Arch, Hanging::None, 2.0, colors(0, 0, 0)));
    d.add(bridge(1.0, BridgeKind::Girder, Hanging::None, 1.8, colors(0, 0, 0)));
    d.add(bridge(-2.5, BridgeKind::Cable { railing: 3 }, Hanging::Vines, 1.2, colors(0, 0, 1)));

    // ------------------------------------------------------------------ blocks (front-right)
    d.add(block(s(8.0, 6.0), Vec2::new(3.0, 2.2), 4.5, RoofStyle::Pitched, Material::Concrete, false, colors(3, 0, 0), 0.1));
    let mut green = House {
        footprint: Footprint::Rect { center: s(16.0, 1.0), half: Vec2::new(2.2, 2.2), angle: 0.0 },
        wall_height: 3.6,
        roof: RoofStyle::Green,
        roof_pitch: 0.0,
        material: Material::Timber,
        fins: false,
        foundation: 0.5,
        planters: true,
        openings: Vec::new(),
        colors: colors(0, 0, 1),
        overgrowth: 0.5,
    };
    green.openings.push(Opening { along: 1.5, kind: OpeningKind::WideDoor });
    d.add(Structure::House(green));
    d.add(block(s(7.0, 15.0), Vec2::new(2.4, 2.4), 3.8, RoofStyle::Shell, Material::RammedEarth, false, colors(2, 2, 4), 0.45));
    // Pavilion: very low walls → canopy on pilotis.
    d.add(block(s(15.0, 10.0), Vec2::new(2.4, 1.8), 0.8, RoofStyle::Pitched, Material::Concrete, false, colors(5, 0, 2), 0.25));
    // Fins block with a green roof (front-left, by the long wall).
    d.add(block(s(-15.0, 5.0), Vec2::new(2.4, 2.0), 6.0, RoofStyle::Green, Material::Concrete, true, colors(1, 4, 3), 0.65));
    // Ruined wall with exposed rebar, deep in moss.
    let mut ruin = wall(vec![s(12.5, 17.0), s(15.5, 15.5), s(18.0, 12.5)], 2.8, 0.9);
    ruin.ruined = true;
    ruin.foundation = 0.3;
    d.add(Structure::Wall(ruin));
    // Rammed-earth planter wall wrapping the pond.
    let mut planters = wall(vec![s(-19.0, 9.5), s(-15.5, 14.5), s(-11.0, 17.5)], 2.0, 0.75);
    planters.material = Material::RammedEarth;
    planters.planters = true;
    planters.colors = colors(0, 2, 4);
    d.add(Structure::Wall(planters));
    d.add(Structure::Platform(Platform { center: s(-0.8, 12.0), half: Vec2::new(2.2, 1.6), angle: 0.0, height: 1.2, colors: colors(0, 0, 0) }));

    // ------------------------------------------------------------------ paths
    let path = |pts: &[(f32, f32)], width: f32, kind: PathKind| {
        Structure::Path(Path { points: pts.iter().map(|&(u, w)| s(u, w)).collect(), width, kind })
    };
    d.add(path(&[(-15.0, -9.5), (-10.0, -6.0), (-10.0, 3.0), (-12.0, 6.0)], 2.0, PathKind::Pavers));
    d.add(path(&[(-12.0, 6.0), (-7.0, 10.0), (-3.0, 11.0)], 1.2, PathKind::Stepping));
    d.add(path(&[(-4.0, -5.4), (2.3, -4.7)], 1.4, PathKind::Gravel));
    d.add(path(&[(-5.0, 1.2), (-1.7, -0.7)], 1.6, PathKind::Slabs));
    d.add(path(&[(7.7, 0.7), (12.0, 2.5)], 1.6, PathKind::Slabs));
    d.add(path(&[(3.7, 4.7), (3.0, 10.0), (11.8, 10.2)], 1.8, PathKind::Poured));

    // ------------------------------------------------------------------ fences
    let fence = |pts: &[(f32, f32)], kind: FenceKind, c: Colors| Structure::Fence(Fence { points: pts.iter().map(|&(u, w)| s(u, w)).collect(), kind, colors: c });
    d.add(fence(&[(1.5, 15.0), (1.5, 18.5)], FenceKind::Railing, colors(0, 0, 0)));
    d.add(fence(&[(-10.5, 15.2), (-5.0, 15.6)], FenceKind::Balustrade, Colors::default()));
    d.add(fence(&[(18.2, 4.6), (16.2, 6.6)], FenceKind::Gabion, Colors::default()));
    d.add(fence(&[(-1.0, 16.5), (4.5, 18.5)], FenceKind::Hedge, Colors::default()));

    // ------------------------------------------------------------------ trees
    for (u, w, k, sc) in [
        (-19.0, -15.0, TreeKind::Oak, 1.0),
        (-19.0, -7.5, TreeKind::Pine, 0.9),
        (-1.0, -14.5, TreeKind::Birch, 1.0),
        (8.0, -16.5, TreeKind::Pine, 1.1),
        (18.5, -11.0, TreeKind::Oak, 0.9),
        (-13.0, 0.5, TreeKind::Fruit, 1.0),
        (11.5, 18.0, TreeKind::Birch, 0.9),
        (18.0, 8.5, TreeKind::Fruit, 0.9),
        (-4.0, 7.6, TreeKind::Fern, 1.0),
        (-12.5, 12.8, TreeKind::Fern, 1.1),
        (-2.5, 3.0, TreeKind::Fern, 0.8),
        (-19.0, 17.0, TreeKind::Birch, 1.0),
    ] {
        d.add(placed(s(u, w), u * 0.37 + w, sc, PropKind::Tree(k), Colors::default()));
    }

    // ------------------------------------------------------------------ clutter (every kind)
    for (i, (u, w, k)) in [
        (5.5, 2.8, ClutterKind::RainBarrel),
        (-8.4, 1.4, ClutterKind::Planter),
        (-0.8, -7.8, ClutterKind::Compost),
        (-6.6, -12.0, ClutterKind::Cistern),
        (-5.2, -1.0, ClutterKind::Wheelbarrow),
        (-0.5, -2.6, ClutterKind::Bench),
        (-10.8, -1.0, ClutterKind::LampPost),
        (2.4, 8.6, ClutterKind::LampPost),
        (10.5, 12.6, ClutterKind::LampPost),
        (19.0, -3.0, ClutterKind::WindTurbine),
        (-9.0, -12.5, ClutterKind::WashingLine),
        (-7.5, -6.6, ClutterKind::LogPile),
        (18.5, -7.5, ClutterKind::Rock),
        (-17.0, 16.5, ClutterKind::Rock),
        (-12.5, 17.8, ClutterKind::Bush),
        (13.0, 14.0, ClutterKind::Bush),
        (6.5, -14.0, ClutterKind::Beehive),
        (11.0, -1.2, ClutterKind::Solar),
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

    /// `cargo test -p greywild_core --release -- --ignored --nocapture demo_timing`
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
