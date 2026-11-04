// Cell-id rendering for hardware (visible-only) selection.
struct NodeUniform {
    view_proj: mat4x4<f32>,
    camera_pos: vec4<f32>,
    light_dir: vec4<f32>,
    viewport: vec4<f32>,
    color: vec4<f32>,
    range: vec4<f32>,
    // lighting, point size, spheres, 1 = this node is the selection target
    flags: vec4<f32>,
};

@group(0) @binding(0) var<uniform> node: NodeUniform;

struct VIn {
    @location(0) pos: vec3<f32>,
    @location(3) id: u32,
};

struct VOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) @interpolate(flat) id: u32,
};

@vertex
fn vs_id(in: VIn) -> VOut {
    var o: VOut;
    o.clip = node.view_proj * vec4<f32>(in.pos, 1.0);
    o.id = in.id;
    return o;
}

@fragment
fn fs_id(in: VOut) -> @location(0) u32 {
    if (node.flags.w < 0.5) {
        return 0u;
    }
    return in.id + 1u;
}
