//! The user guide's filter reference (`docs/userguide/src/filter-reference.md`) is generated from
//! the registry: this test fails when it is out of date. Regenerate it with
//! `AFX_WRITE_REFERENCE=1 cargo test -p aurafx-filters --test filter_reference`.
use aurafx_filters::params::{ParamKind, ParamValue};

fn value(v: &ParamValue) -> String {
    match v {
        ParamValue::Bool(b) => if *b { "on" } else { "off" }.into(),
        ParamValue::Int(i) => i.to_string(),
        ParamValue::Float(f) => {
            if f.is_infinite() {
                if *f > 0.0 { "∞" } else { "-∞" }.into()
            } else {
                format!("{f}")
            }
        }
        ParamValue::Vec3(a) => format!("({}, {}, {})", a[0], a[1], a[2]),
        ParamValue::Str(s) if s.is_empty() => "(empty)".into(),
        ParamValue::Str(s) => format!("`{}`", s.replace('\n', "; ").replace('|', "\\|")),
        ParamValue::Array { name, .. } if name.is_empty() => "(the active array)".into(),
        ParamValue::Array { name, .. } => format!("`{name}`"),
        ParamValue::Floats(v) => format!("{v:?}"),
    }
}

fn kind(k: &ParamKind) -> String {
    match k {
        ParamKind::Bool => "on/off".into(),
        ParamKind::Int { min, max } => format!("whole number {min} to {max}").replace("9223372036854775807", "any"),
        ParamKind::Float { min, max, .. } => {
            let f = |x: f64| {
                if x.is_infinite() {
                    if x > 0.0 {
                        "∞".to_string()
                    } else {
                        "-∞".to_string()
                    }
                } else {
                    format!("{x}")
                }
            };
            format!("number {} to {}", f(*min), f(*max))
        }
        ParamKind::Vec3 => "x, y, z".into(),
        ParamKind::Str => "text".into(),
        ParamKind::Choice(c) => c.join(" / "),
        ParamKind::Array { .. } => "array".into(),
        _ => "list of numbers".into(),
    }
}

fn reference() -> String {
    let mut defs: Vec<_> = aurafx_filters::registry::registry().iter().filter(|f| f.ext.is_none()).cloned().collect();
    defs.sort_by(|a, b| a.category.cmp(b.category).then(a.label.cmp(b.label)));
    let mut out = String::from(
        "# Filter reference\n\n<!-- Generated from the filter registry by crates/aurafx-filters/tests/filter_reference.rs; do not edit. -->\n\nEvery filter and source, by menu, with what it does, its Python name (`node.<name>(...)` for a filter, `afx.<name>(...)` for a source) and its settings with their defaults. Settings marked *advanced* show under **Show advanced** in Properties.\n\n",
    );
    let mut cat = "";
    for f in &defs {
        if f.category != cat {
            cat = f.category;
            out += &format!("## {cat}\n\n");
        }
        out += &format!("### {}\n\n{}\n\nPython: `{}`", f.label, f.description.replace('\n', " "), f.id);
        if !f.keywords.is_empty() {
            out += &format!(" · also found by: {}", f.keywords.join(", "));
        }
        out += "\n\n";
        if !f.params.is_empty() {
            out += "| Setting | Python | Takes | Default | |\n|---|---|---|---|---|\n";
            for p in &f.params {
                let note =
                    [if p.advanced { "*advanced*" } else { "" }, p.help.as_str()].iter().filter(|s| !s.is_empty()).cloned().collect::<Vec<_>>().join(" ");
                out += &format!(
                    "| {} | `{}` | {} | {} | {} |\n",
                    p.label.replace('|', "\\|"),
                    p.key,
                    kind(&p.kind).replace('|', "\\|"),
                    value(&p.default),
                    note.replace('|', "\\|").replace('\n', " ")
                );
            }
            out += "\n";
        }
    }
    out
}

#[test]
fn the_filter_reference_is_up_to_date() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/userguide/src/filter-reference.md");
    let want = reference();
    if std::env::var("AFX_WRITE_REFERENCE").is_ok() {
        std::fs::write(&path, &want).unwrap();
        return;
    }
    let have = std::fs::read_to_string(&path).unwrap_or_default();
    assert!(have == want, "docs/userguide/src/filter-reference.md is out of date: AFX_WRITE_REFERENCE=1 cargo test -p aurafx-filters --test filter_reference");
}
