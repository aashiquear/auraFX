// Full-screen passes after the geometry: Eye-Dome Lighting and the transparency composite.

struct VsOut {
    @builtin(position) pos: vec4<f32>,
};

@vertex
fn vs_full(@builtin(vertex_index) i: u32) -> VsOut {
    let p = vec2<f32>(f32((i << 1u) & 2u), f32(i & 2u));
    var o: VsOut;
    o.pos = vec4<f32>(p * 2.0 - 1.0, 0.0, 1.0);
    return o;
}

// ---- Eye-Dome Lighting (Boucheny 2009): darken pixels that lie behind their neighbours ----
@group(0) @binding(0) var scene_depth: texture_depth_multisampled_2d;

struct EdlUniform {
    camera: vec4<f32>,   // near, far, orthographic, strength
    view: vec4<f32>,     // width, height, radius in pixels, 0
};
@group(1) @binding(0) var<uniform> edl: EdlUniform;

fn log_depth(c: vec2<i32>) -> f32 {
    let size = vec2<i32>(i32(edl.view.x), i32(edl.view.y));
    let q = clamp(c, vec2<i32>(0), size - 1);
    let d = textureLoad(scene_depth, q, 0);
    if (d >= 1.0) {
        return 0.0; // background counts as very near, so silhouettes get an outline
    }
    let n = edl.camera.x;
    let f = edl.camera.y;
    var z = n + d * (f - n);
    if (edl.camera.z < 0.5) {
        z = n * f / (f - d * (f - n));
    }
    return log2(max(z, 1e-30));
}

@fragment
fn fs_edl(v: VsOut) -> @location(0) vec4<f32> {
    let c = vec2<i32>(v.pos.xy);
    let d0 = textureLoad(scene_depth, c, 0);
    if (d0 >= 1.0) {
        return vec4<f32>(1.0);
    }
    let l0 = log_depth(c);
    let r = i32(edl.view.z);
    var sum = 0.0;
    let offs = array<vec2<i32>, 8>(vec2<i32>(1, 0), vec2<i32>(-1, 0), vec2<i32>(0, 1), vec2<i32>(0, -1),
                                   vec2<i32>(1, 1), vec2<i32>(-1, 1), vec2<i32>(1, -1), vec2<i32>(-1, -1));
    for (var k = 0; k < 8; k = k + 1) {
        sum = sum + max(0.0, l0 - log_depth(c + offs[k] * r));
    }
    let shade = exp(-edl.camera.w * 120.0 * sum / 8.0);
    return vec4<f32>(shade, shade, shade, 1.0);
}

// ---- weighted blended transparency: resolve the accumulated layers over the scene ----
@group(0) @binding(0) var oit_accum: texture_2d<f32>;
@group(0) @binding(1) var oit_reveal: texture_2d<f32>;

@fragment
fn fs_oit(v: VsOut) -> @location(0) vec4<f32> {
    let c = vec2<i32>(v.pos.xy);
    let reveal = textureLoad(oit_reveal, c, 0).r;
    if (reveal >= 1.0) {
        discard;
    }
    let a = textureLoad(oit_accum, c, 0);
    let color = a.rgb / clamp(a.a, 1e-5, 5e4);
    return vec4<f32>(color, 1.0 - reveal);
}

// ---- screen-space ambient occlusion (depth only; normals from neighbouring depths) ----
struct AoUniform {
    camera: vec4<f32>,   // near, far, orthographic, tan(fov_y / 2) or ortho half-height
    view: vec4<f32>,     // width, height, radius (world units), strength
};
@group(1) @binding(0) var<uniform> ao: AoUniform;
@group(1) @binding(1) var ao_tex: texture_2d<f32>;

fn view_pos(c: vec2<i32>) -> vec3<f32> {
    let size = vec2<i32>(i32(ao.view.x), i32(ao.view.y));
    let q = clamp(c, vec2<i32>(0), size - 1);
    let d = textureLoad(scene_depth, q, 0);
    let n = ao.camera.x;
    let f = ao.camera.y;
    let ndc = vec2<f32>((f32(q.x) + 0.5) / ao.view.x * 2.0 - 1.0, 1.0 - (f32(q.y) + 0.5) / ao.view.y * 2.0);
    let aspect = ao.view.x / ao.view.y;
    if (ao.camera.z > 0.5) {
        let z = n + d * (f - n);
        return vec3<f32>(ndc.x * ao.camera.w * aspect, ndc.y * ao.camera.w, -z);
    }
    let z = n * f / (f - d * (f - n));
    return vec3<f32>(ndc.x * z * ao.camera.w * aspect, ndc.y * z * ao.camera.w, -z);
}

fn project(p: vec3<f32>) -> vec2<f32> {
    let aspect = ao.view.x / ao.view.y;
    var ndc = vec2<f32>(0.0);
    if (ao.camera.z > 0.5) {
        ndc = vec2<f32>(p.x / (ao.camera.w * aspect), p.y / ao.camera.w);
    } else {
        ndc = vec2<f32>(p.x / (-p.z * ao.camera.w * aspect), p.y / (-p.z * ao.camera.w));
    }
    return vec2<f32>((ndc.x * 0.5 + 0.5) * ao.view.x, (0.5 - ndc.y * 0.5) * ao.view.y);
}

