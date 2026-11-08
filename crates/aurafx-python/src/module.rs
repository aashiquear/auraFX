//! The `aurafx` Python module.

use crate::session;
use aurafx_data::{ArrayData, DataSet};
use aurafx_filters::Association;
use aurafx_pipeline::session::CameraRequest;
use aurafx_pipeline::{ColorBy, NodeId, Representation};
use numpy::{PyArray1, PyArrayMethods};
use pyo3::exceptions::{PyKeyError, PyRuntimeError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::{PyDict, PyList, PyModule, PyTuple};

fn err<E: std::fmt::Display>(e: E) -> PyErr {
    PyRuntimeError::new_err(format!("{e:#}"))
}

/// Convert a Python value to a filter parameter via JSON.
fn to_json(v: &Bound<'_, PyAny>) -> PyResult<serde_json::Value> {
    if v.is_none() {
        return Ok(serde_json::Value::Null);
    }
    if let Ok(b) = v.extract::<bool>() {
        return Ok(serde_json::Value::Bool(b));
    }
    if let Ok(i) = v.extract::<i64>() {
        return Ok(serde_json::Value::from(i));
    }
    if let Ok(f) = v.extract::<f64>() {
        return Ok(serde_json::Value::from(f));
    }
    if let Ok(s) = v.extract::<String>() {
        return Ok(serde_json::Value::String(s));
    }
    if let Ok(seq) = v.extract::<Vec<f64>>() {
        return Ok(serde_json::Value::Array(seq.into_iter().map(serde_json::Value::from).collect()));
    }
    if let Ok(d) = v.cast::<PyDict>() {
        let mut m = serde_json::Map::new();
        for (k, val) in d.iter() {
            m.insert(k.extract::<String>()?, to_json(&val)?);
        }
        return Ok(serde_json::Value::Object(m));
    }
    // Nested sequences (lists of points, tuples of mixed values).
    if !v.is_instance_of::<pyo3::types::PyString>() {
        if let Ok(it) = v.try_iter() {
            return Ok(serde_json::Value::Array(it.map(|x| x.and_then(|x| to_json(&x))).collect::<PyResult<Vec<_>>>()?));
        }
    }
    Err(PyValueError::new_err(format!("unsupported parameter value {}", v.repr()?)))
}

fn apply_kwargs(id: NodeId, kwargs: Option<&Bound<'_, PyDict>>) -> PyResult<()> {
    let Some(kw) = kwargs else { return Ok(()) };
    let mut s = session().lock();
    for (k, v) in kw.iter() {
        let key: String = k.extract()?;
        let val = to_json(&v)?;
        // A filter's own parameter wins over a display property of the same name.
        let is_param = s.nodes.get(&id).is_some_and(|n| n.params.get(&key).is_some());
        match key.as_str() {
            "name" => {
                if let Some(n) = val.as_str() {
                    s.rename(id, n.to_string());
                }
            }
            "visible" | "show" => s.set_visible(id, val.as_bool().unwrap_or(true)),
            "fetch_full" | "all_data" => s.set_fetch_full(id, val.as_bool().unwrap_or(true)),
            "draw_on" => {
                let v = match val.as_str().unwrap_or("auto").to_lowercase().as_str() {
                    "server" | "remote" => aurafx_pipeline::DrawOn::Server,
                    "local" | "here" | "client" => aurafx_pipeline::DrawOn::Local,
                    _ => aurafx_pipeline::DrawOn::Auto,
                };
                s.set_display(id, |d| d.draw_on = v);
            }
            "color_by" => match val.as_str() {
                Some(n) => {
                    let cb = resolve_color_by(&s, id, n, None)?;
                    s.color_by(id, Some(cb));
                }
                None if val.is_null() => s.color_by(id, None),
                None => return Err(PyValueError::new_err("color_by takes an array name or None")),
            },
            "component" if !is_param => {
                let c =
                    if val.is_null() { None } else { Some(val.as_u64().ok_or_else(|| PyValueError::new_err("component takes an index or None"))? as usize) };
                s.set_display(id, |d| {
                    if let Some(cb) = d.color_by.as_mut() {
                        cb.component = c;
                    }
                });
            }
            "range" if !is_param => {
                let r = if val.is_null() {
                    None
                } else {
                    let v: [f64; 2] = serde_json::from_value(val.clone()).map_err(|_| PyValueError::new_err("range takes (min, max) or None"))?;
                    Some((v[0], v[1]))
                };
                s.set_display(id, |d| d.range = r);
            }
            "solid_color" | "color" | "edge_color" if !is_param => {
                let c: [f32; 3] = serde_json::from_value(val.clone()).map_err(|_| PyValueError::new_err(format!("{key} takes (r, g, b) from 0 to 1")))?;
                let edge = key == "edge_color";
                s.set_display(id, |d| if edge { d.edge_color = c } else { d.solid_color = c });
            }
            "line_width" if !is_param => {
                let v = val.as_f64().ok_or_else(|| PyValueError::new_err("line_width takes a number"))? as f32;
                s.set_display(id, |d| d.line_width = v.max(0.1));
            }
            "points_as_spheres" | "log_scale" | "lighting" | "scalar_bar" | "show_scalar_bar" | "direct_colors" if !is_param => {
                let v = val.as_bool().ok_or_else(|| PyValueError::new_err(format!("{key} takes True or False")))?;
                let k = key.clone();
                s.set_display(id, |d| match k.as_str() {
                    "points_as_spheres" => d.points_as_spheres = v,
                    "log_scale" => d.log_scale = v,
                    "lighting" => d.lighting = v,
                    "direct_colors" => d.direct_colors = v,
                    _ => d.show_scalar_bar = v,
                });
            }
            "colormap" | "cmap" => {
                let name = val.as_str().unwrap_or("Viridis").to_string();
                // User presets are copied into the object so state files stay self-contained.
                let user = aurafx_pipeline::colormap::user().into_iter().find(|m| m.name.eq_ignore_ascii_case(&name)).map(|m| m.stops);
                s.set_display(id, |d| {
                    d.colormap = name;
                    d.colormap_stops = user;
                });
            }
            "cmap_points" | "colormap_points" => {
                let pts: Vec<[f64; 4]> = serde_json::from_value(val.clone()).map_err(|_| PyValueError::new_err("cmap_points must be [[t, r, g, b], ...]"))?;
                let stops: Vec<(f32, [f32; 3])> = pts.iter().map(|p| (p[0] as f32, [p[1] as f32, p[2] as f32, p[3] as f32])).collect();
                s.set_display(id, |d| d.colormap_stops = if stops.len() >= 2 { Some(stops) } else { None });
            }
            "cmap_reverse" | "reverse_colormap" => {
                let v = val.as_bool().unwrap_or(true);
                s.set_display(id, |d| d.colormap_reverse = v);
            }
            "cmap_bands" | "bands" => {
                let v = val.as_u64().unwrap_or(0) as u32;
                s.set_display(id, |d| d.colormap_bands = v);
            }
            "nonlinear_subdivision" | "curved_subdivision" => {
                let v = val.as_u64().unwrap_or(1).min(6) as u32;
                s.set_display(id, |d| d.nonlinear_subdivision = v);
            }
            "opacity" => {
                let o = val.as_f64().unwrap_or(1.0) as f32;
                s.set_display(id, |d| d.opacity = o);
            }
            "representation" | "repr" => {
                let r = val.as_str().and_then(Representation::parse).ok_or_else(|| PyValueError::new_err("unknown representation"))?;
                s.set_display(id, |d| d.representation = r);
            }
            "volume_opacity" | "density" => {
                let v = val.as_f64().unwrap_or(1.0) as f32;
                s.set_display(id, |d| d.volume_opacity = v);
            }
            "metallic" | "roughness" | "specular" => {
                let v = val.as_f64().ok_or_else(|| PyValueError::new_err(format!("{key} must be a number")))? as f32;
                let k = key.to_string();
                s.set_display(id, |d| match k.as_str() {
                    "metallic" => d.metallic = v.clamp(0.0, 1.0),
                    "roughness" => d.roughness = v.clamp(0.02, 1.0),
                    _ => d.specular = v.max(0.0),
                });
            }
            "texture" => {
                let v = val.as_str().filter(|t| !t.trim().is_empty()).map(str::to_string);
                if let Some(t) = &v {
                    if !std::path::Path::new(t).is_file() {
                        return Err(PyValueError::new_err(format!("texture: no image file {t}")));
                    }
                }
                s.set_display(id, |d| d.texture = v);
            }
            "molecule_style" | "atom_scale" | "bond_radius" | "bond_orders" if !is_param => {
                let k = key.clone();
                let style = match k.as_str() {
                    "molecule_style" => Some(
                        val.as_str()
                            .and_then(aurafx_pipeline::MoleculeStyle::parse)
                            .ok_or_else(|| PyValueError::new_err("molecule_style: 'Ball and Stick', 'Van der Waals' or 'Liquorice'"))?,
                    ),
                    _ => None,
                };
                let x = val.as_f64().unwrap_or(0.0) as f32;
                let flag = val.as_bool().unwrap_or(true);
                s.set_display(id, |d| match k.as_str() {
                    "molecule_style" => d.molecule_style = style.unwrap(),
                    "atom_scale" => d.atom_scale = x.max(0.0),
                    "bond_radius" => d.bond_radius = x.max(0.0),
                    _ => d.bond_orders = flag,
                });
            }
            "lic_vectors" => {
                let v = val.as_str().map(str::to_string);
                s.set_display(id, |d| d.lic_vectors = v);
            }
            "lic_strength" => {
                let v = val.as_f64().unwrap_or(0.8).clamp(0.0, 1.0) as f32;
                s.set_display(id, |d| d.lic_strength = v);
            }
            "lic_length" => {
                let v = val.as_u64().unwrap_or(20).clamp(1, 200) as u32;
                s.set_display(id, |d| d.lic_length = v);
            }
            "volume_threshold" => {
                let v = val.as_f64().unwrap_or(0.0) as f32;
                s.set_display(id, |d| d.volume_threshold = v);
            }
            "axes_grid" | "grid" => {
                let v = val.as_bool().unwrap_or(true);
                s.set_display(id, |d| d.axes_grid = v);
            }
            "point_size" => {
                let v = val.as_f64().unwrap_or(4.0) as f32;
                s.set_display(id, |d| d.point_size = v);
            }
            _ => {
                // Readers take only the options their file offers (a reader on a server is
                // checked once the server has said what they are).
                let reader =
                    s.nodes.get(&id).is_some_and(|n| matches!(n.kind, aurafx_pipeline::NodeKind::Reader { .. } | aurafx_pipeline::NodeKind::Remote { .. }));
                if let Some(specs) = s.reader_specs(id).filter(|_| reader) {
                    if !specs.iter().any(|p| p.key == key) {
                        let keys: Vec<&str> = specs.iter().map(|p| p.key.as_str()).collect();
                        return Err(PyValueError::new_err(if keys.is_empty() {
                            format!("{key}: this file's reader has no options")
                        } else {
                            format!("{key}: not an option of this reader (options: {})", keys.join(", "))
                        }));
                    }
                }
                // The parameters of a CAD document are the node's parameters: node.set(width=50).
                if s.is_cad_model(id) && s.nodes.get(&id).is_some_and(|n| n.params.get(&key).is_none()) {
                    s.cad_set_parameter(id, &key, &val).map_err(|e| PyValueError::new_err(format!("{e:#}")))?;
                } else {
                    s.set_param_json(id, &key, &val)
                }
            }
        }
    }
    Ok(())
}

fn resolve_color_by(s: &aurafx_pipeline::Session, id: NodeId, name: &str, assoc: Option<&str>) -> PyResult<ColorBy> {
    let node = s.nodes.get(&id).ok_or_else(|| PyKeyError::new_err("node no longer exists"))?;
    let assoc = match assoc {
        Some("cells") | Some("cell") => Association::Cells,
        Some(_) => Association::Points,
        None => {
            // Prefer whichever attribute set actually has the array.
            match node.output.as_deref() {
                Some(ds) if ds.point_data().map(|a| a.get(name).is_some()).unwrap_or(false) => Association::Points,
                Some(ds) if ds.cell_data().map(|a| a.get(name).is_some()).unwrap_or(false) => Association::Cells,
                _ => Association::Points,
            }
        }
    };
    Ok(ColorBy { assoc, name: name.to_string(), component: None })
}

/// A pipeline node handle.
#[pyclass(name = "Node", module = "aurafx")]
#[derive(Clone)]
pub struct PyNode {
    #[pyo3(get)]
    pub id: NodeId,
}

/// Callable returned by `node.<filter>` so that `node.slice(normal=[0,0,1])` works.
#[pyclass(module = "aurafx")]
struct FilterCall {
    input: NodeId,
    filter_id: String,
}

#[pymethods]
impl FilterCall {
    /// `node.append_datasets(other, ...)`: positional nodes are extra inputs of multi-input filters.
    #[pyo3(signature = (*args, **kwargs))]
    fn __call__(&self, args: Vec<PyRef<PyNode>>, kwargs: Option<&Bound<'_, PyDict>>) -> PyResult<PyNode> {
        let mut inputs = vec![self.input];
        inputs.extend(args.iter().map(|n| n.id));
        let id = session().lock().add_filter_multi(&self.filter_id, &inputs).map_err(err)?;
        apply_kwargs(id, kwargs)?;
        Ok(PyNode { id })
    }
    fn __repr__(&self) -> String {
        format!("<filter {} on node {}>", self.filter_id, self.input)
    }
}

#[pymethods]
impl PyNode {
    #[getter]
    fn name(&self) -> PyResult<String> {
        session().lock().nodes.get(&self.id).map(|n| n.name.clone()).ok_or_else(|| PyKeyError::new_err("node no longer exists"))
    }

    #[setter]
    fn set_name(&self, name: String) {
        session().lock().rename(self.id, name);
    }

    fn __repr__(&self) -> String {
        let s = session().lock();
        match s.nodes.get(&self.id) {
            Some(n) => format!("<Node {} \"{}\" ({})>", n.id, n.name, n.type_label()),
            None => format!("<Node {} (deleted)>", self.id),
        }
    }

    fn __getattr__(&self, name: String) -> PyResult<FilterCall> {
        match aurafx_filters::find(&name) {
            Some(def) if !def.is_source => Ok(FilterCall { input: self.id, filter_id: def.id.to_string() }),
            _ => {
                Err(pyo3::exceptions::PyAttributeError::new_err(format!("Node has no attribute or filter named '{name}'. Try afx.filters() to list filters.")))
            }
        }
    }

    /// All inputs of this node: the pipeline input first, then the extra inputs of
    /// multi-input filters.
    #[getter]
    fn inputs(&self) -> PyResult<Vec<PyNode>> {
        let s = session().lock();
        let n = s.nodes.get(&self.id).ok_or_else(|| PyKeyError::new_err("node no longer exists"))?;
        Ok(n.input.iter().chain(n.extra_inputs.iter()).map(|&id| PyNode { id }).collect())
    }

    /// Make this the active node (the one the Properties panel and new filters use).
    fn activate(&self) -> PyResult<PyNode> {
        let mut s = session().lock();
        if !s.nodes.contains_key(&self.id) {
            return Err(PyKeyError::new_err("node no longer exists"));
        }
        s.active = Some(self.id);
        s.touch();
        Ok(self.clone())
    }

    /// Replace the extra inputs of a multi-input filter: `app.set_inputs(b, c)`.
    #[pyo3(signature = (*nodes))]
    fn set_inputs(&self, nodes: Vec<PyRef<PyNode>>) -> PyResult<PyNode> {
        session().lock().set_extra_inputs(self.id, nodes.iter().map(|n| n.id).collect()).map_err(err)?;
        Ok(self.clone())
    }

    /// Set parameters / display options: node.set(normal=[0,0,1], opacity=0.5).
    #[pyo3(signature = (**kwargs))]
    fn set(&self, kwargs: Option<&Bound<'_, PyDict>>) -> PyResult<PyNode> {
        apply_kwargs(self.id, kwargs)?;
        Ok(self.clone())
    }

    #[pyo3(signature = (name=None, assoc=None, component=None, cmap=None, range=None))]
    fn color_by(
        &self,
        name: Option<String>,
        assoc: Option<String>,
        component: Option<usize>,
        cmap: Option<String>,
        range: Option<(f64, f64)>,
    ) -> PyResult<PyNode> {
        let mut s = session().lock();
        match name {
            Some(n) => {
                let mut cb = resolve_color_by(&s, self.id, &n, assoc.as_deref())?;
                cb.component = component;
                s.color_by(self.id, Some(cb));
            }
            None => s.color_by(self.id, None),
        }
        if let Some(c) = cmap {
            s.set_display(self.id, |d| d.colormap = c);
        }
        if range.is_some() {
            s.set_display(self.id, |d| d.range = range);
        }
        Ok(self.clone())
    }

    /// Reload automatically when the file changes (live view of a running simulation).
    #[pyo3(signature = (on=true, follow_latest=true))]
    fn watch(&self, on: bool, follow_latest: bool) -> PyNode {
        let mut s = session().lock();
        s.set_watch(self.id, on);
        if let Some(n) = s.nodes.get_mut(&self.id) {
            n.follow_latest = follow_latest;
        }
        self.clone()
    }

    fn show(&self) -> PyNode {
        session().lock().set_visible(self.id, true);
        self.clone()
    }

    fn hide(&self) -> PyNode {
        session().lock().set_visible(self.id, false);
        self.clone()
    }

    /// Show this object in another 3D view. `view` is a view number (0 = main view) or
    /// "right"/"down" to open a new view holding only this object. By default it is moved
    /// (hidden in the other views); keep=True shows it there as well.
    #[pyo3(signature = (view, keep=false))]
    fn send_to_view(&self, view: &Bound<'_, PyAny>, keep: bool) -> PyResult<PyNode> {
        let target = if let Ok(i) = view.extract::<u32>() {
            i.to_string()
        } else {
            let s: String = view.extract()?;
            if !["right", "down"].contains(&s.as_str()) {
                return Err(PyValueError::new_err("view must be a view number, 'right' or 'down'"));
            }
            s
        };
        app_command(format!("send {} {target}{}", self.id, if keep { " keep" } else { "" }));
        Ok(self.clone())
    }

    /// Keyframe parameter `param` at animation time t (0..1). Without `value`, the current value is used.
    /// Display properties work too: "opacity", "point_size", "line_width", "solid_color",
    /// "edge_color", "lic_strength", "volume_opacity", "volume_threshold". `interp` says how
    /// the value moves on to the next key ("linear", "step", "ease in", "ease out",
    /// "ease in-out", "exponential").
    #[pyo3(signature = (param, t, value=None, interp="linear"))]
    fn key(&self, param: &str, t: f64, value: Option<&Bound<'_, PyAny>>, interp: &str) -> PyResult<PyNode> {
        let interp = parse_interp(interp)?;
        let mut s = session().lock();
        let display = param.strip_prefix("display.").unwrap_or(param);
        let is_display = aurafx_pipeline::animation::DISPLAY_TRACKS.contains(&display) && s.nodes.get(&self.id).is_some_and(|n| n.params.get(param).is_none());
        let track = if is_display { format!("display.{display}") } else { param.to_string() };
        let current = s
            .nodes
            .get(&self.id)
            .and_then(|n| if is_display { aurafx_pipeline::animation::display_value(&n.display, display) } else { n.params.get(param).cloned() })
            .ok_or_else(|| PyKeyError::new_err(format!("no parameter '{param}'")))?;
        let v = match value {
            None => current,
            Some(v) => {
                if let Ok(x) = v.extract::<[f64; 3]>() {
                    aurafx_filters::params::ParamValue::Vec3(x)
                } else if let (aurafx_filters::params::ParamValue::Int(_), Ok(i)) = (&current, v.extract::<i64>()) {
                    aurafx_filters::params::ParamValue::Int(i)
                } else {
                    aurafx_filters::params::ParamValue::Float(v.extract::<f64>()?)
                }
            }
        };
        if !aurafx_pipeline::animation::animatable(&v) {
            return Err(PyValueError::new_err(format!("'{param}' is not a number or 3-vector")));
        }
        s.animation.key_param_with(self.id, &track, t.clamp(0.0, 1.0), v, interp);
        Ok(self.clone())
    }

    fn delete(&self) {
        session().lock().remove_node(self.id);
    }

    /// Run the pipeline up to this node and return the dataset summary.
    fn update(&self) -> PyResult<String> {
        let mut s = session().lock();
        s.update_blocking();
        let n = s.nodes.get(&self.id).ok_or_else(|| PyKeyError::new_err("node no longer exists"))?;
        if let Some(e) = &n.error {
            return Err(PyRuntimeError::new_err(e.clone()));
        }
        Ok(n.info.as_ref().map(|i| i.summary()).unwrap_or_default())
    }

    /// Dataset summary as a dict (points, cells, bounds, arrays).
    fn info<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let mut s = session().lock();
        s.update_blocking();
        let n = s.nodes.get(&self.id).ok_or_else(|| PyKeyError::new_err("node no longer exists"))?;
        let info = n.info.clone().ok_or_else(|| PyRuntimeError::new_err(n.error.clone().unwrap_or_else(|| "no output".into())))?;
        let json = serde_json::to_string(&info).map_err(err)?;
        let m = py.import("json")?;
        m.call_method1("loads", (json,))
    }

    /// Open the CAD environment on this node (a CAD Model): `m.edit_cad()`.
    fn edit_cad(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
        let m = py.import("aurafx")?;
        m.getattr("_edit_cad")?.call1((self.clone(),)).map(|o| o.unbind())
    }

    /// The CAD document of a CAD Model node: `node.cad.parameter("width", 40)`, `node.cad.box(...)`.
    #[getter]
    fn cad(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
        if !session().lock().is_cad_model(self.id) {
            return Err(pyo3::exceptions::PyAttributeError::new_err("only a CAD Model node has .cad: afx.cad_model() makes one"));
        }
        let m = py.import("aurafx")?;
        Ok(m.getattr("_CadApi")?.call1((self.clone(),))?.unbind())
    }

    #[getter]
    fn params<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let s = session().lock();
        let n = s.nodes.get(&self.id).ok_or_else(|| PyKeyError::new_err("node no longer exists"))?;
        let json = serde_json::to_string(&n.params).map_err(err)?;
        py.import("json")?.call_method1("loads", (json,))
    }

    /// Array names: {"points": [...], "cells": [...]}.
    fn arrays<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyDict>> {
        let mut s = session().lock();
        s.update_blocking();
        let n = s.nodes.get(&self.id).ok_or_else(|| PyKeyError::new_err("node no longer exists"))?;
        let d = PyDict::new(py);
        if let Some(info) = &n.info {
            d.set_item("points", info.point_arrays.iter().map(|a| a.name.clone()).collect::<Vec<_>>())?;
            d.set_item("cells", info.cell_arrays.iter().map(|a| a.name.clone()).collect::<Vec<_>>())?;
            d.set_item("field", info.field_arrays.iter().map(|a| a.name.clone()).collect::<Vec<_>>())?;
        }
        Ok(d)
    }

    /// Get an array as a NumPy array (copy). `assoc` is "points", "cells" or "field".
    #[pyo3(signature = (name, assoc=None))]
    fn array<'py>(&self, py: Python<'py>, name: String, assoc: Option<String>) -> PyResult<Bound<'py, PyAny>> {
        let ds = self.flat_output()?;
        let find = |a: Option<&aurafx_data::Attributes>| a.and_then(|x| x.get(&name).cloned());
        let arr = match assoc.as_deref() {
            Some("cells") | Some("cell") => find(ds.cell_data()),
            Some("field") => find(Some(ds.field_data())),
            Some(_) => find(ds.point_data()),
            None => find(ds.point_data()).or_else(|| find(ds.cell_data())).or_else(|| find(Some(ds.field_data()))).or_else(|| match &*ds {
                DataSet::Table(t) => t.column(&name).cloned(),
                _ => None,
            }),
        }
        .ok_or_else(|| PyKeyError::new_err(format!("no array named {name:?}")))?;
        array_to_numpy(py, &arr.data, arr.ncomp)
    }

    /// Write this node's data: .vtu .vtp .vti .vtr .vts .vtm .vtk .vtkhdf .exo .case (EnSight Gold)
    /// .xmf (Xdmf + .h5) .cgns .csv .stl .ply .dream3d (with the pipeline) .spparks .tex (format from the extension). all_times=True writes every
    /// time step: into one Exodus, VTKHDF or CGNS file (CGNS keeps one mesh), or as an EnSight or
    /// Xdmf series.
    #[pyo3(signature = (path, all_times=false))]
    fn save(&self, path: String, all_times: bool) -> PyResult<()> {
        let p = std::path::Path::new(&path);
        if all_times {
            if !aurafx_pipeline::Session::holds_all_times(p) {
                return Err(PyValueError::new_err(
                    "all_times=True needs an Exodus (.exo), VTKHDF (.vtkhdf), EnSight (.case), Xdmf (.xmf) or CGNS (.cgns) file name",
                ));
            }
            return session().lock().save_all_times(self.id, p).map(|_| ()).map_err(err);
        }
        let ds = self.output()?;
        if p.extension().is_some_and(|e| e.eq_ignore_ascii_case("dream3d")) {
            // Records the pipeline that made the data.
            return session().lock().save_data(self.id, p).map_err(err);
        }
        aurafx_io::writers::write_file(&ds, p).map_err(err)
    }

    /// The pipeline that makes this node, as JSON text (the form .dream3d files record).
    fn pipeline_json(&self) -> PyResult<String> {
        Ok(session().lock().pipeline_json(self.id))
    }

    /// Point coordinates as an (N, 3) NumPy array.
    #[getter]
    fn points<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let ds = self.flat_output()?;
        let pts = ds.points().ok_or_else(|| PyRuntimeError::new_err("dataset has no points"))?;
        array_to_numpy(py, &pts.data, 3)
    }

    /// Add a NumPy array as point (or cell) data.
    #[pyo3(signature = (name, values, assoc="points"))]
    fn add_array(&self, name: String, values: Bound<'_, PyAny>, assoc: &str) -> PyResult<PyNode> {
        let (data, ncomp) = numpy_to_array(&values)?;
        let mut s = session().lock();
        let node = s.nodes.get_mut(&self.id).ok_or_else(|| PyKeyError::new_err("node no longer exists"))?;
        let Some(out) = node.output.as_ref() else { return Err(PyRuntimeError::new_err("node has no output yet; call update() first")) };
        let mut ds = (**out).clone();
        let arr = aurafx_data::Array::new(name, ncomp, data);
        let target = if assoc == "cells" { ds.cell_data_mut() } else { ds.point_data_mut() };
        match target {
            Some(t) => {
                t.insert(arr);
            }
            None => return Err(PyRuntimeError::new_err("dataset cannot hold arrays")),
        }
        let ds = std::sync::Arc::new(ds);
        node.info = Some(aurafx_data::DataSetInfo::of(&ds));
        node.output = Some(ds);
        s.data_revision += 1;
        s.revision += 1;
        Ok(self.clone())
    }
}

