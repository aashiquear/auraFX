//! Color maps (256-entry RGB lookup tables): built-in presets, user presets registered at run
//! time, and ParaView `.json` color map import/export.

use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use std::sync::{LazyLock, OnceLock};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ColorMap {
    pub name: String,
    /// Control points (t, [r, g, b]) with t and colors in 0..1, sorted by t.
    pub stops: Vec<(f32, [f32; 3])>,
}

impl ColorMap {
    pub fn new(name: impl Into<String>, mut stops: Vec<(f32, [f32; 3])>) -> Self {
        stops.sort_by(|a, b| a.0.total_cmp(&b.0));
        if stops.is_empty() {
            stops = vec![(0.0, [0.0; 3]), (1.0, [1.0; 3])];
        }
        Self { name: name.into(), stops }
    }

    /// The same map running the other way.
    pub fn reversed(&self) -> Self {
        let mut stops: Vec<(f32, [f32; 3])> = self.stops.iter().map(|(t, c)| (1.0 - t, *c)).collect();
        stops.reverse();
        Self { name: self.name.clone(), stops }
    }

    /// `n` flat color bands (n >= 2), each the map's color at the band center.
    pub fn banded(&self, n: u32) -> Self {
        let n = n.max(2) as usize;
        let mut stops = Vec::with_capacity(2 * n);
        for i in 0..n {
            let c = self.sample((i as f32 + 0.5) / n as f32);
            let (a, b) = (i as f32 / n as f32, (i + 1) as f32 / n as f32);
            stops.push((a, c));
            stops.push((b - 1e-6, c));
        }
        Self { name: self.name.clone(), stops }
    }

    /// Good for readers with color-vision deficiency (perceptually ordered, no red/green split).
    pub fn colorblind_safe(&self) -> bool {
        matches!(
            self.name.as_str(),
            "Viridis" | "Plasma" | "Inferno" | "Magma" | "Cividis" | "Grayscale" | "X Ray" | "Blues" | "Reds" | "Greens" | "YlOrRd" | "RdBu" | "Cool to Warm"
        )
    }

    pub fn sample(&self, t: f32) -> [f32; 3] {
        let t = t.clamp(0.0, 1.0);
        let s = &self.stops;
        if t <= s[0].0 {
            return s[0].1;
        }
        for w in s.windows(2) {
            if t <= w[1].0 {
                let u = (t - w[0].0) / (w[1].0 - w[0].0).max(1e-9);
                return [w[0].1[0] + u * (w[1].1[0] - w[0].1[0]), w[0].1[1] + u * (w[1].1[1] - w[0].1[1]), w[0].1[2] + u * (w[1].1[2] - w[0].1[2])];
            }
        }
        s[s.len() - 1].1
    }

    /// 256 RGBA8 texels.
    pub fn lut_rgba8(&self) -> Vec<u8> {
        let mut v = Vec::with_capacity(1024);
        for i in 0..256 {
            let c = self.sample(i as f32 / 255.0);
            v.extend_from_slice(&[(c[0] * 255.0) as u8, (c[1] * 255.0) as u8, (c[2] * 255.0) as u8, 255]);
        }
        v
    }
}

fn evenly(name: &str, colors: &[[f32; 3]]) -> ColorMap {
    let n = colors.len();
    ColorMap { name: name.into(), stops: colors.iter().enumerate().map(|(i, c)| (i as f32 / (n - 1) as f32, *c)).collect() }
}

fn hex(s: &str) -> [f32; 3] {
    let v = u32::from_str_radix(s.trim_start_matches('#'), 16).unwrap();
    [((v >> 16) & 255) as f32 / 255.0, ((v >> 8) & 255) as f32 / 255.0, (v & 255) as f32 / 255.0]
}

