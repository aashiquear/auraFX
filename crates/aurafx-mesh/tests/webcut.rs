//! webcut, imprint and merge in the geometry commands.
use aurafx_mesh::bodymesh::mesh_model;
use aurafx_mesh::model::Model;

/// A hexahedron's volume: five tetrahedra of its corners (VTK order).
fn hex_volume(m: &aurafx_mesh::hex::HexMesh, e: usize) -> f64 {
    let c: Vec<[f64; 3]> = m.cell(e)[..8].iter().map(|k| m.points[*k as usize]).collect();
    let tet = |a: usize, b: usize, cc: usize, d: usize| {
        let (u, v, w) = (
            [c[b][0] - c[a][0], c[b][1] - c[a][1], c[b][2] - c[a][2]],
            [c[cc][0] - c[a][0], c[cc][1] - c[a][1], c[cc][2] - c[a][2]],
            [c[d][0] - c[a][0], c[d][1] - c[a][1], c[d][2] - c[a][2]],
        );
        (u[0] * (v[1] * w[2] - v[2] * w[1]) - u[1] * (v[0] * w[2] - v[2] * w[0]) + u[2] * (v[0] * w[1] - v[1] * w[0])).abs() / 6.0
    };
    tet(0, 1, 3, 4) + tet(1, 2, 3, 6) + tet(1, 4, 5, 6) + tet(3, 4, 6, 7) + tet(1, 3, 4, 6)
}

fn model(cmds: &str) -> Model {
    let mut m = Model::new();
    m.run(cmds).unwrap();
    m
}

#[test]
fn a_brick_webcut_along_its_axis_gives_two_structured_bricks_that_meet_node_to_node() {
    // A unit brick (centred at 0.5) cut at x = 0.8: 0.3 and 0.7 wide.
    let m = model("brick x 1 y 1 z 1\nmove body 1 x 0.5 y 0.5 z 0.5\nwebcut body 1 with plane xplane offset 0.8\nmesh body 1 2 size 0.1");
    assert_eq!(m.bodies.len(), 2);
    let mesh = mesh_model(&m).unwrap();
    // The whole brick's nodes: 11³, the cut layer shared.
    assert_eq!(mesh.points.len(), 11 * 11 * 11);
    assert_eq!(mesh.n_elements(), 1000);
    // The two halves are two blocks with an interface set of 10 x 10 faces on x = 0.8.
    let iface = mesh.side_sets.iter().find(|s| s.0.starts_with("interface ")).expect("an interface side set");
    assert_eq!(iface.1.len(), 100);
    for (e, s) in &iface.1 {
        let c = mesh.side_corners(*e as usize, *s);
        assert!(c.iter().all(|k| (mesh.points[*k as usize][0] - 0.8).abs() < 1e-9));
    }
    // The cut face is named and both bodies' outer faces keep their names.
    assert!(m.faces.iter().any(|f| f == "webcut 1"));
    assert!(mesh.side_set("brick 1 x-").is_some() && mesh.side_set("brick 1 x+").is_some());
}

#[test]
fn a_turned_brick_is_cut_along_its_own_axis_and_a_slanted_cut_goes_to_the_overlay_mesher() {
    // Turned 90 degrees about z: its own x axis is the world's y axis.
    let m = model("brick x 2 y 1 z 1\nrotate body 1 angle 90 about z\nwebcut body 1 with plane yplane offset 0.25\nmesh body 1 2 size 0.25");
    let mesh = mesh_model(&m).unwrap();
    // 8 x 4 x 4 cells in all; the cut lands on the 0.25 node layer, so the halves meet node to node.
    assert_eq!(mesh.n_elements(), 8 * 4 * 4);
    assert_eq!(mesh.points.len(), 9 * 5 * 5);
    // A slanted plane makes general pieces, meshed together on one overlay grid (touching node to node).
    // (offset a little: a plane exactly through a row of grid-cell centres gives those cells to the first body)
    let m = model("brick x 1 y 1 z 1\nwebcut body 1 with plane normal 1 1 0 offset 0.013\nmesh body 1 2 size 0.1");
    let mesh = mesh_model(&m).unwrap();
    assert!(mesh.side_sets.iter().any(|s| s.0.starts_with("interface ")), "{:?}", mesh.side_sets.iter().map(|s| &s.0).collect::<Vec<_>>());
    let vol: f64 = (0..mesh.n_elements()).map(|e| hex_volume(&mesh, e)).sum();
    // The overlay grid rounds sharp edges a little: compare with the uncut brick on the same grid.
    let whole = mesh_model(&model("brick x 1 y 1 z 1\nmesh body 1 size 0.1 scheme overlay")).unwrap();
    let vw: f64 = (0..whole.n_elements()).map(|e| hex_volume(&whole, e)).sum();
    // The pieces have 45-degree wedge edges, which the grid rounds more than right angles: within 5 %.
    assert!((vol - vw).abs() < 0.05 * vw, "cut {vol}, whole {vw}");
    // The side x + y <= 0.013 √2 holds 1 - (1 - s)²/2 of the brick (s = 0.013 √2).
    let s2 = 0.013 * 2f64.sqrt();
    let share = 1.0 - (1.0 - s2).powi(2) / 2.0;
    let mut pieces = std::collections::BTreeMap::new();
    for e in 0..mesh.n_elements() {
        *pieces.entry(mesh.block[e]).or_insert(0.0) += hex_volume(&mesh, e);
    }
    let v1 = pieces.values().next().copied().unwrap();
    assert!((v1 / vol - share).abs() < 0.03, "body 1 holds {} of the volume, {share} expected", v1 / vol);
}

