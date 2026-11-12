//! The CAD environment's session layer: enter, edit the working copy, evaluate, apply as one
//! undo step, discard.
use aurafx_data::DataSet;
use aurafx_pipeline::Session;
use serde_json::json;

fn faces(s: &Session, id: u64) -> usize {
    match s.nodes[&id].output.as_deref() {
        Some(DataSet::PolyData(p)) => p.field_data.get("FaceNames").unwrap().len(),
        _ => 0,
    }
}

#[test]
fn enter_edit_apply_discard() {
    let mut s = Session::new();
    let m = s.add_source("cad_model").unwrap();
    s.update_blocking();
    assert_eq!(faces(&s, m), 6, "the default plate");
    assert!(s.cad_env_edit("add", &json!({})).is_err(), "not editing yet");

    // Enter: the pipeline pauses, the node keeps what it had.
    s.cad_enter(m).unwrap();
    assert_eq!(s.cad_editing(), Some(m));
    s.nodes.get_mut(&m).unwrap().dirty = true;
    assert!(s.take_work().is_none(), "nothing runs while the environment is open");
    s.nodes.get_mut(&m).unwrap().dirty = false;

    // A blind hole: its wall and its bottom are two faces more.
    s.cad_env_edit("add", &json!({"kind": "hole", "name": "Bore", "diameter": 6, "depth": 3, "at": [0, 0, 8]})).unwrap();
    assert_eq!(s.cad_document(m).unwrap().features.len(), 2, "reads see the working copy");
    assert_eq!(s.cad_overrides(m).len(), 0);
    let sum = s.cad_env_evaluate(64).unwrap();
    assert_eq!(faces(&s, m), 8, "{:?}", sum.faces);
    assert!(sum.faces.iter().any(|f| f.name == "Bore bottom") && sum.faces.iter().any(|f| f.name == "Bore side"));
    let expect = 40.0 * 30.0 * 8.0 - std::f64::consts::PI * 9.0 * 3.0;
    assert!((sum.volume - expect).abs() / expect < 0.01, "{}", sum.volume);

    // The environment's own undo and redo.
    s.cad_env_undo().unwrap();
    assert_eq!(s.cad_document(m).unwrap().features.len(), 1);
    s.cad_env_redo().unwrap();
    assert_eq!(s.cad_document(m).unwrap().features.len(), 2);
    // A rollback marker builds only the features before it.
    s.cad_env_rollback(Some(1));
    s.cad_env_evaluate(64).unwrap();
    assert_eq!(faces(&s, m), 6);
    s.cad_env_rollback(None);
    s.cad_env_evaluate(64).unwrap();
    assert_eq!(faces(&s, m), 8);

    // A part that does not build is refused at Apply, with the reason.
    s.cad_env_edit("parameter", &json!({"name": "w2", "expr": "40"})).unwrap();
    s.cad_env_edit("suppress", &json!({"feature": "Base"})).unwrap();
    let e = s.cad_apply(false).unwrap_err().to_string();
    assert!(e.contains("does not build"), "{e}");
    s.cad_env_edit("suppress", &json!({"feature": "Base", "suppressed": false})).unwrap();

    // Apply: the node has the document, the pipeline resumes, it is one undo step.
    let undo_before = s.undo_label().map(str::to_string);
    assert_eq!(undo_before.as_deref(), Some("Edit CAD"), "one step recorded on entry, none per edit");
    let applied = s.cad_apply(false).unwrap();
    assert_eq!(applied.features, 2);
    assert_eq!(s.cad_editing(), None);
    s.update_blocking();
    assert_eq!(faces(&s, m), 8);
    assert!(s.undo().is_some());
    s.update_blocking();
    assert_eq!(s.cad_document(m).unwrap().features.len(), 1, "one undo takes back every edit of the session");
    assert_eq!(faces(&s, m), 6);

    // Discard puts the node back as it was.
    s.cad_enter(m).unwrap();
    s.cad_env_edit("add", &json!({"kind": "sphere", "radius": 4, "at": [0, 0, 20]})).unwrap();
    s.cad_env_evaluate(32).unwrap();
    assert_ne!(faces(&s, m), 6);
    s.cad_discard().unwrap();
    assert_eq!(s.cad_editing(), None);
    assert_eq!(faces(&s, m), 6, "the output is the old one");
    assert_eq!(s.cad_document(m).unwrap().features.len(), 1);
    assert_ne!(s.undo_label(), Some("Edit CAD"));

    // Apply and keep editing leaves the environment open.
    s.cad_enter(m).unwrap();
    s.cad_env_edit("add", &json!({"kind": "hole", "diameter": 4, "depth": 20, "at": [5, 5, 8]})).unwrap();
    let a = s.cad_apply(true).unwrap();
    assert!(a.kept_editing && s.cad_editing() == Some(m));
    assert!(!s.cad_edit.as_ref().unwrap().changed);
    s.cad_discard().unwrap();
}

