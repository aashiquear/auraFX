//! Morphological models of microstructures: random sets and architectures made on a grid of
//! cells from a few numbers (a size, a volume fraction, a seed). Particles that overlap or
//! not, fibres, layers, random fields, lattices, foams, automata, growths and structures of
//! living materials. Every model gives an image with a `Phases` cell array (1: the matrix or
//! the pores, 2: the solid or the particles, 3 and above: further phases) and, where the
//! structure is made of separate objects, `FeatureIds`.
//!
//! The models follow the published methods of mathematical morphology and stochastic
//! geometry (the references are in the user guide, chapter Synthetic microstructures).

pub mod bio;
pub mod defs;
pub mod fibres;
pub mod fields;
pub mod growth;
pub mod lattices;
pub mod particles;

use crate::synthetic::Draws;
use anyhow::{bail, Result};
use aurafx_data::*;
use rayon::prelude::*;

/// The types of the Synthetic Microstructure window, by family: source ids in menu order.
pub const FAMILIES: &[(&str, &[&str])] = &[
    ("Grains and polycrystals", &["voronoi_grains", "pack_grains", "columnar_grains", "nucleation_growth_grains", "dead_leaves_mosaic"]),
    ("Particles", &["overlapping_spheres", "penetrable_spheres", "hard_spheres", "clustered_spheres"]),
    (
        "Fibres and layers",
        &[
            "overlapping_fibres",
            "poisson_line_fibres",
            "curved_fibres",
            "hard_fibres",
            "bending_fibres",
            "cross_linked_fibres",
            "delaunay_fibre_network",
            "random_laminate",
        ],
    ),
    ("Random fields", &["gaussian_field_medium", "transparent_leaves_medium", "reconstructed_medium"]),
    (
        "Lattices and foams",
        &[
            "triply_periodic_surface",
            "strut_lattice",
            "disordered_strut_lattice",
            "hierarchical_lattice",
            "fractal_sponge",
            "open_cell_foam",
            "level_set_foam",
            "scale_rich_network",
        ],
    ),
    (
        "Automata and growth",
        &[
            "cellular_automaton_structure",
            "activator_inhibitor_pattern",
            "reaction_diffusion_pattern",
            "aggregation_cluster",
            "pipe_network",
            "branching_network",
        ],
    ),
    ("Bio-inspired", &["nacre", "twisted_plywood", "honeycomb", "haversian_bone"]),
];

/// Whether a source is one of the Synthetic Microstructure types.
pub fn is_microstructure(id: &str) -> bool {
    FAMILIES.iter().any(|(_, ids)| ids.contains(&id))
}

pub const PHASE_MATRIX: i32 = 1;
pub const PHASE_SOLID: i32 = 2;

/// The grid of cells a model is made on: `n` cubes of edge `h` from the origin.
#[derive(Clone, Debug)]
pub struct Grid {
    pub n: [usize; 3],
    pub h: f64,
    pub size: [f64; 3],
    pub periodic: bool,
}

