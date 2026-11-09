//! Halos, shape measures and densities: friends-of-friends halos of particles with their
//! spherical-overdensity radius and mass, the Minkowski functionals of a region (volume,
//! surface area, integrated mean curvature, Euler characteristic), and kernel density
//! estimates of one array (a curve) or two arrays (a density map).

use crate::params::Association;
use anyhow::{anyhow, bail, Result};
use aurafx_data::kdtree::KdTree;
use aurafx_data::*;

/// Halo finder settings.
#[derive(Debug, Clone)]
pub struct HaloOptions {
    /// Linking length as a fraction of the mean particle spacing (0.2 is usual).
    pub linking: f64,
    /// Halos with fewer particles are dropped.
    pub min_members: usize,
    /// Point array of particle masses (empty: 1 each).
    pub mass: String,
    /// Point array of velocities (empty: none).
    pub velocity: String,
    /// Overdensity for the spherical-overdensity radius, times the mean density (200 is usual).
    pub overdensity: f64,
}

/// Friends-of-friends halos: particles closer than the linking length are in one group.
/// Returns the particles with `HaloId` (-1 outside halos) and a table of halos: count, mass,
/// centre of mass, mean velocity, and the radius and mass within which the mean density is
/// `overdensity` times the mean density of the box (R_SOD, M_SOD).
pub fn friends_of_friends(ds: &DataSet, opt: &HaloOptions) -> Result<(DataSet, Table)> {
    let pts = ds.points().ok_or_else(|| anyhow!("the halo finder needs particles (points)"))?;
    let n = pts.len();
    if n == 0 {
        bail!("no particles");
    }
    let p: Vec<[f64; 3]> = (0..n).map(|i| pts.get(i)).collect();
    let pd = ds.point_data();
    let get = |name: &str| -> Option<&Array> {
        if name.is_empty() {
            None
        } else {
            pd.and_then(|a| a.get(name))
        }
    };
    let mass = get(&opt.mass);
    let vel = get(&opt.velocity);
    if !opt.mass.is_empty() && mass.is_none() {
        bail!("no point array named {:?}", opt.mass);
    }
    let m = |i: usize| mass.map(|a| a.get(i, 0)).unwrap_or(1.0);
    let b = ds.bounds();
    let s = b.size();
    let volume = (s[0].max(1e-300)) * (s[1].max(1e-300)) * (s[2].max(1e-300));
    let spacing = (volume / n as f64).cbrt();
    let link = opt.linking * spacing;
    let tree = KdTree::new(&p);
    let mut parent: Vec<usize> = (0..n).collect();
    fn find(p: &mut [usize], mut i: usize) -> usize {
        while p[i] != i {
            p[i] = p[p[i]];
            i = p[i];
        }
        i
    }
    for i in 0..n {
        for (j, _) in tree.within(p[i], link) {
            if j > i {
                let (a, c) = (find(&mut parent, i), find(&mut parent, j));
                if a != c {
                    parent[a.max(c)] = a.min(c);
                }
            }
        }
    }
    let mut members: std::collections::BTreeMap<usize, Vec<usize>> = Default::default();
    for i in 0..n {
        let r = find(&mut parent, i);
        members.entry(r).or_default().push(i);
    }
    let mut groups: Vec<Vec<usize>> = members.into_values().filter(|g| g.len() >= opt.min_members.max(1)).collect();
    groups.sort_by(|a, c| c.len().cmp(&a.len()).then(a[0].cmp(&c[0])));
    let mut halo_id = vec![-1i64; n];
    let total_mass: f64 = (0..n).map(m).sum();
    let mean_density = total_mass / volume;
    let mut t = Table::default();
    let (mut cnt, mut ms, mut ctr, mut vmean, mut rsod, mut msod) = (Vec::new(), Vec::new(), Vec::new(), Vec::new(), Vec::new(), Vec::new());
    for (h, g) in groups.iter().enumerate() {
        let mut mt = 0.0;
        let mut c = [0.0; 3];
        let mut v = [0.0; 3];
        for &i in g {
            halo_id[i] = h as i64;
            let w = m(i);
            mt += w;
            for a in 0..3 {
                c[a] += w * p[i][a];
                if let Some(va) = vel {
                    v[a] += w * va.get(i, a.min(va.ncomp - 1));
                }
            }
        }
        let c = c.map(|x| x / mt);
        let v = v.map(|x| x / mt);
        // Spherical overdensity: grow a sphere around the centre over the particles sorted by
        // distance until the mean density inside falls below the threshold.
        let reach = (b.diagonal()).max(link);
        let mut around: Vec<(f64, usize)> = tree.within(c, reach).into_iter().map(|(i, d2)| (d2.sqrt(), i)).collect();
        around.sort_by(|a, b2| a.0.total_cmp(&b2.0));
        let (mut r_sod, mut m_sod, mut acc) = (0.0, 0.0, 0.0);
        for &(r, i) in &around {
            acc += m(i);
            if r > 0.0 {
                let dens = acc / (4.0 / 3.0 * std::f64::consts::PI * r * r * r);
                if dens >= opt.overdensity * mean_density {
                    r_sod = r;
                    m_sod = acc;
                }
            }
        }
        cnt.push(g.len() as f64);
        ms.push(mt);
        ctr.extend_from_slice(&c);
        vmean.extend_from_slice(&v);
        rsod.push(r_sod);
        msod.push(m_sod);
    }
    t.columns.insert(Array::new("HaloId", 1, ArrayData::I64((0..groups.len() as i64).collect())));
    t.columns.insert(Array::scalars_f64("Count", cnt));
    t.columns.insert(Array::scalars_f64("Mass", ms));
    t.columns.insert(Array::new("Center", 3, ArrayData::F64(ctr)));
    if vel.is_some() {
        t.columns.insert(Array::new("Velocity", 3, ArrayData::F64(vmean)));
    }
    t.columns.insert(Array::scalars_f64("RadiusSOD", rsod));
    t.columns.insert(Array::scalars_f64("MassSOD", msod));
    t.field_data.insert(Array::scalars_f64("LinkingLength", vec![link]));
    t.field_data.insert(Array::scalars_f64("HaloCount", vec![groups.len() as f64]));
    let mut out = ds.clone();
    out.point_data_mut().ok_or_else(|| anyhow!("cannot add arrays to these particles"))?.insert(Array::new("HaloId", 1, ArrayData::I64(halo_id)));
    Ok((out, t))
}

