//! Crash reports: when auraFX stops on an internal error, what went wrong is written to a file
//! (nothing is sent anywhere), and the next start says so and where the file is, so it can be
//! attached to a bug report. The session is saved as the pipeline changes, so the work can be
//! restored with **Restore last session**.

use std::path::PathBuf;

/// Where reports are kept.
pub fn dir() -> PathBuf {
    aurafx_jobs::cache_dir().join("crashes")
}

fn marker() -> PathBuf {
    dir().join("last")
}

/// Write a report when a thread panics (the default message still goes to the terminal).
pub fn install() {
    let default = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        // Filters and kernels that fail inside a guarded call are reported to the user there;
        // only failures of the window's own thread end the program.
        let thread = std::thread::current();
        let name = thread.name().unwrap_or("unnamed").to_string();
        let msg = info
            .payload()
            .downcast_ref::<String>()
            .cloned()
            .or_else(|| info.payload().downcast_ref::<&str>().map(|s| s.to_string()))
            .unwrap_or_else(|| "unknown".into());
        let at = info.location().map(|l| format!("{}:{}", l.file(), l.line())).unwrap_or_default();
        let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
        let text = format!(
            "auraFX {} crash report\ntime (unix seconds): {secs}\nsystem: {} {}\nthread: {name}\nerror: {msg}\nat: {at}\n\nbacktrace:\n{}\n",
            env!("CARGO_PKG_VERSION"),
            std::env::consts::OS,
            std::env::consts::ARCH,
            std::backtrace::Backtrace::force_capture()
        );
        let d = dir();
        if std::fs::create_dir_all(&d).is_ok() {
            let file = d.join(format!("crash-{secs}.txt"));
            if std::fs::write(&file, text).is_ok() && name == "main" {
                let _ = std::fs::write(marker(), file.display().to_string());
            }
        }
        default(info);
    }));
}

/// The report of a crash of the previous run, once (None when the last run ended normally).
pub fn take_last() -> Option<PathBuf> {
    let m = marker();
    let p = std::fs::read_to_string(&m).ok()?;
    let _ = std::fs::remove_file(&m);
    let p = PathBuf::from(p.trim());
    p.exists().then_some(p)
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_report_names_the_error_and_is_offered_once() {
        // The marker is read once and removed.
        let d = super::dir();
        std::fs::create_dir_all(&d).unwrap();
        let file = d.join("crash-test.txt");
        std::fs::write(&file, "auraFX crash report\nerror: test").unwrap();
        std::fs::write(super::marker(), file.display().to_string()).unwrap();
        assert_eq!(super::take_last().as_deref(), Some(file.as_path()));
        assert_eq!(super::take_last(), None);
        let _ = std::fs::remove_file(file);
    }
}
