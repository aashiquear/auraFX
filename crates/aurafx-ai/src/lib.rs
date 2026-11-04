//! Chat providers with streaming. Everything runs on plain threads with blocking HTTP so
//! the UI stays simple: it polls a channel of [`StreamEvent`]s.

use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use std::io::{BufRead, BufReader};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::time::Duration;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum ProviderKind {
    #[default]
    Ollama,
    Anthropic,
    OpenAI,
}

impl ProviderKind {
    pub const ALL: [ProviderKind; 3] = [ProviderKind::Ollama, ProviderKind::Anthropic, ProviderKind::OpenAI];
    pub fn label(self) -> &'static str {
        match self {
            ProviderKind::Ollama => "Ollama (local)",
            ProviderKind::Anthropic => "Anthropic (Claude)",
            ProviderKind::OpenAI => "OpenAI-compatible",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AiConfig {
    pub provider: ProviderKind,
    pub model: String,
    /// Stored in the settings file; environment variables ANTHROPIC_API_KEY / OPENAI_API_KEY override when empty.
    pub api_key: String,
    pub ollama_url: String,
    pub openai_url: String,
    pub anthropic_url: String,
    pub max_tokens: u32,
    pub temperature: f32,
}

impl Default for AiConfig {
    fn default() -> Self {
        Self {
            provider: ProviderKind::Ollama,
            model: String::new(),
            api_key: String::new(),
            ollama_url: std::env::var("OLLAMA_HOST")
                .ok()
                .filter(|h| !h.is_empty())
                .map(|h| if h.starts_with("http") { h } else { format!("http://{h}") })
                .unwrap_or_else(|| "http://localhost:11434".into()),
            openai_url: "https://api.openai.com/v1".into(),
            anthropic_url: "https://api.anthropic.com".into(),
            max_tokens: 4096,
            temperature: 0.2,
        }
    }
}

impl AiConfig {
    pub fn resolved_key(&self) -> String {
        if !self.api_key.is_empty() {
            return self.api_key.clone();
        }
        match self.provider {
            ProviderKind::Anthropic => std::env::var("ANTHROPIC_API_KEY").unwrap_or_default(),
            ProviderKind::OpenAI => std::env::var("OPENAI_API_KEY").unwrap_or_default(),
            ProviderKind::Ollama => String::new(),
        }
    }
    pub fn default_model(&self) -> &'static str {
        match self.provider {
            ProviderKind::Anthropic => "claude-sonnet-5-5",
            ProviderKind::OpenAI => "gpt-5",
            ProviderKind::Ollama => "",
        }
    }
    pub fn model_or_default(&self) -> String {
        if self.model.is_empty() {
            self.default_model().to_string()
        } else {
            self.model.clone()
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Role {
    User,
    Assistant,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: Role,
    pub content: String,
}

#[derive(Debug, Clone)]
pub enum StreamEvent {
    Delta(String),
    Done,
    Error(String),
}

fn client() -> Result<reqwest::blocking::Client> {
    Ok(reqwest::blocking::Client::builder().timeout(Duration::from_secs(600)).connect_timeout(Duration::from_secs(10)).build()?)
}

/// Start a streaming chat completion on a background thread.
pub fn stream_chat(cfg: AiConfig, system: String, messages: Vec<ChatMessage>) -> Receiver<StreamEvent> {
    let (tx, rx) = channel();
    std::thread::Builder::new()
        .name("aurafx-ai".into())
        .spawn(move || {
            let r = match cfg.provider {
                ProviderKind::Ollama => stream_ollama(&cfg, &system, &messages, &tx),
                ProviderKind::Anthropic => stream_anthropic(&cfg, &system, &messages, &tx),
                ProviderKind::OpenAI => stream_openai(&cfg, &system, &messages, &tx),
            };
            match r {
                Ok(()) => {
                    let _ = tx.send(StreamEvent::Done);
                }
                Err(e) => {
                    let _ = tx.send(StreamEvent::Error(format!("{e:#}")));
                }
            }
        })
        .expect("spawn ai thread");
    rx
}

fn role_str(r: Role) -> &'static str {
    match r {
        Role::User => "user",
        Role::Assistant => "assistant",
    }
}

fn stream_ollama(cfg: &AiConfig, system: &str, messages: &[ChatMessage], tx: &Sender<StreamEvent>) -> Result<()> {
    let mut msgs = vec![serde_json::json!({"role": "system", "content": system})];
    msgs.extend(messages.iter().map(|m| serde_json::json!({"role": role_str(m.role), "content": m.content})));
    let body = serde_json::json!({
        "model": cfg.model_or_default(),
        "messages": msgs,
        "stream": true,
        "options": {"temperature": cfg.temperature, "num_predict": cfg.max_tokens}
    });
    let resp = client()?.post(format!("{}/api/chat", cfg.ollama_url.trim_end_matches('/'))).json(&body).send().context("connecting to Ollama")?;
    if !resp.status().is_success() {
        let status = resp.status();
        let text = resp.text().unwrap_or_default();
        return Err(anyhow!("Ollama returned {status}: {text}"));
    }
    let reader = BufReader::new(resp);
    for line in reader.lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let v: serde_json::Value = serde_json::from_str(&line).with_context(|| format!("bad Ollama line: {line}"))?;
        if let Some(err) = v.get("error").and_then(|e| e.as_str()) {
            return Err(anyhow!("Ollama: {err}"));
        }
        if let Some(c) = v.pointer("/message/content").and_then(|c| c.as_str()) {
            if !c.is_empty() && tx.send(StreamEvent::Delta(c.to_string())).is_err() {
                return Ok(());
            }
        }
        if v.get("done").and_then(|d| d.as_bool()) == Some(true) {
            break;
        }
    }
    Ok(())
}

fn stream_anthropic(cfg: &AiConfig, system: &str, messages: &[ChatMessage], tx: &Sender<StreamEvent>) -> Result<()> {
    let key = cfg.resolved_key();
    if key.is_empty() {
        return Err(anyhow!("No Anthropic API key. Set it in Settings > AI or export ANTHROPIC_API_KEY."));
    }
    let body = serde_json::json!({
        "model": cfg.model_or_default(),
        "max_tokens": cfg.max_tokens,
        "temperature": cfg.temperature,
        "system": system,
        "stream": true,
        "messages": messages.iter().map(|m| serde_json::json!({"role": role_str(m.role), "content": m.content})).collect::<Vec<_>>(),
    });
    let resp = client()?
        .post(format!("{}/v1/messages", cfg.anthropic_url.trim_end_matches('/')))
        .header("x-api-key", key)
        .header("anthropic-version", "2023-06-01")
        .header("content-type", "application/json")
        .json(&body)
        .send()
        .context("connecting to Anthropic")?;
    if !resp.status().is_success() {
        let status = resp.status();
        let text = resp.text().unwrap_or_default();
        return Err(anyhow!("Anthropic returned {status}: {text}"));
    }
    for line in BufReader::new(resp).lines() {
        let line = line?;
        let Some(data) = line.strip_prefix("data:") else { continue };
        let data = data.trim();
        if data.is_empty() {
            continue;
        }
        let v: serde_json::Value = match serde_json::from_str(data) {
            Ok(v) => v,
            Err(_) => continue,
        };
        match v.get("type").and_then(|t| t.as_str()) {
            Some("content_block_delta") => {
                if let Some(t) = v.pointer("/delta/text").and_then(|t| t.as_str()) {
                    if tx.send(StreamEvent::Delta(t.to_string())).is_err() {
                        return Ok(());
                    }
                }
            }
            Some("error") => return Err(anyhow!("Anthropic: {}", v.pointer("/error/message").and_then(|m| m.as_str()).unwrap_or("unknown error"))),
            Some("message_stop") => break,
            _ => {}
        }
    }
    Ok(())
}

fn stream_openai(cfg: &AiConfig, system: &str, messages: &[ChatMessage], tx: &Sender<StreamEvent>) -> Result<()> {
    let key = cfg.resolved_key();
    let mut msgs = vec![serde_json::json!({"role": "system", "content": system})];
    msgs.extend(messages.iter().map(|m| serde_json::json!({"role": role_str(m.role), "content": m.content})));
    let body = serde_json::json!({
        "model": cfg.model_or_default(),
        "messages": msgs,
        "stream": true,
        "temperature": cfg.temperature,
        "max_completion_tokens": cfg.max_tokens,
    });
    let mut req = client()?.post(format!("{}/chat/completions", cfg.openai_url.trim_end_matches('/'))).json(&body);
    if !key.is_empty() {
        req = req.bearer_auth(key);
    }
    let resp = req.send().context("connecting to OpenAI-compatible server")?;
    if !resp.status().is_success() {
        let status = resp.status();
        let text = resp.text().unwrap_or_default();
        return Err(anyhow!("server returned {status}: {text}"));
    }
    for line in BufReader::new(resp).lines() {
        let line = line?;
        let Some(data) = line.strip_prefix("data:") else { continue };
        let data = data.trim();
        if data == "[DONE]" {
            break;
        }
        let v: serde_json::Value = match serde_json::from_str(data) {
            Ok(v) => v,
            Err(_) => continue,
        };
        if let Some(t) = v.pointer("/choices/0/delta/content").and_then(|t| t.as_str()) {
            if tx.send(StreamEvent::Delta(t.to_string())).is_err() {
                return Ok(());
            }
        }
    }
    Ok(())
}

/// List models available for the configured provider.
pub fn list_models(cfg: &AiConfig) -> Result<Vec<String>> {
    let c = client()?;
    match cfg.provider {
        ProviderKind::Ollama => {
            let v: serde_json::Value = c
                .get(format!("{}/api/tags", cfg.ollama_url.trim_end_matches('/')))
                .timeout(Duration::from_secs(5))
                .send()
                .context("connecting to Ollama")?
                .json()?;
            Ok(v.get("models")
                .and_then(|m| m.as_array())
                .map(|a| a.iter().filter_map(|m| m.get("name").and_then(|n| n.as_str()).map(|s| s.to_string())).collect())
                .unwrap_or_default())
        }
        ProviderKind::Anthropic => {
            let key = cfg.resolved_key();
            if key.is_empty() {
                return Ok(vec!["claude-sonnet-5-5".into(), "claude-opus-5-5".into(), "claude-haiku-5-5".into()]);
            }
            let v: serde_json::Value = c
                .get(format!("{}/v1/models", cfg.anthropic_url.trim_end_matches('/')))
                .header("x-api-key", key)
                .header("anthropic-version", "2023-06-01")
                .timeout(Duration::from_secs(10))
                .send()?
                .json()?;
            Ok(v.get("data")
                .and_then(|d| d.as_array())
                .map(|a| a.iter().filter_map(|m| m.get("id").and_then(|n| n.as_str()).map(|s| s.to_string())).collect())
                .unwrap_or_default())
        }
        ProviderKind::OpenAI => {
            let key = cfg.resolved_key();
            let mut req = c.get(format!("{}/models", cfg.openai_url.trim_end_matches('/'))).timeout(Duration::from_secs(10));
            if !key.is_empty() {
                req = req.bearer_auth(key);
            }
            let v: serde_json::Value = req.send()?.json()?;
            Ok(v.get("data")
                .and_then(|d| d.as_array())
                .map(|a| a.iter().filter_map(|m| m.get("id").and_then(|n| n.as_str()).map(|s| s.to_string())).collect())
                .unwrap_or_default())
        }
    }
}

/// Extract fenced code blocks (```python ... ```) from assistant text.
pub fn python_blocks(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = text;
    while let Some(start) = rest.find("```") {
        let after = &rest[start + 3..];
        let nl = after.find('\n').unwrap_or(after.len());
        let lang = after[..nl].trim().to_ascii_lowercase();
        let body = &after[nl.min(after.len())..];
        let body = body.strip_prefix('\n').unwrap_or(body);
        let Some(end) = body.find("```") else { break };
        if lang.is_empty() || lang == "python" || lang == "py" {
            out.push(body[..end].trim_end().to_string());
        }
        rest = &body[end + 3..];
    }
    out
}

/// The system prompt: tells the model what auraFX is and how to script it.
pub fn system_prompt(pipeline_description: &str, filter_catalog: &str) -> String {
    format!(
        "You are the built-in assistant of auraFX, a scientific visualization application (similar to ParaView, written in Rust).\n\
You help the user visualize and analyze scientific data (meshes, particles, fields, time series).\n\
When an action is needed, answer with a short explanation and ONE ```python code block that uses the embedded `afx` module; \
the user can run it with one click. Keep code minimal. Prefer method chaining on nodes.\n\n\
API cheat sheet:\n\
- afx.load(path) -> Node; afx.active() -> Node or None; afx.nodes(); afx.get(name)\n\
- node.<filter>(**params) -> Node for any filter id below (e.g. node.slice(normal=[0,0,1]), node.contour(values=[0.5]), node.threshold(lower=1, upper=2), node.clip(type='Sphere', radius=2), node.calculator(expression='mag(velocity)', result='speed'), node.glyph(orient='velocity'))\n\
- node.set(**params_or_display) ; display keys: visible, opacity, colormap, representation ('Surface','Wireframe','Points','Surface With Edges','Outline'), point_size, name\n\
- node.color_by('array', assoc='points'|'cells', cmap='Viridis', range=(lo, hi)); node.show(); node.hide(); node.delete()\n\
- node.info() -> dict; node.arrays(); node.array('name') -> numpy; node.points -> numpy (N,3); node.add_array('name', numpy_array)\n\
- afx.<source>(**params) for sources (sphere, cone, wavelet, plane, line, box, cylinder, point_source)\n\
- afx.time(index); afx.times(); afx.view('+x'|'-y'|'iso'|[x,y,z]); afx.reset_camera(); afx.screenshot('file.png'); afx.describe(); afx.run()\n\
- afx.plot(x, y, label=...); afx.over_time(node, 'array', stat='max'); afx.show_panel('Charts'); afx.hide_panel('Log'); afx.layout('analyze'); afx.set_theme('light')\n\
- afx.submit('cmd', host=None, workdir=..., gpu='amd:0', outputs='*.e', auto_load=True) runs simulations; afx.job(id); afx.gpus()\n\
- numpy is available as np; standard scientific Python can be used on arrays.\n\n\
Available filters/sources (id: description):\n{filter_catalog}\n\n\
Current pipeline state:\n{pipeline_description}"
    )
}
