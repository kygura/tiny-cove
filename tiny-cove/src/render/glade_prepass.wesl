// Prepass (depth + normal for the outline pass) and shadow pass for GladeMaterial,
// WaterMaterial and SeaMaterial. The camera prepass snaps vertices exactly like glade.wesl so
// depth and colour line up; the shadow pass (no NORMAL_PREPASS) does not snap. Wind sway and
// sea waves are mirrored from glade.wesl so every pass agrees.
//
// Normal target: rgb = world normal * 0.5 + 0.5, a = surface cls for the post pass
// (1 = regular, 2/3 = emissive window/glow: no creases, 1/3 = water: shore foam). Water
// writes a fixed normal: +y for the open sea, +z for lagoons (post: no foam, lagoon rim).

import bevy_pbr::render::{
    mesh_functions::{get_world_from_local, mesh_position_local_to_world, mesh_normal_local_to_world},
    mesh_view_bindings::view,
    view_transformations::position_world_to_clip,
};
import bevy_pbr::prepass::io::FragmentOutput;

struct GlobalsHead {
    a: vec4<f32>, b: vec4<f32>, c: vec4<f32>, d: vec4<f32>,
    e: vec4<f32>, f: vec4<f32>,
    sky_top_time: vec4<f32>,
    sky_hor_snap: vec4<f32>,
    wind_tide: vec4<f32>,
    sea0: vec4<f32>,
    sea1: vec4<f32>, sea2: vec4<f32>, sea3: vec4<f32>, sea4: vec4<f32>,
    sea5: vec4<f32>,
    outline: vec4<f32>,
    field: vec4<f32>,
}

@group(constants::MATERIAL_BIND_GROUP) @binding(0) var<storage, read> globals: GlobalsHead;
@group(constants::MATERIAL_BIND_GROUP) @binding(1) var atlas: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(3) var heightfield: texture_2d<f32>;

const S_LEAVES: u32 = 19u;
const S_FIBER: u32 = 20u;
const S_WATER: u32 = 24u;

struct Vertex {
    @builtin(instance_index) instance_index: u32,
    @location(0) position: vec3<f32>,
    @location(2) uv_b: vec2<f32>,
    @location(3) normal: vec3<f32>,
    @location(4) uv: vec2<f32>,
    @location(5) color: vec4<f32>,
}

struct VertexOut {
    @builtin(position) position: vec4<f32>,
    @location(0) world_normal: vec3<f32>,
    @location(1) @interpolate(flat) surface: u32,
    @location(2) unclipped_depth: f32,
    @location(3) @interpolate(linear) uv: vec2<f32>,
    @location(4) world_y: f32,
    @location(5) world_xz: vec2<f32>,
}

fn sway(world: vec3<f32>, s: u32) -> vec3<f32> {
    if s != S_LEAVES && s != S_FIBER {
        return world;
    }
    let w = globals.wind_tide.xy;
    let strength = length(w);
    let t = globals.sky_top_time.w;
    let q = floor(world * 4.0 + 0.5);
    let ph = fract(sin(dot(q, vec3(12.9898, 78.233, 37.719))) * 43758.5453) * 6.2831;
    let dir = select(vec2(1.0, 0.0), w / max(strength, 1e-4), strength > 1e-3);
    let k = 0.03 + 0.018 * min(strength, 10.0);
    let gust = sin(t * (1.7 + 0.12 * strength) + ph) * 0.6 + sin(t * 3.3 + ph * 2.0) * 0.4;
    let lean = 0.5 * min(strength / 6.0, 1.0);
    return world + vec3(dir.x, 0.0, dir.y) * k * (gust + lean) + vec3(0.0, k * 0.35 * gust, 0.0);
}

// Pennant flutter (vertex colour alpha = 1 - flutter weight); mirrors glade.wesl.
fn flutter(world: vec3<f32>, alpha: f32) -> vec3<f32> {
    if alpha >= 0.999 {
        return world;
    }
    let w = globals.wind_tide.xy;
    let strength = length(w);
    let dir = select(vec2(1.0, 0.0), w / max(strength, 1e-4), strength > 1e-3);
    let t = globals.sky_top_time.w;
    let along = dot(world.xz, dir);
    let gust = 0.11 + 0.05 * sin(t * (5.0 + 0.4 * min(strength, 10.0)) - along * 3.1) + 0.03 * sin(t * 1.3 + along);
    return world + vec3(w.x, 0.0, w.y) * (1.0 - clamp(alpha, 0.0, 1.0)) * gust;
}

// Open sea beyond the horizon line (outline.w metres from the island centre along the
// camera's ground-forward axis) is cut away so the post pass paints sky there. Mirrors
// glade.wesl.
fn past_horizon(xz: vec2<f32>) -> bool {
    let back = view.world_from_view[2].xyz;
    let l = length(back.xz);
    return l > 1e-3 && dot(xz, -back.xz / l) > globals.outline.w;
}

fn seabed(p: vec2<f32>) -> f32 {
    let f = globals.field;
    let n = max(f.w, 2.0);
    let q = (p - f.xy) / max(f.z, 1e-4);
    let qc = clamp(q, vec2(0.0), vec2(n - 1.001));
    let i = vec2<i32>(floor(qc));
    let fr = qc - floor(qc);
    let h00 = textureLoad(heightfield, i, 0).r;
    let h10 = textureLoad(heightfield, i + vec2(1, 0), 0).r;
    let h01 = textureLoad(heightfield, i + vec2(0, 1), 0).r;
    let h11 = textureLoad(heightfield, i + vec2(1, 1), 0).r;
    let h = mix(mix(h00, h10, fr.x), mix(h01, h11, fr.x), fr.y);
    // Radial, not box, distance past the field edge, so the abyss step is a ring, not a square.
    let outside = max(length(q - (n - 1.0) * 0.5) - (n - 1.0) * 0.5, 0.0) * f.z;
    return mix(h, min(h, -24.0), clamp(outside / 30.0, 0.0, 1.0));
}

