//! Orbiting isometric camera rig: orbit (RMB drag, Q/E 45° steps), zoom (scroll), pan
//! (WASD / arrows / MMB drag), tilt (PageUp/PageDown), Home to reset. Pointer input that starts
//! over the HUD (`tools::PointerOverUi`) is ignored. Distance stays [`CAMERA_DISTANCE`].

use crate::render::{DEFAULT_PITCH, DEFAULT_YAW, GameCamera, RenderTest, camera_transform};
use crate::tools::PointerOverUi;
use bevy::{
    input::mouse::{AccumulatedMouseMotion, AccumulatedMouseScroll, MouseScrollUnit},
    prelude::*,
};
use cairn_cove_core::GLADE_HALF;

pub(crate) const MIN_ZOOM: f32 = 0.3;
/// Far enough out to see the whole 64 m island plus open sea around it.
pub(crate) const MAX_ZOOM: f32 = 3.2;
/// Default framing: the whole island and a margin of sea (see `default_frames_the_island`).
const DEFAULT_ZOOM: f32 = 2.3;
const MIN_PITCH: f32 = 25.0;
const MAX_PITCH: f32 = 55.0;
const PAN_SPEED: f32 = 14.0;
const ORBIT_DEG_PER_PX: f32 = 0.3;
/// Exponential smoothing rate (1/s).
const SMOOTH: f32 = 12.0;

/// Camera state. `yaw_deg`/`pitch_deg`/`zoom` chase their `target_*` values.
#[derive(Resource, Clone, Debug)]
pub struct CameraRig {
    pub focus: Vec3,
    pub yaw_deg: f32,
    pub pitch_deg: f32,
    /// Orthographic projection scale (<1 closer, >1 further).
    pub zoom: f32,
    pub target_yaw: f32,
    pub target_pitch: f32,
    pub target_zoom: f32,
}

impl Default for CameraRig {
    fn default() -> Self {
        Self {
            focus: Vec3::ZERO,
            yaw_deg: DEFAULT_YAW,
            pitch_deg: DEFAULT_PITCH,
            zoom: DEFAULT_ZOOM,
            target_yaw: DEFAULT_YAW,
            target_pitch: DEFAULT_PITCH,
            target_zoom: DEFAULT_ZOOM,
        }
    }
}

impl CameraRig {
    /// Jump (no smoothing) to a yaw.
    pub fn set_yaw(&mut self, yaw: f32) {
        self.yaw_deg = yaw;
        self.target_yaw = yaw;
    }
    /// Jump (no smoothing) to a zoom.
    pub fn set_zoom(&mut self, zoom: f32) {
        self.zoom = zoom.clamp(MIN_ZOOM, MAX_ZOOM);
        self.target_zoom = self.zoom;
    }
    /// Unit vectors on the ground plane: screen-right and screen-up (into the view).
    pub fn ground_axes(&self) -> (Vec3, Vec3) {
        let y = self.yaw_deg.to_radians();
        (Vec3::new(y.cos(), 0.0, -y.sin()), Vec3::new(-y.sin(), 0.0, -y.cos()))
    }
}

/// Tools set these (every frame they apply) when they consume scroll / right mouse.
#[derive(Resource, Default, Clone, Copy, Debug)]
pub struct CameraInput {
    pub scroll_captured: bool,
    pub rmb_captured: bool,
}

/// Next 45° yaw step from `yaw` in direction `dir` (+1 / -1).
pub fn step_yaw(yaw: f32, dir: f32) -> f32 {
    let s = yaw / 45.0;
    if dir > 0.0 { (s + 1e-3).floor() + 1.0 } else { (s - 1e-3).ceil() - 1.0 }.mul_add(45.0, 0.0)
}

/// Nearest 45° yaw (where a free right-drag orbit settles on release).
pub fn snap_yaw(yaw: f32) -> f32 {
    (yaw / 45.0).round() * 45.0
}

