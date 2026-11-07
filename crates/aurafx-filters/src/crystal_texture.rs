//! Texture filters: pole figures, inverse pole figure densities and ODF sections of the
//! orientations of a dataset (see `aurafx_crystal::texture` for the definitions).

use crate::crystal::{classes_of, orientation_array, Repr};
use crate::Association;
use anyhow::{anyhow, bail, Result};
use aurafx_crystal::texture as tx;
use aurafx_crystal::{Laue, Quat};
use aurafx_data::*;

pub const STEREOGRAPHIC: &str = "Stereographic";
pub const EQUAL_AREA: &str = "Equal area";

/// Which orientations to use: one phase (0: every indexed point), an optional mask, and at
/// most `max` of them (a seeded random subset when there are more).
pub struct Selection<'a> {
    pub orientations: Option<(Association, String)>,
    pub structure: &'a str,
    pub phases: Option<&'a str>,
    pub phase: i64,
    pub mask: Option<&'a str>,
    pub max: usize,
    pub seed: u64,
}

/// The orientations chosen, their class, and how many there were before subsampling.
pub struct Sample {
    pub q: Vec<Quat>,
    pub laue: Laue,
    pub available: usize,
}

/// Every point's orientation when it is selected (None otherwise), and their crystal class.
pub fn selected(ds: &DataSet, sel: &Selection) -> Result<(Vec<Option<Quat>>, Laue)> {
    let (assoc, a) = orientation_array(ds, sel.orientations.clone())?;
    let repr = Repr::guess(a.ncomp).ok_or_else(|| anyhow!("{}: {} components is not an orientation array (3, 4 or 9)", a.name, a.ncomp))?;
    let n = a.len();
    let cls = classes_of(ds, assoc, n, sel.structure, sel.phases)?;
    let attrs = match assoc {
        Association::Cells => ds.cell_data(),
        Association::Points => ds.point_data(),
    };
    let pname = sel.phases.filter(|p| !p.is_empty()).unwrap_or("Phases");
    let ph = attrs.and_then(|x| x.get(pname));
    let mask = match sel.mask.filter(|m| !m.is_empty()) {
        Some(m) => Some(attrs.and_then(|x| x.get(m)).ok_or_else(|| anyhow!("no mask array {m}"))?),
        None => None,
    };
    let mut laue: Option<Laue> = None;
    let mut out = vec![None; n];
    for (i, o) in out.iter_mut().enumerate() {
        let p = ph.map(|p| p.get(i, 0) as i64).unwrap_or(1);
        if p <= 0 || (sel.phase > 0 && p != sel.phase) || mask.is_some_and(|m| m.get(i, 0) == 0.0) {
            continue;
        }
        let Some(l) = cls[i] else { continue };
        match laue {
            None => laue = Some(l),
            Some(k) if k != l => bail!("the points belong to more than one crystal class: choose a phase"),
            _ => {}
        }
        *o = Some(repr.decode_values(a, i));
    }
    let laue = laue.ok_or_else(|| anyhow!("no indexed orientations to analyse (check the phase and the mask)"))?;
    Ok((out, laue))
}

fn rng(seed: u64) -> impl FnMut() -> u64 {
    let mut s = seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1;
    move || {
        s ^= s << 13;
        s ^= s >> 7;
        s ^= s << 17;
        s
    }
}

pub fn sample(ds: &DataSet, sel: &Selection) -> Result<Sample> {
    let (all, laue) = selected(ds, sel)?;
    let mut q: Vec<Quat> = all.into_iter().flatten().collect();
    let available = q.len();
    let m = sel.max.max(1);
    if q.len() > m {
        // A seeded partial shuffle, kept in the original order.
        let mut idx: Vec<usize> = (0..q.len()).collect();
        let mut next = rng(sel.seed);
        for k in 0..m {
            let j = k + (next() % (idx.len() - k) as u64) as usize;
            idx.swap(k, j);
        }
        idx.truncate(m);
        idx.sort_unstable();
        q = idx.iter().map(|i| q[*i]).collect();
    }
    Ok(Sample { q, laue, available })
}

