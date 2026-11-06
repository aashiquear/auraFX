struct NodeUniform {
    view_proj: mat4x4<f32>,    // includes the node's local origin
    camera_pos: vec4<f32>,     // origin-relative
    light_dir: vec4<f32>,
    viewport: vec4<f32>,       // width, height, LIC strength, line width (px)
    color: vec4<f32>,          // solid color + opacity
    range: vec4<f32>,          // lo, hi, log_scale, use_scalar
    flags: vec4<f32>,          // lighting, point_size_px, spheres, 0
    shadow_vp: mat4x4<f32>,    // light view-projection (node-origin relative)
    light_info: vec4<f32>,     // light count, ambient, shadows on, shadow bias
    light_dirs: array<vec4<f32>, 8>,   // world direction towards the light; w = casts shadows
    light_colors: array<vec4<f32>, 8>, // color * intensity
    material: vec4<f32>,       // metallic, roughness, specular, highlight power
    env: vec4<f32>,            // environment on, intensity, mip levels, textured
    sh: array<vec4<f32>, 9>,   // environment irradiance (spherical harmonics, RGB)
};

@group(0) @binding(0) var<uniform> node: NodeUniform;
@group(0) @binding(1) var lut: texture_1d<f32>;
@group(0) @binding(2) var lut_sampler: sampler;
@group(0) @binding(3) var shadow_map: texture_depth_2d;
@group(0) @binding(4) var shadow_sampler: sampler_comparison;
@group(0) @binding(5) var env_map: texture_2d<f32>;
@group(0) @binding(6) var env_sampler: sampler;
@group(0) @binding(7) var surface_tex: texture_2d<f32>;
@group(0) @binding(8) var surface_sampler: sampler;

/// Equirectangular coordinates of a direction (+Z up).
fn env_uv(d: vec3<f32>) -> vec2<f32> {
    return vec2<f32>(atan2(d.y, d.x) / 6.2831853 + 0.5, acos(clamp(d.z, -1.0, 1.0)) / 3.1415927);
}

/// Diffuse light from the environment for a surface facing `n`.
fn env_irradiance(n: vec3<f32>) -> vec3<f32> {
    let b0 = 0.282095;
    var e = node.sh[0].rgb * b0;
    e = e + node.sh[1].rgb * (0.488603 * n.y) + node.sh[2].rgb * (0.488603 * n.z) + node.sh[3].rgb * (0.488603 * n.x);
    e = e + node.sh[4].rgb * (1.092548 * n.x * n.y) + node.sh[5].rgb * (1.092548 * n.y * n.z);
    e = e + node.sh[6].rgb * (0.315392 * (3.0 * n.z * n.z - 1.0)) + node.sh[7].rgb * (1.092548 * n.x * n.z);
    e = e + node.sh[8].rgb * (0.546274 * (n.x * n.x - n.y * n.y));
    return max(e, vec3<f32>(0.0)) * node.env.y;
}

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
    // Texture coordinates (textured surfaces: packed as two halves in the scalar).
    @location(4) uv: vec2<f32>,
    // The vertex's own color (direct colors: packed in the scalar as r * 65536 + g * 256 + b).
    @location(5) rgb: vec3<f32>,
};

fn unpack_rgb(s: f32) -> vec3<f32> {
    let v = u32(clamp(round(s), 0.0, 16777215.0));
    return vec3<f32>(f32((v >> 16u) & 255u), f32((v >> 8u) & 255u), f32(v & 255u)) / 255.0;
}

fn textured() -> bool {
    return node.env.w > 0.5 && node.env.w < 1.5;
}

