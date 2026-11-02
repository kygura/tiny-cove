//! Particle effects of the ambient life: flames, smoke, fish leaps and splashes, fireflies.
//! Everything here is a pure function of time and inputs, so it is deterministic and testable.

use crate::render::{Particle, ParticleKind};
use crate::weather::WeatherKind;
use bevy::prelude::*;
use cairn_cove_core::{mesh::hash_f, terrain::Terrain};

/// DESIGN.md §Art "Fire and light" / "Water, sky, ink and life" tokens (linear RGB).
pub mod tok {
    use bevy::prelude::Vec3;
    use cairn_cove_core::palette::srgb;
    pub fn flame_white() -> Vec3 { srgb(0xFFF7AD) }
    pub fn glow_core() -> Vec3 { srgb(0xFFE65A) }
    pub fn glow_fire() -> Vec3 { srgb(0xFF7B19) }
    pub fn glow_ember() -> Vec3 { srgb(0xFF3A31) }
    pub fn ember_fade() -> Vec3 { srgb(0x84195A) }
    pub fn smoke() -> Vec3 { srgb(0xE6DEF7) }
    pub fn smoke_shade() -> Vec3 { srgb(0xA59CDE) }
    pub fn smoke_night() -> Vec3 { srgb(0x5A4A9C) }
    pub fn firefly() -> Vec3 { srgb(0xDEFF5A) }
    pub fn gull() -> Vec3 { srgb(0xF7EFDE) }
    pub fn gull_tip() -> Vec3 { srgb(0x31217B) }
    pub fn crab() -> Vec3 { srgb(0xFF5A31) }
    pub fn fish() -> Vec3 { srgb(0x9CEFFF) }
    pub fn foam() -> Vec3 { srgb(0xFFFFFF) }
}

/// Flames per fire (DESIGN §A7: 6-10) and the life of one flame particle (s).
pub const FLAMES: usize = 8;
pub const FLAME_LIFE: f32 = 0.6;
/// A leap lasts this long (s), then its landing splash ring spreads for [`RING_LIFE`].
pub const LEAP_TIME: f32 = 0.9;
pub const RING_LIFE: f32 = 0.7;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FireKind {
    /// A basket on top of a beacon tower.
    Beacon,
    /// A firepit on the ground.
    Pit,
}

/// A `Fire` marker well above the ground is a beacon basket, else a firepit.
pub fn fire_kind(pos: Vec3, terrain: &Terrain) -> FireKind {
    if pos.y > terrain.height(pos.xz()) + 2.0 { FireKind::Beacon } else { FireKind::Pit }
}

/// Beacon baskets burn 19:30-06:00 and in storms and sea fog; firepits 18:00-07:00.
pub fn fire_lit(kind: FireKind, hour: f32, weather: WeatherKind) -> bool {
    let h = hour.rem_euclid(24.0);
    match kind {
        FireKind::Beacon => h >= 19.5 || h < 6.0 || matches!(weather, WeatherKind::Storm | WeatherKind::Fog),
        FireKind::Pit => h >= 18.0 || h < 7.0,
    }
}

/// Flame life stage 0..=3: flame_white, glow_core, glow_fire, glow_ember in four equal steps.
pub fn flame_color(stage: usize) -> Vec3 {
    match stage {
        0 => tok::flame_white(),
        1 => tok::glow_core(),
        2 => tok::glow_fire(),
        _ => tok::glow_ember(),
    }
}

/// Hot flame quads plus two rising sparks for one fire at `pos`.
pub fn flame_particles(pos: Vec3, kind: FireKind, t: f32, salt: f32, wind: Vec2, out: &mut Vec<Particle>) {
    let k = if kind == FireKind::Beacon { 1.5 } else { 1.0 };
    // Stepped ~8 Hz flicker, never smoothed.
    let flick = 0.85 + 0.3 * hash_f(0x51, (t * 8.0).floor(), salt);
    for i in 0..FLAMES {
        let u = (t / FLAME_LIFE + i as f32 / FLAMES as f32 + salt * 0.37).fract();
        let a = i as f32 * 2.4 + salt;
        let r = 0.14 * k * (1.0 - u);
        let p = pos + Vec3::new(a.cos() * r + (t * 9.0 + a).sin() * 0.03, u * 0.75 * k, a.sin() * r);
        let size = (0.46 - 0.24 * u) * k * flick;
        out.push(Particle::new(ParticleKind::Spark, p, size, flame_color((u * 4.0) as usize).extend(1.0)));
    }
    for j in 0..2 {
        let u = (t / 1.4 + j as f32 * 0.5 + salt * 0.21).fract();
        let drift = wind * u * 1.6;
        let p = pos + Vec3::new(drift.x + (u * 7.0 + j as f32).sin() * 0.15, 0.5 * k + u * 1.8, drift.y);
        out.push(Particle::new(ParticleKind::Spark, p, 0.15, tok::glow_ember().lerp(tok::ember_fade(), u).extend(1.0 - u)));
    }
}

