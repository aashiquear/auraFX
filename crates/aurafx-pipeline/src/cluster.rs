//! Parallel servers: a coordinator spreads each file over worker processes, each holding one
//! piece (see `aurafx_data::pieces`), and combines what they send.
//!
//! Readers and filters that only look at one cell at a time run on every piece; their results
//! are appended (whole data), merged with the faces between pieces removed (surfaces), summed
//! (summaries) or composited by depth (pictures). Filters that look at a cell's neighbours run
//! on every piece too, with a layer of the neighbouring pieces' cells added as ghosts (see
//! `aurafx_filters::ghost_exchange`). Anything else runs on the coordinator on the gathered
//! data.

use super::remote::*;
use crate::session::NodeId;
use anyhow::{anyhow, bail, Result};
use aurafx_data::{DataSet, DataSetInfo};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

/// Filters that work on each piece separately (the result over the whole is the pieces'
/// results together).
pub const PIECE_LOCAL: &[&str] = &[
    "slice",
    "clip",
    "contour",
    "threshold",
    "calculator",
    "warp_by_vector",
    "warp_by_scalar",
    "transform",
    "cell_centers",
    "point_data_to_cell_data",
    "shrink",
    "elevation",
    "cell_size",
    "mesh_quality",
    "tetrahedralize",
    "triangulate",
    "tessellate",
    "extract_block",
    "extract_component",
    "rename_array",
    "glyph",
];

/// Filters that need the cells around each point or cell: they run on every piece with one
/// layer of ghost cells from the neighbouring pieces, then the ghosts are dropped.
pub const GHOSTED: &[&str] = &["gradient", "cell_data_to_point_data", "surface_normals", "curvature", "compute_derivatives"];

/// Ids of the extra nodes a ghosted filter becomes on the workers.
fn ghost_id(id: NodeId, k: u64) -> NodeId {
    (1u64 << 62) | (id << 3) | k
}

/// Nodes of `specs` that can run piece by piece: readers, and piece-local filters of them.
pub fn piece_local(specs: &[NodeSpec]) -> HashSet<NodeId> {
    let mut local = HashSet::new();
    for s in specs {
        let ok = match &s.kind {
            SpecKind::Reader { .. } => true,
            SpecKind::Filter { id } => {
                (PIECE_LOCAL.contains(&id.as_str()) || GHOSTED.contains(&id.as_str()))
                    && s.input.is_some_and(|i| local.contains(&i))
                    && s.extra.iter().all(|e| local.contains(e))
            }
        };
        if ok {
            local.insert(s.id);
        }
    }
    local
}

/// The workers of a parallel server.
pub struct Cluster {
    pub workers: Vec<Arc<RemoteClient>>,
    /// Gathered outputs of piece-local nodes used by other nodes: client id -> fingerprint.
    pub gathered: HashMap<NodeId, u64>,
    /// Summaries of whole outputs, by recipe fingerprint (for automatic parameter values).
    pub infos: parking_lot::Mutex<HashMap<u64, DataSetInfo>>,
    /// Keys of the ghost cells each worker holds, by recipe fingerprint of the input.
    pub ghosts: parking_lot::Mutex<HashMap<u64, Vec<String>>>,
}

/// Filters whose automatic parameter values (centers, ranges, sizes) come from their input.
const AUTO_VALUES: &[&str] = &["slice", "clip", "contour", "threshold", "glyph", "elevation"];

impl Cluster {
    pub fn new(workers: Vec<Arc<RemoteClient>>) -> Self {
        Self { workers, gathered: HashMap::new(), infos: Default::default(), ghosts: Default::default() }
    }

