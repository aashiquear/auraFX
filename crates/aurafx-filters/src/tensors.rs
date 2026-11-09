//! Tensors and derivatives:
//!
//! - **Compute Derivatives** of a point vector field: its gradient (the 9-component Jacobian
//!   `dv_i/dx_j`), strain rate (the symmetric part), vorticity (the curl), divergence, and the vortex
//!   criteria Q (positive where rotation beats strain) and λ2 (negative inside a vortex core).
//! - **Tensor Principal Invariants**: the principal values (largest first), principal directions and
//!   the invariants of a symmetric tensor array (6 components XX YY ZZ XY YZ XZ, or 9 row by row).
//! - **Yield Criteria**: von Mises, Tresca, the largest principal stress, the pressure and the
//!   stress triaxiality of a stress tensor array.
//! - **Tensor Glyph**: an ellipsoid at points, its axes along the principal directions and as long as
//!   the principal values (their absolute values).
//! - **Surface Vectors** and **Surface Flow**: a vector field on a surface made tangent to it (or its
//!   normal part kept), and its flux through the surface (∫ v·n dA).

use crate::params::Association;
use crate::simplex::parts;
use anyhow::{anyhow, bail, Result};
use aurafx_data::*;

pub type M3 = [[f64; 3]; 3];

/// A tensor of an array at tuple `i`: 6 components (XX YY ZZ XY YZ XZ) or 9 (row by row, made
/// symmetric by averaging with its transpose when `symmetrize`).
pub fn tensor_at(a: &Array, i: usize, symmetrize: bool) -> Result<M3> {
    Ok(match a.ncomp {
        6 => {
            let c = |k| a.get(i, k);
            [[c(0), c(3), c(5)], [c(3), c(1), c(4)], [c(5), c(4), c(2)]]
        }
        9 => {
            let m: M3 = std::array::from_fn(|r| std::array::from_fn(|k| a.get(i, 3 * r + k)));
            if symmetrize {
                std::array::from_fn(|r| std::array::from_fn(|k| 0.5 * (m[r][k] + m[k][r])))
            } else {
                m
            }
        }
        n => bail!("{} has {n} components: a tensor has 6 (XX YY ZZ XY YZ XZ) or 9", a.name),
    })
}

/// Eigenvalues (largest first) and unit eigenvectors (rows) of a symmetric 3x3 matrix, by Jacobi
/// rotations (accurate to rounding for any symmetric matrix).
pub fn eigen_sym(m: M3) -> ([f64; 3], [[f64; 3]; 3]) {
    let mut a = m;
    let mut v: M3 = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];
    for _ in 0..50 {
        let off = a[0][1] * a[0][1] + a[0][2] * a[0][2] + a[1][2] * a[1][2];
        let scale = a[0][0] * a[0][0] + a[1][1] * a[1][1] + a[2][2] * a[2][2] + 2.0 * off;
        if off <= 1e-30 * scale.max(1e-300) {
            break;
        }
        for (p, q) in [(0usize, 1usize), (0, 2), (1, 2)] {
            if a[p][q].abs() < 1e-300 {
                continue;
            }
            let theta = (a[q][q] - a[p][p]) / (2.0 * a[p][q]);
            let t = theta.signum().max(0.0).mul_add(2.0, -1.0) / (theta.abs() + (theta * theta + 1.0).sqrt());
            let t = if theta == 0.0 { 1.0 } else { t };
            let c = 1.0 / (t * t + 1.0).sqrt();
            let s = t * c;
            // a <- Jᵀ a J for the rotation J in the (p, q) plane.
            for k in 0..3 {
                let (akp, akq) = (a[k][p], a[k][q]);
                a[k][p] = c * akp - s * akq;
                a[k][q] = s * akp + c * akq;
            }
            for k in 0..3 {
                let (apk, aqk) = (a[p][k], a[q][k]);
                a[p][k] = c * apk - s * aqk;
                a[q][k] = s * apk + c * aqk;
            }
            for k in 0..3 {
                let (vkp, vkq) = (v[k][p], v[k][q]);
                v[k][p] = c * vkp - s * vkq;
                v[k][q] = s * vkp + c * vkq;
            }
        }
    }
    let mut idx = [0usize, 1, 2];
    idx.sort_by(|x, y| a[*y][*y].total_cmp(&a[*x][*x]));
    let vals = idx.map(|k| a[k][k]);
    let vecs = idx.map(|k| [v[0][k], v[1][k], v[2][k]]);
    (vals, vecs)
}

