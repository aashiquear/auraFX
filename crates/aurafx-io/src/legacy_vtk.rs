//! Legacy VTK (`.vtk`) reader: ASCII and binary, all five dataset types, VTK 5.1 cell layout.

use anyhow::{anyhow, bail, Context, Result};
use aurafx_data::*;
use std::path::Path;

pub fn read(path: &Path) -> Result<DataSet> {
    let bytes = crate::read_bytes(path)?;
    parse(&bytes).with_context(|| format!("parsing legacy VTK file {}", path.display()))
}

struct Cursor<'a> {
    buf: &'a [u8],
    pos: usize,
    binary: bool,
}

impl<'a> Cursor<'a> {
    fn eof(&self) -> bool {
        self.pos >= self.buf.len()
    }

    /// Next non-empty line, trimmed; `None` at EOF.
    fn line(&mut self) -> Option<String> {
        loop {
            if self.eof() {
                return None;
            }
            let rest = &self.buf[self.pos..];
            let end = memchr::memchr(b'\n', rest).map(|i| i + 1).unwrap_or(rest.len());
            let raw = &rest[..end];
            self.pos += end;
            let s = String::from_utf8_lossy(raw).trim().to_string();
            if !s.is_empty() {
                return Some(s);
            }
        }
    }

    fn peek_line(&mut self) -> Option<String> {
        let save = self.pos;
        let l = self.line();
        self.pos = save;
        l
    }

    fn skip_ws(&mut self) {
        while !self.eof() && self.buf[self.pos].is_ascii_whitespace() {
            self.pos += 1;
        }
    }

    /// Read `n` numeric values of `ty` into an ArrayData.
    fn values(&mut self, ty: ScalarType, n: usize) -> Result<ArrayData> {
        if self.binary {
            // Binary payload begins after the single newline that ended the header line.
            let sz = if ty == ScalarType::U8 && false { 1 } else { ty.size_of() };
            let nbytes = n * sz;
            if self.pos + nbytes > self.buf.len() {
                bail!("unexpected end of binary data (need {nbytes} bytes at {})", self.pos);
            }
            let slice = &self.buf[self.pos..self.pos + nbytes];
            self.pos += nbytes;
            ArrayData::from_bytes(ty, slice, false)
        } else {
            let mut out = Vec::with_capacity(n);
            while out.len() < n {
                self.skip_ws();
                if self.eof() {
                    bail!("unexpected end of ASCII data (got {} of {n} values)", out.len());
                }
                let start = self.pos;
                while !self.eof() && !self.buf[self.pos].is_ascii_whitespace() {
                    self.pos += 1;
                }
                let tok = std::str::from_utf8(&self.buf[start..self.pos])?;
                let v: f64 = tok.parse().or_else(|_| {
                    // "nan", "inf", or integers like "1e3"
                    tok.to_ascii_lowercase().parse::<f64>()
                })?;
                out.push(v);
            }
            Ok(ArrayData::from_f64(ty, &out))
        }
    }

    /// Bit arrays: ASCII 0/1 or packed bits in binary.
    fn bits(&mut self, n: usize) -> Result<ArrayData> {
        if self.binary {
            let nbytes = n.div_ceil(8);
            let slice = &self.buf[self.pos..self.pos + nbytes];
            self.pos += nbytes;
            Ok(ArrayData::U8((0..n).map(|i| (slice[i / 8] >> (7 - i % 8)) & 1).collect()))
        } else {
            self.values(ScalarType::U8, n)
        }
    }
}

fn parse_type(s: &str) -> Result<ScalarType> {
    ScalarType::from_vtk_name(s).ok_or_else(|| anyhow!("unknown VTK data type '{s}'"))
}

fn tokens(line: &str) -> Vec<String> {
    line.split_whitespace().map(|s| s.to_string()).collect()
}

