//! Synthetic microstructures: columnar grains against the AM orientation toolkit's
//! generate_columns (the same random numbers replayed, testdata/synthetic/columns_reference.json
//! from scripts/columns_reference.py), and tessellations, orientation noise and ensemble
//! statistics against known properties.

use aurafx_crystal::{self as cx, Laue};
use aurafx_data::*;
use aurafx_filters::synthetic as syn;
use std::path::PathBuf;

fn run(id: &str, input: Option<&DataSet>, set: &[(&str, serde_json::Value)]) -> DataSet {
    let def = aurafx_filters::registry::find(id).unwrap_or_else(|| panic!("no filter {id}"));
    let mut p = def.default_params();
    for (k, v) in set {
        p.set_json(k, v);
    }
    def.run(input, &p).unwrap_or_else(|e| panic!("{id}: {e:#}"))
}

#[test]
fn columns_match_toolkit() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../testdata/synthetic/columns_reference.json");
    let cases: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    for c in cases.as_array().unwrap() {
        let mic = &c["microstructure"];
        let f = |k: &str| mic.get(k).and_then(|v| v.as_f64()).unwrap_or(0.0);
        let b: Vec<f64> = c["box_um"].as_array().unwrap().iter().map(|x| x.as_f64().unwrap()).collect();
        let p = syn::ColumnParams {
            size: [b[0], b[1], b[2]],
            voxel: c["voxel_um"].as_f64().unwrap(),
            width: f("grain_width_um"),
            height: f("grain_height_um"),
            height_sigma: f("height_sigma"),
            tilt_sd_deg: f("column_tilt_sd_deg"),
            width_sigma: f("width_sigma"),
            width_height_exponent: f("width_height_exponent"),
        };
        let draws: Vec<(bool, f64)> = c["draws"].as_array().unwrap().iter().map(|d| (d[0].as_str().unwrap() == "n", d[1].as_f64().unwrap())).collect();
        let nd = draws.len();
        let mut r = syn::Replay { draws, pos: 0 };
        let col = syn::columns(&p, &mut r).unwrap();
        assert_eq!(r.pos, nd, "{}: all the draws are used", c["name"]);
        let rle: Vec<u32> = c["labels_rle"].as_array().unwrap().iter().map(|x| x.as_u64().unwrap() as u32).collect();
        let theirs: Vec<u32> = rle.chunks(2).flat_map(|p| std::iter::repeat_n(p[0], p[1] as usize)).collect();
        assert_eq!(col.labels.len(), theirs.len());
        let diff = col.labels.iter().zip(&theirs).filter(|(a, b)| a != b).count();
        assert_eq!(diff, 0, "{}: {diff} voxels differ", c["name"]);
        let base: Vec<f64> = c["base_um"].as_array().unwrap().iter().map(|x| x.as_f64().unwrap()).collect();
        assert_eq!(col.base, base);
    }
}

