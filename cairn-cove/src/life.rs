//! Ambient life: gulls, crabs, leaping fish, bobbing canoes, flames, smoke and fireflies.
//!
//! Gulls, crabs and canoes are tiny low-poly rigs built once from `MeshData` (so they get the
//! PS1/cel material) and animated through `Transform`s only. Flames, smoke, fish splashes and
//! fireflies are four CPU particle clouds, one mesh each. Everything re-targets when
//! [`BuiltScene`] changes. Behaviour lives in `fauna` (creatures) and `fx` (particles).
//!
//! Inputs: `Environment` (hour, night, wetness, wind), `Weather` (beacons burn in storm / fog),
//! the tide computed as `Tide::at(hour)` (same constants as the sea) and the scene markers
//! `Perch`, `Fire`, `Boat` and `ChimneySmoke`.

mod fauna;
mod fx;

use crate::camera::CameraRig;
use crate::env::{Environment, Tide};
use crate::render::{GladeAssets, particle_mesh, to_bevy_mesh, write_particle_mesh};
use crate::weather::Weather;
use crate::world::{BuiltScene, Glade};
use bevy::{camera::visibility::NoFrustumCulling, prelude::*};
use cairn_cove_core::{
    MarkerKind, Scene, TRIM_COLORS,
    mesh::{Rng, hash_f, hash2},
    terrain::glade_dist,
};
use fauna::{Crab, Gull, GullWorld, Shore};
use fx::FireKind;
use std::{collections::HashSet, sync::Arc};

const GULLS: usize = 7;
const CRABS: usize = 6;
const MAX_BOATS: usize = 12;
const MAX_FIRES: usize = 10;
const MAX_CHIMNEYS: usize = 14;
const MAX_PERCHES: usize = 48;
const MAX_FLY_HOMES: usize = 14;
const MAX_LEAPS: usize = 2;
/// Fireflies stay this far inside the glade edge (m).
const EDGE_MARGIN: f32 = 4.0;
/// Fireflies spawn within this distance of the origin so they stay near the camera.
const ROAM_RADIUS: f32 = 20.0;
/// Fish leap within this distance of the camera focus when they can (m).
const LEAP_NEAR: f32 = 18.0;
const GULL_SCALE: f32 = 1.7;
const CRAB_SCALE: f32 = 2.2;
/// Seed for the canoe hull tints.
const LIFE_SEED: u64 = 0xCA17;

pub struct LifePlugin;

impl Plugin for LifePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<LifeWorld>().init_resource::<Flock>().init_resource::<Crabs>().init_resource::<Leaps>().add_systems(
            Update,
            (refresh_world, spawn_life, sync_boats, update_gulls, update_crabs, update_boats, update_leaps, update_clouds).chain(),
        );
    }
}

// ---------------------------------------------------------------- pure helpers

/// Coarse 1 m occupancy grid of structure geometry (xz), for avoidance.
#[derive(Default, Debug)]
pub struct Obstacles {
    cells: HashSet<(i32, i32)>,
}

impl Obstacles {
    /// Marks every cell touched by the triangles (sampled barycentrically, step <= 0.5 m).
    pub fn from_tris(tris: impl Iterator<Item = [Vec3; 3]>) -> Self {
        let mut cells = HashSet::new();
        for [a, b, c] in tris {
            let longest = (b - a).xz().length().max((c - a).xz().length()).max((c - b).xz().length());
            let n = ((longest / 0.5).ceil() as i32).clamp(1, 24);
            for i in 0..=n {
                for j in 0..=(n - i) {
                    let p = a + (b - a) * (i as f32 / n as f32) + (c - a) * (j as f32 / n as f32);
                    cells.insert((p.x.floor() as i32, p.z.floor() as i32));
                }
            }
        }
        Self { cells }
    }

