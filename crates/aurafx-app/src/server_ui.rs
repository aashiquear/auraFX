//! "Open on Server": browse a host's files and open them there, so readers and filters run on
//! that machine and only what is drawn comes back.

use crate::app::State;
use aurafx_jobs::Host;
use egui::RichText;
use std::sync::mpsc::Receiver;

type Listing = anyhow::Result<Vec<(String, bool, u64)>>;

pub struct ServerWindow {
    pub open: bool,
    host: usize,
    pub path: String,
    pub entries: Vec<(String, bool, u64)>,
    selected: Option<String>,
    list_rx: Option<Receiver<Listing>>,
    error: Option<String>,
    check_rx: Option<Receiver<anyhow::Result<String>>>,
    status: Option<Result<String, String>>,
    /// Server processes to spread the data over (1: one process).
    processes: u32,
    /// How the processes start: empty (processes of the server), mpirun, srun or a template.
    launcher: String,
    /// Where the data opened here is drawn: this computer, the server (only pictures come back), or auto.
    pub draw_on: aurafx_pipeline::DrawOn,
    /// The "add a machine" form.
    new_name: String,
    new_target: String,
    new_port: String,
    new_key: String,
    new_jump: String,
}

impl Default for ServerWindow {
    fn default() -> Self {
        Self {
            open: false,
            host: 0,
            path: "~".into(),
            entries: vec![],
            selected: None,
            list_rx: None,
            error: None,
            check_rx: None,
            status: None,
            processes: 1,
            launcher: String::new(),
            draw_on: aurafx_pipeline::DrawOn::Auto,
            new_name: String::new(),
            new_target: String::new(),
            new_port: String::new(),
            new_key: String::new(),
            new_jump: String::new(),
        }
    }
}

/// The aurafx-server binary shipped next to the app.
pub fn local_server_binary() -> Option<std::path::PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let name = if cfg!(windows) { "aurafx-server.exe" } else { "aurafx-server" };
    let p = exe.with_file_name(name);
    p.exists().then_some(p)
}

/// Check the server on a host: its version line, or why it cannot start.
fn check(host: &Host) -> anyhow::Result<String> {
    let script = aurafx_pipeline::remote::SERVER_COMMAND.replace("--stdio", "--version --gpu");
    let mut c = host.command(&script);
    if host.is_local() && std::env::var_os("AURAFX_SERVER").is_none() {
        if let Some(p) = local_server_binary() {
            c.env("AURAFX_SERVER", p);
        }
    }
    let out = c.output()?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).trim().replace('\n', " · "))
    } else {
        anyhow::bail!("aurafx-server was not found on {} ({})", host.name, String::from_utf8_lossy(&out.stderr).trim())
    }
}

/// Copy this computer's aurafx-server to ~/.local/bin on a host.
fn install(host: &Host) -> anyhow::Result<String> {
    let local = local_server_binary().ok_or_else(|| anyhow::anyhow!("no aurafx-server next to this app to copy"))?;
    let arch = host.run_capture("uname -sm", std::time::Duration::from_secs(20))?;
    let here = format!("{} {}", std::env::consts::OS, std::env::consts::ARCH);
    let ok = arch.to_lowercase().contains("linux") && arch.contains(std::env::consts::ARCH) && cfg!(target_os = "linux");
    if !ok {
        anyhow::bail!(
            "{} runs {}; this app's server is built for {here}. Build aurafx-server there (cargo build --release -p aurafx-server) or copy a matching one.",
            host.name,
            arch.trim()
        );
    }
    host.run_capture("mkdir -p ~/.local/bin", std::time::Duration::from_secs(20))?;
    host.upload(&local, ".local/bin/aurafx-server")?;
    let v = host
        .run_capture("chmod +x ~/.local/bin/aurafx-server && ~/.local/bin/aurafx-server --version --gpu", std::time::Duration::from_secs(20))
        .map_err(|e| {
            if format!("{e:#}").contains("GLIBC") {
                anyhow::anyhow!("{e:#}\nThis app was built on a newer Linux than {}. Use aurafx-server from the release packages (built on Ubuntu 22.04) or build it there.", host.name)
            } else {
                e
            }
        })?;
    Ok(format!("Installed: {}", v.trim().replace('\n', " · ")))
}

impl ServerWindow {
    fn list(&mut self, host: Host, ctx: &egui::Context) {
        let path = self.path.clone();
        let (tx, rx) = std::sync::mpsc::channel();
        let c = ctx.clone();
        std::thread::spawn(move || {
            let _ = tx.send(host.list_dir(&path));
            c.request_repaint();
        });
        self.list_rx = Some(rx);
        self.selected = None;
    }

