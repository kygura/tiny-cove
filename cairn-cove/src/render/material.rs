//! The PS1 materials and the per-frame globals buffer they share.
//!
//! Shaders: `glade.wesl` (main pass: `vertex`/`fragment` for [`GladeMaterial`],
//! `water_fragment` for [`WaterMaterial`], `sea_vertex`/`sea_fragment` for [`SeaMaterial`],
//! `particle_vertex`/`particle_fragment` for [`ParticleMaterial`]) and `glade_prepass.wesl`
//! (depth/normal prepass + shadow pass, snapped and displaced identically so the outline pass
//! lines up).
//!
//! Bindings (material group): 0 globals (storage), 1 atlas, 2 water params, 3 sea-bed
//! heightfield (R32Float, see [`super::sea`]).

use crate::env::Environment;
use bevy::{
    asset::uuid_handle,
    mesh::MeshVertexBufferLayoutRef,
    pbr::{MaterialPipeline, MaterialPipelineKey},
    prelude::*,
    render::{
        render_resource::{AsBindGroup, RenderPipelineDescriptor, ShaderType, SpecializedMeshPipelineError},
        storage::ShaderBuffer,
    },
    shader::ShaderRef,
};

const SHADER: &str = "embedded://cairn_cove/render/glade.wesl";
const PREPASS_SHADER: &str = "embedded://cairn_cove/render/glade_prepass.wesl";

/// The one storage buffer holding [`Globals`], shared by every material.
pub const GLOBALS: Handle<ShaderBuffer> = uuid_handle!("3d0c6a7e-91f2-4b8a-a5d3-7e19c40b2f61");

/// Vertex snap grid as a fraction of the low-res resolution: 1.0 = snap to every
/// low-res pixel, 0.5 = to every second pixel (more wobble), 0 = off.
#[derive(Resource, Clone, Copy, Debug, PartialEq)]
pub struct VertexSnap(pub f32);

impl Default for VertexSnap {
    fn default() -> Self {
        Self(0.5)
    }
}

/// Distance (view depth, metres) where fog starts: the camera focus, so the near half of
/// the island is crisp and the far half fades.
pub const FOG_START: f32 = super::CAMERA_DISTANCE;

/// Heightfield placement for the shaders: (min x, min z, spacing, samples per side).
#[derive(Resource, Clone, Copy, Debug, PartialEq)]
pub struct SeaField(pub Vec4);

impl Default for SeaField {
    fn default() -> Self {
        Self(Vec4::new(-1.0, -1.0, 1.0, 2.0))
    }
}

/// Packs [`Environment`] into the std430 vec4 layout of `Globals` in `glade.wesl`
/// (`glade_prepass.wesl` mirrors the first 9 slots).
pub fn globals_from_env(env: &Environment, time: f32, snap: f32, field: Vec4) -> Vec<[f32; 4]> {
    let v = |a: Vec3, w: f32| [a.x, a.y, a.z, w];
    let s = &env.sea;
    vec![
        v(env.sun_dir.normalize_or(Vec3::Y), env.night.clamp(0.0, 1.0)),
        v(env.sun_color, env.wetness.clamp(0.0, 1.0)),
        v(env.fill_color, env.snow.clamp(0.0, 1.0)),
        v(env.shadow_tint, env.flash.clamp(0.0, 1.0)),
        v(env.rim_color, env.fog_density.max(0.0)),
        v(env.fog_color, FOG_START),
        v(env.sky_top, time),
        v(env.sky_horizon, snap.max(0.0)),
        [env.wind.x, env.wind.y, env.tide, env.lit_rot],
        v(s[0], env.storm.clamp(0.0, 1.0)),
        v(s[1], env.sea_fog.clamp(0.0, 1.0)),
        v(s[2], env.fog_top),
        v(s[3], if env.tide_falling { 1.0 } else { 0.0 }),
        v(s[4], env.dark.clamp(0.0, 1.0)),
        v(s[5], env.wind.length()),
        v(env.outline, 0.0),
        field.to_array(),
        v(env.ambient_sky, 0.0),
    ]
}

/// Uploads the globals every frame (time changes every frame anyway).
pub(super) fn sync_globals(
    env: Res<Environment>,
    snap: Res<VertexSnap>,
    field: Res<SeaField>,
    time: Res<Time>,
    mut buffers: ResMut<Assets<ShaderBuffer>>,
) {
    let data = globals_from_env(&env, time.elapsed_secs_wrapped(), snap.0, field.0);
    if let Some(mut buf) = buffers.get_mut(&GLOBALS) {
        buf.clear();
        buf.extend(data);
    } else {
        let _ = buffers.insert(&GLOBALS, ShaderBuffer::from(data));
    }
}

