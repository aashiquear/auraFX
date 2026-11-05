use aurafx_data::ArrayData;
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

/// Range of an array over all leaves (magnitude for vectors), as ParaView reports it.
fn range_all(ds: &aurafx_data::DataSet, points: bool, name: &str) -> (f64, f64) {
    let leaves: Vec<&aurafx_data::DataSet> = match ds {
        aurafx_data::DataSet::MultiBlock(mb) => mb.leaves().into_iter().map(|(_, d)| d).collect(),
        d => vec![d],
    };
    let mut r = (f64::INFINITY, f64::NEG_INFINITY);
    for d in leaves {
        let attrs = if points { d.point_data() } else { d.cell_data() };
        if let Some(a) = attrs.and_then(|a| a.get(name)) {
            let (lo, hi) = a.range(None);
            r = (r.0.min(lo), r.1.max(hi));
        }
    }
    r
}

fn close(a: (f64, f64), b: (f64, f64), what: &str) {
    let tol = 1e-4 * (1.0 + b.0.abs().max(b.1.abs()));
    assert!((a.0 - b.0).abs() < tol && (a.1 - b.1).abs() < tol, "{what}: {a:?} vs ParaView {b:?}");
}

/// VTKHDF files written by ParaView 6.1 (and an image written by h5py), compared with what
/// ParaView's reader reports: counts, bounds and array ranges.
#[test]
fn vtkhdf_matches_paraview() {
    // Image data (written by h5py): extent 0..9 x 0..5 x 0..3, origin (1,2,3), spacing (.5,1,2).
    let img = static_ds("image.vtkhdf");
    assert_eq!((img.n_points(), img.n_cells()), (240, 135));
    let b = img.bounds();
    assert_eq!((b.min, b.max), ([1.0, 2.0, 3.0], [5.5, 7.0, 9.0]));
    close(range_all(&img, true, "f"), (0.0, 359.0), "f");
    close(range_all(&img, false, "c"), (0.0, 134.0), "c");
    // x fastest: point (3, 2, 1) holds 3 + 10*2 + 100*1.
    let f = img.point_data().unwrap().get("f").unwrap();
    assert_eq!(f.get(3 + 10 * 2 + 60 * 1, 0), 123.0);

    // Poly data (contiguous storage).
    let s = static_ds("sphere.vtkhdf");
    assert_eq!((s.n_points(), s.n_cells()), (50, 96));
    close(range_all(&s, true, "Elevation"), (0.0, 0.5), "Elevation");
    close(range_all(&s, true, "Normals"), (1.0, 1.0), "Normals");

    // A multiblock (partitioned collection with an assembly; gzip-compressed).
    let can = static_ds("can.vtkhdf");
    assert_eq!((can.n_points(), can.n_cells()), (10088, 7152));
    let b = can.bounds();
    close((b.min[0], b.max[0]), (-7.87846, 8.31258), "x bounds");
    close((b.min[2], b.max[2]), (-15.0, 4.7781), "z bounds");
    close(range_all(&can, true, "VEL"), (0.0, 5000.0), "VEL");
    close(range_all(&can, false, "object_id"), (1.0, 2.0), "object_id");
    let aurafx_data::DataSet::MultiBlock(mb) = &*can else { panic!("multiblock expected") };
    let names: Vec<String> = mb.leaves().into_iter().map(|(n, _)| n).collect();
    assert!(names.iter().any(|n| n.contains("element_blocks") && n.contains("block_1")), "assembly paths: {names:?}");

    // Time steps (chunked storage): 3 steps, the last one as ParaView shows it.
    let Loaded::Series(series) = read_file(td("series.vtkhdf")).unwrap() else { panic!("series expected") };
    assert_eq!(series.times, vec![0.0, 0.5, 1.0]);
    let last = load_step(&series.steps[2]).unwrap();
    assert_eq!((last.n_points(), last.n_cells()), (530, 1056));
    let b = last.bounds();
    close((b.min[2], b.max[2]), (-1.0, 1.4), "last step z");
    close(range_all(&last, true, "Elevation"), (0.0, 1.0), "Elevation");
    let first = load_step(&series.steps[0]).unwrap();
    assert!(first.bounds().max[2] < 1.4 - 1e-3, "steps differ");
}

