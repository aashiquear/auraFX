//! Point-merging and attribute interpolation helpers shared by cutting filters.

use aurafx_data::*;
use std::collections::HashMap;

/// Builds a new point set from (a) original points that are kept and (b) points interpolated
/// along edges, de-duplicating both. Also interpolates point attributes.
pub struct PointBuilder<'a> {
    src_points: &'a Points,
    src_attrs: &'a Attributes,
    pub points: Vec<f64>,
    kept: HashMap<u32, u32>,
    edges: HashMap<(u32, u32, u32), u32>,
    /// For each output point: (i, j, t) meaning (1-t)*p_i + t*p_j.
    recipe: Vec<(u32, u32, f32)>,
}

impl<'a> PointBuilder<'a> {
    pub fn new(src_points: &'a Points, src_attrs: &'a Attributes) -> Self {
        Self { src_points, src_attrs, points: Vec::new(), kept: HashMap::new(), edges: HashMap::new(), recipe: Vec::new() }
    }

    pub fn len(&self) -> usize {
        self.recipe.len()
    }

    pub fn keep(&mut self, i: u32) -> u32 {
        if let Some(&k) = self.kept.get(&i) {
            return k;
        }
        let p = self.src_points.get(i as usize);
        self.points.extend_from_slice(&p);
        let id = self.recipe.len() as u32;
        self.recipe.push((i, i, 0.0));
        self.kept.insert(i, id);
        id
    }

    /// Point at parameter `t` along edge i->j. `t` is quantized for de-duplication.
    pub fn on_edge(&mut self, i: u32, j: u32, t: f64) -> u32 {
        let (a, b, t) = if i <= j { (i, j, t) } else { (j, i, 1.0 - t) };
        let q = (t * 1e6).round() as u32;
        if let Some(&k) = self.edges.get(&(a, b, q)) {
            return k;
        }
        let pa = self.src_points.get(a as usize);
        let pb = self.src_points.get(b as usize);
        self.points.extend_from_slice(&[pa[0] + t * (pb[0] - pa[0]), pa[1] + t * (pb[1] - pa[1]), pa[2] + t * (pb[2] - pa[2])]);
        let id = self.recipe.len() as u32;
        self.recipe.push((a, b, t as f32));
        self.edges.insert((a, b, q), id);
        id
    }

    pub fn finish(self) -> (Points, Attributes) {
        let mut attrs = Attributes::new();
        for arr in self.src_attrs.iter() {
            if arr.scalar_type() == ScalarType::String {
                continue;
            }
            let nc = arr.ncomp;
            let mut out = Vec::with_capacity(self.recipe.len() * nc);
            for &(i, j, t) in &self.recipe {
                let t = t as f64;
                for c in 0..nc {
                    let a = arr.get(i as usize, c);
                    if t == 0.0 {
                        out.push(a);
                    } else {
                        out.push(a + t * (arr.get(j as usize, c) - a));
                    }
                }
            }
            let ty = if arr.scalar_type().is_float() { arr.scalar_type() } else { ScalarType::F64 };
            let mut a = Array::new(arr.name.clone(), nc, ArrayData::from_f64(ty, &out));
            a.component_names = arr.component_names.clone();
            attrs.insert(a);
        }
        attrs.active = self.src_attrs.active.clone();
        attrs.active.retain(|_, n| attrs.arrays.contains_key(n));
        let pts = if matches!(self.src_points.data, ArrayData::F32(_)) {
            Points::new_f32(self.points.iter().map(|&v| v as f32).collect())
        } else {
            Points::new_f64(self.points)
        };
        (pts, attrs)
    }
}

/// Compact a dataset's points to only those referenced by `conn`, remapping indices in place.
pub fn compact_points(points: &Points, attrs: &Attributes, conn: &mut [u32]) -> (Points, Attributes) {
    let mut map: HashMap<u32, u32> = HashMap::new();
    let mut order: Vec<usize> = Vec::new();
    for c in conn.iter_mut() {
        let id = *map.entry(*c).or_insert_with(|| {
            order.push(*c as usize);
            (order.len() - 1) as u32
        });
        *c = id;
    }
    (points.gather(&order), attrs.gather(&order))
}

/// Per-point signed value accessor used by slice/clip/contour.
pub enum ScalarFn<'a> {
    Plane { origin: [f64; 3], normal: [f64; 3], points: &'a Points },
    Sphere { center: [f64; 3], radius: f64, points: &'a Points },
    Box { min: [f64; 3], max: [f64; 3], points: &'a Points },
    Array { arr: &'a Array, comp: Option<usize>, offset: f64 },
    Values(Vec<f64>),
}

impl<'a> ScalarFn<'a> {
    #[inline]
    pub fn eval(&self, i: usize) -> f64 {
        match self {
            ScalarFn::Plane { origin, normal, points } => {
                let p = points.get(i);
                (p[0] - origin[0]) * normal[0] + (p[1] - origin[1]) * normal[1] + (p[2] - origin[2]) * normal[2]
            }
            ScalarFn::Sphere { center, radius, points } => {
                let p = points.get(i);
                let d = ((p[0] - center[0]).powi(2) + (p[1] - center[1]).powi(2) + (p[2] - center[2]).powi(2)).sqrt();
                d - radius
            }
            ScalarFn::Box { min, max, points } => {
                let p = points.get(i);
                // Signed distance-ish: max over axes of distance outside.
                let mut m = f64::NEG_INFINITY;
                for k in 0..3 {
                    m = m.max((min[k] - p[k]).max(p[k] - max[k]));
                }
                m
            }
            ScalarFn::Array { arr, comp, offset } => match comp {
                Some(c) => arr.get(i, *c) - offset,
                None => arr.magnitude(i) - offset,
            },
            ScalarFn::Values(v) => v[i],
        }
    }
}
