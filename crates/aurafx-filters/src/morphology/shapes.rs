//! The description of a microstructure made of objects, kept with its image: every sphere,
//! fibre piece and strut as two end points and two radii. From it come the objects as points
//! and lines, a smooth surface at any resolution, and the rigidity of a frame of struts.

use super::bio::phases_of;
use super::*;
use anyhow::{anyhow, bail, Result};

/// One object: the ends of its axis and the radius at each (a sphere when the ends are the same).
pub type Shape = [f64; 8];

pub fn sphere(c: [f64; 3], r: f64) -> Shape {
    [c[0], c[1], c[2], c[0], c[1], c[2], r, r]
}

pub fn capsule(a: [f64; 3], b: [f64; 3], r: f64) -> Shape {
    [a[0], a[1], a[2], b[0], b[1], b[2], r, r]
}

/// Keep the description with the image (nothing when there are too many objects to be of use).
pub fn keep(img: &mut ImageData, shapes: &[Shape], periodic: bool) {
    if shapes.is_empty() || shapes.len() > 2_000_000 {
        return;
    }
    img.field_data.insert(Array::new("Shapes", 8, ArrayData::F64(shapes.iter().flatten().copied().collect())));
    img.field_data.insert(Array::scalars_f64("ShapesPeriodic", vec![periodic as u8 as f64]));
}

/// The description kept with an image, and whether the structure wraps through the box.
pub fn of(ds: &DataSet) -> Result<(ImageData, Vec<Shape>, bool)> {
    let DataSet::ImageData(img) = ds else { bail!("the input must be a synthetic microstructure (an image)") };
    let a = img.field_data.get("Shapes").filter(|a| a.ncomp == 8).ok_or_else(|| {
        anyhow!("this microstructure keeps no description of its objects: only those made of spheres, fibres or struts do (and only until they are stretched or combined)")
    })?;
    let shapes = (0..a.len()).map(|k| std::array::from_fn(|j| a.get(k, j))).collect();
    let periodic = img.field_data.get("ShapesPeriodic").is_some_and(|p| p.len() > 0 && p.get(0, 0) > 0.5);
    Ok((img.clone(), shapes, periodic))
}

/// Microstructure Objects: the spheres as points and the fibre pieces and struts as lines,
/// with their radius, ready for glyphs and tubes or for a mesher.
pub fn objects(ds: &DataSet) -> Result<DataSet> {
    let (_, shapes, _) = of(ds)?;
    let mut pts: Vec<f64> = Vec::new();
    let mut point_radius: Vec<f64> = Vec::new();
    let (mut verts, mut lines) = (aurafx_data::Cells::new(), aurafx_data::Cells::new());
    let (mut vert_data, mut line_data): (Vec<[f64; 2]>, Vec<[f64; 2]>) = (Vec::new(), Vec::new());
    for s in &shapes {
        let (a, b) = ([s[0], s[1], s[2]], [s[3], s[4], s[5]]);
        let first = (pts.len() / 3) as u32;
        pts.extend(a);
        point_radius.push(s[6]);
        if a == b {
            verts.push(CellType::Vertex, &[first]);
            vert_data.push([s[6], 0.0]);
        } else {
            pts.extend(b);
            point_radius.push(s[7]);
            lines.push(CellType::Line, &[first, first + 1]);
            line_data.push([0.5 * (s[6] + s[7]), norm(sub(b, a))]);
        }
    }
    let mut pd = PolyData { points: Points::new_f64(pts), verts, lines, ..Default::default() };
    pd.point_data.insert(Array::scalars_f64("Radius", point_radius));
    let all: Vec<[f64; 2]> = vert_data.into_iter().chain(line_data).collect();
    pd.cell_data.insert(Array::scalars_f64("Radius", all.iter().map(|d| d[0]).collect()));
    pd.cell_data.insert(Array::scalars_f64("Length", all.iter().map(|d| d[1]).collect()));
    Ok(DataSet::PolyData(pd))
}

