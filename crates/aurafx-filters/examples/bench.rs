//! Timing of the heavy filters on a large wavelet: `cargo run --release -p aurafx-filters --example bench [half_size]`.
use aurafx_data::DataSet;
use std::time::Instant;

/// Fingerprint of a dataset's geometry, topology and arrays.
fn digest(ds: &DataSet) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    fn walk(ds: &DataSet, h: &mut impl Hasher) {
        let attrs = |a: Option<&aurafx_data::Attributes>, h: &mut dyn Hasher| {
            if let Some(a) = a {
                for arr in a.iter() {
                    arr.name.hash(&mut HashW(h));
                    for i in 0..arr.data.len() {
                        arr.data.get_f64(i).to_bits().hash(&mut HashW(h));
                    }
                }
            }
        };
        match ds {
            DataSet::PolyData(p) => {
                for i in 0..p.points.data.len() {
                    p.points.data.get_f64(i).to_bits().hash(h);
                }
                for c in [&p.verts, &p.lines, &p.polys, &p.strips] {
                    c.connectivity.hash(h);
                    c.offsets.hash(h);
                }
            }
            DataSet::UnstructuredGrid(u) => {
                for i in 0..u.points.data.len() {
                    u.points.data.get_f64(i).to_bits().hash(h);
                }
                u.cells.connectivity.hash(h);
                u.cells.offsets.hash(h);
                u.cells.types.iter().for_each(|t| (*t as u8).hash(h));
            }
            DataSet::Table(t) => t.columns.iter().for_each(|c| (0..c.data.len()).for_each(|i| c.data.get_f64(i).to_bits().hash(h))),
            _ => ds.n_points().hash(h),
        }
        attrs(ds.point_data(), h);
        attrs(ds.cell_data(), h);
    }
    walk(ds, &mut h);
    h.finish()
}

struct HashW<'a>(&'a mut dyn std::hash::Hasher);
impl std::hash::Hasher for HashW<'_> {
    fn finish(&self) -> u64 {
        self.0.finish()
    }
    fn write(&mut self, b: &[u8]) {
        self.0.write(b)
    }
}

fn main() {
    let h: i64 = std::env::args().nth(1).and_then(|a| a.parse().ok()).unwrap_or(80);
    let img = DataSet::ImageData(aurafx_filters::sources::wavelet([-h, h, -h, h, -h, h], 255.0, 0.5));
    let ug = DataSet::UnstructuredGrid(img.to_unstructured().unwrap());
    println!("{} points, {} cells, {} threads", ug.n_points(), ug.n_cells(), rayon::current_num_threads());
    let only = std::env::args().nth(2);
    let run = |label: &str, id: &str, input: &DataSet, set: &[(&str, serde_json::Value)]| {
        if only.as_deref().is_some_and(|o| o != id) {
            return;
        }
        let def = aurafx_filters::find(id).unwrap();
        let mut p = def.default_params();
        for (k, v) in set {
            p.set_json(k, v);
        }
        let t0 = Instant::now();
        let out = def.run(Some(input), &p).unwrap();
        let ms = t0.elapsed().as_secs_f64() * 1e3;
        println!("{label:<32} {ms:>8.0} ms   -> {:>8} points {:>8} cells  {:016x}", out.n_points(), out.n_cells(), digest(&out));
    };
    for (name, ds) in [("image", &img), ("unstructured", &ug)] {
        run(&format!("slice ({name})"), "slice", ds, &[("normal", serde_json::json!([1.0, 1.0, 1.0]))]);
        run(&format!("contour ({name})"), "contour", ds, &[("values", serde_json::json!([150.0]))]);
        run(&format!("clip ({name})"), "clip", ds, &[("normal", serde_json::json!([1.0, 1.0, 0.0]))]);
        run(&format!("threshold ({name})"), "threshold", ds, &[("lower", serde_json::json!(150.0)), ("upper", serde_json::json!(300.0))]);
        run(&format!("extract_surface ({name})"), "extract_surface", ds, &[]);
        run(&format!("gradient ({name})"), "gradient", ds, &[("array", "RTData".into())]);
        run(&format!("integrate ({name})"), "integrate_variables", ds, &[]);
        run(&format!("cell_to_point ({name})"), "cell_data_to_point_data", ds, &[]);
    }
}
