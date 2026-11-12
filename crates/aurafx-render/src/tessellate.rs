//! Convert datasets to GPU-friendly vertex streams according to display properties.

use aurafx_data::*;
use aurafx_filters::Association;
use aurafx_pipeline::{DisplayProps, Representation};
use bytemuck::{Pod, Zeroable};
use rayon::prelude::*;

#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable, Default)]
pub struct Vertex {
    pub pos: [f32; 3],
    pub scalar: f32,
    pub normal: [f32; 3],
    pub _pad: f32,
}

/// CPU-side renderable geometry.
#[derive(Debug, Default, Clone)]
pub struct MeshData {
    pub tris: Vec<Vertex>,
    pub lines: Vec<Vertex>,
    pub points: Vec<Vertex>,
    /// Range of the scalar used for coloring, if any.
    pub scalar_range: Option<(f64, f64)>,
    pub has_scalars: bool,
    pub bounds: Bounds,
    /// Local origin subtracted from all positions (for precision with large coordinates).
    pub origin: [f64; 3],
    pub volume: Option<VolumeData>,
    /// Surface LIC: the vector at each vertex of `tris` (empty for other representations).
    pub lic: Vec<[f32; 4]>,
    /// The vertices carry texture coordinates (packed in `scalar`) for the display's image.
    pub textured: bool,
    /// The vertices carry their own RGB colors, packed in `scalar` as r * 65536 + g * 256 + b.
    pub direct_rgb: bool,
    /// Polygon edges of Surface With Edges (drawn in the edge color).
    pub edges: Vec<Vertex>,
    /// 3D Glyphs: one shape drawn at every instance.
    pub glyphs: Option<GlyphData>,
}

/// The 3D Glyphs representation: a unit shape (pointing along +x, length 1) and, per point,
/// [x, y, z, value, dx, dy, dz, size] (position relative to `MeshData::origin`, unit direction).
#[derive(Debug, Clone, Default)]
pub struct GlyphData {
    pub shape: Vec<Vertex>,
    pub instances: Vec<[f32; 8]>,
    /// Every `stride`-th point has a glyph (1: every point; more when the data has more than
    /// [`MAX_GLYPHS`] points).
    pub stride: usize,
}

/// The most glyphs drawn for one object; data with more points shows every n-th point.
pub const MAX_GLYPHS: usize = 2_000_000;

/// What the glyphs of all blocks of an object share: the largest magnitude of the scaling
/// array, the size of the largest glyph, and which points get one.
struct GlyphPlan {
    max_mag: f64,
    size: f64,
    stride: usize,
    /// Points passed so far (over all blocks).
    seen: usize,
}

fn glyph_magnitude(a: &aurafx_data::Array, i: usize) -> f64 {
    if a.ncomp == 1 {
        a.get(i, 0).abs()
    } else {
        a.magnitude(i)
    }
}

impl GlyphPlan {
    fn new<'a>(blocks: impl Iterator<Item = &'a PolyData>, display: &DisplayProps, bounds: &Bounds) -> Self {
        let (mut n, mut max_mag, mut scaled) = (0usize, 0.0f64, false);
        for pd in blocks {
            n += pd.points.len();
            if let Some(a) = Some(display.glyph_scale_by.as_str()).filter(|s| !s.is_empty()).and_then(|s| pd.point_data.get(s)) {
                scaled = true;
                max_mag = (0..a.len()).map(|i| glyph_magnitude(a, i)).filter(|m| m.is_finite()).fold(max_mag, f64::max);
            }
        }
        let stride = n.div_ceil(MAX_GLYPHS).max(1);
        let shown = n.div_ceil(stride);
        // Without a size: about the spacing of the glyphs if they filled the bounds.
        let size = if display.glyph_scale > 0.0 { display.glyph_scale as f64 } else { bounds.diagonal() / (1.7 * (shown as f64).cbrt()).max(1.0) };
        Self { max_mag: if scaled { max_mag.max(1e-300) } else { 1.0 }, size, stride, seen: 0 }
    }
}

/// A scalar field on a uniform grid for ray casting. Values are normalized to [0, 1] over `range`
/// (NaN marks samples outside the data).
#[derive(Debug, Clone)]
pub struct VolumeData {
    pub dims: [u32; 3],
    /// Box corners relative to `MeshData::origin`.
    pub box_min: [f32; 3],
    pub box_max: [f32; 3],
    pub values: Vec<half::f16>,
}

