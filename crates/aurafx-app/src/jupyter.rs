//! auraFX as a Jupyter kernel: `aurafx --jupyter-kernel CONNECTION_FILE` (what the kernel spec
//! that `aurafx --install-jupyter-kernel` writes asks Jupyter to run). The window opens as usual;
//! the notebook's cells run in its Python (the `afx` module), their printed output, results,
//! errors and pictures (`afx.display`, `afx.snapshot`) go back to the notebook.
//!
//! The Jupyter messaging protocol 5.3 over ZeroMQ: heartbeat (REP), shell and control (ROUTER),
//! iopub (PUB), stdin (ROUTER, unused); messages signed with HMAC-SHA256.

use crate::worker::{PythonRequest, RunOutput};
use anyhow::{anyhow, Context, Result};
use base64::Engine;
use bytes::Bytes;
use hmac::{KeyInit, Mac};
use serde_json::{json, Value};
use std::sync::mpsc::Sender;
use zeromq::{Socket, SocketRecv, SocketSend, ZmqMessage};

const DELIM: &[u8] = b"<IDS|MSG>";

/// The connection file Jupyter writes for a kernel.
#[derive(Debug, serde::Deserialize)]
pub struct Connection {
    pub ip: String,
    pub transport: String,
    pub shell_port: u16,
    pub iopub_port: u16,
    pub stdin_port: u16,
    pub control_port: u16,
    pub hb_port: u16,
    #[serde(default)]
    pub key: String,
    #[serde(default)]
    pub signature_scheme: String,
}

impl Connection {
    fn endpoint(&self, port: u16) -> String {
        format!("{}://{}:{}", self.transport, self.ip, port)
    }
}

/// A message of the protocol: who sent it (router identities), header, parent header, metadata
/// and content.
#[derive(Debug, Clone)]
pub struct Msg {
    pub ids: Vec<Bytes>,
    pub header: Value,
    pub parent: Value,
    pub metadata: Value,
    pub content: Value,
}

pub fn sign(key: &[u8], parts: &[&[u8]]) -> String {
    if key.is_empty() {
        return String::new();
    }
    let mut mac = hmac::Hmac::<sha2::Sha256>::new_from_slice(key).expect("any key length");
    for p in parts {
        mac.update(p);
    }
    mac.finalize().into_bytes().iter().map(|b| format!("{b:02x}")).collect()
}

/// Split the frames of a received message and check its signature.
pub fn parse(frames: Vec<Bytes>, key: &[u8]) -> Result<Msg> {
    let d = frames.iter().position(|f| f.as_ref() == DELIM).ok_or_else(|| anyhow!("no <IDS|MSG> delimiter"))?;
    if frames.len() < d + 6 {
        return Err(anyhow!("a message with {} frames", frames.len()));
    }
    let (sig, h, p, m, c) = (&frames[d + 1], &frames[d + 2], &frames[d + 3], &frames[d + 4], &frames[d + 5]);
    let want = sign(key, &[h, p, m, c]);
    if !key.is_empty() && want.as_bytes() != sig.as_ref() {
        return Err(anyhow!("bad signature"));
    }
    let js = |b: &Bytes| serde_json::from_slice::<Value>(b).unwrap_or(Value::Null);
    Ok(Msg { ids: frames[..d].to_vec(), header: js(h), parent: js(p), metadata: js(m), content: js(c) })
}

fn new_id() -> String {
    let mut b = [0u8; 16];
    if std::fs::File::open("/dev/urandom").and_then(|mut f| std::io::Read::read_exact(&mut f, &mut b)).is_err() {
        let t = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_nanos();
        b = (t ^ ((std::process::id() as u128) << 70)).to_le_bytes();
    }
    let h: String = b.iter().map(|x| format!("{x:02x}")).collect();
    format!("{}-{}-{}-{}-{}", &h[0..8], &h[8..12], &h[12..16], &h[16..20], &h[20..32])
}

pub struct Kernel {
    key: Vec<u8>,
    session: String,
    shell: zeromq::RouterSocket,
    control: zeromq::RouterSocket,
    iopub: zeromq::PubSocket,
    python: Sender<PythonRequest>,
    count: u64,
}

