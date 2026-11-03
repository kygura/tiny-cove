// Prepass (depth + normal for the outline pass) and shadow pass for GladeMaterial and
// WaterMaterial. The camera prepass snaps vertices exactly like glade.wesl so depth and
// colour line up; the shadow pass (no NORMAL_PREPASS) does not snap.
//
// Normal target: rgb = world normal * 0.5 + 0.5, a = surface cls for the post pass
// (1 = regular, 2/3 = emissive window/glow: no creases, 1/3 = water: shore foam).

import bevy_pbr::render::{
    mesh_functions::{get_world_from_local, mesh_position_local_to_world, mesh_normal_local_to_world},
    mesh_view_bindings::view,
    view_transformations::position_world_to_clip,
};
import bevy_pbr::prepass::io::FragmentOutput;

struct GlobalsHead {
    a: vec4<f32>, b: vec4<f32>, c: vec4<f32>, d: vec4<f32>,
    e: vec4<f32>, f: vec4<f32>, g: vec4<f32>,
    sky_hor_snap: vec4<f32>,
}

@group(constants::MATERIAL_BIND_GROUP) @binding(0) var<storage, read> globals: GlobalsHead;

struct Vertex {
    @builtin(instance_index) instance_index: u32,
    @location(0) position: vec3<f32>,
    @location(2) uv_b: vec2<f32>,
    @location(3) normal: vec3<f32>,
}

struct VertexOut {
    @builtin(position) position: vec4<f32>,
    @location(0) world_normal: vec3<f32>,
    @location(1) @interpolate(flat) surface: u32,
    @location(2) unclipped_depth: f32,
}

@vertex
fn vertex(v: Vertex) -> VertexOut {
    let world = mesh_position_local_to_world(get_world_from_local(v.instance_index), vec4(v.position, 1.0));
    var clip = position_world_to_clip(world.xyz);
    var out: VertexOut;
@if(NORMAL_PREPASS) {
    let snap = globals.sky_hor_snap.w;
    if clip.w > 0.0 && snap > 0.0 {
        let half = max(view.viewport.zw * snap * 0.5, vec2(1.0));
        let ndc = clip.xy / clip.w;
        clip = vec4(round(ndc * half) / half * clip.w, clip.zw);
    }
}
    let surface = u32(max(v.uv_b.x, 0.0) + 0.5);
@if(NORMAL_PREPASS) {
    // Same decal bias as glade.wesl.
    if surface == 1u || (surface >= 12u && surface <= 16u) {
        clip.z += 0.12 * view.clip_from_view[2].z * clip.w;
    }
}
    out.unclipped_depth = clip.z;
@if(UNCLIPPED_DEPTH_ORTHO_EMULATION) {
    clip.z = min(clip.z, 1.0);
}
    out.position = clip;
    out.world_normal = mesh_normal_local_to_world(v.normal, v.instance_index);
    out.surface = surface;
    return out;
}

@if(PREPASS_FRAGMENT)
@fragment
fn fragment(in: VertexOut, @builtin(front_facing) is_front: bool) -> FragmentOutput {
    var out: FragmentOutput;
@if(NORMAL_PREPASS) {
    var cls = 1.0;
    if in.surface == 15u || in.surface == 22u {
        cls = 2.0 / 3.0;
    }
    if in.surface == 23u {
        cls = 1.0 / 3.0;
    }
    var n = vec3(0.0, 1.0, 0.0);
    if dot(in.world_normal, in.world_normal) > 1e-12 {
        n = normalize(in.world_normal);
    }
    if !is_front {
        n = -n;
    }
    out.normal = vec4(n * 0.5 + vec3(0.5), cls);
}
@if(UNCLIPPED_DEPTH_ORTHO_EMULATION)
    out.frag_depth = in.unclipped_depth;
    return out;
}
@else
@fragment
fn fragment(in: VertexOut) {}
