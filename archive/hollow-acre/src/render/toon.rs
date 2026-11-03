//! The toon material, voxel mesh seam and shared material handles.

use super::atmosphere::Atmosphere;
use super::palette;
use bevy::{
    asset::RenderAssetUsages,
    mesh::{Indices, PrimitiveTopology},
    asset::uuid_handle,
    prelude::*,
    render::{
        render_resource::{AsBindGroup, ShaderType},
        storage::ShaderBuffer,
    },
    shader::ShaderRef,
};

const SHADER: &str = "embedded://hollow_acre/render/toon.wesl";
const PREPASS_SHADER: &str = "embedded://hollow_acre/render/toon_prepass.wesl";

/// Per-material uniform; field order mirrors `ToonParams` in the toon shaders.
/// Frame-wide lighting lives in the shared [`ToonGlobals`] buffer instead.
#[derive(Clone, Copy, Debug, PartialEq, ShaderType)]
pub struct ToonParams {
    /// Linear RGBA albedo for meshes without vertex colours.
    pub base_color: Vec4,
    /// Linear RGB emissive colour for meshes without UV_1.
    pub emissive: Vec4,
    pub emissive_day: f32,
    pub emissive_night: f32,
    /// 0 = cel-shaded solid, 1 = flat water.
    pub mode: u32,
    pub _pad: f32,
}

impl Default for ToonParams {
    fn default() -> Self {
        Self {
            base_color: Vec4::ONE,
            emissive: Vec4::ZERO,
            emissive_day: 0.0,
            emissive_night: 0.0,
            mode: 0,
            _pad: 0.0,
        }
    }
}

/// Lamp point lights shaded per frame (nearest to the view centre).
pub const MAX_LAMPS: usize = 32;

/// The one storage buffer holding [`ToonGlobals`], shared by every [`ToonMaterial`].
pub const TOON_GLOBALS: Handle<ShaderBuffer> = uuid_handle!("6b1f2c7e-4d0a-4e57-9a3c-2f5d8e1b7a90");

fn lin(c: Color) -> Vec3 {
    let l = c.to_linear();
    Vec3::new(l.red, l.green, l.blue)
}

/// Lightning flash colour `#c9a8ff`.
const FLASH: Color = Color::srgb(0xc9 as f32 / 255.0, 0xa8 as f32 / 255.0, 1.0);

/// Frame-wide toon lighting, uploaded once per frame (only when it changed) into
/// [`TOON_GLOBALS`]. Layout mirrors `ToonGlobals` in `toon.wesl`.
#[derive(Clone, Debug, PartialEq)]
pub struct ToonGlobals {
    pub sun_dir: Vec3,
    pub night: f32,
    /// Linear sun colour × relative intensity.
    pub sun_color: Vec3,
    pub shadow_strength: f32,
    /// Linear ambient × brightness, flash mixed in.
    pub ambient: Vec3,
    pub wetness: f32,
    pub shadow_tint: Vec3,
    pub fog_density: f32,
    pub sky_color: Vec3,
    pub height_fog: f32,
    pub fog_color: Vec3,
    pub fog_start: f32,
    pub cloud_offset: Vec2,
    pub cloud_cover: f32,
    pub time: f32,
    /// Overall multiplier on lit (non-emissive) colour: overcast/storm darken.
    pub dim: f32,
    pub lamps: Vec<Vec3>,
}

impl ToonGlobals {
    pub fn from_atmosphere(a: &Atmosphere, fog_start: f32, lamps: &[Vec3]) -> Self {
        let flash = a.flash.clamp(0.0, 1.0);
        let night = a.night.clamp(0.0, 1.0);
        let sun = a.sun_illuminance.max(0.0);
        let cover = a.cloud_cover.clamp(0.0, 1.0);
        // Daytime dim from weather: storm (sun 0.35) ~0.74, overcast (0.55) ~0.82.
        let weather_dim = (0.6 + 0.4 * sun).clamp(0.6, 1.0) * (1.0 - 0.12 * cover);
        Self {
            sun_dir: a.sun_dir.normalize_or(Vec3::Y),
            night,
            sun_color: lin(a.sun_color) * sun,
            shadow_strength: a.shadow_strength.clamp(0.0, 1.0),
            ambient: (lin(a.ambient_color) * a.ambient_brightness.max(0.0)).lerp(lin(FLASH) * 1.6, flash),
            wetness: a.wetness.clamp(0.0, 1.0),
            shadow_tint: lin(a.shadow_tint),
            fog_density: a.fog_density.max(0.0),
            sky_color: lin(a.sky_color),
            height_fog: a.height_fog.max(0.0),
            fog_color: lin(a.fog_color),
            fog_start,
            cloud_offset: a.cloud_offset,
            cloud_cover: cover,
            time: a.time_secs,
            dim: 1.0 + (weather_dim - 1.0) * (1.0 - night),
            lamps: lamps.iter().take(MAX_LAMPS).copied().collect(),
        }
    }