/// Build volume data: image data is used directly, anything else is resampled.
fn build_volume(ds: &DataSet, display: &DisplayProps, origin: [f64; 3]) -> Option<(VolumeData, (f64, f64))> {
    let img: ImageData = match ds {
        DataSet::ImageData(i) => i.clone(),
        other => {
            let b = other.bounds();
            if b.is_empty() {
                return None;
            }
            let s = b.size();
            let mx = s.iter().cloned().fold(0.0, f64::max).max(1e-30);
            let target = 128.0;
            let dims = [
                ((s[0] / mx) * target).ceil().max(2.0) as usize,
                ((s[1] / mx) * target).ceil().max(2.0) as usize,
                ((s[2] / mx) * target).ceil().max(2.0) as usize,
            ];
            aurafx_filters::probe::resample_to_image(other, dims).ok()?
        }
    };
    let (name, comp) = match &display.color_by {
        Some(cb) => (cb.name.clone(), cb.component),
        None => (aurafx_filters::default_array(&DataSet::ImageData(img.clone()), false)?.1, None),
    };
    // Cell data on images is converted to points by averaging.
    let img = if img.point_data.get(&name).is_none() {
        match aurafx_filters::attributes::cell_to_point(&DataSet::ImageData(img.clone()), &[name.clone()]).ok()? {
            DataSet::UnstructuredGrid(u) => ImageData { point_data: u.point_data, ..img },
            _ => img,
        }
    } else {
        img
    };
    let arr = img.point_data.get(&name)?;
    let vals = arr.component_f32(comp.filter(|&c| c < arr.ncomp));
    let (lo, hi) = display.range.unwrap_or_else(|| arr.range(comp.filter(|&c| c < arr.ncomp)));
    let d = img.dims();
    let mask = img.point_data.get("vtkValidPointMask");
    let span = (hi - lo).max(1e-300);
    let values: Vec<half::f16> = vals
        .iter()
        .enumerate()
        .map(|(i, &v)| {
            if mask.map(|m| m.get(i, 0) == 0.0).unwrap_or(false) || !v.is_finite() {
                half::f16::NAN
            } else {
                half::f16::from_f64(((v as f64 - lo) / span).clamp(0.0, 1.0))
            }
        })
        .collect();
    let b = img.bounds();
    let rel = |p: [f64; 3]| [(p[0] - origin[0]) as f32, (p[1] - origin[1]) as f32, (p[2] - origin[2]) as f32];
    Some((VolumeData { dims: [d[0] as u32, d[1] as u32, d[2] as u32], box_min: rel(b.min), box_max: rel(b.max), values }, (lo, hi)))
}

struct Leaf {
    pd: PolyData,
    /// Index of this leaf's first cell in the flattened numbering used by selections.
    cell_offset: usize,
}

/// Collect renderable leaves: everything is turned into polydata (surfaces) first.
fn leaves(ds: &DataSet, repr: Representation, level: u32) -> Vec<Leaf> {
    leaves_at(ds, repr, level, 0)
}

fn leaves_at(ds: &DataSet, repr: Representation, level: u32, cell_offset: usize) -> Vec<Leaf> {
    // Glyphs sit on every point, inside too.
    if repr == Representation::Glyphs && !matches!(ds, DataSet::MultiBlock(_) | DataSet::Table(_) | DataSet::PolyData(_)) {
        let pd = PolyData {
            points: ds.points().unwrap_or_else(aurafx_data::Points::empty),
            point_data: ds.point_data().cloned().unwrap_or_default(),
            ..Default::default()
        };
        return vec![Leaf { pd, cell_offset }];
    }
    match ds {
        DataSet::MultiBlock(mb) => {
            let mut off = cell_offset;
            let mut out = Vec::new();
            for (_, d) in mb.leaves() {
                out.extend(leaves_at(d, repr, level, off));
                off += d.n_cells();
            }
            out
        }
        DataSet::Table(_) => vec![],
        DataSet::PolyData(pd) => vec![Leaf { pd: pd.clone(), cell_offset }],
        other => match aurafx_filters::surface::extract_surface_level(other, level) {
            Ok(pd) => vec![Leaf { pd, cell_offset }],
            Err(e) => {
                tracing::warn!("surface extraction failed: {e:#}");
                vec![]
            }
        },
    }
}

/// The texture coordinates of a point set: the active ones, else a 2-component point array
/// named like texture coordinates.
pub fn tcoords(attrs: &Attributes) -> Option<&aurafx_data::Array> {
    let ok = |a: &&aurafx_data::Array| a.ncomp >= 2 && a.scalar_type() != ScalarType::String;
    attrs.active(aurafx_data::AttributeKind::TCoords).filter(ok).or_else(|| {
        attrs.iter().filter(ok).find(|a| {
            let n = a.name.to_ascii_lowercase();
            a.ncomp == 2 && (n.contains("texture") || n.contains("tcoord"))
        })
    })
}

/// Texture coordinates packed as two halves into one float (unpacked by the shader).
fn packed_tcoords(a: &aurafx_data::Array) -> Vec<f32> {
    (0..a.len())
        .map(|i| {
            let (u, v) = (half::f16::from_f64(a.get(i, 0)).to_bits() as u32, half::f16::from_f64(a.get(i, 1)).to_bits() as u32);
            f32::from_bits(u | (v << 16))
        })
        .collect()
}

/// The vertex scalar of a point or cell without a value (NaN, missing). A finite sentinel
/// rather than NaN: some GPU drivers do not carry NaN through vertex interpolation, so the
/// shaders test for this value (`s < -1e37`) as well as for NaN.
pub const NO_VALUE: f32 = -3.0e38;

fn no_value(s: f32) -> f32 {
    if s.is_finite() {
        s
    } else {
        NO_VALUE
    }
}

/// Per-point scalar values (component or magnitude) for coloring, or None.
/// An array drawn as colors: 8-bit unsigned with 3 or 4 components.
fn is_rgb(a: &aurafx_data::Array) -> bool {
    a.scalar_type() == ScalarType::U8 && (a.ncomp == 3 || a.ncomp == 4)
}

/// Each tuple's RGB packed into an integer-valued float (exact in f32): r * 65536 + g * 256 + b.
fn packed_rgb(a: &aurafx_data::Array) -> Vec<f32> {
    (0..a.len()).map(|i| (a.get(i, 0) as u32 * 65536 + a.get(i, 1) as u32 * 256 + a.get(i, 2) as u32) as f32).collect()
}

