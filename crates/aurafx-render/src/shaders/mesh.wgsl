struct NodeUniform {
    view_proj: mat4x4<f32>,    // includes the node's local origin
    camera_pos: vec4<f32>,     // origin-relative
    light_dir: vec4<f32>,
    viewport: vec4<f32>,       // width, height, 0, 0
    color: vec4<f32>,          // solid color + opacity
    range: vec4<f32>,          // lo, hi, log_scale, use_scalar
    flags: vec4<f32>,          // lighting, point_size_px, spheres, 0
    shadow_vp: mat4x4<f32>,    // light view-projection (node-origin relative)
    light_info: vec4<f32>,     // light count, ambient, shadows on, shadow bias
    light_dirs: array<vec4<f32>, 8>,   // world direction towards the light; w = casts shadows
    light_colors: array<vec4<f32>, 8>, // color * intensity
};

@group(0) @binding(0) var<uniform> node: NodeUniform;
@group(0) @binding(1) var lut: texture_1d<f32>;
@group(0) @binding(2) var lut_sampler: sampler;
@group(0) @binding(3) var shadow_map: texture_depth_2d;
@group(0) @binding(4) var shadow_sampler: sampler_comparison;

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
    @location(3) shadow: vec4<f32>,
};

/// Fraction of light reaching a point (3x3 percentage-closer filtering of the shadow map).
fn lit(shadow: vec4<f32>) -> f32 {
    if (node.light_info.z < 0.5 || shadow.w <= 0.0) {
        return 1.0;
    }
    let ndc = shadow.xyz / shadow.w;
    let uv = vec2<f32>(ndc.x * 0.5 + 0.5, 0.5 - ndc.y * 0.5);
    if (uv.x < 0.0 || uv.x > 1.0 || uv.y < 0.0 || uv.y > 1.0 || ndc.z > 1.0) {
        return 1.0;
    }
    let texel = 1.0 / vec2<f32>(textureDimensions(shadow_map));
    var sum = 0.0;
    for (var y = -1; y <= 1; y = y + 1) {
        for (var x = -1; x <= 1; x = x + 1) {
            sum = sum + textureSampleCompareLevel(shadow_map, shadow_sampler, uv + vec2<f32>(f32(x), f32(y)) * texel, ndc.z - node.light_info.w);
        }
    }
    return sum / 9.0;
}

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

fn shade(color: vec3<f32>, n_in: vec3<f32>, world: vec3<f32>, shadow: vec4<f32>) -> vec3<f32> {
    if (node.flags.x < 0.5) {
        return color;
    }
    var n = normalize(n_in);
    let view_dir = normalize(node.camera_pos.xyz - world);
    if (dot(n, view_dir) < 0.0) {
        n = -n;
    }
    var light = vec3<f32>(node.light_info.y);
    var spec = vec3<f32>(0.0);
    let count = u32(node.light_info.x);
    for (var k = 0u; k < count; k = k + 1u) {
        let l = normalize(node.light_dirs[k].xyz);
        var vis = 1.0;
        if (node.light_dirs[k].w > 0.5) {
            vis = lit(shadow);
        }
        let c = node.light_colors[k].rgb;
        light = light + c * max(dot(n, l), 0.0) * vis;
        let h = normalize(l + view_dir);
        spec = spec + c * pow(max(dot(n, h), 0.0), 60.0) * 0.12 * vis;
    }
    return color * light + spec;
}

// ---- weighted blended order-independent transparency (McGuire & Bavoil 2013) ----
struct OitOut {
    @location(0) accum: vec4<f32>,
    @location(1) reveal: vec4<f32>,
};

fn oit(c: vec3<f32>, a: f32, z: f32) -> OitOut {
    let w = clamp(pow(min(1.0, a * 10.0) + 0.01, 3.0) * 1e8 * pow(1.0 - z * 0.9, 3.0), 1e-2, 3e3);
    var o: OitOut;
    o.accum = vec4<f32>(c * a, a) * w;
    o.reveal = vec4<f32>(a);
    return o;
}

@vertex
fn vs_mesh(in: VertexIn) -> VertexOut {
    var out: VertexOut;
    out.clip = node.view_proj * vec4<f32>(in.pos, 1.0);
    out.world = in.pos;
    out.normal = in.normal;
    out.scalar = in.scalar;
    out.shadow = node.shadow_vp * vec4<f32>(in.pos, 1.0);
    return out;
}

// Depth from the shadow-casting light.
@vertex
fn vs_shadow(in: VertexIn) -> @builtin(position) vec4<f32> {
    return node.shadow_vp * vec4<f32>(in.pos, 1.0);
}

@fragment
fn fs_mesh(in: VertexOut) -> @location(0) vec4<f32> {
    let c = shade(base_color(in.scalar), in.normal, in.world, in.shadow);
    return vec4<f32>(c, node.color.a);
}

@fragment
fn fs_line(in: VertexOut) -> @location(0) vec4<f32> {
    return vec4<f32>(base_color(in.scalar), node.color.a);
}

@fragment
fn fs_mesh_oit(in: VertexOut) -> OitOut {
    return oit(shade(base_color(in.scalar), in.normal, in.world, in.shadow), node.color.a, in.clip.z);
}

@fragment
fn fs_line_oit(in: VertexOut) -> OitOut {
    return oit(base_color(in.scalar), node.color.a, in.clip.z);
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

fn point_color(in: PointOut) -> vec3<f32> {
    let r2 = dot(in.uv, in.uv);
    var c = base_color(in.scalar);
    if (node.flags.z > 0.5 && node.flags.x > 0.5) {
        let z = sqrt(max(0.0, 1.0 - r2));
        let n = vec3<f32>(in.uv.x, in.uv.y, z);
        let l = normalize(vec3<f32>(0.4, 0.5, 1.0));
        let diffuse = max(dot(n, l), 0.0);
        let spec = pow(max(dot(n, normalize(l + vec3<f32>(0.0, 0.0, 1.0))), 0.0), 30.0) * 0.3;
        c = c * (0.3 + 0.7 * diffuse) + vec3<f32>(spec);
    }
    return c;
}

@fragment
fn fs_point(in: PointOut) -> @location(0) vec4<f32> {
    if (dot(in.uv, in.uv) > 1.0) {
        discard;
    }
    return vec4<f32>(point_color(in), node.color.a);
}

@fragment
fn fs_point_oit(in: PointOut) -> OitOut {
    if (dot(in.uv, in.uv) > 1.0) {
        discard;
    }
    return oit(point_color(in), node.color.a, in.clip.z);
}
