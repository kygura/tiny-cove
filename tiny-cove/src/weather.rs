//! Day/night clock, tide, wind and weather → [`Environment`], plus rain/snow/lightning effects.
//!
//! The light rig is DESIGN.md A4 implemented literally: keyframes (sRGB, interpolated by the
//! hour) 04:30 night → 06:00 dawn → 08:30 noon → 15:00 noon → 17:00 golden → 19:00 dusk →
//! 20:30 night, each with sun/fill/tint/litRot/rim, a 3-stop sky, disc, fog, outline and the
//! A6 sea bands. Weather modifiers are deltas on the current time row, blended over 6 s.
//!
//! Contract used by ui / shot: [`Clock`] and [`Weather`] resources (UI writes, this reads).
//! Writes [`Environment`], [`Wind`] and [`Tide`] every frame.
//!
//! CLI (for screenshots): `--weather NAME --time H [--flash]`. With `--render-test` the clock is
//! paused and weather snapped; without those flags the render test keeps its own preset.

use crate::env::{Environment, Tide, Wind};
use crate::render::{GameCamera, GladeAssets, Particle, ParticleKind, particle_mesh, write_particle_mesh};
use bevy::{camera::visibility::NoFrustumCulling, prelude::*};
use cairn_cove_core::palette::{hex_to_unit, hsv_to_rgb, rgb_to_hsv, srgb};

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
    /// Parses a CLI weather name; `seafog` is an alias of `fog`.
    pub fn parse(s: &str) -> Option<Self> {
        if s.eq_ignore_ascii_case("seafog") || s.eq_ignore_ascii_case("sea-fog") {
            return Some(WeatherKind::Fog);
        }
        Self::ALL.into_iter().find(|w| w.name().eq_ignore_ascii_case(s))
    }
    fn index(self) -> usize {
        self as usize
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
/// minutes. Transitions blend smoothly over 6 s (or instantly when `snap` is set, which the
/// weather system clears after applying — used by `--demo-shot`).
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

// ---------------------------------------------------------------- the light rig (A4)

/// One row of the light rig. Every colour is sRGB 0..1 (interpolated in sRGB).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rig {
    pub sun: Vec3,
    pub sun_i: f32,
    pub fill: Vec3,
    pub fill_i: f32,
    pub tint: Vec3,
    pub lit_rot: f32,
    pub rim: Vec3,
    /// Zenith, mid, horizon.
    pub sky: [Vec3; 3],
    pub disc: Vec3,
    pub fog: Vec3,
    pub outline: Vec3,
    /// Shallow, mid, deep, abyss, foam, glint.
    pub sea: [Vec3; 6],
    /// Sea-fog colour for this hour (A4 sea fog row).
    pub sea_fog: Vec3,
    /// Fog density.
    pub fog_density: f32,
    /// Sea-fog bank strength 0..1.
    pub bank: f32,
}

fn h(hex: u32) -> Vec3 {
    hex_to_unit(hex)
}

#[allow(clippy::too_many_arguments)]
fn row(sun: (u32, f32), fill: (u32, f32), tint: u32, lit_rot: f32, rim: u32, sky: [u32; 3], disc: u32, fog: u32, outline: Vec3, sea: [u32; 6], sea_fog: u32) -> Rig {
    Rig {
        sun: h(sun.0),
        sun_i: sun.1,
        fill: h(fill.0),
        fill_i: fill.1,
        tint: h(tint),
        lit_rot,
        rim: h(rim),
        sky: sky.map(h),
        disc: h(disc),
        fog: h(fog),
        outline,
        sea: sea.map(h),
        sea_fog: h(sea_fog),
        fog_density: 0.0035,
        bank: 0.0,
    }
}

const OUTLINE_DAY: u32 = 0x31217B;
const OUTLINE_NIGHT: u32 = 0x10104A;

pub fn dawn() -> Rig {
    row((0xFFC5A5, 0.90), (0xD6BDFF, 0.85), 0x6342AD, 0.0, 0xFF9CCE, [0x5263DE, 0xC584D6, 0xFFB5A5], 0xFFD6AD, 0xEFADC5, h(OUTLINE_DAY), [0x4ADEE6, 0x3194D6, 0x2952C5, 0x3131A5, 0xFFEFF7, 0xFFCEC5], 0xFFCEDE)
}
pub fn noon() -> Rig {
    row((0xFFFFFF, 1.0), (0xFFFFFF, 1.0), 0x4A3AAD, 0.0, 0xCEF7FF, [0x1973F7, 0x42ADFF, 0x8CE6FF], 0xFFFFDE, 0xB5EFFF, h(OUTLINE_DAY), [0x19DECE, 0x08ADD6, 0x1052C5, 0x1929A5, 0xFFFFFF, 0xFFFFDE], 0xCEF7EF)
}
pub fn golden() -> Rig {
    row((0xFFD69C, 1.0), (0xEFB5DE, 0.90), 0x7329A5, 0.0, 0xFFD631, [0x3A5AD6, 0xEF84A5, 0xFFC55A], 0xFFE673, 0xFFBD84, h(OUTLINE_DAY), [0x3AE6B5, 0x19B5BD, 0x194AB5, 0x29299C, 0xFFF7DE, 0xFFD67B], 0xFFD6AD)
}
pub fn dusk() -> Rig {
    let outline = h(OUTLINE_DAY).lerp(h(OUTLINE_NIGHT), 0.5);
    row((0xFFADC5, 0.85), (0xB594EF, 0.78), 0x42198C, 10.0, 0xFF4AAD, [0x3A2194, 0xE63A9C, 0xFF7B42], 0xFF8C52, 0xB54AA5, outline, [0x31B5CE, 0x3A63C5, 0x2931A5, 0x29198C, 0xFFC5DE, 0xFF7BD6], 0xD68CC5)
}
pub fn night() -> Rig {
    row((0xA5ADFF, 0.55), (0x7B8CFF, 0.62), 0x21197B, 30.0, 0x63DEFF, [0x081063, 0x102994, 0x2152C5], 0xDEEFFF, 0x19297B, h(OUTLINE_NIGHT), [0x1984A5, 0x105A9C, 0x10318C, 0x10196B, 0x9CBDFF, 0xCEF7FF], 0x293A9C)
}

