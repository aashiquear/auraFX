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
        eprintln!(
            "step {step}: tris {} lines {} points {} bad {bad} range {:?} origin {:?} bounds {:?}",
            m.tris.len() / 3,
            m.lines.len(),
            m.points.len(),
            m.scalar_range,
            m.origin,
            m.bounds
        );
        assert!(m.tris.len() > 0);
        assert_eq!(bad, 0);
    }
}

/// 8-bit RGB cell arrays (an IPF map) carry their colors packed in the scalar, with no range
/// or legend; with direct colors off they go through the color map.
#[test]
fn rgb_arrays_are_drawn_as_their_colors() {
    use aurafx_data::*;
    let mut img = ImageData { extent: [0, 2, 0, 1, 0, 0], ..Default::default() };
    img.cell_data.insert(Array::new("IPFColor", 3, ArrayData::U8(vec![255, 0, 0, 10, 20, 30])));
    let ds = DataSet::ImageData(img);
    let mut d = DisplayProps::default();
    d.color_by = Some(ColorBy { assoc: aurafx_filters::Association::Cells, name: "IPFColor".into(), component: None });
    let m = tessellate(&ds, &d);
    assert!(m.direct_rgb && !m.has_scalars);
    let packed: std::collections::BTreeSet<u32> = m.tris.iter().map(|v| v.scalar as u32).collect();
    assert_eq!(packed.into_iter().collect::<Vec<_>>(), vec![10 * 65536 + 20 * 256 + 30, 255 * 65536]);
    d.direct_colors = false;
    let m = tessellate(&ds, &d);
    assert!(!m.direct_rgb && m.has_scalars);
}
