//! Parallel servers: a coordinator with worker processes (here threads) each holding one piece
//! gives the same answers as one process.

use aurafx_data::DataSet;
use aurafx_jobs::{Host, HostKind};
use aurafx_pipeline::remote::{Payload, RemoteClient, ServeOptions};
use aurafx_pipeline::Session;
use std::path::PathBuf;

fn td(name: &str) -> String {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../testdata").join(name).display().to_string()
}

fn cluster_session(n: usize) -> (Session, Host) {
    let workers = (0..n).map(|k| RemoteClient::in_process_opts(&format!("worker {k}"), ServeOptions { piece: Some((k, n)), ..Default::default() })).collect();
    let host = Host { name: "cluster".into(), kind: HostKind::Local, setup: String::new(), server_processes: n as u32, launcher: String::new() };
    let mut s = Session::new();
    s.remote_clients
        .insert(aurafx_pipeline::remote::client_key(&host), RemoteClient::in_process_opts("cluster", ServeOptions { workers, ..Default::default() }));
    (s, host)
}

fn table(ds: &DataSet) -> std::collections::BTreeMap<String, f64> {
    let DataSet::Table(t) = ds else { panic!("not a table") };
    t.columns.iter().map(|c| (c.name.clone(), c.get(0, 0))).collect()
}

fn integrate(ds: &DataSet) -> std::collections::BTreeMap<String, f64> {
    let def = aurafx_filters::find("integrate_variables").unwrap();
    table(&def.run(Some(ds), &def.default_params()).unwrap())
}

fn close(a: f64, b: f64, what: &str) {
    assert!((a - b).abs() <= 1e-9 * b.abs().max(1.0), "{what}: {a} vs {b}");
}

/// The largest relative difference between the point arrays of `x` and of `y` (one process),
/// compared by point position: border points appear in several pieces, and every copy must
/// agree with one process. Every array and point of `y` must be in `x`.
fn worst_by_position(x: &DataSet, y: &DataSet, what: &str) -> f64 {
    let by_pos = |d: &DataSet| {
        let mut m: std::collections::HashMap<(String, [i64; 3]), Vec<Vec<f64>>> = Default::default();
        for (leaf, _, _) in aurafx_filters::selection::flat_leaves(d) {
            let Some(u) = leaf.to_unstructured() else { continue };
            for a in u.point_data.iter() {
                for i in 0..u.points.len() {
                    let key = (a.name.clone(), u.points.get(i).map(|v| (v * 1e9).round() as i64));
                    m.entry(key).or_default().push((0..a.ncomp).map(|c| a.get(i, c)).collect());
                }
            }
        }
        m
    };
    let (px, py) = (by_pos(x), by_pos(y));
    assert!(!py.is_empty(), "{what}: nothing to compare");
    let mut worst = 0.0f64;
    for (k, v) in &py {
        let copies = px.get(k).unwrap_or_else(|| panic!("{what}: missing {k:?}"));
        for w in copies {
            for (a, b) in w.iter().zip(&v[0]) {
                worst = worst.max((a - b).abs() / b.abs().max(1.0));
            }
        }
    }
    worst
}

/// A streamed server: `n` pieces computed one after another, each forgotten after every request.
fn streamed_session(n: usize) -> (Session, Host) {
    let pieces = (0..n)
        .map(|k| RemoteClient::in_process_opts(&format!("piece {k}"), ServeOptions { piece: Some((k, n)), forget: true, ..Default::default() }))
        .collect();
    let host = Host { name: format!("stream {n}"), kind: HostKind::Local, setup: String::new(), server_processes: n as u32, launcher: "stream".into() };
    let mut s = Session::new();
    s.remote_clients.insert(
        aurafx_pipeline::remote::client_key(&host),
        RemoteClient::in_process_opts("stream", ServeOptions { workers: pieces, sequential: true, ..Default::default() }),
    );
    (s, host)
}

