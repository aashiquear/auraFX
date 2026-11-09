//! More microstructure tools: transformation phases (plates such as twins put inside grains),
//! adjusting feature volumes towards targets, precipitate statistics, surface area to volume
//! (and sphericity) per feature, triangle angles and per-boundary curvature of grain surface
//! meshes.

use crate::features::{average_quats, image, Orients, Vox};
use anyhow::{anyhow, bail, Result};
use aurafx_crystal::rotation::{axis_angle_to_quat, conj, mul, quat_to_euler, rotate};
use aurafx_data::*;

fn ids_of(img: &ImageData, name: &str) -> Result<Vec<i32>> {
    let a = img.cell_data.get(name).ok_or_else(|| anyhow!("no cell array {name} (run Segment Features first)"))?;
    Ok((0..a.len()).map(|k| a.get(k, 0) as i32).collect())
}

/// Insert Transformation Phases: into a fraction of the grains of `parent_phase`, a plate of
/// `thickness` (in cells) through the grain's centre (shifted at random by up to a third of the
/// grain), on one of the parent's {111} planes, becomes a new feature of `new_phase` whose
/// orientation is the parent's turned by `angle` degrees about that plane's normal (60: a twin).
pub fn insert_transformation_phases(
    ds: &DataSet,
    ids_name: &str,
    parent_phase: i32,
    new_phase: i32,
    fraction: f64,
    thickness: f64,
    angle: f64,
    seed: u64,
) -> Result<DataSet> {
    let img = image(ds)?;
    let v = Vox::of(img);
    let mut ids = ids_of(img, ids_name)?;
    let nf = ids.iter().copied().max().unwrap_or(0).max(0) as usize + 1;
    let o = Orients::of(ds, None, crate::crystal::structure_or_phases(""), None)?;
    let avg = average_quats(&o, &ids, nf);
    let phases: Vec<i32> = img.cell_data.get("Phases").map(|a| (0..a.len()).map(|i| a.get(i, 0) as i32).collect()).unwrap_or_else(|| vec![1; ids.len()]);
    let eul = img.cell_data.get("EulerAngles").ok_or_else(|| anyhow!("no EulerAngles cell array"))?;
    let mut euler: Vec<[f64; 3]> = (0..eul.len()).map(|i| [eul.get(i, 0), eul.get(i, 1), eul.get(i, 2)]).collect();
    let mut new_phases = phases.clone();
    // Feature centres and sizes.
    let mut cnt = vec![0usize; nf];
    let mut ctr = vec![[0.0; 3]; nf];
    let mut fphase = vec![0i32; nf];
    for (c, &f) in ids.iter().enumerate() {
        if f > 0 {
            let p = v.center(c);
            cnt[f as usize] += 1;
            for a in 0..3 {
                ctr[f as usize][a] += p[a];
            }
            fphase[f as usize] = phases[c];
        }
    }
    let mut rng = crate::stats::Rng::new(seed);
    let mut next = nf as i32;
    let cell = v.sp.iter().copied().fold(f64::INFINITY, f64::min);
    let half = 0.5 * thickness.max(0.5) * cell;
    let normals = [[1.0, 1.0, 1.0], [-1.0, 1.0, 1.0], [1.0, -1.0, 1.0], [1.0, 1.0, -1.0]].map(|n: [f64; 3]| n.map(|x| x / 3f64.sqrt()));
    let mut members: Vec<Vec<usize>> = vec![Vec::new(); nf];
    for (c, &f) in ids.iter().enumerate() {
        if f > 0 {
            members[f as usize].push(c);
        }
    }
    let mut parents = Vec::new();
    for f in 1..nf {
        if cnt[f] == 0 || (parent_phase > 0 && fphase[f] != parent_phase) || rng.next_f64() >= fraction {
            continue;
        }
        let Some(q) = avg[f] else { continue };
        let c0 = ctr[f].map(|x| x / cnt[f] as f64);
        let size = (cnt[f] as f64).cbrt() * cell;
        let shift: [f64; 3] = std::array::from_fn(|_| (rng.next_f64() - 0.5) * size / 1.5);
        let nc = normals[(rng.next_f64() * 4.0) as usize % 4];
        // The plane normal in the sample frame.
        let ns = rotate(conj(q), nc);
        let twin = mul(axis_angle_to_quat(nc, angle.to_radians()), q);
        let te = quat_to_euler(twin);
        let id = next;
        let mut any = false;
        for &c in &members[f] {
            let p = v.center(c);
            let d = (0..3).map(|a| (p[a] - c0[a] - shift[a]) * ns[a]).sum::<f64>();
            if d.abs() <= half {
                ids[c] = id;
                euler[c] = te;
                new_phases[c] = new_phase;
                any = true;
            }
        }
        if any {
            parents.push((id, f as i32));
            next += 1;
        }
    }
    let mut out = img.clone();
    out.cell_data.insert(Array::new(ids_name, 1, ArrayData::I32(ids)));
    out.cell_data.insert(Array::new("EulerAngles", 3, ArrayData::F32(euler.iter().flatten().map(|x| *x as f32).collect())));
    out.cell_data.insert(Array::new("Phases", 1, ArrayData::I32(new_phases)));
    let mut parent_of = vec![0i32; next as usize];
    for (i, f) in parents {
        parent_of[i as usize] = f;
    }
    out.field_data.insert(Array::new("TransformationParentIds", 1, ArrayData::I32(parent_of)));
    Ok(DataSet::ImageData(out))
}

