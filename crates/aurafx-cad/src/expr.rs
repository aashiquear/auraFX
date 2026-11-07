//! Unit-aware arithmetic expressions for parameters, bindings and sketch dimensions.
//!
//! Ported from EZ-YX-craft (`crates/ezyx-cad/src/expr.rs`, MIT OR Apache-2.0), unchanged.
//!
//! Grammar (usual precedence, `^` right-associative and binding tighter than unary minus):
//! `expr := term (('+'|'-') term)*`, `term := unary (('*'|'/') unary)*`,
//! `unary := ('-'|'+') unary | power`, `power := postfix ('^' unary)?`,
//! `postfix := primary unit?`, `primary := number | ident | ident '(' args ')' | '(' expr ')'`.
//!
//! Results are millimetres for lengths and **degrees** for angles: a unit suffix directly after a
//! number or a parenthesised group scales it (`mm` 1, `cm` 10, `m` 1000, `in`/`inch` 25.4,
//! `ft` 304.8, `deg` 1, `rad` 57.29…). Trig functions take degrees and inverse trig returns degrees.

use std::collections::HashMap;
use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Error)]
pub enum ExprError {
    #[error("parse error at {pos}: {msg}")]
    Parse { pos: usize, msg: String },
    #[error("unknown identifier '{0}'")]
    UnknownIdentifier(String),
    #[error("unknown function '{0}'")]
    UnknownFunction(String),
    #[error("function '{name}' expects {expected} argument(s), got {got}")]
    Arity { name: String, expected: &'static str, got: usize },
    #[error("result is not a finite number (division by zero or domain error)")]
    NotFinite,
}

/// Unit name → factor to mm (lengths) or degrees (angles).
pub fn unit_factor(name: &str) -> Option<f64> {
    Some(match name {
        "mm" => 1.0,
        "cm" => 10.0,
        "m" => 1000.0,
        "in" | "inch" => 25.4,
        "ft" => 304.8,
        "deg" => 1.0,
        "rad" => 180.0 / std::f64::consts::PI,
        _ => return None,
    })
}

const FUNCTIONS: [&str; 14] = ["sin", "cos", "tan", "asin", "acos", "atan", "atan2", "sqrt", "abs", "min", "max", "round", "floor", "ceil"];

#[derive(Debug, Clone, PartialEq)]
enum Tok {
    Num(f64),
    Ident(String),
    Op(char),
    LParen,
    RParen,
    Comma,
}

fn tokenize(s: &str) -> Result<Vec<(Tok, usize)>, ExprError> {
    let b = s.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        let c = b[i] as char;
        if c.is_whitespace() {
            i += 1;
        } else if c.is_ascii_digit() || (c == '.' && b.get(i + 1).is_some_and(|d| d.is_ascii_digit())) {
            let start = i;
            while i < b.len() && (b[i].is_ascii_digit() || b[i] == b'.') {
                i += 1;
            }
            // Exponent only when followed by digits (so `2e` never swallows an identifier).
            if i < b.len() && (b[i] == b'e' || b[i] == b'E') {
                let mut j = i + 1;
                if j < b.len() && (b[j] == b'+' || b[j] == b'-') {
                    j += 1;
                }
                if j < b.len() && b[j].is_ascii_digit() {
                    i = j;
                    while i < b.len() && b[i].is_ascii_digit() {
                        i += 1;
                    }
                }
            }
            let v = s[start..i].parse::<f64>().map_err(|_| ExprError::Parse { pos: start, msg: format!("bad number '{}'", &s[start..i]) })?;
            out.push((Tok::Num(v), start));
        } else if c.is_alphabetic() || c == '_' {
            let start = i;
            while i < b.len() && ((b[i] as char).is_alphanumeric() || b[i] == b'_' || b[i] == b'.') {
                i += 1;
            }
            out.push((Tok::Ident(s[start..i].to_string()), start));
        } else {
            let t = match c {
                '+' | '-' | '*' | '/' | '^' => Tok::Op(c),
                '(' => Tok::LParen,
                ')' => Tok::RParen,
                ',' => Tok::Comma,
                _ => return Err(ExprError::Parse { pos: i, msg: format!("unexpected character '{c}'") }),
            };
            out.push((t, i));
            i += c.len_utf8();
        }
    }
    Ok(out)
}

struct Parser<'a> {
    toks: Vec<(Tok, usize)>,
    i: usize,
    len: usize,
    vars: &'a dyn Fn(&str) -> Option<f64>,
}

