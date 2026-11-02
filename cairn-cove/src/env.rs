//! Shared lighting/atmosphere state. Written by `weather` (time of day + weather),
//! read by `render` (sun light, material uniforms, sky, fog) and `life`.

use bevy::prelude::*;
use cairn_cove_core::{SEA_LEVEL, TIDE_RANGE};

/// Current atmosphere. All colours linear RGB. Default = clear noon.
#[derive(Resource, Clone, Debug, PartialEq)]
pub struct Environment {
    /// Unit vector pointing from the ground towards the sun (or moon at night).
    pub sun_dir: Vec3,
    /// Direct light colour × intensity (cel bands use it as the lit colour).
    pub sun_color: Vec3,
    /// Ambient light from the sky (upward-facing surfaces).
    pub ambient_sky: Vec3,
    /// Ambient bounce from the ground (downward-facing surfaces).
    pub ambient_ground: Vec3,
    /// Tint applied in shadowed bands (cool blue-violet by day).
    pub shadow_tint: Vec3,
    /// Sky gradient.
    pub sky_top: Vec3,
    pub sky_horizon: Vec3,
    /// Distance fog colour and density (0 = none, ~0.02 = light, ~0.08 = thick).
    pub fog_color: Vec3,
    pub fog_density: f32,
    /// 0..1: darkens ground/stone, adds specular glints, puddles.
    pub wetness: f32,
    /// 0..1: white cover on up-facing surfaces.
    pub snow: f32,
    /// 0..1: how much windows and lanterns glow (night, storms).
    pub night: f32,
    /// 0..1: lightning flash intensity this frame.
    pub flash: f32,
    /// Wind vector on the ground plane (m/s); sways foliage, slants rain.
    pub wind: Vec2,
    /// Hour of day 0..24 (for HUD).
    pub hour: f32,
}

impl Default for Environment {
    fn default() -> Self {
        Self {
            sun_dir: Vec3::new(-0.45, 0.75, 0.35).normalize(),
            sun_color: Vec3::new(1.0, 0.94, 0.82) * 2.2,
            ambient_sky: Vec3::new(0.42, 0.50, 0.62),
            ambient_ground: Vec3::new(0.30, 0.28, 0.20),
            shadow_tint: Vec3::new(0.76, 0.74, 0.94),
            sky_top: Vec3::new(0.25, 0.48, 0.85),
            sky_horizon: Vec3::new(0.75, 0.85, 0.95),
            fog_color: Vec3::new(0.75, 0.85, 0.95),
            fog_density: 0.006,
            wetness: 0.0,
            snow: 0.0,
            night: 0.0,
            flash: 0.0,
            wind: Vec2::new(1.0, 0.3),
            hour: 13.0,
        }
    }
}

/// Shared wind. `dir` is a unit vector on the ground plane (x, z) the wind blows toward;
/// `strength` is in m/s (calm ≈ 1, breezy ≈ 3, storm ≈ 9).
#[derive(Resource, Clone, Copy, Debug, PartialEq)]
pub struct Wind {
    pub dir: Vec2,
    pub strength: f32,
}

impl Default for Wind {
    fn default() -> Self {
        Self { dir: Vec2::new(0.96, 0.29), strength: 1.2 }
    }
}

impl Wind {
    /// Wind velocity on the ground plane (m/s).
    pub fn vector(&self) -> Vec2 {
        self.dir * self.strength
    }
    /// Wind velocity as a world vector (y = 0).
    pub fn vector3(&self) -> Vec3 {
        Vec3::new(self.dir.x, 0.0, self.dir.y) * self.strength
    }
}

/// Current tide (render-side; core logic always uses `SEA_LEVEL`).
#[derive(Resource, Clone, Copy, Debug, PartialEq)]
pub struct Tide {
    /// Sea surface height in metres: `SEA_LEVEL ± TIDE_RANGE`.
    pub level: f32,
    /// True while the water is going out.
    pub falling: bool,
}

impl Default for Tide {
    fn default() -> Self {
        Self::at(12.0)
    }
}

impl Tide {
    /// The tide at `hour` (0..24): two full cycles per day, high water at 03:00 and 15:00,
    /// low water at 09:00 and 21:00.
    pub fn at(hour: f32) -> Self {
        let phase = (hour - 3.0) / 12.0 * std::f32::consts::TAU;
        Self { level: SEA_LEVEL + TIDE_RANGE * phase.cos(), falling: phase.sin() > 0.0 }
    }
}
