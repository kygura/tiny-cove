//! Ambient life: sheep, birds, chimney smoke, fireflies and ducks.
//!
//! Everything re-derives its targets when the `Arc<Scene>` inside [`BuiltScene`] changes
//! (pointer compare). Sheep and ducks are pooled entities driven from one system; birds,
//! smoke and fireflies are particles written into a single shared particle mesh.
//! The steering / target-selection maths lives in plain functions with unit tests.
//!
//! `--life-test [out.png] [--test-env NAME] [--life-frames N] [--life-zoom S]` is a dev path
//! that builds the demo scene itself, screenshots and exits (zoom < 1 follows a sheep).

use crate::{
    env::Environment,
    render::{GameCamera, GladeAssets, Particle, ParticleKind, camera_transform, to_bevy_mesh, write_particle_mesh},
    weather::{Weather, WeatherKind},
    world::BuiltScene,
};
use bevy::{camera::visibility::NoFrustumCulling, prelude::*, render::view::screenshot::ScreenshotCaptured};
use glade_core::{GLADE_HALF, MarkerKind, MeshData, Scene, Surface, mesh::Rng, palette};
use std::{f32::consts::{PI, TAU}, path::PathBuf, sync::Arc};

const MAX_SHEEP: usize = 8;
const MAX_PONDS: usize = 6;
const MAX_DUCKS: usize = MAX_PONDS * 2;
const FLIES: usize = 40;
const MAX_SMOKE: usize = 160;
const SHEEP_SCALE: f32 = 1.5;
const DUCK_SCALE: f32 = 1.6;

// ---------------------------------------------------------------- pure logic

/// Wraps an angle to `[-PI, PI)`.
pub fn wrap_angle(a: f32) -> f32 {
    (a + PI).rem_euclid(TAU) - PI
}

/// Turns `yaw` towards `target` by at most `max_step` radians.
pub fn turn_toward(yaw: f32, target: f32, max_step: f32) -> f32 {
    yaw + wrap_angle(target - yaw).clamp(-max_step, max_step)
}

/// True if every sample along `a -> b` (every `step` metres, ends included) satisfies `ok`.
pub fn segment_clear(a: Vec2, b: Vec2, step: f32, ok: &impl Fn(Vec2) -> bool) -> bool {
    let n = ((b - a).length() / step).ceil().max(1.0) as u32;
    (1..=n).all(|i| ok(a.lerp(b, i as f32 / n as f32)))
}

/// True if `p` is inside any `(min, max)` XZ box grown by `margin`.
pub fn near_box(p: Vec2, boxes: &[(Vec2, Vec2)], margin: f32) -> bool {
    boxes.iter().any(|(lo, hi)| p.x > lo.x - margin && p.x < hi.x + margin && p.y > lo.y - margin && p.y < hi.y + margin)
}

/// Random walk target at `dist.0..dist.1` metres from `from`, sometimes near a door.
/// Rejects targets (and straight paths to them) failing `ok`.
pub fn pick_wander_target(rng: &mut Rng, from: Vec2, doors: &[Vec2], dist: (f32, f32), ok: &impl Fn(Vec2) -> bool) -> Option<Vec2> {
    for _ in 0..12 {
        let a = rng.range(0.0, TAU);
        let polar = |r: f32| Vec2::new(a.cos(), a.sin()) * r;
        let p = if !doors.is_empty() && rng.f() < 0.25 {
            doors[(rng.f() * doors.len() as f32) as usize % doors.len()] + polar(rng.range(1.2, 3.5))
        } else {
            from + polar(rng.range(dist.0, dist.1))
        };
        if ok(p) && segment_clear(from, p, 0.75, ok) {
            return Some(p);
        }
    }
    None
}

/// Walkable for sheep: inside the glade, dry, gentle slope, clear of structure boxes.
pub fn grass_ok(scene: &Scene, boxes: &[(Vec2, Vec2)], p: Vec2) -> bool {
    let t = &scene.terrain;
    let dry = |q: Vec2| t.water_level(q).is_none();
    p.abs().max_element() < GLADE_HALF - 1.0
        && dry(p)
        && dry(p + Vec2::X * 0.6)
        && dry(p - Vec2::X * 0.6)
        && dry(p + Vec2::Y * 0.6)
        && dry(p - Vec2::Y * 0.6)
        && t.normal(p).y > 0.92
        && !near_box(p, boxes, 1.5)
}

