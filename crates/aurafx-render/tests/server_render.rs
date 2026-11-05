//! Objects drawn on a server look the same as drawn here, and stay on the server.

use aurafx_jobs::{Host, HostKind};
use aurafx_pipeline::remote::{Payload, RemoteClient, RenderJob};
use aurafx_pipeline::{DrawOn, Session};
use aurafx_render::server::{render_camera, ServerRenderer};
use aurafx_render::*;
use std::sync::Arc;

#[test]
fn server_drawn_objects_match_local_drawing() {
    let Some(hook) = ServerRenderer::new() else {
        eprintln!("no GPU adapter");
        return;
    };
    let gpu = hook.gpu();
    let host = Host { name: "gpu-server".into(), kind: HostKind::Local, setup: String::new() };
    let mut s = Session::new();
    s.remote_clients.insert(host.name.clone(), RemoteClient::in_process_with("gpu-server", Some(Box::new(hook)), Some(gpu)));
    let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../testdata/disk_out_ref.ex2");
    let r = s.open_remote(host.clone(), &path.display().to_string());
    let c = s.add_filter("clip", r).unwrap();
    s.set_display(c, |d| d.draw_on = DrawOn::Server);
    s.update_blocking();
    let n = &s.nodes[&c];
    assert!(n.error.is_none(), "{:?}", n.error);
    let st = n.remote.clone().unwrap();
    assert_eq!(st.payload, Payload::Drawn, "the clip stays on the server");
    assert_eq!(st.sent_bytes, 0);
    assert!(s.drawn_on_server().iter().any(|(id, _)| *id == c));
    let bounds = s.visible_bounds();
    assert!(!bounds.is_empty(), "bounds come from the server's summary");
    let display = n.display.clone();

    // The server's picture of the clip.
    let (w, h) = (320u32, 240u32);
    let mut cam = Camera::default();
    cam.set_view([1.0, -1.0, 0.5], [0.0, 0.0, 1.0], &bounds);
    cam.update_clip(&bounds);
    let client = s.remote_clients[&host.name].clone();
    assert!(client.info.lock().as_ref().unwrap().gpu.is_some());
    let job = RenderJob { targets: vec![(c, display.clone())], camera: render_camera(&cam), width: w, height: h };
    let (iw, ih, color, depth, ms) = client.render(s.remote_specs(c), None, job).unwrap();
    eprintln!("server drew {iw}x{ih} in {ms:.1} ms");
    assert_eq!((iw, ih, color.len(), depth.len()), (w, h, (4 * w * h) as usize, (w * h) as usize));

    // The same clip drawn here.
    let mut local = Session::new();
    let lr = local.open_file(&path).unwrap();
    let lc = local.add_filter("clip", lr).unwrap();
    local.update_blocking();
    let out = local.nodes[&lc].output.clone().unwrap();
    let mut here = headless(w, h).unwrap();
    let mesh = Arc::new(tessellate::tessellate(&out, &display));
    here.render(&cam, &[NodeDraw { id: 1, mesh, display: display.clone(), overlay: false }], &RenderSettings::default());
    let (_, _, want) = here.read_pixels().unwrap();
    let want_depth = here.read_depth().unwrap();
    let covered = depth.iter().filter(|&&d| d < 1.0).count();
    assert!(covered > 5000, "{covered} pixels covered");
    let differ = color
        .chunks(4)
        .zip(want.chunks(4))
        .zip(depth.iter().zip(&want_depth))
        .filter(|((a, b), (da, db))| (**da < 1.0 || **db < 1.0) && a.iter().zip(b.iter()).any(|(p, q)| (*p as i32 - *q as i32).abs() > 3))
        .count();
    eprintln!("{differ} of {covered} covered pixels differ");
    assert!(differ * 100 < covered, "server and local pictures differ in {differ} pixels");

    // Composited into a local scene it looks like the local drawing.
    let mut view = headless(w, h).unwrap();
    view.set_layers(&[Arc::new(ImageLayer { width: w, height: h, color, depth })]);
    view.render(&cam, &[], &RenderSettings::default());
    let (_, _, comp) = view.read_pixels().unwrap();
    let differ = comp.chunks(4).zip(want.chunks(4)).filter(|(a, b)| a.iter().zip(b.iter()).any(|(p, q)| (*p as i32 - *q as i32).abs() > 3)).count();
    assert!(differ * 50 < covered, "composited picture differs in {differ} pixels");

    // Translucent objects can't be blended from a picture: they are drawn here.
    s.set_display(c, |d| d.opacity = 0.5);
    s.update_blocking();
    assert_ne!(s.nodes[&c].remote.as_ref().unwrap().payload, Payload::Drawn);
    assert!(s.drawn_on_server().is_empty());
    s.set_display(c, |d| d.opacity = 1.0);
    s.update_blocking();
    assert_eq!(s.nodes[&c].remote.as_ref().unwrap().payload, Payload::Drawn);
}