/// Adjust Volume: move cells on feature boundaries from one feature to a neighbour when that
/// brings both features' cell counts closer to their targets (a per-feature field array of
/// target cell counts, or of equivalent diameters), for some sweeps.
pub fn adjust_volume(ds: &DataSet, ids_name: &str, target: &str, sweeps: usize) -> Result<DataSet> {
    let img = image(ds)?;
    let v = Vox::of(img);
    let mut ids = ids_of(img, ids_name)?;
    let nf = ids.iter().copied().max().unwrap_or(0).max(0) as usize + 1;
    let t = img.field_data.get(target).ok_or_else(|| anyhow!("no per-feature array {target}"))?;
    if t.len() < nf {
        bail!("{target} has {} rows for {} features", t.len(), nf);
    }
    let mut cnt = vec![0f64; nf];
    for &f in &ids {
        if f > 0 {
            cnt[f as usize] += 1.0;
        }
    }
    // Targets as cell counts: a diameter-like array is turned into volumes, scaled to the total.
    let raw: Vec<f64> = (0..nf).map(|f| t.get(f, 0)).collect();
    let diameters = target.to_ascii_lowercase().contains("diameter") || target.to_ascii_lowercase().contains("esd");
    let vol: Vec<f64> = raw.iter().map(|&x| if diameters { x.max(0.0).powi(3) } else { x.max(0.0) }).collect();
    let total: f64 = cnt[1..].iter().sum();
    let sum: f64 = vol[1..].iter().sum::<f64>().max(1e-300);
    let goal: Vec<f64> = vol.iter().map(|x| x / sum * total).collect();
    let err = |f: usize, n: f64| ((n - goal[f]) / goal[f].max(1.0)).powi(2);
    for _ in 0..sweeps.max(1) {
        let mut moved = 0;
        for c in 0..ids.len() {
            let a = ids[c];
            if a <= 0 {
                continue;
            }
            let mut best: Option<(i32, f64)> = None;
            for nb in v.nbrs(c).into_iter().flatten() {
                let b = ids[nb];
                if b <= 0 || b == a {
                    continue;
                }
                let (ua, ub) = (a as usize, b as usize);
                let gain = err(ua, cnt[ua]) + err(ub, cnt[ub]) - err(ua, cnt[ua] - 1.0) - err(ub, cnt[ub] + 1.0);
                if gain > 1e-12 && best.is_none_or(|x| gain > x.1) && cnt[ua] > 1.0 {
                    best = Some((b, gain));
                }
            }
            if let Some((b, _)) = best {
                cnt[a as usize] -= 1.0;
                cnt[b as usize] += 1.0;
                ids[c] = b;
                moved += 1;
            }
        }
        if moved == 0 {
            break;
        }
    }
    let mut out = img.clone();
    out.cell_data.insert(Array::new(ids_name, 1, ArrayData::I32(ids)));
    Ok(DataSet::ImageData(out))
}

