//! Background execution of the pipeline and of Python code.

use aurafx_pipeline::SharedSession;
use aurafx_python::{PythonRuntime, RunOutput};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// Runs dirty pipeline nodes on a dedicated thread.
pub fn spawn_pipeline_worker(session: SharedSession, ctx: egui::Context) {
    std::thread::Builder::new()
        .name("aurafx-pipeline".into())
        .spawn(move || loop {
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
}

/// Owns the interpreter on its own thread; the UI talks to it through channels.
pub struct PythonWorker {
    tx: Sender<PythonRequest>,
    rx: Receiver<PythonResult>,
    pub version: Arc<parking_lot::Mutex<String>>,
    pub busy: Arc<std::sync::atomic::AtomicBool>,
}

impl PythonWorker {
    pub fn spawn(session: SharedSession, ctx: egui::Context) -> Self {
        let (tx, job_rx) = channel::<PythonRequest>();
        let (res_tx, rx) = channel::<PythonResult>();
        let version = Arc::new(parking_lot::Mutex::new(String::new()));
        let busy = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let (v2, b2) = (version.clone(), busy.clone());
        std::thread::Builder::new()
            .name("aurafx-python".into())
            .spawn(move || {
                let rt = match PythonRuntime::new(session) {
                    Ok(rt) => rt,
                    Err(e) => {
                        *v2.lock() = format!("Python unavailable: {e:#}");
                        return;
                    }
                };
                *v2.lock() = PythonRuntime::version();
                while let Ok(req) = job_rx.recv() {
                    match req {
                        PythonRequest::Run(job) => {
                            b2.store(true, std::sync::atomic::Ordering::Relaxed);
                            let t0 = Instant::now();
                            let output = rt.run(&job.code);
                            b2.store(false, std::sync::atomic::Ordering::Relaxed);
                            let _ = res_tx.send(PythonResult { code: job.code, origin: job.origin, output, elapsed: t0.elapsed() });
                            ctx.request_repaint();
                        }
                        PythonRequest::Complete(prefix, reply) => {
                            let _ = reply.send(rt.complete(&prefix));
                        }
                    }
                }
            })
            .expect("spawn python worker");
        Self { tx, rx, version, busy }
    }

    pub fn run(&self, code: impl Into<String>, origin: &str) {
        let _ = self.tx.send(PythonRequest::Run(PythonJob { code: code.into(), origin: origin.into() }));
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
