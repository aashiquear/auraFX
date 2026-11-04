use aurafx_filters::ParamValue;
use aurafx_pipeline::{CameraState, Session};

fn td(n: &str) -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../testdata").join(n)
}

#[test]
fn undo_redo_roundtrip() {
    let mut s = Session::new();
    let r = s.open_file(td("wavelet_tets.vtu")).unwrap();
    let sl = s.add_filter("slice", r).unwrap();
    s.update_blocking();
    let out_before = s.nodes[&sl].output.clone().unwrap();
    s.set_param(sl, "normal", ParamValue::Vec3([0.0, 0.0, 1.0]));
    std::thread::sleep(std::time::Duration::from_millis(700));
    s.rename(sl, "Z cut".into());
    assert_eq!(s.undo_label(), Some("Rename Slice"));
    s.undo();
    assert_eq!(s.nodes[&sl].name, "Slice");
    s.undo();
    assert_eq!(s.nodes[&sl].params.get("normal"), Some(&ParamValue::Vec3([1.0, 0.0, 0.0])));
    s.undo(); // remove slice
    assert!(!s.nodes.contains_key(&sl));
    s.redo(); // slice back
    assert!(s.nodes.contains_key(&sl));
    s.update_blocking();
    assert!(s.nodes[&sl].output.is_some());
    s.redo();
    s.redo();
    assert_eq!(s.nodes[&sl].name, "Z cut");
    s.update_blocking();
    let out_after = s.nodes[&sl].output.clone().unwrap();
    assert!(out_after.bounds().size()[2] < 1e-9, "redo restored the Z normal");
    assert!(out_before.bounds().size()[0] < 1e-9);
    // Display edits merge into one step.
    for k in 0..5 {
        s.set_display(sl, |d| d.opacity = 0.5 + 0.1 * k as f32);
    }
    assert_eq!(s.undo_label(), Some("Display Z cut"));
    s.undo();
    assert_eq!(s.nodes[&sl].display.opacity, 1.0);
}

#[test]
fn save_and_load_state() {
    let mut s = Session::new();
    let r = s.open_file(td("wavelet_tets.vtu")).unwrap();
    let c = s.add_filter("contour", r).unwrap();
    s.set_param(c, "values", ParamValue::Floats(vec![150.0, 200.0]));
    let cl = s.add_filter("clip", r).unwrap(); // origin stays "auto" (NaN)
    s.set_display(cl, |d| d.opacity = 0.4);
    s.update_blocking();
    let n_cells = s.nodes[&c].info.as_ref().unwrap().n_cells;
    let dir = std::env::temp_dir().join(format!("afx-state-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("test.afx");
    let cam = CameraState { eye: [1.0, 2.0, 3.0], target: [0.0; 3], up: [0.0, 0.0, 1.0], fov_y_deg: 30.0, orthographic: false, ortho_height: 1.0 };
    s.save_state(&path, Some(cam)).unwrap();
    let text = std::fs::read_to_string(&path).unwrap();
    assert!(text.contains("\"auto\""), "NaN params saved as auto");

    let mut t = Session::new();
    let got = t.load_state(&path).unwrap();
    assert_eq!(got, Some(cam));
    t.update_blocking();
    assert_eq!(t.nodes.len(), 3);
    let tc = t.nodes.values().find(|n| n.name == "Contour").unwrap();
    assert_eq!(tc.info.as_ref().unwrap().n_cells, n_cells);
    let tcl = t.nodes.values().find(|n| n.name == "Clip").unwrap();
    assert_eq!(tcl.display.opacity, 0.4);
    assert!(tcl.params.get("origin").unwrap().as_vec3().unwrap()[0].is_nan());
    assert!(tcl.error.is_none());
}

#[test]
fn temporal_statistics_of_lammps() {
    let mut s = Session::new();
    let r = s.open_file(td("lammps_small.dump")).unwrap();
    s.update_blocking();
    let ts = s.temporal_statistics(r).unwrap();
    s.update_blocking();
    let out = s.nodes[&ts].output.clone().unwrap();
    let pd = out.point_data().unwrap();
    // "type" is 1,1,2,2 at both steps: average equals the value, stddev 0.
    let avg = pd.get("type_average").unwrap();
    assert_eq!((0..4).map(|i| avg.get(i, 0)).collect::<Vec<_>>(), vec![1.0, 1.0, 2.0, 2.0]);
    assert_eq!(pd.get("type_stddev").unwrap().range(None), (0.0, 0.0));
}

#[test]
fn keyframes_drive_the_pipeline_and_survive_state_files() {
    let mut s = Session::new();
    let r = s.open_file(td("wavelet_tets.vtu")).unwrap();
    let sl = s.add_filter("slice", r).unwrap();
    s.set_param(sl, "normal", ParamValue::Vec3([0.0, 0.0, 1.0]));
    s.animation.key_param(sl, "origin", 0.0, ParamValue::Vec3([0.0, 0.0, -5.0]));
    s.animation.key_param(sl, "origin", 1.0, ParamValue::Vec3([0.0, 0.0, 5.0]));
    let cam = |x: f64| CameraState { eye: [x, 0.0, 0.0], target: [0.0; 3], up: [0.0, 0.0, 1.0], fov_y_deg: 30.0, orthographic: false, ortho_height: 1.0 };
    s.animation.key_camera(0.0, cam(10.0));
    s.animation.key_camera(1.0, cam(20.0));
    let c = s.apply_animation(0.75).unwrap();
    assert!((c.eye[0] - 10.0 * 2f64.powf(0.75)).abs() < 1e-9);
    s.update_blocking();
    let z = s.nodes[&sl].output.as_ref().unwrap().bounds();
    assert!((z.min[2] - 2.5).abs() < 1e-6 && (z.max[2] - 2.5).abs() < 1e-6, "slice at z = 2.5: {z:?}");

    let path = std::env::temp_dir().join(format!("aurafx-kf-{}.afx", std::process::id()));
    s.save_state(&path, None).unwrap();
    let mut s2 = Session::new();
    s2.load_state(&path).unwrap();
    std::fs::remove_file(&path).ok();
    assert_eq!(s2.animation, s.animation);
}

#[test]
fn openfoam_patches_filter_follows_time() {
    let mut s = Session::new();
    let r = s.open_file(td("foam_binary/case.foam")).unwrap();
    let p = s.add_filter("openfoam_patches", r).unwrap();
    s.update_blocking();
    let outlet_t = |s: &Session| {
        let out = s.nodes[&p].output.clone().unwrap_or_else(|| panic!("{:?}", s.nodes[&p].error));
        let aurafx_data::DataSet::MultiBlock(mb) = &*out else { panic!("expected blocks") };
        assert_eq!(mb.blocks.iter().map(|b| b.0.as_str()).collect::<Vec<_>>(), ["inlet", "outlet", "walls"]);
        let o = mb.blocks[1].1.as_ref().unwrap();
        assert!(o.point_data().unwrap().get("T").is_some(), "point data averaged from faces");
        o.cell_data().unwrap().get("T").unwrap().range(None)
    };
    assert_eq!(outlet_t(&s), (300.0, 300.0));
    s.set_time_index(1);
    s.update_blocking();
    assert_eq!(outlet_t(&s), (335.0, 335.0));
    s.set_param(p, "patches", ParamValue::Str("walls".into()));
    s.update_blocking();
    let out = s.nodes[&p].output.clone().unwrap();
    assert_eq!(out.n_cells(), 40);
}
