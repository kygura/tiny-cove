//! CPU port of the rain-soaked sand path in `glade.wesl` `fragment()`: the wet rule, the A3 cel
//! bands (`cairn_cove_core::palette`) and the wet-honey band ramp, on the real weather rigs.
//! Checks the DESIGN A8 dusk-rain beach (honey -> terracotta -> rust, apart from the cliffs and
//! the soil), that rain after dark never leaves sand as mud (A0.3), and that drying sand fades
//! back without a snap. The near beach only: no rain haze.

use bevy::math::Vec3;
use cairn_cove::weather::{WeatherKind, WeatherMix, environment};
use cairn_cove_core::palette::{Band, anti_mud, hex_to_unit, hsv_to_rgb, rgb_to_hsv, rot, shade};

const SAND: u32 = 0xF7D68C;
const WET_SAND: u32 = 0xDEA54A;
const DIRT: u32 = 0xD6733A;
/// glade.wesl `HONEY_HUE`.
const HONEY_HUE: f32 = 34.0;
const BANDS: [Band; 3] = [Band::Lit, Band::Mid, Band::Shadow];

struct Rig {
    sun: Vec3,
    fill: Vec3,
    tint: Vec3,
    lit_rot: f32,
    wet: f32,
}

fn rig(hour: f32, kind: WeatherKind, wet: Option<f32>) -> Rig {
    let mut m = WeatherMix::settled(kind);
    if let Some(w) = wet {
        m.wet = w;
    }
    let e = environment(hour, &m);
    Rig { sun: e.sun_color, fill: e.fill_color, tint: e.shadow_tint, lit_rot: e.lit_rot, wet: e.wetness }
}

