//! Filter and source registry: one place that describes every operation with a name,
//! category, keywords for search, a typed parameter schema and an `apply` function.

use crate::params::*;
use anyhow::{anyhow, Result};
use aurafx_data::*;
use std::sync::{Arc, OnceLock, RwLock};

pub type ApplyFn = fn(Option<&DataSet>, &Params) -> Result<DataSet>;
pub type MultiApplyFn = fn(&[&DataSet], &Params) -> Result<DataSet>;
/// Implementation of a filter added at run time (Python plugins).
pub type CustomFn = Arc<dyn Fn(&[&DataSet], &Params) -> Result<DataSet> + Send + Sync>;

/// A filter or source registered at run time by a plugin.
pub struct Extension {
    pub apply: CustomFn,
    /// The plugin file that defined it.
    pub plugin: String,
}

pub type TemporalFn = fn(&crate::temporal::TemporalCtx, &Params) -> Result<DataSet>;

/// Filters that read their input at several times (Temporal Interpolator, Particle Tracer).
#[derive(Clone, Copy)]
pub struct Temporal {
    pub apply: TemporalFn,
    /// The time steps the output has, given the input's (empty: the output does not change
    /// with time, e.g. Temporal Statistics).
    pub output_times: fn(&Params, &[f64]) -> Vec<f64>,
}

/// Filters that take more than one input. The pipeline input is input 0; the others are the
/// node's extra inputs, chosen in the Properties panel or passed from Python.
#[derive(Clone, Copy)]
pub struct MultiInput {
    /// What the extra inputs are called in the UI ("Inputs", "Destination mesh").
    pub label: &'static str,
    /// Total number of inputs required, including the pipeline input.
    pub min: usize,
    /// Maximum total number of inputs (`None` = any).
    pub max: Option<usize>,
    pub apply: MultiApplyFn,
}

pub struct FilterDef {
    /// Stable snake_case id used by Python, state files and the AI tools.
    pub id: &'static str,
    pub label: &'static str,
    pub category: &'static str,
    pub description: &'static str,
    pub keywords: &'static [&'static str],
    /// `true` for sources (no input).
    pub is_source: bool,
    /// Set for filters defined by plugins; replaces `apply`/`multi`.
    pub ext: Option<Arc<Extension>>,
    pub params: Vec<ParamSpec>,
    pub apply: ApplyFn,
    /// Set for filters with more than one input.
    pub multi: Option<MultiInput>,
    /// Set for filters that need their input at other times than the current one.
    pub temporal: Option<Temporal>,
}

impl std::fmt::Debug for FilterDef {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "FilterDef({})", self.id)
    }
}

impl FilterDef {
    pub fn default_params(&self) -> Params {
        Params::from_specs(&self.params)
    }
    pub fn run(&self, input: Option<&DataSet>, params: &Params) -> Result<DataSet> {
        if !self.is_source && input.is_none() {
            return Err(anyhow!("{} needs an input", self.label));
        }
        // Filters that are not about hyper tree grids work on their visible leaves.
        let leaves;
        let input = match input {
            Some(DataSet::HyperTreeGrid(h)) if !self.id.starts_with("hyper_tree_grid") => {
                leaves = DataSet::UnstructuredGrid(h.leaves_mesh().0);
                Some(&leaves)
            }
            // Cell grids too, as their cells with the values at the corners.
            Some(DataSet::CellGrid(g)) if !self.id.starts_with("cell_grid") => {
                leaves = DataSet::UnstructuredGrid(g.to_unstructured());
                Some(&leaves)
            }
            other => other,
        };
        if let Some(e) = &self.ext {
            return (e.apply)(&input.into_iter().collect::<Vec<_>>(), params);
        }
        (self.apply)(input, params)
    }
    /// Run with every input (the pipeline input first). Single-input filters use the first.
    /// Run a temporal filter with access to its input over time.
    pub fn run_temporal(&self, ctx: &crate::temporal::TemporalCtx, params: &Params) -> Result<DataSet> {
        match &self.temporal {
            Some(t) => (t.apply)(ctx, params),
            None => {
                let d = (ctx.fetch)(ctx.time)?;
                let mut all: Vec<&DataSet> = vec![&d];
                all.extend(ctx.extra.iter().map(|x| &**x));
                self.run_multi(&all, params)
            }
        }
    }

    pub fn run_multi(&self, inputs: &[&DataSet], params: &Params) -> Result<DataSet> {
        let converted: Vec<DataSet>;
        let refs: Vec<&DataSet>;
        let keep_cell_grids = self.id.starts_with("cell_grid");
        let inputs: &[&DataSet] = if inputs.iter().any(|d| matches!(d, DataSet::HyperTreeGrid(_)) || (matches!(d, DataSet::CellGrid(_)) && !keep_cell_grids)) {
            converted = inputs
                .iter()
                .map(|d| match d {
                    DataSet::HyperTreeGrid(h) => DataSet::UnstructuredGrid(h.leaves_mesh().0),
                    DataSet::CellGrid(g) if !keep_cell_grids => DataSet::UnstructuredGrid(g.to_unstructured()),
                    d => (*d).clone(),
                })
                .collect();
            refs = converted.iter().collect();
            &refs
        } else {
            inputs
        };
        if let Some(e) = &self.ext {
            if !self.is_source && inputs.is_empty() {
                return Err(anyhow!("{} needs an input", self.label));
            }
            if let Some(m) = &self.multi {
                if inputs.len() < m.min {
                    return Err(anyhow!("{} needs {} inputs; choose the {} in Properties", self.label, m.min, m.label.to_lowercase()));
                }
            }
            return (e.apply)(inputs, params);
        }
        match &self.multi {
            Some(m) => {
                if inputs.len() < m.min {
                    return Err(anyhow!("{} needs {} inputs; choose the {} in Properties", self.label, m.min, m.label.to_lowercase()));
                }
                if let Some(max) = m.max {
                    if inputs.len() > max {
                        return Err(anyhow!("{} takes at most {max} inputs", self.label));
                    }
                }
                (m.apply)(inputs, params)
            }
            None => self.run(inputs.first().copied(), params),
        }
    }
    /// Score for fuzzy search (higher is better, 0 means no match).
    pub fn search_score(&self, query: &str) -> u32 {
        let q = query.to_lowercase();
        if q.is_empty() {
            return 1;
        }
        let label = self.label.to_lowercase();
        if label == q {
            return 1000;
        }
        if label.starts_with(&q) {
            return 500;
        }
        // A word of the label, or a keyword, starting with the query beats a match inside a
        // word ("iso" finds isosurface before misorientation).
        if label.split_whitespace().any(|w| w.starts_with(&q)) {
            return 400;
        }
        if self.keywords.iter().any(|k| k.split_whitespace().any(|w| w.starts_with(&q))) {
            return 350;
        }
        if label.contains(&q) || self.id.contains(&q) {
            return 300;
        }
        if self.keywords.iter().any(|k| k.contains(&q)) {
            return 200;
        }
        if self.description.to_lowercase().contains(&q) {
            return 100;
        }
        // Subsequence match on the label.
        let mut it = label.chars();
        if q.chars().all(|c| it.any(|l| l == c)) {
            return 50;
        }
        0
    }
}

fn arr_param(key: &str, label: &str) -> ParamSpec {
    ParamSpec::new(
        key,
        label,
        ParamKind::Array { scalar_only: false, vector_only: false },
        ParamValue::Array { assoc: Association::Points, name: String::new() },
    )
}
fn cell_arr(key: &str, label: &str) -> ParamSpec {
    ParamSpec::new(
        key,
        label,
        ParamKind::Array { scalar_only: false, vector_only: false },
        ParamValue::Array { assoc: Association::Cells, name: String::new() },
    )
}
fn vec_param(key: &str, label: &str) -> ParamSpec {
    ParamSpec::new(
        key,
        label,
        ParamKind::Array { scalar_only: false, vector_only: true },
        ParamValue::Array { assoc: Association::Points, name: String::new() },
    )
}
fn f(key: &str, label: &str, default: f64, min: f64, max: f64) -> ParamSpec {
    ParamSpec::new(key, label, ParamKind::Float { min, max, log: false }, ParamValue::Float(default))
}
fn i(key: &str, label: &str, default: i64, min: i64, max: i64) -> ParamSpec {
    ParamSpec::new(key, label, ParamKind::Int { min, max }, ParamValue::Int(default))
}
fn v3(key: &str, label: &str, default: [f64; 3]) -> ParamSpec {
    ParamSpec::new(key, label, ParamKind::Vec3, ParamValue::Vec3(default))
}
fn b(key: &str, label: &str, default: bool) -> ParamSpec {
    ParamSpec::new(key, label, ParamKind::Bool, ParamValue::Bool(default))
}
fn choice(key: &str, label: &str, opts: &[&str], default: &str) -> ParamSpec {
    ParamSpec::new(key, label, ParamKind::Choice(opts.iter().map(|s| s.to_string()).collect()), ParamValue::Str(default.into()))
}
fn s(key: &str, label: &str, default: &str) -> ParamSpec {
    ParamSpec::new(key, label, ParamKind::Str, ParamValue::Str(default.into()))
}

fn harmonics_spec() -> ParamSpec {
    ParamSpec::new("harmonics", "Harmonics", ParamKind::Floats, ParamValue::Floats(crate::texture::DEFAULT_HARMONICS.to_vec()))
        .help("6 numbers per wave: amplitude, temporal frequency, wave vector x y z, phase. Value = sum of amplitude * sin(frequency * time + wave vector . point + phase).")
}

fn harmonics_param(p: &Params) -> Vec<f64> {
    p.floats("harmonics")
}

fn harmonics_grid(p: &Params, t: f64) -> Result<DataSet> {
    let e = p.floats("extent");
    if e.len() != 6 {
        return Err(anyhow!("the extent takes 6 numbers (x min, x max, y min, y max, z min, z max)"));
    }
    let e: [i64; 6] = std::array::from_fn(|k| e[k].round() as i64);
    crate::texture::harmonics_grid(e, &harmonics_param(p), t)
}

fn ds_first_leaf(ds: &DataSet) -> Option<&DataSet> {
    match ds {
        DataSet::MultiBlock(mb) => mb.blocks.iter().filter_map(|(_, b)| b.as_ref()).find_map(ds_first_leaf),
        d => Some(d),
    }
}

/// Component selection: "" or "Magnitude" -> None, "X"/"0" -> Some(0) ...
fn comp(p: &Params) -> Option<usize> {
    match p.str("component").as_str() {
        "X" | "x" | "0" => Some(0),
        "Y" | "y" | "1" => Some(1),
        "Z" | "z" | "2" => Some(2),
        s => s.parse().ok(),
    }
}

/// Pick a sensible default array when the user did not choose one.
pub fn default_array(ds: &DataSet, want_vector: bool) -> Option<(Association, String)> {
    if let DataSet::CellGrid(g) = ds {
        return default_array(&DataSet::UnstructuredGrid(g.to_unstructured()), want_vector);
    }
    let pick = |a: &Attributes| -> Option<String> {
        if want_vector {
            return a.vectors().map(|x| x.name.clone());
        }
        if let Some(s) = a.active(AttributeKind::Scalars) {
            return Some(s.name.clone());
        }
        // Skip geometric helper arrays and constant arrays; prefer 1-component data.
        let skip: Vec<&str> = [AttributeKind::Normals, AttributeKind::TCoords, AttributeKind::GlobalIds, AttributeKind::PedigreeIds]
            .iter()
            .filter_map(|k| a.active.get(k).map(|s| s.as_str()))
            .collect();
        let useful = |x: &&Array| {
            x.scalar_type() != ScalarType::String
                && !skip.contains(&x.name.as_str())
                && !matches!(x.name.as_str(), "Normals" | "TextureCoordinates" | "vtkOriginalPointIds" | "vtkOriginalCellIds" | "vtkGhostType")
                && {
                    let (lo, hi) = x.range(None);
                    hi > lo
                }
        };
        a.iter().filter(useful).find(|x| x.ncomp == 1).or_else(|| a.iter().find(useful)).map(|x| x.name.clone())
    };
    let ds_for = match ds {
        DataSet::MultiBlock(m) => m.leaves().into_iter().next().map(|(_, d)| d),
        _ => Some(ds),
    }?;
    if let Some(n) = ds_for.point_data().and_then(pick) {
        return Some((Association::Points, n));
    }
    ds_for.cell_data().and_then(pick).map(|n| (Association::Cells, n))
}

/// Particle Tracer, Particle Path and Streak Line share everything but their output.
fn particle_def(id: &'static str, label: &'static str, description: &'static str, reinject: i64) -> FilterDef {
    fn run(ctx: &crate::temporal::TemporalCtx, p: &Params, out: crate::temporal::ParticleOutput) -> Result<DataSet> {
        let first = (ctx.fetch)(ctx.input_times.first().copied().unwrap_or(ctx.time))?;
        let (_, vectors) = array_or_default(&first, p, "vectors", true)?;
        let seeds: Vec<[f64; 3]> = match ctx.extra.first() {
            Some(s) => {
                let pts = s.points().ok_or_else(|| anyhow!("the seed input has no points"))?;
                (0..pts.len()).map(|i| pts.get(i)).collect()
            }
            None => {
                let b = first.bounds();
                let nan3 = |k: &str, d: [f64; 3]| {
                    let v = p.vec3(k, [f64::NAN; 3]);
                    if v.iter().any(|x| x.is_nan()) {
                        d
                    } else {
                        v
                    }
                };
                // Default seeds: a line across the middle half of the data's diagonal.
                let (lo, hi) = (b.min, b.max);
                let at = |f: f64| [lo[0] + f * (hi[0] - lo[0]), lo[1] + f * (hi[1] - lo[1]), lo[2] + f * (hi[2] - lo[2])];
                crate::streamlines::line_seeds(nan3("point1", at(0.25)), nan3("point2", at(0.75)), p.i64("seeds", 10).max(1) as usize)
            }
        };
        let start = p.f64("start_time", f64::NAN);
        let opt = crate::temporal::ParticleOptions {
            vectors,
            reinject_every: p.i64("reinject_every", 0).max(0) as usize,
            start_time: if start.is_nan() { f64::NEG_INFINITY } else { start },
            terminal_speed: p.f64("terminal_speed", 1e-12),
        };
        let mut h = std::collections::hash_map::DefaultHasher::new();
        use std::hash::{Hash, Hasher};
        serde_json::to_string(p).unwrap_or_default().hash(&mut h);
        for s in &seeds {
            for c in s {
                c.to_bits().hash(&mut h);
            }
        }
        crate::temporal::particles(ctx, &seeds, &opt, out, h.finish())
    }
    FilterDef {
        id,
        label,
        category: "Temporal",
        description,
        keywords: &["particles", "pathlines", "streaklines", "unsteady flow", "advection", "trajectories", "lagrangian"],
        is_source: false,
        ext: None,
        temporal: Some(Temporal {
            apply: match reinject {
                0 if id == "particle_tracer" => |ctx, p| run(ctx, p, crate::temporal::ParticleOutput::Tracer),
                0 => |ctx, p| run(ctx, p, crate::temporal::ParticleOutput::Path),
                _ => |ctx, p| run(ctx, p, crate::temporal::ParticleOutput::Streak),
            },
            output_times: |_, t| t.to_vec(),
        }),
        multi: Some(MultiInput { label: "Seeds", min: 1, max: Some(2), apply: |_, _| Err(anyhow!("particle filters need time steps")) }),
        params: vec![
            vec_param("vectors", "Vectors"),
            i("reinject_every", "Release again every N steps (0 = once)", reinject, 0, 1_000_000),
            v3("point1", "Seed line start (no seed input)", [f64::NAN; 3]),
            v3("point2", "Seed line end (no seed input)", [f64::NAN; 3]),
            i("seeds", "Seeds on the line", 10, 1, 100_000),
            f("start_time", "Start time", f64::NAN, f64::NEG_INFINITY, f64::INFINITY).advanced(),
            f("terminal_speed", "Terminal speed", 1e-12, 0.0, f64::INFINITY).advanced(),
        ],
        apply: |input, _| Ok(input.unwrap().clone()),
    }
}

const PROGRAMMABLE_FILTER_TEMPLATE: &str = "# inputs[0] is the input, output starts as a copy of it.\n# Arrays are NumPy: output.point_data['T2'] = inputs[0].point_data['T'] * 2\n# Points: output.points (N x 3). Also cell_data, field_data, n_points, n_cells.\n";

const PROGRAMMABLE_SOURCE_TEMPLATE: &str = "# Assign output, e.g. a random point cloud:\nxyz = np.random.default_rng(0).random((1000, 3))\noutput = afx.make_points(xyz)\noutput.point_data['height'] = xyz[:, 2]\n";

fn array_or_default(ds: &DataSet, p: &Params, key: &str, vector: bool) -> Result<(Association, String)> {
    match p.array(key) {
        // A name given without its association (a plain string from Python) may be on the
        // other side: cell data of AMR blocks and most simulation files.
        Some((a, n)) => {
            let has = |a: Association| match a {
                Association::Points => ds.point_data(),
                Association::Cells => ds.cell_data(),
            }
            .is_some_and(|x| x.get(&n).is_some());
            let other = if a == Association::Points { Association::Cells } else { Association::Points };
            Ok(if !has(a) && has(other) { (other, n) } else { (a, n) })
        }
        None => default_array(ds, vector).ok_or_else(|| anyhow!("no suitable array for '{key}'")),
    }
}

/// Apply a per-leaf operation over multi-block inputs.
/// Run `f` on every leaf. A leaf it fails on (e.g. one without the array, like the atoms next
/// to a density volume) is left empty, as ParaView does; the error is reported only when every
/// leaf fails.
fn per_leaf(ds: &DataSet, f: &dyn Fn(&DataSet) -> Result<DataSet>) -> Result<DataSet> {
    match ds {
        DataSet::MultiBlock(mb) => {
            let mut out = MultiBlock { field_data: mb.field_data.clone(), ..Default::default() };
            let (mut ok, mut first_err) = (false, None);
            for (name, b) in &mb.blocks {
                let r = match b {
                    Some(d) => match per_leaf(d, f) {
                        Ok(r) => {
                            ok = true;
                            Some(r)
                        }
                        Err(e) => {
                            first_err.get_or_insert(e);
                            None
                        }
                    },
                    None => None,
                };
                out.blocks.push((name.clone(), r));
            }
            match first_err {
                Some(e) if !ok => Err(e),
                _ => Ok(DataSet::MultiBlock(out)),
            }
        }
        _ => f(ds),
    }
}

/// Bounds of the whole data. On a parallel server each process holds a piece and the
/// coordinator passes the whole data's bounds as `__bounds`, so automatic values (centers,
/// sizes) agree between pieces.
fn whole_bounds(ds: &DataSet, p: &Params) -> Bounds {
    match p.0.get("__bounds") {
        Some(ParamValue::Floats(v)) if v.len() == 6 => Bounds::new([v[0], v[1], v[2]], [v[3], v[4], v[5]]),
        _ => ds.bounds(),
    }
}

/// Range of an array over the whole data (`__ranges` from a parallel server's coordinator,
/// keyed "P:name" / "C:name"), else over this data.
fn whole_range(arr: &Array, comp: Option<usize>, assoc: Association, p: &Params) -> (f64, f64) {
    if comp.is_none() {
        if let Some(ParamValue::Str(json)) = p.0.get("__ranges") {
            let key = format!("{}:{}", if assoc == Association::Points { "P" } else { "C" }, arr.name);
            if let Some(r) = serde_json::from_str::<std::collections::HashMap<String, [f64; 2]>>(json).ok().and_then(|m| m.get(&key).copied()) {
                return (r[0], r[1]);
            }
        }
    }
    arr.range(comp)
}

fn bounds_center_p(ds: &DataSet, p: &Params) -> [f64; 3] {
    let b = whole_bounds(ds, p);
    if b.is_empty() {
        [0.0; 3]
    } else {
        b.center()
    }
}

#[allow(dead_code)]
fn bounds_center(ds: &DataSet) -> [f64; 3] {
    let b = ds.bounds();
    if b.is_empty() {
        [0.0; 3]
    } else {
        b.center()
    }
}