/// Open water within `radius` of a pond centre (ducks).
pub fn pond_ok(scene: &Scene, pond: Vec3, p: Vec2) -> bool {
    let t = &scene.terrain;
    p.distance(pond.xz()) < 4.0 && t.water_level(p).is_some_and(|l| t.height(p) < l - 0.2)
}

/// Point on a circle of `radius` around `centre`, `lead` radians ahead (counter-clockwise)
/// of the bearing of `pos`.
pub fn orbit_goal(centre: Vec2, pos: Vec2, radius: f32, lead: f32) -> Vec2 {
    let r = pos - centre;
    let a = if r.length_squared() < 1e-6 { 0.0 } else { r.y.atan2(r.x) } + lead;
    centre + Vec2::new(a.cos(), a.sin()) * radius
}

/// Accelerates `vel` towards `desired` by at most `max_dv`.
pub fn steer(vel: Vec3, desired: Vec3, max_dv: f32) -> Vec3 {
    vel + (desired - vel).clamp_length_max(max_dv)
}

/// Push away from flockmates closer than `min_d`.
pub fn separation(i: usize, pos: &[Vec3], min_d: f32) -> Vec3 {
    let mut push = Vec3::ZERO;
    for (j, q) in pos.iter().enumerate() {
        let d = pos[i] - *q;
        let l = d.length();
        if j != i && l < min_d && l > 1e-4 {
            push += d / l * (1.0 - l / min_d);
        }
    }
    push
}

// ---------------------------------------------------------------- state

struct Sheep {
    pos: Vec2,
    yaw: f32,
    target: Option<Vec2>,
    pause: f32,
    head: f32,
    phase: f32,
}

struct Duck {
    pond: Vec3,
    pos: Vec2,
    yaw: f32,
    target: Option<Vec2>,
    pause: f32,
    phase: f32,
}

#[derive(Clone, Copy, PartialEq)]
enum BirdMode {
    Circle,
    Land(Vec3),
    Perched(Vec3),
}

struct Bird {
    pos: Vec3,
    vel: Vec3,
    radius: f32,
    alt: f32,
    mode: BirdMode,
    timer: f32,
}

struct Smoke {
    pos: Vec3,
    age: f32,
    life: f32,
    jit: f32,
}

#[derive(Resource)]
struct Life {
    scene: Option<Arc<Scene>>,
    boxes: Vec<(Vec2, Vec2)>,
    doors: Vec<Vec2>,
    chimneys: Vec<Vec3>,
    perches: Vec<Vec3>,
    ponds: Vec<Vec3>,
    flower_spots: Vec<Vec3>,
    sheep: Vec<Sheep>,
    ducks: Vec<Duck>,
    birds: Vec<Bird>,
    smoke: Vec<Smoke>,
    flies: Vec<(Vec3, f32)>,
    smoke_acc: f32,
    smoke_next: usize,
    t: f32,
    rng: Rng,
    buf: Vec<Particle>,
}

impl Life {
    fn new() -> Self {
        let mut rng = Rng(0x7157_71E6);
        let n = 6 + (rng.f() * 3.0) as usize;
        let birds = (0..n)
            .map(|i| {
                let radius = rng.range(10.0, 22.0);
                let a = i as f32 / n as f32 * TAU;
                let alt = rng.range(12.0, 20.0);
                Bird { pos: Vec3::new(a.cos() * radius, alt, a.sin() * radius), vel: Vec3::ZERO, radius, alt, mode: BirdMode::Circle, timer: 0.0 }
            })
            .collect();
        Self {
            scene: None,
            boxes: vec![],
            doors: vec![],
            chimneys: vec![],
            perches: vec![],
            ponds: vec![],
            flower_spots: vec![],
            sheep: vec![],
            ducks: vec![],
            birds,
            smoke: vec![],
            flies: vec![],
            smoke_acc: 0.0,
            smoke_next: 0,
            t: 0.0,
            rng,
            buf: Vec::with_capacity(256),
        }
    }

