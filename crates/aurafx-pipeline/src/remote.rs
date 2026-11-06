//! Client/server: readers and filters run on another machine (like ParaView's pvserver).
//!
//! The client starts `aurafx-server --stdio` on a host (over SSH, or locally) and talks to it
//! through the process's stdin/stdout, so authentication, jump hosts and port forwarding are
//! whatever `ssh` already does. The server holds its own [`Session`]: every request carries the
//! upstream recipe of the node it wants (readers with remote paths, filters with parameters),
//! the server brings its session in line, runs what changed and caches the rest, and sends back
//! the data, only its surface (for big outputs), or only a summary.
//!
//! Messages are a postcard-encoded header plus an optional attachment (a dataset, postcard-encoded
//! and LZ4-compressed): `u32 header length, header, u64 attachment length, attachment`.

use crate::session::{NodeId, Session};
use anyhow::{anyhow, bail, Context, Result};
use aurafx_data::{DataSet, DataSetInfo};
use aurafx_filters::Params;
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::io::{BufReader, BufWriter, Read, Write};
use std::sync::Arc;

pub const PROTOCOL: u32 = 10;

/// One node of the recipe the client sends, keyed by the client's node id.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NodeSpec {
    pub id: NodeId,
    pub kind: SpecKind,
    #[serde(with = "wire_params")]
    pub params: Params,
    pub input: Option<NodeId>,
    pub extra: Vec<NodeId>,
}

/// Parameters on the wire: `ParamValue` is untagged (for readable state files), which binary
/// formats cannot decode, so it travels as a tagged copy.
mod wire_params {
    use aurafx_filters::{Association, ParamValue, Params};
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    #[derive(Serialize, Deserialize)]
    enum Wire {
        Bool(bool),
        Int(i64),
        Float(f64),
        Vec3([f64; 3]),
        Str(String),
        Array(bool, String),
        Floats(Vec<f64>),
    }

    pub fn serialize<S: Serializer>(p: &Params, s: S) -> Result<S::Ok, S::Error> {
        let v: Vec<(&String, Wire)> =
            p.0.iter()
                .map(|(k, v)| {
                    let w = match v {
                        ParamValue::Bool(b) => Wire::Bool(*b),
                        ParamValue::Int(i) => Wire::Int(*i),
                        ParamValue::Float(f) => Wire::Float(*f),
                        ParamValue::Vec3(a) => Wire::Vec3(*a),
                        ParamValue::Str(t) => Wire::Str(t.clone()),
                        ParamValue::Array { assoc, name } => Wire::Array(*assoc == Association::Cells, name.clone()),
                        ParamValue::Floats(f) => Wire::Floats(f.clone()),
                    };
                    (k, w)
                })
                .collect();
        v.serialize(s)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Params, D::Error> {
        let v: Vec<(String, Wire)> = Vec::deserialize(d)?;
        Ok(Params(
            v.into_iter()
                .map(|(k, w)| {
                    let p = match w {
                        Wire::Bool(b) => ParamValue::Bool(b),
                        Wire::Int(i) => ParamValue::Int(i),
                        Wire::Float(f) => ParamValue::Float(f),
                        Wire::Vec3(a) => ParamValue::Vec3(a),
                        Wire::Str(t) => ParamValue::Str(t),
                        Wire::Array(cells, name) => ParamValue::Array { assoc: if cells { Association::Cells } else { Association::Points }, name },
                        Wire::Floats(f) => ParamValue::Floats(f),
                    };
                    (k, p)
                })
                .collect(),
        ))
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum SpecKind {
    /// A file on the server.
    Reader { path: String },
    /// A built-in filter or source.
    Filter { id: String },
}

/// What to send back.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum Fetch {
    /// Only the summary (arrays, ranges, bounds, time steps): the data stays on the server.
    Info,
    /// The outer surface, which is all the 3D view draws.
    Surface,
    /// Everything.
    Full,
    /// Everything when it is smaller than the limit, else the surface, or (when even the
    /// surface is too big) nothing: the server draws it.
    #[default]
    Auto,
    /// The summary only; the server draws the object for the 3D views.
    Draw,
}

/// What a reply carries.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum Payload {
    #[default]
    None,
    Surface,
    Full,
    /// Kept on the server, which draws it (too big to send, or asked for).
    Drawn,
}

/// The camera of a view, exactly as the client renders it (so depths agree).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, Default)]
pub struct RenderCamera {
    pub eye: [f64; 3],
    pub target: [f64; 3],
    pub up: [f64; 3],
    pub fov_y_deg: f64,
    pub orthographic: bool,
    pub ortho_height: f64,
    pub near: f64,
    pub far: f64,
}

/// What a server draws: these nodes (with their display settings) seen by this camera.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RenderJob {
    pub targets: Vec<(NodeId, crate::DisplayProps)>,
    pub camera: RenderCamera,
    pub width: u32,
    pub height: u32,
    /// Lights and shadows of the view (`Lighting::for_server`).
    pub lighting: crate::Lighting,
}

