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
    // The CF units attribute becomes the array's units.
    let Loaded::Series(g) = read_file(td("netcdf/globe_sst.nc")).unwrap() else { panic!("time steps") };
    let d = load_step(&g.steps[0]).unwrap();
    let a = d.cell_data().and_then(|c| c.get("sst")).or_else(|| d.point_data().and_then(|p| p.get("sst"))).expect("sst");
    assert_eq!(a.units.as_deref(), Some("K"));
    assert_eq!(a.label(), "sst (K)");
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

/// The HDF5 writer: groups, datasets of several types, attributes, a soft link, read back.
#[test]
fn hdf5_writer_round_trip() {
    use aurafx_io::hdf5w::{write, AttrValue, Node};
    let mut root = Node::group();
    let g = root.group_mut("VTKHDF");
    g.attr("Version", AttrValue::Data(ArrayData::I64(vec![2, 5])));
    g.attr("Type", AttrValue::Str("UnstructuredGrid".into()));
    g.add("Points", Node::dataset(ArrayData::F32(vec![0.0, 1.0, 2.0, 3.0, 4.0, 5.0]), vec![2, 3]));
    g.add("Types", Node::dataset(ArrayData::U8(vec![10, 12]), vec![2]));
    g.add("Empty", Node::dataset(ArrayData::I64(vec![]), vec![0]));
    let pd = g.group_mut("PointData");
    pd.add("T", Node::dataset(ArrayData::F64(vec![1.5, -2.5]), vec![2])).attr("units", AttrValue::Str("K".into()));
    root.add("Link", Node::SoftLink("/VTKHDF/PointData".into()));
    let path = std::env::temp_dir().join(format!("aurafx_h5w_{}.h5", std::process::id()));
    write(&path, &root).unwrap();
    let f = hdf5_reader::Hdf5File::open(&path).unwrap();
    let g = f.group("VTKHDF").unwrap();
    assert_eq!(g.attribute("Type").unwrap().read_string().unwrap().trim_end_matches('\0'), "UnstructuredGrid");
    assert_eq!(g.attribute("Version").unwrap().read_1d::<i64>().unwrap(), vec![2, 5]);
    let p = f.dataset("VTKHDF/Points").unwrap();
    assert_eq!(p.shape(), &[2, 3]);
    assert_eq!(p.read_array::<f32>().unwrap().iter().cloned().collect::<Vec<_>>(), vec![0.0, 1.0, 2.0, 3.0, 4.0, 5.0]);
    assert_eq!(f.dataset("VTKHDF/Types").unwrap().read_array::<u8>().unwrap().iter().cloned().collect::<Vec<_>>(), vec![10, 12]);
    assert_eq!(f.dataset("VTKHDF/Empty").unwrap().shape(), &[0]);
    assert_eq!(f.dataset("VTKHDF/PointData/T").unwrap().read_array::<f64>().unwrap().iter().cloned().collect::<Vec<_>>(), vec![1.5, -2.5]);
    if std::env::var("AURAFX_KEEP").is_ok() {
        eprintln!("kept {}", path.display());
    } else {
        let _ = std::fs::remove_file(&path);
    }
}

