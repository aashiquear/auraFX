use aurafx_pipeline::{ColorBy, DisplayProps};
use aurafx_render::*;
use std::sync::Arc;

#[test]
fn render_can_offscreen() {
    let p = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../testdata/can.ex2");
    if !p.exists() {
        return;
    }
    let s = match aurafx_io::read_file(&p).unwrap() {
        aurafx_io::Loaded::Series(s) => s,
        _ => panic!(),
    };
    let ds = aurafx_io::load_step(&s.steps[43]).unwrap();
    let mut d = DisplayProps::default();
    d.color_by = Some(ColorBy { assoc: aurafx_filters::Association::Cells, name: "EQPS".into(), component: None });
    d.colormap = "Inferno".into();
    let mesh = Arc::new(tessellate::tessellate(&ds, &d));
    let mut r = match headless(640, 480) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("no GPU adapter: {e}");
            return;
        }
    };
    let mut cam = Camera::default();
    cam.set_view([1.0, 0.0, 0.0], [0.0, 0.0, 1.0], &ds.bounds());
    eprintln!("cam eye {:?} target {:?} near {} far {}", cam.eye, cam.target, cam.near, cam.far);
    r.render(&cam, &[NodeDraw { id: 1, mesh, display: d, overlay: false }], &RenderSettings::default());
    let (w, h, px) = r.read_pixels().unwrap();
    let bg = px[0..4].to_vec();
    let differing = px.chunks(4).filter(|c| c[..3] != bg[..3]).count();
    eprintln!("{w}x{h}, {differing} non-background pixels");
    let out = std::env::temp_dir().join("aurafx_can.png");
    r.save_png(&out).unwrap();
    eprintln!("saved {}", out.display());
    assert!(differing > 1000);
}
