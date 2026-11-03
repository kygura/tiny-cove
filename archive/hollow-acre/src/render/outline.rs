//! Outline post-process on the low-res camera: reads the colour target plus the depth and
//! normal prepass textures, draws 1 px silhouettes/creases, water foam/sparkle/rings.
//! Runs in `Core3dSystems::PostProcess` before bloom. Requires `Hdr`, `DepthPrepass`
//! and `NormalPrepass` on the camera (the render plugin spawns it that way).

use bevy::{
    core_pipeline::{
        prepass::ViewPrepassTextures,
        schedule::{Core3d, Core3dSystems},
        FullscreenShader,
    },
    post_process::bloom::bloom,
    prelude::*,
    shader::Shader,
    render::{
        extract_component::{
            ComponentUniforms, DynamicUniformIndex, ExtractComponent, ExtractComponentPlugin,
            UniformComponentPlugin,
        },
        render_resource::{
            binding_types::{texture_2d, uniform_buffer},
            *,
        },
        renderer::{RenderContext, ViewQuery},
        view::{ExtractedView, ViewTarget},
        GpuResourceAppExt, Render, RenderApp, RenderStartup, RenderSystems,
    },
};

const SHADER: &str = "embedded://hollow_acre/render/outline.wesl";

/// Per-camera outline settings (uniform). Field order mirrors `outline.wesl`.
#[derive(Component, Clone, Copy, Debug, PartialEq, ExtractComponent, ShaderType)]
#[extract_app(RenderApp)]
pub struct OutlineSettings {
    /// far - near of the orthographic projection.
    pub depth_range: f32,
    /// Depth jump in world units that counts as a silhouette. 1.0 (DESIGN said 0.5) so a
    /// 1-voxel terrain step (0.87 wu jump at 35° pitch) draws no line.
    pub silhouette: f32,
    /// Normal dot below which a same-depth edge is a crease (DESIGN: 0.8).
    pub crease_dot: f32,
    /// Crease blend toward outline colour (DESIGN: 0.4).
    pub crease_strength: f32,
    pub time: f32,
    pub sparkle: f32,
    pub rain: f32,
    /// 0..1 night factor.
    pub night: f32,
    /// x fog density, y height fog, z fog start (view depth), w camera near.
    pub fog: Vec4,
    /// Inverse view-projection of the game camera (reconstructs world y for fog).
    pub world_from_clip: Mat4,
}

impl Default for OutlineSettings {
    fn default() -> Self {
        Self {
            depth_range: 1000.0,
            silhouette: 1.0,
            crease_dot: 0.8,
            crease_strength: 0.4,
            time: 0.0,
            sparkle: 1.0,
            rain: 0.0,
            night: 0.0,
            fog: Vec4::ZERO,
            world_from_clip: Mat4::IDENTITY,
        }
    }
}

pub struct OutlinePlugin;

impl Plugin for OutlinePlugin {
    fn build(&self, app: &mut App) {
        bevy::asset::embedded_asset!(app, "outline.wesl");
        app.add_plugins((
            ExtractComponentPlugin::<OutlineSettings>::default(),
            UniformComponentPlugin::<OutlineSettings>::default(),
        ));
        let Some(render_app) = app.get_sub_app_mut(RenderApp) else {
            return;
        };
        render_app
            .init_gpu_resource::<SpecializedRenderPipelines<OutlinePipeline>>()
            .add_systems(RenderStartup, init_pipeline)
            .add_systems(Render, prepare_pipelines.in_set(RenderSystems::Prepare))
            .add_systems(
                Core3d,
                outline_pass.before(bloom).in_set(Core3dSystems::PostProcess),
            );
    }
}

#[derive(Resource)]
struct OutlinePipeline {
    layout: BindGroupLayoutDescriptor,
    shader: Handle<Shader>,
    fullscreen: FullscreenShader,
}

impl SpecializedRenderPipeline for OutlinePipeline {
    type Key = TextureFormat;

    fn specialize(&self, format: TextureFormat) -> RenderPipelineDescriptor {
        RenderPipelineDescriptor {
            label: Some("outline_pipeline".into()),
            layout: vec![self.layout.clone()],
            vertex: self.fullscreen.to_vertex_state(),
            fragment: Some(FragmentState {
                shader: self.shader.clone(),
                targets: vec![Some(ColorTargetState { format, blend: None, write_mask: ColorWrites::ALL })],
                ..default()
            }),
            ..default()
        }
    }
}

#[derive(Component)]
struct OutlinePipelineId(CachedRenderPipelineId);

fn init_pipeline(
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    fullscreen_shader: Res<FullscreenShader>,
) {
    let layout = BindGroupLayoutDescriptor::new(
        "outline_layout",
        &BindGroupLayoutEntries::sequential(
            ShaderStages::FRAGMENT,
            (
                texture_2d(TextureSampleType::Float { filterable: false }),
                texture_2d(TextureSampleType::Float { filterable: false }),
                texture_2d(TextureSampleType::Float { filterable: false }),
                uniform_buffer::<OutlineSettings>(true),
            ),
        ),
    );
    commands.insert_resource(OutlinePipeline {
        layout,
        shader: asset_server.load(SHADER),
        fullscreen: fullscreen_shader.clone(),
    });
}

fn prepare_pipelines(
    mut commands: Commands,
    pipeline_cache: Res<PipelineCache>,
    mut pipelines: ResMut<SpecializedRenderPipelines<OutlinePipeline>>,
    pipeline: Res<OutlinePipeline>,
    views: Query<(Entity, &ExtractedView), With<OutlineSettings>>,
) {
    for (entity, view) in &views {
        let id = pipelines.specialize(&pipeline_cache, &pipeline, view.target_format);
        commands.entity(entity).insert(OutlinePipelineId(id));
    }
}

fn outline_pass(
    view: ViewQuery<(
        &ViewTarget,
        &ViewPrepassTextures,
        &DynamicUniformIndex<OutlineSettings>,
        &OutlinePipelineId,
    )>,
    pipeline: Option<Res<OutlinePipeline>>,
    pipeline_cache: Res<PipelineCache>,
    uniforms: Res<ComponentUniforms<OutlineSettings>>,
    mut ctx: RenderContext,
) {
    let Some(pipeline) = pipeline else { return };
    let (view_target, prepass, index, id) = view.into_inner();
    let Some(render_pipeline) = pipeline_cache.get_render_pipeline(id.0) else {
        return;
    };
    let (Some(depth), Some(normal)) = (prepass.depth_only_view(), prepass.normal_view()) else {
        return;
    };
    let Some(settings) = uniforms.uniforms().binding() else {
        return;
    };

    let post = view_target.post_process_write();
    let bind_group = ctx.render_device().create_bind_group(
        Some("outline_bind_group"),
        &pipeline_cache.get_bind_group_layout(&pipeline.layout),
        &BindGroupEntries::sequential((post.source, normal, depth, settings.clone())),
    );
    let mut pass = ctx.command_encoder().begin_render_pass(&RenderPassDescriptor {
        label: Some("outline_pass"),
        color_attachments: &[Some(RenderPassColorAttachment {
            view: post.destination,
            depth_slice: None,
            resolve_target: None,
            ops: Operations::default(),
        })],
        depth_stencil_attachment: None,
        timestamp_writes: None,
        occlusion_query_set: None,
        multiview_mask: None,
    });
    pass.set_pipeline(render_pipeline);
    pass.set_bind_group(0, &bind_group, &[index.index()]);
    pass.draw(0..3, 0..1);
}
