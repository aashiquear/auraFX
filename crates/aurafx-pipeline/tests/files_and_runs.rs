//! Files and runs: `.dream3d` files written with the pipeline that made them and read back,
//! pipelines rebuilt from those files, the steps of a recorded pipeline file (the AM Bench
//! serial-section pipeline, testdata/dream3d/am_bench_pipeline.json) mapped to auraFX filters,
//! and SPPARKS site files.

use aurafx_data::*;
use aurafx_pipeline::Session;

fn td(n: &str) -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../testdata").join(n)
}

fn tmp(n: &str) -> std::path::PathBuf {
    std::env::temp_dir().join(format!("aurafx_g7_{}_{n}", std::process::id()))
}

fn load(p: &std::path::Path) -> DataSet {
    match aurafx_io::read_file(p).unwrap() {
        aurafx_io::Loaded::Static(d) => (*d).clone(),
        _ => panic!(),
    }
}

fn same_arrays(a: &Attributes, b: &Attributes) {
    for x in a.iter().filter(|x| !matches!(x.data, ArrayData::String(_))) {
        let y = b.get(&x.name).unwrap_or_else(|| panic!("{} missing", x.name));
        assert_eq!((x.len(), x.ncomp), (y.len(), y.ncomp), "{}", x.name);
        for i in 0..x.len() {
            for c in 0..x.ncomp {
                let (u, v) = (x.get(i, c), y.get(i, c));
                assert!(u == v || (u - v).abs() <= 1e-6 * u.abs().max(1.0) || (u.is_nan() && v.is_nan()), "{} [{i},{c}]: {u} vs {v}", x.name);
            }
        }
    }
}

#[test]
fn dream3d_with_pipeline_round_trip() {
    let mut s = Session::new();
    let r = s.open_file(td("dream3d/am_bench_subset.dream3d")).unwrap();
    let seg = s.add_filter("segment_features", r).unwrap();
    s.set_param_json(seg, "tolerance", &3.0.into());
    s.set_param_json(seg, "mask", &serde_json::json!(["CELLS", "SampleVoxels"]));
    let kam = s.add_filter("kernel_average_misorientation", seg).unwrap();
    let sizes = s.add_filter("feature_sizes", kam).unwrap();
    s.update_blocking();
    let out = s.nodes[&sizes].output.clone().unwrap();
    let f = tmp("pipe.dream3d");
    s.save_data(sizes, &f).unwrap();
    // The data reads back unchanged (cell and field arrays).
    let back = load(&f);
    let (DataSet::ImageData(a), DataSet::ImageData(b)) = (&*out, &back) else { panic!() };
    assert_eq!((a.extent, a.spacing), (b.extent, b.spacing));
    same_arrays(&a.cell_data, &b.cell_data);
    same_arrays(&a.field_data, &b.field_data);
    // The file records the pipeline; importing it rebuilds the same result.
    let text = aurafx_io::ebsd_h5::dream3d_pipeline(&f).unwrap().unwrap();
    assert!(text.contains("auraFX::segment_features") && text.contains("PipelineBuilder"));
    let mut s2 = Session::new();
    let made = s2.import_pipeline(&f, None).unwrap();
    assert!(made.skipped.is_empty(), "{:?}", made.skipped);
    assert_eq!(made.nodes.len(), 4);
    s2.update_blocking();
    let again = s2.nodes[made.nodes.last().unwrap()].output.clone().unwrap();
    let DataSet::ImageData(c) = &*again else { panic!() };
    same_arrays(&a.cell_data, &c.cell_data);
    // A surface mesh writes as a triangle geometry.
    let qs = s.add_filter("quick_surface_mesh", seg).unwrap();
    s.update_blocking();
    let fs = tmp("surf.dream3d");
    s.save_data(qs, &fs).unwrap();
    let sb = load(&fs);
    let so = s.nodes[&qs].output.clone().unwrap();
    assert_eq!(sb.to_unstructured().unwrap().cells.len(), so.to_unstructured().unwrap().cells.len());
    if let Ok(dir) = std::env::var("AURAFX_KEEP_FILES") {
        let _ = std::fs::copy(&f, std::path::Path::new(&dir).join("volume.dream3d"));
        let _ = std::fs::copy(&fs, std::path::Path::new(&dir).join("surface.dream3d"));
    }
    let _ = std::fs::remove_file(&f);
    let _ = std::fs::remove_file(&fs);
}