impl PyNode {
    /// Output with multi-block data merged into one mesh (for array/point access).
    fn flat_output(&self) -> PyResult<std::sync::Arc<DataSet>> {
        let ds = self.output()?;
        if matches!(&*ds, DataSet::MultiBlock(_)) {
            let f = aurafx_filters::find("merge_blocks").ok_or_else(|| PyRuntimeError::new_err("merge_blocks missing"))?;
            return f.run(Some(&ds), &f.default_params()).map(std::sync::Arc::new).map_err(err);
        }
        Ok(ds)
    }

    fn output(&self) -> PyResult<std::sync::Arc<DataSet>> {
        let mut s = session().lock();
        s.update_blocking();
        let n = s.nodes.get(&self.id).ok_or_else(|| PyKeyError::new_err("node no longer exists"))?;
        n.output.clone().ok_or_else(|| PyRuntimeError::new_err(n.error.clone().unwrap_or_else(|| "node has no output".into())))
    }
}

pub(crate) fn array_to_numpy<'py>(py: Python<'py>, data: &ArrayData, ncomp: usize) -> PyResult<Bound<'py, PyAny>> {
    macro_rules! conv {
        ($v:expr) => {{
            let v = $v.clone();
            if ncomp == 1 {
                Ok(PyArray1::from_vec(py, v).into_any())
            } else {
                let n = v.len() / ncomp;
                let a = PyArray1::from_vec(py, v);
                Ok(a.reshape([n, ncomp])?.into_any())
            }
        }};
    }
    match data {
        ArrayData::I8(v) => conv!(v),
        ArrayData::U8(v) => conv!(v),
        ArrayData::I16(v) => conv!(v),
        ArrayData::U16(v) => conv!(v),
        ArrayData::I32(v) => conv!(v),
        ArrayData::U32(v) => conv!(v),
        ArrayData::I64(v) => conv!(v),
        ArrayData::U64(v) => conv!(v),
        ArrayData::F32(v) => conv!(v),
        ArrayData::F64(v) => conv!(v),
        ArrayData::F16(v) => conv!(v.iter().map(|x| x.to_f32()).collect::<Vec<f32>>()),
        ArrayData::String(v) => Ok(PyList::new(py, v.clone())?.into_any()),
    }
}

/// A NumPy array (or anything array-like) as an auraFX array: float32/float64/int32/int64/uint8
/// keep their type, everything else becomes float64. Returns (data, components).
pub(crate) fn numpy_to_array(v: &Bound<'_, PyAny>) -> PyResult<(ArrayData, usize)> {
    let py = v.py();
    let np = py.import("numpy")?;
    let arr = np.call_method1("ascontiguousarray", (v,))?;
    let shape: Vec<usize> = arr.getattr("shape")?.extract()?;
    if shape.len() > 2 {
        return Err(PyValueError::new_err(format!("arrays must be 1-D or 2-D (N x components), got shape {shape:?}")));
    }
    let ncomp = if shape.len() == 2 { shape[1].max(1) } else { 1 };
    macro_rules! take {
        ($t:ty, $var:ident) => {
            if let Ok(a) = arr.cast::<numpy::PyArrayDyn<$t>>() {
                return Ok((ArrayData::$var(a.readonly().as_slice()?.to_vec()), ncomp));
            }
        };
    }
    take!(f64, F64);
    take!(f32, F32);
    take!(i32, I32);
    take!(i64, I64);
    take!(u8, U8);
    let arr = arr.call_method1("astype", ("float64",))?;
    let a = arr.cast::<numpy::PyArrayDyn<f64>>()?;
    Ok((ArrayData::F64(a.readonly().as_slice()?.to_vec()), ncomp))
}

// ------------------------------------------------------------------ module functions

/// (id, is_source) of a filter by id or label, for `afx.<plugin filter>(...)`.
#[pyfunction]
fn _filter_kind(name: &str) -> Option<(String, bool)> {
    aurafx_filters::find(name).map(|f| (f.id.to_string(), f.is_source))
}

