//! VTK XML readers: .vti .vtr .vts .vtp .vtu .vtt, parallel .p* files, .vtm multi-block, .pvd collections.
//!
//! Supports inline ASCII, inline base64 binary, and appended (raw or base64) data with
//! zlib or LZ4 block compression and 32- or 64-bit headers.

use anyhow::{anyhow, bail, Context, Result};
use aurafx_data::*;
use base64::Engine;
use quick_xml::events::{BytesStart, Event};
use quick_xml::Reader;
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

#[derive(Debug, Clone, Copy, PartialEq)]
enum Compressor {
    None,
    Zlib,
    Lz4,
    Lzma,
}

struct FileCtx<'a> {
    little_endian: bool,
    header64: bool,
    compressor: Compressor,
    appended: Option<&'a [u8]>,
    appended_base64: bool,
    /// File version older than 2.2: Lagrange/Bezier hexahedra list their last two vertical
    /// edges the other way round (VTK converts them on reading too).
    old_hex_order: bool,
}

/// Generic XML tree node (small files only hold metadata; bulk data is referenced by offset).
#[derive(Debug, Default)]
struct Node {
    name: String,
    attrs: HashMap<String, String>,
    children: Vec<Node>,
    text: String,
}

impl Node {
    fn child(&self, name: &str) -> Option<&Node> {
        self.children.iter().find(|c| c.name == name)
    }
    fn children_named<'a>(&'a self, name: &'a str) -> impl Iterator<Item = &'a Node> + 'a {
        self.children.iter().filter(move |c| c.name == name)
    }
    fn attr(&self, name: &str) -> Option<&str> {
        self.attrs.get(name).map(|s| s.as_str())
    }
}

fn attrs_of(e: &BytesStart) -> Result<HashMap<String, String>> {
    let mut m = HashMap::new();
    for a in e.attributes() {
        let a = a?;
        let k = a.key.as_ref().to_string();
        let v = a.normalized_value(quick_xml::XmlVersion::default()).map(|v| v.to_string()).unwrap_or_else(|_| a.value.to_string());
        m.insert(k, v);
    }
    Ok(m)
}

fn parse_tree(xml: &[u8]) -> Result<Node> {
    let mut reader = Reader::from_reader(xml);
    reader.config_mut().trim_text(true);
    let mut stack: Vec<Node> = vec![Node { name: "<root>".into(), ..Default::default() }];
    let mut buf = Vec::new();
    loop {
        match reader.read_event_into(&mut buf)? {
            Event::Start(e) => {
                stack.push(Node { name: e.name().as_ref().to_string(), attrs: attrs_of(&e)?, ..Default::default() });
            }
            Event::Empty(e) => {
                let n = Node { name: e.name().as_ref().to_string(), attrs: attrs_of(&e)?, ..Default::default() };
                stack.last_mut().unwrap().children.push(n);
            }
            Event::End(_) => {
                let n = stack.pop().unwrap();
                stack.last_mut().unwrap().children.push(n);
            }
            Event::Text(t) => {
                let s = t.xml_content(quick_xml::XmlVersion::default()).to_string();
                let top = stack.last_mut().unwrap();
                if !top.text.is_empty() {
                    top.text.push(' ');
                }
                top.text.push_str(s.trim());
            }
            Event::Eof => break,
            _ => {}
        }
        buf.clear();
    }
    while stack.len() > 1 {
        let n = stack.pop().unwrap();
        stack.last_mut().unwrap().children.push(n);
    }
    Ok(stack.pop().unwrap())
}

/// Split the file into the XML part (with appended data removed) and the appended blob.
fn split_appended(bytes: &[u8]) -> (Vec<u8>, Option<(&[u8], bool)>) {
    let Some(start) = memchr::memmem::find(bytes, b"<AppendedData") else {
        return (bytes.to_vec(), None);
    };
    let tag_end = start + memchr::memchr(b'>', &bytes[start..]).unwrap_or(0);
    let tag = String::from_utf8_lossy(&bytes[start..=tag_end]).to_string();
    let base64_enc = tag.contains("base64");
    let underscore = tag_end + 1 + memchr::memchr(b'_', &bytes[tag_end + 1..]).unwrap_or(0) + 1;
    let end = memchr::memmem::rfind(bytes, b"</AppendedData>").unwrap_or(bytes.len());
    let blob = &bytes[underscore..end];
    let mut xml = bytes[..start].to_vec();
    xml.extend_from_slice(b"</VTKFile>");
    (xml, Some((blob, base64_enc)))
}