fn arr_f64(name: &str, ncomp: usize, v: Vec<f64>) -> Array {
    Array::new(name, ncomp, ArrayData::F64(v))
}

/// The attributes holding `name` (points or cells) and the count of tuples.
fn find<'a>(ds: &'a DataSet, assoc: Association, name: &str) -> Result<(&'a Array, Association)> {
    let (_, pd, cd, _) = parts(ds).ok_or_else(|| anyhow!("unsupported dataset"))?;
    let on = |a: Association| match a {
        Association::Points => pd.get(name).map(|x| (x, a)),
        Association::Cells => cd.get(name).map(|x| (x, a)),
    };
    let other = if assoc == Association::Points { Association::Cells } else { Association::Points };
    on(assoc).or_else(|| on(other)).ok_or_else(|| anyhow!("no array {name:?}"))
}

/// Put arrays on the same side (points or cells) as `assoc` of a copy of `ds`.
fn with_arrays(ds: &DataSet, assoc: Association, arrays: Vec<Array>) -> Result<DataSet> {
    let mut out = crate::simplex::as_explicit(ds).into_owned();
    let attrs = match (&mut out, assoc) {
        (DataSet::ImageData(d), Association::Points) => &mut d.point_data,
        (DataSet::ImageData(d), Association::Cells) => &mut d.cell_data,
        (DataSet::RectilinearGrid(d), Association::Points) => &mut d.point_data,
        (DataSet::RectilinearGrid(d), Association::Cells) => &mut d.cell_data,
        (DataSet::StructuredGrid(d), Association::Points) => &mut d.point_data,
        (DataSet::StructuredGrid(d), Association::Cells) => &mut d.cell_data,
        (DataSet::PolyData(d), Association::Points) => &mut d.point_data,
        (DataSet::PolyData(d), Association::Cells) => &mut d.cell_data,
        (DataSet::UnstructuredGrid(d), Association::Points) => &mut d.point_data,
        (DataSet::UnstructuredGrid(d), Association::Cells) => &mut d.cell_data,
        _ => bail!("unsupported dataset"),
    };
    for a in arrays {
        attrs.insert(a);
    }
    Ok(out)
}

/// Principal values, directions and invariants of a tensor array.
pub fn principal_invariants(ds: &DataSet, assoc: Association, name: &str) -> Result<DataSet> {
    let (a, assoc) = find(ds, assoc, name)?;
    let n = a.len();
    let mut vals = Vec::with_capacity(3 * n);
    let mut dirs: [Vec<f64>; 3] = [Vec::with_capacity(3 * n), Vec::with_capacity(3 * n), Vec::with_capacity(3 * n)];
    let (mut i1, mut i2, mut i3, mut j2, mut j3) =
        (Vec::with_capacity(n), Vec::with_capacity(n), Vec::with_capacity(n), Vec::with_capacity(n), Vec::with_capacity(n));
    for i in 0..n {
        let t = tensor_at(a, i, true)?;
        let (l, v) = eigen_sym(t);
        vals.extend(l);
        for k in 0..3 {
            dirs[k].extend(v[k]);
        }
        let tr = l[0] + l[1] + l[2];
        i1.push(tr);
        i2.push(l[0] * l[1] + l[1] * l[2] + l[2] * l[0]);
        i3.push(l[0] * l[1] * l[2]);
        let m = tr / 3.0;
        let s = l.map(|x| x - m);
        j2.push(0.5 * (s[0] * s[0] + s[1] * s[1] + s[2] * s[2]));
        j3.push(s[0] * s[1] * s[2]);
    }
    let [d1, d2, d3] = dirs;
    with_arrays(
        ds,
        assoc,
        vec![
            arr_f64(&format!("{name}_principal_values"), 3, vals),
            arr_f64(&format!("{name}_principal_direction_1"), 3, d1),
            arr_f64(&format!("{name}_principal_direction_2"), 3, d2),
            arr_f64(&format!("{name}_principal_direction_3"), 3, d3),
            arr_f64(&format!("{name}_I1"), 1, i1),
            arr_f64(&format!("{name}_I2"), 1, i2),
            arr_f64(&format!("{name}_I3"), 1, i3),
            arr_f64(&format!("{name}_J2"), 1, j2),
            arr_f64(&format!("{name}_J3"), 1, j3),
        ],
    )
}