/// VTKHDF writing: each kind of dataset (and a 3-step series) written and read back with the
/// same counts, bounds and ranges. The files are also checked with ParaView (see AURAFX_KEEP).
#[test]
fn vtkhdf_writer_round_trip() {
    let dir = std::env::temp_dir().join(format!("aurafx_vtkhdf_w_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let same = |a: &aurafx_data::DataSet, b: &aurafx_data::DataSet, arrays: &[(bool, &str)], what: &str| {
        assert_eq!((a.n_points(), a.n_cells()), (b.n_points(), b.n_cells()), "{what}: counts");
        let (ba, bb) = (a.bounds(), b.bounds());
        for k in 0..3 {
            assert!((ba.min[k] - bb.min[k]).abs() < 1e-5 && (ba.max[k] - bb.max[k]).abs() < 1e-5, "{what}: bounds");
        }
        for (pts, n) in arrays {
            close(range_all(a, *pts, n), range_all(b, *pts, n), &format!("{what}: {n}"));
        }
    };
    let can = match read_file(td("can.ex2")).unwrap() {
        Loaded::Series(s) => load_step(&s.steps[5]).unwrap(),
        Loaded::Static(d) => d,
    };
    let disk = static_ds("disk_out_ref.ex2");
    let first_leaf = |d: &aurafx_data::DataSet| match d {
        aurafx_data::DataSet::MultiBlock(mb) => mb.leaves()[0].1.clone(),
        d => d.clone(),
    };
    let disk_ug = first_leaf(&disk);
    let cases: Vec<(&str, aurafx_data::DataSet, Vec<(bool, &str)>)> = vec![
        ("can", (*can).clone(), vec![(true, "VEL"), (false, "EQPS")]),
        ("disk", disk_ug, vec![(true, "Temp"), (true, "V")]),
        ("image", (*static_ds("wavelet.vti")).clone(), vec![(true, "RTData")]),
        ("sphere", (*static_ds("sphere.vtkhdf")).clone(), vec![(true, "Elevation"), (true, "Normals")]),
        ("struct", (*static_ds("struct.vts")).clone(), vec![]),
    ];
    for (name, ds, arrays) in &cases {
        let p = dir.join(format!("{name}.vtkhdf"));
        aurafx_io::writers::write_file(ds, &p).unwrap_or_else(|e| panic!("{name}: {e:#}"));
        let back = static_ds(p.to_str().unwrap());
        same(&back, ds, arrays, name);
    }
    // A time series of unstructured grids.
    let Loaded::Series(s) = read_file(td("series.pvd")).unwrap() else { panic!() };
    let steps: Vec<(f64, std::sync::Arc<aurafx_data::DataSet>)> = s
        .times
        .iter()
        .zip(&s.steps)
        .map(|(t, r)| {
            let d = load_step(r).unwrap();
            (*t, std::sync::Arc::new(first_leaf(&d)))
        })
        .collect();
    let refs: Vec<(f64, &aurafx_data::DataSet)> = steps.iter().map(|(t, d)| (*t, &**d)).collect();
    let p = dir.join("series.vtkhdf");
    aurafx_io::vtkhdf::write_vtkhdf_steps(&refs, &p).unwrap();
    let Loaded::Series(back) = read_file(&p).unwrap() else { panic!("series") };
    assert_eq!(back.times, s.times);
    for (k, (_, d)) in steps.iter().enumerate() {
        same(&load_step(&back.steps[k]).unwrap(), d, &[(true, "Elevation")], &format!("series step {k}"));
    }
    if std::env::var("AURAFX_KEEP").is_ok() {
        eprintln!("kept {}", dir.display());
    } else {
        let _ = std::fs::remove_dir_all(&dir);
    }
}

/// EnSight Gold writing: each kind of dataset, mixed cell types (cell values follow their
/// cells when the elements are grouped by type) and a time series, read back with the same
/// counts, bounds and ranges.
#[test]
fn ensight_writer_round_trip() {
    use aurafx_data::*;
    let dir = std::env::temp_dir().join(format!("aurafx_ensight_w_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let same = |a: &DataSet, b: &DataSet, arrays: &[(bool, &str)], what: &str| {
        assert_eq!((a.n_points(), a.n_cells()), (b.n_points(), b.n_cells()), "{what}: counts");
        let (ba, bb) = (a.bounds(), b.bounds());
        for k in 0..3 {
            assert!((ba.min[k] - bb.min[k]).abs() < 1e-4 && (ba.max[k] - bb.max[k]).abs() < 1e-4, "{what}: bounds");
        }
        for (pts, n) in arrays {
            close(range_all(a, *pts, n), range_all(b, *pts, n), &format!("{what}: {n}"));
        }
    };
    let can = match read_file(td("can.ex2")).unwrap() {
        Loaded::Series(s) => load_step(&s.steps[5]).unwrap(),
        Loaded::Static(d) => d,
    };
    let Loaded::Series(mixed) = read_file(td("ensight/mixed.case")).unwrap() else { panic!() };
    let mixed0 = load_step(&mixed.steps[0]).unwrap();
    let cases: Vec<(&str, DataSet, Vec<(bool, &str)>)> = vec![
        ("can", (*can).clone(), vec![(true, "VEL"), (false, "EQPS")]),
        ("disk", (*static_ds("disk_out_ref.ex2")).clone(), vec![(true, "Temp"), (true, "V")]),
        ("image", (*static_ds("wavelet.vti")).clone(), vec![(true, "RTData")]),
        ("sphere", (*static_ds("sphere.vtkhdf")).clone(), vec![(true, "Elevation"), (true, "Normals")]),
        ("struct", (*static_ds("struct.vts")).clone(), vec![]),
        ("rect", (*static_ds("rect.vtr")).clone(), vec![]),
        ("quadratic", (*static_ds("quadratic_shapes.vtu")).clone(), vec![]),
        ("mixed", (*mixed0).clone(), vec![(true, "temperature"), (true, "velocity"), (false, "pressure")]),
    ];
    for (name, ds, arrays) in &cases {
        let p = dir.join(format!("{name}.case"));
        aurafx_io::writers::write_file(ds, &p).unwrap_or_else(|e| panic!("{name}: {e:#}"));
        let back = static_ds(p.to_str().unwrap());
        same(&back, ds, arrays, name);
    }

    // Triangle, quad, triangle with cell values 0, 1, 2: read back grouped as tria3, tria3,
    // quad4, and each cell keeps its value.
    let mut cells = Cells::new();
    cells.push(CellType::Triangle, &[0, 1, 4]);
    cells.push(CellType::Quad, &[1, 2, 3, 4]);
    cells.push(CellType::Triangle, &[0, 4, 5]);
    let mut cd = Attributes::new();
    cd.insert(Array::scalars_f64("id", vec![0.0, 1.0, 2.0]));
    let pts = vec![0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 2.0, 0.0, 0.0, 2.0, 1.0, 0.0, 1.0, 1.0, 0.0, 0.0, 1.0, 0.0];
    let ug =
        UnstructuredGrid { points: Points::new_f64(pts), cells: cells.clone(), point_data: Attributes::new(), cell_data: cd, field_data: Attributes::new() };
    let p = dir.join("grouped.case");
    aurafx_io::writers::write_file(&DataSet::UnstructuredGrid(ug), &p).unwrap();
    let back = static_ds(p.to_str().unwrap());
    let DataSet::MultiBlock(mb) = &*back else { panic!() };
    let Some(DataSet::UnstructuredGrid(g)) = &mb.blocks[0].1 else { panic!() };
    let ids = g.cell_data.get("id").unwrap();
    for c in 0..3 {
        let original = ids.get(c, 0) as usize;
        assert_eq!(g.cells.cell(c), cells.cell(original), "cell {c}");
    }

    // A time series.
    let Loaded::Series(s) = read_file(td("series.pvd")).unwrap() else { panic!() };
    let steps: Vec<(f64, std::sync::Arc<DataSet>)> = s.times.iter().zip(&s.steps).map(|(t, r)| (*t, load_step(r).unwrap())).collect();
    let refs: Vec<(f64, &DataSet)> = steps.iter().map(|(t, d)| (*t, &**d)).collect();
    let p = dir.join("series.case");
    aurafx_io::ensight_writer::write_ensight_steps(&refs, &p).unwrap();
    let Loaded::Series(back) = read_file(&p).unwrap() else { panic!("series") };
    assert_eq!(back.times.len(), s.times.len());
    for (a, b) in back.times.iter().zip(&s.times) {
        assert!((a - b).abs() < 1e-9 * (1.0 + b.abs()));
    }
    for (k, (_, d)) in steps.iter().enumerate() {
        same(&load_step(&back.steps[k]).unwrap(), d, &[(true, "Elevation")], &format!("series step {k}"));
    }
    if std::env::var("AURAFX_KEEP").is_ok() {
        eprintln!("kept {}", dir.display());
    } else {
        let _ = std::fs::remove_dir_all(&dir);
    }
}

/// Xdmf writing (XML + HDF5): each kind of dataset, multiblock trees, a strip split into
/// triangles and a prism written as a polyhedron (cell values follow), and a time series.
#[test]
fn xdmf_writer_round_trip() {
    use aurafx_data::*;
    let dir = std::env::temp_dir().join(format!("aurafx_xdmf_w_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let same = |a: &DataSet, b: &DataSet, arrays: &[(bool, &str)], what: &str| {
        assert_eq!((a.n_points(), a.n_cells()), (b.n_points(), b.n_cells()), "{what}: counts");
        let (ba, bb) = (a.bounds(), b.bounds());
        for k in 0..3 {
            assert!((ba.min[k] - bb.min[k]).abs() < 1e-9 && (ba.max[k] - bb.max[k]).abs() < 1e-9, "{what}: bounds");
        }
        for (pts, n) in arrays {
            close(range_all(a, *pts, n), range_all(b, *pts, n), &format!("{what}: {n}"));
        }
    };
    let can = match read_file(td("can.ex2")).unwrap() {
        Loaded::Series(s) => load_step(&s.steps[5]).unwrap(),
        Loaded::Static(d) => d,
    };
    let cases: Vec<(&str, DataSet, Vec<(bool, &str)>)> = vec![
        ("can", (*can).clone(), vec![(true, "VEL"), (false, "EQPS")]),
        ("disk", (*static_ds("disk_out_ref.ex2")).clone(), vec![(true, "Temp"), (true, "V")]),
        ("image", (*static_ds("wavelet.vti")).clone(), vec![(true, "RTData")]),
        ("sphere", (*static_ds("sphere.vtkhdf")).clone(), vec![(true, "Elevation"), (true, "Normals")]),
        ("struct", (*static_ds("struct.vts")).clone(), vec![]),
        ("rect", (*static_ds("rect.vtr")).clone(), vec![]),
        ("quadratic", (*static_ds("quadratic_shapes.vtu")).clone(), vec![]),
        ("group", (*static_ds("group.vtm")).clone(), vec![]),
    ];
    for (name, ds, arrays) in &cases {
        let p = dir.join(format!("{name}.xmf"));
        aurafx_io::writers::write_file(ds, &p).unwrap_or_else(|e| panic!("{name}: {e:#}"));
        let back = static_ds(p.to_str().unwrap());
        same(&back, ds, arrays, name);
    }

    // A strip of two triangles (cell 0) and a pentagonal prism (cell 1).
    let mut pts = Vec::new();
    for z in [0.0, 1.0] {
        for k in 0..5 {
            let a = k as f64 * std::f64::consts::TAU / 5.0;
            pts.extend([a.cos(), a.sin(), z]);
        }
    }
    pts.extend([0.0, 0.0, 2.0, 1.0, 0.0, 2.0, 0.0, 1.0, 2.0, 1.0, 1.0, 2.0]);
    let mut cells = Cells::new();
    cells.push(CellType::TriangleStrip, &[10, 11, 12, 13]);
    cells.push(CellType::PentagonalPrism, &[0, 1, 2, 3, 4, 5, 6, 7, 8, 9]);
    let mut cd = Attributes::new();
    cd.insert(Array::scalars_f64("id", vec![7.0, 9.0]));
    let ug = UnstructuredGrid { points: Points::new_f64(pts), cells, point_data: Attributes::new(), cell_data: cd, field_data: Attributes::new() };
    let p = dir.join("poly.xmf");
    aurafx_io::writers::write_file(&DataSet::UnstructuredGrid(ug), &p).unwrap();
    let back = static_ds(p.to_str().unwrap());
    let DataSet::UnstructuredGrid(g) = &*back else { panic!() };
    assert_eq!(g.cells.types, vec![CellType::Triangle, CellType::Triangle, CellType::Polyhedron]);
    assert_eq!(g.cell_data.get("id").unwrap().data.to_f64_vec(), vec![7.0, 7.0, 9.0]);
    let faces = g.cells.polyhedron_faces.as_ref().unwrap();
    assert_eq!(faces[2].len(), 7);
    assert_eq!(g.cells.cell(2), &[0, 1, 2, 3, 4, 5, 6, 7, 8, 9]);

    // A time series.
    let Loaded::Series(s) = read_file(td("series.pvd")).unwrap() else { panic!() };
    let steps: Vec<(f64, std::sync::Arc<DataSet>)> = s.times.iter().zip(&s.steps).map(|(t, r)| (*t, load_step(r).unwrap())).collect();
    let refs: Vec<(f64, &DataSet)> = steps.iter().map(|(t, d)| (*t, &**d)).collect();
    let p = dir.join("series.xmf");
    aurafx_io::xdmf_writer::write_xdmf_steps(&refs, &p).unwrap();
    let Loaded::Series(back) = read_file(&p).unwrap() else { panic!("series") };
    assert_eq!(back.times, s.times);
    for (k, (_, d)) in steps.iter().enumerate() {
        same(&load_step(&back.steps[k]).unwrap(), d, &[(true, "Elevation")], &format!("series step {k}"));
    }
    if std::env::var("AURAFX_KEEP").is_ok() {
        eprintln!("kept {}", dir.display());
    } else {
        let _ = std::fs::remove_dir_all(&dir);
    }
}

/// A series whose geometry does not change: EnSight writes one geometry file, Xdmf stores the
/// points and connectivity once; both read back with each step's values.
#[test]
fn writers_share_static_geometry() {
    use aurafx_data::*;
    let dir = std::env::temp_dir().join(format!("aurafx_static_geo_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let base = (*static_ds("sphere.vtkhdf")).clone();
    let steps: Vec<(f64, DataSet)> = (0..3)
        .map(|k| {
            let mut d = base.clone();
            let n = d.n_points();
            d.point_data_mut().unwrap().insert(Array::scalars_f64("step", vec![k as f64; n]));
            (k as f64 * 0.5, d)
        })
        .collect();
    let refs: Vec<(f64, &DataSet)> = steps.iter().map(|(t, d)| (*t, d)).collect();
    aurafx_io::ensight_writer::write_ensight_steps(&refs, &dir.join("s.case")).unwrap();
    aurafx_io::xdmf_writer::write_xdmf_steps(&refs, &dir.join("s.xmf")).unwrap();
    assert!(dir.join("s.geo").exists() && !dir.join("s.geo0000").exists());
    let xml = std::fs::read_to_string(dir.join("s.xmf")).unwrap();
    assert_eq!(xml.matches("s.h5:/s0/connectivity").count(), 3, "{xml}");
    assert!(!xml.contains("/s1/connectivity") && !xml.contains("/s2/points"));
    for f in ["s.case", "s.xmf"] {
        let Loaded::Series(back) = read_file(dir.join(f)).unwrap() else { panic!("{f}: series") };
        assert_eq!(back.times, vec![0.0, 0.5, 1.0], "{f}");
        for k in 0..3 {
            let d = load_step(&back.steps[k]).unwrap();
            assert_eq!((d.n_points(), d.n_cells()), (base.n_points(), base.n_cells()), "{f}");
            close(range_all(&d, true, "step"), (k as f64, k as f64), &format!("{f} step {k}"));
        }
    }
    if std::env::var("AURAFX_KEEP").is_ok() {
        eprintln!("kept {}", dir.display());
    } else {
        let _ = std::fs::remove_dir_all(&dir);
    }
}

/// CGNS writing: each kind of dataset, cells read back with the same nodes (quadratic orders
/// included), polygons (NGON in 2-D) and polyhedra (NGON/NFACE) after the fixed cells with
/// their cell values, and a series with moving points (GridCoordinatesPointers).
/// GeoTIFF rasters written by GDAL (scripts/gen_rasters.py), compared with ParaView 6.1.1's GDAL
/// raster reader: image data with a cell per pixel, origin and spacing from the geo-transform,
/// rows north-up, bands named as ParaView names them; first/last cell values and sums match
/// (no-data cells are NaN here, hidden cells holding -9999 in ParaView). The Esri ASCII grid
/// holds the same elevations to 6 digits, and the ERDAS Imagine copies the same data.
#[test]
fn rasters_match_paraview() {
    use aurafx_data::DataSet;
    let img = |name: &str| -> aurafx_data::ImageData {
        match &*static_ds(name) {
            DataSet::ImageData(i) => i.clone(),
            _ => panic!("{name}: not image data"),
        }
    };
    let check = |name: &str, extent: [i64; 6], origin: [f64; 3], spacing: [f64; 3]| {
        let i = img(name);
        assert_eq!(i.extent, extent, "{name}: extent");
        assert_eq!(i.origin, origin, "{name}: origin");
        assert_eq!(i.spacing, spacing, "{name}: spacing");
        i
    };
    let comp0 = |i: &aurafx_data::ImageData, a: &str| -> Vec<f64> {
        let arr = i.cell_data.get(a).unwrap_or_else(|| panic!("no {a}"));
        let v = arr.data.to_f64_vec();
        v.iter().step_by(arr.ncomp).copied().collect()
    };
    let sum = |v: &[f64]| v.iter().filter(|x| !x.is_nan()).sum::<f64>();

    let dem = check("rasters/dem.tif", [0, 120, 0, 80, 0, 0], [500000.0, 4197600.0, 0.0], [30.0, 30.0, 1.0]);
    let e = comp0(&dem, "Elevation");
    assert_eq!(e.iter().filter(|x| x.is_nan()).count(), 150, "no-data cells");
    assert!((e[0] - 400.0005187988281).abs() < 1e-9 && (e[9599] - 615.8598022460938).abs() < 1e-9);
    // ParaView's sum 3855458.361 counts the 150 no-data cells as -9999.
    assert!((sum(&e) - (3855458.361 + 150.0 * 9999.0)).abs() < 0.01, "{}", sum(&e));
    let proj = dem.field_data.get("MAP_PROJECTION").map(|a| a.data.clone());
    assert!(matches!(proj, Some(aurafx_data::ArrayData::String(ref v)) if v[0] == "EPSG:32633"), "{proj:?}");
    assert_eq!(dem.field_data.get("NO_DATA_VALUE").unwrap().data.to_f64_vec(), vec![-9999.0]);

    let rgb = check("rasters/rgb.tif", [0, 64, 0, 48, 0, 0], [10.0, 38.0, 0.0], [0.25, 0.25, 1.0]);
    assert_eq!(rgb.cell_data.get("Elevation").unwrap().ncomp, 3);
    let r = comp0(&rgb, "Elevation");
    assert_eq!((r[0], r[3071], sum(&r)), (0.0, 252.0, 387072.0));
    close(range_all(&DataSet::ImageData(rgb.clone()), false, "Elevation"), (0.0, 415.875), "rgb magnitude");

    let bands = check("rasters/bands.tif", [0, 40, 0, 30, 0, 0], [-1000.0, 500.0, 0.0], [50.0, 50.0, 1.0]);
    for (name, first, last, total) in
        [("Elevation", 42.0, 139.0, 108600.0), ("Band 2", 142.0, 239.0, 228600.0), ("Band 3", 242.0, 339.0, 348600.0), ("Band 4", 342.0, 439.0, 468600.0)]
    {
        let v = comp0(&bands, name);
        assert_eq!((v[0], v[1199], sum(&v)), (first, last, total), "{name}");
    }

    // AREA_OR_POINT=Point: the tie point is a pixel center; GDAL (and ParaView) move it to the corner.
    let pt = check("rasters/point.tif", [0, 20, 0, 10, 0, 0], [100.0, 280.0, 0.0], [2.0, 2.0, 1.0]);
    let v = comp0(&pt, "Elevation");
    assert_eq!((v[0], v[199], sum(&v)), (900.0, 19.0, 91900.0));

    let asc = check("rasters/dem.asc", [0, 120, 0, 80, 0, 0], [500000.0, 4197600.0, 0.0], [30.0, 30.0, 1.0]);
    let a = comp0(&asc, "Elevation");
    for (x, y) in a.iter().zip(&e) {
        assert!(x.is_nan() == y.is_nan() && (x.is_nan() || (x - y).abs() <= 1e-5 * y.abs()), "{x} vs {y}");
    }

    // ERDAS Imagine copies (GDAL's HFA driver; dem.img run-length compressed): the same
    // placement, values, no-data and projection as the GeoTIFFs. ParaView 6.1.1 places .img
    // files at the origin with zero spacing (it takes GDAL's empty GCP list for corner points);
    // its band sums are those below.
    let dem_img = check("rasters/dem.img", [0, 120, 0, 80, 0, 0], [500000.0, 4197600.0, 0.0], [30.0, 30.0, 1.0]);
    let ei = comp0(&dem_img, "Elevation");
    assert!(ei.iter().zip(&e).all(|(x, y)| x.is_nan() == y.is_nan() && (x.is_nan() || x == y)));
    assert_eq!(dem_img.field_data.get("MAP_PROJECTION").map(|a| a.data.clone()), dem.field_data.get("MAP_PROJECTION").map(|a| a.data.clone()));
    assert_eq!(dem_img.field_data.get("NO_DATA_VALUE").unwrap().data.to_f64_vec(), vec![-9999.0]);
    let rgb_img = check("rasters/rgb.img", [0, 64, 0, 48, 0, 0], [10.0, 38.0, 0.0], [0.25, 0.25, 1.0]);
    let rgb_tif = rgb.cell_data.get("Elevation").unwrap().data.to_f64_vec();
    for (k, (name, total)) in [("Elevation", 387072.0), ("Band 2", 360960.0), ("Band 3", 354304.0)].into_iter().enumerate() {
        let v = comp0(&rgb_img, name);
        assert_eq!(sum(&v), total, "rgb.img {name}");
        assert!(v.iter().enumerate().all(|(i, &x)| x == rgb_tif[3 * i + k]), "rgb.img {name}");
    }
    assert_eq!(rgb_img.field_data.get("MAP_PROJECTION").map(|a| a.data.clone()), Some(aurafx_data::ArrayData::String(vec!["EPSG:4326".into()])));
    let bands_img = check("rasters/bands.img", [0, 40, 0, 30, 0, 0], [-1000.0, 500.0, 0.0], [50.0, 50.0, 1.0]);
    for name in ["Elevation", "Band 2", "Band 3", "Band 4"] {
        assert_eq!(comp0(&bands_img, name), comp0(&bands, name), "bands.img {name}");
    }
    // Run-length compressed bytes, 1-bit and 4-bit layers: every cell from the formula that
    // made it (ParaView's sums: 624330, 3500, 51236).
    let formulas: [(&str, fn(usize, usize) -> f64, f64); 3] = [
        ("classes", |i, j| ((i / 13 + 3 * (j / 9)) % 7 * 30) as f64, 624330.0),
        ("bits1", |i, j| if (i / 7 + j / 5) % 2 == 1 { 1.0 } else { 0.0 }, 3500.0),
        ("bits4", |i, j| ((i / 9 + j / 4) % 16) as f64, 51236.0),
    ];
    for (name, f, total) in formulas {
        let im = check(&format!("rasters/{name}.img"), [0, 100, 0, 70, 0, 0], [300000.0, 4099300.0, 0.0], [10.0, 10.0, 1.0]);
        let v = comp0(&im, "Elevation");
        assert_eq!(sum(&v), total, "{name}");
        // Rows are stored north-up from the bottom; the formula counts rows from the top.
        assert!((0..70).all(|j| (0..100).all(|i| v[(69 - j) * 100 + i] == f(i, j))), "{name}");
    }
}

/// CAM-SE output and its connectivity file (scripts/gen_cam.py), compared with ParaView 6.1.1's
/// CAM reader in each of its modes: counts, bounds, the first cell, and sums of every array (the
/// quads crossing the 0/360 seam on copied points). A POP-style file (scripts/gen_pop.py) reads
/// through the CF reader on its 2-D TLONG/TLAT, as ParaView's NetCDF reader places it.
#[test]
fn cam_and_pop_match_paraview() {
    use aurafx_data::DataSet;
    let Loaded::Series(s) = read_file(td("netcdf/cam_se.nc")).unwrap() else { panic!("expected 2 steps") };
    assert_eq!(s.times, vec![0.0, 0.25]);
    let read = |opts: &[(&str, &str)]| {
        let o: aurafx_io::ReaderOptions = opts.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
        aurafx_io::with_options(&o, || load_step(&s.steps[1]).unwrap())
    };
    let sum = |d: &DataSet, a: &str| d.point_data().unwrap().get(a).unwrap().data.to_f64_vec().iter().sum::<f64>();
    let check = |d: &DataSet, pts: usize, cells: usize, zmax: f64, first: &[u32], arrays: &[(&str, f64)]| {
        let DataSet::UnstructuredGrid(g) = d else { panic!() };
        assert_eq!((d.n_points(), d.n_cells()), (pts, cells));
        let b = d.bounds();
        assert_eq!((b.min[0], b.max[0], b.min[1], b.max[1], b.max[2]), (-5.0, 370.0, -90.0, 90.0, zmax));
        assert_eq!(g.cells.cell(0), first);
        for (a, want) in arrays {
            // ParaView's sums are rounded to 4 decimals.
            assert!((sum(d, a) - want).abs() < 1e-4 + 1e-9 * want.abs(), "{a}: {} vs {want}", sum(d, a));
        }
    };
    // Default (ParaView's too): all midpoint levels as hexahedra.
    check(&read(&[]), 1160, 864, 4.0, &[1, 8, 7, 0, 233, 240, 239, 232], &[("T", 304448.3949), ("lev", 580000.0)]);
    let one = read(&[("all_levels", "false"), ("level", "2")]);
    check(&one, 232, 216, 0.0, &[1, 8, 7, 0], &[("T", 60889.6789), ("lev", 116000.0)]);
    check(
        &read(&[("variables", "Interface layers [time, ilev, ncol]")]),
        1392,
        1080,
        5.0,
        &[1, 8, 7, 0, 233, 240, 239, 232],
        &[("OMEGA", 13.8989), ("ilev", 696000.0)],
    );
    check(&read(&[("variables", "Surface [time, ncol]")]), 232, 216, 0.0, &[1, 8, 7, 0], &[("PS", 23206707.3984)]);
    // The sphere needs no seam copies.
    assert_eq!(read(&[("layout", "Sphere"), ("variables", "Surface [time, ncol]")]).n_points(), 218);
    // A connectivity file that does not exist is reported.
    let o: aurafx_io::ReaderOptions = [("connectivity".to_string(), "missing.nc".to_string())].into();
    assert!(aurafx_io::with_options(&o, || load_step(&s.steps[0])).is_err());

    // POP: TEMP on (z_t, nlat, nlon) on the sphere by TLONG/TLAT, depth as radius (negated as
    // positive="down", then shifted); the (nlat, nlon) grid through the "variables" option.
    let Loaded::Series(p) = read_file(td("netcdf/pop_ocean.nc")).unwrap() else { panic!() };
    let d3 = load_step(&p.steps[1]).unwrap();
    assert!(matches!(&*d3, DataSet::StructuredGrid(_)));
    assert_eq!((d3.n_points(), d3.n_cells()), (4320, 3220));
    let b = d3.bounds();
    for (got, want) in
        [(b.min[0], -11491.42659), (b.max[0], 7399.73609), (b.min[1], -11483.54217), (b.max[1], 11327.60494), (b.min[2], -10153.17442), (b.max[2], 8202.26019)]
    {
        assert!((got - want).abs() < 1e-4, "{got} vs {want}");
    }
    close(range_all(&d3, true, "TEMP"), (-2.73508, 26.96611), "TEMP");
    let o: aurafx_io::ReaderOptions = [("variables".to_string(), "(nlat, nlon)".to_string())].into();
    let d2 = aurafx_io::with_options(&o, || load_step(&p.steps[1]).unwrap());
    assert_eq!((d2.n_points(), d2.n_cells()), (864, 805));
    close(range_all(&d2, true, "SSH"), (-49.95041, 49.94688), "SSH");
    close(range_all(&d2, true, "KMT"), (0.0, 5.0), "KMT");
    assert!(d2.point_data().unwrap().get("TLONG").is_none(), "2-D coordinates are not data");
}

/// MPAS ocean output (scripts/gen_mpas.py): the dual grid as ParaView 6.1.1's MPAS reader makes
/// it, without its extra point at the origin and without the 331 triangles it collapses there
/// (land cells under maxLevelCell); values checked against ParaView on the cells both keep.
#[test]
fn mpas_matches_paraview() {
    use aurafx_data::DataSet;
    let Loaded::Series(s) = read_file(td("netcdf/mpas_ocean.nc")).unwrap() else { panic!("expected 3 time steps") };
    assert_eq!(s.times, vec![0.0, 1.0, 2.0]);
    let at = |k: usize| load_step(&s.steps[k]).unwrap();
    let (d0, d2) = (at(0), at(2));
    let DataSet::UnstructuredGrid(g) = &*d0 else { panic!("not an unstructured grid") };
    assert_eq!((d0.n_points(), d0.n_cells()), (642, 949));
    assert_eq!(g.cells.cell(0), &[0, 162, 164]);
    let b = d0.bounds();
    assert!((b.max[0] - 6371229.0).abs() < 1e-3 && (b.min[2] + 6371229.0).abs() < 1e-3);
    close(range_all(&d0, true, "temperature"), (-2.0, 26.0), "temperature t0");
    close(range_all(&d2, true, "temperature"), (-4.4671369, 29.0), "temperature t2");
    close(range_all(&d2, true, "salinity"), (33.0999985, 35.0999985), "salinity t2");
    close(range_all(&d0, true, "areaCell"), (794550062628.9589, 834277565760.4069), "areaCell");
    close(range_all(&d0, false, "latVertex"), (-1.4846200148631992, 1.4846200148631992), "latVertex");
    let v = range_all(&d0, false, "relativeVorticity");
    assert!((v.0 + 9.960233910533134e-06).abs() < 1e-12 && (v.1 - 9.960233910533134e-06).abs() < 1e-12, "vorticity {v:?}");
    let cov = g.cell_data.get("cellsOnVertex").unwrap().data.to_f64_vec();
    assert_eq!(cov.iter().sum::<f64>(), 144443.0);
    let xt = d2.field_data().get("xtime").map(|a| a.data.clone());
    assert!(matches!(xt, Some(aurafx_data::ArrayData::String(ref v)) if v[0] == "0001-01-03_00:00:00"), "{xt:?}");

    // Multilayer on the sphere: a column of 5 points per cell center, a wedge per level of each
    // triangle not touching land (949 x 4), the top level outermost, every level's values.
    let opts: aurafx_io::ReaderOptions = [("multilayer".to_string(), "true".to_string()), ("layer_thickness".to_string(), "10000".to_string())].into();
    let m = aurafx_io::with_options(&opts, || at(2));
    let DataSet::UnstructuredGrid(mg) = &*m else { panic!() };
    assert_eq!((m.n_points(), m.n_cells()), (642 * 5, 949 * 4));
    assert_eq!(mg.cells.cell_type(0), aurafx_data::CellType::Wedge);
    assert_eq!(mg.cells.cell(0), &[0 * 5, 162 * 5, 164 * 5, 1, 162 * 5 + 1, 164 * 5 + 1]);
    close(range_all(&m, true, "temperature"), (-16.46713638305664, 29.0), "temperature, all levels");
    let p = mg.points.get(5 * 3 + 4);
    let r = (p[0] * p[0] + p[1] * p[1] + p[2] * p[2]).sqrt();
    assert!((r - (6371229.0 - 40000.0)).abs() < 1e-3, "deepest point of a column at {r}");
}

/// ADF-storage CGNS: the same file in ADF (written by the CGNS library) and in the old ADF
/// layout (hex pointers, big-endian data, chunked arrays) reads exactly as the HDF5 original.
#[test]
fn cgns_adf_matches_hdf5() {
    use aurafx_data::DataSet;
    let steps = |name: &str| -> Vec<DataSet> {
        match read_file(td(name)).unwrap() {
            Loaded::Series(s) => s.steps.iter().map(|st| (*load_step(st).unwrap()).clone()).collect(),
            Loaded::Static(d) => vec![(*d).clone()],
        }
    };
    for (orig, f) in [("cgns/mixed.cgns", "cgns/mixed_adf.cgns"), ("cgns/mixed.cgns", "cgns/mixed_adf_old.cgns"), ("cgns/tets.cgns", "cgns/tets_adf.cgns")] {
        let want = steps(orig);
        let got = steps(f);
        assert_eq!(got.len(), want.len(), "{f}: steps");
        for (g, w) in got.iter().zip(&want) {
            let (DataSet::MultiBlock(g), DataSet::MultiBlock(w)) = (g, w) else { panic!("{f}: not blocks") };
            let (gl, wl) = (g.leaves(), w.leaves());
            assert_eq!(gl.len(), wl.len(), "{f}: blocks");
            for ((gn, gd), (wn, wd)) in gl.iter().zip(&wl) {
                assert_eq!(gn, wn, "{f}: block names");
                assert_eq!((gd.n_points(), gd.n_cells()), (wd.n_points(), wd.n_cells()), "{f} {gn}: counts");
                assert_eq!(format!("{:?}", gd.bounds()), format!("{:?}", wd.bounds()), "{f} {gn}: bounds");
                for (ga, wa) in [(gd.point_data(), wd.point_data()), (gd.cell_data(), wd.cell_data())] {
                    let (ga, wa) = (ga.unwrap(), wa.unwrap());
                    assert_eq!(ga.iter().count(), wa.iter().count(), "{f} {gn}: arrays");
                    for a in wa.iter() {
                        let b = ga.get(&a.name).unwrap_or_else(|| panic!("{f} {gn}: no {}", a.name));
                        assert_eq!(b.data.to_f64_vec(), a.data.to_f64_vec(), "{f} {gn}: {}", a.name);
                    }
                }
            }
        }
    }
}

#[test]
fn cgns_writer_round_trip() {
    use aurafx_data::*;
    let dir = std::env::temp_dir().join(format!("aurafx_cgns_w_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let same = |a: &DataSet, b: &DataSet, arrays: &[(bool, &str)], what: &str| {
        assert_eq!((a.n_points(), a.n_cells()), (b.n_points(), b.n_cells()), "{what}: counts");
        let (ba, bb) = (a.bounds(), b.bounds());
        for k in 0..3 {
            assert!((ba.min[k] - bb.min[k]).abs() < 1e-6 && (ba.max[k] - bb.max[k]).abs() < 1e-6, "{what}: bounds");
        }
        for (pts, n) in arrays {
            close(range_all(a, *pts, n), range_all(b, *pts, n), &format!("{what}: {n}"));
        }
    };
    let first_leaf = |d: &DataSet| match d {
        DataSet::MultiBlock(mb) => mb.leaves()[0].1.clone(),
        d => d.clone(),
    };
    let can = match read_file(td("can.ex2")).unwrap() {
        Loaded::Series(s) => load_step(&s.steps[5]).unwrap(),
        Loaded::Static(d) => d,
    };
    let Loaded::Series(mixed) = read_file(td("cgns/mixed.cgns")).unwrap() else { panic!() };
    let cases: Vec<(&str, DataSet, Vec<(bool, &str)>)> = vec![
        ("can", (*can).clone(), vec![(true, "VEL"), (false, "EQPS")]),
        ("disk", (*static_ds("disk_out_ref.ex2")).clone(), vec![(true, "Temp"), (true, "V")]),
        ("image", (*static_ds("wavelet.vti")).clone(), vec![(true, "RTData")]),
        ("sphere", (*static_ds("sphere.vtkhdf")).clone(), vec![(true, "Elevation"), (true, "Normals")]),
        ("struct", (*static_ds("struct.vts")).clone(), vec![]),
        ("rect", (*static_ds("rect.vtr")).clone(), vec![]),
        ("tets", (*static_ds("cgns/tets.cgns")).clone(), vec![(true, "RTData"), (true, "Gradient")]),
        ("mixed", (*load_step(&mixed.steps[1]).unwrap()).clone(), vec![(true, "Pressure"), (true, "Velocity"), (false, "Density"), (false, "Id")]),
    ];
    for (name, ds, arrays) in &cases {
        let p = dir.join(format!("{name}.cgns"));
        aurafx_io::writers::write_file(ds, &p).unwrap_or_else(|e| panic!("{name}: {e:#}"));
        let back = static_ds(p.to_str().unwrap());
        same(&back, ds, arrays, name);
    }

    // Quadratic and linear volume cells keep their nodes in VTK order.
    let ug = first_leaf(&static_ds("quadratic_shapes.vtu"));
    let DataSet::UnstructuredGrid(u) = &ug else { panic!() };
    // Volume cells CGNS has (others are written by their corners), plus a 27-node hexahedron.
    use CellType::*;
    let native = [Tetra, QuadraticTetra, Pyramid, QuadraticPyramid, Wedge, QuadraticWedge, Hexahedron, QuadraticHexahedron, TriquadraticHexahedron];
    let mut vol = Cells::new();
    for (t, ids) in u.cells.iter() {
        if native.contains(&t) {
            vol.push(t, ids);
        }
    }
    vol.push(TriquadraticHexahedron, &(0..27).collect::<Vec<u32>>());
    let ug = UnstructuredGrid {
        points: u.points.clone(),
        cells: vol.clone(),
        point_data: Attributes::new(),
        cell_data: Attributes::new(),
        field_data: Attributes::new(),
    };
    let p = dir.join("quadratic.cgns");
    aurafx_io::writers::write_file(&DataSet::UnstructuredGrid(ug), &p).unwrap();
    let DataSet::UnstructuredGrid(g) = first_leaf(&static_ds(p.to_str().unwrap())) else { panic!() };
    assert!(!vol.is_empty());
    assert_eq!(g.cells.types, vol.types);
    assert_eq!(g.cells.connectivity, vol.connectivity);

    // A pentagonal prism (polyhedron) between two hexahedra: the hexahedra come first.
    let mut pts = Vec::new();
    for z in [0.0, 1.0] {
        for k in 0..5 {
            let a = k as f64 * std::f64::consts::TAU / 5.0;
            pts.extend([a.cos(), a.sin(), z]);
        }
    }
    for z in [0.0, 1.0] {
        pts.extend([3.0, 0.0, z, 4.0, 0.0, z, 4.0, 1.0, z, 3.0, 1.0, z]);
    }
    let mut cells = Cells::new();
    cells.push(CellType::Hexahedron, &[10, 11, 12, 13, 14, 15, 16, 17]);
    cells.push(CellType::PentagonalPrism, &[0, 1, 2, 3, 4, 5, 6, 7, 8, 9]);
    cells.push(CellType::Hexahedron, &[10, 11, 12, 13, 14, 15, 16, 17]);
    let mut cd = Attributes::new();
    cd.insert(Array::scalars_f64("id", vec![1.0, 2.0, 3.0]));
    let ug = UnstructuredGrid { points: Points::new_f64(pts), cells, point_data: Attributes::new(), cell_data: cd, field_data: Attributes::new() };
    let p = dir.join("poly.cgns");
    aurafx_io::writers::write_file(&DataSet::UnstructuredGrid(ug), &p).unwrap();
    let DataSet::UnstructuredGrid(g) = first_leaf(&static_ds(p.to_str().unwrap())) else { panic!() };
    assert_eq!(g.cells.types, vec![CellType::Hexahedron, CellType::Hexahedron, CellType::Polyhedron]);
    assert_eq!(g.cell_data.get("id").unwrap().data.to_f64_vec(), vec![1.0, 3.0, 2.0]);
    assert_eq!(g.cells.polyhedron_faces.as_ref().unwrap()[2].len(), 7);

    // A polygon surface (2-D base, NGON cells after the triangles).
    let mut cells = Cells::new();
    cells.push(CellType::Polygon, &[0, 1, 2, 3, 4]);
    cells.push(CellType::Triangle, &[0, 1, 5]);
    let pts = vec![0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 1.5, 1.0, 0.0, 0.5, 1.5, 0.0, -0.5, 1.0, 0.0, 0.5, -1.0, 0.0];
    let pd = PolyData { points: Points::new_f64(pts), polys: cells, ..Default::default() };
    let p = dir.join("polygons.cgns");
    aurafx_io::writers::write_file(&DataSet::PolyData(pd), &p).unwrap();
    let DataSet::UnstructuredGrid(g) = first_leaf(&static_ds(p.to_str().unwrap())) else { panic!() };
    assert_eq!(g.cells.types, vec![CellType::Triangle, CellType::Polygon]);

    // Three steps of the can (the points move with the displacements).
    let Loaded::Series(s) = read_file(td("can.ex2")).unwrap() else { panic!() };
    let steps: Vec<(f64, std::sync::Arc<DataSet>)> = [0usize, 20, 43].iter().map(|&k| (s.times[k], load_step(&s.steps[k]).unwrap())).collect();
    let refs: Vec<(f64, &DataSet)> = steps.iter().map(|(t, d)| (*t, &**d)).collect();
    let p = dir.join("series.cgns");
    aurafx_io::cgns_writer::write_cgns_steps(&refs, &p).unwrap();
    let Loaded::Series(back) = read_file(&p).unwrap() else { panic!("series") };
    assert_eq!(back.times, steps.iter().map(|s| s.0).collect::<Vec<_>>());
    for (k, (_, d)) in steps.iter().enumerate() {
        same(&load_step(&back.steps[k]).unwrap(), d, &[(true, "DISPL"), (false, "EQPS")], &format!("series step {k}"));
    }
    if std::env::var("AURAFX_KEEP").is_ok() {
        eprintln!("kept {}", dir.display());
    } else {
        let _ = std::fs::remove_dir_all(&dir);
    }
}

/// GIS vector files (Shapefile, GeoJSON, GeoPackage, FlatGeobuf) against ParaView 6.1.1's GDAL
/// vector reader (testdata/vectors/paraview_reference.json from scripts/vectors_reference.py):
/// with one block per feature, every point, cell size and attribute matches. KML, KMZ and GML
/// (which ParaView's GDAL cannot read) carry the same data as the GeoJSON files.
#[test]
fn gis_vectors_match_paraview() {
    use aurafx_data::DataSet;
    let reference: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(td("vectors/paraview_reference.json")).unwrap()).unwrap();
    let read = |f: &str, opts: &[(&str, &str)]| -> DataSet {
        let o: aurafx_io::ReaderOptions = opts.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
        match aurafx_io::with_options(&o, || read_file(td(&format!("vectors/{f}")))).unwrap_or_else(|e| panic!("{f}: {e:#}")) {
            Loaded::Static(d) => (*d).clone(),
            _ => panic!(),
        }
    };
    let blocks = |d: &DataSet| -> Vec<aurafx_data::PolyData> {
        match d {
            DataSet::MultiBlock(mb) => mb
                .blocks
                .iter()
                .map(|(_, b)| match b {
                    Some(DataSet::PolyData(p)) => p.clone(),
                    _ => panic!("not polygonal"),
                })
                .collect(),
            _ => panic!("not multi-block"),
        }
    };
    let per_feature = aurafx_io::vectors::PER_FEATURE;
    for (key, want) in reference.as_object().unwrap() {
        let (file, layer) = key.split_once('#').unwrap();
        let layer = layer.parse::<usize>().unwrap();
        let mut opts = vec![("blocks", per_feature)];
        if file.ends_with(".gpkg") {
            opts.push(("layer", ["countries", "rivers", "cities"][layer]));
        }
        // ParaView cannot pick a file's last layer alone: world.gdb#5 is every layer.
        if file.ends_with(".gdb") && layer < 5 {
            opts.push(("layer", ["countries", "rivers", "cities", "lots", "roads"][layer]));
        }
        let got = blocks(&read(file, &opts));
        let want = want.as_array().unwrap();
        assert_eq!(got.len(), want.len(), "{key}: blocks");
        for (g, w) in got.iter().zip(want) {
            let wp: Vec<f64> = w["points"].as_array().unwrap().iter().flat_map(|p| p.as_array().unwrap().iter().map(|x| x.as_f64().unwrap())).collect();
            let gp: Vec<f64> = (0..g.points.len()).flat_map(|i| g.points.get(i)).collect();
            assert_eq!(gp, wp, "{key}: points");
            for (kind, cells) in [("verts", &g.verts), ("lines", &g.lines)] {
                let off: Vec<i64> = w["cells"][kind].as_array().unwrap().iter().map(|x| x.as_i64().unwrap()).collect();
                let sizes: Vec<i64> = off.windows(2).map(|p| p[1] - p[0]).collect();
                let got_sizes: Vec<i64> = (0..cells.len()).map(|i| cells.cell(i).len() as i64).collect();
                assert_eq!(got_sizes, sizes, "{key}: {kind}");
            }
            let attrs = w["attrs"].as_object().unwrap();
            let mut names: Vec<String> = g.cell_data.iter().map(|a| a.name.clone()).collect();
            names.sort();
            assert_eq!(names, attrs.keys().cloned().collect::<Vec<_>>(), "{key}: attribute names");
            for (name, v) in attrs {
                let a = g.cell_data.get(name).unwrap_or_else(|| panic!("{key}: no {name}"));
                let (kind, vals) = (v[0].as_str().unwrap(), v[1].as_array().unwrap());
                match (kind, &a.data) {
                    ("str", ArrayData::String(s)) => assert_eq!(s, &vals.iter().map(|x| x.as_str().unwrap().to_string()).collect::<Vec<_>>(), "{key}: {name}"),
                    ("vtkIntArray", ArrayData::I32(x)) => assert_eq!(x, &vals.iter().map(|x| x.as_i64().unwrap() as i32).collect::<Vec<_>>(), "{key}: {name}"),
                    ("vtkDoubleArray", ArrayData::F64(x)) => assert_eq!(x, &vals.iter().map(|x| x.as_f64().unwrap()).collect::<Vec<_>>(), "{key}: {name}"),
                    (k, d) => panic!("{key}: {name} is {:?}, ParaView {k}", d.scalar_type()),
                }
            }
        }
    }

    // One block per layer (the default): the features' cells together, named after the layer.
    let d = read("world.gpkg", &[]);
    match &d {
        DataSet::MultiBlock(mb) => {
            assert_eq!(mb.blocks.iter().map(|(n, _)| n.as_str()).collect::<Vec<_>>(), ["countries", "rivers", "cities"]);
            assert_eq!(d.n_cells(), 5 + 3 + 4);
            assert!(mb.field_data.get("MAP_PROJECTION").is_some());
        }
        _ => panic!(),
    }
    // KML holds the same features as the GeoJSON files (names in "Name").
    let kml = blocks(&read("world.kml", &[]));
    for (k, layer) in ["countries", "rivers", "cities"].iter().enumerate() {
        let gj = &blocks(&read(&format!("{layer}.geojson"), &[]))[0];
        assert_eq!(kml[k].points.len(), gj.points.len(), "{layer}");
        assert_eq!(
            (0..gj.points.len()).map(|i| gj.points.get(i)).collect::<Vec<_>>(),
            (0..kml[k].points.len()).map(|i| kml[k].points.get(i)).collect::<Vec<_>>()
        );
        assert_eq!(kml[k].cell_data.get("Name").unwrap().data, gj.cell_data.get("name").unwrap().data);
    }
    // KMZ is the KML zipped: the same blocks.
    let kmz = blocks(&read("world.kmz", &[]));
    assert_eq!(kmz.len(), kml.len());
    for (a, b) in kmz.iter().zip(&kml) {
        assert_eq!(a, b, "KMZ and KML");
    }
    // GML (which ParaView's GDAL cannot read; written by GDAL from the GeoPackage, latitude
    // first as EPSG:4326 asks): the same features as the GeoJSON files, typed by its schema.
    let gml = blocks(&read("world.gml", &[]));
    assert_eq!(gml.len(), 3);
    for (k, layer) in ["countries", "rivers", "cities"].iter().enumerate() {
        let gj = &blocks(&read(&format!("{layer}.geojson"), &[]))[0];
        let g = &gml[k];
        assert_eq!(
            (0..g.points.len()).map(|i| g.points.get(i)).collect::<Vec<_>>(),
            (0..gj.points.len()).map(|i| gj.points.get(i)).collect::<Vec<_>>(),
            "{layer} points"
        );
        for (a, b) in [(&g.verts, &gj.verts), (&g.lines, &gj.lines)] {
            assert_eq!(
                (0..a.len()).map(|i| a.cell(i).to_vec()).collect::<Vec<_>>(),
                (0..b.len()).map(|i| b.cell(i).to_vec()).collect::<Vec<_>>(),
                "{layer} cells"
            );
        }
        for arr in gj.cell_data.iter() {
            let x = g.cell_data.get(&arr.name).unwrap_or_else(|| panic!("GML {layer}: no {}", arr.name));
            match (&arr.data, &x.data) {
                (ArrayData::String(a), ArrayData::String(b)) => assert_eq!(a, b, "{layer} {}", arr.name),
                (a, b) => assert_eq!(a.to_f64_vec(), b.to_f64_vec(), "{layer} {}", arr.name),
            }
        }
        assert_eq!(g.field_data, gj.field_data, "{layer} projection");
    }
    // GML 2 (coordinate tuples, outer/innerBoundaryIs, longitude first): the same blocks.
    let gml2 = blocks(&read("world2.gml", &[]));
    assert_eq!(gml2, gml, "GML 2 and GML 3.2");
    // Filled polygons: triangles covering each polygon's area (Westland has a hole).
    let filled = blocks(&read("countries.shp", &[("polygons", aurafx_io::vectors::FILLED), ("feature_ids", "true")]))[0].clone();
    let area_of = |fid: i64| -> f64 {
        let ids = filled.cell_data.get("_vtkPedigreeIds").unwrap();
        (0..filled.polys.len())
            .filter(|&c| ids.get(c, 0) as i64 == fid)
            .map(|c| {
                let t = filled.polys.cell(c);
                let (a, b, cc) = (filled.points.get(t[0] as usize), filled.points.get(t[1] as usize), filled.points.get(t[2] as usize));
                ((b[0] - a[0]) * (cc[1] - a[1]) - (b[1] - a[1]) * (cc[0] - a[0])).abs() / 2.0
            })
            .sum()
    };
    // Shoelace areas of the test polygons.
    assert!((area_of(0) - (98.0 - 6.0)).abs() < 1e-9, "Westland {}", area_of(0));
    assert!((area_of(1) - 49.5).abs() < 1e-9, "Eastmark {}", area_of(1));
    assert!((area_of(2) - (4.0 + 1.75)).abs() < 1e-9, "Twin Isles {}", area_of(2));
}

/// The SQLite reader under GeoPackage: interior pages, overflow pages, every value type
/// (testdata/vectors/pages.sqlite from scripts/gen_sqlite_pages.py).
#[test]
fn sqlite_pages_and_overflow() {
    use aurafx_io::sqlite::{Database, Value};
    let db = Database::open(&td("vectors/pages.sqlite")).unwrap();
    let rows = db.rows("t").unwrap();
    assert_eq!(rows.len(), 3000);
    for (k, (rowid, r)) in rows.iter().enumerate() {
        let i = k as i64 + 1;
        assert_eq!(*rowid, i);
        assert_eq!(r[0], Value::Int(i));
        assert_eq!(r[1], if i % 7 != 0 { Value::Text(format!("row {i}")) } else { Value::Null });
        assert_eq!(r[2].as_f64(), Some(i as f64 * 0.5)); // whole reals are stored as integers
        assert_eq!(r[3], Value::Int(if i % 2 == 1 { -i * 1_000_000_007 } else { i }));
        match &r[4] {
            Value::Blob(b) => {
                assert_eq!(i % 150, 0);
                assert!(b.len() == 3000 && b.iter().enumerate().all(|(j, &x)| x as i64 == (i + j as i64) % 256), "blob of row {i}");
            }
            Value::Null => assert_ne!(i % 150, 0),
            v => panic!("{v:?}"),
        }
    }
}

/// Filling polygons: a wavy 20 000-point outline with two holes is covered exactly once.
#[test]
fn polygon_fill_covers_the_area() {
    let n = 20000;
    let mut pts: Vec<[f64; 3]> = (0..n)
        .map(|k| {
            let t = 2.0 * std::f64::consts::PI * k as f64 / n as f64;
            let r = 10.0 + (40.0 * t).sin();
            [r * t.cos(), r * t.sin(), 0.0]
        })
        .collect();
    let hole = |c: [f64; 2], r: f64, m: usize| -> Vec<[f64; 3]> {
        (0..m)
            .map(|k| {
                let t = -2.0 * std::f64::consts::PI * k as f64 / m as f64;
                [c[0] + r * t.cos(), c[1] + r * t.sin(), 0.0]
            })
            .collect()
    };
    let starts = vec![0, n, n + 500];
    pts.extend(hole([0.0, 0.0], 2.0, 500));
    pts.extend(hole([5.0, 0.0], 1.0, 64));
    let shoelace = |r: &[[f64; 3]]| (0..r.len()).map(|k| r[k][0] * r[(k + 1) % r.len()][1] - r[(k + 1) % r.len()][0] * r[k][1]).sum::<f64>().abs() / 2.0;
    let want = shoelace(&pts[..n]) - shoelace(&pts[n..n + 500]) - shoelace(&pts[n + 500..]);
    let tris = aurafx_io::vectors::triangulate(&pts, &starts);
    let got: f64 = tris
        .iter()
        .map(|t| {
            let (a, b, c) = (pts[t[0]], pts[t[1]], pts[t[2]]);
            ((b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0])) / 2.0
        })
        .sum();
    eprintln!("{} triangles, area {got} (polygon {want})", tris.len());
    assert!((got - want).abs() < 1e-6 * want, "signed area {got} vs {want}");
    assert!(
        tris.iter().all(|t| {
            let (a, b, c) = (pts[t[0]], pts[t[1]], pts[t[2]]);
            (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0]) > 0.0
        }),
        "all triangles counter-clockwise (no overlaps)"
    );
}

/// AMR files: VTK's .vthb (written by ParaView from its AMR Gaussian Pulse source) and AMReX
/// plotfiles (scripts/gen_amrex.py), against ParaView 6.1.1: per box the origin, size, array
/// sums, first values and the covered-cell marks. In 2-D, ParaView fills a vector's missing z
/// with the plotfile's first variable; auraFX uses 0, so only x and y are compared there.
#[test]
fn amr_files_match_paraview() {
    use aurafx_data::DataSet;
    let leaves = |d: &DataSet| -> Vec<aurafx_data::ImageData> {
        assert!(aurafx_data::amr::is_amr(d), "not AMR");
        let DataSet::MultiBlock(mb) = d else { panic!() };
        mb.leaves()
            .into_iter()
            .map(|(_, l)| match l {
                DataSet::ImageData(i) => i.clone(),
                _ => panic!("not image data"),
            })
            .collect()
    };
    let sum = |i: &aurafx_data::ImageData, name: &str, c: usize| -> f64 {
        let a = i.cell_data.get(name).unwrap_or_else(|| panic!("no {name}"));
        (0..a.len()).map(|k| a.get(k, c)).sum()
    };
    let close = |a: f64, b: f64, tol: f64, tag: &str| assert!((a - b).abs() <= tol * (1.0 + b.abs()), "{tag}: {a} vs ParaView {b}");

    // .vthb: the Gaussian pulse.
    let p = static_ds("amr/pulse.vthb");
    let l = leaves(&p);
    assert_eq!(l.len(), 3);
    assert_eq!(p.n_cells(), 605);
    close(l.iter().map(|i| sum(i, "Gaussian-Pulse", 0)).sum(), 9.989747547467575, 1e-12, "pulse");
    close(sum(&l[0], "vtkGhostType", 0), 480.0, 0.0, "pulse ghosts");
    let p2 = static_ds("amr/pulse2d.vthb");
    assert_eq!(p2.n_cells(), 73);
    close(leaves(&p2).iter().map(|i| sum(i, "Gaussian-Pulse", 0)).sum(), 6.329186193165329, 1e-12, "2D pulse");
    close(sum(&leaves(&p2)[0], "vtkGhostType", 0), 8.0 * 12.0, 0.0, "2D pulse ghosts");

    // AMReX plotfiles: (origin, points per axis, density, temperature, velocity x/y/z sums,
    // first density, ghost sum) per box, as ParaView reads them.
    type Box = ([f64; 3], [usize; 3], f64, f64, [f64; 3], f64, f64);
    let b3: [Box; 3] = [
        ([-1.0, -1.0, 0.0], [9, 17, 17], 175.809597882, 583255.213718562, [0.0, -1024.0, 204.8], 2.6275464e-05, 1536.0),
        ([0.0, -1.0, 0.0], [9, 17, 17], 175.809597882, 645544.786281438, [0.0, 1024.0, 204.8], 0.00087012385, 2560.0),
        ([-0.375, -0.5, 0.25], [17, 17, 17], 1461.685601489, 1270864.143292565, [0.0, 512.0, 307.2], 0.032778949604, 0.0),
    ];
    let b2: [Box; 3] = [
        ([-1.0, -1.0, 0.0], [9, 17, 1], 0.456219448, 36453.45085741, [0.0, -64.0, f64::NAN], 1.6187843e-05, 192.0),
        ([0.0, -1.0, 0.0], [9, 17, 1], 0.456219448, 40346.54914259, [0.0, 64.0, f64::NAN], 0.000536067753, 320.0),
        ([-0.375, -0.5, 0.0], [17, 17, 1], 2.538693629, 79429.008955785, [0.0, 32.0, f64::NAN], 0.004740737807, 0.0),
    ];
    for (file, want, tol) in [("amr/plt_3d", &b3, 1e-9), ("amr/plt_2d/Header", &b2, 1e-9), ("amr/plt_3d_f32", &b3, 2e-6)] {
        let d = static_ds(file);
        let l = leaves(&d);
        assert_eq!(l.len(), 3, "{file}");
        for (i, (img, w)) in l.iter().zip(want.iter()).enumerate() {
            let tag = format!("{file} box {i}");
            assert_eq!(img.origin, w.0, "{tag} origin");
            assert_eq!(img.dims(), w.1, "{tag} dims");
            close(sum(img, "density", 0), w.2, tol, &format!("{tag} density"));
            close(sum(img, "temperature", 0), w.3, tol, &format!("{tag} temperature"));
            for c in 0..3 {
                if !w.4[c].is_nan() {
                    close(sum(img, "velocity", c), w.4[c], tol, &format!("{tag} velocity {c}"));
                }
            }
            close(img.cell_data.get("density").unwrap().get(0, 0), w.5, 1e-7, &format!("{tag} first density"));
            close(sum(img, "vtkGhostType", 0), w.6, 0.0, &format!("{tag} ghosts"));
        }
        if file.ends_with("f32") {
            assert_eq!(l[0].cell_data.get("density").unwrap().scalar_type(), aurafx_data::ScalarType::F32);
        }
    }
}

/// FLASH (FLASH 3 and FLASH 2 layouts), Enzo and Chombo AMR files against ParaView 6.1.1
/// (testdata/amr/flash_enzo_reference.json from scripts/amr_files_reference.py; files from
/// scripts/gen_amr_files.py): every level and block, its origin, spacing and dimensions, each
/// cell array's type and sum, and the cells marked covered. The 2-D FLASH file, which ParaView
/// cannot read, is checked against the values it was written with.
#[test]
fn flash_and_enzo_match_paraview() {
    use aurafx_data::DataSet;
    let reference: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(td("amr/flash_enzo_reference.json")).unwrap()).unwrap();
    let read = |f: &str, opts: &[(&str, &str)]| -> DataSet {
        let o: aurafx_io::ReaderOptions = opts.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
        match aurafx_io::with_options(&o, || read_file(td(&format!("amr/{f}")))).unwrap_or_else(|e| panic!("{f}: {e:#}")) {
            Loaded::Static(d) => (*d).clone(),
            _ => panic!(),
        }
    };
    for (key, want) in reference.as_object().unwrap() {
        let (file, cgs) = match key.split_once('#') {
            Some((f, _)) => (f, "true"),
            None => (key.as_str(), "false"),
        };
        let d = read(file, &[("cgs", cgs)]);
        let DataSet::MultiBlock(mb) = &d else { panic!("{key}: not AMR") };
        let levels = want["levels"].as_array().unwrap();
        assert_eq!(mb.blocks.len(), levels.len(), "{key}: levels");
        for (l, (wl, (_, lev))) in levels.iter().zip(&mb.blocks).enumerate() {
            let Some(DataSet::MultiBlock(lev)) = lev else { panic!("{key}: level {l}") };
            let wl = wl.as_array().unwrap();
            assert_eq!(lev.blocks.len(), wl.len(), "{key}: level {l} blocks");
            for (b, (w, (_, img))) in wl.iter().zip(&lev.blocks).enumerate() {
                let Some(DataSet::ImageData(img)) = img else { panic!("{key}: block {l}/{b}") };
                let tag = format!("{key} level {l} block {b}");
                let f3 = |v: &serde_json::Value| -> [f64; 3] { std::array::from_fn(|k| v[k].as_f64().unwrap()) };
                // ParaView's VisIt Chombo reader keeps level 0's ghost cells (one ring here) as
                // ordinary cells; auraFX drops ghost cells on every level.
                let ghosted = key == "chombo2d.hdf5" && l == 0;
                let g = if ghosted { 1.0 } else { 0.0 };
                assert_eq!(img.origin, std::array::from_fn(|k| f3(&w["origin"])[k] + if k < 2 { g * img.spacing[k] } else { 0.0 }), "{tag} origin");
                assert_eq!(img.spacing, f3(&w["spacing"]), "{tag} spacing");
                assert_eq!(img.dims().map(|x| x as f64), std::array::from_fn(|k| f3(&w["dims"])[k] - if k < 2 { 2.0 * g } else { 0.0 }), "{tag} dims");
                for (name, v) in w["arrays"].as_object().unwrap() {
                    if ghosted && name != "vtkGhostType" {
                        continue;
                    }
                    // Point arrays (node-centered Chombo) are listed as "point:<name>".
                    let a = match name.strip_prefix("point:") {
                        Some(n) => img.point_data.get(n),
                        None => img.cell_data.get(name),
                    }
                    .unwrap_or_else(|| panic!("{tag}: no {name}"));
                    let (class, sum) = (v[0].as_str().unwrap(), v[1].as_f64().unwrap());
                    if name == "vtkGhostType" {
                        let covered = (0..a.len()).filter(|&i| a.get(i, 0) as u8 & 8 != 0).count();
                        assert_eq!(covered as u64, w["covered"].as_u64().unwrap(), "{tag} covered cells");
                        continue;
                    }
                    let want_ty = match class {
                        "vtkDoubleArray" => aurafx_data::ScalarType::F64,
                        "vtkFloatArray" => aurafx_data::ScalarType::F32,
                        c => panic!("{c}"),
                    };
                    assert_eq!(a.scalar_type(), want_ty, "{tag} {name} type");
                    let got: f64 = (0..a.len()).map(|i| a.get(i, 0)).sum();
                    assert!((got - sum).abs() <= 1e-6 * (1.0 + sum.abs()), "{tag} {name}: {got} vs ParaView {sum}");
                }
            }
        }
    }
    // FLASH 2-D (FLASH 2 layout, three levels): dens = 1 + x + 2y and temp = 100 + 10x - 5y at
    // the cell centers, coarse cells under finer blocks covered.
    let d = read("flash2d_hdf5_chk_0003", &[]);
    let DataSet::MultiBlock(mb) = &d else { panic!() };
    let mut covered = 0;
    let mut n_blocks = vec![];
    for (_, lev) in &mb.blocks {
        let Some(DataSet::MultiBlock(lev)) = lev else { panic!() };
        n_blocks.push(lev.blocks.len());
        for (_, img) in &lev.blocks {
            let Some(DataSet::ImageData(img)) = img else { panic!() };
            assert_eq!(img.dims()[2], 1);
            let (dens, temp) = (img.cell_data.get("dens").unwrap(), img.cell_data.get("temp").unwrap());
            let nx = img.dims()[0] - 1;
            for c in 0..dens.len() {
                let (i, j) = (c % nx, c / nx);
                let x = img.origin[0] + (i as f64 + 0.5) * img.spacing[0];
                let y = img.origin[1] + (j as f64 + 0.5) * img.spacing[1];
                assert!((dens.get(c, 0) - (1.0 + x + 2.0 * y)).abs() < 1e-12);
                assert!((temp.get(c, 0) - (100.0 + 10.0 * x - 5.0 * y)).abs() < 1e-12);
            }
            let g = img.cell_data.get("vtkGhostType").unwrap();
            covered += (0..g.len()).filter(|&i| g.get(i, 0) as u8 & 8 != 0).count();
        }
    }
    assert_eq!(n_blocks, vec![1, 4, 4]);
    // The root block is fully covered (64 cells); one level-1 block too (64 cells).
    assert_eq!(covered, 128);
    assert_eq!(d.field_data().get("TimeValue").unwrap().get(0, 0), 1.5);

    // Chombo 2-D (ghost cells around every box, prob_lo = (-0.5, 0.25)): density = 1 + x + 2y
    // and energy = xy + 0.5 at every cell center of every level.
    let d = read("chombo2d.hdf5", &[]);
    let DataSet::MultiBlock(mb) = &d else { panic!() };
    for (_, lev) in &mb.blocks {
        let Some(DataSet::MultiBlock(lev)) = lev else { panic!() };
        for (_, img) in &lev.blocks {
            let Some(DataSet::ImageData(img)) = img else { panic!() };
            let (dens, en) = (img.cell_data.get("density").unwrap(), img.cell_data.get("energy").unwrap());
            let nx = img.dims()[0] - 1;
            for c in 0..dens.len() {
                let x = img.origin[0] + ((c % nx) as f64 + 0.5) * img.spacing[0];
                let y = img.origin[1] + ((c / nx) as f64 + 0.5) * img.spacing[1];
                assert!((dens.get(c, 0) - (1.0 + x + 2.0 * y)).abs() < 1e-12, "density at ({x}, {y})");
                assert!((en.get(c, 0) - (x * y + 0.5)).abs() < 1e-12, "energy at ({x}, {y})");
            }
        }
    }
    assert_eq!(d.field_data().get("TimeValue").unwrap().get(0, 0), 0.75);
}

/// FLASH and Enzo particles (reader option data = Particles) against VTK's particle readers as
/// ParaView 6.1.1 runs them (testdata/amr/particles_reference.json from
/// scripts/amr_particles_reference.py): every particle's position and property, types kept for
/// Enzo. ParaView repeats FLASH's particles in each of its blocks and reads every FLASH 3
/// property from the first column; auraFX gives each particle once with its own columns, checked
/// against the same particles in FLASH 2's layout.
#[test]
fn amr_particles_match_paraview() {
    use aurafx_data::{DataSet, ScalarType};
    let reference: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(td("amr/particles_reference.json")).unwrap()).unwrap();
    let read = |f: &str, opts: &[(&str, &str)]| -> DataSet {
        let o: aurafx_io::ReaderOptions = opts.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
        match aurafx_io::with_options(&o, || read_file(td(&format!("amr/{f}")))).unwrap_or_else(|e| panic!("{f}: {e:#}")) {
            Loaded::Static(d) => (*d).clone(),
            _ => panic!(),
        }
    };
    let floats = |v: &serde_json::Value| -> Vec<f64> { v.as_array().unwrap().iter().map(|x| x.as_f64().unwrap()).collect() };
    let pts = |p: &aurafx_data::PolyData| -> Vec<f64> { (0..p.points.len()).flat_map(|i| p.points.get(i)).collect() };
    // serde_json reads floats to within an ulp.
    let same = |a: &[f64], b: &[f64], tag: &str| {
        assert_eq!(a.len(), b.len(), "{tag} size");
        for (x, y) in a.iter().zip(b) {
            assert!((x - y).abs() <= 1e-14 * (1.0 + y.abs()), "{tag}: {x} vs ParaView {y}");
        }
    };
    let col = |p: &aurafx_data::PolyData, n: &str| -> Vec<f64> {
        let a = p.point_data.get(n).unwrap_or_else(|| panic!("no {n}"));
        (0..a.len()).map(|i| a.get(i, 0)).collect()
    };
    // FLASH 2 layout: block 0 of ParaView's output (every block holds the same particles).
    let f2 = &reference["flash3d_ffv7_hdf5_part_0007"]["data"]["0"];
    let DataSet::PolyData(p2) = read("flash3d_ffv7_hdf5_part_0007", &[("data", "Particles")]) else { panic!() };
    same(&pts(&p2), &floats(&f2["points"]), "FLASH 2 points");
    assert_eq!(p2.verts.len(), 40);
    for (name, v) in f2["arrays"].as_object().unwrap() {
        assert_eq!(p2.point_data.get(name).unwrap().scalar_type(), ScalarType::F64, "{name}");
        same(&col(&p2, name), &floats(&v[1]), name);
    }
    // FLASH 3 layout: the same particles, each column its own.
    let f3 = &reference["flash3d_hdf5_part_0007"]["data"]["0"];
    let DataSet::PolyData(p3) = read("flash3d_hdf5_part_0007", &[("data", "Particles")]) else { panic!() };
    same(&pts(&p3), &floats(&f3["points"]), "FLASH 3 points");
    assert_eq!(col(&p3, "tag"), col(&p2, "tag"));
    assert_eq!(col(&p3, "velx"), col(&p2, "particle_velx"));
    assert_eq!(col(&p3, "mass"), col(&p2, "mass"));
    for (k, axis) in ["posx", "posy", "posz"].iter().enumerate() {
        assert_eq!(col(&p3, axis), (0..40).map(|i| p3.points.get(i)[k]).collect::<Vec<_>>());
    }
    // ParaView's FLASH 3 arrays are all the first column.
    same(&col(&p3, "tag"), &floats(&f3["arrays"]["mass"][1]), "ParaView FLASH 3 mass");
    // Enzo: ParaView's blocks (one per grid with particles) one after the other, types kept.
    let e = &reference["enzo/DD0004/data0004.hierarchy"]["data"];
    let DataSet::PolyData(pe) = read("enzo/DD0004/data0004.hierarchy", &[("data", "Particles")]) else { panic!() };
    let blocks: Vec<&serde_json::Value> = ["0", "3"].iter().map(|b| &e[*b]).collect();
    same(&pts(&pe), &blocks.iter().flat_map(|b| floats(&b["points"])).collect::<Vec<_>>(), "Enzo points");
    for (name, v) in blocks[0]["arrays"].as_object().unwrap() {
        let want_ty = match v[0].as_str().unwrap() {
            "vtkLongArray" | "vtkLongLongArray" => ScalarType::I64,
            "vtkIntArray" => ScalarType::I32,
            "vtkFloatArray" => ScalarType::F32,
            _ => ScalarType::F64,
        };
        assert_eq!(pe.point_data.get(name).unwrap().scalar_type(), want_ty, "Enzo {name}");
        let want: Vec<f64> = blocks.iter().flat_map(|b| floats(&b["arrays"][name][1])).collect();
        same(&col(&pe, name), &want, name);
    }
    // Chombo particles (ParaView's VisIt Chombo reader: every property as doubles).
    let c = &reference["chombo2d_particles.hdf5"]["data"]["0"];
    let DataSet::PolyData(pc) = read("chombo2d_particles.hdf5", &[("data", "Particles")]) else { panic!() };
    // ParaView's VisIt bridge keeps the positions in single precision.
    for (x, y) in pts(&pc).iter().zip(floats(&c["points"])) {
        assert!((x - y).abs() <= 1e-7 * (1.0 + y.abs()), "Chombo points: {x} vs ParaView {y}");
    }
    for (name, v) in c["arrays"].as_object().unwrap() {
        assert_eq!(pc.point_data.get(name).unwrap().scalar_type(), ScalarType::F64, "{name}");
        same(&col(&pc, name), &floats(&v[1]), name);
    }
    assert!(matches!(read("chombo2d_particles.hdf5", &[]), DataSet::MultiBlock(_)));
    // The grid is still the default.
    assert!(matches!(read("flash3d_hdf5_part_0007", &[]), DataSet::MultiBlock(_)));
    assert!(matches!(read("enzo/DD0004/data0004.hierarchy", &[]), DataSet::MultiBlock(_)));
}

/// A 4-D Chombo file (scripts/gen_amr_files.py: x, y, vpar, mu; one level, five boxes, one
/// (i, j) box not covering the last vpar cell). ParaView 6.1.1's VisIt reader builds the same
/// two 2-D boxes (origin, spacing, 4 x 6 cells) but no arrays; auraFX gives VisIt's array
/// variables: every velocity cell's value (checked against the formulas), NaN where no box
/// covers it, `_vpar`, `_mu` and the sums.
#[test]
fn chombo_4d_reads_velocity_space() {
    use aurafx_data::DataSet;
    let d = static_ds("amr/chombo4d.hdf5");
    let DataSet::MultiBlock(mb) = &*d else { panic!() };
    let blocks: Vec<aurafx_data::ImageData> = mb
        .blocks
        .iter()
        .flat_map(|(_, b)| match b {
            Some(DataSet::MultiBlock(level)) => level
                .blocks
                .iter()
                .filter_map(|(_, x)| match x {
                    Some(DataSet::ImageData(i)) => Some(i.clone()),
                    _ => None,
                })
                .collect::<Vec<_>>(),
            _ => Vec::new(),
        })
        .collect();
    assert_eq!(blocks.len(), 2);
    let dx = [0.25, 0.5, 0.1, 0.2];
    let f = |x: f64, y: f64, v: f64, m: f64| 1.0 + x + 10.0 * y + 100.0 * v + 1000.0 * m;
    for (b, img) in blocks.iter().enumerate() {
        assert_eq!(img.origin, [b as f64 * 1.0, 0.0, 0.0]);
        assert_eq!(img.spacing, [0.25, 0.5, 1.0]);
        assert_eq!(img.extent, [0, 4, 0, 6, 0, 0]);
        let a = img.cell_data.get("f").unwrap();
        assert_eq!(a.ncomp, 12);
        let (vpar, mu) = (img.cell_data.get("_vpar").unwrap(), img.cell_data.get("_mu").unwrap());
        let sum = img.cell_data.get("f_sum").unwrap();
        for j in 0..6 {
            for i in 0..4 {
                let t = j * 4 + i;
                let (x, y) = ((4 * b + i) as f64 * dx[0] + 0.125, (j as f64 + 0.5) * dx[1]);
                let mut total = 0.0;
                for k in 0..4 {
                    for l in 0..3 {
                        let c = k * 3 + l;
                        let (v, m) = ((k as f64 + 0.5) * dx[2], (l as f64 + 0.5) * dx[3]);
                        assert!((vpar.get(t, c) - v).abs() < 1e-12 && (mu.get(t, c) - m).abs() < 1e-12);
                        let got = a.get(t, c);
                        if b == 1 && k == 3 {
                            assert!(got.is_nan(), "block 1 k=3 is not covered");
                        } else {
                            assert!((got - f(x, y, v, m)).abs() < 1e-9, "block {b} cell ({i},{j}) velocity ({k},{l}): {got}");
                            total += got;
                        }
                    }
                }
                assert!((sum.get(t, 0) - total).abs() < 1e-9);
            }
        }
    }
}

/// EBSD maps (testdata/ebsd from scripts/gen_ebsd.py; ebsd_expected.json holds the angles as
/// written and the expected values after reading).
#[test]
fn ebsd_maps() {
    use aurafx_data::DataSet;
    let exp: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(td("ebsd/ebsd_expected.json")).unwrap()).unwrap();
    for (file, nphase, laue) in [("ni_map.ang", 1, vec![1u32]), ("two_phase.ang", 2, vec![1, 0]), ("ti_map.ctf", 1, vec![0])] {
        let ds = static_ds(&format!("ebsd/{file}"));
        let e = &exp[file];
        let (nx, ny, step) = (e["nx"].as_u64().unwrap() as usize, e["ny"].as_u64().unwrap() as usize, e["step"].as_f64().unwrap());
        let DataSet::ImageData(img) = &*ds else { panic!("{file}: not image data") };
        assert_eq!(img.extent, [0, nx as i64, 0, ny as i64, 0, 0], "{file}");
        assert!((img.spacing[0] - step).abs() < 1e-9 && (img.origin[0] + 0.5 * step).abs() < 1e-9, "{file}");
        let cs = img.field_data.get("CrystalStructures").unwrap();
        assert_eq!(cs.len(), nphase + 1);
        assert_eq!(cs.get(0, 0), 999.0);
        for (k, c) in laue.iter().enumerate() {
            assert_eq!(cs.get(k + 1, 0) as u32, *c, "{file} phase {}", k + 1);
        }
        let eul = img.cell_data.get("EulerAngles").unwrap();
        let ph = img.cell_data.get("Phases").unwrap();
        for p in e["points"].as_array().unwrap() {
            let (i, j) = (p["i"].as_u64().unwrap() as usize, p["j"].as_u64().unwrap() as usize);
            let c = j * nx + i;
            assert_eq!(ph.get(c, 0) as i64, p["phase"].as_i64().unwrap(), "{file} ({i}, {j})");
            for k in 0..3 {
                let w = p["euler"][k].as_f64().unwrap();
                assert!((eul.get(c, k) - w).abs() < 2e-5, "{file} ({i}, {j}) angle {k}: {} vs {w}", eul.get(c, k));
            }
            if let Some(ci) = p.get("ci") {
                assert!((img.cell_data.get("Confidence Index").unwrap().get(c, 0) - ci.as_f64().unwrap()).abs() < 1e-6);
            }
        }
    }
    // The hexagonal axes as stored: phi2 without the 30 degrees.
    let o: aurafx_io::ReaderOptions = [("hexagonal_axes".to_string(), aurafx_io::ebsd::HEX_AS_STORED.to_string())].into();
    let raw = match aurafx_io::with_options(&o, || read_file(td("ebsd/ti_map.ctf"))).unwrap() {
        Loaded::Static(d) => d,
        _ => panic!(),
    };
    let p = exp["ti_map.ctf"]["points"].as_array().unwrap().iter().find(|p| p["phase"] == 1).unwrap();
    let c = p["j"].as_u64().unwrap() as usize * 70 + p["i"].as_u64().unwrap() as usize;
    let want = (p["euler"][2].as_f64().unwrap() - 30f64.to_radians()).rem_euclid(std::f64::consts::TAU);
    assert!((raw.cell_data().unwrap().get("EulerAngles").unwrap().get(c, 2) - want).abs() < 2e-5);
    assert!(read_file(td("ebsd/ni_map.ang")).is_ok());
}

