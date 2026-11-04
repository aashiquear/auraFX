//! Calculator: evaluates an expression per point (or per cell) producing a new array.
//!
//! Grammar: standard arithmetic with `+ - * / ^`, unary minus, parentheses, function calls,
//! scalar and 3-vector values. Variables are array names (quote names with spaces as
//! `"my array"`), plus `coordsX/Y/Z`, `coords`, components as `name_X` / `name_0`, and
//! `iHat jHat kHat`, `pi`, `e`.

use crate::params::Association;
use anyhow::{anyhow, bail, Result};
use aurafx_data::*;
use rayon::prelude::*;
use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    S(f64),
    V([f64; 3]),
}

impl Value {
    fn map(self, f: impl Fn(f64) -> f64) -> Value {
        match self {
            Value::S(s) => Value::S(f(s)),
            Value::V(v) => Value::V([f(v[0]), f(v[1]), f(v[2])]),
        }
    }
    fn zip(self, o: Value, f: impl Fn(f64, f64) -> f64) -> Result<Value> {
        Ok(match (self, o) {
            (Value::S(a), Value::S(b)) => Value::S(f(a, b)),
            (Value::V(a), Value::S(b)) => Value::V([f(a[0], b), f(a[1], b), f(a[2], b)]),
            (Value::S(a), Value::V(b)) => Value::V([f(a, b[0]), f(a, b[1]), f(a, b[2])]),
            (Value::V(a), Value::V(b)) => Value::V([f(a[0], b[0]), f(a[1], b[1]), f(a[2], b[2])]),
        })
    }
    fn scalar(&self) -> Result<f64> {
        match self {
            Value::S(s) => Ok(*s),
            Value::V(_) => bail!("expected a scalar, got a vector"),
        }
    }
    fn vector(&self) -> Result<[f64; 3]> {
        match self {
            Value::V(v) => Ok(*v),
            Value::S(_) => bail!("expected a vector, got a scalar"),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
enum Tok {
    Num(f64),
    Ident(String),
    Op(char),
    LParen,
    RParen,
    Comma,
}

fn tokenize(src: &str) -> Result<Vec<Tok>> {
    let mut out = Vec::new();
    let chars: Vec<char> = src.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c.is_whitespace() {
            i += 1;
        } else if c.is_ascii_digit() || (c == '.' && i + 1 < chars.len() && chars[i + 1].is_ascii_digit()) {
            let start = i;
            while i < chars.len() && (chars[i].is_ascii_digit() || chars[i] == '.') {
                i += 1;
            }
            if i < chars.len() && (chars[i] == 'e' || chars[i] == 'E') {
                let save = i;
                i += 1;
                if i < chars.len() && (chars[i] == '+' || chars[i] == '-') {
                    i += 1;
                }
                if i < chars.len() && chars[i].is_ascii_digit() {
                    while i < chars.len() && chars[i].is_ascii_digit() {
                        i += 1;
                    }
                } else {
                    i = save;
                }
            }
            let s: String = chars[start..i].iter().collect();
            out.push(Tok::Num(s.parse().map_err(|_| anyhow!("bad number {s:?}"))?));
        } else if c.is_alphabetic() || c == '_' {
            let start = i;
            while i < chars.len() && (chars[i].is_alphanumeric() || chars[i] == '_') {
                i += 1;
            }
            out.push(Tok::Ident(chars[start..i].iter().collect()));
        } else if c == '"' || c == '\'' {
            let q = c;
            i += 1;
            let start = i;
            while i < chars.len() && chars[i] != q {
                i += 1;
            }
            out.push(Tok::Ident(chars[start..i].iter().collect()));
            i += 1;
        } else {
            match c {
                '(' => out.push(Tok::LParen),
                ')' => out.push(Tok::RParen),
                ',' => out.push(Tok::Comma),
                '+' | '-' | '*' | '/' | '^' | '%' | '<' | '>' | '=' | '!' | '&' | '|' => {
                    // Two-char comparison ops.
                    if i + 1 < chars.len() && chars[i + 1] == '=' && matches!(c, '<' | '>' | '=' | '!') {
                        out.push(Tok::Op(match c {
                            '<' => 'l',
                            '>' => 'g',
                            '=' => 'q',
                            _ => 'n',
                        }));
                        i += 1;
                    } else if i + 1 < chars.len() && chars[i + 1] == c && matches!(c, '&' | '|') {
                        out.push(Tok::Op(c));
                        i += 1;
                    } else {
                        out.push(Tok::Op(c));
                    }
                }
                _ => bail!("unexpected character {c:?}"),
            }
            i += 1;
        }
    }
    Ok(out)
}

#[derive(Debug, Clone)]
pub enum Expr {
    Num(f64),
    Var(String),
    Neg(Box<Expr>),
    Not(Box<Expr>),
    Bin(char, Box<Expr>, Box<Expr>),
    Call(String, Vec<Expr>),
    Cond(Box<Expr>, Box<Expr>, Box<Expr>),
}

struct Parser {
    toks: Vec<Tok>,
    pos: usize,
}

impl Parser {
    fn peek(&self) -> Option<&Tok> {
        self.toks.get(self.pos)
    }
    fn next(&mut self) -> Option<Tok> {
        let t = self.toks.get(self.pos).cloned();
        self.pos += 1;
        t
    }
    fn expect(&mut self, t: Tok) -> Result<()> {
        match self.next() {
            Some(x) if x == t => Ok(()),
            other => bail!("expected {t:?}, found {other:?}"),
        }
    }
    fn prec(op: char) -> u8 {
        match op {
            '|' => 1,
            '&' => 2,
            'q' | 'n' | '<' | '>' | 'l' | 'g' => 3,
            '+' | '-' => 4,
            '*' | '/' | '%' => 5,
            '^' => 7,
            _ => 0,
        }
    }
    fn expr(&mut self, min_prec: u8) -> Result<Expr> {
        let mut lhs = self.unary()?;
        loop {
            let Some(Tok::Op(op)) = self.peek().cloned() else { break };
            let p = Self::prec(op);
            if p == 0 || p < min_prec {
                break;
            }
            self.next();
            let rhs = if op == '^' { self.expr(p)? } else { self.expr(p + 1)? };
            lhs = Expr::Bin(op, Box::new(lhs), Box::new(rhs));
        }
        Ok(lhs)
    }
    fn unary(&mut self) -> Result<Expr> {
        match self.peek() {
            Some(Tok::Op('-')) => {
                self.next();
                Ok(Expr::Neg(Box::new(self.expr(6)?)))
            }
            Some(Tok::Op('+')) => {
                self.next();
                self.expr(6)
            }
            Some(Tok::Op('!')) => {
                self.next();
                Ok(Expr::Not(Box::new(self.expr(6)?)))
            }
            _ => self.atom(),
        }
    }
    fn atom(&mut self) -> Result<Expr> {
        match self.next() {
            Some(Tok::Num(n)) => Ok(Expr::Num(n)),
            Some(Tok::LParen) => {
                let e = self.expr(1)?;
                self.expect(Tok::RParen)?;
                Ok(e)
            }
            Some(Tok::Ident(name)) => {
                if self.peek() == Some(&Tok::LParen) {
                    self.next();
                    let mut args = Vec::new();
                    if self.peek() != Some(&Tok::RParen) {
                        loop {
                            args.push(self.expr(1)?);
                            if self.peek() == Some(&Tok::Comma) {
                                self.next();
                            } else {
                                break;
                            }
                        }
                    }
                    self.expect(Tok::RParen)?;
                    if name == "if" && args.len() == 3 {
                        let mut it = args.into_iter();
                        return Ok(Expr::Cond(Box::new(it.next().unwrap()), Box::new(it.next().unwrap()), Box::new(it.next().unwrap())));
                    }
                    Ok(Expr::Call(name, args))
                } else {
                    Ok(Expr::Var(name))
                }
            }
            other => bail!("unexpected token {other:?}"),
        }
    }
}

pub fn parse(src: &str) -> Result<Expr> {
    let toks = tokenize(src)?;
    if toks.is_empty() {
        bail!("empty expression");
    }
    let mut p = Parser { toks, pos: 0 };
    let e = p.expr(1)?;
    if p.pos != p.toks.len() {
        bail!("unexpected trailing tokens in expression");
    }
    Ok(e)
}

struct Env<'a> {
    arrays: HashMap<String, &'a Array>,
    coords: Option<&'a Points>,
}

fn b(x: bool) -> f64 {
    if x {
        1.0
    } else {
        0.0
    }
}

fn eval(e: &Expr, env: &Env, i: usize) -> Result<Value> {
    Ok(match e {
        Expr::Num(n) => Value::S(*n),
        Expr::Var(name) => lookup(name, env, i)?,
        Expr::Neg(x) => eval(x, env, i)?.map(|v| -v),
        Expr::Not(x) => eval(x, env, i)?.map(|v| b(v == 0.0)),
        Expr::Cond(c, a, bb) => {
            if eval(c, env, i)?.scalar()? != 0.0 {
                eval(a, env, i)?
            } else {
                eval(bb, env, i)?
            }
        }
        Expr::Bin(op, l, r) => {
            let (a, bv) = (eval(l, env, i)?, eval(r, env, i)?);
            match op {
                '+' => a.zip(bv, |x, y| x + y)?,
                '-' => a.zip(bv, |x, y| x - y)?,
                '*' => match (&a, &bv) {
                    (Value::V(x), Value::V(y)) => Value::S(x[0] * y[0] + x[1] * y[1] + x[2] * y[2]),
                    _ => a.zip(bv, |x, y| x * y)?,
                },
                '/' => a.zip(bv, |x, y| x / y)?,
                '%' => a.zip(bv, |x, y| x % y)?,
                '^' => a.zip(bv, |x, y| x.powf(y))?,
                '<' => Value::S(b(a.scalar()? < bv.scalar()?)),
                '>' => Value::S(b(a.scalar()? > bv.scalar()?)),
                'l' => Value::S(b(a.scalar()? <= bv.scalar()?)),
                'g' => Value::S(b(a.scalar()? >= bv.scalar()?)),
                'q' => Value::S(b(a == bv)),
                'n' => Value::S(b(a != bv)),
                '&' => Value::S(b(a.scalar()? != 0.0 && bv.scalar()? != 0.0)),
                '|' => Value::S(b(a.scalar()? != 0.0 || bv.scalar()? != 0.0)),
                _ => bail!("unknown operator {op}"),
            }
        }
        Expr::Call(f, args) => {
            let vals: Vec<Value> = args.iter().map(|a| eval(a, env, i)).collect::<Result<_>>()?;
            let one = || -> Result<Value> { vals.first().cloned().ok_or_else(|| anyhow!("{f}() needs an argument")) };
            match f.as_str() {
                "abs" => one()?.map(f64::abs),
                "sqrt" => one()?.map(f64::sqrt),
                "exp" => one()?.map(f64::exp),
                "ln" | "log" => one()?.map(f64::ln),
                "log10" => one()?.map(f64::log10),
                "log2" => one()?.map(f64::log2),
                "sin" => one()?.map(f64::sin),
                "cos" => one()?.map(f64::cos),
                "tan" => one()?.map(f64::tan),
                "asin" => one()?.map(f64::asin),
                "acos" => one()?.map(f64::acos),
                "atan" => one()?.map(f64::atan),
                "sinh" => one()?.map(f64::sinh),
                "cosh" => one()?.map(f64::cosh),
                "tanh" => one()?.map(f64::tanh),
                "floor" => one()?.map(f64::floor),
                "ceil" => one()?.map(f64::ceil),
                "round" => one()?.map(f64::round),
                "sign" => one()?.map(f64::signum),
                "mag" | "norm" | "length" => Value::S({
                    let v = one()?.vector()?;
                    (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt()
                }),
                "normalize" | "unit" => {
                    let v = one()?.vector()?;
                    let l = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt().max(1e-300);
                    Value::V([v[0] / l, v[1] / l, v[2] / l])
                }
                "dot" => {
                    let (a, bb) = (vals[0].vector()?, vals.get(1).ok_or_else(|| anyhow!("dot needs 2 args"))?.vector()?);
                    Value::S(a[0] * bb[0] + a[1] * bb[1] + a[2] * bb[2])
                }
                "cross" => {
                    let (a, bb) = (vals[0].vector()?, vals.get(1).ok_or_else(|| anyhow!("cross needs 2 args"))?.vector()?);
                    Value::V([a[1] * bb[2] - a[2] * bb[1], a[2] * bb[0] - a[0] * bb[2], a[0] * bb[1] - a[1] * bb[0]])
                }
                "vec" | "vector" => {
                    if vals.len() != 3 {
                        bail!("vec needs 3 args");
                    }
                    Value::V([vals[0].scalar()?, vals[1].scalar()?, vals[2].scalar()?])
                }
                "min" => vals.iter().skip(1).try_fold(vals[0].clone(), |acc, v| acc.zip(v.clone(), f64::min))?,
                "max" => vals.iter().skip(1).try_fold(vals[0].clone(), |acc, v| acc.zip(v.clone(), f64::max))?,
                "pow" => vals[0].clone().zip(vals.get(1).cloned().ok_or_else(|| anyhow!("pow needs 2 args"))?, f64::powf)?,
                "atan2" => Value::S(vals[0].scalar()?.atan2(vals.get(1).ok_or_else(|| anyhow!("atan2 needs 2 args"))?.scalar()?)),
                "clamp" => {
                    let (lo, hi) = (vals[1].scalar()?, vals[2].scalar()?);
                    vals[0].clone().map(|x| x.clamp(lo, hi))
                }
                "x" => Value::S(one()?.vector()?[0]),
                "y" => Value::S(one()?.vector()?[1]),
                "z" => Value::S(one()?.vector()?[2]),
                _ => bail!("unknown function {f}()"),
            }
        }
    })
}

fn lookup(name: &str, env: &Env, i: usize) -> Result<Value> {
    match name {
        "pi" => return Ok(Value::S(std::f64::consts::PI)),
        "e" => return Ok(Value::S(std::f64::consts::E)),
        "iHat" => return Ok(Value::V([1.0, 0.0, 0.0])),
        "jHat" => return Ok(Value::V([0.0, 1.0, 0.0])),
        "kHat" => return Ok(Value::V([0.0, 0.0, 1.0])),
        "coords" | "coordinates" => {
            if let Some(p) = env.coords {
                return Ok(Value::V(p.get(i)));
            }
        }
        "coordsX" | "coordsY" | "coordsZ" => {
            if let Some(p) = env.coords {
                let v = p.get(i);
                return Ok(Value::S(v[(name.as_bytes()[6] - b'X') as usize]));
            }
        }
        _ => {}
    }
    if let Some(a) = env.arrays.get(name) {
        return Ok(if a.ncomp == 3 {
            Value::V([a.get(i, 0), a.get(i, 1), a.get(i, 2)])
        } else if a.ncomp == 1 {
            Value::S(a.get(i, 0))
        } else {
            Value::S(a.magnitude(i))
        });
    }
    // name_X / name_0 component access.
    if let Some((base, suffix)) = name.rsplit_once('_') {
        if let Some(a) = env.arrays.get(base) {
            let c = match suffix {
                "X" | "x" => Some(0),
                "Y" | "y" => Some(1),
                "Z" | "z" => Some(2),
                "Magnitude" | "mag" => None,
                s => s.parse::<usize>().ok(),
            };
            return Ok(Value::S(match c {
                Some(c) if c < a.ncomp => a.get(i, c),
                Some(c) => bail!("component {c} out of range for {base}"),
                None => a.magnitude(i),
            }));
        }
    }
    bail!("unknown variable {name:?}")
}

/// Evaluate `expression` for every point (or cell) and add the result as `result_name`.
pub fn calculator(ds: &DataSet, assoc: Association, expression: &str, result_name: &str) -> Result<DataSet> {
    let expr = parse(expression)?;
    let mut out = ds.clone();
    let coords_owned = if assoc == Association::Points { ds.points() } else { Some(crate::attributes::cell_centers(ds)?.points) };
    let n = match assoc {
        Association::Points => ds.n_points(),
        Association::Cells => ds.n_cells(),
    };
    let attrs = match assoc {
        Association::Points => ds.point_data(),
        Association::Cells => ds.cell_data(),
    }
    .ok_or_else(|| anyhow!("calculator: dataset has no attributes"))?;
    let env = Env { arrays: attrs.iter().map(|a| (a.name.clone(), a)).collect(), coords: coords_owned.as_ref() };
    // Determine result arity from the first tuple (or scalar if empty).
    let first = if n > 0 { eval(&expr, &env, 0)? } else { Value::S(0.0) };
    let nc = if matches!(first, Value::V(_)) { 3 } else { 1 };
    let results: Vec<Result<Value>> = (0..n).into_par_iter().map(|i| eval(&expr, &env, i)).collect();
    let mut data = Vec::with_capacity(n * nc);
    for r in results {
        match r? {
            Value::S(s) if nc == 1 => data.push(s),
            Value::V(v) if nc == 3 => data.extend_from_slice(&v),
            Value::S(s) => data.extend_from_slice(&[s, s, s]),
            Value::V(v) => data.push((v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt()),
        }
    }
    let arr = Array::new(result_name, nc, ArrayData::F64(data));
    let target = match assoc {
        Association::Points => out.point_data_mut(),
        Association::Cells => out.cell_data_mut(),
    }
    .ok_or_else(|| anyhow!("calculator: dataset has no attributes"))?;
    if nc == 1 {
        target.insert_active(AttributeKind::Scalars, arr);
    } else {
        target.insert_active(AttributeKind::Vectors, arr);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arithmetic() {
        let env = Env { arrays: HashMap::new(), coords: None };
        let v = eval(&parse("2 + 3 * 4 ^ 2 / 8 - 1").unwrap(), &env, 0).unwrap();
        assert_eq!(v, Value::S(7.0));
        let v = eval(&parse("mag(vec(3,4,0)) + if(1 < 2, 10, 20)").unwrap(), &env, 0).unwrap();
        assert_eq!(v, Value::S(15.0));
        let v = eval(&parse("cross(iHat, jHat)").unwrap(), &env, 0).unwrap();
        assert_eq!(v, Value::V([0.0, 0.0, 1.0]));
        let v = eval(&parse("-2^2").unwrap(), &env, 0).unwrap();
        assert_eq!(v, Value::S(-4.0));
    }
}