/// Crystal directions written like `100, 110, -111` or `1 0 0; 1 1 2`.
pub fn parse_families(s: &str) -> Result<Vec<(String, [f64; 3])>> {
    let mut out = Vec::new();
    for tok in s.split([',', ';']).map(str::trim).filter(|t| !t.is_empty()) {
        let nums: Vec<f64> = if tok.contains(char::is_whitespace) {
            tok.split_whitespace().map(|x| x.parse::<f64>().map_err(|_| anyhow!("{tok}: not three numbers"))).collect::<Result<_>>()?
        } else {
            let mut v = Vec::new();
            let mut neg = false;
            for c in tok.chars() {
                match c {
                    '-' => neg = true,
                    d if d.is_ascii_digit() => {
                        let x = d.to_digit(10).unwrap() as f64;
                        v.push(if neg { -x } else { x });
                        neg = false;
                    }
                    _ => bail!("{tok}: write directions like 100, 110, -111 or 1 0 0"),
                }
            }
            v
        };
        if nums.len() != 3 || nums.iter().all(|x| *x == 0.0) {
            bail!("{tok}: a direction needs three indices, not all zero");
        }
        out.push((tok.replace(char::is_whitespace, ""), [nums[0], nums[1], nums[2]]));
    }
    if out.is_empty() {
        bail!("no directions given");
    }
    Ok(out)
}

/// Radius in the projection of a direction at polar angle `t` (1 at 90 degrees).
fn radius(t: f64, projection: &str) -> f64 {
    if projection == EQUAL_AREA {
        std::f64::consts::SQRT_2 * (t / 2.0).sin()
    } else {
        (t / 2.0).tan()
    }
}

fn polar_angle(r: f64, projection: &str) -> f64 {
    if projection == EQUAL_AREA {
        2.0 * (r / std::f64::consts::SQRT_2).clamp(-1.0, 1.0).asin()
    } else {
        2.0 * r.atan()
    }
}

/// A disk of `n` rings (the upper hemisphere in projection): plane points, the directions they
/// stand for, and its cells.
fn disk(n: usize, projection: &str) -> (Vec<[f64; 3]>, Vec<[f64; 3]>, Cells) {
    let m = 4 * n;
    let mut pts = vec![[0.0, 0.0, 0.0]];
    let mut dirs = vec![[0.0, 0.0, 1.0]];
    for k in 1..=n {
        let r = k as f64 / n as f64;
        let t = polar_angle(r, projection);
        for j in 0..m {
            let p = j as f64 / m as f64 * std::f64::consts::TAU;
            pts.push([r * p.cos(), r * p.sin(), 0.0]);
            dirs.push([t.sin() * p.cos(), t.sin() * p.sin(), t.cos()]);
        }
    }
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
    (pts, dirs, cells)
}

fn panel(pts: Vec<[f64; 3]>, cells: Cells, name: &str, values: Vec<f64>, extra: Vec<Array>, field: Vec<(&str, f64)>) -> DataSet {
    let mut pd = PolyData { points: Points::new_f64(pts.into_iter().flatten().collect()), polys: cells, ..Default::default() };
    let (lo, hi) = values.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), v| (a.min(*v), b.max(*v)));
    pd.point_data.insert(Array::new(name, 1, ArrayData::F64(values)));
    for a in extra {
        pd.point_data.insert(a);
    }
    pd.field_data.insert(Array::new(&format!("Max{name}"), 1, ArrayData::F64(vec![hi])));
    pd.field_data.insert(Array::new(&format!("Min{name}"), 1, ArrayData::F64(vec![lo])));
    for (k, v) in field {
        pd.field_data.insert(Array::new(k, 1, ArrayData::F64(vec![v])));
    }
    DataSet::PolyData(pd)
}

fn shift(pts: &mut [[f64; 3]], dx: f64, dy: f64) {
    for p in pts {
        p[0] += dx;
        p[1] += dy;
    }
}

/// Pole Figure: one disk per family (sample x to the right, y up, z at the centre), side by
/// side, colored by MRD.
pub fn pole_figures(ds: &DataSet, sel: &Selection, families: &str, projection: &str, sigma_deg: f64, bin_deg: f64, resolution: usize) -> Result<DataSet> {
    let fams = parse_families(families)?;
    let s = sample(ds, sel)?;
    let (pts0, dirs, cells) = disk(resolution.clamp(4, 400), projection);
    let mut mb = MultiBlock::default();
    for (k, (name, hkl)) in fams.iter().enumerate() {
        let v = tx::pole_figure(&s.q, None, *hkl, s.laue, &dirs, sigma_deg.to_radians(), bin_deg);
        let mut pts = pts0.clone();
        shift(&mut pts, 2.3 * k as f64, 0.0);
        let dir = Array::new("SampleDirection", 3, ArrayData::F64(dirs.iter().flatten().copied().collect()));
        mb.blocks.push((format!("{{{name}}}"), Some(panel(pts, cells.clone(), "MRD", v, vec![dir], vec![("Orientations", s.q.len() as f64)]))));
    }
    mb.field_data.insert(Array::new("Orientations", 1, ArrayData::F64(vec![s.q.len() as f64])));
    Ok(DataSet::MultiBlock(mb))
}

