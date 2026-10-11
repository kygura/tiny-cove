//! The toon material, voxel mesh seam and shared material handles.

use super::atmosphere::Atmosphere;
use super::palette;
use bevy::{
    asset::RenderAssetUsages,
    mesh::{Indices, PrimitiveTopology},
    prelude::*,
    render::render_resource::{AsBindGroup, ShaderType},
    shader::ShaderRef,
};

const SHADER: &str = "embedded://hollow_acre/render/toon.wesl";
const PREPASS_SHADER: &str = "embedded://hollow_acre/render/toon_prepass.wesl";

/// Material uniform; field order mirrors `ToonParams` in `toon.wesl` / `toon_prepass.wesl`.
/// Atmosphere fields are rewritten every frame by the render plugin; set only the
/// per-material ones (`base_color`, `emissive*`, `mode`).
#[derive(Clone, Copy, Debug, PartialEq, ShaderType)]
pub struct ToonParams {
    /// Linear RGBA albedo for meshes without vertex colours.
    pub base_color: Vec4,
    /// Linear RGB emissive colour for meshes without UV_1.
    pub emissive: Vec4,
    pub sun_dir: Vec3,
    pub emissive_day: f32,
    pub sun_color: Vec3,
    pub emissive_night: f32,
    pub ambient: Vec3,
    pub night: f32,
    pub shadow_tint: Vec3,
    pub shadow_strength: f32,
    pub sky_color: Vec3,
    pub fog_density: f32,
    pub fog_color: Vec3,
    pub height_fog: f32,
    pub cloud_offset: Vec2,
    pub cloud_cover: f32,
    pub wetness: f32,
    pub time: f32,
    /// 0 = cel-shaded solid, 1 = flat water.
    pub mode: u32,
    /// View depth where distance fog starts.
    pub fog_start: f32,
    pub _pad: f32,
}

impl Default for ToonParams {
    fn default() -> Self {
        let mut p = Self {
            base_color: Vec4::ONE,
            emissive: Vec4::ZERO,
            sun_dir: Vec3::Y,
            emissive_day: 0.0,
            sun_color: Vec3::ONE,
            emissive_night: 0.0,
            ambient: Vec3::splat(0.3),
            night: 0.0,
            shadow_tint: Vec3::splat(0.3),
            shadow_strength: 1.0,
            sky_color: Vec3::ONE,
            fog_density: 0.0,
            fog_color: Vec3::ONE,
            height_fog: 0.0,
            cloud_offset: Vec2::ZERO,
            cloud_cover: 0.0,
            wetness: 0.0,
            time: 0.0,
            mode: 0,
            fog_start: 0.0,
            _pad: 0.0,
        };
        p.apply_atmosphere(&Atmosphere::default(), 0.0);
        p
    }
}

fn lin(c: Color) -> Vec3 {
    let l = c.to_linear();
    Vec3::new(l.red, l.green, l.blue)
}

/// Lightning flash colour `#c9a8ff`.
const FLASH: Color = Color::srgb(0xc9 as f32 / 255.0, 0xa8 as f32 / 255.0, 1.0);

impl ToonParams {
    /// Copies the frame's atmosphere into the uniform.
    pub fn apply_atmosphere(&mut self, a: &Atmosphere, fog_start: f32) {
        self.sun_dir = a.sun_dir.normalize_or(Vec3::Y);
        self.sun_color = lin(a.sun_color) * a.sun_illuminance.max(0.0);
        let flash = a.flash.clamp(0.0, 1.0);
        self.ambient = (lin(a.ambient_color) * a.ambient_brightness).lerp(lin(FLASH) * 1.6, flash);
        self.night = a.night.clamp(0.0, 1.0);
        self.shadow_tint = lin(a.shadow_tint);
        self.shadow_strength = a.shadow_strength.clamp(0.0, 1.0);
        self.sky_color = lin(a.sky_color);
        self.fog_density = a.fog_density.max(0.0);
        self.fog_color = lin(a.fog_color);
        self.height_fog = a.height_fog.max(0.0);
        self.cloud_offset = a.cloud_offset;
        self.cloud_cover = a.cloud_cover.clamp(0.0, 1.0);
        self.wetness = a.wetness.clamp(0.0, 1.0);
        self.time = a.time_secs;
        self.fog_start = fog_start;
    }
}

/// Cel-shaded material for voxel chunks and props. Lit by the (single) shadow-casting
/// [`DirectionalLight`] spawned by the render plugin.
#[derive(Asset, TypePath, AsBindGroup, Debug, Clone)]
pub struct ToonMaterial {
    #[uniform(0)]
    pub params: ToonParams,
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
    ToonMaterial {
        params: ToonParams {
            base_color: Vec4::new(c.red, c.green, c.blue, 1.0),
            emissive: lin(emissive).extend(1.0),
            emissive_day: day,
            emissive_night: night,
            ..default()
        },
    }
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
    let idx: Vec<u32> = if indices.iter().all(|&i| i < max) && indices.len() % 3 == 0 {
        indices.to_vec()
    } else {
        warn!("voxel_mesh: dropping triangles with out-of-range indices");
        indices
            .chunks_exact(3)
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

/// Keeps every [`ToonMaterial`]'s atmosphere fields in sync with [`Atmosphere`].
pub(super) fn sync_materials(
    atmos: Res<Atmosphere>,
    mut materials: ResMut<Assets<ToonMaterial>>,
    fog_start: Res<super::FogStart>,
) {
    let ids: Vec<_> = materials.ids().collect();
    for id in ids {
        let Some(m) = materials.get(id) else { continue };
        let mut p = m.params;
        p.apply_atmosphere(&atmos, fog_start.0);
        if p != m.params {
            if let Some(mut m) = materials.get_mut(id) {
                m.params = p;
            }
        }
    }
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
    fn voxel_mesh_drops_bad_indices() {
        let p = [[0.0; 3]; 3];
        let m = voxel_mesh(&p, &[], &[], &[], &[0, 1, 2, 0, 1, 7]);
        assert_eq!(m.indices().map(|i| i.len()), Some(3));
    }
}
