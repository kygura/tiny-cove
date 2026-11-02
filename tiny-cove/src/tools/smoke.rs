//! Hidden `--tool-smoke [out.png]` self-test: drives every tool through its real gesture code
//! with synthetic mouse / keyboard input and a [`CursorOverride`], checks the committed
//! document after each gesture, logs `SMOKE ok|FAIL <step>`, optionally saves a screenshot and
//! exits with status 0 (all passed) or 1.

use super::{CursorOverride, Tool, ToolSettings, hash01, q2};
use crate::camera::CameraRig;
use crate::weather::Clock;
use crate::world::{BuiltScene, Glade};
use bevy::{
    input::{InputSystems, mouse::{AccumulatedMouseScroll, MouseScrollUnit}},
    prelude::*,
    render::view::screenshot::{Screenshot, save_to_disk},
};
use cairn_cove_core::{
    BridgeKind, ClutterKind, Document, FenceKind, Footprint, Hanging, Material, OpeningKind, PaintLayer, PathKind,
    PropKind, RoofStyle, StairKind, Structure, TerrainDab, TerrainOp, TreeKind, WallKind, buildings::is_beacon,
    geom::is_closed,
};
use std::path::PathBuf;

/// Frames to let the scene rebuild after a gesture before checking.
const SETTLE: u32 = 4;

type Check = Box<dyn Fn(&Document, &Document, &ToolSettings) -> Result<(), String> + Send + Sync>;
type SetFn = Box<dyn Fn(&mut ToolSettings) + Send + Sync>;

enum Act {
    Key(KeyCode),
    Set(SetFn),
    /// Cursor at ground (x, z) plus `dy` metres up.
    Aim(Vec2, f32),
    Px(Vec2),
    Press,
    Release,
    Shift(bool),
    Scroll(f32),
    Wait(u32),
    /// Undo straight on the Glade, like the HUD button does.
    Undo,
}

struct Step {
    name: String,
    acts: Vec<Act>,
    check: Check,
}

#[derive(Default, PartialEq)]
enum Stage {
    #[default]
    Starting,
    Running,
    Shooting,
}

#[derive(Resource, Default)]
struct Smoke {
    shot: Option<PathBuf>,
    steps: Vec<Step>,
    i: usize,
    j: usize,
    wait: u32,
    frames: u32,
    before: Option<Document>,
    passed: usize,
    failed: usize,
    stage: Stage,
    release_keys: Vec<KeyCode>,
}

pub(super) fn install(app: &mut App) {
    let args: Vec<String> = std::env::args().collect();
    let Some(k) = args.iter().position(|a| a == "--tool-smoke") else { return };
    let shot = args.get(k + 1).filter(|a| !a.starts_with("--")).map(PathBuf::from);
    app.insert_resource(Smoke { shot, steps: steps(), ..default() })
        .insert_resource(CursorOverride::default())
        .add_systems(Startup, |mut clock: ResMut<Clock>, mut glade: ResMut<Glade>, mut rig: ResMut<CameraRig>| {
            clock.hour = 12.0;
            clock.paused = true;
            rig.set_zoom(1.9);
            glade.reset(arena());
        })
        .add_systems(PreUpdate, drive.after(InputSystems));
}

// --- script ------------------------------------------------------------------------------------

/// Seed 7 with a level 1.5 m plateau over the middle of the island, so the scripted coordinates
/// land on dry flat ground whatever the island generator does at the edges. It is the committed
/// base of the history; every step's own edits come after it.
fn arena() -> Document {
    let mut d = Document::new(7);
    for i in -4..=4 {
        for j in -4..=4 {
            d.terrain.push(TerrainDab { center: Vec2::new(i as f32 * 6.0, j as f32 * 6.0), radius: 7.0, op: TerrainOp::Flatten { height: 1.5, strength: 1.0 } });
        }
    }
    d
}

/// Ground point from diorama coordinates: `s` = depth (+ towards the camera), `d` = screen-right.
fn sd(s: f32, d: f32) -> Vec2 {
    Vec2::new((s + d) * 0.5, (s - d) * 0.5)
}

/// Horizontal unit vector towards the default camera.
fn n() -> Vec2 {
    Vec2::ONE.normalize()
}

fn set(f: impl Fn(&mut ToolSettings) + Send + Sync + 'static) -> Act {
    Act::Set(Box::new(f))
}

fn drag(pts: &[Vec2]) -> Vec<Act> {
    let mut v = vec![Act::Aim(pts[0], 0.0), Act::Press];
    for w in pts.windows(2) {
        let k = (w[0].distance(w[1]) / 0.5).ceil().max(1.0) as usize;
        for i in 1..=k {
            v.push(Act::Aim(w[0].lerp(w[1], i as f32 / k as f32), 0.0));
        }
    }
    v.extend([Act::Wait(2), Act::Release]);
    v
}

fn click(p: Vec2, dy: f32) -> Vec<Act> {
    vec![Act::Aim(p, dy), Act::Wait(1), Act::Press, Act::Release]
}

