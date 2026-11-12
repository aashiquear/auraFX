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
    /// Send nothing off this computer's network: only Ollama at a local or private address, and no
    /// cloud model (a name ending in ":cloud").
    #[serde(default)]
    pub local_only: bool,
    /// Replace the home folder with ~ and full file paths with their file names in what is sent.
    #[serde(default)]
    pub redact_paths: bool,
    /// Saved prompts (name, text); `{object}` stands for the selected object's name.
    #[serde(default = "default_prompts")]
    pub prompts: Vec<PromptTemplate>,
    /// Skills (Markdown files in the skills folder) switched off, by file name.
    #[serde(default)]
    pub skills_off: Vec<String>,
}

/// A saved prompt.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PromptTemplate {
    pub name: String,
    pub text: String,
}

impl PromptTemplate {
    /// The text with `{object}` replaced by the selected object's name.
    pub fn fill(&self, object: &str) -> String {
        self.text.replace("{object}", object)
    }
}

/// The prompts offered before the user saves their own.
pub fn default_prompts() -> Vec<PromptTemplate> {
    [
        ("Describe", "Describe {object}: what kind of data it is, its arrays with their ranges, its bounds and time steps."),
        ("Find problems", "Look for problems in {object}: outliers in its arrays (Find Outliers), bad cells (Mesh Quality, Validate Cells), values that are not numbers. Say what you find."),
        ("Slice and color", "Slice {object} through its middle and color the slice by its most interesting array, with a fitting color map."),
        ("Plot over time", "Plot the maximum of the main array of {object} over time and say where it changes (Find Change Points)."),
    ]
    .iter()
    .map(|(n, t)| PromptTemplate { name: n.to_string(), text: t.to_string() })
    .collect()
}

/// Skills: Markdown files whose text joins the assistant's instructions (house rules, a lab's
/// usual steps, naming conventions). Returns (file name, text) of those not switched off.
pub fn load_skills(dir: &std::path::Path, off: &[String]) -> Vec<(String, String)> {
    let mut v: Vec<(String, String)> = std::fs::read_dir(dir)
        .map(|rd| {
            rd.filter_map(|e| e.ok())
                .map(|e| e.path())
                .filter(|p| p.extension().is_some_and(|x| x.eq_ignore_ascii_case("md") || x.eq_ignore_ascii_case("txt")))
                .filter_map(|p| {
                    let name = p.file_name()?.to_str()?.to_string();
                    let text = std::fs::read_to_string(&p).ok()?;
                    Some((name, text))
                })
                .collect()
        })
        .unwrap_or_default();
    v.sort_by(|a, b| a.0.cmp(&b.0));
    v.retain(|(n, _)| !off.contains(n));
    v
}

/// The skills' text to add to the system prompt (empty when there are none).
pub fn skills_prompt(skills: &[(String, String)]) -> String {
    if skills.is_empty() {
        return String::new();
    }
    let mut s = String::from("\n\nThe user's skills (follow them where they apply):\n");
    for (n, t) in skills {
        s += &format!("\n## {n}\n{}\n", t.trim());
    }
    s
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
            local_only: false,
            redact_paths: false,
            prompts: default_prompts(),
            skills_off: Vec::new(),
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
    /// Pictures sent with the message (PNG, base64), for models that see images.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub images: Vec<String>,
}

impl ChatMessage {
    pub fn new(role: Role, content: impl Into<String>) -> Self {
        Self { role, content: content.into(), images: Vec::new() }
    }
}

/// Why a configuration may not be used in local-only mode (None: it may).
pub fn local_only_refusal(cfg: &AiConfig) -> Option<String> {
    if !cfg.local_only {
        return None;
    }
    if cfg.provider != ProviderKind::Ollama {
        return Some(format!(
            "Local-only mode is on: {} is a hosted service. Use Ollama on this computer or network, or turn local-only off in the assistant's settings.",
            cfg.provider.label()
        ));
    }
    if cfg.model.ends_with(":cloud") || cfg.model.ends_with("-cloud") {
        return Some(format!("Local-only mode is on: {} runs in the cloud. Choose a model that runs on your Ollama server.", cfg.model));
    }
    let host = cfg.ollama_url.split("://").nth(1).unwrap_or(&cfg.ollama_url).split(['/', ':']).next().unwrap_or("").to_string();
    let private = host == "localhost"
        || host.starts_with("127.")
        || host.starts_with("10.")
        || host.starts_with("192.168.")
        || host == "::1"
        || host.split('.').nth(1).and_then(|b| b.parse::<u8>().ok()).is_some_and(|b| host.starts_with("172.") && (16..=31).contains(&b))
        || host.ends_with(".local")
        || !host.contains('.');
    (!private).then(|| format!("Local-only mode is on: the Ollama server {host} is not a local or private address."))
}

/// The home folder as ~ and absolute paths as their file names: `/home/ann/runs/a/out.e` -> `out.e`.
pub fn redact_paths(text: &str) -> String {
    let home = std::env::var("HOME").ok().filter(|h| h.len() > 1);
    let t = match &home {
        Some(h) => text.replace(h.as_str(), "~"),
        None => text.to_string(),
    };
    let mut out = String::with_capacity(t.len());
    let mut word = String::new();
    let flush = |w: &mut String, out: &mut String| {
        let is_path =
            (w.starts_with('/') || w.starts_with("~/") || (w.len() > 2 && w.as_bytes()[1] == b':' && (w.as_bytes()[2] == b'\\' || w.as_bytes()[2] == b'/')))
                && w[1..].contains(['/', '\\']);
        if is_path {
            out.push_str(w.rsplit(['/', '\\']).next().unwrap_or(""));
        } else {
            out.push_str(w);
        }
        w.clear();
    };
    for c in t.chars() {
        if c.is_whitespace() || matches!(c, '"' | '\'' | '(' | ')' | ',' | '=' | '[' | ']') {
            flush(&mut word, &mut out);
            out.push(c);
        } else {
            word.push(c);
        }
    }
    flush(&mut word, &mut out);
    out
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
    if let Some(why) = local_only_refusal(&cfg) {
        let _ = tx.send(StreamEvent::Error(why));
        return rx;
    }
    let (system, messages) = if cfg.redact_paths {
        (redact_paths(&system), messages.into_iter().map(|m| ChatMessage { content: redact_paths(&m.content), ..m }).collect())
    } else {
        (system, messages)
    };
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

/// A message as each provider takes it, pictures included.
fn ollama_msg(m: &ChatMessage) -> serde_json::Value {
    let mut v = serde_json::json!({"role": role_str(m.role), "content": m.content});
    if !m.images.is_empty() {
        v["images"] = serde_json::json!(m.images);
    }
    v
}

fn anthropic_msg(m: &ChatMessage) -> serde_json::Value {
    if m.images.is_empty() {
        return serde_json::json!({"role": role_str(m.role), "content": m.content});
    }
    let mut parts: Vec<serde_json::Value> =
        m.images.iter().map(|b| serde_json::json!({"type": "image", "source": {"type": "base64", "media_type": "image/png", "data": b}})).collect();
    parts.push(serde_json::json!({"type": "text", "text": m.content}));
    serde_json::json!({"role": role_str(m.role), "content": parts})
}

fn openai_msg(m: &ChatMessage) -> serde_json::Value {
    if m.images.is_empty() {
        return serde_json::json!({"role": role_str(m.role), "content": m.content});
    }
    let mut parts = vec![serde_json::json!({"type": "text", "text": m.content})];
    parts.extend(m.images.iter().map(|b| serde_json::json!({"type": "image_url", "image_url": {"url": format!("data:image/png;base64,{b}")}})));
    serde_json::json!({"role": role_str(m.role), "content": parts})
}

fn role_str(r: Role) -> &'static str {
    match r {
        Role::User => "user",
        Role::Assistant => "assistant",
    }
}

fn stream_ollama(cfg: &AiConfig, system: &str, messages: &[ChatMessage], tx: &Sender<StreamEvent>) -> Result<()> {
    let mut msgs = vec![serde_json::json!({"role": "system", "content": system})];
    msgs.extend(messages.iter().map(ollama_msg));
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
        "messages": messages.iter().map(anthropic_msg).collect::<Vec<_>>(),
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
    msgs.extend(messages.iter().map(openai_msg));
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
        // A block of nothing but comments and blank lines does nothing: it is not code to run (and in
        // agent mode it would keep the loop going after the task is done).
        let code = body[..end].trim_end();
        if (lang.is_empty() || lang == "python" || lang == "py") && code.lines().any(|l| !l.trim().is_empty() && !l.trim_start().starts_with('#')) {
            out.push(code.to_string());
        }
        rest = &body[end + 3..];
    }
    out
}