impl Grid {
    pub fn new(cells: &[f64], spacing: f64, periodic: bool) -> Result<Grid> {
        let n: [usize; 3] = std::array::from_fn(|k| cells.get(k).copied().unwrap_or(64.0).max(1.0) as usize);
        if n[0] * n[1] * n[2] > 100_000_000 {
            bail!("more than 100 million cells");
        }
        if !(spacing > 0.0) || !spacing.is_finite() {
            bail!("the cell size must be above 0");
        }
        Ok(Grid { n, h: spacing, size: n.map(|x| x as f64 * spacing), periodic })
    }
    pub fn len(&self) -> usize {
        self.n[0] * self.n[1] * self.n[2]
    }
    pub fn volume(&self) -> f64 {
        self.size[0] * self.size[1] * self.size[2]
    }
    pub fn ijk(&self, c: usize) -> [usize; 3] {
        [c % self.n[0], (c / self.n[0]) % self.n[1], c / (self.n[0] * self.n[1])]
    }
    pub fn index(&self, ijk: [usize; 3]) -> usize {
        ijk[0] + self.n[0] * (ijk[1] + self.n[1] * ijk[2])
    }
    /// The centre of a cell.
    pub fn pos(&self, c: usize) -> [f64; 3] {
        self.ijk(c).map(|i| (i as f64 + 0.5) * self.h)
    }
    /// One layer of cells: the model is drawn in the plane.
    pub fn planar(&self) -> bool {
        self.n[2] == 1
    }
    /// `a - b`, by the nearest image when the box is periodic.
    pub fn delta(&self, a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
        std::array::from_fn(|k| {
            let d = a[k] - b[k];
            if self.periodic {
                d - self.size[k] * (d / self.size[k]).round()
            } else {
                d
            }
        })
    }
    pub fn dist2(&self, a: [f64; 3], b: [f64; 3]) -> f64 {
        let d = self.delta(a, b);
        d[0] * d[0] + d[1] * d[1] + d[2] * d[2]
    }
    /// A point uniform in the box.
    pub fn random_point(&self, r: &mut dyn Draws) -> [f64; 3] {
        std::array::from_fn(|k| r.uniform(0.0, self.size[k]))
    }
    /// A point uniform in the box widened by `margin` on every side (objects that reach into a
    /// box that is not periodic); in the box itself when it is periodic.
    pub fn random_point_around(&self, margin: f64, r: &mut dyn Draws) -> [f64; 3] {
        let m = if self.periodic { 0.0 } else { margin };
        std::array::from_fn(|k| r.uniform(-m, self.size[k] + m))
    }
    /// The volume `random_point_around` draws in.
    pub fn volume_around(&self, margin: f64) -> f64 {
        let m = if self.periodic { 0.0 } else { margin };
        (0..3).map(|k| self.size[k] + 2.0 * m).product()
    }
    /// Every cell whose centre lies in the box `lo..hi`, with the position of that centre (not
    /// wrapped: a cell reached through a periodic face comes with the position of its image).
    pub fn stamp(&self, lo: [f64; 3], hi: [f64; 3], mut f: impl FnMut(usize, [f64; 3])) {
        let mut range = [(0i64, 0i64); 3];
        for k in 0..3 {
            let mut a = (lo[k] / self.h - 0.5).ceil() as i64;
            let mut b = (hi[k] / self.h - 0.5).floor() as i64;
            if !self.periodic {
                a = a.max(0);
                b = b.min(self.n[k] as i64 - 1);
            }
            if b < a {
                return;
            }
            range[k] = (a, b);
        }
        let n = self.n.map(|x| x as i64);
        for kz in range[2].0..=range[2].1 {
            let cz = kz.rem_euclid(n[2]) as usize * self.n[0] * self.n[1];
            for ky in range[1].0..=range[1].1 {
                let cy = cz + ky.rem_euclid(n[1]) as usize * self.n[0];
                for kx in range[0].0..=range[0].1 {
                    let c = cy + kx.rem_euclid(n[0]) as usize;
                    f(c, [(kx as f64 + 0.5) * self.h, (ky as f64 + 0.5) * self.h, (kz as f64 + 0.5) * self.h]);
                }
            }
        }
    }
    /// The cells of a sphere.
    pub fn stamp_sphere(&self, c: [f64; 3], radius: f64, mut f: impl FnMut(usize, f64)) {
        let r2 = radius * radius;
        self.stamp(c.map(|x| x - radius), c.map(|x| x + radius), |cell, p| {
            let d2 = (p[0] - c[0]).powi(2) + (p[1] - c[1]).powi(2) + (p[2] - c[2]).powi(2);
            if d2 <= r2 {
                f(cell, d2);
            }
        });
    }
    /// The cells within `reach` of the segment `a b`, with their distance to it and the
    /// position along it (0 at `a`, 1 at `b`, not clamped: beyond the ends for the caps).
    pub fn stamp_segment(&self, a: [f64; 3], b: [f64; 3], reach: f64, mut f: impl FnMut(usize, f64, f64)) {
        let ab = sub(b, a);
        let len2 = dot(ab, ab);
        let len = len2.sqrt();
        // In pieces, so that a long slanted segment does not visit its whole bounding box.
        let pieces = ((len / (4.0 * reach + 8.0 * self.h)).ceil() as usize).max(1);
        for k in 0..pieces {
            let (t0, t1) = (k as f64 / pieces as f64, (k + 1) as f64 / pieces as f64);
            let (p0, p1) = (add(a, scale(ab, t0)), add(a, scale(ab, t1)));
            let lo: [f64; 3] = std::array::from_fn(|i| p0[i].min(p1[i]) - reach);
            let hi: [f64; 3] = std::array::from_fn(|i| p0[i].max(p1[i]) + reach);
            self.stamp(lo, hi, |cell, p| {
                let ap = sub(p, a);
                let t = if len2 > 0.0 { dot(ap, ab) / len2 } else { 0.0 };
                // Each piece takes the cells that project on it (the ends take what lies beyond).
                if (t < t0 && k > 0) || (t >= t1 && k + 1 < pieces) {
                    return;
                }
                let tc = t.clamp(0.0, 1.0);
                let q = sub(ap, scale(ab, tc));
                let d = dot(q, q).sqrt();
                if d <= reach {
                    f(cell, d, t);
                }
            });
        }
    }
}

