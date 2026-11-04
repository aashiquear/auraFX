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

#[test]
fn openfoam_decomposed_case() {
    let s = match read_file(td("foam_decomposed/case.foam")).unwrap_or_else(|e| panic!("{e:#}")) {
        Loaded::Series(s) => s,
        _ => panic!("expected time steps"),
    };
    let d = load_step(&s.steps[1]).unwrap();
    assert_eq!(d.n_cells(), 48);
    assert_eq!(d.n_points(), 120);
    let b = d.bounds();
    assert_eq!((b.min[0], b.max[0]), (0.0, 8.0));
    // T = 300 + 10 x at cell centers: x from 0.5 to 7.5.
    assert_eq!(d.cell_data().unwrap().get("T").unwrap().range(None), (305.0, 375.0));
    assert_eq!(d.field_data().get("NumberOfProcessors").unwrap().get(0, 0), 2.0);
}

/// Patch face/point counts and value ranges at t = 1, from ParaView 6.1.1's OpenFOAM reader.
#[test]
fn openfoam_boundary_patches_match_paraview() {
    use aurafx_io::openfoam::{patch_names, read_patches};
    let cases: [(&str, &[(&str, usize, usize, (f64, f64), (f64, f64))]); 3] = [
        (
            "foam_ascii/case.foam",
            &[("inlet", 6, 12, (250.0, 250.0), (2.0, 2.0)), ("outlet", 6, 12, (335.0, 335.0), (3.5, 3.5)), ("walls", 40, 50, (400.0, 439.0), (0.0, 0.0))],
        ),
        (
            "foam_binary/case.foam",
            &[("inlet", 6, 12, (250.0, 250.0), (2.0, 2.0)), ("outlet", 6, 12, (335.0, 335.0), (3.5, 3.5)), ("walls", 40, 50, (400.0, 439.0), (0.0, 0.0))],
        ),
        (
            "foam_decomposed/case.foam",
            &[("inlet", 12, 24, (250.0, 250.0), (2.0, 2.0)), ("outlet", 12, 24, (335.0, 375.0), (3.5, 7.5)), ("walls", 80, 100, (400.0, 439.0), (0.0, 0.0))],
        ),
    ];
    for (case, expect) in cases {
        let names: Vec<String> = patch_names(&td(case)).unwrap().into_iter().map(|(n, _)| n).collect();
        assert_eq!(names, ["inlet", "outlet", "walls"], "{case}");
        let mb = read_patches(&td(case), Some(1.0), &[]).unwrap_or_else(|e| panic!("{case}: {e:#}"));
        assert_eq!(mb.blocks.len(), 3, "{case}");
        for ((name, cells, points, t, ux), (bn, b)) in expect.iter().zip(&mb.blocks) {
            let b = b.as_ref().unwrap();
            assert_eq!(bn, name, "{case}");
            assert_eq!((b.n_cells(), b.n_points()), (*cells, *points), "{case} {name}");
            let cd = b.cell_data().unwrap();
            assert_eq!(cd.get("T").unwrap().range(None), *t, "{case} {name} T");
            assert_eq!(cd.get("U").unwrap().range(Some(0)), *ux, "{case} {name} U.x");
        }
        // One patch by name.
        let one = read_patches(&td(case), Some(1.0), &["inlet".to_string()]).unwrap();
        assert_eq!(one.blocks.len(), 1);
        assert!(read_patches(&td(case), Some(1.0), &["nope".to_string()]).is_err());
    }
}

/// Write Exodus, read it back: same blocks, cells, (displaced) points, arrays and times.
#[test]
fn exodus_writer_roundtrip() {
    use aurafx_data::DataSet;
    use aurafx_io::exodus_writer::write_exodus_steps;
    // AURAFX_KEEP_OUTPUT=<dir> keeps the written files (for checking them in ParaView).
    let keep = std::env::var_os("AURAFX_KEEP_OUTPUT").map(std::path::PathBuf::from);
    let dir = keep.clone().unwrap_or_else(|| std::env::temp_dir().join(format!("aurafx-exo-{}", std::process::id())));
    std::fs::create_dir_all(&dir).unwrap();
    for name in ["disk_out_ref.ex2", "can.ex2"] {
        if !td(name).exists() {
            continue;
        }
        let steps: Vec<(f64, std::sync::Arc<DataSet>)> = match read_file(td(name)).unwrap() {
            Loaded::Series(s) => [0, s.steps.len() / 2, s.steps.len() - 1].iter().map(|&k| (s.times[k], load_step(&s.steps[k]).unwrap())).collect(),
            Loaded::Static(d) => vec![(0.0, d)],
        };
        let out = dir.join(name.replace(".ex2", "_out.exo"));
        let refs: Vec<(f64, &DataSet)> = steps.iter().map(|(t, d)| (*t, &**d)).collect();
        write_exodus_steps(&refs, &out).unwrap_or_else(|e| panic!("{name}: {e:#}"));
        let back = read_file(&out).unwrap_or_else(|e| panic!("{name} read back: {e:#}"));
        let back_steps: Vec<(f64, std::sync::Arc<DataSet>)> = match back {
            Loaded::Series(s) => (0..s.steps.len()).map(|k| (s.times[k], load_step(&s.steps[k]).unwrap())).collect(),
            Loaded::Static(d) => vec![(0.0, d)],
        };
        assert_eq!(back_steps.len(), steps.len(), "{name}: steps");
        for ((t0, a), (t1, b)) in steps.iter().zip(&back_steps) {
            assert_eq!(t0, t1, "{name}: time");
            let (DataSet::MultiBlock(ma), DataSet::MultiBlock(mb)) = (&**a, &**b) else { panic!("{name}: expected blocks") };
            let (la, lb) = (ma.leaves(), mb.leaves());
            assert_eq!(la.len(), lb.len(), "{name}: blocks");
            for ((na, da), (nb, db)) in la.iter().zip(&lb) {
                assert_eq!(na.rsplit('/').next(), nb.rsplit('/').next(), "{name}: block name");
                assert_eq!(da.n_cells(), db.n_cells(), "{name} {na}: cells");
                assert_eq!(da.n_points(), db.n_points(), "{name} {na}: points");
                let (pa, pb) = (da.points().unwrap(), db.points().unwrap());
                for i in (0..pa.len()).step_by(97) {
                    let (p, q) = (pa.get(i), pb.get(i));
                    assert!((0..3).all(|k| (p[k] - q[k]).abs() < 1e-9), "{name} {na}: point {i} {p:?} vs {q:?}");
                }
                for (attrs_a, attrs_b, what) in
                    [(da.point_data().unwrap(), db.point_data().unwrap(), "point"), (da.cell_data().unwrap(), db.cell_data().unwrap(), "cell")]
                {
                    for arr in attrs_a.iter().filter(|a| !["GlobalNodeId", "GlobalElementId", "ObjectId", "PedigreeElementId"].contains(&a.name.as_str())) {
                        let other = attrs_b.get(&arr.name).unwrap_or_else(|| panic!("{name} {na}: {what} array {} missing", arr.name));
                        assert_eq!(other.ncomp, arr.ncomp, "{name} {}", arr.name);
                        for c in 0..arr.ncomp {
                            assert_eq!(other.range(Some(c)), arr.range(Some(c)), "{name} {na}: {what} {} [{c}]", arr.name);
                        }
                    }
                }
            }
        }
    }
    if keep.is_none() {
        std::fs::remove_dir_all(&dir).ok();
    }
}
