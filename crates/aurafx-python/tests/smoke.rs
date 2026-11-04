use aurafx_pipeline::Session;
use aurafx_python::PythonRuntime;

#[test]
fn script_drives_pipeline() {
    let session = Session::shared();
    let rt = PythonRuntime::new(session.clone()).unwrap();
    let td = concat!(env!("CARGO_MANIFEST_DIR"), "/../../testdata/wavelet_tets.vtu");
    let out = rt.run(&format!(
        "m = afx.load({td:?})\ns = m.slice(normal=[0,0,1]).color_by('RTData', cmap='Plasma')\nc = m.contour(values=[150])\nprint(c.info()['n_cells'] > 0)\nimport numpy as np\npts = s.points\nprint(pts.shape[1], float(np.abs(pts[:,2]).max()) < 1e-6)\nm.add_array('twice', s.array('RTData')*2 if False else m.array('RTData')*2)\nprint(len(afx.nodes()))\n"
    ));
    assert!(out.error.is_none(), "{:?}", out.error);
    assert_eq!(out.stdout.trim(), "True\n3 True\n3");
    let out = rt.run("1 + 2");
    assert_eq!(out.value.as_deref(), Some("3"));
    let out = rt.run("afx.describe()");
    assert!(out.value.unwrap().contains("Slice"));
    assert!(rt.complete("afx.lo").contains(&"load".to_string()));
    let s = session.lock();
    assert_eq!(s.nodes.len(), 3);
    assert!(s.nodes.values().any(|n| n.display.colormap == "Plasma"));
}
