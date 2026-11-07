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
    assert!(sum.faces.iter().any(|f| f.0 == "Bore bottom") && sum.faces.iter().any(|f| f.0 == "Bore side"));
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