pub fn sub(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
pub fn add(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}
pub fn scale(a: [f64; 3], s: f64) -> [f64; 3] {
    [a[0] * s, a[1] * s, a[2] * s]
}
pub fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
pub fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}
pub fn norm(a: [f64; 3]) -> f64 {
    dot(a, a).sqrt()
}
pub fn unit(a: [f64; 3]) -> [f64; 3] {
    let l = norm(a);
    if l > 0.0 {
        scale(a, 1.0 / l)
    } else {
        [0.0, 0.0, 1.0]
    }
}
/// Two unit vectors that make a right-handed frame with the unit vector `d`.
pub fn frame(d: [f64; 3]) -> ([f64; 3], [f64; 3]) {
    let other = if d[0].abs() < 0.9 { [1.0, 0.0, 0.0] } else { [0.0, 1.0, 0.0] };
    let u = unit(cross(d, other));
    (u, cross(d, u))
}

/// A direction uniform on the sphere.
pub fn isotropic(r: &mut dyn Draws) -> [f64; 3] {
    let z = r.uniform(-1.0, 1.0);
    let a = r.uniform(0.0, std::f64::consts::TAU);
    let s = (1.0 - z * z).max(0.0).sqrt();
    [s * a.cos(), s * a.sin(), z]
}

pub const DIR_ISOTROPIC: &str = "Isotropic";
pub const DIR_Z: &str = "Along Z";
pub const DIR_PLANE: &str = "In the XY plane";
pub const DIR_BETA: &str = "Beta law about Z";
pub const DIR_GIRDLE: &str = "Girdle about the XY plane";
pub const DIRECTIONS: &[&str] = &[DIR_ISOTROPIC, DIR_Z, DIR_PLANE, DIR_BETA, DIR_GIRDLE];

/// The law of the directions of fibres. `shape` is the beta of the beta law (density
/// proportional to `beta sin t / (1 + (beta^2 - 1) cos^2 t)^(3/2)` at the angle `t` from Z:
/// 1 isotropic, towards 0 along Z, large in the plane) or the standard deviation in degrees
/// of the angle from the plane for the girdle.
#[derive(Clone, Debug)]
pub struct Directions {
    pub law: String,
    pub shape: f64,
    /// In one layer of cells every direction lies in the plane.
    pub planar: bool,
}

