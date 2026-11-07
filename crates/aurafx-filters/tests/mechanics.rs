//! Crystal mechanics against the AM orientation toolkit (taylor_factor, schmid_max,
//! youngs_modulus of am_build_campaign) on 2000 orientations of the AM Bench window
//! (testdata/crystal/mechanics_reference.json from scripts/mechanics_reference.py), and the
//! random-texture Taylor factor.

use aurafx_crystal::mechanics as mech;
use aurafx_crystal::{self as cx};
use aurafx_data::*;
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
fn mechanics_match_toolkit() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../testdata/crystal/mechanics_reference.json");
    let r: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    let q: Vec<cx::Quat> = r["euler"].as_array().unwrap().iter().map(|e| cx::euler_to_quat([0, 1, 2].map(|k| e[k].as_f64().unwrap()))).collect();
    let systems = mech::slip_systems(&[0], 1.0);
    let bases = mech::TaylorBases::new(&systems, &vec![1.0; 12]).unwrap();
    let s = mech::compliance(&mech::stiffness(mech::Lattice::Fcc, &[250.0, 160.0, 120.0])).unwrap();
    for (axis, d) in [("z", [0.0, 0.0, 1.0]), ("x", [1.0, 0.0, 0.0])] {
        let (t, sm, e) = (&r[format!("taylor_{axis}")], &r[format!("schmid_{axis}")], &r[format!("youngs_{axis}")]);
        let mut worst = [0.0f64; 3];
        for (i, g) in q.iter().enumerate() {
            let tt = bases.taylor(*g, d);
            let (ss, _) = mech::schmid(*g, d, &systems);
            let ee = mech::youngs_modulus(&s, cx::rotate(*g, d));
            worst[0] = worst[0].max((tt - t[i].as_f64().unwrap()).abs());
            worst[1] = worst[1].max((ss - sm[i].as_f64().unwrap()).abs());
            worst[2] = worst[2].max((ee - e[i].as_f64().unwrap()).abs());
        }
        eprintln!("{axis}: largest differences Taylor {:.1e}, Schmid {:.1e}, Young {:.1e} GPa", worst[0], worst[1], worst[2]);
        assert!(worst[0] < 1e-9 && worst[1] < 1e-9 && worst[2] < 1e-9);
    }
    // A random FCC texture: Taylor factor 3.07 (Kocks: 3.06), largest Schmid factor 0.45.
    let mut st = 5u64;
    let mut u = || {
        st ^= st << 13;
        st ^= st >> 7;
        st ^= st << 17;
        (st >> 11) as f64 / (1u64 << 53) as f64
    };
    let n = 20_000;
    let (mut mt, mut ms) = (0.0, 0.0);
    for _ in 0..n {
        let g = cx::random_quat(u(), u(), u());
        mt += bases.taylor(g, [0.0, 0.0, 1.0]);
        ms += mech::schmid(g, [0.0, 0.0, 1.0], &systems).0;
    }
    eprintln!("random: Taylor {:.4}, Schmid {:.4}", mt / n as f64, ms / n as f64);
    assert!((mt / n as f64 - 3.06).abs() < 0.03);
    assert!((ms / n as f64 - 0.45).abs() < 0.01);
    // HCP: Taylor factors exist with the pyramidal <c+a> systems, and grow along c.
    let ti = mech::slip_systems(&[4, 5, 6, 7], 1.587);
    let w: Vec<f64> = ti.iter().map(|s| [1.0, 0.7, 1.0, 3.0][s.family - 4]).collect();
    let tb = mech::TaylorBases::new(&ti, &w).unwrap();
    let (mc, ma) = (tb.taylor(cx::IDENTITY, [0.0, 0.0, 1.0]), tb.taylor(cx::IDENTITY, [1.0, 0.0, 0.0]));
    eprintln!("titanium: Taylor along c {mc:.2}, along a {ma:.2}");
    assert!(mc > ma);
}

