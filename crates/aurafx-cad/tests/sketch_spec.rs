//! A sketch written point by point with constraints and dimensions (what the Python sketch
//! object sends) solves, follows the part's dimensions and says what is wrong with it.
use aurafx_cad::edit::{apply_op, sketch_from_spec};
use aurafx_cad::kernel::{evaluate_with, KernelChoice};
use aurafx_cad::{Document, SolveStatus};
use serde_json::json;

#[test]
fn points_lines_constraints_and_dimensions_make_a_solved_outline() {
    let mut d = Document::new("p");
    apply_op(&mut d, "parameter", &json!({"name": "w", "expr": 40})).unwrap();
    // A rough quadrilateral, squared up by its constraints and sized by a dimension of the part.
    let spec = json!({
        "plane": "XY",
        "points": [[0, 0, true], [37, 2], [41, 18], [1, 22]],
        "entities": [{"line": [0, 1]}, {"line": [1, 2]}, {"line": [2, 3]}, {"line": [3, 0]}],
        "constraints": [
            {"horizontal": [0]}, {"vertical": [1]}, {"horizontal": [2]}, {"vertical": [3]},
            {"length": [0], "value": "w"}, {"distance": [1, 2], "value": 20},
        ],
    });
    let mut sk = sketch_from_spec(&d, &spec).unwrap();
    let r = sk.solve_with(&d.parameter_values().unwrap()).unwrap();
    assert_eq!(r.status, SolveStatus::FullyDefined, "{r:?}");
    assert!((sk.point(2) - glam::DVec2::new(40.0, 20.0)).length() < 1e-6, "{:?}", sk.point(2));
    apply_op(&mut d, "sketch_extrude", &json!({"name": "Plate", "depth": 5, "sketch": spec})).unwrap();
    let v = evaluate_with(KernelChoice::Auto, &d, 48, true).unwrap().volume();
    assert!((v - 4000.0).abs() < 1.0, "{v}");
    apply_op(&mut d, "parameter", &json!({"name": "w", "expr": 60})).unwrap();
    let v = evaluate_with(KernelChoice::Auto, &d, 48, true).unwrap().volume();
    assert!((v - 6000.0).abs() < 1.0, "{v}");
    // Circles, arcs and construction lines; a tangent line.
    let round = json!({
        "points": [[0, 0, true], [10, 0], [0, 10], [0, 0]],
        "entities": [{"circle": [0, 4]}, {"arc": [3, 1, 2]}, {"line": [1, 2], "construction": true}],
        "constraints": [{"radius": [0], "value": 6}, {"coincident": [0, 3]}],
    });
    let mut sk = sketch_from_spec(&d, &round).unwrap();
    assert!(sk.solve().converged);
    assert_eq!(sk.entities.len(), 3);
    assert!(sk.entities[2].construction());
    // Mistakes are named.
    let err = |spec: serde_json::Value| sketch_from_spec(&d, &spec).unwrap_err().to_string();
    assert!(err(json!({"points": [[0, 0]], "entities": [{"line": [0, 3]}]})).contains("names point 3"));
    assert!(err(json!({"points": [[0, 0], [1, 0]], "entities": [{"line": [0, 1]}], "constraints": [{"length": [0]}]})).contains("needs a \"value\""));
    assert!(err(json!({"points": [[0, 0], [1, 0]], "entities": [{"line": [0, 1]}], "constraints": [{"wobbly": [0]}]})).contains("unknown constraint"));
    assert!(err(json!({"points": [[0, 0], [1, 0]], "entities": [{"line": [0, 1]}], "constraints": [{"horizontal": [4]}]})).contains("names entity 4"));
}
