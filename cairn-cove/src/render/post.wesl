// Low-res post pass (DESIGN A4/A5/A6): a three-band posterised sky with a hard sun disc,
// a cratered moon and twinkling stars where nothing was drawn; 1 px indigo keylines from the
// depth + normal prepass, faded into the fog colour (never grey); 2 px contact foam with a
// foam_shade pixel wherever anything meets the sea (doubled in storms); then 15-bit colour
// (5 bits per channel) with a 4x4 ordered Bayer dither (1.5 LSB at night).

import bevy_core_pipeline::fullscreen_vertex_shader::FullscreenVertexOutput;

struct PostSettings {
    depth: vec4<f32>,       // x far-near, y near, z silhouette jump, w crease strength
    fog: vec4<f32>,         // xyz fog colour, w density
    sky_top: vec4<f32>,     // xyz, w fog start
    sky_horizon: vec4<f32>, // xyz, w flash
    flags: vec4<f32>,       // x outline, y dither, z sky darkness, w time
    sky_mid: vec4<f32>,     // xyz, w sea line uv y
    outline: vec4<f32>,     // xyz keyline, w storm
    foam: vec4<f32>,        // xyz foam, w wetness
    disc: vec4<f32>,        // xyz disc colour, w kind (0 none, 1 sun, 2 moon)
    disc_pos: vec4<f32>,    // xy uv, z radius px, w night
    ycoef: vec4<f32>,       // world y = x + y*ndc_x + z*ndc_y + w*dist
    bank: vec4<f32>,        // x sea-fog strength, y bank top
}

@group(0) @binding(0) var screen: texture_2d<f32>;
@group(0) @binding(1) var normals: texture_2d<f32>;
@group(0) @binding(2) var depth_tex: texture_2d<f32>;
@group(0) @binding(3) var<uniform> s: PostSettings;

fn to_srgb(c: vec3<f32>) -> vec3<f32> {
    let x = max(c, vec3(0.0));
    return select(1.055 * pow(x, vec3(1.0 / 2.4)) - 0.055, x * 12.92, x <= vec3(0.0031308));
}

fn to_linear(c: vec3<f32>) -> vec3<f32> {
    let x = max(c, vec3(0.0));
    return select(pow((x + 0.055) / 1.055, vec3(2.4)), x / 12.92, x <= vec3(0.04045));
}

fn hex_lin(r: f32, g: f32, b: f32) -> vec3<f32> {
    return to_linear(vec3(r, g, b) / 255.0);
}

fn bayer4(p: vec2<u32>) -> f32 {
    var m = array<f32, 16>(0.0, 8.0, 2.0, 10.0, 12.0, 4.0, 14.0, 6.0, 3.0, 11.0, 1.0, 9.0, 15.0, 7.0, 13.0, 5.0);
    return (m[(p.y % 4u) * 4u + p.x % 4u] + 0.5) / 16.0 - 0.5;
}

fn hash2(p: vec2<f32>) -> f32 {
    return fract(sin(dot(p, vec2(127.1, 311.7))) * 43758.5453);
}

// View depth (m) from reverse-z orthographic ndc depth (1 = near, 0 = far).
fn dist_at(p: vec2<i32>) -> f32 {
    let d = textureLoad(depth_tex, p, 0).r;
    return s.depth.y + (1.0 - d) * s.depth.x;
}

fn class_of(a: f32) -> i32 {
    // 2-bit alpha: 0 none, 1 water, 2 emissive, 3 regular.
    return i32(round(a * 3.0));
}

// Three hard sky bands (zenith / mid / horizon) with an 8 px Bayer seam at each boundary.
fn sky(uv: vec2<f32>, p: vec2<i32>, size: vec2<i32>) -> vec3<f32> {
    let line = max(s.sky_mid.w, 0.05);
    let ypx = uv.y * f32(size.y);
    let b1 = line * 0.40 * f32(size.y);
    let b2 = line * 0.78 * f32(size.y);
    let dz = bayer4(vec2<u32>(p)) * 8.0;
    var c = s.sky_top.xyz;
    if ypx + dz > b2 {
        c = s.sky_horizon.xyz;
    } else if ypx + dz > b1 {
        c = s.sky_mid.xyz;
    }
    let t = uv.y / line;
    // Stars: ~1 per 120 sky px in the top 60 %, 1 in 16 toggled every 0.5 s.
    let dark = s.flags.z;
    if dark > 0.0 && t < 0.6 {
        let h = hash2(vec2<f32>(p));
        let blink = hash2(vec2<f32>(p) + floor(s.flags.w * 2.0) * 13.7) < 1.0 / 16.0;
        if h < dark / 120.0 && !blink {
            let k = hash2(vec2<f32>(p) * 1.37 + 5.0);
            c = select(select(hex_lin(247.0, 239.0, 222.0), hex_lin(156.0, 222.0, 255.0), k < 0.25), hex_lin(255.0, 173.0, 214.0), k < 0.125);
        }
    }
    // Sun (hard 6 px disc with a 1 px horizon-colour ring) or moon (5 px with crater pixels).
    let kind = s.disc.w;
    if kind > 0.5 {
        let centre = s.disc_pos.xy * vec2<f32>(size);
        let q = vec2<f32>(p) + 0.5 - centre;
        let r = length(q);
        let rad = s.disc_pos.z;
        if r <= rad {
            c = s.disc.xyz;
            if kind > 1.5 {
                let o = vec2<i32>(floor(q));
                if all(o == vec2(-1, 0)) || all(o == vec2(1, 1)) || all(o == vec2(0, -2)) || all(o == vec2(1, -1)) {
                    c = hex_lin(165.0, 189.0, 247.0);
                }
            }
        } else if kind < 1.5 && r <= rad + 1.0 {
            c = s.sky_horizon.xyz;
        }
    }
    return c;
}