/// The Minkowski functionals of the region where `array` >= `threshold`, from its boundary
/// surface (the data is padded so the surface closes at the data's edges): volume, surface
/// area, integrated mean curvature (the integral of (k1 + k2) / 2 over the surface) and Euler
/// characteristic, with the normalised W0..W3.
pub fn minkowski(ds: &DataSet, assoc: Association, array: &str, threshold: f64) -> Result<Table> {
    let surface = match ds {
        DataSet::ImageData(im) => {
            // Pad with one layer below the threshold all round, so the surface is closed.
            let point = if assoc == Association::Cells { crate::attributes::cell_to_point(ds, &[array.to_string()])? } else { ds.clone() };
            let DataSet::ImageData(im2) = &point else { unreachable!() };
            let a = im2.point_data.get(array).ok_or_else(|| anyhow!("no point array {array:?}"))?;
            let d = im.dims();
            let pd_ = [d[0] + 2, d[1] + 2, d[2] + 2];
            let low = threshold - 1.0 - a.data.to_f64_vec().iter().fold(0.0f64, |m, v| m.max((v - threshold).abs()));
            let mut v = vec![low; pd_[0] * pd_[1] * pd_[2]];
            for k in 0..d[2] {
                for j in 0..d[1] {
                    for i in 0..d[0] {
                        v[(i + 1) + pd_[0] * ((j + 1) + pd_[1] * (k + 1))] = a.get(i + d[0] * (j + d[1] * k), 0);
                    }
                }
            }
            // The first point of the extent, one spacing further out.
            let mut padded = ImageData {
                extent: [0, pd_[0] as i64 - 1, 0, pd_[1] as i64 - 1, 0, pd_[2] as i64 - 1],
                origin: [
                    im.origin[0] + (im.extent[0] - 1) as f64 * im.spacing[0],
                    im.origin[1] + (im.extent[2] - 1) as f64 * im.spacing[1],
                    im.origin[2] + (im.extent[4] - 1) as f64 * im.spacing[2],
                ],
                spacing: im.spacing,
                direction: im.direction,
                ..Default::default()
            };
            padded.point_data.insert(Array::scalars_f64(array, v));
            crate::contour::contour(&DataSet::ImageData(padded), Association::Points, array, None, &[threshold])?
        }
        DataSet::PolyData(p) if !p.polys.is_empty() => p.clone(),
        d => crate::contour::contour(d, assoc, array, None, &[threshold])?,
    };
    let (v, a, m, chi) = surface_measures(&surface)?;
    let mut t = Table::default();
    for (name, val) in [
        ("Volume", v),
        ("SurfaceArea", a),
        ("IntegratedMeanCurvature", m),
        ("EulerCharacteristic", chi),
        ("W0", v),
        ("W1", a / 3.0),
        ("W2", m / 3.0),
        ("W3", 4.0 * std::f64::consts::PI / 3.0 * chi),
    ] {
        t.columns.insert(Array::scalars_f64(name, vec![val]));
    }
    Ok(t)
}

