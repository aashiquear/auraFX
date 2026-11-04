//! Simulations panel: run jobs locally / over SSH / via Slurm, watch logs, monitor GPUs,
//! manage hosts and browse remote files.

use crate::app::State;
use crate::icons;
use aurafx_jobs::{gpu::GpuInfo, templates::TEMPLATES, GpuRequest, Host, HostKind, JobId, JobSpec, JobState, Scheduler};
use egui::{Color32, RichText};
use std::sync::mpsc::Receiver;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum GpuChoice {
    None,
    Any,
    Nvidia,
    Amd,
}

pub struct JobsPanel {
    pub template: usize,
    pub host: usize,
    pub name: String,
    pub workdir: String,
    pub exe: String,
    pub input: String,
    pub np: u32,
    pub gpu: GpuChoice,
    pub gpu_ids: String,
    pub slurm: bool,
    pub partition: String,
    pub time: String,
    pub nodes: u32,
    pub extra: String,
    pub outputs: String,
    pub auto_load: bool,
    pub env: String,
    pub command_override: Option<String>,
    pub sweep_var: String,
    pub sweep_values: String,
    pub selected: Option<JobId>,
    pub log_filter: String,
    // Host editor
    pub new_host_name: String,
    pub new_host_target: String,
    pub new_host_port: String,
    pub new_host_identity: String,
    pub new_host_jump: String,
    pub new_host_setup: String,
    pub host_test: Option<Receiver<(String, anyhow::Result<String>, Vec<GpuInfo>)>>,
    pub host_test_result: Option<(String, Result<String, String>, Vec<GpuInfo>)>,
    // GPU monitor
    pub gpus: Vec<GpuInfo>,
    gpu_rx: Option<Receiver<Vec<GpuInfo>>>,
    gpu_last: f64,
    pub gpu_host: usize,
    // Remote browser
    pub browse_host: usize,
    pub browse_path: String,
    pub browse_entries: Vec<(String, bool, u64)>,
    browse_rx: Option<Receiver<anyhow::Result<Vec<(String, bool, u64)>>>>,
    pub browse_error: Option<String>,
    fetch_rx: Option<Receiver<anyhow::Result<std::path::PathBuf>>>,
}

impl Default for JobsPanel {
    fn default() -> Self {
        let t = &TEMPLATES[0];
        Self {
            template: 0,
            host: 0,
            name: "LAMMPS run".into(),
            workdir: std::env::current_dir().map(|p| p.display().to_string()).unwrap_or_else(|_| ".".into()),
            exe: t.default_exe.into(),
            input: t.default_input.into(),
            np: 1,
            gpu: GpuChoice::None,
            gpu_ids: "0".into(),
            slurm: false,
            partition: String::new(),
            time: "01:00:00".into(),
            nodes: 1,
            extra: String::new(),
            outputs: t.outputs.into(),
            auto_load: true,
            env: String::new(),
            command_override: None,
            sweep_var: String::new(),
            sweep_values: String::new(),
            selected: None,
            log_filter: String::new(),
            new_host_name: String::new(),
            new_host_target: String::new(),
            new_host_port: String::new(),
            new_host_identity: String::new(),
            new_host_jump: String::new(),
            new_host_setup: String::new(),
            host_test: None,
            host_test_result: None,
            gpus: Vec::new(),
            gpu_rx: None,
            gpu_last: -100.0,
            gpu_host: 0,
            browse_host: 0,
            browse_path: "~".into(),
            browse_entries: Vec::new(),
            browse_rx: None,
            browse_error: None,
            fetch_rx: None,
        }
    }
}

impl JobsPanel {
    fn gpu_request(&self) -> GpuRequest {
        let ids: Vec<u32> = self.gpu_ids.split(',').filter_map(|s| s.trim().parse().ok()).collect();
        match self.gpu {
            GpuChoice::None => GpuRequest::None,
            GpuChoice::Any => GpuRequest::Any(ids),
            GpuChoice::Nvidia => GpuRequest::Nvidia(ids),
            GpuChoice::Amd => GpuRequest::Amd(ids),
        }
    }

    fn generated_command(&self) -> String {
        aurafx_jobs::templates::render(&TEMPLATES[self.template], &self.exe, &self.input, self.np, &self.gpu_request())
    }

