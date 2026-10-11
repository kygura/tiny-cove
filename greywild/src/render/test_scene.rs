//! `--render-test out.png [--test-env noon|dusk|night|rain|storm|snow|fog] [--test-zoom S]`: a small
//! hand-built diorama (greywild_core mesh helpers only), rendered for a few frames, saved
//! and the app exits. Also the reference for how to spawn meshes/particles/lanterns.

use super::{GladeAssets, Particle, ParticleKind, lantern_light, particle_mesh, to_bevy_mesh};
use crate::env::Environment;
use bevy::{camera::visibility::NoFrustumCulling, prelude::*, render::view::screenshot::ScreenshotCaptured};
use greywild_core::{Colors, MeshData, Surface, palette};
use std::path::PathBuf;

const SETTLE_FRAMES: u32 = 90;
const TIMEOUT_FRAMES: u32 = 300;

/// Present when running with `--render-test`.
#[derive(Resource, Clone, Debug)]
pub struct RenderTest {
    pub path: PathBuf,
    pub env: String,
    /// Orthographic zoom (`--test-zoom`, projection scale; >1 = further out).
    pub zoom: f32,
    frames: u32,
    requested: bool,
}

impl RenderTest {
    /// Parses `--render-test PATH [--test-env NAME]`.
    pub fn from_args(args: impl IntoIterator<Item = String>) -> Option<Self> {
        let args: Vec<String> = args.into_iter().collect();
        let value = |flag: &str| args.iter().position(|a| a == flag).and_then(|i| args.get(i + 1)).cloned();
        let path = value("--render-test")?;
        Some(Self {
            path: path.into(),
            env: value("--test-env").unwrap_or_else(|| "noon".into()),
            zoom: value("--test-zoom").and_then(|z| z.parse().ok()).unwrap_or(1.0),
            frames: 0,
            requested: false,
        })
    }
}

pub(super) fn build(app: &mut App) {
    app.add_systems(Startup, spawn_test_scene.run_if(resource_exists::<RenderTest>))
        .add_systems(Update, animate_particles.run_if(resource_exists::<RenderTest>))
        .add_systems(Last, capture.run_if(resource_exists::<RenderTest>));
}

