//! AI assistant panel: chat with pipeline context, runnable code blocks.

use crate::app::State;
use crate::settings::Settings;
use crate::touch::FitScreen;
use crate::worker::PythonResult;
use aurafx_ai::{AiConfig, ChatMessage, ProviderKind, Role, StreamEvent};
use egui::RichText;
use egui_commonmark::{CommonMarkCache, CommonMarkViewer};
use std::sync::mpsc::Receiver;

/// The model list's entry when no model is chosen (the shipped default).
pub const NO_MODEL: &str = "<no model>";

/// The folder of skills: Markdown files beside the settings (`skills/`), made on first use.
pub fn skills_dir() -> Option<std::path::PathBuf> {
    let d = directories::ProjectDirs::from("org", "aurafx", "auraFX")?.config_dir().join("skills");
    let _ = std::fs::create_dir_all(&d);
    Some(d)
}

pub struct Assistant {
    pub messages: Vec<ChatMessage>,
    pub input: String,
    stream: Option<Receiver<StreamEvent>>,
    partial: String,
    pub models: Vec<String>,
    models_rx: Option<Receiver<anyhow::Result<Vec<String>>>>,
    cache: CommonMarkCache,
    pub focus_input: bool,
    pub error: Option<String>,
    /// Output of code blocks run from the chat, keyed by message index.
    code_results: Vec<(usize, String, bool)>,
    cfg: AiConfig,
    auto_ran: usize,
    // Agent mode: run code, feed results back, repeat.
    agent_steps: usize,
    agent_pending: usize,
    agent_results: Vec<String>,
    agent_continue: bool,
    pub agent_active: bool,
    /// Send a picture of the 3D view with the next message.
    pub include_view: bool,
    /// A message waiting for its picture of the view: the screenshot's id and file, the text and the
    /// pipeline description.
    pending_view: Option<(u64, std::path::PathBuf, String, String)>,
    /// The "Describe a filter" window: open, and its text.
    pub describe_open: bool,
    pub describe_text: String,
}

impl Assistant {
    /// Whether an answer is still arriving.
    pub fn busy(&self) -> bool {
        self.stream.is_some()
    }
    pub fn reset_agent_steps(&mut self) {
        self.agent_steps = 0;
    }
}

pub const TOOL_PREFIX: &str = "[tool result]";
const AGENT_MAX_STEPS: usize = 8;

impl Assistant {
    pub fn new(settings: &Settings) -> Self {
        let mut a = Self {
            messages: Vec::new(),
            input: String::new(),
            stream: None,
            partial: String::new(),
            models: Vec::new(),
            models_rx: None,
            cache: CommonMarkCache::default(),
            focus_input: false,
            error: None,
            code_results: Vec::new(),
            cfg: settings.ai.clone(),
            auto_ran: 0,
            agent_steps: 0,
            agent_pending: 0,
            agent_results: Vec::new(),
            agent_continue: false,
            agent_active: false,
            include_view: false,
            pending_view: None,
            describe_open: false,
            describe_text: String::new(),
        };
        a.refresh_models();
        a
    }