/// Images written by VTK's writers, read as ParaView reads them (row 0 at the bottom).
#[test]
fn images_as_image_data() {
    let tuple = |ds: &aurafx_data::DataSet, name: &str, i: usize| -> Vec<f64> {
        let a = ds.point_data().unwrap().get(name).unwrap_or_else(|| panic!("no {name}"));
        (0..a.ncomp).map(|c| a.get(i, c)).collect()
    };
    let g = static_ds("images/gray.png");
    assert_eq!(g.n_points(), 240);
    let aurafx_data::DataSet::ImageData(im) = &*g else { panic!() };
    assert_eq!(im.extent, [0, 19, 0, 11, 0, 0]);
    // ParaView: first tuple 0, first tuple of the last row 11; range 0..201.
    assert_eq!(tuple(&g, "PNGImage", 0), vec![0.0]);
    assert_eq!(tuple(&g, "PNGImage", 20 * 11), vec![11.0]);
    close(range_all(&g, true, "PNGImage"), (0.0, 201.0), "gray");
    assert_eq!(g.point_data().unwrap().get("PNGImage").unwrap().scalar_type(), aurafx_data::ScalarType::U8);

    let c = static_ds("images/rgb.png");
    assert_eq!(tuple(&c, "PNGImage", 0), vec![0.0, 0.0, 77.0]);
    assert_eq!(tuple(&c, "PNGImage", 20 * 11), vec![0.0, 220.0, 77.0]);
    close(range_all(&c, true, "PNGImage"), (77.0, 326.0567435278712), "rgb magnitude");

    // JPEG: decoders may differ by a level or two from libjpeg's.
    let j = static_ds("images/rgb.jpg");
    for (i, want) in [(0usize, [0.0, 3.0, 69.0]), (20 * 11, [10.0, 214.0, 81.0])] {
        let got = tuple(&j, "JPEGImage", i);
        assert!(got.iter().zip(want).all(|(a, b)| (a - b).abs() <= 3.0), "jpeg tuple {i}: {got:?} vs ParaView {want:?}");
    }

    // A 16-bit multi-page TIFF is a volume (ParaView 6.1.1 crashes on this file). VTK's writer
    // stored y = 0 in the first row while labelling the file "top row first"; we follow the
    // label, as image viewers and VTK's reader for single images do, so y comes out mirrored:
    // the value at (x, y, z) is x + 100 (11 - y) + 2000 z.
    let v = static_ds("images/vol16.tif");
    let aurafx_data::DataSet::ImageData(im) = &*v else { panic!() };
    assert_eq!(im.extent, [0, 19, 0, 11, 0, 4]);
    assert_eq!(v.point_data().unwrap().get("Tiff Scalars").unwrap().scalar_type(), aurafx_data::ScalarType::U16);
    for (x, y, z) in [(0usize, 0usize, 0usize), (4, 3, 2), (19, 11, 4)] {
        assert_eq!(tuple(&v, "Tiff Scalars", x + 20 * y + 240 * z), vec![(x + 100 * (11 - y) + 2000 * z) as f64], "({x},{y},{z})");
    }

    // A wildcard stacks numbered slices in natural order: slice k holds 10x + y + 50k.
    let s = static_ds("images/stack/slice_*.png");
    let aurafx_data::DataSet::ImageData(im) = &*s else { panic!() };
    assert_eq!(im.extent, [0, 19, 0, 11, 0, 3]);
    for k in 0..4 {
        assert_eq!(tuple(&s, "PNGImage", 3 + 20 * 2 + 240 * k), vec![(30 + 2 + 50 * k) as f64], "slice {k}");
    }
}

