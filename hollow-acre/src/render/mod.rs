//! Low-res toon rendering.
//!
//! - [`GameCamera`]: orthographic `Camera3d` (order -1) rendering into the low-res
//!   [`LowResTarget`] image (about [`LOW_RES_HEIGHT`] px tall, sized by [`fill_layout`]),
//!   with depth+normal prepass, outline post-process ([`outline`]) and HDR bloom for
//!   emissives. Its `Transform` and projection are driven by `crate::camera`.
//! - [`WindowCamera`]: `Camera2d` (order 0, `IsDefaultUiCamera`) showing the image at
//!   integer scale `round(window_h / 270)`, filling the window (see [`fill_layout`]). Bevy UI without a
//!   `UiTargetCamera` renders here at full window resolution.
//! - [`Sun`]: the one shadow-casting `DirectionalLight` (2048², 1 cascade, hard edges).
//! - [`Atmosphere`] is read every frame and pushed into the light, clear colour,
//!   all [`ToonMaterial`]s and the outline settings.

pub mod atmosphere;
pub mod outline;
pub mod palette;
pub mod test_scene;
pub mod toon;

pub use atmosphere::{AtmosPreset, Atmosphere, AtmosphereDriven};
pub use outline::OutlineSettings;
pub use toon::{MAX_LAMPS, ToonMaterial, ToonParams, VoxelMaterials, toon_material, voxel_mesh};

use bevy::{
    camera::{Hdr, RenderTarget, ScalingMode},
    core_pipeline::prepass::{DepthPrepass, NormalPrepass},
    image::ImageSampler,
    light::{CascadeShadowConfigBuilder, DirectionalLightShadowMap, ShadowFilteringMethod},
    picking::Pickable,
    post_process::bloom::{Bloom, BloomCompositeMode, BloomPrefilter},
    prelude::*,
    render::{
        render_resource::{Extent3d, TextureFormat},
        view::{DebandDither, Tonemapping},
    },
    window::PrimaryWindow,
};

/// Internal render height in pixels (DESIGN: 270).
pub const LOW_RES_HEIGHT: u32 = 270;
/// Distance from the camera to its focus along the view direction (world units).
pub const CAMERA_DISTANCE: f32 = 150.0;
/// Orthographic near/far planes.
pub const CAMERA_NEAR: f32 = 1.0;
pub const CAMERA_FAR: f32 = 400.0;
/// Letterbox colour (night sky `#1d2347`).
pub const LETTERBOX: Color = Color::srgb(0x1d as f32 / 255.0, 0x23 as f32 / 255.0, 0x47 as f32 / 255.0);
/// Lightning sky colour `#8e7ad1`.
const FLASH_SKY: Color = Color::srgb(0x8e as f32 / 255.0, 0x7a as f32 / 255.0, 0xd1 as f32 / 255.0);

/// Low-res target size and where it sits in the window.
#[derive(Resource, Clone, Copy, Debug, PartialEq)]
pub struct LowRes {
    /// Target size in pixels.
    pub size: UVec2,
    /// Top-left of the upscaled image in the window, logical px (always zero now).
    pub offset: Vec2,
    /// Logical window px per low-res pixel (integer in physical px).
    pub scale: f32,
}

impl LowRes {
    /// Window cursor position (logical px, top-left origin) → low-res pixel position,
    /// or `None` outside the image.
    pub fn window_to_low_res(&self, cursor: Vec2) -> Option<Vec2> {
        let p = (cursor - self.offset) / self.scale.max(1e-6);
        let s = self.size.as_vec2();
        (p.x >= 0.0 && p.y >= 0.0 && p.x < s.x && p.y < s.y).then_some(p)
    }
}

/// The image the [`GameCamera`] renders into.
#[derive(Resource, Clone)]
pub struct LowResTarget(pub Handle<Image>);

#[derive(Component)]
pub struct GameCamera;

#[derive(Component)]
pub struct WindowCamera;