    pub fn refresh_models(&mut self) {
        let cfg = self.cfg.clone();
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let _ = tx.send(aurafx_ai::list_models(&cfg));
        });
        self.models_rx = Some(rx);
    }

    pub fn is_streaming(&self) -> bool {
        self.stream.is_some()
    }

    pub fn poll(&mut self, ctx: &egui::Context) {
        if self.models_rx.is_some() {
            ctx.request_repaint_after(std::time::Duration::from_millis(100));
        }
        if let Some(rx) = &self.models_rx {
            if let Ok(r) = rx.try_recv() {
                match r {
                    Ok(m) => {
                        tracing::info!("AI: {} models available from {:?}", m.len(), self.cfg.provider);
                        self.models = m;
                        // No model is chosen for the user: the list stays at <no model> until they pick one.
                    }
                    Err(e) => {
                        tracing::warn!("AI: listing models failed: {e:#}");
                        self.error = Some(format!("{e:#}"));
                    }
                }
                self.models_rx = None;
            }
        }
        let mut done = false;
        if let Some(rx) = &self.stream {
            for ev in rx.try_iter() {
                match ev {
                    StreamEvent::Delta(d) => self.partial.push_str(&d),
                    StreamEvent::Done => done = true,
                    StreamEvent::Error(e) => {
                        self.error = Some(e);
                        done = true;
                    }
                }
            }
            ctx.request_repaint_after(std::time::Duration::from_millis(50));
        }
        if done {
            if !self.partial.is_empty() {
                self.messages.push(ChatMessage::new(Role::Assistant, std::mem::take(&mut self.partial)));
            }
            self.stream = None;
        }
    }

    pub fn send(&mut self, text: String, pipeline: String) {
        self.send_with_images(text, pipeline, Vec::new());
    }

    /// Ask for a picture of the active 3D view and send the message with it once it is taken.
    pub fn send_with_view(&mut self, text: String, pipeline: String, session: &aurafx_pipeline::SharedSession) {
        if self.stream.is_some() || self.pending_view.is_some() {
            tracing::warn!("AI: request dropped (an answer is still arriving)");
            return;
        }
        let id =
            0x5649_0000_0000 + (std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(1) & 0xffff_ffff);
        let path = std::env::temp_dir().join(format!("aurafx_assistant_view_{}_{id}.png", std::process::id()));
        session.lock().screenshot_request =
            Some(aurafx_pipeline::session::ScreenshotRequest { size: Some((960, 0)), id, ..aurafx_pipeline::session::ScreenshotRequest::new(path.clone()) });
        self.pending_view = Some((id, path, text, pipeline));
    }

    /// Every frame: when the picture of the view is ready, send the message waiting for it.
    pub fn tick_view(&mut self, session: &aurafx_pipeline::SharedSession) {
        let Some((id, _, _, _)) = &self.pending_view else { return };
        let id = *id;
        let result = {
            let mut s = session.lock();
            s.screenshot_results.iter().position(|r| r.0 == id).map(|k| s.screenshot_results.remove(k).1)
        };
        let Some(result) = result else { return };
        let (_, path, text, pipeline) = self.pending_view.take().unwrap();
        let images = match result.and_then(|_| std::fs::read(&path).map_err(|e| e.to_string())) {
            Ok(bytes) => {
                use base64::Engine;
                vec![base64::engine::general_purpose::STANDARD.encode(bytes)]
            }
            Err(e) => {
                self.error = Some(format!("the picture of the view could not be taken ({e}); sent without it"));
                Vec::new()
            }
        };
        let _ = std::fs::remove_file(&path);
        self.send_with_images(text, pipeline, images);
    }

    pub fn send_with_images(&mut self, text: String, pipeline: String, images: Vec<String>) {
        if text.trim().is_empty() || self.stream.is_some() {
            tracing::warn!("AI: request dropped ({})", if text.trim().is_empty() { "empty" } else { "an answer is still arriving" });
            return;
        }
        self.error = None;
        if self.cfg.model.is_empty() {
            self.error = Some(if self.models.is_empty() && self.cfg.provider == ProviderKind::Ollama {
                "No model selected, and none is listed: check the Ollama URL in Settings, then pick a model in the model list.".into()
            } else {
                "No model selected: pick one in the model list beside the provider.".into()
            });
            return;
        }
        let mut m = ChatMessage::new(Role::User, text);
        m.images = images;
        self.messages.push(m);
        let catalog: String = aurafx_filters::registry()
            .iter()
            .map(|f| format!("- {}: {} [{}]", f.id, f.description, f.params.iter().map(|p| p.key.as_str()).collect::<Vec<_>>().join(", ")))
            .collect::<Vec<_>>()
            .join("\n");
        let mut system = aurafx_ai::system_prompt(&pipeline, &catalog);
        if let Some(dir) = skills_dir() {
            system += &aurafx_ai::skills_prompt(&aurafx_ai::load_skills(&dir, &self.cfg.skills_off));
        }
        // Keep the last 20 messages to bound context.
        let msgs: Vec<ChatMessage> = self.messages.iter().rev().take(20).rev().cloned().collect();
        self.stream = Some(aurafx_ai::stream_chat(self.cfg.clone(), system, msgs));
        self.partial.clear();
    }

    pub fn stop_agent(&mut self) {
        self.agent_active = false;
        self.agent_pending = 0;
        self.agent_continue = false;
        self.agent_results.clear();
        if self.stream.take().is_some() && !self.partial.is_empty() {
            let text = std::mem::take(&mut self.partial);
            self.messages.push(ChatMessage::new(Role::Assistant, format!("{text}\n\n*(stopped)*")));
        }
    }

    pub fn on_python_result(&mut self, r: &PythonResult) {
        let idx = self.messages.len().saturating_sub(1);
        let mut text = String::new();
        if !r.output.stdout.is_empty() {
            text += &r.output.stdout;
        }
        if let Some(v) = &r.output.value {
            text += v;
        }
        let err = r.output.error.is_some();
        if let Some(e) = &r.output.error {
            text += e;
        }
        if text.is_empty() {
            text = "ok".into();
        }
        if r.origin == "agent" && self.agent_active {
            let mut t = text.clone();
            if t.len() > 4000 {
                t = format!("{}\n… (truncated)", &t[..4000]);
            }
            self.agent_results.push(format!("{}:\n```\n{}\n```", if err { "Error" } else { "Output" }, t.trim_end()));
            self.agent_pending = self.agent_pending.saturating_sub(1);
            if self.agent_pending == 0 {
                self.agent_continue = true;
            }
        }
        self.code_results.push((idx, text, err));
    }

    pub fn settings_ui(&mut self, ui: &mut egui::Ui, settings: &mut Settings) {
        let before = self.cfg.clone();
        egui::Grid::new("ai_settings").num_columns(2).spacing([12.0, 6.0]).show(ui, |ui| {
            ui.label("Provider");
            egui::ComboBox::from_id_salt("provider").selected_text(self.cfg.provider.label()).show_ui(ui, |ui| {
                for p in ProviderKind::ALL {
                    ui.selectable_value(&mut self.cfg.provider, p, p.label());
                }
            });
            ui.end_row();
            ui.label("Model");
            ui.horizontal(|ui| {
                egui::ComboBox::from_id_salt("model")
                    .selected_text(if self.cfg.model.is_empty() { NO_MODEL.to_string() } else { self.cfg.model.clone() })
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut self.cfg.model, String::new(), NO_MODEL);
                        for m in self.models.clone() {
                            ui.selectable_value(&mut self.cfg.model, m.clone(), m);
                        }
                    });
                ui.text_edit_singleline(&mut self.cfg.model);
                if ui.button(crate::icons::ARROW_CLOCKWISE).on_hover_text("Refresh model list").clicked() {
                    self.refresh_models();
                }
            });
            ui.end_row();
            match self.cfg.provider {
                ProviderKind::Ollama => {
                    ui.label("Ollama URL");
                    ui.text_edit_singleline(&mut self.cfg.ollama_url);
                    ui.end_row();
                }
                ProviderKind::Anthropic => {
                    ui.label("API key");
                    ui.add(egui::TextEdit::singleline(&mut self.cfg.api_key).password(true).hint_text("or set ANTHROPIC_API_KEY"));
                    ui.end_row();
                    ui.label("API URL");
                    ui.text_edit_singleline(&mut self.cfg.anthropic_url);
                    ui.end_row();
                }
                ProviderKind::OpenAI => {
                    ui.label("API key");
                    ui.add(egui::TextEdit::singleline(&mut self.cfg.api_key).password(true).hint_text("or set OPENAI_API_KEY"));
                    ui.end_row();
                    ui.label("Base URL");
                    ui.text_edit_singleline(&mut self.cfg.openai_url).on_hover_text("Any OpenAI-compatible server: OpenAI, Groq, vLLM, LM Studio, OpenRouter…");
                    ui.end_row();
                }
            }
            ui.label("Max tokens");
            ui.add(egui::DragValue::new(&mut self.cfg.max_tokens).range(256..=64000));
            ui.end_row();
            ui.label("Temperature");
            ui.add(egui::Slider::new(&mut self.cfg.temperature, 0.0..=1.5));
            ui.end_row();
            ui.label("Privacy");
            ui.vertical(|ui| {
                ui.checkbox(&mut self.cfg.local_only, "Local only").on_hover_text(
                    "Send nothing off this computer's network: only Ollama at a local or private address, and no cloud models",
                );
                ui.checkbox(&mut self.cfg.redact_paths, "Hide file paths").on_hover_text(
                    "Replace your home folder with ~ and full file paths with their file names in what is sent (the assistant then names files without their folders)",
                );
            });
            ui.end_row();
            ui.label("Auto-run code");
            ui.checkbox(&mut settings.auto_run_ai_code, "Run Python blocks from the assistant automatically")
                .on_hover_text("Off by default: review code before running it.");
            ui.end_row();
        });
        // Saved prompts.
        egui::CollapsingHeader::new(format!("Prompts ({})", self.cfg.prompts.len())).id_salt("ai prompts").show(ui, |ui| {
            ui.label(RichText::new("Offered by the Prompts button of the assistant; {object} stands for the selected object's name.").weak().small());
            let mut remove = None;
            for (i, p) in self.cfg.prompts.iter_mut().enumerate() {
                ui.horizontal(|ui| {
                    ui.add(egui::TextEdit::singleline(&mut p.name).desired_width(110.0));
                    ui.add(egui::TextEdit::multiline(&mut p.text).desired_rows(2).desired_width(300.0));
                    if crate::touch::small_button(ui, "×").on_hover_text("Remove this prompt").clicked() {
                        remove = Some(i);
                    }
                });
            }
            if let Some(i) = remove {
                self.cfg.prompts.remove(i);
            }
            ui.horizontal(|ui| {
                if ui.button("Add a prompt").clicked() {
                    self.cfg.prompts.push(aurafx_ai::PromptTemplate { name: "New prompt".into(), text: "Show {object} ...".into() });
                }
                if ui.button("Defaults").clicked() {
                    self.cfg.prompts = aurafx_ai::default_prompts();
                }
            });
        });
        // Skills: Markdown files whose text joins the assistant's instructions.
        let dir = skills_dir();
        let all = dir.as_ref().map(|d| aurafx_ai::load_skills(d, &[])).unwrap_or_default();
        egui::CollapsingHeader::new(format!("Skills ({})", all.len())).id_salt("ai skills").show(ui, |ui| {
            ui.label(
                RichText::new("Markdown files in the skills folder (house rules, usual steps, naming): the text of each one ticked goes with every request.")
                    .weak()
                    .small(),
            );
            for (name, text) in &all {
                let mut on = !self.cfg.skills_off.contains(name);
                if ui.checkbox(&mut on, name).on_hover_text(text.chars().take(400).collect::<String>()).changed() {
                    if on {
                        self.cfg.skills_off.retain(|n| n != name);
                    } else {
                        self.cfg.skills_off.push(name.clone());
                    }
                }
            }
            if let Some(d) = &dir {
                ui.horizontal(|ui| {
                    ui.label(RichText::new(d.display().to_string()).monospace().small());
                    if crate::touch::small_button(ui, "Open").clicked() {
                        let _ = crate::plugins_ui::open_in_os(d);
                    }
                });
            }
        });
        if before.prompts != self.cfg.prompts || before.skills_off != self.cfg.skills_off {
            settings.ai = self.cfg.clone();
        }
        if before.provider != self.cfg.provider
            || before.ollama_url != self.cfg.ollama_url
            || before.api_key != self.cfg.api_key
            || before.openai_url != self.cfg.openai_url
        {
            if before.provider != self.cfg.provider {
                self.cfg.model.clear();
            }
            self.refresh_models();
        }
        if !cfg_eq(&before, &self.cfg) {
            settings.ai = self.cfg.clone();
        }
    }
}

