//! Sweeps and lofts drawn as sketches kept with the part: built by both kernels, their faces
//! named, their mistakes refused with the document left as it was.
use aurafx_cad::edit::apply_op;
use aurafx_cad::kernel::{evaluate_with, KernelChoice};
use aurafx_cad::{Document, Shape};
use serde_json::json;
use std::f64::consts::PI;

fn run(d: &mut Document, op: &str, args: serde_json::Value) -> serde_json::Value {
    apply_op(d, op, &args).unwrap_or_else(|e| panic!("{op}: {e}"))
}

fn names(ev: &aurafx_cad::Evaluated) -> Vec<String> {
    ev.faces().map(|(_, f)| f.name.clone()).collect()
}

#[test]
fn a_loft_through_two_sketches_is_a_frustum_on_both_kernels() {
    let mut d = Document::new("loft");
    run(&mut d, "sketch_add", json!({"name": "Base", "sketch": {"plane": "XY", "items": [{"rectangle": {"center": [0, 0], "w": 20, "h": 20}}]}}));
    run(&mut d, "sketch_add", json!({"name": "Top", "sketch": {"plane": "XY", "offset": 10, "items": [{"rectangle": {"center": [0, 0], "w": 10, "h": 10}}]}}));
    let made = run(&mut d, "loft", json!({"sections": ["Base", "Top"]}));
    assert_eq!(made["name"], "Loft");
    assert!(matches!(d.features[0].shape, Shape::SketchLoft { .. }));
    let frustum = 10.0 / 3.0 * (400.0 + 100.0 + 200.0);
    for choice in [KernelChoice::Implicit, KernelChoice::Brep, KernelChoice::Auto] {
        let ev = evaluate_with(choice, &d, 72, true).unwrap_or_else(|e| panic!("{choice:?}: {e}"));
        assert!((ev.volume() - frustum).abs() / frustum < 0.03, "{choice:?}: {} vs {frustum}", ev.volume());
        let n = names(&ev);
        for want in ["Loft bottom", "Loft top", "Loft side"] {
            assert!(n.iter().any(|x| x == want), "{choice:?}: {n:?}");
        }
        let top = ev.face_by_name("Loft top").unwrap();
        assert!((top.centroid[2] - 10.0).abs() < 0.05 && (top.area - 100.0).abs() < 4.0, "{choice:?}: {top:?}");
    }
    // The exact kernel builds it: nothing is approximate.
    let auto = evaluate_with(KernelChoice::Auto, &d, 48, true).unwrap();
    assert_eq!(auto.kernel, "brep");
    assert!(auto.approximate.is_empty(), "{:?}", auto.approximate);
    // The sketch follows a dimension, and so does the loft.
    run(&mut d, "parameter", json!({"name": "top", "expr": 10}));
    run(
        &mut d,
        "sketch_set",
        json!({"name": "Top", "sketch": {"plane": "XY", "offset": 10, "items": [{"rectangle": {"center": [0, 0], "w": "top", "h": "top"}}]}}),
    );
    run(&mut d, "parameter", json!({"name": "top", "expr": 20}));
    let v = evaluate_with(KernelChoice::Auto, &d, 48, true).unwrap().volume();
    assert!((v - 4000.0).abs() < 40.0, "{v}");
    // A loft on the Front plane lies along -y (that plane's normal).
    let mut f = Document::new("front");
    run(&mut f, "sketch_add", json!({"name": "A", "sketch": {"plane": "XZ", "items": [{"circle": {"r": 6}}]}}));
    run(&mut f, "sketch_add", json!({"name": "B", "sketch": {"plane": "XZ", "offset": 12, "items": [{"circle": {"r": 3}}]}}));
    run(&mut f, "loft", json!({"sections": ["A", "B"], "name": "Cone"}));
    let ev = evaluate_with(KernelChoice::Implicit, &f, 64, true).unwrap();
    let b = &ev.bodies[0].bounds;
    assert!((b.0[1] + 12.0).abs() < 0.2 && b.1[1].abs() < 0.2, "{b:?}");
}