#[test]
fn only_one_part_is_edited_at_a_time_and_other_nodes_are_refused() {
    let mut s = Session::new();
    let a = s.add_source("cad_model").unwrap();
    let b = s.add_source("cad_model").unwrap();
    let sphere = s.add_source("sphere").unwrap();
    s.cad_enter(a).unwrap();
    let e = s.cad_enter(b).unwrap_err().to_string();
    assert!(e.contains("already open"), "{e}");
    s.cad_discard().unwrap();
    assert!(s.cad_enter(sphere).unwrap_err().to_string().contains("not a CAD Model"));
}

/// The worked example from a CAD file: STEP -> CAD Model -> overlay hex mesh (the faces are side
/// sets) -> grains -> a MOOSE case, end to end.
#[test]
fn step_to_mesh_to_grains_to_solver_case() {
    let step = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../testdata/cad/plate_with_hole.step");
    if !step.exists() {
        return; // tests that need a missing file skip themselves
    }
    let mut s = Session::new();
    let cad = s.open_file(&step).unwrap();
    assert!(s.is_cad_model(cad));
    let mesh = s.add_filter("overlay_mesh", cad).unwrap();
    s.set_param(mesh, "cell", aurafx_filters::ParamValue::Float(2.5));
    let grains = s.add_filter("assign_grains", mesh).unwrap();
    s.set_param(grains, "grains", aurafx_filters::ParamValue::Int(12));
    s.set_param(grains, "blocks", aurafx_filters::ParamValue::Bool(true));
    s.set_param(grains, "boundaries", aurafx_filters::ParamValue::Bool(true));
    s.update_blocking();
    for id in [cad, mesh, grains] {
        assert!(s.nodes[&id].error.is_none(), "{}: {:?}", s.nodes[&id].name, s.nodes[&id].error);
    }
    let out = s.nodes[&grains].output.clone().unwrap();
    let m = aurafx_mesh::hex::HexMesh::from_dataset(&out).unwrap();
    assert!(m.block_names.len() >= 8, "one block per grain: {}", m.block_names.len());
    assert!(m.side_set("grain boundaries").is_some());
    // The imported solid's faces survive as sets (the STEP file's own face numbers).
    assert!(m.side_sets.iter().any(|(n, _)| n.contains("face")), "{:?}", m.side_sets.iter().map(|(n, _)| n.clone()).collect::<Vec<_>>());
    let dir = std::env::temp_dir().join(format!("afx_cad_case_{}", std::process::id()));
    // Fix one face of the part and load another, by the names the STEP file's faces got.
    let faces: Vec<String> = m.side_sets.iter().filter(|(n, v)| n.contains("face") && !v.is_empty()).map(|(n, _)| n.clone()).collect();
    assert!(faces.len() >= 2, "{faces:?}");
    let opts = aurafx_pipeline::cases::CaseOptions { fixed: faces[0].clone(), loaded: faces[1].clone(), ..Default::default() };
    let case = aurafx_pipeline::cases::write_case(&out, "moose", &dir, &opts).unwrap();
    assert!(!case.files.is_empty() && dir.join("mesh.e").exists() && dir.join("grains.tex").exists());
    std::fs::remove_dir_all(&dir).ok();
}

