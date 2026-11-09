//! LAS and LAZ point clouds (lidar): the points of LAS 1.0 to 1.4 files in point formats 0 to
//! 10, scaled and offset to real coordinates, with intensity, returns, classification, scan
//! angle, GPS time and colors as point arrays. LAZ (compressed LAS) is decompressed with the
//! `laz` crate.

use anyhow::{anyhow, bail, Context, Result};
use aurafx_data::*;
use std::path::Path;

struct Header {
    major: u8,
    minor: u8,
    offset_to_points: usize,
    n_vlr: usize,
    header_size: usize,
    format: u8,
    compressed: bool,
    record_len: usize,
    n_points: usize,
    scale: [f64; 3],
    offset: [f64; 3],
}

fn header(b: &[u8]) -> Result<Header> {
    if b.len() < 227 || &b[0..4] != b"LASF" {
        bail!("not a LAS file (no LASF signature)");
    }
    let u16_at = |p: usize| u16::from_le_bytes([b[p], b[p + 1]]) as usize;
    let u32_at = |p: usize| u32::from_le_bytes(b[p..p + 4].try_into().unwrap()) as usize;
    let f64_at = |p: usize| f64::from_le_bytes(b[p..p + 8].try_into().unwrap());
    let raw_format = b[104];
    let (major, minor) = (b[24], b[25]);
    let legacy = u32_at(107);
    let n_points = if major == 1 && minor >= 4 && b.len() >= 255 {
        let n = u64::from_le_bytes(b[247..255].try_into().unwrap()) as usize;
        if n > 0 {
            n
        } else {
            legacy
        }
    } else {
        legacy
    };
    Ok(Header {
        major,
        minor,
        header_size: u16_at(94),
        offset_to_points: u32_at(96),
        n_vlr: u32_at(100),
        format: raw_format & 0x3f,
        compressed: raw_format & 0xc0 != 0,
        record_len: u16_at(105),
        n_points,
        scale: [f64_at(131), f64_at(139), f64_at(147)],
        offset: [f64_at(155), f64_at(163), f64_at(171)],
    })
}

/// The variable length records: (user id, record id, data).
fn vlrs(b: &[u8], h: &Header) -> Vec<(String, u16, Vec<u8>)> {
    let mut out = Vec::new();
    let mut pos = h.header_size;
    for _ in 0..h.n_vlr {
        if pos + 54 > b.len() {
            break;
        }
        let user = String::from_utf8_lossy(&b[pos + 2..pos + 18]).trim_end_matches('\0').to_string();
        let rid = u16::from_le_bytes([b[pos + 18], b[pos + 19]]);
        let len = u16::from_le_bytes([b[pos + 20], b[pos + 21]]) as usize;
        let data = b.get(pos + 54..pos + 54 + len).map(|d| d.to_vec()).unwrap_or_default();
        out.push((user, rid, data));
        pos += 54 + len;
    }
    out
}