    /// Re-derives everything that depends on the scene.
    fn refresh(&mut self, scene: Arc<Scene>) {
        let at = |kind| scene.markers.iter().filter(move |m| m.kind == kind).map(|m| m.pos);
        self.doors = at(MarkerKind::Door).map(|p| p.xz()).collect();
        self.chimneys = at(MarkerKind::ChimneySmoke).collect();
        self.perches = at(MarkerKind::Perch).collect();
        let ponds: Vec<Vec3> = at(MarkerKind::Pond).take(MAX_PONDS).collect();
        self.boxes = scene
            .parts
            .iter()
            .filter_map(|p| p.mesh.bounds())
            .filter(|(lo, hi)| hi.y - lo.y > 0.4)
            .map(|(lo, hi)| (lo.xz(), hi.xz()))
            .collect();
        // Flower spots: scatter vertices using the petal tile (strided to keep it small).
        let petal = Surface::Petal as u8;
        let flowers: Vec<usize> = (0..scene.scatter.surfaces.len()).filter(|&i| scene.scatter.surfaces[i] == petal).collect();
        let stride = (flowers.len() / 300).max(1);
        self.flower_spots = flowers.iter().step_by(stride).map(|&i| Vec3::from_array(scene.scatter.positions[i])).collect();

        let boxes = &self.boxes;
        let ok = |p: Vec2| grass_ok(&scene, boxes, p);
        let rng = &mut self.rng;
        let random_spot = |rng: &mut Rng| {
            (0..80).map(|_| Vec2::new(rng.range(-1.0, 1.0), rng.range(-1.0, 1.0)) * (GLADE_HALF - 2.0)).find(|&p| ok(p))
        };
        // Sheep: spawn on first scene, afterwards only rescue the ones that got built over.
        if self.sheep.is_empty() && self.scene.is_none() {
            let n = 4 + (rng.f() * 5.0) as usize;
            for _ in 0..n.min(MAX_SHEEP) {
                if let Some(pos) = random_spot(rng) {
                    self.sheep.push(Sheep { pos, yaw: rng.range(-PI, PI), target: None, pause: rng.range(0.0, 3.0), head: 0.0, phase: 0.0 });
                }
            }
        }
        for s in &mut self.sheep {
            if !ok(s.pos) {
                if let Some(p) = random_spot(rng) {
                    s.pos = p;
                }
            }
            s.target = None;
        }
        // Ducks: rebuilt only if the set of ponds changed.
        if ponds != self.ponds {
            self.ducks.clear();
            for pond in &ponds {
                for k in 0..1 + (rng.f() * 2.0) as usize {
                    let off = Vec2::new(rng.range(-1.0, 1.0), rng.range(-1.0, 1.0)) * 1.2;
                    if pond_ok(&scene, *pond, pond.xz() + off) && self.ducks.len() < MAX_DUCKS {
                        self.ducks.push(Duck { pond: *pond, pos: pond.xz() + off, yaw: k as f32 * 2.0, target: None, pause: rng.range(0.0, 2.0), phase: rng.range(0.0, TAU) });
                    }
                }
            }
            self.ponds = ponds;
        }
        // Fireflies: mostly flowers, some over water, else open grass.
        self.flies.clear();
        for k in 0..FLIES {
            let base = if k % 3 == 2 && !self.ponds.is_empty() {
                let p = self.ponds[(rng.f() * self.ponds.len() as f32) as usize % self.ponds.len()];
                let a = rng.range(0.0, TAU);
                let r = rng.range(0.5, 3.5);
                Some(Vec3::new(p.x + a.cos() * r, p.y + 0.5, p.z + a.sin() * r))
            } else if !self.flower_spots.is_empty() {
                Some(self.flower_spots[(rng.f() * self.flower_spots.len() as f32) as usize % self.flower_spots.len()])
            } else {
                random_spot(rng).map(|p| Vec3::new(p.x, scene.terrain.height(p), p.y))
            };
            if let Some(b) = base {
                self.flies.push((b + Vec3::Y * rng.range(0.4, 1.2), rng.range(0.0, TAU)));
            }
        }
        // Birds that lost their perch take off.
        for b in &mut self.birds {
            if let BirdMode::Land(p) | BirdMode::Perched(p) = b.mode {
                if !self.perches.iter().any(|q| q.distance(p) < 0.3) {
                    b.mode = BirdMode::Circle;
                }
            }
        }
        self.scene = Some(scene);
    }
}

// ---------------------------------------------------------------- meshes & components

#[derive(Component)]
struct SheepBody(usize);
#[derive(Component)]
struct SheepHead(usize);
#[derive(Component)]
struct DuckBody(usize);
#[derive(Component)]
struct LifeParticles;

