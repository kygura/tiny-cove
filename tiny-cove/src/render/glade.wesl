// TINY COVE main pass: PS1 rasteriser (snapped vertices, affine UVs, nearest atlas) with
// "Folk Pigment" cel lighting (DESIGN.md A3): three hard bands from N.L x the sun shadow map,
// band maths in sRGB with hue-shifted, saturated shadows (never grey), an anti-mud floor, rim
// light, emissive windows / glow / fire at night, a wet-sand honey band above the current
// tide, snow ramp on up-facing surfaces, wind sway on leaves and fibres, pennant flutter
// (vertex colour alpha = 1 - flutter weight), height sea fog mixed without greying, and the
// open sea cut at the horizon so the post pass shows the sky.
// 15-bit quantise + Bayer dither happen in the post pass (post.wesl).
//
// Entry points: vertex/fragment (GladeMaterial), water_fragment (WaterMaterial),
// sea_vertex/sea_fragment (SeaMaterial), particle_vertex/particle_fragment (ParticleMaterial).

import bevy_pbr::render::{
    mesh_functions::{get_world_from_local, mesh_position_local_to_world, mesh_normal_local_to_world},
    mesh_view_bindings::{view, lights, clustered_lights},
    view_transformations::position_world_to_clip,
    shadows::fetch_directional_shadow,
    mesh_view_types::DIRECTIONAL_LIGHT_FLAGS_SHADOWS_ENABLED_BIT,
    clustered_forward::{view_fragment_cluster_index, unpack_clusterable_object_index_ranges, get_clusterable_object_id},
};

// Mirrors `globals_from_env` in material.rs. Rig colours (sun, fill, tint, rim) are sRGB
// multipliers; every other colour is linear.
struct Globals {
    sun_dir_night: vec4<f32>,   // 0  xyz towards sun/moon, w emissive strength (night)
    sun_wet: vec4<f32>,         // 1  sun sRGB x intensity, w wetness
    fill_snow: vec4<f32>,       // 2  fill sRGB x intensity, w snow
    tint_flash: vec4<f32>,      // 3  shadow tint sRGB, w lightning
    rim_fogd: vec4<f32>,        // 4  rim sRGB, w fog density
    fog_start: vec4<f32>,       // 5  fog colour, w fog start (view depth)
    sky_top_time: vec4<f32>,    // 6  sky top, w time (s)
    sky_hor_snap: vec4<f32>,    // 7  sky horizon, w snap fraction of the low-res grid
    wind_tide: vec4<f32>,       // 8  xy wind (m/s), z tide level, w lit-band rotation (deg)
    sea0: vec4<f32>,            // 9  shallow, w storm
    sea1: vec4<f32>,            // 10 mid, w sea-fog bank
    sea2: vec4<f32>,            // 11 deep, w fog bank top (m)
    sea3: vec4<f32>,            // 12 abyss, w tide falling
    sea4: vec4<f32>,            // 13 foam, w sky darkness
    sea5: vec4<f32>,            // 14 glint, w wind strength
    outline: vec4<f32>,         // 15 outline
    field: vec4<f32>,           // 16 heightfield min x, min z, spacing, samples per side
    amb: vec4<f32>,             // 17 ambient sky (particles)
    n_lights: vec4<f32>,        // 18 x = number of lit lanterns / fires in `lights`
    lights: array<vec4<f32>>,   // 19.. xyz position of each lit lantern / fire
}

struct WaterParams {
    deep: vec4<f32>,
    shallow: vec4<f32>,
}

@group(constants::MATERIAL_BIND_GROUP) @binding(0) var<storage, read> g: Globals;
@group(constants::MATERIAL_BIND_GROUP) @binding(1) var atlas: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(2) var<uniform> water: WaterParams;
@group(constants::MATERIAL_BIND_GROUP) @binding(3) var heightfield: texture_2d<f32>;

// Surface ids (cairn_cove_core::Surface).
const S_GRASS: u32 = 0u;
const S_DIRT: u32 = 1u;
const S_ROCK: u32 = 2u;
const S_SAND: u32 = 3u;
const S_DRYSTONE: u32 = 4u;
const S_SHELL: u32 = 12u;
const S_PEBBLE: u32 = 13u;
const S_BOARDS: u32 = 15u;
const S_WINDOW: u32 = 16u;
const S_DOOR: u32 = 17u;
const S_LEAVES: u32 = 19u;
const S_FIBER: u32 = 20u;
const S_GLOW: u32 = 23u;
const S_WATER: u32 = 24u;
const TILE: u32 = 32u;
const COLS: u32 = 8u;
const SEA_LEVEL: f32 = 0.0;
const TIDE_RANGE: f32 = 0.3;

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

// ---------------------------------------------------------------- colour helpers

fn to_srgb(c: vec3<f32>) -> vec3<f32> {
    let x = max(c, vec3(0.0));
    return select(1.055 * pow(x, vec3(1.0 / 2.4)) - 0.055, x * 12.92, x <= vec3(0.0031308));
}

fn to_linear(c: vec3<f32>) -> vec3<f32> {
    let x = max(c, vec3(0.0));
    return select(pow((x + 0.055) / 1.055, vec3(2.4)), x / 12.92, x <= vec3(0.04045));
}

fn modp(x: f32, m: f32) -> f32 {
    return x - m * floor(x / m);
}

