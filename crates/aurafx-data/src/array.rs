//! Typed, multi-component data arrays.

use half::f16;
use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use std::fmt;

/// Element type of an [`Array`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ScalarType {
    I8,
    U8,
    I16,
    U16,
    I32,
    U32,
    I64,
    U64,
    F16,
    F32,
    F64,
    String,
}

impl ScalarType {
    pub fn size_of(self) -> usize {
        match self {
            ScalarType::I8 | ScalarType::U8 => 1,
            ScalarType::I16 | ScalarType::U16 | ScalarType::F16 => 2,
            ScalarType::I32 | ScalarType::U32 | ScalarType::F32 => 4,
            ScalarType::I64 | ScalarType::U64 | ScalarType::F64 => 8,
            ScalarType::String => 0,
        }
    }

    /// VTK XML type name ("Float32", "Int64", ...).
    pub fn vtk_name(self) -> &'static str {
        match self {
            ScalarType::I8 => "Int8",
            ScalarType::U8 => "UInt8",
            ScalarType::I16 => "Int16",
            ScalarType::U16 => "UInt16",
            ScalarType::I32 => "Int32",
            ScalarType::U32 => "UInt32",
            ScalarType::I64 => "Int64",
            ScalarType::U64 => "UInt64",
            ScalarType::F16 => "Float16",
            ScalarType::F32 => "Float32",
            ScalarType::F64 => "Float64",
            ScalarType::String => "String",
        }
    }

    pub fn from_vtk_name(name: &str) -> Option<Self> {
        Some(match name {
            "Int8" | "char" => ScalarType::I8,
            "UInt8" | "unsigned_char" | "bit" => ScalarType::U8,
            "Int16" | "short" => ScalarType::I16,
            "UInt16" | "unsigned_short" => ScalarType::U16,
            "Int32" | "int" | "vtkIdType32" | "vtktypeint32" => ScalarType::I32,
            "UInt32" | "unsigned_int" | "vtktypeuint32" => ScalarType::U32,
            "Int64" | "long" | "vtkIdType" | "long_long" | "vtktypeint64" => ScalarType::I64,
            "UInt64" | "unsigned_long" | "unsigned_long_long" | "vtktypeuint64" => ScalarType::U64,
            "vtktypeint8" => ScalarType::I8,
            "vtktypeuint8" => ScalarType::U8,
            "vtktypeint16" => ScalarType::I16,
            "vtktypeuint16" => ScalarType::U16,
            "vtktypefloat32" => ScalarType::F32,
            "vtktypefloat64" => ScalarType::F64,
            "Float16" => ScalarType::F16,
            "Float32" | "float" => ScalarType::F32,
            "Float64" | "double" => ScalarType::F64,
            "String" | "string" => ScalarType::String,
            _ => return None,
        })
    }

    pub fn is_float(self) -> bool {
        matches!(self, ScalarType::F16 | ScalarType::F32 | ScalarType::F64)
    }
}

/// Backing storage for an [`Array`]. Component-interleaved (tuple-major) layout.
#[derive(Clone, PartialEq, Serialize, Deserialize)]
pub enum ArrayData {
    I8(Vec<i8>),
    U8(Vec<u8>),
    I16(Vec<i16>),
    U16(Vec<u16>),
    I32(Vec<i32>),
    U32(Vec<u32>),
    I64(Vec<i64>),
    U64(Vec<u64>),
    F16(Vec<f16>),
    F32(Vec<f32>),
    F64(Vec<f64>),
    String(Vec<String>),
}

macro_rules! dispatch {
    ($self:expr, $v:ident => $body:expr) => {
        match $self {
            ArrayData::I8($v) => $body,
            ArrayData::U8($v) => $body,
            ArrayData::I16($v) => $body,
            ArrayData::U16($v) => $body,
            ArrayData::I32($v) => $body,
            ArrayData::U32($v) => $body,
            ArrayData::I64($v) => $body,
            ArrayData::U64($v) => $body,
            ArrayData::F16($v) => $body,
            ArrayData::F32($v) => $body,
            ArrayData::F64($v) => $body,
            ArrayData::String($v) => $body,
        }
    };
}

