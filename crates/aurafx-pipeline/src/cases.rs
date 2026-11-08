//! Simulation cases from pipeline data: the files a solver needs, written into one folder,
//! and the command that runs them.
//!
//! - MOOSE polycrystal: a grain mesh (Assign Grains, Overlay Mesh of a microstructure) as an
//!   Exodus file with one block per grain, the grains' orientations (`grain_euler.txt`, one row
//!   per block, read by `PropertyReadFile`; `grains.tex` for `EulerAngleFileReader`), the slip
//!   systems and a crystal-plasticity input file pulling the mesh along an axis.
//! - SPPARKS grain growth: a microstructure volume as a site file and a Potts-model input.
//! - PINNACLE crystal plasticity: the grain mesh as an Exodus file, an orientation point file
//!   (element centres with their grain and Euler angles) and an input deck.

use anyhow::{anyhow, bail, Result};
use aurafx_data::*;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

pub const SOLVERS: [&str; 3] = ["moose", "spparks", "pinnacle"];

#[derive(Debug, Clone)]
pub struct CaseOptions {
    /// A material of the crystal mechanics table (elastic constants, shear modulus, Burgers vector).
    pub material: String,
    /// Loading axis 0, 1, 2 (x, y, z).
    pub axis: usize,
    /// Applied strain at the end of the run.
    pub strain: f64,
    pub steps: usize,
    pub end_time: f64,
    /// Side or node set held along the axis and the one pulled (empty: found by name, `x-`/`x+`).
    pub fixed: String,
    pub loaded: String,
    /// SPPARKS: Monte Carlo steps, temperature (kT), dump interval, random seed.
    pub mc_steps: f64,
    pub temperature: f64,
    pub dump_every: f64,
    pub seed: u64,
}

impl Default for CaseOptions {
    fn default() -> Self {
        Self {
            material: "Aluminium".into(),
            axis: 0,
            strain: 0.02,
            steps: 50,
            end_time: 1.0,
            fixed: String::new(),
            loaded: String::new(),
            mc_steps: 100.0,
            temperature: 0.0,
            dump_every: 10.0,
            seed: 12345,
        }
    }
}

/// A written case.
#[derive(Debug, Clone)]
pub struct Case {
    pub folder: PathBuf,
    pub files: Vec<PathBuf>,
    /// The job template (crate aurafx-jobs) and the input file it runs.
    pub template: &'static str,
    pub input: String,
    pub command: String,
    pub outputs: String,
}

fn material(name: &str) -> Result<&'static aurafx_crystal::mechanics::Material> {
    aurafx_crystal::mechanics::MATERIALS
        .iter()
        .find(|m| m.name.eq_ignore_ascii_case(name) || m.name.to_ascii_lowercase().starts_with(&name.to_ascii_lowercase()))
        .ok_or_else(|| {
            anyhow!("unknown material {name}; one of: {}", aurafx_crystal::mechanics::MATERIALS.iter().map(|m| m.name).collect::<Vec<_>>().join(", "))
        })
}

/// The grain mesh: grain ids per element, one block per grain, set names without spaces.
fn grain_mesh(ds: &DataSet) -> Result<(UnstructuredGrid, Vec<i32>)> {
    let mut ug = ds
        .to_unstructured()
        .filter(|u| !u.cells.is_empty())
        .ok_or_else(|| anyhow!("a solver case needs a mesh: run Assign Grains (or Overlay Mesh of a microstructure) first"))?;
    let g =
        ug.cell_data.get("grain_id").or_else(|| ug.cell_data.get("FeatureIds")).ok_or_else(|| anyhow!("the mesh has no grain_id: run Assign Grains first"))?;
    let gid: Vec<i32> = (0..g.len()).map(|i| g.get(i, 0) as i32).collect();
    if gid.iter().any(|g| *g <= 0) {
        bail!("some elements have grain id 0 (no grain): every element needs a grain for a polycrystal case");
    }
    let mut ids: Vec<i32> = gid.clone();
    ids.sort_unstable();
    ids.dedup();
    ug.cell_data.insert(Array::new("ElementBlock", 1, ArrayData::I32(gid.clone())));
    ug.field_data.insert(Array::new("ElementBlockIds", 1, ArrayData::I32(ids.clone())));
    ug.field_data.insert(Array::new("ElementBlockNames", 1, ArrayData::String(ids.iter().map(|g| format!("grain_{g}")).collect())));
    // Set names as solvers' input files can name them (no spaces).
    let renamed: Vec<Array> = ug
        .field_data
        .iter()
        .filter(|a| a.name.starts_with("SideSet ") || a.name.starts_with("NodeSet "))
        .map(|a| {
            let (kind, name) = a.name.split_at(8);
            let mut b = a.clone();
            b.name = format!("{kind}{}", name.trim().replace(' ', "_"));
            b
        })
        .collect();
    let old: Vec<String> = ug.field_data.iter().filter(|a| a.name.starts_with("SideSet ") || a.name.starts_with("NodeSet ")).map(|a| a.name.clone()).collect();
    for n in old {
        ug.field_data.remove(&n);
    }
    for a in renamed {
        ug.field_data.insert(a);
    }
    Ok((ug, gid))
}

