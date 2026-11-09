//! Solver inputs from a microstructure volume: DAMASK, the FFT texture file and LAMMPS atoms.
use aurafx_data::DataSet;
use aurafx_filters::ParamValue;
use aurafx_pipeline::cases::{write_case, CaseOptions};
use aurafx_pipeline::Session;

/// A periodic Voronoi microstructure of `n` grains on `cells`³ voxels.
fn micro(cells: f64, n: i64) -> DataSet {
    let mut s = Session::new();
    let v = s.add_source("voronoi_grains").unwrap();
    s.set_param(v, "cells", ParamValue::Floats(vec![cells; 3]));
    s.set_param(v, "grains", ParamValue::Int(n));
    s.set_param(v, "periodic", ParamValue::Bool(true));
    s.update_blocking();
    assert!(s.nodes[&v].error.is_none(), "{:?}", s.nodes[&v].error);
    (*s.nodes[&v].output.clone().unwrap()).clone()
}

fn tmp(name: &str) -> std::path::PathBuf {
    let d = std::env::temp_dir().join(format!("afx_cases_micro_{}_{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    d
}

#[test]
fn a_damask_case_has_one_material_per_grain_with_its_orientation() {
    let ds = micro(12.0, 6);
    let dir = tmp("damask");
    let c = write_case(&ds, "damask", &dir, &CaseOptions { material: "Copper".into(), strain: 0.05, steps: 20, ..Default::default() }).unwrap();
    assert_eq!(c.template, "damask");
    // The grid: 12³ cells, material = grain id - 1.
    let aurafx_io::Loaded::Static(grid) = aurafx_io::read_file(dir.join("grid.vti")).unwrap() else { panic!() };
    let DataSet::ImageData(g) = &*grid else { panic!() };
    let m = g.cell_data.get("material").expect("the material array");
    assert_eq!(m.len(), 12 * 12 * 12);
    let max = (0..m.len()).map(|i| m.get(i, 0) as i64).max().unwrap();
    assert_eq!(max, 5, "materials 0..5 for 6 grains");
    let y = std::fs::read_to_string(dir.join("material.yaml")).unwrap();
    assert_eq!(y.matches("- homogenization: SX").count(), 6, "{y}");
    assert!(y.contains("lattice: cF") && y.contains("C_11: 1.") && y.contains("e+11"), "{y}");
    // Every orientation is a unit quaternion with a non-negative real part.
    for line in y.lines().filter(|l| l.trim_start().starts_with("O: [")) {
        let v: Vec<f64> = line.trim().trim_start_matches("O: [").trim_end_matches(']').split(',').map(|x| x.trim().parse().unwrap()).collect();
        assert!((v.iter().map(|x| x * x).sum::<f64>() - 1.0).abs() < 1e-9 && v[0] >= 0.0, "{line}");
    }
    let load = std::fs::read_to_string(dir.join("load.yaml")).unwrap();
    assert!(load.contains("dot_F: [[5.000000e-2, 0, 0], [0, x, 0], [0, 0, x]]") && load.contains("P: [[x, x, x], [x, 0, x], [x, x, 0]]"), "{load}");
    assert!(load.contains("N: 20"));
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn the_fft_texture_file_has_a_line_per_voxel_with_its_grain() {
    let ds = micro(8.0, 4);
    let dir = tmp("fft");
    write_case(&ds, "fft", &dir, &CaseOptions::default()).unwrap();
    let t = std::fs::read_to_string(dir.join("micro.txt")).unwrap();
    let lines: Vec<&str> = t.lines().collect();
    assert_eq!(lines.len(), 512);
    let first: Vec<&str> = lines[0].split_whitespace().collect();
    assert_eq!(first.len(), 8);
    assert_eq!(&first[3..6], &["1", "1", "1"], "voxel indices from 1");
    let last: Vec<&str> = lines[511].split_whitespace().collect();
    assert_eq!(&last[3..6], &["8", "8", "8"]);
    assert_eq!(lines[1].split_whitespace().nth(3), Some("2"), "x fastest");
    // The angles are degrees, and every voxel of a grain has that grain's angles.
    let DataSet::ImageData(img) = &ds else { panic!() };
    let f = img.cell_data.get("FeatureIds").unwrap();
    let mut by_grain = std::collections::HashMap::new();
    for (c, l) in lines.iter().enumerate() {
        let v: Vec<f64> = l.split_whitespace().map(|x| x.parse().unwrap()).collect();
        assert_eq!(v[6] as i64, f.get(c, 0) as i64, "the grain column");
        assert!((0.0..=360.0).contains(&v[0]) && (0.0..=180.0).contains(&v[1]));
        let e = by_grain.entry(v[6] as i64).or_insert([v[0], v[1], v[2]]);
        assert!((e[0] - v[0]).abs() < 1e-3 && (e[1] - v[1]).abs() < 1e-3);
    }
    assert_eq!(by_grain.len(), 4);
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn lammps_atoms_fill_the_grains_at_their_orientations() {
    let ds = micro(10.0, 3);
    let dir = tmp("lammps");
    // Aluminium (FCC, a = 4.05 Å), 2 nm voxels: a 20 nm cube.
    let o = CaseOptions { material: "Aluminium".into(), angstrom_per_unit: 20.0, ..Default::default() };
    write_case(&ds, "lammps", &dir, &o).unwrap();
    let data = std::fs::read_to_string(dir.join("atoms.data")).unwrap();
    let n: usize = data.lines().find(|l| l.ends_with(" atoms") && !l.contains("file")).unwrap().split_whitespace().next().unwrap().parse().unwrap();
    let mat = aurafx_crystal::mechanics::MATERIALS.iter().find(|m| m.name == "Aluminium").unwrap();
    let a = aurafx_pipeline::cases_micro::lattice_constant(mat).unwrap();
    assert!((a - 4.05).abs() < 0.05, "a = {a}");
    // FCC holds 4 atoms per a³; the boundaries lose a few percent to the overlap removal.
    let ideal = 4.0 * 200f64.powi(3) / a.powi(3);
    assert!(n as f64 > 0.93 * ideal && (n as f64) < 1.01 * ideal, "{n} atoms, a perfect crystal would have {ideal:.0}");
    let pos: Vec<[f64; 3]> = data
        .lines()
        .skip_while(|l| !l.starts_with("Atoms"))
        .skip(1)
        .filter(|l| !l.trim().is_empty())
        .map(|l| {
            let v: Vec<f64> = l.split_whitespace().skip(2).map(|x| x.parse().unwrap()).collect();
            [v[0], v[1], v[2]]
        })
        .collect();
    assert_eq!(pos.len(), n);
    let grains: Vec<i32> =
        std::fs::read_to_string(dir.join("atom_grains.txt")).unwrap().lines().skip(1).map(|l| l.split_whitespace().nth(1).unwrap().parse().unwrap()).collect();
    assert_eq!(grains.len(), n);
    // No two atoms closer than half the nearest-neighbour distance (a/√2), and inside a grain the nearest
    // neighbours are its <110> directions turned by the grain's orientation.
    let mi = aurafx_pipeline::cases_micro::Micro::of(&ds).unwrap();
    let nn = a / 2f64.sqrt();
    let g1: Vec<usize> = (0..n).filter(|i| grains[*i] == 1).take(400).collect();
    let om = aurafx_crystal::rotation::euler_to_matrix(mi.euler[1]);
    let mut checked = 0;
    for &i in &g1 {
        for j in 0..n {
            if i == j {
                continue;
            }
            let d: [f64; 3] = std::array::from_fn(|k| pos[j][k] - pos[i][k]);
            let r = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt();
            assert!(r > 0.5 * nn - 1e-6, "atoms {i} and {j} are {r} apart");
            if grains[j] == 1 && (r - nn).abs() < 1e-3 {
                // In the crystal frame a nearest neighbour is a <110>/2 vector: two components ±a/2, one 0.
                let c: [f64; 3] = std::array::from_fn(|p| (0..3).map(|q| om[p][q] * d[q]).sum::<f64>() / (a / 2.0));
                let mut v: Vec<f64> = c.iter().map(|x| x.abs()).collect();
                v.sort_by(|x, y| x.total_cmp(y));
                assert!(v[0] < 1e-6 && (v[1] - 1.0).abs() < 1e-6 && (v[2] - 1.0).abs() < 1e-6, "{c:?}");
                checked += 1;
            }
        }
        if checked > 200 {
            break;
        }
    }
    assert!(checked > 100, "{checked} neighbour pairs checked");
    let deck = std::fs::read_to_string(dir.join("in.lammps")).unwrap();
    assert!(deck.find("compute         pe").unwrap() < deck.find("dump            1").unwrap(), "the compute comes before the dump that uses it");
    assert!(deck.contains("pair_coeff      * * Al.eam.alloy Al"));
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn a_mesh_is_refused_for_volume_cases_with_the_reason() {
    let mut s = Session::new();
    let sph = s.add_source("sphere").unwrap();
    s.update_blocking();
    let ds = (*s.nodes[&sph].output.clone().unwrap()).clone();
    let e = write_case(&ds, "damask", &tmp("refuse"), &CaseOptions::default()).unwrap_err().to_string();
    assert!(e.contains("microstructure volume"), "{e}");
}

/// Writes a DAMASK case into $AFX_CASE_DIR (and each grain's Bunge angles in euler.txt) for checking
/// with DAMASK's own Python package: `cargo test -p aurafx-pipeline --test cases_micro -- --ignored`.
#[test]
#[ignore]
fn write_a_damask_case_for_an_outside_check() {
    let Ok(dir) = std::env::var("AFX_CASE_DIR") else { return };
    let dir = std::path::PathBuf::from(dir);
    let ds = micro(12.0, 6);
    write_case(&ds, "damask", &dir, &CaseOptions { material: "Copper".into(), ..Default::default() }).unwrap();
    let mi = aurafx_pipeline::cases_micro::Micro::of(&ds).unwrap();
    let t: String = (1..=mi.grains()).map(|g| format!("{} {} {}\n", mi.euler[g][0], mi.euler[g][1], mi.euler[g][2])).collect();
    std::fs::write(dir.join("euler.txt"), t).unwrap();
}