    /// Packed std430 layout: 8 header vec4s + `MAX_LAMPS` lamp vec4s (fixed size, so
    /// the GPU buffer is rewritten in place).
    pub fn to_vec4s(&self) -> Vec<[f32; 4]> {
        let v = |a: Vec3, w: f32| [a.x, a.y, a.z, w];
        let mut out = vec![
            v(self.sun_dir, self.night),
            v(self.sun_color, self.shadow_strength),
            v(self.ambient, self.wetness),
            v(self.shadow_tint, self.fog_density),
            v(self.sky_color, self.height_fog),
            v(self.fog_color, self.fog_start),
            [self.cloud_offset.x, self.cloud_offset.y, self.cloud_cover, self.time],
            [self.lamps.len().min(MAX_LAMPS) as f32, self.dim, 0.0, 0.0],
        ];
        out.extend((0..MAX_LAMPS).map(|i| self.lamps.get(i).map_or([0.0; 4], |p| v(*p, 1.0))));
        out
    }
}

/// Cel-shaded material for voxel chunks and props. Lit by the (single) shadow-casting
/// [`DirectionalLight`] spawned by the render plugin; frame-wide lighting comes from the
/// shared [`TOON_GLOBALS`] buffer (always set `globals: TOON_GLOBALS`).
#[derive(Asset, TypePath, AsBindGroup, Debug, Clone)]
pub struct ToonMaterial {
    #[uniform(0)]
    pub params: ToonParams,
    #[storage(1, read_only)]
    pub globals: Handle<ShaderBuffer>,
}

impl ToonMaterial {
    pub fn new(params: ToonParams) -> Self {
        Self { params, globals: TOON_GLOBALS }
    }
}

impl Material for ToonMaterial {
    fn fragment_shader() -> ShaderRef {
        SHADER.into()
    }

    fn prepass_fragment_shader() -> ShaderRef {
        PREPASS_SHADER.into()
    }
}

/// Shared materials for voxel chunk meshes. Both are [`ToonMaterial`]s; `voxel_mesh`
/// flags water vertices, so a mixed chunk renders correctly with `opaque` alone.
/// `water` forces flat water shading for meshes built only from water faces.
#[derive(Resource, Clone)]
pub struct VoxelMaterials {
    pub opaque: Handle<ToonMaterial>,
    pub water: Handle<ToonMaterial>,
}

/// A toon material for non-voxel props (sheep, motes...): flat `color` albedo plus
/// `emissive` glowing with `day`/`night` strengths (DESIGN emissive convention).
/// Meshes need POSITION and NORMAL only.
pub fn toon_material(color: Color, emissive: Color, day: f32, night: f32) -> ToonMaterial {
    let c = color.to_linear();
    ToonMaterial::new(ToonParams {
        base_color: Vec4::new(c.red, c.green, c.blue, 1.0),
        emissive: lin(emissive).extend(1.0),
        emissive_day: day,
        emissive_night: night,
        ..default()
    })
}

/// Flag value in UV_1.y marking water vertices.
const WATER_FLAG: f32 = 1.0;

