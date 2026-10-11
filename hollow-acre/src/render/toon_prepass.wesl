// Toon prepass: world normal in rgb, surface class in the 2-bit alpha for the outline pass:
//   1.0 = regular, 2/3 = emissive (silhouette outlines only), 1/3 = water (foam).

import bevy_pbr::prepass::io::{VertexOutput, FragmentOutput};

struct ToonParams {
    base_color: vec4<f32>,
    emissive: vec4<f32>,
    sun_dir: vec3<f32>,
    emissive_day: f32,
    sun_color: vec3<f32>,
    emissive_night: f32,
    ambient: vec3<f32>,
    night: f32,
    shadow_tint: vec3<f32>,
    shadow_strength: f32,
    sky_color: vec3<f32>,
    fog_density: f32,
    fog_color: vec3<f32>,
    height_fog: f32,
    cloud_offset: vec2<f32>,
    cloud_cover: f32,
    wetness: f32,
    time: f32,
    mode: u32,
    fog_start: f32,
    _pad: f32,
}

@group(constants::MATERIAL_BIND_GROUP) @binding(0) var<uniform> params: ToonParams;

@if(PREPASS_FRAGMENT)
@fragment
fn fragment(in: VertexOutput, @builtin(front_facing) is_front: bool) -> FragmentOutput {
    var out: FragmentOutput;

@if(NORMAL_PREPASS) {
    var water = params.mode == 1u;
    var strength = mix(params.emissive_day, params.emissive_night, params.night);
@if(VERTEX_UVS_A)
    strength = mix(in.uv.x, in.uv.y, params.night);
@if(VERTEX_UVS_B)
    water = water || in.uv_b.y > 0.5;
    var surface = 1.0;
    if strength > 0.0 {
        surface = 2.0 / 3.0;
    }
    if water {
        surface = 1.0 / 3.0;
    }
    var n = normalize(in.world_normal);
    if !is_front {
        n = -n;
    }
    out.normal = vec4(n * 0.5 + vec3(0.5), surface);
}

@if(UNCLIPPED_DEPTH_ORTHO_EMULATION)
    out.frag_depth = in.unclipped_depth;

    return out;
}
@else
@fragment
fn fragment(in: VertexOutput) {}
