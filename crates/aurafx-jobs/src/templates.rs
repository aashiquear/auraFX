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
    /// Templates that write their input files from the selected pipeline data: the case kind
    /// (moose, spparks, pinnacle).
    pub case: Option<&'static str>,
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
        case: None,
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
        case: None,
    },
    Template {
        id: "moose_polycrystal",
        label: "MOOSE polycrystal",
        description: "Crystal plasticity of a grain mesh (Assign Grains): writes the mesh with one block per grain, the grain orientations, the slip systems and the input file from the selected data, then runs a MOOSE application with the solid mechanics module.",
        command: "mpiexec -n {np} {exe} -i {input}",
        gpu_command: "mpiexec -n {np} {exe} -i {input}",
        default_exe: "solid_mechanics-opt",
        default_input: "polycrystal.i",
        outputs: "polycrystal_out.e",
        case: Some("moose"),
    },
    Template {
        id: "spparks",
        label: "SPPARKS grain growth",
        description: "Potts-model grain growth of a microstructure volume: writes the site file and the input from the selected data; the snapshots (dump.potts) open as volumes.",
        command: "mpirun -np {np} {exe} -in {input}",
        gpu_command: "mpirun -np {np} {exe} -in {input}",
        default_exe: "spk_mpi",
        default_input: "in.potts",
        outputs: "dump.potts",
        case: Some("spparks"),
    },
    Template {
        id: "pinnacle",
        label: "PINNACLE crystal plasticity",
        description: "Dislocation-density crystal plasticity of a grain mesh: writes the Exodus mesh, the orientation point file and the input deck from the selected data; the field snapshots open as a time series.",
        command: "{exe} run {input}",
        gpu_command: "{exe} run {input}",
        default_exe: "pinnacle",
        default_input: "case.i",
        outputs: "solver/fields.pvd",
        case: Some("pinnacle"),
    },
    Template {
        id: "damask",
        label: "DAMASK crystal plasticity (grid)",
        description: "Spectral crystal plasticity of a microstructure volume: writes grid.vti (one material per grain), material.yaml with the grains' orientations and load.yaml (tension along an axis) from the selected data.",
        command: "{exe} --load load.yaml --geom grid.vti --material material.yaml",
        gpu_command: "{exe} --load load.yaml --geom grid.vti --material material.yaml",
        default_exe: "DAMASK_grid",
        default_input: "load.yaml",
        outputs: "grid_load.hdf5",
        case: Some("damask"),
    },
    Template {
        id: "fft",
        label: "FFT texture file",
        description: "The microstructure volume as an FFT code's texture file (micro.txt: Bunge angles, voxel indices, grain and phase per voxel); give it to the code's own input.",
        command: "{exe}",
        gpu_command: "{exe}",
        default_exe: "evpfft",
        default_input: "micro.txt",
        outputs: "",
        case: Some("fft"),
    },
    Template {
        id: "lammps_polycrystal",
        label: "LAMMPS polycrystal",
        description: "Fills every grain of a microstructure volume with the material's lattice at the grain's orientation, removes overlapping atoms at the boundaries, and writes atoms.data, atom_grains.txt and an input that relaxes and heats it.",
        command: "mpirun -np {np} {exe} -in {input}",
        gpu_command: "mpirun -np {np} {exe} -sf gpu -pk gpu {ngpu} -in {input}",
        default_exe: "lmp",
        default_input: "in.lammps",
        outputs: "dump.lammpstrj",
        case: Some("lammps"),
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
        case: None,
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
        case: None,
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
        case: None,
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