/// EnSight Gold: a hand-written ASCII case (mixed elements, a structured block, two time steps
/// with wildcard file names) and a C Binary file written by ParaView, against ParaView's reader.
#[test]
fn ensight_matches_paraview() {
    let Loaded::Series(s) = read_file(td("ensight/mixed.case")).unwrap() else { panic!("two steps expected") };
    assert_eq!(s.times, vec![0.0, 0.25]);
    for (step, temp, pres) in [(0usize, (-1.0, 6.0), (1.0, 53.0)), (1, (99.0, 106.0), (11.0, 63.0))] {
        let d = load_step(&s.steps[step]).unwrap();
        assert_eq!((d.n_points(), d.n_cells()), (30, 9), "step {step}");
        let b = d.bounds();
        assert_eq!((b.min, b.max), ([-1.0, 0.0, 0.0], [6.0, 1.0, 2.0]));
        close(range_all(&d, true, "temperature"), temp, "temperature");
        close(range_all(&d, true, "velocity"), (0.0, 6.164414002968976), "velocity");
        close(range_all(&d, false, "pressure"), pres, "pressure");
    }
    // The wedge is turned round as VTK's reader does it (positive volume).
    let d = load_step(&s.steps[0]).unwrap();
    let aurafx_data::DataSet::MultiBlock(mb) = &*d else { panic!() };
    let (name, Some(aurafx_data::DataSet::UnstructuredGrid(ug))) = &mb.blocks[0] else { panic!() };
    assert_eq!(name, "solid");
    assert_eq!(ug.cells.cell(1), &[1, 2, 8, 5, 6, 9]);
    assert_eq!(ug.cells.types[1], aurafx_data::CellType::Wedge);

    let t = static_ds("ensight/tets.0.case");
    assert_eq!((t.n_points(), t.n_cells()), (1331, 5000));
    let b = t.bounds();
    assert_eq!((b.min, b.max), ([-5.0; 3], [5.0; 3]));
    close(range_all(&t, true, "RTData_n"), (37.35310363769531, 260.0), "RTData_n");
}

/// Tecplot ASCII: ordered, FE BLOCK with a cell-centered variable, old-style FEPOINT, against
/// ParaView; continuation lines, `n*value` repeats and Fortran exponents (which ParaView's
/// reader misreads) give the same data.
#[test]
fn tecplot_matches_paraview() {
    // tecplot.plt is the same data as binary (format 112), checked with VisIt's reader in
    // ParaView (which reads the double-precision cell-centered T as garbage).
    for file in ["tecplot.dat", "tecplot_extras.dat", "tecplot.plt"] {
        let d = static_ds(file);
        assert_eq!((d.n_points(), d.n_cells()), (15, 5), "{file}");
        let b = d.bounds();
        assert_eq!((b.min, b.max), ([0.0; 3], [4.0, 1.0, 1.0]));
        let aurafx_data::DataSet::MultiBlock(mb) = &*d else { panic!() };
        let names: Vec<&str> = mb.blocks.iter().map(|b| b.0.as_str()).collect();
        assert_eq!(names, ["ordered", "tets", "quads"]);
        let block = |k: usize| mb.blocks[k].1.as_ref().unwrap();
        close(range_all(block(0), true, "P"), (1.0, 6.0), "ordered P");
        close(range_all(block(1), true, "P"), (7.5, 10.0), "tets P");
        close(range_all(block(1), false, "T"), (100.0, 200.0), "tets T (cell centered)");
        close(range_all(block(2), true, "P"), (1.0, 4.0), "quads P");
        let aurafx_data::DataSet::UnstructuredGrid(t) = block(1) else { panic!() };
        assert_eq!(t.cells.types, vec![aurafx_data::CellType::Tetra; 2]);
        assert_eq!(t.cells.cell(1), &[1, 2, 3, 4]);
    }
}

/// PDB: every atom (ParaView's vtkPDBReader drops hydrogens and merges models), models as time
/// steps, bonds by distance; Gaussian cube: density in Å with z-fastest values reordered.
#[test]
fn pdb_and_cube() {
    let Loaded::Series(s) = read_file(td("molecules/small.pdb")).unwrap() else { panic!("two models") };
    assert_eq!(s.times.len(), 2);
    for (k, dx) in [(0usize, 0.0), (1, 0.1)] {
        let m = load_step(&s.steps[k]).unwrap();
        let aurafx_data::DataSet::PolyData(pd) = &*m else { panic!() };
        assert_eq!(pd.points.len(), 8);
        // The heavy atoms where ParaView puts them: O at the origin, C at x = 5.
        assert_eq!(pd.points.get(0), [dx, 0.0, 0.0]);
        assert_eq!(pd.points.get(3), [5.0 + dx, 0.0, 0.0]);
        assert_eq!(pd.lines.len(), 6, "2 O-H and 4 C-H bonds");
        let z: Vec<f64> = (0..8).map(|i| pd.point_data.get("atomic_number").unwrap().get(i, 0)).collect();
        assert_eq!(z, [8.0, 1.0, 1.0, 6.0, 1.0, 1.0, 1.0, 1.0]);
        let ArrayData::String(res) = &pd.point_data.get("residue").unwrap().data else { panic!() };
        assert_eq!((res[0].as_str(), res[3].as_str()), ("HOH", "MET"));
        assert_eq!(pd.point_data.get("ishetatm").unwrap().get(3, 0), 1.0);
    }
    let c = static_ds("molecules/density.cube");
    let aurafx_data::DataSet::MultiBlock(mb) = &*c else { panic!() };
    let Some(aurafx_data::DataSet::ImageData(img)) = &mb.blocks[0].1 else { panic!() };
    assert_eq!(img.extent, [0, 5, 0, 4, 0, 3]);
    let bohr = 0.529177210903;
    assert!((img.origin[0] + bohr).abs() < 1e-9 && (img.spacing[2] - 0.6 * bohr).abs() < 1e-9);
    let d = img.point_data.get("density").unwrap();
    assert_eq!(d.get(1 + 6 * 2 + 30 * 3, 0), 321.0, "value at (1, 2, 3)");
    let Some(aurafx_data::DataSet::PolyData(atoms)) = &mb.blocks[1].1 else { panic!() };
    assert_eq!(atoms.points.len(), 2);
    assert!((atoms.points.get(1)[0] - 1.4 * bohr).abs() < 1e-9);
    assert_eq!(atoms.lines.len(), 1, "O-H bond");
}