fn build() -> Vec<ColorMap> {
    vec![
        evenly("Viridis", &["440154", "482878", "3e4989", "31688e", "26828e", "1f9e89", "35b779", "6ece58", "b5de2b", "fde725"].map(hex)),
        evenly("Plasma", &["0d0887", "41049d", "6a00a8", "8f0da4", "b12a90", "cc4778", "e16462", "f2844b", "fca636", "fcce25", "f0f921"].map(hex)),
        evenly("Inferno", &["000004", "160b39", "420a68", "6a176e", "932667", "bc3754", "dd513a", "f37819", "fca50a", "f6d746", "fcffa4"].map(hex)),
        evenly("Magma", &["000004", "140e36", "3b0f70", "641a80", "8c2981", "b73779", "de4968", "f7705c", "fe9f6d", "fecf92", "fcfdbf"].map(hex)),
        evenly("Cividis", &["00224e", "123570", "3b496c", "575d6d", "707173", "8a8678", "a59c74", "c3b369", "e1cc55", "fee838"].map(hex)),
        evenly("Turbo", &["30123b", "4662d7", "36aaf9", "1ae4b6", "72fe5e", "c7ef34", "faba39", "f66b19", "ca2a04", "7a0403"].map(hex)),
        ColorMap { name: "Cool to Warm".into(), stops: vec![(0.0, [0.231, 0.298, 0.753]), (0.5, [0.865, 0.865, 0.865]), (1.0, [0.706, 0.016, 0.149])] },
        ColorMap {
            name: "Blue to Red Rainbow".into(),
            stops: vec![(0.0, [0.0, 0.0, 1.0]), (0.25, [0.0, 1.0, 1.0]), (0.5, [0.0, 1.0, 0.0]), (0.75, [1.0, 1.0, 0.0]), (1.0, [1.0, 0.0, 0.0])],
        },
        ColorMap {
            name: "Jet".into(),
            stops: vec![
                (0.0, [0.0, 0.0, 0.5]),
                (0.125, [0.0, 0.0, 1.0]),
                (0.375, [0.0, 1.0, 1.0]),
                (0.625, [1.0, 1.0, 0.0]),
                (0.875, [1.0, 0.0, 0.0]),
                (1.0, [0.5, 0.0, 0.0]),
            ],
        },
        ColorMap { name: "Grayscale".into(), stops: vec![(0.0, [0.0, 0.0, 0.0]), (1.0, [1.0, 1.0, 1.0])] },
        ColorMap {
            name: "Black-Body Radiation".into(),
            stops: vec![(0.0, [0.0, 0.0, 0.0]), (0.4, [0.9, 0.0, 0.0]), (0.8, [0.9, 0.9, 0.0]), (1.0, [1.0, 1.0, 1.0])],
        },
        ColorMap { name: "X Ray".into(), stops: vec![(0.0, [1.0, 1.0, 1.0]), (1.0, [0.0, 0.0, 0.0])] },
        evenly("Spectral", &["9e0142", "d53e4f", "f46d43", "fdae61", "fee08b", "ffffbf", "e6f598", "abdda4", "66c2a5", "3288bd", "5e4fa2"].map(hex)),
        evenly("RdBu", &["67001f", "b2182b", "d6604d", "f4a582", "fddbc7", "f7f7f7", "d1e5f0", "92c5de", "4393c3", "2166ac", "053061"].map(hex)),
        evenly("Blues", &["f7fbff", "deebf7", "c6dbef", "9ecae1", "6baed6", "4292c6", "2171b5", "08519c", "08306b"].map(hex)),
        evenly("Reds", &["fff5f0", "fee0d2", "fcbba1", "fc9272", "fb6a4a", "ef3b2c", "cb181d", "a50f15", "67000d"].map(hex)),
        evenly("Greens", &["f7fcf5", "e5f5e0", "c7e9c0", "a1d99b", "74c476", "41ab5d", "238b45", "006d2c", "00441b"].map(hex)),
        evenly("YlOrRd", &["ffffcc", "ffeda0", "fed976", "feb24c", "fd8d3c", "fc4e2a", "e31a1c", "bd0026", "800026"].map(hex)),
        elements(),
    ]
}

/// Name of the color map that colors atoms by element (atomic number over the range
/// [-0.5, 127.5]: each element owns two of the 256 lookup entries).
pub const ELEMENTS: &str = "Elements";

/// Element colors (Jmol, as VTK's periodic table) by atomic number, flat per element.
fn elements() -> ColorMap {
    let mut stops = Vec::with_capacity(256);
    for z in 0..128usize {
        let c = aurafx_data::elements::ELEMENTS.get(z).map(|e| e.color).unwrap_or([0.5, 0.5, 0.5]);
        stops.push((z as f32 / 128.0, c));
        stops.push(((z + 1) as f32 / 128.0 - 1e-6, c));
    }
    ColorMap { name: ELEMENTS.into(), stops }
}

static MAPS: OnceLock<Vec<ColorMap>> = OnceLock::new();
static USER: LazyLock<RwLock<Vec<ColorMap>>> = LazyLock::new(|| RwLock::new(Vec::new()));

/// Built-in presets.
pub fn builtin() -> &'static [ColorMap] {
    MAPS.get_or_init(build)
}

/// User presets (registered by the app from its settings, or by scripts).
pub fn user() -> Vec<ColorMap> {
    USER.read().clone()
}

/// Add or replace a user preset.
pub fn register_user(map: ColorMap) {
    let mut u = USER.write();
    u.retain(|m| !m.name.eq_ignore_ascii_case(&map.name));
    u.push(map);
}

pub fn remove_user(name: &str) {
    USER.write().retain(|m| !m.name.eq_ignore_ascii_case(name));
}

/// Look up a preset by name (user presets first); unknown names give Viridis.
pub fn get(name: &str) -> ColorMap {
    if let Some(m) = USER.read().iter().find(|m| m.name.eq_ignore_ascii_case(name)) {
        return m.clone();
    }
    builtin().iter().find(|m| m.name.eq_ignore_ascii_case(name)).unwrap_or(&builtin()[0]).clone()
}

