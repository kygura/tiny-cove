//! Ambient life (DESIGN "Life and ambience"): robot sheep, fireflies near crops, steam
//! from rooftop tanks and motes around crystals / runes. Everything is a small cuboid
//! with shared materials; the world is sampled only when `DisplayedGrid::version`
//! changes (debounced), never per frame.

use crate::game::DisplayedGrid;
use crate::render::{Atmosphere, ToonMaterial, toon_material};
use crate::shot::CliArgs;
use crate::weather::{Rng, WeatherState};
use acre_core::voxel::{NX, NY, NZ};
use acre_core::{ORIGIN, VOXEL_SIZE, Voxel, VoxelGrid};
use bevy::light::NotShadowCaster;
use bevy::prelude::*;
use std::collections::HashMap;
use std::f32::consts::TAU;

const SHEEP_SPEED: f32 = 0.6;
const FIREFLY_CAP: usize = 120;
const MOTE_CAP: usize = 160;
const STEAM_CAP: usize = 12;
const STEAM_LIFE: f32 = 3.0;
const STEAM_PER_SOURCE: usize = 6;

pub struct LifePlugin;

impl Plugin for LifePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<LifeState>()
            .add_systems(Startup, setup_assets)
            .add_systems(
                Update,
                (rescan, sheep_update, firefly_update, steam_update, mote_update).chain(),
            );
    }
}

// ---------------------------------------------------------------------------
// World sampling (pure)
// ---------------------------------------------------------------------------

/// Things found in the world that props attach to.
#[derive(Default, Clone, Debug, PartialEq)]
pub struct Sources {
    /// Sub-sampled Crop voxel centres.
    pub crops: Vec<Vec3>,
    /// Steam outlets on top of roof-mounted tanks.
    pub tanks: Vec<Vec3>,
    /// Crystal / rune cluster centres with their cluster size class.
    pub motes: Vec<(Vec3, u8)>,
    /// Grass/Meadow-topped columns.
    pub grass_cols: usize,
}

fn vox_world(x: i32, y: i32, z: i32) -> Vec3 {
    Vec3::new(
        ORIGIN + (x as f32 + 0.5) * VOXEL_SIZE,
        (y as f32 + 0.5) * VOXEL_SIZE,
        ORIGIN + (z as f32 + 0.5) * VOXEL_SIZE,
    )
}

fn is_ground(v: Voxel) -> bool {
    matches!(v, Voxel::Soil | Voxel::Grass | Voxel::Rock | Voxel::Sand | Voxel::Asphalt | Voxel::Gravel | Voxel::Water)
}

/// One pass over every column from the top down to the natural ground.
pub fn scan(grid: &VoxelGrid) -> Sources {
    let mut out = Sources::default();
    let mut bins: HashMap<(i32, i32, i32), (Vec3, u32, bool)> = HashMap::new();
    let mut tank_cells: HashMap<(i32, i32), Vec3> = HashMap::new();
    for z in 0..NZ {
        for x in 0..NX {
            let Some(top) = (0..NY).rev().find(|&y| grid.get(x, y, z) != Voxel::Air) else { continue };
            let top_kind = grid.get(x, top, z);
            if matches!(top_kind, Voxel::Grass | Voxel::Meadow) {
                out.grass_cols += 1;
            }
            // Roof tank: Rust (and Steel legs/rim) stack resting on Concrete.
            if top_kind == Voxel::Rust {
                let mut y = top;
                while y >= 0 && matches!(grid.get(x, y, z), Voxel::Rust | Voxel::Steel | Voxel::Air) {
                    y -= 1;
                }
                if y >= 0 && matches!(grid.get(x, y, z), Voxel::Concrete | Voxel::ConcreteDark) {
                    let p = vox_world(x, top, z) + Vec3::Y * 0.6;
                    let key = (x / 8, z / 8);
                    let e = tank_cells.entry(key).or_insert(p);
                    if p.y > e.y {
                        *e = p;
                    }
                }
            }
            for y in (0..=top).rev() {
                let v = grid.get(x, y, z);
                match v {
                    Voxel::Crop if (x + z) % 3 == 0 => out.crops.push(vox_world(x, y, z)),
                    Voxel::Crystal | Voxel::Rune => {
                        let e = bins
                            .entry((x / 8, y / 8, z / 8))
                            .or_insert((Vec3::ZERO, 0, v == Voxel::Rune));
                        e.0 += vox_world(x, y, z);
                        e.1 += 1;
                    }
                    _ if is_ground(v) => break,
                    _ => {}
                }
            }
        }
    }
    out.crops.truncate(2000);
    let mut tanks: Vec<_> = tank_cells.into_values().collect();
    tanks.sort_by(|a, b| a.x.total_cmp(&b.x).then(a.z.total_cmp(&b.z)));
    tanks.truncate(STEAM_CAP);
    out.tanks = tanks;
    let mut motes: Vec<_> = bins
        .into_values()
        .filter(|b| b.1 >= 4)
        .map(|(sum, n, rune)| (sum / n as f32, if rune { 4 } else { 4 + (n / 12).min(4) as u8 }))
        .collect();
    motes.sort_by(|a, b| a.0.x.total_cmp(&b.0.x).then(a.0.z.total_cmp(&b.0.z)));
    motes.truncate(24);
    out.motes = motes;
    out
}