/// The sample directions of an Inverse Pole Figure choice.
pub fn ipf_directions(choice: &str) -> Vec<(&'static str, [f64; 3])> {
    let all = [("X", [1.0, 0.0, 0.0]), ("Y", [0.0, 1.0, 0.0]), ("Z", [0.0, 0.0, 1.0])];
    match choice {
        "X" => vec![all[0]],
        "Y" => vec![all[1]],
        "Z" => vec![all[2]],
        _ => all.to_vec(),
    }
}

/// Inverse Pole Figure: the density of the crystal directions along each sample direction, on
/// the standard sector of the class, side by side.
pub fn inverse_pole_figures(
    ds: &DataSet,
    sel: &Selection,
    directions: &str,
    projection: &str,
    sigma_deg: f64,
    bin_deg: f64,
    resolution: usize,
) -> Result<DataSet> {
    let s = sample(ds, sel)?;
    let sector = tx::sector_of(s.laue);
    let n = resolution.clamp(4, 400);
    let grid = tx::sector_grid(&sector, n);
    let at: Vec<[f64; 3]> = grid.iter().map(|(e, c)| [c.sin() * e.cos(), c.sin() * e.sin(), c.cos()]).collect();
    let pts0: Vec<[f64; 3]> = grid
        .iter()
        .map(|(e, c)| {
            let r = radius(*c, projection);
            [r * e.cos(), r * e.sin(), 0.0]
        })
        .collect();
    let mut cells = Cells::new();
    for r in 0..n {
        for k in 0..n {
            let id = |r: usize, k: usize| (r * (n + 1) + k) as u32;
            cells.push(CellType::Quad, &[id(r, k), id(r + 1, k), id(r + 1, k + 1), id(r, k + 1)]);
        }
    }
    let (xmin, xmax) = pts0.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), p| (a.min(p[0]), b.max(p[0])));
    let width = (xmax - xmin).max(0.1) + 0.25;
    let mut mb = MultiBlock::default();
    for (k, (name, d)) in ipf_directions(directions).into_iter().enumerate() {
        let v = tx::inverse_pole_figure(&s.q, None, d, s.laue, &at, sigma_deg.to_radians(), bin_deg);
        let mut pts = pts0.clone();
        shift(&mut pts, width * k as f64, 0.0);
        let dir = Array::new("CrystalDirection", 3, ArrayData::F64(at.iter().flatten().copied().collect()));
        mb.blocks.push((format!("IPF {name}"), Some(panel(pts, cells.clone(), "MRD", v, vec![dir], vec![("Orientations", s.q.len() as f64)]))));
    }
    mb.field_data.insert(Array::new("Orientations", 1, ArrayData::F64(vec![s.q.len() as f64])));
    Ok(DataSet::MultiBlock(mb))
}