@fragment
fn fs_ssao(v: VsOut) -> @location(0) vec4<f32> {
    let c = vec2<i32>(v.pos.xy);
    if (textureLoad(scene_depth, c, 0) >= 1.0) {
        return vec4<f32>(1.0);
    }
    let p = view_pos(c);
    // Normal from the neighbours on the side with the smaller depth jump (no smearing at edges).
    let px = view_pos(c + vec2<i32>(1, 0));
    let nx = view_pos(c - vec2<i32>(1, 0));
    let py = view_pos(c + vec2<i32>(0, 1));
    let ny = view_pos(c - vec2<i32>(0, 1));
    var dx = px - p;
    if (abs(nx.z - p.z) < abs(px.z - p.z)) {
        dx = p - nx;
    }
    var dy = py - p;
    if (abs(ny.z - p.z) < abs(py.z - p.z)) {
        dy = p - ny;
    }
    var n = normalize(cross(dy, dx));
    if (dot(n, -p) < 0.0) {
        n = -n;
    }
    // A rotated spiral of samples in the hemisphere around the normal.
    let cell = vec2<u32>(u32(c.x) & 3u, u32(c.y) & 3u);
    let hash = (f32(cell.x * 4u + cell.y) * 0.4472 + f32(cell.y) * 0.0625) % 1.0;
    var t = normalize(cross(n, vec3<f32>(0.577, 0.577, 0.577) + vec3<f32>(hash, 0.0, -hash)));
    let b = cross(n, t);
    let radius = ao.view.z;
    var occluded = 0.0;
    let count = 16;
    for (var k = 0; k < count; k = k + 1) {
        let fk = f32(k) + 0.5;
        let ang = fk * 2.39996 + hash * 6.2832;
        let r = sqrt(fk / f32(count));
        let h = sqrt(max(0.0, 1.0 - r * r));
        let s = p + (t * cos(ang) * r + b * sin(ang) * r + n * h) * radius * (0.3 + 0.7 * fk / f32(count));
        let sc = vec2<i32>(project(s));
        if (sc.x < 0 || sc.y < 0 || sc.x >= i32(ao.view.x) || sc.y >= i32(ao.view.y)) {
            continue;
        }
        if (textureLoad(scene_depth, sc, 0) >= 1.0) {
            continue;
        }
        let q = view_pos(sc);
        // Occluded when the surface there is in front of the sample, but not far in front.
        let dz = q.z - s.z;
        if (dz > 0.02 * radius) {
            occluded = occluded + smoothstep(0.0, 1.0, radius / max(abs(p.z - q.z), 1e-6));
        }
    }
    let a = clamp(1.0 - 1.6 * ao.view.w * occluded / f32(count), 0.0, 1.0);
    return vec4<f32>(a, a, a, 1.0);
}

// ---- final pass: FXAA and tone mapping ----
@group(0) @binding(0) var final_src: texture_2d<f32>;
struct FinalUniform {
    params: vec4<f32>,   // fxaa on, tone map (0 none, 1 Reinhard, 2 ACES), exposure, 0
};
@group(0) @binding(1) var<uniform> fin: FinalUniform;

fn luma(c: vec3<f32>) -> f32 {
    return dot(c, vec3<f32>(0.299, 0.587, 0.114));
}

fn texel(c: vec2<i32>) -> vec3<f32> {
    let size = vec2<i32>(textureDimensions(final_src));
    return textureLoad(final_src, clamp(c, vec2<i32>(0), size - 1), 0).rgb;
}

@fragment
fn fs_final(v: VsOut) -> @location(0) vec4<f32> {
    let c = vec2<i32>(v.pos.xy);
    var col = texel(c);
    if (fin.params.x > 0.5) {
        // FXAA-style: blend across edges found from luma contrast.
        let lc = luma(col);
        let ln = luma(texel(c + vec2<i32>(0, -1)));
        let ls = luma(texel(c + vec2<i32>(0, 1)));
        let le = luma(texel(c + vec2<i32>(1, 0)));
        let lw = luma(texel(c + vec2<i32>(-1, 0)));
        let lo = min(lc, min(min(ln, ls), min(le, lw)));
        let hi = max(lc, max(max(ln, ls), max(le, lw)));
        if (hi - lo > max(0.0312, hi * 0.125)) {
            let horizontal = abs(ln + ls - 2.0 * lc) >= abs(le + lw - 2.0 * lc);
            var a = vec3<f32>(0.0);
            var b = vec3<f32>(0.0);
            if (horizontal) {
                a = texel(c + vec2<i32>(0, -1));
                b = texel(c + vec2<i32>(0, 1));
            } else {
                a = texel(c + vec2<i32>(-1, 0));
                b = texel(c + vec2<i32>(1, 0));
            }
            let blend = clamp((hi - lo) / max(hi, 1e-4), 0.0, 1.0) * 0.5;
            col = mix(col, (a + b) * 0.5, blend);
        }
    }
    col = col * fin.params.z;
    if (fin.params.y > 1.5) {
        // ACES filmic (Narkowicz fit).
        col = clamp((col * (2.51 * col + 0.03)) / (col * (2.43 * col + 0.59) + 0.14), vec3<f32>(0.0), vec3<f32>(1.0));
    } else if (fin.params.y > 0.5) {
        col = col / (col + vec3<f32>(1.0));
    }
    return vec4<f32>(col, textureLoad(final_src, c, 0).a);
}