fn sources_hash(s: &Sources) -> u64 {
    let mut h = 0xcbf2_9ce4_8422_2325u64;
    let mut mix = |v: Vec3| {
        for f in [v.x, v.y, v.z] {
            h = (h ^ f.to_bits() as u64).wrapping_mul(0x100_0000_01b3);
        }
    };
    s.crops.iter().step_by(7).for_each(|&v| mix(v));
    s.tanks.iter().for_each(|&v| mix(v));
    s.motes.iter().for_each(|&(v, n)| mix(v + Vec3::splat(n as f32)));
    h ^ (s.crops.len().checked_ilog2().unwrap_or(0) as u64)
}

#[inline]
fn cell(v: f32) -> i32 {
    ((v - ORIGIN) / VOXEL_SIZE).floor() as i32
}

/// Top voxel kind and y of the column under world (x, z).
fn column(grid: &VoxelGrid, x: f32, z: f32) -> Option<(Voxel, i32)> {
    let (ix, iz) = (cell(x), cell(z));
    if !VoxelGrid::in_bounds(ix, 0, iz) {
        return None;
    }
    let t = grid.top(ix, iz);
    (t >= 0).then(|| (grid.get(ix, t, iz), t))
}

/// Walking surface height (wu); tufts are walked through.
fn ground_h(grid: &VoxelGrid, x: f32, z: f32) -> Option<f32> {
    let (k, t) = column(grid, x, z)?;
    Some(if k == Voxel::Meadow { t as f32 * VOXEL_SIZE + 0.1 } else { (t + 1) as f32 * VOXEL_SIZE })
}

fn is_walkable(grid: &VoxelGrid, x: f32, z: f32) -> bool {
    matches!(column(grid, x, z), Some((Voxel::Grass | Voxel::Meadow, _)))
}

/// No structure / tree / crystal column within `r` wu (8 samples).
fn clear_around(grid: &VoxelGrid, x: f32, z: f32, r: f32) -> bool {
    (0..8).all(|i| {
        let a = i as f32 * TAU / 8.0;
        match column(grid, x + a.cos() * r, z + a.sin() * r) {
            None => false,
            Some((k, _)) => !(k.is_structure() || k.is_vegetation() && k != Voxel::Meadow || k == Voxel::Crystal),
        }
    })
}

/// May a sheep stand at (x, z), coming from a surface at `from_h`?
fn can_stand(grid: &VoxelGrid, x: f32, z: f32, from_h: Option<f32>) -> bool {
    if !is_walkable(grid, x, z) {
        return false;
    }
    match (ground_h(grid, x, z), from_h) {
        (Some(h), Some(f)) if (h - f).abs() > VOXEL_SIZE + 0.15 => false,
        (None, _) => false,
        _ => true,
    }
}

// ---------------------------------------------------------------------------
// State, assets, components
// ---------------------------------------------------------------------------

