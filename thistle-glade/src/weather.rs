//! Day/night clock and weather → `Environment`, plus rain/snow/lightning effects.
//!
//! Contract used by ui / shot: [`Clock`] and [`Weather`] resources (UI writes, this reads).
//!
//! CLI (for screenshots): `--weather NAME --time H [--flash]`. With `--render-test` the clock is
//! paused and weather snapped; without those flags the render test keeps its own preset.

use crate::env::Environment;
use crate::render::{GameCamera, GladeAssets, Particle, ParticleKind, particle_mesh, write_particle_mesh};
use bevy::{camera::visibility::NoFrustumCulling, prelude::*};

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

// ---------------------------------------------------------------- pure helpers

type Pal = [Vec3; 8];
const fn v(r: f32, g: f32, b: f32) -> Vec3 {
    Vec3::new(r, g, b)
}
/// Palette slots: sun×intensity, ambient sky, ambient ground, shadow tint, sky top, horizon, fog, (night,0,0).
const NIGHT: Pal = [v(0.25, 0.3, 0.5), v(0.09, 0.11, 0.22), v(0.05, 0.05, 0.08), v(0.55, 0.6, 1.0), v(0.01, 0.015, 0.05), v(0.05, 0.07, 0.15), v(0.05, 0.07, 0.15), v(1.0, 0.0, 0.0)];
const KEYS: [(f32, Pal); 9] = [
    (0.0, NIGHT),
    (4.5, NIGHT),
    // dawn
    (6.0, [v(1.0, 0.62, 0.45), v(0.4, 0.36, 0.5), v(0.27, 0.2, 0.17), v(0.6, 0.52, 0.88), v(0.3, 0.38, 0.7), v(0.98, 0.68, 0.55), v(0.85, 0.6, 0.5), v(0.3, 0.0, 0.0)]),
    // morning
    (8.0, [v(2.0, 1.76, 1.4), v(0.4, 0.47, 0.6), v(0.28, 0.26, 0.19), v(0.57, 0.58, 0.86), v(0.28, 0.5, 0.85), v(0.82, 0.85, 0.9), v(0.82, 0.85, 0.9), v(0.0, 0.0, 0.0)]),
    // noon
    (12.0, [v(2.2, 2.07, 1.8), v(0.42, 0.5, 0.62), v(0.3, 0.28, 0.2), v(0.55, 0.58, 0.85), v(0.25, 0.48, 0.85), v(0.75, 0.85, 0.95), v(0.75, 0.85, 0.95), v(0.0, 0.0, 0.0)]),
    // golden hour
    (17.5, [v(2.1, 1.58, 0.95), v(0.4, 0.4, 0.52), v(0.29, 0.23, 0.16), v(0.6, 0.55, 0.88), v(0.25, 0.42, 0.78), v(0.98, 0.78, 0.55), v(0.9, 0.72, 0.55), v(0.0, 0.0, 0.0)]),
    // dusk (peach / pink)
    (19.0, [v(1.9, 1.05, 0.57), v(0.36, 0.32, 0.5), v(0.26, 0.18, 0.14), v(0.62, 0.52, 0.9), v(0.22, 0.26, 0.58), v(0.95, 0.55, 0.36), v(0.8, 0.5, 0.4), v(0.35, 0.0, 0.0)]),
    // twilight
    (20.5, [v(0.4, 0.36, 0.68), v(0.2, 0.2, 0.38), v(0.1, 0.08, 0.12), v(0.6, 0.58, 1.0), v(0.06, 0.08, 0.22), v(0.4, 0.28, 0.4), v(0.3, 0.22, 0.32), v(0.85, 0.0, 0.0)]),
    (22.0, NIGHT),
];

