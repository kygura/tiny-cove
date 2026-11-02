//! Post pass on the low-res camera: 1 px outlines from depth/normal discontinuities
//! (faded with fog), shore foam on water, sky gradient behind everything, then 15-bit
//! quantise with 4×4 Bayer dither. Runs in `Core3dSystems::PostProcess`. Requires
//! `DepthPrepass` + `NormalPrepass` and `Msaa::Off` on the camera (spawned that way).

use bevy::{
    core_pipeline::{
        prepass::ViewPrepassTextures,
        schedule::{Core3d, Core3dSystems},
        FullscreenShader,
    },
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

const SHADER: &str = "embedded://cairn_cove/render/post.wesl";

/// Per-camera post settings (uniform). Field order mirrors `post.wesl`.
#[derive(Component, Clone, Copy, Debug, PartialEq, ExtractComponent, ShaderType)]
#[extract_app(RenderApp)]
pub struct PostSettings {
    /// x = far - near of the orthographic projection, y = near, z = silhouette depth
    /// jump (m), w = crease strength (0 disables creases).
    pub depth: Vec4,
    /// xyz fog colour, w fog density.
    pub fog: Vec4,
    /// xyz sky top, w fog start (view depth).
    pub sky_top: Vec4,
    /// xyz sky horizon, w lightning flash.
    pub sky_horizon: Vec4,
    /// x outline on (1/0), y dither on (1/0), z night, w time.
    pub flags: Vec4,
}

impl Default for PostSettings {
    fn default() -> Self {
        Self {
            depth: Vec4::new(1000.0, 0.1, 0.6, 0.5),
            fog: Vec4::ZERO,
            sky_top: Vec4::ZERO,
            sky_horizon: Vec4::ZERO,
            flags: Vec4::new(1.0, 1.0, 0.0, 0.0),
        }
    }
}

pub struct PostPlugin;

impl Plugin for PostPlugin {
    fn build(&self, app: &mut App) {
        bevy::asset::embedded_asset!(app, "post.wesl");
        app.add_plugins((
            ExtractComponentPlugin::<PostSettings>::default(),
            UniformComponentPlugin::<PostSettings>::default(),
        ));
        let Some(render_app) = app.get_sub_app_mut(RenderApp) else {
            return;
        };
        render_app
            .init_gpu_resource::<SpecializedRenderPipelines<PostPipeline>>()
            .add_systems(RenderStartup, init_pipeline)
            .add_systems(Render, prepare_pipelines.in_set(RenderSystems::Prepare))
            .add_systems(
                Core3d,
                post_pass.in_set(Core3dSystems::PostProcess),
            );
    }
}

#[derive(Resource)]
struct PostPipeline {
    layout: BindGroupLayoutDescriptor,
    shader: Handle<Shader>,
    fullscreen: FullscreenShader,
}

impl SpecializedRenderPipeline for PostPipeline {
    type Key = TextureFormat;

    fn specialize(&self, format: TextureFormat) -> RenderPipelineDescriptor {
        RenderPipelineDescriptor {
            label: Some("glade_post_pipeline".into()),
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
struct PostPipelineId(CachedRenderPipelineId);

fn init_pipeline(
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    fullscreen_shader: Res<FullscreenShader>,
) {
    let layout = BindGroupLayoutDescriptor::new(
        "glade_post_layout",
        &BindGroupLayoutEntries::sequential(
            ShaderStages::FRAGMENT,
            (
                texture_2d(TextureSampleType::Float { filterable: false }),
                texture_2d(TextureSampleType::Float { filterable: false }),
                texture_2d(TextureSampleType::Float { filterable: false }),
                uniform_buffer::<PostSettings>(true),
            ),
        ),
    );
    commands.insert_resource(PostPipeline {
        layout,
        shader: asset_server.load(SHADER),
        fullscreen: fullscreen_shader.clone(),
    });
}

fn prepare_pipelines(
    mut commands: Commands,
    pipeline_cache: Res<PipelineCache>,
    mut pipelines: ResMut<SpecializedRenderPipelines<PostPipeline>>,
    pipeline: Res<PostPipeline>,
    views: Query<(Entity, &ExtractedView), With<PostSettings>>,
) {
    for (entity, view) in &views {
        let id = pipelines.specialize(&pipeline_cache, &pipeline, view.target_format);
        commands.entity(entity).insert(PostPipelineId(id));
    }
}

fn post_pass(
    view: ViewQuery<(
        &ViewTarget,
        &ViewPrepassTextures,
        &DynamicUniformIndex<PostSettings>,
        &PostPipelineId,
    )>,
    pipeline: Option<Res<PostPipeline>>,
    pipeline_cache: Res<PipelineCache>,
    uniforms: Res<ComponentUniforms<PostSettings>>,
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
        Some("glade_post_bind_group"),
        &pipeline_cache.get_bind_group_layout(&pipeline.layout),
        &BindGroupEntries::sequential((post.source, normal, depth, settings.clone())),
    );
    let mut pass = ctx.command_encoder().begin_render_pass(&RenderPassDescriptor {
        label: Some("glade_post_pass"),
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