/// Enclosed volume, area, integrated mean curvature and Euler characteristic (of the solid) of a
/// closed triangulated surface.
pub fn surface_measures(p: &PolyData) -> Result<(f64, f64, f64, f64)> {
    // Merge coincident points first (contours share edge points, but be safe).
    let b = p.points.bounds();
    let tol = (b.diagonal() * 1e-9).max(1e-300);
    let mut key: std::collections::HashMap<[i64; 3], usize> = Default::default();
    let mut pts: Vec<[f64; 3]> = Vec::new();
    let mut rep = vec![0usize; p.points.len()];
    for (i, r) in rep.iter_mut().enumerate() {
        let q = p.points.get(i);
        let k = [(q[0] / tol).round() as i64, (q[1] / tol).round() as i64, (q[2] / tol).round() as i64];
        *r = *key.entry(k).or_insert_with(|| {
            pts.push(q);
            pts.len() - 1
        });
    }
    let mut tris: Vec<[usize; 3]> = Vec::new();
    for (_, c) in p.polys.iter() {
        for k in 1..c.len().saturating_sub(1) {
            let t = [rep[c[0] as usize], rep[c[k] as usize], rep[c[k + 1] as usize]];
            if t[0] != t[1] && t[1] != t[2] && t[0] != t[2] {
                tris.push(t);
            }
        }
    }
    if tris.is_empty() {
        bail!("the region has no surface");
    }
    for comp in orient(&mut tris) {
        // Each closed piece outward: its own enclosed volume positive.
        let v: f64 = comp
            .iter()
            .map(|&f| {
                let t = tris[f];
                let (a, b, c) = (pts[t[0]], pts[t[1]], pts[t[2]]);
                a[0] * (b[1] * c[2] - b[2] * c[1]) - a[1] * (b[0] * c[2] - b[2] * c[0]) + a[2] * (b[0] * c[1] - b[1] * c[0])
            })
            .sum();
        if v < 0.0 {
            for &f in &comp {
                tris[f].swap(1, 2);
            }
        }
    }
    let sub = |a: [f64; 3], b: [f64; 3]| [a[0] - b[0], a[1] - b[1], a[2] - b[2]];
    let cross = |a: [f64; 3], b: [f64; 3]| [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]];
    let dot = |a: [f64; 3], b: [f64; 3]| a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
    let mut vol = 0.0;
    let mut area = 0.0;
    let normals: Vec<[f64; 3]> = tris
        .iter()
        .map(|t| {
            let n = cross(sub(pts[t[1]], pts[t[0]]), sub(pts[t[2]], pts[t[0]]));
            area += 0.5 * dot(n, n).sqrt();
            vol += dot(pts[t[0]], cross(pts[t[1]], pts[t[2]])) / 6.0;
            n
        })
        .collect();
    // Orient outward: the enclosed volume comes out positive.
    let sign = if vol < 0.0 { -1.0 } else { 1.0 };
    vol = vol.abs();
    // Edges with their two faces.
    let mut edges: std::collections::HashMap<(usize, usize), Vec<(usize, bool)>> = Default::default();
    for (f, t) in tris.iter().enumerate() {
        for k in 0..3 {
            let (a, c) = (t[k], t[(k + 1) % 3]);
            edges.entry((a.min(c), a.max(c))).or_default().push((f, a < c));
        }
    }
    let mut mean = 0.0;
    for ((a, c), fs) in &edges {
        if fs.len() != 2 {
            continue;
        }
        let (n1, n2) = (normals[fs[0].0].map(|x| x * sign), normals[fs[1].0].map(|x| x * sign));
        let (l1, l2) = (dot(n1, n1).sqrt(), dot(n2, n2).sqrt());
        if l1 <= 0.0 || l2 <= 0.0 {
            continue;
        }
        let cosang = (dot(n1, n2) / (l1 * l2)).clamp(-1.0, 1.0);
        let angle = cosang.acos();
        // Convex where the second face's far corner lies below the first face's plane.
        let t2 = tris[fs[1].0];
        let far = t2.iter().copied().find(|&v| v != *a && v != *c).unwrap();
        let convex = dot(sub(pts[far], pts[*a]), n1) <= 0.0;
        let len = dot(sub(pts[*c], pts[*a]), sub(pts[*c], pts[*a])).sqrt();
        mean += 0.5 * len * if convex { angle } else { -angle };
    }
    let chi_surface = pts.len() as f64 - edges.len() as f64 + tris.len() as f64;
    Ok((vol, area, mean, chi_surface / 2.0))
}

