//! Mates between the parts of an assembly: solved into placements on every build, reported
//! when they cannot be met, and moved by their values.
use aurafx_cad::edit::apply_op;
use aurafx_cad::kernel::{evaluate_with, KernelChoice};
use aurafx_cad::{Document, Evaluated};
use glam::DVec3;
use serde_json::{json, Value};

fn run(d: &mut Document, op: &str, args: Value) -> Value {
    apply_op(d, op, &args).unwrap_or_else(|e| panic!("{op}: {e}"))
}

fn plate(name: &str, hole: bool) -> Value {
    let mut d = Document::new(name);
    run(&mut d, "add", json!({"kind": "box", "name": "Slab", "size": [40, 40, 10], "at": [0, 0, 5]}));
    if hole {
        run(&mut d, "add", json!({"kind": "hole", "name": "Bore", "diameter": 8, "depth": 30, "at": [0, 0, 10]}));
    }
    serde_json::to_value(d).unwrap()
}

fn face(ev: &Evaluated, name: &str) -> (DVec3, DVec3) {
    // The face's plane as the mates see it (the plain average over a face's triangles is rougher).
    let g = aurafx_cad::face_geometry(ev, name).unwrap_or_else(|| panic!("no face {name}"));
    assert!(g.flat, "{name} is flat");
    (DVec3::from_array(g.point), DVec3::from_array(g.dir))
}

fn assembly() -> Document {
    let mut a = Document::new("Assembly");
    run(&mut a, "component_add", json!({"name": "Base", "document": plate("Base", false)}));
    // Somewhere else, and turned.
    run(&mut a, "component_add", json!({"name": "Cap", "document": plate("Cap", false), "at": [100, 30, 50], "rotate": [20, 0, 30]}));
    a
}

#[test]
fn coincident_distance_and_flush_mates_seat_a_part_and_a_value_moves_it() {
    for choice in [KernelChoice::Implicit, KernelChoice::Auto] {
        let mut a = assembly();
        run(&mut a, "mate", json!({"kind": "fixed", "a": "Base"}));
        let made = run(&mut a, "mate", json!({"kind": "coincident", "a": "Base/Slab z+", "b": "Cap/Slab z-"}));
        assert_eq!(made["mate"], "Coincident 1");
        assert_eq!(made["dof"], 3, "the cap can still slide two ways and turn on the base");
        let ev = evaluate_with(choice, &a, 48, true).unwrap();
        let (c, n) = face(&ev, "Cap/Slab z-");
        assert!((c.z - 10.0).abs() < 1e-3 && (n + DVec3::Z).length() < 1e-3, "{choice:?}: {c} {n}");
        assert_eq!(ev.mate_dof, Some(3));
        assert!(ev.mates.iter().all(|m| m.error.is_none()), "{:?}", ev.mates);
        // The base did not move: it is fixed.
        assert!((face(&ev, "Base/Slab z+").0 - DVec3::new(0.0, 0.0, 10.0)).length() < 0.3);
        // 5 mm between the sides that face each other, and the fronts flush: nothing is left to move.
        run(&mut a, "mate", json!({"kind": "distance", "a": "Base/Slab x+", "b": "Cap/Slab x-", "value": 5, "name": "Gap"}));
        let made = run(&mut a, "mate", json!({"kind": "coincident", "a": "Base/Slab y-", "b": "Cap/Slab y-", "flip": true}));
        assert_eq!(made["dof"], 0);
        assert!(made["status"].as_str().unwrap().contains("fully held"));
        let ev = evaluate_with(choice, &a, 48, true).unwrap();
        let (c, n) = face(&ev, "Cap/Slab x-");
        assert!((c.x - 25.0).abs() < 2e-3 && (n + DVec3::X).length() < 1e-3, "{choice:?}: {c} {n}");
        assert!((face(&ev, "Cap/Slab y-").0.y + 20.0).abs() < 2e-3);
        let cap = &ev.placements.iter().find(|p| p.0 == "Cap").unwrap().1;
        assert!((cap.translate[0] - 45.0).abs() < 2e-3 && cap.translate[1].abs() < 2e-3 && (cap.translate[2] - 10.0).abs() < 2e-3, "{cap:?}");
        // The value of the distance moves the cap: this is how an assembly is put in motion.
        run(&mut a, "mate_set", json!({"mate": "Gap", "value": 12}));
        let ev = evaluate_with(choice, &a, 48, true).unwrap();
        assert!((face(&ev, "Cap/Slab x-").0.x - 32.0).abs() < 2e-3);
        // ... and it may follow a dimension of the assembly.
        run(&mut a, "parameter", json!({"name": "gap", "expr": 20}));
        run(&mut a, "mate_set", json!({"mate": "Gap", "value": "gap + 1"}));
        let ev = evaluate_with(choice, &a, 48, true).unwrap();
        assert!((face(&ev, "Cap/Slab x-").0.x - 41.0).abs() < 2e-3);
        // Interference uses the mated places: seated like this the two parts only touch.
        assert!(aurafx_cad::kernel::assemble::component_interference(choice, &a, 0.5).unwrap().is_empty());
    }
}

