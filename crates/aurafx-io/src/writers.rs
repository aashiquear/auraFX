//! Writers: VTK XML (.vtu .vtp .vti .vtr .vts .vtm), legacy VTK (.vtk), CSV, STL, PLY; Exodus,
//! VTKHDF, EnSight Gold, Xdmf and CGNS live in their own modules.

use anyhow::{anyhow, bail, Context, Result};
use aurafx_data::*;
use std::io::Write;
use std::path::{Path, PathBuf};

/// (extensions, description) of writable formats.
pub fn writable_formats(ds: &DataSet) -> Vec<(&'static str, &'static str)> {
    let mut v = match ds {
        DataSet::UnstructuredGrid(_) => vec![("vtu", "VTK unstructured grid"), ("inp", "Abaqus input (blocks, node sets, surfaces)"), ("msh", "Gmsh 4.1 (blocks and side sets as physical groups)")],
        DataSet::PolyData(_) => vec![("vtp", "VTK polygonal data"), ("stl", "STL surface"), ("ply", "PLY surface")],
        DataSet::ImageData(_) => vec![("vti", "VTK image data")],
        DataSet::RectilinearGrid(_) => vec![("vtr", "VTK rectilinear grid")],
        DataSet::StructuredGrid(_) => vec![("vts", "VTK structured grid")],
        DataSet::MultiBlock(_) => vec![("vtm", "VTK multiblock (folder of pieces)")],
        DataSet::Table(_) => vec![],
        DataSet::HyperTreeGrid(_) => vec![("htg", "VTK hyper tree grid"), ("vtu", "VTK unstructured grid (the leaves)")],
        DataSet::CellGrid(_) => vec![("dg", "VTK cell grid"), ("vtu", "VTK unstructured grid (values at the corners)")],
    };
    if !matches!(ds, DataSet::MultiBlock(_) | DataSet::Table(_)) {
        v.push(("vtk", "Legacy VTK"));
    }
    if !matches!(ds, DataSet::Table(_)) {
        v.push(("exo", "Exodus II"));
        v.push(("vtkhdf", "VTKHDF (HDF5)"));
        v.push(("case", "EnSight Gold"));
        v.push(("xmf", "Xdmf 3 (with an .h5 file)"));
        v.push(("cgns", "CGNS (HDF5)"));
    }
    v.push(("csv", "CSV (point data or table)"));
    v
}

pub fn write_file(ds: &DataSet, path: &Path) -> Result<()> {
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase();
    if ext == "dg" {
        return match ds {
            DataSet::CellGrid(g) => crate::cellgrid::write(g, path).with_context(|| format!("writing {}", path.display())),
            other => bail!(".dg files hold cell grids, not {} (Convert To Cell Grid first)", other.type_name()),
        };
    }
    if ext == "htg" {
        return match ds {
            DataSet::HyperTreeGrid(h) => write_htg(h, path).with_context(|| format!("writing {}", path.display())),
            other => bail!(".htg files hold hyper tree grids, not {}", other.type_name()),
        };
    }
    // Other formats get a hyper tree grid's visible leaves (an unstructured grid).
    if let DataSet::HyperTreeGrid(h) = ds {
        return write_file(&DataSet::UnstructuredGrid(h.leaves_mesh().0), path);
    }
    if let DataSet::CellGrid(g) = ds {
        return write_file(&DataSet::UnstructuredGrid(g.to_unstructured()), path);
    }
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase();
    let r = match ext.as_str() {
        "vtu" | "vtp" | "vti" | "vtr" | "vts" => write_xml(ds, path),
        "vtm" => write_vtm(ds, path),
        "vtk" => write_legacy(ds, path),
        "csv" => write_csv(ds, path),
        "stl" => write_stl(ds, path),
        "ply" => write_ply(ds, path),
        "e" | "exo" | "ex2" | "exii" | "g" => crate::exodus_writer::write_exodus(ds, path),
        "vtkhdf" | "hdf" => crate::vtkhdf::write_vtkhdf(ds, path),
        "case" | "encas" => crate::ensight_writer::write_ensight(ds, path),
        "cgns" => crate::cgns_writer::write_cgns(ds, path),
        "inp" => crate::mesh_writers::write_abaqus(ds, path),
        "msh" => crate::mesh_writers::write_gmsh(ds, path),
        "xmf" | "xdmf" | "xdmf3" => crate::xdmf_writer::write_xdmf(ds, path),
        e => bail!("don't know how to write .{e} files"),
    };
    r.with_context(|| format!("writing {}", path.display()))
}