#[test]
fn recorded_pipeline_steps() {
    // The AM Bench pipeline: 71 steps; its reading steps name files not here, so the steps run
    // on the selected data.
    let mut s = Session::new();
    let r = s.open_file(td("dream3d/am_bench_subset.dream3d")).unwrap();
    let made = s.import_pipeline(&td("dream3d/am_bench_pipeline.json"), Some(r)).unwrap();
    let ids: Vec<String> = made.nodes.iter().map(|n| s.nodes[n].def().unwrap().id.to_string()).collect();
    let find = |id: &str| made.nodes.iter().copied().filter(|n| s.nodes[n].def().unwrap().id == id).collect::<Vec<_>>();
    // Steps 48 to 67: convert, segment (2 degrees, the SampleVoxels mask), fill bad data (7),
    // sizes, minimum size (7), centroids, shapes, average and reference orientations, KAM
    // (kernel 2 2 1), IPF colors.
    let seg = find("segment_features");
    assert_eq!(seg.len(), 1);
    assert_eq!(s.nodes[&seg[0]].params.f64("tolerance", 0.0), 2.0);
    assert_eq!(s.nodes[&seg[0]].params.array("mask").map(|m| m.1), Some("SampleVoxels".into()));
    assert_eq!(s.nodes[&find("minimum_size")[0]].params.i64("min_cells", 0), 7);
    assert_eq!(s.nodes[&find("fill_bad_data")[0]].params.i64("min_defect", 0), 7);
    assert_eq!(s.nodes[&find("kernel_average_misorientation")[0]].params.vec3("kernel", [0.0; 3]), [2.0, 2.0, 1.0]);
    assert_eq!(find("orientation_colors").len(), 3);
    let thr = find("calculator");
    assert_eq!(s.nodes[&thr[0]].params.str("expression"), "if(Otsu > 0, 1, 0)");
    assert_eq!(s.nodes[&thr[1]].params.str("expression"), "if(Error == 0, 1, 0) * if(Phases > 0, 1, 0)");
    let smooth = find("smooth_boundaries");
    assert!((s.nodes[&smooth[0]].params.f64("mu", 0.0) - (-1.03 * 0.15)).abs() < 1e-6);
    eprintln!("mapped {} steps: {ids:?}\nskipped {}: {:?}", made.nodes.len(), made.skipped.len(), made.skipped.iter().map(|x| &x.0).collect::<Vec<_>>());
    assert!(made.skipped.iter().any(|(n, _)| n.contains("Write DREAM.3D")));
}

#[test]
fn recorded_pipeline_runs() {
    // The segmentation part of the AM Bench pipeline (steps 48 to 67) on the AM Bench window.
    // testdata/dream3d/am_bench_segmentation.json: steps 48 to 67 of the full pipeline.
    let mut s = Session::new();
    let r = s.open_file(td("dream3d/am_bench_subset.dream3d")).unwrap();
    let made = s.import_pipeline(&td("dream3d/am_bench_segmentation.json"), Some(r)).unwrap();
    // Skipped: three Delete Data steps and a CSV export.
    assert_eq!(made.skipped.len(), 4, "{:?}", made.skipped);
    assert_eq!(made.nodes.len(), 16);
    s.update_blocking();
    for n in &made.nodes {
        assert!(s.nodes[n].error.is_none(), "{}: {:?}", s.nodes[n].name, s.nodes[n].error);
    }
    let last = s.nodes[made.nodes.last().unwrap()].output.clone().unwrap();
    let DataSet::ImageData(img) = &*last else { panic!() };
    for a in ["FeatureIds", "KernelAverageMisorientations", "IPFColorZ", "Quats", "EquivalentDiameters"] {
        assert!(img.cell_data.get(a).is_some(), "{a}");
    }
    let f = img.cell_data.get("FeatureIds").unwrap();
    let n = (0..f.len()).map(|i| f.get(i, 0) as i64).max().unwrap();
    assert!(n > 50, "{n} features");
    // The segmentation ran (the window's own ids are numbered 1..146 in another order).
    let seg = made.nodes.iter().find(|x| s.nodes[*x].def().unwrap().id == "segment_features").unwrap();
    let DataSet::ImageData(si) = &*s.nodes[seg].output.clone().unwrap() else { panic!() };
    let DataSet::ImageData(ri) = &*s.nodes[&r].output.clone().unwrap() else { panic!() };
    let (a, b) = (si.cell_data.get("FeatureIds").unwrap(), ri.cell_data.get("FeatureIds").unwrap());
    assert!((0..a.len()).any(|i| a.get(i, 0) != b.get(i, 0)));
    eprintln!("segmented into {} features (largest id after cleanup {n})", (0..a.len()).map(|i| a.get(i, 0) as i64).max().unwrap());
}

