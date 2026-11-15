//! Point interpolation: the point arrays of scattered points (particles, measurements, a point
//! cloud) carried onto other points through a kernel over their neighbours: Gaussian, Shepard
//! (inverse distance), linear (an average), nearest point (Voronoi) or SPH.

use anyhow::{anyhow, Result};
use aurafx_data::kdtree::KdTree;
use aurafx_data::*;
use rayon::prelude::*;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Kernel {
    /// exp(-(sharpness * d / radius)^2).
    Gaussian { sharpness: f64 },
    /// 1 / d^power.
    Shepard { power: f64 },
    /// Equal weights.
    Linear,
    /// The nearest point's value.
    Voronoi,
    /// Smoothed-particle kernel of smoothing length h (support 2h for cubic, 2h for Wendland).
    Sph { h: f64, quintic: bool },
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Neighbours {
    /// The points within this distance (0: chosen from the points' spacing).
    Radius(f64),
    /// The N nearest points.
    Closest(usize),
}

#[derive(Debug, Clone)]
pub struct Options {
    pub kernel: Kernel,
    pub neighbours: Neighbours,
    /// Value given where no source point is near (NaN allowed); `None`: the nearest point's.
    pub null_value: Option<f64>,
    /// SPH: arrays of the particles' mass and density (volume = mass / density); empty: the
    /// kernel weights are normalised to sum to one.
    pub mass: String,
    pub density: String,
}

impl Default for Options {
    fn default() -> Self {
        Options {
            kernel: Kernel::Shepard { power: 2.0 },
            neighbours: Neighbours::Closest(8),
            null_value: Some(0.0),
            mass: String::new(),
            density: String::new(),
        }
    }
}

/// The source's points and point arrays (multi-blocks are appended into one).
fn source_points(src: &DataSet) -> Result<(Vec<[f64; 3]>, Attributes)> {
    let merged;
    let src = match src {
        DataSet::MultiBlock(mb) => {
            let leaves: Vec<&DataSet> = mb.leaves().into_iter().map(|l| l.1).collect();
            merged = DataSet::UnstructuredGrid(crate::append::append_datasets(&leaves)?);
            &merged
        }
        d => d,
    };
    let pts = src.points().ok_or_else(|| anyhow!("the source has no points"))?;
    let pd = src.point_data().cloned().unwrap_or_default();
    Ok(((0..pts.len()).map(|i| pts.get(i)).collect(), pd))
}

/// A radius from the points' spacing: about two and a half times the mean distance to the
/// nearest neighbour (sampled).
pub fn auto_radius(tree: &KdTree, pts: &[[f64; 3]]) -> f64 {
    if pts.len() < 2 {
        return 1.0;
    }
    let step = (pts.len() / 500).max(1);
    let (mut s, mut n) = (0.0, 0usize);
    for p in pts.iter().step_by(step) {
        if let Some(&(_, d)) = tree.nearest(*p, 2).get(1) {
            s += d.sqrt();
            n += 1;
        }
    }
    let m = if n > 0 { s / n as f64 } else { 1.0 };
    if m > 0.0 {
        2.5 * m
    } else {
        1.0
    }
}

fn sph_w(r: f64, h: f64, quintic: bool) -> f64 {
    let q = r / h;
    if quintic {
        // Wendland C2 (3-D), support 2h.
        if q >= 2.0 {
            return 0.0;
        }
        let a = 21.0 / (16.0 * std::f64::consts::PI * h * h * h);
        a * (1.0 - q / 2.0).powi(4) * (2.0 * q + 1.0)
    } else {
        // Cubic spline (3-D), support 2h.
        let a = 1.0 / (std::f64::consts::PI * h * h * h);
        if q < 1.0 {
            a * (1.0 - 1.5 * q * q + 0.75 * q * q * q)
        } else if q < 2.0 {
            a * 0.25 * (2.0 - q).powi(3)
        } else {
            0.0
        }
    }
}

/// Interpolate the source's point arrays at `targets`. The output holds one array per source
/// array and `vtkValidPointMask` (1 where source points were found).
pub fn interpolate(src: &DataSet, targets: &[[f64; 3]], opt: &Options) -> Result<Attributes> {
    let (pts, pd) = source_points(src)?;
    if pts.is_empty() {
        return Err(anyhow!("the source has no points"));
    }
    let arrays: Vec<&Array> = pd.iter().filter(|a| a.scalar_type() != ScalarType::String).collect();
    if arrays.is_empty() {
        return Err(anyhow!("the source has no point arrays to interpolate"));
    }
    let tree = KdTree::new(&pts);
    let radius = match (opt.kernel, opt.neighbours) {
        (Kernel::Sph { h, .. }, _) => 2.0 * if h > 0.0 { h } else { auto_radius(&tree, &pts) / 2.5 * 1.3 },
        (_, Neighbours::Radius(r)) if r > 0.0 => r,
        _ => auto_radius(&tree, &pts),
    };
    let h = radius / 2.0;
    let volume: Option<Vec<f64>> = match opt.kernel {
        Kernel::Sph { .. } if !opt.density.is_empty() => {
            let rho = pd.get(&opt.density).ok_or_else(|| anyhow!("no point array named {}", opt.density))?;
            let mass = if opt.mass.is_empty() { None } else { Some(pd.get(&opt.mass).ok_or_else(|| anyhow!("no point array named {}", opt.mass))?) };
            Some((0..pts.len()).map(|i| mass.map(|m| m.get(i, 0)).unwrap_or(1.0) / rho.get(i, 0).max(1e-300)).collect())
        }
        _ => None,
    };
    // For every target: (source index, weight) pairs, already normalised where the kernel is.
    let weights: Vec<Vec<(usize, f64)>> = targets
        .par_iter()
        .map(|&q| {
            let near = match (opt.kernel, opt.neighbours) {
                (Kernel::Voronoi, _) => tree.nearest(q, 1),
                (Kernel::Sph { .. }, _) => tree.within(q, radius),
                (_, Neighbours::Closest(n)) => tree.nearest(q, n.max(1)),
                (_, Neighbours::Radius(_)) => tree.within(q, radius),
            };
            if near.is_empty() {
                return Vec::new();
            }
            if let Some(&(i, d2)) = near.iter().find(|n| n.1 == 0.0) {
                if !matches!(opt.kernel, Kernel::Sph { .. }) {
                    return vec![(i, 1.0)];
                }
                let _ = d2;
            }
            let mut w: Vec<(usize, f64)> = near
                .iter()
                .map(|&(i, d2)| {
                    let d = d2.sqrt();
                    let wt = match opt.kernel {
                        Kernel::Gaussian { sharpness } => (-(sharpness * d / radius).powi(2)).exp(),
                        Kernel::Shepard { power } => 1.0 / d.powf(power),
                        Kernel::Linear | Kernel::Voronoi => 1.0,
                        Kernel::Sph { quintic, .. } => sph_w(d, h, quintic) * volume.as_ref().map(|v| v[i]).unwrap_or(1.0),
                    };
                    (i, wt)
                })
                .collect();
            let sum: f64 = w.iter().map(|x| x.1).sum();
            let normalise = !(matches!(opt.kernel, Kernel::Sph { .. }) && volume.is_some());
            if normalise {
                if sum <= 0.0 || !sum.is_finite() {
                    return Vec::new();
                }
                for x in &mut w {
                    x.1 /= sum;
                }
            }
            w
        })
        .collect();
    let mut out = Attributes::new();
    for arr in arrays {
        let nc = arr.ncomp;
        let mut v = vec![0.0; targets.len() * nc];
        for (t, w) in weights.iter().enumerate() {
            for c in 0..nc {
                v[t * nc + c] = if w.is_empty() {
                    match opt.null_value {
                        Some(x) => x,
                        None => tree.nearest_one(targets[t]).map(|(i, _)| arr.get(i, c)).unwrap_or(f64::NAN),
                    }
                } else {
                    w.iter().map(|&(i, wt)| wt * arr.get(i, c)).sum()
                };
            }
        }
        let mut a = Array::new(arr.name.clone(), nc, ArrayData::F64(v));
        a.component_names = arr.component_names.clone();
        out.insert(a);
    }
    out.insert(Array::new("vtkValidPointMask", 1, ArrayData::F64(weights.iter().map(|w| if w.is_empty() { 0.0 } else { 1.0 }).collect())));
    if let Some(s) = pd.active(AttributeKind::Scalars) {
        out.set_active(AttributeKind::Scalars, &s.name.clone());
    }
    Ok(out)
}

/// Where the interpolated values go.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Target {
    /// A grid of `dims` points over the source's bounds.
    Volume([usize; 3]),
    /// A plane across the source's bounds, normal to axis 0/1/2, at `at` (0..1) of the bounds.
    Plane { axis: usize, at: f64, dims: [usize; 2] },
    /// A line of `resolution` segments.
    Line { p1: [f64; 3], p2: [f64; 3], resolution: usize },
}

