//! Sky clock, weather schedule, the hour+weather → [`Atmosphere`] mapping (DESIGN
//! "Time of day" / "Weather"), rain streaks, ground splashes and lightning.
//! Sole writer of [`Sky`] and of the driven fields of [`Atmosphere`].

use crate::camera::CameraRig;
use crate::game::{DisplayedGrid, Sky, UiCommand, WeatherKind};
use crate::render::{Atmosphere, AtmosphereDriven};
use crate::shot::CliArgs;
use acre_core::{ORIGIN, VOXEL_SIZE};
use bevy::light::NotShadowCaster;
use bevy::prelude::*;

/// Real seconds per 24 h.
pub const DAY_SECS: f32 = 360.0;
const HOURS_PER_SEC: f32 = 24.0 / DAY_SECS;
/// Weather parameter blend time constant (seconds); ~95% done after 3x.
const BLEND_TAU: f32 = 2.0;
const RAIN_POOL: usize = 1100;
const SPLASH_POOL: usize = 64;

pub struct WeatherPlugin;

impl Plugin for WeatherPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<WeatherState>()
            .add_systems(Startup, (init_sky, setup_rain).chain())
            .add_systems(
                Update,
                (clock_input, auto_weather, drive_atmosphere, lightning, rain_update).chain(),
            );
    }
}

/// Read-only view of the blended weather for other systems (life, particles).
#[derive(Resource)]
pub struct WeatherState {
    pub params: WParams,
    /// Wind direction on the ground plane (unit).
    pub wind_dir: Vec2,
    /// 0..1 how rainy / stormy the blended weather is.
    pub rain: f32,
    pub storm: f32,
    next_change: f32,
    next_flash: f32,
    flash_frames: u32,
    flash_time: f32,
    rng: Rng,
    driven: bool,
}

impl Default for WeatherState {
    fn default() -> Self {
        Self {
            params: WParams::of(WeatherKind::Clear),
            wind_dir: Vec2::new(1.0, 0.35).normalize(),
            rain: 0.0,
            storm: 0.0,
            next_change: 180.0,
            next_flash: 10.0,
            flash_frames: u32::MAX,
            flash_time: 0.0,
            rng: Rng(0x9E37_79B9_7F4A_7C15),
            driven: false,
        }
    }
}

/// Tiny xorshift RNG (no external dependency).
#[derive(Clone, Copy, Debug)]
pub struct Rng(pub u64);

impl Rng {
    pub fn new(seed: u64) -> Self {
        Self(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1)
    }
    pub fn next_u64(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }
    /// 0..1
    pub fn f32(&mut self) -> f32 {
        (self.next_u64() >> 40) as f32 / (1u64 << 24) as f32
    }
    pub fn range(&mut self, a: f32, b: f32) -> f32 {
        a + (b - a) * self.f32()
    }
}

// ---------------------------------------------------------------------------
// Pure mapping
// ---------------------------------------------------------------------------

fn hex(v: u32) -> Vec3 {
    Vec3::new(((v >> 16) & 255) as f32, ((v >> 8) & 255) as f32, (v & 255) as f32) / 255.0
}

fn col(v: Vec3) -> Color {
    Color::srgb(v.x, v.y, v.z)
}

/// Time-of-day look (colours are sRGB triples).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Tod {
    pub sky: Vec3,
    pub sun_color: Vec3,
    pub sun_intensity: f32,
    pub ambient_color: Vec3,
    pub ambient: f32,
    pub shadow_tint: Vec3,
    /// 0 day .. 1 night (emissive factor).
    pub night: f32,
}

impl Tod {
    fn lerp(a: &Tod, b: &Tod, t: f32) -> Tod {
        Tod {
            sky: a.sky.lerp(b.sky, t),
            sun_color: a.sun_color.lerp(b.sun_color, t),
            sun_intensity: a.sun_intensity + (b.sun_intensity - a.sun_intensity) * t,
            ambient_color: a.ambient_color.lerp(b.ambient_color, t),
            ambient: a.ambient + (b.ambient - a.ambient) * t,
            shadow_tint: a.shadow_tint.lerp(b.shadow_tint, t),
            night: a.night + (b.night - a.night) * t,
        }
    }
}

