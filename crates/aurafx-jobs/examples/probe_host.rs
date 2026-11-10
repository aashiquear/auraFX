//! Probe a host: `cargo run -p aurafx-jobs --example probe_host -- <ssh-target>` (no arg = local).
use aurafx_jobs::*;

fn main() {
    let host = match std::env::args().nth(1) {
        Some(t) => Host {
            name: t.clone(),
            kind: HostKind::Ssh { target: t, port: None, identity: None, jump: None },
            setup: String::new(),
            server_processes: 0,
            launcher: String::new(),
        },
        None => Host::local(),
    };
    match host.test() {
        Ok(s) => println!("connected:\n{s}"),
        Err(e) => {
            println!("connection failed: {e:#}");
            return;
        }
    }
    for g in gpu::query(&host) {
        println!(
            "GPU {:?} #{} {} {:.0}/{:.0} MB util {}% temp {:?}",
            g.vendor, g.index, g.name, g.memory_used_mb, g.memory_total_mb, g.utilization, g.temperature_c
        );
    }
}
