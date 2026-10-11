//! Renders a hand-built voxel scene with the PS1 material, screenshots it, exits.
//! Usage: `cargo run --example ps1_probe -- [out.png] [frames] [near]`

use bevy::{
    platform::collections::HashMap, prelude::*, render::view::screenshot::ScreenshotCaptured,
};
use concrete_glade::{
    mesher::ChunkMesh,
    render::{
        GameCamera, Ps1Plugin, VoxelMaterialHandle, atlas::tile_uv_rect, chunk_mesh_to_bevy,
        request_screenshot,
    },
    voxel::{VOXEL_SIZE, Voxel},
};

#[derive(Resource)]
struct Probe {
    path: String,
    frames: u32,
    near: bool,
}

fn main() {
    let mut args = std::env::args().skip(1);
    let path = args.next().unwrap_or_else(|| "probe.png".into());
    let frames = args.next().and_then(|s| s.parse().ok()).unwrap_or(30);
    let near = args.next().is_some_and(|s| s == "near");
    App::new()
        .add_plugins((DefaultPlugins, Ps1Plugin))
        .insert_resource(Probe { path, frames, near })
        .add_systems(Startup, scene)
        .add_systems(Update, shoot)
        .run();
}

const FACES: [([i32; 3], f32); 6] = [
    ([1, 0, 0], 0.80),
    ([-1, 0, 0], 0.70),
    ([0, 1, 0], 1.00),
    ([0, -1, 0], 0.50),
    ([0, 0, 1], 0.85),
    ([0, 0, -1], 0.65),
];

/// Minimal culled-face mesher over a sparse voxel map (voxel coords, y = 0 is ground top).
fn mesh(voxels: &HashMap<IVec3, Voxel>) -> ChunkMesh {
    let mut m = ChunkMesh::default();
    for (&p, &v) in voxels {
        for (n, shade) in FACES {
            let n = IVec3::from(n);
            if voxels.contains_key(&(p + n)) {
                continue;
            }
            // Two tangent axes for the face, ordered for counter-clockwise winding.
            let (a, b) = match n.to_array() {
                [1, 0, 0] => (IVec3::Y, IVec3::Z),
                [-1, 0, 0] => (IVec3::Z, IVec3::Y),
                [0, 1, 0] => (IVec3::Z, IVec3::X),
                [0, -1, 0] => (IVec3::X, IVec3::Z),
                [0, 0, 1] => (IVec3::X, IVec3::Y),
                _ => (IVec3::Y, IVec3::X),
            };
            let origin = p + n.max(IVec3::ZERO);
            let base = m.positions.len() as u32;
            let (uv0, uv1) = tile_uv_rect(v);
            let emissive = if matches!(v, Voxel::WindowLit | Voxel::Lamp) {
                1.0
            } else {
                0.0
            };
            for (da, db) in [(0, 0), (1, 0), (1, 1), (0, 1)] {
                let q = (origin + a * da + b * db).as_vec3() * VOXEL_SIZE;
                m.positions.push(q.to_array());
                m.normals.push(n.as_vec3().to_array());
                m.colors.push([shade, shade, shade, emissive]);
                // u along the horizontal axis, v down the vertical one.
                let (u, w) = if n.y != 0 {
                    (da, db)
                } else if a == IVec3::Y {
                    (db, 1 - da)
                } else {
                    (da, 1 - db)
                };
                m.uvs.push([
                    if u == 0 { uv0[0] } else { uv1[0] },
                    if w == 0 { uv0[1] } else { uv1[1] },
                ]);
            }
            m.indices
                .extend([base, base + 1, base + 2, base, base + 2, base + 3]);
        }
    }
    m
}

