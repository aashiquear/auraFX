//! EBSD maps: TSL/EDAX `.ang` and Oxford `.ctf` text files.
//!
//! A map becomes image data with one cell per measured point (cells centred on the points,
//! square grid). Cell data: `EulerAngles` (Bunge, radians, 3 components), `Phases` (0 = not
//! indexed) and the file's other columns. Field data holds one entry per phase, index 0 being
//! "not indexed": `CrystalStructures` (Laue class numbers, 999 unknown), `MaterialName` and
//! `LatticeConstants` (a, b, c, α, β, γ).

use crate::{option, ReaderOption, ReaderOptionKind};
use anyhow::{anyhow, bail, Context, Result};
use aurafx_crystal::Laue;
use aurafx_data::*;
use std::path::Path;

pub const HEX_ALIGN: &str = "a1 along x (+30 degrees on phi2)";
pub const HEX_AS_STORED: &str = "As stored";

pub fn options(path: &Path) -> Vec<ReaderOption> {
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase();
    if ext != "ctf" {
        return Vec::new();
    }
    vec![ReaderOption::new(
        "hexagonal_axes",
        "Hexagonal axes",
        ReaderOptionKind::Choice(vec![HEX_ALIGN.into(), HEX_AS_STORED.into()]),
        HEX_ALIGN,
        "Oxford files put the hexagonal [10-10] direction along x; adding 30 degrees to phi2 puts [2-1-10] along x, the convention of .ang files and of the IPF colors.",
    )]
}

pub(crate) struct Phase {
    pub name: String,
    pub laue: Option<Laue>,
    pub lattice: [f64; 6],
}

/// The image of a map: `nx` x `ny` cells of `step`, origin at the first point's cell corner.
fn image(nx: usize, ny: usize, step: [f64; 2], first: [f64; 2], cells: Attributes, phases: &[Phase]) -> DataSet {
    grid([nx, ny, 1], [step[0], step[1], 1.0], [first[0], first[1], 0.0], cells, phases)
}

/// Image data of `dims` cells of `step` whose first cell is centred on `first`, with the
/// phases as field data.
pub(crate) fn grid(dims: [usize; 3], step: [f64; 3], first: [f64; 3], cells: Attributes, phases: &[Phase]) -> DataSet {
    let mut img = ImageData {
        // A single map stays a flat (2-D) image.
        extent: [0, dims[0] as i64, 0, dims[1] as i64, 0, if dims[2] <= 1 { 0 } else { dims[2] as i64 }],
        origin: std::array::from_fn(|k| first[k] - 0.5 * step[k]),
        spacing: step,
        cell_data: cells,
        ..Default::default()
    };
    let mut codes = vec![Laue::UNKNOWN_CODE];
    let mut names = vec!["Not indexed".to_string()];
    let mut lat = vec![0.0f32; 6];
    for p in phases {
        codes.push(p.laue.map(|l| l.code()).unwrap_or(Laue::UNKNOWN_CODE));
        names.push(p.name.clone());
        lat.extend(p.lattice.iter().map(|v| *v as f32));
    }
    img.field_data.insert(Array::new("CrystalStructures", 1, ArrayData::U32(codes)));
    img.field_data.insert(Array::new("MaterialName", 1, ArrayData::String(names)));
    img.field_data.insert(Array::new("LatticeConstants", 6, ArrayData::F32(lat)));
    DataSet::ImageData(img)
}