/// Loaded plugins: [{path, filters, readers, error, enabled}].
#[pyfunction]
fn plugins<'py>(py: Python<'py>) -> PyResult<Bound<'py, PyList>> {
    let l = PyList::empty(py);
    for p in crate::plugins::list() {
        let d = PyDict::new(py);
        d.set_item("path", p.path.display().to_string())?;
        d.set_item("filters", p.filters)?;
        d.set_item("readers", p.readers)?;
        d.set_item("error", p.error)?;
        d.set_item("enabled", p.enabled)?;
        l.append(d)?;
    }
    Ok(l)
}

/// Open a file. With `host=` (a host from the Simulations panel, an ssh alias or user@machine,
/// or "local") or an `ssh://host/path` URL, the file is read on that machine by aurafx-server
/// and filters added below it run there too.
#[pyfunction]
#[pyo3(signature = (path, host=None, processes=None, **kwargs))]
fn load(path: String, host: Option<String>, processes: Option<u32>, kwargs: Option<&Bound<'_, PyDict>>) -> PyResult<PyNode> {
    let (host, path) = match (host, path.strip_prefix("ssh://")) {
        (Some(h), _) => (Some(h), path),
        (None, Some(rest)) => {
            let (h, p) = rest.split_once('/').ok_or_else(|| PyValueError::new_err("use ssh://host/path/to/file"))?;
            let p = if p.starts_with('~') { p.to_string() } else { format!("/{p}") };
            (Some(h.to_string()), p)
        }
        (None, None) => (None, path),
    };
    let id = match host {
        Some(h) => {
            let mut host = aurafx_jobs::resolve_host(Some(&h));
            if let Some(n) = processes {
                host.server_processes = n;
            }
            let mut s = session().lock();
            let id = s.open_remote(host, &path);
            s.update_blocking();
            if let Some(e) = s.nodes.get(&id).and_then(|n| n.error.clone()) {
                s.remove_node(id);
                return Err(PyRuntimeError::new_err(e));
            }
            id
        }
        None => session().lock().open_file(&path).map_err(err)?,
    };
    apply_kwargs(id, kwargs)?;
    Ok(PyNode { id })
}

/// Servers this session talks to: name, whether connected, and the machine's details.
#[pyfunction]
fn servers(py: Python<'_>) -> PyResult<Vec<Py<PyDict>>> {
    let s = session().lock();
    let mut out = Vec::new();
    for (name, c) in &s.remote_clients {
        let d = PyDict::new(py);
        d.set_item("name", name)?;
        d.set_item("connected", c.is_connected())?;
        d.set_item("processes", c.info.lock().as_ref().map(|i| i.processes.max(1)).unwrap_or(1))?;
        if let Some(i) = c.info.lock().as_ref() {
            d.set_item("hostname", &i.hostname)?;
            d.set_item("cpus", i.cpus)?;
            d.set_item("version", &i.version)?;
            d.set_item("gpu", i.gpu.clone())?;
        }
        out.push(d.unbind());
    }
    Ok(out)
}

#[pyfunction]
#[pyo3(signature = (filter_id, input=None, **kwargs))]
fn apply(filter_id: String, input: Option<PyRef<PyNode>>, kwargs: Option<&Bound<'_, PyDict>>) -> PyResult<PyNode> {
    let id = {
        let mut s = session().lock();
        let input = input.map(|n| n.id).or(s.active);
        match input {
            Some(i) => s.add_filter(&filter_id, i).map_err(err)?,
            None => s.add_source(&filter_id).map_err(err)?,
        }
    };
    apply_kwargs(id, kwargs)?;
    Ok(PyNode { id })
}

/// Add a multi-input filter: `afx.apply_multi("append_datasets", [a, b, c])`.
#[pyfunction]
#[pyo3(signature = (filter_id, inputs, **kwargs))]
fn apply_multi(filter_id: String, inputs: Vec<PyRef<PyNode>>, kwargs: Option<&Bound<'_, PyDict>>) -> PyResult<PyNode> {
    let ids: Vec<NodeId> = inputs.iter().map(|n| n.id).collect();
    let id = session().lock().add_filter_multi(&filter_id, &ids).map_err(err)?;
    apply_kwargs(id, kwargs)?;
    Ok(PyNode { id })
}

#[pyfunction]
fn nodes() -> Vec<PyNode> {
    session().lock().nodes.keys().map(|&id| PyNode { id }).collect()
}

#[pyfunction]
fn active() -> Option<PyNode> {
    session().lock().active.map(|id| PyNode { id })
}

#[pyfunction]
fn get(name: String) -> PyResult<PyNode> {
    let s = session().lock();
    if let Ok(id) = name.parse::<NodeId>() {
        if s.nodes.contains_key(&id) {
            return Ok(PyNode { id });
        }
    }
    s.find_by_name(&name).map(|id| PyNode { id }).ok_or_else(|| PyKeyError::new_err(format!("no node named {name:?}")))
}

#[pyfunction]
fn run() {
    session().lock().update_blocking();
}

#[pyfunction]
fn describe() -> String {
    session().lock().describe()
}

#[pyfunction]
fn filters<'py>(py: Python<'py>) -> PyResult<Bound<'py, PyList>> {
    let l = PyList::empty(py);
    for f in aurafx_filters::registry().iter() {
        let d = PyDict::new(py);
        d.set_item("id", f.id)?;
        d.set_item("label", f.label)?;
        d.set_item("category", f.category)?;
        d.set_item("description", f.description)?;
        d.set_item("source", f.is_source)?;
        d.set_item("params", f.params.iter().map(|p| p.key.clone()).collect::<Vec<_>>())?;
        l.append(d)?;
    }
    Ok(l)
}

#[pyfunction]
fn help_filter(id: String) -> PyResult<String> {
    let f = aurafx_filters::find(&id).ok_or_else(|| PyKeyError::new_err(format!("unknown filter {id}")))?;
    let mut s = format!("{} ({})\n{}\n\nParameters:\n", f.label, f.id, f.description);
    for p in &f.params {
        s += &format!(
            "  {} - {} (default {}){}\n",
            p.key,
            p.label,
            serde_json::to_string(&p.default).unwrap_or_default(),
            if p.help.is_empty() { String::new() } else { format!(": {}", p.help) }
        );
    }
    Ok(s)
}

#[pyfunction]
#[pyo3(signature = (index=None))]
fn time(index: Option<usize>) -> (usize, usize, Option<f64>) {
    let mut s = session().lock();
    if let Some(i) = index {
        s.set_time_index(i);
    }
    (s.time_index, s.times.len(), s.current_time())
}

#[pyfunction]
fn times() -> Vec<f64> {
    session().lock().times.clone()
}

#[pyfunction]
fn reset_camera() {
    // A pending view request already fits the data; keep it (both share one slot).
    let mut s = session().lock();
    if !matches!(s.camera_request, Some(CameraRequest::View { .. })) {
        s.camera_request = Some(CameraRequest::ResetToBounds);
    }
}

/// View from a named direction ("+x", "-y", "iso", ...) or a vector [x, y, z] pointing from the data to the camera.
#[pyfunction]
#[pyo3(signature = (direction, up=None))]
fn view(direction: Bound<'_, PyAny>, up: Option<[f64; 3]>) -> PyResult<()> {
    if let Ok(v) = direction.extract::<[f64; 3]>() {
        let up = up.unwrap_or(if v[0].abs() < 1e-9 && v[1].abs() < 1e-9 { [0.0, 1.0, 0.0] } else { [0.0, 0.0, 1.0] });
        session().lock().camera_request = Some(CameraRequest::View { direction: v, up });
        return Ok(());
    }
    let direction: String = direction.extract()?;
    let (dir, up) = match direction.to_ascii_lowercase().as_str() {
        "+x" | "x" => ([1.0, 0.0, 0.0], [0.0, 0.0, 1.0]),
        "-x" => ([-1.0, 0.0, 0.0], [0.0, 0.0, 1.0]),
        "+y" | "y" => ([0.0, 1.0, 0.0], [0.0, 0.0, 1.0]),
        "-y" => ([0.0, -1.0, 0.0], [0.0, 0.0, 1.0]),
        "+z" | "z" => ([0.0, 0.0, 1.0], [0.0, 1.0, 0.0]),
        "-z" => ([0.0, 0.0, -1.0], [0.0, 1.0, 0.0]),
        "iso" | "isometric" => ([1.0, 1.0, 1.0], [0.0, 0.0, 1.0]),
        _ => return Err(PyValueError::new_err("direction must be one of +x -x +y -y +z -z iso")),
    };
    session().lock().camera_request = Some(CameraRequest::View { direction: dir, up });
    Ok(())
}

#[pyfunction]
#[pyo3(signature = (path, width=None, height=None, transparent=false, annotations=true))]
fn screenshot(py: Python<'_>, path: String, width: Option<u32>, height: Option<u32>, transparent: bool, annotations: bool) -> PyResult<()> {
    let id = request_screenshot(path, width, height, transparent, annotations);
    // Wait until the app has saved it (a ray-traced picture may take a while).
    wait_result(py, id).map_err(|e| PyRuntimeError::new_err(format!("screenshot: {e}")))
}

/// Export the visible objects as a 3D scene: `afx.export_scene("scene.glb")` (glTF binary, for
/// Blender, three.js, PowerPoint, ...) or `.obj` (with vertex colors).
#[pyfunction]
fn export_scene(py: Python<'_>, path: String) -> PyResult<()> {
    let ext = std::path::Path::new(&path).extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase();
    if !matches!(ext.as_str(), "glb" | "obj" | "x3d" | "wrl") {
        return Err(PyValueError::new_err("export_scene writes .glb, .obj, .x3d or .wrl files (afx.screenshot('fig.svg') for a vector picture)"));
    }
    screenshot(py, path, None, None, false, true)
}

fn request_screenshot(path: String, width: Option<u32>, height: Option<u32>, transparent: bool, overlays: bool) -> u64 {
    let mut s = session().lock();
    // One of width/height keeps the view's aspect ratio (the UI fills in the other).
    let size = match (width, height) {
        (Some(w), Some(h)) => Some((w, h)),
        (Some(w), None) => Some((w, 0)),
        (None, Some(h)) => Some((0, h)),
        _ => None,
    };
    s.next_screenshot_id += 1;
    let id = s.next_screenshot_id;
    s.screenshot_request = Some(aurafx_pipeline::session::ScreenshotRequest { path: std::path::PathBuf::from(path), size, transparent, id, overlays });
    s.revision += 1;
    id
}

/// Add a line to the Charts panel: afx.plot(x, y, label="T max").
#[pyfunction]
#[pyo3(signature = (x, y=None, label=None, title=None, xlabel=None, ylabel=None, clear=false))]
#[allow(clippy::too_many_arguments)]
fn plot(
    x: Bound<'_, PyAny>,
    y: Option<Bound<'_, PyAny>>,
    label: Option<String>,
    title: Option<String>,
    xlabel: Option<String>,
    ylabel: Option<String>,
    clear: bool,
) -> PyResult<()> {
    let to_vec = |v: &Bound<'_, PyAny>| -> PyResult<Vec<f64>> {
        if let Ok(l) = v.extract::<Vec<f64>>() {
            return Ok(l);
        }
        let (d, _) = numpy_to_array(v)?;
        Ok(d.to_f64_vec())
    };
    let (xs, ys) = match y {
        Some(y) => (to_vec(&x)?, to_vec(&y)?),
        None => {
            let ys = to_vec(&x)?;
            ((0..ys.len()).map(|i| i as f64).collect(), ys)
        }
    };
    if xs.len() != ys.len() {
        return Err(PyValueError::new_err(format!("x has {} values but y has {}", xs.len(), ys.len())));
    }
    let mut s = session().lock();
    if clear {
        s.chart.series.clear();
    }
    if let Some(t) = title {
        s.chart.title = t;
    }
    if let Some(t) = xlabel {
        s.chart.x_label = t;
    }
    if let Some(t) = ylabel {
        s.chart.y_label = t;
    }
    let n = s.chart.series.len() + 1;
    s.add_series(label.unwrap_or_else(|| format!("series {n}")), xs, ys);
    Ok(())
}

#[pyfunction]
fn clear_plots() {
    let mut s = session().lock();
    s.chart = Default::default();
    s.revision += 1;
}

/// Statistic of an array over every time step, plotted in the Charts panel.
/// stat: "max", "min", "mean", "sum" or "value" (for global/field variables). Returns (times, values).
#[pyfunction]
#[pyo3(signature = (node, array, stat="max", plot=true))]
fn over_time(py: Python<'_>, node: PyRef<PyNode>, array: String, stat: &str, plot: bool) -> PyResult<(Vec<f64>, Vec<f64>)> {
    let id = node.id;
    let stat = stat.to_string();
    let r = py.detach(|| {
        let mut s = session().lock();
        let r = s.over_time(id, &array, &stat);
        if let (Ok((x, y)), true) = (&r, plot) {
            let name = format!("{stat}({array})");
            if s.chart.x_label.is_empty() {
                s.chart.x_label = "time".into();
            }
            s.add_series(name, x.clone(), y.clone());
        }
        r
    });
    r.map_err(err)
}

fn app_command(cmd: String) {
    let wake = {
        let mut s = session().lock();
        s.app_commands.push(cmd);
        s.revision += 1;
        s.wake_ui.clone()
    };
    // An idle window draws no frames: wake it so that it carries the command out.
    if let Some(w) = wake {
        w();
    }
}

/// Mean/min/max/stddev of every array over all time steps, as a new node.
#[pyfunction]
fn temporal_statistics(py: Python<'_>, node: PyRef<PyNode>) -> PyResult<PyNode> {
    let id = node.id;
    py.detach(|| session().lock().temporal_statistics(id)).map(|id| PyNode { id }).map_err(err)
}

/// Add a dataset built from NumPy arrays as a point cloud node: afx.points_from(xyz, temperature=t).
#[pyfunction]
#[pyo3(signature = (xyz, name="points", **arrays))]
fn points_from(xyz: Bound<'_, PyAny>, name: &str, arrays: Option<&Bound<'_, PyDict>>) -> PyResult<PyNode> {
    let (pts, nc) = numpy_to_array(&xyz)?;
    if nc != 3 {
        return Err(PyValueError::new_err("xyz must have shape (N, 3)"));
    }
    let mut pd = aurafx_data::PolyData::from_points(aurafx_data::Points::from_array(pts));
    if let Some(a) = arrays {
        for (k, v) in a.iter() {
            let (d, c) = numpy_to_array(&v)?;
            pd.point_data.insert(aurafx_data::Array::new(k.extract::<String>()?, c, d));
        }
    }
    let id = session().lock().add_data(name, DataSet::PolyData(pd));
    Ok(PyNode { id })
}

/// Add a text label to the 3D views. corner: "upper left", "upper right", "lower right", "upper center", "lower center".
#[pyfunction]
#[pyo3(signature = (text, corner="upper left", size=16.0))]
fn annotate(text: String, corner: &str, size: f32) {
    let mut s = session().lock();
    s.annotations.push(aurafx_pipeline::Annotation { text, corner: corner.to_string(), size });
    s.revision += 1;
}

#[pyfunction]
fn clear_annotations() {
    let mut s = session().lock();
    s.annotations.clear();
    s.revision += 1;
}

/// Show (or hide) the current time in the 3D views.
#[pyfunction]
#[pyo3(signature = (on=true))]
fn time_label(on: bool) {
    let mut s = session().lock();
    s.show_time_label = on;
    s.revision += 1;
}

