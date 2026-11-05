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

    // ---------------- plugins and Python filters ----------------
    let dir = std::env::temp_dir().join(format!("afx_plugins_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let plugin = dir.join("my_tools.py");
    let write_plugin = |factor: &str| {
        std::fs::write(
            &plugin,
            format!(
                r#"
import aurafx as afx
import numpy as np

@afx.filter(category="Test")
def scale_points(data, factor={factor}, mode=afx.Choice(["a", "b"]), array=afx.ArrayParam()):
    """Scale the points."""
    data.points = data.points * factor
    data.field_data["mode"] = np.array([1.0 if mode == "a" else 2.0])
    return data

@afx.filter(inputs=(2, None))
def count_points(inputs):
    out = inputs[0].copy()
    out.field_data["total"] = np.array([float(sum(d.n_points for d in inputs))])
    return out

@afx.source
def tri(size=1.0):
    return afx.make_polydata([[0, 0, 0], [size, 0, 0], [0, size, 0]], polys=[[0, 1, 2]])

@afx.reader(extensions=["xyzt"], label="XYZT")
def read_xyzt(path):
    a = np.loadtxt(path, ndmin=2)
    d = afx.make_points(a[:, :3])
    d.point_data["T"] = a[:, 3]
    return d

@afx.filter
def sneaky(data):
    afx.load("nothing.vtu")
    return data
"#
            ),
        )
        .unwrap();
    };
    write_plugin("2.0");
    std::fs::write(dir.join("pts.xyzt"), "0 0 0 1\n1 0 0 2\n0 1 0 3\n").unwrap();
    let out = rt.run(&format!(
        "ids = afx.load_plugin({:?})\nprint(sorted(ids))\nt = afx.tri(size=2.0)\ns = t.scale_points(mode='b')\nprint(s.info()['bounds']['max'][0], s.info()['n_points'])\nc = afx.count_points(t, s)\nprint(c.info()['n_points'])\nr = afx.load({:?})\nprint(r.info()['n_points'], r.array('T').tolist())\nbad = t.sneaky()\ntry:\n    bad.info()\nexcept Exception as e:\n    print('cannot be used inside' in str(e))",
        plugin,
        dir.join("pts.xyzt")
    ));
    assert!(out.error.is_none(), "{:?}\n{}", out.error, out.stdout);
    assert_eq!(out.stdout.trim(), "['count_points', 'scale_points', 'sneaky', 'tri']\n4.0 3\n3\n3 [1.0, 2.0, 3.0]\nTrue", "{}", out.stdout);
    {
        let s = session.lock();
        let sn = s.nodes.values().find(|n| n.def().is_some_and(|d| d.id == "scale_points")).unwrap();
        let fd = sn.output.as_ref().unwrap().field_data().get("mode").unwrap().get(0, 0);
        assert_eq!(fd, 2.0, "Choice parameter reached the function");
    }
    // Hot reload: a new default factor applies to the existing node (its own value was default).
    std::thread::sleep(std::time::Duration::from_millis(20));
    write_plugin("3.0");
    let (msgs, changed) = aurafx_python::plugins::sync(&[dir.clone()], &[]);
    assert!(changed.iter().any(|c| c == "scale_points"), "{msgs:?}");
    session.lock().refresh_filter_defs(&changed);
    session.lock().update_blocking();
    let out = rt.run("print(s.info()['bounds']['max'][0])");
    assert_eq!(out.stdout.trim(), "4.0", "the node kept its own parameter value (2.0 was stored when created)");
    // Disabling removes the filters.
    aurafx_python::plugins::sync(&[dir.clone()], &[plugin.clone()]);
    assert!(aurafx_filters::find("scale_points").is_none());
    assert!(aurafx_python::plugins::list().iter().any(|p| p.path == plugin && !p.enabled));

    // Programmable Filter / Source and Python Calculator.
    let out = rt.run(
        "w = afx.wavelet()\np = w.programmable_filter(script=\"output.point_data['twice'] = inputs[0].point_data['RTData'] * 2\\noutput.PointData.append(np.ones(output.n_points), 'one')\")\nprint(sorted(p.arrays()['points']))\nq = afx.programmable_source(script='output = afx.make_points(np.zeros((5, 3)))')\nprint(q.info()['n_points'])\nk = w.python_calculator(expression='RTData * 3 + points[:, 0]', result='rt3')\nimport numpy as _np\nprint(bool(_np.allclose(k.array('rt3'), w.array('RTData') * 3 + w.points[:, 0])))\ne = w.programmable_filter(script='raise ValueError(\"boom\")')\ntry:\n    e.info()\nexcept Exception as x:\n    print('boom' in str(x))",
    );
    assert!(out.error.is_none(), "{:?}\n{}", out.error, out.stdout);
    assert_eq!(out.stdout.trim(), "['RTData', 'one', 'twice']\n5\nTrue\nTrue", "{}", out.stdout);
    let _ = std::fs::remove_dir_all(&dir);

    // Higher-order cells: the cell type source, Tessellate and the display subdivision level.
    let out = rt.run(
        "c = afx.unstructured_cell_types(cell_type='Lagrange Hexahedron', blocks=(2, 1, 1), order=2)\nprint(c.info()['n_points'], c.info()['n_cells'], c.info()['cell_types'])\nt = c.tessellate(level=1)\nprint(t.info()['n_cells'], t.info()['curved_cells'])\nc.set(nonlinear_subdivision=3)",
    );
    assert!(out.error.is_none(), "{:?}\n{}", out.error, out.stdout);
    assert_eq!(out.stdout.trim(), "45 2 [['LagrangeHexahedron', 2]]\n16 0", "{}", out.stdout);
    {
        let s = session.lock();
        assert!(s.nodes.values().any(|n| n.display.nonlinear_subdivision == 3));
    }

    // Client/server: a file opened on a server (here one running in this process).
    session.lock().remote_clients.insert("inproc".into(), aurafx_pipeline::remote::RemoteClient::in_process("inproc"));
    let disk = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../testdata/disk_out_ref.ex2");
    let out = rt.run(&format!(
        "r = afx.load({:?}, host='inproc')\nsl = r.slice(normal=(0, 0, 1))\nprint(sl.info()['n_cells'] > 0, [x['name'] for x in afx.servers()])\ntry:\n    afx.load('ssh://inproc/no/such/file.vtu')\nexcept Exception as e:\n    print('such' in str(e))",
        disk.display().to_string()
    ));
    assert!(out.error.is_none(), "{:?}\n{}", out.error, out.stdout);
    assert_eq!(out.stdout.trim(), "True ['inproc']\nTrue", "{}", out.stdout);
    {
        let s = session.lock();
        let sl = s.nodes.values().find(|n| n.def().is_some_and(|d| d.id == "slice") && s.remote_host(n.id).is_some()).expect("slice on the server");
        assert!(sl.remote.is_some());
    }

    // Textures and made-up data (the user guide's examples).
    let img = concat!(env!("CARGO_MANIFEST_DIR"), "/../../testdata/images/planet.jpg");
    let out = rt.run(&format!(
        "ball = afx.sphere(radius=1, theta=128, phi=96)\nglobe = ball.texture_map_to_sphere(prevent_seam=False)\nglobe.set(texture={img:?})\nprint('Texture Coordinates' in str(globe.info()))\n\
         h = afx.spatio_temporal_harmonics_grid()\nh.set(harmonics=[2, 1, 0.3, 0, 0, 0])\nafx.time(5)\nprint(len(afx.times()) >= 20)\n\
         n = afx.plane(xres=200, yres=200).perlin_noise(frequency=(4, 4, 4), amplitude=0.1)\nw = n.warp_by_scalar(array='PerlinNoise')\nprint(w.info()['n_points'])\n\
         try:\n    globe.set(texture='/no/such.png')\nexcept ValueError as e:\n    print('no image' in str(e))\nglobe.set(texture=None)"
    ));
    assert!(out.error.is_none(), "{:?}\n{}", out.error, out.stdout);
    assert_eq!(out.stdout.trim(), "True\nTrue\n40401\nTrue", "{}", out.stdout);
    let s = session.lock();
    let g = s.nodes.values().find(|n| n.def().is_some_and(|d| d.id == "texture_map_to_sphere")).unwrap();
    assert_eq!(g.display.texture, None);
    let h = s.nodes.values().find(|n| n.def().is_some_and(|d| d.id == "spatio_temporal_harmonics_grid")).unwrap();
    assert_eq!(h.params.floats("harmonics"), vec![2.0, 1.0, 0.3, 0.0, 0.0, 0.0]);
    drop(s);

    // Display keys of node.set (the guide's examples), on a reader with options and on filters;
    // a filter's own parameter of the same name (Threshold's component) stays a parameter.
    let gj = concat!(env!("CARGO_MANIFEST_DIR"), "/../../testdata/vectors/rivers.geojson");
    let out = rt.run(&format!(
        "r = afx.load({gj:?})\nr.set(color_by=None, solid_color=(0.1, 0.4, 0.9), line_width=3, lighting=False)\nr.set(polygons='Filled')\n\
         w = afx.wavelet()\nw.set(color_by='RTData', range=(50, 200), log_scale=True, scalar_bar=False)\nt = w.threshold(component='X')\n\
         try:\n    r.set(no_such_option=1)\nexcept ValueError as e:\n    print('blocks' in str(e))"
    ));
    assert!(out.error.is_none(), "{:?}\n{}", out.error, out.stdout);
    assert_eq!(out.stdout.trim(), "True");
    let s = session.lock();
    let r = s.nodes.values().find(|n| n.name == "rivers.geojson").unwrap();
    assert_eq!((r.display.color_by.is_none(), r.display.solid_color, r.display.line_width, r.display.lighting), (true, [0.1, 0.4, 0.9], 3.0, false));
    assert_eq!(r.params.get("polygons").and_then(|v| v.as_str()), Some("Filled"));
    let w = s.nodes.values().filter(|n| n.def().is_some_and(|d| d.id == "wavelet")).max_by_key(|n| n.id).unwrap();
    assert_eq!((w.display.range, w.display.log_scale, w.display.show_scalar_bar), (Some((50.0, 200.0)), true, false));
    let t = s.nodes.values().filter(|n| n.def().is_some_and(|d| d.id == "threshold")).max_by_key(|n| n.id).unwrap();
    assert_eq!(t.params.str("component"), "X");
}