fn sheep_meshes() -> (MeshData, MeshData) {
    let wool = palette::white() * 0.95;
    let dark = Vec3::new(0.09, 0.075, 0.07);
    let mut body = MeshData::default();
    for (z, r, h) in [(-0.2, 0.26, 0.38), (0.0, 0.31, 0.45), (0.2, 0.26, 0.38)] {
        body.cylinder(Vec3::new(0.0, 0.27, z), r, r * 0.8, h, 7, true, true, wool, Surface::Fiber, 1.0);
    }
    body.aabb(Vec3::new(-0.06, 0.42, -0.5), Vec3::new(0.06, 0.55, -0.36), wool, Surface::Fiber, 1.0);
    for (x, z) in [(-0.14, -0.22), (0.14, -0.22), (-0.14, 0.22), (0.14, 0.22)] {
        body.aabb(Vec3::new(x - 0.035, 0.0, z - 0.035), Vec3::new(x + 0.035, 0.34, z + 0.035), dark, Surface::Fiber, 1.0);
    }
    let mut head = MeshData::default();
    head.aabb(Vec3::new(-0.075, -0.1, 0.0), Vec3::new(0.075, 0.08, 0.25), dark, Surface::Fiber, 1.0);
    for s in [-1.0f32, 1.0] {
        let (a, b) = (0.07 * s, 0.17 * s);
        head.aabb(Vec3::new(a.min(b), 0.02, 0.0), Vec3::new(a.max(b), 0.07, 0.09), dark, Surface::Fiber, 1.0);
    }
    head.aabb(Vec3::new(-0.07, 0.07, -0.03), Vec3::new(0.07, 0.13, 0.08), wool, Surface::Fiber, 1.0);
    (body, head)
}

fn duck_mesh() -> MeshData {
    let body = Vec3::new(0.42, 0.3, 0.18);
    let green = Vec3::new(0.06, 0.28, 0.13);
    let mut m = MeshData::default();
    m.cylinder(Vec3::new(0.0, 0.0, -0.05), 0.12, 0.09, 0.13, 6, true, true, body, Surface::Fiber, 1.0);
    m.cylinder(Vec3::new(0.0, 0.0, 0.06), 0.1, 0.08, 0.12, 6, true, true, body, Surface::Fiber, 1.0);
    m.cylinder(Vec3::new(0.0, 0.1, 0.13), 0.045, 0.04, 0.17, 5, true, false, green, Surface::Fiber, 1.0);
    m.aabb(Vec3::new(-0.025, 0.2, 0.15), Vec3::new(0.025, 0.23, 0.25), Vec3::new(0.9, 0.55, 0.1), Surface::Petal, 1.0);
    m.aabb(Vec3::new(-0.03, 0.1, -0.2), Vec3::new(0.03, 0.17, -0.1), body, Surface::Fiber, 1.0);
    m
}

fn spawn_pool(mut commands: Commands, assets: Res<GladeAssets>, mut meshes: ResMut<Assets<Mesh>>) {
    let mat = assets.opaque.clone();
    let (body, head) = sheep_meshes();
    let (body, head) = (meshes.add(to_bevy_mesh(&body)), meshes.add(to_bevy_mesh(&head)));
    for i in 0..MAX_SHEEP {
        commands
            .spawn((SheepBody(i), Mesh3d(body.clone()), MeshMaterial3d(mat.clone()), Transform::from_scale(Vec3::splat(SHEEP_SCALE)), Visibility::Hidden))
            .with_child((SheepHead(i), Mesh3d(head.clone()), MeshMaterial3d(mat.clone()), Transform::from_xyz(0.0, 0.62, 0.36)));
    }
    let duck = meshes.add(to_bevy_mesh(&duck_mesh()));
    for i in 0..MAX_DUCKS {
        commands.spawn((DuckBody(i), Mesh3d(duck.clone()), MeshMaterial3d(mat.clone()), Transform::from_scale(Vec3::splat(DUCK_SCALE)), Visibility::Hidden));
    }
    commands.spawn((
        LifeParticles,
        Mesh3d(meshes.add(crate::render::particle_mesh(&[]))),
        MeshMaterial3d(assets.particle.clone()),
        NoFrustumCulling,
        Transform::default(),
    ));
}

// ---------------------------------------------------------------- update