/// The system prompt: tells the model what auraFX is and how to script it.
pub fn system_prompt(pipeline_description: &str, filter_catalog: &str) -> String {
    format!(
        "You are the built-in assistant of auraFX, a scientific visualization, analysis and simulation application written in Rust.\n\
You help the user visualize and analyze scientific data (meshes, particles, fields, time series).\n\
When an action is needed, answer with a short explanation and ONE ```python code block that uses the embedded `afx` module; \
the user can run it with one click. Keep code minimal. Prefer method chaining on nodes.\n\n\
API cheat sheet:\n\
- afx.load(path) -> Node (numbered files like run_0001.vtk open with their siblings as a time series; Exodus, VTK legacy/XML/.vtkhdf, EnSight .case, CGNS .cgns, Xdmf .xmf/.xdmf, NetCDF CF .nc (2-D lon/lat like POP's TLONG/TLAT too; reader option variables='(nlat, nlon)' picks the grid), CAM-SE .nc with its connectivity file found beside it (reader options connectivity='file.nc', variables='Surface [time, ncol]'|'Midpoint layers [time, lev, ncol]'|'Interface layers [time, ilev, ncol]', all_levels=True, level=0, layout='Lat/Lon map'|'Sphere') and MPAS .nc (MPAS reader options: node.set(vertical_level=2, multilayer=True, layer_thickness=0, atmosphere=False, layout='Lat/Lon map', center_lon=180)), OpenFOAM, Tecplot .dat/.plt, PLOT3D .xyz/.q, LAMMPS, VASP POSCAR/CHGCAR, XYZ, PDB, Gaussian .cube, STL/PLY/OBJ, CSV, PNG/JPEG/TIFF images, molecules (PDB, XYZ, VASP, .cml: atoms with atomic_number and bonds as lines; they open with representation='Molecule' colored by element with cmap='Elements'; node.set(molecule_style='Ball and Stick'|'Van der Waals'|'Liquorice', atom_scale=0.3, bond_radius=0.075, bond_orders=True); node.compute_molecule_bonds(tolerance=0.45, absolute=True)), hyper tree grids (.htg; afx.hyper_tree_grid(dimensions=(3, 3, 1), max_depth=3, descriptor='R... | .R.. | ....'), afx.random_hyper_tree_grid(seed=3, masked_fraction=0.25); node.save('tree.htg') keeps the grid; node.hyper_tree_grid_to_unstructured_grid(), node.hyper_tree_grid_to_dual_grid() leaf centers joined into cells with point data, node.hyper_tree_grid_geometry() outer surface and walls next to masked cells (grids with material interfaces are drawn cut to their material; fill_material=False gives the 2-D interface lines), node.hyper_tree_grid_feature_edges() outline/corner edges, node.hyper_tree_grid_depth_limiter(depth=2), node.hyper_tree_grid_generate_fields(), node.hyper_tree_grid_evaluate_coarse(operator='Average'); other filters work on the visible leaves), cell grids (.dg discontinuous Galerkin fields: HGRAD/HCURL/HDIV/constant on DG cells; drawn and filtered as linear cells with the fields at the corners; node.cell_grid_centers() exact values at cell centers, node.cell_grid_elevation(axis=(0, 0, 1)), node.cell_grid_to_unstructured_grid(); node.cell_grid_transform(translate=, rotate=, scale=), node.cell_grid_warp(attribute='hgrad', scale=0.5), mesh.convert_to_cell_grid(), node.cell_grid_probe(points_node, attribute='curl1') cells, parametric coordinates and values at another dataset's points, node.cell_grid_compute_sides() outer faces of 3-D cells drawn instead of the cells (include='Edges of inputs'|'Vertices of inputs'|'Surfaces of inputs'|'All sides', omit_renderable=False to also get sides of 2-D cells, preserve_renderable=False, strategy='Boundary'|'Odd count'|'Every side'), node.save('x.dg'); afx.cell_grid(cell_type='Hexahedron') test source), AMR data (AMReX/BoxLib plotfile folders or their Header, VTK .vthb, FLASH HDF5 plot/checkpoint files (no extension needed), Chombo .hdf5/.h5 (cell or node centered; ghost cells dropped; data='Particles' for its particles; 4-D files give a 2-D grid with per-cell velocity-space arrays, _vpar, _mu and <component>_sum), Enzo dumps via their .hierarchy file (reader option cgs=True converts to CGS units; node.set(data='Particles') reads FLASH/Enzo particles as points instead of the grid): levels of image boxes, covered coarse cells hidden via vtkGhostType; representation='Outline' shows the boxes; afx.amr_gaussian_pulse() test source; node.overlapping_amr_level_ids(); node.amr_contour(array='density', values=[0.3]) crack-free isosurface of a cell array across levels (node.contour on AMR cell data does the same); node.amr_dual_grid() stitched mesh of cell centers for crack-free clip/slice; node.resample_amr(samples=(64, 64, 64)) one uniform grid from the finest cells, whole_domain=False with min=/max= for a box, to_nodes=False for cell data), GIS vectors (.shp shapefiles, .geojson, .kml, .kmz, .gpkg GeoPackage, .fgb FlatGeobuf, .gml GML, MapInfo .tab and .mif, Esri file geodatabase .gdb folders: points/lines/polygon outlines with attributes as cell data, a block per layer; reader options polygons='Filled' fills areas to color by an attribute, layer='name', blocks='One per feature'), GeoTIFF, Esri ASCII grid (.asc) and ERDAS Imagine (.img) rasters at map coordinates (bands as cell data; for terrain: .cell_data_to_point_data().warp_by_scalar(array='Elevation')), EBSD maps (.ang TSL/EDAX square grids, and hexagonal grids read on a square grid of the x step by nearest point with field data ResampledFromHexGrid, .ctf Oxford): image data with one cell per point, cell arrays EulerAngles (Bunge, radians), Phases (0 = not indexed) and the quality columns (.ang: Image Quality, Confidence Index, SEM Signal, Fit; .ctf: Bands, Error, MAD, BC, BS), field arrays CrystalStructures (crystal class per phase, index 0 = not indexed, 999 unknown), MaterialName and LatticeConstants; .ctf reader option hexagonal_axes='a1 along x (+30 degrees on phi2)' (default) or 'As stored'; then map.orientation_colors() gives IPFColor; .h5ebsd stacks of TSL or Oxford slices (one volume, slices Z Resolution apart; the sample and Euler transformations are kept as field arrays, not applied), EDAX OIM .h5 and Bruker Esprit .h5 scans (reader option scan='Scan 1'), .dream3d microstructures (image, rectilinear, vertex, edge, triangle, quad, tet and hex geometries; cell arrays as cell data, vertex arrays as point data, feature and phase arrays as field data; reader options data_container='name' and arrays='EulerAngles, Phases' to read only some cell arrays of big volumes); afx.load('slices/img_*.png') stacks images into a volume; medical and microscopy volumes as image data with their spacing, origin and orientation: NRRD .nrrd/.nhdr (array NRRDImage), MetaImage .mha/.mhd (MetaImage), MRC/CCP4 maps .mrc/.map/.rec (Scalars; axis order put back to x y z), DICOM .dcm files or a folder of a series (DICOMImage, rescaled by slope and intercept, slices ordered along the normal; compressed DICOM is not read), OME-TIFF .ome.tif (one array per channel named after it, time steps for T > 1); lidar point clouds LAS .las and LAZ .laz (points with Intensity, ReturnNumber, NumberOfReturns, Classification (2 ground, 5 vegetation, 6 building), ScanAngle, UserData, PointSourceId, GpsTime, Color); scenes glTF .gltf/.glb and VRML 2.0 .wrl (a block per mesh, moved by its transforms, Colors and Normals) and OFF .off (COFF colors); H5Part .h5part (Step# groups as time steps of points) and openPMD .h5 (iterations as time steps: block meshes as image data, a block per particle species); Nek5000 .nek5000 or name0.f00001 (spectral elements as hexahedra; Velocity, Pressure, Temperature, S01...); LS-DYNA d3plot with its family files d3plot01... (states move the nodes: Displacement, Velocity, Acceleration, Stress (6), EffectivePlasticStrain, BeamResultants, Material, ElementId, ElementKind 1 solid 2 thick shell 3 beam 4 shell; field KineticEnergy...); Nastran bulk data .bdf/.nas/.bulk (or .dat with GRID cards; property = ElementBlock, NodeId, ElementId; CORD2R/C/S systems) and Fluent .msh/.cas (cells rebuilt from faces; cell zones as ElementBlock, boundary zones as SideSet <zone>); a .pvtu/.pvtp/.pvti read as one mesh, or as one block per piece with node.set(pieces='Partitions')); afx.active() -> Node or None; afx.nodes(); afx.get(name)\n\
- node.<filter>(**params) -> Node for any filter id below (e.g. node.slice(normal=[0,0,1]), node.contour(values=[0.5]), node.threshold(lower=1, upper=2), node.clip(type='Sphere', radius=2), node.calculator(expression='mag(velocity)', result='speed'), node.glyph(orient='velocity'))\n\
- node.set(**params_or_display) ; display keys: visible, opacity, colormap, representation ('Surface','Wireframe','Points','Surface With Edges','Outline','Volume','Surface LIC'), volume_opacity, volume_threshold, lic_vectors (array name; Surface LIC shows a vector field as streaks on surfaces, e.g. a slice of a velocity field), lic_strength, lic_length, texture='/path/image.png' (shows an image on a surface with texture coordinates: first node.texture_map_to_sphere() / texture_map_to_plane(automatic=True) / texture_map_to_cylinder(prevent_seam=False); texture=None removes it), point_size, line_width (pixels, for lines, wireframes and edges), name\n\
- node.color_by('array', assoc='points'|'cells', cmap='Viridis', range=(lo, hi)); node.show(); node.hide(); node.delete()\n\
- node.info() -> dict; node.arrays(); node.array('name') -> numpy; node.points -> numpy (N,3) (multi-block data is merged automatically); node.add_array('name', numpy_array); node.save('out.vtu') (also .vtk .vtkhdf .exo .case .xmf .cgns .csv .stl .ply; meshes as .inp Abaqus, .msh Gmsh, .cas Fluent (or name.fluent.msh), .bdf Nastran; image data as .mha/.mhd MetaImage, .png (one picture per slice) or .tif (one multi-page TIFF); node.save('run.pvtu', pieces=8) writes a parallel file with a folder of pieces (a partitioned multi-block writes one piece per block), node.save('run.pvd', all_times=True) one VTK file per time step and the .pvd listing them); afx.probe(node, [x,y,z]) -> dict\n\
- afx.<source>(**params) for sources (sphere, cone, wavelet, plane, line, box, cylinder, point_source)\n\
- afx.time(index); afx.times(); afx.view('+x'|'-y'|'iso'|[x,y,z]); afx.reset_camera(); afx.screenshot('file.png', width=1920, transparent=False) (waits until saved); afx.export_scene('scene.glb' or '.obj', '.x3d', '.wrl', or '.html' for a web page that shows the scene in 3D in any browser); afx.screenshot('figure.svg') writes a vector picture; afx.describe(); afx.run()\n\
- afx.plot(x, y, label=..., view=0); afx.over_time(node, 'array', stat='max'); afx.show_panel('Charts'); several chart views at once, each plotting its own object: n = line.plot('T', 'p', x='arc_length', style='lines'|'points'|'markers'|'bars', data='auto'|'points'|'cells', log_x=False, log_y=False, title='...') opens a new chart view of that object and returns its number, afx.chart_view(node=None, y=[...], x=..., style=..., view=None) opens one (node=None follows the selected object) or changes view n (view=0 is the Charts panel), afx.chart_views() lists them, afx.close_chart_view(n), afx.plot(x, y, view=n) adds a line to chart view n; in the window: View > New Chart View, the Pipeline's right-click Plot in a new chart view, and each chart's first box chooses its object; pictures of several views as laid out: afx.screenshot('slide.png', views='all'|'charts'|'3d'|'chart'|'chart 2'|'view 2', transparent=False, controls=False) (every visible 3D view and chart view in one picture with the window's pixels; chart controls left out unless controls=True), afx.save_animation('all.mp4', views='all') records every visible view as laid out (also with cue=...); .webm files (VP9) are written too, like .mp4 (ffmpeg on the PATH); node.save('part.obj'|'part.glb'|'part.geo') writes a surface as OBJ, binary glTF or Houdini text geometry; afx.set_shortcut('screenshot', 'Ctrl+P') binds keys to a window action (open, palette, save_state, screenshot, undo, redo, reset_camera, delete_node, play, next_step, prev_step, settings, view_x, view_y, view_z, view_iso), afx.set_shortcut(vim=True) turns on h j k l view keys; afx.set_theme('high contrast'); 3D Glyphs representation: node.set(representation='3D Glyphs', glyph_shape='arrow' or 'cone' or 'sphere' or 'cube', glyph_orient=vector array, glyph_scale_by=array, glyph_scale=size of the largest or 0 for automatic), drawn by the GPU per point (not picked, no shadows); big surfaces: chunks outside the view are skipped and surfaces over a million triangles are drawn coarse while the camera moves; GPU for filters: Slice and Contour of big grids use the GPU to find the cells to cut (same results), Settings > Use the GPU for filters, afx.gpu_filters(False) turns it off, afx.gpu_benchmark(100) times them with and without; files larger than memory: afx.load(path, stream=16) reads the file in 16 pieces one after another in one server process (VTK XML grids and .vtu read only each piece; other formats are read whole for each piece), the Open on Server window: started by > one process, one piece at a time; parallel servers: Gradient, Cell Data to Point Data, Surface Normals, Curvature and Compute Derivatives run on every process with ghost cells from the neighbouring pieces (same results as one process); filters needing the whole data run on the main process; batch schedulers: afx.submit(cmd, host=..., slurm={{'partition': 'short', 'time': '00:30:00', 'nodes': '2', 'tasks': '64'}}) uses Slurm; add 'system': 'pbs' or 'lsf' (and 'queue') for PBS or LSF; the Simulations panel has the same choice under through a scheduler; recovery: the pipeline is saved every half minute and File > Restore Last Session brings it back; after a crash the next start shows where the report file is (~/.cache/aurafx/crashes, nothing is sent); finding commands: right-click an object in the Pipeline or the 3D view for filters suggested for its data (others are faded with the reason), Ctrl+K ranks by the selected object, Help > Welcome and Help > User guide, the Model menu starts a CAD part or a Geometry source; in the CAD environment the Next strip at the top of the model tree says what to do, S searches CAD commands, right-click a face for what can be done with it, Inspect measures the distance between two faces, Files > Export writes STEP, IGES, 3MF or STL; a Geometry source's Properties have Insert menus for its commands and the steps From geometry to a mesh; units: arrays may carry units (NetCDF units are read; the Set Units filter or data.point_data.set_units(name, units) sets them; node.info() lists them; the legend shows name (units); no conversion); notebooks: afx.snapshot(width=640, height=400) shows a picture of the 3D view in a notebook (views='all' for every view), afx.display(path or bytes or object) shows a picture or text; the Notebook panel (Windows > Notebook) runs cells and opens and saves .ipynb files; aurafx --install-jupyter-kernel makes Jupyter list auraFX (Python) as a kernel that drives the window; plugin panels: @afx.panel('Title') on a function of ui (ui.label, ui.button(text) True when pressed, ui.slider(name, value, lo, hi), ui.number, ui.checkbox, ui.text, ui.choice(name, options), ui.image(path), ui.separator()) adds a window under Windows > Plugin panels, run again on each press or change; Rust plugins: shared libraries built with the aurafx-plugin crate (export_plugin!, Filter::new, Filter::source, Param) in the plugin folder or afx.load_plugin('libx.so'); afx.assistant_prompt('Find problems') puts a saved prompt (Settings > AI assistant > Prompts; the word object in braces stands for the selected object) in the box; skills are Markdown files in the skills folder beside the settings whose text goes with every request; readers also include NIfTI (.nii, .nii.gz, .hdr with .img: array NIFTI, scaled, placed by sform or qform) and PTS point clouds (.pts: Intensity, Colors, ScanId); File > Save Screenshot and File > Animation have the same choice (the active 3D view, all views, chart views), and each chart has a Picture button; afx.hide_panel('Log'); afx.layout('analyze'); afx.set_theme('light'); afx.split_view('right'|'down') adds a 3D view (it becomes active; node.show()/hide() apply to the active view); afx.active_view(i); node.send_to_view(i | 'right' | 'down', keep=False) moves an object to view i or into a new view of its own; node.set(axes_grid=True) draws a labelled axes grid around an object; keyframe animation: afx.key_camera(t) keys the current view at t in [0,1], node.key('param', t, value, interp='ease in-out') keys a filter value or a display property ('opacity', 'solid_color', 'point_size', ...; interp: linear, step, ease in, ease out, ease in-out, exponential), afx.key_camera(t, interp=...), afx.animation(frames=120, time=True), afx.animation_time(t) previews, afx.save_animation('out.mp4', fps) records keyframes when any exist, afx.save_animation('out.gif', cue=lambda t: ..., frames=60) calls a Python function before each frame and returns when the file is written, pictures and animations take a size, a shape and a background: afx.screenshot('fig.png', width=1920, height=1080, background='white') (background: 'screen', 'transparent', 'white', 'black', '#rrggbb', (r, g, b), or [bottom, top] for a gradient; width and height together set the shape, one alone keeps the view's), afx.save_animation('a.mp4', orbit=72, width=1920, height=1080, background=['white', '#6b8cbd']) the same for the frames (no transparent), with views='all' the background colors the space between the views; File > Save Screenshot and File > Animation have Picture of / Record (active 3D view, all views, the 3D views, 3D view N, chart views), Shape (the view's, 16:9, 4:3, 1:1, 9:16 ...), Size and Background (as on screen, transparent, white, black, color, gradient); afx.install_ffmpeg() downloads the video encoder MP4 and WebM need when ffmpeg is not installed (the Animation window has the same button; GIF and PNG frames need nothing), poster='clip.jpg' on any save_animation also writes the first frame as a picture (.jpg or .png, the still a web page shows before a video plays); OpenFOAM boundary patches: case.openfoam_patches(patches='inlet,walls'); node.save('out.exo', all_times=True) writes Exodus with every time step (also .vtkhdf, EnSight .case, Xdmf .xmf, .cgns); color maps: node.set(cmap='Cool to Warm', cmap_reverse=True, cmap_bands=8, cmap_points=[[0, r,g,b], ..., [1, r,g,b]]), afx.define_colormap(name, points), afx.import_colormaps('paraview.json'), afx.show_panel('colormap') opens the editor; afx.link_views(True); afx.match_views(); curved (higher-order Lagrange/Bezier/quadratic) cells: node.set(nonlinear_subdivision=2) draws them smoother, node.tessellate(level=2) turns them into linear cells, afx.unstructured_cell_types(cell_type='Lagrange Hexahedron', blocks=(2, 2, 2), order=3) makes test cells, info()['cell_types'] lists cell types; data on another machine: afx.load('/path/on/host.e', host='cluster') (or 'ssh://cluster/path') reads and filters it there with aurafx-server and brings back only what is shown, node.set(all_data=True) brings everything, afx.servers() lists connections, node.set(draw_on='server') has the server's GPU draw it (only pictures come over), afx.load(path, host='cluster', processes=8) spreads the data over 8 server processes (launcher='mpirun' or 'srun' or a command with {{n}} and {{cmd}} starts them through MPI or Slurm; the ranks connect back to the server); afx.lighting(edl=True) turns on Eye-Dome Lighting (good for point clouds/particles), afx.lighting(shadows=True, ssao=True) adds shadows and ambient occlusion, afx.lighting(lights=[{{'direction': (0, 0, 1), 'color': (1, 0.9, 0.8), 'intensity': 0.8, 'camera_relative': False}}]) sets custom lights, afx.lighting(fxaa=True, tone_mapping='aces', exposure=1.2) for the final picture, afx.lighting(ray_tracing=True, ray_samples=256, ray_device='auto'|'gpu'|'cpu') path-traces opaque surfaces (screenshots wait for it), afx.lighting(environment='sky'|'studio'|'white'|'/path/map.hdr', environment_intensity=1, show_environment=True) lights with an environment map, node.set(metallic=1, roughness=0.2, specular=0.5) sets the material; arrays of 8-bit values with 3 or 4 components (like IPFColor) are drawn as their own colors when colored by: node.set(color_by='IPFColor'); node.set(direct_colors=False) maps them through the color map instead; geometry: g = afx.mesh.geometry(); g.cmd('brick x 4 y 2 z 1'); g.cmd('cylinder radius 0.5 height 2; subtract body 2 from body 1'); g.bodies() lists bodies with history and faces; g.journal(); part = g.result(resolution=96) adds the Geometry source (afx.geometry(commands=...)), whose surface has cell arrays BodyId and FaceId and field arrays FaceNames, Volume, SurfaceArea (output='Distance field' gives the signed distance grid); geometry commands, one per line or separated by ; (sizes in model units, angles in degrees, primitives centred on the origin, round ones along z): brick x 4 y 2 z 1 | brick 2, sphere radius 1, cylinder radius 0.5 height 2, cone radius 1 top 0.4 height 2, torus major 2 minor 0.5, prism sides 6 radius 1 height 2, superquadric x 1 y 0.8 z 0.6 e1 0.5 e2 0.7, harmonic radius 1 coefficient l m c ..., import 'part.stl' scale 0.001; outline 1 polyline x y ... | rectangle x 2 y 1 center 0 0 | circle radius 1 | ellipse x 2 y 1 | regular sides 6 radius 1 | spline x y ...; outline 1 add line x y ... | add arc center 0 0 radius 1 start 90 end -90; outline 1 fillet radius 0.2; extrude outline 1 height 2; revolve outline 1 angle 360 (x = distance from the z axis); sweep outline 1 path x y z x y z ...; move body 2 x 1 y 0 z 0.5; rotate body 2 angle 30 about z|x|y|ax ay az center x y z; scale body 2 1.5 | scale body 2 x 2 y 1 z 1; unite body 1 2; subtract body 2 from body 1 [keep]; intersect body 1 2; webcut body 1 with plane xplane|yplane|zplane offset d (or with plane normal x y z offset d, with cylinder radius r axis z center x y z, with body 3, with sweep outline 4 direction 0 0 1 (an open curve swept through the body; the left of the curve keeps the number), with sheet 'cut.stl' (an open surface; the side its triangles face away from keeps the number)) cuts a body in two sharing the face webcut N (a brick cut along its own axis gives two bricks, both still structured); imprint body 1 2 (or imprint all) meshes touching bodies with one size so they meet node to node; merge body 1 2 shares their interface nodes and refuses meshes that do not meet node to node; faces between two bodies' elements become side and node sets named interface <body> - <body>; copy body 1; delete body 2; body 1 name 'bracket'; face 6 name 'inlet' (faces are numbered; names like brick 1 z+, cylinder 2 side, extrusion 3 edge 1 keep through booleans); node.signed_distance(resolution=128, margin=0.1) turns a closed surface into a Distance grid; hexahedral meshes: afx.box_mesh(size=(2, 1, 1), cells=(40, 20, 20), grading=(3, 1, 1), cluster='Start', element='HEX8'|'HEX20'|'HEX27'), afx.inclusion_mesh(size=(1, 1, 1), cells=24, count=8, radius=0.075, radius_std=0, shape='Sphere'|'Ellipsoid'|'Superquadric'|'Harmonic grain', aspect=(1, 0.7, 0.5), exponents=[4, 4], roughness=0.12, seed=7, periodic=False, element='HEX8', separate_blocks=False) packs inclusions in a box and meshes them with conforming hexahedra (blocks 1 matrix and 100 inclusions, side sets x- x+ y- y+ z- z+ and interface, node sets x- ... z+, element variable particle_id; periodic adds node sets x-_node_set/x+_node_set ... and PeriodicPairs); generated meshes keep the cell array ElementBlock and field arrays ElementBlockIds, ElementBlockNames, SideSet <name> (element, Exodus side), NodeSet <name>; mesh.extract_set(set='interface') shows a side set or node set; mesh.elevate(element='HEX27'); mesh.mesh_quality() adds the hexahedron metrics (ScaledJacobian, Skew, Taper, MaxAspectFrobenius, Oddy, Shear, Shape, Distortion, ...) and mesh.mesh_quality(output='Summary table') gives min/max/mean and acceptable ranges per metric; afx.plate_mesh(size=(100, 50, 1), cell=2.5, hole='None'|'Circle'|'Ellipse', a=2.5, b=2.5, angle=0, centre=[50, 25]) (blocks plate and ring, sets x- ... z+, outer, grips, hole); afx.dogbone_mesh(length=25.4, gage_length=8, gage_width=1.85, grip_width=4.8, fillet_radius=3.175, thickness=0.35, stations=[0, 0.37, 4, 0.35], cell=0.2, grip_cell=0.4, layers=1) (blocks gage, shoulder, grip; node sets grips, gage-, gage+, station_k ...); surface.sweep_quads(direction=(0, 0, 1), distance=1, layers=4, grading=1) sweeps quadrilaterals into hexahedra (side sets bottom, top, side); afx.inclusion_mesh(packing='Collective') packs denser; afx.geometry(commands=..., output='Tet mesh') fills the bodies of mesh body commands with tetrahedra (side set per face); surface.tet_mesh(cell=0) fills a closed surface with tetrahedra; tets.refine_region(box_min=(...), box_max=(...)) or (array='name', value=1) refines part of a tetrahedral mesh conformingly; mesh.assign_grains(method='Voronoi'|'Nearest point'|'From a volume', grains=30, size_sigma=0, orientations='Random', grain_array='grain_id', blocks=False, boundaries=False, seed=1) gives cell arrays grain_id and EulerAngles (second input: points or a table x y z grain_id [phi1 Phi phi2], or a volume with FeatureIds; field GrainEulerAngles, GrainSeeds); mesh.save('grains.tex') writes a texture file (4 header lines, then phi1 Phi phi2 weight in degrees per grain id from 1); volume_or_mesh.save('x.dream3d') writes a .dream3d file recording the pipeline (node.pipeline_json() gives it); volume.save('grains.spparks') writes a SPPARKS site file (SPPARKS .spparks/.sites files and lattice dumps id i1 x y z open as volumes with FeatureIds); afx.import_pipeline('pipeline.json' or 'file.dream3d', input_node) builds a pipeline from a numbered-step pipeline file (File > Import Pipeline), mapping steps to auraFX filters and printing skipped ones; afx.simulation_case(node, 'moose'|'spparks'|'pinnacle'|'damask'|'fft'|'lammps', folder, material='Aluminium', axis='x', strain=0.02, steps=50, end_time=1, fixed='', loaded='', mc_steps=100, temperature=0, dump_every=10, seed=12345, lattice_constant=0, angstrom_per_unit=0) writes a solver case (damask, fft and lammps take a microstructure volume with FeatureIds and orientations: damask writes grid.vti, material.yaml with one material per grain as a quaternion, load.yaml; fft writes micro.txt, phi1 Phi phi2 x y z grain phase per voxel; lammps fills every grain with the material's FCC/BCC lattice at its orientation and writes atoms.data, atom_grains.txt, atoms.dump colored by grain, in.lammps; the others take a grain mesh: moose: mesh.e with one block per grain, grain_euler.txt, grains.tex, slip_systems.txt, polycrystal.i; pinnacle: mesh.e, orientations_points.csv, case.i) or a volume with FeatureIds (spparks: grains.spparks, in.potts) and returns folder, files, input, command, outputs; afx.run_case(node, solver, workdir, host=None, exe=None, np=1, auto_load=True, wait=False, slurm=None, **case options) writes and runs it (files uploaded to remote hosts) and opens the results; afx.simulation_template('pinnacle') opens the Simulations panel on a template (MOOSE polycrystal, SPPARKS grain growth, PINNACLE crystal plasticity write their files from the selected node); grains on a PINNACLE result: result.assign_grains(afx.load('orientations_points.csv'), method='Nearest point'); overlay_mesh of a volume with orientations also gives grain_id and EulerAngles; mesh.refine_mesh(levels=1) splits hexahedra or tetrahedra into 8 each (sets kept); mesh.convert_elements(to='Hexahedra to tetrahedra'|'Tetrahedra to hexahedra') (12 tets per hex, 4 hexes per tet); mesh.smooth_mesh(scheme='Smart Laplacian'|'Laplacian'|'Condition number', iterations=10, fixed='nodeset') holds the boundary; mesh.boundary_layers(side_set='z+', layers=3, first=0.01, growth=1.3) inserts wall layers; mesh_quality also gives the tetrahedron metrics (MinimumDihedralAngle, AspectRatio, RadiusRatio, Condition ...); image_or_surface.overlay_mesh(array='FeatureIds', cell=0, mode='Smooth'|'Stair-step', void=False, periodic=False, min_cells=8, sharp=True, element='HEX8') meshes a labelled image (one block per label, 0 empty) or a closed surface on an overlay grid with pillowed smooth interfaces (field data MinScaledJacobian, InvertedElements; side sets x- ... z+, surface <block>, interface <a> - <b>); geometry command mesh body 1 size 0.1 intervals 20 8 4 bias 3 1 1 cluster start|end|both sets a structured body's pieces and grading per axis; mesh body 3 size 0.1 scheme mapped meshes a ball-like or rounded body as one structured block; group 1 add body 1 2 face 6 name 'clamped' then mesh group clamped size 0.1, block 9 add group 1, sideset 4 add group clamped; thicken 'sheet.stl' thickness 0.2 makes a solid from an open surface; loft outline 1 outline 2 height 3 joins two outlines; sew 'a.stl' 'b.stl' tolerance 1e-4 closes patches into one body; geometry command mesh body 3 size 0.1 scheme overlay|stairstep|structured meshes any body (non-brick, non-cylinder bodies use overlay by default, sharing one grid); geometry commands mesh body 1 2 size 0.1 element HEX20, block 7 add body 2 name 'rod', sideset 1 add face 6 name 'top', nodeset 2 add face 1 name 'left' with afx.geometry(commands=..., output='Hex mesh') mesh bricks and cylinders (each face also a side set named after it); ebsd.pole_figure(families='100, 110, 111', projection='Stereographic'|'Equal area', sigma=7.5, phase=1) gives disks colored by MRD (multiples of random); ebsd.inverse_pole_figure(directions='X, Y and Z'|'X'|'Y'|'Z', sigma=5) the density on the standard triangle; ebsd.odf_sections(sections='0, 45, 65', step=5, sigma=7.5) ODF sections (phi1 across, Phi down) with field data TextureIndex and MaxODF; ebsd.misorientation_distribution(min_angle=5, bin=1) gives a table (bin_center, Neighbours, Random pairs, Uniform as densities per degree) for the Charts panel; ebsd.texture_components(families='100, 110, 111', axes='X, Y, Z', cone=15) a table of fractions; ebsd.chord_lengths(threshold=10, spacing=0, output='Chords'|'Histogram', bin=0) grain-boundary chords along x, y, z with field data MeanChordX, ChordsX ...; volume.quick_surface_mesh() gives boundary triangles (cell FaceLabels a b, normal from a into b, -1 outside; point NodeType 2/3/4, +10 on the outer faces; field AvgQuats, FeaturePhases); surface.smooth_boundaries(iterations=20, lambda=0.5, mu=-0.53 (0: Laplacian), triple_lambda=0.25, quad_lambda=0); surface.triangle_geometry() (FaceAreas, FaceNormals, FaceCentroids, FaceMinimumAngles; field SurfaceVolumes, SurfaceAreas per feature); surface.feature_surfaces(features=[]) one closed surface per feature (save as .stl writes one file per feature); surface.face_orientation_colors() (FaceIPFColors, FaceMisorientations, FaceMisorientationColors); surface.gbcd_section(angle=60, axis=(1, 1, 1), tolerance=5, sigma=5) the GBCD for one misorientation in multiples of random (field MisorientationMRD, MaxMRD); volume.crystal_mechanics(material='IN718'|'CoNi GammaPrint-700'|'Nickel'|'Copper'|'Aluminium'|'Iron (BCC)'|'Titanium (HCP)'|'Magnesium (HCP)'|'Custom', direction=(0, 0, 1), taylor=True, gnd='GND') adds SchmidFactor, SlipSystem, TaylorFactor, YoungsModulus and field means, YoungsModulusVoigt/Reuss/Hill, YieldEstimateMPa (custom: lattice='FCC'|'BCC'|'HCP', elastic=[...], families='Basal <a>=1, Prismatic <a>=0.7'); volume.srod(layer=40, plate_top=0, interface=40, output='Volume'|'Bins'|'By height') gives SROD per cell and per grain (GrainClass 0 plate 1 epitaxial 2 interface 3 originating, SRODRate degrees per layer); volume.gnd_density(method='Nye tensor'|'From KAM', burgers=2.54e-10, kernel=3) gives GND (1/m2); afx.voronoi_grains(cells=[64, 64, 64], spacing=1, grains=100, size_sigma=0 (Voronoi) or above (Laguerre), periodic=False, orientations='Random'|'<100> along Z'|'<111> along Z', spread=10, structure='Cubic m-3m', seed=1) and afx.columnar_grains(size=(300, 300, 450), voxel=5, width=50, height=150, height_sigma=0.5, tilt=6, orientations='<100> along Z') make synthetic microstructures (FeatureIds, Phases, EulerAngles); grains.orientation_noise(sigma=2, per_feature=False), grains.jumble_orientations(), grains.orientation_gradient(rate=0.05 (degrees per unit length along direction=(0, 0, 1)), rate_spread=0.5, axis='Random'|'Perpendicular to the direction'|'Fixed', reference='Centroid'|'Lowest point', amplitude=0.5 (smooth spread, degrees), correlation_length=0, cell_size=0 (sub-grain cells), cell_angle=1, size_exponent=0, cap=0) gives every grain an orientation that changes inside it as in additively manufactured material (SROD then measures rate x layer; keep cell_angle under 0.4 of the segment_features tolerance), grains.realistic_grains(relax=4, width=1.5, coarsen=0, roughness=0, roughness_length=0, periodic=False) makes tessellated grains look grown (boundaries relaxed towards 120 degree junctions, smallest grains removed, wavy boundaries; cell arrays follow, ids renumbered from 1); the sources take realism=0..1 and gradient=degrees per unit length: afx.voronoi_grains(..., realism=0.8, gradient=0.05) and afx.columnar_grains(..., realism=0.8, layer=40, competitive=..., epitaxial=..., scan_tilt=..., equiaxed=..., relax=..., roughness=..., gradient=0.03) (competitive growth of <100> columns, layers with new grains, lean, fine grains at layer boundaries), afx.pack_grains(..., gradient=0.05); grains.ensemble_statistics(include_surface=False) (table: Features, ESD mu, ESD sigma, b/a alpha, b/a beta, c/a alpha, c/a beta, Neighbors mu ...); afx.pack_grains(cells=[64, 64, 64], spacing=1, esd_mu=2.3, esd_sigma=0.2, ba=[15, 2], ca=[12, 4], shape='Ellipsoid'|'Superellipsoid'|'Cube-octahedron'|'Cylinder', aligned=False, fraction=1 (below 1: matrix phase 2), periodic=False, calibrate=True) packs grains from goal statistics; measured.pack_like(cells=[100, 100, 100]) packs a statistical copy; afx.show_panel('goal statistics') (View > Goal Statistics) edits the size and shape goals of a Pack Grains source with live plots of the goal distributions over the packed volume's measured ones, and Pack applies them; ebsd.save_pole_figures('folder', families='100, 110, 111', projection='Stereographic', sigma=7.5, size=256, phase=0) writes one PNG per phase and family (phase1_100.png ...) and returns the files with their largest MRD; grain_surface.save('grains.triangles') writes grains.nodes (id, type, x, y, z) and grains.triangles (id, three nodes, the two features on either side) text files; grains.insert_precipitates(fraction=0.05, esd_mu=1, esd_sigma=0.2, at_boundaries=False) adds a particle phase; synthetic.match_crystallography(reference) assigns orientations from the reference and matches its boundary misorientation distribution (field data MDFErrorBefore/After); texture filters take phase (0: all), mask and max_orientations, and save to CSV with a Block column; mesh.save('part.e'|'part.inp'|'part.msh') writes Exodus (blocks, side sets, node sets, HEX20/27), Abaqus input or Gmsh 4.1; grains: node.segment_features(tolerance=2, mask='SampleVoxels') writes the cell array FeatureIds, then .fill_bad_data(min_defect=7).minimum_size(min_cells=7) clean up, and feature_sizes, feature_centroids, feature_shapes, average_orientations, reference_misorientations, kernel_average_misorientation(kernel=(1, 1, 1)), feature_neighbors, distance_to_boundaries, merge_twins add per-feature results as field arrays (one row per feature id, row 0 for id 0: NumElements, Volumes, EquivalentDiameters, Centroids, AxisLengths, AspectRatios, Omega3s, AvgQuats, FeatureAvgMisorientations, NumNeighbors, SurfaceFeatures, ParentIds) or cell arrays (KernelAverageMisorientations, FeatureReferenceMisorientations, BoundaryCells, GBEuclideanDistances); node.feature_table() gives them as a table (Spreadsheet, CSV); node.feature_colors() gives FeatureColors (color_by='FeatureColors'); node.feature_boundaries() gives the faces between features with Misorientation and Twin\n\
- CAD models (a parametric part with named faces; filter id cad_model, Sources > CAD Model): m = afx.cad_model() starts a 40 x 30 x 8 mm plate: the dimensions width, depth, thickness (40 mm, 30 mm, 8 mm) and an extruded feature named Base that follows them, so to change the plate set the dimensions (m.cad.parameter('width', '60 mm')) instead of adding another plate; a feature name can be used once (a second one with the same name is refused); to change a part that already exists use ITS node (m = afx.get('CAD Model') or afx.active()), never a new afx.cad_model() (that starts another part): change a dimension with m.cad.parameter('width', '80 mm'), a feature's own number with m.cad.set('Hole 1', '/shape/diameter', 8) or make it follow a dimension with m.cad.parameter('hole_d', '8 mm') then m.cad.bind('Hole 1', '/shape/diameter', 'hole_d'), then read m.cad.volume() (m.cad.clear() also removes the dimensions, so do not use it to edit); m.cad.clear('Name') empties the part to start from nothing, afx.cad_model('part.afxcad') opens a document, afx.cad_model(template='flange') starts from a template (enclosure, flange, knob, threaded rod, vase, bracket); m.cad.parameter('width', '40 mm') names a dimension, and any number below may be an expression of parameters ('width / 2') so the geometry follows the parameter; solids: m.cad.box(size=(40, 20, 10), name='Base', op='add'|'cut'|'intersect', at=(x, y, z), rotate=(rx, ry, rz), corner_radius=0, blend=0), sphere(radius), cylinder(radius, height), cone(r_bottom, r_top, height), torus(major, minor), hole(diameter, depth, kind='simple'|'drilled'|'counterbore'|'countersink'|'tapped', cb_diameter, cb_depth, cs_diameter, cs_angle, pitch) cuts downward (-z) from at, thread(major_diameter, pitch, length, internal); sketches: s = m.cad.sketch(plane='XY'|'XZ'|'YZ', offset=0).rectangle(w, h, center=(0, 0)) or .circle(r, center) or .polygon(points) or .regular(sides, r), then m.cad.extrude(s, depth, name='Base', op='add', end='blind'|'midplane'|'through_all', draft=0, reverse=False, top={{'fillet': 1}}) or m.cad.revolve(s, angle=360) (x is the radius); modifiers: fillet(radius, feature=None) (feature None: the whole part), shell(thickness, open_top=True), linear_pattern(count, step), grid_pattern(counts, step), circular_pattern(count, total_deg, axis), mirror(plane), split(plane, offset), scale(factor); the timeline: suppress(name), remove(name), rename(name, to), move(name, to), bind(name, '/shape/size/0', 'width'), configuration('small', width=20) with activate('small'); reading: m.cad.faces() (name, kind, area, centroid, direction), volume(), features(), parameters(), info(); files: m.cad.save('part.afxcad'), m.cad.load(path); m.set(width=50) changes a dimension without editing the document and re-runs everything downstream; the node's surface has cell arrays BodyId and FaceId and field data FaceNames (faces of a sketch-extruded feature are named like Base top, Base bottom, Base side 1 (cad.faces() lists them), Base side 2 or Bore side) so extract_set, boundary layers and the meshers refer to faces by name; output='Distance field' gives the signed distance; mesh a part with overlay_mesh or tet_mesh; the Properties panel lists the dimensions (type an expression, press Enter), the features (the checkbox suppresses one) and the faces; the CAD environment (Sources > CAD Model..., Edit in CAD... on a CAD Model, or e = m.edit_cad()) is a mode of the window with a ribbon, a Model tree, a Feature panel and a timeline bar, and the pipeline is paused while it is open; inside it the same m.cad calls edit a working copy (e = m.edit_cad(); e.box(...); e.hole(...); e.evaluate() builds and shows it; e.rollback(n) builds only the first n features; e.undo(); e.redo(); e.select('feature name'); e.editing(); e.state()); e.apply() hands the part to the node as one undo step and resumes the pipeline (e.apply(keep_editing=True) stays), e.discard() puts everything back; a part that does not build is refused at apply with the reason; Apply first runs the downstream check: e.check() (or afx.cad_check(m)) returns changed, diff (text), downstream (node ids), conflicts (node, node_name, param, value, reference, kind, may_reference, fix with value and reason and check) and prompt; the check finds face names, body names and face numbers in the parameters and command text of the nodes below, and positions inside the old bounds that are outside the new ones, and gives default fixes (a renamed face is renamed everywhere, a gone face with one successor is replaced and marked check, a position is moved onto the part); e.apply(fix='default'|'none'|[{{'node': 7, 'param': 'set', 'value': 'Pin hole side'}}, ...]|answer_text, run='now'|'later') applies the part with those fixes as one undo step and returns features, fixes and held, e.run_held() runs nodes a run='later' held back; the sketch canvas (ribbon Sketch tab: New sketch on Top (XY) | Front (XZ) | Right (YZ) | Top of the part, Sketch here on a flat face in the feature panel, Edit sketch... on a feature) draws lines, rectangles, circles, arcs, splines and points with constraints (horizontal, vertical, parallel, perpendicular, equal, tangent, concentric, coincident, midpoint, on line, on circle, symmetric, fix) and dimensions (length, radius, diameter, distance, angle) typed as numbers or expressions of the part's dimensions, solved live (blue under-defined with the degrees of freedom left, white fully defined, red over-defined), and finishes as Extrude, Cut, Revolve or Apply sketch; from Python e.sketch_new(plane='XY', offset=0, items=[{{'rectangle': {{'w': 20, 'h': 10, 'center': [0, 0]}}}}]) and e.sketch_edit('Bracket') open the canvas in the window, and the sketch items of m.cad.sketch() (rectangle, circle, polygon, regular) build the same sketches without it; reference planes to sketch on: m.cad.plane(name='Tilted', from_plane='XY', offset=20, angle=30, about='x') (from_plane may be another reference plane), m.cad.plane(name='P', points=[(0, 0, 0), (10, 0, 0), (0, 10, 5)]) or m.cad.plane(name='Q', origin=(0, 0, 5), normal=(1, 1, 0)), m.cad.remove_plane('P'), then m.cad.sketch(plane='Tilted') or e.sketch_new(plane='Tilted'); in the window the Sketch tab has A face... (pick any flat face, also slanted ones, by clicking it), the reference planes, and Plane... (from a plane or a face, moved and turned, or through 3 points), e.sketch_on_face() and e.plane_form() open them; on the sketch canvas the Snap toggle snaps to points, centers, midpoints, quadrants, intersections, curves, guide lines through points and the grid (each switched on or off in the properties, Ctrl held places freely), a point placed on a midpoint, curve or intersection gets the matching constraint; dimensions show with arrows, a label is dragged to move it and double-clicked to type a value or an expression; dimension kinds: length, distance (plain, horizontal, vertical), distance from a point to a line or between parallel lines, radius, diameter, angle; kernels and files: the CAD Model has a Kernel parameter (Auto: the exact B-rep kernel when it builds the part, else Implicit; B-rep: exact faces and edges, refuses what it cannot build with the reason; Implicit: distance fields that build every feature), the surface has field data Kernel and KernelNotes, node.set(kernel='B-rep'); afx.load('part.step') or File > Open of .step .stp .igs .iges .3mf .sc .afxcad opens a CAD Model whose base feature is the file's solid (STL and OBJ keep their readers), afx.cad_model('part.step') does the same, m.cad.save('part.step'|'.3mf'|'.stl'|'.obj'|'.sc'|'.afxcad') writes it and returns bytes, exact and note (exact False: a faceted tessellation because only the Implicit kernel builds the part; .igs is written only for a part the exact kernel builds); edges and faces of the exact part: m.cad.edges() lists the edges (index, point, length, straight, faces on either side), m.cad.fillet_edges(edges, size=2) and chamfer_edges(edges, size=1) take indices, points or a pair of face names like ('Base top', 'Bore side'), m.cad.shell_faces(['Base top'], thickness=2) hollows the part with open faces (names or points), they need the B-rep kernel (the implicit one leaves picked edges sharp and says so in the notes); m.cad.mass(density) gives volume, area, center, bounds and mass, m.cad.measure_faces(a, b) the distance between parallel faces and the angle, m.cad.interference(other, at=(x, y, z), rotate=(rx, ry, rz)) the overlap volume of two CAD Model nodes; in the window the Modify tab's Fillet edges, Chamfer edges and Shell open faces list the edges or faces to tick (or click them in the 3D view while the list is open), e.open_picker('fillet'|'chamfer'|'shell'|'draft'|'mirror'|'pattern') and e.show_tab('Modify') open them from a script; from a CAD part to a mesh: overlay_mesh of a CAD Model gives a hexahedral mesh with one side set per CAD face (named like the faces: 'Bore side', 'Base top', 'bracket face 3' for a STEP file), extract_set(set='Bore side') shows one, geometry commands import 'part.step' (also .stp .igs .iges .3mf .sc .afxcad) with face names kept and then mesh body 1 size 0.5 scheme overlay, afx.simulation_case(mesh, 'moose', folder, fixed='bracket face 1', loaded='bracket face 2') names the sets to hold and pull; more body and face operations (exact kernel for draft, the implicit one says it leaves the faces vertical): m.cad.mirror_about('Base side 4' (a face name or a point), (1, 0, 0)) reflects the part so far about that plane and joins it, m.cad.draft(['Base side 2'], angle=5, neutral_origin=(0, 0, 0), neutral_normal=(0, 0, 1)) tapers side faces about a neutral plane, m.cad.edit('face_pattern', face='Base top', kind='hole'|'boss', layout='linear'|'grid'|'circular', count=4, spacing=12 (a grid: counts=[3, 2], spacing=[15, 20]), radius=15 (circular), diameter=6, depth=8 (a boss: height), offset=[dx, dy]) lays holes or bosses out on a flat face, following the face's own axes, m.cad.edit('combine', template='flange'|path='b.step'|document={{...}}, op='add'|'subtract'|'intersect', at=(x, y, z), rotate=(rx, ry, rz)) adds, cuts or intersects another part; assemblies (parts side by side, each keeping its own features and named faces like Cap/Base top): m.cad.component_add(name='Cap', template='flange'|file='cap.step'|document=None for a blank part, at=(x, y, z), rotate=(rx, ry, rz)), component_move('Cap', at=(x, y, z), rotate=(...), by=(dx, dy, dz)), component_op('Cap', 'add', kind='box', size=[10, 10, 5]) edits the part of a component with any operation of this API, component_visible, component_rename, component_remove, components(), component_interference() lists the pairs of parts that overlap with the overlap volume (mm3); the ribbon's Assemble tab has Insert part, Add part, Cut with part, Keep common part and Check fit, the Components section of the feature tree places, turns, hides and removes parts, and the Modify tab has Draft faces, Pattern on a face and Mirror about a face (they open a list of faces); modelling a part for the user: work out the dimensions first and name them (m.cad.parameter), build the first stage, then check it with print(m.cad.volume(), [f['name'] for f in m.cad.faces()]) against numbers worked out by hand before adding more (a cut that leaves the volume unchanged missed), put holes at the face they start from (a hole is cut downward from its position), keep feature names unique and meaningful (faces are named after them), use e = m.edit_cad() when the user wants to see and edit it in the window and e.apply() when done; keyboard in the CAD environment: Ctrl+Enter applies, Esc leaves, Delete removes the selected feature, F2 renames it, Alt+1 to Alt+6 switch the ribbon tabs; sketches with constraints from Python (no sketch JSON): s = m.cad.sketch(plane='XY'|'XZ'|'YZ'|a reference plane, offset=0) or m.cad.sketch(face='Plate z+') (on a flat face by its name: the sketch stays on that face when the part changes, a boss grows out of the part and a cut goes into it); drawing calls return handles: a = s.line((0, 0), (40, 0)), b = s.line(a.end, (40, 20)) (a.start, a.end are points; pass one to share a corner), s.lines(points, closed=True), p = s.point(x, y, fixed=False), c = s.add_circle((cx, cy), r) (c.center), s.arc(center, start, end); constraints: s.horizontal(a), s.vertical(b), s.coincident(p, q), s.parallel(a, b), s.perpendicular(a, b), s.equal(a, b), s.tangent(a, c), s.concentric(c1, c2), s.midpoint(p, a), s.point_on(p, a_or_c), s.symmetric(p, q, a), s.fix(p); dimensions (a number or an expression of the part's dimensions such as 'width / 2'): s.distance(a, 40) (a line's length), s.distance(p, q, 40), s.distance(p, a, 10) (point to line), s.dimension(...) is the same, s.length(a, v), s.hdistance(p, q, v), s.vdistance(p, q, v), s.radius(c, v), s.diameter(c, v), s.angle(a, b, degrees); s.solve() returns status ('fully defined', 'under-defined', 'over-defined', 'failed'), dof, converged, outlines, paths and moves the handles' .xy; the ready shapes rectangle, circle, polygon, regular and path(points) (an open path) still work on the same sketch; then m.cad.extrude(s, depth), revolve(s) or sketch_add(s); sweeps and lofts: m.cad.sketch_add(sketch, name='Profile') keeps a sketch with the part (sketches(), sketch_set(name, sketch), sketch_rename(name, to), sketch_remove(name)), m.cad.sweep('Profile', 'Path', twist=0, name='Handle', op='add'|'cut') sweeps the closed outline of one kept sketch along the open path of another (or pass the sketches themselves; draw the profile on a plane the path leaves at right angles, where the path starts), m.cad.loft(['Base', 'Top'], name='Vase', op='add'|'cut') lofts through closed outlines on parallel planes at different offsets; m.cad.edit('sweep_set', feature='Handle', profile='Other') and m.cad.edit('loft_set', feature='Vase', sections=[...]) choose other sketches; faces are named Handle start, Handle end, Handle side and Vase bottom, Vase top, Vase side; the exact kernel builds lofts and sweeps along a flat path without twist, others are built by the implicit kernel and noted; in the window: Sketch tab, draw, Keep as a sketch (twice), then Features tab Sweep or Loft (a button that is off says what is missing), and the feature panel chooses profile, path, sections and add or cut; sketch entities have stable numbers, so the side faces of an extrusion (Base side 3) keep their names when lines are added to or removed from the outline (new lines get new numbers); a sketch whose face is gone stops the part with a message naming the feature: m.cad.sketch_face('Boss', 'Slab z+') puts it on another face, m.cad.sketch_face('Boss', None) lets go of the face (in the window: right-click the feature, Pick another face or Detach from its face); a part from another object: c = afx.cad_from(node, name=None) makes a CAD Model whose first features are the solid that node shows (each body of a Geometry source apart, or any closed surface; up to 400000 triangles) as imported bodies that features add to and cut from, the node itself is hidden and not changed; c.edit_cad() then opens it; in the window: right-click the object in the Pipeline panel, Edit in CAD (also in the Model menu); an open surface is refused; editing a component of an assembly in place: e = m.edit_cad(); e.edit_component('Cap') makes every call of e (box, hole, extrude, features, faces, select) act on that component's own part, shown alone in its own coordinates, e.edit_component(None) goes back, e.state()['component'] says where one is, undo is the assembly's; in the window: double-click the part under Components in the Model tree (or right-click, Edit component), and the breadcrumb Assembly > Part at the top of the tree goes back; mates between the parts of an assembly (solved into the parts' places every time the assembly is built): e.mate('fixed', 'Base') holds a part where it is, e.mate('coincident', 'Base/Slab z+', 'Cap/Slab z-') joins two flat faces (flip=True: flush, looking the same way), e.mate('concentric', 'Base/Bore side', 'Pin/Rod side') puts two round faces on one axis, e.mate('distance', a, b, value=5, name='Gap') keeps two flat faces 5 mm apart, e.mate('angle', a, b, value=30) sets the angle between two faces (0 as coincident leaves them); faces are 'Component/Face' (a face of the assembly's own part is 'Face', the assembly's planes are 'XY', 'XZ', 'YZ'); the call returns mate, dof (ways left to move) and status, and refuses faces of the wrong kind or mates that contradict each other; e.mates() lists them with error and dof, e.placements() gives where the mates put each part, e.mate_set('Gap', value=12) moves the assembly (also flip=, suppressed=), e.mate_remove('Gap'); motion: give the value an expression of a dimension (e.parameter('gap', 5); e.mate('distance', a, b, value='gap')), apply, then m.key('gap', 0, 5) and m.key('gap', 1, 40) animate it (m.set(gap=20) sets it); in the window: Assemble tab, Add mate, click a face on each of two parts, choose the kind; the Mates section of the Model tree lists them with a slider on each distance and angle (dragging moves the assembly), marks the ones that cannot be met and says how many ways are left to move; right-click a part under Components to Fix it in place; when asked to fix a CAD conflict answer with a JSON array only: [{{'node': id, 'param': 'key', 'value': new value, 'reason': 'why'}}]\n\
- multi-input filters: afx.append_datasets(a, b, c) (one unstructured grid, arrays common to all inputs kept), afx.append_geometry(a, b) (surfaces into one surface), afx.append_attributes(a, b) (arrays of b copied onto a, same point/cell count), data.resample_with_dataset(mesh) or afx.resample_with_dataset(data, mesh) (sample data's arrays at mesh's points, adds vtkValidPointMask); node.inputs lists all inputs; node.set_inputs(b, c) replaces the extra inputs; node.activate() makes a node the active one (Properties panel); temporal filters (data with time steps): f.particle_tracer(seeds_node) / f.particle_path(seeds_node) / f.streak_line(seeds_node) (or seeds=N along point1..point2 without a seed node; reinject_every=N), f.temporal_interpolator(interval=0.1) adds in-between time steps, f.temporal_shift_scale(pre_shift=, scale=, post_shift=), f.temporal_array_operator(array='T' (or ['POINTS','T']), operator='-', relative=True, shift=-1) gives T_sub (change since the previous step), f.temporal_statistics() gives <name>_average/_minimum/_maximum/_stddev; Python in the pipeline: node.programmable_filter(script=\"output.point_data['x2'] = inputs[0].point_data['T'] * 2\") (inputs = list of afx.Data, output = copy of the first; arrays are NumPy; also output.points, cell_data, field_data, n_points), afx.programmable_source(script='output = afx.make_points(xyz)'), node.python_calculator(expression='np.linalg.norm(velocity, axis=1)', result='speed', attribute='Points'); afx.make_points(xyz) / make_polydata(points, polys=[[0,1,2]]) / make_image(dims, spacing, origin) / make_grid(points, cells, vtk_type) build data; plugins (reusable, in the plugin folder, afx.show_panel('plugins') opens the Plugins window): @afx.filter(category=...) def f(data, factor=2.0, mode=afx.Choice(['a','b']), array=afx.ArrayParam()): ... return data; @afx.source def g(n=10): return afx.make_points(...); @afx.reader(extensions=['ext']) def r(path): return afx.Data; afx.load_plugin(path); afx.plugins(); never call afx.load/filters/show inside a filter function\n\
- afx.submit('cmd', host=None, workdir=..., gpu='amd:0', outputs='*.e', auto_load=True) runs simulations; afx.job(id); afx.gpus(); afx.sweep('lmp -var T {{T}} -in in.lj', 'T', [300, 350], workdir=...); node.watch() live-reloads a file a running job writes\n\
- the assistant from Python: afx.ask(text, agent=False, view=False) sends a request to the assistant panel (agent=True runs the code and iterates, view=True attaches a picture of the 3D view for models that see images), afx.assistant_status() returns busy, agent, messages, last, error, history; in the window the camera checkbox next to Send attaches the view, Filters > Describe a filter... turns a sentence into filters on the selected object, and the assistant settings have Privacy: Local only (only Ollama at a local or private address, no cloud models) and Hide file paths (folders replaced by file names in what is sent)\n\
- open-source software in auraFX: Help > About auraFX > Third-party licences... lists every library auraFX is built from with its licence and the notices and licence texts they ask to be passed on (a Find box filters the lines); the same text is THIRD-PARTY-NOTICES.txt next to the program; the CAD B-rep kernel and CAD file formats come from the open-source SolidCraft and solidcore projects, carried in the repository under vendor/ so the CAD environment works without fetching anything\n\
- numpy is available as np; standard scientific Python can be used on arrays.\n\n\
Available filters/sources (id: description):\n{filter_catalog}\n\n\
Current pipeline state:\n{pipeline_description}"
    )
}

