//! The scripted showcase cove (for `--demo`, `--demo-shot` and tests). It uses every kit
//! primitive and every swatch.
//!
//! Composed in canonical screen coordinates `(u, w)` for the default camera (yaw 45°,
//! pitch 35°, zoom 1): `u` runs left→right, `w` from the back (top) towards the viewer
//! (bottom); the shot shows roughly |u|, |w| ≤ 20. In this frame the island's cove opens
//! towards the bottom-right (along `u = w`) and the headland rises on the bottom-left flank.
//! [`Frame`] turns (and mirrors) the composition with the seed's island, so every seed gets
//! the same layout around its own cove; for the default demo seed (7) the frame is the
//! camera's. Anything that must meet the shoreline, the headland top or a sea stack is placed
//! by querying the terrain.
//!
//! Layout: a hamlet of three huts in a row (thatch, hide, boat roofs) behind the cove beach,
//! a stilted turf-roofed hut over the water on the headland side, a stone hut by a dry-stone
//! field dyke ring that the shell track cuts through, two roundhouses, a beacon on the
//! headland reached by stone steps, a jetty with moored canoes, a breakwater sheltering the
//! cove, a lagoon crossed by every bridge kind, and a rope bridge out to a sea stack.

use crate::model::*;
use crate::terrain::{IslandPlan, Terrain};
use glam::{Vec2, Vec3};
use std::f32::consts::FRAC_1_SQRT_2;

/// The cove frame: local `a` runs out along the cove axis, local `b` towards the headland.
struct Frame {
    cove: Vec2,
    side: Vec2,
}

impl Frame {
    fn new(plan: &IslandPlan) -> Self {
        let perp = plan.cove.perp();
        let side = if perp.dot(plan.headland) >= 0.0 { perp } else { -perp };
        Frame { cove: plan.cove, side }
    }

    /// Local cove coordinates → world.
    fn local(&self, a: f32, b: f32) -> Vec2 {
        self.cove * a + self.side * b
    }

    /// Canonical screen coordinates → world.
    fn s(&self, u: f32, w: f32) -> Vec2 {
        self.local((u + w) * FRAC_1_SQRT_2, (w - u) * FRAC_1_SQRT_2)
    }

    /// World angle (for `Footprint::Rect` / `Placed`) of the canonical direction `(du, dw)`.
    fn angle(&self, du: f32, dw: f32) -> f32 {
        let d = self.s(du, dw);
        d.y.atan2(d.x)
    }
}

fn colors(roof: u8, wall: u8, trim: u8) -> Colors {
    Colors { roof, wall, trim }
}

fn dyke(points: Vec<Vec2>, kind: WallKind, height: f32, c: Colors, weathering: f32) -> Wall {
    Wall {
        points,
        height,
        thickness: 0.9,
        kind,
        crest: true,
        tumbled: false,
        footing: 0.0,
        roofed: false,
        roof: RoofStyle::Thatch,
        roof_pitch: 0.7,
        openings: Vec::new(),
        colors: c,
        weathering,
    }
}

#[allow(clippy::too_many_arguments)]
fn hut(fp: Footprint, wall_height: f32, roof: RoofStyle, material: Material, carved: bool, hangings: bool, c: Colors, weathering: f32) -> Structure {
    Structure::House(House {
        footprint: fp,
        wall_height,
        roof,
        roof_pitch: 0.9,
        material,
        carved,
        hangings,
        stilts: 0.0,
        openings: Vec::new(),
        colors: c,
        weathering,
    })
}

fn placed(at: Vec2, angle: f32, scale: f32, kind: PropKind, c: Colors) -> Structure {
    Structure::Placed(Placed { at, angle, scale, kind, colors: c })
}

/// Distance along the cove axis (at lateral offset `b`) where the sea begins.
fn shore_a(t: &Terrain, f: &Frame, b: f32) -> f32 {
    (0..240).map(|i| i as f32 * 0.25).find(|a| t.is_sea(f.local(*a, b))).unwrap_or(30.0)
}

