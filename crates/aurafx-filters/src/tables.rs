//! Tables and per-point bookkeeping arrays:
//!
//! - **Table To Points**: rows as points (three columns give x, y, z), every column as point data.
//! - **Table To Structured Grid**: rows as the points of a structured grid of given dimensions.
//! - **Transpose Table**: rows become columns; a `Column` column names the old columns.
//! - **Table FFT**: the discrete Fourier transform of every numeric column (complex, with the
//!   magnitude), and the frequency of each row when a time step is given.
//! - **Merge Vector Components**: three scalar arrays into one vector array.
//! - **Point And Cell Ids**, **Generate Global Ids** (unique over every block of a multiblock) and
//!   **Coordinates** (the point positions as an array).

use crate::params::Association;
use anyhow::{anyhow, bail, Result};
use aurafx_data::*;

fn numeric(a: &Array) -> bool {
    !matches!(a.data, ArrayData::String(_))
}

/// The column of a table by name, or (when `want` is empty) the `k`-th numeric column.
fn column<'a>(t: &'a Table, want: &str, k: usize) -> Result<&'a Array> {
    if !want.is_empty() {
        return t.columns.get(want).ok_or_else(|| anyhow!("the table has no column {want:?}"));
    }
    // Columns named x, y, z (any case) first, else the first numeric ones.
    let names = [["x", "X", "Points_0", "coordsX"], ["y", "Y", "Points_1", "coordsY"], ["z", "Z", "Points_2", "coordsZ"]];
    if let Some(a) = names.get(k).and_then(|ns| ns.iter().find_map(|n| t.columns.get(n))) {
        return Ok(a);
    }
    t.columns.iter().filter(|a| numeric(a) && a.ncomp == 1).nth(k).ok_or_else(|| anyhow!("the table has fewer than {} numeric columns", k + 1))
}

fn table_of(ds: &DataSet) -> Result<&Table> {
    match ds {
        DataSet::Table(t) => Ok(t),
        _ => bail!("this filter works on a table (CSV, spreadsheet data)"),
    }
}

/// Rows as points: x, y, z from three columns (a missing z is 0); every column as point data.
pub fn table_to_points(ds: &DataSet, xyz: [&str; 3], twod: bool) -> Result<PolyData> {
    let t = table_of(ds)?;
    let n = t.n_rows();
    let x = column(t, xyz[0], 0)?;
    let y = column(t, xyz[1], 1)?;
    let z = if twod { None } else { Some(column(t, xyz[2], 2)?) };
    let pts: Vec<f64> = (0..n).flat_map(|i| [x.get(i, 0), y.get(i, 0), z.map(|z| z.get(i, 0)).unwrap_or(0.0)]).collect();
    let mut verts = Cells::new();
    for i in 0..n {
        verts.push(CellType::Vertex, &[i as u32]);
    }
    let mut pd = PolyData { points: Points::new_f64(pts), verts, ..Default::default() };
    for a in t.columns.iter() {
        pd.point_data.insert(a.clone());
    }
    pd.field_data = t.field_data.clone();
    Ok(pd)
}

/// Rows as the points of an nx × ny × nz structured grid (x fastest).
pub fn table_to_structured_grid(ds: &DataSet, xyz: [&str; 3], dims: [usize; 3]) -> Result<StructuredGrid> {
    let t = table_of(ds)?;
    let n = t.n_rows();
    let want: usize = dims.iter().product();
    if want != n {
        bail!("{} x {} x {} = {want} points, but the table has {n} rows", dims[0], dims[1], dims[2]);
    }
    let pd = table_to_points(ds, xyz, false)?;
    Ok(StructuredGrid {
        extent: [0, dims[0] as i64 - 1, 0, dims[1] as i64 - 1, 0, dims[2] as i64 - 1],
        points: pd.points,
        point_data: pd.point_data,
        cell_data: Attributes::new(),
        field_data: Attributes::new(),
    })
}