    /// Poll background requests; called every frame.
    pub fn poll(&mut self, ctx: &egui::Context, now: f64, monitoring: bool) {
        if let Some(rx) = &self.gpu_rx {
            if let Ok(g) = rx.try_recv() {
                self.gpus = g;
                self.gpu_rx = None;
            }
        }
        if monitoring && self.gpu_rx.is_none() && now - self.gpu_last > 3.0 {
            self.gpu_last = now;
            let host = aurafx_jobs::hosts().get(self.gpu_host).cloned().unwrap_or_else(Host::local);
            let (tx, rx) = std::sync::mpsc::channel();
            let c = ctx.clone();
            std::thread::spawn(move || {
                let _ = tx.send(aurafx_jobs::gpu::query(&host));
                c.request_repaint();
            });
            self.gpu_rx = Some(rx);
        }
        if let Some(rx) = &self.host_test {
            if let Ok((name, r, g)) = rx.try_recv() {
                self.host_test_result = Some((name, r.map_err(|e| format!("{e:#}")), g));
                self.host_test = None;
            }
        }
        if let Some(rx) = &self.browse_rx {
            if let Ok(r) = rx.try_recv() {
                match r {
                    Ok(mut e) => {
                        e.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.to_lowercase().cmp(&b.0.to_lowercase())));
                        self.browse_entries = e;
                        self.browse_error = None;
                    }
                    Err(e) => self.browse_error = Some(format!("{e:#}")),
                }
                self.browse_rx = None;
            }
        }
        if self.host_test.is_some() || self.browse_rx.is_some() || self.fetch_rx.is_some() {
            ctx.request_repaint_after(std::time::Duration::from_millis(150));
        }
        if monitoring {
            ctx.request_repaint_after(std::time::Duration::from_millis(1000));
        }
    }

    fn browse(&mut self, ctx: &egui::Context) {
        let host = aurafx_jobs::hosts().get(self.browse_host).cloned().unwrap_or_else(Host::local);
        let path = self.browse_path.clone();
        let (tx, rx) = std::sync::mpsc::channel();
        let c = ctx.clone();
        std::thread::spawn(move || {
            let _ = tx.send(host.list_dir(&path));
            c.request_repaint();
        });
        self.browse_rx = Some(rx);
    }
}

fn state_color(s: &JobState) -> Color32 {
    match s {
        JobState::Running => crate::theme::INFO,
        JobState::Queued | JobState::Submitted(_) => crate::theme::WARN,
        JobState::Finished(0) => crate::theme::OK,
        JobState::Cancelled => Color32::from_gray(150),
        _ => crate::theme::ERROR,
    }
}

fn fmt_dur(d: std::time::Duration) -> String {
    let s = d.as_secs();
    if s >= 3600 {
        format!("{}h{:02}m", s / 3600, (s % 3600) / 60)
    } else if s >= 60 {
        format!("{}m{:02}s", s / 60, s % 60)
    } else {
        format!("{:.1}s", d.as_secs_f64())
    }
}

