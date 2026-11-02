//! The open sea (DESIGN A6) and its horizon.
//!
//! - One big grid mesh ([`sea_mesh`]) drawn with [`SeaMaterial`]. Its vertices are moved in
//!   the vertex shader to `tide + waves`, so the CPU mesh is static and flat at y = 0.
//! - The sea-bed heightfield ([`heightfield`]) is sampled from the built scene's terrain into
//!   an `R32Float` texture whenever the scene changes; the sea and lagoon shaders read it for
//!   depth bands, foam contours and wave damping near the shore.
//! - [`Horizon`] says where the open sea is cut for the sky: never inside the buildable square.
//! - [`flicker`] / [`FIRE_LUMENS`] are the fire light's stepped flicker and strength; the one
//!   writer of the light pool is `world::update_lanterns` (by `world::warm_light`).

use super::{GladeAssets, SeaField};
use crate::world::BuiltScene;
use bevy::{
    asset::RenderAssetUsages,
    image::{Image, ImageSampler},
    light::NotShadowCaster,
    prelude::*,
    render::render_resource::{Extent3d, TextureDimension, TextureFormat},
};
use cairn_cove_core::{GLADE_HALF, MeshData, Surface, terrain::Terrain};

/// Half size of the sea plane (m). Far beyond the island so the view never sees its edge.
pub const SEA_HALF: f32 = 150.0;
/// Grid cell size of the sea plane (m).
pub const SEA_CELL: f32 = 1.5;
/// Heightfield half extent (m) and spacing (m).
pub const FIELD_HALF: f32 = 48.0;
pub const FIELD_SPACING: f32 = 0.5;
/// Fire light intensity (lumens before the night factor), cf. `world::LANTERN_LUMENS`.
pub const FIRE_LUMENS: f32 = 1800.0;

/// Horizon distance (m) before any scene is built: the buildable square's far corner.
pub const DEFAULT_HORIZON: f32 = (GLADE_HALF + BUILD_MARGIN) * std::f32::consts::SQRT_2;
/// Sea kept behind the farthest land / shallows (foam rings, the turquoise wedge) before the
/// horizon line cuts the open sea for the sky.
const HORIZON_MARGIN: f32 = 6.0;
/// Sea kept around the buildable square (`±GLADE_HALF`): canoes, jetties and stilt huts
/// built at its edge never stand on sky.
pub const BUILD_MARGIN: f32 = 2.0;
/// Seabed height above which a sample counts as shore (the shallow band, DESIGN A6).
const SHORE_DEPTH: f32 = -1.5;

/// Where the open sea ends and the post pass paints the sky (DESIGN A4/A8: sky above the
/// sea line). Holds the land and shallow-water points of the built terrain; the horizon sits
/// [`HORIZON_MARGIN`] behind the farthest of them along the camera's ground-forward axis, and
/// never nearer than the far edge of the buildable square plus [`BUILD_MARGIN`], so the
/// island, its shallows, the sea stacks and anything built on the glade's sea stay in front
/// of it at any yaw, pan and zoom.
#[derive(Resource, Clone, Debug, Default, PartialEq)]
pub struct Horizon {
    pub points: Vec<Vec2>,
}

impl Horizon {
    /// Shore points (1 m grid) of a heightfield from [`heightfield`].
    pub fn from_heightfield(heights: &[f32], field: Vec4) -> Self {
        let n = if field.w.is_finite() { field.w.max(0.0) as usize } else { 0 };
        let step = ((1.0 / field.z.max(1e-3)).round() as usize).max(1);
        let points = (0..n.saturating_mul(n).min(heights.len()))
            .filter(|i| (i % n) % step == 0 && (i / n) % step == 0 && heights[*i] > SHORE_DEPTH)
            .map(|i| Vec2::new(field.x + (i % n) as f32 * field.z, field.y + (i / n) as f32 * field.z))
            .filter(|p| p.is_finite())
            .collect();
        Self { points }
    }

    /// Horizon distance (m from the island centre) for a camera looking along `fwd` (its
    /// ground projection is used).
    pub fn distance(&self, fwd: Vec3) -> f32 {
        let f = Vec2::new(fwd.x, fwd.z).normalize_or(Vec2::NEG_Y);
        let f = if f.is_finite() { f } else { Vec2::NEG_Y };
        // Farthest corner of the buildable square (+ margin) along the view.
        let square = (GLADE_HALF + BUILD_MARGIN) * (f.x.abs() + f.y.abs());
        let shore = self.points.iter().map(|p| p.dot(f) + HORIZON_MARGIN).fold(f32::NEG_INFINITY, f32::max);
        shore.max(square).clamp(12.0, SEA_HALF - 10.0)
    }
}

/// Marker for the sea entity.
#[derive(Component)]
pub struct Sea;