/// The shadow-casting sun/moon light.
#[derive(Component)]
pub struct Sun;

/// The fullscreen `ImageNode` showing the low-res image.
#[derive(Component)]
pub struct LowResDisplay;

/// World positions of lamp heads. Fill/replace this (e.g. from the voxel grid's Lamp
/// voxels); at night the [`MAX_LAMPS`] nearest to the view centre light the toon
/// materials (radius 3 wu, 2 bands).
#[derive(Resource, Clone, Default, Debug)]
pub struct LampLights(pub Vec<Vec3>);

/// The lamps currently uploaded to the materials (subset of [`LampLights`]).
#[derive(Resource, Clone, Default, PartialEq)]
pub struct ActiveLamps(pub Vec<Vec3>);

fn pick_lamps(
    lamps: Res<LampLights>,
    atmos: Res<Atmosphere>,
    cam: Query<&GlobalTransform, With<GameCamera>>,
    mut active: ResMut<ActiveLamps>,
) {
    let mut pick = Vec::new();
    if atmos.night > 0.0
        && let Ok(gt) = cam.single()
    {
        // Distance to the camera's view axis = distance from the view centre on screen.
        let (origin, fwd) = (gt.translation(), gt.forward().as_vec3());
        let off_axis = |p: &Vec3| (*p - origin).reject_from_normalized(fwd).length_squared();
        pick = lamps.0.clone();
        pick.sort_by(|a, b| off_axis(a).total_cmp(&off_axis(b)));
        pick.truncate(MAX_LAMPS);
    }
    if active.0 != pick {
        active.0 = pick;
    }
}

/// View depth where distance fog starts (kept in sync with zoom).
#[derive(Resource, Clone, Copy, Default)]
pub struct FogStart(pub f32);

/// Runs in `PreStartup`; afterwards [`LowRes`], [`LowResTarget`], [`VoxelMaterials`],
/// both cameras and the sun exist.
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct RenderSetup;

pub struct ToonRenderPlugin;

impl Plugin for ToonRenderPlugin {
    fn build(&self, app: &mut App) {
        bevy::asset::embedded_asset!(app, "toon.wesl");
        bevy::asset::embedded_asset!(app, "toon_prepass.wesl");
        app.add_plugins((MaterialPlugin::<ToonMaterial>::default(), outline::OutlinePlugin))
            .insert_resource(DirectionalLightShadowMap { size: 2048 })
            .init_resource::<Atmosphere>()
            .init_resource::<atmosphere::DebugPreset>()
            .init_resource::<FogStart>()
            .init_resource::<LampLights>()
            .init_resource::<ActiveLamps>()
            .add_systems(PreStartup, setup.in_set(RenderSetup))
            .add_systems(
                Startup,
                test_scene::spawn_test_scene.run_if(resource_exists::<test_scene::TestScene>),
            )
            .add_systems(
                Update,
                (atmosphere::cycle_preset_on_f5, atmosphere::tick_default_atmosphere),
            )
            .add_systems(
                PostUpdate,
                (fit_to_window, apply_atmosphere, pick_lamps, toon::sync_materials).chain(),
            );
    }
}

/// Fill layout for a window (physical px): integer scale `s = round(h / 270)` (min 1)
/// and low-res size `ceil(window / s)`, so the upscaled image covers the whole window
/// (at most `s - 1` px cropped at the right/bottom edge). Returns (size, scale).
pub fn fill_layout(window_physical: Vec2) -> (UVec2, u32) {
    let w = window_physical.max(Vec2::ONE);
    let w = if w.is_finite() { w } else { Vec2::new(1280.0, 720.0) };
    let scale = ((w.y / LOW_RES_HEIGHT as f32).round() as u32).max(1);
    let size = (w / scale as f32).ceil().as_uvec2().max(UVec2::ONE);
    (size, scale)
}