#[test]
fn parallel_server_matches_one_process() {
    let (mut s, host) = cluster_session(3);
    let mut local = Session::new();
    let r = s.open_remote(host.clone(), &td("disk_out_ref.ex2"));
    let lr = local.open_file(td("disk_out_ref.ex2")).unwrap();
    s.update_blocking();
    local.update_blocking();
    let n = &s.nodes[&r];
    assert!(n.error.is_none(), "{:?}", n.error);
    let full = local.nodes[&lr].output.clone().unwrap();
    // The gathered reader: every cell once, same values.
    assert_eq!(n.remote.as_ref().unwrap().payload, Payload::Full);
    let got = n.output.clone().unwrap();
    assert_eq!(got.n_cells(), full.n_cells());
    assert_eq!(got.bounds(), full.bounds());
    assert_eq!(n.info.as_ref().unwrap().n_cells, full.n_cells());
    for (k, v) in integrate(&full) {
        close(integrate(&got)[&k], v, &format!("integral of {k} over the gathered reader"));
    }
    let info = s.remote_clients[&aurafx_pipeline::remote::client_key(&host)].info.lock().clone().unwrap();
    assert_eq!(info.processes, 3);

    // Piece-local filters: same geometry, piece by piece.
    for (id, set) in [
        ("slice", serde_json::json!({"normal": [0.0, 0.0, 1.0]})),
        ("contour", serde_json::json!({"array": "Temp", "values": [400.0]})),
        ("clip", serde_json::json!({"normal": [1.0, 1.0, 0.0]})),
    ] {
        let a = s.add_filter(id, r).unwrap();
        let b = local.add_filter(id, lr).unwrap();
        for (k, v) in set.as_object().unwrap() {
            s.set_param_json(a, k, v);
            local.set_param_json(b, k, v);
        }
        s.update_blocking();
        local.update_blocking();
        assert!(s.nodes[&a].error.is_none(), "{id}: {:?}", s.nodes[&a].error);
        let (x, y) = (s.nodes[&a].output.clone().unwrap(), local.nodes[&b].output.clone().unwrap());
        assert_eq!(x.n_cells(), y.n_cells(), "{id} cells");
        for (k, v) in integrate(&y) {
            close(integrate(&x)[&k], v, &format!("{id}: integral of {k}"));
        }
    }

    // A big output comes back as one surface without the faces between pieces.
    s.remote_max_bytes = 1000;
    let t = s.add_filter("tetrahedralize", r).unwrap();
    let lt = local.add_filter("tetrahedralize", lr).unwrap();
    s.update_blocking();
    local.update_blocking();
    assert_eq!(s.nodes[&t].remote.as_ref().unwrap().payload, Payload::Surface);
    let surf = s.nodes[&t].output.clone().unwrap();
    let want = aurafx_filters::surface::extract_surface(local.nodes[&lt].output.as_ref().unwrap()).unwrap();
    let DataSet::PolyData(sp) = &*surf else { panic!() };
    assert_eq!(sp.polys.len(), want.polys.len(), "no faces between pieces");
    close(integrate(&surf)["Area"], integrate(&DataSet::PolyData(want))["Area"], "surface area");
    s.remote_max_bytes = 0;

    // Filters that need the whole data run on the coordinator on the gathered pieces.
    let i = s.add_filter("integrate_variables", r).unwrap();
    let li = local.add_filter("integrate_variables", lr).unwrap();
    s.update_blocking();
    local.update_blocking();
    assert!(s.nodes[&i].error.is_none(), "{:?}", s.nodes[&i].error);
    let (x, y) = (table(s.nodes[&i].output.as_ref().unwrap()), table(local.nodes[&li].output.as_ref().unwrap()));
    for (k, v) in y {
        close(x[&k], v, &format!("Integrate Variables: {k}"));
    }
    // ... and follow parameter changes upstream.
    let sl = s.nodes.values().find(|n| n.def().is_some_and(|d| d.id == "slice")).unwrap().id;
    let isl = s.add_filter("integrate_variables", sl).unwrap();
    s.update_blocking();
    let a1 = table(s.nodes[&isl].output.as_ref().unwrap())["Area"];
    s.set_param_json(sl, "normal", &serde_json::json!([1.0, 0.0, 0.0]));
    s.update_blocking();
    let a2 = table(s.nodes[&isl].output.as_ref().unwrap())["Area"];
    assert!((a1 - a2).abs() > 1e-6, "the gathered input was refreshed");
}

