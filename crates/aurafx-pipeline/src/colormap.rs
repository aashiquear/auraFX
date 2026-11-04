//! Color maps (256-entry RGB lookup tables).

use std::sync::OnceLock;

#[derive(Debug, Clone)]
pub struct ColorMap {
    pub name: &'static str,
    /// Control points (t, r, g, b) in 0..1.
    pub stops: Vec<(f32, [f32; 3])>,
}

impl ColorMap {
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

fn evenly(name: &'static str, colors: &[[f32; 3]]) -> ColorMap {
    let n = colors.len();
    ColorMap { name, stops: colors.iter().enumerate().map(|(i, c)| (i as f32 / (n - 1) as f32, *c)).collect() }
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
        ColorMap { name: "Cool to Warm", stops: vec![(0.0, [0.231, 0.298, 0.753]), (0.5, [0.865, 0.865, 0.865]), (1.0, [0.706, 0.016, 0.149])] },
        ColorMap {
            name: "Blue to Red Rainbow",
            stops: vec![(0.0, [0.0, 0.0, 1.0]), (0.25, [0.0, 1.0, 1.0]), (0.5, [0.0, 1.0, 0.0]), (0.75, [1.0, 1.0, 0.0]), (1.0, [1.0, 0.0, 0.0])],
        },
        ColorMap {
            name: "Jet",
            stops: vec![
                (0.0, [0.0, 0.0, 0.5]),
                (0.125, [0.0, 0.0, 1.0]),
                (0.375, [0.0, 1.0, 1.0]),
                (0.625, [1.0, 1.0, 0.0]),
                (0.875, [1.0, 0.0, 0.0]),
                (1.0, [0.5, 0.0, 0.0]),
            ],
        },
        ColorMap { name: "Grayscale", stops: vec![(0.0, [0.0, 0.0, 0.0]), (1.0, [1.0, 1.0, 1.0])] },
        ColorMap { name: "Black-Body Radiation", stops: vec![(0.0, [0.0, 0.0, 0.0]), (0.4, [0.9, 0.0, 0.0]), (0.8, [0.9, 0.9, 0.0]), (1.0, [1.0, 1.0, 1.0])] },
        ColorMap { name: "X Ray", stops: vec![(0.0, [1.0, 1.0, 1.0]), (1.0, [0.0, 0.0, 0.0])] },
        evenly("Spectral", &["9e0142", "d53e4f", "f46d43", "fdae61", "fee08b", "ffffbf", "e6f598", "abdda4", "66c2a5", "3288bd", "5e4fa2"].map(hex)),
        evenly("RdBu", &["67001f", "b2182b", "d6604d", "f4a582", "fddbc7", "f7f7f7", "d1e5f0", "92c5de", "4393c3", "2166ac", "053061"].map(hex)),
        evenly("Blues", &["f7fbff", "deebf7", "c6dbef", "9ecae1", "6baed6", "4292c6", "2171b5", "08519c", "08306b"].map(hex)),
        evenly("Reds", &["fff5f0", "fee0d2", "fcbba1", "fc9272", "fb6a4a", "ef3b2c", "cb181d", "a50f15", "67000d"].map(hex)),
        evenly("Greens", &["f7fcf5", "e5f5e0", "c7e9c0", "a1d99b", "74c476", "41ab5d", "238b45", "006d2c", "00441b"].map(hex)),
        evenly("YlOrRd", &["ffffcc", "ffeda0", "fed976", "feb24c", "fd8d3c", "fc4e2a", "e31a1c", "bd0026", "800026"].map(hex)),
    ]
}

static MAPS: OnceLock<Vec<ColorMap>> = OnceLock::new();

pub fn all() -> &'static [ColorMap] {
    MAPS.get_or_init(build)
}

pub fn get(name: &str) -> &'static ColorMap {
    all().iter().find(|m| m.name.eq_ignore_ascii_case(name)).unwrap_or(&all()[0])
}

pub fn names() -> Vec<&'static str> {
    all().iter().map(|m| m.name).collect()
}
