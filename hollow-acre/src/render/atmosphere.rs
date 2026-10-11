//! [`Atmosphere`]: the one place weather/time write lighting state; render reads it
//! every frame (sun light, clear colour, toon material uniforms, outline pass).

use bevy::prelude::*;

/// Lighting state for the current frame. Colours are sRGB `Color`s; strengths are
/// relative multipliers as in DESIGN.md (1.0 = clear noon).
#[derive(Resource, Clone, Debug, PartialEq)]
pub struct Atmosphere {
    /// Unit vector pointing from the ground towards the sun (or moon at night).
    pub sun_dir: Vec3,
    pub sun_color: Color,
    /// Relative sun intensity (DESIGN "Sun intensity (rel.)" × weather multiplier).
    pub sun_illuminance: f32,
    pub ambient_color: Color,
    /// DESIGN "Ambient strength" × weather ambient multiplier.
    pub ambient_brightness: f32,
    /// Cel shadow multiply colour (`#6a6fa8` day, `#3a4270` night).
    pub shadow_tint: Color,
    /// 0..1: how far cast/self shadows go towards `shadow_tint` (weather "Shadow strength").
    pub shadow_strength: f32,
    pub sky_color: Color,
    pub fog_color: Color,
    /// Exponential distance fog density per world unit.
    pub fog_density: f32,
    /// Extra height-fog density below y = 6 wu (Mist: 0.08, otherwise 0).
    pub height_fog: f32,
    /// 0..1 fraction of the ground under cloud shadow (0 disables cloud shadows).
    pub cloud_cover: f32,
    /// World xz offset of the cloud-shadow noise; advance it along the wind.
    pub cloud_offset: Vec2,
    /// 0..1 lightning flash (adds violet ambient and sky).
    pub flash: f32,
    /// 0..1 night factor: emissive strength = mix(day, night, night).
    pub night: f32,
    /// 0..1 surface wetness (rain darkens upward faces).
    pub wetness: f32,
    /// Seconds, drives water sparkle and other shader animation.
    pub time_secs: f32,
}

fn hex(v: u32) -> Color {
    Color::srgb_u8((v >> 16) as u8, (v >> 8) as u8, v as u8)
}

/// Direction towards a light at `elevation_deg` above the horizon and `azimuth_deg`
/// (0 = +x / east, 90 = +z / south).
pub fn sun_direction(elevation_deg: f32, azimuth_deg: f32) -> Vec3 {
    let (e, a) = (elevation_deg.to_radians(), azimuth_deg.to_radians());
    Vec3::new(e.cos() * a.cos(), e.sin(), e.cos() * a.sin()).normalize()
}

impl Default for Atmosphere {
    /// Clear afternoon (~14:00).
    fn default() -> Self {
        Self::preset(AtmosPreset::Afternoon)
    }
}

/// Debug presets (F5 cycles, `--atmos NAME` picks one).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum AtmosPreset {
    #[default]
    Afternoon,
    Dusk,
    Night,
    Overcast,
    Storm,
    Mist,
}

impl AtmosPreset {
    pub const ALL: [AtmosPreset; 6] = [
        Self::Afternoon,
        Self::Dusk,
        Self::Night,
        Self::Overcast,
        Self::Storm,
        Self::Mist,
    ];

    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|p| format!("{p:?}").eq_ignore_ascii_case(name.trim()))
    }

    pub fn next(self) -> Self {
        let i = Self::ALL.iter().position(|&p| p == self).unwrap_or(0);
        Self::ALL[(i + 1) % Self::ALL.len()]
    }
}