/// A tetrahedral mesh of a CAD part keeps the faces' names as side sets, like the hexahedral one.
#[test]
fn a_tet_mesh_of_a_cad_part_has_its_faces_as_named_side_sets() {
    let mut s = Session::new();
    let cad = s.add_source("cad_model").unwrap();
    let tet = s.add_filter("tet_mesh", cad).unwrap();
    s.set_param(tet, "cell", aurafx_filters::ParamValue::Float(4.0));
    s.update_blocking();
    for id in [cad, tet] {
        assert!(s.nodes[&id].error.is_none(), "{}: {:?}", s.nodes[&id].name, s.nodes[&id].error);
    }
    let out = s.nodes[&tet].output.clone().unwrap();
    let m = aurafx_mesh::tet::TetMesh::from_dataset(&out).unwrap();
    let names: Vec<String> = m.side_sets.iter().filter(|(_, v)| !v.is_empty()).map(|(n, _)| n.clone()).collect();
    // The default plate has a top, a bottom and four sides, named after its feature.
    assert!(names.contains(&"Base top".to_string()) && names.contains(&"Base bottom".to_string()), "{names:?}");
    assert!(names.iter().filter(|n| n.starts_with("Base side")).count() == 4, "{names:?}");
    assert!(!names.iter().any(|n| n.starts_with("surface")), "no numbered patches left: {names:?}");
}

/// The environment's live preview shows every part of an assembly, not only the document's own features.
#[test]
fn the_environments_preview_shows_every_part_of_an_assembly() {
    let mut s = Session::new();
    let cad = s.add_source("cad_model").unwrap();
    s.cad_enter(cad).unwrap();
    let alone = s.cad_env_evaluate(48).unwrap();
    s.cad_env_edit("component_add", &serde_json::json!({"name": "Far", "template": "bracket", "at": [300, 0, 0]})).unwrap();
    let both = s.cad_env_evaluate(48).unwrap();
    assert!(both.volume > alone.volume + 5000.0, "{} then {}", alone.volume, both.volume);
    assert!(both.bounds.1[0] > 300.0 && both.bounds.0[0] < alone.bounds.1[0], "the box covers both: {:?}", both.bounds);
    assert!(both.faces.iter().any(|f| f.name.starts_with("Far/")), "faces of a part are named Part/Face");
    s.cad_discard().unwrap();
}