// ------------------------------------------------------------------ VTK XML

struct Appended {
    blob: Vec<u8>,
}

impl Appended {
    /// Add raw bytes with a UInt64 length header; returns the offset.
    fn add(&mut self, bytes: &[u8]) -> usize {
        let off = self.blob.len();
        self.blob.extend_from_slice(&(bytes.len() as u64).to_le_bytes());
        self.blob.extend_from_slice(bytes);
        off
    }
}

fn le_bytes(d: &ArrayData) -> Vec<u8> {
    match d {
        ArrayData::I8(v) => v.iter().map(|x| *x as u8).collect(),
        ArrayData::U8(v) => v.clone(),
        ArrayData::I16(v) => v.iter().flat_map(|x| x.to_le_bytes()).collect(),
        ArrayData::U16(v) => v.iter().flat_map(|x| x.to_le_bytes()).collect(),
        ArrayData::I32(v) => v.iter().flat_map(|x| x.to_le_bytes()).collect(),
        ArrayData::U32(v) => v.iter().flat_map(|x| x.to_le_bytes()).collect(),
        ArrayData::I64(v) => v.iter().flat_map(|x| x.to_le_bytes()).collect(),
        ArrayData::U64(v) => v.iter().flat_map(|x| x.to_le_bytes()).collect(),
        ArrayData::F16(v) => v.iter().flat_map(|x| x.to_f32().to_le_bytes()).collect(),
        ArrayData::F32(v) => v.iter().flat_map(|x| x.to_le_bytes()).collect(),
        ArrayData::F64(v) => v.iter().flat_map(|x| x.to_le_bytes()).collect(),
        ArrayData::String(v) => v.iter().flat_map(|s| s.bytes().chain(std::iter::once(0u8))).collect(),
    }
}

fn type_name(d: &ArrayData) -> &'static str {
    match d.scalar_type() {
        ScalarType::F16 => "Float32",
        t => t.vtk_name(),
    }
}

