//! auraFX data model.
//!
//! Column-oriented, VTK-compatible dataset types. Everything here is plain data with no
//! rendering or I/O dependencies so it can be shared between the pipeline, the Python
//! bindings, readers, filters and the renderer.

pub mod array;
pub mod attributes;
pub mod cell;
pub mod dataset;
pub mod bounds;
pub mod info;

pub use array::{Array, ArrayData, ScalarType};
pub use attributes::{Attributes, AttributeKind};
pub use bounds::Bounds;
pub use cell::{CellType, Cells};
pub use dataset::{
    DataSet, ImageData, MultiBlock, PolyData, Points, RectilinearGrid, StructuredGrid, Table,
    TimeSeries, TimeStepRef, UnstructuredGrid,
};
pub use info::DataSetInfo;