/// Von Mises, Tresca, the largest principal stress, the pressure and the triaxiality of a stress array.
pub fn yield_criteria(ds: &DataSet, assoc: Association, name: &str) -> Result<DataSet> {
    let (a, assoc) = find(ds, assoc, name)?;
    let n = a.len();
    let (mut vm, mut tr, mut mp, mut pr, mut tx) =
        (Vec::with_capacity(n), Vec::with_capacity(n), Vec::with_capacity(n), Vec::with_capacity(n), Vec::with_capacity(n));
    for i in 0..n {
        let (l, _) = eigen_sym(tensor_at(a, i, true)?);
        let mean = (l[0] + l[1] + l[2]) / 3.0;
        let mises = (0.5 * ((l[0] - l[1]).powi(2) + (l[1] - l[2]).powi(2) + (l[2] - l[0]).powi(2))).sqrt();
        vm.push(mises);
        tr.push(l[0] - l[2]);
        mp.push(l[0]);
        pr.push(-mean);
        tx.push(if mises > 0.0 { mean / mises } else { 0.0 });
    }
    with_arrays(
        ds,
        assoc,
        vec![arr_f64("von_Mises", 1, vm), arr_f64("Tresca", 1, tr), arr_f64("max_principal", 1, mp), arr_f64("pressure", 1, pr), arr_f64("triaxiality", 1, tx)],
    )
}

/// Gradient, strain rate, vorticity, divergence, Q and λ2 of a point vector field.
pub fn compute_derivatives(ds: &DataSet, name: &str) -> Result<DataSet> {
    let d = crate::simplex::as_explicit(ds).into_owned();
    let (_, pd, _, _) = parts(&d).ok_or_else(|| anyhow!("Compute Derivatives: unsupported dataset"))?;
    let v = pd.get(name).ok_or_else(|| anyhow!("Compute Derivatives: no point array {name:?}"))?;
    if v.ncomp != 3 {
        bail!("Compute Derivatives works on a vector array (3 components); {name} has {}", v.ncomp);
    }
    let n = v.len();
    // The gradient of each component, through the scalar Gradient.
    let mut grads: Vec<Vec<f64>> = Vec::new();
    for c in 0..3 {
        let tmp_name = format!("__afx_comp_{c}");
        let comp: Vec<f64> = (0..n).map(|i| v.get(i, c)).collect();
        let with = with_arrays(&d, Association::Points, vec![arr_f64(&tmp_name, 1, comp)])?;
        let g = crate::analysis::gradient(&with, &tmp_name)?;
        let (_, gpd, _, _) = parts(&g).ok_or_else(|| anyhow!("gradient failed"))?;
        let ga = gpd.get(&format!("{tmp_name}_gradient")).ok_or_else(|| anyhow!("gradient failed"))?;
        grads.push((0..n).flat_map(|i| (0..3).map(move |k| ga.get(i, k))).collect());
    }
    let (mut jac, mut strain, mut vort, mut div, mut q, mut l2) =
        (Vec::with_capacity(9 * n), Vec::with_capacity(6 * n), Vec::with_capacity(3 * n), Vec::with_capacity(n), Vec::with_capacity(n), Vec::with_capacity(n));
    for i in 0..n {
        // J[r][k] = d v_r / d x_k
        let j: M3 = std::array::from_fn(|r| std::array::from_fn(|k| grads[r][3 * i + k]));
        jac.extend(j.iter().flatten());
        let s: M3 = std::array::from_fn(|r| std::array::from_fn(|k| 0.5 * (j[r][k] + j[k][r])));
        let w: M3 = std::array::from_fn(|r| std::array::from_fn(|k| 0.5 * (j[r][k] - j[k][r])));
        strain.extend([s[0][0], s[1][1], s[2][2], s[0][1], s[1][2], s[0][2]]);
        vort.extend([j[2][1] - j[1][2], j[0][2] - j[2][0], j[1][0] - j[0][1]]);
        div.push(j[0][0] + j[1][1] + j[2][2]);
        let n2 = |m: &M3| m.iter().flatten().map(|x| x * x).sum::<f64>();
        q.push(0.5 * (n2(&w) - n2(&s)));
        // λ2: the middle eigenvalue of S² + Ω².
        let mm: M3 = std::array::from_fn(|r| std::array::from_fn(|k| (0..3).map(|t| s[r][t] * s[t][k] + w[r][t] * w[t][k]).sum()));
        l2.push(eigen_sym(mm).0[1]);
    }
    with_arrays(
        &d,
        Association::Points,
        vec![
            arr_f64(&format!("{name}_gradient"), 9, jac),
            arr_f64(&format!("{name}_strain_rate"), 6, strain),
            arr_f64(&format!("{name}_vorticity"), 3, vort),
            arr_f64(&format!("{name}_divergence"), 1, div),
            arr_f64("Q_criterion", 1, q),
            arr_f64("lambda2", 1, l2),
        ],
    )
}

