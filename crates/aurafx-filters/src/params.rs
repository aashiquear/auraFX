//! Typed parameter values and schemas.

use indexmap::IndexMap;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum Association {
    #[default]
    Points,
    Cells,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ParamValue {
    Bool(bool),
    Int(i64),
    Float(f64),
    Vec3([f64; 3]),
    Str(String),
    /// Reference to a data array: (association, name).
    Array {
        assoc: Association,
        name: String,
    },
    Floats(Vec<f64>),
}

impl ParamValue {
    pub fn as_f64(&self) -> Option<f64> {
        match self {
            ParamValue::Float(v) => Some(*v),
            ParamValue::Int(v) => Some(*v as f64),
            ParamValue::Bool(b) => Some(if *b { 1.0 } else { 0.0 }),
            _ => None,
        }
    }
    pub fn as_i64(&self) -> Option<i64> {
        match self {
            ParamValue::Int(v) => Some(*v),
            ParamValue::Float(v) => Some(*v as i64),
            _ => None,
        }
    }
    pub fn as_bool(&self) -> Option<bool> {
        match self {
            ParamValue::Bool(b) => Some(*b),
            ParamValue::Int(v) => Some(*v != 0),
            _ => None,
        }
    }
    pub fn as_vec3(&self) -> Option<[f64; 3]> {
        match self {
            ParamValue::Vec3(v) => Some(*v),
            ParamValue::Floats(v) if v.len() == 3 => Some([v[0], v[1], v[2]]),
            _ => None,
        }
    }
    pub fn as_str(&self) -> Option<&str> {
        match self {
            ParamValue::Str(s) => Some(s),
            ParamValue::Array { name, .. } => Some(name),
            _ => None,
        }
    }
    pub fn as_array(&self) -> Option<(Association, &str)> {
        match self {
            ParamValue::Array { assoc, name } => Some((*assoc, name)),
            ParamValue::Str(s) => Some((Association::Points, s)),
            _ => None,
        }
    }
    pub fn as_floats(&self) -> Option<Vec<f64>> {
        match self {
            ParamValue::Floats(v) => Some(v.clone()),
            ParamValue::Float(v) => Some(vec![*v]),
            ParamValue::Int(v) => Some(vec![*v as f64]),
            ParamValue::Vec3(v) => Some(v.to_vec()),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ParamKind {
    Bool,
    Int {
        min: i64,
        max: i64,
    },
    Float {
        min: f64,
        max: f64,
        log: bool,
    },
    Vec3,
    Str,
    Choice(Vec<String>),
    /// Array selector; `scalar_only` restricts to 1-component arrays.
    Array {
        scalar_only: bool,
        vector_only: bool,
    },
    Floats,
    /// Multi-line expression (calculator).
    Expression,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ParamSpec {
    pub key: String,
    pub label: String,
    pub help: String,
    pub kind: ParamKind,
    pub default: ParamValue,
    pub advanced: bool,
}

impl ParamSpec {
    pub fn new(key: &str, label: &str, kind: ParamKind, default: ParamValue) -> Self {
        Self { key: key.into(), label: label.into(), help: String::new(), kind, default, advanced: false }
    }
    pub fn help(mut self, h: &str) -> Self {
        self.help = h.into();
        self
    }
    pub fn advanced(mut self) -> Self {
        self.advanced = true;
        self
    }
}

/// Concrete parameter values for one filter instance.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Params(pub IndexMap<String, ParamValue>);

impl Params {
    pub fn from_specs(specs: &[ParamSpec]) -> Self {
        Params(specs.iter().map(|s| (s.key.clone(), s.default.clone())).collect())
    }
    pub fn get(&self, key: &str) -> Option<&ParamValue> {
        self.0.get(key)
    }
    pub fn set(&mut self, key: &str, v: ParamValue) {
        self.0.insert(key.to_string(), v);
    }
    pub fn f64(&self, key: &str, default: f64) -> f64 {
        self.get(key).and_then(|v| v.as_f64()).unwrap_or(default)
    }
    pub fn i64(&self, key: &str, default: i64) -> i64 {
        self.get(key).and_then(|v| v.as_i64()).unwrap_or(default)
    }
    pub fn bool(&self, key: &str, default: bool) -> bool {
        self.get(key).and_then(|v| v.as_bool()).unwrap_or(default)
    }
    pub fn vec3(&self, key: &str, default: [f64; 3]) -> [f64; 3] {
        self.get(key).and_then(|v| v.as_vec3()).unwrap_or(default)
    }
    pub fn str(&self, key: &str) -> String {
        self.get(key).and_then(|v| v.as_str()).unwrap_or("").to_string()
    }
    pub fn array(&self, key: &str) -> Option<(Association, String)> {
        self.get(key).and_then(|v| v.as_array().map(|(a, n)| (a, n.to_string()))).filter(|(_, n)| !n.is_empty())
    }
    pub fn floats(&self, key: &str) -> Vec<f64> {
        self.get(key).and_then(|v| v.as_floats()).unwrap_or_default()
    }
    /// Set from a loosely-typed JSON value (used by Python and the AI tools).
    pub fn set_json(&mut self, key: &str, v: &serde_json::Value) {
        let pv = match v {
            serde_json::Value::Bool(b) => ParamValue::Bool(*b),
            serde_json::Value::Number(n) => {
                if let Some(i) = n.as_i64() {
                    ParamValue::Int(i)
                } else {
                    ParamValue::Float(n.as_f64().unwrap_or(0.0))
                }
            }
            serde_json::Value::String(s) => ParamValue::Str(s.clone()),
            serde_json::Value::Array(a) => {
                let f: Vec<f64> = a.iter().filter_map(|x| x.as_f64()).collect();
                if f.len() == 3 && f.len() == a.len() {
                    ParamValue::Vec3([f[0], f[1], f[2]])
                } else {
                    ParamValue::Floats(f)
                }
            }
            serde_json::Value::Object(o) => ParamValue::Array {
                assoc: if o.get("assoc").and_then(|a| a.as_str()) == Some("cells") { Association::Cells } else { Association::Points },
                name: o.get("name").and_then(|n| n.as_str()).unwrap_or("").to_string(),
            },
            serde_json::Value::Null => return,
        };
        // Coerce to existing kind when possible.
        let coerced = match (self.0.get(key), pv) {
            (Some(ParamValue::Array { assoc, .. }), ParamValue::Str(s)) => ParamValue::Array { assoc: *assoc, name: s },
            (Some(ParamValue::Float(_)), ParamValue::Int(i)) => ParamValue::Float(i as f64),
            (Some(ParamValue::Bool(_)), ParamValue::Int(i)) => ParamValue::Bool(i != 0),
            (_, v) => v,
        };
        self.0.insert(key.to_string(), coerced);
    }
}
