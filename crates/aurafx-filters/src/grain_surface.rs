//! Grain boundary surfaces: the faces between voxels of different features as a triangle
//! mesh (each triangle knowing the features on its two sides, each node how many features
//! meet there), smoothing that respects those node types, triangle measures, the closed
//! surface of each feature, and face colours by orientation.

use crate::features::{image, Vox};
use anyhow::{anyhow, bail, Result};
use aurafx_data::*;
use std::collections::HashMap;

/// Label of the space outside the volume.
pub const OUTSIDE: i32 = -1;

fn cell_ids(img: &ImageData, name: &str) -> Result<Vec<i32>> {
    let a = img.cell_data.get(name).ok_or_else(|| anyhow!("no cell array {name} (run Segment Features first)"))?;
    Ok((0..a.len()).map(|i| a.get(i, 0) as i32).collect())
}

/// Quick Surface Mesh: two triangles for every voxel face between different features (and
/// between a feature and the outside). `FaceLabels` (cell data, 2 components) are the
/// features on the two sides, the triangle's normal pointing from the first into the second
/// (-1: outside). `NodeType` (point data): the number of features meeting at the node (2 on
/// a boundary, 3 on a triple line, 4 or more at a quadruple point), plus 10 on the outer
/// surface of the volume.
pub fn quick_surface_mesh(ds: &DataSet, ids_name: &str) -> Result<DataSet> {
    let img = image(ds)?;
    let v = Vox::of(img);
    if v.n.iter().any(|n| *n < 1) || v.flat.iter().any(|f| *f) {
        bail!("Quick Surface Mesh needs a volume of cells (a 2-D map has boundary lines: use Feature Boundaries)");
    }
    let ids = cell_ids(img, ids_name)?;
    let n = v.n;
    let lab = |i: i64, j: i64, k: i64| -> i32 {
        if i < 0 || j < 0 || k < 0 || i >= n[0] as i64 || j >= n[1] as i64 || k >= n[2] as i64 {
            OUTSIDE
        } else {
            ids[i as usize + n[0] * (j as usize + n[1] * k as usize)]
        }
    };
    let node_index = |i: usize, j: usize, k: usize| i + (n[0] + 1) * (j + (n[1] + 1) * k);
    let mut node_of: HashMap<usize, u32> = HashMap::new();
    let mut pts: Vec<f64> = Vec::new();
    let mut node_ijk: Vec<[usize; 3]> = Vec::new();
    let mut tris: Vec<[u32; 3]> = Vec::new();
    let mut labels: Vec<i32> = Vec::new();
    let mut id = |q: [usize; 3], pts: &mut Vec<f64>, node_ijk: &mut Vec<[usize; 3]>| -> u32 {
        *node_of.entry(node_index(q[0], q[1], q[2])).or_insert_with(|| {
            pts.extend((0..3).map(|a| v.corner[a] + q[a] as f64 * v.sp[a]));
            node_ijk.push(q);
            (node_ijk.len() - 1) as u32
        })
    };
    for a in 0..3 {
        let (b, c) = ((a + 1) % 3, (a + 2) % 3);
        // Faces normal to axis a at plane index p (between cells p-1 and p).
        for p in 0..=n[a] {
            for jb in 0..n[b] {
                for jc in 0..n[c] {
                    let mut lo = [0i64; 3];
                    lo[a] = p as i64 - 1;
                    lo[b] = jb as i64;
                    lo[c] = jc as i64;
                    let mut hi = lo;
                    hi[a] += 1;
                    let (l0, l1) = (lab(lo[0], lo[1], lo[2]), lab(hi[0], hi[1], hi[2]));
                    if l0 == l1 {
                        continue;
                    }
                    // The face's corners, counter-clockwise seen from +a (normal along +a).
                    let corner = |db: usize, dc: usize| -> [usize; 3] {
                        let mut q = [0usize; 3];
                        q[a] = p;
                        q[b] = jb + db;
                        q[c] = jc + dc;
                        q
                    };
                    let k = [corner(0, 0), corner(1, 0), corner(1, 1), corner(0, 1)].map(|q| id(q, &mut pts, &mut node_ijk));
                    // Normal along +a: from the cell below (l0) into the cell above (l1).
                    tris.push([k[0], k[1], k[2]]);
                    tris.push([k[0], k[2], k[3]]);
                    labels.extend([l0, l1, l0, l1]);
                }
            }
        }
    }
    if tris.is_empty() {
        bail!("the volume holds a single feature: no boundaries");
    }
    // Node types: distinct features among the eight cells around a node, +10 on the outside.
    let types: Vec<i8> = node_ijk
        .iter()
        .map(|q| {
            let mut seen: Vec<i32> = Vec::with_capacity(8);
            let mut outside = false;
            for d in 0..8 {
                let l = lab(q[0] as i64 - 1 + (d & 1) as i64, q[1] as i64 - 1 + ((d >> 1) & 1) as i64, q[2] as i64 - 1 + ((d >> 2) & 1) as i64);
                if l == OUTSIDE {
                    outside = true;
                } else if !seen.contains(&l) {
                    seen.push(l);
                }
            }
            let t = seen.len().clamp(2, 4) as i8;
            if outside { t + 10 } else { t }
        })
        .collect();
    let mut polys = Cells::new();
    for t in &tris {
        polys.push(CellType::Triangle, t);
    }
    let mut pd = PolyData { points: Points::new_f64(pts), polys, ..Default::default() };
    pd.cell_data.insert(Array::new("FaceLabels", 2, ArrayData::I32(labels)));
    pd.point_data.insert(Array::new("NodeType", 1, ArrayData::I8(types)));
    // Keep the volume's per-feature and ensemble arrays, and add each feature's mean
    // orientation (AvgQuats, x y z w) and phase (FeaturePhases) when the volume has
    // orientations, for the face colours and the GBCD.
    for a in img.field_data.iter() {
        pd.field_data.insert(a.clone());
    }
    let nf = ids.iter().copied().max().unwrap_or(0).max(0) as usize + 1;
    if let Ok(o) = crate::features::Orients::of(ds, None, crate::crystal::FROM_PHASES, None) {
        let q = crate::features::average_quats(&o, &ids, nf);
        let mut phase = vec![0i32; nf];
        for (c, f) in ids.iter().enumerate() {
            if *f > 0 && o.laue[c] != 255 {
                phase[*f as usize] = o.phase[c];
            }
        }
        let vals: Vec<f64> = q.iter().flat_map(|q| match q {
            Some(q) => cx_xyzw(*q),
            None => [0.0, 0.0, 0.0, 1.0],
        }).collect();
        pd.field_data.insert(Array::new("AvgQuats", 4, ArrayData::F64(vals)));
        pd.field_data.insert(Array::new("FeaturePhases", 1, ArrayData::I32(phase)));
    }
    Ok(DataSet::PolyData(pd))
}

