//! The live document and its built scene, kept in sync with ECS entities.
//!
//! Contract used by tools / ui / life / shot:
//! - [`Glade`] resource: undo history + an optional live preview document. Tools call
//!   `set_preview` while dragging and `commit` on release; the world rebuilds the scene from
//!   `doc()` whenever `revision` changes.
//! - [`BuiltScene`] resource: the latest built `glade_core::Scene` (terrain, markers…), for
//!   picking (`glade_core::scene::pick`) and for life/weather to read markers and terrain.
//! - Entities: every spawned scene entity carries [`SceneEntity`]; structure parts also carry
//!   [`PartOf`] with their structure id (for hover highlight / eraser).
//!
//! Sync is incremental: only parts whose mesh changed are respawned. Lanterns become a small
//! pool of point lights placed at the markers nearest to the camera focus.

use crate::camera::CameraRig;
use crate::env::Environment;
use crate::render::{GladeAssets, RenderSetup, lantern_light, to_bevy_mesh};
use crate::shot::Cli;
use bevy::{light::NotShadowCaster, prelude::*};
use glade_core::{Document, History, MarkerKind, MeshData, Scene, StructureId};
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
        self.preview = None;
        let ok = self.history.undo();
        self.revision += 1;
        ok
    }
    pub fn redo(&mut self) -> bool {
        self.preview = None;
        let ok = self.history.redo();
        self.revision += 1;
        ok
    }
    pub fn can_undo(&self) -> bool {
        self.history.can_undo()
    }
    pub fn can_redo(&self) -> bool {
        self.history.can_redo()
    }
}

/// Latest built scene (`None` until the first build).
#[derive(Resource, Default, Clone)]
pub struct BuiltScene(pub Option<Arc<Scene>>);

/// Marker on every entity the world spawns from the scene.
#[derive(Component)]
pub struct SceneEntity;

/// Structure a mesh entity belongs to.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub struct PartOf(pub StructureId);

/// Maximum simultaneously lit lanterns.
pub const MAX_LANTERNS: usize = 12;
const LANTERN_LUMENS: f32 = 1500.0;
const LANTERN_RANGE: f32 = 5.5;
/// Camera focus movement (m) that triggers re-picking the nearest lanterns.
const REPICK_DISTANCE: f32 = 3.0;

pub struct WorldPlugin;

impl Plugin for WorldPlugin {
    fn build(&self, app: &mut App) {
        let cli = app.world().get_resource::<Cli>().cloned().unwrap_or_default();
        let doc = if cli.showcase {
            glade_core::demo::showcase_document(cli.seed())
        } else if cli.demo || cli.shot.is_some() {
            glade_core::demo::demo_document(cli.seed())
        } else {
            Document::new(cli.seed())
        };
        app.init_resource::<BuiltScene>()
            .init_resource::<SyncState>()
            .insert_resource(Glade::new(doc))
            .add_systems(Startup, spawn_lantern_pool.after(RenderSetup))
            .add_systems(Update, (sync_scene, update_lanterns.after(sync_scene)));
    }
}

#[derive(Resource, Default)]
struct SyncState {
    revision: u64,
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
    mut state: ResMut<SyncState>,
) {
    let Some(assets) = assets else { return };
    if built.0.is_some() && state.revision == glade.revision {
        return;
    }
    let t0 = std::time::Instant::now();
    let scene = Arc::new(glade_core::scene::build_incremental(glade.doc(), built.0.as_deref()));
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
        let e = match st.parts.remove(&part.id).filter(|_| same) {
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
    st.revision = glade.revision;
    debug!("scene sync: build {:.1} ms, total {:.1} ms", t_build.as_secs_f32() * 1e3, t0.elapsed().as_secs_f32() * 1e3);
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
    let mut v: Vec<(f32, Vec3)> = points.map(|p| ((p - focus).xz().length_squared(), p)).collect();
    v.sort_by(|a, b| a.0.total_cmp(&b.0));
    v.into_iter().take(n).map(|(_, p)| p).collect()
}

fn update_lanterns(
    built: Res<BuiltScene>,
    rig: Option<Res<CameraRig>>,
    env: Res<Environment>,
    mut lights: Query<(&mut PointLight, &mut Transform, &mut Visibility), With<LanternLight>>,
    mut picked: Local<(Vec3, Vec<Vec3>)>,
) {
    let focus = rig.map_or(Vec3::ZERO, |r| r.focus);
    if built.is_changed() || (picked.0 - focus).length() > REPICK_DISTANCE {
        let pts = built.0.iter().flat_map(|s| s.markers.iter()).filter(|m| m.kind == MarkerKind::Lantern).map(|m| m.pos);
        *picked = (focus, nearest_points(pts, focus, MAX_LANTERNS));
    }
    let on = env.night > 0.01;
    for (i, (mut light, mut tf, mut vis)) in lights.iter_mut().enumerate() {
        match picked.1.get(i).filter(|_| on) {
            Some(p) => {
                tf.translation = *p;
                light.intensity = LANTERN_LUMENS * env.night;
                *vis = Visibility::Inherited;
            }
            None => *vis = Visibility::Hidden,
        }
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
    fn history_bumps_revision() {
        let mut g = Glade::new(Document::new(1));
        let r = g.revision;
        g.commit(Document::new(2));
        assert!(g.revision > r && g.can_undo());
    }
}
