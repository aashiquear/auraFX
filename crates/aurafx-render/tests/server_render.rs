//! Objects drawn on a server look the same as drawn here, and stay on the server.

use aurafx_jobs::{Host, HostKind};
use aurafx_pipeline::remote::{Payload, RemoteClient, RenderJob};
use aurafx_pipeline::{DrawOn, Session};
use aurafx_render::server::{render_camera, ServerRenderer};
use aurafx_render::*;
use std::sync::Arc;

/// In-process servers draw on background threads: dropping the session drops their clients,
/// which wait for those threads to finish (releasing their GPU devices) before the test ends.
fn shut_down(s: Session) {
    drop(s);
}

#[test]
fn server_drawn_objects_match_local_drawing() {
    let Some(hook) = ServerRenderer::new() else {
        eprintln!("no GPU adapter");
        return;
    };
    let gpu = hook.gpu();
    let host = Host { name: "gpu-server".into(), kind: HostKind::Local, setup: String::new(), server_processes: 0 };
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
    let client = s.remote_clients[&aurafx_pipeline::remote::client_key(&host)].clone();
    assert!(client.info.lock().as_ref().unwrap().gpu.is_some());
    let job = RenderJob { targets: vec![(c, display.clone())], camera: render_camera(&cam), width: w, height: h, lighting: Default::default() };
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

    // The view's lights and shadows go with the request: a red light fixed in the scene.
    let lighting = aurafx_pipeline::Lighting {
        lights: vec![aurafx_pipeline::Light { color: [1.0, 0.3, 0.3], camera_relative: false, direction: [0.3, -0.5, 1.0], ..Default::default() }],
        shadows: true,
        ..Default::default()
    };
    let job = RenderJob { targets: vec![(c, display.clone())], camera: render_camera(&cam), width: w, height: h, lighting: lighting.clone() };
    let (_, _, red, _, _) = client.render(s.remote_specs(c), None, job).unwrap();
    here.render(
        &cam,
        &[NodeDraw { id: 1, mesh: Arc::new(tessellate::tessellate(&out, &display)), display: display.clone(), overlay: false }],
        &RenderSettings { lighting, ..Default::default() },
    );
    let (_, _, want_red) = here.read_pixels().unwrap();
    let differ = red
        .chunks(4)
        .zip(want_red.chunks(4))
        .zip(&depth)
        .filter(|((a, b), d)| **d < 1.0 && a.iter().zip(b.iter()).any(|(p, q)| (*p as i32 - *q as i32).abs() > 3))
        .count();
    assert!(differ * 100 < covered, "server lighting differs in {differ} pixels");
    assert!(
        red.chunks(4).zip(&color.chunks(4).collect::<Vec<_>>()).any(|(a, b)| a[0] as i32 - a[1] as i32 > b[0] as i32 - b[1] as i32 + 30),
        "the red light reached the server"
    );

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
    shut_down(s);
}

/// A parallel server's workers each draw their piece; the coordinator composites the pictures
/// by depth ("sort-last"), which looks like drawing the whole here.
#[test]
fn parallel_server_composites_pieces_by_depth() {
    use aurafx_pipeline::remote::ServeOptions;
    if ServerRenderer::new().is_none() {
        eprintln!("no GPU adapter");
        return;
    }
    let n = 3;
    let workers = (0..n)
        .map(|k| {
            let hook = ServerRenderer::for_pieces().unwrap();
            let gpu = Some(hook.gpu());
            RemoteClient::in_process_opts(&format!("w{k}"), ServeOptions { hook: Some(Box::new(hook)), gpu, piece: Some((k, n)), workers: vec![] })
        })
        .collect();
    let host = Host { name: "cluster".into(), kind: HostKind::Local, setup: String::new(), server_processes: n as u32 };
    let mut s = Session::new();
    s.remote_clients
        .insert(aurafx_pipeline::remote::client_key(&host), RemoteClient::in_process_opts("cluster", ServeOptions { workers, ..Default::default() }));
    let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../testdata/disk_out_ref.ex2");
    let r = s.open_remote(host.clone(), &path.display().to_string());
    let c = s.add_filter("clip", r).unwrap();
    s.set_display(c, |d| d.draw_on = DrawOn::Server);
    s.update_blocking();
    assert_eq!(s.nodes[&c].remote.as_ref().unwrap().payload, Payload::Drawn, "{:?}", s.nodes[&c].error);
    let bounds = s.visible_bounds();
    let (w, h) = (320u32, 240u32);
    let mut cam = Camera::default();
    cam.set_view([1.0, -1.0, 0.5], [0.0, 0.0, 1.0], &bounds);
    cam.update_clip(&bounds);
    let display = s.nodes[&c].display.clone();
    let job = RenderJob { targets: vec![(c, display.clone())], camera: render_camera(&cam), width: w, height: h, lighting: Default::default() };
    let (_, _, color, depth, _) = s.remote_clients[&aurafx_pipeline::remote::client_key(&host)].render(s.remote_specs(c), None, job).unwrap();

    let mut local = Session::new();
    let lr = local.open_file(&path).unwrap();
    let lc = local.add_filter("clip", lr).unwrap();
    local.update_blocking();
    let mut here = headless(w, h).unwrap();
    let mesh = Arc::new(tessellate::tessellate(local.nodes[&lc].output.as_ref().unwrap(), &display));
    here.render(&cam, &[NodeDraw { id: 1, mesh, display, overlay: false }], &RenderSettings::default());
    let (_, _, want) = here.read_pixels().unwrap();
    // Away from the outline (where 2x2 supersampling and MSAA smooth edges a little
    // differently) the pictures agree; in particular there are no seams between pieces.
    let want_depth = here.read_depth().unwrap();
    let inside = |i: usize| {
        let (x, y) = ((i % w as usize) as i64, (i / w as usize) as i64);
        (-1..=1).all(|dy| {
            (-1..=1).all(|dx| {
                let (xx, yy) = (x + dx, y + dy);
                xx >= 0 && yy >= 0 && xx < w as i64 && yy < h as i64 && {
                    let j = (yy * w as i64 + xx) as usize;
                    depth[j] < 1.0 && want_depth[j] < 1.0
                }
            })
        })
    };
    let covered = (0..(w * h) as usize).filter(|&i| inside(i)).count();
    let differ = (0..(w * h) as usize).filter(|&i| inside(i) && (0..3).any(|c| (color[4 * i + c] as i32 - want[4 * i + c] as i32).abs() > 3)).count();
    eprintln!("{n} pieces composited: {differ} of {covered} pixels differ from drawing the whole");
    if let Ok(dir) = std::env::var("AURAFX_DUMP") {
        let _ = image::save_buffer(format!("{dir}/composite.png"), &color, w, h, image::ColorType::Rgba8);
        let _ = image::save_buffer(format!("{dir}/whole.png"), &want, w, h, image::ColorType::Rgba8);
    }
    assert!(covered > 5000);
    assert!(differ * 100 < covered, "{differ} pixels differ");
    shut_down(s);
}
