// Progressive path tracer (one sample per pixel per dispatch): the GPU twin of raytrace.rs.

// Threaded BVH: interior nodes continue with their first child on a hit; any node continues
// with `escape` on a miss (and leaves after testing their triangles). No stack needed.
struct Node {
    min: vec3<f32>,
    escape: u32,
    max: vec3<f32>,
    // Leaf: first triangle | count << 28. Interior: first child (count 0).
    first_count: u32,
};

// Corner positions (intersection reads only these).
struct TriPos {
    p0: vec4<f32>,
    e1: vec4<f32>,
    e2: vec4<f32>,
};

struct Tri {
    n0: vec4<f32>,
    n1: vec4<f32>,
    n2: vec4<f32>,
    // scalars at the corners, material index (bits)
    s: vec4<f32>,
};

struct Material {
    solid: vec4<f32>,   // rgb, lit
    range: vec4<f32>,   // lo, hi, log, use scalars
    mat: vec4<f32>,     // metallic, roughness, specular, highlight power
};

struct Params {
    eye: vec4<f32>,
    fwd: vec4<f32>,
    right: vec4<f32>,
    up: vec4<f32>,
    view: vec4<f32>,        // width, height, half height, orthographic
    frame: vec4<f32>,       // pass, bounces, eps, light count
    sky: vec4<f32>,         // ambient, environment on, intensity, show environment
    background: vec4<f32>,
    view_proj: mat4x4<f32>, // scene-relative, for the depth of first hits
    light_dirs: array<vec4<f32>, 8>,
    light_colors: array<vec4<f32>, 8>,
};

@group(0) @binding(0) var<uniform> P: Params;
@group(0) @binding(1) var<storage, read> nodes: array<Node>;
@group(0) @binding(2) var<storage, read> tris: array<Tri>;
@group(0) @binding(3) var<storage, read> materials: array<Material>;
@group(0) @binding(4) var<storage, read> luts: array<vec4<f32>>;
@group(0) @binding(5) var<storage, read_write> accum: array<vec4<f32>>;
@group(0) @binding(6) var<storage, read_write> depth: array<f32>;
@group(0) @binding(7) var env_map: texture_2d<f32>;
@group(0) @binding(8) var env_sampler: sampler;
@group(0) @binding(9) var<storage, read> tri_pos: array<TriPos>;

var<private> rng_state: u32;

fn pcg(v: u32) -> u32 {
    let state = v * 747796405u + 2891336453u;
    let word = ((state >> ((state >> 28u) + 4u)) ^ state) * 277803737u;
    return (word >> 22u) ^ word;
}

fn rnd() -> f32 {
    rng_state = pcg(rng_state);
    return f32(rng_state >> 8u) / 16777216.0;
}

fn basis(n: vec3<f32>) -> mat2x3<f32> {
    var a = vec3<f32>(1.0, 0.0, 0.0);
    if (abs(n.x) > 0.9) {
        a = vec3<f32>(0.0, 1.0, 0.0);
    }
    let t = normalize(cross(n, a));
    return mat2x3<f32>(t, cross(n, t));
}

fn cosine_dir(n: vec3<f32>) -> vec3<f32> {
    let b = basis(n);
    let u = rnd();
    let v = rnd();
    let r = sqrt(u);
    let phi = 6.2831855 * v;
    return normalize(b[0] * (r * cos(phi)) + b[1] * (r * sin(phi)) + n * sqrt(max(1.0 - u, 0.0)));
}

fn env(d: vec3<f32>) -> vec3<f32> {
    let uv = vec2<f32>(atan2(d.y, d.x) / 6.2831853 + 0.5, acos(clamp(d.z, -1.0, 1.0)) / 3.1415927);
    return textureSampleLevel(env_map, env_sampler, uv, 0.0).rgb * P.sky.z;
}

const END: u32 = 0xffffffffu;