impl Kernel {
    /// The frames of a message (to `ids`) of type `kind` in reply to `parent`.
    pub fn frames(key: &[u8], session: &str, ids: &[Bytes], kind: &str, parent: &Msg, content: Value) -> Vec<Bytes> {
        let header = json!({
            "msg_id": new_id(),
            "session": session,
            "username": "aurafx",
            "date": now_iso(),
            "msg_type": kind,
            "version": "5.3",
        });
        let h = serde_json::to_vec(&header).unwrap();
        let p = serde_json::to_vec(&parent.header).unwrap();
        let m = b"{}".to_vec();
        let c = serde_json::to_vec(&content).unwrap();
        let sig = sign(key, &[&h, &p, &m, &c]);
        let mut out: Vec<Bytes> = ids.to_vec();
        out.push(Bytes::from_static(DELIM));
        out.extend([Bytes::from(sig), Bytes::from(h), Bytes::from(p), Bytes::from(m), Bytes::from(c)]);
        out
    }

    async fn publish(&mut self, kind: &str, parent: &Msg, content: Value) {
        let topic = [Bytes::from(format!("kernel.{}.{kind}", self.session))];
        let f = Self::frames(&self.key, &self.session, &topic, kind, parent, content);
        if let Ok(m) = ZmqMessage::try_from(f) {
            let _ = self.iopub.send(m).await;
        }
    }

    async fn reply(&mut self, control: bool, kind: &str, parent: &Msg, content: Value) {
        let f = Self::frames(&self.key, &self.session, &parent.ids, kind, parent, content);
        if let Ok(m) = ZmqMessage::try_from(f) {
            let _ = if control { self.control.send(m).await } else { self.shell.send(m).await };
        }
    }

    fn info() -> Value {
        json!({
            "status": "ok",
            "protocol_version": "5.3",
            "implementation": "aurafx",
            "implementation_version": env!("CARGO_PKG_VERSION"),
            "language_info": {"name": "python", "version": "3", "mimetype": "text/x-python", "file_extension": ".py", "pygments_lexer": "ipython3", "codemirror_mode": {"name": "ipython", "version": 3}},
            "banner": format!("auraFX {} — the `afx` module drives the auraFX window; afx.snapshot() shows the 3D view here", env!("CARGO_PKG_VERSION")),
            "help_links": [],
        })
    }

    /// Run a cell in the app's Python and wait for its output.
    async fn run(&self, code: String) -> RunOutput {
        let (tx, rx) = std::sync::mpsc::channel();
        if self.python.send(PythonRequest::RunFor(code, tx)).is_err() {
            return RunOutput { error: Some("the Python of auraFX is not running".into()), ..Default::default() };
        }
        tokio::task::spawn_blocking(move || rx.recv().unwrap_or_else(|_| RunOutput { error: Some("Python stopped".into()), ..Default::default() }))
            .await
            .unwrap_or_default()
    }