fn read_header_values(ctx: &FileCtx, bytes: &[u8], n: usize) -> Vec<u64> {
    let sz = if ctx.header64 { 8 } else { 4 };
    (0..n)
        .map(|i| {
            let c = &bytes[i * sz..(i + 1) * sz];
            if ctx.header64 {
                let a: [u8; 8] = c.try_into().unwrap();
                if ctx.little_endian {
                    u64::from_le_bytes(a)
                } else {
                    u64::from_be_bytes(a)
                }
            } else {
                let a: [u8; 4] = c.try_into().unwrap();
                (if ctx.little_endian { u32::from_le_bytes(a) } else { u32::from_be_bytes(a) }) as u64
            }
        })
        .collect()
}

fn decompress_block(ctx: &FileCtx, data: &[u8], out_size: usize) -> Result<Vec<u8>> {
    match ctx.compressor {
        Compressor::None => Ok(data.to_vec()),
        Compressor::Zlib => {
            use std::io::Read;
            let mut out = Vec::with_capacity(out_size);
            flate2::read::ZlibDecoder::new(data).read_to_end(&mut out)?;
            Ok(out)
        }
        Compressor::Lz4 => Ok(lz4_flex::block::decompress(data, out_size)?),
        Compressor::Lzma => bail!("LZMA-compressed VTK XML is not supported yet"),
    }
}

/// Decode a binary payload (header + blocks) starting at `bytes[0]`. Returns decoded bytes.
fn decode_binary_payload(ctx: &FileCtx, bytes: &[u8]) -> Result<Vec<u8>> {
    let hsz = if ctx.header64 { 8 } else { 4 };
    if ctx.compressor == Compressor::None {
        let n = read_header_values(ctx, bytes, 1)[0] as usize;
        if hsz + n > bytes.len() {
            bail!("truncated binary array ({} bytes needed, {} available)", hsz + n, bytes.len());
        }
        return Ok(bytes[hsz..hsz + n].to_vec());
    }
    let head = read_header_values(ctx, bytes, 3);
    let nblocks = head[0] as usize;
    let block_size = head[1] as usize;
    let last_size = head[2] as usize;
    let sizes = read_header_values(ctx, &bytes[3 * hsz..], nblocks);
    let mut pos = (3 + nblocks) * hsz;
    let mut out = Vec::with_capacity(block_size * nblocks);
    for (i, &cs) in sizes.iter().enumerate() {
        let cs = cs as usize;
        let expect = if i + 1 == nblocks && last_size > 0 { last_size } else { block_size };
        if pos + cs > bytes.len() {
            bail!("truncated compressed block");
        }
        out.extend(decompress_block(ctx, &bytes[pos..pos + cs], expect)?);
        pos += cs;
    }
    Ok(out)
}

fn b64_decode(s: &[u8]) -> Result<Vec<u8>> {
    let cleaned: Vec<u8> = s.iter().copied().filter(|b| !b.is_ascii_whitespace()).collect();
    Ok(base64::engine::general_purpose::STANDARD.decode(&cleaned).or_else(|_| base64::engine::general_purpose::STANDARD_NO_PAD.decode(&cleaned))?)
}

/// Decode a base64 payload where (for compressed data) the header and the data are encoded separately.
/// Only the characters belonging to this array are decoded, so `text` may be followed by other arrays.
fn decode_base64_payload(ctx: &FileCtx, text: &[u8]) -> Result<Vec<u8>> {
    let cleaned: Vec<u8> = text.iter().copied().filter(|b| !b.is_ascii_whitespace()).collect();
    let hsz = if ctx.header64 { 8 } else { 4 };
    let chars_for = |nbytes: usize| nbytes.div_ceil(3) * 4;
    if ctx.compressor == Compressor::None {
        // Header and data are one base64 stream.
        let first = b64_decode(&cleaned[..chars_for(hsz).min(cleaned.len())])?;
        let nbytes = read_header_values(ctx, &first, 1)[0] as usize;
        let all = b64_decode(&cleaned[..chars_for(hsz + nbytes).min(cleaned.len())])?;
        return decode_binary_payload(ctx, &all);
    }
    let first = b64_decode(&cleaned[..chars_for(hsz).min(cleaned.len())])?;
    let nblocks = read_header_values(ctx, &first, 1)[0] as usize;
    let header_len = (3 + nblocks) * hsz;
    let hchars = chars_for(header_len).min(cleaned.len());
    let header = b64_decode(&cleaned[..hchars])?;
    let sizes = read_header_values(ctx, &header[3 * hsz..], nblocks);
    let total: usize = sizes.iter().map(|&s| s as usize).sum();
    let dchars = chars_for(total).min(cleaned.len() - hchars);
    let data = b64_decode(&cleaned[hchars..hchars + dchars])?;
    let mut joined = header;
    joined.extend(data);
    decode_binary_payload(ctx, &joined)
}

