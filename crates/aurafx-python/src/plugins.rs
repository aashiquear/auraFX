//! Python plugins: `.py` files that add filters, sources and readers with decorators.
//!
//! ```python
//! import aurafx as afx
//!
//! @afx.filter(category="My tools")
//! def scale_points(data, factor=2.0):
//!     """Scale the points about the origin."""
//!     data.points = data.points * factor
//!     return data
//!
//! @afx.reader(extensions=["xyzt"], label="XYZ with temperature")
//! def read_xyzt(path):
//!     a = np.loadtxt(path)
//!     d = afx.make_points(a[:, :3])
//!     d.point_data["T"] = a[:, 3]
//!     return d
//! ```
//!
//! Plugins are loaded from the user plugin folder (`<config>/aurafx/plugins`), from
//! `AURAFX_PLUGIN_PATH` and from paths added in the Plugins window, and are reloaded when the
//! file changes.

use crate::data::{as_filter, data_of, py_error, PyData};
use anyhow::{anyhow, Result};
use aurafx_data::DataSet;
use aurafx_filters::{Association, Extension, FilterDef, MultiInput, ParamKind, ParamSpec, ParamValue, Params};
use parking_lot::Mutex;
use pyo3::prelude::*;
use pyo3::types::{PyDict, PyList, PyTuple};
use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::SystemTime;

/// What a plugin file provided, or why it failed.
#[derive(Debug, Clone)]
pub struct PluginInfo {
    pub path: PathBuf,
    /// Filter and source ids it registered.
    pub filters: Vec<String>,
    /// Labels of the readers it registered.
    pub readers: Vec<String>,
    pub error: Option<String>,
    pub modified: Option<SystemTime>,
    pub enabled: bool,
}

static PLUGINS: Mutex<Vec<PluginInfo>> = Mutex::new(Vec::new());

thread_local! {
    /// Plugin file being executed (registrations are attributed to it).
    static CURRENT: RefCell<Option<String>> = const { RefCell::new(None) };
}

fn current_plugin() -> String {
    CURRENT.with(|c| c.borrow().clone()).unwrap_or_else(|| "<console>".into())
}

/// Loaded plugins (for the Plugins window).
pub fn list() -> Vec<PluginInfo> {
    PLUGINS.lock().clone()
}

/// The user plugin folder.
pub fn user_plugin_dir() -> Option<PathBuf> {
    directories::ProjectDirs::from("org", "aurafx", "auraFX").map(|d| d.config_dir().join("plugins"))
}

/// Plugin folders and files to scan: the user folder, `AURAFX_PLUGIN_PATH` and `extra`.
pub fn search_paths(extra: &[PathBuf]) -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = user_plugin_dir().into_iter().collect();
    if let Some(p) = std::env::var_os("AURAFX_PLUGIN_PATH") {
        v.extend(std::env::split_paths(&p));
    }
    v.extend(extra.iter().cloned());
    v
}

/// `.py` files in the given folders (and files given directly), sorted.
pub fn discover(paths: &[PathBuf]) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for p in paths {
        if p.is_file() {
            out.push(p.clone());
        } else if let Ok(rd) = std::fs::read_dir(p) {
            let mut files: Vec<PathBuf> = rd
                .flatten()
                .map(|e| e.path())
                .filter(|f| f.extension().is_some_and(|e| e == "py") && !f.file_name().is_some_and(|n| n.to_string_lossy().starts_with(['_', '.'])))
                .collect();
            files.sort();
            out.extend(files);
        }
    }
    out.dedup();
    out
}

fn mtime(p: &Path) -> Option<SystemTime> {
    std::fs::metadata(p).and_then(|m| m.modified()).ok()
}

fn unregister(path: &Path) -> Vec<String> {
    let key = path.display().to_string();
    let ids = aurafx_filters::unregister_plugin(&key);
    aurafx_io::unregister_readers(&key);
    ids
}