/// Test atmospheres (the weather module owns the real ones).
pub fn preset(name: &str) -> Option<Environment> {
    let d = Environment::default();
    Some(match name {
        "noon" => d,
        "dusk" => Environment {
            sun_dir: Vec3::new(0.85, 0.22, 0.35).normalize(),
            sun_color: Vec3::new(1.0, 0.55, 0.3) * 1.9,
            ambient_sky: Vec3::new(0.36, 0.32, 0.5),
            ambient_ground: Vec3::new(0.26, 0.18, 0.14),
            shadow_tint: Vec3::new(0.62, 0.52, 0.9),
            sky_top: Vec3::new(0.22, 0.26, 0.58),
            sky_horizon: Vec3::new(0.95, 0.55, 0.36),
            fog_color: Vec3::new(0.8, 0.5, 0.4),
            fog_density: 0.008,
            night: 0.35,
            hour: 19.0,
            ..d
        },
        "night" => Environment {
            sun_dir: Vec3::new(0.3, 0.7, -0.5).normalize(),
            sun_color: Vec3::new(0.35, 0.42, 0.7) * 0.7,
            ambient_sky: Vec3::new(0.09, 0.11, 0.22),
            ambient_ground: Vec3::new(0.05, 0.05, 0.08),
            shadow_tint: Vec3::new(0.55, 0.6, 1.0),
            sky_top: Vec3::new(0.01, 0.015, 0.05),
            sky_horizon: Vec3::new(0.05, 0.07, 0.15),
            fog_color: Vec3::new(0.05, 0.07, 0.15),
            fog_density: 0.008,
            night: 1.0,
            hour: 23.0,
            ..d
        },
        "rain" | "storm" => {
            let storm = name == "storm";
            let k = if storm { 0.6 } else { 1.0 };
            Environment {
                sun_color: Vec3::new(0.62, 0.64, 0.7) * 1.3 * k,
                ambient_sky: Vec3::new(0.42, 0.46, 0.52) * k,
                ambient_ground: Vec3::new(0.24, 0.24, 0.22) * k,
                shadow_tint: Vec3::new(0.72, 0.75, 0.88),
                sky_top: Vec3::new(0.3, 0.34, 0.4) * k,
                sky_horizon: Vec3::new(0.52, 0.56, 0.6) * k,
                fog_color: Vec3::new(0.52, 0.56, 0.6) * k,
                fog_density: 0.02,
                wetness: 1.0,
                flash: if storm { 0.5 } else { 0.0 },
                night: if storm { 0.3 } else { 0.0 },
                wind: Vec2::new(4.0, 1.0),
                ..d
            }
        }
        "snow" => Environment {
            sun_color: Vec3::new(1.0, 0.97, 0.95) * 1.9,
            ambient_sky: Vec3::new(0.5, 0.56, 0.68),
            sky_top: Vec3::new(0.45, 0.58, 0.8),
            sky_horizon: Vec3::new(0.85, 0.88, 0.93),
            fog_color: Vec3::new(0.85, 0.88, 0.93),
            fog_density: 0.015,
            snow: 1.0,
            ..d
        },
        "fog" => Environment {
            fog_density: 0.06,
            fog_color: Vec3::new(0.78, 0.8, 0.82),
            sky_horizon: Vec3::new(0.78, 0.8, 0.82),
            sky_top: Vec3::new(0.6, 0.66, 0.74),
            sun_color: Vec3::new(1.0, 0.96, 0.9) * 1.5,
            ..d
        },
        _ => return None,
    })
}

#[derive(Component)]
struct TestParticles {
    kind: ParticleKind,
}

/// greywild_core's `aabb`/`oriented_box` currently wind their faces inwards; build boxes
/// through these wrappers (flip winding + normals) until core is fixed.
trait OutwardBoxes {
    fn aabb_out(&mut self, min: Vec3, max: Vec3, color: Vec3, s: Surface, uv: f32);
    fn obox_out(&mut self, center: Vec3, half: Vec3, x_axis: Vec3, color: Vec3, s: Surface, uv: f32);
}

fn flipped(mut b: MeshData) -> MeshData {
    let inward = b.indices.chunks(3).next().is_some_and(|t| {
        let p = |i: u32| Vec3::from_array(b.positions[i as usize]);
        let c = b.bounds().map_or(Vec3::ZERO, |(lo, hi)| (lo + hi) * 0.5);
        let n = (p(t[1]) - p(t[0])).cross(p(t[2]) - p(t[0]));
        n.dot(p(t[0]) - c) < 0.0
    });
    if inward {
        for t in b.indices.chunks_mut(3) {
            t.swap(1, 2);
        }
        for n in &mut b.normals {
            *n = [-n[0], -n[1], -n[2]];
        }
    }
    b
}

impl OutwardBoxes for MeshData {
    fn aabb_out(&mut self, min: Vec3, max: Vec3, color: Vec3, s: Surface, uv: f32) {
        let mut b = MeshData::default();
        b.aabb(min, max, color, s, uv);
        self.append(&flipped(b));
    }
    fn obox_out(&mut self, center: Vec3, half: Vec3, x_axis: Vec3, color: Vec3, s: Surface, uv: f32) {
        let mut b = MeshData::default();
        b.oriented_box(center, half, x_axis, color, s, uv);
        self.append(&flipped(b));
    }
}

