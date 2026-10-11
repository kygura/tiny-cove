// GREYWILD main pass: PS1 rasteriser (snapped vertices, affine UVs, nearest atlas)
// with Tiny Glade-ish cel lighting (3 hard bands from N.L x sun shadow map, cool shadow
// tint, hemispheric ambient, rim), weather (wet glints, snow cover, lightning) and fog.
// 15-bit quantise + Bayer dither happen in the post pass (post.wesl) so outlines and
// sky are dithered too.
//
// Entry points: vertex/fragment (GladeMaterial), water_fragment (WaterMaterial),
// particle_vertex/particle_fragment (ParticleMaterial).

import bevy_pbr::render::{
    mesh_functions::{get_world_from_local, mesh_position_local_to_world, mesh_normal_local_to_world},
    mesh_view_bindings::{view, lights, clustered_lights},
    view_transformations::position_world_to_clip,
    shadows::fetch_directional_shadow,
    mesh_view_types::DIRECTIONAL_LIGHT_FLAGS_SHADOWS_ENABLED_BIT,
    clustered_forward::{view_fragment_cluster_index, unpack_clusterable_object_index_ranges, get_clusterable_object_id},
};

// Mirrors `globals_from_env` in material.rs.
struct Globals {
    sun_dir_night: vec4<f32>,      // xyz towards sun, w night 0..1
    sun_color_wet: vec4<f32>,      // xyz sun colour x intensity, w wetness
    sky_amb_snow: vec4<f32>,       // xyz ambient from sky, w snow
    ground_amb_flash: vec4<f32>,   // xyz ambient from ground, w lightning flash
    tint_fog_density: vec4<f32>,   // xyz shadow tint, w fog density
    fog_color_start: vec4<f32>,    // xyz fog colour, w fog start (view depth)
    sky_top_time: vec4<f32>,       // xyz sky top, w time (s)
    sky_hor_snap: vec4<f32>,       // xyz sky horizon, w snap fraction of low-res grid
    wind: vec4<f32>,               // xy wind
}

struct WaterParams {
    deep: vec4<f32>,
    shallow: vec4<f32>,
}

@group(constants::MATERIAL_BIND_GROUP) @binding(0) var<storage, read> g: Globals;
@group(constants::MATERIAL_BIND_GROUP) @binding(1) var atlas: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(2) var<uniform> water: WaterParams;

// Surface ids (greywild_core::Surface).
const S_GRASS: u32 = 0u;
const S_GRAVEL: u32 = 3u;
const S_CONCRETE: u32 = 4u;
const S_PLINTH: u32 = 5u;
const S_EARTH: u32 = 6u;
const S_STEEL: u32 = 8u;
const S_SEDUM: u32 = 9u;
const S_PRECAST: u32 = 11u;
const S_PAVERS: u32 = 12u;
const S_POURED: u32 = 14u;
const S_WINDOW: u32 = 15u;
const S_LEAVES: u32 = 18u;
const S_FIBER: u32 = 19u;
const S_GLOW: u32 = 22u;
const S_WATER: u32 = 23u;
const TILE: u32 = 32u;
const COLS: u32 = 8u;

struct Vertex {
    @builtin(instance_index) instance_index: u32,
    @location(0) position: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) uv: vec2<f32>,
    @location(3) uv_b: vec2<f32>,
    @location(5) color: vec4<f32>,
}

struct VertexOut {
    @builtin(position) clip: vec4<f32>,
    // Screen-space linear interpolation = PS1 affine texture mapping.
    @location(0) @interpolate(linear) uv: vec2<f32>,
    @location(1) color: vec4<f32>,
    @location(2) world_pos: vec4<f32>,
    @location(3) normal: vec3<f32>,
    @location(4) @interpolate(flat) surface: u32,
}

// Decals (paths on terrain, windows/doors on walls) win against coplanar terrain despite vertex snapping:
// pull them 0.12 m towards the camera (orthographic: ndc z is linear in depth).
fn decal_bias(clip: vec4<f32>, surface: u32) -> vec4<f32> {
    if surface == 1u || (surface >= S_PAVERS && surface <= 16u) {
        return vec4(clip.xy, clip.z + 0.12 * view.clip_from_view[2].z * clip.w, clip.w);
    }
    return clip;
}

