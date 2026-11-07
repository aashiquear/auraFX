//! Grains on meshes: the benchmark study's element-to-grain rule (nearest point of its grain
//! point file at each element's centre, testdata/benchmark), Voronoi grains against brute
//! force, grains from a volume, blocks, boundary side sets and orientation files.

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

fn run_multi(id: &str, inputs: &[&DataSet], set: &[(&str, serde_json::Value)]) -> DataSet {
    let def = aurafx_filters::registry::find(id).unwrap();
    let mut p = def.default_params();
    for (k, v) in set {
        p.set_json(k, v);
    }
    if inputs.len() > 1 { def.run_multi(inputs, &p) } else { def.run(Some(inputs[0]), &p) }.unwrap_or_else(|e| panic!("{id}: {e:#}"))
}

#[test]
fn benchmark_grain_rule() {
    // The benchmark's plate (100 x 50 x 1, cells of 2.5, a hole of diameter 5) from Plate Mesh,
    // which builds it node for node and element for element as the benchmark did.
    let def = aurafx_filters::registry::find("plate_mesh").unwrap();
    let mesh = def.run(None, &def.default_params()).unwrap();
    let pts = load("benchmark/grain_points_gn2.csv");
    let out = run_multi("assign_grains", &[&mesh, &pts], &[("method", "Nearest point".into())]);
    let g = out.cell_data().unwrap().get("grain_id").unwrap();
    // The reference lists the grain of every element in the benchmark's element order.
    let reference = std::fs::read_to_string(root().join("benchmark/plate_h40_hole5_elem_grain.csv")).unwrap();
    let want: Vec<i32> = reference.lines().skip(1).map(|l| l.split(',').nth(1).unwrap().trim().parse().unwrap()).collect();
    assert_eq!(want.len(), g.len());
    let diff = (0..g.len()).filter(|i| g.get(*i, 0) as i32 != want[*i]).count();
    eprintln!("benchmark rule: {} elements, {diff} differ", g.len());
    assert_eq!(diff, 0);
}

#[test]
fn voronoi_volume_blocks_and_files() {
    let def = aurafx_filters::registry::find("box_mesh").unwrap();
    let mesh = def.run(None, &def.default_params()).unwrap();
    let v = run_multi("assign_grains", &[&mesh], &[("grains", 12.into()), ("blocks", true.into()), ("boundaries", true.into())]);
    let g = v.cell_data().unwrap().get("grain_id").unwrap();
    let blocks = v.cell_data().unwrap().get("ElementBlock").unwrap();
    assert!((0..g.len()).all(|i| g.get(i, 0) == blocks.get(i, 0)));
    let n: std::collections::BTreeSet<i64> = (0..g.len()).map(|i| g.get(i, 0) as i64).collect();
    assert!(n.len() >= 10 && n.len() <= 12, "{}", n.len());
    assert!(v.field_data().get("SideSet grain boundaries").is_some_and(|a| a.len() > 50));
    // Every element went to its nearest seed (brute force over the seeds).
    let seeds = v.field_data().get("GrainSeeds").unwrap();
    let ug = v.to_unstructured().unwrap();
    for (e, (_, ids)) in ug.cells.iter().enumerate() {
        let c: [f64; 3] = std::array::from_fn(|a| ids.iter().map(|k| ug.points.get(*k as usize)[a]).sum::<f64>() / ids.len() as f64);
        let best = (1..seeds.len()).min_by(|x, y| {
            let d = |s: usize| (0..3).map(|a| (seeds.get(s, a) - c[a]).powi(2)).sum::<f64>();
            d(*x).total_cmp(&d(*y))
        }).unwrap();
        assert_eq!(best as f64, g.get(e, 0), "element {e}");
    }
    // Orientation file: one line per grain.
    let tex = std::env::temp_dir().join(format!("aurafx_grains_{}.tex", std::process::id()));
    aurafx_io::writers::write_file(&v, &tex).unwrap();
    let text = std::fs::read_to_string(&tex).unwrap();
    let _ = std::fs::remove_file(&tex);
    let lines: Vec<&str> = text.lines().skip(4).collect();
    assert_eq!(text.lines().nth(3), Some("B 12"));
    assert_eq!(lines.len(), 12);
    assert!(lines.iter().all(|l| l.split_whitespace().count() == 4));
    // From a volume: the grains of the AM Bench window on a box mesh over it.
    let vol = load("dream3d/am_bench_subset.dream3d");
    let DataSet::ImageData(img) = &vol else { panic!() };
    let lo: [f64; 3] = std::array::from_fn(|a| img.origin[a] + img.extent[2 * a] as f64 * img.spacing[a]);
    let size: [f64; 3] = std::array::from_fn(|a| (img.extent[2 * a + 1] - img.extent[2 * a]) as f64 * img.spacing[a]);
    let bm = def.run(None, &{
        let mut p = def.default_params();
        p.set_json("size", &serde_json::json!(size));
        p.set_json("origin", &serde_json::json!(lo));
        p.set_json("cells", &serde_json::json!([24, 20, 3]));
        p
    }).unwrap();
    let fv = run_multi("assign_grains", &[&bm, &vol], &[("method", "From a volume".into())]);
    let fg = fv.cell_data().unwrap().get("grain_id").unwrap();
    let distinct: std::collections::BTreeSet<i64> = (0..fg.len()).map(|i| fg.get(i, 0) as i64).collect();
    assert!(distinct.len() > 20);
}

