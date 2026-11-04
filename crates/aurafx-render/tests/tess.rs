use aurafx_pipeline::{ColorBy, DisplayProps};
use aurafx_render::tessellate::tessellate;

#[test]
fn can_exodus_tessellates() {
    let p = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../testdata/can.ex2");
    if !p.exists() {
        return;
    }
    let s = match aurafx_io::read_file(&p).unwrap() {
        aurafx_io::Loaded::Series(s) => s,
        _ => panic!(),
    };
    for step in [0usize, 43] {
        let ds = aurafx_io::load_step(&s.steps[step]).unwrap();
        let mut d = DisplayProps::default();
        d.color_by = Some(ColorBy { assoc: aurafx_filters::Association::Cells, name: "EQPS".into(), component: None });
        let m = tessellate(&ds, &d);
        let bad = m.tris.iter().filter(|v| !v.pos.iter().all(|x| x.is_finite()) || !v.normal.iter().all(|x| x.is_finite())).count();
        eprintln!("step {step}: tris {} lines {} points {} bad {bad} range {:?} origin {:?} bounds {:?}", m.tris.len() / 3, m.lines.len(), m.points.len(), m.scalar_range, m.origin, m.bounds);
        assert!(m.tris.len() > 0);
        assert_eq!(bad, 0);
    }
}
