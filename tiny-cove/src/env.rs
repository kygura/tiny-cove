//! Shared lighting/atmosphere state. Written by `weather` (time of day + weather),
//! read by `render` (sun light, material uniforms, sky, fog, sea), `life` and `world`.
//!
//! - [`Environment`]: the current light rig (DESIGN A4) + weather effects.
//! - [`Wind`]: shared wind (direction + strength). Sways leaves and fibres in the vertex
//!   shader; drives smoke, pennants, rain slant and wave direction.
//! - [`Tide`]: the current sea level (mean sea level ± `TIDE_RANGE`, two tides per day).

use bevy::prelude::*;
use cairn_cove_core::{SEA_LEVEL, TIDE_RANGE, palette};

/// Current atmosphere. Default = clear noon.
///
/// Colour spaces: the cel rig (`sun_color`, `fill_color`, `shadow_tint`, `rim_color`) is in
/// **sRGB multipliers** because the band maths run in sRGB (DESIGN A3); every other colour
/// (sky, fog, sea, ambient) is linear RGB.
#[derive(Resource, Clone, Debug, PartialEq)]
pub struct Environment {
    /// Unit vector pointing from the ground towards the sun (or moon at night).
    pub sun_dir: Vec3,
    /// Direct light colour × intensity (sRGB multiplier; noon = 1,1,1).
    pub sun_color: Vec3,
    /// Fill (shadow-side) light colour × intensity (sRGB multiplier).
    pub fill_color: Vec3,
    /// Ambient light from the sky (linear; particles).
    pub ambient_sky: Vec3,
    /// Ambient bounce from the ground (linear; particles).
    pub ambient_ground: Vec3,
    /// Hue the mid/shadow bands rotate toward (sRGB, 0..1). Never grey.
    pub shadow_tint: Vec3,
    /// Rim light colour (sRGB, 0..1; zero = off).
    pub rim_color: Vec3,
    /// Pre-rotation of the lit band toward the tint, degrees (dusk 10, night 30).
    pub lit_rot: f32,
    /// Sky gradient: zenith, mid (55 %), horizon (linear).
    pub sky_top: Vec3,
    pub sky_mid: Vec3,
    pub sky_horizon: Vec3,
    /// Sun / moon disc colour (linear).
    pub disc_color: Vec3,
    /// 1-px keyline colour (linear).
    pub outline: Vec3,
    /// Distance fog colour and density (0 = none, ~0.02 = light, ~0.08 = thick).
    pub fog_color: Vec3,
    pub fog_density: f32,
    /// Sea fog: 0..1 strength of the low height-fog bank and its top (m above sea level).
    pub sea_fog: f32,
    pub fog_top: f32,
    /// Sea bands (linear): shallow, mid, deep, abyss, foam, glint.
    pub sea: [Vec3; 6],
    /// 0..1 storm amount (whitecaps, swell, doubled foam).
    pub storm: f32,
    /// 0..1: darkens ground/stone, adds glints and puddles.
    pub wetness: f32,
    /// 0..1: snow ramp on up-facing surfaces.
    pub snow: f32,
    /// 0..1: how much windows, glow and fires shine (night, storms, fog).
    pub night: f32,
    /// 0..1: how dark the sky is (stars, moon disc).
    pub dark: f32,
    /// 0..1: lightning flash this frame (2 frames, no fade).
    pub flash: f32,
    /// Wind vector on the ground plane (m/s): `Wind::dir * Wind::strength`.
    pub wind: Vec2,
    /// Current sea surface height (m), see [`Tide`].
    pub tide: f32,
    /// True while the tide is falling (wet-sand glisten).
    pub tide_falling: bool,
    /// Hour of day 0..24 (for HUD).
    pub hour: f32,
}

impl Default for Environment {
    fn default() -> Self {
        let s = palette::hex_to_unit;
        Self {
            sun_dir: Vec3::new(-0.45, 0.75, 0.35).normalize(),
            sun_color: Vec3::ONE,
            fill_color: Vec3::ONE,
            ambient_sky: palette::srgb(0x8CE6FF),
            ambient_ground: palette::srgb(0xF7D68C) * 0.5,
            shadow_tint: s(0x4A3AAD),
            rim_color: s(0xCEF7FF),
            lit_rot: 0.0,
            sky_top: palette::srgb(0x1973F7),
            sky_mid: palette::srgb(0x42ADFF),
            sky_horizon: palette::srgb(0x8CE6FF),
            disc_color: palette::srgb(0xFFFFDE),
            outline: palette::outline_day(),
            fog_color: palette::srgb(0xB5EFFF),
            fog_density: 0.006,
            sea_fog: 0.0,
            fog_top: 3.0,
            sea: [palette::sea_shallow(), palette::sea_mid(), palette::sea_deep(), palette::sea_abyss(), palette::foam(), palette::glint()],
            storm: 0.0,
            wetness: 0.0,
            snow: 0.0,
            night: 0.0,
            dark: 0.0,
            flash: 0.0,
            wind: Vec2::new(1.0, 0.3),
            tide: SEA_LEVEL,
            tide_falling: false,
            hour: 12.0,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tide_twice_a_day() {
        let hi = Tide::at(3.0).level;
        assert!((hi - (SEA_LEVEL + TIDE_RANGE)).abs() < 1e-5);
        assert!((Tide::at(15.0).level - hi).abs() < 1e-5);
        assert!((Tide::at(9.0).level - (SEA_LEVEL - TIDE_RANGE)).abs() < 1e-5);
        assert!(Tide::at(6.0).falling && !Tide::at(12.0).falling);
        for h in 0..96 {
            let t = Tide::at(h as f32 * 0.25);
            assert!(t.level.is_finite() && (t.level - SEA_LEVEL).abs() <= TIDE_RANGE + 1e-5);
        }
    }

    #[test]
    fn wind_vector() {
        let w = Wind { dir: Vec2::X, strength: 3.0 };
        assert_eq!(w.vector(), Vec2::new(3.0, 0.0));
        assert_eq!(w.vector3(), Vec3::new(3.0, 0.0, 0.0));
    }
}