macro_rules! dispatch_numeric {
    ($self:expr, $v:ident => $body:expr, $string:expr) => {
        match $self {
            ArrayData::I8($v) => $body,
            ArrayData::U8($v) => $body,
            ArrayData::I16($v) => $body,
            ArrayData::U16($v) => $body,
            ArrayData::I32($v) => $body,
            ArrayData::U32($v) => $body,
            ArrayData::I64($v) => $body,
            ArrayData::U64($v) => $body,
            ArrayData::F16($v) => $body,
            ArrayData::F32($v) => $body,
            ArrayData::F64($v) => $body,
            ArrayData::String(_) => $string,
        }
    };
}

/// Anything that can be losslessly-ish converted to f64 for generic numeric code.
pub trait AsF64: Copy + Send + Sync {
    fn as_f64(self) -> f64;
}
macro_rules! impl_as_f64 { ($($t:ty),*) => { $(impl AsF64 for $t { #[inline] fn as_f64(self) -> f64 { self as f64 } })* } }
impl_as_f64!(i8, u8, i16, u16, i32, u32, i64, u64, f32, f64);
impl AsF64 for f16 {
    #[inline]
    fn as_f64(self) -> f64 {
        self.to_f64()
    }
}

impl ArrayData {
    pub fn scalar_type(&self) -> ScalarType {
        match self {
            ArrayData::I8(_) => ScalarType::I8,
            ArrayData::U8(_) => ScalarType::U8,
            ArrayData::I16(_) => ScalarType::I16,
            ArrayData::U16(_) => ScalarType::U16,
            ArrayData::I32(_) => ScalarType::I32,
            ArrayData::U32(_) => ScalarType::U32,
            ArrayData::I64(_) => ScalarType::I64,
            ArrayData::U64(_) => ScalarType::U64,
            ArrayData::F16(_) => ScalarType::F16,
            ArrayData::F32(_) => ScalarType::F32,
            ArrayData::F64(_) => ScalarType::F64,
            ArrayData::String(_) => ScalarType::String,
        }
    }

    /// Total number of scalar values (tuples * components).
    pub fn len(&self) -> usize {
        dispatch!(self, v => v.len())
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    #[inline]
    pub fn get_f64(&self, i: usize) -> f64 {
        dispatch_numeric!(self, v => v[i].as_f64(), f64::NAN)
    }

    pub fn to_f64_vec(&self) -> Vec<f64> {
        dispatch_numeric!(self, v => v.iter().map(|x| x.as_f64()).collect(), vec![f64::NAN; self.len()])
    }

    pub fn to_f32_vec(&self) -> Vec<f32> {
        match self {
            ArrayData::F32(v) => v.clone(),
            _ => dispatch_numeric!(self, v => v.iter().map(|x| x.as_f64() as f32).collect(), vec![f32::NAN; self.len()]),
        }
    }

    pub fn zeros(ty: ScalarType, len: usize) -> Self {
        match ty {
            ScalarType::I8 => ArrayData::I8(vec![0; len]),
            ScalarType::U8 => ArrayData::U8(vec![0; len]),
            ScalarType::I16 => ArrayData::I16(vec![0; len]),
            ScalarType::U16 => ArrayData::U16(vec![0; len]),
            ScalarType::I32 => ArrayData::I32(vec![0; len]),
            ScalarType::U32 => ArrayData::U32(vec![0; len]),
            ScalarType::I64 => ArrayData::I64(vec![0; len]),
            ScalarType::U64 => ArrayData::U64(vec![0; len]),
            ScalarType::F16 => ArrayData::F16(vec![f16::ZERO; len]),
            ScalarType::F32 => ArrayData::F32(vec![0.0; len]),
            ScalarType::F64 => ArrayData::F64(vec![0.0; len]),
            ScalarType::String => ArrayData::String(vec![String::new(); len]),
        }
    }

    /// Build from a slice of f64 values, converting to `ty`.
    pub fn from_f64(ty: ScalarType, values: &[f64]) -> Self {
        match ty {
            ScalarType::I8 => ArrayData::I8(values.iter().map(|&x| x as i8).collect()),
            ScalarType::U8 => ArrayData::U8(values.iter().map(|&x| x as u8).collect()),
            ScalarType::I16 => ArrayData::I16(values.iter().map(|&x| x as i16).collect()),
            ScalarType::U16 => ArrayData::U16(values.iter().map(|&x| x as u16).collect()),
            ScalarType::I32 => ArrayData::I32(values.iter().map(|&x| x as i32).collect()),
            ScalarType::U32 => ArrayData::U32(values.iter().map(|&x| x as u32).collect()),
            ScalarType::I64 => ArrayData::I64(values.iter().map(|&x| x as i64).collect()),
            ScalarType::U64 => ArrayData::U64(values.iter().map(|&x| x as u64).collect()),
            ScalarType::F16 => ArrayData::F16(values.iter().map(|&x| f16::from_f64(x)).collect()),
            ScalarType::F32 => ArrayData::F32(values.iter().map(|&x| x as f32).collect()),
            ScalarType::F64 => ArrayData::F64(values.to_vec()),
            ScalarType::String => ArrayData::String(values.iter().map(|x| x.to_string()).collect()),
        }
    }

    /// Decode from little- or big-endian bytes.
    pub fn from_bytes(ty: ScalarType, bytes: &[u8], little_endian: bool) -> anyhow::Result<Self> {
        let sz = ty.size_of();
        if sz == 0 {
            anyhow::bail!("cannot decode string arrays from raw bytes");
        }
        if bytes.len() % sz != 0 {
            anyhow::bail!("byte length {} is not a multiple of element size {} for {:?}", bytes.len(), sz, ty);
        }
        macro_rules! conv {
            ($t:ty, $variant:ident) => {{
                let v: Vec<$t> = bytes
                    .chunks_exact(sz)
                    .map(|c| {
                        let a: [u8; std::mem::size_of::<$t>()] = c.try_into().unwrap();
                        if little_endian {
                            <$t>::from_le_bytes(a)
                        } else {
                            <$t>::from_be_bytes(a)
                        }
                    })
                    .collect();
                ArrayData::$variant(v)
            }};
        }
        Ok(match ty {
            ScalarType::I8 => conv!(i8, I8),
            ScalarType::U8 => ArrayData::U8(bytes.to_vec()),
            ScalarType::I16 => conv!(i16, I16),
            ScalarType::U16 => conv!(u16, U16),
            ScalarType::I32 => conv!(i32, I32),
            ScalarType::U32 => conv!(u32, U32),
            ScalarType::I64 => conv!(i64, I64),
            ScalarType::U64 => conv!(u64, U64),
            ScalarType::F16 => {
                let v: Vec<f16> = bytes
                    .chunks_exact(2)
                    .map(|c| {
                        let a = [c[0], c[1]];
                        f16::from_bits(if little_endian { u16::from_le_bytes(a) } else { u16::from_be_bytes(a) })
                    })
                    .collect();
                ArrayData::F16(v)
            }
            ScalarType::F32 => conv!(f32, F32),
            ScalarType::F64 => conv!(f64, F64),
            ScalarType::String => unreachable!(),
        })
    }

    /// Gather tuples by index (each tuple has `ncomp` values).
    pub fn gather(&self, ncomp: usize, idx: &[usize]) -> Self {
        dispatch!(self, v => {
            let mut out = Vec::with_capacity(idx.len() * ncomp);
            for &i in idx {
                out.extend_from_slice(&v[i * ncomp..(i + 1) * ncomp]);
            }
            Self::wrap(out)
        })
    }

    pub fn append(&mut self, other: &ArrayData) -> anyhow::Result<()> {
        macro_rules! app { ($($var:ident),*) => {
            match (self, other) {
                $( (ArrayData::$var(a), ArrayData::$var(b)) => { a.extend_from_slice(b); Ok(()) } )*
                (a, b) => anyhow::bail!("cannot append {:?} to {:?}", b.scalar_type(), a.scalar_type()),
            }
        }}
        app!(I8, U8, I16, U16, I32, U32, I64, U64, F16, F32, F64, String)
    }

    fn wrap<T: ArrayElement>(v: Vec<T>) -> Self {
        T::wrap(v)
    }
}

/// Internal helper trait mapping Rust element types to `ArrayData` variants.
pub trait ArrayElement: Clone + Send + Sync + 'static {
    fn wrap(v: Vec<Self>) -> ArrayData;
    fn unwrap_ref(a: &ArrayData) -> Option<&Vec<Self>>;
    const SCALAR_TYPE: ScalarType;
}
macro_rules! impl_elem { ($($t:ty => $var:ident),*) => { $(
    impl ArrayElement for $t {
        fn wrap(v: Vec<Self>) -> ArrayData { ArrayData::$var(v) }
        fn unwrap_ref(a: &ArrayData) -> Option<&Vec<Self>> { if let ArrayData::$var(v) = a { Some(v) } else { None } }
        const SCALAR_TYPE: ScalarType = ScalarType::$var;
    }
)* } }
impl_elem!(i8 => I8, u8 => U8, i16 => I16, u16 => U16, i32 => I32, u32 => U32, i64 => I64, u64 => U64, f16 => F16, f32 => F32, f64 => F64, String => String);

impl fmt::Debug for ArrayData {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}[{}]", self.scalar_type(), self.len())
    }
}

/// A named, multi-component array (e.g. "velocity" with 3 components).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Array {
    pub name: String,
    pub ncomp: usize,
    pub data: ArrayData,
    /// Optional per-component names (e.g. ["X","Y","Z"]).
    pub component_names: Option<Vec<String>>,
}

impl Array {
    pub fn new(name: impl Into<String>, ncomp: usize, data: ArrayData) -> Self {
        Self { name: name.into(), ncomp: ncomp.max(1), data, component_names: None }
    }

    pub fn from_vec<T: ArrayElement>(name: impl Into<String>, ncomp: usize, v: Vec<T>) -> Self {
        Self::new(name, ncomp, T::wrap(v))
    }

    pub fn scalars_f64(name: impl Into<String>, v: Vec<f64>) -> Self {
        Self::new(name, 1, ArrayData::F64(v))
    }

    pub fn scalars_f32(name: impl Into<String>, v: Vec<f32>) -> Self {
        Self::new(name, 1, ArrayData::F32(v))
    }

    pub fn scalar_type(&self) -> ScalarType {
        self.data.scalar_type()
    }

    /// Number of tuples.
    pub fn len(&self) -> usize {
        self.data.len() / self.ncomp
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    #[inline]
    pub fn get(&self, tuple: usize, comp: usize) -> f64 {
        self.data.get_f64(tuple * self.ncomp + comp)
    }

    pub fn tuple(&self, i: usize) -> Vec<f64> {
        (0..self.ncomp).map(|c| self.get(i, c)).collect()
    }

    pub fn as_slice<T: ArrayElement>(&self) -> Option<&[T]> {
        T::unwrap_ref(&self.data).map(|v| v.as_slice())
    }

    /// Magnitude of tuple `i` (for ncomp==1 this is |value|).
    #[inline]
    pub fn magnitude(&self, i: usize) -> f64 {
        if self.ncomp == 1 {
            return self.get(i, 0);
        }
        let mut s = 0.0;
        for c in 0..self.ncomp {
            let v = self.get(i, c);
            s += v * v;
        }
        s.sqrt()
    }

    /// Range of a component, or of the magnitude when `comp` is `None`. Ignores NaN.
    pub fn range(&self, comp: Option<usize>) -> (f64, f64) {
        let n = self.len();
        if n == 0 || self.scalar_type() == ScalarType::String {
            return (0.0, 1.0);
        }
        let (lo, hi) = (0..n)
            .into_par_iter()
            .map(|i| match comp {
                Some(c) => self.get(i, c),
                None => self.magnitude(i),
            })
            .filter(|v| v.is_finite())
            .fold(|| (f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), v| (lo.min(v), hi.max(v)))
            .reduce(|| (f64::INFINITY, f64::NEG_INFINITY), |a, b| (a.0.min(b.0), a.1.max(b.1)));
        if lo > hi {
            (0.0, 1.0)
        } else {
            (lo, hi)
        }
    }

    /// Component `comp` (or magnitude if `None`) of every tuple as f32, for rendering.
    pub fn component_f32(&self, comp: Option<usize>) -> Vec<f32> {
        let n = self.len();
        match (comp, &self.data, self.ncomp) {
            (_, ArrayData::F32(v), 1) => v.clone(),
            (Some(c), _, _) => (0..n).map(|i| self.get(i, c) as f32).collect(),
            (None, _, 1) => self.data.to_f32_vec(),
            (None, _, _) => (0..n).map(|i| self.magnitude(i) as f32).collect(),
        }
    }

    pub fn gather(&self, idx: &[usize]) -> Array {
        Array { name: self.name.clone(), ncomp: self.ncomp, data: self.data.gather(self.ncomp, idx), component_names: self.component_names.clone() }
    }

    pub fn n_bytes(&self) -> usize {
        self.data.len() * self.scalar_type().size_of()
    }
}