    /// Structure parts of `scene`, ignoring flat low triangles (paths, floors, kerbs).
    pub fn from_scene(scene: &Scene) -> Self {
        let tris = scene.parts.iter().flat_map(|part| {
            let m = &part.mesh;
            m.indices.chunks_exact(3).filter_map(move |t| {
                let v = |i: u32| m.positions.get(i as usize).map(|p| Vec3::from_array(*p));
                let tri = [v(t[0])?, v(t[1])?, v(t[2])?];
                let (lo, hi) = tri.iter().fold((f32::MAX, f32::MIN), |(l, h), p| (l.min(p.y), h.max(p.y)));
                let ground = scene.terrain.height(tri[0].xz());
                (hi - lo >= 0.3 || lo > ground + 0.5).then_some(tri)
            })
        });
        Self::from_tris(tris)
    }

    /// True when something solid lies within about `r` metres of `p`.
    pub fn blocked(&self, p: Vec2, r: f32) -> bool {
        let (x0, x1) = ((p.x - r).floor() as i32, (p.x + r).floor() as i32);
        let (z0, z1) = ((p.y - r).floor() as i32, (p.y + r).floor() as i32);
        (x0..=x1).any(|x| (z0..=z1).any(|z| self.cells.contains(&(x, z))))
    }
}

/// A random spot within `radius` of `center` satisfying `ok`.
pub fn random_spot(rng: &mut Rng, center: Vec2, radius: f32, ok: &dyn Fn(Vec2) -> bool) -> Option<Vec2> {
    (0..80).find_map(|_| {
        let ang = rng.range(0.0, std::f32::consts::TAU);
        let p = center + Vec2::new(ang.cos(), ang.sin()) * radius * rng.f().sqrt();
        ok(p).then_some(p)
    })
}

/// Move `cur` towards `target` by at most `max` radians (shortest way).
fn turn_towards(cur: f32, target: f32, max: f32) -> f32 {
    let d = (target - cur + std::f32::consts::PI).rem_euclid(std::f32::consts::TAU) - std::f32::consts::PI;
    cur + d.clamp(-max, max)
}

fn approach(cur: f32, target: f32, rate: f32, dt: f32) -> f32 {
    cur + (target - cur).clamp(-rate * dt, rate * dt)
}

// ---------------------------------------------------------------- world cache

/// Per-scene data derived once whenever the built scene changes.
#[derive(Resource, Default)]
struct LifeWorld {
    scene: Option<Arc<Scene>>,
    generation: u32,
    obstacles: Obstacles,
    /// Gull view of the island: tower tops, perches, open sea.
    air: GullWorld,
    /// Open sea at least ~1.2 m deep (2 m grid): fish leap here.
    deep_sea: Vec<Vec2>,
    /// Dry sand near the water where crabs live (1 m grid).
    beach: Vec<Vec2>,
    /// Fire markers (capped) with their kind.
    fires: Vec<(Vec3, FireKind)>,
    /// Smoke hole markers (capped).
    chimneys: Vec<Vec3>,
    /// Firefly home points over grass.
    fly_homes: Vec<Vec3>,
}

impl LifeWorld {
    /// Dry, unobstructed ground inside the glade.
    fn crab_ok(&self, p: Vec2) -> bool {
        self.scene.as_ref().is_some_and(|s| {
            glade_dist(p) < -1.0 && s.terrain.water_level(p).is_none() && !self.obstacles.blocked(p, 0.6)
        })
    }
}