#[test]
fn mechanics_filter() {
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../testdata/dream3d/am_bench_subset.dream3d");
    let d = match aurafx_io::read_file(p).unwrap() {
        aurafx_io::Loaded::Static(d) => (*d).clone(),
        _ => panic!(),
    };
    let m = run("crystal_mechanics", Some(&d), &[]);
    let fd = m.field_data();
    let g = |k: &str| fd.get(k).unwrap().get(0, 0);
    eprintln!("AM Bench window along z: Taylor {:.3}, Schmid {:.3}, E Voigt {:.1} Reuss {:.1} GPa, yield {:.0} MPa", g("MeanTaylorFactor"), g("MeanSchmidFactor"), g("YoungsModulusVoigt"), g("YoungsModulusReuss"), g("YieldEstimateMPa"));
    assert!(g("YoungsModulusReuss") <= g("YoungsModulusVoigt"));
    assert!((g("YieldEstimateMPa") - 647.6 * g("MeanTaylorFactor") / 3.06).abs() < 1e-6);
    let c = m.cell_data().unwrap();
    assert!(c.get("TaylorFactor").is_some() && c.get("SchmidFactor").is_some());
    let hcp = run("crystal_mechanics", Some(&d), &[("material", "Custom".into()), ("lattice", "HCP".into()), ("elastic", serde_json::json!([162.4, 92.0, 69.0, 180.7, 46.7])), ("families", "Basal <a>, Prismatic <a>=0.7, Pyramidal <c+a>=3".into()), ("structure", "Hexagonal 6/mmm".into())]);
    assert!(hcp.field_data().get("MeanTaylorFactor").unwrap().get(0, 0) > 2.0);
}

/// SROD and the Nye GND density against the toolkit's am_ebsd3d_analysis (analyse, nye_gnd) on
/// the 1 um AM Bench feature window (testdata/crystal/am_reference.json, scripts/am_reference.py).
#[test]
fn srod_and_gnd_match_toolkit() {
    use aurafx_filters::am;
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../testdata");
    let r: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(root.join("crystal/am_reference.json")).unwrap()).unwrap();
    let d = match aurafx_io::read_file(root.join("dream3d/am_bench_features.dream3d")).unwrap() {
        aurafx_io::Loaded::Static(d) => (*d).clone(),
        _ => panic!(),
    };
    let DataSet::ImageData(img) = &d else { panic!() };
    let bottom = img.origin[2] + img.extent[4] as f64 * img.spacing[2];
    let ru = &r["rules"];
    let f = |k: &str| ru[k].as_f64().unwrap();
    let rules = am::SrodRules {
        layer_um: f("bin_um"),
        plate_top: bottom + r["plate_top_above_bottom"].as_f64().unwrap(),
        interface_um: f("interface_um"),
        min_volume: f("min_volume_um3"),
        reference_volume: f("reference_volume_um3"),
        reference_sections: ru["reference_sections"].as_u64().unwrap() as usize,
        min_bin_volume: f("min_bin_volume_um3"),
    };
    let s = am::srod(&d, "FeatureIds", &rules).unwrap();
    let g = &r["grains"];
    let feats: Vec<usize> = g["feature"].as_array().unwrap().iter().map(|x| x.as_u64().unwrap() as usize).collect();
    let analysed = s.grains.iter().filter(|x| x.0 >= 0).count();
    assert_eq!(analysed, feats.len());
    let classes = ["plate", "epitaxial", "interface", "originating"];
    let mut worst = 0.0f64;
    for (i, ft) in feats.iter().enumerate() {
        let ours = s.grains[*ft];
        assert_eq!(classes[ours.0 as usize], g["class"][i].as_str().unwrap(), "feature {ft}");
        assert_eq!(ours.3, g["n_bins"][i].as_u64().unwrap() as usize, "feature {ft}");
        for (k, key) in [(4, "first"), (5, "max"), (6, "rate"), (7, "voxel_mean")] {
            let w = g[key][i].as_f64().unwrap_or(f64::NAN);
            let o = [ours.4, ours.5, ours.6, ours.7][k - 4];
            if w.is_nan() {
                assert!(o.is_nan(), "{key} of {ft}: {o}");
            } else {
                worst = worst.max((o - w).abs());
            }
        }
    }
    let b = &r["bins"];
    assert_eq!(s.bins.len(), b["bin"].as_array().unwrap().len());
    for (i, ob) in s.bins.iter().enumerate() {
        assert_eq!(ob.feature as u64, b["feature"][i].as_u64().unwrap());
        assert_eq!(ob.bin as u64, b["bin"][i].as_u64().unwrap());
        assert_eq!(ob.n_vox as u64, b["n_vox"][i].as_u64().unwrap());
        worst = worst.max((ob.srod - b["srod"][i].as_f64().unwrap()).abs());
        worst = worst.max((ob.stats[3] - b["p50"][i].as_f64().unwrap()).abs());
    }
    let sv = &r["srod_voxels"];
    for (k, idx) in sv["index"].as_array().unwrap().iter().enumerate() {
        worst = worst.max((s.field[idx.as_u64().unwrap() as usize] - sv["value"][k].as_f64().unwrap()).abs());
    }
    eprintln!("SROD: {analysed} grains, {} bins, largest difference {worst:.2e} deg", s.bins.len());
    assert!(worst < 2e-3, "{worst}");
    // Nye GND: the same cells defined, values within the toolkit's 32-bit arithmetic.
    let (rho, _) = am::nye_gnd(&d, "FeatureIds", 2.54e-10, 3, 6).unwrap();
    let ny = &r["nye"];
    let nan = rho.iter().filter(|x| x.is_nan()).count();
    assert_eq!(nan as u64, ny["nan_count"].as_u64().unwrap());
    let mut wr = 0.0f64;
    for (k, idx) in ny["index"].as_array().unwrap().iter().enumerate() {
        let w = ny["value"][k].as_f64().unwrap();
        wr = wr.max((rho[idx.as_u64().unwrap() as usize] - w).abs() / w.max(1e12));
    }
    let mean = rho.iter().filter(|x| x.is_finite()).sum::<f64>() / rho.iter().filter(|x| x.is_finite()).count() as f64;
    eprintln!("Nye: mean {mean:.4e} vs {:.4e}, largest relative difference {wr:.1e}", ny["mean"].as_f64().unwrap());
    assert!(wr < 1e-3, "{wr}");
}

