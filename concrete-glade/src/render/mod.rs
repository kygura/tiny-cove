//! PS1-style rendering: low-res target, vertex snapping, affine UVs, dithering, fog.
//!
//! Cameras:
//! - [`GameCamera`]: the `Camera3d` (order -1) rendering the world into the low-res
//!   [`Ps1Target`] image. Move it through its `Transform`.
//! - [`WindowCamera`]: a `Camera2d` (order 0) on the primary window marked
//!   `IsDefaultUiCamera`. All bevy_ui root nodes without `UiTargetCamera` render here at
//!   full window resolution, on top of the upscaled image (which is itself a
//!   fullscreen `ImageNode` at `GlobalZIndex(i32::MIN)`, ignored by picking).

pub mod atlas;
mod material;

pub use material::{FOG_COLOR, VoxelMaterial, VoxelMaterialHandle, VoxelParams};

use crate::mesher::ChunkMesh;
use crate::voxel::Voxel;
use bevy::{
    asset::RenderAssetUsages,
    camera::RenderTarget,
    image::ImageSampler,
    mesh::{Indices, PrimitiveTopology},
    picking::Pickable,
    prelude::*,
    render::{
        render_resource::{Extent3d, TextureFormat},
        view::{
            DebandDither, Tonemapping,
            screenshot::{Screenshot, save_to_disk},
        },
    },
    window::PrimaryWindow,
};
use std::path::PathBuf;

/// Internal render height in pixels.
pub const LOW_RES_HEIGHT: u32 = 240;
/// Vertex snap grid as a fraction of the low-res size (lower = more wobble).
pub const SNAP_SCALE: f32 = 0.5;

pub struct Ps1Plugin;

/// Size of the low-res render target. Tracks the window aspect.
#[derive(Resource, Clone, Copy, Debug, PartialEq, Eq)]
pub struct LowRes {
    pub size: UVec2,
}

/// The image the [`GameCamera`] renders into.
#[derive(Resource, Clone)]
pub struct Ps1Target(pub Handle<Image>);

/// The 3D world camera.
#[derive(Component)]
pub struct GameCamera;

/// The window camera that shows the upscaled image and hosts the UI.
#[derive(Component)]
pub struct WindowCamera;

/// Runs in `PreStartup`; after it, [`LowRes`], [`Ps1Target`], [`VoxelMaterialHandle`]
/// and both cameras exist, so every `Startup` system can use them.
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct Ps1Setup;

impl Plugin for Ps1Plugin {
    fn build(&self, app: &mut App) {
        bevy::asset::embedded_asset!(app, "voxel.wesl");
        app.add_plugins(MaterialPlugin::<VoxelMaterial>::default())
            .add_systems(PreStartup, setup.in_set(Ps1Setup))
            .add_systems(Update, (fit_to_window, screenshot_on_f12, slow_sun));
    }
}

fn low_res_for(window: Option<&Window>) -> UVec2 {
    let aspect = window.map_or(16.0 / 9.0, |w| w.width() / w.height().max(1.0));
    let aspect = if aspect.is_finite() && aspect > 0.0 {
        aspect
    } else {
        16.0 / 9.0
    };
    UVec2::new(
        ((LOW_RES_HEIGHT as f32 * aspect).round() as u32).max(1),
        LOW_RES_HEIGHT,
    )
}

fn setup(
    mut commands: Commands,
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<VoxelMaterial>>,
    window: Query<&Window, With<PrimaryWindow>>,
) {
    let size = low_res_for(window.single().ok());
    let mut target = Image::new_target_texture(size.x, size.y, TextureFormat::Rgba8UnormSrgb, None);
    target.sampler = ImageSampler::nearest();
    let target = images.add(target);

    let atlas = images.add(atlas::atlas_image());
    let material = materials.add(VoxelMaterial {
        params: VoxelParams {
            snap_res: size.as_vec2() * SNAP_SCALE,
            ..default()
        },
        atlas,
    });

    commands.insert_resource(LowRes { size });
    commands.insert_resource(Ps1Target(target.clone()));
    commands.insert_resource(VoxelMaterialHandle(material));

    commands.spawn((
        GameCamera,
        Camera3d::default(),
        Camera {
            order: -1,
            clear_color: ClearColorConfig::Custom(FOG_COLOR),
            ..default()
        },
        RenderTarget::Image(target.clone().into()),
        Msaa::Off,
        Tonemapping::None,
        DebandDither::Disabled,
        Transform::from_xyz(30.0, 25.0, 30.0).looking_at(Vec3::ZERO, Vec3::Y),
    ));
    commands.spawn((
        WindowCamera,
        Camera2d,
        Camera {
            order: 0,
            clear_color: ClearColorConfig::Custom(Color::BLACK),
            ..default()
        },
        Msaa::Off,
        Tonemapping::None,
        DebandDither::Disabled,
        IsDefaultUiCamera,
    ));
    commands.spawn((
        ImageNode::new(target),
        Node {
            position_type: PositionType::Absolute,
            width: percent(100),
            height: percent(100),
            ..default()
        },
        GlobalZIndex(i32::MIN),
        Pickable::IGNORE,
    ));
}

fn fit_to_window(
    window: Query<&Window, (With<PrimaryWindow>, Changed<Window>)>,
    mut low: ResMut<LowRes>,
    target: Res<Ps1Target>,
    material: Res<VoxelMaterialHandle>,
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<VoxelMaterial>>,
) {
    let Ok(window) = window.single() else { return };
    let size = low_res_for(Some(window));
    if size == low.size {
        return;
    }
    low.size = size;
    if let Some(mut image) = images.get_mut(&target.0) {
        image.resize(Extent3d {
            width: size.x,
            height: size.y,
            depth_or_array_layers: 1,
        });
    }
    if let Some(mut m) = materials.get_mut(&material.0) {
        m.params.snap_res = size.as_vec2() * SNAP_SCALE;
    }
}