/// Built-in then user preset names.
pub fn names() -> Vec<String> {
    builtin().iter().map(|m| m.name.clone()).chain(USER.read().iter().map(|m| m.name.clone())).collect()
}

/// Read ParaView color map presets (`[{"Name": ..., "RGBPoints": [x, r, g, b, ...]}, ...]`, or a
/// single such object). Data values in RGBPoints are normalized to 0..1.
pub fn from_paraview_json(text: &str) -> anyhow::Result<Vec<ColorMap>> {
    let v: serde_json::Value = serde_json::from_str(text)?;
    let items = match &v {
        serde_json::Value::Array(a) => a.clone(),
        o => vec![o.clone()],
    };
    let mut out = Vec::new();
    for (k, it) in items.iter().enumerate() {
        let Some(pts) = it.get("RGBPoints").and_then(|p| p.as_array()) else { continue };
        let nums: Vec<f64> = pts.iter().filter_map(|x| x.as_f64()).collect();
        if nums.len() < 8 || nums.len() % 4 != 0 {
            continue;
        }
        let (lo, hi) = (nums[0], nums[nums.len() - 4]);
        let span = if hi > lo { hi - lo } else { 1.0 };
        let stops = nums.chunks_exact(4).map(|c| (((c[0] - lo) / span) as f32, [c[1] as f32, c[2] as f32, c[3] as f32])).collect();
        let name = it.get("Name").and_then(|n| n.as_str()).map(|s| s.to_string()).unwrap_or_else(|| format!("Imported {}", k + 1));
        out.push(ColorMap::new(name, stops));
    }
    if out.is_empty() {
        anyhow::bail!("no color maps with RGBPoints found");
    }
    Ok(out)
}

/// A ParaView-compatible preset file for `maps`.
pub fn to_paraview_json(maps: &[ColorMap]) -> String {
    let arr: Vec<serde_json::Value> = maps
        .iter()
        .map(|m| {
            let pts: Vec<f64> = m.stops.iter().flat_map(|(t, c)| [*t as f64, c[0] as f64, c[1] as f64, c[2] as f64]).collect();
            serde_json::json!({ "Name": m.name, "ColorSpace": "RGB", "RGBPoints": pts, "Creator": "auraFX" })
        })
        .collect();
    serde_json::to_string_pretty(&arr).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reverse_and_bands() {
        let m = ColorMap::new("t", vec![(0.0, [0.0; 3]), (1.0, [1.0; 3])]);
        assert_eq!(m.reversed().sample(0.0), [1.0; 3]);
        let b = m.banded(4);
        assert_eq!(b.sample(0.1), b.sample(0.2), "flat inside a band");
        assert!((b.sample(0.1)[0] - 0.125).abs() < 1e-6);
        assert!((b.sample(0.9)[0] - 0.875).abs() < 1e-6);
    }

    #[test]
    fn paraview_json_roundtrip() {
        let pv = r#"[{"ColorSpace":"Diverging","Name":"Mine","RGBPoints":[-2,0,0,1, 0,1,1,1, 2,1,0,0]}]"#;
        let maps = from_paraview_json(pv).unwrap();
        assert_eq!(maps[0].name, "Mine");
        assert_eq!(maps[0].stops, vec![(0.0, [0.0, 0.0, 1.0]), (0.5, [1.0, 1.0, 1.0]), (1.0, [1.0, 0.0, 0.0])]);
        let back = from_paraview_json(&to_paraview_json(&maps)).unwrap();
        assert_eq!(back, maps);
        register_user(maps[0].clone());
        assert_eq!(get("mine").stops.len(), 3);
        assert!(names().iter().any(|n| n == "Mine"));
        remove_user("Mine");
        assert_eq!(get("Mine").name, "Viridis");
    }
}

#[cfg(test)]
mod display_tests {
    use crate::DisplayProps;

    #[test]
    fn edits_change_the_effective_map_and_its_key() {
        let mut d = DisplayProps::default();
        let k0 = d.colormap_key();
        d.colormap_reverse = true;
        let k1 = d.colormap_key();
        assert_ne!(k0, k1);
        assert_eq!(d.effective_colormap().sample(0.0), crate::colormap::get("Viridis").sample(1.0));
        d.colormap_stops = Some(vec![(0.0, [1.0, 0.0, 0.0]), (1.0, [0.0, 0.0, 1.0])]);
        assert_eq!(d.effective_colormap().sample(0.0), [0.0, 0.0, 1.0], "edited stops, reversed");
        d.colormap_bands = 2;
        let c = d.effective_colormap();
        assert_eq!(c.sample(0.1), c.sample(0.4));
        assert_ne!(d.colormap_key(), k1);
        // Display settings round-trip through JSON (state files).
        let back: DisplayProps = serde_json::from_value(serde_json::to_value(&d).unwrap()).unwrap();
        assert_eq!(back, d);
    }
}
