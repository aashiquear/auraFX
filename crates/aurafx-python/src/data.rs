//! `afx.Data`: a dataset handed to Python filters, plugins and programmable filters, with its
//! arrays as NumPy. Also the hook that runs Python inside the pipeline.

use crate::module::{array_to_numpy, numpy_to_array};
use anyhow::anyhow;
use aurafx_data::*;
use pyo3::exceptions::{PyKeyError, PyRuntimeError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::{PyDict, PyList};
use std::cell::Cell;
use std::sync::Arc;

thread_local! {
    /// Set while Python runs inside the pipeline (the session may be locked by this thread).
    static IN_PIPELINE: Cell<bool> = const { Cell::new(false) };
}

/// Whether the current thread is running Python on behalf of a pipeline filter.
pub(crate) fn in_pipeline() -> bool {
    IN_PIPELINE.with(|f| f.get())
}

/// Run Python on behalf of the pipeline: with the "inside a filter" flag set, and with a panic
/// (e.g. a forbidden `afx.load` inside a filter) turned into an error of that filter.
pub(crate) fn as_filter<R>(f: impl FnOnce() -> anyhow::Result<R>) -> anyhow::Result<R> {
    struct Reset(bool);
    impl Drop for Reset {
        fn drop(&mut self) {
            IN_PIPELINE.with(|x| x.set(self.0));
        }
    }
    let _reset = Reset(IN_PIPELINE.with(|x| x.replace(true)));
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(f)) {
        Ok(r) => r,
        Err(p) => {
            let msg = p.downcast_ref::<String>().cloned().or_else(|| p.downcast_ref::<&str>().map(|s| s.to_string())).unwrap_or_else(|| "panic".into());
            Err(anyhow!("{msg}"))
        }
    }
}

#[derive(Clone, Copy, PartialEq)]
enum Which {
    Points,
    Cells,
    Field,
}

/// A dataset: `points`, `point_data`, `cell_data`, `field_data` (NumPy), `n_points`, `n_cells`.
#[pyclass(name = "Data", module = "aurafx")]
pub struct PyData {
    pub ds: DataSet,
}

impl PyData {
    fn attrs(&self, w: Which) -> Option<&Attributes> {
        match w {
            Which::Points => self.ds.point_data(),
            Which::Cells => self.ds.cell_data(),
            Which::Field => Some(self.ds.field_data()),
        }
    }
    fn attrs_mut(&mut self, w: Which) -> Option<&mut Attributes> {
        match w {
            Which::Points => self.ds.point_data_mut(),
            Which::Cells => self.ds.cell_data_mut(),
            Which::Field => match &mut self.ds {
                DataSet::ImageData(d) => Some(&mut d.field_data),
                DataSet::RectilinearGrid(d) => Some(&mut d.field_data),
                DataSet::StructuredGrid(d) => Some(&mut d.field_data),
                DataSet::UnstructuredGrid(d) => Some(&mut d.field_data),
                DataSet::PolyData(d) => Some(&mut d.field_data),
                DataSet::MultiBlock(d) => Some(&mut d.field_data),
                DataSet::HyperTreeGrid(d) => Some(&mut d.field_data),
                DataSet::CellGrid(d) => Some(&mut d.field_data),
                DataSet::Table(_) => None,
            },
        }
    }
    fn len(&self, w: Which) -> Option<usize> {
        match w {
            Which::Points => Some(self.ds.n_points()),
            Which::Cells => Some(self.ds.n_cells()),
            Which::Field => None,
        }
    }
}

