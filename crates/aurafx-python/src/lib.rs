//! Embedded Python with the `aurafx` module.
//!
//! The module is designed for one-liners:
//! ```python
//! import aurafx as afx
//! m = afx.load("run.e")
//! s = m.slice(normal=[0, 0, 1]).color_by("stress")
//! afx.screenshot("slice.png")
//! ```

mod module;

use anyhow::Result;
use aurafx_pipeline::SharedSession;
use pyo3::prelude::*;
use pyo3::types::{PyDict, PyModule};
use std::sync::OnceLock;

static SESSION: OnceLock<SharedSession> = OnceLock::new();

pub(crate) fn session() -> &'static SharedSession {
    SESSION.get().expect("aurafx python runtime not initialized")
}

/// Installed packages ship a private CPython next to the executable. When one is found (and the
/// user has not set PYTHONHOME), point the embedded interpreter at it.
///
/// Layouts: Linux AppImage / tarball `<prefix>/bin/aurafx` + `<prefix>/lib/aurafx/python`,
/// macOS `auraFX.app/Contents/MacOS/aurafx` + `Contents/Resources/python`,
/// Windows `aurafx.exe` + `python\`.
pub fn bundled_python_home() -> Option<std::path::PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let dir = exe.parent()?;
    let candidates = [dir.join("../lib/aurafx/python"), dir.join("../Resources/python"), dir.join("python")];
    candidates.into_iter().find(|c| c.join("lib").exists() || c.join("Lib").exists()).and_then(|c| c.canonicalize().ok())
}

fn use_bundled_python() {
    if std::env::var_os("PYTHONHOME").is_some() {
        return;
    }
    if let Some(home) = bundled_python_home() {
        // SAFETY: called before the interpreter (and any other thread reading the environment) starts.
        unsafe { std::env::set_var("PYTHONHOME", &home) };
        tracing::info!("using bundled Python at {}", home.display());
    }
}

#[derive(Debug, Default, Clone)]
pub struct RunOutput {
    pub stdout: String,
    pub stderr: String,
    pub error: Option<String>,
    /// repr() of the last expression value, if the code was an expression.
    pub value: Option<String>,
}

pub struct PythonRuntime {
    globals: Py<PyDict>,
}

impl PythonRuntime {
    /// Initialize the interpreter (once per process) bound to `session`.
    pub fn new(session: SharedSession) -> Result<Self> {
        use_bundled_python();
        if SESSION.set(session).is_err() {
            tracing::warn!("python runtime initialized twice; keeping the first session");
        }
        pyo3::append_to_inittab!(aurafx);
        Python::initialize();
        let globals = Python::attach(|py| -> PyResult<Py<PyDict>> {
            let g = PyDict::new(py);
            g.set_item("__name__", "__main__")?;
            let builtins = py.import("builtins")?;
            g.set_item("__builtins__", builtins)?;
            let afx = py.import("aurafx")?;
            g.set_item("afx", &afx)?;
            g.set_item("aurafx", afx)?;
            // Best-effort numpy import for convenience.
            if let Ok(np) = py.import("numpy") {
                g.set_item("np", np)?;
            }
            Ok(g.unbind())
        })?;
        Ok(Self { globals })
    }

    pub fn version() -> String {
        Python::attach(|_py| Python::version_str().to_string())
    }

    /// Execute code. Expressions return their repr in `value`; statements run as a script.
    pub fn run(&self, code: &str) -> RunOutput {
        Python::attach(|py| {
            let mut out = RunOutput::default();
            let g = self.globals.bind(py);
            // Capture stdout/stderr.
            let io = match py.import("io") {
                Ok(m) => m,
                Err(e) => {
                    out.error = Some(e.to_string());
                    return out;
                }
            };
            let sys = py.import("sys").unwrap();
            let so = io.call_method0("StringIO").unwrap();
            let se = io.call_method0("StringIO").unwrap();
            let old_out = sys.getattr("stdout").ok();
            let old_err = sys.getattr("stderr").ok();
            let _ = sys.setattr("stdout", &so);
            let _ = sys.setattr("stderr", &se);

            let code_c = std::ffi::CString::new(code).unwrap_or_default();
            let trimmed = code.trim();
            let single_line = !trimmed.contains('\n');
            let result: PyResult<()> = (|| {
                if single_line {
                    if let Ok(v) = py.eval(&code_c, Some(g), None) {
                        if !v.is_none() {
                            out.value = Some(v.repr()?.to_string());
                            let _ = g.set_item("_", v);
                        }
                        return Ok(());
                    }
                }
                py.run(&code_c, Some(g), None)
            })();
            if let Err(e) = result {
                let tb = e.traceback(py).and_then(|t| t.format().ok()).unwrap_or_default();
                out.error = Some(format!("{tb}{e}"));
            }
            if let Some(o) = old_out {
                let _ = sys.setattr("stdout", o);
            }
            if let Some(o) = old_err {
                let _ = sys.setattr("stderr", o);
            }
            out.stdout = so.call_method0("getvalue").and_then(|v| v.extract()).unwrap_or_default();
            out.stderr = se.call_method0("getvalue").and_then(|v| v.extract()).unwrap_or_default();
            out
        })
    }

    /// Names in the global namespace (for completion).
    pub fn global_names(&self) -> Vec<String> {
        Python::attach(|py| self.globals.bind(py).keys().iter().filter_map(|k| k.extract().ok()).collect())
    }

    /// Attribute names of an object expression (for completion), e.g. "afx." or "mesh.".
    pub fn complete(&self, prefix: &str) -> Vec<String> {
        let (obj, part) = match prefix.rsplit_once('.') {
            Some((o, p)) => (Some(o), p),
            None => (None, prefix),
        };
        Python::attach(|py| {
            let g = self.globals.bind(py);
            let names: Vec<String> = match obj {
                None => {
                    let mut v: Vec<String> = g.keys().iter().filter_map(|k| k.extract().ok()).collect();
                    if let Ok(b) = py.import("builtins") {
                        if let Ok(d) = b.dir() {
                            v.extend(d.iter().filter_map(|k| k.extract::<String>().ok()));
                        }
                    }
                    v
                }
                Some(expr) => {
                    let c = std::ffi::CString::new(expr).unwrap_or_default();
                    match py.eval(&c, Some(g), None).and_then(|o| o.dir()) {
                        Ok(d) => d.iter().filter_map(|k| k.extract::<String>().ok()).collect(),
                        Err(_) => vec![],
                    }
                }
            };
            let mut v: Vec<String> = names.into_iter().filter(|n| n.starts_with(part) && (!n.starts_with('_') || part.starts_with('_'))).collect();
            v.sort();
            v.dedup();
            v
        })
    }
}

#[pymodule]
fn aurafx(m: &Bound<'_, PyModule>) -> PyResult<()> {
    module::register(m)
}
