//! The structured hex mesher against hexaform's own output (testdata/hexaform, from
//! scripts/hexaform_reference.py): four inclusions (sphere, rotated ellipsoid, superquadric,
//! spherical-harmonic grain) in a 12^3 grid, HEX8 and HEX27, node for node.

use aurafx_mesh::hex::HexMesh;
use aurafx_mesh::inclusions::*;
use serde_json::Value;
use std::io::Read;

fn td(name: &str) -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../testdata/hexaform").join(name)
}

fn gz_json(name: &str) -> Value {
    let mut s = String::new();
    flate2::read::GzDecoder::new(std::fs::File::open(td(name)).unwrap()).read_to_string(&mut s).unwrap();
    serde_json::from_str(&s).unwrap()
}

fn v3(v: &Value) -> [f64; 3] {
    std::array::from_fn(|k| v[k].as_f64().unwrap())
}

fn setup() -> (Grid, Vec<Placed>) {
    let s: Value = serde_json::from_str(&std::fs::read_to_string(td("four_shapes.json")).unwrap()).unwrap();
    let m = s["divisions"].as_u64().unwrap() as usize;
    let grid = Grid { div: [m; 3], origin: [0.0; 3], h: s["length"].as_f64().unwrap() / m as f64 };
    let mut placed = Vec::new();
    for (i, inc) in s["inclusions"].as_array().unwrap().iter().enumerate() {
        let c = v3(&inc["centre"]);
        let p = &inc["params"];
        let rot = |v: &Value| -> Option<[[f64; 3]; 3]> { v.as_array().map(|r| std::array::from_fn(|i| v3(&r[i]))) };
        let shape = match inc["kind"].as_str().unwrap() {
            "sphere" => StarShape::Sphere { centre: c, radius: p["radius"].as_f64().unwrap() },
            "ellipsoid" => StarShape::Ellipsoid { centre: c, semi_axes: v3(&p["semi_axes"]), rotation: rot(&p["rotation"]) },
            "superquadric" => StarShape::Superquadric {
                centre: c,
                semi_axes: v3(&p["semi_axes"]),
                exponent_xy: p["exponent_xy"].as_f64().unwrap(),
                exponent_z: p["exponent_z"].as_f64().unwrap(),
                rotation: None,
            },
            _ => StarShape::Harmonic {
                centre: c,
                radius: p["radius"].as_f64().unwrap(),
                coefficients: p["coefficients"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|t| (t[0].as_i64().unwrap() as i32, t[1].as_i64().unwrap() as i32, t[2].as_f64().unwrap()))
                    .collect(),
            },
        };
        let lo = v3(&inc["box_lo"]).map(|x| x as i64);
        let k = v3(&inc["k"]).map(|x| x as usize);
        // Our box sizing gives the same cells.
        assert_eq!(box_cells(&shape, grid.h, 1.35, 4), k, "box of inclusion {}", i + 1);
        placed.push(Placed { shape, lo, k, id: i as u32 + 1 });
    }
    (grid, placed)
}