#[pymethods]
impl PyData {
    #[getter]
    fn n_points(&self) -> usize {
        self.ds.n_points()
    }
    #[getter]
    fn n_cells(&self) -> usize {
        self.ds.n_cells()
    }
    /// "Unstructured Grid", "Polygonal Mesh", "Image Data", ...
    #[getter(r#type)]
    fn type_name(&self) -> &'static str {
        self.ds.type_name()
    }
    /// (xmin, xmax, ymin, ymax, zmin, zmax)
    #[getter]
    fn bounds(&self) -> (f64, f64, f64, f64, f64, f64) {
        let b = self.ds.bounds();
        (b.min[0], b.max[0], b.min[1], b.max[1], b.min[2], b.max[2])
    }
    /// Point coordinates (N x 3, a copy). Assign to move the points of meshes with explicit points.
    #[getter]
    fn points<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let pts = match &self.ds {
            DataSet::MultiBlock(_) => {
                let merged = aurafx_filters::append::append_datasets(&[&self.ds]).map_err(|e| PyRuntimeError::new_err(e.to_string()))?;
                merged.points
            }
            d => d.points().unwrap_or_default(),
        };
        array_to_numpy(py, &ArrayData::F64(pts.data.to_f64_vec()), 3)
    }
    #[setter]
    fn set_points(&mut self, v: &Bound<'_, PyAny>) -> PyResult<()> {
        let (data, nc) = numpy_to_array(v)?;
        if nc != 3 {
            return Err(PyValueError::new_err("points must be an N x 3 array"));
        }
        let n = data.len() / 3;
        let pts = match &mut self.ds {
            DataSet::UnstructuredGrid(g) => &mut g.points,
            DataSet::PolyData(p) => &mut p.points,
            DataSet::StructuredGrid(g) => &mut g.points,
            _ => {
                // Implicit-point grids become explicit (image -> unstructured grid).
                let ug = self.ds.to_unstructured().ok_or_else(|| PyValueError::new_err("this data has no points to set"))?;
                self.ds = DataSet::UnstructuredGrid(ug);
                match &mut self.ds {
                    DataSet::UnstructuredGrid(g) => &mut g.points,
                    _ => unreachable!(),
                }
            }
        };
        if n != pts.len() {
            return Err(PyValueError::new_err(format!("expected {} points, got {n}", pts.len())));
        }
        *pts = Points::from_array(ArrayData::F64(data.to_f64_vec()));
        Ok(())
    }
    #[getter]
    fn point_data(slf: Py<Self>) -> ArrayMap {
        ArrayMap { owner: slf, which: Which::Points }
    }
    #[getter]
    fn cell_data(slf: Py<Self>) -> ArrayMap {
        ArrayMap { owner: slf, which: Which::Cells }
    }
    #[getter]
    fn field_data(slf: Py<Self>) -> ArrayMap {
        ArrayMap { owner: slf, which: Which::Field }
    }
    /// ParaView-style names (`output.PointData.append(array, "name")`).
    #[getter(PointData)]
    fn point_data_pv(slf: Py<Self>) -> ArrayMap {
        ArrayMap { owner: slf, which: Which::Points }
    }
    #[getter(CellData)]
    fn cell_data_pv(slf: Py<Self>) -> ArrayMap {
        ArrayMap { owner: slf, which: Which::Cells }
    }
    #[getter(FieldData)]
    fn field_data_pv(slf: Py<Self>) -> ArrayMap {
        ArrayMap { owner: slf, which: Which::Field }
    }
    /// Cells as (connectivity offsets, connectivity, VTK cell types) NumPy arrays.
    #[getter]
    fn cells<'py>(&self, py: Python<'py>) -> PyResult<(Bound<'py, PyAny>, Bound<'py, PyAny>, Bound<'py, PyAny>)> {
        let ug = match &self.ds {
            DataSet::MultiBlock(_) => aurafx_filters::append::append_datasets(&[&self.ds]).map_err(|e| PyRuntimeError::new_err(e.to_string()))?,
            d => d.to_unstructured().ok_or_else(|| PyValueError::new_err("this data has no cells"))?,
        };
        let c = &ug.cells;
        Ok((
            array_to_numpy(py, &ArrayData::I64(c.offsets.iter().map(|&o| o as i64).collect()), 1)?,
            array_to_numpy(py, &ArrayData::I64(c.connectivity.iter().map(|&o| o as i64).collect()), 1)?,
            array_to_numpy(py, &ArrayData::U8(c.types.iter().map(|t| t.vtk_id()).collect()), 1)?,
        ))
    }
    /// The pieces of a multi-block dataset (copies), or [self] for other data.
    fn blocks(&self) -> Vec<PyData> {
        match &self.ds {
            DataSet::MultiBlock(mb) => mb.leaves().into_iter().map(|(_, d)| PyData { ds: d.clone() }).collect(),
            d => vec![PyData { ds: d.clone() }],
        }
    }
    fn copy(&self) -> PyData {
        PyData { ds: self.ds.clone() }
    }
    fn __repr__(&self) -> String {
        format!("<afx.Data {}: {} points, {} cells>", self.ds.type_name(), self.ds.n_points(), self.ds.n_cells())
    }
}