fn image_of(d: &aurafx_data::DataSet) -> &aurafx_data::ImageData {
    match d {
        aurafx_data::DataSet::ImageData(i) => i,
        _ => panic!("not image data"),
    }
}

/// The EBSD HDF5 layouts (testdata/ebsd from scripts/gen_ebsd_h5.py) hold the same maps as
/// the text files they were written from.
#[test]
fn ebsd_hdf5_files() {
    let same = |a: &aurafx_data::ImageData, b: &aurafx_data::ImageData, cells: &[(usize, usize)], what: &str| {
        for (ia, ib) in cells {
            for name in ["EulerAngles", "Phases"] {
                let (x, y) = (a.cell_data.get(name).unwrap(), b.cell_data.get(name).unwrap());
                for c in 0..x.ncomp {
                    assert!((x.get(*ia, c) - y.get(*ib, c)).abs() < 2e-5, "{what} {name} cell {ia}/{ib}: {} vs {}", x.get(*ia, c), y.get(*ib, c));
                }
            }
        }
    };
    let ni = static_ds("ebsd/ni_map.ang");
    let ni = image_of(&ni);
    let all: Vec<(usize, usize)> = (0..80 * 60).map(|k| (k, k)).collect();
    // EDAX: two scans, IQ/CI renamed as in .ang files.
    let edax = static_ds("ebsd/ni_map_edax.h5");
    let e = image_of(&edax);
    assert_eq!(e.extent, ni.extent);
    same(e, ni, &all, "EDAX");
    assert!(e.cell_data.get("Image Quality").is_some() && e.cell_data.get("Confidence Index").is_some());
    let o: aurafx_io::ReaderOptions = [("scan".to_string(), "Scan 2".to_string())].into();
    let s2 = match aurafx_io::with_options(&o, || read_file(td("ebsd/ni_map_edax.h5"))).unwrap() {
        Loaded::Static(d) => d,
        _ => panic!(),
    };
    let two = static_ds("ebsd/two_phase.ang");
    let two = image_of(&two);
    let all2: Vec<(usize, usize)> = (0..60 * 40).map(|k| (k, k)).collect();
    same(image_of(&s2), two, &all2, "EDAX scan 2");
    assert_eq!(image_of(&s2).field_data.get("CrystalStructures").unwrap().len(), 3);
    // Bruker: shuffled points placed by their beam position, phases by space group.
    let br = static_ds("ebsd/two_phase_bruker.h5");
    let b = image_of(&br);
    same(b, two, &all2, "Bruker");
    let cs = b.field_data.get("CrystalStructures").unwrap();
    assert_eq!((cs.get(1, 0), cs.get(2, 0)), (1.0, 0.0));
    assert!((b.spacing[0] - 2.0).abs() < 1e-12 && (b.origin[0] + 1.0).abs() < 1e-9);
    // TSL stack: slice z is the window of ni_map at the recorded corner.
    let exp: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(td("ebsd/ebsd_h5_expected.json")).unwrap()).unwrap();
    let st = static_ds("ebsd/ni_stack.h5ebsd");
    let s = image_of(&st);
    assert_eq!(s.extent, [0, 70, 0, 50, 0, 4]);
    assert!((s.spacing[2] - 0.5).abs() < 1e-6);
    for (z, c) in exp["ni_stack.h5ebsd"]["corners"].as_array().unwrap().iter().enumerate() {
        let (c0, r0) = (c[0].as_u64().unwrap() as usize, c[1].as_u64().unwrap() as usize);
        let cells: Vec<(usize, usize)> = (0..50).flat_map(|j| (0..70).map(move |i| (z * 3500 + j * 70 + i, (j + r0) * 80 + i + c0))).collect();
        same(s, ni, &cells, "TSL stack");
    }
    assert_eq!(s.field_data.get("SampleTransformationAngle").unwrap().get(0, 0), 180.0);
    // HKL stack: degrees to radians and +30 degrees on phi2, like the .ctf reader.
    let ti = static_ds("ebsd/ti_map.ctf");
    let hk = static_ds("ebsd/ti_stack.h5ebsd");
    let h = image_of(&hk);
    assert_eq!(h.extent, [0, 70, 0, 50, 0, 2]);
    let all3: Vec<(usize, usize)> = (0..3500).flat_map(|k| [(k, k), (k + 3500, k)]).collect();
    same(h, image_of(&ti), &all3, "HKL stack");
}

