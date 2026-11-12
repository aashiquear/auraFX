//! The CAD environment's session layer for sweeps and lofts, a part started from another
//! node's solid, a component edited on its own, and mates.
use aurafx_data::DataSet;
use aurafx_filters::ParamValue;
use aurafx_pipeline::Session;
use serde_json::json;

#[test]
fn a_geometry_source_or_a_closed_surface_becomes_the_first_body_of_a_part() {
    let mut s = Session::new();
    let g = s.add_source("geometry").unwrap();
    s.set_param(g, "commands", ParamValue::Str("brick x 30 y 20 z 10\ncylinder radius 4 height 10\nmove body 2 x 40".into()));
    s.update_blocking();
    assert!(s.nodes[&g].error.is_none(), "{:?}", s.nodes[&g].error);
    let m = s.cad_from_node(g).unwrap();
    assert!(s.is_cad_model(m) && m != g);
    assert!(!s.nodes[&g].visible, "the node it came from is hidden, not changed");
    assert_eq!(s.cad_from_node(m).unwrap(), m, "a CAD Model is itself");
    let doc = s.cad_document(m).unwrap();
    assert_eq!(doc.features.len(), 2, "one imported body per body of the geometry");
    assert!(doc.features.iter().all(|f| matches!(f.shape, aurafx_cad::Shape::Brep { .. })));
    assert!(s.trace.last().unwrap().contains("afx.cad_from("), "{:?}", s.trace.last());
    // Features cut from it and add to it.
    s.cad_enter(m).unwrap();
    let before = s.cad_env_evaluate(64).unwrap();
    let want = 30.0 * 20.0 * 10.0 + std::f64::consts::PI * 16.0 * 10.0;
    assert!((before.volume - want).abs() / want < 0.03, "{} vs {want}", before.volume);
    s.cad_env_edit("add", &json!({"kind": "hole", "name": "Bore", "diameter": 6, "depth": 30, "at": [0, 0, 6]})).unwrap();
    s.cad_env_edit("add", &json!({"kind": "box", "name": "Rib", "size": [4, 20, 6], "at": [0, 0, 8]})).unwrap();
    let after = s.cad_env_evaluate(64).unwrap();
    let change = -std::f64::consts::PI * 9.0 * 10.0 + 4.0 * 20.0 * 6.0 - 4.0 * 20.0 * 3.0 + std::f64::consts::PI * 9.0 * 0.0;
    assert!(((after.volume - before.volume) - change).abs() < 0.08 * want, "{} vs {change}", after.volume - before.volume);
    assert!(after.faces.iter().any(|f| f.name == "Bore side"), "{:?}", after.faces.iter().map(|f| &f.name).collect::<Vec<_>>());
    s.cad_apply(false).unwrap();
    s.update_blocking();
    assert!(matches!(s.nodes[&m].output.as_deref(), Some(DataSet::PolyData(_))) && s.nodes[&m].error.is_none(), "{:?}", s.nodes[&m].error);

    // Any closed surface will do; an open one is refused with what to do about it.
    let sph = s.add_source("sphere").unwrap();
    s.update_blocking();
    let ms = s.cad_from_node(sph).unwrap();
    assert_eq!(s.cad_document(ms).unwrap().features.len(), 1);
    let pl = s.add_source("plane").unwrap();
    s.update_blocking();
    let nodes = s.nodes.len();
    let e = s.cad_from_node(pl).unwrap_err().to_string();
    assert!(e.contains("not a closed surface"), "{e}");
    assert_eq!(s.nodes.len(), nodes, "nothing was added");
}

fn plate() -> serde_json::Value {
    let mut d = aurafx_cad::Document::new("Plate");
    aurafx_cad::edit::apply_op(&mut d, "add", &json!({"kind": "box", "name": "Slab", "size": [40, 40, 10], "at": [0, 0, 5]})).unwrap();
    serde_json::to_value(d).unwrap()
}