fn build() -> Vec<FilterDef> {
    vec![
        // ---------------- Common ----------------
        FilterDef {
            id: "slice",
            label: "Slice",
            category: "Common",
            description: "Cut the data with a plane or sphere and show the values on the cut surface.",
            keywords: &["cut", "plane", "section", "cross section"],
            is_source: false,
            ext: None,
            temporal: None,
            multi: None,
            params: vec![
                choice("type", "Cut type", &["Plane", "Sphere"], "Plane"),
                v3("origin", "Origin", [f64::NAN; 3]).help("Leave unset to use the center of the data."),
                v3("normal", "Normal", [1.0, 0.0, 0.0]),
                f("radius", "Radius (sphere)", 1.0, 0.0, f64::INFINITY),
                ParamSpec::new("offsets", "Offsets", ParamKind::Floats, ParamValue::Floats(vec![0.0]))
                    .advanced()
                    .help("Additional parallel slices at these distances along the normal."),
            ],
            apply: |input, p| {
                let ds = input.unwrap();
                let origin = p.vec3("origin", [f64::NAN; 3]);
                let origin = if origin.iter().any(|v| v.is_nan()) { bounds_center_p(ds, p) } else { origin };
                let normal = crate::slice::normalize(p.vec3("normal", [1.0, 0.0, 0.0]));
                let radius = p.f64("radius", 1.0);
                let ty = p.str("type");
                let offsets = {
                    let o = p.floats("offsets");
                    if o.is_empty() {
                        vec![0.0]
                    } else {
                        o
                    }
                };
                per_leaf(ds, &|d| {
                    if ty == "Sphere" {
                        return Ok(DataSet::PolyData(crate::slice::slice_sphere(d, origin, radius)?));
                    }
                    if offsets.len() == 1 {
                        let o = offsets[0];
                        let org = [origin[0] + o * normal[0], origin[1] + o * normal[1], origin[2] + o * normal[2]];
                        return Ok(DataSet::PolyData(crate::slice::slice_plane(d, org, normal)?));
                    }
                    let mut mb = MultiBlock::default();
                    for &o in &offsets {
                        let org = [origin[0] + o * normal[0], origin[1] + o * normal[1], origin[2] + o * normal[2]];
                        mb.blocks.push((format!("offset {o}"), Some(DataSet::PolyData(crate::slice::slice_plane(d, org, normal)?))));
                    }
                    Ok(DataSet::MultiBlock(mb))
                })
            },
        },
        FilterDef {
            id: "clip",
            label: "Clip",
            category: "Common",
            description: "Cut away part of the data with a plane, box, sphere or scalar value.",
            keywords: &["cut", "remove", "half", "crop", "cutaway"],
            is_source: false,
            ext: None,
            temporal: None,
            multi: None,
            params: vec![
                choice("type", "Clip type", &["Plane", "Box", "Sphere", "Scalar"], "Plane"),
                v3("origin", "Origin / center", [f64::NAN; 3]),
                v3("normal", "Normal", [1.0, 0.0, 0.0]),
                f("radius", "Radius (sphere)", 1.0, 0.0, f64::INFINITY),
                v3("box_min", "Box min", [f64::NAN; 3]),
                v3("box_max", "Box max", [f64::NAN; 3]),
                arr_param("array", "Scalar (for Scalar clip)"),
                s("component", "Component", ""),
                f("value", "Value (for Scalar clip)", 0.0, f64::NEG_INFINITY, f64::INFINITY),
                b("invert", "Invert", false),
            ],
            apply: |input, p| {
                let ds = input.unwrap();
                let origin = p.vec3("origin", [f64::NAN; 3]);
                let origin = if origin.iter().any(|v| v.is_nan()) { bounds_center_p(ds, p) } else { origin };
                let inv = p.bool("invert", false);
                let ty = p.str("type");
                per_leaf(ds, &|d| {
                    Ok(DataSet::UnstructuredGrid(match ty.as_str() {
                        "Sphere" => crate::clip::clip_sphere(d, origin, p.f64("radius", 1.0), inv)?,
                        "Box" => {
                            let bb = whole_bounds(d, p);
                            let mut lo = p.vec3("box_min", [f64::NAN; 3]);
                            let mut hi = p.vec3("box_max", [f64::NAN; 3]);
                            if lo.iter().any(|v| v.is_nan()) || hi.iter().any(|v| v.is_nan()) {
                                let c = bb.center();
                                let s = bb.size();
                                lo = [c[0] - 0.25 * s[0], c[1] - 0.25 * s[1], c[2] - 0.25 * s[2]];
                                hi = [c[0] + 0.25 * s[0], c[1] + 0.25 * s[1], c[2] + 0.25 * s[2]];
                            }
                            crate::clip::clip_box(d, lo, hi, inv)?
                        }
                        "Scalar" => {
                            let (a, n) = array_or_default(d, p, "array", false)?;
                            crate::clip::clip_scalar(d, a, &n, comp(p), p.f64("value", 0.0), inv)?
                        }
                        _ => crate::clip::clip_plane(d, origin, p.vec3("normal", [1.0, 0.0, 0.0]), inv)?,
                    }))
                })
            },
        },
        FilterDef {
            id: "contour",
            label: "Contour",
            category: "Common",
            description: "Isosurfaces (3D) or isolines (2D) where a scalar equals given values.",
            keywords: &["isosurface", "isoline", "level set", "marching cubes", "iso"],
            is_source: false,
            ext: None,
            temporal: None,
            multi: None,
            params: vec![
                arr_param("array", "Contour by"),
                s("component", "Component", ""),
                ParamSpec::new("values", "Values", ParamKind::Floats, ParamValue::Floats(vec![])).help("Leave empty for the midpoint of the data range."),
                i("count", "Auto count (when values empty)", 1, 1, 100).advanced(),
            ],
            apply: |input, p| {
                let ds = input.unwrap();
                // A cell array of AMR data is contoured on its dual grid: no cracks between levels.
                if aurafx_data::amr::is_amr(ds) {
                    if let Some(leaf) = ds_first_leaf(ds) {
                        if let Ok((Association::Cells, n)) = array_or_default(leaf, p, "array", false) {
                            if let Some(def) = find("amr_contour") {
                                let mut q = def.default_params();
                                q.set("array", ParamValue::Array { assoc: Association::Cells, name: n });
                                for k in ["component", "values"] {
                                    if let Some(v) = p.get(k) {
                                        q.set(k, v.clone());
                                    }
                                }
                                return def.run(Some(ds), &q);
                            }
                        }
                    }
                }
                per_leaf(ds, &|d| {
                    let (a, n) = array_or_default(d, p, "array", false)?;
                    let mut values = p.floats("values");
                    if values.is_empty() {
                        let attrs = if a == Association::Points { d.point_data() } else { d.cell_data() };
                        let arr = attrs.and_then(|x| x.get(&n)).ok_or_else(|| anyhow!("no array {n}"))?;
                        let (lo, hi) = whole_range(arr, comp(p), a, p);
                        let k = p.i64("count", 1).max(1) as usize;
                        values = (1..=k).map(|j| lo + (hi - lo) * j as f64 / (k + 1) as f64).collect();
                    }
                    Ok(DataSet::PolyData(crate::contour::contour(d, a, &n, comp(p), &values)?))
                })
            },
        },
        FilterDef {
            id: "threshold",
            label: "Threshold",
            category: "Common",
            description: "Keep only cells whose values fall within a range.",
            keywords: &["filter", "range", "between", "select", "cutoff"],
            is_source: false,
            ext: None,
            temporal: None,
            multi: None,
            params: vec![
                arr_param("array", "Scalars"),
                s("component", "Component", ""),
                f("lower", "Lower", f64::NAN, f64::NEG_INFINITY, f64::INFINITY),
                f("upper", "Upper", f64::NAN, f64::NEG_INFINITY, f64::INFINITY),
                choice("method", "Point rule", &["All points", "Any point"], "All points").advanced(),
                b("invert", "Invert", false),
            ],
            apply: |input, p| {
                let ds = input.unwrap();
                per_leaf(ds, &|d| {
                    let (a, n) = array_or_default(d, p, "array", false)?;
                    let attrs = if a == Association::Points { d.point_data() } else { d.cell_data() };
                    let arr = attrs.and_then(|x| x.get(&n)).ok_or_else(|| anyhow!("no array {n}"))?;
                    let (lo0, hi0) = whole_range(arr, comp(p), a, p);
                    let mut lo = p.f64("lower", f64::NAN);
                    let mut hi = p.f64("upper", f64::NAN);
                    if lo.is_nan() {
                        lo = lo0 + 0.25 * (hi0 - lo0);
                    }
                    if hi.is_nan() {
                        hi = hi0;
                    }
                    let mode = if p.str("method") == "Any point" { crate::threshold::PointMode::Any } else { crate::threshold::PointMode::All };
                    Ok(DataSet::UnstructuredGrid(crate::threshold::threshold(d, a, &n, comp(p), lo, hi, mode, p.bool("invert", false))?))
                })
            },
        },
        FilterDef {
            id: "calculator",
            label: "Calculator",
            category: "Common",
            description: "Compute a new array from an expression of existing arrays and coordinates.",
            keywords: &["expression", "formula", "math", "compute", "derived"],
            is_source: false,
            ext: None,
            temporal: None,
            multi: None,
            params: vec![
                choice("attribute", "Attribute", &["Points", "Cells"], "Points"),
                ParamSpec::new("expression", "Expression", ParamKind::Expression, ParamValue::Str("coordsX".into()))
                    .help("e.g. mag(velocity), sqrt(x^2+y^2), if(T > 300, 1, 0)"),
                s("result", "Result name", "Result"),
            ],
            apply: |input, p| {
                let assoc = if p.str("attribute") == "Cells" { Association::Cells } else { Association::Points };
                let expr = p.str("expression");
                let name = {
                    let n = p.str("result");
                    if n.is_empty() {
                        "Result".to_string()
                    } else {
                        n
                    }
                };
                per_leaf(input.unwrap(), &|d| crate::calculator::calculator(d, assoc, &expr, &name))
            },
        },
        FilterDef {
            id: "warp_by_vector",
            label: "Warp By Vector",
            category: "Common",
            description: "Displace points by a vector field (e.g. displacement) times a scale factor.",
            keywords: &["deform", "displacement", "displace", "deformed shape"],
            is_source: false,
            ext: None,
            temporal: None,
            multi: None,
            params: vec![vec_param("vectors", "Vectors"), f("scale", "Scale factor", 1.0, f64::NEG_INFINITY, f64::INFINITY)],
            apply: |input, p| {
                per_leaf(input.unwrap(), &|d| {
                    let (_, n) = array_or_default(d, p, "vectors", true)?;
                    crate::warp::warp_by_vector(d, &n, p.f64("scale", 1.0))
                })
            },
        },
        FilterDef {
            id: "warp_by_scalar",
            label: "Warp By Scalar",
            category: "Common",
            description: "Displace points along a direction (or normals) by a scalar value.",
            keywords: &["height", "elevation", "carpet plot", "deform"],
            is_source: false,
            ext: None,
            temporal: None,
            multi: None,
            params: vec![
                arr_param("array", "Scalars"),
                f("scale", "Scale factor", 1.0, f64::NEG_INFINITY, f64::INFINITY),
                v3("normal", "Direction", [0.0, 0.0, 1.0]),
                b("use_normals", "Use point normals", false),
            ],
            apply: |input, p| {
                per_leaf(input.unwrap(), &|d| {
                    let (_, n) = array_or_default(d, p, "array", false)?;
                    crate::warp::warp_by_scalar(d, &n, p.f64("scale", 1.0), p.vec3("normal", [0.0, 0.0, 1.0]), p.bool("use_normals", false))
                })
            },
        },
        FilterDef {
            id: "glyph",
            label: "Glyph",
            category: "Common",
            description: "Draw arrows, spheres or cubes at points, oriented and scaled by arrays.",
            keywords: &["arrows", "vectors", "quiver", "hedgehog", "markers"],
            is_source: false,
            ext: None,
            temporal: None,
            multi: None,
            params: vec![
                choice("shape", "Shape", &["Arrow", "Line", "Sphere", "Cube"], "Arrow"),
                vec_param("orient", "Orientation"),
                arr_param("scale_array", "Scale by"),
                f("scale", "Scale factor", f64::NAN, 0.0, f64::INFINITY).help("Leave unset for 5% of the data size."),
                i("max_glyphs", "Maximum glyphs", 5000, 1, 10_000_000),
                i("resolution", "Resolution", 12, 3, 64).advanced(),
            ],
            apply: |input, p| {
                per_leaf(input.unwrap(), &|d| {
                    let orient =
                        p.array("orient").map(|(_, n)| n).or_else(|| default_array(d, true).filter(|(a, _)| *a == Association::Points).map(|(_, n)| n));
                    let scale_array = p.array("scale_array").map(|(_, n)| n);
                    let mut sf = p.f64("scale", f64::NAN);
                    if sf.is_nan() {
                        let diag = whole_bounds(d, p).diagonal().max(1e-12);
                        let maxmag = orient
                            .as_ref()
                            .and_then(|n| d.point_data().and_then(|a| a.get(n)))
                            .map(|a| whole_range(a, None, Association::Points, p).1)
                            .filter(|v| *v > 0.0)
                            .unwrap_or(1.0);
                        sf = 0.05 * diag / if scale_array.is_some() || orient.is_some() { maxmag } else { 1.0 };
                    }
                    let opt = crate::glyph::GlyphOptions {
                        shape: crate::glyph::GlyphShape::parse(&p.str("shape")),
                        orient_array: orient,
                        scale_array,
                        scale_factor: sf,
                        resolution: p.i64("resolution", 12) as usize,
                        stride: 1,
                        max_glyphs: p.i64("max_glyphs", 5000) as usize,
                    };
                    Ok(DataSet::PolyData(crate::glyph::glyph(d, &opt)?))
                })
            },
        },
        FilterDef {
            id: "plot_over_line",
            label: "Plot Over Line",
            category: "Data",
            description: "Sample values along a straight line; shows a chart of every array versus distance.",
            keywords: &["line plot", "profile", "probe line", "graph", "sample"],
            is_source: false,
            ext: None,
            temporal: None,
            multi: None,
            params: vec![
                v3("point1", "Point 1", [f64::NAN; 3]).help("Unset: from the minimum corner of the data bounds."),
                v3("point2", "Point 2", [f64::NAN; 3]).help("Unset: to the maximum corner of the data bounds."),
                i("resolution", "Resolution", 1000, 1, 1_000_000),
            ],
            apply: |input, p| {
                let ds = input.unwrap();
                let b = ds.bounds();
                let p1 = p.vec3("point1", [f64::NAN; 3]);
                let p2 = p.vec3("point2", [f64::NAN; 3]);
                let p1 = if p1.iter().any(|v| v.is_nan()) { b.min } else { p1 };
                let p2 = if p2.iter().any(|v| v.is_nan()) { b.max } else { p2 };
                Ok(DataSet::PolyData(crate::probe::plot_over_line(ds, p1, p2, p.i64("resolution", 1000) as usize)?))
            },
        },
        FilterDef {
            id: "probe_location",
            label: "Probe Location",
            category: "Data",
            description: "Interpolated values at a single point.",
            keywords: &["probe", "point value", "query", "sample point"],
            is_source: false,
            ext: None,
            temporal: None,
            multi: None,
            params: vec![v3("location", "Location", [f64::NAN; 3])],
            apply: |input, p| {
                let ds = input.unwrap();
                let loc = p.vec3("location", [f64::NAN; 3]);
                let loc = if loc.iter().any(|v| v.is_nan()) { bounds_center(ds) } else { loc };
                let attrs = crate::probe::probe_points(ds, &[loc])?;
                let mut pd = PolyData::from_points(Points::new_f64(loc.to_vec()));
                pd.point_data = attrs;
                Ok(DataSet::PolyData(pd))
            },
        },
        FilterDef {
            id: "resample_to_image",
            label: "Resample To Image",
            category: "Data",
            description: "Interpolate any mesh onto a uniform grid (for volume-style analysis or export).",
            keywords: &["regrid", "voxelize", "interpolate", "uniform grid"],
            is_source: false,
            ext: None,
            temporal: None,
            multi: None,
            params: vec![i("nx", "X samples", 50, 1, 4096), i("ny", "Y samples", 50, 1, 4096), i("nz", "Z samples", 50, 1, 4096)],
            apply: |input, p| {
                let dims = [p.i64("nx", 50) as usize, p.i64("ny", 50) as usize, p.i64("nz", 50) as usize];
                Ok(DataSet::ImageData(crate::probe::resample_to_image(input.unwrap(), dims)?))
            },
        },
        FilterDef {
            id: "histogram",
            label: "Histogram",
            category: "Data",
            description: "Distribution of an array as a table of bins (shown in the Charts panel).",
            keywords: &["distribution", "bins", "frequency", "pdf"],
            is_source: false,
            ext: None,
            temporal: None,
            multi: None,
            params: vec![
                arr_param("array", "Array"),
                s("component", "Component", ""),
                i("bins", "Bins", 64, 1, 100_000),
                f("lower", "Range min", f64::NAN, f64::NEG_INFINITY, f64::INFINITY).advanced(),
                f("upper", "Range max", f64::NAN, f64::NEG_INFINITY, f64::INFINITY).advanced(),
            ],
            apply: |input, p| {
                let ds = input.unwrap();
                let (a, n) = array_or_default(ds, p, "array", false)?;
                let (lo, hi) = (p.f64("lower", f64::NAN), p.f64("upper", f64::NAN));
                let range = if lo.is_finite() && hi.is_finite() { Some((lo, hi)) } else { None };
                Ok(DataSet::Table(crate::analysis::histogram(ds, a, &n, comp(p), p.i64("bins", 64) as usize, range)?))
            },
        },
        FilterDef {
            id: "descriptive_statistics",
            label: "Descriptive Statistics",
            category: "Data",
            description: "Count, min, max, mean, standard deviation, median and sum of every array.",
            keywords: &["stats", "mean", "average", "std", "summary"],
            is_source: false,
            ext: None,
            temporal: None,
            multi: None,
            params: vec![],
            apply: |input, _| Ok(DataSet::Table(crate::analysis::descriptive_statistics(input.unwrap())?)),
        },
        FilterDef {
            id: "integrate_variables",
            label: "Integrate Variables",
            category: "Data",
            description: "Volume (or area/length) and the integral of every array over the domain.",
            keywords: &["integral", "total", "volume", "area", "mass", "sum"],
            is_source: false,
            ext: None,
            temporal: None,
            multi: None,
            params: vec![],
            apply: |input, _| Ok(DataSet::Table(crate::analysis::integrate_variables(input.unwrap())?)),
        },
        FilterDef {
            id: "gradient",
            label: "Gradient",
            category: "Data",
            description: "Spatial gradient of a point scalar (adds <name>_gradient).",
            keywords: &["derivative", "slope", "grad", "flux"],
            is_source: false,
            ext: None,
            temporal: None,
            multi: None,
            params: vec![ParamSpec::new(
                "array",
                "Scalar",
                ParamKind::Array { scalar_only: true, vector_only: false },
                ParamValue::Array { assoc: Association::Points, name: String::new() },
            )],
            apply: |input, p| {
                per_leaf(input.unwrap(), &|d| {
                    let (_, n) = array_or_default(d, p, "array", false)?;
                    crate::analysis::gradient(d, &n)
                })
            },
        },
        FilterDef {
            id: "extract_selection",
            label: "Extract Selection",
            category: "Data",
            description: "Extract the selected cells or points as a new dataset.",
            keywords: &["selection", "extract", "subset", "pick", "cells", "points"],
            is_source: false,
            ext: None,
            temporal: None,
            multi: None,
            params: vec![
                choice("attribute", "Selection of", &["Cells", "Points"], "Cells"),
                ParamSpec::new("ids", "Ids", ParamKind::Floats, ParamValue::Floats(vec![]))
                    .help("Flattened cell or point indices (filled in by the 3D view's selection tools)."),
            ],
            apply: |input, p| {
                let assoc = if p.str("attribute") == "Points" { Association::Points } else { Association::Cells };
                let ids: Vec<usize> = p.floats("ids").into_iter().filter(|v| *v >= 0.0).map(|v| v as usize).collect();
                crate::selection::extract_ids(input.unwrap(), assoc, &ids)
            },
        },
        FilterDef {
            id: "find_data",
            label: "Find Data",
            category: "Data",
            description: "Extract cells or points matching a condition, e.g. Temp > 600 & Pres < 0.1.",
            keywords: &["query", "search", "where", "condition", "select"],
            is_source: false,
            ext: None,
            temporal: None,
            multi: None,
            params: vec![
                choice("attribute", "Find", &["Cells", "Points"], "Cells"),
                ParamSpec::new("expression", "Condition", ParamKind::Expression, ParamValue::Str(String::new()))
                    .help("Calculator syntax; non-zero means selected. Examples: T > 300, mag(V) >= 2 & coordsZ < 0"),
            ],
            apply: |input, p| {
                let assoc = if p.str("attribute") == "Points" { Association::Points } else { Association::Cells };
                let expr = p.str("expression");
                let ds = input.unwrap();
                if expr.trim().is_empty() {
                    return Err(anyhow!("enter a condition, e.g. Temp > 600"));
                }
                let ids = crate::selection::find_ids(ds, assoc, &expr)?;
                crate::selection::extract_ids(ds, assoc, &ids)
            },
        },
        // ---------------- Geometry ----------------
        FilterDef {
            id: "extract_surface",
            label: "Extract Surface",
            category: "Geometry",
            description: "Outer surface of a volume mesh as polygons.",
            keywords: &["skin", "boundary", "faces", "exterior"],
            is_source: false,
            ext: None,
            temporal: None,
            multi: None,
            params: vec![i("nonlinear_subdivision_level", "Nonlinear subdivision level", 1, 0, 6)
                .help("Curved (higher-order) cells: 0 uses only their corners, 1 goes through their nodes, each further level halves the pieces.")
                .advanced()],
            apply: |input, p| {
                let level = p.i64("nonlinear_subdivision_level", 1).clamp(0, 6) as u32;
                // Block by block, as ParaView does, so each block keeps its cell data.
                per_leaf(input.unwrap(), &|d| Ok(DataSet::PolyData(crate::surface::extract_surface_level(d, level)?)))
            },
        },
        FilterDef {
            id: "tessellate",
            label: "Tessellate",
            category: "Geometry",
            description: "Replace curved (higher-order Lagrange, Bezier and quadratic) cells by linear cells that follow their shape.",
            keywords: &["higher order", "lagrange", "bezier", "quadratic", "curved", "linearize", "subdivide", "nonlinear"],
            is_source: false,
            ext: None,
            temporal: None,
            multi: None,
            params: vec![i("level", "Subdivision level", 2, 1, 6)
                .help("1 places one linear cell between neighbouring nodes; each further level halves them (8x the cells in 3D).")],
            apply: |input, p| crate::higher_order::tessellate(input.unwrap(), p.i64("level", 2).clamp(1, 6) as u32),
        },
        FilterDef {
            id: "outline",
            label: "Outline",
            category: "Geometry",
            description: "Bounding box of the data as lines.",
            keywords: &["bounding box", "bbox", "frame"],
            is_source: false,
            ext: None,
            temporal: None,
            multi: None,
            params: vec![],
            apply: |input, _| Ok(DataSet::PolyData(crate::outline::outline(input.unwrap()))),
        },
        FilterDef {
            id: "extract_edges",
            label: "Extract Edges",
            category: "Geometry",
            description: "All unique cell edges as lines.",
            keywords: &["wireframe", "edges", "mesh lines"],
            is_source: false,
            ext: None,
            temporal: None,
            multi: None,
            params: vec![],
            apply: |input, _| per_leaf(input.unwrap(), &|d| Ok(DataSet::PolyData(crate::edges::extract_edges(d)?))),
        },
        FilterDef {
            id: "feature_edges",
            label: "Feature Edges",
            category: "Geometry",
            description: "Boundary and sharp edges of a surface.",
            keywords: &["silhouette", "crease", "sharp", "boundary edges"],
            is_source: false,
            ext: None,
            temporal: None,
            multi: None,
            params: vec![f("angle", "Feature angle", 30.0, 0.0, 180.0), b("boundary", "Boundary edges", true), b("sharp", "Sharp edges", true)],
            apply: |input, p| {
                per_leaf(input.unwrap(), &|d| {
                    let surf = match d {
                        DataSet::PolyData(pd) => pd.clone(),
                        _ => crate::surface::extract_surface(d)?,
                    };
                    Ok(DataSet::PolyData(crate::edges::feature_edges(&surf, p.f64("angle", 30.0), p.bool("boundary", true), p.bool("sharp", true))?))
                })
            },
        },
        FilterDef {
            id: "transform",
            label: "Transform",
            category: "Geometry",
            description: "Translate, rotate and scale the data.",
            keywords: &["move", "rotate", "scale", "translate", "affine"],
            is_source: false,
            ext: None,
            temporal: None,
            multi: None,
            params: vec![v3("translate", "Translate", [0.0; 3]), v3("rotate", "Rotate (deg)", [0.0; 3]), v3("scale", "Scale", [1.0; 3])],
            apply: |input, p| {
                per_leaf(input.unwrap(), &|d| {
                    crate::transform::transform(d, p.vec3("translate", [0.0; 3]), p.vec3("rotate", [0.0; 3]), p.vec3("scale", [1.0; 3]))
                })
            },
        },
        FilterDef {
            id: "cell_centers",
            label: "Cell Centers",
            category: "Geometry",
            description: "A point at the center of every cell, carrying the cell data.",
            keywords: &["centroids", "centers"],
            is_source: false,
            ext: None,
            temporal: None,
            multi: None,
            params: vec![],
            apply: |input, _| per_leaf(input.unwrap(), &|d| Ok(DataSet::PolyData(crate::attributes::cell_centers(d)?))),
        },
        FilterDef {
            id: "merge_blocks",
            label: "Merge Blocks",
            category: "Geometry",
            description: "Combine all blocks of a multi-block dataset into one mesh.",
            keywords: &["append", "combine", "flatten"],
            is_source: false,
            ext: None,
            temporal: None,
            multi: None,
            params: vec![],
            apply: |input, _| match input.unwrap() {
                ds @ DataSet::MultiBlock(_) => Ok(DataSet::UnstructuredGrid(crate::append::append_datasets(&[ds])?)),
                other => Ok(other.clone()),
            },
        },
        FilterDef {
            id: "append_datasets",
            label: "Append Datasets",
            category: "Common",
            description: "Combine several datasets into one unstructured grid. Arrays present in every input are kept.",
            keywords: &["append", "combine", "merge", "join", "concatenate", "multiple inputs"],
            is_source: false,
            ext: None,
            temporal: None,
            multi: Some(MultiInput {
                label: "Inputs",
                min: 1,
                max: None,
                apply: |inputs, _| Ok(DataSet::UnstructuredGrid(crate::append::append_datasets(inputs)?)),
            }),
            params: vec![],
            apply: |input, _| Ok(DataSet::UnstructuredGrid(crate::append::append_datasets(&[input.unwrap()])?)),
        },
        FilterDef {
            id: "append_geometry",
            label: "Append Geometry",
            category: "Geometry",
            description: "Combine several surface meshes into one polygonal mesh.",
            keywords: &["append", "combine", "merge polydata", "join surfaces"],
            is_source: false,
            ext: None,
            temporal: None,
            multi: Some(MultiInput { label: "Inputs", min: 1, max: None, apply: |inputs, _| Ok(DataSet::PolyData(crate::append::append_geometry(inputs)?)) }),
            params: vec![],
            apply: |input, _| Ok(DataSet::PolyData(crate::append::append_geometry(&[input.unwrap()])?)),
        },
        FilterDef {
            id: "append_attributes",
            label: "Append Attributes",
            category: "Data",
            description: "Copy the arrays of other inputs with the same number of points or cells onto the first input.",
            keywords: &["merge arrays", "combine fields", "add arrays", "join attributes"],
            is_source: false,
            ext: None,
            temporal: None,
            multi: Some(MultiInput { label: "Inputs", min: 1, max: None, apply: |inputs, _| crate::append::append_attributes(inputs) }),
            params: vec![],
            apply: |input, _| Ok(input.unwrap().clone()),
        },
        FilterDef {
            id: "resample_with_dataset",
            label: "Resample With Dataset",
            category: "Data",
            description: "Sample this data's arrays at the points of another mesh (the destination).",
            keywords: &["probe", "interpolate onto", "map fields", "transfer", "sample on mesh", "project"],
            is_source: false,
            ext: None,
            temporal: None,
            multi: Some(MultiInput {
                label: "Destination mesh",
                min: 2,
                max: Some(2),
                apply: |inputs, p| {
                    crate::append::resample_with_dataset(inputs[0], inputs[1], p.bool("pass_point_arrays", false), p.bool("pass_cell_arrays", false))
                },
            }),
            params: vec![b("pass_point_arrays", "Keep destination point arrays", false), b("pass_cell_arrays", "Keep destination cell arrays", false)],
            apply: |_, _| Err(anyhow!("Resample With Dataset needs a destination mesh; choose it in Properties")),
        },
        // ---------------- Temporal ----------------
        FilterDef {
            id: "temporal_interpolator",
            label: "Temporal Interpolator",
            category: "Temporal",
            description: "Blend the two nearest time steps so any time in between can be shown; optionally add regular time steps.",
            keywords: &["interpolate time", "smooth animation", "in-between steps", "resample time"],
            is_source: false,
            ext: None,
            temporal: Some(Temporal {
                apply: |ctx, _| crate::temporal::temporal_interpolator(ctx),
                output_times: |p, t| crate::temporal::interpolator_times(p.f64("interval", 0.0), t),
            }),
            multi: None,
            params: vec![f("interval", "Time step spacing (0 = input steps)", 0.0, 0.0, f64::INFINITY)],
            apply: |input, _| Ok(input.unwrap().clone()),
        },
        FilterDef {
            id: "temporal_shift_scale",
            label: "Temporal Shift Scale",
            category: "Temporal",
            description: "Shift and stretch the time axis: output time = (input time + pre shift) x scale + post shift.",
            keywords: &["time offset", "time scale", "align time", "shift time"],
            is_source: false,
            ext: None,
            temporal: Some(Temporal {
                apply: |ctx, p| crate::temporal::temporal_shift_scale(ctx, p.f64("pre_shift", 0.0), p.f64("scale", 1.0), p.f64("post_shift", 0.0)),
                output_times: |p, t| crate::temporal::shift_scale_times(p.f64("pre_shift", 0.0), p.f64("scale", 1.0), p.f64("post_shift", 0.0), t),
            }),
            multi: None,
            params: vec![
                f("pre_shift", "Pre shift", 0.0, f64::NEG_INFINITY, f64::INFINITY),
                f("scale", "Scale", 1.0, f64::NEG_INFINITY, f64::INFINITY),
                f("post_shift", "Post shift", 0.0, f64::NEG_INFINITY, f64::INFINITY),
            ],
            apply: |input, _| Ok(input.unwrap().clone()),
        },
        FilterDef {
            id: "temporal_array_operator",
            label: "Temporal Array Operator",
            category: "Temporal",
            description: "Combine an array at two time steps (+ - * /), e.g. the change since the previous step.",
            keywords: &["difference over time", "change", "delta", "time derivative", "compare steps"],
            is_source: false,
            ext: None,
            temporal: Some(Temporal {
                apply: |ctx, p| {
                    let now = (ctx.fetch)(ctx.time)?;
                    let (assoc, name) = array_or_default(&now, p, "array", false)?;
                    crate::temporal::temporal_array_operator(
                        ctx,
                        &crate::temporal::ArrayOp {
                            points: assoc == Association::Points,
                            array: &name,
                            op: &p.str("operator"),
                            relative: p.bool("relative", true),
                            shift: p.i64("shift", -1),
                            first: p.i64("first_step", 0),
                            second: p.i64("second_step", 0),
                            suffix: &p.str("suffix"),
                        },
                    )
                },
                output_times: |p, t| if p.bool("relative", true) { t.to_vec() } else { Vec::new() },
            }),
            multi: None,
            params: vec![
                arr_param("array", "Array"),
                choice("operator", "Operator", &["+", "-", "*", "/"], "-"),
                b("relative", "Relative to the current step", true),
                i("shift", "Step shift (relative)", -1, -100_000, 100_000),
                i("first_step", "First step index", 0, 0, 1_000_000),
                i("second_step", "Second step index", 0, 0, 1_000_000),
                s("suffix", "Output name suffix (default _add/_sub/_mul/_div)", "").advanced(),
            ],
            apply: |input, _| Ok(input.unwrap().clone()),
        },
        FilterDef {
            id: "temporal_statistics",
            label: "Temporal Statistics",
            category: "Temporal",
            description: "Average, minimum, maximum and standard deviation of every array over all time steps.",
            keywords: &["time average", "mean over time", "max over time", "std over time"],
            is_source: false,
            ext: None,
            temporal: Some(Temporal { apply: |ctx, _| crate::temporal::temporal_statistics(ctx), output_times: |_, _| Vec::new() }),
            multi: None,
            params: vec![],
            apply: |input, _| Ok(input.unwrap().clone()),
        },
        particle_def("particle_tracer", "Particle Tracer", "Particles released at seed points, carried by a time-varying flow; shows where they are now.", 0),
        particle_def("particle_path", "Particle Path", "The path each particle released at the seeds has followed through a time-varying flow (pathlines).", 0),
        particle_def("streak_line", "Streak Line", "For each seed, a line through all particles it released so far, like dye in a flow.", 1),
        // ---------------- Python ----------------
        FilterDef {
            id: "programmable_filter",
            label: "Programmable Filter",
            category: "Python",
            description: "Run your own Python (NumPy) on the data: read inputs[0], change output (a copy of it).",
            keywords: &["python", "script", "numpy", "custom", "code"],
            is_source: false,
            ext: None,
            temporal: None,
            multi: Some(MultiInput {
                label: "Inputs",
                min: 1,
                max: None,
                apply: |inputs, p| crate::python::run_python(crate::python::PyTask::Filter { script: &p.str("script"), inputs }),
            }),
            params: vec![ParamSpec::new("script", "Script", ParamKind::Code, ParamValue::Str(PROGRAMMABLE_FILTER_TEMPLATE.into()))],
            apply: |input, p| {
                let inputs = [input.unwrap()];
                crate::python::run_python(crate::python::PyTask::Filter { script: &p.str("script"), inputs: &inputs })
            },
        },
        FilterDef {
            id: "programmable_source",
            label: "Programmable Source",
            category: "Sources",
            description: "Make data with your own Python (NumPy): assign output.",
            keywords: &["python", "script", "numpy", "custom", "generate"],
            is_source: true,
            ext: None,
            temporal: None,
            multi: None,
            params: vec![ParamSpec::new("script", "Script", ParamKind::Code, ParamValue::Str(PROGRAMMABLE_SOURCE_TEMPLATE.into()))],
            apply: |_, p| crate::python::run_python(crate::python::PyTask::Source { script: &p.str("script") }),
        },
        FilterDef {
            id: "python_calculator",
            label: "Python Calculator",
            category: "Python",
            description: "A new array from a NumPy expression over the arrays, e.g. T * 2 or np.linalg.norm(velocity, axis=1).",
            keywords: &["numpy", "expression", "formula", "compute array", "python"],
            is_source: false,
            ext: None,
            temporal: None,
            multi: None,
            params: vec![
                ParamSpec::new("expression", "Expression", ParamKind::Expression, ParamValue::Str(String::new())),
                s("result", "Result name", "result"),
                choice("attribute", "Attribute", &["Points", "Cells"], "Points"),
            ],
            apply: |input, p| {
                let input = input.unwrap();
                let expression = p.str("expression");
                if expression.trim().is_empty() {
                    return Ok(input.clone());
                }
                crate::python::run_python(crate::python::PyTask::Calculator {
                    expression: &expression,
                    input,
                    cells: p.str("attribute") == "Cells",
                    result: &p.str("result"),
                })
            },
        },
        // ---------------- Data ----------------
        FilterDef {
            id: "cell_data_to_point_data",
            label: "Cell Data to Point Data",
            category: "Data",
            description: "Average cell values onto the points for smooth coloring.",
            keywords: &["interpolate", "smooth", "nodal"],
            is_source: false,
            ext: None,
            temporal: None,
            multi: None,
            params: vec![b("pass_cell_data", "Pass cell data", false)],
            apply: |input, p| {
                let pass = p.bool("pass_cell_data", false);
                per_leaf(input.unwrap(), &|d| {
                    let mut o = crate::attributes::cell_to_point(d, &[])?;
                    if !pass {
                        if let Some(cd) = o.cell_data_mut() {
                            *cd = Attributes::new();
                        }
                    }
                    Ok(o)
                })
            },
        },
        FilterDef {
            id: "point_data_to_cell_data",
            label: "Point Data to Cell Data",
            category: "Data",
            description: "Average point values onto the cells.",
            keywords: &["elemental", "average"],
            is_source: false,
            ext: None,
            temporal: None,
            multi: None,
            params: vec![b("pass_point_data", "Pass point data", false)],
            apply: |input, p| {
                let pass = p.bool("pass_point_data", false);
                per_leaf(input.unwrap(), &|d| {
                    let mut o = crate::attributes::point_to_cell(d, &[])?;
                    if !pass {
                        if let Some(pd) = o.point_data_mut() {
                            *pd = Attributes::new();
                        }
                    }
                    Ok(o)
                })
            },
        },
        FilterDef {
            id: "stream_tracer",
            label: "Stream Tracer",
            category: "Common",
            description: "Streamlines of a vector field from seeds on a line or in a sphere.",
            keywords: &["streamlines", "flow", "trajectories", "integrate", "velocity", "field lines"],
            is_source: false,
            ext: None,
            temporal: None,
            multi: None,
            params: vec![
                vec_param("vectors", "Vectors"),
                choice("seed_type", "Seeds", &["Line", "Point Cloud"], "Point Cloud"),
                v3("point1", "Line start", [f64::NAN; 3]),
                v3("point2", "Line end", [f64::NAN; 3]),
                v3("center", "Cloud center", [f64::NAN; 3]),
                f("radius", "Cloud radius", f64::NAN, 0.0, f64::INFINITY),
                i("seeds", "Number of seeds", 100, 1, 100_000),
                choice("direction", "Direction", &["Both", "Forward", "Backward"], "Both"),
                i("max_steps", "Max steps", 2000, 1, 1_000_000).advanced(),
                f("step", "Step (fraction of size)", 0.005, 1e-6, 1.0).advanced(),
                f("max_length", "Max length (fraction of size)", 2.0, 0.0, 1000.0).advanced(),
            ],
            apply: |input, p| {
                let ds = input.unwrap();
                let b = ds.bounds();
                let (_, vname) = array_or_default(ds, p, "vectors", true)?;
                let nan3 = |k: &str, d: [f64; 3]| {
                    let v = p.vec3(k, [f64::NAN; 3]);
                    if v.iter().any(|x| x.is_nan()) {
                        d
                    } else {
                        v
                    }
                };
                let n = p.i64("seeds", 100) as usize;
                let seeds = if p.str("seed_type") == "Line" {
                    crate::streamlines::line_seeds(nan3("point1", b.min), nan3("point2", b.max), n)
                } else {
                    let r = p.f64("radius", f64::NAN);
                    crate::streamlines::sphere_seeds(nan3("center", b.center()), if r.is_nan() { 0.1 * b.diagonal() } else { r }, n)
                };
                let opt = crate::streamlines::StreamOptions {
                    vectors: vname,
                    direction: match p.str("direction").as_str() {
                        "Forward" => crate::streamlines::Direction::Forward,
                        "Backward" => crate::streamlines::Direction::Backward,
                        _ => crate::streamlines::Direction::Both,
                    },
                    max_steps: p.i64("max_steps", 2000) as usize,
                    max_length: p.f64("max_length", 2.0) * b.diagonal(),
                    step_fraction: p.f64("step", 0.005),
                    terminal_speed: 1e-12,
                };
                Ok(DataSet::PolyData(crate::streamlines::stream_tracer(ds, &seeds, &opt)?))
            },
        },
        FilterDef {
            id: "tube",
            label: "Tube",
            category: "Geometry",
            description: "Turn lines (e.g. streamlines) into 3D tubes.",
            keywords: &["pipe", "thick lines", "streamtubes", "cylinder"],
            is_source: false,
            ext: None,
            temporal: None,
            multi: None,
            params: vec![
                f("radius", "Radius", f64::NAN, 0.0, f64::INFINITY).help("Unset: 0.5% of the data size."),
                i("sides", "Sides", 12, 3, 64),
                arr_param("scale_by", "Vary radius by").advanced(),
            ],
            apply: |input, p| {
                per_leaf(input.unwrap(), &|d| {
                    let r = p.f64("radius", f64::NAN);
                    let r = if r.is_nan() { 0.005 * d.bounds().diagonal() } else { r };
                    let sb = p.array("scale_by").map(|(_, n)| n);
                    Ok(DataSet::PolyData(crate::geometry::tube(d, r, p.i64("sides", 12) as usize, sb.as_deref())?))
                })
            },
        },
        FilterDef {
            id: "surface_normals",
            label: "Surface Normals",
            category: "Geometry",
            description: "Compute normals of a surface (adds a Normals array).",
            keywords: &["normals", "orientation", "shading"],
            is_source: false,
            ext: None,
            temporal: None,
            multi: None,
            params: vec![b("flip", "Flip normals", false), b("cell_normals", "Also cell normals", false)],
            apply: |input, p| {
                per_leaf(input.unwrap(), &|d| Ok(DataSet::PolyData(crate::geometry::surface_normals(d, p.bool("flip", false), p.bool("cell_normals", false))?)))
            },
        },
        FilterDef {
            id: "clean",
            label: "Clean",
            category: "Geometry",
            description: "Merge duplicate points and remove unused ones.",
            keywords: &["merge points", "duplicate", "weld", "tolerance"],
            is_source: false,
            ext: None,
            temporal: None,
            multi: None,
            params: vec![f("tolerance", "Tolerance", 0.0, 0.0, f64::INFINITY)],
            apply: |input, p| per_leaf(input.unwrap(), &|d| crate::geometry::clean(d, p.f64("tolerance", 0.0))),
        },
        FilterDef {
            id: "connectivity",
            label: "Connectivity",
            category: "Geometry",
            description: "Label connected regions (RegionId) and optionally keep only one.",
            keywords: &["regions", "islands", "components", "fragments", "largest"],
            is_source: false,
            ext: None,
            temporal: None,
            multi: None,
            params: vec![s("extract", "Keep", "all").help("'all', 'largest' or a region number")],
            apply: |input, p| per_leaf(input.unwrap(), &|d| crate::geometry::connectivity(d, &p.str("extract"))),
        },
        FilterDef {
            id: "extract_block",
            label: "Extract Block",
            category: "Geometry",
            description: "Keep some blocks of a multi-block dataset (by name or index).",
            keywords: &["block", "part", "subset", "element block", "material"],
            is_source: false,
            ext: None,
            temporal: None,
            multi: None,
            params: vec![s("blocks", "Blocks", "0").help("Comma-separated names or indices, e.g. 0,2 or Block 1")],
            apply: |input, p| crate::geometry::extract_block(input.unwrap(), &p.str("blocks")),
        },
        FilterDef {
            id: "extract_subset",
            label: "Extract Subset",
            category: "Common",
            description: "Crop an image or structured grid to a box of grid indices, optionally keeping every n-th point.",
            keywords: &["voi", "crop", "subsample", "downsample", "stride", "region", "ijk", "index"],
            is_source: false,
            ext: None,
            temporal: None,
            multi: None,
            params: vec![
                v3("voi_min", "From index (i, j, k)", [f64::NAN; 3]).help("First grid index kept along each axis; leave unset for the start of the grid."),
                v3("voi_max", "To index (i, j, k)", [f64::NAN; 3]).help("Last grid index kept along each axis; leave unset for the end of the grid."),
                v3("rate", "Every n-th point", [1.0, 1.0, 1.0]).help("Sample rate along i, j and k (2 keeps every other point)."),
                b("include_boundary", "Include last index", false).help("Always keep the last index even when the sample rate skips it."),
            ],
            apply: |input, p| {
                let v = |k: &str, d: [f64; 3]| match p.get(k) {
                    Some(ParamValue::Vec3(x)) => *x,
                    _ => d,
                };
                crate::geometry::extract_subset(
                    input.unwrap(),
                    v("voi_min", [f64::NAN; 3]),
                    v("voi_max", [f64::NAN; 3]),
                    v("rate", [1.0; 3]),
                    p.bool("include_boundary", false),
                )
            },
        },
        FilterDef {
            id: "openfoam_patches",
            label: "Boundary Patches (OpenFOAM)",
            category: "Data",
            description: "Show the boundary patches (inlet, outlet, walls, ...) of an OpenFOAM case with their boundary values.",
            keywords: &["openfoam", "foam", "patch", "boundary", "wall", "inlet", "outlet", "mesh regions"],
            is_source: false,
            ext: None,
            temporal: None,
            multi: None,
            params: vec![s("patches", "Patches", "").help("Comma-separated patch names; leave empty for all patches")],
            apply: |input, p| openfoam_patches(input.unwrap(), &p.str("patches")),
        },
        FilterDef {
            id: "shrink",
            label: "Shrink",
            category: "Geometry",
            description: "Shrink cells toward their centers to see the mesh structure.",
            keywords: &["explode", "cells", "mesh"],
            is_source: false,
            ext: None,
            temporal: None,
            multi: None,
            params: vec![f("factor", "Factor", 0.8, 0.0, 1.0)],
            apply: |input, p| per_leaf(input.unwrap(), &|d| Ok(DataSet::UnstructuredGrid(crate::geometry::shrink(d, p.f64("factor", 0.8))?))),
        },
        FilterDef {
            id: "reflect",
            label: "Reflect",
            category: "Geometry",
            description: "Mirror the data across a plane (symmetric models).",
            keywords: &["mirror", "symmetry"],
            is_source: false,
            ext: None,
            temporal: None,
            multi: None,
            params: vec![
                choice("plane", "Plane", &["X", "Y", "Z"], "X"),
                f("position", "Position", 0.0, f64::NEG_INFINITY, f64::INFINITY),
                b("copy_input", "Keep original", true),
            ],
            apply: |input, p| {
                let axis = match p.str("plane").as_str() {
                    "Y" => 1,
                    "Z" => 2,
                    _ => 0,
                };
                per_leaf(input.unwrap(), &|d| crate::geometry::reflect(d, axis, p.f64("position", 0.0), p.bool("copy_input", true)))
            },
        },
        FilterDef {
            id: "elevation",
            label: "Elevation",
            category: "Data",
            description: "Scalar that increases along a direction (height coloring).",
            keywords: &["height", "gradient color", "z value"],
            is_source: false,
            ext: None,
            temporal: None,
            multi: None,
            params: vec![v3("low_point", "Low point", [f64::NAN; 3]), v3("high_point", "High point", [f64::NAN; 3])],
            apply: |input, p| {
                per_leaf(input.unwrap(), &|d| {
                    let b = whole_bounds(d, p);
                    let lo = p.vec3("low_point", [f64::NAN; 3]);
                    let hi = p.vec3("high_point", [f64::NAN; 3]);
                    let lo = if lo.iter().any(|x| x.is_nan()) { [b.center()[0], b.center()[1], b.min[2]] } else { lo };
                    let hi = if hi.iter().any(|x| x.is_nan()) { [b.center()[0], b.center()[1], b.max[2]] } else { hi };
                    crate::geometry::elevation(d, lo, hi, (0.0, 1.0))
                })
            },
        },
        FilterDef {
            id: "texture_map_to_plane",
            label: "Texture Map to Plane",
            category: "Data",
            description: "Texture coordinates from a plane: fitted to the points, or through Origin, Point 1 and Point 2.",
            keywords: &["texture coordinates", "uv", "tcoords", "planar mapping", "image on surface"],
            is_source: false,
            ext: None,
            temporal: None,
            multi: None,
            params: vec![
                b("automatic", "Automatic", true).help("Fit the plane to the points (Origin and Point 1 must stay at 0 0 0)."),
                v3("origin", "Origin", [0.0; 3]).help("Where s and t are 0."),
                v3("point1", "Point 1", [0.0; 3]).help("Where s is 1 (t = 0)."),
                v3("point2", "Point 2", [0.0; 3]).help("Where t is 1 (s = 0)."),
            ],
            apply: |input, p| {
                let (o, p1, p2) = (p.vec3("origin", [0.0; 3]), p.vec3("point1", [0.0; 3]), p.vec3("point2", [0.0; 3]));
                per_leaf(input.unwrap(), &|d| crate::texture::map_to_plane(d, p.bool("automatic", true), o, p1, p2))
            },
        },
        FilterDef {
            id: "texture_map_to_cylinder",
            label: "Texture Map to Cylinder",
            category: "Data",
            description: "Texture coordinates around a cylinder: t along its axis, s around it.",
            keywords: &["texture coordinates", "uv", "tcoords", "cylindrical mapping", "image on surface", "wrap"],
            is_source: false,
            ext: None,
            temporal: None,
            multi: None,
            params: vec![
                b("automatic", "Automatic", true).help("Put the axis along the longest side of the data's oriented bounding box."),
                v3("point1", "Point 1", [0.0; 3]).help("Axis start (t = 0) when Automatic is off."),
                v3("point2", "Point 2", [0.0; 3]).help("Axis end (t = 1) when Automatic is off."),
                b("prevent_seam", "Prevent seam", true).help("s goes 0 to 1 and back around the axis, so the texture shows twice without a seam."),
            ],
            apply: |input, p| {
                let (p1, p2) = (p.vec3("point1", [0.0; 3]), p.vec3("point2", [0.0; 3]));
                per_leaf(input.unwrap(), &|d| crate::texture::map_to_cylinder(d, p.bool("automatic", true), p1, p2, p.bool("prevent_seam", true)))
            },
        },
        FilterDef {
            id: "texture_map_to_sphere",
            label: "Texture Map to Sphere",
            category: "Data",
            description: "Texture coordinates on a sphere around the data: t from pole to pole (along z), s around z.",
            keywords: &["texture coordinates", "uv", "tcoords", "spherical mapping", "globe", "earth texture", "image on surface"],
            is_source: false,
            ext: None,
            temporal: None,
            multi: None,
            params: vec![
                v3("center", "Center", [f64::NAN; 3]).help("Leave empty to use the mean of the points.").advanced(),
                b("prevent_seam", "Prevent seam", true).help("s goes 0 to 1 and back around z, so the texture shows twice without a seam. Turn off for a world map."),
            ],
            apply: |input, p| {
                let c = p.vec3("center", [f64::NAN; 3]);
                let c = if c.iter().any(|x| x.is_nan()) { None } else { Some(c) };
                per_leaf(input.unwrap(), &|d| crate::texture::map_to_sphere(d, c, p.bool("prevent_seam", true)))
            },
        },
        FilterDef {
            id: "perlin_noise",
            label: "Perlin Noise",
            category: "Data",
            description: "Smooth random-looking values at every point (Perlin noise), for textures and test data.",
            keywords: &["noise", "random", "procedural", "turbulence", "texture"],
            is_source: false,
            ext: None,
            temporal: None,
            multi: None,
            params: vec![
                v3("frequency", "Frequency", [1.0; 3]).help("Bumps per unit length along x, y and z."),
                v3("phase", "Phase", [0.0; 3]).help("Shifts the pattern (half a unit per 1)."),
                f("amplitude", "Amplitude", 1.0, f64::NEG_INFINITY, f64::INFINITY),
                s("result", "Result name", "PerlinNoise"),
            ],
            apply: |input, p| {
                let (fr, ph, a, name) = (p.vec3("frequency", [1.0; 3]), p.vec3("phase", [0.0; 3]), p.f64("amplitude", 1.0), p.str("result"));
                per_leaf(input.unwrap(), &|d| crate::texture::perlin_noise(d, fr, ph, a, &name))
            },
        },
        FilterDef {
            id: "spatio_temporal_harmonics",
            label: "Spatio Temporal Harmonics",
            category: "Data",
            description: "A sum of travelling sine waves at every point, changing with the input's time.",
            keywords: &["waves", "sine", "harmonics", "synthetic", "test data", "animation"],
            is_source: false,
            ext: None,
            temporal: Some(Temporal {
                apply: |ctx, p| {
                    let h = harmonics_param(p);
                    let d = (ctx.fetch)(ctx.time)?;
                    let t = if ctx.input_times.is_empty() { 0.0 } else { ctx.time };
                    per_leaf(&d, &|d| crate::texture::harmonics(d, &h, t))
                },
                output_times: |_, t| t.to_vec(),
            }),
            multi: None,
            params: vec![harmonics_spec()],
            apply: |input, p| {
                let h = harmonics_param(p);
                per_leaf(input.unwrap(), &|d| crate::texture::harmonics(d, &h, 0.0))
            },
        },
        FilterDef {
            id: "amr_contour",
            label: "AMR Contour",
            category: "Common",
            description: "Isosurfaces of a cell array of AMR data, without cracks where levels meet (on the dual grid).",
            keywords: &["amr", "amr contour", "cell data contour", "adaptive mesh refinement", "crack-free", "dual"],
            is_source: false,
            ext: None,
            temporal: None,
            multi: None,
            params: vec![
                arr_param("array", "Cell array"),
                s("component", "Component", ""),
                ParamSpec::new("values", "Values", ParamKind::Floats, ParamValue::Floats(vec![])).help("Leave empty for the middle of the data range."),
            ],
            apply: |input, p| {
                let ds = input.unwrap();
                let (_, name) = p.array("array").filter(|(_, n)| !n.is_empty()).or_else(|| {
                    ds_first_leaf(ds).and_then(|l| l.cell_data().and_then(|cd| cd.iter().find(|a| a.name != "vtkGhostType" && a.scalar_type() != ScalarType::String).map(|a| (Association::Cells, a.name.clone()))))
                }).ok_or_else(|| anyhow!("choose a cell array"))?;
                let mut values = p.floats("values");
                if values.is_empty() {
                    let (mut lo, mut hi) = (f64::INFINITY, f64::NEG_INFINITY);
                    if let DataSet::MultiBlock(mb) = ds {
                        for (_, l) in mb.leaves() {
                            if let Some(a) = l.cell_data().and_then(|c| c.get(&name)) {
                                let (a0, a1) = a.range(comp(p));
                                lo = lo.min(a0);
                                hi = hi.max(a1);
                            }
                        }
                    }
                    if lo > hi {
                        return Err(anyhow!("no cell array {name}"));
                    }
                    values = vec![0.5 * (lo + hi)];
                }
                Ok(DataSet::PolyData(crate::amr::amr_contour(ds, &name, comp(p), &values)?))
            },
        },
        FilterDef {
            id: "amr_dual_grid",
            label: "AMR Dual Grid",
            category: "Data",
            description: "AMR data as one mesh joining the cell centers (levels stitched together), cell arrays on its points: for crack-free clips, slices and streamlines.",
            keywords: &["amr", "dual grid", "cell centers", "stitch levels", "adaptive mesh refinement", "dual clip"],
            is_source: false,
            ext: None,
            temporal: None,
            multi: None,
            params: vec![],
            apply: |input, _| Ok(DataSet::UnstructuredGrid(crate::amr::dual_grid(input.unwrap())?)),
        },
        FilterDef {
            id: "resample_amr",
            label: "Resample AMR",
            category: "Data",
            description: "AMR data sampled on a uniform grid: each point takes the values of the finest cell holding it.",
            keywords: &["amr", "resample", "uniform grid", "regrid", "adaptive mesh refinement", "flatten levels"],
            is_source: false,
            ext: None,
            temporal: None,
            multi: None,
            params: vec![
                v3("samples", "Number of samples", [10.0, 10.0, 10.0]),
                b("whole_domain", "Whole domain", true).help("Sample the data's bounds; off to use Min and Max."),
                v3("min", "Min", [0.0; 3]),
                v3("max", "Max", [1.0; 3]),
                b("to_nodes", "Transfer to nodes", true).help("Values at the grid points (point data); off for values at the cell centers (cell data)."),
            ],
            apply: |input, p| {
                let s = p.vec3("samples", [10.0; 3]).map(|v| v.max(1.0) as usize);
                let region = if p.bool("whole_domain", true) { None } else { Some((p.vec3("min", [0.0; 3]), p.vec3("max", [1.0; 3]))) };
                Ok(DataSet::ImageData(crate::amr::resample_amr(input.unwrap(), s, region, p.bool("to_nodes", true))?))
            },
        },
        FilterDef {
            id: "hyper_tree_grid_to_unstructured_grid",
            label: "Hyper Tree Grid To Unstructured Grid",
            category: "Data",
            description: "The visible leaves of a hyper tree grid as an unstructured grid (voxels, pixels or lines), with OriginalIds.",
            keywords: &["htg", "hyper tree grid", "leaves", "convert", "unstructured"],
            is_source: false,
            ext: None,
            temporal: None,
            multi: None,
            params: vec![],
            apply: |input, _| per_leaf(input.unwrap(), &|d| Ok(DataSet::UnstructuredGrid(crate::htg::to_unstructured(d)?))),
        },
        FilterDef {
            id: "hyper_tree_grid_to_dual_grid",
            label: "Hyper Tree Grid To Dual Grid",
            category: "Data",
            description: "A point at the center of every visible leaf of a hyper tree grid, joined into cells across the leaves (gives smooth contours and point data).",
            keywords: &["hyper tree grid", "htg", "dual", "cell centers", "unstructured"],
            is_source: false,
            ext: None,
            temporal: None,
            multi: None,
            params: vec![],
            apply: |input, _| {
                per_leaf(input.unwrap(), &|d| match d {
                    DataSet::HyperTreeGrid(h) => Ok(DataSet::UnstructuredGrid(crate::htg::dual_grid(h)?)),
                    other => Err(anyhow::anyhow!("this filter needs a hyper tree grid, not {}", other.type_name())),
                })
            },
        },
        FilterDef {
            id: "hyper_tree_grid_geometry",
            label: "Hyper Tree Grid Geometry",
            category: "Data",
            description: "The outer surface of a hyper tree grid's visible leaves (and the walls next to masked cells), as polygons with the leaves' values.",
            keywords: &["hyper tree grid", "htg", "surface", "geometry", "boundary"],
            is_source: false,
            ext: None,
            temporal: None,
            multi: None,
            params: vec![b("fill_material", "Fill material", true)
                .help("With material interfaces: draw the material's part of each cell (off: only the interface lines in 2-D; pure cells left out in 1-D and 2-D).")],
            apply: |input, p| {
                per_leaf(input.unwrap(), &|d| match d {
                    DataSet::HyperTreeGrid(h) => Ok(DataSet::PolyData(crate::htg::geometry(h, p.bool("fill_material", true))?)),
                    other => Err(anyhow::anyhow!("this filter needs a hyper tree grid, not {}", other.type_name())),
                })
            },
        },
        FilterDef {
            id: "hyper_tree_grid_feature_edges",
            label: "Hyper Tree Grid Feature Edges",
            category: "Data",
            description: "The edges of a hyper tree grid's visible region: its outline in 2-D, its corners and folds in 3-D, as lines with the leaves' values.",
            keywords: &["hyper tree grid", "htg", "edges", "outline", "feature edges"],
            is_source: false,
            ext: None,
            temporal: None,
            multi: None,
            params: vec![],
            apply: |input, _| {
                per_leaf(input.unwrap(), &|d| match d {
                    DataSet::HyperTreeGrid(h) => Ok(DataSet::PolyData(crate::htg::feature_edges(h)?)),
                    other => Err(anyhow::anyhow!("this filter needs a hyper tree grid, not {}", other.type_name())),
                })
            },
        },
        FilterDef {
            id: "hyper_tree_grid_depth_limiter",
            label: "Hyper Tree Grid Depth Limiter",
            category: "Data",
            description: "Cut the trees of a hyper tree grid at a depth: cells at that depth become leaves.",
            keywords: &["htg", "hyper tree grid", "depth", "level", "coarsen", "limit"],
            is_source: false,
            ext: None,
            temporal: None,
            multi: None,
            params: vec![i("depth", "Depth", 0, 0, 64)],
            apply: |input, p| per_leaf(input.unwrap(), &|d| crate::htg::depth_limiter(d, p.i64("depth", 0).max(0) as usize)),
        },
        FilterDef {
            id: "hyper_tree_grid_generate_fields",
            label: "Hyper Tree Grid Generate Fields",
            category: "Data",
            description: "Cell arrays CellSize, ValidCell and CellCenter, and TotalVisibleVolume in field data.",
            keywords: &["htg", "hyper tree grid", "cell size", "volume", "visible leaves size", "cell center"],
            is_source: false,
            ext: None,
            temporal: None,
            multi: None,
            params: vec![],
            apply: |input, _| per_leaf(input.unwrap(), &crate::htg::generate_fields),
        },
        FilterDef {
            id: "hyper_tree_grid_evaluate_coarse",
            label: "Hyper Tree Grid Evaluate Coarse",
            category: "Data",
            description: "Values of refined (coarse) cells computed from their children: min, max, sum, average...",
            keywords: &["htg", "hyper tree grid", "coarse", "average", "aggregate", "parent values"],
            is_source: false,
            ext: None,
            temporal: None,
            multi: None,
            params: vec![
                choice("operator", "Operator", &crate::htg::COARSE_OPERATORS, "Average"),
                f("default", "Default", 0.0, f64::NEG_INFINITY, f64::INFINITY).help("Value counted for missing children (Average, Splatting Average)."),
            ],
            apply: |input, p| per_leaf(input.unwrap(), &|d| crate::htg::evaluate_coarse(d, &p.str("operator"), p.f64("default", 0.0))),
        },
        FilterDef {
            id: "compute_molecule_bonds",
            label: "Compute Molecule Bonds",
            category: "Data",
            description: "Bonds between atoms closer than the sum of their covalent radii plus a tolerance.",
            keywords: &["molecule", "bonds", "atoms", "chemistry", "covalent", "connect atoms"],
            is_source: false,
            ext: None,
            temporal: None,
            multi: None,
            params: vec![
                arr_param("array", "Atomic numbers").help("Leave empty for atomic_number (or VTK's Atomic Numbers)."),
                f("tolerance", "Tolerance", 0.45, 0.0, f64::INFINITY).help("Absolute: Å added to the radii sum. Relative: factor on each radius."),
                b("absolute", "Absolute tolerance", true),
            ],
            apply: |input, p| {
                let name = p.array("array").map(|(_, n)| n).filter(|n| !n.is_empty());
                per_leaf(input.unwrap(), &|d| crate::molecule::compute_bonds(d, name.as_deref(), p.f64("tolerance", 0.45), p.bool("absolute", true)))
            },
        },
        FilterDef {
            id: "cell_grid_to_unstructured_grid",
            label: "Cell Grid To Unstructured Grid",
            category: "Data",
            description: "A cell grid as ordinary cells, its fields evaluated at the corners (averaged where cells meet).",
            keywords: &["cell grid", "dg", "convert", "unstructured"],
            is_source: false,
            ext: None,
            temporal: None,
            multi: None,
            params: vec![],
            apply: |input, _| per_leaf(input.unwrap(), &|d| Ok(DataSet::UnstructuredGrid(crate::cellgrid::to_unstructured(d)?))),
        },
        FilterDef {
            id: "cell_grid_centers",
            label: "Cell Grid Centers",
            category: "Data",
            description: "A point at the center of every cell of a cell grid, with its fields evaluated there.",
            keywords: &["cell grid", "dg", "centers", "centroids"],
            is_source: false,
            ext: None,
            temporal: None,
            multi: None,
            params: vec![],
            apply: |input, _| per_leaf(input.unwrap(), &|d| Ok(DataSet::PolyData(crate::cellgrid::centers(d)?))),
        },
        FilterDef {
            id: "cell_grid_transform",
            label: "Cell Grid Transform",
            category: "Data",
            description: "Move, rotate and scale a cell grid (its shape, or another 3-D attribute): scale, then rotate about Y, X and Z, then translate.",
            keywords: &["cell grid", "dg", "transform", "translate", "rotate", "scale"],
            is_source: false,
            ext: None,
            temporal: None,
            multi: None,
            params: vec![
                s("attribute", "Attribute", "").help("The attribute to transform; empty for the cells' shape."),
                v3("translate", "Translate", [0.0; 3]),
                v3("rotate", "Rotate (deg)", [0.0; 3]),
                v3("scale", "Scale", [1.0; 3]),
            ],
            apply: |input, p| {
                per_leaf(input.unwrap(), &|d| crate::cellgrid::transform(d, &p.str("attribute"), p.vec3("translate", [0.0; 3]), p.vec3("rotate", [0.0; 3]), p.vec3("scale", [1.0; 3])))
            },
        },
        FilterDef {
            id: "cell_grid_warp",
            label: "Cell Grid Warp",
            category: "Data",
            description: "Move a cell grid's points by a vector attribute shared at its points, times a scale.",
            keywords: &["cell grid", "dg", "warp", "deform", "displacement"],
            is_source: false,
            ext: None,
            temporal: None,
            multi: None,
            params: vec![s("attribute", "Deformation attribute", ""), f("scale", "Scale", 1.0, f64::NEG_INFINITY, f64::INFINITY)],
            apply: |input, p| per_leaf(input.unwrap(), &|d| crate::cellgrid::warp(d, &p.str("attribute"), p.f64("scale", 1.0))),
        },
        FilterDef {
            id: "cell_grid_probe",
            label: "Cell Grid Probe",
            category: "Data",
            description: "Find the cells of a cell grid holding the points of another dataset: each point's cell, parametric coordinates and an attribute's value there.",
            keywords: &["cell grid", "dg", "probe", "sample", "locate", "parametric coordinates"],
            is_source: false,
            ext: None,
            temporal: None,
            multi: Some(MultiInput {
                label: "Probe points",
                min: 2,
                max: Some(2),
                apply: |inputs, p| {
                    // The cell grid is the pipeline input; the points may come either way round.
                    let (grid, pts) = if matches!(inputs[0], DataSet::CellGrid(_)) || !matches!(inputs[1], DataSet::CellGrid(_)) { (inputs[0], inputs[1]) } else { (inputs[1], inputs[0]) };
                    Ok(DataSet::PolyData(crate::cellgrid::probe(grid, pts, &p.str("attribute"))?))
                },
            }),
            params: vec![s("attribute", "Attribute", "").help("The attribute to sample at the points; empty to only find the cells.")],
            apply: |_, _| Err(anyhow!("Cell Grid Probe needs probe points; choose them in Properties")),
        },
        FilterDef {
            id: "cell_grid_compute_sides",
            label: "Cell Grid Compute Sides",
            category: "Data",
            description: "Find the sides of a cell grid's cells (by default the outer faces of its 3-D cells) and draw them instead of the cells.",
            keywords: &["cell grid", "dg", "sides", "faces", "edges", "boundary", "surface", "compute sides"],
            is_source: false,
            ext: None,
            temporal: None,
            multi: None,
            params: vec![
                choice("include", "Sides to include", &["Next lowest dimension", "Surfaces of inputs", "Edges of inputs", "Vertices of inputs", "All sides"], "Next lowest dimension")
                    .help("Next lowest dimension: faces of 3-D cells, edges of 2-D cells and end points of edges. The others: faces, edges or corners of every cell of higher dimension, or all of them."),
                b("omit_renderable", "Omit sides of surfaces and lines", true).help("Cells (and side lists) of 2 or fewer dimensions give no sides."),
                b("preserve_renderable", "Keep surfaces and lines", true).help("Keep drawing the 2-D and 1-D cells and the side lists already there; 3-D cells are always hidden."),
                choice("strategy", "Keep", &["Boundary", "Odd count", "Every side"], "Boundary")
                    .help("Boundary: faces found once (not shared by two cells) and every lower side; Odd count: sides found an odd number of times; Every side: each side once.")
                    .advanced(),
            ],
            apply: |input, p| {
                use crate::cellgrid::{side_flags::*, SidesStrategy};
                let flags = match p.str("include").as_str() {
                    "Surfaces of inputs" => SURFACES_OF_INPUTS,
                    "Edges of inputs" => EDGES_OF_INPUTS,
                    "Vertices of inputs" => VERTICES_OF_INPUTS,
                    "All sides" => ALL_SIDES,
                    _ => NEXT_LOWEST_DIMENSION,
                };
                let strategy = match p.str("strategy").as_str() {
                    "Odd count" => SidesStrategy::Winding,
                    "Every side" => SidesStrategy::AnyOccurrence,
                    _ => SidesStrategy::Boundary,
                };
                let (omit, keep) = (p.bool("omit_renderable", true), p.bool("preserve_renderable", true));
                per_leaf(input.unwrap(), &|d| crate::cellgrid::compute_sides(d, flags, omit, keep, strategy))
            },
        },
        FilterDef {
            id: "convert_to_cell_grid",
            label: "Convert To Cell Grid",
            category: "Data",
            description: "A mesh of linear cells as a cell grid: point data as attributes at the points, cell data as constant attributes.",
            keywords: &["cell grid", "dg", "convert", "discontinuous galerkin"],
            is_source: false,
            ext: None,
            temporal: None,
            multi: None,
            params: vec![],
            apply: |input, _| per_leaf(input.unwrap(), &crate::cellgrid::convert),
        },
        FilterDef {
            id: "cell_grid_elevation",
            label: "Cell Grid Elevation",
            category: "Data",
            description: "A field on a cell grid: the distance along an axis from an origin (or from the origin).",
            keywords: &["cell grid", "dg", "elevation", "height"],
            is_source: false,
            ext: None,
            temporal: None,
            multi: None,
            params: vec![
                s("name", "Name", "Elevation"),
                v3("origin", "Origin", [0.0; 3]),
                v3("axis", "Axis", [0.0, 0.0, 1.0]),
                choice("distance", "Distance", &["Planar distance", "Spherical distance"], "Planar distance"),
            ],
            apply: |input, p| {
                per_leaf(input.unwrap(), &|d| {
                    crate::cellgrid::elevation(d, &p.str("name"), p.vec3("origin", [0.0; 3]), p.vec3("axis", [0.0, 0.0, 1.0]), p.str("distance") == "Spherical distance")
                })
            },
        },
        FilterDef {
            id: "overlapping_amr_level_ids",
            label: "Overlapping AMR Level Ids",
            category: "Data",
            description: "Cell array LevelIdScalars with the refinement level of every cell of AMR data.",
            keywords: &["amr", "level", "refinement level", "adaptive mesh"],
            is_source: false,
            ext: None,
            temporal: None,
            multi: None,
            params: vec![],
            apply: |input, _| crate::geometry::amr_level_ids(input.unwrap()),
        },
        FilterDef {
            id: "cell_size",
            label: "Cell Size",
            category: "Data",
            description: "Volume, area and length of every cell.",
            keywords: &["volume", "area", "length", "measure"],
            is_source: false,
            ext: None,
            temporal: None,
            multi: None,
            params: vec![],
            apply: |input, _| per_leaf(input.unwrap(), &crate::geometry::cell_size),
        },
        FilterDef {
            id: "mesh_quality",
            label: "Mesh Quality",
            category: "Data",
            description: "Edge ratio and minimum angle of every cell, and for hexahedra and tetrahedra the full sets of quality metrics (scaled Jacobian, skew, aspect ratio, dihedral angles...), or a summary table with acceptable ranges.",
            keywords: &["quality", "aspect ratio", "skew", "distortion", "jacobian", "scaled jacobian", "taper", "oddy", "condition", "verdict", "mesh check"],
            is_source: false,
            ext: None,
            temporal: None,
            multi: None,
            params: vec![choice("output", "Output", &["Cell values", "Summary table"], "Cell values")
                .help("Summary table: each hexahedron and tetrahedron metric's minimum, maximum and mean, its acceptable range and how many cells fall outside it.")],
            apply: |input, p| {
                let summary = p.str("output") == "Summary table";
                per_leaf(input.unwrap(), &|d| crate::mesh_gen::mesh_quality(d, summary))
            },
        },
        FilterDef {
            id: "smooth",
            label: "Smooth",
            category: "Geometry",
            description: "Laplacian smoothing of a surface.",
            keywords: &["laplacian", "fair", "denoise"],
            is_source: false,
            ext: None,
            temporal: None,
            multi: None,
            params: vec![i("iterations", "Iterations", 20, 1, 10_000), f("relaxation", "Relaxation", 0.1, 0.0, 1.0)],
            apply: |input, p| {
                per_leaf(input.unwrap(), &|d| Ok(DataSet::PolyData(crate::geometry::smooth(d, p.i64("iterations", 20) as usize, p.f64("relaxation", 0.1))?)))
            },
        },
        FilterDef {
            id: "triangulate",
            label: "Triangulate",
            category: "Geometry",
            description: "Split surface polygons into triangles.",
            keywords: &["triangles", "polygons"],
            is_source: false,
            ext: None,
            temporal: None,
            multi: None,
            params: vec![],
            apply: |input, _| per_leaf(input.unwrap(), &|d| Ok(DataSet::PolyData(crate::geometry::triangulate(d)?))),
        },
        FilterDef {
            id: "tetrahedralize",
            label: "Tetrahedralize",
            category: "Geometry",
            description: "Split every cell into tetrahedra.",
            keywords: &["tets", "simplices"],
            is_source: false,
            ext: None,
            temporal: None,
            multi: None,
            params: vec![],
            apply: |input, _| per_leaf(input.unwrap(), &|d| Ok(DataSet::UnstructuredGrid(crate::geometry::tetrahedralize(d)?))),
        },
        FilterDef {
            id: "mask_points",
            label: "Mask Points",
            category: "Geometry",
            description: "Keep a subset of points (every Nth, at most a maximum count).",
            keywords: &["subsample", "decimate points", "thin"],
            is_source: false,
            ext: None,
            temporal: None,
            multi: None,
            params: vec![i("stride", "Every Nth", 2, 1, 1_000_000), i("max_points", "Maximum points", 5000, 1, 100_000_000)],
            apply: |input, p| {
                per_leaf(input.unwrap(), &|d| {
                    Ok(DataSet::PolyData(crate::geometry::mask_points(d, p.i64("stride", 2) as usize, p.i64("max_points", 5000) as usize)?))
                })
            },
        },
        FilterDef {
            id: "extract_component",
            label: "Extract Component",
            category: "Data",
            description: "Copy one component of a vector/tensor into a scalar array.",
            keywords: &["component", "x", "y", "z"],
            is_source: false,
            ext: None,
            temporal: None,
            multi: None,
            params: vec![vec_param("array", "Array"), i("component", "Component", 0, 0, 8)],
            apply: |input, p| {
                per_leaf(input.unwrap(), &|d| {
                    let (a, n) = array_or_default(d, p, "array", true)?;
                    crate::geometry::extract_component(d, &n, p.i64("component", 0) as usize, a == Association::Cells)
                })
            },
        },
        FilterDef {
            id: "rename_array",
            label: "Rename Array",
            category: "Data",
            description: "Give an array a new name.",
            keywords: &["rename", "label"],
            is_source: false,
            ext: None,
            temporal: None,
            multi: None,
            params: vec![arr_param("array", "Array"), s("new_name", "New name", "renamed")],
            apply: |input, p| {
                per_leaf(input.unwrap(), &|d| {
                    let (_, n) = array_or_default(d, p, "array", false)?;
                    crate::geometry::rename_array(d, &n, &p.str("new_name"))
                })
            },
        },
        FilterDef {
            id: "pass_arrays",
            label: "Pass Arrays",
            category: "Data",
            description: "Keep only the listed arrays (smaller data, faster export).",
            keywords: &["remove arrays", "filter arrays", "keep"],
            is_source: false,
            ext: None,
            temporal: None,
            multi: None,
            params: vec![s("point_arrays", "Point arrays", "").help("Comma-separated names; empty keeps none"), s("cell_arrays", "Cell arrays", "")],
            apply: |input, p| {
                let split = |k: &str| p.str(k).split(',').map(|x| x.trim().to_string()).filter(|x| !x.is_empty()).collect::<Vec<_>>();
                let (pa, ca) = (split("point_arrays"), split("cell_arrays"));
                per_leaf(input.unwrap(), &|d| Ok(crate::attributes::pass_arrays(d, &pa, &ca)))
            },
        },
        // ---------------- Sources ----------------
        FilterDef {
            id: "sphere",
            label: "Sphere",
            category: "Sources",
            description: "A polygonal sphere.",
            keywords: &["ball", "shape"],
            is_source: true,
            ext: None,
            temporal: None,
            multi: None,
            params: vec![
                v3("center", "Center", [0.0; 3]),
                f("radius", "Radius", 0.5, 0.0, f64::INFINITY),
                i("theta", "Theta resolution", 32, 3, 1024),
                i("phi", "Phi resolution", 32, 3, 1024),
            ],
            apply: |_, p| {
                Ok(DataSet::PolyData(crate::sources::sphere(
                    p.vec3("center", [0.0; 3]),
                    p.f64("radius", 0.5),
                    p.i64("theta", 32) as usize,
                    p.i64("phi", 32) as usize,
                )))
            },
        },
        FilterDef {
            id: "cone",
            label: "Cone",
            category: "Sources",
            description: "A polygonal cone.",
            keywords: &["shape"],
            is_source: true,
            ext: None,
            temporal: None,
            multi: None,
            params: vec![
                v3("center", "Center", [0.0; 3]),
                v3("direction", "Direction", [1.0, 0.0, 0.0]),
                f("height", "Height", 1.0, 0.0, f64::INFINITY),
                f("radius", "Radius", 0.5, 0.0, f64::INFINITY),
                i("resolution", "Resolution", 32, 3, 1024),
            ],
            apply: |_, p| {
                Ok(DataSet::PolyData(crate::sources::cone(
                    p.vec3("center", [0.0; 3]),
                    p.f64("height", 1.0),
                    p.f64("radius", 0.5),
                    p.i64("resolution", 32) as usize,
                    p.vec3("direction", [1.0, 0.0, 0.0]),
                )))
            },
        },
        FilterDef {
            id: "cylinder",
            label: "Cylinder",
            category: "Sources",
            description: "A capped cylinder along Y.",
            keywords: &["tube", "shape"],
            is_source: true,
            ext: None,
            temporal: None,
            multi: None,
            params: vec![
                v3("center", "Center", [0.0; 3]),
                f("height", "Height", 1.0, 0.0, f64::INFINITY),
                f("radius", "Radius", 0.5, 0.0, f64::INFINITY),
                i("resolution", "Resolution", 32, 3, 1024),
            ],
            apply: |_, p| {
                Ok(DataSet::PolyData(crate::sources::cylinder(
                    p.vec3("center", [0.0; 3]),
                    p.f64("height", 1.0),
                    p.f64("radius", 0.5),
                    p.i64("resolution", 32) as usize,
                )))
            },
        },
        FilterDef {
            id: "box",
            label: "Box",
            category: "Sources",
            description: "An axis-aligned box.",
            keywords: &["cube", "shape"],
            is_source: true,
            ext: None,
            temporal: None,
            multi: None,
            params: vec![v3("center", "Center", [0.0; 3]), v3("size", "Size", [1.0; 3])],
            apply: |_, p| Ok(DataSet::PolyData(crate::sources::cube(p.vec3("center", [0.0; 3]), p.vec3("size", [1.0; 3])))),
        },
        FilterDef {
            id: "plane",
            label: "Plane",
            category: "Sources",
            description: "A subdivided rectangle.",
            keywords: &["quad", "grid", "shape"],
            is_source: true,
            ext: None,
            temporal: None,
            multi: None,
            params: vec![
                v3("origin", "Origin", [-0.5, -0.5, 0.0]),
                v3("point1", "Point 1", [0.5, -0.5, 0.0]),
                v3("point2", "Point 2", [-0.5, 0.5, 0.0]),
                i("xres", "X resolution", 10, 1, 4096),
                i("yres", "Y resolution", 10, 1, 4096),
            ],
            apply: |_, p| {
                Ok(DataSet::PolyData(crate::sources::plane(
                    p.vec3("origin", [-0.5, -0.5, 0.0]),
                    p.vec3("point1", [0.5, -0.5, 0.0]),
                    p.vec3("point2", [-0.5, 0.5, 0.0]),
                    p.i64("xres", 10) as usize,
                    p.i64("yres", 10) as usize,
                )))
            },
        },
        FilterDef {
            id: "line",
            label: "Line",
            category: "Sources",
            description: "A straight polyline.",
            keywords: &["segment"],
            is_source: true,
            ext: None,
            temporal: None,
            multi: None,
            params: vec![v3("point1", "Point 1", [-0.5, 0.0, 0.0]), v3("point2", "Point 2", [0.5, 0.0, 0.0]), i("resolution", "Resolution", 10, 1, 1_000_000)],
            apply: |_, p| {
                Ok(DataSet::PolyData(crate::sources::line(
                    p.vec3("point1", [-0.5, 0.0, 0.0]),
                    p.vec3("point2", [0.5, 0.0, 0.0]),
                    p.i64("resolution", 10) as usize,
                )))
            },
        },
        FilterDef {
            id: "point_source",
            label: "Point Cloud",
            category: "Sources",
            description: "Random points inside a sphere.",
            keywords: &["random", "points", "seeds"],
            is_source: true,
            ext: None,
            temporal: None,
            multi: None,
            params: vec![
                v3("center", "Center", [0.0; 3]),
                f("radius", "Radius", 0.5, 0.0, f64::INFINITY),
                i("count", "Number of points", 1000, 1, 100_000_000),
                i("seed", "Random seed", 1, 0, i64::MAX).advanced(),
            ],
            apply: |_, p| {
                Ok(DataSet::PolyData(crate::sources::point_source(
                    p.vec3("center", [0.0; 3]),
                    p.f64("radius", 0.5),
                    p.i64("count", 1000) as usize,
                    p.i64("seed", 1) as u64,
                )))
            },
        },
        FilterDef {
            id: "unstructured_cell_types",
            label: "Unstructured Cell Types",
            category: "Sources",
            description: "A block of cells of one type, including quadratic, Lagrange and Bezier cells of any order, for trying filters on them.",
            keywords: &["cell type", "higher order", "lagrange", "bezier", "quadratic", "tetrahedra", "wedge", "pyramid", "test data"],
            is_source: true,
            ext: None,
            temporal: None,
            multi: None,
            params: vec![
                choice("cell_type", "Cell type", &crate::sources::CELL_TYPE_CHOICES.iter().map(|(n, _)| *n).collect::<Vec<_>>(), "Hex"),
                v3("blocks", "Blocks", [5.0, 5.0, 5.0]).help("Number of unit blocks along x, y and z (2D cells use x and y, curves x)."),
                i("order", "Cell order", 3, 1, 10).help("Order of Lagrange and Bezier cells."),
            ],
            apply: |_, p| {
                let name = p.str("cell_type");
                let ty = crate::sources::CELL_TYPE_CHOICES
                    .iter()
                    .find(|(n, _)| n.eq_ignore_ascii_case(&name))
                    .map(|(_, t)| *t)
                    .ok_or_else(|| anyhow!("unknown cell type {name}"))?;
                let b = p.vec3("blocks", [5.0, 5.0, 5.0]);
                let dims = b.map(|v| v.round().clamp(1.0, 200.0) as usize);
                Ok(DataSet::UnstructuredGrid(crate::sources::unstructured_cell_types(ty, dims, p.i64("order", 3).clamp(1, 10) as usize)))
            },
        },
        FilterDef {
            id: "spatio_temporal_harmonics_grid",
            label: "Spatio Temporal Harmonics",
            category: "Sources",
            description: "Image volume of travelling sine waves with its own time steps (animated test data).",
            keywords: &["waves", "sine", "harmonics", "synthetic", "test data", "animation", "time steps", "uniform grid"],
            is_source: true,
            ext: None,
            temporal: Some(Temporal {
                apply: |ctx, p| harmonics_grid(p, ctx.time),
                output_times: |p, _| {
                    let mut t = p.floats("time_steps");
                    t.retain(|x| x.is_finite());
                    t.sort_by(f64::total_cmp);
                    t.dedup();
                    t
                },
            }),
            multi: None,
            params: vec![
                ParamSpec::new("extent", "Whole extent", ParamKind::Floats, ParamValue::Floats(vec![-10.0, 10.0, -10.0, 10.0, -10.0, 10.0]))
                    .help("x min, x max, y min, y max, z min, z max (point indices, spacing 1)."),
                harmonics_spec(),
                ParamSpec::new("time_steps", "Time steps", ParamKind::Floats, ParamValue::Floats(crate::texture::DEFAULT_TIMES.to_vec()))
                    .help("The source's time steps (default: one period of the first wave in 20 steps)."),
            ],
            apply: |_, p| harmonics_grid(p, p.floats("time_steps").first().copied().unwrap_or(0.0)),
        },
        FilterDef {
            id: "amr_gaussian_pulse",
            label: "AMR Gaussian Pulse",
            category: "Sources",
            description: "A small AMR test dataset: a coarse box and two finer boxes holding a Gaussian pulse.",
            keywords: &["amr", "adaptive mesh refinement", "levels", "test data", "overlapping amr"],
            is_source: true,
            ext: None,
            temporal: None,
            multi: None,
            params: vec![
                choice("dimension", "Dimension", &["2D", "3D"], "3D"),
                f("root_spacing", "Root spacing", 0.5, 1e-9, f64::INFINITY),
                choice("refinement_ratio", "Refinement ratio", &["2", "3", "4"], "2"),
                v3("pulse_origin", "Pulse origin", [0.0; 3]),
                v3("pulse_width", "Pulse width", [0.5; 3]),
                f("pulse_amplitude", "Pulse amplitude", 0.5, f64::NEG_INFINITY, f64::INFINITY),
            ],
            apply: |_, p| {
                Ok(crate::sources::amr_gaussian_pulse(
                    if p.str("dimension") == "2D" { 2 } else { 3 },
                    p.f64("root_spacing", 0.5),
                    p.str("refinement_ratio").parse().unwrap_or(2),
                    p.vec3("pulse_origin", [0.0; 3]),
                    p.vec3("pulse_width", [0.5; 3]),
                    p.f64("pulse_amplitude", 0.5),
                ))
            },
        },
        FilterDef {
            id: "hyper_tree_grid",
            label: "Hyper Tree Grid",
            category: "Sources",
            description: "A hyper tree grid (tree-based refinement) from a text descriptor.",
            keywords: &["htg", "hyper tree grid", "octree", "quadtree", "adaptive", "refinement", "test data"],
            is_source: true,
            ext: None,
            temporal: None,
            multi: None,
            params: vec![
                v3("dimensions", "Dimensions", [2.0, 2.0, 2.0]).help("Points of the root grid along x, y, z (1 makes an axis flat)."),
                choice("branch_factor", "Branch factor", &["2", "3"], "2"),
                i("max_depth", "Max depth", 1, 1, 64),
                s("descriptor", "Descriptor", ".").help("Levels separated by |; per level, R = refined and . = leaf for every vertex of all trees in order (spaces are ignored)."),
                v3("grid_scale", "Grid scale", [1.0; 3]),
                b("use_mask", "Use mask", false),
                s("mask", "Mask", "1").help("Same layout as the descriptor: 0 hides a leaf, 1 keeps it."),
            ],
            apply: |_, p| {
                let d = p.vec3("dimensions", [2.0; 3]).map(|v| v.round().max(1.0) as usize);
                let mask = p.str("mask");
                Ok(DataSet::HyperTreeGrid(crate::htg::from_descriptor(
                    d,
                    p.str("branch_factor").parse().unwrap_or(2),
                    p.i64("max_depth", 1).max(1) as usize,
                    &p.str("descriptor"),
                    p.vec3("grid_scale", [1.0; 3]),
                    p.bool("use_mask", false).then_some(mask.as_str()),
                )?))
            },
        },
        FilterDef {
            id: "random_hyper_tree_grid",
            label: "Random Hyper Tree Grid",
            category: "Sources",
            description: "A hyper tree grid refined at random.",
            keywords: &["htg", "hyper tree grid", "random", "octree", "adaptive", "test data"],
            is_source: true,
            ext: None,
            temporal: None,
            multi: None,
            params: vec![
                v3("dimensions", "Dimensions", [6.0, 6.0, 3.0]).help("Points of the root grid along x, y, z (1 makes an axis flat)."),
                ParamSpec::new("bounds", "Output bounds", ParamKind::Floats, ParamValue::Floats(vec![-10.0, 10.0, -10.0, 10.0, -10.0, 10.0])),
                i("seed", "Seed", 0, i64::from(i32::MIN), i64::from(i32::MAX)),
                i("max_depth", "Max depth", 5, 0, 30),
                f("split_fraction", "Split fraction", 0.5, 0.0, 1.0).help("Chance that a cell is refined."),
                f("masked_fraction", "Masked fraction", 0.0, 0.0, 1.0).help("About this share of the volume is hidden (whole subtrees at a time)."),
            ],
            apply: |_, p| {
                let d = p.vec3("dimensions", [6.0, 6.0, 3.0]).map(|v| v.round().max(1.0) as usize);
                let b = p.floats("bounds");
                let b: [f64; 6] = if b.len() == 6 { std::array::from_fn(|k| b[k]) } else { [-10.0, 10.0, -10.0, 10.0, -10.0, 10.0] };
                Ok(DataSet::HyperTreeGrid(crate::htg::random(d, b, p.i64("seed", 0) as i32, p.i64("max_depth", 5).max(0) as usize, p.f64("split_fraction", 0.5), p.f64("masked_fraction", 0.0))?))
            },
        },
        FilterDef {
            id: "cell_grid",
            label: "Cell Grid",
            category: "Sources",
            description: "One cell of a discontinuous-Galerkin cell grid with test fields.",
            keywords: &["cell grid", "dg", "discontinuous galerkin", "hcurl", "hdiv", "hgrad", "test data"],
            is_source: true,
            ext: None,
            temporal: None,
            multi: None,
            params: vec![choice("cell_type", "Cell type", &crate::cellgrid::CELL_TYPES, "Hexahedron")],
            apply: |_, p| crate::cellgrid::source(&p.str("cell_type")),
        },
        FilterDef {
            id: "wavelet",
            label: "Wavelet",
            category: "Sources",
            description: "Synthetic 3D scalar volume, handy for trying filters.",
            keywords: &["test data", "volume", "example", "rtdata"],
            is_source: true,
            ext: None,
            temporal: None,
            multi: None,
            params: vec![
                i("half_size", "Half size", 10, 1, 512),
                f("maximum", "Maximum", 255.0, f64::NEG_INFINITY, f64::INFINITY),
                f("std_dev", "Std. deviation", 0.5, 0.0, f64::INFINITY),
            ],
            apply: |_, p| {
                let h = p.i64("half_size", 10);
                Ok(DataSet::ImageData(crate::sources::wavelet([-h, h, -h, h, -h, h], p.f64("maximum", 255.0), p.f64("std_dev", 0.5))))
            },
        },
        FilterDef {
            id: "orientation_colors",
            label: "Orientation Colors",
            category: "Grains",
            description: "Color crystal orientations by the inverse pole figure key or by their Euler angles (an 8-bit RGB array drawn as its own colors).",
            keywords: &["ipf", "inverse pole figure", "ebsd", "orientation map", "grain", "crystal", "euler colors", "texture"],
            is_source: false,
            ext: None,
            temporal: None,
            multi: None,
            params: vec![
                cell_arr("orientations", "Orientations").help("Euler angles (3 components, radians), quaternions (4, stored x y z w) or orientation matrices (9). Empty: EulerAngles, Quats or OrientationMatrix."),
                choice("key", "Color key", &[crate::crystal::KEY_IPF, crate::crystal::KEY_EULER], crate::crystal::KEY_IPF),
                v3("direction", "Reference direction", [0.0, 0.0, 1.0]).help("The sample direction whose crystal direction is colored (z: the map normal)."),
                choice("structure", "Crystal structure", &crate::crystal::structure_choices(), crate::crystal::FROM_PHASES)
                    .help("From the phases: each point's class comes from its phase and the CrystalStructures field array, as EBSD files give them."),
                cell_arr("phases", "Phases").help("The phase of each point (0: not indexed). Empty: Phases.").advanced(),
                cell_arr("mask", "Mask").help("Points where this array is 0 are black.").advanced(),
                s("name", "Output name", "IPFColor").advanced(),
            ],
            apply: |input, p| {
                per_leaf(input.unwrap(), &|d| {
                    let phases = p.array("phases").map(|x| x.1);
                    let mask = p.array("mask").map(|x| x.1);
                    let name = p.str("name");
                    crate::crystal::orientation_colors(d, p.array("orientations"), &p.str("key"), p.vec3("direction", [0.0, 0.0, 1.0]), &p.str("structure"), phases.as_deref(), mask.as_deref(), if name.is_empty() { "IPFColor" } else { &name })
                })
            },
        },
        FilterDef {
            id: "convert_orientations",
            label: "Convert Orientations",
            category: "Grains",
            description: "Convert orientations between Euler angles, quaternions, orientation matrices, axis-angle, Rodrigues and homochoric vectors, optionally reduced to the fundamental zone.",
            keywords: &["euler", "quaternion", "rodrigues", "homochoric", "axis angle", "orientation matrix", "fundamental zone", "crystal"],
            is_source: false,
            ext: None,
            temporal: None,
            multi: None,
            params: vec![
                cell_arr("orientations", "Orientations"),
                choice("input_type", "Input", &crate::crystal::Repr::LABELS, "Euler angles"),
                choice("output_type", "Output", &crate::crystal::Repr::LABELS, "Quaternions"),
                b("reduce", "Reduce to the fundamental zone", false).help("Replace each orientation by its symmetry equivalent with the smallest rotation angle."),
                choice("structure", "Crystal structure", &crate::crystal::structure_choices(), crate::crystal::FROM_PHASES),
                cell_arr("phases", "Phases").advanced(),
                s("name", "Output name", "").help("Empty: EulerAngles, Quats, OrientationMatrix, AxisAngles, Rodrigues or Homochoric.").advanced(),
            ],
            apply: |input, p| {
                use crate::crystal::Repr;
                let from = Repr::from_label(&p.str("input_type")).ok_or_else(|| anyhow!("unknown input type"))?;
                let to = Repr::from_label(&p.str("output_type")).ok_or_else(|| anyhow!("unknown output type"))?;
                per_leaf(input.unwrap(), &|d| {
                    let phases = p.array("phases").map(|x| x.1);
                    crate::crystal::convert_orientations(d, p.array("orientations"), from, to, &p.str("name"), p.bool("reduce", false), &p.str("structure"), phases.as_deref())
                })
            },
        },
        FilterDef {
            id: "rotate_orientations",
            label: "Rotate Orientations",
            category: "Grains",
            description: "Re-express orientations after turning the sample or crystal reference frame by an angle about an axis (the data does not move).",
            keywords: &["reference frame", "sample frame", "crystal frame", "euler", "rotate", "ebsd"],
            is_source: false,
            ext: None,
            temporal: None,
            multi: None,
            params: vec![
                cell_arr("orientations", "Orientations"),
                choice("frame", "Frame", &[crate::crystal::FRAME_SAMPLE, crate::crystal::FRAME_CRYSTAL], crate::crystal::FRAME_SAMPLE),
                v3("axis", "Axis", [0.0, 0.0, 1.0]),
                f("angle", "Angle (degrees)", 90.0, -360.0, 360.0),
            ],
            apply: |input, p| {
                per_leaf(input.unwrap(), &|d| crate::crystal::rotate_orientations(d, p.array("orientations"), &p.str("frame"), p.vec3("axis", [0.0, 0.0, 1.0]), p.f64("angle", 90.0)))
            },
        },
        FilterDef {
            id: "crop_volume",
            label: "Crop",
            category: "Grains",
            description: "Keep the cells of a voxel volume or EBSD map inside a box (sample coordinates), renumbering the features that remain.",
            keywords: &["crop", "box", "region", "subvolume", "trim", "voxels", "ebsd", "features"],
            is_source: false,
            ext: None,
            temporal: None,
            multi: None,
            params: vec![
                v3("min", "Box minimum", [f64::NAN; 3]).help("Cells whose centres lie at or above these coordinates are kept; leave unset for an open side."),
                v3("max", "Box maximum", [f64::NAN; 3]).help("Cells whose centres lie at or below these coordinates are kept; leave unset for an open side."),
                b("renumber_features", "Renumber features", true).help("Number the remaining feature ids from 1 and keep only their rows of the per-feature field arrays."),
                s("feature_ids", "Feature ids", "FeatureIds").advanced(),
            ],
            apply: |input, p| {
                let v = |k: &str| match p.get(k) {
                    Some(ParamValue::Vec3(x)) => *x,
                    _ => [f64::NAN; 3],
                };
                crate::grain_grid::crop(input.unwrap(), v("min"), v("max"), p.bool("renumber_features", true), &p.str("feature_ids"))
            },
        },
        FilterDef {
            id: "change_resolution",
            label: "Change Resolution",
            category: "Grains",
            description: "Resample a voxel volume or EBSD map onto larger or smaller cells, each taking the value of the old cell at its centre.",
            keywords: &["resample", "resolution", "voxel size", "coarsen", "refine", "spacing", "downsample", "ebsd"],
            is_source: false,
            ext: None,
            temporal: None,
            multi: None,
            params: vec![
                v3("spacing", "New cell size", [1.0, 1.0, 1.0]),
                b("renumber_features", "Renumber features", true).help("Number the remaining feature ids from 1 and keep only their rows of the per-feature field arrays."),
                s("feature_ids", "Feature ids", "FeatureIds").advanced(),
            ],
            apply: |input, p| crate::grain_grid::change_resolution(input.unwrap(), p.vec3("spacing", [1.0; 3]), p.bool("renumber_features", true), &p.str("feature_ids")),
        },
        FilterDef {
            id: "align_sections",
            label: "Align Sections",
            category: "Grains",
            description: "Move the slices of a serial-section volume in x and y so each matches the one below, by orientation or by feature ids, or by given shifts.",
            keywords: &["serial sectioning", "registration", "align", "slices", "shift", "3d ebsd", "stack"],
            is_source: false,
            ext: None,
            temporal: None,
            multi: None,
            params: vec![
                choice("method", "Match by", &[crate::grain_grid::ALIGN_MISORIENTATION, crate::grain_grid::ALIGN_FEATURES, crate::grain_grid::ALIGN_SHIFTS], crate::grain_grid::ALIGN_MISORIENTATION)
                    .help("Misorientation: cells match when their orientations differ by less than the tolerance. Feature ids: cells match when they belong to the same feature. Given shifts: move each slice by the numbers below."),
                f("tolerance", "Tolerance (degrees)", 5.0, 0.0, 180.0),
                i("max_shift", "Largest shift (cells)", 20, 0, 10000).help("The largest move of one slice against the one below, in cells along x and y."),
                cell_arr("mask", "Mask").help("Only cells where this array is not 0 are compared (for example a good-data or sample mask)."),
                cell_arr("orientations", "Orientations").advanced(),
                choice("structure", "Crystal structure", &crate::crystal::structure_choices(), crate::crystal::FROM_PHASES).advanced(),
                cell_arr("phases", "Phases").advanced(),
                s("feature_ids", "Feature ids", "FeatureIds").advanced(),
                ParamSpec::new("shifts", "Given shifts", ParamKind::Floats, ParamValue::Floats(Vec::new()))
                    .help("x and y cells for each slice after the first, against the slice below (Given shifts only).")
                    .advanced(),
            ],
            apply: |input, p| {
                let mask = p.array("mask").map(|x| x.1);
                let phases = p.array("phases").map(|x| x.1);
                crate::grain_grid::align_sections(
                    input.unwrap(),
                    &p.str("method"),
                    p.array("orientations"),
                    p.f64("tolerance", 5.0),
                    &p.str("structure"),
                    phases.as_deref(),
                    &p.str("feature_ids"),
                    mask.as_deref(),
                    p.i64("max_shift", 20),
                    &p.floats("shifts"),
                )
            },
        },
        FilterDef {
            id: "segment_features",
            label: "Segment Features",
            category: "Grains",
            description: "Group neighbouring cells into features (grains): by misorientation, by c-axis alignment or by a scalar, giving FeatureIds.",
            keywords: &["grains", "segmentation", "reconstruct", "feature ids", "misorientation", "grain map", "ebsd", "flood fill"],
            is_source: false,
            ext: None,
            temporal: None,
            multi: None,
            params: vec![
                choice("method", "Group by", &[crate::features::SEG_MISORIENTATION, crate::features::SEG_CAXIS, crate::features::SEG_SCALAR], crate::features::SEG_MISORIENTATION),
                f("tolerance", "Tolerance", 5.0, 0.0, f64::INFINITY).help("Degrees for misorientation and c-axis alignment; a value difference for Scalar."),
                cell_arr("mask", "Mask").help("Only cells where this array is not 0 are grouped (a good-data or sample mask)."),
                cell_arr("scalar", "Scalar").help("The cell array compared by the Scalar method."),
                b("shuffle", "Shuffle ids", false).help("Number the features in a shuffled order, so neighbouring features get unrelated ids (better colors)."),
                cell_arr("orientations", "Orientations").advanced(),
                choice("structure", "Crystal structure", &crate::crystal::structure_choices(), crate::crystal::FROM_PHASES).advanced(),
                cell_arr("phases", "Phases").advanced(),
            ],
            apply: |input, p| {
                let mask = p.array("mask").map(|x| x.1);
                let phases = p.array("phases").map(|x| x.1);
                crate::features::segment(input.unwrap(), &p.str("method"), p.f64("tolerance", 5.0), mask.as_deref(), p.array("orientations"), &p.str("structure"), phases.as_deref(), p.array("scalar"), p.bool("shuffle", false).then_some(1))
            },
        },
        FilterDef {
            id: "fill_bad_data",
            label: "Fill Bad Data",
            category: "Grains",
            description: "Fill small clusters of cells without a feature from the surrounding features (every cell array copied from a neighbour).",
            keywords: &["clean up", "fill", "defects", "holes", "unindexed", "noise", "ebsd"],
            is_source: false,
            ext: None,
            temporal: None,
            multi: None,
            params: vec![i("min_defect", "Smallest kept defect (cells)", 7, 1, 1_000_000_000).help("Clusters of cells without a feature with fewer cells than this are filled."), s("feature_ids", "Feature ids", "FeatureIds").advanced()],
            apply: |input, p| crate::features::fill_bad_data(input.unwrap(), p.i64("min_defect", 7).max(1) as usize, &p.str("feature_ids")),
        },
        FilterDef {
            id: "minimum_size",
            label: "Minimum Size",
            category: "Grains",
            description: "Remove features with fewer cells than a minimum, filling them from their neighbours, and renumber the rest.",
            keywords: &["clean up", "small grains", "minimum size", "remove", "noise"],
            is_source: false,
            ext: None,
            temporal: None,
            multi: None,
            params: vec![i("min_cells", "Fewest cells", 7, 1, 1_000_000_000), s("feature_ids", "Feature ids", "FeatureIds").advanced()],
            apply: |input, p| crate::features::minimum_size(input.unwrap(), p.i64("min_cells", 7).max(1) as usize, &p.str("feature_ids")),
        },
        FilterDef {
            id: "feature_sizes",
            label: "Feature Sizes",
            category: "Grains",
            description: "Cells, volume and equivalent sphere (or circle) diameter of every feature.",
            keywords: &["grain size", "volume", "equivalent diameter", "esd", "statistics"],
            is_source: false,
            ext: None,
            temporal: None,
            multi: None,
            params: vec![s("feature_ids", "Feature ids", "FeatureIds").advanced()],
            apply: |input, p| crate::features::feature_sizes(input.unwrap(), &p.str("feature_ids")),
        },
        FilterDef {
            id: "feature_centroids",
            label: "Feature Centroids",
            category: "Grains",
            description: "The centre (mean of the cell centres) of every feature.",
            keywords: &["centroid", "grain centre", "position", "statistics"],
            is_source: false,
            ext: None,
            temporal: None,
            multi: None,
            params: vec![s("feature_ids", "Feature ids", "FeatureIds").advanced()],
            apply: |input, p| crate::features::feature_centroids(input.unwrap(), &p.str("feature_ids")),
        },
        FilterDef {
            id: "feature_shapes",
            label: "Feature Shapes",
            category: "Grains",
            description: "Axis lengths, aspect ratios, axis directions and Omega3 of every feature, from its moments of inertia.",
            keywords: &["shape", "aspect ratio", "ellipsoid", "axis lengths", "omega3", "elongation", "statistics"],
            is_source: false,
            ext: None,
            temporal: None,
            multi: None,
            params: vec![s("feature_ids", "Feature ids", "FeatureIds").advanced()],
            apply: |input, p| crate::features::feature_shapes(input.unwrap(), &p.str("feature_ids")),
        },
        FilterDef {
            id: "average_orientations",
            label: "Average Orientations",
            category: "Grains",
            description: "The average crystal orientation of every feature (AvgQuats, AvgEulerAngles).",
            keywords: &["mean orientation", "grain orientation", "avg quats", "texture"],
            is_source: false,
            ext: None,
            temporal: None,
            multi: None,
            params: vec![
                s("feature_ids", "Feature ids", "FeatureIds").advanced(),
                cell_arr("orientations", "Orientations").advanced(),
                choice("structure", "Crystal structure", &crate::crystal::structure_choices(), crate::crystal::FROM_PHASES).advanced(),
                cell_arr("phases", "Phases").advanced(),
            ],
            apply: |input, p| {
                let phases = p.array("phases").map(|x| x.1);
                crate::features::average_orientations(input.unwrap(), &p.str("feature_ids"), p.array("orientations"), &p.str("structure"), phases.as_deref())
            },
        },
        FilterDef {
            id: "reference_misorientations",
            label: "Reference Misorientations",
            category: "Grains",
            description: "Each cell's misorientation from its feature's average orientation, and the mean per feature.",
            keywords: &["grain reference orientation deviation", "gros", "misorientation", "deformation", "intragranular"],
            is_source: false,
            ext: None,
            temporal: None,
            multi: None,
            params: vec![
                s("feature_ids", "Feature ids", "FeatureIds").advanced(),
                cell_arr("orientations", "Orientations").advanced(),
                choice("structure", "Crystal structure", &crate::crystal::structure_choices(), crate::crystal::FROM_PHASES).advanced(),
                cell_arr("phases", "Phases").advanced(),
            ],
            apply: |input, p| {
                let phases = p.array("phases").map(|x| x.1);
                crate::features::reference_misorientations(input.unwrap(), &p.str("feature_ids"), p.array("orientations"), &p.str("structure"), phases.as_deref())
            },
        },
        FilterDef {
            id: "kernel_average_misorientation",
            label: "Kernel Average Misorientation",
            category: "Grains",
            description: "Each cell's mean misorientation to the cells of its feature around it (KAM), a measure of local lattice curvature.",
            keywords: &["kam", "local misorientation", "deformation", "dislocation", "strain", "gnd"],
            is_source: false,
            ext: None,
            temporal: None,
            multi: None,
            params: vec![
                v3("kernel", "Kernel (cells each way)", [1.0, 1.0, 1.0]).help("How many cells either side along x, y and z (1 1 1: the 26 neighbours)."),
                s("feature_ids", "Feature ids", "FeatureIds").advanced(),
                cell_arr("orientations", "Orientations").advanced(),
                choice("structure", "Crystal structure", &crate::crystal::structure_choices(), crate::crystal::FROM_PHASES).advanced(),
                cell_arr("phases", "Phases").advanced(),
            ],
            apply: |input, p| {
                let phases = p.array("phases").map(|x| x.1);
                let k = p.vec3("kernel", [1.0; 3]).map(|v| v.round() as i64);
                crate::features::kernel_average_misorientation(input.unwrap(), &p.str("feature_ids"), k, p.array("orientations"), &p.str("structure"), phases.as_deref())
            },
        },
        FilterDef {
            id: "feature_neighbors",
            label: "Feature Neighbors",
            category: "Grains",
            description: "Neighbour counts, features on the surface, boundary cells and the mean misorientation to neighbouring features.",
            keywords: &["neighbors", "boundary cells", "surface features", "grain boundary", "contiguity"],
            is_source: false,
            ext: None,
            temporal: None,
            multi: None,
            params: vec![s("feature_ids", "Feature ids", "FeatureIds").advanced()],
            apply: |input, p| crate::features::feature_neighbors(input.unwrap(), &p.str("feature_ids")),
        },
        FilterDef {
            id: "distance_to_boundaries",
            label: "Distance to Boundaries",
            category: "Grains",
            description: "Each cell's distance to the nearest grain boundary, triple line and quadruple point.",
            keywords: &["euclidean distance map", "distance", "boundary", "triple junction", "quadruple point"],
            is_source: false,
            ext: None,
            temporal: None,
            multi: None,
            params: vec![s("feature_ids", "Feature ids", "FeatureIds").advanced()],
            apply: |input, p| crate::features::distance_to_boundaries(input.unwrap(), &p.str("feature_ids")),
        },
        FilterDef {
            id: "merge_twins",
            label: "Merge Twins",
            category: "Grains",
            description: "Give twin-related neighbouring features (cubic, 60 degrees about <111>) a common parent id.",
            keywords: &["twins", "sigma 3", "annealing twins", "parent grains", "fcc"],
            is_source: false,
            ext: None,
            temporal: None,
            multi: None,
            params: vec![
                f("axis_tolerance", "Axis tolerance (degrees)", 3.0, 0.0, 90.0),
                f("angle_tolerance", "Angle tolerance (degrees)", 2.0, 0.0, 60.0),
                s("feature_ids", "Feature ids", "FeatureIds").advanced(),
            ],
            apply: |input, p| crate::features::merge_twins(input.unwrap(), &p.str("feature_ids"), p.f64("axis_tolerance", 3.0), p.f64("angle_tolerance", 2.0)),
        },
        FilterDef {
            id: "feature_boundaries",
            label: "Feature Boundaries",
            category: "Grains",
            description: "The faces between features as a surface, with the misorientation across them and twin boundaries marked.",
            keywords: &["grain boundaries", "boundary map", "twin boundaries", "interfaces", "misorientation map"],
            is_source: false,
            ext: None,
            temporal: None,
            multi: None,
            params: vec![
                b("outer", "Include the outer surface", false).help("Also the faces against cells without a feature and the edge of the volume."),
                f("axis_tolerance", "Twin axis tolerance (degrees)", 3.0, 0.0, 90.0).advanced(),
                f("angle_tolerance", "Twin angle tolerance (degrees)", 2.0, 0.0, 60.0).advanced(),
                s("feature_ids", "Feature ids", "FeatureIds").advanced(),
                cell_arr("orientations", "Orientations").advanced(),
                choice("structure", "Crystal structure", &crate::crystal::structure_choices(), crate::crystal::FROM_PHASES).advanced(),
                cell_arr("phases", "Phases").advanced(),
            ],
            apply: |input, p| {
                let phases = p.array("phases").map(|x| x.1);
                crate::features::feature_boundaries(input.unwrap(), &p.str("feature_ids"), p.bool("outer", false), p.f64("axis_tolerance", 3.0), p.f64("angle_tolerance", 2.0), p.array("orientations"), &p.str("structure"), phases.as_deref())
            },
        },
        FilterDef {
            id: "feature_table",
            label: "Feature Table",
            category: "Grains",
            description: "The per-feature results as a table, one row per feature, for the spreadsheet, charts and CSV files.",
            keywords: &["table", "statistics", "csv", "spreadsheet", "grain list", "histogram"],
            is_source: false,
            ext: None,
            temporal: None,
            multi: None,
            params: vec![s("feature_ids", "Feature ids", "FeatureIds").advanced()],
            apply: |input, p| crate::features::feature_table(input.unwrap(), &p.str("feature_ids")),
        },
        FilterDef {
            id: "feature_colors",
            label: "Feature Colors",
            category: "Grains",
            description: "A distinct color for every feature (FeatureColors, drawn as its own colors).",
            keywords: &["grain map", "random colors", "feature map", "label colors"],
            is_source: false,
            ext: None,
            temporal: None,
            multi: None,
            params: vec![s("feature_ids", "Feature ids", "FeatureIds").advanced()],
            apply: |input, p| crate::features::feature_colors(input.unwrap(), &p.str("feature_ids")),
        },
        FilterDef {
            id: "pole_figure",
            label: "Pole Figure",
            category: "Grains",
            description: "Pole figures of crystal directions (densities in multiples of random) as disks, one per family, sample X right and Y up.",
            keywords: &["texture", "pole figure", "pf", "mrd", "stereographic", "equal area", "ebsd", "crystallographic texture", "100 110 111"],
            is_source: false,
            ext: None,
            temporal: None,
            multi: None,
            params: {
                let mut v = vec![
                    s("families", "Directions", "100, 110, 111").help("Crystal directions, like 100, 110, -111 or 1 1 2, separated by commas; every symmetry equivalent is included."),
                    choice("projection", "Projection", &[crate::crystal_texture::STEREOGRAPHIC, crate::crystal_texture::EQUAL_AREA], crate::crystal_texture::STEREOGRAPHIC),
                    f("sigma", "Smoothing (degrees)", 7.5, 0.5, 45.0).help("Width of the Gaussian kernel."),
                    i("resolution", "Resolution", 40, 4, 400).advanced(),
                    f("bin", "Bin size (degrees)", 2.0, 0.25, 10.0).advanced(),
                ];
                v.extend(texture_selection_params(60_000));
                v
            },
            apply: |input, p| {
                let (st, ph, mk) = (p.str("structure"), p.array("phases").map(|x| x.1), p.array("mask").map(|x| x.1));
                let sel = texture_selection(p, &st, ph.as_deref(), mk.as_deref(), 60_000);
                crate::crystal_texture::pole_figures(input.unwrap(), &sel, &p.str("families"), &p.str("projection"), p.f64("sigma", 7.5), p.f64("bin", 2.0), p.i64("resolution", 40).max(4) as usize)
            },
        },
        FilterDef {
            id: "inverse_pole_figure",
            label: "Inverse Pole Figure",
            category: "Grains",
            description: "Inverse pole figures: the density (multiples of random) of the crystal directions along sample X, Y or Z, on the standard triangle of the crystal class.",
            keywords: &["texture", "ipf density", "inverse pole figure", "mrd", "fiber texture", "standard triangle", "ebsd"],
            is_source: false,
            ext: None,
            temporal: None,
            multi: None,
            params: {
                let mut v = vec![
                    choice("directions", "Sample directions", &["X, Y and Z", "X", "Y", "Z"], "X, Y and Z"),
                    choice("projection", "Projection", &[crate::crystal_texture::STEREOGRAPHIC, crate::crystal_texture::EQUAL_AREA], crate::crystal_texture::STEREOGRAPHIC),
                    f("sigma", "Smoothing (degrees)", 5.0, 0.5, 45.0).help("Width of the Gaussian kernel."),
                    i("resolution", "Resolution", 40, 4, 400).advanced(),
                    f("bin", "Bin size (degrees)", 1.0, 0.25, 10.0).advanced(),
                ];
                v.extend(texture_selection_params(60_000));
                v
            },
            apply: |input, p| {
                let (st, ph, mk) = (p.str("structure"), p.array("phases").map(|x| x.1), p.array("mask").map(|x| x.1));
                let sel = texture_selection(p, &st, ph.as_deref(), mk.as_deref(), 60_000);
                crate::crystal_texture::inverse_pole_figures(input.unwrap(), &sel, &p.str("directions"), &p.str("projection"), p.f64("sigma", 5.0), p.f64("bin", 1.0), p.i64("resolution", 40).max(4) as usize)
            },
        },
        FilterDef {
            id: "odf_sections",
            label: "ODF Sections",
            category: "Grains",
            description: "The orientation distribution function on sections of constant phi2 (phi1 across, Phi down, degrees), with the texture index and the ODF maximum.",
            keywords: &["texture", "odf", "orientation distribution function", "euler space", "texture index", "texture strength", "phi2 sections", "mrd"],
            is_source: false,
            ext: None,
            temporal: None,
            multi: None,
            params: {
                let mut v = vec![
                    s("sections", "Sections (phi2, degrees)", "0, 45, 65"),
                    f("step", "Step (degrees)", 5.0, 0.5, 30.0),
                    f("sigma", "Smoothing (degrees)", 7.5, 1.0, 45.0).help("Width of the Gaussian kernel on the disorientation angle."),
                    f("index_step", "Texture index step (degrees)", 5.0, 1.0, 30.0).advanced(),
                ];
                v.extend(texture_selection_params(10_000));
                v
            },
            apply: |input, p| {
                let (st, ph, mk) = (p.str("structure"), p.array("phases").map(|x| x.1), p.array("mask").map(|x| x.1));
                let sel = texture_selection(p, &st, ph.as_deref(), mk.as_deref(), 10_000);
                crate::crystal_texture::odf_sections(input.unwrap(), &sel, &p.str("sections"), p.f64("step", 5.0), p.f64("sigma", 7.5), p.f64("index_step", 5.0))
            },
        },
        FilterDef {
            id: "misorientation_distribution",
            label: "Misorientation Distribution",
            category: "Grains",
            description: "The distribution of disorientation angles between neighbouring points, between random pairs of points and between random orientations (a table for the Charts panel).",
            keywords: &["mdf", "misorientation distribution", "disorientation angle", "grain boundary angles", "mackenzie", "random distribution", "boundary statistics"],
            is_source: false,
            ext: None,
            temporal: None,
            multi: None,
            params: {
                let mut v = vec![
                    f("min_angle", "Smallest angle (degrees)", 5.0, 0.0, 179.0).help("Angles below it (inside grains) are left out of every curve."),
                    f("bin", "Bin size (degrees)", 1.0, 0.1, 30.0),
                    i("pairs", "Random pairs", 100_000, 1000, 100_000_000).advanced(),
                ];
                v.extend(texture_selection_params(10_000_000).into_iter().filter(|p| p.key != "max_orientations"));
                v
            },
            apply: |input, p| {
                let (st, ph, mk) = (p.str("structure"), p.array("phases").map(|x| x.1), p.array("mask").map(|x| x.1));
                let sel = texture_selection(p, &st, ph.as_deref(), mk.as_deref(), i64::MAX);
                crate::crystal_texture::misorientation_distribution(input.unwrap(), &sel, p.f64("min_angle", 5.0), p.f64("bin", 1.0), p.i64("pairs", 100_000).max(1) as usize)
            },
        },
        FilterDef {
            id: "texture_components",
            label: "Texture Components",
            category: "Grains",
            description: "The fraction of orientations with a crystal direction of each family within a cone around each sample axis (a table).",
            keywords: &["texture fractions", "fiber fraction", "component fraction", "cube texture", "volume fraction", "100 fiber", "111 fiber"],
            is_source: false,
            ext: None,
            temporal: None,
            multi: None,
            params: {
                let mut v = vec![
                    s("families", "Directions", "100, 110, 111"),
                    s("axes", "Sample axes", "X, Y, Z"),
                    f("cone", "Cone (degrees)", 15.0, 0.1, 90.0),
                ];
                v.extend(texture_selection_params(10_000_000));
                v
            },
            apply: |input, p| {
                let (st, ph, mk) = (p.str("structure"), p.array("phases").map(|x| x.1), p.array("mask").map(|x| x.1));
                let sel = texture_selection(p, &st, ph.as_deref(), mk.as_deref(), 10_000_000);
                crate::crystal_texture::texture_components(input.unwrap(), &sel, &p.str("families"), &p.str("axes"), p.f64("cone", 15.0))
            },
        },
        FilterDef {
            id: "chord_lengths",
            label: "Chord Lengths",
            category: "Grains",
            description: "Grain-boundary chord lengths along lines in x, y and z of a map or volume, as a list or a histogram, with their means.",
            keywords: &["chord length", "linear intercept", "grain size", "intercept length", "line intercept", "grain boundary spacing"],
            is_source: false,
            ext: None,
            temporal: None,
            multi: None,
            params: {
                let mut v = vec![
                    f("threshold", "Boundary angle (degrees)", 10.0, 0.1, 180.0),
                    f("spacing", "Line spacing", 0.0, 0.0, f64::INFINITY).help("Distance between the lines in length units; 0: every row of points."),
                    choice("output", "Output", &[crate::crystal_texture::CHORDS, crate::crystal_texture::HISTOGRAM], crate::crystal_texture::CHORDS),
                    f("bin", "Histogram bin (length)", 0.0, 0.0, f64::INFINITY).help("0: a thirtieth of the longest chord."),
                    i("max_gap", "Largest gap (points)", 3, 0, 1000).help("Runs of more unused points end a line segment; chords touching a segment end are left out.").advanced(),
                    b("despeckle", "Ignore single-point specks", false).advanced(),
                ];
                v.extend(texture_selection_params(10_000_000).into_iter().filter(|p| p.key != "max_orientations"));
                v
            },
            apply: |input, p| {
                let (st, ph, mk) = (p.str("structure"), p.array("phases").map(|x| x.1), p.array("mask").map(|x| x.1));
                let sel = texture_selection(p, &st, ph.as_deref(), mk.as_deref(), i64::MAX);
                crate::crystal_texture::chord_lengths(input.unwrap(), &sel, p.f64("threshold", 10.0), p.f64("spacing", 0.0), p.i64("max_gap", 3).max(0) as usize, p.bool("despeckle", false), &p.str("output"), p.f64("bin", 0.0))
            },
        },
        FilterDef {
            id: "orientation_noise",
            label: "Add Orientation Noise",
            category: "Grains",
            description: "Turn every point's orientation (or every feature's) by a small random rotation with a normally distributed angle.",
            keywords: &["noise", "perturb orientations", "scatter", "synthetic", "orientation spread"],
            is_source: false,
            ext: None,
            temporal: None,
            multi: None,
            params: vec![
                f("sigma", "Angle (degrees, standard deviation)", 2.0, 0.0, 180.0),
                b("per_feature", "One rotation per feature", false),
                cell_arr("orientations", "Orientations").help("Empty: EulerAngles, Quats or OrientationMatrix.").advanced(),
                s("feature_ids", "Feature ids", "FeatureIds").advanced(),
                i("seed", "Random seed", 1, 0, i64::MAX).advanced(),
            ],
            apply: |input, p| {
                let name = p.array("orientations").map(|x| x.1).unwrap_or_default();
                crate::synthetic::orientation_noise(input.unwrap(), &name, p.f64("sigma", 2.0), p.bool("per_feature", false), &p.str("feature_ids"), p.i64("seed", 1) as u64)
            },
        },
        FilterDef {
            id: "jumble_orientations",
            label: "Jumble Orientations",
            category: "Grains",
            description: "Swap the orientations of the features at random: the same set of grain orientations, placed on other grains.",
            keywords: &["shuffle orientations", "randomize", "uncorrelated", "synthetic", "permute"],
            is_source: false,
            ext: None,
            temporal: None,
            multi: None,
            params: vec![
                cell_arr("orientations", "Orientations").help("Empty: EulerAngles, Quats or OrientationMatrix.").advanced(),
                s("feature_ids", "Feature ids", "FeatureIds").advanced(),
                i("seed", "Random seed", 1, 0, i64::MAX).advanced(),
            ],
            apply: |input, p| {
                let name = p.array("orientations").map(|x| x.1).unwrap_or_default();
                crate::synthetic::jumble_orientations(input.unwrap(), &name, &p.str("feature_ids"), p.i64("seed", 1) as u64)
            },
        },
        FilterDef {
            id: "ensemble_statistics",
            label: "Generate Ensemble Statistics",
            category: "Grains",
            description: "Fit the grain size (lognormal), shape (beta fits of the aspect ratios b/a and c/a) and neighbour count distributions of a segmented microstructure (a table).",
            keywords: &["statistics", "grain size distribution", "lognormal", "beta", "aspect ratio", "neighbors", "stats generator", "fit"],
            is_source: false,
            ext: None,
            temporal: None,
            multi: None,
            params: vec![
                b("include_surface", "Include features on the surface", false),
                s("feature_ids", "Feature ids", "FeatureIds").advanced(),
            ],
            apply: |input, p| crate::synthetic::ensemble_statistics(input.unwrap(), &p.str("feature_ids"), p.bool("include_surface", false)),
        },
        FilterDef {
            id: "pack_like",
            label: "Pack Like",
            category: "Grains",
            description: "A new microstructure packed with the grain size, shape and orientation statistics measured on the input (a synthetic copy of a measured volume).",
            keywords: &["synthetic copy", "statistically equivalent", "rve", "pack grains", "reconstruct", "ensemble statistics"],
            is_source: false,
            ext: None,
            temporal: None,
            multi: None,
            params: vec![
                ParamSpec::new("cells", "Cells", ParamKind::Floats, ParamValue::Floats(vec![64.0, 64.0, 64.0])),
                f("spacing", "Cell size", 0.0, 0.0, f64::INFINITY).help("0: the input's."),
                s("feature_ids", "Feature ids", "FeatureIds").advanced(),
                i("seed", "Random seed", 1, 0, i64::MAX).advanced(),
            ],
            apply: |input, p| {
                let c = p.floats("cells");
                let n = [0, 1, 2].map(|k| c.get(k).copied().unwrap_or(64.0).max(1.0) as usize);
                crate::synthetic::pack_like(input.unwrap(), n, p.f64("spacing", 0.0), &p.str("feature_ids"), p.i64("seed", 1) as u64)
            },
        },
        FilterDef {
            id: "insert_precipitates",
            label: "Insert Precipitates",
            category: "Grains",
            description: "Insert particles of a new phase (lognormal sizes, apart from one another) into a microstructure until they fill a volume fraction, at random or on grain boundaries.",
            keywords: &["precipitates", "second phase", "particles", "inclusions", "carbides", "synthetic"],
            is_source: false,
            ext: None,
            temporal: None,
            multi: None,
            params: vec![
                f("fraction", "Volume fraction", 0.05, 0.0, 0.9),
                f("esd_mu", "Size mu (log of diameter)", 1.0, f64::NEG_INFINITY, f64::INFINITY).help("The diameters are lognormal: mean of their logarithms."),
                f("esd_sigma", "Size sigma", 0.2, 0.0, 3.0),
                b("at_boundaries", "On grain boundaries", false),
                choice("structure", "Crystal structure", &crate::crystal::structure_choices()[1..], "Cubic m-3m"),
                s("feature_ids", "Feature ids", "FeatureIds").advanced(),
                i("seed", "Random seed", 1, 0, i64::MAX).advanced(),
            ],
            apply: |input, p| {
                crate::synthetic::insert_precipitates(
                    input.unwrap(),
                    &crate::synthetic::PrecipitateParams {
                        fraction: p.f64("fraction", 0.05),
                        esd_mu: p.f64("esd_mu", 1.0),
                        esd_sigma: p.f64("esd_sigma", 0.2),
                        at_boundaries: p.bool("at_boundaries", false),
                        structure: p.str("structure"),
                        seed: p.i64("seed", 1) as u64,
                    },
                    &p.str("feature_ids"),
                )
            },
        },
        FilterDef {
            id: "match_crystallography",
            label: "Match Crystallography",
            category: "Grains",
            description: "Give the grains orientations drawn from a reference microstructure, then swap them until the misorientations across grain boundaries are distributed as in the reference.",
            keywords: &["match texture", "match mdf", "orientation assignment", "synthetic", "misorientation distribution", "odf"],
            is_source: false,
            ext: None,
            temporal: None,
            multi: Some(MultiInput {
                label: "Reference",
                min: 2,
                max: Some(2),
                apply: |inputs, p| crate::synthetic::match_crystallography(inputs[0], inputs[1], &p.str("feature_ids"), p.i64("moves", 50).max(1) as usize, p.i64("seed", 1) as u64),
            }),
            params: vec![
                i("moves", "Moves per grain", 50, 1, 100_000).advanced(),
                s("feature_ids", "Feature ids", "FeatureIds").advanced(),
                i("seed", "Random seed", 1, 0, i64::MAX).advanced(),
            ],
            apply: |_, _| Err(anyhow!("Match Crystallography needs a reference microstructure as its second input")),
        },
        FilterDef {
            id: "quick_surface_mesh",
            label: "Quick Surface Mesh",
            category: "Grains",
            description: "Triangles on every voxel face between different features, with the features on each side (FaceLabels) and the number of features at each node (NodeType).",
            keywords: &["grain boundary mesh", "surface mesh", "boundaries", "triangles", "stl", "interfaces", "voxel surface", "face labels"],
            is_source: false,
            ext: None,
            temporal: None,
            multi: None,
            params: vec![s("feature_ids", "Feature ids", "FeatureIds").advanced()],
            apply: |input, p| crate::grain_surface::quick_surface_mesh(input.unwrap(), &p.str("feature_ids")),
        },
        FilterDef {
            id: "smooth_boundaries",
            label: "Smooth Boundaries",
            category: "Grains",
            description: "Laplacian or Taubin smoothing of a grain boundary surface: triple lines follow triple lines, quadruple points and the outer faces stay put.",
            keywords: &["laplacian smoothing", "taubin", "smooth surface mesh", "staircase", "grain boundaries", "node type"],
            is_source: false,
            ext: None,
            temporal: None,
            multi: None,
            params: vec![
                i("iterations", "Iterations", 20, 0, 10_000),
                f("lambda", "Step (lambda)", 0.5, 0.0, 1.0),
                f("mu", "Inflating step (mu, Taubin)", -0.53, -1.0, 0.0).help("0: plain Laplacian smoothing, which shrinks the grains; about -lambda - 0.03 keeps their volume."),
                f("triple_lambda", "Step on triple lines", 0.25, 0.0, 1.0).advanced(),
                f("quad_lambda", "Step at quadruple points", 0.0, 0.0, 1.0).advanced(),
                b("smooth_outer", "Smooth on the outer faces", true).advanced(),
            ],
            apply: |input, p| {
                crate::grain_surface::smooth(
                    input.unwrap(),
                    &crate::grain_surface::SmoothParams {
                        iterations: p.i64("iterations", 20).max(0) as usize,
                        lambda: p.f64("lambda", 0.5),
                        mu: p.f64("mu", -0.53),
                        triple_lambda: p.f64("triple_lambda", 0.25),
                        quad_lambda: p.f64("quad_lambda", 0.0),
                        smooth_outer: p.bool("smooth_outer", true),
                    },
                )
            },
        },
        FilterDef {
            id: "triangle_geometry",
            label: "Triangle Geometry",
            category: "Grains",
            description: "Areas, normals, centroids and smallest angles of the triangles of a boundary surface, and the volume and boundary area of every feature it encloses.",
            keywords: &["triangle area", "face normals", "minimum angle", "dihedral", "surface area", "enclosed volume", "mesh quality"],
            is_source: false,
            ext: None,
            temporal: None,
            multi: None,
            params: vec![],
            apply: |input, _| crate::grain_surface::triangle_geometry(input.unwrap()),
        },
        FilterDef {
            id: "feature_surfaces",
            label: "Feature Surfaces",
            category: "Grains",
            description: "The closed surface of each feature of a boundary surface, facing outwards, one block per feature (save as STL for one file each).",
            keywords: &["stl per grain", "grain surfaces", "export grains", "closed surface", "watertight", "per feature"],
            is_source: false,
            ext: None,
            temporal: None,
            multi: None,
            params: vec![ParamSpec::new("features", "Features (empty: all)", ParamKind::Floats, ParamValue::Floats(Vec::new()))],
            apply: |input, p| crate::grain_surface::feature_surfaces(input.unwrap(), &p.floats("features").iter().map(|x| *x as i64).collect::<Vec<_>>()),
        },
        FilterDef {
            id: "face_orientation_colors",
            label: "Face Orientation Colors",
            category: "Grains",
            description: "Colour boundary triangles by the inverse pole figure key of their plane in the crystal, and by the misorientation across them.",
            keywords: &["face ipf", "boundary plane", "misorientation colors", "grain boundary character", "colour boundaries"],
            is_source: false,
            ext: None,
            temporal: None,
            multi: None,
            params: vec![],
            apply: |input, _| crate::grain_surface::face_orientation_colors(input.unwrap()),
        },
        FilterDef {
            id: "gbcd_section",
            label: "GBCD Section",
            category: "Grains",
            description: "The grain boundary character distribution for one misorientation: how often each boundary plane occurs, in multiples of random, as a pole figure of crystal directions.",
            keywords: &["gbcd", "grain boundary character distribution", "boundary planes", "sigma 3", "twin boundaries", "five parameter", "mrd"],
            is_source: false,
            ext: None,
            temporal: None,
            multi: None,
            params: vec![
                f("angle", "Misorientation angle (degrees)", 60.0, 0.0, 180.0),
                v3("axis", "Misorientation axis (crystal)", [1.0, 1.0, 1.0]),
                f("tolerance", "Tolerance (degrees)", 5.0, 0.5, 30.0),
                f("sigma", "Smoothing (degrees)", 5.0, 0.5, 45.0),
                choice("projection", "Projection", &[crate::crystal_texture::STEREOGRAPHIC, crate::crystal_texture::EQUAL_AREA], crate::crystal_texture::STEREOGRAPHIC),
                i("resolution", "Resolution", 40, 4, 400).advanced(),
            ],
            apply: |input, p| {
                crate::grain_surface::gbcd_section(input.unwrap(), p.f64("angle", 60.0), p.vec3("axis", [1.0, 1.0, 1.0]), p.f64("tolerance", 5.0), p.f64("sigma", 5.0), &p.str("projection"), p.i64("resolution", 40).max(4) as usize)
            },
        },
        FilterDef {
            id: "crystal_mechanics",
            label: "Crystal Mechanics",
            category: "Grains",
            description: "Schmid factor, Taylor factor and directional Young's modulus of every orientation for a loading direction, from a material's slip systems and elastic constants, with volume means and a yield-strength estimate.",
            keywords: &["schmid factor", "taylor factor", "youngs modulus", "elastic anisotropy", "slip systems", "yield strength", "crystal plasticity", "fcc bcc hcp"],
            is_source: false,
            ext: None,
            temporal: None,
            multi: None,
            params: {
                let mut v = vec![
                    choice("material", "Material", &crate::mechanics::material_names(), "IN718"),
                    v3("direction", "Loading direction", [0.0, 0.0, 1.0]),
                    b("taylor", "Taylor factor", true).help("The full-constraint Taylor factor (slower: every set of five independent slip systems is tried)."),
                    cell_arr("gnd", "Dislocation density (1/m2)").help("Optional: its mean adds the Taylor hardening alpha mu b M sqrt(rho) to the yield estimate.").advanced(),
                    choice("lattice", "Custom: lattice", &["FCC", "BCC", "HCP"], "FCC").advanced(),
                    ParamSpec::new("elastic", "Custom: elastic constants (GPa)", ParamKind::Floats, ParamValue::Floats(vec![250.0, 160.0, 120.0])).help("Cubic: C11 C12 C44. Hexagonal: C11 C12 C13 C33 C44.").advanced(),
                    f("shear_modulus", "Custom: shear modulus (GPa)", 80.0, 0.0, f64::INFINITY).advanced(),
                    f("burgers", "Custom: Burgers vector (m)", 2.5e-10, 0.0, f64::INFINITY).advanced(),
                    f("c_over_a", "Custom: c/a", 1.633, 0.1, 10.0).advanced(),
                    s("families", "Custom: slip families", "").help("Like 'Basal <a>=1, Prismatic <a>=0.7, Pyramidal <c+a>=3' (relative resolved shear stresses). Empty: the lattice's first family.").advanced(),
                    f("sigma_base", "Custom: base yield strength (MPa)", 0.0, 0.0, f64::INFINITY).advanced(),
                ];
                v.extend(texture_selection_params(10_000_000).into_iter().filter(|p| p.key != "max_orientations"));
                v
            },
            apply: |input, p| {
                let (st, ph, mk) = (p.str("structure"), p.array("phases").map(|x| x.1), p.array("mask").map(|x| x.1));
                let sel = texture_selection(p, &st, ph.as_deref(), mk.as_deref(), i64::MAX);
                let custom = crate::mechanics::CustomMaterial {
                    lattice: p.str("lattice"),
                    elastic: p.floats("elastic"),
                    shear_modulus_gpa: p.f64("shear_modulus", 80.0),
                    burgers_m: p.f64("burgers", 2.5e-10),
                    c_over_a: p.f64("c_over_a", 1.633),
                    families: p.str("families"),
                    sigma_base_mpa: p.f64("sigma_base", 0.0),
                };
                let gnd = p.array("gnd").map(|x| x.1);
                crate::mechanics::crystal_mechanics(input.unwrap(), &sel, &p.str("material"), &custom, p.vec3("direction", [0.0, 0.0, 1.0]), p.bool("taylor", true), gnd.as_deref())
            },
        },
        FilterDef {
            id: "srod",
            label: "SROD",
            category: "Grains",
            description: "How far each grain's orientation has turned from the one it started with, by height above its base in an additive build (z the build direction), with the grain's class (plate, epitaxial, interface, originating).",
            keywords: &["srod", "orientation shift", "misorientation from start", "additive manufacturing", "build height", "columnar grains", "lattice rotation", "layer"],
            is_source: false,
            ext: None,
            temporal: None,
            multi: None,
            params: vec![
                f("layer", "Layer thickness", 40.0, 1e-9, f64::INFINITY).help("The height of a bin, in the volume's length unit."),
                f("plate_top", "Plate top (z)", f64::NAN, f64::NEG_INFINITY, f64::INFINITY).help("Height of the base plate's top; empty: the bottom of the volume."),
                f("interface", "Interface height", f64::NAN, 0.0, f64::INFINITY).help("Grains starting below this height above the plate are 'interface' grains; empty: one layer."),
                choice("output", "Output", &[crate::am::OUT_VOLUME, crate::am::OUT_BINS, crate::am::OUT_HEIGHT], crate::am::OUT_VOLUME),
                f("min_volume", "Smallest grain (volume)", 212.0, 0.0, f64::INFINITY).advanced(),
                f("reference_volume", "Reference volume", 106.0, 0.0, f64::INFINITY).help("The grain's lowest sections holding this volume give its starting orientation.").advanced(),
                i("reference_sections", "At most this many sections", 5, 1, 1000).advanced(),
                f("min_bin_volume", "Smallest bin (volume)", 424.0, 0.0, f64::INFINITY).advanced(),
                s("feature_ids", "Feature ids", "FeatureIds").advanced(),
            ],
            apply: |input, p| {
                let d = input.unwrap();
                let img = crate::features::image(d)?;
                let bottom = img.origin[2] + img.extent[4] as f64 * img.spacing[2];
                let layer = p.f64("layer", 40.0);
                let pt = p.f64("plate_top", f64::NAN);
                let itf = p.f64("interface", f64::NAN);
                let rules = crate::am::SrodRules {
                    layer_um: layer,
                    plate_top: if pt.is_finite() { pt } else { bottom },
                    interface_um: if itf.is_finite() { itf } else { layer },
                    min_volume: p.f64("min_volume", 212.0),
                    reference_volume: p.f64("reference_volume", 106.0),
                    reference_sections: p.i64("reference_sections", 5).max(1) as usize,
                    min_bin_volume: p.f64("min_bin_volume", 424.0),
                };
                crate::am::srod_filter(d, &p.str("feature_ids"), &rules, &p.str("output"))
            },
        },
        FilterDef {
            id: "gnd_density",
            label: "GND Density",
            category: "Grains",
            description: "Density of geometrically necessary dislocations in every cell (1/m2): from Nye's tensor of the lattice curvature inside each grain, or from a kernel average misorientation.",
            keywords: &["gnd", "geometrically necessary dislocations", "nye tensor", "dislocation density", "lattice curvature", "kam", "stored energy"],
            is_source: false,
            ext: None,
            temporal: None,
            multi: None,
            params: vec![
                choice("method", "Method", &[crate::am::GND_NYE, crate::am::GND_KAM], crate::am::GND_NYE),
                f("burgers", "Burgers vector (m)", 2.54e-10, 1e-12, 1e-8),
                i("kernel", "Kernel (cells)", 3, 3, 15).help("Nye tensor: the curvature is fitted over this many cells across (odd)."),
                cell_arr("kam", "KAM array (degrees)").advanced(),
                f("step", "Step for KAM (micrometres)", 0.0, 0.0, f64::INFINITY).help("0: the smallest cell size.").advanced(),
                s("feature_ids", "Feature ids", "FeatureIds").advanced(),
            ],
            apply: |input, p| {
                let kam = p.array("kam").map(|x| x.1).filter(|x| !x.is_empty()).unwrap_or_else(|| "KernelAverageMisorientations".into());
                crate::am::gnd_density(input.unwrap(), &p.str("feature_ids"), &p.str("method"), p.f64("burgers", 2.54e-10), p.i64("kernel", 3).max(3) as usize, &kam, p.f64("step", 0.0))
            },
        },
        FilterDef {
            id: "pack_grains",
            label: "Pack Grains",
            category: "Sources",
            description: "A synthetic microstructure packed from goal statistics: lognormal grain sizes, beta-distributed aspect ratios, ellipsoids or other shapes, with an optional matrix phase.",
            keywords: &["pack primary phases", "synthetic microstructure", "statistics", "grain size distribution", "ellipsoids", "rve", "matrix phase", "stats generator"],
            is_source: true,
            ext: None,
            temporal: None,
            multi: None,
            params: vec![
                ParamSpec::new("cells", "Cells", ParamKind::Floats, ParamValue::Floats(vec![64.0, 64.0, 64.0])),
                f("spacing", "Cell size", 1.0, 1e-9, f64::INFINITY),
                f("esd_mu", "Size mu (log of diameter)", 2.3, f64::NEG_INFINITY, f64::INFINITY).help("The equivalent diameters are lognormal: mean of their logarithms (2.3: about 10 cells)."),
                f("esd_sigma", "Size sigma", 0.2, 0.0, 3.0),
                f("min_cut", "Smallest size (sigmas below)", 3.0, 0.0, 10.0).advanced(),
                f("max_cut", "Largest size (sigmas above)", 3.0, 0.0, 10.0).advanced(),
                ParamSpec::new("ba", "b/a beta (alpha, beta)", ParamKind::Floats, ParamValue::Floats(vec![15.0, 2.0])),
                ParamSpec::new("ca", "c/a beta (alpha, beta)", ParamKind::Floats, ParamValue::Floats(vec![12.0, 4.0])).help("At most b/a."),
                choice("shape", "Shape", &[crate::synthetic::SHAPE_ELLIPSOID, crate::synthetic::SHAPE_SUPER, crate::synthetic::SHAPE_CUBOCT, crate::synthetic::SHAPE_CYLINDER], crate::synthetic::SHAPE_ELLIPSOID),
                f("exponent", "Superellipsoid exponent", 4.0, 1.0, 50.0).advanced(),
                b("aligned", "Axes along x, y, z", false),
                f("fraction", "Grain volume fraction", 1.0, 0.01, 1.0).help("Below 1, the space between the grains is a matrix phase."),
                b("periodic", "Periodic", false),
                choice("orientations", "Orientations", &[crate::synthetic::ORIENT_RANDOM, crate::synthetic::ORIENT_FIBER100, crate::synthetic::ORIENT_FIBER111], crate::synthetic::ORIENT_RANDOM),
                f("spread", "Fibre spread (degrees)", 10.0, 0.0, 90.0),
                choice("structure", "Crystal structure", &crate::crystal::structure_choices()[1..], "Cubic m-3m"),
                b("calibrate", "Match the goal sizes and shapes", true).help("Space-filling grains measure more irregular than the shapes drawn: repack with corrected goals until the measured statistics match (slower)."),
                i("moves", "Placement moves per grain", 20, 0, 100_000).advanced(),
                i("seed", "Random seed", 1, 0, i64::MAX).advanced(),
            ],
            apply: |_, p| {
                let c = p.floats("cells");
                let n = [0, 1, 2].map(|k| c.get(k).copied().unwrap_or(64.0).max(1.0) as usize);
                let pair = |k: &str| {
                    let v = p.floats(k);
                    (v.first().copied().unwrap_or(12.0), v.get(1).copied().unwrap_or(4.0))
                };
                crate::synthetic::pack_grains(
                    &crate::synthetic::PackParams {
                        cells: n,
                        spacing: p.f64("spacing", 1.0),
                        esd_mu: p.f64("esd_mu", 2.3),
                        esd_sigma: p.f64("esd_sigma", 0.2),
                        min_cut: p.f64("min_cut", 3.0),
                        max_cut: p.f64("max_cut", 3.0),
                        ba: pair("ba"),
                        ca: pair("ca"),
                        shape: p.str("shape"),
                        exponent: p.f64("exponent", 4.0),
                        aligned: p.bool("aligned", false),
                        fraction: p.f64("fraction", 1.0),
                        periodic: p.bool("periodic", false),
                        orientations: p.str("orientations"),
                        spread_deg: p.f64("spread", 10.0),
                        structure: p.str("structure"),
                        seed: p.i64("seed", 1) as u64,
                        moves: p.i64("moves", 20).max(0) as usize,
                        calibrate: p.bool("calibrate", true),
                    },
                    None,
                )
            },
        },
        FilterDef {
            id: "voronoi_grains",
            label: "Voronoi Grains",
            category: "Sources",
            description: "A synthetic microstructure of grains: a Voronoi tessellation of random seeds, or a Laguerre (power) tessellation with spread grain sizes, with random or fibre-textured orientations.",
            keywords: &["voronoi", "laguerre", "tessellation", "synthetic microstructure", "polycrystal", "grains", "rve", "power diagram"],
            is_source: true,
            ext: None,
            temporal: None,
            multi: None,
            params: vec![
                ParamSpec::new("cells", "Cells", ParamKind::Floats, ParamValue::Floats(vec![64.0, 64.0, 64.0])),
                f("spacing", "Cell size", 1.0, 1e-9, f64::INFINITY),
                i("grains", "Grains", 100, 1, 10_000_000),
                f("size_sigma", "Size spread (log sigma)", 0.0, 0.0, 3.0).help("0: Voronoi grains. Above 0: Laguerre grains, seed radii spread lognormally."),
                b("periodic", "Periodic", false),
                choice("orientations", "Orientations", &[crate::synthetic::ORIENT_RANDOM, crate::synthetic::ORIENT_FIBER100, crate::synthetic::ORIENT_FIBER111], crate::synthetic::ORIENT_RANDOM),
                f("spread", "Fibre spread (degrees)", 10.0, 0.0, 90.0),
                choice("structure", "Crystal structure", &crate::crystal::structure_choices()[1..], "Cubic m-3m"),
                i("seed", "Random seed", 1, 0, i64::MAX).advanced(),
            ],
            apply: |_, p| {
                let c = p.floats("cells");
                let n = [0, 1, 2].map(|k| c.get(k).copied().unwrap_or(64.0).max(1.0) as usize);
                crate::synthetic::tessellation(&crate::synthetic::TessellationParams {
                    cells: n,
                    spacing: p.f64("spacing", 1.0),
                    grains: p.i64("grains", 100).max(1) as usize,
                    size_sigma: p.f64("size_sigma", 0.0),
                    periodic: p.bool("periodic", false),
                    orientations: p.str("orientations"),
                    spread_deg: p.f64("spread", 10.0),
                    structure: p.str("structure"),
                    seed: p.i64("seed", 1) as u64,
                })
            },
        },
        FilterDef {
            id: "columnar_grains",
            label: "Columnar Grains",
            category: "Sources",
            description: "Columnar grains of an additive build: tilted columns growing from the base plate along Z, stopping at lognormal heights or when overgrown, replaced by new grains.",
            keywords: &["additive manufacturing", "am", "columnar", "build", "epitaxial", "synthetic microstructure", "laser powder bed", "grains"],
            is_source: true,
            ext: None,
            temporal: None,
            multi: None,
            params: vec![
                v3("size", "Box size", [300.0, 300.0, 450.0]),
                f("voxel", "Voxel size", 5.0, 1e-9, f64::INFINITY),
                f("width", "Grain width", 50.0, 1e-9, f64::INFINITY),
                f("height", "Median grain height", 150.0, 1e-9, f64::INFINITY),
                f("height_sigma", "Height spread (log sigma)", 0.5, 0.0, 3.0),
                f("tilt", "Column tilt (degrees, standard deviation)", 6.0, 0.0, 60.0),
                f("width_sigma", "Width spread (log sigma)", 0.0, 0.0, 3.0).advanced(),
                f("width_height_exponent", "Width grows with height (exponent)", 0.0, -2.0, 2.0).advanced(),
                choice("orientations", "Orientations", &[crate::synthetic::ORIENT_RANDOM, crate::synthetic::ORIENT_FIBER100, crate::synthetic::ORIENT_FIBER111], crate::synthetic::ORIENT_FIBER100),
                f("spread", "Fibre spread (degrees)", 10.0, 0.0, 90.0),
                choice("structure", "Crystal structure", &crate::crystal::structure_choices()[1..], "Cubic m-3m"),
                i("seed", "Random seed", 1, 0, i64::MAX).advanced(),
            ],
            apply: |_, p| {
                let s = p.vec3("size", [300.0, 300.0, 450.0]);
                crate::synthetic::columnar_grains(
                    &crate::synthetic::ColumnParams {
                        size: s,
                        voxel: p.f64("voxel", 5.0),
                        width: p.f64("width", 50.0),
                        height: p.f64("height", 150.0),
                        height_sigma: p.f64("height_sigma", 0.5),
                        tilt_sd_deg: p.f64("tilt", 6.0),
                        width_sigma: p.f64("width_sigma", 0.0),
                        width_height_exponent: p.f64("width_height_exponent", 0.0),
                    },
                    &p.str("orientations"),
                    p.f64("spread", 10.0),
                    &p.str("structure"),
                    p.i64("seed", 1) as u64,
                )
            },
        },
        FilterDef {
            id: "ipf_color_key",
            label: "IPF Color Key",
            category: "Sources",
            description: "The inverse pole figure color key of a crystal class: its standard triangle in stereographic projection, colored.",
            keywords: &["ipf", "legend", "color key", "standard triangle", "stereographic", "crystal", "ebsd"],
            is_source: true,
            ext: None,
            temporal: None,
            multi: None,
            params: vec![
                choice("structure", "Crystal structure", &crate::crystal::structure_choices()[1..], "Cubic m-3m"),
                i("resolution", "Resolution", 60, 4, 400),
            ],
            apply: |_, p| Ok(DataSet::PolyData(crate::crystal::ipf_key(&p.str("structure"), p.i64("resolution", 60).max(4) as usize)?)),
        },
        FilterDef {
            id: "geometry",
            label: "Geometry",
            category: "Sources",
            description: "Solids built from geometry commands (primitives, booleans, extruded, revolved or swept outlines, imported surfaces), with named faces.",
            keywords: &["cad", "csg", "solid", "brick", "cylinder", "boolean", "extrude", "revolve", "sweep", "implicit", "mesh geometry", "model"],
            is_source: true,
            ext: None,
            temporal: None,
            multi: None,
            params: vec![
                ParamSpec::new("commands", "Commands", ParamKind::Code, ParamValue::Str(GEOMETRY_TEMPLATE.into()))
                    .help("One command per line (or separated by ;). See the Geometry and meshing chapter of the user guide, or ask the assistant."),
                i("resolution", "Resolution", 64, 8, 1024).help("Cells along the longest side of each body when drawing its surface."),
                choice("output", "Output", &[crate::geometry_model::OUT_SURFACE, crate::geometry_model::OUT_DISTANCE, crate::geometry_model::OUT_MESH, crate::geometry_model::OUT_TET], crate::geometry_model::OUT_SURFACE)
                    .help("Surface: the bodies' surfaces with BodyId and FaceId. Distance field: the signed distance to the nearest body on a grid. Hex mesh: the hexahedral mesh of the bodies named by 'mesh body' commands. Tet mesh: their tetrahedral mesh."),
                b("project", "Snap to the surface", true).help("Move the drawn points onto the exact surface.").advanced(),
            ],
            apply: |_, p| crate::geometry_model::geometry(&p.str("commands"), p.i64("resolution", 64).max(2) as usize, &p.str("output"), p.bool("project", true)),
        },
        FilterDef {
            id: "signed_distance",
            label: "Signed Distance Field",
            category: "Mesh",
            description: "The distance to a closed surface (negative inside) on an image grid around it.",
            keywords: &["sdf", "distance field", "implicit", "level set", "inside outside", "voxelize", "stl"],
            is_source: false,
            ext: None,
            temporal: None,
            multi: None,
            params: vec![
                i("resolution", "Resolution", 64, 4, 1024).help("Cells along the longest side of the surface."),
                f("margin", "Margin", 0.1, 0.0, 10.0).help("Space added around the surface on every side, as a fraction of its size."),
            ],
            apply: |input, p| per_leaf(input.unwrap(), &|d| crate::geometry_model::signed_distance(d, p.i64("resolution", 64).max(2) as usize, p.f64("margin", 0.1))),
        },
        FilterDef {
            id: "box_mesh",
            label: "Box Mesh",
            category: "Sources",
            description: "A box of hexahedra, optionally graded, with side sets and node sets on its six faces.",
            keywords: &["hex mesh", "structured mesh", "block", "brick mesh", "grid", "graded", "finite element"],
            is_source: true,
            ext: None,
            temporal: None,
            multi: None,
            params: vec![
                v3("size", "Size", [1.0, 1.0, 1.0]),
                v3("origin", "Origin", [0.0, 0.0, 0.0]),
                v3("cells", "Cells", [10.0, 10.0, 10.0]),
                v3("grading", "Grading (largest/smallest)", [1.0, 1.0, 1.0]).help("Ratio of the largest to the smallest interval along x, y and z (1: uniform)."),
                choice("cluster", "Small intervals at", &["Start", "End", "Both"], "Start"),
                choice("element", "Element", &crate::mesh_gen::ELEMENT_TYPES, "HEX8"),
            ],
            apply: |_, p| {
                let c = p.vec3("cells", [10.0; 3]).map(|v| v.round().max(1.0) as usize);
                crate::mesh_gen::box_mesh(p.vec3("origin", [0.0; 3]), p.vec3("size", [1.0; 3]), c, p.vec3("grading", [1.0; 3]), &p.str("cluster"), &p.str("element"))
            },
        },
        FilterDef {
            id: "inclusion_mesh",
            label: "Inclusion Mesh",
            category: "Sources",
            description: "A box of matrix with randomly placed inclusions (spheres, ellipsoids, superquadrics, harmonic grains), meshed with conforming hexahedra that follow the inclusion surfaces exactly.",
            keywords: &["particles", "composite", "rve", "inclusions", "spheres in cube", "hex mesh", "o-grid", "periodic", "microstructure mesh", "voids"],
            is_source: true,
            ext: None,
            temporal: None,
            multi: None,
            params: vec![
                v3("size", "Box size", [1.0, 1.0, 1.0]),
                i("cells", "Cells along the shortest side", 24, 4, 1000).help("The background cell size is the shortest side divided by this."),
                i("count", "Inclusions", 8, 0, 100_000),
                f("radius", "Radius", 0.075, 1e-9, f64::INFINITY),
                f("radius_std", "Radius spread", 0.0, 0.0, f64::INFINITY).help("Standard deviation of the radius (normal, cut at 3 spreads)."),
                choice("shape", "Shape", &crate::mesh_gen::SHAPES, "Sphere"),
                v3("aspect", "Semi-axes / radius", [1.0, 0.7, 0.5]).help("Ellipsoids and superquadrics: their semi-axes as multiples of the radius."),
                ParamSpec::new("exponents", "Exponents", ParamKind::Floats, ParamValue::Floats(vec![4.0, 4.0])).help("Superquadrics: the in-plane and vertical exponents (2 2 is an ellipsoid; larger is boxier)."),
                f("roughness", "Roughness", 0.12, 0.0, 1.0).help("Harmonic grains: size of the surface perturbation."),
                b("random_orientation", "Random orientation", true),
                i("seed", "Seed", 7, 0, i64::MAX),
                b("periodic", "Periodic", false).help("Pair the nodes of opposite faces (node sets x-_node_set ... and PeriodicPairs) for periodic boundary conditions."),
                choice("element", "Element", &crate::mesh_gen::ELEMENT_TYPES, "HEX8").help("HEX20 and HEX27 put their new nodes on the inclusion surfaces too."),
                choice("packing", "Packing", &["Random", "Collective"], "Random").help("Collective moves the placed inclusions to open room for more (denser packings, slower)."),
                b("separate_blocks", "One block per inclusion", false).advanced(),
                f("box_ratio", "Box / inclusion size", 1.35, 1.05, 4.0).advanced(),
                i("gap_cells", "Cells between boxes", 2, 0, 100).advanced(),
                i("wall_cells", "Cells from the walls", 1, 0, 100).advanced(),
                i("n_inner", "Layers inside the surface", 2, 1, 50).advanced(),
                i("n_outer", "Layers outside the surface", 2, 1, 50).advanced(),
                f("inner_grading", "Inner grading", 1.0, 0.01, 100.0).advanced(),
                f("outer_grading", "Outer grading", 1.0, 0.01, 100.0).advanced(),
                f("core_fraction", "Core size", 0.55, 0.1, 0.95).advanced(),
                f("core_roundness", "Core roundness", 0.35, 0.0, 1.0).advanced(),
                f("patch_blend", "Surface spacing blend", 0.6, 0.0, 1.0).advanced(),
            ],
            apply: |_, p| {
                let e = p.floats("exponents");
                crate::mesh_gen::inclusion_mesh(&crate::mesh_gen::InclusionParams {
                    length: p.vec3("size", [1.0; 3]),
                    cells: p.i64("cells", 24).max(1) as usize,
                    count: p.i64("count", 8).max(0) as usize,
                    radius_mean: p.f64("radius", 0.075),
                    radius_std: p.f64("radius_std", 0.0),
                    shape: p.str("shape"),
                    aspect: p.vec3("aspect", [1.0, 0.7, 0.5]),
                    exponents: [e.first().copied().unwrap_or(4.0), e.get(1).copied().unwrap_or(4.0)],
                    roughness: p.f64("roughness", 0.12),
                    random_orientation: p.bool("random_orientation", true),
                    seed: p.i64("seed", 7).max(0) as u64,
                    box_ratio: p.f64("box_ratio", 1.35),
                    gap_cells: p.i64("gap_cells", 2).max(0) as usize,
                    wall_cells: p.i64("wall_cells", 1).max(0) as usize,
                    n_inner: p.i64("n_inner", 2).max(1) as usize,
                    n_outer: p.i64("n_outer", 2).max(1) as usize,
                    inner_grading: p.f64("inner_grading", 1.0),
                    outer_grading: p.f64("outer_grading", 1.0),
                    core_fraction: p.f64("core_fraction", 0.55),
                    core_roundness: p.f64("core_roundness", 0.35),
                    patch_blend: p.f64("patch_blend", 0.6),
                    separate_blocks: p.bool("separate_blocks", false),
                    element: p.str("element"),
                    periodic: p.bool("periodic", false),
                    collective: p.str("packing") == "Collective",
                })
            },
        },
        FilterDef {
            id: "plate_mesh",
            label: "Plate Mesh",
            category: "Sources",
            description: "A plate of hexahedra (one through the thickness) with an optional circular or elliptical through-hole meshed by a conforming ring.",
            keywords: &["plate with hole", "notch", "specimen", "stress concentration", "hex mesh", "o-grid", "fracture", "phase field"],
            is_source: true,
            ext: None,
            temporal: None,
            multi: None,
            params: vec![
                v3("size", "Width, height, thickness", [100.0, 50.0, 1.0]),
                f("cell", "Cell size", 2.5, 1e-9, f64::INFINITY).help("The width and height must be whole multiples of it."),
                choice("hole", "Hole", &["None", "Circle", "Ellipse"], "Circle"),
                f("a", "Semi-axis a (radius)", 2.5, 1e-9, f64::INFINITY),
                f("b", "Semi-axis b", 2.5, 1e-9, f64::INFINITY),
                f("angle", "Ellipse angle (degrees)", 0.0, -360.0, 360.0),
                ParamSpec::new("centre", "Hole centre", ParamKind::Floats, ParamValue::Floats(Vec::new())).help("x y of the hole centre; empty: the plate centre."),
                choice("element", "Element", &crate::mesh_gen::ELEMENT_TYPES, "HEX8"),
            ],
            apply: |_, p| {
                let c = p.floats("centre");
                let centre = (c.len() >= 2).then(|| [c[0], c[1]]);
                crate::mesh_gen::plate_mesh(p.vec3("size", [100.0, 50.0, 1.0]), p.f64("cell", 2.5), &p.str("hole"), p.f64("a", 2.5), p.f64("b", 2.5), centre, p.f64("angle", 0.0), &p.str("element"))
            },
        },
        FilterDef {
            id: "dogbone_mesh",
            label: "Dog-bone Mesh",
            category: "Sources",
            description: "A flat dog-bone tensile specimen of hexahedra with shoulder fillets and an optional thickness profile along the gage.",
            keywords: &["tensile specimen", "dogbone", "dog bone", "coupon", "gage", "hex mesh", "test specimen"],
            is_source: true,
            ext: None,
            temporal: None,
            multi: None,
            params: vec![
                f("length", "Total length", 25.4, 1e-9, f64::INFINITY),
                f("gage_length", "Gage length", 8.0, 1e-9, f64::INFINITY),
                f("gage_width", "Gage width", 1.85, 1e-9, f64::INFINITY),
                f("grip_width", "Grip width", 4.8, 1e-9, f64::INFINITY),
                f("fillet_radius", "Fillet radius", 3.175, 1e-9, f64::INFINITY),
                f("thickness", "Grip thickness", 0.35, 1e-9, f64::INFINITY),
                ParamSpec::new("stations", "Thickness stations", ParamKind::Floats, ParamValue::Floats(Vec::new())).help("Pairs: position from the gage start, thickness there (a spline through them along the gage). Empty: constant thickness."),
                f("cell", "Cell size", 0.2, 1e-9, f64::INFINITY),
                f("grip_cell", "Grip cell size", 0.4, 1e-9, f64::INFINITY),
                i("layers", "Layers through the thickness", 1, 1, 100),
                choice("element", "Element", &crate::mesh_gen::ELEMENT_TYPES, "HEX8"),
            ],
            apply: |_, p| {
                let s = p.floats("stations");
                crate::mesh_gen::dogbone_mesh(
                    aurafx_mesh::specimens::DogboneOptions {
                        length: p.f64("length", 25.4),
                        gage_length: p.f64("gage_length", 8.0),
                        gage_width: p.f64("gage_width", 1.85),
                        grip_width: p.f64("grip_width", 4.8),
                        fillet_radius: p.f64("fillet_radius", 3.175),
                        grip_thickness: p.f64("thickness", 0.35),
                        stations: s.chunks_exact(2).map(|c| (c[0], c[1])).collect(),
                        h: p.f64("cell", 0.2),
                        h_grip: Some(p.f64("grip_cell", 0.4)),
                        n_z: p.i64("layers", 1).max(1) as usize,
                        origin: [0.0; 3],
                    },
                    &p.str("element"),
                )
            },
        },
        FilterDef {
            id: "sweep_quads",
            label: "Sweep Quads",
            category: "Mesh",
            description: "Sweep a surface of quadrilaterals into layers of hexahedra along a direction.",
            keywords: &["extrude", "sweep", "2d to 3d", "quad mesh", "hex mesh", "layers"],
            is_source: false,
            ext: None,
            temporal: None,
            multi: None,
            params: vec![
                v3("direction", "Direction", [0.0, 0.0, 1.0]),
                f("distance", "Distance", 1.0, f64::NEG_INFINITY, f64::INFINITY),
                i("layers", "Layers", 4, 1, 100_000),
                f("grading", "Grading (largest/smallest)", 1.0, 0.01, 100.0),
                choice("cluster", "Small layers at", &["Start", "End", "Both"], "Start"),
            ],
            apply: |input, p| crate::mesh_gen::sweep(input.unwrap(), p.vec3("direction", [0.0, 0.0, 1.0]), p.f64("distance", 1.0), p.i64("layers", 4).max(1) as usize, p.f64("grading", 1.0), &p.str("cluster")),
        },
        FilterDef {
            id: "overlay_mesh",
            label: "Overlay Mesh",
            category: "Mesh",
            description: "Hexahedra on a Cartesian grid laid over a labelled image (each label a block, 0 empty) or a closed surface, with smooth pillowed interfaces or whole cells.",
            keywords: &["voxel mesh", "microstructure mesh", "grain mesh", "stl to hex", "conforming hex", "pillow", "stair step", "rve", "image to mesh", "hex mesh"],
            is_source: false,
            ext: None,
            temporal: None,
            multi: None,
            params: vec![
                cell_arr("array", "Labels").help("Image data: the cell array of material or grain numbers (0: empty). Empty: FeatureIds."),
                f("cell", "Cell size", 0.0, 0.0, f64::INFINITY).help("0: the image's own voxels, or a thirtieth of a surface's largest size."),
                choice("mode", "Interfaces", &[crate::mesh_gen::OVERLAY_SMOOTH, crate::mesh_gen::OVERLAY_STAIR], crate::mesh_gen::OVERLAY_SMOOTH),
                b("void", "Mesh the empty space", false),
                b("periodic", "Periodic (pair opposite faces)", false),
                b("sharp", "Keep sharp edges", true).help("Surfaces: nodes go onto the creases between flat patches and onto corners.").advanced(),
                i("min_cells", "Smallest piece (cells)", 8, 0, 1_000_000).help("Pieces of a material with fewer cells join their surroundings.").advanced(),
                i("smoothing", "Smoothing passes", 8, 0, 200).advanced(),
                choice("element", "Element", &crate::mesh_gen::ELEMENT_TYPES, "HEX8").advanced(),
            ],
            apply: |input, p| {
                let array = p.array("array").map(|x| x.1).filter(|x| !x.is_empty()).unwrap_or_else(|| "FeatureIds".into());
                crate::mesh_gen::overlay_mesh(
                    input.unwrap(),
                    &crate::mesh_gen::OverlayParams {
                        array,
                        cell: p.f64("cell", 0.0),
                        mode: p.str("mode"),
                        void: p.bool("void", false),
                        periodic: p.bool("periodic", false),
                        min_cells: p.i64("min_cells", 8).max(0) as usize,
                        smoothing: p.i64("smoothing", 8).max(0) as usize,
                        element: p.str("element"),
                        sharp: p.bool("sharp", true),
                    },
                )
            },
        },
        FilterDef {
            id: "refine_mesh",
            label: "Refine Mesh",
            category: "Mesh",
            description: "Split every element of a hexahedral or tetrahedral mesh into eight, keeping blocks, side sets and node sets.",
            keywords: &["refine", "subdivide", "uniform refinement", "8x", "h-refinement", "finer mesh", "split elements"],
            is_source: false,
            ext: None,
            temporal: None,
            multi: None,
            params: vec![i("levels", "Levels", 1, 1, 4)],
            apply: |input, p| crate::mesh_gen::refine_mesh(input.unwrap(), p.i64("levels", 1).max(1) as usize),
        },
        FilterDef {
            id: "convert_elements",
            label: "Convert Elements",
            category: "Mesh",
            description: "Hexahedra into tetrahedra (12 each, conforming) or tetrahedra into hexahedra (4 each), keeping blocks and sets.",
            keywords: &["htet", "thex", "hex to tet", "tet to hex", "tetrahedralize", "split hexahedra", "convert mesh"],
            is_source: false,
            ext: None,
            temporal: None,
            multi: None,
            params: vec![choice("to", "Convert", &[crate::mesh_gen::TO_TETS, crate::mesh_gen::TO_HEXES], crate::mesh_gen::TO_TETS)],
            apply: |input, p| crate::mesh_gen::convert_elements(input.unwrap(), &p.str("to")),
        },
        FilterDef {
            id: "smooth_mesh",
            label: "Smooth Mesh",
            category: "Mesh",
            description: "Move the inner nodes of a hexahedral or tetrahedral mesh to better its elements: Laplacian, guarded Laplacian or condition-number optimisation; the boundary stays put.",
            keywords: &["mesh smoothing", "laplacian", "smart laplacian", "condition number", "optimize mesh", "untangle", "improve quality"],
            is_source: false,
            ext: None,
            temporal: None,
            multi: None,
            params: vec![
                choice("scheme", "Scheme", &[aurafx_mesh::improve::SMOOTH_SMART, aurafx_mesh::improve::SMOOTH_LAPLACIAN, aurafx_mesh::improve::SMOOTH_CONDITION], aurafx_mesh::improve::SMOOTH_SMART),
                i("iterations", "Iterations", 10, 1, 1000),
                s("fixed", "Also hold the node set", "").advanced(),
            ],
            apply: |input, p| crate::mesh_gen::smooth_mesh(input.unwrap(), &p.str("scheme"), p.i64("iterations", 10).max(1) as usize, &p.str("fixed")),
        },
        FilterDef {
            id: "boundary_layers",
            label: "Boundary Layers",
            category: "Mesh",
            description: "Insert thin layers of hexahedra, growing in thickness, under the faces of a side set (for walls in flow or contact problems).",
            keywords: &["boundary layer", "inflation layers", "prism layers", "wall layers", "y plus", "near wall", "grading"],
            is_source: false,
            ext: None,
            temporal: None,
            multi: None,
            params: vec![
                s("side_set", "Side set", ""),
                i("layers", "Layers", 3, 1, 50),
                f("first", "First layer thickness", 0.01, 1e-12, f64::INFINITY),
                f("growth", "Growth ratio", 1.3, 1.0, 10.0),
            ],
            apply: |input, p| crate::mesh_gen::boundary_layers(input.unwrap(), &p.str("side_set"), p.i64("layers", 3).max(1) as usize, p.f64("first", 0.01), p.f64("growth", 1.3)),
        },
        FilterDef {
            id: "tet_mesh",
            label: "Tet Mesh",
            category: "Mesh",
            description: "Fill a closed surface (STL, OBJ, PLY ...) with well-shaped tetrahedra of one size, a side set per patch of the surface.",
            keywords: &["tetrahedral mesh", "tetrahedralize", "stl to tet", "volume mesh", "isosurface stuffing", "tet4", "fem mesh"],
            is_source: false,
            ext: None,
            temporal: None,
            multi: None,
            params: vec![f("cell", "Element size", 0.0, 0.0, f64::INFINITY).help("0: a thirtieth of the surface's largest size.")],
            apply: |input, p| crate::mesh_gen::tet_mesh(input.unwrap(), p.f64("cell", 0.0)),
        },
        FilterDef {
            id: "refine_region",
            label: "Refine Region",
            category: "Mesh",
            description: "Split the tetrahedra inside a box (or where a cell array reaches a value) into eight, keeping the mesh conforming around them.",
            keywords: &["local refinement", "adaptive", "refine box", "refine where", "h-refinement", "tetrahedra"],
            is_source: false,
            ext: None,
            temporal: None,
            multi: None,
            params: vec![
                v3("box_min", "Box min", [-1e30; 3]),
                v3("box_max", "Box max", [1e30; 3]),
                cell_arr("array", "Or where this cell array").advanced(),
                f("value", "is at least", 1.0, f64::NEG_INFINITY, f64::INFINITY).advanced(),
            ],
            apply: |input, p| {
                let arr = p.array("array").map(|x| x.1).unwrap_or_default();
                crate::mesh_gen::refine_region(input.unwrap(), p.vec3("box_min", [-1e30; 3]), p.vec3("box_max", [1e30; 3]), &arr, p.f64("value", 1.0))
            },
        },
        FilterDef {
            id: "elevate",
            label: "Elevate",
            category: "Mesh",
            description: "Turn 8-node hexahedra into 20- or 27-node ones (shared mid-edge, mid-face and centre nodes).",
            keywords: &["quadratic", "higher order", "hex20", "hex27", "serendipity", "promote"],
            is_source: false,
            ext: None,
            temporal: None,
            multi: None,
            params: vec![choice("element", "Element", &crate::mesh_gen::ELEMENT_TYPES[1..], "HEX27")],
            apply: |input, p| crate::mesh_gen::elevate(input.unwrap(), &p.str("element")),
        },
        FilterDef {
            id: "extract_set",
            label: "Extract Set",
            category: "Mesh",
            description: "A side set's faces as a surface, or a node set's nodes as points, by name.",
            keywords: &["side set", "node set", "boundary", "surface", "sideset", "nodeset", "boundary condition"],
            is_source: false,
            ext: None,
            temporal: None,
            multi: None,
            params: vec![s("set", "Set name", "interface").help("The name of a side set or node set (for example x-, interface).")],
            apply: |input, p| crate::mesh_gen::extract_set(input.unwrap(), &p.str("set")),
        },
    ]
}

