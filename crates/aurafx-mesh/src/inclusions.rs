//! A box of matrix with star-shaped inclusions, meshed with conforming hexahedra.
//!
//! The method of hexaform (Aashique Rahman's structured hex mesher): a uniform background grid
//! of cells; each inclusion gets a macro-box of whole grid cells, carved out of the grid and
//! filled with a cubed-sphere O-grid block — a transfinite core, an inner shell up to the exact
//! analytic surface, and an outer (matrix) shell out to the flat box faces, whose nodes are the
//! grid's own. Coincident nodes are welded exactly. Element blocks: 1 `matrix` (grid cells and
//! outer shells), 100 `inclusions` (cores and inner shells) or 100, 101, ... one per inclusion;
//! side sets `x-` ... `z+` and `interface`; node sets `x-` ... `z+`; element variable
//! `particle_id` (+id inside an inclusion, -id in its outer shell, 0 elsewhere).

use crate::hex::{HexMesh, P3};
use crate::shape::real_sh;
use std::collections::BTreeMap;
use std::f64::consts::PI;

// ------------------------------------------------------------------ star shapes

/// A body star-shaped about its centre: one surface point along every direction.
#[derive(Clone, Debug)]
pub enum StarShape {
    Sphere { centre: P3, radius: f64 },
    /// `rotation` maps body axes to world axes (columns are the body axes).
    Ellipsoid { centre: P3, semi_axes: P3, rotation: Option<[[f64; 3]; 3]> },
    /// (|x/a|^r + |y/b|^r)^(t/r) + |z/c|^t = 1.
    Superquadric { centre: P3, semi_axes: P3, exponent_xy: f64, exponent_z: f64, rotation: Option<[[f64; 3]; 3]> },
    /// r = R (1 + sum a_lm Y_lm), the perturbation clamped to [-0.6, 2].
    Harmonic { centre: P3, radius: f64, coefficients: Vec<(i32, i32, f64)> },
}

fn normalize(d: P3) -> P3 {
    let n = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt();
    if n > 0.0 {
        [d[0] / n, d[1] / n, d[2] / n]
    } else {
        [1.0, 0.0, 0.0]
    }
}

/// Row vector d times matrix m (world direction to body axes).
fn to_body(d: P3, m: &Option<[[f64; 3]; 3]>) -> P3 {
    match m {
        None => d,
        Some(m) => std::array::from_fn(|j| d[0] * m[0][j] + d[1] * m[1][j] + d[2] * m[2][j]),
    }
}

impl StarShape {
    pub fn centre(&self) -> P3 {
        match self {
            StarShape::Sphere { centre, .. } | StarShape::Ellipsoid { centre, .. } | StarShape::Superquadric { centre, .. } | StarShape::Harmonic { centre, .. } => *centre,
        }
    }
    pub fn with_centre(&self, c: P3) -> StarShape {
        let mut s = self.clone();
        match &mut s {
            StarShape::Sphere { centre, .. } | StarShape::Ellipsoid { centre, .. } | StarShape::Superquadric { centre, .. } | StarShape::Harmonic { centre, .. } => *centre = c,
        }
        s
    }
    /// Distance from the centre to the surface along `d` (any length).
    pub fn support_radius(&self, d: P3) -> f64 {
        let d = normalize(d);
        match self {
            StarShape::Sphere { radius, .. } => *radius,
            StarShape::Ellipsoid { semi_axes, rotation, .. } => {
                let b = to_body(d, rotation);
                1.0 / ((b[0] / semi_axes[0]).powi(2) + (b[1] / semi_axes[1]).powi(2) + (b[2] / semi_axes[2]).powi(2)).sqrt()
            }
            StarShape::Superquadric { semi_axes, exponent_xy: r, exponent_z: t, rotation, .. } => {
                let b = to_body(d, rotation);
                let u: P3 = std::array::from_fn(|k| (b[k] / semi_axes[k]).abs());
                let f = (u[0].powf(*r) + u[1].powf(*r)).powf(t / r) + u[2].powf(*t);
                f.powf(-1.0 / t)
            }
            StarShape::Harmonic { radius, coefficients, .. } => {
                let phi = d[1].atan2(d[0]);
                let mut p = 0.0;
                for (l, m, a) in coefficients {
                    if *a != 0.0 {
                        p += a * real_sh(*l, *m, d[2].clamp(-1.0, 1.0), phi);
                    }
                }
                radius * (1.0 + p.clamp(-0.6, 2.0))
            }
        }
    }
    pub fn surface_point(&self, d: P3) -> P3 {
        let d = normalize(d);
        let r = self.support_radius(d);
        let c = self.centre();
        [c[0] + r * d[0], c[1] + r * d[1], c[2] + r * d[2]]
    }
    /// Move `x` radially onto the surface.
    pub fn project(&self, x: P3) -> P3 {
        let c = self.centre();
        self.surface_point([x[0] - c[0], x[1] - c[1], x[2] - c[2]])
    }
    /// Axis-aligned bounding box from a dense cubed-sphere sampling of the surface.
    pub fn bounding_box(&self) -> (P3, P3) {
        let cs = CubedSphere::new([48; 3], Spacing::Equiangular);
        let (mut lo, mut hi) = ([f64::INFINITY; 3], [f64::NEG_INFINITY; 3]);
        for d in &cs.directions {
            let p = self.surface_point(*d);
            for k in 0..3 {
                lo[k] = lo[k].min(p[k]);
                hi[k] = hi[k].max(p[k]);
            }
        }
        (lo, hi)
    }
    pub fn volume_estimate(&self) -> f64 {
        let cs = CubedSphere::new([64; 3], Spacing::Equiangular);
        let c = self.centre();
        let p: Vec<P3> = cs.directions.iter().map(|d| {
            let s = self.surface_point(*d);
            [s[0] - c[0], s[1] - c[1], s[2] - c[2]]
        }).collect();
        cs.quads.iter().map(|q| {
            let [a, b, cc, d] = q.map(|i| p[i as usize]);
            (crate::hex::dot(a, crate::hex::cross(b, cc)) + crate::hex::dot(a, crate::hex::cross(cc, d))) / 6.0
        }).sum()
    }
}