/// Flat sea grid (Surface::Water, up normals, uv = xz / 2) centred on the origin.
pub fn sea_mesh(half: f32, cell: f32) -> MeshData {
    let mut m = MeshData::default();
    let half = if half.is_finite() { half.clamp(1.0, 2000.0) } else { SEA_HALF };
    let cell = if cell.is_finite() { cell.clamp(0.25, half) } else { SEA_CELL };
    let n = (2.0 * half / cell).ceil() as u32;
    let step = 2.0 * half / n as f32;
    for j in 0..=n {
        for i in 0..=n {
            let p = Vec3::new(-half + i as f32 * step, 0.0, -half + j as f32 * step);
            m.vertex(p, Vec3::Y, Vec2::new(p.x, p.z) * 0.5, Vec3::ONE, Surface::Water);
        }
    }
    let row = n + 1;
    for j in 0..n {
        for i in 0..n {
            let a = j * row + i;
            let (b, c, d) = (a + 1, a + row, a + row + 1);
            // Counter-clockwise seen from above (+y): a, c, b and b, c, d.
            m.indices.extend_from_slice(&[a, c, b, b, c, d]);
        }
    }
    m
}

/// Sea-bed heights sampled from `terrain` on a square grid. Returns the row-major heights
/// and the shader placement `(min x, min z, spacing, samples per side)`.
pub fn heightfield(terrain: &Terrain) -> (Vec<f32>, Vec4) {
    let n = (2.0 * FIELD_HALF / FIELD_SPACING).round() as usize + 1;
    let mut h = Vec::with_capacity(n * n);
    for j in 0..n {
        for i in 0..n {
            let p = Vec2::new(-FIELD_HALF + i as f32 * FIELD_SPACING, -FIELD_HALF + j as f32 * FIELD_SPACING);
            let y = terrain.height(p);
            h.push(if y.is_finite() { y } else { -24.0 });
        }
    }
    (h, Vec4::new(-FIELD_HALF, -FIELD_HALF, FIELD_SPACING, n as f32))
}

/// R32Float image of `n × n` heights (deep sea when `heights` is short).
pub fn heightfield_image(heights: &[f32], n: u32) -> Image {
    let n = n.max(2);
    let data: Vec<u8> = (0..(n * n) as usize).flat_map(|i| heights.get(i).copied().unwrap_or(-24.0).to_le_bytes()).collect();
    let mut img = Image::new(
        Extent3d { width: n, height: n, depth_or_array_layers: 1 },
        TextureDimension::D2,
        data,
        TextureFormat::R32Float,
        RenderAssetUsages::RENDER_WORLD | RenderAssetUsages::MAIN_WORLD,
    );
    img.sampler = ImageSampler::nearest();
    img
}

/// Stepped fire flicker multiplier (±12 %, 8 Hz) for light `i` at time `t`.
pub fn flicker(t: f32, i: u32) -> f32 {
    let step = (t * 8.0).floor() as i64 as u64;
    let mut h = step.wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ (i as u64).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    h ^= h >> 29;
    h = h.wrapping_mul(0x94D0_49BB_1331_11EB);
    h ^= h >> 32;
    let r = (h & 0xffff) as f32 / 65535.0;
    // Four hard levels between -12 % and +12 %.
    1.0 + 0.12 * ((r * 4.0).floor().min(3.0) / 1.5 - 1.0)
}

pub(super) fn spawn_sea(mut commands: Commands, assets: Res<GladeAssets>, mut meshes: ResMut<Assets<Mesh>>) {
    commands.spawn((
        Sea,
        Mesh3d(meshes.add(super::to_bevy_mesh(&sea_mesh(SEA_HALF, SEA_CELL)))),
        MeshMaterial3d(assets.sea.clone()),
        NotShadowCaster,
        Transform::default(),
    ));
}

