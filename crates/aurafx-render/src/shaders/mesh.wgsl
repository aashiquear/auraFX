struct NodeUniform {
    view_proj: mat4x4<f32>,    // includes the node's local origin
    camera_pos: vec4<f32>,     // origin-relative
    light_dir: vec4<f32>,
    viewport: vec4<f32>,       // width, height, 0, 0
    color: vec4<f32>,          // solid color + opacity
    range: vec4<f32>,          // lo, hi, log_scale, use_scalar
    flags: vec4<f32>,          // lighting, point_size_px, spheres, 0
};

@group(0) @binding(0) var<uniform> node: NodeUniform;
@group(0) @binding(1) var lut: texture_1d<f32>;
@group(0) @binding(2) var lut_sampler: sampler;

struct VertexIn {
    @location(0) pos: vec3<f32>,
    @location(1) scalar: f32,
    @location(2) normal: vec3<f32>,
};

struct VertexOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) world: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) scalar: f32,
};

fn scalar_color(s: f32) -> vec3<f32> {
    if (s != s) { // NaN
        return vec3<f32>(0.5, 0.5, 0.5);
    }
    var lo = node.range.x;
    var hi = node.range.y;
    var v = s;
    if (node.range.z > 0.5) {
        lo = log(max(lo, 1e-30));
        hi = log(max(hi, 1e-30));
        v = log(max(s, 1e-30));
    }
    var t = 0.5;
    if (hi > lo) {
        t = clamp((v - lo) / (hi - lo), 0.0, 1.0);
    }
    return textureSample(lut, lut_sampler, t).rgb;
}

fn base_color(s: f32) -> vec3<f32> {
    if (node.range.w > 0.5) {
        return scalar_color(s);
    }
    return node.color.rgb;
}

fn shade(color: vec3<f32>, n_in: vec3<f32>, world: vec3<f32>) -> vec3<f32> {
    if (node.flags.x < 0.5) {
        return color;
    }
    var n = normalize(n_in);
    let view_dir = normalize(node.camera_pos.xyz - world);
    if (dot(n, view_dir) < 0.0) {
        n = -n;
    }
    let l = normalize(node.light_dir.xyz);
    let diffuse = max(dot(n, l), 0.0);
    let h = normalize(l + view_dir);
    let spec = pow(max(dot(n, h), 0.0), 60.0) * 0.08;
    let ambient = 0.3;
    return color * (ambient + 0.7 * diffuse) + vec3<f32>(spec);
}

@vertex
fn vs_mesh(in: VertexIn) -> VertexOut {
    var out: VertexOut;
    out.clip = node.view_proj * vec4<f32>(in.pos, 1.0);
    out.world = in.pos;
    out.normal = in.normal;
    out.scalar = in.scalar;
    return out;
}

@fragment
fn fs_mesh(in: VertexOut) -> @location(0) vec4<f32> {
    let c = shade(base_color(in.scalar), in.normal, in.world);
    return vec4<f32>(c, node.color.a);
}

@fragment
fn fs_line(in: VertexOut) -> @location(0) vec4<f32> {
    return vec4<f32>(base_color(in.scalar), node.color.a);
}

// ---- point sprites: 6 vertices per instance ----
struct PointOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) scalar: f32,
    @location(2) world: vec3<f32>,
};

@vertex
fn vs_point(@builtin(vertex_index) vi: u32, in: VertexIn) -> PointOut {
    var corners = array<vec2<f32>, 6>(
        vec2<f32>(-1.0, -1.0), vec2<f32>(1.0, -1.0), vec2<f32>(1.0, 1.0),
        vec2<f32>(-1.0, -1.0), vec2<f32>(1.0, 1.0), vec2<f32>(-1.0, 1.0));
    let corner = corners[vi % 6u];
    var clip = node.view_proj * vec4<f32>(in.pos, 1.0);
    let size_px = node.flags.y;
    let ndc_per_px = vec2<f32>(2.0 / node.viewport.x, 2.0 / node.viewport.y);
    clip = vec4<f32>(clip.xy + corner * size_px * 0.5 * ndc_per_px * clip.w, clip.zw);
    var out: PointOut;
    out.clip = clip;
    out.uv = corner;
    out.scalar = in.scalar;
    out.world = in.pos;
    return out;
}

@fragment
fn fs_point(in: PointOut) -> @location(0) vec4<f32> {
    let r2 = dot(in.uv, in.uv);
    if (r2 > 1.0) {
        discard;
    }
    var c = base_color(in.scalar);
    if (node.flags.z > 0.5 && node.flags.x > 0.5) {
        let z = sqrt(max(0.0, 1.0 - r2));
        let n = vec3<f32>(in.uv.x, in.uv.y, z);
        let l = normalize(vec3<f32>(0.4, 0.5, 1.0));
        let diffuse = max(dot(n, l), 0.0);
        let spec = pow(max(dot(n, normalize(l + vec3<f32>(0.0, 0.0, 1.0))), 0.0), 30.0) * 0.3;
        c = c * (0.3 + 0.7 * diffuse) + vec3<f32>(spec);
    }
    return vec4<f32>(c, node.color.a);
}