fn point_scalars(attrs: &Attributes, name: &str, comp: Option<usize>) -> Option<Vec<f32>> {
    attrs.get(name).filter(|a| a.scalar_type() != ScalarType::String).map(|a| a.component_f32(comp.filter(|&c| c < a.ncomp)))
}

/// Triangulate polygons and strips: (point ids, owning cell index).
fn triangulate(pd: &PolyData, cell_base: usize) -> Vec<([u32; 3], usize)> {
    let mut tris = Vec::with_capacity(pd.polys.len() * 2);
    for i in 0..pd.polys.len() {
        let c = pd.polys.cell(i);
        // Pixels list their corners x first, then y: split along the diagonal 1-2.
        if pd.polys.types.get(i) == Some(&CellType::Pixel) && c.len() == 4 {
            tris.push(([c[0], c[1], c[3]], cell_base + i));
            tris.push(([c[0], c[3], c[2]], cell_base + i));
            continue;
        }
        for k in 1..c.len().saturating_sub(1) {
            tris.push(([c[0], c[k], c[k + 1]], cell_base + i));
        }
    }
    let strip_base = cell_base + pd.polys.len();
    for i in 0..pd.strips.len() {
        let c = pd.strips.cell(i);
        for k in 0..c.len().saturating_sub(2) {
            let t = if k % 2 == 0 { [c[k], c[k + 1], c[k + 2]] } else { [c[k + 1], c[k], c[k + 2]] };
            tris.push((t, strip_base + i));
        }
    }
    tris
}

/// The vector array a Surface LIC shows: the named one (point or cell data), else the first
/// 3-component point array, else the first 3-component cell array.
pub fn lic_array<'a>(pd: &'a PolyData, name: Option<&str>) -> Option<(&'a aurafx_data::Array, bool)> {
    let three = |a: &&aurafx_data::Array| a.ncomp == 3 && a.scalar_type() != ScalarType::String;
    match name {
        Some(n) => pd.point_data.get(n).filter(three).map(|a| (a, true)).or_else(|| pd.cell_data.get(n).filter(three).map(|a| (a, false))),
        None => pd.point_data.iter().find(three).map(|a| (a, true)).or_else(|| pd.cell_data.iter().find(three).map(|a| (a, false))),
    }
}

/// The LIC vector at each corner of the triangles `emit_triangles` makes (same order).
fn emit_lic_vectors(pd: &PolyData, cell_base: usize, name: Option<&str>, out: &mut Vec<[f32; 4]>) {
    let tris = triangulate(pd, cell_base);
    let Some((a, on_points)) = lic_array(pd, name) else {
        out.extend(std::iter::repeat_n([0.0; 4], tris.len() * 3));
        return;
    };
    let at = |i: usize| [a.get(i, 0) as f32, a.get(i, 1) as f32, a.get(i, 2) as f32, 0.0];
    out.reserve(tris.len() * 3);
    for (t, cell) in &tris {
        for &p in t {
            out.push(if on_points { at(p as usize) } else { at(*cell) });
        }
    }
}

/// Emit triangles with crease-angle normals: a corner averages the normals of incident
/// triangles that are within `crease_deg` of its own face, so curved surfaces look smooth
/// while hard edges (e.g. box corners) stay sharp.
fn emit_triangles(
    pd: &PolyData,
    origin: [f64; 3],
    pscal: Option<&[f32]>,
    cscal: Option<&[f32]>,
    cell_base: usize,
    crease_deg: Option<f64>,
    cell_ids: &(dyn Fn(usize) -> u32 + Sync),
    out: &mut Vec<Vertex>,
) {
    let tris = triangulate(pd, cell_base);
    if tris.is_empty() {
        return;
    }
    let pos: Vec<[f32; 3]> = (0..pd.points.len())
        .into_par_iter()
        .map(|i| {
            let p = pd.points.get(i);
            [(p[0] - origin[0]) as f32, (p[1] - origin[1]) as f32, (p[2] - origin[2]) as f32]
        })
        .collect();
    // Unit face normals.
    let fnorm: Vec<[f32; 3]> = tris
        .par_iter()
        .map(|(t, _)| {
            let (a, b, c) = (pos[t[0] as usize], pos[t[1] as usize], pos[t[2] as usize]);
            let u = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
            let v = [c[0] - a[0], c[1] - a[1], c[2] - a[2]];
            let n = [u[1] * v[2] - u[2] * v[1], u[2] * v[0] - u[0] * v[2], u[0] * v[1] - u[1] * v[0]];
            let l = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
            if l > 0.0 {
                [n[0] / l, n[1] / l, n[2] / l]
            } else {
                [0.0, 0.0, 0.0]
            }
        })
        .collect();
    // Point -> incident triangles (CSR).
    let np = pos.len();
    let (offsets, adj) = match crease_deg {
        Some(_) => {
            let mut counts = vec![0u32; np + 1];
            for (t, _) in &tris {
                for &p in t {
                    counts[p as usize + 1] += 1;
                }
            }
            for i in 0..np {
                counts[i + 1] += counts[i];
            }
            let mut fill = counts.clone();
            let mut adj = vec![0u32; counts[np] as usize];
            for (ti, (t, _)) in tris.iter().enumerate() {
                for &p in t {
                    adj[fill[p as usize] as usize] = ti as u32;
                    fill[p as usize] += 1;
                }
            }
            (counts, adj)
        }
        None => (Vec::new(), Vec::new()),
    };
    let cos_crease = crease_deg.map(|d| d.to_radians().cos() as f32).unwrap_or(1.0);
    let verts: Vec<[Vertex; 3]> = tris
        .par_iter()
        .enumerate()
        .map(|(ti, (t, cell))| {
            let fnrm = fnorm[ti];
            let mut vs = [Vertex::default(); 3];
            for k in 0..3 {
                let p = t[k] as usize;
                let normal = if crease_deg.is_some() {
                    let mut acc = [0.0f32; 3];
                    for &oj in &adj[offsets[p] as usize..offsets[p + 1] as usize] {
                        let on = fnorm[oj as usize];
                        // Orientation-insensitive: treat flipped neighbours as the same surface.
                        let d = on[0] * fnrm[0] + on[1] * fnrm[1] + on[2] * fnrm[2];
                        if d.abs() >= cos_crease {
                            let sgn = if d < 0.0 { -1.0 } else { 1.0 };
                            acc[0] += sgn * on[0];
                            acc[1] += sgn * on[1];
                            acc[2] += sgn * on[2];
                        }
                    }
                    let l = (acc[0] * acc[0] + acc[1] * acc[1] + acc[2] * acc[2]).sqrt();
                    if l > 0.0 {
                        [acc[0] / l, acc[1] / l, acc[2] / l]
                    } else {
                        fnrm
                    }
                } else {
                    fnrm
                };
                let s = match (cscal, pscal) {
                    (Some(c), _) => no_value(c.get(*cell).copied().unwrap_or(f32::NAN)),
                    (None, Some(ps)) => no_value(ps.get(p).copied().unwrap_or(f32::NAN)),
                    _ => 0.0,
                };
                vs[k] = Vertex { pos: pos[p], scalar: s, normal, _pad: f32::from_bits(cell_ids(*cell)) };
            }
            vs
        })
        .collect();
    out.reserve(verts.len() * 3);
    for v in verts {
        out.extend_from_slice(&v);
    }
}

