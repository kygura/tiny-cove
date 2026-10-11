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

// Mirrors `ToonParams` in toon.rs (field order matters).
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
    if params.cloud_cover <= 0.001 {
        return 1.0;
    }
    let p = (xz + params.cloud_offset) / 18.0;
    let n = value_noise(p) * 0.65 + value_noise(p * 2.03 + vec2(5.2, 1.3)) * 0.35;
    // noise is roughly bell-shaped around 0.5; map cover 0..1 to a threshold.
    let threshold = mix(0.2, 0.8, params.cloud_cover);
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

@fragment
fn fragment(in: VertexOutput, @builtin(front_facing) is_front: bool) -> @location(0) vec4<f32> {
    var n = normalize(in.world_normal);
    if !is_front {
        n = -n;
    }
    let world = in.world_position.xyz;

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

    let sun = params.sun_color * cloud_factor(world.xz);
    let shadow = hard_shadow(in.world_position, n, in.position.xy);
    let ndl = max(dot(n, params.sun_dir), 0.0);

    var lit: vec3<f32>;
    if water {
        // Flat water: no bands; sky mixed in 20%.
        let base = albedo * params.ambient * 1.6 + albedo * sun * 0.3 * mix(0.45, 1.0, shadow);
        lit = mix(base, params.sky_color, 0.2);
    } else {
        // AO darkens albedo only; it never shifts the band.
        let a = albedo * mix(1.0, ao, 0.35);
        // Rain darkens upward faces a little.
        let wet = 1.0 - 0.22 * params.wetness * clamp(n.y, 0.0, 1.0);
        let al = a * wet;
        let light = ndl * shadow;
        let mid = al * 0.82 * sun;
        let shade = mix(mid, al * params.shadow_tint, params.shadow_strength);
        if light < 0.15 {
            lit = shade;
        } else if light < 0.55 {
            lit = mid;
        } else {
            lit = al * sun * select(1.0, 1.06, n.y > 0.5);
        }
        lit += al * params.ambient;
    }

    // Fog (distance from the fog start plane in view depth, plus height fog).
    let view_depth = -dot(vec4(view.view_from_world[0].z, view.view_from_world[1].z,
        view.view_from_world[2].z, view.view_from_world[3].z), in.world_position);
    let d = max(view_depth - params.fog_start, 0.0);
    let h = max(6.0 - world.y, 0.0);
    let fog = 1.0 - exp(-(params.fog_density * d + params.height_fog * h * 3.0));

    let strength = mix(e_day, e_night, params.night);
    let glow = emissive * strength;
    var color = mix(lit, params.fog_color, fog) + glow * (1.0 - 0.5 * fog);
    // Non-emissive surfaces stay <= 1 so only emissive pixels feed bloom.
    if strength <= 0.0 {
        color = min(color, vec3(0.98));
    }
    return vec4(color, 1.0);
}