fn surface(ds: &DataSet) -> Result<&PolyData> {
    match ds {
        DataSet::PolyData(p) => Ok(p),
        _ => bail!("this filter works on a boundary surface (from Quick Surface Mesh)"),
    }
}

fn triangles(pd: &PolyData) -> Result<Vec<[u32; 3]>> {
    let mut out = Vec::new();
    for (_, c) in pd.polys.iter() {
        if c.len() != 3 {
            bail!("the surface has a polygon with {} corners: only triangles", c.len());
        }
        out.push([c[0], c[1], c[2]]);
    }
    Ok(out)
}

pub struct SmoothParams {
    pub iterations: usize,
    pub lambda: f64,
    /// Taubin's inflating step (0: plain Laplacian smoothing).
    pub mu: f64,
    pub triple_lambda: f64,
    pub quad_lambda: f64,
    pub smooth_outer: bool,
}

/// Laplacian / Taubin Smoothing of a boundary surface: each node moves towards the mean of
/// its neighbours by `lambda` (then back by `mu` on alternate passes); nodes on triple lines
/// only follow their triple-line neighbours, with `triple_lambda`; quadruple points move by
/// `quad_lambda`; nodes on the outer surface stay on the volume's faces (and stay put unless
/// `smooth_outer`).
pub fn smooth(ds: &DataSet, p: &SmoothParams) -> Result<DataSet> {
    let pd = surface(ds)?;
    let tris = triangles(pd)?;
    let np = pd.points.len();
    let types: Vec<i8> = match pd.point_data.get("NodeType") {
        Some(a) => (0..np).map(|i| a.get(i, 0) as i8).collect(),
        None => vec![2; np],
    };
    let mut pts: Vec<[f64; 3]> = (0..np).map(|i| pd.points.get(i)).collect();
    let (lo, hi) = pts.iter().fold(([f64::INFINITY; 3], [f64::NEG_INFINITY; 3]), |(l, h), q| (std::array::from_fn(|a| l[a].min(q[a])), std::array::from_fn(|a| h[a].max(q[a]))));
    let eps = 1e-9 * (0..3).map(|a| hi[a] - lo[a]).fold(0.0, f64::max).max(1e-300);
    // Axes along which a node lies on the volume's faces (fixed there).
    let on_face: Vec<[bool; 3]> = pts.iter().map(|q| std::array::from_fn(|a| (q[a] - lo[a]).abs() < eps || (q[a] - hi[a]).abs() < eps)).collect();
    let mut nbr: Vec<Vec<u32>> = vec![Vec::new(); np];
    for t in &tris {
        for k in 0..3 {
            let (x, y) = (t[k], t[(k + 1) % 3]);
            if !nbr[x as usize].contains(&y) {
                nbr[x as usize].push(y);
            }
            if !nbr[y as usize].contains(&x) {
                nbr[y as usize].push(x);
            }
        }
    }
    // Neighbours a node follows: on a triple line or quadruple point (or the outer surface),
    // those of the same kind or higher.
    let rank = |t: i8| -> i8 { t % 10 };
    let follow: Vec<Vec<u32>> = (0..np)
        .map(|i| {
            let ti = rank(types[i]);
            let outer = types[i] >= 10;
            nbr[i].iter().copied().filter(|j| rank(types[*j as usize]) >= ti && (!outer || types[*j as usize] >= 10)).collect()
        })
        .collect();
    let step_of = |i: usize| -> f64 {
        let t = types[i];
        if t >= 10 && !p.smooth_outer {
            return 0.0;
        }
        match rank(t) {
            0..=2 => 1.0,
            3 => p.triple_lambda / p.lambda.max(1e-12),
            _ => p.quad_lambda / p.lambda.max(1e-12),
        }
    };
    for it in 0..p.iterations {
        let f = if p.mu != 0.0 && it % 2 == 1 { p.mu } else { p.lambda };
        let next: Vec<[f64; 3]> = (0..np)
            .map(|i| {
                let s = step_of(i) * f;
                if s == 0.0 || follow[i].is_empty() {
                    return pts[i];
                }
                let m: [f64; 3] = std::array::from_fn(|a| follow[i].iter().map(|j| pts[*j as usize][a]).sum::<f64>() / follow[i].len() as f64);
                std::array::from_fn(|a| if on_face[i][a] { pts[i][a] } else { pts[i][a] + s * (m[a] - pts[i][a]) })
            })
            .collect();
        pts = next;
    }
    let mut out = pd.clone();
    out.points = Points::new_f64(pts.into_iter().flatten().collect());
    Ok(DataSet::PolyData(out))
}