/// Keyframe palette at `hour`, wrapping smoothly over midnight.
pub fn sample_palette(hour: f32) -> Pal {
    let h = hour.rem_euclid(24.0);
    let (a, b) = match KEYS.iter().position(|k| k.0 > h) {
        Some(i) => (KEYS[i - 1], KEYS[i]),
        None => (KEYS[KEYS.len() - 1], (24.0, KEYS[0].1)),
    };
    let t = (h - a.0) / (b.0 - a.0);
    let t = t * t * (3.0 - 2.0 * t);
    std::array::from_fn(|i| a.1[i].lerp(b.1[i], t))
}

/// True sun direction (may be below the horizon): east (-x) at 6:00, ~60° at noon, west at 19:00.
pub fn sun_raw(hour: f32) -> Vec3 {
    let h = hour.rem_euclid(24.0);
    let day = (6.0..19.0).contains(&h);
    let (a, el) = if day {
        let a = (h - 6.0) / 13.0 * std::f32::consts::PI;
        (a, a.sin() * 60f32.to_radians())
    } else {
        let a = (h - 19.0).rem_euclid(24.0) / 11.0 * std::f32::consts::PI;
        (a, -a.sin() * 45f32.to_radians())
    };
    let x = if day { -a.cos() } else { a.cos() };
    Vec3::new(x * el.cos(), el.sin(), 0.4 * el.cos()).normalize()
}

/// Direction of the light that casts shadows: sun by day, the moon (antipode) at night.
pub fn light_dir(hour: f32) -> Vec3 {
    let h = hour.rem_euclid(24.0);
    let w = smooth(5.5, 6.5, h) * (1.0 - smooth(19.0, 20.0, h));
    let d = (-sun_raw(h)).lerp(sun_raw(h), w);
    Vec3::new(d.x, d.y.max(0.12), d.z).normalize()
}

