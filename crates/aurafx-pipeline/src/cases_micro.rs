//! Solver inputs made from a microstructure volume (image data with `FeatureIds` and orientations):
//!
//! - **DAMASK** (spectral grid solver): the grid as a `.vti` with the cell array `material`, a
//!   `material.yaml` with one material per grain (its orientation as a quaternion), the phase from
//!   the materials table, and a `load.yaml` pulling the volume along an axis.
//! - **FFT texture file** for elasto-viscoplastic FFT codes: one line per voxel, `phi1 Phi phi2 x y z
//!   grain phase` (Bunge angles in degrees, voxel indices from 1, x fastest).
//! - **LAMMPS atoms from a polycrystal**: every grain filled with the lattice of the material
//!   (FCC or BCC, its lattice constant from the Burgers vector) turned to the grain's orientation;
//!   atoms closer than half the nearest-neighbour distance across a grain boundary are removed. The
//!   data file, the atoms' grains (also as a dump to look at) and an input that relaxes and heats the polycrystal.

use super::cases::{Case, CaseOptions};
use anyhow::{anyhow, bail, Result};
use aurafx_crystal::mechanics::{Lattice, Material};
use aurafx_data::*;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

/// A microstructure volume: feature id per cell, each feature's Bunge angles (radians) and phase.
pub struct Micro<'a> {
    pub img: &'a ImageData,
    pub dims: [usize; 3],
    pub fid: Vec<i32>,
    /// Index = feature id (0 unused).
    pub euler: Vec<[f64; 3]>,
    pub phase: Vec<i32>,
}

impl<'a> Micro<'a> {
    pub fn of(ds: &'a DataSet) -> Result<Self> {
        let DataSet::ImageData(img) = ds else { bail!("this case is made from a microstructure volume (image data with FeatureIds), not a mesh") };
        let f = img
            .cell_data
            .get("FeatureIds")
            .or_else(|| img.cell_data.get("grain_id"))
            .ok_or_else(|| anyhow!("the volume has no FeatureIds: segment it or build one first"))?;
        let fid: Vec<i32> = (0..f.len()).map(|i| f.get(i, 0) as i32).collect();
        if fid.iter().any(|x| *x <= 0) {
            bail!("some cells have feature id 0: fill them first (Fill Bad Data), every voxel needs a grain");
        }
        let nf = *fid.iter().max().unwrap() as usize + 1;
        let dims: [usize; 3] = std::array::from_fn(|a| (img.extent[2 * a + 1] - img.extent[2 * a]).max(1) as usize);
        // Each grain's mean orientation, with the crystal symmetry of its phase.
        let o = aurafx_filters::features::Orients::of(ds, None, aurafx_filters::crystal::FROM_PHASES, None)
            .map_err(|e| anyhow!("{e:#} (the volume needs orientations: EulerAngles or Quats as cell data)"))?;
        let q = aurafx_filters::features::average_quats(&o, &fid, nf);
        let euler: Vec<[f64; 3]> = q.iter().map(|q| q.map(aurafx_crystal::rotation::quat_to_euler).unwrap_or([0.0; 3])).collect();
        let mut phase = vec![1i32; nf];
        let mut seen = vec![false; nf];
        for (c, &g) in fid.iter().enumerate() {
            if !seen[g as usize] {
                seen[g as usize] = true;
                phase[g as usize] = o.phase[c].max(1);
            }
        }
        Ok(Self { img, dims, fid, euler, phase })
    }
    pub fn grains(&self) -> usize {
        self.euler.len() - 1
    }
}

/// DAMASK's quaternion of Bunge angles (radians): its passive convention with P = -1 and a
/// non-negative real part (DAMASK's `Rotation.from_Euler_angles`).
pub fn damask_quaternion(e: [f64; 3]) -> [f64; 4] {
    let (s, d) = (0.5 * (e[0] + e[2]), 0.5 * (e[0] - e[2]));
    let (sp, cp) = (0.5 * e[1]).sin_cos();
    let q = [cp * s.cos(), sp * d.cos(), sp * d.sin(), cp * s.sin()];
    if q[0] < 0.0 {
        q.map(|x| -x)
    } else {
        q
    }
}

