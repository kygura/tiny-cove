// PS1 voxel material: snapped vertices, affine UVs, vertex-colour lighting,
// linear fog, 15-bit colour with 4x4 Bayer dithering.

import bevy_pbr::render::{
    mesh_functions::{get_world_from_local, mesh_position_local_to_world, mesh_normal_local_to_world},
    mesh_view_bindings::view,
    view_transformations::position_world_to_clip,
};

struct VoxelParams {
    fog_color: vec4<f32>,
    sun_dir: vec3<f32>,
    fog_start: f32,
    sun_color: vec3<f32>,
    fog_end: f32,
    ambient: vec3<f32>,
    emissive_fog_start: f32,
    snap_res: vec2<f32>,
}

@group(constants::MATERIAL_BIND_GROUP) @binding(0) var<uniform> params: VoxelParams;
@group(constants::MATERIAL_BIND_GROUP) @binding(1) var atlas: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(2) var atlas_sampler: sampler;

struct Vertex {
    @builtin(instance_index) instance_index: u32,
    @location(0) position: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) uv: vec2<f32>,
    @location(5) color: vec4<f32>,
}

struct VertexOut {
    @builtin(position) clip: vec4<f32>,
    // Screen-space linear interpolation = affine texture mapping.
    @location(0) @interpolate(linear) uv: vec2<f32>,
    @location(1) color: vec4<f32>,
    @location(2) world_pos: vec3<f32>,
    @location(3) normal: vec3<f32>,
}

@vertex
fn vertex(v: Vertex) -> VertexOut {
    let world = mesh_position_local_to_world(get_world_from_local(v.instance_index), vec4(v.position, 1.0));
    var clip = position_world_to_clip(world.xyz);
    if clip.w > 0.0 {
        let half = params.snap_res * 0.5;
        let ndc = clip.xy / clip.w;
        clip = vec4(round(ndc * half) / half * clip.w, clip.zw);
    }
    var out: VertexOut;
    out.clip = clip;
    out.uv = v.uv;
    out.color = v.color;
    out.world_pos = world.xyz;
    out.normal = mesh_normal_local_to_world(v.normal, v.instance_index);
    return out;
}

fn to_srgb(c: vec3<f32>) -> vec3<f32> {
    return select(1.055 * pow(c, vec3(1.0 / 2.4)) - 0.055, c * 12.92, c <= vec3(0.0031308));
}

fn to_linear(c: vec3<f32>) -> vec3<f32> {
    return select(pow((c + 0.055) / 1.055, vec3(2.4)), c / 12.92, c <= vec3(0.04045));
}

fn bayer4(p: vec2<u32>) -> f32 {
    var m = array<f32, 16>(0.0, 8.0, 2.0, 10.0, 12.0, 4.0, 14.0, 6.0, 3.0, 11.0, 1.0, 9.0, 15.0, 7.0, 13.0, 5.0);
    return (m[(p.y % 4u) * 4u + p.x % 4u] + 0.5) / 16.0 - 0.5;
}

@fragment
fn fragment(in: VertexOut) -> @location(0) vec4<f32> {
    let tex = textureSample(atlas, atlas_sampler, in.uv).rgb;
    let n = normalize(in.normal);
    let light = params.ambient + params.sun_color * max(dot(n, params.sun_dir), 0.0);
    let e = clamp(in.color.a, 0.0, 1.0);
    var c = mix(tex * in.color.rgb * light, tex, e);

    let dist = distance(in.world_pos, view.world_position);
    let fog = clamp((dist - params.fog_start) / (params.fog_end - params.fog_start), 0.0, 1.0);
    let efog = clamp((dist - params.emissive_fog_start) / (params.fog_end - params.fog_start), 0.0, 1.0);
    c = mix(c, params.fog_color.rgb, mix(fog, efog, e));

    let levels = 31.0;
    let d = bayer4(vec2<u32>(in.clip.xy));
    let q = floor(clamp(to_srgb(c), vec3(0.0), vec3(1.0)) * levels + d + 0.5) / levels;
    return vec4(to_linear(clamp(q, vec3(0.0), vec3(1.0))), 1.0);
}