/// A TSL/EDAX `.ang` file (square grid).
pub fn read_ang(path: &Path) -> Result<DataSet> {
    let text = std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    let mut phases: Vec<Phase> = Vec::new();
    let (mut xstep, mut ystep, mut ncols, mut nrows, mut ncols_even) = (0.0f64, 0.0f64, 0usize, 0usize, 0usize);
    let mut grid = String::from("SqrGrid");
    let mut data_lines = Vec::new();
    for line in text.lines() {
        let l = line.trim();
        if let Some(h) = l.strip_prefix('#') {
            let h = h.trim();
            let (key, val) = match h.find(|c: char| c == ':' || c.is_whitespace()) {
                Some(k) => (&h[..k], h[k + 1..].trim()),
                None => (h, ""),
            };
            match key.to_ascii_lowercase().as_str() {
                "phase" => phases.push(Phase { name: String::new(), laue: None, lattice: [0.0; 6] }),
                "materialname" => {
                    if let Some(p) = phases.last_mut() {
                        p.name = val.to_string();
                    }
                }
                "symmetry" => {
                    if let Some(p) = phases.last_mut() {
                        p.laue = val.split_whitespace().next().and_then(|v| v.parse::<i64>().ok()).and_then(Laue::from_tsl_symmetry);
                    }
                }
                "latticeconstants" => {
                    if let Some(p) = phases.last_mut() {
                        let v: Vec<f64> = val.split_whitespace().filter_map(|x| x.parse().ok()).collect();
                        for (k, x) in v.iter().take(6).enumerate() {
                            p.lattice[k] = *x;
                        }
                    }
                }
                "grid" => grid = val.to_string(),
                "xstep" => xstep = val.parse().unwrap_or(0.0),
                "ystep" => ystep = val.parse().unwrap_or(0.0),
                "ncols_odd" => ncols = val.parse().unwrap_or(0),
                "ncols_even" => ncols_even = val.parse().unwrap_or(0),
                "nrows" => nrows = val.parse().unwrap_or(0),
                _ => {}
            }
        } else if !l.is_empty() {
            data_lines.push(l);
        }
    }
    let hex = grid.eq_ignore_ascii_case("HexGrid");
    if !hex && !grid.eq_ignore_ascii_case("SqrGrid") {
        bail!("{}: {grid} maps are not read (square and hexagonal grids are)", path.display());
    }
    if ncols == 0 || nrows == 0 {
        bail!("{}: the header gives no NCOLS_ODD / NROWS", path.display());
    }
    // A hexagonal grid alternates rows of NCOLS_ODD and NCOLS_EVEN points, the even ones shifted by half a step.
    let row_len = |r: usize| if hex && r % 2 == 1 { ncols_even.max(1) } else { ncols };
    let n: usize = (0..nrows).map(row_len).sum();
    if data_lines.len() < n {
        bail!("{}: {} points for a {ncols} x {nrows} grid", path.display(), data_lines.len());
    }
    let ncol = data_lines[0].split_whitespace().count();
    if ncol < 8 {
        bail!("{}: {ncol} columns per point (at least 8 expected)", path.display());
    }
    let mut cols = vec![Vec::with_capacity(n); ncol];
    for (k, l) in data_lines.iter().take(n).enumerate() {
        let mut m = 0;
        for (c, v) in l.split_whitespace().take(ncol).enumerate() {
            cols[c].push(v.parse::<f64>().map_err(|_| anyhow!("{}: point {} is not numeric", path.display(), k + 1))?);
            m += 1;
        }
        for c in m..ncol {
            cols[c].push(0.0);
        }
    }
    if phases.is_empty() {
        phases.push(Phase { name: "Unknown".into(), laue: None, lattice: [0.0; 6] });
    }
    // Single-phase scans write phase 0 for every point.
    let single = phases.len() == 1;
    let mut cells = Attributes::new();
    let euler: Vec<f32> = (0..n).flat_map(|i| [cols[0][i] as f32, cols[1][i] as f32, cols[2][i] as f32]).collect();
    cells.insert(Array::new("EulerAngles", 3, ArrayData::F32(euler)));
    let ph: Vec<i32> = cols[7].iter().zip(&cols[6]).map(|(p, ci)| if *p == 0.0 && single && *ci >= 0.0 { 1 } else { *p as i32 }).collect();
    cells.insert(Array::new("Phases", 1, ArrayData::I32(ph)));
    let names = ["Image Quality", "Confidence Index", "SEM Signal", "Fit"];
    for (k, name) in names.iter().enumerate() {
        let c = [5, 6, 8, 9][k];
        if c < ncol {
            cells.insert(Array::new(*name, 1, ArrayData::F32(cols[c].iter().map(|v| *v as f32).collect())));
        }
    }
    for c in 10..ncol {
        cells.insert(Array::new(format!("Column {}", c + 1), 1, ArrayData::F32(cols[c].iter().map(|v| *v as f32).collect())));
    }
    let step = [if xstep > 0.0 { xstep } else { 1.0 }, if ystep > 0.0 { ystep } else { 1.0 }];
    if hex {
        return Ok(hex_to_square(cells, ncols, nrows, &row_len, step, [cols[3][0], cols[4][0]], &phases));
    }
    Ok(image(ncols, nrows, step, [cols[3][0], cols[4][0]], cells, &phases))
}