fn tod_key(which: usize) -> Tod {
    match which {
        0 => Tod {
            // dawn
            sky: hex(0xf4c6a5),
            sun_color: hex(0xffc89a),
            sun_intensity: 0.7,
            ambient_color: hex(0xb59ab8),
            ambient: 0.40,
            shadow_tint: hex(0x6a6fa8).lerp(hex(0x3a4270), 0.2),
            night: 0.4,
        },
        1 => Tod {
            // day
            sky: hex(0xa8d8ea),
            sun_color: hex(0xfff4dc),
            sun_intensity: 1.0,
            ambient_color: hex(0xa9bfd6),
            ambient: 0.45,
            shadow_tint: hex(0x6a6fa8),
            night: 0.0,
        },
        2 => Tod {
            // dusk
            sky: hex(0xe89a7a),
            sun_color: hex(0xffa36b),
            sun_intensity: 0.65,
            ambient_color: hex(0xa585a6),
            ambient: 0.38,
            shadow_tint: hex(0x6a6fa8).lerp(hex(0x3a4270), 0.25),
            night: 0.55,
        },
        _ => Tod {
            // night
            sky: hex(0x1d2347),
            sun_color: hex(0x9fb3e8),
            sun_intensity: 0.25,
            ambient_color: hex(0x2c3463),
            ambient: 0.30,
            shadow_tint: hex(0x3a4270),
            night: 1.0,
        },
    }
}

/// Phase keyframes: (hour, key). Night 20-05, dawn 05-08, day 08-17, dusk 17-20 with a
/// one-hour linear blend at the edges of each phase.
const TOD_KEYS: [(f32, usize); 9] = [
    (0.0, 3),
    (5.0, 3),
    (6.0, 0),
    (7.0, 0),
    (8.0, 1),
    (17.0, 1),
    (18.0, 2),
    (19.0, 2),
    (20.0, 3),
];

pub fn tod_at(hour: f32) -> Tod {
    let h = hour.rem_euclid(24.0);
    for w in TOD_KEYS.windows(2) {
        let ((h0, k0), (h1, k1)) = (w[0], w[1]);
        if h >= h0 && h <= h1 {
            let t = (h - h0) / (h1 - h0);
            return Tod::lerp(&tod_key(k0), &tod_key(k1), t);
        }
    }
    tod_key(3)
}

/// Unit vector towards the sun (or moon at night). Sun rises ~05:40, peaks 55° near
/// 12:30 and sets ~19:20; the moon (40°) takes over through dusk and dawn.
pub fn sun_dir_at(hour: f32) -> Vec3 {
    let h = hour.rem_euclid(24.0);
    let (rise, set) = (5.7, 19.3);
    let sun = {
        let t = ((h - rise) / (set - rise)).clamp(0.0, 1.0);
        let elev = (55.0 * (std::f32::consts::PI * t).sin()).max(6.0);
        let az = -35.0 - 200.0 * (t - 0.54);
        crate::render::atmosphere::sun_direction(elev, az)
    };
    let moon = crate::render::atmosphere::sun_direction(40.0, 60.0 + (h * 15.0).sin() * 8.0);
    // Weight of the moon: 0 by day, 1 at night, blended through 18.5-20 and 4.5-6.
    let m = if h >= 12.0 {
        ((h - 18.5) / 1.5).clamp(0.0, 1.0)
    } else {
        ((6.0 - h) / 1.5).clamp(0.0, 1.0)
    };
    let m = m * m * (3.0 - 2.0 * m);
    sun.lerp(moon, m).normalize()
}

