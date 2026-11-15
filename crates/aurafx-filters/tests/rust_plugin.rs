//! A Rust plugin built from crates/aurafx-plugin-example, loaded and run through the registry.
use aurafx_data::*;

fn plugin_library() -> std::path::PathBuf {
    // Built into its own target folder so this does not wait on the build that runs the tests.
    let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let target = root.join("target/plugin-test");
    let st = std::process::Command::new(env!("CARGO"))
        .args(["build", "--quiet", "-p", "aurafx-plugin-example", "--target-dir"])
        .arg(&target)
        .current_dir(&root)
        .status()
        .expect("cargo");
    assert!(st.success(), "building the example plugin");
    let name = format!("{}aurafx_plugin_example.{}", std::env::consts::DLL_PREFIX, std::env::consts::DLL_EXTENSION);
    target.join("debug").join(name)
}

#[test]
fn a_rust_plugin_adds_a_filter_and_a_source() {
    let lib = plugin_library();
    let ids = aurafx_filters::rust_plugins::load(&lib).unwrap();
    assert_eq!(ids, vec!["scale_points", "spiral"]);
    // The source.
    let spiral = aurafx_filters::find("spiral").unwrap();
    assert!(spiral.is_source);
    let mut p = spiral.default_params();
    p.set_json("points", &serde_json::json!(50));
    let s = spiral.run(None, &p).unwrap();
    assert_eq!(s.n_points(), 50);
    assert!(s.point_data().unwrap().get("Angle").is_some());
    // The filter, with its settings from the Properties panel.
    let f = aurafx_filters::find("Scale Points").unwrap();
    assert_eq!(f.params.iter().map(|q| q.key.as_str()).collect::<Vec<_>>(), vec!["factor", "center"]);
    let mut q = f.default_params();
    q.set_json("factor", &serde_json::json!(3.0));
    let scaled = f.run(Some(&s), &q).unwrap();
    let (a, b) = (s.points().unwrap().get(49), scaled.points().unwrap().get(49));
    assert!((0..3).all(|k| (b[k] - 3.0 * a[k]).abs() < 1e-12), "{a:?} {b:?}");
    // An error from the plugin comes back as an error, not a crash.
    let img = DataSet::ImageData(ImageData { extent: [0, 1, 0, 1, 0, 1], ..Default::default() });
    let e = f.run(Some(&img), &q).unwrap_err().to_string();
    assert!(e.contains("Scale Points takes"), "{e}");
    // Loading it twice is refused.
    assert!(aurafx_filters::rust_plugins::load(&lib).is_err());
}