const GEOMETRY_TEMPLATE: &str = "# A block with a hole and a rounded boss.\nbrick x 4 y 2 z 1\ncylinder radius 0.5 height 2\nsubtract body 2 from body 1\noutline 1 rectangle x 1 y 1\noutline 1 fillet radius 0.3\nextrude outline 1 height 1\nmove body 3 x 1.4 z 0.5\nunite body 1 3\n";

/// Patches of the OpenFOAM case an internal mesh was read from, at the mesh's time.
fn openfoam_patches(input: &DataSet, names: &str) -> Result<DataSet> {
    let fd = input.field_data();
    let case = fd
        .get("CasePath")
        .and_then(|a| match &a.data {
            aurafx_data::ArrayData::String(v) => v.first().cloned(),
            _ => None,
        })
        .ok_or_else(|| anyhow::anyhow!("the input is not an OpenFOAM case (apply this to the .foam reader)"))?;
    let time = fd.get("TimeValue").map(|a| a.get(0, 0));
    let names: Vec<String> = names.split(',').map(|n| n.trim().to_string()).filter(|n| !n.is_empty()).collect();
    let mb = aurafx_io::openfoam::read_patches(std::path::Path::new(&case), time, &names)?;
    // Like the internal mesh, also provide point data averaged from the faces.
    per_leaf(&DataSet::MultiBlock(mb), &|d| crate::attributes::cell_to_point(d, &[]))
}

