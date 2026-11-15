//! Plugin panels (`@afx.panel`): drawn, a press runs the button's branch, values persist.
use aurafx_pipeline::Session;
use aurafx_python::PythonRuntime;

#[test]
fn a_panel_draws_and_answers_events() {
    let rt = PythonRuntime::new(Session::shared()).unwrap();
    let out = rt.run(
        r#"
import aurafx as afx
presses = []
@afx.panel("Tools")
def tools(ui):
    ui.label("Scale")
    f = ui.slider("factor", 2.0, 0.5, 10.0)
    n = ui.number("count", 3)
    on = ui.checkbox("live", False)
    kind = ui.choice("shape", ["sphere", "cube"])
    if ui.button("Apply"):
        presses.append((f, n, on, kind))
    ui.separator()
"#,
    );
    assert!(out.error.is_none(), "{:?}", out.error);
    assert_eq!(rt.panel_titles(), vec!["Tools".to_string()]);
    let items = |t: &str| -> Vec<serde_json::Value> { serde_json::from_str::<serde_json::Value>(t).unwrap()["items"].as_array().cloned().unwrap() };
    let first = items(&rt.panel("Tools", ""));
    assert_eq!(first.len(), 7);
    assert_eq!(first[1]["value"], 2.0);
    // A changed value persists; the press sees it.
    let after = items(&rt.panel("Tools", r#"{"kind": "set", "name": "factor", "value": 4.5}"#));
    assert_eq!(after[1]["value"], 4.5);
    rt.panel("Tools", r#"{"kind": "set", "name": "shape", "value": "cube"}"#);
    rt.panel("Tools", r#"{"kind": "button", "name": "Apply"}"#);
    let v = rt.run("presses");
    assert_eq!(v.value.as_deref(), Some("[(4.5, 3, False, 'cube')]"));
    // An error inside the function comes back with the items drawn so far.
    rt.run("@afx.panel('Broken')\ndef broken(ui):\n    ui.label('before')\n    1/0\n");
    let b: serde_json::Value = serde_json::from_str(&rt.panel("Broken", "")).unwrap();
    assert!(b["error"].as_str().unwrap().contains("ZeroDivisionError"));
    assert_eq!(b["items"][0]["text"], "before");
}