fn window_layout(window: Option<&Window>) -> LowRes {
    let (phys, sf) = window.map_or((Vec2::new(1280.0, 720.0), 1.0), |w| {
        (w.physical_size().as_vec2(), w.scale_factor())
    });
    let (size, scale) = fill_layout(phys);
    LowRes { size, offset: Vec2::ZERO, scale: scale as f32 / sf }
}

fn setup(
    mut commands: Commands,
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<ToonMaterial>>,
    window: Query<&Window, With<PrimaryWindow>>,
) {
    let low = window_layout(window.single().ok());
    let mut target = Image::new_target_texture(low.size.x, low.size.y, TextureFormat::Rgba8UnormSrgb, None);
    target.sampler = ImageSampler::nearest();
    let target = images.add(target);

    let atmos = Atmosphere::default();
    let opaque = materials.add(ToonMaterial { params: ToonParams::default() });
    let water = materials.add(ToonMaterial { params: ToonParams { mode: 1, ..default() } });
    commands.insert_resource(VoxelMaterials { opaque, water });
    commands.insert_resource(low);
    commands.insert_resource(LowResTarget(target.clone()));

    commands.spawn((
        GameCamera,
        Camera3d::default(),
        Camera { order: -1, clear_color: ClearColorConfig::Custom(atmos.sky_color), ..default() },
        RenderTarget::Image(target.clone().into()),
        Projection::Orthographic(OrthographicProjection {
            near: CAMERA_NEAR,
            far: CAMERA_FAR,
            scaling_mode: ScalingMode::FixedVertical { viewport_height: 36.0 },
            ..OrthographicProjection::default_3d()
        }),
        Msaa::Off,
        Hdr,
        Tonemapping::None,
        DebandDither::Disabled,
        (DepthPrepass, NormalPrepass, ShadowFilteringMethod::Hardware2x2),
        Bloom {
            intensity: 0.22,
            low_frequency_boost: 0.4,
            prefilter: BloomPrefilter { threshold: 1.0, threshold_softness: 0.2 },
            composite_mode: BloomCompositeMode::Additive,
            max_mip_dimension: 128,
            ..Bloom::NATURAL
        },
        OutlineSettings { depth_range: CAMERA_FAR - CAMERA_NEAR, ..default() },
        Transform::from_xyz(100.0, 100.0, 100.0).looking_at(Vec3::ZERO, Vec3::Y),
    ));

    // One cascade covering the visible slab around the focus.
    let cascades = CascadeShadowConfigBuilder {
        num_cascades: 1,
        minimum_distance: CAMERA_DISTANCE - 80.0,
        maximum_distance: CAMERA_DISTANCE + 80.0,
        first_cascade_far_bound: CAMERA_DISTANCE + 80.0,
        overlap_proportion: 0.0,
    }
    .build();
    commands.spawn((
        Sun,
        DirectionalLight {
            color: atmos.sun_color,
            illuminance: atmos.sun_illuminance * light_consts::lux::AMBIENT_DAYLIGHT,
            shadow_maps_enabled: true,
            shadow_depth_bias: 0.04,
            shadow_normal_bias: 1.2,
            ..default()
        },
        cascades,
        Transform::from_translation(atmos.sun_dir).looking_at(Vec3::ZERO, Vec3::Y),
    ));

    commands.spawn((
        WindowCamera,
        Camera2d,
        Camera { order: 0, clear_color: ClearColorConfig::Custom(LETTERBOX), ..default() },
        Msaa::Off,
        Tonemapping::None,
        DebandDither::Disabled,
        IsDefaultUiCamera,
    ));
    commands.spawn((
        LowResDisplay,
        ImageNode::new(target),
        display_node(&low),
        GlobalZIndex(i32::MIN),
        Pickable::IGNORE,
    ));
}

fn display_node(low: &LowRes) -> Node {
    Node {
        position_type: PositionType::Absolute,
        left: px(low.offset.x),
        top: px(low.offset.y),
        width: px(low.size.x as f32 * low.scale),
        height: px(low.size.y as f32 * low.scale),
        ..default()
    }
}

