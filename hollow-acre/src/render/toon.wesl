// Hollow Acre toon shading: 3 cel bands from N.L x hard shadow map, cool shadow tint,
// vertex-colour albedo with per-voxel value jitter, subtle vertex AO, cloud shadows,
// emissive added after lighting, distance + height fog.
//
// Vertex data (voxel meshes, see `voxel_mesh`):
//   COLOR  = linear albedo rgb, a = AO (0..1)
//   UV_0   = (emissive day strength, emissive night strength)
//   UV_1   = (emissive sRGB packed as r*65536 + g*256 + b, flags: 1 = water)
// Meshes without vertex colours (props) use the material uniform instead.

import bevy_pbr::render::{
    forward_io::VertexOutput,
    mesh_view_bindings::{view, lights},
    shadows::fetch_directional_shadow,
    mesh_view_types::DIRECTIONAL_LIGHT_FLAGS_SHADOWS_ENABLED_BIT,
};

// Mirrors `ToonParams` in toon.rs (per-material, rarely changes).
struct ToonParams {
    base_color: vec4<f32>,
    emissive: vec4<f32>,
    emissive_day: f32,
    emissive_night: f32,
    mode: u32,
    _pad: f32,
}

// Mirrors `ToonGlobals::to_vec4s` in toon.rs: one shared buffer for all toon materials,
// rewritten once per frame.
struct ToonGlobals {
    sun_dir_night: vec4<f32>,      // xyz sun dir, w night 0..1
    sun_color_shadow: vec4<f32>,   // xyz linear sun colour x intensity, w shadow strength
    ambient_wet: vec4<f32>,        // xyz linear ambient (incl. flash), w wetness
    tint_fog_density: vec4<f32>,   // xyz shadow tint, w fog density
    sky_height_fog: vec4<f32>,     // xyz sky, w height fog density
    fog_color_start: vec4<f32>,    // xyz fog colour, w fog start (view depth)
    cloud: vec4<f32>,              // xy cloud offset, z cover, w time
    misc: vec4<f32>,               // x lamp count, y global dim, zw unused
    lamps: array<vec4<f32>, 32>,
}

fn to_linear(c: vec3<f32>) -> vec3<f32> {
    return select(pow((c + 0.055) / 1.055, vec3(2.4)), c / 12.92, c <= vec3(0.04045));
}

fn unpack_srgb(packed: f32) -> vec3<f32> {
    let v = u32(packed + 0.5);
    return vec3(f32((v >> 16u) & 255u), f32((v >> 8u) & 255u), f32(v & 255u)) / 255.0;
}

// +-4% value jitter hashed from the voxel cell this fragment belongs to (0.5 wu voxels).
fn voxel_jitter(world: vec3<f32>, n: vec3<f32>) -> f32 {
    let cell = floor((world - n * 0.125) * 2.0);
    let h = fract(sin(dot(cell, vec3(12.9898, 78.233, 37.719))) * 43758.5453);
    return 1.0 + (h * 2.0 - 1.0) * 0.04;
}

@group(constants::MATERIAL_BIND_GROUP) @binding(0) var<uniform> params: ToonParams;
@group(constants::MATERIAL_BIND_GROUP) @binding(1) var<storage, read> g: ToonGlobals;

fn hash2(p: vec2<f32>) -> f32 {
    return fract(sin(dot(p, vec2(127.1, 311.7))) * 43758.5453);
}

fn value_noise(p: vec2<f32>) -> f32 {
    let i = floor(p);
    let f = fract(p);
    let u = f * f * (3.0 - 2.0 * f);
    let a = hash2(i);
    let b = hash2(i + vec2(1.0, 0.0));
    let c = hash2(i + vec2(0.0, 1.0));
    let d = hash2(i + vec2(1.0, 1.0));
    return mix(mix(a, b, u.x), mix(c, d, u.x), u.y);
}

// 1.0 = sunlit, 0.55 = under a cloud.
fn cloud_factor(xz: vec2<f32>) -> f32 {
    let cover = g.cloud.z;
    if cover <= 0.001 {
        return 1.0;
    }
    let p = (xz + g.cloud.xy) / 18.0;
    let n = value_noise(p) * 0.65 + value_noise(p * 2.03 + vec2(5.2, 1.3)) * 0.35;
    // noise is roughly bell-shaped around 0.5; map cover 0..1 to a threshold.
    let threshold = mix(0.2, 0.8, cover);
    return select(1.0, 0.55, n < threshold);
}

fn hard_shadow(world_pos: vec4<f32>, normal: vec3<f32>, frag_xy: vec2<f32>) -> f32 {
    if lights.n_directional_lights == 0u {
        return 1.0;
    }
    if (lights.directional_lights[0].flags & DIRECTIONAL_LIGHT_FLAGS_SHADOWS_ENABLED_BIT) == 0u {
        return 1.0;
    }
    let view_z = dot(vec4(view.view_from_world[0].z, view.view_from_world[1].z,
        view.view_from_world[2].z, view.view_from_world[3].z), world_pos);
    let s = fetch_directional_shadow(0u, world_pos, normal, view_z, frag_xy);
    return select(0.0, 1.0, s > 0.5);
}

