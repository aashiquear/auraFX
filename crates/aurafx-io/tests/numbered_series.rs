//! Numbered files (`heat_0000.vtk`, `heat_0050.vtk`, ...) open as a time series whose times
//! are the numbers; each step reads alone; a lone numbered file stays a single dataset.

use aurafx_data::*;
use std::path::PathBuf;

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

#[test]
fn numbered_vtk_files_are_a_series() {
    let aurafx_io::Loaded::Series(s) = aurafx_io::read_file(root().join("examples/heat2d/heat_0100.vtk")).unwrap() else { panic!("not a series") };
    assert_eq!(s.times, (0..9).map(|i| 50.0 * i as f64).collect::<Vec<_>>());
    let first = aurafx_io::load_step(&s.steps[0]).unwrap();
    let last = aurafx_io::load_step(&s.steps[8]).unwrap();
    let DataSet::ImageData(a) = &*first else { panic!() };
    let DataSet::ImageData(b) = &*last else { panic!() };
    let (ta, tb) = (a.point_data.get("temperature").unwrap(), b.point_data.get("temperature").unwrap());
    let max = |t: &Array| (0..t.len()).map(|i| t.get(i, 0)).fold(0.0, f64::max);
    assert!(max(ta) > max(tb) + 10.0, "heat spreads: {} then {}", max(ta), max(tb));
    // A step on its own is not expanded again.
    assert!(matches!(aurafx_io::read_file_single(root().join("examples/heat2d/heat_0100.vtk")).unwrap(), aurafx_io::Loaded::Static(_)));
    // A numbered name without siblings stays single.
    assert!(matches!(aurafx_io::read_file(root().join("testdata/benchmark/plate_h40_hole5.e")).unwrap(), aurafx_io::Loaded::Static(_)));
}
