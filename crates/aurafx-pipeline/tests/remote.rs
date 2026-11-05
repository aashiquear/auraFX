//! Client/server: nodes opened on a server run there and give the same results as locally.

use aurafx_data::DataSet;
use aurafx_jobs::{Host, HostKind};
use aurafx_pipeline::remote::{Payload, RemoteClient};
use aurafx_pipeline::Session;
use std::path::PathBuf;

fn td(name: &str) -> String {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../testdata").join(name).display().to_string()
}

fn server_session() -> (Session, Host) {
    let host = Host { name: "test-server".into(), kind: HostKind::Local, setup: String::new(), server_processes: 0 };
    let mut s = Session::new();
    s.remote_clients.insert(host.name.clone(), RemoteClient::in_process("test-server"));
    (s, host)
}

#[track_caller]
fn same(a: &DataSet, b: &DataSet) {
    assert_eq!((a.n_points(), a.n_cells()), (b.n_points(), b.n_cells()));
    assert_eq!(a.bounds(), b.bounds());
    if a != b {
        let (pa, pb) =
            (a.point_data().map(|p| p.names().map(String::from).collect::<Vec<_>>()), b.point_data().map(|p| p.names().map(String::from).collect::<Vec<_>>()));
        panic!(
            "datasets differ ({} vs {}); point arrays {pa:?} vs {pb:?}; cell arrays {:?} vs {:?}",
            a.type_name(),
            b.type_name(),
            a.cell_data().map(|p| p.names().map(String::from).collect::<Vec<_>>()),
            b.cell_data().map(|p| p.names().map(String::from).collect::<Vec<_>>())
        );
    }
}

