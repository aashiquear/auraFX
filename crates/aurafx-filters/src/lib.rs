//! Filters: pure functions from datasets to datasets, plus a registry with typed
//! parameter schemas so the UI, Python and the AI assistant can all drive them.

pub mod interp;
pub mod molecule;
pub mod params;
pub mod registry;
pub mod simplex;

pub mod amr;
pub mod analysis;
pub mod append;
pub mod attributes;
pub mod calculator;
pub mod clip;
pub mod contour;
pub mod edges;
pub mod geometry;
pub mod glyph;
pub mod higher_order;
pub mod htg;
pub mod outline;
pub mod probe;
pub mod python;
pub mod selection;
pub mod slice;
pub mod sources;
pub mod streamlines;
pub mod surface;
pub mod temporal;
pub mod texture;
pub mod threshold;
pub mod transform;
pub mod warp;

pub use params::{param_from_json, param_to_json, Association, ParamKind, ParamSpec, ParamValue, Params};
pub use registry::{default_array, find, register, registry, search, unregister_plugin, CustomFn, Extension, FilterDef, MultiInput, Temporal};

/// Use `n` threads for filters (0 = all cores; `AURAFX_THREADS` overrides). Only the first call
/// before any parallel work takes effect. Returns the number of threads in use.
pub fn init_threads(n: usize) -> usize {
    let n = std::env::var("AURAFX_THREADS").ok().and_then(|v| v.parse().ok()).unwrap_or(n);
    if n > 0 {
        let _ = rayon::ThreadPoolBuilder::new().num_threads(n).build_global();
    }
    rayon::current_num_threads()
}
