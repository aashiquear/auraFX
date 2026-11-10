//! `aurafx-server`: runs auraFX readers and filters on this machine for an auraFX app
//! elsewhere. The app starts it over ssh (`aurafx-server --stdio`) and talks to it through
//! stdin/stdout; nothing listens on the network.

fn main() -> anyhow::Result<()> {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    // --threads N: cores to use (default all).
    let mut threads = 0;
    if let Some(i) = args.iter().position(|a| a == "--threads") {
        threads = args.get(i + 1).and_then(|v| v.parse().ok()).unwrap_or(0);
        args.drain(i..(i + 2).min(args.len()));
    }
    aurafx_filters::init_threads(threads);
    // --piece k/n: a worker of a parallel server, holding piece k of n.
    let piece = take_value(&mut args, "--piece").and_then(|v| {
        let (k, n) = v.split_once('/')?;
        Some((k.parse::<usize>().ok()?, n.parse::<usize>().ok()?)).filter(|(k, n)| k < n)
    });
    // --workers N | host:N,host:N: coordinate a parallel server over these processes.
    let workers = take_value(&mut args, "--workers");
    // --launcher mpirun|srun|TEMPLATE: start the N workers through MPI or Slurm (they connect back).
    let launcher = take_value(&mut args, "--launcher");
    // --connect HOST:PORT --token T: a worker started by an MPI or Slurm launch (its rank and size
    // from the launcher's environment) that connects back to the coordinator.
    let connect = take_value(&mut args, "--connect");
    let token = take_value(&mut args, "--token").unwrap_or_default();
    let no_gpu = match args.iter().position(|a| a == "--no-gpu") {
        Some(i) => {
            args.remove(i);
            true
        }
        None => false,
    };
    if let Some(addr) = connect {
        return serve_rank(&addr, &token, no_gpu);
    }
    match args.first().map(|s| s.as_str()) {
        Some("--version" | "-V") => {
            println!("aurafx-server {} (protocol {})", env!("CARGO_PKG_VERSION"), aurafx_pipeline::remote::PROTOCOL);
            if args.iter().any(|a| a == "--gpu") {
                match aurafx_render::server::ServerRenderer::new() {
                    Some(r) => println!("GPU: {}", r.gpu()),
                    None => println!("GPU: none (objects are drawn by the app)"),
                }
            }
            Ok(())
        }
        Some("--stdio") | None => {
            // Draw on this machine's GPU when there is one (--no-gpu: never).
            let hook = match (no_gpu, piece) {
                (true, _) => None,
                (false, Some(_)) => aurafx_render::server::ServerRenderer::for_pieces(),
                (false, None) => aurafx_render::server::ServerRenderer::new(),
            };
            let gpu = hook.as_ref().map(|h| h.gpu());
            let hook = hook.map(|h| Box::new(h) as Box<dyn aurafx_pipeline::remote::RenderHook>);
            let workers = match (&workers, &launcher) {
                (Some(spec), Some(l)) => launch_ranks(spec.trim().parse().map_err(|_| anyhow::anyhow!("--launcher takes --workers N"))?, l, no_gpu)?,
                (Some(spec), None) => spawn_workers(spec, no_gpu)?,
                (None, _) => Vec::new(),
            };
            let opts = aurafx_pipeline::remote::ServeOptions { hook, gpu, piece, workers };
            // stdout carries the protocol; anything else must go to stderr.
            aurafx_pipeline::remote::serve_opts(std::io::stdin().lock(), std::io::stdout().lock(), opts)
        }
        Some(other) => {
            eprintln!("usage: aurafx-server [--stdio | --version [--gpu]] [--threads N] [--no-gpu] [--workers N|host:N,...] [--launcher mpirun|srun|TEMPLATE] [--piece k/n] [--connect HOST:PORT --token T]   (unknown argument {other})");
            std::process::exit(2);
        }
    }
}

fn take_value(args: &mut Vec<String>, flag: &str) -> Option<String> {
    let i = args.iter().position(|a| a == flag)?;
    let v = args.get(i + 1).cloned();
    args.drain(i..(i + 2).min(args.len()));
    v
}

