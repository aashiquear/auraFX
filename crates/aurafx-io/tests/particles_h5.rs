//! H5Part and openPMD (HDF5) on testdata/particles (scripts/gen_particles_h5.py: particle k of
//! step s at (k, 2k, 3k) + s (0.5, 0, 0) with energy 10 k + s; mesh value 100 k + 10 j + i +
//! 1000 s on a 4 x 3 x 2 grid of spacing 0.5 x 1 x 2 from (1, 2, 3)).
use aurafx_data::*;
use aurafx_io::{read_file, Loaded};

fn root() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../testdata/particles")
}

fn steps(name: &str) -> (Vec<f64>, Vec<std::sync::Arc<DataSet>>) {
    match read_file(root().join(name)).unwrap_or_else(|e| panic!("{name}: {e:#}")) {
        Loaded::Series(s) => (
            s.times.clone(),
            s.steps
                .iter()
                .map(|st| match st {
                    TimeStepRef::Loaded(d) => d.clone(),
                    _ => panic!(),
                })
                .collect(),
        ),
        Loaded::Static(d) => (vec![0.0], vec![d]),
    }
}

fn check_particles(p: &PolyData, s: f64) {
    assert_eq!(p.points.len(), 6);
    for k in 0..6 {
        let q = p.points.get(k);
        let kf = k as f64;
        assert_eq!(q, [kf + 0.5 * s, 2.0 * kf, 3.0 * kf], "particle {k} step {s}");
        assert_eq!(p.point_data.get("energy").unwrap().get(k, 0), 10.0 * kf + s);
    }
}

#[test]
fn h5part_steps() {
    let (times, ds) = steps("steps.h5part");
    assert_eq!(times, vec![0.0, 0.25, 0.5]);
    for (s, d) in ds.iter().enumerate() {
        let DataSet::PolyData(p) = &**d else { panic!() };
        check_particles(p, s as f64);
        assert_eq!(p.point_data.get("id").unwrap().get(5, 0), 5.0);
    }
}

#[test]
fn openpmd_group_and_file_based() {
    for name in ["group_based.h5", "file_0.h5"] {
        let (times, ds) = steps(name);
        // Iterations 0 and 10 at times 0 and 5, in units of 1e-15 s.
        assert_eq!(times.len(), 2, "{name}");
        assert!((times[1] - 5e-15).abs() < 1e-27);
        for (s, d) in ds.iter().enumerate() {
            let DataSet::MultiBlock(mb) = &**d else { panic!() };
            let get = |n: &str| mb.blocks.iter().find(|b| b.0 == n).and_then(|b| b.1.clone()).unwrap_or_else(|| panic!("{name}: no block {n}"));
            let DataSet::ImageData(im) = get("meshes") else { panic!() };
            assert_eq!((im.dims(), im.spacing, im.origin), ([4, 3, 2], [0.5, 1.0, 2.0], [1.0, 2.0, 3.0]), "{name}");
            let rho = im.point_data.get("rho").unwrap();
            for k in 0..2 {
                for j in 0..3 {
                    for i in 0..4 {
                        assert_eq!(rho.get(i + 4 * (j + 3 * k), 0), (100 * k + 10 * j + i + 1000 * s) as f64);
                    }
                }
            }
            let DataSet::PolyData(p) = get("electrons") else { panic!() };
            check_particles(&p, s as f64);
        }
    }
}

#[test]
fn nek5000_elements() {
    let p = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../testdata/nek5000/box.nek5000");
    let Loaded::Series(s) = read_file(&p).unwrap() else { panic!("two steps") };
    assert_eq!(s.times.len(), 2);
    assert!((s.times[1] - 0.2).abs() < 1e-6);
    for (k, st) in s.steps.iter().enumerate() {
        let TimeStepRef::Loaded(d) = st else { panic!() };
        let DataSet::UnstructuredGrid(u) = &**d else { panic!() };
        // Two elements of 2 x 2 x 2 hexahedra; 27 points each (not merged).
        assert_eq!((u.points.len(), u.cells.len()), (54, 16));
        let q = u.points.get(27 + 13);
        assert_eq!(q, [1.5, 0.5, 0.5]);
        let v = u.point_data.get("Velocity").unwrap();
        assert_eq!((v.get(40, 0), v.get(40, 1), v.get(40, 2)), (1.5, 0.5, 0.5));
        assert_eq!(u.point_data.get("Pressure").unwrap().get(40, 0), 2.0);
        assert_eq!(u.point_data.get("Temperature").unwrap().get(40, 0), 100.0 + 10.0 * (k + 1) as f64 + 0.5);
        // Positive volumes: the hexahedra are numbered as VTK expects.
        let c = u.cells.cell(0);
        let p0 = u.points.get(c[0] as usize);
        let (a, b, d) = (u.points.get(c[1] as usize), u.points.get(c[3] as usize), u.points.get(c[4] as usize));
        let (x, y, z) = ([a[0] - p0[0], a[1] - p0[1], a[2] - p0[2]], [b[0] - p0[0], b[1] - p0[1], b[2] - p0[2]], [d[0] - p0[0], d[1] - p0[1], d[2] - p0[2]]);
        let det = x[0] * (y[1] * z[2] - y[2] * z[1]) - x[1] * (y[0] * z[2] - y[2] * z[0]) + x[2] * (y[0] * z[1] - y[1] * z[0]);
        assert!(det > 0.0);
    }
}

/// testdata/d3plot (scripts/gen_d3plot.py), as ParaView 6.1.1's LS-DYNA reader shows it: three
/// times, two hexahedra and a tetrahedron, stress XX 100 e + s, the apex at x 0.5 + 0.2 s.
#[test]
fn d3plot_states() {
    let p = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../testdata/d3plot/d3plot");
    let Loaded::Series(s) = read_file(&p).unwrap() else { panic!("three states") };
    assert_eq!(s.times.len(), 3);
    assert!((s.times[2] - 2e-3).abs() < 1e-9);
    for (k, st) in s.steps.iter().enumerate() {
        let TimeStepRef::Loaded(d) = st else { panic!() };
        let DataSet::UnstructuredGrid(u) = &**d else { panic!() };
        let types: Vec<CellType> = (0..u.cells.len()).map(|c| u.cells.cell_type(c)).collect();
        assert_eq!(types, vec![CellType::Hexahedron, CellType::Hexahedron, CellType::Tetra, CellType::Line, CellType::Quad]);
        let st = u.cell_data.get("Stress").unwrap();
        assert_eq!((st.get(0, 0), st.get(1, 0), st.get(2, 0), st.get(4, 0)), (k as f64, 100.0 + k as f64, 200.0 + k as f64, k as f64));
        assert!((u.cell_data.get("EffectivePlasticStrain").unwrap().get(1, 0) - 0.01 * k as f64).abs() < 1e-6);
        assert_eq!(u.cell_data.get("BeamResultants").unwrap().get(3, 0), k as f64);
        assert_eq!(u.cell_data.get("Material").unwrap().get(3, 0), 2.0);
        let apex = u.points.get(12);
        assert!((apex[0] - (0.5 + 0.2 * k as f64)).abs() < 1e-6 && apex[2] == 2.0);
        assert!((u.point_data.get("Displacement").unwrap().get(12, 0) - 0.2 * k as f64).abs() < 1e-6);
        assert_eq!(u.point_data.get("Velocity").unwrap().get(0, 0), k as f64);
        assert_eq!(u.field_data.get("KineticEnergy").unwrap().get(0, 0), 10.0 * k as f64);
    }
}
