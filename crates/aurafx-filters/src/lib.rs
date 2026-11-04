//! Filters: pure functions from datasets to datasets, plus a registry with typed
//! parameter schemas so the UI, Python and the AI assistant can all drive them.

pub mod interp;
pub mod params;
pub mod registry;
pub mod simplex;

pub mod analysis;
pub mod append;
pub mod attributes;
pub mod calculator;
pub mod clip;
pub mod contour;
pub mod edges;
pub mod geometry;
pub mod glyph;
pub mod outline;
pub mod probe;
pub mod selection;
pub mod slice;
pub mod sources;
pub mod streamlines;
pub mod surface;
pub mod threshold;
pub mod transform;
pub mod warp;

pub use params::{param_from_json, param_to_json, Association, ParamKind, ParamSpec, ParamValue, Params};
pub use registry::{default_array, find, registry, search, FilterDef, MultiInput};
