//! Volumes and surface areas of primitives, booleans and profile solids against closed forms;
//! face names; command errors.

use aurafx_mesh::{measure, Model};
use std::f64::consts::PI;

fn body(cmds: &str) -> Model {
    Model::from_commands(cmds, None).unwrap_or_else(|e| panic!("{cmds}: {e:#}"))
}

/// Volume and area of the last body, on cells 1/100 of its size.
fn va(cmds: &str) -> (f64, f64) {
    let m = body(cmds);
    let b = m.bodies.values().last().unwrap();
    let (lo, hi) = b.shape.bounds();
    let size = (0..3).map(|k| hi[k] - lo[k]).fold(0.0, f64::max);
    measure(&b.shape, size / 100.0)
}

fn close(what: &str, got: f64, want: f64, rel: f64) {
    assert!(((got - want) / want).abs() < rel, "{what}: {got} vs {want}");
}

fn gamma(x: f64) -> f64 {
    // Lanczos approximation (g = 7).
    const C: [f64; 9] = [
        0.999_999_999_999_809_9,
        676.5203681218851,
        -1259.1392167224028,
        771.323_428_777_653_1,
        -176.615_029_162_140_6,
        12.507343278686905,
        -0.13857109526572012,
        9.984_369_578_019_572e-6,
        1.5056327351493116e-7,
    ];
    if x < 0.5 {
        return PI / ((PI * x).sin() * gamma(1.0 - x));
    }
    let x = x - 1.0;
    let mut a = C[0];
    let t = x + 7.5;
    for (i, c) in C.iter().enumerate().skip(1) {
        a += c / (x + i as f64);
    }
    (2.0 * PI).sqrt() * t.powf(x + 0.5) * (-t).exp() * a
}
fn beta(a: f64, b: f64) -> f64 {
    gamma(a) * gamma(b) / gamma(a + b)
}

#[test]
fn primitives_match_closed_forms() {
    let (v, a) = va("sphere radius 1");
    close("sphere V", v, 4.0 / 3.0 * PI, 0.005);
    close("sphere A", a, 4.0 * PI, 0.005);
    let (v, a) = va("brick x 2 y 1 z 0.5");
    close("brick V", v, 1.0, 0.005);
    close("brick A", a, 7.0, 0.01);
    let (v, a) = va("cylinder radius 0.5 height 2");
    close("cylinder V", v, PI * 0.25 * 2.0, 0.005);
    close("cylinder A", a, 2.0 * PI * 0.5 * 2.0 + 2.0 * PI * 0.25, 0.01);
    let (v, a) = va("cone radius 1 top 0.5 height 1");
    let s = (1.0f64 + 0.25).sqrt();
    close("cone V", v, PI / 3.0 * (1.0 + 0.5 + 0.25), 0.005);
    close("cone A", a, PI * 1.5 * s + PI + PI * 0.25, 0.01);
    let (v, a) = va("torus major 2 minor 0.5");
    close("torus V", v, 2.0 * PI * PI * 2.0 * 0.25, 0.005);
    close("torus A", a, 4.0 * PI * PI * 2.0 * 0.5, 0.005);
    let (v, _) = va("prism sides 6 radius 1 height 2");
    close("prism V", v, 3.0 * 3f64.sqrt() / 2.0 * 2.0, 0.005);
    let (v, _) = va("superquadric x 1 y 0.5 z 0.75");
    close("ellipsoid V", v, 4.0 / 3.0 * PI * 0.375, 0.005);
    let (e1, e2) = (0.5, 0.7);
    let (v, _) = va(&format!("superquadric x 1 y 0.8 z 0.6 e1 {e1} e2 {e2}"));
    close("superquadric V", v, 2.0 * 0.48 * e1 * e2 * beta(e1 / 2.0 + 1.0, e1) * beta(e2 / 2.0, e2 / 2.0), 0.01);
    // Harmonic shape: V = 1/3 of the integral of r^3 over the sphere.
    let (v, _) = va("harmonic radius 1 coefficient 2 0 0.3 coefficient 3 2 0.15");
    let mut want = 0.0;
    let (nt, np) = (400, 800);
    for i in 0..nt {
        let th = PI * (i as f64 + 0.5) / nt as f64;
        for j in 0..np {
            let ph = 2.0 * PI * (j as f64 + 0.5) / np as f64;
            let d = [th.sin() * ph.cos(), th.sin() * ph.sin(), th.cos()];
            let r = aurafx_mesh::shape::harmonic_radius(1.0, &[(2, 0, 0.3), (3, 2, 0.15)], d);
            want += r.powi(3) / 3.0 * th.sin() * (PI / nt as f64) * (2.0 * PI / np as f64);
        }
    }
    close("harmonic V", v, want, 0.01);
}