/// A number as YAML 1.1 readers take it for a float: a decimal point and a signed exponent
/// (`1.684000e+11`, `5.000000e-2`).
pub fn yaml_float(x: f64) -> String {
    let t = format!("{x:.6e}");
    match t.split_once('e') {
        Some((m, e)) if !e.starts_with('-') => format!("{m}e+{e}"),
        _ => t,
    }
}

fn cubic(m: &Material) -> Result<&'static str> {
    Ok(match m.lattice {
        Lattice::Fcc => "cF",
        Lattice::Bcc => "cI",
        _ => bail!("{}: these cases cover cubic (FCC, BCC) materials", m.name),
    })
}

/// The lattice constant (Å) of a cubic material from its Burgers vector: a = b√2 (FCC), 2b/√3 (BCC).
pub fn lattice_constant(m: &Material) -> Result<f64> {
    let b = m.burgers_m * 1e10;
    Ok(match m.lattice {
        Lattice::Fcc => b * 2f64.sqrt(),
        Lattice::Bcc => 2.0 * b / 3f64.sqrt(),
        _ => bail!("{}: these cases cover cubic (FCC, BCC) materials", m.name),
    })
}

fn put(folder: &Path, name: &str, text: &str, files: &mut Vec<PathBuf>) -> Result<()> {
    let p = folder.join(name);
    std::fs::write(&p, text)?;
    files.push(p);
    Ok(())
}

pub(crate) fn write_damask(ds: &DataSet, folder: &Path, m: &Material, o: &CaseOptions) -> Result<Case> {
    let mi = Micro::of(ds)?;
    let lattice = cubic(m)?;
    let mut files = Vec::new();
    // The grid: one material per grain, numbered from 0 in grain order.
    let mut grid = mi.img.clone();
    grid.cell_data = Attributes::new();
    grid.point_data = Attributes::new();
    grid.field_data = Attributes::new();
    grid.cell_data.insert(Array::new("material", 1, ArrayData::I32(mi.fid.iter().map(|g| g - 1).collect())));
    let p = folder.join("grid.vti");
    aurafx_io::writers::write_file(&DataSet::ImageData(grid), &p)?;
    files.push(p);
    // Materials: the phase, then one entry per grain with its orientation.
    let c = |i: usize| m.elastic.get(i).copied().unwrap_or(0.0) * 1e9;
    let phase = m.name.replace([' ', '(', ')'], "_").trim_matches('_').to_string();
    let plastic = match m.lattice {
        Lattice::Fcc => "      type: phenopowerlaw\n      output: [xi_sl, gamma_sl]\n      N_sl: [12]\n      n_sl: 20\n      a_sl: 2.25\n      dot_gamma_0_sl: 0.001\n      h_0_sl-sl: 75.e+6\n      xi_0_sl: [31.e+6]\n      xi_inf_sl: [63.e+6]\n      h_sl-sl: [1, 1, 1.4, 1.4, 1.4, 1.4, 1.4]\n",
        _ => "      type: phenopowerlaw\n      output: [xi_sl, gamma_sl]\n      N_sl: [12, 12]\n      n_sl: 20\n      a_sl: 2.0\n      dot_gamma_0_sl: 0.001\n      h_0_sl-sl: 1.0e+9\n      xi_0_sl: [95.e+6, 97.e+6]\n      xi_inf_sl: [222.e+6, 412.e+6]\n      h_sl-sl: [1, 1.4, 1, 1.4, 1.4, 1.4, 1.4, 1.4, 1.4, 1.4, 1.4, 1.4, 1.4, 1.4, 1.4, 1.4, 1.4, 1.4, 1.4, 1.4]\n",
    };
    let mut y = format!(
        "# DAMASK materials written by auraFX: {n} grains of {name}, one material per grain (material = grain id - 1\n\
# in grid.vti). Orientations are the grains' mean orientations as quaternions (DAMASK's convention).\n\
# The elastic constants are the materials table's; the plasticity parameters are a starting point.\n\
homogenization:\n  SX:\n    N_constituents: 1\n    mechanical: {{type: pass}}\n\n\
phase:\n  {phase}:\n    lattice: {lattice}\n    mechanical:\n      output: [F, P, F_e, F_p, L_p, O]\n      elastic: {{type: Hooke, C_11: {c11}, C_12: {c12}, C_44: {c44}}}\n      plastic:\n{plastic}\n\
material:\n",
        n = mi.grains(),
        name = m.name,
        c11 = yaml_float(c(0)),
        c12 = yaml_float(c(1)),
        c44 = yaml_float(c(2)),
        plastic = plastic.lines().map(|l| format!("  {l}\n")).collect::<String>(),
    );
    for g in 1..=mi.grains() {
        let q = damask_quaternion(mi.euler[g]);
        writeln!(y, "  - homogenization: SX\n    constituents:\n      - phase: {phase}\n        v: 1.0\n        O: [{}, {}, {}, {}]", q[0], q[1], q[2], q[3])?;
    }
    put(folder, "material.yaml", &y, &mut files)?;
    // The load: a strain rate along the axis, the other normal stresses free.
    let rate = o.strain / o.end_time.max(1e-12);
    let mut f = [["0".to_string(), "0".into(), "0".into()], ["0".into(), "0".into(), "0".into()], ["0".into(), "0".into(), "0".into()]];
    let mut p = [["x".to_string(), "x".into(), "x".into()], ["x".into(), "x".into(), "x".into()], ["x".into(), "x".into(), "x".into()]];
    let a = o.axis.min(2);
    for i in 0..3 {
        for j in 0..3 {
            if i == j && i != a {
                f[i][j] = "x".into();
                p[i][j] = "0".into();
            }
        }
    }
    f[a][a] = yaml_float(rate);
    let mat = |m: &[[String; 3]; 3]| format!("[[{}], [{}], [{}]]", m[0].join(", "), m[1].join(", "), m[2].join(", "));
    let load = format!(
        "# Tension along {ax} written by auraFX: a strain rate of {rate}/s up to a strain of {strain} at t = {end} s,\n\
# the other normal stresses free (x: not prescribed).\n\
solver: {{mechanical: spectral_basic}}\n\nloadstep:\n  - boundary_conditions:\n      mechanical:\n        dot_F: {f}\n        P: {p}\n    discretization: {{t: {end}, N: {n}}}\n    f_out: {fout}\n",
        ax = ["x", "y", "z"][a],
        rate = yaml_float(rate),
        strain = o.strain,
        end = o.end_time,
        f = mat(&f),
        p = mat(&p),
        n = o.steps.max(1),
        fout = (o.steps / 10).max(1),
    );
    put(folder, "load.yaml", &load, &mut files)?;
    Ok(Case {
        folder: folder.into(),
        files,
        template: "damask",
        input: "load.yaml".into(),
        command: "{exe} --load load.yaml --geom grid.vti --material material.yaml".into(),
        outputs: "grid_load.hdf5".into(),
    })
}

