//! PS1 + cel-shaded renderer.
//!
//! - Low-res target ([`LOW_RES_HEIGHT`] px tall, width from the window aspect) rendered
//!   by the [`GameCamera`] (orthographic `Camera3d`, order -1) and shown nearest-upscaled
//!   as a fullscreen `ImageNode` on the [`WindowCamera`] (`Camera2d`, `IsDefaultUiCamera`),
//!   so bevy_ui draws at full resolution on top.
//! - Materials ([`GladeAssets`]): [`GladeMaterial`] (everything opaque, from
//!   [`to_bevy_mesh`]), [`WaterMaterial`], [`ParticleMaterial`] (see [`particles`]).
//!   All read one shared globals buffer filled from [`Environment`] every frame.
//! - Post pass ([`post`]): sky gradient, outlines, shore foam, 15-bit Bayer dither.
//! - Sun = shadow-casting `DirectionalLight` ([`Sun`]) driven by `Environment`.
//! - Lanterns: spawn a `PointLight` from [`lantern_light`]; the cel shader reads Bevy's
//!   clustered point lights (2 hard bands).
//!
//! Spawning a mesh:
//! ```ignore
//! commands.spawn((Mesh3d(meshes.add(to_bevy_mesh(&data))), MeshMaterial3d(assets.opaque.clone())));
//! ```

pub mod atlas;
mod material;
pub mod particles;
pub mod post;
pub mod test_scene;

pub use material::{GLOBALS, GladeMaterial, ParticleMaterial, VertexSnap, WaterMaterial, WaterParams};
pub use particles::{Particle, ParticleKind, particle_mesh, particle_quad_mesh, write_particle_mesh};
pub use post::PostSettings;
pub use test_scene::RenderTest;

use crate::env::Environment;
use bevy::{
    asset::RenderAssetUsages,
    camera::{RenderTarget, ScalingMode},
    core_pipeline::prepass::{DepthPrepass, NormalPrepass},
    image::ImageSampler,
    light::{CascadeShadowConfigBuilder, DirectionalLightShadowMap, ShadowFilteringMethod},
    mesh::{Indices, PrimitiveTopology},
    picking::Pickable,
    prelude::*,
    render::{
        render_resource::{Extent3d, TextureFormat},
        storage::ShaderBuffer,
        view::{
            DebandDither, Tonemapping,
            screenshot::{Screenshot, ScreenshotCaptured},
        },
    },
    window::PrimaryWindow,
};
use glade_core::MeshData;
use std::path::{Path, PathBuf};

/// Internal render height in pixels.
pub const LOW_RES_HEIGHT: u32 = 240;
/// Distance from the camera to its focus point along the view direction. Keep the
/// camera at this distance when orbiting/panning (zoom with the projection `scale`):
/// the shadow cascade and fog start assume it.
pub const CAMERA_DISTANCE: f32 = 80.0;
pub const CAMERA_NEAR: f32 = 1.0;
pub const CAMERA_FAR: f32 = 250.0;
/// Default orthographic view height in metres (≈ 42 m across at 16:9).
pub const VIEW_HEIGHT: f32 = 24.0;
pub const DEFAULT_YAW: f32 = 45.0;
pub const DEFAULT_PITCH: f32 = 35.0;

pub struct RenderPlugin;

/// Size of the low-res render target. Tracks the window aspect.
#[derive(Resource, Clone, Copy, Debug, PartialEq, Eq)]
pub struct LowRes {
    pub size: UVec2,
}

/// The image the [`GameCamera`] renders into.
#[derive(Resource, Clone)]
pub struct LowResTarget(pub Handle<Image>);

/// The 3-D world camera (orthographic, renders into [`LowResTarget`]).
#[derive(Component)]
pub struct GameCamera;

/// The window camera showing the upscaled image; hosts the UI.
#[derive(Component)]
pub struct WindowCamera;

/// The shadow-casting sun / moon light.
#[derive(Component)]
pub struct Sun;

#[derive(Component)]
struct LowResDisplay;

/// Shared material handles; inserted in `PreStartup` (set [`RenderSetup`]).
#[derive(Resource, Clone)]
pub struct GladeAssets {
    pub opaque: Handle<GladeMaterial>,
    pub water: Handle<WaterMaterial>,
    pub particle: Handle<ParticleMaterial>,
    pub atlas: Handle<Image>,
}

/// Runs in `PreStartup`; afterwards [`LowRes`], [`LowResTarget`], [`GladeAssets`],
/// both cameras and the sun exist.
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct RenderSetup;

