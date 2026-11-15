//! Drawing big meshes: triangles in chunks with their bounds, so chunks outside the view are not
//! drawn, and a coarse copy of very big surfaces to draw while the camera moves.

use crate::tessellate::Vertex;
use std::collections::HashMap;

/// Triangles per chunk tested against the view.
pub const CHUNK_TRIS: usize = 16 * 1024;
/// Surfaces with more triangles than this get a coarse copy for moving the camera.
pub const LOD_MIN_TRIS: usize = 1_000_000;
/// About this many triangles in the coarse copy.
pub const LOD_TARGET_TRIS: usize = 400_000;
/// A surface gets its coarse copy once it was drawn this many times unchanged and has been
/// there this long (a surface that changes every frame, as in an animation, never gets one).
pub const LOD_SETTLE_FRAMES: u32 = 3;
pub const LOD_SETTLE: std::time::Duration = std::time::Duration::from_millis(300);

/// A run of triangles in one vertex buffer and its bounding box (mesh-local coordinates).
#[derive(Debug, Clone, Copy)]
pub struct Chunk {
    pub buffer: usize,
    pub first: u32,
    pub count: u32,
    pub min: [f32; 3],
    pub max: [f32; 3],
}

/// Chunks of `tris` already split into buffers of these vertex counts.
pub fn chunks(tris: &[Vertex], buffer_lengths: &[usize]) -> Vec<Chunk> {
    let mut out = Vec::new();
    let mut base = 0;
    for (b, &len) in buffer_lengths.iter().enumerate() {
        let mut first = 0;
        while first < len {
            let count = (CHUNK_TRIS * 3).min(len - first);
            let (mut min, mut max) = ([f32::INFINITY; 3], [f32::NEG_INFINITY; 3]);
            for v in &tris[base + first..base + first + count] {
                for a in 0..3 {
                    min[a] = min[a].min(v.pos[a]);
                    max[a] = max[a].max(v.pos[a]);
                }
            }
            out.push(Chunk { buffer: b, first: first as u32, count: count as u32, min, max });
            first += count;
        }
        base += len;
    }
    out
}

/// Whether a box (mesh-local) may be seen through the camera matrix `vp` (mesh-local to clip):
/// false only when all its corners are outside the same side of the view.
pub fn in_view(vp: &glam::Mat4, min: [f32; 3], max: [f32; 3]) -> bool {
    let mut outside = [true; 6];
    for k in 0..8 {
        let p =
            glam::Vec4::new(if k & 1 == 0 { min[0] } else { max[0] }, if k & 2 == 0 { min[1] } else { max[1] }, if k & 4 == 0 { min[2] } else { max[2] }, 1.0);
        let c = *vp * p;
        let tests = [c.x < -c.w, c.x > c.w, c.y < -c.w, c.y > c.w, c.z < 0.0, c.z > c.w];
        for (o, t) in outside.iter_mut().zip(tests) {
            *o &= t;
        }
    }
    !outside.iter().any(|&o| o)
}

/// A coarse copy of a triangle soup: corners snapped to a grid whose cells hold several
/// vertices, triangles that collapse dropped, each cell's vertices averaged (the first one's
/// value kept when values are packed texture coordinates or colors).
pub fn coarse(tris: &[Vertex], target: usize, average_scalar: bool) -> Vec<Vertex> {
    coarse_until(tris, target, average_scalar, &std::sync::atomic::AtomicBool::new(false)).unwrap_or_default()
}