// ------------------------------------------------------------------ cubed sphere

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Spacing {
    Uniform,
    Equiangular,
    /// (1 - w) uniform + w equiangular.
    Blend(f64),
}

/// `n + 1` values of numpy's linspace(-1, 1, n + 1).
fn linspace(a: f64, b: f64, n: usize) -> Vec<f64> {
    let step = (b - a) / n as f64;
    let mut v: Vec<f64> = (0..=n).map(|i| i as f64 * step + a).collect();
    v[n] = b;
    v
}

pub fn patch_spacing(n: usize, mode: Spacing) -> Vec<f64> {
    let u = linspace(-1.0, 1.0, n);
    let w = match mode {
        Spacing::Uniform => return u,
        Spacing::Equiangular => 1.0,
        Spacing::Blend(w) => w.clamp(0.0, 1.0),
    };
    u.iter().map(|x| (1.0 - w) * x + w * (0.25 * PI * x).tan()).collect()
}

/// (outward normal, i direction, j direction) of the six patches, i x j = normal.
const FACES: [(&str, P3, P3, P3); 6] = [
    ("x+", [1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]),
    ("x-", [-1.0, 0.0, 0.0], [0.0, 0.0, 1.0], [0.0, 1.0, 0.0]),
    ("y+", [0.0, 1.0, 0.0], [0.0, 0.0, 1.0], [1.0, 0.0, 0.0]),
    ("y-", [0.0, -1.0, 0.0], [1.0, 0.0, 0.0], [0.0, 0.0, 1.0]),
    ("z+", [0.0, 0.0, 1.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]),
    ("z-", [0.0, 0.0, -1.0], [0.0, 1.0, 0.0], [1.0, 0.0, 0.0]),
];

fn axis_of(v: P3) -> usize {
    (0..3).max_by(|a, b| v[*a].abs().total_cmp(&v[*b].abs())).unwrap()
}

/// Six-patch quadrangulation of the unit sphere; patch edges along axis a carry n[a] cells.
pub struct CubedSphere {
    pub n: [usize; 3],
    pub cube_points: Vec<P3>,
    pub directions: Vec<P3>,
    /// Per patch (in FACES order): (rows, cols, node ids row-major).
    pub index: Vec<(usize, usize, Vec<u32>)>,
    pub quads: Vec<[u32; 4]>,
}

impl CubedSphere {
    pub fn new(n: [usize; 3], spacing: Spacing) -> CubedSphere {
        let s: Vec<Vec<f64>> = (0..3).map(|a| patch_spacing(n[a], spacing)).collect();
        let mut flat: Vec<P3> = Vec::new();
        let mut shapes = Vec::new();
        for (_, normal, idir, jdir) in FACES {
            let (ai, aj) = (axis_of(idir), axis_of(jdir));
            let (mi, mj) = (s[ai].len(), s[aj].len());
            for i in 0..mi {
                for j in 0..mj {
                    flat.push(std::array::from_fn(|k| normal[k] + s[ai][i] * idir[k] + s[aj][j] * jdir[k]));
                }
            }
            shapes.push((mi, mj));
        }
        // Weld shared edges: unique rows of the coordinates rounded to 12 decimals, in sorted
        // order (the numbering of the reference implementation).
        let key = |p: &P3| -> [f64; 3] { p.map(|x| (x * 1e12).round_ties_even() / 1e12) };
        let mut order: Vec<usize> = (0..flat.len()).collect();
        order.sort_by(|a, b| {
            let (ka, kb) = (key(&flat[*a]), key(&flat[*b]));
            ka[0].total_cmp(&kb[0]).then(ka[1].total_cmp(&kb[1])).then(ka[2].total_cmp(&kb[2])).then(a.cmp(b))
        });
        let mut inverse = vec![0u32; flat.len()];
        let mut cube_points = Vec::new();
        let mut last: Option<[f64; 3]> = None;
        for &i in &order {
            let k = key(&flat[i]);
            if last != Some(k) {
                cube_points.push(flat[i]);
                last = Some(k);
            }
            inverse[i] = cube_points.len() as u32 - 1;
        }
        let directions = cube_points.iter().map(|p| normalize(*p)).collect();
        let mut index = Vec::new();
        let mut start = 0;
        let mut quads = Vec::new();
        for (mi, mj) in shapes {
            let ids = inverse[start..start + mi * mj].to_vec();
            start += mi * mj;
            for i in 0..mi - 1 {
                for j in 0..mj - 1 {
                    quads.push([ids[i * mj + j], ids[(i + 1) * mj + j], ids[(i + 1) * mj + j + 1], ids[i * mj + j + 1]]);
                }
            }
            index.push((mi, mj, ids));
        }
        CubedSphere { n, cube_points, directions, index, quads }
    }
    pub fn n_nodes(&self) -> usize {
        self.cube_points.len()
    }
}