fn read_array(ctx: &FileCtx, node: &Node) -> Result<Array> {
    let name = node.attr("Name").unwrap_or("").to_string();
    let ty = parse_type(node.attr("type").unwrap_or("Float32"))?;
    let ncomp: usize = node.attr("NumberOfComponents").and_then(|s| s.parse().ok()).unwrap_or(1);
    let format = node.attr("format").unwrap_or("ascii");
    let data = match format {
        "ascii" => {
            if ty == ScalarType::String {
                // VTK encodes strings as a byte stream of NUL-terminated strings in ascii mode.
                let bytes: Vec<u8> = node.text.split_whitespace().filter_map(|t| t.parse::<u8>().ok()).collect();
                let strings: Vec<String> = bytes.split(|&b| b == 0).filter(|s| !s.is_empty()).map(|s| String::from_utf8_lossy(s).to_string()).collect();
                ArrayData::String(strings)
            } else {
                let vals: Vec<f64> = node
                    .text
                    .split_whitespace()
                    .map(|t| t.parse::<f64>().or_else(|_| t.to_ascii_lowercase().parse::<f64>()))
                    .collect::<std::result::Result<_, _>>()
                    .with_context(|| format!("parsing ASCII data of array {name:?}"))?;
                ArrayData::from_f64(ty, &vals)
            }
        }
        "binary" => {
            let raw = decode_base64_payload(ctx, node.text.as_bytes())?;
            decode_typed(ctx, ty, &raw)?
        }
        "appended" => {
            let offset: usize = node.attr("offset").ok_or_else(|| anyhow!("appended array {name:?} lacks offset"))?.parse()?;
            let blob = ctx.appended.ok_or_else(|| anyhow!("array {name:?} is appended but file has no AppendedData"))?;
            if offset >= blob.len() {
                bail!("appended offset {offset} beyond data ({} bytes)", blob.len());
            }
            let raw = if ctx.appended_base64 { decode_base64_payload(ctx, &blob[offset..])? } else { decode_binary_payload(ctx, &blob[offset..])? };
            decode_typed(ctx, ty, &raw)?
        }
        other => bail!("unknown DataArray format {other:?}"),
    };
    let mut arr = Array::new(name, ncomp, data);
    let comp_names: Vec<String> = (0..ncomp).filter_map(|i| node.attr(&format!("ComponentName{i}")).map(|s| s.to_string())).collect();
    if comp_names.len() == ncomp && ncomp > 1 {
        arr.component_names = Some(comp_names);
    }
    Ok(arr)
}

fn decode_typed(ctx: &FileCtx, ty: ScalarType, raw: &[u8]) -> Result<ArrayData> {
    if ty == ScalarType::String {
        let strings: Vec<String> = raw.split(|&b| b == 0).filter(|s| !s.is_empty()).map(|s| String::from_utf8_lossy(s).to_string()).collect();
        return Ok(ArrayData::String(strings));
    }
    ArrayData::from_bytes(ty, raw, ctx.little_endian)
}

fn parse_type(s: &str) -> Result<ScalarType> {
    ScalarType::from_vtk_name(s).ok_or_else(|| anyhow!("unknown VTK XML type {s:?}"))
}

fn read_attributes(ctx: &FileCtx, node: Option<&Node>) -> Result<Attributes> {
    let mut attrs = Attributes::new();
    let Some(node) = node else { return Ok(attrs) };
    for da in node.children.iter().filter(|c| c.name == "DataArray" || c.name == "Array") {
        let arr = read_array(ctx, da)?;
        attrs.insert(arr);
    }
    for (attr, kind) in [
        ("Scalars", AttributeKind::Scalars),
        ("Vectors", AttributeKind::Vectors),
        ("Normals", AttributeKind::Normals),
        ("TCoords", AttributeKind::TCoords),
        ("Tensors", AttributeKind::Tensors),
        ("GlobalIds", AttributeKind::GlobalIds),
        ("PedigreeIds", AttributeKind::PedigreeIds),
    ] {
        if let Some(n) = node.attr(attr) {
            attrs.set_active(kind, n);
        }
    }
    // Higher-order cells find their degrees and rational weights by these names.
    for tag in ["HigherOrderDegrees", "RationalWeights"] {
        if let Some(n) = node.attr(tag).filter(|n| *n != tag && attrs.get(tag).is_none()) {
            if let Some(mut a) = attrs.remove(n) {
                a.name = tag.to_string();
                attrs.insert(a);
            }
        }
    }
    Ok(attrs)
}