#[test]
fn a_sweep_follows_its_path_sketch_and_names_its_ends() {
    let mut d = Document::new("sweep");
    // A round profile on the Right plane (it looks along x), a path on the Top plane that leaves along x.
    run(&mut d, "sketch_add", json!({"name": "Profile", "sketch": {"plane": "YZ", "items": [{"circle": {"r": 2}}]}}));
    run(&mut d, "sketch_add", json!({"name": "Path", "sketch": {"plane": "XY", "items": [{"path": [[0, 0], [30, 0], [30, 30]]}]}}));
    run(&mut d, "sweep", json!({"profile": "Profile", "path": "Path"}));
    let want = PI * 4.0 * 60.0;
    let imp = evaluate_with(KernelChoice::Implicit, &d, 96, true).unwrap();
    assert!((imp.volume() - want).abs() / want < 0.05, "{} vs {want}", imp.volume());
    let (start, end) = (imp.face_by_name("Sweep start").unwrap(), imp.face_by_name("Sweep end").unwrap());
    assert!(start.centroid[0].abs() < 0.2 && (end.centroid[1] - 30.0).abs() < 0.2 && (end.centroid[0] - 30.0).abs() < 0.3, "{start:?} {end:?}");
    assert!(imp.face_by_name("Sweep side").is_some());
    // Auto: the exact kernel when it builds the sweep, else the other one with the reason noted.
    let auto = evaluate_with(KernelChoice::Auto, &d, 64, true).unwrap();
    assert!((auto.volume() - want).abs() / want < 0.06, "{} ({}) vs {want}", auto.volume(), auto.kernel);
    eprintln!("sweep on Auto: {} kernel, {} mm3 (want {want}), notes {:?}", auto.kernel, auto.volume(), auto.approximate);
    assert!(auto.kernel == "brep" || auto.approximate.iter().any(|a| a.contains("implicit kernel")), "{:?}", auto.approximate);
    assert!(names(&auto).iter().any(|n| n == "Sweep start") && names(&auto).iter().any(|n| n == "Sweep end"), "{:?}", names(&auto));
    // A longer path makes a longer sweep: the feature follows its sketch.
    run(&mut d, "sketch_set", json!({"name": "Path", "sketch": {"plane": "XY", "items": [{"path": [[0, 0], [50, 0]]}]}}));
    let v = evaluate_with(KernelChoice::Implicit, &d, 96, true).unwrap().volume();
    assert!((v - PI * 4.0 * 50.0).abs() / (PI * 200.0) < 0.05, "{v}");
    // A sweep cut from a block removes the channel.
    let mut c = Document::new("cut");
    run(&mut c, "add", json!({"kind": "box", "name": "Block", "size": [40, 20, 10], "at": [20, 0, 0]}));
    run(
        &mut c,
        "sweep",
        json!({"name": "Channel", "op": "cut",
        "profile": {"plane": "YZ", "items": [{"circle": {"r": 2}}]},
        "path": {"plane": "XY", "items": [{"path": [[-5, 0], [45, 0]]}]}}),
    );
    assert_eq!(c.sketches.len(), 2, "sketches given in place are kept with the part");
    assert_eq!(c.sketches[0].name, "Channel profile");
    let v = evaluate_with(KernelChoice::Implicit, &c, 96, true).unwrap().volume();
    let left = 8000.0 - PI * 4.0 * 40.0;
    assert!((v - left).abs() / left < 0.02, "{v} vs {left}");
}

#[test]
fn mistakes_in_sweeps_and_lofts_are_refused_and_change_nothing() {
    let mut d = Document::new("x");
    run(&mut d, "sketch_add", json!({"name": "A", "sketch": {"plane": "XY", "items": [{"circle": {"r": 5}}]}}));
    run(&mut d, "sketch_add", json!({"name": "B", "sketch": {"plane": "XY", "items": [{"circle": {"r": 3}}]}}));
    run(&mut d, "sketch_add", json!({"name": "Tilted", "sketch": {"plane": "XZ", "items": [{"circle": {"r": 3}}]}}));
    run(&mut d, "sketch_add", json!({"name": "Line", "sketch": {"plane": "XZ", "items": [{"path": [[0, 0], [0, 20]]}]}}));
    let before = d.clone();
    let err = |d: &mut Document, op: &str, a: serde_json::Value| apply_op(d, op, &a).unwrap_err().to_string();
    assert!(err(&mut d, "loft", json!({"sections": ["A", "B"]})).contains("same plane"));
    assert!(err(&mut d, "loft", json!({"sections": ["A", "Tilted"]})).contains("not parallel"));
    assert!(err(&mut d, "loft", json!({"sections": ["A"]})).contains("two or more"));
    assert!(err(&mut d, "loft", json!({"sections": ["A", "Nope"]})).contains("no sketch named 'Nope'"));
    assert!(err(&mut d, "sweep", json!({"profile": "A", "path": "B"})).contains("no open path"));
    assert!(err(&mut d, "sweep", json!({"profile": "Line", "path": "Line"})).contains("two sketches"));
    assert!(err(&mut d, "sweep", json!({"profile": "Tilted", "path": "Line"})).contains("lies along"));
    assert!(err(&mut d, "sketch_add", json!({"name": "A", "sketch": {"items": [{"circle": {"r": 1}}]}})).contains("already has a sketch"));
    assert_eq!(d, before, "a refused operation leaves the document as it was");
    // A sketch in use cannot be removed; renaming it takes the feature along.
    run(&mut d, "sweep", json!({"profile": "A", "path": "Line"}));
    assert!(err(&mut d, "sketch_remove", json!({"name": "A"})).contains("used by Sweep"));
    run(&mut d, "sketch_rename", json!({"name": "A", "to": "Round"}));
    assert!(matches!(&d.features[0].shape, Shape::SketchSweep { profile, .. } if profile == "Round"));
    assert!(evaluate_with(KernelChoice::Implicit, &d, 48, true).is_ok());
    // Another profile for the same sweep; one that does not fit is refused.
    run(&mut d, "sweep_set", json!({"feature": "Sweep", "profile": "B"}));
    assert!(err(&mut d, "sweep_set", json!({"feature": "Sweep", "path": "Round"})).contains("no open path"));
    assert!(matches!(&d.features[0].shape, Shape::SketchSweep { profile, path, .. } if profile == "B" && path == "Line"));
    run(&mut d, "sketch_remove", json!({"name": "Tilted"}));
    // The file round trip keeps the sketches and the feature.
    let back = Document::from_afxcad(&d.to_afxcad()).unwrap();
    assert_eq!(back, d);
}