#[test]
fn a_component_is_edited_on_its_own_and_mates_place_the_parts() {
    let mut s = Session::new();
    let m = s.add_source("cad_model").unwrap();
    s.update_blocking();
    s.cad_enter(m).unwrap();
    s.cad_env_edit("new", &json!({"name": "Assembly"})).unwrap();
    s.cad_env_edit("component_add", &json!({"name": "Base", "document": plate()})).unwrap();
    s.cad_env_edit("component_add", &json!({"name": "Cap", "document": plate(), "at": [80, 10, 30], "rotate": [0, 0, 25]})).unwrap();
    let whole = s.cad_env_evaluate(48).unwrap();
    assert!(whole.faces.iter().any(|f| f.name == "Cap/Slab z+"));

    // Into the cap: the working document is its part, the faces are its own, in its own place.
    s.cad_env_focus(Some("Cap")).unwrap();
    {
        let e = s.cad_edit.as_ref().unwrap();
        assert_eq!(e.active().name, "Plate");
        assert_eq!(e.active().features.len(), 1);
    }
    let made = s.cad_env_edit("add", &json!({"kind": "hole", "name": "Bore", "diameter": 8, "depth": 30, "at": [0, 0, 10]})).unwrap();
    assert_eq!(made["name"], "Bore");
    let alone = s.cad_env_evaluate(48).unwrap();
    assert!(
        alone.faces.iter().any(|f| f.name == "Bore side") && alone.faces.iter().all(|f| !f.name.contains('/')),
        "{:?}",
        alone.faces.iter().map(|f| &f.name).collect::<Vec<_>>()
    );
    assert!(alone.bounds.0[0] > -21.0 && alone.bounds.1[0] < 21.0, "shown where it was modelled: {:?}", alone.bounds);
    {
        let e = s.cad_edit.as_ref().unwrap();
        assert_eq!(e.document.components[1].document.features.len(), 2, "the edit is in the assembly's document");
        assert_eq!(e.document.components[0].document.features.len(), 1);
        assert_eq!(e.selected, e.active().features.last().map(|f| f.id));
    }
    // A sketch on a face of the part, by the same operations as in a part of its own.
    s.cad_env_edit("sketch_extrude", &json!({"name": "Boss", "depth": 4, "sketch": {"face": "Slab z+", "items": [{"circle": {"center": [12, 12], "r": 3}}]}}))
        .unwrap();
    assert!((s.cad_env_evaluate(48).unwrap().bounds.1[2] - 14.0).abs() < 0.05);
    // Operations on the assembly wait until one is back in it.
    let e = s.cad_env_edit("component_add", &json!({"name": "X"})).unwrap_err().to_string();
    assert!(e.contains("go back to the assembly"), "{e}");
    // Undo is the assembly's: it takes the boss back, still inside the cap.
    s.cad_env_undo().unwrap();
    assert_eq!(s.cad_edit.as_ref().unwrap().active().features.len(), 2);
    s.cad_env_focus(None).unwrap();
    assert_eq!(s.cad_edit.as_ref().unwrap().active().name, "Assembly");
    assert!(s.cad_env_focus(Some("Nope")).unwrap_err().to_string().contains("the parts are: Base, Cap"));
    let whole = s.cad_env_evaluate(48).unwrap();
    assert!(whole.faces.iter().any(|f| f.name == "Cap/Bore side"));

    // Mates: the cap seated on the base, its hole's side 5 mm from the base's side face.
    s.cad_env_edit("mate", &json!({"kind": "fixed", "a": "Base"})).unwrap();
    s.cad_env_edit("mate", &json!({"kind": "coincident", "a": "Base/Slab z+", "b": "Cap/Slab z-"})).unwrap();
    s.cad_env_edit("mate", &json!({"kind": "distance", "a": "Base/Slab x+", "b": "Cap/Slab x-", "value": 5, "name": "Gap"})).unwrap();
    assert_eq!(s.cad_preview_kernel(m), aurafx_cad::KernelChoice::Auto, "mates are solved on exact faces");
    let sum = s.cad_env_evaluate(48).unwrap();
    assert_eq!(sum.mates.len(), 3);
    assert!(sum.mates.iter().all(|m| m.1.is_none()), "{:?}", sum.mates);
    assert_eq!(sum.mate_dof, Some(1), "the cap can still slide along the gap");
    let cap = sum.placements.iter().find(|p| p.0 == "Cap").unwrap();
    assert!((cap.1[0] - 45.0).abs() < 1e-3 && (cap.1[2] - 10.0).abs() < 1e-3, "{cap:?}");
    // Dragging the value is many edits and one step to undo.
    let steps = s.cad_edit.as_ref().unwrap().undo.len();
    for v in [6.0, 9.0, 14.0] {
        s.cad_env_edit("mate_set", &json!({"mate": "Gap", "value": v})).unwrap();
    }
    assert_eq!(s.cad_edit.as_ref().unwrap().undo.len(), steps + 1);
    let sum = s.cad_env_evaluate(48).unwrap();
    assert!((sum.placements.iter().find(|p| p.0 == "Cap").unwrap().1[0] - 54.0).abs() < 1e-3);
    s.cad_env_undo().unwrap();
    assert_eq!(s.cad_edit.as_ref().unwrap().document.mates[2].value, 5.0);
    // Applied, the node builds the mated assembly.
    s.cad_apply(false).unwrap();
    s.update_blocking();
    assert!(s.nodes[&m].error.is_none(), "{:?}", s.nodes[&m].error);
    let Some(DataSet::PolyData(pd)) = s.nodes[&m].output.as_deref() else { panic!() };
    let b = pd.points.bounds();
    assert!((b.max[0] - 65.0).abs() < 0.05 && (b.max[2] - 20.0).abs() < 0.05, "{b:?}");
}