/// Start the worker processes of a parallel server: `N` here, or `host:N,host:N` (over ssh;
/// the data must be at the same path there, as on a cluster's shared file system).
fn spawn_workers(spec: &str, no_gpu: bool) -> anyhow::Result<Vec<std::sync::Arc<aurafx_pipeline::remote::RemoteClient>>> {
    let mut places: Vec<(Option<String>, usize)> = Vec::new();
    for part in spec.split(',').map(str::trim).filter(|p| !p.is_empty()) {
        match part.rsplit_once(':') {
            Some((h, n)) => places.push((Some(h.to_string()), n.parse()?)),
            None => places.push((None, part.parse()?)),
        }
    }
    let total: usize = places.iter().map(|p| p.1).sum();
    anyhow::ensure!(total >= 1, "--workers needs at least one process");
    let cores = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1);
    let local_count: usize = places.iter().filter(|p| p.0.is_none()).map(|p| p.1).sum();
    let threads = (cores / local_count.max(1)).max(1);
    let exe = std::env::current_exe()?;
    let mut out = Vec::new();
    let mut k = 0;
    for (host, n) in places {
        for _ in 0..n {
            let piece = format!("{k}/{total}");
            let name = format!("worker {k}");
            let gpu = if no_gpu { " --no-gpu" } else { "" };
            match &host {
                None => {
                    let exe = exe.clone();
                    let no_gpu = no_gpu;
                    out.push(aurafx_pipeline::remote::RemoteClient::from_command(&name, move || {
                        let mut c = std::process::Command::new(&exe);
                        c.args(["--stdio", "--piece", &piece, "--threads", &threads.to_string()]);
                        if no_gpu {
                            c.arg("--no-gpu");
                        }
                        c
                    }));
                }
                Some(h) => {
                    let host = aurafx_jobs::Host {
                        name: h.clone(),
                        kind: aurafx_jobs::HostKind::Ssh { target: h.clone(), port: None, identity: None, jump: None },
                        setup: String::new(),
                        server_processes: 0,
                        launcher: String::new(),
                    };
                    let cmd = aurafx_pipeline::remote::SERVER_COMMAND.replace("--stdio", &format!("--stdio --piece {piece}{gpu}"));
                    out.push(aurafx_pipeline::remote::RemoteClient::from_command(&name, move || host.command(&cmd)));
                }
            }
            k += 1;
        }
    }
    Ok(out)
}

/// The rank and the number of ranks of this process from an MPI or Slurm launch.
fn launch_rank() -> Option<(usize, usize)> {
    let get = |names: &[&str]| names.iter().find_map(|n| std::env::var(n).ok()).and_then(|v| v.trim().parse::<usize>().ok());
    let rank = get(&["OMPI_COMM_WORLD_RANK", "PMI_RANK", "PMIX_RANK", "MV2_COMM_WORLD_RANK", "SLURM_PROCID"])?;
    let size = get(&["OMPI_COMM_WORLD_SIZE", "PMI_SIZE", "MV2_COMM_WORLD_SIZE", "SLURM_NTASKS", "SLURM_NPROCS"])?;
    (rank < size).then_some((rank, size))
}

/// A worker started by a launcher: connect back to the coordinator, say which rank this is, and
/// serve that piece over the connection.
fn serve_rank(addr: &str, token: &str, no_gpu: bool) -> anyhow::Result<()> {
    use std::io::Write;
    let (rank, size) =
        launch_rank().ok_or_else(|| anyhow::anyhow!("--connect needs an MPI or Slurm launch (no OMPI_COMM_WORLD_RANK, PMI_RANK or SLURM_PROCID)"))?;
    let mut stream = std::net::TcpStream::connect(addr)?;
    stream.set_nodelay(true)?;
    writeln!(stream, "AURAFX {token} {rank} {size}")?;
    let hook = if no_gpu { None } else { aurafx_render::server::ServerRenderer::for_pieces() };
    let gpu = hook.as_ref().map(|h| h.gpu());
    let hook = hook.map(|h| Box::new(h) as Box<dyn aurafx_pipeline::remote::RenderHook>);
    let opts = aurafx_pipeline::remote::ServeOptions { hook, gpu, piece: Some((rank, size)), workers: vec![] };
    aurafx_pipeline::remote::serve_opts(stream.try_clone()?, stream, opts)
}

