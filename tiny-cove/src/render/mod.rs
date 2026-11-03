//! PS1 + cel-shaded renderer.
//!
//! - Low-res target (always [`LOW_RES_HEIGHT`] px tall, width from the window aspect)
//!   rendered by the [`GameCamera`] (orthographic `Camera3d`, order -1) and shown at an
//!   integer upscale ([`display_scale`], centred vertically: [`display_offset`]) as an
//!   `ImageNode` on the [`WindowCamera`] (`Camera2d`, `IsDefaultUiCamera`), so bevy_ui draws
//!   at full resolution on top.
//! - Materials ([`GladeAssets`]): [`GladeMaterial`] (everything opaque, from
//!   [`to_bevy_mesh`]), [`WaterMaterial`] (lagoons), [`SeaMaterial`] (the open sea, see
//!   [`sea`]), [`ParticleMaterial`] (see [`particles`]). All read one shared globals buffer
//!   filled from [`Environment`] every frame.
//! - Post pass ([`post`]): sky gradient with sun/moon discs and stars, outlines, shore foam,
//!   15-bit Bayer dither.
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
pub mod sea;
pub mod test_scene;

pub use material::{GLOBALS, GladeMaterial, ParticleMaterial, SeaField, SeaMaterial, VertexSnap, WaterMaterial, WaterParams};
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
use cairn_cove_core::MeshData;
use std::path::{Path, PathBuf};

/// Internal render height in pixels at every window size (SPEC: the 240 px target). The
/// window shows it at an integer scale, see [`display_scale`].
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
/// 30° is the 2:1 dimetric of pixel-art isometric games.
pub const DEFAULT_PITCH: f32 = 30.0;
/// Where the camera focus sits in the frame, from the bottom (`OrthographicProjection::
/// viewport_origin.y`). Below the centre, so the frame shows less near sea and more of the
/// sky above the horizon (DESIGN A8: sky across the top of the frame), which can never come
/// nearer than the far edge of the buildable square (see [`sea::Horizon`]).
pub const VIEW_ORIGIN_Y: f32 = 0.4;

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
pub struct LowResDisplay;

/// Shared material handles; inserted in `PreStartup` (set [`RenderSetup`]).
#[derive(Resource, Clone)]
pub struct GladeAssets {
    pub opaque: Handle<GladeMaterial>,
    pub water: Handle<WaterMaterial>,
    pub sea: Handle<SeaMaterial>,
    pub particle: Handle<ParticleMaterial>,
    pub atlas: Handle<Image>,
    /// Sea-bed heights (R32Float), rebuilt from the scene terrain.
    pub heightfield: Handle<Image>,
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
            MaterialPlugin::<SeaMaterial>::default(),
            MaterialPlugin::<ParticleMaterial>::default(),
            post::PostPlugin,
        ))
        .insert_resource(DirectionalLightShadowMap { size: 2048 })
        .init_resource::<Environment>()
        .init_resource::<VertexSnap>()
        .init_resource::<SeaField>()
        .init_resource::<sea::Horizon>()
        .add_systems(PreStartup, setup.in_set(RenderSetup))
        .add_systems(Startup, sea::spawn_sea.run_if(not(resource_exists::<RenderTest>)))
        .add_systems(Update, screenshot_on_f12)
        .add_systems(PostUpdate, (fit_to_window, apply_environment, sea::update_heightfield, material::sync_globals).chain())
        .add_systems(
            PostUpdate,
            update_post_settings
                .after(bevy::transform::TransformSystems::Propagate)
                .after(bevy::camera::CameraUpdateSystems),
        );
        test_scene::build(app);
    }
}

/// Integer upscale factor for a window `height` physical px tall, so every low-res pixel is
/// exactly `s × s` window pixels: the integer nearest `height / LOW_RES_HEIGHT` (at least 1).
/// The [`LOW_RES_HEIGHT`] rows then cover `240·s` px, and the few left over are letterboxed
/// (or cropped) evenly top and bottom by [`display_offset`]. On a tie (1080 p: 4× leaves 120
/// px, 5× is 120 px too tall) it letterboxes rather than crops.
pub fn display_scale(height: u32) -> u32 {
    let lo = (height / LOW_RES_HEIGHT).max(1);
    let (below, above) = (height.saturating_sub(lo * LOW_RES_HEIGHT), ((lo + 1) * LOW_RES_HEIGHT).saturating_sub(height));
    if above < below { lo + 1 } else { lo }
}

/// Low-res target size for a window of `physical` px: [`LOW_RES_HEIGHT`] rows, and as many
/// columns as cover the window width at [`display_scale`] (cropping < s px at the right).
pub fn low_res_for_physical(physical: UVec2) -> UVec2 {
    let s = display_scale(physical.y);
    UVec2::new(physical.x.div_ceil(s).max(1), LOW_RES_HEIGHT)
}