/// Surface area (exposed cell faces: to other features and the outside) and volume per feature,
/// their ratio (`SurfaceAreaVolumeRatio`) and the sphericity (pi^(1/3) (6 V)^(2/3) / A; voxel
/// staircases make it lower than for the smooth shape).
pub fn surface_area_to_volume(ds: &DataSet, ids_name: &str) -> Result<DataSet> {
    let img = image(ds)?;
    let v = Vox::of(img);
    let ids = ids_of(img, ids_name)?;
    let nf = ids.iter().copied().max().unwrap_or(0).max(0) as usize + 1;
    let face = [v.sp[1] * v.sp[2], v.sp[0] * v.sp[2], v.sp[0] * v.sp[1]];
    let vol_cell = v.sp[0] * v.sp[1] * v.sp[2];
    let mut area = vec![0.0; nf];
    let mut vol = vec![0.0; nf];
    for (c, &f) in ids.iter().enumerate() {
        if f <= 0 {
            continue;
        }
        vol[f as usize] += vol_cell;
        // Faces in the order -z, -y, -x, +x, +y, +z.
        for (k, nb) in v.nbrs(c).into_iter().enumerate() {
            let axis = match k {
                0 | 5 => 2,
                1 | 4 => 1,
                _ => 0,
            };
            if v.n[axis] == 1 {
                continue;
            }
            if nb.is_none_or(|nb| ids[nb] != f) {
                area[f as usize] += face[axis];
            }
        }
    }
    let ratio: Vec<f64> = (0..nf).map(|f| if vol[f] > 0.0 { area[f] / vol[f] } else { 0.0 }).collect();
    let sph: Vec<f64> = (0..nf).map(|f| if area[f] > 0.0 { std::f64::consts::PI.cbrt() * (6.0 * vol[f]).powf(2.0 / 3.0) / area[f] } else { 0.0 }).collect();
    let mut out = img.clone();
    out.field_data.insert(Array::scalars_f64("SurfaceAreas", area));
    out.field_data.insert(Array::scalars_f64("SurfaceAreaVolumeRatio", ratio));
    out.field_data.insert(Array::scalars_f64("Sphericity", sph));
    Ok(DataSet::ImageData(out))
}