/// Rendering on the server, supplied by the server program (the pipeline crate has no GPU
/// code). Returns sRGB RGBA8 color and depth (1.0 = empty), row by row from the top.
pub trait RenderHook: Send {
    fn render(&mut self, outputs: &[(NodeId, std::sync::Arc<DataSet>, crate::DisplayProps)], job: &RenderJob) -> Result<(Vec<u8>, Vec<f32>)>;
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Request {
    Hello {
        protocol: u32,
    },
    Compute {
        specs: Vec<NodeSpec>,
        target: NodeId,
        time: Option<f64>,
        fetch: Fetch,
        max_bytes: u64,
        surface_level: u32,
        max_surface_bytes: u64,
    },
    /// Draw nodes (their recipes in `specs`) and return color + depth images.
    Render {
        specs: Vec<NodeSpec>,
        time: Option<f64>,
        job: RenderJob,
    },
    /// The client removed these nodes.
    Forget {
        ids: Vec<NodeId>,
    },
    Shutdown,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerInfo {
    pub protocol: u32,
    pub version: String,
    pub hostname: String,
    pub cpus: usize,
    /// The GPU the server draws with, if it can draw.
    #[serde(default)]
    pub gpu: Option<String>,
    /// Processes the data is spread over (1 for an ordinary server).
    #[serde(default)]
    pub processes: usize,
}

/// How a server process runs.
#[derive(Default)]
pub struct ServeOptions {
    /// Draws on this machine's GPU.
    pub hook: Option<Box<dyn RenderHook>>,
    pub gpu: Option<String>,
    /// A worker of a parallel server: the piece (k, n) of every file it reads.
    pub piece: Option<(usize, usize)>,
    /// A parallel server's coordinator: its workers.
    pub workers: Vec<Arc<RemoteClient>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Reply {
    Hello(ServerInfo),
    /// `info` describes the full output on the server; the attachment is the payload.
    Data {
        info: Option<DataSetInfo>,
        times: Vec<f64>,
        payload: Payload,
        server_ms: f64,
        full_bytes: u64,
        /// The options each reader in the request offers for its file (read on the server).
        reader_options: Vec<(NodeId, Vec<aurafx_io::ReaderOption>)>,
    },
    /// The attachment is LZ4 color (first `color_bytes`) followed by LZ4 depth.
    Image {
        width: u32,
        height: u32,
        server_ms: f64,
        color_bytes: u64,
    },
    Ok,
    Error {
        message: String,
    },
}

// ---------------------------------------------------------------- framing

fn write_msg<T: Serialize>(w: &mut impl Write, msg: &T, attachment: &[u8]) -> Result<()> {
    let head = postcard::to_allocvec(msg)?;
    w.write_all(&(head.len() as u32).to_le_bytes())?;
    w.write_all(&head)?;
    w.write_all(&(attachment.len() as u64).to_le_bytes())?;
    w.write_all(attachment)?;
    w.flush()?;
    Ok(())
}

/// Read one frame: (header bytes, attachment). An error here means the stream is unusable.
fn read_frame(r: &mut impl Read) -> Result<(Vec<u8>, Vec<u8>)> {
    let mut b4 = [0u8; 4];
    r.read_exact(&mut b4).context("connection closed")?;
    let n = u32::from_le_bytes(b4) as usize;
    if n > 64 << 20 {
        bail!("bad message header ({n} bytes): is something else printing on the server's output (e.g. a login script)?");
    }
    let mut head = vec![0u8; n];
    r.read_exact(&mut head)?;
    let mut b8 = [0u8; 8];
    r.read_exact(&mut b8)?;
    let m = u64::from_le_bytes(b8) as usize;
    let mut att = vec![0u8; m];
    r.read_exact(&mut att)?;
    Ok((head, att))
}

fn read_msg<T: for<'de> Deserialize<'de>>(r: &mut impl Read) -> Result<(T, Vec<u8>)> {
    let (head, att) = read_frame(r)?;
    Ok((postcard::from_bytes(&head).context("bad message (client and server versions differ?)")?, att))
}

/// Encode a dataset for the wire.
pub fn encode(ds: &DataSet) -> Result<Vec<u8>> {
    let raw = postcard::to_allocvec(ds)?;
    Ok(lz4_flex::compress_prepend_size(&raw))
}

pub fn decode(bytes: &[u8]) -> Result<DataSet> {
    let raw = lz4_flex::decompress_size_prepended(bytes).map_err(|e| anyhow!("decompressing data: {e}"))?;
    Ok(postcard::from_bytes(&raw)?)
}

// ---------------------------------------------------------------- server

/// The server side: answer requests from `input` on `output` until the client goes away.
pub fn serve(input: impl Read, output: impl Write) -> Result<()> {
    serve_with(input, output, None, None)
}

/// [`serve`] with a renderer for drawing on the server (`gpu` names it for the client).
pub fn serve_with(input: impl Read, output: impl Write, hook: Option<Box<dyn RenderHook>>, gpu: Option<String>) -> Result<()> {
    serve_opts(input, output, ServeOptions { hook, gpu, ..Default::default() })
}

/// [`serve`] as a renderer, a worker holding one piece, or a coordinator of workers.
pub fn serve_opts(input: impl Read, output: impl Write, opts: ServeOptions) -> Result<()> {
    let mut r = BufReader::new(input);
    let mut w = BufWriter::new(output);
    let piece = opts.piece;
    let cluster = (!opts.workers.is_empty()).then(|| crate::cluster::Cluster::new(opts.workers));
    let mut srv = Server { hook: opts.hook, gpu: opts.gpu, cluster, ..Server::default() };
    srv.session.piece = piece;
    loop {
        let Ok((head, _)) = read_frame(&mut r) else { return Ok(()) }; // client went away
        let req: Request = match postcard::from_bytes(&head) {
            Ok(m) => m,
            Err(e) => {
                write_msg(&mut w, &Reply::Error { message: format!("could not read the request: {e}") }, &[])?;
                continue;
            }
        };
        let (reply, att) = match req {
            Request::Shutdown => {
                write_msg(&mut w, &Reply::Ok, &[])?;
                return Ok(());
            }
            other => match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| srv.handle(other))) {
                Ok(r) => r,
                Err(p) => {
                    let msg =
                        p.downcast_ref::<String>().cloned().or_else(|| p.downcast_ref::<&str>().map(|s| s.to_string())).unwrap_or_else(|| "unknown".into());
                    // The session may be half-updated: start over (the next request resends everything).
                    srv = Server { hook: srv.hook.take(), gpu: srv.gpu.clone(), cluster: srv.cluster.take(), ..Server::default() };
                    srv.session.piece = piece;
                    if let Some(c) = srv.cluster.as_mut() {
                        c.gathered.clear();
                        c.infos.lock().clear();
                    }
                    (Reply::Error { message: format!("the server failed: {msg}") }, vec![])
                }
            },
        };
        write_msg(&mut w, &reply, &att)?;
    }
}

struct Server {
    session: Session,
    /// Client node id -> (server node id, the spec it was built from).
    nodes: HashMap<NodeId, (NodeId, NodeSpec)>,
    hook: Option<Box<dyn RenderHook>>,
    gpu: Option<String>,
    /// Set on the coordinator of a parallel server.
    cluster: Option<crate::cluster::Cluster>,
    /// Reader options per file path.
    option_cache: HashMap<String, Vec<aurafx_io::ReaderOption>>,
}

impl Default for Server {
    fn default() -> Self {
        let mut session = Session::new();
        session.suspend_undo = 1;
        Self { session, nodes: HashMap::new(), hook: None, gpu: None, cluster: None, option_cache: HashMap::new() }
    }
}

impl Server {
    fn handle(&mut self, req: Request) -> (Reply, Vec<u8>) {
        match req {
            Request::Hello { .. } => (
                Reply::Hello(ServerInfo {
                    protocol: PROTOCOL,
                    version: env!("CARGO_PKG_VERSION").to_string(),
                    hostname: hostname(),
                    cpus: std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1),
                    gpu: match &self.cluster {
                        Some(c) if c.can_draw() => c.workers[0].info.lock().as_ref().and_then(|i| i.gpu.clone()),
                        Some(_) => None,
                        None => self.gpu.clone(),
                    },
                    processes: self.cluster.as_ref().map(|c| c.workers.len()).unwrap_or(1),
                }),
                vec![],
            ),
            Request::Forget { ids } => {
                if let Some(c) = &self.cluster {
                    let _ = c.each(|w| w.request(&Request::Forget { ids: ids.clone() }).map(|_| ()));
                    for id in &ids {
                        if let Some(c) = self.cluster.as_mut() {
                            c.gathered.remove(id);
                        }
                    }
                }
                for id in ids {
                    if let Some((sid, _)) = self.nodes.remove(&id) {
                        self.session.remove_node(sid);
                    }
                }
                (Reply::Ok, vec![])
            }
            Request::Compute { specs, target, time, fetch, max_bytes, surface_level, max_surface_bytes } => {
                match self.compute(&specs, target, time, fetch, max_bytes, surface_level, max_surface_bytes) {
                    Ok((Reply::Data { info, times, payload, server_ms, full_bytes, .. }, att)) => {
                        let reader_options = self.reader_options(&specs);
                        (Reply::Data { info, times, payload, server_ms, full_bytes, reader_options }, att)
                    }
                    Ok(r) => r,
                    Err(e) => (Reply::Error { message: format!("{e:#}") }, vec![]),
                }
            }
            Request::Render { specs, time, job } => match self.render(&specs, time, &job) {
                Ok(r) => r,
                Err(e) => (Reply::Error { message: format!("{e:#}") }, vec![]),
            },
            Request::Shutdown => (Reply::Ok, vec![]),
        }
    }

