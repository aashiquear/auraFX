//! Geo-referenced rasters, read as ParaView's GDAL raster reader does: GeoTIFF (a TIFF with
//! GeoTIFF tags) and Esri ASCII grids (`.asc`). The result is image data with one cell per
//! pixel and one more point than pixels each way, at the map coordinates of the raster's
//! geo-transform (origin at the lower-left corner, north up). Arrays are cell data named as
//! ParaView names them: a single gray band, or the colors of an RGB(A) image as one array, is
//! "Elevation"; other bands are "Band 2", "Band 3"... (the first stays "Elevation"). No-data
//! values become NaN (ParaView hides those cells instead). Field data carries
//! `MAP_PROJECTION` (the EPSG code, or the `.prj` text beside an ASCII grid) and
//! `NO_DATA_VALUE` per band.

use anyhow::{anyhow, bail, Context, Result};
use aurafx_data::*;
use std::path::Path;
use tiff::decoder::{Decoder, DecodingResult};
use tiff::tags::Tag;

/// x = c + a i + b j, y = f + d i + e j for pixel corner (i, j), j counted from the top row.
struct GeoTransform {
    c: f64,
    a: f64,
    b: f64,
    f: f64,
    d: f64,
    e: f64,
}

/// Whether a TIFF carries GeoTIFF placement tags.
pub fn is_geotiff(path: &Path) -> bool {
    let Ok(f) = std::fs::File::open(path) else { return false };
    let Ok(mut dec) = Decoder::new(std::io::BufReader::new(f)) else { return false };
    [Tag::ModelPixelScaleTag, Tag::ModelTiepointTag, Tag::ModelTransformationTag].into_iter().any(|t| dec.find_tag(t).ok().flatten().is_some())
}

fn f64s<R: std::io::Read + std::io::Seek>(dec: &mut Decoder<R>, tag: Tag) -> Option<Vec<f64>> {
    dec.find_tag(tag).ok().flatten().and_then(|v| v.into_f64_vec().ok())
}

/// Builds the image from the rows (top first) of each band and the geo-transform.
fn image(w: usize, h: usize, gt: &GeoTransform, bands: Vec<(String, usize, ArrayData)>, nodata: &[Option<f64>], projection: Option<String>) -> DataSet {
    // Opposite corners as GDAL reports them; the image is laid out between them.
    let (x0, y0) = (gt.c, gt.f);
    let (x1, y1) = (gt.c + gt.a * w as f64 + gt.b * h as f64, gt.f + gt.d * w as f64 + gt.e * h as f64);
    let (sx, sy) = ((x1 - x0) / w as f64, (y1 - y0) / h as f64);
    let (flip_x, flip_y) = (sx < 0.0, sy < 0.0);
    let mut img = ImageData {
        extent: [0, w as i64, 0, h as i64, 0, 0],
        origin: [x0.min(x1), y0.min(y1), 0.0],
        spacing: [sx.abs(), sy.abs(), 1.0],
        direction: [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0],
        point_data: Attributes::new(),
        cell_data: Attributes::new(),
        field_data: Attributes::new(),
    };
    for (k, (name, ncomp, data)) in bands.into_iter().enumerate() {
        let idx: Vec<usize> = (0..h)
            .flat_map(|j| {
                let row = if flip_y { h - 1 - j } else { j };
                (0..w).map(move |i| row * w + if flip_x { w - 1 - i } else { i })
            })
            .collect();
        let mut data = data.gather(ncomp, &idx);
        if let Some(nd) = nodata.get(k).copied().flatten() {
            let vals = data.to_f64_vec();
            if vals.iter().any(|&v| v == nd || (v as f32) == (nd as f32)) {
                let fix = |v: f64| if v == nd || (v as f32) == (nd as f32) { f64::NAN } else { v };
                data = match data {
                    ArrayData::F64(_) | ArrayData::I32(_) | ArrayData::U32(_) | ArrayData::I64(_) | ArrayData::U64(_) => ArrayData::F64(vals.into_iter().map(fix).collect()),
                    _ => ArrayData::F32(vals.into_iter().map(|v| fix(v) as f32).collect()),
                };
            }
        }
        let arr = Array::new(name, ncomp, data);
        if k == 0 {
            img.cell_data.insert_active(AttributeKind::Scalars, arr);
        } else {
            img.cell_data.insert(arr);
        }
    }
    if let Some(p) = projection {
        img.field_data.insert(Array::new("MAP_PROJECTION", 1, ArrayData::String(vec![p])));
    }
    img.field_data.insert(Array::scalars_f64("NO_DATA_VALUE".to_string(), nodata.iter().map(|n| n.unwrap_or(f64::NAN)).collect()));
    DataSet::ImageData(img)
}