fn refresh_world(built: Res<BuiltScene>, glade: Res<Glade>, mut world: ResMut<LifeWorld>) {
    // Only the committed document drives life; previews would rebuild every frame of a drag.
    if glade.has_preview() && world.scene.is_some() {
        return;
    }
    let Some(scene) = &built.0 else { return };
    if world.scene.as_ref().is_some_and(|s| Arc::ptr_eq(s, scene)) {
        return;
    }
    let terrain = &scene.terrain;
    let mut w = LifeWorld { scene: Some(scene.clone()), generation: world.generation + 1, ..Default::default() };
    w.obstacles = Obstacles::from_scene(scene);

    for (x, z) in (-15..15).flat_map(|x| (-15..15).map(move |z| (x, z))) {
        let p = Vec2::new(x as f32 * 2.0 + 1.0, z as f32 * 2.0 + 1.0);
        if terrain.is_sea(p) && terrain.height(p) < -1.2 {
            w.deep_sea.push(p);
        }
    }
    w.air.sea = w.deep_sea.iter().step_by(2).copied().collect();

    let near_sea = |p: Vec2| (0..8).any(|k| terrain.is_sea(p + Vec2::from_angle(k as f32 * std::f32::consts::FRAC_PI_4) * 2.5));
    for (x, z) in (-32..32).flat_map(|x| (-32..32).map(move |z| (x, z))) {
        let p = Vec2::new(x as f32 + 0.5, z as f32 + 0.5);
        let h = terrain.height(p);
        if glade_dist(p) < -1.0 && (0.0..0.7).contains(&h) && terrain.water_level(p).is_none() && !w.obstacles.blocked(p, 0.6) && near_sea(p) {
            w.beach.push(p);
        }
    }

    let mut perches: Vec<Vec3> = scene.markers.iter().filter(|m| m.kind == MarkerKind::Perch).map(|m| m.pos).collect();
    perches.sort_by(|a, b| a.x.total_cmp(&b.x).then(a.z.total_cmp(&b.z)));
    let stride = perches.len().div_ceil(MAX_PERCHES).max(1);
    w.air.perches = perches.iter().step_by(stride).copied().collect();

    for m in scene.markers.iter() {
        match m.kind {
            MarkerKind::Fire if w.fires.len() < MAX_FIRES => w.fires.push((m.pos, fx::fire_kind(m.pos, terrain))),
            MarkerKind::ChimneySmoke if w.chimneys.len() < MAX_CHIMNEYS => w.chimneys.push(m.pos),
            _ => {}
        }
    }
    w.air.towers = tower_tops(scene, &w.fires);

    // Fireflies: grass (the interior, well above the beach) near the middle of the island.
    let mut rng = Rng::at(77, w.beach.len() as f32, w.air.towers.len() as f32);
    let grass = |p: Vec2| {
        glade_dist(p) < -EDGE_MARGIN
            && (0.7..4.0).contains(&terrain.height(p))
            && terrain.water_level(p).is_none()
            && terrain.normal(p).y > 0.88
            && !w.obstacles.blocked(p, 1.0)
    };
    let mut homes: Vec<Vec2> = Vec::new();
    for _ in 0..500 {
        if homes.len() >= MAX_FLY_HOMES {
            break;
        }
        homes.extend(random_spot(&mut rng, Vec2::ZERO, ROAM_RADIUS, &grass));
    }
    w.fly_homes = homes.into_iter().map(|p| Vec3::new(p.x, terrain.height(p) + 0.1, p.y)).collect();
    *world = w;
}

/// Beacon tops first, then up to three distinct tall perches (or the highest part's top as a
/// fallback): the places gulls circle.
fn tower_tops(scene: &Scene, fires: &[(Vec3, FireKind)]) -> Vec<Vec3> {
    let mut out: Vec<Vec3> = Vec::new();
    let add = |p: Vec3, out: &mut Vec<Vec3>| {
        if out.len() < 4 && out.iter().all(|q| (q.xz() - p.xz()).length() > 5.0) {
            out.push(p);
        }
    };
    for (p, _) in fires.iter().filter(|(_, k)| *k == FireKind::Beacon) {
        add(*p, &mut out);
    }
    let mut perches: Vec<Vec3> = scene.markers.iter().filter(|m| m.kind == MarkerKind::Perch).map(|m| m.pos).collect();
    perches.sort_by(|a, b| b.y.total_cmp(&a.y));
    for p in perches {
        add(p, &mut out);
    }
    if out.is_empty() {
        let top = scene.parts.iter().filter_map(|p| p.mesh.bounds()).max_by(|a, b| a.1.y.total_cmp(&b.1.y));
        if let Some((lo, hi)) = top {
            out.push(Vec3::new((lo.x + hi.x) * 0.5, hi.y, (lo.z + hi.z) * 0.5));
        }
    }
    out
}

// ---------------------------------------------------------------- components + resources