    fn reconcile(&mut self, specs: &[NodeSpec]) -> Result<()> {
        for spec in specs {
            if let Some((sid, old)) = self.nodes.get(&spec.id) {
                if old == spec {
                    continue;
                }
                let same_shape = old.kind == spec.kind && old.input == spec.input && old.extra == spec.extra;
                let sid = *sid;
                if same_shape && self.session.nodes.contains_key(&sid) {
                    self.session.set_params(sid, spec.params.clone());
                    self.nodes.get_mut(&spec.id).unwrap().1 = spec.clone();
                    continue;
                }
                self.session.remove_node(sid);
                self.nodes.remove(&spec.id);
            }
            let map = |id: NodeId, nodes: &HashMap<NodeId, (NodeId, NodeSpec)>| nodes.get(&id).map(|n| n.0).ok_or_else(|| anyhow!("input {id} was not sent"));
            let sid = match &spec.kind {
                SpecKind::Reader { path } => self.session.open_file(expand_home(path))?,
                SpecKind::Filter { id } => {
                    let def = aurafx_filters::find(id).ok_or_else(|| anyhow!("the server has no filter {id:?}"))?;
                    if def.is_source {
                        self.session.add_source(id)?
                    } else {
                        let input = map(spec.input.ok_or_else(|| anyhow!("{id} needs an input"))?, &self.nodes)?;
                        if spec.extra.is_empty() {
                            self.session.add_filter(id, input)?
                        } else {
                            let mut all = vec![input];
                            for e in &spec.extra {
                                all.push(map(*e, &self.nodes)?);
                            }
                            self.session.add_filter_multi(id, &all)?
                        }
                    }
                }
            };
            self.session.set_params(sid, spec.params.clone());
            self.nodes.insert(spec.id, (sid, spec.clone()));
        }
        Ok(())
    }

