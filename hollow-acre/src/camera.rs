//! Orthographic isometric camera rig (DESIGN "Camera"): fixed 35° pitch, free yaw,
//! smoothed orbit/zoom/pan, pixel-snapped to the low-res texel grid, plus [`CursorRay`].

use crate::render::{CAMERA_DISTANCE, GameCamera, LOW_RES_HEIGHT, LowRes};
use bevy::camera::ScalingMode;
use bevy::input::mouse::{AccumulatedMouseMotion, AccumulatedMouseScroll, MouseScrollUnit};
use bevy::picking::hover::HoverMap;
use bevy::picking::pointer::PointerId;
use bevy::prelude::*;
use bevy::window::PrimaryWindow;

pub const PITCH_DEG: f32 = 35.0;
pub const DEFAULT_YAW_DEG: f32 = 45.0;
pub const DEFAULT_VIEW_HEIGHT: f32 = 36.0;
pub const MIN_VIEW_HEIGHT: f32 = 14.0;
pub const MAX_VIEW_HEIGHT: f32 = 80.0;
/// Focus clamp (DESIGN: x,z within ±44).
pub const FOCUS_LIMIT: f32 = 44.0;
const SMOOTH_TIME: f32 = 0.15;
const ORBIT_KEY_DEG_PER_S: f32 = 90.0;
const ORBIT_DRAG_DEG_PER_PX: f32 = 0.3;
const ZOOM_STEP: f32 = 1.12;

pub struct CameraPlugin;

impl Plugin for CameraPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<CameraRig>()
            .init_resource::<CursorRay>()
            .add_systems(Update, (rig_input, apply_rig, update_cursor_ray).chain());
    }
}

/// A camera pose: ground focus point, yaw (radians) and orthographic view height (wu).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RigPose {
    /// Focus point; y is the ground height the camera looks at.
    pub focus: Vec3,
    pub yaw: f32,
    pub view_height: f32,
}

impl Default for RigPose {
    fn default() -> Self {
        Self {
            focus: Vec3::new(0.0, 6.0, 0.0),
            yaw: DEFAULT_YAW_DEG.to_radians(),
            view_height: DEFAULT_VIEW_HEIGHT,
        }
    }
}

impl RigPose {
    pub fn clamped(mut self) -> Self {
        self.focus.x = self.focus.x.clamp(-FOCUS_LIMIT, FOCUS_LIMIT);
        self.focus.z = self.focus.z.clamp(-FOCUS_LIMIT, FOCUS_LIMIT);
        self.view_height = self.view_height.clamp(MIN_VIEW_HEIGHT, MAX_VIEW_HEIGHT);
        self
    }

    /// Camera transform for this pose, before pixel snapping.
    pub fn transform(&self) -> Transform {
        let p = PITCH_DEG.to_radians();
        let back = Vec3::new(p.cos() * self.yaw.sin(), p.sin(), p.cos() * self.yaw.cos());
        Transform::from_translation(self.focus + back * CAMERA_DISTANCE).looking_at(self.focus, Vec3::Y)
    }

    /// Transform snapped so the camera moves in whole low-res texels in its view plane.
    pub fn snapped_transform(&self) -> Transform {
        let mut tf = self.transform();
        let texel = self.view_height / LOW_RES_HEIGHT as f32;
        let (right, up) = (tf.rotation * Vec3::X, tf.rotation * Vec3::Y);
        let (r, u) = (tf.translation.dot(right), tf.translation.dot(up));
        let snap = |v: f32| (v / texel).round() * texel;
        tf.translation += right * (snap(r) - r) + up * (snap(u) - u);
        tf
    }

    /// Unit ground vectors (forward away from the camera, right) for this yaw.
    pub fn ground_axes(&self) -> (Vec3, Vec3) {
        let (s, c) = self.yaw.sin_cos();
        (Vec3::new(-s, 0.0, -c), Vec3::new(c, 0.0, -s))
    }
}

/// `target` is driven by input; `current` follows it (critically damped) and drives
/// the [`GameCamera`].
#[derive(Resource, Default)]
pub struct CameraRig {
    pub target: RigPose,
    pub current: RigPose,
    vel_focus: Vec3,
    vel_yaw: f32,
    vel_zoom: f32,
    orbiting: bool,
    panning: bool,
}

