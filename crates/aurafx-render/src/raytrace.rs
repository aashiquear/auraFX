//! A progressive CPU path tracer for the opaque surfaces of a view.
//!
//! Each pass adds one jittered sample per pixel: direct light from the view's lights with soft
//! shadows, a uniform sky (the ambient level) and diffuse interreflections. The result is a
//! color + depth picture that the renderer composites like a server picture, so points, lines,
//! translucent objects and overlays are still drawn on top by the rasterizer.

use crate::{Camera, ImageLayer, NodeDraw};
use aurafx_pipeline::{Lighting, Representation};
use glam::{DVec3, DVec4, Vec3};
use rayon::prelude::*;

/// Whether a draw is traced (opaque surfaces; the rest stays with the rasterizer).
pub fn traced(d: &NodeDraw) -> bool {
    !d.overlay
        && d.mesh.volume.is_none()
        && d.display.opacity >= 1.0
        && matches!(d.display.representation, Representation::Surface | Representation::SurfaceWithEdges)
        && !d.mesh.tris.is_empty()
}

/// How an object is colored: its color map (linear) and range, or its solid color.
pub(crate) struct Material {
    pub(crate) lut: Vec<Vec3>,
    pub(crate) range: (f32, f32),
    pub(crate) log: bool,
    pub(crate) scalars: bool,
    pub(crate) solid: Vec3,
    pub(crate) lit: bool,
    pub(crate) metallic: f32,
    pub(crate) roughness: f32,
    pub(crate) specular: f32,
    pub(crate) power: f32,
}

impl Material {
    pub(crate) fn of(d: &NodeDraw) -> Material {
        let cmap = d.display.effective_colormap();
        let (lo, hi) = d.mesh.scalar_range.unwrap_or((0.0, 1.0));
        Material {
            lut: (0..256)
                .map(|i| {
                    let c = cmap.sample(i as f32 / 255.0);
                    Vec3::new(srgb_to_linear(c[0]), srgb_to_linear(c[1]), srgb_to_linear(c[2]))
                })
                .collect(),
            range: (lo as f32, hi as f32),
            log: d.display.log_scale,
            scalars: d.mesh.has_scalars,
            solid: Vec3::from_array(d.display.solid_color),
            lit: d.display.lighting,
            metallic: d.display.metallic.clamp(0.0, 1.0),
            roughness: d.display.roughness.clamp(0.0, 1.0),
            specular: d.display.specular.max(0.0),
            power: aurafx_pipeline::display::specular_power(d.display.roughness),
        }
    }

    /// Linear color for a scalar value (as the shader maps it).
    pub(crate) fn color(&self, s: f32) -> Vec3 {
        if !self.scalars {
            return self.solid;
        }
        if s.is_nan() {
            return Vec3::splat(0.5);
        }
        let (mut lo, mut hi, mut v) = (self.range.0, self.range.1, s);
        if self.log {
            lo = lo.max(1e-30).ln();
            hi = hi.max(1e-30).ln();
            v = v.max(1e-30).ln();
        }
        let t = if hi > lo { ((v - lo) / (hi - lo)).clamp(0.0, 1.0) } else { 0.5 };
        let x = t * 255.0;
        let i = (x as usize).min(254);
        self.lut[i].lerp(self.lut[i + 1], x - i as f32)
    }
}

#[derive(Clone, Copy, Default)]
pub(crate) struct BvhNode {
    pub(crate) min: Vec3,
    pub(crate) max: Vec3,
    /// Interior: index of the first child (the second follows). Leaf: first triangle in `order`.
    pub(crate) first: u32,
    /// Triangles in a leaf; 0 for an interior node.
    pub(crate) count: u32,
}

/// Triangles of the traced objects with a bounding volume hierarchy.
pub struct Scene {
    /// World position = scene position + origin.
    pub(crate) origin: DVec3,
    pub(crate) radius: f32,
    /// Three corners per triangle.
    pub(crate) pos: Vec<Vec3>,
    pub(crate) normal: Vec<Vec3>,
    pub(crate) scalar: Vec<f32>,
    pub(crate) material: Vec<u16>,
    pub(crate) materials: Vec<Material>,
    pub(crate) nodes: Vec<BvhNode>,
    pub(crate) order: Vec<u32>,
}

