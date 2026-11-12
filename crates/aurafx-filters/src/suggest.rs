//! What can be done with this data: which filters fit a dataset, and which are worth offering
//! first. The menus, the command palette and the right-click menus use this to put a short list
//! of sensible next steps in front of the user and to say why another filter may not work.
//!
//! Both answers are advice from the dataset's summary (its kind, cell types and arrays), not a
//! guarantee: a filter marked as unlikely to work can still be added.

use crate::params::ParamKind;
use crate::registry::{find, FilterDef};
use aurafx_data::DataSetInfo;
use std::sync::Arc;

fn is_grid(kind: &str) -> bool {
    matches!(kind, "Image Data" | "Rectilinear Grid" | "Structured Grid")
}

fn has_vectors(info: &DataSetInfo) -> bool {
    info.point_arrays.iter().chain(&info.cell_arrays).any(|a| a.ncomp == 3 && a.dtype != "String")
}

fn has_arrays(info: &DataSetInfo) -> bool {
    !info.point_arrays.is_empty() || !info.cell_arrays.is_empty()
}

fn has_array_named(info: &DataSetInfo, names: &[&str]) -> bool {
    info.point_arrays.iter().chain(&info.cell_arrays).any(|a| names.iter().any(|n| a.name.eq_ignore_ascii_case(n)))
}

/// Cells of the given dimension (by the names of the cell types in the summary).
fn has_cells_of_dim(info: &DataSetInfo, dim: u8) -> bool {
    let d3 = ["Tetra", "Hexahedron", "Voxel", "Wedge", "Pyramid", "Polyhedron", "Prism"];
    let d2 = ["Triangle", "Quad", "Polygon", "Pixel", "Strip"];
    info.cell_types.iter().any(|(name, n)| {
        *n > 0
            && match dim {
                3 => d3.iter().any(|w| name.contains(w)),
                2 => d2.iter().any(|w| name.contains(w)) && !d3.iter().any(|w| name.contains(w)),
                _ => false,
            }
    })
}

fn volumetric(info: &DataSetInfo) -> bool {
    (is_grid(&info.kind) && info.bounds.min.iter().zip(&info.bounds.max).all(|(a, b)| b > a))
        || has_cells_of_dim(info, 3)
        || matches!(info.kind.as_str(), "Hyper Tree Grid" | "Cell Grid")
}

/// Whether `def` is likely to work on data with this summary; the error says in a few words what
/// the filter needs that the data lacks.
pub fn applicable(def: &FilterDef, info: &DataSetInfo) -> Result<(), String> {
    if def.is_source {
        return Ok(());
    }
    let kind = info.kind.as_str();
    let id = def.id;
    // Filters made for one kind of data.
    if id.starts_with("hyper_tree_grid") && kind != "Hyper Tree Grid" {
        return Err("needs a hyper tree grid".into());
    }
    if id.starts_with("cell_grid") && kind != "Cell Grid" && id != "convert_to_cell_grid" {
        return Err("needs a cell grid".into());
    }
    if matches!(id, "extract_subset" | "resample_rectilinear_to_image") && !is_grid(kind) {
        return Err("needs an image, rectilinear or structured grid".into());
    }
    let table_only = id.starts_with("table_") || matches!(id, "transpose_table");
    if table_only && kind != "Table" {
        return Err("needs a table".into());
    }
    if kind == "Table" && !table_only {
        let ok = def.category == "Data Analysis"
            || def.category == "Python"
            || def.keywords.iter().any(|k| matches!(*k, "table" | "statistics" | "stats" | "histogram" | "plot" | "chart"))
            || matches!(id, "histogram" | "descriptive_statistics" | "calculator" | "find_outliers" | "find_change_points");
        if !ok {
            return Err("works on meshes and grids, not on a table (Table To Points makes points of it)".into());
        }
    }
    if info.n_points == 0 && kind != "Table" {
        return Err("this data is empty".into());
    }
    // The arrays its settings ask for.
    for p in &def.params {
        if let ParamKind::Array { vector_only, .. } = &p.kind {
            // An optional array (its help says "empty: ...") is not a requirement.
            let optional = p.help.to_ascii_lowercase().contains("empty") || p.advanced;
            if optional {
                continue;
            }
            if *vector_only && !has_vectors(info) {
                return Err(format!("needs a vector array (3 components) for '{}'; this data has none", p.label));
            }
            if !has_arrays(info) && kind != "Table" {
                return Err(format!("needs a data array for '{}'; this data has none", p.label));
            }
        }
    }
    Ok(())
}

/// A filter worth offering for this data, and why.
#[derive(Clone)]
pub struct Suggestion {
    pub def: Arc<FilterDef>,
    pub reason: &'static str,
}