fn read_points(ctx: &FileCtx, piece: &Node) -> Result<Points> {
    let node = piece.child("Points").and_then(|p| p.child("DataArray")).ok_or_else(|| anyhow!("Piece has no Points"))?;
    let arr = read_array(ctx, node)?;
    if arr.ncomp != 3 {
        bail!("Points array must have 3 components, has {}", arr.ncomp);
    }
    Ok(Points::from_array(arr.data))
}

fn read_cells_block(ctx: &FileCtx, node: Option<&Node>, poly_kind: Option<&str>) -> Result<Cells> {
    let Some(node) = node else { return Ok(Cells::new()) };
    let find = |n: &str| node.children.iter().find(|c| c.name == "DataArray" && c.attr("Name") == Some(n));
    let conn = find("connectivity").map(|n| read_array(ctx, n)).transpose()?;
    let offs = find("offsets").map(|n| read_array(ctx, n)).transpose()?;
    let (Some(conn), Some(offs)) = (conn, offs) else { return Ok(Cells::new()) };
    let n_cells = offs.len();
    let mut cells = Cells::with_capacity(n_cells, conn.len());
    cells.connectivity = (0..conn.len()).map(|i| conn.get(i, 0) as u32).collect();
    cells.offsets.extend((0..n_cells).map(|i| offs.get(i, 0) as u32));
    if let Some(types) = find("types") {
        let t = read_array(ctx, types)?;
        cells.types = (0..t.len()).map(|i| CellType::from_vtk(t.get(i, 0) as u8).unwrap_or(CellType::Empty)).collect();
    } else {
        let kind = poly_kind.unwrap_or("Polys");
        cells.types = (0..n_cells)
            .map(|i| {
                let n = cells.cell(i).len();
                match kind {
                    "Verts" => {
                        if n == 1 {
                            CellType::Vertex
                        } else {
                            CellType::PolyVertex
                        }
                    }
                    "Lines" => {
                        if n == 2 {
                            CellType::Line
                        } else {
                            CellType::PolyLine
                        }
                    }
                    "Strips" => CellType::TriangleStrip,
                    _ => match n {
                        3 => CellType::Triangle,
                        4 => CellType::Quad,
                        _ => CellType::Polygon,
                    },
                }
            })
            .collect();
    }
    if let Some(faces) = find("faces") {
        if let Some(faceoffs) = find("faceoffsets") {
            let f = read_array(ctx, faces)?;
            let fo = read_array(ctx, faceoffs)?;
            let mut all = Vec::with_capacity(n_cells);
            for i in 0..n_cells {
                let start = fo.get(i, 0) as i64;
                if start < 0 {
                    all.push(Vec::new());
                    continue;
                }
                let mut p = start as usize;
                let nf = f.get(p, 0) as usize;
                p += 1;
                let mut cell_faces = Vec::with_capacity(nf);
                for _ in 0..nf {
                    let np = f.get(p, 0) as usize;
                    p += 1;
                    cell_faces.push((0..np).map(|k| f.get(p + k, 0) as u32).collect());
                    p += np;
                }
                all.push(cell_faces);
            }
            cells.polyhedron_faces = Some(all);
        }
    }
    Ok(cells)
}

fn parse_extent(s: &str) -> Result<[i64; 6]> {
    let v: Vec<i64> = s.split_whitespace().map(|t| t.parse()).collect::<std::result::Result<_, _>>()?;
    if v.len() != 6 {
        bail!("bad extent {s:?}");
    }
    Ok([v[0], v[1], v[2], v[3], v[4], v[5]])
}

fn parse_f64s(s: &str) -> Vec<f64> {
    s.split_whitespace().filter_map(|t| t.parse().ok()).collect()
}

/// Read a VTK XML file into a dataset.
pub fn read(path: &Path) -> Result<DataSet> {
    let bytes = crate::read_bytes(path)?;
    read_bytes(&bytes, path).with_context(|| format!("reading VTK XML file {}", path.display()))
}