/// Weather-dependent targets, blended smoothly between kinds.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WParams {
    pub tint: Vec3,
    pub tint_amount: f32,
    /// Fog colour override mix (0 = use sky, 1 = `fog`).
    pub fog_mix: f32,
    pub fog: Vec3,
    pub fog_density: f32,
    pub height_fog: f32,
    pub sun_mul: f32,
    pub shadow_strength: f32,
    pub ambient_mul: f32,
    pub cloud_cover: f32,
    /// Minimum night factor (storm light is dim).
    pub night_min: f32,
    /// 0..1 amount of rain streaks, storm adds on top via `storm`.
    pub rain: f32,
    pub storm: f32,
    /// Wind speed in wu/s (cloud drift).
    pub wind: f32,
}

impl WParams {
    pub fn of(k: WeatherKind) -> Self {
        let base = Self {
            tint: Vec3::ZERO,
            tint_amount: 0.0,
            fog_mix: 0.0,
            fog: Vec3::ZERO,
            fog_density: 0.002,
            height_fog: 0.0,
            sun_mul: 1.0,
            shadow_strength: 1.0,
            ambient_mul: 1.0,
            cloud_cover: 0.25,
            night_min: 0.0,
            rain: 0.0,
            storm: 0.0,
            wind: 1.2,
        };
        match k {
            WeatherKind::Clear => base,
            WeatherKind::Overcast => Self {
                tint: hex(0xb8bcc8),
                tint_amount: 0.55,
                fog_density: 0.006,
                sun_mul: 0.55,
                shadow_strength: 0.5,
                ambient_mul: 1.15,
                cloud_cover: 0.6,
                wind: 1.8,
                ..base
            },
            WeatherKind::Rain => Self {
                tint: hex(0x8a93a8),
                tint_amount: 0.65,
                fog_mix: 1.0,
                fog: hex(0x8f98ab),
                fog_density: 0.012,
                sun_mul: 0.45,
                shadow_strength: 0.4,
                ambient_mul: 1.1,
                cloud_cover: 0.0,
                night_min: 0.1,
                rain: 1.0,
                wind: 3.0,
                ..base
            },
            WeatherKind::Storm => Self {
                tint: hex(0x4e4a6b),
                tint_amount: 0.75,
                fog_mix: 1.0,
                fog: hex(0x55516f),
                fog_density: 0.016,
                sun_mul: 0.35,
                shadow_strength: 0.35,
                ambient_mul: 1.0,
                cloud_cover: 0.0,
                night_min: 0.3,
                rain: 1.0,
                storm: 1.0,
                wind: 7.0,
                ..base
            },
            WeatherKind::Mist => Self {
                tint: hex(0xd9dde3),
                tint_amount: 0.7,
                fog_mix: 1.0,
                fog: hex(0xdfe3e6),
                fog_density: 0.035,
                height_fog: 0.08,
                sun_mul: 0.5,
                shadow_strength: 0.3,
                ambient_mul: 1.25,
                cloud_cover: 0.0,
                wind: 0.6,
                ..base
            },
        }
    }

    pub fn lerp(&self, o: &Self, t: f32) -> Self {
        let f = |a: f32, b: f32| a + (b - a) * t;
        Self {
            tint: self.tint.lerp(o.tint, t),
            tint_amount: f(self.tint_amount, o.tint_amount),
            fog_mix: f(self.fog_mix, o.fog_mix),
            fog: self.fog.lerp(o.fog, t),
            fog_density: f(self.fog_density, o.fog_density),
            height_fog: f(self.height_fog, o.height_fog),
            sun_mul: f(self.sun_mul, o.sun_mul),
            shadow_strength: f(self.shadow_strength, o.shadow_strength),
            ambient_mul: f(self.ambient_mul, o.ambient_mul),
            cloud_cover: f(self.cloud_cover, o.cloud_cover),
            night_min: f(self.night_min, o.night_min),
            rain: f(self.rain, o.rain),
            storm: f(self.storm, o.storm),
            wind: f(self.wind, o.wind),
        }
    }
}