fn fit_to_window(
    window: Query<&Window, (With<PrimaryWindow>, Changed<Window>)>,
    mut low: ResMut<LowRes>,
    target: Res<LowResTarget>,
    mut images: ResMut<Assets<Image>>,
    mut display: Query<&mut Node, With<LowResDisplay>>,
) {
    let Ok(window) = window.single() else { return };
    let new = window_layout(Some(window));
    if new == *low {
        return;
    }
    if new.size != low.size
        && let Some(mut image) = images.get_mut(&target.0)
    {
        image.resize(Extent3d { width: new.size.x, height: new.size.y, depth_or_array_layers: 1 });
    }
    *low = new;
    for mut node in &mut display {
        *node = display_node(&new);
    }
}

fn apply_atmosphere(
    atmos: Res<Atmosphere>,
    mut fog_start: ResMut<FogStart>,
    mut sun: Query<(&mut DirectionalLight, &mut Transform), With<Sun>>,
    mut cam: Query<(&mut Camera, &Projection, &mut OutlineSettings), With<GameCamera>>,
) {
    for (mut light, mut tf) in &mut sun {
        let dir = atmos.sun_dir.normalize_or(Vec3::Y);
        let want = Transform::from_translation(dir).looking_at(Vec3::ZERO, Vec3::Y);
        if tf.rotation != want.rotation {
            tf.rotation = want.rotation;
        }
        light.color = atmos.sun_color;
        light.illuminance = atmos.sun_illuminance.max(0.0) * light_consts::lux::AMBIENT_DAYLIGHT;
    }
    for (mut camera, projection, mut outline) in &mut cam {
        let flash = atmos.flash.clamp(0.0, 1.0);
        let sky = atmos.sky_color.mix(&FLASH_SKY, flash);
        camera.clear_color = ClearColorConfig::Custom(sky);
        let view_height = match projection {
            Projection::Orthographic(o) => match o.scaling_mode {
                ScalingMode::FixedVertical { viewport_height } => viewport_height * o.scale,
                _ => 36.0,
            },
            _ => 36.0,
        };
        fog_start.0 = CAMERA_DISTANCE - view_height * 0.3;
        outline.time = atmos.time_secs;
        outline.sparkle = if atmos.sun_illuminance > 0.5 && atmos.night < 0.5 { 1.0 } else { 0.0 };
        outline.rain = atmos.wetness.clamp(0.0, 1.0);
        outline.night = atmos.night.clamp(0.0, 1.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fill_layout_covers_window() {
        assert_eq!(fill_layout(Vec2::new(1920.0, 1080.0)), (UVec2::new(480, 270), 4));
        assert_eq!(fill_layout(Vec2::new(1280.0, 720.0)), (UVec2::new(427, 240), 3));
        assert_eq!(fill_layout(Vec2::new(100.0, 50.0)), (UVec2::new(100, 50), 1));
        for w in [Vec2::new(1366.0, 768.0), Vec2::new(2560.0, 1080.0), Vec2::new(801.0, 599.0)] {
            let (size, s) = fill_layout(w);
            let covered = (size * s).as_vec2();
            assert!(covered.x >= w.x && covered.y >= w.y);
            assert!(covered.x - w.x < s as f32 && covered.y - w.y < s as f32);
        }
    }

    #[test]
    fn cursor_mapping_follows_scale() {
        let low = LowRes { size: UVec2::new(427, 240), offset: Vec2::ZERO, scale: 3.0 };
        assert_eq!(low.window_to_low_res(Vec2::new(-1.0, 10.0)), None);
        assert_eq!(low.window_to_low_res(Vec2::ZERO), Some(Vec2::ZERO));
        assert_eq!(low.window_to_low_res(Vec2::new(6.0, 9.0)), Some(Vec2::new(2.0, 3.0)));
        assert_eq!(low.window_to_low_res(Vec2::new(1279.0, 719.0)), Some(Vec2::new(1279.0 / 3.0, 719.0 / 3.0)));
    }
}
