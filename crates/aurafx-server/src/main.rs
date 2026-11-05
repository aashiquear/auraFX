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
    let no_gpu = match args.iter().position(|a| a == "--no-gpu") {
        Some(i) => {
            args.remove(i);
            true
        }
        None => false,
    };
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
            let workers = match &workers {
                Some(spec) => spawn_workers(spec, no_gpu)?,
                None => Vec::new(),
            };
            let opts = aurafx_pipeline::remote::ServeOptions { hook, gpu, piece, workers };
            // stdout carries the protocol; anything else must go to stderr.
            aurafx_pipeline::remote::serve_opts(std::io::stdin().lock(), std::io::stdout().lock(), opts)
        }
        Some(other) => {
            eprintln!("usage: aurafx-server [--stdio | --version [--gpu]] [--threads N] [--no-gpu] [--workers N|host:N,...] [--piece k/n]   (unknown argument {other})");
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
