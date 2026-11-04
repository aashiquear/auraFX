//! The pipeline: a DAG of readers, sources and filters with memoized outputs, display
//! properties per node, a global time, and a work queue so execution can run off-thread.

pub mod colormap;
pub mod display;
pub mod session;

pub use display::{ColorBy, DisplayProps, Representation};
pub use session::{stat_of, Chart, ChartSeries, LogEntry, LogLevel, Node, NodeId, NodeKind, Session, SharedSession, WorkItem};