/// Rows become columns (numeric columns only); the column `Column` names the old columns.
pub fn transpose(ds: &DataSet) -> Result<Table> {
    let t = table_of(ds)?;
    let cols: Vec<&Array> = t.columns.iter().filter(|a| numeric(a)).collect();
    let rows = t.n_rows();
    let mut out = Table::default();
    let mut names = Vec::new();
    for a in &cols {
        for c in 0..a.ncomp {
            names.push(if a.ncomp == 1 { a.name.clone() } else { format!("{}_{c}", a.name) });
        }
    }
    out.columns.insert(Array::new("Column", 1, ArrayData::String(names)));
    for r in 0..rows {
        let v: Vec<f64> = cols.iter().flat_map(|a| (0..a.ncomp).map(move |c| a.get(r, c))).collect();
        out.columns.insert(Array::scalars_f64(format!("row {r}"), v));
    }
    out.field_data = t.field_data.clone();
    Ok(out)
}

// ------------------------------------------------------------------ FFT

/// In-place radix-2 FFT of a power-of-two length (`inverse` uses e^{+i...}, unscaled).
fn fft_pow2(re: &mut [f64], im: &mut [f64], inverse: bool) {
    let n = re.len();
    let mut j = 0;
    for i in 1..n {
        let mut bit = n >> 1;
        while j & bit != 0 {
            j ^= bit;
            bit >>= 1;
        }
        j |= bit;
        if i < j {
            re.swap(i, j);
            im.swap(i, j);
        }
    }
    let mut len = 2;
    while len <= n {
        let ang = 2.0 * std::f64::consts::PI / len as f64 * if inverse { 1.0 } else { -1.0 };
        let (wr, wi) = (ang.cos(), ang.sin());
        for start in (0..n).step_by(len) {
            let (mut cr, mut ci) = (1.0, 0.0);
            for k in 0..len / 2 {
                let (a, b) = (start + k, start + k + len / 2);
                let (tr, ti) = (re[b] * cr - im[b] * ci, re[b] * ci + im[b] * cr);
                re[b] = re[a] - tr;
                im[b] = im[a] - ti;
                re[a] += tr;
                im[a] += ti;
                let nr = cr * wr - ci * wi;
                ci = cr * wi + ci * wr;
                cr = nr;
            }
        }
        len <<= 1;
    }
}

/// The DFT X_k = Σ x_n e^{-2πikn/N} of any length (Bluestein's method for lengths that are not
/// powers of two).
pub fn dft(re: &[f64], im: &[f64]) -> (Vec<f64>, Vec<f64>) {
    let n = re.len();
    if n == 0 {
        return (vec![], vec![]);
    }
    if n.is_power_of_two() {
        let (mut r, mut i) = (re.to_vec(), im.to_vec());
        fft_pow2(&mut r, &mut i, false);
        return (r, i);
    }
    let m = (2 * n - 1).next_power_of_two();
    // Chirp w_k = e^{-iπk²/N}.
    let chirp: Vec<(f64, f64)> = (0..n)
        .map(|k| {
            let a = std::f64::consts::PI * ((k * k) % (2 * n)) as f64 / n as f64;
            (a.cos(), -a.sin())
        })
        .collect();
    let (mut ar, mut ai) = (vec![0.0; m], vec![0.0; m]);
    for k in 0..n {
        let (c, s) = chirp[k];
        ar[k] = re[k] * c - im[k] * s;
        ai[k] = re[k] * s + im[k] * c;
    }
    let (mut br, mut bi) = (vec![0.0; m], vec![0.0; m]);
    for k in 0..n {
        let (c, s) = chirp[k];
        br[k] = c;
        bi[k] = -s;
        if k > 0 {
            br[m - k] = c;
            bi[m - k] = -s;
        }
    }
    fft_pow2(&mut ar, &mut ai, false);
    fft_pow2(&mut br, &mut bi, false);
    for k in 0..m {
        let (r, i) = (ar[k] * br[k] - ai[k] * bi[k], ar[k] * bi[k] + ai[k] * br[k]);
        ar[k] = r;
        ai[k] = i;
    }
    fft_pow2(&mut ar, &mut ai, true);
    let (mut or, mut oi) = (vec![0.0; n], vec![0.0; n]);
    for k in 0..n {
        let (r, i) = (ar[k] / m as f64, ai[k] / m as f64);
        let (c, s) = chirp[k];
        or[k] = r * c - i * s;
        oi[k] = r * s + i * c;
    }
    (or, oi)
}