impl Rig {
    pub fn lerp(&self, o: &Rig, t: f32) -> Rig {
        let l = |a: Vec3, b: Vec3| a.lerp(b, t);
        let f = |a: f32, b: f32| a + (b - a) * t;
        Rig {
            sun: l(self.sun, o.sun),
            sun_i: f(self.sun_i, o.sun_i),
            fill: l(self.fill, o.fill),
            fill_i: f(self.fill_i, o.fill_i),
            tint: l(self.tint, o.tint),
            lit_rot: f(self.lit_rot, o.lit_rot),
            rim: l(self.rim, o.rim),
            sky: std::array::from_fn(|i| l(self.sky[i], o.sky[i])),
            disc: l(self.disc, o.disc),
            fog: l(self.fog, o.fog),
            outline: l(self.outline, o.outline),
            sea: std::array::from_fn(|i| l(self.sea[i], o.sea[i])),
            sea_fog: l(self.sea_fog, o.sea_fog),
            fog_density: f(self.fog_density, o.fog_density),
            bank: f(self.bank, o.bank),
        }
    }
}

/// The time-of-day rig at `hour` (A4 keyframes, interpolated in sRGB, wrapping at midnight).
pub fn rig_at(hour: f32) -> Rig {
    let hr = hour.rem_euclid(24.0);
    let keys: [(f32, fn() -> Rig); 8] =
        [(4.5, night), (6.0, dawn), (8.5, noon), (15.0, noon), (17.0, golden), (19.0, dusk), (20.5, night), (28.5, night)];
    let x = if hr < 4.5 { hr + 24.0 } else { hr };
    let i = keys.iter().position(|k| k.0 > x).unwrap_or(keys.len() - 1).max(1);
    let (a, b) = (keys[i - 1], keys[i]);
    let t = ((x - a.0) / (b.0 - a.0)).clamp(0.0, 1.0);
    let mut r = (a.1)().lerp(&(b.1)(), t);
    // Interpolating between two hued keys can pass through grey (pink dawn fog → aqua noon
    // fog); keep sky and fog chroma up (DESIGN A0: fog is never grey).
    for c in r.sky.iter_mut().chain([&mut r.fog, &mut r.sea_fog]) {
        *c = vivid(*c, 0.16);
    }
    r
}

/// Raises the HSV saturation of sRGB `c` to at least `min_s`, keeping hue and value.
pub fn vivid(c: Vec3, min_s: f32) -> Vec3 {
    let mut hsv = rgb_to_hsv(c);
    if hsv.y >= min_s || hsv.z <= 0.0 {
        return c;
    }
    hsv.y = min_s;
    hsv_to_rgb(hsv)
}

/// 0 by day, 1 at night (follows the rig keys: 0 until 19:00, 1 from 20:30 to 04:30).
pub fn night_weight(hour: f32) -> f32 {
    let x = hour.rem_euclid(24.0);
    if (6.0..=19.0).contains(&x) {
        0.0
    } else if x > 19.0 && x < 20.5 {
        (x - 19.0) / 1.5
    } else if x > 4.5 && x < 6.0 {
        1.0 - (x - 4.5) / 1.5
    } else {
        1.0
    }
}

/// Emissive strength: Glow / WindowLight / Fire full from 19:30 to 06:00; lanterns from 19:00.
pub fn emissive_weight(hour: f32) -> f32 {
    let x = hour.rem_euclid(24.0);
    let eve = smooth(19.0, 19.5, x);
    let morn = 1.0 - smooth(5.5, 6.0, x);
    if x >= 12.0 { eve } else { morn }
}

fn mix_toward(c: Vec3, target: u32, k: f32) -> Vec3 {
    c.lerp(h(target), k)
}

/// How much of the warm golden / dusk / dawn light survives a weather change at `hour`
/// (0 at noon and at night, 1 around golden hour and dusk, ramping in from 15:30 and out by
/// 20:30; at dawn from 05:00 to 08:30).
pub fn warm_hour(hour: f32) -> f32 {
    let x = hour.rem_euclid(24.0);
    let eve = smooth(15.5, 17.5, x) * (1.0 - smooth(19.5, 20.5, x));
    let dawn = smooth(4.5, 5.5, x) * (1.0 - smooth(7.0, 8.5, x));
    eve.max(dawn)
}

