//! Point-merging and attribute interpolation helpers shared by cutting filters.

use aurafx_data::*;

/// Fast hashing for integer keys (point ids), FxHash-style. Not DoS-resistant, which is fine
/// for ids from our own data.
#[derive(Default, Clone, Copy)]
pub struct FxHasher(u64);

impl std::hash::Hasher for FxHasher {
    #[inline]
    fn write(&mut self, bytes: &[u8]) {
        for &b in bytes {
            self.add(b as u64);
        }
    }
    #[inline]
    fn write_u32(&mut self, n: u32) {
        self.add(n as u64);
    }
    #[inline]
    fn write_u64(&mut self, n: u64) {
        self.add(n);
    }
    #[inline]
    fn write_usize(&mut self, n: usize) {
        self.add(n as u64);
    }
    #[inline]
    fn finish(&self) -> u64 {
        self.0
    }
}

impl FxHasher {
    #[inline]
    fn add(&mut self, v: u64) {
        self.0 = (self.0.rotate_left(5) ^ v).wrapping_mul(0x51_7c_c1_b7_27_22_0a_95);
    }
}

pub type FxBuild = std::hash::BuildHasherDefault<FxHasher>;
pub type HashMap<K, V> = std::collections::HashMap<K, V, FxBuild>;

/// Identity of an output point: a kept input point, or a point on an input edge (endpoints
/// in increasing order, quantized parameter).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum PKey {
    Keep(u32),
    Edge(u32, u32, u32),
}

/// Builds a new point set from (a) original points that are kept and (b) points interpolated
/// along edges, de-duplicating both. Also interpolates point attributes.
///
/// Builders filled for separate chunks of cells (in parallel) are combined with [`merge`]
/// into exactly the points, in exactly the order, one builder would have made.
pub struct PointBuilder<'a> {
    src_points: &'a Points,
    src_attrs: &'a Attributes,
    pub points: Vec<f64>,
    kept: HashMap<u32, u32>,
    edges: HashMap<(u32, u32, u32), u32>,
    /// For each output point: (i, j, t) meaning (1-t)*p_i + t*p_j.
    recipe: Vec<(u32, u32, f32)>,
    keys: Vec<PKey>,
}

impl<'a> PointBuilder<'a> {
    pub fn new(src_points: &'a Points, src_attrs: &'a Attributes) -> Self {
        Self { src_points, src_attrs, points: Vec::new(), kept: HashMap::default(), edges: HashMap::default(), recipe: Vec::new(), keys: Vec::new() }
    }

    pub fn len(&self) -> usize {
        self.recipe.len()
    }

    pub fn is_empty(&self) -> bool {
        self.recipe.is_empty()
    }

    pub fn keep(&mut self, i: u32) -> u32 {
        if let Some(&k) = self.kept.get(&i) {
            return k;
        }
        let p = self.src_points.get(i as usize);
        self.points.extend_from_slice(&p);
        let id = self.recipe.len() as u32;
        self.recipe.push((i, i, 0.0));
        self.keys.push(PKey::Keep(i));
        self.kept.insert(i, id);
        id
    }

    /// A total order of output points that does not depend on chunking: kept input points by
    /// id, then points on input edges by (endpoints, parameter).
    pub fn rank(&self, id: u32) -> (u8, u32, u32, u32) {
        match self.keys[id as usize] {
            PKey::Keep(i) => (0, i, 0, 0),
            PKey::Edge(a, b, q) => (1, a, b, q),
        }
    }

    /// Coordinates of output point `id`.
    pub fn coords(&self, id: u32) -> [f64; 3] {
        let k = 3 * id as usize;
        [self.points[k], self.points[k + 1], self.points[k + 2]]
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
        self.keys.push(PKey::Edge(a, b, q));
        self.edges.insert((a, b, q), id);
        id
    }

