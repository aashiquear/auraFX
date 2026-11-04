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