/// The DFT of every numeric 1-component column: `<name>_fft` (real, imaginary) and
/// `<name>_magnitude`; with a time step `dt` > 0, a `Frequency` column (k / (N dt), negative
/// frequencies in the upper half).
pub fn table_fft(ds: &DataSet, dt: f64) -> Result<Table> {
    let t = table_of(ds)?;
    let n = t.n_rows();
    let mut out = Table::default();
    if dt > 0.0 {
        out.columns.insert(Array::scalars_f64(
            "Frequency",
            (0..n)
                .map(|k| {
                    let kk = if k <= n / 2 { k as f64 } else { k as f64 - n as f64 };
                    kk / (n as f64 * dt)
                })
                .collect(),
        ));
    }
    for a in t.columns.iter().filter(|a| numeric(a) && a.ncomp == 1) {
        let re: Vec<f64> = (0..n).map(|i| a.get(i, 0)).collect();
        let (r, i) = dft(&re, &vec![0.0; n]);
        out.columns.insert(Array::new(format!("{}_fft", a.name), 2, ArrayData::F64(r.iter().zip(&i).flat_map(|(x, y)| [*x, *y]).collect())));
        out.columns.insert(Array::scalars_f64(format!("{}_magnitude", a.name), r.iter().zip(&i).map(|(x, y)| (x * x + y * y).sqrt()).collect()));
    }
    if out.columns.iter().next().is_none() {
        bail!("Table FFT: the table has no numeric column");
    }
    Ok(out)
}

// ------------------------------------------------------------------ arrays

fn attrs_mut(ds: &mut DataSet, assoc: Association) -> Result<&mut Attributes> {
    Ok(match (ds, assoc) {
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
        (DataSet::Table(t), _) => &mut t.columns,
        _ => bail!("unsupported dataset"),
    })
}

/// Three scalar arrays as one vector array (all on points, or all on cells, or table columns).
pub fn merge_vector_components(ds: &DataSet, assoc: Association, names: [&str; 3], out: &str) -> Result<DataSet> {
    let mut d = ds.clone();
    let a = attrs_mut(&mut d, assoc)?;
    let comps: Vec<Array> = names.iter().map(|n| a.get(n).cloned().ok_or_else(|| anyhow!("no array {n:?}"))).collect::<Result<_>>()?;
    let len = comps[0].len();
    if comps.iter().any(|c| c.len() != len || c.ncomp != 1) {
        bail!("Merge Vector Components takes three scalar arrays of the same length");
    }
    let v: Vec<f64> = (0..len).flat_map(|i| comps.iter().map(move |c| c.get(i, 0)).collect::<Vec<_>>()).collect();
    let mut arr = Array::new(out, 3, ArrayData::F64(v));
    arr.component_names = Some(vec![names[0].into(), names[1].into(), names[2].into()]);
    a.insert(arr);
    Ok(d)
}

/// `PointIds` and `CellIds` arrays (the index of each point and cell).
pub fn point_and_cell_ids(ds: &DataSet) -> Result<DataSet> {
    let mut d = crate::simplex::as_explicit(ds).into_owned();
    let (np, nc) = (d.n_points(), d.n_cells());
    attrs_mut(&mut d, Association::Points)?.insert(Array::new("PointIds", 1, ArrayData::I64((0..np as i64).collect())));
    attrs_mut(&mut d, Association::Cells)?.insert(Array::new("CellIds", 1, ArrayData::I64((0..nc as i64).collect())));
    Ok(d)
}

