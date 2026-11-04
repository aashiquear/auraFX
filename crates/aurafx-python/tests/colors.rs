use aurafx_pipeline::Session;
use aurafx_python::PythonRuntime;

#[test]
fn glyph_keeps_requested_colormap() {
    let p = concat!(env!("CARGO_MANIFEST_DIR"), "/../../testdata/disk_out_ref.ex2");
    let session = Session::shared();
    let rt = PythonRuntime::new(session.clone()).unwrap();
    let out = rt.run(&format!("d = afx.load({p:?})\ns = d.slice(normal=[0,0,1])\ng = s.glyph(orient='V', max_glyphs=50, scale=0.1).color_by('Pres', cmap='Cool to Warm')\nafx.run()\nprint(afx.describe())"));
    assert!(out.error.is_none(), "{:?}", out.error);
    let s = session.lock();
    let g = s.nodes.values().find(|n| n.name == "Glyph").unwrap();
    println!("{}", out.stdout);
    assert_eq!(g.display.colormap, "Cool to Warm");
    assert_eq!(g.display.color_by.as_ref().unwrap().name, "Pres");
}