impl Parser<'_> {
    fn peek(&self) -> Option<&Tok> {
        self.toks.get(self.i).map(|t| &t.0)
    }
    fn pos(&self) -> usize {
        self.toks.get(self.i).map(|t| t.1).unwrap_or(self.len)
    }
    fn err<T>(&self, msg: impl Into<String>) -> Result<T, ExprError> {
        Err(ExprError::Parse { pos: self.pos(), msg: msg.into() })
    }
    fn expr(&mut self) -> Result<f64, ExprError> {
        let mut v = self.term()?;
        while let Some(Tok::Op(c @ ('+' | '-'))) = self.peek() {
            let c = *c;
            self.i += 1;
            let r = self.term()?;
            v = if c == '+' { v + r } else { v - r };
        }
        Ok(v)
    }
    fn term(&mut self) -> Result<f64, ExprError> {
        let mut v = self.unary()?;
        while let Some(Tok::Op(c @ ('*' | '/'))) = self.peek() {
            let c = *c;
            self.i += 1;
            let r = self.unary()?;
            v = if c == '*' { v * r } else { v / r };
        }
        Ok(v)
    }
    fn unary(&mut self) -> Result<f64, ExprError> {
        match self.peek() {
            Some(Tok::Op('-')) => {
                self.i += 1;
                Ok(-self.unary()?)
            }
            Some(Tok::Op('+')) => {
                self.i += 1;
                self.unary()
            }
            _ => self.power(),
        }
    }
    fn power(&mut self) -> Result<f64, ExprError> {
        let base = self.postfix()?;
        if let Some(Tok::Op('^')) = self.peek() {
            self.i += 1;
            let e = self.unary()?; // right-associative: 2^3^2 = 2^(3^2)
            return Ok(base.powf(e));
        }
        Ok(base)
    }
    fn postfix(&mut self) -> Result<f64, ExprError> {
        let is_group = matches!(self.peek(), Some(Tok::Num(_) | Tok::LParen));
        let v = self.primary()?;
        if is_group {
            if let Some(Tok::Ident(u)) = self.peek() {
                if let Some(f) = unit_factor(u) {
                    self.i += 1;
                    return Ok(v * f);
                }
            }
        }
        Ok(v)
    }
    fn primary(&mut self) -> Result<f64, ExprError> {
        match self.peek().cloned() {
            Some(Tok::Num(v)) => {
                self.i += 1;
                Ok(v)
            }
            Some(Tok::LParen) => {
                self.i += 1;
                let v = self.expr()?;
                if self.peek() != Some(&Tok::RParen) {
                    return self.err("expected ')'");
                }
                self.i += 1;
                Ok(v)
            }
            Some(Tok::Ident(name)) => {
                self.i += 1;
                if self.peek() == Some(&Tok::LParen) {
                    self.i += 1;
                    let mut args = Vec::new();
                    if self.peek() != Some(&Tok::RParen) {
                        loop {
                            args.push(self.expr()?);
                            match self.peek() {
                                Some(Tok::Comma) => self.i += 1,
                                Some(Tok::RParen) => break,
                                _ => return self.err("expected ',' or ')'"),
                            }
                        }
                    }
                    self.i += 1;
                    return call(&name, &args);
                }
                if name == "pi" {
                    return Ok(std::f64::consts::PI);
                }
                (self.vars)(&name).ok_or(ExprError::UnknownIdentifier(name))
            }
            Some(t) => self.err(format!("unexpected token {t:?}")),
            None => self.err("unexpected end of expression"),
        }
    }
}

fn call(name: &str, a: &[f64]) -> Result<f64, ExprError> {
    let one = |f: fn(f64) -> f64| -> Result<f64, ExprError> {
        if a.len() != 1 {
            return Err(ExprError::Arity { name: name.into(), expected: "1", got: a.len() });
        }
        Ok(f(a[0]))
    };
    match name {
        "sin" => one(|x| x.to_radians().sin()),
        "cos" => one(|x| x.to_radians().cos()),
        "tan" => one(|x| x.to_radians().tan()),
        "asin" => one(|x| x.asin().to_degrees()),
        "acos" => one(|x| x.acos().to_degrees()),
        "atan" => one(|x| x.atan().to_degrees()),
        "sqrt" => one(f64::sqrt),
        "abs" => one(f64::abs),
        "round" => one(f64::round),
        "floor" => one(f64::floor),
        "ceil" => one(f64::ceil),
        "atan2" => {
            if a.len() != 2 {
                return Err(ExprError::Arity { name: name.into(), expected: "2", got: a.len() });
            }
            Ok(a[0].atan2(a[1]).to_degrees())
        }
        "min" | "max" => {
            if a.is_empty() {
                return Err(ExprError::Arity { name: name.into(), expected: "at least 1", got: 0 });
            }
            let it = a.iter().copied();
            Ok(if name == "min" { it.fold(f64::INFINITY, f64::min) } else { it.fold(f64::NEG_INFINITY, f64::max) })
        }
        _ => Err(ExprError::UnknownFunction(name.into())),
    }
}

/// Evaluate `expr`, resolving identifiers through `vars`.
pub fn eval(expr: &str, vars: &dyn Fn(&str) -> Option<f64>) -> Result<f64, ExprError> {
    let toks = tokenize(expr)?;
    let mut p = Parser { toks, i: 0, len: expr.len(), vars };
    let v = p.expr()?;
    if p.i < p.toks.len() {
        return p.err("unexpected trailing input");
    }
    if !v.is_finite() {
        return Err(ExprError::NotFinite);
    }
    Ok(v)
}

