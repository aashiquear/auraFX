//! Procedural sources: sphere, cone, cylinder, box, plane, line, point source, wavelet.

use aurafx_data::*;
use std::collections::HashMap;
use std::f64::consts::{PI, TAU};

pub fn sphere(center: [f64; 3], radius: f64, theta_res: usize, phi_res: usize) -> PolyData {
    let (n, m) = (theta_res.max(3), phi_res.max(3));
    let mut pts = Vec::new();
    pts.extend_from_slice(&[center[0], center[1], center[2] + radius]);
    for j in 1..m - 1 {
        let phi = j as f64 / (m - 1) as f64 * PI;
        for i in 0..n {
            let th = i as f64 / n as f64 * TAU;
            pts.extend_from_slice(&[center[0] + radius * phi.sin() * th.cos(), center[1] + radius * phi.sin() * th.sin(), center[2] + radius * phi.cos()]);
        }
    }
    pts.extend_from_slice(&[center[0], center[1], center[2] - radius]);
    let south = (pts.len() / 3 - 1) as u32;
    let mut polys = Cells::new();
    let ring = |j: usize, i: usize| (1 + (j - 1) * n + (i % n)) as u32;
    for i in 0..n {
        polys.push(CellType::Triangle, &[0, ring(1, i), ring(1, i + 1)]);
    }
    for j in 1..m - 2 {
        for i in 0..n {
            polys.push(CellType::Quad, &[ring(j, i), ring(j + 1, i), ring(j + 1, i + 1), ring(j, i + 1)]);
        }
    }
    for i in 0..n {
        polys.push(CellType::Triangle, &[south, ring(m - 2, i + 1), ring(m - 2, i)]);
    }
    let mut pd = PolyData { points: Points::new_f64(pts), polys, ..Default::default() };
    let n_pts = pd.points.len();
    let mut normals = Vec::with_capacity(3 * n_pts);
    for i in 0..n_pts {
        let p = pd.points.get(i);
        normals.extend_from_slice(&[(p[0] - center[0]) / radius, (p[1] - center[1]) / radius, (p[2] - center[2]) / radius]);
    }
    pd.point_data.insert_active(AttributeKind::Normals, Array::new("Normals", 3, ArrayData::F64(normals)));
    pd
}

pub fn cone(center: [f64; 3], height: f64, radius: f64, res: usize, direction: [f64; 3]) -> PolyData {
    let n = res.max(3);
    let dir = crate::slice::normalize(direction);
    let up = if dir[0].abs() < 0.9 { [1.0, 0.0, 0.0] } else { [0.0, 1.0, 0.0] };
    let z = crate::slice::normalize([dir[1] * up[2] - dir[2] * up[1], dir[2] * up[0] - dir[0] * up[2], dir[0] * up[1] - dir[1] * up[0]]);
    let y = [z[1] * dir[2] - z[2] * dir[1], z[2] * dir[0] - z[0] * dir[2], z[0] * dir[1] - z[1] * dir[0]];
    let mut pts = Vec::new();
    let tip = [center[0] + 0.5 * height * dir[0], center[1] + 0.5 * height * dir[1], center[2] + 0.5 * height * dir[2]];
    let base = [center[0] - 0.5 * height * dir[0], center[1] - 0.5 * height * dir[1], center[2] - 0.5 * height * dir[2]];
    pts.extend_from_slice(&tip);
    pts.extend_from_slice(&base);
    for i in 0..n {
        let a = i as f64 / n as f64 * TAU;
        pts.extend_from_slice(&[
            base[0] + radius * (a.cos() * y[0] + a.sin() * z[0]),
            base[1] + radius * (a.cos() * y[1] + a.sin() * z[1]),
            base[2] + radius * (a.cos() * y[2] + a.sin() * z[2]),
        ]);
    }
    let mut polys = Cells::new();
    for i in 0..n as u32 {
        let j = (i + 1) % n as u32;
        polys.push(CellType::Triangle, &[0, 2 + i, 2 + j]);
        polys.push(CellType::Triangle, &[1, 2 + j, 2 + i]);
    }
    PolyData { points: Points::new_f64(pts), polys, ..Default::default() }
}

