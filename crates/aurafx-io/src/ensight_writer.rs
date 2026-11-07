//! EnSight Gold writer (C Binary): a `.case` file, a geometry file and one file per variable.
//! Each leaf block becomes a part: image data as a uniform block, rectilinear grids as a
//! rectilinear block, structured grids as a curvilinear block, everything else as unstructured
//! elements grouped by type (so cells come back grouped by type, as with any EnSight file).
//! Time series write one geometry and one file per variable for every step.

use anyhow::{bail, Context, Result};
use aurafx_data::*;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

// ---- C Binary encoding ----

#[derive(Default)]
struct Bin(Vec<u8>);

impl Bin {
    fn c_binary() -> Bin {
        let mut b = Bin::default();
        b.s("C Binary");
        b
    }
    /// An 80-byte string.
    fn s(&mut self, t: &str) {
        let mut b = [0u8; 80];
        let t = t.as_bytes();
        let n = t.len().min(79);
        b[..n].copy_from_slice(&t[..n]);
        self.0.extend_from_slice(&b);
    }
    fn i(&mut self, v: usize) {
        self.0.extend_from_slice(&(v as i32).to_le_bytes());
    }
    fn ints(&mut self, v: &[i32]) {
        for x in v {
            self.0.extend_from_slice(&x.to_le_bytes());
        }
    }
    fn floats(&mut self, v: impl IntoIterator<Item = f32>) {
        for x in v {
            self.0.extend_from_slice(&x.to_le_bytes());
        }
    }
    fn save(&self, path: &Path) -> Result<()> {
        std::fs::write(path, &self.0).with_context(|| format!("writing {}", path.display()))
    }
}

// ---- parts ----

/// Element types in the order they are written.
const ORDER: [&str; 17] = [
    "point",
    "bar2",
    "bar3",
    "tria3",
    "tria6",
    "quad4",
    "quad8",
    "tetra4",
    "tetra10",
    "pyramid5",
    "pyramid13",
    "penta6",
    "penta15",
    "hexa8",
    "hexa20",
    "nsided",
    "nfaced",
];

/// One element group: connectivity (1-based) and the source cell of every element.
#[derive(Default)]
struct Group {
    conn: Vec<i32>,
    /// nsided: nodes per element; nfaced: faces per element.
    counts: Vec<i32>,
    /// nfaced: nodes per face.
    face_counts: Vec<i32>,
    cells: Vec<usize>,
}

enum Geometry {
    Uniform { dims: [usize; 3], origin: [f64; 3], spacing: [f64; 3] },
    Rectilinear { x: Vec<f64>, y: Vec<f64>, z: Vec<f64> },
    Curvilinear { dims: [usize; 3], points: Points },
    Elements { points: Points, groups: BTreeMap<usize, Group> },
}

struct Part {
    name: String,
    geometry: Geometry,
    point_data: Attributes,
    cell_data: Attributes,
}

impl Part {
    fn n_points(&self) -> usize {
        match &self.geometry {
            Geometry::Uniform { dims, .. } | Geometry::Curvilinear { dims, .. } => dims.iter().product(),
            Geometry::Rectilinear { x, y, z } => x.len() * y.len() * z.len(),
            Geometry::Elements { points, .. } => points.len(),
        }
    }
}

fn first(n: usize) -> Vec<usize> {
    (0..n).collect()
}

/// EnSight wedges run the other way round; the permutation is its own inverse.
const PENTA6: [usize; 6] = [0, 2, 1, 3, 5, 4];
const PENTA15: [usize; 15] = [0, 2, 1, 3, 5, 4, 8, 7, 6, 11, 10, 9, 12, 14, 13];

/// The EnSight element(s) for one VTK cell: (type index into ORDER, nodes) pairs, or a polygon
/// / polyhedron. Higher-order Lagrange/Bezier cells keep their corners.
enum Elem {
    Fixed(usize, Vec<u32>),
    NSided(Vec<u32>),
    NFaced(Vec<Vec<u32>>),
}

