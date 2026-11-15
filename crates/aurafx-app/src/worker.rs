//! Background execution of the pipeline and of Python code.

use aurafx_pipeline::SharedSession;
#[cfg(feature = "python")]
use aurafx_python::PythonRuntime;
#[cfg(feature = "python")]
pub use aurafx_python::RunOutput;

/// Output of a Python run (mirrors `aurafx_python::RunOutput` so the UI compiles without Python).
#[cfg(not(feature = "python"))]
#[derive(Debug, Default, Clone)]
pub struct RunOutput {
    pub stdout: String,
    pub stderr: String,
    pub error: Option<String>,
    pub value: Option<String>,
    pub displays: Vec<(String, Vec<u8>)>,
}
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// Runs dirty pipeline nodes on a dedicated thread.
pub fn spawn_pipeline_worker(session: SharedSession, ctx: egui::Context) {
    std::thread::Builder::new()
        .name("aurafx-pipeline".into())
        .spawn(move || {
            let mut last_watch = Instant::now();
            loop {
                if last_watch.elapsed() > Duration::from_millis(1000) {
                    last_watch = Instant::now();
                    let watched = session.lock().watched();
                    for (id, path, sig) in watched {
                        let now = aurafx_io::signature(&path);
                        if now != sig {
                            // Let the solver finish writing before re-reading.
                            std::thread::sleep(Duration::from_millis(300));
                            aurafx_io::invalidate(&path);
                            match aurafx_io::read_file(&path) {
                                Ok(l) => session.lock().reload_reader(id, l, aurafx_io::signature(&path)),
                                Err(e) => tracing::debug!("watch: {} not readable yet: {e:#}", path.display()),
                            }
                            ctx.request_repaint();
                        }
                    }
                }
                let item = session.lock().take_work();
                match item {
                    Some(item) => {
                        let t0 = Instant::now();
                        let r = item.execute();
                        session.lock().complete(item, r, t0.elapsed().as_secs_f64() * 1000.0);
                        ctx.request_repaint();
                    }
                    None => std::thread::sleep(Duration::from_millis(15)),
                }
            }
        })
        .expect("spawn pipeline worker");
}

pub struct PythonJob {
    pub code: String,
    /// Where the request came from (console, assistant...) so the UI can route the output.
    pub origin: String,
}

pub struct PythonResult {
    pub code: String,
    pub origin: String,
    pub output: RunOutput,
    pub elapsed: Duration,
}

pub enum PythonRequest {
    Run(PythonJob),
    Complete(String, Sender<Vec<String>>),
    /// Load new/changed plugins and unload removed/disabled ones (paths, disabled, reload all).
    SyncPlugins(Vec<std::path::PathBuf>, Vec<std::path::PathBuf>, bool),
    /// Run code and send the output straight back (the Jupyter kernel's cells).
    RunFor(String, Sender<RunOutput>),
    /// Run a plugin panel for an event (title, event JSON); the reply is its items as JSON.
    Panel(String, String, Sender<String>),
    /// The titles of the plugin panels.
    PanelTitles(Sender<Vec<String>>),
}

/// Owns the interpreter on its own thread; the UI talks to it through channels.
pub struct PythonWorker {
    tx: Sender<PythonRequest>,
    rx: Receiver<PythonResult>,
    pub version: Arc<parking_lot::Mutex<String>>,
    pub busy: Arc<std::sync::atomic::AtomicBool>,
    /// Plugin load/reload messages for the UI to show.
    pub plugin_messages: Arc<parking_lot::Mutex<Vec<String>>>,
}