/// A hexagonal-grid map resampled on a square grid of the hexagonal grid's x step: each square cell
/// takes every value of the nearest scan point. The field array `ResampledFromHexGrid` records it.
fn hex_to_square(
    cells: Attributes,
    ncols: usize,
    nrows: usize,
    row_len: &dyn Fn(usize) -> usize,
    step: [f64; 2],
    first: [f64; 2],
    phases: &[Phase],
) -> DataSet {
    let starts: Vec<usize> = (0..nrows)
        .scan(0usize, |acc, r| {
            let s = *acc;
            *acc += row_len(r);
            Some(s)
        })
        .collect();
    let s = step[0];
    let height = (nrows.saturating_sub(1)) as f64 * step[1];
    let (nx, ny) = (ncols, (height / s).round() as usize + 1);
    // The scan point nearest to a position: the closest of the nearest rows.
    let nearest = |x: f64, y: f64| -> usize {
        let r0 = ((y / step[1]).round() as i64).clamp(0, nrows as i64 - 1);
        let mut best = (f64::INFINITY, 0usize);
        for r in (r0 - 1).max(0)..=(r0 + 1).min(nrows as i64 - 1) {
            let r = r as usize;
            let off = if r % 2 == 1 { 0.5 } else { 0.0 };
            let k = ((x / step[0] - off).round() as i64).clamp(0, row_len(r) as i64 - 1) as usize;
            let (px, py) = ((k as f64 + off) * step[0], r as f64 * step[1]);
            let d = (px - x).powi(2) + (py - y).powi(2);
            if d < best.0 {
                best = (d, starts[r] + k);
            }
        }
        best.1
    };
    let map: Vec<usize> = (0..ny).flat_map(|j| (0..nx).map(move |i| (i, j))).map(|(i, j)| nearest(i as f64 * s, j as f64 * s)).collect();
    let mut out = Attributes::new();
    for a in cells.iter() {
        let mut b = Array::new(a.name.clone(), a.ncomp, ArrayData::F64(Vec::with_capacity(map.len() * a.ncomp)));
        let data: Vec<f64> = map.iter().flat_map(|&p| (0..a.ncomp).map(move |c| a.get(p, c))).collect();
        b.data = match &a.data {
            ArrayData::I32(_) => ArrayData::I32(data.iter().map(|v| *v as i32).collect()),
            ArrayData::F32(_) => ArrayData::F32(data.iter().map(|v| *v as f32).collect()),
            _ => ArrayData::F64(data),
        };
        out.insert(b);
    }
    let mut ds = image(nx, ny, [s, s], first, out, phases);
    if let DataSet::ImageData(img) = &mut ds {
        img.field_data.insert(Array::new(
            "ResampledFromHexGrid",
            1,
            ArrayData::String(vec![format!(
                "hexagonal grid {ncols} x {nrows} (x step {}, y step {}) on a square grid of step {s}, nearest point",
                step[0], step[1]
            )]),
        ));
    }
    ds
}