fn srgb_to_linear(c: f32) -> f32 {
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

fn linear_to_srgb(c: f32) -> f32 {
    let c = c.clamp(0.0, 1.0);
    if c <= 0.0031308 {
        c * 12.92
    } else {
        1.055 * c.powf(1.0 / 2.4) - 0.055
    }
}

impl Scene {
    /// The traced draws (see [`traced`]) as one scene.
    pub fn build(draws: &[NodeDraw]) -> Scene {
        let draws: Vec<&NodeDraw> = draws.iter().filter(|d| traced(d)).collect();
        let bounds = draws.iter().fold(aurafx_data::Bounds::EMPTY, |b, d| b.union(&d.mesh.bounds));
        let origin = if bounds.is_empty() { DVec3::ZERO } else { DVec3::from_array(bounds.center()) };
        let radius = (bounds.diagonal() * 0.5).max(1e-9) as f32;
        let n: usize = draws.iter().map(|d| d.mesh.tris.len()).sum();
        let (mut pos, mut normal, mut scalar, mut material) = (Vec::with_capacity(n), Vec::with_capacity(n), Vec::with_capacity(n), Vec::with_capacity(n / 3));
        let mut materials = Vec::new();
        for d in draws.iter().take(u16::MAX as usize) {
            let shift = (DVec3::from_array(d.mesh.origin) - origin).as_vec3();
            let m = materials.len() as u16;
            materials.push(Material::of(d));
            let tris = &d.mesh.tris[..d.mesh.tris.len() / 3 * 3];
            for v in tris {
                pos.push(Vec3::from_array(v.pos) + shift);
                normal.push(Vec3::from_array(v.normal));
                scalar.push(v.scalar);
            }
            material.extend(std::iter::repeat_n(m, tris.len() / 3));
        }
        let mut scene = Scene { origin, radius, pos, normal, scalar, material, materials, nodes: Vec::new(), order: Vec::new() };
        scene.build_bvh();
        scene
    }

    pub fn triangles(&self) -> usize {
        self.material.len()
    }

    fn build_bvh(&mut self) {
        let n = self.material.len();
        self.order = (0..n as u32).collect();
        if n == 0 {
            return;
        }
        let (bmin, bmax, cent): (Vec<Vec3>, Vec<Vec3>, Vec<Vec3>) = {
            let v: Vec<(Vec3, Vec3, Vec3)> = (0..n)
                .into_par_iter()
                .map(|t| {
                    let (a, b, c) = (self.pos[3 * t], self.pos[3 * t + 1], self.pos[3 * t + 2]);
                    let lo = a.min(b).min(c);
                    let hi = a.max(b).max(c);
                    (lo, hi, (lo + hi) * 0.5)
                })
                .collect();
            let mut x = (Vec::with_capacity(n), Vec::with_capacity(n), Vec::with_capacity(n));
            for (a, b, c) in v {
                x.0.push(a);
                x.1.push(b);
                x.2.push(c);
            }
            x
        };
        self.nodes.reserve(2 * n / 2 + 1);
        self.nodes.push(BvhNode { first: 0, count: n as u32, ..Default::default() });
        // (node, start, end) still to split.
        let mut todo = vec![(0usize, 0usize, n)];
        while let Some((ni, start, end)) = todo.pop() {
            let ids = &mut self.order[start..end];
            let (mut lo, mut hi) = (Vec3::splat(f32::INFINITY), Vec3::splat(f32::NEG_INFINITY));
            let (mut clo, mut chi) = (lo, hi);
            for &t in ids.iter() {
                lo = lo.min(bmin[t as usize]);
                hi = hi.max(bmax[t as usize]);
                clo = clo.min(cent[t as usize]);
                chi = chi.max(cent[t as usize]);
            }
            self.nodes[ni].min = lo;
            self.nodes[ni].max = hi;
            let count = end - start;
            if count <= 4 {
                self.nodes[ni].first = start as u32;
                self.nodes[ni].count = count as u32;
                continue;
            }
            // Binned surface area heuristic over the longest centroid axis.
            let ext = chi - clo;
            let axis = if ext.x >= ext.y && ext.x >= ext.z {
                0
            } else if ext.y >= ext.z {
                1
            } else {
                2
            };
            let mut mid = start + count / 2;
            if ext[axis] > 0.0 {
                const BINS: usize = 16;
                let mut bins = [(Vec3::splat(f32::INFINITY), Vec3::splat(f32::NEG_INFINITY), 0usize); BINS];
                let scale = BINS as f32 / ext[axis];
                let bin_of = |t: u32| (((cent[t as usize][axis] - clo[axis]) * scale) as usize).min(BINS - 1);
                for &t in ids.iter() {
                    let b = &mut bins[bin_of(t)];
                    b.0 = b.0.min(bmin[t as usize]);
                    b.1 = b.1.max(bmax[t as usize]);
                    b.2 += 1;
                }
                let area = |lo: Vec3, hi: Vec3| {
                    let d = (hi - lo).max(Vec3::ZERO);
                    d.x * d.y + d.y * d.z + d.z * d.x
                };
                let mut best = (f32::INFINITY, 0usize);
                for split in 1..BINS {
                    let (mut l0, mut l1, mut ln) = (Vec3::splat(f32::INFINITY), Vec3::splat(f32::NEG_INFINITY), 0);
                    for b in &bins[..split] {
                        l0 = l0.min(b.0);
                        l1 = l1.max(b.1);
                        ln += b.2;
                    }
                    let (mut r0, mut r1, mut rn) = (Vec3::splat(f32::INFINITY), Vec3::splat(f32::NEG_INFINITY), 0);
                    for b in &bins[split..] {
                        r0 = r0.min(b.0);
                        r1 = r1.max(b.1);
                        rn += b.2;
                    }
                    if ln == 0 || rn == 0 {
                        continue;
                    }
                    let cost = area(l0, l1) * ln as f32 + area(r0, r1) * rn as f32;
                    if cost < best.0 {
                        best = (cost, split);
                    }
                }
                if best.0.is_finite() {
                    let mut i = 0;
                    let mut j = ids.len();
                    while i < j {
                        if bin_of(ids[i]) < best.1 {
                            i += 1;
                        } else {
                            j -= 1;
                            ids.swap(i, j);
                        }
                    }
                    mid = start + i;
                } else {
                    ids.sort_unstable_by(|a, b| cent[*a as usize][axis].total_cmp(&cent[*b as usize][axis]));
                }
            }
            if mid == start || mid == end {
                mid = start + count / 2;
            }
            let left = self.nodes.len();
            self.nodes.push(BvhNode::default());
            self.nodes.push(BvhNode::default());
            self.nodes[ni].first = left as u32;
            self.nodes[ni].count = 0;
            todo.push((left, start, mid));
            todo.push((left + 1, mid, end));
        }
    }

    fn hit_box(n: &BvhNode, o: Vec3, inv: Vec3, tmax: f32) -> f32 {
        let t0 = (n.min - o) * inv;
        let t1 = (n.max - o) * inv;
        let tn = t0.min(t1).max_element();
        let tf = t0.max(t1).min_element();
        if tf >= tn.max(0.0) && tn < tmax {
            tn.max(0.0)
        } else {
            f32::INFINITY
        }
    }

    /// Möller–Trumbore: (t, u, v).
    fn hit_tri(&self, t: usize, o: Vec3, d: Vec3) -> Option<(f32, f32, f32)> {
        let (a, b, c) = (self.pos[3 * t], self.pos[3 * t + 1], self.pos[3 * t + 2]);
        let e1 = b - a;
        let e2 = c - a;
        let p = d.cross(e2);
        let det = e1.dot(p);
        if det.abs() < 1e-20 {
            return None;
        }
        let inv = 1.0 / det;
        let s = o - a;
        let u = s.dot(p) * inv;
        if !(0.0..=1.0).contains(&u) {
            return None;
        }
        let q = s.cross(e1);
        let v = d.dot(q) * inv;
        if v < 0.0 || u + v > 1.0 {
            return None;
        }
        let tt = e2.dot(q) * inv;
        (tt > 0.0).then_some((tt, u, v))
    }

    /// Nearest hit: (distance, triangle, u, v); with `any`, the first hit found.
    fn intersect(&self, o: Vec3, d: Vec3, tmax: f32, any: bool) -> Option<(f32, usize, f32, f32)> {
        if self.nodes.is_empty() {
            return None;
        }
        let inv = Vec3::ONE / d;
        let mut best: Option<(f32, usize, f32, f32)> = None;
        let mut limit = tmax;
        let mut stack = [0u32; 64];
        let mut sp = 0usize;
        if Self::hit_box(&self.nodes[0], o, inv, limit).is_infinite() {
            return None;
        }
        let mut node = 0usize;
        loop {
            let n = &self.nodes[node];
            if n.count > 0 {
                for &t in &self.order[n.first as usize..(n.first + n.count) as usize] {
                    if let Some((tt, u, v)) = self.hit_tri(t as usize, o, d) {
                        if tt < limit {
                            limit = tt;
                            best = Some((tt, t as usize, u, v));
                            if any {
                                return best;
                            }
                        }
                    }
                }
            } else {
                let (l, r) = (n.first as usize, n.first as usize + 1);
                let (tl, tr) = (Self::hit_box(&self.nodes[l], o, inv, limit), Self::hit_box(&self.nodes[r], o, inv, limit));
                let (near, far, tn, tf) = if tl <= tr { (l, r, tl, tr) } else { (r, l, tr, tl) };
                if tn.is_finite() {
                    if tf.is_finite() && sp < stack.len() {
                        stack[sp] = far as u32;
                        sp += 1;
                    }
                    node = near;
                    continue;
                }
            }
            // Pop the next node that can still be closer than the best hit.
            loop {
                if sp == 0 {
                    return best;
                }
                sp -= 1;
                let c = stack[sp] as usize;
                if Self::hit_box(&self.nodes[c], o, inv, limit).is_finite() {
                    node = c;
                    break;
                }
            }
        }
    }
}

/// Small fast random numbers (PCG) per pixel and pass.
struct Rng(u64);

impl Rng {
    fn new(seed: u64) -> Self {
        let mut r = Rng(seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407));
        r.next();
        r
    }
    fn next(&mut self) -> f32 {
        let old = self.0;
        self.0 = old.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        let x = (((old >> 18) ^ old) >> 27) as u32;
        let r = (old >> 59) as u32;
        (x.rotate_right(r) >> 8) as f32 / (1u32 << 24) as f32
    }
}