#[cfg(test)]
mod block_tests {
    use super::python_blocks;

    #[test]
    fn a_block_of_only_comments_is_not_code_to_run() {
        let reply = "Done.\n\n```python\n# Task completed\n```\n";
        assert!(python_blocks(reply).is_empty());
        let reply = "Run this:\n```python\n# set it up\nm = afx.cad_model()\n```\nand then\n```python\n# nothing\n\n```";
        assert_eq!(python_blocks(reply), vec!["# set it up\nm = afx.cad_model()".to_string()]);
        assert_eq!(python_blocks("```\nx = 1\n```"), vec!["x = 1".to_string()]);
        assert!(python_blocks("```text\nx = 1\n```").is_empty());
    }
}

#[cfg(test)]
mod privacy_tests {
    use super::*;

    #[test]
    fn local_only_refuses_hosted_services_cloud_models_and_public_servers() {
        let mut c = AiConfig { local_only: true, ..Default::default() };
        c.provider = ProviderKind::Ollama;
        c.ollama_url = "http://10.0.0.45:11434".into();
        c.model = "qwen3:27b".into();
        assert!(local_only_refusal(&c).is_none());
        c.model = "kimi-k2.7-code:cloud".into();
        assert!(local_only_refusal(&c).unwrap().contains("cloud"));
        c.model = "qwen3:27b".into();
        c.ollama_url = "https://ollama.example.com".into();
        assert!(local_only_refusal(&c).unwrap().contains("not a local"));
        c.ollama_url = "http://localhost:11434".into();
        assert!(local_only_refusal(&c).is_none());
        c.provider = ProviderKind::Anthropic;
        assert!(local_only_refusal(&c).unwrap().contains("hosted"));
        c.local_only = false;
        assert!(local_only_refusal(&c).is_none());
    }