fn labels_of(pd: &PolyData) -> Result<Vec<[i32; 2]>> {
    let a = pd.cell_data.get("FaceLabels").ok_or_else(|| anyhow!("no FaceLabels (make the surface with Quick Surface Mesh)"))?;
    if a.ncomp != 2 {
        bail!("FaceLabels must have two components");
    }
    let nv = pd.verts.len() + pd.lines.len();
    Ok((0..pd.polys.len()).map(|i| [a.get(nv + i, 0) as i32, a.get(nv + i, 1) as i32]).collect())
}

fn sub(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}
fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

/// Triangle Geometry: per triangle its area (FaceAreas), unit normal (FaceNormals), centroid
/// (FaceCentroids) and smallest corner angle in degrees (FaceMinimumAngles); per feature (field
/// data, one row per feature id) the enclosed volume (SurfaceVolumes) and boundary area
/// (SurfaceAreas), counting each triangle for both features beside it.
pub fn triangle_geometry(ds: &DataSet) -> Result<DataSet> {
    let pd = surface(ds)?;
    let tris = triangles(pd)?;
    let pts: Vec<[f64; 3]> = (0..pd.points.len()).map(|i| pd.points.get(i)).collect();
    let (mut area, mut normal, mut cen, mut minang) = (Vec::new(), Vec::new(), Vec::new(), Vec::new());
    for t in &tris {
        let p = t.map(|k| pts[k as usize]);
        let c = cross(sub(p[1], p[0]), sub(p[2], p[0]));
        let l = dot(c, c).sqrt();
        area.push(0.5 * l);
        normal.extend(c.map(|x| if l > 0.0 { x / l } else { 0.0 }));
        cen.extend((0..3).map(|a| (p[0][a] + p[1][a] + p[2][a]) / 3.0));
        let ang = (0..3)
            .map(|k| {
                let (u, w) = (sub(p[(k + 1) % 3], p[k]), sub(p[(k + 2) % 3], p[k]));
                let d = (dot(u, u) * dot(w, w)).sqrt();
                if d > 0.0 { (dot(u, w) / d).clamp(-1.0, 1.0).acos().to_degrees() } else { 0.0 }
            })
            .fold(f64::INFINITY, f64::min);
        minang.push(ang);
    }
    let mut out = pd.clone();
    let nv = pd.verts.len() + pd.lines.len();
    let pad = |v: Vec<f64>, nc: usize| -> Vec<f64> { std::iter::repeat_n(0.0, nv * nc).chain(v).collect() };
    out.cell_data.insert(Array::scalars_f64("FaceAreas", pad(area.clone(), 1)));
    out.cell_data.insert(Array::new("FaceNormals", 3, ArrayData::F64(pad(normal, 3))));
    out.cell_data.insert(Array::new("FaceCentroids", 3, ArrayData::F64(pad(cen, 3))));
    out.cell_data.insert(Array::scalars_f64("FaceMinimumAngles", pad(minang, 1)));
    if let Ok(labels) = labels_of(pd) {
        let nf = labels.iter().flat_map(|l| l.iter().copied()).max().unwrap_or(0).max(0) as usize + 1;
        let mut vol = vec![0.0; nf];
        let mut sa = vec![0.0; nf];
        for (t, l) in tris.iter().zip(&labels) {
            let p = t.map(|k| pts[k as usize]);
            let v6 = dot(p[0], cross(p[1], p[2])) / 6.0;
            let a = 0.5 * dot(cross(sub(p[1], p[0]), sub(p[2], p[0])), cross(sub(p[1], p[0]), sub(p[2], p[0]))).sqrt();
            // The normal points out of l[0] and into l[1].
            if l[0] >= 0 {
                vol[l[0] as usize] += v6;
                sa[l[0] as usize] += a;
            }
            if l[1] >= 0 {
                vol[l[1] as usize] -= v6;
                sa[l[1] as usize] += a;
            }
        }
        out.field_data.insert(Array::scalars_f64("SurfaceVolumes", vol));
        out.field_data.insert(Array::scalars_f64("SurfaceAreas", sa));
    }
    Ok(DataSet::PolyData(out))
}