pub(crate) fn write_fft(ds: &DataSet, folder: &Path) -> Result<Case> {
    let mi = Micro::of(ds)?;
    let mut t = String::with_capacity(mi.fid.len() * 48);
    let [nx, ny, _] = mi.dims;
    for (c, &g) in mi.fid.iter().enumerate() {
        let (x, y, z) = (c % nx, (c / nx) % ny, c / (nx * ny));
        let e = mi.euler[g as usize];
        writeln!(t, "{:.4} {:.4} {:.4} {} {} {} {} {}", e[0].to_degrees(), e[1].to_degrees(), e[2].to_degrees(), x + 1, y + 1, z + 1, g, mi.phase[g as usize])?;
    }
    let mut files = Vec::new();
    put(folder, "micro.txt", &t, &mut files)?;
    let note = format!(
        "FFT microstructure written by auraFX: {} x {} x {} voxels, {} grains.\n\
micro.txt has one line per voxel, x fastest: phi1 Phi phi2 (Bunge, degrees), x y z (voxel indices from 1), grain id, phase id.\n\
Give its name and the grid size to the FFT code's input.\n",
        mi.dims[0],
        mi.dims[1],
        mi.dims[2],
        mi.grains()
    );
    put(folder, "micro_README.txt", &note, &mut files)?;
    Ok(Case { folder: folder.into(), files, template: "fft", input: "micro.txt".into(), command: "{exe}".into(), outputs: String::new() })
}

/// An element's atomic mass (g/mol) by the material's name, for the data file's Masses.
fn mass_of(m: &Material) -> f64 {
    let n = m.name.to_ascii_lowercase();
    [
        ("alumin", 26.9815),
        ("copper", 63.546),
        ("nickel", 58.6934),
        ("in718", 58.6934),
        ("coni", 58.9332),
        ("iron", 55.845),
        ("titan", 47.867),
        ("magnes", 24.305),
    ]
    .iter()
    .find(|(k, _)| n.contains(k))
    .map(|(_, v)| *v)
    .unwrap_or(1.0)
}

