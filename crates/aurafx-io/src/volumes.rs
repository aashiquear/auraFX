//! Medical and microscopy volumes: NRRD (`.nrrd`, `.nhdr`), MetaImage (`.mhd`, `.mha`; read
//! and written), MRC (`.mrc`, `.map`, `.rec`, `.st`), DICOM (`.dcm` files, a folder of a series)
//! and OME-TIFF (`.ome.tif`). Each becomes image data with the values as point data, at the
//! file's spacing, origin and orientation.

use anyhow::{anyhow, bail, Context, Result};
use aurafx_data::*;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

// ------------------------------------------------------------------ shared

#[derive(Debug, Clone, Copy, PartialEq)]
enum Ty {
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
}

impl Ty {
    fn size(self) -> usize {
        match self {
            Ty::I8 | Ty::U8 => 1,
            Ty::I16 | Ty::U16 | Ty::F16 => 2,
            Ty::I32 | Ty::U32 | Ty::F32 => 4,
            Ty::I64 | Ty::U64 | Ty::F64 => 8,
        }
    }
}

fn f16_to_f32(h: u16) -> f32 {
    let s = ((h >> 15) & 1) as u32;
    let e = ((h >> 10) & 0x1f) as i32;
    let m = (h & 0x3ff) as u32;
    let bits = if e == 0 {
        if m == 0 {
            s << 31
        } else {
            // Subnormal (m / 1024 * 2^-14): shift the leading one up to bit 10.
            let mut shift = 0i32;
            let mut m2 = m;
            while m2 & 0x400 == 0 {
                m2 <<= 1;
                shift += 1;
            }
            (s << 31) | (((127 - 14 - shift) as u32) << 23) | ((m2 & 0x3ff) << 13)
        }
    } else if e == 31 {
        (s << 31) | 0x7f80_0000 | (m << 13)
    } else {
        (s << 31) | (((e - 15 + 127) as u32) << 23) | (m << 13)
    };
    f32::from_bits(bits)
}

/// Decode `n` values of type `ty` from bytes.
fn decode(bytes: &[u8], ty: Ty, n: usize, little: bool) -> Result<ArrayData> {
    let need = n * ty.size();
    if bytes.len() < need {
        bail!("the file holds {} bytes of data, {} are needed", bytes.len(), need);
    }
    let b = &bytes[..need];
    macro_rules! conv {
        ($t:ty, $sz:expr, $var:ident) => {
            ArrayData::$var(
                b.chunks_exact($sz)
                    .map(|c| {
                        let a: [u8; $sz] = c.try_into().unwrap();
                        if little {
                            <$t>::from_le_bytes(a)
                        } else {
                            <$t>::from_be_bytes(a)
                        }
                    })
                    .collect(),
            )
        };
    }
    Ok(match ty {
        Ty::I8 => ArrayData::I8(b.iter().map(|&x| x as i8).collect()),
        Ty::U8 => ArrayData::U8(b.to_vec()),
        Ty::I16 => conv!(i16, 2, I16),
        Ty::U16 => conv!(u16, 2, U16),
        Ty::I32 => conv!(i32, 4, I32),
        Ty::U32 => conv!(u32, 4, U32),
        Ty::I64 => conv!(i64, 8, I64),
        Ty::U64 => conv!(u64, 8, U64),
        Ty::F32 => conv!(f32, 4, F32),
        Ty::F64 => conv!(f64, 8, F64),
        Ty::F16 => ArrayData::F32(
            b.chunks_exact(2)
                .map(|c| {
                    let a = [c[0], c[1]];
                    f16_to_f32(if little { u16::from_le_bytes(a) } else { u16::from_be_bytes(a) })
                })
                .collect(),
        ),
    })
}

fn inflate(bytes: &[u8], gzip: bool) -> Result<Vec<u8>> {
    use std::io::Read;
    let mut out = Vec::new();
    if gzip {
        flate2::read::MultiGzDecoder::new(bytes).read_to_end(&mut out)?;
    } else {
        // zlib, or raw deflate when there is no zlib header.
        if flate2::read::ZlibDecoder::new(bytes).read_to_end(&mut out).is_err() {
            out.clear();
            flate2::read::DeflateDecoder::new(bytes).read_to_end(&mut out)?;
        }
    }
    Ok(out)
}

/// Unit vectors of a direction matrix (columns: the grid's i, j, k axes in space).
fn direction_from_axes(axes: [[f64; 3]; 3]) -> [f64; 9] {
    let mut d = [0.0; 9];
    for (c, ax) in axes.iter().enumerate() {
        let l = (ax[0] * ax[0] + ax[1] * ax[1] + ax[2] * ax[2]).sqrt();
        for r in 0..3 {
            d[r * 3 + c] = if l > 0.0 {
                ax[r] / l
            } else if r == c {
                1.0
            } else {
                0.0
            };
        }
    }
    d
}

fn image(dims: [usize; 3], spacing: [f64; 3], origin: [f64; 3], direction: [f64; 9]) -> ImageData {
    ImageData {
        extent: [0, dims[0] as i64 - 1, 0, dims[1] as i64 - 1, 0, dims[2] as i64 - 1],
        origin,
        spacing,
        direction,
        point_data: Attributes::new(),
        cell_data: Attributes::new(),
        field_data: Attributes::new(),
    }
}

const IDENTITY: [f64; 9] = [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0];

