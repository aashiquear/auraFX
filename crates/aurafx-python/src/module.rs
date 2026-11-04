//! The `aurafx` Python module.

use crate::session;
use aurafx_data::{ArrayData, DataSet};
use aurafx_filters::Association;
use aurafx_pipeline::session::CameraRequest;
use aurafx_pipeline::{ColorBy, NodeId, Representation};
use numpy::{PyArray1, PyArray2, PyArrayMethods, PyUntypedArrayMethods};
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
            "color_by" => {
                if let Some(n) = val.as_str() {
                    let cb = resolve_color_by(&s, id, n, None)?;
                    s.color_by(id, Some(cb));
                }
            }
            "colormap" | "cmap" => {
                let name = val.as_str().unwrap_or("Viridis").to_string();
                s.set_display(id, |d| d.colormap = name);
            }
            "opacity" => {
                let o = val.as_f64().unwrap_or(1.0) as f32;
                s.set_display(id, |d| d.opacity = o);
            }
            "representation" | "repr" => {
                let r = val.as_str().and_then(Representation::parse).ok_or_else(|| PyValueError::new_err("unknown representation"))?;
                s.set_display(id, |d| d.representation = r);
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
    #[pyo3(signature = (**kwargs))]
    fn __call__(&self, kwargs: Option<&Bound<'_, PyDict>>) -> PyResult<PyNode> {
        let id = session().lock().add_filter(&self.filter_id, self.input).map_err(err)?;
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

    fn show(&self) -> PyNode {
        session().lock().set_visible(self.id, true);
        self.clone()
    }

    fn hide(&self) -> PyNode {
        session().lock().set_visible(self.id, false);
        self.clone()
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
        let ds = self.output()?;
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

    /// Point coordinates as an (N, 3) NumPy array.
    #[getter]
    fn points<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let ds = self.output()?;
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
    fn output(&self) -> PyResult<std::sync::Arc<DataSet>> {
        let mut s = session().lock();
        s.update_blocking();
        let n = s.nodes.get(&self.id).ok_or_else(|| PyKeyError::new_err("node no longer exists"))?;
        n.output.clone().ok_or_else(|| PyRuntimeError::new_err(n.error.clone().unwrap_or_else(|| "node has no output".into())))
    }
}

fn array_to_numpy<'py>(py: Python<'py>, data: &ArrayData, ncomp: usize) -> PyResult<Bound<'py, PyAny>> {
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

fn numpy_to_array(v: &Bound<'_, PyAny>) -> PyResult<(ArrayData, usize)> {
    if let Ok(a) = v.cast::<PyArray2<f64>>() {
        let ro = a.readonly();
        let shape = ro.shape().to_vec();
        return Ok((ArrayData::F64(ro.as_slice()?.to_vec()), shape[1]));
    }
    if let Ok(a) = v.cast::<PyArray1<f64>>() {
        return Ok((ArrayData::F64(a.readonly().as_slice()?.to_vec()), 1));
    }
    // Fallback: go through Python to a flat list of floats.
    let np = v.py().import("numpy")?;
    let arr = np.call_method1("asarray", (v,))?.call_method1("astype", ("float64",))?;
    let shape: Vec<usize> = arr.getattr("shape")?.extract()?;
    let ncomp = if shape.len() >= 2 { shape[1] } else { 1 };
    let flat: Vec<f64> = arr.call_method0("ravel")?.call_method0("tolist")?.extract()?;
    Ok((ArrayData::F64(flat), ncomp))
}

// ------------------------------------------------------------------ module functions

#[pyfunction]
#[pyo3(signature = (path, **kwargs))]
fn load(path: String, kwargs: Option<&Bound<'_, PyDict>>) -> PyResult<PyNode> {
    let id = session().lock().open_file(&path).map_err(err)?;
    apply_kwargs(id, kwargs)?;
    Ok(PyNode { id })
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
    for f in aurafx_filters::registry() {
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
    session().lock().camera_request = Some(CameraRequest::ResetToBounds);
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
#[pyo3(signature = (path, width=None, height=None))]
fn screenshot(path: String, width: Option<u32>, height: Option<u32>) {
    let mut s = session().lock();
    s.screenshot_request = Some((std::path::PathBuf::from(path), width, height));
    s.revision += 1;
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

/// Bring a panel to the front: "3D View", "Python", "Assistant", "Simulations", "Spreadsheet", "Log", ...
#[pyfunction]
fn show_panel(name: String) {
    app_command(format!("show {name}"));
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

#[pyfunction]
fn clear() {
    let mut s = session().lock();
    let ids: Vec<NodeId> = s.nodes.keys().copied().collect();
    for id in ids {
        s.remove_node(id);
    }
}

#[pyfunction]
fn colormaps() -> Vec<&'static str> {
    aurafx_pipeline::colormap::names()
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
    m.add_function(wrap_pyfunction!(jobs, m)?)?;
    m.add_function(wrap_pyfunction!(cancel, m)?)?;
    m.add_function(wrap_pyfunction!(gpus, m)?)?;
    m.add_function(wrap_pyfunction!(hosts, m)?)?;
    m.add_function(wrap_pyfunction!(load_remote, m)?)?;
    m.add_class::<PyNode>()?;
    m.add_class::<FilterCall>()?;
    m.add_function(wrap_pyfunction!(load, m)?)?;
    m.add_function(wrap_pyfunction!(apply, m)?)?;
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
    m.add_function(wrap_pyfunction!(show_panel, m)?)?;
    m.add_function(wrap_pyfunction!(set_theme, m)?)?;
    m.add_function(wrap_pyfunction!(layout, m)?)?;
    m.add_function(wrap_pyfunction!(hide_panel, m)?)?;
    m.add_function(wrap_pyfunction!(plot, m)?)?;
    m.add_function(wrap_pyfunction!(clear_plots, m)?)?;
    m.add_function(wrap_pyfunction!(over_time, m)?)?;
    m.add_function(wrap_pyfunction!(colormaps, m)?)?;
    let py = m.py();
    for f in aurafx_filters::registry() {
        if f.is_source {
            m.add(f.id, make_source_fn(py, f.id)?)?;
        }
    }
    m.add("__version__", env!("CARGO_PKG_VERSION"))?;
    m.add("__doc__", "auraFX scripting: afx.load(path), node.slice(...), node.color_by('T'), afx.screenshot('out.png'). See afx.filters().")?;
    let _ = PyTuple::empty(py);
    Ok(())
}