/// Feature Surfaces: the closed surface of each feature (its triangles, turned to face
/// outwards), as blocks named `feature N`; `which` lists the features (empty: all).
pub fn feature_surfaces(ds: &DataSet, which: &[i64]) -> Result<DataSet> {
    let pd = surface(ds)?;
    let tris = triangles(pd)?;
    let labels = labels_of(pd)?;
    let mut features: Vec<i32> = labels.iter().flat_map(|l| l.iter().copied()).filter(|f| *f >= 0).collect();
    features.sort_unstable();
    features.dedup();
    if !which.is_empty() {
        features.retain(|f| which.contains(&(*f as i64)));
    }
    if features.is_empty() {
        bail!("none of those features is on the surface");
    }
    let mut by_feature: HashMap<i32, Vec<[u32; 3]>> = HashMap::new();
    for (t, l) in tris.iter().zip(&labels) {
        if l[0] >= 0 {
            by_feature.entry(l[0]).or_default().push(*t);
        }
        if l[1] >= 0 {
            by_feature.entry(l[1]).or_default().push([t[0], t[2], t[1]]);
        }
    }
    let mut mb = MultiBlock::default();
    for f in features {
        let ts = &by_feature[&f];
        let mut map: HashMap<u32, u32> = HashMap::new();
        let mut pts = Vec::new();
        let mut polys = Cells::new();
        for t in ts {
            let k = t.map(|i| {
                *map.entry(i).or_insert_with(|| {
                    pts.extend(pd.points.get(i as usize));
                    (pts.len() / 3 - 1) as u32
                })
            });
            polys.push(CellType::Triangle, &k);
        }
        let mut s = PolyData { points: Points::new_f64(pts), polys, ..Default::default() };
        s.field_data.insert(Array::new("FeatureId", 1, ArrayData::I32(vec![f])));
        mb.blocks.push((format!("feature {f}"), Some(DataSet::PolyData(s))));
    }
    Ok(DataSet::MultiBlock(mb))
}

