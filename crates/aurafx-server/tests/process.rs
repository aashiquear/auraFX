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

/// Other programs may find the coordinator's port while the ranks start: connections that say
/// nothing, or send bytes that are not text, are dropped and the ranks still connect. The
/// one-time token is not on any command line.
#[test]
fn stray_connections_do_not_stop_a_launch() {
    if !std::path::Path::new("/bin/bash").exists() {
        eprintln!("no bash: skipped");
        return;
    }
    std::env::set_var("AURAFX_SERVER", env!("CARGO_BIN_EXE_aurafx-server"));
    std::env::set_var("AURAFX_CONNECT_HOST", "127.0.0.1");
    let dir = std::env::temp_dir().join(format!("aurafx_stray_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let script = dir.join("fake-mpirun");
    let seen = dir.join("command-line");
    // Like fake-mpirun above, after two connections that are not ranks: one sends bytes that
    // are not text and stays, one says nothing.
    std::fs::write(
        &script,
        format!(
            "#!/bin/bash\nn=$2; shift 2\necho \"$*\" > '{}'\nport=$(echo \"$*\" | sed -n 's/.*--connect [^ :]*:\\([0-9]*\\).*/\\1/p')\n\
             ( exec 3<>/dev/tcp/127.0.0.1/$port; printf '\\377\\376\\375 not text\\n' >&3; sleep 4 ) &\n\
             ( exec 4<>/dev/tcp/127.0.0.1/$port; sleep 4 ) &\nsleep 0.3\n\
             i=0\nwhile [ $i -lt $n ]; do OMPI_COMM_WORLD_RANK=$i OMPI_COMM_WORLD_SIZE=$n sh -c \"$*\" & i=$((i+1)); done\nwait\n",
            seen.display()
        ),
    )
    .unwrap();
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    let td = |n: &str| std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../testdata").join(n).display().to_string();
    let host = Host {
        name: "mpi cluster with strays".into(),
        kind: HostKind::Local,
        setup: String::new(),
        server_processes: 2,
        launcher: format!("{} -np {{n}} {{cmd}}", script.display()),
    };
    let mut s = Session::new();
    let r = s.open_remote(host.clone(), &td("wavelet.vti"));
    s.update_blocking();
    assert!(s.nodes[&r].error.is_none(), "{:?}", s.nodes[&r].error);
    let info = s.remote_clients[&aurafx_pipeline::remote::client_key(&host)].info.lock().clone().unwrap();
    assert_eq!(info.processes, 2);
    let line = std::fs::read_to_string(&seen).unwrap();
    assert!(line.contains("--connect") && !line.contains("--token"), "{line}");
    assert!(!line.split_whitespace().any(|w| w.len() == 32 && w.chars().all(|c| c.is_ascii_hexdigit())), "the token is not on the command line: {line}");
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
            // Every point array of one process is there, with the same value at every point
            // (by position: explicit pieces repeat the points on their borders, and every
            // copy must agree). A gradient without ghost cells differs on those borders.
            let by_pos = |d: &aurafx_data::DataSet| {
                let u = d.to_unstructured().unwrap();
                let mut m: std::collections::HashMap<(String, [i64; 3]), Vec<Vec<f64>>> = Default::default();
                for a in u.point_data.iter() {
                    for i in 0..u.points.len() {
                        let key = (a.name.clone(), u.points.get(i).map(|v| (v * 1e9).round() as i64));
                        m.entry(key).or_default().push((0..a.ncomp).map(|c| a.get(i, c)).collect());
                    }
                }
                m
            };
            let (px, py) = (by_pos(&x), by_pos(&y));
            assert!(!py.is_empty(), "{file} {id}: point arrays to compare");
            if id == "gradient" {
                assert!(py.keys().any(|k| k.0 != "RTData"), "{file}: the gradient array is in the result");
            }
            let mut worst = 0.0f64;
            for (k, v) in &py {
                let copies = px.get(k).unwrap_or_else(|| panic!("{file} {id}: missing {k:?}"));
                for w in copies {
                    for (a, b) in w.iter().zip(&v[0]) {
                        worst = worst.max((a - b).abs() / b.abs().max(1.0));
                    }
                }
            }
            assert!(worst < 1e-9, "{file} {id}: values differ by {worst}");
        }
    }
}
