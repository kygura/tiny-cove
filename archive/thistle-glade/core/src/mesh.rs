//! Plain mesh buffers and small geometry helpers shared by every generator.
//!
//! Conventions:
//! - Counter-clockwise front faces (seen from outside), normals point out. Flat shading is
//!   preferred (PS1 look): helpers emit unshared vertices per face.
//! - `uv` is in metres of surface (1.0 = one texture tile repeat per metre unless the helper
//!   takes a scale). The renderer wraps it with `fract` inside the atlas tile `Surface`.
//! - `color` is a linear RGB tint multiplied with the texture; alpha unused (1.0).

use glam::{Vec2, Vec3};

/// Atlas tile / surface type. The renderer draws one procedural texture per variant.
#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Surface {
    Grass = 0,
    Dirt,
    Rock,
    Sand,
    /// Rough fieldstone blocks (walls).
    Stone,
    /// Darker big-block foundation stone.
    Foundation,
    /// Smooth lime plaster.
    Plaster,
    /// Vertical planks.
    Planks,
    /// Timber beam (half-timbering, posts, rails).
    Timber,
    /// Clay roof tiles (rows).
    RoofTile,
    /// Slate shingles.
    Slate,
    /// Straw thatch.
    Thatch,
    Cobble,
    Flagstone,
    Setts,
    /// Window glass with mullions. Emissive at night.
    Window,
    /// Plank door with iron straps.
    Door,
    Bark,
    Leaves,
    /// Rope / cloth / hay — fibrous.
    Fiber,
    /// Flower petals / small plants: flat colour with a little speckle.
    Petal,
    /// Metal (lantern frames, hinges).
    Metal,
    /// Lantern glass / flame. Always emissive.
    Glow,
    /// Water surface (only used in the water mesh).
    Water,
}

impl Surface {
    pub const COUNT: usize = Surface::Water as usize + 1;
}

/// Mesh buffers. Indices are `u32` triangles.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct MeshData {
    pub positions: Vec<[f32; 3]>,
    pub normals: Vec<[f32; 3]>,
    pub uvs: Vec<[f32; 2]>,
    pub colors: Vec<[f32; 4]>,
    /// Per-vertex `Surface as u8`.
    pub surfaces: Vec<u8>,
    pub indices: Vec<u32>,
}

impl MeshData {
    pub fn is_empty(&self) -> bool {
        self.indices.is_empty()
    }

    pub fn vertex_count(&self) -> usize {
        self.positions.len()
    }

    /// Appends another mesh.
    pub fn append(&mut self, other: &MeshData) {
        let base = self.positions.len() as u32;
        self.positions.extend_from_slice(&other.positions);
        self.normals.extend_from_slice(&other.normals);
        self.uvs.extend_from_slice(&other.uvs);
        self.colors.extend_from_slice(&other.colors);
        self.surfaces.extend_from_slice(&other.surfaces);
        self.indices.extend(other.indices.iter().map(|i| i + base));
    }

    /// Pushes one vertex and returns its index.
    pub fn vertex(&mut self, p: Vec3, n: Vec3, uv: Vec2, color: Vec3, s: Surface) -> u32 {
        let i = self.positions.len() as u32;
        self.positions.push(p.to_array());
        self.normals.push(n.to_array());
        self.uvs.push(uv.to_array());
        self.colors.push([color.x, color.y, color.z, 1.0]);
        self.surfaces.push(s as u8);
        i
    }

    /// Flat triangle `a, b, c` (counter-clockwise = front). UVs are planar-projected using
    /// the face normal, scaled by `uv_scale` (tiles per metre).
    pub fn tri(&mut self, a: Vec3, b: Vec3, c: Vec3, color: Vec3, s: Surface, uv_scale: f32) {
        let n = (b - a).cross(c - a).normalize_or_zero();
        let (u, v) = tangent_frame(n);
        let base = self.positions.len() as u32;
        for p in [a, b, c] {
            let uv = Vec2::new(p.dot(u), p.dot(v)) * uv_scale;
            self.vertex(p, n, uv, color, s);
        }
        self.indices.extend_from_slice(&[base, base + 1, base + 2]);
    }