impl PythonWorker {
    pub fn spawn(session: SharedSession, ctx: egui::Context) -> Self {
        let (tx, job_rx) = channel::<PythonRequest>();
        let (res_tx, rx) = channel::<PythonResult>();
        let version = Arc::new(parking_lot::Mutex::new(String::new()));
        let busy = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let (v2, b2) = (version.clone(), busy.clone());
        let plugin_messages = Arc::new(parking_lot::Mutex::new(Vec::new()));
        let pm2 = plugin_messages.clone();
        #[cfg(feature = "python")]
        let session2 = session.clone();
        std::thread::Builder::new()
            .name("aurafx-python".into())
            .spawn(move || {
                #[cfg(feature = "python")]
                let rt = match PythonRuntime::new(session) {
                    Ok(rt) => rt,
                    Err(e) => {
                        *v2.lock() = format!("Python unavailable: {e:#}");
                        return;
                    }
                };
                #[cfg(feature = "python")]
                {
                    *v2.lock() = PythonRuntime::version();
                }
                #[cfg(not(feature = "python"))]
                {
                    let _ = session;
                    *v2.lock() = "not included in this build".into();
                }
                while let Ok(req) = job_rx.recv() {
                    match req {
                        PythonRequest::Run(job) => {
                            b2.store(true, std::sync::atomic::Ordering::Relaxed);
                            let t0 = Instant::now();
                            #[cfg(feature = "python")]
                            let output = rt.run(&job.code);
                            #[cfg(not(feature = "python"))]
                            let output = RunOutput { error: Some("Python is not included in this build of auraFX.".into()), ..Default::default() };
                            b2.store(false, std::sync::atomic::Ordering::Relaxed);
                            let _ = res_tx.send(PythonResult { code: job.code, origin: job.origin, output, elapsed: t0.elapsed() });
                            ctx.request_repaint();
                        }
                        PythonRequest::SyncPlugins(paths, disabled, force) => {
                            #[cfg(feature = "python")]
                            {
                                if force {
                                    aurafx_python::plugins::forget_mtimes();
                                }
                                let (msgs, changed) = aurafx_python::plugins::sync(&paths, &disabled);
                                if !changed.is_empty() {
                                    session2.lock().refresh_filter_defs(&changed);
                                }
                                if !msgs.is_empty() {
                                    let mut s = session2.lock();
                                    for m in &msgs {
                                        let lvl = if m.contains("failed") { aurafx_pipeline::LogLevel::Error } else { aurafx_pipeline::LogLevel::Info };
                                        s.log(lvl, m.clone());
                                    }
                                    pm2.lock().extend(msgs);
                                    ctx.request_repaint();
                                }
                            }
                            #[cfg(not(feature = "python"))]
                            let _ = (paths, disabled, force, &pm2);
                        }
                        PythonRequest::RunFor(code, reply) => {
                            b2.store(true, std::sync::atomic::Ordering::Relaxed);
                            #[cfg(feature = "python")]
                            let output = rt.run(&code);
                            #[cfg(not(feature = "python"))]
                            let output = {
                                let _ = &code;
                                RunOutput { error: Some("Python is not included in this build of auraFX.".into()), ..Default::default() }
                            };
                            b2.store(false, std::sync::atomic::Ordering::Relaxed);
                            let _ = reply.send(output);
                            ctx.request_repaint();
                        }
                        PythonRequest::Panel(title, event, reply) => {
                            #[cfg(feature = "python")]
                            let _ = reply.send(rt.panel(&title, &event));
                            #[cfg(not(feature = "python"))]
                            let _ = (title, event, reply);
                            ctx.request_repaint();
                        }
                        PythonRequest::PanelTitles(reply) => {
                            #[cfg(feature = "python")]
                            let _ = reply.send(rt.panel_titles());
                            #[cfg(not(feature = "python"))]
                            let _ = reply.send(vec![]);
                        }
                        PythonRequest::Complete(prefix, reply) => {
                            #[cfg(feature = "python")]
                            let _ = reply.send(rt.complete(&prefix));
                            #[cfg(not(feature = "python"))]
                            let _ = (prefix, reply.send(vec![]));
                        }
                    }
                }
            })
            .expect("spawn python worker");
        Self { tx, rx, version, busy, plugin_messages }
    }

    pub fn sync_plugins(&self, paths: Vec<std::path::PathBuf>, disabled: Vec<std::path::PathBuf>, force: bool) {
        let _ = self.tx.send(PythonRequest::SyncPlugins(paths, disabled, force));
    }

    pub fn run(&self, code: impl Into<String>, origin: &str) {
        let _ = self.tx.send(PythonRequest::Run(PythonJob { code: code.into(), origin: origin.into() }));
    }

    /// A sender for requests from other threads (the Jupyter kernel).
    pub fn sender(&self) -> Sender<PythonRequest> {
        self.tx.clone()
    }

    pub fn complete(&self, prefix: &str) -> Vec<String> {
        let (tx, rx) = channel();
        if self.tx.send(PythonRequest::Complete(prefix.to_string(), tx)).is_err() {
            return vec![];
        }
        rx.recv_timeout(Duration::from_millis(300)).unwrap_or_default()
    }

    pub fn poll(&self) -> Vec<PythonResult> {
        self.rx.try_iter().collect()
    }
}