/// Dict-like view of one attribute set; values are NumPy copies.
#[pyclass(module = "aurafx")]
pub struct ArrayMap {
    owner: Py<PyData>,
    which: Which,
}

#[pymethods]
impl ArrayMap {
    fn keys(&self, py: Python<'_>) -> Vec<String> {
        self.owner.borrow(py).attrs(self.which).map(|a| a.names().map(|s| s.to_string()).collect()).unwrap_or_default()
    }
    fn __len__(&self, py: Python<'_>) -> usize {
        self.keys(py).len()
    }
    fn __contains__(&self, py: Python<'_>, name: &str) -> bool {
        self.keys(py).iter().any(|k| k == name)
    }
    fn __iter__(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
        Ok(PyList::new(py, self.keys(py))?.call_method0("__iter__")?.unbind())
    }
    fn __getitem__<'py>(&self, py: Python<'py>, name: &str) -> PyResult<Bound<'py, PyAny>> {
        let d = self.owner.borrow(py);
        let a = d.attrs(self.which).and_then(|a| a.get(name)).ok_or_else(|| PyKeyError::new_err(format!("no array {name:?}")))?;
        array_to_numpy(py, &a.data, a.ncomp)
    }
    fn get<'py>(&self, py: Python<'py>, name: &str) -> PyResult<Option<Bound<'py, PyAny>>> {
        if self.__contains__(py, name) {
            self.__getitem__(py, name).map(Some)
        } else {
            Ok(None)
        }
    }
    fn __setitem__(&self, py: Python<'_>, name: &str, value: &Bound<'_, PyAny>) -> PyResult<()> {
        let (data, ncomp) = numpy_to_array(value)?;
        let mut d = self.owner.borrow_mut(py);
        if let Some(n) = d.len(self.which) {
            // A scalar broadcasts to every point/cell.
            let (data, ncomp) = if data.len() == 1 && n != 1 { (ArrayData::F64(vec![data.get_f64(0); n]), 1) } else { (data, ncomp) };
            if data.len() != n * ncomp {
                return Err(PyValueError::new_err(format!("{name}: expected {n} values per component, got {}", data.len() / ncomp.max(1))));
            }
            return insert(&mut d, self.which, name, data, ncomp);
        }
        insert(&mut d, self.which, name, data, ncomp)
    }
    fn __delitem__(&self, py: Python<'_>, name: &str) -> PyResult<()> {
        let mut d = self.owner.borrow_mut(py);
        d.attrs_mut(self.which).and_then(|a| a.remove(name)).map(|_| ()).ok_or_else(|| PyKeyError::new_err(format!("no array {name:?}")))
    }
    /// The units of an array ("K", "m/s"), or None.
    fn units(&self, py: Python<'_>, name: &str) -> PyResult<Option<String>> {
        let d = self.owner.borrow(py);
        let a = d.attrs(self.which).and_then(|a| a.get(name)).ok_or_else(|| PyKeyError::new_err(format!("no array {name:?}")))?;
        Ok(a.units.clone())
    }
    /// Set the units of an array (an empty string clears them).
    fn set_units(&self, py: Python<'_>, name: &str, units: &str) -> PyResult<()> {
        let mut d = self.owner.borrow_mut(py);
        let a = d.attrs_mut(self.which).and_then(|a| a.get_mut(name)).ok_or_else(|| PyKeyError::new_err(format!("no array {name:?}")))?;
        *a = a.clone().with_units(units);
        Ok(())
    }
    /// `append(array, name)`, the same as `self[name] = array`.
    fn append(&self, py: Python<'_>, value: &Bound<'_, PyAny>, name: &str) -> PyResult<()> {
        self.__setitem__(py, name, value)
    }
    fn __repr__(&self, py: Python<'_>) -> String {
        format!("{:?}", self.keys(py))
    }
}

fn insert(d: &mut PyData, w: Which, name: &str, data: ArrayData, ncomp: usize) -> PyResult<()> {
    let attrs = d.attrs_mut(w).ok_or_else(|| PyValueError::new_err("this data has no such attributes"))?;
    let set_scalars = attrs.scalars().is_none() && ncomp == 1;
    attrs.insert(Array::new(name, ncomp, data));
    if set_scalars {
        attrs.set_active(AttributeKind::Scalars, name);
    }
    Ok(())
}

