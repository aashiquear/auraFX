//! Readers and writers.
//!
//! Entry point: [`read_file`], which sniffs the format from the extension (and a few
//! magic bytes) and returns either a single dataset or a lazily-loaded time series.

pub mod cgns;
pub mod cgns_writer;
pub mod csv;
pub mod ensight;
pub mod ensight_writer;
pub mod exodus;
pub mod exodus_writer;
pub mod hdf5w;
pub mod images;
pub mod lammps;
pub mod legacy_vtk;
pub mod molecular;
pub mod netcdf3;
pub mod netcdf_cf;
pub mod openfoam;
pub mod plot3d;
pub mod series;
pub mod stl;
pub mod surfaces;
pub mod tecplot;
pub mod vtkhdf;
pub mod writers;
pub mod xdmf;
pub mod xdmf_writer;
pub mod xml_vtk;

use anyhow::{anyhow, bail, Context, Result};
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
        (&["vtkhdf", "hdf"], "VTKHDF"),
        (&["case", "encas"], "EnSight Gold"),
        (&["tec", "tp", "dat", "plt"], "Tecplot (ASCII and binary)"),
        (&["xmf", "xdmf", "xdmf3"], "Xdmf"),
        (&["cgns", "adf"], "CGNS (HDF5)"),
        (&["nc", "nc4", "cdf", "netcdf"], "NetCDF (CF conventions)"),
        (&["xyz", "x", "p3d", "bin"], "PLOT3D grid (+ .q solution)"),
        (&["pdb", "ent"], "Protein Data Bank"),
        (&["cube"], "Gaussian cube"),
        (&["vasp", "poscar"], "VASP (POSCAR, CONTCAR, CHGCAR, LOCPOT by name)"),
        (&["png", "jpg", "jpeg", "bmp", "tif", "tiff"], "Images (TIFF stacks as volumes)"),
    ]
}

/// Every extension auraFX can open, plugin readers included.
pub fn all_extensions() -> Vec<String> {
    let mut v: Vec<String> = supported_formats().into_iter().flat_map(|(e, _)| e.iter().map(|x| x.to_string())).collect();
    for r in plugin_readers() {
        v.extend(r.extensions.iter().cloned());
    }
    v
}

/// A reader added at run time (Python plugins).
pub struct PluginReader {
    /// Lower-case extensions without the dot.
    pub extensions: Vec<String>,
    pub label: String,
    /// The plugin file that defined it.
    pub plugin: String,
    pub read: Arc<dyn Fn(&Path) -> Result<DataSet> + Send + Sync>,
}

static PLUGIN_READERS: std::sync::RwLock<Vec<Arc<PluginReader>>> = std::sync::RwLock::new(Vec::new());

pub fn plugin_readers() -> Vec<Arc<PluginReader>> {
    PLUGIN_READERS.read().unwrap().clone()
}

/// Add a reader; it takes precedence over built-in readers for its extensions.
pub fn register_reader(r: PluginReader) {
    let mut v = PLUGIN_READERS.write().unwrap();
    v.retain(|x| !(x.plugin == r.plugin && x.label == r.label));
    v.push(Arc::new(r));
}