    /// The recipe each worker runs: filters with automatic values get the bounds and ranges of
    /// their whole input (so every piece uses the same plane, iso-values, thresholds and sizes),
    /// and filters that look at neighbours get ghost cells from the other pieces.
    pub fn prepare(&self, specs: &[NodeSpec], time: Option<f64>) -> Result<Vec<Vec<NodeSpec>>> {
        let n = self.workers.len();
        let mut per: Vec<Vec<NodeSpec>> = vec![Vec::new(); n];
        let fp_all = |per: &[Vec<NodeSpec>], input: NodeId| per.iter().fold(input.rotate_left(17), |h, s| h.rotate_left(5) ^ fingerprint(s, time));
        for s in specs {
            let mut s = s.clone();
            let id = match &s.kind {
                SpecKind::Filter { id } => id.clone(),
                _ => String::new(),
            };
            if let (true, Some(input)) = (AUTO_VALUES.contains(&id.as_str()), s.input) {
                let fp = fp_all(&per, input);
                let cached = self.infos.lock().get(&fp).cloned();
                let info = match cached {
                    Some(i) => i,
                    None => {
                        let parts = self.each_k(|k, w| w.compute(per[k].clone(), input, time, Fetch::Info, 0, 1, 0))?;
                        let i = DataSetInfo::merge(&parts.iter().filter_map(|p| p.info.clone()).collect::<Vec<_>>());
                        self.infos.lock().insert(fp, i.clone());
                        i
                    }
                };
                let b = info.bounds;
                let mut ranges = serde_json::Map::new();
                for (tag, arrays) in [("P", &info.point_arrays), ("C", &info.cell_arrays)] {
                    for a in arrays {
                        ranges.insert(format!("{tag}:{}", a.name), serde_json::json!([a.range.0, a.range.1]));
                    }
                }
                let p = &mut s.params.0;
                p.insert("__bounds".into(), aurafx_filters::ParamValue::Floats(vec![b.min[0], b.min[1], b.min[2], b.max[0], b.max[1], b.max[2]]));
                p.insert("__ranges".into(), aurafx_filters::ParamValue::Str(serde_json::Value::Object(ranges).to_string()));
            }
            if let (true, Some(input), true) = (GHOSTED.contains(&id.as_str()), s.input, n > 1) {
                let keys = self.ghost_keys(&per, input, time, fp_all(&per, input))?;
                let (a, g) = (ghost_id(s.id, 1), ghost_id(s.id, 2));
                for (k, sp) in per.iter_mut().enumerate() {
                    let mut ap = aurafx_filters::Params::default();
                    ap.0.insert("layers".into(), aurafx_filters::ParamValue::Int(1));
                    ap.0.insert("key".into(), aurafx_filters::ParamValue::Str(keys[k].clone()));
                    let filter = |f: &str| SpecKind::Filter { id: f.to_string() };
                    sp.push(NodeSpec { id: a, kind: filter("__append_ghosts"), params: ap, input: Some(input), extra: vec![] });
                    sp.push(NodeSpec { id: g, kind: s.kind.clone(), params: s.params.clone(), input: Some(a), extra: s.extra.clone() });
                    sp.push(NodeSpec { id: s.id, kind: filter("__drop_ghosts"), params: Default::default(), input: Some(g), extra: vec![input] });
                }
                continue;
            }
            for sp in per.iter_mut() {
                sp.push(s.clone());
            }
        }
        Ok(per)
    }

    /// Hand every worker the border cells of the other pieces next to it (the output of
    /// `input`); returns the key each worker keeps them under.
    fn ghost_keys(&self, per: &[Vec<NodeSpec>], input: NodeId, time: Option<f64>, fp: u64) -> Result<Vec<String>> {
        if let Some(k) = self.ghosts.lock().get(&fp) {
            return Ok(k.clone());
        }
        let bid = ghost_id(input, 3);
        let borders = self.each_k(|k, w| {
            let mut sp = per[k].clone();
            let mut bp = aurafx_filters::Params::default();
            bp.0.insert("layers".into(), aurafx_filters::ParamValue::Int(1));
            sp.push(NodeSpec { id: bid, kind: SpecKind::Filter { id: "__piece_border".into() }, params: bp, input: Some(input), extra: vec![] });
            Ok(w.compute(sp, bid, time, Fetch::Full, u64::MAX, 1, u64::MAX)?.data)
        })?;
        let boxes: Vec<Option<aurafx_data::Bounds>> = borders.iter().map(|b| (b.n_cells() > 0).then(|| b.bounds())).collect();
        let overlap = |a: &aurafx_data::Bounds, b: &aurafx_data::Bounds| {
            let tol = (a.diagonal() + b.diagonal()) * 1e-9;
            (0..3).all(|i| a.min[i] <= b.max[i] + tol && b.min[i] <= a.max[i] + tol)
        };
        let mut keys = Vec::new();
        for k in 0..borders.len() {
            let mut mb = aurafx_data::MultiBlock::default();
            if let Some(bk) = &boxes[k] {
                for (j, b) in borders.iter().enumerate() {
                    if j != k && boxes[j].as_ref().is_some_and(|bj| overlap(bk, bj)) {
                        mb.blocks.push((format!("piece {j}"), Some(b.clone())));
                    }
                }
            }
            let bytes = encode(&DataSet::MultiBlock(mb))?;
            let key = {
                use std::hash::{Hash, Hasher};
                let mut h = std::collections::hash_map::DefaultHasher::new();
                bytes.hash(&mut h);
                format!("{:016x}-{k}", h.finish())
            };
            self.workers[k].request_with(&Request::Ghosts { key: key.clone() }, &bytes)?;
            keys.push(key);
        }
        let mut g = self.ghosts.lock();
        if g.len() > 256 {
            g.clear();
        }
        g.insert(fp, keys.clone());
        Ok(keys)
    }