/// ODF Sections: the ODF on sections of constant φ2 (φ1 to the right, Φ downwards, degrees),
/// one under the other, with the texture index and the ODF maximum as field data.
pub fn odf_sections(ds: &DataSet, sel: &Selection, sections: &str, step_deg: f64, sigma_deg: f64, index_step_deg: f64) -> Result<DataSet> {
    let phi2: Vec<f64> = sections
        .split([',', ';', ' '])
        .filter(|t| !t.trim().is_empty())
        .map(|t| t.trim().parse::<f64>().map_err(|_| anyhow!("{t}: sections are φ2 angles in degrees, like 0, 45, 65")))
        .collect::<Result<_>>()?;
    if phi2.is_empty() {
        bail!("no sections given");
    }
    let step = step_deg.clamp(0.5, 30.0);
    let s = sample(ds, sel)?;
    let sigma = sigma_deg.to_radians();
    let (_, phi_max) = tx::euler_region(s.laue);
    let (j, fmax) = tx::texture_index(&s.q, sigma, s.laue, index_step_deg.clamp(1.0, 30.0).to_radians());
    let field = vec![("TextureIndex", j), ("MaxODF", fmax), ("Orientations", s.q.len() as f64)];
    let mut mb = MultiBlock::default();
    let height = phi_max.to_degrees() + 25.0;
    for (k, p2) in phi2.iter().enumerate() {
        let (eul, n1, n2) = tx::section_grid(p2.to_radians(), step.to_radians(), phi_max);
        let g: Vec<Quat> = eul.iter().map(|e| aurafx_crystal::euler_to_quat(*e)).collect();
        let v = tx::odf_values(&g, &s.q, sigma, s.laue);
        let pts: Vec<[f64; 3]> = eul.iter().map(|e| [e[0].to_degrees(), -e[1].to_degrees() - height * k as f64, 0.0]).collect();
        let mut cells = Cells::new();
        for r in 0..n2 - 1 {
            for c in 0..n1 - 1 {
                let id = |r: usize, c: usize| (r * n1 + c) as u32;
                cells.push(CellType::Quad, &[id(r, c), id(r, c + 1), id(r + 1, c + 1), id(r + 1, c)]);
            }
        }
        let ang = |c: usize, name: &str| Array::new(name, 1, ArrayData::F64(eul.iter().map(|e| e[c].to_degrees()).collect()));
        let mut f = field.clone();
        f.push(("phi2", *p2));
        mb.blocks.push((format!("phi2 = {p2}"), Some(panel(pts, cells, "ODF", v, vec![ang(0, "phi1"), ang(1, "Phi"), ang(2, "phi2")], f))));
    }
    for (k, v) in field {
        mb.field_data.insert(Array::new(k, 1, ArrayData::F64(vec![v])));
    }
    Ok(DataSet::MultiBlock(mb))
}

// ------------------------------------------------------------------ G3b

fn table(cols: Vec<Array>, field: Vec<(&str, f64)>) -> DataSet {
    let mut t = Table::default();
    for c in cols {
        t.columns.insert(c);
    }
    for (k, v) in field {
        t.field_data.insert(Array::new(k, 1, ArrayData::F64(vec![v])));
    }
    DataSet::Table(t)
}

fn col(name: &str, v: Vec<f64>) -> Array {
    Array::new(name, 1, ArrayData::F64(v))
}

/// Misorientation Distribution: the disorientation angles (from `min_deg` up) between face
/// neighbours, between random pairs of points, and between random orientations (exact), each
/// as a density per degree over the angles shown.
pub fn misorientation_distribution(ds: &DataSet, sel: &Selection, min_deg: f64, bin_deg: f64, pairs: usize) -> Result<DataSet> {
    let img = crate::features::image(ds)?;
    let v = crate::features::Vox::of(img);
    let (q, laue) = selected(ds, sel)?;
    if q.len() != v.len() {
        bail!("the orientations must be cell data");
    }
    let uni = tx::random_disorientations(laue);
    let bin = bin_deg.clamp(0.1, 30.0);
    let lo = min_deg.clamp(0.0, 179.0);
    let top = uni.max_angle().to_degrees();
    let nb = (((top - lo) / bin).ceil() as usize).max(1);
    let hist = |angles: &[f64]| -> (Vec<f64>, usize) {
        let mut h = vec![0.0; nb];
        let mut n = 0;
        for a in angles {
            if *a >= lo {
                h[(((a - lo) / bin) as usize).min(nb - 1)] += 1.0;
                n += 1;
            }
        }
        (h.into_iter().map(|c| if n > 0 { c / n as f64 / bin } else { 0.0 }).collect(), n)
    };
    use rayon::prelude::*;
    let nbr: Vec<f64> = (0..v.len())
        .into_par_iter()
        .flat_map_iter(|c| {
            let nb = v.nbrs(c);
            let q = &q;
            [nb[3], nb[4], nb[5]].into_iter().flatten().filter_map(move |d| match (q[c], q[d]) {
                (Some(a), Some(b)) => Some(aurafx_crystal::disorientation_angle(a, b, laue).to_degrees()),
                _ => None,
            })
        })
        .collect();
    let (hn, above) = hist(&nbr);
    let idx: Vec<usize> = (0..q.len()).filter(|i| q[*i].is_some()).collect();
    let mut next = rng(7);
    let rnd: Vec<f64> = (0..pairs)
        .map(|_| {
            let a = idx[(next() % idx.len() as u64) as usize];
            let b = idx[(next() % idx.len() as u64) as usize];
            aurafx_crystal::disorientation_angle(q[a].unwrap(), q[b].unwrap(), laue).to_degrees()
        })
        .collect();
    let (hr, _) = hist(&rnd);
    let r = |d: f64| d.to_radians();
    let total = uni.fraction(r(lo), std::f64::consts::PI);
    let hu: Vec<f64> = (0..nb).map(|k| uni.fraction(r(lo + k as f64 * bin), r(lo + (k + 1) as f64 * bin)) / total / bin).collect();
    let starts: Vec<f64> = (0..nb).map(|k| lo + k as f64 * bin).collect();
    let mean = nbr.iter().filter(|a| **a >= lo).sum::<f64>() / above.max(1) as f64;
    Ok(table(
        vec![
            col("bin_start", starts.clone()),
            col("bin_end", starts.iter().map(|s| s + bin).collect()),
            col("bin_center", starts.iter().map(|s| s + bin / 2.0).collect()),
            col("Neighbours", hn),
            col("Random pairs", hr),
            col("Uniform", hu),
        ],
        vec![("NeighbourPairs", nbr.len() as f64), ("NeighbourPairsShown", above as f64), ("MeanNeighbourAngle", mean)],
    ))
}