fn esc(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

fn data_array(out: &mut String, app: &mut Appended, name: &str, ncomp: usize, d: &ArrayData) {
    let off = app.add(&le_bytes(d));
    out.push_str(&format!(
        "        <DataArray type=\"{}\" Name=\"{}\" NumberOfComponents=\"{}\" format=\"appended\" offset=\"{}\"/>\n",
        type_name(d),
        esc(name),
        ncomp,
        off
    ));
}

fn attributes(out: &mut String, app: &mut Appended, tag: &str, a: &Attributes) {
    let mut act = String::new();
    for (k, attr) in [
        (AttributeKind::Scalars, "Scalars"),
        (AttributeKind::Vectors, "Vectors"),
        (AttributeKind::Normals, "Normals"),
        (AttributeKind::TCoords, "TCoords"),
        (AttributeKind::Tensors, "Tensors"),
        (AttributeKind::GlobalIds, "GlobalIds"),
    ] {
        if let Some(n) = a.active.get(&k) {
            act += &format!(" {attr}=\"{}\"", esc(n));
        }
    }
    // Higher-order cell attributes VTK only uses when they are tagged.
    for (name, need) in [("HigherOrderDegrees", 3), ("RationalWeights", 1)] {
        if a.get(name).is_some_and(|x| x.ncomp == need) {
            act += &format!(" {name}=\"{name}\"");
        }
    }
    out.push_str(&format!("      <{tag}{act}>\n"));
    for arr in a.iter() {
        data_array(out, app, &arr.name, arr.ncomp, &arr.data);
    }
    out.push_str(&format!("      </{tag}>\n"));
}

fn cells_block(out: &mut String, app: &mut Appended, tag: &str, c: &Cells, with_types: bool) {
    out.push_str(&format!("      <{tag}>\n"));
    data_array(out, app, "connectivity", 1, &ArrayData::I64(c.connectivity.iter().map(|&x| x as i64).collect()));
    data_array(out, app, "offsets", 1, &ArrayData::I64(c.offsets.iter().skip(1).map(|&x| x as i64).collect()));
    if with_types {
        data_array(out, app, "types", 1, &ArrayData::U8(c.types.iter().map(|t| t.vtk_id()).collect()));
        if let Some(faces) = &c.polyhedron_faces {
            let mut f = Vec::new();
            let mut fo = Vec::new();
            for cell in faces {
                if cell.is_empty() {
                    fo.push(-1i64);
                    continue;
                }
                f.push(cell.len() as i64);
                for face in cell {
                    f.push(face.len() as i64);
                    f.extend(face.iter().map(|&x| x as i64));
                }
                fo.push(f.len() as i64);
            }
            data_array(out, app, "faces", 1, &ArrayData::I64(f));
            data_array(out, app, "faceoffsets", 1, &ArrayData::I64(fo));
        }
    }
    out.push_str(&format!("      </{tag}>\n"));
}

fn extent_str(e: &[i64; 6]) -> String {
    e.iter().map(|x| x.to_string()).collect::<Vec<_>>().join(" ")
}

pub fn write_xml(ds: &DataSet, path: &Path) -> Result<()> {
    let mut app = Appended { blob: Vec::new() };
    let mut body = String::new();
    let ty = match ds {
        DataSet::HyperTreeGrid(h) => return write_xml(&DataSet::UnstructuredGrid(h.leaves_mesh().0), path),
        DataSet::CellGrid(g) => return write_xml(&DataSet::UnstructuredGrid(g.to_unstructured()), path),
        DataSet::UnstructuredGrid(u) => {
            body += &format!("  <UnstructuredGrid>\n    <Piece NumberOfPoints=\"{}\" NumberOfCells=\"{}\">\n", u.points.len(), u.cells.len());
            attributes(&mut body, &mut app, "PointData", &u.point_data);
            attributes(&mut body, &mut app, "CellData", &u.cell_data);
            body += "      <Points>\n";
            data_array(&mut body, &mut app, "Points", 3, &u.points.data);
            body += "      </Points>\n";
            cells_block(&mut body, &mut app, "Cells", &u.cells, true);
            body += "    </Piece>\n";
            if !u.field_data.is_empty() {
                attributes(&mut body, &mut app, "FieldData", &u.field_data);
            }
            body += "  </UnstructuredGrid>\n";
            "UnstructuredGrid"
        }
        DataSet::PolyData(p) => {
            body += &format!(
                "  <PolyData>\n    <Piece NumberOfPoints=\"{}\" NumberOfVerts=\"{}\" NumberOfLines=\"{}\" NumberOfStrips=\"{}\" NumberOfPolys=\"{}\">\n",
                p.points.len(),
                p.verts.len(),
                p.lines.len(),
                p.strips.len(),
                p.polys.len()
            );
            attributes(&mut body, &mut app, "PointData", &p.point_data);
            attributes(&mut body, &mut app, "CellData", &p.cell_data);
            body += "      <Points>\n";
            data_array(&mut body, &mut app, "Points", 3, &p.points.data);
            body += "      </Points>\n";
            for (tag, c) in [("Verts", &p.verts), ("Lines", &p.lines), ("Strips", &p.strips), ("Polys", &p.polys)] {
                cells_block(&mut body, &mut app, tag, c, false);
            }
            body += "    </Piece>\n";
            if !p.field_data.is_empty() {
                attributes(&mut body, &mut app, "FieldData", &p.field_data);
            }
            body += "  </PolyData>\n";
            "PolyData"
        }
        DataSet::ImageData(im) => {
            let e = extent_str(&im.extent);
            let d = im.direction.iter().map(|x| x.to_string()).collect::<Vec<_>>().join(" ");
            body += &format!(
                "  <ImageData WholeExtent=\"{e}\" Origin=\"{} {} {}\" Spacing=\"{} {} {}\" Direction=\"{d}\">\n    <Piece Extent=\"{e}\">\n",
                im.origin[0], im.origin[1], im.origin[2], im.spacing[0], im.spacing[1], im.spacing[2]
            );
            attributes(&mut body, &mut app, "PointData", &im.point_data);
            attributes(&mut body, &mut app, "CellData", &im.cell_data);
            body += "    </Piece>\n  </ImageData>\n";
            "ImageData"
        }
        DataSet::RectilinearGrid(g) => {
            let e = extent_str(&g.extent);
            body += &format!("  <RectilinearGrid WholeExtent=\"{e}\">\n    <Piece Extent=\"{e}\">\n");
            attributes(&mut body, &mut app, "PointData", &g.point_data);
            attributes(&mut body, &mut app, "CellData", &g.cell_data);
            body += "      <Coordinates>\n";
            for (n, c) in [("x", &g.x), ("y", &g.y), ("z", &g.z)] {
                data_array(&mut body, &mut app, n, 1, &ArrayData::F64(c.clone()));
            }
            body += "      </Coordinates>\n    </Piece>\n  </RectilinearGrid>\n";
            "RectilinearGrid"
        }
        DataSet::StructuredGrid(g) => {
            let e = extent_str(&g.extent);
            body += &format!("  <StructuredGrid WholeExtent=\"{e}\">\n    <Piece Extent=\"{e}\">\n");
            attributes(&mut body, &mut app, "PointData", &g.point_data);
            attributes(&mut body, &mut app, "CellData", &g.cell_data);
            body += "      <Points>\n";
            data_array(&mut body, &mut app, "Points", 3, &g.points.data);
            body += "      </Points>\n    </Piece>\n  </StructuredGrid>\n";
            "StructuredGrid"
        }
        DataSet::Table(_) => bail!("tables are written as CSV"),
        DataSet::MultiBlock(_) => return write_vtm(ds, path),
    };
    let mut f = std::io::BufWriter::new(std::fs::File::create(path)?);
    write!(f, "<?xml version=\"1.0\"?>\n<VTKFile type=\"{ty}\" version=\"2.2\" byte_order=\"LittleEndian\" header_type=\"UInt64\">\n{body}  <AppendedData encoding=\"raw\">\n   _")?;
    f.write_all(&app.blob)?;
    write!(f, "\n  </AppendedData>\n</VTKFile>\n")?;
    Ok(())
}

/// Bits packed most significant first, as VTK's bit arrays.
fn pack_bits(bits: &[bool]) -> Vec<u8> {
    let mut out = vec![0u8; bits.len().div_ceil(8)];
    for (i, _) in bits.iter().enumerate().filter(|(_, b)| **b) {
        out[i / 8] |= 0x80 >> (i % 8);
    }
    out
}

/// VTK hyper tree grid file (`.htg`, XML format version 2, as ParaView 5.10 and later write):
/// the root grid's coordinates; for every tree in index order its breadth-first refinement
/// descriptor (all levels but the last), its vertices per depth, the mask, and the cell data in
/// that breadth-first order.
pub fn write_htg(h: &aurafx_data::HyperTreeGrid, path: &Path) -> Result<()> {
    let mut app = Appended { blob: Vec::new() };
    let nc = h.children_per_vertex();
    let (mut descriptor, mut per_depth, mut tree_ids, mut depth_per_tree, mut mask, mut order) = (Vec::new(), Vec::<i64>::new(), Vec::<i64>::new(), Vec::<u32>::new(), Vec::new(), Vec::new());
    for (ti, tree) in h.trees.iter().enumerate() {
        let Some(tree) = tree.as_ref().filter(|t| !t.refined.is_empty()) else { continue };
        // Level sizes of the breadth-first list.
        let mut sizes = vec![1usize];
        let mut start = 0;
        loop {
            let n = *sizes.last().unwrap();
            let refined = tree.refined[start..(start + n).min(tree.refined.len())].iter().filter(|r| **r).count();
            start += n;
            if refined == 0 || start >= tree.refined.len() {
                break;
            }
            sizes.push(refined * nc);
        }
        let total: usize = sizes.iter().sum();
        if total != tree.refined.len() {
            bail!("tree {ti}: {} vertices but its refinement gives {total}", tree.refined.len());
        }
        let last = *sizes.last().unwrap();
        descriptor.extend_from_slice(&tree.refined[..total - last]);
        tree_ids.push(ti as i64);
        depth_per_tree.push(sizes.len() as u32);
        per_depth.extend(sizes.iter().map(|&s| s as i64));
        mask.extend(tree.global.iter().map(|&g| h.is_masked(g)));
        order.extend_from_slice(&tree.global);
    }
    let mut body = String::new();
    let dims = h.dims();
    body += &format!(
        "  <HyperTreeGrid BranchFactor=\"{}\" TransposedRootIndexing=\"{}\" Dimensions=\"{} {} {}\"{}>\n    <Grid>\n",
        h.branch_factor,
        h.transposed as u8,
        dims[0],
        dims[1],
        dims[2],
        h.interface.as_ref().map(|(n, i)| format!(" InterfaceNormalsName=\"{}\" InterfaceInterceptsName=\"{}\"", n, i)).unwrap_or_default()
    );
    for (k, name) in ["XCoordinates", "YCoordinates", "ZCoordinates"].iter().enumerate() {
        let off = app.add(&le_bytes(&ArrayData::F64(h.coords[k].clone())));
        body += &format!("      <DataArray type=\"Float64\" Name=\"{name}\" NumberOfTuples=\"{}\" format=\"appended\" offset=\"{off}\"/>\n", h.coords[k].len());
    }
    body += "    </Grid>\n    <Trees>\n";
    let bits = |app: &mut Appended, body: &mut String, name: &str, b: &[bool]| {
        let off = app.add(&pack_bits(b));
        *body += &format!("      <DataArray type=\"Bit\" Name=\"{name}\" NumberOfTuples=\"{}\" format=\"appended\" offset=\"{off}\"/>\n", b.len());
    };
    bits(&mut app, &mut body, "Descriptors", &descriptor);
    for (name, d) in [("NumberOfVerticesPerDepth", ArrayData::I64(per_depth)), ("TreeIds", ArrayData::I64(tree_ids)), ("DepthPerTree", ArrayData::U32(depth_per_tree))] {
        let off = app.add(&le_bytes(&d));
        body += &format!("      <DataArray type=\"{}\" Name=\"{name}\" NumberOfTuples=\"{}\" format=\"appended\" offset=\"{off}\"/>\n", type_name(&d), d.len());
    }
    if h.mask.is_some() {
        bits(&mut app, &mut body, "Mask", &mask);
    }
    body += "    </Trees>\n";
    let n_cells = h.cell_data.iter().map(|a| a.len()).max().unwrap_or(0);
    let ok: Vec<usize> = order.iter().copied().filter(|&g| g < n_cells).collect();
    let mut cd = h.cell_data.clone();
    if ok.len() == order.len() {
        cd = cd.gather(&order);
    } else if !cd.is_empty() {
        bail!("cell data shorter than the trees ({n_cells} values, {} cells)", order.len());
    }
    for arr in cd.iter() {
        let off = app.add(&le_bytes(&arr.data));
        body += &format!(
            "      <DataArray type=\"{}\" Name=\"{}\" NumberOfComponents=\"{}\" NumberOfTuples=\"{}\" format=\"appended\" offset=\"{off}\"/>\n",
            type_name(&arr.data),
            esc(&arr.name),
            arr.ncomp,
            arr.len()
        );
    }
    body = body.replacen("    </Trees>\n", "    </Trees>\n    <CellData>\n", 1);
    body += "    </CellData>\n";
    if !h.field_data.is_empty() {
        attributes(&mut body, &mut app, "FieldData", &h.field_data);
    }
    body += "  </HyperTreeGrid>\n";
    let mut f = std::io::BufWriter::new(std::fs::File::create(path)?);
    write!(f, "<?xml version=\"1.0\"?>\n<VTKFile type=\"HyperTreeGrid\" version=\"2.0\" byte_order=\"LittleEndian\" header_type=\"UInt64\">\n{body}  <AppendedData encoding=\"raw\">\n   _")?;
    f.write_all(&app.blob)?;
    write!(f, "\n  </AppendedData>\n</VTKFile>\n")?;
    Ok(())
}

/// Multiblock: writes `<name>.vtm` and a folder `<name>/` with one file per leaf.
pub fn write_vtm(ds: &DataSet, path: &Path) -> Result<()> {
    let DataSet::MultiBlock(mb) = ds else { bail!("vtm needs a multiblock dataset") };
    let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("data").to_string();
    let dir: PathBuf = path.with_file_name(&stem);
    std::fs::create_dir_all(&dir)?;
    let mut xml = String::from("<?xml version=\"1.0\"?>\n<VTKFile type=\"vtkMultiBlockDataSet\" version=\"1.0\" byte_order=\"LittleEndian\" header_type=\"UInt64\">\n  <vtkMultiBlockDataSet>\n");
    fn walk(mb: &MultiBlock, dir: &Path, stem: &str, xml: &mut String, counter: &mut usize, depth: usize) -> Result<()> {
        for (i, (name, b)) in mb.blocks.iter().enumerate() {
            let pad = "  ".repeat(depth + 2);
            match b {
                Some(DataSet::MultiBlock(inner)) => {
                    xml.push_str(&format!("{pad}<Block index=\"{i}\" name=\"{}\">\n", esc(name)));
                    walk(inner, dir, stem, xml, counter, depth + 1)?;
                    xml.push_str(&format!("{pad}</Block>\n"));
                }
                Some(leaf) => {
                    let ext = match leaf {
                        DataSet::UnstructuredGrid(_) => "vtu",
                        DataSet::PolyData(_) => "vtp",
                        DataSet::ImageData(_) => "vti",
                        DataSet::RectilinearGrid(_) => "vtr",
                        DataSet::StructuredGrid(_) => "vts",
                        _ => continue,
                    };
                    let fname = format!("{stem}_{}.{ext}", *counter);
                    *counter += 1;
                    write_xml(leaf, &dir.join(&fname))?;
                    xml.push_str(&format!("{pad}<DataSet index=\"{i}\" name=\"{}\" file=\"{}/{}\"/>\n", esc(name), stem, fname));
                }
                None => {}
            }
        }
        Ok(())
    }
    let mut counter = 0;
    walk(mb, &dir, &stem, &mut xml, &mut counter, 0)?;
    xml.push_str("  </vtkMultiBlockDataSet>\n</VTKFile>\n");
    std::fs::write(path, xml)?;
    Ok(())
}

// ------------------------------------------------------------------ legacy VTK (binary, big-endian)

fn be_bytes(d: &ArrayData) -> (Vec<u8>, &'static str) {
    match d {
        ArrayData::I8(v) => (v.iter().map(|x| *x as u8).collect(), "char"),
        ArrayData::U8(v) => (v.clone(), "unsigned_char"),
        ArrayData::I16(v) => (v.iter().flat_map(|x| x.to_be_bytes()).collect(), "short"),
        ArrayData::U16(v) => (v.iter().flat_map(|x| x.to_be_bytes()).collect(), "unsigned_short"),
        ArrayData::I32(v) => (v.iter().flat_map(|x| x.to_be_bytes()).collect(), "int"),
        ArrayData::U32(v) => (v.iter().flat_map(|x| x.to_be_bytes()).collect(), "unsigned_int"),
        ArrayData::I64(v) => (v.iter().flat_map(|x| x.to_be_bytes()).collect(), "vtktypeint64"),
        ArrayData::U64(v) => (v.iter().flat_map(|x| x.to_be_bytes()).collect(), "vtktypeuint64"),
        ArrayData::F16(v) => (v.iter().flat_map(|x| x.to_f32().to_be_bytes()).collect(), "float"),
        ArrayData::F32(v) => (v.iter().flat_map(|x| x.to_be_bytes()).collect(), "float"),
        ArrayData::F64(v) => (v.iter().flat_map(|x| x.to_be_bytes()).collect(), "double"),
        ArrayData::String(_) => (vec![], "string"),
    }
}

fn legacy_name(n: &str) -> String {
    let s: String = n.chars().map(|c| if c.is_whitespace() { '_' } else { c }).collect();
    if s.is_empty() {
        "array".into()
    } else {
        s
    }
}

fn legacy_attrs(f: &mut impl Write, a: &Attributes, n: usize, kw: &str) -> Result<()> {
    let arrays: Vec<&Array> = a.iter().filter(|x| x.scalar_type() != ScalarType::String).collect();
    if arrays.is_empty() || n == 0 {
        return Ok(());
    }
    writeln!(f, "{kw} {n}\nFIELD FieldData {}", arrays.len())?;
    for arr in arrays {
        let (b, t) = be_bytes(&arr.data);
        writeln!(f, "{} {} {} {t}", legacy_name(&arr.name), arr.ncomp, arr.len())?;
        f.write_all(&b)?;
        writeln!(f)?;
    }
    Ok(())
}

fn legacy_cells(f: &mut impl Write, kw: &str, c: &Cells) -> Result<()> {
    if c.is_empty() {
        return Ok(());
    }
    writeln!(f, "{kw} {} {}", c.len() + 1, c.connectivity.len())?;
    writeln!(f, "OFFSETS vtktypeint64")?;
    f.write_all(&c.offsets.iter().flat_map(|&x| (x as i64).to_be_bytes()).collect::<Vec<u8>>())?;
    writeln!(f, "\nCONNECTIVITY vtktypeint64")?;
    f.write_all(&c.connectivity.iter().flat_map(|&x| (x as i64).to_be_bytes()).collect::<Vec<u8>>())?;
    writeln!(f)?;
    Ok(())
}

pub fn write_legacy(ds: &DataSet, path: &Path) -> Result<()> {
    let mut f = std::io::BufWriter::new(std::fs::File::create(path)?);
    writeln!(f, "# vtk DataFile Version 5.1\nWritten by auraFX\nBINARY")?;
    let points = |f: &mut std::io::BufWriter<std::fs::File>, p: &Points| -> Result<()> {
        let (b, t) = be_bytes(&p.data);
        writeln!(f, "POINTS {} {t}", p.len())?;
        f.write_all(&b)?;
        writeln!(f)?;
        Ok(())
    };
    match ds {
        DataSet::UnstructuredGrid(u) => {
            writeln!(f, "DATASET UNSTRUCTURED_GRID")?;
            points(&mut f, &u.points)?;
            legacy_cells(&mut f, "CELLS", &u.cells)?;
            writeln!(f, "CELL_TYPES {}", u.cells.len())?;
            f.write_all(&u.cells.types.iter().flat_map(|t| (t.vtk_id() as i32).to_be_bytes()).collect::<Vec<u8>>())?;
            writeln!(f)?;
            legacy_attrs(&mut f, &u.cell_data, u.cells.len(), "CELL_DATA")?;
            legacy_attrs(&mut f, &u.point_data, u.points.len(), "POINT_DATA")?;
        }
        DataSet::PolyData(p) => {
            writeln!(f, "DATASET POLYDATA")?;
            points(&mut f, &p.points)?;
            legacy_cells(&mut f, "VERTICES", &p.verts)?;
            legacy_cells(&mut f, "LINES", &p.lines)?;
            legacy_cells(&mut f, "POLYGONS", &p.polys)?;
            legacy_cells(&mut f, "TRIANGLE_STRIPS", &p.strips)?;
            legacy_attrs(&mut f, &p.cell_data, p.n_cells(), "CELL_DATA")?;
            legacy_attrs(&mut f, &p.point_data, p.points.len(), "POINT_DATA")?;
        }
        DataSet::ImageData(im) => {
            let d = im.dims();
            writeln!(f, "DATASET STRUCTURED_POINTS\nDIMENSIONS {} {} {}", d[0], d[1], d[2])?;
            let e = &im.extent;
            let o = [im.origin[0] + e[0] as f64 * im.spacing[0], im.origin[1] + e[2] as f64 * im.spacing[1], im.origin[2] + e[4] as f64 * im.spacing[2]];
            writeln!(f, "ORIGIN {} {} {}\nSPACING {} {} {}", o[0], o[1], o[2], im.spacing[0], im.spacing[1], im.spacing[2])?;
            legacy_attrs(&mut f, &im.cell_data, im.n_cells(), "CELL_DATA")?;
            legacy_attrs(&mut f, &im.point_data, im.n_points(), "POINT_DATA")?;
        }
        DataSet::RectilinearGrid(g) => {
            writeln!(f, "DATASET RECTILINEAR_GRID\nDIMENSIONS {} {} {}", g.x.len(), g.y.len(), g.z.len())?;
            for (kw, c) in [("X_COORDINATES", &g.x), ("Y_COORDINATES", &g.y), ("Z_COORDINATES", &g.z)] {
                writeln!(f, "{kw} {} double", c.len())?;
                f.write_all(&c.iter().flat_map(|x| x.to_be_bytes()).collect::<Vec<u8>>())?;
                writeln!(f)?;
            }
            legacy_attrs(&mut f, &g.cell_data, ds.n_cells(), "CELL_DATA")?;
            legacy_attrs(&mut f, &g.point_data, g.n_points(), "POINT_DATA")?;
        }
        DataSet::StructuredGrid(g) => {
            let d = g.dims();
            writeln!(f, "DATASET STRUCTURED_GRID\nDIMENSIONS {} {} {}", d[0], d[1], d[2])?;
            points(&mut f, &g.points)?;
            legacy_attrs(&mut f, &g.cell_data, ds.n_cells(), "CELL_DATA")?;
            legacy_attrs(&mut f, &g.point_data, g.points.len(), "POINT_DATA")?;
        }
        _ => bail!("legacy VTK cannot store {}", ds.type_name()),
    }
    Ok(())
}

// ------------------------------------------------------------------ CSV / STL / PLY

pub fn write_csv(ds: &DataSet, path: &Path) -> Result<()> {
    let leaf = match ds {
        DataSet::MultiBlock(mb) => mb.leaves().into_iter().next().map(|(_, d)| d.clone()).ok_or_else(|| anyhow!("empty dataset"))?,
        d => d.clone(),
    };
    let (attrs, n, pts) = match &leaf {
        DataSet::Table(t) => (t.columns.clone(), t.n_rows(), None),
        d => (d.point_data().cloned().unwrap_or_default(), d.n_points(), d.points()),
    };
    let mut f = std::io::BufWriter::new(std::fs::File::create(path)?);
    let mut header: Vec<String> = Vec::new();
    if pts.is_some() {
        header.extend(["Points:0", "Points:1", "Points:2"].map(String::from));
    }
    for a in attrs.iter() {
        if a.ncomp == 1 {
            header.push(format!("\"{}\"", a.name));
        } else {
            header.extend((0..a.ncomp).map(|c| format!("\"{}:{c}\"", a.name)));
        }
    }
    writeln!(f, "{}", header.join(","))?;
    for i in 0..n {
        let mut row: Vec<String> = Vec::with_capacity(header.len());
        if let Some(p) = &pts {
            row.extend(p.get(i).iter().map(|v| v.to_string()));
        }
        for a in attrs.iter() {
            if a.scalar_type() == ScalarType::String {
                row.push(format!("\"{}\"", a.as_slice::<String>().map(|s| s[i].clone()).unwrap_or_default()));
            } else {
                for c in 0..a.ncomp {
                    row.push(a.get(i, c).to_string());
                }
            }
        }
        writeln!(f, "{}", row.join(","))?;
    }
    Ok(())
}

fn surface_triangles(ds: &DataSet) -> Result<(Vec<[f32; 3]>, Vec<[u32; 3]>)> {
    let DataSet::PolyData(p) = ds else { bail!("STL/PLY need a surface (apply Extract Surface first)") };
    let pts: Vec<[f32; 3]> = (0..p.points.len()).map(|i| p.points.get(i).map(|x| x as f32)).collect();
    let mut tris = Vec::new();
    for c in [&p.polys, &p.strips] {
        for i in 0..c.len() {
            let cell = c.cell(i);
            if c.types.get(i) == Some(&CellType::TriangleStrip) {
                for k in 0..cell.len().saturating_sub(2) {
                    tris.push(if k % 2 == 0 { [cell[k], cell[k + 1], cell[k + 2]] } else { [cell[k + 1], cell[k], cell[k + 2]] });
                }
            } else {
                for k in 1..cell.len().saturating_sub(1) {
                    tris.push([cell[0], cell[k], cell[k + 1]]);
                }
            }
        }
    }
    Ok((pts, tris))
}

pub fn write_stl(ds: &DataSet, path: &Path) -> Result<()> {
    let (pts, tris) = surface_triangles(ds)?;
    let mut f = std::io::BufWriter::new(std::fs::File::create(path)?);
    let mut header = [0u8; 80];
    header[..17].copy_from_slice(b"auraFX binary STL");
    f.write_all(&header)?;
    f.write_all(&(tris.len() as u32).to_le_bytes())?;
    for t in tris {
        let (a, b, c) = (pts[t[0] as usize], pts[t[1] as usize], pts[t[2] as usize]);
        let u = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
        let v = [c[0] - a[0], c[1] - a[1], c[2] - a[2]];
        let n = [u[1] * v[2] - u[2] * v[1], u[2] * v[0] - u[0] * v[2], u[0] * v[1] - u[1] * v[0]];
        let l = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt().max(1e-30);
        for x in [n[0] / l, n[1] / l, n[2] / l] {
            f.write_all(&x.to_le_bytes())?;
        }
        for p in [a, b, c] {
            for x in p {
                f.write_all(&x.to_le_bytes())?;
            }
        }
        f.write_all(&[0, 0])?;
    }
    Ok(())
}

pub fn write_ply(ds: &DataSet, path: &Path) -> Result<()> {
    let (pts, tris) = surface_triangles(ds)?;
    let mut f = std::io::BufWriter::new(std::fs::File::create(path)?);
    write!(f, "ply\nformat binary_little_endian 1.0\ncomment auraFX\nelement vertex {}\nproperty float x\nproperty float y\nproperty float z\nelement face {}\nproperty list uchar int vertex_indices\nend_header\n", pts.len(), tris.len())?;
    for p in &pts {
        for x in p {
            f.write_all(&x.to_le_bytes())?;
        }
    }
    for t in &tris {
        f.write_all(&[3u8])?;
        for i in t {
            f.write_all(&(*i as i32).to_le_bytes())?;
        }
    }
    Ok(())
}