/// Interpolate onto a generated grid, plane or line.
pub fn interpolate_onto(src: &DataSet, target: Target, opt: &Options) -> Result<DataSet> {
    let b = src.bounds();
    if b.is_empty() {
        return Err(anyhow!("the source is empty"));
    }
    match target {
        Target::Volume(dims) => {
            let d = [dims[0].max(1), dims[1].max(1), dims[2].max(1)];
            let img = grid_over(&b, d, None);
            finish_image(src, img, opt)
        }
        Target::Plane { axis, at, dims } => {
            let mut d = [dims[0].max(1), dims[1].max(1), 1];
            let (u, v) = match axis {
                0 => (1, 2),
                1 => (0, 2),
                _ => (0, 1),
            };
            let mut full = [1usize; 3];
            full[u] = d[0];
            full[v] = d[1];
            d = full;
            let img = grid_over(&b, d, Some((axis, b.min[axis] + at.clamp(0.0, 1.0) * (b.max[axis] - b.min[axis]))));
            finish_image(src, img, opt)
        }
        Target::Line { p1, p2, resolution } => {
            let n = resolution.max(1) + 1;
            let pts: Vec<[f64; 3]> = (0..n)
                .map(|i| {
                    let t = i as f64 / (n - 1) as f64;
                    [p1[0] + t * (p2[0] - p1[0]), p1[1] + t * (p2[1] - p1[1]), p1[2] + t * (p2[2] - p1[2])]
                })
                .collect();
            let len = ((p2[0] - p1[0]).powi(2) + (p2[1] - p1[1]).powi(2) + (p2[2] - p1[2]).powi(2)).sqrt();
            let mut attrs = interpolate(src, &pts, opt)?;
            attrs.insert(Array::new("arc_length", 1, ArrayData::F64((0..n).map(|i| len * i as f64 / (n - 1) as f64).collect())));
            let mut lines = Cells::new();
            lines.push(CellType::PolyLine, &(0..n as u32).collect::<Vec<_>>());
            Ok(DataSet::PolyData(PolyData { points: Points::new_f64(pts.into_iter().flatten().collect()), lines, point_data: attrs, ..Default::default() }))
        }
    }
}