/// Builds a toon-ready mesh from mesher output (`acre_core::ChunkMesh` fields).
/// `voxels[i]` is the voxel kind id of vertex `i`; `ao[i]` its 0..1 occlusion.
/// Attributes: POSITION, NORMAL, COLOR (linear albedo, a = AO), UV_0 (emissive
/// day/night strength), UV_1 (packed emissive sRGB, water flag). Slices shorter than
/// `positions` are padded (Air / AO 1) rather than panicking.
pub fn voxel_mesh(
    positions: &[[f32; 3]],
    normals: &[[f32; 3]],
    voxels: &[u8],
    ao: &[f32],
    indices: &[u32],
) -> Mesh {
    let n = positions.len();
    if normals.len() != n || voxels.len() != n || ao.len() != n {
        warn!(
            "voxel_mesh: attribute length mismatch (pos {n}, nrm {}, vox {}, ao {})",
            normals.len(),
            voxels.len(),
            ao.len()
        );
    }
    let mut nrm = Vec::with_capacity(n);
    let mut col = Vec::with_capacity(n);
    let mut uv0 = Vec::with_capacity(n);
    let mut uv1 = Vec::with_capacity(n);
    for i in 0..n {
        nrm.push(normals.get(i).copied().unwrap_or([0.0, 1.0, 0.0]));
        let v = voxels.get(i).copied().unwrap_or(palette::kind::AIR);
        let e = palette::entry(v);
        let a = Color::srgb_u8(e.albedo[0], e.albedo[1], e.albedo[2]).to_linear();
        let occ = ao.get(i).copied().unwrap_or(1.0).clamp(0.0, 1.0);
        col.push([a.red, a.green, a.blue, occ]);
        uv0.push([e.day, e.night]);
        let packed = ((e.emissive[0] as u32) << 16) | ((e.emissive[1] as u32) << 8) | e.emissive[2] as u32;
        let water = if v == palette::kind::WATER { WATER_FLAG } else { 0.0 };
        uv1.push([packed as f32, water]);
    }
    let max = n as u32;
    let idx: Vec<u32> = if indices.iter().all(|&i| i < max) && indices.len().is_multiple_of(3) {
        indices.to_vec()
    } else {
        warn!("voxel_mesh: dropping triangles with out-of-range indices");
        indices
            .as_chunks::<3>()
            .0
            .iter()
            .filter(|t| t.iter().all(|&i| i < max))
            .flatten()
            .copied()
            .collect()
    };
    Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default())
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions.to_vec())
        .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, nrm)
        .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, col)
        .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, uv0)
        .with_inserted_attribute(Mesh::ATTRIBUTE_UV_1, uv1)
        .with_inserted_indices(Indices::U32(idx))
}

/// Uploads [`ToonGlobals`] into the shared buffer when anything changed.
pub(super) fn sync_globals(
    atmos: Res<Atmosphere>,
    fog_start: Res<super::FogStart>,
    lamps: Res<super::ActiveLamps>,
    mut last: Local<Option<ToonGlobals>>,
    mut buffers: ResMut<Assets<ShaderBuffer>>,
) {
    let g = ToonGlobals::from_atmosphere(&atmos, fog_start.0, &lamps.0);
    if last.as_ref() == Some(&g) && buffers.contains(&TOON_GLOBALS) {
        return;
    }
    let data = g.to_vec4s();
    if let Some(mut buf) = buffers.get_mut(&TOON_GLOBALS) {
        buf.clear();
        buf.extend(data);
    } else {
        let _ = buffers.insert(&TOON_GLOBALS, ShaderBuffer::from(data));
    }
    *last = Some(g);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn voxel_mesh_attributes() {
        let p = [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]];
        let n = [[0.0, 0.0, 1.0]; 3];
        let m = voxel_mesh(&p, &n, &[palette::kind::CRYSTAL, 6, 9], &[1.0, 0.5, 1.0], &[0, 1, 2]);
        assert_eq!(m.count_vertices(), 3);
        let Some(bevy::mesh::VertexAttributeValues::Float32x2(uv1)) = m.attribute(Mesh::ATTRIBUTE_UV_1)
        else {
            panic!("uv1 missing")
        };
        assert_eq!(uv1[0][0] as u32, 0x5cf0e0);
        assert_eq!(uv1[1][1], WATER_FLAG);
        assert_eq!(uv1[2], [0.0, 0.0]);
    }

    #[test]
    fn globals_layout_is_fixed_size() {
        let a = Atmosphere::default();
        let g = ToonGlobals::from_atmosphere(&a, 10.0, &vec![Vec3::ONE; 50]);
        let v = g.to_vec4s();
        assert_eq!(v.len(), 8 + MAX_LAMPS);
        assert_eq!(v[7][0], MAX_LAMPS as f32);
        assert_eq!(v[5][3], 10.0);
        assert_eq!(ToonGlobals::from_atmosphere(&a, 0.0, &[]).to_vec4s().len(), 8 + MAX_LAMPS);
        assert!((g.dim - (0.6 + 0.4) * (1.0 - 0.12 * a.cloud_cover)).abs() < 1e-5);
    }

    #[test]
    fn voxel_mesh_drops_bad_indices() {
        let p = [[0.0; 3]; 3];
        let m = voxel_mesh(&p, &[], &[], &[], &[0, 1, 2, 0, 1, 7]);
        assert_eq!(m.indices().map(|i| i.len()), Some(3));
    }
}