    /// Run `f` on every worker at once; results in worker order.
    pub fn each<T: Send>(&self, f: impl Fn(&RemoteClient) -> Result<T> + Sync) -> Result<Vec<T>> {
        self.each_k(|_, w| f(w))
    }

    /// [`Cluster::each`] with the worker's number.
    pub fn each_k<T: Send>(&self, f: impl Fn(usize, &RemoteClient) -> Result<T> + Sync) -> Result<Vec<T>> {
        std::thread::scope(|s| {
            let f = &f;
            let hs: Vec<_> = self.workers.iter().enumerate().map(|(k, w)| s.spawn(move || f(k, w))).collect();
            hs.into_iter()
                .enumerate()
                .map(|(i, h)| h.join().map_err(|_| anyhow!("worker {i} failed")).and_then(|r| r.map_err(|e| e.context(format!("process {}", i + 1)))))
                .collect()
        })
    }

    pub fn can_draw(&self) -> bool {
        // Connect (if needed) to learn whether every worker has a GPU.
        let ok = self.each(|w| {
            if w.info.lock().is_none() {
                w.request(&Request::Hello { protocol: PROTOCOL })?;
            }
            Ok(w.info.lock().as_ref().and_then(|i| i.gpu.clone()).is_some())
        });
        ok.map(|v| v.iter().all(|&b| b)).unwrap_or(false)
    }

    /// The whole output of a piece-local node, gathered from the pieces.
    pub fn gather(&self, specs: &[NodeSpec], target: NodeId, time: Option<f64>) -> Result<(DataSet, Vec<f64>)> {
        let per = self.prepare(specs, time)?;
        let parts = self.each_k(|k, w| w.compute(per[k].clone(), target, time, Fetch::Full, u64::MAX, 1, u64::MAX))?;
        let times = parts.first().map(|p| p.times.clone()).unwrap_or_default();
        Ok((aurafx_data::pieces::merge(parts.into_iter().map(|p| p.data).collect()), times))
    }

    /// Compute a piece-local target over all pieces and answer like a single server would.
    #[allow(clippy::too_many_arguments)]
    pub fn compute_local(
        &self,
        specs: &[NodeSpec],
        target: NodeId,
        time: Option<f64>,
        fetch: Fetch,
        max_bytes: u64,
        level: u32,
        max_surface: u64,
    ) -> Result<(Reply, Vec<u8>)> {
        let t0 = std::time::Instant::now();
        let per = self.prepare(specs, time)?;
        let infos = self.each_k(|k, w| w.compute(per[k].clone(), target, time, Fetch::Info, max_bytes, level, max_surface))?;
        let info = DataSetInfo::merge(&infos.iter().filter_map(|f| f.info.clone()).collect::<Vec<_>>());
        let full_bytes: u64 = infos.iter().map(|f| f.full_bytes).sum();
        let times = infos.first().map(|f| f.times.clone()).unwrap_or_default();
        let surface_useful = !matches!(info.kind.as_str(), "Polygonal Mesh" | "Table");
        let wanted = match fetch {
            Fetch::Info => Payload::None,
            Fetch::Draw if self.can_draw() => Payload::Drawn,
            Fetch::Draw => Payload::Surface,
            Fetch::Full => Payload::Full,
            Fetch::Surface if surface_useful => Payload::Surface,
            Fetch::Surface => Payload::Full,
            Fetch::Auto if full_bytes <= max_bytes || !surface_useful => Payload::Full,
            Fetch::Auto => Payload::Surface,
        };
        let (payload, att) = match wanted {
            Payload::None | Payload::Drawn => (wanted, vec![]),
            Payload::Full => {
                let parts = self.each_k(|k, w| w.compute(per[k].clone(), target, time, Fetch::Full, u64::MAX, level, u64::MAX))?;
                (Payload::Full, encode(&aurafx_data::pieces::merge(parts.into_iter().map(|p| p.data).collect()))?)
            }
            Payload::Surface => {
                let parts = self.each_k(|k, w| w.compute(per[k].clone(), target, time, Fetch::Surface, 0, level, u64::MAX))?;
                // Each piece's cell ids start after the cells of the pieces before it.
                let mut offset = 0;
                let mut pieces = Vec::new();
                for (p, inf) in parts.into_iter().zip(&infos) {
                    if let DataSet::PolyData(pd) = p.data {
                        pieces.push((pd, offset));
                    }
                    offset += inf.info.as_ref().map(|i| i.n_cells).unwrap_or(0);
                }
                let s = DataSet::PolyData(aurafx_filters::surface::merge_piece_surfaces(pieces));
                if fetch == Fetch::Auto && s.n_bytes() as u64 > max_surface && self.can_draw() {
                    (Payload::Drawn, vec![])
                } else {
                    (Payload::Surface, encode(&s)?)
                }
            }
        };
        Ok((Reply::Data { info: Some(info), times, payload, server_ms: t0.elapsed().as_secs_f64() * 1e3, full_bytes, reader_options: Vec::new() }, att))
    }