fn grid_over(b: &Bounds, d: [usize; 3], flat: Option<(usize, f64)>) -> ImageData {
    let s = b.size();
    let mut origin = b.min;
    let mut spacing = [0.0; 3];
    for a in 0..3 {
        spacing[a] = if d[a] > 1 { s[a] / (d[a] - 1) as f64 } else { 1.0 };
    }
    if let Some((axis, at)) = flat {
        origin[axis] = at;
    }
    ImageData { extent: [0, d[0] as i64 - 1, 0, d[1] as i64 - 1, 0, d[2] as i64 - 1], origin, spacing, ..Default::default() }
}

fn finish_image(src: &DataSet, mut img: ImageData, opt: &Options) -> Result<DataSet> {
    let d = img.dims();
    let mut pts = Vec::with_capacity(d[0] * d[1] * d[2]);
    for k in 0..d[2] {
        for j in 0..d[1] {
            for i in 0..d[0] {
                pts.push([img.origin[0] + i as f64 * img.spacing[0], img.origin[1] + j as f64 * img.spacing[1], img.origin[2] + k as f64 * img.spacing[2]]);
            }
        }
    }
    img.point_data = interpolate(src, &pts, opt)?;
    Ok(DataSet::ImageData(img))
}

/// Interpolate onto the points of another dataset, keeping its cells and arrays.
pub fn interpolate_onto_dataset(src: &DataSet, dst: &DataSet, opt: &Options) -> Result<DataSet> {
    crate::registry::per_leaf(dst, &|leaf| {
        let pts = leaf.points().ok_or_else(|| anyhow!("the destination has no points"))?;
        let targets: Vec<[f64; 3]> = (0..pts.len()).map(|i| pts.get(i)).collect();
        let attrs = interpolate(src, &targets, opt)?;
        let mut out = leaf.clone();
        let pd = out.point_data_mut().ok_or_else(|| anyhow!("the destination cannot hold point arrays"))?;
        for a in attrs.iter() {
            pd.insert(a.clone());
        }
        Ok(out)
    })
}