/// White noise smoothing of a field on the cells by a Gaussian of `sigma` cells (the faces
/// of the box repeat their values).
fn smoothed(n: [usize; 3], field: Vec<f32>, sigma: f64) -> Vec<f32> {
    if !(sigma > 0.0) {
        return field;
    }
    let reach = (3.0 * sigma).ceil() as i64;
    let kernel: Vec<f32> = {
        let k: Vec<f64> = (-reach..=reach).map(|i| (-0.5 * (i as f64 / sigma).powi(2)).exp()).collect();
        let s: f64 = k.iter().sum();
        k.iter().map(|x| (x / s) as f32).collect()
    };
    let stride = [1, n[0], n[0] * n[1]];
    let mut field = field;
    for axis in 0..3 {
        if n[axis] == 1 {
            continue;
        }
        let src = field;
        field = (0..src.len())
            .into_par_iter()
            .map(|c| {
                let i = ((c / stride[axis]) % n[axis]) as i64;
                let base = c - i as usize * stride[axis];
                kernel.iter().enumerate().map(|(k, w)| w * src[base + (i + k as i64 - reach).clamp(0, n[axis] as i64 - 1) as usize * stride[axis]]).sum()
            })
            .collect();
    }
    field
}

/// Smooth Surface: the closed surface of a phase as triangles. From the description of the
/// objects when the microstructure keeps one (the exact spheres, round-ended fibres and
/// struts, at `refine` times the resolution of the image); from the cells otherwise, their
/// steps smoothed over `smoothing` cells. The surface is closed where the box cuts the phase.
pub fn surface(ds: &DataSet, array: &str, phase: i32, refine: usize, smoothing: f64, from_cells: bool) -> Result<DataSet> {
    let (img, phases) = phases_of(ds, array)?;
    let v = crate::features::Vox::of(&img);
    let described = if from_cells || phase != PHASE_SOLID { None } else { of(ds).ok() };
    // Values at the centres of a grid of cells, negative inside the phase.
    let (n, h, values): ([usize; 3], [f64; 3], Vec<f32>) = match described {
        Some((_, shapes, periodic)) => {
            let k = refine.clamp(1, 16);
            let n: [usize; 3] = std::array::from_fn(|a| if v.n[a] > 1 { v.n[a] * k } else { 1 });
            if n[0] * n[1] * n[2] > 200_000_000 {
                bail!("more than 200 million points: lower the refinement");
            }
            let g = Grid { n, h: v.sp[0] / k as f64, size: std::array::from_fn(|a| v.n[a] as f64 * v.sp[0]), periodic };
            let mut d = vec![f32::INFINITY; g.len()];
            let band = 2.5 * g.h;
            for s in &shapes {
                let (a, b) = ([s[0], s[1], s[2]], [s[3], s[4], s[5]]);
                let reach = s[6].max(s[7]) + band;
                g.stamp_segment(a, b, reach, |c, dist, t| {
                    let value = (dist - (s[6] + (s[7] - s[6]) * t.clamp(0.0, 1.0))) as f32;
                    if value < d[c] {
                        d[c] = value;
                    }
                });
            }
            let far = band as f32;
            (n, [g.h; 3], d.into_iter().map(|x| x.min(far)).collect())
        }
        None => {
            if !phases.contains(&phase) {
                bail!("no cell of phase {phase}");
            }
            let field: Vec<f32> = phases.iter().map(|p| if *p == phase { -0.5 } else { 0.5 }).collect();
            (v.n, v.sp, smoothed(v.n, field, smoothing))
        }
    };
    // As values at the points of a grid one layer wider, the layer outside the phase.
    let dims: [usize; 3] = std::array::from_fn(|a| if n[a] > 1 { n[a] + 2 } else { 1 });
    if dims[2] == 1 {
        bail!("the surface needs more than one layer of cells");
    }
    let mut padded = vec![1.0f32; dims[0] * dims[1] * dims[2]];
    for (c, x) in values.iter().enumerate() {
        let p = [c % n[0], (c / n[0]) % n[1], c / (n[0] * n[1])];
        padded[(p[0] + 1) + dims[0] * ((p[1] + 1) + dims[1] * (p[2] + 1))] = *x;
    }
    let mut grid = ImageData {
        extent: [0, dims[0] as i64 - 1, 0, dims[1] as i64 - 1, 0, dims[2] as i64 - 1],
        origin: std::array::from_fn(|a| v.corner[a] - 0.5 * h[a]),
        spacing: h,
        ..Default::default()
    };
    grid.point_data.insert(Array::scalars_f32("Distance", padded));
    let pd = crate::contour::contour(&DataSet::ImageData(grid), crate::Association::Points, "Distance", None, &[0.0])?;
    if pd.polys.is_empty() {
        bail!("no surface: the phase is empty or fills the box");
    }
    Ok(DataSet::PolyData(pd))
}