pub fn cylinder(center: [f64; 3], height: f64, radius: f64, res: usize) -> PolyData {
    let n = res.max(3);
    let mut pts = Vec::new();
    for i in 0..n {
        let a = i as f64 / n as f64 * TAU;
        pts.extend_from_slice(&[center[0] + radius * a.cos(), center[1] - 0.5 * height, center[2] + radius * a.sin()]);
        pts.extend_from_slice(&[center[0] + radius * a.cos(), center[1] + 0.5 * height, center[2] + radius * a.sin()]);
    }
    let mut polys = Cells::new();
    for i in 0..n as u32 {
        let j = (i + 1) % n as u32;
        polys.push(CellType::Quad, &[2 * i, 2 * j, 2 * j + 1, 2 * i + 1]);
    }
    let bottom: Vec<u32> = (0..n as u32).rev().map(|i| 2 * i).collect();
    let top: Vec<u32> = (0..n as u32).map(|i| 2 * i + 1).collect();
    polys.push(CellType::Polygon, &bottom);
    polys.push(CellType::Polygon, &top);
    PolyData { points: Points::new_f64(pts), polys, ..Default::default() }
}

pub fn cube(center: [f64; 3], size: [f64; 3]) -> PolyData {
    let mut pts = Vec::new();
    for k in 0..8 {
        pts.extend_from_slice(&[
            center[0] + if k & 1 != 0 { 0.5 } else { -0.5 } * size[0],
            center[1] + if k & 2 != 0 { 0.5 } else { -0.5 } * size[1],
            center[2] + if k & 4 != 0 { 0.5 } else { -0.5 } * size[2],
        ]);
    }
    let mut polys = Cells::new();
    for f in CellType::Voxel.faces() {
        polys.push(CellType::Quad, &[f[0] as u32, f[1] as u32, f[2] as u32, f[3] as u32]);
    }
    PolyData { points: Points::new_f64(pts), polys, ..Default::default() }
}

pub fn plane(origin: [f64; 3], p1: [f64; 3], p2: [f64; 3], xres: usize, yres: usize) -> PolyData {
    let (nx, ny) = (xres.max(1) + 1, yres.max(1) + 1);
    let mut pts = Vec::with_capacity(3 * nx * ny);
    let mut tc = Vec::with_capacity(2 * nx * ny);
    for j in 0..ny {
        let v = j as f64 / (ny - 1) as f64;
        for i in 0..nx {
            let u = i as f64 / (nx - 1) as f64;
            pts.extend_from_slice(&[
                origin[0] + u * (p1[0] - origin[0]) + v * (p2[0] - origin[0]),
                origin[1] + u * (p1[1] - origin[1]) + v * (p2[1] - origin[1]),
                origin[2] + u * (p1[2] - origin[2]) + v * (p2[2] - origin[2]),
            ]);
            tc.extend_from_slice(&[u, v]);
        }
    }
    let mut polys = Cells::new();
    for j in 0..ny - 1 {
        for i in 0..nx - 1 {
            let a = (j * nx + i) as u32;
            polys.push(CellType::Quad, &[a, a + 1, a + nx as u32 + 1, a + nx as u32]);
        }
    }
    let mut pd = PolyData { points: Points::new_f64(pts), polys, ..Default::default() };
    pd.point_data.insert_active(AttributeKind::TCoords, Array::new("TextureCoordinates", 2, ArrayData::F64(tc)));
    pd
}