impl Directions {
    pub fn draw(&self, r: &mut dyn Draws) -> [f64; 3] {
        let a = r.uniform(0.0, std::f64::consts::TAU);
        if self.planar {
            return if self.law == DIR_Z { [1.0, 0.0, 0.0] } else { [a.cos(), a.sin(), 0.0] };
        }
        let z = match self.law.as_str() {
            DIR_Z => return [0.0, 0.0, 1.0],
            DIR_PLANE => 0.0,
            DIR_BETA => {
                let (u, b) = (r.uniform(-1.0, 1.0), self.shape.max(1e-6));
                u / (b * b + u * u * (1.0 - b * b)).sqrt()
            }
            DIR_GIRDLE => (self.shape.to_radians() * r.std_normal()).sin(),
            _ => r.uniform(-1.0, 1.0),
        };
        let s = (1.0 - z * z).max(0.0).sqrt();
        [s * a.cos(), s * a.sin(), z]
    }
}

/// A Poisson number of mean `mean`.
pub fn poisson(mean: f64, r: &mut dyn Draws) -> usize {
    if mean <= 0.0 {
        0
    } else if mean > 50.0 {
        (mean + mean.sqrt() * r.std_normal()).round().max(0.0) as usize
    } else {
        let limit = (-mean).exp();
        let (mut k, mut p) = (0usize, r.random());
        while p > limit {
            k += 1;
            p *= r.random();
        }
        k
    }
}

/// A size: `median` when `sigma` is 0, lognormal about it otherwise (within three sigmas).
pub fn lognormal(median: f64, sigma: f64, r: &mut dyn Draws) -> f64 {
    if sigma > 0.0 {
        median * (sigma * r.std_normal().clamp(-3.0, 3.0)).exp()
    } else {
        median
    }
}

/// The mean of the `k`-th power of that size (the clamp at three sigmas left out).
pub fn lognormal_moment(median: f64, sigma: f64, k: f64) -> f64 {
    median.powf(k) * (0.5 * k * k * sigma * sigma).exp()
}

/// The value below which the fraction `q` of the values lie.
pub fn quantile(values: &[f32], q: f64) -> f32 {
    if values.is_empty() {
        return 0.0;
    }
    let mut v = values.to_vec();
    let k = ((q.clamp(0.0, 1.0) * v.len() as f64) as usize).min(v.len() - 1);
    *v.select_nth_unstable_by(k, |a, b| a.total_cmp(b)).1
}

/// The solid (phase 2) where `field` is below its quantile `fraction`, the matrix elsewhere.
pub fn below_quantile(field: &[f32], fraction: f64) -> Vec<i32> {
    let t = quantile(field, fraction);
    // Exactly the fraction asked when many values are equal to the threshold.
    let want = (fraction.clamp(0.0, 1.0) * field.len() as f64).round() as usize;
    let mut left = want.saturating_sub(field.iter().filter(|x| **x < t).count());
    field
        .iter()
        .map(|x| {
            if *x < t {
                PHASE_SOLID
            } else if *x == t && left > 0 {
                left -= 1;
                PHASE_SOLID
            } else {
                PHASE_MATRIX
            }
        })
        .collect()
}

/// For every cell, the distance to the nearest of the segments `(a, b)`, up to `reach`
/// (infinite beyond).
pub fn segment_distance(g: &Grid, segments: &[([f64; 3], [f64; 3])], reach: f64) -> Vec<f32> {
    let mut d = vec![f32::INFINITY; g.len()];
    for (a, b) in segments {
        g.stamp_segment(*a, *b, reach, |c, dist, _| {
            if (dist as f32) < d[c] {
                d[c] = dist as f32;
            }
        });
    }
    d
}

