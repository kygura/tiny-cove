//! Tiny Glade-style orbit camera around a focus point on the ground.

use crate::render::GameCamera;
use crate::voxel::{SIZE_X, SIZE_Z, VOXEL_SIZE};
use bevy::input::mouse::{AccumulatedMouseMotion, AccumulatedMouseScroll, MouseScrollUnit};
use bevy::picking::hover::HoverMap;
use bevy::picking::pointer::PointerId;
use bevy::prelude::*;

pub struct CameraPlugin;

pub const MIN_DIST: f32 = 8.0;
pub const MAX_DIST: f32 = 120.0;
pub const MIN_PITCH: f32 = 15.0_f32.to_radians();
pub const MAX_PITCH: f32 = 80.0_f32.to_radians();
const HALF_X: f32 = SIZE_X as f32 * VOXEL_SIZE / 2.0;
const HALF_Z: f32 = SIZE_Z as f32 * VOXEL_SIZE / 2.0;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct OrbitPose {
    /// Ground-plane focus (world x/z).
    pub focus: Vec2,
    pub yaw: f32,
    pub pitch: f32,
    pub distance: f32,
}

impl Default for OrbitPose {
    fn default() -> Self {
        Self { focus: Vec2::ZERO, yaw: 35f32.to_radians(), pitch: 35f32.to_radians(), distance: 45.0 }
    }
}

impl OrbitPose {
    pub fn clamped(mut self) -> Self {
        self.focus = self.focus.clamp(Vec2::new(-HALF_X, -HALF_Z), Vec2::new(HALF_X, HALF_Z));
        self.pitch = self.pitch.clamp(MIN_PITCH, MAX_PITCH);
        self.distance = self.distance.clamp(MIN_DIST, MAX_DIST);
        self
    }

    pub fn transform(&self) -> Transform {
        let f = Vec3::new(self.focus.x, 0.0, self.focus.y);
        let (sp, cp) = self.pitch.sin_cos();
        let offset = Vec3::new(cp * self.yaw.sin(), sp, cp * self.yaw.cos()) * self.distance;
        Transform::from_translation(f + offset).looking_at(f, Vec3::Y)
    }

    /// Unit ground vectors (forward away from the camera, right) for the current yaw.
    fn axes(&self) -> (Vec2, Vec2) {
        let (s, c) = self.yaw.sin_cos();
        (Vec2::new(-s, -c), Vec2::new(c, -s))
    }
}

/// `target` is driven by input; `current` eases toward it and drives the camera.
#[derive(Resource, Default)]
pub struct Orbit {
    pub target: OrbitPose,
    pub current: OrbitPose,
    orbiting: bool,
    panning: bool,
}

impl Orbit {
    /// Jumps straight to `pose` without easing.
    pub fn snap(&mut self, pose: OrbitPose) {
        self.target = pose.clamped();
        self.current = self.target;
    }
}

impl Plugin for CameraPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Orbit>().add_systems(Update, (orbit_input, apply_orbit).chain());
    }
}

/// True if the mouse is over a pickable UI node, e.g. a HUD button.
pub fn pointer_over_ui(hover: Option<&HoverMap>, nodes: &Query<(), With<Node>>) -> bool {
    hover
        .and_then(|h| h.get(&PointerId::Mouse))
        .is_some_and(|hits| hits.keys().any(|&e| nodes.contains(e)))
}

fn orbit_input(
    mut orbit: ResMut<Orbit>,
    mouse: Res<ButtonInput<MouseButton>>,
    keys: Res<ButtonInput<KeyCode>>,
    motion: Res<AccumulatedMouseMotion>,
    scroll: Res<AccumulatedMouseScroll>,
    time: Res<Time>,
    hover: Option<Res<HoverMap>>,
    nodes: Query<(), With<Node>>,
) {
    let over_ui = pointer_over_ui(hover.as_deref(), &nodes);
    let o = &mut *orbit;
    if mouse.just_pressed(MouseButton::Right) && !over_ui {
        o.orbiting = true;
    }
    if mouse.just_pressed(MouseButton::Middle) && !over_ui {
        o.panning = true;
    }
    o.orbiting &= mouse.pressed(MouseButton::Right);
    o.panning &= mouse.pressed(MouseButton::Middle);

    let dt = time.delta_secs();
    let t = &mut o.target;
    let (fwd, right) = t.axes();
    if o.orbiting {
        t.yaw -= motion.delta.x * 0.006;
        t.pitch += motion.delta.y * 0.004;
    }
    if o.panning {
        let k = t.distance * 0.0015;
        t.focus += (-right * motion.delta.x + fwd * motion.delta.y) * k;
    }
    if !over_ui && scroll.delta.y != 0.0 {
        let lines = match scroll.unit {
            MouseScrollUnit::Line => scroll.delta.y,
            MouseScrollUnit::Pixel => scroll.delta.y / 40.0,
        };
        t.distance *= (-lines * 0.15).exp();
    }
    let ctrl = keys.any_pressed([KeyCode::ControlLeft, KeyCode::ControlRight]);
    if !ctrl {
        let axis = |pos: [KeyCode; 2], neg: [KeyCode; 2]| {
            keys.any_pressed(pos) as i32 as f32 - keys.any_pressed(neg) as i32 as f32
        };
        let f = axis([KeyCode::KeyW, KeyCode::ArrowUp], [KeyCode::KeyS, KeyCode::ArrowDown]);
        let r = axis([KeyCode::KeyD, KeyCode::ArrowRight], [KeyCode::KeyA, KeyCode::ArrowLeft]);
        t.focus += (fwd * f + right * r) * t.distance * 0.9 * dt;
        t.yaw += axis([KeyCode::KeyQ, KeyCode::KeyQ], [KeyCode::KeyE, KeyCode::KeyE]) * 1.6 * dt;
    }
    o.target = o.target.clamped();
}

fn apply_orbit(mut orbit: ResMut<Orbit>, time: Res<Time>, mut cam: Query<&mut Transform, With<GameCamera>>) {
    let a = 1.0 - (-12.0 * time.delta_secs()).exp();
    let (c, t) = (orbit.current, orbit.target);
    let next = OrbitPose {
        focus: c.focus.lerp(t.focus, a),
        yaw: c.yaw + (t.yaw - c.yaw) * a,
        pitch: c.pitch + (t.pitch - c.pitch) * a,
        distance: (c.distance.ln() + (t.distance.ln() - c.distance.ln()) * a).exp(),
    };
    if next != c {
        orbit.current = next;
    }
    for mut tf in &mut cam {
        let new = orbit.current.transform();
        if *tf != new {
            *tf = new;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clamps_pose() {
        let p = OrbitPose { focus: Vec2::new(99.0, -99.0), yaw: 0.0, pitch: 2.0, distance: 1.0 }.clamped();
        assert_eq!(p.focus, Vec2::new(40.0, -40.0));
        assert_eq!(p.pitch, MAX_PITCH);
        assert_eq!(p.distance, MIN_DIST);
    }

    #[test]
    fn camera_above_ground_and_facing_focus() {
        let p = OrbitPose::default();
        let tf = p.transform();
        assert!(tf.translation.y > 0.0);
        assert!((tf.translation.length() - p.distance).abs() < 1e-3);
        let (fwd, _) = p.axes();
        let look = tf.forward().as_vec3();
        assert!(Vec2::new(look.x, look.z).normalize().dot(fwd) > 0.99);
    }
}
