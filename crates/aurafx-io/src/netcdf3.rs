//! Pure-Rust NetCDF-3 reader (classic, 64-bit offset and CDF-5), memory-mapped.

use anyhow::{anyhow, bail, Context, Result};
use std::collections::HashMap;
use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NcType {
    Byte,
    Char,
    Short,
    Int,
    Float,
    Double,
    UByte,
    UShort,
    UInt,
    Int64,
    UInt64,
}

impl NcType {
    fn from_code(c: u32) -> Result<Self> {
        Ok(match c {
            1 => NcType::Byte,
            2 => NcType::Char,
            3 => NcType::Short,
            4 => NcType::Int,
            5 => NcType::Float,
            6 => NcType::Double,
            7 => NcType::UByte,
            8 => NcType::UShort,
            9 => NcType::UInt,
            10 => NcType::Int64,
            11 => NcType::UInt64,
            _ => bail!("unknown NetCDF type code {c}"),
        })
    }
    pub fn size(self) -> usize {
        match self {
            NcType::Byte | NcType::Char | NcType::UByte => 1,
            NcType::Short | NcType::UShort => 2,
            NcType::Int | NcType::Float | NcType::UInt => 4,
            NcType::Double | NcType::Int64 | NcType::UInt64 => 8,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Dim {
    pub name: String,
    pub len: usize,
    pub unlimited: bool,
}

#[derive(Debug, Clone)]
pub enum AttrValue {
    Text(String),
    Numbers(Vec<f64>),
}

#[derive(Debug, Clone)]
pub struct Var {
    pub name: String,
    pub dims: Vec<usize>,
    pub attrs: HashMap<String, AttrValue>,
    pub ty: NcType,
    pub vsize: usize,
    pub begin: usize,
    pub is_record: bool,
}

pub struct NcFile {
    map: memmap2::Mmap,
    pub dims: Vec<Dim>,
    pub attrs: HashMap<String, AttrValue>,
    pub vars: Vec<Var>,
    pub numrecs: usize,
    recsize: usize,
    var_index: HashMap<String, usize>,
}

struct Rd<'a> {
    b: &'a [u8],
    p: usize,
    v5: bool,
}

impl Rd<'_> {
    fn u32(&mut self) -> Result<u32> {
        let s = self.b.get(self.p..self.p + 4).ok_or_else(|| anyhow!("truncated NetCDF header"))?;
        self.p += 4;
        Ok(u32::from_be_bytes(s.try_into().unwrap()))
    }
    fn u64(&mut self) -> Result<u64> {
        let s = self.b.get(self.p..self.p + 8).ok_or_else(|| anyhow!("truncated NetCDF header"))?;
        self.p += 8;
        Ok(u64::from_be_bytes(s.try_into().unwrap()))
    }
    fn nelems(&mut self) -> Result<usize> {
        Ok(if self.v5 { self.u64()? as usize } else { self.u32()? as usize })
    }
    fn name(&mut self) -> Result<String> {
        let n = self.nelems()?;
        let s = self.b.get(self.p..self.p + n).ok_or_else(|| anyhow!("truncated name"))?;
        let name = String::from_utf8_lossy(s).to_string();
        self.p += n.div_ceil(4) * 4;
        Ok(name)
    }
    fn attrs(&mut self) -> Result<HashMap<String, AttrValue>> {
        let tag = self.u32()?;
        let n = self.nelems()?;
        let mut m = HashMap::new();
        if tag == 0 {
            return Ok(m);
        }
        if tag != 0x0C {
            bail!("expected attribute list tag, got {tag:#x}");
        }
        for _ in 0..n {
            let name = self.name()?;
            let ty = NcType::from_code(self.u32()?)?;
            let count = self.nelems()?;
            let nbytes = count * ty.size();
            let raw = self.b.get(self.p..self.p + nbytes).ok_or_else(|| anyhow!("truncated attribute"))?;
            let v = if ty == NcType::Char {
                AttrValue::Text(String::from_utf8_lossy(raw).trim_end_matches('\0').to_string())
            } else {
                AttrValue::Numbers(decode_f64(ty, raw))
            };
            self.p += nbytes.div_ceil(4) * 4;
            m.insert(name, v);
        }
        Ok(m)
    }
}

pub fn decode_f64(ty: NcType, raw: &[u8]) -> Vec<f64> {
    let sz = ty.size();
    raw.chunks_exact(sz)
        .map(|c| match ty {
            NcType::Byte => c[0] as i8 as f64,
            NcType::Char | NcType::UByte => c[0] as f64,
            NcType::Short => i16::from_be_bytes([c[0], c[1]]) as f64,
            NcType::UShort => u16::from_be_bytes([c[0], c[1]]) as f64,
            NcType::Int => i32::from_be_bytes(c.try_into().unwrap()) as f64,
            NcType::UInt => u32::from_be_bytes(c.try_into().unwrap()) as f64,
            NcType::Float => f32::from_be_bytes(c.try_into().unwrap()) as f64,
            NcType::Double => f64::from_be_bytes(c.try_into().unwrap()),
            NcType::Int64 => i64::from_be_bytes(c.try_into().unwrap()) as f64,
            NcType::UInt64 => u64::from_be_bytes(c.try_into().unwrap()) as f64,
        })
        .collect()
}

impl NcFile {
    pub fn open(path: &Path) -> Result<Self> {
        let f = std::fs::File::open(path).with_context(|| format!("opening {}", path.display()))?;
        // SAFETY: read-only mapping of a file we do not modify.
        let map = unsafe { memmap2::Mmap::map(&f)? };
        if map.len() < 8 || &map[..3] != b"CDF" {
            if map.len() >= 8 && &map[1..4] == b"HDF" {
                bail!("this is a NetCDF-4/HDF5 file; NetCDF-4 Exodus support needs the `hdf5` feature (not built in yet)");
            }
            bail!("not a NetCDF-3 file");
        }
        let version = map[3];
        let (v64, v5) = match version {
            1 => (false, false),
            2 => (true, false),
            5 => (true, true),
            v => bail!("unsupported NetCDF version {v}"),
        };
        let mut r = Rd { b: &map, p: 4, v5 };
        let numrecs_raw = if v5 { r.u64()? } else { r.u32()? as u64 };
        // Dimensions.
        let tag = r.u32()?;
        let ndims = r.nelems()?;
        let mut dims = Vec::new();
        if tag == 0x0A {
            for _ in 0..ndims {
                let name = r.name()?;
                let len = r.nelems()?;
                dims.push(Dim { name, len, unlimited: len == 0 });
            }
        } else if tag != 0 {
            bail!("bad dimension list tag {tag:#x}");
        }
        let attrs = r.attrs()?;
        // Variables.
        let tag = r.u32()?;
        let nvars = r.nelems()?;
        let mut vars = Vec::new();
        if tag == 0x0B {
            for _ in 0..nvars {
                let name = r.name()?;
                let nd = r.nelems()?;
                let mut vd = Vec::with_capacity(nd);
                for _ in 0..nd {
                    vd.push(r.nelems()?);
                }
                let vattrs = r.attrs()?;
                let ty = NcType::from_code(r.u32()?)?;
                let vsize = r.nelems()?;
                let begin = if v64 { r.u64()? as usize } else { r.u32()? as usize };
                let is_record = vd.first().map(|&d| dims.get(d).map(|x| x.unlimited).unwrap_or(false)).unwrap_or(false);
                vars.push(Var { name, dims: vd, attrs: vattrs, ty, vsize, begin, is_record });
            }
        } else if tag != 0 {
            bail!("bad variable list tag {tag:#x}");
        }
        let rec_vars: Vec<&Var> = vars.iter().filter(|v| v.is_record).collect();
        let recsize = if rec_vars.len() == 1 {
            // Special case: a single record variable is not padded.
            let v = rec_vars[0];
            v.dims.iter().skip(1).map(|&d| dims[d].len).product::<usize>() * v.ty.size()
        } else {
            rec_vars.iter().map(|v| v.vsize).sum()
        };
        let numrecs = if numrecs_raw == 0xFFFF_FFFF || numrecs_raw == u64::MAX {
            // Streaming: infer from file size.
            let first = rec_vars.iter().map(|v| v.begin).min().unwrap_or(map.len());
            if recsize > 0 {
                (map.len() - first) / recsize
            } else {
                0
            }
        } else {
            numrecs_raw as usize
        };
        let var_index = vars.iter().enumerate().map(|(i, v)| (v.name.clone(), i)).collect();
        Ok(Self { map, dims, attrs, vars, numrecs, recsize, var_index })
    }