/// The image of a model: `Phases`, `FeatureIds` when the structure is made of objects, and
/// the measured volume fraction of every phase as `PhaseFractions` (entry 0 is not used).
pub fn phase_image(g: &Grid, phases: Vec<i32>, features: Option<Vec<i32>>) -> ImageData {
    let mut img = ImageData { extent: [0, g.n[0] as i64, 0, g.n[1] as i64, 0, g.n[2] as i64], origin: [0.0; 3], spacing: [g.h; 3], ..Default::default() };
    let top = phases.iter().copied().max().unwrap_or(1).max(1) as usize;
    let mut count = vec![0usize; top + 1];
    for p in &phases {
        count[(*p).max(0) as usize] += 1;
    }
    let total = phases.len().max(1) as f64;
    if let Some(f) = features {
        img.cell_data.insert(Array::new("FeatureIds", 1, ArrayData::I32(f)));
    }
    img.cell_data.insert(Array::new("Phases", 1, ArrayData::I32(phases)));
    img.field_data.insert(Array::scalars_f64("PhaseFractions", count.iter().map(|c| *c as f64 / total).collect()));
    img
}

/// A number kept with the image (an expected fraction, a count).
pub fn note(img: &mut ImageData, name: &str, values: Vec<f64>) {
    img.field_data.insert(Array::scalars_f64(name, values));
}

/// The measured fraction of the cells of phase `phase`.
pub fn fraction_of(phases: &[i32], phase: i32) -> f64 {
    phases.iter().filter(|p| **p == phase).count() as f64 / phases.len().max(1) as f64
}

/// Evaluate `f` at every cell centre, in parallel.
pub fn per_cell<T: Send>(g: &Grid, f: impl Fn([f64; 3]) -> T + Sync) -> Vec<T> {
    (0..g.len()).into_par_iter().map(|c| f(g.pos(c))).collect()
}

/// Spheres found by the bucket of their centre: the neighbours of a point within a distance.
pub struct Buckets {
    nb: [usize; 3],
    width: [f64; 3],
    size: [f64; 3],
    periodic: bool,
    cells: Vec<Vec<u32>>,
}

impl Buckets {
    /// Buckets at least `reach` wide over the box of `g`.
    pub fn new(g: &Grid, reach: f64) -> Buckets {
        let nb: [usize; 3] = std::array::from_fn(|k| ((g.size[k] / reach.max(1e-12)).floor() as usize).clamp(1, 96));
        Buckets {
            nb,
            width: std::array::from_fn(|k| g.size[k] / nb[k] as f64),
            size: g.size,
            periodic: g.periodic,
            cells: vec![Vec::new(); nb[0] * nb[1] * nb[2]],
        }
    }
    fn coord(&self, p: [f64; 3]) -> [i64; 3] {
        std::array::from_fn(|k| {
            let x = if self.periodic { p[k].rem_euclid(self.size[k]) } else { p[k].clamp(0.0, self.size[k]) };
            ((x / self.width[k]) as i64).min(self.nb[k] as i64 - 1)
        })
    }
    pub fn insert(&mut self, p: [f64; 3], id: usize) {
        let b = self.coord(p);
        self.cells[b[0] as usize + self.nb[0] * (b[1] as usize + self.nb[1] * b[2] as usize)].push(id as u32);
    }
    pub fn clear(&mut self) {
        self.cells.iter_mut().for_each(Vec::clear);
    }
    /// The ids in the buckets around `p` (every id once): those within one bucket width.
    pub fn near(&self, p: [f64; 3], mut f: impl FnMut(usize)) {
        let b = self.coord(p);
        let mut span = [(0i64, 0i64); 3];
        for k in 0..3 {
            let n = self.nb[k] as i64;
            span[k] = if n <= 3 {
                (0, n - 1)
            } else if self.periodic {
                (b[k] - 1, b[k] + 1)
            } else {
                ((b[k] - 1).max(0), (b[k] + 1).min(n - 1))
            };
        }
        for z in span[2].0..=span[2].1 {
            for y in span[1].0..=span[1].1 {
                for x in span[0].0..=span[0].1 {
                    let i = x.rem_euclid(self.nb[0] as i64) as usize
                        + self.nb[0] * (y.rem_euclid(self.nb[1] as i64) as usize + self.nb[1] * z.rem_euclid(self.nb[2] as i64) as usize);
                    for id in &self.cells[i] {
                        f(*id as usize);
                    }
                }
            }
        }
    }
}