fn direct_rgb() -> bool {
    return node.env.w > 1.5;
}

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
    if (is_nan(s)) {
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

/// The image on a textured surface (t = 0 is the image's bottom row, as in VTK), else the
/// scalar or solid color.
fn surface_color(s: f32, uv: vec2<f32>, rgb: vec3<f32>) -> vec3<f32> {
    let c = textureSample(surface_tex, surface_sampler, vec2<f32>(uv.x, 1.0 - uv.y)).rgb;
    if (textured()) {
        return c;
    }
    if (direct_rgb()) {
        return srgb_to_linear3(rgb);
    }
    return base_color(s);
}

fn srgb_to_linear3(c: vec3<f32>) -> vec3<f32> {
    return select(pow((c + 0.055) / 1.055, vec3<f32>(2.4)), c / 12.92, c <= vec3<f32>(0.04045));
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
    let metallic = node.material.x;
    let power = node.material.w;
    // Metals reflect in their own color and at full strength, and have no diffuse light.
    let spec_color = mix(vec3<f32>(node.material.z), color, metallic);
    var light = vec3<f32>(node.light_info.y);
    if (node.env.x > 0.5) {
        light = env_irradiance(n);
    }
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
        spec = spec + c * pow(max(dot(n, h), 0.0), power) * vis;
    }
    var out = color * light * (1.0 - metallic) + spec * spec_color;
    if (node.env.x > 0.5) {
        // Reflection of the environment, blurrier for rougher surfaces (Schlick's Fresnel).
        let r = reflect(-view_dir, n);
        let rough = node.material.y;
        let reflected = textureSampleLevel(env_map, env_sampler, env_uv(r), rough * node.env.z).rgb * node.env.y;
        let f0 = mix(vec3<f32>(node.material.z / 3.0), color, metallic);
        let fresnel = f0 + (vec3<f32>(1.0) - f0) * pow(1.0 - max(dot(n, view_dir), 0.0), 5.0) * (1.0 - rough);
        out = out + reflected * fresnel * (1.0 - 0.6 * rough);
    }
    return out;
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
    out.uv = vec2<f32>(0.0);
    out.rgb = vec3<f32>(0.0);
    if (textured()) {
        out.uv = unpack2x16float(bitcast<u32>(in.scalar));
    }
    if (direct_rgb()) {
        out.rgb = unpack_rgb(in.scalar);
    }
    return out;
}

// Depth from the shadow-casting light.
@vertex
fn vs_shadow(in: VertexIn) -> @builtin(position) vec4<f32> {
    return node.shadow_vp * vec4<f32>(in.pos, 1.0);
}

@fragment
fn fs_mesh(in: VertexOut) -> @location(0) vec4<f32> {
    let c = shade(surface_color(in.scalar, in.uv, in.rgb), in.normal, in.world, in.shadow);
    return vec4<f32>(c, node.color.a);
}

@fragment
fn fs_line(in: VertexOut) -> @location(0) vec4<f32> {
    return vec4<f32>(surface_color(in.scalar, in.uv, in.rgb), node.color.a);
}

@fragment
fn fs_mesh_oit(in: VertexOut) -> OitOut {
    return oit(shade(surface_color(in.scalar, in.uv, in.rgb), in.normal, in.world, in.shadow), node.color.a, in.clip.z);
}

@fragment
fn fs_line_oit(in: VertexOut) -> OitOut {
    return oit(surface_color(in.scalar, in.uv, in.rgb), node.color.a, in.clip.z);
}

// ---- wide lines: one instance per segment (its two vertices), drawn as a screen-space quad
// of node.viewport.w pixels, extended by half the width at each end so polylines join ----
struct SegmentIn {
    @location(0) pos0: vec3<f32>,
    @location(1) scalar0: f32,
    @location(2) normal0: vec3<f32>,
    @location(3) pos1: vec3<f32>,
    @location(4) scalar1: f32,
    @location(5) normal1: vec3<f32>,
};