/// The decoded samples as an array (interleaved when there are several per pixel).
fn samples(r: DecodingResult) -> Result<ArrayData> {
    Ok(match r {
        DecodingResult::U8(v) => ArrayData::U8(v),
        DecodingResult::U16(v) => ArrayData::U16(v),
        DecodingResult::U32(v) => ArrayData::U32(v),
        DecodingResult::U64(v) => ArrayData::U64(v),
        DecodingResult::I8(v) => ArrayData::I8(v),
        DecodingResult::I16(v) => ArrayData::I16(v),
        DecodingResult::I32(v) => ArrayData::I32(v),
        DecodingResult::I64(v) => ArrayData::I64(v),
        DecodingResult::F32(v) => ArrayData::F32(v),
        DecodingResult::F64(v) => ArrayData::F64(v),
        _ => bail!("unsupported GeoTIFF sample type"),
    })
}

/// A band-interleaved (planar) image, band after band: the chunks (strips or tiles) of each
/// band in turn, placed into rows.
fn read_planes<R: std::io::Read + std::io::Seek>(dec: &mut Decoder<R>, w: usize, h: usize, spp: usize) -> Result<ArrayData> {
    let (cw, ch) = dec.chunk_dimensions();
    let (cw, ch) = (cw as usize, ch as usize);
    let across = w.div_ceil(cw);
    let per_plane = across * h.div_ceil(ch);
    let mut all: Option<ArrayData> = None;
    // Where each output value (band, row, column) sits in the chunks read one after another.
    let mut at = vec![0usize; spp * w * h];
    let mut offset = 0;
    for c in 0..spp * per_plane {
        let chunk = samples(dec.read_chunk(c as u32)?)?;
        let (dw, dh) = dec.chunk_data_dimensions(c as u32);
        let (dw, dh) = (dw as usize, dh as usize);
        let (k, local) = (c / per_plane, c % per_plane);
        let (x0, y0) = ((local % across) * cw, (local / across) * ch);
        for y in 0..dh.min(h.saturating_sub(y0)) {
            for x in 0..dw.min(w.saturating_sub(x0)) {
                at[k * w * h + (y0 + y) * w + x0 + x] = offset + y * dw + x;
            }
        }
        offset += chunk.len();
        match &mut all {
            None => all = Some(chunk),
            Some(a) => a.append(&chunk)?,
        }
    }
    Ok(all.ok_or_else(|| anyhow!("no image data"))?.gather(1, &at))
}

fn band_name(k: usize, n: usize) -> String {
    if k == 0 {
        return "Elevation".into();
    }
    let digits = ((n + 1) as f64).log10().ceil().max(1.0) as usize;
    format!("Band {:0digits$}", k + 1)
}