@fragment
fn fragment(in: FullscreenVertexOutput) -> @location(0) vec4<f32> {
    let size = vec2<i32>(textureDimensions(screen));
    let p = vec2<i32>(in.position.xy);
    var col = textureLoad(screen, p, 0).rgb;
    let raw = textureLoad(depth_tex, p, 0).r;

    if raw <= 0.0 {
        // Background. Particles have no prepass depth: keep anything drawn over the clear colour.
        let cleared = all(abs(to_srgb(col) - to_srgb(s.sky_horizon.xyz)) < vec3(1.5 / 255.0));
        if cleared {
            col = sky(in.uv, p, size);
        }
    } else if s.flags.x > 0.5 {
        let d = dist_at(p);
        let nsample = textureLoad(normals, p, 0);
        let n = nsample.xyz * 2.0 - 1.0;
        let cls = class_of(nsample.a);
        let sea = cls == 1 && n.y > 0.5;
        var silhouette = false;
        var crease = false;
        let offs = array<vec2<i32>, 4>(vec2(1, 0), vec2(-1, 0), vec2(0, 1), vec2(0, -1));
        for (var i = 0; i < 4; i++) {
            let q = p + offs[i];
            if any(q < vec2(0)) || any(q >= size) {
                continue;
            }
            let qraw = textureLoad(depth_tex, q, 0).r;
            let dq = select(1e9, dist_at(q), qraw > 0.0);
            let qs = textureLoad(normals, q, 0);
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
        }
        // Contact foam: sea pixels within R px of something standing in the water.
        var foam_k = 99;
        if sea {
            let r = 2 + i32(s.foam.w > 0.5) + i32(s.outline.w > 0.5) * 2;
            for (var i = 0; i < 4; i++) {
                for (var k = 1; k <= r + 1; k++) {
                    if k >= foam_k {
                        break;
                    }
                    let q = p + offs[i] * k;
                    if any(q < vec2(0)) || any(q >= size) {
                        break;
                    }
                    let qraw = textureLoad(depth_tex, q, 0).r;
                    if qraw <= 0.0 {
                        break;
                    }
                    let qcls = class_of(textureLoad(normals, q, 0).a);
                    if qcls != 1 && abs(dist_at(q) - d) < 1.2 + 0.3 * f32(k) {
                        foam_k = k;
                        break;
                    }
                }
            }
            if foam_k <= r {
                col = s.foam.xyz;
            } else if foam_k == r + 1 {
                col = s.foam.xyz * hex_lin(140.0, 239.0, 239.0);
            }
        }
        // Fog at this pixel (distance + the sea-fog bank by world height).
        let ndc = vec2(in.uv.x * 2.0 - 1.0, 1.0 - in.uv.y * 2.0);
        let wy = s.ycoef.x + s.ycoef.y * ndc.x + s.ycoef.z * ndc.y + s.ycoef.w * d;
        var bank = 0.0;
        let top = s.bank.y;
        if wy < top * 0.34 {
            bank = 0.82;
        } else if wy < top * 0.67 {
            bank = 0.68;
        } else if wy < top {
            bank = 0.5;
        }
        let dist_fog = 1.0 - exp(-s.fog.w * max(d - s.sky_top.w, 0.0));
        let fog = 1.0 - (1.0 - dist_fog) * (1.0 - bank * s.bank.x);
        let ink = mix(s.outline.xyz, s.fog.xyz, fog);
        if silhouette && cls != 1 {
            col = ink;
        } else if crease && cls == 3 {
            col = mix(col, ink, s.depth.w);
        }
    }

    if s.flags.y > 0.5 {
        let levels = 31.0;
        let amp = 1.0 + 0.5 * clamp(s.disc_pos.w, 0.0, 1.0);
        let dd = bayer4(vec2<u32>(p)) * amp;
        let q = floor(clamp(to_srgb(col), vec3(0.0), vec3(1.0)) * levels + dd + 0.5) / levels;
        col = to_linear(clamp(q, vec3(0.0), vec3(1.0)));
    }
    return vec4(col, 1.0);
}