pub fn read_bytes(bytes: &[u8], path: &Path) -> Result<DataSet> {
    let (xml, appended) = split_appended(bytes);
    let root = parse_tree(&xml)?;
    let vtkfile = root.child("VTKFile").ok_or_else(|| anyhow!("missing <VTKFile> root"))?;
    let ty = vtkfile.attr("type").unwrap_or("");
    let ctx = FileCtx {
        little_endian: vtkfile.attr("byte_order").map(|b| b != "BigEndian").unwrap_or(true),
        header64: vtkfile.attr("header_type") == Some("UInt64"),
        compressor: match vtkfile.attr("compressor") {
            None | Some("") => Compressor::None,
            Some("vtkZLibDataCompressor") => Compressor::Zlib,
            Some("vtkLZ4DataCompressor") => Compressor::Lz4,
            Some("vtkLZMADataCompressor") => Compressor::Lzma,
            Some(other) => bail!("unknown compressor {other}"),
        },
        appended: appended.map(|(b, _)| b),
        appended_base64: appended.map(|(_, b)| b).unwrap_or(false),
        old_hex_order: {
            let v = vtkfile.attr("version").unwrap_or("0.1");
            let (a, b) = v.split_once('.').unwrap_or((v, "0"));
            (a.parse::<u32>().unwrap_or(0), b.parse::<u32>().unwrap_or(0)) < (2, 2)
        },
    };
    let body = vtkfile.child(ty).ok_or_else(|| anyhow!("missing <{ty}> element"))?;
    let mut ds = match ty {
        "ImageData" => read_image(&ctx, body)?,
        "RectilinearGrid" => read_rectilinear(&ctx, body)?,
        "StructuredGrid" => read_structured(&ctx, body)?,
        "PolyData" => read_polydata(&ctx, body)?,
        "UnstructuredGrid" => read_unstructured(&ctx, body)?,
        "Table" => read_table(&ctx, body)?,
        "vtkMultiBlockDataSet" => read_multiblock(path, body)?,
        "PImageData" | "PRectilinearGrid" | "PStructuredGrid" | "PPolyData" | "PUnstructuredGrid" => read_parallel(path, body)?,
        other => bail!("unsupported VTK XML type {other}"),
    };
    // FieldData lives directly under the dataset element.
    let fd = read_attributes(&ctx, body.child("FieldData"))?;
    if !fd.is_empty() {
        match &mut ds {
            DataSet::ImageData(d) => d.field_data = fd,
            DataSet::RectilinearGrid(d) => d.field_data = fd,
            DataSet::StructuredGrid(d) => d.field_data = fd,
            DataSet::PolyData(d) => d.field_data = fd,
            DataSet::UnstructuredGrid(d) => d.field_data = fd,
            DataSet::Table(d) => d.field_data = fd,
            DataSet::MultiBlock(d) => d.field_data = fd,
        }
    }
    Ok(ds)
}

fn read_image(ctx: &FileCtx, body: &Node) -> Result<DataSet> {
    let mut img = ImageData::default();
    img.extent = parse_extent(body.attr("WholeExtent").ok_or_else(|| anyhow!("ImageData lacks WholeExtent"))?)?;
    if let Some(o) = body.attr("Origin") {
        let v = parse_f64s(o);
        if v.len() == 3 {
            img.origin = [v[0], v[1], v[2]];
        }
    }
    if let Some(s) = body.attr("Spacing") {
        let v = parse_f64s(s);
        if v.len() == 3 {
            img.spacing = [v[0], v[1], v[2]];
        }
    }
    if let Some(d) = body.attr("Direction") {
        let v = parse_f64s(d);
        if v.len() == 9 {
            img.direction.copy_from_slice(&v);
        }
    }
    let pieces: Vec<&Node> = body.children_named("Piece").collect();
    if pieces.len() == 1 {
        let p = pieces[0];
        if let Some(e) = p.attr("Extent") {
            let pe = parse_extent(e)?;
            if pe != img.extent {
                tracing::warn!("ImageData piece extent {pe:?} differs from whole extent {:?}; using piece", img.extent);
                img.extent = pe;
            }
        }
        img.point_data = read_attributes(ctx, p.child("PointData"))?;
        img.cell_data = read_attributes(ctx, p.child("CellData"))?;
    } else if pieces.len() > 1 {
        bail!("multi-piece ImageData files are not supported yet");
    }
    Ok(DataSet::ImageData(img))
}