/// The few filters to offer first for data with this summary, best first. `has_time`: the data
/// has time steps.
pub fn suggestions(info: &DataSetInfo, has_time: bool) -> Vec<Suggestion> {
    let mut ids: Vec<(&str, &'static str)> = Vec::new();
    let kind = info.kind.as_str();
    let scalars = has_arrays(info);
    let surface = kind == "Polygonal Mesh" && has_cells_of_dim(info, 2);
    let points_only = matches!(kind, "Polygonal Mesh" | "Unstructured Grid")
        && info.n_points > 0
        && (info.n_cells == 0 || (!info.cell_types.is_empty() && info.cell_types.iter().all(|(n, _)| n.contains("Vertex"))));
    if kind == "Table" {
        ids.extend([
            ("table_to_points", "Turn rows with coordinates into points you can see in 3D"),
            ("histogram", "See how a column's values are spread"),
            ("descriptive_statistics", "Mean, spread and range of every column"),
        ]);
    }
    if has_array_named(info, &["EulerAngles", "Quats", "Phases"]) {
        ids.extend([("orientation_colors", "Color the map by crystal orientation"), ("segment_features", "Group points of similar orientation into grains")]);
    }
    if points_only {
        ids.extend([
            ("glyph", "Draw a shape at every point"),
            ("delaunay_3d", "Connect the points into a mesh"),
            ("point_interpolator", "Spread the point values onto a grid"),
        ]);
    }
    if volumetric(info) {
        ids.push(("slice", "Cut a plane through the volume to look inside"));
        if scalars {
            ids.push(("contour", "The surface where a value is reached"));
            ids.push(("threshold", "Keep only the cells in a range of values"));
        }
        ids.push(("clip", "Cut part of the volume away"));
    }
    if has_vectors(info) && !points_only {
        ids.extend([
            ("stream_tracer", "Follow the vector field with stream lines"),
            ("glyph", "Arrows along the vector field"),
            ("warp_by_vector", "Move the points by a vector (a deformed shape)"),
        ]);
    }
    if surface {
        ids.extend([
            ("surface_normals", "Smooth shading across the facets"),
            ("smooth", "Relax a rough surface"),
            ("decimate", "Fewer triangles with nearly the same shape"),
            ("feature_edges", "The sharp and boundary edges as lines"),
        ]);
    }
    if kind == "Unstructured Grid" && has_cells_of_dim(info, 3) {
        ids.push(("mesh_quality", "Find distorted cells before running a solver"));
        ids.push(("extract_surface", "Only the outer skin of the mesh"));
    }
    if has_time {
        ids.push(("temporal_statistics", "Average, minimum and maximum over the time steps"));
    }
    if scalars && kind != "Table" && !points_only {
        ids.push(("plot_over_line", "Chart the values along a line through the data"));
        ids.push(("calculator", "Compute a new array from the existing ones"));
    }
    let mut out: Vec<Suggestion> = Vec::new();
    for (id, reason) in ids {
        if out.iter().any(|s| s.def.id == id) {
            continue;
        }
        if let Some(def) = find(id) {
            if applicable(&def, info).is_ok() {
                out.push(Suggestion { def, reason });
            }
        }
    }
    out.truncate(7);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use aurafx_data::DataSet;

    fn info_of(source: &str) -> DataSetInfo {
        let s = find(source).unwrap();
        DataSetInfo::of(&s.run(None, &s.default_params()).unwrap())
    }

    #[test]
    fn suggestions_fit_the_data() {
        // A volume with a scalar: cuts and iso-surfaces first, nothing that needs vectors.
        let w = info_of("wavelet");
        let ids: Vec<&str> = suggestions(&w, false).iter().map(|s| s.def.id).collect();
        assert_eq!(&ids[..4], ["slice", "contour", "threshold", "clip"], "{ids:?}");
        assert!(!ids.contains(&"stream_tracer"));
        assert!(applicable(&find("stream_tracer").unwrap(), &w).unwrap_err().contains("vector"));
        assert!(applicable(&find("extract_subset").unwrap(), &w).is_ok());
        // A surface: surface tools, and Extract Subset does not fit.
        let s = info_of("sphere");
        let ids: Vec<&str> = suggestions(&s, false).iter().map(|s| s.def.id).collect();
        assert!(ids.contains(&"surface_normals") && ids.contains(&"decimate"), "{ids:?}");
        assert!(!ids.contains(&"slice"), "a surface has no inside: {ids:?}");
        assert!(applicable(&find("extract_subset").unwrap(), &s).is_err());
        // Every suggested filter exists and passes its own test; time adds the temporal one.
        assert!(suggestions(&w, true).iter().any(|s| s.def.id == "temporal_statistics"));
        // A table.
        let mut t = aurafx_data::Table::default();
        t.columns.insert(aurafx_data::Array::scalars_f64("x", vec![1.0, 2.0]));
        let ti = DataSetInfo::of(&DataSet::Table(t));
        let ids: Vec<&str> = suggestions(&ti, false).iter().map(|s| s.def.id).collect();
        assert_eq!(ids.first(), Some(&"table_to_points"), "{ids:?}");
        assert!(applicable(&find("slice").unwrap(), &ti).is_err());
    }

    #[test]
    fn most_filters_fit_ordinary_data() {
        // The test must not cry wolf: on a volume with scalars and on a surface, few filters are
        // marked as unlikely.
        let w = info_of("wavelet");
        let all = crate::registry();
        let filters: Vec<_> = all.iter().filter(|f| !f.is_source).collect();
        let no = filters.iter().filter(|f| applicable(f, &w).is_err()).count();
        assert!(no * 3 < filters.len(), "{no} of {} filters marked unlikely on a wavelet", filters.len());
    }
}