// sRGB -> HSV (hue in degrees).
fn rgb2hsv(c: vec3<f32>) -> vec3<f32> {
    let mx = max(c.r, max(c.g, c.b));
    let mn = min(c.r, min(c.g, c.b));
    let d = mx - mn;
    var h = 0.0;
    if d > 1e-6 {
        if mx == c.r {
            h = 60.0 * modp((c.g - c.b) / d, 6.0);
        } else if mx == c.g {
            h = 60.0 * ((c.b - c.r) / d + 2.0);
        } else {
            h = 60.0 * ((c.r - c.g) / d + 4.0);
        }
    }
    return vec3(h, select(0.0, d / mx, mx > 1e-6), mx);
}

fn hsv2rgb(c: vec3<f32>) -> vec3<f32> {
    let h = modp(c.x, 360.0) / 60.0;
    let s = clamp(c.y, 0.0, 1.0);
    let k = (vec3(5.0, 3.0, 1.0) + h) - 6.0 * floor((vec3(5.0, 3.0, 1.0) + h) / 6.0);
    return c.z - c.z * s * max(min(min(k, 4.0 - k), vec3(1.0)), vec3(0.0));
}

// DESIGN A3 direction rule: positive when 60 <= h < t, else the warm (negative) path.
fn rotate_toward(h: f32, t: f32, amount: f32) -> f32 {
    if h >= 60.0 && h < t {
        return h + min(amount, t - h);
    }
    let dist = modp(h - t, 360.0);
    return modp(h - min(amount, dist), 360.0);
}

// DESIGN A3 shade(c, band, tint): band 1 = mid, 2 = shadow. sRGB in, sRGB out.
// CPU port + calibration goldens: cairn_cove_core::palette::shade.
fn shade_band(c: vec3<f32>, band: u32, tint: vec3<f32>) -> vec3<f32> {
    let shadow = band == 2u;
    let t = rgb2hsv(tint).x;
    var a = rgb2hsv(c);
    let white = a.y < 0.15;
    let floor_white = select(0.25, 0.30, shadow);
    if white {
        a.x = t;
        a.y = max(a.y, floor_white);
    } else {
        let warm = !(a.x >= 60.0 && a.x < t);
        let amt = select(select(20.0, 14.0, warm), select(36.0, 25.0, warm), shadow);
        a.x = rotate_toward(a.x, t, amt);
    }
    a.y = min(a.y * select(1.0, 1.2, shadow), 1.0);
    a.z = a.z * select(0.90, 0.78, shadow);
    let mixed = mix(hsv2rgb(a), tint, select(0.08, 0.15, shadow));
    var m = rgb2hsv(mixed);
    let f = select(select(0.0, 0.40, shadow), floor_white, white);
    if m.y < f {
        m.y = f;
        return hsv2rgb(m);
    }
    return mixed;
}

// Lit-band pre-rotation toward the tint (dusk 10 deg, night 30 deg).
fn rot_lit(c: vec3<f32>, tint: vec3<f32>, deg: f32) -> vec3<f32> {
    if deg <= 0.0 {
        return c;
    }
    var a = rgb2hsv(c);
    if a.y < 0.15 {
        return c;
    }
    a.x = rotate_toward(a.x, rgb2hsv(tint).x, deg);
    return hsv2rgb(a);
}

fn tint_dist(h: f32, t: f32) -> f32 {
    if h >= 60.0 && h < t {
        return t - h;
    }
    return modp(h - t, 360.0);
}

// DESIGN A3 anti-mud floor: low-chroma darks pulled toward the tint hue; dark ochres and
// olives (hue 30-100, V < 0.45: a warm albedo under a cool storm / night light) turned toward
// it too; V >= 0.22. CPU port: cairn_cove_core::palette::anti_mud.
fn anti_mud(c: vec3<f32>, tint: vec3<f32>) -> vec3<f32> {
    var a = rgb2hsv(c);
    let t = rgb2hsv(tint).x;
    if a.z < 0.80 && a.y < 0.35 {
        let k = (0.35 - a.y) / 0.35;
        a.x = rotate_toward(a.x, t, tint_dist(a.x, t) * k);
        a.y = 0.35;
    }
    if a.z < 0.45 && a.x >= 30.0 && a.x < 100.0 {
        let k = 0.25 + 0.5 * (0.45 - a.z) / 0.45;
        a.x = rotate_toward(a.x, t, tint_dist(a.x, t) * k);
        a.y = max(a.y, 0.4);
    }
    a.z = max(a.z, 0.22);
    return hsv2rgb(a);
}

// Fog mix that never greys (DESIGN A0.3, A0.5, A4 sea fog): a linear mix, but where it
// would drop below the blend of the two saturations (capped at A4's S 0.30 floor, so it only
// steps in against grey), saturation is held there and the hue walks from the
// object's hue toward the fog's along the A3 direction rule (analogous path, never through
// the complement). A pale fog (pearl sea fog, snow) barely turns the hue, so things keep
// their identity behind the veil; a saturated fog (storm, dusk, night) pulls them all the
// way. Linear in, linear out.
fn fog_mix(c: vec3<f32>, fog: vec3<f32>, k: f32) -> vec3<f32> {
    let m = mix(c, fog, k);
    if k <= 0.001 {
        return m;
    }
    let a = rgb2hsv(to_srgb(c));
    let f = rgb2hsv(to_srgb(fog));
    var o = rgb2hsv(to_srgb(m));
    let s_min = min(mix(a.y, f.y, k), 0.3);
    if o.y >= s_min {
        return m;
    }
    // The mix's own hue is kept while it still carries one (analogous inputs); only a
    // near-grey mix (complementary inputs) takes the walked hue.
    var h = select(f.x, o.x, o.y >= 0.5 * s_min);
    if a.y > 0.08 && o.y < 0.5 * s_min {
        var dist = modp(a.x - f.x, 360.0);
        if a.x >= 60.0 && a.x < f.x {
            dist = f.x - a.x;
        }
        h = rotate_toward(a.x, f.x, dist * k * smoothstep(0.1, 0.5, f.y));
    }
    o.x = h;
    o.y = s_min;
    return to_linear(hsv2rgb(o));
}