/// 3D view lighting and screen effects, e.g. `afx.lighting(shadows=True, ssao=True)`.
///
/// Options: light_kit, lights (list of dicts: direction, color, intensity, camera_relative,
/// shadows; [] for the light kit), ambient, shadows, edl, edl_strength, ssao, ssao_strength,
/// ssao_radius, fxaa, tone_mapping ("none", "reinhard", "aces"), exposure. `afx.lighting()`
/// with no options returns the defaults.
#[pyfunction]
#[pyo3(signature = (**options))]
fn lighting(py: Python<'_>, options: Option<&Bound<'_, PyDict>>) -> PyResult<Py<PyAny>> {
    let defaults = serde_json::to_value(aurafx_pipeline::Lighting::default()).map_err(err)?;
    let Some(options) = options.filter(|o| !o.is_empty()) else {
        return Ok(py.import("json")?.call_method1("loads", (defaults.to_string(),))?.unbind());
    };
    let patch = to_json(options.as_any())?;
    // Check names and types here, so mistakes raise in Python.
    let mut v = defaults.clone();
    for (k, val) in patch.as_object().into_iter().flatten() {
        let Some(slot) = v.as_object_mut().and_then(|o| o.get_mut(k)) else {
            return Err(PyValueError::new_err(format!(
                "unknown lighting option {k:?}; options: {}",
                defaults.as_object().unwrap().keys().cloned().collect::<Vec<_>>().join(", ")
            )));
        };
        *slot = val.clone();
    }
    serde_json::from_value::<aurafx_pipeline::Lighting>(v).map_err(|e| PyValueError::new_err(format!("lighting: {e}")))?;
    app_command(format!("lighting {patch}"));
    Ok(py.None())
}

/// Bring a panel to the front: "3D View", "Python", "Assistant", "Simulations", "Spreadsheet", "Log", ...
#[pyfunction]
fn show_panel(name: String) {
    app_command(format!("show {name}"));
}

/// Open the Simulations panel on a template: "lammps", "moose", "moose_polycrystal",
/// "spparks", "pinnacle", "openfoam", "python", "shell" (or its label).
#[pyfunction]
fn simulation_template(name: String) {
    app_command(format!("sim_template {name}"));
}

/// Send a request to the AI assistant panel. With agent=True it runs its code and iterates.
#[pyfunction]
#[pyo3(signature = (text, agent=false))]
fn ask(text: String, agent: bool) {
    app_command(format!("ask{} {text}", if agent { "-agent" } else { "" }));
}

/// The assistant panel's state: busy (an answer is arriving), agent (agent mode is iterating), the
/// number of messages, the last answer, the last error and the last messages as (role, text). Empty
/// values without the app.
#[pyfunction]
fn assistant_status(py: Python<'_>) -> PyResult<Py<PyDict>> {
    let a = session().lock().assistant.clone();
    let d = PyDict::new(py);
    d.set_item("busy", a.busy)?;
    d.set_item("agent", a.agent)?;
    d.set_item("messages", a.messages)?;
    d.set_item("last", a.last)?;
    d.set_item("error", a.error)?;
    d.set_item("history", a.history)?;
    d.set_item("frame", a.frame)?;
    Ok(d.into())
}

/// Add a 3D view next to the active one: "right" (side by side) or "down" (above/below).
/// The new view becomes active and starts with the same camera and visible objects. Returns
/// its number once it exists (None without the app).
#[pyfunction]
#[pyo3(signature = (direction="right"))]
fn split_view(py: Python<'_>, direction: &str) -> PyResult<Option<u32>> {
    app_command(format!("split {direction}"));
    settle(py, "add the view")?;
    let s = session().lock();
    Ok((!s.view_ids.is_empty()).then_some(s.active_view))
}

/// Link (or unlink) the cameras of all 3D views.
#[pyfunction]
#[pyo3(signature = (on=true))]
fn link_views(on: bool) {
    app_command(format!("link {}", if on { "on" } else { "off" }));
}

/// Copy the active view's camera to every other view once.
#[pyfunction]
fn match_views() {
    app_command("match".into());
}

/// Wait (without the GIL) until `done` holds for the session the app updates every frame.
fn wait_app(py: Python<'_>, what: &str, done: &(dyn Fn(&aurafx_pipeline::Session) -> bool + Sync)) -> PyResult<()> {
    py.detach(|| {
        let start = std::time::Instant::now();
        loop {
            if done(&session().lock()) {
                return Ok(());
            }
            if start.elapsed() > std::time::Duration::from_secs(10) {
                return Err(PyRuntimeError::new_err(format!("the app did not {what} within 10 s")));
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
    })
}

/// Wait until the app has run the queued commands and finished a frame after them (no-op
/// without an app).
fn settle(py: Python<'_>, what: &str) -> PyResult<()> {
    if session().lock().view_ids.is_empty() {
        return Ok(());
    }
    wait_app(py, what, &|s| s.app_commands.is_empty())?;
    let frame = session().lock().app_frame;
    wait_app(py, what, &|s| s.app_frame > frame)
}

/// Make a 3D view active by number (0 = main view, 1, 2, ... = views added with split_view).
/// Returns when the view is active, so `node.hide()` and the like that follow apply to it.
#[pyfunction]
fn active_view(py: Python<'_>, index: u32) -> PyResult<()> {
    if session().lock().view_ids.is_empty() {
        // No app (plain Python): nothing to switch.
        return Ok(());
    }
    // Let earlier commands (split_view, ...) finish a frame so new views are known.
    settle(py, "switch views")?;
    let views = session().lock().view_ids.clone();
    if !views.contains(&index) {
        return Err(PyValueError::new_err(format!("no 3D view {index}; open views: {views:?}")));
    }
    app_command(format!("activate {index}"));
    wait_app(py, "switch views", &|s| s.active_view == index && s.app_commands.is_empty())
}

/// UI theme: "dark", "light" or "system".
#[pyfunction]
fn set_theme(mode: String) -> PyResult<()> {
    let m = mode.to_ascii_lowercase();
    if !["dark", "light", "system"].contains(&m.as_str()) {
        return Err(PyValueError::new_err("mode must be 'dark', 'light' or 'system'"));
    }
    app_command(format!("theme {m}"));
    Ok(())
}

/// Window layout preset: "full", "visualize", "analyze", "script" or "simulate".
#[pyfunction]
fn layout(name: String) -> PyResult<()> {
    let m = name.to_ascii_lowercase();
    if !["full", "visualize", "analyze", "script", "simulate"].contains(&m.as_str()) {
        return Err(PyValueError::new_err("layout must be one of full, visualize, analyze, script, simulate"));
    }
    app_command(format!("layout {m}"));
    Ok(())
}

/// Close a panel (it can be reopened from the Windows menu or with show_panel).
#[pyfunction]
fn hide_panel(name: String) {
    app_command(format!("hide {name}"));
}

/// Select cells (or points) of a node by flattened index; shown highlighted in the 3D view.
#[pyfunction]
#[pyo3(signature = (node, ids, assoc="cells"))]
fn select(node: PyRef<PyNode>, ids: Vec<usize>, assoc: &str) {
    let a = if assoc.starts_with("point") { Association::Points } else { Association::Cells };
    session().lock().set_selection(node.id, a, ids);
}

/// Current selection as (node, assoc, ids), or None.
#[pyfunction]
fn selection() -> Option<(PyNode, String, Vec<usize>)> {
    session().lock().selection.clone().map(|s| (PyNode { id: s.node }, if s.assoc == Association::Points { "points".into() } else { "cells".into() }, s.ids))
}

#[pyfunction]
fn clear_selection() {
    session().lock().clear_selection();
}

/// Turn the selection into an Extract Selection node.
#[pyfunction]
fn extract_selection() -> PyResult<PyNode> {
    session().lock().extract_selection().map(|id| PyNode { id }).map_err(err)
}

/// Values of every array of `node` at a point: {"name": value, ...}. Points outside the data return {}.
#[pyfunction]
fn probe<'py>(py: Python<'py>, node: PyRef<PyNode>, point: [f64; 3]) -> PyResult<Bound<'py, PyDict>> {
    let ds = node.output()?;
    let attrs = aurafx_filters::probe::probe_points(&ds, &[point]).map_err(err)?;
    let d = PyDict::new(py);
    if attrs.get("vtkValidPointMask").map(|m| m.get(0, 0) == 1.0).unwrap_or(false) {
        for a in attrs.iter().filter(|a| a.name != "vtkValidPointMask") {
            if a.ncomp == 1 {
                d.set_item(&a.name, a.get(0, 0))?;
            } else {
                d.set_item(&a.name, a.tuple(0))?;
            }
        }
    }
    Ok(d)
}

#[pyfunction]
fn undo() -> Option<String> {
    session().lock().undo()
}

#[pyfunction]
fn redo() -> Option<String> {
    session().lock().redo()
}

/// Save the pipeline, display settings and camera to an .afx file.
#[pyfunction]
fn save_state(path: String) -> PyResult<()> {
    let s = session().lock();
    let cam = s.camera_state;
    s.save_state(std::path::Path::new(&path), cam).map_err(err)
}

/// Build a pipeline from a pipeline file: a .json pipeline (numbered steps, the layout .dream3d
/// files record) or the pipeline inside a .dream3d file. Steps that read files open them when
/// they exist; otherwise the pipeline starts from `input` (a node). Returns the new nodes; steps
/// without an auraFX filter are listed in the console.
#[pyfunction]
#[pyo3(signature = (path, input=None))]
fn import_pipeline(path: String, input: Option<PyRef<'_, PyNode>>) -> PyResult<Vec<PyNode>> {
    let mut s = session().lock();
    let r = s.import_pipeline(std::path::Path::new(&path), input.map(|n| n.id)).map_err(err)?;
    for (step, why) in &r.skipped {
        s.log(aurafx_pipeline::session::LogLevel::Warn, format!("pipeline step {step} skipped: {why}"));
        println!("skipped {step}: {why}");
    }
    Ok(r.nodes.into_iter().map(|id| PyNode { id }).collect())
}

/// Replace the pipeline with the contents of an .afx file.
#[pyfunction]
fn load_state(path: String) -> PyResult<()> {
    session().lock().load_state(std::path::Path::new(&path)).map(|_| ()).map_err(err)
}

/// Record an animation: keyframes if any were added (key_camera / node.key), otherwise time
/// steps, or a 360° camera orbit with orbit=frames.
/// The format follows the extension: .gif, .mp4 (needs ffmpeg) or .png (a folder of frames).
///
/// With `cue`, a Python function called with the animation time t (0..1) before each frame
/// (after the keyframes are applied), the script renders the frames itself and returns when
/// the file is written: `afx.save_animation("a.mp4", cue=lambda t: s.set(value=t * 900))`.
#[pyfunction]
#[pyo3(signature = (path, fps=10.0, orbit=None, cue=None, frames=None, width=None, height=None))]
#[allow(clippy::too_many_arguments)]
fn save_animation(
    py: Python<'_>,
    path: String,
    fps: f32,
    orbit: Option<usize>,
    cue: Option<Bound<'_, PyAny>>,
    frames: Option<usize>,
    width: Option<u32>,
    height: Option<u32>,
) -> PyResult<()> {
    let Some(cue) = cue else {
        session().lock().animation_request = Some((std::path::PathBuf::from(path), fps, orbit));
        return Ok(());
    };
    if !cue.is_callable() {
        return Err(PyValueError::new_err("cue must be a function of the animation time t"));
    }
    let out = std::path::PathBuf::from(&path);
    let ext = out.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase();
    if !matches!(ext.as_str(), "gif" | "mp4" | "png") {
        return Err(PyValueError::new_err("save_animation writes .gif, .mp4 or .png (a folder of frames)"));
    }
    let (n, saved, time_index) = {
        let s = session().lock();
        (frames.unwrap_or(s.animation.frames).max(2), s.animated_params(), s.time_index)
    };
    let stem = out.file_stem().and_then(|s| s.to_str()).unwrap_or("frames").to_string();
    let dir = if ext == "png" {
        out.with_file_name(&stem)
    } else {
        std::env::temp_dir().join(format!("aurafx-cue-{}-{}", std::process::id(), session().lock().next_screenshot_id))
    };
    std::fs::create_dir_all(&dir).map_err(err)?;
    let render = || -> PyResult<()> {
        for i in 0..n {
            let t = i as f64 / (n - 1) as f64;
            {
                let mut s = session().lock();
                if let Some(c) = s.apply_animation(t) {
                    s.pending_camera = Some(c);
                }
            }
            cue.call1((t,))?;
            screenshot(py, dir.join(format!("frame_{i:05}.png")).display().to_string(), width, height, false, true)?;
        }
        Ok(())
    };
    let rendered = render();
    {
        let mut s = session().lock();
        s.restore_animated(&saved);
        s.set_time_index(time_index);
    }
    rendered?;
    if ext == "png" {
        return Ok(());
    }
    // The app encodes (it has the GIF/MP4 writers); wait for it.
    let id = {
        let mut s = session().lock();
        s.next_screenshot_id += 1;
        let id = s.next_screenshot_id;
        s.encode_request = Some(aurafx_pipeline::session::EncodeRequest { dir, frames: n, out, fps: fps.max(0.5), id });
        s.revision += 1;
        id
    };
    wait_result(py, id).map_err(|e| PyRuntimeError::new_err(format!("save_animation: {e}")))
}

/// Wait for the app to finish request `id` (screenshot, export, encoding).
fn wait_result(py: Python<'_>, id: u64) -> Result<(), String> {
    py.detach(|| {
        let start = std::time::Instant::now();
        loop {
            std::thread::sleep(std::time::Duration::from_millis(20));
            let mut s = session().lock();
            if let Some(i) = s.screenshot_results.iter().position(|r| r.0 == id) {
                return s.screenshot_results.remove(i).1;
            }
            if start.elapsed() > std::time::Duration::from_secs(3600) {
                return Err("the app did not finish within an hour".into());
            }
        }
    })
}

fn parse_interp(interp: &str) -> PyResult<aurafx_pipeline::animation::Interp> {
    aurafx_pipeline::animation::Interp::parse(interp).ok_or_else(|| {
        let all: Vec<&str> = aurafx_pipeline::animation::Interp::ALL.iter().map(|i| i.label()).collect();
        PyValueError::new_err(format!("unknown interpolation {interp:?}; one of: {}", all.join(", ")))
    })
}

/// Keyframe the current 3D view camera at animation time t (0..1); `interp` says how it moves
/// on to the next key: "linear", "step", "ease in", "ease out", "ease in-out", "exponential".
#[pyfunction]
#[pyo3(signature = (t, interp="linear"))]
fn key_camera(t: f64, interp: &str) -> PyResult<()> {
    let interp = parse_interp(interp)?;
    let mut s = session().lock();
    let cam = s.camera_state.ok_or_else(|| PyRuntimeError::new_err("no 3D view yet"))?;
    s.animation.key_camera_with(t.clamp(0.0, 1.0), cam, interp);
    Ok(())
}

/// Keyframe animation settings: number of frames, and whether time steps play along.
#[pyfunction]
#[pyo3(signature = (frames=None, time=None))]
fn animation(frames: Option<usize>, time: Option<bool>) -> (usize, bool) {
    let mut s = session().lock();
    if let Some(f) = frames {
        s.animation.frames = f.max(2);
    }
    if let Some(t) = time {
        s.animation.animate_time = t;
    }
    (s.animation.frames, s.animation.animate_time)
}

/// Show the keyframe animation at time t (0..1).
#[pyfunction]
fn animation_time(t: f64) {
    let mut s = session().lock();
    if let Some(c) = s.apply_animation(t) {
        s.pending_camera = Some(c);
    }
}

