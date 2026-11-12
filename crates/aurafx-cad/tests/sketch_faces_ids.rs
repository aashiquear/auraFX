//! Sketch entities keep their numbers (and the faces made from them their names) when the
//! outline changes, older files get numbers on load, and a sketch drawn on a face follows it.
use aurafx_cad::edit::apply_op;
use aurafx_cad::kernel::{evaluate_with, KernelChoice};
use aurafx_cad::{Document, Shape, Sketch};
use serde_json::json;

fn run(d: &mut Document, op: &str, args: serde_json::Value) -> serde_json::Value {
    apply_op(d, op, &args).unwrap_or_else(|e| panic!("{op}: {e}"))
}

fn sketch_of(d: &Document, feature: &str) -> Sketch {
    d.features.iter().find(|f| f.name == feature).and_then(|f| f.shape.sketch()).cloned().unwrap()
}

#[test]
fn a_side_keeps_its_name_when_the_outline_gains_and_loses_lines() {
    let mut d = Document::new("p");
    run(
        &mut d,
        "sketch_extrude",
        json!({"name": "P", "depth": 6, "sketch": {"plane": "XY", "items": [{"polygon": [[0, 0], [20, 0], [20, 10], [10, 16], [0, 10]]}]}}),
    );
    let sk = sketch_of(&d, "P");
    assert_eq!(sk.entity_ids, vec![1, 2, 3, 4, 5]);
    for choice in [KernelChoice::Implicit, KernelChoice::Brep] {
        let mut d = d.clone();
        let before = evaluate_with(choice, &d, 64, true).unwrap();
        let side3 = before.face_by_name("P side 3").expect("the third line's face").clone();
        // Between (20, 10) and (10, 16).
        assert!((side3.centroid[0] - 15.0).abs() < 0.3 && (side3.centroid[1] - 13.0).abs() < 0.3, "{choice:?}: {:?}", side3.centroid);
        // The bottom line goes; two lines through a new corner take its place.
        let mut sk = sketch_of(&d, "P");
        let (a, b) = match &sk.entities[0] {
            aurafx_cad::Entity::Line { a, b, .. } => (*a, *b),
            _ => unreachable!(),
        };
        sk.remove_entity(0);
        let c = sk.add_fixed_point(10.0, -4.0);
        sk.add_line(a, c);
        sk.add_line(c, b);
        run(&mut d, "set_sketch", json!({"feature": "P", "sketch_json": serde_json::to_value(&sk).unwrap()}));
        assert_eq!(sketch_of(&d, "P").entity_ids, vec![2, 3, 4, 5, 6, 7], "the old numbers stay, the new lines get new ones");
        let after = evaluate_with(choice, &d, 64, true).unwrap();
        let again = after.face_by_name("P side 3").expect("still there under its name");
        for k in 0..3 {
            assert!((again.centroid[k] - side3.centroid[k]).abs() < 0.05, "{choice:?}: {:?} vs {:?}", again.centroid, side3.centroid);
        }
        assert_eq!(again.id, side3.id, "{choice:?}: and under its stable id");
        let names: Vec<&str> = after.faces().map(|(_, f)| f.name.as_str()).collect();
        assert!(names.contains(&"P side 6") && names.contains(&"P side 7") && !names.contains(&"P side 1"), "{choice:?}: {names:?}");
    }
    // A number is never used again, also after its entity went.
    let mut sk = sketch_of(&d, "P");
    sk.remove_entity(4);
    sk.add_line_xy([0.0, 10.0], [0.0, 0.0]);
    sk.normalize_entity_ids();
    assert_eq!(sk.entity_ids, vec![1, 2, 3, 4, 6]);
}

#[test]
fn files_without_entity_numbers_load_and_name_their_faces_as_before() {
    let mut d = Document::new("old");
    // A rectangle by its center: the construction diagonal comes before the four lines.
    run(
        &mut d,
        "sketch_extrude",
        json!({"name": "Plate", "depth": 5, "sketch": {"plane": "XY", "items": [{"rectangle": {"center": [0, 0], "w": 30, "h": 20}}]}}),
    );
    let now = evaluate_with(KernelChoice::Implicit, &d, 48, true).unwrap();
    let mut v: serde_json::Value = serde_json::from_str(&d.to_afxcad()).unwrap();
    let sk = v.pointer_mut("/document/features/0/shape/sketch").unwrap().as_object_mut().unwrap();
    assert!(sk.remove("entity_ids").is_some() && sk.remove("next_entity_id").is_some());
    let old = Document::from_afxcad(&v.to_string()).unwrap();
    let ids = sketch_of(&old, "Plate").entity_ids;
    assert_eq!(ids.len(), 5);
    assert_eq!(old, d, "the numbers given on load are the ones a new sketch gets");
    let then = evaluate_with(KernelChoice::Implicit, &old, 48, true).unwrap();
    for n in ["Plate side 1", "Plate side 2", "Plate side 3", "Plate side 4"] {
        let (a, b) = (now.face_by_name(n).unwrap(), then.face_by_name(n).unwrap());
        assert!((0..3).all(|k| (a.centroid[k] - b.centroid[k]).abs() < 1e-9), "{n}");
    }
    assert!(then.face_by_name("Plate side 5").is_none(), "the construction line makes no face");
}