#[test]
fn spparks_sites() {
    let vol = load(&td("dream3d/am_bench_subset.dream3d"));
    let f = tmp("grains.spparks");
    aurafx_io::writers::write_file(&vol, &f).unwrap();
    let back = load(&f);
    let (DataSet::ImageData(a), DataSet::ImageData(b)) = (&vol, &back) else { panic!() };
    assert_eq!(a.extent, b.extent);
    let (x, y) = (a.cell_data.get("FeatureIds").unwrap(), b.cell_data.get("FeatureIds").unwrap());
    assert!((0..x.len()).all(|i| x.get(i, 0) == y.get(i, 0)));
    // A lattice dump (id i1 x y z) reads as the same volume.
    let d = tmp("dump.potts");
    let (nx, ny, nz) = (48, 40, 6);
    let mut t = format!("ITEM: TIMESTEP\n0\nITEM: NUMBER OF ATOMS\n{}\nITEM: BOX BOUNDS\n0 {nx}\n0 {ny}\n0 {nz}\nITEM: ATOMS id i1 x y z\n", nx * ny * nz);
    for k in (0..nx * ny * nz).rev() {
        t += &format!("{} {} {} {} {}\n", k + 1, x.get(k, 0) as i64, k % nx, (k / nx) % ny, k / (nx * ny));
    }
    std::fs::write(&d, t).unwrap();
    let dv = load(&d);
    let DataSet::ImageData(c) = &dv else { panic!("not a volume") };
    assert_eq!(c.extent, a.extent);
    let z = c.cell_data.get("FeatureIds").unwrap();
    assert!((0..x.len()).all(|i| x.get(i, 0) == z.get(i, 0)));
    let _ = std::fs::remove_file(&f);
    let _ = std::fs::remove_file(&d);
}

fn grain_box(cells: [i64; 3], grains: i64) -> DataSet {
    let b = aurafx_filters::find("box_mesh").unwrap();
    let mut p = b.default_params();
    p.set_json("cells", &serde_json::json!(cells));
    p.set_json("size", &serde_json::json!([4.0, 2.0, 1.0]));
    let mesh = b.run(None, &p).unwrap();
    let a = aurafx_filters::find("assign_grains").unwrap();
    let mut q = a.default_params();
    q.set_json("grains", &grains.into());
    q.set_json("blocks", &true.into());
    a.run(Some(&mesh), &q).unwrap()
}

#[test]
fn solver_cases() {
    let poly = grain_box([8, 4, 2], 6);
    let dir = std::env::var("AURAFX_KEEP_FILES").map(std::path::PathBuf::from).unwrap_or_else(|_| tmp("cases"));
    let o = aurafx_pipeline::cases::CaseOptions { steps: 2, end_time: 1.0, strain: 0.002, ..Default::default() };
    for solver in ["moose", "pinnacle"] {
        let c = aurafx_pipeline::cases::write_case(&poly, solver, &dir.join(solver), &o).unwrap();
        assert!(c.files.iter().all(|f| f.exists()), "{solver}");
        // The mesh reads back with one block per grain and the sets named for the deck.
        let m = load(&c.folder.join("mesh.e"));
        let DataSet::MultiBlock(mb) = &m else { panic!() };
        let g = poly.cell_data().unwrap().get("grain_id").unwrap();
        let mut blocks = 0;
        for (_, leaf) in mb.leaves() {
            let Some(id) = leaf.cell_data().and_then(|c| c.get("ObjectId")).map(|a| a.get(0, 0)) else { continue };
            let n = leaf.to_unstructured().unwrap().cells.len();
            assert_eq!(n, (0..g.len()).filter(|i| g.get(*i, 0) == id).count(), "block {id}");
            blocks += 1;
        }
        assert!(blocks >= 5, "{blocks} blocks");
        let deck = std::fs::read_to_string(c.folder.join(&c.input)).unwrap();
        assert!(deck.contains("x-") && deck.contains("x+"), "{deck}");
    }
    // MOOSE: one orientation row per grain id, the 12 FCC slip systems.
    let rows = std::fs::read_to_string(dir.join("moose/grain_euler.txt")).unwrap();
    let gmax = poly.field_data().get("GrainEulerAngles").unwrap().len() - 1;
    assert_eq!(rows.lines().count(), gmax);
    assert_eq!(std::fs::read_to_string(dir.join("moose/slip_systems.txt")).unwrap().lines().count(), 12);
    // PINNACLE: the point file gives every element its own grain by the nearest point.
    let pts = std::fs::read_to_string(dir.join("pinnacle/orientations_points.csv")).unwrap();
    assert_eq!(pts.lines().count(), 2 + poly.to_unstructured().unwrap().cells.len());
    // SPPARKS: from a volume.
    let vol = load(&td("dream3d/am_bench_subset.dream3d"));
    let fill = aurafx_filters::find("fill_bad_data").unwrap();
    let mut fp = fill.default_params();
    fp.set_json("min_defect", &100000.into());
    let filled = fill.run(Some(&vol), &fp).unwrap();
    let c = aurafx_pipeline::cases::write_case(&filled, "spparks", &dir.join("spparks"), &o).unwrap();
    let deck = std::fs::read_to_string(c.folder.join("in.potts")).unwrap();
    assert!(deck.contains("region          box block 0 48 0 40 0 6"), "{deck}");
    if std::env::var("AURAFX_KEEP_FILES").is_err() {
        let _ = std::fs::remove_dir_all(&dir);
    }
}