    /// Flat quad `a, b, c, d` in counter-clockwise order. UVs: `a`=(0,0), `b`=(w,0),
    /// `c`=(w,h), `d`=(0,h) where w = |b-a|, h = |d-a|, times `uv_scale`.
    /// Use this for walls/roofs so textures run along the surface.
    pub fn quad(&mut self, a: Vec3, b: Vec3, c: Vec3, d: Vec3, color: Vec3, s: Surface, uv_scale: f32) {
        let n = (b - a).cross(d - a).normalize_or_zero();
        let w = (b - a).length() * uv_scale;
        let h = (d - a).length() * uv_scale;
        let base = self.positions.len() as u32;
        self.vertex(a, n, Vec2::new(0.0, 0.0), color, s);
        self.vertex(b, n, Vec2::new(w, 0.0), color, s);
        self.vertex(c, n, Vec2::new(w, h), color, s);
        self.vertex(d, n, Vec2::new(0.0, h), color, s);
        self.indices.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
    }

    /// Same as [`quad`](Self::quad) but with explicit UVs per corner.
    #[allow(clippy::too_many_arguments)]
    pub fn quad_uv(&mut self, p: [Vec3; 4], uv: [Vec2; 4], color: Vec3, s: Surface) {
        let n = (p[1] - p[0]).cross(p[3] - p[0]).normalize_or_zero();
        let base = self.positions.len() as u32;
        for k in 0..4 {
            self.vertex(p[k], n, uv[k], color, s);
        }
        self.indices.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
    }

    /// Oriented box. `center` is the box centre, `half` the half extents along the local
    /// axes `x_axis` (horizontal, unit) and +y; the local z axis is `x_axis × y` rotated.
    /// All six faces, flat shaded.
    pub fn oriented_box(&mut self, center: Vec3, half: Vec3, x_axis: Vec3, color: Vec3, s: Surface, uv_scale: f32) {
        let x = x_axis.normalize_or_zero() * half.x;
        let y = Vec3::Y * half.y;
        let z = x_axis.normalize_or_zero().cross(Vec3::Y).normalize_or_zero() * half.z;
        let c = |sx: f32, sy: f32, sz: f32| center + x * sx + y * sy + z * sz;
        // +x, -x, +y, -y, +z, -z faces (counter-clockwise seen from outside)
        self.quad(c(1., -1., -1.), c(1., 1., -1.), c(1., 1., 1.), c(1., -1., 1.), color, s, uv_scale);
        self.quad(c(-1., -1., 1.), c(-1., 1., 1.), c(-1., 1., -1.), c(-1., -1., -1.), color, s, uv_scale);
        self.quad(c(-1., 1., -1.), c(-1., 1., 1.), c(1., 1., 1.), c(1., 1., -1.), color, s, uv_scale);
        self.quad(c(-1., -1., 1.), c(-1., -1., -1.), c(1., -1., -1.), c(1., -1., 1.), color, s, uv_scale);
        self.quad(c(1., -1., 1.), c(1., 1., 1.), c(-1., 1., 1.), c(-1., -1., 1.), color, s, uv_scale);
        self.quad(c(-1., -1., -1.), c(-1., 1., -1.), c(1., 1., -1.), c(1., -1., -1.), color, s, uv_scale);
    }

    /// Axis-aligned box from `min` to `max`.
    pub fn aabb(&mut self, min: Vec3, max: Vec3, color: Vec3, s: Surface, uv_scale: f32) {
        self.oriented_box((min + max) * 0.5, (max - min) * 0.5, Vec3::X, color, s, uv_scale);
    }

    /// Box spanning the segment `a`→`b` (its centre line), with given width (horizontal,
    /// perpendicular to the segment) and height (vertical, measured from the segment up).
    /// Good for beams, rails, planks. For vertical segments use `oriented_box`.
    pub fn beam(&mut self, a: Vec3, b: Vec3, width: f32, height: f32, color: Vec3, s: Surface, uv_scale: f32) {
        let d = b - a;
        let len = d.length();
        if len < 1e-5 {
            return;
        }
        let dir = d / len;
        let side = dir.cross(Vec3::Y).normalize_or(Vec3::X) * (width * 0.5);
        let up = side.cross(dir).normalize_or(Vec3::Y) * height;
        let p = |t: Vec3, sx: f32, sy: f32| t + side * sx + up * sy;
        let (a0, a1, a2, a3) = (p(a, -1., 0.), p(a, 1., 0.), p(a, 1., 1.), p(a, -1., 1.));
        let (b0, b1, b2, b3) = (p(b, -1., 0.), p(b, 1., 0.), p(b, 1., 1.), p(b, -1., 1.));
        self.quad(a1, b1, b2, a2, color, s, uv_scale);
        self.quad(b0, a0, a3, b3, color, s, uv_scale);
        self.quad(a3, a2, b2, b3, color, s, uv_scale);
        self.quad(a0, b0, b1, a1, color, s, uv_scale);
        self.quad(a0, a1, a2, a3, color, s, uv_scale);
        self.quad(b1, b0, b3, b2, color, s, uv_scale);
    }