fn read_rectilinear(ctx: &FileCtx, body: &Node) -> Result<DataSet> {
    let mut g = RectilinearGrid::default();
    g.extent = parse_extent(body.attr("WholeExtent").ok_or_else(|| anyhow!("missing WholeExtent"))?)?;
    let p = body.child("Piece").ok_or_else(|| anyhow!("no Piece"))?;
    if let Some(e) = p.attr("Extent") {
        g.extent = parse_extent(e)?;
    }
    let coords = p.child("Coordinates").ok_or_else(|| anyhow!("no Coordinates"))?;
    let arrs: Vec<Array> = coords.children_named("DataArray").map(|n| read_array(ctx, n)).collect::<Result<_>>()?;
    if arrs.len() != 3 {
        bail!("Coordinates must have 3 arrays");
    }
    g.x = arrs[0].data.to_f64_vec();
    g.y = arrs[1].data.to_f64_vec();
    g.z = arrs[2].data.to_f64_vec();
    g.point_data = read_attributes(ctx, p.child("PointData"))?;
    g.cell_data = read_attributes(ctx, p.child("CellData"))?;
    Ok(DataSet::RectilinearGrid(g))
}

fn read_structured(ctx: &FileCtx, body: &Node) -> Result<DataSet> {
    let mut g = StructuredGrid::default();
    g.extent = parse_extent(body.attr("WholeExtent").ok_or_else(|| anyhow!("missing WholeExtent"))?)?;
    let p = body.child("Piece").ok_or_else(|| anyhow!("no Piece"))?;
    if let Some(e) = p.attr("Extent") {
        g.extent = parse_extent(e)?;
    }
    g.points = read_points(ctx, p)?;
    g.point_data = read_attributes(ctx, p.child("PointData"))?;
    g.cell_data = read_attributes(ctx, p.child("CellData"))?;
    Ok(DataSet::StructuredGrid(g))
}

fn read_polydata(ctx: &FileCtx, body: &Node) -> Result<DataSet> {
    let mut out = PolyData::default();
    let mut first = true;
    for p in body.children_named("Piece") {
        let mut pd = PolyData::default();
        pd.points = if p.child("Points").is_some() { read_points(ctx, p)? } else { Points::empty() };
        pd.verts = read_cells_block(ctx, p.child("Verts"), Some("Verts"))?;
        pd.lines = read_cells_block(ctx, p.child("Lines"), Some("Lines"))?;
        pd.polys = read_cells_block(ctx, p.child("Polys"), Some("Polys"))?;
        pd.strips = read_cells_block(ctx, p.child("Strips"), Some("Strips"))?;
        pd.point_data = read_attributes(ctx, p.child("PointData"))?;
        pd.cell_data = read_attributes(ctx, p.child("CellData"))?;
        if first {
            out = pd;
            first = false;
        } else {
            append_polydata(&mut out, &pd)?;
        }
    }
    Ok(DataSet::PolyData(out))
}

/// Files before version 2.2 number the vertical edges of Lagrange and Bezier hexahedra
/// (0,0), (1,0), (0,1), (1,1); now they go round: (0,0), (1,0), (1,1), (0,1).
fn upgrade_hex_order(ug: &mut UnstructuredGrid) {
    use aurafx_data::higher_order::{cell_order, Shape};
    let degrees = ug.cell_data.get("HigherOrderDegrees").filter(|a| a.ncomp == 3).cloned();
    for c in 0..ug.cells.len() {
        if !matches!(ug.cells.types[c], CellType::LagrangeHexahedron | CellType::BezierHexahedron) {
            continue;
        }
        let (a, b) = (ug.cells.offsets[c] as usize, ug.cells.offsets[c + 1] as usize);
        let deg = degrees.as_ref().filter(|d| c < d.len()).map(|d| [d.get(c, 0), d.get(c, 1), d.get(c, 2)]);
        let Some([p, q, r]) = cell_order(Shape::Hexahedron, b - a, deg) else { continue };
        let (n, start) = (r - 1, 8 + 4 * (p - 1) + 4 * (q - 1));
        let ids = &mut ug.cells.connectivity[a..b];
        for k in 0..n {
            ids.swap(start + 2 * n + k, start + 3 * n + k);
        }
    }
}