/// When the grid's axes are the space axes swapped or reversed (a direction matrix of 0 and +-1),
/// reorder the values so that the grid runs along +x, +y, +z with the identity direction: the
/// points land where the file puts them. Oblique grids keep their direction matrix.
pub(crate) fn align_axes(im: &mut ImageData) {
    let d = im.direction;
    // Column k (grid axis k) is +-e_a for some space axis a.
    let mut map = [(0usize, 1.0f64); 3];
    let mut used = [false; 3];
    for k in 0..3 {
        let col = [d[k], d[3 + k], d[6 + k]];
        let Some(a) = (0..3).find(|&a| (col[a].abs() - 1.0).abs() < 1e-9 && (0..3).all(|b| b == a || col[b].abs() < 1e-9)) else { return };
        if used[a] {
            return;
        }
        used[a] = true;
        map[k] = (a, col[a].signum());
    }
    if map.iter().enumerate().all(|(k, m)| m.0 == k && m.1 > 0.0) {
        return;
    }
    let old = im.dims();
    let mut nd = [1usize; 3];
    let mut sp = [1.0; 3];
    let mut origin = im.origin;
    for k in 0..3 {
        let (a, sign) = map[k];
        nd[a] = old[k];
        sp[a] = im.spacing[k];
        if sign < 0.0 {
            // The grid runs down axis a: the first point is at the far end.
            origin[a] = im.origin[a] - (old[k] as f64 - 1.0) * im.spacing[k];
        }
    }
    let n = old[0] * old[1] * old[2];
    let mut idx = vec![0usize; n];
    for kk in 0..old[2] {
        for jj in 0..old[1] {
            for ii in 0..old[0] {
                let o = [ii, jj, kk];
                let mut p = [0usize; 3];
                for k in 0..3 {
                    let (a, sign) = map[k];
                    p[a] = if sign < 0.0 { old[k] - 1 - o[k] } else { o[k] };
                }
                idx[p[0] + nd[0] * (p[1] + nd[1] * p[2])] = ii + old[0] * (jj + old[1] * kk);
            }
        }
    }
    let names: Vec<String> = im.point_data.iter().map(|a| a.name.clone()).collect();
    for name in names {
        let g = im.point_data.get(&name).unwrap().gather(&idx);
        im.point_data.insert(g);
    }
    im.extent = [0, nd[0] as i64 - 1, 0, nd[1] as i64 - 1, 0, nd[2] as i64 - 1];
    im.spacing = sp;
    im.origin = origin;
    im.direction = IDENTITY;
}

// ------------------------------------------------------------------ NRRD

fn nrrd_type(s: &str) -> Option<Ty> {
    Some(match s.trim() {
        "signed char" | "int8" | "int8_t" => Ty::I8,
        "uchar" | "unsigned char" | "uint8" | "uint8_t" => Ty::U8,
        "short" | "short int" | "signed short" | "signed short int" | "int16" | "int16_t" => Ty::I16,
        "ushort" | "unsigned short" | "unsigned short int" | "uint16" | "uint16_t" => Ty::U16,
        "int" | "signed int" | "int32" | "int32_t" => Ty::I32,
        "uint" | "unsigned int" | "uint32" | "uint32_t" => Ty::U32,
        "longlong" | "long long" | "long long int" | "signed long long" | "signed long long int" | "int64" | "int64_t" => Ty::I64,
        "ulonglong" | "unsigned long long" | "unsigned long long int" | "uint64" | "uint64_t" => Ty::U64,
        "float" => Ty::F32,
        "double" => Ty::F64,
        _ => return None,
    })
}

fn parse_vec(s: &str) -> Option<Vec<f64>> {
    let t = s.trim();
    if t == "none" {
        return None;
    }
    let inner = t.trim_start_matches('(').trim_end_matches(')');
    inner.split(',').map(|v| v.trim().parse::<f64>().ok()).collect()
}