/// Ellipsoids at the points (every `stride`-th), axes along the principal directions and as long as
/// `scale` times the absolute principal values; the point array `principal_value_1` colors them.
pub fn tensor_glyph(ds: &DataSet, name: &str, scale: f64, stride: usize, resolution: usize) -> Result<PolyData> {
    let (pts, pd, _, _) = parts(ds).ok_or_else(|| anyhow!("Tensor Glyph: unsupported dataset"))?;
    let a = pd.get(name).ok_or_else(|| anyhow!("Tensor Glyph: no point tensor array {name:?}"))?;
    let unit = crate::sources::sphere([0.0; 3], 1.0, resolution.max(6), (resolution / 2).max(4));
    let up: Vec<[f64; 3]> = (0..unit.points.len()).map(|i| unit.points.get(i)).collect();
    let mut out_pts: Vec<f64> = Vec::new();
    let mut polys = Cells::new();
    let mut color = Vec::new();
    for i in (0..pts.len()).step_by(stride.max(1)) {
        let (l, v) = eigen_sym(tensor_at(a, i, true)?);
        let c = pts.get(i);
        let base = (out_pts.len() / 3) as u32;
        for p in &up {
            // x = c + Σ_k scale |λ_k| p_k v_k
            let q: [f64; 3] = std::array::from_fn(|r| c[r] + (0..3).map(|k| scale * l[k].abs() * p[k] * v[k][r]).sum::<f64>());
            out_pts.extend(q);
            color.push(l[0]);
        }
        for (t, cell) in unit.polys.iter() {
            polys.push(t, &cell.iter().map(|x| x + base).collect::<Vec<_>>());
        }
    }
    let mut out = PolyData { points: Points::new_f64(out_pts), polys, ..Default::default() };
    out.point_data.insert(arr_f64("principal_value_1", 1, color));
    Ok(out)
}

/// Per-point normals of a surface: the area-weighted normals of the polygons around each point.
fn point_normals(pd: &PolyData) -> Vec<[f64; 3]> {
    let mut nrm = vec![[0.0f64; 3]; pd.points.len()];
    for (_, c) in pd.polys.iter() {
        let p: Vec<[f64; 3]> = c.iter().map(|i| pd.points.get(*i as usize)).collect();
        let mut nn = [0.0; 3];
        for k in 1..p.len().saturating_sub(1) {
            let (u, w) = ([p[k][0] - p[0][0], p[k][1] - p[0][1], p[k][2] - p[0][2]], [p[k + 1][0] - p[0][0], p[k + 1][1] - p[0][1], p[k + 1][2] - p[0][2]]);
            nn[0] += 0.5 * (u[1] * w[2] - u[2] * w[1]);
            nn[1] += 0.5 * (u[2] * w[0] - u[0] * w[2]);
            nn[2] += 0.5 * (u[0] * w[1] - u[1] * w[0]);
        }
        for i in c {
            for k in 0..3 {
                nrm[*i as usize][k] += nn[k];
            }
        }
    }
    nrm.iter()
        .map(|n| {
            let l = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
            if l > 0.0 {
                n.map(|x| x / l)
            } else {
                [0.0; 3]
            }
        })
        .collect()
}