/// (Re)load one plugin file. Returns the ids of filters it had and has (for refreshing nodes).
pub fn load(path: &Path) -> (PluginInfo, Vec<String>) {
    let mut changed = unregister(path);
    let key = path.display().to_string();
    let mut info = PluginInfo { path: path.to_path_buf(), filters: Vec::new(), readers: Vec::new(), error: None, modified: mtime(path), enabled: true };
    let r: Result<()> = (|| {
        let code = std::fs::read_to_string(path).map_err(|e| anyhow!("cannot read {}: {e}", path.display()))?;
        let code = std::ffi::CString::new(code).map_err(|_| anyhow!("the file contains a NUL character"))?;
        CURRENT.with(|c| *c.borrow_mut() = Some(key.clone()));
        let r = Python::attach(|py| -> Result<()> {
            let g = PyDict::new(py);
            let stem = path.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
            let set = |k: &str, v: Bound<'_, PyAny>| g.set_item(k, v).map_err(|e| py_error(py, e));
            set("__name__", pyo3::types::PyString::new(py, &format!("aurafx_plugin_{stem}")).into_any())?;
            set("__file__", pyo3::types::PyString::new(py, &key).into_any())?;
            set("__builtins__", py.import("builtins").map_err(|e| py_error(py, e))?.into_any())?;
            let afx = py.import("aurafx").map_err(|e| py_error(py, e))?;
            set("afx", afx.clone().into_any())?;
            set("aurafx", afx.into_any())?;
            if let Ok(np) = py.import("numpy") {
                set("np", np.into_any())?;
            }
            py.run(&code, Some(&g), None).map_err(|e| py_error(py, e))
        });
        CURRENT.with(|c| *c.borrow_mut() = None);
        r
    })();
    info.filters = aurafx_filters::registry().iter().filter(|f| f.ext.as_ref().is_some_and(|e| e.plugin == key)).map(|f| f.id.to_string()).collect();
    info.readers = aurafx_io::plugin_readers().iter().filter(|r| r.plugin == key).map(|r| r.label.clone()).collect();
    if let Err(e) = r {
        info.error = Some(format!("{e:#}"));
    }
    changed.extend(info.filters.iter().cloned());
    changed.sort();
    changed.dedup();
    let mut all = PLUGINS.lock();
    all.retain(|p| p.path != path);
    all.push(info.clone());
    (info, changed)
}

/// Unload a plugin (its filters and readers disappear); it stays listed as disabled.
pub fn disable(path: &Path) -> Vec<String> {
    let ids = unregister(path);
    let mut all = PLUGINS.lock();
    if let Some(p) = all.iter_mut().find(|p| p.path == path) {
        p.enabled = false;
        p.filters.clear();
        p.readers.clear();
        p.error = None;
    } else {
        all.push(PluginInfo { path: path.to_path_buf(), filters: vec![], readers: vec![], error: None, modified: mtime(path), enabled: false });
    }
    ids
}

/// Bring the loaded plugins in line with the files on disk: load new and changed files,
/// unload deleted and disabled ones. Returns (messages, ids of filters that changed).
pub fn sync(paths: &[PathBuf], disabled: &[PathBuf]) -> (Vec<String>, Vec<String>) {
    let files = discover(paths);
    let mut msgs = Vec::new();
    let mut changed = Vec::new();
    let known = list();
    for f in &files {
        let off = disabled.iter().any(|d| d == f);
        let prev = known.iter().find(|p| &p.path == f);
        if off {
            if prev.is_none_or(|p| p.enabled) {
                changed.extend(disable(f));
                msgs.push(format!("Plugin disabled: {}", f.display()));
            }
            continue;
        }
        let stale = match prev {
            None => true,
            Some(p) => !p.enabled || p.modified != mtime(f),
        };
        if stale {
            let (info, ids) = load(f);
            changed.extend(ids);
            msgs.push(match &info.error {
                None => format!("Plugin loaded: {} ({} filters, {} readers)", f.display(), info.filters.len(), info.readers.len()),
                Some(e) => format!("Plugin {} failed: {}", f.display(), e.lines().last().unwrap_or("")),
            });
        }
    }
    // Files that disappeared.
    for p in known.iter().filter(|p| !files.contains(&p.path)) {
        changed.extend(unregister(&p.path));
        PLUGINS.lock().retain(|x| x.path != p.path);
        msgs.push(format!("Plugin removed: {}", p.path.display()));
    }
    changed.sort();
    changed.dedup();
    (msgs, changed)
}