/// Applies weather `kind`'s A4 modifiers to the time row `base`. `day` (0..1) fades the
/// absolute replacements (sky, fill, fog) toward the time row at night, so a rainy night
/// stays an ultramarine night. `warm` ([`warm_hour`]) keeps part of a golden / dusk / dawn
/// row under rain: a warm horizon band in the sky and a warmer fill (DESIGN A8 dusk rain).
pub fn apply_weather(base: &Rig, kind: WeatherKind, day: f32, warm: f32) -> Rig {
    let mut r = *base;
    let set = |c: Vec3, hex: u32| c.lerp(h(hex), day);
    // Absolute intensities (DESIGN "Fill #… × k") fade to the time row at night too.
    let seti = |v: f32, k: f32| v + (k - v) * day;
    let sky = |r: &mut Rig, s: [u32; 3]| {
        for (c, hx) in r.sky.iter_mut().zip(s) {
            *c = c.lerp(h(hx), day);
        }
    };
    match kind {
        WeatherKind::Clear => {}
        WeatherKind::Overcast => {
            r.sun_i *= 0.60;
            r.sun = mix_toward(r.sun, 0xF7EFFF, 0.5);
            r.fill = set(r.fill, 0xC5BDFF);
            r.fill_i = seti(r.fill_i, 1.25);
            r.tint = set(r.tint, 0x6B63CE);
            r.rim = Vec3::ZERO;
            sky(&mut r, [0x7BA5EF, 0xA5BDFF, 0xCEDEFF]);
            r.fog = set(r.fog, 0xC5CEFF);
            r.sea[0] = set(r.sea[0], 0x21C5BD);
            r.fog_density = 0.006;
        }
        WeatherKind::Rain => {
            // At golden hour and dusk the rain keeps the hour's warmth: the horizon band and
            // half the mid band stay sunset, the fill stays half pink-violet, and the outline
            // goes to night ink so it stays darker than the wet, dimmer land.
            let keep = warm.clamp(0.0, 1.0);
            r.sun_i *= 0.45 + 0.35 * keep;
            r.fill = base.fill.lerp(h(0x8C9CF7), day * (1.0 - 0.6 * keep));
            r.fill_i = seti(r.fill_i, 1.0 + 0.2 * keep);
            r.tint = set(r.tint, 0x4A3AB5);
            for (i, (c, hx)) in r.sky.iter_mut().zip([0x4A6BD6, 0x6B94EF, 0x9CB5FF]).enumerate() {
                let hold = [0.0, 0.45, 0.85][i] * keep;
                *c = c.lerp(h(hx), day * (1.0 - hold));
            }
            r.fog = set(r.fog, 0x6B9CE6);
            r.fog_density *= 2.0;
            r.sea[1] = set(r.sea[1], 0x08A5C5);
            r.outline = r.outline.lerp(h(OUTLINE_NIGHT), 0.5 * day);
        }
        WeatherKind::Storm => {
            // DESIGN: sun ×0.30, fill ×0.90. Lifted (×0.45, ×1.3) so the storm frame stays
            // readable at 240p: warm albedos go plum and slate, not near-black mud.
            r.sun = set(r.sun, 0x8CEFE6);
            r.sun_i = base.sun_i * 0.45;
            r.fill = set(r.fill, 0x5A7BDE);
            r.fill_i = seti(r.fill_i, 1.3);
            r.tint = set(r.tint, 0x31107B);
            r.rim = h(0x42FFE6);
            // Night ink: the day keyline (#31217B) is lighter than the storm-dark land.
            r.outline = h(OUTLINE_NIGHT);
            sky(&mut r, [0x29107B, 0x2952BD, 0x19A5AD]);
            r.fog = set(r.fog, 0x29639C);
            for (i, hx) in [0x10C5B5, 0x1084AD, 0x103A9C, 0x10216B].into_iter().enumerate() {
                r.sea[i] = set(r.sea[i], hx);
            }
            r.fog_density = 0.009;
        }
        WeatherKind::Fog => {
            r.sun_i *= 0.65;
            r.fog = base.sea_fog;
            r.fill = base.sea_fog;
            r.fill_i = 0.85 + 0.15 * (1.0 - day);
            for c in r.sky.iter_mut() {
                *c = c.lerp(base.sea_fog, 0.4);
            }
            // The height bank (dense below 3 m) does the swallowing; distance fog stays near
            // a clear day's, so roofs, palms and beacons above the bank keep their chroma.
            r.fog_density = 0.004;
            r.bank = 1.0;
        }
        WeatherKind::Snow => {
            r.sun = set(r.sun, 0xFFF7EF);
            r.sun_i *= 0.90;
            r.fill = set(r.fill, 0xADC5FF);
            r.fill_i = seti(r.fill_i, 1.0);
            r.tint = set(r.tint, 0x5A5AE6);
            sky(&mut r, [0x73B5FF, 0xA5D6FF, 0xDEF7FF]);
            r.fog = set(r.fog, 0xCEE6FF);
            // The sea moves one band deeper.
            let s = base.sea;
            r.sea[0] = set(s[0], 0x10A5CE);
            r.sea[1] = s[2];
            r.sea[2] = s[3];
            r.fog_density = 0.006;
        }
    }
    r
}

/// Wind strength (m/s) per weather, indexed like [`WeatherKind::ALL`].
const WIND: [f32; 6] = [1.2, 2.5, 4.0, 9.0, 0.6, 1.5];
const AUTO_ODDS: [f32; 6] = [40.0, 20.0, 15.0, 10.0, 8.0, 7.0];

/// True sun direction (may be below the horizon): rises in the east (-x) at 05:00, peaks at
/// 60° at 12:00, sets in the west at 19:00 (half-set at dusk).
pub fn sun_raw(hour: f32) -> Vec3 {
    let hr = hour.rem_euclid(24.0);
    let a = (hr - 5.0) / 14.0 * std::f32::consts::PI;
    let el = if (5.0..19.0).contains(&hr) { a.sin() * 60f32.to_radians() } else { -(a - std::f32::consts::PI).sin().abs() * 40f32.to_radians() };
    let az = Vec3::new(-a.cos(), 0.0, 0.6).normalize();
    az * el.cos() + Vec3::Y * el.sin()
}