fn surface_of(ds: &DataSet) -> Result<PolyData> {
    match ds {
        DataSet::PolyData(p) if p.polys.len() > 0 => Ok(p.clone()),
        other => crate::surface::extract_surface(other),
    }
}

/// A point vector array on a surface made tangent to it (`normal_part` keeps the normal part instead).
pub fn surface_vectors(ds: &DataSet, name: &str, normal_part: bool) -> Result<DataSet> {
    let pd = surface_of(ds)?;
    let v = pd.point_data.get(name).ok_or_else(|| anyhow!("Surface Vectors: no point vector array {name:?}"))?;
    if v.ncomp != 3 {
        bail!("Surface Vectors works on a vector array (3 components)");
    }
    let nrm = point_normals(&pd);
    let nrm = &nrm;
    let out: Vec<f64> = (0..pd.points.len())
        .flat_map(|i| {
            let x = [v.get(i, 0), v.get(i, 1), v.get(i, 2)];
            let d = x[0] * nrm[i][0] + x[1] * nrm[i][1] + x[2] * nrm[i][2];
            (0..3).map(move |k| if normal_part { d * nrm[i][k] } else { x[k] - d * nrm[i][k] })
        })
        .collect();
    let mut p = pd.clone();
    p.point_data.insert(arr_f64(&format!("{name}_{}", if normal_part { "normal" } else { "tangent" }), 3, out));
    Ok(DataSet::PolyData(p))
}