/// Wind every triangle the same way as its neighbours (a triangle across a shared edge runs
/// that edge the other way), component by component. Returns the faces of each component.
fn orient(tris: &mut [[usize; 3]]) -> Vec<Vec<usize>> {
    let mut comps = Vec::new();
    let mut by_edge: std::collections::HashMap<(usize, usize), Vec<usize>> = Default::default();
    for (f, t) in tris.iter().enumerate() {
        for k in 0..3 {
            let (a, b) = (t[k], t[(k + 1) % 3]);
            by_edge.entry((a.min(b), a.max(b))).or_default().push(f);
        }
    }
    let mut done = vec![false; tris.len()];
    for start in 0..tris.len() {
        if done[start] {
            continue;
        }
        done[start] = true;
        let mut stack = vec![start];
        let mut comp = Vec::new();
        while let Some(f) = stack.pop() {
            comp.push(f);
            let t = tris[f];
            for k in 0..3 {
                let (a, b) = (t[k], t[(k + 1) % 3]);
                for &g in &by_edge[&(a.min(b), a.max(b))] {
                    if g == f || done[g] {
                        continue;
                    }
                    // g must run the edge b -> a.
                    let u = tris[g];
                    let same = (0..3).any(|j| u[j] == a && u[(j + 1) % 3] == b);
                    if same {
                        tris[g].swap(1, 2);
                    }
                    done[g] = true;
                    stack.push(g);
                }
            }
        }
        comps.push(comp);
    }
    comps
}

/// Gaussian kernel density of the values `v` (bandwidth: Scott's rule when 0) at `n` points
/// across their range: (x, density).
pub fn kde_1d(v: &[f64], bandwidth: f64, n: usize) -> (Vec<f64>, Vec<f64>) {
    let v: Vec<f64> = v.iter().copied().filter(|x| x.is_finite()).collect();
    if v.is_empty() {
        return (Vec::new(), Vec::new());
    }
    let mean = v.iter().sum::<f64>() / v.len() as f64;
    let sd = (v.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / (v.len() as f64 - 1.0).max(1.0)).sqrt();
    let h = if bandwidth > 0.0 { bandwidth } else { (1.06 * sd * (v.len() as f64).powf(-0.2)).max(1e-12) };
    let (lo, hi) = v.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), &x| (a.min(x), b.max(x)));
    let (lo, hi) = (lo - 3.0 * h, hi + 3.0 * h);
    let n = n.max(2);
    let xs: Vec<f64> = (0..n).map(|k| lo + (hi - lo) * k as f64 / (n - 1) as f64).collect();
    let c = 1.0 / (v.len() as f64 * h * (2.0 * std::f64::consts::PI).sqrt());
    use rayon::prelude::*;
    let ys = xs.par_iter().map(|&x| c * v.iter().map(|&y| (-0.5 * ((x - y) / h).powi(2)).exp()).sum::<f64>()).collect();
    (xs, ys)
}

