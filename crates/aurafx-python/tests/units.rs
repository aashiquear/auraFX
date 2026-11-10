//! Units on arrays (in its own test binary: the Python module keeps one session per process).
use aurafx_pipeline::Session;
use aurafx_python::PythonRuntime;

/// Units: the Set Units filter, node.info() and the units of a Data object's arrays.
#[test]
fn arrays_carry_units() {
    let rt = PythonRuntime::new(Session::shared()).unwrap();
    let out = rt.run(
        r#"
import aurafx as afx, numpy as np
w = afx.wavelet()
u = w.set_units(array="RTData", units="K")
a = [x for x in u.info()["point_arrays"] if x["name"] == "RTData"][0]
d = afx.make_points(np.zeros((3, 3)))
d.point_data["p"] = np.arange(3.0)
d.point_data.set_units("p", "MPa")
r = (a["units"], d.point_data.units("p"))
d.point_data.set_units("p", "")
r = r + (d.point_data.units("p"),)
"#,
    );
    assert!(out.error.is_none(), "{:?}", out.error);
    assert_eq!(rt.run("r").value.as_deref(), Some("('K', 'MPa', None)"));
}