/// Remove every camera keyframe and parameter track.
#[pyfunction]
fn clear_keyframes() {
    let mut s = session().lock();
    s.animation.camera.clear();
    s.animation.tracks.clear();
}

#[pyfunction]
fn clear() {
    let mut s = session().lock();
    let ids: Vec<NodeId> = s.nodes.keys().copied().collect();
    for id in ids {
        s.remove_node(id);
    }
}

/// Names of the available color maps (built-in and user presets).
#[pyfunction]
fn colormaps() -> Vec<String> {
    aurafx_pipeline::colormap::names()
}

/// Load ParaView color map presets (.json) as user presets. Returns their names.
#[pyfunction]
fn import_colormaps(path: String) -> PyResult<Vec<String>> {
    let text = std::fs::read_to_string(&path).map_err(err)?;
    let maps = aurafx_pipeline::colormap::from_paraview_json(&text).map_err(err)?;
    let names = maps.iter().map(|m| m.name.clone()).collect();
    for m in maps {
        aurafx_pipeline::colormap::register_user(m);
    }
    Ok(names)
}

/// Define (or replace) a user color map from [[t, r, g, b], ...] with values in 0..1.
#[pyfunction]
fn define_colormap(name: String, points: Vec<[f64; 4]>) -> PyResult<String> {
    if points.len() < 2 {
        return Err(PyValueError::new_err("a color map needs at least two points"));
    }
    let stops = points.iter().map(|p| (p[0] as f32, [p[1] as f32, p[2] as f32, p[3] as f32])).collect();
    aurafx_pipeline::colormap::register_user(aurafx_pipeline::colormap::ColorMap::new(name.clone(), stops));
    Ok(name)
}

// ------------------------------------------------------------------ CAD

fn cad_err(e: impl std::fmt::Display) -> PyErr {
    PyValueError::new_err(format!("{e:#}"))
}

/// One edit of a CAD Model node's document: operation name and JSON arguments; returns JSON.
#[pyfunction]
fn _cad_edit(node: PyRef<PyNode>, op: String, args: String) -> PyResult<String> {
    let args: serde_json::Value = serde_json::from_str(&args).map_err(cad_err)?;
    let out = session().lock().cad_edit(node.id, &op, &args).map_err(cad_err)?;
    Ok(out.to_string())
}

/// The document of a CAD Model node as `.afxcad` text.
#[pyfunction]
fn _cad_document(node: PyRef<PyNode>) -> PyResult<String> {
    let doc = session().lock().cad_document(node.id).map_err(cad_err)?;
    Ok(aurafx_cad::CadFile::new(doc).to_json())
}

/// Replace the document of a CAD Model node by `.afxcad` text.
#[pyfunction]
fn _cad_set_document(node: PyRef<PyNode>, text: String) -> PyResult<()> {
    let doc = aurafx_cad::Document::from_afxcad(&text).map_err(cad_err)?;
    session().lock().cad_set_document(node.id, &doc);
    Ok(())
}

/// What the document has: parameters (with values), features, configurations, overrides.
#[pyfunction]
fn _cad_info(node: PyRef<PyNode>) -> PyResult<String> {
    let s = session().lock();
    let doc = s.cad_document(node.id).map_err(cad_err)?;
    let eff = s.cad_effective_document(node.id).ok();
    let vals = eff.as_ref().and_then(|d| d.parameter_values().ok());
    let params: Vec<serde_json::Value> = doc
        .parameters
        .iter()
        .map(|p| serde_json::json!({ "name": p.name, "expr": p.expr, "comment": p.comment, "value": vals.as_ref().and_then(|v| v.get(&p.name)) }))
        .collect();
    Ok(serde_json::json!({
        "name": doc.name,
        "parameters": params,
        "features": doc.feature_list(),
        "finish": doc.finish.len(),
        "components": doc.components.iter().map(|c| serde_json::json!({ "name": c.name, "id": c.id.0, "at": c.placement.translate, "rotate": c.placement.rotate_deg, "visible": c.visible, "part": c.document.name, "features": c.document.features.len() })).collect::<Vec<_>>(),
        "configurations": doc.configurations.iter().map(|c| c.name.clone()).collect::<Vec<_>>(),
        "active_configuration": doc.active_configuration,
        "overrides": s.cad_overrides(node.id),
    })
    .to_string())
}

/// Evaluate the part (with the node's overrides) now: volume, bounds and the faces.
#[pyfunction]
#[pyo3(signature = (node, resolution=64, kernel="Auto".to_string()))]
fn _cad_evaluate(node: PyRef<PyNode>, resolution: usize, kernel: String) -> PyResult<String> {
    // Inside the environment the build is shown too (it becomes the node's output).
    if session().lock().cad_editing() == Some(node.id) {
        let sum = session().lock().cad_env_evaluate(resolution).map_err(cad_err)?;
        let faces: Vec<serde_json::Value> = sum.faces.iter().enumerate().map(|(i, f)| serde_json::json!({ "id": i, "name": f.name, "kind": f.kind, "area": f.area, "centroid": f.centroid, "direction": f.direction, "feature": f.feature.0 })).collect();
        return Ok(serde_json::json!({ "kernel": sum.kernel, "volume": sum.volume, "area": sum.area, "centroid": sum.centroid, "bounds": [sum.bounds.0, sum.bounds.1], "faces": faces }).to_string());
    }
    let doc = session().lock().cad_effective_document(node.id).map_err(cad_err)?;
    let choice = aurafx_cad::KernelChoice::parse(&kernel).ok_or_else(|| PyValueError::new_err("kernel is Auto, Implicit or B-rep"))?;
    let ev = aurafx_filters::cad::evaluate_with(choice, &doc, resolution, true).map_err(cad_err)?;
    let faces: Vec<serde_json::Value> = ev
        .faces()
        .map(|(_, f)| {
            serde_json::json!({ "id": f.face_id, "name": f.name, "kind": f.kind.label(), "area": f.area, "centroid": f.centroid, "direction": f.direction, "feature": (f.id.0).0 })
        })
        .collect();
    let b = &ev.bodies[0];
    Ok(serde_json::json!({ "kernel": ev.kernel, "volume": ev.volume(), "area": b.area, "centroid": b.centroid, "bounds": [b.bounds.0, b.bounds.1], "faces": faces, "notes": ev.approximate }).to_string())
}

/// Open the CAD environment on a CAD Model node (the pipeline pauses).
#[pyfunction]
fn _cad_enter(node: PyRef<PyNode>) -> PyResult<()> {
    session().lock().cad_enter(node.id).map_err(cad_err)
}

fn conflict_json(c: &aurafx_pipeline::Conflict) -> serde_json::Value {
    serde_json::json!({
        "node": c.node, "node_name": c.node_name, "param": c.param, "value": c.value, "reference": c.reference,
        "kind": c.kind.label(), "may_reference": c.may_reference,
        "fix": c.fix.as_ref().map(|f| serde_json::json!({ "value": f.value, "reason": f.reason, "check": f.check })),
    })
}

/// The downstream check of the environment's working copy: what changed, the nodes below the
/// part, the conflicts with their default fixes, and the prompt for the assistant.
#[pyfunction]
fn _cad_check() -> PyResult<String> {
    let s = session().lock();
    let r = s.cad_check().map_err(cad_err)?;
    Ok(serde_json::json!({
        "changed": !r.diff.is_empty(),
        "diff": r.diff.describe(),
        "downstream": r.downstream,
        "conflicts": r.conflicts.iter().map(conflict_json).collect::<Vec<_>>(),
        "prompt": s.cad_check_prompt(&r),
    })
    .to_string())
}

/// Read fixes out of an assistant's answer (a JSON list of node, param, value, reason).
#[pyfunction]
fn _cad_parse_fixes(answer: String) -> PyResult<String> {
    let s = session().lock();
    let f = aurafx_pipeline::cad_check::parse_ai_fixes(&s, &answer).map_err(cad_err)?;
    Ok(serde_json::to_string(&f).map_err(cad_err)?)
}

/// Apply the environment's working copy to the node. `fix`: "default", "none" or a JSON list of
/// fixes; `run`: "now" or "later"; `keep` stays in the environment.
#[pyfunction]
#[pyo3(signature = (keep=false, fix="default".to_string(), run="now".to_string()))]
fn _cad_apply(keep: bool, fix: String, run: String) -> PyResult<String> {
    let mode = match fix.as_str() {
        "default" => aurafx_pipeline::FixMode::Default,
        "none" => aurafx_pipeline::FixMode::None,
        list => aurafx_pipeline::FixMode::Fixes(serde_json::from_str(list).map_err(|_| PyValueError::new_err("fix is 'default', 'none' or a list of fixes"))?),
    };
    let run = match run.as_str() {
        "now" => aurafx_pipeline::RunMode::Now,
        "later" => aurafx_pipeline::RunMode::Later,
        _ => return Err(PyValueError::new_err("run is 'now' or 'later'")),
    };
    let a = session().lock().cad_apply_with(keep, mode, run).map_err(cad_err)?;
    Ok(serde_json::json!({ "features": a.features, "kept_editing": a.kept_editing, "fixes": a.fixes, "held": a.held }).to_string())
}

/// Write the node's part as a CAD file by the path's extension (.afxcad, .step, .3mf, .stl, .obj, .sc):
/// returns {bytes, exact, note}. `kernel`: Auto, Implicit or B-rep.
#[pyfunction]
#[pyo3(signature = (node, path, kernel="Auto".to_string()))]
fn _cad_export(node: PyRef<PyNode>, path: String, kernel: String) -> PyResult<String> {
    let doc = session().lock().cad_effective_document(node.id).map_err(cad_err)?;
    let choice = aurafx_cad::KernelChoice::parse(&kernel).ok_or_else(|| PyValueError::new_err("kernel is Auto, Implicit or B-rep"))?;
    let r = aurafx_cad::io::export_file(&doc, std::path::Path::new(&path), choice).map_err(cad_err)?;
    Ok(serde_json::json!({ "bytes": r.bytes, "exact": r.exact, "note": r.note }).to_string())
}

/// Read a CAD file as `.afxcad` text (a solid file becomes a base feature).
#[pyfunction]
fn _cad_import(path: String) -> PyResult<String> {
    let doc = aurafx_cad::io::import_file(std::path::Path::new(&path)).map_err(cad_err)?;
    Ok(aurafx_cad::CadFile::new(doc).to_json())
}

/// The edges of the part as the exact kernel builds it: index, a point on the edge, length, the
/// faces on either side.
#[pyfunction]
fn _cad_edges(node: PyRef<PyNode>) -> PyResult<String> {
    let doc = session().lock().cad_effective_document(node.id).map_err(cad_err)?;
    let e = aurafx_cad::kernel::brep::list_edges(&doc).map_err(cad_err)?;
    Ok(serde_json::json!(e
        .iter()
        .map(|e| serde_json::json!({ "index": e.index, "point": e.mid, "length": e.length, "faces": e.faces, "straight": e.straight }))
        .collect::<Vec<_>>())
    .to_string())
}

/// A point on each named face of the built part (for picking faces by name).
#[pyfunction]
fn _cad_face_points(node: PyRef<PyNode>, names: Vec<String>) -> PyResult<String> {
    let doc = session().lock().cad_effective_document(node.id).map_err(cad_err)?;
    let ev = aurafx_filters::cad::evaluate_with(aurafx_cad::KernelChoice::Auto, &doc, 64, true).map_err(cad_err)?;
    let mut out = Vec::new();
    for n in &names {
        out.push(ev.face_pick(n).ok_or_else(|| PyValueError::new_err(format!("no face named '{n}' (cad.faces() lists them)")))?);
    }
    Ok(serde_json::json!(out).to_string())
}

/// Mass properties, or the measure between two faces.
#[pyfunction]
#[pyo3(signature = (node, what, a=None, b=None, density=7.85))]
fn _cad_measure(node: PyRef<PyNode>, what: String, a: Option<String>, b: Option<String>, density: f64) -> PyResult<String> {
    let doc = session().lock().cad_effective_document(node.id).map_err(cad_err)?;
    let ev = aurafx_filters::cad::evaluate_with(aurafx_cad::KernelChoice::Auto, &doc, 64, true).map_err(cad_err)?;
    match what.as_str() {
        "mass" => serde_json::to_string(&ev.mass_properties(density)).map_err(cad_err),
        "faces" => {
            let (a, b) = (a.unwrap_or_default(), b.unwrap_or_default());
            let m = aurafx_cad::measure_faces(&ev, &a, &b)
                .ok_or_else(|| PyValueError::new_err(format!("no faces named '{a}' and '{b}' (cad.faces() lists them)")))?;
            serde_json::to_string(&m).map_err(cad_err)
        }
        w => Err(PyValueError::new_err(format!("unknown measure '{w}'"))),
    }
}

/// The overlap volume of two CAD Model nodes, the second placed at `at` / `rotate`.
#[pyfunction]
#[pyo3(signature = (node, other, at=(0.0, 0.0, 0.0), rotate=(0.0, 0.0, 0.0), resolution=0.5))]
fn _cad_interference(node: PyRef<PyNode>, other: PyRef<PyNode>, at: (f64, f64, f64), rotate: (f64, f64, f64), resolution: f64) -> PyResult<f64> {
    let (da, db) = {
        let s = session().lock();
        (s.cad_effective_document(node.id).map_err(cad_err)?, s.cad_effective_document(other.id).map_err(cad_err)?)
    };
    let k = aurafx_cad::KernelChoice::Auto;
    let (ea, eb) =
        (aurafx_filters::cad::evaluate_with(k, &da, 64, true).map_err(cad_err)?, aurafx_filters::cad::evaluate_with(k, &db, 64, true).map_err(cad_err)?);
    let id = aurafx_cad::Placement::default();
    let pb = aurafx_cad::Placement { translate: [at.0, at.1, at.2], rotate_deg: [rotate.0, rotate.1, rotate.2] };
    Ok(aurafx_cad::interference_built(&ea, &id, &eb, &pb, resolution))
}

/// The pairs of parts of an assembly that overlap, with the overlap volume in mm3.
#[pyfunction]
#[pyo3(signature = (node, resolution=0.5))]
fn _cad_component_interference(node: PyRef<PyNode>, resolution: f64) -> PyResult<String> {
    let doc = session().lock().cad_effective_document(node.id).map_err(cad_err)?;
    let r = aurafx_cad::kernel::assemble::component_interference(aurafx_cad::KernelChoice::Auto, &doc, resolution).map_err(cad_err)?;
    Ok(serde_json::json!(r.iter().map(|(a, b, v)| serde_json::json!({ "a": a, "b": b, "volume": v })).collect::<Vec<_>>()).to_string())
}

/// Send a command to the app's window (the Apply button's dialog).
#[pyfunction]
fn _cad_app_command(cmd: String) {
    session().lock().app_commands.push(cmd);
}

/// Let the nodes held back by a postponed re-run run.
#[pyfunction]
fn _cad_run_held() {
    session().lock().cad_run_held();
}

/// Leave the environment without applying.
#[pyfunction]
fn _cad_discard() -> PyResult<()> {
    session().lock().cad_discard().map_err(cad_err)
}

/// The state of the environment: whether it is open on this node, changes, revision, rollback.
#[pyfunction]
fn _cad_state(node: PyRef<PyNode>) -> String {
    let s = session().lock();
    match s.cad_edit.as_ref().filter(|e| e.node == node.id) {
        Some(e) => serde_json::json!({ "editing": true, "changed": e.changed, "revision": e.revision, "rollback": e.rollback, "selected": e.selected.map(|i| i.0), "undo": e.undo.len(), "redo": e.redo.len(), "error": e.error }).to_string(),
        None => serde_json::json!({ "editing": false, "other": s.cad_editing() }).to_string(),
    }
}

