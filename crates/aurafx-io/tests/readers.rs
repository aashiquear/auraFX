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

#[test]
fn exodus_examples() {
    for (name, min_blocks) in [("disk_out_ref.ex2", 1), ("can.ex2", 2), ("bake.e", 1)] {
        let p = td(name);
        if !p.exists() {
            eprintln!("skipping {name}: not present");
            continue;
        }
        let loaded = read_file(&p).unwrap_or_else(|e| panic!("{name}: {e:#}"));
        let ds = match loaded {
            Loaded::Static(d) => d,
            Loaded::Series(s) => {
                assert!(s.times.len() > 1);
                load_step(s.steps.last().unwrap()).unwrap()
            }
        };
        let aurafx_data::DataSet::MultiBlock(mb) = &*ds else { panic!("{name}: expected multiblock") };
        let leaves = mb.leaves();
        assert!(leaves.len() >= min_blocks, "{name}: {} blocks", leaves.len());
        assert!(ds.n_cells() > 0 && ds.n_points() > 0);
        let info = aurafx_data::DataSetInfo::of(&ds);
        eprintln!("{name}: {}", info.summary());
    }
}

#[test]
fn writers_roundtrip() {
    let dir = std::env::temp_dir().join(format!("afx-writers-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    for (src, ext) in [
        ("wavelet_tets.vtu", "vtu"),
        ("wavelet_tets.vtu", "vtk"),
        ("contour.vtp", "vtp"),
        ("contour.vtp", "vtk"),
        ("wavelet.vti", "vti"),
        ("wavelet.vti", "vtk"),
        ("rect.vtr", "vtr"),
        ("struct.vts", "vts"),
        ("group.vtm", "vtm"),
    ] {
        let a = static_ds(src);
        let out = dir.join(format!("{}_{}.{ext}", src.replace('.', "_"), ext));
        aurafx_io::writers::write_file(&a, &out).unwrap_or_else(|e| panic!("{src} -> {ext}: {e:#}"));
        let b = match read_file(&out).unwrap_or_else(|e| panic!("re-read {}: {e:#}", out.display())) {
            Loaded::Static(d) => d,
            _ => panic!(),
        };
        assert_eq!(a.n_points(), b.n_points(), "{src}->{ext}");
        assert_eq!(a.n_cells(), b.n_cells(), "{src}->{ext}");
        assert_eq!(a.bounds(), b.bounds(), "{src}->{ext}");
        if let (Some(pa), Some(pb)) = (a.point_data(), b.point_data()) {
            for arr in pa.iter() {
                let other = pb.get(&arr.name).unwrap_or_else(|| panic!("{src}->{ext}: lost {}", arr.name));
                assert_eq!(arr.range(None), other.range(None), "{src}->{ext}: {}", arr.name);
            }
        }
    }
    let sphere = static_ds("sphere.vtp");
    for ext in ["stl", "ply", "csv"] {
        aurafx_io::writers::write_file(&sphere, &dir.join(format!("sphere.{ext}"))).unwrap();
    }
    let stl = static_ds_abs(&dir.join("sphere.stl"));
    assert_eq!(stl.n_cells(), sphere.n_cells());
}

fn static_ds_abs(p: &std::path::Path) -> std::sync::Arc<aurafx_data::DataSet> {
    match read_file(p).unwrap() {
        Loaded::Static(d) => d,
        _ => panic!(),
    }
}

#[test]
fn exodus_netcdf4_matches_classic() {
    for (classic, nc4) in [("disk_out_ref.ex2", "disk_out_ref_nc4.exo"), ("can.ex2", "can_nc4.exo")] {
        if !td(nc4).exists() || !td(classic).exists() {
            eprintln!("skipping {nc4}");
            continue;
        }
        let steps = |name: &str| -> Vec<std::sync::Arc<aurafx_data::DataSet>> {
            match read_file(td(name)).unwrap_or_else(|e| panic!("{name}: {e:#}")) {
                Loaded::Static(d) => vec![d],
                Loaded::Series(s) => vec![load_step(&s.steps[0]).unwrap(), load_step(s.steps.last().unwrap()).unwrap()],
            }
        };
        let (a, b) = (steps(classic), steps(nc4));
        assert_eq!(a.len(), b.len(), "{nc4}: time steps");
        for (x, y) in a.iter().zip(&b) {
            assert_eq!(x.n_points(), y.n_points(), "{nc4}");
            assert_eq!(x.n_cells(), y.n_cells(), "{nc4}");
            assert_eq!(x.bounds(), y.bounds(), "{nc4}");
            let (ix, iy) = (aurafx_data::DataSetInfo::of(x), aurafx_data::DataSetInfo::of(y));
            for (p, q) in ix.point_arrays.iter().zip(&iy.point_arrays) {
                assert_eq!((&p.name, p.range), (&q.name, q.range), "{nc4}");
            }
            for (p, q) in ix.cell_arrays.iter().zip(&iy.cell_arrays) {
                assert_eq!((&p.name, p.range), (&q.name, q.range), "{nc4}");
            }
            assert_eq!(ix.blocks, iy.blocks);
        }
    }
}

#[test]
fn openfoam_cases_match_paraview() {
    for case in ["foam_ascii/case.foam", "foam_binary/case.foam"] {
        let s = match read_file(td(case)).unwrap_or_else(|e| panic!("{case}: {e:#}")) {
            Loaded::Series(s) => s,
            _ => panic!("{case}: expected two time steps"),
        };
        assert_eq!(s.times, vec![0.0, 1.0]);
        let d = load_step(&s.steps[1]).unwrap();
        assert_eq!(d.n_points(), 60, "{case}");
        assert_eq!(d.n_cells(), 24, "{case}");
        let aurafx_data::DataSet::UnstructuredGrid(u) = &*d else { panic!() };
        assert!(u.cells.types.iter().all(|t| *t == aurafx_data::CellType::Hexahedron), "{case}: all hexes");
        assert_eq!(u.cell_data.get("T").unwrap().range(None), (305.0, 335.0));
        let ur = u.cell_data.get("U").unwrap().range(None);
        assert!((ur.0 - 0.8660254037844386).abs() < 1e-12 && (ur.1 - 4.55521678957215).abs() < 1e-12, "{ur:?}");
        assert!(u.point_data.get("T").is_some());
        // Hexes must have positive volume (correct VTK ordering).
        let v = aurafx_data::DataSet::UnstructuredGrid(u.clone());
        let surf = d.bounds();
        assert_eq!((surf.min, surf.max), ([0.0; 3], [4.0, 3.0, 2.0]));
        let _ = v;
        let t0 = load_step(&s.steps[0]).unwrap();
        assert_eq!(t0.cell_data().unwrap().get("T").unwrap().range(None), (300.0, 300.0));
    }
}

#[test]
fn md_and_surface_formats() {
    let x = match read_file(td("water.xyz")).unwrap() {
        Loaded::Series(s) => s,
        _ => panic!("two frames"),
    };
    let f0 = load_step(&x.steps[0]).unwrap();
    assert_eq!(f0.n_points(), 3);
    let pd = f0.point_data().unwrap();
    assert_eq!(pd.get("atomic_number").unwrap().range(None), (1.0, 8.0));
    assert_eq!(pd.get("forces").unwrap().ncomp, 3);
    assert_eq!(f0.field_data().get("energy").unwrap().get(0, 0), -14.2);
    let l = static_ds("two_waters.data");
    assert_eq!(l.n_points(), 6);
    let aurafx_data::DataSet::PolyData(p) = &*l else { panic!() };
    assert_eq!(p.lines.len(), 4);
    assert_eq!(p.point_data.get("mass").unwrap().range(None), (1.008, 15.9994));
    assert_eq!(p.point_data.get("velocity").unwrap().ncomp, 3);
    let o = static_ds("pyramid.obj");
    assert_eq!((o.n_points(), o.n_cells()), (5, 5));
    let ply_dir = std::env::temp_dir().join(format!("afx-ply-{}", std::process::id()));
    std::fs::create_dir_all(&ply_dir).unwrap();
    let sphere = static_ds("sphere.vtp");
    aurafx_io::writers::write_file(&sphere, &ply_dir.join("s.ply")).unwrap();
    let back = static_ds_abs(&ply_dir.join("s.ply"));
    assert_eq!((back.n_points(), back.n_cells()), (sphere.n_points(), 1056));
}