#[test]
fn a_sketch_on_a_face_follows_the_face_and_says_so_when_the_face_is_gone() {
    let mut d = Document::new("plate");
    run(&mut d, "parameter", json!({"name": "t", "expr": 8}));
    run(&mut d, "add", json!({"kind": "box", "name": "Plate", "size": [40, 30, "t"], "at": [0, 0, "t / 2"]}));
    run(&mut d, "sketch_extrude", json!({"name": "Boss", "depth": 5, "sketch": {"face": "Plate z+", "items": [{"circle": {"center": [5, 0], "r": 5}}]}}));
    let sk = sketch_of(&d, "Boss");
    assert_eq!(sk.face.as_deref(), Some("Plate z+"));
    assert!((sk.offset - 8.0).abs() < 1e-6, "{}", sk.offset);
    let boss = std::f64::consts::PI * 25.0 * 5.0;
    for (t, choice) in [(8.0, KernelChoice::Implicit), (12.0, KernelChoice::Implicit), (12.0, KernelChoice::Auto), (3.0, KernelChoice::Auto)] {
        run(&mut d, "parameter", json!({"name": "t", "expr": t}));
        let ev = evaluate_with(choice, &d, 72, true).unwrap();
        let top = ev.bodies[0].bounds.1[2];
        assert!((top - (t + 5.0)).abs() < 0.05, "{choice:?} t = {t}: the boss stands on the plate's top, {top}");
        let want = 40.0 * 30.0 * t + boss;
        assert!((ev.volume() - want).abs() / want < 0.02, "{choice:?} t = {t}: {} vs {want}", ev.volume());
    }
    // The exact kernel gets the plane to a nanometre: the boss and the plate make one body with no gap.
    let exact = evaluate_with(KernelChoice::Brep, &d, 48, true).unwrap();
    assert_eq!(exact.bodies.len(), 1);
    // A pocket on the same face goes into the part, a boss on a side face out of it.
    run(&mut d, "parameter", json!({"name": "t", "expr": 8}));
    let v0 = evaluate_with(KernelChoice::Implicit, &d, 72, true).unwrap().volume();
    run(
        &mut d,
        "sketch_extrude",
        json!({"name": "Pocket", "op": "cut", "depth": 2, "sketch": {"face": "Plate z+", "items": [{"rectangle": {"center": [-12, 0], "w": 6, "h": 6}}]}}),
    );
    let v1 = evaluate_with(KernelChoice::Implicit, &d, 72, true).unwrap().volume();
    assert!((v0 - v1 - 72.0).abs() < 4.0, "{}", v0 - v1);
    run(&mut d, "sketch_extrude", json!({"name": "Lug", "depth": 4, "sketch": {"face": "Plate x-", "items": [{"circle": {"center": [0, 4], "r": 2}}]}}));
    let ev = evaluate_with(KernelChoice::Implicit, &d, 72, true).unwrap();
    assert!((ev.bodies[0].bounds.0[0] + 24.0).abs() < 0.1, "{:?}", ev.bodies[0].bounds);
    run(&mut d, "remove", json!({"feature": "Lug"}));
    run(&mut d, "remove", json!({"feature": "Pocket"}));

    // The face goes (its feature is renamed, so the name is no longer on the part): the feature says so.
    run(&mut d, "rename", json!({"feature": "Plate", "to": "Slab"}));
    let e = evaluate_with(KernelChoice::Implicit, &d, 48, true).unwrap_err().to_string();
    assert!(e.contains("'Boss'") && e.contains("'Plate z+'") && e.contains("pick another face"), "{e}");
    assert!(evaluate_with(KernelChoice::Auto, &d, 48, true).is_err());
    // Repair: another face for the sketch. A face the part does not have is refused and changes nothing.
    let before = d.clone();
    let err = apply_op(&mut d, "sketch_face", &json!({"feature": "Boss", "face": "Nope"})).unwrap_err().to_string();
    assert!(err.contains("no face 'Nope'"), "{err}");
    assert_eq!(d, before);
    run(&mut d, "sketch_face", json!({"feature": "Boss", "face": "Slab z+"}));
    let ev = evaluate_with(KernelChoice::Implicit, &d, 72, true).unwrap();
    assert!((ev.bodies[0].bounds.1[2] - 13.0).abs() < 0.05);
    // On the bottom face the same sketch grows the other way.
    run(&mut d, "sketch_face", json!({"feature": "Boss", "face": "Slab z-"}));
    assert_eq!(sketch_of(&d, "Boss").offset, 0.0);
    // Let go of the face: the sketch stays where it is and no longer follows.
    run(&mut d, "sketch_face", json!({"feature": "Boss", "face": null}));
    run(&mut d, "parameter", json!({"name": "t", "expr": 20}));
    assert!(sketch_of(&d, "Boss").face.is_none());
    assert!(evaluate_with(KernelChoice::Implicit, &d, 48, true).is_ok());
    assert!(matches!(d.features[1].shape, Shape::SketchExtrude { .. }));
}

#[test]
fn a_sketch_follows_a_face_that_turns() {
    // A sketch on the plane z = 5 whose face is now tilted about x: the plane goes with it, the
    // sketch's x axis stays along x.
    let (mut sk, against) = Sketch::on_face([0.0, 0.0, 5.0], [0.0, 0.0, 1.0], "F").unwrap();
    assert!(!against);
    let n = [0.0, -(0.5f64).sin(), (0.5f64).cos()];
    sk.follow_face([0.0, 0.0, 5.0], n);
    let (r, o) = sk.placement_frame();
    assert!((r.z_axis - glam::DVec3::from_array(n)).length() < 1e-12 && (r.x_axis - glam::DVec3::X).length() < 1e-12, "{r:?}");
    assert!((o - glam::DVec3::new(0.0, 0.0, 5.0)).dot(r.z_axis).abs() < 1e-12);
    // A face seen from behind (the bottom of a plate) reverses a boss.
    assert!(Sketch::on_face([0.0; 3], [0.0, 0.0, -1.0], "B").unwrap().1);
}