    fn render(&mut self, specs: &[NodeSpec], time: Option<f64>, job: &RenderJob) -> Result<(Reply, Vec<u8>)> {
        let t0 = std::time::Instant::now();
        if self.cluster.is_some() {
            let local = crate::cluster::piece_local(specs);
            let (mine, theirs): (Vec<_>, Vec<_>) = job.targets.iter().cloned().partition(|(id, _)| local.contains(id));
            let mut image: Option<(Vec<u8>, Vec<f32>)> = None;
            if !mine.is_empty() {
                let sub: Vec<NodeSpec> = specs.iter().filter(|s| local.contains(&s.id)).cloned().collect();
                let j = RenderJob { targets: mine, ..job.clone() };
                image = Some(self.cluster.as_ref().unwrap().render_local(&sub, time, &j)?);
            }
            if !theirs.is_empty() {
                // Drawn here from the gathered data.
                self.prepare_gathered(specs, &local, time)?;
                let j = RenderJob { targets: theirs, ..job.clone() };
                let (c, d) = self.render_here(time, &j)?;
                match image.as_mut() {
                    Some((ic, id)) => crate::cluster::composite(ic, id, &c, &d),
                    None => image = Some((c, d)),
                }
            }
            let (color, depth) = image.unwrap_or_default();
            return Ok(image_reply(job, color, depth, t0));
        }
        if self.hook.is_none() {
            bail!("this server cannot draw (no GPU, or built without rendering)");
        }
        self.reconcile(specs)?;
        if let Some(t) = time {
            self.session.set_time_value(t);
        }
        self.session.update_blocking();
        let mut outputs = Vec::new();
        for (id, display) in &job.targets {
            let sid = self.nodes.get(id).ok_or_else(|| anyhow!("node {id} was not sent"))?.0;
            let n = self.session.nodes.get(&sid).ok_or_else(|| anyhow!("node {id} is gone"))?;
            if let Some(out) = &n.output {
                outputs.push((*id, out.clone(), display.clone()));
            }
        }
        let (color, depth) = self.hook.as_mut().unwrap().render(&outputs, job)?;
        Ok(image_reply(job, color, depth, t0))
    }

    /// Draw nodes already in this server's session.
    fn render_here(&mut self, time: Option<f64>, job: &RenderJob) -> Result<(Vec<u8>, Vec<f32>)> {
        let hook = self.hook.as_mut().ok_or_else(|| anyhow!("this server cannot draw (no GPU)"))?;
        if let Some(t) = time {
            self.session.set_time_value(t);
        }
        self.session.update_blocking();
        let mut outputs = Vec::new();
        for (id, display) in &job.targets {
            let sid = self.nodes.get(id).ok_or_else(|| anyhow!("node {id} was not sent"))?.0;
            if let Some(out) = self.session.nodes.get(&sid).and_then(|n| n.output.clone()) {
                outputs.push((*id, out, display.clone()));
            }
        }
        hook.render(&outputs, job)
    }

