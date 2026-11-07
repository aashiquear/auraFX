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
        "You are the built-in assistant of auraFX, a scientific visualization, analysis and simulation application written in Rust.\n\
You help the user visualize and analyze scientific data (meshes, particles, fields, time series).\n\
When an action is needed, answer with a short explanation and ONE ```python code block that uses the embedded `afx` module; \
the user can run it with one click. Keep code minimal. Prefer method chaining on nodes.\n\n\
API cheat sheet:\n\
- afx.load(path) -> Node (Exodus, VTK legacy/XML/.vtkhdf, EnSight .case, CGNS .cgns, Xdmf .xmf/.xdmf, NetCDF CF .nc (2-D lon/lat like POP's TLONG/TLAT too; reader option variables='(nlat, nlon)' picks the grid), CAM-SE .nc with its connectivity file found beside it (reader options connectivity='file.nc', variables='Surface [time, ncol]'|'Midpoint layers [time, lev, ncol]'|'Interface layers [time, ilev, ncol]', all_levels=True, level=0, layout='Lat/Lon map'|'Sphere') and MPAS .nc (MPAS reader options: node.set(vertical_level=2, multilayer=True, layer_thickness=0, atmosphere=False, layout='Lat/Lon map', center_lon=180)), OpenFOAM, Tecplot .dat/.plt, PLOT3D .xyz/.q, LAMMPS, VASP POSCAR/CHGCAR, XYZ, PDB, Gaussian .cube, STL/PLY/OBJ, CSV, PNG/JPEG/TIFF images, molecules (PDB, XYZ, VASP, .cml: atoms with atomic_number and bonds as lines; they open with representation='Molecule' colored by element with cmap='Elements'; node.set(molecule_style='Ball and Stick'|'Van der Waals'|'Liquorice', atom_scale=0.3, bond_radius=0.075, bond_orders=True); node.compute_molecule_bonds(tolerance=0.45, absolute=True)), hyper tree grids (.htg; afx.hyper_tree_grid(dimensions=(3, 3, 1), max_depth=3, descriptor='R... | .R.. | ....'), afx.random_hyper_tree_grid(seed=3, masked_fraction=0.25); node.save('tree.htg') keeps the grid; node.hyper_tree_grid_to_unstructured_grid(), node.hyper_tree_grid_to_dual_grid() leaf centers joined into cells with point data, node.hyper_tree_grid_geometry() outer surface and walls next to masked cells (grids with material interfaces are drawn cut to their material; fill_material=False gives the 2-D interface lines), node.hyper_tree_grid_feature_edges() outline/corner edges, node.hyper_tree_grid_depth_limiter(depth=2), node.hyper_tree_grid_generate_fields(), node.hyper_tree_grid_evaluate_coarse(operator='Average'); other filters work on the visible leaves), cell grids (.dg discontinuous Galerkin fields: HGRAD/HCURL/HDIV/constant on DG cells; drawn and filtered as linear cells with the fields at the corners; node.cell_grid_centers() exact values at cell centers, node.cell_grid_elevation(axis=(0, 0, 1)), node.cell_grid_to_unstructured_grid(); node.cell_grid_transform(translate=, rotate=, scale=), node.cell_grid_warp(attribute='hgrad', scale=0.5), mesh.convert_to_cell_grid(), node.cell_grid_probe(points_node, attribute='curl1') cells, parametric coordinates and values at another dataset's points, node.cell_grid_compute_sides() outer faces of 3-D cells drawn instead of the cells (include='Edges of inputs'|'Vertices of inputs'|'Surfaces of inputs'|'All sides', omit_renderable=False to also get sides of 2-D cells, preserve_renderable=False, strategy='Boundary'|'Odd count'|'Every side'), node.save('x.dg'); afx.cell_grid(cell_type='Hexahedron') test source), AMR data (AMReX/BoxLib plotfile folders or their Header, VTK .vthb, FLASH HDF5 plot/checkpoint files (no extension needed), Chombo .hdf5/.h5 (cell or node centered; ghost cells dropped; data='Particles' for its particles; 4-D files give a 2-D grid with per-cell velocity-space arrays, _vpar, _mu and <component>_sum), Enzo dumps via their .hierarchy file (reader option cgs=True converts to CGS units; node.set(data='Particles') reads FLASH/Enzo particles as points instead of the grid): levels of image boxes, covered coarse cells hidden via vtkGhostType; representation='Outline' shows the boxes; afx.amr_gaussian_pulse() test source; node.overlapping_amr_level_ids(); node.amr_contour(array='density', values=[0.3]) crack-free isosurface of a cell array across levels (node.contour on AMR cell data does the same); node.amr_dual_grid() stitched mesh of cell centers for crack-free clip/slice; node.resample_amr(samples=(64, 64, 64)) one uniform grid from the finest cells, whole_domain=False with min=/max= for a box, to_nodes=False for cell data), GIS vectors (.shp shapefiles, .geojson, .kml, .kmz, .gpkg GeoPackage, .fgb FlatGeobuf, .gml GML, MapInfo .tab and .mif, Esri file geodatabase .gdb folders: points/lines/polygon outlines with attributes as cell data, a block per layer; reader options polygons='Filled' fills areas to color by an attribute, layer='name', blocks='One per feature'), GeoTIFF, Esri ASCII grid (.asc) and ERDAS Imagine (.img) rasters at map coordinates (bands as cell data; for terrain: .cell_data_to_point_data().warp_by_scalar(array='Elevation')), EBSD maps (.ang TSL/EDAX square grids, .ctf Oxford): image data with one cell per point, cell arrays EulerAngles (Bunge, radians), Phases (0 = not indexed) and the quality columns (.ang: Image Quality, Confidence Index, SEM Signal, Fit; .ctf: Bands, Error, MAD, BC, BS), field arrays CrystalStructures (crystal class per phase, index 0 = not indexed, 999 unknown), MaterialName and LatticeConstants; .ctf reader option hexagonal_axes='a1 along x (+30 degrees on phi2)' (default) or 'As stored'; then map.orientation_colors() gives IPFColor; .h5ebsd stacks of TSL or Oxford slices (one volume, slices Z Resolution apart; the sample and Euler transformations are kept as field arrays, not applied), EDAX OIM .h5 and Bruker Esprit .h5 scans (reader option scan='Scan 1'), .dream3d microstructures (image, rectilinear, vertex, edge, triangle, quad, tet and hex geometries; cell arrays as cell data, vertex arrays as point data, feature and phase arrays as field data; reader options data_container='name' and arrays='EulerAngles, Phases' to read only some cell arrays of big volumes); afx.load('slices/img_*.png') stacks images into a volume); afx.active() -> Node or None; afx.nodes(); afx.get(name)\n\
- node.<filter>(**params) -> Node for any filter id below (e.g. node.slice(normal=[0,0,1]), node.contour(values=[0.5]), node.threshold(lower=1, upper=2), node.clip(type='Sphere', radius=2), node.calculator(expression='mag(velocity)', result='speed'), node.glyph(orient='velocity'))\n\
- node.set(**params_or_display) ; display keys: visible, opacity, colormap, representation ('Surface','Wireframe','Points','Surface With Edges','Outline','Volume','Surface LIC'), volume_opacity, volume_threshold, lic_vectors (array name; Surface LIC shows a vector field as streaks on surfaces, e.g. a slice of a velocity field), lic_strength, lic_length, texture='/path/image.png' (shows an image on a surface with texture coordinates: first node.texture_map_to_sphere() / texture_map_to_plane(automatic=True) / texture_map_to_cylinder(prevent_seam=False); texture=None removes it), point_size, line_width (pixels, for lines, wireframes and edges), name\n\
- node.color_by('array', assoc='points'|'cells', cmap='Viridis', range=(lo, hi)); node.show(); node.hide(); node.delete()\n\
- node.info() -> dict; node.arrays(); node.array('name') -> numpy; node.points -> numpy (N,3) (multi-block data is merged automatically); node.add_array('name', numpy_array); node.save('out.vtu') (also .vtk .vtkhdf .exo .case .xmf .cgns .csv .stl .ply); afx.probe(node, [x,y,z]) -> dict\n\
- afx.<source>(**params) for sources (sphere, cone, wavelet, plane, line, box, cylinder, point_source)\n\
- afx.time(index); afx.times(); afx.view('+x'|'-y'|'iso'|[x,y,z]); afx.reset_camera(); afx.screenshot('file.png', width=1920, transparent=False) (waits until saved); afx.export_scene('scene.glb' or '.obj', '.x3d', '.wrl'); afx.screenshot('figure.svg') writes a vector picture; afx.describe(); afx.run()\n\
- afx.plot(x, y, label=...); afx.over_time(node, 'array', stat='max'); afx.show_panel('Charts'); afx.hide_panel('Log'); afx.layout('analyze'); afx.set_theme('light'); afx.split_view('right'|'down') adds a 3D view (it becomes active; node.show()/hide() apply to the active view); afx.active_view(i); node.send_to_view(i | 'right' | 'down', keep=False) moves an object to view i or into a new view of its own; node.set(axes_grid=True) draws a labelled axes grid around an object; keyframe animation: afx.key_camera(t) keys the current view at t in [0,1], node.key('param', t, value, interp='ease in-out') keys a filter value or a display property ('opacity', 'solid_color', 'point_size', ...; interp: linear, step, ease in, ease out, ease in-out, exponential), afx.key_camera(t, interp=...), afx.animation(frames=120, time=True), afx.animation_time(t) previews, afx.save_animation('out.mp4', fps) records keyframes when any exist, afx.save_animation('out.gif', cue=lambda t: ..., frames=60) calls a Python function before each frame and returns when the file is written; OpenFOAM boundary patches: case.openfoam_patches(patches='inlet,walls'); node.save('out.exo', all_times=True) writes Exodus with every time step (also .vtkhdf, EnSight .case, Xdmf .xmf, .cgns); color maps: node.set(cmap='Cool to Warm', cmap_reverse=True, cmap_bands=8, cmap_points=[[0, r,g,b], ..., [1, r,g,b]]), afx.define_colormap(name, points), afx.import_colormaps('paraview.json'), afx.show_panel('colormap') opens the editor; afx.link_views(True); afx.match_views(); curved (higher-order Lagrange/Bezier/quadratic) cells: node.set(nonlinear_subdivision=2) draws them smoother, node.tessellate(level=2) turns them into linear cells, afx.unstructured_cell_types(cell_type='Lagrange Hexahedron', blocks=(2, 2, 2), order=3) makes test cells, info()['cell_types'] lists cell types; data on another machine: afx.load('/path/on/host.e', host='cluster') (or 'ssh://cluster/path') reads and filters it there with aurafx-server and brings back only what is shown, node.set(all_data=True) brings everything, afx.servers() lists connections, node.set(draw_on='server') has the server's GPU draw it (only pictures come over), afx.load(path, host='cluster', processes=8) spreads the data over 8 server processes; afx.lighting(edl=True) turns on Eye-Dome Lighting (good for point clouds/particles), afx.lighting(shadows=True, ssao=True) adds shadows and ambient occlusion, afx.lighting(lights=[{{'direction': (0, 0, 1), 'color': (1, 0.9, 0.8), 'intensity': 0.8, 'camera_relative': False}}]) sets custom lights, afx.lighting(fxaa=True, tone_mapping='aces', exposure=1.2) for the final picture, afx.lighting(ray_tracing=True, ray_samples=256, ray_device='auto'|'gpu'|'cpu') path-traces opaque surfaces (screenshots wait for it), afx.lighting(environment='sky'|'studio'|'white'|'/path/map.hdr', environment_intensity=1, show_environment=True) lights with an environment map, node.set(metallic=1, roughness=0.2, specular=0.5) sets the material; arrays of 8-bit values with 3 or 4 components (like IPFColor) are drawn as their own colors when colored by: node.set(color_by='IPFColor'); node.set(direct_colors=False) maps them through the color map instead; geometry: g = afx.mesh.geometry(); g.cmd('brick x 4 y 2 z 1'); g.cmd('cylinder radius 0.5 height 2; subtract body 2 from body 1'); g.bodies() lists bodies with history and faces; g.journal(); part = g.result(resolution=96) adds the Geometry source (afx.geometry(commands=...)), whose surface has cell arrays BodyId and FaceId and field arrays FaceNames, Volume, SurfaceArea (output='Distance field' gives the signed distance grid); geometry commands, one per line or separated by ; (sizes in model units, angles in degrees, primitives centred on the origin, round ones along z): brick x 4 y 2 z 1 | brick 2, sphere radius 1, cylinder radius 0.5 height 2, cone radius 1 top 0.4 height 2, torus major 2 minor 0.5, prism sides 6 radius 1 height 2, superquadric x 1 y 0.8 z 0.6 e1 0.5 e2 0.7, harmonic radius 1 coefficient l m c ..., import 'part.stl' scale 0.001; outline 1 polyline x y ... | rectangle x 2 y 1 center 0 0 | circle radius 1 | ellipse x 2 y 1 | regular sides 6 radius 1 | spline x y ...; outline 1 add line x y ... | add arc center 0 0 radius 1 start 90 end -90; outline 1 fillet radius 0.2; extrude outline 1 height 2; revolve outline 1 angle 360 (x = distance from the z axis); sweep outline 1 path x y z x y z ...; move body 2 x 1 y 0 z 0.5; rotate body 2 angle 30 about z|x|y|ax ay az center x y z; scale body 2 1.5 | scale body 2 x 2 y 1 z 1; unite body 1 2; subtract body 2 from body 1 [keep]; intersect body 1 2; copy body 1; delete body 2; body 1 name 'bracket'; face 6 name 'inlet' (faces are numbered; names like brick 1 z+, cylinder 2 side, extrusion 3 edge 1 keep through booleans); node.signed_distance(resolution=128, margin=0.1) turns a closed surface into a Distance grid; hexahedral meshes: afx.box_mesh(size=(2, 1, 1), cells=(40, 20, 20), grading=(3, 1, 1), cluster='Start', element='HEX8'|'HEX20'|'HEX27'), afx.inclusion_mesh(size=(1, 1, 1), cells=24, count=8, radius=0.075, radius_std=0, shape='Sphere'|'Ellipsoid'|'Superquadric'|'Harmonic grain', aspect=(1, 0.7, 0.5), exponents=[4, 4], roughness=0.12, seed=7, periodic=False, element='HEX8', separate_blocks=False) packs inclusions in a box and meshes them with conforming hexahedra (blocks 1 matrix and 100 inclusions, side sets x- x+ y- y+ z- z+ and interface, node sets x- ... z+, element variable particle_id; periodic adds node sets x-_node_set/x+_node_set ... and PeriodicPairs); generated meshes keep the cell array ElementBlock and field arrays ElementBlockIds, ElementBlockNames, SideSet <name> (element, Exodus side), NodeSet <name>; mesh.extract_set(set='interface') shows a side set or node set; mesh.elevate(element='HEX27'); mesh.mesh_quality() adds the hexahedron metrics (ScaledJacobian, Skew, Taper, MaxAspectFrobenius, Oddy, Shear, Shape, Distortion, ...) and mesh.mesh_quality(output='Summary table') gives min/max/mean and acceptable ranges per metric; afx.plate_mesh(size=(100, 50, 1), cell=2.5, hole='None'|'Circle'|'Ellipse', a=2.5, b=2.5, angle=0, centre=[50, 25]) (blocks plate and ring, sets x- ... z+, outer, grips, hole); afx.dogbone_mesh(length=25.4, gage_length=8, gage_width=1.85, grip_width=4.8, fillet_radius=3.175, thickness=0.35, stations=[0, 0.37, 4, 0.35], cell=0.2, grip_cell=0.4, layers=1) (blocks gage, shoulder, grip; node sets grips, gage-, gage+, station_k ...); surface.sweep_quads(direction=(0, 0, 1), distance=1, layers=4, grading=1) sweeps quadrilaterals into hexahedra (side sets bottom, top, side); afx.inclusion_mesh(packing='Collective') packs denser; afx.geometry(commands=..., output='Tet mesh') fills the bodies of mesh body commands with tetrahedra (side set per face); surface.tet_mesh(cell=0) fills a closed surface with tetrahedra; tets.refine_region(box_min=(...), box_max=(...)) or (array='name', value=1) refines part of a tetrahedral mesh conformingly; mesh.refine_mesh(levels=1) splits hexahedra or tetrahedra into 8 each (sets kept); mesh.convert_elements(to='Hexahedra to tetrahedra'|'Tetrahedra to hexahedra') (12 tets per hex, 4 hexes per tet); mesh.smooth_mesh(scheme='Smart Laplacian'|'Laplacian'|'Condition number', iterations=10, fixed='nodeset') holds the boundary; mesh.boundary_layers(side_set='z+', layers=3, first=0.01, growth=1.3) inserts wall layers; mesh_quality also gives the tetrahedron metrics (MinimumDihedralAngle, AspectRatio, RadiusRatio, Condition ...); image_or_surface.overlay_mesh(array='FeatureIds', cell=0, mode='Smooth'|'Stair-step', void=False, periodic=False, min_cells=8, sharp=True, element='HEX8') meshes a labelled image (one block per label, 0 empty) or a closed surface on an overlay grid with pillowed smooth interfaces (field data MinScaledJacobian, InvertedElements; side sets x- ... z+, surface <block>, interface <a> - <b>); geometry command mesh body 3 size 0.1 scheme overlay|stairstep|structured meshes any body (non-brick, non-cylinder bodies use overlay by default, sharing one grid); geometry commands mesh body 1 2 size 0.1 element HEX20, block 7 add body 2 name 'rod', sideset 1 add face 6 name 'top', nodeset 2 add face 1 name 'left' with afx.geometry(commands=..., output='Hex mesh') mesh bricks and cylinders (each face also a side set named after it); ebsd.pole_figure(families='100, 110, 111', projection='Stereographic'|'Equal area', sigma=7.5, phase=1) gives disks colored by MRD (multiples of random); ebsd.inverse_pole_figure(directions='X, Y and Z'|'X'|'Y'|'Z', sigma=5) the density on the standard triangle; ebsd.odf_sections(sections='0, 45, 65', step=5, sigma=7.5) ODF sections (phi1 across, Phi down) with field data TextureIndex and MaxODF; ebsd.misorientation_distribution(min_angle=5, bin=1) gives a table (bin_center, Neighbours, Random pairs, Uniform as densities per degree) for the Charts panel; ebsd.texture_components(families='100, 110, 111', axes='X, Y, Z', cone=15) a table of fractions; ebsd.chord_lengths(threshold=10, spacing=0, output='Chords'|'Histogram', bin=0) grain-boundary chords along x, y, z with field data MeanChordX, ChordsX ...; volume.quick_surface_mesh() gives boundary triangles (cell FaceLabels a b, normal from a into b, -1 outside; point NodeType 2/3/4, +10 on the outer faces; field AvgQuats, FeaturePhases); surface.smooth_boundaries(iterations=20, lambda=0.5, mu=-0.53 (0: Laplacian), triple_lambda=0.25, quad_lambda=0); surface.triangle_geometry() (FaceAreas, FaceNormals, FaceCentroids, FaceMinimumAngles; field SurfaceVolumes, SurfaceAreas per feature); surface.feature_surfaces(features=[]) one closed surface per feature (save as .stl writes one file per feature); surface.face_orientation_colors() (FaceIPFColors, FaceMisorientations, FaceMisorientationColors); surface.gbcd_section(angle=60, axis=(1, 1, 1), tolerance=5, sigma=5) the GBCD for one misorientation in multiples of random (field MisorientationMRD, MaxMRD); afx.voronoi_grains(cells=[64, 64, 64], spacing=1, grains=100, size_sigma=0 (Voronoi) or above (Laguerre), periodic=False, orientations='Random'|'<100> along Z'|'<111> along Z', spread=10, structure='Cubic m-3m', seed=1) and afx.columnar_grains(size=(300, 300, 450), voxel=5, width=50, height=150, height_sigma=0.5, tilt=6, orientations='<100> along Z') make synthetic microstructures (FeatureIds, Phases, EulerAngles); grains.orientation_noise(sigma=2, per_feature=False), grains.jumble_orientations(), grains.ensemble_statistics(include_surface=False) (table: Features, ESD mu, ESD sigma, b/a alpha, b/a beta, c/a alpha, c/a beta, Neighbors mu ...); afx.pack_grains(cells=[64, 64, 64], spacing=1, esd_mu=2.3, esd_sigma=0.2, ba=[15, 2], ca=[12, 4], shape='Ellipsoid'|'Superellipsoid'|'Cube-octahedron'|'Cylinder', aligned=False, fraction=1 (below 1: matrix phase 2), periodic=False, calibrate=True) packs grains from goal statistics; measured.pack_like(cells=[100, 100, 100]) packs a statistical copy; grains.insert_precipitates(fraction=0.05, esd_mu=1, esd_sigma=0.2, at_boundaries=False) adds a particle phase; synthetic.match_crystallography(reference) assigns orientations from the reference and matches its boundary misorientation distribution (field data MDFErrorBefore/After); texture filters take phase (0: all), mask and max_orientations, and save to CSV with a Block column; mesh.save('part.e'|'part.inp'|'part.msh') writes Exodus (blocks, side sets, node sets, HEX20/27), Abaqus input or Gmsh 4.1; grains: node.segment_features(tolerance=2, mask='SampleVoxels') writes the cell array FeatureIds, then .fill_bad_data(min_defect=7).minimum_size(min_cells=7) clean up, and feature_sizes, feature_centroids, feature_shapes, average_orientations, reference_misorientations, kernel_average_misorientation(kernel=(1, 1, 1)), feature_neighbors, distance_to_boundaries, merge_twins add per-feature results as field arrays (one row per feature id, row 0 for id 0: NumElements, Volumes, EquivalentDiameters, Centroids, AxisLengths, AspectRatios, Omega3s, AvgQuats, FeatureAvgMisorientations, NumNeighbors, SurfaceFeatures, ParentIds) or cell arrays (KernelAverageMisorientations, FeatureReferenceMisorientations, BoundaryCells, GBEuclideanDistances); node.feature_table() gives them as a table (Spreadsheet, CSV); node.feature_colors() gives FeatureColors (color_by='FeatureColors'); node.feature_boundaries() gives the faces between features with Misorientation and Twin\n\
- multi-input filters: afx.append_datasets(a, b, c) (one unstructured grid, arrays common to all inputs kept), afx.append_geometry(a, b) (surfaces into one surface), afx.append_attributes(a, b) (arrays of b copied onto a, same point/cell count), data.resample_with_dataset(mesh) or afx.resample_with_dataset(data, mesh) (sample data's arrays at mesh's points, adds vtkValidPointMask); node.inputs lists all inputs; node.set_inputs(b, c) replaces the extra inputs; node.activate() makes a node the active one (Properties panel); temporal filters (data with time steps): f.particle_tracer(seeds_node) / f.particle_path(seeds_node) / f.streak_line(seeds_node) (or seeds=N along point1..point2 without a seed node; reinject_every=N), f.temporal_interpolator(interval=0.1) adds in-between time steps, f.temporal_shift_scale(pre_shift=, scale=, post_shift=), f.temporal_array_operator(array='T' (or ['POINTS','T']), operator='-', relative=True, shift=-1) gives T_sub (change since the previous step), f.temporal_statistics() gives <name>_average/_minimum/_maximum/_stddev; Python in the pipeline: node.programmable_filter(script=\"output.point_data['x2'] = inputs[0].point_data['T'] * 2\") (inputs = list of afx.Data, output = copy of the first; arrays are NumPy; also output.points, cell_data, field_data, n_points), afx.programmable_source(script='output = afx.make_points(xyz)'), node.python_calculator(expression='np.linalg.norm(velocity, axis=1)', result='speed', attribute='Points'); afx.make_points(xyz) / make_polydata(points, polys=[[0,1,2]]) / make_image(dims, spacing, origin) / make_grid(points, cells, vtk_type) build data; plugins (reusable, in the plugin folder, afx.show_panel('plugins') opens the Plugins window): @afx.filter(category=...) def f(data, factor=2.0, mode=afx.Choice(['a','b']), array=afx.ArrayParam()): ... return data; @afx.source def g(n=10): return afx.make_points(...); @afx.reader(extensions=['ext']) def r(path): return afx.Data; afx.load_plugin(path); afx.plugins(); never call afx.load/filters/show inside a filter function\n\
- afx.submit('cmd', host=None, workdir=..., gpu='amd:0', outputs='*.e', auto_load=True) runs simulations; afx.job(id); afx.gpus(); afx.sweep('lmp -var T {{T}} -in in.lj', 'T', [300, 350], workdir=...); node.watch() live-reloads a file a running job writes\n\
- numpy is available as np; standard scientific Python can be used on arrays.\n\n\
Available filters/sources (id: description):\n{filter_catalog}\n\n\
Current pipeline state:\n{pipeline_description}"
    )
}