/// An Oxford `.ctf` file (angles in degrees; phase 0 = not indexed).
pub fn read_ctf(path: &Path) -> Result<DataSet> {
    let text = std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    let mut lines = text.lines();
    let (mut nx, mut ny, mut xs, mut ys) = (0usize, 0usize, 0.0f64, 0.0f64);
    let mut phases = Vec::new();
    let mut header: Vec<String> = Vec::new();
    while let Some(line) = lines.next() {
        let f: Vec<&str> = line.split('\t').collect();
        match f[0].trim() {
            "XCells" => nx = f.get(1).and_then(|v| v.trim().parse().ok()).unwrap_or(0),
            "YCells" => ny = f.get(1).and_then(|v| v.trim().parse().ok()).unwrap_or(0),
            "XStep" => xs = f.get(1).and_then(|v| v.trim().parse().ok()).unwrap_or(0.0),
            "YStep" => ys = f.get(1).and_then(|v| v.trim().parse().ok()).unwrap_or(0.0),
            "Phases" => {
                let k: usize = f.get(1).and_then(|v| v.trim().parse().ok()).unwrap_or(0);
                for _ in 0..k {
                    let p = lines.next().ok_or_else(|| anyhow!("{}: phase lines missing", path.display()))?;
                    let g: Vec<&str> = p.split('\t').collect();
                    let mut lattice = [0.0; 6];
                    for (k, v) in g.first().unwrap_or(&"").split(';').chain(g.get(1).unwrap_or(&"").split(';')).take(6).enumerate() {
                        lattice[k] = v.trim().parse().unwrap_or(0.0);
                    }
                    let laue = g.get(3).and_then(|v| v.trim().parse::<i64>().ok()).and_then(Laue::from_oxford);
                    phases.push(Phase { name: g.get(2).unwrap_or(&"").trim().to_string(), laue, lattice });
                }
            }
            "Phase" if f.len() > 3 => {
                header = f.iter().map(|s| s.trim().to_string()).collect();
                break;
            }
            _ => {}
        }
    }
    if header.is_empty() {
        bail!("{}: no column header (a line starting with Phase, X, Y ...)", path.display());
    }
    if nx == 0 || ny == 0 {
        bail!("{}: XCells / YCells missing", path.display());
    }
    let n = nx * ny;
    let mut cols: Vec<Vec<f64>> = vec![Vec::with_capacity(n); header.len()];
    for (k, line) in lines.filter(|l| !l.trim().is_empty()).take(n).enumerate() {
        let f: Vec<&str> = line.split('\t').collect();
        for (c, col) in cols.iter_mut().enumerate() {
            let v = f.get(c).map(|v| v.trim()).unwrap_or("0");
            col.push(v.parse::<f64>().map_err(|_| anyhow!("{}: point {} column {} is not numeric", path.display(), k + 1, c + 1))?);
        }
    }
    if cols[0].len() < n {
        bail!("{}: {} points for a {nx} x {ny} grid", path.display(), cols[0].len());
    }
    let col = |name: &str| header.iter().position(|h| h.eq_ignore_ascii_case(name));
    let (e1, e2, e3) = (col("Euler1"), col("Euler2"), col("Euler3"));
    let (Some(e1), Some(e2), Some(e3)) = (e1, e2, e3) else { bail!("{}: no Euler1/Euler2/Euler3 columns", path.display()) };
    let pc = col("Phase").unwrap_or(0);
    let align = option("hexagonal_axes").is_none_or(|v| v != HEX_AS_STORED);
    let thirty = 30f64.to_radians();
    let mut euler = Vec::with_capacity(3 * n);
    for i in 0..n {
        let p = cols[pc][i] as usize;
        let hex = p > 0 && phases.get(p - 1).and_then(|ph: &Phase| ph.laue).is_some_and(|l| l.is_hexagonal());
        let mut phi2 = cols[e3][i].to_radians();
        if align && hex {
            phi2 = (phi2 + thirty).rem_euclid(std::f64::consts::TAU);
        }
        euler.extend([cols[e1][i].to_radians() as f32, cols[e2][i].to_radians() as f32, phi2 as f32]);
    }
    let mut cells = Attributes::new();
    cells.insert(Array::new("EulerAngles", 3, ArrayData::F32(euler)));
    cells.insert(Array::new("Phases", 1, ArrayData::I32(cols[pc].iter().map(|v| *v as i32).collect())));
    for (c, h) in header.iter().enumerate() {
        if [pc, e1, e2, e3].contains(&c) || h.eq_ignore_ascii_case("X") || h.eq_ignore_ascii_case("Y") {
            continue;
        }
        let ints = matches!(h.as_str(), "Bands" | "Error" | "BC" | "BS");
        let data = if ints { ArrayData::I32(cols[c].iter().map(|v| *v as i32).collect()) } else { ArrayData::F32(cols[c].iter().map(|v| *v as f32).collect()) };
        cells.insert(Array::new(h.clone(), 1, data));
    }
    let (xc, yc) = (col("X").unwrap_or(1), col("Y").unwrap_or(2));
    let step = [if xs > 0.0 { xs } else { 1.0 }, if ys > 0.0 { ys } else { 1.0 }];
    Ok(image(nx, ny, step, [cols[xc][0], cols[yc][0]], cells, &phases))
}