fn emit_lines(pd: &PolyData, origin: [f64; 3], pscal: Option<&[f32]>, cscal: Option<&[f32]>, cell_base: usize, out: &mut Vec<Vertex>) {
    let pt = |i: u32| {
        let p = pd.points.get(i as usize);
        [(p[0] - origin[0]) as f32, (p[1] - origin[1]) as f32, (p[2] - origin[2]) as f32]
    };
    for i in 0..pd.lines.len() {
        let c = pd.lines.cell(i);
        for w in c.windows(2) {
            for &id in w {
                let s = match (cscal, pscal) {
                    (Some(cs), _) => no_value(cs.get(cell_base + i).copied().unwrap_or(f32::NAN)),
                    (None, Some(ps)) => no_value(ps.get(id as usize).copied().unwrap_or(f32::NAN)),
                    _ => 0.0,
                };
                out.push(Vertex { pos: pt(id), scalar: s, normal: [0.0, 0.0, 1.0], _pad: 0.0 });
            }
        }
    }
}

/// Edges of polygons (unique), as line vertices.
fn emit_polygon_edges(pd: &PolyData, origin: [f64; 3], pscal: Option<&[f32]>, out: &mut Vec<Vertex>) {
    use std::collections::HashSet;
    let mut seen: HashSet<(u32, u32)> = HashSet::new();
    let pt = |i: u32| {
        let p = pd.points.get(i as usize);
        [(p[0] - origin[0]) as f32, (p[1] - origin[1]) as f32, (p[2] - origin[2]) as f32]
    };
    for cells in [&pd.polys, &pd.strips] {
        for i in 0..cells.len() {
            let c = cells.cell(i);
            let n = c.len();
            for k in 0..n {
                let (a, b) = (c[k], c[(k + 1) % n]);
                let key = if a < b { (a, b) } else { (b, a) };
                if seen.insert(key) {
                    for id in [a, b] {
                        let s = pscal.and_then(|p| p.get(id as usize).copied()).unwrap_or(0.0);
                        out.push(Vertex { pos: pt(id), scalar: s, normal: [0.0, 0.0, 1.0], _pad: 0.0 });
                    }
                }
            }
        }
    }
}

fn emit_points(pd: &PolyData, origin: [f64; 3], pscal: Option<&[f32]>, cscal: Option<&[f32]>, out: &mut Vec<Vertex>) {
    let n = pd.points.len();
    // If there are vertex cells, cell scalars map through them; otherwise every point.
    if cscal.is_some() && pd.verts.len() > 0 {
        for i in 0..pd.verts.len() {
            for &id in pd.verts.cell(i) {
                let p = pd.points.get(id as usize);
                out.push(Vertex {
                    pos: [(p[0] - origin[0]) as f32, (p[1] - origin[1]) as f32, (p[2] - origin[2]) as f32],
                    scalar: cscal.unwrap()[i],
                    normal: [0.0, 0.0, 1.0],
                    _pad: 0.0,
                });
            }
        }
        return;
    }
    out.reserve(n);
    for i in 0..n {
        let p = pd.points.get(i);
        let s = pscal.and_then(|ps| ps.get(i).copied()).unwrap_or(0.0);
        out.push(Vertex {
            pos: [(p[0] - origin[0]) as f32, (p[1] - origin[1]) as f32, (p[2] - origin[2]) as f32],
            scalar: s,
            normal: [0.0, 0.0, 1.0],
            _pad: 0.0,
        });
    }
}