/// `GlobalPointIds` and `GlobalCellIds`: unique over all the blocks of a multiblock, points that
/// coincide (to `tol` of the whole size) sharing one id.
pub fn global_ids(ds: &DataSet) -> Result<DataSet> {
    let size = ds.bounds().diagonal().max(1e-300);
    let tol = 1e-9 * size;
    let mut seen: std::collections::HashMap<[i64; 3], i64> = std::collections::HashMap::new();
    let mut next_cell = 0i64;
    let mut visit = |d: &DataSet| -> Result<DataSet> {
        let mut out = crate::simplex::as_explicit(d).into_owned();
        let np = out.n_points();
        let pts: Vec<[f64; 3]> = match crate::simplex::parts(&out) {
            Some((p, ..)) => (0..np).map(|i| p.get(i)).collect(),
            None => return Ok(out),
        };
        let ids: Vec<i64> = pts
            .iter()
            .map(|p| {
                let k = p.map(|x| (x / tol).round() as i64);
                let next = seen.len() as i64;
                *seen.entry(k).or_insert(next)
            })
            .collect();
        let nc = out.n_cells();
        attrs_mut(&mut out, Association::Points)?.insert(Array::new("GlobalPointIds", 1, ArrayData::I64(ids)));
        attrs_mut(&mut out, Association::Cells)?.insert(Array::new("GlobalCellIds", 1, ArrayData::I64((next_cell..next_cell + nc as i64).collect())));
        next_cell += nc as i64;
        Ok(out)
    };
    match ds {
        DataSet::MultiBlock(mb) => {
            let mut out = mb.clone();
            for (_, b) in out.blocks.iter_mut() {
                if let Some(d) = b.as_mut() {
                    *d = match &*d {
                        DataSet::MultiBlock(_) => return Err(anyhow!("Generate Global Ids: nested multiblocks are not handled; Merge Blocks first")),
                        leaf => visit(leaf)?,
                    };
                }
            }
            Ok(DataSet::MultiBlock(out))
        }
        d => visit(d),
    }
}

