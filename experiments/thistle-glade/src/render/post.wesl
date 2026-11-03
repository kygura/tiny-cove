// Low-res post pass: sky gradient where nothing was drawn, 1 px outlines from the depth +
// normal prepass (darkened scene colour, faded with fog), shore foam on water, then
// 15-bit colour (5 bits per channel) with a 4x4 ordered Bayer dither.

import bevy_core_pipeline::fullscreen_vertex_shader::FullscreenVertexOutput;

struct PostSettings {
    depth: vec4<f32>,       // x far-near, y near, z silhouette jump, w crease strength
    fog: vec4<f32>,         // xyz fog colour, w density
    sky_top: vec4<f32>,     // xyz, w fog start
    sky_horizon: vec4<f32>, // xyz, w flash
    flags: vec4<f32>,       // x outline, y dither, z night, w time
}

@group(0) @binding(0) var screen: texture_2d<f32>;
@group(0) @binding(1) var normals: texture_2d<f32>;
@group(0) @binding(2) var depth_tex: texture_2d<f32>;
@group(0) @binding(3) var<uniform> s: PostSettings;

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

// View depth (m) from reverse-z orthographic ndc depth (1 = near, 0 = far).
fn dist_at(p: vec2<i32>) -> f32 {
    let d = textureLoad(depth_tex, p, 0).r;
    return s.depth.y + (1.0 - d) * s.depth.x;
}

// Still the camera clear colour (sky horizon)?
fn is_clear(c: vec3<f32>) -> bool {
    return all(abs(to_srgb(c) - to_srgb(s.sky_horizon.xyz)) < vec3(1.5 / 255.0));
}

fn class_of(a: f32) -> i32 {
    // 2-bit alpha: 0 none, 1 water, 2 emissive, 3 regular.
    return i32(round(a * 3.0));
}

// Darker, slightly cooler and more saturated version of the scene colour.
fn outline_color(c: vec3<f32>) -> vec3<f32> {
    let l = dot(c, vec3(0.3, 0.55, 0.15));
    let sat = mix(vec3(l), c, 1.25);
    return max(sat * vec3(0.3, 0.3, 0.38), vec3(0.008, 0.008, 0.016));
}

@fragment
fn fragment(in: FullscreenVertexOutput) -> @location(0) vec4<f32> {
    let size = vec2<i32>(textureDimensions(screen));
    let p = vec2<i32>(in.position.xy);
    var col = textureLoad(screen, p, 0).rgb;
    let raw = textureLoad(depth_tex, p, 0).r;
    // Safety net: the prepass has geometry here but the main pass drew nothing (still
    // the clear colour) -> take the colour of a drawn neighbour instead of a sky sliver.
    if raw > 0.0 && is_clear(col) {
        let offs = array<vec2<i32>, 4>(vec2(1, 0), vec2(-1, 0), vec2(0, 1), vec2(0, -1));
        for (var i = 0; i < 4; i++) {
            let q = clamp(p + offs[i], vec2(0), size - 1);
            let qc = textureLoad(screen, q, 0).rgb;
            if textureLoad(depth_tex, q, 0).r > 0.0 && !is_clear(qc) {
                col = qc;
                break;
            }
        }
    }

    if raw <= 0.0 {
        // Background: sky gradient (screen-space for the orthographic view).
        let t = clamp(1.0 - in.uv.y * 1.3, 0.0, 1.0);
        let sky = mix(s.sky_horizon.xyz, s.sky_top.xyz, t * t * (3.0 - 2.0 * t));
        // Particles have no prepass depth: keep anything already drawn over the clear colour.
        if is_clear(col) {
            col = sky + vec3(0.75, 0.82, 1.0) * s.sky_horizon.w * 0.5;
        }
    } else if s.flags.x > 0.5 {
        let d = dist_at(p);
        let nsample = textureLoad(normals, p, 0);
        let n = nsample.xyz * 2.0 - 1.0;
        let cls = class_of(nsample.a);
        var silhouette = false;
        var crease = false;
        var shore = false;
        let offs = array<vec2<i32>, 4>(vec2(1, 0), vec2(-1, 0), vec2(0, 1), vec2(0, -1));
        for (var i = 0; i < 4; i++) {
            let q = p + offs[i];
            if any(q < vec2(0)) || any(q >= size) {
                continue;
            }
            let qraw = textureLoad(depth_tex, q, 0).r;
            let dq = select(1e9, dist_at(q), qraw > 0.0);
            let qs = textureLoad(normals, q, 0);
            let qcls = class_of(qs.a);
            if dq - d > s.depth.z {
                silhouette = true;
            } else if abs(dq - d) <= s.depth.z {
                let nq = qs.xyz * 2.0 - 1.0;
                // Nearer side only; on exact ties the +x / +y neighbour pair draws.
                let nearer = dq > d + 0.02 || (abs(dq - d) <= 0.02 && i % 2 == 0);
                let both_up = n.y > 0.6 && nq.y > 0.6;
                if dot(n, nq) < 0.75 && nearer && !both_up {
                    crease = true;
                }
            }
            if cls == 1 && qcls != 1 && qraw > 0.0 {
                shore = true;
            }
        }
        let fog = 1.0 - exp(-s.fog.w * max(d - s.sky_top.w, 0.0));
        let vis = 1.0 - fog;
        if silhouette && cls != 1 {
            col = mix(col, outline_color(col), vis);
        } else if crease && cls == 3 {
            col = mix(col, outline_color(col), s.depth.w * vis);
        } else if shore {
            let foam = mix(vec3(0.85, 0.93, 0.92), col, 0.6 * s.flags.z);
            col = mix(col, foam, 0.85 * vis);
        }
    }

    if s.flags.y > 0.5 {
        let levels = 31.0;
        let d = bayer4(vec2<u32>(p));
        let q = floor(clamp(to_srgb(col), vec3(0.0), vec3(1.0)) * levels + d + 0.5) / levels;
        col = to_linear(clamp(q, vec3(0.0), vec3(1.0)));
    }
    return vec4(col, 1.0);
}