fn cx_xyzw(q: aurafx_crystal::Quat) -> [f64; 4] {
    aurafx_crystal::quat_to_xyzw(q)
}

/// Per feature: orientation and crystal class (from AvgQuats, FeaturePhases and
/// CrystalStructures in the surface's field data).
fn feature_crystals(pd: &PolyData) -> Result<Vec<Option<(aurafx_crystal::Quat, aurafx_crystal::Laue)>>> {
    let q = pd.field_data.get("AvgQuats").ok_or_else(|| anyhow!("the surface has no feature orientations (make it with Quick Surface Mesh from a volume with orientations)"))?;
    let ph = pd.field_data.get("FeaturePhases");
    let cs = pd.field_data.get("CrystalStructures");
    Ok((0..q.len())
        .map(|f| {
            let p = ph.map(|a| a.get(f, 0) as usize).unwrap_or(1);
            if f == 0 || p == 0 {
                return None;
            }
            let code = cs.and_then(|a| (p < a.len()).then(|| a.get(p, 0) as u32)).unwrap_or(aurafx_crystal::Laue::CubicHigh.code());
            let l = aurafx_crystal::Laue::from_code(code)?;
            let quat = aurafx_crystal::quat_from_xyzw([q.get(f, 0), q.get(f, 1), q.get(f, 2), q.get(f, 3)]);
            Some((quat, l))
        })
        .collect())
}

fn unit_normal(p: [[f64; 3]; 3]) -> ([f64; 3], f64) {
    let c = cross(sub(p[1], p[0]), sub(p[2], p[0]));
    let l = dot(c, c).sqrt();
    (if l > 0.0 { c.map(|x| x / l) } else { [0.0; 3] }, 0.5 * l)
}

