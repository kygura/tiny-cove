//! Ash, smoke and walking figures, all drawn with the PS1 voxel material.

use crate::game::Doc;
use crate::render::{GameCamera, VoxelMaterialHandle, cube_mesh};
use crate::generate::stack_pos;
use crate::structure::{Structure, polyline_len};
use crate::voxel::Voxel;
use bevy::prelude::*;

pub struct AmbiencePlugin;

impl Plugin for AmbiencePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Roads>()
            .add_systems(Startup, setup)
            .add_systems(
                Update,
                (
                    rebuild.run_if(resource_exists_and_changed::<Doc>),
                    (drift_ash, rise_smoke, walk_figures),
                )
                    .chain(),
            );
    }
}

type Rebuilt = Or<(With<Smoke>, With<Figure>)>;

const ASH_COUNT: usize = 300;
const ASH_RADIUS: f32 = 25.0;
const ASH_TOP: f32 = 20.0;
const SMOKE_COLUMNS: usize = 3;
const SMOKE_PER_COLUMN: usize = 12;
const SMOKE_LIFE: f32 = 7.0;
const FIGURE_CAP: usize = 200;
const FIGURE_SPACING: f32 = 4.0;
const FIGURE_SPEED: f32 = 0.6;
const FIGURE_SIZE: Vec3 = Vec3::new(0.5, 1.0, 0.5);

#[derive(Resource)]
struct Assets3d {
    ash: Handle<Mesh>,
    smoke: Handle<Mesh>,
    figure: Handle<Mesh>,
}

/// Road polylines the figures walk, rebuilt when the document changes.
#[derive(Resource, Default)]
struct Roads(Vec<Vec<Vec2>>);

#[derive(Component)]
struct Ash {
    vel: Vec3,
}

#[derive(Component)]
struct Smoke {
    origin: Vec3,
    phase: f32,
    seed: f32,
}

#[derive(Component)]
struct Figure {
    road: usize,
    offset: f32,
}

/// Cheap deterministic hash to 0..1.
fn hash01(i: u32) -> f32 {
    let mut x = i.wrapping_mul(0x9E37_79B1) ^ 0x85EB_CA6B;
    x ^= x >> 15;
    x = x.wrapping_mul(0x2C1B_3C6D);
    x ^= x >> 12;
    (x & 0xffff) as f32 / 65536.0
}

fn setup(mut commands: Commands, mut meshes: ResMut<Assets<Mesh>>, material: Res<VoxelMaterialHandle>) {
    let assets = Assets3d {
        ash: meshes.add(cube_mesh(Voxel::Ash)),
        smoke: meshes.add(cube_mesh(Voxel::Scorch)),
        figure: meshes.add(cube_mesh(Voxel::ConcreteDark)),
    };
    for i in 0..ASH_COUNT as u32 {
        let r = |k: u32| hash01(i * 7 + k);
        commands.spawn((
            Ash {
                vel: Vec3::new(0.2 + r(1) * 0.5, -(0.3 + r(2) * 0.5), (r(3) - 0.5) * 0.4),
            },
            Mesh3d(assets.ash.clone()),
            MeshMaterial3d(material.0.clone()),
            Transform::from_xyz(
                (r(4) - 0.5) * 2.0 * ASH_RADIUS,
                r(5) * ASH_TOP,
                (r(6) - 0.5) * 2.0 * ASH_RADIUS,
            )
            .with_scale(Vec3::splat(0.1)),
        ));
    }
    commands.insert_resource(assets);
}

/// Total figures for roads of the given lengths: about one per [`FIGURE_SPACING`], capped.
fn figure_counts(lengths: &[f32], cap: usize) -> Vec<usize> {
    let mut left = cap;
    lengths
        .iter()
        .map(|&l| {
            let n = ((l / FIGURE_SPACING).round() as usize).min(left);
            left -= n;
            n
        })
        .collect()
}

/// Point and unit direction at distance `d` along the polyline (clamped to its ends).
fn point_at(pts: &[Vec2], d: f32) -> (Vec2, Vec2) {
    let mut left = d.max(0.0);
    let mut dir = Vec2::X;
    for w in pts.windows(2) {
        let len = w[0].distance(w[1]);
        if len > 0.0 {
            dir = (w[1] - w[0]) / len;
            if left <= len {
                return (w[0] + dir * left, dir);
            }
            left -= len;
        }
    }
    (pts.last().copied().unwrap_or_default(), dir)
}