/// Build renderable geometry for a dataset.
pub fn tessellate(ds: &DataSet, display: &DisplayProps) -> MeshData {
    // Cell grids are drawn as their cells with the fields at the corners.
    if let DataSet::CellGrid(g) = ds {
        return tessellate(&DataSet::UnstructuredGrid(g.to_unstructured()), display);
    }
    // Hyper tree grids with material interfaces are drawn cut to their material, as ParaView
    // draws them (through Hyper Tree Grid Geometry).
    if let DataSet::HyperTreeGrid(h) = ds {
        if h.interface.is_some() && !matches!(display.representation, Representation::Volume | Representation::Outline) {
            if let Ok((mut pd, sources)) = aurafx_filters::htg::geometry_with_sources(h, true) {
                // Picking and selection number the visible leaves (as the leaf mesh does).
                let leaf_of: std::collections::HashMap<usize, usize> = h.leaves_mesh().1.into_iter().enumerate().map(|(i, g)| (g, i)).collect();
                let ids: Vec<i64> = sources.iter().map(|g| leaf_of.get(g).map(|&i| i as i64).unwrap_or(0)).collect();
                pd.cell_data.insert(aurafx_data::Array::new("vtkOriginalCellIds", 1, aurafx_data::ArrayData::I64(ids)));
                return tessellate(&DataSet::PolyData(pd), display);
            }
        }
    }
    let bounds = ds.bounds();
    let origin = if bounds.is_empty() { [0.0; 3] } else { bounds.center() };
    let mut mesh = MeshData { bounds, origin, ..Default::default() };
    if display.representation == Representation::Volume {
        let leaf = match ds {
            DataSet::MultiBlock(_) => {
                aurafx_filters::find("merge_blocks").and_then(|f| f.run(Some(ds), &f.default_params()).ok()).unwrap_or_else(|| ds.clone())
            }
            d => d.clone(),
        };
        if let Some((v, r)) = build_volume(&leaf, display, origin) {
            mesh.volume = Some(v);
            mesh.scalar_range = Some(r);
            mesh.has_scalars = true;
        }
        return mesh;
    }
    if display.representation == Representation::Outline {
        // AMR data shows the outline of every box (the refinement), as in ParaView.
        let boxes: Vec<&DataSet> = match ds {
            DataSet::MultiBlock(mb) if aurafx_data::amr::is_amr(ds) => mb.leaves().into_iter().map(|(_, d)| d).collect(),
            d => vec![d],
        };
        for b in boxes {
            let pd = aurafx_filters::outline::outline(b);
            emit_lines(&pd, origin, None, None, 0, &mut mesh.lines);
        }
        return mesh;
    }
    // An image on the surface replaces the array colors where there are texture coordinates.
    if display.texture.as_deref().is_some_and(|t| !t.trim().is_empty()) {
        let has = |d: &DataSet| d.point_data().is_some_and(|a| tcoords(a).is_some());
        mesh.textured = match ds {
            DataSet::MultiBlock(mb) => mb.leaves().into_iter().any(|(_, d)| has(d)),
            d => has(d),
        };
    }
    // 8-bit RGB(A) arrays drawn as their own colors.
    if let Some(cb) = display.color_by.as_ref().filter(|cb| display.direct_colors && !mesh.textured && cb.component.is_none()) {
        let rgb = |d: &DataSet| {
            let attrs = match cb.assoc {
                Association::Points => d.point_data(),
                Association::Cells => d.cell_data(),
            };
            attrs.and_then(|x| x.get(&cb.name)).is_some_and(is_rgb)
        };
        mesh.direct_rgb = match ds {
            DataSet::MultiBlock(mb) => {
                let leaves = mb.leaves();
                !leaves.is_empty() && leaves.into_iter().all(|(_, d)| rgb(d))
            }
            d => rgb(d),
        };
    }
    // Global scalar range for consistent coloring across leaves.
    if let Some(cb) = display.color_by.as_ref().filter(|_| !mesh.textured && !mesh.direct_rgb) {
        let mut lo = f64::INFINITY;
        let mut hi = f64::NEG_INFINITY;
        let mut visit = |d: &DataSet| {
            let attrs = match cb.assoc {
                Association::Points => d.point_data(),
                Association::Cells => d.cell_data(),
            };
            if let Some(a) = attrs.and_then(|x| x.get(&cb.name)) {
                let (l, h) = a.range(cb.component.filter(|&c| c < a.ncomp));
                lo = lo.min(l);
                hi = hi.max(h);
            }
        };
        match ds {
            DataSet::MultiBlock(mb) => mb.leaves().into_iter().for_each(|(_, d)| visit(d)),
            d => visit(d),
        }
        if lo <= hi {
            mesh.scalar_range = Some((lo, hi));
            mesh.has_scalars = true;
        }
    }
    if let Some(r) = display.range {
        if mesh.has_scalars {
            mesh.scalar_range = Some(r);
        }
    }
    // Element colors: each atomic number owns its own entries of the color map.
    if display.colormap == aurafx_pipeline::colormap::ELEMENTS && display.colormap_stops.is_none() && mesh.has_scalars && display.range.is_none() {
        mesh.scalar_range = Some((-0.5, 127.5));
    }

    let all = leaves(ds, display.representation, display.nonlinear_subdivision.min(6));
    // 3D Glyphs: one size and one largest magnitude for all blocks.
    let mut glyph_plan = (display.representation == Representation::Glyphs).then(|| GlyphPlan::new(all.iter().map(|l| &l.pd), display, &mesh.bounds));
    for leaf in all {
        let pd = &leaf.pd;
        let (pscal, cscal) = match &display.color_by {
            _ if mesh.textured => (tcoords(&pd.point_data).map(packed_tcoords), None),
            Some(cb) if mesh.direct_rgb => match cb.assoc {
                Association::Points => (pd.point_data.get(&cb.name).map(packed_rgb), None),
                Association::Cells => (None, pd.cell_data.get(&cb.name).map(packed_rgb)),
            },
            Some(cb) if mesh.has_scalars => match cb.assoc {
                Association::Points => (point_scalars(&pd.point_data, &cb.name, cb.component), None),
                Association::Cells => (None, point_scalars(&pd.cell_data, &cb.name, cb.component)),
            },
            _ => (None, None),
        };
        let (ps, cs) = (pscal.as_deref(), cscal.as_deref());
        let poly_cell_base = pd.verts.len() + pd.lines.len();
        // Flattened original cell id of each surface cell (for picking/selection).
        let orig = pd.cell_data.get("vtkOriginalCellIds").cloned();
        let off = leaf.cell_offset;
        let cell_ids = move |c: usize| -> u32 { (off + orig.as_ref().map(|a| a.get(c, 0) as usize).unwrap_or(c)) as u32 };
        let crease = if display.lighting { Some(30.0) } else { None };
        match display.representation {
            Representation::SurfaceLic => {
                emit_triangles(pd, origin, ps, cs, poly_cell_base, crease, &cell_ids, &mut mesh.tris);
                emit_lic_vectors(pd, poly_cell_base, display.lic_vectors.as_deref(), &mut mesh.lic);
                mesh.lic.resize(mesh.tris.len(), [0.0; 4]);
            }
            Representation::Surface | Representation::SurfaceWithEdges => {
                emit_triangles(pd, origin, ps, cs, poly_cell_base, crease, &cell_ids, &mut mesh.tris);
                emit_lines(pd, origin, ps, cs, pd.verts.len(), &mut mesh.lines);
                if display.representation == Representation::SurfaceWithEdges {
                    emit_polygon_edges(pd, origin, None, &mut mesh.edges);
                }
                if pd.polys.is_empty() && pd.strips.is_empty() && pd.lines.is_empty() {
                    emit_points(pd, origin, ps, cs, &mut mesh.points);
                }
            }
            Representation::Wireframe => {
                emit_polygon_edges(pd, origin, ps, &mut mesh.lines);
                emit_lines(pd, origin, ps, cs, pd.verts.len(), &mut mesh.lines);
                if pd.polys.is_empty() && pd.strips.is_empty() && pd.lines.is_empty() {
                    emit_points(pd, origin, ps, cs, &mut mesh.points);
                }
            }
            Representation::Points => emit_points(pd, origin, ps, cs, &mut mesh.points),
            // Blocks without atoms (the volume next to the atoms of a cube file) stay surfaces.
            Representation::Molecule if aurafx_filters::molecule::atomic_numbers(&pd.point_data, None).is_some() => {
                emit_molecule(pd, origin, ps, display, &cell_ids, &mut mesh.tris)
            }
            Representation::Molecule => {
                emit_triangles(pd, origin, ps, cs, poly_cell_base, crease, &cell_ids, &mut mesh.tris);
                emit_lines(pd, origin, ps, cs, pd.verts.len(), &mut mesh.lines);
            }
            Representation::Glyphs => {
                if let Some(plan) = glyph_plan.as_mut() {
                    let g =
                        mesh.glyphs.get_or_insert_with(|| GlyphData { shape: glyph_shape(display.glyph_shape), instances: Vec::new(), stride: plan.stride });
                    emit_glyph_instances(pd, origin, ps, display, plan, &mut g.instances);
                }
            }
            Representation::Outline | Representation::Volume => unreachable!(),
        }
    }
    mesh
}