static REGISTRY: OnceLock<RwLock<Arc<Vec<Arc<FilterDef>>>>> = OnceLock::new();

fn table() -> &'static RwLock<Arc<Vec<Arc<FilterDef>>>> {
    REGISTRY.get_or_init(|| RwLock::new(Arc::new(build().into_iter().map(Arc::new).collect())))
}

/// Every filter and source: the built-in ones, then those added by plugins.
pub fn registry() -> Arc<Vec<Arc<FilterDef>>> {
    table().read().unwrap().clone()
}

/// Add (or replace) a filter defined at run time. Built-in ids cannot be replaced.
pub fn register(def: FilterDef) -> Result<()> {
    if def.ext.is_none() {
        return Err(anyhow!("only plugin filters can be registered at run time"));
    }
    let mut t = table().write().unwrap();
    if t.iter().any(|f| f.id == def.id && f.ext.is_none()) {
        return Err(anyhow!("\"{}\" is the id of a built-in filter; choose another name", def.id));
    }
    let mut v: Vec<Arc<FilterDef>> = t.iter().filter(|f| f.id != def.id).cloned().collect();
    v.push(Arc::new(def));
    *t = Arc::new(v);
    Ok(())
}

/// Remove every filter a plugin file registered; returns their ids.
pub fn unregister_plugin(plugin: &str) -> Vec<String> {
    let mut t = table().write().unwrap();
    let gone: Vec<String> = t.iter().filter(|f| f.ext.as_ref().is_some_and(|e| e.plugin == plugin)).map(|f| f.id.to_string()).collect();
    if !gone.is_empty() {
        *t = Arc::new(t.iter().filter(|f| !f.ext.as_ref().is_some_and(|e| e.plugin == plugin)).cloned().collect());
    }
    gone
}