fn house(m: &mut MeshData, origin: Vec3, w: f32, d: f32, h: f32, colors: Colors, roof: Surface) {
    let wall = palette::wall(colors);
    let trim = palette::trim(colors);
    let roofc = palette::roof(colors);
    let o = origin;
    m.aabb_out(o + Vec3::new(-w / 2.0 - 0.1, 0.0, -d / 2.0 - 0.1), o + Vec3::new(w / 2.0 + 0.1, 0.5, d / 2.0 + 0.1), palette::foundation(), Surface::Plinth, 1.0);
    m.aabb_out(o + Vec3::new(-w / 2.0, 0.5, -d / 2.0), o + Vec3::new(w / 2.0, h, d / 2.0), wall, Surface::Earth, 1.0);
    // Half-timbering on the front (+z) face.
    let fz = o.z + d / 2.0 + 0.04;
    for x in [-w / 2.0, -w / 6.0, w / 6.0, w / 2.0] {
        m.obox_out(Vec3::new(o.x + x, o.y + (h + 0.5) / 2.0, fz), Vec3::new(0.12, (h - 0.5) / 2.0, 0.06), Vec3::X, trim, Surface::Steel, 1.0);
    }
    m.beam(Vec3::new(o.x - w / 2.0, o.y + h * 0.62, fz), Vec3::new(o.x + w / 2.0, o.y + h * 0.62, fz), 0.12, 0.2, trim, Surface::Steel, 1.0);
    // Door + windows (front), windows (side).
    let p = |x: f32, y: f32| Vec3::new(o.x + x, o.y + y, fz + 0.03);
    m.quad(p(-0.45, 0.5), p(0.45, 0.5), p(0.45, 2.3), p(-0.45, 2.3), trim, Surface::Door, 1.0 / 1.8);
    for x in [-w / 3.0, w / 3.0] {
        m.quad(p(x - 0.4, 1.3), p(x + 0.4, 1.3), p(x + 0.4, 2.2), p(x - 0.4, 2.2), Vec3::ONE, Surface::Window, 1.0 / 0.9);
    }
    let sx = o.x + w / 2.0 + 0.03;
    let s = |z: f32, y: f32| Vec3::new(sx, o.y + y, o.z + z);
    m.quad(s(0.4, 1.4), s(-0.4, 1.4), s(-0.4, 2.3), s(0.4, 2.3), Vec3::ONE, Surface::Window, 1.0 / 0.9);
    // Gable roof along x.
    let ov = 0.35;
    let ridge = o.y + h + d * 0.55;
    let (x0, x1) = (o.x - w / 2.0 - ov, o.x + w / 2.0 + ov);
    let (z0, z1) = (o.z - d / 2.0 - ov, o.z + d / 2.0 + ov);
    let eave = o.y + h - 0.2;
    m.quad(Vec3::new(x0, eave, z1), Vec3::new(x1, eave, z1), Vec3::new(x1, ridge, o.z), Vec3::new(x0, ridge, o.z), roofc, roof, 1.0);
    m.quad(Vec3::new(x1, eave, z0), Vec3::new(x0, eave, z0), Vec3::new(x0, ridge, o.z), Vec3::new(x1, ridge, o.z), roofc, roof, 1.0);
    // Gable ends.
    for (x, flip) in [(o.x - w / 2.0, true), (o.x + w / 2.0, false)] {
        let a = Vec3::new(x, o.y + h, o.z - d / 2.0);
        let b = Vec3::new(x, o.y + h, o.z + d / 2.0);
        let c = Vec3::new(x, ridge - 0.15, o.z);
        if flip { m.tri(a, b, c, wall, Surface::Earth, 1.0) } else { m.tri(b, a, c, wall, Surface::Earth, 1.0) }
    }
    // Chimney.
    m.aabb_out(Vec3::new(o.x + w / 4.0 - 0.35, o.y + h, o.z - 0.9), Vec3::new(o.x + w / 4.0 + 0.35, ridge + 0.9, o.z - 0.2), palette::stone(), Surface::Concrete, 1.0);
}