    /// Pictures of piece-local targets from every worker, composited by depth.
    /// Workers draw without multisampling at twice the size; the pictures are composited and
    /// then reduced 2x2, which smooths edges without seams between pieces.
    pub fn render_local(&self, specs: &[NodeSpec], time: Option<f64>, job: &RenderJob) -> Result<(Vec<u8>, Vec<f32>)> {
        let per = self.prepare(specs, time)?;
        let big = RenderJob { width: job.width * 2, height: job.height * 2, ..job.clone() };
        let images = self.each_k(|k, w| w.render(per[k].clone(), time, big.clone()))?;
        let mut it = images.into_iter();
        let (_, _, mut color, mut depth, _) = it.next().ok_or_else(|| anyhow!("no workers"))?;
        for (_, _, c, d, _) in it {
            composite(&mut color, &mut depth, &c, &d);
        }
        Ok(downsample(&color, &depth, job.width as usize, job.height as usize))
    }
}

/// Halve a picture: colors averaged (in linear light), depth the nearest of the four.
pub fn downsample(color: &[u8], depth: &[f32], w: usize, h: usize) -> (Vec<u8>, Vec<f32>) {
    let to_lin = |c: u8| {
        let c = c as f32 / 255.0;
        if c <= 0.04045 {
            c / 12.92
        } else {
            ((c + 0.055) / 1.055).powf(2.4)
        }
    };
    let to_srgb = |l: f32| {
        let c = if l <= 0.0031308 { l * 12.92 } else { 1.055 * l.powf(1.0 / 2.4) - 0.055 };
        (c * 255.0).round().clamp(0.0, 255.0) as u8
    };
    let (bw, bh) = (2 * w, 2 * h);
    let mut oc = vec![0u8; 4 * w * h];
    let mut od = vec![1.0f32; w * h];
    for y in 0..h {
        for x in 0..w {
            let idx = [(2 * y) * bw + 2 * x, (2 * y) * bw + 2 * x + 1, (2 * y + 1) * bw + 2 * x, (2 * y + 1) * bw + 2 * x + 1];
            if idx[3] >= bw * bh || 4 * idx[3] + 3 >= color.len() {
                continue;
            }
            od[y * w + x] = idx.iter().map(|&i| depth[i]).fold(1.0f32, f32::min);
            for ch in 0..4 {
                let v = if ch == 3 {
                    idx.iter().map(|&i| color[4 * i + 3] as f32 / 255.0).sum::<f32>() / 4.0
                } else {
                    to_srgb(idx.iter().map(|&i| to_lin(color[4 * i + ch])).sum::<f32>() / 4.0) as f32 / 255.0
                };
                oc[4 * (y * w + x) + ch] = (v * 255.0).round().clamp(0.0, 255.0) as u8;
            }
        }
    }
    (oc, od)
}

/// Keep the nearer of two pictures at every pixel.
pub fn composite(color: &mut [u8], depth: &mut [f32], c: &[u8], d: &[f32]) {
    for i in 0..depth.len().min(d.len()) {
        if d[i] < depth[i] {
            depth[i] = d[i];
            color[4 * i..4 * i + 4].copy_from_slice(&c[4 * i..4 * i + 4]);
        }
    }
}

/// Fingerprint of the recipe of a node and the time it is needed at.
pub fn fingerprint(specs: &[NodeSpec], time: Option<f64>) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    postcard::to_allocvec(specs).unwrap_or_default().hash(&mut h);
    time.map(f64::to_bits).hash(&mut h);
    h.finish()
}

pub fn check_workers(n: usize) -> Result<()> {
    if n == 0 {
        bail!("a parallel server needs at least one worker");
    }
    Ok(())
}
