//! Against a real SSH host (skipped unless AURAFX_TEST_SSH_HOST is set). The host needs the
//! server at AURAFX_TEST_SSH_SERVER (or on its PATH) and disk_out_ref.ex2 / rotating.pvd in
//! AURAFX_TEST_SSH_DIR.

use aurafx_jobs::{Host, HostKind};
use aurafx_pipeline::remote::Payload;
use aurafx_pipeline::Session;

#[test]
fn ssh_server_matches_local() {
    let Ok(target) = std::env::var("AURAFX_TEST_SSH_HOST") else {
        eprintln!("skipped: set AURAFX_TEST_SSH_HOST");
        return;
    };
    let dir = std::env::var("AURAFX_TEST_SSH_DIR").unwrap_or_else(|_| ".".into());
    let setup = std::env::var("AURAFX_TEST_SSH_SERVER").map(|s| format!("export AURAFX_SERVER={s}")).unwrap_or_default();
    let host = Host { name: target.clone(), kind: HostKind::Ssh { target, port: None, identity: None, jump: None }, setup };
    let td = |n: &str| std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../testdata").join(n).display().to_string();

    let t0 = std::time::Instant::now();
    let mut s = Session::new();
    let r = s.open_remote(host.clone(), &format!("{dir}/disk_out_ref.ex2"));
    let sl = s.add_filter("slice", r).unwrap();
    s.update_blocking();
    let mut local = Session::new();
    let lr = local.open_file(td("disk_out_ref.ex2")).unwrap();
    let lsl = local.add_filter("slice", lr).unwrap();
    local.update_blocking();
    assert!(s.nodes[&sl].error.is_none(), "{:?}", s.nodes[&sl].error);
    assert_eq!(s.nodes[&sl].output, local.nodes[&lsl].output);
    assert_eq!(s.nodes[&r].remote.as_ref().unwrap().payload, Payload::None, "the hidden reader stays on the server");
    let st = s.nodes[&sl].remote.clone().unwrap();
    let info = s.remote_clients[&host.name].info.lock().clone().unwrap();
    eprintln!(
        "server {} ({} cpus): slice {} bytes sent, server {:.1} ms, total {:.0} ms",
        info.hostname,
        info.cpus,
        st.sent_bytes,
        st.server_ms,
        t0.elapsed().as_secs_f64() * 1e3
    );

    let p = s.open_remote(host.clone(), &format!("{dir}/rotating.pvd"));
    s.update_blocking();
    let lp = local.open_file(td("rotating.pvd")).unwrap();
    local.update_blocking();
    assert_eq!(s.times, local.times);
    s.set_time_index(4);
    local.set_time_index(4);
    s.update_blocking();
    local.update_blocking();
    assert_eq!(s.nodes[&p].output, local.nodes[&lp].output);
}
