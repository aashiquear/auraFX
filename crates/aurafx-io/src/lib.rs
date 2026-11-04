//! Readers and writers.
//!
//! Entry point: [`read_file`], which sniffs the format from the extension (and a few
//! magic bytes) and returns either a single dataset or a lazily-loaded time series.

pub mod csv;
pub mod exodus;
pub mod lammps;
pub mod legacy_vtk;
pub mod molecular;
pub mod netcdf3;
pub mod openfoam;
pub mod series;
pub mod stl;
pub mod surfaces;
pub mod writers;
pub mod xml_vtk;

use anyhow::{anyhow, Context, Result};
use aurafx_data::{DataSet, TimeSeries, TimeStepRef};
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// Result of opening a file.
#[derive(Debug, Clone)]
pub enum Loaded {
    Static(Arc<DataSet>),
    Series(TimeSeries),
}

/// (extension, description) pairs for file dialogs.
pub fn supported_formats() -> Vec<(&'static [&'static str], &'static str)> {
    vec![
        (&["e", "exo", "ex2", "exii", "g", "gen"], "Exodus II"),
        (&["vtk"], "Legacy VTK"),
        (&["vtu", "vtp", "vti", "vtr", "vts", "vtt"], "VTK XML"),
        (&["pvtu", "pvtp", "pvti", "pvtr", "pvts"], "VTK XML (parallel)"),
        (&["vtm", "vtmb"], "VTK multi-block"),
        (&["pvd"], "ParaView data collection"),
        (&["dump", "lammpstrj", "dump.gz", "lammpstrj.gz"], "LAMMPS dump"),
        (&["xyz", "extxyz"], "XYZ / extended XYZ"),
        (&["data", "lmp"], "LAMMPS data file"),
        (&["foam"], "OpenFOAM case"),
        (&["csv", "tsv", "txt"], "Delimited text"),
        (&["stl"], "Stereolithography"),
        (&["ply"], "PLY surface"),
        (&["obj"], "Wavefront OBJ"),
    ]
}

pub fn all_extensions() -> Vec<&'static str> {
    supported_formats().into_iter().flat_map(|(e, _)| e.iter().copied()).collect()
}

fn ext_of(path: &Path) -> String {
    let name = path.file_name().and_then(|s| s.to_str()).unwrap_or("").to_ascii_lowercase();
    if name.ends_with(".gz") {
        let stem = &name[..name.len() - 3];
        return format!("{}.gz", stem.rsplit('.').next().unwrap_or(""));
    }
    name.rsplit('.').next().unwrap_or("").to_string()
}

/// Open a file by extension.
pub fn read_file(path: impl AsRef<Path>) -> Result<Loaded> {
    let path = path.as_ref();
    let ext = ext_of(path);
    let loaded = match ext.as_str() {
        "e" | "exo" | "ex2" | "exii" | "g" | "gen" | "ex2v2" => exodus::open(path)?,
        "vtk" => Loaded::Static(Arc::new(legacy_vtk::read(path)?)),
        "vtu" | "vtp" | "vti" | "vtr" | "vts" | "vtt" | "pvtu" | "pvtp" | "pvti" | "pvtr" | "pvts" | "vtm" | "vtmb" => {
            Loaded::Static(Arc::new(xml_vtk::read(path)?))
        }
        "pvd" => Loaded::Series(xml_vtk::read_pvd(path)?),
        "dump" | "lammpstrj" | "dump.gz" | "lammpstrj.gz" | "atom" => lammps::open(path)?,
        "csv" | "tsv" | "txt" => Loaded::Static(Arc::new(csv::read(path)?)),
        "stl" => Loaded::Static(Arc::new(stl::read(path)?)),
        "ply" => Loaded::Static(Arc::new(surfaces::read_ply(path)?)),
        "obj" => Loaded::Static(Arc::new(surfaces::read_obj(path)?)),
        "xyz" | "extxyz" => molecular::read_xyz(path)?,
        "data" | "lmp" => Loaded::Static(Arc::new(molecular::read_lammps_data(path)?)),
        "foam" => openfoam::open(path)?,
        _ => {
            // Sniff a few magic bytes.
            let head = std::fs::read(path).ok().map(|b| b[..b.len().min(64)].to_vec()).unwrap_or_default();
            if head.starts_with(b"# vtk DataFile") {
                Loaded::Static(Arc::new(legacy_vtk::read(path)?))
            } else if head.starts_with(b"<?xml") || head.starts_with(b"<VTKFile") {
                Loaded::Static(Arc::new(xml_vtk::read(path)?))
            } else if head.starts_with(b"CDF") {
                exodus::open(path)?
            } else if head.starts_with(b"ITEM: TIMESTEP") {
                lammps::open(path)?
            } else {
                return Err(anyhow!("unsupported file type: {}", path.display()));
            }
        }
    };
    Ok(loaded)
}

/// Resolve one step of a time series.
pub fn load_step(step: &TimeStepRef) -> Result<Arc<DataSet>> {
    match step {
        TimeStepRef::Loaded(ds) => Ok(ds.clone()),
        TimeStepRef::File(p) => match read_file(p)? {
            Loaded::Static(ds) => Ok(ds),
            Loaded::Series(s) => {
                let first = s.steps.first().ok_or_else(|| anyhow!("empty series {}", p.display()))?;
                load_step(first)
            }
        },
        TimeStepRef::FileFrame { path, frame } => {
            if path.extension().map(|e| e == "foam").unwrap_or(false) {
                openfoam::read_time(path, *frame).map(Arc::new)
            } else if exodus::is_exodus_ext(path) {
                exodus::read_step(path, *frame).map(Arc::new)
            } else {
                lammps::read_frame(path, *frame).map(Arc::new)
            }
        }
    }
}

pub(crate) fn read_bytes(path: &Path) -> Result<Vec<u8>> {
    std::fs::read(path).with_context(|| format!("reading {}", path.display()))
}

pub(crate) fn sibling(path: &Path, rel: &str) -> PathBuf {
    let rel_path = Path::new(rel);
    if rel_path.is_absolute() {
        rel_path.to_path_buf()
    } else {
        path.parent().unwrap_or(Path::new(".")).join(rel_path)
    }
}