fn compare(ours: &HexMesh, r: &Value, tol: f64) {
    let pts: Vec<f64> = r["points"].as_array().unwrap().iter().map(|x| x.as_f64().unwrap()).collect();
    assert_eq!(ours.points.len() * 3, pts.len(), "node count");
    let mut worst: f64 = 0.0;
    for (i, p) in ours.points.iter().enumerate() {
        for k in 0..3 {
            worst = worst.max((p[k] - pts[3 * i + k]).abs());
        }
    }
    assert!(worst < tol, "largest coordinate difference {worst}");
    let cells: Vec<u32> = r["cells"].as_array().unwrap().iter().map(|x| x.as_u64().unwrap() as u32).collect();
    assert_eq!(ours.npe, r["nodes_per_element"].as_u64().unwrap() as usize);
    assert_eq!(ours.cells, cells, "connectivity");
    let blocks: Vec<i32> = r["block_id"].as_array().unwrap().iter().map(|x| x.as_i64().unwrap() as i32).collect();
    assert_eq!(ours.block, blocks);
    for (k, v) in r["block_names"].as_object().unwrap() {
        assert_eq!(ours.block_names[&k.parse::<i32>().unwrap()], v.as_str().unwrap());
    }
    for (name, v) in r["side_sets"].as_object().unwrap() {
        let e = v[0].as_array().unwrap();
        let s = v[1].as_array().unwrap();
        let want: Vec<(u32, u8)> = e.iter().zip(s).map(|(e, s)| (e.as_u64().unwrap() as u32, s.as_u64().unwrap() as u8)).collect();
        assert_eq!(ours.side_set(name).unwrap(), &want, "side set {name}");
    }
    for (name, v) in r["node_sets"].as_object().unwrap() {
        let want: Vec<u32> = v.as_array().unwrap().iter().map(|x| x.as_u64().unwrap() as u32).collect();
        assert_eq!(ours.node_set(name).unwrap(), &want, "node set {name}");
    }
    let pid: Vec<f64> = r["particle_id"].as_array().unwrap().iter().map(|x| x.as_f64().unwrap()).collect();
    assert_eq!(ours.elem_field("particle_id").unwrap(), &pid);
}

#[test]
fn inclusions_match_hexaform() {
    let (grid, placed) = setup();
    let block = BlockOptions::default();
    for (npe, file) in [(8usize, "four_shapes_hex8.json.gz"), (27, "four_shapes_hex27.json.gz")] {
        let m = assemble(&grid, &placed, &block, &AssemblyOptions { nodes_per_element: npe, ..Default::default() }).unwrap();
        compare(&m, &gz_json(file), 1e-12);
        // The elements fill the cube exactly.
        if npe == 8 {
            let v: f64 = m.signed_volumes().iter().sum();
            assert!((v - 1.0).abs() < 1e-12, "{v}");
            assert!(m.signed_volumes().iter().all(|x| *x > 0.0));
        }
    }
}

#[test]
fn periodic_pairs_and_packing() {
    let grid = Grid { div: [16; 3], origin: [0.0; 3], h: 1.0 / 16.0 };
    let spec = PackingSpec { count: 6, radius_mean: 0.07, radius_std: 0.01, ..Default::default() };
    let mut rng = Rng::new(7);
    let placed = pack(&grid, &spec, &mut rng, &|r, _| StarShape::Sphere { centre: [0.0; 3], radius: r });
    assert!(placed.len() >= 4, "{}", placed.len());
    // Boxes are disjoint with two cells between them and one from the walls.
    for (i, a) in placed.iter().enumerate() {
        for a_ in 0..3 {
            assert!(a.lo[a_] >= 1 && a.lo[a_] as usize + a.k[a_] < 16);
        }
        for b in &placed[i + 1..] {
            let apart = (0..3).any(|x| a.lo[x] + a.k[x] as i64 + 2 <= b.lo[x] || b.lo[x] + b.k[x] as i64 + 2 <= a.lo[x]);
            assert!(apart);
        }
    }
    let m = assemble(&grid, &placed, &BlockOptions::default(), &AssemblyOptions { periodic: true, ..Default::default() }).unwrap();
    for (axis, name) in ["x", "y", "z"].iter().enumerate() {
        let pairs = &m.periodic_pairs.iter().find(|(n, _)| n == name).unwrap().1;
        assert_eq!(pairs.len(), 17 * 17);
        for (a, b) in pairs {
            let (p, q) = (m.points[*a as usize], m.points[*b as usize]);
            assert!(p[axis].abs() < 1e-12 && (q[axis] - 1.0).abs() < 1e-12);
            for k in 0..3 {
                if k != axis {
                    assert!((p[k] - q[k]).abs() < 1e-12);
                }
            }
        }
    }
    // Interface nodes lie on the spheres.
    for (e, s) in m.side_set("interface").unwrap() {
        let pid = m.elem_field("particle_id").unwrap()[*e as usize].abs() as usize;
        let StarShape::Sphere { centre, radius } = &placed[pid - 1].shape else { panic!() };
        for n in m.side_corners(*e as usize, *s) {
            let p = m.points[n as usize];
            let r = ((p[0] - centre[0]).powi(2) + (p[1] - centre[1]).powi(2) + (p[2] - centre[2]).powi(2)).sqrt();
            assert!((r - radius).abs() < 1e-12);
        }
    }
}

