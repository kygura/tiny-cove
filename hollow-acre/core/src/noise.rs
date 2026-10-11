//! Tiny deterministic hash + value-noise helpers (no external RNG).

pub fn mix(mut z: u64) -> u64 {
    z = z.wrapping_add(0x9e37_79b9_7f4a_7c15);
    z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    z ^ (z >> 31)
}

/// Hash of a seed and three integers.
pub fn hash(seed: u64, a: i32, b: i32, c: i32) -> u64 {
    mix(seed ^ mix(a as u32 as u64 ^ mix(((b as u32 as u64) << 32) | c as u32 as u64)))
}

/// Uniform in [0, 1).
pub fn rnd(seed: u64, a: i32, b: i32, c: i32) -> f32 {
    (hash(seed, a, b, c) >> 40) as f32 / (1u64 << 24) as f32
}

pub fn smoothstep(e0: f32, e1: f32, x: f32) -> f32 {
    let t = ((x - e0) / (e1 - e0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Smooth 2D value noise in [0, 1), lattice spacing 1.
pub fn value_noise(seed: u64, x: f32, z: f32) -> f32 {
    let (xi, zi) = (x.floor() as i32, z.floor() as i32);
    let (fx, fz) = (x - xi as f32, z - zi as f32);
    let (sx, sz) = (fx * fx * (3.0 - 2.0 * fx), fz * fz * (3.0 - 2.0 * fz));
    let v = |dx, dz| rnd(seed, xi + dx, zi + dz, 0);
    let a = v(0, 0) + (v(1, 0) - v(0, 0)) * sx;
    let b = v(0, 1) + (v(1, 1) - v(0, 1)) * sx;
    a + (b - a) * sz
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rnd_in_range_and_stable() {
        for i in 0..1000 {
            let r = rnd(7, i, -i, 3);
            assert!((0.0..1.0).contains(&r));
            assert_eq!(r, rnd(7, i, -i, 3));
        }
        let n = value_noise(1, 3.7, -2.2);
        assert!((0.0..1.0).contains(&n));
    }
}