#[derive(Resource, Default)]
struct Flock {
    gulls: Vec<Gull>,
    generation: u32,
}

#[derive(Resource, Default)]
struct Crabs {
    crabs: Vec<Crab>,
}

/// One fish leap: where and which way it jumps, and its age in seconds.
struct Leap {
    from: Vec2,
    dir: Vec2,
    age: f32,
}

#[derive(Resource)]
struct Leaps {
    list: Vec<Leap>,
    next: f32,
    rng: Rng,
}

impl Default for Leaps {
    fn default() -> Self {
        Self { list: Vec::new(), next: 3.0, rng: Rng(0x1EA9) }
    }
}

#[derive(Component)]
struct GullBody(usize);
#[derive(Component)]
struct GullWing {
    left: bool,
}
#[derive(Component)]
struct CrabBody(usize);
#[derive(Component)]
struct Canoe {
    pos: Vec3,
    yaw: f32,
    phase: f32,
}

#[derive(Component, Clone, Copy, PartialEq, Eq)]
enum Cloud {
    Flame,
    Smoke,
    Fish,
    Firefly,
}

fn spawn_life(
    mut commands: Commands,
    assets: Option<Res<GladeAssets>>,
    world: Res<LifeWorld>,
    mut flock: ResMut<Flock>,
    mut crabs: ResMut<Crabs>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut done: Local<bool>,
) {
    let (Some(assets), Some(scene)) = (assets, &world.scene) else { return };
    if *done {
        return;
    }
    *done = true;
    let mat = assets.opaque.clone();

    let (body, wing) = (meshes.add(to_bevy_mesh(&fauna::gull_body())), meshes.add(to_bevy_mesh(&fauna::gull_wing())));
    let seed = (scene.terrain.height(Vec2::ZERO) * 1000.0) as i64 as u64;
    for i in 0..GULLS {
        let g = Gull::new(Rng::at(31 ^ seed, i as f32, 1.0), &world.air, 0.0);
        let t = Transform::from_translation(g.pos).with_scale(Vec3::splat(GULL_SCALE));
        flock.gulls.push(g);
        commands.spawn((t, Visibility::default(), GullBody(i))).with_children(|p| {
            p.spawn((Mesh3d(body.clone()), MeshMaterial3d(mat.clone())));
            for left in [false, true] {
                p.spawn((Mesh3d(wing.clone()), MeshMaterial3d(mat.clone()), Transform::from_xyz(if left { -0.07 } else { 0.07 }, 0.07, 0.06), GullWing { left }));
            }
        });
    }

    let crab = meshes.add(to_bevy_mesh(&fauna::crab_body()));
    for i in 0..CRABS {
        let mut rng = Rng::at(53 ^ seed, i as f32, 2.0);
        let pos = world.beach.get((rng.next_u64() % world.beach.len().max(1) as u64) as usize).copied().unwrap_or(Vec2::ZERO);
        crabs.crabs.push(Crab::new(rng, pos));
        commands.spawn((Mesh3d(crab.clone()), MeshMaterial3d(mat.clone()), Transform::from_scale(Vec3::splat(CRAB_SCALE)), Visibility::Hidden, CrabBody(i)));
    }

    for kind in [Cloud::Flame, Cloud::Smoke, Cloud::Fish, Cloud::Firefly] {
        commands.spawn((Mesh3d(meshes.add(particle_mesh(&[]))), MeshMaterial3d(assets.particle.clone()), NoFrustumCulling, kind));
    }
}

// ---------------------------------------------------------------- gulls