/// A unit glyph shape along +x (an arrow or cone from 0 to 1, a sphere or cube of size 1 about 0).
pub fn glyph_shape(shape: aurafx_pipeline::GlyphShape) -> Vec<Vertex> {
    use aurafx_pipeline::GlyphShape;
    let v = |p: [f32; 3], n: [f32; 3]| Vertex { pos: p, scalar: 0.0, normal: n, _pad: 0.0 };
    let seg = 16;
    let ring = |k: usize| {
        let a = 2.0 * std::f32::consts::PI * k as f32 / seg as f32;
        (a.cos(), a.sin())
    };
    let mut out = Vec::new();
    // A cone or cylinder side along x from x0 (radius r0) to x1 (radius r1), and its base caps.
    let mut frustum = |x0: f32, r0: f32, x1: f32, r1: f32, cap0: bool| {
        let slope = (r0 - r1) / (x1 - x0);
        for k in 0..seg {
            let ((c0, s0), (c1, s1)) = (ring(k), ring(k + 1));
            let n = |c: f32, s: f32| {
                let l = (slope * slope + 1.0).sqrt();
                [slope / l, c / l, s / l]
            };
            let (a, b, c, d) = ([x0, r0 * c0, r0 * s0], [x0, r0 * c1, r0 * s1], [x1, r1 * c1, r1 * s1], [x1, r1 * c0, r1 * s0]);
            out.extend([v(a, n(c0, s0)), v(b, n(c1, s1)), v(c, n(c1, s1)), v(a, n(c0, s0)), v(c, n(c1, s1)), v(d, n(c0, s0))]);
            if cap0 {
                out.extend([v([x0, 0.0, 0.0], [-1.0, 0.0, 0.0]), v(b, [-1.0, 0.0, 0.0]), v(a, [-1.0, 0.0, 0.0])]);
            }
        }
    };
    match shape {
        GlyphShape::Arrow => {
            frustum(0.0, 0.03, 0.65, 0.03, true);
            frustum(0.65, 0.1, 1.0, 0.0, true);
        }
        GlyphShape::Cone => frustum(0.0, 0.25, 1.0, 0.0, true),
        GlyphShape::Sphere => {
            for p in unit_sphere(8) {
                out.push(v([p[0] * 0.5, p[1] * 0.5, p[2] * 0.5], p));
            }
        }
        GlyphShape::Cube => {
            for a in 0..3 {
                for sgn in [-1.0f32, 1.0] {
                    let (b, c) = ((a + 1) % 3, (a + 2) % 3);
                    let mut n = [0.0; 3];
                    n[a] = sgn;
                    let corner = |u: f32, w: f32| {
                        let mut p = [0.0; 3];
                        p[a] = 0.5 * sgn;
                        p[b] = 0.5 * u;
                        p[c] = 0.5 * w;
                        v(p, n)
                    };
                    out.extend([corner(-1.0, -1.0), corner(1.0, -1.0), corner(1.0, 1.0), corner(-1.0, -1.0), corner(1.0, 1.0), corner(-1.0, 1.0)]);
                }
            }
        }
    }
    out
}