fn read_unstructured(ctx: &FileCtx, body: &Node) -> Result<DataSet> {
    let mut out = UnstructuredGrid::default();
    let mut first = true;
    for p in body.children_named("Piece") {
        let mut ug = UnstructuredGrid::default();
        ug.points = if p.child("Points").is_some() { read_points(ctx, p)? } else { Points::empty() };
        ug.cells = read_cells_block(ctx, p.child("Cells"), None)?;
        ug.point_data = read_attributes(ctx, p.child("PointData"))?;
        ug.cell_data = read_attributes(ctx, p.child("CellData"))?;
        if ctx.old_hex_order {
            upgrade_hex_order(&mut ug);
        }
        if first {
            out = ug;
            first = false;
        } else {
            append_unstructured(&mut out, &ug)?;
        }
    }
    Ok(DataSet::UnstructuredGrid(out))
}

fn read_table(ctx: &FileCtx, body: &Node) -> Result<DataSet> {
    let mut t = Table::default();
    for p in body.children_named("Piece") {
        let cols = read_attributes(ctx, p.child("RowData"))?;
        if t.columns.is_empty() {
            t.columns = cols;
        } else {
            for a in cols.iter() {
                if let Some(existing) = t.columns.get_mut(&a.name) {
                    existing.data.append(&a.data)?;
                }
            }
        }
    }
    Ok(DataSet::Table(t))
}

fn read_multiblock(path: &Path, body: &Node) -> Result<DataSet> {
    fn walk(path: &Path, node: &Node) -> Result<MultiBlock> {
        let mut mb = MultiBlock::default();
        for (i, c) in node.children.iter().enumerate() {
            let name = c.attr("name").map(|s| s.to_string()).unwrap_or_else(|| format!("{} {}", c.name, c.attr("index").unwrap_or(&i.to_string())));
            match c.name.as_str() {
                "Block" | "Piece" | "Partitions" => {
                    let inner = walk(path, c)?;
                    mb.blocks.push((name, Some(DataSet::MultiBlock(inner))));
                }
                "DataSet" => match c.attr("file") {
                    Some(f) => {
                        let sub = crate::sibling(path, f);
                        let ds = match crate::read_file(&sub)? {
                            crate::Loaded::Static(d) => Arc::try_unwrap(d).unwrap_or_else(|a| (*a).clone()),
                            crate::Loaded::Series(s) => {
                                let first = s.steps.first().ok_or_else(|| anyhow!("empty series"))?;
                                let d = crate::load_step(first)?;
                                (*d).clone()
                            }
                        };
                        mb.blocks.push((name, Some(ds)));
                    }
                    None => mb.blocks.push((name, None)),
                },
                _ => {}
            }
        }
        Ok(mb)
    }
    Ok(DataSet::MultiBlock(walk(path, body)?))
}

fn read_parallel(path: &Path, body: &Node) -> Result<DataSet> {
    let mut sources: Vec<String> = body.children_named("Piece").filter_map(|p| p.attr("Source").map(|s| s.to_string())).collect();
    if sources.is_empty() {
        bail!("parallel file has no pieces");
    }
    // A parallel server process reads only its share of the piece files.
    if let Some((k, n)) = crate::current_piece() {
        if sources.len() >= n {
            let (lo, hi) = (k * sources.len() / n, (k + 1) * sources.len() / n);
            sources = sources[lo..hi].to_vec();
            crate::mark_pieced();
            if sources.is_empty() {
                return Ok(DataSet::UnstructuredGrid(Default::default()));
            }
        }
    }
    use rayon::prelude::*;
    let pieces: Vec<DataSet> = sources
        .par_iter()
        .map(|s| {
            let sub = crate::sibling(path, s);
            read(&sub)
        })
        .collect::<Result<_>>()?;
    merge_pieces(pieces)
}

/// Append pieces of the same type into a single dataset (unstructured/poly only; others become multiblock).
pub fn merge_pieces(pieces: Vec<DataSet>) -> Result<DataSet> {
    let mut it = pieces.into_iter();
    let mut first = it.next().ok_or_else(|| anyhow!("no pieces"))?;
    let mut rest: Vec<DataSet> = it.collect();
    match &mut first {
        DataSet::UnstructuredGrid(ug) => {
            for p in &rest {
                if let DataSet::UnstructuredGrid(o) = p {
                    append_unstructured(ug, o)?;
                } else {
                    bail!("mixed piece types");
                }
            }
            Ok(first)
        }
        DataSet::PolyData(pd) => {
            for p in &rest {
                if let DataSet::PolyData(o) = p {
                    append_polydata(pd, o)?;
                } else {
                    bail!("mixed piece types");
                }
            }
            Ok(first)
        }
        _ => {
            if rest.is_empty() {
                return Ok(first);
            }
            let mut mb = MultiBlock::default();
            mb.blocks.push(("piece 0".into(), Some(first)));
            for (i, p) in rest.drain(..).enumerate() {
                mb.blocks.push((format!("piece {}", i + 1), Some(p)));
            }
            Ok(DataSet::MultiBlock(mb))
        }
    }
}