fn axes(s: &str) -> Result<Vec<(String, [f64; 3])>> {
    let mut out = Vec::new();
    for t in s.split([',', ';', ' ']).map(str::trim).filter(|t| !t.is_empty()) {
        let v = match t.to_ascii_uppercase().as_str() {
            "X" => [1.0, 0.0, 0.0],
            "Y" => [0.0, 1.0, 0.0],
            "Z" => [0.0, 0.0, 1.0],
            _ => bail!("{t}: sample axes are X, Y or Z"),
        };
        out.push((t.to_ascii_uppercase(), v));
    }
    if out.is_empty() {
        bail!("no sample axes given");
    }
    Ok(out)
}

/// Texture Components: for every crystal direction family and sample axis, the fraction of the
/// orientations with a direction of the family within `cone_deg` of the axis.
pub fn texture_components(ds: &DataSet, sel: &Selection, families: &str, sample_axes: &str, cone_deg: f64) -> Result<DataSet> {
    let fams = parse_families(families)?;
    let ax = axes(sample_axes)?;
    let s = sample(ds, sel)?;
    let cmin = cone_deg.to_radians().cos();
    let (mut dname, mut aname, mut frac, mut count) = (Vec::new(), Vec::new(), Vec::new(), Vec::new());
    for (name, hkl) in &fams {
        let fam = tx::pole_family(*hkl, s.laue);
        for (an, a) in &ax {
            let n =
                s.q.iter()
                    .filter(|g| {
                        let c = aurafx_crystal::rotate(**g, *a);
                        fam.iter().any(|p| (c[0] * p[0] + c[1] * p[1] + c[2] * p[2]) / (p[0] * p[0] + p[1] * p[1] + p[2] * p[2]).sqrt() >= cmin)
                    })
                    .count();
            dname.push(format!("<{name}>"));
            aname.push(an.clone());
            frac.push(n as f64 / s.q.len() as f64);
            count.push(n as f64);
        }
    }
    Ok(table(
        vec![Array::new("Direction", 1, ArrayData::String(dname)), Array::new("Axis", 1, ArrayData::String(aname)), col("Fraction", frac), col("Count", count)],
        vec![("Orientations", s.q.len() as f64), ("Cone", cone_deg)],
    ))
}

/// The chords of one line of points (None: not used): the distances between successive
/// boundaries (disorientation of at least `threshold`, radians) between the valid points, with
/// runs of more than `max_gap` invalid points ending a segment; chords touching a segment end
/// are dropped. `despeckle`: a single point that differs from two neighbours which agree is
/// not a boundary.
pub fn line_chords(line: &[Option<Quat>], laue: Laue, threshold: f64, max_gap: usize, despeckle: bool, step: f64) -> Vec<f64> {
    let idx: Vec<usize> = (0..line.len()).filter(|i| line[*i].is_some()).collect();
    if idx.len() < 3 {
        return Vec::new();
    }
    let g = |k: usize| line[idx[k]].unwrap();
    let m = idx.len() - 1;
    let mut bnd: Vec<bool> = (0..m).map(|k| aurafx_crystal::disorientation_angle(g(k), g(k + 1), laue) >= threshold).collect();
    if despeckle && idx.len() > 3 {
        let spike: Vec<bool> = (0..m - 1).map(|k| bnd[k] && bnd[k + 1] && aurafx_crystal::disorientation_angle(g(k), g(k + 2), laue) < threshold).collect();
        for (k, s) in spike.iter().enumerate() {
            if *s {
                bnd[k] = false;
                bnd[k + 1] = false;
            }
        }
    }
    let gap: Vec<bool> = (0..m).map(|k| idx[k + 1] - idx[k] > max_gap + 1).collect();
    let pos = |k: usize| 0.5 * (idx[k] + idx[k + 1]) as f64;
    let cut: Vec<usize> = (0..m).filter(|k| bnd[*k] || gap[*k]).collect();
    let mut out = Vec::new();
    for w in cut.windows(2) {
        let (a0, a1) = (w[0], w[1]);
        if bnd[a0] && bnd[a1] && !gap[a0..=a1].iter().any(|x| *x) {
            out.push((pos(a1) - pos(a0)) * step);
        }
    }
    out
}