    /// On a coordinator: make the nodes that are not piece-local available in this server's
    /// session, fed by the gathered outputs of the piece-local nodes they read.
    fn prepare_gathered(&mut self, specs: &[NodeSpec], local: &HashSet<NodeId>, time: Option<f64>) -> Result<()> {
        for s in specs.iter().filter(|s| !local.contains(&s.id)) {
            if let SpecKind::Filter { id } = &s.kind {
                if aurafx_filters::find(id).is_some_and(|d| d.temporal.is_some()) {
                    bail!("{id} needs every time step and is not available on a parallel server yet; use one server process");
                }
            }
        }
        let sub: Vec<NodeSpec> = specs.iter().filter(|s| local.contains(&s.id)).cloned().collect();
        let boundary: Vec<NodeId> =
            sub.iter().map(|s| s.id).filter(|id| specs.iter().any(|s| !local.contains(&s.id) && (s.input == Some(*id) || s.extra.contains(id)))).collect();
        let mut changed = false;
        for b in boundary {
            let fp = crate::cluster::fingerprint(&sub, time);
            let cluster = self.cluster.as_mut().unwrap();
            if cluster.gathered.get(&b) == Some(&fp) && self.nodes.contains_key(&b) {
                continue;
            }
            let (data, _) = cluster.gather(&sub, b, time)?;
            cluster.gathered.insert(b, fp);
            if let Some((old, _)) = self.nodes.remove(&b) {
                self.session.remove_node(old);
            }
            let sid = self.session.add_data("gathered", data);
            let spec = sub.iter().find(|s| s.id == b).cloned().unwrap();
            self.nodes.insert(b, (sid, spec));
            changed = true;
        }
        let rest: Vec<NodeSpec> = specs.iter().filter(|s| !local.contains(&s.id)).cloned().collect();
        if changed {
            // Rebuild the nodes below the replaced inputs.
            for s in &rest {
                if let Some((sid, _)) = self.nodes.remove(&s.id) {
                    self.session.remove_node(sid);
                }
            }
        }
        self.reconcile(&rest)
    }

    /// The options of the reader specs' files, looked up once per file.
    fn reader_options(&mut self, specs: &[NodeSpec]) -> Vec<(NodeId, Vec<aurafx_io::ReaderOption>)> {
        specs
            .iter()
            .filter_map(|s| match &s.kind {
                SpecKind::Reader { path } => {
                    let o = self.option_cache.entry(path.clone()).or_insert_with(|| aurafx_io::reader_options(expand_home(path))).clone();
                    Some((s.id, o))
                }
                _ => None,
            })
            .collect()
    }

    #[allow(clippy::too_many_arguments)]
    fn compute(
        &mut self,
        specs: &[NodeSpec],
        target: NodeId,
        time: Option<f64>,
        fetch: Fetch,
        max_bytes: u64,
        level: u32,
        max_surface: u64,
    ) -> Result<(Reply, Vec<u8>)> {
        let t0 = std::time::Instant::now();
        if let Some(cluster) = &self.cluster {
            let local = crate::cluster::piece_local(specs);
            if local.contains(&target) {
                let sub: Vec<NodeSpec> = specs.iter().filter(|s| local.contains(&s.id)).cloned().collect();
                return cluster.compute_local(&sub, target, time, fetch, max_bytes, level, max_surface);
            }
            self.prepare_gathered(specs, &local, time)?;
        } else {
            self.reconcile(specs)?;
        }
        if let Some(t) = time {
            self.session.set_time_value(t);
        }
        self.session.update_blocking();
        let sid = self.nodes.get(&target).ok_or_else(|| anyhow!("node {target} was not sent"))?.0;
        let n = self.session.nodes.get(&sid).ok_or_else(|| anyhow!("node {target} is gone"))?;
        if let Some(e) = &n.error {
            bail!("{e}");
        }
        let out = n.output.clone().ok_or_else(|| anyhow!("no output"))?;
        let full_bytes = out.n_bytes() as u64;
        let times = self.session.node_times(sid, 0);
        let surface_useful = !matches!(&*out, DataSet::PolyData(_) | DataSet::Table(_));
        let can_draw = self.hook.is_some();
        let mut surface = None;
        let payload = match fetch {
            Fetch::Info => Payload::None,
            Fetch::Draw if can_draw => Payload::Drawn,
            Fetch::Draw => Payload::Surface,
            Fetch::Full => Payload::Full,
            Fetch::Surface if surface_useful => Payload::Surface,
            Fetch::Surface => Payload::Full,
            Fetch::Auto if full_bytes <= max_bytes || !surface_useful => Payload::Full,
            Fetch::Auto => {
                let s = DataSet::PolyData(aurafx_filters::surface::extract_surface_level(&out, level)?);
                let big = s.n_bytes() as u64 > max_surface;
                surface = Some(s);
                if big && can_draw {
                    Payload::Drawn
                } else {
                    Payload::Surface
                }
            }
        };
        let att = match payload {
            Payload::None | Payload::Drawn => vec![],
            Payload::Full => encode(&out)?,
            Payload::Surface => match surface {
                Some(s) => encode(&s)?,
                None => encode(&DataSet::PolyData(aurafx_filters::surface::extract_surface_level(&out, level)?))?,
            },
        };
        let info = n.info.clone().or_else(|| Some(DataSetInfo::of(&out)));
        Ok((Reply::Data { info, times, payload, server_ms: t0.elapsed().as_secs_f64() * 1e3, full_bytes, reader_options: Vec::new() }, att))
    }
}