fn snap_clip(clip: vec4<f32>) -> vec4<f32> {
    let snap = g.sky_hor_snap.w;
    if clip.w <= 0.0 || snap <= 0.0 {
        return clip;
    }
    let half = max(view.viewport.zw * snap * 0.5, vec2(1.0));
    let ndc = clip.xy / clip.w;
    return vec4(round(ndc * half) / half * clip.w, clip.zw);
}

@vertex
fn vertex(v: Vertex) -> VertexOut {
    let world = mesh_position_local_to_world(get_world_from_local(v.instance_index), vec4(v.position, 1.0));
    var out: VertexOut;
    out.surface = u32(max(v.uv_b.x, 0.0) + 0.5);
    out.clip = decal_bias(snap_clip(position_world_to_clip(world.xyz)), out.surface);
    out.uv = v.uv;
    out.color = v.color;
    out.world_pos = world;
    out.normal = mesh_normal_local_to_world(v.normal, v.instance_index);
    return out;
}

fn hash2(p: vec2<f32>) -> f32 {
    return fract(sin(dot(p, vec2(127.1, 311.7))) * 43758.5453);
}

fn value_noise(p: vec2<f32>) -> f32 {
    let i = floor(p);
    let f = fract(p);
    let u = f * f * (3.0 - 2.0 * f);
    return mix(mix(hash2(i), hash2(i + vec2(1.0, 0.0)), u.x),
        mix(hash2(i + vec2(0.0, 1.0)), hash2(i + vec2(1.0, 1.0)), u.x), u.y);
}

fn atlas_texel(surface: u32, uv: vec2<f32>) -> vec4<f32> {
    let s = min(surface, S_WATER);
    let f = fract(uv);
    let t = min(vec2<u32>(vec2(f.x, 1.0 - f.y) * f32(TILE)), vec2(TILE - 1u));
    let origin = vec2((s % COLS) * TILE, (s / COLS) * TILE);
    return textureLoad(atlas, origin + t, 0);
}

fn view_depth(world: vec4<f32>) -> f32 {
    return -dot(vec4(view.view_from_world[0].z, view.view_from_world[1].z,
        view.view_from_world[2].z, view.view_from_world[3].z), world);
}

fn fog_amount(depth: f32) -> f32 {
    let d = max(depth - g.fog_color_start.w, 0.0);
    return 1.0 - exp(-g.tint_fog_density.w * d);
}

fn sun_shadow(world: vec4<f32>, n: vec3<f32>, frag_xy: vec2<f32>) -> f32 {
    if lights.n_directional_lights == 0u {
        return 1.0;
    }
    if (lights.directional_lights[0].flags & DIRECTIONAL_LIGHT_FLAGS_SHADOWS_ENABLED_BIT) == 0u {
        return 1.0;
    }
    let view_z = -view_depth(world);
    let s = fetch_directional_shadow(0u, world, n, view_z, frag_xy);
    return select(0.0, 1.0, s > 0.5);
}

fn hemi_ambient(n: vec3<f32>) -> vec3<f32> {
    return mix(g.ground_amb_flash.xyz, g.sky_amb_snow.xyz, n.y * 0.5 + 0.5);
}

// Toward the (orthographic) camera.
fn to_camera() -> vec3<f32> {
    return normalize(view.world_from_view[2].xyz);
}

// Clustered point lights (lanterns) in 2 hard bands. Colour scale: a PointLight of
// 1000 lm (see `lantern_light`) contributes ~1x its colour at full band.
fn point_lights(world: vec4<f32>, n: vec3<f32>, frag: vec4<f32>) -> vec3<f32> {
    let view_z = -view_depth(world);
    let is_ortho = view.clip_from_view[3].w == 1.0;
    let cluster = view_fragment_cluster_index(frag.xy, view_z, is_ortho);
    let ranges = unpack_clusterable_object_index_ranges(cluster);
    var acc = vec3(0.0);
    for (var i = ranges.first_point_light_index_offset; i < ranges.first_spot_light_index_offset; i++) {
        let id = get_clusterable_object_id(i);
        let light = clustered_lights.data[id];
        let to = light.position_radius.xyz - world.xyz;
        let dist = length(to);
        let range = inverseSqrt(max(light.color_inverse_square_range.w, 1e-6));
        if dist < range && dist > 1e-4 {
            let att = 1.0 - dist / range;
            let k = att * att * max(dot(n, to / dist) * 0.7 + 0.3, 0.0);
            let band = select(select(0.0, 0.45, k > 0.04), 1.0, k > 0.25);
            acc += light.color_inverse_square_range.xyz * (1.0 / 80.0) * band;
        }
    }
    return acc;
}