    #[test]
    fn redaction_keeps_file_names_and_hides_folders() {
        let t = redact_paths("- [1] \"out.e\" params: path=/scratch/ann/run42/out.e, mesh=C:\\Users\\ann\\m.vtu; ratio 1/2");
        assert!(t.contains("path=out.e") && t.contains("mesh=m.vtu"), "{t}");
        assert!(!t.contains("/scratch") && !t.contains("Users"), "{t}");
        assert!(t.contains("ratio 1/2"), "a fraction is not a path: {t}");
        if let Ok(h) = std::env::var("HOME") {
            let t = redact_paths(&format!("{h}/data/a.vtk"));
            assert_eq!(t, "a.vtk");
        }
    }

    #[test]
    fn pictures_go_in_each_providers_format() {
        let mut m = ChatMessage::new(Role::User, "what is this?");
        m.images.push("QUJD".into());
        assert_eq!(ollama_msg(&m)["images"][0], "QUJD");
        assert_eq!(anthropic_msg(&m)["content"][0]["source"]["data"], "QUJD");
        assert_eq!(anthropic_msg(&m)["content"][1]["text"], "what is this?");
        assert_eq!(openai_msg(&m)["content"][1]["image_url"]["url"], "data:image/png;base64,QUJD");
        let plain = ChatMessage::new(Role::User, "hi");
        assert_eq!(openai_msg(&plain)["content"], "hi");
    }
}

