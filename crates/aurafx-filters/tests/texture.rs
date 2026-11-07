//! Texture analysis against the AM orientation toolkit (am_texture, am_validation_metrics) on the
//! AM Bench 2022 IN718 window (testdata/crystal/texture_reference.json from
//! scripts/texture_reference.py), and against properties every density must have.

use aurafx_crystal::texture as tx;
use aurafx_crystal::{self as cx, Laue, Quat};
use aurafx_data::*;
use std::path::PathBuf;

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../testdata")
}

fn load(name: &str) -> DataSet {
    match aurafx_io::read_file(root().join(name)).unwrap() {
        aurafx_io::Loaded::Static(d) => (*d).clone(),
        _ => panic!(),
    }
}

fn reference() -> serde_json::Value {
    serde_json::from_str(&std::fs::read_to_string(root().join("crystal/texture_reference.json")).unwrap()).unwrap()
}

fn floats(v: &serde_json::Value) -> Vec<f64> {
    v.as_array().unwrap().iter().map(|x| x.as_f64().unwrap()).collect()
}

/// The indexed orientations of the AM Bench window, in cell order.
fn am_bench() -> (DataSet, Vec<Quat>) {
    let d = load("dream3d/am_bench_subset.dream3d");
    let c = d.cell_data().unwrap();
    let (e, ph) = (c.get("EulerAngles").unwrap(), c.get("Phases").unwrap());
    let q = (0..e.len()).filter(|i| ph.get(*i, 0) > 0.0).map(|i| cx::euler_to_quat([e.get(i, 0), e.get(i, 1), e.get(i, 2)])).collect();
    (d, q)
}

/// The toolkit's stereographic grid directions.
fn stereo(n: usize) -> Vec<[f64; 3]> {
    let mut out = Vec::new();
    for j in 0..n {
        for i in 0..n {
            let u = |k: usize| -1.0 + 2.0 * k as f64 / (n - 1) as f64;
            let (x, y) = (u(i), u(j));
            let r2 = x * x + y * y;
            out.push([2.0 * x / (1.0 + r2), 2.0 * y / (1.0 + r2), (1.0 - r2) / (1.0 + r2)]);
        }
    }
    out
}