/// Where the upscaled image's top-left corner sits in the window (physical px): flush left,
/// centred vertically (positive y = letterbox bar above it, negative = rows cropped above).
pub fn display_offset(physical: UVec2) -> Vec2 {
    let s = display_scale(physical.y);
    Vec2::new(0.0, ((physical.y as f32 - (LOW_RES_HEIGHT * s) as f32) * 0.5).floor())
}

fn low_res_for(window: Option<&Window>) -> UVec2 {
    match window.map(|w| w.physical_size()).filter(|p| p.x > 0 && p.y > 0) {
        Some(p) => low_res_for_physical(p),
        None => low_res_for_physical(UVec2::new(1280, 720)),
    }
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
    mut sea_mat: ResMut<Assets<SeaMaterial>>,
    mut particle: ResMut<Assets<ParticleMaterial>>,
    env: Res<Environment>,
    snap: Res<VertexSnap>,
    field: Res<SeaField>,
    window: Query<&Window, With<PrimaryWindow>>,
) {
    let size = low_res_for(window.single().ok());
    let mut target = Image::new_target_texture(size.x, size.y, TextureFormat::Rgba8UnormSrgb, None);
    target.sampler = ImageSampler::nearest();
    let target = images.add(target);

    let _ = buffers.insert(&GLOBALS, ShaderBuffer::from(material::globals_from_env(&env, 0.0, snap.0, field.0, sea::DEFAULT_HORIZON, &[])));
    let atlas = images.add(atlas::atlas_image());
    let heightfield = images.add(sea::heightfield_image(&[], field.0.w as u32));
    commands.insert_resource(GladeAssets {
        opaque: opaque.add(GladeMaterial { globals: GLOBALS, atlas: atlas.clone() }),
        water: water.add(WaterMaterial { globals: GLOBALS, atlas: atlas.clone(), params: WaterParams::default(), heightfield: heightfield.clone() }),
        sea: sea_mat.add(SeaMaterial { globals: GLOBALS, atlas: atlas.clone(), heightfield: heightfield.clone() }),
        particle: particle.add(ParticleMaterial { globals: GLOBALS }),
        atlas,
        heightfield,
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
            viewport_origin: Vec2::new(0.5, VIEW_ORIGIN_Y),
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
        // Letterbox bars (see `display_offset`): night-ink indigo, never black.
        Camera { order: 0, clear_color: ClearColorConfig::Custom(lin(cairn_cove_core::palette::outline_night())), ..default() },
        Msaa::Off,
        Tonemapping::None,
        DebandDither::Disabled,
        IsDefaultUiCamera,
    ));
    commands.spawn((
        LowResDisplay,
        ImageNode::new(target),
        // Sized to exactly s × the low-res image by `fit_to_window`.
        Node { position_type: PositionType::Absolute, left: px(0), top: px(0), width: percent(100), height: percent(100), ..default() },
        GlobalZIndex(i32::MIN),
        Pickable::IGNORE,
    ));
}