/// Rebuilds the heightfield texture whenever the scene (terrain) changes.
pub(super) fn update_heightfield(
    built: Option<Res<BuiltScene>>,
    assets: Option<Res<GladeAssets>>,
    mut images: ResMut<Assets<Image>>,
    mut field: ResMut<SeaField>,
    mut horizon: ResMut<Horizon>,
) {
    let (Some(built), Some(assets)) = (built, assets) else { return };
    let Some(scene) = built.0.as_ref().filter(|_| built.is_changed()) else { return };
    let (h, f) = heightfield(&scene.terrain);
    let r = Horizon::from_heightfield(&h, f);
    if *horizon != r {
        *horizon = r;
    }
    if let Some(mut img) = images.get_mut(&assets.heightfield) {
        // Terrain unchanged (e.g. only a hut moved): keep the texture.
        let same = img.data.as_ref().is_some_and(|d| d.len() == h.len() * 4 && d.chunks_exact(4).zip(&h).all(|(b, v)| b == v.to_le_bytes()));
        if same && field.0 == f {
            return;
        }
        *img = heightfield_image(&h, f.w as u32);
    }
    field.0 = f;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sea_mesh_is_flat_valid_grid() {
        let m = sea_mesh(10.0, 2.0);
        assert!(m.is_valid() && !m.is_empty());
        assert_eq!(m.vertex_count(), 11 * 11);
        assert_eq!(m.indices.len(), 10 * 10 * 6);
        assert!(m.positions.iter().all(|p| p[1] == 0.0 && p[0].abs() <= 10.0 && p[2].abs() <= 10.0));
        assert!(m.surfaces.iter().all(|s| *s == Surface::Water as u8));
        // Faces point up.
        let p = |i: u32| Vec3::from_array(m.positions[i as usize]);
        for t in m.indices.chunks(3) {
            assert!((p(t[1]) - p(t[0])).cross(p(t[2]) - p(t[0])).y > 0.0);
        }
        // Garbage inputs stay finite and bounded.
        for (h, c) in [(f32::NAN, 1.0), (5.0, f32::INFINITY), (-3.0, 0.0)] {
            let m = sea_mesh(h, c);
            assert!(m.is_valid() && !m.is_empty() && m.vertex_count() < 2_000_000);
        }
    }

    #[test]
    fn heightfield_matches_terrain() {
        let t = Terrain::build(3, &[], &[]);
        let (h, f) = heightfield(&t);
        let n = f.w as usize;
        assert_eq!(h.len(), n * n);
        assert!(h.iter().all(|v| v.is_finite()));
        let i = n / 2;
        assert_eq!(h[i * n + i], t.height(Vec2::ZERO));
        assert_eq!(heightfield(&t), (h.clone(), f), "deterministic");
        let img = heightfield_image(&h, n as u32);
        assert_eq!(img.width(), n as u32);
        assert_eq!(img.data.as_ref().map(|d| d.len()), Some(n * n * 4));
        // Short input pads with deep sea.
        let small = heightfield_image(&[1.0], 2);
        let d = small.data.unwrap();
        assert_eq!(f32::from_le_bytes([d[4], d[5], d[6], d[7]]), -24.0);
    }

    #[test]
    fn horizon_keeps_every_shore_sample_in_front() {
        let t = Terrain::build(3, &[], &[]);
        let (h, f) = heightfield(&t);
        let hz = Horizon::from_heightfield(&h, f);
        assert!(!hz.points.is_empty());
        let n = f.w as usize;
        for yaw in 0..16 {
            let a = yaw as f32 * std::f32::consts::TAU / 16.0;
            let fwd = Vec3::new(a.sin(), -0.6, a.cos());
            let r = hz.distance(fwd);
            assert!((12.0..SEA_HALF).contains(&r));
            for (i, v) in h.iter().enumerate() {
                let p = Vec2::new(f.x + (i % n) as f32 * f.z, f.y + (i / n) as f32 * f.z);
                if *v > 0.0 {
                    assert!(p.dot(Vec2::new(a.sin(), a.cos())) + 4.0 < r, "land at {p} behind the horizon {r}");
                }
            }
        }
        assert_eq!(Horizon::default().distance(Vec3::NEG_Z), GLADE_HALF + BUILD_MARGIN);
        let bad = Horizon::from_heightfield(&[f32::NAN; 4], Vec4::new(f32::NAN, 0.0, 1.0, 2.0));
        assert_eq!(bad.distance(Vec3::new(f32::NAN, 0.0, 1.0)), GLADE_HALF + BUILD_MARGIN);
    }

    /// The cut never reaches the buildable square (+ a margin): every point of it, even a
    /// corner, stays in front of the horizon for every view direction, whatever the shore.
    #[test]
    fn horizon_never_cuts_the_buildable_square() {
        let small = Horizon { points: vec![Vec2::ZERO, Vec2::new(5.0, -3.0)] };
        let half = GLADE_HALF + BUILD_MARGIN - 0.01;
        let corners = [Vec2::new(half, half), Vec2::new(-half, half), Vec2::new(half, -half), Vec2::new(-half, -half)];
        for h in [small, Horizon::default()] {
            for yaw in 0..64 {
                let a = yaw as f32 * std::f32::consts::TAU / 64.0;
                let fwd = Vec3::new(a.sin(), -0.57, a.cos());
                let r = h.distance(fwd);
                for c in corners {
                    assert!(c.dot(Vec2::new(a.sin(), a.cos())) < r, "corner {c} behind the horizon {r} at yaw {yaw}");
                }
            }
        }
    }

    #[test]
    fn flicker_is_stepped_and_bounded() {
        for i in 0..200 {
            let k = flicker(i as f32 * 0.037, i % 7);
            assert!((0.879..=1.121).contains(&k));
        }
        assert_eq!(flicker(1.01, 3), flicker(1.1, 3), "constant within one 1/8 s step");
        let distinct: std::collections::HashSet<u32> = (0..64).map(|s| (flicker(s as f32 / 8.0, 0) * 1000.0) as u32).collect();
        assert!(distinct.len() >= 3);
    }
}