#[derive(Resource)]
struct LifeState {
    seen_version: u64,
    last_scan: f32,
    hash: u64,
    rng: Rng,
    time: f32,
}

impl Default for LifeState {
    fn default() -> Self {
        Self { seen_version: u64::MAX, last_scan: -10.0, hash: 0, rng: Rng::new(42), time: 0.0 }
    }
}

#[derive(Resource)]
struct LifeAssets {
    wool: Handle<ToonMaterial>,
    steel: Handle<ToonMaterial>,
    slate: Handle<ToonMaterial>,
    eye: Handle<ToonMaterial>,
    firefly: Handle<StandardMaterial>,
    mote: Handle<StandardMaterial>,
    steam: Handle<StandardMaterial>,
    cube: Handle<Mesh>,
    body: Handle<Mesh>,
    leg: Handle<Mesh>,
    head: Handle<Mesh>,
    eye_mesh: Handle<Mesh>,
    antenna: Handle<Mesh>,
}

#[derive(Component)]
struct Sheep {
    pos: Vec2,
    y: f32,
    yaw: f32,
    target: Option<Vec2>,
    graze: f32,
    walk_phase: f32,
    dip: f32,
    recheck: bool,
}

#[derive(Component)]
struct HeadPivot;

#[derive(Component)]
struct Firefly {
    home: Vec3,
    phase: f32,
    period: f32,
    freq: Vec3,
}

#[derive(Component)]
struct Puff {
    origin: Vec3,
    offset: f32,
}

#[derive(Component)]
struct Mote {
    center: Vec3,
    r: f32,
    omega: f32,
    phase: f32,
    bob: f32,
}

fn setup_assets(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut toon: ResMut<Assets<ToonMaterial>>,
    mut std: ResMut<Assets<StandardMaterial>>,
    args: Option<Res<CliArgs>>,
    mut state: ResMut<LifeState>,
) {
    if let Some(a) = args.as_deref() {
        state.rng = Rng::new(a.seed.unwrap_or(1) ^ 0x5eed);
    }
    let c = |r: u8, g: u8, b: u8| Color::srgb_u8(r, g, b);
    let teal = c(0x5c, 0xf0, 0xe0);
    let lime = c(0xe6, 0xff, 0x8a);
    let glow = |col: Color, k: f32| {
        let l = col.to_linear();
        StandardMaterial {
            base_color: Color::linear_rgba(l.red * k, l.green * k, l.blue * k, 1.0),
            unlit: true,
            alpha_mode: AlphaMode::Blend,
            ..default()
        }
    };
    commands.insert_resource(LifeAssets {
        wool: toon.add(toon_material(c(0xec, 0xe6, 0xda), Color::BLACK, 0.0, 0.0)),
        steel: toon.add(toon_material(c(0x7d, 0x8a, 0x96), Color::BLACK, 0.0, 0.0)),
        slate: toon.add(toon_material(c(0x5a, 0x56, 0x70), Color::BLACK, 0.0, 0.0)),
        eye: toon.add(toon_material(teal, teal, 0.6, 2.0)),
        // Blended unlit: no prepass, so the outline pass leaves the specks alone; HDR
        // values >1 still feed the bloom.
        firefly: std.add(glow(lime, 1.5)),
        mote: std.add(glow(teal, 1.0)),
        steam: std.add(StandardMaterial {
            base_color: Color::srgba_u8(0xf4, 0xf1, 0xec, 190),
            unlit: true,
            alpha_mode: AlphaMode::Blend,
            ..default()
        }),
        cube: meshes.add(Cuboid::new(1.0, 1.0, 1.0)),
        body: meshes.add(Cuboid::new(1.5, 1.0, 2.0)),
        leg: meshes.add(Cuboid::new(0.25, 0.3, 0.25)),
        head: meshes.add(Cuboid::new(0.9, 0.9, 0.9)),
        eye_mesh: meshes.add(Cuboid::new(0.22, 0.22, 0.1)),
        antenna: meshes.add(Cuboid::new(0.06, 0.45, 0.06)),
    });
}