#[test]
fn partitioned_files_are_read_piece_by_piece() {
    // A .pvtu with four piece files: each of two processes reads two of them.
    let img = DataSet::ImageData(aurafx_filters::sources::wavelet([-6, 6, -6, 6, -6, 6], 255.0, 0.5));
    let ug = DataSet::UnstructuredGrid(img.to_unstructured().unwrap());
    let dir = std::env::temp_dir().join(format!("aurafx_pvtu_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let mut entries = String::new();
    let mut piece_cells = Vec::new();
    for k in 0..4 {
        let p = aurafx_data::pieces::split(&ug, k, 4);
        piece_cells.push(p.n_cells());
        aurafx_io::writers::write_file(&p, &dir.join(format!("w_{k}.vtu"))).unwrap();
        entries += &format!("<Piece Source=\"w_{k}.vtu\"/>");
    }
    std::fs::write(
        dir.join("w.pvtu"),
        format!("<?xml version=\"1.0\"?><VTKFile type=\"PUnstructuredGrid\" version=\"1.0\"><PUnstructuredGrid GhostLevel=\"0\"><PPoints><PDataArray type=\"Float64\" NumberOfComponents=\"3\"/></PPoints>{entries}</PUnstructuredGrid></VTKFile>"),
    )
    .unwrap();
    let path = dir.join("w.pvtu");
    for k in 0..2 {
        let aurafx_io::Loaded::Static(d) = aurafx_io::read_file_piece(&path, Some((k, 2))).unwrap() else { panic!() };
        assert_eq!(d.n_cells(), piece_cells[2 * k] + piece_cells[2 * k + 1], "process {k} read its two piece files");
    }
    let (mut s, host) = cluster_session(2);
    let r = s.open_remote(host, &path.display().to_string());
    let c = s.add_filter("contour", r).unwrap();
    s.set_param_json(c, "values", &serde_json::json!([150.0]));
    s.update_blocking();
    let mut local = Session::new();
    let lr = local.open_file(&path).unwrap();
    let lc = local.add_filter("contour", lr).unwrap();
    local.set_param_json(lc, "values", &serde_json::json!([150.0]));
    local.update_blocking();
    close(integrate(s.nodes[&c].output.as_ref().unwrap())["Area"], integrate(local.nodes[&lc].output.as_ref().unwrap())["Area"], "contour over the pieces");
    let _ = std::fs::remove_dir_all(&dir);
}

/// Reader options reach every process of a parallel server: each worker reads its piece with
/// them, and the pieces together are the whole file read with the option.
#[test]
fn parallel_server_reader_options() {
    let (mut s, host) = cluster_session(3);
    let mut local = Session::new();
    let r = s.open_remote(host.clone(), &td("netcdf/mpas_ocean.nc"));
    let lr = local.open_file(td("netcdf/mpas_ocean.nc")).unwrap();
    // A server node learns its time steps from the first answer.
    s.update_blocking();
    for sess in [&mut s, &mut local] {
        let id = *sess.nodes.keys().next().unwrap();
        sess.set_param(id, "vertical_level", aurafx_filters::ParamValue::Int(3));
        sess.set_time_index(1);
        sess.update_blocking();
    }
    assert!(s.nodes[&r].error.is_none(), "{:?}", s.nodes[&r].error);
    let (a, b) = (s.nodes[&r].output.clone().unwrap(), local.nodes[&lr].output.clone().unwrap());
    assert_eq!(a.n_cells(), b.n_cells());
    let range = |d: &DataSet| d.point_data().unwrap().get("temperature").unwrap().range(None);
    assert_eq!(range(&a), range(&b));
    assert_eq!(range(&b), (-14.967137336730957, 15.5), "level 3 at step 1");
    assert_eq!(s.reader_specs(r).map(|v| v.len()), Some(6));
}

/// Filters that look at neighbours run on every piece with ghost cells from the neighbouring
/// pieces, and match one process at the piece borders: an explicit mesh (Exodus, several
/// blocks), an image (wavelet) and a surface (normals, curvature).
#[test]
fn neighbour_filters_match_one_process() {
    let (mut s, host) = cluster_session(4);
    let mut local = Session::new();
    let cases: [(&str, &str, serde_json::Value); 7] = [
        ("wavelet.vti", "gradient", serde_json::json!({"array": {"assoc": "Points", "name": "RTData"}})),
        ("wavelet.vti", "cell_data_to_point_data", serde_json::json!({})),
        ("disk_out_ref.ex2", "gradient", serde_json::json!({"array": {"assoc": "Points", "name": "Temp"}})),
        ("disk_out_ref.ex2", "cell_data_to_point_data", serde_json::json!({})),
        ("wavelet_tets.vtu", "gradient", serde_json::json!({"array": {"assoc": "Points", "name": "RTData"}})),
        ("contour.vtp", "surface_normals", serde_json::json!({})),
        ("contour.vtp", "curvature", serde_json::json!({})),
    ];
    for (file, id, set) in cases {
        let r = s.open_remote(host.clone(), &td(file));
        let lr = local.open_file(td(file)).unwrap();
        let a = s.add_filter(id, r).unwrap();
        let b = local.add_filter(id, lr).unwrap();
        for (k, v) in set.as_object().unwrap() {
            s.set_param_json(a, k, v);
            local.set_param_json(b, k, v);
        }
        s.update_blocking();
        local.update_blocking();
        assert!(s.nodes[&a].error.is_none(), "{file} {id}: {:?}", s.nodes[&a].error);
        let (x, y) = (s.nodes[&a].output.clone().unwrap(), local.nodes[&b].output.clone().unwrap());
        // Explicit pieces repeat the points on their borders; cells appear once.
        assert_eq!(x.n_cells(), y.n_cells(), "{file} {id}: cells");
        let worst = worst_by_position(&x, &y, &format!("{file} {id}"));
        assert!(worst < 1e-9, "{file} {id}: values differ by {worst}");
    }
}

/// The workers keep the ghost cells of the most recent inputs only, and may lose them (a
/// restarted worker): many different inputs one after another, back to the first, and a
/// request after every worker dropped its ghost cells all give what one process gives.
#[test]
fn ghost_cells_are_sent_again_when_needed() {
    let (mut s, host) = cluster_session(4);
    let mut local = Session::new();
    let r = s.open_remote(host.clone(), &td("wavelet.vti"));
    let lr = local.open_file(td("wavelet.vti")).unwrap();
    let result = serde_json::json!({"assoc": "Points", "name": "Result"});
    let (c, lc) = (s.add_filter("calculator", r).unwrap(), local.add_filter("calculator", lr).unwrap());
    let (g, lg) = (s.add_filter("gradient", c).unwrap(), local.add_filter("gradient", lc).unwrap());
    s.set_param_json(g, "array", &result);
    local.set_param_json(lg, "array", &result);
    // More inputs than the workers keep ghost cells for, then the first again.
    let n = aurafx_pipeline::cluster::KEPT + 6;
    for i in (1..=n).chain([1]) {
        s.set_param_json(c, "expression", &serde_json::json!(format!("RTData*{i}")));
        s.update_blocking();
        assert!(s.nodes[&g].error.is_none(), "input {i}: {:?}", s.nodes[&g].error);
    }
    local.set_param_json(lc, "expression", &serde_json::json!("RTData*1"));
    local.update_blocking();
    let (x, y) = (s.nodes[&g].output.clone().unwrap(), local.nodes[&lg].output.clone().unwrap());
    assert_eq!(x.n_cells(), y.n_cells());
    let worst = worst_by_position(&x, &y, "gradient after many inputs");
    assert!(worst < 1e-9, "gradient after many inputs: values differ by {worst}");
    // Every worker loses its ghost cells; another neighbour filter of the same input (whose
    // ghost cells the coordinator believes they hold) still works.
    aurafx_filters::ghost_exchange::forget_all();
    let (p, lp) = (s.add_filter("point_data_to_cell_data", c).unwrap(), local.add_filter("point_data_to_cell_data", lc).unwrap());
    let (q, lq) = (s.add_filter("cell_data_to_point_data", p).unwrap(), local.add_filter("cell_data_to_point_data", lp).unwrap());
    let (g2, lg2) = (s.add_filter("gradient", c).unwrap(), local.add_filter("gradient", lc).unwrap());
    s.set_param_json(g2, "array", &serde_json::json!({"assoc": "Points", "name": "RTData"}));
    local.set_param_json(lg2, "array", &serde_json::json!({"assoc": "Points", "name": "RTData"}));
    s.update_blocking();
    local.update_blocking();
    for (what, a, b) in [("cell data to point data", q, lq), ("a second gradient", g2, lg2)] {
        assert!(s.nodes[&a].error.is_none(), "{what} after the ghost cells were lost: {:?}", s.nodes[&a].error);
        let (x, y) = (s.nodes[&a].output.clone().unwrap(), local.nodes[&b].output.clone().unwrap());
        assert_eq!(x.n_cells(), y.n_cells(), "{what}");
        let worst = worst_by_position(&x, &y, what);
        assert!(worst < 1e-9, "{what}: values differ by {worst}");
    }
    // Deleting a neighbour filter is passed on to the workers without trouble.
    s.remove_node(g);
    s.update_blocking();
    assert!(s.nodes[&q].error.is_none());
}

/// Surface Normals and Curvature of data that is not a surface take the outer surface first,
/// which needs the whole data: they run on the gathered pieces and match one process.
#[test]
fn surface_filters_of_volumes_match_one_process() {
    let (mut s, host) = cluster_session(4);
    let mut local = Session::new();
    for (file, id) in [("wavelet_tets.vtu", "surface_normals"), ("disk_out_ref.ex2", "surface_normals"), ("wavelet.vti", "curvature")] {
        let r = s.open_remote(host.clone(), &td(file));
        let lr = local.open_file(td(file)).unwrap();
        let (a, b) = (s.add_filter(id, r).unwrap(), local.add_filter(id, lr).unwrap());
        s.update_blocking();
        local.update_blocking();
        assert!(s.nodes[&a].error.is_none(), "{file} {id}: {:?}", s.nodes[&a].error);
        let (x, y) = (s.nodes[&a].output.clone().unwrap(), local.nodes[&b].output.clone().unwrap());
        assert_eq!(x.n_cells(), y.n_cells(), "{file} {id}: cells");
        close(integrate(&x)["Area"], integrate(&y)["Area"], &format!("{file} {id}: area"));
    }
}

/// More pieces than a process ever kept ghost cells for, computed one after another: each
/// piece gets its ghost cells just before it runs, and the gradient equals one process.
#[test]
fn many_streamed_pieces_get_their_ghost_cells() {
    let n = 70;
    let img = DataSet::ImageData(aurafx_filters::sources::wavelet([-2, 2, -2, 2, 0, 2 * n as i64], 255.0, 0.5));
    let dir = std::env::temp_dir().join(format!("aurafx_stream70_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("long.vti");
    aurafx_io::writers::write_file(&img, &path).unwrap();
    let (mut s, host) = streamed_session(n);
    let mut local = Session::new();
    let r = s.open_remote(host.clone(), &path.display().to_string());
    let lr = local.open_file(&path).unwrap();
    let rt = serde_json::json!({"assoc": "Points", "name": "RTData"});
    let (g, lg) = (s.add_filter("gradient", r).unwrap(), local.add_filter("gradient", lr).unwrap());
    s.set_param_json(g, "array", &rt);
    local.set_param_json(lg, "array", &rt);
    s.update_blocking();
    local.update_blocking();
    assert!(s.nodes[&g].error.is_none(), "{:?}", s.nodes[&g].error);
    let (x, y) = (s.nodes[&g].output.clone().unwrap(), local.nodes[&lg].output.clone().unwrap());
    assert_eq!(x.n_cells(), y.n_cells());
    assert!(x.point_data().is_some_and(|p| p.iter().count() > y.point_data().unwrap().iter().count() - 1));
    let worst = worst_by_position(&x, &y, "streamed gradient");
    assert!(worst < 1e-9, "streamed gradient: values differ by {worst}");
    let _ = std::fs::remove_dir_all(&dir);
}

/// Memory of a streamed server on a big file (set AFX_BIG to a large .vti; run with --ignored).
#[test]
#[ignore]
fn streamed_memory() {
    let Ok(big) = std::env::var("AFX_BIG") else { return };
    let hwm = || std::fs::read_to_string("/proc/self/status").unwrap().lines().find(|l| l.starts_with("VmHWM")).unwrap().to_string();
    let rss = || std::fs::read_to_string("/proc/self/status").unwrap().lines().find(|l| l.starts_with("VmRSS")).unwrap().to_string();
    let n = 16;
    let pieces = (0..n)
        .map(|k| RemoteClient::in_process_opts(&format!("piece {k}"), ServeOptions { piece: Some((k, n)), forget: true, ..Default::default() }))
        .collect();
    let host = Host { name: "stream".into(), kind: HostKind::Local, setup: String::new(), server_processes: n as u32, launcher: "stream".into() };
    let mut s = Session::new();
    s.remote_clients.insert(
        aurafx_pipeline::remote::client_key(&host),
        RemoteClient::in_process_opts("stream", ServeOptions { workers: pieces, sequential: true, ..Default::default() }),
    );
    let t = std::time::Instant::now();
    let r = s.open_remote(host.clone(), &big);
    s.update_blocking();
    println!("open: {:.1} s {} {} payload {:?}", t.elapsed().as_secs_f64(), hwm(), rss(), s.nodes[&r].remote.as_ref().map(|x| x.payload));
    let sl = s.add_filter("slice", r).unwrap();
    s.set_param_json(sl, "normal", &serde_json::json!([0.0, 0.0, 1.0]));
    s.update_blocking();
    println!(
        "slice: {:.1} s {} {} cells {:?} {:?} {:?}",
        t.elapsed().as_secs_f64(),
        hwm(),
        rss(),
        s.nodes[&sl].output.as_ref().map(|o| o.n_cells()),
        s.nodes[&sl].remote.as_ref().map(|x| x.payload),
        s.nodes[&sl].error
    );
    let c = s.add_filter("contour", r).unwrap();
    s.set_param_json(c, "values", &serde_json::json!([0.5]));
    s.update_blocking();
    println!(
        "contour: {:.1} s {} {} cells {:?} {:?}",
        t.elapsed().as_secs_f64(),
        hwm(),
        rss(),
        s.nodes[&c].output.as_ref().map(|o| o.n_cells()),
        s.nodes[&c].error
    );
}

/// A streamed server (pieces in turn, each forgotten after every request) gives the same
/// surface, slice and contour as one process.
#[test]
fn streamed_pieces_match_one_process() {
    let (mut s, host) = streamed_session(5);
    s.remote_max_bytes = 1000; // the reader comes back as its surface
    let mut local = Session::new();
    let r = s.open_remote(host.clone(), &td("wavelet.vti"));
    let lr = local.open_file(td("wavelet.vti")).unwrap();
    // The reader alone comes back as its outer surface, merged piece by piece.
    s.update_blocking();
    local.update_blocking();
    let surf = s.nodes[&r].output.clone().unwrap();
    let want = aurafx_filters::surface::extract_surface(local.nodes[&lr].output.as_ref().unwrap()).unwrap();
    assert_eq!(surf.n_cells(), want.polys.len(), "surface: no faces between pieces");
    close(integrate(&surf)["Area"], integrate(&DataSet::PolyData(want))["Area"], "surface area");
    let mut pairs = Vec::new();
    for (id, k, v) in [("slice", "normal", serde_json::json!([0.0, 0.0, 1.0])), ("contour", "values", serde_json::json!([150.0]))] {
        let a = s.add_filter(id, r).unwrap();
        let b = local.add_filter(id, lr).unwrap();
        s.set_param_json(a, k, &v);
        local.set_param_json(b, k, &v);
        pairs.push((id, a, b));
    }
    s.update_blocking();
    local.update_blocking();
    for (id, a, b) in pairs {
        assert!(s.nodes[&a].error.is_none(), "{id}: {:?}", s.nodes[&a].error);
        let x = s.nodes[&a].output.clone().unwrap();
        let y = local.nodes[&b].output.clone().unwrap();
        assert_eq!(x.n_cells(), y.n_cells(), "{id}: cells");
        for (k, v) in integrate(&y) {
            close(integrate(&x)[&k], v, &format!("{id}: integral of {k}"));
        }
    }
    // Neighbour filters, alone and one after another (each needs ghost cells of its own
    // input): every value equals one process, also on the borders between pieces.
    s.remote_max_bytes = 0;
    let rt = serde_json::json!({"assoc": "Points", "name": "RTData"});
    let chain = |sess: &mut Session, r| {
        let g = sess.add_filter("gradient", r).unwrap();
        sess.set_param_json(g, "array", &rt);
        let p = sess.add_filter("point_data_to_cell_data", r).unwrap();
        let q = sess.add_filter("cell_data_to_point_data", p).unwrap();
        let g2 = sess.add_filter("gradient", q).unwrap();
        sess.set_param_json(g2, "array", &rt);
        [g, g2]
    };
    let (a, b) = (chain(&mut s, r), chain(&mut local, lr));
    s.update_blocking();
    local.update_blocking();
    for (what, a, b) in [("gradient", a[0], b[0]), ("gradient of smoothed data", a[1], b[1])] {
        assert!(s.nodes[&a].error.is_none(), "{what}: {:?}", s.nodes[&a].error);
        let (x, y) = (s.nodes[&a].output.clone().unwrap(), local.nodes[&b].output.clone().unwrap());
        assert_eq!(x.n_cells(), y.n_cells(), "{what}: cells");
        let worst = worst_by_position(&x, &y, what);
        assert!(worst < 1e-9, "{what}: values differ by {worst}");
    }
}