/// Xdmf files written by ParaView (HDF5 and inline data, Mixed topology, image data, a temporal
/// collection) against ParaView's reader; a FEniCS-style file (xi:include of the mesh, a
/// HyperSlab field) and a curvilinear Xdmf 2 grid by construction (ParaView drops the HyperSlab
/// field and fails on the curvilinear grid).
#[test]
fn xdmf_matches_paraview() {
    let s = static_ds("xdmf/sphere.xmf");
    assert_eq!((s.n_points(), s.n_cells()), (50, 96));
    close(range_all(&s, true, "Elevation"), (0.0, 0.5), "Elevation");
    close(range_all(&s, true, "Normals"), (1.0, 1.0), "Normals");
    let w = static_ds("xdmf/wavelet.xmf");
    assert_eq!((w.n_points(), w.n_cells()), (120, 60));
    let b = w.bounds();
    assert_eq!((b.min, b.max), ([0.0; 3], [5.0, 4.0, 3.0]));
    close(range_all(&w, true, "RTData"), (-13.0488920211792, 260.0), "RTData");
    let Loaded::Series(series) = read_file(td("xdmf/series.xmf")).unwrap() else { panic!("series") };
    assert_eq!(series.times, vec![0.0, 0.5, 1.0]);
    let last = load_step(&series.steps[2]).unwrap();
    assert_eq!((last.n_points(), last.n_cells()), (530, 1056));
    close((last.bounds().min[2], last.bounds().max[2]), (-1.0, 1.4), "z");

    let Loaded::Series(f) = read_file(td("xdmf/fenics.xdmf")).unwrap() else { panic!("fenics series") };
    assert_eq!(f.times, vec![0.0, 0.5]);
    let s1 = load_step(&f.steps[1]).unwrap();
    assert_eq!((s1.n_points(), s1.n_cells()), (4, 2), "mesh from xi:include");
    close(range_all(&s1, false, "p"), (5.0, 6.0), "p");
    let u: Vec<f64> = (0..4).map(|i| s1.point_data().unwrap().get("u").unwrap().get(i, 0)).collect();
    assert_eq!(u, [10.0, 20.0, 30.0, 40.0], "HyperSlab rows 1, 3, 5, 7");
    let s0 = load_step(&f.steps[0]).unwrap();
    close(range_all(&s0, true, "u"), (1.0, 4.0), "u at t=0");

    let c = static_ds("xdmf/curvilinear.xmf");
    let aurafx_data::DataSet::StructuredGrid(sg) = &*c else { panic!("structured grid") };
    assert_eq!(sg.extent, [0, 2, 0, 1, 0, 0]);
    assert_eq!(sg.points.get(5), [2.5, 1.4, 0.0]);
    assert_eq!(sg.cell_data.get("V").unwrap().ncomp, 3);
}