/// `n + 1` monotone weights over [0, 1]: uniform, or graded with largest/smallest interval
/// `ratio`, small intervals at the `start`, `end` or `both`.
pub fn graded_weights(n: usize, ratio: f64, cluster: &str) -> Vec<f64> {
    if cluster == "none" || (ratio - 1.0).abs() < 1e-12 {
        return linspace(0.0, 1.0, n);
    }
    if cluster == "both" {
        let half = (n / 2).max(1);
        let left: Vec<f64> = graded_weights(half, ratio, "start").iter().map(|x| 0.5 * x).collect();
        let right: Vec<f64> = graded_weights(n - half, ratio, "end").iter().map(|x| 0.5 + 0.5 * x).collect();
        return left.into_iter().chain(right.into_iter().skip(1)).collect();
    }
    let g = ratio.powf(1.0 / (n.max(2) - 1) as f64);
    let mut steps: Vec<f64> = (0..n).map(|i| g.powi(i as i32)).collect();
    if cluster == "end" {
        steps.reverse();
    }
    let mut w = vec![0.0];
    let mut acc = 0.0;
    for s in steps {
        acc += s;
        w.push(acc);
    }
    let last = *w.last().unwrap();
    w.iter().map(|x| x / last).collect()
}

/// Transfinite fill of a lattice from its six faces (x_lo/x_hi indexed [v][w], y_* [u][w],
/// z_* [u][v]); returns points [u][v][w] flattened u-major.
#[allow(clippy::too_many_arguments)]
pub fn tfi_volume(x_lo: &[Vec<P3>], x_hi: &[Vec<P3>], y_lo: &[Vec<P3>], y_hi: &[Vec<P3>], z_lo: &[Vec<P3>], z_hi: &[Vec<P3>]) -> Vec<P3> {
    let (nv, nw) = (x_lo.len() - 1, x_lo[0].len() - 1);
    let nu = y_lo.len() - 1;
    let (us, vs, ws) = (linspace(0.0, 1.0, nu), linspace(0.0, 1.0, nv), linspace(0.0, 1.0, nw));
    let c = [[[x_lo[0][0], x_lo[0][nw]], [x_lo[nv][0], x_lo[nv][nw]]], [[x_hi[0][0], x_hi[0][nw]], [x_hi[nv][0], x_hi[nv][nw]]]];
    let mut out = Vec::with_capacity((nu + 1) * (nv + 1) * (nw + 1));
    for i in 0..=nu {
        let u = us[i];
        for j in 0..=nv {
            let v = vs[j];
            for k in 0..=nw {
                let w = ws[k];
                let p: P3 = std::array::from_fn(|a| {
                    let uu = (1.0 - u) * x_lo[j][k][a] + u * x_hi[j][k][a];
                    let vv = (1.0 - v) * y_lo[i][k][a] + v * y_hi[i][k][a];
                    let ww = (1.0 - w) * z_lo[i][j][a] + w * z_hi[i][j][a];
                    let uv = (1.0 - u) * (1.0 - v) * x_lo[0][k][a] + (1.0 - u) * v * x_lo[nv][k][a] + u * (1.0 - v) * x_hi[0][k][a] + u * v * x_hi[nv][k][a];
                    let uw = (1.0 - u) * (1.0 - w) * x_lo[j][0][a] + (1.0 - u) * w * x_lo[j][nw][a] + u * (1.0 - w) * x_hi[j][0][a] + u * w * x_hi[j][nw][a];
                    let vw = (1.0 - v) * (1.0 - w) * y_lo[i][0][a] + (1.0 - v) * w * y_lo[i][nw][a] + v * (1.0 - w) * y_hi[i][0][a] + v * w * y_hi[i][nw][a];
                    let uvw = (1.0 - u) * (1.0 - v) * (1.0 - w) * c[0][0][0][a]
                        + (1.0 - u) * (1.0 - v) * w * c[0][0][1][a]
                        + (1.0 - u) * v * (1.0 - w) * c[0][1][0][a]
                        + (1.0 - u) * v * w * c[0][1][1][a]
                        + u * (1.0 - v) * (1.0 - w) * c[1][0][0][a]
                        + u * (1.0 - v) * w * c[1][0][1][a]
                        + u * v * (1.0 - w) * c[1][1][0][a]
                        + u * v * w * c[1][1][1][a];
                    uu + vv + ww - uv - uw - vw + uvw
                });
                out.push(p);
            }
        }
    }
    out
}

// ------------------------------------------------------------------ macro-block

#[derive(Clone, Debug)]
pub struct BlockOptions {
    pub core_fraction: f64,
    pub core_roundness: f64,
    pub n_inner: usize,
    pub n_outer: usize,
    pub inner_grading: f64,
    pub outer_grading: f64,
    pub spacing: Spacing,
}