impl Atmosphere {
    pub fn preset(p: AtmosPreset) -> Self {
        let day = Self {
            sun_dir: sun_direction(50.0, 120.0),
            sun_color: hex(0xfff4dc),
            sun_illuminance: 1.0,
            ambient_color: hex(0xa9bfd6),
            ambient_brightness: 0.45,
            shadow_tint: hex(0x6a6fa8),
            shadow_strength: 1.0,
            sky_color: hex(0xa8d8ea),
            fog_color: hex(0xa8d8ea),
            fog_density: 0.002,
            height_fog: 0.0,
            cloud_cover: 0.25,
            cloud_offset: Vec2::ZERO,
            flash: 0.0,
            night: 0.0,
            wetness: 0.0,
            time_secs: 0.0,
        };
        let mix = |a: Color, b: u32, t: f32| a.mix(&hex(b), t);
        match p {
            AtmosPreset::Afternoon => day,
            AtmosPreset::Dusk => Self {
                sun_dir: sun_direction(10.0, 200.0),
                sun_color: hex(0xffa36b),
                sun_illuminance: 0.65,
                ambient_color: hex(0xa585a6),
                ambient_brightness: 0.38,
                sky_color: hex(0xe89a7a),
                fog_color: hex(0xe89a7a),
                night: 0.55,
                ..day
            },
            AtmosPreset::Night => Self {
                sun_dir: sun_direction(40.0, 60.0),
                sun_color: hex(0x9fb3e8),
                sun_illuminance: 0.25,
                ambient_color: hex(0x2c3463),
                ambient_brightness: 0.30,
                shadow_tint: hex(0x3a4270),
                sky_color: hex(0x1d2347),
                fog_color: hex(0x1d2347),
                cloud_cover: 0.0,
                night: 1.0,
                ..day
            },
            AtmosPreset::Overcast => Self {
                sun_illuminance: 0.55,
                ambient_brightness: 0.45 * 1.15,
                shadow_strength: 0.5,
                sky_color: mix(day.sky_color, 0xb8bcc8, 0.55),
                fog_color: mix(day.sky_color, 0xb8bcc8, 0.55),
                fog_density: 0.006,
                cloud_cover: 0.6,
                ..day
            },
            AtmosPreset::Storm => Self {
                sun_illuminance: 0.35,
                shadow_strength: 0.35,
                sky_color: mix(day.sky_color, 0x4e4a6b, 0.75),
                fog_color: hex(0x55516f),
                fog_density: 0.016,
                cloud_cover: 0.0,
                wetness: 1.0,
                night: 0.3,
                ..day
            },
            AtmosPreset::Mist => Self {
                sun_illuminance: 0.5,
                ambient_brightness: 0.45 * 1.25,
                shadow_strength: 0.3,
                sky_color: mix(day.sky_color, 0xd9dde3, 0.7),
                fog_color: hex(0xdfe3e6),
                fog_density: 0.035,
                height_fog: 0.08,
                cloud_cover: 0.0,
                ..day
            },
        }
    }
}

/// Which debug preset F5 last selected.
#[derive(Resource, Default)]
pub struct DebugPreset(pub AtmosPreset);

/// F5 cycles the debug presets (keeps the running clock/cloud offset).
pub fn cycle_preset_on_f5(
    keys: Res<ButtonInput<KeyCode>>,
    mut preset: ResMut<DebugPreset>,
    mut atmos: ResMut<Atmosphere>,
) {
    if keys.just_pressed(KeyCode::F5) {
        preset.0 = preset.0.next();
        let keep = (atmos.time_secs, atmos.cloud_offset);
        *atmos = Atmosphere::preset(preset.0);
        (atmos.time_secs, atmos.cloud_offset) = keep;
        info!("atmosphere preset: {:?}", preset.0);
    }
}

/// Default clock + cloud drift so the scene moves before a weather system exists.
/// Weather systems that own the atmosphere should remove this by inserting
/// [`AtmosphereDriven`].
pub fn tick_default_atmosphere(
    time: Res<Time>,
    driven: Option<Res<AtmosphereDriven>>,
    mut atmos: ResMut<Atmosphere>,
) {
    if driven.is_some() {
        return;
    }
    let dt = time.delta_secs();
    atmos.time_secs += dt;
    atmos.cloud_offset += Vec2::new(1.0, 0.35).normalize() * 1.2 * dt;
}

/// Insert this resource when another system takes over writing `time_secs` and
/// `cloud_offset` every frame.
#[derive(Resource, Default)]
pub struct AtmosphereDriven;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preset_names_round_trip() {
        for p in AtmosPreset::ALL {
            assert_eq!(AtmosPreset::from_name(&format!("{p:?}").to_lowercase()), Some(p));
        }
        assert_eq!(AtmosPreset::from_name("nope"), None);
        assert_eq!(AtmosPreset::Mist.next(), AtmosPreset::Afternoon);
    }

    #[test]
    fn sun_dir_is_up_and_unit() {
        let d = sun_direction(55.0, 30.0);
        assert!((d.length() - 1.0).abs() < 1e-5 && d.y > 0.8);
    }
}