    async fn handle(&mut self, control: bool, msg: Msg) -> bool {
        let kind = msg.header["msg_type"].as_str().unwrap_or("").to_string();
        self.publish("status", &msg, json!({"execution_state": "busy"})).await;
        let mut keep = true;
        match kind.as_str() {
            "kernel_info_request" => self.reply(control, "kernel_info_reply", &msg, Self::info()).await,
            "execute_request" => {
                let code = msg.content["code"].as_str().unwrap_or("").to_string();
                let silent = msg.content["silent"].as_bool().unwrap_or(false);
                if !silent {
                    self.count += 1;
                }
                let n = self.count;
                self.publish("execute_input", &msg, json!({"code": code, "execution_count": n})).await;
                let out = if code.trim().is_empty() { RunOutput::default() } else { self.run(code).await };
                if !silent {
                    if !out.stdout.is_empty() {
                        self.publish("stream", &msg, json!({"name": "stdout", "text": out.stdout})).await;
                    }
                    if !out.stderr.is_empty() {
                        self.publish("stream", &msg, json!({"name": "stderr", "text": out.stderr})).await;
                    }
                    for (mime, data) in &out.displays {
                        let v = if mime.starts_with("text/") || mime == "image/svg+xml" {
                            Value::String(String::from_utf8_lossy(data).into_owned())
                        } else {
                            Value::String(base64::engine::general_purpose::STANDARD.encode(data))
                        };
                        self.publish("display_data", &msg, json!({"data": {mime: v}, "metadata": {}})).await;
                    }
                    if let Some(v) = &out.value {
                        self.publish("execute_result", &msg, json!({"execution_count": n, "data": {"text/plain": v}, "metadata": {}})).await;
                    }
                }
                match &out.error {
                    Some(e) => {
                        let lines: Vec<String> = e.lines().map(str::to_string).collect();
                        let last = lines.last().cloned().unwrap_or_default();
                        let (ename, evalue) = last.split_once(": ").map(|(a, b)| (a.to_string(), b.to_string())).unwrap_or(("Error".into(), last.clone()));
                        let err = json!({"ename": ename, "evalue": evalue, "traceback": lines});
                        self.publish("error", &msg, err.clone()).await;
                        let mut r = err;
                        r["status"] = json!("error");
                        r["execution_count"] = json!(n);
                        self.reply(control, "execute_reply", &msg, r).await;
                    }
                    None => {
                        self.reply(control, "execute_reply", &msg, json!({"status": "ok", "execution_count": n, "user_expressions": {}, "payload": []})).await
                    }
                }
            }
            "is_complete_request" => {
                let code = msg.content["code"].as_str().unwrap_or("");
                let open = code.trim_end().ends_with(':') || code.ends_with('\\') || code.matches('(').count() > code.matches(')').count();
                let st = if open { json!({"status": "incomplete", "indent": "    "}) } else { json!({"status": "complete"}) };
                self.reply(control, "is_complete_reply", &msg, st).await;
            }
            "complete_request" => {
                // The word before the cursor, completed by the app's Python.
                let code = msg.content["code"].as_str().unwrap_or("");
                let pos = (msg.content["cursor_pos"].as_u64().unwrap_or(code.len() as u64) as usize).min(code.chars().count());
                let before: String = code.chars().take(pos).collect();
                let start = before.rfind(|c: char| !(c.is_alphanumeric() || c == '_' || c == '.')).map(|i| i + 1).unwrap_or(0);
                let word = before[start..].to_string();
                let (tx, rx) = std::sync::mpsc::channel();
                let matches: Vec<String> = if self.python.send(PythonRequest::Complete(word.clone(), tx)).is_ok() {
                    tokio::task::spawn_blocking(move || rx.recv_timeout(std::time::Duration::from_secs(2)).unwrap_or_default()).await.unwrap_or_default()
                } else {
                    vec![]
                };
                // Names after a dot complete the part after it (`afx.wave` → `wavelet` from the dot on).
                let mut start_chars = before[..start].chars().count();
                if let Some(dot) = word.rfind('.') {
                    if !matches.iter().any(|m| m.contains('.')) {
                        start_chars += word[..dot + 1].chars().count();
                    }
                }
                self.reply(
                    control,
                    "complete_reply",
                    &msg,
                    json!({"status": "ok", "matches": matches, "cursor_start": start_chars, "cursor_end": pos, "metadata": {}}),
                )
                .await;
            }
            "inspect_request" => self.reply(control, "inspect_reply", &msg, json!({"status": "ok", "found": false, "data": {}, "metadata": {}})).await,
            "history_request" => self.reply(control, "history_reply", &msg, json!({"status": "ok", "history": []})).await,
            "comm_info_request" => self.reply(control, "comm_info_reply", &msg, json!({"status": "ok", "comms": {}})).await,
            "interrupt_request" => self.reply(control, "interrupt_reply", &msg, json!({"status": "ok"})).await,
            "shutdown_request" => {
                let restart = msg.content["restart"].as_bool().unwrap_or(false);
                self.reply(control, "shutdown_reply", &msg, json!({"status": "ok", "restart": restart})).await;
                keep = false;
            }
            other => tracing::debug!("jupyter: {other} not handled"),
        }
        self.publish("status", &msg, json!({"execution_state": "idle"})).await;
        keep
    }
}

fn now_iso() -> String {
    let t = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default();
    let secs = t.as_secs() as i64;
    let (days, rem) = (secs.div_euclid(86400), secs.rem_euclid(86400));
    // Civil date from days since 1970-01-01.
    let z = days + 719468;
    let era = z.div_euclid(146097);
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!("{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}.{:06}Z", rem / 3600, rem % 3600 / 60, rem % 60, t.subsec_micros())
}