/// The Point Interpolator filter: options and target from its parameters; a second input is the
/// destination.
pub fn run(inputs: &[&DataSet], p: &crate::params::Params) -> Result<DataSet> {
    let kernel = match p.str("kernel").as_str() {
        "Gaussian" => Kernel::Gaussian { sharpness: p.f64("sharpness", 2.0) },
        "Linear" => Kernel::Linear,
        "Voronoi" | "Nearest point" => Kernel::Voronoi,
        "SPH cubic" => Kernel::Sph { h: p.f64("smoothing_length", 0.0), quintic: false },
        "SPH quintic" => Kernel::Sph { h: p.f64("smoothing_length", 0.0), quintic: true },
        _ => Kernel::Shepard { power: p.f64("power", 2.0) },
    };
    let neighbours =
        if p.str("neighbours") == "Radius" { Neighbours::Radius(p.f64("radius", 0.0)) } else { Neighbours::Closest(p.i64("n_closest", 8).max(1) as usize) };
    let null_value = if p.str("null") == "Nearest point" { None } else { Some(p.f64("null_value", 0.0)) };
    let opt = Options { kernel, neighbours, null_value, mass: p.str("mass"), density: p.str("density") };
    let src = inputs[0];
    if let Some(dst) = inputs.get(1) {
        return interpolate_onto_dataset(src, dst, &opt);
    }
    let b = src.bounds();
    let target = match p.str("output").as_str() {
        "Plane" => Target::Plane {
            axis: match p.str("axis").as_str() {
                "X" => 0,
                "Y" => 1,
                _ => 2,
            },
            at: p.f64("at", 0.5),
            dims: [p.i64("nx", 50).max(1) as usize, p.i64("ny", 50).max(1) as usize],
        },
        "Line" => {
            let p1 = p.vec3("point1", [f64::NAN; 3]);
            let p2 = p.vec3("point2", [f64::NAN; 3]);
            Target::Line {
                p1: if p1.iter().all(|v| v.is_finite()) { p1 } else { b.min },
                p2: if p2.iter().all(|v| v.is_finite()) { p2 } else { b.max },
                resolution: p.i64("resolution", 200).max(1) as usize,
            }
        }
        _ => Target::Volume([p.i64("nx", 50).max(1) as usize, p.i64("ny", 50).max(1) as usize, p.i64("nz", 50).max(1) as usize]),
    };
    interpolate_onto(src, target, &opt)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cloud(n: usize) -> PolyData {
        let mut s = 0x2545_F491_4F6C_DD1Du64;
        let mut r = || {
            s ^= s << 13;
            s ^= s >> 7;
            s ^= s << 17;
            (s >> 11) as f64 / (1u64 << 53) as f64
        };
        let pts: Vec<[f64; 3]> = (0..n).map(|_| [r(), r(), r()]).collect();
        let f: Vec<f64> = pts.iter().map(|p| 2.0 * p[0] + 3.0 * p[1] - p[2] + 1.0).collect();
        let mut pd = PolyData { points: Points::new_f64(pts.iter().flatten().copied().collect()), ..Default::default() };
        pd.point_data.insert(Array::scalars_f64("f", f));
        pd.point_data.insert(Array::scalars_f64("rho", vec![n as f64; n]));
        pd
    }

    fn max_err(out: &Attributes, targets: &[[f64; 3]]) -> f64 {
        let f = out.get("f").unwrap();
        targets.iter().enumerate().map(|(i, p)| (f.get(i, 0) - (2.0 * p[0] + 3.0 * p[1] - p[2] + 1.0)).abs()).fold(0.0, f64::max)
    }

    #[test]
    fn kernels_reproduce_a_linear_field() {
        let src = DataSet::PolyData(cloud(20000));
        let targets: Vec<[f64; 3]> = (0..200)
            .map(|i| [0.2 + 0.6 * ((i * 37) % 200) as f64 / 200.0, 0.2 + 0.6 * ((i * 53) % 200) as f64 / 200.0, 0.2 + 0.6 * (i as f64 / 200.0)])
            .collect();
        let field_range = 6.0;
        for (kernel, tol) in
            [(Kernel::Shepard { power: 2.0 }, 0.02), (Kernel::Gaussian { sharpness: 2.0 }, 0.02), (Kernel::Linear, 0.03), (Kernel::Voronoi, 0.06)]
        {
            let opt = Options { kernel, ..Default::default() };
            let out = interpolate(&src, &targets, &opt).unwrap();
            let e = max_err(&out, &targets);
            assert!(e < tol * field_range, "{kernel:?}: {e}");
            assert!(out.get("vtkValidPointMask").unwrap().get(0, 0) == 1.0);
        }
        // SPH with mass and density (volume = mass / density) on a lattice: the kernel sum is the
        // integral of the field, within a fraction of a percent inside.
        let n = 31;
        let sp = 1.0 / (n - 1) as f64;
        let mut lat = PolyData::default();
        let (mut f, mut m) = (Vec::new(), Vec::new());
        for k in 0..n {
            for j in 0..n {
                for i in 0..n {
                    let p = [i as f64 * sp, j as f64 * sp, k as f64 * sp];
                    lat.points.push(p);
                    f.push(2.0 * p[0] + 3.0 * p[1] - p[2] + 1.0);
                    m.push(2.0 * sp * sp * sp);
                }
            }
        }
        lat.point_data.insert(Array::scalars_f64("f", f));
        lat.point_data.insert(Array::scalars_f64("m", m));
        lat.point_data.insert(Array::scalars_f64("rho", vec![2.0; n * n * n]));
        let lat = DataSet::PolyData(lat);
        for quintic in [false, true] {
            let opt = Options { kernel: Kernel::Sph { h: 1.3 * sp, quintic }, density: "rho".into(), mass: "m".into(), ..Default::default() };
            let out = interpolate(&lat, &targets, &opt).unwrap();
            assert!(max_err(&out, &targets) < 0.01 * field_range, "sph {quintic} {}", max_err(&out, &targets));
        }
        let opt = Options { kernel: Kernel::Sph { h: 0.05, quintic: true }, ..Default::default() };
        let out = interpolate(&src, &targets, &opt).unwrap();
        assert!(max_err(&out, &targets) < 0.03 * field_range, "sph normalised {}", max_err(&out, &targets));
        // At a source point the value is exact.
        let p = cloud(20000).points.get(5);
        let out = interpolate(&src, &[p], &Options::default()).unwrap();
        assert!((out.get("f").unwrap().get(0, 0) - (2.0 * p[0] + 3.0 * p[1] - p[2] + 1.0)).abs() < 1e-12);
    }

    #[test]
    fn targets_and_null_values() {
        let src = DataSet::PolyData(cloud(3000));
        let vol = interpolate_onto(&src, Target::Volume([5, 6, 7]), &Options::default()).unwrap();
        assert_eq!(vol.n_points(), 210);
        let plane = interpolate_onto(&src, Target::Plane { axis: 2, at: 0.5, dims: [10, 12] }, &Options::default()).unwrap();
        let DataSet::ImageData(im) = &plane else { panic!() };
        assert_eq!(im.dims(), [10, 12, 1]);
        let line = interpolate_onto(&src, Target::Line { p1: [0.0; 3], p2: [1.0; 3], resolution: 10 }, &Options::default()).unwrap();
        assert_eq!(line.n_points(), 11);
        // Far from every point, a radius search finds nothing: the null value, or the nearest point's.
        let far = [[10.0, 10.0, 10.0]];
        let opt = Options { neighbours: Neighbours::Radius(0.1), null_value: Some(-1.0), ..Default::default() };
        let out = interpolate(&src, &far, &opt).unwrap();
        assert_eq!(out.get("f").unwrap().get(0, 0), -1.0);
        assert_eq!(out.get("vtkValidPointMask").unwrap().get(0, 0), 0.0);
        let opt = Options { null_value: None, ..opt };
        assert!(interpolate(&src, &far, &opt).unwrap().get("f").unwrap().get(0, 0) > 0.0);
    }
}