#[test]
fn box_and_mapped_blocks() {
    use aurafx_mesh::structured::*;
    let m = box_mesh([0.0; 3], [2.0, 1.0, 0.5], [&Division::uniform(4), &Division { cells: 3, ratio: 2.0, cluster: "start".into() }, &Division::uniform(2)]);
    assert_eq!((m.points.len(), m.n_elements()), (5 * 4 * 3, 24));
    let v: f64 = m.signed_volumes().iter().sum();
    assert!((v - 1.0).abs() < 1e-12);
    assert_eq!(m.side_set("x+").unwrap().len(), 6);
    assert_eq!(m.node_set("z-").unwrap().len(), 20);
    // Graded: the first y interval is half the last.
    let ys: Vec<f64> = (0..4).map(|j| m.points[j * 5][1]).collect();
    assert!(((ys[3] - ys[2]) / (ys[1] - ys[0]) - 2.0).abs() < 1e-9, "{ys:?}");
    // A skewed block keeps positive volumes; HEX20 and HEX27 nodes are shared.
    let mut b = mapped_block(
        [[0., 0., 0.], [2., 0., 0.], [2.5, 1., 0.], [0.3, 1.2, 0.], [0., 0., 1.], [2., 0.2, 1.1], [2.4, 1., 1.], [0., 1., 1.]],
        [&Division::uniform(3), &Division::uniform(3), &Division::uniform(3)],
    );
    assert!(b.signed_volumes().iter().all(|v| *v > 0.0));
    b.elevate(27, &[], &|_, p| p);
    // Nodes: corners 4^3, edges 3*3*4*4... the structured counts of a 7^3 lattice.
    assert_eq!(b.points.len(), 7 * 7 * 7);
    let mut c = box_mesh([0.0; 3], [1.0; 3], [&Division::uniform(2), &Division::uniform(2), &Division::uniform(2)]);
    c.elevate(20, &[], &|_, p| p);
    assert_eq!(c.points.len(), 27 + 54);
}

/// The same mesh up to the order of its elements (Exodus stores each block's elements together).
fn same_mesh(a: &HexMesh, b: &HexMesh) {
    assert_eq!(a.points.len(), b.points.len());
    for (p, q) in a.points.iter().zip(&b.points) {
        assert!((0..3).all(|k| (p[k] - q[k]).abs() < 1e-12));
    }
    assert_eq!(a.npe, b.npe);
    assert_eq!(a.n_elements(), b.n_elements());
    let index: std::collections::HashMap<&[u32], usize> = (0..b.n_elements()).map(|e| (b.cell(e), e)).collect();
    let map: Vec<usize> = (0..a.n_elements()).map(|e| *index.get(a.cell(e)).unwrap_or_else(|| panic!("element {e} missing"))).collect();
    let (pa, pb) = (a.elem_field("particle_id").unwrap(), b.elem_field("particle_id").unwrap());
    for (e, f) in map.iter().enumerate() {
        assert_eq!(a.block[e], b.block[*f]);
        assert_eq!(pa[e], pb[*f]);
    }
    assert_eq!(a.block_names, b.block_names);
    for (n, f) in &a.side_sets {
        let mut want: Vec<(u32, u8)> = f.iter().map(|(e, s)| (map[*e as usize] as u32, *s)).collect();
        let mut got = b.side_set(n).unwrap_or_else(|| panic!("side set {n}")).clone();
        want.sort_unstable();
        got.sort_unstable();
        assert_eq!(got, want, "side set {n}");
    }
    for (n, v) in &a.node_sets {
        assert_eq!(b.node_set(n), Some(v), "node set {n}");
    }
}

fn read(path: &std::path::Path) -> aurafx_data::DataSet {
    match aurafx_io::read_file(path).unwrap() {
        aurafx_io::Loaded::Static(d) => (*d).clone(),
        aurafx_io::Loaded::Series(s) => (*aurafx_io::load_step(&s.steps[0]).unwrap()).clone(),
    }
}