// Depth-aware 4x4 blur of the occlusion (the per-pixel sample rotation repeats every 4 pixels
// in effect), multiplied onto the scene. Neighbours at a different depth are left out, so edges
// stay sharp.
@fragment
fn fs_ao_blur(v: VsOut) -> @location(0) vec4<f32> {
    let c = vec2<i32>(v.pos.xy);
    if (textureLoad(scene_depth, c, 0) >= 1.0) {
        return vec4<f32>(1.0);
    }
    let z = view_pos(c).z;
    let size = vec2<i32>(i32(ao.view.x), i32(ao.view.y));
    var sum = 0.0;
    var weight = 0.0;
    for (var y = -2; y <= 1; y = y + 1) {
        for (var x = -2; x <= 1; x = x + 1) {
            let q = clamp(c + vec2<i32>(x, y), vec2<i32>(0), size - 1);
            let dz = abs(view_pos(q).z - z);
            let w = select(0.0, 1.0, dz < 0.25 * ao.view.z);
            sum = sum + textureLoad(ao_tex, q, 0).r * w;
            weight = weight + w;
        }
    }
    let a = select(1.0, sum / weight, weight > 0.0);
    return vec4<f32>(a, a, a, 1.0);
}

// ---- Surface LIC: convolve noise along the projected vector field ----
// Uses the occlusion bindings: ao.camera = near, far, orthographic, unused;
// ao.view = width, height, steps each way, contrast; ao_tex = directions from fs_lic_vec.
fn linear_depth(d: f32) -> f32 {
    let n = ao.camera.x;
    let f = ao.camera.y;
    if (ao.camera.z > 0.5) {
        return n + d * (f - n);
    }
    return n * f / (f - d * (f - n));
}

fn lattice(i: vec2<i32>) -> f32 {
    var h = (u32(i.x) * 73856093u) ^ (u32(i.y) * 19349663u);
    h = (h ^ (h >> 13u)) * 1274126177u;
    h = h ^ (h >> 16u);
    return f32(h & 0xffffu) / 65535.0;
}

// White noise on the pixel lattice, bilinearly interpolated.
fn value_noise(p: vec2<f32>) -> f32 {
    let q = p - 0.5;
    let i = vec2<i32>(floor(q));
    let f = q - floor(q);
    let a = mix(lattice(i), lattice(i + vec2<i32>(1, 0)), f.x);
    let b = mix(lattice(i + vec2<i32>(0, 1)), lattice(i + vec2<i32>(1, 1)), f.x);
    return mix(a, b, f.y);
}

@fragment
fn fs_lic(v: VsOut) -> @location(0) vec4<f32> {
    let c = vec2<i32>(v.pos.xy);
    let here = textureLoad(ao_tex, c, 0);
    if (here.w < 0.75) {
        return vec4<f32>(1.0);
    }
    // Only where the LIC surface is what is seen.
    let seen = linear_depth(textureLoad(scene_depth, c, 0));
    let mine = linear_depth(here.z);
    if (abs(seen - mine) > 0.01 * mine) {
        return vec4<f32>(1.0);
    }
    let size = vec2<i32>(i32(ao.view.x), i32(ao.view.y));
    let steps = i32(ao.view.z);
    var sum = value_noise(v.pos.xy);
    var count = 1.0;
    for (var side = 0; side < 2; side = side + 1) {
        var p = v.pos.xy;
        var t = here.xy;
        if (side == 1) {
            t = -t;
        }
        for (var k = 0; k < steps; k = k + 1) {
            p = p + t;
            let q = vec2<i32>(floor(p));
            if (q.x < 0 || q.y < 0 || q.x >= size.x || q.y >= size.y) {
                break;
            }
            let s = textureLoad(ao_tex, q, 0);
            if (s.w < 0.75 || abs(linear_depth(s.z) - mine) > 0.05 * mine) {
                break;
            }
            var nt = s.xy;
            if (dot(nt, t) < 0.0) {
                nt = -nt;
            }
            t = nt;
            sum = sum + value_noise(p);
            count = count + 1.0;
        }
    }
    // Contrast: the mean of n noise samples spreads as 1/sqrt(n).
    let m = sum / count;
    let e = clamp(0.5 + (m - 0.5) * sqrt(count) * ao.view.w, 0.0, 1.0);
    let f = 1.0 - clamp(here.w - 1.0, 0.0, 1.0) * (1.0 - e);
    return vec4<f32>(f, f, f, 1.0);
}