fn basis(n: Vec3) -> (Vec3, Vec3) {
    let a = if n.x.abs() > 0.9 { Vec3::Y } else { Vec3::X };
    let t = n.cross(a).normalize();
    (t, n.cross(t))
}

/// A direction around `n`, cosine-weighted.
fn cosine_dir(n: Vec3, rng: &mut Rng) -> Vec3 {
    let (t, b) = basis(n);
    let (u, v) = (rng.next(), rng.next());
    let r = u.sqrt();
    let phi = 6.283_185_5 * v;
    (t * (r * phi.cos()) + b * (r * phi.sin()) + n * (1.0 - u).max(0.0).sqrt()).normalize()
}

/// What a frame traces: the camera, the lights and the picture size.
#[derive(Clone)]
pub struct View {
    pub camera: Camera,
    pub lighting: Lighting,
    pub width: u32,
    pub height: u32,
    /// Linear background color (for anti-aliased silhouettes).
    pub background: [f32; 3],
    /// Diffuse bounces after the first hit.
    pub bounces: u32,
    /// The environment lighting the scene (see `Lighting::environment`).
    pub environment: Option<std::sync::Arc<crate::env::Environment>>,
}

/// Camera rays and lights of a view, in scene coordinates.
pub(crate) struct CameraFrame {
    pub(crate) eye: Vec3,
    pub(crate) fwd: Vec3,
    pub(crate) right: Vec3,
    pub(crate) up: Vec3,
    pub(crate) half_h: f32,
    /// (direction towards the light, color * intensity)
    pub(crate) lights: Vec<(Vec3, Vec3)>,
    pub(crate) ambient: f32,
}