/// Exodus round trips (blocks, side sets, node sets, HEX27), hexaform's own Exodus file, and the
/// Abaqus and Gmsh files.
#[test]
fn mesh_files() {
    let (grid, placed) = setup();
    let dir = std::env::temp_dir().join(format!("aurafx_hexmesh_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    for npe in [8usize, 27] {
        let m = assemble(&grid, &placed, &BlockOptions::default(), &AssemblyOptions { nodes_per_element: npe, ..Default::default() }).unwrap();
        let ds = m.to_dataset();
        assert_eq!(HexMesh::from_dataset(&ds).map(|b| b.cells).unwrap(), m.cells);
        let path = dir.join(format!("m{npe}.e"));
        aurafx_io::writers::write_file(&ds, &path).unwrap();
        let back = HexMesh::from_dataset(&read(&path)).unwrap();
        same_mesh(&m, &back);
        if npe == 8 {
            // hexaform's own file reads as the same mesh.
            let theirs = HexMesh::from_dataset(&read(&td("four_shapes_hex8.e"))).unwrap();
            same_mesh(&m, &theirs);
            // Abaqus: every node, element, set and surface.
            let p = dir.join("m.inp");
            aurafx_io::writers::write_file(&ds, &p).unwrap();
            let t = std::fs::read_to_string(&p).unwrap();
            assert_eq!(t.lines().filter(|l| l.starts_with("*Element, type=C3D8")).count(), 2);
            assert!(t.contains("elset=matrix") && t.contains("elset=inclusions"));
            assert!(t.contains("*Surface, type=ELEMENT, name=interface") && t.contains("*Nset, nset=xm"));
            let elems: usize = t.split("*Element").skip(1).map(|sec| sec.lines().skip(1).take_while(|l| !l.starts_with('*')).count()).sum();
            assert_eq!(elems, m.n_elements());
            // Gmsh: physical groups for blocks and side sets, all elements.
            let p = dir.join("m.msh");
            aurafx_io::writers::write_file(&ds, &p).unwrap();
            let t = std::fs::read_to_string(&p).unwrap();
            assert!(t.contains("3 1 \"matrix\"") && t.contains("3 100 \"inclusions\"") && t.contains("\"interface\""));
            let el = t.split("$Elements\n").nth(1).unwrap().lines().next().unwrap().to_string();
            let total: usize = el.split_whitespace().nth(1).unwrap().parse().unwrap();
            let faces: usize = m.side_sets.iter().map(|(_, f)| f.len()).sum();
            assert_eq!(total, m.n_elements() + faces);
        }
    }
    std::fs::remove_dir_all(&dir).ok();
}

fn bench(name: &str) -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../testdata/benchmark").join(name)
}

/// Same nodes in the same order and the same elements (any order), blocks and sets.
fn same_as_file(ours: &HexMesh, theirs: &HexMesh) {
    assert_eq!(ours.points.len(), theirs.points.len(), "node count");
    let worst = ours.points.iter().zip(&theirs.points).map(|(p, q)| (0..3).map(|k| (p[k] - q[k]).abs()).fold(0.0, f64::max)).fold(0.0, f64::max);
    assert!(worst < 1e-12, "largest coordinate difference {worst}");
    assert_eq!(ours.n_elements(), theirs.n_elements());
    let index: std::collections::HashMap<&[u32], usize> = (0..theirs.n_elements()).map(|e| (theirs.cell(e), e)).collect();
    let map: Vec<usize> = (0..ours.n_elements()).map(|e| *index.get(ours.cell(e)).expect("element missing")).collect();
    for (e, f) in map.iter().enumerate() {
        assert_eq!(ours.block[e], theirs.block[*f]);
    }
    assert_eq!(ours.block_names, theirs.block_names);
    for (n, f) in &ours.side_sets {
        let mut want: Vec<(u32, u8)> = f.iter().map(|(e, s)| (map[*e as usize] as u32, *s)).collect();
        let mut got = theirs.side_set(n).unwrap_or_else(|| panic!("side set {n}")).clone();
        want.sort_unstable();
        got.sort_unstable();
        assert_eq!(got, want, "side set {n}");
    }
    for (n, v) in &theirs.node_sets {
        assert_eq!(ours.node_set(n), Some(v), "node set {n}");
    }
    assert_eq!(ours.side_sets.len(), theirs.side_sets.len());
}

