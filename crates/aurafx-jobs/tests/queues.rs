//! PBS and LSF: jobs are submitted, followed and collected through stand-in `qsub`/`qstat` and
//! `bsub`/`bjobs` commands (shell scripts that run the job script and answer like the real
//! ones), since no batch system is installed where the tests run.
#![cfg(unix)]
use aurafx_jobs::{Host, JobSpec, JobState, Scheduler};
use std::os::unix::fs::PermissionsExt;

fn fake(dir: &std::path::Path, name: &str, body: &str) {
    let p = dir.join(name);
    std::fs::write(&p, format!("#!/bin/sh\nD={}\n{body}\n", dir.display())).unwrap();
    std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
}

fn run(spec: JobSpec) -> (JobState, Vec<String>) {
    let jobs = aurafx_jobs::global();
    let id = aurafx_jobs::submit(&jobs, spec);
    for _ in 0..400 {
        let done = jobs.lock().jobs.iter().find(|j| j.id == id).map(|j| matches!(j.state, JobState::Finished(_) | JobState::Failed(_))).unwrap_or(false);
        if done {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    let m = jobs.lock();
    let j = m.jobs.iter().find(|j| j.id == id).unwrap();
    (j.state.clone(), j.log.iter().cloned().collect())
}

#[test]
fn pbs_and_lsf_jobs_run_and_report() {
    let dir = std::env::temp_dir().join(format!("aurafx_queues_{}", std::process::id()));
    let bin = dir.join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    // qsub FILE: run it in the background, print an id; qstat -xf ID: the state and exit status.
    fake(&bin, "qsub", "f=$1; (sh $f > $D/pbs.out 2>&1; echo $? > $D/pbs.code; cat $D/pbs.out >> $(sed -n 's/^#PBS -o //p' $f)) &\necho 41.server");
    fake(
        &bin,
        "qstat",
        "if [ -f $D/pbs.code ]; then echo '    job_state = F'; echo \"    Exit_status = $(cat $D/pbs.code)\"; else echo '    job_state = R'; fi",
    );
    fake(&bin, "qdel", "true");
    // bsub < script; bjobs -noheader -o stat|exit_code ID.
    fake(&bin, "bsub", "cat > $D/lsf.sh; (sh $D/lsf.sh > $D/lsf.out 2>&1; echo $? > $D/lsf.code; cat $D/lsf.out >> $(sed -n 's/^#BSUB -o //p' $D/lsf.sh)) &\necho 'Job <77> is submitted to queue <normal>.'");
    fake(&bin, "bjobs", "if [ \"$3\" = exit_code ]; then cat $D/lsf.code 2>/dev/null; elif [ -f $D/lsf.code ]; then if [ \"$(cat $D/lsf.code)\" = 0 ]; then echo DONE; else echo EXIT; fi; else echo RUN; fi");
    fake(&bin, "bkill", "true");
    std::env::set_var("PATH", format!("{}:{}", bin.display(), std::env::var("PATH").unwrap_or_default()));
    std::env::set_var("AURAFX_QUEUE_POLL_FAST", "1");

    let work = dir.join("work");
    let mut spec = JobSpec::new("pbs run", "echo hello from pbs; echo done > result.txt");
    spec.host = Host::local();
    spec.workdir = work.display().to_string();
    spec.scheduler = Scheduler::batch("pbs", "short", "00:10:00", 2, 8, "-A project").unwrap();
    let script = spec.pbs_script("short", "00:10:00", 2, 8, "-A project");
    assert!(
        script.contains("#PBS -q short")
            && script.contains("#PBS -l walltime=00:10:00")
            && script.contains("#PBS -l select=2:ncpus=4:mpiprocs=4")
            && script.contains("#PBS -A project"),
        "{script}"
    );
    let (state, log) = run(spec);
    assert_eq!(state, JobState::Finished(0), "{log:?}");
    assert!(log.iter().any(|l| l.contains("submitted PBS job 41.server")) && log.iter().any(|l| l.contains("hello from pbs")), "{log:?}");
    assert!(work.join("result.txt").exists());

    // LSF, and a job that fails: its exit code comes back.
    let mut spec = JobSpec::new("lsf run", "echo hello from lsf; exit 3");
    spec.host = Host::local();
    spec.workdir = work.display().to_string();
    spec.scheduler = Scheduler::batch("LSF", "normal", "01:30:00", 1, 4, "").unwrap();
    let script = spec.lsf_script("normal", "01:30:00", 1, 4, "");
    assert!(script.contains("#BSUB -q normal") && script.contains("#BSUB -W 01:30") && script.contains("#BSUB -n 4"), "{script}");
    let (state, log) = run(spec);
    assert_eq!(state, JobState::Finished(3), "{log:?}");
    assert!(log.iter().any(|l| l.contains("submitted LSF job 77")) && log.iter().any(|l| l.contains("hello from lsf")), "{log:?}");
    assert!(Scheduler::batch("nonsense", "", "", 1, 1, "").is_none());
    let _ = std::fs::remove_dir_all(&dir);
}
