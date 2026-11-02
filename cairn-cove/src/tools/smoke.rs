//! Hidden `--tool-smoke [out.png]` self-test: drives every tool through its real gesture code
//! with synthetic mouse / keyboard input and a [`CursorOverride`], checks the committed
//! document after each gesture, logs `SMOKE ok|FAIL <step>`, optionally saves a screenshot and
//! exits with status 0 (all passed) or 1.

use super::{CursorOverride, Tool, ToolSettings};
use crate::weather::Clock;
use crate::world::{BuiltScene, Glade};
use bevy::{
    input::{InputSystems, mouse::{AccumulatedMouseScroll, MouseScrollUnit}},
    prelude::*,
    render::view::screenshot::{Screenshot, save_to_disk},
};
use cairn_cove_core::{
    BridgeKind, ClutterKind, Document, FenceKind, Footprint, Material, OpeningKind, PaintLayer, PathKind, PropKind,
    RoofStyle, StairKind, Structure, TerrainOp, TreeKind, geom::is_closed,
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
        .add_systems(Startup, |mut clock: ResMut<Clock>, mut glade: ResMut<Glade>| {
            clock.hour = 12.0;
            clock.paused = true;
            // Fixed terrain so the scripted coordinates behave the same every run.
            glade.reset(Document::new(7));
        })
        .add_systems(PreUpdate, drive.after(InputSystems));
}

// --- script ------------------------------------------------------------------------------------

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