fn element_of(m: &Material) -> &'static str {
    let n = m.name.to_ascii_lowercase();
    [("alumin", "Al"), ("copper", "Cu"), ("nickel", "Ni"), ("in718", "Ni"), ("coni", "Co"), ("iron", "Fe")]
        .iter()
        .find(|(k, _)| n.contains(k))
        .map(|(_, v)| *v)
        .unwrap_or("X")
}

/// The atoms of a polycrystal: positions (Å) and the grain of each.
pub fn polycrystal_atoms(mi: &Micro, a: f64, lattice: Lattice, scale: f64) -> (Vec<[f64; 3]>, Vec<i32>, [f64; 3]) {
    let basis: Vec<[f64; 3]> = match lattice {
        Lattice::Bcc => vec![[0.0, 0.0, 0.0], [0.5, 0.5, 0.5]],
        _ => vec![[0.0, 0.0, 0.0], [0.5, 0.5, 0.0], [0.5, 0.0, 0.5], [0.0, 0.5, 0.5]],
    };
    let h = mi.img.spacing;
    let box_len: [f64; 3] = std::array::from_fn(|k| mi.dims[k] as f64 * h[k] * scale);
    let voxel: [f64; 3] = std::array::from_fn(|k| h[k] * scale);
    // Each grain's voxel bounds.
    let ng = mi.grains();
    let mut lo = vec![[usize::MAX; 3]; ng + 1];
    let mut hi = vec![[0usize; 3]; ng + 1];
    let [nx, ny, _] = mi.dims;
    for (c, &g) in mi.fid.iter().enumerate() {
        let ijk = [c % nx, (c / nx) % ny, c / (nx * ny)];
        for k in 0..3 {
            lo[g as usize][k] = lo[g as usize][k].min(ijk[k]);
            hi[g as usize][k] = hi[g as usize][k].max(ijk[k] + 1);
        }
    }
    let per_grain: Vec<(Vec<[f64; 3]>, i32)> = {
        use rayon::prelude::*;
        (1..=ng)
            .into_par_iter()
            .filter(|g| lo[*g][0] != usize::MAX)
            .map(|g| {
                // g maps sample to crystal coordinates; atoms are placed at gᵀ·(crystal lattice point).
                let om = aurafx_crystal::rotation::euler_to_matrix(mi.euler[g]);
                let to_c = |x: [f64; 3]| -> [f64; 3] { std::array::from_fn(|i| (0..3).map(|j| om[i][j] * x[j]).sum()) };
                let to_s = |x: [f64; 3]| -> [f64; 3] { std::array::from_fn(|i| (0..3).map(|j| om[j][i] * x[j]).sum()) };
                let smin: [f64; 3] = std::array::from_fn(|k| lo[g][k] as f64 * voxel[k]);
                let smax: [f64; 3] = std::array::from_fn(|k| hi[g][k] as f64 * voxel[k]);
                let (mut cmin, mut cmax) = ([f64::INFINITY; 3], [f64::NEG_INFINITY; 3]);
                for i in 0..8 {
                    let corner =
                        [if i & 1 == 0 { smin[0] } else { smax[0] }, if i & 2 == 0 { smin[1] } else { smax[1] }, if i & 4 == 0 { smin[2] } else { smax[2] }];
                    let cc = to_c(corner);
                    for k in 0..3 {
                        cmin[k] = cmin[k].min(cc[k]);
                        cmax[k] = cmax[k].max(cc[k]);
                    }
                }
                let r: [(i64, i64); 3] = std::array::from_fn(|k| ((cmin[k] / a).floor() as i64 - 1, (cmax[k] / a).ceil() as i64 + 1));
                let mut out = Vec::new();
                for i in r[0].0..=r[0].1 {
                    for j in r[1].0..=r[1].1 {
                        for k in r[2].0..=r[2].1 {
                            for b in &basis {
                                let pc = [(i as f64 + b[0]) * a, (j as f64 + b[1]) * a, (k as f64 + b[2]) * a];
                                let ps = to_s(pc);
                                if (0..3).any(|d| ps[d] < smin[d] || ps[d] >= smax[d]) {
                                    continue;
                                }
                                let v: [usize; 3] = std::array::from_fn(|d| ((ps[d] / voxel[d]) as usize).min(mi.dims[d] - 1));
                                if mi.fid[v[0] + nx * (v[1] + ny * v[2])] == g as i32 {
                                    out.push(ps);
                                }
                            }
                        }
                    }
                }
                (out, g as i32)
            })
            .collect()
    };
    let mut pos = Vec::new();
    let mut grain = Vec::new();
    for (p, g) in per_grain {
        grain.extend(std::iter::repeat_n(g, p.len()));
        pos.extend(p);
    }
    // Remove one atom of every pair closer than half the nearest-neighbour distance across a boundary
    // (periodic box): a cell list of that size.
    let nn = match lattice {
        Lattice::Bcc => a * 3f64.sqrt() / 2.0,
        _ => a / 2f64.sqrt(),
    };
    let cut = 0.5 * nn;
    let nc: [usize; 3] = std::array::from_fn(|k| ((box_len[k] / cut).floor() as usize).max(1));
    let cell_of = |p: &[f64; 3]| -> [usize; 3] { std::array::from_fn(|k| (((p[k] / box_len[k]).rem_euclid(1.0) * nc[k] as f64) as usize).min(nc[k] - 1)) };
    let mut cells: std::collections::HashMap<[usize; 3], Vec<usize>> = std::collections::HashMap::new();
    for (i, p) in pos.iter().enumerate() {
        cells.entry(cell_of(p)).or_default().push(i);
    }
    let mut dead = vec![false; pos.len()];
    for i in 0..pos.len() {
        if dead[i] {
            continue;
        }
        let c = cell_of(&pos[i]);
        for dx in -1i64..=1 {
            for dy in -1i64..=1 {
                for dz in -1i64..=1 {
                    let key = [
                        (c[0] as i64 + dx).rem_euclid(nc[0] as i64) as usize,
                        (c[1] as i64 + dy).rem_euclid(nc[1] as i64) as usize,
                        (c[2] as i64 + dz).rem_euclid(nc[2] as i64) as usize,
                    ];
                    let Some(list) = cells.get(&key) else { continue };
                    for &j in list {
                        if j <= i || dead[j] || grain[j] == grain[i] {
                            continue;
                        }
                        let d2: f64 = (0..3)
                            .map(|k| {
                                let mut d = pos[j][k] - pos[i][k];
                                d -= box_len[k] * (d / box_len[k]).round();
                                d * d
                            })
                            .sum();
                        if d2 < cut * cut {
                            dead[j] = true;
                        }
                    }
                }
            }
        }
    }
    let keep: Vec<usize> = (0..pos.len()).filter(|i| !dead[*i]).collect();
    (keep.iter().map(|i| pos[*i]).collect(), keep.iter().map(|i| grain[*i]).collect(), box_len)
}