/// The sun circles the sky a few degrees a minute; the material (and so every chunk's
/// bind group) is only rewritten twice a second.
const SUN_DEG_PER_SEC: f32 = 4.0 / 60.0;

fn slow_sun(
    time: Res<Time>,
    material: Res<VoxelMaterialHandle>,
    mut materials: ResMut<Assets<VoxelMaterial>>,
    mut last: Local<f32>,
) {
    let now = time.elapsed_secs();
    if now - *last < 0.5 {
        return;
    }
    *last = now;
    if let Some(mut m) = materials.get_mut(&material.0) {
        let turn = Quat::from_rotation_y((now * SUN_DEG_PER_SEC).to_radians());
        m.params.sun_dir = turn * VoxelParams::default().sun_dir;
    }
}

/// Converts a [`ChunkMesh`] to a Bevy triangle-list mesh with POSITION, NORMAL,
/// COLOR (Float32x4) and UV_0. An empty `ChunkMesh` gives a valid mesh with no
/// vertices; callers should skip spawning (or despawn) such chunks instead.
pub fn chunk_mesh_to_bevy(m: &ChunkMesh) -> Mesh {
    Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::default(),
    )
    .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, m.positions.clone())
    .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, m.normals.clone())
    .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, m.colors.clone())
    .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, m.uvs.clone())
    .with_inserted_indices(Indices::U32(m.indices.clone()))
}

/// Unit cube centred on the origin, textured with `v`'s atlas tile.
pub fn cube_mesh(v: Voxel) -> Mesh {
    // (normal, shade, corners CCW seen from outside)
    const FACES: [([f32; 3], f32, [[f32; 3]; 4]); 6] = [
        ([0., 1., 0.], 1.0, [[0., 1., 0.], [0., 1., 1.], [1., 1., 1.], [1., 1., 0.]]),
        ([0., -1., 0.], 0.5, [[0., 0., 0.], [1., 0., 0.], [1., 0., 1.], [0., 0., 1.]]),
        ([1., 0., 0.], 0.8, [[1., 0., 0.], [1., 1., 0.], [1., 1., 1.], [1., 0., 1.]]),
        ([-1., 0., 0.], 0.8, [[0., 0., 0.], [0., 0., 1.], [0., 1., 1.], [0., 1., 0.]]),
        ([0., 0., 1.], 0.68, [[0., 0., 1.], [1., 0., 1.], [1., 1., 1.], [0., 1., 1.]]),
        ([0., 0., -1.], 0.68, [[0., 0., 0.], [0., 1., 0.], [1., 1., 0.], [1., 0., 0.]]),
    ];
    let (uv0, uv1) = atlas::tile_uv_rect(v);
    let uvs = [uv0, [uv1[0], uv0[1]], uv1, [uv0[0], uv1[1]]];
    let mut m = ChunkMesh::default();
    for (n, shade, corners) in FACES {
        let base = m.positions.len() as u32;
        for (c, uv) in corners.iter().zip(uvs) {
            m.positions.push([c[0] - 0.5, c[1] - 0.5, c[2] - 0.5]);
            m.normals.push(n);
            m.colors.push([shade, shade, shade, 0.0]);
            m.uvs.push(uv);
        }
        m.indices.extend([base, base + 1, base + 2, base, base + 2, base + 3]);
    }
    chunk_mesh_to_bevy(&m)
}

/// Maps a window cursor position (logical px, top-left origin) to low-res target pixels.
pub fn window_to_low_res(cursor: Vec2, window_size: Vec2, low: UVec2) -> Vec2 {
    cursor * low.as_vec2() / window_size.max(Vec2::ONE)
}

/// World-space ray under the mouse cursor for the [`GameCamera`].
pub fn cursor_ray(
    window: &Window,
    cam: &Camera,
    cam_gt: &GlobalTransform,
    low: &LowRes,
) -> Option<Ray3d> {
    let p = window_to_low_res(window.cursor_position()?, window.size(), low.size);
    cam.viewport_to_world(cam_gt, p).ok()
}

/// Saves the primary window (upscaled image plus UI) to `path` as soon as it is
/// rendered, creating parent directories. Attach more observers to the returned
/// entity (e.g. `On<ScreenshotCaptured>`) to act after the file is written.
pub fn request_screenshot<'a>(
    commands: &'a mut Commands,
    path: impl Into<PathBuf>,
) -> EntityCommands<'a> {
    let path = path.into();
    if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
        let _ = std::fs::create_dir_all(dir);
    }
    let mut e = commands.spawn(Screenshot::primary_window());
    e.observe(save_to_disk(path));
    e
}

fn screenshot_on_f12(mut commands: Commands, keys: Res<ButtonInput<KeyCode>>) {
    if keys.just_pressed(KeyCode::F12) {
        let ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_secs());
        request_screenshot(&mut commands, format!("screenshots/{ts}.png"));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cursor_scales_to_low_res() {
        let low = UVec2::new(427, 240);
        let win = Vec2::new(1280.0, 720.0);
        assert_eq!(window_to_low_res(Vec2::ZERO, win, low), Vec2::ZERO);
        assert_eq!(window_to_low_res(win, win, low), low.as_vec2());
        let mid = window_to_low_res(win / 2.0, win, low);
        assert!((mid - low.as_vec2() / 2.0).length() < 1e-4);
    }

    #[test]
    fn low_res_tracks_aspect() {
        assert_eq!(low_res_for(None), UVec2::new(427, 240));
    }
}