#[test]
fn booleans_and_transforms() {
    let (v, a) = va("brick 2; cylinder radius 0.5 height 4; subtract body 2 from body 1");
    close("brick with hole V", v, 8.0 - PI * 0.25 * 2.0, 0.005);
    close("brick with hole A", a, 24.0 - 2.0 * PI * 0.25 + 2.0 * PI * 0.5 * 2.0, 0.01);
    let (v, _) = va("sphere radius 1; sphere radius 1; move body 2 x 1; unite body 1 2");
    close("two spheres V", v, 8.0 / 3.0 * PI - PI * 5.0 / 12.0, 0.005);
    let (v, _) = va("sphere radius 1; sphere radius 1; move body 2 x 1; intersect body 1 2");
    close("lens V", v, PI * 5.0 / 12.0, 0.01);
    let (v, _) = va("brick x 2 y 1 z 1; rotate body 1 angle 37 about 1 1 0; scale body 1 2");
    close("rotated scaled brick V", v, 16.0, 0.005);
    let (v, _) = va("brick 1; scale body 1 x 2 y 1 z 3");
    close("stretched cube V", v, 6.0, 0.005);
    // Keep the tool.
    let m = body("brick 2; sphere radius 0.5; subtract body 2 from body 1 keep");
    assert_eq!(m.bodies.len(), 2);
}

#[test]
fn outlines_extruded_revolved_swept() {
    let (v, a) = va("outline 1 rectangle x 2 y 1; extrude outline 1 height 3");
    close("extruded rectangle V", v, 6.0, 0.005);
    close("extruded rectangle A", a, 4.0 + 18.0, 0.01);
    let (v, _) = va("outline 1 rectangle x 2 y 2; outline 1 fillet radius 0.5; extrude outline 1 height 1");
    close("rounded square V", v, 4.0 - (4.0 - PI) * 0.25, 0.005);
    let (v, _) = va("outline 1 rectangle x 1 y 1 center 1.5 0.5; revolve outline 1");
    close("ring V", v, PI * (4.0 - 1.0), 0.005);
    let (v, _) = va("outline 1 rectangle x 1 y 1 center 1.5 0.5; revolve outline 1 angle 90");
    close("quarter ring V", v, PI * 3.0 / 4.0, 0.01);
    let (v, _) = va("outline 1 polyline 0 0 1 0 0 1; revolve outline 1");
    close("revolved triangle (cone) V", v, PI / 3.0, 0.01);
    let (v, _) = va("outline 1 circle radius 0.2; sweep outline 1 path 0 0 0 2 0 0");
    close("swept circle V", v, PI * 0.04 * 2.0, 0.01);
    let (v, _) = va("outline 1 circle radius 0.2; sweep outline 1 path 0 0 0 2 0 0 2 2 0");
    // Two cylinders of length 2 and a quarter of a sphere-like joint at the bend (between the
    // mitred and the rounded joint): within 2 % of two plain cylinders.
    close("bent sweep V", v, PI * 0.04 * 4.0, 0.03);
    // An outline built from pieces: a line and an arc (a "D" shape).
    let (v, _) = va("outline 1 polyline 0 -1 0 1; outline 1 add arc center 0 0 radius 1 start 90 end -90; extrude outline 1 height 1");
    close("D shape V", v, PI / 2.0, 0.01);
}

