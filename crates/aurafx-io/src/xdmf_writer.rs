//! Xdmf 3 writer: the description in `name.xmf`, the arrays in `name.h5` (written with our own
//! HDF5 encoder). Image data becomes a 3DCoRectMesh, rectilinear grids a 3DRectMesh, structured
//! grids a 3DSMesh, everything else an unstructured grid (one cell type when the cells share it,
//! else a Mixed topology, polyhedra included). Multiblocks become spatial collections with
//! their tree; time series a temporal collection.

use crate::hdf5w::{self, Node};
use anyhow::{bail, Context, Result};
use aurafx_data::*;
use std::fmt::Write as _;
use std::path::Path;

fn esc(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

/// Xdmf number type and precision of an array.
fn number_type(d: &ArrayData) -> Option<(&'static str, usize)> {
    Some(match d {
        ArrayData::I8(_) => ("Char", 1),
        ArrayData::U8(_) => ("UChar", 1),
        ArrayData::I16(_) => ("Int", 2),
        ArrayData::U16(_) => ("UInt", 2),
        ArrayData::I32(_) => ("Int", 4),
        ArrayData::U32(_) => ("UInt", 4),
        ArrayData::I64(_) => ("Int", 8),
        ArrayData::U64(_) => ("UInt", 8),
        ArrayData::F16(_) | ArrayData::F32(_) => ("Float", 4),
        ArrayData::F64(_) => ("Float", 8),
        ArrayData::String(_) => return None,
    })
}

/// Collects the HDF5 datasets while the XML is written.
struct Writer {
    xml: String,
    h5: Node,
    h5_name: String,
    /// Points and connectivity already stored, by block (the path without its step): a later
    /// step with the same values points at them instead of storing them again.
    shared: std::collections::HashMap<String, (ArrayData, Vec<usize>, String)>,
}

/// `/s3/b0/points` -> `/b0/points`: the same block in another time step.
fn block_key(path: &str) -> Option<String> {
    let rest = path.strip_prefix("/s")?;
    let (step, tail) = rest.split_once('/')?;
    step.chars().all(|c| c.is_ascii_digit()).then(|| format!("/{tail}"))
}

impl Writer {
    /// Store `data` at `path` (`/a/b/c`) and write the matching DataItem.
    fn item(&mut self, indent: usize, path: &str, data: ArrayData, dims: &[usize]) {
        let data = match data {
            ArrayData::F16(v) => ArrayData::F32(v.iter().map(|x| x.to_f32()).collect()),
            d => d,
        };
        let (nt, prec) = number_type(&data).unwrap_or(("Float", 8));
        let d = dims.iter().map(|d| d.to_string()).collect::<Vec<_>>().join(" ");
        let geometry = ["points", "connectivity", "x", "y", "z"].iter().any(|n| path.ends_with(&format!("/{n}")));
        let key = if geometry { block_key(path) } else { None };
        if let Some((_, _, stored)) = key.as_ref().and_then(|k| self.shared.get(k)).filter(|(sd, sdims, _)| *sdims == dims && *sd == data) {
            let _ = writeln!(
                self.xml,
                "{:indent$}<DataItem Dimensions=\"{d}\" NumberType=\"{nt}\" Precision=\"{prec}\" Format=\"HDF\">{}:{stored}</DataItem>",
                "",
                esc(&self.h5_name)
            );
            return;
        }
        if let Some(k) = key {
            self.shared.insert(k, (data.clone(), dims.to_vec(), path.to_string()));
        }
        let mut parts: Vec<&str> = path.trim_start_matches('/').split('/').collect();
        let leaf = parts.pop().unwrap();
        let mut g = &mut self.h5;
        for p in parts {
            g = g.group_mut(p);
        }
        g.add(leaf, Node::dataset(data, dims.iter().map(|&d| d as u64).collect()));
        let _ = writeln!(
            self.xml,
            "{:indent$}<DataItem Dimensions=\"{d}\" NumberType=\"{nt}\" Precision=\"{prec}\" Format=\"HDF\">{}:{path}</DataItem>",
            "",
            esc(&self.h5_name)
        );
    }

    fn inline(&mut self, indent: usize, values: &[f64]) {
        let v = values.iter().map(|x| format!("{x:?}")).collect::<Vec<_>>().join(" ");
        let _ = writeln!(self.xml, "{:indent$}<DataItem Dimensions=\"{}\" NumberType=\"Float\" Precision=\"8\" Format=\"XML\">{v}</DataItem>", "", values.len());
    }

    fn line(&mut self, indent: usize, s: &str) {
        let _ = writeln!(self.xml, "{:indent$}{s}", "");
    }

    fn grid(&mut self, ds: &DataSet, name: &str, h5: &str, time: Option<f64>, indent: usize, skipped: &mut usize) -> Result<()> {
        if let DataSet::MultiBlock(mb) = ds {
            self.line(indent, &format!("<Grid Name=\"{}\" GridType=\"Collection\" CollectionType=\"Spatial\">", esc(name)));
            if let Some(t) = time {
                self.line(indent + 2, &format!("<Time Value=\"{t:?}\"/>"));
            }
            for (k, (bname, b)) in mb.blocks.iter().enumerate() {
                if let Some(b) = b {
                    if !matches!(b, DataSet::Table(_)) {
                        self.grid(b, bname, &format!("{h5}/b{k}"), None, indent + 2, skipped)?;
                    }
                }
            }
            self.line(indent, "</Grid>");
            return Ok(());
        }
        self.line(indent, &format!("<Grid Name=\"{}\" GridType=\"Uniform\">", esc(name)));
        if let Some(t) = time {
            self.line(indent + 2, &format!("<Time Value=\"{t:?}\"/>"));
        }
        let i2 = indent + 2;
        let i4 = indent + 4;
        // Cells of the written topology, as indices of the source cells (None: unchanged).
        let mut cell_map: Option<Vec<usize>> = None;
        let (pd, cd) = match ds {
            DataSet::ImageData(im) if is_identity(&im.direction) => {
                let [nx, ny, nz] = im.dims();
                let e = &im.extent;
                let o = [0, 1, 2].map(|a| im.origin[a] + e[2 * a] as f64 * im.spacing[a]);
                self.line(i2, &format!("<Topology TopologyType=\"3DCoRectMesh\" Dimensions=\"{nz} {ny} {nx}\"/>"));
                self.line(i2, "<Geometry GeometryType=\"ORIGIN_DXDYDZ\">");
                self.inline(i4, &[o[2], o[1], o[0]]);
                self.inline(i4, &[im.spacing[2], im.spacing[1], im.spacing[0]]);
                self.line(i2, "</Geometry>");
                (&im.point_data, &im.cell_data)
            }
            DataSet::ImageData(im) => {
                let [nx, ny, nz] = im.dims();
                self.line(i2, &format!("<Topology TopologyType=\"3DSMesh\" Dimensions=\"{nz} {ny} {nx}\"/>"));
                self.line(i2, "<Geometry GeometryType=\"XYZ\">");
                self.item(i4, &format!("{h5}/points"), im.points().data, &[nx * ny * nz, 3]);
                self.line(i2, "</Geometry>");
                (&im.point_data, &im.cell_data)
            }
            DataSet::RectilinearGrid(g) => {
                let [nx, ny, nz] = g.dims();
                self.line(i2, &format!("<Topology TopologyType=\"3DRectMesh\" Dimensions=\"{nz} {ny} {nx}\"/>"));
                self.line(i2, "<Geometry GeometryType=\"VXVYVZ\">");
                for (a, v) in [("x", &g.x), ("y", &g.y), ("z", &g.z)] {
                    self.item(i4, &format!("{h5}/{a}"), ArrayData::F64(v.clone()), &[v.len()]);
                }
                self.line(i2, "</Geometry>");
                (&g.point_data, &g.cell_data)
            }
            DataSet::StructuredGrid(g) => {
                let [nx, ny, nz] = g.dims();
                self.line(i2, &format!("<Topology TopologyType=\"3DSMesh\" Dimensions=\"{nz} {ny} {nx}\"/>"));
                self.line(i2, "<Geometry GeometryType=\"XYZ\">");
                self.item(i4, &format!("{h5}/points"), g.points.data.clone(), &[g.points.len(), 3]);
                self.line(i2, "</Geometry>");
                (&g.point_data, &g.cell_data)
            }
            DataSet::UnstructuredGrid(_) | DataSet::PolyData(_) => {
                let ug = ds.to_unstructured().unwrap();
                let topo = topology(&ug, skipped);
                match topo.single {
                    Some((tname, npe)) => {
                        let n = topo.cells.len();
                        self.line(i2, &format!("<Topology TopologyType=\"{tname}\" NumberOfElements=\"{n}\" NodesPerElement=\"{npe}\">"));
                        self.item(i4, &format!("{h5}/connectivity"), ArrayData::I64(topo.stream), &[n, npe]);
                    }
                    None => {
                        let n = topo.cells.len();
                        let len = topo.stream.len();
                        self.line(i2, &format!("<Topology TopologyType=\"Mixed\" NumberOfElements=\"{n}\">"));
                        self.item(i4, &format!("{h5}/connectivity"), ArrayData::I64(topo.stream), &[len]);
                    }
                }
                self.line(i2, "</Topology>");
                self.line(i2, "<Geometry GeometryType=\"XYZ\">");
                self.item(i4, &format!("{h5}/points"), ug.points.data.clone(), &[ug.points.len(), 3]);
                self.line(i2, "</Geometry>");
                if topo.cells.len() != ug.cells.len() || topo.cells.iter().enumerate().any(|(i, &c)| i != c) {
                    cell_map = Some(topo.cells);
                }
                self.attributes(i2, h5, &ug.point_data, &ug.cell_data, cell_map.as_deref());
                self.line(indent, "</Grid>");
                return Ok(());
            }
            DataSet::Table(_) | DataSet::MultiBlock(_) => bail!("tables cannot be written as Xdmf"),
        };
        self.attributes(i2, h5, pd, cd, None);
        self.line(indent, "</Grid>");
        Ok(())
    }

    fn attributes(&mut self, indent: usize, h5: &str, pd: &Attributes, cd: &Attributes, cell_map: Option<&[usize]>) {
        for (center, attrs, tag) in [("Node", pd, "p"), ("Cell", cd, "c")] {
            for (k, a) in attrs.iter().enumerate() {
                if a.scalar_type() == ScalarType::String || a.is_empty() {
                    continue;
                }
                let a = match (center, cell_map) {
                    ("Cell", Some(m)) => a.gather(m),
                    _ => a.clone(),
                };
                let ty = match a.ncomp {
                    1 => "Scalar",
                    3 => "Vector",
                    6 => "Tensor6",
                    9 => "Tensor",
                    _ => "Matrix",
                };
                self.line(indent, &format!("<Attribute Name=\"{}\" AttributeType=\"{ty}\" Center=\"{center}\">", esc(&a.name)));
                let dims: Vec<usize> = if a.ncomp == 1 { vec![a.len()] } else { vec![a.len(), a.ncomp] };
                self.item(indent + 2, &format!("{h5}/{tag}{k}"), a.data, &dims);
                self.line(indent, "</Attribute>");
            }
        }
    }
}

fn is_identity(d: &[f64; 9]) -> bool {
    d.iter().zip([1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0]).all(|(a, b)| (a - b).abs() < 1e-12)
}

struct Topology {
    /// Connectivity (single type) or the Mixed stream.
    stream: Vec<i64>,
    /// Source cell of every written cell.
    cells: Vec<usize>,
    /// Topology name and nodes per element when every cell has the same fixed type.
    single: Option<(&'static str, usize)>,
}

/// Xdmf code, topology name and node order for a VTK cell type with a fixed node count.
fn fixed(ty: CellType) -> Option<(i64, &'static str, &'static [usize])> {
    use CellType::*;
    const P: [usize; 27] = [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25, 26];
    Some(match ty {
        Triangle => (4, "Triangle", &P[..3]),
        Quad => (5, "Quadrilateral", &P[..4]),
        Pixel => (5, "Quadrilateral", &[0, 1, 3, 2]),
        Tetra => (6, "Tetrahedron", &P[..4]),
        Pyramid => (7, "Pyramid", &P[..5]),
        Wedge => (8, "Wedge", &P[..6]),
        Hexahedron => (9, "Hexahedron", &P[..8]),
        Voxel => (9, "Hexahedron", &[0, 1, 3, 2, 4, 5, 7, 6]),
        QuadraticEdge => (34, "Edge_3", &P[..3]),
        BiquadraticQuad => (35, "Quadrilateral_9", &P[..9]),
        QuadraticTriangle => (36, "Triangle_6", &P[..6]),
        QuadraticQuad => (37, "Quadrilateral_8", &P[..8]),
        QuadraticTetra => (38, "Tetrahedron_10", &P[..10]),
        QuadraticPyramid => (39, "Pyramid_13", &P[..13]),
        QuadraticWedge => (40, "Wedge_15", &P[..15]),
        BiquadraticQuadraticWedge => (41, "Wedge_18", &P[..18]),
        QuadraticHexahedron => (48, "Hexahedron_20", &P[..20]),
        TriquadraticHexahedron => (50, "Hexahedron_27", &P[..27]),
        // Other higher-order cells keep their corners.
        CubicLine | LagrangeCurve | BezierCurve => return None,
        BiquadraticTriangle => (36, "Triangle_6", &P[..6]),
        QuadraticLinearQuad | LagrangeQuadrilateral | BezierQuadrilateral => (5, "Quadrilateral", &P[..4]),
        LagrangeTriangle | BezierTriangle => (4, "Triangle", &P[..3]),
        LagrangeTetrahedron | BezierTetrahedron => (6, "Tetrahedron", &P[..4]),
        LagrangePyramid | BezierPyramid => (7, "Pyramid", &P[..5]),
        QuadraticLinearWedge | LagrangeWedge | BezierWedge => (8, "Wedge", &P[..6]),
        BiquadraticQuadraticHexahedron => (48, "Hexahedron_20", &P[..20]),
        LagrangeHexahedron | BezierHexahedron => (9, "Hexahedron", &P[..8]),
        _ => return None,
    })
}

fn topology(ug: &UnstructuredGrid, skipped: &mut usize) -> Topology {
    let mut stream = Vec::new();
    let mut cells = Vec::new();
    // (topology name, node count) of the cells so far; None after a second kind is seen.
    let mut kinds: Option<Option<(&'static str, usize)>> = None;
    let note = |kinds: &mut Option<Option<(&'static str, usize)>>, k: Option<(&'static str, usize)>| {
        *kinds = match *kinds {
            None => Some(k),
            Some(prev) if prev == k => Some(prev),
            _ => Some(None),
        };
    };
    let faces = ug.cells.polyhedron_faces.as_ref();
    let push_poly = |stream: &mut Vec<i64>, code: i64, ids: &[u32]| {
        stream.push(code);
        stream.push(ids.len() as i64);
        stream.extend(ids.iter().map(|&i| i as i64));
    };
    let push_polyhedron = |stream: &mut Vec<i64>, fs: &[Vec<u32>]| {
        stream.push(16);
        stream.push(fs.len() as i64);
        for f in fs {
            stream.push(f.len() as i64);
            stream.extend(f.iter().map(|&i| i as i64));
        }
    };
    for (c, (ty, ids)) in ug.cells.iter().enumerate() {
        use CellType::*;
        match ty {
            Vertex | PolyVertex => {
                push_poly(&mut stream, 1, ids);
                note(&mut kinds, (ids.len() == 1).then_some(("Polyvertex", 1)));
            }
            Line | PolyLine => {
                push_poly(&mut stream, 2, ids);
                note(&mut kinds, (ids.len() == 2).then_some(("Polyline", 2)));
            }
            CubicLine | LagrangeCurve | BezierCurve => {
                push_poly(&mut stream, 2, &ids[..2.min(ids.len())]);
                note(&mut kinds, Some(("Polyline", 2)));
            }
            Polygon => {
                push_poly(&mut stream, 3, ids);
                note(&mut kinds, None);
            }
            TriangleStrip => {
                for k in 0..ids.len().saturating_sub(2) {
                    let t = if k % 2 == 0 { [ids[k], ids[k + 1], ids[k + 2]] } else { [ids[k + 1], ids[k], ids[k + 2]] };
                    stream.push(4);
                    stream.extend(t.iter().map(|&i| i as i64));
                    note(&mut kinds, Some(("Triangle", 3)));
                    cells.push(c);
                }
                continue;
            }
            PentagonalPrism | HexagonalPrism => {
                let n = ids.len() / 2;
                let mut f: Vec<Vec<u32>> = vec![ids[..n].iter().rev().copied().collect(), ids[n..2 * n].to_vec()];
                for i in 0..n {
                    let j = (i + 1) % n;
                    f.push(vec![ids[i], ids[j], ids[n + j], ids[n + i]]);
                }
                push_polyhedron(&mut stream, &f);
                note(&mut kinds, None);
            }
            Polyhedron => match faces.and_then(|f| f.get(c)).filter(|f| !f.is_empty()) {
                Some(f) => {
                    push_polyhedron(&mut stream, f);
                    note(&mut kinds, None);
                }
                None => {
                    *skipped += 1;
                    continue;
                }
            },
            t => match fixed(t) {
                Some((code, name, order)) if order.iter().all(|&k| k < ids.len()) => {
                    stream.push(code);
                    stream.extend(order.iter().map(|&k| ids[k] as i64));
                    note(&mut kinds, Some((name, order.len())));
                }
                _ => {
                    *skipped += 1;
                    continue;
                }
            },
        }
        cells.push(c);
    }
    let single = kinds.flatten();
    if let Some((_, npe)) = single {
        // Drop the codes (and counts of the poly types): plain connectivity.
        let step = if matches!(single, Some(("Polyvertex", _)) | Some(("Polyline", _))) { 2 } else { 1 };
        stream = stream.chunks_exact(step + npe).flat_map(|c| c[step..].to_vec()).collect();
    }
    Topology { stream, cells, single }
}

/// Write `ds` as `path` (`name.xmf`) and `name.h5` next to it.
pub fn write_xdmf(ds: &DataSet, path: &Path) -> Result<()> {
    write_xdmf_steps(&[(0.0, ds)], path)
}

/// Write every step (time, data) as a temporal collection.
pub fn write_xdmf_steps(steps: &[(f64, &DataSet)], path: &Path) -> Result<()> {
    if steps.is_empty() {
        bail!("nothing to write");
    }
    if steps.iter().any(|(_, d)| matches!(d, DataSet::Table(_))) {
        bail!("tables cannot be written as Xdmf");
    }
    let stem = path.file_stem().and_then(|s| s.to_str()).filter(|s| !s.is_empty()).context("the file needs a name")?;
    let h5_name = format!("{stem}.h5");
    let h5_path = path.with_file_name(&h5_name);
    let mut w = Writer { xml: String::new(), h5: Node::group(), h5_name, shared: Default::default() };
    w.line(0, "<?xml version=\"1.0\" encoding=\"utf-8\"?>");
    w.line(0, "<Xdmf xmlns:xi=\"http://www.w3.org/2001/XInclude\" Version=\"3.0\">");
    w.line(2, "<Domain>");
    let mut skipped = 0usize;
    if steps.len() == 1 {
        w.grid(steps[0].1, stem, "/data", None, 4, &mut skipped)?;
    } else {
        w.line(4, &format!("<Grid Name=\"{}\" GridType=\"Collection\" CollectionType=\"Temporal\">", esc(stem)));
        for (i, (t, d)) in steps.iter().enumerate() {
            w.grid(d, &format!("step {i}"), &format!("/s{i}"), Some(*t), 6, &mut skipped)?;
        }
        w.line(4, "</Grid>");
    }
    w.line(2, "</Domain>");
    w.line(0, "</Xdmf>");
    if skipped > 0 {
        tracing::warn!("Xdmf writer: {skipped} cells of unsupported types were left out");
    }
    hdf5w::write(&h5_path, &w.h5).with_context(|| format!("writing {}", h5_path.display()))?;
    std::fs::write(path, &w.xml).with_context(|| format!("writing {}", path.display()))
}