/// NRRD: a text header (`.nrrd` with the data after it, or a detached `.nhdr`), raw, gzip or
/// ASCII data. A non-spatial first axis (vector, RGB-color, ...) gives components.
pub fn read_nrrd(path: &Path) -> Result<DataSet> {
    let bytes = std::fs::read(path).with_context(|| format!("reading {}", path.display()))?;
    if !bytes.starts_with(b"NRRD000") {
        bail!("{} is not a NRRD file", path.display());
    }
    // Header: lines up to an empty line.
    let mut fields: BTreeMap<String, String> = BTreeMap::new();
    let mut pos = 0;
    let mut first = true;
    loop {
        let end = bytes[pos..].iter().position(|&b| b == b'\n').map(|e| pos + e).unwrap_or(bytes.len());
        let line = String::from_utf8_lossy(&bytes[pos..end]).trim_end_matches('\r').to_string();
        pos = (end + 1).min(bytes.len());
        if first {
            first = false;
            continue;
        }
        if line.is_empty() || end >= bytes.len() {
            break;
        }
        if line.starts_with('#') {
            continue;
        }
        if let Some((k, v)) = line.split_once(": ") {
            fields.insert(k.trim().to_ascii_lowercase(), v.trim().to_string());
        } else if let Some((k, v)) = line.split_once(":=") {
            fields.insert(format!("key {}", k.trim()), v.trim().to_string());
        }
    }
    let get = |k: &str| fields.get(k).cloned();
    let ty = nrrd_type(&get("type").ok_or_else(|| anyhow!("NRRD: no type"))?).ok_or_else(|| anyhow!("NRRD: unknown type {:?}", get("type")))?;
    let ndim: usize = get("dimension").and_then(|d| d.parse().ok()).ok_or_else(|| anyhow!("NRRD: no dimension"))?;
    let sizes: Vec<usize> = get("sizes").ok_or_else(|| anyhow!("NRRD: no sizes"))?.split_whitespace().map(|s| s.parse().unwrap_or(1)).collect();
    if sizes.len() != ndim {
        bail!("NRRD: {} sizes for dimension {}", sizes.len(), ndim);
    }
    let kinds: Vec<String> = get("kinds").map(|k| k.split_whitespace().map(|s| s.to_ascii_lowercase()).collect()).unwrap_or_default();
    let spatial = |i: usize| kinds.get(i).map(|k| matches!(k.as_str(), "domain" | "space" | "time" | "???" | "none")).unwrap_or(true);
    // Components on the first axis when it is not spatial (or a 4-D file).
    let comp_axis = ndim >= 2 && (!spatial(0) || (ndim == 4 && kinds.is_empty()));
    let ncomp = if comp_axis { sizes[0] } else { 1 };
    let sp_axes: Vec<usize> = (if comp_axis { 1 } else { 0 }..ndim).collect();
    if sp_axes.len() > 3 {
        bail!("NRRD: {} spatial axes (at most 3)", sp_axes.len());
    }
    let mut dims = [1usize; 3];
    for (k, &a) in sp_axes.iter().enumerate() {
        dims[k] = sizes[a];
    }
    let mut spacing = [1.0; 3];
    let mut origin = [0.0; 3];
    let mut direction = IDENTITY;
    if let Some(sd) = get("space directions") {
        let parts: Vec<&str> = sd.split_whitespace().collect();
        let vecs: Vec<Option<Vec<f64>>> = parts.iter().map(|p| parse_vec(p)).collect();
        let sp_vecs: Vec<Vec<f64>> = vecs.into_iter().flatten().collect();
        let mut axes = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];
        for (k, v) in sp_vecs.iter().take(3).enumerate() {
            let mut a = [0.0; 3];
            for (c, x) in v.iter().take(3).enumerate() {
                a[c] = *x;
            }
            spacing[k] = (a[0] * a[0] + a[1] * a[1] + a[2] * a[2]).sqrt();
            axes[k] = a;
        }
        direction = direction_from_axes(axes);
    } else if let Some(sp) = get("spacings") {
        let v: Vec<f64> = sp.split_whitespace().map(|s| s.parse().unwrap_or(f64::NAN)).collect();
        for (k, &a) in sp_axes.iter().enumerate() {
            if let Some(x) = v.get(a).filter(|x| x.is_finite()) {
                spacing[k] = *x;
            }
        }
    }
    if let Some(o) = get("space origin").and_then(|o| parse_vec(&o)) {
        for (k, x) in o.iter().take(3).enumerate() {
            origin[k] = *x;
        }
    } else if let Some(o) = get("axis mins") {
        let v: Vec<f64> = o.split_whitespace().map(|s| s.parse().unwrap_or(0.0)).collect();
        for (k, &a) in sp_axes.iter().enumerate() {
            origin[k] = v.get(a).copied().filter(|x| x.is_finite()).unwrap_or(0.0);
        }
    }
    let n = sizes.iter().product::<usize>();
    let encoding = get("encoding").unwrap_or_else(|| "raw".into()).to_ascii_lowercase();
    let little = get("endian").map(|e| e.to_ascii_lowercase() != "big").unwrap_or(true);
    let raw: Vec<u8> = match get("data file").or_else(|| get("datafile")) {
        Some(f) => {
            let p = crate::sibling(path, f.split_whitespace().next().unwrap_or(&f));
            std::fs::read(&p).with_context(|| format!("reading the NRRD data file {}", p.display()))?
        }
        None => bytes[pos..].to_vec(),
    };
    let skip: usize = get("byte skip").and_then(|s| s.parse::<i64>().ok()).unwrap_or(0).max(0) as usize;
    let data = match encoding.as_str() {
        "raw" => decode(&raw[skip.min(raw.len())..], ty, n, little)?,
        "gzip" | "gz" => decode(&inflate(&raw, true)?[skip..], ty, n, little)?,
        "ascii" | "text" | "txt" => {
            let v: Vec<f64> = String::from_utf8_lossy(&raw).split_whitespace().take(n).map(|s| s.parse().unwrap_or(f64::NAN)).collect();
            if v.len() < n {
                bail!("NRRD: {} values of {}", v.len(), n);
            }
            ArrayData::F64(v)
        }
        other => bail!("NRRD encoding {other:?} is not supported (raw, gzip and ascii are)"),
    };
    let mut im = image(dims, spacing, origin, direction);
    im.point_data.insert_active(AttributeKind::Scalars, Array::new("NRRDImage", ncomp, data));
    align_axes(&mut im);
    Ok(DataSet::ImageData(im))
}

// ------------------------------------------------------------------ MetaImage

fn met_type(s: &str) -> Option<Ty> {
    Some(match s.trim() {
        "MET_CHAR" => Ty::I8,
        "MET_UCHAR" => Ty::U8,
        "MET_SHORT" => Ty::I16,
        "MET_USHORT" => Ty::U16,
        "MET_INT" | "MET_LONG" => Ty::I32,
        "MET_UINT" | "MET_ULONG" => Ty::U32,
        "MET_LONG_LONG" => Ty::I64,
        "MET_ULONG_LONG" => Ty::U64,
        "MET_FLOAT" => Ty::F32,
        "MET_DOUBLE" => Ty::F64,
        _ => return None,
    })
}

