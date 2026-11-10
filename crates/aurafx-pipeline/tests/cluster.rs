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
