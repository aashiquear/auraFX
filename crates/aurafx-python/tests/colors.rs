//! One test function: the embedded interpreter (and its stdout capture) is process-wide.
use aurafx_pipeline::Session;
use aurafx_python::PythonRuntime;

#[test]
fn python_api_behaviour() {
    let p = concat!(env!("CARGO_MANIFEST_DIR"), "/../../testdata/disk_out_ref.ex2");
    let session = Session::shared();
    let rt = PythonRuntime::new(session.clone()).unwrap();

    // Requested colormap survives execution and auto-coloring.
    let out = rt.run(&format!(
        "d = afx.load({p:?})\ns = d.slice(normal=[0,0,1])\ng = s.glyph(orient='V', max_glyphs=50, scale=0.1).color_by('Pres', cmap='Cool to Warm')\nafx.run()"
    ));
    assert!(out.error.is_none(), "{:?}", out.error);
    {
        let s = session.lock();
        let g = s.nodes.values().find(|n| n.name == "Glyph").unwrap();
        assert_eq!(g.display.colormap, "Cool to Warm");
        assert_eq!(g.display.color_by.as_ref().unwrap().name, "Pres");
    }

    // Multi-block data (Exodus) gives merged points and arrays.
    let out = rt.run("print(d.points.shape[1], d.array('Temp').shape[0] == d.points.shape[0])");
    assert!(out.error.is_none(), "{:?}", out.error);
    assert_eq!(out.stdout.trim(), "3 True");

    // Selection + probe + undo.
    let out =
        rt.run("afx.select(d, [0, 1, 2])\nprint(len(afx.selection()[2]))\nprint(round(afx.probe(d, [0, 0, 0])['Temp']) > 0)\nprint(afx.undo() is not None)");
    assert!(out.error.is_none(), "{:?}", out.error);
    assert_eq!(out.stdout.trim(), "3\nTrue\nTrue");

    // Multi-input filters: module function, node method with positional inputs, set_inputs.
    let out = rt.run(
        "a = afx.sphere()\nb = afx.cone()\nj = afx.append_datasets(a, b)\nprint(len(j.inputs))\nk = a.append_datasets(b)\nprint([n.id for n in k.inputs] == [a.id, b.id])\nw = afx.wavelet()\nr = w.resample_with_dataset(a)\nprint(r.info()['n_points'] == a.info()['n_points'], 'vtkValidPointMask' in str(r.info()))\nj.set_inputs()\nprint(len(j.inputs))\nprint(j.activate().id == afx.active().id)",
    );
    assert!(out.error.is_none(), "{:?}", out.error);
    assert_eq!(out.stdout.trim(), "2\nTrue\nTrue True\n1\nTrue");

    // Temporal filters: default seed line, and node method with keyword parameters.
    let rot = concat!(env!("CARGO_MANIFEST_DIR"), "/../../testdata/rotating.pvd");
    let out = rt.run(&format!(
        "f = afx.load({rot:?})\nt = f.particle_tracer(seeds=5)\nprint(t.info()['n_points'])\ni = f.temporal_interpolator(interval=0.5)\nprint(len(afx.times()))\nd = f.temporal_array_operator(array=['POINTS', 'T'])\nprint('T_sub' in str(d.info()))"
    ));
    assert!(out.error.is_none(), "{:?}", out.error);
    assert_eq!(out.stdout.trim(), "5\n9\nTrue");
}
