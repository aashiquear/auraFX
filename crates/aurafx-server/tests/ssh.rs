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

    // Drawn on the server's GPU: compare with drawing the same clip here.
    let c = s.add_filter("clip", r).unwrap();
    s.set_display(c, |d| d.draw_on = aurafx_pipeline::DrawOn::Server);
    s.update_blocking();
    let gpu = s.remote_clients[&host.name].info.lock().clone().unwrap().gpu;
    let Some(gpu) = gpu else {
        eprintln!("server has no GPU; drawing skipped");
        return;
    };
    assert_eq!(s.nodes[&c].remote.as_ref().unwrap().payload, Payload::Drawn);
    let bounds = s.visible_bounds();
    let mut cam = aurafx_render::Camera::default();
    cam.set_view([1.0, -1.0, 0.5], [0.0, 0.0, 1.0], &bounds);
    cam.update_clip(&bounds);
    let (w, h) = (640u32, 480u32);
    let display = s.nodes[&c].display.clone();
    let job =
        aurafx_pipeline::remote::RenderJob { targets: vec![(c, display.clone())], camera: aurafx_render::server::render_camera(&cam), width: w, height: h };
    let t = std::time::Instant::now();
    let (_, _, color, depth, ms) = s.remote_clients[&host.name].render(s.remote_specs(c), None, job).unwrap();
    let total = t.elapsed().as_secs_f64() * 1e3;
    let lc = local.add_filter("clip", lr).unwrap();
    local.update_blocking();
    let Ok(mut here) = aurafx_render::headless(w, h) else { return };
    let mesh = std::sync::Arc::new(aurafx_render::tessellate::tessellate(local.nodes[&lc].output.as_ref().unwrap(), &display));
    here.render(&cam, &[aurafx_render::NodeDraw { id: 1, mesh, display, overlay: false }], &Default::default());
    let (_, _, want) = here.read_pixels().unwrap();
    let covered = depth.iter().filter(|&&d| d < 1.0).count();
    let differ = color.chunks(4).zip(want.chunks(4)).filter(|(a, b)| a.iter().zip(b.iter()).any(|(p, q)| (*p as i32 - *q as i32).abs() > 6)).count();
    eprintln!("drawn on {gpu}: {covered} pixels, {differ} differ from this GPU's picture; server {ms:.1} ms, round trip {total:.0} ms");
    assert!(covered > 20000);
    assert!(differ * 50 < covered, "{differ} of {covered} pixels differ");
}