impl Default for BlockOptions {
    fn default() -> Self {
        BlockOptions { core_fraction: 0.55, core_roundness: 0.35, n_inner: 2, n_outer: 2, inner_grading: 1.0, outer_grading: 1.0, spacing: Spacing::Blend(0.6) }
    }
}

/// One inclusion's block: points, HEX8 cells, which cells are inclusion material, and the
/// matrix-side faces on the analytic surface.
pub struct MacroBlock {
    pub points: Vec<P3>,
    pub cells: Vec<[u32; 8]>,
    pub is_inclusion: Vec<bool>,
    pub interface: Vec<(u32, u8)>,
}

/// Mesh `shape` inside the box of `k` grid cells whose lower corner is grid index `lo`.
pub fn macro_block(shape: &StarShape, lo: [i64; 3], k: [usize; 3], h: f64, origin: P3, opt: &BlockOptions) -> Result<MacroBlock, String> {
    if k.iter().any(|x| *x < 2) {
        return Err("a macro-box must span at least 2 cells per axis".into());
    }
    let cs = CubedSphere::new(k, opt.spacing);
    let ns = cs.n_nodes();
    let c = shape.centre();
    let radii: Vec<f64> = cs.directions.iter().map(|d| shape.support_radius(*d)).collect();
    let surf: Vec<P3> = (0..ns).map(|i| std::array::from_fn(|a| c[a] + radii[i] * cs.directions[i][a])).collect();
    let s3 = 3f64.sqrt();
    let core: Vec<P3> = (0..ns)
        .map(|i| {
            let q = cs.cube_points[i];
            let d = cs.directions[i];
            std::array::from_fn(|a| c[a] + opt.core_fraction * radii[i] * ((1.0 - opt.core_roundness) * (q[a] / s3) + opt.core_roundness * d[a]))
        })
        .collect();
    // Box faces by integer index arithmetic (bit-identical to the grid's nodes).
    let mut boxp = vec![[0.0; 3]; ns];
    for (f, (_, normal, idir, jdir)) in FACES.iter().enumerate() {
        let axis = axis_of(*normal);
        let side = if normal[axis] > 0.0 { k[axis] as i64 } else { 0 };
        let (ai, aj) = (axis_of(*idir), axis_of(*jdir));
        let (mi, mj, ids) = &cs.index[f];
        for i in 0..*mi {
            for j in 0..*mj {
                let mut idx = [0i64; 3];
                idx[axis] = side;
                idx[ai] = i as i64;
                idx[aj] = j as i64;
                boxp[ids[i * mj + j] as usize] = std::array::from_fn(|a| origin[a] + (lo[a] + idx[a]) as f64 * h);
            }
        }
    }
    // Core lattice: transfinite fill from the six patches, reoriented to lattice order.
    let patch = |f: usize, src: &[P3]| -> Vec<Vec<P3>> {
        let (mi, mj, ids) = &cs.index[f];
        (0..*mi).map(|i| (0..*mj).map(|j| src[ids[i * mj + j] as usize]).collect()).collect()
    };
    let tr = |m: Vec<Vec<P3>>| -> Vec<Vec<P3>> { (0..m[0].len()).map(|j| (0..m.len()).map(|i| m[i][j]).collect()).collect() };
    let x_hi = patch(0, &core);
    let x_lo = tr(patch(1, &core));
    let y_hi = tr(patch(2, &core));
    let y_lo = patch(3, &core);
    let z_hi = patch(4, &core);
    let z_lo = tr(patch(5, &core));
    let lattice = tfi_volume(&x_lo, &x_hi, &y_lo, &y_hi, &z_lo, &z_hi);
    let n_core = lattice.len();
    let w_in = graded_weights(opt.n_inner, opt.inner_grading, "end");
    let w_out = graded_weights(opt.n_outer, opt.outer_grading, "start");
    let mut points = lattice;
    for t in &w_in[1..] {
        points.extend((0..ns).map(|i| std::array::from_fn::<f64, 3, _>(|a| (1.0 - t) * core[i][a] + t * surf[i][a])));
    }
    for t in &w_out[1..] {
        points.extend((0..ns).map(|i| std::array::from_fn::<f64, 3, _>(|a| (1.0 - t) * surf[i][a] + t * boxp[i][a])));
    }
    let inner_base = |t: usize| n_core + (t - 1) * ns;
    let outer_base = |t: usize| n_core + opt.n_inner * ns + (t - 1) * ns;
    let surface_base = inner_base(opt.n_inner);
    // Surface node -> core lattice node (layer 0 of the inner shell).
    let (kx, ky, kz) = (k[0], k[1], k[2]);
    let lat = |i: usize, j: usize, l: usize| ((i * (ky + 1) + j) * (kz + 1) + l) as u32;
    let mut surf_to_core = vec![u32::MAX; ns];
    // (axis, side, transposed): patch (i, j) -> lattice indices.
    let map: [(usize, bool, bool); 6] = [(0, true, false), (0, false, true), (1, true, true), (1, false, false), (2, true, false), (2, false, true)];
    for (f, (axis, hi_side, transpose)) in map.iter().enumerate() {
        let (mi, mj, ids) = &cs.index[f];
        for i in 0..*mi {
            for j in 0..*mj {
                // The plane's two remaining axes in increasing order (a, b); the patch gives
                // (a, b) directly or transposed.
                let (pa, pb) = if *transpose { (j, i) } else { (i, j) };
                let fixed = if *hi_side { k[*axis] } else { 0 };
                let id = match axis {
                    0 => lat(fixed, pa, pb),
                    1 => lat(pa, fixed, pb),
                    _ => lat(pa, pb, fixed),
                };
                surf_to_core[ids[i * mj + j] as usize] = id;
            }
        }
    }
    if surf_to_core.contains(&u32::MAX) {
        return Err("surface-to-core mapping is incomplete".into());
    }
    let layer = |inner: bool, t: usize| -> Vec<u32> {
        if inner {
            if t == 0 {
                surf_to_core.clone()
            } else {
                (inner_base(t) as u32..(inner_base(t) + ns) as u32).collect()
            }
        } else if t == 0 {
            (surface_base as u32..(surface_base + ns) as u32).collect()
        } else {
            (outer_base(t) as u32..(outer_base(t) + ns) as u32).collect()
        }
    };
    let mut cells: Vec<[u32; 8]> = Vec::new();
    for i in 0..kx {
        for j in 0..ky {
            for l in 0..kz {
                cells.push([lat(i, j, l), lat(i + 1, j, l), lat(i + 1, j + 1, l), lat(i, j + 1, l), lat(i, j, l + 1), lat(i + 1, j, l + 1), lat(i + 1, j + 1, l + 1), lat(i, j + 1, l + 1)]);
            }
        }
    }
    let mut is_inclusion = vec![true; cells.len()];
    let shell = |cells: &mut Vec<[u32; 8]>, inner: &[u32], outer: &[u32]| {
        for (mi, mj, ids) in &cs.index {
            for i in 0..mi - 1 {
                for j in 0..mj - 1 {
                    let q = [ids[i * mj + j], ids[(i + 1) * mj + j], ids[(i + 1) * mj + j + 1], ids[i * mj + j + 1]];
                    cells.push([inner[q[0] as usize], inner[q[1] as usize], inner[q[2] as usize], inner[q[3] as usize], outer[q[0] as usize], outer[q[1] as usize], outer[q[2] as usize], outer[q[3] as usize]]);
                }
            }
        }
    };
    for t in 0..opt.n_inner {
        shell(&mut cells, &layer(true, t), &layer(true, t + 1));
    }
    is_inclusion.resize(cells.len(), true);
    let first_outer = cells.len() as u32;
    for t in 0..opt.n_outer {
        shell(&mut cells, &layer(false, t), &layer(false, t + 1));
    }
    is_inclusion.resize(cells.len(), false);
    let interface = (first_outer..first_outer + cs.quads.len() as u32).map(|e| (e, 5u8)).collect();
    Ok(MacroBlock { points, cells, is_inclusion, interface })
}