// ---------------------------------------------------------------------------
// Rescan: debounced; refreshes sources, prop sets and sheep validity
// ---------------------------------------------------------------------------

#[derive(Component)]
struct Prop;

#[allow(clippy::too_many_arguments)]
fn rescan(
    time: Res<Time>,
    grid: Res<DisplayedGrid>,
    assets: Option<Res<LifeAssets>>,
    mut state: ResMut<LifeState>,
    mut commands: Commands,
    props: Query<Entity, With<Prop>>,
    mut sheep: Query<(Entity, &mut Sheep)>,
) {
    let Some(assets) = assets else { return };
    state.time += time.delta_secs();
    if grid.version == state.seen_version || state.time - state.last_scan < 0.5 {
        return;
    }
    if grid.grid.top(NX / 2, NZ / 2) < 0 {
        return; // world not built yet
    }
    state.seen_version = grid.version;
    state.last_scan = state.time;
    let src = scan(&grid.grid);

    // Sheep count follows grass area (each column is 0.25 wu^2).
    let area = src.grass_cols as f32 * VOXEL_SIZE * VOXEL_SIZE;
    let want = if area < 80.0 { 0 } else { (8.0 + area / 1200.0).clamp(8.0, 14.0) as usize };
    let have: Vec<Entity> = sheep.iter().map(|(e, _)| e).collect();
    for (_, mut s) in &mut sheep {
        s.recheck = true;
    }
    for &e in have.iter().skip(want) {
        commands.entity(e).despawn();
    }
    for _ in have.len()..want {
        let mut rng = state.rng;
        if let Some(p) = random_spot(&grid.grid, &mut rng, None) {
            spawn_sheep(&mut commands, &assets, p, rng.range(0.0, TAU), &mut rng);
        }
        state.rng = rng;
    }

    // Other props only when their sources actually changed.
    let h = sources_hash(&src);
    if h == state.hash {
        return;
    }
    state.hash = h;
    for e in &props {
        commands.entity(e).despawn();
    }
    let mut rng = Rng::new(h | 1);
    if !src.crops.is_empty() {
        let n = (8 + src.crops.len() / 3).min(FIREFLY_CAP);
        for _ in 0..n {
            let c = src.crops[(rng.next_u64() as usize) % src.crops.len()];
            let home = c + Vec3::new(rng.range(-3.0, 3.0), rng.range(0.8, 2.6), rng.range(-3.0, 3.0));
            commands.spawn((
                Prop,
                Firefly {
                    home,
                    phase: rng.range(0.0, 100.0),
                    period: 0.6 + rng.range(1.0, 3.0),
                    freq: Vec3::new(rng.range(0.15, 0.3), rng.range(0.3, 0.6), rng.range(0.15, 0.3)),
                },
                Mesh3d(assets.cube.clone()),
                MeshMaterial3d(assets.firefly.clone()),
                Transform::from_translation(home).with_scale(Vec3::splat(0.14)),
                Visibility::Hidden,
                NotShadowCaster,
            ));
        }
    }
    for &t in &src.tanks {
        for i in 0..STEAM_PER_SOURCE {
            commands.spawn((
                Prop,
                Puff { origin: t, offset: i as f32 * STEAM_LIFE / STEAM_PER_SOURCE as f32 },
                Mesh3d(assets.cube.clone()),
                MeshMaterial3d(assets.steam.clone()),
                Transform::from_translation(t).with_scale(Vec3::ZERO),
                Visibility::Hidden,
                NotShadowCaster,
            ));
        }
    }
    let mut total = 0;
    for &(c, size) in &src.motes {
        for _ in 0..size {
            if total >= MOTE_CAP {
                break;
            }
            total += 1;
            let r = rng.range(0.8, 1.8);
            let speed = rng.range(0.5, 1.5) * if rng.f32() < 0.5 { 1.0 } else { -1.0 };
            commands.spawn((
                Prop,
                Mote { center: c, r, omega: speed / r, phase: rng.range(0.0, TAU), bob: rng.range(0.0, TAU) },
                Mesh3d(assets.cube.clone()),
                MeshMaterial3d(assets.mote.clone()),
                Transform::from_translation(c).with_scale(Vec3::splat(0.14)),
                NotShadowCaster,
            ));
        }
    }
}