fn smoothstep(a: f32, b: f32, x: f32) -> f32 {
    let t = ((x - a) / (b - a)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// sRGB albedo after the wet rules: sand mixed toward wet honey by wetness, then S x(1 + 0.15
/// wet); V x(1 - 0.18 wet) except on honey (already as dark as wet sand gets).
fn wet_albedo(hex: u32, honey: bool, wet: f32) -> Vec3 {
    let mut a = hex_to_unit(hex);
    if honey {
        a = a.lerp(hex_to_unit(WET_SAND), wet.clamp(0.0, 1.0));
    }
    if wet > 0.0 {
        let mut w = rgb_to_hsv(a);
        w.y = (w.y * (1.0 + 0.15 * wet)).min(1.0);
        w.z *= 1.0 - if honey { 0.0 } else { 0.18 } * wet;
        a = hsv_to_rgb(w);
    }
    a
}

/// glade.wesl `cel()`: the band colour times the band light, then anti-mud.
fn cel(a: Vec3, band: Band, r: &Rig) -> Vec3 {
    let c = match band {
        Band::Lit => rot(a, r.tint, r.lit_rot) * r.sun.max(r.fill),
        Band::Mid => shade(a, Band::Mid, r.tint) * r.sun.lerp(r.fill, 0.5).max(r.fill),
        Band::Shadow => shade(a, Band::Shadow, r.tint) * r.fill,
    };
    anti_mud(c.min(Vec3::ONE), r.tint)
}

/// The wet-honey ramp in `fragment()` (haze 0).
fn honey(a: Vec3, band: Band, r: &Rig) -> Vec3 {
    let c = cel(a, band, r);
    let warm = smoothstep(0.02, 0.15, (r.sun.x - r.sun.z) / r.sun.x.max(1e-3));
    let mut hv = rgb_to_hsv(cel(a, Band::Lit, r));
    if hv.x < HONEY_HUE || hv.x > 270.0 {
        hv.x = HONEY_HUE;
    }
    let k = r.wet.clamp(0.0, 1.0) * warm * smoothstep(0.50, 0.62, hv.z);
    if k <= 0.0 {
        return c;
    }
    match band {
        Band::Shadow => hv = Vec3::new(hv.x - 15.0, (hv.y * 1.2).min(1.0), hv.z * 0.78),
        Band::Mid => hv = Vec3::new(hv.x - 7.0, hv.y, hv.z * 0.90),
        Band::Lit => {}
    }
    anti_mud(c.lerp(hsv_to_rgb(hv), k), r.tint)
}

fn hue_dist(a: f32, b: f32) -> f32 {
    ((a - b + 180.0).rem_euclid(360.0) - 180.0).abs()
}

/// DESIGN A3 anti-mud: hue 15-100 below V 0.45 is brown / ochre / olive mud.
fn is_mud(c: Vec3) -> bool {
    let h = rgb_to_hsv(c);
    (15.0..100.0).contains(&h.x) && h.z < 0.45
}

fn apart(a: Vec3, b: Vec3, deg: f32) -> bool {
    hue_dist(a.x, b.x) >= deg || (a.z - b.z).abs() >= 0.1
}

#[test]
fn dusk_rain_sand_steps_honey_terracotta_rust() {
    let r = rig(18.5, WeatherKind::Rain, None);
    let a = wet_albedo(SAND, true, r.wet);
    let [lit, mid, shadow] = BANDS.map(|b| rgb_to_hsv(honey(a, b, &r)));
    assert!((30.0..=45.0).contains(&lit.x), "lit wet sand is honey: {lit}");
    // A3 ramp: value steps down, hue walks the warm path, and the shadow stays orange.
    assert!(lit.z > mid.z && mid.z > shadow.z, "{lit} {mid} {shadow}");
    assert!(lit.x > mid.x && mid.x > shadow.x && shadow.x >= 12.0, "{lit} {mid} {shadow}");
    assert!(mid.y > 0.5 && shadow.y > 0.5, "vivid: {mid} {shadow}");
    // Mid sand clears the lit-cliff brick and the oxblood soil the judge measured.
    for hex in [0x944A42, 0x7B2129] {
        assert!(apart(mid, rgb_to_hsv(hex_to_unit(hex)), 15.0), "mid {mid} vs #{hex:06X}");
    }
    // No sand band takes a dirt band's colour under the same rig.
    let dirt = wet_albedo(DIRT, false, r.wet);
    for d in BANDS.map(|b| rgb_to_hsv(cel(dirt, b, &r))) {
        for s in [mid, shadow] {
            assert!(apart(s, d, 10.0), "sand {s} vs dirt {d}");
        }
    }
}

#[test]
fn rain_never_leaves_sand_as_mud() {
    for q in 0..96 {
        let hour = q as f32 * 0.25;
        let r = rig(hour, WeatherKind::Rain, None);
        let a = wet_albedo(SAND, true, r.wet);
        for b in BANDS {
            let c = honey(a, b, &r);
            assert!(!is_mud(c), "{hour}h {b:?}: {}", rgb_to_hsv(c));
        }
    }
    // Moonlit soaked sand is plum (A3 night sand lit #5A3A52), as the dry night's is violet.
    let r = rig(23.0, WeatherKind::Rain, None);
    let lit = rgb_to_hsv(honey(wet_albedo(SAND, true, r.wet), Band::Lit, &r));
    assert!(lit.x >= 300.0, "night rain lit sand {lit}");
}

#[test]
fn noon_rain_sand_is_untouched() {
    let r = rig(12.0, WeatherKind::Rain, None);
    let a = wet_albedo(SAND, true, r.wet);
    for b in BANDS {
        assert_eq!(honey(a, b, &r), cel(a, b, &r), "{b:?}");
    }
}

#[test]
fn drying_sand_fades_without_a_snap() {
    for hour in [6.5, 17.5, 18.5, 19.25, 23.0] {
        for b in BANDS {
            let mut prev: Option<Vec3> = None;
            for i in 0..=100 {
                let wet = i as f32 / 100.0;
                let r = rig(hour, WeatherKind::Clear, Some(wet));
                let c = honey(wet_albedo(SAND, wet > 0.0, wet), b, &r);
                if let Some(p) = prev {
                    let step = (c - p).abs().max_element();
                    assert!(step < 0.04, "{hour}h {b:?} wet {wet}: jump {step} ({p} -> {c})");
                }
                prev = Some(c);
            }
        }
    }
}
