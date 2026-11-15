//! Sketch geometry taken from the part, from Python: the edges around a face, the edge between
//! two faces. One test function: the embedded interpreter is process-wide.
use aurafx_pipeline::Session;
use aurafx_python::PythonRuntime;

#[test]
fn a_sketch_takes_edges_of_the_part_from_python() {
    let session = Session::shared();
    let rt = PythonRuntime::new(session.clone()).unwrap();
    let out = rt.run(
        r#"
m = afx.cad_model(name="Block")
m.cad.clear("Block")
m.cad.box(size=(20, 20, 20), name="Block")
m.cad.hole(diameter=6, depth=20, name="Bore", at=(0, 0, 10))
v0 = m.cad.volume()
# The rim alone: the edge between the top and the wall of the bore.
rim = m.cad.sketch(plane="XY", offset=10).project(edge=("Block z+", "Bore side"))
print(rim.solve()["outlines"])
# The outline of the top and the rim of the bore, raised 5 mm: a collar that keeps the bore open.
s = m.cad.sketch(plane="XY", offset=10).project(face="Block z+")
r = s.solve()
print(r["status"], r["outlines"])
m.cad.extrude(s, depth=5, name="Collar")
print(round(m.cad.volume() - v0, 1))
try:
    m.cad.extrude(m.cad.sketch(plane="XY", offset=10).project(face="No such face"), depth=1, name="Bad")
except Exception as e:
    print("refused:", "no such edge" in str(e))
"#,
    );
    assert!(out.error.is_none(), "{:?}", out.error);
    let l: Vec<&str> = out.stdout.trim().lines().collect();
    assert_eq!(l[0], "1", "{}", out.stdout);
    assert_eq!(l[1], "fully defined 2", "{}", out.stdout);
    let added: f64 = l[2].parse().unwrap();
    assert!((added - (400.0 - std::f64::consts::PI * 9.0) * 5.0).abs() < 2.0, "{}", out.stdout);
    assert_eq!(l[3], "refused: True", "{}", out.stdout);
}
