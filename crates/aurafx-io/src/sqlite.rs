//! A small read-only SQLite reader: the schema table and the rows of ordinary (rowid) tables,
//! enough for GeoPackage files. Follows the file format at https://sqlite.org/fileformat.html.

use anyhow::{anyhow, bail, Context, Result};
use std::path::Path;

/// A value of a row.
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Null,
    Int(i64),
    Real(f64),
    Text(String),
    Blob(Vec<u8>),
}

impl Value {
    pub fn as_i64(&self) -> Option<i64> {
        match self {
            Value::Int(v) => Some(*v),
            Value::Real(v) => Some(*v as i64),
            Value::Text(s) => s.trim().parse().ok(),
            _ => None,
        }
    }
    pub fn as_f64(&self) -> Option<f64> {
        match self {
            Value::Int(v) => Some(*v as f64),
            Value::Real(v) => Some(*v),
            Value::Text(s) => s.trim().parse().ok(),
            _ => None,
        }
    }
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Value::Text(s) => Some(s),
            _ => None,
        }
    }
}

/// One entry of the schema table (`sqlite_master`).
#[derive(Debug, Clone)]
pub struct SchemaEntry {
    pub kind: String,
    pub name: String,
    pub rootpage: u32,
    pub sql: String,
}

pub struct Database {
    data: Vec<u8>,
    page_size: usize,
    usable: usize,
    pub schema: Vec<SchemaEntry>,
}

fn varint(b: &[u8], mut i: usize) -> Result<(u64, usize)> {
    let mut v: u64 = 0;
    for k in 0..9 {
        let c = *b.get(i).ok_or_else(|| anyhow!("truncated varint"))?;
        i += 1;
        if k == 8 {
            return Ok(((v << 8) | c as u64, i));
        }
        v = (v << 7) | (c & 0x7f) as u64;
        if c & 0x80 == 0 {
            return Ok((v, i));
        }
    }
    unreachable!()
}

fn be(b: &[u8]) -> u64 {
    b.iter().fold(0, |a, &x| (a << 8) | x as u64)
}

impl Database {
    pub fn open(path: &Path) -> Result<Self> {
        let data = std::fs::read(path).with_context(|| format!("reading {}", path.display()))?;
        if !data.starts_with(b"SQLite format 3\0") || data.len() < 100 {
            bail!("{} is not an SQLite database", path.display());
        }
        let ps = u16::from_be_bytes([data[16], data[17]]) as usize;
        let page_size = if ps == 1 { 65536 } else { ps };
        let usable = page_size - data[20] as usize;
        if data[56..60] != [0, 0, 0, 1] && data[56..60] != [0, 0, 0, 0] {
            bail!("{}: only UTF-8 SQLite databases are supported", path.display());
        }
        let mut db = Database { data, page_size, usable, schema: Vec::new() };
        let mut schema = Vec::new();
        db.scan(1, &mut |_, rec| {
            let s = |i: usize| rec.get(i).and_then(|v| v.as_str()).unwrap_or("").to_string();
            schema.push(SchemaEntry { kind: s(0), name: s(1), rootpage: rec.get(3).and_then(|v| v.as_i64()).unwrap_or(0) as u32, sql: s(4) });
            Ok(())
        })?;
        db.schema = schema;
        Ok(db)
    }

    pub fn table(&self, name: &str) -> Option<&SchemaEntry> {
        self.schema.iter().find(|e| e.kind == "table" && e.name.eq_ignore_ascii_case(name))
    }

    /// Column names and declared types of a table, from its CREATE TABLE statement.
    pub fn columns(&self, table: &str) -> Result<Vec<(String, String)>> {
        let e = self.table(table).ok_or_else(|| anyhow!("no table {table}"))?;
        parse_columns(&e.sql)
    }

    /// Every row of a table: (rowid, values in column order; an INTEGER PRIMARY KEY column
    /// holds the rowid).
    pub fn rows(&self, table: &str) -> Result<Vec<(i64, Vec<Value>)>> {
        let e = self.table(table).ok_or_else(|| anyhow!("no table {table}"))?;
        let cols = parse_columns(&e.sql)?;
        // An INTEGER PRIMARY KEY column is the rowid (stored as NULL in the record).
        let pk = cols.iter().position(|(_, t)| {
            let t = t.to_ascii_uppercase();
            t.starts_with("INTEGER") && t.contains("PRIMARY KEY")
        });
        let mut out = Vec::new();
        self.scan(e.rootpage, &mut |rowid, mut rec| {
            rec.resize(cols.len().max(rec.len()), Value::Null);
            if let Some(k) = pk {
                if rec[k] == Value::Null {
                    rec[k] = Value::Int(rowid);
                }
            }
            out.push((rowid, rec));
            Ok(())
        })?;
        Ok(out)
    }

    fn page(&self, n: u32) -> Result<&[u8]> {
        let start = (n as usize).checked_sub(1).ok_or_else(|| anyhow!("page 0"))? * self.page_size;
        self.data.get(start..start + self.page_size).ok_or_else(|| anyhow!("page {n} is past the end of the file"))
    }