fn append_attrs(a: &mut Attributes, b: &Attributes) -> Result<()> {
    let names: Vec<String> = a.arrays.keys().cloned().collect();
    for name in names {
        match b.get(&name) {
            Some(src) => {
                let dst = a.get_mut(&name).unwrap();
                dst.data.append(&src.data)?;
            }
            None => {
                a.remove(&name);
            }
        }
    }
    Ok(())
}

pub fn append_unstructured(a: &mut UnstructuredGrid, b: &UnstructuredGrid) -> Result<()> {
    let off = a.points.len() as u32;
    a.points.append(&b.points);
    a.cells.append(&b.cells, off);
    append_attrs(&mut a.point_data, &b.point_data)?;
    append_attrs(&mut a.cell_data, &b.cell_data)?;
    Ok(())
}

pub fn append_polydata(a: &mut PolyData, b: &PolyData) -> Result<()> {
    let off = a.points.len() as u32;
    a.points.append(&b.points);
    // Cell data ordering (verts, lines, polys, strips) must be re-interleaved.
    let a_counts = [a.verts.len(), a.lines.len(), a.polys.len(), a.strips.len()];
    let b_counts = [b.verts.len(), b.lines.len(), b.polys.len(), b.strips.len()];
    let mut order = Vec::with_capacity(a_counts.iter().sum::<usize>() + b_counts.iter().sum::<usize>());
    let (mut ai, mut bi) = (0usize, a_counts.iter().sum::<usize>());
    let mut new_cd = Attributes::new();
    let has_cd = !a.cell_data.is_empty() && !b.cell_data.is_empty();
    if has_cd {
        // Build a merged attribute set first (concatenated), then reorder.
        let mut merged = a.cell_data.clone();
        append_attrs(&mut merged, &b.cell_data)?;
        for k in 0..4 {
            order.extend(ai..ai + a_counts[k]);
            ai += a_counts[k];
            order.extend(bi..bi + b_counts[k]);
            bi += b_counts[k];
        }
        new_cd = merged.gather(&order);
    }
    a.verts.append(&b.verts, off);
    a.lines.append(&b.lines, off);
    a.polys.append(&b.polys, off);
    a.strips.append(&b.strips, off);
    append_attrs(&mut a.point_data, &b.point_data)?;
    if has_cd {
        a.cell_data = new_cd;
    } else {
        a.cell_data = Attributes::new();
    }
    Ok(())
}

/// Read a `.pvd` collection into a time series. Multi-part steps become multi-block files on load.
pub fn read_pvd(path: &Path) -> Result<TimeSeries> {
    let bytes = crate::read_bytes(path)?;
    let root = parse_tree(&bytes)?;
    let coll = root.child("VTKFile").and_then(|v| v.child("Collection")).ok_or_else(|| anyhow!("not a PVD collection"))?;
    let mut by_time: indexmap::IndexMap<String, (f64, Vec<std::path::PathBuf>)> = indexmap::IndexMap::new();
    for ds in coll.children_named("DataSet") {
        let t = ds.attr("timestep").unwrap_or("0");
        let tv: f64 = t.parse().unwrap_or(0.0);
        let f = ds.attr("file").ok_or_else(|| anyhow!("DataSet without file"))?;
        by_time.entry(t.to_string()).or_insert((tv, Vec::new())).1.push(crate::sibling(path, f));
    }
    let mut entries: Vec<(f64, Vec<std::path::PathBuf>)> = by_time.into_values().collect();
    entries.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());
    let mut series = TimeSeries::default();
    for (t, files) in entries {
        series.times.push(t);
        if files.len() == 1 {
            series.steps.push(TimeStepRef::File(files.into_iter().next().unwrap()));
        } else {
            // Multiple parts for one time: load lazily as a group by writing a tiny in-memory multiblock on demand.
            series.steps.push(TimeStepRef::File(files[0].clone()));
            tracing::warn!("PVD step {t} has {} parts; only the first is loaded for now", files.len());
        }
    }
    Ok(series)
}