#[test]
fn imprinted_bricks_of_different_sizes_are_meshed_alike_and_merge_node_to_node() {
    // Two touching unit bricks asked for different sizes.
    let cmds = "brick x 1 y 1 z 1\nbrick x 1 y 1 z 1\nmove body 2 x 1\nmesh body 1 size 0.25\nmesh body 2 size 0.2";
    // Without imprint or merge the shared face does not match: 5 x 5 against 4 x 4 cells, no shared nodes.
    let plain = mesh_model(&model(cmds)).unwrap();
    // (only the shared face's four corners coincide)
    assert_eq!(plain.points.len(), 6 * 6 * 6 + 5 * 5 * 5 - 4);
    // Merged, but one brick shifted by a fraction of a cell: no size makes their nodes meet, and merge says so.
    let mut m = model("brick x 1 y 1 z 1\nbrick x 1 y 1 z 1\nmove body 2 x 1 y 0.1\nmesh body 1 2 size 0.25");
    m.run("merge body 1 2").unwrap();
    let e = mesh_model(&m).unwrap_err();
    assert!(e.contains("do not meet node to node"), "{e}");
    // Imprinted, they take the smaller size and share the face's nodes: the union's node count.
    let mut m = model(cmds);
    m.run("imprint body 1 2\nmerge body 1 2").unwrap();
    let mesh = mesh_model(&m).unwrap();
    assert_eq!(mesh.points.len(), 11 * 6 * 6, "two 5 x 5 x 5 bricks share a 6 x 6 face of nodes");
    let iface = mesh.side_sets.iter().find(|s| s.0.starts_with("interface ")).unwrap();
    assert_eq!(iface.1.len(), 25);
    // The interface goes to the Exodus file as a side set.
    let ds = mesh.to_dataset();
    let p = std::env::temp_dir().join(format!("afx_webcut_{}.e", std::process::id()));
    aurafx_io::writers::write_file(&ds, &p).unwrap();
    let back = aurafx_io::read_file(&p).unwrap();
    let aurafx_io::Loaded::Static(d) = back else { panic!() };
    let names: Vec<String> = d.field_data().iter().map(|a| a.name.clone()).collect();
    assert!(names.iter().any(|n| n.starts_with("SideSet interface")), "{names:?}");
    std::fs::remove_file(&p).ok();
}

#[test]
fn a_cylinder_webcut_cores_a_brick() {
    let m = model("brick x 2 y 2 z 1\nwebcut body 1 with cylinder radius 0.5 axis z\nmesh body 1 2 size 0.08");
    assert_eq!(m.bodies.len(), 2);
    let mesh = mesh_model(&m).unwrap();
    let mut vol = [0.0f64; 2];
    for e in 0..mesh.n_elements() {
        let k = usize::from(mesh.block[e] != mesh.block[0]);
        vol[k] += hex_volume(&mesh, e);
    }
    let core = std::f64::consts::PI * 0.25;
    let (a, b) = if vol[0] < vol[1] { (vol[0], vol[1]) } else { (vol[1], vol[0]) };
    assert!((a - core).abs() < 0.05 * core && (b - (4.0 - core)).abs() < 0.05 * (4.0 - core), "{vol:?}");
    assert!(mesh.side_sets.iter().any(|s| s.0.starts_with("interface ")));
}
