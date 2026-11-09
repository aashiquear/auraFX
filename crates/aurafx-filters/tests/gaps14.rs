//! The filters added for section 14.1, run through the registry with their parameters.
use aurafx_data::*;

fn run(id: &str, input: Option<&DataSet>, kv: &[(&str, serde_json::Value)]) -> DataSet {
    let def = aurafx_filters::registry::find(id).unwrap_or_else(|| panic!("no filter {id}"));
    let mut p = def.default_params();
    for (k, v) in kv {
        p.set_json(k, v);
    }
    def.run(input, &p).unwrap_or_else(|e| panic!("{id}: {e:#}"))
}

fn run_multi(id: &str, inputs: &[&DataSet], kv: &[(&str, serde_json::Value)]) -> DataSet {
    let def = aurafx_filters::registry::find(id).unwrap();
    let mut p = def.default_params();
    for (k, v) in kv {
        p.set_json(k, v);
    }
    def.run_multi(inputs, &p).unwrap_or_else(|e| panic!("{id}: {e:#}"))
}

const NEW: &[&str] = &[
    "point_interpolator",
    "resample_to_line",
    "plot_on_sorted_lines",
    "plot_on_intersection_curves",
    "evenly_spaced_streamlines_2d",
    "extract_time_steps",
    "generate_time_steps",
    "force_time",
    "temporal_snap_to_time_step",
    "temporal_cache",
    "k_means",
    "principal_component_analysis",
    "multicorrelative_statistics",
    "contingency_statistics",
    "compute_quartiles",
    "random_attributes",
    "annotate_time_filter",
    "annotate_global_data",
    "annotate_attribute_data",
    "annotate_selection",
    "environment_annotation",
    "python_annotation",
    "validate_cells",
    "overlapping_cells_detector",
    "redistribute_dataset",
    "ghost_cells",
    "remove_ghost_information",
    "material_interface",
    "fragment_integration",
    "amr_dual_clip",
    "halo_finder",
    "minkowski_functionals",
    "kernel_smoothing",
    "density_map",
];

#[test]
fn every_new_filter_is_described() {
    for id in NEW {
        let d = aurafx_filters::registry::find(id).unwrap_or_else(|| panic!("no filter {id}"));
        assert!(d.description.len() > 40 && !d.keywords.is_empty(), "{id}");
        // Each parameter has a label and a default.
        let _ = d.default_params();
    }
}

fn wavelet() -> DataSet {
    run("wavelet", None, &[])
}

#[test]
fn sampling_and_interpolation_through_the_registry() {
    let w = wavelet();
    let line = run("resample_to_line", Some(&w), &[("sampling", "Cell boundaries".into())]);
    assert!(line.n_points() > 10 && line.point_data().unwrap().get("arc_length").is_some());
    // Points with a field, carried onto a volume and onto the wavelet's points.
    let pts = run("mask_points", Some(&w), &[]);
    let vol = run("point_interpolator", Some(&pts), &[("nx", 10.into()), ("ny", 10.into()), ("nz", 10.into())]);
    assert_eq!(vol.n_points(), 1000);
    let onto = run_multi("point_interpolator", &[&pts, &w], &[("kernel", "Gaussian".into())]);
    assert_eq!(onto.n_points(), w.n_points());
    let sphere = run("sphere", None, &[]);
    let DataSet::MultiBlock(curves) = run("plot_on_intersection_curves", Some(&sphere), &[]) else { panic!() };
    assert_eq!(curves.blocks.len(), 1, "one circle round the sphere");
}

#[test]
fn statistics_annotations_checks_through_the_registry() {
    let w = wavelet();
    let k = run("k_means", Some(&w), &[("k", 4.into()), ("variables", "RTData".into())]);
    assert!(k.point_data().unwrap().get("ClusterId").is_some());
    let q = run("compute_quartiles", Some(&w), &[]);
    assert!(matches!(q, DataSet::Table(_)));
    let a = run("annotate_attribute_data", Some(&w), &[("array", "RTData".into()), ("id", 0.into())]);
    let (text, corner, _) = aurafx_filters::annotate::annotation_of(&a).unwrap();
    assert!(text.starts_with("RTData = ") && corner == "upper left", "{text}");
    let v = run("validate_cells", Some(&w), &[]);
    assert_eq!(v.field_data().get("InvalidCells").unwrap().get(0, 0), 0.0);
    let DataSet::MultiBlock(parts) = run("redistribute_dataset", Some(&w), &[("partitions", 3.into()), ("ghost_layers", 1.into())]) else { panic!() };
    assert_eq!(parts.blocks.len(), 3);
    let n: usize = parts.blocks.iter().map(|b| b.1.as_ref().unwrap().n_cells()).sum();
    assert!(n > w.n_cells(), "ghost cells added");
    let back = run("remove_ghost_information", Some(&DataSet::MultiBlock(parts)), &[]);
    assert_eq!(back.n_cells(), w.n_cells());
}

#[test]
fn amr_dual_clip_keeps_the_high_values() {
    let amr = run("amr_gaussian_pulse", None, &[]);
    let name = "Gaussian-Pulse".to_string();
    assert!(amr.leaves_first_cell_array().is_some());
    let all = run("amr_dual_clip", Some(&amr), &[("array", serde_json::json!({"assoc": "Cells", "name": name})), ("value", (-1e30).into())]);
    let part = run("amr_dual_clip", Some(&amr), &[("array", serde_json::json!({"assoc": "Cells", "name": name})), ("value", 0.1.into())]);
    let rng = |d: &DataSet| {
        let a = d.point_data().unwrap().get(&name).unwrap();
        (0..a.len()).map(|i| a.get(i, 0)).fold((f64::INFINITY, f64::NEG_INFINITY), |(l, h), x| (l.min(x), h.max(x)))
    };
    assert!(
        part.n_cells() > 0 && part.n_cells() < all.n_cells(),
        "{} {} {:?} {:?}",
        part.n_cells(),
        all.n_cells(),
        rng(&all),
        if part.n_cells() > 0 { rng(&part) } else { (0.0, 0.0) }
    );
    let a = part.point_data().unwrap().get(&name).unwrap();
    assert!((0..a.len()).all(|i| a.get(i, 0) >= 0.1 - 1e-6), "{:?} of {:?}", rng(&part), rng(&all));
}

trait FirstArray {
    fn leaves_first_cell_array(&self) -> Option<String>;
}

impl FirstArray for DataSet {
    fn leaves_first_cell_array(&self) -> Option<String> {
        let DataSet::MultiBlock(mb) = self else { return None };
        mb.leaves().into_iter().find_map(|(_, l)| l.cell_data().and_then(|c| c.iter().find(|a| !a.name.starts_with("vtk")).map(|a| a.name.clone())))
    }
}