#[test]
fn texture_matches_toolkit() {
    let r = reference();
    let (_, q) = am_bench();
    assert_eq!(q.len(), r["n"].as_u64().unwrap() as usize);
    let sigma = 7.5f64.to_radians();
    // Pole figures: node for node.
    let grid = stereo(41);
    for (name, hkl) in [("100", [1.0, 0.0, 0.0]), ("110", [1.0, 1.0, 0.0]), ("111", [1.0, 1.0, 1.0])] {
        let ours = tx::pole_figure(&q, None, hkl, Laue::CubicHigh, &grid, sigma, 2.0);
        let theirs = floats(&r["pole_figures"][name]);
        let worst = ours.iter().zip(&theirs).map(|(a, b)| (a - b).abs()).fold(0.0, f64::max);
        assert!(worst < 1e-9, "{{{name}}}: {worst}");
    }
    // ODF kernel means on three sections: the same numbers; the cubic shortcut and the general
    // symmetry search agree.
    for s in ["0", "45", "65"] {
        let (eul, _, _) = tx::section_grid(s.parse::<f64>().unwrap().to_radians(), 10f64.to_radians(), std::f64::consts::FRAC_PI_2);
        let g: Vec<Quat> = eul.iter().map(|e| cx::euler_to_quat(*e)).collect();
        let ours = tx::odf_kernel_means(&g, &q, sigma, Laue::CubicHigh);
        let theirs = floats(&r["odf_kernel_means"][s]);
        let worst = ours.iter().zip(&theirs).map(|(a, b)| (a - b).abs() / b.abs().max(1e-3)).fold(0.0, f64::max);
        assert!(worst < 1e-9, "phi2 {s}: {worst}");
    }
    let g: Vec<Quat> = (0..40).map(|k| cx::euler_to_quat([0.1 * k as f64, 0.05 * k as f64, 0.13 * k as f64])).collect();
    let fast = tx::odf_kernel_means(&g, &q[..2000], sigma, Laue::CubicHigh);
    let ops = Laue::CubicHigh.operators();
    for (k, gk) in g.iter().enumerate() {
        let slow: f64 = q[..2000].iter().map(|s| tx::kernel(cx::disorientation_angle(*gk, *s, Laue::CubicHigh), sigma)).sum::<f64>() / 2000.0;
        assert!((fast[k] - slow).abs() < 1e-12, "{k}: {} {slow} ({} operators)", fast[k], ops.len());
    }
    // The uniform value: ours is exact; the toolkit's is a Monte Carlo estimate.
    let e = tx::uniform_odf_expectation(sigma, Laue::CubicHigh);
    let mc = r["uniform_expectation_mc"].as_f64().unwrap();
    assert!((e - mc).abs() / e < 0.01, "{e} vs {mc}");
    // Texture index: ours integrates with midpoints on 5 degree steps, the toolkit's on 10
    // degree steps with both ends; they agree to a few percent.
    let (j, fmax) = tx::texture_index(&q, sigma, Laue::CubicHigh, 5f64.to_radians());
    let (jt, mt) = (r["texture_index"].as_f64().unwrap(), r["odf_max"].as_f64().unwrap());
    eprintln!("J {j} (toolkit {jt}), max {fmax} (toolkit {mt}), E {e} (toolkit {mc})");
    // The toolkit's own quadrature (φ2 0..80, Φ 0..90 and φ1 0..360 in 10 degree steps, both
    // ends included) with its uniform value gives its numbers exactly ...
    let mut grid = Vec::new();
    let mut w = Vec::new();
    for p2 in 0..9 {
        for p in 0..10 {
            for p1 in 0..37 {
                grid.push(cx::euler_to_quat([(p1 * 10) as f64, (p * 10) as f64, (p2 * 10) as f64].map(f64::to_radians)));
                w.push(((p * 10) as f64).to_radians().sin());
            }
        }
    }
    let f: Vec<f64> = tx::odf_kernel_means(&grid, &q, sigma, Laue::CubicHigh).iter().map(|v| v / mc).collect();
    let jq = f.iter().zip(&w).map(|(f, w)| f * f * w).sum::<f64>() / w.iter().sum::<f64>();
    let mq = f.iter().cloned().fold(0.0, f64::max);
    assert!((jq - jt).abs() / jt < 1e-9 && (mq - mt).abs() / mt < 1e-9, "{jq} {mq}");
    // ... and ours converges: 5 and 2.5 degree steps agree within 0.5 %.
    let (j2, _) = tx::texture_index(&q, sigma, Laue::CubicHigh, 2.5f64.to_radians());
    eprintln!("J at 2.5 degrees {j2}");
    assert!((j - j2).abs() / j2 < 0.005, "J {j} vs {j2}");
    assert!(fmax >= mt * 0.95, "max {fmax} vs {mt}");
}

#[test]
fn densities_average_to_one() {
    let (_, q) = am_bench();
    let sigma = 7.5f64.to_radians();
    // Pole figure: the mean over the upper hemisphere (equal-area cells) is 1.
    let n = 60;
    let mut dirs = Vec::new();
    for i in 0..n {
        let z = (i as f64 + 0.5) / n as f64;
        for k in 0..4 * n {
            let p = (k as f64 + 0.5) / (4 * n) as f64 * std::f64::consts::TAU;
            let r = (1.0 - z * z).sqrt();
            dirs.push([r * p.cos(), r * p.sin(), z]);
        }
    }
    for hkl in [[1.0, 0.0, 0.0], [1.0, 1.0, 1.0]] {
        let pf = tx::pole_figure(&q, None, hkl, Laue::CubicHigh, &dirs, sigma, 2.0);
        let m = pf.iter().sum::<f64>() / pf.len() as f64;
        assert!((m - 1.0).abs() < 0.01, "{m}");
    }
    // Inverse pole figure: the mean over the standard triangle (area-weighted) is 1.
    for laue in [Laue::CubicHigh, Laue::HexagonalHigh] {
        let sector = tx::sector_of(laue);
        let mut at = Vec::new();
        let mut w = Vec::new();
        let m = 80;
        for i in 0..m {
            for k in 0..m {
                let eta = sector.eta_min + (sector.eta_max - sector.eta_min) * (k as f64 + 0.5) / m as f64;
                let cm = sector.chi_max(eta);
                let z0 = cm.cos();
                let z = 1.0 - (1.0 - z0) * (i as f64 + 0.5) / m as f64;
                let chi = z.acos();
                at.push([chi.sin() * eta.cos(), chi.sin() * eta.sin(), z]);
                w.push(1.0 - z0);
            }
        }
        let d = tx::inverse_pole_figure(&q, None, [0.0, 0.0, 1.0], laue, &at, sigma, 1.0);
        let mean = d.iter().zip(&w).map(|(a, b)| a * b).sum::<f64>() / w.iter().sum::<f64>();
        assert!((mean - 1.0).abs() < 0.02, "{laue:?} {mean}");
    }
    // A random texture has an ODF near 1 and a texture index near 1 (plus the sampling floor).
    let mut s = 12345u64;
    let mut u = || {
        s ^= s << 13;
        s ^= s >> 7;
        s ^= s << 17;
        (s >> 11) as f64 / (1u64 << 53) as f64
    };
    let rnd: Vec<Quat> = (0..20000).map(|_| cx::random_quat(u(), u(), u())).collect();
    let (j, _) = tx::texture_index(&rnd, sigma, Laue::CubicHigh, 10f64.to_radians());
    eprintln!("random J {j}");
    assert!(j > 1.0 && j < 1.01, "{j}");
    for l in [Laue::HexagonalHigh, Laue::Orthorhombic] {
        let (j, _) = tx::texture_index(&rnd, sigma, l, 10f64.to_radians());
        assert!(j > 1.0 && j < 1.1, "{l:?} {j}");
    }
}

