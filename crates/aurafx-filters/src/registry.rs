//! Filter and source registry: one place that describes every operation with a name,
//! category, keywords for search, a typed parameter schema and an `apply` function.

use crate::params::*;
use anyhow::{anyhow, Result};
use aurafx_data::*;
use std::sync::{Arc, OnceLock};

pub type ApplyFn = fn(Option<&DataSet>, &Params) -> Result<DataSet>;

pub struct FilterDef {
    /// Stable snake_case id used by Python, state files and the AI tools.
    pub id: &'static str,
    pub label: &'static str,
    pub category: &'static str,
    pub description: &'static str,
    pub keywords: &'static [&'static str],
    /// `true` for sources (no input).
    pub is_source: bool,
    pub params: Vec<ParamSpec>,
    pub apply: ApplyFn,
}

impl std::fmt::Debug for FilterDef {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "FilterDef({})", self.id)
    }
}

impl FilterDef {
    pub fn default_params(&self) -> Params {
        Params::from_specs(&self.params)
    }
    pub fn run(&self, input: Option<&DataSet>, params: &Params) -> Result<DataSet> {
        if !self.is_source && input.is_none() {
            return Err(anyhow!("{} needs an input", self.label));
        }
        (self.apply)(input, params)
    }
    /// Score for fuzzy search (higher is better, 0 means no match).
    pub fn search_score(&self, query: &str) -> u32 {
        let q = query.to_lowercase();
        if q.is_empty() {
            return 1;
        }
        let label = self.label.to_lowercase();
        if label == q {
            return 1000;
        }
        if label.starts_with(&q) {
            return 500;
        }
        if label.contains(&q) || self.id.contains(&q) {
            return 300;
        }
        if self.keywords.iter().any(|k| k.contains(&q)) {
            return 200;
        }
        if self.description.to_lowercase().contains(&q) {
            return 100;
        }
        // Subsequence match on the label.
        let mut it = label.chars();
        if q.chars().all(|c| it.any(|l| l == c)) {
            return 50;
        }
        0
    }
}

fn arr_param(key: &str, label: &str) -> ParamSpec {
    ParamSpec::new(key, label, ParamKind::Array { scalar_only: false, vector_only: false }, ParamValue::Array { assoc: Association::Points, name: String::new() })
}
fn vec_param(key: &str, label: &str) -> ParamSpec {
    ParamSpec::new(key, label, ParamKind::Array { scalar_only: false, vector_only: true }, ParamValue::Array { assoc: Association::Points, name: String::new() })
}
fn f(key: &str, label: &str, default: f64, min: f64, max: f64) -> ParamSpec {
    ParamSpec::new(key, label, ParamKind::Float { min, max, log: false }, ParamValue::Float(default))
}
fn i(key: &str, label: &str, default: i64, min: i64, max: i64) -> ParamSpec {
    ParamSpec::new(key, label, ParamKind::Int { min, max }, ParamValue::Int(default))
}
fn v3(key: &str, label: &str, default: [f64; 3]) -> ParamSpec {
    ParamSpec::new(key, label, ParamKind::Vec3, ParamValue::Vec3(default))
}
fn b(key: &str, label: &str, default: bool) -> ParamSpec {
    ParamSpec::new(key, label, ParamKind::Bool, ParamValue::Bool(default))
}
fn choice(key: &str, label: &str, opts: &[&str], default: &str) -> ParamSpec {
    ParamSpec::new(key, label, ParamKind::Choice(opts.iter().map(|s| s.to_string()).collect()), ParamValue::Str(default.into()))
}
fn s(key: &str, label: &str, default: &str) -> ParamSpec {
    ParamSpec::new(key, label, ParamKind::Str, ParamValue::Str(default.into()))
}

/// Component selection: "" or "Magnitude" -> None, "X"/"0" -> Some(0) ...
fn comp(p: &Params) -> Option<usize> {
    match p.str("component").as_str() {
        "X" | "x" | "0" => Some(0),
        "Y" | "y" | "1" => Some(1),
        "Z" | "z" | "2" => Some(2),
        s => s.parse().ok(),
    }
}

