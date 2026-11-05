//! `aurafx-server`: runs auraFX readers and filters on this machine for an auraFX app
//! elsewhere. The app starts it over ssh (`aurafx-server --stdio`) and talks to it through
//! stdin/stdout; nothing listens on the network.

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
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
            eprintln!("usage: aurafx-server [--stdio | --version]   (unknown argument {other})");
            std::process::exit(2);
        }
    }
}