/// The flux ∫ v·n dA of a point vector array through a surface (polygons as fans, the vector
/// averaged over each triangle), as a one-row table: `Flow` and `Area`.
pub fn surface_flow(ds: &DataSet, name: &str) -> Result<Table> {
    let pd = surface_of(ds)?;
    let v = pd.point_data.get(name).ok_or_else(|| anyhow!("Surface Flow: no point vector array {name:?}"))?;
    if v.ncomp != 3 {
        bail!("Surface Flow works on a vector array (3 components)");
    }
    let (mut flow, mut area) = (0.0, 0.0);
    for (_, c) in pd.polys.iter() {
        for k in 1..c.len().saturating_sub(1) {
            let t = [c[0], c[k], c[k + 1]];
            let p = t.map(|i| pd.points.get(i as usize));
            let (u, w) = ([p[1][0] - p[0][0], p[1][1] - p[0][1], p[1][2] - p[0][2]], [p[2][0] - p[0][0], p[2][1] - p[0][1], p[2][2] - p[0][2]]);
            let an = [0.5 * (u[1] * w[2] - u[2] * w[1]), 0.5 * (u[2] * w[0] - u[0] * w[2]), 0.5 * (u[0] * w[1] - u[1] * w[0])];
            let vm: [f64; 3] = std::array::from_fn(|r| t.iter().map(|i| v.get(*i as usize, r)).sum::<f64>() / 3.0);
            flow += vm[0] * an[0] + vm[1] * an[1] + vm[2] * an[2];
            area += (an[0] * an[0] + an[1] * an[1] + an[2] * an[2]).sqrt();
        }
    }
    let mut t = Table::default();
    t.columns.insert(Array::scalars_f64("Flow", vec![flow]));
    t.columns.insert(Array::scalars_f64("Area", vec![area]));
    Ok(t)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn grid(n: i64, h: f64) -> ImageData {
        ImageData {
            extent: [0, n, 0, n, 0, n],
            origin: [-0.5; 3],
            spacing: [h; 3],
            direction: [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0],
            point_data: Attributes::new(),
            cell_data: Attributes::new(),
            field_data: Attributes::new(),
        }
    }

    fn points_of(img: &ImageData) -> Vec<[f64; 3]> {
        let n = (img.extent[1] + 1) as usize;
        (0..n * n * n)
            .map(|i| {
                let (a, b, c) = (i % n, (i / n) % n, i / (n * n));
                [img.origin[0] + a as f64 * img.spacing[0], img.origin[1] + b as f64 * img.spacing[1], img.origin[2] + c as f64 * img.spacing[2]]
            })
            .collect()
    }

    #[test]
    fn eigenvalues_and_vectors_of_a_known_tensor() {
        // Eigenvalues 5, 2, -1 along a rotated frame.
        let (c, s) = (0.6f64, 0.8f64);
        let r: M3 = [[c, -s, 0.0], [s, c, 0.0], [0.0, 0.0, 1.0]];
        let d = [5.0, -1.0, 2.0];
        let m: M3 = std::array::from_fn(|i| std::array::from_fn(|j| (0..3).map(|k| r[i][k] * d[k] * r[j][k]).sum()));
        let (l, v) = eigen_sym(m);
        assert!((l[0] - 5.0).abs() < 1e-12 && (l[1] - 2.0).abs() < 1e-12 && (l[2] + 1.0).abs() < 1e-12, "{l:?}");
        // The first direction is the rotated x axis (either sign).
        assert!((v[0][0].abs() - c).abs() < 1e-12 && (v[0][1].abs() - s).abs() < 1e-12, "{:?}", v[0]);
        for k in 0..3 {
            let mv: [f64; 3] = std::array::from_fn(|i| (0..3).map(|j| m[i][j] * v[k][j]).sum());
            assert!((0..3).all(|i| (mv[i] - l[k] * v[k][i]).abs() < 1e-10));
        }
    }

    #[test]
    fn yield_criteria_of_uniaxial_stress_and_pure_shear() {
        let mut pd = PolyData { points: Points::new_f64(vec![0.0; 6]), ..Default::default() };
        // Point 0: uniaxial 100 along x; point 1: pure shear 50 in xy.
        pd.point_data.insert(Array::new("stress", 6, ArrayData::F64(vec![100.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 50.0, 0.0, 0.0])));
        let out = yield_criteria(&DataSet::PolyData(pd), Association::Points, "stress").unwrap();
        let DataSet::PolyData(o) = out else { panic!() };
        let g = |n: &str, i: usize| o.point_data.get(n).unwrap().get(i, 0);
        assert!((g("von_Mises", 0) - 100.0).abs() < 1e-9 && (g("Tresca", 0) - 100.0).abs() < 1e-9);
        assert!((g("von_Mises", 1) - 3f64.sqrt() * 50.0).abs() < 1e-9 && (g("Tresca", 1) - 100.0).abs() < 1e-9);
        assert!((g("max_principal", 1) - 50.0).abs() < 1e-9 && (g("pressure", 0) + 100.0 / 3.0).abs() < 1e-9);
        assert!((g("triaxiality", 0) - 1.0 / 3.0).abs() < 1e-9);
    }

    #[test]
    fn derivatives_of_a_rigid_rotation_and_a_stretch() {
        // v = ω × r with ω = (0, 0, 2): vorticity 2ω, no strain, no divergence, Q > 0.
        let mut img = grid(8, 0.125);
        let p = points_of(&img);
        let v: Vec<f64> = p.iter().flat_map(|r| [-2.0 * r[1], 2.0 * r[0], 0.0]).collect();
        img.point_data.insert(Array::new("v", 3, ArrayData::F64(v)));
        let out = compute_derivatives(&DataSet::ImageData(img), "v").unwrap();
        let (_, pd, _, _) = parts(&out).unwrap();
        let w = pd.get("v_vorticity").unwrap();
        let s = pd.get("v_strain_rate").unwrap();
        for i in (0..w.len()).step_by(37) {
            assert!((w.get(i, 2) - 4.0).abs() < 1e-9 && w.get(i, 0).abs() < 1e-9, "{}", w.get(i, 2));
            assert!((0..6).all(|k| s.get(i, k).abs() < 1e-9));
            assert!(pd.get("v_divergence").unwrap().get(i, 0).abs() < 1e-9);
            assert!((pd.get("Q_criterion").unwrap().get(i, 0) - 4.0).abs() < 1e-9, "Q = |Ω|²/2 = 4");
            assert!(pd.get("lambda2").unwrap().get(i, 0) < 0.0, "inside a vortex core");
        }
        // v = (3x, -y, 0): strain XX 3, YY -1, divergence 2, no vorticity.
        let mut img = grid(6, 0.2);
        let p = points_of(&img);
        img.point_data.insert(Array::new("u", 3, ArrayData::F64(p.iter().flat_map(|r| [3.0 * r[0], -r[1], 0.0]).collect())));
        let out = compute_derivatives(&DataSet::ImageData(img), "u").unwrap();
        let (_, pd, _, _) = parts(&out).unwrap();
        let s = pd.get("u_strain_rate").unwrap();
        assert!((s.get(10, 0) - 3.0).abs() < 1e-9 && (s.get(10, 1) + 1.0).abs() < 1e-9);
        assert!((pd.get("u_divergence").unwrap().get(10, 0) - 2.0).abs() < 1e-9);
        assert!(pd.get("u_vorticity").unwrap().get(10, 2).abs() < 1e-9);
    }

    #[test]
    fn principal_invariants_and_glyphs() {
        let mut pd = PolyData { points: Points::new_f64(vec![0.0, 0.0, 0.0, 5.0, 0.0, 0.0]), ..Default::default() };
        pd.point_data.insert(Array::new(
            "T",
            9,
            ArrayData::F64(vec![2.0, 0.0, 0.0, 0.0, 3.0, 0.0, 0.0, 0.0, 4.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0]),
        ));
        let out = principal_invariants(&DataSet::PolyData(pd.clone()), Association::Points, "T").unwrap();
        let DataSet::PolyData(o) = out else { panic!() };
        let pv = o.point_data.get("T_principal_values").unwrap();
        assert_eq!([pv.get(0, 0), pv.get(0, 1), pv.get(0, 2)], [4.0, 3.0, 2.0]);
        assert_eq!(o.point_data.get("T_I1").unwrap().get(0, 0), 9.0);
        assert_eq!(o.point_data.get("T_I3").unwrap().get(0, 0), 24.0);
        assert!(o.point_data.get("T_J2").unwrap().get(1, 0).abs() < 1e-12, "an isotropic tensor has no deviator");
        // Glyphs: an ellipsoid of semi-axes 4, 3, 2 (scale 1) at the first point.
        let g = tensor_glyph(&DataSet::PolyData(pd), "T", 1.0, 1, 24).unwrap();
        let half = g.points.len() / 2;
        let ext: [f64; 3] = std::array::from_fn(|k| (0..half).map(|i| g.points.get(i)[k].abs()).fold(0.0, f64::max));
        // (the sphere's sample points come within a few percent of its axes)
        assert!((ext[2] - 4.0).abs() < 0.05 * 4.0 && (ext[1] - 3.0).abs() < 0.05 * 3.0 && (ext[0] - 2.0).abs() < 0.05 * 2.0, "{ext:?}");
    }

    #[test]
    fn surface_flow_through_a_closed_sphere_is_the_divergence_integral() {
        // v = r through a sphere of radius 1: flux = 3 × volume ≈ 4π.
        let s = crate::sources::sphere([0.0; 3], 1.0, 96, 64);
        let v: Vec<f64> = (0..s.points.len()).flat_map(|i| s.points.get(i)).collect();
        let mut s2 = s.clone();
        s2.point_data.insert(Array::new("v", 3, ArrayData::F64(v)));
        let t = surface_flow(&DataSet::PolyData(s2.clone()), "v").unwrap();
        let flow = t.columns.get("Flow").unwrap().get(0, 0);
        assert!((flow.abs() - 4.0 * std::f64::consts::PI).abs() < 0.02 * 4.0 * std::f64::consts::PI, "{flow}");
        // A radial field on a sphere has no tangent part.
        let tv = surface_vectors(&DataSet::PolyData(s2), "v", false).unwrap();
        let DataSet::PolyData(o) = tv else { panic!() };
        let tg = o.point_data.get("v_tangent").unwrap();
        assert!((0..tg.len()).all(|i| (tg.get(i, 0).powi(2) + tg.get(i, 1).powi(2) + tg.get(i, 2).powi(2)).sqrt() < 0.02));
    }
}