/// [`coarse`], given up (None) soon after `stop` is set: the surface it is for was replaced.
pub fn coarse_until(tris: &[Vertex], target: usize, average_scalar: bool, stop: &std::sync::atomic::AtomicBool) -> Option<Vec<Vertex>> {
    let stopped = |i: usize| i % (1 << 16) == 0 && stop.load(std::sync::atomic::Ordering::Relaxed);
    if tris.len() < 3 {
        return Some(tris.to_vec());
    }
    let (mut min, mut max) = ([f32::INFINITY; 3], [f32::NEG_INFINITY; 3]);
    for v in tris {
        for a in 0..3 {
            min[a] = min[a].min(v.pos[a]);
            max[a] = max[a].max(v.pos[a]);
        }
    }
    let size = (0..3).map(|a| max[a] - min[a]).fold(0.0f32, f32::max).max(1e-30);
    // A surface crossing a grid of n cells per side has about 2 n^2 triangles.
    let n = ((target as f32 / 2.0).sqrt()).clamp(8.0, 4096.0);
    let cell = size / n;
    let key = |p: [f32; 3]| -> u64 {
        let q = |a: usize| (((p[a] - min[a]) / cell) as u64).min((1 << 21) - 1);
        q(0) | (q(1) << 21) | (q(2) << 42)
    };
    let mut ids: HashMap<u64, u32> = HashMap::new();
    let mut sums: Vec<([f32; 3], [f32; 3], f32, f32, u32)> = Vec::new();
    let mut corner = Vec::with_capacity(tris.len());
    for (i, v) in tris.iter().enumerate() {
        if stopped(i) {
            return None;
        }
        let next = sums.len() as u32;
        let id = *ids.entry(key(v.pos)).or_insert(next);
        if id == next {
            sums.push(([0.0; 3], [0.0; 3], 0.0, v.scalar, 0));
        }
        let s = &mut sums[id as usize];
        for a in 0..3 {
            s.0[a] += v.pos[a];
            s.1[a] += v.normal[a];
        }
        s.2 += v.scalar;
        s.4 += 1;
        corner.push(id);
    }
    let rep: Vec<Vertex> = sums
        .iter()
        .map(|(p, nrm, sc, first, c)| {
            let k = *c as f32;
            let l = (nrm[0] * nrm[0] + nrm[1] * nrm[1] + nrm[2] * nrm[2]).sqrt().max(1e-30);
            Vertex {
                pos: [p[0] / k, p[1] / k, p[2] / k],
                scalar: if average_scalar { sc / k } else { *first },
                normal: [nrm[0] / l, nrm[1] / l, nrm[2] / l],
                _pad: 0.0,
            }
        })
        .collect();
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::new();
    for (i, t) in corner.chunks_exact(3).enumerate() {
        if stopped(i) {
            return None;
        }
        if t[0] == t[1] || t[1] == t[2] || t[0] == t[2] {
            continue;
        }
        let mut s = [t[0], t[1], t[2]];
        s.sort_unstable();
        if seen.insert(s) {
            out.extend(t.iter().map(|&i| rep[i as usize]));
        }
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(p: [f32; 3]) -> Vertex {
        Vertex { pos: p, scalar: p[0], normal: [0.0, 0.0, 1.0], _pad: 0.0 }
    }

    #[test]
    fn coarse_copy_and_culling() {
        // A 400 x 400 grid of quads in the plane z = 0: 320 000 triangles.
        let n = 400;
        let mut tris = Vec::new();
        for j in 0..n {
            for i in 0..n {
                let (x, y) = (i as f32, j as f32);
                tris.extend([v([x, y, 0.0]), v([x + 1.0, y, 0.0]), v([x + 1.0, y + 1.0, 0.0])]);
                tris.extend([v([x, y, 0.0]), v([x + 1.0, y + 1.0, 0.0]), v([x, y + 1.0, 0.0])]);
            }
        }
        let c = coarse(&tris, 20_000, true);
        // Stopped before it starts: nothing; never stopped: the same copy.
        let stop = std::sync::atomic::AtomicBool::new(true);
        assert!(coarse_until(&tris, 20_000, true, &stop).is_none());
        stop.store(false, std::sync::atomic::Ordering::Relaxed);
        assert_eq!(coarse_until(&tris, 20_000, true, &stop).map(|v| v.len()), Some(c.len()));
        let nt = c.len() / 3;
        assert!(nt > 5_000 && nt < 40_000, "{nt} triangles");
        assert!(c.iter().all(|v| v.pos[0] >= 0.0 && v.pos[0] <= n as f32 && v.normal[2] > 0.99));
        // Chunks: every triangle in one, boxes inside the grid.
        let ch = chunks(&tris, &[tris.len()]);
        assert_eq!(ch.iter().map(|c| c.count as usize).sum::<usize>(), tris.len());
        // A camera looking at one corner sees some chunks, not all.
        let cam = crate::camera::Camera { eye: glam::DVec3::new(10.0, 10.0, 30.0), target: glam::DVec3::new(10.0, 10.0, 0.0), ..Default::default() };
        let vp = cam.view_proj_f32(1.0, glam::DVec3::ZERO);
        let seen = ch.iter().filter(|c| in_view(&vp, c.min, c.max)).count();
        assert!(seen > 0 && seen < ch.len(), "{seen} of {}", ch.len());
    }
}