/// Faults found in a review of the environment: each case failed before its fix.
#[test]
fn edits_are_all_or_nothing_and_features_stay_distinct() {
    let mut s = Session::new();
    let m = s.add_source("cad_model").unwrap();
    s.update_blocking();
    s.cad_enter(m).unwrap();

    // A refused edit leaves the working copy exactly as it was (a parameter cycle used to stay).
    s.cad_env_edit("parameter", &json!({"name": "a", "expr": "10"})).unwrap();
    let before = s.cad_document(m).unwrap();
    assert!(s.cad_env_edit("parameter", &json!({"name": "a", "expr": "a + 1"})).is_err(), "a cycle");
    assert!(s.cad_env_edit("remove_parameter", &json!({"name": "nope"})).is_err());
    assert_eq!(s.cad_document(m).unwrap(), before);

    // Two features never share a name: the second fillet, mirror or combine gets its own, so
    // the tree's actions reach the one clicked.
    s.cad_env_edit("mirror_plane", &json!({"origin": [0, 0, 0], "normal": [1, 0, 0]})).unwrap();
    s.cad_env_edit("mirror_plane", &json!({"origin": [0, 0, 0], "normal": [0, 1, 0]})).unwrap();
    let d = s.cad_document(m).unwrap();
    let names: Vec<&str> = d.features.iter().map(|f| f.name.as_str()).collect();
    assert_eq!(&names[1..], ["Mirror", "Mirror 2"], "{names:?}");
    assert!(s.cad_env_edit("rename", &json!({"feature": "Mirror 2", "to": "Mirror"})).is_err(), "a taken name");
    assert!(s.cad_env_edit("rename", &json!({"feature": "Mirror 2", "to": "  "})).is_err(), "an empty name");
    s.cad_env_edit("remove", &json!({"feature": "Mirror"})).unwrap();
    s.cad_env_edit("remove", &json!({"feature": "Mirror 2"})).unwrap();

    // A dimension in use cannot be removed; one that is not can.
    s.cad_env_edit("add", &json!({"kind": "cylinder", "name": "Post", "radius": "a / 2", "height": 5, "at": [0, 0, 8]})).unwrap();
    let e = s.cad_env_edit("remove_parameter", &json!({"name": "a"})).unwrap_err().to_string();
    assert!(e.contains("still used"), "{e}");
    assert!(s.cad_document(m).unwrap().parameters.iter().any(|p| p.name == "a"));

    // Removing a modifier drops its bindings and renumbers the later ones: the part still builds.
    s.cad_env_edit("modifier", &json!({"feature": "Post", "modifier": {"kind": "Offset", "by": [1, 0, 0]}})).unwrap();
    s.cad_env_edit("modifier", &json!({"feature": "Post", "modifier": {"kind": "LinearPattern", "count": 2, "step": [10, 0, 0]}})).unwrap();
    s.cad_env_edit("bind", &json!({"feature": "Post", "path": "/modifiers/1/count", "expr": "a / 5"})).unwrap();
    s.cad_env_edit("remove_modifier", &json!({"feature": "Post", "index": 0})).unwrap();
    let d = s.cad_document(m).unwrap();
    let post = d.features.iter().find(|f| f.name == "Post").unwrap();
    assert!(post.bindings.iter().any(|b| b.path == "/modifiers/0/count"), "{:?}", post.bindings);
    d.resolved().expect("still resolves");

    // A suppressed feature with a broken expression does not stop the part.
    s.cad_env_edit("suppress", &json!({"feature": "Post"})).unwrap();
    s.cad_env_evaluate(32).unwrap();

    // While rolled back, a new feature goes in at the marker and is built.
    s.cad_env_edit("suppress", &json!({"feature": "Post", "suppressed": false})).unwrap();
    s.cad_env_rollback(Some(1));
    s.cad_env_edit("add", &json!({"kind": "hole", "name": "Bore", "diameter": 4, "depth": 3, "at": [0, 0, 8]})).unwrap();
    let d = s.cad_document(m).unwrap();
    let names: Vec<&str> = d.features.iter().map(|f| f.name.as_str()).collect();
    assert_eq!(names, ["Base", "Bore", "Post"]);
    assert_eq!(s.cad_edit.as_ref().unwrap().rollback, Some(2));
    let sum = s.cad_env_evaluate(48).unwrap();
    assert!(
        sum.faces.iter().any(|f| f.name.starts_with("Bore")) && !sum.faces.iter().any(|f| f.name.starts_with("Post")),
        "{:?}",
        sum.faces.iter().map(|f| &f.name).collect::<Vec<_>>()
    );
    s.cad_env_rollback(None);

    // Apply and keep editing, then discard: the applied part stays undoable as one step.
    s.cad_apply(true).unwrap();
    assert_eq!(s.undo_label(), Some("Edit CAD"));
    s.cad_env_edit("add", &json!({"kind": "box", "name": "Extra"})).unwrap();
    s.cad_discard().unwrap();
    assert_eq!(s.undo_label(), Some("Edit CAD"), "the step of the applied edit survives the discard");
    assert_eq!(s.cad_document(m).unwrap().features.len(), 3, "the applied part, without the discarded box");

    // Deleting the part while it is being edited closes the environment.
    s.cad_enter(m).unwrap();
    s.remove_node(m);
    assert_eq!(s.cad_editing(), None);
}

/// Features written without ids, or with the same id twice, get ids of their own on reading.
#[test]
fn ids_are_made_unique_on_load() {
    let mut doc = aurafx_cad::Document::new("p");
    doc.push(aurafx_cad::Feature::new("a", aurafx_cad::BooleanOp::Add, aurafx_cad::Shape::Sphere { radius: 1.0 }));
    doc.push(aurafx_cad::Feature::new("b", aurafx_cad::BooleanOp::Add, aurafx_cad::Shape::Sphere { radius: 2.0 }));
    let mut v: serde_json::Value = serde_json::from_str(&doc.to_json()).unwrap();
    for f in v["features"].as_array_mut().unwrap() {
        f.as_object_mut().unwrap().remove("id");
    }
    let back = aurafx_cad::Document::from_json(&v.to_string()).unwrap();
    assert!(
        back.features[0].id != back.features[1].id && back.features.iter().all(|f| f.id.0 != 0),
        "{:?}",
        back.features.iter().map(|f| f.id).collect::<Vec<_>>()
    );
    // Every template has distinct feature ids and names.
    for (title, t) in aurafx_cad::templates() {
        let mut ids: Vec<u64> = t.features.iter().map(|f| f.id.0).collect();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), t.features.len(), "{title}: an id used twice");
        assert!(!ids.contains(&0), "{title}: a feature without an id");
    }
}