impl Plugin for RenderPlugin {
    fn build(&self, app: &mut App) {
        bevy::asset::embedded_asset!(app, "glade.wesl");
        bevy::asset::embedded_asset!(app, "glade_prepass.wesl");
        app.add_plugins((
            MaterialPlugin::<GladeMaterial>::default(),
            MaterialPlugin::<WaterMaterial>::default(),
            MaterialPlugin::<ParticleMaterial>::default(),
            post::PostPlugin,
        ))
        .insert_resource(DirectionalLightShadowMap { size: 2048 })
        .init_resource::<Environment>()
        .init_resource::<VertexSnap>()
        .add_systems(PreStartup, setup.in_set(RenderSetup))
        .add_systems(Update, screenshot_on_f12)
        .add_systems(PostUpdate, (fit_to_window, apply_environment, material::sync_globals).chain())
        .add_systems(
            PostUpdate,
            update_post_settings
                .after(bevy::transform::TransformSystems::Propagate)
                .after(bevy::camera::CameraUpdateSystems),
        );
        test_scene::build(app);
    }
}

fn low_res_for(window: Option<&Window>) -> UVec2 {
    let aspect = window.map_or(16.0 / 9.0, |w| w.width() / w.height().max(1.0));
    let aspect = if aspect.is_finite() && aspect > 0.0 { aspect } else { 16.0 / 9.0 };
    UVec2::new(((LOW_RES_HEIGHT as f32 * aspect).round() as u32).max(1), LOW_RES_HEIGHT)
}

/// Camera transform looking at `focus` from `yaw`/`pitch` (degrees) at [`CAMERA_DISTANCE`].
pub fn camera_transform(focus: Vec3, yaw_deg: f32, pitch_deg: f32) -> Transform {
    let (yaw, pitch) = (yaw_deg.to_radians(), pitch_deg.to_radians());
    let back = Vec3::new(pitch.cos() * yaw.sin(), pitch.sin(), pitch.cos() * yaw.cos());
    Transform::from_translation(focus + back * CAMERA_DISTANCE).looking_at(focus, Vec3::Y)
}

fn lin(v: Vec3) -> Color {
    Color::linear_rgb(v.x.max(0.0), v.y.max(0.0), v.z.max(0.0))
}

fn setup(
    mut commands: Commands,
    mut images: ResMut<Assets<Image>>,
    mut buffers: ResMut<Assets<ShaderBuffer>>,
    mut opaque: ResMut<Assets<GladeMaterial>>,
    mut water: ResMut<Assets<WaterMaterial>>,
    mut particle: ResMut<Assets<ParticleMaterial>>,
    env: Res<Environment>,
    snap: Res<VertexSnap>,
    window: Query<&Window, With<PrimaryWindow>>,
) {
    let size = low_res_for(window.single().ok());
    let mut target = Image::new_target_texture(size.x, size.y, TextureFormat::Rgba8UnormSrgb, None);
    target.sampler = ImageSampler::nearest();
    let target = images.add(target);

    let _ = buffers.insert(&GLOBALS, ShaderBuffer::from(material::globals_from_env(&env, 0.0, snap.0)));
    let atlas = images.add(atlas::atlas_image());
    commands.insert_resource(GladeAssets {
        opaque: opaque.add(GladeMaterial { globals: GLOBALS, atlas: atlas.clone() }),
        water: water.add(WaterMaterial { globals: GLOBALS, params: WaterParams::default() }),
        particle: particle.add(ParticleMaterial { globals: GLOBALS }),
        atlas,
    });
    commands.insert_resource(LowRes { size });
    commands.insert_resource(LowResTarget(target.clone()));

    commands.spawn((
        GameCamera,
        Camera3d::default(),
        Camera { order: -1, clear_color: ClearColorConfig::Custom(lin(env.sky_horizon)), ..default() },
        RenderTarget::Image(target.clone().into()),
        Projection::Orthographic(OrthographicProjection {
            near: CAMERA_NEAR,
            far: CAMERA_FAR,
            scaling_mode: ScalingMode::FixedVertical { viewport_height: VIEW_HEIGHT },
            ..OrthographicProjection::default_3d()
        }),
        Msaa::Off,
        Tonemapping::None,
        DebandDither::Disabled,
        (DepthPrepass, NormalPrepass, ShadowFilteringMethod::Hardware2x2),
        PostSettings::default(),
        camera_transform(Vec3::ZERO, DEFAULT_YAW, DEFAULT_PITCH),
    ));

    // One cascade covering the visible slab around the focus (70 m diorama).
    let cascades = CascadeShadowConfigBuilder {
        num_cascades: 1,
        minimum_distance: CAMERA_DISTANCE - 70.0,
        maximum_distance: CAMERA_DISTANCE + 70.0,
        first_cascade_far_bound: CAMERA_DISTANCE + 70.0,
        overlap_proportion: 0.0,
    }
    .build();
    commands.spawn((
        Sun,
        DirectionalLight {
            shadow_maps_enabled: true,
            shadow_depth_bias: 0.03,
            shadow_normal_bias: 0.8,
            ..default()
        },
        cascades,
        Transform::from_translation(env.sun_dir).looking_at(Vec3::ZERO, Vec3::Y),
    ));

    commands.spawn((
        WindowCamera,
        Camera2d,
        Camera { order: 0, clear_color: ClearColorConfig::Custom(Color::BLACK), ..default() },
        Msaa::Off,
        Tonemapping::None,
        DebandDither::Disabled,
        IsDefaultUiCamera,
    ));
    commands.spawn((
        LowResDisplay,
        ImageNode::new(target),
        Node { position_type: PositionType::Absolute, width: percent(100), height: percent(100), ..default() },
        GlobalZIndex(i32::MIN),
        Pickable::IGNORE,
    ));
}