/// Combines time of day and (blended) weather into the frame's lighting. Leaves
/// `flash`, `wetness`, `cloud_offset` and `time_secs` untouched.
pub fn compose(hour: f32, w: &WParams, out: &mut Atmosphere) {
    let t = tod_at(hour);
    let sky = t.sky.lerp(w.tint, w.tint_amount);
    let night = t.night.max(w.night_min);
    let fog_w = sky.lerp(w.fog, w.fog_mix);
    // At night fog leans towards the night sky.
    let fog = fog_w.lerp(hex(0x1d2347), 0.7 * t.night);
    out.sun_dir = sun_dir_at(hour);
    out.sun_color = col(t.sun_color);
    out.sun_illuminance = t.sun_intensity * w.sun_mul;
    out.ambient_color = col(t.ambient_color);
    out.ambient_brightness = t.ambient * w.ambient_mul;
    out.shadow_tint = col(t.shadow_tint);
    out.shadow_strength = w.shadow_strength;
    out.sky_color = col(sky.lerp(hex(0x1d2347), 0.0));
    out.fog_color = col(fog);
    out.fog_density = w.fog_density;
    out.height_fog = w.height_fog;
    out.cloud_cover = w.cloud_cover * (1.0 - t.night).clamp(0.0, 1.0);
    out.night = night;
}

/// Weighted pick of the next weather, never the current one. `r` in 0..1.
pub fn pick_weather(current: WeatherKind, r: f32) -> WeatherKind {
    let weight = |k: WeatherKind| match k {
        WeatherKind::Clear => 4.0,
        WeatherKind::Overcast => 3.0,
        WeatherKind::Rain => 2.0,
        WeatherKind::Storm => 1.0,
        WeatherKind::Mist => 1.5,
    };
    let total: f32 = WeatherKind::ALL.iter().filter(|&&k| k != current).map(|&k| weight(k)).sum();
    let mut x = r.clamp(0.0, 0.9999) * total;
    for k in WeatherKind::ALL {
        if k == current {
            continue;
        }
        x -= weight(k);
        if x < 0.0 {
            return k;
        }
    }
    WeatherKind::Clear
}

/// Lightning flash level for `frames` frames since the strike and `t` seconds since
/// the strike: 2 bright frames, 3 dim, then a 0.3 s decay.
pub fn flash_level(frames: u32, since_dim: f32) -> f32 {
    match frames {
        0..=1 => 1.0,
        2..=4 => 0.5,
        _ => (0.5 * (1.0 - since_dim / 0.3)).max(0.0),
    }
}

// ---------------------------------------------------------------------------
// Systems
// ---------------------------------------------------------------------------

fn init_sky(
    mut commands: Commands,
    args: Option<Res<CliArgs>>,
    mut sky: ResMut<Sky>,
    mut state: ResMut<WeatherState>,
    mut atmos: ResMut<Atmosphere>,
) {
    let mut debug_atmos = false;
    if let Some(a) = args.as_deref() {
        if let Some(h) = a.time {
            sky.hour = h.rem_euclid(24.0);
        }
        if let Some(w) = a.weather.as_deref().and_then(WeatherKind::from_name) {
            sky.weather = w;
        }
        if a.demo_shot.is_some() {
            sky.paused = true;
        }
        state.rng = Rng::new(a.seed.unwrap_or(7) ^ 0xA5A5);
        debug_atmos = a.atmos.is_some();
    }
    state.params = WParams::of(sky.weather);
    state.rain = state.params.rain;
    state.storm = state.params.storm;
    state.next_change = state.rng.range(120.0, 240.0);
    state.next_flash = state.rng.range(2.0, 6.0);
    if debug_atmos {
        return; // `--atmos` pins a debug preset; leave the default ticker running.
    }
    state.driven = true;
    commands.insert_resource(AtmosphereDriven);
    compose(sky.hour, &state.params, &mut atmos);
    atmos.wetness = state.rain.max(0.0);
}

