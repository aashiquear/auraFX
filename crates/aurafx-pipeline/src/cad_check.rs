//! The downstream check (roadmap section 13, stage C5): before an edited part reaches the
//! pipeline, find what the nodes below it refer to that the edit changed or removed, say what
//! the change is, and offer fixes. References are found in the nodes' parameters: face and
//! body names (whole values and names inside command or code text), numbers that are face or
//! body ids, positions inside the old bounds that fall outside the new ones, and the part's own
//! parameter overrides. The old and the new build are compared by `aurafx_cad::diff`; each
//! conflict gets a default fix where one is safe (a renamed face is renamed, a removed face
//! with one successor is rewritten and flagged "check"), and the assistant can suggest more.

use crate::session::{NodeId, NodeKind, Session};
use anyhow::{anyhow, bail, Result};
use aurafx_cad::diff::{diff, FaceFate, GeometryDiff, GeometrySnapshot};
use aurafx_filters::{ParamKind, ParamValue};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ConflictKind {
    /// The face, body or dimension is gone.
    Removed,
    /// It is still there under another name.
    Renamed { to: String },
    /// A position the node uses is no longer on or inside the part.
    Moved { outside: bool },
    /// Several plausible successors.
    Ambiguous { candidates: Vec<String> },
    /// The node assumed a number of bodies.
    Count { was: usize, now: usize },
}

impl ConflictKind {
    pub fn label(&self) -> String {
        match self {
            ConflictKind::Removed => "removed".into(),
            ConflictKind::Renamed { to } => format!("renamed to '{to}'"),
            ConflictKind::Moved { outside } => if *outside { "now outside the part" } else { "moved" }.into(),
            ConflictKind::Ambiguous { candidates } => format!("one of: {}", candidates.join(", ")),
            ConflictKind::Count { was, now } => format!("bodies {was} -> {now}"),
        }
    }
}