pub fn read_geotiff(path: &Path) -> Result<DataSet> {
    let f = std::fs::File::open(path).with_context(|| format!("opening {}", path.display()))?;
    let mut dec = Decoder::new(std::io::BufReader::new(f)).with_context(|| format!("{} is not a TIFF", path.display()))?;
    let (w, h) = dec.dimensions()?;
    let (w, h) = (w as usize, h as usize);
    let spp = dec.find_tag_unsigned::<u16>(Tag::SamplesPerPixel).ok().flatten().unwrap_or(1) as usize;
    let photometric = dec.find_tag_unsigned::<u16>(Tag::PhotometricInterpretation).ok().flatten().unwrap_or(1);
    let planar = dec.find_tag_unsigned::<u16>(Tag::PlanarConfiguration).ok().flatten().unwrap_or(1);

    // Geo-transform from a transformation matrix, or a tie point and pixel scale.
    let mut gt = if let Some(m) = f64s(&mut dec, Tag::ModelTransformationTag).filter(|m| m.len() >= 8) {
        GeoTransform { a: m[0], b: m[1], c: m[3], d: m[4], e: m[5], f: m[7] }
    } else {
        let tp = f64s(&mut dec, Tag::ModelTiepointTag).filter(|t| t.len() >= 6).unwrap_or(vec![0.0; 6]);
        let sc = f64s(&mut dec, Tag::ModelPixelScaleTag).filter(|s| s.len() >= 2).unwrap_or(vec![1.0, 1.0]);
        GeoTransform { a: sc[0], b: 0.0, c: tp[3] - tp[0] * sc[0], d: 0.0, e: -sc[1], f: tp[4] + tp[1] * sc[1] }
    };
    // GeoKeys: raster type (a tie point at a pixel center moves to its corner, as GDAL does)
    // and the EPSG code of the coordinate system.
    let mut projection = None;
    if let Some(keys) = dec.find_tag_unsigned_vec::<u16>(Tag::GeoKeyDirectoryTag).ok().flatten() {
        for e in keys.chunks_exact(4).skip(1) {
            match (e[0], e[1], e[3]) {
                (1025, 0, 2) => {
                    gt.c -= 0.5 * (gt.a + gt.b);
                    gt.f -= 0.5 * (gt.d + gt.e);
                }
                (3072 | 2048, 0, code) if code > 0 && code < 32767 => {
                    if projection.is_none() || e[0] == 3072 {
                        projection = Some(format!("EPSG:{code}"));
                    }
                }
                _ => {}
            }
        }
    }
    let nodata = dec
        .find_tag(Tag::GdalNodata)
        .ok()
        .flatten()
        .and_then(|v| v.into_string().ok())
        .and_then(|s| s.trim_end_matches('\0').trim().parse::<f64>().ok());

    let raw = if planar == 2 && spp > 1 {
        read_planes(&mut dec, w, h, spp).with_context(|| format!("decoding {}", path.display()))?
    } else {
        samples(dec.read_image().with_context(|| format!("decoding {}", path.display()))?)?
    };
    let n = w * h;
    // Split into bands (planar files hold the bands one after the other).
    let band = |k: usize| -> ArrayData {
        if planar == 2 {
            raw.gather(1, &((k * n)..((k + 1) * n)).collect::<Vec<_>>())
        } else {
            raw.gather(1, &(0..n).map(|p| p * spp + k).collect::<Vec<_>>())
        }
    };
    let color = photometric == 2 && spp >= 3;
    let mut bands = Vec::new();
    if color {
        let nc = spp.min(4);
        let data = if planar == 2 {
            raw.gather(1, &(0..n * nc).map(|q| (q % nc) * n + q / nc).collect::<Vec<_>>())
        } else {
            raw.gather(1, &(0..n).flat_map(|p| (0..nc).map(move |k| p * spp + k)).collect::<Vec<_>>())
        };
        bands.push(("Elevation".to_string(), nc, data));
        for k in nc..spp {
            bands.push((band_name(k, spp), 1, band(k)));
        }
    } else {
        for k in 0..spp {
            bands.push((band_name(k, spp), 1, band(k)));
        }
    }
    let nd: Vec<Option<f64>> = bands.iter().map(|_| nodata).collect();
    Ok(image(w, h, &gt, bands, &nd, projection))
}

/// Esri ASCII grid: a header (`ncols`, `nrows`, `xllcorner`/`xllcenter`, `yllcorner`/
/// `yllcenter`, `cellsize` or `dx`/`dy`, `NODATA_value`) and rows of values from the top.
pub fn read_ascii_grid(path: &Path) -> Result<DataSet> {
    let text = std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    let mut words = text.split_ascii_whitespace().peekable();
    let mut head = std::collections::HashMap::new();
    while let Some(wd) = words.peek() {
        if wd.parse::<f64>().is_ok() {
            break;
        }
        let key = words.next().unwrap().to_ascii_lowercase();
        let val: f64 = words.next().ok_or_else(|| anyhow!("{key} has no value"))?.parse().map_err(|_| anyhow!("{key}: not a number"))?;
        head.insert(key, val);
    }
    let get = |k: &str| head.get(k).copied();
    let (w, h) = (get("ncols").ok_or_else(|| anyhow!("no ncols"))? as usize, get("nrows").ok_or_else(|| anyhow!("no nrows"))? as usize);
    let dx = get("cellsize").or(get("dx")).ok_or_else(|| anyhow!("no cellsize"))?;
    let dy = get("cellsize").or(get("dy")).unwrap_or(dx);
    let x0 = get("xllcorner").or(get("xllcenter").map(|c| c - 0.5 * dx)).ok_or_else(|| anyhow!("no xllcorner"))?;
    let y0 = get("yllcorner").or(get("yllcenter").map(|c| c - 0.5 * dy)).ok_or_else(|| anyhow!("no yllcorner"))?;
    let vals: Vec<f64> = words.take(w * h).map(|s| s.parse::<f64>().map_err(|_| anyhow!("bad value {s:?}"))).collect::<Result<_>>()?;
    if vals.len() != w * h {
        bail!("{}: {} values for {w} x {h} cells", path.display(), vals.len());
    }
    let gt = GeoTransform { c: x0, a: dx, b: 0.0, f: y0 + dy * h as f64, d: 0.0, e: -dy };
    let projection = std::fs::read_to_string(path.with_extension("prj")).ok().map(|s| s.trim().to_string()).filter(|s| !s.is_empty());
    Ok(image(w, h, &gt, vec![("Elevation".into(), 1, ArrayData::F64(vals))], &[get("nodata_value")], projection))
}

// ------------------------------------------------------------------ ERDAS Imagine (.img)