/// Undo / redo / rollback / select inside the environment.
#[pyfunction]
#[pyo3(signature = (what, arg=None))]
fn _cad_env(what: String, arg: Option<String>) -> PyResult<bool> {
    let mut s = session().lock();
    Ok(match what.as_str() {
        "undo" => s.cad_env_undo().is_some(),
        "redo" => s.cad_env_redo().is_some(),
        "rollback" => {
            let n = arg
                .as_deref()
                .filter(|a| !a.is_empty())
                .map(|a| a.parse::<usize>())
                .transpose()
                .map_err(|_| PyValueError::new_err("rollback takes a number of features or None"))?;
            s.cad_env_rollback(n);
            true
        }
        "select" => {
            let id = match arg.as_deref().filter(|a| !a.is_empty()) {
                None => None,
                Some(name) => {
                    let id = s.cad_edit.as_ref().and_then(|e| e.document.features.iter().find(|f| f.name == name).map(|f| f.id));
                    if id.is_none() {
                        return Err(PyValueError::new_err(format!("no feature named '{name}'")));
                    }
                    id
                }
            };
            s.cad_env_select(id);
            true
        }
        w => return Err(PyValueError::new_err(format!("unknown environment action '{w}'"))),
    })
}

/// Check geometry commands: the bodies they make (id, name, history lines, faces as
/// (id, name), bounds), or the error with the line it is on.
#[pyfunction]
fn _mesh_check<'py>(py: Python<'py>, commands: String) -> PyResult<Bound<'py, PyList>> {
    let m = aurafx_mesh::Model::from_commands(&commands, None).map_err(|e| PyValueError::new_err(format!("{e:#}")))?;
    let out = PyList::empty(py);
    for (id, name, history, faces) in m.describe() {
        let d = PyDict::new(py);
        d.set_item("id", id)?;
        d.set_item("name", name)?;
        d.set_item("history", history)?;
        d.set_item("faces", faces)?;
        let (lo, hi) = m.bodies[&id].shape.bounds();
        d.set_item("bounds", (lo[0], hi[0], lo[1], hi[1], lo[2], hi[2]))?;
        out.append(d)?;
    }
    Ok(out)
}

/// `afx.mesh`: geometry built command by command, checked as it goes.
const MESH_API: &str = r#"
class _MeshGeometry:
    """Geometry built from commands. g.cmd('brick x 2 y 1 z 1') adds commands (checked at once),
    g.bodies() lists the bodies and their faces, g.journal() returns every command,
    g.result(resolution=64, output='Surface') adds the Geometry source to the pipeline."""
    def __init__(self, commands=""):
        self._cmds = []
        if commands:
            self.cmd(commands)
    def cmd(self, text):
        _mesh_check("\n".join(self._cmds + [text]))
        self._cmds.append(text)
        return self
    def journal(self):
        return "\n".join(self._cmds)
    def bodies(self):
        return _mesh_check(self.journal())
    def result(self, resolution=64, output="Surface"):
        return apply("geometry", None, commands=self.journal(), resolution=resolution, output=output)
    def __repr__(self):
        return "<geometry: %d bodies, %d commands>" % (len(self.bodies()), len(self._cmds))

class _MeshModule:
    """Geometry and meshing. afx.mesh.geometry() starts geometry built from commands."""
    def geometry(self, commands=""):
        return _MeshGeometry(commands)

mesh = _MeshModule()
"#;

/// `afx.cad_model()` and the `cad` object of a CAD Model node.
const CAD_API: &str = r#"
import json as _json

class _CadSketch:
    """A sketch described item by item; pass it to extrude() or revolve()."""
    def __init__(self, plane="XY", offset=0):
        self.plane, self.offset, self.items = plane, offset, []
    def rectangle(self, w, h, corner=None, center=None):
        d = {"w": w, "h": h}
        if center is not None: d["center"] = list(center)
        elif corner is not None: d["corner"] = list(corner)
        else: d["center"] = [0, 0]
        self.items.append({"rectangle": d}); return self
    def circle(self, r, center=(0, 0)):
        self.items.append({"circle": {"r": r, "center": list(center)}}); return self
    def polygon(self, points):
        self.items.append({"polygon": [list(p) for p in points]}); return self
    def regular(self, sides, r, center=(0, 0)):
        self.items.append({"regular": {"sides": sides, "r": r, "center": list(center)}}); return self
    def spec(self):
        return {"plane": self.plane, "offset": self.offset, "items": self.items}

class _CadApi:
    """The parametric document of a CAD Model node. Numbers can be expressions of the document's
    parameters ("width / 2"): the geometry then follows the parameter."""
    def __init__(self, node):
        self._node = node
    def edit(self, op, args=None, **kw):
        a = dict(args or {}); a.update(kw)
        return _json.loads(_cad_edit(self._node, op, _json.dumps(a)))
    # parameters and configurations
    def parameter(self, name, expr, comment=""):
        return self.edit("parameter", name=name, expr=expr, comment=comment)
    def remove_parameter(self, name):
        return self.edit("remove_parameter", name=name)
    def parameters(self):
        return {p["name"]: p["value"] for p in self.info()["parameters"]}
    def configuration(self, name, **overrides):
        return self.edit("configuration", name=name, overrides=overrides)
    def activate(self, name=None):
        return self.edit("activate", name=name)
    # solids
    def _prim(self, kind, name, op, at, rotate, blend, kw):
        a = dict(kw); a.update(kind=kind, op=op, at=list(at), rotate=list(rotate))
        if name: a["name"] = name
        if blend: a["blend"] = blend
        return self.edit("add", a)
    def box(self, size=(10, 10, 10), name=None, op="add", at=(0, 0, 0), rotate=(0, 0, 0), corner_radius=0, blend=0):
        return self._prim("box", name, op, at, rotate, blend, {"size": list(size), "corner_radius": corner_radius})
    def sphere(self, radius=5, name=None, op="add", at=(0, 0, 0), rotate=(0, 0, 0), blend=0):
        return self._prim("sphere", name, op, at, rotate, blend, {"radius": radius})
    def cylinder(self, radius=5, height=10, name=None, op="add", at=(0, 0, 0), rotate=(0, 0, 0), blend=0):
        return self._prim("cylinder", name, op, at, rotate, blend, {"radius": radius, "height": height})
    def cone(self, r_bottom=5, r_top=0, height=10, name=None, op="add", at=(0, 0, 0), rotate=(0, 0, 0), blend=0):
        return self._prim("cone", name, op, at, rotate, blend, {"r_bottom": r_bottom, "r_top": r_top, "height": height})
    def torus(self, major=10, minor=2, name=None, op="add", at=(0, 0, 0), rotate=(0, 0, 0), blend=0):
        return self._prim("torus", name, op, at, rotate, blend, {"major": major, "minor": minor})
    def hole(self, diameter=5, depth=10, kind="simple", name=None, at=(0, 0, 0), rotate=(0, 0, 0), **kw):
        """A hole cut downward (-z) from `at`: kind simple, drilled, counterbore (cb_diameter, cb_depth),
        countersink (cs_diameter, cs_angle) or tapped (pitch)."""
        k = dict(kw); k.update(diameter=diameter, depth=depth, hole=kind)
        return self._prim("hole", name, "cut", at, rotate, 0, k)
    def thread(self, major_diameter=8, pitch=1.25, length=10, internal=False, name=None, op="add", at=(0, 0, 0)):
        return self._prim("thread", name, op, at, (0, 0, 0), 0, {"major_diameter": major_diameter, "pitch": pitch, "length": length, "internal": internal})
    def sketch(self, plane="XY", offset=0):
        return _CadSketch(plane, offset)
    def extrude(self, sketch, depth=10, name=None, op="add", end="blind", draft=0, reverse=False, at=(0, 0, 0), top=None, bottom=None):
        """Extrude a sketch along its plane's normal. end: blind, midplane or through_all.
        top / bottom: {"fillet": r} or {"chamfer": d} on the cap edges."""
        a = {"sketch": sketch.spec() if hasattr(sketch, "spec") else sketch, "depth": depth, "op": op, "end": end, "draft": draft, "reverse": reverse, "at": list(at)}
        if name: a["name"] = name
        if top: a["top"] = top
        if bottom: a["bottom"] = bottom
        return self.edit("sketch_extrude", a)
    def revolve(self, sketch, angle=360, name=None, op="add", at=(0, 0, 0)):
        """Revolve a sketch about its local y axis (on the XZ plane: the world z axis); x is the radius."""
        a = {"sketch": sketch.spec() if hasattr(sketch, "spec") else sketch, "angle": angle, "op": op, "at": list(at)}
        if name: a["name"] = name
        return self.edit("sketch_revolve", a)
    # modifiers: on a feature (by name), or on the whole part when feature is None
    def modifier(self, kind, feature=None, **kw):
        m = {"kind": kind}; m.update(kw)
        return self.edit("modifier", feature=feature, modifier=m)
    def fillet(self, radius, feature=None):
        return self.modifier("Fillet", feature, radius=radius)
    def shell(self, thickness, feature=None, open_top=None):
        if open_top is None: return self.modifier("Shell", feature, thickness=thickness)
        return self.modifier("ShellOpen", feature, thickness=thickness, open_top=open_top)
    def linear_pattern(self, count, step, feature=None):
        return self.modifier("LinearPattern", feature, count=count, step=list(step))
    def grid_pattern(self, counts, step, feature=None):
        return self.modifier("GridPattern", feature, counts=list(counts), step=list(step))
    def circular_pattern(self, count, total_deg=360, axis="Z", feature=None):
        return self.modifier("CircularPatternAngle", feature, count=count, total_deg=total_deg, axis=axis)
    def mirror(self, plane="YZ", feature=None):
        return self.modifier("Mirror", feature, plane=plane)
    def split(self, plane="XY", offset=0, keep_positive=True, feature=None):
        return self.modifier("Split", feature, plane=plane, offset=offset, keep_positive=keep_positive)
    def scale(self, factor, feature=None):
        return self.modifier("Scale", feature, factor=factor)
    # edges, faces and measures of the exact part
    def edges(self):
        """The edges of the part as the exact (B-rep) kernel builds it: index, a point on the edge, length,
        straight, and the names of the faces on either side. Raises when the exact kernel cannot build it."""
        return _json.loads(_cad_edges(self._node))
    def _edge_points(self, edges):
        if isinstance(edges, tuple) and len(edges) == 2 and all(isinstance(e, str) for e in edges):
            edges = [e for e in self.edges() if edges[0] in e["faces"] and edges[1] in e["faces"]]
            if not edges: raise ValueError("no edge between those two faces")
            return [e["point"] for e in edges]
        all_edges = None
        pts = []
        for e in edges:
            if isinstance(e, int):
                if all_edges is None: all_edges = self.edges()
                pts.append(all_edges[e]["point"])
            elif isinstance(e, dict): pts.append(e["point"])
            else: pts.append(list(e))
        return pts
    def fillet_edges(self, edges, size=1, name=None):
        """Round picked edges (exact kernel): edges are indices from edges(), points on the edges, or
        a pair of face names ('Base z+', 'Bore side') for the edge between them."""
        return self.edit("edge_blend", edges=self._edge_points(edges), size=size, chamfer=False, **({"name": name} if name else {}))
    def chamfer_edges(self, edges, size=1, name=None):
        """Chamfer picked edges (see fillet_edges)."""
        return self.edit("edge_blend", edges=self._edge_points(edges), size=size, chamfer=True, **({"name": name} if name else {}))
    def shell_faces(self, faces=(), thickness=2, name=None):
        """Hollow the part so far with this wall, removing the picked faces (names from faces(), or points);
        no faces: a closed hollow body."""
        pts = [list(f) if not isinstance(f, str) else None for f in faces]
        names = [f for f in faces if isinstance(f, str)]
        if names:
            got = iter(_json.loads(_cad_face_points(self._node, names)))
            pts = [next(got) if p is None else p for p in pts]
        return self.edit("shell_faces", faces=pts, thickness=thickness, **({"name": name} if name else {}))
    def mass(self, density=7.85):
        """Volume, surface, center, bounds and mass (density in g/cm3) of the built part."""
        return _json.loads(_cad_measure(self._node, "mass", None, None, density))
    def measure_faces(self, a, b):
        """The distance between two parallel flat faces (or their centers), and the angle between them."""
        return _json.loads(_cad_measure(self._node, "faces", a, b))
    def interference(self, other, at=(0, 0, 0), rotate=(0, 0, 0), resolution=0.5):
        """The overlap volume with another CAD Model node placed at `at` (and rotated by `rotate` degrees)."""
        return _cad_interference(self._node, other._node if hasattr(other, "_node") else other, tuple(at), tuple(rotate), resolution)
    # more edge, face and body operations
    def mirror_about(self, origin, normal, name=None):
        """Mirror the part so far about a plane, e.g. a flat face: origin is a point on it (or a face name
        from faces()), normal its direction. The reflection is joined to the part."""
        if isinstance(origin, str): origin = _json.loads(_cad_face_points(self._node, [origin]))[0]
        return self.edit("mirror_plane", origin=list(origin), normal=list(normal), **({"name": name} if name else {}))
    def draft(self, faces, angle=3, neutral_origin=(0, 0, 0), neutral_normal=(0, 0, 1), name=None):
        """Taper side faces (names from faces(), or points) by `angle` degrees about a neutral plane they
        stay fixed on. The exact kernel builds it; the implicit one leaves the faces vertical and says so."""
        names = [f for f in faces if isinstance(f, str)]
        got = iter(_json.loads(_cad_face_points(self._node, names))) if names else iter(())
        pts = [next(got) if isinstance(f, str) else list(f) for f in faces]
        return self.edit("draft", faces=pts, angle=angle, neutral_origin=list(neutral_origin), neutral_normal=list(neutral_normal), **({"name": name} if name else {}))
    # assemblies: parts placed beside the part
    def component_add(self, name=None, template=None, file=None, document=None, at=(0, 0, 0), rotate=(0, 0, 0)):
        """Place another part: a template ('flange'), a CAD file (.afxcad, .step, .igs, .3mf, .sc), a document
        (a dict) or a blank part to model in with component_op(). at / rotate (degrees) place it."""
        a = {"at": list(at), "rotate": list(rotate)}
        if name: a["name"] = name
        if template: a["template"] = template
        if file: a["path"] = str(file)
        if document: a["document"] = document
        return self.edit("component_add", **a)
    def component_move(self, component, at=None, rotate=None, by=None):
        """Place a component at a position (and rotation in degrees), or move it by an offset."""
        a = {"component": component}
        if at is not None: a["at"] = list(at)
        if rotate is not None: a["rotate"] = list(rotate)
        if by is not None: a["by"] = list(by)
        return self.edit("component_move", **a)
    def component_remove(self, component):
        return self.edit("component_remove", component=component)
    def component_visible(self, component, visible=True):
        return self.edit("component_visible", component=component, visible=visible)
    def component_rename(self, component, to):
        return self.edit("component_rename", component=component, to=to)
    def component_op(self, component, op, **args):
        """Edit a component's own part with any operation of this API, e.g.
        component_op('Plate', 'add', kind='box', size=[40, 40, 5])."""
        return self.edit("component_op", {"component": component, "inner_op": op, "args": args})
    def components(self):
        """The placed parts: name, position, rotation, visibility."""
        return self.info().get("components", [])
    def component_interference(self, resolution=0.5):
        """The pairs of parts that overlap, with the overlap volume in mm3."""
        return _json.loads(_cad_component_interference(self._node, resolution))
    # the timeline
    def suppress(self, feature, suppressed=True):
        return self.edit("suppress", feature=feature, suppressed=suppressed)
    def remove(self, feature):
        return self.edit("remove", feature=feature)
    def rename(self, feature, to):
        return self.edit("rename", feature=feature, to=to)
    def move(self, feature, to):
        return self.edit("move", feature=feature, to=to)
    def bind(self, feature, path, expr):
        """Drive a numeric field of a feature by an expression; path is a JSON pointer such as /shape/size/0."""
        return self.edit("bind", feature=feature, path=path, expr=expr)
    def set(self, feature, path, value):
        return self.edit("set", feature=feature, path=path, value=value)
    def template(self, name):
        """Start from a template: enclosure, flange, knob, threaded rod, vase or bracket."""
        return self.edit("template", name=name)
    def clear(self, name="Part"):
        return self.edit("new", name=name)
    # reading
    def info(self):
        return _json.loads(_cad_info(self._node))
    def features(self):
        return self.info()["features"]
    def evaluate(self, resolution=64, kernel="Auto"):
        """Build the part now: volume, area, centroid, bounds, the faces (name, kind, area, centroid, direction),
        the kernel that built it and notes on what it could not build exactly."""
        return _json.loads(_cad_evaluate(self._node, resolution, kernel))
    def faces(self, resolution=64):
        return self.evaluate(resolution)["faces"]
    def volume(self, resolution=64):
        return self.evaluate(resolution)["volume"]
    def apply(self, keep_editing=False, fix="default", run="now"):
        """Inside the CAD environment: hand the part to the node and leave (keep_editing=True stays),
        after the downstream check. fix: "default" (apply the default fixes), "none" (leave what the
        check found as it is), a list of {"node", "param", "value"} fixes, or the text of an assistant's
        answer (a JSON list). run: "now", or "later" (the nodes below wait for run_held()).
        Returns {"features", "fixes", "held", "kept_editing"}. Outside the environment: bring the
        node up to date and return its summary."""
        if self.editing():
            if isinstance(fix, str) and fix not in ("default", "none"):
                fix = _cad_parse_fixes(fix)           # an assistant's answer
            elif not isinstance(fix, str):
                fix = _json.dumps(fix)
            return _json.loads(_cad_apply(keep_editing, fix, run))
        return self._node.update()
    def show_tab(self, name):
        """Show a tab of the CAD ribbon (Sketch, Features, Modify, Inspect, Files) in the window."""
        _cad_app_command("cad_tab " + name)
    def open_picker(self, kind="fillet"):
        """Open the edge or face picker in the window: fillet, chamfer, shell, draft, mirror or pattern."""
        _cad_app_command("cad_pick " + kind)
    def select_component(self, name):
        """Select a component in the feature tree (it shows its position and turn). In the window only."""
        _cad_app_command("cad_component " + name)
    def check_fit(self):
        """Run the Assemble tab's Check fit and show the overlaps. In the window only."""
        _cad_app_command("cad_fit")
    def sketch_new(self, plane="XY", offset=0, items=None):
        """Open the sketch canvas (in the window) on a new sketch on a plane, optionally with ready
        shapes (the items of cad.sketch()). Only useful from a script running in the app."""
        spec = {"plane": plane, "offset": offset}
        if items is not None: spec["items"] = items
        _cad_app_command("cad_sketch_new " + _json.dumps(spec))
    def sketch_edit(self, feature):
        """Open the sketch canvas on the sketch of an extruded or revolved feature."""
        _cad_app_command("cad_sketch_edit " + feature)
    def ask_apply(self, keep_editing=False):
        """What the Apply button does in the window: run the downstream check and open its dialog
        when the nodes below are affected (only useful from a script running in the app)."""
        _cad_app_command("cad_apply keep" if keep_editing else "cad_apply")
    def check(self):
        """The downstream check of the changes made in the environment: what changed, the nodes below
        the part, the conflicts (node, param, reference, what happened, the default fix) and the
        prompt to ask an assistant for more fixes."""
        return _json.loads(_cad_check())
    def run_held(self):
        """Run the nodes a postponed apply held back."""
        _cad_run_held()
    # the environment
    def editing(self):
        """True while the CAD environment is open on this node."""
        return _json.loads(_cad_state(self._node))["editing"]
    def state(self):
        return _json.loads(_cad_state(self._node))
    def discard(self):
        """Leave the environment without applying."""
        _cad_discard()
    def undo(self):
        return _cad_env("undo")
    def redo(self):
        return _cad_env("redo")
    def rollback(self, features=None):
        """Build only the first `features` features (None: all), like the timeline's rollback marker."""
        return _cad_env("rollback", None if features is None else str(features))
    def select(self, feature=None):
        return _cad_env("select", feature)
    # files
    def document(self):
        return _cad_document(self._node)
    def save(self, path, kernel="Auto"):
        """Write the part by the file's extension: .afxcad (the document), .step/.stp, .3mf, .stl, .obj
        or .sc (SolidCraft). Exact faces and edges when the B-rep kernel builds the part; otherwise a
        faceted tessellation (the result says which). Returns {"bytes", "exact", "note"}."""
        return _json.loads(_cad_export(self._node, str(path), kernel))
    def load(self, path):
        """Replace the document by a file: .afxcad, or a solid (.step/.stp, .igs, .3mf, .sc, .stl, .obj)
        as the base feature of a new document."""
        _cad_set_document(self._node, _cad_import(str(path))); return self._node
    def __repr__(self):
        i = self.info()
        return "<cad %r: %d features, %d parameters>" % (i["name"], len(i["features"]), len(i["parameters"]))