fn parse(buf: &[u8]) -> Result<DataSet> {
    let mut c = Cursor { buf, pos: 0, binary: false };
    let header = c.line().ok_or_else(|| anyhow!("empty file"))?;
    if !header.starts_with("# vtk DataFile") {
        bail!("not a legacy VTK file (header: {header:?})");
    }
    // Title line may be empty, so read raw.
    {
        let rest = &c.buf[c.pos..];
        let end = memchr::memchr(b'\n', rest).map(|i| i + 1).unwrap_or(rest.len());
        c.pos += end;
    }
    let fmt = c.line().ok_or_else(|| anyhow!("missing ASCII/BINARY line"))?;
    c.binary = fmt.eq_ignore_ascii_case("BINARY");
    let ds_line = c.line().ok_or_else(|| anyhow!("missing DATASET line"))?;
    let t = tokens(&ds_line);
    if t.len() < 2 || !t[0].eq_ignore_ascii_case("DATASET") {
        bail!("expected DATASET line, got {ds_line:?}");
    }

    let mut field_data = Attributes::new();
    let mut ds = match t[1].to_ascii_uppercase().as_str() {
        "STRUCTURED_POINTS" => read_structured_points(&mut c)?,
        "STRUCTURED_GRID" => read_structured_grid(&mut c)?,
        "RECTILINEAR_GRID" => read_rectilinear(&mut c)?,
        "POLYDATA" => read_polydata(&mut c)?,
        "UNSTRUCTURED_GRID" => read_unstructured(&mut c)?,
        other => bail!("unsupported legacy dataset type {other}"),
    };

    // Attribute sections.
    while let Some(line) = c.line() {
        let t = tokens(&line);
        match t[0].to_ascii_uppercase().as_str() {
            "POINT_DATA" => {
                let n: usize = t.get(1).ok_or_else(|| anyhow!("POINT_DATA needs a count"))?.parse()?;
                let attrs = read_attributes(&mut c, n)?;
                if let Some(pd) = ds.point_data_mut() {
                    *pd = attrs;
                }
            }
            "CELL_DATA" => {
                let n: usize = t.get(1).ok_or_else(|| anyhow!("CELL_DATA needs a count"))?.parse()?;
                let attrs = read_attributes(&mut c, n)?;
                if let Some(cd) = ds.cell_data_mut() {
                    *cd = attrs;
                }
            }
            "FIELD" => {
                let n: usize = t.get(2).ok_or_else(|| anyhow!("FIELD needs a count"))?.parse()?;
                read_field(&mut c, n, &mut field_data)?;
            }
            "METADATA" => skip_metadata(&mut c),
            _ => tracing::warn!("ignoring unexpected legacy VTK section {line:?}"),
        }
    }
    if !field_data.is_empty() {
        match &mut ds {
            DataSet::ImageData(d) => d.field_data = field_data,
            DataSet::RectilinearGrid(d) => d.field_data = field_data,
            DataSet::StructuredGrid(d) => d.field_data = field_data,
            DataSet::PolyData(d) => d.field_data = field_data,
            DataSet::UnstructuredGrid(d) => d.field_data = field_data,
            _ => {}
        }
    }
    Ok(ds)
}

fn skip_metadata(c: &mut Cursor) {
    // METADATA\nINFORMATION n\n(NAME ... LOCATION ...\nDATA ...)*\n<blank>
    while let Some(l) = c.peek_line() {
        let u = l.to_ascii_uppercase();
        if u.starts_with("INFORMATION") || u.starts_with("NAME ") || u.starts_with("DATA ") || u.starts_with("DATA\t") {
            c.line();
        } else {
            break;
        }
    }
}

fn expect(c: &mut Cursor, kw: &str) -> Result<Vec<String>> {
    loop {
        let line = c.line().ok_or_else(|| anyhow!("expected {kw}, found EOF"))?;
        let t = tokens(&line);
        if t[0].eq_ignore_ascii_case("METADATA") {
            skip_metadata(c);
            continue;
        }
        if !t[0].eq_ignore_ascii_case(kw) {
            bail!("expected {kw}, found {line:?}");
        }
        return Ok(t);
    }
}

fn dims_of(t: &[String]) -> Result<[usize; 3]> {
    Ok([t[1].parse()?, t[2].parse()?, t[3].parse()?])
}

fn extent(d: [usize; 3]) -> [i64; 6] {
    [0, d[0] as i64 - 1, 0, d[1] as i64 - 1, 0, d[2] as i64 - 1]
}

fn read_points(c: &mut Cursor, t: &[String]) -> Result<Points> {
    let n: usize = t[1].parse()?;
    let ty = parse_type(&t[2])?;
    Ok(Points::from_array(c.values(ty, 3 * n)?))
}

fn read_structured_points(c: &mut Cursor) -> Result<DataSet> {
    let mut img = ImageData::default();
    let mut dims = [1usize; 3];
    while let Some(l) = c.peek_line() {
        let t = tokens(&l);
        match t[0].to_ascii_uppercase().as_str() {
            "DIMENSIONS" => {
                c.line();
                dims = dims_of(&t)?;
            }
            "ORIGIN" => {
                c.line();
                img.origin = [t[1].parse()?, t[2].parse()?, t[3].parse()?];
            }
            "SPACING" | "ASPECT_RATIO" => {
                c.line();
                img.spacing = [t[1].parse()?, t[2].parse()?, t[3].parse()?];
            }
            "METADATA" => {
                c.line();
                skip_metadata(c);
            }
            _ => break,
        }
    }
    img.extent = extent(dims);
    Ok(DataSet::ImageData(img))
}

fn read_structured_grid(c: &mut Cursor) -> Result<DataSet> {
    let t = expect(c, "DIMENSIONS")?;
    let dims = dims_of(&t)?;
    let t = expect(c, "POINTS")?;
    let points = read_points(c, &t)?;
    Ok(DataSet::StructuredGrid(StructuredGrid { extent: extent(dims), points, ..Default::default() }))
}

