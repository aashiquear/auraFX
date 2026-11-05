//! The pipeline: a DAG of readers, sources and filters with memoized outputs, display
//! properties per node, a global time, and a work queue so execution can run off-thread.

pub mod animation;
pub mod colormap;
pub mod display;
pub mod remote;
pub mod session;
pub mod state;

pub use animation::{Animation, CameraKey, ParamKey, ParamTrack};
pub use display::{ColorBy, DisplayProps, Representation};
pub use session::{
    stat_of, Annotation, Chart, ChartSeries, LogEntry, LogLevel, Node, NodeId, NodeKind, RemoteStatus, Selection, Session, SharedSession, WorkItem,
};
pub use state::{CameraState, Snapshot};