/// The point positions as a 3-component point array `Coordinates`.
pub fn coordinates(ds: &DataSet) -> Result<DataSet> {
    let mut d = crate::simplex::as_explicit(ds).into_owned();
    let np = d.n_points();
    let v: Vec<f64> = match crate::simplex::parts(&d) {
        Some((p, ..)) => (0..np).flat_map(|i| p.get(i)).collect(),
        None => bail!("Coordinates: the data has no points"),
    };
    attrs_mut(&mut d, Association::Points)?.insert(Array::new("Coordinates", 3, ArrayData::F64(v)));
    Ok(d)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn table() -> DataSet {
        let mut t = Table::default();
        t.columns.insert(Array::scalars_f64("x", vec![0.0, 1.0, 2.0, 3.0]));
        t.columns.insert(Array::scalars_f64("y", vec![0.0, 0.0, 1.0, 1.0]));
        t.columns.insert(Array::scalars_f64("z", vec![5.0, 5.0, 5.0, 5.0]));
        t.columns.insert(Array::scalars_f64("T", vec![10.0, 20.0, 30.0, 40.0]));
        DataSet::Table(t)
    }

    #[test]
    fn rows_become_points_and_grids_with_their_columns() {
        let p = table_to_points(&table(), ["", "", ""], false).unwrap();
        assert_eq!(p.points.len(), 4);
        assert_eq!(p.points.get(2), [2.0, 1.0, 5.0]);
        assert_eq!(p.point_data.get("T").unwrap().get(3, 0), 40.0);
        assert_eq!(p.verts.len(), 4);
        let flat = table_to_points(&table(), ["x", "T", ""], true).unwrap();
        assert_eq!(flat.points.get(1), [1.0, 20.0, 0.0]);
        let g = table_to_structured_grid(&table(), ["", "", ""], [2, 2, 1]).unwrap();
        assert_eq!(g.dims(), [2, 2, 1]);
        assert!(table_to_structured_grid(&table(), ["", "", ""], [3, 2, 1]).unwrap_err().to_string().contains("4 rows"));
    }

    #[test]
    fn transposing_twice_gives_the_table_back() {
        let t1 = transpose(&table()).unwrap();
        assert_eq!(t1.n_rows(), 4);
        assert_eq!(t1.columns.get("row 2").unwrap().get(3, 0), 30.0);
        let ArrayData::String(names) = &t1.columns.get("Column").unwrap().data else { panic!() };
        assert_eq!(names, &["x", "y", "z", "T"]);
        // Transposing again: four rows (the old columns) of four values.
        let t2 = transpose(&DataSet::Table(t1)).unwrap();
        assert_eq!(t2.columns.get("row 3").unwrap().get(1, 0), 20.0, "row 3 is the old T column, its second value 20");
    }

    #[test]
    fn the_fft_finds_a_sine_at_its_frequency_for_any_length() {
        for n in [64usize, 100, 37] {
            let dt = 0.01;
            let f0 = 5.0 / (n as f64 * dt) * 1.0;
            let mut t = Table::default();
            t.columns.insert(Array::scalars_f64("s", (0..n).map(|i| (2.0 * std::f64::consts::PI * f0 * i as f64 * dt).sin()).collect()));
            let out = table_fft(&DataSet::Table(t), dt).unwrap();
            let mag = out.columns.get("s_magnitude").unwrap();
            let freq = out.columns.get("Frequency").unwrap();
            let peak = (0..n / 2).max_by(|a, b| mag.get(*a, 0).total_cmp(&mag.get(*b, 0))).unwrap();
            assert_eq!(peak, 5, "n = {n}");
            assert!((freq.get(peak, 0) - f0).abs() < 1e-9);
            assert!((mag.get(5, 0) - n as f64 / 2.0).abs() < 1e-6 * n as f64, "{} for n = {n}", mag.get(5, 0));
        }
        // Against a direct DFT on an odd length.
        let x: Vec<f64> = (0..13).map(|i| (i as f64 * 0.7).cos() + 0.1 * i as f64).collect();
        let (r, i) = dft(&x, &vec![0.0; 13]);
        for k in 0..13 {
            let (mut sr, mut si) = (0.0, 0.0);
            for (nn, v) in x.iter().enumerate() {
                let a = -2.0 * std::f64::consts::PI * (k * nn) as f64 / 13.0;
                sr += v * a.cos();
                si += v * a.sin();
            }
            assert!((sr - r[k]).abs() < 1e-9 && (si - i[k]).abs() < 1e-9);
        }
    }

    #[test]
    fn ids_coordinates_and_merged_components() {
        let p = DataSet::PolyData(table_to_points(&table(), ["", "", ""], false).unwrap());
        let ids = point_and_cell_ids(&p).unwrap();
        let DataSet::PolyData(o) = &ids else { panic!() };
        assert_eq!(o.point_data.get("PointIds").unwrap().get(3, 0), 3.0);
        assert_eq!(o.cell_data.get("CellIds").unwrap().len(), 4);
        let c = coordinates(&p).unwrap();
        let DataSet::PolyData(o) = &c else { panic!() };
        assert_eq!(o.point_data.get("Coordinates").unwrap().get(2, 1), 1.0);
        let m = merge_vector_components(&p, Association::Points, ["x", "y", "T"], "v").unwrap();
        let DataSet::PolyData(o) = &m else { panic!() };
        let v = o.point_data.get("v").unwrap();
        assert_eq!((v.ncomp, v.get(3, 2)), (3, 40.0));
        // Global ids over two blocks sharing a point.
        let mut mb = MultiBlock::default();
        let a = table_to_points(&table(), ["", "", ""], false).unwrap();
        let mut b = a.clone();
        b.points = Points::new_f64(vec![3.0, 1.0, 5.0, 9.0, 9.0, 9.0, 8.0, 8.0, 8.0, 7.0, 7.0, 7.0]);
        mb.blocks.push(("a".into(), Some(DataSet::PolyData(a))));
        mb.blocks.push(("b".into(), Some(DataSet::PolyData(b))));
        let g = global_ids(&DataSet::MultiBlock(mb)).unwrap();
        let DataSet::MultiBlock(g) = g else { panic!() };
        let Some(DataSet::PolyData(b)) = &g.blocks[1].1 else { panic!() };
        let gp = b.point_data.get("GlobalPointIds").unwrap();
        assert_eq!(gp.get(0, 0), 3.0, "the shared point keeps block a's id");
        assert_eq!(gp.get(1, 0), 4.0);
        assert_eq!(b.cell_data.get("GlobalCellIds").unwrap().get(0, 0), 4.0);
    }
}