/// A change to one parameter of one node, applied with `set_param_json`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Fix {
    pub node: NodeId,
    pub param: String,
    pub value: Value,
    pub reason: String,
    /// The fix is a guess a person should look at.
    pub check: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Conflict {
    pub node: NodeId,
    pub node_name: String,
    pub param: String,
    pub param_label: String,
    /// The parameter's value now.
    pub value: String,
    /// What it refers to: a face name, a body name, a position.
    pub reference: String,
    pub kind: ConflictKind,
    /// The default fix, when there is a safe one.
    pub fix: Option<Fix>,
    /// Found by a text search of code (a Python filter): it may or may not be a reference.
    pub may_reference: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CheckReport {
    pub node: NodeId,
    pub diff: GeometryDiff,
    /// The nodes below the part (not counting it).
    pub downstream: Vec<NodeId>,
    pub conflicts: Vec<Conflict>,
}

impl CheckReport {
    /// The default fixes of all conflicts that have one.
    pub fn default_fixes(&self) -> Vec<Fix> {
        self.conflicts.iter().filter_map(|c| c.fix.clone()).collect()
    }
}

/// Whole-word occurrences of `name` in `text`: the byte ranges (a name is not found inside a
/// longer name: `Base side 1` is not in `Base side 10`).
fn find_name(text: &str, name: &str) -> Vec<(usize, usize)> {
    if name.is_empty() {
        return Vec::new();
    }
    let word = |c: char| c.is_alphanumeric() || c == '_';
    let mut out = Vec::new();
    let mut from = 0;
    while let Some(i) = text[from..].find(name) {
        let (a, b) = (from + i, from + i + name.len());
        let before = text[..a].chars().next_back();
        let after = text[b..].chars().next();
        let first = name.chars().next().unwrap();
        let last = name.chars().next_back().unwrap();
        if !(before.is_some_and(word) && word(first)) && !(after.is_some_and(word) && word(last)) {
            out.push((a, b));
        }
        from = b;
    }
    out
}

fn replace_names(text: &str, old: &str, new: &str) -> String {
    let mut out = String::new();
    let mut last = 0;
    for (a, b) in find_name(text, old) {
        out += &text[last..a];
        out += new;
        last = b;
    }
    out + &text[last..]
}

fn inside(b: &([f64; 3], [f64; 3]), p: [f64; 3], margin: f64) -> bool {
    (0..3).all(|k| p[k] >= b.0[k] - margin && p[k] <= b.1[k] + margin)
}

fn clamp(b: &([f64; 3], [f64; 3]), p: [f64; 3]) -> [f64; 3] {
    std::array::from_fn(|k| p[k].clamp(b.0[k], b.1[k]))
}

fn param_text(v: &ParamValue) -> String {
    match v {
        ParamValue::Str(s) => s.clone(),
        ParamValue::Vec3(a) => format!("({}, {}, {})", a[0], a[1], a[2]),
        ParamValue::Int(i) => i.to_string(),
        ParamValue::Float(f) => f.to_string(),
        other => format!("{other:?}"),
    }
}

/// Collect the conflicts of the nodes below `cad` between the old and the new build.
pub fn collect(session: &Session, cad: NodeId, old: &GeometrySnapshot, new: &GeometrySnapshot) -> CheckReport {
    let d = diff(old, new);
    let below: Vec<NodeId> = session.downstream(cad).into_iter().filter(|n| *n != cad).collect();
    let mut conflicts: Vec<Conflict> = Vec::new();
    let margin = 1e-6 * old.size().max(1.0);

    // What a name reference turns into, by the fate of the face (or body) of that name.
    let name_kind = |name: &str| -> Option<(ConflictKind, Option<String>, bool)> {
        // -> (kind, replacement for the default fix, the fix is a guess)
        if let Some(c) = d.faces.iter().find(|c| c.old.name == name) {
            return match &c.fate {
                FaceFate::Same => None,
                FaceFate::Renamed { to } => Some((ConflictKind::Renamed { to: to.name.clone() }, Some(to.name.clone()), false)),
                FaceFate::Replaced { by } => Some((ConflictKind::Removed, Some(by.name.clone()), true)),
                FaceFate::Ambiguous { candidates } => Some((ConflictKind::Ambiguous { candidates: candidates.iter().map(|c| c.name.clone()).collect() }, None, true)),
                FaceFate::Removed => Some((ConflictKind::Removed, None, false)),
            };
        }
        if let Some(b) = d.bodies_removed.iter().find(|b| b.name == name) {
            // The same body under another name (the document was renamed)?
            if let Some(n) = new.bodies.iter().find(|n| n.id == b.id) {
                return Some((ConflictKind::Renamed { to: n.name.clone() }, Some(n.name.clone()), false));
            }
            return Some((ConflictKind::Removed, None, false));
        }
        if let Some(b) = old.bodies.iter().find(|b| b.name == name) {
            if let Some(n) = new.bodies.iter().find(|n| n.id == b.id && n.name != b.name) {
                return Some((ConflictKind::Renamed { to: n.name.clone() }, Some(n.name.clone()), false));
            }
        }
        None
    };
    let names: Vec<String> = old.faces.iter().map(|f| f.name.clone()).chain(old.bodies.iter().map(|b| b.name.clone())).collect();

    for &id in &below {
        let Some(n) = session.nodes.get(&id) else { continue };
        let def = match &n.kind {
            NodeKind::Source { def } | NodeKind::Filter { def } => def.clone(),
            _ => continue,
        };
        let python = def.ext.is_some();
        for spec in &def.params {
            let Some(val) = n.params.get(&spec.key) else { continue };
            match (&spec.kind, val) {
                (ParamKind::Str | ParamKind::Choice(_) | ParamKind::Expression | ParamKind::Code, ParamValue::Str(text)) => {
                    // Longer names first, so "Bore side 2" is not taken for "Bore side".
                    let mut sorted = names.clone();
                    sorted.sort_by_key(|n| std::cmp::Reverse(n.len()));
                    let mut remaining = text.clone();
                    let mut fixed = text.clone();
                    let mut had_fix = true;
                    let mut found: Vec<(String, ConflictKind, bool)> = Vec::new();
                    for name in &sorted {
                        if find_name(&remaining, name).is_empty() {
                            continue;
                        }
                        // The text of this name is consumed so shorter names inside it do not match again.
                        remaining = replace_names(&remaining, name, "\u{1}");
                        if let Some((kind, rep, guess)) = name_kind(name) {
                            match &rep {
                                Some(r) if !python => fixed = replace_names(&fixed, name, r),
                                _ => had_fix = false,
                            }
                            found.push((name.clone(), kind, guess));
                        }
                    }
                    for (name, kind, guess) in found {
                        let fix = if had_fix && !python && fixed != *text && !matches!(kind, ConflictKind::Ambiguous { .. }) {
                            Some(Fix { node: id, param: spec.key.clone(), value: json!(fixed), reason: format!("'{name}' -> {}", kind.label()), check: guess })
                        } else {
                            None
                        };
                        conflicts.push(Conflict {
                            node: id,
                            node_name: n.name.clone(),
                            param: spec.key.clone(),
                            param_label: spec.label.clone(),
                            value: text.clone(),
                            reference: name,
                            kind,
                            fix,
                            may_reference: python,
                        });
                    }
                }
                (ParamKind::Vec3, ParamValue::Vec3(p)) if p.iter().all(|x| x.is_finite()) => {
                    if inside(&old.bounds, *p, margin) && !inside(&new.bounds, *p, margin) {
                        let to = clamp(&new.bounds, *p);
                        conflicts.push(Conflict {
                            node: id,
                            node_name: n.name.clone(),
                            param: spec.key.clone(),
                            param_label: spec.label.clone(),
                            value: param_text(val),
                            reference: param_text(val),
                            kind: ConflictKind::Moved { outside: true },
                            fix: Some(Fix { node: id, param: spec.key.clone(), value: json!(to), reason: "moved onto the new bounds".into(), check: true }),
                            may_reference: false,
                        });
                    }
                }
                (ParamKind::Int { .. }, ParamValue::Int(v)) if matches!(spec.key.as_str(), "face" | "face_id" | "faceid") => {
                    if let Some(c) = d.faces.iter().find(|c| i64::from(c.old.face_id) == *v) {
                        let (kind, to, guess) = match &c.fate {
                            FaceFate::Same if c.old.face_id == new.faces.iter().find(|f| f.name == c.old.name).map(|f| f.face_id).unwrap_or(c.old.face_id) => continue,
                            FaceFate::Same => {
                                let f = new.faces.iter().find(|f| f.name == c.old.name).unwrap();
                                (ConflictKind::Renamed { to: f.name.clone() }, Some(f.face_id), false)
                            }
                            FaceFate::Renamed { to } => (ConflictKind::Renamed { to: to.name.clone() }, Some(to.face_id), false),
                            FaceFate::Replaced { by } => (ConflictKind::Removed, Some(by.face_id), true),
                            FaceFate::Ambiguous { candidates } => (ConflictKind::Ambiguous { candidates: candidates.iter().map(|c| c.name.clone()).collect() }, None, true),
                            FaceFate::Removed => (ConflictKind::Removed, None, false),
                        };
                        conflicts.push(Conflict {
                            node: id,
                            node_name: n.name.clone(),
                            param: spec.key.clone(),
                            param_label: spec.label.clone(),
                            value: v.to_string(),
                            reference: format!("{} (face {})", c.old.name, v),
                            fix: to.map(|t| Fix { node: id, param: spec.key.clone(), value: json!(t), reason: format!("face {v} -> {t}"), check: guess }),
                            kind,
                            may_reference: false,
                        });
                    }
                }
                (ParamKind::Int { .. }, ParamValue::Int(v)) if matches!(spec.key.as_str(), "body" | "body_id" | "bodyid") => {
                    if *v >= 0 && (*v as usize) >= new.bodies.len() && new.bodies.len() != old.bodies.len() {
                        conflicts.push(Conflict {
                            node: id,
                            node_name: n.name.clone(),
                            param: spec.key.clone(),
                            param_label: spec.label.clone(),
                            value: v.to_string(),
                            reference: format!("body {v}"),
                            kind: ConflictKind::Count { was: old.bodies.len(), now: new.bodies.len() },
                            fix: None,
                            may_reference: false,
                        });
                    }
                }
                _ => {}
            }
        }
    }
    // The part's own overrides of dimensions that are gone.
    if let Some(n) = session.nodes.get(&cad) {
        for (name, _) in aurafx_filters::cad::parse_overrides(&n.params.str("parameters")).unwrap_or_default() {
            if d.parameters_removed.contains(&name) {
                let mut rest: serde_json::Map<String, Value> =
                    serde_json::from_str::<Value>(&n.params.str("parameters")).ok().and_then(|v| v.as_object().cloned()).unwrap_or_default();
                rest.remove(&name);
                conflicts.push(Conflict {
                    node: cad,
                    node_name: n.name.clone(),
                    param: "parameters".into(),
                    param_label: "Parameter overrides".into(),
                    value: n.params.str("parameters"),
                    reference: name.clone(),
                    kind: ConflictKind::Removed,
                    fix: Some(Fix { node: cad, param: "parameters".into(), value: json!(Value::Object(rest).to_string()), reason: format!("dimension '{name}' no longer exists: drop its override"), check: false }),
                    may_reference: false,
                });
            }
        }
    }
    CheckReport { node: cad, diff: d, downstream: below, conflicts }
}

/// The prompt that asks the assistant for fixes: the diff in words, the conflicts, each
/// affected node's filter and parameters.
pub fn ai_prompt(session: &Session, report: &CheckReport) -> String {
    let mut p = String::from(
        "I edited a CAD part and the nodes below it in the pipeline refer to things that changed. Suggest fixes.\n\nWhat changed in the part:\n",
    );
    p += &report.diff.describe();
    p += "\nConflicts (node id, filter, parameter, current value, what it refers to, what happened):\n";
    for c in &report.conflicts {
        let filter = session.nodes.get(&c.node).and_then(|n| n.def().map(|d| d.id)).unwrap_or("?");
        p += &format!("- node {} ({}, filter {filter}), parameter '{}' = {:?}: refers to '{}', {}{}\n", c.node, c.node_name, c.param, c.value, c.reference, c.kind.label(), if c.may_reference { " (found in code: may not be a reference)" } else { "" });
    }
    p += "\nAnswer with a JSON array only, one object per parameter to change: [{\"node\": <node id>, \"param\": \"<parameter key>\", \"value\": <new value>, \"reason\": \"<short reason>\"}]. Use [] when nothing should change. Keep values of the same type as the current ones.";
    p
}

/// Read the assistant's answer: the first JSON array in the text (inside a code fence or bare)
/// of `{node, param, value, reason}` objects. Nodes may be named by id or by name.
pub fn parse_ai_fixes(session: &Session, answer: &str) -> Result<Vec<Fix>> {
    let start = answer.find('[').ok_or_else(|| anyhow!("the answer has no JSON list"))?;
    // The matching bracket, ignoring brackets inside strings.
    let bytes: Vec<char> = answer[start..].chars().collect();
    let (mut depth, mut in_str, mut esc, mut end) = (0i32, false, false, None);
    for (i, c) in bytes.iter().enumerate() {
        if in_str {
            if esc {
                esc = false;
            } else if *c == '\\' {
                esc = true;
            } else if *c == '"' {
                in_str = false;
            }
            continue;
        }
        match c {
            '"' => in_str = true,
            '[' => depth += 1,
            ']' => {
                depth -= 1;
                if depth == 0 {
                    end = Some(i);
                    break;
                }
            }
            _ => {}
        }
    }
    let end = end.ok_or_else(|| anyhow!("the JSON list in the answer is not closed"))?;
    let text: String = bytes[..=end].iter().collect();
    let v: Value = serde_json::from_str(&text).map_err(|e| anyhow!("the answer's JSON list does not parse: {e}"))?;
    let list = v.as_array().ok_or_else(|| anyhow!("the answer is not a list"))?;
    let mut out = Vec::new();
    for item in list {
        let node = match item.get("node") {
            Some(Value::Number(n)) => n.as_u64().ok_or_else(|| anyhow!("bad node id {n}"))?,
            Some(Value::String(s)) => match s.parse::<u64>() {
                Ok(i) => i,
                Err(_) => session.nodes.values().find(|n| &n.name == s).map(|n| n.id).ok_or_else(|| anyhow!("no node named '{s}'"))?,
            },
            _ => bail!("a fix needs a \"node\""),
        };
        let n = session.nodes.get(&node).ok_or_else(|| anyhow!("no node {node}"))?;
        let param = item.get("param").and_then(|p| p.as_str()).ok_or_else(|| anyhow!("a fix needs a \"param\""))?.to_string();
        if n.params.get(&param).is_none() {
            bail!("node {node} ({}) has no parameter '{param}'", n.name);
        }
        let value = item.get("value").cloned().ok_or_else(|| anyhow!("a fix needs a \"value\""))?;
        out.push(Fix { node, param, value, reason: item.get("reason").and_then(|r| r.as_str()).unwrap_or("suggested by the assistant").to_string(), check: true });
    }
    Ok(out)
}

/// How Apply treats the conflicts the check found.
#[derive(Debug, Clone)]
pub enum FixMode {
    /// Apply the default fixes.
    Default,
    /// Leave every conflicting parameter as it is.
    None,
    /// Apply exactly these fixes (the user's choices, or the assistant's).
    Fixes(Vec<Fix>),
}

/// Whether the nodes below the part re-run at once or wait.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunMode {
    Now,
    Later,
}