fn clock_input(
    time: Res<Time>,
    keys: Res<ButtonInput<KeyCode>>,
    mut cmds: MessageReader<UiCommand>,
    mut sky: ResMut<Sky>,
    mut hold: Local<f32>,
) {
    let dt = time.delta_secs();
    if keys.just_pressed(KeyCode::KeyT) {
        sky.paused = !sky.paused;
    }
    for c in cmds.read() {
        if matches!(c, UiCommand::TogglePause) {
            sky.paused = !sky.paused;
        }
    }
    let dir = keys.pressed(KeyCode::BracketRight) as i32 - keys.pressed(KeyCode::BracketLeft) as i32;
    if keys.just_pressed(KeyCode::BracketRight) {
        sky.hour += 1.0;
        *hold = 0.0;
    } else if keys.just_pressed(KeyCode::BracketLeft) {
        sky.hour -= 1.0;
        *hold = 0.0;
    } else if dir != 0 {
        *hold += dt;
        if *hold > 0.35 {
            sky.hour += dir as f32 * 4.0 * dt;
        }
    }
    if !sky.paused {
        sky.hour += HOURS_PER_SEC * dt;
    }
    sky.hour = sky.hour.rem_euclid(24.0);
}

fn auto_weather(
    time: Res<Time>,
    args: Option<Res<CliArgs>>,
    mut cmds: MessageReader<UiCommand>,
    mut sky: ResMut<Sky>,
    mut state: ResMut<WeatherState>,
) {
    let demo = args.is_some_and(|a| a.demo_shot.is_some());
    let mut set = None;
    for c in cmds.read() {
        if let UiCommand::SetWeather(w) = c {
            set = Some(*w);
        }
    }
    if let Some(w) = set {
        sky.weather = w;
        state.next_change = state.rng.range(120.0, 240.0);
        return;
    }
    if demo {
        return;
    }
    state.next_change -= time.delta_secs();
    if state.next_change <= 0.0 {
        let r = state.rng.f32();
        sky.weather = pick_weather(sky.weather, r);
        state.next_change = state.rng.range(120.0, 240.0);
    }
}

fn drive_atmosphere(
    time: Res<Time>,
    sky: Res<Sky>,
    mut state: ResMut<WeatherState>,
    mut atmos: ResMut<Atmosphere>,
) {
    if !state.driven {
        return;
    }
    let dt = time.delta_secs();
    let target = WParams::of(sky.weather);
    let k = 1.0 - (-dt / BLEND_TAU).exp();
    let cur = state.params;
    state.params = cur.lerp(&target, k);
    state.rain = state.params.rain;
    state.storm = state.params.storm;

    let flash = atmos.flash;
    let wet = atmos.wetness;
    compose(sky.hour, &state.params, &mut atmos);
    atmos.flash = flash;
    // Wetness rises in rain, dries slowly.
    atmos.wetness = if state.rain > 0.5 {
        (wet + dt / 8.0).min(1.0)
    } else {
        (wet - dt / 90.0).max(0.0)
    };
    atmos.time_secs += dt;
    let wind = state.wind_dir * state.params.wind;
    atmos.cloud_offset += wind * dt;
}

fn lightning(time: Res<Time>, mut state: ResMut<WeatherState>, mut atmos: ResMut<Atmosphere>) {
    if !state.driven {
        return;
    }
    let dt = time.delta_secs();
    let sky_storm = state.storm > 0.6;
    if sky_storm {
        state.next_flash -= dt;
        if state.next_flash <= 0.0 {
            state.next_flash = state.rng.range(8.0, 25.0);
            state.flash_frames = 0;
            state.flash_time = 0.0;
        }
    }
    if state.flash_frames != u32::MAX {
        let lvl = flash_level(state.flash_frames, state.flash_time);
        atmos.flash = lvl;
        if state.flash_frames >= 5 {
            state.flash_time += dt;
        }
        state.flash_frames += 1;
        if state.flash_frames > 5 && lvl <= 0.0 {
            state.flash_frames = u32::MAX;
            atmos.flash = 0.0;
        }
    } else {
        atmos.flash = 0.0;
    }
}