/// MetaImage: a `key = value` header (`.mhd` with a separate data file, or `.mha` with the data
/// after `ElementDataFile = LOCAL`), raw or zlib data.
pub fn read_metaimage(path: &Path) -> Result<DataSet> {
    let bytes = std::fs::read(path).with_context(|| format!("reading {}", path.display()))?;
    let mut fields: BTreeMap<String, String> = BTreeMap::new();
    let mut pos = 0;
    while pos < bytes.len() {
        let end = bytes[pos..].iter().position(|&b| b == b'\n').map(|e| pos + e).unwrap_or(bytes.len());
        let line = String::from_utf8_lossy(&bytes[pos..end]).trim_end_matches('\r').to_string();
        pos = (end + 1).min(bytes.len());
        if let Some((k, v)) = line.split_once('=') {
            let k = k.trim().to_string();
            fields.insert(k.to_ascii_lowercase(), v.trim().to_string());
            if k.eq_ignore_ascii_case("ElementDataFile") {
                break;
            }
        }
    }
    let get = |k: &str| fields.get(&k.to_ascii_lowercase()).cloned();
    let nums = |k: &str| -> Option<Vec<f64>> { get(k).map(|v| v.split_whitespace().filter_map(|x| x.parse().ok()).collect()) };
    let ndims: usize = get("NDims").and_then(|v| v.parse().ok()).ok_or_else(|| anyhow!("MetaImage: no NDims"))?;
    let size: Vec<usize> = nums("DimSize").ok_or_else(|| anyhow!("MetaImage: no DimSize"))?.iter().map(|&x| x as usize).collect();
    if ndims > 3 || size.len() < ndims {
        bail!("MetaImage: {ndims} dimensions (at most 3 are read)");
    }
    let ty = met_type(&get("ElementType").unwrap_or_default()).ok_or_else(|| anyhow!("MetaImage: unknown ElementType {:?}", get("ElementType")))?;
    let ncomp: usize = get("ElementNumberOfChannels").and_then(|v| v.parse().ok()).unwrap_or(1);
    let mut dims = [1usize; 3];
    let mut spacing = [1.0; 3];
    let mut origin = [0.0; 3];
    for k in 0..ndims {
        dims[k] = size[k];
    }
    if let Some(s) = nums("ElementSpacing").or_else(|| nums("ElementSize")) {
        for k in 0..ndims.min(s.len()) {
            spacing[k] = s[k];
        }
    }
    if let Some(o) = nums("Offset").or_else(|| nums("Origin")).or_else(|| nums("Position")) {
        for k in 0..ndims.min(o.len()) {
            origin[k] = o[k];
        }
    }
    let mut direction = IDENTITY;
    if let Some(m) = nums("TransformMatrix").or_else(|| nums("Rotation")).or_else(|| nums("Orientation")) {
        if m.len() == ndims * ndims && ndims == 3 {
            // Stored row by row, one axis per row.
            direction = direction_from_axes([[m[0], m[1], m[2]], [m[3], m[4], m[5]], [m[6], m[7], m[8]]]);
        }
    }
    let msb = get("BinaryDataByteOrderMSB").or_else(|| get("ElementByteOrderMSB")).map(|v| v.eq_ignore_ascii_case("true")).unwrap_or(false);
    let compressed = get("CompressedData").map(|v| v.eq_ignore_ascii_case("true")).unwrap_or(false);
    let n = dims.iter().product::<usize>() * ncomp;
    let file = get("ElementDataFile").ok_or_else(|| anyhow!("MetaImage: no ElementDataFile"))?;
    let raw = if file.eq_ignore_ascii_case("LOCAL") {
        bytes[pos..].to_vec()
    } else if file.to_ascii_uppercase().starts_with("LIST") {
        bail!("MetaImage: a LIST of slice files is not supported");
    } else {
        let p = crate::sibling(path, &file);
        std::fs::read(&p).with_context(|| format!("reading the MetaImage data file {}", p.display()))?
    };
    let raw = if compressed { inflate(&raw, false)? } else { raw };
    let header_size: i64 = get("HeaderSize").and_then(|v| v.parse().ok()).unwrap_or(0);
    let start = if header_size > 0 {
        header_size as usize
    } else if header_size == -1 {
        raw.len().saturating_sub(n * ty.size())
    } else {
        0
    };
    let data = decode(&raw[start.min(raw.len())..], ty, n, !msb)?;
    let mut im = image(dims, spacing, origin, direction);
    im.point_data.insert_active(AttributeKind::Scalars, Array::new("MetaImage", ncomp, data));
    align_axes(&mut im);
    Ok(DataSet::ImageData(im))
}

/// Write image data as MetaImage: `.mha` (one file) or `.mhd` (with a `.raw` beside it). The
/// active (else first) point array, or cell array for data without point arrays.
pub fn write_metaimage(ds: &DataSet, path: &Path, compress: bool) -> Result<()> {
    let DataSet::ImageData(im) = ds else { bail!("MetaImage files hold image data (a uniform grid); resample first") };
    let (arr, cells) = match im.point_data.scalars().or_else(|| im.point_data.iter().find(|a| a.scalar_type() != ScalarType::String)) {
        Some(a) => (a, false),
        None => (
            im.cell_data
                .scalars()
                .or_else(|| im.cell_data.iter().find(|a| a.scalar_type() != ScalarType::String))
                .ok_or_else(|| anyhow!("no array to write"))?,
            true,
        ),
    };
    let mut dims = im.dims();
    if cells {
        dims = dims.map(|d| (d - 1).max(1));
    }
    let (ty, bytes): (&str, Vec<u8>) = match &arr.data {
        ArrayData::I8(v) => ("MET_CHAR", v.iter().map(|&x| x as u8).collect()),
        ArrayData::U8(v) => ("MET_UCHAR", v.clone()),
        ArrayData::I16(v) => ("MET_SHORT", v.iter().flat_map(|x| x.to_le_bytes()).collect()),
        ArrayData::U16(v) => ("MET_USHORT", v.iter().flat_map(|x| x.to_le_bytes()).collect()),
        ArrayData::I32(v) => ("MET_INT", v.iter().flat_map(|x| x.to_le_bytes()).collect()),
        ArrayData::U32(v) => ("MET_UINT", v.iter().flat_map(|x| x.to_le_bytes()).collect()),
        ArrayData::I64(v) => ("MET_LONG_LONG", v.iter().flat_map(|x| x.to_le_bytes()).collect()),
        ArrayData::U64(v) => ("MET_ULONG_LONG", v.iter().flat_map(|x| x.to_le_bytes()).collect()),
        ArrayData::F32(v) => ("MET_FLOAT", v.iter().flat_map(|x| x.to_le_bytes()).collect()),
        other => ("MET_DOUBLE", other.to_f64_vec().iter().flat_map(|x| x.to_le_bytes()).collect()),
    };
    let data = if compress {
        use std::io::Write;
        let mut e = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
        e.write_all(&bytes)?;
        e.finish()?
    } else {
        bytes
    };
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase();
    let local = ext != "mhd";
    let raw_path = path.with_extension(if compress { "zraw" } else { "raw" });
    let origin = [
        im.origin[0] + im.extent[0] as f64 * im.spacing[0] + if cells { 0.5 * im.spacing[0] } else { 0.0 },
        im.origin[1] + im.extent[2] as f64 * im.spacing[1] + if cells { 0.5 * im.spacing[1] } else { 0.0 },
        im.origin[2] + im.extent[4] as f64 * im.spacing[2] + if cells { 0.5 * im.spacing[2] } else { 0.0 },
    ];
    let d = im.direction;
    let mut h = String::new();
    h += "ObjectType = Image\nNDims = 3\n";
    h += &format!("BinaryData = True\nBinaryDataByteOrderMSB = False\nCompressedData = {}\n", if compress { "True" } else { "False" });
    if compress {
        h += &format!("CompressedDataSize = {}\n", data.len());
    }
    h += &format!("TransformMatrix = {} {} {} {} {} {} {} {} {}\n", d[0], d[3], d[6], d[1], d[4], d[7], d[2], d[5], d[8]);
    h += &format!("Offset = {} {} {}\n", origin[0], origin[1], origin[2]);
    h += &format!("ElementSpacing = {} {} {}\n", im.spacing[0], im.spacing[1], im.spacing[2]);
    h += &format!("DimSize = {} {} {}\n", dims[0], dims[1], dims[2]);
    if arr.ncomp > 1 {
        h += &format!("ElementNumberOfChannels = {}\n", arr.ncomp);
    }
    h += &format!("ElementType = {ty}\n");
    if local {
        h += "ElementDataFile = LOCAL\n";
        let mut out = h.into_bytes();
        out.extend_from_slice(&data);
        std::fs::write(path, out).with_context(|| format!("writing {}", path.display()))?;
    } else {
        h += &format!("ElementDataFile = {}\n", raw_path.file_name().unwrap().to_string_lossy());
        std::fs::write(path, h).with_context(|| format!("writing {}", path.display()))?;
        std::fs::write(&raw_path, data).with_context(|| format!("writing {}", raw_path.display()))?;
    }
    Ok(())
}