/// Drag a footprint, release, lift by `px` pixels, click.
fn raise(a: Vec2, b: Vec2, px: f32) -> Vec<Act> {
    let mut v = drag(&[a, b]);
    v.extend([Act::Px(Vec2::new(0.0, -px)), Act::Wait(2), Act::Press, Act::Release, Act::Px(Vec2::ZERO)]);
    v
}

fn count(d: &Document, f: impl Fn(&Structure) -> bool) -> usize {
    d.structures.iter().filter(|(_, s)| f(s)).count()
}

/// Exactly one new structure matching `f` (the last one); returns it.
fn added<'a>(b: &Document, a: &'a Document, f: impl Fn(&Structure) -> bool + Copy) -> Result<&'a Structure, String> {
    let (nb, na) = (count(b, f), count(a, f));
    if na != nb + 1 {
        return Err(format!("expected one new structure, count {nb} -> {na} (total {} -> {})", b.structures.len(), a.structures.len()));
    }
    Ok(a.structures.iter().rev().find(|(_, s)| f(s)).map(|(_, s)| s).unwrap())
}

fn ok_if(c: bool, why: impl FnOnce() -> String) -> Result<(), String> {
    if c { Ok(()) } else { Err(why()) }
}

fn new_dabs<'a>(b: &Document, a: &'a Document) -> Result<&'a [cairn_cove_core::TerrainDab], String> {
    let n = b.terrain.len();
    ok_if(a.terrain.len() > n, || "no terrain dabs added".into())?;
    ok_if(a.structures == b.structures, || "structures changed".into())?;
    Ok(&a.terrain[n..])
}

fn step(name: impl Into<String>, acts: Vec<Act>, check: impl Fn(&Document, &Document, &ToolSettings) -> Result<(), String> + Send + Sync + 'static) -> Step {
    Step { name: name.into(), acts, check: Box::new(check) }
}

fn with(mut pre: Vec<Act>, rest: Vec<Act>) -> Vec<Act> {
    pre.extend(rest);
    pre
}

fn is_wall(s: &Structure) -> bool {
    matches!(s, Structure::Wall(_))
}

fn is_stairs(s: &Structure) -> bool {
    matches!(s, Structure::Stairs(_))
}

/// Point on a hut's camera-facing (+x) wall, a little inside the face so the ray meets the wall.
fn hut_face(c: Vec2) -> Vec2 {
    Vec2::new(c.x + 1.94, c.y + 0.3)
}