// ------------------------------------------------------------------ grid, packing, assembly

/// A uniform grid of `div` cells of size `h` from `origin`.
#[derive(Clone, Copy, Debug)]
pub struct Grid {
    pub div: [usize; 3],
    pub origin: P3,
    pub h: f64,
}

impl Grid {
    pub fn upper(&self) -> P3 {
        std::array::from_fn(|a| self.origin[a] + self.div[a] as f64 * self.h)
    }
    /// All grid nodes (x slowest) and the cells outside the carved boxes.
    pub fn build(&self, boxes: &[([i64; 3], [usize; 3])]) -> (Vec<P3>, Vec<[u32; 8]>) {
        let [nx, ny, nz] = self.div;
        let (px, py, pz) = (nx + 1, ny + 1, nz + 1);
        let mut points = Vec::with_capacity(px * py * pz);
        for i in 0..px {
            for j in 0..py {
                for l in 0..pz {
                    let idx = [i as f64, j as f64, l as f64];
                    points.push(std::array::from_fn(|a| self.origin[a] + idx[a] * self.h));
                }
            }
        }
        let mut alive = vec![true; nx * ny * nz];
        for (lo, k) in boxes {
            for i in lo[0] as usize..lo[0] as usize + k[0] {
                for j in lo[1] as usize..lo[1] as usize + k[1] {
                    for l in lo[2] as usize..lo[2] as usize + k[2] {
                        alive[(i * ny + j) * nz + l] = false;
                    }
                }
            }
        }
        let id = |i: usize, j: usize, l: usize| ((i * py + j) * pz + l) as u32;
        let mut cells = Vec::new();
        for i in 0..nx {
            for j in 0..ny {
                for l in 0..nz {
                    if alive[(i * ny + j) * nz + l] {
                        cells.push([id(i, j, l), id(i + 1, j, l), id(i + 1, j + 1, l), id(i, j + 1, l), id(i, j, l + 1), id(i + 1, j, l + 1), id(i + 1, j + 1, l + 1), id(i, j + 1, l + 1)]);
                    }
                }
            }
        }
        (points, cells)
    }
}

/// One inclusion and its macro-box (lower grid index, cells per axis).
#[derive(Clone, Debug)]
pub struct Placed {
    pub shape: StarShape,
    pub lo: [i64; 3],
    pub k: [usize; 3],
    pub id: u32,
}

