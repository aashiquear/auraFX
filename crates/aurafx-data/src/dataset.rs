//! Dataset types.

use crate::array::{Array, ArrayData};
use crate::attributes::Attributes;
use crate::bounds::Bounds;
use crate::cell::{CellType, Cells};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::Arc;

/// Point coordinates (3 components, f32 or f64).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Points {
    pub data: ArrayData,
}

impl Points {
    pub fn new_f64(v: Vec<f64>) -> Self {
        Self { data: ArrayData::F64(v) }
    }
    pub fn new_f32(v: Vec<f32>) -> Self {
        Self { data: ArrayData::F32(v) }
    }
    pub fn from_array(data: ArrayData) -> Self {
        Self { data }
    }
    pub fn empty() -> Self {
        Self { data: ArrayData::F32(Vec::new()) }
    }
    pub fn len(&self) -> usize {
        self.data.len() / 3
    }
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
    #[inline]
    pub fn get(&self, i: usize) -> [f64; 3] {
        match &self.data {
            ArrayData::F64(v) => [v[3 * i], v[3 * i + 1], v[3 * i + 2]],
            ArrayData::F32(v) => [v[3 * i] as f64, v[3 * i + 1] as f64, v[3 * i + 2] as f64],
            d => [d.get_f64(3 * i), d.get_f64(3 * i + 1), d.get_f64(3 * i + 2)],
        }
    }
    pub fn set(&mut self, i: usize, p: [f64; 3]) {
        match &mut self.data {
            ArrayData::F64(v) => {
                v[3 * i..3 * i + 3].copy_from_slice(&p);
            }
            ArrayData::F32(v) => {
                v[3 * i] = p[0] as f32;
                v[3 * i + 1] = p[1] as f32;
                v[3 * i + 2] = p[2] as f32;
            }
            _ => {
                let mut all = self.data.to_f64_vec();
                all[3 * i..3 * i + 3].copy_from_slice(&p);
                self.data = ArrayData::F64(all);
            }
        }
    }
    pub fn push(&mut self, p: [f64; 3]) {
        match &mut self.data {
            ArrayData::F64(v) => v.extend_from_slice(&p),
            ArrayData::F32(v) => v.extend_from_slice(&[p[0] as f32, p[1] as f32, p[2] as f32]),
            _ => {
                let mut all = self.data.to_f64_vec();
                all.extend_from_slice(&p);
                self.data = ArrayData::F64(all);
            }
        }
    }
    pub fn bounds(&self) -> Bounds {
        let mut b = Bounds::EMPTY;
        for i in 0..self.len() {
            b.add_point(self.get(i));
        }
        b
    }
    pub fn to_f32(&self) -> Vec<f32> {
        self.data.to_f32_vec()
    }
    pub fn gather(&self, idx: &[usize]) -> Points {
        Points { data: self.data.gather(3, idx) }
    }
    pub fn append(&mut self, other: &Points) {
        if self.data.append(&other.data).is_err() {
            let mut all = self.data.to_f64_vec();
            all.extend(other.data.to_f64_vec());
            self.data = ArrayData::F64(all);
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct UnstructuredGrid {
    pub points: Points,
    pub cells: Cells,
    pub point_data: Attributes,
    pub cell_data: Attributes,
    pub field_data: Attributes,
}

impl Default for Points {
    fn default() -> Self {
        Points::empty()
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct PolyData {
    pub points: Points,
    pub verts: Cells,
    pub lines: Cells,
    pub polys: Cells,
    pub strips: Cells,
    pub point_data: Attributes,
    /// Cell data is ordered verts, lines, polys, strips (VTK convention).
    pub cell_data: Attributes,
    pub field_data: Attributes,
}

impl PolyData {
    pub fn n_cells(&self) -> usize {
        self.verts.len() + self.lines.len() + self.polys.len() + self.strips.len()
    }
    /// Point cloud with one vertex cell per point.
    pub fn from_points(points: Points) -> Self {
        let n = points.len();
        let mut verts = Cells::with_capacity(n, n);
        for i in 0..n {
            verts.push(CellType::Vertex, &[i as u32]);
        }
        Self { points, verts, ..Default::default() }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ImageData {
    /// Whole extent: [x0, x1, y0, y1, z0, z1] (inclusive point indices).
    pub extent: [i64; 6],
    pub origin: [f64; 3],
    pub spacing: [f64; 3],
    pub direction: [f64; 9],
    pub point_data: Attributes,
    pub cell_data: Attributes,
    pub field_data: Attributes,
}

impl Default for ImageData {
    fn default() -> Self {
        Self {
            extent: [0, -1, 0, -1, 0, -1],
            origin: [0.0; 3],
            spacing: [1.0; 3],
            direction: [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0],
            point_data: Attributes::new(),
            cell_data: Attributes::new(),
            field_data: Attributes::new(),
        }
    }
}

impl ImageData {
    pub fn dims(&self) -> [usize; 3] {
        let e = &self.extent;
        [
            (e[1] - e[0] + 1).max(0) as usize,
            (e[3] - e[2] + 1).max(0) as usize,
            (e[5] - e[4] + 1).max(0) as usize,
        ]
    }
    pub fn n_points(&self) -> usize {
        let d = self.dims();
        d[0] * d[1] * d[2]
    }
    pub fn n_cells(&self) -> usize {
        let d = self.dims();
        d.iter().map(|&n| n.saturating_sub(1).max(if n == 0 { 0 } else { 1 })).product::<usize>()
            * if d.iter().any(|&n| n == 0) { 0 } else { 1 }
    }
    pub fn point(&self, i: usize, j: usize, k: usize) -> [f64; 3] {
        let e = &self.extent;
        [
            self.origin[0] + (e[0] as f64 + i as f64) * self.spacing[0],
            self.origin[1] + (e[2] as f64 + j as f64) * self.spacing[1],
            self.origin[2] + (e[4] as f64 + k as f64) * self.spacing[2],
        ]
    }
    pub fn bounds(&self) -> Bounds {
        let d = self.dims();
        if d.iter().any(|&n| n == 0) {
            return Bounds::EMPTY;
        }
        let mut b = Bounds::EMPTY;
        b.add_point(self.point(0, 0, 0));
        b.add_point(self.point(d[0] - 1, d[1] - 1, d[2] - 1));
        b
    }
    /// Generate explicit points (for conversion to structured/unstructured).
    pub fn points(&self) -> Points {
        let d = self.dims();
        let mut v = Vec::with_capacity(3 * d[0] * d[1] * d[2]);
        for k in 0..d[2] {
            for j in 0..d[1] {
                for i in 0..d[0] {
                    v.extend_from_slice(&self.point(i, j, k));
                }
            }
        }
        Points::new_f64(v)
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct RectilinearGrid {
    pub extent: [i64; 6],
    pub x: Vec<f64>,
    pub y: Vec<f64>,
    pub z: Vec<f64>,
    pub point_data: Attributes,
    pub cell_data: Attributes,
    pub field_data: Attributes,
}

impl RectilinearGrid {
    pub fn dims(&self) -> [usize; 3] {
        [self.x.len(), self.y.len(), self.z.len()]
    }
    pub fn n_points(&self) -> usize {
        self.x.len() * self.y.len() * self.z.len()
    }
    pub fn points(&self) -> Points {
        let mut v = Vec::with_capacity(3 * self.n_points());
        for &z in &self.z {
            for &y in &self.y {
                for &x in &self.x {
                    v.extend_from_slice(&[x, y, z]);
                }
            }
        }
        Points::new_f64(v)
    }
    pub fn bounds(&self) -> Bounds {
        if self.n_points() == 0 {
            return Bounds::EMPTY;
        }
        let mut b = Bounds::EMPTY;
        b.add_point([self.x[0], self.y[0], self.z[0]]);
        b.add_point([*self.x.last().unwrap(), *self.y.last().unwrap(), *self.z.last().unwrap()]);
        b
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct StructuredGrid {
    pub extent: [i64; 6],
    pub points: Points,
    pub point_data: Attributes,
    pub cell_data: Attributes,
    pub field_data: Attributes,
}

impl StructuredGrid {
    pub fn dims(&self) -> [usize; 3] {
        let e = &self.extent;
        [
            (e[1] - e[0] + 1).max(0) as usize,
            (e[3] - e[2] + 1).max(0) as usize,
            (e[5] - e[4] + 1).max(0) as usize,
        ]
    }
}

/// Row-oriented table (every column has the same number of tuples).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Table {
    pub columns: Attributes,
    pub field_data: Attributes,
}

impl Table {
    pub fn n_rows(&self) -> usize {
        self.columns.iter().next().map(|a| a.len()).unwrap_or(0)
    }
    pub fn column(&self, name: &str) -> Option<&Array> {
        self.columns.get(name)
    }
}

/// Composite dataset: a tree of named blocks.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct MultiBlock {
    pub blocks: Vec<(String, Option<DataSet>)>,
    pub field_data: Attributes,
}

impl MultiBlock {
    /// Depth-first iteration over all leaf datasets with their path names.
    pub fn leaves(&self) -> Vec<(String, &DataSet)> {
        let mut out = Vec::new();
        fn walk<'a>(mb: &'a MultiBlock, prefix: &str, out: &mut Vec<(String, &'a DataSet)>) {
            for (name, ds) in &mb.blocks {
                let path = if prefix.is_empty() { name.clone() } else { format!("{prefix}/{name}") };
                match ds {
                    Some(DataSet::MultiBlock(inner)) => walk(inner, &path, out),
                    Some(ds) => out.push((path, ds)),
                    None => {}
                }
            }
        }
        walk(self, "", &mut out);
        out
    }
}

/// A lazily-resolved time series: each step is either loaded or refers to a file.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct TimeSeries {
    pub times: Vec<f64>,
    pub steps: Vec<TimeStepRef>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum TimeStepRef {
    File(PathBuf),
    /// Frame `frame` inside a multi-frame file (e.g. LAMMPS dump).
    FileFrame { path: PathBuf, frame: usize },
    #[serde(skip)]
    Loaded(Arc<DataSet>),
}

impl Default for TimeStepRef {
    fn default() -> Self {
        TimeStepRef::File(PathBuf::new())
    }
}

/// Any dataset.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum DataSet {
    ImageData(ImageData),
    RectilinearGrid(RectilinearGrid),
    StructuredGrid(StructuredGrid),
    PolyData(PolyData),
    UnstructuredGrid(UnstructuredGrid),
    Table(Table),
    MultiBlock(MultiBlock),
}

impl DataSet {
    pub fn type_name(&self) -> &'static str {
        match self {
            DataSet::ImageData(_) => "Image Data",
            DataSet::RectilinearGrid(_) => "Rectilinear Grid",
            DataSet::StructuredGrid(_) => "Structured Grid",
            DataSet::PolyData(_) => "Polygonal Mesh",
            DataSet::UnstructuredGrid(_) => "Unstructured Grid",
            DataSet::Table(_) => "Table",
            DataSet::MultiBlock(_) => "Multi-block Dataset",
        }
    }

    pub fn n_points(&self) -> usize {
        match self {
            DataSet::ImageData(d) => d.n_points(),
            DataSet::RectilinearGrid(d) => d.n_points(),
            DataSet::StructuredGrid(d) => d.points.len(),
            DataSet::PolyData(d) => d.points.len(),
            DataSet::UnstructuredGrid(d) => d.points.len(),
            DataSet::Table(_) => 0,
            DataSet::MultiBlock(m) => m.leaves().iter().map(|(_, d)| d.n_points()).sum(),
        }
    }

    pub fn n_cells(&self) -> usize {
        match self {
            DataSet::ImageData(d) => d.n_cells(),
            DataSet::RectilinearGrid(d) => {
                let dm = d.dims();
                dm.iter().map(|&n| n.saturating_sub(1).max(1)).product::<usize>() * if dm.iter().any(|&n| n == 0) { 0 } else { 1 }
            }
            DataSet::StructuredGrid(d) => {
                let dm = d.dims();
                dm.iter().map(|&n| n.saturating_sub(1).max(1)).product::<usize>() * if dm.iter().any(|&n| n == 0) { 0 } else { 1 }
            }
            DataSet::PolyData(d) => d.n_cells(),
            DataSet::UnstructuredGrid(d) => d.cells.len(),
            DataSet::Table(t) => t.n_rows(),
            DataSet::MultiBlock(m) => m.leaves().iter().map(|(_, d)| d.n_cells()).sum(),
        }
    }

    pub fn bounds(&self) -> Bounds {
        match self {
            DataSet::ImageData(d) => d.bounds(),
            DataSet::RectilinearGrid(d) => d.bounds(),
            DataSet::StructuredGrid(d) => d.points.bounds(),
            DataSet::PolyData(d) => d.points.bounds(),
            DataSet::UnstructuredGrid(d) => d.points.bounds(),
            DataSet::Table(_) => Bounds::EMPTY,
            DataSet::MultiBlock(m) => m.leaves().iter().fold(Bounds::EMPTY, |b, (_, d)| b.union(&d.bounds())),
        }
    }

    pub fn point_data(&self) -> Option<&Attributes> {
        match self {
            DataSet::ImageData(d) => Some(&d.point_data),
            DataSet::RectilinearGrid(d) => Some(&d.point_data),
            DataSet::StructuredGrid(d) => Some(&d.point_data),
            DataSet::PolyData(d) => Some(&d.point_data),
            DataSet::UnstructuredGrid(d) => Some(&d.point_data),
            DataSet::Table(_) | DataSet::MultiBlock(_) => None,
        }
    }

    pub fn point_data_mut(&mut self) -> Option<&mut Attributes> {
        match self {
            DataSet::ImageData(d) => Some(&mut d.point_data),
            DataSet::RectilinearGrid(d) => Some(&mut d.point_data),
            DataSet::StructuredGrid(d) => Some(&mut d.point_data),
            DataSet::PolyData(d) => Some(&mut d.point_data),
            DataSet::UnstructuredGrid(d) => Some(&mut d.point_data),
            DataSet::Table(_) | DataSet::MultiBlock(_) => None,
        }
    }

    pub fn cell_data(&self) -> Option<&Attributes> {
        match self {
            DataSet::ImageData(d) => Some(&d.cell_data),
            DataSet::RectilinearGrid(d) => Some(&d.cell_data),
            DataSet::StructuredGrid(d) => Some(&d.cell_data),
            DataSet::PolyData(d) => Some(&d.cell_data),
            DataSet::UnstructuredGrid(d) => Some(&d.cell_data),
            DataSet::Table(_) | DataSet::MultiBlock(_) => None,
        }
    }

    pub fn cell_data_mut(&mut self) -> Option<&mut Attributes> {
        match self {
            DataSet::ImageData(d) => Some(&mut d.cell_data),
            DataSet::RectilinearGrid(d) => Some(&mut d.cell_data),
            DataSet::StructuredGrid(d) => Some(&mut d.cell_data),
            DataSet::PolyData(d) => Some(&mut d.cell_data),
            DataSet::UnstructuredGrid(d) => Some(&mut d.cell_data),
            DataSet::Table(_) | DataSet::MultiBlock(_) => None,
        }
    }

    pub fn field_data(&self) -> &Attributes {
        match self {
            DataSet::ImageData(d) => &d.field_data,
            DataSet::RectilinearGrid(d) => &d.field_data,
            DataSet::StructuredGrid(d) => &d.field_data,
            DataSet::PolyData(d) => &d.field_data,
            DataSet::UnstructuredGrid(d) => &d.field_data,
            DataSet::Table(t) => &t.field_data,
            DataSet::MultiBlock(m) => &m.field_data,
        }
    }

    /// Explicit point coordinates for any geometric dataset (generated for implicit grids).
    pub fn points(&self) -> Option<Points> {
        match self {
            DataSet::ImageData(d) => Some(d.points()),
            DataSet::RectilinearGrid(d) => Some(d.points()),
            DataSet::StructuredGrid(d) => Some(d.points.clone()),
            DataSet::PolyData(d) => Some(d.points.clone()),
            DataSet::UnstructuredGrid(d) => Some(d.points.clone()),
            _ => None,
        }
    }

    /// Approximate memory footprint in bytes.
    pub fn n_bytes(&self) -> usize {
        let attrs = |a: &Attributes| a.n_bytes();
        match self {
            DataSet::ImageData(d) => attrs(&d.point_data) + attrs(&d.cell_data),
            DataSet::RectilinearGrid(d) => attrs(&d.point_data) + attrs(&d.cell_data) + 8 * (d.x.len() + d.y.len() + d.z.len()),
            DataSet::StructuredGrid(d) => attrs(&d.point_data) + attrs(&d.cell_data) + d.points.data.len() * d.points.data.scalar_type().size_of(),
            DataSet::PolyData(d) => {
                attrs(&d.point_data)
                    + attrs(&d.cell_data)
                    + d.points.data.len() * d.points.data.scalar_type().size_of()
                    + d.verts.n_bytes()
                    + d.lines.n_bytes()
                    + d.polys.n_bytes()
                    + d.strips.n_bytes()
            }
            DataSet::UnstructuredGrid(d) => {
                attrs(&d.point_data) + attrs(&d.cell_data) + d.points.data.len() * d.points.data.scalar_type().size_of() + d.cells.n_bytes()
            }
            DataSet::Table(t) => attrs(&t.columns),
            DataSet::MultiBlock(m) => m.leaves().iter().map(|(_, d)| d.n_bytes()).sum(),
        }
    }

    /// Convert any structured dataset into an unstructured grid (hexahedra / quads / lines).
    pub fn to_unstructured(&self) -> Option<UnstructuredGrid> {
        let (dims, points, pd, cd, fd) = match self {
            DataSet::ImageData(d) => (d.dims(), d.points(), d.point_data.clone(), d.cell_data.clone(), d.field_data.clone()),
            DataSet::RectilinearGrid(d) => (d.dims(), d.points(), d.point_data.clone(), d.cell_data.clone(), d.field_data.clone()),
            DataSet::StructuredGrid(d) => (d.dims(), d.points.clone(), d.point_data.clone(), d.cell_data.clone(), d.field_data.clone()),
            DataSet::UnstructuredGrid(d) => return Some(d.clone()),
            DataSet::PolyData(p) => {
                let mut cells = Cells::new();
                for c in [&p.verts, &p.lines, &p.polys, &p.strips] {
                    cells.append(c, 0);
                }
                return Some(UnstructuredGrid {
                    points: p.points.clone(),
                    cells,
                    point_data: p.point_data.clone(),
                    cell_data: p.cell_data.clone(),
                    field_data: p.field_data.clone(),
                });
            }
            _ => return None,
        };
        Some(UnstructuredGrid { points, cells: structured_cells(dims), point_data: pd, cell_data: cd, field_data: fd })
    }
}

/// Build hexahedra (or quads/lines for degenerate dims) for an i-fastest structured grid.
pub fn structured_cells(dims: [usize; 3]) -> Cells {
    let [nx, ny, nz] = dims;
    let idx = |i: usize, j: usize, k: usize| (i + nx * (j + ny * k)) as u32;
    let cx = nx.saturating_sub(1).max(if nx == 0 { 0 } else { 1 });
    let cy = ny.saturating_sub(1).max(if ny == 0 { 0 } else { 1 });
    let cz = nz.saturating_sub(1).max(if nz == 0 { 0 } else { 1 });
    let mut cells = Cells::with_capacity(cx * cy * cz, 8 * cx * cy * cz);
    if nx == 0 || ny == 0 || nz == 0 {
        return cells;
    }
    let dim3 = nx > 1 && ny > 1 && nz > 1;
    for k in 0..cz {
        for j in 0..cy {
            for i in 0..cx {
                if dim3 {
                    cells.push(
                        CellType::Hexahedron,
                        &[
                            idx(i, j, k),
                            idx(i + 1, j, k),
                            idx(i + 1, j + 1, k),
                            idx(i, j + 1, k),
                            idx(i, j, k + 1),
                            idx(i + 1, j, k + 1),
                            idx(i + 1, j + 1, k + 1),
                            idx(i, j + 1, k + 1),
                        ],
                    );
                } else if nz == 1 && nx > 1 && ny > 1 {
                    cells.push(CellType::Quad, &[idx(i, j, 0), idx(i + 1, j, 0), idx(i + 1, j + 1, 0), idx(i, j + 1, 0)]);
                } else if ny == 1 && nx > 1 && nz > 1 {
                    cells.push(CellType::Quad, &[idx(i, 0, k), idx(i + 1, 0, k), idx(i + 1, 0, k + 1), idx(i, 0, k + 1)]);
                } else if nx == 1 && ny > 1 && nz > 1 {
                    cells.push(CellType::Quad, &[idx(0, j, k), idx(0, j + 1, k), idx(0, j + 1, k + 1), idx(0, j, k + 1)]);
                } else if nx > 1 {
                    cells.push(CellType::Line, &[idx(i, 0, 0), idx(i + 1, 0, 0)]);
                } else if ny > 1 {
                    cells.push(CellType::Line, &[idx(0, j, 0), idx(0, j + 1, 0)]);
                } else if nz > 1 {
                    cells.push(CellType::Line, &[idx(0, 0, k), idx(0, 0, k + 1)]);
                } else {
                    cells.push(CellType::Vertex, &[0]);
                }
            }
        }
    }
    cells
}