fn steps() -> Vec<Step> {
    let mut v = Vec::new();
    let key = |t: Tool| Act::Key(t.hotkey().0);

    // Walls.
    let wa = sd(-14.0, -18.0);
    let wb = sd(-14.0, -6.0);
    v.push(step("wall", with(vec![key(Tool::Wall), set(|s| { s.planters = false; s.ruined = false; })], drag(&[wa, wb])), |b, a, _| {
        match added(b, a, is_wall)? {
            Structure::Wall(w) => ok_if(!w.planters && !w.ruined && (w.height - 4.0).abs() < 0.01 && w.points.len() == 2, || format!("{w:?}")),
            _ => unreachable!(),
        }
    }));
    v.push(step(
        "wall planters+ruined+scroll height",
        with(vec![set(|s| { s.planters = true; s.ruined = true; })], {
            let mut d = drag(&[sd(-20.0, -18.0), sd(-20.0, -10.0)]);
            let r = d.len() - 2;
            d.insert(r, Act::Scroll(4.0));
            d
        }),
        |b, a, s| match added(b, a, is_wall)? {
            Structure::Wall(w) => ok_if(w.planters && w.ruined && (w.height - 6.0).abs() < 0.01 && s.wall_height == 6.0, || format!("height {}", w.height)),
            _ => unreachable!(),
        },
    ));
    let c = Vec2::new(-4.0, -12.0);
    let ring = [c + Vec2::new(-3.0, -3.0), c + Vec2::new(3.0, -3.0), c + Vec2::new(3.0, 3.0), c + Vec2::new(-3.0, 3.0), c + Vec2::new(-3.0, -3.0)];
    v.push(step("wall closed ring", with(vec![set(|s| { s.planters = false; s.ruined = false; s.wall_height = 4.0; })], drag(&ring)), |b, a, _| {
        match added(b, a, is_wall)? {
            Structure::Wall(w) => ok_if(is_closed(&w.points), || format!("not closed: {:?}", w.points)),
            _ => unreachable!(),
        }
    }));
    v.push(step(
        "roof scroll pitch + shift style",
        vec![key(Tool::Roof), set(|s| { s.roof = RoofStyle::Pitched; s.roof_pitch_deg = 30.0; }), Act::Aim(c, 0.0), Act::Wait(1), Act::Scroll(2.0), Act::Wait(1), Act::Shift(true), Act::Wait(1), Act::Scroll(1.0), Act::Wait(1), Act::Shift(false)],
        |b, a, s| {
            ok_if(a == b, || "document changed".into())?;
            ok_if(s.roof_pitch_deg == 40.0 && s.roof == RoofStyle::Sawtooth, || format!("pitch {} roof {:?}", s.roof_pitch_deg, s.roof))
        },
    ));
    v.push(step("roof on ring", with(vec![key(Tool::Roof), set(|s| s.roof = RoofStyle::Sawtooth)], click(c, 0.0)), |b, a, _| {
        ok_if(count(b, |s| matches!(s, Structure::Wall(w) if w.roofed)) == 0, || "pre-roofed".into())?;
        ok_if(count(a, |s| matches!(s, Structure::Wall(w) if w.roofed && w.roof == RoofStyle::Sawtooth)) == 1, || "ring not roofed".into())
    }));

    // Openings on the first wall's camera-facing face.
    let face = sd(-14.0, -15.0) + n() * 0.35;
    let first_wall_openings = |d: &Document| match d.structures.first() {
        Some((_, Structure::Wall(w))) => w.openings.iter().map(|o| o.kind).collect::<Vec<_>>(),
        _ => Vec::new(),
    };
    v.push(step("opening place", with(vec![key(Tool::Opening), set(|s| s.opening = OpeningKind::Window)], click(face, 2.0)), move |_, a, _| {
        let k = first_wall_openings(a);
        ok_if(k == [OpeningKind::Window], || format!("{k:?}"))
    }));
    v.push(step("opening upgrade", click(face, 2.0), move |_, a, _| {
        let k = first_wall_openings(a);
        ok_if(k == [OpeningKind::Door], || format!("{k:?}"))
    }));

    // Stairs.
    let is_stairs = |s: &Structure| matches!(s, Structure::Stairs(_));
    for (name, kind, p, run) in [("stairs steel to wall top", StairKind::Steel, sd(-14.0, -9.0), 4.6), ("stairs ladder to wall top", StairKind::Ladder, sd(-14.0, -12.0), 1.2)] {
        let acts = with(
            vec![key(Tool::Stairs), set(move |s| s.stairs = kind)],
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
        "stairs concrete",
        with(vec![set(|s| s.stairs = StairKind::Concrete)], drag(&[sd(-4.0, -16.0), sd(-4.0, -10.0)])),
        move |b, a, _| match added(b, a, is_stairs)? {
            Structure::Stairs(st) => ok_if(st.kind == StairKind::Concrete, || format!("{:?}", st.kind)),
            _ => unreachable!(),
        },
    ));

    // Deck.
    v.push(step("deck", with(vec![key(Tool::Deck)], raise(Vec2::new(-3.5, -3.5), Vec2::new(-0.5, -0.5), 20.0)), |b, a, _| {
        match added(b, a, |s| matches!(s, Structure::Platform(_)))? {
            Structure::Platform(p) => ok_if(p.height > DECK_MIN && p.half.min_element() > 1.0, || format!("{p:?}")),
            _ => unreachable!(),
        }
    }));

    // Bridges.
    for (i, kind) in [BridgeKind::Cable { railing: 1 }, BridgeKind::Arch, BridgeKind::Girder].into_iter().enumerate() {
        let s0 = 2.0 + 3.0 * i as f32;
        v.push(step(
            format!("bridge {kind:?}"),
            with(vec![key(Tool::Bridge), set(move |s| s.bridge = kind)], drag(&[sd(s0, -20.0), sd(s0, -13.0)])),
            move |b, a, _| match added(b, a, |s| matches!(s, Structure::Bridge(_)))? {
                Structure::Bridge(br) => ok_if(br.kind == kind, || format!("{:?}", br.kind)),
                _ => unreachable!(),
            },
        ));
    }

    // Fences.
    for (i, kind) in [FenceKind::Railing, FenceKind::Balustrade, FenceKind::Gabion, FenceKind::Hedge].into_iter().enumerate() {
        let s0 = 2.0 + 2.0 * i as f32;
        v.push(step(
            format!("fence {kind:?}"),
            with(vec![key(Tool::Fence), set(move |s| s.fence = kind)], drag(&[sd(s0, -9.0), sd(s0, -3.0)])),
            move |b, a, _| match added(b, a, |s| matches!(s, Structure::Fence(_)))? {
                Structure::Fence(f) => ok_if(f.kind == kind, || format!("{:?}", f.kind)),
                _ => unreachable!(),
            },
        ));
    }

    // Paths.
    for (i, kind) in [PathKind::Gravel, PathKind::Pavers, PathKind::Slabs, PathKind::Poured, PathKind::Stepping].into_iter().enumerate() {
        let s0 = 2.0 * i as f32;
        v.push(step(
            format!("path {kind:?}"),
            with(vec![key(Tool::Path), set(move |s| s.path = kind)], drag(&[sd(s0, 1.0), sd(s0, 6.0), sd(s0 + 0.5, 10.0)])),
            move |b, a, _| match added(b, a, |s| matches!(s, Structure::Path(_)))? {
                Structure::Path(p) => ok_if(p.kind == kind && p.points.len() >= 2, || format!("{p:?}")),
                _ => unreachable!(),
            },
        ));
    }

    // Blocks and tower.
    let roofs = [RoofStyle::Pitched, RoofStyle::Sawtooth, RoofStyle::Shell, RoofStyle::Green];
    let mats = [Material::RammedEarth, Material::Concrete, Material::Timber, Material::Concrete];
    let block_c = |i: usize| sd(12.0, -22.0 + 7.0 * i as f32);
    for (i, (roof, mat)) in roofs.into_iter().zip(mats).enumerate() {
        let cc = block_c(i);
        v.push(step(
            format!("block {roof:?}"),
            with(vec![key(Tool::Block), set(move |s| { s.roof = roof; s.material = mat; })], raise(cc - Vec2::splat(2.0), cc + Vec2::splat(2.0), 50.0)),
            move |b, a, _| match added(b, a, |s| matches!(s, Structure::House(h) if matches!(h.footprint, Footprint::Rect { .. })))? {
                Structure::House(h) => ok_if(h.roof == roof && h.material == mat && h.wall_height > 5.5, || format!("roof {:?} h {}", h.roof, h.wall_height)),
                _ => unreachable!(),
            },
        ));
    }
    let tc = sd(-12.0, 20.0);
    v.push(step("tower", with(vec![key(Tool::Tower), set(|s| s.roof = RoofStyle::Green)], raise(tc, tc + Vec2::new(2.5, 0.0), 30.0)), |b, a, _| {
        match added(b, a, |s| matches!(s, Structure::House(h) if matches!(h.footprint, Footprint::Round { .. })))? {
            Structure::House(h) => ok_if(h.roof == RoofStyle::Green && h.wall_height > TOWER_MIN, || format!("roof {:?} h {}", h.roof, h.wall_height)),
            _ => unreachable!(),
        }
    }));

    // Style copy (block 0: earth + pitched) and apply (block 1 becomes timber).
    v.push(step(
        "style copy",
        with(vec![key(Tool::Style), set(|s| { s.material = Material::Concrete; s.roof = RoofStyle::Green; })], click(block_c(0), 3.0)),
        |b, a, s| {
            ok_if(a == b, || "document changed".into())?;
            ok_if(s.material == Material::RammedEarth && s.roof == RoofStyle::Pitched, || format!("{:?} {:?}", s.material, s.roof))
        },
    ));
    v.push(step(
        "style apply",
        with(vec![set(|s| s.material = Material::Timber), Act::Shift(true)], with(click(block_c(1), 3.0), vec![Act::Shift(false)])),
        |b, a, _| {
            let timber = |d: &Document| count(d, |s| matches!(s, Structure::House(h) if h.material == Material::Timber));
            ok_if(timber(a) == timber(b) + 1 && a.structures.len() == b.structures.len(), || format!("timber {} -> {}", timber(b), timber(a)))
        },
    ));

    // Trees and clutter along the front edge.
    let slot = |k: usize| sd(22.0, -19.0 + 2.0 * k as f32);
    let trees = [TreeKind::Oak, TreeKind::Pine, TreeKind::Birch, TreeKind::Fruit, TreeKind::Fern];
    for (k, t) in trees.into_iter().enumerate() {
        v.push(step(format!("tree {t:?}"), with(vec![key(Tool::Tree), set(move |s| s.tree = t)], click(slot(k), 0.0)), move |b, a, _| {
            match added(b, a, |s| matches!(s, Structure::Placed(_)))? {
                Structure::Placed(p) => ok_if(p.kind == PropKind::Tree(t) && (0.9..=1.15).contains(&p.scale), || format!("{p:?}")),
                _ => unreachable!(),
            }
        }));
    }
    let clutter = [
        ClutterKind::RainBarrel,
        ClutterKind::Planter,
        ClutterKind::Compost,
        ClutterKind::Cistern,
        ClutterKind::Wheelbarrow,
        ClutterKind::Bench,
        ClutterKind::LampPost,
        ClutterKind::WindTurbine,
        ClutterKind::WashingLine,
        ClutterKind::LogPile,
        ClutterKind::Rock,
        ClutterKind::Bush,
        ClutterKind::Beehive,
        ClutterKind::Solar,
    ];
    for (k, c) in clutter.into_iter().enumerate() {
        v.push(step(format!("clutter {c:?}"), with(vec![key(Tool::Clutter), set(move |s| s.clutter = c)], click(slot(5 + k), 0.0)), move |b, a, _| {
            match added(b, a, |s| matches!(s, Structure::Placed(_)))? {
                Structure::Placed(p) => ok_if(p.kind == PropKind::Clutter(c), || format!("{:?}", p.kind)),
                _ => unreachable!(),
            }
        }));
    }
    v.push(step("erase", with(vec![key(Tool::Erase)], click(slot(5), 0.5)), |b, a, _| {
        let barrel = |d: &Document| count(d, |s| matches!(s, Structure::Placed(p) if p.kind == PropKind::Clutter(ClutterKind::RainBarrel)));
        ok_if(a.structures.len() + 1 == b.structures.len() && barrel(a) + 1 == barrel(b), || format!("{} -> {}", b.structures.len(), a.structures.len()))
    }));

    // Terrain brushes.
    let stroke = |s0: f32| vec![sd(s0, 14.0), sd(s0, 22.0)];
    for (i, layer) in [PaintLayer::Flowers, PaintLayer::Ferns, PaintLayer::Moss, PaintLayer::Gravel].into_iter().enumerate() {
        v.push(step(
            format!("paint {layer:?}"),
            with(vec![key(Tool::Paint), set(move |s| s.paint = layer)], drag(&stroke(-4.0 + i as f32))),
            move |b, a, _| {
                let d = new_dabs(b, a)?;
                ok_if(d.iter().all(|d| matches!(d.op, TerrainOp::Paint { layer: l, strength } if l == layer && strength > 0.0)), || format!("{:?}", d[0].op))
            },
        ));
    }
    let sculpt = |up: bool| move |b: &Document, a: &Document, _: &ToolSettings| {
        let d = new_dabs(b, a)?;
        ok_if(d.iter().all(|d| matches!(d.op, TerrainOp::Sculpt { amount } if (amount > 0.0) == up)), || format!("{:?}", d[0].op))
    };
    v.push(step("raise", with(vec![key(Tool::Sculpt), set(|s| s.lower = false)], drag(&stroke(-8.0))), sculpt(true)));
    v.push(step("lower (shift)", with(vec![Act::Shift(true)], with(drag(&stroke(-8.0)), vec![Act::Shift(false)])), sculpt(false)));
    v.push(step("flatten", with(vec![key(Tool::Flatten)], drag(&stroke(-6.0))), |b, a, _| {
        let d = new_dabs(b, a)?;
        ok_if(d.iter().all(|d| matches!(d.op, TerrainOp::Flatten { .. })), || format!("{:?}", d[0].op))
    }));
    v.push(step("water", with(vec![key(Tool::Water)], drag(&[sd(4.0, 16.0), sd(4.0, 22.0)])), |b, a, _| {
        let d = new_dabs(b, a)?;
        ok_if(d.iter().all(|d| matches!(d.op, TerrainOp::Water { .. })), || format!("{:?}", d[0].op))
    }));
    let mut acts = vec![key(Tool::Wall), Act::Aim(sd(-2.0, -22.0), 0.0), Act::Press];
    acts.extend((1..=8).map(|i| Act::Aim(sd(-2.0, -22.0 + i as f32), 0.0)));
    acts.extend([Act::Undo, Act::Wait(1)]);
    acts.extend((9..=12).map(|i| Act::Aim(sd(-2.0, -22.0 + i as f32), 0.0)));
    acts.extend([Act::Release, Act::Wait(2)]);
    v.push(step("undo mid-gesture cancels it", acts, |b, a, _| {
        ok_if(a.terrain.len() < b.terrain.len(), || "undo did not happen".into())?;
        ok_if(a.structures == b.structures, || format!("structures {} -> {}", b.structures.len(), a.structures.len()))
    }));
    v
}

const DECK_MIN: f32 = 1.5;
const TOWER_MIN: f32 = 12.5;

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
