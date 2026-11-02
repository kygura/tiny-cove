//! The live document and its built scene, kept in sync with ECS entities.
//!
//! Contract used by tools / ui / life / shot:
//! - [`Glade`] resource: undo history + an optional live preview document. Tools call
//!   `set_preview` while dragging and `commit` on release; the world rebuilds the scene from
//!   `doc()` whenever `revision` changes.
//! - [`BuiltScene`] resource: the latest built `cairn_cove_core::Scene` (terrain, markers…), for
//!   picking (`cairn_cove_core::scene::pick`) and for life/weather to read markers and terrain.
//! - Entities: every spawned scene entity carries [`SceneEntity`]; structure parts also carry
//!   [`PartOf`] with their structure id (for hover highlight / eraser).
//!
//! Sync is incremental: only parts whose mesh changed are respawned. Committed documents are
//! built at once; previews (a drag in progress) are built on the async compute pool, one at a
//! time, so a ~36 ms build never stalls a frame. A preview result that a commit or undo has
//! overtaken is dropped. [`BuiltInfo`] says which revision the shown scene is and whether it is
//! a preview.
//!
//! Lights: [`warm_light`] is the one rule for every flame and warm light (life's flame
//! particles use it too). Lit fires and lanterns become a small pool of point lights placed at
//! the markers nearest to the camera focus.

use crate::camera::CameraRig;
use crate::env::Environment;
use crate::render::{
    GladeAssets, RenderSetup, lantern_light,
    sea::{FIRE_LUMENS, flicker},
    to_bevy_mesh,
};
use crate::shot::Cli;
use bevy::{
    light::NotShadowCaster,
    prelude::*,
    tasks::{AsyncComputeTaskPool, Task, futures::check_ready},
};
use cairn_cove_core::{Document, History, MarkerKind, MeshData, Scene, StructureId, terrain::Terrain};
use std::{collections::HashMap, sync::Arc};

#[derive(Resource)]
pub struct Glade {
    history: History,
    preview: Option<Document>,
    /// Bumped on every change of `doc()`.
    pub revision: u64,
}

impl Glade {
    pub fn new(doc: Document) -> Self {
        Self { history: History::new(doc), preview: None, revision: 1 }
    }
    /// What should be shown: the preview if any, else the committed document.
    pub fn doc(&self) -> &Document {
        self.preview.as_ref().unwrap_or(self.history.current())
    }
    /// The committed document (ignores the preview).
    pub fn committed(&self) -> &Document {
        self.history.current()
    }
    pub fn has_preview(&self) -> bool {
        self.preview.is_some()
    }
    pub fn set_preview(&mut self, doc: Option<Document>) {
        if self.preview != doc {
            self.preview = doc;
            self.revision += 1;
        }
    }
    /// Commits `doc` (clears any preview).
    pub fn commit(&mut self, doc: Document) {
        self.preview = None;
        self.history.push(doc);
        self.revision += 1;
    }
    pub fn undo(&mut self) -> bool {
        let had_preview = self.preview.take().is_some();
        let ok = self.history.undo();
        if ok || had_preview {
            self.revision += 1;
        }
        ok
    }
    pub fn redo(&mut self) -> bool {
        let had_preview = self.preview.take().is_some();
        let ok = self.history.redo();
        if ok || had_preview {
            self.revision += 1;
        }
        ok
    }
    pub fn can_undo(&self) -> bool {
        self.history.can_undo()
    }
    pub fn can_redo(&self) -> bool {
        self.history.can_redo()
    }
    /// Replace everything (new glade): fresh history.
    pub fn reset(&mut self, doc: Document) {
        *self = Self { revision: self.revision + 1, ..Self::new(doc) };
    }
}

/// Latest built scene (`None` until the first build).
#[derive(Resource, Default, Clone)]
pub struct BuiltScene(pub Option<Arc<Scene>>);

/// Where the shown [`BuiltScene`] came from: the `Glade::revision` it was built for, and
/// whether that was a preview (life ignores preview scenes).
#[derive(Resource, Default, Clone, Copy, Debug, PartialEq, Eq)]
pub struct BuiltInfo {
    pub revision: u64,
    pub preview: bool,
}

/// Marker on every entity the world spawns from the scene.
#[derive(Component)]
pub struct SceneEntity;

/// Structure a mesh entity belongs to.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub struct PartOf(pub StructureId);