fn convert(ty: CellType, ids: &[u32], faces: Option<&Vec<Vec<u32>>>) -> Vec<Elem> {
    let pick = |name: &str, order: &[usize]| {
        let t = ORDER.iter().position(|n| *n == name).unwrap();
        if order.iter().all(|&k| k < ids.len()) {
            vec![Elem::Fixed(t, order.iter().map(|&k| ids[k]).collect())]
        } else {
            Vec::new()
        }
    };
    use CellType::*;
    match ty {
        Vertex => pick("point", &[0]),
        PolyVertex => ids.iter().flat_map(|&p| vec![Elem::Fixed(0, vec![p])]).collect(),
        Line | CubicLine | LagrangeCurve | BezierCurve => pick("bar2", &[0, 1]),
        PolyLine => ids.windows(2).map(|w| Elem::Fixed(1, w.to_vec())).collect(),
        QuadraticEdge => pick("bar3", &first(3)),
        Triangle | LagrangeTriangle | BezierTriangle => pick("tria3", &first(3)),
        TriangleStrip => (0..ids.len().saturating_sub(2))
            .map(|k| Elem::Fixed(3, if k % 2 == 0 { vec![ids[k], ids[k + 1], ids[k + 2]] } else { vec![ids[k + 1], ids[k], ids[k + 2]] }))
            .collect(),
        QuadraticTriangle | BiquadraticTriangle => pick("tria6", &first(6)),
        Polygon if ids.len() == 3 => pick("tria3", &first(3)),
        Polygon if ids.len() == 4 => pick("quad4", &first(4)),
        Polygon if ids.len() > 4 => vec![Elem::NSided(ids.to_vec())],
        Pixel => pick("quad4", &[0, 1, 3, 2]),
        Quad | QuadraticLinearQuad | LagrangeQuadrilateral | BezierQuadrilateral => pick("quad4", &first(4)),
        QuadraticQuad | BiquadraticQuad => pick("quad8", &first(8)),
        Tetra | LagrangeTetrahedron | BezierTetrahedron => pick("tetra4", &first(4)),
        QuadraticTetra => pick("tetra10", &first(10)),
        Pyramid | LagrangePyramid | BezierPyramid => pick("pyramid5", &first(5)),
        QuadraticPyramid => pick("pyramid13", &first(13)),
        Wedge | QuadraticLinearWedge | BiquadraticQuadraticWedge | LagrangeWedge | BezierWedge => pick("penta6", &PENTA6),
        QuadraticWedge => pick("penta15", &PENTA15),
        Voxel => pick("hexa8", &[0, 1, 3, 2, 4, 5, 7, 6]),
        Hexahedron | LagrangeHexahedron | BezierHexahedron => pick("hexa8", &first(8)),
        QuadraticHexahedron | TriquadraticHexahedron | BiquadraticQuadraticHexahedron => pick("hexa20", &first(20)),
        PentagonalPrism | HexagonalPrism => {
            let n = ids.len() / 2;
            let mut f: Vec<Vec<u32>> = vec![ids[..n].iter().rev().copied().collect(), ids[n..2 * n].to_vec()];
            for i in 0..n {
                let j = (i + 1) % n;
                f.push(vec![ids[i], ids[j], ids[n + j], ids[n + i]]);
            }
            vec![Elem::NFaced(f)]
        }
        Polyhedron => match faces {
            Some(f) if !f.is_empty() => vec![Elem::NFaced(f.clone())],
            _ => Vec::new(),
        },
        _ => Vec::new(),
    }
}

fn elements(ug: &UnstructuredGrid, skipped: &mut usize) -> BTreeMap<usize, Group> {
    let mut groups: BTreeMap<usize, Group> = BTreeMap::new();
    let faces = ug.cells.polyhedron_faces.as_ref();
    for (c, (ty, ids)) in ug.cells.iter().enumerate() {
        let elems = convert(ty, ids, faces.and_then(|f| f.get(c)));
        if elems.is_empty() && ty != CellType::Empty {
            *skipped += 1;
        }
        for e in elems {
            match e {
                Elem::Fixed(t, nodes) => {
                    let g = groups.entry(t).or_default();
                    g.conn.extend(nodes.iter().map(|&i| i as i32 + 1));
                    g.cells.push(c);
                }
                Elem::NSided(nodes) => {
                    let g = groups.entry(15).or_default();
                    g.counts.push(nodes.len() as i32);
                    g.conn.extend(nodes.iter().map(|&i| i as i32 + 1));
                    g.cells.push(c);
                }
                Elem::NFaced(fs) => {
                    let g = groups.entry(16).or_default();
                    g.counts.push(fs.len() as i32);
                    for f in &fs {
                        g.face_counts.push(f.len() as i32);
                        g.conn.extend(f.iter().map(|&i| i as i32 + 1));
                    }
                    g.cells.push(c);
                }
            }
        }
    }
    groups
}