fn is_prepass(descriptor: &RenderPipelineDescriptor) -> bool {
    descriptor.label.as_deref().is_some_and(|l| l.contains("prepass"))
}

/// Shared vertex layouts: main pass (pos 0, normal 1, uv 2, uv1 3, colour 5) and
/// prepass/shadow pass (pos 0, uv1 2, normal 3, uv 4).
fn specialize_solid(
    descriptor: &mut RenderPipelineDescriptor,
    layout: &MeshVertexBufferLayoutRef,
    vertex_entry: &'static str,
    fragment_entry: &'static str,
) -> Result<(), SpecializedMeshPipelineError> {
    if is_prepass(descriptor) {
        descriptor.vertex.buffers = vec![layout.0.get_layout(&[
            Mesh::ATTRIBUTE_POSITION.at_shader_location(0),
            Mesh::ATTRIBUTE_UV_1.at_shader_location(2),
            Mesh::ATTRIBUTE_NORMAL.at_shader_location(3),
            Mesh::ATTRIBUTE_UV_0.at_shader_location(4),
        ])?];
        descriptor.vertex.entry_point = Some(vertex_entry.into());
        return Ok(());
    }
    descriptor.vertex.buffers = vec![layout.0.get_layout(&[
        Mesh::ATTRIBUTE_POSITION.at_shader_location(0),
        Mesh::ATTRIBUTE_NORMAL.at_shader_location(1),
        Mesh::ATTRIBUTE_UV_0.at_shader_location(2),
        Mesh::ATTRIBUTE_UV_1.at_shader_location(3),
        Mesh::ATTRIBUTE_COLOR.at_shader_location(5),
    ])?];
    descriptor.vertex.entry_point = Some(vertex_entry.into());
    if let Some(frag) = descriptor.fragment.as_mut() {
        frag.entry_point = Some(fragment_entry.into());
    }
    Ok(())
}

/// Opaque PS1 cel material for every [`cairn_cove_core::MeshData`] (via
/// [`super::to_bevy_mesh`]). One shared instance lives in `GladeAssets::opaque`.
#[derive(Asset, TypePath, AsBindGroup, Debug, Clone)]
pub struct GladeMaterial {
    #[storage(0, read_only)]
    pub globals: Handle<ShaderBuffer>,
    #[texture(1)]
    pub atlas: Handle<Image>,
}

impl Material for GladeMaterial {
    fn vertex_shader() -> ShaderRef {
        SHADER.into()
    }
    fn fragment_shader() -> ShaderRef {
        SHADER.into()
    }
    fn prepass_vertex_shader() -> ShaderRef {
        PREPASS_SHADER.into()
    }
    fn prepass_fragment_shader() -> ShaderRef {
        PREPASS_SHADER.into()
    }
    fn specialize(
        _pipeline: &MaterialPipeline,
        descriptor: &mut RenderPipelineDescriptor,
        layout: &MeshVertexBufferLayoutRef,
        _key: MaterialPipelineKey<Self>,
    ) -> Result<(), SpecializedMeshPipelineError> {
        specialize_solid(descriptor, layout, "vertex", "fragment")
    }
}

/// Lagoon colours (linear RGB, w unused): `deep` = lagoon ≥ 0.6 m, `shallow` = lagoon.
#[derive(Clone, Copy, Debug, PartialEq, ShaderType)]
pub struct WaterParams {
    pub deep: Vec4,
    pub shallow: Vec4,
}

impl Default for WaterParams {
    fn default() -> Self {
        use cairn_cove_core::palette;
        Self { deep: palette::lagoon_deep().extend(1.0), shallow: palette::lagoon().extend(1.0) }
    }
}

/// Lagoons and tide pools (`Scene::water`): flat `lagoon` / `lagoon_deep` by depth with the
/// Water tile's caustics and a `lagoon_rim` edge. Water at mean sea level is left to the sea.
#[derive(Asset, TypePath, AsBindGroup, Debug, Clone)]
pub struct WaterMaterial {
    #[storage(0, read_only)]
    pub globals: Handle<ShaderBuffer>,
    #[texture(1)]
    pub atlas: Handle<Image>,
    #[uniform(2)]
    pub params: WaterParams,
    #[texture(3, sample_type = "float", filterable = false)]
    pub heightfield: Handle<Image>,
}

