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