/// Remove the readers a plugin file registered.
pub fn unregister_readers(plugin: &str) -> usize {
    let mut v = PLUGIN_READERS.write().unwrap();
    let n = v.len();
    v.retain(|x| x.plugin != plugin);
    n - v.len()
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
    if let Some(r) = plugin_readers().into_iter().rev().find(|r| r.extensions.iter().any(|e| *e == ext)) {
        let ds = (r.read)(path).with_context(|| format!("{} reader ({})", r.label, r.plugin))?;
        return Ok(Loaded::Static(Arc::new(ds)));
    }
    // VASP files are known by name (POSCAR, CHGCAR, ...).
    if molecular::is_vasp_name(path) {
        return Ok(Loaded::Static(Arc::new(molecular::read_vasp(path)?)));
    }
    // A wildcard file name stacks matching images into a volume.
    let wild = path.file_name().and_then(|f| f.to_str()).is_some_and(|f| f.contains(['*', '?']));
    if wild && images::is_image_ext(path) {
        return Ok(Loaded::Static(Arc::new(images::read_stack(path)?)));
    }
    let loaded = match ext.as_str() {
        "e" | "exo" | "ex2" | "exii" | "g" | "gen" | "ex2v2" => exodus::open(path)?,
        "png" | "jpg" | "jpeg" | "bmp" | "tif" | "tiff" => Loaded::Static(Arc::new(images::read(path)?)),
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
        "xyz" if plot3d::is_binary_grid(path) => Loaded::Static(Arc::new(plot3d::read(path, None)?)),
        "xyz" => match molecular::read_xyz(path) {
            Ok(l) => l,
            Err(e) => match plot3d::read(path, None) {
                Ok(d) => Loaded::Static(Arc::new(d)),
                Err(_) => return Err(e),
            },
        },
        "extxyz" => molecular::read_xyz(path)?,
        "x" | "p3d" | "bin" => Loaded::Static(Arc::new(plot3d::read(path, None)?)),
        "q" => bail!("{} is a PLOT3D solution: open the grid file (.xyz/.x/.g) next to it", path.display()),
        "data" | "lmp" => Loaded::Static(Arc::new(molecular::read_lammps_data(path)?)),
        "foam" => openfoam::open(path)?,
        "vtkhdf" | "hdf" | "vtkhdf5" => vtkhdf::open(path)?,
        "case" | "encas" => ensight::open(path)?,
        "tec" | "tp" => Loaded::Static(Arc::new(tecplot::read(path)?)),
        "pdb" | "ent" => molecular::read_pdb(path)?,
        "xmf" | "xdmf" | "xdmf3" => xdmf::open(path)?,
        "cgns" | "adf" => cgns::open(path)?,
        "nc" | "nc4" | "cdf" | "netcdf" if netcdf_cf::is_exodus(path) => exodus::open(path)?,
        "nc" | "nc4" | "cdf" | "netcdf" => netcdf_cf::open(path)?,
        "cube" => Loaded::Static(Arc::new(molecular::read_cube(path)?)),
        "dat" | "plt" | "szplt" if tecplot::is_binary(path) => Loaded::Static(Arc::new(tecplot::read_binary(path)?)),
        "dat" | "plt" if tecplot::sniff(path) => Loaded::Static(Arc::new(tecplot::read(path)?)),
        "szplt" => bail!("{}: Tecplot SZL (.szplt) files are not supported; save as .plt or ASCII", path.display()),
        "plt" => bail!("{}: not a Tecplot file", path.display()),
        "dat" => Loaded::Static(Arc::new(csv::read(path)?)),
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

thread_local! {
    /// Piece (k, n) being read on this thread, and whether a reader already restricted itself to it.
    static PIECE: std::cell::Cell<Option<(usize, usize)>> = const { std::cell::Cell::new(None) };
    static PIECED: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// The piece the current read is for (readers of partitioned formats read only theirs and
/// call [`mark_pieced`]).
pub(crate) fn current_piece() -> Option<(usize, usize)> {
    PIECE.with(|p| p.get())
}

pub(crate) fn mark_pieced() {
    PIECED.with(|p| p.set(true));
}

fn with_piece<T>(piece: Option<(usize, usize)>, f: impl FnOnce() -> Result<T>) -> Result<(T, bool)> {
    let prev = PIECE.with(|p| p.replace(piece));
    PIECED.with(|p| p.set(false));
    let r = f();
    let pieced = PIECED.with(|p| p.replace(false));
    PIECE.with(|p| p.set(prev));
    r.map(|v| (v, pieced))
}

/// Read piece `k` of `n` of a file (for parallel servers). Partitioned formats (`.pvtu` and
/// the other parallel VTK files) read only the piece files of this piece; other data is read
/// and then split with [`aurafx_data::pieces::split`].
pub fn read_file_piece(path: impl AsRef<Path>, piece: Option<(usize, usize)>) -> Result<Loaded> {
    let Some((k, n)) = piece.filter(|p| p.1 > 1) else { return read_file(path) };
    let (loaded, pieced) = with_piece(Some((k, n)), || read_file(path.as_ref()))?;
    Ok(match loaded {
        Loaded::Static(ds) if !pieced => Loaded::Static(Arc::new(aurafx_data::pieces::split(&ds, k, n))),
        other => other,
    })
}

/// [`load_step`] for piece `k` of `n`.
pub fn load_step_piece(step: &TimeStepRef, piece: Option<(usize, usize)>) -> Result<Arc<DataSet>> {
    let Some((k, n)) = piece.filter(|p| p.1 > 1) else { return load_step(step) };
    if let TimeStepRef::Loaded(ds) = step {
        return Ok(ds.clone());
    }
    let (ds, pieced) = with_piece(Some((k, n)), || load_step(step))?;
    Ok(if pieced { ds } else { Arc::new(aurafx_data::pieces::split(&ds, k, n)) })
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
            } else if path.extension().is_some_and(|e| e.eq_ignore_ascii_case("case") || e.eq_ignore_ascii_case("encas")) {
                ensight::read_step(path, *frame).map(Arc::new)
            } else if path.extension().is_some_and(|e| e.eq_ignore_ascii_case("cgns") || e.eq_ignore_ascii_case("adf")) {
                cgns::read_step(path, *frame).map(Arc::new)
            } else if netcdf_cf::is_netcdf_ext(path) {
                if netcdf_cf::is_exodus(path) { exodus::read_step(path, *frame) } else { netcdf_cf::read_step(path, *frame) }.map(Arc::new)
            } else if xdmf::is_xdmf_ext(path) {
                xdmf::read_step(path, *frame).map(Arc::new)
            } else if vtkhdf::is_vtkhdf_ext(path) {
                vtkhdf::read_step(path, *frame).map(Arc::new)
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

/// A cheap fingerprint of what a reader depends on, to detect changes while a solver runs:
/// size and modification time of the file (and, for directories / OpenFOAM cases, of its entries).
pub fn signature(path: &Path) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    fn add(h: &mut std::collections::hash_map::DefaultHasher, p: &Path) {
        if let Ok(m) = std::fs::metadata(p) {
            m.len().hash(h);
            if let Ok(t) = m.modified() {
                t.hash(h);
            }
        }
    }
    add(&mut h, path);
    let is_foam = path.extension().map(|e| e == "foam").unwrap_or(false);
    let dir = if is_foam { path.parent().map(|p| p.to_path_buf()) } else { None };
    if let Some(d) = dir {
        if let Ok(rd) = std::fs::read_dir(&d) {
            let mut names: Vec<String> = rd.flatten().map(|e| e.file_name().to_string_lossy().to_string()).collect();
            names.sort();
            names.hash(&mut h);
            for n in names.iter().filter(|n| n.parse::<f64>().is_ok()) {
                add(&mut h, &d.join(n));
            }
        }
    }
    h.finish()
}

/// Forget cached metadata so the next read sees the new file contents.
pub fn invalidate(path: &Path) {
    lammps::forget(path);
    exodus::forget(path);
    openfoam::forget(path);
}
