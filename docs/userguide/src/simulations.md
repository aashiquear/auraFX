# Running simulations

![The Simulations panel](images/simulations.png)

The **Simulations** panel (right side; **Simulate › Run a simulation…**, or
**Windows › Simulations**) launches solvers and opens their results when they finish.

## A new simulation

| Field | Meaning |
|---|---|
| **Template** | LAMMPS, MOOSE app, OpenFOAM, Python pipeline or Custom command; fills in the program, input and result-file patterns |
| **Run on** | this computer, or a remote machine from the SSH list; **via Slurm** submits with `sbatch` there (partition, time and extra `#SBATCH` lines appear) |
| **Name**, **Working directory** | where the run lives; a remote directory when running over SSH |
| **Program**, **Input**, **MPI processes** | the command line is assembled from these and shown below, editable |
| **GPU** | None, Any, NVIDIA or AMD; the panel finds the GPUs on the host and sets the solver's flags (LAMMPS GPU package, `CUDA_VISIBLE_DEVICES` / `HIP_VISIBLE_DEVICES`) |
| **Environment** | extra `VAR=value` lines (one per line) |
| **Sweep** | a variable name and a list of values: one job per value, `{var}` in the command replaced, each in its own folder |
| **Result files** | patterns of the files to open when the run ends (**open when done**) |

**Run** starts the job. Jobs appear in the **Jobs** list with their state, a live log tail,
and buttons to cancel, open the results or the folder, and resubmit. Files a running solver
writes can be opened while it runs; auraFX reloads them as they change (live reload).

## Remote machines

**Remote machines (SSH)** holds the hosts: a name, the SSH target (`user@machine`, or an
alias from `~/.ssh/config`), port, key file, jump host, and **setup lines** run before every
command (module loads, `source` of an environment, `AURAFX_SERVER=...`). SSH keys or an agent
must be set up so that `ssh host` works without a password prompt. The same hosts serve
**File › Open on Server…** (see [Data on another machine](client-server.md)).

**GPUs** lists the GPUs of each host; **Browse files** opens a remote file browser to pick
inputs or results.

## From Python

```python
jid = afx.submit("lmp -sf gpu -pk gpu 1 -in in.melt", host="rocm-server",
                 workdir="~/runs/melt", gpu="amd:0", outputs="dump.melt", auto_load=True)
afx.sweep("lmp -var T {T} -in in.melt", T=[300, 600, 900], host="cluster", workdir="~/sweep")
afx.jobs(); afx.job(jid); afx.cancel(jid)
afx.gpus("rocm-server"); afx.hosts()
afx.load_remote("rocm-server", "~/runs/melt/dump.melt")   # copy a file here and open it
```

`gpu` is `None`, `"any"`, `"nvidia"`, `"amd"` or e.g. `"nvidia:0,1"`. `afx.job(id)` returns
the state, exit code, log tail and output files.