def cad_check(node=None):
    """The downstream check of the CAD environment that is open (see node.cad.check())."""
    return _json.loads(_cad_check())

def _edit_cad(node):
    """Open the CAD environment on a CAD Model node: the pipeline pauses, node.cad.* edits a
    working copy, node.cad.apply() hands it to the node and node.cad.discard() throws it away."""
    _cad_enter(node)
    return node.cad

def cad_model(source=None, template=None, name=None, **kw):
    """A parametric part. afx.cad_model() starts a plate; afx.cad_model("part.afxcad") opens a
    document; template="flange" starts from a template. The returned node's .cad builds the part
    and node.set(width=50) changes a parameter."""
    node = apply("cad_model", None)
    if source is not None:
        node.cad.load(source)
    elif template is not None:
        node.cad.template(template)
    if name:
        node.cad.edit("rename_document", name=name)
    if kw:
        node.set(**kw)
    return node
"#;

/// Build a source function for each registered source so `afx.sphere(radius=2)` works.
fn make_source_fn(py: Python<'_>, id: &'static str) -> PyResult<Py<PyAny>> {
    let code = format!("def {id}(**kw):\n    import aurafx\n    return aurafx.apply('{id}', None, **kw)\n{id}.__doc__ = aurafx_doc\n");
    let doc = aurafx_filters::find(id).map(|f| f.description.to_string()).unwrap_or_default();
    let g = PyDict::new(py);
    g.set_item("aurafx_doc", doc)?;
    py.run(&std::ffi::CString::new(code).unwrap(), Some(&g), None)?;
    Ok(g.get_item(id)?.unwrap().unbind())
}

/// `afx.append_datasets(a, b, c, **kw)` for multi-input filters.
fn make_multi_fn(py: Python<'_>, id: &'static str) -> PyResult<Py<PyAny>> {
    let code = format!("def {id}(*inputs, **kw):\n    import aurafx\n    return aurafx.apply_multi('{id}', list(inputs), **kw)\n{id}.__doc__ = aurafx_doc\n");
    let doc = aurafx_filters::find(id).map(|f| f.description.to_string()).unwrap_or_default();
    let g = PyDict::new(py);
    g.set_item("aurafx_doc", doc)?;
    py.run(&std::ffi::CString::new(code).unwrap(), Some(&g), None)?;
    Ok(g.get_item(id)?.unwrap().unbind())
}

// ------------------------------------------------------------------ jobs

fn job_dict<'py>(py: Python<'py>, j: &aurafx_jobs::Job, tail: usize) -> PyResult<Bound<'py, PyDict>> {
    let d = PyDict::new(py);
    d.set_item("id", j.id)?;
    d.set_item("name", &j.spec.name)?;
    d.set_item("host", &j.spec.host.name)?;
    d.set_item("state", j.state.label())?;
    d.set_item("active", j.state.is_active())?;
    d.set_item(
        "exit_code",
        match j.state {
            aurafx_jobs::JobState::Finished(c) => Some(c),
            _ => None,
        },
    )?;
    d.set_item("seconds", j.runtime().map(|r| r.as_secs_f64()))?;
    let n = j.log.len();
    d.set_item("log", j.log.iter().skip(n.saturating_sub(tail)).cloned().collect::<Vec<_>>())?;
    d.set_item("outputs", j.outputs.iter().map(|p| p.display().to_string()).collect::<Vec<_>>())?;
    Ok(d)
}

fn case_options(kwargs: Option<&Bound<'_, PyDict>>) -> PyResult<aurafx_pipeline::cases::CaseOptions> {
    let mut o = aurafx_pipeline::cases::CaseOptions::default();
    if let Some(k) = kwargs {
        for (key, v) in k.iter() {
            let key: String = key.extract()?;
            match key.as_str() {
                "material" => o.material = v.extract()?,
                "axis" => {
                    o.axis = match v.extract::<String>() {
                        Ok(s) => ["x", "y", "z"].iter().position(|a| s.eq_ignore_ascii_case(a)).ok_or_else(|| PyValueError::new_err("axis is x, y or z"))?,
                        Err(_) => v.extract::<usize>()?.min(2),
                    }
                }
                "strain" => o.strain = v.extract()?,
                "steps" => o.steps = v.extract()?,
                "end_time" => o.end_time = v.extract()?,
                "fixed" => o.fixed = v.extract()?,
                "loaded" => o.loaded = v.extract()?,
                "mc_steps" => o.mc_steps = v.extract()?,
                "temperature" => o.temperature = v.extract()?,
                "dump_every" => o.dump_every = v.extract()?,
                "seed" => o.seed = v.extract()?,
                k => {
                    return Err(PyValueError::new_err(format!(
                        "unknown case option {k}: material, axis, strain, steps, end_time, fixed, loaded, mc_steps, temperature, dump_every, seed"
                    )))
                }
            }
        }
    }
    Ok(o)
}

/// Write a simulation case from a node's data into `folder`: solver "moose" (a grain mesh:
/// mesh.e with one block per grain, grain_euler.txt, grains.tex, slip_systems.txt,
/// polycrystal.i), "spparks" (a volume with FeatureIds: grains.spparks, in.potts) or "pinnacle"
/// (a grain mesh: mesh.e, orientations_points.csv, case.i). Options: material, axis ('x', 'y',
/// 'z'), strain, steps, end_time, fixed and loaded (set names), mc_steps, temperature,
/// dump_every, seed. Returns a dict: folder, files, input, command, outputs, template.
#[pyfunction]
#[pyo3(signature = (node, solver, folder, **kwargs))]
fn simulation_case<'py>(
    py: Python<'py>,
    node: PyRef<'_, PyNode>,
    solver: String,
    folder: String,
    kwargs: Option<&Bound<'_, PyDict>>,
) -> PyResult<Bound<'py, PyDict>> {
    let o = case_options(kwargs)?;
    let ds = node.output()?;
    let c = aurafx_pipeline::cases::write_case(&ds, &solver, std::path::Path::new(&folder), &o).map_err(err)?;
    let t = aurafx_jobs::templates::TEMPLATES.iter().find(|t| t.id == c.template).unwrap();
    let d = PyDict::new(py);
    d.set_item("folder", c.folder.display().to_string())?;
    d.set_item("files", c.files.iter().map(|f| f.display().to_string()).collect::<Vec<_>>())?;
    d.set_item("input", &c.input)?;
    d.set_item("command", aurafx_jobs::templates::render(t, t.default_exe, &c.input, 1, &aurafx_jobs::GpuRequest::None))?;
    d.set_item("outputs", &c.outputs)?;
    d.set_item("template", c.template)?;
    Ok(d)
}

/// Write a simulation case from a node's data (see simulation_case) and run it: on this
/// computer in `workdir`, or on `host` (the case files are uploaded to `workdir` there). `exe`
/// is the solver program (default: solid_mechanics-opt, spk_mpi, pinnacle), `np` the MPI
/// processes. The results open when the run ends (auto_load). Returns the job id.
#[pyfunction]
#[pyo3(signature = (node, solver, workdir, host=None, exe=None, np=1, auto_load=true, wait=false, slurm=None, **kwargs))]
#[allow(clippy::too_many_arguments)]
fn run_case(
    py: Python<'_>,
    node: PyRef<'_, PyNode>,
    solver: String,
    workdir: String,
    host: Option<String>,
    exe: Option<String>,
    np: u32,
    auto_load: bool,
    wait: bool,
    slurm: Option<std::collections::HashMap<String, String>>,
    kwargs: Option<&Bound<'_, PyDict>>,
) -> PyResult<u64> {
    let o = case_options(kwargs)?;
    let ds = node.output()?;
    let h = aurafx_jobs::resolve_host(host.as_deref());
    let name = format!("{} {}", solver, std::path::Path::new(&workdir).file_name().map(|f| f.to_string_lossy().to_string()).unwrap_or_default());
    let local = if h.is_local() { std::path::PathBuf::from(&workdir) } else { aurafx_jobs::cache_dir().join("cases").join(name.replace(' ', "_")) };
    let c = aurafx_pipeline::cases::write_case(&ds, &solver, &local, &o).map_err(err)?;
    let t = aurafx_jobs::templates::TEMPLATES.iter().find(|t| t.id == c.template).unwrap();
    let cmd = aurafx_jobs::templates::render(t, exe.as_deref().unwrap_or(t.default_exe), &c.input, np, &aurafx_jobs::GpuRequest::None);
    let mut spec = aurafx_jobs::JobSpec::new(name, cmd);
    spec.host = h;
    spec.workdir = workdir;
    spec.outputs = c.outputs.clone();
    spec.auto_load = auto_load;
    spec.inputs = if spec.host.is_local() { Vec::new() } else { c.files.clone() };
    if let Some(sl) = slurm {
        spec.scheduler = aurafx_jobs::Scheduler::Slurm {
            partition: sl.get("partition").cloned().unwrap_or_default(),
            time: sl.get("time").cloned().unwrap_or_else(|| "01:00:00".into()),
            nodes: sl.get("nodes").and_then(|v| v.parse().ok()).unwrap_or(1),
            tasks: np,
            extra: sl.get("extra").cloned().unwrap_or_default(),
        };
    }
    let jobs = aurafx_jobs::global();
    let id = aurafx_jobs::submit(&jobs, spec);
    if wait {
        loop {
            let active = jobs.lock().jobs.iter().find(|j| j.id == id).map(|j| !j.settled).unwrap_or(false);
            if !active {
                break;
            }
            py.detach(|| std::thread::sleep(std::time::Duration::from_millis(200)));
            py.check_signals()?;
        }
    }
    Ok(id)
}