fn read_rectilinear(c: &mut Cursor) -> Result<DataSet> {
    let t = expect(c, "DIMENSIONS")?;
    let dims = dims_of(&t)?;
    let mut g = RectilinearGrid { extent: extent(dims), ..Default::default() };
    for (kw, slot) in [("X_COORDINATES", 0), ("Y_COORDINATES", 1), ("Z_COORDINATES", 2)] {
        let t = expect(c, kw)?;
        let n: usize = t[1].parse()?;
        let ty = parse_type(&t[2])?;
        let v = c.values(ty, n)?.to_f64_vec();
        match slot {
            0 => g.x = v,
            1 => g.y = v,
            _ => g.z = v,
        }
    }
    Ok(DataSet::RectilinearGrid(g))
}

/// Read a cell block in either legacy (`n size` + packed lists) or 5.1 (`OFFSETS`/`CONNECTIVITY`) form.
fn read_cell_block(c: &mut Cursor, t: &[String], default_type: CellType) -> Result<Cells> {
    let a: usize = t[1].parse()?;
    let b: usize = t[2].parse()?;
    let next = c.peek_line().unwrap_or_default();
    if next.to_ascii_uppercase().starts_with("OFFSETS") {
        let ot = tokens(&c.line().unwrap());
        let oty = parse_type(&ot[1])?;
        let offsets = c.values(oty, a)?.to_f64_vec();
        let ct = expect(c, "CONNECTIVITY")?;
        let cty = parse_type(&ct[1])?;
        let conn = c.values(cty, b)?.to_f64_vec();
        let n_cells = a.saturating_sub(1);
        let mut cells = Cells::with_capacity(n_cells, b);
        cells.connectivity = conn.iter().map(|&v| v as u32).collect();
        cells.offsets = offsets.iter().map(|&v| v as u32).collect();
        cells.types = vec![default_type; n_cells];
        Ok(cells)
    } else {
        let raw = c.values(ScalarType::I32, b)?.to_f64_vec();
        let mut cells = Cells::with_capacity(a, b);
        let mut i = 0;
        while i < raw.len() {
            let n = raw[i] as usize;
            let pts: Vec<u32> = raw[i + 1..i + 1 + n].iter().map(|&v| v as u32).collect();
            cells.push(default_type, &pts);
            i += 1 + n;
        }
        Ok(cells)
    }
}

fn fix_poly_types(cells: &mut Cells, kind: &str) {
    for i in 0..cells.len() {
        let n = cells.cell(i).len();
        cells.types[i] = match kind {
            "VERTICES" => if n == 1 { CellType::Vertex } else { CellType::PolyVertex },
            "LINES" => if n == 2 { CellType::Line } else { CellType::PolyLine },
            "POLYGONS" => match n { 3 => CellType::Triangle, 4 => CellType::Quad, _ => CellType::Polygon },
            _ => CellType::TriangleStrip,
        };
    }
}

fn read_polydata(c: &mut Cursor) -> Result<DataSet> {
    let mut pd = PolyData::default();
    while let Some(l) = c.peek_line() {
        let t = tokens(&l);
        let kw = t[0].to_ascii_uppercase();
        match kw.as_str() {
            "POINTS" => {
                c.line();
                pd.points = read_points(c, &t)?;
            }
            "VERTICES" | "LINES" | "POLYGONS" | "TRIANGLE_STRIPS" => {
                c.line();
                let mut cells = read_cell_block(c, &t, CellType::Polygon)?;
                fix_poly_types(&mut cells, &kw);
                match kw.as_str() {
                    "VERTICES" => pd.verts = cells,
                    "LINES" => pd.lines = cells,
                    "POLYGONS" => pd.polys = cells,
                    _ => pd.strips = cells,
                }
            }
            "METADATA" => {
                c.line();
                skip_metadata(c);
            }
            _ => break,
        }
    }
    Ok(DataSet::PolyData(pd))
}

fn read_unstructured(c: &mut Cursor) -> Result<DataSet> {
    let mut ug = UnstructuredGrid::default();
    while let Some(l) = c.peek_line() {
        let t = tokens(&l);
        match t[0].to_ascii_uppercase().as_str() {
            "POINTS" => {
                c.line();
                ug.points = read_points(c, &t)?;
            }
            "CELLS" => {
                c.line();
                ug.cells = read_cell_block(c, &t, CellType::Empty)?;
            }
            "CELL_TYPES" => {
                c.line();
                let n: usize = t[1].parse()?;
                let types = c.values(ScalarType::I32, n)?.to_f64_vec();
                if ug.cells.len() != n {
                    bail!("CELL_TYPES count {n} does not match {} cells", ug.cells.len());
                }
                for (i, ty) in types.iter().enumerate() {
                    ug.cells.types[i] = CellType::from_vtk(*ty as u8).ok_or_else(|| anyhow!("unknown cell type {ty}"))?;
                }
            }
            "METADATA" => {
                c.line();
                skip_metadata(c);
            }
            _ => break,
        }
    }
    Ok(DataSet::UnstructuredGrid(ug))
}