/// Maximum simultaneously lit lanterns.
pub const MAX_LANTERNS: usize = 12;
const LANTERN_LUMENS: f32 = 1500.0;
const LANTERN_RANGE: f32 = 10.0;
/// Camera focus movement (m) that triggers re-picking the nearest lanterns.
const REPICK_DISTANCE: f32 = 3.0;
/// Fire light colour (DESIGN: `#FF8C29`); lanterns use `lantern_light`'s `#FFA542`.
const FIRE_COLOR: Color = Color::srgb(1.0, 140.0 / 255.0, 41.0 / 255.0);
const LANTERN_COLOR: Color = Color::srgb(1.0, 165.0 / 255.0, 66.0 / 255.0);

/// The kinds of warm light in the glade.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum WarmLight {
    /// The fire basket on top of a beacon tower.
    Beacon,
    /// A firepit on the ground.
    Pit,
    /// Lanterns and window slots.
    Lantern,
}

impl WarmLight {
    /// A `Fire` marker well above the ground is a beacon basket, else a firepit.
    pub fn of_fire(pos: Vec3, terrain: &Terrain) -> Self {
        if pos.y > terrain.height(pos.xz()) + 2.0 { Self::Beacon } else { Self::Pit }
    }
}

/// The one lighting rule (DESIGN A4, A7) for flames, point lights and glow: how strongly a
/// warm light of `kind` shines, 0..1, at `hour` with `storm` and `fog` (0..1 weather weights,
/// e.g. `Environment::storm` and `Environment::sea_fog`).
///
/// Beacons burn 19:30-06:00 and in storms and sea fog. Firepits burn 18:00-07:00 whatever the
/// weather. Lanterns and windows follow the clock only (from 19:00, full by 19:30, out by
/// 06:00), so a daytime storm lights the beacons and nothing else. A light whose weight is
/// below one half has no flame; at 0 it has no light either.
pub fn warm_light(kind: WarmLight, hour: f32, storm: f32, fog: f32) -> f32 {
    let h = hour.rem_euclid(24.0);
    let on = |b: bool| if b { 1.0f32 } else { 0.0 };
    match kind {
        WarmLight::Beacon => on(h >= 19.5 || h < 6.0).max(storm).max(fog).clamp(0.0, 1.0),
        WarmLight::Pit => on(h >= 18.0 || h < 7.0),
        WarmLight::Lantern => crate::weather::emissive_weight(h),
    }
}

pub struct WorldPlugin;

impl Plugin for WorldPlugin {
    fn build(&self, app: &mut App) {
        let cli = app.world().get_resource::<Cli>().cloned().unwrap_or_default();
        let doc = if cli.demo || cli.shot.is_some() {
            cairn_cove_core::demo::demo_document(cli.seed())
        } else {
            Document::new(cli.seed())
        };
        app.init_resource::<BuiltScene>()
            .init_resource::<BuiltInfo>()
            .init_resource::<SyncState>()
            .insert_resource(Glade::new(doc))
            .add_systems(Startup, spawn_lantern_pool.after(RenderSetup))
            .add_systems(Update, sync_scene)
            // Last: after every other light writer, so this rule has the final say.
            .add_systems(Last, update_lanterns);
    }
}

#[derive(Resource, Default)]
struct SyncState {
    revision: u64,
    /// The preview build in flight, with the revision it is for.
    pending: Option<(u64, Task<Scene>)>,
    ground: Option<Entity>,
    water: Option<Entity>,
    scatter: Option<Entity>,
    parts: HashMap<StructureId, Entity>,
}

/// Keeps `slot` if the mesh is unchanged, else respawns it (or clears it when empty).
fn refresh(
    commands: &mut Commands,
    slot: &mut Option<Entity>,
    old: Option<&MeshData>,
    new: &MeshData,
    spawn: &mut dyn FnMut(&mut Commands, &MeshData) -> Entity,
) {
    if slot.is_some() && old == Some(new) {
        return;
    }
    if let Some(e) = slot.take() {
        commands.entity(e).despawn();
    }
    if !new.is_empty() {
        *slot = Some(spawn(commands, new));
    }
}