/// Precipitate statistics of the features of one phase: count, volume fraction, number density,
/// the lognormal fit of their equivalent diameters (mu, sigma of the log), and the mean distance
/// from each precipitate centre to the nearest other one, as a one-row table.
pub fn precipitate_statistics(ds: &DataSet, ids_name: &str, phase: i32) -> Result<Table> {
    let img = image(ds)?;
    let v = Vox::of(img);
    let ids = ids_of(img, ids_name)?;
    let nf = ids.iter().copied().max().unwrap_or(0).max(0) as usize + 1;
    let ph = img.cell_data.get("Phases");
    let mut cnt = vec![0usize; nf];
    let mut fph = vec![0i32; nf];
    let mut ctr = vec![[0.0; 3]; nf];
    for (c, &f) in ids.iter().enumerate() {
        if f > 0 {
            cnt[f as usize] += 1;
            fph[f as usize] = ph.map(|p| p.get(c, 0) as i32).unwrap_or(1);
            let p = v.center(c);
            for a in 0..3 {
                ctr[f as usize][a] += p[a];
            }
        }
    }
    let sel: Vec<usize> = (1..nf).filter(|&f| cnt[f] > 0 && fph[f] == phase).collect();
    if sel.is_empty() {
        bail!("no features of phase {phase}");
    }
    let flat = v.n.iter().any(|&k| k == 1);
    let cell_v = if flat { v.sp[0] * v.sp[1] } else { v.sp[0] * v.sp[1] * v.sp[2] };
    let total = ids.len() as f64 * cell_v;
    let esd: Vec<f64> = sel
        .iter()
        .map(|&f| {
            let vol = cnt[f] as f64 * cell_v;
            if flat {
                2.0 * (vol / std::f64::consts::PI).sqrt()
            } else {
                (6.0 * vol / std::f64::consts::PI).cbrt()
            }
        })
        .collect();
    let logs: Vec<f64> = esd.iter().map(|d| d.ln()).collect();
    let mu = logs.iter().sum::<f64>() / logs.len() as f64;
    let sigma = (logs.iter().map(|x| (x - mu).powi(2)).sum::<f64>() / (logs.len() as f64 - 1.0).max(1.0)).sqrt();
    let pts: Vec<[f64; 3]> = sel.iter().map(|&f| ctr[f].map(|x| x / cnt[f] as f64)).collect();
    let tree = aurafx_data::kdtree::KdTree::new(&pts);
    let nn = if pts.len() > 1 { pts.iter().map(|p| tree.nearest(*p, 2)[1].1.sqrt()).sum::<f64>() / pts.len() as f64 } else { 0.0 };
    let frac = sel.iter().map(|&f| cnt[f]).sum::<usize>() as f64 * cell_v / total;
    let mut t = Table::default();
    for (n, x) in [
        ("Count", sel.len() as f64),
        ("VolumeFraction", frac),
        ("NumberDensity", sel.len() as f64 / total),
        ("ESDLogMean", mu),
        ("ESDLogStdDev", sigma),
        ("MeanNearestNeighbourDistance", nn),
    ] {
        t.columns.insert(Array::scalars_f64(n, vec![x]));
    }
    Ok(t)
}

/// Triangle angles of a surface mesh: the smallest and largest interior angle of each triangle
/// (`MinimumAngles`, `MaximumAngles`, degrees) and the dihedral angle across each triangle's
/// edges with its neighbours on the same boundary (the largest, `MaximumDihedralAngles`).
pub fn triangle_angles(ds: &DataSet) -> Result<DataSet> {
    let DataSet::PolyData(pd) = ds else { bail!("this filter takes a triangle surface (Quick Surface Mesh)") };
    let n = pd.polys.len();
    let mut mins = vec![0.0; n];
    let mut maxs = vec![0.0; n];
    let mut normals = vec![[0.0; 3]; n];
    let sub = |a: [f64; 3], b: [f64; 3]| [a[0] - b[0], a[1] - b[1], a[2] - b[2]];
    let dot = |a: [f64; 3], b: [f64; 3]| a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
    let mut edges: std::collections::HashMap<(u32, u32), Vec<usize>> = Default::default();
    for t in 0..n {
        let c = pd.polys.cell(t);
        let p: Vec<[f64; 3]> = c.iter().map(|&i| pd.points.get(i as usize)).collect();
        let k = p.len();
        let mut angles = Vec::new();
        for i in 0..k {
            let (a, b) = (sub(p[(i + 1) % k], p[i]), sub(p[(i + k - 1) % k], p[i]));
            let cosv = (dot(a, b) / (dot(a, a) * dot(b, b)).sqrt().max(1e-300)).clamp(-1.0, 1.0);
            angles.push(cosv.acos().to_degrees());
            let e = (c[i].min(c[(i + 1) % k]), c[i].max(c[(i + 1) % k]));
            edges.entry(e).or_default().push(t);
        }
        mins[t] = angles.iter().copied().fold(f64::INFINITY, f64::min);
        maxs[t] = angles.iter().copied().fold(0.0, f64::max);
        let (u, w) = (sub(p[1], p[0]), sub(p[2], p[0]));
        let nn = [u[1] * w[2] - u[2] * w[1], u[2] * w[0] - u[0] * w[2], u[0] * w[1] - u[1] * w[0]];
        let l = dot(nn, nn).sqrt().max(1e-300);
        normals[t] = nn.map(|x| x / l);
    }
    let labels = pd.cell_data.get("FaceLabels");
    let same = |a: usize, b: usize| {
        labels.is_none_or(|l| {
            let (a0, a1, b0, b1) = (l.get(a, 0), l.get(a, 1), l.get(b, 0), l.get(b, 1));
            (a0 == b0 && a1 == b1) || (a0 == b1 && a1 == b0)
        })
    };
    let mut dih = vec![0.0f64; n];
    for ts in edges.values() {
        if ts.len() == 2 && same(ts[0], ts[1]) {
            let ang = dot(normals[ts[0]], normals[ts[1]]).abs().clamp(-1.0, 1.0).acos().to_degrees();
            for &t in ts {
                dih[t] = dih[t].max(ang);
            }
        }
    }
    let mut out = pd.clone();
    out.cell_data.insert(Array::scalars_f64("MinimumAngles", mins));
    out.cell_data.insert(Array::scalars_f64("MaximumAngles", maxs));
    out.cell_data.insert(Array::scalars_f64("MaximumDihedralAngles", dih));
    Ok(DataSet::PolyData(out))
}

