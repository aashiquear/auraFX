//! The user guide's Python API reference (`docs/userguide/src/python-api.md`) is generated from
//! the `aurafx` module's own docstrings and signatures: this test fails when it is out of date.
//! Regenerate it with `AFX_WRITE_REFERENCE=1 cargo test -p aurafx-python --test api_reference`.
use aurafx_pipeline::Session;
use aurafx_python::PythonRuntime;

const SCRIPT: &str = include_str!("api_reference.py");

#[test]
fn python_api_reference_is_current() {
    let rt = PythonRuntime::new(Session::shared()).unwrap();
    let out = rt.run(SCRIPT);
    assert!(out.error.is_none(), "{:?}", out.error);
    let text = rt.run("print(_reference, end='')").stdout;
    assert!(text.contains("afx.load(") && text.contains("## Functions") && text.len() > 5000, "the reference looks empty:\n{}", &text[..text.len().min(400)]);
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/userguide/src/python-api.md");
    if std::env::var_os("AFX_WRITE_REFERENCE").is_some() {
        std::fs::write(&path, &text).unwrap();
        return;
    }
    let on_disk = std::fs::read_to_string(&path).unwrap_or_default();
    assert!(on_disk == text, "docs/userguide/src/python-api.md is out of date: run AFX_WRITE_REFERENCE=1 cargo test -p aurafx-python --test api_reference");
}