fn is_ground(s: u32) -> bool {
    return s <= S_PLINTH || (s >= S_PAVERS && s <= S_POURED);
}

// Cast mineral surfaces that weather: moss on tops, rain streaks on faces.
fn is_cast(s: u32) -> bool {
    return s == S_CONCRETE || s == S_PLINTH || s == S_EARTH || s == S_PRECAST;
}

@fragment
fn fragment(in: VertexOut, @builtin(front_facing) is_front: bool) -> @location(0) vec4<f32> {
    var n = normalize(in.normal);
    if !is_front {
        n = -n;
    }
    let s = in.surface;
    let world = in.world_pos;
    let night = g.sun_dir_night.w;
    let wet = g.sun_color_wet.w;
    let snow = g.sky_amb_snow.w;
    let flash = g.ground_amb_flash.w;
    let sun_dir = normalize(g.sun_dir_night.xyz);
    let sun = g.sun_color_wet.xyz;

    let tex = atlas_texel(s, in.uv);
    let glass = tex.a > 0.9;
    var albedo = select(tex.rgb * in.color.rgb, tex.rgb, glass && s == S_WINDOW);
    if s == S_GLOW {
        albedo = tex.rgb * in.color.rgb;
    }

    // Snow on up-facing surfaces, hard noisy edge.
    var snowed = false;
    if snow > 0.0 && s != S_WINDOW && s != S_GLOW && s != S_WATER {
        let cell = floor(world.xz * 3.0);
        let noise = hash2(cell) * 0.35 + hash2(floor(world.xz * 0.7) + 17.0) * 0.25;
        let cover = (n.y - 0.5) * 2.0 * snow * 1.4 - noise;
        if n.y > 0.5 && cover > 0.15 {
            snowed = true;
            albedo = vec3(0.88, 0.91, 0.96) * mix(1.0, tex.r, 0.15);
        }
    }

    // Wet darkening (ground/stone stronger).
    let ground = is_ground(s);
    if wet > 0.0 && !snowed && !glass {
        albedo *= 1.0 - wet * select(0.18, 0.35, ground);
    }

    let shadow = sun_shadow(world, n, in.clip.xy);
    let ndl = dot(n, sun_dir);
    let light = max(ndl, 0.0) * shadow;
    let amb = hemi_ambient(n);
    var col: vec3<f32>;
    if s == S_GLOW {
        col = albedo * 1.2;
    } else {
        // 3 hard bands: lit / mid / shadow (shadow = albedo x ambient x cool tint).
        let shade = albedo * amb * g.tint_fog_density.xyz * 1.35;
        let mid = albedo * (amb * 0.62 + sun * 0.17);
        let lit = albedo * (amb * 0.4 + sun * 0.36);
        if light < 0.06 {
            col = shade;
        } else if light < 0.22 {
            col = mid;
        } else {
            col = lit;
        }
        // Rim light, subtle: grazing faces pick up sky colour.
        let rim = pow(1.0 - max(dot(n, to_camera()), 0.0), 4.0);
        col += albedo * g.sky_amb_snow.xyz * rim * 0.35;
        // Lanterns.
        col += albedo * point_lights(world, n, in.clip);
        // Wet ground: noisy puddles reflect a little sky, with a hard specular glint
        // band (relative to a flat surface, since the iso view never sees a mirror angle).
        if wet > 0.05 && ground && s != S_GRASS && s != S_SAND && n.y > 0.7 && !snowed {
            let puddle = value_noise(world.xz * 0.9) * 0.7 + value_noise(world.xz * 2.3 + 7.0) * 0.3;
            if puddle > 1.0 - wet * 0.42 {
                col = mix(col * 0.7, g.sky_hor_snap.xyz * amb * 1.2, 0.35);
                let h = normalize(sun_dir + to_camera());
                let jitter = (tex.g - 0.8) * 0.8 + (hash2(floor(world.xz * 6.0)) - 0.5) * 0.5;
                let sp = dot(normalize(n + vec3(jitter, 0.0, -jitter)), h);
                if sp > h.y + 0.12 && shadow > 0.5 {
                    col += sun * 0.4 * wet;
                }
            }
        }
    }

    // Emissive: windows glow warm at night (and a bit in storms); glow always.
    var emissive = vec3(0.0);
    if s == S_WINDOW && glass {
        let strength = clamp(night + wet * 0.25, 0.0, 1.0);
        let lum = dot(tex.rgb, vec3(0.3, 0.5, 0.2));
        emissive = vec3(1.0, 0.66, 0.3) * (0.8 + lum * 1.5) * strength * tex.a;
        col = mix(col, vec3(0.0), strength * 0.6);
    } else if s == S_GLOW {
        emissive = albedo * (0.4 + 0.6 * night);
    }

    // Lightning: cold white boost.
    col += albedo * vec3(0.75, 0.82, 1.0) * flash * 1.2;

    let fog = fog_amount(view_depth(world));
    let out = mix(col, g.fog_color_start.xyz, fog) + emissive * (1.0 - 0.6 * fog);
    return vec4(out, 1.0);
}