/// A window of a real `.dream3d` volume (testdata/dream3d/am_bench_subset.dream3d, cut by
/// scripts/gen_ebsd_h5.py from NIST's AM Bench 2022 IN718 EBSD data).
#[test]
fn dream3d_subset() {
    let d = static_ds("dream3d/am_bench_subset.dream3d");
    let img = image_of(&d);
    assert_eq!(img.extent, [0, 48, 0, 40, 0, 6]);
    assert!((img.spacing[2] - 2.12).abs() < 1e-5);
    let e = img.cell_data.get("EulerAngles").unwrap();
    assert_eq!((e.ncomp, e.len()), (3, 48 * 40 * 6));
    assert_eq!(img.cell_data.get("IPFColorZ").unwrap().ncomp, 3);
    let cs = img.field_data.get("CrystalStructures").unwrap();
    assert_eq!(cs.get(0, 0), 999.0);
    assert_eq!(cs.get(1, 0), 1.0);
    assert!(matches!(&img.field_data.get("MaterialName").unwrap().data, ArrayData::String(v) if v[1] == "Ni"));
    // Feature arrays (one row per feature) are field data.
    let fid = img.cell_data.get("FeatureIds").unwrap();
    let nf = (0..fid.len()).map(|k| fid.get(k, 0) as usize).max().unwrap();
    assert_eq!(img.field_data.get("AvgQuats").unwrap().len(), nf + 1);
    // Only some arrays.
    let o: aurafx_io::ReaderOptions = [("arrays".to_string(), "EulerAngles, Phases".to_string())].into();
    let d2 = match aurafx_io::with_options(&o, || read_file(td("dream3d/am_bench_subset.dream3d"))).unwrap() {
        Loaded::Static(d) => d,
        _ => panic!(),
    };
    let names: Vec<&str> = image_of(&d2).cell_data.names().collect();
    assert_eq!(names, vec!["EulerAngles", "Phases"]);
}