/// Evaluate with a variable map.
pub fn eval_map(expr: &str, vars: &HashMap<String, f64>) -> Result<f64, ExprError> {
    eval(expr, &|n| vars.get(n).copied())
}

/// Evaluate an expression that must not reference any variable (e.g. `"1/4 in"`).
pub fn eval_const(expr: &str) -> Result<f64, ExprError> {
    eval(expr, &|_| None)
}

/// Variable names referenced by `expr` (functions, unit suffixes and `pi` excluded), in order of
/// first appearance. Unparseable input yields the identifiers that could be tokenized.
pub fn identifiers(expr: &str) -> Vec<String> {
    let Ok(toks) = tokenize(expr) else { return vec![] };
    let mut out: Vec<String> = Vec::new();
    for (k, (t, _)) in toks.iter().enumerate() {
        let Tok::Ident(name) = t else { continue };
        let is_call = matches!(toks.get(k + 1), Some((Tok::LParen, _))) && FUNCTIONS.contains(&name.as_str());
        let is_unit = k > 0 && matches!(toks[k - 1].0, Tok::Num(_) | Tok::RParen) && unit_factor(name).is_some();
        if !is_call && !is_unit && name != "pi" && !out.contains(name) {
            out.push(name.clone());
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ev(s: &str) -> f64 {
        eval_const(s).unwrap()
    }

    #[test]
    fn arithmetic_and_precedence() {
        assert_eq!(ev("1 + 2 * 3"), 7.0);
        assert_eq!(ev("(1 + 2) * 3"), 9.0);
        assert_eq!(ev("2^3^2"), 512.0);
        assert_eq!(ev("-2^2"), -4.0);
        assert_eq!(ev("2^-1"), 0.5);
        assert_eq!(ev("10 / 4 - 1"), 1.5);
        assert!((ev("1e-3") - 0.001).abs() < 1e-15);
        assert!((ev("2.5E2") - 250.0).abs() < 1e-12);
        assert!((ev("pi") - std::f64::consts::PI).abs() < 1e-15);
    }

    #[test]
    fn units() {
        assert!((ev("1in") - 25.4).abs() < 1e-12);
        assert!((ev("1/4 in") - 1.0 / 101.6).abs() < 1e-12, "unit binds to the number only: write (1/4)in");
        assert!((ev("(1/4)in") - 6.35).abs() < 1e-12);
        assert!((ev("2cm + 5mm") - 25.0).abs() < 1e-12);
        assert!((ev("1.5 m") - 1500.0).abs() < 1e-12);
        assert!((ev("1ft") - 304.8).abs() < 1e-12);
        assert!(eval_const("pi rad").is_err(), "units only follow numbers or (groups)");
        assert!((ev("(pi)rad") - 180.0).abs() < 1e-9);
        assert!((ev("2 inch") - 50.8).abs() < 1e-12);
    }

    #[test]
    fn functions_use_degrees() {
        assert!((ev("sin(30)") - 0.5).abs() < 1e-12);
        assert!((ev("cos(60deg)") - 0.5).abs() < 1e-12);
        assert!((ev("atan2(1, 1)") - 45.0).abs() < 1e-12);
        assert!((ev("acos(0)") - 90.0).abs() < 1e-12);
        assert_eq!(ev("max(1, 7, 3) + min(4, 2)"), 9.0);
        assert_eq!(ev("round(2.6) + floor(1.9) + ceil(0.1) + abs(-2) + sqrt(16)"), 3.0 + 1.0 + 1.0 + 2.0 + 4.0);
    }

    #[test]
    fn variables_and_identifiers() {
        let mut m = HashMap::new();
        m.insert("width".to_string(), 80.0);
        m.insert("wall".to_string(), 2.0);
        assert_eq!(eval_map("width - 2*wall", &m).unwrap(), 76.0);
        assert_eq!(identifiers("width/2 - wall*sin(30) + 3mm + pi + (1)in + width"), vec!["width", "wall"]);
        assert_eq!(eval_map("depth", &m), Err(ExprError::UnknownIdentifier("depth".into())));
    }

    #[test]
    fn errors() {
        assert!(matches!(eval_const("1 +"), Err(ExprError::Parse { pos: 3, .. })));
        assert!(matches!(eval_const("(1 + 2"), Err(ExprError::Parse { .. })));
        assert!(matches!(eval_const("2 $ 3"), Err(ExprError::Parse { pos: 2, .. })));
        assert_eq!(eval_const("1/0"), Err(ExprError::NotFinite));
        assert!(matches!(eval_const("foo(1)"), Err(ExprError::UnknownFunction(_))));
        assert!(matches!(eval_const("atan2(1)"), Err(ExprError::Arity { .. })));
        assert!(matches!(eval_const("1 2"), Err(ExprError::Parse { .. })));
    }
}