fn fit_to_window(
    window: Query<&Window, With<PrimaryWindow>>,
    ui_scale: Option<Res<UiScale>>,
    mut low: ResMut<LowRes>,
    target: Res<LowResTarget>,
    mut images: ResMut<Assets<Image>>,
    mut display: Query<&mut Node, With<LowResDisplay>>,
) {
    let Ok(window) = window.single() else { return };
    if window.physical_width() == 0 || window.physical_height() == 0 {
        return;
    }
    let size = low_res_for(Some(window));
    if size != low.size {
        low.size = size;
        if let Some(mut image) = images.get_mut(&target.0) {
            image.resize(Extent3d { width: size.x, height: size.y, depth_or_array_layers: 1 });
        }
    }
    // Show it at exactly s×, centred vertically (UI px = physical / (dpi × UiScale)).
    let s = display_scale(window.physical_height()) as f32;
    let k = (window.scale_factor() * ui_scale.map_or(1.0, |u| u.0)).max(1e-3);
    let (w, h) = (Val::Px(size.x as f32 * s / k), Val::Px(size.y as f32 * s / k));
    let top = Val::Px(display_offset(window.physical_size()).y / k);
    for mut node in &mut display {
        if node.width != w || node.height != h || node.top != top {
            node.width = w;
            node.height = h;
            node.top = top;
        }
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
    horizon: Res<sea::Horizon>,
    time: Res<Time>,
    mut cam: Query<(&Projection, &GlobalTransform, &mut PostSettings), With<GameCamera>>,
) {
    for (projection, tf, mut post) in &mut cam {
        let (near, far, area) = match projection {
            Projection::Orthographic(o) => (o.near, o.far, o.area),
            _ => (CAMERA_NEAR, CAMERA_FAR, Rect::from_center_half_size(Vec2::ZERO, Vec2::new(VIEW_HEIGHT * 0.89, VIEW_HEIGHT * 0.5))),
        };
        let p = post_settings(&env, tf, near, far, area, horizon.distance(tf.forward().as_vec3()), time.elapsed_secs_wrapped(), *post);
        if p != *post {
            *post = p;
        }
    }
}

/// Post settings for the camera at `tf` (orthographic, `area` = the projection's view
/// rectangle in metres around the camera axis, `horizon` = [`sea::Horizon`] distance where
/// the open sea is cut for the sky).
#[allow(clippy::too_many_arguments)]
pub fn post_settings(env: &Environment, tf: &GlobalTransform, near: f32, far: f32, area: Rect, horizon: f32, time: f32, prev: PostSettings) -> PostSettings {
    let (half, centre) = (area.half_size(), area.center());
    let mut p = prev;
    let (right, up, fwd) = (tf.right().as_vec3(), tf.up().as_vec3(), tf.forward().as_vec3());
    let pos = tf.translation();
    p.depth.x = far - near;
    p.depth.y = near;
    p.fog = env.fog_color.extend(env.fog_density.max(0.0));
    p.sky_top = env.sky_top.extend(material::FOG_START);
    p.sky_horizon = env.sky_horizon.extend(env.flash.clamp(0.0, 1.0));
    p.flags.z = env.dark.clamp(0.0, 1.0);
    p.flags.w = time;
    // Sea line: where the sea is cut for the sky (`horizon` m from the island centre along
    // the ground-forward axis, as in the sea shader), in screen uv y (0 = top).
    let fwd_h = Vec3::new(fwd.x, 0.0, fwd.z).normalize_or(Vec3::NEG_Z);
    let edge = fwd_h * horizon + Vec3::Y * env.tide;
    let horizon = sea_line_uv(edge - pos, up, area);
    p.sky_mid = env.sky_mid.extend(horizon);
    p.outline = env.outline.extend(env.storm.clamp(0.0, 1.0));
    p.foam = env.sea[4].extend(env.wetness.clamp(0.0, 1.0));
    // Sun or moon disc above the sea line, placed by azimuth and altitude.
    let moon = env.dark > 0.5;
    let d = if moon { env.sun_dir } else { crate::weather::sun_raw(env.hour) };
    let alt = d.y.clamp(-1.0, 1.0).asin();
    let x = 0.5 + 0.5 * Vec3::new(d.x, 0.0, d.z).normalize_or_zero().dot(right) * 0.8;
    // Altitude maps into the visible sky band: 0° on the sea line, 90° near the frame top.
    let y = horizon - (alt / std::f32::consts::FRAC_PI_2) * horizon.max(0.0) * 0.85;
    // No sun disc behind storm clouds (DESIGN A8: the beacon is the only warm point).
    let kind = if moon { 2.0 } else if alt > -0.05 && env.storm < 0.5 { 1.0 } else { 0.0 };
    p.disc = env.disc_color.extend(kind);
    p.disc_pos = Vec4::new(x, y, if moon { 2.5 } else { 3.0 }, env.night.clamp(0.0, 1.0));
    p.ycoef = Vec4::new(pos.y + right.y * centre.x + up.y * centre.y, right.y * half.x, up.y * half.y, fwd.y);
    p.bank = Vec4::new(env.sea_fog.clamp(0.0, 1.0), env.fog_top, 0.0, 0.0);
    let rim = Color::srgb(env.rim_color.x, env.rim_color.y, env.rim_color.z).to_linear();
    p.rim = Vec4::new(rim.red, rim.green, rim.blue, if env.rim_color.max_element() > 0.0 { 0.8 * env.storm.clamp(0.0, 1.0) } else { 0.0 });
    p
}

/// Screen uv y (0 = top) of a point `rel` (relative to the camera) for an orthographic view
/// with up axis `up` covering `area` (metres around the camera axis). Clamped to a sane range.
fn sea_line_uv(rel: Vec3, up: Vec3, area: Rect) -> f32 {
    ((area.max.y - rel.dot(up)) / area.height().max(1e-3)).clamp(-4.0, 1.0)
}

/// Window position (physical px) of a low-res pixel position: the inverse of
/// [`window_to_low_res`] for a window of `physical` px (for screen-space HUD markers).
pub fn low_res_to_window(low_px: Vec2, physical: UVec2) -> Vec2 {
    low_px * display_scale(physical.y) as f32 + display_offset(physical)
}

/// Converts cairn_cove_core mesh buffers to a Bevy mesh with POSITION, NORMAL, UV_0, COLOR
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
/// `world::warm_light` (0 by day) to fade lanterns.
pub fn lantern_light(intensity_lm: f32, range: f32) -> PointLight {
    PointLight {
        color: Color::srgb_u8(0xFF, 0xA5, 0x42),
        intensity: intensity_lm,
        range,
        radius: 0.1,
        shadow_maps_enabled: false,
        ..default()
    }
}

/// Maps a window cursor position (physical px, top-left origin) to low-res pixels (the
/// low-res image is shown at the integer `scale` with its top-left corner at `offset`, see
/// [`display_offset`]).
pub fn window_to_low_res(cursor_physical: Vec2, scale: u32, offset: Vec2) -> Vec2 {
    (cursor_physical - offset) / scale.max(1) as f32
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
    let physical = window.physical_size();
    let p = window_to_low_res(window_cursor * window.scale_factor(), display_scale(physical.y), display_offset(physical));
    // The letterbox bars are not part of the image: no ray there, so tools ignore them.
    if !(p.cmpge(Vec2::ZERO).all() && p.cmple(low_res.as_vec2()).all()) {
        return None;
    }
    cam.viewport_to_world(cam_tf, p).ok()
}

/// Height in logical px of the shown (letterboxed) image: the [`VIEW_HEIGHT`] metres span
/// this many px, not the whole window.
pub fn image_height(window: &Window) -> f32 {
    (LOW_RES_HEIGHT * display_scale(window.physical_size().y)) as f32 / window.scale_factor()
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
    use cairn_cove_core::Surface;

    #[test]
    fn cursor_scales_to_low_res() {
        assert_eq!(window_to_low_res(Vec2::ZERO, 3, Vec2::ZERO), Vec2::ZERO);
        assert_eq!(window_to_low_res(Vec2::new(1280.0, 720.0), 3, Vec2::ZERO), Vec2::new(1280.0 / 3.0, 240.0));
        assert_eq!(window_to_low_res(Vec2::new(5.0, 5.0), 0, Vec2::ZERO), Vec2::new(5.0, 5.0));
        // 1080 p letterboxes 60 px above the image: the image's first row is window row 60.
        let (s, off) = (display_scale(1080), display_offset(UVec2::new(1920, 1080)));
        assert_eq!(window_to_low_res(Vec2::new(8.0, 60.0), s, off), Vec2::new(2.0, 0.0));
        assert_eq!(window_to_low_res(Vec2::new(1920.0, 1020.0), s, off), Vec2::new(480.0, 240.0));
    }

    /// The internal target is 240 rows at every window height (SPEC), shown at an integer
    /// scale: each low-res pixel is exactly s × s window px, the width covers the window
    /// (crop < s px) and the leftover rows are split evenly top and bottom.
    #[test]
    fn low_res_keeps_240_rows_at_an_integer_scale() {
        assert_eq!(low_res_for(None), UVec2::new(427, 240));
        for (w, h, scale, top) in [
            (1280u32, 720u32, 3u32, 0.0f32),
            (1920, 1080, 4, 60.0),
            (2560, 1440, 6, 0.0),
            (1366, 768, 3, 24.0),
            (1440, 900, 4, -30.0),
            (1280, 719, 3, -1.0),
            (640, 479, 2, -1.0),
            (800, 600, 2, 60.0),
            (300, 200, 1, -20.0),
        ] {
            let (s, low, off) = (display_scale(h), low_res_for_physical(UVec2::new(w, h)), display_offset(UVec2::new(w, h)));
            assert_eq!((s, low.y, off.y), (scale, LOW_RES_HEIGHT, top), "{w}x{h}");
            assert!(low.x * s >= w && low.x * s < w + s, "{w}x{h}: width covers with < s crop");
            assert!((h as f32 - (low.y * s) as f32).abs() <= (LOW_RES_HEIGHT * s) as f32 / 2.0, "{w}x{h}: nearest scale");
        }
    }

    #[test]
    fn mesh_conversion_keeps_surfaces() {
        let mut m = MeshData::default();
        m.aabb(Vec3::ZERO, Vec3::ONE, Vec3::ONE, Surface::Thatch, 1.0);
        let b = to_bevy_mesh(&m);
        assert_eq!(b.count_vertices(), 24);
        let Some(bevy::mesh::VertexAttributeValues::Float32x2(s)) = b.attribute(Mesh::ATTRIBUTE_UV_1) else {
            panic!("uv1 missing")
        };
        assert_eq!(s[0][0], Surface::Thatch as u8 as f32);
    }

    #[test]
    fn default_camera_looks_at_focus() {
        let t = camera_transform(Vec3::ZERO, DEFAULT_YAW, DEFAULT_PITCH);
        assert!((t.translation.length() - CAMERA_DISTANCE).abs() < 1e-3);
        assert!(t.forward().dot(-t.translation.normalize()) > 0.999);
    }
}