pub const CHORDS: &str = "Chords";
pub const HISTOGRAM: &str = "Histogram";

/// Chord Lengths: grain-boundary chords along lines in x, y (and z for volumes) every
/// `spacing` length units, as a list or a histogram, with the mean per direction.
#[allow(clippy::too_many_arguments)]
pub fn chord_lengths(
    ds: &DataSet,
    sel: &Selection,
    threshold_deg: f64,
    spacing: f64,
    max_gap: usize,
    despeckle: bool,
    output: &str,
    bin: f64,
) -> Result<DataSet> {
    let img = crate::features::image(ds)?;
    let v = crate::features::Vox::of(img);
    let (q, laue) = selected(ds, sel)?;
    if q.len() != v.len() {
        bail!("the orientations must be cell data");
    }
    let th = threshold_deg.to_radians();
    let n = v.n;
    let every = |a: usize| -> usize {
        if spacing > 0.0 {
            ((spacing / v.sp[a]).round() as usize).max(1)
        } else {
            1
        }
    };
    let at = |i: usize, j: usize, k: usize| q[i + n[0] * (j + n[1] * k)];
    let mut chords: Vec<Vec<f64>> = vec![Vec::new(); 3];
    let dirs: Vec<usize> = (0..3).filter(|a| n[*a] > 1).collect();
    for &a in &dirs {
        let (b, c) = ((a + 1) % 3, (a + 2) % 3);
        // Lines along a, stepping across the other axes (the slowest-varying last).
        let (lo_ax, hi_ax) = if b < c { (b, c) } else { (c, b) };
        let mut k2 = 0;
        while k2 < n[hi_ax] {
            let mut k1 = 0;
            while k1 < n[lo_ax] {
                let line: Vec<Option<Quat>> = (0..n[a])
                    .map(|t| {
                        let mut p = [0; 3];
                        p[a] = t;
                        p[lo_ax] = k1;
                        p[hi_ax] = k2;
                        at(p[0], p[1], p[2])
                    })
                    .collect();
                chords[a].extend(line_chords(&line, laue, th, max_gap, despeckle, v.sp[a]));
                k1 += every(lo_ax);
            }
            k2 += every(hi_ax);
        }
    }
    let names = ["X", "Y", "Z"];
    let mut field = Vec::new();
    for &a in &dirs {
        let c = &chords[a];
        let mean = if c.is_empty() { 0.0 } else { c.iter().sum::<f64>() / c.len() as f64 };
        field.push((["MeanChordX", "MeanChordY", "MeanChordZ"][a], mean));
        field.push((["ChordsX", "ChordsY", "ChordsZ"][a], c.len() as f64));
    }
    if output == HISTOGRAM {
        let top = chords.iter().flatten().cloned().fold(0.0, f64::max);
        let w = if bin > 0.0 { bin } else { (top / 30.0).max(1e-12) };
        let nb = ((top / w).floor() as usize + 1).max(1);
        let mut cols = vec![
            col("bin_start", (0..nb).map(|k| k as f64 * w).collect()),
            col("bin_end", (0..nb).map(|k| (k + 1) as f64 * w).collect()),
            col("bin_center", (0..nb).map(|k| (k as f64 + 0.5) * w).collect()),
        ];
        for &a in &dirs {
            let mut h = vec![0.0; nb];
            for c in &chords[a] {
                h[((c / w) as usize).min(nb - 1)] += 1.0;
            }
            cols.push(col(names[a], h));
        }
        return Ok(table(cols, field));
    }
    let mut dn = Vec::new();
    let mut len = Vec::new();
    for &a in &dirs {
        for c in &chords[a] {
            dn.push(names[a].to_string());
            len.push(*c);
        }
    }
    Ok(table(vec![Array::new("Direction", 1, ArrayData::String(dn)), col("Length", len)], field))
}