pub fn find(id: &str) -> Option<Arc<FilterDef>> {
    let idl = id.to_ascii_lowercase().replace([' ', '-'], "_");
    registry().iter().find(|f| f.id == idl || f.label.eq_ignore_ascii_case(id)).cloned()
}

/// Search filters and sources, best match first.
pub fn search(query: &str) -> Vec<Arc<FilterDef>> {
    let mut v: Vec<(u32, Arc<FilterDef>)> = registry().iter().map(|f| (f.search_score(query), f.clone())).filter(|(s, _)| *s > 0).collect();
    v.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.label.cmp(b.1.label)));
    v.into_iter().map(|(_, f)| f).collect()
}

/// The parameters choosing the orientations of a texture filter.
fn texture_selection_params(max: i64) -> Vec<ParamSpec> {
    vec![
        cell_arr("orientations", "Orientations").help("Euler angles, quaternions or orientation matrices. Empty: EulerAngles, Quats or OrientationMatrix."),
        choice("structure", "Crystal structure", &crate::crystal::structure_choices(), crate::crystal::FROM_PHASES),
        i("phase", "Phase", 1, 0, 255).help("The phase analysed (0: every indexed point, which must all be of one crystal class)."),
        cell_arr("phases", "Phases").help("Empty: Phases.").advanced(),
        cell_arr("mask", "Mask").help("Points where this array is 0 are left out.").advanced(),
        i("max_orientations", "Orientations used at most", max, 100, 10_000_000).help("A random subset (fixed seed) when there are more.").advanced(),
    ]
}

fn texture_selection<'a>(p: &crate::params::Params, structure: &'a str, phases: Option<&'a str>, mask: Option<&'a str>, max: i64) -> crate::crystal_texture::Selection<'a> {
    crate::crystal_texture::Selection {
        orientations: p.array("orientations"),
        structure,
        phases,
        phase: p.i64("phase", 1),
        mask,
        max: p.i64("max_orientations", max).max(1) as usize,
        seed: 1,
    }
}