// ------------------------------------------------------------------ MRC

/// MRC / CCP4 maps (cryo-EM, tomography): a 1024-byte header, an extended header, then the
/// sections. Modes 0 (int8), 1 (int16), 2 (float32), 6 (uint16) and 12 (float16).
pub fn read_mrc(path: &Path) -> Result<DataSet> {
    let b = std::fs::read(path).with_context(|| format!("reading {}", path.display()))?;
    if b.len() < 1024 {
        bail!("{} is too short for an MRC file", path.display());
    }
    // Byte order: machine stamp at word 53, else guess from the mode.
    let little = match b.get(212) {
        Some(0x44) | Some(0x41) => true,
        Some(0x11) => false,
        _ => {
            let m = i32::from_le_bytes(b[12..16].try_into().unwrap());
            (0..=12).contains(&m) || m == 101
        }
    };
    let i = |w: usize| -> i32 {
        let a: [u8; 4] = b[w * 4..w * 4 + 4].try_into().unwrap();
        if little {
            i32::from_le_bytes(a)
        } else {
            i32::from_be_bytes(a)
        }
    };
    let f = |w: usize| -> f32 {
        let a: [u8; 4] = b[w * 4..w * 4 + 4].try_into().unwrap();
        if little {
            f32::from_le_bytes(a)
        } else {
            f32::from_be_bytes(a)
        }
    };
    let (nx, ny, nz, mode) = (i(0).max(0) as usize, i(1).max(0) as usize, i(2).max(0) as usize, i(3));
    let start = [i(4), i(5), i(6)];
    let m = [i(7).max(1), i(8).max(1), i(9).max(1)];
    let cell = [f(10), f(11), f(12)];
    let map = [i(16), i(17), i(18)];
    let nsymbt = i(23).max(0) as usize;
    let origin_words = [f(49), f(50), f(51)];
    let ty = match mode {
        0 => Ty::I8,
        1 => Ty::I16,
        2 => Ty::F32,
        6 => Ty::U16,
        12 => Ty::F16,
        3 | 4 => bail!("MRC mode {mode} (complex values) is not supported"),
        other => bail!("MRC mode {other} is not supported"),
    };
    let n = nx * ny * nz;
    let data = decode(&b[1024 + nsymbt.min(b.len() - 1024)..], ty, n, little)?;
    // Columns, rows, sections run along the axes map[0], map[1], map[2] (1 = x).
    let axis = |k: usize| (map[k] - 1).clamp(0, 2) as usize;
    let file_dims = [nx, ny, nz];
    let mut dims = [1usize; 3];
    for k in 0..3 {
        dims[axis(k)] = file_dims[k];
    }
    let mut spacing = [1.0; 3];
    for a in 0..3 {
        spacing[a] = if cell[a] > 0.0 { cell[a] as f64 / m[a] as f64 } else { 1.0 };
    }
    let mut origin = [0.0; 3];
    for k in 0..3 {
        let a = axis(k);
        origin[a] = start[k] as f64 * spacing[a];
    }
    if origin_words.iter().any(|v| *v != 0.0 && v.is_finite()) {
        origin = origin_words.map(|v| v as f64);
    }
    let data = if (0..3).all(|k| axis(k) == k) {
        data
    } else {
        // Reorder into x-fastest.
        let src = data.to_f64_vec();
        let mut out = vec![0.0; n];
        for s in 0..nz {
            for r in 0..ny {
                for c in 0..nx {
                    let mut p = [0usize; 3];
                    p[axis(0)] = c;
                    p[axis(1)] = r;
                    p[axis(2)] = s;
                    out[p[0] + dims[0] * (p[1] + dims[1] * p[2])] = src[c + nx * (r + ny * s)];
                }
            }
        }
        ArrayData::F64(out)
    };
    let mut im = image(dims, spacing, origin, IDENTITY);
    im.point_data.insert_active(AttributeKind::Scalars, Array::new("Scalars", 1, data));
    im.field_data.insert(Array::scalars_f64("MRCMode", vec![mode as f64]));
    Ok(DataSet::ImageData(im))
}

// ------------------------------------------------------------------ DICOM

/// The elements of one DICOM file that a volume needs.
#[derive(Debug, Default, Clone)]
struct Dicom {
    rows: usize,
    cols: usize,
    frames: usize,
    samples: usize,
    bits: usize,
    signed: bool,
    slope: f64,
    intercept: f64,
    spacing: [f64; 2],
    thickness: f64,
    position: Option<[f64; 3]>,
    orientation: Option<[f64; 6]>,
    instance: i64,
    series: String,
    pixel: Vec<u8>,
    little: bool,
}

/// Is this a DICOM file (the "DICM" mark after the 128-byte preamble)?
pub fn is_dicom(path: &Path) -> bool {
    use std::io::Read;
    let mut h = [0u8; 132];
    std::fs::File::open(path).and_then(|mut f| f.read_exact(&mut h)).is_ok() && &h[128..132] == b"DICM"
}

/// Does this folder hold DICOM files?
pub fn dir_has_dicom(dir: &Path) -> bool {
    std::fs::read_dir(dir).map(|r| r.filter_map(|e| e.ok()).take(200).any(|e| e.path().is_file() && is_dicom(&e.path()))).unwrap_or(false)
}