fn diorama() -> (MeshData, MeshData, Vec3) {
    let mut m = MeshData::default();
    let grass = palette::grass();
    // Ground: a grid of quads so the low-res wobble shows on the terrain.
    let n: i32 = 14;
    let size = 4.0;
    for i in -n..n {
        for j in -n..n {
            let (x, z) = (i as f32 * size, j as f32 * size);
            let in_pond = (x + 8.0).abs() < 7.0 && (z - 6.0).abs() < 5.0;
            if in_pond {
                continue;
            }
            let c = if (i * 7 + j * 3).rem_euclid(9) == 0 { palette::grass_dry() } else { grass };
            m.quad(Vec3::new(x, 0.0, z + size), Vec3::new(x + size, 0.0, z + size), Vec3::new(x + size, 0.0, z), Vec3::new(x, 0.0, z), c, Surface::Grass, 0.5);
        }
    }
    // Pond bed (sand) under the water.
    m.quad(Vec3::new(-16.0, -0.4, 12.0), Vec3::new(0.0, -0.4, 12.0), Vec3::new(0.0, -0.4, 0.0), Vec3::new(-16.0, -0.4, 0.0), palette::sand(), Surface::Gravel, 1.0);
    // Cobble path and flagstones.
    for k in 0..8 {
        let z = -2.0 + k as f32 * 1.6;
        m.quad(Vec3::new(4.2, 0.02, z + 1.6), Vec3::new(5.8, 0.02, z + 1.6), Vec3::new(5.8, 0.02, z), Vec3::new(4.2, 0.02, z), palette::rock(), Surface::Pavers, 0.6);
    }
    m.quad(Vec3::new(5.8, 0.02, 1.0), Vec3::new(12.0, 0.02, 1.0), Vec3::new(12.0, 0.02, -0.6), Vec3::new(5.8, 0.02, -0.6), palette::dirt(), Surface::Dirt, 0.5);
    m.quad(Vec3::new(-2.0, 0.02, 12.0), Vec3::new(4.2, 0.02, 12.0), Vec3::new(4.2, 0.02, 9.6), Vec3::new(-2.0, 0.02, 9.6), palette::stone(), Surface::Slabs, 0.5);

    house(&mut m, Vec3::new(0.0, 0.0, 0.0), 6.0, 4.5, 3.2, Colors { roof: 0, wall: 0, trim: 0 }, Surface::Sedum);
    house(&mut m, Vec3::new(9.0, 0.0, -6.0), 4.5, 4.0, 2.6, Colors { roof: 1, wall: 2, trim: 2 }, Surface::Seam);
    house(&mut m, Vec3::new(-6.0, 0.0, -8.0), 5.0, 4.0, 2.4, Colors { roof: 4, wall: 3, trim: 1 }, Surface::Precast);

    // Stone wall with battlements and a tower.
    let stone = palette::stone();
    m.beam(Vec3::new(-14.0, 0.0, -14.0), Vec3::new(14.0, 0.0, -14.0), 1.0, 3.0, stone, Surface::Concrete, 0.7);
    let mut x = -13.6;
    while x < 14.0 {
        m.aabb_out(Vec3::new(x, 3.0, -14.5), Vec3::new(x + 0.8, 3.6, -13.5), stone, Surface::Concrete, 0.7);
        x += 1.6;
    }
    let t = Vec3::new(14.0, 0.0, -14.0);
    m.cylinder(t, 2.2, 2.0, 7.0, 10, true, false, stone, Surface::Concrete, 0.7);
    m.cylinder(t + Vec3::Y * 7.0, 2.6, 0.0, 4.0, 10, false, true, palette::roof(Colors { roof: 1, wall: 0, trim: 0 }), Surface::Seam, 1.0);
    m.quad(t + Vec3::new(-0.3, 4.0, 2.05), t + Vec3::new(0.3, 4.0, 2.05), t + Vec3::new(0.3, 5.2, 2.05), t + Vec3::new(-0.3, 5.2, 2.05), Vec3::ONE, Surface::Window, 1.0);

    // Wooden fence.
    let wood = palette::wood();
    for k in 0..7 {
        let x = -16.0 + k as f32 * 1.5;
        m.obox_out(Vec3::new(x, 0.5, 14.0), Vec3::new(0.08, 0.5, 0.08), Vec3::X, wood, Surface::Steel, 1.0);
    }
    m.beam(Vec3::new(-16.0, 0.65, 14.0), Vec3::new(-7.0, 0.65, 14.0), 0.08, 0.12, wood, Surface::Cladding, 1.0);
    m.beam(Vec3::new(-16.0, 0.3, 14.0), Vec3::new(-7.0, 0.3, 14.0), 0.08, 0.12, wood, Surface::Cladding, 1.0);

    // Trees.
    for (p, pine) in [(Vec3::new(-12.0, 0.0, -3.0), false), (Vec3::new(12.0, 0.0, 8.0), true), (Vec3::new(-3.0, 0.0, -11.0), false), (Vec3::new(16.0, 0.0, 2.0), true)] {
        m.cylinder(p, 0.3, 0.22, 2.2, 6, false, false, palette::bark(), Surface::Bark, 1.0);
        if pine {
            for (k, r) in [2.0f32, 1.6, 1.1].iter().enumerate() {
                m.cylinder(p + Vec3::Y * (1.6 + k as f32 * 1.3), *r, 0.0, 2.2, 7, false, true, palette::pine(), Surface::Leaves, 1.0);
            }
        } else {
            m.cylinder(p + Vec3::Y * 1.8, 1.4, 1.9, 1.4, 7, false, true, palette::leaves(), Surface::Leaves, 1.0);
            m.cylinder(p + Vec3::Y * 3.2, 1.9, 0.9, 1.6, 7, true, false, palette::leaves(), Surface::Leaves, 1.0);
        }
    }
    // Flowers.
    for k in 0..24 {
        let a = k as f32 * 2.399;
        let r = 3.0 + (k as f32 * 0.37).fract() * 4.0;
        let p = Vec3::new(-8.0 + a.cos() * r * 1.4, 0.0, -2.0 + a.sin() * r * 0.5);
        m.aabb_out(p, p + Vec3::new(0.25, 0.3, 0.25), palette::flower(k), Surface::Petal, 1.0);
    }
    // Haystack, crate, lantern post.
    m.cylinder(Vec3::new(6.5, 0.0, 6.0), 1.0, 0.2, 1.8, 8, false, false, palette::hay(), Surface::Precast, 1.0);
    m.aabb_out(Vec3::new(3.5, 0.0, 3.0), Vec3::new(4.2, 0.7, 3.7), wood, Surface::Cladding, 1.0);
    let lamp = Vec3::new(3.8, 0.0, 6.0);
    m.obox_out(lamp + Vec3::Y * 1.1, Vec3::new(0.07, 1.1, 0.07), Vec3::X, palette::metal(), Surface::Metal, 1.0);
    m.aabb_out(lamp + Vec3::new(-0.18, 2.2, -0.18), lamp + Vec3::new(0.18, 2.6, 0.18), palette::glow(), Surface::Glow, 1.0);
    m.aabb_out(lamp + Vec3::new(-0.22, 2.6, -0.22), lamp + Vec3::new(0.22, 2.7, 0.22), palette::metal(), Surface::Metal, 1.0);

    let mut water = MeshData::default();
    water.quad(Vec3::new(-15.0, -0.15, 11.0), Vec3::new(-1.0, -0.15, 11.0), Vec3::new(-1.0, -0.15, 1.0), Vec3::new(-15.0, -0.15, 1.0), Vec3::ONE, Surface::Water, 1.0);
    // Pond banks (sand) from ground level down to the bed.
    let s = palette::sand();
    m.quad(Vec3::new(-15.0, 0.0, 1.0), Vec3::new(-1.0, 0.0, 1.0), Vec3::new(-1.0, -0.4, 1.0), Vec3::new(-15.0, -0.4, 1.0), s, Surface::Gravel, 1.0);
    m.quad(Vec3::new(-1.0, 0.0, 11.0), Vec3::new(-15.0, 0.0, 11.0), Vec3::new(-15.0, -0.4, 11.0), Vec3::new(-1.0, -0.4, 11.0), s, Surface::Gravel, 1.0);
    m.quad(Vec3::new(-1.0, 0.0, 1.0), Vec3::new(-1.0, 0.0, 11.0), Vec3::new(-1.0, -0.4, 11.0), Vec3::new(-1.0, -0.4, 1.0), s, Surface::Gravel, 1.0);
    m.quad(Vec3::new(-15.0, 0.0, 11.0), Vec3::new(-15.0, 0.0, 1.0), Vec3::new(-15.0, -0.4, 1.0), Vec3::new(-15.0, -0.4, 11.0), s, Surface::Gravel, 1.0);
    (m, water, lamp + Vec3::Y * 2.4)
}