/// Direction of the shadow-casting light: the sun by day, the moon (antipode) from 20:30 to
/// 04:30. Kept above ~20° (shadows at most ~2.7× the caster height) so shadows stay sharp
/// and readable at every hour.
pub fn light_dir(hour: f32) -> Vec3 {
    let hr = hour.rem_euclid(24.0);
    let w = smooth(4.5, 5.5, hr) * (1.0 - smooth(19.5, 20.5, hr));
    let s = sun_raw(hr);
    let moon = Vec3::new(-s.x, s.y.abs().max(0.6), -s.z * 0.4 + 0.3);
    let d = moon.normalize().lerp(Vec3::new(s.x, s.y.max(0.2), s.z), w);
    Vec3::new(d.x, d.y.max(0.36), d.z).normalize()
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

/// Where the camera looks on the ground plane (y = 0).
pub fn ground_focus(pos: Vec3, fwd: Vec3) -> Vec3 {
    if fwd.y < -0.05 { pos + fwd * (-pos.y / fwd.y) } else { Vec3::new(pos.x, 0.0, pos.z) }
}

/// Rain / snow volume that fills an orthographic view of half size `half` (m) pitched with
/// `pitch_sin`, for particles falling from `top` m: returns the box centre offset along the
/// ground-forward axis and its half extents (right, forward). A particle at height y shows
/// y·cos(pitch) higher on screen, so the box reaches further back for falling particles to
/// cover the frame's foreground as well as its top.
pub fn precip_box(half: Vec2, pitch_sin: f32, top: f32) -> (f32, Vec2) {
    let s = if pitch_sin.is_finite() { pitch_sin.clamp(0.25, 1.0) } else { 0.5 };
    let c = (1.0 - s * s).sqrt();
    let half = if half.is_finite() { half.clamp(Vec2::splat(1.0), Vec2::splat(400.0)) } else { Vec2::splat(20.0) };
    let lift = top.clamp(0.0, 100.0) * c;
    (-0.5 * lift / s, Vec2::new(half.x + 2.0, (half.y + 0.5 * lift) / s + 2.0))
}

/// Rain streak direction and drift: wind × this (exaggerated so the slant reads at 240p).
const RAIN_SLANT: f32 = 3.0;
/// Least on-screen lean (horizontal / vertical) of a rain streak in a full wind, so the
/// 1×4 px streak steps at least one pixel sideways whatever the wind's angle to the view.
const RAIN_MIN_LEAN: f32 = 0.75;

/// Horizontal velocity of rain falling at `speed` in `wind` (m/s, world xz), seen by a
/// camera with ground axes `right_h` / `fwd_h` pitched down by `pitch_sin`: the wind × the
/// slant factor, plus a sideways push when the wind blows along the view so the streak
/// still leans on screen (DESIGN A4: "slanted by wind"). No wind, no lean.
pub fn rain_velocity(wind: Vec2, speed: f32, right_h: Vec2, fwd_h: Vec2, pitch_sin: f32) -> Vec2 {
    let wind = if wind.is_finite() { wind } else { Vec2::ZERO };
    let mut vel = wind * RAIN_SLANT;
    let s = pitch_sin.clamp(0.0, 1.0);
    let c = (1.0 - s * s).sqrt();
    // Screen motion: right = vel·right, down = fall·cos + toward-camera drift·sin.
    let down = (speed * c - vel.dot(fwd_h) * s).abs().max(1e-3);
    let side = vel.dot(right_h);
    let want = RAIN_MIN_LEAN * down * (wind.length() / 3.0).min(1.0);
    if side.abs() < want {
        let sign = if side < 0.0 { -1.0 } else { 1.0 };
        vel += right_h * (sign * want - side);
    }
    vel
}
/// Precipitation density: particles per m² of ground box (rain, snow) and caps.
const RAIN_PER_M2: f32 = 320.0 / 1600.0;
const SNOW_PER_M2: f32 = 800.0 / 1600.0;
const MAX_RAIN: f32 = 7000.0;
const MAX_SNOW: f32 = 5000.0;

fn wrap(x: f32, center: f32, size: f32) -> f32 {
    center - size * 0.5 + (x - center + size * 0.5).rem_euclid(size)
}

fn hash(i: u32, s: u32) -> f32 {
    let h = i.wrapping_mul(0x9E37_79B9) ^ s.wrapping_mul(0x85EB_CA6B);
    let h = (h ^ (h >> 15)).wrapping_mul(0x2C1B_3C6D);
    (h ^ (h >> 13)) as f32 / u32::MAX as f32
}

/// Inputs to [`environment`] besides the hour.
#[derive(Clone, Copy, Debug, Default)]
pub struct WeatherMix {
    /// Weights of each [`WeatherKind`] (indexed like `ALL`), summing to ~1.
    pub w: [f32; 6],
    pub wet: f32,
    pub snow: f32,
    /// Lightning on this frame.
    pub flash: bool,
    pub wind: Vec2,
}

impl WeatherMix {
    /// All of `kind`, settled (wet / snow cover at their targets).
    pub fn settled(kind: WeatherKind) -> Self {
        let mut w = [0.0; 6];
        w[kind.index()] = 1.0;
        let dir = Vec2::new(0.96, 0.29);
        Self {
            w,
            wet: if matches!(kind, WeatherKind::Rain | WeatherKind::Storm) { 1.0 } else { 0.0 },
            snow: if kind == WeatherKind::Snow { 1.0 } else { 0.0 },
            flash: false,
            wind: dir * WIND[kind.index()],
        }
    }
}

/// The full environment at `hour` under the weather mix `m`. Pure.
pub fn environment(hour: f32, m: &WeatherMix) -> Environment {
    let base = rig_at(hour);
    let nw = night_weight(hour);
    let day = 1.0 - nw;
    let warm = warm_hour(hour);
    let sum: f32 = m.w.iter().sum::<f32>().max(1e-4);
    let mut rig = base;
    let mut acc: Option<Rig> = None;
    let mut acc_w = 0.0;
    for k in WeatherKind::ALL {
        let wk = m.w[k.index()] / sum;
        if wk <= 0.0 {
            continue;
        }
        let r = apply_weather(&base, k, day, warm);
        acc = Some(match acc {
            None => r,
            Some(a) => a.lerp(&r, wk / (acc_w + wk)),
        });
        acc_w += wk;
    }
    if let Some(a) = acc {
        rig = a;
    }
    let wk = |k: WeatherKind| m.w[k.index()] / sum;
    let storm = wk(WeatherKind::Storm);
    let fog = wk(WeatherKind::Fog);

    if m.flash {
        // Lightning: the whole frame lit as if sunI = 1.6 with sun #E6DEFF, flat lilac sky.
        rig.sun = h(0xE6DEFF);
        rig.sun_i = 1.6;
        rig.sky = [h(0xCEBDFF); 3];
    }
    let lin = |c: Vec3| srgb(cairn_cove_core::palette::unit_to_hex(c));
    let fill = rig.fill * rig.fill_i;
    let sun_raw_y = sun_raw(hour).y;
    let tide = Tide::at(hour);
    Environment {
        sun_dir: light_dir(hour),
        sun_color: rig.sun * rig.sun_i,
        fill_color: fill,
        ambient_sky: lin(fill.min(Vec3::ONE)) * 0.8 + lin(rig.sky[2]) * 0.2,
        ambient_ground: lin((fill * 0.7).min(Vec3::ONE)),
        shadow_tint: rig.tint,
        rim_color: rig.rim,
        lit_rot: rig.lit_rot,
        sky_top: lin(rig.sky[0]),
        sky_mid: lin(rig.sky[1]),
        sky_horizon: lin(rig.sky[2]),
        disc_color: lin(rig.disc),
        outline: lin(rig.outline),
        fog_color: lin(rig.fog),
        fog_density: rig.fog_density,
        sea_fog: rig.bank,
        fog_top: 3.0,
        sea: rig.sea.map(lin),
        storm,
        wetness: m.wet,
        snow: m.snow,
        night: emissive_weight(hour).max(0.75 * storm).max(0.6 * fog).min(1.0),
        dark: smooth(-0.02, -0.105, sun_raw_y),
        flash: if m.flash { 1.0 } else { 0.0 },
        wind: m.wind,
        tide: tide.level,
        tide_falling: tide.falling,
        hour: hour.rem_euclid(24.0),
    }
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
    /// Frames of lightning left (a strike lasts 2 frames).
    flash_frames: u32,
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
        let (w, hr) = (value("--weather").and_then(|s| WeatherKind::parse(&s)), value("--time").and_then(|s| s.parse::<f32>().ok()));
        let mut clock = Clock::default();
        let mut weather = Weather::default();
        if let Some(hr) = hr.filter(|h| h.is_finite()) {
            clock.hour = hr.rem_euclid(24.0);
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
            // A scripted `--demo-shot` is deterministic: no random strike unless `--flash`.
            next_flash: if args.iter().any(|a| a == "--demo-shot") { f32::INFINITY } else { 2.0 },
            flash_frames: 0,
            bolt: Vec::new(),
            freeze_flash: args.iter().any(|a| a == "--flash"),
            mesh: None,
            drive: !test || w.is_some() || hr.is_some(),
        };
        app.insert_resource(clock)
            .insert_resource(weather)
            .insert_resource(state)
            .init_resource::<Wind>()
            .init_resource::<Tide>()
            .add_systems(Update, (tick_clock, update_env, particles).chain());
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
    if !clock.hour.is_finite() {
        clock.hour = 12.0;
    }
    clock.hour = clock.hour.rem_euclid(24.0);
}

fn update_env(
    time: Res<Time>,
    clock: Res<Clock>,
    mut weather: ResMut<Weather>,
    mut st: ResMut<State>,
    mut env: ResMut<Environment>,
    mut wind: ResMut<Wind>,
    mut tide: ResMut<Tide>,
) {
    *tide = Tide::at(clock.hour);
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
                weather.kind = WeatherKind::ALL[AUTO_ODDS.iter().position(|o| {
                    r -= o;
                    r < 0.0
                })
                .unwrap_or(0)];
            }
            st.auto_timer = 120.0 + 180.0 * st.rand();
        }
    }
    st.first = false;
    let target = weather.kind.index();
    if snap {
        st.w = [0.0; 6];
        st.w[target] = 1.0;
    } else {
        step_weights(&mut st.w, target, 6.0, dt);
    }
    let (w, sum) = (st.w, st.w.iter().sum::<f32>().max(1e-4));
    let wk = |k: WeatherKind| w[k.index()] / sum;
    let wet_target = wk(WeatherKind::Rain) + wk(WeatherKind::Storm);
    let snow_target = wk(WeatherKind::Snow);
    st.wet = if snap { wet_target.round() } else { approach(st.wet, wet_target, 20.0, 60.0, dt) };
    st.snow = if snap { snow_target.round() } else { approach(st.snow, snow_target, 30.0, 120.0, dt) };
    let storm = wk(WeatherKind::Storm);

    // Wind: strength from the weather mix, direction wandering slowly.
    let strength: f32 = WIND.iter().zip(st.w.iter()).map(|(s, w)| s * w / sum).sum::<f32>() * (0.85 + 0.15 * (st.t * 0.13).sin());
    let ang = 0.3 + 0.5 * (st.t * 0.021).sin() + 0.2 * (st.t * 0.057).sin();
    *wind = Wind { dir: Vec2::from_angle(ang), strength };

    // Lightning: every 6–14 s in a storm, 2 frames, no fade.
    if storm > 0.6 {
        st.next_flash -= dt;
        if st.next_flash <= 0.0 || (st.freeze_flash && st.flash_frames == 0) {
            st.next_flash = 6.0 + 8.0 * st.rand();
            st.flash_frames = 2;
            st.bolt.clear();
        }
    }
    let flash = st.flash_frames > 0 && storm > 0.0;
    if !st.freeze_flash {
        st.flash_frames = st.flash_frames.saturating_sub(1);
    }

    let mix = WeatherMix { w: st.w, wet: st.wet, snow: weather.snow_override.unwrap_or(st.snow), flash, wind: wind.vector() };
    let mut e = environment(clock.hour, &mix);
    e.fog_density += weather.fog_extra.clamp(0.0, 0.08);
    *env = e;
}