pub(crate) fn write_lammps(ds: &DataSet, folder: &Path, m: &Material, o: &CaseOptions) -> Result<Case> {
    let mi = Micro::of(ds)?;
    cubic(m)?;
    let a = if o.lattice_constant > 0.0 { o.lattice_constant } else { lattice_constant(m)? };
    // Å per unit of the volume's coordinates: given, or so that its longest side holds about 40 cells.
    let longest = (0..3).map(|k| mi.dims[k] as f64 * mi.img.spacing[k]).fold(0.0, f64::max).max(1e-12);
    let scale = if o.angstrom_per_unit > 0.0 { o.angstrom_per_unit } else { 40.0 * a / longest };
    let (pos, grain, box_len) = polycrystal_atoms(&mi, a, m.lattice, scale);
    if pos.is_empty() {
        bail!("no atom fits in the volume: give a larger angstrom_per_unit");
    }
    let mut files = Vec::new();
    let mut d = String::with_capacity(pos.len() * 64);
    writeln!(
        d,
        "LAMMPS data file written by auraFX: a polycrystal of {} grains of {} (a = {a} A), {} atoms\n\n{} atoms\n1 atom types\n\n0 {} xlo xhi\n0 {} ylo yhi\n0 {} zlo zhi\n\nMasses\n\n1 {}\n\nAtoms # atomic\n",
        mi.grains(),
        m.name,
        pos.len(),
        pos.len(),
        box_len[0],
        box_len[1],
        box_len[2],
        mass_of(m)
    )?;
    for (i, p) in pos.iter().enumerate() {
        writeln!(d, "{} 1 {:.6} {:.6} {:.6}", i + 1, p[0], p[1], p[2])?;
    }
    put(folder, "atoms.data", &d, &mut files)?;
    let mut g = String::from("# atom id, grain id (the grain of the microstructure the atom was placed in)\n");
    for (i, gr) in grain.iter().enumerate() {
        writeln!(g, "{} {}", i + 1, gr)?;
    }
    put(folder, "atom_grains.txt", &g, &mut files)?;
    // The same atoms as a dump with their grains, to look at them (auraFX opens it colored by grain).
    let mut dump = format!(
        "ITEM: TIMESTEP\n0\nITEM: NUMBER OF ATOMS\n{}\nITEM: BOX BOUNDS pp pp pp\n0 {}\n0 {}\n0 {}\nITEM: ATOMS id type x y z grain\n",
        pos.len(),
        box_len[0],
        box_len[1],
        box_len[2]
    );
    for (i, (p, gr)) in pos.iter().zip(&grain).enumerate() {
        writeln!(dump, "{} 1 {:.4} {:.4} {:.4} {gr}", i + 1, p[0], p[1], p[2])?;
    }
    put(folder, "atoms.dump", &dump, &mut files)?;
    let el = element_of(m);
    let deck = format!(
        "# Polycrystal written by auraFX: {n} grains of {name} in a {bx:.1} x {by:.1} x {bz:.1} A periodic box ({na} atoms).\n\
# Set the potential file below (an EAM potential for {el}, for example from the NIST repository).\n\n\
units           metal\nboundary        p p p\natom_style      atomic\nread_data       atoms.data\n\n\
pair_style      eam/alloy\npair_coeff      * * {el}.eam.alloy {el}\n\n\
neighbor        2.0 bin\nneigh_modify    every 1 delay 0 check yes\n\n\
# relax the grain boundaries, then heat and hold at {t} K\n\
thermo          100\nthermo_style    custom step temp pe press vol\nminimize        1.0e-8 1.0e-10 1000 10000\n\
velocity        all create {t} {seed} mom yes rot yes\nfix             1 all npt temp {t} {t} 0.1 iso 0 0 1.0\n\
timestep        0.001\ncompute         pe all pe/atom\ndump            1 all custom {every} dump.lammpstrj id type x y z c_pe\n\
run             {steps}\n",
        n = mi.grains(),
        name = m.name,
        bx = box_len[0],
        by = box_len[1],
        bz = box_len[2],
        na = pos.len(),
        t = if o.temperature > 0.0 { o.temperature } else { 300.0 },
        seed = o.seed,
        every = (o.steps.max(1) * 100 / 10).max(1),
        steps = o.steps.max(1) * 100,
    );
    put(folder, "in.lammps", &deck, &mut files)?;
    Ok(Case {
        folder: folder.into(),
        files,
        template: "lammps",
        input: "in.lammps".into(),
        command: "mpirun -np {np} {exe} -in in.lammps".into(),
        outputs: "dump.lammpstrj".into(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn damask_quaternions_follow_its_convention() {
        // A turn of phi1 about z: (cos phi1/2, 0, 0, sin phi1/2) in DAMASK's convention.
        let q = damask_quaternion([90f64.to_radians(), 0.0, 0.0]);
        let h = (45f64).to_radians();
        assert!((q[0] - h.cos()).abs() < 1e-12 && q[1].abs() < 1e-12 && q[2].abs() < 1e-12 && (q[3] - h.sin()).abs() < 1e-12, "{q:?}");
        // Phi about x: (cos Phi/2, sin Phi/2, 0, 0); the real part is never negative.
        let q = damask_quaternion([0.0, 60f64.to_radians(), 0.0]);
        assert!((q[1] - 30f64.to_radians().sin()).abs() < 1e-12 && q[2].abs() < 1e-12);
        for e in [[5.0, 2.0, 4.0], [0.3, 3.0, 6.1]] {
            let q = damask_quaternion(e);
            assert!(q[0] >= 0.0 && (q.iter().map(|x| x * x).sum::<f64>() - 1.0).abs() < 1e-12);
        }
    }
}