    fn run_check(&mut self, host: Host, ctx: &egui::Context, do_install: bool) {
        let (tx, rx) = std::sync::mpsc::channel();
        let c = ctx.clone();
        std::thread::spawn(move || {
            let _ = tx.send(if do_install { install(&host) } else { check(&host) });
            c.request_repaint();
        });
        self.check_rx = Some(rx);
        self.status = None;
    }

    fn poll(&mut self) {
        if let Some(rx) = &self.list_rx {
            if let Ok(r) = rx.try_recv() {
                match r {
                    Ok(mut e) => {
                        e.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.to_lowercase().cmp(&b.0.to_lowercase())));
                        self.entries = e;
                        self.error = None;
                    }
                    Err(e) => self.error = Some(format!("{e:#}")),
                }
                self.list_rx = None;
            }
        }
        if let Some(rx) = &self.check_rx {
            if let Ok(r) = rx.try_recv() {
                self.status = Some(r.map_err(|e| format!("{e:#}")));
                self.check_rx = None;
            }
        }
    }
}

pub fn server_ui(st: &mut State, ctx: &egui::Context) {
    if !st.server.open {
        return;
    }
    st.server.poll();
    let hosts = aurafx_jobs::hosts();
    let mut open = true;
    let mut to_open: Option<(Host, String)> = None;
    let mut add_host: Option<Host> = None;
    egui::Window::new(format!("{} Remote: open data on another machine", crate::icons::CLOUD)).open(&mut open).default_width(560.0).show(ctx, |ui| {
        ui.label(
            RichText::new(
                "aurafx-server reads and filters the file on that machine (over SSH); this window shows the result. \
                 The data never has to be copied here.",
            )
            .weak(),
        );
        // ------------------------------------------------------------ 1. machine
        ui.add_space(4.0);
        ui.label(RichText::new("1  The machine").strong());
        let w = &mut st.server;
        if w.host >= hosts.len() {
            w.host = 0;
        }
        let mut host = hosts.get(w.host).cloned().unwrap_or_else(Host::local);
        host.server_processes = w.processes.max(1);
        host.launcher = w.launcher.clone();
        ui.horizontal(|ui| {
            ui.label("Host");
            let before = w.host;
            egui::ComboBox::from_id_salt("server_host").selected_text(host.name.clone()).show_ui(ui, |ui| {
                for (i, h) in hosts.iter().enumerate() {
                    ui.selectable_value(&mut w.host, i, &h.name);
                }
            });
            if w.host != before {
                w.entries.clear();
                w.status = None;
            }
            if hosts.len() <= 1 {
                ui.label(RichText::new("only this computer so far: add a machine below").weak().small());
            }
        });
        egui::CollapsingHeader::new(format!("{} Add a machine", crate::icons::PLUS)).default_open(hosts.len() <= 1).show(ui, |ui| {
            egui::Grid::new("server new host").num_columns(2).spacing([10.0, 4.0]).show(ui, |ui| {
                ui.label("SSH target");
                ui.add(egui::TextEdit::singleline(&mut w.new_target).hint_text("user@machine, or an alias from ~/.ssh/config"));
                ui.end_row();
                ui.label("Name");
                ui.add(egui::TextEdit::singleline(&mut w.new_name).hint_text("gpu-box (optional)"));
                ui.end_row();
                ui.label("Port");
                ui.add(egui::TextEdit::singleline(&mut w.new_port).hint_text("22"));
                ui.end_row();
                ui.label("Key file");
                ui.add(egui::TextEdit::singleline(&mut w.new_key).hint_text("~/.ssh/id_ed25519 (optional)"));
                ui.end_row();
                ui.label("Jump host");
                ui.add(egui::TextEdit::singleline(&mut w.new_jump).hint_text("bastion (optional)"));
                ui.end_row();
            });
            ui.horizontal_wrapped(|ui| {
                if ui.add_enabled(!w.new_target.trim().is_empty(), egui::Button::new(format!("{} Add", crate::icons::PLUS))).clicked() {
                    let target = w.new_target.trim().to_string();
                    add_host = Some(Host {
                        name: if w.new_name.trim().is_empty() { target.clone() } else { w.new_name.trim().to_string() },
                        kind: aurafx_jobs::HostKind::Ssh {
                            target,
                            port: w.new_port.trim().parse().ok(),
                            identity: Some(w.new_key.trim().to_string()).filter(|s| !s.is_empty()),
                            jump: Some(w.new_jump.trim().to_string()).filter(|s| !s.is_empty()),
                        },
                        setup: String::new(),
                        server_processes: 0,
                        launcher: String::new(),
                    });
                }
                let aliases = crate::jobs_panel::ssh_config_hosts();
                if !aliases.is_empty() {
                    ui.label(RichText::new("from ~/.ssh/config:").weak().small());
                    for a in aliases.into_iter().filter(|a| !hosts.iter().any(|h| &h.name == a)).take(8) {
                        if ui.small_button(format!("+ {a}")).clicked() {
                            add_host = Some(Host { name: a.clone(), kind: aurafx_jobs::HostKind::Ssh { target: a, port: None, identity: None, jump: None }, setup: String::new(), server_processes: 0, launcher: String::new() });
                        }
                    }
                }
            });
            ui.label(RichText::new("The machine needs SSH access with a key (no password prompt). Machines added here are also the Simulations panel's.").weak().small());
        });
        // ------------------------------------------------------------ 2. server
        ui.add_space(4.0);
        ui.label(RichText::new("2  aurafx-server on it").strong());
        ui.horizontal(|ui| {
            if ui.button("Check server").on_hover_text("Is aurafx-server installed there?").clicked() {
                let h = hosts.get(w.host).cloned().unwrap_or_else(Host::local);
                w.run_check(h, ctx, false);
            }
            if !host.is_local() && ui.button("Install server").on_hover_text("Copy this computer's aurafx-server to ~/.local/bin on the host").clicked() {
                w.run_check(host.clone(), ctx, true);
            }
            if w.check_rx.is_some() {
                ui.spinner();
            }
        });
        match &w.status {
            Some(Ok(v)) => {
                ui.colored_label(crate::theme::OK, v);
            }
            Some(Err(e)) => {
                ui.colored_label(crate::theme::ERROR, e);
            }
            None => {}
        }
        // ------------------------------------------------------------ 3. display
        ui.add_space(4.0);
        ui.label(RichText::new("3  Where the picture is drawn").strong());
        {
            use aurafx_pipeline::DrawOn;
            ui.radio_value(&mut w.draw_on, DrawOn::Auto, "Auto: the geometry comes here, unless even its surface is too big to send");
            ui.radio_value(&mut w.draw_on, DrawOn::Server, "On the server: it renders with its GPU and only pictures come here (no picking)");
            ui.radio_value(&mut w.draw_on, DrawOn::Local, "On this computer: the geometry always comes here (picking and selection work)");
            ui.label(RichText::new("You can change it for each object later: Properties › Display › Draw on.").weak().small());
        }
        // ------------------------------------------------------------ 4. files
        ui.add_space(4.0);
        ui.label(RichText::new("4  Open a file").strong());
        if let Some(c) = st.session.lock().remote_clients.get(&aurafx_pipeline::remote::client_key(&host)) {
            if let Some(i) = c.info.lock().as_ref() {
                let procs = if i.processes > 1 { format!(", {} processes", i.processes) } else { String::new() };
                let gpu = i.gpu.as_ref().map(|g| format!(", draws with {g}")).unwrap_or_default();
                ui.label(RichText::new(format!("Connected: {} ({} CPUs{procs}{gpu}), server {}", i.hostname, i.cpus, i.version)).small().weak());
            }
        }
        ui.horizontal(|ui| {
            ui.label("Processes");
            ui.add(egui::DragValue::new(&mut st.server.processes).range(1..=1024))
                .on_hover_text("Spread each file over this many server processes (a parallel server). Partitioned files (.pvtu, ...) are read piece by piece; other files are read, then split.");
            if st.server.processes > 1 {
                ui.label("started by");
                let shown = match st.server.launcher.as_str() {
                    "" => "the server".to_string(),
                    "stream" => "one, in turn".to_string(),
                    l => l.to_string(),
                };
                egui::ComboBox::from_id_salt("server_launcher").selected_text(shown).show_ui(ui, |ui| {
                    ui.selectable_value(&mut st.server.launcher, String::new(), "the server (its own processes)");
                    ui.selectable_value(&mut st.server.launcher, "mpirun".to_string(), "mpirun (MPI)");
                    ui.selectable_value(&mut st.server.launcher, "srun".to_string(), "srun (Slurm)");
                    ui.selectable_value(&mut st.server.launcher, "stream".to_string(), "one process, one piece at a time")
                        .on_hover_text("For files larger than memory: one process reads the pieces one after another and keeps only what comes out of each (a surface, a slice, a contour). Slower, but needs memory for one piece only.");
                    if ui.selectable_label(st.server.launcher.contains("{cmd}"), "a command…").clicked() {
                        st.server.launcher = "mpiexec -n {n} -hostfile hosts {cmd}".into();
                    }
                });
                if st.server.launcher.contains("{cmd}") {
                    ui.add(egui::TextEdit::singleline(&mut st.server.launcher).desired_width(220.0))
                        .on_hover_text("{n}: the number of processes, {cmd}: the worker command");
                }
            }
        });
        ui.separator();
        let w = &mut st.server;
        let mut go = false;
        ui.horizontal(|ui| {
            let r = ui.add(egui::TextEdit::singleline(&mut w.path).desired_width(330.0).hint_text("folder or file on the host"));
            go |= ui.button("Go").clicked() || (r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)));
            if ui.button("Up").clicked() {
                let p = w.path.trim_end_matches('/');
                w.path = match p.rfind('/') {
                    Some(0) => "/".into(),
                    Some(k) => p[..k].to_string(),
                    None => "~".into(),
                };
                go = true;
            }
            if w.list_rx.is_some() {
                ui.spinner();
            }
        });
        if go || (w.entries.is_empty() && w.list_rx.is_none() && w.error.is_none()) {
            w.list(host.clone(), ctx);
        }
        if let Some(e) = &w.error {
            ui.colored_label(crate::theme::ERROR, e);
        }
        let exts = aurafx_io::all_extensions();
        let mut into: Option<String> = None;
        egui::ScrollArea::vertical().max_height(300.0).auto_shrink([false, true]).show(ui, |ui| {
            for (name, is_dir, size) in &w.entries {
                let readable = !is_dir && exts.iter().any(|e| name.to_lowercase().ends_with(&format!(".{e}")));
                let label = if *is_dir { format!("{} {name}/", crate::icons::FOLDER_OPEN) } else { format!("{name}   {}", human(*size)) };
                let text = if readable || *is_dir { RichText::new(label) } else { RichText::new(label).weak() };
                let r = ui.selectable_label(w.selected.as_deref() == Some(name), text);
                if r.clicked() {
                    if *is_dir {
                        into = Some(name.clone());
                    } else {
                        w.selected = Some(name.clone());
                    }
                }
                if r.double_clicked() && readable {
                    to_open = Some((host.clone(), join(&w.path, name)));
                }
            }
        });
        if let Some(d) = into {
            w.path = join(&w.path, &d);
            w.list(host.clone(), ctx);
        }
        ui.separator();
        ui.horizontal(|ui| {
            let target = w.selected.as_ref().map(|f| join(&w.path, f));
            if ui.add_enabled(target.is_some(), egui::Button::new(format!("{} Open on {}", crate::icons::CLOUD, host.name))).clicked() {
                to_open = target.map(|t| (host.clone(), t));
            }
            ui.label(RichText::new("Data larger than 64 MB comes back as its surface; \"Bring all data\" in Properties gets the rest.").small().weak());
        });
    });
    st.server.open = open;
    if let Some(h) = add_host {
        let name = h.name.clone();
        st.settings.hosts.retain(|x| x.name != name);
        st.settings.hosts.push(h);
        aurafx_jobs::set_hosts(st.settings.hosts.clone());
        st.settings.save();
        st.server.host = aurafx_jobs::hosts().iter().position(|x| x.name == name).unwrap_or(0);
        st.server.entries.clear();
        st.server.status = None;
        st.server.new_target.clear();
        st.server.new_name.clear();
        st.server.new_port.clear();
        st.server.new_key.clear();
        st.server.new_jump.clear();
    }
    if let Some((host, path)) = to_open {
        let draw_on = st.server.draw_on;
        let mut s = st.session.lock();
        let id = s.open_remote(host, &path);
        if let Some(n) = s.nodes.get_mut(&id) {
            n.display.draw_on = draw_on;
        }
        drop(s);
        st.server.open = false;
    }
}

fn join(dir: &str, name: &str) -> String {
    format!("{}/{}", dir.trim_end_matches('/'), name)
}

pub fn human(b: u64) -> String {
    let b = b as f64;
    if b >= 1e9 {
        format!("{:.1} GB", b / 1e9)
    } else if b >= 1e6 {
        format!("{:.1} MB", b / 1e6)
    } else if b >= 1e3 {
        format!("{:.0} kB", b / 1e3)
    } else {
        format!("{b:.0} B")
    }
}

impl ServerWindow {
    /// The process count and how they start (for scripts and the guide's pictures).
    pub fn set_processes(&mut self, n: u32, launcher: &str) {
        self.processes = n.max(1);
        self.launcher = launcher.to_string();
    }
}