impl CameraFrame {
    pub(crate) fn new(view: &View, origin: DVec3) -> Self {
        let c = &view.camera;
        let f = (c.target - c.eye).normalize_or_zero();
        let s = f.cross(c.up).normalize_or_zero();
        let u = s.cross(f);
        let (lights, ambient) = view.lighting.effective_lights();
        let lights = lights
            .iter()
            .map(|l| {
                let d = DVec3::new(l.direction[0] as f64, l.direction[1] as f64, l.direction[2] as f64);
                // Camera-relative: x right, y up, z towards the viewer.
                let w = if l.camera_relative { s * d.x + u * d.y - f * d.z } else { d };
                (w.normalize_or_zero().as_vec3(), Vec3::from_array(l.color) * l.intensity)
            })
            .collect();
        let half_h = if c.orthographic { c.ortho_height } else { (c.fov_y_deg.to_radians() * 0.5).tan() } as f32;
        CameraFrame { eye: (c.eye - origin).as_vec3(), fwd: f.as_vec3(), right: s.as_vec3(), up: u.as_vec3(), half_h, lights, ambient }
    }
}

/// Push surfaces back by their depth slope per pixel, like the rasterizer's polygon offset, so
/// edges and lines drawn on them by the rasterizer stay in front.
pub(crate) fn slope_bias(d: &[f32], w: usize, h: usize) -> Vec<f32> {
    let at = |x: usize, y: usize| d[y * w + x];
    // The smaller one-sided difference (silhouettes don't count).
    let slope = |c: f32, a: Option<f32>, b: Option<f32>| [a, b].into_iter().flatten().filter(|v| *v < 1.0).map(|v| (v - c).abs()).fold(f32::INFINITY, f32::min);
    (0..w * h)
        .map(|i| {
            let (x, y) = (i % w, i / w);
            let c = d[i];
            if c >= 1.0 {
                return c;
            }
            let sx = slope(c, (x > 0).then(|| at(x - 1, y)), (x + 1 < w).then(|| at(x + 1, y)));
            let sy = slope(c, (y > 0).then(|| at(x, y - 1)), (y + 1 < h).then(|| at(x, y + 1)));
            let s = [sx, sy].into_iter().filter(|v| v.is_finite()).fold(0.0f32, f32::max);
            (c + s + 2.0 * f32::EPSILON).min(0.999_999)
        })
        .collect()
}

