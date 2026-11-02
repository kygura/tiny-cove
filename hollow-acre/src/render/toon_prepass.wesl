// Toon prepass: world normal in rgb, surface class in the 2-bit alpha for the outline pass:
//   1.0 = regular, 2/3 = emissive (silhouette outlines only), 1/3 = water (foam).

import bevy_pbr::prepass::io::{VertexOutput, FragmentOutput};

struct ToonParams {
    base_color: vec4<f32>,
    emissive: vec4<f32>,
    emissive_day: f32,
    emissive_night: f32,
    mode: u32,
    _pad: f32,
}

// Only the first field of the shared globals is needed here (w = night).
struct ToonGlobalsHead {
    sun_dir_night: vec4<f32>,
}

@group(constants::MATERIAL_BIND_GROUP) @binding(0) var<uniform> params: ToonParams;
@group(constants::MATERIAL_BIND_GROUP) @binding(1) var<storage, read> g: ToonGlobalsHead;

@if(PREPASS_FRAGMENT)
@fragment
fn fragment(in: VertexOutput, @builtin(front_facing) is_front: bool) -> FragmentOutput {
    var out: FragmentOutput;

@if(NORMAL_PREPASS) {
    var water = params.mode == 1u;
    var strength = mix(params.emissive_day, params.emissive_night, g.sun_dir_night.w);
@if(VERTEX_UVS_A)
    strength = mix(in.uv.x, in.uv.y, g.sun_dir_night.w);
@if(VERTEX_UVS_B)
    water = water || in.uv_b.y > 0.5;
    var surface = 1.0;
    if strength > 0.0 {
        surface = 2.0 / 3.0;
    }
    if water {
        surface = 1.0 / 3.0;
    }
    var n = vec3(0.0, 1.0, 0.0);
    if dot(in.world_normal, in.world_normal) > 1e-12 {
        n = normalize(in.world_normal);
    }
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
