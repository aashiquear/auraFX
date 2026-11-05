use aurafx_filters::ParamValue;
use aurafx_pipeline::{CameraState, Session};

fn td(n: &str) -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../testdata").join(n)
}

#[test]
fn undo_redo_roundtrip() {
    let mut s = Session::new();
    let r = s.open_file(td("wavelet_tets.vtu")).unwrap();
    let sl = s.add_filter("slice", r).unwrap();
    s.update_blocking();
    let out_before = s.nodes[&sl].output.clone().unwrap();
    s.set_param(sl, "normal", ParamValue::Vec3([0.0, 0.0, 1.0]));
    std::thread::sleep(std::time::Duration::from_millis(700));
    s.rename(sl, "Z cut".into());
    assert_eq!(s.undo_label(), Some("Rename Slice"));
    s.undo();
    assert_eq!(s.nodes[&sl].name, "Slice");
    s.undo();
    assert_eq!(s.nodes[&sl].params.get("normal"), Some(&ParamValue::Vec3([1.0, 0.0, 0.0])));
    s.undo(); // remove slice
    assert!(!s.nodes.contains_key(&sl));
    s.redo(); // slice back
    assert!(s.nodes.contains_key(&sl));
    s.update_blocking();
    assert!(s.nodes[&sl].output.is_some());
    s.redo();
    s.redo();
    assert_eq!(s.nodes[&sl].name, "Z cut");
    s.update_blocking();
    let out_after = s.nodes[&sl].output.clone().unwrap();
    assert!(out_after.bounds().size()[2] < 1e-9, "redo restored the Z normal");
    assert!(out_before.bounds().size()[0] < 1e-9);
    // Display edits merge into one step.
    for k in 0..5 {
        s.set_display(sl, |d| d.opacity = 0.5 + 0.1 * k as f32);
    }
    assert_eq!(s.undo_label(), Some("Display Z cut"));
    s.undo();
    assert_eq!(s.nodes[&sl].display.opacity, 1.0);
}