fn smooth(a: f32, b: f32, x: f32) -> f32 {
    let t = ((x - a) / (b - a)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Moves `cur` towards `target` linearly (`up`/`down` seconds for a full 0..1 swing).
pub fn approach(cur: f32, target: f32, up: f32, down: f32, dt: f32) -> f32 {
    if cur < target { (cur + dt / up).min(target) } else { (cur - dt / down).max(target) }
}

/// Cross-fades kind weights towards `target` over `secs` seconds.
fn step_weights(w: &mut [f32; 6], target: usize, secs: f32, dt: f32) {
    for (i, x) in w.iter_mut().enumerate() {
        *x = approach(*x, if i == target { 1.0 } else { 0.0 }, secs, secs, dt);
    }
}

/// Lightning flash intensity `t` seconds after the strike: spike to 1, 0.25 s decay, second flicker.
pub fn flash_curve(t: f32) -> f32 {
    let a = (1.0 - t / 0.25).clamp(0.0, 1.0);
    let b = if t >= 0.12 { 0.8 * (1.0 - (t - 0.12) / 0.25).clamp(0.0, 1.0) } else { 0.0 };
    a.max(b)
}

/// Where the camera looks on the ground plane (y = 0).
pub fn ground_focus(pos: Vec3, fwd: Vec3) -> Vec3 {
    if fwd.y < -0.05 { pos + fwd * (-pos.y / fwd.y) } else { Vec3::new(pos.x, 0.0, pos.z) }
}

fn wrap(x: f32, center: f32, size: f32) -> f32 {
    center - size * 0.5 + (x - center + size * 0.5).rem_euclid(size)
}

fn hash(i: u32, s: u32) -> f32 {
    let h = i.wrapping_mul(0x9E37_79B9) ^ s.wrapping_mul(0x85EB_CA6B);
    let h = (h ^ (h >> 15)).wrapping_mul(0x2C1B_3C6D);
    (h ^ (h >> 13)) as f32 / u32::MAX as f32
}

// ---------------------------------------------------------------- weather looks

const SUN: usize = 0; // sun intensity ×
const SAT: usize = 1; // light saturation
const GREY: usize = 2; // sky/fog → grey mix
const SKY: usize = 3; // grey sky brightness ×
const AMB: usize = 4; // ambient ×
const FLAT: usize = 5; // shadow tint flattening
const FOG: usize = 6; // fog density added
const NIGHT_ADD: usize = 7;
const WIND: usize = 8; // m/s
const WET: usize = 11; // wetness target
const COVER: usize = 12; // snow cover target
const COLD: usize = 13; // cold light tint
type Look = [f32; 15];
/// Indexed like `WeatherKind::ALL`; slots 9, 10, 14 (rain, snowfall, fog wisps) are unused here:
/// particle amounts come straight from the kind weights.
const LOOKS: [Look; 6] = [
    // Clear
    [1.0, 1.0, 0.0, 1.0, 1.0, 0.0, 0.0, 0.0, 1.2, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
    // Overcast
    [0.55, 0.5, 0.7, 0.85, 1.05, 0.6, 0.008, 0.12, 2.5, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
    // Rain
    [0.35, 0.4, 0.85, 0.75, 0.95, 0.7, 0.014, 0.25, 4.0, 1.0, 0.0, 1.0, 0.0, 0.0, 0.0],
    // Storm
    [0.2, 0.3, 0.95, 0.5, 0.65, 0.8, 0.018, 0.5, 9.0, 1.3, 0.0, 1.0, 0.0, 0.0, 0.0],
    // Fog
    [0.55, 0.6, 0.5, 1.0, 0.95, 0.5, 0.054, 0.05, 0.6, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0],
    // Snow
    [0.8, 0.8, 0.35, 0.95, 1.0, 0.4, 0.012, 0.05, 1.5, 0.0, 1.0, 0.0, 1.0, 1.0, 0.0],
];
const STORM: usize = 3;
const AUTO_ODDS: [f32; 6] = [40.0, 20.0, 15.0, 10.0, 8.0, 7.0];

fn lum(c: Vec3) -> f32 {
    c.dot(Vec3::new(0.3, 0.59, 0.11))
}

// ---------------------------------------------------------------- systems

#[derive(Resource)]
struct State {
    w: [f32; 6],
    first: bool,
    t: f32,
    rng: u32,
    auto_timer: f32,
    wet: f32,
    snow: f32,
    next_flash: f32,
    flash_age: f32,
    bolt: Vec<Vec3>,
    freeze_flash: bool,
    mesh: Option<Handle<Mesh>>,
    /// False under `--render-test` without weather flags: the test scene owns `Environment`.
    drive: bool,
}

impl State {
    fn rand(&mut self) -> f32 {
        self.rng ^= self.rng << 13;
        self.rng ^= self.rng >> 17;
        self.rng ^= self.rng << 5;
        self.rng as f32 / u32::MAX as f32
    }
}

pub struct WeatherPlugin;

impl Plugin for WeatherPlugin {
    fn build(&self, app: &mut App) {
        let args: Vec<String> = std::env::args().collect();
        let value = |f: &str| args.iter().position(|a| a == f).and_then(|i| args.get(i + 1)).cloned();
        let test = args.iter().any(|a| a == "--render-test");
        let (w, h) = (value("--weather").and_then(|s| WeatherKind::parse(&s)), value("--time").and_then(|s| s.parse::<f32>().ok()));
        let mut clock = Clock::default();
        let mut weather = Weather::default();
        if let Some(h) = h {
            clock.hour = h;
        }
        if let Some(k) = w {
            weather.kind = k;
            weather.auto = false;
            weather.snap = true;
        }
        if test {
            clock.paused = true;
            weather.auto = false;
        }
        let state = State {
            w: [0.0; 6],
            first: true,
            t: 0.0,
            rng: 0x1234_5679,
            auto_timer: 0.0,
            wet: 0.0,
            snow: 0.0,
            next_flash: 3.0,
            flash_age: 99.0,
            bolt: Vec::new(),
            freeze_flash: args.iter().any(|a| a == "--flash"),
            mesh: None,
            drive: !test || w.is_some() || h.is_some(),
        };
        app.insert_resource(clock).insert_resource(weather).insert_resource(state).add_systems(Update, (tick_clock, update_env, particles).chain());
    }
}

fn tick_clock(time: Res<Time>, keys: Res<ButtonInput<KeyCode>>, mut clock: ResMut<Clock>) {
    if keys.just_pressed(KeyCode::KeyT) {
        clock.paused = !clock.paused;
    }
    if keys.just_pressed(KeyCode::BracketRight) {
        clock.hour += 1.0;
    }
    if keys.just_pressed(KeyCode::BracketLeft) {
        clock.hour -= 1.0;
    }
    if !clock.paused {
        clock.hour += clock.speed * time.delta_secs();
    }
    clock.hour = clock.hour.rem_euclid(24.0);
}

fn update_env(time: Res<Time>, clock: Res<Clock>, mut weather: ResMut<Weather>, mut st: ResMut<State>, mut env: ResMut<Environment>) {
    if !st.drive {
        return;
    }
    let dt = time.delta_secs().min(0.25);
    st.t += dt;
    let snap = weather.snap || st.first;
    weather.snap = false;
    if weather.auto {
        st.auto_timer -= dt;
        if st.auto_timer <= 0.0 {
            if !st.first {
                let mut r = st.rand() * AUTO_ODDS.iter().sum::<f32>();
                weather.kind = WeatherKind::ALL[AUTO_ODDS.iter().position(|o| { r -= o; r < 0.0 }).unwrap_or(0)];
            }
            st.auto_timer = 120.0 + 180.0 * st.rand();
        }
    }
    st.first = false;
    let target = WeatherKind::ALL.iter().position(|k| *k == weather.kind).unwrap_or(0);
    if snap {
        st.w = [0.0; 6];
        st.w[target] = 1.0;
    } else {
        step_weights(&mut st.w, target, 6.0, dt);
    }
    let sum: f32 = st.w.iter().sum();
    let mut look: Look = [0.0; 15];
    for (i, l) in LOOKS.iter().enumerate() {
        for (o, x) in look.iter_mut().zip(l) {
            *o += x * st.w[i] / sum;
        }
    }
    let storm = st.w[STORM] / sum;

    // Wetness and snow cover lag behind the weather.
    st.wet = if snap { look[WET].round() } else { approach(st.wet, look[WET], 20.0, 60.0, dt) };
    st.snow = if snap { look[COVER].round() } else { approach(st.snow, look[COVER], 30.0, 120.0, dt) };

    // Palette → environment.
    let p = sample_palette(clock.hour);
    let desat = |c: Vec3, k: f32| Vec3::splat(lum(c)).lerp(c, k);
    let grey = |c: Vec3| c.lerp(Vec3::splat(lum(c) * look[SKY]) * Vec3::new(0.97, 1.0, 1.04), look[GREY]);
    let cold = Vec3::ONE.lerp(Vec3::new(0.88, 0.96, 1.1), look[COLD]);
    let wind_dir = 0.4 + 0.8 * (st.t * 0.021).sin() + 0.3 * (st.t * 0.057).sin();
    *env = Environment {
        sun_dir: light_dir(clock.hour),
        sun_color: desat(p[0], look[SAT]) * look[SUN] * cold,
        ambient_sky: desat(p[1], 0.5 + 0.5 * look[SAT]) * look[AMB],
        ambient_ground: desat(p[2], 0.5 + 0.5 * look[SAT]) * look[AMB],
        shadow_tint: p[3].lerp(Vec3::new(0.78, 0.8, 0.9), look[FLAT]),
        sky_top: grey(p[4]),
        sky_horizon: grey(p[5]),
        fog_color: grey(p[6]),
        fog_density: 0.006 + look[FOG] + weather.fog_extra,
        wetness: st.wet,
        snow: weather.snow_override.unwrap_or(st.snow),
        night: (p[7].x + look[NIGHT_ADD]).min(1.0),
        flash: 0.0,
        wind: Vec2::new(wind_dir.cos(), wind_dir.sin()) * look[WIND] * (0.8 + 0.2 * (st.t * 0.13).sin()),
        hour: clock.hour,
    };

    // Lightning.
    if storm > 0.6 {
        st.next_flash -= dt;
        if st.next_flash <= 0.0 || (st.freeze_flash && st.bolt.is_empty()) {
            st.next_flash = 4.0 + 11.0 * st.rand();
            st.flash_age = 0.0;
            st.bolt.clear();
        }
    }
    st.flash_age = if st.freeze_flash && storm > 0.6 { 0.0 } else { st.flash_age + dt };
    env.flash = flash_curve(st.flash_age) * storm.min(1.0);
}

fn particles(
    time: Res<Time>,
    env: Res<Environment>,
    mut st: ResMut<State>,
    cam: Query<&GlobalTransform, With<GameCamera>>,
    assets: Option<Res<GladeAssets>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut commands: Commands,
) {
    let (Some(assets), Ok(cam)) = (assets, cam.single()) else { return };
    if !st.drive {
        return;
    }
    if st.mesh.is_none() {
        let h = meshes.add(particle_mesh(&[]));
        commands.spawn((Mesh3d(h.clone()), MeshMaterial3d(assets.particle.clone()), NoFrustumCulling));
        st.mesh = Some(h);
    }
    let focus = ground_focus(cam.translation(), cam.forward().as_vec3());
    let t = time.elapsed_secs() + 1000.0;
    let sum: f32 = st.w.iter().sum::<f32>().max(1e-3);
    let wn = |k: WeatherKind| st.w[WeatherKind::ALL.iter().position(|x| *x == k).unwrap_or(0)] / sum;
    let rain = wn(WeatherKind::Rain) + 1.3 * wn(WeatherKind::Storm);
    let snow = wn(WeatherKind::Snow);
    let fogw = wn(WeatherKind::Fog);
    let wind = env.wind;
    let mut ps: Vec<Particle> = Vec::new();

    if rain > 0.01 {
        let (speed, hgt) = (18.0 + 4.0 * rain, 24.0);
        let dir = Vec3::new(wind.x * 0.8, -speed, wind.y * 0.8).normalize();
        let col = Vec3::new(0.75, 0.8, 0.92) * (1.0 - 0.65 * env.night);
        for i in 0..(1500.0 * rain) as u32 {
            let s = (hash(i, 3) + t * speed / hgt).fract();
            let (y, fall) = (hgt * (1.0 - s), s * hgt / speed);
            let x = wrap(focus.x + hash(i, 1) * 40.0 - 20.0 + wind.x * 0.8 * fall, focus.x, 40.0);
            let z = wrap(focus.z + hash(i, 2) * 40.0 - 20.0 + wind.y * 0.8 * fall, focus.z, 40.0);
            if y < 0.6 {
                // Splash.
                ps.push(Particle::new(ParticleKind::Flake, Vec3::new(x, 0.08, z), 0.16, col.extend(0.7)));
            } else {
                let mut p = Particle::new(ParticleKind::Streak, Vec3::new(x, y, z), 1.0, col.extend(0.75));
                p.dir = dir;
                ps.push(p);
            }
        }
    }
    if snow > 0.01 {
        for i in 0..(800.0 * snow) as u32 {
            let speed = 1.1 + 0.8 * hash(i, 4);
            let y = 20.0 * (1.0 - (hash(i, 3) + t * speed / 20.0).fract());
            let sway = Vec2::new((t * 0.8 + i as f32).sin(), (t * 0.6 + i as f32 * 1.7).cos()) * 0.35;
            let x = wrap(focus.x + hash(i, 1) * 40.0 - 20.0 + wind.x * 0.35 * t + sway.x, focus.x, 40.0);
            let z = wrap(focus.z + hash(i, 2) * 40.0 - 20.0 + wind.y * 0.35 * t + sway.y, focus.z, 40.0);
            ps.push(Particle::new(ParticleKind::Flake, Vec3::new(x, y, z), 0.14, Vec4::ONE));
        }
    }
    if fogw > 0.01 {
        for i in 0..(14.0 * fogw).ceil() as u32 {
            let x = wrap(focus.x + hash(i, 1) * 50.0 - 25.0 + wind.x * 0.6 * t, focus.x, 50.0);
            let z = wrap(focus.z + hash(i, 2) * 50.0 - 25.0 + wind.y * 0.6 * t, focus.z, 50.0);
            ps.push(Particle::new(ParticleKind::Puff, Vec3::new(x, 0.8 + 1.8 * hash(i, 3), z), 5.0 + 4.0 * hash(i, 4), (env.fog_color * 1.05).extend(0.35 * fogw)));
        }
    }
    if st.flash_age < 0.15 {
        if st.bolt.is_empty() {
            let mut p = focus + Vec3::new(st.rand() * 14.0 - 7.0, 45.0, st.rand() * 8.0 - 4.0);
            let mut pts = vec![p];
            for k in 1..=14 {
                p = Vec3::new(p.x + (st.rand() - 0.5) * 3.0, 45.0 * (1.0 - k as f32 / 14.0), p.z + (st.rand() - 0.5) * 2.0);
                pts.push(p);
            }
            st.bolt = pts;
        }
        for pair in st.bolt.windows(2) {
            let d = pair[1] - pair[0];
            let mut p = Particle::new(ParticleKind::Streak, (pair[0] + pair[1]) * 0.5, d.length(), Vec4::new(0.95, 0.96, 1.0, 1.0));
            p.dir = d.normalize();
            ps.push(p);
        }
    } else {
        st.bolt.clear();
    }
    if let Some(mut m) = st.mesh.as_ref().and_then(|h| meshes.get_mut(h)) {
        write_particle_mesh(&mut m, &ps);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn palette_continuous_over_midnight() {
        let (a, b, c) = (sample_palette(23.999), sample_palette(0.0), sample_palette(0.001));
        for i in 0..8 {
            assert!(a[i].distance(b[i]) < 1e-3 && b[i].distance(c[i]) < 1e-3);
        }
        assert_eq!(sample_palette(12.0)[7].x, 0.0);
        assert_eq!(sample_palette(-1.0)[7].x, 1.0);
    }

    #[test]
    fn sun_arc() {
        assert!(sun_raw(12.5).y > 0.8);
        assert!(sun_raw(0.0).y < -0.5);
        assert!(sun_raw(6.0).x < -0.9 && sun_raw(19.0).x > 0.9);
        assert!(light_dir(0.0).y > 0.5, "moon lights the night");
        assert!(light_dir(12.0).y > 0.8);
        assert!((light_dir(19.5).length() - 1.0).abs() < 1e-4 && light_dir(19.5).y > 0.1);
    }

    #[test]
    fn blends_take_six_seconds() {
        let mut w = [1.0, 0.0, 0.0, 0.0, 0.0, 0.0];
        for _ in 0..30 {
            step_weights(&mut w, 2, 6.0, 0.1);
        }
        assert!((w[2] - 0.5).abs() < 0.01 && (w[0] - 0.5).abs() < 0.01);
        for _ in 0..40 {
            step_weights(&mut w, 2, 6.0, 0.1);
        }
        assert_eq!(w, [0.0, 0.0, 1.0, 0.0, 0.0, 0.0]);
        assert_eq!(approach(1.0, 0.0, 20.0, 60.0, 30.0), 0.5);
    }

    #[test]
    fn flash_and_focus() {
        assert_eq!(flash_curve(0.0), 1.0);
        assert!(flash_curve(0.2) > 0.2 && flash_curve(0.5) == 0.0);
        let f = ground_focus(Vec3::new(10.0, 10.0, 10.0), Vec3::new(-1.0, -1.0, -1.0).normalize());
        assert!(f.distance(Vec3::ZERO) < 1e-3);
    }
}