/// Run a shell command as a job (locally, on an SSH host, or via Slurm).
/// gpu: None, "any", "nvidia", "amd", or e.g. "nvidia:0,1". Returns the job id.
#[pyfunction]
#[pyo3(signature = (command, host=None, workdir=None, gpu=None, outputs=None, auto_load=false, name=None, env=None, slurm=None, wait=false))]
#[allow(clippy::too_many_arguments)]
fn submit(
    py: Python<'_>,
    command: String,
    host: Option<String>,
    workdir: Option<String>,
    gpu: Option<String>,
    outputs: Option<String>,
    auto_load: bool,
    name: Option<String>,
    env: Option<std::collections::HashMap<String, String>>,
    slurm: Option<std::collections::HashMap<String, String>>,
    wait: bool,
) -> PyResult<u64> {
    let mut spec = aurafx_jobs::JobSpec::new(name.unwrap_or_else(|| command.split_whitespace().next().unwrap_or("job").to_string()), command);
    spec.host = aurafx_jobs::resolve_host(host.as_deref());
    if let Some(w) = workdir {
        spec.workdir = w;
    } else if !spec.host.is_local() {
        spec.workdir = "~".into();
    }
    spec.gpu = parse_gpu(gpu.as_deref())?;
    spec.outputs = outputs.unwrap_or_default();
    spec.auto_load = auto_load;
    spec.env = env.unwrap_or_default().into_iter().collect();
    if let Some(sl) = slurm {
        spec.scheduler = aurafx_jobs::Scheduler::Slurm {
            partition: sl.get("partition").cloned().unwrap_or_default(),
            time: sl.get("time").cloned().unwrap_or_else(|| "01:00:00".into()),
            nodes: sl.get("nodes").and_then(|v| v.parse().ok()).unwrap_or(1),
            tasks: sl.get("tasks").and_then(|v| v.parse().ok()).unwrap_or(1),
            extra: sl.get("extra").cloned().unwrap_or_default(),
        };
    }
    let jobs = aurafx_jobs::global();
    let id = aurafx_jobs::submit(&jobs, spec);
    if wait {
        loop {
            let active = jobs.lock().jobs.iter().find(|j| j.id == id).map(|j| !j.settled).unwrap_or(false);
            if !active {
                break;
            }
            py.detach(|| std::thread::sleep(std::time::Duration::from_millis(200)));
            py.check_signals()?;
        }
        py.detach(|| std::thread::sleep(std::time::Duration::from_millis(200)));
    }
    Ok(id)
}

fn parse_gpu(g: Option<&str>) -> PyResult<aurafx_jobs::GpuRequest> {
    use aurafx_jobs::GpuRequest;
    let Some(g) = g else { return Ok(GpuRequest::None) };
    let (kind, ids) = g.split_once(':').unwrap_or((g, "0"));
    let ids: Vec<u32> = ids.split(',').filter_map(|s| s.trim().parse().ok()).collect();
    Ok(match kind.to_ascii_lowercase().as_str() {
        "" | "none" => GpuRequest::None,
        "any" | "gpu" => GpuRequest::Any(ids),
        "nvidia" | "cuda" => GpuRequest::Nvidia(ids),
        "amd" | "rocm" | "hip" => GpuRequest::Amd(ids),
        _ => return Err(PyValueError::new_err("gpu must be 'any', 'nvidia', 'amd' optionally followed by ':0,1'")),
    })
}

/// Parameter sweep: one job per value; "{var}" in the command is replaced and each run gets
/// its own folder <workdir>/<var>_<value>. Returns the job ids.
#[pyfunction]
#[pyo3(signature = (command, var, values, host=None, workdir=None, gpu=None, outputs=None, auto_load=false))]
#[allow(clippy::too_many_arguments)]
fn sweep(
    command: String,
    var: String,
    values: Vec<Bound<'_, PyAny>>,
    host: Option<String>,
    workdir: Option<String>,
    gpu: Option<String>,
    outputs: Option<String>,
    auto_load: bool,
) -> PyResult<Vec<u64>> {
    let vals: Vec<String> = values.iter().map(|v| v.str().map(|s| s.to_string())).collect::<PyResult<_>>()?;
    let mut base = aurafx_jobs::JobSpec::new(format!("sweep {var}"), command);
    base.host = aurafx_jobs::resolve_host(host.as_deref());
    if let Some(w) = workdir {
        base.workdir = w;
    }
    base.gpu = parse_gpu(gpu.as_deref())?;
    base.outputs = outputs.unwrap_or_default();
    base.auto_load = auto_load;
    let jobs = aurafx_jobs::global();
    Ok(aurafx_jobs::sweep(&base, &var, &vals).into_iter().map(|s| aurafx_jobs::submit(&jobs, s)).collect())
}

/// Status of a job (dict with state, exit_code, log tail, outputs).
#[pyfunction]
#[pyo3(signature = (id, tail=50))]
fn job<'py>(py: Python<'py>, id: u64, tail: usize) -> PyResult<Bound<'py, PyDict>> {
    let jobs = aurafx_jobs::global();
    let m = jobs.lock();
    let j = m.jobs.iter().find(|j| j.id == id).ok_or_else(|| PyKeyError::new_err(format!("no job {id}")))?;
    job_dict(py, j, tail)
}

#[pyfunction]
fn jobs<'py>(py: Python<'py>) -> PyResult<Bound<'py, PyList>> {
    let jm = aurafx_jobs::global();
    let m = jm.lock();
    let l = PyList::empty(py);
    for j in &m.jobs {
        l.append(job_dict(py, j, 5)?)?;
    }
    Ok(l)
}

#[pyfunction]
fn cancel(id: u64) -> PyResult<()> {
    aurafx_jobs::cancel(&aurafx_jobs::global(), id).map_err(err)
}

/// GPUs on a host (local by default): list of dicts.
#[pyfunction]
#[pyo3(signature = (host=None))]
fn gpus<'py>(py: Python<'py>, host: Option<String>) -> PyResult<Bound<'py, PyList>> {
    let h = aurafx_jobs::resolve_host(host.as_deref());
    let list = py.detach(|| aurafx_jobs::gpu::query(&h));
    let l = PyList::empty(py);
    for g in list {
        let d = PyDict::new(py);
        d.set_item("vendor", format!("{:?}", g.vendor))?;
        d.set_item("index", g.index)?;
        d.set_item("name", g.name)?;
        d.set_item("memory_total_mb", g.memory_total_mb)?;
        d.set_item("memory_used_mb", g.memory_used_mb)?;
        d.set_item("utilization", g.utilization)?;
        d.set_item("temperature_c", g.temperature_c)?;
        l.append(d)?;
    }
    Ok(l)
}

#[pyfunction]
fn hosts() -> Vec<String> {
    aurafx_jobs::hosts().into_iter().map(|h| h.name).collect()
}

/// Copy a file from a host into the local cache and open it.
#[pyfunction]
fn load_remote(host: String, path: String) -> PyResult<PyNode> {
    let h = aurafx_jobs::resolve_host(Some(&host));
    let name = std::path::Path::new(&path).file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or_else(|| "remote".into());
    let dest = aurafx_jobs::cache_dir().join("remote").join(h.name.replace(['/', '@', ':'], "_")).join(name);
    h.fetch(&path, &dest).map_err(err)?;
    let id = session().lock().open_file(&dest).map_err(err)?;
    Ok(PyNode { id })
}

pub fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(submit, m)?)?;
    m.add_function(wrap_pyfunction!(job, m)?)?;
    m.add_function(wrap_pyfunction!(sweep, m)?)?;
    m.add_function(wrap_pyfunction!(jobs, m)?)?;
    m.add_function(wrap_pyfunction!(cancel, m)?)?;
    m.add_function(wrap_pyfunction!(gpus, m)?)?;
    m.add_function(wrap_pyfunction!(hosts, m)?)?;
    m.add_function(wrap_pyfunction!(load_remote, m)?)?;
    m.add_class::<PyNode>()?;
    m.add_class::<FilterCall>()?;
    m.add_function(wrap_pyfunction!(load, m)?)?;
    m.add_function(wrap_pyfunction!(servers, m)?)?;
    m.add_function(wrap_pyfunction!(lighting, m)?)?;
    m.add_function(wrap_pyfunction!(export_scene, m)?)?;
    m.add_function(wrap_pyfunction!(apply, m)?)?;
    m.add_function(wrap_pyfunction!(apply_multi, m)?)?;
    m.add_function(wrap_pyfunction!(nodes, m)?)?;
    m.add_function(wrap_pyfunction!(active, m)?)?;
    m.add_function(wrap_pyfunction!(get, m)?)?;
    m.add_function(wrap_pyfunction!(run, m)?)?;
    m.add_function(wrap_pyfunction!(describe, m)?)?;
    m.add_function(wrap_pyfunction!(filters, m)?)?;
    m.add_function(wrap_pyfunction!(help_filter, m)?)?;
    m.add_function(wrap_pyfunction!(time, m)?)?;
    m.add_function(wrap_pyfunction!(times, m)?)?;
    m.add_function(wrap_pyfunction!(reset_camera, m)?)?;
    m.add_function(wrap_pyfunction!(view, m)?)?;
    m.add_function(wrap_pyfunction!(screenshot, m)?)?;
    m.add_function(wrap_pyfunction!(clear, m)?)?;
    m.add_function(wrap_pyfunction!(save_animation, m)?)?;
    m.add_function(wrap_pyfunction!(show_panel, m)?)?;
    m.add_function(wrap_pyfunction!(annotate, m)?)?;
    m.add_function(wrap_pyfunction!(key_camera, m)?)?;
    m.add_function(wrap_pyfunction!(import_colormaps, m)?)?;
    m.add_function(wrap_pyfunction!(define_colormap, m)?)?;
    m.add_function(wrap_pyfunction!(animation, m)?)?;
    m.add_function(wrap_pyfunction!(animation_time, m)?)?;
    m.add_function(wrap_pyfunction!(clear_keyframes, m)?)?;
    m.add_function(wrap_pyfunction!(clear_annotations, m)?)?;
    m.add_function(wrap_pyfunction!(time_label, m)?)?;
    m.add_function(wrap_pyfunction!(set_theme, m)?)?;
    m.add_function(wrap_pyfunction!(split_view, m)?)?;
    m.add_function(wrap_pyfunction!(link_views, m)?)?;
    m.add_function(wrap_pyfunction!(match_views, m)?)?;
    m.add_function(wrap_pyfunction!(active_view, m)?)?;
    m.add_function(wrap_pyfunction!(ask, m)?)?;
    m.add_function(wrap_pyfunction!(undo, m)?)?;
    m.add_function(wrap_pyfunction!(select, m)?)?;
    m.add_function(wrap_pyfunction!(selection, m)?)?;
    m.add_function(wrap_pyfunction!(clear_selection, m)?)?;
    m.add_function(wrap_pyfunction!(extract_selection, m)?)?;
    m.add_function(wrap_pyfunction!(probe, m)?)?;
    m.add_function(wrap_pyfunction!(redo, m)?)?;
    m.add_function(wrap_pyfunction!(save_state, m)?)?;
    m.add_function(wrap_pyfunction!(load_state, m)?)?;
    m.add_function(wrap_pyfunction!(import_pipeline, m)?)?;
    m.add_function(wrap_pyfunction!(simulation_case, m)?)?;
    m.add_function(wrap_pyfunction!(run_case, m)?)?;
    m.add_function(wrap_pyfunction!(simulation_template, m)?)?;
    m.add_function(wrap_pyfunction!(layout, m)?)?;
    m.add_function(wrap_pyfunction!(hide_panel, m)?)?;
    m.add_function(wrap_pyfunction!(plot, m)?)?;
    m.add_function(wrap_pyfunction!(clear_plots, m)?)?;
    m.add_function(wrap_pyfunction!(over_time, m)?)?;
    m.add_function(wrap_pyfunction!(temporal_statistics, m)?)?;
    m.add_function(wrap_pyfunction!(points_from, m)?)?;
    m.add_function(wrap_pyfunction!(colormaps, m)?)?;
    let py = m.py();
    for f in aurafx_filters::registry().iter() {
        if f.is_source {
            m.add(f.id, make_source_fn(py, f.id)?)?;
        } else if f.multi.is_some() {
            m.add(f.id, make_multi_fn(py, f.id)?)?;
        }
    }
    m.add_class::<crate::data::PyData>()?;
    m.add_class::<crate::data::ArrayMap>()?;
    m.add_function(wrap_pyfunction!(crate::data::make_points, m)?)?;
    m.add_function(wrap_pyfunction!(crate::data::make_polydata, m)?)?;
    m.add_function(wrap_pyfunction!(crate::data::make_image, m)?)?;
    m.add_function(wrap_pyfunction!(crate::data::make_grid, m)?)?;
    m.add_function(wrap_pyfunction!(crate::data::make_multiblock, m)?)?;
    m.add_function(wrap_pyfunction!(crate::plugins::_register_filter, m)?)?;
    m.add_function(wrap_pyfunction!(crate::plugins::_register_reader, m)?)?;
    m.add_function(wrap_pyfunction!(crate::plugins::load_plugin, m)?)?;
    m.add_function(wrap_pyfunction!(_filter_kind, m)?)?;
    m.add_function(wrap_pyfunction!(plugins, m)?)?;
    m.add_function(wrap_pyfunction!(_mesh_check, m)?)?;
    m.add_function(wrap_pyfunction!(_cad_edit, m)?)?;
    m.add_function(wrap_pyfunction!(_cad_enter, m)?)?;
    m.add_function(wrap_pyfunction!(_cad_apply, m)?)?;
    m.add_function(wrap_pyfunction!(_cad_check, m)?)?;
    m.add_function(wrap_pyfunction!(_cad_parse_fixes, m)?)?;
    m.add_function(wrap_pyfunction!(_cad_run_held, m)?)?;
    m.add_function(wrap_pyfunction!(_cad_app_command, m)?)?;
    m.add_function(wrap_pyfunction!(_cad_export, m)?)?;
    m.add_function(wrap_pyfunction!(_cad_edges, m)?)?;
    m.add_function(wrap_pyfunction!(_cad_face_points, m)?)?;
    m.add_function(wrap_pyfunction!(_cad_measure, m)?)?;
    m.add_function(wrap_pyfunction!(assistant_status, m)?)?;
    m.add_function(wrap_pyfunction!(_cad_interference, m)?)?;
    m.add_function(wrap_pyfunction!(_cad_component_interference, m)?)?;
    m.add_function(wrap_pyfunction!(_cad_import, m)?)?;
    m.add_function(wrap_pyfunction!(_cad_discard, m)?)?;
    m.add_function(wrap_pyfunction!(_cad_state, m)?)?;
    m.add_function(wrap_pyfunction!(_cad_env, m)?)?;
    m.add_function(wrap_pyfunction!(_cad_document, m)?)?;
    m.add_function(wrap_pyfunction!(_cad_set_document, m)?)?;
    m.add_function(wrap_pyfunction!(_cad_info, m)?)?;
    m.add_function(wrap_pyfunction!(_cad_evaluate, m)?)?;
    // Decorators (@afx.filter, @afx.source, @afx.reader) and helpers are plain Python.
    m.py().run(&std::ffi::CString::new(crate::plugins::PY_API).unwrap(), Some(&m.dict()), None)?;
    m.py().run(&std::ffi::CString::new(MESH_API).unwrap(), Some(&m.dict()), None)?;
    m.py().run(&std::ffi::CString::new(CAD_API).unwrap(), Some(&m.dict()), None)?;
    m.add("__version__", env!("CARGO_PKG_VERSION"))?;
    m.add("__doc__", "auraFX scripting: afx.load(path), node.slice(...), node.color_by('T'), afx.screenshot('out.png'). See afx.filters().")?;
    let _ = PyTuple::empty(py);
    Ok(())
}