fn set_names(ug: &UnstructuredGrid, kind: &str) -> Vec<String> {
    ug.field_data.iter().filter_map(|a| a.name.strip_prefix(kind).map(|n| n.trim().to_string())).collect()
}

/// The set named `want`, or the one whose name ends with the axis face (`x-`, `x+`).
fn pick_set(names: &[String], want: &str, face: &str) -> Option<String> {
    if !want.is_empty() {
        // Set names lose their spaces on the way to the solver's files: compare without them.
        let plain = |s: &str| s.replace('_', " ");
        return names.iter().find(|n| n.as_str() == want || plain(n) == plain(want)).cloned();
    }
    names.iter().find(|n| n.as_str() == face).or_else(|| names.iter().find(|n| n.ends_with(&format!("_{face}")))).cloned()
}

fn axis_name(a: usize) -> &'static str {
    ["x", "y", "z"][a.min(2)]
}

/// Slip systems as Miller indices (plane normal, then direction): FCC {111}<110>, BCC {110}<111>.
fn slip_file(m: &aurafx_crystal::mechanics::Material) -> Result<String> {
    use aurafx_crystal::mechanics::Lattice;
    let (planes, dirs): (Vec<[i32; 3]>, Vec<[i32; 3]>) = {
        let p111 = vec![[1, 1, 1], [-1, 1, 1], [1, -1, 1], [1, 1, -1]];
        let d110 = vec![[0, 1, -1], [1, 0, -1], [1, -1, 0], [0, 1, 1], [1, 0, 1], [1, 1, 0]];
        match m.lattice {
            Lattice::Fcc => (p111, d110),
            Lattice::Bcc => (d110, p111),
            _ => bail!("{}: the crystal-plasticity case covers cubic (FCC, BCC) materials", m.name),
        }
    };
    let mut s = String::new();
    for p in &planes {
        for d in &dirs {
            if p[0] * d[0] + p[1] * d[1] + p[2] * d[2] == 0 {
                writeln!(s, "{} {} {} {} {} {}", p[0], p[1], p[2], d[0], d[1], d[2])?;
            }
        }
    }
    Ok(s)
}

fn write(folder: &Path, name: &str, text: &str, files: &mut Vec<PathBuf>) -> Result<()> {
    let p = folder.join(name);
    std::fs::write(&p, text)?;
    files.push(p);
    Ok(())
}