/// Glyph instances for the points of `pd` (one block of an object): turned along
/// `glyph_orient`, sized by `glyph_scale_by` (relative to its largest magnitude over all blocks)
/// times the glyph size. Points whose position, vector or size is not a finite number get none.
fn emit_glyph_instances(pd: &PolyData, origin: [f64; 3], pscal: Option<&[f32]>, display: &DisplayProps, plan: &mut GlyphPlan, out: &mut Vec<[f32; 8]>) {
    let n = pd.points.len();
    if n == 0 {
        return;
    }
    let orient = Some(display.glyph_orient.as_str()).filter(|s| !s.is_empty()).and_then(|s| pd.point_data.get(s)).filter(|a| a.ncomp == 3);
    let scale_by = Some(display.glyph_scale_by.as_str()).filter(|s| !s.is_empty()).and_then(|s| pd.point_data.get(s));
    out.reserve(n / plan.stride + 1);
    for i in 0..n {
        let g = plan.seen + i;
        if g % plan.stride != 0 {
            continue;
        }
        let p = pd.points.get(i);
        let mut d = orient.map(|a| [a.get(i, 0), a.get(i, 1), a.get(i, 2)]).unwrap_or([1.0, 0.0, 0.0]);
        let l = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt();
        let s = plan.size * scale_by.map(|a| glyph_magnitude(a, i) / plan.max_mag).unwrap_or(1.0);
        if !(l.is_finite() && s.is_finite() && p.iter().all(|v| v.is_finite())) {
            continue;
        }
        if l > 0.0 {
            d = [d[0] / l, d[1] / l, d[2] / l];
        } else {
            d = [1.0, 0.0, 0.0];
        }
        let value = pscal.and_then(|v| v.get(i).copied()).unwrap_or(0.0);
        out.push([(p[0] - origin[0]) as f32, (p[1] - origin[1]) as f32, (p[2] - origin[2]) as f32, value, d[0] as f32, d[1] as f32, d[2] as f32, s as f32]);
    }
    plan.seen += n;
}

/// Unit sphere triangles (normals = positions) with `rings` latitude bands.
fn unit_sphere(rings: usize) -> Vec<[f32; 3]> {
    let segs = 2 * rings;
    let p = |i: usize, j: usize| {
        let (t, f) = (std::f32::consts::PI * i as f32 / rings as f32, 2.0 * std::f32::consts::PI * j as f32 / segs as f32);
        [t.sin() * f.cos(), t.sin() * f.sin(), t.cos()]
    };
    let mut out = Vec::with_capacity(rings * segs * 6);
    for i in 0..rings {
        for j in 0..segs {
            let (a, b, c, d) = (p(i, j), p(i + 1, j), p(i + 1, j + 1), p(i, j + 1));
            if i > 0 {
                out.extend_from_slice(&[a, b, d]);
            }
            if i + 1 < rings {
                out.extend_from_slice(&[d, b, c]);
            }
        }
    }
    out
}

