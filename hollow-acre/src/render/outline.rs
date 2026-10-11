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
        view::ViewTarget,
        RenderApp, RenderStartup,
    },
};

const SHADER: &str = "embedded://hollow_acre/render/outline.wesl";

/// Per-camera outline settings (uniform). Field order mirrors `outline.wesl`.
#[derive(Component, Clone, Copy, Debug, ExtractComponent, ShaderType)]
#[extract_app(RenderApp)]
pub struct OutlineSettings {
    /// far - near of the orthographic projection.
    pub depth_range: f32,
    /// Depth jump in world units that counts as a silhouette (DESIGN: 0.5).
    pub silhouette: f32,
    /// Normal dot below which a same-depth edge is a crease (DESIGN: 0.8).
    pub crease_dot: f32,
    /// Crease blend toward outline colour (DESIGN: 0.4).
    pub crease_strength: f32,
    pub time: f32,
    pub sparkle: f32,
    pub rain: f32,
    pub _pad: f32,
}

impl Default for OutlineSettings {
    fn default() -> Self {
        Self {
            depth_range: 1000.0,
            silhouette: 0.5,
            crease_dot: 0.8,
            crease_strength: 0.4,
            time: 0.0,
            sparkle: 1.0,
            rain: 0.0,
            _pad: 0.0,
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
            .add_systems(RenderStartup, init_pipeline)
            .add_systems(
                Core3d,
                outline_pass.before(bloom).in_set(Core3dSystems::PostProcess),
            );
    }
}

#[derive(Resource)]
struct OutlinePipeline {
    layout: BindGroupLayoutDescriptor,
    pipeline_id: CachedRenderPipelineId,
}

fn init_pipeline(
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    fullscreen_shader: Res<FullscreenShader>,
    pipeline_cache: Res<PipelineCache>,
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
    let shader = asset_server.load(SHADER);
    let pipeline_id = pipeline_cache.queue_render_pipeline(RenderPipelineDescriptor {
        label: Some("outline_pipeline".into()),
        layout: vec![layout.clone()],
        vertex: fullscreen_shader.to_vertex_state(),
        fragment: Some(FragmentState {
            shader,
            targets: vec![Some(ColorTargetState {
                format: ViewTarget::TEXTURE_FORMAT_HDR,
                blend: None,
                write_mask: ColorWrites::ALL,
            })],
            ..default()
        }),
        ..default()
    });
    commands.insert_resource(OutlinePipeline { layout, pipeline_id });
}

fn outline_pass(
    view: ViewQuery<(
        &ViewTarget,
        &ViewPrepassTextures,
        &DynamicUniformIndex<OutlineSettings>,
    )>,
    pipeline: Option<Res<OutlinePipeline>>,
    pipeline_cache: Res<PipelineCache>,
    uniforms: Res<ComponentUniforms<OutlineSettings>>,
    mut ctx: RenderContext,
) {
    let Some(pipeline) = pipeline else { return };
    let (view_target, prepass, index) = view.into_inner();
    if !view_target.is_hdr() {
        return;
    }
    let Some(render_pipeline) = pipeline_cache.get_render_pipeline(pipeline.pipeline_id) else {
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