/// Cells per axis of a macro-box for `shape`: ceil(box_ratio * width / h), at least `k_min`.
pub fn box_cells(shape: &StarShape, h: f64, box_ratio: f64, k_min: usize) -> [usize; 3] {
    let (lo, hi) = shape.bounding_box();
    std::array::from_fn(|a| ((box_ratio * (hi[a] - lo[a]) / h).ceil() as usize).max(k_min))
}

/// A small deterministic random generator (xorshift64*), for packing.
pub struct Rng(u64);
impl Rng {
    pub fn new(seed: u64) -> Rng {
        Rng(seed.wrapping_mul(0x9E3779B97F4A7C15) ^ 0xD1B54A32D192ED03 | 1)
    }
    pub fn next_u64(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545F4914F6CDD1D)
    }
    pub fn uniform(&mut self) -> f64 {
        (self.next_u64() >> 11) as f64 / (1u64 << 53) as f64
    }
    pub fn int(&mut self, lo: i64, hi: i64) -> i64 {
        lo + (self.next_u64() % ((hi - lo + 1) as u64)) as i64
    }
    pub fn normal(&mut self) -> f64 {
        let (u1, u2) = (self.uniform().max(1e-300), self.uniform());
        (-2.0 * u1.ln()).sqrt() * (2.0 * PI * u2).cos()
    }
}

/// How the inclusions are spread: count, radii (truncated normal), macro-box margins.
#[derive(Clone, Debug)]
pub struct PackingSpec {
    pub count: usize,
    pub radius_mean: f64,
    pub radius_std: f64,
    pub radius_min: Option<f64>,
    pub radius_max: Option<f64>,
    pub box_ratio: f64,
    pub gap_cells: usize,
    pub wall_cells: usize,
    pub min_radius_cells: f64,
    pub max_tries: usize,
}

impl Default for PackingSpec {
    fn default() -> Self {
        PackingSpec { count: 10, radius_mean: 0.08, radius_std: 0.0, radius_min: None, radius_max: None, box_ratio: 1.35, gap_cells: 2, wall_cells: 1, min_radius_cells: 1.5, max_tries: 4000 }
    }
}

/// Random sequential addition of whole macro-boxes on the grid (largest first), each at
/// least `gap_cells` from the others and `wall_cells` from the domain's faces. `make(radius,
/// rng)` builds each body (at the origin). Returns the placed inclusions (fewer than asked when
/// some do not fit).
pub fn pack(grid: &Grid, spec: &PackingSpec, rng: &mut Rng, make: &dyn Fn(f64, &mut Rng) -> StarShape) -> Vec<Placed> {
    let h = grid.h;
    let lo_r = spec.radius_min.unwrap_or(spec.radius_mean - 3.0 * spec.radius_std).max(spec.min_radius_cells * h).max(1e-12);
    let hi_r = spec.radius_max.unwrap_or(spec.radius_mean + 3.0 * spec.radius_std).max(lo_r);
    let radii: Vec<f64> = (0..spec.count)
        .map(|_| {
            if spec.radius_std <= 0.0 {
                return spec.radius_mean.clamp(lo_r, hi_r);
            }
            for _ in 0..1000 {
                let r = spec.radius_mean + spec.radius_std * rng.normal();
                if r >= lo_r && r <= hi_r {
                    return r;
                }
            }
            spec.radius_mean.clamp(lo_r, hi_r)
        })
        .collect();
    let shapes: Vec<StarShape> = radii.iter().map(|r| make(*r, rng)).collect();
    let ks: Vec<[usize; 3]> = shapes.iter().map(|s| box_cells(s, h, spec.box_ratio, 4)).collect();
    let mut order: Vec<usize> = (0..shapes.len()).collect();
    order.sort_by_key(|i| std::cmp::Reverse(ks[*i].iter().sum::<usize>()));
    let [nx, ny, nz] = grid.div;
    let mut occ = vec![false; nx * ny * nz];
    let mut placed = Vec::new();
    for idx in order {
        let k = ks[idx];
        let w = spec.wall_cells as i64;
        let hi: [i64; 3] = std::array::from_fn(|a| grid.div[a] as i64 - k[a] as i64 - w);
        if (0..3).any(|a| hi[a] < w) {
            continue;
        }
        for _ in 0..spec.max_tries {
            let lo: [i64; 3] = std::array::from_fn(|a| rng.int(w, hi[a]));
            let g = spec.gap_cells as i64;
            let a0: [usize; 3] = std::array::from_fn(|a| (lo[a] - g).max(0) as usize);
            let b0: [usize; 3] = std::array::from_fn(|a| ((lo[a] + k[a] as i64 + g) as usize).min(grid.div[a]));
            let mut free = true;
            'scan: for i in a0[0]..b0[0] {
                for j in a0[1]..b0[1] {
                    for l in a0[2]..b0[2] {
                        if occ[(i * ny + j) * nz + l] {
                            free = false;
                            break 'scan;
                        }
                    }
                }
            }
            if free {
                for i in lo[0] as usize..lo[0] as usize + k[0] {
                    for j in lo[1] as usize..lo[1] as usize + k[1] {
                        for l in lo[2] as usize..lo[2] as usize + k[2] {
                            occ[(i * ny + j) * nz + l] = true;
                        }
                    }
                }
                let centre: P3 = std::array::from_fn(|a| grid.origin[a] + (lo[a] as f64 + k[a] as f64 / 2.0) * h);
                placed.push(Placed { shape: shapes[idx].with_centre(centre), lo, k, id: placed.len() as u32 + 1 });
                break;
            }
        }
    }
    placed
}