fn sync_scene(
    mut commands: Commands,
    glade: Res<Glade>,
    assets: Option<Res<GladeAssets>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut built: ResMut<BuiltScene>,
    mut info: ResMut<BuiltInfo>,
    mut state: ResMut<SyncState>,
) {
    let Some(assets) = assets else { return };
    let t0 = std::time::Instant::now();
    let preview = glade.has_preview() && built.0.is_some();
    // A finished preview build: show it unless a commit / undo / cancel has overtaken it.
    let mut ready = None;
    if let Some((rev, task)) = state.pending.as_mut()
        && let Some(scene) = check_ready(task)
    {
        let rev = *rev;
        state.pending = None;
        if preview && rev > state.revision {
            ready = Some((rev, scene));
        }
    }
    if !preview {
        // Committed documents build at once (picking and life want them this frame).
        state.pending = None;
    } else if state.pending.is_none() && glade.revision != ready.as_ref().map_or(state.revision, |r| r.0) {
        // One preview build at a time; newer gestures coalesce into the next one.
        let doc = glade.doc().clone();
        state.pending = Some((glade.revision, AsyncComputeTaskPool::get().spawn(async move { cairn_cove_core::build(&doc) })));
    }
    let (revision, scene) = match ready {
        Some(r) => r,
        None if preview || (built.0.is_some() && state.revision == glade.revision) => return,
        None => (glade.revision, cairn_cove_core::build(glade.doc())),
    };
    let scene = Arc::new(scene);
    let t_build = t0.elapsed();
    let old = built.0.take();
    let old = old.as_deref();

    let mut spawn = |c: &mut Commands, m: &MeshData, water: bool| {
        let mesh = Mesh3d(meshes.add(to_bevy_mesh(m)));
        if water {
            c.spawn((mesh, MeshMaterial3d(assets.water.clone()), SceneEntity, NotShadowCaster)).id()
        } else {
            c.spawn((mesh, MeshMaterial3d(assets.opaque.clone()), SceneEntity)).id()
        }
    };
    let st = &mut *state;
    refresh(&mut commands, &mut st.ground, old.map(|o| &o.ground), &scene.ground, &mut |c, m| spawn(c, m, false));
    refresh(&mut commands, &mut st.scatter, old.map(|o| &o.scatter), &scene.scatter, &mut |c, m| spawn(c, m, false));
    refresh(&mut commands, &mut st.water, old.map(|o| &o.water), &scene.water, &mut |c, m| spawn(c, m, true));

    let mut kept = HashMap::new();
    for part in &scene.parts {
        let same = old.and_then(|o| o.parts.iter().find(|p| p.id == part.id)).is_some_and(|p| p.mesh == part.mesh);
        let prev = match st.parts.remove(&part.id) {
            Some(e) if same => Some(e),
            Some(e) => {
                commands.entity(e).despawn();
                None
            }
            None => None,
        };
        let e = match prev {
            Some(e) => Some(e),
            None if part.mesh.is_empty() => None,
            None => {
                let e = spawn(&mut commands, &part.mesh, false);
                commands.entity(e).insert(PartOf(part.id));
                Some(e)
            }
        };
        if let Some(e) = e {
            kept.insert(part.id, e);
        }
    }
    for (_, e) in st.parts.drain() {
        commands.entity(e).despawn();
    }
    st.parts = kept;
    st.revision = revision;
    *info = BuiltInfo { revision, preview };
    debug!("scene sync ({}): build {:.1} ms, total {:.1} ms", if preview { "preview" } else { "commit" }, t_build.as_secs_f32() * 1e3, t0.elapsed().as_secs_f32() * 1e3);
    built.0 = Some(scene);
}

#[derive(Component)]
struct LanternLight;

fn spawn_lantern_pool(mut commands: Commands) {
    for _ in 0..MAX_LANTERNS {
        commands.spawn((lantern_light(0.0, LANTERN_RANGE), Transform::default(), Visibility::Hidden, LanternLight, SceneEntity));
    }
}

/// Positions of the `n` points nearest to `focus` (horizontal distance).
pub fn nearest_points(points: impl Iterator<Item = Vec3>, focus: Vec3, n: usize) -> Vec<Vec3> {
    nearest_by(points, |p| *p, focus, n)
}

/// The `n` items nearest to `focus` (horizontal distance of `pos(item)`), nearest first.
pub fn nearest_by<T>(items: impl Iterator<Item = T>, pos: impl Fn(&T) -> Vec3, focus: Vec3, n: usize) -> Vec<T> {
    let mut v: Vec<(f32, T)> = items.map(|t| ((pos(&t) - focus).xz().length_squared(), t)).collect();
    v.sort_by(|a, b| a.0.total_cmp(&b.0));
    v.into_iter().take(n).map(|(_, t)| t).collect()
}

