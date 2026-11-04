//! Running simulations and Python pipelines: locally, on remote machines over SSH, or
//! through a Slurm queue, with GPU selection for NVIDIA (CUDA) and AMD (ROCm/HIP).
//!
//! SSH uses the system `ssh`/`scp` binaries so the user's `~/.ssh/config`, keys, agent,
//! ProxyJump and 2FA setup all work unchanged.

pub mod gpu;
pub mod templates;

use anyhow::{anyhow, bail, Context, Result};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, LazyLock};
use std::time::{Duration, Instant, SystemTime};

pub use gpu::{GpuInfo, GpuVendor};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum HostKind {
    #[default]
    Local,
    /// `target` is anything `ssh` accepts: "user@host", or an alias from ~/.ssh/config.
    Ssh { target: String, port: Option<u16>, identity: Option<String>, jump: Option<String> },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Host {
    pub name: String,
    pub kind: HostKind,
    /// Optional shell lines run before every command (e.g. `module load lammps`).
    #[serde(default)]
    pub setup: String,
}

impl Host {
    pub fn local() -> Self {
        Host { name: "This computer".into(), kind: HostKind::Local, setup: String::new() }
    }
    pub fn is_local(&self) -> bool {
        matches!(self.kind, HostKind::Local)
    }
    fn ssh_args(&self) -> Vec<String> {
        let mut a = vec!["-o".into(), "BatchMode=yes".into(), "-o".into(), "ServerAliveInterval=30".into()];
        if let HostKind::Ssh { target, port, identity, jump } = &self.kind {
            if let Some(p) = port {
                a.extend(["-p".into(), p.to_string()]);
            }
            if let Some(i) = identity.as_ref().filter(|s| !s.is_empty()) {
                a.extend(["-i".into(), shellexpand(i)]);
            }
            if let Some(j) = jump.as_ref().filter(|s| !s.is_empty()) {
                a.extend(["-J".into(), j.clone()]);
            }
            a.push(target.clone());
        }
        a
    }
    /// Build a process that runs `script` (bash) on this host.
    pub fn command(&self, script: &str) -> Command {
        let full = if self.setup.trim().is_empty() { script.to_string() } else { format!("{}\n{}", self.setup, script) };
        match &self.kind {
            HostKind::Local => {
                let mut c = Command::new("bash");
                c.arg("-lc").arg(full);
                c
            }
            HostKind::Ssh { .. } => {
                let mut c = Command::new("ssh");
                c.args(self.ssh_args());
                c.arg(format!("bash -lc {}", shell_quote(&full)));
                c
            }
        }
    }
    /// Run a short command and capture stdout.
    pub fn run_capture(&self, script: &str, timeout: Duration) -> Result<String> {
        let mut c = self.command(script);
        c.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
        let child = c.spawn().with_context(|| format!("starting command on {}", self.name))?;
        let out = wait_with_timeout(child, timeout)?;
        if !out.status.success() {
            bail!("{} (exit {:?}): {}", self.name, out.status.code(), String::from_utf8_lossy(&out.stderr).trim());
        }
        Ok(String::from_utf8_lossy(&out.stdout).to_string())
    }
    /// Test connectivity; returns the remote `uname -a`.
    pub fn test(&self) -> Result<String> {
        Ok(self.run_capture("uname -a; nproc; command -v nvidia-smi rocm-smi sbatch lmp mpirun python3 2>/dev/null", Duration::from_secs(20))?.trim().to_string())
    }
    /// List a directory: (name, is_dir, size).
    pub fn list_dir(&self, path: &str) -> Result<Vec<(String, bool, u64)>> {
        let out = self.run_capture(&format!("cd {} && for f in .* *; do [ \"$f\" = . ] && continue; [ -e \"$f\" ] || continue; if [ -d \"$f\" ]; then echo \"d 0 $f\"; else echo \"f $(stat -c %s \"$f\" 2>/dev/null || stat -f %z \"$f\") $f\"; fi; done", shell_quote(path)), Duration::from_secs(20))?;
        Ok(out
            .lines()
            .filter_map(|l| {
                let mut it = l.splitn(3, ' ');
                let kind = it.next()?;
                let size = it.next()?.parse().unwrap_or(0);
                let name = it.next()?.to_string();
                Some((name, kind == "d", size))
            })
            .collect())
    }
    /// Copy a remote file to a local path (no-op copy for local hosts).
    pub fn fetch(&self, remote: &str, local: &Path) -> Result<()> {
        if let Some(dir) = local.parent() {
            std::fs::create_dir_all(dir)?;
        }
        match &self.kind {
            HostKind::Local => {
                std::fs::copy(remote, local)?;
                Ok(())
            }
            HostKind::Ssh { target, port, identity, jump } => {
                let mut c = Command::new("scp");
                c.args(["-o", "BatchMode=yes", "-q"]);
                if let Some(p) = port {
                    c.args(["-P", &p.to_string()]);
                }
                if let Some(i) = identity.as_ref().filter(|s| !s.is_empty()) {
                    c.args(["-i", &shellexpand(i)]);
                }
                if let Some(j) = jump.as_ref().filter(|s| !s.is_empty()) {
                    c.args(["-J", j]);
                }
                c.arg(format!("{target}:{remote}")).arg(local);
                let out = c.output().context("running scp")?;
                if !out.status.success() {
                    bail!("scp failed: {}", String::from_utf8_lossy(&out.stderr).trim());
                }
                Ok(())
            }
        }
    }
    /// Copy a local file to the host.
    pub fn upload(&self, local: &Path, remote: &str) -> Result<()> {
        match &self.kind {
            HostKind::Local => {
                std::fs::copy(local, remote)?;
                Ok(())
            }
            HostKind::Ssh { target, port, identity, jump } => {
                let mut c = Command::new("scp");
                c.args(["-o", "BatchMode=yes", "-q"]);
                if let Some(p) = port {
                    c.args(["-P", &p.to_string()]);
                }
                if let Some(i) = identity.as_ref().filter(|s| !s.is_empty()) {
                    c.args(["-i", &shellexpand(i)]);
                }
                if let Some(j) = jump.as_ref().filter(|s| !s.is_empty()) {
                    c.args(["-J", j]);
                }
                c.arg(local).arg(format!("{target}:{remote}"));
                let out = c.output().context("running scp")?;
                if !out.status.success() {
                    bail!("scp failed: {}", String::from_utf8_lossy(&out.stderr).trim());
                }
                Ok(())
            }
        }
    }
}

fn wait_with_timeout(mut child: Child, timeout: Duration) -> Result<std::process::Output> {
    let start = Instant::now();
    loop {
        if child.try_wait()?.is_some() {
            return Ok(child.wait_with_output()?);
        }
        if start.elapsed() > timeout {
            let _ = child.kill();
            bail!("timed out after {:?}", timeout);
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

pub fn shell_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

fn shellexpand(p: &str) -> String {
    if let Some(rest) = p.strip_prefix("~/") {
        if let Some(h) = std::env::var_os("HOME") {
            return format!("{}/{}", h.to_string_lossy(), rest);
        }
    }
    p.to_string()
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum GpuRequest {
    #[default]
    None,
    /// Any GPU: sets both CUDA and HIP visibility to the given ids.
    Any(Vec<u32>),
    Nvidia(Vec<u32>),
    Amd(Vec<u32>),
}

impl GpuRequest {
    /// Environment exports selecting the devices.
    pub fn env(&self) -> Vec<(String, String)> {
        let ids = |v: &Vec<u32>| v.iter().map(|i| i.to_string()).collect::<Vec<_>>().join(",");
        match self {
            GpuRequest::None => vec![],
            GpuRequest::Nvidia(v) => vec![("CUDA_VISIBLE_DEVICES".into(), ids(v))],
            GpuRequest::Amd(v) => vec![("HIP_VISIBLE_DEVICES".into(), ids(v)), ("ROCR_VISIBLE_DEVICES".into(), ids(v))],
            GpuRequest::Any(v) => vec![("CUDA_VISIBLE_DEVICES".into(), ids(v)), ("HIP_VISIBLE_DEVICES".into(), ids(v))],
        }
    }
    pub fn count(&self) -> usize {
        match self {
            GpuRequest::None => 0,
            GpuRequest::Any(v) | GpuRequest::Nvidia(v) | GpuRequest::Amd(v) => v.len().max(1),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum Scheduler {
    /// Run directly on the host.
    #[default]
    Direct,
    /// Submit with `sbatch` and poll `squeue`.
    Slurm { partition: String, time: String, nodes: u32, tasks: u32, extra: String },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct JobSpec {
    pub name: String,
    pub host: Host,
    pub workdir: String,
    /// Shell command(s) to run (bash).
    pub command: String,
    #[serde(default)]
    pub env: Vec<(String, String)>,
    #[serde(default)]
    pub gpu: GpuRequest,
    #[serde(default)]
    pub scheduler: Scheduler,
    /// Glob (relative to workdir) of result files to fetch/open when the job ends, e.g. "*.e" or "dump.*".
    #[serde(default)]
    pub outputs: String,
    #[serde(default)]
    pub auto_load: bool,
}

impl JobSpec {
    pub fn new(name: impl Into<String>, command: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            host: Host::local(),
            workdir: std::env::current_dir().map(|p| p.display().to_string()).unwrap_or_else(|_| ".".into()),
            command: command.into(),
            env: vec![],
            gpu: GpuRequest::None,
            scheduler: Scheduler::Direct,
            outputs: String::new(),
            auto_load: false,
        }
    }

    /// The bash script that will be executed (or submitted).
    pub fn script(&self) -> String {
        let mut s = String::new();
        for (k, v) in self.env.iter().cloned().chain(self.gpu.env()) {
            s += &format!("export {k}={}\n", shell_quote(&v));
        }
        s += &format!("cd {} || exit 97\n", shell_quote(&self.workdir));
        s += &self.command;
        s.push('\n');
        s
    }

    fn slurm_script(&self, partition: &str, time: &str, nodes: u32, tasks: u32, extra: &str) -> String {
        let mut s = String::from("#!/bin/bash\n");
        s += &format!("#SBATCH --job-name={}\n", self.name.replace(' ', "_"));
        s += &format!("#SBATCH --output={}/aurafx-%j.log\n", self.workdir);
        if !partition.is_empty() {
            s += &format!("#SBATCH --partition={partition}\n");
        }
        if !time.is_empty() {
            s += &format!("#SBATCH --time={time}\n");
        }
        s += &format!("#SBATCH --nodes={}\n#SBATCH --ntasks={}\n", nodes.max(1), tasks.max(1));
        if self.gpu.count() > 0 {
            s += &format!("#SBATCH --gpus={}\n", self.gpu.count());
        }
        for l in extra.lines().filter(|l| !l.trim().is_empty()) {
            s += &format!("#SBATCH {}\n", l.trim().trim_start_matches("#SBATCH").trim());
        }
        if !self.host.setup.trim().is_empty() {
            s += &self.host.setup;
            s.push('\n');
        }
        s += &self.script();
        s
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum JobState {
    Queued,
    Submitted(String),
    Running,
    Finished(i32),
    Failed(String),
    Cancelled,
}

impl JobState {
    pub fn is_active(&self) -> bool {
        matches!(self, JobState::Queued | JobState::Submitted(_) | JobState::Running)
    }
    pub fn label(&self) -> String {
        match self {
            JobState::Queued => "queued".into(),
            JobState::Submitted(id) => format!("submitted ({id})"),
            JobState::Running => "running".into(),
            JobState::Finished(0) => "done".into(),
            JobState::Finished(c) => format!("exit {c}"),
            JobState::Failed(e) => format!("failed: {e}"),
            JobState::Cancelled => "cancelled".into(),
        }
    }
}

pub type JobId = u64;

pub struct Job {
    pub id: JobId,
    pub spec: JobSpec,
    pub state: JobState,
    pub log: VecDeque<String>,
    pub started: Option<SystemTime>,
    pub elapsed: Option<Duration>,
    start_instant: Option<Instant>,
    child: Option<Arc<Mutex<Option<Child>>>>,
    /// Local paths of fetched/located outputs once the job is finished.
    pub outputs: Vec<PathBuf>,
    /// Set when outputs are ready for the UI to auto-load; the UI clears it.
    pub outputs_pending_load: bool,
}

impl Job {
    pub fn runtime(&self) -> Option<Duration> {
        self.elapsed.or_else(|| self.start_instant.map(|s| s.elapsed()))
    }
}

const MAX_LOG: usize = 20_000;

#[derive(Default)]
pub struct JobManager {
    pub jobs: Vec<Job>,
    next: JobId,
    pub revision: u64,
}

pub type SharedJobs = Arc<Mutex<JobManager>>;

static GLOBAL: LazyLock<SharedJobs> = LazyLock::new(|| Arc::new(Mutex::new(JobManager { next: 1, ..Default::default() })));

/// The process-wide job manager (shared by the UI and the Python API).
pub fn global() -> SharedJobs {
    GLOBAL.clone()
}

fn push_log(jobs: &SharedJobs, id: JobId, line: String) {
    let mut m = jobs.lock();
    if let Some(j) = m.jobs.iter_mut().find(|j| j.id == id) {
        j.log.push_back(line);
        while j.log.len() > MAX_LOG {
            j.log.pop_front();
        }
    }
    m.revision += 1;
}

fn set_state(jobs: &SharedJobs, id: JobId, state: JobState) {
    let mut m = jobs.lock();
    if let Some(j) = m.jobs.iter_mut().find(|j| j.id == id) {
        if !state.is_active() {
            j.elapsed = j.start_instant.map(|s| s.elapsed());
        }
        if state == JobState::Running && j.start_instant.is_none() {
            j.start_instant = Some(Instant::now());
            j.started = Some(SystemTime::now());
        }
        // Do not overwrite a user cancel with the exit status of the killed process.
        if j.state != JobState::Cancelled {
            j.state = state;
        }
    }
    m.revision += 1;
}

/// Submit a job; it starts immediately on a background thread.
pub fn submit(jobs: &SharedJobs, spec: JobSpec) -> JobId {
    let id = {
        let mut m = jobs.lock();
        let id = m.next.max(1);
        m.next = id + 1;
        m.jobs.push(Job { id, spec: spec.clone(), state: JobState::Queued, log: VecDeque::new(), started: None, elapsed: None, start_instant: None, child: None, outputs: vec![], outputs_pending_load: false });
        m.revision += 1;
        id
    };
    let jobs2 = jobs.clone();
    std::thread::Builder::new()
        .name(format!("aurafx-job-{id}"))
        .spawn(move || {
            let r = match &spec.scheduler {
                Scheduler::Direct => run_direct(&jobs2, id, &spec),
                Scheduler::Slurm { partition, time, nodes, tasks, extra } => run_slurm(&jobs2, id, &spec, partition, time, *nodes, *tasks, extra),
            };
            if let Err(e) = r {
                push_log(&jobs2, id, format!("[auraFX] error: {e:#}"));
                set_state(&jobs2, id, JobState::Failed(format!("{e:#}")));
            }
            collect_outputs(&jobs2, id, &spec);
        })
        .expect("spawn job thread");
    id
}

fn stream(jobs: &SharedJobs, id: JobId, r: impl std::io::Read + Send + 'static, prefix: &'static str) -> std::thread::JoinHandle<()> {
    let jobs = jobs.clone();
    std::thread::spawn(move || {
        for line in BufReader::new(r).lines() {
            match line {
                Ok(l) => push_log(&jobs, id, format!("{prefix}{l}")),
                Err(_) => break,
            }
        }
    })
}

fn run_direct(jobs: &SharedJobs, id: JobId, spec: &JobSpec) -> Result<()> {
    let script = spec.script();
    push_log(jobs, id, format!("[auraFX] {} on {}: {}", spec.name, spec.host.name, spec.command.lines().next().unwrap_or("")));
    let mut cmd = spec.host.command(&script);
    cmd.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
    if !spec.host.is_local() {
        // Allocate a pseudo-terminal so cancelling (killing ssh) also stops the remote process.
        let args: Vec<String> = cmd.get_args().map(|a| a.to_string_lossy().to_string()).collect();
        cmd = Command::new("ssh");
        cmd.arg("-tt").args(args).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped());
    }
    let mut child = cmd.spawn().with_context(|| format!("starting job on {}", spec.host.name))?;
    let out = child.stdout.take().unwrap();
    let err = child.stderr.take().unwrap();
    let holder = Arc::new(Mutex::new(Some(child)));
    {
        let mut m = jobs.lock();
        if let Some(j) = m.jobs.iter_mut().find(|j| j.id == id) {
            j.child = Some(holder.clone());
        }
    }
    set_state(jobs, id, JobState::Running);
    let t1 = stream(jobs, id, out, "");
    let t2 = stream(jobs, id, err, "! ");
    let status = loop {
        {
            let mut g = holder.lock();
            match g.as_mut() {
                Some(c) => {
                    if let Some(st) = c.try_wait()? {
                        break Some(st);
                    }
                }
                None => break None,
            }
        }
        std::thread::sleep(Duration::from_millis(100));
    };
    let _ = t1.join();
    let _ = t2.join();
    let code = status.and_then(|s| s.code()).unwrap_or(-1);
    push_log(jobs, id, format!("[auraFX] exited with code {code}"));
    set_state(jobs, id, JobState::Finished(code));
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn run_slurm(jobs: &SharedJobs, id: JobId, spec: &JobSpec, partition: &str, time: &str, nodes: u32, tasks: u32, extra: &str) -> Result<()> {
    let script = spec.slurm_script(partition, time, nodes, tasks, extra);
    let submit = format!("mkdir -p {wd} && cd {wd} && cat > .aurafx_job_{id}.sh <<'AURAFX_EOF'\n{script}AURAFX_EOF\nsbatch --parsable .aurafx_job_{id}.sh", wd = shell_quote(&spec.workdir));
    let bare = Host { setup: String::new(), ..spec.host.clone() };
    let out = bare.run_capture(&submit, Duration::from_secs(60))?;
    let slurm_id = out.trim().split(';').next().unwrap_or("").to_string();
    if slurm_id.is_empty() {
        bail!("sbatch did not return a job id: {out}");
    }
    push_log(jobs, id, format!("[auraFX] submitted Slurm job {slurm_id}"));
    set_state(jobs, id, JobState::Submitted(slurm_id.clone()));
    let log_path = format!("{}/aurafx-{slurm_id}.log", spec.workdir);
    let mut shown = 0usize;
    loop {
        if jobs.lock().jobs.iter().find(|j| j.id == id).map(|j| j.state == JobState::Cancelled).unwrap_or(true) {
            let _ = bare.run_capture(&format!("scancel {slurm_id}"), Duration::from_secs(30));
            return Ok(());
        }
        let st = bare.run_capture(&format!("squeue -h -j {slurm_id} -o %T 2>/dev/null; true"), Duration::from_secs(30)).unwrap_or_default();
        let st = st.trim();
        if st == "RUNNING" {
            set_state(jobs, id, JobState::Running);
        }
        // Tail new log lines.
        if let Ok(text) = bare.run_capture(&format!("tail -n +{} {} 2>/dev/null; true", shown + 1, shell_quote(&log_path)), Duration::from_secs(30)) {
            for l in text.lines() {
                push_log(jobs, id, l.to_string());
                shown += 1;
            }
        }
        if st.is_empty() {
            let code = bare
                .run_capture(&format!("sacct -n -X -j {slurm_id} -o ExitCode 2>/dev/null | head -1"), Duration::from_secs(30))
                .ok()
                .and_then(|s| s.trim().split(':').next().and_then(|c| c.parse().ok()))
                .unwrap_or(0);
            push_log(jobs, id, format!("[auraFX] Slurm job {slurm_id} finished (exit {code})"));
            set_state(jobs, id, JobState::Finished(code));
            return Ok(());
        }
        std::thread::sleep(Duration::from_secs(5));
    }
}

/// Find output files matching the spec's glob; for remote hosts copy them into a local cache.
fn collect_outputs(jobs: &SharedJobs, id: JobId, spec: &JobSpec) {
    if spec.outputs.trim().is_empty() {
        return;
    }
    let pattern = spec.outputs.trim();
    let listing = spec.host.run_capture(&format!("cd {} && ls -1d {} 2>/dev/null; true", shell_quote(&spec.workdir), pattern), Duration::from_secs(60));
    let files: Vec<String> = listing.map(|s| s.lines().map(|l| l.trim().to_string()).filter(|l| !l.is_empty()).collect()).unwrap_or_default();
    let mut local = Vec::new();
    for f in files {
        let remote = format!("{}/{}", spec.workdir.trim_end_matches('/'), f);
        if spec.host.is_local() {
            local.push(PathBuf::from(remote));
        } else {
            let dest = cache_dir().join(format!("job{id}")).join(&f);
            push_log(jobs, id, format!("[auraFX] fetching {f}"));
            match spec.host.fetch(&remote, &dest) {
                Ok(()) => local.push(dest),
                Err(e) => push_log(jobs, id, format!("[auraFX] fetch failed for {f}: {e:#}")),
            }
        }
    }
    let mut m = jobs.lock();
    if let Some(j) = m.jobs.iter_mut().find(|j| j.id == id) {
        j.outputs = local;
        j.outputs_pending_load = spec.auto_load && !j.outputs.is_empty() && matches!(j.state, JobState::Finished(0));
    }
    m.revision += 1;
}

pub fn cache_dir() -> PathBuf {
    std::env::var_os("XDG_CACHE_HOME").map(PathBuf::from).or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".cache"))).unwrap_or_else(std::env::temp_dir).join("aurafx")
}

pub fn cancel(jobs: &SharedJobs, id: JobId) -> Result<()> {
    let holder = {
        let mut m = jobs.lock();
        let j = m.jobs.iter_mut().find(|j| j.id == id).ok_or_else(|| anyhow!("no job {id}"))?;
        if !j.state.is_active() {
            return Ok(());
        }
        j.state = JobState::Cancelled;
        j.elapsed = j.start_instant.map(|s| s.elapsed());
        m.revision += 1;
        m.jobs.iter().find(|j| j.id == id).and_then(|j| j.child.clone())
    };
    if let Some(h) = holder {
        if let Some(c) = h.lock().as_mut() {
            let _ = c.kill();
        }
    }
    push_log(jobs, id, "[auraFX] cancelled".into());
    Ok(())
}

/// Remove finished jobs from the list.
pub fn clear_finished(jobs: &SharedJobs) {
    let mut m = jobs.lock();
    m.jobs.retain(|j| j.state.is_active());
    m.revision += 1;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wait(jobs: &SharedJobs, id: JobId) -> JobState {
        for _ in 0..200 {
            let st = jobs.lock().jobs.iter().find(|j| j.id == id).unwrap().state.clone();
            if !st.is_active() {
                // Allow output collection to finish.
                std::thread::sleep(Duration::from_millis(100));
                return st;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        panic!("job did not finish");
    }

    #[test]
    fn local_job_streams_and_collects() {
        let jobs: SharedJobs = Arc::new(Mutex::new(JobManager { next: 1, ..Default::default() }));
        let dir = std::env::temp_dir().join(format!("aurafx-job-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let mut spec = JobSpec::new("echo", "echo hello; echo oops >&2; echo $CUDA_VISIBLE_DEVICES; touch out1.vtu out2.vtu; exit 3");
        spec.workdir = dir.display().to_string();
        spec.gpu = GpuRequest::Nvidia(vec![1, 2]);
        spec.outputs = "*.vtu".into();
        spec.auto_load = true;
        let id = submit(&jobs, spec);
        assert_eq!(wait(&jobs, id), JobState::Finished(3));
        let m = jobs.lock();
        let j = &m.jobs[0];
        let log: Vec<&String> = j.log.iter().collect();
        assert!(log.iter().any(|l| *l == "hello"));
        assert!(log.iter().any(|l| *l == "! oops"));
        assert!(log.iter().any(|l| *l == "1,2"));
        assert_eq!(j.outputs.len(), 2);
        assert!(!j.outputs_pending_load, "non-zero exit must not auto-load");
    }

    #[test]
    fn cancel_kills() {
        let jobs: SharedJobs = Arc::new(Mutex::new(JobManager { next: 1, ..Default::default() }));
        let mut spec = JobSpec::new("sleep", "sleep 30");
        spec.workdir = "/tmp".into();
        let id = submit(&jobs, spec);
        std::thread::sleep(Duration::from_millis(300));
        cancel(&jobs, id).unwrap();
        assert_eq!(wait(&jobs, id), JobState::Cancelled);
    }
}

static HOSTS: LazyLock<Mutex<Vec<Host>>> = LazyLock::new(|| Mutex::new(Vec::new()));

/// Hosts configured by the user (set by the app from its settings). Always includes the local machine.
pub fn hosts() -> Vec<Host> {
    let mut v = vec![Host::local()];
    v.extend(HOSTS.lock().iter().cloned());
    v
}

pub fn set_hosts(h: Vec<Host>) {
    *HOSTS.lock() = h;
}

/// Find a host by name, or treat the string as an ssh target ("user@machine").
pub fn resolve_host(name: Option<&str>) -> Host {
    match name {
        None | Some("") | Some("local") | Some("localhost") => Host::local(),
        Some(n) => hosts().into_iter().find(|h| h.name == n).unwrap_or_else(|| Host { name: n.to_string(), kind: HostKind::Ssh { target: n.to_string(), port: None, identity: None, jump: None }, setup: String::new() }),
    }
}
