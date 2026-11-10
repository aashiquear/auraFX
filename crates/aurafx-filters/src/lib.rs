//! Filters: pure functions from datasets to datasets, plus a registry with typed
//! parameter schemas so the UI, Python and the AI assistant can all drive them.

pub mod interp;
pub mod mesh_sets;
pub mod micro_extra;
pub mod molecule;
pub mod params;
pub mod registry;
pub mod simplex;

pub mod am;
pub mod amr;
pub mod analysis;
pub mod annotate;
pub mod anomaly;
pub mod append;
pub mod attributes;
pub mod cad;
pub mod calculator;
pub mod cellcheck;
pub mod cellgrid;
pub mod cleanup;
pub mod clip;
pub mod contour;
pub mod cosmology;
pub mod crystal;
pub mod crystal_texture;
pub mod delaunay3d;
pub mod edges;
pub mod extract;
pub mod feature_metrics;
pub mod features;
pub mod geometry;
pub mod geometry_model;
pub mod glyph;
pub mod grain_grid;
pub mod grain_surface;
pub mod higher_order;
pub mod htg;
pub mod image_filters;
pub mod lines;
pub mod materials;
pub mod mechanics;
pub mod mesh_gen;
pub mod outline;
pub mod partitions;
pub mod point_ops;
pub mod pointinterp;
pub mod probe;
pub mod python;
pub mod selection;
pub mod slice;
pub mod sources;
pub mod stats;
pub mod streamlines;
pub mod surface;
pub mod surface_ops;
pub mod synthetic;
pub mod tables;
pub mod temporal;
pub mod temporal_util;
pub mod tensors;
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