// ------------------------------------------------------------------ constructors

fn cells_from_lists(lists: Option<Vec<Vec<u32>>>, ty: impl Fn(usize) -> CellType) -> Cells {
    let mut c = Cells::new();
    for l in lists.unwrap_or_default() {
        c.push(ty(l.len()), &l);
    }
    c
}

fn points_of(xyz: &Bound<'_, PyAny>) -> PyResult<Points> {
    let (data, nc) = numpy_to_array(xyz)?;
    if nc != 3 {
        return Err(PyValueError::new_err("points must be an N x 3 array"));
    }
    Ok(Points::from_array(ArrayData::F64(data.to_f64_vec())))
}

/// A point cloud (one vertex per point): `afx.make_points(xyz)`.
#[pyfunction]
pub fn make_points(xyz: &Bound<'_, PyAny>) -> PyResult<PyData> {
    Ok(PyData { ds: DataSet::PolyData(PolyData::from_points(points_of(xyz)?)) })
}

/// A surface: `afx.make_polydata(points, polys=[[0,1,2], ...], lines=[[...]], verts=[[...]])`.
#[pyfunction]
#[pyo3(signature = (points, polys=None, lines=None, verts=None))]
pub fn make_polydata(points: &Bound<'_, PyAny>, polys: Option<Vec<Vec<u32>>>, lines: Option<Vec<Vec<u32>>>, verts: Option<Vec<Vec<u32>>>) -> PyResult<PyData> {
    let pts = points_of(points)?;
    let n = pts.len() as u32;
    let p = PolyData {
        points: pts,
        polys: cells_from_lists(polys, |k| match k {
            3 => CellType::Triangle,
            4 => CellType::Quad,
            _ => CellType::Polygon,
        }),
        lines: cells_from_lists(lines, |k| if k == 2 { CellType::Line } else { CellType::PolyLine }),
        verts: cells_from_lists(verts, |k| if k == 1 { CellType::Vertex } else { CellType::PolyVertex }),
        ..Default::default()
    };
    for c in [&p.polys, &p.lines, &p.verts] {
        if c.connectivity.iter().any(|&i| i >= n) {
            return Err(PyValueError::new_err("a cell refers to a point that does not exist"));
        }
    }
    Ok(PyData { ds: DataSet::PolyData(p) })
}

/// A uniform grid: `afx.make_image(dims=(nx, ny, nz), spacing=(1, 1, 1), origin=(0, 0, 0))`.
#[pyfunction]
#[pyo3(signature = (dims, spacing=(1.0, 1.0, 1.0), origin=(0.0, 0.0, 0.0)))]
pub fn make_image(dims: (usize, usize, usize), spacing: (f64, f64, f64), origin: (f64, f64, f64)) -> PyData {
    let e = |n: usize| n.max(1) as i64 - 1;
    PyData {
        ds: DataSet::ImageData(ImageData {
            extent: [0, e(dims.0), 0, e(dims.1), 0, e(dims.2)],
            spacing: [spacing.0, spacing.1, spacing.2],
            origin: [origin.0, origin.1, origin.2],
            ..Default::default()
        }),
    }
}

/// An unstructured grid: `afx.make_grid(points, cells=[[...], ...], types=[12, ...])` with VTK
/// cell type numbers (10 tetra, 12 hexahedron, 5 triangle, ...); one type applies to all cells.
#[pyfunction]
pub fn make_grid(points: &Bound<'_, PyAny>, cells: Vec<Vec<u32>>, types: &Bound<'_, PyAny>) -> PyResult<PyData> {
    let pts = points_of(points)?;
    let types: Vec<u8> = match types.extract::<u8>() {
        Ok(t) => vec![t; cells.len()],
        Err(_) => types.extract()?,
    };
    if types.len() != cells.len() {
        return Err(PyValueError::new_err("one type per cell (or a single type for all)"));
    }
    let mut c = Cells::new();
    for (ids, t) in cells.iter().zip(types) {
        if ids.iter().any(|&i| i as usize >= pts.len()) {
            return Err(PyValueError::new_err("a cell refers to a point that does not exist"));
        }
        c.push(CellType::from_vtk(t).ok_or_else(|| PyValueError::new_err(format!("unknown VTK cell type {t}")))?, ids);
    }
    Ok(PyData { ds: DataSet::UnstructuredGrid(UnstructuredGrid { points: pts, cells: c, ..Default::default() }) })
}