/// Options of the assembled mesh.
#[derive(Clone, Debug, Default)]
pub struct AssemblyOptions {
    /// One block per inclusion (100, 101, ...) instead of one `inclusions` block.
    pub separate_blocks: bool,
    /// HEX8, HEX20 or HEX27 (8, 20, 27).
    pub nodes_per_element: usize,
    /// Pair nodes of opposite faces (node sets x-_node_set ... and PeriodicPairs).
    pub periodic: bool,
}

pub const MATRIX_BLOCK: i32 = 1;
pub const INCLUSION_BLOCK: i32 = 100;

/// The mesh of the grid with the inclusions carved out and their blocks welded in.
pub fn assemble(grid: &Grid, inclusions: &[Placed], block: &BlockOptions, opt: &AssemblyOptions) -> Result<HexMesh, String> {
    let boxes: Vec<([i64; 3], [usize; 3])> = inclusions.iter().map(|p| (p.lo, p.k)).collect();
    let (bp, bc) = grid.build(&boxes);
    let mut m = HexMesh { npe: 8, ..Default::default() };
    m.points = bp;
    m.cells = bc.iter().flatten().copied().collect();
    m.block = vec![MATRIX_BLOCK; bc.len()];
    m.block_names.insert(MATRIX_BLOCK, "matrix".into());
    let mut pid: Vec<f64> = vec![0.0; bc.len()];
    let mut interface = Vec::new();
    let mut owner: BTreeMap<u32, usize> = BTreeMap::new();
    for (n, inc) in inclusions.iter().enumerate() {
        let mb = macro_block(&inc.shape, inc.lo, inc.k, grid.h, grid.origin, block)?;
        let bid = if opt.separate_blocks { INCLUSION_BLOCK + inc.id as i32 - 1 } else { INCLUSION_BLOCK };
        m.block_names.insert(bid, if opt.separate_blocks { format!("inclusion_{:04}", inc.id) } else { "inclusions".into() });
        let e0 = m.n_elements() as u32;
        let n0 = m.points.len() as u32;
        m.points.extend_from_slice(&mb.points);
        for (c, incl) in mb.cells.iter().zip(&mb.is_inclusion) {
            m.cells.extend(c.iter().map(|x| x + n0));
            m.block.push(if *incl { bid } else { MATRIX_BLOCK });
            pid.push(if *incl { inc.id as f64 } else { -(inc.id as f64) });
        }
        for (e, s) in mb.interface {
            interface.push((e + e0, s));
            owner.insert(e + e0, n);
        }
    }
    m.elem_fields.push(("particle_id".into(), pid));
    if !interface.is_empty() {
        m.add_side_set("interface", interface.clone());
    }
    m.weld(1e-9 * grid.h.max(1e-30));
    let flipped = m.fix_orientation();
    let _ = flipped;
    let npe = if opt.nodes_per_element == 0 { 8 } else { opt.nodes_per_element };
    if npe != 8 {
        let curved: Vec<(u32, u8, usize)> = m.side_set("interface").map(|faces| faces.iter().map(|(e, s)| (*e, *s, owner[e])).collect()).unwrap_or_default();
        m.elevate(npe, &curved, &|i, p| inclusions[i].shape.project(p));
    }
    let tol = 1e-9 * grid.h;
    let up = grid.upper();
    for axis in 0..3 {
        for side in 0..2 {
            let value = if side == 0 { grid.origin[axis] } else { up[axis] };
            let name = ["x-", "x+", "y-", "y+", "z-", "z+"][2 * axis + side];
            let faces = m.faces_on_plane(axis, value, tol);
            m.add_side_set(name, faces);
            let nodes = m.nodes_on_plane(axis, value, tol);
            m.add_node_set(name, nodes);
        }
    }
    if opt.periodic {
        m.pair_periodic(grid.origin, up, tol)?;
    }
    Ok(m)
}