#[test]
fn a_pin_goes_on_the_axis_of_a_hole_and_an_angle_opens_a_lid() {
    let mut a = Document::new("Assembly");
    run(&mut a, "component_add", json!({"name": "Base", "document": plate("Base", true), "at": [10, 20, 0]}));
    let mut pin = Document::new("Pin");
    run(&mut pin, "add", json!({"kind": "cylinder", "name": "Rod", "radius": 4, "height": 30}));
    run(&mut a, "component_add", json!({"name": "Pin", "document": serde_json::to_value(pin).unwrap(), "at": [70, -10, 5], "rotate": [35, 10, 0]}));
    run(&mut a, "mate", json!({"kind": "fixed", "a": "Base"}));
    let made = run(&mut a, "mate", json!({"kind": "concentric", "a": "Base/Bore side", "b": "Pin/Rod side"}));
    assert_eq!(made["dof"], 2, "the pin slides along the hole and spins in it");
    let ev = evaluate_with(KernelChoice::Auto, &a, 48, true).unwrap();
    let pl = &ev.placements.iter().find(|p| p.0 == "Pin").unwrap().1;
    // The rod's axis is its own z through its origin: now along z through the hole at (10, 20).
    assert!((pl.translate[0] - 10.0).abs() < 0.02 && (pl.translate[1] - 20.0).abs() < 0.02, "{pl:?}");
    let side = ev.face_by_name("Pin/Rod top").unwrap();
    assert!(side.direction[2].abs() > 0.9999, "{:?}", side.direction);
    // A flat face cannot be concentric, a round one not coincident: said at once, nothing changed.
    let before = a.clone();
    let e = apply_op(&mut a, "mate", &json!({"kind": "concentric", "a": "Base/Slab z+", "b": "Pin/Rod side"})).unwrap_err().to_string();
    assert!(e.contains("two round faces") && e.contains("is flat"), "{e}");
    let e = apply_op(&mut a, "mate", &json!({"kind": "coincident", "a": "Base/Slab z+", "b": "Pin/Nope"})).unwrap_err().to_string();
    assert!(e.contains("has no face 'Nope'"), "{e}");
    let e = apply_op(&mut a, "mate", &json!({"kind": "coincident", "a": "Base/Slab z+", "b": "Lid/Slab z-"})).unwrap_err().to_string();
    assert!(e.contains("no part 'Lid'") || e.contains("has no face"), "{e}");
    assert_eq!(a, before);

    // A lid on the base: 0 degrees is closed (the faces meet), 40 opens it by that angle.
    let mut h = assembly();
    run(&mut h, "mate", json!({"kind": "fixed", "a": "Base"}));
    run(&mut h, "mate", json!({"kind": "angle", "a": "Base/Slab z+", "b": "Cap/Slab z-", "value": 0, "name": "Lid"}));
    let ev = evaluate_with(KernelChoice::Implicit, &h, 40, true).unwrap();
    assert!((face(&ev, "Cap/Slab z-").1 + DVec3::Z).length() < 1e-3);
    for angle in [40.0f64, 90.0, 10.0] {
        run(&mut h, "mate_set", json!({"mate": "Lid", "value": angle}));
        let ev = evaluate_with(KernelChoice::Implicit, &h, 40, true).unwrap();
        let (na, nb) = (face(&ev, "Base/Slab z+").1, face(&ev, "Cap/Slab z-").1);
        let between = na.dot(nb).clamp(-1.0, 1.0).acos().to_degrees();
        assert!((between - (180.0 - angle)).abs() < 0.1, "{angle}: {between}");
        assert!(ev.mates.iter().all(|m| m.error.is_none()), "{:?}", ev.mates);
    }
}

