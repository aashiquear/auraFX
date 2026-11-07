//! Kernels turn a document into geometry. Two are planned (design section 3.2): `implicit`
//! (always built; the port of EZ-YX-craft's signed-distance kernel onto `aurafx-mesh`) and
//! `brep` (SolidCraft's B-rep kernel, carried in `vendor/` and built in). Stage C0 defines the
//! contract they both fill.

use crate::Id;
use aurafx_data::DataSet;

/// What a kernel produces for a document in one configuration.
pub struct Evaluated {
    /// One entry per solid body, in timeline order.
    pub bodies: Vec<EvaluatedBody>,
    /// The display and meshing surface: a multiblock of polygonal data, one block per body,
    /// with cell arrays `BodyId` and `FaceId` and field arrays `BodyNames`, `FaceNames`,
    /// `Volume`, `SurfaceArea` (the layout the Geometry source writes today, so every consumer
    /// of its output works unchanged).
    pub surface: DataSet,
    /// Signed distance to the union of the bodies (negative inside), when the kernel has one.
    pub distance: Option<Box<dyn Fn([f64; 3]) -> f64 + Send + Sync>>,
}

pub struct EvaluatedBody {
    pub id: Id,
    pub name: String,
    pub faces: Vec<EvaluatedFace>,
    pub volume: f64,
    pub centroid: [f64; 3],
    pub bounds: ([f64; 3], [f64; 3]),
}

pub struct EvaluatedFace {
    /// Stable across edits while the owning feature exists: `(feature id, index in feature)`.
    pub id: (Id, u32),
    /// `BodyId`/`FaceId` values written into `surface`.
    pub face_id: u32,
    pub name: String,
    pub kind: FaceKind,
    pub area: f64,
    pub centroid: [f64; 3],
    /// Outward normal for planar faces, the axis for round ones.
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

/// A kernel evaluates a document. `Document` arrives with stage C1; until then the trait is
/// generic over it so the contract can be reviewed on its own.
pub trait Kernel<Document>: Send + Sync {
    /// "implicit" or "brep".
    fn name(&self) -> &'static str;
    fn evaluate(&self, document: &Document) -> Result<Evaluated, crate::CadError>;
}