impl Material for WaterMaterial {
    fn vertex_shader() -> ShaderRef {
        SHADER.into()
    }
    fn fragment_shader() -> ShaderRef {
        SHADER.into()
    }
    fn prepass_vertex_shader() -> ShaderRef {
        PREPASS_SHADER.into()
    }
    fn prepass_fragment_shader() -> ShaderRef {
        PREPASS_SHADER.into()
    }
    fn enable_shadows() -> bool {
        false
    }
    fn specialize(
        _pipeline: &MaterialPipeline,
        descriptor: &mut RenderPipelineDescriptor,
        layout: &MeshVertexBufferLayoutRef,
        _key: MaterialPipelineKey<Self>,
    ) -> Result<(), SpecializedMeshPipelineError> {
        specialize_solid(descriptor, layout, "vertex", "water_fragment")
    }
}

/// The open sea (DESIGN A6): one big grid at `SEA_LEVEL + tide` with snapped vertex waves,
/// depth bands from the sea-bed heightfield, foam, glints and whitecaps.
#[derive(Asset, TypePath, AsBindGroup, Debug, Clone)]
pub struct SeaMaterial {
    #[storage(0, read_only)]
    pub globals: Handle<ShaderBuffer>,
    #[texture(1)]
    pub atlas: Handle<Image>,
    #[texture(3, sample_type = "float", filterable = false)]
    pub heightfield: Handle<Image>,
}

impl Material for SeaMaterial {
    fn vertex_shader() -> ShaderRef {
        SHADER.into()
    }
    fn fragment_shader() -> ShaderRef {
        SHADER.into()
    }
    fn prepass_vertex_shader() -> ShaderRef {
        PREPASS_SHADER.into()
    }
    fn prepass_fragment_shader() -> ShaderRef {
        PREPASS_SHADER.into()
    }
    fn enable_shadows() -> bool {
        false
    }
    fn specialize(
        _pipeline: &MaterialPipeline,
        descriptor: &mut RenderPipelineDescriptor,
        layout: &MeshVertexBufferLayoutRef,
        _key: MaterialPipelineKey<Self>,
    ) -> Result<(), SpecializedMeshPipelineError> {
        specialize_solid(descriptor, layout, "sea_vertex", "sea_fragment")
    }
}

/// Unlit camera-facing particles with screen-door (dithered) alpha. Meshes come from
/// [`super::particles::particle_mesh`]: the quad is expanded in the vertex shader, so a
/// whole particle system is one mesh rebuilt per frame. No prepass, no shadows.
#[derive(Asset, TypePath, AsBindGroup, Debug, Clone)]
pub struct ParticleMaterial {
    #[storage(0, read_only)]
    pub globals: Handle<ShaderBuffer>,
}

impl Material for ParticleMaterial {
    fn vertex_shader() -> ShaderRef {
        SHADER.into()
    }
    fn fragment_shader() -> ShaderRef {
        SHADER.into()
    }
    fn enable_prepass() -> bool {
        false
    }
    fn enable_shadows() -> bool {
        false
    }
    fn specialize(
        _pipeline: &MaterialPipeline,
        descriptor: &mut RenderPipelineDescriptor,
        layout: &MeshVertexBufferLayoutRef,
        _key: MaterialPipelineKey<Self>,
    ) -> Result<(), SpecializedMeshPipelineError> {
        descriptor.vertex.buffers = vec![layout.0.get_layout(&[
            Mesh::ATTRIBUTE_POSITION.at_shader_location(0),
            Mesh::ATTRIBUTE_UV_0.at_shader_location(1),
            Mesh::ATTRIBUTE_UV_1.at_shader_location(2),
            Mesh::ATTRIBUTE_COLOR.at_shader_location(3),
            Mesh::ATTRIBUTE_NORMAL.at_shader_location(4),
        ])?];
        descriptor.vertex.entry_point = Some("particle_vertex".into());
        if let Some(frag) = descriptor.fragment.as_mut() {
            frag.entry_point = Some("particle_fragment".into());
        }
        // Billboards may face either way after expansion.
        descriptor.primitive.cull_mode = None;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn globals_layout() {
        let g = globals_from_env(&Environment::default(), 1.5, 0.5, Vec4::new(-40.0, -40.0, 0.5, 161.0));
        assert_eq!(g.len(), 18);
        assert_eq!(g[6][3], 1.5);
        assert_eq!(g[7][3], 0.5);
        assert_eq!(g[5][3], FOG_START);
        assert_eq!(g[16], [-40.0, -40.0, 0.5, 161.0]);
        assert!(g.iter().flatten().all(|v| v.is_finite()));
    }
}