fn steps() -> Vec<Step> {
    let mut v = Vec::new();
    let key = |t: Tool| Act::Key(t.hotkey().0);

    // Dykes: every kind, crest / tumbled / footing / weathering, scroll height.
    let wa = sd(-14.0, -18.0);
    let wb = sd(-14.0, -6.0);
    v.push(step("dyke", with(vec![key(Tool::Dyke), set(|s| { s.crest = false; s.tumbled = false; })], drag(&[wa, wb])), |b, a, _| {
        match added(b, a, is_wall)? {
            Structure::Wall(w) => ok_if(!w.crest && !w.tumbled && w.kind == WallKind::DryStone && (w.height - 4.0).abs() < 0.01 && w.points.len() == 2, || format!("{w:?}")),
            _ => unreachable!(),
        }
    }));
    v.push(step(
        "dyke crest+tumbled+scroll height",
        with(vec![set(|s| { s.crest = true; s.tumbled = true; })], {
            let mut d = drag(&[sd(-20.0, -18.0), sd(-20.0, -10.0)]);
            let r = d.len() - 2;
            d.insert(r, Act::Scroll(4.0));
            d
        }),
        |b, a, s| match added(b, a, is_wall)? {
            Structure::Wall(w) => ok_if(w.crest && w.tumbled && (w.height - 6.0).abs() < 0.01 && s.wall_height == 6.0, || format!("height {}", w.height)),
            _ => unreachable!(),
        },
    ));
    for (s0, kind) in [(-24.0, WallKind::Turf), (-27.0, WallKind::Palisade)] {
        v.push(step(
            format!("dyke {kind:?} footing+weathering"),
            with(vec![set(move |s| { s.wall_kind = kind; s.footing = 1.0; s.weathering = 0.8; s.crest = false; s.tumbled = false; s.wall_height = 4.0; })], drag(&[sd(s0, -18.0), sd(s0, -10.0)])),
            move |b, a, _| match added(b, a, is_wall)? {
                Structure::Wall(w) => ok_if(w.kind == kind && (w.footing - 1.0).abs() < 1e-3 && (w.weathering - 0.8).abs() < 1e-3, || format!("{w:?}")),
                _ => unreachable!(),
            },
        ));
    }
    let c = Vec2::new(-4.0, -12.0);
    let ring = [c + Vec2::new(-3.0, -3.0), c + Vec2::new(3.0, -3.0), c + Vec2::new(3.0, 3.0), c + Vec2::new(-3.0, 3.0), c + Vec2::new(-3.0, -3.0)];
    v.push(step("dyke closed ring", with(vec![set(|s| { s.wall_kind = WallKind::DryStone; s.footing = 0.0; s.weathering = 0.25; s.crest = false; s.tumbled = false; s.wall_height = 4.0; })], drag(&ring)), |b, a, _| {
        match added(b, a, is_wall)? {
            Structure::Wall(w) => ok_if(is_closed(&w.points), || format!("not closed: {:?}", w.points)),
            _ => unreachable!(),
        }
    }));
    v.push(step(
        "roof scroll pitch + shift style",
        vec![key(Tool::Roof), set(|s| { s.roof = RoofStyle::Boat; s.roof_pitch_deg = 30.0; }), Act::Aim(c, 0.0), Act::Wait(1), Act::Scroll(2.0), Act::Wait(1), Act::Shift(true), Act::Wait(1), Act::Scroll(1.0), Act::Wait(1), Act::Shift(false)],
        |b, a, s| {
            ok_if(a == b, || "document changed".into())?;
            ok_if(s.roof_pitch_deg == 40.0 && s.roof == RoofStyle::Hide, || format!("pitch {} roof {:?}", s.roof_pitch_deg, s.roof))
        },
    ));
    v.push(step("roof on ring", with(vec![key(Tool::Roof), set(|s| s.roof = RoofStyle::Hide)], click(c, 0.0)), |b, a, _| {
        ok_if(count(b, |s| matches!(s, Structure::Wall(w) if w.roofed)) == 0, || "pre-roofed".into())?;
        ok_if(count(a, |s| matches!(s, Structure::Wall(w) if w.roofed && w.roof == RoofStyle::Hide)) == 1, || "ring not roofed".into())
    }));

    // Openings on the first dyke's camera-facing face: click cycles window -> door -> wide door -> none.
    let face = sd(-14.0, -15.0) + n() * 0.35;
    let first_wall_openings = |d: &Document| match d.structures.first() {
        Some((_, Structure::Wall(w))) => w.openings.iter().map(|o| o.kind).collect::<Vec<_>>(),
        _ => Vec::new(),
    };
    v.push(step("opening place", with(vec![key(Tool::Opening), set(|s| s.opening = OpeningKind::Window)], click(face, 2.0)), move |_, a, _| {
        let k = first_wall_openings(a);
        ok_if(k == [OpeningKind::Window], || format!("{k:?}"))
    }));
    for (name, want) in [("opening click cycles to door", vec![OpeningKind::Door]), ("opening click cycles to wide door", vec![OpeningKind::WideDoor]), ("opening click cycles to none", vec![])] {
        v.push(step(name, click(face, 2.0), move |_, a, _| {
            let k = first_wall_openings(a);
            ok_if(k == want, || format!("{k:?}"))
        }));
    }

    // Steps: log and ladder up a dyke top, stone on the flat.
    for (name, kind, p, run) in [("steps log to dyke top", StairKind::Log, sd(-14.0, -9.0), 4.6), ("steps ladder to dyke top", StairKind::RopeLadder, sd(-14.0, -12.0), 1.2)] {
        let acts = with(
            vec![key(Tool::Steps), set(move |s| s.stairs = kind)],
            vec![Act::Aim(p + n() * run, 0.0), Act::Wait(1), Act::Press, Act::Aim(p + n() * 2.0, 0.0), Act::Aim(p + n() * 0.35, 3.6), Act::Wait(3), Act::Release],
        );
        v.push(step(name, acts, move |b, a, _| match added(b, a, is_stairs)? {
            Structure::Stairs(st) => {
                let top = st.from.y.max(st.to.y);
                let low = st.from.y.min(st.to.y);
                ok_if(st.kind == kind && top - low > 2.5, || format!("kind {:?} from {} to {}", st.kind, st.from, st.to))
            }
            _ => unreachable!(),
        }));
    }
    v.push(step(
        "steps stone on the flat",
        with(vec![set(|s| s.stairs = StairKind::Stone)], drag(&[sd(-4.0, -16.0), sd(-4.0, -10.0)])),
        move |b, a, _| match added(b, a, is_stairs)? {
            Structure::Stairs(st) => ok_if(st.kind == StairKind::Stone, || format!("{:?}", st.kind)),
            _ => unreachable!(),
        },
    ));

    // Jetty: footprint, lift, click. Then steps up onto its deck.
    let deck_c = Vec2::new(-2.0, -2.0);
    v.push(step("jetty", with(vec![key(Tool::Jetty)], raise(Vec2::new(-3.5, -3.5), Vec2::new(-0.5, -0.5), 20.0)), |b, a, _| {
        match added(b, a, |s| matches!(s, Structure::Platform(_)))? {
            Structure::Platform(p) => ok_if(p.height > DECK_MIN && p.half.min_element() > 1.0, || format!("{p:?}")),
            _ => unreachable!(),
        }
    }));
    v.push(step(
        "steps to jetty deck",
        with(vec![key(Tool::Steps), set(|s| s.stairs = StairKind::Stone)], vec![Act::Aim(deck_c + n() * 7.0, 0.0), Act::Wait(1), Act::Press, Act::Aim(deck_c + n() * 3.0, 0.0), Act::Aim(deck_c, 0.0), Act::Wait(3), Act::Release]),
        move |b, a, _| match added(b, a, is_stairs)? {
            Structure::Stairs(st) => ok_if(st.to.y - st.from.y > 1.0 && st.to.xz().distance(deck_c) < 2.6, || format!("from {} to {}", st.from, st.to)),
            _ => unreachable!(),
        },
    ));

    // Bridges: rope with each railing and hanging, causeway, log.
    let bridges = [(BridgeKind::Rope { railing: 1 }, Hanging::Lanterns, 2.0), (BridgeKind::Causeway, Hanging::None, 5.0), (BridgeKind::Log, Hanging::None, 8.0), (BridgeKind::Rope { railing: 3 }, Hanging::Pennants, -1.0)];
    for (kind, hang, s0) in bridges {
        v.push(step(
            format!("bridge {kind:?} {hang:?}"),
            with(vec![key(Tool::Bridge), set(move |s| { s.bridge = kind; s.hanging = hang; })], drag(&[sd(s0, -20.0), sd(s0, -13.0)])),
            move |b, a, _| match added(b, a, |s| matches!(s, Structure::Bridge(_)))? {
                Structure::Bridge(br) => ok_if(br.kind == kind && br.hanging == hang, || format!("{:?} {:?}", br.kind, br.hanging)),
                _ => unreachable!(),
            },
        ));
    }

    // Windbreaks.
    for (i, kind) in [FenceKind::Driftwood, FenceKind::Wattle, FenceKind::NetLine, FenceKind::Reed].into_iter().enumerate() {
        let s0 = 2.0 + 2.0 * i as f32;
        v.push(step(
            format!("windbreak {kind:?}"),
            with(vec![key(Tool::Windbreak), set(move |s| { s.fence = kind; s.colors.trim = 3; })], drag(&[sd(s0, -9.0), sd(s0, -3.0)])),
            move |b, a, _| match added(b, a, |s| matches!(s, Structure::Fence(_)))? {
                Structure::Fence(f) => ok_if(f.kind == kind && f.colors.trim == 3, || format!("{:?}", f.kind)),
                _ => unreachable!(),
            },
        ));
    }

    // Tracks: every kind; the first one scrolls its width.
    for (i, kind) in [PathKind::Shell, PathKind::Pebble, PathKind::Flagstone, PathKind::Boardwalk, PathKind::Stepping].into_iter().enumerate() {
        let s0 = 2.0 * i as f32;
        let mut acts = with(vec![key(Tool::Track), set(move |s| { s.path = kind; s.path_width = 1.6; })], drag(&[sd(s0, 1.0), sd(s0, 6.0), sd(s0 + 0.5, 10.0)]));
        let want = if i == 0 {
            let r = acts.len() - 2;
            acts.insert(r, Act::Scroll(2.0));
            2.0
        } else {
            1.6
        };
        v.push(step(format!("track {kind:?}"), acts, move |b, a, _| match added(b, a, |s| matches!(s, Structure::Path(_)))? {
            Structure::Path(p) => ok_if(p.kind == kind && p.points.len() >= 2 && (p.width - want).abs() < 1e-3, || format!("{p:?}")),
            _ => unreachable!(),
        }));
    }

    // Huts: rectangle, lift, click, with every roof / material and the HUD toggles and sliders.
    let roofs = [RoofStyle::Boat, RoofStyle::Hide, RoofStyle::Thatch, RoofStyle::Turf];
    let mats = [Material::Stone, Material::Wattle, Material::Plank, Material::Wattle];
    let block_c = |i: usize| sd(12.0, -22.0 + 7.0 * i as f32);
    for (i, (roof, mat)) in roofs.into_iter().zip(mats).enumerate() {
        let cc = block_c(i);
        // Hut 0 is carved without hangings, hut 1 has custom colours, hut 2 stands on explicit stilts.
        let (carved, hangings, stilts, trim) = [(true, false, 0.0, 0u8), (false, true, 0.0, 4), (false, true, 1.5, 0), (false, true, 0.0, 0)][i];
        v.push(step(
            format!("hut {roof:?}"),
            with(vec![key(Tool::Hut), set(move |s| { s.roof = roof; s.material = mat; s.carved = carved; s.hangings = hangings; s.stilts = stilts; s.colors.trim = trim; s.colors.roof = 3; })], raise(cc - Vec2::splat(2.0), cc + Vec2::splat(2.0), 50.0)),
            move |b, a, _| match added(b, a, |s| matches!(s, Structure::House(h) if matches!(h.footprint, Footprint::Rect { .. })))? {
                Structure::House(h) => ok_if(
                    h.roof == roof && h.material == mat && h.wall_height > 3.0 && h.carved == carved && h.hangings == hangings && (h.stilts - stilts).abs() < 1e-3 && h.colors.trim == trim && h.colors.roof == 3,
                    || format!("{h:?}"),
                ),
                _ => unreachable!(),
            },
        ));
    }
    // Roundhouse: low and round; then raised tall it becomes a beacon (and the ghost flag clears).
    let tc = sd(-12.0, 20.0);
    v.push(step("roundhouse", with(vec![key(Tool::Roundhouse), set(|s| { s.roof = RoofStyle::Turf; s.stilts = 0.0; s.colors.roof = 0; })], raise(tc, tc + Vec2::new(2.5, 0.0), 8.0)), |b, a, s| {
        match added(b, a, |s| matches!(s, Structure::House(h) if matches!(h.footprint, Footprint::Round { .. })))? {
            Structure::House(h) => ok_if(h.roof == RoofStyle::Turf && !is_beacon(h) && !s.ghost_beacon, || format!("roof {:?} h {}", h.roof, h.wall_height)),
            _ => unreachable!(),
        }
    }));
    let bc = sd(-4.0, 22.0);
    v.push(step("beacon", with(vec![key(Tool::Roundhouse), set(|s| s.roof = RoofStyle::Thatch)], raise(bc, bc + Vec2::new(2.5, 0.0), 120.0)), |b, a, s| {
        match added(b, a, |s| matches!(s, Structure::House(h) if matches!(h.footprint, Footprint::Round { .. })))? {
            Structure::House(h) => ok_if(is_beacon(h) && !s.ghost_beacon, || format!("not a beacon: h {}", h.wall_height)),
            _ => unreachable!(),
        }
    }));

    // Steps up to a hut floor (stilted hut 2) and an opening in a hut wall.
    let h2 = block_c(2);
    v.push(step(
        "steps to hut floor",
        with(vec![key(Tool::Steps), set(|s| s.stairs = StairKind::Log)], vec![Act::Aim(h2 + n() * 6.0, 0.0), Act::Wait(1), Act::Press, Act::Aim(h2 + n() * 3.5, 0.0), Act::Aim(hut_face(h2), 1.0), Act::Wait(3), Act::Release]),
        move |b, a, _| match added(b, a, is_stairs)? {
            Structure::Stairs(st) => ok_if(st.to.y - st.from.y > 1.0 && st.to.xz().distance(h2) < 3.2, || format!("from {} to {}", st.from, st.to)),
            _ => unreachable!(),
        },
    ));
    let h3 = block_c(3);
    v.push(step("opening in a hut wall", with(vec![key(Tool::Opening), set(|s| s.opening = OpeningKind::Door)], click(hut_face(h3), 1.2)), |b, a, _| {
        let open = |d: &Document| d.structures.iter().map(|(_, s)| if let Structure::House(h) = s { h.openings.len() } else { 0 }).sum::<usize>();
        ok_if(open(a) == open(b) + 1, || format!("openings {} -> {}", open(b), open(a)))
    }));

    // Dye: Shift+click picks a hut's style (and names it), a plain click applies the strip to another.
    v.push(step(
        "dye shift+click picks",
        with(vec![key(Tool::Dye), set(|s| { s.material = Material::Wattle; s.roof = RoofStyle::Turf; }), Act::Shift(true)], with(click(block_c(0), 3.0), vec![Act::Shift(false)])),
        |b, a, s| {
            ok_if(a == b, || "document changed".into())?;
            ok_if(s.material == Material::Stone && s.roof == RoofStyle::Boat && s.dye == Some("HUT") && s.carved, || format!("{:?} {:?} {:?}", s.material, s.roof, s.dye))
        },
    ));
    v.push(step(
        "dye click applies",
        with(vec![set(|s| { s.material = Material::Plank; s.colors.wall = 2; s.weathering = 0.7; })], click(block_c(1), 3.0)),
        |b, a, _| {
            // The hut keeps its own (hide) roof: the Dye strip has no roof control.
            let dyed = |d: &Document| count(d, |s| matches!(s, Structure::House(h) if h.material == Material::Plank && h.roof == RoofStyle::Hide && h.colors.wall == 2 && (h.weathering - 0.7).abs() < 1e-3));
            ok_if(dyed(a) == dyed(b) + 1 && a.structures.len() == b.structures.len(), || format!("dyed {} -> {}", dyed(b), dyed(a)))
        },
    ));

    // Trees and props along the front edge.
    let slot = |k: usize| sd(22.0, -19.0 + 2.0 * k as f32);
    let trees = [TreeKind::Snag, TreeKind::SeaPine, TreeKind::Cypress, TreeKind::Fig, TreeKind::Palm];
    for (k, t) in trees.into_iter().enumerate() {
        v.push(step(format!("tree {t:?}"), with(vec![key(Tool::Tree), set(move |s| s.tree = t)], click(slot(k), 0.0)), move |b, a, _| {
            match added(b, a, |s| matches!(s, Structure::Placed(_)))? {
                Structure::Placed(p) => ok_if(p.kind == PropKind::Tree(t) && (0.9..=1.15).contains(&p.scale), || format!("{p:?}")),
                _ => unreachable!(),
            }
        }));
    }
    let clutter = [
        ClutterKind::Baskets,
        ClutterKind::CrabPots,
        ClutterKind::Firepit,
        ClutterKind::CarvedPole,
        ClutterKind::NetRack,
        ClutterKind::Canoe,
        ClutterKind::LanternPost,
        ClutterKind::Pennant,
        ClutterKind::DryingRack,
        ClutterKind::Woodpile,
        ClutterKind::Rock,
        ClutterKind::Bush,
        ClutterKind::Cairn,
        ClutterKind::StandingStone,
    ];
    for (k, c) in clutter.into_iter().enumerate() {
        v.push(step(format!("prop {c:?}"), with(vec![key(Tool::Prop), set(move |s| s.clutter = c)], click(slot(5 + k), 0.0)), move |b, a, _| {
            match added(b, a, |s| matches!(s, Structure::Placed(_)))? {
                Structure::Placed(p) => ok_if(p.kind == PropKind::Clutter(c), || format!("{:?}", p.kind)),
                _ => unreachable!(),
            }
        }));
    }
    // Prop: click holds the ghost, scroll rotates it 15 degrees per notch, release places it.
    let spot = slot(19);
    v.push(step(
        "prop click + scroll rotates",
        vec![key(Tool::Prop), set(|s| s.clutter = ClutterKind::Cairn), Act::Aim(spot, 0.0), Act::Wait(1), Act::Press, Act::Wait(1), Act::Scroll(2.0), Act::Wait(1), Act::Release],
        move |b, a, _| match added(b, a, |s| matches!(s, Structure::Placed(_)))? {
            Structure::Placed(p) => {
                let want = (hash01(q2(spot), 1) * std::f32::consts::TAU + 30f32.to_radians()).rem_euclid(std::f32::consts::TAU);
                ok_if((p.angle - want).abs() < 1e-3, || format!("angle {} want {want}", p.angle))
            }
            _ => unreachable!(),
        },
    ));
    v.push(step("erase", with(vec![key(Tool::Erase)], click(slot(5), 0.5)), |b, a, _| {
        let basket = |d: &Document| count(d, |s| matches!(s, Structure::Placed(p) if p.kind == PropKind::Clutter(ClutterKind::Baskets)));
        ok_if(a.structures.len() + 1 == b.structures.len() && basket(a) + 1 == basket(b), || format!("{} -> {}", b.structures.len(), a.structures.len()))
    }));

    // Erase takes exactly one structure per click, however many frames the button is held: two
    // cairns stacked on one spot, a 12-frame press, one cairn gone (and the other still there).
    let pile = slot(20);
    let cairns = |d: &Document| count(d, |s| matches!(s, Structure::Placed(p) if p.kind == PropKind::Clutter(ClutterKind::Cairn)));
    v.push(step("stack two cairns", with(vec![key(Tool::Prop), set(|s| s.clutter = ClutterKind::Cairn)], with(click(pile, 0.0), click(pile, 0.0))), move |b, a, _| {
        ok_if(cairns(a) == cairns(b) + 2, || format!("cairns {} -> {}", cairns(b), cairns(a)))
    }));
    v.push(step("erase held 12 frames takes one", vec![key(Tool::Erase), Act::Aim(pile, 0.5), Act::Wait(1), Act::Press, Act::Wait(12), Act::Release], move |b, a, _| {
        ok_if(a.structures.len() + 1 == b.structures.len() && cairns(a) + 1 == cairns(b), || format!("{} -> {} structures", b.structures.len(), a.structures.len()))
    }));
    v.push(step("erase held takes the other on a second click", vec![Act::Aim(pile, 0.5), Act::Wait(1), Act::Press, Act::Wait(8), Act::Release], move |b, a, _| {
        ok_if(a.structures.len() + 1 == b.structures.len() && cairns(a) + 1 == cairns(b), || format!("{} -> {} structures", b.structures.len(), a.structures.len()))
    }));

    // Dye recolours only: a click on a prop, a causeway and a firepit keeps their kinds.
    v.push(step(
        "dye click keeps prop and bridge kinds",
        with(
            vec![key(Tool::Dye), set(|s| { s.clutter = ClutterKind::Canoe; s.bridge = BridgeKind::Rope { railing: 0 }; s.hanging = Hanging::Lanterns; s.tree = TreeKind::Palm; s.colors.trim = 3; s.dye = None; })],
            with(click(slot(7), 0.5), click(sd(5.0, -16.5), 0.5)),
        ),
        |b, a, _| {
            ok_if(a.structures.len() == b.structures.len() && a != b, || "nothing dyed".into())?;
            for ((_, x), (_, y)) in b.structures.iter().zip(&a.structures) {
                match (x, y) {
                    (Structure::Placed(p), Structure::Placed(q)) => ok_if(p.kind == q.kind, || format!("prop {:?} -> {:?}", p.kind, q.kind))?,
                    (Structure::Bridge(p), Structure::Bridge(q)) => ok_if(p.kind == q.kind && p.hanging == q.hanging, || format!("bridge {:?} -> {:?}", p.kind, q.kind))?,
                    _ => {}
                }
            }
            ok_if(count(a, |s| matches!(s, Structure::Placed(p) if p.kind == PropKind::Clutter(ClutterKind::Firepit) && p.colors.trim == 3)) == 1, || "firepit not recoloured".into())
        },
    ));

    // Tree drag scatters trees along the path; one undo takes the whole scatter back.
    let trees_in = |d: &Document| count(d, |s| matches!(s, Structure::Placed(p) if matches!(p.kind, PropKind::Tree(_))));
    v.push(step("tree drag scatters", with(vec![key(Tool::Tree), set(|s| s.tree = TreeKind::Cypress)], drag(&[sd(26.0, -18.0), sd(26.0, -6.0)])), move |b, a, _| {
        ok_if(trees_in(a) >= trees_in(b) + 3 && a.structures.len() == b.structures.len() + (trees_in(a) - trees_in(b)), || format!("trees {} -> {}", trees_in(b), trees_in(a)))
    }));
    v.push(step("tree scatter is one undo", vec![Act::Undo, Act::Wait(2)], move |b, a, _| {
        ok_if(trees_in(b) >= trees_in(a) + 3 && a.structures.len() < b.structures.len(), || format!("trees {} -> {}", trees_in(b), trees_in(a)))
    }));

    // Terrain brushes.
    let stroke = |s0: f32| vec![sd(s0, 14.0), sd(s0, 22.0)];
    for (i, layer) in [PaintLayer::SeaPinks, PaintLayer::DuneGrass, PaintLayer::Wrack, PaintLayer::Pebbles].into_iter().enumerate() {
        v.push(step(
            format!("paint {layer:?}"),
            with(vec![key(Tool::Paint), set(move |s| s.paint = layer)], drag(&stroke(-4.0 + i as f32))),
            move |b, a, _| {
                let d = new_dabs(b, a)?;
                ok_if(d.iter().all(|d| matches!(d.op, TerrainOp::Paint { layer: l, strength } if l == layer && strength > 0.0)), || format!("{:?}", d[0].op))
            },
        ));
    }
    v.push(step("paint shift erases", with(vec![key(Tool::Paint), set(|s| s.paint = PaintLayer::DuneGrass), Act::Shift(true)], with(drag(&stroke(0.0)), vec![Act::Shift(false)])), |b, a, _| {
        let d = new_dabs(b, a)?;
        ok_if(d.iter().all(|d| matches!(d.op, TerrainOp::Paint { strength, .. } if strength < 0.0)), || format!("{:?}", d[0].op))
    }));
    v.push(step("paint Clear", with(vec![set(|s| s.paint = PaintLayer::Clear)], drag(&stroke(1.0))), |b, a, _| {
        let d = new_dabs(b, a)?;
        ok_if(d.iter().all(|d| matches!(d.op, TerrainOp::Paint { layer: PaintLayer::Clear, .. })), || format!("{:?}", d[0].op))
    }));
    let sculpt = |up: bool| move |b: &Document, a: &Document, _: &ToolSettings| {
        let d = new_dabs(b, a)?;
        ok_if(d.iter().all(|d| matches!(d.op, TerrainOp::Sculpt { amount } if (amount > 0.0) == up)), || format!("{:?}", d[0].op))
    };
    v.push(step("raise", with(vec![key(Tool::Raise), set(|s| s.lower = false)], drag(&stroke(-8.0))), sculpt(true)));
    v.push(step("raise + shift lowers", with(vec![Act::Shift(true)], with(drag(&stroke(-8.0)), vec![Act::Shift(false)])), sculpt(false)));
    v.push(step("flatten", with(vec![key(Tool::Flatten)], drag(&stroke(-6.0))), |b, a, _| {
        let d = new_dabs(b, a)?;
        ok_if(d.iter().all(|d| matches!(d.op, TerrainOp::Flatten { .. })), || format!("{:?}", d[0].op))
    }));
    v.push(step("lagoon", with(vec![key(Tool::Lagoon)], drag(&[sd(4.0, 16.0), sd(4.0, 22.0)])), |b, a, _| {
        let d = new_dabs(b, a)?;
        ok_if(d.iter().all(|d| matches!(d.op, TerrainOp::Water { .. })), || format!("{:?}", d[0].op))
    }));
    let mut acts = vec![key(Tool::Dyke), Act::Aim(sd(-2.0, -22.0), 0.0), Act::Press];
    acts.extend((1..=8).map(|i| Act::Aim(sd(-2.0, -22.0 + i as f32), 0.0)));
    acts.extend([Act::Undo, Act::Wait(1)]);
    acts.extend((9..=12).map(|i| Act::Aim(sd(-2.0, -22.0 + i as f32), 0.0)));
    acts.extend([Act::Release, Act::Wait(2)]);
    v.push(step("undo mid-gesture cancels it", acts, |b, a, _| {
        ok_if(a.terrain.len() < b.terrain.len(), || "undo did not happen".into())?;
        ok_if(a.structures == b.structures, || format!("structures {} -> {}", b.structures.len(), a.structures.len()))
    }));
    // Leave the Hut strip open for the screenshot.
    v.push(step("hut strip for the screenshot", vec![key(Tool::Hut), set(|s| s.dye = None)], |b, a, s| ok_if(a == b && s.tool == Tool::Hut, || "tool not selected".into())));
    v
}