#[test]
fn faces_are_named() {
    let m = body("brick 2; cylinder radius 0.5 height 4; subtract body 2 from body 1; face 6 name 'top'");
    let b = &m.bodies[&1];
    let name = |p| m.faces[b.shape.eval(p).1 as usize].clone();
    assert_eq!(name([0.2, 0.7, 1.0]), "top");
    assert_eq!(name([1.0, 0.3, 0.2]), "brick 1 x+");
    assert_eq!(name([0.5, 0.0, 0.3]), "cylinder 2 side");
    let d = m.describe();
    assert_eq!(d.len(), 1);
    assert_eq!(d[0].2, vec!["subtract", "  brick 2 x 2 x 2", "  cylinder radius 0.5 height 4"]);
    // The cylinder's caps lie outside the brick but are still listed as faces of the body.
    assert!(d[0].3.iter().any(|(_, n)| n == "cylinder 2 side"));
    let m = body("outline 1 rectangle x 2 y 1; extrude outline 1 height 3");
    let names: Vec<&str> = m.faces.iter().skip(1).map(|s| s.as_str()).collect();
    assert_eq!(names, vec!["extrusion 1 edge 1", "extrusion 1 edge 2", "extrusion 1 edge 3", "extrusion 1 edge 4", "extrusion 1 bottom", "extrusion 1 top"]);
}

#[test]
fn command_errors_say_where() {
    let e = Model::from_commands("brick 1\nsphere 2", None).err().unwrap();
    assert!(format!("{e:#}").contains("line 2"), "{e:#}");
    assert!(Model::from_commands("subtract body 3 from body 1", None).is_err());
    assert!(Model::from_commands("bogus", None).is_err());
    let m = body("brick 1 # a comment\nsphere radius 1; body 2 name 'ball'");
    assert_eq!(m.journal, vec!["brick 1", "sphere radius 1", "body 2 name 'ball'"]);
    assert_eq!(m.bodies[&2].name, "ball");
}

/// Hexahedron quality metrics against ParaView 6.1.1's Mesh Quality (Verdict) on 24 hexahedra
/// (testdata/mesh/hex_quality_reference.json, scripts/hex_quality_reference.py).
#[test]
fn hex_quality_matches_reference() {
    use aurafx_mesh::quality::{hex_metric, HexMetric};
    let p = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../testdata/mesh/hex_quality_reference.json");
    let r: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(p).unwrap()).unwrap();
    let hexes: Vec<[[f64; 3]; 8]> =
        r["hexes"].as_array().unwrap().iter().map(|h| std::array::from_fn(|i| std::array::from_fn(|k| h[i][k].as_f64().unwrap() as f32 as f64))).collect();
    let vols: Vec<f64> = hexes.iter().map(|h| hex_metric(HexMetric::Volume, h, 0.0)).collect();
    let avg = vols.iter().sum::<f64>() / vols.len() as f64;
    let names = [
        ("Diagonal", HexMetric::Diagonal),
        ("Dimension", HexMetric::Dimension),
        ("Distortion", HexMetric::Distortion),
        ("Edge Ratio", HexMetric::EdgeRatio),
        ("Jacobian", HexMetric::Jacobian),
        ("Maximum Aspect Frobenius", HexMetric::MaxAspectFrobenius),
        ("Maximum Edge Ratio", HexMetric::MaxEdgeRatio),
        ("Mean Aspect Frobenius", HexMetric::MeanAspectFrobenius),
        ("Oddy", HexMetric::Oddy),
        ("Relative Size Squared", HexMetric::RelativeSizeSquared),
        ("Scaled Jacobian", HexMetric::ScaledJacobian),
        ("Shape", HexMetric::Shape),
        ("Shape and Size", HexMetric::ShapeAndSize),
        ("Shear", HexMetric::Shear),
        ("Shear and Size", HexMetric::ShearAndSize),
        ("Skew", HexMetric::Skew),
        ("Stretch", HexMetric::Stretch),
        ("Taper", HexMetric::Taper),
        ("Volume", HexMetric::Volume),
    ];
    let mut bad = Vec::new();
    for (name, m) in names {
        let want = r["measures"][name].as_array().unwrap_or_else(|| panic!("{name}"));
        for (i, h) in hexes.iter().enumerate() {
            let w = want[i].as_f64().unwrap();
            let g = hex_metric(m, h, avg);
            // ParaView keeps points and results in 32-bit floats.
            if (g - w).abs() > 2e-6 * w.abs().max(1.0) {
                bad.push(format!("{name} #{i}: {g} vs {w}"));
            }
        }
    }
    assert!(bad.is_empty(), "{} differ:\n{}", bad.len(), bad.join("\n"));
}

