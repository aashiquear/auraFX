//! An example Rust plugin for auraFX. Build it with
//! `cargo build --release -p aurafx-plugin-example` and copy the library
//! (`libaurafx_plugin_example.so`, `.dylib` or `aurafx_plugin_example.dll`) into the plugin folder.
use aurafx_plugin::aurafx_data::{Array, CellType, Points, PolyData};
use aurafx_plugin::{export_plugin, DataSet, Filter, Param, Params};

/// Scale the points of the input about a centre.
fn scale_points(inputs: &[DataSet], p: &Params) -> Result<DataSet, String> {
    let mut d = inputs.first().cloned().ok_or("no input")?;
    let f = p.f64("factor", 2.0);
    let c = p.vec3("center", [0.0; 3]);
    match &mut d {
        DataSet::PolyData(pd) => scale(&mut pd.points, f, c),
        DataSet::UnstructuredGrid(ug) => scale(&mut ug.points, f, c),
        DataSet::StructuredGrid(sg) => scale(&mut sg.points, f, c),
        _ => return Err("Scale Points takes a surface, an unstructured grid or a structured grid".into()),
    }
    Ok(d)
}

fn scale(pts: &mut Points, f: f64, c: [f64; 3]) {
    for i in 0..pts.len() {
        let q = pts.get(i);
        pts.set(i, [c[0] + f * (q[0] - c[0]), c[1] + f * (q[1] - c[1]), c[2] + f * (q[2] - c[2])]);
    }
}

/// A spiral of points joined by a line, with its angle as a point array.
fn spiral(_: &[DataSet], p: &Params) -> Result<DataSet, String> {
    let n = p.i64("points", 200).clamp(2, 1_000_000) as usize;
    let turns = p.f64("turns", 3.0);
    let mut pd = PolyData::default();
    let mut angle = Vec::with_capacity(n);
    for k in 0..n {
        let t = k as f64 / (n - 1) as f64;
        let a = t * turns * std::f64::consts::TAU;
        pd.points.push([t * a.cos(), t * a.sin(), t]);
        angle.push(a);
    }
    let ids: Vec<u32> = (0..n as u32).collect();
    pd.lines.push(CellType::PolyLine, &ids);
    pd.point_data.insert(Array::scalars_f64("Angle", angle));
    Ok(DataSet::PolyData(pd))
}

export_plugin! {
    name: "Example Rust plugin",
    filters: [
        Filter::new("scale_points", "Scale Points", scale_points)
            .describe("Scale the points about a centre (an example of a Rust plugin).")
            .keywords(&["scale", "grow", "shrink"])
            .param(Param::float("factor", "Factor", 2.0).range(0.0, 100.0))
            .param(Param::vec3("center", "Centre", [0.0, 0.0, 0.0])),
        Filter::source("spiral", "Spiral", spiral)
            .describe("A spiral of points joined by a line (an example of a Rust plugin source).")
            .param(Param::int("points", "Points", 200).range(2.0, 1e6))
            .param(Param::float("turns", "Turns", 3.0)),
    ],
}