fn hit_box(n: Node, o: vec3<f32>, inv: vec3<f32>, tmax: f32) -> bool {
    let t0 = (n.min - o) * inv;
    let t1 = (n.max - o) * inv;
    let tn = max(max(min(t0.x, t1.x), min(t0.y, t1.y)), min(t0.z, t1.z));
    let tf = min(min(max(t0.x, t1.x), max(t0.y, t1.y)), max(t0.z, t1.z));
    return tf >= max(tn, 0.0) && tn < tmax;
}

struct Hit {
    t: f32,
    tri: u32,
    u: f32,
    v: f32,
};

fn intersect(o: vec3<f32>, d: vec3<f32>, any_hit: bool) -> Hit {
    var best = Hit(3.0e38, END, 0.0, 0.0);
    let safe = select(d, vec3<f32>(1e-30), abs(d) < vec3<f32>(1e-30));
    let inv = 1.0 / safe;
    var node = 0u;
    loop {
        if (node == END) {
            break;
        }
        let n = nodes[node];
        if (!hit_box(n, o, inv, best.t)) {
            node = n.escape;
            continue;
        }
        let count = n.first_count >> 28u;
        if (count == 0u) {
            node = n.first_count;
            continue;
        }
        let first = n.first_count & 0x0fffffffu;
        for (var k = first; k < first + count; k = k + 1u) {
            let tp = tri_pos[k];
            let p = cross(d, tp.e2.xyz);
            let det = dot(tp.e1.xyz, p);
            if (abs(det) < 1e-20) {
                continue;
            }
            let id = 1.0 / det;
            let s = o - tp.p0.xyz;
            let u = dot(s, p) * id;
            if (u < 0.0 || u > 1.0) {
                continue;
            }
            let q = cross(s, tp.e1.xyz);
            let v = dot(d, q) * id;
            if (v < 0.0 || u + v > 1.0) {
                continue;
            }
            let t = dot(tp.e2.xyz, q) * id;
            if (t > 0.0 && t < best.t) {
                best = Hit(t, k, u, v);
                if (any_hit) {
                    return best;
                }
            }
        }
        node = n.escape;
    }
    return best;
}

fn color_of(m: u32, s: f32) -> vec3<f32> {
    let mat = materials[m];
    if (mat.range.w < 0.5) {
        return mat.solid.rgb;
    }
    if (s != s) {
        return vec3<f32>(0.5);
    }
    var lo = mat.range.x;
    var hi = mat.range.y;
    var v = s;
    if (mat.range.z > 0.5) {
        lo = log(max(lo, 1e-30));
        hi = log(max(hi, 1e-30));
        v = log(max(v, 1e-30));
    }
    var t = 0.5;
    if (hi > lo) {
        t = clamp((v - lo) / (hi - lo), 0.0, 1.0);
    }
    let x = t * 255.0;
    let i = min(u32(x), 254u);
    return mix(luts[m * 256u + i].rgb, luts[m * 256u + i + 1u].rgb, x - f32(i));
}