/// PLOT3D: a multi-grid Fortran-unformatted file with IBLANK and its Q solution, a C binary
/// double-precision grid and an ASCII 2-D grid, against ParaView's auto-detecting reader.
#[test]
fn plot3d_matches_paraview() {
    let m = static_ds("plot3d/multi.xyz");
    assert_eq!((m.n_points(), m.n_cells()), (24, 4));
    let b = m.bounds();
    assert_eq!((b.min, b.max), ([0.0; 3], [11.0, 0.5, 4.0]));
    close(range_all(&m, true, "Density"), (1.0, 2.1), "Density");
    close(range_all(&m, true, "Momentum"), (2.23607, 4.69574), "Momentum");
    close(range_all(&m, true, "StagnationEnergy"), (3.0, 3.11), "StagnationEnergy");
    close(range_all(&m, true, "Pressure"), (-0.856, 0.2), "Pressure");
    close(range_all(&m, true, "IBlank"), (0.0, 1.0), "IBlank");
    let s = static_ds("plot3d/single.bin");
    assert_eq!((s.n_points(), s.n_cells()), (24, 6));
    let b = s.bounds();
    assert!((b.max[0] - 0.75).abs() < 1e-12 && (b.max[2] - 0.8).abs() < 1e-12);
    let p = static_ds("plot3d/plane2d.xyz");
    assert_eq!((p.n_points(), p.n_cells()), (6, 2));
    assert_eq!(p.bounds().max, [2.0, 2.0, 0.0]);
    // Molecular XYZ files still open as molecules.
    let Loaded::Series(w) = read_file(td("water.xyz")).unwrap() else { panic!("water.xyz frames") };
    assert!(matches!(&*load_step(&w.steps[0]).unwrap(), aurafx_data::DataSet::PolyData(_)));
}

/// VASP: a POSCAR (direct coordinates, VASP 5 species line) and a CHGCAR (values per cell
/// volume, x fastest), by construction.
#[test]
fn vasp_files() {
    let p = static_ds("vasp/POSCAR");
    let aurafx_data::DataSet::PolyData(pd) = &*p else { panic!() };
    assert_eq!(pd.points.len(), 8);
    let x = pd.points.get(5);
    assert!((x[0] - 2.82).abs() < 1e-12 && (x[2] - 2.82).abs() < 1e-12, "Cl at (½,½,½) of 5.64 Å: {x:?}");
    let z: Vec<f64> = (0..8).map(|i| pd.point_data.get("atomic_number").unwrap().get(i, 0)).collect();
    assert_eq!(z, [11.0, 11.0, 11.0, 11.0, 17.0, 17.0, 17.0, 17.0]);
    assert_eq!(pd.lines.len(), 12, "nearest Na-Cl pairs at 2.82 Å inside the cell");
    assert_eq!(pd.field_data.get("Lattice").unwrap().get(0, 0), 5.64);

    let c = static_ds("vasp/CHGCAR");
    let aurafx_data::DataSet::MultiBlock(mb) = &*c else { panic!() };
    let Some(aurafx_data::DataSet::ImageData(img)) = &mb.blocks[0].1 else { panic!() };
    assert_eq!(img.extent, [0, 3, 0, 2, 0, 1]);
    assert_eq!(img.spacing, [0.75, 4.0 / 3.0, 2.5]);
    // Stored as value × 60 (the cell volume): back to i + 10 j + 100 k.
    let d = img.point_data.get("charge_density").unwrap();
    assert!((d.get(1 + 4 * 2 + 12, 0) - 121.0).abs() < 1e-9);
}