fn fit_to_window(
    window: Query<&Window, (With<PrimaryWindow>, Changed<Window>)>,
    mut low: ResMut<LowRes>,
    target: Res<LowResTarget>,
    mut images: ResMut<Assets<Image>>,
) {
    let Ok(window) = window.single() else { return };
    if window.physical_width() == 0 || window.physical_height() == 0 {
        return;
    }
    let size = low_res_for(Some(window));
    if size == low.size {
        return;
    }
    low.size = size;
    if let Some(mut image) = images.get_mut(&target.0) {
        image.resize(Extent3d { width: size.x, height: size.y, depth_or_array_layers: 1 });
    }
}

/// Sun light + clear colour from [`Environment`].
fn apply_environment(
    env: Res<Environment>,
    mut sun: Query<(&mut DirectionalLight, &mut Transform), With<Sun>>,
    mut cam: Query<&mut Camera, With<GameCamera>>,
) {
    if !env.is_changed() {
        return;
    }
    for (mut light, mut tf) in &mut sun {
        let dir = env.sun_dir.normalize_or(Vec3::Y);
        let up = if dir.cross(Vec3::Y).length_squared() < 1e-6 { Vec3::Z } else { Vec3::Y };
        tf.rotation = Transform::from_translation(dir).looking_at(Vec3::ZERO, up).rotation;
        let peak = env.sun_color.max_element().max(1e-4);
        light.color = lin(env.sun_color / peak);
        light.illuminance = peak * light_consts::lux::AMBIENT_DAYLIGHT;
    }
    for mut camera in &mut cam {
        camera.clear_color = ClearColorConfig::Custom(lin(env.sky_horizon));
    }
}

fn update_post_settings(
    env: Res<Environment>,
    time: Res<Time>,
    mut cam: Query<(&Projection, &mut PostSettings), With<GameCamera>>,
) {
    for (projection, mut post) in &mut cam {
        let (near, far) = match projection {
            Projection::Orthographic(o) => (o.near, o.far),
            _ => (CAMERA_NEAR, CAMERA_FAR),
        };
        let mut p = *post;
        p.depth.x = far - near;
        p.depth.y = near;
        p.fog = env.fog_color.extend(env.fog_density.max(0.0));
        p.sky_top = env.sky_top.extend(material::FOG_START);
        p.sky_horizon = env.sky_horizon.extend(env.flash.clamp(0.0, 1.0));
        p.flags.z = env.night.clamp(0.0, 1.0);
        p.flags.w = time.elapsed_secs_wrapped();
        if p != *post {
            *post = p;
        }
    }
}

/// Converts glade_core mesh buffers to a Bevy mesh with POSITION, NORMAL, UV_0, COLOR
/// and UV_1 = (surface id, 0). Short attribute slices are padded instead of panicking.
pub fn to_bevy_mesh(m: &MeshData) -> Mesh {
    let n = m.positions.len();
    let normals: Vec<[f32; 3]> = (0..n).map(|i| m.normals.get(i).copied().unwrap_or([0.0, 1.0, 0.0])).collect();
    let uvs: Vec<[f32; 2]> = (0..n).map(|i| m.uvs.get(i).copied().unwrap_or([0.0; 2])).collect();
    let colors: Vec<[f32; 4]> = (0..n).map(|i| m.colors.get(i).copied().unwrap_or([1.0; 4])).collect();
    let surf: Vec<[f32; 2]> = (0..n).map(|i| [m.surfaces.get(i).copied().unwrap_or(0) as f32, 0.0]).collect();
    let max = n as u32;
    let indices: Vec<u32> = m
        .indices
        .as_chunks::<3>()
        .0
        .iter()
        .filter(|t| t.iter().all(|&i| i < max))
        .flatten()
        .copied()
        .collect();
    Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default())
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, m.positions.clone())
        .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, normals)
        .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, uvs)
        .with_inserted_attribute(Mesh::ATTRIBUTE_UV_1, surf)
        .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, colors)
        .with_inserted_indices(Indices::U32(indices))
}

