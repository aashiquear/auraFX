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