/// A field of an HFA data dictionary type (GDAL's HFAField): `count:[p|*]type[object,]name,`.
#[derive(Debug, Clone)]
struct HfaField {
    count: usize,
    pointer: Option<char>,
    ty: char,
    object: Option<String>,
    enums: Vec<String>,
    name: String,
}

/// A decoded field value.
#[derive(Debug, Clone)]
enum HfaVal {
    Num(Vec<f64>),
    Text(String),
    Enum(Vec<String>),
    Obj(Vec<std::collections::BTreeMap<String, HfaVal>>),
    /// Rows x columns of numbers (BASEDATA).
    Base(Vec<f64>),
}

type HfaObj = std::collections::BTreeMap<String, HfaVal>;

/// The data dictionary: the fields of each type, parsed as GDAL's HFADictionary does.
struct HfaDict {
    types: std::collections::HashMap<String, Vec<HfaField>>,
}

impl HfaDict {
    fn parse(text: &str) -> HfaDict {
        let mut types = std::collections::HashMap::new();
        let mut s = text;
        while !s.is_empty() && !s.starts_with('.') {
            let Some(open) = s.find('{') else { break };
            s = &s[open + 1..];
            let mut fields = Vec::new();
            while !s.is_empty() && !s.starts_with('}') {
                match Self::field(s) {
                    Some((f, rest)) => {
                        fields.push(f);
                        s = rest;
                    }
                    None => return HfaDict { types },
                }
            }
            s = s.get(1..).unwrap_or("");
            let end = s.find(',').unwrap_or(s.len());
            types.insert(s[..end].to_string(), fields);
            s = s.get(end + 1..).unwrap_or("");
        }
        HfaDict { types }
    }
    fn field(s: &str) -> Option<(HfaField, &str)> {
        let colon = s.find(':')?;
        let count = s[..colon].trim().parse().ok()?;
        let mut s = &s[colon + 1..];
        let pointer = s.chars().next().filter(|c| *c == 'p' || *c == '*');
        if pointer.is_some() {
            s = &s[1..];
        }
        let mut ty = s.chars().next()?;
        s = &s[1..];
        let mut object = None;
        if ty == 'o' {
            let end = s.find(',')?;
            object = Some(s[..end].to_string());
            s = &s[end + 1..];
        } else if ty == 'x' && s.starts_with('{') {
            // An inline definition: skipped, its type name taken.
            let mut depth = 0;
            let mut i = 0;
            for (k, c) in s.char_indices() {
                if c == '{' {
                    depth += 1;
                } else if c == '}' {
                    depth -= 1;
                    if depth == 0 {
                        i = k + 1;
                        break;
                    }
                }
            }
            s = &s[i..];
            let end = s.find(',')?;
            object = Some(s[..end].to_string());
            s = &s[end + 1..];
            ty = 'o';
        }
        let mut enums = Vec::new();
        if ty == 'e' {
            let colon = s.find(':')?;
            let n: usize = s[..colon].parse().ok()?;
            s = &s[colon + 1..];
            for _ in 0..n {
                let end = s.find(',')?;
                enums.push(s[..end].to_string());
                s = &s[end + 1..];
            }
        }
        let end = s.find(',')?;
        let name = s[..end].to_string();
        Some((HfaField { count, pointer, ty, object, enums, name }, &s[end + 1..]))
    }
    fn item_size(ty: char) -> usize {
        match ty {
            '1' | '2' | '4' | 'c' | 'C' => 1,
            'e' | 's' | 'S' => 2,
            't' | 'l' | 'L' | 'f' => 4,
            'd' | 'm' => 8,
            'M' => 16,
            _ => 0,
        }
    }
    /// A type's fixed size, None when it varies (GDAL's CompleteDefn).
    fn type_size(&self, name: &str, depth: usize) -> Option<usize> {
        if depth > 16 {
            return None;
        }
        self.types.get(name)?.iter().map(|f| self.field_size(f, depth + 1)).sum()
    }
    fn field_size(&self, f: &HfaField, depth: usize) -> Option<usize> {
        if f.pointer == Some('p') {
            return None;
        }
        if let Some(o) = &f.object {
            let n = self.type_size(o, depth)? * f.count;
            return Some(if f.pointer == Some('*') { n + 8 } else { n });
        }
        match f.ty {
            'b' => None,
            t => Some(Self::item_size(t) * f.count),
        }
    }
    /// The fields of an instance of type `name` from `b`, and the bytes it took.
    fn decode(&self, name: &str, b: &[u8], depth: usize) -> Result<(HfaObj, usize)> {
        if depth > 16 {
            bail!("HFA: types nested too deep");
        }
        let fields = self.types.get(name).ok_or_else(|| anyhow!("HFA: no type {name} in the dictionary"))?;
        let mut out = HfaObj::new();
        let mut at = 0;
        for f in fields {
            let rest = b.get(at..).unwrap_or(&[]);
            let (v, used) = self.decode_field(f, rest, depth)?;
            out.insert(f.name.clone(), v);
            at += self.field_size(f, depth + 1).unwrap_or(used);
        }
        Ok((out, at))
    }
    fn decode_field(&self, f: &HfaField, b: &[u8], depth: usize) -> Result<(HfaVal, usize)> {
        let u32_at = |o: usize| -> u32 { b.get(o..o + 4).map(|x| u32::from_le_bytes(x.try_into().unwrap())).unwrap_or(0) };
        let mut at = 0;
        let mut count = f.count;
        if f.pointer.is_some() {
            count = u32_at(0) as usize;
            at = 8;
        }
        if let Some(o) = &f.object {
            let mut list = Vec::new();
            for _ in 0..count {
                let (v, used) = self.decode(o, b.get(at..).unwrap_or(&[]), depth + 1)?;
                list.push(v);
                at += used;
                if used == 0 {
                    break;
                }
            }
            return Ok((HfaVal::Obj(list), at));
        }
        Ok(match f.ty {
            'b' => {
                if count == 0 {
                    return Ok((HfaVal::Base(Vec::new()), at));
                }
                let rows = u32_at(at) as usize;
                let cols = u32_at(at + 4) as usize;
                let dt = b.get(at + 8..at + 10).map(|x| i16::from_le_bytes([x[0], x[1]])).unwrap_or(0);
                at += 12;
                let n = rows.saturating_mul(cols).min(b.len());
                let (vals, bytes) = hfa_samples(dt, b.get(at..).unwrap_or(&[]), n)?;
                (HfaVal::Base(vals), at + bytes)
            }
            'c' | 'C' => {
                let raw = b.get(at..at + count).unwrap_or(&[]);
                let end = raw.iter().position(|&c| c == 0).unwrap_or(raw.len());
                (HfaVal::Text(String::from_utf8_lossy(&raw[..end]).into_owned()), at + count)
            }
            'e' => {
                let v = (0..count).map(|k| b.get(at + 2 * k..at + 2 * k + 2).map(|x| u16::from_le_bytes([x[0], x[1]]) as usize).unwrap_or(0)).map(|i| f.enums.get(i).cloned().unwrap_or_default()).collect();
                (HfaVal::Enum(v), at + 2 * count)
            }
            t => {
                let size = Self::item_size(t);
                let v = (0..count)
                    .map(|k| {
                        let x = b.get(at + size * k..at + size * (k + 1)).unwrap_or(&[]);
                        let get = |n: usize| -> [u8; 8] {
                            let mut a = [0u8; 8];
                            a[..n.min(x.len())].copy_from_slice(&x[..n.min(x.len())]);
                            a
                        };
                        match t {
                            's' => i16::from_le_bytes(get(2)[..2].try_into().unwrap()) as f64,
                            'S' => u16::from_le_bytes(get(2)[..2].try_into().unwrap()) as f64,
                            'l' => i32::from_le_bytes(get(4)[..4].try_into().unwrap()) as f64,
                            'L' | 't' => u32::from_le_bytes(get(4)[..4].try_into().unwrap()) as f64,
                            'f' => f32::from_le_bytes(get(4)[..4].try_into().unwrap()) as f64,
                            'd' => f64::from_le_bytes(get(8)),
                            _ => get(1)[0] as f64,
                        }
                    })
                    .collect();
                (HfaVal::Num(v), at + size * count)
            }
        })
    }
}

