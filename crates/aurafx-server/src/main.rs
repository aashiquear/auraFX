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
    match args.first().map(|s| s.as_str()) {
        Some("--version" | "-V") => {
            println!("aurafx-server {} (protocol {})", env!("CARGO_PKG_VERSION"), aurafx_pipeline::remote::PROTOCOL);
            Ok(())
        }
        Some("--stdio") | None => {
            // stdout carries the protocol; anything else must go to stderr.
            aurafx_pipeline::remote::serve(std::io::stdin().lock(), std::io::stdout().lock())
        }
        Some(other) => {
            eprintln!("usage: aurafx-server [--stdio | --version] [--threads N]   (unknown argument {other})");
            std::process::exit(2);
        }
    }
}
