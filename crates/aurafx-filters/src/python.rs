//! Bridge to the embedded Python interpreter for filters written in Python (Programmable
//! Filter/Source, Python Calculator, plugin filters). The filters crate does not depend on
//! Python: `aurafx-python` installs a hook at start-up.

use anyhow::{anyhow, Result};
use aurafx_data::DataSet;
use std::sync::{Arc, OnceLock};

/// Work handed to Python.
pub enum PyTask<'a> {
    /// Run `script` with `inputs` (list of data) and `output` (a copy of the first input);
    /// the result is whatever `output` holds afterwards.
    Filter { script: &'a str, inputs: &'a [&'a DataSet] },
    /// Run `script`, which must assign `output`.
    Source { script: &'a str },
    /// Evaluate a NumPy expression over the arrays of `input` and add it as `result`.
    Calculator { expression: &'a str, input: &'a DataSet, cells: bool, result: &'a str },
}

pub type PyHook = Arc<dyn Fn(PyTask) -> Result<DataSet> + Send + Sync>;

static HOOK: OnceLock<PyHook> = OnceLock::new();

/// Installed once by the Python runtime.
pub fn set_python_hook(h: PyHook) {
    let _ = HOOK.set(h);
}

pub fn python_available() -> bool {
    HOOK.get().is_some()
}

pub fn run_python(task: PyTask) -> Result<DataSet> {
    let h = HOOK.get().ok_or_else(|| anyhow!("Python is not available (start auraFX with its Python runtime to use Python filters)"))?;
    h(task)
}