/// `n` smoke puffs (2x2 px class) rising from `pos`, drifting with the wind, Bayer-faded over ~3 s.
#[allow(clippy::too_many_arguments)]
pub fn smoke_particles(pos: Vec3, n: usize, scale: f32, t: f32, salt: f32, wind: Vec2, night: f32, out: &mut Vec<Particle>) {
    for i in 0..n {
        let age = (t / 3.0 + i as f32 / n as f32 + salt * 0.37).fract();
        let drift = wind * age * 2.2;
        let p = pos + Vec3::new(drift.x + (age * 9.0 + i as f32).sin() * 0.12, age * 3.2, drift.y);
        let c = tok::smoke().lerp(tok::smoke_shade(), age * 0.7).lerp(tok::smoke_night(), night);
        out.push(Particle::new(ParticleKind::Puff, p, (0.35 + age * 0.9) * scale, c.extend((1.3 - 1.2 * age).clamp(0.0, 1.0))));
    }
}

/// Position of a leaping fish `age` seconds after takeoff: a parabola from `from` along `dir`.
pub fn leap_pos(from: Vec2, dir: Vec2, level: f32, age: f32) -> Vec3 {
    let u = (age / LEAP_TIME).clamp(0.0, 1.0);
    let flat = from + dir * (u * 2.4);
    Vec3::new(flat.x, level + 4.0 * u * (1.0 - u) * 1.1, flat.y)
}

/// Fish arc (silver-cyan body with a short trail) and the takeoff / landing splash rings.
pub fn leap_particles(from: Vec2, dir: Vec2, level: f32, age: f32, out: &mut Vec<Particle>) {
    if age < LEAP_TIME {
        for k in 0..4 {
            let a = age - k as f32 * 0.04;
            if a < 0.0 {
                continue;
            }
            let fade = 1.0 - k as f32 * 0.22;
            out.push(Particle::new(ParticleKind::Spark, leap_pos(from, dir, level, a), 0.3 * fade, tok::fish().extend(fade)));
        }
    }
    if age < 0.5 {
        ring(from, level, age, out);
    }
    if age > LEAP_TIME {
        ring(leap_pos(from, dir, level, LEAP_TIME).xz(), level, age - LEAP_TIME, out);
    }
}

fn ring(c: Vec2, level: f32, age: f32, out: &mut Vec<Particle>) {
    if age >= RING_LIFE {
        return;
    }
    let (r, fade) = (0.15 + age * 1.6, 1.0 - age / RING_LIFE);
    for i in 0..8 {
        let a = i as f32 / 8.0 * std::f32::consts::TAU;
        let p = Vec3::new(c.x + a.cos() * r, level + 0.05 + age * 0.3 * fade, c.y + a.sin() * r);
        out.push(Particle::new(ParticleKind::Puff, p, 0.28, tok::foam().extend(fade)));
    }
}

