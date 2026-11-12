//! The downstream check: references in the nodes below a CAD part, the diff, the default fixes,
//! the assistant's prompt and answer, postponed re-runs, and one undo step for all of it.
use aurafx_filters::ParamValue;
use aurafx_pipeline::{ConflictKind, FixMode, RunMode, Session};
use serde_json::json;

/// A plate with a bore, an Extract Set of the bore's wall below it, and a refinement box.
fn pipeline() -> (Session, u64, u64, u64) {
    let mut s = Session::new();
    let m = s.add_source("cad_model").unwrap();
    s.cad_edit(m, "add", &json!({"kind": "hole", "name": "Bore", "diameter": 8, "depth": 20, "at": [0, 0, 12]})).unwrap();
    let ex = s.add_filter("extract_set", m).unwrap();
    s.set_param(ex, "set", ParamValue::Str("Bore side".into()));
    let rr = s.add_filter("refine_region", m).unwrap();
    s.set_param(rr, "box_min", ParamValue::Vec3([-5.0, -5.0, 0.0]));
    s.set_param(rr, "box_max", ParamValue::Vec3([5.0, 5.0, 8.0]));
    s.update_blocking();
    assert!(s.nodes[&m].error.is_none(), "{:?}", s.nodes[&m].error);
    (s, m, ex, rr)
}

#[test]
fn a_renamed_hole_is_fixed_by_default_and_all_of_it_is_one_undo_step() {
    let (mut s, m, ex, _rr) = pipeline();
    s.cad_enter(m).unwrap();
    s.cad_env_edit("rename", &json!({"feature": "Bore", "to": "Pin hole"})).unwrap();
    let r = s.cad_check().unwrap();
    assert!(!r.diff.is_empty());
    let c = r.conflicts.iter().find(|c| c.node == ex && c.param == "set").expect("the Extract Set refers to the bore's wall");
    assert_eq!(c.reference, "Bore side");
    assert_eq!(c.kind, ConflictKind::Renamed { to: "Pin hole side".into() });
    assert_eq!(c.fix.as_ref().unwrap().value, json!("Pin hole side"));
    assert!(!c.fix.as_ref().unwrap().check, "a rename is certain");
    assert_eq!(r.downstream.len(), 2);
    let a = s.cad_apply(false).unwrap();
    assert_eq!(a.fixes, 1);
    assert_eq!(s.nodes[&ex].params.str("set"), "Pin hole side");
    // One undo takes back the rename of the feature and the fix together.
    assert!(s.undo().is_some());
    assert_eq!(s.nodes[&ex].params.str("set"), "Bore side");
    assert_eq!(s.cad_document(m).unwrap().features[1].name, "Bore");
}

#[test]
fn a_removed_hole_is_offered_but_not_fixed_and_apply_without_fixing_leaves_it() {
    let (mut s, m, ex, _) = pipeline();
    s.cad_enter(m).unwrap();
    s.cad_env_edit("remove", &json!({"feature": "Bore"})).unwrap();
    let r = s.cad_check().unwrap();
    let c = r.conflicts.iter().find(|c| c.node == ex).unwrap();
    assert_eq!(c.kind, ConflictKind::Removed);
    assert!(c.fix.is_none(), "nothing to fix it with");
    let a = s.cad_apply_with(false, FixMode::Default, RunMode::Now).unwrap();
    assert_eq!(a.fixes, 0);
    assert_eq!(s.nodes[&ex].params.str("set"), "Bore side", "left as it was");
    // The node now shows its own error when it runs (no such set).
    s.update_blocking();
    assert!(s.nodes[&ex].error.is_some());
}

#[test]
fn a_replaced_hole_names_its_successor_and_flags_the_fix_as_a_guess() {
    let (mut s, m, ex, _) = pipeline();
    s.cad_enter(m).unwrap();
    s.cad_env_edit("remove", &json!({"feature": "Bore"})).unwrap();
    s.cad_env_edit("add", &json!({"kind": "hole", "name": "Drill", "diameter": 8, "depth": 20, "at": [0, 0, 12]})).unwrap();
    let r = s.cad_check().unwrap();
    let c = r.conflicts.iter().find(|c| c.node == ex).unwrap();
    let f = c.fix.as_ref().expect("one successor");
    assert_eq!(f.value, json!("Drill side"));
    assert!(f.check, "a successor is a guess a person should look at");
}

#[test]
fn a_refinement_box_outside_the_new_part_is_moved_onto_it() {
    let (mut s, m, _, rr) = pipeline();
    s.cad_enter(m).unwrap();
    s.cad_env_edit("parameter", &json!({"name": "width", "expr": 8})).unwrap();
    let r = s.cad_check().unwrap();
    let cs: Vec<_> = r.conflicts.iter().filter(|c| c.node == rr).collect();
    assert_eq!(cs.len(), 2, "{cs:?}");
    assert!(cs.iter().all(|c| c.kind == ConflictKind::Moved { outside: true }));
    let max = cs.iter().find(|c| c.param == "box_max").unwrap();
    let to: Vec<f64> = serde_json::from_value(max.fix.as_ref().unwrap().value.clone()).unwrap();
    assert!((to[0] - 4.0).abs() < 0.2 && (to[1] - 5.0).abs() < 1e-9, "{to:?}");
    s.cad_apply(false).unwrap();
    let v = s.nodes[&rr].params.vec3("box_max", [0.0; 3]);
    assert!((v[0] - 4.0).abs() < 0.2, "{v:?}");
}