/// The rank of a matrix (rows of equal length), by elimination with the largest entry as pivot.
fn rank(mut rows: Vec<Vec<f64>>) -> usize {
    let cols = rows.first().map(|r| r.len()).unwrap_or(0);
    let scale = rows.iter().flatten().fold(0.0f64, |a, b| a.max(b.abs()));
    if scale == 0.0 {
        return 0;
    }
    let mut used = vec![false; cols];
    let mut rank = 0;
    for r in 0..rows.len() {
        let Some((c, v)) = (0..cols).filter(|c| !used[*c]).map(|c| (c, rows[r][c])).max_by(|a, b| a.1.abs().total_cmp(&b.1.abs())) else { break };
        if v.abs() <= 1e-9 * scale {
            continue;
        }
        used[c] = true;
        rank += 1;
        let pivot = rows[r].clone();
        rows[r + 1..].par_iter_mut().for_each(|row| {
            let f = row[c] / v;
            if f != 0.0 {
                row.iter_mut().zip(&pivot).for_each(|(a, b)| *a -= f * b);
                row[c] = 0.0;
            }
        });
    }
    rank
}

/// Framework Rigidity: the struts of a lattice as bars joined by pins. Counting the bars
/// against the freedoms of the nodes tells only their difference; the rank of the matrix
/// that gives the stretch of every bar from the movements of the nodes tells both the ways
/// the frame can move without stretching a bar (mechanisms) and the ways its bars can be
/// stressed without load (states of self-stress), and, for a periodic lattice, the mean
/// strains it cannot resist.
pub fn rigidity(ds: &DataSet) -> Result<DataSet> {
    let (img, shapes, periodic) = of(ds)?;
    let v = crate::features::Vox::of(&img);
    let size: [f64; 3] = std::array::from_fn(|k| v.n[k] as f64 * v.sp[k]);
    let tol = 1e-6 * norm(size);
    let key = |p: [f64; 3]| -> [i64; 3] {
        std::array::from_fn(|k| {
            let x = if periodic { p[k].rem_euclid(size[k]) } else { p[k] };
            let q = (x / tol / 4.0).round() as i64;
            // A node on a periodic face is the same on the opposite face.
            if periodic && q == (size[k] / tol / 4.0).round() as i64 {
                0
            } else {
                q
            }
        })
    };
    let mut nodes: std::collections::HashMap<[i64; 3], usize> = std::collections::HashMap::new();
    let mut bars: Vec<(usize, usize, [f64; 3])> = Vec::new();
    for s in &shapes {
        let (a, b) = ([s[0], s[1], s[2]], [s[3], s[4], s[5]]);
        let d = sub(b, a);
        if norm(d) <= tol {
            continue;
        }
        let next = nodes.len();
        let i = *nodes.entry(key(a)).or_insert(next);
        let next = nodes.len();
        let j = *nodes.entry(key(b)).or_insert(next);
        bars.push((i, j, unit(d)));
    }
    let (count, joints) = (bars.len(), nodes.len());
    if count == 0 {
        bail!("the description holds no strut");
    }
    if count * 3 * joints > 12_000_000 {
        bail!("{count} struts between {joints} nodes are too many to analyse: crop the lattice to a few cells first");
    }
    let rows = |strain: bool| -> Vec<Vec<f64>> {
        bars.iter()
            .map(|(i, j, d)| {
                let mut row = vec![0.0; 3 * joints + if strain { 6 } else { 0 }];
                for k in 0..3 {
                    row[3 * i + k] -= d[k];
                    row[3 * j + k] += d[k];
                }
                if strain {
                    row[3 * joints..].copy_from_slice(&[d[0] * d[0], d[1] * d[1], d[2] * d[2], 2.0 * d[1] * d[2], 2.0 * d[0] * d[2], 2.0 * d[0] * d[1]]);
                }
                row
            })
            .collect()
    };
    let r = rank(rows(false));
    let rigid_motions = if periodic { 3 } else { 6 };
    let mut t = Table::default();
    let mut put = |name: &str, x: f64| {
        t.columns.insert(Array::scalars_f64(name, vec![x]));
    };
    put("Nodes", joints as f64);
    put("Struts", count as f64);
    put("StrutsPerNode", 2.0 * count as f64 / joints as f64);
    put("MaxwellNumber", count as f64 - 3.0 * joints as f64 + rigid_motions as f64);
    put("Mechanisms", (3 * joints) as f64 - rigid_motions as f64 - r as f64);
    put("StatesOfSelfStress", count as f64 - r as f64);
    if periodic {
        put("FloppyStrains", 6.0 - (rank(rows(true)) - r) as f64);
    }
    Ok(DataSet::Table(t))
}
