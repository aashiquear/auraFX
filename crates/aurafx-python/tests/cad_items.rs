//! Sweeps and lofts, sketches with constraints, sketches on faces, a part from another node's
//! solid, a component edited on its own, and mates, all from Python. One test function: the
//! embedded interpreter is process-wide.
use aurafx_pipeline::Session;
use aurafx_python::PythonRuntime;

fn run(rt: &PythonRuntime, code: &str) -> String {
    let out = rt.run(code);
    assert!(out.error.is_none(), "{:?}\n{code}", out.error);
    out.stdout.trim().to_string()
}

#[test]
fn the_cad_items_from_python() {
    let session = Session::shared();
    let rt = PythonRuntime::new(session.clone()).unwrap();

    // A sketch drawn line by line, squared by constraints, sized by a dimension of the part.
    let o = run(
        &rt,
        r#"
m = afx.cad_model(name="Bracket")
m.cad.clear("Bracket")
m.cad.parameter("w", 40)
s = m.cad.sketch(plane="XY")
a = s.line((0, 0), (38, 1))
b = s.line(a.end, (41, 19))
c = s.line(b.end, (2, 22))
d = s.line(c.end, a.start)
s.fix(a.start)
s.horizontal(a).vertical(b).horizontal(c).vertical(d)
print(s.solve()["status"], s.solve()["dof"])
s.distance(a, "w")
s.dimension(a.end, b.end, 20)
r = s.solve()
print(r["status"], r["dof"], r["outlines"], [round(v, 6) for v in b.end.xy])
m.cad.extrude(s, depth=5, name="Plate")
print(round(m.cad.volume()))
m.set(w=60)
print(round(m.cad.volume()))
hole = m.cad.sketch(face="Plate top")
k = hole.add_circle((10, 10), 3)
hole.radius(k, 4)
hole.hdistance(k.center, hole.point(0, 0, fixed=True), 12)
m.cad.extrude(hole, depth=2, op="cut", name="Pocket")
print(round(60 * 20 * 5 - m.cad.volume(), 1))
try:
    s.horizontal(k)
except ValueError as e:
    print("refused:", "this sketch" in str(e))
"#,
    );
    let l: Vec<&str> = o.lines().collect();
    assert_eq!(l[0], "under-defined 2", "{o}");
    assert_eq!(l[1], "fully defined 0 1 [40.0, 20.0]", "{o}");
    assert_eq!(l[2], "4000", "{o}");
    assert_eq!(l[3], "6000", "{o}");
    let pocket: f64 = l[4].parse().unwrap();
    assert!((pocket - std::f64::consts::PI * 16.0 * 2.0).abs() < 1.0, "{o}");
    assert_eq!(l[5], "refused: True", "{o}");

    // A sweep and a loft from sketches kept with the part.
    let o = run(
        &rt,
        r#"
h = afx.cad_model(name="Handle")
h.cad.clear("Handle")
h.cad.sketch_add(h.cad.sketch(plane="YZ").circle(r=3), name="Profile")
path = h.cad.sketch(plane="XY")
p1 = path.line((0, 0), (40, 0)); path.line(p1.end, (40, 25))
h.cad.sketch_add(path, name="Path")
print(h.cad.sweep("Profile", "Path")["name"], [k["name"] for k in h.cad.sketches()])
names = sorted(f["name"] for f in h.cad.faces())
print(names)
print(h.cad.evaluate()["kernel"])
h.cad.loft([h.cad.sketch(offset=-12).rectangle(20, 20), h.cad.sketch(offset=-3).circle(r=4)], name="Foot")
print(len(h.cad.sketches()), any(f["name"] == "Foot top" for f in h.cad.faces()))
try:
    h.cad.sweep("Profile", "Profile")
except ValueError as e:
    print("refused:", "two sketches" in str(e))
"#,
    );
    let l: Vec<&str> = o.lines().collect();
    assert_eq!(l[0], "Sweep ['Profile', 'Path']", "{o}");
    assert_eq!(l[1], "['Sweep end', 'Sweep side', 'Sweep start']", "{o}");
    assert_eq!(l[3], "4 True", "{o}");
    assert_eq!(l[4], "refused: True", "{o}");

    // A boss on a face follows the face; a face that goes is said and repaired.
    let o = run(
        &rt,
        r#"
p = afx.cad_model(name="P")
p.cad.clear("P")
p.cad.parameter("t", 6)
p.cad.box(size=(30, 20, "t"), at=(0, 0, "t / 2"), name="Plate")
p.cad.extrude(p.cad.sketch(face="Plate z+").circle(r=4), depth=5, name="Boss")
print(round(p.cad.evaluate()["bounds"][1][2], 3))
p.set(t=10)
print(round(p.cad.evaluate()["bounds"][1][2], 3))
p.cad.rename("Plate", "Slab")
try:
    p.cad.evaluate()
except ValueError as e:
    print("pick another face" in str(e))
p.cad.sketch_face("Boss", "Slab z+")
print(round(p.cad.evaluate()["bounds"][1][2], 3))
"#,
    );
    assert_eq!(o, "11.0\n15.0\nTrue\n15.0", "{o}");

    // A Geometry source as the first body of a part.
    let o = run(
        &rt,
        r#"
g = afx.mesh.geometry()
g.cmd("brick x 30 y 20 z 10")
body = g.result(resolution=48)
body.update()
c = afx.cad_from(body, name="From commands")
print(c.name, len(c.cad.features()), c.cad.features()[0]["shape"])
v0 = c.cad.volume()
c.cad.hole(diameter=6, depth=30, at=(0, 0, 6), name="Bore")
print(round(v0), round(v0 - c.cad.volume()))
"#,
    );
    let l: Vec<&str> = o.lines().collect();
    assert!(l[0].ends_with("1 Brep") && l[0].contains("(CAD)"), "{o}");
    let nums: Vec<f64> = l[1].split_whitespace().map(|x| x.parse().unwrap()).collect();
    assert!((nums[0] - 6000.0).abs() < 60.0 && (nums[1] - std::f64::consts::PI * 90.0).abs() < 25.0, "{o}");

    // An assembly: a component edited on its own, mates, and a mate's value on the animation.
    let o = run(
        &rt,
        r#"
a = afx.cad_model(name="Assembly")
a.cad.clear("Assembly")
a.update
e = a.edit_cad()
e.component_add(name="Base")
e.component_add(name="Cap", at=(70, 10, 30), rotate=(0, 0, 20))
e.component_op("Base", "add", kind="box", name="Slab", size=[40, 40, 10], at=[0, 0, 5])
e.edit_component("Cap")
print(e.state()["component"], len(e.features()))
e.box(size=(40, 40, 10), at=(0, 0, 5), name="Slab")
e.hole(diameter=8, depth=30, at=(0, 0, 10), name="Bore")
print([f["name"] for f in e.features()], any("/" in f["name"] for f in e.faces()))
try:
    e.component_add(name="X")
except ValueError as err:
    print("back first:", "go back to the assembly" in str(err))
e.edit_component(None)
print(e.state()["component"], [k["features"] for k in e.components()])
e.parameter("gap", 5)
e.mate("fixed", "Base")
print(e.mate("coincident", "Base/Slab z+", "Cap/Slab z-")["dof"])
r = e.mate("distance", "Base/Slab x+", "Cap/Slab x-", value="gap", name="Gap")
print(r["dof"], r["status"])
cap = [q for q in e.placements() if q["name"] == "Cap"][0]
print([round(v, 3) for v in cap["at"]])
e.mate_set("Gap", value=12)
print(round([q for q in e.placements() if q["name"] == "Cap"][0]["at"][0], 3))
e.mate_set("Gap", value="gap")
print([(q["name"], q["error"], q["dof"]) for q in e.mates()][-1])
try:
    e.mate("distance", "Base/Slab z+", "Cap/Slab z-", value=3)
except ValueError as err:
    print("refused:", "contradict" in str(err))
e.apply()
a.key("gap", 0, 5)
a.key("gap", 1, 25)
"#,
    );
    let l: Vec<&str> = o.lines().collect();
    assert_eq!(l[0], "Cap 0", "{o}");
    assert_eq!(l[1], "['Slab', 'Bore'] False", "{o}");
    assert_eq!(l[2], "back first: True", "{o}");
    assert_eq!(l[3], "None [1, 2]", "{o}");
    assert_eq!(l[4], "3", "{o}");
    assert_eq!(l[5], "1 1 way(s) left to move", "{o}");
    assert_eq!(l[6], "[45.0, 10.0, 10.0]", "{o}");
    assert_eq!(l[7], "52.0", "{o}");
    assert_eq!(l[8], "('Gap', None, 1)", "{o}");
    assert_eq!(l[9], "refused: True", "{o}");
    // The key on "gap" is a track on the document's dimension: half way, the cap is 15 mm off.
    {
        let mut s = session.lock();
        assert!(s.animation.tracks.iter().any(|t| t.key == "cad.gap"), "{:?}", s.animation.tracks.iter().map(|t| &t.key).collect::<Vec<_>>());
        s.apply_animation(0.5);
    }
    let o = run(&rt, "print(round([q for q in a.cad.placements() if q['name'] == 'Cap'][0]['at'][0], 3), a.cad.parameters()['gap'])");
    assert_eq!(o, "55.0 15.0", "{o}");
}