fn cfg_eq(a: &AiConfig, o: &AiConfig) -> bool {
    a.provider == o.provider
        && a.model == o.model
        && a.api_key == o.api_key
        && a.ollama_url == o.ollama_url
        && a.openai_url == o.openai_url
        && a.anthropic_url == o.anthropic_url
        && a.max_tokens == o.max_tokens
        && a.temperature == o.temperature
        && a.local_only == o.local_only
        && a.redact_paths == o.redact_paths
}

/// The "Describe a filter" window: a sentence about what to see becomes a filter (or a short chain)
/// on the selected object, made by the assistant in agent mode.
pub fn describe_window(state: &mut State, ctx: &egui::Context) {
    if !state.assistant.describe_open {
        return;
    }
    let active = {
        let s = state.session.lock();
        s.active.and_then(|id| s.nodes.get(&id).map(|n| n.name.clone()))
    };
    let mut open = true;
    let mut go = false;
    egui::Window::new(format!("{} Describe a filter", crate::icons::SPARKLE)).open(&mut open).collapsible(false).default_width(460.0).show_fit(ctx, |ui| {
        match &active {
            Some(n) => ui.label(format!("Say what you want to see in \"{n}\"; the assistant adds the filters that show it.")),
            None => ui.label(RichText::new("Select an object in the Pipeline first.").color(crate::theme::ERROR)),
        };
        let r = ui.add(
            egui::TextEdit::multiline(&mut state.assistant.describe_text)
                .desired_rows(2)
                .desired_width(f32::INFINITY)
                .hint_text("regions where the von Mises stress is above 300 MPa\nthe vortex cores of the flow\na slice through the hottest point"),
        );
        let enter = r.has_focus() && ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Enter));
        ui.horizontal(|ui| {
            let ok = active.is_some() && !state.assistant.describe_text.trim().is_empty() && !state.assistant.is_streaming();
            go = (ui.add_enabled(ok, egui::Button::new(RichText::new(format!("{} Make it", crate::icons::SPARKLE)).strong())).clicked() || enter) && ok;
            ui.label(RichText::new("uses the assistant's model; the steps show in the Assistant panel").weak().small());
        });
    });
    if go {
        let want = std::mem::take(&mut state.assistant.describe_text);
        let name = active.unwrap_or_default();
        let text = format!(
            "Add filters to the object \"{name}\" (afx.active()) that show this: {}. Use the afx filter calls on that node (one filter, or a short chain), \
             choose the arrays and values from its data (look at its arrays and ranges first when you need them), color the result sensibly, \
             then say in one sentence what the result shows.",
            want.trim()
        );
        let desc = state.session.lock().describe();
        state.settings.ai_agent_mode = true;
        state.assistant.agent_active = true;
        state.assistant.reset_agent_steps();
        state.assistant.send(text, desc);
        state.layout_actions.push(crate::app::LayoutAction::Show(crate::app::Pane::Assistant));
        state.assistant.describe_open = false;
    } else if !open {
        state.assistant.describe_open = false;
    }
}

