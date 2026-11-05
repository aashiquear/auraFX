// Full-screen passes for server-drawn images: resolving the scene depth for read-back, and
// compositing a (color, depth) image into the scene with correct occlusion.

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

@group(0) @binding(0) var scene_depth: texture_depth_multisampled_2d;

@fragment
fn fs_resolve_depth(v: VsOut) -> @location(0) f32 {
    return textureLoad(scene_depth, vec2<i32>(v.pos.xy), 0);
}

struct LayerUniform {
    // image width, height, target width, height
    size: vec4<f32>,
};

@group(0) @binding(0) var layer_color: texture_2d<f32>;
@group(0) @binding(1) var layer_depth: texture_2d<f32>;
@group(0) @binding(2) var<uniform> layer: LayerUniform;

struct LayerOut {
    @location(0) color: vec4<f32>,
    @builtin(frag_depth) depth: f32,
};

@fragment
fn fs_layer(v: VsOut) -> LayerOut {
    let uv = v.pos.xy * layer.size.xy / layer.size.zw;
    let c = vec2<i32>(clamp(uv, vec2<f32>(0.0), layer.size.xy - 1.0));
    let d = textureLoad(layer_depth, c, 0).r;
    if d >= 1.0 {
        discard;
    }
    var o: LayerOut;
    o.color = vec4<f32>(textureLoad(layer_color, c, 0).rgb, 1.0);
    o.depth = d;
    return o;
}
