//! The PS1 voxel material. Shader: `assets/shaders/voxel.wesl` (embedded).

use bevy::{
    mesh::MeshVertexBufferLayoutRef,
    pbr::{MaterialPipeline, MaterialPipelineKey},
    asset::uuid_handle,
    prelude::*,
    render::render_resource::{
        AsBindGroup, RenderPipelineDescriptor, ShaderType, SpecializedMeshPipelineError,
    },
    shader::ShaderRef,
};

/// Embedded at compile time so the binary does not depend on the asset directory.
pub(crate) const SHADER: Handle<Shader> = uuid_handle!("7c1d6c55-2a4e-4f0b-9a63-5b0e8f3c2d91");
pub(crate) const SHADER_SOURCE: &str = include_str!("../../assets/shaders/voxel.wesl");

/// Sky / fog colour `#6b6258`.
pub const FOG_COLOR: Color = Color::srgb_u8(0x6b, 0x62, 0x58);

/// Uniform block; field order mirrors `VoxelParams` in the shader.
#[derive(Clone, Copy, Debug, ShaderType)]
pub struct VoxelParams {
    /// Linear RGB, alpha unused.
    pub fog_color: Vec4,
    /// Unit vector pointing towards the sun.
    pub sun_dir: Vec3,
    pub fog_start: f32,
    pub sun_color: Vec3,
    pub fog_end: f32,
    pub ambient: Vec3,
    /// Distance where emissive surfaces (vertex alpha 1) start to fog.
    pub emissive_fog_start: f32,
    /// Vertex snap grid in pixels; kept in sync with `LowRes` by `Ps1Plugin`.
    pub snap_res: Vec2,
    /// Free for animation; the plugin does not update it.
    pub time: f32,
}

impl Default for VoxelParams {
    fn default() -> Self {
        Self {
            fog_color: FOG_COLOR.to_linear().to_vec4(),
            sun_dir: Vec3::new(-0.5, 0.6, -0.35).normalize(),
            fog_start: 25.0,
            sun_color: Vec3::new(0.50, 0.46, 0.40),
            fog_end: 90.0,
            ambient: Vec3::new(0.56, 0.55, 0.56),
            emissive_fog_start: 60.0,
            snap_res: Vec2::new(427.0, 240.0),
            time: 0.0,
        }
    }
}

#[derive(Asset, TypePath, AsBindGroup, Debug, Clone)]
pub struct VoxelMaterial {
    #[uniform(0)]
    pub params: VoxelParams,
    #[texture(1)]
    #[sampler(2)]
    pub atlas: Handle<Image>,
}

/// The one shared voxel material. Inserted in `PreStartup` (set `Ps1Setup`), so any
/// `Startup` system can use it.
#[derive(Resource, Clone)]
pub struct VoxelMaterialHandle(pub Handle<VoxelMaterial>);

impl Material for VoxelMaterial {
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
            Mesh::ATTRIBUTE_NORMAL.at_shader_location(1),
            Mesh::ATTRIBUTE_UV_0.at_shader_location(2),
            Mesh::ATTRIBUTE_COLOR.at_shader_location(5),
        ])?];
        Ok(())
    }
}