#[allow(clippy::type_complexity)]
fn update_life(
    time: Res<Time>,
    built: Res<BuiltScene>,
    env: Res<Environment>,
    weather: Res<Weather>,
    mut life: ResMut<Life>,
    mut meshes: ResMut<Assets<Mesh>>,
    particles: Query<&Mesh3d, With<LifeParticles>>,
    mut sheep_q: Query<(&SheepBody, &mut Transform, &mut Visibility), (Without<SheepHead>, Without<DuckBody>)>,
    mut head_q: Query<(&SheepHead, &mut Transform), (Without<SheepBody>, Without<DuckBody>)>,
    mut duck_q: Query<(&DuckBody, &mut Transform, &mut Visibility), (Without<SheepHead>, Without<SheepBody>)>,
) {
    let dt = time.delta_secs().min(0.1);
    if let Some(new) = &built.0 {
        if !life.scene.as_ref().is_some_and(|old| Arc::ptr_eq(old, new)) {
            life.refresh(new.clone());
        }
    }
    let Some(scene) = life.scene.clone() else { return };
    let life = &mut *life;
    life.t += dt;
    let t = life.t;
    let Life { boxes, doors, chimneys, perches, sheep, ducks, birds, smoke, flies, rng, buf, .. } = life;

    // Sheep.
    let ok = |p: Vec2| grass_ok(&scene, boxes, p);
    for s in sheep.iter_mut() {
        if let Some(goal) = s.target {
            let d = goal - s.pos;
            if d.length() < 0.2 {
                s.target = None;
                s.pause = rng.range(3.0, 9.0);
            } else {
                let want = d.x.atan2(d.y);
                s.yaw = turn_toward(s.yaw, want, 3.0 * dt);
                if wrap_angle(want - s.yaw).abs() < 0.8 {
                    let np = s.pos + Vec2::new(s.yaw.sin(), s.yaw.cos()) * 0.6 * dt;
                    if ok(np) {
                        s.pos = np;
                        s.phase += dt * 7.0;
                    } else {
                        s.target = None;
                        s.pause = 1.0;
                    }
                }
            }
        } else {
            s.pause -= dt;
            if s.pause <= 0.0 {
                s.target = pick_wander_target(rng, s.pos, doors, (3.0, 9.0), &ok);
                s.pause = if s.target.is_none() { 2.0 } else { 0.0 };
            }
        }
        let want_head = if s.target.is_none() { 1.0 } else { 0.0 };
        s.head += (want_head - s.head) * (dt * 3.0).min(1.0);
    }
    for (id, mut tf, mut vis) in &mut sheep_q {
        let Some(s) = sheep.get(id.0) else {
            *vis = Visibility::Hidden;
            continue;
        };
        *vis = Visibility::Inherited;
        let hop = if s.target.is_some() { s.phase.sin().abs() * 0.07 } else { 0.0 };
        tf.translation = Vec3::new(s.pos.x, scene.terrain.height(s.pos) + hop, s.pos.y);
        tf.rotation = Quat::from_rotation_arc(Vec3::Y, scene.terrain.normal(s.pos)) * Quat::from_rotation_y(s.yaw);
    }
    for (id, mut tf) in &mut head_q {
        let Some(s) = sheep.get(id.0) else { continue };
        let chew = s.head * (t * 6.0 + id.0 as f32).sin() * 0.06;
        tf.rotation = Quat::from_rotation_x(s.head * 0.95 + chew);
        tf.translation = Vec3::new(0.0, 0.62 - s.head * 0.2, 0.36 + s.head * 0.08);
    }

    // Ducks.
    for d in ducks.iter_mut() {
        let pond_ok = |p: Vec2| pond_ok(&scene, d.pond, p);
        if let Some(goal) = d.target {
            let v = goal - d.pos;
            if v.length() < 0.25 {
                d.target = None;
                d.pause = rng.range(1.0, 4.0);
            } else {
                let want = v.x.atan2(v.y);
                d.yaw = turn_toward(d.yaw, want, 1.5 * dt);
                let np = d.pos + Vec2::new(d.yaw.sin(), d.yaw.cos()) * 0.35 * dt;
                if pond_ok(np) {
                    d.pos = np;
                } else {
                    d.target = None;
                    d.pause = 0.5;
                }
            }
        } else {
            d.pause -= dt;
            if d.pause <= 0.0 {
                d.target = pick_wander_target(rng, d.pos, &[], (1.0, 3.5), &pond_ok);
                d.pause = if d.target.is_none() { 1.0 } else { 0.0 };
            }
        }
    }
    for (id, mut tf, mut vis) in &mut duck_q {
        let Some(d) = ducks.get(id.0) else {
            *vis = Visibility::Hidden;
            continue;
        };
        *vis = Visibility::Inherited;
        let level = scene.terrain.water_level(d.pos).unwrap_or(d.pond.y);
        tf.translation = Vec3::new(d.pos.x, level - 0.03 + (t * 2.0 + d.phase).sin() * 0.015, d.pos.y);
        tf.rotation = Quat::from_rotation_y(d.yaw) * Quat::from_rotation_z((t * 1.3 + d.phase).sin() * 0.04);
    }

    // Particles: smoke, fireflies, birds.
    buf.clear();
    let wind = Vec3::new(env.wind.x, 0.0, env.wind.y);
    let cold = env.snow > 0.2;
    if !chimneys.is_empty() {
        life.smoke_acc += (if cold { 1.0 } else { 1.7 }) * chimneys.len() as f32 * dt;
    }
    while life.smoke_acc >= 1.0 {
        life.smoke_acc -= 1.0;
        if smoke.len() < MAX_SMOKE && !chimneys.is_empty() {
            let c = chimneys[life.smoke_next % chimneys.len()];
            life.smoke_next += 1;
            let jit = rng.range(0.0, TAU);
            smoke.push(Smoke { pos: c + Vec3::new(jit.cos(), 0.0, jit.sin()) * 0.08, age: 0.0, life: if cold { 6.5 } else { 4.5 }, jit });
        }
    }
    smoke.retain_mut(|s| {
        s.age += dt;
        let k = (s.age / s.life).min(1.0);
        s.pos += (Vec3::Y * if cold { 0.55 } else { 0.95 } + wind * (0.15 + 0.5 * k) + Vec3::new((s.age * 1.5 + s.jit).sin(), 0.0, (s.age * 1.2 + s.jit).cos()) * 0.12) * dt;
        s.age < s.life
    });
    for s in smoke.iter() {
        let k = s.age / s.life;
        let base = if cold { Vec3::new(0.95, 0.96, 1.0) } else { Vec3::new(0.7, 0.67, 0.64) };
        let col = base.lerp(Vec3::splat(0.9), k * 0.6);
        buf.push(Particle::new(ParticleKind::Puff, s.pos, 0.35 + 1.3 * k, col.extend((1.0 - k) * (s.age * 5.0).min(1.0) * 0.85)));
    }

    let vis = ((env.night - 0.5) / 0.25).clamp(0.0, 1.0);
    if vis > 0.0 {
        for (base, ph) in flies.iter() {
            let pos = *base + Vec3::new((t * 0.7 + ph).sin() * 0.8, (t * 1.3 + ph * 3.0).sin() * 0.25, (t * 0.5 + ph * 2.0).cos() * 0.8);
            let blink = 0.5 + 0.5 * (t * 2.2 + ph * 5.0).sin();
            buf.push(Particle::new(ParticleKind::Spark, pos, 0.3, Vec4::new(0.9, 1.0, 0.4, vis * (0.3 + 0.7 * blink))));
        }
    }

    if env.night <= 0.5 {
        let storm = weather.kind == WeatherKind::Storm;
        let centre = Vec2::ZERO;
        let flock: Vec<Vec3> = birds.iter().map(|b| b.pos).collect();
        let perched = birds.iter().filter(|b| matches!(b.mode, BirdMode::Perched(_) | BirdMode::Land(_))).count();
        let mut taken: Vec<Vec3> = birds.iter().filter_map(|b| if let BirdMode::Land(p) | BirdMode::Perched(p) = b.mode { Some(p) } else { None }).collect();
        for (i, b) in birds.iter_mut().enumerate() {
            let (goal, speed) = match b.mode {
                BirdMode::Perched(p) => {
                    b.pos = p;
                    b.vel = Vec3::ZERO;
                    b.timer -= dt;
                    if storm || b.timer <= 0.0 {
                        b.mode = BirdMode::Circle;
                        b.vel = Vec3::Y * 2.5;
                    }
                    continue;
                }
                BirdMode::Land(p) => {
                    let d = p.distance(b.pos);
                    if storm {
                        b.mode = BirdMode::Circle;
                    } else if d < 0.35 {
                        b.mode = BirdMode::Perched(p);
                        b.timer = rng.range(12.0, 30.0);
                    }
                    (p, d.clamp(1.5, 5.0))
                }
                BirdMode::Circle => {
                    if !storm && perched < 3 && rng.f() < dt / 35.0 && !perches.is_empty() {
                        let p = perches[(rng.f() * perches.len() as f32) as usize % perches.len()];
                        if !taken.iter().any(|q| q.distance(p) < 0.5) {
                            taken.push(p);
                            b.mode = BirdMode::Land(p);
                        }
                    }
                    if storm {
                        let away = if b.pos.xz().length() < 1.0 { Vec2::X } else { b.pos.xz().normalize() };
                        (Vec3::new(away.x * 90.0, 24.0, away.y * 90.0), 9.0)
                    } else {
                        let g = orbit_goal(centre, b.pos.xz(), b.radius, 0.7);
                        (Vec3::new(g.x, b.alt + (t * 0.4 + i as f32).sin() * 1.5, g.y), 5.0)
                    }
                }
            };
            let desired = (goal - b.pos).normalize_or_zero() * speed + separation(i, &flock, 2.5) * 2.0;
            b.vel = steer(b.vel, desired, 8.0 * dt);
            b.pos += b.vel * dt;
        }
        for b in birds.iter() {
            let size = if matches!(b.mode, BirdMode::Perched(_)) { 0.9 } else { 1.3 };
            buf.push(Particle::new(ParticleKind::Bird, b.pos, size, Vec4::new(0.12, 0.12, 0.16, 1.0)));
        }
    }

    if let Ok(handle) = particles.single() {
        if let Some(mut mesh) = meshes.get_mut(&handle.0) {
            write_particle_mesh(&mut mesh, buf);
        }
    }
}