// ---------------------------------------------------------------------------
// Rain
// ---------------------------------------------------------------------------

#[derive(Resource)]
struct RainAssets {
    streak: Handle<Mesh>,
    splash: Handle<Mesh>,
    rain_mat: Handle<StandardMaterial>,
}

#[derive(Component)]
struct RainDrop {
    idx: usize,
    seed: f32,
}

#[derive(Component)]
struct Splash {
    age: f32,
}

fn setup_rain(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut mats: ResMut<Assets<StandardMaterial>>,
) {
    // Blended unlit materials skip the depth/normal prepass, so the outline pass
    // leaves the streaks alone.
    let unlit = |c: Color| StandardMaterial {
        base_color: c,
        unlit: true,
        alpha_mode: AlphaMode::Blend,
        ..default()
    };
    let rain_mat = mats.add(unlit(Color::srgba_u8(0xc4, 0xd4, 0xea, 175)));
    let splash_mat = mats.add(unlit(Color::srgba_u8(0xdf, 0xea, 0xf5, 220)));
    let streak = meshes.add(Cuboid::new(0.13, 1.0, 0.13));
    let splash = meshes.add(Cuboid::new(0.2, 0.1, 0.2));
    let mut rng = Rng::new(99);
    for idx in 0..RAIN_POOL {
        commands.spawn((
            RainDrop { idx, seed: rng.f32() },
            Mesh3d(streak.clone()),
            MeshMaterial3d(rain_mat.clone()),
            Transform::from_translation(Vec3::new(0.0, -1000.0, 0.0)),
            Visibility::Hidden,
            NotShadowCaster,
        ));
    }
    for _ in 0..SPLASH_POOL {
        commands.spawn((
            Splash { age: 1.0 },
            Mesh3d(splash.clone()),
            MeshMaterial3d(splash_mat.clone()),
            Transform::from_translation(Vec3::new(0.0, -1000.0, 0.0)),
            Visibility::Hidden,
            NotShadowCaster,
        ));
    }
    commands.insert_resource(RainAssets { streak, splash, rain_mat });
}

/// Ground height (world units) of the top surface at world (x, z), `None` off-map.
fn surface_y(grid: &acre_core::VoxelGrid, x: f32, z: f32) -> Option<f32> {
    let ix = ((x - ORIGIN) / VOXEL_SIZE).floor() as i32;
    let iz = ((z - ORIGIN) / VOXEL_SIZE).floor() as i32;
    if !acre_core::VoxelGrid::in_bounds(ix, 0, iz) {
        return None;
    }
    let t = grid.top(ix, iz);
    (t >= 0).then(|| (t + 1) as f32 * VOXEL_SIZE)
}