fn update_gulls(
    time: Res<Time>,
    env: Res<Environment>,
    world: Res<LifeWorld>,
    mut flock: ResMut<Flock>,
    mut bodies: Query<(&mut Transform, &GullBody, &Children)>,
    mut wings: Query<(&mut Transform, &GullWing), Without<GullBody>>,
) {
    let (dt, t) = (time.delta_secs().min(0.1), time.elapsed_secs());
    let flock = &mut *flock;
    if flock.generation != world.generation {
        flock.generation = world.generation;
        // Perch indices may point somewhere else now: everybody takes off and chooses again.
        for g in flock.gulls.iter_mut().filter(|g| matches!(g.mode, fauna::GullMode::Land { .. } | fauna::GullMode::Perched { .. })) {
            g.timer = 0.0;
            g.mode = fauna::GullMode::Roam { target: g.pos };
        }
    }
    for i in 0..flock.gulls.len() {
        let held: HashSet<usize> = flock.gulls.iter().enumerate().filter(|(j, _)| *j != i).flat_map(|(_, g)| (0..world.air.perches.len()).filter(|&k| g.holds(k))).collect();
        flock.gulls[i].step(dt, &world.air, 0.0, env.wind, &|k| held.contains(&k));
    }
    for (mut tf, GullBody(i), children) in &mut bodies {
        let Some(g) = flock.gulls.get(*i) else { continue };
        let breath = if g.is_perched() { 0.0 } else { 0.04 * (t * 1.7 + *i as f32).sin() };
        tf.translation = g.pos + Vec3::Y * breath;
        tf.rotation = Quat::from_rotation_y(g.heading) * Quat::from_rotation_z(g.bank);
        let angle = g.wing_angle(t, *i as f32 * 1.9);
        let spread = if g.is_perched() { 0.45 } else { 1.0 };
        for child in children.iter() {
            if let Ok((mut wt, wing)) = wings.get_mut(child) {
                wt.rotation = if wing.left { Quat::from_rotation_y(std::f32::consts::PI) } else { Quat::IDENTITY } * Quat::from_rotation_z(angle);
                wt.scale = Vec3::new(spread, 1.0, 1.0);
            }
        }
    }
}

// ---------------------------------------------------------------- crabs

fn update_crabs(
    time: Res<Time>,
    env: Res<Environment>,
    world: Res<LifeWorld>,
    mut crabs: ResMut<Crabs>,
    mut bodies: Query<(&mut Transform, &mut Visibility, &CrabBody)>,
) {
    let Some(scene) = &world.scene else { return };
    let (dt, t) = (time.delta_secs().min(0.1), time.elapsed_secs());
    let tide = Tide::at(env.hour).level;
    let terrain = &scene.terrain;
    let height = |p: Vec2| terrain.height(p);
    let seaward = |p: Vec2| {
        let n = terrain.normal(p).xz();
        if n.length() > 0.02 { n.normalize() } else { Vec2::ZERO }
    };
    let ok = |p: Vec2| world.crab_ok(p);
    let shore = Shore { height: &height, seaward: &seaward, ok: &ok };
    let active = !world.beach.is_empty();
    for (mut tf, mut vis, CrabBody(i)) in &mut bodies {
        let Some(c) = crabs.crabs.get_mut(*i) else { continue };
        if !active {
            *vis = Visibility::Hidden;
            continue;
        }
        if !ok(c.pos) {
            // The scene changed under the crab: scuttle off to another bit of beach.
            c.pos = world.beach[(c.rng.next_u64() % world.beach.len() as u64) as usize];
        }
        let moving = c.step(dt, tide, &shore);
        let wobble = if moving { (t * 22.0 + *i as f32).sin() * 0.07 } else { 0.0 };
        tf.translation = Vec3::new(c.pos.x, terrain.height(c.pos), c.pos.y);
        tf.rotation = Quat::from_rotation_y(c.yaw) * Quat::from_rotation_z(wobble);
        *vis = Visibility::Inherited;
    }
}

// ---------------------------------------------------------------- canoes