// ---------------------------------------------------------------- plugin

pub struct LifePlugin;

impl Plugin for LifePlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(Life::new())
            .add_systems(Startup, spawn_pool)
            .add_systems(Update, update_life);
        if let Some(test) = LifeTest::from_args(std::env::args()) {
            app.insert_resource(test)
                .add_systems(Startup, dev_scene.after(spawn_pool))
                .add_systems(Update, dev_camera)
                .add_systems(Last, dev_capture);
        }
    }
}

// ---------------------------------------------------------------- dev path

/// Present with `--life-test`.
#[derive(Resource, Clone, Debug)]
struct LifeTest {
    path: PathBuf,
    env: String,
    zoom: f32,
    duck: bool,
    frames: u32,
    shoot_at: u32,
    requested: bool,
}

impl LifeTest {
    fn from_args(args: impl IntoIterator<Item = String>) -> Option<Self> {
        let args: Vec<String> = args.into_iter().collect();
        let i = args.iter().position(|a| a == "--life-test")?;
        let value = |flag: &str| args.iter().position(|a| a == flag).and_then(|i| args.get(i + 1)).cloned();
        let path = args.get(i + 1).filter(|a| !a.starts_with("--")).cloned().unwrap_or_else(|| "life.png".into());
        Some(Self {
            path: path.into(),
            env: value("--test-env").unwrap_or_else(|| "noon".into()),
            zoom: value("--life-zoom").and_then(|z| z.parse().ok()).unwrap_or(1.0),
            duck: args.iter().any(|a| a == "--life-duck"),
            frames: 0,
            shoot_at: value("--life-frames").and_then(|z| z.parse().ok()).unwrap_or(300),
            requested: false,
        })
    }
}