    /// Vertical prism (cylinder approximation) with `sides` facets, from `base` up by
    /// `height`, radii `r0` at the bottom and `r1` at the top (cone if `r1` = 0).
    /// `cap_top`/`cap_bottom` add flat caps.
    #[allow(clippy::too_many_arguments)]
    pub fn cylinder(
        &mut self,
        base: Vec3,
        r0: f32,
        r1: f32,
        height: f32,
        sides: u32,
        cap_top: bool,
        cap_bottom: bool,
        color: Vec3,
        s: Surface,
        uv_scale: f32,
    ) {
        let sides = sides.max(3);
        let top = base + Vec3::Y * height;
        let ring = |r: f32, c: Vec3, i: u32| {
            let a = i as f32 / sides as f32 * std::f32::consts::TAU;
            c + Vec3::new(a.cos() * r, 0.0, a.sin() * r)
        };
        let circ = std::f32::consts::TAU * r0.max(r1) / sides as f32;
        for i in 0..sides {
            let (b0, b1) = (ring(r0, base, i), ring(r0, base, i + 1));
            let (t0, t1) = (ring(r1, top, i), ring(r1, top, i + 1));
            let u0 = i as f32 * circ * uv_scale;
            let u1 = (i + 1) as f32 * circ * uv_scale;
            let h = height * uv_scale;
            if r1 > 1e-4 {
                self.quad_uv(
                    [b1, b0, t0, t1],
                    [Vec2::new(u1, 0.0), Vec2::new(u0, 0.0), Vec2::new(u0, h), Vec2::new(u1, h)],
                    color,
                    s,
                );
            } else {
                self.tri(b1, b0, top, color, s, uv_scale);
            }
            if cap_top && r1 > 1e-4 {
                self.tri(top, t1, t0, color, s, uv_scale);
            }
            if cap_bottom {
                self.tri(base, b0, b1, color, s, uv_scale);
            }
        }
    }

    /// Fan-triangulated flat polygon (convex or star-shaped around its centroid) at its own
    /// heights, facing +y if `up`, else -y. Points in counter-clockwise order seen from above.
    pub fn polygon(&mut self, pts: &[Vec3], up: bool, color: Vec3, s: Surface, uv_scale: f32) {
        if pts.len() < 3 {
            return;
        }
        let c = pts.iter().copied().sum::<Vec3>() / pts.len() as f32;
        for i in 0..pts.len() {
            let a = pts[i];
            let b = pts[(i + 1) % pts.len()];
            if up {
                self.tri(c, b, a, color, s, uv_scale);
            } else {
                self.tri(c, a, b, color, s, uv_scale);
            }
        }
    }

    /// Bounding box (min, max). `None` if empty.
    pub fn bounds(&self) -> Option<(Vec3, Vec3)> {
        let mut it = self.positions.iter().map(|p| Vec3::from_array(*p));
        let first = it.next()?;
        Some(it.fold((first, first), |(lo, hi), p| (lo.min(p), hi.max(p))))
    }

    /// True if every position/normal/uv is finite and all indices are in range.
    pub fn is_valid(&self) -> bool {
        let n = self.positions.len();
        self.normals.len() == n
            && self.uvs.len() == n
            && self.colors.len() == n
            && self.surfaces.len() == n
            && self.indices.len() % 3 == 0
            && self.indices.iter().all(|&i| (i as usize) < n)
            && self.positions.iter().flatten().all(|v| v.is_finite())
            && self.normals.iter().flatten().all(|v| v.is_finite())
            && self.uvs.iter().flatten().all(|v| v.is_finite())
    }
}

