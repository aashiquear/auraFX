//! Images as `ImageData`: PNG, JPEG, BMP and TIFF (8/16-bit, float; gray, RGB, RGBA). A
//! multi-page TIFF is a volume; a wildcard path (`slices/img_*.png`) stacks matching files
//! (in natural order) into a volume. Row 0 is the bottom of the picture, as in VTK, and the
//! array is named like ParaView's (`PNGImage`, `JPEGImage`, `Tiff Scalars`, `BMPImage`).

use anyhow::{anyhow, bail, Context, Result};
use aurafx_data::*;
use std::path::{Path, PathBuf};

pub const EXTENSIONS: [&str; 6] = ["png", "jpg", "jpeg", "bmp", "tif", "tiff"];

pub fn is_image_ext(path: &Path) -> bool {
    path.extension().and_then(|e| e.to_str()).is_some_and(|e| EXTENSIONS.contains(&e.to_ascii_lowercase().as_str()))
}

fn array_name(path: &Path) -> &'static str {
    match path.extension().and_then(|e| e.to_str()).map(|e| e.to_ascii_lowercase()).as_deref() {
        Some("png") => "PNGImage",
        Some("jpg" | "jpeg") => "JPEGImage",
        Some("bmp") => "BMPImage",
        _ => "Tiff Scalars",
    }
}

/// One decoded picture (or TIFF page): width, height, components, rows top first.
struct Picture {
    w: usize,
    h: usize,
    ncomp: usize,
    data: ArrayData,
    /// Rows are stored bottom first already (TIFF orientation tag "bottom-left").
    bottom_up: bool,
}

/// Reverse the row order (top-first pictures become VTK's bottom-first).
fn flip_rows(data: &ArrayData, w: usize, h: usize, ncomp: usize) -> ArrayData {
    let row = w * ncomp;
    let idx: Vec<usize> = (0..h).rev().flat_map(|y| (0..row).map(move |i| y * row + i)).collect();
    data.gather(1, &idx)
}

fn decode_common(path: &Path) -> Result<Picture> {
    let img = image::open(path).with_context(|| format!("reading {}", path.display()))?;
    let (w, h) = (img.width() as usize, img.height() as usize);
    use image::DynamicImage as D;
    let (ncomp, data) = match img {
        D::ImageLuma8(i) => (1, ArrayData::U8(i.into_raw())),
        D::ImageLumaA8(i) => (2, ArrayData::U8(i.into_raw())),
        D::ImageRgb8(i) => (3, ArrayData::U8(i.into_raw())),
        D::ImageRgba8(i) => (4, ArrayData::U8(i.into_raw())),
        D::ImageLuma16(i) => (1, ArrayData::U16(i.into_raw())),
        D::ImageLumaA16(i) => (2, ArrayData::U16(i.into_raw())),
        D::ImageRgb16(i) => (3, ArrayData::U16(i.into_raw())),
        D::ImageRgba16(i) => (4, ArrayData::U16(i.into_raw())),
        D::ImageRgb32F(i) => (3, ArrayData::F32(i.into_raw())),
        D::ImageRgba32F(i) => (4, ArrayData::F32(i.into_raw())),
        other => (4, ArrayData::U8(other.to_rgba8().into_raw())),
    };
    Ok(Picture { w, h, ncomp, data, bottom_up: false })
}

/// Every page of a TIFF.
fn decode_tiff(path: &Path) -> Result<Vec<Picture>> {
    use tiff::decoder::{Decoder, DecodingResult};
    use tiff::tags::Tag;
    let f = std::fs::File::open(path).with_context(|| format!("opening {}", path.display()))?;
    let mut dec = Decoder::new(std::io::BufReader::new(f)).with_context(|| format!("{} is not a TIFF", path.display()))?;
    let mut pages = Vec::new();
    loop {
        let (w, h) = dec.dimensions()?;
        let ncomp = match dec.colortype()? {
            tiff::ColorType::Gray(_) => 1,
            tiff::ColorType::GrayA(_) => 2,
            tiff::ColorType::RGB(_) | tiff::ColorType::YCbCr(_) => 3,
            tiff::ColorType::RGBA(_) | tiff::ColorType::CMYK(_) => 4,
            other => bail!("{}: unsupported TIFF colors {other:?}", path.display()),
        };
        // Orientation 4 = rows start at the bottom-left (as VTK writes them).
        let bottom_up = dec.find_tag(Tag::Orientation).ok().flatten().and_then(|v| v.into_u16().ok()) == Some(4);
        let data = match dec.read_image()? {
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
            other => bail!("{}: unsupported TIFF samples {:?}", path.display(), std::mem::discriminant(&other)),
        };
        pages.push(Picture { w: w as usize, h: h as usize, ncomp, data, bottom_up });
        if !dec.more_images() {
            break;
        }
        dec.next_image()?;
    }
    Ok(pages)
}