fn particles(
    time: Res<Time>,
    env: Res<Environment>,
    mut st: ResMut<State>,
    cam: Query<(&GlobalTransform, &Projection), With<GameCamera>>,
    assets: Option<Res<GladeAssets>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut commands: Commands,
) {
    let (Some(assets), Ok((cam, projection))) = (assets, cam.single()) else { return };
    if !st.drive {
        return;
    }
    if st.mesh.is_none() {
        let h = meshes.add(particle_mesh(&[]));
        commands.spawn((Mesh3d(h.clone()), MeshMaterial3d(assets.particle.clone()), NoFrustumCulling));
        st.mesh = Some(h);
    }
    let fwd3 = cam.forward().as_vec3();
    // Fill the whole view (not a column around the focus): a box in the camera's ground axes,
    // centred on the ground point at the middle of the frame (the focus sits below it, see
    // `render::VIEW_ORIGIN_Y`).
    let view = match projection {
        Projection::Orthographic(o) => o.area,
        _ => Rect::from_center_half_size(Vec2::ZERO, Vec2::new(crate::render::VIEW_HEIGHT * 0.89, crate::render::VIEW_HEIGHT * 0.5)),
    };
    let half_view = view.half_size();
    let centre = cam.translation() + cam.right().as_vec3() * view.center().x + cam.up().as_vec3() * view.center().y;
    let focus = ground_focus(centre, fwd3);
    let fwd_h = Vec2::new(fwd3.x, fwd3.z).normalize_or(Vec2::NEG_Y);
    let right_h = Vec2::new(-fwd_h.y, fwd_h.x);
    let pitch_sin = -fwd3.y;
    let place = |u: f32, v: f32, drift: Vec2, top: f32| -> (Vec2, f32) {
        let (off, h) = precip_box(half_view, pitch_sin, top);
        let u = wrap(u * h.x + drift.dot(right_h), 0.0, 2.0 * h.x);
        let v = wrap(off + v * h.y + drift.dot(fwd_h), off, 2.0 * h.y);
        (focus.xz() + right_h * u + fwd_h * v, 4.0 * h.x * h.y)
    };
    let t = time.elapsed_secs() + 1000.0;
    let sum: f32 = st.w.iter().sum::<f32>().max(1e-3);
    let wn = |k: WeatherKind| st.w[k.index()] / sum;
    let rain = wn(WeatherKind::Rain) + 1.3 * wn(WeatherKind::Storm);
    let snow = wn(WeatherKind::Snow);
    let fogw = wn(WeatherKind::Fog);
    let wind = env.wind;
    let mut ps: Vec<Particle> = Vec::new();

    if rain > 0.01 {
        // Pale cyan streaks, slanted by the wind.
        let (speed, hgt) = (18.0 + 4.0 * rain, 24.0);
        let slant = rain_velocity(wind, speed, right_h, fwd_h, pitch_sin);
        let dir = Vec3::new(slant.x, -speed, slant.y).normalize();
        let col = cairn_cove_core::palette::rain() * (1.0 - 0.55 * env.dark);
        let area = place(0.0, 0.0, Vec2::ZERO, hgt).1;
        for i in 0..(RAIN_PER_M2 * area * rain).min(MAX_RAIN) as u32 {
            let s = (hash(i, 3) + t * speed / hgt).fract();
            let (y, fall) = (hgt * (1.0 - s), s * hgt / speed);
            let (xz, _) = place(hash(i, 1) * 2.0 - 1.0, hash(i, 2) * 2.0 - 1.0, slant * fall, hgt);
            let (x, z) = (xz.x, xz.y);
            if y < 0.6 {
                ps.push(Particle::new(ParticleKind::Flake, Vec3::new(x, env.tide.max(0.0) + 0.08, z), 0.12, col.extend(0.6)));
            } else {
                let mut p = Particle::new(ParticleKind::Streak, Vec3::new(x, y, z), 0.3, col.extend(0.6));
                p.dir = dir;
                ps.push(p);
            }
        }
    }
    if snow > 0.01 {
        // 1-px white flakes with a lilac pixel below, drifting with the wind.
        let area = place(0.0, 0.0, Vec2::ZERO, 20.0).1;
        for i in 0..(SNOW_PER_M2 * area * snow).min(MAX_SNOW) as u32 {
            let speed = 1.1 + 0.8 * hash(i, 4);
            let y = 20.0 * (1.0 - (hash(i, 3) + t * speed / 20.0).fract());
            let sway = Vec2::new((t * 0.8 + i as f32).sin(), (t * 0.6 + i as f32 * 1.7).cos()) * 0.35;
            let (xz, _) = place(hash(i, 1) * 2.0 - 1.0, hash(i, 2) * 2.0 - 1.0, wind * 0.6 * t + sway, 20.0);
            let (x, z) = (xz.x, xz.y);
            ps.push(Particle::new(ParticleKind::Snow, Vec3::new(x, y, z), 0.1, Vec4::ONE));
        }
    }
    if fogw > 0.01 {
        for i in 0..(10.0 * fogw).ceil() as u32 {
            let x = wrap(focus.x + hash(i, 1) * 50.0 - 25.0 + wind.x * 0.6 * t, focus.x, 50.0);
            let z = wrap(focus.z + hash(i, 2) * 50.0 - 25.0 + wind.y * 0.6 * t, focus.z, 50.0);
            ps.push(Particle::new(ParticleKind::Puff, Vec3::new(x, 1.0 + 1.6 * hash(i, 3), z), 5.0 + 4.0 * hash(i, 4), (env.fog_color * 1.05).extend(0.3 * fogw)));
        }
    }
    if env.flash > 0.0 {
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
            let mut p = Particle::new(ParticleKind::Streak, (pair[0] + pair[1]) * 0.5, d.length(), Vec4::ONE);
            p.dir = d.normalize();
            ps.push(p);
        }
    }
    if let Some(mut m) = st.mesh.as_ref().and_then(|h| meshes.get_mut(h)) {
        write_particle_mesh(&mut m, &ps);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cairn_cove_core::palette::unit_to_hex;

    #[test]
    fn weather_names_parse() {
        for k in WeatherKind::ALL {
            assert_eq!(WeatherKind::parse(k.name()), Some(k));
            assert_eq!(WeatherKind::ALL[k.index()], k);
        }
        assert_eq!(WeatherKind::parse("SeaFog"), Some(WeatherKind::Fog));
        assert_eq!(WeatherKind::parse("drizzle"), None);
    }

    /// The rig hits the A4 keyframes exactly at their hours.
    #[test]
    fn rig_keyframes() {
        let same = |a: Rig, b: Rig| {
            let v = |r: &Rig| [r.sun, r.fill, r.tint, r.rim, r.sky[0], r.sky[1], r.sky[2], r.disc, r.fog, r.outline, r.sea[0], r.sea[3], r.sea[4]];
            v(&a).iter().zip(v(&b)).all(|(x, y)| x.distance(y) < 1e-5) && (a.sun_i - b.sun_i).abs() < 1e-5 && (a.lit_rot - b.lit_rot).abs() < 1e-4
        };
        for (hr, key) in [(12.0, noon()), (10.0, noon()), (6.0, dawn()), (17.0, golden()), (19.0, dusk()), (23.0, night()), (2.0, night())] {
            assert!(same(rig_at(hr), key), "keyframe at {hr}");
        }
        assert_eq!(unit_to_hex(rig_at(12.0).tint), 0x4A3AAD);
        assert_eq!(unit_to_hex(rig_at(23.0).sky[0]), 0x081063);
    }

    #[test]
    fn rig_continuous_over_midnight_and_keys() {
        for hr in [0.0f32, 4.5, 6.0, 8.5, 15.0, 17.0, 19.0, 20.5] {
            let (a, b) = (rig_at(hr - 0.001), rig_at(hr + 0.001));
            assert!(a.sun.distance(b.sun) < 0.01 && a.sky[1].distance(b.sky[1]) < 0.01, "jump at {hr}");
        }
        assert!(rig_at(23.999).sky[0].distance(rig_at(0.0).sky[0]) < 1e-3);
    }

    /// Never grey: every sky, fog and tint stop at every hour and weather keeps chroma.
    #[test]
    fn rig_never_grey() {
        for k in WeatherKind::ALL {
            for i in 0..48 {
                let hr = i as f32 * 0.5;
                let env = environment(hr, &WeatherMix::settled(k));
                let lin_s = |c: Vec3| rgb_to_hsv(Vec3::new(c.x.powf(1.0 / 2.2), c.y.powf(1.0 / 2.2), c.z.powf(1.0 / 2.2))).y;
                for (name, c) in [("tint", rgb_to_hsv(env.shadow_tint).y), ("fog", lin_s(env.fog_color)), ("sky", lin_s(env.sky_top))] {
                    assert!(c > 0.15, "{} {hr}: {name} grey ({c})", k.name());
                }
                assert!(env.sun_color.is_finite() && env.fill_color.max_element() > 0.2);
            }
        }
    }

    #[test]
    fn sun_arc() {
        assert!((sun_raw(12.0).y - 60f32.to_radians().sin()).abs() < 0.05, "60° at noon");
        assert!(sun_raw(0.0).y < -0.3);
        assert!(sun_raw(6.0).x < -0.8 && sun_raw(18.5).x > 0.8);
        assert!(sun_raw(6.0).y > 0.0 && sun_raw(6.0).y < 0.4, "low at dawn");
        for i in 0..96 {
            let d = light_dir(i as f32 * 0.25);
            assert!((d.length() - 1.0).abs() < 1e-4 && d.y > 0.2, "light always above the horizon");
        }
    }

    #[test]
    fn emissive_hours() {
        assert_eq!(emissive_weight(12.0), 0.0);
        assert_eq!(emissive_weight(19.5), 1.0);
        assert_eq!(emissive_weight(2.0), 1.0);
        assert_eq!(emissive_weight(6.0), 0.0);
        assert!(emissive_weight(19.2) > 0.0 && emissive_weight(19.2) < 1.0, "lanterns from 19:00");
        let fog = environment(12.0, &WeatherMix::settled(WeatherKind::Fog));
        let storm = environment(12.0, &WeatherMix::settled(WeatherKind::Storm));
        assert!(fog.night > 0.5 && storm.night > 0.5, "beacons burn in fog and storms");
        assert_eq!(environment(12.0, &WeatherMix::settled(WeatherKind::Clear)).night, 0.0);
    }

    #[test]
    fn weather_modifiers() {
        let clear = environment(12.0, &WeatherMix::settled(WeatherKind::Clear));
        assert_eq!(clear.sun_color, Vec3::ONE);
        assert_eq!(unit_to_hex(clear.shadow_tint), 0x4A3AAD);
        let storm = environment(12.0, &WeatherMix::settled(WeatherKind::Storm));
        assert!(storm.storm > 0.99 && storm.sun_color.max_element() < 0.5);
        assert_eq!(unit_to_hex(storm.rim_color), 0x42FFE6);
        let fog = environment(12.0, &WeatherMix::settled(WeatherKind::Fog));
        assert!(fog.sea_fog > 0.99);
        assert_eq!(unit_to_hex(fog_unit(fog.fog_color)), 0xCEF7EF);
        let over = environment(12.0, &WeatherMix::settled(WeatherKind::Overcast));
        assert_eq!(over.rim_color, Vec3::ZERO);
        // Lightning lights the frame lilac-white.
        let mut m = WeatherMix::settled(WeatherKind::Storm);
        m.flash = true;
        let f = environment(12.0, &m);
        assert!(f.flash == 1.0 && f.sun_color.max_element() > 1.5);
        // Night rain stays a night.
        let nr = environment(23.0, &WeatherMix::settled(WeatherKind::Rain));
        assert_eq!(nr.sky_top, environment(23.0, &WeatherMix::settled(WeatherKind::Clear)).sky_top);
        // Storm keylines are night ink, darker than the storm-lit land.
        assert_eq!(unit_to_hex(fog_unit(storm.outline)), 0x10104A);
    }

    /// RL-5: a dusk rain keeps a warm sky band and a warm fill; a noon rain is all rain.
    #[test]
    fn dusk_rain_keeps_its_warmth() {
        let hue = |lin: Vec3| rgb_to_hsv(fog_unit(lin)).x;
        let warm = |hue: f32| !(60.0..=280.0).contains(&hue);
        let dusk = environment(18.5, &WeatherMix::settled(WeatherKind::Rain));
        assert!(warm(hue(dusk.sky_horizon)), "dusk-rain horizon {:06X}", unit_to_hex(fog_unit(dusk.sky_horizon)));
        assert!(dusk.fill_color.x > dusk.fill_color.y, "dusk-rain fill is warm: {}", dusk.fill_color);
        let noon = environment(12.0, &WeatherMix::settled(WeatherKind::Rain));
        assert_eq!(unit_to_hex(fog_unit(noon.sky_horizon)), 0x9CB5FF);
        assert_eq!((warm_hour(12.0), warm_hour(23.0), warm_hour(18.5)), (0.0, 0.0, 1.0));
    }

    fn fog_unit(lin: Vec3) -> Vec3 {
        let e = |v: f32| if v <= 0.0031308 { v * 12.92 } else { 1.055 * v.powf(1.0 / 2.4) - 0.055 };
        Vec3::new(e(lin.x), e(lin.y), e(lin.z))
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
        // A half-blended environment is finite and between the two.
        let mut m = WeatherMix::settled(WeatherKind::Clear);
        m.w = [0.5, 0.0, 0.5, 0.0, 0.0, 0.0];
        let e = environment(12.0, &m);
        assert!(e.sun_color.x < 1.0 && e.sun_color.x > 0.45);
    }

    /// The precipitation box fills the whole view: at the ground and at the top of the fall,
    /// every screen point maps inside it (no central column), at any zoom and pitch.
    #[test]
    fn precipitation_fills_the_view() {
        for (half, pitch) in [(Vec2::new(21.3, 12.0), 35f32), (Vec2::new(63.0, 35.5), 35.0), (Vec2::new(98.0, 55.0), 25.0), (Vec2::new(10.0, 5.6), 55.0)] {
            let (s, c) = (pitch.to_radians().sin(), pitch.to_radians().cos());
            for top in [20.0f32, 24.0] {
                let (off, h) = precip_box(half, s, top);
                assert!(h.x >= half.x, "covers the width");
                for y in [0.0, top] {
                    for sy in [-half.y, 0.0, half.y] {
                        let v = (sy - y * c) / s;
                        assert!(v >= off - h.y - 1e-3 && v <= off + h.y + 1e-3, "{half} {pitch}: screen y {sy} at height {y} uncovered");
                    }
                }
            }
            // Much wider than the old 40 m column at the demo-shot zoom.
            if half.x > 60.0 {
                assert!(precip_box(half, s, 24.0).1.x * 2.0 > 120.0);
            }
        }
        let (off, h) = precip_box(Vec2::NAN, f32::NAN, f32::INFINITY);
        assert!(off.is_finite() && h.is_finite());
    }

    /// Rain leans at least 1 px per 1×4 px streak on screen, whatever the wind's angle to
    /// the view (yaw), and stays upright without wind.
    #[test]
    fn rain_slant_reads_at_low_res() {
        let pitch = 35f32.to_radians();
        let (s, c) = (pitch.sin(), pitch.cos());
        for kind in [2usize, 3] {
            let speed = 18.0 + 4.0 * if kind == 3 { 1.3 } else { 1.0 };
            for yaw in 0..16 {
                let a = yaw as f32 * std::f32::consts::TAU / 16.0;
                let fwd_h = Vec2::new(a.sin(), a.cos());
                let right_h = Vec2::new(-fwd_h.y, fwd_h.x);
                for wa in 0..8 {
                    let wind = Vec2::from_angle(wa as f32 * 0.8) * WIND[kind] * 0.85;
                    let v = rain_velocity(wind, speed, right_h, fwd_h, s);
                    let side = v.dot(right_h).abs();
                    let down = speed * c - v.dot(fwd_h) * s;
                    assert!(down > 0.0, "rain falls down the screen");
                    assert!(side / down >= 1.0 / 3.0, "kind {kind} yaw {yaw} wind {wa}: lean {}", side / down);
                    assert!(v.length() < speed * 2.0, "still mostly falling");
                }
            }
        }
        let calm = rain_velocity(Vec2::ZERO, 22.0, Vec2::X, Vec2::Y, s);
        assert_eq!(calm, Vec2::ZERO);
        assert!(rain_velocity(Vec2::NAN, 22.0, Vec2::X, Vec2::Y, s).is_finite());
    }

    #[test]
    fn focus_on_ground() {
        let f = ground_focus(Vec3::new(10.0, 10.0, 10.0), Vec3::new(-1.0, -1.0, -1.0).normalize());
        assert!(f.distance(Vec3::ZERO) < 1e-3);
    }
}