    /// Combine builders of consecutive cell chunks. Returns the combined builder and, per
    /// chunk, the new id of each of its points. Points are numbered in order of first use,
    /// exactly as a single builder over all cells would. The result can only be finished.
    pub fn merge(src_points: &'a Points, src_attrs: &'a Attributes, parts: Vec<PointBuilder<'a>>) -> (PointBuilder<'a>, Vec<Vec<u32>>) {
        use rayon::prelude::*;
        use std::hash::{BuildHasher, Hash, Hasher};
        use std::sync::atomic::{AtomicU32, Ordering};
        const SHARDS: usize = 64;
        let nc = parts.len();
        // 1. Which chunk uses each point first: kept points by an atomic minimum, edge points by
        //    sharded maps filled in chunk order.
        let first_kept: Vec<AtomicU32> = (0..src_points.len()).map(|_| AtomicU32::new(u32::MAX)).collect();
        let hasher = FxBuild::default();
        let shard_of = |k: &(u32, u32, u32)| {
            let mut h = hasher.build_hasher();
            k.hash(&mut h);
            // High bits: the low ones of this hash are weak.
            (h.finish() >> 58) as usize % SHARDS
        };
        let by_shard: Vec<Vec<Vec<u32>>> = parts
            .par_iter()
            .enumerate()
            .map(|(c, p)| {
                let mut b = vec![Vec::new(); SHARDS];
                for (l, key) in p.keys.iter().enumerate() {
                    match *key {
                        PKey::Keep(i) => {
                            first_kept[i as usize].fetch_min(c as u32, Ordering::Relaxed);
                        }
                        PKey::Edge(a, bb, q) => b[shard_of(&(a, bb, q))].push(l as u32),
                    }
                }
                b
            })
            .collect();
        let shard_maps: Vec<HashMap<(u32, u32, u32), (u32, u32)>> = (0..SHARDS)
            .into_par_iter()
            .map(|sh| {
                let mut m = HashMap::default();
                for (c, p) in parts.iter().enumerate() {
                    for &l in &by_shard[c][sh] {
                        if let PKey::Edge(a, b, q) = p.keys[l as usize] {
                            m.entry((a, b, q)).or_insert((c as u32, l));
                        }
                    }
                }
                m
            })
            .collect();
        drop(by_shard);
        let is_first = |c: usize, l: usize, key: &PKey| match *key {
            PKey::Keep(i) => first_kept[i as usize].load(Ordering::Relaxed) == c as u32,
            PKey::Edge(a, b, q) => shard_maps[shard_of(&(a, b, q))][&(a, b, q)] == (c as u32, l as u32),
        };
        // 2. Number the first uses chunk by chunk.
        let firsts: Vec<usize> = parts.par_iter().enumerate().map(|(c, p)| p.keys.iter().enumerate().filter(|(l, k)| is_first(c, *l, k)).count()).collect();
        let mut base = Vec::with_capacity(nc + 1);
        let mut acc = 0usize;
        for f in &firsts {
            base.push(acc);
            acc += f;
        }
        base.push(acc);
        let total = acc;
        let mut remaps: Vec<Vec<u32>> = parts
            .par_iter()
            .enumerate()
            .map(|(c, p)| {
                let mut next = base[c] as u32;
                p.keys
                    .iter()
                    .enumerate()
                    .map(|(l, k)| {
                        if is_first(c, l, k) {
                            next += 1;
                            next - 1
                        } else {
                            u32::MAX
                        }
                    })
                    .collect()
            })
            .collect();
        // 3. Later uses take the id of the first use.
        let kept_gid: Vec<AtomicU32> = (0..src_points.len()).map(|_| AtomicU32::new(u32::MAX)).collect();
        parts.par_iter().zip(&remaps).for_each(|(p, r)| {
            for (l, key) in p.keys.iter().enumerate() {
                if let (PKey::Keep(i), true) = (key, r[l] != u32::MAX) {
                    kept_gid[*i as usize].store(r[l], Ordering::Relaxed);
                }
            }
        });
        let resolved: Vec<Vec<(usize, u32)>> = parts
            .par_iter()
            .zip(&remaps)
            .map(|(p, r)| {
                p.keys
                    .iter()
                    .enumerate()
                    .filter(|(l, _)| r[*l] == u32::MAX)
                    .map(|(l, key)| {
                        let g = match *key {
                            PKey::Keep(i) => kept_gid[i as usize].load(Ordering::Relaxed),
                            PKey::Edge(a, b, q) => {
                                let (c0, l0) = shard_maps[shard_of(&(a, b, q))][&(a, b, q)];
                                remaps[c0 as usize][l0 as usize]
                            }
                        };
                        (l, g)
                    })
                    .collect()
            })
            .collect();
        for (r, res) in remaps.iter_mut().zip(resolved) {
            for (l, g) in res {
                r[l] = g;
            }
        }
        // 4. Copy the first uses into place (each chunk owns a contiguous range).
        let mut points = vec![0.0f64; 3 * total];
        let mut recipe = vec![(0u32, 0u32, 0f32); total];
        let mut keys = vec![PKey::Keep(0); total];
        {
            let mut pts_s: Vec<&mut [f64]> = Vec::with_capacity(nc);
            let mut rec_s: Vec<&mut [(u32, u32, f32)]> = Vec::with_capacity(nc);
            let mut key_s: Vec<&mut [PKey]> = Vec::with_capacity(nc);
            let (mut pr, mut rr, mut kr) = (points.as_mut_slice(), recipe.as_mut_slice(), keys.as_mut_slice());
            for c in 0..nc {
                let n = firsts[c];
                let (a, b) = pr.split_at_mut(3 * n);
                pts_s.push(a);
                pr = b;
                let (a, b) = rr.split_at_mut(n);
                rec_s.push(a);
                rr = b;
                let (a, b) = kr.split_at_mut(n);
                key_s.push(a);
                kr = b;
            }
            parts.par_iter().zip(pts_s).zip(rec_s).zip(key_s).zip(&remaps).enumerate().for_each(|(c, ((((p, ps), rs), ks), r))| {
                // A chunk's first uses are exactly its ids from base[c] on (later uses point back).
                let b = base[c] as u32;
                for (l, key) in p.keys.iter().enumerate() {
                    let g = r[l];
                    if g >= b {
                        let o = (g - b) as usize;
                        ps[3 * o..3 * o + 3].copy_from_slice(&p.points[3 * l..3 * l + 3]);
                        rs[o] = p.recipe[l];
                        ks[o] = *key;
                    }
                }
            });
        }
        let mut g = PointBuilder::new(src_points, src_attrs);
        g.points = points;
        g.recipe = recipe;
        g.keys = keys;
        (g, remaps)
    }

