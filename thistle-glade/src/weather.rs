//! Day/night clock and weather → `Environment`, plus rain/snow/lightning effects.
//!
//! Contract used by ui / shot: [`Clock`] and [`Weather`] resources (UI writes, this reads).
//! STUB systems — owned by the weather task.

use bevy::prelude::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum WeatherKind {
    #[default]
    Clear,
    Overcast,
    Rain,
    Storm,
    Fog,
    Snow,
}

impl WeatherKind {
    pub const ALL: [WeatherKind; 6] =
        [WeatherKind::Clear, WeatherKind::Overcast, WeatherKind::Rain, WeatherKind::Storm, WeatherKind::Fog, WeatherKind::Snow];
    pub fn name(self) -> &'static str {
        match self {
            WeatherKind::Clear => "clear",
            WeatherKind::Overcast => "overcast",
            WeatherKind::Rain => "rain",
            WeatherKind::Storm => "storm",
            WeatherKind::Fog => "fog",
            WeatherKind::Snow => "snow",
        }
    }
    pub fn parse(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|w| w.name().eq_ignore_ascii_case(s))
    }
}

/// Time of day.
#[derive(Resource, Clone, Debug)]
pub struct Clock {
    /// Hour 0..24.
    pub hour: f32,
    pub paused: bool,
    /// Game hours per real second (default 1/30).
    pub speed: f32,
}

impl Default for Clock {
    fn default() -> Self {
        Self { hour: 10.0, paused: false, speed: 1.0 / 30.0 }
    }
}

/// Weather target. With `auto`, the weather system picks the next state itself every few
/// minutes. Transitions blend smoothly over a few seconds (or instantly when `snap` is set,
/// which the weather system clears after applying — used by `--demo-shot`).
#[derive(Resource, Clone, Debug)]
pub struct Weather {
    pub kind: WeatherKind,
    pub auto: bool,
    /// Extra fog density added on top of the weather's own (HUD slider), 0..0.08.
    pub fog_extra: f32,
    /// Snow cover override 0..1 (`None` = follow the weather).
    pub snow_override: Option<f32>,
    pub snap: bool,
}

impl Default for Weather {
    fn default() -> Self {
        Self { kind: WeatherKind::Clear, auto: true, fog_extra: 0.0, snow_override: None, snap: false }
    }
}

pub struct WeatherPlugin;

impl Plugin for WeatherPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Clock>().init_resource::<Weather>();
    }
}