/// Two unit vectors spanning the plane with normal `n` (for planar UVs).
pub fn tangent_frame(n: Vec3) -> (Vec3, Vec3) {
    let helper = if n.y.abs() > 0.9 { Vec3::X } else { Vec3::Y };
    let u = helper.cross(n).normalize_or(Vec3::X);
    let v = n.cross(u).normalize_or(Vec3::Z);
    // Keep "up" textures upright on vertical faces.
    if n.y.abs() <= 0.9 { (-u, v) } else { (u, v) }
}

/// Things the Bevy side turns into lights, particles or AI targets.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Marker {
    pub kind: MarkerKind,
    pub pos: Vec3,
    /// Facing direction (unit) where meaningful (door outward, window normal), else +y.
    pub dir: Vec3,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum MarkerKind {
    /// Warm point light (lantern, lantern post, hanging lantern).
    Lantern,
    /// A window that glows at night (light leaking out).
    WindowLight,
    /// Chimney top: smoke emitter.
    ChimneySmoke,
    /// In front of a door: where villagers / sheep may gather.
    Door,
    /// Roof ridge / wall top / tower top: birds land here.
    Perch,
    /// Open water: ducks swim here.
    Pond,
    /// Flag that should flutter (renderer may animate); pos = top of pole.
    Flag,
}

/// Deterministic hash of a seed and a position (decoration randomness).
pub fn hash2(seed: u64, x: f32, z: f32) -> u64 {
    let xi = (x * 64.0).round() as i64 as u64;
    let zi = (z * 64.0).round() as i64 as u64;
    let mut h = seed ^ 0x9E37_79B9_7F4A_7C15;
    for v in [xi, zi] {
        h ^= v.wrapping_mul(0xBF58_476D_1CE4_E5B9);
        h = h.rotate_left(27).wrapping_mul(0x94D0_49BB_1331_11EB);
    }
    h ^ (h >> 31)
}

/// Hash → float in [0, 1).
pub fn hash_f(seed: u64, x: f32, z: f32) -> f32 {
    (hash2(seed, x, z) >> 40) as f32 / (1u64 << 24) as f32
}

/// Small deterministic RNG (SplitMix64) seeded from a hash.
#[derive(Clone, Debug)]
pub struct Rng(pub u64);

impl Rng {
    pub fn at(seed: u64, x: f32, z: f32) -> Self {
        Self(hash2(seed, x, z))
    }
    pub fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }
    /// Uniform in [0, 1).
    pub fn f(&mut self) -> f32 {
        (self.next_u64() >> 40) as f32 / (1u64 << 24) as f32
    }
    /// Uniform in [lo, hi).
    pub fn range(&mut self, lo: f32, hi: f32) -> f32 {
        lo + (hi - lo) * self.f()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn helpers_emit_valid_meshes() {
        let mut m = MeshData::default();
        m.aabb(Vec3::ZERO, Vec3::ONE, Vec3::ONE, Surface::Stone, 1.0);
        m.beam(Vec3::ZERO, Vec3::new(3.0, 1.0, 0.0), 0.2, 0.2, Vec3::ONE, Surface::Timber, 1.0);
        m.cylinder(Vec3::ZERO, 1.0, 0.0, 2.0, 8, false, true, Vec3::ONE, Surface::Slate, 1.0);
        m.cylinder(Vec3::ZERO, 1.0, 1.0, 2.0, 8, true, true, Vec3::ONE, Surface::Stone, 1.0);
        assert!(m.is_valid());
        assert!(!m.is_empty());
        // Box top normal faces up.
        let mut b = MeshData::default();
        b.aabb(Vec3::ZERO, Vec3::ONE, Vec3::ONE, Surface::Stone, 1.0);
        assert!(b.normals.iter().any(|n| n[1] > 0.99));
        assert!(b.normals.iter().any(|n| n[1] < -0.99));
        // Every face of a box points away from its centre (outward winding).
        for (p, n) in b.positions.iter().zip(&b.normals) {
            assert!((Vec3::from_array(*p) - Vec3::splat(0.5)).dot(Vec3::from_array(*n)) > 0.0);
        }
    }

    #[test]
    fn hash_is_deterministic() {
        assert_eq!(hash2(7, 1.5, -2.25), hash2(7, 1.5, -2.25));
        assert_ne!(hash2(7, 1.5, -2.25), hash2(8, 1.5, -2.25));
        let f = hash_f(3, 0.1, 0.2);
        assert!((0.0..1.0).contains(&f));
    }
}