/// The pages of a TIFF as (width, height, values with rows bottom first), one value per pixel.
pub(crate) fn decode_tiff_pages(path: &Path) -> Result<Vec<(usize, usize, ArrayData)>> {
    decode_tiff(path)?
        .into_iter()
        .map(|p| {
            if p.ncomp != 1 {
                bail!("{}: pages with {} values per pixel (one channel per page is expected)", path.display(), p.ncomp);
            }
            let rows = if p.bottom_up { p.data } else { flip_rows(&p.data, p.w, p.h, 1) };
            Ok((p.w, p.h, rows))
        })
        .collect()
}

fn decode(path: &Path) -> Result<Vec<Picture>> {
    let tiff = path.extension().and_then(|e| e.to_str()).is_some_and(|e| matches!(e.to_ascii_lowercase().as_str(), "tif" | "tiff"));
    if tiff {
        decode_tiff(path)
    } else {
        Ok(vec![decode_common(path)?])
    }
}

/// Pictures (all the same size and kind) stacked along z into one image.
fn volume(pages: Vec<Picture>, name: &str, what: &Path) -> Result<DataSet> {
    let first = pages.first().ok_or_else(|| anyhow!("{}: no image", what.display()))?;
    let (w, h, ncomp) = (first.w, first.h, first.ncomp);
    let mut data: Option<ArrayData> = None;
    for p in &pages {
        if (p.w, p.h, p.ncomp) != (w, h, ncomp) || std::mem::discriminant(&p.data) != std::mem::discriminant(&first.data) {
            bail!("{}: the images differ in size or kind ({}x{}x{} vs {}x{}x{})", what.display(), p.w, p.h, p.ncomp, w, h, ncomp);
        }
        let rows = if p.bottom_up { p.data.clone() } else { flip_rows(&p.data, w, h, ncomp) };
        match &mut data {
            None => data = Some(rows),
            Some(d) => d.append(&rows)?,
        }
    }
    let nz = pages.len();
    let mut img = ImageData {
        extent: [0, w as i64 - 1, 0, h as i64 - 1, 0, nz as i64 - 1],
        origin: [0.0; 3],
        spacing: [1.0; 3],
        direction: [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0],
        point_data: Attributes::new(),
        cell_data: Attributes::new(),
        field_data: Attributes::new(),
    };
    img.point_data.insert_active(AttributeKind::Scalars, Array::new(name, ncomp, data.unwrap()));
    Ok(DataSet::ImageData(img))
}

/// One image file (a multi-page TIFF is a volume).
pub fn read(path: &Path) -> Result<DataSet> {
    volume(decode(path)?, array_name(path), path)
}

/// Files matching a wildcard pattern (`*` and `?` in the file name), in natural order.
pub fn matching(pattern: &Path) -> Result<Vec<PathBuf>> {
    let dir = pattern.parent().filter(|d| !d.as_os_str().is_empty()).unwrap_or(Path::new("."));
    let pat = pattern.file_name().and_then(|f| f.to_str()).ok_or_else(|| anyhow!("bad pattern {}", pattern.display()))?;
    fn matches(p: &[u8], s: &[u8]) -> bool {
        match (p.first(), s.first()) {
            (None, None) => true,
            (Some(b'*'), _) => matches(&p[1..], s) || (!s.is_empty() && matches(p, &s[1..])),
            (Some(b'?'), Some(_)) => matches(&p[1..], &s[1..]),
            (Some(a), Some(b)) if a == b => matches(&p[1..], &s[1..]),
            _ => false,
        }
    }
    let mut files: Vec<PathBuf> = std::fs::read_dir(dir)
        .with_context(|| format!("listing {}", dir.display()))?
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.file_name().and_then(|f| f.to_str()).is_some_and(|f| matches(pat.as_bytes(), f.as_bytes())))
        .collect();
    // Natural order: compare runs of digits by value.
    let key = |p: &PathBuf| -> Vec<(String, u64)> {
        let s = p.file_name().and_then(|f| f.to_str()).unwrap_or("").to_string();
        let mut out = Vec::new();
        let mut text = String::new();
        let mut num = String::new();
        for c in s.chars() {
            if c.is_ascii_digit() {
                num.push(c);
            } else {
                if !num.is_empty() {
                    out.push((std::mem::take(&mut text), num.parse().unwrap_or(0)));
                    num.clear();
                }
                text.push(c);
            }
        }
        out.push((text, num.parse().unwrap_or(0)));
        out
    };
    files.sort_by_key(key);
    if files.is_empty() {
        bail!("no files match {}", pattern.display());
    }
    Ok(files)
}

/// The images matching `pattern`, stacked along z.
pub fn read_stack(pattern: &Path) -> Result<DataSet> {
    let files = matching(pattern)?;
    let mut pages = Vec::new();
    for f in &files {
        pages.extend(decode(f)?);
    }
    volume(pages, array_name(&files[0]), pattern)
}