/// sRGB-encoded RGBA8 of summed linear colors times `k`.
pub(crate) fn encode(sum: impl Iterator<Item = Vec3>, k: f32) -> Vec<u8> {
    let mut color = Vec::new();
    for c in sum {
        let c = c * k;
        color.extend_from_slice(&[
            (linear_to_srgb(c.x) * 255.0 + 0.5) as u8,
            (linear_to_srgb(c.y) * 255.0 + 0.5) as u8,
            (linear_to_srgb(c.z) * 255.0 + 0.5) as u8,
            255,
        ]);
    }
    color
}

/// Accumulates passes for one view of one scene.
pub struct Tracer {
    scene: std::sync::Arc<Scene>,
    view: View,
    lights: Vec<(Vec3, Vec3)>,
    ambient: f32,
    // Camera frame in scene coordinates.
    eye: Vec3,
    fwd: Vec3,
    right: Vec3,
    up: Vec3,
    half_h: f32,
    accum: Vec<Vec3>,
    depth: Vec<f32>,
    pub passes: u32,
}

impl Tracer {
    pub fn new(scene: std::sync::Arc<Scene>, view: View) -> Self {
        let fr = CameraFrame::new(&view, scene.origin);
        let n = (view.width * view.height) as usize;
        Self {
            eye: fr.eye,
            fwd: fr.fwd,
            right: fr.right,
            up: fr.up,
            half_h: fr.half_h,
            scene,
            lights: fr.lights,
            ambient: fr.ambient,
            accum: vec![Vec3::ZERO; n],
            depth: vec![1.0; n],
            passes: 0,
            view,
        }
    }