// ---------------------------------------------------------------------------
// Sheep
// ---------------------------------------------------------------------------

/// A valid resting spot: grass, flat, nothing solid within 1.6 wu. Prefers `near`.
fn random_spot(grid: &VoxelGrid, rng: &mut Rng, near: Option<Vec2>) -> Option<Vec2> {
    for i in 0..120 {
        let p = match near {
            Some(n) if i < 60 => n + Vec2::new(rng.range(-12.0, 12.0), rng.range(-12.0, 12.0)),
            _ => Vec2::new(rng.range(-42.0, 42.0), rng.range(-42.0, 42.0)),
        };
        if can_stand(grid, p.x, p.y, None) && clear_around(grid, p.x, p.y, 1.6) {
            return Some(p);
        }
    }
    None
}

fn spawn_sheep(commands: &mut Commands, a: &LifeAssets, p: Vec2, yaw: f32, rng: &mut Rng) {
    let y = 0.0;
    commands
        .spawn((
            Sheep {
                pos: p,
                y,
                yaw,
                target: None,
                graze: rng.range(0.0, 4.0),
                walk_phase: rng.range(0.0, TAU),
                dip: 0.0,
                recheck: false,
            },
            Transform::from_xyz(p.x, y, p.y).with_rotation(Quat::from_rotation_y(yaw)),
            Visibility::Inherited,
        ))
        .with_children(|c| {
            c.spawn((Mesh3d(a.body.clone()), MeshMaterial3d(a.wool.clone()), Transform::from_xyz(0.0, 0.8, 0.0)));
            for (sx, sz) in [(-0.5, -0.7), (0.5, -0.7), (-0.5, 0.7), (0.5, 0.7)] {
                c.spawn((Mesh3d(a.leg.clone()), MeshMaterial3d(a.steel.clone()), Transform::from_xyz(sx, 0.15, sz)));
            }
            c.spawn((HeadPivot, Transform::from_xyz(0.0, 1.0, 0.9), Visibility::Inherited)).with_children(|h| {
                h.spawn((Mesh3d(a.head.clone()), MeshMaterial3d(a.slate.clone()), Transform::from_xyz(0.0, 0.0, 0.5)))
                    .with_children(|hd| {
                        hd.spawn((
                            Mesh3d(a.eye_mesh.clone()),
                            MeshMaterial3d(a.eye.clone()),
                            Transform::from_xyz(0.0, 0.08, 0.47),
                        ));
                        hd.spawn((
                            Mesh3d(a.antenna.clone()),
                            MeshMaterial3d(a.steel.clone()),
                            Transform::from_xyz(0.2, 0.65, -0.1),
                        ));
                    });
            });
        });
}