#[test]
fn remote_nodes_match_local_ones() {
    let (mut s, host) = server_session();
    let mut local = Session::new();

    // A reader and a slice on the server; the same pipeline locally.
    let r = s.open_remote(host.clone(), &td("disk_out_ref.ex2"));
    let lr = local.open_file(td("disk_out_ref.ex2")).unwrap();
    s.update_blocking();
    local.update_blocking();
    let n = &s.nodes[&r];
    assert!(n.error.is_none(), "{:?}", n.error);
    let st = n.remote.clone().expect("ran on the server");
    assert_eq!(st.payload, Payload::Full, "small data comes over whole");
    same(n.output.as_ref().unwrap(), local.nodes[&lr].output.as_ref().unwrap());
    assert_eq!(n.info.as_ref().unwrap().n_points, local.nodes[&lr].info.as_ref().unwrap().n_points);

    let sl = s.add_filter("slice", r).unwrap();
    let lsl = local.add_filter("slice", lr).unwrap();
    for (sess, id) in [(&mut s, sl), (&mut local, lsl)] {
        sess.set_param_json(id, "normal", &serde_json::json!([0.0, 0.0, 1.0]));
    }
    s.update_blocking();
    local.update_blocking();
    assert!(s.nodes[&sl].remote.is_some(), "the slice ran on the server: {:?}", s.nodes[&sl].error);
    same(s.nodes[&sl].output.as_ref().unwrap(), local.nodes[&lsl].output.as_ref().unwrap());
    // The reader was hidden by the slice: it now only sends its summary.
    assert_eq!(s.nodes[&r].remote.as_ref().unwrap().payload, Payload::Full, "fetched before it was hidden");
    s.mark_dirty(r);
    s.update_blocking();
    assert!(!s.nodes[&r].visible);
    assert_eq!(s.nodes[&r].remote.as_ref().unwrap().payload, Payload::None);
    assert_eq!(s.nodes[&r].info.as_ref().unwrap().n_cells, local.nodes[&lr].info.as_ref().unwrap().n_cells, "summary of the full data");
    same(s.nodes[&sl].output.as_ref().unwrap(), local.nodes[&lsl].output.as_ref().unwrap());
    // Showing it again brings the data.
    s.set_visible(r, true);
    s.update_blocking();
    assert_eq!(s.nodes[&r].remote.as_ref().unwrap().payload, Payload::Full);

    // Big outputs come back as their surface, with the summary of the full data.
    s.remote_max_bytes = 1000;
    let c = s.add_filter("clip", r).unwrap();
    let lc = local.add_filter("clip", lr).unwrap();
    s.update_blocking();
    local.update_blocking();
    let st = s.nodes[&c].remote.clone().unwrap();
    assert_eq!(st.payload, Payload::Surface);
    assert!(st.full_bytes > 1000 && st.sent_bytes < st.full_bytes);
    let full = local.nodes[&lc].output.clone().unwrap();
    assert_eq!(s.nodes[&c].info.as_ref().unwrap().n_cells, full.n_cells());
    same(s.nodes[&c].output.as_ref().unwrap(), &DataSet::PolyData(aurafx_filters::surface::extract_surface(&full).unwrap()));
    // Asking for everything.
    s.set_fetch_full(c, true);
    s.update_blocking();
    assert_eq!(s.nodes[&c].remote.as_ref().unwrap().payload, Payload::Full);
    same(s.nodes[&c].output.as_ref().unwrap(), &full);

    // Parameter changes re-run on the server.
    s.remote_max_bytes = 0;
    for (sess, id) in [(&mut s, sl), (&mut local, lsl)] {
        sess.set_param_json(id, "normal", &serde_json::json!([1.0, 0.0, 0.0]));
        sess.update_blocking();
    }
    same(s.nodes[&sl].output.as_ref().unwrap(), local.nodes[&lsl].output.as_ref().unwrap());

    // State files remember the host and reconnect.
    let file = std::env::temp_dir().join(format!("aurafx_remote_{}.afx", std::process::id()));
    s.save_state(&file, None).unwrap();
    let (mut s2, _) = server_session();
    s2.load_state(&file).unwrap();
    s2.update_blocking();
    let _ = std::fs::remove_file(&file);
    let sl2 = s2.nodes.values().find(|n| n.name == s.nodes[&sl].name).unwrap().id;
    assert!(s2.nodes[&sl2].remote.is_some(), "{:?}", s2.nodes[&sl2].error);
    same(s2.nodes[&sl2].output.as_ref().unwrap(), s.nodes[&sl].output.as_ref().unwrap());

    // Errors on the server show on the node.
    let bad = s.open_remote(host.clone(), "/no/such/file.vtu");
    s.update_blocking();
    assert!(s.nodes[&bad].error.as_deref().unwrap_or("").contains("file.vtu"), "{:?}", s.nodes[&bad].error);
}

#[test]
fn remote_time_series_and_temporal_filters() {
    let (mut s, host) = server_session();
    let mut local = Session::new();
    let r = s.open_remote(host, &td("rotating.pvd"));
    let lr = local.open_file(td("rotating.pvd")).unwrap();
    s.update_blocking();
    local.update_blocking();
    assert_eq!(s.times, local.times, "time steps come from the server");
    assert!(s.times.len() > 2);
    for idx in [3, s.times.len() - 1] {
        s.set_time_index(idx);
        local.set_time_index(idx);
        s.update_blocking();
        local.update_blocking();
        same(s.nodes[&r].output.as_ref().unwrap(), local.nodes[&lr].output.as_ref().unwrap());
    }
    // A temporal filter runs on the server with access to every step.
    let p = s.add_filter("particle_tracer", r).unwrap();
    let lp = local.add_filter("particle_tracer", lr).unwrap();
    s.update_blocking();
    local.update_blocking();
    assert!(s.nodes[&p].error.is_none(), "{:?}", s.nodes[&p].error);
    assert!(s.nodes[&p].remote.is_some());
    same(s.nodes[&p].output.as_ref().unwrap(), local.nodes[&lp].output.as_ref().unwrap());
}