    fn ray(&self, x: f32, y: f32) -> (Vec3, Vec3) {
        let (w, h) = (self.view.width as f32, self.view.height as f32);
        let aspect = w / h.max(1.0);
        let nx = x / w * 2.0 - 1.0;
        let ny = 1.0 - y / h * 2.0;
        if self.view.camera.orthographic {
            (self.eye + self.right * (nx * self.half_h * aspect) + self.up * (ny * self.half_h), self.fwd)
        } else {
            (self.eye, (self.fwd + self.right * (nx * self.half_h * aspect) + self.up * (ny * self.half_h)).normalize())
        }
    }

    /// Radiance along a camera ray, and the distance to the first hit.
    fn radiance(&self, mut o: Vec3, mut d: Vec3, rng: &mut Rng) -> (Vec3, Option<f32>) {
        let sc = &self.scene;
        let eps = 2e-5 * sc.radius;
        let mut through = Vec3::ONE;
        let mut out = Vec3::ZERO;
        let mut first = None;
        for bounce in 0..=self.view.bounces {
            let Some((t, tri, u, v)) = sc.intersect(o, d, f32::INFINITY, false) else {
                let lighting = &self.view.lighting;
                let sky = || match &self.view.environment {
                    Some(e) => e.sample(d) * lighting.environment_intensity,
                    None => Vec3::splat(self.ambient),
                };
                out += through
                    * if bounce > 0 {
                        // Clamped: a rare bounce into a tiny bright spot (the sun) would leave
                        // speckles that take thousands of passes to average out.
                        sky().min(Vec3::splat(4.0))
                    } else if lighting.show_environment && self.view.environment.is_some() {
                        sky()
                    } else {
                        Vec3::from_array(self.view.background)
                    };
                break;
            };
            if bounce == 0 {
                first = Some(t);
            }
            let w = 1.0 - u - v;
            let m = &sc.materials[sc.material[tri] as usize];
            let s = sc.scalar[3 * tri] * w + sc.scalar[3 * tri + 1] * u + sc.scalar[3 * tri + 2] * v;
            let albedo = m.color(s);
            if !m.lit {
                out += through * albedo;
                break;
            }
            let (a, b, c) = (sc.pos[3 * tri], sc.pos[3 * tri + 1], sc.pos[3 * tri + 2]);
            let mut ng = (b - a).cross(c - a).normalize_or_zero();
            if ng.dot(d) > 0.0 {
                ng = -ng;
            }
            let mut n = (sc.normal[3 * tri] * w + sc.normal[3 * tri + 1] * u + sc.normal[3 * tri + 2] * v).normalize_or_zero();
            if n == Vec3::ZERO {
                n = ng;
            }
            if n.dot(ng) < 0.0 {
                n = -n;
            }
            let p = o + d * t + ng * eps;
            let spec_color = Vec3::splat(m.specular).lerp(albedo, m.metallic);
            // Direct light, with soft shadows (lights a few degrees wide).
            for (l, col) in &self.lights {
                let (lt, lb) = basis(*l);
                let r = 0.03 * rng.next().sqrt();
                let phi = 6.283_185_5 * rng.next();
                let ld = (*l + lt * (r * phi.cos()) + lb * (r * phi.sin())).normalize();
                let ndl = n.dot(ld);
                if ndl <= 0.0 || ng.dot(ld) <= 0.0 || sc.intersect(p, ld, f32::INFINITY, true).is_some() {
                    continue;
                }
                out += through * albedo * *col * ndl * (1.0 - m.metallic);
                let hv = (ld - d).normalize();
                out += through * spec_color * *col * n.dot(hv).max(0.0).powf(m.power);
            }
            // Next direction: a glossy reflection (weight: the reflectance at normal incidence)
            // or a diffuse bounce (cosine-weighted, so the weight is the albedo).
            let f0 = Vec3::splat(m.specular / 3.0).lerp(albedo, m.metallic);
            let (ws, wd) = (f0.max_element(), 1.0 - m.metallic);
            let p_spec = if ws + wd > 0.0 { ws / (ws + wd) } else { 0.0 };
            if rng.next() < p_spec {
                let r = d - n * (2.0 * d.dot(n));
                let g = m.roughness * m.roughness;
                let jitter = cosine_dir(n, rng) - n;
                let nd = (r + jitter * g).normalize_or_zero();
                if nd.dot(ng) <= 0.0 {
                    break;
                }
                through *= f0 / p_spec;
                if bounce >= 2 {
                    let keep = through.max_element().clamp(0.05, 1.0);
                    if rng.next() > keep {
                        break;
                    }
                    through /= keep;
                }
                o = p;
                d = nd;
                continue;
            }
            through *= albedo * (wd / (1.0 - p_spec));
            if bounce >= 2 {
                let keep = through.max_element().clamp(0.05, 1.0);
                if rng.next() > keep {
                    break;
                }
                through /= keep;
            }
            o = p;
            d = cosine_dir(n, rng);
            if d.dot(ng) <= 0.0 {
                break;
            }
        }
        (out, first)
    }