/// The flattest high spot on the headland, a little back from the cliff edge.
fn beacon_site(t: &Terrain, plan: &IslandPlan) -> Vec2 {
    let mut best = (f32::MIN, plan.headland * 18.0);
    for i in 0..60 {
        for k in -6..=6 {
            let dir = Vec2::from_angle(plan.headland.to_angle() + k as f32 * 0.04);
            let p = dir * (10.0 + i as f32 * 0.4);
            let y = t.height(p);
            let flat = (0..8).all(|j| (t.height(p + Vec2::from_angle(j as f32 * 0.785) * 3.0) - y).abs() < 0.9);
            if flat && y > best.0 {
                best = (y, p);
            }
        }
    }
    best.1
}

/// Ground point at `p`, lifted by `up`.
fn on(t: &Terrain, p: Vec2, up: f32) -> Vec3 {
    Vec3::new(p.x, t.height(p) + up, p.y)
}

/// The demo document. Deterministic for a given seed.
pub fn demo_document(seed: u64) -> Document {
    let mut d = Document::new(seed);
    let base = Terrain::build(seed, &[], &[]);
    let plan = base.plan().clone();
    let f = Frame::new(&plan);
    let s = |u: f32, w: f32| f.s(u, w);
    let along_u = f.angle(1.0, 0.0);
    let along_cove = plan.cove.to_angle();

    // Key spots found on the bare island.
    let jetty_b = -2.0;
    let jetty_shore = shore_a(&base, &f, jetty_b);
    let stilt_b = 5.5;
    let stilt_shore = shore_a(&base, &f, stilt_b);
    let beacon = beacon_site(&base, &plan);
    let stack = plan.stacks[0];

    // ------------------------------------------------------------------ terrain
    // A lagoon behind the dunes on the spit side, elongated front to back.
    for w in [-11.5, -8.5, -5.5, -2.5] {
        d.terrain.push(TerrainDab { center: s(14.5, w), radius: 2.9, op: TerrainOp::Water { depth: 1.2 } });
    }
    let paint = |d: &mut Document, p: Vec2, r: f32, layer: PaintLayer| d.terrain.push(TerrainDab { center: p, radius: r, op: TerrainOp::Paint { layer, strength: 1.0 } });
    // Sea pinks on the headland top and the turf by the huts; marram on the dunes; wrack at
    // the waterline; pebbles where the canoes are drawn up.
    for k in 0..3 {
        let q = beacon - plan.headland * (2.5 + 2.5 * k as f32) + f.side.perp() * (k as f32 - 1.0) * 2.0;
        paint(&mut d, q, 3.0, PaintLayer::SeaPinks);
    }
    paint(&mut d, s(-4.0, 5.5), 2.2, PaintLayer::SeaPinks);
    paint(&mut d, s(-11.0, -11.0), 2.5, PaintLayer::SeaPinks);
    paint(&mut d, s(9.0, -13.0), 2.5, PaintLayer::SeaPinks);
    for k in -5..=3 {
        let b = k as f32 * 3.0;
        let a = shore_a(&base, &f, b);
        paint(&mut d, f.local(a - 5.5, b), 2.2, PaintLayer::DuneGrass);
        paint(&mut d, f.local(a - 0.4, b), 1.6, PaintLayer::Wrack);
    }
    paint(&mut d, f.local(shore_a(&base, &f, -6.0) - 2.0, -6.0), 2.0, PaintLayer::Pebbles);
    let ground = Terrain::build(seed, &d.terrain, &[]);
    let t = &ground;

    // ------------------------------------------------------------------ hamlet
    // Three huts in a row facing the cove (A8): Chalk Lime under Golden Straw thatch, Sky
    // Wash under a Madder Dyed hide roof, Rose Lime under a Woad-striped boat roof (carved).
    let row = |u: f32, half: Vec2| Footprint::Rect { center: s(u, -1.0), half, angle: along_u };
    d.add(hut(row(-8.0, Vec2::new(2.4, 1.7)), 2.4, RoofStyle::Thatch, Material::Wattle, false, false, colors(0, 0, 0), 0.2));
    d.add(hut(row(0.0, Vec2::new(2.2, 1.6)), 2.2, RoofStyle::Hide, Material::Plank, false, false, colors(2, 3, 3), 0.1));
    d.add(hut(row(8.0, Vec2::new(2.5, 1.7)), 2.3, RoofStyle::Boat, Material::Wattle, true, false, colors(3, 4, 1), 0.3));
    // On stilts over the cove's edge, with fish and nets hanging under the eaves.
    let stilt_c = f.local(stilt_shore + 0.4, stilt_b);
    d.add(hut(Footprint::Rect { center: stilt_c, half: Vec2::new(2.3, 1.6), angle: along_cove }, 2.2, RoofStyle::Turf, Material::Plank, false, true, colors(1, 1, 4), 0.5));
    // Stone hut by the field dyke.
    d.add(hut(Footprint::Rect { center: s(-1.0, -12.0), half: Vec2::new(2.0, 1.6), angle: f.angle(1.0, 0.25) }, 2.2, RoofStyle::Thatch, Material::Stone, false, false, colors(5, 2, 2), 0.7));
    // Roundhouses: Malachite thatch towards the headland, a turf dome at the back.
    d.add(hut(Footprint::Round { center: s(-13.0, 0.5), radius: 2.6 }, 2.0, RoofStyle::Thatch, Material::Wattle, false, false, colors(4, 1, 2), 0.2));
    d.add(hut(Footprint::Round { center: s(6.5, -15.0), radius: 2.3 }, 1.8, RoofStyle::Turf, Material::Stone, false, false, colors(1, 2, 4), 0.5));
    // The beacon on the headland, with a thatch cone over the fire basket.
    d.add(hut(Footprint::Round { center: beacon, radius: 1.5 }, 6.0, RoofStyle::Thatch, Material::Stone, false, false, colors(1, 0, 0), 0.4));

    // ------------------------------------------------------------------ the cove
    // Jetty: a plank deck from the beach out over the water, its deck ~1.1 m above the sea.
    let jetty_c = f.local(jetty_shore + 2.5, jetty_b);
    let jetty_half = Vec2::new(5.0, 1.0);
    let gmin = (0..=4)
        .flat_map(|i| (0..=4).map(move |j| (i, j)))
        .map(|(i, j)| {
            let l = Vec2::new(-jetty_half.x + 2.0 * jetty_half.x * i as f32 / 4.0, -jetty_half.y + 2.0 * jetty_half.y * j as f32 / 4.0);
            t.height(jetty_c + Vec2::from_angle(along_cove).rotate(l))
        })
        .fold(f32::MAX, f32::min);
    d.add(Structure::Platform(Platform { center: jetty_c, half: jetty_half, angle: along_cove, height: (SEA_LEVEL + 1.1 - gmin).max(0.4), colors: colors(0, 0, 0) }));
    // A canoe afloat in the cove (a Boat marker), two drawn up on the sand.
    d.add(placed(f.local(jetty_shore + 5.5, 3.0), along_cove + 0.4, 1.0, PropKind::Clutter(ClutterKind::Canoe), colors(0, 0, 0)));
    for (b, trim) in [(-4.8, 0u8), (-6.9, 2)] {
        let a = shore_a(t, &f, b) - 1.8;
        d.add(placed(f.local(a, b), along_cove + 0.15 * b.signum(), 1.0, PropKind::Clutter(ClutterKind::Canoe), colors(0, 0, trim)));
    }
    // Breakwater: from the spit-side beach out across the cove mouth.
    let br_b = -9.5;
    let br_a = shore_a(t, &f, br_b) - 3.0;
    let mut mole = dyke(vec![f.local(br_a, br_b), f.local(br_a + 7.0, br_b + 0.5), f.local(br_a + 10.5, br_b + 3.0)], WallKind::DryStone, 1.2, colors(0, 0, 3), 0.6);
    mole.thickness = 1.8;
    mole.footing = 0.5;
    mole.crest = false;
    d.add(Structure::Wall(mole));

    // ------------------------------------------------------------------ dykes
    // Dry-stone field ring; the shell track passes through it (gaps with gateposts).
    let ring = vec![s(-14.0, -16.0), s(-4.0, -16.5), s(-3.0, -9.5), s(-13.0, -8.0), s(-14.0, -16.0)];
    d.add(Structure::Wall(dyke(ring, WallKind::DryStone, 1.3, colors(0, 0, 0), 0.4)));
    // Turf dyke with thrift on its crest, back right.
    d.add(Structure::Wall(dyke(vec![s(3.0, -19.5), s(10.0, -19.0), s(16.0, -15.5)], WallKind::Turf, 1.2, colors(0, 0, 1), 0.2)));
    // Palisade guarding the headland path, stake tips painted.
    d.add(Structure::Wall(dyke(vec![s(-19.0, -5.0), s(-17.5, 3.0)], WallKind::Palisade, 1.7, colors(0, 0, 3), 0.3)));
    // A tumbled old dyke at the back left.
    let mut ruin = dyke(vec![s(-19.0, -14.0), s(-18.0, -9.0)], WallKind::DryStone, 1.0, colors(0, 0, 0), 0.9);
    ruin.tumbled = true;
    ruin.crest = false;
    ruin.footing = 0.3;
    d.add(Structure::Wall(ruin));

    // ------------------------------------------------------------------ tracks
    let path = |pts: Vec<Vec2>, width: f32, kind: PathKind| Structure::Path(Path { points: pts, width, kind });
    // Shell track: jetty root → hamlet green → between the huts → through the field ring.
    let root = f.local(jetty_shore - 2.5, jetty_b);
    d.add(path(vec![root, s(2.5, 5.0), s(-4.0, 2.0), s(-4.0, -4.0), s(-8.0, -11.0), s(-10.0, -20.0)], 1.6, PathKind::Shell));
    // Pebble track along the beach top.
    d.add(path((0..4).map(|k| { let b = -4.0 - 3.0 * k as f32; f.local(shore_a(t, &f, b) - 4.0, b) }).collect(), 1.4, PathKind::Pebble));
    // Boardwalk over the sand to the stilted hut.
    d.add(path(vec![f.local(stilt_shore - 7.5, stilt_b), f.local(stilt_shore - 2.2, stilt_b)], 1.5, PathKind::Boardwalk));
    // Flagstones from the green to the roundhouse and on to the headland steps.
    let steps_dir = (Vec2::ZERO - beacon).normalize_or(-plan.headland);
    let steps_top = beacon + steps_dir * 2.6;
    let mut steps_foot = steps_top + steps_dir * 8.0;
    for k in 3..30 {
        let q = steps_top + steps_dir * k as f32 * 0.5;
        if t.height(q) < t.height(steps_top) - 3.0 {
            steps_foot = q;
            break;
        }
    }
    d.add(path(vec![s(-4.0, 2.0), s(-10.0, 3.5), steps_foot - steps_dir * 0.3], 1.4, PathKind::Flagstone));
    // Stepping stones across the lagoon's front end.
    d.add(path(vec![s(10.5, -1.5), s(18.5, -1.5)], 1.2, PathKind::Stepping));

    // ------------------------------------------------------------------ steps
    d.add(Structure::Stairs(Stairs { from: on(t, steps_foot, 0.0), to: on(t, steps_top, 0.0), width: 1.3, kind: StairKind::Stone, colors: colors(0, 0, 0) }));
    // Log steps from the beach up the dune on the headland side.
    let dune_b = 9.0;
    let dune_a = shore_a(t, &f, dune_b);
    d.add(Structure::Stairs(Stairs { from: on(t, f.local(dune_a - 1.5, dune_b), 0.0), to: on(t, f.local(dune_a - 6.0, dune_b), 0.0), width: 1.0, kind: StairKind::Log, colors: colors(0, 0, 0) }));
    // A rope ladder up the seaward face of the sea stack.
    let (sc, srad, _) = stack;
    let out = sc.normalize_or(plan.headland);
    let ladder_top = sc + out * srad * 0.45;
    d.add(Structure::Stairs(Stairs {
        from: Vec3::new(sc.x + out.x * (srad + 0.6), SEA_LEVEL + 0.2, sc.y + out.y * (srad + 0.6)),
        to: on(t, ladder_top, 0.0),
        width: 0.6,
        kind: StairKind::RopeLadder,
        colors: colors(0, 0, 4),
    }));

    // ------------------------------------------------------------------ bridges
    let bridge = |a: Vec2, b: Vec2, kind: BridgeKind, hanging: Hanging, width: f32, c: Colors| {
        let y = t.height(a).max(t.height(b)) + 0.15;
        Structure::Bridge(Bridge { a: Vec3::new(a.x, y, a.y), b: Vec3::new(b.x, y, b.y), width, kind, hanging, colors: c })
    };
    d.add(bridge(s(10.5, -10.5), s(18.5, -10.5), BridgeKind::Causeway, Hanging::None, 1.8, colors(0, 0, 0)));
    d.add(bridge(s(10.5, -7.5), s(18.5, -7.5), BridgeKind::Log, Hanging::None, 1.4, colors(0, 0, 0)));
    d.add(bridge(s(10.5, -4.5), s(18.5, -4.5), BridgeKind::Rope { railing: 1 }, Hanging::Lanterns, 1.2, colors(0, 0, 2)));
    // Rope bridge from the headland cliff top out to the nearest sea stack.
    let toward = (beacon - sc).normalize_or(-out);
    let mut cliff = sc + toward * (srad + 2.0);
    for k in 0..60 {
        let q = sc + toward * (srad + k as f32 * 0.25);
        if t.height(q) > 2.5 {
            cliff = q + toward * 0.8;
            break;
        }
    }
    let stack_end = sc + toward * srad * 0.4;
    d.add(Structure::Bridge(Bridge { a: on(t, cliff, 0.1), b: on(t, stack_end, 0.1), width: 1.1, kind: BridgeKind::Rope { railing: 3 }, hanging: Hanging::Pennants, colors: colors(0, 0, 1) }));

    // ------------------------------------------------------------------ windbreaks
    let fence = |pts: Vec<Vec2>, kind: FenceKind, c: Colors| Structure::Fence(Fence { points: pts, kind, colors: c });
    for (b0, b1, trim) in [(-5.5, -8.5, 0u8), (-11.5, -14.5, 3)] {
        let line = vec![f.local(shore_a(t, &f, b0) - 3.2, b0), f.local(shore_a(t, &f, b1) - 3.2, b1)];
        d.add(fence(line, FenceKind::NetLine, colors(0, 0, trim)));
    }
    d.add(fence(vec![s(-3.0, -5.5), s(3.5, -5.5), s(4.5, -8.5)], FenceKind::Wattle, colors(0, 0, 0)));
    d.add(fence(vec![s(19.5, -13.0), s(20.0, -5.0)], FenceKind::Driftwood, colors(0, 0, 0)));
    d.add(fence(vec![s(9.5, -14.0), s(12.5, -12.5)], FenceKind::Reed, colors(0, 0, 0)));

    // ------------------------------------------------------------------ trees
    for b in [-12.5, 2.0, 12.0] {
        let a = shore_a(t, &f, b) - 2.8;
        d.add(placed(f.local(a, b), b * 0.7, 1.0, PropKind::Tree(TreeKind::Palm), Colors::default()));
    }
    d.add(placed(s(16.5, 1.5), 2.0, 0.9, PropKind::Tree(TreeKind::Palm), Colors::default()));
    let pine_a = beacon - plan.headland * 6.0 + f.side.perp() * 3.0;
    for (p, sc_) in [(pine_a, 1.0), (s(-17.0, -18.0), 1.1), (s(13.0, -20.0), 0.9)] {
        d.add(placed(p, p.x * 0.3, sc_, PropKind::Tree(TreeKind::SeaPine), Colors::default()));
    }
    for (p, sc_) in [(s(-16.5, -2.5), 1.0), (s(-2.0, -19.0), 0.9), (s(-15.5, 2.5), 0.8)] {
        d.add(placed(p, 0.0, sc_, PropKind::Tree(TreeKind::Cypress), Colors::default()));
    }
    for (p, sc_) in [(s(-11.5, -4.5), 1.0), (s(11.5, 2.0), 0.9)] {
        d.add(placed(p, p.y, sc_, PropKind::Tree(TreeKind::Fig), Colors::default()));
    }
    let snag_b = -17.0;
    d.add(placed(f.local(shore_a(t, &f, snag_b) - 2.5, snag_b), 0.6, 1.0, PropKind::Tree(TreeKind::Snag), Colors::default()));
    d.add(placed(beacon - plan.headland * 3.5 - f.side.perp() * 4.0, 2.2, 0.8, PropKind::Tree(TreeKind::Snag), Colors::default()));

    // ------------------------------------------------------------------ props
    use ClutterKind::*;
    let mut prop = |p: Vec2, angle: f32, k: ClutterKind, trim: u8| {
        d.add(placed(p, angle, 1.0, PropKind::Clutter(k), colors(0, 0, trim)));
    };
    prop(s(0.0, 3.8), 0.0, Firepit, 0);
    prop(s(-4.5, 6.0), along_u, DryingRack, 0);
    prop(s(5.0, 5.5), along_u + 0.3, DryingRack, 3);
    prop(s(-1.5, 6.8), 0.0, CarvedPole, 1);
    prop(s(5.5, 3.0), 0.0, CarvedPole, 4);
    prop(s(-6.0, 2.0), 0.4, Baskets, 3);
    prop(s(10.5, 1.8), 1.2, Baskets, 1);
    prop(s(11.0, -1.0), 0.0, Woodpile, 0);
    prop(f.local(jetty_shore - 1.5, jetty_b - 2.0), 0.3, CrabPots, 0);
    prop(f.local(jetty_shore - 2.5, jetty_b + 2.2), 1.1, CrabPots, 2);
    prop(f.local(jetty_shore - 3.5, jetty_b + 1.5), 0.0, LanternPost, 0);
    prop(s(-3.0, -1.5), 0.0, LanternPost, 0);
    prop(steps_foot - steps_dir * 1.5 + steps_dir.perp() * 1.6, 0.0, LanternPost, 0);
    prop(f.local(shore_a(t, &f, -10.0) - 5.5, -10.0), along_cove, NetRack, 4);
    prop(f.local(jetty_shore - 4.0, jetty_b - 2.0), 0.0, Pennant, 1);
    prop(beacon + steps_dir.perp() * 3.0, 0.0, Pennant, 4);
    prop(beacon - steps_dir * 2.5 + steps_dir.perp() * -2.0, 0.0, Cairn, 0);
    prop(f.local(shore_a(t, &f, -15.0) - 3.5, -15.0), 0.0, Cairn, 0);
    prop(s(-6.5, -13.0), 0.3, StandingStone, 2);
    prop(f.local(shore_a(t, &f, 11.0) - 1.0, 11.0), 0.5, Rock, 0);
    prop(s(17.5, -18.0), 1.4, Rock, 0);
    prop(s(-9.0, -6.0), 0.0, Bush, 0);
    prop(s(2.5, -10.0), 0.0, Bush, 0);
    d
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::buildings::needs_stilts;
    use crate::scene::build;
    use crate::MarkerKind;

    #[test]
    fn demo_builds_valid_meshes() {
        let doc = demo_document(7);
        let s = build(&doc);
        assert_eq!(s.parts.len(), doc.structures.len());
        let mut empty = 0;
        for p in &s.parts {
            assert!(p.mesh.is_valid(), "part {}", p.id);
            if p.mesh.is_empty() {
                // Only the canoe afloat in the cove is a marker without a mesh.
                assert!(matches!(doc.get(p.id), Some(Structure::Placed(pl)) if pl.kind == PropKind::Clutter(ClutterKind::Canoe)), "empty part {} {:?}", p.id, doc.get(p.id));
                empty += 1;
            }
        }
        assert_eq!(empty, 1);
        assert!(s.ground.is_valid() && !s.ground.is_empty());
        assert!(s.water.is_valid() && !s.water.is_empty(), "the lagoon");
        assert!(s.scatter.is_valid());
        for k in [MarkerKind::Fire, MarkerKind::Boat, MarkerKind::Lantern, MarkerKind::ChimneySmoke, MarkerKind::Pond] {
            assert!(s.markers.iter().any(|m| m.kind == k), "{k:?}");
        }
        assert!(s.markers.iter().all(|m| m.pos.is_finite() && m.dir.is_finite()));
        // Moored canoes at the jetty plus the one afloat.
        assert!(s.markers.iter().filter(|m| m.kind == MarkerKind::Boat).count() >= 2);
    }

    /// The composition holds together on other islands too.
    #[test]
    fn demo_builds_for_other_seeds() {
        for seed in [1, 42, 1234] {
            let doc = demo_document(seed);
            let s = build(&doc);
            assert!(s.parts.iter().all(|p| p.mesh.is_valid()));
            assert!(s.parts.iter().filter(|p| p.mesh.is_empty()).count() <= 1, "seed {seed}");
            for k in [MarkerKind::Fire, MarkerKind::Boat, MarkerKind::Lantern] {
                assert!(s.markers.iter().any(|m| m.kind == k), "seed {seed} {k:?}");
            }
        }
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
            assert!(has(&|s| matches!(s, Structure::House(h) if h.roof == r && matches!(h.footprint, Footprint::Rect { .. }))), "{r:?}");
        }
        for m in [Material::Wattle, Material::Plank, Material::Stone] {
            assert!(has(&|s| matches!(s, Structure::House(h) if h.material == m)), "{m:?}");
        }
        for k in [WallKind::DryStone, WallKind::Turf, WallKind::Palisade] {
            assert!(has(&|s| matches!(s, Structure::Wall(w) if w.kind == k)), "{k:?}");
        }
        assert!(has(&|s| matches!(s, Structure::House(h) if h.carved)));
        assert!(has(&|s| matches!(s, Structure::House(h) if h.hangings)));
        assert!(has(&|s| matches!(s, Structure::Bridge(b) if b.kind == BridgeKind::Causeway)));
        assert!(has(&|s| matches!(s, Structure::Bridge(b) if b.kind == BridgeKind::Log)));
        assert!(has(&|s| matches!(s, Structure::Bridge(b) if b.hanging == Hanging::Lanterns)));
        assert!(has(&|s| matches!(s, Structure::Bridge(b) if b.hanging == Hanging::Pennants)));
        assert!(has(&|s| matches!(s, Structure::Wall(w) if w.tumbled)));
        assert!(has(&|s| matches!(s, Structure::Wall(w) if w.crest)));
        assert!(has(&|s| matches!(s, Structure::Platform(_))));
        // Roundhouses (low) and a beacon (tall).
        let round = |tall: bool| has(&|s| matches!(s, Structure::House(h) if matches!(h.footprint, Footprint::Round { radius, .. } if (h.wall_height >= 2.5 * radius) == tall)));
        assert!(round(false) && round(true));
        // Every swatch appears at least once (DESIGN.md §Art).
        let cs: Vec<Colors> = doc
            .structures
            .iter()
            .filter_map(|(_, s)| match s {
                Structure::Wall(w) => Some(w.colors),
                Structure::House(h) => Some(h.colors),
                Structure::Fence(f) => Some(f.colors),
                Structure::Placed(p) => Some(p.colors),
                Structure::Bridge(b) => Some(b.colors),
                Structure::Stairs(t) => Some(t.colors),
                Structure::Platform(p) => Some(p.colors),
                Structure::Path(_) => None,
            })
            .collect();
        let houses: Vec<Colors> = doc.structures.iter().filter_map(|(_, s)| if let Structure::House(h) = s { Some(h.colors) } else { None }).collect();
        for i in 0..ROOF_COLORS {
            assert!(houses.iter().any(|c| c.roof == i), "roof swatch {i}");
        }
        for i in 0..WALL_COLORS {
            assert!(houses.iter().any(|c| c.wall == i), "wall swatch {i}");
        }
        for i in 0..TRIM_COLORS {
            assert!(cs.iter().any(|c| c.trim == i), "trim swatch {i}");
        }
        // Painted layers.
        for l in [PaintLayer::SeaPinks, PaintLayer::DuneGrass, PaintLayer::Wrack, PaintLayer::Pebbles] {
            assert!(doc.terrain.iter().any(|dab| matches!(dab.op, TerrainOp::Paint { layer, .. } if layer == l)), "{l:?}");
        }
        // Same seed, same document.
        assert_eq!(demo_document(7), doc);
    }

    /// The island-specific pieces land where they belong: jetty and breakwater out over the
    /// sea, the stilted hut over the water and NOT flattened, the beacon high on the headland,
    /// the dyke ring cut by the shell track.
    #[test]
    fn demo_sits_on_the_island() {
        let doc = demo_document(7);
        let s = build(&doc);
        let raw = Terrain::build(doc.seed, &doc.terrain, &[]);
        let t = &s.terrain;
        let mut stilted = 0;
        for (_, st) in &doc.structures {
            match st {
                Structure::Platform(p) => assert!(t.is_sea(p.center + Vec2::from_angle(p.angle) * p.half.x * 0.8), "jetty head over the sea"),
                Structure::Wall(w) if w.footing > 0.4 => assert!(w.points.iter().skip(1).all(|p| t.is_sea(*p)), "breakwater over the sea"),
                Structure::House(h) if needs_stilts(h, &raw) => {
                    stilted += 1;
                    let poly = crate::geom::footprint_polygon(&h.footprint);
                    assert!(poly.iter().any(|p| t.is_sea(*p)));
                    for p in poly {
                        assert_eq!(t.height(p), raw.height(p), "ground under a stilted hut is left alone");
                    }
                }
                Structure::House(h) => {
                    if let Footprint::Round { center, radius } = h.footprint {
                        if h.wall_height >= 2.5 * radius {
                            assert!(t.height(center) > 4.0, "beacon on the headland");
                        }
                    }
                }
                _ => {}
            }
        }
        assert_eq!(stilted, 1);
        // The shell track crosses the field ring twice.
        let ring = doc.structures.iter().find_map(|(_, st)| match st {
            Structure::Wall(w) if crate::geom::is_closed(&w.points) => Some(w.points.clone()),
            _ => None,
        });
        let track = doc.structures.iter().find_map(|(_, st)| match st {
            Structure::Path(p) if p.kind == PathKind::Shell => Some(p.points.clone()),
            _ => None,
        });
        assert_eq!(crate::geom::polyline_crossings(&ring.unwrap(), &track.unwrap()).len(), 2);
    }

    /// `cargo test -p cairn_cove_core --release -- --ignored --nocapture demo_timing`
    #[test]
    #[ignore]
    fn demo_timing() {
        let doc = demo_document(7);
        let t = std::time::Instant::now();
        let s = build(&doc);
        let tris: usize = s.parts.iter().map(|p| p.mesh.indices.len() / 3).sum::<usize>()
            + (s.ground.indices.len() + s.water.indices.len() + s.scatter.indices.len()) / 3;
        println!("build: {:?}, triangles: {tris}, ground tris: {}, scatter tris: {}", t.elapsed(), s.ground.indices.len() / 3, s.scatter.indices.len() / 3);
    }
}