impl CameraRig {
    /// Jumps straight to `pose` without easing.
    pub fn snap(&mut self, pose: RigPose) {
        self.target = pose.clamped();
        self.current = self.target;
        self.vel_focus = Vec3::ZERO;
        self.vel_yaw = 0.0;
        self.vel_zoom = 0.0;
    }
}

/// World-space ray from the mouse through the low-res camera, `None` when the cursor is
/// outside the window or in the letterbox. Updated every `Update`.
#[derive(Resource, Default, Clone, Copy, Debug)]
pub struct CursorRay(pub Option<Ray3d>);

/// True if the mouse is over a pickable UI node, e.g. a HUD button.
pub fn pointer_over_ui(hover: Option<&HoverMap>, nodes: &Query<(), With<Node>>) -> bool {
    hover
        .and_then(|h| h.get(&PointerId::Mouse))
        .is_some_and(|hits| hits.keys().any(|&e| nodes.contains(e)))
}

fn rig_input(
    mut rig: ResMut<CameraRig>,
    mouse: Res<ButtonInput<MouseButton>>,
    keys: Res<ButtonInput<KeyCode>>,
    (motion, scroll): (Res<AccumulatedMouseMotion>, Res<AccumulatedMouseScroll>),
    time: Res<Time>,
    low: Option<Res<LowRes>>,
    hover: Option<Res<HoverMap>>,
    nodes: Query<(), With<Node>>,
) {
    let over_ui = pointer_over_ui(hover.as_deref(), &nodes);
    let r = &mut *rig;
    if mouse.just_pressed(MouseButton::Right) && !over_ui {
        r.orbiting = true;
    }
    if mouse.just_pressed(MouseButton::Middle) && !over_ui {
        r.panning = true;
    }
    r.orbiting &= mouse.pressed(MouseButton::Right);
    r.panning &= mouse.pressed(MouseButton::Middle);

    let dt = time.delta_secs();
    let t = &mut r.target;
    let (fwd, right) = t.ground_axes();
    if r.orbiting {
        t.yaw -= (motion.delta.x * ORBIT_DRAG_DEG_PER_PX).to_radians();
    }
    if r.panning {
        // Window px -> world units on the view plane; screen-up maps to ground-forward
        // stretched by 1/sin(pitch).
        let px_scale = low.as_deref().map_or(1.0, |l| l.scale.max(1e-3));
        let wu = t.view_height / LOW_RES_HEIGHT as f32 / px_scale;
        let up_k = 1.0 / PITCH_DEG.to_radians().sin();
        t.focus += (-right * motion.delta.x + fwd * motion.delta.y * up_k) * wu;
    }
    if !over_ui && scroll.delta.y != 0.0 {
        let steps = match scroll.unit {
            MouseScrollUnit::Line => scroll.delta.y,
            MouseScrollUnit::Pixel => scroll.delta.y / 40.0,
        };
        t.view_height *= ZOOM_STEP.powf(-steps);
    }
    let ctrl = keys.any_pressed([KeyCode::ControlLeft, KeyCode::ControlRight]);
    if !ctrl {
        let axis = |pos: [KeyCode; 2], neg: [KeyCode; 2]| {
            keys.any_pressed(pos) as i32 as f32 - keys.any_pressed(neg) as i32 as f32
        };
        let f = axis([KeyCode::KeyW, KeyCode::ArrowUp], [KeyCode::KeyS, KeyCode::ArrowDown]);
        let s = axis([KeyCode::KeyD, KeyCode::ArrowRight], [KeyCode::KeyA, KeyCode::ArrowLeft]);
        t.focus += (fwd * f + right * s) * t.view_height * 0.8 * dt;
        let q = axis([KeyCode::KeyQ, KeyCode::KeyQ], [KeyCode::KeyE, KeyCode::KeyE]);
        t.yaw += q * ORBIT_KEY_DEG_PER_S.to_radians() * dt;
    }
    r.target = r.target.clamped();
}

/// Critically damped spring step (Game Programming Gems 4, "smooth damp").
fn smooth_damp(cur: f32, target: f32, vel: &mut f32, dt: f32) -> f32 {
    let omega = 2.0 / SMOOTH_TIME;
    let x = omega * dt;
    let exp = 1.0 / (1.0 + x + 0.48 * x * x + 0.235 * x * x * x);
    let change = cur - target;
    let temp = (*vel + omega * change) * dt;
    *vel = (*vel - omega * temp) * exp;
    target + (change + temp) * exp
}