/// Bits per sample of an HFA pixel type (EPT_u1 .. EPT_c128).
fn hfa_bits(dt: i16) -> usize {
    match dt {
        0 => 1,
        1 => 2,
        2 => 4,
        3 | 4 => 8,
        5 | 6 => 16,
        7..=9 => 32,
        10 | 11 => 64,
        12 => 128,
        _ => 0,
    }
}

/// `n` samples of HFA pixel type `dt` from `b` (sub-byte types low bits first), and the bytes
/// they take.
fn hfa_samples(dt: i16, b: &[u8], n: usize) -> Result<(Vec<f64>, usize)> {
    let bits = hfa_bits(dt);
    if bits == 0 || dt >= 11 {
        bail!("HFA: pixel type {dt} is not supported");
    }
    let bytes = (n * bits).div_ceil(8);
    if b.len() < bytes {
        bail!("HFA: a block is shorter than its pixels");
    }
    let v = (0..n)
        .map(|i| match dt {
            0 => ((b[i >> 3] >> (i & 7)) & 1) as f64,
            1 => ((b[i >> 2] >> (2 * (i & 3))) & 3) as f64,
            2 => ((b[i >> 1] >> (4 * (i & 1))) & 15) as f64,
            3 => b[i] as f64,
            4 => b[i] as i8 as f64,
            5 => u16::from_le_bytes([b[2 * i], b[2 * i + 1]]) as f64,
            6 => i16::from_le_bytes([b[2 * i], b[2 * i + 1]]) as f64,
            7 => u32::from_le_bytes(b[4 * i..4 * i + 4].try_into().unwrap()) as f64,
            8 => i32::from_le_bytes(b[4 * i..4 * i + 4].try_into().unwrap()) as f64,
            9 => f32::from_le_bytes(b[4 * i..4 * i + 4].try_into().unwrap()) as f64,
            _ => f64::from_le_bytes(b[8 * i..8 * i + 8].try_into().unwrap()),
        })
        .collect();
    Ok((v, bytes))
}