fn image_reply(job: &RenderJob, color: Vec<u8>, depth: Vec<f32>, t0: std::time::Instant) -> (Reply, Vec<u8>) {
    let mut att = lz4_flex::compress_prepend_size(&color);
    let color_bytes = att.len() as u64;
    let raw: Vec<u8> = depth.iter().flat_map(|d| d.to_le_bytes()).collect();
    att.extend(lz4_flex::compress_prepend_size(&raw));
    (Reply::Image { width: job.width, height: job.height, server_ms: t0.elapsed().as_secs_f64() * 1e3, color_bytes }, att)
}

fn expand_home(p: &str) -> String {
    match p.strip_prefix("~/").or(if p == "~" { Some("") } else { None }) {
        Some(rest) => match std::env::var("HOME").or_else(|_| std::env::var("USERPROFILE")) {
            Ok(h) => format!("{}/{rest}", h.trim_end_matches('/')),
            Err(_) => p.to_string(),
        },
        None => p.to_string(),
    }
}

fn hostname() -> String {
    std::fs::read_to_string("/etc/hostname")
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .or_else(|| std::env::var("HOSTNAME").ok())
        .or_else(|| std::env::var("COMPUTERNAME").ok())
        .unwrap_or_else(|| "server".into())
}

// ---------------------------------------------------------------- client

/// Result of a remote computation.
#[derive(Debug, Clone)]
pub struct Fetched {
    pub data: DataSet,
    pub info: Option<DataSetInfo>,
    pub times: Vec<f64>,
    pub payload: Payload,
    pub server_ms: f64,
    pub full_bytes: u64,
    pub sent_bytes: u64,
    /// Reader options of the readers in the request (by client node id).
    pub reader_options: Vec<(NodeId, Vec<aurafx_io::ReaderOption>)>,
}

struct Conn {
    child: Option<std::process::Child>,
    r: Box<dyn Read + Send>,
    w: Box<dyn Write + Send>,
}

type Spawner = Box<dyn Fn() -> Result<(Conn, Arc<Mutex<String>>)> + Send + Sync>;

/// A connection to one server, started on first use and restarted if it drops. Requests are
/// served one at a time.
pub struct RemoteClient {
    pub name: String,
    conn: Mutex<Option<Conn>>,
    spawn: Spawner,
    stderr: Mutex<Arc<Mutex<String>>>,
    pub info: Mutex<Option<ServerInfo>>,
}

impl std::fmt::Debug for RemoteClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "RemoteClient({})", self.name)
    }
}

/// Name under which a session keeps its connection to a host (a parallel server on the same
/// host is a different connection).
pub fn client_key(h: &aurafx_jobs::Host) -> String {
    if h.server_processes > 1 {
        format!("{} ×{}", h.name, h.server_processes)
    } else {
        h.name.clone()
    }
}

/// Shell command that starts the server on a host: `$AURAFX_SERVER`, `aurafx-server` on the
/// PATH, or `~/.local/bin/aurafx-server`.
pub const SERVER_COMMAND: &str = r#"S="${AURAFX_SERVER:-$(command -v aurafx-server || echo "$HOME/.local/bin/aurafx-server")}"; exec "$S" --stdio"#;

impl RemoteClient {
    /// A server started on `host` (through ssh for remote hosts).
    pub fn for_host(host: aurafx_jobs::Host) -> Arc<Self> {
        let name = host.name.clone();
        let spawn: Spawner = Box::new(move || {
            let cmd = if host.server_processes > 1 {
                SERVER_COMMAND.replace("--stdio", &format!("--stdio --workers {}", host.server_processes))
            } else {
                SERVER_COMMAND.to_string()
            };
            let mut c = host.command(&cmd);
            // On this machine, use the server installed next to the app.
            if host.is_local() && std::env::var_os("AURAFX_SERVER").is_none() {
                if let Some(p) = std::env::current_exe()
                    .ok()
                    .map(|e| e.with_file_name(if cfg!(windows) { "aurafx-server.exe" } else { "aurafx-server" }))
                    .filter(|p| p.exists())
                {
                    c.env("AURAFX_SERVER", p);
                }
            }
            c.stdin(std::process::Stdio::piped()).stdout(std::process::Stdio::piped()).stderr(std::process::Stdio::piped());
            let mut child = c.spawn().with_context(|| format!("starting aurafx-server on {}", host.name))?;
            let r = child.stdout.take().unwrap();
            let w = child.stdin.take().unwrap();
            let err = Arc::new(Mutex::new(String::new()));
            if let Some(mut e) = child.stderr.take() {
                let err = err.clone();
                std::thread::spawn(move || {
                    let mut buf = [0u8; 4096];
                    while let Ok(n) = e.read(&mut buf) {
                        if n == 0 {
                            break;
                        }
                        let mut s = err.lock();
                        s.push_str(&String::from_utf8_lossy(&buf[..n]));
                        if s.len() > 8192 {
                            let cut = s.len() - 8192;
                            s.drain(..cut);
                        }
                    }
                });
            }
            Ok((Conn { child: Some(child), r: Box::new(r), w: Box::new(BufWriter::new(w)) }, err))
        });
        Arc::new(Self { name, conn: Mutex::new(None), spawn, stderr: Mutex::new(Arc::default()), info: Mutex::new(None) })
    }