/// Tetrahedron quality against ParaView's Mesh Quality (Verdict) on 24 tetrahedra
/// (testdata/mesh/tet_quality_reference.json from scripts/tet_quality_reference.py).
#[test]
fn tet_quality_matches_reference() {
    use aurafx_mesh::quality::{tet_metric, TetMetric};
    let p = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../testdata/mesh/tet_quality_reference.json");
    let r: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(p).unwrap()).unwrap();
    let tets: Vec<[[f64; 3]; 4]> =
        r["tets"].as_array().unwrap().iter().map(|h| std::array::from_fn(|i| std::array::from_fn(|k| h[i][k].as_f64().unwrap() as f32 as f64))).collect();
    let vols: Vec<f64> = tets.iter().map(|t| tet_metric(TetMetric::Volume, t, 0.0)).collect();
    let avg = vols.iter().sum::<f64>() / vols.len() as f64;
    let names = [
        ("Aspect Frobenius", TetMetric::AspectFrobenius),
        ("Aspect Gamma", TetMetric::AspectGamma),
        ("Aspect Ratio", TetMetric::AspectRatio),
        ("Collapse Ratio", TetMetric::CollapseRatio),
        ("Condition", TetMetric::Condition),
        ("Distortion", TetMetric::Distortion),
        ("Edge Ratio", TetMetric::EdgeRatio),
        ("Equiangle Skew", TetMetric::EquiangleSkew),
        ("Inradius", TetMetric::Inradius),
        ("Jacobian", TetMetric::Jacobian),
        ("Mean Ratio", TetMetric::MeanRatio),
        ("Minimum Dihedral Angle", TetMetric::MinimumDihedralAngle),
        ("Normalized Inradius", TetMetric::NormalizedInradius),
        ("Radius Ratio", TetMetric::RadiusRatio),
        ("Relative Size Squared", TetMetric::RelativeSizeSquared),
        ("Scaled Jacobian", TetMetric::ScaledJacobian),
        ("Shape", TetMetric::Shape),
        ("Shape and Size", TetMetric::ShapeAndSize),
        ("Squish Index", TetMetric::SquishIndex),
        ("Volume", TetMetric::Volume),
    ];
    let mut bad = Vec::new();
    for (name, m) in names {
        let want = r["measures"][name].as_array().unwrap_or_else(|| panic!("{name}"));
        for (i, t) in tets.iter().enumerate() {
            let w = want[i].as_f64().unwrap();
            let g = tet_metric(m, t, avg);
            if (g - w).abs() > 2e-6 * w.abs().max(1.0) {
                bad.push(format!("{name} #{i}: {g} vs {w}"));
            }
        }
    }
    assert!(bad.is_empty(), "{} differ:\n{}", bad.len(), bad.join("\n"));
}
