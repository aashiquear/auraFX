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
            "color_by" => {
                if let Some(n) = val.as_str() {
                    let cb = resolve_color_by(&s, id, n, None)?;
                    s.color_by(id, Some(cb));
                }
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
            _ => s.set_param_json(id, &key, &val),
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
    /// .xmf (Xdmf + .h5) .cgns .csv .stl .ply (format from the extension). all_times=True writes every
    /// time step: into one Exodus, VTKHDF or CGNS file (CGNS keeps one mesh), or as an EnSight or
    /// Xdmf series.
    #[pyo3(signature = (path, all_times=false))]
    fn save(&self, path: String, all_times: bool) -> PyResult<()> {
        let p = std::path::Path::new(&path);
        if all_times {
            if !aurafx_pipeline::Session::holds_all_times(p) {
                return Err(PyValueError::new_err("all_times=True needs an Exodus (.exo), VTKHDF (.vtkhdf), EnSight (.case), Xdmf (.xmf) or CGNS (.cgns) file name"));
            }
            return session().lock().save_all_times(self.id, p).map(|_| ()).map_err(err);
        }
        let ds = self.output()?;
        aurafx_io::writers::write_file(&ds, p).map_err(err)
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
    let mut s = session().lock();
    s.app_commands.push(cmd);
    s.revision += 1;
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

/// Send a request to the AI assistant panel. With agent=True it runs its code and iterates.
#[pyfunction]
#[pyo3(signature = (text, agent=false))]
fn ask(text: String, agent: bool) {
    app_command(format!("ask{} {text}", if agent { "-agent" } else { "" }));
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
            let active = jobs.lock().jobs.iter().find(|j| j.id == id).map(|j| j.state.is_active()).unwrap_or(false);
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
    // Decorators (@afx.filter, @afx.source, @afx.reader) and helpers are plain Python.
    m.py().run(&std::ffi::CString::new(crate::plugins::PY_API).unwrap(), Some(&m.dict()), None)?;
    m.add("__version__", env!("CARGO_PKG_VERSION"))?;
    m.add("__doc__", "auraFX scripting: afx.load(path), node.slice(...), node.color_by('T'), afx.screenshot('out.png'). See afx.filters().")?;
    let _ = PyTuple::empty(py);
    Ok(())
}