fn is_identity(d: &[f64; 9]) -> bool {
    d.iter().zip([1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0]).all(|(a, b)| (a - b).abs() < 1e-12)
}

fn parts(ds: &DataSet, skipped: &mut usize) -> Result<Vec<Part>> {
    let leaves: Vec<(String, &DataSet)> = match ds {
        DataSet::MultiBlock(mb) => mb.leaves(),
        DataSet::Table(_) => bail!("tables cannot be written as EnSight"),
        d => vec![("part".to_string(), d)],
    };
    let mut out = Vec::new();
    for (name, d) in leaves {
        let (geometry, pd, cd) = match d {
            DataSet::ImageData(im) if is_identity(&im.direction) => {
                let e = &im.extent;
                let origin = [0, 1, 2].map(|a| im.origin[a] + e[2 * a] as f64 * im.spacing[a]);
                (Geometry::Uniform { dims: im.dims(), origin, spacing: im.spacing }, &im.point_data, &im.cell_data)
            }
            DataSet::ImageData(im) => (Geometry::Curvilinear { dims: im.dims(), points: im.points() }, &im.point_data, &im.cell_data),
            DataSet::RectilinearGrid(g) => (Geometry::Rectilinear { x: g.x.clone(), y: g.y.clone(), z: g.z.clone() }, &g.point_data, &g.cell_data),
            DataSet::StructuredGrid(g) => (Geometry::Curvilinear { dims: g.dims(), points: g.points.clone() }, &g.point_data, &g.cell_data),
            DataSet::Table(_) | DataSet::MultiBlock(_) => continue,
            d => {
                let ug = d.to_unstructured().unwrap();
                let groups = elements(&ug, skipped);
                out.push(Part { name, geometry: Geometry::Elements { points: ug.points, groups }, point_data: ug.point_data, cell_data: ug.cell_data });
                continue;
            }
        };
        if let Geometry::Uniform { dims, .. } | Geometry::Curvilinear { dims, .. } = &geometry {
            if dims.iter().any(|&n| n == 0) {
                continue;
            }
        }
        out.push(Part { name, geometry, point_data: pd.clone(), cell_data: cd.clone() });
    }
    if out.is_empty() {
        bail!("nothing to write: no blocks with points");
    }
    Ok(out)
}

fn geometry_bytes(parts: &[Part]) -> Bin {
    let mut b = Bin::c_binary();
    b.s("Written by auraFX");
    b.s("EnSight Gold geometry");
    b.s("node id off");
    b.s("element id off");
    for (k, p) in parts.iter().enumerate() {
        b.s("part");
        b.i(k + 1);
        b.s(&p.name);
        match &p.geometry {
            Geometry::Uniform { dims, origin, spacing } => {
                b.s("block uniform");
                b.ints(&dims.map(|n| n as i32));
                b.floats(origin.map(|v| v as f32));
                b.floats(spacing.map(|v| v as f32));
            }
            Geometry::Rectilinear { x, y, z } => {
                b.s("block rectilinear");
                b.ints(&[x.len() as i32, y.len() as i32, z.len() as i32]);
                for v in [x, y, z] {
                    b.floats(v.iter().map(|&a| a as f32));
                }
            }
            Geometry::Curvilinear { dims, points } => {
                b.s("block");
                b.ints(&dims.map(|n| n as i32));
                for a in 0..3 {
                    b.floats((0..points.len()).map(|i| points.get(i)[a] as f32));
                }
            }
            Geometry::Elements { points, groups } => {
                b.s("coordinates");
                b.i(points.len());
                for a in 0..3 {
                    b.floats((0..points.len()).map(|i| points.get(i)[a] as f32));
                }
                for (&t, g) in groups {
                    b.s(ORDER[t]);
                    b.i(g.cells.len());
                    match ORDER[t] {
                        "nsided" => b.ints(&g.counts),
                        "nfaced" => {
                            b.ints(&g.counts);
                            b.ints(&g.face_counts);
                        }
                        _ => {}
                    }
                    b.ints(&g.conn);
                }
            }
        }
    }
    b
}