/// Pick a sensible default array when the user did not choose one.
pub fn default_array(ds: &DataSet, want_vector: bool) -> Option<(Association, String)> {
    let pick = |a: &Attributes| -> Option<String> {
        if want_vector {
            return a.vectors().map(|x| x.name.clone());
        }
        if let Some(s) = a.active(AttributeKind::Scalars) {
            return Some(s.name.clone());
        }
        // Skip geometric helper arrays and constant arrays; prefer 1-component data.
        let skip: Vec<&str> = [AttributeKind::Normals, AttributeKind::TCoords, AttributeKind::GlobalIds, AttributeKind::PedigreeIds].iter().filter_map(|k| a.active.get(k).map(|s| s.as_str())).collect();
        let useful = |x: &&Array| {
            x.scalar_type() != ScalarType::String && !skip.contains(&x.name.as_str()) && !matches!(x.name.as_str(), "Normals" | "TextureCoordinates" | "vtkOriginalPointIds" | "vtkOriginalCellIds" | "vtkGhostType") && {
                let (lo, hi) = x.range(None);
                hi > lo
            }
        };
        a.iter().filter(useful).find(|x| x.ncomp == 1).or_else(|| a.iter().find(useful)).map(|x| x.name.clone())
    };
    let ds_for = match ds {
        DataSet::MultiBlock(m) => m.leaves().into_iter().next().map(|(_, d)| d),
        _ => Some(ds),
    }?;
    if let Some(n) = ds_for.point_data().and_then(pick) {
        return Some((Association::Points, n));
    }
    ds_for.cell_data().and_then(pick).map(|n| (Association::Cells, n))
}

fn array_or_default(ds: &DataSet, p: &Params, key: &str, vector: bool) -> Result<(Association, String)> {
    p.array(key).or_else(|| default_array(ds, vector)).ok_or_else(|| anyhow!("no suitable array for '{key}'"))
}

/// Apply a per-leaf operation over multi-block inputs.
fn per_leaf(ds: &DataSet, f: &dyn Fn(&DataSet) -> Result<DataSet>) -> Result<DataSet> {
    match ds {
        DataSet::MultiBlock(mb) => {
            let mut out = MultiBlock { field_data: mb.field_data.clone(), ..Default::default() };
            for (name, b) in &mb.blocks {
                let r = match b {
                    Some(d) => Some(per_leaf(d, f)?),
                    None => None,
                };
                out.blocks.push((name.clone(), r));
            }
            Ok(DataSet::MultiBlock(out))
        }
        _ => f(ds),
    }
}

fn bounds_center(ds: &DataSet) -> [f64; 3] {
    let b = ds.bounds();
    if b.is_empty() { [0.0; 3] } else { b.center() }
}