#[allow(clippy::too_many_arguments)]
fn rain_update(
    time: Res<Time>,
    state: Res<WeatherState>,
    atmos: Res<Atmosphere>,
    rig: Res<CameraRig>,
    grid: Res<DisplayedGrid>,
    assets: Option<Res<RainAssets>>,
    mut mats: ResMut<Assets<StandardMaterial>>,
    mut drops: Query<(&mut RainDrop, &mut Transform, &mut Visibility), Without<Splash>>,
    mut splashes: Query<(&mut Splash, &mut Transform, &mut Visibility), Without<RainDrop>>,
    mut cursor: Local<usize>,
    mut frame: Local<u32>,
    mut rng: Local<Option<Rng>>,
) {
    let Some(assets) = assets else { return };
    let rng = rng.get_or_insert_with(|| Rng::new(1234));
    let dt = time.delta_secs().min(0.1);
    *frame += 1;
    let amount = state.rain.clamp(0.0, 1.0);
    let storm = state.storm.clamp(0.0, 1.0);
    // Active streaks: rain 600, storm 1100 (DESIGN).
    let active = if amount < 0.02 { 0 } else { ((600.0 + 500.0 * storm) * amount) as usize };

    // Night colour for the streaks.
    if let Some(mut m) = mats.get_mut(&assets.rain_mat) {
        let day = hex(0xc4d4ea);
        let nite = hex(0x7d8cc0);
        let c = day.lerp(nite, atmos.night);
        m.base_color = Color::srgba(c.x, c.y, c.z, 0.69);
    }

    let fall = 24.0 + 6.0 * storm;
    let slant = (12.0 + 18.0 * storm).to_radians();
    let horiz = fall * slant.tan();
    let wind = state.wind_dir;
    let vel = Vec3::new(wind.x * horiz, -fall, wind.y * horiz);
    let rot = Quat::from_rotation_arc(Vec3::Y, -vel.normalize());
    let len = 0.75 + 0.45 * storm;

    let focus = rig.current.focus;
    let radius = rig.current.view_height * 1.05;
    let top = focus.y + rig.current.view_height * 0.8;
    let bottom = focus.y - 4.0;

    let mut spawn_splash: Vec<Vec3> = Vec::new();
    for (mut d, mut tf, mut vis) in &mut drops {
        if d.idx >= active {
            if *vis != Visibility::Hidden {
                *vis = Visibility::Hidden;
            }
            continue;
        }
        let respawn = |rng: &mut Rng| {
            Vec3::new(
                focus.x + rng.range(-radius, radius),
                rng.range(bottom, top),
                focus.z + rng.range(-radius, radius),
            )
        };
        if *vis == Visibility::Hidden {
            tf.translation = respawn(rng);
            *vis = Visibility::Inherited;
            d.seed = rng.f32();
        }
        tf.translation += vel * dt * (0.9 + 0.2 * d.seed);
        let p = tf.translation;
        // Wrap if outside the box around the focus (camera panned) or hit the ground.
        let mut hit = false;
        let mut gy = bottom;
        if p.y < focus.y + 7.0 {
            if let Some(y) = surface_y(&grid.grid, p.x, p.z) {
                gy = gy.max(y);
                if p.y <= y {
                    hit = true;
                }
            }
        }
        let outside = (p.x - focus.x).abs() > radius || (p.z - focus.z).abs() > radius;
        if hit || p.y < gy || outside || p.y < bottom {
            if hit && spawn_splash.len() < 6 && rng.f32() < 0.5 {
                spawn_splash.push(Vec3::new(p.x, gy + 0.05, p.z));
            }
            // Re-enter from the top, upwind of the focus.
            tf.translation = Vec3::new(
                focus.x + rng.range(-radius, radius),
                top + rng.range(0.0, 6.0),
                focus.z + rng.range(-radius, radius),
            );
            d.seed = rng.f32();
        }
        tf.rotation = rot;
        tf.scale = Vec3::new(1.0, len, 1.0);
    }

    // Splashes: pooled, 0.1 s.
    let mut slots: Vec<usize> = Vec::new();
    for (i, (mut s, mut tf, mut vis)) in splashes.iter_mut().enumerate() {
        s.age += dt;
        if s.age >= 0.12 {
            if *vis != Visibility::Hidden {
                *vis = Visibility::Hidden;
            }
            slots.push(i);
        } else {
            tf.scale = Vec3::splat(1.0);
        }
    }
    for (n, pos) in spawn_splash.into_iter().enumerate() {
        let Some(&slot) = slots.get((n + *cursor) % slots.len().max(1)) else { break };
        if let Some((mut s, mut tf, mut vis)) = splashes.iter_mut().nth(slot) {
            s.age = 0.0;
            tf.translation = pos;
            *vis = Visibility::Inherited;
        }
    }
    *cursor = cursor.wrapping_add(1);
    let _ = (&assets.streak, &assets.splash);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sun_dir_always_unit_and_up() {
        for i in 0..240 {
            let h = i as f32 * 0.1;
            let d = sun_dir_at(h);
            assert!((d.length() - 1.0).abs() < 1e-4);
            assert!(d.y > 0.05, "hour {h} dir {d:?}");
        }
        // Noon is higher than early morning.
        assert!(sun_dir_at(12.5).y > sun_dir_at(6.5).y);
        assert!(sun_dir_at(12.5).y > 0.7);
    }

    #[test]
    fn tod_matches_design_keys() {
        assert_eq!(tod_at(13.0).sun_intensity, 1.0);
        assert_eq!(tod_at(0.0).sun_intensity, 0.25);
        assert!((tod_at(6.5).sun_intensity - 0.7).abs() < 1e-5);
        assert!((tod_at(18.5).sun_intensity - 0.65).abs() < 1e-5);
        assert_eq!(tod_at(12.0).night, 0.0);
        assert_eq!(tod_at(23.0).night, 1.0);
        // Wraps.
        assert_eq!(tod_at(24.0), tod_at(0.0));
    }

    #[test]
    fn blend_endpoints() {
        let a = WParams::of(WeatherKind::Clear);
        let b = WParams::of(WeatherKind::Storm);
        assert_eq!(a.lerp(&b, 0.0), a);
        let e = a.lerp(&b, 1.0);
        assert!((e.sun_mul - b.sun_mul).abs() < 1e-5 && (e.fog_density - b.fog_density).abs() < 1e-6);
        assert!((e.tint - b.tint).length() < 1e-5);
        let m = a.lerp(&b, 0.5);
        assert!((m.fog_density - 0.009).abs() < 1e-6);
    }

    #[test]
    fn compose_storm_noon_is_dim_and_foggy() {
        let mut a = Atmosphere::default();
        compose(13.0, &WParams::of(WeatherKind::Storm), &mut a);
        assert!((a.sun_illuminance - 0.35).abs() < 1e-5);
        assert!((a.fog_density - 0.016).abs() < 1e-6);
        assert!(a.night >= 0.3);
        let mut c = Atmosphere::default();
        compose(13.0, &WParams::of(WeatherKind::Clear), &mut c);
        assert!((c.sun_illuminance - 1.0).abs() < 1e-5 && c.cloud_cover > 0.2);
    }

    #[test]
    fn schedule_never_repeats_and_covers_all() {
        let mut seen = std::collections::HashSet::new();
        for i in 0..100 {
            let r = i as f32 / 100.0;
            let n = pick_weather(WeatherKind::Rain, r);
            assert_ne!(n, WeatherKind::Rain);
            seen.insert(n as u8);
        }
        assert_eq!(seen.len(), 4);
        // Weighted towards clear/overcast.
        let clear = (0..100).filter(|&i| pick_weather(WeatherKind::Mist, i as f32 / 100.0) == WeatherKind::Clear).count();
        let storm = (0..100).filter(|&i| pick_weather(WeatherKind::Mist, i as f32 / 100.0) == WeatherKind::Storm).count();
        assert!(clear > storm);
    }

    #[test]
    fn flash_shape() {
        assert_eq!(flash_level(0, 0.0), 1.0);
        assert_eq!(flash_level(1, 0.0), 1.0);
        assert_eq!(flash_level(2, 0.0), 0.5);
        assert_eq!(flash_level(4, 0.0), 0.5);
        assert!(flash_level(5, 0.15) < 0.5 && flash_level(5, 0.3) <= 0.0);
    }

    #[test]
    fn rng_is_in_range() {
        let mut r = Rng::new(3);
        for _ in 0..1000 {
            let x = r.f32();
            assert!((0.0..1.0).contains(&x));
        }
    }
}