/// Respawns the bobbing canoes whenever the scene changes (one per `Boat` marker).
fn sync_boats(
    mut commands: Commands,
    assets: Option<Res<GladeAssets>>,
    world: Res<LifeWorld>,
    old: Query<Entity, With<Canoe>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut hulls: Local<Vec<Handle<Mesh>>>,
    mut seen: Local<u32>,
) {
    let (Some(assets), Some(scene)) = (assets, &world.scene) else { return };
    if *seen == world.generation {
        return;
    }
    *seen = world.generation;
    for e in &old {
        commands.entity(e).despawn();
    }
    if hulls.is_empty() {
        *hulls = fauna::canoe_meshes(&scene.terrain, LIFE_SEED).iter().map(|m| meshes.add(to_bevy_mesh(m))).collect();
    }
    if hulls.is_empty() {
        return;
    }
    for m in scene.markers.iter().filter(|m| m.kind == MarkerKind::Boat).take(MAX_BOATS) {
        let h = hash2(LIFE_SEED, m.pos.x, m.pos.z);
        let yaw = -m.dir.z.atan2(m.dir.x);
        commands.spawn((
            Mesh3d(hulls[(h % TRIM_COLORS as u64) as usize % hulls.len()].clone()),
            MeshMaterial3d(assets.opaque.clone()),
            Transform::from_translation(m.pos).with_rotation(Quat::from_rotation_y(yaw)),
            Canoe { pos: m.pos, yaw, phase: hash_f(5, m.pos.x, m.pos.z) * std::f32::consts::TAU },
        ));
    }
}

fn update_boats(time: Res<Time>, env: Res<Environment>, mut boats: Query<(&mut Transform, &Canoe)>) {
    let t = time.elapsed_secs();
    let tide = Tide::at(env.hour).level;
    let wind = env.wind.length();
    for (mut tf, c) in &mut boats {
        let pose = fauna::boat_pose(t, c.phase, wind);
        tf.translation = Vec3::new(c.pos.x, fauna::boat_level(c.pos.y, tide) + pose.y, c.pos.z);
        // Bow along +x, outrigger along +z: pitch is about z, roll about x.
        tf.rotation = Quat::from_rotation_y(c.yaw + pose.yaw) * Quat::from_rotation_z(pose.pitch) * Quat::from_rotation_x(pose.roll);
    }
}

// ---------------------------------------------------------------- fish

fn update_leaps(time: Res<Time>, world: Res<LifeWorld>, rig: Option<Res<CameraRig>>, mut leaps: ResMut<Leaps>) {
    let dt = time.delta_secs().min(0.1);
    let leaps = &mut *leaps;
    for l in &mut leaps.list {
        l.age += dt;
    }
    leaps.list.retain(|l| l.age < fx::LEAP_TIME + fx::RING_LIFE);
    leaps.next -= dt;
    if leaps.next > 0.0 {
        return;
    }
    leaps.next = leaps.rng.range(2.5, 6.0);
    if leaps.list.len() >= MAX_LEAPS || world.deep_sea.is_empty() {
        return;
    }
    let focus = rig.map_or(Vec2::ZERO, |r| r.focus.xz());
    let mut pick = Vec2::ZERO;
    for _ in 0..30 {
        pick = world.deep_sea[(leaps.rng.next_u64() % world.deep_sea.len() as u64) as usize];
        if (pick - focus).length() < LEAP_NEAR {
            break;
        }
    }
    let a = leaps.rng.range(0.0, std::f32::consts::TAU);
    leaps.list.push(Leap { from: pick, dir: Vec2::new(a.cos(), a.sin()), age: 0.0 });
}

// ---------------------------------------------------------------- particle clouds

