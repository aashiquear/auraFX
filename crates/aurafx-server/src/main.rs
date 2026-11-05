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
            let hook = if no_gpu { None } else { aurafx_render::server::ServerRenderer::new() };
            let gpu = hook.as_ref().map(|h| h.gpu());
            let hook = hook.map(|h| Box::new(h) as Box<dyn aurafx_pipeline::remote::RenderHook>);
            // stdout carries the protocol; anything else must go to stderr.
            aurafx_pipeline::remote::serve_with(std::io::stdin().lock(), std::io::stdout().lock(), hook, gpu)
        }
        Some(other) => {
            eprintln!("usage: aurafx-server [--stdio | --version [--gpu]] [--threads N] [--no-gpu]   (unknown argument {other})");
            std::process::exit(2);
        }
    }
}