// ---- variables ----

#[derive(Clone)]
struct VarDef {
    /// Name of the array in the data.
    array: String,
    /// Name in the case file (unique, EnSight-safe).
    label: String,
    per_node: bool,
    ncomp: usize,
}

fn kind(ncomp: usize) -> &'static str {
    match ncomp {
        1 => "scalar",
        6 => "tensor symm",
        9 => "tensor asym",
        _ => "vector",
    }
}

/// EnSight-safe variable name: letters, digits and `_`, not starting with a digit.
fn safe_name(s: &str) -> String {
    let mut n: String = s.chars().map(|c| if c.is_ascii_alphanumeric() || c == '_' { c } else { '_' }).collect();
    if n.is_empty() || n.starts_with(|c: char| c.is_ascii_digit()) {
        n.insert_str(0, "v_");
    }
    n.truncate(49);
    n
}

fn writable(a: &Array) -> bool {
    a.scalar_type() != ScalarType::String && matches!(a.ncomp, 1 | 2 | 3 | 6 | 9) && !crate::exodus_writer::is_internal(&a.name)
}

fn variables(parts: &[Part]) -> Vec<VarDef> {
    let mut out: Vec<VarDef> = Vec::new();
    for per_node in [true, false] {
        for p in parts {
            let attrs = if per_node { &p.point_data } else { &p.cell_data };
            for a in attrs.iter().filter(|a| writable(a)) {
                if out.iter().any(|v| v.per_node == per_node && v.array == a.name) {
                    continue;
                }
                let mut label = safe_name(&a.name);
                while out.iter().any(|v| v.label.eq_ignore_ascii_case(&label)) {
                    label.push_str(if per_node { "_n" } else { "_c" });
                }
                out.push(VarDef { array: a.name.clone(), label, per_node, ncomp: if a.ncomp == 2 { 3 } else { a.ncomp } });
            }
        }
    }
    out
}

/// Component `c` of every tuple listed in `idx` (all tuples when `None`), in EnSight's order.
fn component(a: &Array, ncomp: usize, c: usize, idx: Option<&[usize]>) -> Vec<f32> {
    // Symmetric tensors: VTK XX YY ZZ XY YZ XZ; EnSight 11 22 33 12 13 23.
    let src = if ncomp == 6 { [0, 1, 2, 3, 5, 4][c] } else { c };
    let get = |i: usize| if src < a.ncomp { a.get(i, src) as f32 } else { 0.0 };
    match idx {
        Some(idx) => idx.iter().map(|&i| get(i)).collect(),
        None => (0..a.len()).map(get).collect(),
    }
}

fn write_variable(parts: &[Part], v: &VarDef, path: &Path) -> Result<()> {
    let mut b = Bin::default();
    b.s(&v.label);
    for (k, p) in parts.iter().enumerate() {
        let attrs = if v.per_node { &p.point_data } else { &p.cell_data };
        let Some(a) = attrs.get(&v.array).filter(|a| writable(a) && (if a.ncomp == 2 { 3 } else { a.ncomp }) == v.ncomp) else { continue };
        if v.per_node {
            if a.len() != p.n_points() {
                continue;
            }
            b.s("part");
            b.i(k + 1);
            b.s("coordinates");
            for c in 0..v.ncomp {
                b.floats(component(a, v.ncomp, c, None));
            }
            continue;
        }
        match &p.geometry {
            Geometry::Elements { groups, .. } => {
                if groups.values().flat_map(|g| &g.cells).any(|&c| c >= a.len()) {
                    continue;
                }
                b.s("part");
                b.i(k + 1);
                for (&t, g) in groups {
                    b.s(ORDER[t]);
                    for c in 0..v.ncomp {
                        b.floats(component(a, v.ncomp, c, Some(&g.cells)));
                    }
                }
            }
            _ => {
                b.s("part");
                b.i(k + 1);
                b.s("block");
                for c in 0..v.ncomp {
                    b.floats(component(a, v.ncomp, c, None));
                }
            }
        }
    }
    b.save(path)
}