fn dev_scene(
    mut commands: Commands,
    test: Res<LifeTest>,
    assets: Res<GladeAssets>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut built: ResMut<BuiltScene>,
    mut env: ResMut<Environment>,
    mut weather: ResMut<Weather>,
) {
    let scene = Arc::new(glade_core::build(&glade_core::demo::demo_document(42)));
    let mut spawn = |m: &MeshData| {
        if !m.is_empty() {
            commands.spawn((Mesh3d(meshes.add(to_bevy_mesh(m))), MeshMaterial3d(assets.opaque.clone())));
        }
    };
    spawn(&scene.ground);
    spawn(&scene.scatter);
    scene.parts.iter().for_each(|p| spawn(&p.mesh));
    if !scene.water.is_empty() {
        commands.spawn((Mesh3d(meshes.add(to_bevy_mesh(&scene.water))), MeshMaterial3d(assets.water.clone())));
    }
    built.0 = Some(scene);
    if let Some(e) = crate::render::test_scene::preset(&test.env) {
        *env = e;
    }
    if test.env == "storm" {
        weather.kind = WeatherKind::Storm;
    }
}

/// Zoomed shots follow the first sheep (or duck with `--life-duck`).
fn dev_camera(test: Res<LifeTest>, life: Res<Life>, mut cam: Query<(&mut Transform, &mut Projection), With<GameCamera>>) {
    let focus = if test.zoom >= 1.0 {
        None
    } else if test.duck {
        life.ducks.first().map(|d| Vec3::new(d.pos.x, d.pond.y, d.pos.y))
    } else {
        life.sheep.first().map(|s| Vec3::new(s.pos.x, 0.5, s.pos.y))
    };
    for (mut tf, mut proj) in &mut cam {
        if let Projection::Orthographic(o) = &mut *proj {
            o.scale = test.zoom;
        }
        if let Some(f) = focus {
            *tf = camera_transform(f, crate::render::DEFAULT_YAW, crate::render::DEFAULT_PITCH);
        }
    }
}