    /// Walk a table b-tree in rowid order.
    fn scan(&self, root: u32, f: &mut dyn FnMut(i64, Vec<Value>) -> Result<()>) -> Result<()> {
        let mut stack = vec![root];
        let mut depth = 0;
        // Depth-first, children in order: push in reverse.
        while let Some(pg) = stack.pop() {
            depth += 1;
            if depth > 10_000_000 {
                bail!("SQLite b-tree loop");
            }
            let page = self.page(pg)?;
            let h = if pg == 1 { 100 } else { 0 };
            let kind = page[h];
            let ncells = u16::from_be_bytes([page[h + 3], page[h + 4]]) as usize;
            match kind {
                0x05 => {
                    let right = be(&page[h + 8..h + 12]) as u32;
                    let mut kids = Vec::with_capacity(ncells + 1);
                    for c in 0..ncells {
                        let off = u16::from_be_bytes([page[h + 12 + 2 * c], page[h + 13 + 2 * c]]) as usize;
                        kids.push(be(&page[off..off + 4]) as u32);
                    }
                    kids.push(right);
                    stack.extend(kids.into_iter().rev());
                }
                0x0d => {
                    for c in 0..ncells {
                        let off = u16::from_be_bytes([page[h + 8 + 2 * c], page[h + 9 + 2 * c]]) as usize;
                        let (size, i) = varint(page, off)?;
                        let (rowid, i) = varint(page, i)?;
                        let payload = self.payload(page, i, size as usize)?;
                        f(rowid as i64, record(&payload)?)?;
                    }
                }
                k => bail!("unexpected SQLite page type {k} in a table"),
            }
        }
        Ok(())
    }

    /// The payload of a leaf table cell starting at `i`, following overflow pages.
    fn payload(&self, page: &[u8], i: usize, size: usize) -> Result<Vec<u8>> {
        let u = self.usable;
        let x = u - 35;
        if size <= x {
            return Ok(page[i..i + size].to_vec());
        }
        let m = ((u - 12) * 32 / 255) - 23;
        let k = m + ((size - m) % (u - 4));
        let local = if k <= x { k } else { m };
        let mut out = Vec::with_capacity(size);
        out.extend_from_slice(&page[i..i + local]);
        let mut next = be(&page[i + local..i + local + 4]) as u32;
        while out.len() < size {
            if next == 0 {
                bail!("SQLite overflow chain ends early");
            }
            let p = self.page(next)?;
            next = be(&p[0..4]) as u32;
            let take = (size - out.len()).min(u - 4);
            out.extend_from_slice(&p[4..4 + take]);
        }
        Ok(out)
    }
}

fn record(p: &[u8]) -> Result<Vec<Value>> {
    let (hsize, mut i) = varint(p, 0)?;
    let mut types = Vec::new();
    while i < hsize as usize {
        let (t, j) = varint(p, i)?;
        types.push(t);
        i = j;
    }
    let mut at = hsize as usize;
    let mut out = Vec::with_capacity(types.len());
    let get = |at: usize, n: usize| p.get(at..at + n).ok_or_else(|| anyhow!("truncated SQLite record"));
    for t in types {
        let (v, n) = match t {
            0 => (Value::Null, 0),
            1..=6 => {
                let n = [0, 1, 2, 3, 4, 6, 8][t as usize];
                let b = get(at, n)?;
                let raw = be(b);
                let shift = 64 - 8 * n as u32;
                (Value::Int(((raw << shift) as i64) >> shift), n)
            }
            7 => (Value::Real(f64::from_bits(be(get(at, 8)?))), 8),
            8 => (Value::Int(0), 0),
            9 => (Value::Int(1), 0),
            t if t >= 12 && t % 2 == 0 => {
                let n = ((t - 12) / 2) as usize;
                (Value::Blob(get(at, n)?.to_vec()), n)
            }
            t if t >= 13 => {
                let n = ((t - 13) / 2) as usize;
                (Value::Text(String::from_utf8_lossy(get(at, n)?).into_owned()), n)
            }
            t => bail!("bad SQLite serial type {t}"),
        };
        at += n;
        out.push(v);
    }
    Ok(out)
}

/// (name, declared type and constraints) of each column of a CREATE TABLE statement.
pub fn parse_columns(sql: &str) -> Result<Vec<(String, String)>> {
    let open = sql.find('(').ok_or_else(|| anyhow!("no column list in {sql}"))?;
    let close = sql.rfind(')').ok_or_else(|| anyhow!("no column list in {sql}"))?;
    let body = &sql[open + 1..close];
    // Split at top-level commas (types like DECIMAL(10,2) have their own).
    let (mut parts, mut depth, mut cur, mut quote) = (Vec::new(), 0, String::new(), None::<char>);
    for c in body.chars() {
        match (quote, c) {
            (Some(q), c) if c == q => {
                quote = None;
                cur.push(c)
            }
            (Some(_), c) => cur.push(c),
            (None, '"' | '`' | '\'' | '[') => {
                quote = Some(if c == '[' { ']' } else { c });
                cur.push(c)
            }
            (None, '(') => {
                depth += 1;
                cur.push(c)
            }
            (None, ')') => {
                depth -= 1;
                cur.push(c)
            }
            (None, ',') if depth == 0 => parts.push(std::mem::take(&mut cur)),
            (None, c) => cur.push(c),
        }
    }
    parts.push(cur);
    let mut cols = Vec::new();
    for p in parts {
        let p = p.trim();
        let upper = p.to_ascii_uppercase();
        if ["CONSTRAINT", "PRIMARY KEY", "UNIQUE", "CHECK", "FOREIGN KEY"].iter().any(|k| upper.starts_with(k)) || p.is_empty() {
            continue;
        }
        let (name, rest) = match p.chars().next() {
            Some(q @ ('"' | '`' | '[')) => {
                let end = if q == '[' { ']' } else { q };
                let e = p[1..].find(end).ok_or_else(|| anyhow!("unterminated name in {p}"))? + 1;
                (p[1..e].to_string(), p[e + 1..].trim().to_string())
            }
            _ => {
                let e = p.find(char::is_whitespace).unwrap_or(p.len());
                (p[..e].to_string(), p[e..].trim().to_string())
            }
        };
        cols.push((name, rest));
    }
    Ok(cols)
}