#[cfg(test)]
mod hex_tests {
    use super::*;

    /// A hexagonal .ang scan of `nodd` / `nodd - 1` points per row; phi1 holds each point's index.
    pub(crate) fn hex_ang(nodd: usize, nrows: usize, xstep: f64) -> String {
        let ystep = xstep * 3f64.sqrt() / 2.0;
        let mut t = format!(
            "# TEM_PIXperUM 1.0\n# Phase 1\n# MaterialName Nickel\n# Symmetry 43\n# LatticeConstants 3.52 3.52 3.52 90 90 90\n# GRID: HexGrid\n# XSTEP: {xstep}\n# YSTEP: {ystep}\n# NCOLS_ODD: {nodd}\n# NCOLS_EVEN: {}\n# NROWS: {nrows}\n#\n",
            nodd - 1
        );
        let mut k = 0;
        for r in 0..nrows {
            let (n, off) = if r % 2 == 1 { (nodd - 1, 0.5) } else { (nodd, 0.0) };
            for i in 0..n {
                t += &format!("{} 0.5 0.25 {} {} 100 0.9 1 1 0.5\n", k as f64 * 0.001, (i as f64 + off) * xstep, r as f64 * ystep);
                k += 1;
            }
        }
        t
    }

    #[test]
    fn a_hexagonal_scan_is_resampled_on_a_square_grid_by_the_nearest_point() {
        let (nodd, nrows, xs) = (9, 12, 0.5);
        let p = std::env::temp_dir().join(format!("afx_hex_{}.ang", std::process::id()));
        std::fs::write(&p, hex_ang(nodd, nrows, xs)).unwrap();
        let ds = read_ang(&p).unwrap();
        std::fs::remove_file(&p).ok();
        let DataSet::ImageData(img) = &ds else { panic!() };
        let ys = xs * 3f64.sqrt() / 2.0;
        let ny = ((nrows - 1) as f64 * ys / xs).round() as usize + 1;
        assert_eq!([img.extent[1] - img.extent[0], img.extent[3] - img.extent[2]], [nodd as i64, ny as i64]);
        assert_eq!(img.spacing[0], xs);
        assert_eq!(img.spacing[1], xs, "square cells");
        assert!(img.field_data.get("ResampledFromHexGrid").is_some());
        // Every square cell holds the value of the scan point nearest to it (brute force).
        let pts: Vec<(f64, f64)> = (0..nrows)
            .flat_map(|r| {
                let (n, off) = if r % 2 == 1 { (nodd - 1, 0.5) } else { (nodd, 0.0) };
                (0..n).map(move |i| ((i as f64 + off) * xs, r as f64 * ys))
            })
            .collect();
        let e = img.cell_data.get("EulerAngles").unwrap();
        for j in 0..ny {
            for i in 0..nodd {
                let (x, y) = (i as f64 * xs, j as f64 * xs);
                let d = |p: &(f64, f64)| (p.0 - x).powi(2) + (p.1 - y).powi(2);
                let best = pts.iter().map(d).fold(f64::INFINITY, f64::min);
                let got = (e.get(j * nodd + i, 0) / 0.001).round() as usize;
                assert!((d(&pts[got]) - best).abs() < 1e-9, "cell ({i}, {j}) took point {got}");
            }
        }
        // Single-phase points keep phase 1 and the other columns come along.
        assert!(img.cell_data.get("Confidence Index").is_some());
    }
}