@compute @workgroup_size(8, 8)
fn trace(@builtin(global_invocation_id) gid: vec3<u32>) {
    let w = u32(P.view.x);
    let h = u32(P.view.y);
    if (gid.x >= w || gid.y >= h) {
        return;
    }
    let pix = gid.y * w + gid.x;
    let pass_index = u32(P.frame.x);
    rng_state = pcg(pix * 9781u + pass_index * 6271u + 1u);
    var jx = 0.5;
    var jy = 0.5;
    if (pass_index > 0u) {
        jx = rnd();
        jy = rnd();
    }
    let aspect = P.view.x / max(P.view.y, 1.0);
    let nx = (f32(gid.x) + jx) / P.view.x * 2.0 - 1.0;
    let ny = 1.0 - (f32(gid.y) + jy) / P.view.y * 2.0;
    var o = P.eye.xyz;
    var d = normalize(P.fwd.xyz + P.right.xyz * (nx * P.view.z * aspect) + P.up.xyz * (ny * P.view.z));
    if (P.view.w > 0.5) {
        o = P.eye.xyz + P.right.xyz * (nx * P.view.z * aspect) + P.up.xyz * (ny * P.view.z);
        d = P.fwd.xyz;
    }
    var through = vec3<f32>(1.0);
    var out = vec3<f32>(0.0);
    let bounces = u32(P.frame.y);
    let eps = P.frame.z;
    for (var bounce = 0u; bounce <= bounces; bounce = bounce + 1u) {
        let hit = intersect(o, d, false);
        if (hit.tri == END) {
            var sky = vec3<f32>(P.sky.x);
            if (P.sky.y > 0.5) {
                sky = env(d);
            }
            if (bounce > 0u) {
                out = out + through * min(sky, vec3<f32>(4.0));
            } else if (P.sky.w > 0.5 && P.sky.y > 0.5) {
                out = out + through * sky;
            } else {
                out = out + through * P.background.rgb;
            }
            break;
        }
        let tr = tris[hit.tri];
        if (bounce == 0u && pass_index == 0u) {
            let wp = o + d * hit.t;
            let clip = P.view_proj * vec4<f32>(wp, 1.0);
            depth[pix] = clamp(clip.z / clip.w, 0.0, 0.999999);
        }
        let wgt = 1.0 - hit.u - hit.v;
        let m = bitcast<u32>(tr.s.w);
        let mat = materials[m];
        let albedo = color_of(m, tr.s.x * wgt + tr.s.y * hit.u + tr.s.z * hit.v);
        if (mat.solid.w < 0.5) {
            out = out + through * albedo;
            break;
        }
        let tp = tri_pos[hit.tri];
        var ng = normalize(cross(tp.e1.xyz, tp.e2.xyz));
        if (dot(ng, d) > 0.0) {
            ng = -ng;
        }
        var n = tr.n0.xyz * wgt + tr.n1.xyz * hit.u + tr.n2.xyz * hit.v;
        if (length(n) < 1e-20) {
            n = ng;
        }
        n = normalize(n);
        if (dot(n, ng) < 0.0) {
            n = -n;
        }
        let p = o + d * hit.t + ng * eps;
        let metallic = mat.mat.x;
        let spec_color = mix(vec3<f32>(mat.mat.z), albedo, metallic);
        let count = u32(P.frame.w);
        for (var k = 0u; k < count; k = k + 1u) {
            let l = P.light_dirs[k].xyz;
            let b = basis(l);
            let r = 0.03 * sqrt(rnd());
            let phi = 6.2831855 * rnd();
            let ld = normalize(l + b[0] * (r * cos(phi)) + b[1] * (r * sin(phi)));
            let ndl = dot(n, ld);
            if (ndl <= 0.0 || dot(ng, ld) <= 0.0) {
                continue;
            }
            if (intersect(p, ld, true).tri != END) {
                continue;
            }
            let col = P.light_colors[k].rgb;
            out = out + through * albedo * col * ndl * (1.0 - metallic);
            let hv = normalize(ld - d);
            out = out + through * spec_color * col * pow(max(dot(n, hv), 0.0), mat.mat.w);
        }
        let f0 = mix(vec3<f32>(mat.mat.z / 3.0), albedo, metallic);
        let ws = max(max(f0.x, f0.y), f0.z);
        let wd = 1.0 - metallic;
        var p_spec = 0.0;
        if (ws + wd > 0.0) {
            p_spec = ws / (ws + wd);
        }
        var nd = vec3<f32>(0.0);
        if (rnd() < p_spec) {
            let refl = d - n * (2.0 * dot(d, n));
            let g = mat.mat.y * mat.mat.y;
            nd = normalize(refl + (cosine_dir(n) - n) * g);
            through = through * f0 / p_spec;
        } else {
            nd = cosine_dir(n);
            through = through * albedo * (wd / (1.0 - p_spec));
        }
        if (dot(nd, ng) <= 0.0) {
            break;
        }
        if (bounce >= 2u) {
            let keep = clamp(max(max(through.x, through.y), through.z), 0.05, 1.0);
            if (rnd() > keep) {
                break;
            }
            through = through / keep;
        }
        o = p;
        d = nd;
    }
    accum[pix] = accum[pix] + vec4<f32>(out, 1.0);
}