#[test]
fn a_sweep_and_a_loft_are_made_in_the_environment() {
    let mut s = Session::new();
    let m = s.add_source("cad_model").unwrap();
    s.update_blocking();
    s.cad_enter(m).unwrap();
    s.cad_env_edit("new", &json!({"name": "Handle"})).unwrap();
    s.cad_env_edit("sketch_add", &json!({"name": "Profile", "sketch": {"plane": "YZ", "items": [{"circle": {"r": 3}}]}})).unwrap();
    s.cad_env_edit("sketch_add", &json!({"name": "Path", "sketch": {"plane": "XY", "items": [{"path": [[0, 0], [40, 0], [40, 25]]}]}})).unwrap();
    let made = s.cad_env_edit("sweep", &json!({"profile": "Profile", "path": "Path"})).unwrap();
    assert_eq!(made["shape"], "SketchSweep");
    let sum = s.cad_env_evaluate(64).unwrap();
    for f in ["Sweep start", "Sweep end", "Sweep side"] {
        assert!(sum.faces.iter().any(|x| x.name == f), "{f}: {:?}", sum.faces.iter().map(|f| &f.name).collect::<Vec<_>>());
    }
    s.cad_env_edit(
        "sketch_add",
        &json!({"name": "Foot", "sketch": {"plane": "XY", "offset": -10, "items": [{"rectangle": {"center": [0, 0], "w": 20, "h": 20}}]}}),
    )
    .unwrap();
    s.cad_env_edit("sketch_add", &json!({"name": "Neck", "sketch": {"plane": "XY", "offset": -3, "items": [{"circle": {"r": 4}}]}})).unwrap();
    s.cad_env_edit("loft", &json!({"sections": ["Foot", "Neck"]})).unwrap();
    let sum = s.cad_env_evaluate(64).unwrap();
    assert!(sum.faces.iter().any(|x| x.name == "Loft bottom"));
    let a = s.cad_apply(false).unwrap();
    assert_eq!(a.features, 2);
    s.update_blocking();
    assert!(s.nodes[&m].error.is_none(), "{:?}", s.nodes[&m].error);
}
