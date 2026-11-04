//! Filters: pure functions from datasets to datasets, plus a registry with typed
//! parameter schemas so the UI, Python and the AI assistant can all drive them.

pub mod params;
pub mod registry;
pub mod simplex;
pub mod interp;

pub mod surface;
pub mod slice;
pub mod clip;
pub mod contour;
pub mod threshold;
pub mod attributes;
pub mod calculator;
pub mod warp;
pub mod outline;
pub mod edges;
pub mod transform;
pub mod glyph;
pub mod sources;
pub mod probe;

pub use params::{ParamKind, ParamSpec, ParamValue, Params, Association};
pub use registry::{default_array, find, registry, search, FilterDef};