#[test]
fn tessellations_and_orientations() {
    // Voronoi grains: as many as seeds (on a fine enough grid), each face-connected, with the
    // mean neighbour count of a Poisson-Voronoi tessellation (15.5) for interior grains.
    let v = run("voronoi_grains", None, &[("cells", serde_json::json!([60, 60, 60])), ("grains", 200.into()), ("periodic", true.into())]);
    let st = run("ensemble_statistics", Some(&v), &[("include_surface", true.into())]);
    let fd = st.field_data();
    let g = |k: &str| fd.get(k).unwrap().get(0, 0);
    eprintln!("voronoi: {} features, ESD mu {:.3} sigma {:.3}, neighbours mean {:.2}", g("Features"), g("ESD mu"), g("ESD sigma"), g("Neighbors mean"));
    assert_eq!(g("Features"), 200.0);
    assert!((g("Neighbors mean") - 15.5).abs() < 1.2, "{}", g("Neighbors mean"));
    // Poisson-Voronoi cell volumes: coefficient of variation 0.42, so the log-sigma of the
    // diameters is about 0.42 / 3.
    assert!((g("ESD sigma") - 0.14).abs() < 0.04, "{}", g("ESD sigma"));
    // Laguerre grains with spread sizes.
    let l = run(
        "voronoi_grains",
        None,
        &[("cells", serde_json::json!([60, 60, 60])), ("grains", 200.into()), ("size_sigma", 0.4.into()), ("periodic", true.into())],
    );
    let sl = run("ensemble_statistics", Some(&l), &[("include_surface", true.into())]);
    let s2 = sl.field_data().get("ESD sigma").unwrap().get(0, 0);
    eprintln!("laguerre: ESD sigma {s2:.3}");
    assert!(s2 > g("ESD sigma") + 0.05);
    // Orientation noise: angles of |N(0, 3 deg)| (mean 3 sqrt(2/pi) = 2.39 deg).
    let n = run("orientation_noise", Some(&v), &[("sigma", 3.0.into())]);
    let (a, b) = (v.cell_data().unwrap().get("EulerAngles").unwrap(), n.cell_data().unwrap().get("EulerAngles").unwrap());
    let k = 5000;
    let mean: f64 = (0..k)
        .map(|i| {
            cx::disorientation_angle(
                cx::euler_to_quat([a.get(i, 0), a.get(i, 1), a.get(i, 2)]),
                cx::euler_to_quat([b.get(i, 0), b.get(i, 1), b.get(i, 2)]),
                Laue::Triclinic,
            )
            .to_degrees()
        })
        .sum::<f64>()
        / k as f64;
    assert!((mean - 3.0 * (2.0 / std::f64::consts::PI).sqrt()).abs() < 0.1, "{mean}");
    // Jumble: the same set of feature orientations, assigned differently.
    let j = run("jumble_orientations", Some(&v), &[]);
    let ids = v.cell_data().unwrap().get("FeatureIds").unwrap();
    let ja = j.cell_data().unwrap().get("EulerAngles").unwrap();
    let per = |e: &Array| -> Vec<[i64; 3]> {
        let mut m = std::collections::BTreeMap::new();
        for i in 0..ids.len() {
            m.entry(ids.get(i, 0) as i64).or_insert([0, 1, 2].map(|c| (e.get(i, c) * 1e4).round() as i64));
        }
        let mut v: Vec<[i64; 3]> = m.into_values().collect();
        v.sort();
        v
    };
    assert_eq!(per(a), per(ja));
    assert_ne!((0..ids.len()).map(|i| ja.get(i, 0)).take(1000).collect::<Vec<_>>(), (0..ids.len()).map(|i| a.get(i, 0)).take(1000).collect::<Vec<_>>());
    // Columnar grains with a <100> fibre along Z: the build direction is near a cube axis.
    let c = run("columnar_grains", None, &[("orientations", "<100> along Z".into()), ("spread", 5.0.into())]);
    let e = c.cell_data().unwrap().get("EulerAngles").unwrap();
    let near = (0..e.len()).step_by(97).all(|i| {
        let q = cx::euler_to_quat([e.get(i, 0), e.get(i, 1), e.get(i, 2)]);
        let d = cx::rotate(q, [0.0, 0.0, 1.0]);
        d.iter().map(|x| x.abs()).fold(0.0, f64::max) > 20f64.to_radians().cos()
    });
    assert!(near);
    assert!(c.field_data().get("GrainTop").is_some());
}

fn stat(d: &DataSet, k: &str) -> f64 {
    d.field_data().get(k).unwrap_or_else(|| panic!("no {k}")).get(0, 0)
}

