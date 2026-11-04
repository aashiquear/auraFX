// Ray-cast volume rendering of a normalized scalar field in a 3D texture.

struct NodeUniform {
    view_proj: mat4x4<f32>,
    camera_pos: vec4<f32>,
    light_dir: vec4<f32>,
    viewport: vec4<f32>,
    color: vec4<f32>,
    range: vec4<f32>,
    flags: vec4<f32>,
};

struct VolumeParams {
    box_min: vec4<f32>,
    box_max: vec4<f32>,
    dims: vec4<f32>,
    // opacity scale, threshold, samples across the box diagonal, unused
    params: vec4<f32>,
};

@group(0) @binding(0) var<uniform> node: NodeUniform;
@group(0) @binding(1) var lut: texture_1d<f32>;
@group(0) @binding(2) var lut_sampler: sampler;
@group(1) @binding(0) var vol: texture_3d<f32>;
@group(1) @binding(1) var vol_sampler: sampler;
@group(1) @binding(2) var<uniform> vp: VolumeParams;

struct VOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) local: vec3<f32>,
};

@vertex
fn vs_volume(@builtin(vertex_index) vi: u32) -> VOut {
    // 12 triangles of the unit cube.
    var idx = array<u32, 36>(
        0u, 2u, 1u, 0u, 3u, 2u,  4u, 5u, 6u, 4u, 6u, 7u,
        0u, 1u, 5u, 0u, 5u, 4u,  3u, 7u, 6u, 3u, 6u, 2u,
        0u, 4u, 7u, 0u, 7u, 3u,  1u, 2u, 6u, 1u, 6u, 5u);
    let c = idx[vi];
    let corner = vec3<f32>(f32((c & 1u) ^ ((c >> 1u) & 1u)), f32((c >> 1u) & 1u), f32((c >> 2u) & 1u));
    let p = mix(vp.box_min.xyz, vp.box_max.xyz, corner);
    var o: VOut;
    o.clip = node.view_proj * vec4<f32>(p, 1.0);
    o.local = p;
    return o;
}

fn lut_color(v: f32) -> vec3<f32> {
    let x = clamp(v, 0.0, 1.0) * 255.0;
    let i0 = i32(floor(x));
    let i1 = min(i0 + 1, 255);
    let f = x - f32(i0);
    return mix(textureLoad(lut, i0, 0).rgb, textureLoad(lut, i1, 0).rgb, f);
}

@fragment
fn fs_volume(in: VOut) -> @location(0) vec4<f32> {
    let cam = node.camera_pos.xyz;
    let dir = normalize(in.local - cam);
    let bmin = vp.box_min.xyz;
    let bmax = vp.box_max.xyz;
    let inv = 1.0 / dir;
    let ta = (bmin - cam) * inv;
    let tb = (bmax - cam) * inv;
    let tlo = min(ta, tb);
    let thi = max(ta, tb);
    let t_enter = max(max(tlo.x, tlo.y), tlo.z);
    let t_exit = min(min(thi.x, thi.y), thi.z);
    let start = max(t_enter, 0.0);
    if (t_exit <= start) {
        discard;
    }
    let diag = length(bmax - bmin);
    let step = diag / max(vp.params.z, 16.0);
    let unit = diag / 64.0;
    let n = min(i32(ceil((t_exit - start) / step)), 2048);
    let size = bmax - bmin;
    let dims = vp.dims.xyz;
    var acc = vec3<f32>(0.0);
    var alpha = 0.0;
    for (var i = 0; i < n; i = i + 1) {
        let p = cam + dir * (start + (f32(i) + 0.5) * step);
        var tc = (p - bmin) / max(size, vec3<f32>(1e-20));
        tc = tc * (dims - 1.0) / dims + 0.5 / dims;
        let v = textureSampleLevel(vol, vol_sampler, tc, 0.0).r;
        if (v != v || v < vp.params.y) {
            continue;
        }
        let ramp = clamp((v - vp.params.y) / max(1.0 - vp.params.y, 1e-4), 0.0, 1.0);
        let a_unit = clamp(ramp * 0.35 * vp.params.x, 0.0, 0.995);
        let a = 1.0 - pow(1.0 - a_unit, step / unit);
        let c = lut_color(v);
        acc = acc + (1.0 - alpha) * a * c;
        alpha = alpha + (1.0 - alpha) * a;
        if (alpha > 0.985) {
            break;
        }
    }
    return vec4<f32>(acc, alpha);
}
