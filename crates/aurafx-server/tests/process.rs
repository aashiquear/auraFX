//! The real server executable, started the way the app starts it on a host (through
//! `bash -lc`, here on this machine).

use aurafx_jobs::{Host, HostKind};
use aurafx_pipeline::remote::{Payload, RemoteClient};
use aurafx_pipeline::Session;

#[test]
fn server_process_runs_a_pipeline() {
    std::env::set_var("AURAFX_SERVER", env!("CARGO_BIN_EXE_aurafx-server"));
    let td = |n: &str| std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../testdata").join(n).display().to_string();
    let host = Host { name: "this computer".into(), kind: HostKind::Local, setup: String::new() };
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
    std::env::set_var("AURAFX_SERVER", "/nonexistent/aurafx-server");
    let c2 = RemoteClient::for_host(Host { name: "nowhere".into(), kind: HostKind::Local, setup: String::new() });
    let err = c2.request(&aurafx_pipeline::remote::Request::Hello { protocol: 1 }).unwrap_err();
    assert!(format!("{err:#}").contains("nonexistent"), "{err:#}");
}