/// Start the kernel on its own thread; `on_shutdown` runs when the notebook shuts it down.
pub fn start(connection_file: &std::path::Path, python: Sender<PythonRequest>, on_shutdown: Box<dyn FnOnce() + Send>) -> Result<()> {
    let text = std::fs::read_to_string(connection_file).with_context(|| format!("reading {}", connection_file.display()))?;
    let conn: Connection = serde_json::from_str(&text).context("the connection file")?;
    if !conn.signature_scheme.is_empty() && conn.signature_scheme != "hmac-sha256" {
        return Err(anyhow!("signature scheme {} is not supported (hmac-sha256 is)", conn.signature_scheme));
    }
    std::thread::Builder::new().name("aurafx-jupyter".into()).spawn(move || {
        let rt = match tokio::runtime::Builder::new_current_thread().enable_all().build() {
            Ok(rt) => rt,
            Err(e) => {
                tracing::error!("jupyter: {e}");
                return;
            }
        };
        let r: Result<()> = rt.block_on(async move {
            let mut hb = zeromq::RepSocket::new();
            hb.bind(&conn.endpoint(conn.hb_port)).await?;
            let mut shell = zeromq::RouterSocket::new();
            shell.bind(&conn.endpoint(conn.shell_port)).await?;
            let mut control = zeromq::RouterSocket::new();
            control.bind(&conn.endpoint(conn.control_port)).await?;
            let mut stdin = zeromq::RouterSocket::new();
            stdin.bind(&conn.endpoint(conn.stdin_port)).await?;
            let mut iopub = zeromq::PubSocket::new();
            iopub.bind(&conn.endpoint(conn.iopub_port)).await?;
            tracing::info!("jupyter: kernel listening on {} (shell {})", conn.ip, conn.shell_port);
            // Heartbeat: echo every ping.
            tokio::spawn(async move {
                while let Ok(m) = hb.recv().await {
                    if hb.send(m).await.is_err() {
                        break;
                    }
                }
            });
            let key = conn.key.as_bytes().to_vec();
            let mut k = Kernel { key: key.clone(), session: new_id(), shell, control, iopub, python, count: 0 };
            // Tell the frontend the kernel is up.
            let hello = Msg { ids: vec![], header: json!({}), parent: json!({}), metadata: json!({}), content: json!({}) };
            k.publish("status", &hello, json!({"execution_state": "starting"})).await;
            let _keep_stdin = stdin;
            loop {
                let (control, m) = tokio::select! {
                    m = k.control.recv() => (true, m),
                    m = k.shell.recv() => (false, m),
                };
                let m = match m {
                    Ok(m) => m,
                    Err(e) => {
                        tracing::warn!("jupyter: {e}");
                        continue;
                    }
                };
                match parse(m.into_vec(), &key) {
                    Ok(msg) => {
                        if !k.handle(control, msg).await {
                            break;
                        }
                    }
                    Err(e) => tracing::warn!("jupyter: dropped a message: {e:#}"),
                }
            }
            Ok(())
        });
        if let Err(e) = r {
            tracing::error!("jupyter kernel: {e:#}");
        }
        on_shutdown();
    })?;
    Ok(())
}

/// Write the kernel spec (`kernel.json` in Jupyter's kernels folder) so that Jupyter lists
/// "auraFX" and starts this program as the kernel. Returns the folder.
pub fn install_kernel_spec() -> Result<std::path::PathBuf> {
    let exe = std::env::current_exe()?;
    let base = if cfg!(target_os = "macos") {
        directories::BaseDirs::new().map(|d| d.home_dir().join("Library/Jupyter"))
    } else if cfg!(windows) {
        std::env::var_os("APPDATA").map(|a| std::path::PathBuf::from(a).join("jupyter"))
    } else {
        directories::BaseDirs::new().map(|d| d.data_dir().join("jupyter"))
    }
    .ok_or_else(|| anyhow!("no home folder"))?;
    let base = std::env::var_os("JUPYTER_DATA_DIR").map(std::path::PathBuf::from).unwrap_or(base);
    let dir = base.join("kernels").join("aurafx");
    std::fs::create_dir_all(&dir)?;
    let spec = json!({
        "argv": [exe.display().to_string(), "--jupyter-kernel", "{connection_file}"],
        "display_name": "auraFX (Python)",
        "language": "python",
        "interrupt_mode": "message",
        "metadata": {"debugger": false},
    });
    std::fs::write(dir.join("kernel.json"), serde_json::to_string_pretty(&spec)?)?;
    Ok(dir)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn messages_are_signed_and_parsed_back() {
        let key = b"secret";
        let parent = Msg {
            ids: vec![Bytes::from_static(b"client")],
            header: json!({"msg_id": "1", "msg_type": "execute_request"}),
            parent: json!({}),
            metadata: json!({}),
            content: json!({}),
        };
        let f = Kernel::frames(key, "s", &parent.ids, "execute_reply", &parent, json!({"status": "ok"}));
        let m = parse(f.clone(), key).unwrap();
        assert_eq!(m.ids, vec![Bytes::from_static(b"client")]);
        assert_eq!(m.header["msg_type"], "execute_reply");
        assert_eq!(m.parent["msg_id"], "1");
        assert_eq!(m.content["status"], "ok");
        // A wrong key is refused.
        assert!(parse(f, b"other").is_err());
        // HMAC-SHA256 of a known message (RFC 4231 test case 2).
        assert_eq!(sign(b"Jefe", &[b"what do ya want ", b"for nothing?"]), "5bdcc146bf60754e6a042426089575c75a003f089d2739839dec58b964ec3843");
        assert_eq!(now_iso().len(), 27);
    }
}