/// Plates with a circular and a rotated elliptical hole, and a dog-bone with a thickness
/// profile, against the meshes the benchmark study made with hexaform (testdata/benchmark).
#[test]
fn specimens_match_benchmark_meshes() {
    use aurafx_mesh::specimens::*;
    for name in ["plate_h40_hole5", "plate_h40_ell_x2.5_y5_a45"] {
        let j: Value = serde_json::from_str(&std::fs::read_to_string(bench(&format!("{name}.json"))).unwrap()).unwrap();
        let hj = &j["hole"];
        let hole = Hole {
            ellipse: hj["kind"] == "ellipse",
            a: hj["a"].as_f64().unwrap(),
            b: hj["b"].as_f64().unwrap(),
            centre: Some([hj["centre"][0].as_f64().unwrap(), hj["centre"][1].as_f64().unwrap()]),
            angle: hj["angle"].as_f64().unwrap(),
        };
        let o = PlateOptions {
            width: j["width"].as_f64().unwrap(),
            height: j["height"].as_f64().unwrap(),
            thickness: j["thickness"].as_f64().unwrap(),
            h: j["h"].as_f64().unwrap(),
            hole: Some(hole),
            ..Default::default()
        };
        let ours = plate(&o).unwrap();
        let theirs = HexMesh::from_dataset(&read(&bench(&format!("{name}.e")))).unwrap();
        same_as_file(&ours, &theirs);
        assert_eq!(ours.n_elements() as u64, j["n_elements"].as_u64().unwrap());
    }
    let j: Value = serde_json::from_str(&std::fs::read_to_string(bench("dogbone_P1.json")).unwrap()).unwrap();
    let f = |k: &str| j[k].as_f64().unwrap();
    let o = DogboneOptions {
        length: f("length"),
        gage_length: f("gage_length"),
        gage_width: f("gage_width"),
        grip_width: f("grip_width"),
        fillet_radius: f("fillet_radius"),
        grip_thickness: f("grip_thickness"),
        stations: j["stations"].as_array().unwrap().iter().map(|s| (s[0].as_f64().unwrap(), s[1].as_f64().unwrap())).collect(),
        h: f("h"),
        h_grip: Some(f("h_grip")),
        n_z: j["divisions"][2].as_u64().unwrap() as usize,
        origin: [0.0; 3],
    };
    let ours = dogbone(&o).unwrap();
    let theirs = HexMesh::from_dataset(&read(&bench("dogbone_P1.e"))).unwrap();
    same_as_file(&ours, &theirs);
}