fn build() -> Vec<FilterDef> {
    vec![
        // ---------------- Common ----------------
        FilterDef {
            id: "slice",
            label: "Slice",
            category: "Common",
            description: "Cut the data with a plane or sphere and show the values on the cut surface.",
            keywords: &["cut", "plane", "section", "cross section"],
            is_source: false,
            params: vec![
                choice("type", "Cut type", &["Plane", "Sphere"], "Plane"),
                v3("origin", "Origin", [f64::NAN; 3]).help("Leave unset to use the center of the data."),
                v3("normal", "Normal", [1.0, 0.0, 0.0]),
                f("radius", "Radius (sphere)", 1.0, 0.0, f64::INFINITY),
                ParamSpec::new("offsets", "Offsets", ParamKind::Floats, ParamValue::Floats(vec![0.0])).advanced().help("Additional parallel slices at these distances along the normal."),
            ],
            apply: |input, p| {
                let ds = input.unwrap();
                let origin = p.vec3("origin", [f64::NAN; 3]);
                let origin = if origin.iter().any(|v| v.is_nan()) { bounds_center(ds) } else { origin };
                let normal = crate::slice::normalize(p.vec3("normal", [1.0, 0.0, 0.0]));
                let radius = p.f64("radius", 1.0);
                let ty = p.str("type");
                let offsets = { let o = p.floats("offsets"); if o.is_empty() { vec![0.0] } else { o } };
                per_leaf(ds, &|d| {
                    if ty == "Sphere" {
                        return Ok(DataSet::PolyData(crate::slice::slice_sphere(d, origin, radius)?));
                    }
                    if offsets.len() == 1 {
                        let o = offsets[0];
                        let org = [origin[0] + o * normal[0], origin[1] + o * normal[1], origin[2] + o * normal[2]];
                        return Ok(DataSet::PolyData(crate::slice::slice_plane(d, org, normal)?));
                    }
                    let mut mb = MultiBlock::default();
                    for &o in &offsets {
                        let org = [origin[0] + o * normal[0], origin[1] + o * normal[1], origin[2] + o * normal[2]];
                        mb.blocks.push((format!("offset {o}"), Some(DataSet::PolyData(crate::slice::slice_plane(d, org, normal)?))));
                    }
                    Ok(DataSet::MultiBlock(mb))
                })
            },
        },
        FilterDef {
            id: "clip",
            label: "Clip",
            category: "Common",
            description: "Cut away part of the data with a plane, box, sphere or scalar value.",
            keywords: &["cut", "remove", "half", "crop", "cutaway"],
            is_source: false,
            params: vec![
                choice("type", "Clip type", &["Plane", "Box", "Sphere", "Scalar"], "Plane"),
                v3("origin", "Origin / center", [f64::NAN; 3]),
                v3("normal", "Normal", [1.0, 0.0, 0.0]),
                f("radius", "Radius (sphere)", 1.0, 0.0, f64::INFINITY),
                v3("box_min", "Box min", [f64::NAN; 3]),
                v3("box_max", "Box max", [f64::NAN; 3]),
                arr_param("array", "Scalar (for Scalar clip)"),
                s("component", "Component", ""),
                f("value", "Value (for Scalar clip)", 0.0, f64::NEG_INFINITY, f64::INFINITY),
                b("invert", "Invert", false),
            ],
            apply: |input, p| {
                let ds = input.unwrap();
                let origin = p.vec3("origin", [f64::NAN; 3]);
                let origin = if origin.iter().any(|v| v.is_nan()) { bounds_center(ds) } else { origin };
                let inv = p.bool("invert", false);
                let ty = p.str("type");
                per_leaf(ds, &|d| {
                    Ok(DataSet::UnstructuredGrid(match ty.as_str() {
                        "Sphere" => crate::clip::clip_sphere(d, origin, p.f64("radius", 1.0), inv)?,
                        "Box" => {
                            let bb = d.bounds();
                            let mut lo = p.vec3("box_min", [f64::NAN; 3]);
                            let mut hi = p.vec3("box_max", [f64::NAN; 3]);
                            if lo.iter().any(|v| v.is_nan()) || hi.iter().any(|v| v.is_nan()) {
                                let c = bb.center();
                                let s = bb.size();
                                lo = [c[0] - 0.25 * s[0], c[1] - 0.25 * s[1], c[2] - 0.25 * s[2]];
                                hi = [c[0] + 0.25 * s[0], c[1] + 0.25 * s[1], c[2] + 0.25 * s[2]];
                            }
                            crate::clip::clip_box(d, lo, hi, inv)?
                        }
                        "Scalar" => {
                            let (a, n) = array_or_default(d, p, "array", false)?;
                            crate::clip::clip_scalar(d, a, &n, comp(p), p.f64("value", 0.0), inv)?
                        }
                        _ => crate::clip::clip_plane(d, origin, p.vec3("normal", [1.0, 0.0, 0.0]), inv)?,
                    }))
                })
            },
        },
        FilterDef {
            id: "contour",
            label: "Contour",
            category: "Common",
            description: "Isosurfaces (3D) or isolines (2D) where a scalar equals given values.",
            keywords: &["isosurface", "isoline", "level set", "marching cubes", "iso"],
            is_source: false,
            params: vec![
                arr_param("array", "Contour by"),
                s("component", "Component", ""),
                ParamSpec::new("values", "Values", ParamKind::Floats, ParamValue::Floats(vec![])).help("Leave empty for the midpoint of the data range."),
                i("count", "Auto count (when values empty)", 1, 1, 100).advanced(),
            ],
            apply: |input, p| {
                let ds = input.unwrap();
                per_leaf(ds, &|d| {
                    let (a, n) = array_or_default(d, p, "array", false)?;
                    let mut values = p.floats("values");
                    if values.is_empty() {
                        let attrs = if a == Association::Points { d.point_data() } else { d.cell_data() };
                        let arr = attrs.and_then(|x| x.get(&n)).ok_or_else(|| anyhow!("no array {n}"))?;
                        let (lo, hi) = arr.range(comp(p));
                        let k = p.i64("count", 1).max(1) as usize;
                        values = (1..=k).map(|j| lo + (hi - lo) * j as f64 / (k + 1) as f64).collect();
                    }
                    Ok(DataSet::PolyData(crate::contour::contour(d, a, &n, comp(p), &values)?))
                })
            },
        },
        FilterDef {
            id: "threshold",
            label: "Threshold",
            category: "Common",
            description: "Keep only cells whose values fall within a range.",
            keywords: &["filter", "range", "between", "select", "cutoff"],
            is_source: false,
            params: vec![
                arr_param("array", "Scalars"),
                s("component", "Component", ""),
                f("lower", "Lower", f64::NAN, f64::NEG_INFINITY, f64::INFINITY),
                f("upper", "Upper", f64::NAN, f64::NEG_INFINITY, f64::INFINITY),
                choice("method", "Point rule", &["All points", "Any point"], "All points").advanced(),
                b("invert", "Invert", false),
            ],
            apply: |input, p| {
                let ds = input.unwrap();
                per_leaf(ds, &|d| {
                    let (a, n) = array_or_default(d, p, "array", false)?;
                    let attrs = if a == Association::Points { d.point_data() } else { d.cell_data() };
                    let arr = attrs.and_then(|x| x.get(&n)).ok_or_else(|| anyhow!("no array {n}"))?;
                    let (lo0, hi0) = arr.range(comp(p));
                    let mut lo = p.f64("lower", f64::NAN);
                    let mut hi = p.f64("upper", f64::NAN);
                    if lo.is_nan() { lo = lo0 + 0.25 * (hi0 - lo0); }
                    if hi.is_nan() { hi = hi0; }
                    let mode = if p.str("method") == "Any point" { crate::threshold::PointMode::Any } else { crate::threshold::PointMode::All };
                    Ok(DataSet::UnstructuredGrid(crate::threshold::threshold(d, a, &n, comp(p), lo, hi, mode, p.bool("invert", false))?))
                })
            },
        },
        FilterDef {
            id: "calculator",
            label: "Calculator",
            category: "Common",
            description: "Compute a new array from an expression of existing arrays and coordinates.",
            keywords: &["expression", "formula", "math", "compute", "derived"],
            is_source: false,
            params: vec![
                choice("attribute", "Attribute", &["Points", "Cells"], "Points"),
                ParamSpec::new("expression", "Expression", ParamKind::Expression, ParamValue::Str("coordsX".into())).help("e.g. mag(velocity), sqrt(x^2+y^2), if(T > 300, 1, 0)"),
                s("result", "Result name", "Result"),
            ],
            apply: |input, p| {
                let assoc = if p.str("attribute") == "Cells" { Association::Cells } else { Association::Points };
                let expr = p.str("expression");
                let name = { let n = p.str("result"); if n.is_empty() { "Result".to_string() } else { n } };
                per_leaf(input.unwrap(), &|d| crate::calculator::calculator(d, assoc, &expr, &name))
            },
        },
        FilterDef {
            id: "warp_by_vector",
            label: "Warp By Vector",
            category: "Common",
            description: "Displace points by a vector field (e.g. displacement) times a scale factor.",
            keywords: &["deform", "displacement", "displace", "deformed shape"],
            is_source: false,
            params: vec![vec_param("vectors", "Vectors"), f("scale", "Scale factor", 1.0, f64::NEG_INFINITY, f64::INFINITY)],
            apply: |input, p| {
                per_leaf(input.unwrap(), &|d| {
                    let (_, n) = array_or_default(d, p, "vectors", true)?;
                    crate::warp::warp_by_vector(d, &n, p.f64("scale", 1.0))
                })
            },
        },
        FilterDef {
            id: "warp_by_scalar",
            label: "Warp By Scalar",
            category: "Common",
            description: "Displace points along a direction (or normals) by a scalar value.",
            keywords: &["height", "elevation", "carpet plot", "deform"],
            is_source: false,
            params: vec![arr_param("array", "Scalars"), f("scale", "Scale factor", 1.0, f64::NEG_INFINITY, f64::INFINITY), v3("normal", "Direction", [0.0, 0.0, 1.0]), b("use_normals", "Use point normals", false)],
            apply: |input, p| {
                per_leaf(input.unwrap(), &|d| {
                    let (_, n) = array_or_default(d, p, "array", false)?;
                    crate::warp::warp_by_scalar(d, &n, p.f64("scale", 1.0), p.vec3("normal", [0.0, 0.0, 1.0]), p.bool("use_normals", false))
                })
            },
        },
        FilterDef {
            id: "glyph",
            label: "Glyph",
            category: "Common",
            description: "Draw arrows, spheres or cubes at points, oriented and scaled by arrays.",
            keywords: &["arrows", "vectors", "quiver", "hedgehog", "markers"],
            is_source: false,
            params: vec![
                choice("shape", "Shape", &["Arrow", "Line", "Sphere", "Cube"], "Arrow"),
                vec_param("orient", "Orientation"),
                arr_param("scale_array", "Scale by"),
                f("scale", "Scale factor", f64::NAN, 0.0, f64::INFINITY).help("Leave unset for 5% of the data size."),
                i("max_glyphs", "Maximum glyphs", 5000, 1, 10_000_000),
                i("resolution", "Resolution", 12, 3, 64).advanced(),
            ],
            apply: |input, p| {
                per_leaf(input.unwrap(), &|d| {
                    let orient = p.array("orient").map(|(_, n)| n).or_else(|| default_array(d, true).filter(|(a, _)| *a == Association::Points).map(|(_, n)| n));
                    let scale_array = p.array("scale_array").map(|(_, n)| n);
                    let mut sf = p.f64("scale", f64::NAN);
                    if sf.is_nan() {
                        let diag = d.bounds().diagonal().max(1e-12);
                        let maxmag = orient.as_ref().and_then(|n| d.point_data().and_then(|a| a.get(n))).map(|a| a.range(None).1).filter(|v| *v > 0.0).unwrap_or(1.0);
                        sf = 0.05 * diag / if scale_array.is_some() || orient.is_some() { maxmag } else { 1.0 };
                    }
                    let opt = crate::glyph::GlyphOptions {
                        shape: crate::glyph::GlyphShape::parse(&p.str("shape")),
                        orient_array: orient,
                        scale_array,
                        scale_factor: sf,
                        resolution: p.i64("resolution", 12) as usize,
                        stride: 1,
                        max_glyphs: p.i64("max_glyphs", 5000) as usize,
                    };
                    Ok(DataSet::PolyData(crate::glyph::glyph(d, &opt)?))
                })
            },
        },
        // ---------------- Geometry ----------------
        FilterDef {
            id: "extract_surface",
            label: "Extract Surface",
            category: "Geometry",
            description: "Outer surface of a volume mesh as polygons.",
            keywords: &["skin", "boundary", "faces", "exterior"],
            is_source: false,
            params: vec![],
            apply: |input, _| Ok(DataSet::PolyData(crate::surface::extract_surface(input.unwrap())?)),
        },
        FilterDef {
            id: "outline",
            label: "Outline",
            category: "Geometry",
            description: "Bounding box of the data as lines.",
            keywords: &["bounding box", "bbox", "frame"],
            is_source: false,
            params: vec![],
            apply: |input, _| Ok(DataSet::PolyData(crate::outline::outline(input.unwrap()))),
        },
        FilterDef {
            id: "extract_edges",
            label: "Extract Edges",
            category: "Geometry",
            description: "All unique cell edges as lines.",
            keywords: &["wireframe", "edges", "mesh lines"],
            is_source: false,
            params: vec![],
            apply: |input, _| per_leaf(input.unwrap(), &|d| Ok(DataSet::PolyData(crate::edges::extract_edges(d)?))),
        },
        FilterDef {
            id: "feature_edges",
            label: "Feature Edges",
            category: "Geometry",
            description: "Boundary and sharp edges of a surface.",
            keywords: &["silhouette", "crease", "sharp", "boundary edges"],
            is_source: false,
            params: vec![f("angle", "Feature angle", 30.0, 0.0, 180.0), b("boundary", "Boundary edges", true), b("sharp", "Sharp edges", true)],
            apply: |input, p| {
                per_leaf(input.unwrap(), &|d| {
                    let surf = match d {
                        DataSet::PolyData(pd) => pd.clone(),
                        _ => crate::surface::extract_surface(d)?,
                    };
                    Ok(DataSet::PolyData(crate::edges::feature_edges(&surf, p.f64("angle", 30.0), p.bool("boundary", true), p.bool("sharp", true))?))
                })
            },
        },
        FilterDef {
            id: "transform",
            label: "Transform",
            category: "Geometry",
            description: "Translate, rotate and scale the data.",
            keywords: &["move", "rotate", "scale", "translate", "affine"],
            is_source: false,
            params: vec![v3("translate", "Translate", [0.0; 3]), v3("rotate", "Rotate (deg)", [0.0; 3]), v3("scale", "Scale", [1.0; 3])],
            apply: |input, p| per_leaf(input.unwrap(), &|d| crate::transform::transform(d, p.vec3("translate", [0.0; 3]), p.vec3("rotate", [0.0; 3]), p.vec3("scale", [1.0; 3]))),
        },
        FilterDef {
            id: "cell_centers",
            label: "Cell Centers",
            category: "Geometry",
            description: "A point at the center of every cell, carrying the cell data.",
            keywords: &["centroids", "centers"],
            is_source: false,
            params: vec![],
            apply: |input, _| per_leaf(input.unwrap(), &|d| Ok(DataSet::PolyData(crate::attributes::cell_centers(d)?))),
        },
        FilterDef {
            id: "merge_blocks",
            label: "Merge Blocks",
            category: "Geometry",
            description: "Combine all blocks of a multi-block dataset into one mesh.",
            keywords: &["append", "combine", "flatten"],
            is_source: false,
            params: vec![],
            apply: |input, _| {
                let ds = input.unwrap();
                match ds {
                    DataSet::MultiBlock(mb) => {
                        let mut out = UnstructuredGrid::default();
                        let mut first = true;
                        for (_, leaf) in mb.leaves() {
                            let Some(ug) = leaf.to_unstructured() else { continue };
                            if first {
                                out = ug;
                                first = false;
                            } else {
                                let off = out.points.len() as u32;
                                out.points.append(&ug.points);
                                out.cells.append(&ug.cells, off);
                                let names: Vec<String> = out.point_data.arrays.keys().cloned().collect();
                                for n in names {
                                    match ug.point_data.get(&n) {
                                        Some(a) => { let _ = out.point_data.get_mut(&n).unwrap().data.append(&a.data); }
                                        None => { out.point_data.remove(&n); }
                                    }
                                }
                                let names: Vec<String> = out.cell_data.arrays.keys().cloned().collect();
                                for n in names {
                                    match ug.cell_data.get(&n) {
                                        Some(a) => { let _ = out.cell_data.get_mut(&n).unwrap().data.append(&a.data); }
                                        None => { out.cell_data.remove(&n); }
                                    }
                                }
                            }
                        }
                        Ok(DataSet::UnstructuredGrid(out))
                    }
                    other => Ok(other.clone()),
                }
            },
        },
        // ---------------- Data ----------------
        FilterDef {
            id: "cell_data_to_point_data",
            label: "Cell Data to Point Data",
            category: "Data",
            description: "Average cell values onto the points for smooth coloring.",
            keywords: &["interpolate", "smooth", "nodal"],
            is_source: false,
            params: vec![],
            apply: |input, _| per_leaf(input.unwrap(), &|d| crate::attributes::cell_to_point(d, &[])),
        },
        FilterDef {
            id: "point_data_to_cell_data",
            label: "Point Data to Cell Data",
            category: "Data",
            description: "Average point values onto the cells.",
            keywords: &["elemental", "average"],
            is_source: false,
            params: vec![],
            apply: |input, _| per_leaf(input.unwrap(), &|d| crate::attributes::point_to_cell(d, &[])),
        },
        // ---------------- Sources ----------------
        FilterDef {
            id: "sphere",
            label: "Sphere",
            category: "Sources",
            description: "A polygonal sphere.",
            keywords: &["ball", "shape"],
            is_source: true,
            params: vec![v3("center", "Center", [0.0; 3]), f("radius", "Radius", 0.5, 0.0, f64::INFINITY), i("theta", "Theta resolution", 32, 3, 1024), i("phi", "Phi resolution", 32, 3, 1024)],
            apply: |_, p| Ok(DataSet::PolyData(crate::sources::sphere(p.vec3("center", [0.0; 3]), p.f64("radius", 0.5), p.i64("theta", 32) as usize, p.i64("phi", 32) as usize))),
        },
        FilterDef {
            id: "cone",
            label: "Cone",
            category: "Sources",
            description: "A polygonal cone.",
            keywords: &["shape"],
            is_source: true,
            params: vec![v3("center", "Center", [0.0; 3]), v3("direction", "Direction", [1.0, 0.0, 0.0]), f("height", "Height", 1.0, 0.0, f64::INFINITY), f("radius", "Radius", 0.5, 0.0, f64::INFINITY), i("resolution", "Resolution", 32, 3, 1024)],
            apply: |_, p| Ok(DataSet::PolyData(crate::sources::cone(p.vec3("center", [0.0; 3]), p.f64("height", 1.0), p.f64("radius", 0.5), p.i64("resolution", 32) as usize, p.vec3("direction", [1.0, 0.0, 0.0])))),
        },
        FilterDef {
            id: "cylinder",
            label: "Cylinder",
            category: "Sources",
            description: "A capped cylinder along Y.",
            keywords: &["tube", "shape"],
            is_source: true,
            params: vec![v3("center", "Center", [0.0; 3]), f("height", "Height", 1.0, 0.0, f64::INFINITY), f("radius", "Radius", 0.5, 0.0, f64::INFINITY), i("resolution", "Resolution", 32, 3, 1024)],
            apply: |_, p| Ok(DataSet::PolyData(crate::sources::cylinder(p.vec3("center", [0.0; 3]), p.f64("height", 1.0), p.f64("radius", 0.5), p.i64("resolution", 32) as usize))),
        },
        FilterDef {
            id: "box",
            label: "Box",
            category: "Sources",
            description: "An axis-aligned box.",
            keywords: &["cube", "shape"],
            is_source: true,
            params: vec![v3("center", "Center", [0.0; 3]), v3("size", "Size", [1.0; 3])],
            apply: |_, p| Ok(DataSet::PolyData(crate::sources::cube(p.vec3("center", [0.0; 3]), p.vec3("size", [1.0; 3])))),
        },
        FilterDef {
            id: "plane",
            label: "Plane",
            category: "Sources",
            description: "A subdivided rectangle.",
            keywords: &["quad", "grid", "shape"],
            is_source: true,
            params: vec![v3("origin", "Origin", [-0.5, -0.5, 0.0]), v3("point1", "Point 1", [0.5, -0.5, 0.0]), v3("point2", "Point 2", [-0.5, 0.5, 0.0]), i("xres", "X resolution", 10, 1, 4096), i("yres", "Y resolution", 10, 1, 4096)],
            apply: |_, p| Ok(DataSet::PolyData(crate::sources::plane(p.vec3("origin", [-0.5, -0.5, 0.0]), p.vec3("point1", [0.5, -0.5, 0.0]), p.vec3("point2", [-0.5, 0.5, 0.0]), p.i64("xres", 10) as usize, p.i64("yres", 10) as usize))),
        },
        FilterDef {
            id: "line",
            label: "Line",
            category: "Sources",
            description: "A straight polyline.",
            keywords: &["segment"],
            is_source: true,
            params: vec![v3("point1", "Point 1", [-0.5, 0.0, 0.0]), v3("point2", "Point 2", [0.5, 0.0, 0.0]), i("resolution", "Resolution", 10, 1, 1_000_000)],
            apply: |_, p| Ok(DataSet::PolyData(crate::sources::line(p.vec3("point1", [-0.5, 0.0, 0.0]), p.vec3("point2", [0.5, 0.0, 0.0]), p.i64("resolution", 10) as usize))),
        },
        FilterDef {
            id: "point_source",
            label: "Point Cloud",
            category: "Sources",
            description: "Random points inside a sphere.",
            keywords: &["random", "points", "seeds"],
            is_source: true,
            params: vec![v3("center", "Center", [0.0; 3]), f("radius", "Radius", 0.5, 0.0, f64::INFINITY), i("count", "Number of points", 1000, 1, 100_000_000), i("seed", "Random seed", 1, 0, i64::MAX).advanced()],
            apply: |_, p| Ok(DataSet::PolyData(crate::sources::point_source(p.vec3("center", [0.0; 3]), p.f64("radius", 0.5), p.i64("count", 1000) as usize, p.i64("seed", 1) as u64))),
        },
        FilterDef {
            id: "wavelet",
            label: "Wavelet",
            category: "Sources",
            description: "Synthetic 3D scalar volume, handy for trying filters.",
            keywords: &["test data", "volume", "example", "rtdata"],
            is_source: true,
            params: vec![i("half_size", "Half size", 10, 1, 512), f("maximum", "Maximum", 255.0, f64::NEG_INFINITY, f64::INFINITY), f("std_dev", "Std. deviation", 0.5, 0.0, f64::INFINITY)],
            apply: |_, p| {
                let h = p.i64("half_size", 10);
                Ok(DataSet::ImageData(crate::sources::wavelet([-h, h, -h, h, -h, h], p.f64("maximum", 255.0), p.f64("std_dev", 0.5))))
            },
        },
    ]
}

static REGISTRY: OnceLock<Vec<Arc<FilterDef>>> = OnceLock::new();

pub fn registry() -> &'static [Arc<FilterDef>] {
    REGISTRY.get_or_init(|| build().into_iter().map(Arc::new).collect())
}

pub fn find(id: &str) -> Option<Arc<FilterDef>> {
    let idl = id.to_ascii_lowercase().replace([' ', '-'], "_");
    registry().iter().find(|f| f.id == idl || f.label.eq_ignore_ascii_case(id)).cloned()
}

/// Search filters and sources, best match first.
pub fn search(query: &str) -> Vec<Arc<FilterDef>> {
    let mut v: Vec<(u32, Arc<FilterDef>)> = registry().iter().map(|f| (f.search_score(query), f.clone())).filter(|(s, _)| *s > 0).collect();
    v.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.label.cmp(b.1.label)));
    v.into_iter().map(|(_, f)| f).collect()
}
