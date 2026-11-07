//! The CAD Model source and the `cad` object from Python. One test function: the embedded
//! interpreter is process-wide.
use aurafx_pipeline::Session;
use aurafx_python::PythonRuntime;

fn run(rt: &PythonRuntime, code: &str) -> String {
    let out = rt.run(code);
    assert!(out.error.is_none(), "{:?}\n{code}", out.error);
    out.stdout.trim().to_string()
}

#[test]
fn cad_model_from_python() {
    let session = Session::shared();
    let rt = PythonRuntime::new(session.clone()).unwrap();

    // A plate, a hole, a parameter: the node follows the parameter without any editing.
    let o = run(
        &rt,
        r#"
m = afx.cad_model()
m.cad.parameter("width", "50 mm")
m.cad.hole(diameter=6, depth=20, name="Bore", at=(0, 0, 12))
v = m.cad.volume(resolution=64)
print(v)
print(sorted(f["name"] for f in m.cad.faces() if f["name"].startswith("Bore")))
info = m.update()
print("polys" in info or "cells" in info or len(info) > 0)
m.set(width=80)
print(m.cad.volume(resolution=64))
print(m.cad.parameters()["width"])
"#,
    );
    let lines: Vec<&str> = o.lines().collect();
    // 50 x 30 x 8 plate minus a 6 mm hole through 8 mm.
    let v1 = 50.0 * 30.0 * 8.0 - std::f64::consts::PI * 9.0 * 8.0;
    let close = |s: &str, e: f64| (s.parse::<f64>().unwrap() - e).abs() / e < 0.01;
    assert!(close(lines[0], v1), "{o}");
    assert_eq!(lines[1], "['Bore side']", "{o}");
    assert_eq!(lines[2], "True");
    let v2 = 80.0 * 30.0 * 8.0 - std::f64::consts::PI * 9.0 * 8.0;
    assert!(close(lines[3], v2), "{o}");
    assert_eq!(lines[4], "80.0", "the override is what the node uses: {o}");

    // Mistakes read as sentences.
    let out = rt.run("m.set(wdth=3)");
    let e = out.error.unwrap();
    assert!(e.contains("no parameter 'wdth'") && e.contains("width"), "{e}");
    let out = rt.run("m.cad.box(size=(1, 2))");
    assert!(out.error.unwrap().contains("takes 3 values"));

    // Templates, files, undo and the trace.
    let o = run(
        &rt,
        r#"
t = afx.cad_model(template="flange")
print(len(t.cad.features()) >= 3)
t.cad.save("/tmp/afx_cad_py_test.afxcad")
u = afx.cad_model("/tmp/afx_cad_py_test.afxcad")
print(len(u.cad.features()) == len(t.cad.features()))
n = len(u.cad.features())
u.cad.sphere(radius=3, name="Ball", at=(0, 0, 50))
print(len(u.cad.features()) == n + 1)
afx.undo()
print(len(u.cad.features()) == n)
"#,
    );
    assert_eq!(o, "True\nTrue\nTrue\nTrue", "{o}");
    let trace = session.lock().trace.join("\n");
    assert!(trace.contains(".cad.edit('add'"), "{trace}");
    std::fs::remove_file("/tmp/afx_cad_py_test.afxcad").ok();
}
