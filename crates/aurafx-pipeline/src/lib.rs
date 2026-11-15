//! The pipeline: a DAG of readers, sources and filters with memoized outputs, display
//! properties per node, a global time, and a work queue so execution can run off-thread.

pub mod animation;
pub mod cad_check;
pub mod cad_edit;
pub mod cases;
pub mod cases_micro;
pub mod charts;
pub mod cluster;
pub mod colormap;
pub mod display;
pub mod pipeline_json;
pub mod remote;
pub mod session;
pub mod state;

pub use animation::{Animation, CameraKey, ParamKey, ParamTrack};
pub use cad_check::{CheckReport, Conflict, ConflictKind, Fix, FixMode, RunMode};
pub use cad_edit::{CadApplied, CadEdit, CadFace, CadSummary};
pub use display::{ColorBy, DisplayProps, DrawOn, GlyphShape, Light, Lighting, MoleculeStyle, RayDevice, Representation, ToneMapping};
pub use session::{
    reader_options_of, reader_param_specs, stat_of, Annotation, AssistantStatus, Chart, ChartSeries, LogEntry, LogLevel, Node, NodeId, NodeKind, RemoteStatus,
    Selection, Session, SharedSession, WorkItem,
};
pub use state::{CameraState, Snapshot};