/// Write the case for `solver` ("moose", "spparks" or "pinnacle") from `ds` into `folder`.
pub fn write_case(ds: &DataSet, solver: &str, folder: &Path, o: &CaseOptions) -> Result<Case> {
    std::fs::create_dir_all(folder)?;
    let mut files = Vec::new();
    match solver.to_ascii_lowercase().as_str() {
        "moose" => {
            let m = material(&o.material)?;
            let (ug, _) = grain_mesh(ds)?;
            let mesh = DataSet::UnstructuredGrid(ug.clone());
            let rows = aurafx_io::writers::grain_euler_rows(&mesh)?;
            let p = folder.join("mesh.e");
            aurafx_io::writers::write_file(&mesh, &p)?;
            files.push(p);
            let p = folder.join("grains.tex");
            aurafx_io::writers::write_file(&mesh, &p)?;
            files.push(p);
            let mut t = String::new();
            for e in &rows {
                writeln!(t, "{:.6} {:.6} {:.6}", e[0], e[1], e[2])?;
            }
            write(folder, "grain_euler.txt", &t, &mut files)?;
            write(folder, "slip_systems.txt", &slip_file(m)?, &mut files)?;
            let sides = set_names(&ug, "SideSet ");
            let ax = axis_name(o.axis);
            let fixed = pick_set(&sides, &o.fixed, &format!("{ax}-")).ok_or_else(|| anyhow!("no side set to hold (named {ax}-); give fixed="))?;
            let loaded = pick_set(&sides, &o.loaded, &format!("{ax}+")).ok_or_else(|| anyhow!("no side set to pull (named {ax}+); give loaded="))?;
            let b = ds.bounds();
            let length = b.size()[o.axis.min(2)];
            let c = m.elastic.iter().map(|x| x * 1000.0).collect::<Vec<_>>();
            let disp = ["disp_x", "disp_y", "disp_z"];
            let mut bcs = format!(
                "  [hold]\n    type = DirichletBC\n    variable = {}\n    boundary = '{fixed}'\n    value = 0\n  []\n  [pull]\n    type = FunctionDirichletBC\n    variable = {}\n    boundary = '{loaded}'\n    function = pull\n  []\n",
                disp[o.axis], disp[o.axis]
            );
            // The other two directions held on their lower faces, when the mesh names them.
            for a in (0..3).filter(|a| *a != o.axis) {
                if let Some(s) = pick_set(&sides, "", &format!("{}-", axis_name(a))) {
                    write!(bcs, "  [hold_{}]\n    type = DirichletBC\n    variable = {}\n    boundary = '{s}'\n    value = 0\n  []\n", axis_name(a), disp[a])?;
                }
            }
            let comp = format!("{ax}{ax}");
            let deck = format!(
                "# Polycrystal crystal plasticity written by auraFX: {n} grains, {ne} elements, {mat}.\n\
# The mesh has one block per grain (block id = grain id); grain_euler.txt holds each block's\n\
# Bunge Euler angles in degrees (row k: block k + 1). The {fixed} faces are held along {ax}, the\n\
# {loaded} faces pulled to a strain of {strain} at t = {end}.\n\n\
[GlobalParams]\n  displacements = 'disp_x disp_y disp_z'\n[]\n\n\
[Mesh]\n  [file]\n    type = FileMeshGenerator\n    file = mesh.e\n  []\n[]\n\n\
[Physics/SolidMechanics/QuasiStatic]\n  [all]\n    strain = FINITE\n    incremental = true\n    add_variables = true\n    generate_output = 'stress_{comp} vonmises_stress'\n  []\n[]\n\n\
[UserObjects]\n  [orientations]\n    type = PropertyReadFile\n    prop_file_name = grain_euler.txt\n    nprop = 3\n    read_type = block\n    nblock = {n}\n    use_zero_based_block_indexing = false\n  []\n[]\n\n\
[Functions]\n  [pull]\n    type = ParsedFunction\n    expression = '{rate} * t'\n  []\n[]\n\n\
[BCs]\n{bcs}[]\n\n\
[Materials]\n  [elasticity_tensor]\n    type = ComputeElasticityTensorCP\n    C_ijkl = '{c11} {c12} {c12} {c11} {c12} {c11} {c44} {c44} {c44}'\n    fill_method = symmetric9\n    read_prop_user_object = orientations\n  []\n  [stress]\n    type = ComputeMultipleCrystalPlasticityStress\n    crystal_plasticity_models = 'slip'\n    tan_mod_type = exact\n  []\n  [slip]\n    # Hardening parameters: the model's defaults; set them for the material.\n    type = CrystalPlasticityKalidindiUpdate\n    number_slip_systems = 12\n    slip_sys_file_name = slip_systems.txt\n  []\n[]\n\n\
[Postprocessors]\n  [stress]\n    type = ElementAverageValue\n    variable = stress_{comp}\n  []\n  [vonmises]\n    type = ElementAverageValue\n    variable = vonmises_stress\n  []\n[]\n\n\
[Preconditioning]\n  [smp]\n    type = SMP\n    full = true\n  []\n[]\n\n\
[Executioner]\n  type = Transient\n  solve_type = 'PJFNK'\n  petsc_options_iname = '-pc_type -pc_asm_overlap -sub_pc_type -ksp_type -ksp_gmres_restart'\n  petsc_options_value = ' asm      2              lu            gmres     200'\n  nl_abs_tol = 1e-8\n  nl_rel_tol = 1e-8\n  dt = {dt}\n  dtmin = {dtmin}\n  end_time = {end}\n[]\n\n\
[Outputs]\n  exodus = true\n  csv = true\n[]\n",
                n = rows.len(),
                ne = ug.cells.len(),
                mat = m.name,
                strain = o.strain,
                end = o.end_time,
                rate = o.strain * length / o.end_time,
                c11 = c[0],
                c12 = c[1],
                c44 = c[2],
                dt = o.end_time / o.steps.max(1) as f64,
                dtmin = o.end_time / o.steps.max(1) as f64 / 64.0,
            );
            write(folder, "polycrystal.i", &deck, &mut files)?;
            Ok(Case {
                folder: folder.into(),
                files,
                template: "moose_polycrystal",
                input: "polycrystal.i".into(),
                command: "mpiexec -n {np} {exe} -i polycrystal.i".into(),
                outputs: "polycrystal_out.e".into(),
            })
        }
        "spparks" => {
            let DataSet::ImageData(img) = ds else { bail!("a SPPARKS case is made from a microstructure volume (image data with FeatureIds)") };
            let f = img.cell_data.get("FeatureIds").or_else(|| img.cell_data.get("grain_id")).ok_or_else(|| anyhow!("the volume has no FeatureIds"))?;
            let ids: Vec<i64> = (0..f.len()).map(|i| f.get(i, 0) as i64).collect();
            if ids.iter().any(|x| *x <= 0) {
                bail!("some cells have feature id 0: fill them first (Fill Bad Data), every site needs a grain");
            }
            let q = *ids.iter().max().unwrap();
            let d: [i64; 3] = std::array::from_fn(|a| img.extent[2 * a + 1] - img.extent[2 * a]);
            let p = folder.join("grains.spparks");
            aurafx_io::spparks::write_sites(ds, &p)?;
            files.push(p);
            let h = img.spacing;
            let lattice = if (h[0] - h[1]).abs() < 1e-9 * h[0].abs() && (h[0] - h[2]).abs() < 1e-9 * h[0].abs() { h[0] } else { 1.0 };
            let deck = format!(
                "# Grain growth (Potts model) from the microstructure written by auraFX: {q} grains on a\n\
# {nx} x {ny} x {nz} lattice (lattice constant {lattice}, the volume's cell size when it is the same along x, y and z).\n\n\
seed            {seed}\n\
app_style       potts/neighonly {q}\n\
dimension       3\n\
lattice         sc/26n {lattice}\n\
region          box block 0 {nx} 0 {ny} 0 {nz}\n\
create_box      box\n\
create_sites    box\n\
read_sites      grains.spparks\n\
sweep           random\n\
temperature     {kt}\n\
diag_style      energy\n\
stats           {every}\n\
dump            1 text {every} dump.potts id i1 x y z\n\
run             {steps}\n",
                nx = d[0],
                ny = d[1],
                nz = d[2],
                seed = o.seed,
                kt = o.temperature,
                every = o.dump_every,
                steps = o.mc_steps,
            );
            write(folder, "in.potts", &deck, &mut files)?;
            Ok(Case {
                folder: folder.into(),
                files,
                template: "spparks",
                input: "in.potts".into(),
                command: "mpirun -np {np} {exe} -in in.potts".into(),
                outputs: "dump.potts".into(),
            })
        }
        "pinnacle" => {
            let m = material(&o.material)?;
            let (ug, gid) = grain_mesh(ds)?;
            let mesh = DataSet::UnstructuredGrid(ug.clone());
            let rows = aurafx_io::writers::grain_euler_rows(&mesh)?;
            let p = folder.join("mesh.e");
            aurafx_io::writers::write_file(&mesh, &p)?;
            files.push(p);
            // Orientation points: each element's centre with its grain and orientation.
            let mut t = String::from("# One row per element centre, written by auraFX; Euler angles in degrees.\nx,y,z,phi1,Phi,phi2,grain_id\n");
            for (e, (_, ids)) in ug.cells.iter().enumerate() {
                let k = ids.len().min(8).max(1);
                let c: [f64; 3] = std::array::from_fn(|a| ids[..k].iter().map(|i| ug.points.get(*i as usize)[a]).sum::<f64>() / k as f64);
                let r = rows[(gid[e] - 1) as usize];
                writeln!(t, "{},{},{},{},{},{},{}", c[0], c[1], c[2], r[0], r[1], r[2], gid[e])?;
            }
            write(folder, "orientations_points.csv", &t, &mut files)?;
            let nodes = set_names(&ug, "NodeSet ");
            let ax = axis_name(o.axis);
            let fixed = pick_set(&nodes, &o.fixed, &format!("{ax}-")).ok_or_else(|| anyhow!("no node set to hold (named {ax}-); give fixed="))?;
            let loaded = pick_set(&nodes, &o.loaded, &format!("{ax}+")).ok_or_else(|| anyhow!("no node set to pull (named {ax}+); give loaded="))?;
            let length = ds.bounds().size()[o.axis.min(2)];
            let mut bcs = format!(
                "  [./hold]\n    variable   = u\n    kind       = dirichlet\n    node_set   = {fixed}\n    components = [{a}]\n    values     = [0.0]\n  []\n  [./pull]\n    variable   = u\n    kind       = dirichlet\n    node_set   = {loaded}\n    components = [{a}]\n    values     = [1.0]\n    function   = pull\n  []\n",
                a = o.axis
            );
            for a in (0..3).filter(|a| *a != o.axis) {
                if let Some(s) = pick_set(&nodes, "", &format!("{}-", axis_name(a))) {
                    write!(bcs, "  [./hold_{n}]\n    variable   = u\n    kind       = dirichlet\n    node_set   = {s}\n    components = [{a}]\n    values     = [0.0]\n  []\n", n = axis_name(a))?;
                }
            }
            let c = m.elastic.iter().map(|x| x * 1000.0).collect::<Vec<_>>();
            let structure = match m.lattice {
                aurafx_crystal::mechanics::Lattice::Fcc => "fcc",
                aurafx_crystal::mechanics::Lattice::Bcc => "bcc",
                _ => bail!("{}: the crystal-plasticity case covers cubic (FCC, BCC) materials", m.name),
            };
            let deck = format!(
                "# Polycrystal crystal plasticity written by auraFX: {n} grains, {ne} elements, {mat}.\n\
# Orientations: orientations_points.csv (each element's centre, nearest point). The {fixed} nodes\n\
# are held along {ax}, the {loaded} nodes pulled to a strain of {strain} at t = {end}.\n\
# The slip and hardening parameters are a starting point: set them for the material.\n\n\
[Global]\n  backend    = numpy\n  use_gpu    = false\n  precision  = float64\n  seed       = 0\n  output_dir = solver\n[]\n\n\
[Problem]\n  type          = composed\n  dimension     = 3\n  finite_strain = true\n[]\n\n\
[Mesh]\n  type   = file\n  path   = mesh.e\n  format = exodus\n  scale  = 1.0\n  verify = true\n[]\n\n\
[Variables]\n  [./u]\n    family     = fem\n    components = 3\n  []\n[]\n\n\
[Functions]\n  [./pull]\n    kind       = parsed\n    expression = {rate}*t\n  []\n[]\n\n\
[Materials]\n  [./cp]\n    type                  = dislocation_density_cp\n    C11                   = {c11}\n    C12                   = {c12}\n    C44                   = {c44}\n    mu                    = {mu}\n    gamma0                = 1e-05\n    rate_exponent_n       = 8.33333333333\n    tau_c0                = 5.29147\n    q_lat                 = 1.4\n    alpha0                = 0.4\n    burgers               = {b}\n    rho0                  = 47.9872\n    k1                    = 0.198833\n    annihilation_diameter = 0.00343758\n    [./crystal]\n      structure    = {structure}\n      euler_angles = '0 0 0'\n    []\n  []\n[]\n\n\
[Kernels]\n  [./sd_cp]\n    type          = stress_divergence_cp\n    variable      = u\n    material      = cp\n    dim           = 3\n    finite_strain = true\n    gnd           = false\n  []\n[]\n\n\
[OrientationField]\n  [./source]\n    type          = point_file\n    path          = orientations_points.csv\n    interpolation = nearest\n  []\n[]\n\n\
[BCs]\n{bcs}[]\n\n\
[Executioner]\n  type             = cp_quasi_static\n  n_steps          = {steps}\n  t_end            = {end}\n  newton_tol       = 0.001\n  newton_rel_tol   = 0\n  max_iter         = 50\n  max_cutbacks     = 10\n  linear_method    = direct\n  snapshots_every  = {snap}\n[]\n\n\
[Output]\n  formats         = 'vtk csv json'\n  save_checkpoint = false\n  fields          = 'displacement stress strain equivalent_plastic_strain slip_activity mean_rho'\n[]\n",
                n = rows.len(),
                ne = ug.cells.len(),
                mat = m.name,
                strain = o.strain,
                end = o.end_time,
                rate = o.strain * length / o.end_time,
                c11 = c[0],
                c12 = c[1],
                c44 = c[2],
                mu = m.shear_modulus_gpa * 1000.0,
                // micrometres
                b = m.burgers_m * 1e6,
                steps = o.steps.max(1),
                snap = (o.steps / 4).max(1),
            );
            write(folder, "case.i", &deck, &mut files)?;
            Ok(Case {
                folder: folder.into(),
                files,
                template: "pinnacle",
                input: "case.i".into(),
                command: "{exe} run case.i".into(),
                outputs: "solver/fields.pvd".into(),
            })
        }
        s => bail!("unknown solver {s}: one of moose, spparks, pinnacle"),
    }
}