/// Warm lantern light. The cel shader bands clustered point lights by distance within
/// `range` and scales colour so ~1000 lm ≈ 1× albedo; scale `intensity` by
/// `Environment::night` (0 by day) to fade lanterns.
pub fn lantern_light(intensity_lm: f32, range: f32) -> PointLight {
    PointLight {
        color: Color::srgb(1.0, 0.78, 0.45),
        intensity: intensity_lm,
        range,
        radius: 0.1,
        shadow_maps_enabled: false,
        ..default()
    }
}

/// Maps a window cursor position (logical px, top-left origin) to low-res pixels
/// (the low-res image is stretched over the whole window).
pub fn window_to_low_res(cursor: Vec2, window_size: Vec2, low: UVec2) -> Vec2 {
    cursor * low.as_vec2() / window_size.max(Vec2::ONE)
}

/// World ray under `window_cursor` (e.g. `window.cursor_position()`) for the
/// [`GameCamera`]; `low_res` = [`LowRes::size`].
pub fn cursor_ray(
    window_cursor: Vec2,
    window: &Window,
    cam: &Camera,
    cam_tf: &GlobalTransform,
    low_res: UVec2,
) -> Option<Ray3d> {
    let p = window_to_low_res(window_cursor, window.size(), low_res);
    cam.viewport_to_world(cam_tf, p).ok()
}

/// Saves `image` (PNG by extension), creating parent directories.
fn save_image(image: Image, path: &Path) -> Result<(), String> {
    if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
        std::fs::create_dir_all(dir).map_err(|e| format!("creating {}: {e}", dir.display()))?;
    }
    let img = image.try_into_dynamic().map_err(|e| format!("converting screenshot: {e}"))?;
    img.to_rgb8().save(path).map_err(|e| format!("writing {}: {e}", path.display()))
}

/// Captures the primary window (upscaled image + UI) to `path`. Attach more observers
/// (`On<ScreenshotCaptured>`) to the returned entity to act afterwards.
pub fn request_screenshot<'a>(commands: &'a mut Commands, path: PathBuf) -> EntityCommands<'a> {
    let mut e = commands.spawn(Screenshot::primary_window());
    e.observe(move |shot: On<ScreenshotCaptured>| match save_image(shot.image.clone(), &path) {
        Ok(()) => info!("screenshot saved to {}", path.display()),
        Err(err) => error!("screenshot failed: {err}"),
    });
    e
}

fn screenshot_on_f12(mut commands: Commands, keys: Res<ButtonInput<KeyCode>>) {
    if keys.just_pressed(KeyCode::F12) {
        let ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_millis());
        request_screenshot(&mut commands, PathBuf::from(format!("screenshots/{ts}.png")));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use glade_core::Surface;

    #[test]
    fn cursor_scales_to_low_res() {
        let low = UVec2::new(427, 240);
        let win = Vec2::new(1280.0, 720.0);
        assert_eq!(window_to_low_res(Vec2::ZERO, win, low), Vec2::ZERO);
        assert_eq!(window_to_low_res(win, win, low), low.as_vec2());
    }

    #[test]
    fn low_res_tracks_aspect() {
        assert_eq!(low_res_for(None), UVec2::new(427, 240));
    }

    #[test]
    fn mesh_conversion_keeps_surfaces() {
        let mut m = MeshData::default();
        m.aabb(Vec3::ZERO, Vec3::ONE, Vec3::ONE, Surface::Slate, 1.0);
        let b = to_bevy_mesh(&m);
        assert_eq!(b.count_vertices(), 24);
        let Some(bevy::mesh::VertexAttributeValues::Float32x2(s)) = b.attribute(Mesh::ATTRIBUTE_UV_1) else {
            panic!("uv1 missing")
        };
        assert_eq!(s[0][0], Surface::Slate as u8 as f32);
    }

    #[test]
    fn default_camera_looks_at_focus() {
        let t = camera_transform(Vec3::ZERO, DEFAULT_YAW, DEFAULT_PITCH);
        assert!((t.translation.length() - CAMERA_DISTANCE).abs() < 1e-3);
        assert!(t.forward().dot(-t.translation.normalize()) > 0.999);
    }
}
