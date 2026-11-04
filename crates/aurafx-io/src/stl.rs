//! STL (ASCII and binary) reader -> PolyData with merged vertices.

use anyhow::Result;
use aurafx_data::*;
use std::collections::HashMap;
use std::path::Path;

pub fn read(path: &Path) -> Result<DataSet> {
    let bytes = crate::read_bytes(path)?;
    let is_ascii = bytes.starts_with(b"solid") && !looks_binary(&bytes);
    let tris: Vec<[[f32; 3]; 3]> = if is_ascii { parse_ascii(&bytes) } else { parse_binary(&bytes) };
    let mut map: HashMap<[u32; 3], u32> = HashMap::new();
    let mut pts: Vec<f32> = Vec::new();
    let mut polys = Cells::with_capacity(tris.len(), tris.len() * 3);
    for t in &tris {
        let mut ids = [0u32; 3];
        for (k, v) in t.iter().enumerate() {
            let key = [v[0].to_bits(), v[1].to_bits(), v[2].to_bits()];
            let id = *map.entry(key).or_insert_with(|| {
                pts.extend_from_slice(v);
                (pts.len() / 3 - 1) as u32
            });
            ids[k] = id;
        }
        polys.push(CellType::Triangle, &ids);
    }
    Ok(DataSet::PolyData(PolyData { points: Points::new_f32(pts), polys, ..Default::default() }))
}

fn looks_binary(b: &[u8]) -> bool {
    if b.len() < 84 {
        return false;
    }
    let n = u32::from_le_bytes([b[80], b[81], b[82], b[83]]) as usize;
    84 + n * 50 == b.len()
}

fn parse_binary(b: &[u8]) -> Vec<[[f32; 3]; 3]> {
    if b.len() < 84 {
        return vec![];
    }
    let n = u32::from_le_bytes([b[80], b[81], b[82], b[83]]) as usize;
    let f = |o: usize| f32::from_le_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]]);
    (0..n)
        .filter_map(|i| {
            let o = 84 + i * 50 + 12;
            if o + 36 > b.len() {
                return None;
            }
            Some([[f(o), f(o + 4), f(o + 8)], [f(o + 12), f(o + 16), f(o + 20)], [f(o + 24), f(o + 28), f(o + 32)]])
        })
        .collect()
}

fn parse_ascii(b: &[u8]) -> Vec<[[f32; 3]; 3]> {
    let text = String::from_utf8_lossy(b);
    let mut out = Vec::new();
    let mut cur: Vec<[f32; 3]> = Vec::new();
    for line in text.lines() {
        let t: Vec<&str> = line.split_whitespace().collect();
        if t.len() == 4 && t[0] == "vertex" {
            if let (Ok(x), Ok(y), Ok(z)) = (t[1].parse(), t[2].parse(), t[3].parse()) {
                cur.push([x, y, z]);
                if cur.len() == 3 {
                    out.push([cur[0], cur[1], cur[2]]);
                    cur.clear();
                }
            }
        }
    }
    out
}
