//! # aurafx-cad: the parametric CAD document
//!
//! The data behind the CAD modeling environment (roadmap section 13, design in
//! `docs/design/cad-environment.md`): a feature timeline with constrained sketches, parameters
//! and unit-aware expressions, evaluated by a *kernel* into bodies with named faces, a surface
//! and a signed distance, which is what the rest of auraFX (meshers, grains, solver cases)
//! consumes. The document never depends on a kernel; it is plain serde data.
//!
//! Stage C0 (this commit) holds the crate, the kernel trait and the expression evaluator
//! ported from EZ-YX-craft (the user's own project, MIT OR Apache-2.0). The stages that
//! follow, with their files, tests and sizes, are listed in the design document, section 6:
//!
//! - C1 `document` (features, parameters, configurations, templates, analysis, assemblies),
//!   `sketch` and `path` (the constraint solver), `.afxcad` files;
//! - C2 `kernel::implicit` on `aurafx-mesh`'s implicit geometry;
//! - C5 `diff` (old versus new geometry for the downstream check);
//! - C6 `kernel::brep` (SolidCraft's kernel, carried in `vendor/`) and `io` (STEP, IGES,
//!   3MF, `.sc`).

pub mod analysis;
pub mod assembly;
pub mod diff;
pub mod document;
pub mod edit;
pub mod expr;
pub mod file;
pub mod io;
pub mod kernel;
pub mod mesh;
mod params;
pub mod path;
pub mod sketch;
pub mod templates;

pub use analysis::{interference_built, interference_volume, measure_faces, FaceMeasure, MassProperties};
pub use assembly::{Assembly, Component};
pub use document::{
    Axis, Binding, BlendKind, BooleanOp, Configuration, Document, EdgeTreatment, EndCondition, Feature, HoleKind, LoftSection, MirrorPlane, Modifier,
    Parameter, Placement, Shape,
};
pub use expr::{eval, eval_const, eval_map, identifiers, unit_factor, ExprError};
pub use file::{CadFile, FileError, FORMAT_VERSION};
pub use kernel::{Evaluated, EvaluatedBody, EvaluatedFace, FaceKind, Kernel, KernelChoice};
pub use sketch::{Constraint, Dim, Entity, PlaneFrame, SPoint, Sketch, SketchPlane, SolveReport, SolveStatus};
pub use templates::templates;

/// Errors of the document and its kernels.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum CadError {
    #[error("expression for '{name}': {source}")]
    Expr {
        name: String,
        #[source]
        source: ExprError,
    },
    #[error("parameter cycle: {0}")]
    Cycle(String),
    #[error("unknown configuration '{0}'")]
    UnknownConfiguration(String),
    #[error("binding '{path}' on feature '{feature}': {reason}")]
    Binding { feature: String, path: String, reason: String },
    #[error("{kernel} kernel: {reason}")]
    Kernel { kernel: &'static str, reason: String },
    #[error("the {0} kernel refused: {1}")]
    KernelRefused(&'static str, String),
    #[error("{0}")]
    Edit(String),
}

/// A stable identity of a feature, sketch entity or body: never reused within a document, so
/// the downstream check and the B-rep mirror can follow an object across edits.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default, serde::Serialize, serde::Deserialize)]
pub struct Id(pub u64);