const DECK_MIN: f32 = 1.5;

// --- driver ------------------------------------------------------------------------------------

#[allow(clippy::too_many_arguments)]
fn drive(
    mut smoke: ResMut<Smoke>,
    mut mouse: ResMut<ButtonInput<MouseButton>>,
    mut keys: ResMut<ButtonInput<KeyCode>>,
    mut scroll: ResMut<AccumulatedMouseScroll>,
    mut cursor: ResMut<CursorOverride>,
    mut settings: ResMut<ToolSettings>,
    mut glade: ResMut<Glade>,
    built: Res<BuiltScene>,
    mut commands: Commands,
) {
    let sm = &mut *smoke;
    sm.frames += 1;
    for k in sm.release_keys.drain(..) {
        keys.release(k);
    }
    match sm.stage {
        Stage::Starting => {
            if built.0.is_some() && sm.frames > 20 {
                sm.stage = Stage::Running;
            }
            return;
        }
        Stage::Shooting => {
            let done = sm.shot.as_ref().is_none_or(|p| std::fs::metadata(p).is_ok_and(|m| m.len() > 0));
            if (done && sm.wait == 0) || sm.frames > 2000 {
                std::process::exit(if sm.failed == 0 { 0 } else { 1 });
            }
            sm.wait = sm.wait.saturating_sub(1);
            return;
        }
        Stage::Running => {}
    }
    if sm.wait > 0 {
        sm.wait -= 1;
        return;
    }
    if sm.i >= sm.steps.len() {
        let n = sm.passed + sm.failed;
        if sm.failed == 0 {
            println!("SMOKE PASS {}/{n}", sm.passed);
        } else {
            println!("SMOKE FAILED ({} of {n} failed)", sm.failed);
        }
        *cursor = CursorOverride::default();
        sm.stage = Stage::Shooting;
        sm.wait = 10;
        if let Some(p) = sm.shot.clone() {
            let _ = std::fs::remove_file(&p);
            commands.spawn(Screenshot::primary_window()).observe(save_to_disk(p));
        }
        return;
    }
    if sm.j == 0 {
        sm.before = Some(glade.committed().clone());
    }
    let (i, j) = (sm.i, sm.j);
    if j < sm.steps[i].acts.len() {
        sm.j += 1;
        match &sm.steps[i].acts[j] {
            Act::Key(k) => {
                keys.press(*k);
                sm.release_keys.push(*k);
            }
            Act::Set(f) => f(&mut settings),
            Act::Aim(p, dy) => {
                let h = built.0.as_ref().map_or(0.0, |s| s.terrain.height(*p));
                cursor.target = Some(Vec3::new(p.x, h + dy, p.y));
            }
            Act::Px(px) => cursor.px = *px,
            Act::Press => mouse.press(MouseButton::Left),
            Act::Release => mouse.release(MouseButton::Left),
            Act::Shift(on) => {
                if *on {
                    keys.press(KeyCode::ShiftLeft)
                } else {
                    keys.release(KeyCode::ShiftLeft)
                }
            }
            Act::Scroll(lines) => *scroll = AccumulatedMouseScroll { unit: MouseScrollUnit::Line, delta: Vec2::new(0.0, *lines) },
            Act::Wait(n) => sm.wait = *n,
            Act::Undo => {
                glade.undo();
            }
        }
        if sm.j == sm.steps[i].acts.len() {
            sm.wait = sm.wait.max(SETTLE);
        }
        return;
    }
    // Gesture done and settled: check.
    let before = sm.before.take().unwrap_or_else(|| glade.committed().clone());
    let step = &sm.steps[i];
    let res = if glade.has_preview() { Err("gesture still in progress (preview left)".to_string()) } else { Ok(()) }
        .and_then(|_| (step.check)(&before, glade.committed(), &settings));
    match res {
        Ok(()) => {
            println!("SMOKE ok {}", step.name);
            sm.passed += 1;
        }
        Err(e) => {
            println!("SMOKE FAIL {}: {e}", step.name);
            sm.failed += 1;
        }
    }
    mouse.release(MouseButton::Left);
    keys.release(KeyCode::ShiftLeft);
    sm.i += 1;
    sm.j = 0;
}