/// Feature Face Curvature: the mean and Gaussian curvature of each boundary (the triangles with
/// one pair of FaceLabels) computed on that boundary alone, per triangle (the average of its
/// corners').
pub fn feature_face_curvature(ds: &DataSet) -> Result<DataSet> {
    let DataSet::PolyData(pd) = ds else { bail!("this filter takes a grain surface mesh (Quick Surface Mesh)") };
    let labels = pd.cell_data.get("FaceLabels").ok_or_else(|| anyhow!("no FaceLabels cell array (make the mesh with Quick Surface Mesh)"))?;
    let n = pd.polys.len();
    let mut groups: std::collections::BTreeMap<(i64, i64), Vec<usize>> = Default::default();
    for t in 0..n {
        let (a, b) = (labels.get(t, 0) as i64, labels.get(t, 1) as i64);
        groups.entry((a.min(b), a.max(b))).or_default().push(t);
    }
    let mut mean = vec![0.0; n];
    let mut gauss = vec![0.0; n];
    for tris in groups.values() {
        // The patch on its own.
        let mut map = std::collections::HashMap::new();
        let mut patch = PolyData::default();
        for &t in tris {
            let ids: Vec<u32> = pd
                .polys
                .cell(t)
                .iter()
                .map(|&i| {
                    *map.entry(i).or_insert_with(|| {
                        patch.points.push(pd.points.get(i as usize));
                        (patch.points.len() - 1) as u32
                    })
                })
                .collect();
            patch.polys.push(CellType::Triangle, &ids);
        }
        let ds = DataSet::PolyData(patch);
        let m = crate::surface_ops::curvature(&ds, true)?;
        let g = crate::surface_ops::curvature(&ds, false)?;
        let (Some(ma), Some(ga)) = (m.point_data.iter().find(|a| a.ncomp == 1).cloned(), g.point_data.iter().find(|a| a.ncomp == 1).cloned()) else { continue };
        for (k, &t) in tris.iter().enumerate() {
            let c = m.polys.cell(k);
            mean[t] = c.iter().map(|&i| ma.get(i as usize, 0)).sum::<f64>() / c.len() as f64;
            gauss[t] = c.iter().map(|&i| ga.get(i as usize, 0)).sum::<f64>() / c.len() as f64;
        }
    }
    let mut out = pd.clone();
    out.cell_data.insert(Array::scalars_f64("MeanCurvatures", mean));
    out.cell_data.insert(Array::scalars_f64("GaussianCurvatures", gauss));
    Ok(DataSet::PolyData(out))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn block(n: usize, ids: impl Fn(usize, usize, usize) -> i32) -> ImageData {
        let mut v = Vec::new();
        for k in 0..n {
            for j in 0..n {
                for i in 0..n {
                    v.push(ids(i, j, k));
                }
            }
        }
        let mut im = ImageData { extent: [0, n as i64, 0, n as i64, 0, n as i64], ..Default::default() };
        im.cell_data.insert(Array::new("FeatureIds", 1, ArrayData::I32(v)));
        im
    }

    #[test]
    fn area_volume_and_adjust() {
        // A 4^3 cube feature in the middle of an 8^3 grid of feature 2.
        let im = block(8, |i, j, k| if (2..6).contains(&i) && (2..6).contains(&j) && (2..6).contains(&k) { 1 } else { 2 });
        let d = surface_area_to_volume(&DataSet::ImageData(im.clone()), "FeatureIds").unwrap();
        let fd = d.field_data();
        assert_eq!(fd.get("SurfaceAreas").unwrap().get(1, 0), 96.0);
        assert!((fd.get("SurfaceAreaVolumeRatio").unwrap().get(1, 0) - 1.5).abs() < 1e-12);
        assert!((fd.get("Sphericity").unwrap().get(1, 0) - std::f64::consts::PI.cbrt() * (6.0 * 64.0f64).powf(2.0 / 3.0) / 96.0).abs() < 1e-12);
        // Targets 200 and 312 cells (the cube grows from 64).
        let mut t = im.clone();
        t.field_data.insert(Array::scalars_f64("Goal", vec![0.0, 200.0, 312.0]));
        let a = adjust_volume(&DataSet::ImageData(t), "FeatureIds", "Goal", 50).unwrap();
        let ids = a.cell_data().unwrap().get("FeatureIds").unwrap();
        let ones = (0..ids.len()).filter(|&i| ids.get(i, 0) == 1.0).count();
        assert!((ones as f64 - 200.0).abs() <= 2.0, "{ones}");
    }

    #[test]
    fn twin_plates_and_precipitates() {
        let mut im = block(20, |i, _, _| if i < 10 { 1 } else { 2 });
        let n = 8000;
        im.cell_data.insert(Array::new("EulerAngles", 3, ArrayData::F32(vec![0.0; 3 * n])));
        im.cell_data.insert(Array::new("Phases", 1, ArrayData::I32(vec![1; n])));
        im.field_data.insert(Array::new("CrystalStructures", 1, ArrayData::U32(vec![999, 1])));
        let out = insert_transformation_phases(&DataSet::ImageData(im), "FeatureIds", 1, 2, 1.0, 2.0, 60.0, 3).unwrap();
        let ids = out.cell_data().unwrap().get("FeatureIds").unwrap();
        let new: Vec<usize> = (0..n).filter(|&i| ids.get(i, 0) >= 3.0).collect();
        assert!(!new.is_empty());
        // The plates are 60 degrees from their parents about a <111>.
        let e = out.cell_data().unwrap().get("EulerAngles").unwrap();
        let q = aurafx_crystal::rotation::euler_to_quat([e.get(new[0], 0), e.get(new[0], 1), e.get(new[0], 2)]);
        let ang = aurafx_crystal::disorientation_angle(q, [1.0, 0.0, 0.0, 0.0], aurafx_crystal::Laue::CubicHigh).to_degrees();
        assert!((ang - 60.0).abs() < 1e-3, "{ang}");
        let ph = out.cell_data().unwrap().get("Phases").unwrap();
        assert_eq!(ph.get(new[0], 0), 2.0);
        let t = precipitate_statistics(&out, "FeatureIds", 2).unwrap();
        assert_eq!(t.columns.get("Count").unwrap().get(0, 0), 2.0);
    }

    #[test]
    fn angles_and_patch_curvature() {
        // A right isosceles triangle: 45, 45, 90.
        let mut pd = PolyData::default();
        for p in [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]] {
            pd.points.push(p);
        }
        pd.polys.push(CellType::Triangle, &[0, 1, 2]);
        let DataSet::PolyData(o) = triangle_angles(&DataSet::PolyData(pd)).unwrap() else { panic!() };
        assert!((o.cell_data.get("MinimumAngles").unwrap().get(0, 0) - 45.0).abs() < 1e-9);
        assert!((o.cell_data.get("MaximumAngles").unwrap().get(0, 0) - 90.0).abs() < 1e-9);
    }
}