/// Lit fires first (so a far beacon keeps its light and water streak), then lit lanterns,
/// nearest to `focus`, at most [`MAX_LANTERNS`]. Unlit fires get no light at all.
pub fn pick_lights(scene: &Scene, focus: Vec3, hour: f32, storm: f32, fog: f32) -> Vec<(Vec3, WarmLight)> {
    let lit = |k: WarmLight| warm_light(k, hour, storm, fog) > 0.01;
    let fires = scene.markers.iter().filter(|m| m.kind == MarkerKind::Fire).map(|m| (m.pos, WarmLight::of_fire(m.pos, &scene.terrain)));
    let mut out = nearest_by(fires.filter(|(_, k)| lit(*k)), |f| f.0, focus, MAX_LANTERNS);
    if lit(WarmLight::Lantern) {
        let lanterns = scene.markers.iter().filter(|m| m.kind == MarkerKind::Lantern).map(|m| (m.pos, WarmLight::Lantern));
        out.extend(nearest_by(lanterns, |f| f.0, focus, MAX_LANTERNS - out.len()));
    }
    out
}

/// Focus at the last pick, which kinds were lit then, and the picked lights.
type Picked = (Vec3, [bool; 3], Vec<(Vec3, WarmLight)>);

/// Places, colours and dims the light pool by [`warm_light`]. Runs in `Last` so it overrides
/// any other writer of these lights.
fn update_lanterns(
    built: Res<BuiltScene>,
    rig: Option<Res<CameraRig>>,
    env: Res<Environment>,
    time: Res<Time>,
    mut lights: Query<(&mut PointLight, &mut Transform, &mut Visibility), With<LanternLight>>,
    mut picked: Local<Picked>,
) {
    let focus = rig.map_or(Vec3::ZERO, |r| r.focus);
    let weight = |k: WarmLight| warm_light(k, env.hour, env.storm, env.sea_fog);
    let lit = [WarmLight::Beacon, WarmLight::Pit, WarmLight::Lantern].map(|k| weight(k) > 0.01);
    if built.is_changed() || lit != picked.1 || (picked.0 - focus).length() > REPICK_DISTANCE {
        let pts = built.0.as_ref().map(|s| pick_lights(s, focus, env.hour, env.storm, env.sea_fog)).unwrap_or_default();
        *picked = (focus, lit, pts);
    }
    let t = time.elapsed_secs();
    for (i, (mut light, mut tf, mut vis)) in lights.iter_mut().enumerate() {
        let Some(&(p, kind)) = picked.2.get(i) else {
            *vis = Visibility::Hidden;
            continue;
        };
        tf.translation = p;
        let w = weight(kind);
        if kind == WarmLight::Lantern {
            light.color = LANTERN_COLOR;
            light.intensity = LANTERN_LUMENS * w;
        } else {
            // Stepped ±12 % flicker at 8 Hz (DESIGN A7).
            light.color = FIRE_COLOR;
            light.intensity = FIRE_LUMENS * w * flicker(t, i as u32);
        }
        *vis = Visibility::Inherited;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn picks_nearest_first() {
        let pts = [Vec3::new(10.0, 0.0, 0.0), Vec3::new(1.0, 5.0, 0.0), Vec3::new(-4.0, 0.0, 0.0)];
        let got = nearest_points(pts.into_iter(), Vec3::ZERO, 2);
        assert_eq!(got, vec![pts[1], pts[2]]);
    }

    #[test]
    fn one_rule_for_every_warm_light() {
        use WarmLight::*;
        let w = |k, h, storm, fog| warm_light(k, h, storm, fog);
        // Night: everything burns.
        assert!([Beacon, Pit, Lantern].iter().all(|&k| w(k, 23.0, 0.0, 0.0) == 1.0));
        // Noon storm or sea fog: only beacons (DESIGN A4: "the only warm hue on screen").
        for (storm, fog) in [(1.0, 0.0), (0.0, 1.0)] {
            assert_eq!(w(Beacon, 14.0, storm, fog), 1.0);
            assert_eq!(w(Pit, 14.0, storm, fog), 0.0);
            assert_eq!(w(Lantern, 14.0, storm, fog), 0.0);
        }
        assert_eq!(w(Beacon, 12.0, 0.0, 0.0), 0.0);
        // Firepits light at 18:00, beacons at 19:30, lanterns from 19:00.
        assert_eq!((w(Pit, 18.5, 0.0, 0.0), w(Beacon, 18.5, 0.0, 0.0), w(Lantern, 18.5, 0.0, 0.0)), (1.0, 0.0, 0.0));
        assert!(w(Lantern, 19.25, 0.0, 0.0) > 0.0 && w(Beacon, 19.25, 0.0, 0.0) == 0.0);
        assert_eq!((w(Pit, 6.5, 0.0, 0.0), w(Beacon, 6.5, 0.0, 0.0), w(Pit, 7.5, 0.0, 0.0)), (1.0, 0.0, 0.0));
        // A storm blending in fades the beacon in with it.
        assert_eq!(w(Beacon, 12.0, 0.4, 0.0), 0.4);
    }

    #[test]
    fn daytime_storm_lights_only_the_beacon() {
        let scene = cairn_cove_core::build(&cairn_cove_core::demo::demo_document(7));
        let kinds = |h, storm, fog| pick_lights(&scene, Vec3::ZERO, h, storm, fog).into_iter().map(|(_, k)| k).collect::<Vec<_>>();
        let storm = kinds(14.0, 1.0, 0.0);
        assert!(!storm.is_empty() && storm.iter().all(|k| *k == WarmLight::Beacon), "{storm:?}");
        assert!(kinds(12.0, 0.0, 0.0).is_empty(), "clear noon: no warm light at all");
        let dusk = kinds(18.5, 0.0, 0.0);
        assert!(!dusk.is_empty() && dusk.iter().all(|k| *k == WarmLight::Pit), "18:30: firepits only, {dusk:?}");
        let night = kinds(23.0, 0.0, 0.0);
        assert!(night.contains(&WarmLight::Beacon) && night.contains(&WarmLight::Lantern) && night.len() <= MAX_LANTERNS);
    }

    #[test]
    fn previews_build_off_the_main_thread() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins);
        let doc = cairn_cove_core::demo::demo_document(7);
        app.insert_resource(Glade::new(doc.clone()))
            .insert_resource(GladeAssets {
                opaque: default(),
                water: default(),
                sea: default(),
                particle: default(),
                atlas: default(),
                heightfield: default(),
            })
            .init_resource::<Assets<Mesh>>()
            .init_resource::<BuiltScene>()
            .init_resource::<BuiltInfo>()
            .init_resource::<SyncState>()
            .add_systems(Update, sync_scene);
        app.update();
        let info = *app.world().resource::<BuiltInfo>();
        assert!(!info.preview && app.world().resource::<BuiltScene>().0.is_some(), "first build is immediate");

        // A preview: the frame that starts it does not build (or show) it.
        let mut moved = doc.clone();
        moved.structures.pop();
        app.world_mut().resource_mut::<Glade>().set_preview(Some(moved.clone()));
        let t0 = std::time::Instant::now();
        app.update();
        let frame = t0.elapsed();
        assert_eq!(*app.world().resource::<BuiltInfo>(), info, "preview frame shows the old scene");
        let mut frames = 0;
        while !app.world().resource::<BuiltInfo>().preview && frames < 2000 {
            app.update();
            frames += 1;
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        let shown = *app.world().resource::<BuiltInfo>();
        assert!(shown.preview && shown.revision == app.world().resource::<Glade>().revision, "preview arrives: {shown:?}");
        let parts = app.world().resource::<BuiltScene>().0.as_ref().map_or(0, |s| s.parts.len());
        let t1 = std::time::Instant::now();
        assert_eq!(parts, cairn_cove_core::build(&moved).parts.len());
        // For the record: the old path ran this whole build inside the frame.
        eprintln!("preview frame {:.2} ms vs a main-thread build {:.1} ms; shown after {frames} frames", frame.as_secs_f32() * 1e3, t1.elapsed().as_secs_f32() * 1e3);

        // A preview overtaken by a commit is dropped; the commit is built at once.
        let mut other = moved.clone();
        other.structures.pop();
        app.world_mut().resource_mut::<Glade>().set_preview(Some(other));
        app.update();
        app.world_mut().resource_mut::<Glade>().commit(doc.clone());
        app.update();
        let rev = app.world().resource::<Glade>().revision;
        assert_eq!(*app.world().resource::<BuiltInfo>(), BuiltInfo { revision: rev, preview: false });
        for _ in 0..50 {
            app.update();
        }
        assert_eq!(*app.world().resource::<BuiltInfo>(), BuiltInfo { revision: rev, preview: false }, "stale preview dropped");
        assert_eq!(app.world().resource::<BuiltScene>().0.as_ref().map(|s| s.parts.len()), Some(cairn_cove_core::build(&doc).parts.len()));
    }

    #[test]
    fn history_bumps_revision() {
        let mut g = Glade::new(Document::new(1));
        let r = g.revision;
        g.commit(Document::new(2));
        assert!(g.revision > r && g.can_undo());
    }
}