fn run(id: &str, input: Option<&DataSet>, set: &[(&str, serde_json::Value)]) -> DataSet {
    let def = aurafx_filters::registry::find(id).unwrap_or_else(|| panic!("no filter {id}"));
    let mut p = def.default_params();
    for (k, v) in set {
        p.set_json(k, v);
    }
    def.run(input, &p).unwrap_or_else(|e| panic!("{id}: {e:#}"))
}

fn blocks(d: &DataSet) -> Vec<(String, DataSet)> {
    match d {
        DataSet::MultiBlock(m) => m.blocks.iter().map(|(n, b)| (n.clone(), b.clone().unwrap())).collect(),
        _ => panic!("not a multiblock"),
    }
}

#[test]
fn texture_filters() {
    let (d, q) = am_bench();
    let pf = blocks(&run("pole_figure", Some(&d), &[]));
    assert_eq!(pf.iter().map(|b| b.0.as_str()).collect::<Vec<_>>(), ["{100}", "{110}", "{111}"]);
    let mrd = pf[0].1.point_data().unwrap().get("MRD").unwrap();
    // The centre of the disk is the sample z direction.
    let centre = tx::pole_figure(&q, None, [1.0, 0.0, 0.0], Laue::CubicHigh, &[[0.0, 0.0, 1.0]], 7.5f64.to_radians(), 2.0)[0];
    assert!((mrd.get(0, 0) - centre).abs() < 1e-9);
    let ea = blocks(&run("pole_figure", Some(&d), &[("families", "1 1 1".into()), ("projection", "Equal area".into())]));
    assert_eq!(ea.len(), 1);
    let ipf = blocks(&run("inverse_pole_figure", Some(&d), &[("directions", "Z".into())]));
    assert_eq!(ipf[0].0, "IPF Z");
    let odf = run("odf_sections", Some(&d), &[("step", 10.into())]);
    let j = odf.field_data().get("TextureIndex").unwrap().get(0, 0);
    assert!(j > 2.0 && j < 3.0, "{j}");
    let s = blocks(&odf);
    assert_eq!(s.len(), 3);
    assert_eq!(s[1].1.n_points(), 37 * 10);
    let csv = std::env::temp_dir().join(format!("aurafx_odf_{}.csv", std::process::id()));
    aurafx_io::writers::write_file(&odf, &csv).unwrap();
    let text = std::fs::read_to_string(&csv).unwrap();
    let _ = std::fs::remove_file(&csv);
    assert!(text.starts_with("\"Block\",Points:0"), "{}", &text[..80]);
    assert_eq!(text.lines().count(), 1 + 3 * 370);
    assert!(text.lines().nth(400).unwrap().starts_with("\"phi2 = 45\""));
    // A phase that is absent, and a structure that does not fit, are reported.
    let def = aurafx_filters::registry::find("pole_figure").unwrap();
    let mut p = def.default_params();
    p.set_json("phase", &2.into());
    assert!(def.run(Some(&d), &p).is_err());
    assert!(aurafx_filters::crystal_texture::parse_families("100, -1-12, 1 2 3").unwrap().len() == 3);
    assert!(aurafx_filters::crystal_texture::parse_families("10").is_err());
}