fn spawn_test_scene(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    assets: Res<GladeAssets>,
    test: Res<RenderTest>,
    mut env: ResMut<Environment>,
    mut cam: Query<&mut Projection, With<super::GameCamera>>,
) {
    for mut p in &mut cam {
        if let Projection::Orthographic(o) = &mut *p {
            o.scale = test.zoom;
        }
    }
    match preset(&test.env) {
        Some(e) => *env = e,
        None => warn!("unknown --test-env {:?}; using noon", test.env),
    }
    let (ground, water, lamp) = diorama();
    commands.spawn((Mesh3d(meshes.add(to_bevy_mesh(&ground))), MeshMaterial3d(assets.opaque.clone())));
    commands.spawn((Mesh3d(meshes.add(to_bevy_mesh(&water))), MeshMaterial3d(assets.water.clone())));
    let mut light = lantern_light(1000.0, 6.0);
    light.intensity *= env.night;
    commands.spawn((light, Transform::from_translation(lamp)));

    let mut kinds = vec![ParticleKind::Puff, ParticleKind::Bird];
    if env.wetness > 0.0 {
        kinds.push(ParticleKind::Streak);
    }
    if env.snow > 0.0 {
        kinds.push(ParticleKind::Flake);
    }
    if env.night > 0.5 {
        kinds.push(ParticleKind::Spark);
    }
    for kind in kinds {
        commands.spawn((
            TestParticles { kind },
            Mesh3d(meshes.add(particle_mesh(&[]))),
            MeshMaterial3d(assets.particle.clone()),
            NoFrustumCulling,
        ));
    }
}