#[test]
fn the_assistant_gets_the_diff_and_its_canned_answer_is_parsed_and_applied() {
    let (mut s, m, ex, _) = pipeline();
    s.cad_enter(m).unwrap();
    s.cad_env_edit("remove", &json!({"feature": "Bore"})).unwrap();
    let r = s.cad_check().unwrap();
    let prompt = s.cad_check_prompt(&r);
    assert!(prompt.contains("Bore side") && prompt.contains("is gone") && prompt.contains("extract_set") && prompt.contains("JSON array"), "{prompt}");
    let answer = format!("The wall is gone; use the top face instead.\n```json\n[{{\"node\": {ex}, \"param\": \"set\", \"value\": \"Base z+\", \"reason\": \"the nearest surface\"}}]\n```\nHope that helps [really].");
    let fixes = aurafx_pipeline::cad_check::parse_ai_fixes(&s, &answer).unwrap();
    assert_eq!(fixes.len(), 1);
    assert_eq!((fixes[0].node, fixes[0].param.as_str(), &fixes[0].value), (ex, "set", &json!("Base z+")));
    s.cad_apply_with(false, FixMode::Fixes(fixes), RunMode::Now).unwrap();
    assert_eq!(s.nodes[&ex].params.str("set"), "Base z+");
    // Bad answers are refused with a reason.
    let e = |t: &str| aurafx_pipeline::cad_check::parse_ai_fixes(&s, t).unwrap_err().to_string();
    assert!(e("no list here").contains("no JSON list"));
    assert!(e(r#"[{"node": 999, "param": "set", "value": 1}]"#).contains("no node 999"));
    assert!(e(&format!(r#"[{{"node": {ex}, "param": "nope", "value": 1}}]"#)).contains("no parameter 'nope'"));
}

#[test]
fn names_inside_command_text_are_found_whole_and_the_rerun_can_wait() {
    let mut s = Session::new();
    let m = s.add_source("cad_model").unwrap();
    s.cad_edit(m, "add", &json!({"kind": "hole", "name": "Bore", "diameter": 8, "depth": 20, "at": [0, 0, 12]})).unwrap();
    let g = s.add_filter("python_calculator", m).or_else(|_| s.add_filter("calculator", m)).unwrap();
    // A calculator expression is text: it mentions the face by name in quotes.
    let key = s.nodes[&g].def().unwrap().params.iter().find(|p| matches!(p.kind, aurafx_filters::ParamKind::Expression)).map(|p| p.key.clone()).unwrap();
    s.set_param(g, &key, ParamValue::Str("x + 'Bore side' + 'Bore sides'".into()));
    s.update_blocking();
    s.cad_enter(m).unwrap();
    s.cad_env_edit("rename", &json!({"feature": "Bore", "to": "Pin"})).unwrap();
    let r = s.cad_check().unwrap();
    let c = r.conflicts.iter().find(|c| c.node == g).unwrap();
    // 'Bore sides' is another name: only the whole 'Bore side' is rewritten.
    assert_eq!(c.fix.as_ref().unwrap().value, json!("x + 'Pin side' + 'Bore sides'"));
    // Postponed run: the nodes below keep their old results until released.
    let a = s.cad_apply_with(false, FixMode::Default, RunMode::Later).unwrap();
    assert_eq!(a.held, 1);
    s.update_blocking();
    assert!(s.nodes[&g].dirty, "held back");
    s.cad_run_held();
    s.update_blocking();
    assert!(!s.nodes[&g].dirty);
}

/// A direction is not a position: a clip's normal that happens to lie inside the old part's box
/// is left alone when the part shrinks, and a node working on face numbers is told when they
/// shifted.
#[test]
fn directions_are_left_alone_and_face_numbers_are_flagged() {
    let (mut s, m, _, _) = pipeline();
    let clip = s.add_filter("clip", m).unwrap();
    s.set_param_json(clip, "normal", &json!([1.0, 0.0, 0.0]));
    let th = s.add_filter("threshold", m).unwrap();
    s.set_param_json(th, "array", &json!({"assoc": "Cells", "name": "FaceId"}));
    s.update_blocking();
    s.cad_enter(m).unwrap();
    s.cad_env_edit("parameter", &json!({"name": "width", "expr": 1})).unwrap();
    s.cad_env_edit("remove", &json!({"feature": "Bore"})).unwrap();
    let r = s.cad_check().unwrap();
    assert!(!r.conflicts.iter().any(|c| c.node == clip), "{:?}", r.conflicts.iter().filter(|c| c.node == clip).collect::<Vec<_>>());
    let c = r.conflicts.iter().find(|c| c.node == th).expect("the threshold on FaceId");
    assert!(matches!(c.kind, ConflictKind::Renumbered { faces: true, .. }) && c.fix.is_none(), "{c:?}");
}
