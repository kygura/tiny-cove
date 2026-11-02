//! Ambient life: deer, swifts, ducks, fireflies, vent steam.
//!
//! Creatures are tiny low-poly rigs built once from `MeshData` (so they get the PS1/cel
//! material) and animated through `Transform`s only. Swifts, steam and fireflies are three
//! CPU particle clouds, one mesh each. Everything re-targets when [`BuiltScene`] changes.

use crate::env::Environment;
use crate::render::{GladeAssets, Particle, ParticleKind, particle_mesh, to_bevy_mesh, write_particle_mesh};
use crate::world::{BuiltScene, Glade};
use bevy::{camera::visibility::NoFrustumCulling, prelude::*};
use cairn_cove_core::{MarkerKind, MeshData, PaintLayer, Scene, Surface, mesh::Rng, terrain::glade_dist};
use std::{collections::HashSet, sync::Arc};

const DEER: usize = 4;
const DUCKS: usize = 3;
const SWIFTS: usize = 8;
const MAX_STEAM: usize = 12;
const MAX_FLY_HOMES: usize = 14;
/// Creatures stay this far inside the glade edge (m).
const EDGE_MARGIN: f32 = 4.0;
/// Creatures spawn / wander within this distance of the origin so they stay near the camera.
const ROAM_RADIUS: f32 = 20.0;

pub struct LifePlugin;