// ---- the case file ----

/// `base.ext` for one step, or with the step number in place of the `*`s.
fn file_name(base: &str, ext: &str, step: Option<(usize, usize)>) -> String {
    match step {
        None => format!("{base}.{ext}"),
        Some((i, width)) => format!("{base}.{ext}{i:0width$}"),
    }
}

fn wildcard(base: &str, ext: &str, width: Option<usize>) -> String {
    match width {
        None => format!("{base}.{ext}"),
        Some(w) => format!("{base}.{ext}{}", "*".repeat(w)),
    }
}

fn case_paths(path: &Path) -> Result<(PathBuf, String)> {
    let dir = path.parent().map(|p| if p.as_os_str().is_empty() { Path::new(".") } else { p }).unwrap_or(Path::new(".")).to_path_buf();
    let base = path.file_stem().and_then(|s| s.to_str()).filter(|s| !s.is_empty()).context("the file needs a name")?;
    Ok((dir, base.to_string()))
}

/// Write `ds` as `path` (`name.case`) plus `name.geo` and `name.<variable>` next to it.
pub fn write_ensight(ds: &DataSet, path: &Path) -> Result<()> {
    write_ensight_steps(&[(0.0, ds)], path)
}

/// Write every step (time, data) as one EnSight case with a time set.
pub fn write_ensight_steps(steps: &[(f64, &DataSet)], path: &Path) -> Result<()> {
    if steps.is_empty() {
        bail!("nothing to write");
    }
    let (dir, base) = case_paths(path)?;
    let timed = steps.len() > 1;
    let width = timed.then(|| steps.len().saturating_sub(1).to_string().len().max(4));
    let mut skipped = 0usize;
    let mut vars: Vec<VarDef> = Vec::new();
    // The geometry is written once while every step has the same; the first step that differs
    // turns it into one file per step.
    let mut first_geo: Option<Bin> = None;
    let mut geo_changes = !timed;
    for (i, (_, ds)) in steps.iter().enumerate() {
        let parts = parts(ds, &mut skipped)?;
        if i == 0 {
            vars = variables(&parts);
        }
        let step = width.map(|w| (i, w));
        let geo = geometry_bytes(&parts);
        if geo_changes {
            geo.save(&dir.join(file_name(&base, "geo", step)))?;
        } else if let Some(first) = &first_geo {
            if first.0 != geo.0 {
                geo_changes = true;
                for k in 0..i {
                    first.save(&dir.join(file_name(&base, "geo", width.map(|w| (k, w)))))?;
                }
                geo.save(&dir.join(file_name(&base, "geo", step)))?;
            }
        } else {
            first_geo = Some(geo);
        }
        for v in &vars {
            write_variable(&parts, v, &dir.join(file_name(&base, &v.label, step)))?;
        }
    }
    if !geo_changes {
        if let Some(first) = &first_geo {
            first.save(&dir.join(file_name(&base, "geo", None)))?;
        }
    }
    if skipped > 0 {
        tracing::warn!("EnSight writer: {skipped} cells of unsupported types were left out");
    }
    let ts = if timed { "1 " } else { "" };
    let mut c = String::from("FORMAT\ntype: ensight gold\n\nGEOMETRY\n");
    if geo_changes {
        c += &format!("model: {ts}{}\n", wildcard(&base, "geo", width));
    } else {
        c += &format!("model: {}\n", wildcard(&base, "geo", None));
    }
    if !vars.is_empty() {
        c += "\nVARIABLE\n";
        for v in &vars {
            c += &format!("{} per {}: {ts}{} {}\n", kind(v.ncomp), if v.per_node { "node" } else { "element" }, v.label, wildcard(&base, &v.label, width));
        }
    }
    if timed {
        c += &format!("\nTIME\ntime set: 1\nnumber of steps: {}\nfilename start number: 0\nfilename increment: 1\ntime values:\n", steps.len());
        for chunk in steps.chunks(6) {
            c += &chunk.iter().map(|(t, _)| format!("{t:.9e}")).collect::<Vec<_>>().join(" ");
            c += "\n";
        }
    }
    std::fs::write(path, c).with_context(|| format!("writing {}", path.display()))
}