/// Molecule representation: a sphere per atom (point with an atomic number) and a cylinder
/// per bond (two-point line), each half of a bond in its atom's color, as ParaView draws
/// molecules. Detail drops for large molecules.
fn emit_molecule(
    pd: &PolyData,
    origin: [f64; 3],
    pscal: Option<&[f32]>,
    display: &DisplayProps,
    cell_ids: &(dyn Fn(usize) -> u32 + Sync),
    out: &mut Vec<Vertex>,
) {
    use aurafx_pipeline::MoleculeStyle;
    let n = pd.points.len();
    let z = aurafx_filters::molecule::atomic_numbers(&pd.point_data, None);
    let pos = |i: usize| {
        let p = pd.points.get(i);
        [(p[0] - origin[0]) as f32, (p[1] - origin[1]) as f32, (p[2] - origin[2]) as f32]
    };
    let scalar = |i: usize| pscal.and_then(|s| s.get(i).copied()).unwrap_or(0.0);
    let style = display.molecule_style;
    let bond_r = match style {
        MoleculeStyle::Liquorice => 2.0 * display.bond_radius,
        _ => display.bond_radius,
    };
    let radius = |i: usize| -> f32 {
        let vdw = z.map(|a| aurafx_data::elements::element(a.get(i, 0).round() as i64).vdw_radius).unwrap_or(1.5) as f32;
        match style {
            MoleculeStyle::BallAndStick => vdw * display.atom_scale,
            MoleculeStyle::VanDerWaals => vdw,
            MoleculeStyle::Liquorice => bond_r,
        }
    };
    let rings = if n <= 2_000 {
        10
    } else if n <= 50_000 {
        5
    } else {
        3
    };
    let sphere = unit_sphere(rings);
    out.reserve(n * sphere.len());
    let nv = pd.verts.len();
    for i in 0..n {
        let (c, r, s) = (pos(i), radius(i), scalar(i));
        let id = f32::from_bits(cell_ids(if i < nv { i } else { 0 }));
        for v in &sphere {
            out.push(Vertex { pos: [c[0] + r * v[0], c[1] + r * v[1], c[2] + r * v[2]], scalar: s, normal: *v, _pad: id });
        }
    }
    if style == MoleculeStyle::VanDerWaals {
        return;
    }
    let sides = if pd.lines.len() <= 20_000 { 12 } else { 6 };
    let order = pd.cell_data.get("bond_order").or_else(|| pd.cell_data.get("Bond Orders"));
    let sub = |a: [f32; 3], b: [f32; 3]| [a[0] - b[0], a[1] - b[1], a[2] - b[2]];
    let cross = |a: [f32; 3], b: [f32; 3]| [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]];
    let norm = |a: [f32; 3]| {
        let l = (a[0] * a[0] + a[1] * a[1] + a[2] * a[2]).sqrt().max(1e-20);
        [a[0] / l, a[1] / l, a[2] / l]
    };
    let cylinder = |a: [f32; 3], b: [f32; 3], r: f32, s: f32, id: f32, out: &mut Vec<Vertex>| {
        let d = norm(sub(b, a));
        let helper = if d[0].abs() < 0.9 { [1.0, 0.0, 0.0] } else { [0.0, 1.0, 0.0] };
        let u = norm(cross(d, helper));
        let w = cross(d, u);
        for k in 0..sides {
            let (t0, t1) = (2.0 * std::f32::consts::PI * k as f32 / sides as f32, 2.0 * std::f32::consts::PI * (k + 1) as f32 / sides as f32);
            let n0 = [u[0] * t0.cos() + w[0] * t0.sin(), u[1] * t0.cos() + w[1] * t0.sin(), u[2] * t0.cos() + w[2] * t0.sin()];
            let n1 = [u[0] * t1.cos() + w[0] * t1.sin(), u[1] * t1.cos() + w[1] * t1.sin(), u[2] * t1.cos() + w[2] * t1.sin()];
            let p = |c: [f32; 3], n: [f32; 3]| [c[0] + r * n[0], c[1] + r * n[1], c[2] + r * n[2]];
            let v = |c: [f32; 3], n: [f32; 3]| Vertex { pos: p(c, n), scalar: s, normal: n, _pad: id };
            out.extend_from_slice(&[v(a, n0), v(b, n0), v(b, n1), v(a, n0), v(b, n1), v(a, n1)]);
        }
    };
    for c in 0..pd.lines.len() {
        let l = pd.lines.cell(c);
        if l.len() != 2 {
            continue;
        }
        let (i, j) = (l[0] as usize, l[1] as usize);
        let (a, b) = (pos(i), pos(j));
        let mid = [(a[0] + b[0]) / 2.0, (a[1] + b[1]) / 2.0, (a[2] + b[2]) / 2.0];
        let id = f32::from_bits(cell_ids(nv + c));
        let k = if display.bond_orders && style == MoleculeStyle::BallAndStick {
            order.map(|o| o.get(nv + c, 0).round().clamp(1.0, 3.0) as usize).unwrap_or(1)
        } else {
            1
        };
        // Several bonds side by side, spread across the bond.
        let r = if k == 1 { bond_r } else { bond_r * 0.6 };
        let d = norm(sub(b, a));
        let helper = if d[2].abs() < 0.9 { [0.0, 0.0, 1.0] } else { [1.0, 0.0, 0.0] };
        let side = norm(cross(d, helper));
        for m in 0..k {
            let off = (m as f32 - (k - 1) as f32 / 2.0) * 3.5 * r;
            let o = [side[0] * off, side[1] * off, side[2] * off];
            let sh = |p: [f32; 3]| [p[0] + o[0], p[1] + o[1], p[2] + o[2]];
            cylinder(sh(a), sh(mid), r, scalar(i), id, out);
            cylinder(sh(mid), sh(b), r, scalar(j), id, out);
        }
    }
}
