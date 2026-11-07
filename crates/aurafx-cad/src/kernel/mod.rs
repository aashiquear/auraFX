//! Kernels turn a document into geometry. Two exist (design section 3.2): `implicit` (always
//! built; signed distance fields, never fails topologically) and `brep` (the B-rep kernel
//! carried in `vendor/`, arrives with stage C6). Both fill the same [`Evaluated`] contract, so
//! the meshers, the grain tools and the solver cases consume either.

pub mod implicit;
pub mod sdf;

use crate::{CadError, Document, Id};
use aurafx_data::*;
use std::sync::Arc;

/// A signed distance to the union of the bodies (negative inside), shareable and callable
/// from any thread.
pub type DistanceFn = Arc<dyn Fn([f64; 3]) -> f64 + Send + Sync>;

/// What a kernel produces for a document in one configuration.
#[derive(Clone)]
pub struct Evaluated {
    /// One entry per solid body, in timeline order.
    pub bodies: Vec<EvaluatedBody>,
    /// The display and meshing surface: polygonal data with the cell arrays `BodyId` and
    /// `FaceId` and the field arrays `FaceNames` (indexed by `FaceId`), `BodyIds`, `BodyNames`,
    /// `Volume`, `SurfaceArea` (the layout the Geometry source writes, so every consumer of its
    /// output works unchanged).
    pub surface: DataSet,
    /// Signed distance to the union of the bodies, when the kernel has one.
    pub distance: Option<DistanceFn>,
    /// Which kernel made this.
    pub kernel: &'static str,
    /// Operations this kernel had to approximate (a fillet on picked edges on the implicit
    /// kernel, say), by feature name, for the timeline to flag.
    pub approximate: Vec<String>,
}

impl std::fmt::Debug for Evaluated {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Evaluated({} kernel, {} bodies)", self.kernel, self.bodies.len())
    }
}

impl Evaluated {
    /// Every face of every body, with its body.
    pub fn faces(&self) -> impl Iterator<Item = (&EvaluatedBody, &EvaluatedFace)> {
        self.bodies.iter().flat_map(|b| b.faces.iter().map(move |f| (b, f)))
    }
    pub fn face_by_name(&self, name: &str) -> Option<&EvaluatedFace> {
        self.faces().map(|(_, f)| f).find(|f| f.name == name)
    }
    pub fn body_by_name(&self, name: &str) -> Option<&EvaluatedBody> {
        self.bodies.iter().find(|b| b.name == name)
    }
    pub fn volume(&self) -> f64 {
        self.bodies.iter().map(|b| b.volume).sum()
    }
}

#[derive(Debug, Clone)]
pub struct EvaluatedBody {
    pub id: Id,
    pub name: String,
    pub faces: Vec<EvaluatedFace>,
    pub volume: f64,
    pub area: f64,
    pub centroid: [f64; 3],
    pub bounds: ([f64; 3], [f64; 3]),
}

#[derive(Debug, Clone)]
pub struct EvaluatedFace {
    /// Stable across edits while the owning feature exists: `(feature id, index in feature)`.
    pub id: (Id, u32),
    /// `FaceId` value written into `surface`.
    pub face_id: u32,
    pub name: String,
    pub kind: FaceKind,
    pub area: f64,
    pub centroid: [f64; 3],
    /// Outward normal for planar faces, the mean outward direction for round ones.
    pub direction: [f64; 3],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FaceKind {
    Plane,
    Cylinder,
    Cone,
    Sphere,
    Torus,
    Freeform,
}

impl FaceKind {
    pub fn label(self) -> &'static str {
        match self {
            FaceKind::Plane => "plane",
            FaceKind::Cylinder => "cylinder",
            FaceKind::Cone => "cone",
            FaceKind::Sphere => "sphere",
            FaceKind::Torus => "torus",
            FaceKind::Freeform => "freeform",
        }
    }
}

/// A kernel evaluates a document (with its parameters and active configuration resolved).
pub trait Kernel: Send + Sync {
    /// "implicit" or "brep".
    fn name(&self) -> &'static str;
    fn evaluate(&self, document: &Document) -> Result<Evaluated, CadError>;
}

/// One body's triangles as a kernel produces them, before they become the surface data set.
pub struct BodyMesh {
    pub id: Id,
    pub name: String,
    pub points: Vec<[f64; 3]>,
    pub tris: Vec<[u32; 3]>,
    /// `FaceId` of every triangle.
    pub face: Vec<u32>,
}

/// The surface data set of bodies, in the layout of [`Evaluated::surface`].
pub fn surface_dataset(meshes: &[BodyMesh], face_names: &[String], volumes: &[f64], areas: &[f64]) -> DataSet {
    let mut pts: Vec<f64> = Vec::new();
    let mut polys = Cells::new();
    let (mut body_ids, mut face_ids) = (Vec::new(), Vec::new());
    for m in meshes {
        let base = (pts.len() / 3) as u32;
        pts.extend(m.points.iter().flatten());
        for t in &m.tris {
            polys.push(CellType::Triangle, &t.map(|i| i + base));
        }
        body_ids.extend(std::iter::repeat_n(m.id.0 as i32, m.tris.len()));
        face_ids.extend(m.face.iter().map(|f| *f as i32));
    }
    let mut field = Attributes::new();
    field.insert(Array::new("FaceNames", 1, ArrayData::String(face_names.to_vec())));
    field.insert(Array::new("BodyIds", 1, ArrayData::I32(meshes.iter().map(|m| m.id.0 as i32).collect())));
    field.insert(Array::new("BodyNames", 1, ArrayData::String(meshes.iter().map(|m| m.name.clone()).collect())));
    field.insert(Array::scalars_f64("Volume", volumes.to_vec()));
    field.insert(Array::scalars_f64("SurfaceArea", areas.to_vec()));
    let mut pd = PolyData { points: Points::new_f64(pts), polys, field_data: field, ..Default::default() };
    pd.cell_data.insert(Array::new("BodyId", 1, ArrayData::I32(body_ids)));
    pd.cell_data.insert(Array::new("FaceId", 1, ArrayData::I32(face_ids)));
    DataSet::PolyData(pd)
}