#[test]
fn srod_and_gnd_filters() {
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../testdata/dream3d/am_bench_features.dream3d");
    let d = match aurafx_io::read_file(p).unwrap() {
        aurafx_io::Loaded::Static(d) => (*d).clone(),
        _ => panic!(),
    };
    let v = run("srod", Some(&d), &[("layer", 4.0.into()), ("min_volume", 200.0.into()), ("reference_volume", 50.0.into()), ("min_bin_volume", 20.0.into())]);
    assert!(v.cell_data().unwrap().get("SROD").is_some() && v.field_data().get("SRODRate").is_some());
    let b = run("srod", Some(&d), &[("layer", 4.0.into()), ("min_volume", 200.0.into()), ("reference_volume", 50.0.into()), ("min_bin_volume", 20.0.into()), ("output", "Bins".into())]);
    let DataSet::Table(t) = &b else { panic!() };
    assert!(t.n_rows() > 100);
    let h = run("srod", Some(&d), &[("layer", 4.0.into()), ("min_volume", 200.0.into()), ("output", "By height".into())]);
    let DataSet::Table(t) = &h else { panic!() };
    assert!(t.columns.get("SROD epitaxial").is_some());
    let n = run("gnd_density", Some(&d), &[]);
    assert!(n.field_data().get("MeanGND").unwrap().get(0, 0) > 1e12);
    let k = run("kernel_average_misorientation", Some(&d), &[]);
    let g = run("gnd_density", Some(&k), &[("method", "From KAM".into())]);
    let kam = k.cell_data().unwrap().get("KernelAverageMisorientations").unwrap();
    let rho = g.cell_data().unwrap().get("GND").unwrap();
    let i = (0..kam.len()).find(|i| kam.get(*i, 0) > 0.1).unwrap();
    assert!((rho.get(i, 0) - kam.get(i, 0).to_radians() / (1e-6 * 2.54e-10)).abs() / rho.get(i, 0) < 1e-12);
}
