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
    rim: vec4<f32>,         // xyz rim colour, w ridge-rim strength (storm)
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

// Colour helpers mirrored from glade.wesl.
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

// Hue where the cool path of the A3 direction rule starts (palette::WARM_EDGE).
const WARM_EDGE: f32 = 70.0;

// DESIGN A3 direction rule: positive when WARM_EDGE <= h < t, else the warm (negative) path.
fn rotate_toward(h: f32, t: f32, amount: f32) -> f32 {
    if h >= WARM_EDGE && h < t {
        return h + min(amount, t - h);
    }
    let dist = modp(h - t, 360.0);
    return modp(h - min(amount, dist), 360.0);
}

// Fog mix that never greys: mirrors glade.wesl `fog_mix` (a pale fog lends its own hue to
// whatever it holds at the saturation floor; a saturated fog walks the hue along A3).
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
    var h = select(f.x, o.x, o.y >= 0.5 * s_min);
    if f.y < 0.3 || a.y < 0.15 {
        h = f.x;
    } else if o.y < 0.5 * s_min {
        var dist = modp(a.x - f.x, 360.0);
        if a.x >= WARM_EDGE && a.x < f.x {
            dist = f.x - a.x;
        }
        h = rotate_toward(a.x, f.x, dist * k * smoothstep(0.1, 0.5, f.y));
    }
    o.x = h;
    o.y = s_min;
    return to_linear(hsv2rgb(o));
}