@vertex
fn vs_wide_line(@builtin(vertex_index) vi: u32, in: SegmentIn) -> VertexOut {
    // (end, side) of the six corners of the quad.
    var ends = array<f32, 6>(0.0, 1.0, 1.0, 0.0, 1.0, 0.0);
    var sides = array<f32, 6>(-1.0, -1.0, 1.0, -1.0, 1.0, 1.0);
    let t = ends[vi % 6u];
    let side = sides[vi % 6u];
    var a = node.view_proj * vec4<f32>(in.pos0, 1.0);
    var b = node.view_proj * vec4<f32>(in.pos1, 1.0);
    // Clip the segment to the space in front of the camera before going to pixels.
    let near = 1e-5;
    var ta = 0.0;
    var tb = 1.0;
    if (a.w < near && b.w >= near) {
        ta = (near - a.w) / (b.w - a.w);
    }
    if (b.w < near && a.w >= near) {
        tb = (near - a.w) / (b.w - a.w);
    }
    let a2 = mix(a, b, ta);
    let b2 = mix(a, b, tb);
    let half_px = node.viewport.xy * 0.5;
    let sa = a2.xy / a2.w * half_px;
    let sb = b2.xy / b2.w * half_px;
    var d = sb - sa;
    if (length(d) < 1e-6) {
        d = vec2<f32>(1.0, 0.0);
    }
    d = normalize(d);
    let n = vec2<f32>(-d.y, d.x);
    let w = node.viewport.w * 0.5;
    let f = mix(ta, tb, t);
    var clip = mix(a2, b2, t);
    let offset_px = n * side * w + d * (2.0 * t - 1.0) * w;
    clip = vec4<f32>(clip.xy + offset_px / half_px * clip.w, clip.zw);
    if (a.w < near && b.w < near) {
        clip = vec4<f32>(0.0, 0.0, -1.0, 1.0); // behind the camera: nothing drawn
    }
    let pos = mix(in.pos0, in.pos1, f);
    var out: VertexOut;
    out.clip = clip;
    out.world = pos;
    out.normal = select(in.normal0, in.normal1, t > 0.5);
    out.scalar = select(in.scalar0, in.scalar1, t > 0.5);
    out.shadow = node.shadow_vp * vec4<f32>(pos, 1.0);
    out.uv = vec2<f32>(0.0);
    out.rgb = vec3<f32>(0.0);
    if (textured()) {
        out.uv = unpack2x16float(bitcast<u32>(out.scalar));
    }
    if (direct_rgb()) {
        out.rgb = unpack_rgb(out.scalar);
    }
    return out;
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
    if (direct_rgb()) {
        c = srgb_to_linear3(unpack_rgb(in.scalar));
    }
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

// ---- Surface LIC: screen-space direction of the vector field ----
struct LicOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) dir: vec2<f32>,
};

@vertex
fn vs_lic(in: VertexIn, @location(3) v: vec4<f32>) -> LicOut {
    var out: LicOut;
    let c = node.view_proj * vec4<f32>(in.pos, 1.0);
    out.clip = c;
    // Tangential part of the vector, projected to pixels (derivative of the projection).
    var t = v.xyz;
    let nl = length(in.normal);
    if (nl > 0.0) {
        let n = in.normal / nl;
        t = t - n * dot(n, t);
    }
    let dc = node.view_proj * vec4<f32>(t, 0.0);
    let d = (dc.xy * c.w - c.xy * dc.w) / (c.w * c.w);
    out.dir = vec2<f32>(d.x * node.viewport.x, -d.y * node.viewport.y);
    return out;
}

// x, y: unit direction in pixels; z: depth; w: 1 + strength = vector, 0.5 = surface without one.
@fragment
fn fs_lic_vec(in: LicOut) -> @location(0) vec4<f32> {
    let l = length(in.dir);
    if (l < 1e-20) {
        return vec4<f32>(0.0, 0.0, in.clip.z, 0.5);
    }
    return vec4<f32>(in.dir / l, in.clip.z, 1.0 + node.viewport.z);
}

/// NaN by bit pattern (`x != x` may be optimized away by GPU compilers).
fn is_nan(x: f32) -> bool {
    let b = bitcast<u32>(x);
    return (b & 0x7f800000u) == 0x7f800000u && (b & 0x007fffffu) != 0u;
}