#[test]
fn chords_fractions_and_mdf() {
    let r = reference();
    let (d, _) = am_bench();
    let DataSet::ImageData(img) = &d else { panic!() };
    let c = img.cell_data.get("EulerAngles").unwrap();
    let ph = img.cell_data.get("Phases").unwrap();
    let (nx, ny) = (48, 40);
    let sp = floats(&r["spacing"]);
    let q = |i: usize, j: usize| -> Option<Quat> {
        let k = i + nx * j;
        (ph.get(k, 0) > 0.0).then(|| cx::euler_to_quat([c.get(k, 0), c.get(k, 1), c.get(k, 2)]))
    };
    // Chords of the first slice, along x (rows) and y (columns), as the toolkit lists them.
    let th = 10f64.to_radians();
    let mut first = Vec::new();
    for j in 0..ny {
        let line: Vec<_> = (0..nx).map(|i| q(i, j)).collect();
        first.extend(aurafx_filters::crystal_texture::line_chords(&line, Laue::CubicHigh, th, 3, false, sp[0]));
    }
    let mut second = Vec::new();
    for i in 0..nx {
        let line: Vec<_> = (0..ny).map(|j| q(i, j)).collect();
        second.extend(aurafx_filters::crystal_texture::line_chords(&line, Laue::CubicHigh, th, 3, false, sp[1]));
    }
    for (ours, theirs) in [(first, floats(&r["chords_first"])), (second, floats(&r["chords_second"]))] {
        assert_eq!(ours.len(), theirs.len());
        assert!(ours.iter().zip(&theirs).all(|(a, b)| (a - b).abs() < 1e-9));
    }
    // The filter on the whole volume adds the other slices and the z lines.
    let ch = run("chord_lengths", Some(&d), &[]);
    let fd = ch.field_data();
    assert!(fd.get("ChordsX").unwrap().get(0, 0) > r["chords_first"].as_array().unwrap().len() as f64);
    assert!(fd.get("MeanChordZ").is_some());
    let h = run("chord_lengths", Some(&d), &[("output", "Histogram".into()), ("bin", 2.0.into())]);
    let DataSet::Table(t) = &h else { panic!() };
    let hx = t.columns.get("X").unwrap();
    assert_eq!((0..hx.len()).map(|k| hx.get(k, 0)).sum::<f64>(), fd.get("ChordsX").unwrap().get(0, 0));
    // Component fractions: the toolkit's five (BD = Z).
    let tc = run("texture_components", Some(&d), &[]);
    let DataSet::Table(t) = &tc else { panic!() };
    let fr = t.columns.get("Fraction").unwrap();
    let names = |k: usize| -> String {
        let s = |n: &str| t.columns.get(n).unwrap().as_slice::<String>().unwrap()[k].clone();
        format!("{} {}", s("Direction"), s("Axis"))
    };
    for (key, want) in [("f100_X", "<100> X"), ("f100_Y", "<100> Y"), ("f100_BD", "<100> Z"), ("f110_BD", "<110> Z"), ("f111_BD", "<111> Z")] {
        let k = (0..fr.len()).find(|k| names(*k) == want).unwrap();
        let theirs = r["fractions"][key].as_f64().unwrap();
        assert!((fr.get(k, 0) - theirs).abs() < 1e-12, "{key}: {} vs {theirs}", fr.get(k, 0));
    }
    // Misorientation distribution: every curve is a density over the angles shown; the
    // uniform cubic one has the known mean of 40.74 degrees.
    let m = run("misorientation_distribution", Some(&d), &[("min_angle", 0.0.into()), ("bin", 0.5.into())]);
    let DataSet::Table(t) = &m else { panic!() };
    let get = |n: &str| -> Vec<f64> {
        let a = t.columns.get(n).unwrap();
        (0..a.len()).map(|k| a.get(k, 0)).collect()
    };
    let x = get("bin_center");
    for n in ["Neighbours", "Random pairs", "Uniform"] {
        let s: f64 = get(n).iter().sum::<f64>() * 0.5;
        assert!((s - 1.0).abs() < 1e-9, "{n} {s}");
    }
    let mean: f64 = get("Uniform").iter().zip(&x).map(|(p, a)| p * a * 0.5).sum();
    assert!((mean - 40.74).abs() < 0.05, "{mean}");
    let rp: f64 = get("Random pairs").iter().zip(&x).map(|(p, a)| p * a * 0.5).sum();
    assert!(rp > 30.0 && rp < 45.0, "{rp}");
    // Every face pair with both points indexed is counted.
    let v = aurafx_filters::features::Vox::of(img);
    let n = (0..v.len()).map(|k| v.nbrs(k)[3..].iter().flatten().filter(|o| ph.get(**o, 0) > 0.0 && ph.get(k, 0) > 0.0).count()).sum::<usize>();
    assert_eq!(t.field_data.get("NeighbourPairs").unwrap().get(0, 0) as usize, n);
}