pub fn line(p1: [f64; 3], p2: [f64; 3], res: usize) -> PolyData {
    let n = res.max(1) + 1;
    let mut pts = Vec::with_capacity(3 * n);
    for i in 0..n {
        let t = i as f64 / (n - 1) as f64;
        pts.extend_from_slice(&[p1[0] + t * (p2[0] - p1[0]), p1[1] + t * (p2[1] - p1[1]), p1[2] + t * (p2[2] - p1[2])]);
    }
    let mut lines = Cells::new();
    lines.push(CellType::PolyLine, &(0..n as u32).collect::<Vec<_>>());
    PolyData { points: Points::new_f64(pts), lines, ..Default::default() }
}

pub fn point_source(center: [f64; 3], radius: f64, n: usize, seed: u64) -> PolyData {
    // Simple deterministic LCG so results are reproducible.
    let mut s = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
    let mut rnd = || {
        s = s.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        ((s >> 11) as f64) / ((1u64 << 53) as f64)
    };
    let mut pts = Vec::with_capacity(3 * n);
    for _ in 0..n {
        let r = radius * rnd().cbrt();
        let th = TAU * rnd();
        let ph = (2.0 * rnd() - 1.0).acos();
        pts.extend_from_slice(&[center[0] + r * ph.sin() * th.cos(), center[1] + r * ph.sin() * th.sin(), center[2] + r * ph.cos()]);
    }
    PolyData::from_points(Points::new_f64(pts))
}

/// The classic VTK "Wavelet" synthetic volume (RTData), identical to vtkRTAnalyticSource.
pub fn wavelet(extent: [i64; 6], maximum: f64, std_dev: f64) -> ImageData {
    let mut img = ImageData { extent, ..Default::default() };
    let d = img.dims();
    let mut v = Vec::with_capacity(d[0] * d[1] * d[2]);
    let (xf, yf, zf, xm, ym, zm) = (60.0, 30.0, 40.0, 10.0, 18.0, 5.0);
    let scale = |a: i64, b: i64| if b > a { 1.0 / (b - a) as f64 } else { 1.0 };
    let (xs, ys, zs) = (scale(extent[0], extent[1]), scale(extent[2], extent[3]), scale(extent[4], extent[5]));
    let temp2 = 1.0 / (2.0 * std_dev * std_dev);
    for k in 0..d[2] {
        let z = -((extent[4] + k as i64) as f64) * zs;
        for j in 0..d[1] {
            let y = -((extent[2] + j as i64) as f64) * ys;
            for i in 0..d[0] {
                let x = -((extent[0] + i as i64) as f64) * xs;
                let sum = x * x + y * y + z * z;
                let val = maximum * (-sum * temp2).exp() + xm * (xf * x).sin() + ym * (yf * y).sin() + zm * (zf * z).cos();
                v.push(val as f32);
            }
        }
    }
    img.point_data.insert_active(AttributeKind::Scalars, Array::new("RTData", 1, ArrayData::F32(v)));
    img
}

/// Cell types offered by the Unstructured Cell Types source (ParaView names) and their VTK
/// type, linear shape and whether the order parameter applies.
pub const CELL_TYPE_CHOICES: &[(&str, CellType)] = &[
    ("Line", CellType::Line),
    ("Quadratic Line", CellType::QuadraticEdge),
    ("Triangle", CellType::Triangle),
    ("Quadratic Triangle", CellType::QuadraticTriangle),
    ("Quad", CellType::Quad),
    ("Bi-Quadratic Quad", CellType::BiquadraticQuad),
    ("Tet", CellType::Tetra),
    ("Quadratic Tet", CellType::QuadraticTetra),
    ("Hex", CellType::Hexahedron),
    ("Tri-Quadratic Hexahedron", CellType::TriquadraticHexahedron),
    ("Wedge", CellType::Wedge),
    ("Pyramid", CellType::Pyramid),
    ("Lagrange Curve", CellType::LagrangeCurve),
    ("Lagrange Triangle", CellType::LagrangeTriangle),
    ("Lagrange Quadrilateral", CellType::LagrangeQuadrilateral),
    ("Lagrange Tetrahedron", CellType::LagrangeTetrahedron),
    ("Lagrange Hexahedron", CellType::LagrangeHexahedron),
    ("Lagrange Wedge", CellType::LagrangeWedge),
    ("Bezier Curve", CellType::BezierCurve),
    ("Bezier Triangle", CellType::BezierTriangle),
    ("Bezier Quadrilateral", CellType::BezierQuadrilateral),
    ("Bezier Tetrahedron", CellType::BezierTetrahedron),
    ("Bezier Hexahedron", CellType::BezierHexahedron),
    ("Bezier Wedge", CellType::BezierWedge),
];