fn sheep_update(
    time: Res<Time>,
    grid: Res<DisplayedGrid>,
    mut state: ResMut<LifeState>,
    mut sheep: Query<(&mut Sheep, &mut Transform, &Children)>,
    mut heads: Query<&mut Transform, (With<HeadPivot>, Without<Sheep>)>,
) {
    let dt = time.delta_secs().min(0.1);
    let g = &grid.grid;
    let mut rng = state.rng;
    for (mut s, mut tf, children) in &mut sheep {
        let here_ok = can_stand(g, s.pos.x, s.pos.y, None);
        if s.recheck {
            s.recheck = false;
            if !here_ok {
                if let Some(p) = random_spot(g, &mut rng, Some(s.pos)) {
                    s.pos = p;
                    s.target = None;
                    s.graze = rng.range(0.5, 2.0);
                }
            } else if !clear_around(g, s.pos.x, s.pos.y, 1.0) {
                s.target = None; // wander off on the next pick
                s.graze = s.graze.min(0.3);
            }
        }
        let here_h = ground_h(g, s.pos.x, s.pos.y);
        let mut walking = false;
        if s.graze > 0.0 {
            s.graze -= dt;
            if s.graze <= 0.0 {
                // Pick a nearby target we can plausibly walk to.
                s.target = None;
                for _ in 0..8 {
                    let a = rng.range(0.0, TAU);
                    let d = rng.range(3.0, 10.0);
                    let t = s.pos + Vec2::new(a.cos(), a.sin()) * d;
                    let mid = (s.pos + t) * 0.5;
                    if can_stand(g, t.x, t.y, None)
                        && clear_around(g, t.x, t.y, 1.6)
                        && can_stand(g, mid.x, mid.y, None)
                    {
                        s.target = Some(t);
                        break;
                    }
                }
                if s.target.is_none() {
                    s.graze = rng.range(1.0, 3.0);
                }
            }
        } else if let Some(t) = s.target {
            let to = t - s.pos;
            let dist = to.length();
            if dist < 0.25 {
                s.target = None;
                s.graze = rng.range(3.0, 8.0);
            } else {
                let dir = to / dist;
                // Turn towards the target, then walk when roughly aligned.
                let want = dir.x.atan2(dir.y);
                let mut da = (want - s.yaw + std::f32::consts::PI).rem_euclid(TAU) - std::f32::consts::PI;
                da = da.clamp(-3.0 * dt, 3.0 * dt);
                s.yaw += da;
                let (sy, cy) = s.yaw.sin_cos();
                let fwd = Vec2::new(sy, cy);
                if (want - s.yaw).abs() < 0.6 || da.abs() < 3.0 * dt {
                    let step = fwd * SHEEP_SPEED * dt;
                    let next = s.pos + step;
                    let ahead = s.pos + fwd * 1.0;
                    let escaping = !clear_around(g, s.pos.x, s.pos.y, 1.0);
                    let ok = can_stand(g, next.x, next.y, here_h)
                        && (escaping || clear_around(g, ahead.x, ahead.y, 1.0));
                    if ok {
                        s.pos = next;
                        walking = true;
                    } else {
                        // Blocked by structure / water / steep step: stop and re-pick.
                        s.target = None;
                        s.graze = rng.range(0.5, 2.0);
                        s.yaw += 0.8;
                    }
                }
            }
        } else {
            s.graze = rng.range(0.5, 2.0);
        }

        if walking {
            s.walk_phase += dt * 7.0;
        }
        let want_dip = if s.graze > 0.0 && s.target.is_none() { 0.75 } else { 0.0 };
        s.dip += (want_dip - s.dip) * (dt * 6.0).min(1.0);
        let gh = ground_h(g, s.pos.x, s.pos.y).unwrap_or(s.y);
        s.y = if s.y == 0.0 { gh } else { s.y + (gh - s.y) * (dt * 8.0).min(1.0) };
        let bob = if walking { 0.1 * s.walk_phase.sin().abs() } else { 0.0 };
        tf.translation = Vec3::new(s.pos.x, s.y + bob, s.pos.y);
        tf.rotation = Quat::from_rotation_y(s.yaw);
        for &c in children {
            if let Ok(mut h) = heads.get_mut(c) {
                h.rotation = Quat::from_rotation_x(s.dip);
            }
        }
    }
    state.rng = rng;
}

// ---------------------------------------------------------------------------
// Particles
// ---------------------------------------------------------------------------

fn firefly_update(
    time: Res<Time>,
    atmos: Res<Atmosphere>,
    mut flies: Query<(&Firefly, &mut Transform, &mut Visibility)>,
) {
    let t = time.elapsed_secs();
    let night_on = atmos.night > 0.6;
    for (f, mut tf, mut vis) in &mut flies {
        let lit = night_on && (t + f.phase).rem_euclid(f.period) < 0.6;
        let want = if lit { Visibility::Inherited } else { Visibility::Hidden };
        if *vis != want {
            *vis = want;
        }
        if lit {
            let a = t + f.phase;
            tf.translation = f.home
                + Vec3::new(
                    (a * f.freq.x).sin() * 1.2,
                    (a * f.freq.y).sin() * 0.4,
                    (a * f.freq.z).cos() * 1.2,
                );
        }
    }
}

