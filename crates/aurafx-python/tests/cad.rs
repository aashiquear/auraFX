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
    // The environment: enter, add a blind hole (two faces more), apply.
    let o = run(
        &rt,
        r#"
p = afx.cad_model()
p.update()
n0 = len(p.cad.faces())
e = p.edit_cad()
print(e.editing())
e.hole(diameter=6, depth=3, name="Pocket", at=(0, 0, 8))
print(len(e.faces()) - n0)
e.rollback(1)
print(len(e.faces()) == n0)
e.rollback(None)
e.undo(); e.redo()
print(e.state()["changed"], e.state()["undo"] >= 1)
r = e.apply()
print(r["features"], p.cad.editing())
p.update()
print(len(p.cad.faces()) - n0)
e2 = p.edit_cad(); e2.sphere(radius=3, at=(0, 0, 30)); e2.discard()
print(len(p.cad.features()))
"#,
    );
    assert_eq!(o, "True\n2\nTrue\nTrue True\n2 False\n2\n2", "{o}");
    let out = rt.run("q = afx.cad_model(); q.edit_cad(); other = afx.cad_model(); other.edit_cad()");
    assert!(out.error.unwrap().contains("already open"));
    run(&rt, "q.cad.discard()");
    // The downstream check from Python: rename the hole, see the conflict, apply the default fix.
    let o = run(
        &rt,
        r#"
c = afx.cad_model()
c.cad.hole(diameter=8, depth=20, name="Bore", at=(0, 0, 12))
w = c.extract_set(set="Bore side")
e = c.edit_cad()
e.rename("Bore", "Pin hole")
chk = afx.cad_check(c)
print(len(chk["conflicts"]), chk["conflicts"][0]["kind"], chk["conflicts"][0]["fix"]["value"])
print("Bore side" in chk["diff"] and "Pin hole side" in chk["diff"], "JSON array" in chk["prompt"])
r = e.apply()
print(r["fixes"], w.params["set"])
e = c.edit_cad()
e.remove("Pin hole")
r2 = e.apply(fix="none", run="later")
print(r2["fixes"], r2["held"], w.params["set"])
c.cad.run_held()
"#,
    );
    assert_eq!(o, "1 renamed to 'Pin hole side' Pin hole side\nTrue True\n1 Pin hole side\n0 1 Pin hole side", "{o}");
    // Kernels and files from Python: a STEP file opens as a CAD Model, can be cut into and written back.
    let o = run(
        &rt,
        r#"
import os, tempfile
d = tempfile.mkdtemp()
src = afx.cad_model(name="Source")
src.cad.hole(diameter=8, depth=20, name="Bore", at=(0, 0, 12))
r = src.cad.save(os.path.join(d, "src.step"))
print(r["exact"], r["bytes"] > 1000)
m = afx.load(os.path.join(d, "src.step"))
print(m.params["kernel"], len(m.cad.features()))
m.cad.hole(diameter=4, depth=20, name="Pin", at=(12, 0, 12))
print(round(m.cad.volume()))
print(m.update() != "")
m.set(kernel="Implicit")
print(round(m.cad.volume() / 100) == round(m.cad.volume() / 100))
r2 = m.cad.save(os.path.join(d, "out.3mf"))
print(r2["exact"])
"#,
    );
    let l: Vec<&str> = o.lines().collect();
    assert_eq!(l[0], "True True", "{o}");
    assert_eq!(l[1], "Auto 1", "{o}");
    let expect = 40.0 * 30.0 * 8.0 - std::f64::consts::PI * 16.0 * 8.0 - std::f64::consts::PI * 4.0 * 8.0;
    assert!((l[2].parse::<f64>().unwrap() - expect).abs() / expect < 0.02, "{o}");
    assert_eq!(l[5], "True", "{o}");
    // Edges, faces and measures of the exact part (stage C7).
    let o = run(
        &rt,
        r#"
b = afx.cad_model(name="Block")
b.cad.clear("Block")
b.cad.box(size=(20, 20, 20), name="Block")
print(len(b.cad.edges()))
v0 = b.cad.volume()
m = b.cad.measure_faces("Block x-", "Block x+")
print(m["parallel"], round(m["distance"], 3))
b.cad.fillet_edges(("Block z+", "Block x+"), size=2)
print(round(v0 - b.cad.volume()))
b.cad.shell_faces(["Block z+"], thickness=2)
print(b.cad.mass(7.85)["mass_g"] > 0, b.cad.evaluate()["kernel"])
c = afx.cad_model(name="Cube")
c.cad.clear("Cube"); c.cad.box(size=(20, 20, 20), name="C")
print(round(c.cad.interference(c, at=(12, 6, 0), resolution=0.4) / 100))
"#,
    );
    let l: Vec<&str> = o.lines().collect();
    assert_eq!(l[0], "12", "{o}");
    assert_eq!(l[1], "True 20.0", "{o}");
    let lost: i64 = l[2].parse().unwrap();
    assert!((lost - 17).abs() <= 2, "one filleted edge of 20 mm and r=2 loses about 17: {o}");
    assert_eq!(l[3], "True brep", "{o}");
    assert_eq!(l[4], "22", "overlap 8 x 14 x 20 = 2240: {o}");
    // Mirror about a face, a draft, a pattern on a face, and an assembly of components.
    let o = run(
        &rt,
        r#"
h = afx.cad_model(name="Half")
h.cad.clear("Half")
h.cad.box(size=(20, 20, 10), name="Half", at=(10, 0, 0))
v0 = h.cad.volume()
h.cad.mirror_about("Half x-", (1, 0, 0))
print(round(h.cad.volume() / v0, 2))
p = afx.cad_model(name="Plate")
p.cad.clear("Plate")
p.cad.box(size=(60, 60, 10), name="Slab", at=(0, 0, 5))
v1 = p.cad.volume()
p.cad.edit("face_pattern", face="Slab z+", layout="circular", count=4, radius=15, diameter=6, depth=10)
print(round((v1 - p.cad.volume()) / (4 * 3.14159265 * 9 * 10), 2))
d = afx.cad_model(name="Draft")
d.cad.clear("Draft")
d.cad.box(size=(20, 20, 20), name="Block", at=(0, 0, 10))
vd = d.cad.volume()
d.cad.draft(["Block x+"], angle=5)
print(round((vd - d.cad.volume()) / 350.0, 1))
a = afx.cad_model(name="Assembly")
a.cad.clear("Assembly")
a.cad.component_add(name="A", template="bracket")
a.cad.component_add(name="B", template="bracket", at=(20, 0, 0))
a.cad.component_add(name="C", template="bracket", at=(500, 0, 0))
print([c["name"] for c in a.cad.components()])
ov = a.cad.component_interference(resolution=1.0)
print([(o["a"], o["b"]) for o in ov])
a.cad.component_move("B", by=(240, 0, 0))
print(len(a.cad.component_interference(resolution=1.0)))
a.cad.component_op("C", "add", kind="box", name="Extra", size=[5, 5, 5], at=[0, 0, 30])
print(len(a.cad.evaluate()["faces"]) > 12, a.cad.faces()[0]["name"].count("/"))
a.cad.component_visible("C", False)
print(len(a.cad.components()), a.cad.components()[2]["visible"])
"#,
    );
    let l: Vec<&str> = o.lines().collect();
    assert_eq!(l[0], "2.0", "mirror doubles the half: {o}");
    assert_eq!(l[1], "1.0", "four 6 mm holes through 10 mm: {o}");
    assert_eq!(l[2], "1.0", "a 5 degree draft over 20 mm removes a wedge of 0.5 x 20 x 20 tan 5 x 20 = 350: {o}");
    assert_eq!(l[3], "['A', 'B', 'C']", "{o}");
    assert_eq!(l[4], "[('A', 'B')]", "{o}");
    assert_eq!(l[5], "0", "{o}");
    assert_eq!(l[6], "True 1", "faces of a component are named Component/Face: {o}");
    assert_eq!(l[7], "3 False", "{o}");
    let trace = session.lock().trace.join("\n");
    assert!(trace.contains(".cad.edit('add'"), "{trace}");
    std::fs::remove_file("/tmp/afx_cad_py_test.afxcad").ok();
}