impl Plugin for LifePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<LifeWorld>().add_systems(
            Update,
            (refresh_world, spawn_life, manage_ducks, update_deer, update_ducks, update_swifts, update_steam, update_fireflies).chain(),
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

/// True when every ~0.5 m sample of the segment `a`→`b` satisfies `ok`.
pub fn path_clear(a: Vec2, b: Vec2, ok: &dyn Fn(Vec2) -> bool) -> bool {
    let n = ((b - a).length() / 0.5).ceil().max(1.0) as i32;
    (1..=n).all(|i| ok(a.lerp(b, i as f32 / n as f32)))
}

/// A random target within `radius` of `from` (at least `radius / 3` away) whose straight
/// path is clear. `None` if nothing is found in a few tries.
pub fn pick_wander(rng: &mut Rng, from: Vec2, radius: f32, ok: &dyn Fn(Vec2) -> bool) -> Option<Vec2> {
    (0..24).find_map(|_| {
        let ang = rng.range(0.0, std::f32::consts::TAU);
        let d = rng.range(radius / 3.0, radius);
        let p = from + Vec2::new(ang.cos(), ang.sin()) * d;
        (ok(p) && path_clear(from, p, ok)).then_some(p)
    })
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
    /// Open-water sample points (1 m grid).
    water: Vec<Vec2>,
    /// Tops of the tallest structures, birds circle these.
    towers: Vec<Vec3>,
    /// Firefly home points (ferns / water edge).
    fly_homes: Vec<Vec3>,
}

impl LifeWorld {
    fn walkable(&self, p: Vec2) -> bool {
        let Some(s) = &self.scene else { return false };
        glade_dist(p) < -EDGE_MARGIN
            && p.length() < ROAM_RADIUS + 6.0
            && s.terrain.water_level(p).is_none()
            && s.terrain.normal(p).y > 0.88
            && !self.obstacles.blocked(p, 1.0)
    }
    fn is_water(&self, p: Vec2) -> bool {
        self.scene.as_ref().is_some_and(|s| glade_dist(p) < -2.0 && s.terrain.water_level(p).is_some())
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
    let mut w = LifeWorld { scene: Some(scene.clone()), generation: world.generation + 1, ..Default::default() };
    w.obstacles = Obstacles::from_scene(scene);
    for z in -32..32 {
        for x in -32..32 {
            let p = Vec2::new(x as f32 + 0.5, z as f32 + 0.5);
            if glade_dist(p) < -2.0 && scene.terrain.water_level(p).is_some() {
                w.water.push(p);
            }
        }
    }
    w.towers = tower_tops(scene);
    let mut rng = Rng::at(77, w.water.len() as f32, w.towers.len() as f32);
    let near_water = |p: Vec2| {
        [Vec2::X, Vec2::NEG_X, Vec2::Y, Vec2::NEG_Y].iter().any(|d| scene.terrain.water_level(p + *d * 1.5).is_some())
    };
    let free = |p: Vec2| {
        glade_dist(p) < -EDGE_MARGIN && scene.terrain.water_level(p).is_none() && !w.obstacles.blocked(p, 1.0)
    };
    let mut homes: Vec<Vec2> = Vec::new();
    for _ in 0..500 {
        if homes.len() >= MAX_FLY_HOMES {
            break;
        }
        let p = Vec2::new(rng.range(-ROAM_RADIUS, ROAM_RADIUS), rng.range(-ROAM_RADIUS, ROAM_RADIUS));
        if free(p) && (scene.terrain.paint(PaintLayer::DuneGrass, p) > 0.25 || near_water(p)) {
            homes.push(p);
        }
    }
    if homes.len() < 6 {
        homes.extend(random_spot(&mut rng, Vec2::ZERO, ROAM_RADIUS * 0.6, &free));
        while homes.len() < 6 {
            match random_spot(&mut rng, Vec2::ZERO, ROAM_RADIUS * 0.6, &free) {
                Some(p) => homes.push(p),
                None => break,
            }
        }
    }
    w.fly_homes = homes.into_iter().map(|p| p.extend(0.0).xzy().with_y(scene.terrain.height(p) + 0.1)).collect();
    *world = w;
}

/// Up to three distinct tall perch points (or the highest part's top as a fallback).
fn tower_tops(scene: &Scene) -> Vec<Vec3> {
    let mut perches: Vec<Vec3> = scene.markers.iter().filter(|m| m.kind == MarkerKind::Perch).map(|m| m.pos).collect();
    perches.sort_by(|a, b| b.y.total_cmp(&a.y));
    let mut out: Vec<Vec3> = Vec::new();
    for p in perches {
        if out.len() < 3 && out.iter().all(|q| (q.xz() - p.xz()).length() > 5.0) {
            out.push(p);
        }
    }
    if out.is_empty() {
        let top = scene.parts.iter().filter_map(|p| p.mesh.bounds()).max_by(|a, b| a.1.y.total_cmp(&b.1.y));
        if let Some((lo, hi)) = top {
            out.push(Vec3::new((lo.x + hi.x) * 0.5, hi.y, (lo.z + hi.z) * 0.5));
        }
    }
    out
}

// ---------------------------------------------------------------- creature meshes

fn deer_body() -> MeshData {
    let (brown, dark, pale) = (Vec3::new(0.48, 0.32, 0.18), Vec3::new(0.34, 0.22, 0.13), Vec3::new(0.85, 0.8, 0.7));
    let mut m = MeshData::default();
    let bx = |m: &mut MeshData, c: Vec3, h: Vec3, col: Vec3, s: Surface| m.oriented_box(c, h, Vec3::X, col, s, 1.0);
    // Origin is the shoulder pivot (0, 0.95, 0) of the rig.
    bx(&mut m, Vec3::ZERO, Vec3::new(0.2, 0.21, 0.58), brown, Surface::Petal);
    bx(&mut m, Vec3::new(0.0, -0.17, 0.0), Vec3::new(0.16, 0.05, 0.5), pale, Surface::Petal);
    bx(&mut m, Vec3::new(0.0, 0.3, 0.6), Vec3::new(0.09, 0.28, 0.1), brown, Surface::Petal); // neck
    bx(&mut m, Vec3::new(0.0, 0.58, 0.8), Vec3::new(0.09, 0.09, 0.2), dark, Surface::Petal); // head
    bx(&mut m, Vec3::new(0.0, 0.54, 1.0), Vec3::new(0.05, 0.05, 0.04), pale, Surface::Petal); // muzzle
    bx(&mut m, Vec3::new(0.0, 0.1, -0.62), Vec3::new(0.06, 0.09, 0.05), pale, Surface::Petal); // tail
    for s in [-1.0f32, 1.0] {
        bx(&mut m, Vec3::new(0.1 * s, 0.76, 0.7), Vec3::new(0.025, 0.07, 0.04), dark, Surface::Petal); // ears
        bx(&mut m, Vec3::new(0.06 * s, 0.9, 0.72), Vec3::new(0.018, 0.17, 0.018), pale, Surface::Bark); // antler
        bx(&mut m, Vec3::new(0.06 * s, 1.05, 0.78), Vec3::new(0.018, 0.018, 0.1), pale, Surface::Bark);
    }
    m
}

fn deer_leg() -> MeshData {
    let mut m = MeshData::default();
    m.oriented_box(Vec3::new(0.0, -0.35, 0.0), Vec3::new(0.055, 0.35, 0.055), Vec3::X, Vec3::new(0.3, 0.2, 0.12), Surface::Petal, 1.0);
    m
}

fn duck_mesh() -> MeshData {
    let mut m = MeshData::default();
    let bx = |m: &mut MeshData, c: Vec3, h: Vec3, col: Vec3| m.oriented_box(c, h, Vec3::X, col, Surface::Petal, 1.0);
    bx(&mut m, Vec3::new(0.0, 0.1, -0.02), Vec3::new(0.12, 0.08, 0.2), Vec3::new(0.4, 0.33, 0.25));
    bx(&mut m, Vec3::new(0.0, 0.1, -0.2), Vec3::new(0.06, 0.06, 0.06), Vec3::new(0.25, 0.2, 0.15)); // tail
    bx(&mut m, Vec3::new(0.0, 0.24, 0.15), Vec3::new(0.045, 0.1, 0.045), Vec3::new(0.1, 0.35, 0.18)); // neck
    bx(&mut m, Vec3::new(0.0, 0.33, 0.18), Vec3::new(0.05, 0.05, 0.06), Vec3::new(0.1, 0.35, 0.18)); // head
    bx(&mut m, Vec3::new(0.0, 0.31, 0.26), Vec3::new(0.03, 0.015, 0.045), Vec3::new(0.95, 0.65, 0.1)); // beak
    m
}

// ---------------------------------------------------------------- components

#[derive(Component)]
struct Deer {
    rng: Rng,
    pos: Vec2,
    target: Vec2,
    yaw: f32,
    pause: f32,
    grazing: bool,
    graze: f32,
    walk: f32,
    phase: f32,
    lie: f32,
    generation: u32,
}

#[derive(Component)]
struct Leg(usize);

#[derive(Component)]
struct DeerBody;

#[derive(Component)]
struct Duck {
    rng: Rng,
    pos: Vec2,
    target: Vec2,
    yaw: f32,
    pause: f32,
    seed: f32,
    generation: u32,
}

#[derive(Component)]
struct SwiftCloud;
#[derive(Component)]
struct SteamCloud;
#[derive(Component)]
struct FireflyCloud;

const HIP: f32 = 0.7;
const SHOULDER: f32 = 0.95;
const DEER_SCALE: f32 = 1.2;
const LEG_X: [f32; 4] = [-0.13, 0.13, -0.13, 0.13];
const LEG_Z: [f32; 4] = [0.42, 0.42, -0.42, -0.42];
const LEG_PHASE: [f32; 4] = [0.0, std::f32::consts::PI, std::f32::consts::PI, 0.0];

fn spawn_life(
    mut commands: Commands,
    assets: Option<Res<GladeAssets>>,
    world: Res<LifeWorld>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut done: Local<bool>,
) {
    let (Some(assets), Some(scene)) = (assets, &world.scene) else { return };
    if *done {
        return;
    }
    *done = true;
    let mat = assets.opaque.clone();
    let body = meshes.add(to_bevy_mesh(&deer_body()));
    let leg = meshes.add(to_bevy_mesh(&deer_leg()));
    let mut rng = Rng::at(31, scene.terrain.height(Vec2::ZERO), 1.0);
    for i in 0..DEER {
        let mut r = Rng(rng.next_u64());
        let pos = random_spot(&mut r, Vec2::ZERO, ROAM_RADIUS * 0.7, &|p| world.walkable(p)).unwrap_or(Vec2::ZERO);
        commands
            .spawn((
                Transform::from_translation(Vec3::new(pos.x, scene.terrain.height(pos), pos.y)).with_scale(Vec3::splat(DEER_SCALE)),
                Visibility::default(),
                Deer {
                    rng: Rng(r.next_u64()),
                    pos,
                    target: pos,
                    yaw: r.range(0.0, std::f32::consts::TAU),
                    pause: r.range(0.0, 4.0),
                    grazing: i % 2 == 0,
                    graze: 0.0,
                    walk: 0.0,
                    phase: r.range(0.0, 6.0),
                    lie: 0.0,
                    generation: world.generation,
                },
            ))
            .with_children(|p| {
                p.spawn((Mesh3d(body.clone()), MeshMaterial3d(mat.clone()), Transform::from_xyz(0.0, SHOULDER, 0.0), DeerBody));
                for k in 0..4 {
                    p.spawn((Mesh3d(leg.clone()), MeshMaterial3d(mat.clone()), Transform::from_xyz(LEG_X[k], HIP, LEG_Z[k]), Leg(k)));
                }
            });
    }
    let cloud = |commands: &mut Commands, meshes: &mut Assets<Mesh>| {
        commands.spawn((Mesh3d(meshes.add(particle_mesh(&[]))), MeshMaterial3d(assets.particle.clone()), NoFrustumCulling)).id()
    };
    let e = cloud(&mut commands, &mut meshes);
    commands.entity(e).insert(SwiftCloud);
    let e = cloud(&mut commands, &mut meshes);
    commands.entity(e).insert(SteamCloud);
    let e = cloud(&mut commands, &mut meshes);
    commands.entity(e).insert(FireflyCloud);
}

// ---------------------------------------------------------------- deer

fn update_deer(
    time: Res<Time>,
    env: Res<Environment>,
    world: Res<LifeWorld>,
    mut deer: Query<(&mut Transform, &mut Deer, &Children)>,
    mut legs: Query<(&mut Transform, &Leg), Without<Deer>>,
    mut bodies: Query<&mut Transform, (With<DeerBody>, Without<Deer>, Without<Leg>)>,
) {
    let Some(scene) = &world.scene else { return };
    let dt = time.delta_secs().min(0.1);
    let asleep = env.night > 0.5;
    let ok = |p: Vec2| world.walkable(p);
    for (mut tf, mut d, children) in &mut deer {
        if d.generation != world.generation {
            d.generation = world.generation;
            d.target = d.pos;
            d.pause = 0.2;
            if !ok(d.pos) {
                let mut r = Rng(d.rng.next_u64());
                if let Some(p) = random_spot(&mut r, Vec2::ZERO, ROAM_RADIUS * 0.7, &ok) {
                    d.pos = p;
                }
            }
        }
        d.lie = approach(d.lie, if asleep { 1.0 } else { 0.0 }, 0.5, dt);
        let mut moving = false;
        if d.lie < 0.05 && !asleep {
            if d.pause > 0.0 {
                d.pause -= dt;
                if d.pause <= 0.0 {
                    let (from, rng) = (d.pos, &mut d.rng);
                    match pick_wander(rng, from, 7.0, &ok) {
                        Some(t) => d.target = t,
                        None => d.pause = 1.0,
                    }
                    d.grazing = false;
                }
            } else {
                let delta = d.target - d.pos;
                if delta.length() < 0.3 {
                    d.pause = d.rng.range(3.0, 9.0);
                    d.grazing = d.rng.f() < 0.7;
                } else {
                    let want = delta.x.atan2(delta.y);
                    d.yaw = turn_towards(d.yaw, want, 2.2 * dt);
                    let facing = (want - d.yaw).cos().max(0.0);
                    let step = Vec2::new(d.yaw.sin(), d.yaw.cos()) * 0.9 * dt * facing;
                    if ok(d.pos + step) {
                        d.pos += step;
                        moving = facing > 0.3;
                    } else {
                        d.target = d.pos;
                        d.pause = 0.5;
                    }
                }
            }
        }
        d.walk = approach(d.walk, if moving { 1.0 } else { 0.0 }, 4.0, dt);
        let graze_target = if d.grazing && d.pause > 0.0 && d.lie < 0.05 { 1.0 } else { 0.0 };
        d.graze = approach(d.graze, graze_target, 1.5, dt);
        d.phase += dt * 5.0 * d.walk;
        let pos = d.pos;
        tf.translation = Vec3::new(pos.x, scene.terrain.height(pos), pos.y);
        tf.rotation = Quat::from_rotation_y(d.yaw);
        let lie = d.lie;
        for child in children.iter() {
            if let Ok((mut lt, leg)) = legs.get_mut(child) {
                lt.translation.y = HIP - 0.42 * lie;
                lt.scale.y = 1.0 - 0.65 * lie;
                lt.rotation = Quat::from_rotation_x((d.phase + LEG_PHASE[leg.0]).sin() * 0.5 * d.walk);
            } else if let Ok(mut bt) = bodies.get_mut(child) {
                bt.translation.y = SHOULDER - 0.42 * lie;
                bt.rotation = Quat::from_rotation_x(0.3 * d.graze + 0.05 * (d.phase * 2.0).sin() * d.walk);
            }
        }
    }
}

// ---------------------------------------------------------------- ducks

/// Ducks live only while there is a pond: spawned lazily when enough water exists, removed when it is gone.
fn manage_ducks(
    mut commands: Commands,
    assets: Option<Res<GladeAssets>>,
    world: Res<LifeWorld>,
    mut meshes: ResMut<Assets<Mesh>>,
    ducks: Query<Entity, With<Duck>>,
    mut mesh: Local<Option<Handle<Mesh>>>,
) {
    let Some(assets) = assets else { return };
    if !world.is_changed() {
        return;
    }
    if world.water.is_empty() {
        for e in &ducks {
            commands.entity(e).despawn();
        }
    } else if world.water.len() >= 6 && ducks.is_empty() {
        let handle = mesh.get_or_insert_with(|| meshes.add(to_bevy_mesh(&duck_mesh()))).clone();
        let mut rng = Rng::at(53, world.water.len() as f32, 2.0);
        for i in 0..DUCKS {
            let pos = world.water[(rng.next_u64() % world.water.len() as u64) as usize];
            commands.spawn((
                Mesh3d(handle.clone()),
                MeshMaterial3d(assets.opaque.clone()),
                Transform::from_translation(Vec3::new(pos.x, 0.0, pos.y)).with_scale(Vec3::splat(1.6)),
                Duck { rng: Rng(rng.next_u64()), pos, target: pos, yaw: i as f32 * 2.1, pause: 0.0, seed: i as f32 * 1.7, generation: world.generation },
            ));
        }
    }
}

fn update_ducks(time: Res<Time>, world: Res<LifeWorld>, mut ducks: Query<(&mut Transform, &mut Duck)>) {
    let Some(scene) = &world.scene else { return };
    let (dt, t) = (time.delta_secs().min(0.1), time.elapsed_secs());
    let ok = |p: Vec2| world.is_water(p) && world.water.len() > 1;
    for (mut tf, mut d) in &mut ducks {
        if d.generation != world.generation {
            d.generation = world.generation;
            d.target = d.pos;
            d.pause = 0.1;
            if !ok(d.pos) && !world.water.is_empty() {
                d.pos = world.water[(d.rng.next_u64() % world.water.len() as u64) as usize];
            }
        }
        if d.pause > 0.0 {
            d.pause -= dt;
            if d.pause <= 0.0 {
                let (from, rng) = (d.pos, &mut d.rng);
                match pick_wander(rng, from, 4.0, &ok) {
                    Some(p) => d.target = p,
                    None => d.pause = 2.0,
                }
            }
        } else {
            let delta = d.target - d.pos;
            if delta.length() < 0.2 {
                d.pause = d.rng.range(2.0, 6.0);
            } else {
                let want = delta.x.atan2(delta.y);
                d.yaw = turn_towards(d.yaw, want, 2.0 * dt);
                let step = Vec2::new(d.yaw.sin(), d.yaw.cos()) * 0.35 * dt * (want - d.yaw).cos().max(0.0);
                if ok(d.pos + step) {
                    d.pos += step;
                } else {
                    d.target = d.pos;
                    d.pause = 0.5;
                }
            }
        }
        let level = scene.terrain.water_level(d.pos).unwrap_or(0.0);
        let bob = (t * 1.6 + d.seed).sin() * 0.012;
        tf.translation = Vec3::new(d.pos.x, level - 0.02 + bob, d.pos.y);
        tf.rotation = Quat::from_rotation_y(d.yaw) * Quat::from_rotation_z((t * 1.1 + d.seed).sin() * 0.03);
    }
}

// ---------------------------------------------------------------- particle clouds

fn update_swifts(
    time: Res<Time>,
    env: Res<Environment>,
    world: Res<LifeWorld>,
    cloud: Query<&Mesh3d, With<SwiftCloud>>,
    mut meshes: ResMut<Assets<Mesh>>,
) {
    let Ok(m) = cloud.single() else { return };
    let Some(mut mesh) = meshes.get_mut(&m.0) else { return };
    let mut ps = Vec::new();
    if env.night < 0.3 && env.wetness < 0.5 && !world.towers.is_empty() {
        let t = time.elapsed_secs();
        for i in 0..SWIFTS {
            let c = world.towers[i % world.towers.len()];
            let f = i as f32;
            let dir = if i % 2 == 0 { 1.0 } else { -1.0 };
            let ang = f * 1.3 + dir * (t * (0.7 + 0.08 * (i % 3) as f32) + 0.9 * (0.6 * t + f).sin());
            let r = 3.5 + (i % 3) as f32 * 1.6 + 0.8 * (0.4 * t + f).sin();
            let y = c.y + 2.5 + (i % 4) as f32 * 0.9 + 0.7 * (1.3 * t + f * 2.0).sin();
            let pos = Vec3::new(c.x + ang.cos() * r, y, c.z + ang.sin() * r);
            ps.push(Particle::new(ParticleKind::Bird, pos, 1.1, Vec4::new(0.08, 0.08, 0.1, 1.0)));
        }
    }
    write_particle_mesh(&mut mesh, &ps);
}

fn update_steam(
    time: Res<Time>,
    env: Res<Environment>,
    built: Res<BuiltScene>,
    cloud: Query<&Mesh3d, With<SteamCloud>>,
    mut meshes: ResMut<Assets<Mesh>>,
) {
    let Ok(m) = cloud.single() else { return };
    let Some(mut mesh) = meshes.get_mut(&m.0) else { return };
    let mut ps = Vec::new();
    let t = time.elapsed_secs();
    let shade = 0.9 - 0.5 * env.night;
    if let Some(scene) = &built.0 {
        for (k, mk) in scene.markers.iter().filter(|m| m.kind == MarkerKind::ChimneySmoke).take(MAX_STEAM).enumerate() {
            for i in 0..10 {
                let age = (t * 0.22 + i as f32 / 10.0 + k as f32 * 0.37).fract();
                let drift = env.wind * age * 0.8;
                let pos = mk.pos + Vec3::new(drift.x + (age * 9.0 + i as f32).sin() * 0.15, age * 4.5, drift.y);
                let c = Vec3::splat(shade).lerp(Vec3::splat(shade * 0.7), age);
                ps.push(Particle::new(ParticleKind::Puff, pos, 0.6 + age * 1.3, c.extend((1.3 - 1.1 * age).clamp(0.0, 1.0))));
            }
        }
    }
    write_particle_mesh(&mut mesh, &ps);
}

fn update_fireflies(
    time: Res<Time>,
    env: Res<Environment>,
    world: Res<LifeWorld>,
    cloud: Query<&Mesh3d, With<FireflyCloud>>,
    mut meshes: ResMut<Assets<Mesh>>,
) {
    let Ok(m) = cloud.single() else { return };
    let Some(mut mesh) = meshes.get_mut(&m.0) else { return };
    let mut ps = Vec::new();
    if env.night > 0.25 && env.wetness < 0.6 {
        let t = time.elapsed_secs();
        for (h, home) in world.fly_homes.iter().enumerate() {
            for i in 0..3 {
                let s = h as f32 * 3.1 + i as f32 * 1.9;
                let pos = *home
                    + Vec3::new(
                        (t * 0.31 + s).sin() * 1.1 + (t * 0.7 + s * 2.0).sin() * 0.3,
                        0.5 + 0.4 * (t * 0.5 + s * 1.3).sin(),
                        (t * 0.27 + s * 1.7).cos() * 1.1,
                    );
                let blink = (0.5 + 0.7 * (t * 1.7 + s * 2.3).sin()).clamp(0.0, 1.0);
                ps.push(Particle::new(ParticleKind::Spark, pos, 0.3, Vec4::new(1.0, 0.92, 0.4, blink * env.night)));
            }
        }
    }
    write_particle_mesh(&mut mesh, &ps);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn in_glade(p: Vec2) -> bool {
        glade_dist(p) < -EDGE_MARGIN
    }

    #[test]
    fn wander_stays_in_glade() {
        let mut rng = Rng(5);
        let mut p = Vec2::new(20.0, 20.0);
        for _ in 0..500 {
            let t = pick_wander(&mut rng, p, 7.0, &in_glade).expect("target");
            assert!(in_glade(t) && (t - p).length() <= 7.0 + 1e-3);
            p = t;
        }
    }

    #[test]
    fn avoids_walls() {
        // Tall wall along x = 3, z in -10..10.
        let a = Vec3::new(3.0, 0.0, -10.0);
        let b = Vec3::new(3.0, 0.0, 10.0);
        let up = Vec3::Y * 3.0;
        let obs = Obstacles::from_tris([[a, b, b + up], [a, b + up, a + up]].into_iter());
        assert!(obs.blocked(Vec2::new(3.2, 0.0), 0.5));
        assert!(!obs.blocked(Vec2::new(8.0, 0.0), 0.5));
        let ok = |p: Vec2| in_glade(p) && !obs.blocked(p, 1.0);
        let mut rng = Rng(9);
        for _ in 0..300 {
            let t = pick_wander(&mut rng, Vec2::new(-3.0, 0.0), 7.0, &ok).expect("target");
            assert!(t.x < 2.0 || t.y.abs() > 10.5 || t.x > 4.0 && t.y.abs() > 10.5, "crossed wall: {t}");
        }
    }

    #[test]
    fn turning_takes_short_way() {
        assert!((turn_towards(0.1, std::f32::consts::TAU - 0.1, 1.0) - (-0.1)).abs() < 1e-4);
    }
}