/// A multi-block dataset from several pieces: `afx.make_multiblock([a, b], names=None)`.
#[pyfunction]
#[pyo3(signature = (blocks, names=None))]
pub fn make_multiblock(blocks: Vec<PyRef<PyData>>, names: Option<Vec<String>>) -> PyData {
    let names = names.unwrap_or_default();
    let mb = MultiBlock {
        blocks: blocks.iter().enumerate().map(|(i, b)| (names.get(i).cloned().unwrap_or_else(|| format!("Block {i}")), Some(b.ds.clone()))).collect(),
        ..Default::default()
    };
    PyData { ds: DataSet::MultiBlock(mb) }
}

// ------------------------------------------------------------------ pipeline hook

pub(crate) fn py_error(py: Python<'_>, e: PyErr) -> anyhow::Error {
    let tb = e.traceback(py).and_then(|t| t.format().ok()).unwrap_or_default();
    anyhow!("{tb}{e}")
}

/// Pull the dataset out of a Python result (`afx.Data`).
pub(crate) fn data_of(obj: &Bound<'_, PyAny>, what: &str) -> anyhow::Result<DataSet> {
    match obj.cast::<PyData>() {
        Ok(d) => Ok(d.borrow().ds.clone()),
        Err(_) => Err(anyhow!(
            "{what} must be afx.Data (got {}); build one with afx.make_points/make_polydata/make_image/make_grid",
            obj.get_type().name().map(|n| n.to_string()).unwrap_or_default()
        )),
    }
}

fn script_globals<'py>(py: Python<'py>) -> PyResult<Bound<'py, PyDict>> {
    let g = PyDict::new(py);
    g.set_item("__builtins__", py.import("builtins")?)?;
    let afx = py.import("aurafx")?;
    g.set_item("afx", &afx)?;
    g.set_item("aurafx", afx)?;
    if let Ok(np) = py.import("numpy") {
        g.set_item("np", np)?;
    }
    Ok(g)
}