/// The address the ranks reach this machine at: `AURAFX_CONNECT_HOST`, else the host name.
fn reachable_host() -> String {
    if let Ok(h) = std::env::var("AURAFX_CONNECT_HOST") {
        return h;
    }
    std::process::Command::new("hostname")
        .output()
        .ok()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .filter(|h| !h.is_empty())
        .unwrap_or_else(|| "127.0.0.1".into())
}

/// Start `n` workers with an MPI or Slurm launcher (`mpirun`, `srun`, or a template with `{n}` and
/// `{cmd}`): they connect back to a port of this process with a one-time token, and each serves
/// the piece of its rank.
fn launch_ranks(n: usize, launcher: &str, no_gpu: bool) -> anyhow::Result<Vec<std::sync::Arc<aurafx_pipeline::remote::RemoteClient>>> {
    use std::io::BufRead;
    anyhow::ensure!(n >= 1, "--workers needs at least one process");
    let listener = std::net::TcpListener::bind(("0.0.0.0", 0))?;
    let port = listener.local_addr()?.port();
    let token: String = {
        let mut b = [0u8; 16];
        if std::fs::File::open("/dev/urandom").and_then(|mut f| std::io::Read::read_exact(&mut f, &mut b)).is_err() {
            let t = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_nanos() ^ (std::process::id() as u128) << 64;
            b = t.to_le_bytes();
        }
        b.iter().map(|x| format!("{x:02x}")).collect()
    };
    let exe = std::env::current_exe()?;
    let cmd = format!("'{}' --connect {}:{} --token {}{}", exe.display(), reachable_host(), port, token, if no_gpu { " --no-gpu" } else { "" });
    let full = match launcher.trim() {
        "mpirun" | "mpiexec" => format!("{} -np {n} {cmd}", launcher.trim()),
        "srun" => format!("srun -n {n} {cmd}"),
        t if t.contains("{cmd}") => t.replace("{n}", &n.to_string()).replace("{cmd}", &cmd),
        t => format!("{t} {cmd}"),
    };
    eprintln!("aurafx-server: starting {n} workers: {full}");
    let mut child = std::process::Command::new("sh").arg("-c").arg(&full).stdin(std::process::Stdio::null()).stdout(std::process::Stdio::null()).spawn()?;
    // Wait for every rank (60 s), checking that the launch has not failed.
    listener.set_nonblocking(true)?;
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(60);
    let mut ranks: Vec<Option<std::net::TcpStream>> = (0..n).map(|_| None).collect();
    while ranks.iter().any(|r| r.is_none()) {
        match listener.accept() {
            Ok((stream, _)) => {
                stream.set_nonblocking(false)?;
                stream.set_read_timeout(Some(std::time::Duration::from_secs(10)))?;
                let mut line = String::new();
                std::io::BufReader::new(stream.try_clone()?).read_line(&mut line)?;
                let w: Vec<&str> = line.split_whitespace().collect();
                if w.len() == 4 && w[0] == "AURAFX" && w[1] == token && w[3].parse() == Ok(n) {
                    if let Ok(r) = w[2].parse::<usize>() {
                        if r < n {
                            stream.set_read_timeout(None)?;
                            stream.set_nodelay(true)?;
                            ranks[r] = Some(stream);
                        }
                    }
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                if let Ok(Some(st)) = child.try_wait() {
                    anyhow::bail!("the launcher ended ({st}) before all {n} workers connected: {full}");
                }
                if std::time::Instant::now() > deadline {
                    let _ = child.kill();
                    anyhow::bail!("only {} of {n} workers connected within 60 s: {full}", ranks.iter().filter(|r| r.is_some()).count());
                }
                std::thread::sleep(std::time::Duration::from_millis(20));
            }
            Err(e) => return Err(e.into()),
        }
    }
    // The launcher ends when the workers do (they leave when this server closes their connection).
    std::thread::spawn(move || {
        let _ = child.wait();
    });
    let mut out = Vec::new();
    for (r, s) in ranks.into_iter().enumerate() {
        let s = s.unwrap();
        out.push(aurafx_pipeline::remote::RemoteClient::from_streams(&format!("rank {r}"), s.try_clone()?, s));
    }
    Ok(out)
}