/// Frame-rate independent approach of `cur` towards `target`.
fn approach(cur: f32, target: f32, dt: f32) -> f32 {
    let k = 1.0 - (-SMOOTH * dt).exp();
    let n = cur + (target - cur) * k;
    if (target - n).abs() < 1e-3 { target } else { n }
}

pub struct CameraPlugin;

impl Plugin for CameraPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<CameraRig>().init_resource::<CameraInput>().add_systems(
            PostUpdate,
            drive_camera.before(TransformSystems::Propagate).run_if(not(resource_exists::<RenderTest>)),
        );
    }
}

#[allow(clippy::too_many_arguments)]
fn drive_camera(
    time: Res<Time>,
    keys: Res<ButtonInput<KeyCode>>,
    mouse: Res<ButtonInput<MouseButton>>,
    motion: Res<AccumulatedMouseMotion>,
    scroll: Res<AccumulatedMouseScroll>,
    window: Option<Single<&Window, With<bevy::window::PrimaryWindow>>>,
    over_ui: Option<Res<PointerOverUi>>,
    mut ui_drag: Local<[bool; 2]>,
    mut orbiting: Local<bool>,
    mut input: ResMut<CameraInput>,
    mut rig: ResMut<CameraRig>,
    mut cam: Query<(&mut Transform, &mut Projection), With<GameCamera>>,
) {
    let dt = time.delta_secs().min(0.1);
    let captured = std::mem::take(&mut *input);
    let over_ui = over_ui.is_some_and(|o| o.0);
    // A drag that starts over the HUD never orbits / pans the world.
    for (i, b) in [MouseButton::Right, MouseButton::Middle].into_iter().enumerate() {
        if mouse.just_pressed(b) {
            ui_drag[i] = over_ui;
        }
    }
    let orbit = mouse.pressed(MouseButton::Right) && !ui_drag[0] && !captured.rmb_captured;
    let pan = mouse.pressed(MouseButton::Middle) && !ui_drag[1];

    if keys.just_pressed(KeyCode::Home) {
        *rig = CameraRig { target_yaw: DEFAULT_YAW, target_pitch: DEFAULT_PITCH, ..default() };
    }
    if keys.just_pressed(KeyCode::KeyQ) {
        rig.target_yaw = step_yaw(rig.target_yaw, -1.0);
    }
    if keys.just_pressed(KeyCode::KeyE) {
        rig.target_yaw = step_yaw(rig.target_yaw, 1.0);
    }
    if orbit {
        rig.target_yaw -= motion.delta.x * ORBIT_DEG_PER_PX;
        *orbiting = true;
    } else if *orbiting {
        // Right button let go (or the drag was captured): settle on the nearest 45 degree step.
        *orbiting = false;
        rig.target_yaw = snap_yaw(rig.target_yaw);
    }
    let tilt = keys.pressed(KeyCode::PageUp) as i8 - keys.pressed(KeyCode::PageDown) as i8;
    rig.target_pitch = (rig.target_pitch + tilt as f32 * 40.0 * dt).clamp(MIN_PITCH, MAX_PITCH);
    if !captured.scroll_captured && !over_ui && scroll.delta.y != 0.0 {
        let lines = if scroll.unit == MouseScrollUnit::Line { scroll.delta.y } else { scroll.delta.y / 30.0 };
        rig.target_zoom = (rig.target_zoom * 0.88_f32.powf(lines)).clamp(MIN_ZOOM, MAX_ZOOM);
    }

    rig.yaw_deg = approach(rig.yaw_deg, rig.target_yaw, dt);
    rig.pitch_deg = approach(rig.pitch_deg, rig.target_pitch, dt);
    rig.zoom = approach(rig.zoom, rig.target_zoom, dt);

    let (right, up) = rig.ground_axes();
    let axis = |pos: &[KeyCode], neg: &[KeyCode]| {
        keys.any_pressed(pos.iter().copied()) as i8 as f32 - keys.any_pressed(neg.iter().copied()) as i8 as f32
    };
    let k = axis(&[KeyCode::KeyD, KeyCode::ArrowRight], &[KeyCode::KeyA, KeyCode::ArrowLeft]);
    let f = axis(&[KeyCode::KeyW, KeyCode::ArrowUp], &[KeyCode::KeyS, KeyCode::ArrowDown]);
    let mut delta = (right * k + up * f) * PAN_SPEED * rig.zoom * dt;
    if pan {
        let h = window.map_or(720.0, |w| crate::render::image_height(&w)).max(1.0);
        let per_px = crate::render::VIEW_HEIGHT * rig.zoom / h;
        let sin_p = rig.pitch_deg.to_radians().sin().max(0.2);
        delta += -right * motion.delta.x * per_px + up * (motion.delta.y * per_px / sin_p);
    }
    rig.focus = (rig.focus + delta).clamp(Vec3::splat(-GLADE_HALF), Vec3::splat(GLADE_HALF)).with_y(0.0);

    if let Ok((mut tf, mut proj)) = cam.single_mut() {
        *tf = camera_transform(rig.focus, rig.yaw_deg, rig.pitch_deg);
        if let Projection::Orthographic(o) = &mut *proj {
            o.scale = rig.zoom;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn yaw_steps_snap_to_45() {
        assert_eq!(step_yaw(45.0, 1.0), 90.0);
        assert_eq!(step_yaw(45.0, -1.0), 0.0);
        assert_eq!(step_yaw(50.0, 1.0), 90.0);
        assert_eq!(step_yaw(50.0, -1.0), 45.0);
        assert_eq!(step_yaw(0.0, -1.0), -45.0);
    }

    /// Half extents (m) of what the view covers on screen at `zoom` and `aspect`: (width, height).
    fn view_half(zoom: f32, aspect: f32) -> (f32, f32) {
        let h = crate::render::VIEW_HEIGHT * zoom * 0.5;
        (h * aspect, h)
    }

    #[test]
    fn orbit_release_snaps_to_45() {
        assert_eq!(snap_yaw(63.0), 45.0);
        assert_eq!(snap_yaw(68.0), 90.0);
        assert_eq!(snap_yaw(-20.0), -0.0);
        assert_eq!(snap_yaw(-30.0), -45.0);
        assert_eq!(snap_yaw(90.0), 90.0);
    }

    #[test]
    fn default_frames_the_island() {
        // At yaw 45 a 64 m square is a diamond 2 * 32 * sqrt(2) wide; its screen height is that
        // times sin(pitch). Both must fit at 16:9, with sea to spare.
        let diag = GLADE_HALF * std::f32::consts::SQRT_2;
        let rig = CameraRig::default();
        let (hw, hh) = view_half(rig.zoom, 16.0 / 9.0);
        assert!(hw > diag * 1.05, "island wider than the default view: {hw} vs {diag}");
        assert!(hh > diag * rig.pitch_deg.to_radians().sin() * 1.05, "island taller than the default view");
        // Zooming all the way out still shows more than the default.
        assert!(MAX_ZOOM > DEFAULT_ZOOM && view_half(MAX_ZOOM, 16.0 / 9.0).0 > hw);
        assert!((MIN_ZOOM..=MAX_ZOOM).contains(&DEFAULT_ZOOM));
    }

    #[test]
    fn pan_axes_match_camera() {
        let rig = CameraRig::default();
        let (right, up) = rig.ground_axes();
        let t = camera_transform(Vec3::ZERO, rig.yaw_deg, rig.pitch_deg);
        assert!(t.right().with_y(0.0).normalize().dot(right) > 0.999);
        assert!(t.forward().with_y(0.0).normalize().dot(up) > 0.999);
    }

    #[test]
    fn approach_converges() {
        let mut v = 0.0;
        for _ in 0..200 {
            v = approach(v, 45.0, 0.016);
        }
        assert_eq!(v, 45.0);
    }
}