// Mirrors glade.wesl `bank_fog`: what the day bank veils colour `c` toward when the pixel is
// inside the bank (`inside`), so the ink fades to a lilac pearl.
fn bank_fog(c: vec3<f32>, inside: bool) -> vec3<f32> {
    let f = s.fog.xyz;
    let fs = to_srgb(f);
    let pearl = clamp((fs.g - fs.r) / 0.15, 0.0, 1.0)
        * smoothstep(0.55, 0.75, min(fs.r, min(fs.g, fs.b)));
    if s.bank.x < 0.05 || !inside || pearl <= 0.0 {
        return f;
    }
    let a = rgb2hsv(to_srgb(c));
    let k = smoothstep(0.12, 0.3, a.y) * pearl * s.bank.x;
    let warm = max(1.0 - smoothstep(60.0, 90.0, a.x), smoothstep(320.0, 345.0, a.x));
    let lilac = smoothstep(190.0, 215.0, a.x) * (1.0 - smoothstep(320.0, 345.0, a.x));
    let v = max(fs.r, max(fs.g, fs.b));
    let hw = modp(clamp(select(a.x, a.x - 360.0, a.x > 200.0), -10.0, 45.0), 360.0);
    let hl = clamp(a.x, 240.0, 290.0);
    var o = mix(f, to_linear(hsv2rgb(vec3(hw, 0.1, v))), warm * k);
    o = mix(o, to_linear(hsv2rgb(vec3(hl, 0.1, v))), lilac * k);
    return o;
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

// Three hard sky bands (zenith / mid / horizon) with a 4 px Bayer seam at each boundary, so
// the bands stay distinct even when the sky is a short strip.
fn sky(uv: vec2<f32>, p: vec2<i32>, size: vec2<i32>) -> vec3<f32> {
    let line = max(s.sky_mid.w, 0.05);
    let ypx = uv.y * f32(size.y);
    let b1 = line * 0.40 * f32(size.y);
    let b2 = line * 0.78 * f32(size.y);
    let dz = bayer4(vec2<u32>(p)) * 4.0;
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
        var sil = array<bool, 4>(false, false, false, false);
        var crease = false;
        let offs = array<vec2<i32>, 4>(vec2(1, 0), vec2(-1, 0), vec2(0, 1), vec2(0, -1));
        // Neighbour view depths (1e9 = background or off-screen).
        var dqs = array<f32, 4>(1e9, 1e9, 1e9, 1e9);
        for (var i = 0; i < 4; i++) {
            let q = p + offs[i];
            if all(q >= vec2(0)) && all(q < size) && textureLoad(depth_tex, q, 0).r > 0.0 {
                dqs[i] = dist_at(q);
            }
        }
        for (var i = 0; i < 4; i++) {
            let q = p + offs[i];
            if any(q < vec2(0)) || any(q >= size) {
                continue;
            }
            let dq = dqs[i];
            // A slope seen at a grazing angle steps in depth evenly from pixel to pixel; only
            // a step bigger than the one on the other side (a real edge) is a silhouette.
            let even = max(d - dqs[i ^ 1], 0.0);
            if dq - d > s.depth.z + even {
                sil[i] = true;
            } else if abs(dq - d) <= s.depth.z + even {
                let nq = textureLoad(normals, q, 0).xyz * 2.0 - 1.0;
                // Nearer side only; on exact ties the +x / +y neighbour pair draws.
                let nearer = dq > d + 0.02 || (abs(dq - d) <= 0.02 && i % 2 == 0);
                let both_up = n.y > 0.6 && nq.y > 0.6;
                if dot(n, nq) < 0.75 && nearer && !both_up {
                    crease = true;
                }
            }
        }
        // A body's edge is inked; a feature only 1 px across (open on both opposite sides: a
        // post, stake or rope at far zoom) keeps its own colour, so thin things never turn
        // into a net of ink.
        let silhouette = sil[0] != sil[1] || sil[2] != sil[3];
        // Top edge against something well behind it (open above, solid below): the storm's
        // cyan ridge hairline (DESIGN A4).
        let ridge = sil[3] && !sil[2] && dqs[3] - d > 3.0;
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
        // Bank layers mirror glade.wesl BANK_LOW / BANK_MID / BANK_TOP.
        if wy < top * 0.34 {
            bank = 0.6;
        } else if wy < top * 0.67 {
            bank = 0.48;
        } else if wy < top {
            bank = 0.36;
        }
        let dist_fog = 1.0 - exp(-s.fog.w * max(d - s.sky_top.w, 0.0));
        let fog = 1.0 - (1.0 - dist_fog) * (1.0 - bank * s.bank.x);
        var ink = fog_mix(s.outline.xyz, bank_fog(s.outline.xyz, wy < top), fog);
        // Ink is always darker than what it outlines (a storm-dark cliff keeps a dark line,
        // never a light hairline over a darker fill).
        var iv = rgb2hsv(to_srgb(ink));
        let cv = rgb2hsv(to_srgb(col)).z;
        if iv.z > cv * 0.7 {
            iv.z = cv * 0.7;
            ink = to_linear(hsv2rgb(iv));
        }
        if silhouette && cls != 1 {
            col = ink;
            if ridge && cls == 3 && s.rim.w > 0.0 {
                col = mix(ink, s.rim.xyz, s.rim.w * (1.0 - fog));
            }
        } else if crease && cls == 3 {
            // In the sea-fog bank the ink has taken a pale fog's hue, far from that of warm
            // stone: a plain half mix of the two is grey (DESIGN A0.3), so the crease goes
            // through the fog mix's saturation hold.
            col = select(mix(col, ink, s.depth.w), fog_mix(col, ink, s.depth.w), s.bank.x > 0.0);
        }
        // A pale stake under fogged ink, or its crease, lands on a mid-value grey-lilac in sea
        // fog, and a pearl-veiled thatch eave under its crease on a brown: never under S 0.3
        // there (the 5-bit dither keeps it above 0.2), nor a dark brown under the A3 anti-mud
        // S 0.4 (DESIGN A0.3). Pale fog itself (V >= 0.65) is untouched.
        if s.bank.x > 0.0 && cls != 0 {
            var cc = rgb2hsv(to_srgb(col));
            if cc.z < 0.65 {
                let mud = cc.z < 0.45 && cc.x >= 15.0 && cc.x < 100.0;
                cc.y = max(cc.y, select(0.3, 0.4, mud));
                col = to_linear(hsv2rgb(cc));
            }
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