// Lantern / fire pool: the warm light adds to the albedo (2 hard bands from
// point_lights), but a pool that lands in the yellow-green zone below V 0.8 (olive / khaki,
// DESIGN A0.3 "no mud", A3 "grass never goes olive") is pushed to a bright amber-gold
// instead. `c` sRGB, `albedo` and `pl` linear; returns sRGB.
fn lamp_pool(c: vec3<f32>, albedo: vec3<f32>, pl: vec3<f32>) -> vec3<f32> {
    if max(pl.r, max(pl.g, pl.b)) <= 0.01 {
        return c;
    }
    var o = rgb2hsv(to_srgb(to_linear(c) + albedo * pl));
    if o.x > 50.0 && o.x < 105.0 && o.z < 0.8 {
        o.x = 45.0 + (o.x - 50.0) * 0.2;
        o.y = max(o.y, 0.6);
        o.z = max(o.z, 0.7);
    }
    return hsv2rgb(o);
}

fn hex_lin(r: f32, gg: f32, b: f32) -> vec3<f32> {
    return to_linear(vec3(r, gg, b) / 255.0);
}

// ---------------------------------------------------------------- vertex

// Decals (paths on terrain, windows/doors on walls) win against coplanar terrain despite
// vertex snapping: pull them 0.12 m towards the camera (orthographic: ndc z is linear in depth).
fn decal_bias(clip: vec4<f32>, surface: u32) -> vec4<f32> {
    if surface == 1u || (surface >= S_SHELL && surface <= S_DOOR) {
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

// Wind sway for leaves and fibres, plus pennant flutter: vertex colour alpha = 1 - flutter
// weight, so any vertex with alpha < 0.999 streams with the wind vector, scaled by its
// weight and a travelling gust. Vertices at the same position move together, so flat-shaded
// meshes never tear. Mirrored in glade_prepass.wesl.
fn flutter(world: vec3<f32>, alpha: f32) -> vec3<f32> {
    if alpha >= 0.999 {
        return world;
    }
    let w = g.wind_tide.xy;
    let strength = length(w);
    let dir = select(vec2(1.0, 0.0), w / max(strength, 1e-4), strength > 1e-3);
    let t = g.sky_top_time.w;
    let along = dot(world.xz, dir);
    let gust = 0.11 + 0.05 * sin(t * (5.0 + 0.4 * min(strength, 10.0)) - along * 3.1) + 0.03 * sin(t * 1.3 + along);
    return world + vec3(w.x, 0.0, w.y) * (1.0 - clamp(alpha, 0.0, 1.0)) * gust;
}

fn sway(world: vec3<f32>, s: u32) -> vec3<f32> {
    if s != S_LEAVES && s != S_FIBER {
        return world;
    }
    let w = g.wind_tide.xy;
    let strength = length(w);
    let t = g.sky_top_time.w;
    let q = floor(world * 4.0 + 0.5);
    let ph = fract(sin(dot(q, vec3(12.9898, 78.233, 37.719))) * 43758.5453) * 6.2831;
    let dir = select(vec2(1.0, 0.0), w / max(strength, 1e-4), strength > 1e-3);
    let k = 0.03 + 0.018 * min(strength, 10.0);
    let gust = sin(t * (1.7 + 0.12 * strength) + ph) * 0.6 + sin(t * 3.3 + ph * 2.0) * 0.4;
    let lean = 0.5 * min(strength / 6.0, 1.0);
    return world + vec3(dir.x, 0.0, dir.y) * k * (gust + lean) + vec3(0.0, k * 0.35 * gust, 0.0);
}

@vertex
fn vertex(v: Vertex) -> VertexOut {
    let world0 = mesh_position_local_to_world(get_world_from_local(v.instance_index), vec4(v.position, 1.0));
    var out: VertexOut;
    out.surface = u32(max(v.uv_b.x, 0.0) + 0.5);
    let world = vec4(flutter(sway(world0.xyz, out.surface), v.color.a), 1.0);
    out.clip = decal_bias(snap_clip(position_world_to_clip(world.xyz)), out.surface);
    out.uv = v.uv;
    out.color = v.color;
    out.world_pos = world;
    out.normal = mesh_normal_local_to_world(v.normal, v.instance_index);
    return out;
}

// ---------------------------------------------------------------- shared lighting

fn hash2(p: vec2<f32>) -> f32 {
    return fract(sin(dot(p, vec2(127.1, 311.7))) * 43758.5453);
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

// Sea-fog bank layers (fog amount) below 1/3, 2/3 and 3/3 of the bank top; mirrored in
// post.wesl. Thick enough to swallow the shore in a pearl wash, thin enough that the jetty,
// tracks, dykes and beach still read through it (SPEC Done #3).
const BANK_LOW: f32 = 0.6;
const BANK_MID: f32 = 0.48;
const BANK_TOP: f32 = 0.36;

// Distance fog plus the sea-fog bank: dense below `fog_top` with a hard, banded top edge.
fn fog_amount(depth: f32, y: f32) -> f32 {
    let d = max(depth - g.fog_start.w, 0.0);
    let dist = 1.0 - exp(-g.rim_fogd.w * d);
    let bank = g.sea1.w;
    let top = g.sea2.w;
    var b = 0.0;
    if y < top * 0.34 {
        b = BANK_LOW;
    } else if y < top * 0.67 {
        b = BANK_MID;
    } else if y < top {
        b = BANK_TOP;
    }
    return 1.0 - (1.0 - dist) * (1.0 - b * bank);
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

// Toward the (orthographic) camera.
fn to_camera() -> vec3<f32> {
    return normalize(view.world_from_view[2].xyz);
}

// Clustered point lights (lanterns, fires) in 2 hard bands. Colour scale: a PointLight of
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
            let band = select(select(0.0, 0.45, k > 0.3), 1.0, k > 0.6);
            acc += light.color_inverse_square_range.xyz * (1.0 / 100.0) * band;
        }
    }
    return acc;
}

// Sea fog: each lit lantern / fire makes a #FF9C42 Bayer halo disc (radius 2 m, 30 %) that
// shows through the fog (added after the fog mix).
fn fog_halo(world: vec4<f32>, frag: vec4<f32>) -> vec3<f32> {
    let bank = g.sea1.w;
    if bank < 0.05 || world.y > g.sea2.w {
        return vec3(0.0);
    }
    let view_z = -view_depth(world);
    let is_ortho = view.clip_from_view[3].w == 1.0;
    let ranges = unpack_clusterable_object_index_ranges(view_fragment_cluster_index(frag.xy, view_z, is_ortho));
    for (var i = ranges.first_point_light_index_offset; i < ranges.first_spot_light_index_offset; i++) {
        let light = clustered_lights.data[get_clusterable_object_id(i)];
        if light.color_inverse_square_range.x > 0.0 && length(light.position_radius.xyz - world.xyz) < 2.0 {
            if bayer4(vec2<u32>(frag.xy)) < 0.3 * bank {
                return hex_lin(255.0, 156.0, 66.0);
            }
            return vec3(0.0);
        }
    }
    return vec3(0.0);
}

// Ground-like surfaces that take the wet-sand honey band and puddles.
fn is_shore(s: u32) -> bool {
    return s == S_SAND || s == S_DIRT || s == S_GRASS || s == S_PEBBLE || s == S_SHELL;
}

fn bayer4(p: vec2<u32>) -> f32 {
    var m = array<f32, 16>(0.0, 8.0, 2.0, 10.0, 12.0, 4.0, 14.0, 6.0, 3.0, 11.0, 1.0, 9.0, 15.0, 7.0, 13.0, 5.0);
    return (m[(p.y % 4u) * 4u + p.x % 4u] + 0.5) / 16.0;
}

// Band light multipliers (sRGB): lit, mid, shadow. When weather dims the sun below the
// fill (rain, storm), the lit band never gets darker than the shadow band.
fn band_lights() -> array<vec3<f32>, 3> {
    let sun = g.sun_wet.xyz;
    let fill = g.fill_snow.xyz;
    return array<vec3<f32>, 3>(max(sun, fill), max(mix(sun, fill, 0.5), fill), fill);
}

// The three cel bands for sRGB albedo `a` (DESIGN A3), then the anti-mud floor.
fn cel(a: vec3<f32>, light: f32) -> vec3<f32> {
    let tint = g.tint_flash.xyz;
    let k = band_lights();
    var c: vec3<f32>;
    if light >= 0.30 {
        c = rot_lit(a, tint, g.wind_tide.w) * k[0];
    } else if light >= 0.08 {
        c = shade_band(a, 1u, tint) * k[1];
    } else {
        c = shade_band(a, 2u, tint) * k[2];
    }
    return anti_mud(min(c, vec3(1.0)), tint);
}

// ---------------------------------------------------------------- opaque fragment

@fragment
fn fragment(in: VertexOut, @builtin(front_facing) is_front: bool) -> @location(0) vec4<f32> {
    var n = normalize(in.normal);
    if !is_front {
        n = -n;
    }
    let s = in.surface;
    let world = in.world_pos;
    let night = g.sun_dir_night.w;
    let wet = g.sun_wet.w;
    let snow = g.fill_snow.w;
    let sun_dir = normalize(g.sun_dir_night.xyz);
    let tint = g.tint_flash.xyz;
    let px = vec2<u32>(in.clip.xy);

    // Atlas classes (texel alpha): 0 cutout, 0.5 tinted, 0.75 accent, 0.85 hot accent,
    // 1.0 emissive. Accent texels with rgb 0 are the trim sentinel.
    let tex = atlas_texel(s, in.uv);
    if tex.a < 0.25 {
        discard;
    }
    // Sea bed past the horizon would show once the sea is cut there: the sky owns it.
    if world.y < g.wind_tide.z - 0.05 && past_horizon(world.xz) {
        discard;
    }
    let accent = tex.a > 0.6 && tex.a < 0.95;
    let hot = tex.a > 0.8 && tex.a < 0.95;
    let emissive_texel = tex.a >= 0.95;
    var albedo = tex.rgb * in.color.rgb;
    if accent {
        albedo = tex.rgb;
        // Trim sentinel: a deeper cut of the vertex tint. (Vertex alpha is the pennant
        // flutter weight, not a trim index.)
        if max(tex.r, max(tex.g, tex.b)) < 0.002 {
            albedo = in.color.rgb * 0.82;
        }
        // The basket rim is plain iron by day and glows iron_hot while fires burn.
        if hot {
            albedo = mix(in.color.rgb, tex.rgb, clamp(g.sun_dir_night.w, 0.0, 1.0));
        }
    }
    // Window slot: outline ink by day, emissive window light at night.
    let slot = s == S_WINDOW && emissive_texel;
    if slot {
        albedo = hex_lin(49.0, 33.0, 123.0);
    }
    if s == S_GLOW {
        // Flames and lantern glass: always emissive, untinted, hard bands.
        let fog = fog_amount(view_depth(world), world.y);
        let c = select(tex.rgb * in.color.rgb, tex.rgb, emissive_texel);
        return vec4(fog_mix(c * (1.0 + 0.4 * g.tint_flash.w), g.fog_start.xyz, fog * 0.5), 1.0);
    }

    var a = to_srgb(albedo);

    // Wet-sand honey band just above the current tide line, with the high-tide wrack line.
    let tide = g.wind_tide.z;
    var shore_wet = false;
    if is_shore(s) && n.y > 0.5 {
        if world.y < tide + 0.35 && world.y > tide - 0.6 {
            a = vec3(222.0, 165.0, 74.0) / 255.0;
            shore_wet = true;
        }
        let mark = SEA_LEVEL + TIDE_RANGE + 0.05;
        let fy = max(fwidth(world.y), 0.004);
        if abs(world.y - mark) < fy * 0.6 && (px.x + px.y) % 2u == 0u {
            a = select(vec3(206.0, 123.0, 49.0), vec3(206.0, 156.0, 16.0), px.x % 4u == 0u) / 255.0;
        }
    } else if (s == S_ROCK || s == S_DRYSTONE) && world.y < tide + 0.35 && world.y > tide - 0.6 {
        var w = rgb2hsv(a);
        w.y = min(w.y * 1.15, 1.0);
        w.z *= 0.82;
        a = hsv2rgb(w);
    }

    // Rain: wet things get richer, not duller (V x 0.82, S x 1.15).
    if wet > 0.0 && !slot {
        var w = rgb2hsv(a);
        w.y = min(w.y * (1.0 + 0.15 * wet), 1.0);
        w.z *= 1.0 - 0.18 * wet;
        a = hsv2rgb(w);
    }

    // Snow on up-facing surfaces with a hard noisy edge; vertical faces and trim keep chroma.
    var snowed = false;
    if snow > 0.0 && !slot && s != S_WATER && n.y > 0.6 {
        let cell = floor(world.xz * 3.0);
        let noise = hash2(cell) * 0.3 + hash2(floor(world.xz * 0.7) + 17.0) * 0.25;
        if (n.y - 0.6) * 2.5 * snow * 1.3 - noise > 0.1 {
            snowed = true;
        }
    }

    let shadow = sun_shadow(world, n, in.clip.xy);
    let light = max(dot(n, sun_dir), 0.0) * shadow;
    var c: vec3<f32>;
    if snowed {
        // Fixed snow ramp; the lit band keeps the sun's hue but not its dimming (snow glows).
        let k = band_lights();
        if light >= 0.30 {
            let hue = k[0] / max(max(k[0].r, max(k[0].g, k[0].b)), 0.5);
            c = vec3(247.0, 255.0, 255.0) / 255.0 * mix(hue, vec3(1.0), 0.5);
        } else if light >= 0.08 {
            c = vec3(156.0, 173.0, 255.0) / 255.0 * k[1];
        } else {
            c = vec3(123.0, 140.0, 255.0) / 255.0 * k[2];
        }
        c = anti_mud(min(c, vec3(1.0)), tint);
    } else {
        c = cel(a, light);
    }

    // Glistening wet sand while the tide goes out; puddle glints in the rain.
    if shore_wet && light >= 0.30 && g.sea3.w > 0.5 && hash2(floor(world.xz * 8.0)) > 0.975 {
        c = to_srgb(g.sea5.xyz);
    }
    if wet > 0.5 && is_shore(s) && n.y > 0.8 && light >= 0.30 && hash2(floor(world.xz * 6.0) + floor(g.sky_top_time.w * 2.0)) > 0.985 {
        c = to_srgb(g.sea5.xyz);
    }

    // Rim: grazing faces on the back-lit side catch a 1-px-ish rim of the hour's rim colour.
    let rim_k = 1.0 - max(dot(n, to_camera()), 0.0);
    if rim_k > 0.62 && dot(n.xz, sun_dir.xz) < 0.0 && n.y < 0.7 {
        c = min(c + g.rim_fogd.xyz * 0.6, vec3(1.0));
    }

    // Lanterns and fires (warm pools, never olive).
    c = lamp_pool(c, albedo, point_lights(world, n, in.clip));
    var col = to_linear(c);

    // Emissive: window slots and hot iron glow at night.
    var emissive = vec3(0.0);
    if slot {
        let lit = clamp(night * 1.2, 0.0, 1.0);
        emissive = tex.rgb * lit;
        col = mix(col, vec3(0.0), lit);
    } else if hot {
        emissive = albedo * clamp(night, 0.0, 1.0) * 0.8;
    }

    let fog = fog_amount(view_depth(world), world.y);
    var out = fog_mix(col, g.fog_start.xyz, fog) + emissive * (1.0 - 0.5 * fog);
    let halo = fog_halo(world, in.clip);
    if any(halo > vec3(0.0)) {
        out = halo;
    }
    return vec4(out, 1.0);
}

// ---------------------------------------------------------------- sea-bed + waves

// Sea-bed height (bilinear in the heightfield). Beyond the field the bed keeps falling
// toward the abyss so the far sea reads cobalt.
fn seabed(p: vec2<f32>) -> f32 {
    let f = g.field;
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
    let w = g.wind_tide.xy;
    let s = length(w);
    return select(vec2(0.96, 0.29), w / max(s, 1e-4), s > 1e-3);
}

// Wave height in about -1..1 (mirrored in glade_prepass.wesl).
fn wave(p: vec2<f32>, t: f32) -> f32 {
    let dir = wind_dir();
    let perp = vec2(-dir.y, dir.x);
    let a = sin(dot(p, dir) * 1.1 - t * 1.4);
    let b = sin(dot(p, normalize(dir + perp * 0.6)) * 1.9 - t * 2.1 + 1.3);
    let c = sin(dot(p, perp) * 0.7 + t * 0.8);
    let swell = sin(dot(p, dir) * 0.32 - t * 0.9) * g.sea0.w;
    return a * 0.5 + b * 0.3 + c * 0.2 + swell * 0.8;
}

fn wave_amp() -> f32 {
    let s = g.sea5.w;
    return (0.05 + 0.012 * min(s, 12.0)) * (1.0 + 1.5 * g.sea0.w);
}

fn sea_height(p: vec2<f32>) -> f32 {
    let depth = g.wind_tide.z - seabed(p);
    let damp = clamp(depth / 1.5, 0.15, 1.0);
    return g.wind_tide.z + wave(p, g.sky_top_time.w) * wave_amp() * damp;
}

@vertex
fn sea_vertex(v: Vertex) -> VertexOut {
    let w0 = mesh_position_local_to_world(get_world_from_local(v.instance_index), vec4(v.position, 1.0));
    let world = vec4(w0.x, sea_height(w0.xz), w0.z, 1.0);
    var out: VertexOut;
    out.surface = S_WATER;
    out.clip = snap_clip(position_world_to_clip(world.xyz));
    out.uv = v.uv;
    out.color = v.color;
    out.world_pos = world;
    out.normal = vec3(0.0, 1.0, 0.0);
    return out;
}

// Hard sea band by depth (A6) with a 0.25 m checker seam at each step.
fn sea_band(depth: f32, px: vec2<u32>) -> vec3<f32> {
    let d = depth + select(-0.125, 0.125, (px.x + px.y) % 2u == 0u);
    if d < 1.5 {
        return g.sea0.xyz;
    } else if d < 4.0 {
        return g.sea1.xyz;
    } else if d < 10.0 {
        return g.sea2.xyz;
    }
    return g.sea3.xyz;
}

// Screen position (low-res px, y down) of a world point.
fn to_screen(p: vec3<f32>) -> vec2<f32> {
    let c = view.clip_from_world * vec4(p, 1.0);
    let ndc = c.xy / c.w;
    return vec2((ndc.x * 0.5 + 0.5) * view.viewport.z, (0.5 - ndc.y * 0.5) * view.viewport.w);
}

// Broken vertical reflection streak (glow_fire dashes, 1 px wide, 2 px gaps) on the water
// under each lit lantern / fire near the sea (DESIGN A6, A8 night). The streak
// runs down the light's screen column from just below the light to past its mirror image in
// the water plane, so it lands on the sea even when the light stands on a cliff or a tower;
// it spreads up to 18 m out over the ripples. Lights come from the globals list (not the
// clusters, which only hold lights in range).
fn light_streaks(world: vec4<f32>, frag: vec4<f32>) -> f32 {
    let n = min(u32(max(g.n_lights.x, 0.0)), arrayLength(&g.lights));
    let tide = g.wind_tide.z;
    for (var i = 0u; i < n; i++) {
        let lp = g.lights[i].xyz;
        if length(lp.xz - world.xz) > 18.0 {
            continue;
        }
        let top = to_screen(lp);
        let mirror = to_screen(vec3(lp.x, 2.0 * tide - lp.y, lp.z));
        let dy = frag.y - top.y;
        // Dashes shimmer with the swell: the pattern slides one step every 0.25 s.
        let phase = u32(dy * 0.5 + floor(g.sky_top_time.w * 4.0) + f32(i) * 3.0);
        if abs(frag.x - top.x) < 0.75 && dy > 2.0 && frag.y < mirror.y + 36.0 && phase % 2u == 0u {
            return 1.0;
        }
    }
    return 0.0;
}

// Open sea beyond the horizon line (outline.w m from the island centre along the camera's
// ground-forward axis) is cut away so the post pass paints the sky there (DESIGN A4, A8).
fn past_horizon(xz: vec2<f32>) -> bool {
    let back = view.world_from_view[2].xyz;
    let l = length(back.xz);
    return l > 1e-3 && dot(xz, -back.xz / l) > g.outline.w;
}

@fragment
fn sea_fragment(in: VertexOut) -> @location(0) vec4<f32> {
    let world = in.world_pos;
    if past_horizon(world.xz) {
        discard;
    }
    let p = world.xz;
    let t = g.sky_top_time.w;
    let px = vec2<u32>(in.clip.xy);
    let tide = g.wind_tide.z;
    let storm = g.sea0.w;
    let depth = tide - seabed(p);
    let wv = wave(p, t);

    var col = sea_band(depth, px);
    // Wave relief: crests one step lighter, troughs one step deeper (hard).
    if depth > 0.6 {
        if wv > 0.62 {
            col = mix(col, g.sea0.xyz, 0.4);
        } else if wv < -0.72 {
            col = mix(col, g.sea3.xyz, 0.3);
        }
    }
    var c = to_srgb(col);

    // Structures' shadows fall on the water in the hue-shifted shadow band.
    let shadow = sun_shadow(world, vec3(0.0, 1.0, 0.0), in.clip.xy);
    if shadow < 0.5 {
        c = anti_mud(shade_band(c, 2u, g.tint_flash.xyz), g.tint_flash.xyz);
    }
    c *= 1.0 + 0.5 * g.tint_flash.w;

    let foam = g.sea4.xyz;
    let foam_shade = foam * hex_lin(140.0, 239.0, 239.0);
    var out = to_linear(c);
    var lit_px = false;

    // Foam band 2: a 1-px line ~0.6 m seaward that breathes with the swell (doubled in
    // storms), with a foam_shade pixel on its seaward side.
    let fw = max(fwidth(depth), 1e-4);
    let dir = wind_dir();
    let k2 = 0.22 * (1.0 + storm) + 0.07 * sin(t * 0.9 + dot(p, dir) * 0.25);
    let wide = 0.5 + 0.5 * storm + 0.5 * g.sun_wet.w;
    if depth > 0.02 && fw < 0.2 {
        let e = depth - k2;
        if abs(e) < fw * wide {
            out = foam;
            lit_px = true;
        } else if e >= fw * wide && e < fw * (wide + 1.0) {
            out = foam_shade;
        }
    }

    // Storm whitecaps on 1 in 6 crests.
    if storm > 0.3 && depth > 1.0 && wv > 0.7 && hash2(floor(p * 0.4)) < 0.17 * storm {
        out = foam;
        lit_px = true;
    }

    // Hard cel glint: single pixels on lit crest facets (about 1 crest in 3).
    let n = normalize(cross(dpdy(world.xyz), dpdx(world.xyz)));
    let nn = select(n, -n, n.y < 0.0);
    let hv = normalize(normalize(g.sun_dir_night.xyz) + to_camera());
    let spec = dot(nn, hv) - hv.y;
    if shadow > 0.5 && wv > 0.35 && spec > 0.006 && hash2(floor(p * 0.7)) < 0.33 && hash2(vec2<f32>(px)) < 0.08 {
        out = g.sea5.xyz;
        lit_px = true;
    }

    // Rain rings: 1-px pale cyan pricks.
    if g.sun_wet.w > 0.3 && hash2(vec2<f32>(px) + floor(t * 4.0) * 7.31) > 0.992 {
        out = hex_lin(206.0, 247.0, 255.0) * 0.8;
    }

    // Lantern / fire light on the water and its broken reflection streak.
    out += col * point_lights(world, vec3(0.0, 1.0, 0.0), in.clip) * 0.6;
    if g.sun_dir_night.w > 0.3 && light_streaks(world, in.clip) > 0.5 {
        out = hex_lin(255.0, 123.0, 25.0);
        lit_px = true;
    }

    // Fog: distance fog plus the low bank (turquoise shows only near the shore).
    var fog = fog_amount(view_depth(world), 0.0);
    if g.sea1.w > 0.05 && depth < 1.5 {
        fog *= 0.55;
    }
    if lit_px {
        fog *= 0.7;
    }
    return vec4(fog_mix(out, g.fog_start.xyz, fog), 1.0);
}

// ---------------------------------------------------------------- lagoons

@fragment
fn water_fragment(in: VertexOut) -> @location(0) vec4<f32> {
    let world = in.world_pos;
    // Water at mean sea level is the open sea: the sea mesh draws it (with the tide).
    if abs(world.y - SEA_LEVEL) < 0.03 {
        discard;
    }
    let p = world.xz;
    let px = vec2<u32>(in.clip.xy);
    let depth = world.y - seabed(p);
    // Same time-of-day tint as the sea's shallow band.
    let noon = hex_lin(25.0, 222.0, 206.0);
    let tod = g.sea0.xyz / max(noon, vec3(1e-3));
    var col = select(water.shallow.rgb, water.deep.rgb, depth + select(-0.06, 0.06, (px.x + px.y) % 2u == 0u) >= 0.6) * tod;
    // Caustics from the Water tile (M0 squiggles over M1).
    let tex = atlas_texel(S_WATER, p * 0.5 + vec2(g.sky_top_time.w * 0.05, 0.0));
    if tex.r > 0.97 {
        col = mix(col, to_linear(vec3(206.0, 255.0, 247.0) / 255.0), 0.45);
    }
    var c = to_srgb(col);
    let shadow = sun_shadow(world, vec3(0.0, 1.0, 0.0), in.clip.xy);
    if shadow < 0.5 {
        c = anti_mud(shade_band(c, 2u, g.tint_flash.xyz), g.tint_flash.xyz);
    }
    var out = to_linear(c);
    // 1-px lagoon rim.
    let fw = max(fwidth(depth), 1e-4);
    if depth < fw * 1.2 && fw < 0.3 {
        out = hex_lin(206.0, 255.0, 247.0);
    }
    out += col * point_lights(world, vec3(0.0, 1.0, 0.0), in.clip) * 0.6;
    let fog = fog_amount(view_depth(world), world.y);
    return vec4(fog_mix(out, g.fog_start.xyz, fog), 1.0);
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
    @location(5) y: f32,
}

const P_FLAKE: u32 = 0u;
const P_STREAK: u32 = 1u;
const P_PUFF: u32 = 2u;
const P_BIRD: u32 = 3u;
const P_SPARK: u32 = 4u;
const P_SNOW: u32 = 5u;

@vertex
fn particle_vertex(v: ParticleVertex) -> ParticleOut {
    let world = mesh_position_local_to_world(get_world_from_local(v.instance_index), vec4(v.position, 1.0));
    let center = view.view_from_world * world;
    let kind = u32(v.size_kind.y + 0.5);
    let size = v.size_kind.x;
    var ax = vec2(1.0, 0.0);
    var ay = vec2(0.0, 1.0);
    var half = vec2(size * 0.5);
    // World size of one low-res pixel.
    let px = 2.0 / (view.clip_from_view[1].y * view.viewport.w);
    if kind == P_STREAK {
        let d = (view.view_from_world * vec4(v.dir, 0.0)).xy;
        if dot(d, d) > 1e-8 {
            ay = normalize(d);
            ax = vec2(ay.y, -ay.x);
        }
        // 1 px wide, 4 px long (DESIGN says 1x3; one more pixel lets the wind slant read).
        half = vec2(px * 0.6, max(size * 0.5, px * 2.0));
    } else if kind == P_SNOW {
        // 1 px wide, 2 px tall: white over a lilac pixel.
        half = vec2(px * 0.55, px * 1.05);
    }
    let offset = ax * v.corner.x * half.x + ay * v.corner.y * half.y;
    // Snap the centre only, so thin streaks keep their shape (corner snapping folds them).
    var out: ParticleOut;
    out.clip = snap_clip(view.clip_from_view * center) + view.clip_from_view * vec4(offset, 0.0, 0.0);
    out.corner = v.corner;
    out.color = v.color;
    out.kind = kind;
    out.depth = -center.z;
    out.seed = fract(dot(v.position, vec3(0.13, 0.71, 0.37)));
    out.y = world.y;
    return out;
}

@fragment
fn particle_fragment(in: ParticleOut) -> @location(0) vec4<f32> {
    let c = in.corner;
    let r = length(c);
    var alpha = in.color.a;
    var col = in.color.rgb;
    // Particles take the fill/sun light loosely (they are not cel-banded).
    let lit = to_linear(min(mix(g.fill_snow.xyz, g.sun_wet.xyz, 0.5), vec3(1.0)));
    switch in.kind {
        case P_FLAKE: {
            if r > 0.9 { discard; }
            col *= lit;
        }
        case P_STREAK: {
            col *= mix(vec3(1.0), lit, 0.4);
        }
        case P_PUFF: {
            if r > 1.0 { discard; }
            alpha *= 1.0 - r * r * 0.8;
            col *= lit * mix(0.85, 1.1, step(0.0, c.y));
            // Smoke and fog puffs are pale and hued, never grey (DESIGN A0.3).
            var ph = rgb2hsv(to_srgb(col));
            if ph.z < 0.85 && ph.y < 0.25 {
                ph.x = select(ph.x, rgb2hsv(g.tint_flash.xyz).x, ph.y < 0.02);
                ph.y = 0.25;
                col = to_linear(hsv2rgb(ph));
            }
        }
        case P_BIRD: {
            // Flapping V silhouette.
            let flap = sin(g.sky_top_time.w * 9.0 + in.seed * 40.0) * 0.45;
            let wing = abs(c.x) * (0.25 + flap) - c.y * 0.6;
            let body = abs(c.x) < 0.12 && abs(c.y) < 0.25;
            if !(abs(wing) < 0.14 || body) || abs(c.x) > 0.95 { discard; }
            col *= mix(lit, vec3(1.0), 0.3);
        }
        case P_SNOW: {
            col = select(hex_lin(156.0, 173.0, 255.0), col, c.y > 0.0) * mix(vec3(1.0), lit, 0.5 * g.sea4.w);
        }
        default: {
            // Spark / firefly: bright core, dithered halo, unlit.
            if r > 1.0 { discard; }
            alpha *= select(1.0 - r, 1.0, r < 0.35);
        }
    }
    // Streaks screen-door per streak, not per pixel, so a 1x4 px streak keeps its slant.
    let door = select(bayer4(vec2<u32>(in.clip.xy)), fract(in.seed * 97.13), in.kind == P_STREAK);
    if alpha < door {
        discard;
    }
    col *= 1.0 + 0.5 * g.tint_flash.w;
    var fog = fog_amount(in.depth, in.y);
    if in.kind == P_SPARK {
        fog *= 0.5;
    }
    return vec4(fog_mix(col, g.fog_start.xyz, fog), 1.0);
}