fn apply_rig(
    mut rig: ResMut<CameraRig>,
    time: Res<Time>,
    mut cam: Query<(&mut Transform, &mut Projection), With<GameCamera>>,
) {
    let dt = time.delta_secs();
    let r = &mut *rig;
    let (c, t) = (r.current, r.target);
    let mut next = c;
    if dt > 0.0 {
        let mut vf = r.vel_focus;
        next.focus = Vec3::new(
            smooth_damp(c.focus.x, t.focus.x, &mut vf.x, dt),
            smooth_damp(c.focus.y, t.focus.y, &mut vf.y, dt),
            smooth_damp(c.focus.z, t.focus.z, &mut vf.z, dt),
        );
        r.vel_focus = vf;
        next.yaw = smooth_damp(c.yaw, t.yaw, &mut r.vel_yaw, dt);
        next.view_height = smooth_damp(c.view_height, t.view_height, &mut r.vel_zoom, dt);
        if (next.focus - t.focus).length() < 1e-4 && (next.yaw - t.yaw).abs() < 1e-5 {
            next.focus = t.focus;
            next.yaw = t.yaw;
        }
        if (next.view_height - t.view_height).abs() < 1e-4 {
            next.view_height = t.view_height;
        }
    }
    if next != c {
        r.current = next;
    }
    let pose = r.current;
    for (mut tf, mut proj) in &mut cam {
        let new = pose.snapped_transform();
        if *tf != new {
            *tf = new;
        }
        if let Projection::Orthographic(o) = &mut *proj {
            let current = match o.scaling_mode {
                ScalingMode::FixedVertical { viewport_height } => Some(viewport_height),
                _ => None,
            };
            if current != Some(pose.view_height) {
                o.scaling_mode = ScalingMode::FixedVertical { viewport_height: pose.view_height };
            }
        }
    }
}

/// World-space ray under `cursor` (window logical px) for the low-res game camera.
pub fn cursor_ray(cursor: Vec2, low: &LowRes, camera: &Camera, cam_gt: &GlobalTransform) -> Option<Ray3d> {
    let p = low.window_to_low_res(cursor)?;
    camera.viewport_to_world(cam_gt, p).ok()
}

fn update_cursor_ray(
    window: Query<&Window, With<PrimaryWindow>>,
    low: Option<Res<LowRes>>,
    cam: Query<(&Camera, &GlobalTransform), With<GameCamera>>,
    mut ray: ResMut<CursorRay>,
) {
    let new = (|| {
        let cursor = window.single().ok()?.cursor_position()?;
        let (camera, gt) = cam.single().ok()?;
        cursor_ray(cursor, low.as_deref()?, camera, gt)
    })();
    if ray.0 != new {
        ray.0 = new;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clamps_pose() {
        let p = RigPose { focus: Vec3::new(99.0, 3.0, -99.0), yaw: 0.0, view_height: 1.0 }.clamped();
        assert_eq!(p.focus, Vec3::new(FOCUS_LIMIT, 3.0, -FOCUS_LIMIT));
        assert_eq!(p.view_height, MIN_VIEW_HEIGHT);
    }

    #[test]
    fn camera_looks_at_focus_with_pitch() {
        let p = RigPose::default();
        let tf = p.transform();
        let fwd = tf.forward().as_vec3();
        assert!((fwd.y + PITCH_DEG.to_radians().sin()).abs() < 1e-4);
        let (g, _) = p.ground_axes();
        assert!(Vec3::new(fwd.x, 0.0, fwd.z).normalize().dot(g) > 0.999);
    }

    #[test]
    fn snapping_moves_less_than_a_texel_and_is_stable() {
        let p = RigPose { focus: Vec3::new(1.234, 5.0, -7.89), ..default() };
        let (a, b) = (p.transform(), p.snapped_transform());
        let texel = p.view_height / LOW_RES_HEIGHT as f32;
        assert!(a.translation.distance(b.translation) <= texel * 0.75);
        assert_eq!(b.rotation, a.rotation);
        let right = b.rotation * Vec3::X;
        let r = b.translation.dot(right) / texel;
        assert!((r - r.round()).abs() < 1e-2);
    }

    #[test]
    fn smooth_damp_converges() {
        let (mut x, mut v) = (0.0, 0.0);
        for _ in 0..120 {
            x = smooth_damp(x, 10.0, &mut v, 1.0 / 60.0);
        }
        assert!((x - 10.0).abs() < 0.01);
    }
}