fn parse_dicom(path: &Path) -> Result<Dicom> {
    let b = std::fs::read(path).with_context(|| format!("reading {}", path.display()))?;
    if b.len() < 132 || &b[128..132] != b"DICM" {
        bail!("{} is not a DICOM file", path.display());
    }
    let mut d = Dicom { slope: 1.0, intercept: 0.0, spacing: [1.0, 1.0], thickness: 0.0, frames: 1, samples: 1, bits: 16, little: true, ..Default::default() };
    // File meta information: explicit VR little endian.
    let mut pos = 132;
    let mut syntax = String::from("1.2.840.10008.1.2.1");
    while pos + 8 <= b.len() {
        let group = u16::from_le_bytes([b[pos], b[pos + 1]]);
        if group != 0x0002 {
            break;
        }
        let elem = u16::from_le_bytes([b[pos + 2], b[pos + 3]]);
        let vr = &b[pos + 4..pos + 6];
        let (len, hdr) = if matches!(vr, b"OB" | b"OW" | b"SQ" | b"UN" | b"UT" | b"OF" | b"OD" | b"OL" | b"UC" | b"UR" | b"OV" | b"SV" | b"UV") {
            (u32::from_le_bytes(b[pos + 8..pos + 12].try_into().unwrap()) as usize, 12)
        } else {
            (u16::from_le_bytes([b[pos + 6], b[pos + 7]]) as usize, 8)
        };
        if elem == 0x0010 {
            syntax = String::from_utf8_lossy(&b[pos + hdr..(pos + hdr + len).min(b.len())]).trim_end_matches(['\0', ' ']).to_string();
        }
        pos += hdr + len;
    }
    let (explicit, little, deflated) = match syntax.as_str() {
        "1.2.840.10008.1.2" => (false, true, false),
        "1.2.840.10008.1.2.1" => (true, true, false),
        "1.2.840.10008.1.2.2" => (true, false, false),
        "1.2.840.10008.1.2.1.99" => (true, true, true),
        other => bail!("{}: compressed DICOM (transfer syntax {other}) is not supported; save the series uncompressed", path.display()),
    };
    d.little = little;
    let body: Vec<u8> = if deflated { inflate(&b[pos..], false)? } else { b[pos..].to_vec() };
    walk_dicom(&body, explicit, little, &mut d, 0)?;
    if d.rows == 0 || d.cols == 0 || d.pixel.is_empty() {
        bail!("{}: no image in this DICOM file", path.display());
    }
    Ok(d)
}

fn walk_dicom(b: &[u8], explicit: bool, little: bool, d: &mut Dicom, depth: usize) -> Result<usize> {
    let u16_at = |p: usize| if little { u16::from_le_bytes([b[p], b[p + 1]]) } else { u16::from_be_bytes([b[p], b[p + 1]]) };
    let u32_at = |p: usize| {
        let a: [u8; 4] = b[p..p + 4].try_into().unwrap();
        if little {
            u32::from_le_bytes(a)
        } else {
            u32::from_be_bytes(a)
        }
    };
    let mut pos = 0;
    while pos + 8 <= b.len() {
        let group = u16_at(pos);
        let elem = u16_at(pos + 2);
        // Item and sequence delimiters (always 4-byte length, no VR).
        if group == 0xFFFE {
            let len = u32_at(pos + 4);
            pos += 8;
            if elem == 0xE00D || elem == 0xE0DD {
                return Ok(pos);
            }
            if elem == 0xE000 {
                if len == 0xFFFF_FFFF {
                    pos += walk_dicom(&b[pos..], explicit, little, &mut d.clone(), depth + 1)?;
                } else {
                    pos += len as usize;
                }
            }
            continue;
        }
        let (vr, len, hdr): ([u8; 2], u32, usize) = if explicit {
            let vr = [b[pos + 4], b[pos + 5]];
            if matches!(&vr, b"OB" | b"OW" | b"SQ" | b"UN" | b"UT" | b"OF" | b"OD" | b"OL" | b"UC" | b"UR" | b"OV" | b"SV" | b"UV") {
                (vr, u32_at(pos + 8), 12)
            } else {
                (vr, u16_at(pos + 6) as u32, 8)
            }
        } else {
            ([0, 0], u32_at(pos + 4), 8)
        };
        pos += hdr;
        let is_sq = &vr == b"SQ" || (!explicit && len == 0xFFFF_FFFF && !(group == 0x7FE0 && elem == 0x0010));
        if len == 0xFFFF_FFFF {
            if group == 0x7FE0 && elem == 0x0010 {
                bail!("encapsulated (compressed) pixel data is not supported");
            }
            if is_sq || &vr == b"UN" {
                // A sequence of undefined length: skip its items.
                let mut q = pos;
                loop {
                    if q + 8 > b.len() {
                        return Ok(b.len());
                    }
                    let (g2, e2, l2) = (u16_at(q), u16_at(q + 2), u32_at(q + 4));
                    q += 8;
                    if g2 == 0xFFFE && e2 == 0xE0DD {
                        break;
                    }
                    if g2 == 0xFFFE && e2 == 0xE000 {
                        if l2 == 0xFFFF_FFFF {
                            let mut scratch = d.clone();
                            q += walk_dicom(&b[q..], explicit, little, &mut scratch, depth + 1)?;
                        } else {
                            q += l2 as usize;
                        }
                    }
                }
                pos = q;
                continue;
            }
        }
        let len = len as usize;
        if pos + len > b.len() {
            if group == 0x7FE0 && elem == 0x0010 {
                d.pixel = b[pos..].to_vec();
            }
            break;
        }
        let v = &b[pos..pos + len];
        if is_sq {
            pos += len;
            continue;
        }
        let text = || String::from_utf8_lossy(v).trim_end_matches(['\0', ' ']).trim().to_string();
        let nums = || -> Vec<f64> { text().split('\\').filter_map(|s| s.trim().parse().ok()).collect() };
        let us = || if len >= 2 { (if little { u16::from_le_bytes([v[0], v[1]]) } else { u16::from_be_bytes([v[0], v[1]]) }) as usize } else { 0 };
        if depth == 0 {
            match (group, elem) {
                (0x0028, 0x0010) => d.rows = us(),
                (0x0028, 0x0011) => d.cols = us(),
                (0x0028, 0x0002) => d.samples = us().max(1),
                (0x0028, 0x0100) => d.bits = us(),
                (0x0028, 0x0103) => d.signed = us() == 1,
                (0x0028, 0x0008) => d.frames = text().parse().unwrap_or(1),
                (0x0028, 0x1053) => d.slope = nums().first().copied().unwrap_or(1.0),
                (0x0028, 0x1052) => d.intercept = nums().first().copied().unwrap_or(0.0),
                (0x0028, 0x0030) => {
                    let n = nums();
                    if n.len() >= 2 {
                        // Row spacing (between rows), then column spacing.
                        d.spacing = [n[1], n[0]];
                    }
                }
                (0x0018, 0x0050) => d.thickness = nums().first().copied().unwrap_or(0.0),
                (0x0018, 0x0088) if d.thickness == 0.0 => d.thickness = nums().first().copied().unwrap_or(0.0),
                (0x0020, 0x0032) => {
                    let n = nums();
                    if n.len() >= 3 {
                        d.position = Some([n[0], n[1], n[2]]);
                    }
                }
                (0x0020, 0x0037) => {
                    let n = nums();
                    if n.len() >= 6 {
                        d.orientation = Some([n[0], n[1], n[2], n[3], n[4], n[5]]);
                    }
                }
                (0x0020, 0x0013) => d.instance = text().parse().unwrap_or(0),
                (0x0020, 0x000E) => d.series = text(),
                (0x7FE0, 0x0010) => d.pixel = v.to_vec(),
                _ => {}
            }
        }
        pos += len;
    }
    Ok(pos)
}