#[cfg(test)]
mod hex_testdata {
    /// testdata/ebsd/hex_grains.ang: a 60 x 52 hexagonal-grid scan of seven nickel grains (each point
    /// takes the orientation of the nearest of seven fixed seeds; CI falls near the boundaries).
    /// `cargo test -p aurafx-io write_hex_testdata -- --ignored`.
    #[test]
    #[ignore]
    fn write_hex_testdata() {
        let (nodd, nrows, xs) = (60usize, 52usize, 0.5);
        let ys = xs * 3f64.sqrt() / 2.0;
        let seeds = [
            (3.0, 4.0, [0.2, 0.4, 0.1]),
            (12.0, 3.0, [1.1, 0.7, 0.3]),
            (25.0, 6.0, [2.0, 0.3, 1.2]),
            (6.0, 15.0, [3.0, 1.0, 0.5]),
            (17.0, 13.0, [4.2, 0.5, 0.9]),
            (27.0, 17.0, [5.1, 0.9, 0.2]),
            (14.0, 20.0, [0.7, 1.2, 1.4]),
        ];
        let mut t = format!(
            "# TEM_PIXperUM 1.0\n# x-star 0.5\n# y-star 0.5\n# z-star 0.7\n# WorkingDistance 15.0\n#\n# Phase 1\n# MaterialName Nickel\n# Formula Ni\n# Symmetry 43\n# LatticeConstants 3.520 3.520 3.520 90.000 90.000 90.000\n#\n# GRID: HexGrid\n# XSTEP: {xs}\n# YSTEP: {ys}\n# NCOLS_ODD: {nodd}\n# NCOLS_EVEN: {}\n# NROWS: {nrows}\n#\n# OPERATOR: auraFX test data\n# SAMPLEID: hex_grains\n# SCANID: 1\n#\n",
            nodd - 1
        );
        for r in 0..nrows {
            let (n, off) = if r % 2 == 1 { (nodd - 1, 0.5) } else { (nodd, 0.0) };
            for i in 0..n {
                let (x, y) = ((i as f64 + off) * xs, r as f64 * ys);
                let mut d: Vec<(f64, usize)> = seeds.iter().enumerate().map(|(k, s)| ((s.0 - x).powi(2) + (s.1 - y).powi(2), k)).collect();
                d.sort_by(|a, b| a.0.total_cmp(&b.0));
                let e = seeds[d[0].1].2;
                let ci = ((d[1].0.sqrt() - d[0].0.sqrt()) / 2.0).clamp(0.05, 1.0);
                t += &format!("{:.5} {:.5} {:.5} {x:.5} {y:.5} {:.1} {ci:.3} 1 0 0.500\n", e[0], e[1], e[2], 100.0 + 50.0 * ci);
            }
        }
        let p = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../testdata/ebsd/hex_grains.ang");
        std::fs::write(p, t).unwrap();
    }
}
