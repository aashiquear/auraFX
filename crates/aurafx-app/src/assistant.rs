//! AI assistant panel: chat with pipeline context, runnable code blocks.

use crate::app::State;
use crate::settings::Settings;
use crate::worker::PythonResult;
use aurafx_ai::{AiConfig, ChatMessage, ProviderKind, Role, StreamEvent};
use egui::RichText;
use egui_commonmark::{CommonMarkCache, CommonMarkViewer};
use std::sync::mpsc::Receiver;

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
}

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
                        if self.cfg.model.is_empty() {
                            if let Some(first) = self.models.first() {
                                self.cfg.model = first.clone();
                            }
                        }
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
                self.messages.push(ChatMessage { role: Role::Assistant, content: std::mem::take(&mut self.partial) });
            }
            self.stream = None;
        }
    }

    pub fn send(&mut self, text: String, pipeline: String) {
        if text.trim().is_empty() || self.stream.is_some() {
            return;
        }
        self.error = None;
        self.messages.push(ChatMessage { role: Role::User, content: text });
        let catalog: String = aurafx_filters::registry()
            .iter()
            .map(|f| format!("- {}: {} [{}]", f.id, f.description, f.params.iter().map(|p| p.key.as_str()).collect::<Vec<_>>().join(", ")))
            .collect::<Vec<_>>()
            .join("\n");
        let system = aurafx_ai::system_prompt(&pipeline, &catalog);
        // Keep the last 20 messages to bound context.
        let msgs: Vec<ChatMessage> = self.messages.iter().rev().take(20).rev().cloned().collect();
        self.stream = Some(aurafx_ai::stream_chat(self.cfg.clone(), system, msgs));
        self.partial.clear();
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
                    .selected_text(if self.cfg.model.is_empty() { "(default)".to_string() } else { self.cfg.model.clone() })
                    .show_ui(ui, |ui| {
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
            ui.label("Auto-run code");
            ui.checkbox(&mut settings.auto_run_ai_code, "Run Python blocks from the assistant automatically")
                .on_hover_text("Off by default: review code before running it.");
            ui.end_row();
        });
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
}

pub fn assistant_ui(state: &mut State, ui: &mut egui::Ui) {
    // Header: provider + model quick switch.
    ui.horizontal(|ui| {
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
        let model_label = if a.cfg.model.is_empty() { a.cfg.model_or_default() } else { a.cfg.model.clone() };
        let before_model = a.cfg.model.clone();
        egui::ComboBox::from_id_salt("asst_model").selected_text(model_label).width(160.0).show_ui(ui, |ui| {
            for m in a.models.clone() {
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
    let input_h = 100.0;
    let h = (ui.available_height() - input_h).max(60.0);
    let mut run_code: Option<String> = None;
    let mut send_text: Option<String> = None;
    egui::ScrollArea::vertical().id_salt("chat").auto_shrink([false, false]).max_height(h).stick_to_bottom(true).show(ui, |ui| {
        let a = &mut state.assistant;
        if a.messages.is_empty() && a.partial.is_empty() {
            ui.label(RichText::new("Ask about your data or tell me what to visualize, e.g.\n• \"slice this through the middle and color by temperature\"\n• \"what arrays does this file have?\"\n• \"make an isosurface at the mean value\"\n• \"plot the maximum stress over time\"").weak());
            ui.add_space(6.0);
            for s in ["Describe the loaded data", "Show a slice colored by the main scalar", "Make a contour at the median value", "What filters would help analyze this?"] {
                if ui.small_button(s).clicked() {
                    send_text = Some(s.to_string());
                }
            }
        }
        let n = a.messages.len();
        for i in 0..n {
            let m = &a.messages[i];
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
                        if ui.small_button("Copy to console").clicked() {
                            state.console.input = b.clone();
                        }
                    });
                }
                for (idx, text, err) in a.code_results.iter().filter(|(idx, _, _)| *idx == i) {
                    let _ = idx;
                    let col = if *err { crate::theme::ERROR } else { ui.visuals().text_color() };
                    ui.label(RichText::new(text.trim_end()).monospace().small().color(col));
                    if *err && ui.small_button("Ask AI to fix").clicked() {
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
    ui.horizontal(|ui| {
        let busy = state.assistant.is_streaming();
        if ui.add_enabled(!busy, egui::Button::new("Send")).clicked() || enter {
            let t = std::mem::take(&mut state.assistant.input);
            send_text = Some(t);
        }
        ui.checkbox(&mut state.settings.auto_run_ai_code, "auto-run code").on_hover_text("Run Python blocks from replies automatically");
    });
    if let Some(t) = send_text {
        let desc = state.session.lock().describe();
        state.assistant.send(t, desc);
    }
    // Auto-run newly completed assistant messages.
    if state.settings.auto_run_ai_code && !state.assistant.is_streaming() {
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