/// The packing builder recovers its goal statistics from the volume it generates.
#[test]
fn packing_recovers_goal_statistics() {
    // Goal: diameters lognormal (mu ln 12, sigma 0.15), b/a ~ Beta(12, 4) (mean 0.75), c/a ~
    // Beta(10, 6) (mean 0.63).
    let mu = 12f64.ln();
    let g = run(
        "pack_grains",
        None,
        &[
            ("cells", serde_json::json!([80, 80, 80])),
            ("esd_mu", mu.into()),
            ("esd_sigma", 0.15.into()),
            ("ba", serde_json::json!([12, 4])),
            ("ca", serde_json::json!([10, 6])),
            ("periodic", true.into()),
        ],
    );
    let st = run("ensemble_statistics", Some(&g), &[("include_surface", true.into())]);
    eprintln!(
        "packed: {} grains, ESD mu {:.3} (goal {mu:.3}) sigma {:.3} (0.15), b/a mean {:.3} (0.75), c/a mean {:.3} (0.63), overlap {:.3}",
        stat(&st, "Features"),
        stat(&st, "ESD mu"),
        stat(&st, "ESD sigma"),
        stat(&st, "b/a mean"),
        stat(&st, "c/a mean"),
        stat(&g, "OverlapFraction")
    );
    // Interior grains (the ones the calibration measures).
    let si = run("ensemble_statistics", Some(&g), &[]);
    eprintln!("interior: ESD mu {:.3}, b/a {:.3}, c/a {:.3}", stat(&si, "ESD mu"), stat(&si, "b/a mean"), stat(&si, "c/a mean"));
    assert!((stat(&si, "ESD mu") - mu).abs() < 0.05);
    assert!((stat(&si, "b/a mean") - 0.75).abs() < 0.03);
    assert!((stat(&si, "c/a mean") - 0.625).abs() < 0.03);
    assert!(stat(&st, "ESD sigma") < 0.25);
    // A matrix phase fills what the grains leave at a 60 % grain fraction.
    let m = run("pack_grains", None, &[("cells", serde_json::json!([60, 60, 60])), ("esd_mu", 10f64.ln().into()), ("fraction", 0.6.into())]);
    let ph = m.cell_data().unwrap().get("Phases").unwrap();
    let frac = (0..ph.len()).filter(|i| ph.get(*i, 0) == 2.0).count() as f64 / ph.len() as f64;
    eprintln!("matrix fraction {frac:.3} (goal 0.4)");
    assert!((frac - 0.4).abs() < 0.08, "{frac}");
    assert_eq!(m.field_data().get("CrystalStructures").unwrap().len(), 3);
    // Precipitates: 5 % of the volume as particles of a new phase.
    let p = run("insert_precipitates", Some(&g), &[("fraction", 0.05.into()), ("esd_mu", 4f64.ln().into())]);
    let ph = p.cell_data().unwrap().get("Phases").unwrap();
    let pf = (0..ph.len()).filter(|i| ph.get(*i, 0) == 2.0).count() as f64 / ph.len() as f64;
    eprintln!("precipitates {pf:.4} (goal 0.05)");
    assert!((pf - 0.05).abs() < 0.01, "{pf}");
    // Pack Like: a copy of the Laguerre volume with its size statistics.
    let l = run("voronoi_grains", None, &[("cells", serde_json::json!([60, 60, 60])), ("grains", 150.into()), ("size_sigma", 0.3.into())]);
    let sl = run("ensemble_statistics", Some(&l), &[]);
    let copy = run("pack_like", Some(&l), &[("cells", serde_json::json!([60, 60, 60]))]);
    let sc = run("ensemble_statistics", Some(&copy), &[]);
    eprintln!(
        "pack like: ESD mu {:.3} vs {:.3}, sigma {:.3} vs {:.3}",
        stat(&sc, "ESD mu"),
        stat(&sl, "ESD mu"),
        stat(&sc, "ESD sigma"),
        stat(&sl, "ESD sigma")
    );
    assert!((stat(&sc, "ESD mu") - stat(&sl, "ESD mu")).abs() < 0.15);
}

/// Match Crystallography brings the boundary misorientations of a synthetic volume close to
/// a reference whose neighbouring grains have similar orientations.
#[test]
fn crystallography_matches_reference() {
    // Reference: Voronoi grains turned about z by an angle that grows with x, so neighbours
    // are close in orientation although the texture spreads over 90 degrees.
    let reference = run("voronoi_grains", None, &[("cells", serde_json::json!([48, 48, 48])), ("grains", 150.into()), ("seed", 3.into())]);
    let DataSet::ImageData(ri) = &reference else { panic!() };
    let mut ri = ri.clone();
    let seeds = ri.field_data.get("Seeds").unwrap().clone();
    let ids = ri.cell_data.get("FeatureIds").unwrap().clone();
    let e: Vec<f32> = (0..ids.len())
        .flat_map(|c| {
            let f = ids.get(c, 0) as usize;
            let x = seeds.get(f, 0) / 48.0;
            cx::quat_to_euler(cx::axis_angle_to_quat([0.0, 0.0, 1.0], x * std::f64::consts::FRAC_PI_2)).map(|v| v as f32)
        })
        .collect();
    ri.cell_data.insert(Array::new("EulerAngles", 3, ArrayData::F32(e)));
    let reference = DataSet::ImageData(ri);
    let synth = run("voronoi_grains", None, &[("cells", serde_json::json!([48, 48, 48])), ("grains", 150.into()), ("seed", 9.into())]);
    let def = aurafx_filters::registry::find("match_crystallography").unwrap();
    let out = def.run_multi(&[&synth, &reference], &def.default_params()).unwrap();
    let (b, a) = (stat(&out, "MDFErrorBefore"), stat(&out, "MDFErrorAfter"));
    eprintln!("MDF error {b:.3} -> {a:.3}");
    assert!(a < 0.5 * b, "{b} {a}");
}

