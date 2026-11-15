//! OFF, glTF, VRML and LAS on the files of scripts/gen_scenes.py, with the counts and bounds
//! ParaView 6.1.1 gives (its VRML reader leaves the transforms out; these are applied here).
use aurafx_data::*;
use aurafx_io::{read_file, Loaded};

fn load(rel: &str) -> std::sync::Arc<DataSet> {
    let p = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../testdata").join(rel);
    match read_file(&p).unwrap_or_else(|e| panic!("{rel}: {e:#}")) {
        Loaded::Static(d) => d,
        _ => panic!("{rel}: a series"),
    }
}

fn close(a: [f64; 6], b: [f64; 6], tol: f64) -> bool {
    a.iter().zip(&b).all(|(x, y)| (x - y).abs() <= tol)
}

fn bounds(d: &DataSet) -> [f64; 6] {
    let b = d.bounds();
    [b.min[0], b.max[0], b.min[1], b.max[1], b.min[2], b.max[2]]
}

#[test]
fn gltf_like_the_reference() {
    for f in ["scenes/boxes.glb", "scenes/boxes.gltf"] {
        let d = load(f);
        assert_eq!((d.n_points(), d.n_cells()), (48, 24), "{f}");
        assert!(close(bounds(&d), [0.0, 3.0, 0.0, 2.0, 0.0, 2.0], 1e-6), "{f} {:?}", bounds(&d));
        let DataSet::MultiBlock(mb) = &*d else { panic!() };
        assert_eq!(mb.blocks[1].0, "box B");
        let Some(DataSet::PolyData(b)) = &mb.blocks[1].1 else { panic!() };
        // 0.9 as a 32-bit float is just under 0.9: 229.49... rounds to 229.
        assert_eq!(b.point_data.get("Colors").unwrap().get(0, 0), 229.0);
        assert!(b.point_data.get("Normals").is_some());
    }
}

#[test]
fn vrml_off_las() {
    let w = load("scenes/scene.wrl");
    assert_eq!((w.n_points(), w.n_cells()), (11, 7));
    // The box: scaled by 2, turned 90 degrees about z, moved by 10 in x; the line moved by 10.
    assert!(close(bounds(&w), [8.0, 13.0, 0.0, 2.0, 0.0, 3.0], 1e-6), "{:?}", bounds(&w));
    let o = load("scenes/tetra.off");
    let DataSet::PolyData(t) = &*o else { panic!() };
    assert_eq!((t.points.len(), t.polys.len()), (4, 4));
    assert_eq!(t.point_data.get("Colors").unwrap().get(2, 2), 255.0);
    let l = load("lidar/terrain.las");
    assert_eq!(l.n_points(), 20000);
    assert!(close(bounds(&l), [0.009, 99.99, 0.003, 99.993, 0.001, 12.204], 1e-3), "{:?}", bounds(&l));
    let c = l.point_data().unwrap().get("Classification").unwrap();
    let classes: std::collections::BTreeSet<i64> = (0..c.len()).map(|i| c.get(i, 0) as i64).collect();
    assert_eq!(classes.into_iter().collect::<Vec<_>>(), vec![2, 5, 6]);
}
