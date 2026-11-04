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