fn safe_normal(v: vec3<f32>) -> vec3<f32> {
    if dot(v, v) < 1e-12 {
        return vec3(0.0, 1.0, 0.0);
    }
    return normalize(v);
}

@fragment
fn fragment(in: VertexOutput, @builtin(front_facing) is_front: bool) -> @location(0) vec4<f32> {
    var n = safe_normal(in.world_normal);
    if !is_front {
        n = -n;
    }
    let world = in.world_position.xyz;
    let night = g.sun_dir_night.w;
    let sun_dir = g.sun_dir_night.xyz;
    let ambient = g.ambient_wet.xyz;

    var albedo = params.base_color.rgb;
    var emissive = params.emissive.rgb;
    var e_day = params.emissive_day;
    var e_night = params.emissive_night;
    var ao = 1.0;
    var water = params.mode == 1u;

@if(VERTEX_COLORS) {
    albedo = in.color.rgb * voxel_jitter(world, n);
    ao = in.color.a;
}
@if(VERTEX_UVS_A) {
    e_day = in.uv.x;
    e_night = in.uv.y;
}
@if(VERTEX_UVS_B) {
    emissive = to_linear(unpack_srgb(in.uv_b.x));
    water = water || in.uv_b.y > 0.5;
}

    let sun = g.sun_color_shadow.xyz * cloud_factor(world.xz);
    let shadow = hard_shadow(in.world_position, n, in.position.xy);
    let ndl = max(dot(n, sun_dir), 0.0);
    // Night floor: a kind blue lift so night never goes green-black (#2c3463).
    let night_floor = vec3(0.025, 0.034, 0.125) * 0.9 * night;

    var lit: vec3<f32>;
    if water {
        // Flat water: no bands; sky mixed in 20%, a teal share always kept.
        let base = albedo * ambient * 1.6 + albedo * sun * 0.3 * mix(0.45, 1.0, shadow);
        lit = mix(mix(base, g.sky_height_fog.xyz, 0.2), albedo * 0.7, 0.25) + night_floor;
    } else {
        // AO darkens albedo only; it never shifts the band.
        let a = albedo * mix(1.0, ao, 0.35);
        // Rain darkens upward faces a little.
        let wet = 1.0 - 0.22 * g.ambient_wet.w * clamp(n.y, 0.0, 1.0);
        let al = a * wet;
        let light = ndl * shadow;
        let mid = al * 0.82 * sun;
        // Vertical faces keep more warmth in shadow (tint at 60%) so concrete reads warm.
        let vertical = 1.0 - abs(n.y);
        let tint_amount = g.sun_color_shadow.w * mix(1.0, 0.6, vertical);
        let shade = mix(mid, al * g.tint_fog_density.xyz, tint_amount);
        if light < 0.15 {
            lit = shade;
        } else if light < 0.55 {
            lit = mid;
        } else {
            lit = al * sun * select(1.0, 1.06, n.y > 0.5);
        }
        // Ambient, with a little extra lift on verticals.
        lit += al * ambient * (1.0 + 0.35 * vertical);
        // Night lamps: radius 3 wu, warm #ffd28a, banded like the sun with 2 bands.
        if night > 0.0 {
            let lamp_col = to_linear(vec3(1.0, 0.8235, 0.5412)) * night * 1.3;
            let count = min(u32(g.misc.x), 32u);
            for (var i = 0u; i < count; i++) {
                let to = g.lamps[i].xyz - world;
                let dist = length(to);
                if dist < 3.0 && dist > 1e-4 {
                    let k = (1.0 - dist / 3.0) * max(dot(n, to / dist), 0.0);
                    lit += al * lamp_col * select(select(0.0, 0.5, k > 0.04), 1.0, k > 0.3);
                }
            }
        }
        lit = lit * g.misc.y + night_floor;
    }

    // Fog (distance from the fog start plane in view depth, plus height fog).
    let view_depth = -dot(vec4(view.view_from_world[0].z, view.view_from_world[1].z,
        view.view_from_world[2].z, view.view_from_world[3].z), in.world_position);
    let d = max(view_depth - g.fog_color_start.w, 0.0);
    let h = max(6.0 - world.y, 0.0);
    var fog = 1.0 - exp(-(g.tint_fog_density.w * d + g.sky_height_fog.w * h));
    if water {
        fog *= 0.6;
    }

    let strength = mix(e_day, e_night, night);
    let glow = emissive * strength;
    var color = mix(lit, g.fog_color_start.xyz, fog) + glow * (1.0 - 0.5 * fog);
    // Non-emissive surfaces stay <= 1 so only emissive pixels feed bloom.
    if strength <= 0.0 {
        color = min(color, vec3(0.98));
    }
    return vec4(color, 1.0);
}