fn hash(i: u32, s: u32) -> f32 {
    let h = i.wrapping_mul(0x9E37_79B9) ^ s.wrapping_mul(0x85EB_CA6B);
    let h = (h ^ (h >> 15)).wrapping_mul(0x2C1B_3C6D);
    (h ^ (h >> 13)) as f32 / u32::MAX as f32
}

/// Example particle systems: one mesh per system, rewritten every frame.
fn animate_particles(
    time: Res<Time>,
    env: Res<Environment>,
    systems: Query<(&TestParticles, &Mesh3d)>,
    mut meshes: ResMut<Assets<Mesh>>,
) {
    let t = time.elapsed_secs();
    let wind = env.wind.extend(0.0).xzy();
    for (sys, handle) in &systems {
        let mut ps = Vec::new();
        match sys.kind {
            ParticleKind::Puff => {
                let chimney = Vec3::new(1.5, 6.6, -0.55);
                for i in 0..12 {
                    let age = (t * 0.25 + i as f32 / 12.0).fract();
                    let pos = chimney + Vec3::Y * age * 4.0 + wind * age * 0.6 + Vec3::new((age * 9.0 + i as f32).sin() * 0.2, 0.0, 0.0);
                    let c = Vec3::splat(0.85).lerp(Vec3::splat(0.6), age);
                    ps.push(Particle::new(ParticleKind::Puff, pos, 0.6 + age * 1.2, c.extend(1.2 - age)));
                }
            }
            ParticleKind::Bird => {
                for i in 0..4 {
                    let a = t * 0.4 + i as f32 * 1.4;
                    let pos = Vec3::new(14.0 + a.cos() * 6.0, 13.0 + i as f32 * 0.6, -14.0 + a.sin() * 6.0);
                    ps.push(Particle::new(ParticleKind::Bird, pos, 0.9, Vec4::new(0.12, 0.12, 0.16, 1.0)));
                }
            }
            ParticleKind::Streak => {
                let dir = (Vec3::new(0.0, -9.0, 0.0) + wind).normalize();
                for i in 0..450 {
                    let y = (hash(i, 3) * 20.0 - t * 14.0).rem_euclid(20.0);
                    let pos = Vec3::new(hash(i, 1) * 50.0 - 25.0, y, hash(i, 2) * 50.0 - 25.0);
                    let mut p = Particle::new(ParticleKind::Streak, pos, 0.9, Vec4::new(0.75, 0.8, 0.9, 1.0));
                    p.dir = dir;
                    ps.push(p);
                }
            }
            ParticleKind::Flake => {
                for i in 0..700 {
                    let y = (hash(i, 3) * 20.0 - t * 1.5).rem_euclid(20.0);
                    let sway = (t + i as f32).sin() * 0.3;
                    let pos = Vec3::new(hash(i, 1) * 50.0 - 25.0 + sway, y, hash(i, 2) * 50.0 - 25.0);
                    ps.push(Particle::new(ParticleKind::Flake, pos, 0.14, Vec4::new(1.0, 1.0, 1.0, 1.0)));
                }
            }
            ParticleKind::Spark => {
                for i in 0..30 {
                    let base = Vec3::new(hash(i, 1) * 20.0 - 14.0, 0.6 + hash(i, 2) * 1.2, hash(i, 4) * 14.0 - 4.0);
                    let wob = Vec3::new((t * 0.7 + i as f32).sin(), (t * 1.3 + i as f32).sin() * 0.3, (t * 0.5 + i as f32 * 2.0).cos());
                    let blink = 0.5 + 0.5 * (t * 2.0 + i as f32 * 3.0).sin();
                    ps.push(Particle::new(ParticleKind::Spark, base + wob, 0.22, Vec4::new(0.9, 1.0, 0.4, blink)));
                }
            }
        }
        if let Some(mut mesh) = meshes.get_mut(&handle.0) {
            super::write_particle_mesh(&mut mesh, &ps);
        }
    }
}