/// Face Orientation Colors: per triangle, the inverse pole figure color of its normal in the
/// crystal frame of the feature it points out of, or of the feature inside on the outer
/// surface (FaceIPFColors), the disorientation angle
/// across it in degrees (FaceMisorientations), and a color of that disorientation
/// (FaceMisorientationColors: the axis's place in the standard triangle as hue, darker for
/// smaller angles; black on the outer surface and between features of different classes).
pub fn face_orientation_colors(ds: &DataSet) -> Result<DataSet> {
    use aurafx_crystal as cx;
    let pd = surface(ds)?;
    let tris = triangles(pd)?;
    let labels = labels_of(pd)?;
    let fc = feature_crystals(pd)?;
    let get = |f: i32| -> Option<(cx::Quat, cx::Laue)> { (f > 0).then(|| fc.get(f as usize).copied().flatten()).flatten() };
    let mut ipf = Vec::with_capacity(tris.len() * 3);
    let mut ang = Vec::with_capacity(tris.len());
    let mut mcol = Vec::with_capacity(tris.len() * 3);
    for (t, l) in tris.iter().zip(&labels) {
        let p = t.map(|k| pd.points.get(k as usize));
        let (n, _) = unit_normal(p);
        // The plane seen from the feature the normal points out of (on the outer surface,
        // from the feature inside).
        match (get(l[0]), get(l[1])) {
            (Some((q, laue)), _) => ipf.extend(cx::ipf_color(q, laue, n)),
            (None, Some((q, laue))) => ipf.extend(cx::ipf_color(q, laue, n.map(|x| -x))),
            _ => ipf.extend([0u8; 3]),
        }
        match (get(l[0]), get(l[1])) {
            (Some((a, la)), Some((b, lb))) if la == lb => {
                let (w, axis) = cx::disorientation(a, b, la);
                ang.push(w.to_degrees());
                let max = cx::texture::random_disorientations(la).max_angle();
                let f = (w / max).clamp(0.0, 1.0);
                let rgb = match cx::ipf::to_sector(axis, la) {
                    Some((eta, chi)) => cx::ipf::sector_color(&cx::ipf_sector(la), eta, chi),
                    None => [1.0; 3],
                };
                mcol.extend(rgb.map(|x| (x * f * 255.0).round() as u8));
            }
            _ => {
                ang.push(f64::NAN);
                mcol.extend([0u8; 3]);
            }
        }
    }
    let mut out = pd.clone();
    let nv = pd.verts.len() + pd.lines.len();
    let padu = |v: Vec<u8>| -> Vec<u8> { std::iter::repeat_n(0u8, nv * 3).chain(v).collect() };
    out.cell_data.insert(Array::new("FaceIPFColors", 3, ArrayData::U8(padu(ipf))));
    out.cell_data.insert(Array::new("FaceMisorientationColors", 3, ArrayData::U8(padu(mcol))));
    out.cell_data.insert(Array::scalars_f64("FaceMisorientations", std::iter::repeat_n(f64::NAN, nv).chain(ang).collect()));
    Ok(DataSet::PolyData(out))
}

/// The boundary plane normals (crystal frame of the first grain, both signs) of every
/// symmetry-equivalent description of the boundaries whose misorientation lies within `tol`
/// of `target`, weighted by area, and the area a random microstructure would give.
pub fn gbcd_normals(pd: &PolyData, target: aurafx_crystal::Quat, tol: f64) -> Result<(Vec<[f64; 3]>, Vec<f64>, f64, f64)> {
    use aurafx_crystal as cx;
    use rayon::prelude::*;
    let tris = triangles(pd)?;
    let labels = labels_of(pd)?;
    let fc = feature_crystals(pd)?;
    let get = |f: i32| -> Option<(cx::Quat, cx::Laue)> { (f > 0).then(|| fc.get(f as usize).copied().flatten()).flatten() };
    let target_angle = cx::to_axis_angle(target).0;
    let cos_tol = (tol / 2.0).cos();
    let mut laue_seen = None;
    let parts: Vec<(Vec<[f64; 3]>, Vec<f64>, f64)> = tris
        .par_iter()
        .zip(labels.par_iter())
        .map(|(t, l)| {
            let mut ns = Vec::new();
            let mut ws = Vec::new();
            let (Some((qa, la)), Some((qb, lb))) = (get(l[0]), get(l[1])) else { return (ns, ws, 0.0) };
            if la != lb {
                return (ns, ws, 0.0);
            }
            let p = t.map(|k| pd.points.get(k as usize));
            let (n, area) = unit_normal(p);
            // Both ways round: from a (normal n) and from b (normal -n).
            for (g1, g2, nn) in [(qa, qb, n), (qb, qa, n.map(|x| -x))] {
                let delta = cx::mul(g2, cx::conj(g1));
                // Some equivalent is within tol of the target only if the smallest of them
                // (the disorientation) is not larger than the target's angle plus tol.
                if cx::disorientation_angle(g1, g2, la) > target_angle + tol + 1e-9 {
                    continue;
                }
                let ops = la.operators();
                for s1 in ops {
                    let n1 = cx::rotate(cx::mul(*s1, g1), nn);
                    for s2 in ops {
                        let d = cx::mul(cx::mul(*s2, delta), cx::conj(*s1));
                        let c = (d[0] * target[0] + d[1] * target[1] + d[2] * target[2] + d[3] * target[3]).abs();
                        if c >= cos_tol {
                            ns.push(n1);
                            ws.push(area);
                            ns.push(n1.map(|x| -x));
                            ws.push(area);
                        }
                    }
                }
            }
            (ns, ws, area)
        })
        .collect();
    let mut normals = Vec::new();
    let mut weights = Vec::new();
    let mut total_area = 0.0;
    for (n, w, a) in parts {
        normals.extend(n);
        weights.extend(w);
        total_area += a;
    }
    for l in labels.iter() {
        if let (Some((_, la)), Some(_)) = (get(l[0]), get(l[1])) {
            laue_seen = Some(la);
            break;
        }
    }
    let laue = laue_seen.ok_or_else(|| anyhow!("no boundaries between two oriented features"))?;
    let g = laue.operators().len() as f64;
    // Random misorientations: each ordering (2) and each of the |G|^2 descriptions falls within
    // tol with the probability (tol - sin tol) / pi; each match adds both signs (2).
    let random = 2.0 * 2.0 * total_area * g * g * (tol - tol.sin()) / std::f64::consts::PI;
    Ok((normals, weights, total_area, random))
}