    pub fn var(&self, name: &str) -> Option<&Var> {
        self.var_index.get(name).map(|&i| &self.vars[i])
    }

    pub fn dim_len(&self, name: &str) -> Option<usize> {
        self.dims.iter().find(|d| d.name == name).map(|d| if d.unlimited { self.numrecs } else { d.len })
    }

    /// Shape excluding the record dimension.
    pub fn shape(&self, v: &Var) -> Vec<usize> {
        v.dims.iter().skip(if v.is_record { 1 } else { 0 }).map(|&d| self.dims[d].len).collect()
    }

    fn slice(&self, v: &Var, record: Option<usize>) -> Result<&[u8]> {
        let n: usize = self.shape(v).iter().product::<usize>() * v.ty.size();
        let start = if v.is_record { v.begin + record.unwrap_or(0) * self.recsize } else { v.begin };
        self.map.get(start..start + n).ok_or_else(|| anyhow!("variable {} extends past end of file", v.name))
    }

    /// Read a non-record variable (or one record of a record variable) as f64.
    pub fn read_f64(&self, name: &str, record: Option<usize>) -> Result<Vec<f64>> {
        let v = self.var(name).ok_or_else(|| anyhow!("no variable {name}"))?;
        Ok(decode_f64(v.ty, self.slice(v, record)?))
    }

    /// Read every record of a record variable (e.g. time values).
    pub fn read_all_records(&self, name: &str) -> Result<Vec<f64>> {
        let mut out = Vec::new();
        for r in 0..self.numrecs {
            out.extend(self.read_f64(name, Some(r))?);
        }
        Ok(out)
    }

    /// Read a 2-D char variable as a list of strings.
    pub fn read_strings(&self, name: &str) -> Result<Vec<String>> {
        let v = self.var(name).ok_or_else(|| anyhow!("no variable {name}"))?;
        let shape = self.shape(v);
        let raw = self.slice(v, None)?;
        let len = *shape.last().unwrap_or(&1);
        Ok(raw.chunks(len.max(1)).map(|c| String::from_utf8_lossy(c).trim_end_matches('\0').trim().to_string()).collect())
    }

    pub fn var_attr_text(&self, var: &str, attr: &str) -> Option<String> {
        match self.var(var)?.attrs.get(attr)? {
            AttrValue::Text(s) => Some(s.clone()),
            _ => None,
        }
    }
}