    /// A server process started by `make` (with stdin/stdout piped), restarted when needed.
    pub fn from_command(name: &str, make: impl Fn() -> std::process::Command + Send + Sync + 'static) -> Arc<Self> {
        let label = name.to_string();
        let spawn: Spawner = Box::new(move || {
            let mut c = make();
            c.stdin(std::process::Stdio::piped()).stdout(std::process::Stdio::piped()).stderr(std::process::Stdio::piped());
            let mut child = c.spawn().with_context(|| format!("starting {label}"))?;
            let r = child.stdout.take().unwrap();
            let w = child.stdin.take().unwrap();
            let err = Arc::new(Mutex::new(String::new()));
            if let Some(mut e) = child.stderr.take() {
                let err = err.clone();
                std::thread::spawn(move || {
                    let mut buf = [0u8; 4096];
                    while let Ok(n) = e.read(&mut buf) {
                        if n == 0 {
                            break;
                        }
                        let mut s = err.lock();
                        s.push_str(&String::from_utf8_lossy(&buf[..n]));
                        if s.len() > 8192 {
                            let cut = s.len() - 8192;
                            s.drain(..cut);
                        }
                    }
                });
            }
            Ok((Conn { child: Some(child), r: Box::new(r), w: Box::new(BufWriter::new(w)) }, err))
        });
        Arc::new(Self { name: name.to_string(), conn: Mutex::new(None), spawn, stderr: Mutex::new(Arc::default()), info: Mutex::new(None) })
    }

    /// A server running on threads of this process with these options (tests).
    pub fn in_process_opts(name: &str, opts: ServeOptions) -> Arc<Self> {
        let (c2s_r, c2s_w) = pipe();
        let (s2c_r, s2c_w) = pipe();
        std::thread::spawn(move || {
            let _ = serve_opts(c2s_r, s2c_w, opts);
        });
        Self::from_streams(name, s2c_r, c2s_w)
    }

    /// A client over existing streams (tests, or a server in the same process).
    pub fn from_streams(name: &str, r: impl Read + Send + 'static, w: impl Write + Send + 'static) -> Arc<Self> {
        let cell = Mutex::new(Some((Box::new(r) as Box<dyn Read + Send>, Box::new(w) as Box<dyn Write + Send>)));
        let spawn: Spawner = Box::new(move || {
            let (r, w) = cell.lock().take().ok_or_else(|| anyhow!("the connection was closed"))?;
            Ok((Conn { child: None, r, w }, Arc::default()))
        });
        Arc::new(Self { name: name.to_string(), conn: Mutex::new(None), spawn, stderr: Mutex::new(Arc::default()), info: Mutex::new(None) })
    }

    /// A server running on threads of this process (for trying client/server locally).
    pub fn in_process(name: &str) -> Arc<Self> {
        Self::in_process_with(name, None, None)
    }

    /// [`in_process`](Self::in_process) with a renderer, so the server can draw.
    pub fn in_process_with(name: &str, hook: Option<Box<dyn RenderHook>>, gpu: Option<String>) -> Arc<Self> {
        let (c2s_r, c2s_w) = pipe();
        let (s2c_r, s2c_w) = pipe();
        std::thread::spawn(move || {
            let _ = serve_with(c2s_r, s2c_w, hook, gpu);
        });
        Self::from_streams(name, s2c_r, c2s_w)
    }

    pub fn is_connected(&self) -> bool {
        self.conn.lock().is_some()
    }

    fn connect(&self) -> Result<Conn> {
        let (mut conn, err) = (self.spawn)()?;
        *self.stderr.lock() = err.clone();
        let hello = (|| -> Result<ServerInfo> {
            write_msg(&mut conn.w, &Request::Hello { protocol: PROTOCOL }, &[])?;
            match read_msg::<Reply>(&mut conn.r)?.0 {
                Reply::Hello(info) if info.protocol == PROTOCOL => Ok(info),
                Reply::Hello(info) => bail!("server speaks protocol {} (this auraFX: {PROTOCOL}); update aurafx-server", info.protocol),
                other => bail!("unexpected greeting {other:?}"),
            }
        })();
        match hello {
            Ok(info) => {
                *self.info.lock() = Some(info);
                Ok(conn)
            }
            Err(e) => {
                if let Some(mut ch) = conn.child.take() {
                    let _ = ch.kill();
                    let _ = ch.wait();
                }
                std::thread::sleep(std::time::Duration::from_millis(100));
                let msg = err.lock().trim().to_string();
                if msg.is_empty() {
                    Err(e.context(format!("could not start aurafx-server on {}", self.name)))
                } else {
                    Err(anyhow!("could not start aurafx-server on {}: {msg}", self.name))
                }
            }
        }
    }