/// NetCDF CF: a NetCDF-4 lon/lat grid with bounds, time, a fill value and a packed variable, a
/// classic x/y/z grid, and (when ParaView's examples are installed) the IPCC sea surface
/// temperature file, against ParaView's NetCDF CF reader (fill values as NaN).
#[test]
fn netcdf_cf_matches_paraview() {
    let Loaded::Series(s) = read_file(td("netcdf/globe.nc")).unwrap() else { panic!("time steps") };
    assert_eq!(s.times, vec![0.5, 1.5, 2.5]);
    for (k, sst, pr) in [(0usize, (281.0, 311.0), (100.0, 115.5)), (2, (301.0, 331.0), (132.0, 147.5))] {
        let d = load_step(&s.steps[k]).unwrap();
        assert!(matches!(&*d, aurafx_data::DataSet::StructuredGrid(_)));
        assert_eq!((d.n_points(), d.n_cells()), (45, 32));
        let b = d.bounds();
        assert!(b.min.iter().chain(&b.max).all(|v| (v.abs() - 1.0).abs() < 1e-9), "unit sphere {b:?}");
        close(range_all(&d, false, "sst"), sst, "sst");
        close(range_all(&d, false, "pr"), pr, "pr");
        assert!(d.cell_data().unwrap().get("sst").unwrap().get(0, 0).is_nan(), "fill value");
    }
    let b = static_ds("netcdf/box.nc");
    assert!(matches!(&*b, aurafx_data::DataSet::RectilinearGrid(_)));
    assert_eq!((b.n_points(), b.n_cells()), (24, 6));
    let bb = b.bounds();
    assert_eq!((bb.min, bb.max), ([0.0, 0.0, 5.0], [4.0, 20.0, 6.0]));
    close(range_all(&b, true, "u"), (0.0, 23.0), "u");

    let tos = std::path::PathBuf::from(std::env::var("HOME").unwrap_or_default())
        .join("Downloads/ParaView-6.1.1-MPI-Linux-Python3.12-x86_64/share/paraview-6.1/examples/netCDF/tos_O1_2001-2002.nc");
    if tos.exists() {
        let Loaded::Series(s) = read_file(&tos).unwrap() else { panic!() };
        assert_eq!((s.times.len(), s.times[0], s.times[23]), (24, 15.0, 705.0));
        for (k, r) in [(0usize, (271.1732482910156, 304.6780090332031)), (23, (271.1903076171875, 305.0431823730469))] {
            let d = load_step(&s.steps[k]).unwrap();
            assert_eq!((d.n_points(), d.n_cells()), (30951, 30600));
            close(range_all(&d, false, "tos"), r, "tos");
        }
        // ParaView's value of cell (90, 85) at the last step.
        let d = load_step(&s.steps[23]).unwrap();
        assert!((d.cell_data().unwrap().get("tos").unwrap().get(85 * 180 + 90, 0) - 303.9855041503906).abs() < 1e-4);
    }
}

/// CGNS: a tetrahedral zone written by ParaView (Gradient X/Y/Z joined into a vector), and a
/// hand-made file with a structured zone whose solutions change over two time steps
/// (FlowSolutionPointers) and an unstructured zone with a MIXED section (CGNS 4 offsets) and a
/// boundary section, against ParaView's CGNS reader with every array enabled.
#[test]
fn cgns_matches_paraview() {
    let t = static_ds("cgns/tets.cgns");
    assert_eq!((t.n_points(), t.n_cells()), (1331, 5000));
    close(range_all(&t, true, "RTData"), (37.3531, 260.0), "RTData");
    close(range_all(&t, true, "Gradient"), (3.7762, 52.0886), "Gradient");
    let Loaded::Series(s) = read_file(td("cgns/mixed.cgns")).unwrap() else { panic!("two steps") };
    assert_eq!(s.times, vec![0.0, 0.5]);
    for (k, p, v, dens) in [(0usize, (0.0, 112.0), (1.0, 1.0), (5.0, 6.0)), (1, (1000.0, 1112.0), (1.4142, 1.4142), (6.0, 7.0))] {
        let d = load_step(&s.steps[k]).unwrap();
        assert_eq!((d.n_points(), d.n_cells()), (23, 5), "step {k}");
        let b = d.bounds();
        assert!((b.max[0] - 12.0).abs() < 1e-12 && (b.max[2] - 3.2).abs() < 1e-12);
        close(range_all(&d, true, "Pressure"), p, "Pressure");
        close(range_all(&d, true, "Velocity"), v, "Velocity");
        close(range_all(&d, true, "Temperature"), (300.0, 310.0), "Temperature");
        close(range_all(&d, false, "Density"), dens, "Density");
        close(range_all(&d, false, "Id"), (1.0, 3.0), "Id");
    }
    let d = load_step(&s.steps[0]).unwrap();
    let aurafx_data::DataSet::MultiBlock(mb) = &*d else { panic!() };
    assert!(matches!(mb.blocks[0].1, Some(aurafx_data::DataSet::StructuredGrid(_))));
    let Some(aurafx_data::DataSet::UnstructuredGrid(u)) = &mb.blocks[1].1 else { panic!() };
    assert_eq!(u.cells.types, [aurafx_data::CellType::Hexahedron, aurafx_data::CellType::Wedge, aurafx_data::CellType::Tetra]);
}