#[test]
fn save_and_load_state() {
    let mut s = Session::new();
    let r = s.open_file(td("wavelet_tets.vtu")).unwrap();
    let c = s.add_filter("contour", r).unwrap();
    s.set_param(c, "values", ParamValue::Floats(vec![150.0, 200.0]));
    let cl = s.add_filter("clip", r).unwrap(); // origin stays "auto" (NaN)
    s.set_display(cl, |d| d.opacity = 0.4);
    s.update_blocking();
    let n_cells = s.nodes[&c].info.as_ref().unwrap().n_cells;
    let dir = std::env::temp_dir().join(format!("afx-state-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("test.afx");
    let cam = CameraState { eye: [1.0, 2.0, 3.0], target: [0.0; 3], up: [0.0, 0.0, 1.0], fov_y_deg: 30.0, orthographic: false, ortho_height: 1.0 };
    s.save_state(&path, Some(cam)).unwrap();
    let text = std::fs::read_to_string(&path).unwrap();
    assert!(text.contains("\"auto\""), "NaN params saved as auto");

    let mut t = Session::new();
    let got = t.load_state(&path).unwrap();
    assert_eq!(got, Some(cam));
    t.update_blocking();
    assert_eq!(t.nodes.len(), 3);
    let tc = t.nodes.values().find(|n| n.name == "Contour").unwrap();
    assert_eq!(tc.info.as_ref().unwrap().n_cells, n_cells);
    let tcl = t.nodes.values().find(|n| n.name == "Clip").unwrap();
    assert_eq!(tcl.display.opacity, 0.4);
    assert!(tcl.params.get("origin").unwrap().as_vec3().unwrap()[0].is_nan());
    assert!(tcl.error.is_none());
}

#[test]
fn temporal_statistics_of_lammps() {
    let mut s = Session::new();
    let r = s.open_file(td("lammps_small.dump")).unwrap();
    s.update_blocking();
    let ts = s.temporal_statistics(r).unwrap();
    s.update_blocking();
    let out = s.nodes[&ts].output.clone().unwrap();
    let pd = out.point_data().unwrap();
    // "type" is 1,1,2,2 at both steps: average equals the value, stddev 0.
    let avg = pd.get("type_average").unwrap();
    assert_eq!((0..4).map(|i| avg.get(i, 0)).collect::<Vec<_>>(), vec![1.0, 1.0, 2.0, 2.0]);
    assert_eq!(pd.get("type_stddev").unwrap().range(None), (0.0, 0.0));
}

#[test]
fn keyframes_drive_the_pipeline_and_survive_state_files() {
    let mut s = Session::new();
    let r = s.open_file(td("wavelet_tets.vtu")).unwrap();
    let sl = s.add_filter("slice", r).unwrap();
    s.set_param(sl, "normal", ParamValue::Vec3([0.0, 0.0, 1.0]));
    s.animation.key_param(sl, "origin", 0.0, ParamValue::Vec3([0.0, 0.0, -5.0]));
    s.animation.key_param(sl, "origin", 1.0, ParamValue::Vec3([0.0, 0.0, 5.0]));
    let cam = |x: f64| CameraState { eye: [x, 0.0, 0.0], target: [0.0; 3], up: [0.0, 0.0, 1.0], fov_y_deg: 30.0, orthographic: false, ortho_height: 1.0 };
    s.animation.key_camera(0.0, cam(10.0));
    s.animation.key_camera(1.0, cam(20.0));
    let c = s.apply_animation(0.75).unwrap();
    assert!((c.eye[0] - 10.0 * 2f64.powf(0.75)).abs() < 1e-9);
    s.update_blocking();
    let z = s.nodes[&sl].output.as_ref().unwrap().bounds();
    assert!((z.min[2] - 2.5).abs() < 1e-6 && (z.max[2] - 2.5).abs() < 1e-6, "slice at z = 2.5: {z:?}");

    let path = std::env::temp_dir().join(format!("aurafx-kf-{}.afx", std::process::id()));
    s.save_state(&path, None).unwrap();
    let mut s2 = Session::new();
    s2.load_state(&path).unwrap();
    std::fs::remove_file(&path).ok();
    assert_eq!(s2.animation, s.animation);
}

#[test]
fn openfoam_patches_filter_follows_time() {
    let mut s = Session::new();
    let r = s.open_file(td("foam_binary/case.foam")).unwrap();
    let p = s.add_filter("openfoam_patches", r).unwrap();
    s.update_blocking();
    let outlet_t = |s: &Session| {
        let out = s.nodes[&p].output.clone().unwrap_or_else(|| panic!("{:?}", s.nodes[&p].error));
        let aurafx_data::DataSet::MultiBlock(mb) = &*out else { panic!("expected blocks") };
        assert_eq!(mb.blocks.iter().map(|b| b.0.as_str()).collect::<Vec<_>>(), ["inlet", "outlet", "walls"]);
        let o = mb.blocks[1].1.as_ref().unwrap();
        assert!(o.point_data().unwrap().get("T").is_some(), "point data averaged from faces");
        o.cell_data().unwrap().get("T").unwrap().range(None)
    };
    assert_eq!(outlet_t(&s), (300.0, 300.0));
    s.set_time_index(1);
    s.update_blocking();
    assert_eq!(outlet_t(&s), (335.0, 335.0));
    s.set_param(p, "patches", ParamValue::Str("walls".into()));
    s.update_blocking();
    let out = s.nodes[&p].output.clone().unwrap();
    assert_eq!(out.n_cells(), 40);
}

#[test]
fn exodus_all_time_steps_from_a_filter() {
    let mut s = Session::new();
    let r = s.open_file(td("foam_ascii/case.foam")).unwrap();
    let c = s.add_filter("cell_centers", r).unwrap();
    let path = std::env::temp_dir().join(format!("aurafx-steps-{}.exo", std::process::id()));
    assert_eq!(s.save_exodus_all_times(c, &path).unwrap(), 2);
    assert_eq!(s.time_index, 0, "time restored");
    let back = aurafx_io::read_file(&path).unwrap();
    std::fs::remove_file(&path).ok();
    let aurafx_io::Loaded::Series(series) = back else { panic!("expected two steps") };
    assert_eq!(series.times, vec![0.0, 1.0]);
    let last = aurafx_io::load_step(&series.steps[1]).unwrap();
    assert_eq!(last.n_points(), 24);
    let aurafx_data::DataSet::MultiBlock(mb) = &*last else { panic!("expected blocks") };
    let leaf = mb.leaves()[0].1;
    assert_eq!(leaf.point_data().unwrap().get("T").unwrap().range(None), (305.0, 335.0));
}

#[test]
fn multi_input_filters_run_follow_their_inputs_and_survive_state_files() {
    let mut s = Session::new();
    let a = s.add_source("sphere").unwrap();
    let b = s.add_source("cone").unwrap();
    let c = s.add_source("box").unwrap();
    let app = s.add_filter_multi("append_datasets", &[a, b]).unwrap();
    assert_eq!(s.nodes[&app].extra_inputs, vec![b]);
    assert!(!s.nodes[&a].visible && !s.nodes[&b].visible, "appending hides its inputs");
    assert!(s.trace.last().unwrap().contains("= afx.append_datasets(sphere, cone)"), "{:?}", s.trace.last());
    s.update_blocking();
    let n = |s: &Session, id| s.nodes[&id].output.as_ref().map(|o| o.n_points()).unwrap_or(0);
    assert_eq!(n(&s, app), n(&s, a) + n(&s, b));

    // Editing an extra input re-runs the filter.
    s.set_param(b, "resolution", ParamValue::Int(12));
    assert!(s.nodes[&app].dirty);
    s.update_blocking();
    assert_eq!(n(&s, app), n(&s, a) + n(&s, b));

    // A downstream node cannot become an input (no cycles); a filter on one input can't take more.
    let sh = s.add_filter("shrink", app).unwrap();
    assert!(s.set_extra_inputs(app, vec![sh]).is_err());
    assert!(s.set_extra_inputs(sh, vec![c]).is_err());
    s.set_extra_inputs(app, vec![b, c]).unwrap();
    s.update_blocking();
    assert_eq!(n(&s, app), n(&s, a) + n(&s, b) + n(&s, c));
    assert!(s.describe().contains(&format!("more inputs = [{b}] [{c}]")));

    // State files keep the extra inputs.
    let dir = std::env::temp_dir().join(format!("afx_multi_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let f = dir.join("multi.afx");
    s.save_state(&f, None).unwrap();
    let mut t = Session::new();
    t.load_state(&f).unwrap();
    t.update_blocking();
    assert_eq!(t.nodes[&app].extra_inputs, vec![b, c]);
    assert_eq!(n(&t, app), n(&s, app));
    let _ = std::fs::remove_dir_all(&dir);

    // Undo goes back to two inputs.
    s.undo();
    assert_eq!(s.nodes[&app].extra_inputs, vec![b]);

    // Deleting an extra input keeps the filter and drops the reference.
    s.remove_node(b);
    assert!(s.nodes.contains_key(&app));
    assert!(s.nodes[&app].extra_inputs.is_empty());
    s.update_blocking();
    assert_eq!(n(&s, app), n(&s, a));

    // Resample With Dataset needs its destination: error first, result once it is set.
    let w = s.add_source("wavelet").unwrap();
    let r = s.add_filter("resample_with_dataset", w).unwrap();
    s.update_blocking();
    assert!(s.nodes[&r].error.as_deref().unwrap_or("").contains("destination"));
    s.set_extra_inputs(r, vec![a]).unwrap();
    s.update_blocking();
    let out = s.nodes[&r].output.clone().expect("resampled");
    assert_eq!(out.n_points(), n(&s, a));
    assert!(out.point_data().unwrap().get("vtkValidPointMask").is_some());
    assert!(s.set_extra_inputs(r, vec![a, c]).is_err(), "resample takes one destination");
}

/// testdata/rotating.pvd: rigid rotation about z with angular speed 1 + t, steps 0, 0.25 .. 2.
/// A particle released at angle 0 at t = 0 is at angle t + t^2/2, at the same radius.
#[test]
fn temporal_filters_on_a_rotating_flow() {
    use aurafx_data::DataSet;
    let mut s = Session::new();
    let r = s.open_file(td("rotating.pvd")).unwrap();
    assert_eq!(s.times.len(), 9);
    let seeds = s.add_source("line").unwrap();
    s.set_param(seeds, "point1", ParamValue::Vec3([0.3, 0.0, 0.0]));
    s.set_param(seeds, "point2", ParamValue::Vec3([0.6, 0.0, 0.0]));
    s.set_param(seeds, "resolution", ParamValue::Int(3));
    let tracer = s.add_filter_multi("particle_tracer", &[r, seeds]).unwrap();
    let path = s.add_filter_multi("particle_path", &[r, seeds]).unwrap();
    let streak = s.add_filter_multi("streak_line", &[r, seeds]).unwrap();
    let out = |s: &Session, id| -> std::sync::Arc<DataSet> {
        let n = &s.nodes[&id];
        n.output.clone().unwrap_or_else(|| panic!("{}: {:?}", n.name, n.error))
    };
    let check_angles = |ds: &DataSet, t: f64| {
        let want = t + t * t / 2.0;
        let pts = ds.points().unwrap();
        assert_eq!(pts.len(), 4);
        for i in 0..4 {
            let p = pts.get(i);
            let (rad, ang) = (p[0].hypot(p[1]), p[1].atan2(p[0]));
            let d = (ang - want).rem_euclid(std::f64::consts::TAU);
            let d = d.min(std::f64::consts::TAU - d);
            assert!(d < 2e-4, "t {t}: angle {ang} want {want}");
            assert!((rad - (0.3 + 0.1 * i as f64)).abs() < 1e-4, "radius {rad}");
        }
    };
    // Play forward one step at a time (incremental), then compare with a fresh jump to the end.
    for k in 0..9 {
        s.set_time_index(k);
        s.update_blocking();
        check_angles(&out(&s, tracer), s.times[k]);
    }
    let tr = out(&s, tracer);
    let pd = tr.point_data().unwrap();
    for name in ["velocity", "T", "ParticleId", "InjectedPointId", "InjectionStepId", "ParticleAge"] {
        assert!(pd.get(name).is_some(), "missing {name}");
    }
    assert_eq!(pd.get("ParticleAge").unwrap().range(None), (2.0, 2.0));
    // T = x + t sampled at the particles.
    let (p0, t0) = (tr.points().unwrap().get(0), pd.get("T").unwrap().get(0, 0));
    assert!((t0 - (p0[0] + 2.0)).abs() < 1e-6);
    let p = out(&s, path);
    assert_eq!((p.n_points(), p.n_cells()), (36, 4), "4 paths through 9 steps");
    // Arrays recorded along the paths: T = x + t, and every particle was released at t = 0.
    let (pp, pt, age) = (p.points().unwrap(), p.point_data().unwrap().get("T").unwrap(), p.point_data().unwrap().get("ParticleAge").unwrap());
    for i in 0..p.n_points() {
        assert!((pt.get(i, 0) - (pp.get(i)[0] + age.get(i, 0))).abs() < 1e-6, "path point {i}");
    }
    let st = out(&s, streak);
    assert_eq!((st.n_points(), st.n_cells()), (36, 4), "4 streaks of 9 particles each");
    // Restart from scratch (seed moved, then back) and jump straight to the end: same answer.
    s.set_time_index(0);
    s.set_param(seeds, "resolution", ParamValue::Int(3));
    s.mark_dirty(seeds);
    s.set_time_index(8);
    s.update_blocking();
    check_angles(&out(&s, tracer), 2.0);
    // Between steps (time from another reader's steps is not needed: interpolate in time).
    s.set_time_index(0);
    s.update_blocking();
    let p = out(&s, path);
    assert_eq!((p.n_points(), p.n_cells()), (4, 4), "at the start each path is one point");

    // Temporal Interpolator with a 0.1 spacing adds time steps; T = x + t at t = 0.1.
    let ti = s.add_filter("temporal_interpolator", r).unwrap();
    s.set_param(ti, "interval", ParamValue::Float(0.1));
    assert_eq!(s.times.len(), 21 + 4, "0..2 by 0.1 plus 0.25, 0.75, 1.25, 1.75");
    let k = s.times.iter().position(|&t| (t - 0.1).abs() < 1e-9).unwrap();
    s.set_time_index(k);
    s.update_blocking();
    let d = out(&s, ti);
    assert!((d.point_data().unwrap().get("T").unwrap().get(0, 0) - (-0.9)).abs() < 1e-12);
    s.remove_node(ti);
    assert_eq!(s.times.len(), 9);

    // Temporal Shift Scale: (t + 1) * 2 + 0.5 -> 2.5 .. 6.5; at 2.5 the input is at t = 0.
    let ss = s.add_filter("temporal_shift_scale", r).unwrap();
    s.set_param(ss, "pre_shift", ParamValue::Float(1.0));
    s.set_param(ss, "scale", ParamValue::Float(2.0));
    s.set_param(ss, "post_shift", ParamValue::Float(0.5));
    assert!(s.times.iter().any(|&t| (t - 6.5).abs() < 1e-12));
    let k = s.times.iter().position(|&t| (t - 2.5).abs() < 1e-12).unwrap();
    s.set_time_index(k);
    s.update_blocking();
    assert_eq!(out(&s, ss).point_data().unwrap().get("T").unwrap().get(0, 0), -1.0);
    s.remove_node(ss);

    // Temporal Array Operator (ParaView: T_sub = 0.25 relative to the previous step,
    // -1 between steps 0 and 4).
    let ta = s.add_filter("temporal_array_operator", r).unwrap();
    s.set_param(ta, "array", ParamValue::Array { assoc: aurafx_filters::Association::Points, name: "T".into() });
    s.set_time_index(4);
    s.update_blocking();
    let (lo, hi) = out(&s, ta).point_data().unwrap().get("T_sub").unwrap().range(None);
    assert!((lo - 0.25).abs() < 1e-12 && (hi - 0.25).abs() < 1e-12);
    s.set_time_index(0);
    s.update_blocking();
    assert_eq!(out(&s, ta).point_data().unwrap().get("T_sub").unwrap().range(None), (0.0, 0.0), "first step compares with itself");
    s.set_param(ta, "relative", ParamValue::Bool(false));
    s.set_param(ta, "second_step", ParamValue::Int(4));
    s.update_blocking();
    let (lo, hi) = out(&s, ta).point_data().unwrap().get("T_sub").unwrap().range(None);
    assert!((lo + 1.0).abs() < 1e-12 && (hi + 1.0).abs() < 1e-12);

    // Temporal Statistics is a live filter: mean of T = x + t over t in 0..2 is x + 1.
    let ts = s.temporal_statistics(r).unwrap();
    s.update_blocking();
    let d = out(&s, ts);
    let avg = d.point_data().unwrap().get("T_average").unwrap();
    assert!((avg.get(0, 0) - 0.0).abs() < 1e-12, "x = -1 at point 0, mean t = 1");
    assert_eq!(s.nodes[&ts].def().unwrap().id, "temporal_statistics");
    // Its result does not depend on the current time, so changing time does not recompute it.
    s.set_time_index(3);
    s.update_blocking();
    assert!(std::sync::Arc::ptr_eq(&d, &out(&s, ts)));
}

/// A worker thread holds a reader's run (as the app's worker does while the time changes)
/// when a script saves every time step: the blocking update takes the run over, so each step
/// is written with its own data, and the worker's late result is dropped.
#[test]
fn blocking_updates_take_over_runs_held_by_a_worker() {
    let mut s = Session::new();
    let r = s.open_file(td("can.ex2")).unwrap();
    s.update_blocking();
    s.set_time_index(43);
    let held = s.take_work().expect("the reader is dirty");
    let path = std::env::temp_dir().join(format!("aurafx-takeover-{}.case", std::process::id()));
    assert_eq!(s.save_all_times(r, &path).unwrap(), 44);
    // The worker finishes late with the last step's data: dropped.
    let late = held.execute();
    s.complete(held, late, 1.0);
    let eqps_max = |d: &aurafx_data::DataSet| {
        let aurafx_data::DataSet::MultiBlock(mb) = d else { panic!() };
        mb.leaves().iter().map(|(_, l)| l.cell_data().unwrap().get("EQPS").unwrap().range(None).1).fold(f64::MIN, f64::max)
    };
    let aurafx_io::Loaded::Series(written) = aurafx_io::read_file(&path).unwrap() else { panic!() };
    let aurafx_io::Loaded::Series(original) = aurafx_io::read_file(td("can.ex2")).unwrap() else { panic!() };
    for k in [0, 30, 43] {
        let (a, b) = (aurafx_io::load_step(&written.steps[k]).unwrap(), aurafx_io::load_step(&original.steps[k]).unwrap());
        assert!((eqps_max(&a) - eqps_max(&b)).abs() < 1e-6, "step {k}");
    }
    // The node shows the restored time step, not the worker's stale result.
    assert_eq!(s.time_index, 43);
    s.update_blocking();
    let shown = s.nodes[&r].output.clone().unwrap();
    let last = aurafx_io::load_step(&original.steps[43]).unwrap();
    assert!((eqps_max(&shown) - eqps_max(&last)).abs() < 1e-6);
    let dir = path.parent().unwrap();
    for e in std::fs::read_dir(dir).unwrap().flatten() {
        if e.file_name().to_string_lossy().starts_with(&format!("aurafx-takeover-{}.", std::process::id())) {
            let _ = std::fs::remove_file(e.path());
        }
    }
}

/// Reader options (here MPAS's vertical level, multilayer view and lon/lat map): setting one
/// reads the file again with it, at the current time step and after moving in time, and the
/// choice is kept in state files.
#[test]
fn reader_options_reread_the_file() {
    let range = |s: &Session, id, name: &str, points: bool| {
        let d = s.nodes[&id].output.clone().unwrap();
        let a = if points { d.point_data().unwrap().get(name).cloned() } else { d.cell_data().unwrap().get(name).cloned() };
        a.unwrap().range(None)
    };
    let specs = aurafx_pipeline::reader_param_specs(&td("netcdf/mpas_ocean.nc"));
    let keys: Vec<&str> = specs.iter().map(|p| p.key.as_str()).collect();
    assert_eq!(keys, ["vertical_level", "multilayer", "layer_thickness", "atmosphere", "layout", "center_lon"]);
    assert!(aurafx_pipeline::reader_param_specs(&td("can.ex2")).is_empty());

    let mut s = Session::new();
    let r = s.open_file(td("netcdf/mpas_ocean.nc")).unwrap();
    s.set_time_index(2);
    s.update_blocking();
    assert_eq!(range(&s, r, "temperature", true), (-4.467136859893799, 29.0));
    // Level 2 (ParaView with ExtraDimensionIndices nVertLevels=2 gives the same ranges).
    s.set_param(r, "vertical_level", ParamValue::Int(2));
    s.update_blocking();
    assert_eq!(range(&s, r, "temperature", true), (-12.467137336730957, 21.0));
    let v = range(&s, r, "relativeVorticity", false);
    assert!((v.1 - 3.3120497846539365e-6).abs() < 1e-15, "{v:?}");
    // Other time steps are read with the option too.
    s.set_time_index(0);
    s.update_blocking();
    assert_eq!(range(&s, r, "temperature", true), (-10.0, 18.0));
    // Lon/lat map: ParaView's 780 points (with its extra one at the origin) and 980 cells it keeps.
    s.set_param(r, "layout", ParamValue::Str("Lat/Lon map".into()));
    s.update_blocking();
    let d = s.nodes[&r].output.clone().unwrap();
    assert_eq!((d.n_points(), d.n_cells()), (779, 980));
    assert!((d.bounds().max[0] - 391.717474411461).abs() < 1e-6);
    // Multilayer on the map: 5 points per column, layers 5 degrees apart (20 / 4 levels).
    s.set_param(r, "multilayer", ParamValue::Bool(true));
    s.update_blocking();
    let d = s.nodes[&r].output.clone().unwrap();
    assert_eq!((d.n_points(), d.bounds().min[2]), (779 * 5, -20.0));

    let dir = std::env::temp_dir().join(format!("aurafx_reader_opts_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("mpas.afx");
    s.save_state(&path, None).unwrap();
    let mut t = Session::new();
    t.load_state(&path).unwrap();
    t.update_blocking();
    let id = *t.nodes.keys().next().unwrap();
    let d = t.nodes[&id].output.clone().unwrap();
    assert_eq!(d.n_points(), 779 * 5, "options restored from the state file");
    std::fs::remove_dir_all(&dir).ok();

    // Back to the defaults: the file as first read.
    s.set_params(r, aurafx_filters::Params::default());
    s.update_blocking();
    let d = s.nodes[&r].output.clone().unwrap();
    assert_eq!((d.n_points(), d.n_cells()), (642, 949));
}

/// A source with time steps of its own (Spatio Temporal Harmonics): its steps become the
/// animation's, it is recomputed for the current one, filters below follow, and a saved state
/// brings it back with them.
#[test]
fn source_with_time_steps() {
    let value_at = |s: &Session, id, i: usize| s.nodes[&id].output.as_ref().unwrap().point_data().unwrap().get("SpatioTemporalHarmonics").unwrap().get(i, 0);
    let mut s = Session::new();
    let src = s.add_source("spatio_temporal_harmonics_grid").unwrap();
    s.set_param(src, "extent", ParamValue::Floats(vec![-2.0, 2.0, -2.0, 2.0, 0.0, 0.0]));
    let cut = s.add_filter("threshold", src).unwrap();
    s.update_blocking();
    assert_eq!(s.times.len(), 20);
    assert_eq!(s.times[3], 0.9425);
    let h = aurafx_filters::texture::DEFAULT_HARMONICS;
    let expect = |t: f64| aurafx_filters::texture::harmonics_value(&h, [-2.0, -2.0, 0.0], t);
    assert!((value_at(&s, src, 0) - expect(0.0)).abs() < 1e-12);
    s.set_time_index(3);
    s.update_blocking();
    assert!((value_at(&s, src, 0) - expect(0.9425)).abs() < 1e-12);
    // The threshold below was recomputed at the new time too.
    let range = |s: &Session, id| s.nodes[&id].output.as_ref().unwrap().point_data().unwrap().get("SpatioTemporalHarmonics").unwrap().range(None);
    assert_eq!(range(&s, cut).1, range(&s, src).1);
    // Fewer time steps from the parameter.
    s.set_param(src, "time_steps", ParamValue::Floats(vec![0.0, 0.5]));
    s.update_blocking();
    assert_eq!(s.times, vec![0.0, 0.5]);
    let path = std::env::temp_dir().join(format!("aurafx-harmonics-{}.afx", std::process::id()));
    s.save_state(&path, None).unwrap();
    let mut t = Session::new();
    t.load_state(&path).unwrap();
    t.update_blocking();
    let _ = std::fs::remove_file(&path);
    assert_eq!(t.times, vec![0.0, 0.5]);
}