// ---------------------------------------------------------------- water

@fragment
fn water_fragment(in: VertexOut) -> @location(0) vec4<f32> {
    let world = in.world_pos;
    let t = g.sky_top_time.w;
    let night = g.sun_dir_night.w;
    let p = world.xz;
    // Two drifting ripple fields, quantised into hard bands.
    let wave = sin(p.x * 2.1 + p.y * 1.2 + t * 1.1 + sin(p.y * 1.4 - t * 0.6) * 1.5)
        + sin(p.y * 2.6 - p.x * 0.7 - t * 0.8 + sin(p.x * 1.1 + t * 0.3) * 1.2);
    // Quantised: deep body, lighter ripple bands, rare bright crests.
    var k = 0.0;
    if wave > 0.6 && wave < 1.0 {
        k = 0.55;
    } else if wave >= 1.0 {
        k = 1.0;
    } else if wave < -1.2 {
        k = -0.35;
    }
    var base = mix(water.deep.rgb, water.shallow.rgb, 0.3 + 0.45 * k) * in.color.rgb;
    let sky = mix(g.sky_hor_snap.xyz, g.sky_top_time.xyz, 0.4);
    base = mix(base, sky, 0.22);
    let amb = g.sky_amb_snow.xyz * 0.8 + g.sun_color_wet.xyz * 0.25;
    var col = base * amb;
    if wave > 1.9 {
        col = mix(col, g.sun_color_wet.xyz * 0.45 + vec3(0.15), 0.6 * (1.0 - night * 0.7));
    }
    let shadow = sun_shadow(world, vec3(0.0, 1.0, 0.0), in.clip.xy);
    col *= mix(0.72, 1.0, shadow);
    col += point_lights(world, vec3(0.0, 1.0, 0.0), in.clip) * base;
    col += base * vec3(0.75, 0.82, 1.0) * g.ground_amb_flash.w;
    let fog = fog_amount(view_depth(world));
    return vec4(mix(col, g.fog_color_start.xyz, fog), 1.0);
}

// ---------------------------------------------------------------- particles
//
// Every particle is 4 vertices sharing its centre POSITION; UV_0 = corner (-1..1),
// UV_1 = (size m, kind), COLOR = linear rgb + alpha (dither), NORMAL = streak direction.

struct ParticleVertex {
    @builtin(instance_index) instance_index: u32,
    @location(0) position: vec3<f32>,
    @location(1) corner: vec2<f32>,
    @location(2) size_kind: vec2<f32>,
    @location(3) color: vec4<f32>,
    @location(4) dir: vec3<f32>,
}