    pub fn finish(self) -> (Points, Attributes) {
        use rayon::prelude::*;
        let mut attrs = Attributes::new();
        for arr in self.src_attrs.iter() {
            if arr.scalar_type() == ScalarType::String {
                continue;
            }
            let nc = arr.ncomp;
            let out: Vec<f64> = self
                .recipe
                .par_iter()
                .with_min_len(4096)
                .flat_map_iter(|&(i, j, t)| {
                    let t = t as f64;
                    (0..nc).map(move |c| {
                        let a = arr.get(i as usize, c);
                        if t == 0.0 {
                            a
                        } else {
                            a + t * (arr.get(j as usize, c) - a)
                        }
                    })
                })
                .collect();
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

/// Cells are processed in chunks of this many (fixed, so results do not depend on the number
/// of threads).
pub const CHUNK: usize = 16384;

/// Run `body` over every visible cell of an explicit dataset in parallel chunks, each with its own
/// state from `make`; the states come back in cell order.
pub fn par_cell_chunks<T: Send>(
    ds: &DataSet,
    make: impl Fn() -> T + Sync,
    body: impl Fn(&mut T, usize, CellType, &[u32], Option<&[Vec<u32>]>) + Sync,
) -> Vec<T> {
    use rayon::prelude::*;
    let n = ds.n_cells();
    let chunks = n.div_ceil(CHUNK).max(1);
    // Cells hidden by the ghost array (AMR cells covered by finer ones, blanked cells) are
    // skipped, as VTK's filters skip invisible cells.
    let hidden = crate::simplex::parts(ds).and_then(|(_, _, cd, _)| aurafx_data::amr::hidden_cells(cd));
    (0..chunks)
        .into_par_iter()
        .map(|c| {
            let mut st = make();
            crate::simplex::for_each_cell_in(ds, c * CHUNK..((c + 1) * CHUNK).min(n), |cid, ty, pts, faces| {
                if hidden.as_ref().is_some_and(|h| h.get(cid).copied().unwrap_or(false)) {
                    return;
                }
                body(&mut st, cid, ty, pts, faces)
            });
            st
        })
        .collect()
}

/// Concatenate chunk cells, renumbering each chunk's points through its remap, in parallel.
pub fn concat_remapped(parts: &[(&Cells, &[u32])]) -> Cells {
    use rayon::prelude::*;
    let n_cells: usize = parts.iter().map(|(c, _)| c.len()).sum();
    let n_conn: usize = parts.iter().map(|(c, _)| c.connectivity.len()).sum();
    let mut conn = vec![0u32; n_conn];
    let mut offsets = vec![0u32; n_cells + 1];
    let mut types = Vec::with_capacity(n_cells);
    for (c, _) in parts {
        types.extend_from_slice(&c.types);
    }
    let mut cs: Vec<(&mut [u32], &mut [u32], u32)> = Vec::with_capacity(parts.len());
    let (mut cr, mut or) = (conn.as_mut_slice(), &mut offsets[1..]);
    let mut base = 0u32;
    for (c, _) in parts {
        let (a, b) = cr.split_at_mut(c.connectivity.len());
        let (x, y) = or.split_at_mut(c.len());
        cs.push((a, x, base));
        base += c.connectivity.len() as u32;
        cr = b;
        or = y;
    }
    parts.par_iter().zip(cs).for_each(|((c, remap), (dst, offs, base))| {
        for (d, &p) in dst.iter_mut().zip(&c.connectivity) {
            *d = remap[p as usize];
        }
        for (d, &o) in offs.iter_mut().zip(c.offsets.iter().skip(1)) {
            *d = base + o;
        }
    });
    Cells { connectivity: conn, offsets, types, polyhedron_faces: None }
}

/// Append `src` to `dst`, renumbering points through `remap`.
pub fn append_remapped(dst: &mut Cells, src: &Cells, remap: &[u32]) {
    dst.connectivity.extend(src.connectivity.iter().map(|&p| remap[p as usize]));
    let base = *dst.offsets.last().unwrap_or(&0);
    if dst.offsets.is_empty() {
        dst.offsets.push(0);
    }
    dst.offsets.extend(src.offsets.iter().skip(1).map(|&o| base + o));
    dst.types.extend_from_slice(&src.types);
}

/// Compact a dataset's points to only those referenced by `conn`, remapping indices in place.
pub fn compact_points(points: &Points, attrs: &Attributes, conn: &mut [u32]) -> (Points, Attributes) {
    let mut map = vec![u32::MAX; points.len()];
    let mut order: Vec<usize> = Vec::new();
    for c in conn.iter_mut() {
        let m = &mut map[*c as usize];
        if *m == u32::MAX {
            *m = order.len() as u32;
            order.push(*c as usize);
        }
        *c = *m;
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
