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
    let l = run("voronoi_grains", None, &[("cells", serde_json::json!([60, 60, 60])), ("grains", 200.into()), ("size_sigma", 0.4.into()), ("periodic", true.into())]);
    let sl = run("ensemble_statistics", Some(&l), &[("include_surface", true.into())]);
    let s2 = sl.field_data().get("ESD sigma").unwrap().get(0, 0);
    eprintln!("laguerre: ESD sigma {s2:.3}");
    assert!(s2 > g("ESD sigma") + 0.05);
    // Orientation noise: angles of |N(0, 3 deg)| (mean 3 sqrt(2/pi) = 2.39 deg).
    let n = run("orientation_noise", Some(&v), &[("sigma", 3.0.into())]);
    let (a, b) = (v.cell_data().unwrap().get("EulerAngles").unwrap(), n.cell_data().unwrap().get("EulerAngles").unwrap());
    let k = 5000;
    let mean: f64 = (0..k).map(|i| cx::disorientation_angle(cx::euler_to_quat([a.get(i, 0), a.get(i, 1), a.get(i, 2)]), cx::euler_to_quat([b.get(i, 0), b.get(i, 1), b.get(i, 2)]), Laue::Triclinic).to_degrees()).sum::<f64>() / k as f64;
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
