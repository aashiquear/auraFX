use aurafx_io::{load_step, read_file, Loaded};
use std::path::PathBuf;

fn td(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../testdata").join(name)
}

fn static_ds(name: &str) -> std::sync::Arc<aurafx_data::DataSet> {
    match read_file(td(name)).unwrap_or_else(|e| panic!("{name}: {e:#}")) {
        Loaded::Static(d) => d,
        Loaded::Series(_) => panic!("{name}: expected static dataset"),
    }
}

#[test]
fn vti_wavelet() {
    let ds = static_ds("wavelet.vti");
    assert_eq!(ds.n_points(), 11 * 11 * 11);
    let a = ds.point_data().unwrap().get("RTData").unwrap();
    let (lo, hi) = a.range(None);
    assert!(lo > 0.0 && hi < 300.0, "range {lo} {hi}");
}

#[test]
fn vtu_variants_agree() {
    let a = static_ds("wavelet_tets.vtu");
    let b = static_ds("wavelet_tets_lz4.vtu");
    let c = static_ds("wavelet_tets_b64.vtu");
    let d = static_ds("wavelet_tets_legacy.vtk");
    for x in [&b, &c, &d] {
        assert_eq!(a.n_points(), x.n_points());
        assert_eq!(a.n_cells(), x.n_cells());
        assert_eq!(a.bounds(), x.bounds());
        let ra = a.point_data().unwrap().get("RTData").unwrap().range(None);
        let rx = x.point_data().unwrap().get("RTData").unwrap().range(None);
        assert!((ra.0 - rx.0).abs() < 1e-3 && (ra.1 - rx.1).abs() < 1e-3);
    }
    assert!(a.n_cells() > 1000);
}

#[test]
fn vtp_variants_agree() {
    let a = static_ds("contour.vtp");
    let b = static_ds("contour_ascii.vtp");
    let c = static_ds("contour_legacy_binary.vtk");
    let d = static_ds("contour_legacy_ascii.vtk");
    for x in [&b, &c, &d] {
        assert_eq!(a.n_points(), x.n_points());
        assert_eq!(a.n_cells(), x.n_cells());
    }
    assert!(a.n_cells() > 100);
}

#[test]
fn sphere_formats() {
    let vtp = static_ds("sphere.vtp");
    let vtk = static_ds("sphere_legacy.vtk");
    let stl = static_ds("sphere.stl");
    assert_eq!(vtp.n_cells(), vtk.n_cells());
    assert_eq!(vtp.n_cells(), stl.n_cells());
    assert!(vtp.point_data().unwrap().get("Elevation").is_some());
}

#[test]
fn rectilinear_and_structured() {
    let r = static_ds("rect.vtr");
    assert_eq!(r.n_points(), 24);
    assert_eq!(r.n_cells(), 6);
    let s = static_ds("struct.vts");
    assert_eq!(s.n_points(), 27);
    assert_eq!(s.n_cells(), 8);
}

#[test]
fn multiblock() {
    let g = static_ds("group.vtm");
    if let aurafx_data::DataSet::MultiBlock(mb) = &*g {
        assert_eq!(mb.leaves().len(), 2);
    } else {
        panic!("expected multiblock");
    }
}

#[test]
fn pvd_series() {
    match read_file(td("series.pvd")).unwrap() {
        Loaded::Series(s) => {
            assert_eq!(s.times, vec![0.0, 0.5, 1.0]);
            let d = load_step(&s.steps[2]).unwrap();
            assert!(d.n_points() > 0);
        }
        _ => panic!("expected series"),
    }
}

#[test]
fn lammps_dump_and_gz() {
    for name in ["lammps_small.dump", "lammps_small.dump.gz"] {
        match read_file(td(name)).unwrap() {
            Loaded::Series(s) => {
                assert_eq!(s.times, vec![0.0, 100.0]);
                let f0 = load_step(&s.steps[0]).unwrap();
                assert_eq!(f0.n_points(), 4);
                let pd = f0.point_data().unwrap();
                assert_eq!(pd.get("c_s").unwrap().ncomp, 3);
                assert_eq!(pd.get("velocity").unwrap().ncomp, 3);
                let f1 = load_step(&s.steps[1]).unwrap();
                let b = f1.bounds();
                assert!((b.min[0] - 1.0).abs() < 1e-9 && (b.max[2] - 5.0).abs() < 1e-9, "{b:?}");
            }
            _ => panic!("expected series"),
        }
    }
}