/// Unstructured Cell Types source: a block of `dims` unit hexahedra filling [0, dims] split
/// into cells of one type, like ParaView's (12 tets or 6 pyramids around each block's center,
/// 2 wedges per block, 2 triangles per square). Lagrange and Bezier cells get `order`;
/// quadratic types are order 2. Point arrays: DistanceToCenter and Polynomial (1 + x + y + z).
pub fn unstructured_cell_types(ty: CellType, dims: [usize; 3], order: usize) -> UnstructuredGrid {
    use aurafx_data::higher_order::{HighOrderCell, Shape};
    let shape_dim = ty.dimension() as usize;
    let d = [dims[0].max(1), if shape_dim >= 2 { dims[1].max(1) } else { 0 }, if shape_dim >= 3 { dims[2].max(1) } else { 0 }];
    let (shape, p) = match ty.high_order() {
        Some((_, s)) => (s, if ty.is_arbitrary_order() { order.clamp(1, 10) } else { 2 }),
        None => (
            match ty {
                CellType::Line => Shape::Curve,
                CellType::Triangle => Shape::Triangle,
                CellType::Quad => Shape::Quad,
                CellType::Tetra => Shape::Tetra,
                CellType::Wedge => Shape::Wedge,
                _ => Shape::Hexahedron,
            },
            1,
        ),
    };
    // Integer coordinates scaled by s = 2p so block centers and every node are integers.
    let s = 2 * p as i64;
    let mut keys: HashMap<[i64; 3], u32> = HashMap::new();
    let mut coords: Vec<f64> = Vec::new();
    let mut id = |k: [i64; 3]| -> u32 {
        *keys.entry(k).or_insert_with(|| {
            coords.extend(k.iter().map(|&v| v as f64 / s as f64));
            (coords.len() / 3 - 1) as u32
        })
    };
    let mut cells = Cells::new();
    // Linear pieces of one block as integer corner coordinates.
    let block = |i: i64, j: i64, k: i64| -> Vec<Vec<[i64; 3]>> {
        let c = |a: i64, b: i64, e: i64| [(i + a) * s, (j + b) * s, (k + e) * s];
        match shape_dim {
            1 => vec![vec![c(0, 0, 0), c(1, 0, 0)]],
            2 => {
                let q = [c(0, 0, 0), c(1, 0, 0), c(1, 1, 0), c(0, 1, 0)];
                if shape == Shape::Triangle {
                    vec![vec![q[0], q[1], q[2]], vec![q[0], q[2], q[3]]]
                } else {
                    vec![q.to_vec()]
                }
            }
            _ => {
                let h = [c(0, 0, 0), c(1, 0, 0), c(1, 1, 0), c(0, 1, 0), c(0, 0, 1), c(1, 0, 1), c(1, 1, 1), c(0, 1, 1)];
                let center = [i * s + s / 2, j * s + s / 2, k * s + s / 2];
                match (shape, ty) {
                    (Shape::Hexahedron, CellType::Pyramid) => {
                        CellType::Hexahedron.faces().iter().map(|f| vec![h[f[3]], h[f[2]], h[f[1]], h[f[0]], center]).collect()
                    }
                    (Shape::Hexahedron, _) => vec![h.to_vec()],
                    // Linear wedges have the base clockwise seen from the top; VTK's
                    // higher-order wedges counter-clockwise.
                    (Shape::Wedge, CellType::Wedge) => vec![vec![h[0], h[3], h[1], h[4], h[7], h[5]], vec![h[1], h[3], h[2], h[5], h[7], h[6]]],
                    (Shape::Wedge, _) => vec![vec![h[0], h[1], h[3], h[4], h[5], h[7]], vec![h[1], h[2], h[3], h[5], h[6], h[7]]],
                    // Tets: each face split in two, joined to the center.
                    _ => CellType::Hexahedron
                        .faces()
                        .iter()
                        .flat_map(|f| [vec![h[f[0]], h[f[2]], h[f[1]], center], vec![h[f[0]], h[f[3]], h[f[2]], center]])
                        .collect(),
                }
            }
        }
    };
    let ho = (p > 1 || ty.high_order().is_some()).then(|| {
        HighOrderCell::with_order(
            aurafx_data::higher_order::Basis::Lagrange,
            shape,
            aurafx_data::higher_order::cell_order(shape, aurafx_data::higher_order::node_count(shape, [p, p, p]), None).unwrap(),
        )
    });
    for k in 0..d[2].max(1) as i64 {
        for j in 0..d[1].max(1) as i64 {
            for i in 0..d[0] as i64 {
                for corners in block(i, j, k) {
                    let ids: Vec<u32> = match (&ho, ty) {
                        (_, CellType::Pyramid) | (None, _) => corners.iter().map(|&c| id(c)).collect(),
                        (Some(cell), _) => {
                            // Nodes: the corners' affine (or trilinear) map at each node's lattice position.
                            let r = shape.corners();
                            (0..cell.len())
                                .map(|n| {
                                    let pc = cell.node_pcoords(n);
                                    let mut w = vec![0.0; r.len()];
                                    match shape {
                                        Shape::Curve => w.copy_from_slice(&[1.0 - pc[0], pc[0]]),
                                        Shape::Triangle => w.copy_from_slice(&[1.0 - pc[0] - pc[1], pc[0], pc[1]]),
                                        Shape::Tetra => w.copy_from_slice(&[1.0 - pc[0] - pc[1] - pc[2], pc[0], pc[1], pc[2]]),
                                        Shape::Wedge => {
                                            let t = [1.0 - pc[0] - pc[1], pc[0], pc[1]];
                                            for a in 0..3 {
                                                w[a] = t[a] * (1.0 - pc[2]);
                                                w[a + 3] = t[a] * pc[2];
                                            }
                                        }
                                        _ => {
                                            for (a, rc) in r.iter().enumerate() {
                                                w[a] = (0..3).map(|x| if rc[x] > 0.5 { pc[x] } else { 1.0 - pc[x] }).take(shape_dim).product();
                                            }
                                        }
                                    }
                                    let x: [i64; 3] =
                                        std::array::from_fn(|a| corners.iter().zip(&w).map(|(c, wi)| c[a] as f64 * wi).sum::<f64>().round() as i64);
                                    id(x)
                                })
                                .collect()
                        }
                    };
                    cells.push(ty, &ids);
                }
            }
        }
    }
    let n = coords.len() / 3;
    let center = [d[0] as f64 / 2.0, d[1] as f64 / 2.0, d[2] as f64 / 2.0];
    let dist: Vec<f32> = (0..n).map(|i| (0..3).map(|a| (coords[3 * i + a] - center[a]).powi(2)).sum::<f64>().sqrt() as f32).collect();
    let poly: Vec<f64> = (0..n).map(|i| 1.0 + coords[3 * i] + coords[3 * i + 1] + coords[3 * i + 2]).collect();
    let mut ug = UnstructuredGrid { points: Points::new_f64(coords), cells, ..Default::default() };
    ug.point_data.insert(Array::scalars_f32("DistanceToCenter", dist));
    ug.point_data.insert_active(AttributeKind::Scalars, Array::scalars_f64("Polynomial", poly));
    ug
}