fn rebuild(
    mut commands: Commands,
    doc: Res<Doc>,
    assets: Res<Assets3d>,
    material: Res<VoxelMaterialHandle>,
    mut roads: ResMut<Roads>,
    old: Query<Entity, Rebuilt>,
) {
    for e in &old {
        commands.entity(e).despawn();
    }

    // Smoke rises from the tallest blocks' real rooftop stacks.
    let mut stacks: Vec<Vec3> = doc
        .0
        .placed
        .iter()
        .filter_map(|p| match &p.structure {
            Structure::Block { min, max, height } => stack_pos(*min, *max, *height, doc.0.seed),
            _ => None,
        })
        .collect();
    stacks.sort_by(|a, b| b.y.total_cmp(&a.y));
    for (c, origin) in stacks.into_iter().take(SMOKE_COLUMNS).enumerate() {
        for i in 0..SMOKE_PER_COLUMN {
            commands.spawn((
                Smoke {
                    origin,
                    phase: i as f32 / SMOKE_PER_COLUMN as f32,
                    seed: hash01((c * 100 + i) as u32) * std::f32::consts::TAU,
                },
                Mesh3d(assets.smoke.clone()),
                MeshMaterial3d(material.0.clone()),
                Transform::from_translation(origin).with_scale(Vec3::ZERO),
            ));
        }
    }

    roads.0 = doc
        .0
        .placed
        .iter()
        .filter_map(|p| match &p.structure {
            Structure::Road { points } if points.len() >= 2 => Some(points.clone()),
            _ => None,
        })
        .collect();
    let lengths: Vec<f32> = roads.0.iter().map(|r| polyline_len(r)).collect();
    for (road, n) in figure_counts(&lengths, FIGURE_CAP).into_iter().enumerate() {
        for i in 0..n {
            commands.spawn((
                Figure { road, offset: lengths[road] * i as f32 / n as f32 },
                Mesh3d(assets.figure.clone()),
                MeshMaterial3d(material.0.clone()),
                Transform::from_scale(FIGURE_SIZE),
            ));
        }
    }
}

/// Where the camera looks on the ground (y = 0), or a point ahead if it looks up.
fn look_focus(cam: &Transform) -> Vec3 {
    let fwd = cam.forward().as_vec3();
    if fwd.y < -0.01 {
        cam.translation + fwd * (-cam.translation.y / fwd.y)
    } else {
        cam.translation + fwd * 30.0
    }
}

fn drift_ash(
    time: Res<Time>,
    cam: Single<&Transform, (With<GameCamera>, Without<Ash>)>,
    mut ash: Query<(&Ash, &mut Transform)>,
) {
    let focus = look_focus(&cam);
    let dt = time.delta_secs();
    let wrap = |d: f32| (d + ASH_RADIUS).rem_euclid(2.0 * ASH_RADIUS) - ASH_RADIUS;
    for (a, mut t) in &mut ash {
        t.translation += a.vel * dt;
        t.translation.x = focus.x + wrap(t.translation.x - focus.x);
        t.translation.z = focus.z + wrap(t.translation.z - focus.z);
        if t.translation.y < 0.0 {
            t.translation.y += ASH_TOP;
        }
    }
}

fn rise_smoke(time: Res<Time>, mut smoke: Query<(&Smoke, &mut Transform)>) {
    let now = time.elapsed_secs();
    for (s, mut t) in &mut smoke {
        let life = (now / SMOKE_LIFE + s.phase).fract();
        let sway = life * 2.0;
        t.translation = s.origin
            + Vec3::new(
                (s.seed + now * 0.5).sin() * sway,
                life * 10.0,
                (s.seed * 1.7 + now * 0.4).cos() * sway,
            );
        // grow, then shrink away at the end of its life
        t.scale = Vec3::splat((0.2 + life * 0.8) * (1.0 - life).min(0.25) * 4.0);
    }
}

fn walk_figures(time: Res<Time>, roads: Res<Roads>, mut figures: Query<(&Figure, &mut Transform)>) {
    let now = time.elapsed_secs();
    for (f, mut t) in &mut figures {
        let Some(pts) = roads.0.get(f.road) else { continue };
        let len = polyline_len(pts);
        if len <= 0.0 {
            continue;
        }
        let (p, dir) = point_at(pts, (f.offset + now * FIGURE_SPEED).rem_euclid(len));
        let bob = (now * 6.0 + f.offset * 3.0).sin().abs() * 0.04;
        t.translation = Vec3::new(p.x, FIGURE_SIZE.y * 0.5 + bob, p.y);
        t.rotation = Quat::from_rotation_y(-dir.y.atan2(dir.x));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn l_shape() -> Vec<Vec2> {
        vec![Vec2::ZERO, Vec2::new(3.0, 0.0), Vec2::new(3.0, 4.0)]
    }

    #[test]
    fn length_and_point_at() {
        let p = l_shape();
        assert_eq!(polyline_len(&p), 7.0);
        assert_eq!(point_at(&p, 1.5).0, Vec2::new(1.5, 0.0));
        let (pt, dir) = point_at(&p, 5.0);
        assert_eq!(pt, Vec2::new(3.0, 2.0));
        assert_eq!(dir, Vec2::Y);
        assert_eq!(point_at(&p, 99.0).0, Vec2::new(3.0, 4.0));
    }

    #[test]
    fn figure_counts_follow_length_and_cap() {
        assert_eq!(figure_counts(&[8.0, 16.0], 200), vec![2, 4]);
        assert_eq!(figure_counts(&[400.0, 400.0], 200), vec![100, 100]);
        assert_eq!(figure_counts(&[2000.0, 400.0], 200), vec![200, 0]);
    }

    #[test]
    fn hash_in_unit_range() {
        assert!((0..1000).all(|i| (0.0..1.0).contains(&hash01(i))));
    }
}