/// Mesh Grains: an overlay mesh of a microstructure carries each grain's orientation.
#[test]
fn overlay_mesh_grains() {
    let vol = load("dream3d/am_bench_subset.dream3d");
    let def = aurafx_filters::registry::find("overlay_mesh").unwrap();
    let m = def.run(Some(&vol), &def.default_params()).unwrap();
    let c = m.cell_data().unwrap();
    let (g, b) = (c.get("grain_id").unwrap(), c.get("ElementBlock").unwrap());
    assert!((0..g.len()).all(|i| g.get(i, 0) == b.get(i, 0)));
    assert!(c.get("EulerAngles").is_some() && m.field_data().get("GrainEulerAngles").is_some());
}

/// The guide's worked example (at a coarser size): geometry, mesh, grains, solver files.
#[test]
fn worked_example() {
    let g = aurafx_filters::registry::find("geometry").unwrap();
    let mut p = g.default_params();
    p.set_json("commands", &"brick x 4 y 1 z 0.5\ncylinder radius 0.2 height 1\nsubtract body 2 from body 1\nmesh body 1 size 0.1 scheme overlay".into());
    p.set_json("output", &"Hex mesh".into());
    let part = g.run(None, &p).unwrap();
    let poly = run_multi("assign_grains", &[&part], &[("grains", 40.into()), ("blocks", true.into()), ("boundaries", true.into())]);
    let dir = std::env::temp_dir().join(format!("aurafx_worked_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    aurafx_io::writers::write_file(&poly, &dir.join("plate.e")).unwrap();
    aurafx_io::writers::write_file(&poly, &dir.join("plate.tex")).unwrap();
    let back = load_abs(&dir.join("plate.e"));
    let m = aurafx_mesh::hex::HexMesh::from_dataset(&back).unwrap();
    assert!(m.block_names.len() >= 35 && m.side_set("grain boundaries").is_some() && m.side_set("cylinder 2 side").is_some());
    let tex = std::fs::read_to_string(dir.join("plate.tex")).unwrap();
    assert_eq!(tex.lines().skip(4).count(), *m.block_names.keys().max().unwrap() as usize);
    let _ = std::fs::remove_dir_all(&dir);
}

fn load_abs(p: &std::path::Path) -> DataSet {
    match aurafx_io::read_file(p).unwrap() {
        aurafx_io::Loaded::Static(d) => (*d).clone(),
        aurafx_io::Loaded::Series(s) => (*aurafx_io::load_step(&s.steps[0]).unwrap()).clone(),
    }
}
