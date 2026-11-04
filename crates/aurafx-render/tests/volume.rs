use aurafx_pipeline::{DisplayProps, Representation};
use aurafx_render::*;
use std::sync::Arc;

#[test]
fn volume_renders_offscreen() {
    let img = aurafx_filters::sources::wavelet([-10, 10, -10, 10, -10, 10], 255.0, 0.5);
    let ds = aurafx_data::DataSet::ImageData(img);
    let mut d = DisplayProps::default();
    d.representation = Representation::Volume;
    d.colormap = "Inferno".into();
    let mesh = Arc::new(tessellate::tessellate(&ds, &d));
    assert!(mesh.volume.is_some());
    let mut r = match headless(400, 300) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("no GPU: {e}");
            return;
        }
    };
    let mut cam = Camera::default();
    cam.set_view([1.0, 1.0, 1.0], [0.0, 0.0, 1.0], &ds.bounds());
    r.render(&cam, &[NodeDraw { id: 1, mesh, display: d, overlay: false }], &RenderSettings::default());
    let (_, _, px) = r.read_pixels().unwrap();
    let bg = px[0..3].to_vec();
    let lit = px.chunks(4).filter(|c| c[..3] != bg[..]).count();
    let out = std::env::temp_dir().join("aurafx_volume.png");
    r.save_png(&out).unwrap();
    eprintln!("{lit} non-background pixels, saved {}", out.display());
    assert!(lit > 10_000);
}

#[test]
fn volume_stops_at_opaque_geometry() {
    // An opaque plane through the middle of the volume, seen face-on: the volume half behind it
    // must not show through, so the image equals plane + front half only.
    let img = aurafx_filters::sources::wavelet([-10, 10, -10, 10, -10, 10], 255.0, 0.5);
    let vol = aurafx_data::DataSet::ImageData(img);
    let mut dv = DisplayProps::default();
    dv.representation = Representation::Volume;
    dv.volume_opacity = 4.0;
    let plane = aurafx_data::DataSet::PolyData(aurafx_filters::sources::plane([-12.0, -12.0, 0.0], [12.0, -12.0, 0.0], [-12.0, 12.0, 0.0], 1, 1));
    let mut dp = DisplayProps::default();
    dp.solid_color = [0.0, 1.0, 0.0];
    dp.lighting = false;
    let mut r = match headless(200, 200) {
        Ok(r) => r,
        Err(_) => return,
    };
    let mut cam = Camera::default();
    cam.set_view([0.0, 0.0, 1.0], [0.0, 1.0, 0.0], &vol.bounds());
    let center = |r: &Renderer| {
        let (w, h, px) = r.read_pixels().unwrap();
        let i = 4 * ((h / 2) * w + w / 2) as usize;
        [px[i], px[i + 1], px[i + 2]]
    };
    let mv = Arc::new(tessellate::tessellate(&vol, &dv));
    let mp = Arc::new(tessellate::tessellate(&plane, &dp));
    // Plane alone.
    r.render(&cam, &[NodeDraw { id: 2, mesh: mp.clone(), display: dp.clone(), overlay: false }], &RenderSettings::default());
    let plane_only = center(&r);
    assert!(plane_only[1] > 200 && plane_only[0] < 30, "{plane_only:?}");
    // Plane + volume from the front: the volume in front tints it.
    r.render(&cam, &[NodeDraw { id: 1, mesh: mv.clone(), display: dv.clone(), overlay: false }, NodeDraw { id: 2, mesh: mp.clone(), display: dp.clone(), overlay: false }], &RenderSettings::default());
    let front = center(&r);
    assert_ne!(front, plane_only, "volume in front of the plane must be visible");
    // Seen from behind with the plane moved to the back face: the volume is fully in front.
    // Moving the plane in front of the volume must hide the volume completely.
    let plane_front = aurafx_data::DataSet::PolyData(aurafx_filters::sources::plane([-12.0, -12.0, 10.5], [12.0, -12.0, 10.5], [-12.0, 12.0, 10.5], 1, 1));
    let mpf = Arc::new(tessellate::tessellate(&plane_front, &dp));
    r.render(&cam, &[NodeDraw { id: 1, mesh: mv, display: dv, overlay: false }, NodeDraw { id: 2, mesh: mpf, display: dp, overlay: false }], &RenderSettings::default());
    let hidden = center(&r);
    assert_eq!(hidden, plane_only, "a plane in front of the volume hides it");
}