/// Sweeps, collective packing and the mesh commands of the geometry language.
#[test]
fn sweep_packing_and_mesh_commands() {
    use aurafx_mesh::structured::*;
    // A disk of quads swept into a cylinder: volume close to pi r^2 h, sets on its faces.
    let (pts, quads) = disk_quads(1.0, 8, 4);
    let m = sweep_quads(&pts, &quads, &vec![1; quads.len()], [0.0, 0.0, 1.0], 2.0, &Division::uniform(4)).unwrap();
    assert!(m.signed_volumes().iter().all(|v| *v > 0.0));
    let v: f64 = m.signed_volumes().iter().sum();
    assert!((v - 2.0 * std::f64::consts::PI).abs() / (2.0 * std::f64::consts::PI) < 0.01, "{v}");
    assert_eq!(m.side_set("bottom").unwrap().len(), quads.len());
    assert_eq!(m.side_set("side").unwrap().len(), 4 * 32);
    // Collective packing never places fewer inclusions than one random pass, and keeps the
    // boxes apart.
    let grid = Grid { div: [24; 3], origin: [0.0; 3], h: 1.0 / 24.0 };
    let spec = PackingSpec { count: 40, radius_mean: 0.06, ..Default::default() };
    let sphere = |r: f64, _: &mut Rng| StarShape::Sphere { centre: [0.0; 3], radius: r };
    let rsa = pack(&grid, &spec, &mut Rng::new(1), &sphere);
    let col = pack_collective(&grid, &spec, &mut Rng::new(1), &sphere, 2000, 0.5, 3);
    assert!(col.len() >= rsa.len() && col.len() > rsa.len(), "{} vs {}", col.len(), rsa.len());
    for (i, a) in col.iter().enumerate() {
        for b in &col[i + 1..] {
            assert!((0..3).any(|x| a.lo[x] + a.k[x] as i64 + 2 <= b.lo[x] || b.lo[x] + b.k[x] as i64 + 2 <= a.lo[x]));
        }
    }
    let mesh = assemble(&grid, &col, &BlockOptions::default(), &AssemblyOptions::default()).unwrap();
    assert!(mesh.signed_volumes().iter().all(|v| *v > 0.0));
    // Mesh commands: a turned brick and a cylinder, blocks and sets.
    let cmds = "brick x 2 y 1 z 1; rotate body 1 angle 30 about z; move body 1 x 3\ncylinder radius 0.5 height 2\nmesh body 1 size 0.25\nmesh body 2 size 0.1 element HEX27\nblock 7 add body 2 name 'rod'\nsideset 1 add face 6 name 'top'\nnodeset 2 add face 1 name 'left'";
    let model = aurafx_mesh::Model::from_commands(cmds, None).unwrap();
    let m = aurafx_mesh::bodymesh::mesh_model(&model).unwrap();
    assert_eq!(m.npe, 27);
    assert_eq!(m.block_names[&7], "rod");
    assert_eq!(m.block_names[&1], "brick 1");
    let nbrick = m.block.iter().filter(|b| **b == 1).count();
    assert_eq!(nbrick, 8 * 4 * 4);
    assert_eq!(m.side_set("top").unwrap().len(), 8 * 4);
    assert_eq!(m.side_set("brick 1 z+").unwrap().len(), 8 * 4);
    assert!(m.side_set("cylinder 2 side").is_some_and(|f| !f.is_empty()));
    assert_eq!(m.node_set("left").unwrap().len(), 5 * 5);
    // HEX27 nodes of the cylinder's side lie on its surface.
    for (e, s) in m.side_set("cylinder 2 side").unwrap() {
        for k in aurafx_mesh::hex::side_nodes(27, *s) {
            let p = m.points[m.cell(*e as usize)[k] as usize];
            assert!(((p[0] * p[0] + p[1] * p[1]).sqrt() - 0.5).abs() < 1e-9, "{p:?}");
        }
    }
    // Other bodies go to the overlay mesher; with HEX27 their face nodes lie on the surface.
    let model = aurafx_mesh::Model::from_commands("sphere radius 1; brick x 1 y 1 z 1; move body 2 x 3; mesh body 1 size 0.2 element HEX27; mesh body 2 size 0.25 scheme overlay; sideset 5 add face 1 name 'ball'", None).unwrap();
    let m = aurafx_mesh::bodymesh::mesh_model(&model).unwrap();
    assert!(m.signed_volumes().iter().all(|v| *v > 0.0));
    let ball = m.side_set("ball").unwrap();
    assert_eq!(ball.len(), m.side_set("sphere 1 surface").unwrap().len());
    for (e, s) in ball {
        for k in aurafx_mesh::hex::side_nodes(27, *s) {
            let p = m.points[m.cell(*e as usize)[k] as usize];
            assert!(((p[0] * p[0] + p[1] * p[1] + p[2] * p[2]).sqrt() - 1.0).abs() < 1e-9, "{p:?}");
        }
    }
    assert!(m.side_set("brick 2 x+").is_some());
    assert!(aurafx_mesh::Model::from_commands("sphere radius 1; mesh body 1 size 0.1 scheme pave", None).is_err());
}