    /// One more sample per pixel.
    pub fn pass(&mut self) {
        let (w, h) = (self.view.width as usize, self.view.height as usize);
        let pass = self.passes;
        let vp = self.view.camera.proj_matrix(w as f64 / h.max(1) as f64) * self.view.camera.view_matrix();
        let origin = self.scene.origin;
        let rows: Vec<(Vec<Vec3>, Vec<Option<f32>>)> = (0..h)
            .into_par_iter()
            .map(|y| {
                let mut c = Vec::with_capacity(w);
                let mut z = Vec::with_capacity(w);
                for x in 0..w {
                    let mut rng = Rng::new(((y * w + x) as u64) << 20 ^ pass as u64);
                    // The first pass goes through pixel centers (its hits give the depth).
                    let (jx, jy) = if pass == 0 { (0.5, 0.5) } else { (rng.next(), rng.next()) };
                    let (o, d) = self.ray(x as f32 + jx, y as f32 + jy);
                    let (l, t) = self.radiance(o, d, &mut rng);
                    c.push(l);
                    z.push(if pass == 0 {
                        t.map(|t| o + d * t).map(|p| {
                            let wp = p.as_dvec3() + origin;
                            let clip = vp * DVec4::new(wp.x, wp.y, wp.z, 1.0);
                            (clip.z / clip.w) as f32
                        })
                    } else {
                        None
                    });
                }
                (c, z)
            })
            .collect();
        for (y, (c, z)) in rows.into_iter().enumerate() {
            for x in 0..w {
                self.accum[y * w + x] += c[x];
                if let Some(d) = z[x] {
                    self.depth[y * w + x] = d.clamp(0.0, 0.999_999);
                }
            }
        }
        if pass == 0 {
            self.bias_depth();
        }
        self.passes += 1;
    }

    fn bias_depth(&mut self) {
        self.depth = slope_bias(&self.depth, self.view.width as usize, self.view.height as usize);
    }

    /// The accumulated picture (sRGB) and depth.
    pub fn image(&self) -> ImageLayer {
        let color = encode(self.accum.iter().copied(), 1.0 / self.passes.max(1) as f32);
        ImageLayer { width: self.view.width, height: self.view.height, color, depth: self.depth.clone() }
    }
}