struct ParticleOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) corner: vec2<f32>,
    @location(1) color: vec4<f32>,
    @location(2) @interpolate(flat) kind: u32,
    @location(3) depth: f32,
    @location(4) seed: f32,
}

const P_FLAKE: u32 = 0u;
const P_STREAK: u32 = 1u;
const P_PUFF: u32 = 2u;
const P_BIRD: u32 = 3u;
const P_SPARK: u32 = 4u;

@vertex
fn particle_vertex(v: ParticleVertex) -> ParticleOut {
    let world = mesh_position_local_to_world(get_world_from_local(v.instance_index), vec4(v.position, 1.0));
    var center = view.view_from_world * world;
    let kind = u32(v.size_kind.y + 0.5);
    let size = v.size_kind.x;
    var ax = vec2(1.0, 0.0);
    var ay = vec2(0.0, 1.0);
    var half = vec2(size * 0.5);
    if kind == P_STREAK {
        let d = (view.view_from_world * vec4(v.dir, 0.0)).xy;
        if dot(d, d) > 1e-8 {
            ay = normalize(d);
            ax = vec2(ay.y, -ay.x);
        }
        // ~1 low-res pixel wide.
        let px = 2.0 / (view.clip_from_view[1].y * view.viewport.w);
        half = vec2(px * 0.6, size * 0.5);
    }
    let offset = ax * v.corner.x * half.x + ay * v.corner.y * half.y;
    center = vec4(center.xy + offset, center.zw);
    var out: ParticleOut;
    out.clip = snap_clip(view.clip_from_view * center);
    out.corner = v.corner;
    out.color = v.color;
    out.kind = kind;
    out.depth = -center.z;
    out.seed = fract(dot(v.position, vec3(0.13, 0.71, 0.37)));
    return out;
}

fn bayer4(p: vec2<u32>) -> f32 {
    var m = array<f32, 16>(0.0, 8.0, 2.0, 10.0, 12.0, 4.0, 14.0, 6.0, 3.0, 11.0, 1.0, 9.0, 15.0, 7.0, 13.0, 5.0);
    return (m[(p.y % 4u) * 4u + p.x % 4u] + 0.5) / 16.0;
}

@fragment
fn particle_fragment(in: ParticleOut) -> @location(0) vec4<f32> {
    let c = in.corner;
    let r = length(c);
    var alpha = in.color.a;
    var col = in.color.rgb;
    let lit = g.sky_amb_snow.xyz * 0.7 + g.sun_color_wet.xyz * 0.35;
    switch in.kind {
        case P_FLAKE: {
            if r > 0.9 { discard; }
            col *= lit;
        }
        case P_STREAK: {
            col *= 0.6 + 0.4 * (g.sky_amb_snow.xyz + g.sun_color_wet.xyz * 0.3);
        }
        case P_PUFF: {
            if r > 1.0 { discard; }
            alpha *= 1.0 - r * r * 0.8;
            col *= lit * mix(0.85, 1.1, step(0.0, c.y));
        }
        case P_BIRD: {
            // Flapping V silhouette.
            let flap = sin(g.sky_top_time.w * 9.0 + in.seed * 40.0) * 0.45;
            let wing = abs(c.x) * (0.25 + flap) - c.y * 0.6;
            let body = abs(c.x) < 0.12 && abs(c.y) < 0.25;
            if !(abs(wing) < 0.14 || body) || abs(c.x) > 0.95 { discard; }
            col *= mix(lit, vec3(1.0), 0.3);
        }
        default: {
            // Spark / firefly: bright core, dithered halo, unlit.
            if r > 1.0 { discard; }
            alpha *= select(1.0 - r, 1.0, r < 0.35);
        }
    }
    if alpha < bayer4(vec2<u32>(in.clip.xy)) {
        discard;
    }
    col += col * vec3(0.75, 0.82, 1.0) * g.ground_amb_flash.w;
    var fog = fog_amount(in.depth);
    if in.kind == P_SPARK {
        fog *= 0.5;
    }
    return vec4(mix(col, g.fog_color_start.xyz, fog), 1.0);
}