/// Read a LAS or LAZ file into points (one vertex per point).
pub fn read(path: &Path) -> Result<DataSet> {
    let b = std::fs::read(path).with_context(|| format!("reading {}", path.display()))?;
    let h = header(&b).with_context(|| path.display().to_string())?;
    if h.format > 10 {
        bail!("LAS point format {} is not known", h.format);
    }
    let base_len = match h.format {
        0 => 20,
        1 => 28,
        2 => 26,
        3 => 34,
        4 => 57,
        5 => 63,
        6 => 30,
        7 => 36,
        8 => 38,
        9 => 59,
        _ => 67,
    };
    if h.record_len < base_len {
        bail!("LAS point records of {} bytes are too short for format {} ({} bytes)", h.record_len, h.format, base_len);
    }
    let n = h.n_points;
    let raw: Vec<u8> = if h.compressed || path.extension().and_then(|e| e.to_str()).is_some_and(|e| e.eq_ignore_ascii_case("laz")) {
        let vlr = vlrs(&b, &h)
            .into_iter()
            .find(|v| v.0 == "laszip encoded" && v.1 == 22204)
            .ok_or_else(|| anyhow!("{}: compressed, but without the LASzip record", path.display()))?;
        let lazvlr = laz::LazVlr::from_buffer(&vlr.2).map_err(|e| anyhow!("LASzip record: {e}"))?;
        let mut cursor = std::io::Cursor::new(&b[..]);
        use std::io::Seek;
        cursor.seek(std::io::SeekFrom::Start(h.offset_to_points as u64))?;
        let mut dec = laz::LasZipDecompressor::new(cursor, lazvlr).map_err(|e| anyhow!("LAZ: {e}"))?;
        let mut out = vec![0u8; n * h.record_len];
        dec.decompress_many(&mut out).map_err(|e| anyhow!("LAZ: {e}"))?;
        out
    } else {
        let end = h.offset_to_points + n * h.record_len;
        if b.len() < end {
            bail!("{}: {} points of {} bytes do not fit in the file", path.display(), n, h.record_len);
        }
        b[h.offset_to_points..end].to_vec()
    };
    let rl = h.record_len;
    let i32_at = |r: usize, o: usize| i32::from_le_bytes(raw[r * rl + o..r * rl + o + 4].try_into().unwrap());
    let u16_at = |r: usize, o: usize| u16::from_le_bytes([raw[r * rl + o], raw[r * rl + o + 1]]);
    let f64_at = |r: usize, o: usize| f64::from_le_bytes(raw[r * rl + o..r * rl + o + 8].try_into().unwrap());
    let mut pts = Vec::with_capacity(n * 3);
    for r in 0..n {
        for a in 0..3 {
            pts.push(i32_at(r, 4 * a) as f64 * h.scale[a] + h.offset[a]);
        }
    }
    let new_style = h.format >= 6;
    let intensity: Vec<u16> = (0..n).map(|r| u16_at(r, 12)).collect();
    let (ret, nret, class, angle, user, source): (Vec<u8>, Vec<u8>, Vec<u8>, Vec<f32>, Vec<u8>, Vec<u16>) = if new_style {
        (
            (0..n).map(|r| raw[r * rl + 14] & 0x0f).collect(),
            (0..n).map(|r| raw[r * rl + 14] >> 4).collect(),
            (0..n).map(|r| raw[r * rl + 16]).collect(),
            (0..n).map(|r| i16::from_le_bytes([raw[r * rl + 18], raw[r * rl + 19]]) as f32 * 0.006).collect(),
            (0..n).map(|r| raw[r * rl + 17]).collect(),
            (0..n).map(|r| u16_at(r, 20)).collect(),
        )
    } else {
        (
            (0..n).map(|r| raw[r * rl + 14] & 0x07).collect(),
            (0..n).map(|r| (raw[r * rl + 14] >> 3) & 0x07).collect(),
            (0..n).map(|r| raw[r * rl + 15] & 0x1f).collect(),
            (0..n).map(|r| raw[r * rl + 16] as i8 as f32).collect(),
            (0..n).map(|r| raw[r * rl + 17]).collect(),
            (0..n).map(|r| u16_at(r, 18)).collect(),
        )
    };
    let gps_off = match h.format {
        1 | 3 | 4 | 5 => Some(20),
        6..=10 => Some(22),
        _ => None,
    };
    let rgb_off = match h.format {
        2 => Some(20),
        3 | 5 => Some(28),
        7 | 8 | 10 => Some(30),
        _ => None,
    };
    let mut pd = PolyData { points: Points::new_f64(pts), ..Default::default() };
    for r in 0..n as u32 {
        pd.verts.push(CellType::Vertex, &[r]);
    }
    let a = &mut pd.point_data;
    a.insert_active(AttributeKind::Scalars, Array::new("Intensity", 1, ArrayData::U16(intensity)));
    a.insert(Array::new("ReturnNumber", 1, ArrayData::U8(ret)));
    a.insert(Array::new("NumberOfReturns", 1, ArrayData::U8(nret)));
    a.insert(Array::new("Classification", 1, ArrayData::U8(class)));
    a.insert(Array::new("ScanAngle", 1, ArrayData::F32(angle)));
    a.insert(Array::new("UserData", 1, ArrayData::U8(user)));
    a.insert(Array::new("PointSourceId", 1, ArrayData::U16(source)));
    if let Some(o) = gps_off {
        a.insert(Array::new("GpsTime", 1, ArrayData::F64((0..n).map(|r| f64_at(r, o)).collect())));
    }
    if let Some(o) = rgb_off {
        // 16-bit colors; most files scale them to 0..65535, some to 0..255.
        let c16: Vec<u16> = (0..n).flat_map(|r| [u16_at(r, o), u16_at(r, o + 2), u16_at(r, o + 4)]).collect();
        let wide = c16.iter().any(|&v| v > 255);
        let c8: Vec<u8> = c16.iter().map(|&v| if wide { (v >> 8) as u8 } else { v as u8 }).collect();
        a.insert(Array::new("Color", 3, ArrayData::U8(c8)));
        if matches!(h.format, 8 | 10) {
            a.insert(Array::new("NearInfrared", 1, ArrayData::U16((0..n).map(|r| u16_at(r, o + 6)).collect())));
        }
    }
    pd.field_data.insert(Array::new("LASVersion", 1, ArrayData::String(vec![format!("{}.{}", h.major, h.minor)])));
    pd.field_data.insert(Array::scalars_f64("LASPointFormat", vec![h.format as f64]));
    Ok(DataSet::PolyData(pd))
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// A LAS 1.2 (format 3) or 1.4 (format 7) file with `n` points on a line, as bytes.
    pub fn make_las(n: usize, format: u8) -> Vec<u8> {
        let v14 = format >= 6;
        let header_size: u16 = if v14 { 375 } else { 227 };
        let rl: u16 = if format == 3 { 34 } else { 36 };
        let mut h = vec![0u8; header_size as usize];
        h[0..4].copy_from_slice(b"LASF");
        h[24] = 1;
        h[25] = if v14 { 4 } else { 2 };
        h[94..96].copy_from_slice(&header_size.to_le_bytes());
        h[96..100].copy_from_slice(&(header_size as u32).to_le_bytes());
        h[104] = format;
        h[105..107].copy_from_slice(&rl.to_le_bytes());
        h[107..111].copy_from_slice(&(if v14 { 0 } else { n as u32 }).to_le_bytes());
        for (k, s) in [0.01f64, 0.01, 0.001].iter().enumerate() {
            h[131 + 8 * k..139 + 8 * k].copy_from_slice(&s.to_le_bytes());
        }
        for (k, o) in [1000.0f64, 2000.0, 0.0].iter().enumerate() {
            h[155 + 8 * k..163 + 8 * k].copy_from_slice(&o.to_le_bytes());
        }
        if v14 {
            h[247..255].copy_from_slice(&(n as u64).to_le_bytes());
        }
        let mut b = h;
        for i in 0..n {
            let mut r = vec![0u8; rl as usize];
            r[0..4].copy_from_slice(&(i as i32 * 100).to_le_bytes()); // x = 1000 + i
            r[4..8].copy_from_slice(&(i as i32 * 50).to_le_bytes()); // y = 2000 + i/2
            r[8..12].copy_from_slice(&(i as i32 * 3).to_le_bytes()); // z = 0.003 i
            r[12..14].copy_from_slice(&(i as u16 * 10).to_le_bytes());
            if v14 {
                r[14] = 1 | (2 << 4);
                r[16] = 6; // building
                r[18..20].copy_from_slice(&(-500i16).to_le_bytes());
                r[22..30].copy_from_slice(&(i as f64 + 0.5).to_le_bytes());
                for c in 0..3 {
                    r[30 + 2 * c..32 + 2 * c].copy_from_slice(&((i * 1000 + c * 256) as u16).to_le_bytes());
                }
            } else {
                r[14] = 1 | (2 << 3);
                r[15] = 2; // ground
                r[16] = (-5i8) as u8;
                r[20..28].copy_from_slice(&(i as f64 + 0.5).to_le_bytes());
                for c in 0..3 {
                    r[28 + 2 * c..30 + 2 * c].copy_from_slice(&((i * 1000 + c * 256) as u16).to_le_bytes());
                }
            }
            b.extend_from_slice(&r);
        }
        b
    }

    fn write(name: &str, b: &[u8]) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("afx-las-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        let p = d.join(name);
        std::fs::write(&p, b).unwrap();
        p
    }

    #[test]
    fn las_12_and_14() {
        for (fmt, class, angle) in [(3u8, 2.0, -5.0), (7, 6.0, -3.0)] {
            let p = write(&format!("f{fmt}.las"), &make_las(20, fmt));
            let DataSet::PolyData(pd) = read(&p).unwrap() else { panic!() };
            assert_eq!(pd.points.len(), 20);
            let q = pd.points.get(7);
            assert!((q[0] - 1007.0).abs() < 1e-9 && (q[1] - 2003.5).abs() < 1e-9 && (q[2] - 0.021).abs() < 1e-12, "{q:?}");
            let a = &pd.point_data;
            assert_eq!(a.get("Intensity").unwrap().get(7, 0), 70.0);
            assert_eq!((a.get("ReturnNumber").unwrap().get(0, 0), a.get("NumberOfReturns").unwrap().get(0, 0)), (1.0, 2.0));
            assert_eq!(a.get("Classification").unwrap().get(3, 0), class);
            assert!((a.get("ScanAngle").unwrap().get(0, 0) - angle).abs() < 1e-6);
            assert_eq!(a.get("GpsTime").unwrap().get(4, 0), 4.5);
            assert_eq!(a.get("Color").unwrap().get(19, 1), ((19 * 1000 + 256) >> 8) as f64);
        }
    }

    #[test]
    fn laz_round_trip() {
        // Compress the points of a LAS 1.2 file with LASzip and read them back.
        let las = make_las(500, 3);
        let n = 500;
        let rl = 34;
        let items = laz::LazItemRecordBuilder::default_for_point_format_id(3, 0).unwrap();
        let vlr = laz::LazVlr::from_laz_items(items);
        let mut vlr_bytes = Vec::new();
        vlr.write_to(&mut vlr_bytes).unwrap();
        // Header (format 3 with the compressed bit) and one VLR; the compressor writes after them
        // (LASzip keeps the chunk table's position as an offset in the whole file).
        let mut h = las[..227].to_vec();
        h[104] = 3 | 0x80;
        h[100..104].copy_from_slice(&1u32.to_le_bytes());
        let mut rec = vec![0u8; 54];
        rec[2..2 + 14].copy_from_slice(b"laszip encoded");
        rec[18..20].copy_from_slice(&22204u16.to_le_bytes());
        rec[20..22].copy_from_slice(&(vlr_bytes.len() as u16).to_le_bytes());
        let offset = (227 + 54 + vlr_bytes.len()) as u32;
        h[96..100].copy_from_slice(&offset.to_le_bytes());
        let mut prefix = h;
        prefix.extend_from_slice(&rec);
        prefix.extend_from_slice(&vlr_bytes);
        let mut cursor = std::io::Cursor::new(prefix);
        cursor.set_position(offset as u64);
        let mut comp = laz::LasZipCompressor::new(cursor, vlr).unwrap();
        comp.compress_many(&las[227..227 + n * rl]).unwrap();
        comp.done().unwrap();
        let f = comp.into_inner().into_inner();
        let p = write("c.laz", &f);
        let DataSet::PolyData(pd) = read(&p).unwrap() else { panic!() };
        assert_eq!(pd.points.len(), 500);
        assert!((pd.points.get(499)[0] - 1499.0).abs() < 1e-9);
        assert_eq!(pd.point_data.get("Intensity").unwrap().get(321, 0), 3210.0);
    }
}