/// Gaussian kernel density of the pairs (x, y) on an `n` x `n` image (bandwidths by Scott's
/// rule), with field data `Levels`: the densities inside which 50, 90 and 99 % of the samples lie.
pub fn density_map(x: &[f64], y: &[f64], n: usize) -> Result<ImageData> {
    let pairs: Vec<(f64, f64)> = x.iter().zip(y).filter(|(a, b)| a.is_finite() && b.is_finite()).map(|(a, b)| (*a, *b)).collect();
    if pairs.len() < 2 {
        bail!("need at least two values");
    }
    let stats = |f: &dyn Fn(&(f64, f64)) -> f64| {
        let m = pairs.iter().map(f).sum::<f64>() / pairs.len() as f64;
        let s = (pairs.iter().map(|p| (f(p) - m).powi(2)).sum::<f64>() / (pairs.len() - 1) as f64).sqrt();
        s
    };
    let factor = (pairs.len() as f64).powf(-1.0 / 6.0);
    let hx = (stats(&|p| p.0) * factor).max(1e-12);
    let hy = (stats(&|p| p.1) * factor).max(1e-12);
    let (x0, x1) = pairs.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), p| (a.min(p.0), b.max(p.0)));
    let (y0, y1) = pairs.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), p| (a.min(p.1), b.max(p.1)));
    let (x0, x1, y0, y1) = (x0 - 3.0 * hx, x1 + 3.0 * hx, y0 - 3.0 * hy, y1 + 3.0 * hy);
    let n = n.max(2);
    let (sx, sy) = ((x1 - x0) / (n - 1) as f64, (y1 - y0) / (n - 1) as f64);
    let c = 1.0 / (pairs.len() as f64 * 2.0 * std::f64::consts::PI * hx * hy);
    use rayon::prelude::*;
    let dens: Vec<f64> = (0..n * n)
        .into_par_iter()
        .map(|k| {
            let (i, j) = (k % n, k / n);
            let (px, py) = (x0 + i as f64 * sx, y0 + j as f64 * sy);
            c * pairs.iter().map(|p| (-0.5 * (((px - p.0) / hx).powi(2) + ((py - p.1) / hy).powi(2))).exp()).sum::<f64>()
        })
        .collect();
    // Highest-density levels: the density at each sample, sorted; the level holding q of them.
    let mut at: Vec<f64> =
        pairs.iter().map(|p| c * pairs.iter().map(|q| (-0.5 * (((p.0 - q.0) / hx).powi(2) + ((p.1 - q.1) / hy).powi(2))).exp()).sum::<f64>()).collect();
    at.sort_by(|a, b| b.total_cmp(a));
    let level = |q: f64| at[((q * at.len() as f64) as usize).min(at.len() - 1)];
    let mut im = ImageData { extent: [0, n as i64 - 1, 0, n as i64 - 1, 0, 0], origin: [x0, y0, 0.0], spacing: [sx, sy, 1.0], ..Default::default() };
    im.point_data.insert_active(AttributeKind::Scalars, Array::scalars_f64("Density", dens));
    im.field_data.insert(Array::scalars_f64("Levels", vec![level(0.5), level(0.9), level(0.99)]));
    im.field_data.insert(Array::scalars_f64("Bandwidth", vec![hx, hy]));
    Ok(im)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rng(seed: u64) -> crate::stats::Rng {
        crate::stats::Rng::new(seed)
    }

    #[test]
    fn three_clumps_in_a_sparse_box() {
        let mut r = rng(4);
        let mut pd = PolyData::default();
        // A sparse background over the unit cube and three tight clumps of 300, 200 and 100.
        for _ in 0..400 {
            pd.points.push([r.next_f64(), r.next_f64(), r.next_f64()]);
        }
        for (c, k) in [([0.2, 0.2, 0.2], 300), ([0.7, 0.3, 0.6], 200), ([0.4, 0.8, 0.8], 100)] {
            for _ in 0..k {
                pd.points.push([c[0] + 0.01 * (r.next_f64() - 0.5), c[1] + 0.01 * (r.next_f64() - 0.5), c[2] + 0.01 * (r.next_f64() - 0.5)]);
            }
        }
        let ds = DataSet::PolyData(pd);
        let opt = HaloOptions { linking: 0.2, min_members: 20, mass: String::new(), velocity: String::new(), overdensity: 200.0 };
        let (_, t) = friends_of_friends(&ds, &opt).unwrap();
        let counts: Vec<f64> = (0..t.n_rows()).map(|i| t.columns.get("Count").unwrap().get(i, 0)).collect();
        assert_eq!(counts.len(), 3, "{counts:?}");
        assert!((counts[0] - 300.0).abs() <= 3.0 && (counts[1] - 200.0).abs() <= 3.0 && (counts[2] - 100.0).abs() <= 3.0, "{counts:?}");
        let c = t.columns.get("Center").unwrap();
        assert!((c.get(0, 0) - 0.2).abs() < 0.01);
        assert!(t.columns.get("RadiusSOD").unwrap().get(0, 0) > 0.0);
    }

    fn ball_image(n: usize, f: impl Fn([f64; 3]) -> f64) -> DataSet {
        let h = 2.0 / (n - 1) as f64;
        let mut v = Vec::new();
        for k in 0..n {
            for j in 0..n {
                for i in 0..n {
                    v.push(f([-1.0 + i as f64 * h, -1.0 + j as f64 * h, -1.0 + k as f64 * h]));
                }
            }
        }
        let mut im = ImageData { extent: [0, n as i64 - 1, 0, n as i64 - 1, 0, n as i64 - 1], origin: [-1.0; 3], spacing: [h; 3], ..Default::default() };
        im.point_data.insert(Array::scalars_f64("f", v));
        DataSet::ImageData(im)
    }

    #[test]
    fn minkowski_of_a_ball_and_a_torus() {
        let r = 0.6;
        let ds = ball_image(61, |p| r - (p[0] * p[0] + p[1] * p[1] + p[2] * p[2]).sqrt());
        let t = minkowski(&ds, Association::Points, "f", 0.0).unwrap();
        let g = |n: &str| t.columns.get(n).unwrap().get(0, 0);
        let pi = std::f64::consts::PI;
        assert!((g("Volume") / (4.0 / 3.0 * pi * r * r * r) - 1.0).abs() < 0.01, "{}", g("Volume"));
        assert!((g("SurfaceArea") / (4.0 * pi * r * r) - 1.0).abs() < 0.02, "{}", g("SurfaceArea"));
        assert!((g("IntegratedMeanCurvature") / (4.0 * pi * r) - 1.0).abs() < 0.03, "{}", g("IntegratedMeanCurvature"));
        assert_eq!(g("EulerCharacteristic"), 1.0);
        // A torus (R 0.5, a 0.2): Euler characteristic 0; a ball touching the data's edge is
        // closed by the padding.
        let torus = ball_image(61, |p| 0.2 - (((p[0] * p[0] + p[1] * p[1]).sqrt() - 0.5).powi(2) + p[2] * p[2]).sqrt());
        assert_eq!(minkowski(&torus, Association::Points, "f", 0.0).unwrap().columns.get("EulerCharacteristic").unwrap().get(0, 0), 0.0);
        let cut = ball_image(41, |p| 1.2 - (p[0] * p[0] + p[1] * p[1] + p[2] * p[2]).sqrt());
        assert_eq!(minkowski(&cut, Association::Points, "f", 0.0).unwrap().columns.get("EulerCharacteristic").unwrap().get(0, 0), 1.0);
    }

    #[test]
    fn densities() {
        let mut r = rng(11);
        let v: Vec<f64> = (0..4000).map(|_| (0..12).map(|_| r.next_f64()).sum::<f64>() - 6.0).collect();
        let (xs, ys) = kde_1d(&v, 0.0, 400);
        let step = xs[1] - xs[0];
        assert!((ys.iter().sum::<f64>() * step - 1.0).abs() < 0.01, "integrates to one");
        let peak = ys.iter().copied().fold(0.0, f64::max);
        assert!((peak - 1.0 / (2.0 * std::f64::consts::PI).sqrt()).abs() < 0.04, "{peak}");
        let y: Vec<f64> = v.iter().map(|x| 2.0 * x + 1.0).collect();
        let im = density_map(&v, &y, 60).unwrap();
        let d = im.point_data.get("Density").unwrap();
        let cell = im.spacing[0] * im.spacing[1];
        assert!(((0..d.len()).map(|i| d.get(i, 0)).sum::<f64>() * cell - 1.0).abs() < 0.03);
        let lv = im.field_data.get("Levels").unwrap();
        assert!(lv.get(0, 0) > lv.get(1, 0) && lv.get(1, 0) > lv.get(2, 0));
    }

    #[test]
    fn kde_needs_values() {
        assert!(kde_1d(&[], 0.0, 10).0.is_empty());
        assert!(density_map(&[1.0], &[2.0], 10).is_err());
    }
}