fn wind_dir() -> vec2<f32> {
    let w = globals.wind_tide.xy;
    let s = length(w);
    return select(vec2(0.96, 0.29), w / max(s, 1e-4), s > 1e-3);
}

fn wave(p: vec2<f32>, t: f32) -> f32 {
    let dir = wind_dir();
    let perp = vec2(-dir.y, dir.x);
    let a = sin(dot(p, dir) * 1.1 - t * 1.4);
    let b = sin(dot(p, normalize(dir + perp * 0.6)) * 1.9 - t * 2.1 + 1.3);
    let c = sin(dot(p, perp) * 0.7 + t * 0.8);
    let swell = sin(dot(p, dir) * 0.32 - t * 0.9) * globals.sea0.w;
    return a * 0.5 + b * 0.3 + c * 0.2 + swell * 0.8;
}

fn wave_amp() -> f32 {
    let s = globals.sea5.w;
    return (0.05 + 0.012 * min(s, 12.0)) * (1.0 + 1.5 * globals.sea0.w);
}

fn sea_height(p: vec2<f32>) -> f32 {
    let depth = globals.wind_tide.z - seabed(p);
    let damp = clamp(depth / 1.5, 0.15, 1.0);
    return globals.wind_tide.z + wave(p, globals.sky_top_time.w) * wave_amp() * damp;
}

fn finish(world: vec3<f32>, normal: vec3<f32>, surface: u32, uv: vec2<f32>) -> VertexOut {
    var clip = position_world_to_clip(world);
    var out: VertexOut;
@if(NORMAL_PREPASS) {
    let snap = globals.sky_hor_snap.w;
    if clip.w > 0.0 && snap > 0.0 {
        let half = max(view.viewport.zw * snap * 0.5, vec2(1.0));
        let ndc = clip.xy / clip.w;
        clip = vec4(round(ndc * half) / half * clip.w, clip.zw);
    }
    // Same decal bias as glade.wesl.
    if surface == 1u || (surface >= 12u && surface <= 17u) {
        clip.z += 0.12 * view.clip_from_view[2].z * clip.w;
    }
}
    out.unclipped_depth = clip.z;
@if(UNCLIPPED_DEPTH_ORTHO_EMULATION) {
    clip.z = min(clip.z, 1.0);
}
    out.position = clip;
    out.world_normal = normal;
    out.surface = surface;
    out.uv = uv;
    out.world_y = world.y;
    out.world_xz = world.xz;
    return out;
}

@vertex
fn vertex(v: Vertex) -> VertexOut {
    let world = mesh_position_local_to_world(get_world_from_local(v.instance_index), vec4(v.position, 1.0));
    let surface = u32(max(v.uv_b.x, 0.0) + 0.5);
    let n = mesh_normal_local_to_world(v.normal, v.instance_index);
    return finish(flutter(sway(world.xyz, surface), v.color.a), n, surface, v.uv);
}

@vertex
fn sea_vertex(v: Vertex) -> VertexOut {
    let w0 = mesh_position_local_to_world(get_world_from_local(v.instance_index), vec4(v.position, 1.0));
    var out = finish(vec3(w0.x, sea_height(w0.xz), w0.z), vec3(0.0, 1.0, 0.0), S_WATER, v.uv);
    // Sea, not lagoon: tag with y = SEA (+1) so the fragment keeps the sea normal.
    out.world_y = 1e6;
    return out;
}

fn atlas_alpha(surface: u32, uv: vec2<f32>) -> f32 {
    let s = min(surface, S_WATER);
    let f = fract(uv);
    let t = min(vec2<u32>(vec2(f.x, 1.0 - f.y) * 32.0), vec2(31u));
    let origin = vec2((s % 8u) * 32u, (s / 8u) * 32u);
    return textureLoad(atlas, origin + t, 0).a;
}

@if(PREPASS_FRAGMENT)
@fragment
fn fragment(in: VertexOut, @builtin(front_facing) is_front: bool) -> FragmentOutput {
    var out: FragmentOutput;
    // Cutout texels (leaves, nets, petals) are holes in every pass.
    if in.surface != S_WATER && atlas_alpha(in.surface, in.uv) < 0.25 {
        discard;
    }
    // Lagoon water at mean sea level is left to the sea mesh (matches water_fragment).
    if in.surface == S_WATER && in.world_y < 1e5 && abs(in.world_y) < 0.03 {
        discard;
    }
@if(NORMAL_PREPASS) {
    // Sea and sea bed past the horizon line: the post pass paints sky there.
    if ((in.surface == S_WATER && in.world_y > 1e5) || in.world_y < globals.wind_tide.z - 0.05) && past_horizon(in.world_xz) {
        discard;
    }
}
@if(NORMAL_PREPASS) {
    var cls = 1.0;
    if in.surface == 16u || in.surface == 23u {
        cls = 2.0 / 3.0;
    }
    var n = vec3(0.0, 1.0, 0.0);
    if dot(in.world_normal, in.world_normal) > 1e-12 {
        n = normalize(in.world_normal);
    }
    if !is_front {
        n = -n;
    }
    if in.surface == S_WATER {
        cls = 1.0 / 3.0;
        n = select(vec3(0.0, 0.0, 1.0), vec3(0.0, 1.0, 0.0), in.world_y > 1e5);
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