fn scene(
    mut commands: Commands,
    probe: Res<Probe>,
    mut camera: Single<&mut Transform, With<GameCamera>>,
    mut meshes: ResMut<Assets<Mesh>>,
    material: Res<VoxelMaterialHandle>,
) {
    if probe.near {
        **camera =
            Transform::from_xyz(9.0, 6.0, 8.0).looking_at(Vec3::new(0.0, 4.0, -6.0), Vec3::Y);
    }
    let mut v = HashMap::new();
    let mut fill = |min: IVec3, max: IVec3, f: &dyn Fn(IVec3) -> Voxel| {
        for x in min.x..max.x {
            for y in min.y..max.y {
                for z in min.z..max.z {
                    let p = IVec3::new(x, y, z);
                    v.insert(p, f(p));
                }
            }
        }
    };
    // Ground slab with a scorched crater and a road.
    fill(IVec3::new(-120, -1, -120), IVec3::new(120, 0, 120), &|p| {
        if (p.x - 14).pow(2) + (p.z + 10).pow(2) < 64 {
            Voxel::Scorch
        } else if (-4..2).contains(&p.z) {
            Voxel::Asphalt
        } else {
            Voxel::Ash
        }
    });
    // Board-formed tower with a dark base band and window grid, some lit.
    fill(IVec3::new(-8, 0, -24), IVec3::new(8, 40, -12), &|p| {
        if p.y < 3 {
            Voxel::ConcreteDark
        } else if p.y % 4 == 2 && p.x % 3 != 0 && p.y > 4 {
            if (p.x * 7 + p.y * 3) % 5 == 0 {
                Voxel::WindowLit
            } else {
                Voxel::WindowDark
            }
        } else {
            Voxel::Concrete
        }
    });
    // Low slab, rubble, rust tank, dead tree, lamp post.
    fill(IVec3::new(10, 0, 8), IVec3::new(30, 6, 20), &|p| {
        if p.y == 5 {
            Voxel::ConcreteDark
        } else {
            Voxel::Concrete
        }
    });
    fill(IVec3::new(-20, 0, 10), IVec3::new(-14, 2, 16), &|_| {
        Voxel::Rubble
    });
    fill(IVec3::new(-30, 0, -6), IVec3::new(-24, 8, 0), &|_| {
        Voxel::Rust
    });
    fill(IVec3::new(4, 0, 14), IVec3::new(5, 9, 15), &|_| {
        Voxel::DeadWood
    });
    fill(IVec3::new(0, 0, 4), IVec3::new(1, 10, 5), &|_| Voxel::Rust);
    fill(IVec3::new(0, 10, 4), IVec3::new(1, 11, 5), &|_| Voxel::Lamp);
    // A lit window cube on its own, and a far tower deep in the fog.
    fill(IVec3::new(-4, 0, 20), IVec3::new(0, 4, 24), &|_| {
        Voxel::WindowLit
    });
    fill(IVec3::new(-110, 0, -110), IVec3::new(-90, 60, -90), &|p| {
        if p.y % 5 == 3 && (p.x + p.z) % 4 == 0 {
            Voxel::WindowLit
        } else {
            Voxel::Concrete
        }
    });

    commands.spawn((
        Mesh3d(meshes.add(chunk_mesh_to_bevy(&mesh(&v)))),
        MeshMaterial3d(material.0.clone()),
    ));
    // Plain UI node without a camera target: must land on the window at full res.
    commands.spawn((
        Text::new("CONCRETE GLADE  ps1_probe"),
        TextFont {
            font_size: FontSize::Px(16.0),
            ..default()
        },
        TextColor(Color::srgb_u8(0xd9, 0xd3, 0xc7)),
        Node {
            position_type: PositionType::Absolute,
            top: px(12),
            left: px(12),
            ..default()
        },
    ));
}

fn shoot(mut commands: Commands, probe: Res<Probe>, mut frame: Local<u32>) {
    *frame += 1;
    if *frame == probe.frames {
        request_screenshot(&mut commands, probe.path.clone()).observe(
            |_: On<ScreenshotCaptured>, mut exit: MessageWriter<AppExit>| {
                exit.write(AppExit::Success);
            },
        );
    }
}
