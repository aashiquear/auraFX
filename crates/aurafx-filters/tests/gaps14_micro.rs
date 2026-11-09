//! Section 14.2 checks through the registry.
use aurafx_data::*;

fn run(id: &str, input: Option<&DataSet>, kv: &[(&str, serde_json::Value)]) -> DataSet {
    let def = aurafx_filters::registry::find(id).unwrap_or_else(|| panic!("no filter {id}"));
    let mut p = def.default_params();
    for (k, v) in kv {
        p.set_json(k, v);
    }
    def.run(input, &p).unwrap_or_else(|e| panic!("{id}: {e:#}"))
}

/// A one-grain 2-D map whose lattice turns about z at kappa (rad/um) along x.
fn bent_map(kappa: f64) -> DataSet {
    let (nx, ny) = (20, 10);
    let mut eu = Vec::new();
    for _j in 0..ny {
        for i in 0..nx {
            eu.extend_from_slice(&[(0.3 + kappa * i as f64) as f32, 0.0, 0.0]);
        }
    }
    let mut im = ImageData { extent: [0, nx, 0, ny, 0, 1], spacing: [1.0; 3], ..Default::default() };
    im.cell_data.insert(Array::new("EulerAngles", 3, ArrayData::F32(eu)));
    im.cell_data.insert(Array::new("Phases", 1, ArrayData::I32(vec![1; (nx * ny) as usize])));
    im.cell_data.insert(Array::new("FeatureIds", 1, ArrayData::I32(vec![1; (nx * ny) as usize])));
    im.field_data.insert(Array::new("CrystalStructures", 1, ArrayData::U32(vec![999, 1])));
    DataSet::ImageData(im)
}

#[test]
fn nye_on_a_2d_map() {
    let kappa = 0.01;
    let out = run("gnd_density", Some(&bent_map(kappa)), &[]);
    let a = out.cell_data().unwrap().iter().find(|a| a.name.contains("GND")).cloned().expect("a GND array");
    let b = 2.54e-10;
    let want = kappa * 1e6 / b;
    let mid = 10 + 20 * 5;
    assert!((a.get(mid, 0) / want - 1.0).abs() < 0.02, "{} vs {want}", a.get(mid, 0));
}

#[test]
fn pole_figure_pictures_per_phase() {
    // Two phases of near-cube orientations: a {100} pole sits at the centre of each picture.
    let DataSet::ImageData(mut im) = bent_map(0.0) else { panic!() };
    let n = im.cell_data.get("Phases").unwrap().len();
    im.cell_data.insert(Array::new("Phases", 1, ArrayData::I32((0..n).map(|i| 1 + (i % 2) as i32).collect())));
    im.field_data.insert(Array::new("CrystalStructures", 1, ArrayData::U32(vec![999, 1, 1])));
    let sel = aurafx_filters::crystal_texture::Selection {
        orientations: None,
        structure: aurafx_filters::crystal::FROM_PHASES,
        phases: None,
        phase: 0,
        mask: None,
        max: 100_000,
        seed: 1,
    };
    let out = aurafx_filters::crystal_texture::pole_figure_images(&DataSet::ImageData(im), &sel, "100, 111", "Stereographic", 5.0, 2.0, 64).unwrap();
    assert_eq!(out.iter().map(|o| (o.0, o.1.as_str())).collect::<Vec<_>>(), vec![(1, "100"), (1, "111"), (2, "100"), (2, "111")]);
    let c = out[0].2.cell_data.get("Color").unwrap();
    let centre = 32 * 64 + 32;
    assert!(c.get(centre, 0) > 150.0 && c.get(centre, 2) < 60.0, "centre {:?}", (0..3).map(|k| c.get(centre, k)).collect::<Vec<_>>());
    assert_eq!(c.get(0, 0), 255.0);
}