/// The values of one DICOM file's frames (rescaled when slope or intercept say so).
fn dicom_values(d: &Dicom) -> Result<ArrayData> {
    let n = d.rows * d.cols * d.frames * d.samples;
    let ty = match (d.bits, d.signed) {
        (8, false) => Ty::U8,
        (8, true) => Ty::I8,
        (16, false) => Ty::U16,
        (16, true) => Ty::I16,
        (32, false) => Ty::U32,
        (32, true) => Ty::I32,
        (b, _) => bail!("DICOM: {b} bits per value are not supported"),
    };
    let raw = decode(&d.pixel, ty, n, d.little)?;
    Ok(if d.slope != 1.0 || d.intercept != 0.0 { ArrayData::F32(raw.to_f64_vec().iter().map(|&x| (x * d.slope + d.intercept) as f32).collect()) } else { raw })
}

/// A DICOM file, or the series it belongs to (the files of the same series in its folder, in
/// order along the slice normal). A folder opens the first series in it.
pub fn read_dicom(path: &Path) -> Result<DataSet> {
    let (folder, first) =
        if path.is_dir() { (path.to_path_buf(), None) } else { (path.parent().unwrap_or(Path::new(".")).to_path_buf(), Some(path.to_path_buf())) };
    let mut files: Vec<PathBuf> = std::fs::read_dir(&folder)
        .with_context(|| format!("listing {}", folder.display()))?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.is_file() && is_dicom(p))
        .collect();
    files.sort();
    if files.is_empty() {
        bail!("no DICOM files in {}", folder.display());
    }
    let all: Vec<Dicom> = files.iter().map(|f| parse_dicom(f)).collect::<Result<_>>()?;
    // The chosen file's series; for a folder, the series with the most slices.
    let head = match &first {
        Some(f) => parse_dicom(f)?,
        None => {
            let mut count: BTreeMap<&str, usize> = BTreeMap::new();
            for d in &all {
                *count.entry(d.series.as_str()).or_default() += 1;
            }
            let best = count.iter().max_by_key(|(_, n)| **n).map(|(s, _)| s.to_string()).unwrap_or_default();
            all.iter().find(|d| d.series == best).unwrap().clone()
        }
    };
    let mut slices: Vec<Dicom> = all.into_iter().filter(|d| d.series == head.series && d.rows == head.rows && d.cols == head.cols).collect();
    let o = head.orientation.unwrap_or([1.0, 0.0, 0.0, 0.0, 1.0, 0.0]);
    let (r, c) = ([o[0], o[1], o[2]], [o[3], o[4], o[5]]);
    let nrm = [r[1] * c[2] - r[2] * c[1], r[2] * c[0] - r[0] * c[2], r[0] * c[1] - r[1] * c[0]];
    let along = |d: &Dicom| d.position.map(|p| p[0] * nrm[0] + p[1] * nrm[1] + p[2] * nrm[2]);
    slices.sort_by(|a, b| match (along(a), along(b)) {
        (Some(x), Some(y)) => x.total_cmp(&y),
        _ => a.instance.cmp(&b.instance),
    });
    let ncomp = head.samples;
    let mut data: Option<ArrayData> = None;
    let mut nz = 0;
    for s in &slices {
        let v = dicom_values(s)?;
        nz += s.frames;
        match &mut data {
            None => data = Some(v),
            Some(d) => {
                if std::mem::discriminant(d) != std::mem::discriminant(&v) {
                    let mut f = d.to_f64_vec();
                    f.extend(v.to_f64_vec());
                    *d = ArrayData::F64(f);
                } else {
                    d.append(&v)?;
                }
            }
        }
    }
    let dz = if slices.len() >= 2 {
        match (along(&slices[0]), along(&slices[1])) {
            (Some(a), Some(b)) if (b - a).abs() > 0.0 => (b - a).abs(),
            _ => head.thickness.max(1.0),
        }
    } else if head.thickness > 0.0 {
        head.thickness
    } else {
        1.0
    };
    // Pixel (column i, row j) of a slice sits at its position + i * column spacing along the row
    // direction + j * row spacing along the column direction (patient coordinates).
    let origin = slices[0].position.unwrap_or([0.0; 3]);
    let direction = direction_from_axes([r, c, nrm]);
    let mut im = image([head.cols, head.rows, nz], [head.spacing[0], head.spacing[1], dz], origin, direction);
    im.point_data.insert_active(AttributeKind::Scalars, Array::new("DICOMImage", ncomp, data.unwrap()));
    align_axes(&mut im);
    Ok(DataSet::ImageData(im))
}

// ------------------------------------------------------------------ OME-TIFF

/// Is this TIFF an OME-TIFF (OME-XML in its description)?
pub fn is_ome_tiff(path: &Path) -> bool {
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("").to_ascii_lowercase();
    name.ends_with(".ome.tif") || name.ends_with(".ome.tiff") || ome_xml(path).is_some()
}

