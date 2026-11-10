//! The real server executable, started the way the app starts it on a host (through
//! `bash -lc`, here on this machine). This computer as a server needs a Unix shell; from
//! Windows the app reaches servers over ssh.
#![cfg(unix)]

use aurafx_jobs::{Host, HostKind};
use aurafx_pipeline::remote::{Payload, RemoteClient};
use aurafx_pipeline::Session;

#[test]
fn server_process_runs_a_pipeline() {
    std::env::set_var("AURAFX_SERVER", env!("CARGO_BIN_EXE_aurafx-server"));
    let td = |n: &str| std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../testdata").join(n).display().to_string();
    let host = Host { name: "this computer".into(), kind: HostKind::Local, setup: String::new(), server_processes: 0, launcher: String::new() };
    let mut s = Session::new();
    let r = s.open_remote(host.clone(), &td("disk_out_ref.ex2"));
    let c = s.add_filter("contour", r).unwrap();
    s.update_blocking();
    assert!(s.nodes[&c].error.is_none(), "{:?}", s.nodes[&c].error);
    assert_eq!(s.nodes[&c].remote.as_ref().unwrap().payload, Payload::Full);
    let mut local = Session::new();
    let lr = local.open_file(td("disk_out_ref.ex2")).unwrap();
    let lc = local.add_filter("contour", lr).unwrap();
    local.update_blocking();
    assert_eq!(s.nodes[&c].output, local.nodes[&lc].output);
    let info = s.remote_clients["this computer"].info.lock().clone().unwrap();
    assert_eq!(info.protocol, aurafx_pipeline::remote::PROTOCOL);
    assert!(info.cpus >= 1);

    // A missing server gives a readable error.
    let c2 = RemoteClient::for_host(Host {
        name: "nowhere".into(),
        kind: HostKind::Local,
        setup: "export AURAFX_SERVER=/nonexistent/aurafx-server".into(),
        server_processes: 0,
        launcher: String::new(),
    });
    let err = c2.request(&aurafx_pipeline::remote::Request::Hello { protocol: 1 }).unwrap_err();
    assert!(format!("{err:#}").contains("nonexistent"), "{err:#}");
}

#[test]
fn parallel_server_processes_match_one_process() {
    std::env::set_var("AURAFX_SERVER", env!("CARGO_BIN_EXE_aurafx-server"));
    let td = |n: &str| std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../testdata").join(n).display().to_string();
    let host = Host { name: "this computer".into(), kind: HostKind::Local, setup: String::new(), server_processes: 3, launcher: String::new() };
    let mut s = Session::new();
    let r = s.open_remote(host.clone(), &td("disk_out_ref.ex2"));
    let c = s.add_filter("contour", r).unwrap();
    s.set_param_json(c, "array", &serde_json::json!("Temp"));
    s.update_blocking();
    assert!(s.nodes[&c].error.is_none(), "{:?}", s.nodes[&c].error);
    let info = s.remote_clients[&aurafx_pipeline::remote::client_key(&host)].info.lock().clone().unwrap();
    assert_eq!(info.processes, 3);
    let mut local = Session::new();
    let lr = local.open_file(td("disk_out_ref.ex2")).unwrap();
    let lc = local.add_filter("contour", lr).unwrap();
    local.set_param_json(lc, "array", &serde_json::json!("Temp"));
    local.update_blocking();
    let area = |ds: &aurafx_data::DataSet| {
        let def = aurafx_filters::find("integrate_variables").unwrap();
        let aurafx_data::DataSet::Table(t) = def.run(Some(ds), &def.default_params()).unwrap() else { panic!() };
        t.column("Area").unwrap().get(0, 0)
    };
    let (a, b) = (area(s.nodes[&c].output.as_ref().unwrap()), area(local.nodes[&lc].output.as_ref().unwrap()));
    assert!((a - b).abs() < 1e-9 * b, "contour area {a} vs {b}");
}