fn run_task(py: Python<'_>, task: aurafx_filters::python::PyTask) -> anyhow::Result<DataSet> {
    use aurafx_filters::python::PyTask;
    let g = script_globals(py).map_err(|e| py_error(py, e))?;
    let code = |s: &str| std::ffi::CString::new(s).map_err(|_| anyhow!("the script contains a NUL character"));
    match task {
        PyTask::Filter { script, inputs } => {
            let ins: Vec<Py<PyData>> = inputs.iter().map(|d| Py::new(py, PyData { ds: (*d).clone() })).collect::<PyResult<_>>().map_err(|e| py_error(py, e))?;
            let output = Py::new(py, PyData { ds: inputs.first().map(|d| (*d).clone()).unwrap_or_else(|| DataSet::PolyData(PolyData::default())) })
                .map_err(|e| py_error(py, e))?;
            g.set_item("inputs", PyList::new(py, &ins).map_err(|e| py_error(py, e))?).map_err(|e| py_error(py, e))?;
            g.set_item("output", &output).map_err(|e| py_error(py, e))?;
            py.run(&code(script)?, Some(&g), None).map_err(|e| py_error(py, e))?;
            let out = g.get_item("output").map_err(|e| py_error(py, e))?.ok_or_else(|| anyhow!("the script removed `output`"))?;
            data_of(&out, "output")
        }
        PyTask::Source { script } => {
            g.set_item("output", py.None()).map_err(|e| py_error(py, e))?;
            py.run(&code(script)?, Some(&g), None).map_err(|e| py_error(py, e))?;
            let out = g.get_item("output").map_err(|e| py_error(py, e))?.filter(|o| !o.is_none()).ok_or_else(|| anyhow!("the script must assign `output`"))?;
            data_of(&out, "output")
        }
        PyTask::Calculator { expression, input, cells, result } => {
            let attrs = if cells { input.cell_data() } else { input.point_data() };
            if let Some(a) = attrs {
                for arr in a.iter().filter(|a| a.scalar_type() != ScalarType::String) {
                    if arr.name.chars().all(|c| c.is_alphanumeric() || c == '_') && !arr.name.starts_with(|c: char| c.is_ascii_digit()) {
                        g.set_item(&arr.name, array_to_numpy(py, &arr.data, arr.ncomp).map_err(|e| py_error(py, e))?).map_err(|e| py_error(py, e))?;
                    }
                }
            }
            if !cells {
                if let Some(p) = input.points() {
                    g.set_item("points", array_to_numpy(py, &ArrayData::F64(p.data.to_f64_vec()), 3).map_err(|e| py_error(py, e))?)
                        .map_err(|e| py_error(py, e))?;
                }
            }
            let out = Py::new(py, PyData { ds: input.clone() }).map_err(|e| py_error(py, e))?;
            g.set_item("inputs", PyList::new(py, [&out]).map_err(|e| py_error(py, e))?).map_err(|e| py_error(py, e))?;
            let v = py.eval(&code(expression)?, Some(&g), None).map_err(|e| py_error(py, e))?;
            let map = ArrayMap { owner: out.clone_ref(py), which: if cells { Which::Cells } else { Which::Points } };
            map.__setitem__(py, if result.is_empty() { "result" } else { result }, &v).map_err(|e| py_error(py, e))?;
            let ds = out.borrow(py).ds.clone();
            Ok(ds)
        }
        PyTask::Text { expression, input, time, step } => {
            // Point arrays, then cell arrays not named like a point array, over every block.
            let leaves: Vec<&DataSet> = match input {
                DataSet::MultiBlock(mb) => mb.leaves().into_iter().map(|l| l.1).collect(),
                d => vec![d],
            };
            let mut arrays: std::collections::BTreeMap<String, (usize, Vec<f64>)> = Default::default();
            let mut points: Vec<f64> = Vec::new();
            for which in 0..2 {
                let mut these: std::collections::BTreeMap<String, (usize, Vec<f64>)> = Default::default();
                for leaf in &leaves {
                    let attrs = if which == 0 { leaf.point_data() } else { leaf.cell_data() };
                    for arr in attrs.into_iter().flat_map(|a| a.iter()).filter(|a| a.scalar_type() != ScalarType::String) {
                        if arrays.contains_key(&arr.name) {
                            continue;
                        }
                        let e = these.entry(arr.name.clone()).or_insert((arr.ncomp, Vec::new()));
                        if e.0 == arr.ncomp {
                            e.1.extend(arr.data.to_f64_vec());
                        }
                    }
                    if which == 0 {
                        if let Some(p) = leaf.points() {
                            points.extend(p.data.to_f64_vec());
                        }
                    }
                }
                arrays.extend(these);
            }
            for (name, (nc, v)) in arrays {
                if name.chars().all(|c| c.is_alphanumeric() || c == '_') && !name.starts_with(|c: char| c.is_ascii_digit()) {
                    g.set_item(&name, array_to_numpy(py, &ArrayData::F64(v), nc).map_err(|e| py_error(py, e))?).map_err(|e| py_error(py, e))?;
                }
            }
            g.set_item("points", array_to_numpy(py, &ArrayData::F64(points), 3).map_err(|e| py_error(py, e))?).map_err(|e| py_error(py, e))?;
            let inp = Py::new(py, PyData { ds: input.clone() }).map_err(|e| py_error(py, e))?;
            g.set_item("inputs", PyList::new(py, [&inp]).map_err(|e| py_error(py, e))?).map_err(|e| py_error(py, e))?;
            g.set_item("t", time).map_err(|e| py_error(py, e))?;
            g.set_item("step", step).map_err(|e| py_error(py, e))?;
            let v = py.eval(&code(expression)?, Some(&g), None).map_err(|e| py_error(py, e))?;
            let text: String = v.str().map_err(|e| py_error(py, e))?.to_string();
            let mut t = Table::default();
            t.columns.insert(Array::new("Text", 1, ArrayData::String(vec![text])));
            Ok(DataSet::Table(t))
        }
    }
}

/// Let the pipeline run Python (Programmable Filter/Source, Python Calculator).
pub(crate) fn install_hook() {
    // Panics inside Python filters become filter errors; do not also print them.
    let prev = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        if !in_pipeline() {
            prev(info);
        }
    }));
    aurafx_filters::python::set_python_hook(Arc::new(|task| Python::attach(|py| as_filter(|| run_task(py, task)))));
}