/// Blinking fireflies (3 per home) over grass; `night` fades them in.
pub fn firefly_particles(homes: &[Vec3], t: f32, night: f32, out: &mut Vec<Particle>) {
    for (h, home) in homes.iter().enumerate() {
        for i in 0..3 {
            let s = h as f32 * 3.1 + i as f32 * 1.9;
            let p = *home
                + Vec3::new(
                    (t * 0.31 + s).sin() * 1.1 + (t * 0.7 + s * 2.0).sin() * 0.3,
                    0.5 + 0.4 * (t * 0.5 + s * 1.3).sin(),
                    (t * 0.27 + s * 1.7).cos() * 1.1,
                );
            let blink = (0.5 + 0.7 * (t * 1.7 + s * 2.3).sin()).clamp(0.0, 1.0);
            out.push(Particle::new(ParticleKind::Spark, p, 0.3, tok::firefly().extend(blink * night)));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn finite(ps: &[Particle]) -> bool {
        ps.iter().all(|p| p.pos.is_finite() && p.size.is_finite() && p.color.is_finite())
    }

    #[test]
    fn beacons_burn_at_night_storm_and_fog_pits_evening_to_morning() {
        use FireKind::*;
        use WeatherKind::*;
        assert!(!fire_lit(Beacon, 12.0, Clear));
        assert!(fire_lit(Beacon, 23.0, Clear) && fire_lit(Beacon, 3.0, Clear));
        assert!(fire_lit(Beacon, 12.0, Storm) && fire_lit(Beacon, 12.0, Fog));
        assert!(!fire_lit(Beacon, 12.0, Rain));
        assert!(!fire_lit(Pit, 12.0, Storm), "pits ignore weather");
        assert!(fire_lit(Pit, 18.5, Clear) && fire_lit(Pit, 6.5, Clear) && !fire_lit(Pit, 7.5, Clear));
        assert!(!fire_lit(Beacon, 18.5, Clear), "beacons wait for 19:30");
    }

    #[test]
    fn flames_step_through_hot_colours() {
        assert_eq!(flame_color(0), tok::flame_white());
        assert_eq!(flame_color(3), tok::glow_ember());
        let mut ps = Vec::new();
        flame_particles(Vec3::new(1.0, 2.0, 3.0), FireKind::Pit, 1.234, 0.5, Vec2::new(1.0, 0.0), &mut ps);
        assert_eq!(ps.len(), FLAMES + 2);
        assert!(finite(&ps));
        // Every flame is hot: red channel dominant, never grey.
        assert!(ps.iter().all(|p| p.color.x >= p.color.z));
        // Deterministic.
        let mut again = Vec::new();
        flame_particles(Vec3::new(1.0, 2.0, 3.0), FireKind::Pit, 1.234, 0.5, Vec2::new(1.0, 0.0), &mut again);
        assert_eq!(ps, again);
    }

    #[test]
    fn smoke_drifts_downwind_and_rises() {
        let (mut calm, mut windy) = (Vec::new(), Vec::new());
        smoke_particles(Vec3::ZERO, 8, 1.0, 5.0, 0.0, Vec2::ZERO, 0.0, &mut calm);
        smoke_particles(Vec3::ZERO, 8, 1.0, 5.0, 0.0, Vec2::new(4.0, 0.0), 0.0, &mut windy);
        assert!(finite(&calm) && finite(&windy) && calm.len() == 8);
        let mean = |v: &[Particle]| v.iter().map(|p| p.pos.x).sum::<f32>() / v.len() as f32;
        assert!(mean(&windy) > mean(&calm) + 0.5);
        assert!(calm.iter().all(|p| p.pos.y >= 0.0 && p.color.w >= 0.0 && p.color.w <= 1.0));
    }

    #[test]
    fn leap_is_an_arc_that_returns_to_the_sea() {
        let (from, dir) = (Vec2::new(3.0, 4.0), Vec2::X);
        assert!((leap_pos(from, dir, 0.1, 0.0).y - 0.1).abs() < 1e-5);
        assert!((leap_pos(from, dir, 0.1, LEAP_TIME).y - 0.1).abs() < 1e-5);
        let top = leap_pos(from, dir, 0.1, LEAP_TIME * 0.5);
        assert!(top.y > 0.1 + 0.9 && top.x > from.x);
        for i in 0..40 {
            let age = i as f32 * 0.05;
            let mut ps = Vec::new();
            leap_particles(from, dir, 0.1, age, &mut ps);
            assert!(finite(&ps));
            assert_eq!(ps.is_empty(), age >= LEAP_TIME + RING_LIFE, "age {age}");
        }
    }

    #[test]
    fn fireflies_fade_with_night() {
        let homes = [Vec3::new(1.0, 1.0, 1.0), Vec3::new(-2.0, 1.0, 3.0)];
        let mut ps = Vec::new();
        firefly_particles(&homes, 3.0, 1.0, &mut ps);
        assert_eq!(ps.len(), 6);
        assert!(finite(&ps) && ps.iter().all(|p| p.color.w <= 1.0));
        let mut day = Vec::new();
        firefly_particles(&homes, 3.0, 0.0, &mut day);
        assert!(day.iter().all(|p| p.color.w == 0.0));
    }
}