/// Workers started by an MPI-style launcher (here a script that starts each rank with Open MPI's
/// rank and size variables, as mpirun does) connect back and give the same contour.
#[test]
fn ranks_from_a_launcher_match_one_process() {
    std::env::set_var("AURAFX_SERVER", env!("CARGO_BIN_EXE_aurafx-server"));
    std::env::set_var("AURAFX_CONNECT_HOST", "127.0.0.1");
    let dir = std::env::temp_dir().join(format!("aurafx_fake_mpirun_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let script = dir.join("fake-mpirun");
    std::fs::write(
        &script,
        "#!/bin/sh\n# fake-mpirun -np N CMD...: N copies of CMD with Open MPI's rank variables\nn=$2; shift 2\ni=0\nwhile [ $i -lt $n ]; do OMPI_COMM_WORLD_RANK=$i OMPI_COMM_WORLD_SIZE=$n sh -c \"$*\" & i=$((i+1)); done\nwait\n",
    )
    .unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    let td = |n: &str| std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../testdata").join(n).display().to_string();
    let host = Host {
        name: "mpi cluster".into(),
        kind: HostKind::Local,
        setup: String::new(),
        server_processes: 3,
        launcher: format!("{} -np {{n}} {{cmd}}", script.display()),
    };
    let mut s = Session::new();
    let r = s.open_remote(host.clone(), &td("disk_out_ref.ex2"));
    let c = s.add_filter("contour", r).unwrap();
    s.set_param_json(c, "array", &serde_json::json!("Temp"));
    s.update_blocking();
    assert!(s.nodes[&c].error.is_none(), "{:?}", s.nodes[&c].error);
    let info = s.remote_clients[&aurafx_pipeline::remote::client_key(&host)].info.lock().clone().unwrap();
    assert_eq!(info.processes, 3);
    let mut local = Session::new();
    let lr = local.open_file(td("disk_out_ref.ex2")).unwrap();
    let lc = local.add_filter("contour", lr).unwrap();
    local.set_param_json(lc, "array", &serde_json::json!("Temp"));
    local.update_blocking();
    let area = |ds: &aurafx_data::DataSet| {
        let def = aurafx_filters::find("integrate_variables").unwrap();
        let aurafx_data::DataSet::Table(t) = def.run(Some(ds), &def.default_params()).unwrap() else { panic!() };
        t.column("Area").unwrap().get(0, 0)
    };
    let (a, b) = (area(s.nodes[&c].output.as_ref().unwrap()), area(local.nodes[&lc].output.as_ref().unwrap()));
    assert!((a - b).abs() < 1e-9 * b, "contour area {a} vs {b}");
    drop(s);
    let _ = std::fs::remove_dir_all(&dir);
}

/// A file larger than memory, streamed: one server process reads it in pieces one after
/// another (VTK XML files read only each piece's bytes), and the results equal one process:
/// the reader's surface, a contour, a slice and a gradient (with ghost cells between pieces).
#[test]
fn streamed_pieces_match_one_process() {
    std::env::set_var("AURAFX_SERVER", env!("CARGO_BIN_EXE_aurafx-server"));
    let td = |n: &str| std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../testdata").join(n).display().to_string();
    let host = Host { name: "this computer".into(), kind: HostKind::Local, setup: String::new(), server_processes: 5, launcher: "stream".into() };
    assert_eq!(aurafx_pipeline::remote::client_key(&host), "this computer ×5 streamed");
    for file in ["wavelet_tets.vtu", "wavelet.vti"] {
        let mut s = Session::new();
        let mut local = Session::new();
        let r = s.open_remote(host.clone(), &td(file));
        let lr = local.open_file(td(file)).unwrap();
        let rt = serde_json::json!({"assoc": "Points", "name": "RTData"});
        let mut pairs = Vec::new();
        for (id, k, v) in
            [("contour", "values", serde_json::json!([150.0])), ("slice", "normal", serde_json::json!([0.0, 1.0, 0.0])), ("gradient", "array", rt)]
        {
            let a = s.add_filter(id, r).unwrap();
            let b = local.add_filter(id, lr).unwrap();
            s.set_param_json(a, k, &v);
            local.set_param_json(b, k, &v);
            pairs.push((id, a, b));
        }
        s.update_blocking();
        local.update_blocking();
        let info = s.remote_clients[&aurafx_pipeline::remote::client_key(&host)].info.lock().clone().unwrap();
        assert_eq!(info.processes, 5);
        for (id, a, b) in pairs {
            assert!(s.nodes[&a].error.is_none(), "{file} {id}: {:?}", s.nodes[&a].error);
            let (x, y) = (s.nodes[&a].output.clone().unwrap(), local.nodes[&b].output.clone().unwrap());
            assert_eq!(x.n_cells(), y.n_cells(), "{file} {id}: cells");
            let range = |d: &aurafx_data::DataSet, n: &str| {
                let u = d.to_unstructured().unwrap();
                u.point_data.get(n).map(|a| a.range(None)).unwrap_or((0.0, 0.0))
            };
            for n in ["RTData", "RTData_gradient"] {
                let (p, q) = (range(&x, n), range(&y, n));
                assert!((p.0 - q.0).abs() < 1e-6 * q.0.abs().max(1.0) && (p.1 - q.1).abs() < 1e-6 * q.1.abs().max(1.0), "{file} {id} {n}: {p:?} vs {q:?}");
            }
        }
    }
}