fn read_field(c: &mut Cursor, n_arrays: usize, attrs: &mut Attributes) -> Result<()> {
    for _ in 0..n_arrays {
        let line = c.line().ok_or_else(|| anyhow!("EOF inside FIELD"))?;
        let t = tokens(&line);
        if t.len() < 4 {
            bail!("bad FIELD array header {line:?}");
        }
        let name = t[0].clone();
        let ncomp: usize = t[1].parse()?;
        let ntuples: usize = t[2].parse()?;
        let ty = parse_type(&t[3])?;
        let data = c.values(ty, ncomp * ntuples)?;
        attrs.insert(Array::new(name, ncomp, data));
        if let Some(l) = c.peek_line() {
            if l.to_ascii_uppercase().starts_with("METADATA") {
                c.line();
                skip_metadata(c);
            }
        }
    }
    Ok(())
}

fn read_attributes(c: &mut Cursor, n: usize) -> Result<Attributes> {
    let mut attrs = Attributes::new();
    while let Some(l) = c.peek_line() {
        let t = tokens(&l);
        let kw = t[0].to_ascii_uppercase();
        match kw.as_str() {
            "SCALARS" => {
                c.line();
                let name = t[1].clone();
                let ty = parse_type(&t[2])?;
                let ncomp: usize = t.get(3).and_then(|s| s.parse().ok()).unwrap_or(1);
                // Optional LOOKUP_TABLE line.
                if let Some(nl) = c.peek_line() {
                    if nl.to_ascii_uppercase().starts_with("LOOKUP_TABLE") {
                        c.line();
                    }
                }
                let data = if t[2].eq_ignore_ascii_case("bit") { c.bits(n * ncomp)? } else { c.values(ty, n * ncomp)? };
                attrs.insert_active(AttributeKind::Scalars, Array::new(name, ncomp, data));
            }
            "COLOR_SCALARS" => {
                c.line();
                let name = t[1].clone();
                let ncomp: usize = t[2].parse()?;
                let data = if c.binary { c.values(ScalarType::U8, n * ncomp)? } else { c.values(ScalarType::F32, n * ncomp)? };
                attrs.insert(Array::new(name, ncomp, data));
            }
            "VECTORS" | "NORMALS" => {
                c.line();
                let ty = parse_type(&t[2])?;
                let data = c.values(ty, n * 3)?;
                let kind = if kw == "VECTORS" { AttributeKind::Vectors } else { AttributeKind::Normals };
                attrs.insert_active(kind, Array::new(t[1].clone(), 3, data));
            }
            "TEXTURE_COORDINATES" => {
                c.line();
                let dim: usize = t[2].parse()?;
                let ty = parse_type(&t[3])?;
                let data = c.values(ty, n * dim)?;
                attrs.insert_active(AttributeKind::TCoords, Array::new(t[1].clone(), dim, data));
            }
            "TENSORS" => {
                c.line();
                let ty = parse_type(&t[2])?;
                let data = c.values(ty, n * 9)?;
                attrs.insert_active(AttributeKind::Tensors, Array::new(t[1].clone(), 9, data));
            }
            "LOOKUP_TABLE" => {
                c.line();
                let size: usize = t[2].parse()?;
                if c.binary {
                    c.values(ScalarType::U8, 4 * size)?;
                } else {
                    c.values(ScalarType::F32, 4 * size)?;
                }
            }
            "FIELD" => {
                c.line();
                let na: usize = t[2].parse()?;
                read_field(c, na, &mut attrs)?;
            }
            "METADATA" => {
                c.line();
                skip_metadata(c);
            }
            _ => break,
        }
    }
    Ok(attrs)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ascii_unstructured() {
        let src = b"# vtk DataFile Version 3.0\ntest\nASCII\nDATASET UNSTRUCTURED_GRID\nPOINTS 4 float\n0 0 0\n1 0 0\n0 1 0\n0 0 1\nCELLS 1 5\n4 0 1 2 3\nCELL_TYPES 1\n10\nPOINT_DATA 4\nSCALARS temp float 1\nLOOKUP_TABLE default\n1 2 3 4\n";
        let ds = parse(src).unwrap();
        assert_eq!(ds.n_points(), 4);
        assert_eq!(ds.n_cells(), 1);
        let a = ds.point_data().unwrap().get("temp").unwrap();
        assert_eq!(a.range(None), (1.0, 4.0));
    }
}