#[cfg(test)]
mod skill_tests {
    use super::*;

    #[test]
    fn skills_load_switch_off_and_prompts_fill_in_the_object() {
        let dir = std::env::temp_dir().join(format!("aurafx_skills_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("a-units.md"), "Lengths are in micrometres.").unwrap();
        std::fs::write(dir.join("b-colors.md"), "Use the Inferno color map for temperature.").unwrap();
        std::fs::write(dir.join("notes.bin"), "ignored").unwrap();
        let all = load_skills(&dir, &[]);
        assert_eq!(all.iter().map(|s| s.0.as_str()).collect::<Vec<_>>(), vec!["a-units.md", "b-colors.md"]);
        let some = load_skills(&dir, &["a-units.md".to_string()]);
        assert_eq!(some.len(), 1);
        let p = skills_prompt(&some);
        assert!(p.contains("## b-colors.md") && p.contains("Inferno") && !p.contains("micrometres"));
        assert!(skills_prompt(&[]).is_empty());
        let t = PromptTemplate { name: "D".into(), text: "Describe {object}.".into() };
        assert_eq!(t.fill("can.ex2"), "Describe can.ex2.");
        assert!(AiConfig::default().prompts.len() >= 4);
        let _ = std::fs::remove_dir_all(&dir);
    }
}

/// Download `url` to `dest` (through a `.part` file beside it), unpacking gzip when the address
/// ends in .gz; `progress` gets (bytes so far, total when the server says).
pub fn fetch_to_file(url: &str, dest: &std::path::Path, progress: &dyn Fn(u64, Option<u64>)) -> Result<()> {
    use std::io::{Read, Write};
    let client = reqwest::blocking::Client::builder().connect_timeout(Duration::from_secs(15)).timeout(Duration::from_secs(1800)).build()?;
    let resp = client.get(url).send()?.error_for_status()?;
    let total = resp.content_length();
    if let Some(d) = dest.parent() {
        std::fs::create_dir_all(d)?;
    }
    let part = dest.with_extension("part");
    struct Counted<'a, R> {
        inner: R,
        done: u64,
        total: Option<u64>,
        progress: &'a dyn Fn(u64, Option<u64>),
    }
    impl<R: Read> Read for Counted<'_, R> {
        fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
            let n = self.inner.read(buf)?;
            self.done += n as u64;
            (self.progress)(self.done, self.total);
            Ok(n)
        }
    }
    let counted = Counted { inner: resp, done: 0, total, progress };
    let mut reader: Box<dyn Read + '_> = if url.ends_with(".gz") { Box::new(flate2::read::GzDecoder::new(counted)) } else { Box::new(counted) };
    let mut file = std::fs::File::create(&part)?;
    std::io::copy(&mut reader, &mut file)?;
    file.flush()?;
    drop(file);
    std::fs::rename(&part, dest)?;
    Ok(())
}
