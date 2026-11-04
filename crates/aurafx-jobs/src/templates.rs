//! Ready-made job templates for common simulation codes.

use crate::GpuRequest;

pub struct Template {
    pub id: &'static str,
    pub label: &'static str,
    pub description: &'static str,
    /// `{input}`, `{np}`, `{ngpu}`, `{exe}` are substituted.
    pub command: &'static str,
    pub gpu_command: &'static str,
    pub default_exe: &'static str,
    pub default_input: &'static str,
    pub outputs: &'static str,
}

pub const TEMPLATES: &[Template] = &[
    Template {
        id: "lammps",
        label: "LAMMPS",
        description: "Molecular dynamics. GPU mode uses the GPU package (works with CUDA, HIP/ROCm and OpenCL builds).",
        command: "mpirun -np {np} {exe} -in {input}",
        gpu_command: "mpirun -np {np} {exe} -sf gpu -pk gpu {ngpu} -in {input}",
        default_exe: "lmp",
        default_input: "in.lammps",
        outputs: "*.dump *.lammpstrj dump.*",
    },
    Template {
        id: "moose",
        label: "MOOSE app",
        description: "Finite-element multiphysics (Exodus output).",
        command: "mpiexec -n {np} {exe} -i {input}",
        gpu_command: "mpiexec -n {np} {exe} -i {input}",
        default_exe: "./myapp-opt",
        default_input: "input.i",
        outputs: "*_out.e *.e",
    },
    Template {
        id: "openfoam",
        label: "OpenFOAM",
        description: "CFD case: runs the solver then writes VTK for viewing.",
        command: "{exe} > log.solver 2>&1; tail -n 40 log.solver; foamToVTK -latestTime",
        gpu_command: "{exe} > log.solver 2>&1; tail -n 40 log.solver; foamToVTK -latestTime",
        default_exe: "foamRun",
        default_input: "",
        outputs: "VTK/*.vtm VTK/*.vtk",
    },
    Template {
        id: "python",
        label: "Python pipeline",
        description: "Run a Python script (pre-processing, a simulation, post-processing).",
        command: "{exe} -u {input}",
        gpu_command: "{exe} -u {input}",
        default_exe: "python3",
        default_input: "pipeline.py",
        outputs: "*.vtu *.vtk *.csv",
    },
    Template {
        id: "shell",
        label: "Custom command",
        description: "Any shell command.",
        command: "{exe} {input}",
        gpu_command: "{exe} {input}",
        default_exe: "./run.sh",
        default_input: "",
        outputs: "",
    },
];

pub fn render(t: &Template, exe: &str, input: &str, np: u32, gpu: &GpuRequest) -> String {
    let src = if gpu.count() > 0 { t.gpu_command } else { t.command };
    let mut cmd = src.replace("{exe}", exe).replace("{input}", input).replace("{np}", &np.max(1).to_string()).replace("{ngpu}", &gpu.count().to_string());
    if np <= 1 {
        // Avoid requiring MPI for single-process runs.
        for launcher in ["mpirun -np 1 ", "mpiexec -n 1 "] {
            cmd = cmd.replace(launcher, "");
        }
    }
    cmd.trim().to_string()
}