/// GBCD Section: the distribution of boundary planes (crystal frame) for boundaries of one
/// misorientation, in multiples of random, as a disk (stereographic or equal area) of the
/// upper hemisphere of crystal directions (x right, y up, z at the centre).
pub fn gbcd_section(ds: &DataSet, angle_deg: f64, axis: [f64; 3], tol_deg: f64, sigma_deg: f64, projection: &str, resolution: usize) -> Result<DataSet> {
    use aurafx_crystal as cx;
    let pd = surface(ds)?;
    let l = dot(axis, axis).sqrt();
    if l == 0.0 {
        bail!("the misorientation axis is zero");
    }
    let target = cx::axis_angle_to_quat(axis.map(|x| x / l), angle_deg.to_radians());
    let (normals, weights, _total, random) = gbcd_normals(pd, target, tol_deg.to_radians())?;
    let n = resolution.clamp(4, 400);
    let mut pts = vec![[0.0, 0.0, 0.0]];
    let mut dirs = vec![[0.0, 0.0, 1.0]];
    let m = 4 * n;
    for k in 1..=n {
        let r = k as f64 / n as f64;
        let t = if projection == crate::crystal_texture::EQUAL_AREA { 2.0 * (r / std::f64::consts::SQRT_2).asin() } else { 2.0 * r.atan() };
        for j in 0..m {
            let ph = j as f64 / m as f64 * std::f64::consts::TAU;
            pts.push([r * ph.cos(), r * ph.sin(), 0.0]);
            dirs.push([t.sin() * ph.cos(), t.sin() * ph.sin(), t.cos()]);
        }
    }
    let matched: f64 = weights.iter().sum();
    let values: Vec<f64> = if normals.is_empty() {
        vec![0.0; dirs.len()]
    } else {
        let bins = cx::texture::bin_directions(&normals, Some(&weights), 2.0);
        let dens = cx::texture::sphere_density(&bins, &dirs, sigma_deg.to_radians());
        dens.into_iter().map(|d| d * matched / random).collect()
    };
    let id = |k: usize, j: usize| (1 + (k - 1) * m + j % m) as u32;
    let mut cells = Cells::new();
    for j in 0..m {
        cells.push(CellType::Triangle, &[0, id(1, j), id(1, j + 1)]);
    }
    for k in 1..n {
        for j in 0..m {
            cells.push(CellType::Quad, &[id(k, j), id(k + 1, j), id(k + 1, j + 1), id(k, j + 1)]);
        }
    }
    let mut out = PolyData { points: Points::new_f64(pts.into_iter().flatten().collect()), polys: cells, ..Default::default() };
    let max = values.iter().cloned().fold(0.0, f64::max);
    out.point_data.insert(Array::scalars_f64("MRD", values));
    out.point_data.insert(Array::new("CrystalDirection", 3, ArrayData::F64(dirs.into_iter().flatten().collect())));
    out.field_data.insert(Array::scalars_f64("MaxMRD", vec![max]));
    out.field_data.insert(Array::scalars_f64("MisorientationMRD", vec![matched / random]));
    Ok(DataSet::PolyData(out))
}