fn steam_update(
    time: Res<Time>,
    weather: Res<WeatherState>,
    mut puffs: Query<(&Puff, &mut Transform, &mut Visibility)>,
) {
    let t = time.elapsed_secs();
    let drift = weather.wind_dir * (0.25 + 0.12 * weather.params.wind);
    let calm = (1.0 - weather.storm).clamp(0.0, 1.0);
    for (p, mut tf, mut vis) in &mut puffs {
        let age = (t + p.offset).rem_euclid(STEAM_LIFE);
        let k = age / STEAM_LIFE;
        // Grow to ~0.7 wu by 40%, then shrink away (opaque-ish puffs fade by size).
        let size = calm * if k < 0.4 { 0.25 + 1.1 * k } else { 0.69 * (1.0 - (k - 0.4) / 0.6) };
        if size < 0.02 {
            if *vis != Visibility::Hidden {
                *vis = Visibility::Hidden;
            }
            continue;
        }
        *vis = Visibility::Inherited;
        tf.translation = p.origin + Vec3::new(drift.x * age, 0.8 * age, drift.y * age);
        tf.scale = Vec3::splat(size);
    }
}

fn mote_update(time: Res<Time>, mut motes: Query<(&Mote, &mut Transform)>) {
    let t = time.elapsed_secs();
    for (m, mut tf) in &mut motes {
        let a = m.phase + m.omega * t;
        let bob = (TAU * t / 4.0 + m.bob).sin() * 0.4;
        tf.translation = m.center + Vec3::new(a.cos() * m.r, bob, a.sin() * m.r);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn flat_grid() -> VoxelGrid {
        let mut g = VoxelGrid::new();
        for z in 0..NZ {
            for x in 0..NX {
                g.set(x, 0, z, Voxel::Soil);
                g.set(x, 1, z, Voxel::Grass);
            }
        }
        g
    }

    #[test]
    fn scan_finds_sources() {
        let mut g = flat_grid();
        // Crop patch, crystal cluster, tank on a concrete roof.
        for x in 10..16 {
            for z in 10..16 {
                g.set(x, 2, z, Voxel::Crop);
            }
        }
        for y in 2..8 {
            g.set(40, y, 40, Voxel::Crystal);
            g.set(41, y, 40, Voxel::Crystal);
        }
        for y in 2..14 {
            g.set(80, y, 80, Voxel::Concrete);
        }
        g.set(80, 14, 80, Voxel::Rust);
        g.set(80, 15, 80, Voxel::Rust);
        // A pylon on the ground is not a steam source.
        for y in 2..10 {
            g.set(120, y, 120, Voxel::Rust);
        }
        let s = scan(&g);
        assert!(!s.crops.is_empty());
        assert_eq!(s.tanks.len(), 1);
        assert_eq!(s.motes.len(), 1);
        assert!(s.grass_cols > 30_000);
    }

    #[test]
    fn standing_rules() {
        let mut g = flat_grid();
        let (x, z) = (ORIGIN + 20.0, ORIGIN + 20.0);
        assert!(can_stand(&g, x, z, None));
        assert!(clear_around(&g, x, z, 1.6));
        // Wall nearby blocks clearance.
        for y in 2..10 {
            g.set(cell(x) + 3, y, cell(z), Voxel::Concrete);
        }
        g.set(cell(x) + 3, 10, cell(z), Voxel::Concrete);
        assert!(!clear_around(&g, x, z, 1.5));
        // Water and structure tops are not walkable; a 3-voxel step is too steep.
        g.set(cell(x), 1, cell(z), Voxel::Water);
        assert!(!can_stand(&g, x, z, None));
        let h = ground_h(&g, x + 4.0, z + 4.0);
        for y in 2..5 {
            g.set(cell(x + 6.0), y, cell(z + 6.0), Voxel::Grass);
        }
        assert!(!can_stand(&g, x + 6.0, z + 6.0, h));
    }

    #[test]
    fn random_spot_is_valid() {
        let g = flat_grid();
        let mut rng = Rng::new(5);
        for _ in 0..20 {
            let p = random_spot(&g, &mut rng, None).unwrap();
            assert!(can_stand(&g, p.x, p.y, None));
        }
        assert!(random_spot(&VoxelGrid::new(), &mut rng, None).is_none());
    }
}