/// Make the next [`sync`] reload every enabled plugin.
pub fn forget_mtimes() {
    for p in PLUGINS.lock().iter_mut() {
        p.modified = None;
    }
}

/// A starter plugin, written by "New plugin…".
pub const TEMPLATE: &str = r#""""My auraFX plugin. Saved changes are picked up automatically.

Filters get an afx.Data (NumPy arrays: data.points, data.point_data["T"], data.cell_data)
and return one. Keyword arguments become parameters in the Properties panel.
"""
import aurafx as afx
import numpy as np


@afx.filter(category="My filters")
def scale_array(data, array=afx.ArrayParam(), factor=2.0, name="scaled"):
    """Multiply an array by a factor."""
    if array is None:
        raise ValueError("choose an array")
    values = data.point_data[array] if array in data.point_data else data.cell_data[array]
    target = data.point_data if array in data.point_data else data.cell_data
    target[name] = values * factor
    return data


@afx.source(category="My sources")
def spiral(turns=3.0, points=500):
    """A 3D spiral of points."""
    t = np.linspace(0, turns * 2 * np.pi, int(points))
    d = afx.make_polydata(np.c_[np.cos(t), np.sin(t), t / (2 * np.pi)], lines=[list(range(len(t)))])
    d.point_data["t"] = t
    return d
"#;

// ------------------------------------------------------------------ registration (Python side)

fn leak(s: String) -> &'static str {
    Box::leak(s.into_boxed_str())
}

fn param_spec(d: &Bound<'_, PyDict>) -> PyResult<ParamSpec> {
    let get = |k: &str| d.get_item(k).ok().flatten();
    let key: String = get("key").map(|v| v.extract()).transpose()?.unwrap_or_default();
    let label: String = get("label").map(|v| v.extract()).transpose()?.unwrap_or_else(|| key.clone());
    let kind: String = get("kind").map(|v| v.extract()).transpose()?.unwrap_or_else(|| "str".into());
    let default = get("default");
    let f = |v: Option<Bound<'_, PyAny>>| v.and_then(|x| x.extract::<f64>().ok());
    Ok(match kind.as_str() {
        "bool" => ParamSpec::new(&key, &label, ParamKind::Bool, ParamValue::Bool(default.and_then(|v| v.extract().ok()).unwrap_or(false))),
        "int" => ParamSpec::new(
            &key,
            &label,
            ParamKind::Int {
                min: get("min").and_then(|v| v.extract().ok()).unwrap_or(i64::MIN / 2),
                max: get("max").and_then(|v| v.extract().ok()).unwrap_or(i64::MAX / 2),
            },
            ParamValue::Int(default.and_then(|v| v.extract().ok()).unwrap_or(0)),
        ),
        "float" => ParamSpec::new(
            &key,
            &label,
            ParamKind::Float { min: f(get("min")).unwrap_or(f64::NEG_INFINITY), max: f(get("max")).unwrap_or(f64::INFINITY), log: false },
            ParamValue::Float(f(default).unwrap_or(0.0)),
        ),
        "vec3" => {
            let v: [f64; 3] = default.and_then(|v| v.extract().ok()).unwrap_or([0.0; 3]);
            ParamSpec::new(&key, &label, ParamKind::Vec3, ParamValue::Vec3(v))
        }
        "choice" => {
            let opts: Vec<String> = get("options").map(|v| v.extract()).transpose()?.unwrap_or_default();
            let def: String = default.and_then(|v| v.extract().ok()).or_else(|| opts.first().cloned()).unwrap_or_default();
            ParamSpec::new(&key, &label, ParamKind::Choice(opts), ParamValue::Str(def))
        }
        "array" => {
            let vector: bool = get("vector").and_then(|v| v.extract().ok()).unwrap_or(false);
            let cells: bool = get("cells").and_then(|v| v.extract().ok()).unwrap_or(false);
            ParamSpec::new(
                &key,
                &label,
                ParamKind::Array { scalar_only: false, vector_only: vector },
                ParamValue::Array { assoc: if cells { Association::Cells } else { Association::Points }, name: String::new() },
            )
        }
        "code" => ParamSpec::new(&key, &label, ParamKind::Code, ParamValue::Str(default.and_then(|v| v.extract().ok()).unwrap_or_default())),
        _ => ParamSpec::new(&key, &label, ParamKind::Str, ParamValue::Str(default.and_then(|v| v.extract().ok()).unwrap_or_default())),
    })
}

/// Parameter values as Python keyword arguments.
fn kwargs<'py>(py: Python<'py>, specs: &[ParamSpec], p: &Params) -> PyResult<Bound<'py, PyDict>> {
    let k = PyDict::new(py);
    for s in specs {
        let v = p.get(&s.key).cloned().unwrap_or(s.default.clone());
        match v {
            ParamValue::Bool(b) => k.set_item(&s.key, b)?,
            ParamValue::Int(i) => k.set_item(&s.key, i)?,
            ParamValue::Float(f) => k.set_item(&s.key, f)?,
            ParamValue::Vec3(a) => k.set_item(&s.key, PyTuple::new(py, a)?)?,
            ParamValue::Str(t) => k.set_item(&s.key, t)?,
            ParamValue::Floats(f) => k.set_item(&s.key, f)?,
            ParamValue::Array { name, .. } => {
                if name.is_empty() {
                    k.set_item(&s.key, py.None())?
                } else {
                    k.set_item(&s.key, name)?
                }
            }
        }
    }
    Ok(k)
}

/// Called by `@afx.filter` / `@afx.source` (defined in Python in `aurafx`).
#[pyfunction]
#[pyo3(signature = (func, id, label, category, description, kind, params, min_inputs=1, max_inputs=Some(1), keywords=Vec::new()))]
#[allow(clippy::too_many_arguments)]
pub fn _register_filter(
    func: Py<PyAny>,
    id: String,
    label: String,
    category: String,
    description: String,
    kind: String,
    params: &Bound<'_, PyList>,
    min_inputs: usize,
    max_inputs: Option<usize>,
    keywords: Vec<String>,
) -> PyResult<String> {
    let id = id.to_ascii_lowercase().replace([' ', '-'], "_");
    if id.is_empty() || !id.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
        return Err(pyo3::exceptions::PyValueError::new_err(format!("filter id {id:?} must be letters, digits and _")));
    }
    let specs: Vec<ParamSpec> = params.iter().map(|d| param_spec(d.cast::<PyDict>()?)).collect::<PyResult<_>>()?;
    let is_source = kind == "source";
    let plugin = current_plugin();
    let specs2 = specs.clone();
    let single = !is_source && max_inputs == Some(1);
    let label_c = label.clone();
    let apply: aurafx_filters::CustomFn = Arc::new(move |inputs: &[&DataSet], p: &Params| -> Result<DataSet> {
        Python::attach(|py| {
            as_filter(|| {
                let kw = kwargs(py, &specs2, p).map_err(|e| py_error(py, e))?;
                let f = func.bind(py);
                let datas: Vec<Py<PyData>> =
                    inputs.iter().map(|d| Py::new(py, PyData { ds: (*d).clone() })).collect::<PyResult<_>>().map_err(|e| py_error(py, e))?;
                let r = if is_source {
                    f.call((), Some(&kw))
                } else if single {
                    f.call((datas[0].clone_ref(py),), Some(&kw))
                } else {
                    f.call((PyList::new(py, &datas).map_err(|e| py_error(py, e))?,), Some(&kw))
                }
                .map_err(|e| py_error(py, e))?;
                if r.is_none() {
                    // Changed in place.
                    return match datas.first() {
                        Some(d) => Ok(d.borrow(py).ds.clone()),
                        None => Err(anyhow!("{label_c} returned nothing; return an afx.Data")),
                    };
                }
                data_of(&r, &format!("the result of {label_c}"))
            })
        })
    });
    let def = FilterDef {
        id: leak(id.clone()),
        label: leak(label),
        category: leak(category),
        description: leak(description),
        keywords: Box::leak(keywords.into_iter().map(leak).collect::<Vec<_>>().into_boxed_slice()),
        is_source,
        ext: Some(Arc::new(Extension { apply, plugin })),
        temporal: None,
        multi: (!is_source && max_inputs != Some(1)).then_some(MultiInput {
            label: "Inputs",
            min: min_inputs.max(1),
            max: max_inputs,
            apply: |_, _| Err(anyhow!("plugin filter")),
        }),
        params: specs,
        apply: |_, _| Err(anyhow!("plugin filter")),
    };
    aurafx_filters::register(def).map_err(|e| pyo3::exceptions::PyValueError::new_err(e.to_string()))?;
    Ok(id)
}

/// Called by `@afx.reader`.
#[pyfunction]
pub fn _register_reader(func: Py<PyAny>, extensions: Vec<String>, label: String) -> PyResult<()> {
    let label_c = label.clone();
    let read: Arc<dyn Fn(&Path) -> Result<DataSet> + Send + Sync> = Arc::new(move |path: &Path| {
        Python::attach(|py| {
            as_filter(|| {
                let r = func.bind(py).call1((path.display().to_string(),)).map_err(|e| py_error(py, e))?;
                data_of(&r, &format!("the result of the {label_c} reader"))
            })
        })
    });
    aurafx_io::register_reader(aurafx_io::PluginReader {
        extensions: extensions.into_iter().map(|e| e.trim_start_matches('.').to_ascii_lowercase()).collect(),
        label,
        plugin: current_plugin(),
        read,
    });
    Ok(())
}

/// Load (or reload) a plugin file from Python: `afx.load_plugin("my_tools.py")`.
#[pyfunction]
pub fn load_plugin(py: Python<'_>, path: PathBuf) -> PyResult<Vec<String>> {
    let path = path.canonicalize().unwrap_or(path);
    // A Rust plugin (a shared library built with the aurafx-plugin crate).
    if aurafx_filters::rust_plugins::is_library(&path) {
        let ids = py.detach(|| aurafx_filters::rust_plugins::load(&path)).map_err(|e| pyo3::exceptions::PyRuntimeError::new_err(format!("{e:#}")))?;
        if !crate::data::in_pipeline() {
            crate::session().lock().refresh_filter_defs(&ids);
        }
        return Ok(ids);
    }
    let (info, changed) = py.detach(|| load(&path));
    if let Some(e) = info.error {
        return Err(pyo3::exceptions::PyRuntimeError::new_err(e));
    }
    if !crate::data::in_pipeline() {
        crate::session().lock().refresh_filter_defs(&changed);
    }
    Ok(info.filters)
}

/// Python half of the plugin API (decorators), run when the module is created.
pub const PY_API: &str = r#"
import inspect as _inspect

class Choice:
    """A parameter chosen from a list: mode=afx.Choice(["fast", "accurate"])."""
    def __init__(self, options, default=None):
        self.options = list(options)
        self.default = default if default is not None else (self.options[0] if self.options else "")

class ArrayParam:
    """A parameter naming an array of the input: array=afx.ArrayParam(vector=False)."""
    def __init__(self, vector=False, cells=False):
        self.vector = vector
        self.cells = cells

def __getattr__(name):
    k = _filter_kind(name)
    if k is None:
        raise AttributeError(f"module 'aurafx' has no attribute {name!r}. Try afx.filters() to list filters.")
    fid, is_source = k
    if is_source:
        def make(**kw):
            return apply(fid, None, **kw)
    else:
        def make(*inputs, **kw):
            return apply_multi(fid, list(inputs), **kw)
    make.__name__ = fid
    return make

class Code:
    """A multi-line text parameter edited in a code box."""
    def __init__(self, default=""):
        self.default = default

def _spec(key, value, extra=None):
    d = {"key": key, "label": key.replace("_", " ").capitalize()}
    if isinstance(value, Choice):
        d.update(kind="choice", options=value.options, default=value.default)
    elif isinstance(value, ArrayParam):
        d.update(kind="array", vector=value.vector, cells=value.cells)
    elif isinstance(value, Code):
        d.update(kind="code", default=value.default)
    elif isinstance(value, bool):
        d.update(kind="bool", default=value)
    elif isinstance(value, int):
        d.update(kind="int", default=value)
    elif isinstance(value, float):
        d.update(kind="float", default=value)
    elif isinstance(value, (list, tuple)) and len(value) == 3:
        d.update(kind="vec3", default=[float(x) for x in value])
    elif value is None:
        d.update(kind="str", default="")
    else:
        d.update(kind="str", default=str(value))
    if extra:
        d.update(extra)
    return d

def _specs(fn, params, skip):
    if params is not None:
        return [_spec(k, v) for k, v in params.items()]
    out = []
    for i, (k, p) in enumerate(_inspect.signature(fn).parameters.items()):
        if i < skip or p.kind in (p.VAR_POSITIONAL, p.VAR_KEYWORD):
            continue
        out.append(_spec(k, None if p.default is p.empty else p.default))
    return out

def _doc(fn, description):
    if description:
        return description
    d = (fn.__doc__ or "").strip()
    return d.splitlines()[0] if d else fn.__name__.replace("_", " ")

def _label(fn, label):
    return label or fn.__name__.replace("_", " ").title()

def filter(_fn=None, *, label=None, category="Plugins", description=None, params=None, inputs=1, keywords=(), id=None):
    """Register a filter. The function gets an afx.Data (or a list of them when inputs != 1) and
    keyword parameters, and returns an afx.Data (or None after changing its input in place).
    inputs: 1, an exact count, or (min, max) with max None for any number."""
    if isinstance(inputs, tuple):
        lo, hi = inputs
    elif inputs is None:
        lo, hi = 1, None
    else:
        lo, hi = inputs, inputs
    def wrap(fn):
        _register_filter(fn, id or fn.__name__, _label(fn, label), category, _doc(fn, description), "filter", _specs(fn, params, 1), lo, hi, list(keywords))
        return fn
    return wrap(_fn) if callable(_fn) else wrap

def source(_fn=None, *, label=None, category="Plugins", description=None, params=None, keywords=(), id=None):
    """Register a source: a function of keyword parameters returning an afx.Data."""
    def wrap(fn):
        _register_filter(fn, id or fn.__name__, _label(fn, label), category, _doc(fn, description), "source", _specs(fn, params, 0), 0, 0, list(keywords))
        return fn
    return wrap(_fn) if callable(_fn) else wrap

def reader(extensions, label=None):
    """Register a reader for file extensions: a function of the path returning an afx.Data."""
    if isinstance(extensions, str):
        extensions = [extensions]
    def wrap(fn):
        _register_reader(fn, [e.lstrip(".").lower() for e in extensions], label or fn.__name__.replace("_", " "))
        return fn
    return wrap
"#;