fn dev_capture(life: Res<Life>, mut commands: Commands, mut test: ResMut<LifeTest>, mut exit: MessageWriter<AppExit>) {
    test.frames += 1;
    if test.frames >= test.shoot_at + 600 {
        error!("life test: screenshot never arrived");
        exit.write(AppExit::error());
    } else if test.frames >= test.shoot_at && !test.requested {
        test.requested = true;
        info!("life: {} sheep, {} ducks, {} ponds, {} birds, {} smoke, {} flies, {} chimneys, {} perches", life.sheep.len(), life.ducks.len(), life.ponds.len(), life.birds.len(), life.smoke.len(), life.flies.len(), life.chimneys.len(), life.perches.len());
        crate::render::request_screenshot(&mut commands, test.path.clone())
            .observe(|_: On<ScreenshotCaptured>, mut exit: MessageWriter<AppExit>| {
                exit.write(AppExit::Success);
            });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn angles_wrap_and_turn() {
        assert!((wrap_angle(3.0 * PI) - -PI).abs() < 1e-4 || (wrap_angle(3.0 * PI) - PI).abs() < 1e-4);
        assert!((turn_toward(0.0, 3.0, 0.5) - 0.5).abs() < 1e-6);
        assert!((turn_toward(3.0, -3.0, 1.0) - (3.0 + TAU - 6.0)).abs() < 1e-5); // short way round
        assert_eq!(turn_toward(1.0, 1.2, 0.5), 1.2);
    }

    #[test]
    fn wander_respects_ok_and_path() {
        let mut rng = Rng(7);
        let ok = |p: Vec2| p.x > 0.0;
        for _ in 0..50 {
            let t = pick_wander_target(&mut rng, Vec2::new(10.0, 0.0), &[], (3.0, 9.0), &ok).unwrap();
            assert!(t.x > 0.0);
        }
        // A wall at x in [4, 5] between start and every target on the far side.
        let wall = |p: Vec2| !(4.0..5.0).contains(&p.x);
        for _ in 0..50 {
            if let Some(t) = pick_wander_target(&mut rng, Vec2::new(1.0, 0.0), &[], (3.0, 9.0), &wall) {
                assert!(t.x < 4.0);
            }
        }
        assert_eq!(pick_wander_target(&mut rng, Vec2::ZERO, &[], (3.0, 9.0), &|_| false), None);
    }

    #[test]
    fn wander_visits_doors() {
        let mut rng = Rng(3);
        let door = Vec2::new(20.0, 20.0);
        let near = (0..200)
            .filter_map(|_| pick_wander_target(&mut rng, Vec2::ZERO, &[door], (3.0, 9.0), &|_| true))
            .filter(|t| t.distance(door) < 4.0)
            .count();
        assert!(near > 10);
    }

    #[test]
    fn boxes_have_margin() {
        let b = [(Vec2::ZERO, Vec2::splat(2.0))];
        assert!(near_box(Vec2::new(3.0, 1.0), &b, 1.5));
        assert!(!near_box(Vec2::new(4.0, 1.0), &b, 1.5));
    }

    #[test]
    fn orbit_goal_is_on_circle_and_ahead() {
        let c = Vec2::new(1.0, 2.0);
        let g = orbit_goal(c, c + Vec2::new(5.0, 0.0), 12.0, 0.5);
        assert!((g.distance(c) - 12.0).abs() < 1e-4);
        assert!(g.y > c.y);
    }

    #[test]
    fn steering_is_limited_and_separation_repels() {
        let v = steer(Vec3::ZERO, Vec3::X * 10.0, 1.0);
        assert!((v.length() - 1.0).abs() < 1e-6);
        let pos = [Vec3::ZERO, Vec3::new(1.0, 0.0, 0.0), Vec3::new(50.0, 0.0, 0.0)];
        assert!(separation(0, &pos, 2.5).x < 0.0);
        assert_eq!(separation(2, &pos, 2.5), Vec3::ZERO);
    }

    #[test]
    fn creature_meshes_are_valid() {
        let (b, h) = sheep_meshes();
        assert!(b.is_valid() && h.is_valid() && duck_mesh().is_valid());
    }

    #[test]
    fn demo_scene_has_walkable_grass() {
        let scene = glade_core::build(&glade_core::demo::demo_document(42));
        let mut life = Life::new();
        life.refresh(Arc::new(scene));
        assert!(life.sheep.len() >= 3, "sheep: {}", life.sheep.len());
        let scene = life.scene.clone().unwrap();
        assert!(life.sheep.iter().all(|s| grass_ok(&scene, &life.boxes, s.pos)));
        assert!(!life.flies.is_empty());
    }
}