/// A run-length compressed block (GDAL's UncompressBlock): minimum, runs and values of
/// `bits` bits (big-endian beyond a byte), each added to the minimum; f32 values are the
/// minimum's bits plus the value.
fn hfa_uncompress(c: &[u8], n: usize, dt: i16) -> Result<Vec<f64>> {
    if c.len() < 13 {
        bail!("HFA: short compressed block");
    }
    let min = u32::from_le_bytes(c[0..4].try_into().unwrap());
    let runs = i32::from_le_bytes(c[4..8].try_into().unwrap());
    let data_off = i32::from_le_bytes(c[8..12].try_into().unwrap());
    let nbits = c[12] as usize;
    let value = |vals: &[u8], bit: &mut usize, pos: &mut usize| -> Result<i64> {
        let get = |i: usize| -> Result<u8> { vals.get(i).copied().ok_or_else(|| anyhow!("HFA: compressed block too short")) };
        Ok(match nbits {
            0 => 0,
            1 | 2 | 4 => {
                let v = (get(*bit >> 3)? >> (*bit & 7)) & ((1u8 << nbits) - 1);
                *bit += nbits;
                v as i64
            }
            8 => {
                *pos += 1;
                get(*pos - 1)? as i64
            }
            16 => {
                *pos += 2;
                (get(*pos - 2)? as i64) * 256 + get(*pos - 1)? as i64
            }
            32 => {
                *pos += 4;
                i32::from_be_bytes([get(*pos - 4)?, get(*pos - 3)?, get(*pos - 2)?, get(*pos - 1)?]) as i64
            }
            _ => bail!("HFA: {nbits}-bit compressed values are not supported"),
        })
    };
    let to = |raw: i64| -> f64 {
        let v = (raw as i32).wrapping_add(min as i32);
        match dt {
            9 => f32::from_bits(v as u32) as f64,
            3 => v as u8 as f64,
            4 => v as i8 as f64,
            5 => v as u16 as f64,
            6 => v as i16 as f64,
            7 => v as u32 as f64,
            0..=2 => (v & ((1 << hfa_bits(dt)) - 1)) as f64,
            _ => v as f64,
        }
    };
    let mut out = Vec::with_capacity(n);
    if runs == -1 {
        let vals = &c[13..];
        let (mut bit, mut pos) = (0, 0);
        for _ in 0..n {
            out.push(to(value(vals, &mut bit, &mut pos)?));
        }
        return Ok(out);
    }
    if runs < 0 || data_off < 0 {
        bail!("HFA: bad compressed block");
    }
    let vals = c.get(data_off as usize..).unwrap_or(&[]);
    let (mut bit, mut pos) = (0, 0);
    let mut k = 13;
    for _ in 0..runs {
        let b0 = *c.get(k).ok_or_else(|| anyhow!("HFA: compressed block too short"))? as usize;
        let extra = b0 >> 6;
        let mut count = b0 & 0x3f;
        for j in 1..=extra {
            count = count * 256 + *c.get(k + j).ok_or_else(|| anyhow!("HFA: compressed block too short"))? as usize;
        }
        k += 1 + extra;
        let v = to(value(vals, &mut bit, &mut pos)?);
        for _ in 0..count {
            if out.len() < n {
                out.push(v);
            }
        }
    }
    out.resize(n, 0.0);
    Ok(out)
}

/// A node of the HFA entry tree.
struct HfaNode {
    name: String,
    ty: String,
    data: usize,
    size: usize,
    children: Vec<HfaNode>,
}

impl HfaNode {
    fn child(&self, name: &str) -> Option<&HfaNode> {
        self.children.iter().find(|c| c.name == name)
    }
    fn child_of_type(&self, ty: &str) -> Option<&HfaNode> {
        self.children.iter().find(|c| c.ty == ty)
    }
}

