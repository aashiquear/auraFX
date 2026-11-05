//! auraFX data model.
//!
//! Column-oriented, VTK-compatible dataset types. Everything here is plain data with no
//! rendering or I/O dependencies so it can be shared between the pipeline, the Python
//! bindings, readers, filters and the renderer.

pub mod array;
pub mod attributes;
pub mod bounds;
pub mod cell;
pub mod dataset;
pub mod higher_order;
pub mod info;
pub mod pieces;

pub use array::{Array, ArrayData, ScalarType};
pub use attributes::{AttributeKind, Attributes};
pub use bounds::Bounds;
pub use cell::{CellType, Cells};
pub use dataset::{DataSet, ImageData, MultiBlock, Points, PolyData, RectilinearGrid, StructuredGrid, Table, TimeSeries, TimeStepRef, UnstructuredGrid};
pub use info::DataSetInfo;