/// Collective rearrangement: a largest-first random placement, then rounds that move the placed
/// boxes (a random step, or a push towards the side with the most free cells) and seat the
/// largest unplaced box wherever it now fits. Every state keeps the boxes `gap_cells` apart and
/// `wall_cells` from the faces, so the result always meshes; it never places fewer than the
/// first pass.
pub fn pack_collective(grid: &Grid, spec: &PackingSpec, rng: &mut Rng, make: &dyn Fn(f64, &mut Rng) -> StarShape, max_steps: usize, kinetic: f64, jump: i64) -> Vec<Placed> {
    let h = grid.h;
    let lo_r = spec.radius_min.unwrap_or(spec.radius_mean - 3.0 * spec.radius_std).max(spec.min_radius_cells * h).max(1e-12);
    let hi_r = spec.radius_max.unwrap_or(spec.radius_mean + 3.0 * spec.radius_std).max(lo_r);
    let radii: Vec<f64> = (0..spec.count)
        .map(|_| {
            if spec.radius_std <= 0.0 {
                return spec.radius_mean.clamp(lo_r, hi_r);
            }
            for _ in 0..1000 {
                let r = spec.radius_mean + spec.radius_std * rng.normal();
                if r >= lo_r && r <= hi_r {
                    return r;
                }
            }
            spec.radius_mean.clamp(lo_r, hi_r)
        })
        .collect();
    let shapes: Vec<StarShape> = radii.iter().map(|r| make(*r, rng)).collect();
    let ks: Vec<[usize; 3]> = shapes.iter().map(|s| box_cells(s, h, spec.box_ratio, 4)).collect();
    let mut order: Vec<usize> = (0..shapes.len()).collect();
    order.sort_by_key(|i| std::cmp::Reverse(ks[*i].iter().sum::<usize>()));
    let [nx, ny, nz] = grid.div;
    let mut owner = vec![usize::MAX; nx * ny * nz];
    let (g, w) = (spec.gap_cells as i64, spec.wall_cells as i64);
    let fits = |owner: &[usize], lo: [i64; 3], k: [usize; 3], me: usize| -> bool {
        for a in 0..3 {
            if lo[a] < w || lo[a] + k[a] as i64 > grid.div[a] as i64 - w {
                return false;
            }
        }
        let a0: [usize; 3] = std::array::from_fn(|a| (lo[a] - g).max(0) as usize);
        let b0: [usize; 3] = std::array::from_fn(|a| ((lo[a] + k[a] as i64 + g) as usize).min(grid.div[a]));
        for i in a0[0]..b0[0] {
            for j in a0[1]..b0[1] {
                for l in a0[2]..b0[2] {
                    let o = owner[(i * ny + j) * nz + l];
                    if o != usize::MAX && o != me {
                        return false;
                    }
                }
            }
        }
        true
    };
    let set = |owner: &mut Vec<usize>, lo: [i64; 3], k: [usize; 3], v: usize| {
        for i in lo[0] as usize..lo[0] as usize + k[0] {
            for j in lo[1] as usize..lo[1] as usize + k[1] {
                for l in lo[2] as usize..lo[2] as usize + k[2] {
                    owner[(i * ny + j) * nz + l] = v;
                }
            }
        }
    };
    let random_lo = |rng: &mut Rng, k: [usize; 3]| -> Option<[i64; 3]> {
        let hi: [i64; 3] = std::array::from_fn(|a| grid.div[a] as i64 - k[a] as i64 - w);
        if (0..3).any(|a| hi[a] < w) {
            return None;
        }
        Some(std::array::from_fn(|a| rng.int(w, hi[a])))
    };
    // (shape index, lower corner) of the placed boxes; the rest wait in order.
    let mut placed: Vec<(usize, [i64; 3])> = Vec::new();
    let mut waiting: Vec<usize> = Vec::new();
    for &idx in &order {
        let mut ok = false;
        for _ in 0..400 {
            let Some(lo) = random_lo(rng, ks[idx]) else { break };
            if fits(&owner, lo, ks[idx], idx) {
                set(&mut owner, lo, ks[idx], idx);
                placed.push((idx, lo));
                ok = true;
                break;
            }
        }
        if !ok {
            waiting.push(idx);
        }
    }
    let mut stall = 0;
    for _ in 0..max_steps {
        if waiting.is_empty() || stall > 200 {
            break;
        }
        let moves = ((0.6 * placed.len() as f64).round() as usize).max(1);
        for _ in 0..moves {
            let p = (rng.next_u64() % placed.len() as u64) as usize;
            let (idx, lo) = placed[p];
            let k = ks[idx];
            let step: [i64; 3] = if rng.uniform() < kinetic {
                std::array::from_fn(|_| rng.int(-jump, jump))
            } else {
                // Push towards the face with the most free cells beyond it.
                let mut best = (0i64, 0usize, 0i64);
                for a in 0..3 {
                    for dir in [-1i64, 1] {
                        let mut free = 0;
                        for s in 1..=jump {
                            let mut t = lo;
                            t[a] += dir * s;
                            if fits(&owner, t, k, idx) {
                                free = s;
                            } else {
                                break;
                            }
                        }
                        if free > best.0 {
                            best = (free, a, dir);
                        }
                    }
                }
                let mut s = [0i64; 3];
                s[best.1] = best.2 * best.0;
                s
            };
            let t: [i64; 3] = std::array::from_fn(|a| lo[a] + step[a]);
            if step != [0; 3] && fits(&owner, t, k, idx) {
                set(&mut owner, lo, k, usize::MAX);
                set(&mut owner, t, k, idx);
                placed[p].1 = t;
            }
        }
        let idx = waiting[0];
        let mut seated = false;
        for _ in 0..200 {
            let Some(lo) = random_lo(rng, ks[idx]) else { break };
            if fits(&owner, lo, ks[idx], idx) {
                set(&mut owner, lo, ks[idx], idx);
                placed.push((idx, lo));
                waiting.remove(0);
                seated = true;
                break;
            }
        }
        stall = if seated { 0 } else { stall + 1 };
    }
    placed
        .iter()
        .enumerate()
        .map(|(n, (idx, lo))| {
            let k = ks[*idx];
            let centre: P3 = std::array::from_fn(|a| grid.origin[a] + (lo[a] as f64 + k[a] as f64 / 2.0) * h);
            Placed { shape: shapes[*idx].with_centre(centre), lo: *lo, k, id: n as u32 + 1 }
        })
        .collect()
}