fn hfa_tree(b: &[u8], ptr: usize, depth: usize) -> Result<Vec<HfaNode>> {
    let mut out = Vec::new();
    let mut p = ptr;
    let mut guard = 0;
    while p != 0 && guard < 100_000 {
        let e = b.get(p..p + 120).ok_or_else(|| anyhow!("HFA: entry outside the file"))?;
        let u = |o: usize| u32::from_le_bytes(e[o..o + 4].try_into().unwrap()) as usize;
        let text = |r: std::ops::Range<usize>| {
            let s = &e[r];
            String::from_utf8_lossy(&s[..s.iter().position(|&c| c == 0).unwrap_or(s.len())]).into_owned()
        };
        let child = u(12);
        let children = if child != 0 && depth < 32 { hfa_tree(b, child, depth + 1)? } else { Vec::new() };
        out.push(HfaNode { name: text(24..88), ty: text(88..120), data: u(16), size: u(20), children });
        p = u(0);
        guard += 1;
    }
    Ok(out)
}

/// An ERDAS Imagine raster (`.img`, GDAL's HFA driver): every Eimg_Layer band (u1 to f64,
/// in tiles, uncompressed or run-length compressed), placed by Map_Info as GDAL does, with the
/// band's no-data value and, for UTM or longitude/latitude on WGS 84, the EPSG code.
pub fn read_hfa(path: &Path) -> Result<DataSet> {
    let b = std::fs::read(path).with_context(|| format!("reading {}", path.display()))?;
    if !b.starts_with(b"EHFA_HEADER_TAG") || b.len() < 40 {
        bail!("{}: not an ERDAS Imagine file", path.display());
    }
    let u = |o: usize| -> usize { b.get(o..o + 4).map(|x| u32::from_le_bytes(x.try_into().unwrap()) as usize).unwrap_or(0) };
    let header = u(16);
    let root = u(header + 8);
    let dict_ptr = u(header + 14);
    let dict_text = b.get(dict_ptr..).map(|d| String::from_utf8_lossy(&d[..d.iter().position(|&c| c == 0).unwrap_or(d.len())]).into_owned()).unwrap_or_default();
    let dict = HfaDict::parse(&dict_text);
    let tree = hfa_tree(&b, root, 0)?;
    let root = tree.first().ok_or_else(|| anyhow!("{}: no entries", path.display()))?;
    let read = |n: &HfaNode| -> Result<HfaObj> {
        let data = b.get(n.data..n.data + n.size).ok_or_else(|| anyhow!("HFA: {} data outside the file", n.name))?;
        Ok(dict.decode(&n.ty, data, 0)?.0)
    };
    let num = |o: &HfaObj, k: &str| -> f64 {
        match o.get(k) {
            Some(HfaVal::Num(v)) => v.first().copied().unwrap_or(0.0),
            _ => 0.0,
        }
    };
    let en = |o: &HfaObj, k: &str| -> String {
        match o.get(k) {
            Some(HfaVal::Enum(v)) => v.first().cloned().unwrap_or_default(),
            _ => String::new(),
        }
    };
    let obj = |o: &HfaObj, k: &str| -> HfaObj {
        match o.get(k) {
            Some(HfaVal::Obj(v)) => v.first().cloned().unwrap_or_default(),
            _ => HfaObj::new(),
        }
    };
    let layers: Vec<&HfaNode> = root.children.iter().filter(|c| c.ty == "Eimg_Layer").collect();
    if layers.is_empty() {
        bail!("{}: no raster layers", path.display());
    }
    let (mut w, mut h) = (0, 0);
    let mut bands = Vec::new();
    let mut nodata = Vec::new();
    for (k, layer) in layers.iter().enumerate() {
        let info = read(layer)?;
        let (lw, lh) = (num(&info, "width") as usize, num(&info, "height") as usize);
        if k == 0 {
            (w, h) = (lw, lh);
        } else if (lw, lh) != (w, h) {
            continue; // GDAL skips bands of another size too
        }
        let dt = match &info.get("pixelType") {
            Some(HfaVal::Enum(v)) => ["u1", "u2", "u4", "u8", "s8", "u16", "s16", "u32", "s32", "f32", "f64", "c64", "c128"].iter().position(|t| Some(&t.to_string()) == v.first()).unwrap_or(99) as i16,
            _ => 99,
        };
        let (bw, bh) = (num(&info, "blockWidth").max(1.0) as usize, num(&info, "blockHeight").max(1.0) as usize);
        let nd = layer.child("Eimg_NonInitializedValue").map(&read).transpose()?.and_then(|o| match o.get("valueBD") {
            Some(HfaVal::Base(v)) => v.first().copied(),
            _ => None,
        });
        if layer.child("ExternalRasterDMS").is_some() {
            bail!("{}: the raster data is in an external .ige file, which is not read", path.display());
        }
        let dms = read(layer.child("RasterDMS").ok_or_else(|| anyhow!("{}: no RasterDMS", path.display()))?)?;
        let blocks = match dms.get("blockinfo") {
            Some(HfaVal::Obj(v)) => v.clone(),
            _ => Vec::new(),
        };
        let (bx, by) = (w.div_ceil(bw), h.div_ceil(bh));
        let mut vals = vec![nd.unwrap_or(0.0); w * h];
        for j in 0..by {
            for i in 0..bx {
                let Some(blk) = blocks.get(j * bx + i) else { continue };
                if en(blk, "logvalid") != "true" {
                    continue;
                }
                let (off, size) = (num(blk, "offset") as usize, num(blk, "size") as usize);
                let raw = b.get(off..off + size).ok_or_else(|| anyhow!("{}: a block lies outside the file", path.display()))?;
                let px = if en(blk, "compressionType") == "no compression" { hfa_samples(dt, raw, bw * bh)?.0 } else { hfa_uncompress(raw, bw * bh, dt)? };
                for y in 0..bh {
                    for x in 0..bw {
                        let (gx, gy) = (i * bw + x, j * bh + y);
                        if gx < w && gy < h {
                            vals[gy * w + gx] = px[y * bw + x];
                        }
                    }
                }
            }
        }
        let data = match dt {
            0..=3 => ArrayData::U8(vals.iter().map(|&v| v as u8).collect()),
            4 => ArrayData::I8(vals.iter().map(|&v| v as i8).collect()),
            5 => ArrayData::U16(vals.iter().map(|&v| v as u16).collect()),
            6 => ArrayData::I16(vals.iter().map(|&v| v as i16).collect()),
            7 => ArrayData::U32(vals.iter().map(|&v| v as u32).collect()),
            8 => ArrayData::I32(vals.iter().map(|&v| v as i32).collect()),
            9 => ArrayData::F32(vals.iter().map(|&v| v as f32).collect()),
            _ => ArrayData::F64(vals),
        };
        let name = if bands.is_empty() { "Elevation".to_string() } else { format!("Band {}", bands.len() + 1) };
        bands.push((name, 1, data));
        nodata.push(nd);
    }
    // Placement (HFAGetGeoTransform) and coordinate system from the first band.
    let first = layers[0];
    let mut gt = GeoTransform { c: 0.0, a: 1.0, b: 0.0, f: 0.0, d: 0.0, e: 1.0 };
    let mut projection = None;
    if let Some(mi) = first.child_of_type("Eprj_MapInfo").map(&read).transpose()? {
        let ul = obj(&mi, "upperLeftCenter");
        let lr = obj(&mi, "lowerRightCenter");
        let ps = obj(&mi, "pixelSize");
        let pw = if num(&ps, "width") == 0.0 { 1.0 } else { num(&ps, "width") };
        let mut ph = if num(&ul, "y") >= num(&lr, "y") { -num(&ps, "height") } else { num(&ps, "height") };
        if ph == 0.0 {
            ph = 1.0;
        }
        gt = GeoTransform { c: num(&ul, "x") - num(&ps, "width") * 0.5, a: pw, b: 0.0, f: num(&ul, "y") - ph * 0.5, d: 0.0, e: ph };
        if matches!(mi.get("units"), Some(HfaVal::Text(u)) if u.eq_ignore_ascii_case("ds")) {
            for v in [&mut gt.c, &mut gt.a, &mut gt.f, &mut gt.e] {
                *v /= 3600.0;
            }
        }
    }
    if let Some(proj_node) = first.child_of_type("Eprj_ProParameters") {
        let pro = read(proj_node)?;
        let datum = proj_node.child_of_type("Eprj_Datum").map(&read).transpose()?;
        let datum_name = datum.as_ref().and_then(|d| match d.get("datumname") {
            Some(HfaVal::Text(t)) => Some(t.clone()),
            _ => None,
        });
        let wgs84 = datum_name.as_deref().is_some_and(|d| d.replace(' ', "").eq_ignore_ascii_case("WGS84"));
        let params = match pro.get("proParams") {
            Some(HfaVal::Num(v)) => v.clone(),
            _ => Vec::new(),
        };
        let number = num(&pro, "proNumber") as i64;
        projection = match (number, wgs84) {
            (0, true) => Some("EPSG:4326".to_string()),
            (1, true) => {
                let zone = num(&pro, "proZone") as i64;
                let north = params.get(3).is_none_or(|&p| p >= 0.0);
                Some(format!("EPSG:{}", if north { 32600 } else { 32700 } + zone))
            }
            _ => match pro.get("proName") {
                Some(HfaVal::Text(n)) => Some(match &datum_name {
                    Some(d) => format!("{n} ({d})"),
                    None => n.clone(),
                }),
                _ => None,
            },
        };
    }
    Ok(image(w, h, &gt, bands, &nodata, projection))
}