    /// Send a request and wait for the reply, connecting (or reconnecting once) as needed.
    pub fn request(&self, req: &Request) -> Result<(Reply, Vec<u8>)> {
        let mut guard = self.conn.lock();
        let mut first: Option<anyhow::Error> = None;
        for _ in 0..2 {
            if guard.is_none() {
                match self.connect() {
                    Ok(c) => *guard = Some(c),
                    Err(e) => return Err(first.map(|f| f.context(format!("{e:#}"))).unwrap_or(e)),
                }
            }
            let conn = guard.as_mut().unwrap();
            let res = write_msg(&mut conn.w, req, &[]).and_then(|_| read_msg::<Reply>(&mut conn.r));
            match res {
                Ok(r) => return Ok(r),
                Err(e) => {
                    if let Some(mut ch) = guard.take().and_then(|c| c.child) {
                        let _ = ch.kill();
                        let _ = ch.wait();
                    }
                    let err = self.stderr.lock().lock().trim().to_string();
                    let e = if err.is_empty() { e } else { e.context(err) };
                    if first.is_some() {
                        return Err(e);
                    }
                    first = Some(e);
                }
            }
        }
        Err(first.unwrap_or_else(|| anyhow!("no connection")))
    }

    /// Run `target` (with its upstream `specs`) on the server.
    #[allow(clippy::too_many_arguments)]
    pub fn compute(
        &self,
        specs: Vec<NodeSpec>,
        target: NodeId,
        time: Option<f64>,
        fetch: Fetch,
        max_bytes: u64,
        surface_level: u32,
        max_surface_bytes: u64,
    ) -> Result<Fetched> {
        let (reply, att) = self.request(&Request::Compute { specs, target, time, fetch, max_bytes, surface_level, max_surface_bytes })?;
        match reply {
            Reply::Data { info, times, payload, server_ms, full_bytes, reader_options } => {
                let data = if att.is_empty() { DataSet::PolyData(Default::default()) } else { decode(&att)? };
                Ok(Fetched { data, info, times, payload, server_ms, full_bytes, sent_bytes: att.len() as u64, reader_options })
            }
            Reply::Error { message } => Err(anyhow!("{message}")),
            other => bail!("unexpected reply {other:?}"),
        }
    }

    /// Have the server draw nodes; returns (width, height, sRGB color, depth, server ms).
    pub fn render(&self, specs: Vec<NodeSpec>, time: Option<f64>, job: RenderJob) -> Result<(u32, u32, Vec<u8>, Vec<f32>, f64)> {
        match self.request(&Request::Render { specs, time, job })? {
            (Reply::Image { width, height, server_ms, color_bytes }, att) => {
                let k = (color_bytes as usize).min(att.len());
                let color = lz4_flex::decompress_size_prepended(&att[..k]).map_err(|e| anyhow!("decompressing image: {e}"))?;
                let raw = lz4_flex::decompress_size_prepended(&att[k..]).map_err(|e| anyhow!("decompressing depth: {e}"))?;
                let depth = raw.chunks_exact(4).map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]])).collect();
                Ok((width, height, color, depth, server_ms))
            }
            (Reply::Error { message }, _) => Err(anyhow!("{message}")),
            (other, _) => bail!("unexpected reply {other:?}"),
        }
    }

    /// Tell a connected server to drop nodes (does nothing when not connected).
    pub fn forget(self: &Arc<Self>, ids: Vec<NodeId>) {
        if !self.is_connected() || ids.is_empty() {
            return;
        }
        let me = self.clone();
        std::thread::spawn(move || {
            let _ = me.request(&Request::Forget { ids });
        });
    }
}

impl Drop for RemoteClient {
    fn drop(&mut self) {
        if let Some(mut c) = self.conn.lock().take() {
            let _ = write_msg(&mut c.w, &Request::Shutdown, &[]);
            if let Some(mut ch) = c.child {
                std::thread::spawn(move || {
                    std::thread::sleep(std::time::Duration::from_millis(500));
                    let _ = ch.kill();
                    let _ = ch.wait();
                });
            }
        }
    }
}

/// An in-memory byte pipe (reader, writer).
fn pipe() -> (PipeReader, PipeWriter) {
    let (tx, rx) = std::sync::mpsc::channel::<Vec<u8>>();
    (PipeReader { rx, buf: Vec::new(), pos: 0 }, PipeWriter { tx, buf: Vec::new() })
}

struct PipeReader {
    rx: std::sync::mpsc::Receiver<Vec<u8>>,
    buf: Vec<u8>,
    pos: usize,
}

impl Read for PipeReader {
    fn read(&mut self, out: &mut [u8]) -> std::io::Result<usize> {
        while self.pos >= self.buf.len() {
            match self.rx.recv() {
                Ok(b) => {
                    self.buf = b;
                    self.pos = 0;
                }
                Err(_) => return Ok(0),
            }
        }
        let n = out.len().min(self.buf.len() - self.pos);
        out[..n].copy_from_slice(&self.buf[self.pos..self.pos + n]);
        self.pos += n;
        Ok(n)
    }
}

struct PipeWriter {
    tx: std::sync::mpsc::Sender<Vec<u8>>,
    buf: Vec<u8>,
}

impl Write for PipeWriter {
    fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
        self.buf.extend_from_slice(b);
        Ok(b.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        if !self.buf.is_empty() {
            self.tx.send(std::mem::take(&mut self.buf)).map_err(|_| std::io::Error::from(std::io::ErrorKind::BrokenPipe))?;
        }
        Ok(())
    }
}
