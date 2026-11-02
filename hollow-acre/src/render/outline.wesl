// 1 px outlines at internal resolution from the depth + normal prepass, plus water
// shore foam, sun sparkle and rain rings. Runs before bloom/tonemapping on the low-res view.

import bevy_core_pipeline::fullscreen_vertex_shader::FullscreenVertexOutput;

struct OutlineSettings {
    // far - near of the orthographic camera (ndc depth -> world units).
    depth_range: f32,
    // Depth jump (wu) that counts as a silhouette.
    silhouette: f32,
    // Normal dot below which a same-depth edge is a crease.
    crease_dot: f32,
    // Blend toward outline colour on creases.
    crease_strength: f32,
    time: f32,
    // 1 when the sun is strong enough for water sparkle.
    sparkle: f32,
    // 0..1 rain amount (rings on water).
    rain: f32,
    // 0..1 night factor (dims foam).
    night: f32,
    // x fog density, y height fog, z fog start (view depth), w camera near.
    fog: vec4<f32>,
    world_from_clip: mat4x4<f32>,
}

@group(0) @binding(0) var screen: texture_2d<f32>;
@group(0) @binding(1) var normals: texture_2d<f32>;
@group(0) @binding(2) var depth_tex: texture_2d<f32>;
@group(0) @binding(3) var<uniform> s: OutlineSettings;

fn to_srgb(c: vec3<f32>) -> vec3<f32> {
    return select(1.055 * pow(c, vec3(1.0 / 2.4)) - 0.055, c * 12.92, c <= vec3(0.0031308));
}

fn to_linear(c: vec3<f32>) -> vec3<f32> {
    return select(pow((c + 0.055) / 1.055, vec3(2.4)), c / 12.92, c <= vec3(0.04045));
}

fn rgb2hsv(c: vec3<f32>) -> vec3<f32> {
    let mx = max(c.r, max(c.g, c.b));
    let mn = min(c.r, min(c.g, c.b));
    let d = mx - mn;
    var h = 0.0;
    if d > 1e-5 {
        if mx == c.r {
            h = (c.g - c.b) / d;
            if h < 0.0 { h += 6.0; }
        } else if mx == c.g {
            h = (c.b - c.r) / d + 2.0;
        } else {
            h = (c.r - c.g) / d + 4.0;
        }
    }
    let sat = select(0.0, d / mx, mx > 1e-5);
    return vec3(h * 60.0, sat, mx);
}

fn hsv2rgb(hsv: vec3<f32>) -> vec3<f32> {
    let h = fract(hsv.x / 360.0) * 6.0;
    let k = (vec3(5.0, 3.0, 1.0) + vec3(h)) % vec3(6.0);
    let f = clamp(min(k, vec3(4.0) - k), vec3(0.0), vec3(1.0));
    return hsv.z - hsv.z * hsv.y * f;
}

// DESIGN: value x0.35, saturation x1.2, hue 15 deg toward 250 deg. Never pure black.
fn outline_color(lin: vec3<f32>) -> vec3<f32> {
    var hsv = rgb2hsv(to_srgb(clamp(lin, vec3(0.0), vec3(1.0))));
    var delta = 250.0 - hsv.x;
    if delta > 180.0 { delta -= 360.0; }
    if delta < -180.0 { delta += 360.0; }
    hsv.x = hsv.x + clamp(delta, -15.0, 15.0);
    hsv.y = clamp(hsv.y * 1.2, 0.0, 1.0);
    hsv.z = max(hsv.z * 0.35, 0.06);
    return to_linear(hsv2rgb(hsv));
}

fn hash3(p: vec3<f32>) -> f32 {
    return fract(sin(dot(p, vec3(12.9898, 78.233, 45.164))) * 43758.5453);
}

fn dist_at(p: vec2<i32>) -> f32 {
    let d = textureLoad(depth_tex, p, 0).r;
    // Reverse-z orthographic: ndc depth 1 = near, 0 = far (and sky).
    return (1.0 - d) * s.depth_range;
}

fn class_of(a: f32) -> i32 {
    // 2-bit alpha: 0 sky, 1 water, 2 emissive, 3 regular.
    return i32(round(a * 3.0));
}

fn ring_at(p: vec2<i32>, slot: f32) -> bool {
    return hash3(vec3(vec2<f32>(p), slot * 1.37 + 11.0)) < s.rain * 0.0012;
}

@fragment
fn fragment(in: FullscreenVertexOutput) -> @location(0) vec4<f32> {
    let size = vec2<i32>(textureDimensions(screen));
    let p = vec2<i32>(in.position.xy);
    let c = textureLoad(screen, p, 0);
    let raw = textureLoad(depth_tex, p, 0).r;
    if raw <= 0.0 {
        return vec4(c.rgb, 1.0); // sky
    }
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
        if dq - d > s.silhouette {
            silhouette = true;
        } else if abs(dq - d) <= s.silhouette {
            let nq = qs.xyz * 2.0 - 1.0;
            // Nearer side only; on exact ties the +x/+y pixel draws.
            let nearer = dq > d + 0.02 || (abs(dq - d) <= 0.02 && i % 2 == 0);
            // Terrain: no creases between two upward-facing surfaces.
            let both_up = n.y > 0.5 && nq.y > 0.5;
            if dot(n, nq) < s.crease_dot && nearer && !both_up {
                crease = true;
            }
        }
        if cls == 1 && qcls != 1 && qraw > 0.0 {
            shore = true;
        }
    }

    var col = c.rgb;
    // Outlines fade with the same fog the toon shader applied (distance + height fog).
    let ndc = vec2((in.uv.x * 2.0 - 1.0), (1.0 - in.uv.y * 2.0));
    let wp = s.world_from_clip * vec4(ndc, raw, 1.0);
    let world_y = wp.y / wp.w;
    let view_depth = d + s.fog.w;
    let fog = 1.0 - exp(-(s.fog.x * max(view_depth - s.fog.z, 0.0) + s.fog.y * max(6.0 - world_y, 0.0)));
    let vis = 1.0 - fog;
    if silhouette {
        col = mix(c.rgb, outline_color(c.rgb), vis);
    } else if crease && cls != 2 && cls != 1 {
        col = mix(c.rgb, outline_color(c.rgb), s.crease_strength * vis);
    } else if shore {
        let foam = to_linear(vec3(0xe8, 0xf6, 0xf2) / 255.0);
        col = mix(c.rgb, mix(foam, c.rgb, 0.65 * s.night), vis);
    } else if cls == 1 {
        // Sparkle: ~1 per 400 water px per second, 0.15 s each.
        let slot = floor(s.time / 0.15);
        if s.sparkle > 0.5 && hash3(vec3(vec2<f32>(p), slot)) < 0.15 / 400.0 {
            col = mix(c.rgb, vec3(1.0), vis);
        }
        // Rain rings: 1 px plus shapes for 0.2 s.
        if s.rain > 0.0 {
            let rs = floor(s.time / 0.2);
            if ring_at(p, rs) || ring_at(p + vec2(1, 0), rs) || ring_at(p - vec2(1, 0), rs)
                || ring_at(p + vec2(0, 1), rs) || ring_at(p - vec2(0, 1), rs) {
                col = mix(c.rgb, to_linear(vec3(0xcf, 0xe6, 0xee) / 255.0), vis);
            }
        }
    }
    return vec4(col, 1.0);
}