pub fn jobs_ui(state: &mut State, ui: &mut egui::Ui) {
    let now = state.viewport.time_seconds();
    let ctx = ui.ctx().clone();
    // Fetched remote file ready to open?
    if let Some(rx) = &state.jobs_panel.fetch_rx {
        if let Ok(r) = rx.try_recv() {
            state.jobs_panel.fetch_rx = None;
            match r {
                Ok(p) => state.open_file(p),
                Err(e) => state.toast(format!("Download failed: {e:#}"), true),
            }
        }
    }
    let hosts = aurafx_jobs::hosts();
    egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
        // ---------------------------------------------------------- new job
        egui::CollapsingHeader::new(RichText::new(format!("{} New simulation", icons::PLAY)).strong()).default_open(true).show(ui, |ui| {
            let p = &mut state.jobs_panel;
            egui::Grid::new("newjob").num_columns(2).spacing([10.0, 6.0]).show(ui, |ui| {
                ui.label("Template");
                let before = p.template;
                egui::ComboBox::from_id_salt("tpl").selected_text(TEMPLATES[p.template].label).show_ui(ui, |ui| {
                    for (i, t) in TEMPLATES.iter().enumerate() {
                        ui.selectable_value(&mut p.template, i, t.label).on_hover_text(t.description);
                    }
                });
                if before != p.template {
                    let t = &TEMPLATES[p.template];
                    p.exe = t.default_exe.into();
                    p.input = t.default_input.into();
                    p.outputs = t.outputs.into();
                    p.name = format!("{} run", t.label);
                    p.command_override = None;
                }
                ui.end_row();
                ui.label("Run on");
                ui.horizontal(|ui| {
                    egui::ComboBox::from_id_salt("host").selected_text(hosts.get(p.host).map(|h| h.name.clone()).unwrap_or_default()).show_ui(ui, |ui| {
                        for (i, h) in hosts.iter().enumerate() {
                            ui.selectable_value(&mut p.host, i, &h.name);
                        }
                    });
                    ui.checkbox(&mut p.slurm, "via Slurm");
                });
                ui.end_row();
                ui.label("Name");
                ui.text_edit_singleline(&mut p.name);
                ui.end_row();
                ui.label("Working directory");
                ui.horizontal(|ui| {
                    ui.add(egui::TextEdit::singleline(&mut p.workdir).desired_width(220.0));
                    if hosts.get(p.host).map(|h| h.is_local()).unwrap_or(true) && ui.button(icons::FOLDER_OPEN).clicked() {
                        if let Some(d) = crate::dialogs::FileDialog::new().set_directory(&p.workdir).pick_folder() {
                            p.workdir = d.display().to_string();
                        }
                    }
                });
                ui.end_row();
                ui.label("Program");
                ui.text_edit_singleline(&mut p.exe);
                ui.end_row();
                ui.label("Input");
                ui.text_edit_singleline(&mut p.input);
                ui.end_row();
                ui.label("MPI processes");
                ui.add(egui::DragValue::new(&mut p.np).range(1..=4096));
                ui.end_row();
                ui.label("GPU");
                ui.horizontal(|ui| {
                    ui.selectable_value(&mut p.gpu, GpuChoice::None, "None");
                    ui.selectable_value(&mut p.gpu, GpuChoice::Any, "Any");
                    ui.selectable_value(&mut p.gpu, GpuChoice::Nvidia, "NVIDIA");
                    ui.selectable_value(&mut p.gpu, GpuChoice::Amd, "AMD");
                    if p.gpu != GpuChoice::None {
                        ui.label("ids");
                        ui.add(egui::TextEdit::singleline(&mut p.gpu_ids).desired_width(60.0));
                    }
                });
                ui.end_row();
                if p.slurm {
                    ui.label("Partition / time");
                    ui.horizontal(|ui| {
                        ui.add(egui::TextEdit::singleline(&mut p.partition).hint_text("partition").desired_width(100.0));
                        ui.add(egui::TextEdit::singleline(&mut p.time).desired_width(80.0));
                        ui.label("nodes");
                        ui.add(egui::DragValue::new(&mut p.nodes).range(1..=10000));
                    });
                    ui.end_row();
                    ui.label("Extra #SBATCH");
                    ui.add(egui::TextEdit::multiline(&mut p.extra).desired_rows(1).hint_text("--account=myproj"));
                    ui.end_row();
                }
                ui.label("Environment");
                ui.add(egui::TextEdit::multiline(&mut p.env).desired_rows(1).hint_text("OMP_NUM_THREADS=8 (one per line)"));
                ui.end_row();
                ui.label("Sweep");
                ui.horizontal(|ui| {
                    ui.add(egui::TextEdit::singleline(&mut p.sweep_var).hint_text("variable").desired_width(70.0));
                    ui.label("=");
                    ui.add(egui::TextEdit::singleline(&mut p.sweep_values).hint_text("300, 350, 400").desired_width(140.0));
                })
                .response
                .on_hover_text("Run one job per value. {variable} in the command is replaced; each run gets its own folder <workdir>/<variable>_<value>.");
                ui.end_row();
                ui.label("Result files");
                ui.horizontal(|ui| {
                    ui.add(egui::TextEdit::singleline(&mut p.outputs).desired_width(160.0));
                    ui.checkbox(&mut p.auto_load, "open when done");
                });
                ui.end_row();
            });
            let generated = p.generated_command();
            let mut cmd = p.command_override.clone().unwrap_or(generated.clone());
            ui.label(RichText::new("Command (editable)").weak());
            if ui.add(egui::TextEdit::multiline(&mut cmd).code_editor().desired_rows(2).desired_width(f32::INFINITY)).changed() {
                p.command_override = if cmd == generated { None } else { Some(cmd.clone()) };
            }
            ui.horizontal(|ui| {
                if ui.button(RichText::new(format!("{} Run", icons::PLAY)).strong()).clicked() {
                    let mut spec = JobSpec::new(p.name.clone(), cmd.clone());
                    spec.host = hosts.get(p.host).cloned().unwrap_or_else(Host::local);
                    spec.workdir = p.workdir.clone();
                    spec.gpu = p.gpu_request();
                    spec.outputs = p.outputs.clone();
                    spec.auto_load = p.auto_load;
                    spec.env = p
                        .env
                        .lines()
                        .filter_map(|l| l.split_once('='))
                        .map(|(k, v)| (k.trim().to_string(), v.trim().to_string()))
                        .filter(|(k, _)| !k.is_empty())
                        .collect();
                    if p.slurm {
                        spec.scheduler =
                            Scheduler::Slurm { partition: p.partition.clone(), time: p.time.clone(), nodes: p.nodes, tasks: p.np, extra: p.extra.clone() };
                    }
                    let values: Vec<String> = p.sweep_values.split(',').map(|v| v.trim().to_string()).filter(|v| !v.is_empty()).collect();
                    if !p.sweep_var.trim().is_empty() && !values.is_empty() {
                        for s in aurafx_jobs::sweep(&spec, p.sweep_var.trim(), &values) {
                            p.selected = Some(aurafx_jobs::submit(&aurafx_jobs::global(), s));
                        }
                    } else {
                        let id = aurafx_jobs::submit(&aurafx_jobs::global(), spec);
                        p.selected = Some(id);
                    }
                }
                if p.command_override.is_some() && ui.small_button("Reset command").clicked() {
                    p.command_override = None;
                }
                ui.label(RichText::new(TEMPLATES[p.template].description).weak().small());
            });
        });

        // ---------------------------------------------------------- job list
        let jobs = aurafx_jobs::global();
        let rows: Vec<(JobId, String, String, JobState, Option<std::time::Duration>, usize)> = jobs
            .lock()
            .jobs
            .iter()
            .rev()
            .map(|j| (j.id, j.spec.name.clone(), j.spec.host.name.clone(), j.state.clone(), j.runtime(), j.outputs.len()))
            .collect();
        egui::CollapsingHeader::new(RichText::new(format!("{} Jobs ({})", icons::LIST, rows.len())).strong()).default_open(true).show(ui, |ui| {
            if rows.is_empty() {
                ui.label(RichText::new("No jobs yet. Fill in the form above and press Run, or call afx.submit(...) from Python.").weak());
            } else if ui.small_button("Clear finished").clicked() {
                aurafx_jobs::clear_finished(&jobs);
            }
            egui::Grid::new("joblist").striped(true).num_columns(6).spacing([10.0, 4.0]).show(ui, |ui| {
                for (id, name, host, st, rt, nout) in &rows {
                    let sel = state.jobs_panel.selected == Some(*id);
                    if ui.selectable_label(sel, format!("#{id} {name}")).clicked() {
                        state.jobs_panel.selected = Some(*id);
                    }
                    ui.label(RichText::new(host).weak());
                    ui.label(RichText::new(st.label()).color(state_color(st)));
                    ui.label(rt.map(fmt_dur).unwrap_or_default());
                    if st.is_active() {
                        if ui.small_button(format!("{} Stop", icons::X)).clicked() {
                            let _ = aurafx_jobs::cancel(&jobs, *id);
                        }
                    } else {
                        ui.label("");
                    }
                    if *nout > 0 && ui.small_button(format!("{} Open results ({nout})", icons::FOLDER_OPEN)).clicked() {
                        let outs = jobs.lock().jobs.iter().find(|j| j.id == *id).map(|j| j.outputs.clone()).unwrap_or_default();
                        for o in outs.into_iter().take(8) {
                            state.open_file(o);
                        }
                    } else if *nout == 0 {
                        ui.label("");
                    }
                    ui.end_row();
                }
            });
            // Log of the selected job.
            if let Some(sel) = state.jobs_panel.selected {
                let log: Option<Vec<String>> = jobs.lock().jobs.iter().find(|j| j.id == sel).map(|j| j.log.iter().cloned().collect());
                if let Some(log) = log {
                    ui.separator();
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(format!("Log of job #{sel}")).strong());
                        ui.add(egui::TextEdit::singleline(&mut state.jobs_panel.log_filter).hint_text("filter").desired_width(140.0));
                        if ui.small_button(format!("{} Copy", icons::COPY)).clicked() {
                            ui.ctx().copy_text(log.join("\n"));
                        }
                        if ui.small_button(format!("{} Ask AI", icons::SPARKLE)).on_hover_text("Send the end of this log to the assistant").clicked() {
                            let tail: Vec<&String> = log.iter().rev().take(60).collect::<Vec<_>>().into_iter().rev().collect();
                            let msg = format!(
                                "Here is the end of my simulation log. Explain what happened and suggest fixes if it failed:\n```\n{}\n```",
                                tail.iter().map(|s| s.as_str()).collect::<Vec<_>>().join("\n")
                            );
                            let desc = state.session.lock().describe();
                            state.assistant.send(msg, desc);
                        }
                    });
                    let filt = state.jobs_panel.log_filter.to_lowercase();
                    egui::ScrollArea::vertical().id_salt("joblog").max_height(260.0).stick_to_bottom(true).show(ui, |ui| {
                        for l in log.iter().filter(|l| filt.is_empty() || l.to_lowercase().contains(&filt)) {
                            let c = if l.starts_with("! ") {
                                crate::theme::WARN
                            } else if l.starts_with("[auraFX]") {
                                crate::theme::ACCENT_SOFT
                            } else {
                                ui.visuals().text_color()
                            };
                            ui.label(RichText::new(l).monospace().color(c));
                        }
                    });
                }
            }
        });

        // ---------------------------------------------------------- GPU monitor
        egui::CollapsingHeader::new(RichText::new(format!("{} GPUs", icons::CPU)).strong()).default_open(false).show(ui, |ui| {
            let p = &mut state.jobs_panel;
            ui.horizontal(|ui| {
                ui.label("Host");
                let before = p.gpu_host;
                egui::ComboBox::from_id_salt("gpuhost").selected_text(hosts.get(p.gpu_host).map(|h| h.name.clone()).unwrap_or_default()).show_ui(ui, |ui| {
                    for (i, h) in hosts.iter().enumerate() {
                        ui.selectable_value(&mut p.gpu_host, i, &h.name);
                    }
                });
                if before != p.gpu_host {
                    p.gpus.clear();
                    p.gpu_last = -100.0;
                }
                if p.gpu_rx.is_some() {
                    ui.spinner();
                }
            });
            ui.label(RichText::new(format!("Rendering: {}", state.viewport.adapter_name())).weak().small());
            if p.gpus.is_empty() && p.gpu_rx.is_none() {
                ui.label(RichText::new("No NVIDIA (nvidia-smi) or AMD (rocm-smi) GPUs found on this host.").weak());
            }
            for g in &p.gpus {
                ui.horizontal(|ui| {
                    ui.label(RichText::new(format!("{:?} #{}", g.vendor, g.index)).strong());
                    ui.label(&g.name);
                    if let Some(t) = g.temperature_c {
                        ui.label(RichText::new(format!("{t:.0}°C")).weak());
                    }
                });
                ui.add(egui::ProgressBar::new((g.utilization / 100.0) as f32).text(format!("compute {:.0}%", g.utilization)));
                let frac = if g.memory_total_mb > 0.0 { g.memory_used_mb / g.memory_total_mb } else { 0.0 };
                ui.add(egui::ProgressBar::new(frac as f32).text(format!("memory {:.1} / {:.1} GB", g.memory_used_mb / 1024.0, g.memory_total_mb / 1024.0)));
            }
        });

        // ---------------------------------------------------------- hosts
        egui::CollapsingHeader::new(RichText::new(format!("{} Remote machines (SSH)", icons::TERMINAL_WINDOW)).strong()).default_open(false).show(ui, |ui| {
            ui.label(
                RichText::new(
                    "auraFX uses your system ssh, so ~/.ssh/config aliases, keys, agents and ProxyJump work. Use key-based login (no password prompts).",
                )
                .weak()
                .small(),
            );
            let mut remove: Option<usize> = None;
            for (i, h) in state.settings.hosts.iter().enumerate() {
                ui.horizontal(|ui| {
                    ui.label(RichText::new(&h.name).strong());
                    if let HostKind::Ssh { target, .. } = &h.kind {
                        ui.label(RichText::new(target).weak());
                    }
                    if ui.small_button("Test").clicked() {
                        let host = h.clone();
                        let (tx, rx) = std::sync::mpsc::channel();
                        let c = ctx.clone();
                        std::thread::spawn(move || {
                            let r = host.test();
                            let g = if r.is_ok() { aurafx_jobs::gpu::query(&host) } else { vec![] };
                            let _ = tx.send((host.name.clone(), r, g));
                            c.request_repaint();
                        });
                        state.jobs_panel.host_test = Some(rx);
                    }
                    if ui.small_button(icons::TRASH).clicked() {
                        remove = Some(i);
                    }
                });
            }
            if let Some(i) = remove {
                state.settings.hosts.remove(i);
                aurafx_jobs::set_hosts(state.settings.hosts.clone());
                state.settings.save();
            }
            if state.jobs_panel.host_test.is_some() {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label("Connecting…");
                });
            }
            if let Some((name, r, g)) = &state.jobs_panel.host_test_result {
                match r {
                    Ok(s) => {
                        ui.colored_label(crate::theme::OK, format!("{name}: connected"));
                        ui.label(RichText::new(s).monospace().small());
                        for x in g {
                            ui.label(format!("{:?} GPU #{}: {} ({:.1} GB)", x.vendor, x.index, x.name, x.memory_total_mb / 1024.0));
                        }
                    }
                    Err(e) => {
                        ui.colored_label(crate::theme::ERROR, format!("{name}: {e}"));
                    }
                }
            }
            ui.separator();
            let p = &mut state.jobs_panel;
            egui::Grid::new("newhost").num_columns(2).spacing([10.0, 4.0]).show(ui, |ui| {
                ui.label("Name");
                ui.add(egui::TextEdit::singleline(&mut p.new_host_name).hint_text("gpu-box"));
                ui.end_row();
                ui.label("SSH target");
                ui.add(egui::TextEdit::singleline(&mut p.new_host_target).hint_text("user@host or ssh-config alias"));
                ui.end_row();
                ui.label("Port");
                ui.add(egui::TextEdit::singleline(&mut p.new_host_port).hint_text("22"));
                ui.end_row();
                ui.label("Key file");
                ui.add(egui::TextEdit::singleline(&mut p.new_host_identity).hint_text("~/.ssh/id_ed25519 (optional)"));
                ui.end_row();
                ui.label("Jump host");
                ui.add(egui::TextEdit::singleline(&mut p.new_host_jump).hint_text("bastion (optional)"));
                ui.end_row();
                ui.label("Setup lines");
                ui.add(egui::TextEdit::multiline(&mut p.new_host_setup).desired_rows(1).hint_text("module load lammps"));
                ui.end_row();
            });
            if ui.add_enabled(!p.new_host_target.trim().is_empty(), egui::Button::new(format!("{} Add machine", icons::PLUS))).clicked() {
                let name = if p.new_host_name.trim().is_empty() { p.new_host_target.trim().to_string() } else { p.new_host_name.trim().to_string() };
                state.settings.hosts.push(Host {
                    name,
                    kind: HostKind::Ssh {
                        target: p.new_host_target.trim().to_string(),
                        port: p.new_host_port.trim().parse().ok(),
                        identity: Some(p.new_host_identity.trim().to_string()).filter(|s| !s.is_empty()),
                        jump: Some(p.new_host_jump.trim().to_string()).filter(|s| !s.is_empty()),
                    },
                    setup: p.new_host_setup.clone(),
                });
                aurafx_jobs::set_hosts(state.settings.hosts.clone());
                state.settings.save();
                p.new_host_name.clear();
                p.new_host_target.clear();
                p.new_host_port.clear();
                p.new_host_identity.clear();
                p.new_host_jump.clear();
                p.new_host_setup.clear();
            }
            let ssh_aliases = ssh_config_hosts();
            if !ssh_aliases.is_empty() {
                ui.horizontal_wrapped(|ui| {
                    ui.label(RichText::new("From ~/.ssh/config:").weak());
                    for a in ssh_aliases {
                        if !state.settings.hosts.iter().any(|h| h.name == a) && ui.small_button(format!("+ {a}")).clicked() {
                            state.settings.hosts.push(Host {
                                name: a.clone(),
                                kind: HostKind::Ssh { target: a, port: None, identity: None, jump: None },
                                setup: String::new(),
                            });
                            aurafx_jobs::set_hosts(state.settings.hosts.clone());
                            state.settings.save();
                        }
                    }
                });
            }
        });

        // ---------------------------------------------------------- remote files
        egui::CollapsingHeader::new(RichText::new(format!("{} Browse files", icons::FOLDER_OPEN)).strong()).default_open(false).show(ui, |ui| {
            let p = &mut state.jobs_panel;
            ui.horizontal(|ui| {
                egui::ComboBox::from_id_salt("browsehost").selected_text(hosts.get(p.browse_host).map(|h| h.name.clone()).unwrap_or_default()).show_ui(
                    ui,
                    |ui| {
                        for (i, h) in hosts.iter().enumerate() {
                            ui.selectable_value(&mut p.browse_host, i, &h.name);
                        }
                    },
                );
                let r = ui.add(egui::TextEdit::singleline(&mut p.browse_path).desired_width(220.0));
                if ui.button("Go").clicked() || (r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter))) {
                    p.browse(&ctx);
                }
                if ui.button("Up").clicked() {
                    let parent = std::path::Path::new(&p.browse_path).parent().map(|x| x.display().to_string()).unwrap_or_else(|| "/".into());
                    p.browse_path = if parent.is_empty() { "/".into() } else { parent };
                    p.browse(&ctx);
                }
                if p.browse_rx.is_some() || p.fetch_rx.is_some() {
                    ui.spinner();
                }
            });
            if let Some(e) = &p.browse_error {
                ui.colored_label(crate::theme::ERROR, e);
            }
            let exts = aurafx_io::all_extensions();
            let mut go_into: Option<String> = None;
            let mut fetch: Option<String> = None;
            egui::ScrollArea::vertical().id_salt("browse").max_height(240.0).show(ui, |ui| {
                for (name, is_dir, size) in &p.browse_entries {
                    let openable = !is_dir && exts.iter().any(|e| name.to_lowercase().ends_with(&format!(".{e}")));
                    let label = if *is_dir { format!("{} {name}/", icons::FOLDER_OPEN) } else { format!("{name}   {:.1} MB", *size as f64 / 1e6) };
                    let text = if openable {
                        RichText::new(label).strong()
                    } else if *is_dir {
                        RichText::new(label)
                    } else {
                        RichText::new(label).weak()
                    };
                    let r = ui.selectable_label(false, text);
                    if r.double_clicked() || (r.clicked() && *is_dir) {
                        if *is_dir {
                            go_into = Some(name.clone());
                        } else {
                            fetch = Some(name.clone());
                        }
                    }
                    if openable {
                        r.on_hover_text("Double-click to download and open");
                    }
                }
            });
            if let Some(d) = go_into {
                p.browse_path = format!("{}/{}", p.browse_path.trim_end_matches('/'), d);
                p.browse(&ctx);
            }
            if let Some(f) = fetch {
                let host = hosts.get(p.browse_host).cloned().unwrap_or_else(Host::local);
                let remote = format!("{}/{}", p.browse_path.trim_end_matches('/'), f);
                let (tx, rx) = std::sync::mpsc::channel();
                let c = ctx.clone();
                std::thread::spawn(move || {
                    let r = if host.is_local() {
                        Ok(std::path::PathBuf::from(shell_home(&remote)))
                    } else {
                        let dest = aurafx_jobs::cache_dir().join("remote").join(host.name.replace(['/', '@', ':'], "_")).join(&f);
                        host.fetch(&remote, &dest).map(|_| dest)
                    };
                    let _ = tx.send(r);
                    c.request_repaint();
                });
                p.fetch_rx = Some(rx);
            }
        });
    });
    let monitoring = true;
    state.jobs_panel.poll(&ctx, now, monitoring);
}

fn shell_home(p: &str) -> String {
    match p.strip_prefix('~') {
        Some(rest) => format!("{}{}", std::env::var("HOME").unwrap_or_default(), rest),
        None => p.to_string(),
    }
}

fn ssh_config_hosts() -> Vec<String> {
    let Some(home) = std::env::var_os("HOME") else { return vec![] };
    let Ok(text) = std::fs::read_to_string(std::path::Path::new(&home).join(".ssh/config")) else { return vec![] };
    text.lines()
        .filter_map(|l| l.trim().strip_prefix("Host ").map(|s| s.to_string()))
        .flat_map(|s| s.split_whitespace().map(|x| x.to_string()).collect::<Vec<_>>())
        .filter(|h| !h.contains('*') && !h.contains('?'))
        .collect()
}
