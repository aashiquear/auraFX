//! Against testdata/crystal/reference.json (scripts/crystal_reference.py: scipy rotations,
//! brute-force disorientations over scipy's symmetry groups, independently reduced IPF colors).

use aurafx_crystal::*;
use serde_json::Value;

fn reference() -> Value {
    let p = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../testdata/crystal/reference.json");
    serde_json::from_str(&std::fs::read_to_string(p).unwrap()).unwrap()
}

fn v<const N: usize>(x: &Value) -> [f64; N] {
    std::array::from_fn(|k| x[k].as_f64().unwrap())
}

#[test]
fn conversions_match_the_reference() {
    let r = reference();
    for c in r["conversions"].as_array().unwrap() {
        let e = v::<3>(&c["euler"]);
        let q = euler_to_quat(e);
        let want = positive(v::<4>(&c["quat"]));
        assert!(q.iter().zip(want).all(|(a, b)| (a - b).abs() < 1e-12), "{e:?}: {q:?} vs {want:?}");
        let m = euler_to_matrix(e).concat();
        let wm = v::<9>(&c["matrix"]);
        assert!(m.iter().zip(wm).all(|(a, b)| (a - b).abs() < 1e-12));
    }
}

#[test]
fn disorientations_match_the_brute_force() {
    let r = reference();
    let mut n = 0;
    for (name, list) in r["disorientation"].as_object().unwrap() {
        let l = Laue::from_label(name).unwrap_or_else(|| panic!("{name}"));
        for d in list.as_array().unwrap() {
            let (q1, q2) = (euler_to_quat(v::<3>(&d["e1"])), euler_to_quat(v::<3>(&d["e2"])));
            let want = d["angle"].as_f64().unwrap();
            assert!((disorientation_angle(q1, q2, l) - want).abs() < 1e-9, "{name}");
            assert!((disorientation(q1, q2, l).0 - want).abs() < 1e-9, "{name}");
            n += 1;
        }
    }
    assert_eq!(n, 11 * 12);
}

#[test]
fn ipf_colors_match_the_reference() {
    let r = reference();
    for c in r["ipf"].as_array().unwrap() {
        let q = euler_to_quat(v::<3>(&c["euler"]));
        let d = v::<3>(&c["direction"]);
        for (key, l) in [("cubic", Laue::CubicHigh), ("hexagonal", Laue::HexagonalHigh)] {
            let got = ipf_color(q, l, d);
            let want = v::<3>(&c[key]).map(|x| (x * 255.0).round() as i32);
            assert!(got.iter().zip(want).all(|(a, b)| (*a as i32 - b).abs() <= 1), "{key} {got:?} vs {want:?}");
        }
    }
}