#[allow(clippy::too_many_arguments)]
fn update_clouds(
    time: Res<Time>,
    env: Res<Environment>,
    weather: Res<Weather>,
    world: Res<LifeWorld>,
    leaps: Res<Leaps>,
    clouds: Query<(&Mesh3d, &Cloud)>,
    mut meshes: ResMut<Assets<Mesh>>,
) {
    let t = time.elapsed_secs();
    let tide = Tide::at(env.hour).level;
    let lit = |kind: FireKind| fx::fire_lit(kind, env.hour, weather.kind);
    for (m, kind) in &clouds {
        let Some(mut mesh) = meshes.get_mut(&m.0) else { continue };
        let mut ps = Vec::new();
        match kind {
            Cloud::Flame => {
                for (k, (pos, fk)) in world.fires.iter().enumerate().filter(|(_, (_, fk))| lit(*fk)) {
                    fx::flame_particles(*pos, *fk, t, k as f32 * 0.31, env.wind, &mut ps);
                }
            }
            Cloud::Smoke => {
                for (k, pos) in world.chimneys.iter().enumerate() {
                    fx::smoke_particles(*pos, 8, 1.0, t, k as f32 * 0.37, env.wind, env.night, &mut ps);
                }
                for (k, (pos, _)) in world.fires.iter().enumerate().filter(|(_, (_, fk))| lit(*fk)) {
                    fx::smoke_particles(*pos + Vec3::Y * 0.6, 5, 0.8, t, 0.5 + k as f32 * 0.43, env.wind, env.night, &mut ps);
                }
            }
            Cloud::Fish => {
                for l in &leaps.list {
                    fx::leap_particles(l.from, l.dir, tide, l.age, &mut ps);
                }
            }
            Cloud::Firefly => {
                if env.night > 0.5 && env.wetness < 0.6 {
                    fx::firefly_particles(&world.fly_homes, t, env.night, &mut ps);
                }
            }
        }
        write_particle_mesh(&mut mesh, &ps);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn in_glade(p: Vec2) -> bool {
        glade_dist(p) < -EDGE_MARGIN
    }

    #[test]
    fn random_spot_stays_in_glade_and_is_deterministic() {
        let run = || {
            let mut rng = Rng(5);
            (0..200).map(|_| random_spot(&mut rng, Vec2::new(20.0, 20.0), 12.0, &in_glade).expect("spot")).collect::<Vec<_>>()
        };
        let pts = run();
        assert!(pts.iter().all(|p| in_glade(*p)));
        assert_eq!(pts, run());
    }

    #[test]
    fn obstacles_block_near_walls() {
        // Tall wall along x = 3, z in -10..10.
        let a = Vec3::new(3.0, 0.0, -10.0);
        let b = Vec3::new(3.0, 0.0, 10.0);
        let up = Vec3::Y * 3.0;
        let obs = Obstacles::from_tris([[a, b, b + up], [a, b + up, a + up]].into_iter());
        assert!(obs.blocked(Vec2::new(3.2, 0.0), 0.5));
        assert!(!obs.blocked(Vec2::new(8.0, 0.0), 0.5));
    }

    #[test]
    fn turning_takes_short_way() {
        assert!((turn_towards(0.1, std::f32::consts::TAU - 0.1, 1.0) - (-0.1)).abs() < 1e-4);
    }

    /// The demo cove has everything life needs: perches, beacon, firepit fire, smoke, boats, beach.
    #[test]
    fn demo_cove_feeds_every_kind_of_life() {
        let doc = cairn_cove_core::demo::demo_document(7);
        let scene = Arc::new(cairn_cove_core::build(&doc));
        let mut app = App::new();
        app.insert_resource(BuiltScene(Some(scene.clone()))).insert_resource(Glade::new(doc)).init_resource::<LifeWorld>();
        app.add_systems(Update, refresh_world);
        app.update();
        let w = app.world().resource::<LifeWorld>();
        assert!(!w.air.perches.is_empty() && !w.air.towers.is_empty(), "gulls need perches and towers");
        assert!(w.air.sea.len() > 20 && w.deep_sea.len() > 20, "gulls and fish need open sea");
        assert!(w.beach.len() > 10, "crabs need a beach, got {}", w.beach.len());
        assert!(w.fires.iter().any(|(_, k)| *k == FireKind::Beacon), "beacon fire");
        assert!(w.fires.iter().any(|(_, k)| *k == FireKind::Pit), "firepit fire");
        assert!(!w.chimneys.is_empty() && !w.fly_homes.is_empty());
        assert!(w.beach.iter().all(|p| w.crab_ok(*p)));
        assert!(w.fires.len() <= MAX_FIRES && w.air.perches.len() <= MAX_PERCHES);
        let boats = scene.markers.iter().filter(|m| m.kind == MarkerKind::Boat).count();
        assert!(boats >= 2);
        assert!(!fauna::canoe_meshes(&scene.terrain, LIFE_SEED).is_empty());
    }
}