fn capture(mut commands: Commands, mut test: ResMut<RenderTest>, mut exit: MessageWriter<AppExit>) {
    test.frames += 1;
    if test.frames >= SETTLE_FRAMES + TIMEOUT_FRAMES {
        error!("render test: screenshot never arrived");
        exit.write(AppExit::error());
        return;
    }
    if test.frames >= SETTLE_FRAMES && !test.requested {
        test.requested = true;
        info!("render test -> {}", test.path.display());
        super::request_screenshot(&mut commands, test.path.clone()).observe(
            |_: On<ScreenshotCaptured>, mut exit: MessageWriter<AppExit>| {
                exit.write(AppExit::Success);
            },
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_args() {
        let a = |s: &str| s.split_whitespace().map(String::from).collect::<Vec<_>>();
        let t = RenderTest::from_args(a("bin --render-test x.png --test-env dusk")).unwrap();
        assert_eq!(t.path, PathBuf::from("x.png"));
        assert_eq!(t.env, "dusk");
        assert!(RenderTest::from_args(a("bin")).is_none());
        for p in ["noon", "dusk", "night", "rain", "storm", "snow", "fog"] {
            assert!(preset(p).is_some());
        }
    }

    #[test]
    fn diorama_is_valid() {
        let (m, w, _) = diorama();
        assert!(m.is_valid() && w.is_valid());
    }
}