fn ome_xml(path: &Path) -> Option<String> {
    let f = std::fs::File::open(path).ok()?;
    let mut dec = tiff::decoder::Decoder::new(std::io::BufReader::new(f)).ok()?;
    let d = dec.get_tag_ascii_string(tiff::tags::Tag::ImageDescription).ok()?;
    d.contains("<OME").then_some(d)
}

fn xml_attr(tag: &str, name: &str) -> Option<String> {
    let key = format!("{name}=\"");
    let i = tag.find(&key)? + key.len();
    let j = tag[i..].find('"')? + i;
    Some(tag[i..j].to_string())
}

/// OME-TIFF: the planes of a microscopy image (sizes X, Y, Z, channels C and times T, in the
/// file's dimension order) as a volume with one array per channel, and time steps for T > 1.
pub fn read_ome_tiff(path: &Path) -> Result<crate::Loaded> {
    let xml = ome_xml(path).ok_or_else(|| anyhow!("{}: no OME-XML description", path.display()))?;
    let pix_start = xml.find("<Pixels").ok_or_else(|| anyhow!("OME-XML without Pixels"))?;
    let pix_end = xml[pix_start..].find('>').map(|e| pix_start + e).unwrap_or(xml.len());
    let tag = &xml[pix_start..pix_end];
    let num = |n: &str, d: usize| xml_attr(tag, n).and_then(|v| v.parse::<usize>().ok()).unwrap_or(d);
    let (sx, sy, sz, sc, st) = (num("SizeX", 1), num("SizeY", 1), num("SizeZ", 1), num("SizeC", 1), num("SizeT", 1));
    let order = xml_attr(tag, "DimensionOrder").unwrap_or_else(|| "XYZCT".into());
    let phys = |n: &str| xml_attr(tag, n).and_then(|v| v.parse::<f64>().ok()).unwrap_or(1.0);
    let spacing = [phys("PhysicalSizeX"), phys("PhysicalSizeY"), phys("PhysicalSizeZ")];
    // Channel names.
    let mut names = Vec::new();
    let mut rest = &xml[pix_start..];
    while let Some(i) = rest.find("<Channel ") {
        let e = rest[i..].find('>').map(|e| i + e).unwrap_or(rest.len());
        names.push(xml_attr(&rest[i..e], "Name").unwrap_or_default());
        rest = &rest[e..];
    }
    let pages = crate::images::decode_tiff_pages(path)?;
    let planes = sz * sc * st;
    if pages.len() < planes {
        bail!("{}: {} planes in the file, {} in its description", path.display(), pages.len(), planes);
    }
    // Plane index of (z, c, t) for the dimension order (after X and Y).
    let tail: Vec<char> = order.chars().skip(2).collect();
    let size_of = |ch: char| match ch {
        'Z' => sz,
        'C' => sc,
        _ => st,
    };
    let plane = |z: usize, c: usize, t: usize| -> usize {
        let at = |ch: char| match ch {
            'Z' => z,
            'C' => c,
            _ => t,
        };
        at(tail[0]) + size_of(tail[0]) * (at(tail[1]) + size_of(tail[1]) * at(tail[2]))
    };
    let mut steps = Vec::new();
    for t in 0..st {
        let mut im = image([sx, sy, sz], spacing, [0.0; 3], IDENTITY);
        for c in 0..sc {
            let mut data: Option<ArrayData> = None;
            for z in 0..sz {
                let p = &pages[plane(z, c, t)];
                if p.0 != sx || p.1 != sy {
                    bail!("{}: plane {} is {}x{}, not {}x{}", path.display(), plane(z, c, t), p.0, p.1, sx, sy);
                }
                let rows = p.2.clone();
                match &mut data {
                    None => data = Some(rows),
                    Some(d) => d.append(&rows)?,
                }
            }
            let name = names.get(c).filter(|n| !n.is_empty()).cloned().unwrap_or_else(|| format!("Channel {c}"));
            let a = Array::new(name, 1, data.unwrap());
            if c == 0 {
                im.point_data.insert_active(AttributeKind::Scalars, a);
            } else {
                im.point_data.insert(a);
            }
        }
        steps.push(DataSet::ImageData(im));
    }
    if st == 1 {
        return Ok(crate::Loaded::Static(std::sync::Arc::new(steps.pop().unwrap())));
    }
    let dt = xml_attr(tag, "TimeIncrement").and_then(|v| v.parse::<f64>().ok()).unwrap_or(1.0);
    Ok(crate::Loaded::Series(TimeSeries {
        times: (0..st).map(|t| t as f64 * dt).collect(),
        steps: steps.into_iter().map(|s| TimeStepRef::Loaded(std::sync::Arc::new(s))).collect(),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn half_floats() {
        assert_eq!(f16_to_f32(0x3c00), 1.0);
        assert_eq!(f16_to_f32(0xc000), -2.0);
        assert_eq!(f16_to_f32(0x7bff), 65504.0);
        assert!((f16_to_f32(0x0001) - 5.960_464_5e-8).abs() < 1e-12);
    }

    #[test]
    fn metaimage_round_trip() {
        let dir = std::env::temp_dir().join(format!("afx-mhd-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let mut im = image([3, 4, 5], [0.5, 1.0, 2.0], [1.0, 2.0, 3.0], IDENTITY);
        im.point_data.insert(Array::new("v", 1, ArrayData::I16((0..60).map(|i| i as i16 - 30).collect())));
        let ds = DataSet::ImageData(im);
        for (name, z) in [("a.mha", false), ("b.mhd", false), ("c.mha", true)] {
            let p = dir.join(name);
            write_metaimage(&ds, &p, z).unwrap();
            let DataSet::ImageData(back) = read_metaimage(&p).unwrap() else { panic!() };
            assert_eq!(back.dims(), [3, 4, 5]);
            assert_eq!(back.spacing, [0.5, 1.0, 2.0]);
            assert_eq!(back.origin, [1.0, 2.0, 3.0]);
            let a = back.point_data.get("MetaImage").unwrap();
            assert!(matches!(a.data, ArrayData::I16(_)));
            assert_eq!(a.get(59, 0), 29.0);
        }
        let _ = std::fs::remove_dir_all(&dir);
    }
}