#[test]
fn mates_that_cannot_be_met_are_reported_and_follow_their_parts() {
    let mut a = assembly();
    run(&mut a, "mate", json!({"kind": "fixed", "a": "Base"}));
    run(&mut a, "mate", json!({"kind": "coincident", "a": "Base/Slab z+", "b": "Cap/Slab z-", "name": "Seat"}));
    // The same two faces 5 mm apart as well: refused, with the reason.
    let before = a.clone();
    let e = apply_op(&mut a, "mate", &json!({"kind": "distance", "a": "Base/Slab z+", "b": "Cap/Slab z-", "value": 5})).unwrap_err().to_string();
    assert!(e.contains("contradict"), "{e}");
    assert_eq!(a, before);
    // A face that goes later (its feature is renamed inside the part): the assembly still builds, the mate says why it is off.
    run(&mut a, "component_op", json!({"component": "Cap", "inner_op": "rename", "args": {"feature": "Slab", "to": "Lid"}}));
    let ev = evaluate_with(KernelChoice::Implicit, &a, 40, true).unwrap();
    let seat = ev.mates.iter().find(|m| m.name == "Seat").unwrap();
    assert!(seat.error.as_deref().is_some_and(|e| e.contains("no face 'Slab z-'")), "{seat:?}");
    // Suppressed, it is left out.
    run(&mut a, "mate_set", json!({"mate": "Seat", "suppressed": true}));
    let ev = evaluate_with(KernelChoice::Implicit, &a, 40, true).unwrap();
    assert!(ev.mates.iter().all(|m| m.error.is_none()));
    // Renaming a part takes its mates along; removing it removes them.
    run(&mut a, "component_rename", json!({"component": "Cap", "to": "Cover"}));
    assert_eq!(a.mates[1].b.component, "Cover");
    let back = Document::from_afxcad(&a.to_afxcad()).unwrap();
    assert_eq!(back, a);
    let gone = run(&mut a, "component_remove", json!({"component": "Cover"}));
    assert_eq!(gone["mates_removed"], 1);
    assert_eq!(a.mates.len(), 1);
    run(&mut a, "mate_remove", json!({"mate": "Fixed Base"}));
    assert!(a.mates.is_empty());
    assert!(apply_op(&mut a, "mate_remove", &json!({"mate": "Seat"})).unwrap_err().to_string().contains("no mate named"));
    // Against the assembly's own planes: the base's bottom on the Top plane, 7 above it.
    let mut w = assembly();
    run(&mut w, "mate", json!({"kind": "distance", "a": "XY", "b": "Cap/Slab z-", "value": 7}));
    let ev = evaluate_with(KernelChoice::Implicit, &w, 40, true).unwrap();
    let (c, n) = face(&ev, "Cap/Slab z-");
    assert!((c.z - 7.0).abs() < 2e-3 && (n + DVec3::Z).length() < 1e-3, "{c} {n}");
}