/// ParaView's AMR Gaussian Pulse source (vtkAMRGaussianPulseSource): a 5×5(×5)-cell root box
/// at `root_spacing` from (-2, -2, 0) and two refined patches, with cell arrays `Centroid` and
/// `Gaussian-Pulse` = amplitude · exp(-Σ ((x - origin)/width)²) over the first `dim` axes.
pub fn amr_gaussian_pulse(dim: usize, root_spacing: f64, ratio: i64, origin: [f64; 3], width: [f64; 3], amplitude: f64) -> DataSet {
    use aurafx_data::amr::{Amr, AmrBlock, AmrLevel};
    let dim = dim.clamp(2, 3);
    let pulse = |c: [f64; 3]| -> f64 {
        let r: f64 = (0..dim).map(|i| ((c[i] - origin[i]) / width[i]).powi(2)).sum();
        amplitude * (-r).exp()
    };
    let grid = |o: [f64; 3], h: [f64; 3], n: [usize; 3]| -> ImageData {
        let mut img = ImageData { extent: [0, n[0] as i64 - 1, 0, n[1] as i64 - 1, 0, n[2] as i64 - 1], origin: o, spacing: h, ..Default::default() };
        let c = [n[0].saturating_sub(1).max(1), n[1].saturating_sub(1).max(1), n[2].saturating_sub(1).max(1)];
        let (mut cent, mut val) = (Vec::with_capacity(3 * c[0] * c[1] * c[2]), Vec::with_capacity(c[0] * c[1] * c[2]));
        for k in 0..c[2] {
            for j in 0..c[1] {
                for i in 0..c[0] {
                    let half = |axis: usize, idx: usize| o[axis] + h[axis] * (idx as f64 + if n[axis] > 1 { 0.5 } else { 0.0 });
                    let p = [half(0, i), half(1, j), half(2, k)];
                    cent.extend_from_slice(&p);
                    val.push(pulse(p));
                }
            }
        }
        img.cell_data.insert(Array::new("Centroid", 3, ArrayData::F64(cent)));
        img.cell_data.insert_active(AttributeKind::Scalars, Array::new("Gaussian-Pulse", 1, ArrayData::F64(val)));
        img
    };
    let h0 = [root_spacing; 3];
    let o0 = [-2.0, -2.0, 0.0];
    let n0 = if dim == 3 { [6, 6, 6] } else { [6, 6, 1] };
    let root = grid(o0, h0, n0);
    let patches: [[i64; 6]; 2] = if dim == 3 { [[0, 2, 0, 3, 0, 5], [3, 5, 2, 5, 0, 5]] } else { [[0, 2, 0, 3, 0, 0], [3, 5, 2, 5, 0, 0]] };
    let r = ratio.max(1) as f64;
    let mut h1 = [0.5; 3];
    for i in 0..dim {
        h1[i] = h0[i] / r;
    }
    let fine: Vec<ImageData> = patches
        .iter()
        .map(|p| {
            let min = [o0[0] + p[0] as f64 * h0[0], o0[1] + p[2] as f64 * h0[1], o0[2] + p[4] as f64 * h0[2]];
            let mut n = [1usize; 3];
            for i in 0..dim {
                let pd = (p[2 * i + 1] - p[2 * i] + 1) as usize;
                n[i] = ratio.max(1) as usize * pd - (ratio.max(1) as usize - 1);
            }
            grid(min, h1, n)
        })
        .collect();
    let mut amr = Amr {
        origin: o0,
        levels: vec![
            AmrLevel { spacing: h0, blocks: vec![AmrBlock::locate(o0, h0, root)] },
            AmrLevel { spacing: h1, blocks: fine.into_iter().map(|g| AmrBlock::locate(o0, h1, g)).collect() },
        ],
    };
    amr.blank();
    amr.into_dataset()
}