pub fn assistant_ui(state: &mut State, ui: &mut egui::Ui) {
    // Header: provider + model quick switch.
    crate::touch::row(ui, "assistant top", |ui| {
        let a = &mut state.assistant;
        let before = a.cfg.provider;
        egui::ComboBox::from_id_salt("asst_provider").selected_text(a.cfg.provider.label()).width(140.0).show_ui(ui, |ui| {
            for p in ProviderKind::ALL {
                ui.selectable_value(&mut a.cfg.provider, p, p.label());
            }
        });
        if before != a.cfg.provider {
            a.cfg.model.clear();
            state.settings.ai = a.cfg.clone();
            a.refresh_models();
        }
        let model_label = if a.cfg.model.is_empty() { NO_MODEL.to_string() } else { a.cfg.model.clone() };
        let before_model = a.cfg.model.clone();
        egui::ComboBox::from_id_salt("asst_model").selected_text(model_label).width(160.0).show_ui(ui, |ui| {
            ui.selectable_value(&mut a.cfg.model, String::new(), NO_MODEL);
            let mut listed = a.models.clone();
            // Cloud providers: their usual model is offered even when the list is empty.
            if a.cfg.provider != ProviderKind::Ollama && !listed.contains(&a.cfg.model_or_default()) {
                listed.insert(0, a.cfg.model_or_default());
            }
            for m in listed {
                ui.selectable_value(&mut a.cfg.model, m.clone(), m);
            }
            if a.models.is_empty() {
                ui.label(
                    RichText::new(match a.cfg.provider {
                        ProviderKind::Ollama => format!("No models at {}. Run `ollama pull <model>` or set the URL in Settings.", a.cfg.ollama_url),
                        _ => "No models listed; type one in Settings.".to_string(),
                    })
                    .weak(),
                );
            }
        });
        if before_model != a.cfg.model {
            state.settings.ai = a.cfg.clone();
            state.settings.save();
        }
        if ui.button(crate::icons::GEAR).on_hover_text("AI settings").clicked() {
            state.show_settings = true;
        }
        if ui.button(crate::icons::TRASH).on_hover_text("Clear conversation").clicked() {
            state.assistant.messages.clear();
            state.assistant.code_results.clear();
        }
    });
    if let Some(e) = state.assistant.error.clone() {
        ui.colored_label(crate::theme::ERROR, e);
    }
    ui.separator();
    // (The text box and the row of buttons below it; both are taller in the touch interfaces.)
    let input_h = if crate::touch::mode().is_touch() { 160.0 } else { 100.0 };
    let h = (ui.available_height() - input_h).max(60.0);
    let mut run_code: Option<String> = None;
    let mut send_text: Option<String> = None;
    egui::ScrollArea::vertical().id_salt("chat").auto_shrink([false, false]).max_height(h).stick_to_bottom(true).show(ui, |ui| {
        let a = &mut state.assistant;
        if a.messages.is_empty() && a.partial.is_empty() {
            ui.label(RichText::new("Ask about your data or tell me what to visualize, e.g.\n• \"slice this through the middle and color by temperature\"\n• \"what arrays does this file have?\"\n• \"make an isosurface at the mean value\"\n• \"plot the maximum stress over time\"").weak());
            ui.add_space(6.0);
            for s in ["Describe the loaded data", "Show a slice colored by the main scalar", "Make a contour at the median value", "What filters would help analyze this?"] {
                if crate::touch::small_button(ui, s).clicked() {
                    send_text = Some(s.to_string());
                }
            }
        }
        let n = a.messages.len();
        for i in 0..n {
            let m = &a.messages[i];
            if m.role == Role::User && m.content.starts_with(TOOL_PREFIX) {
                egui::CollapsingHeader::new(RichText::new(format!("{} Code ran — results sent back to the assistant", crate::icons::TERMINAL_WINDOW)).weak().small()).id_salt(("toolmsg", i)).show(ui, |ui| {
                    ui.label(RichText::new(m.content.trim_start_matches(TOOL_PREFIX).trim()).monospace().small());
                });
                continue;
            }
            let (name, color) = match m.role {
                Role::User => ("You", crate::theme::INFO),
                Role::Assistant => ("auraFX AI", crate::theme::ACCENT_SOFT),
            };
            ui.label(RichText::new(name).strong().color(color));
            egui::Frame::group(ui.style()).show(ui, |ui| {
                ui.set_width(ui.available_width());
                CommonMarkViewer::new().show(ui, &mut a.cache, &m.content);
            });
            if m.role == Role::Assistant {
                let blocks = aurafx_ai::python_blocks(&m.content);
                for (k, b) in blocks.iter().enumerate() {
                    ui.horizontal(|ui| {
                        if ui.button(format!("{} Run code {}", crate::icons::PLAY, if blocks.len() > 1 { format!("#{}", k + 1) } else { String::new() })).clicked() {
                            run_code = Some(b.clone());
                        }
                        if crate::touch::small_button(ui, "Copy to console").clicked() {
                            state.console.input = b.clone();
                        }
                    });
                }
                for (idx, text, err) in a.code_results.iter().filter(|(idx, _, _)| *idx == i) {
                    let _ = idx;
                    let col = if *err { crate::theme::ERROR } else { ui.visuals().text_color() };
                    ui.label(RichText::new(text.trim_end()).monospace().small().color(col));
                    if *err && crate::touch::small_button(ui, "Ask AI to fix").clicked() {
                        send_text = Some(format!("Running your code failed with:\n```\n{}\n```\nPlease fix it.", text.trim_end()));
                    }
                }
            }
            ui.add_space(4.0);
        }
        if a.stream.is_some() {
            ui.label(RichText::new("auraFX AI").strong().color(crate::theme::ACCENT_SOFT));
            egui::Frame::group(ui.style()).show(ui, |ui| {
                ui.set_width(ui.available_width());
                if a.partial.is_empty() {
                    ui.spinner();
                } else {
                    CommonMarkViewer::new().show(ui, &mut a.cache, &a.partial);
                }
            });
        }
    });
    ui.separator();
    let resp = ui.add(
        egui::TextEdit::multiline(&mut state.assistant.input)
            .desired_rows(2)
            .desired_width(f32::INFINITY)
            .hint_text("Ask the assistant… (Enter to send, Shift+Enter for newline)"),
    );
    if state.assistant.focus_input {
        resp.request_focus();
        state.assistant.focus_input = false;
    }
    let enter = resp.has_focus() && ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Enter));
    crate::touch::row(ui, "assistant send", |ui| {
        // Saved prompts: one click puts it in the box (the selected object filled in).
        let object = state.session.lock().active_node().map(|n| n.name.clone()).unwrap_or_else(|| "the data".into());
        let prompts = state.assistant.cfg.prompts.clone();
        ui.menu_button("Prompts", |ui| {
            for p in &prompts {
                if ui.button(&p.name).on_hover_text(p.fill(&object)).clicked() {
                    state.assistant.input = p.fill(&object);
                    state.assistant.focus_input = true;
                    ui.close();
                }
            }
            ui.separator();
            if ui.add_enabled(!state.assistant.input.trim().is_empty(), egui::Button::new("Save the message as a prompt")).clicked() {
                let text = state.assistant.input.trim().to_string();
                let name: String = text.split_whitespace().take(4).collect::<Vec<_>>().join(" ");
                state.assistant.cfg.prompts.push(aurafx_ai::PromptTemplate { name, text });
                state.settings.ai = state.assistant.cfg.clone();
                state.settings.save();
                ui.close();
            }
        })
        .response
        .on_hover_text("Saved prompts (edit them in Settings › AI assistant)");

        let busy = state.assistant.is_streaming();
        if ui.add_enabled(!busy, egui::Button::new("Send")).clicked() || enter {
            let t = std::mem::take(&mut state.assistant.input);
            send_text = Some(t);
        }
        ui.checkbox(&mut state.assistant.include_view, format!("{} view", crate::icons::CAMERA))
            .on_hover_text("Send a picture of the 3D view with the message, for models that see images (llava, gemma, qwen-vl, Claude, GPT-4o…)");
        ui.checkbox(&mut state.settings.ai_agent_mode, "agent mode").on_hover_text(
            "Do it for me: the assistant's code runs automatically and the results go back to it, so it can continue or fix errors (up to 8 steps)",
        );
        if !state.settings.ai_agent_mode {
            ui.checkbox(&mut state.settings.auto_run_ai_code, "auto-run code").on_hover_text("Run Python blocks from replies automatically");
        }
        if state.assistant.agent_active || state.assistant.is_streaming() {
            if ui.button(format!("{} Stop", crate::icons::X)).clicked() {
                state.assistant.stop_agent();
            }
            if state.assistant.agent_active {
                ui.label(RichText::new(format!("step {}/{}", state.assistant.agent_steps + 1, AGENT_MAX_STEPS)).weak().small());
            }
        }
    });
    if let Some(t) = send_text {
        let desc = state.session.lock().describe();
        if state.settings.ai_agent_mode {
            state.assistant.agent_active = true;
            state.assistant.agent_steps = 0;
        }
        if state.assistant.include_view {
            let session = state.session.clone();
            state.assistant.send_with_view(t, desc, &session);
        } else {
            state.assistant.send(t, desc);
        }
    }
    // Agent mode: run new code blocks, then send the results back.
    if state.settings.ai_agent_mode && !state.assistant.is_streaming() {
        let n = state.assistant.messages.len();
        while state.assistant.auto_ran < n {
            let i = state.assistant.auto_ran;
            state.assistant.auto_ran += 1;
            let m = state.assistant.messages[i].clone();
            if m.role != Role::Assistant || !state.assistant.agent_active {
                continue;
            }
            let blocks = aurafx_ai::python_blocks(&m.content);
            if blocks.is_empty() || state.assistant.agent_steps >= AGENT_MAX_STEPS {
                state.assistant.agent_active = false;
                continue;
            }
            state.assistant.agent_pending += blocks.len();
            for b in blocks {
                state.run_python(&b, "agent");
            }
        }
        if state.assistant.agent_continue && state.assistant.agent_active {
            state.assistant.agent_continue = false;
            state.assistant.agent_steps += 1;
            let results = std::mem::take(&mut state.assistant.agent_results).join("\n\n");
            let desc = state.session.lock().describe();
            let msg = format!(
                "{TOOL_PREFIX}\n{results}\n\nUpdated pipeline:\n{desc}\nIf the task is done, reply with a short summary and no code. Otherwise reply with the next ```python block (fix any error first)."
            );
            state.assistant.send(msg, desc);
        }
    } else if state.settings.auto_run_ai_code && !state.assistant.is_streaming() {
        let n = state.assistant.messages.len();
        while state.assistant.auto_ran < n {
            let i = state.assistant.auto_ran;
            state.assistant.auto_ran += 1;
            if state.assistant.messages[i].role == Role::Assistant {
                for b in aurafx_ai::python_blocks(&state.assistant.messages[i].content) {
                    state.run_python(&b, "assistant");
                }
            }
        }
    } else {
        state.assistant.auto_ran = state.assistant.messages.len();
    }
    if let Some(code) = run_code {
        state.run_python(&code, "assistant");
    }
}
