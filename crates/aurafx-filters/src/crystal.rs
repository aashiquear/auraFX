//! Crystal orientation filters: orientation colors, conversions between representations,
//! frame rotations, and the IPF color key source.
//!
//! Orientations are read from an array by its number of components: 3 = Bunge Euler angles
//! (radians), 4 = quaternions stored (x, y, z, w), 9 = orientation matrices (row major). The
//! crystal class of each point comes from a phase array indexing the `CrystalStructures` field
//! array (EBSD readers write both), or from a class chosen for the whole dataset.

use anyhow::{anyhow, bail, Result};
use aurafx_crystal::{self as cx, Laue, Quat};
use aurafx_data::*;
use crate::Association;

/// Representations a 3/4/9-component (or any) array can hold.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Repr {
    Euler,
    Quaternion,
    Matrix,
    AxisAngle,
    Rodrigues,
    Homochoric,
}

impl Repr {
    pub const LABELS: [&'static str; 6] = ["Euler angles", "Quaternions", "Orientation matrices", "Axis-angle", "Rodrigues vectors", "Homochoric vectors"];
    pub fn from_label(s: &str) -> Option<Repr> {
        Some(match s {
            "Euler angles" => Repr::Euler,
            "Quaternions" => Repr::Quaternion,
            "Orientation matrices" => Repr::Matrix,
            "Axis-angle" => Repr::AxisAngle,
            "Rodrigues vectors" => Repr::Rodrigues,
            "Homochoric vectors" => Repr::Homochoric,
            _ => return None,
        })
    }
    pub fn ncomp(self) -> usize {
        match self {
            Repr::Euler | Repr::Homochoric => 3,
            Repr::Quaternion | Repr::AxisAngle | Repr::Rodrigues => 4,
            Repr::Matrix => 9,
        }
    }
    pub fn default_name(self) -> &'static str {
        match self {
            Repr::Euler => "EulerAngles",
            Repr::Quaternion => "Quats",
            Repr::Matrix => "OrientationMatrix",
            Repr::AxisAngle => "AxisAngles",
            Repr::Rodrigues => "Rodrigues",
            Repr::Homochoric => "Homochoric",
        }
    }
    /// The representation an array of `ncomp` components is taken to hold.
    pub fn guess(ncomp: usize) -> Option<Repr> {
        Some(match ncomp {
            3 => Repr::Euler,
            4 => Repr::Quaternion,
            9 => Repr::Matrix,
            _ => return None,
        })
    }
    fn decode(self, v: &[f64]) -> Quat {
        match self {
            Repr::Euler => cx::euler_to_quat([v[0], v[1], v[2]]),
            Repr::Quaternion => cx::quat_from_xyzw([v[0], v[1], v[2], v[3]]),
            Repr::Matrix => cx::matrix_to_quat(&[[v[0], v[1], v[2]], [v[3], v[4], v[5]], [v[6], v[7], v[8]]]),
            Repr::AxisAngle => cx::axis_angle_to_quat([v[0], v[1], v[2]], v[3]),
            Repr::Rodrigues => cx::rodrigues_to_quat([v[0], v[1], v[2], v[3]]),
            Repr::Homochoric => cx::homochoric_to_quat([v[0], v[1], v[2]]),
        }
    }
    fn encode(self, q: Quat) -> Vec<f64> {
        match self {
            Repr::Euler => cx::quat_to_euler(q).to_vec(),
            Repr::Quaternion => cx::quat_to_xyzw(q).to_vec(),
            Repr::Matrix => cx::quat_to_matrix(q).concat(),
            Repr::AxisAngle => {
                let (w, n) = cx::to_axis_angle(q);
                vec![n[0], n[1], n[2], w]
            }
            Repr::Rodrigues => cx::quat_to_rodrigues(q).to_vec(),
            Repr::Homochoric => cx::quat_to_homochoric(q).to_vec(),
        }
    }
}

/// The crystal class choice of the filters: from the file's phases, or one for all points.
pub const FROM_PHASES: &str = "From the phases";

pub fn structure_choices() -> Vec<&'static str> {
    let mut v = vec![FROM_PHASES];
    v.extend(Laue::ALL.iter().rev().map(|l| l.label()));
    v
}

/// The attributes holding `name` (cells first unless `assoc` says points).
fn find<'a>(ds: &'a DataSet, assoc: Option<Association>, name: &str) -> Option<(Association, &'a Array)> {
    let get = |a: Association| match a {
        Association::Points => ds.point_data().and_then(|x| x.get(name)).map(|x| (a, x)),
        Association::Cells => ds.cell_data().and_then(|x| x.get(name)).map(|x| (a, x)),
    };
    match assoc {
        Some(a) => get(a).or_else(|| get(if a == Association::Cells { Association::Points } else { Association::Cells })),
        None => get(Association::Cells).or_else(|| get(Association::Points)),
    }
}

/// The orientation array to use: the one named, or the first of EulerAngles, Quats,
/// OrientationMatrix found.
pub fn orientation_array<'a>(ds: &'a DataSet, named: Option<(Association, String)>) -> Result<(Association, &'a Array)> {
    if let Some((a, n)) = &named {
        return find(ds, Some(*a), n).ok_or_else(|| anyhow!("no array {n}"));
    }
    for n in ["EulerAngles", "Quats", "OrientationMatrix"] {
        if let Some(x) = find(ds, None, n) {
            return Ok(x);
        }
    }
    bail!("no orientation array (EulerAngles, Quats or OrientationMatrix); choose one")
}

/// The crystal class of every tuple of an array of `n` tuples (None: unknown or not indexed).
fn classes(ds: &DataSet, assoc: Association, n: usize, structure: &str, phases: Option<&str>) -> Result<Vec<Option<Laue>>> {
    if structure != FROM_PHASES {
        let l = Laue::from_label(structure).ok_or_else(|| anyhow!("unknown crystal structure {structure}"))?;
        return Ok(vec![Some(l); n]);
    }
    let codes: Vec<u32> = match ds.field_data().get("CrystalStructures") {
        Some(a) => (0..a.len()).map(|i| a.get(i, 0) as u32).collect(),
        None => bail!("the data has no CrystalStructures field array; choose the crystal structure"),
    };
    let pname = phases.filter(|p| !p.is_empty()).unwrap_or("Phases");
    let ph = find(ds, Some(assoc), pname).map(|(_, a)| a);
    Ok((0..n)
        .map(|i| {
            // Without a phase array every point belongs to phase 1.
            let p = ph.map(|a| a.get(i, 0) as i64).unwrap_or(1);
            if p <= 0 {
                return None;
            }
            codes.get(p as usize).and_then(|c| Laue::from_code(*c))
        })
        .collect())
}

fn values(a: &Array, i: usize) -> Vec<f64> {
    (0..a.ncomp).map(|c| a.get(i, c)).collect()
}

fn with_array(ds: &DataSet, assoc: Association, arr: Array) -> Result<DataSet> {
    let mut out = ds.clone();
    let attrs = match assoc {
        Association::Points => out.point_data_mut(),
        Association::Cells => out.cell_data_mut(),
    }
    .ok_or_else(|| anyhow!("this kind of data cannot hold arrays"))?;
    attrs.insert(arr);
    Ok(out)
}

/// The color keys of Orientation Colors.
pub const KEY_IPF: &str = "Inverse pole figure";
pub const KEY_EULER: &str = "Euler angles";

/// Orientation Colors: an 8-bit RGB array coloring each orientation, by the inverse pole
/// figure key for sample direction `dir` (crystal direction along `dir`, brought into the
/// standard triangle of its class), or by its Euler angles. Points not indexed (phase 0), of an
/// unknown class, or with a zero `mask` value are black.
#[allow(clippy::too_many_arguments)]
pub fn orientation_colors(
    ds: &DataSet,
    orientations: Option<(Association, String)>,
    key: &str,
    dir: [f64; 3],
    structure: &str,
    phases: Option<&str>,
    mask: Option<&str>,
    name: &str,
) -> Result<DataSet> {
    let (assoc, a) = orientation_array(ds, orientations)?;
    let repr = Repr::guess(a.ncomp).ok_or_else(|| anyhow!("{}: {} components is not an orientation array (3, 4 or 9)", a.name, a.ncomp))?;
    let n = a.len();
    let euler_key = key == KEY_EULER;
    let cls = if euler_key { vec![Some(Laue::Triclinic); n] } else { classes(ds, assoc, n, structure, phases)? };
    let mask = mask.filter(|m| !m.is_empty()).map(|m| find(ds, Some(assoc), m).map(|x| x.1).ok_or_else(|| anyhow!("no mask array {m}"))).transpose()?;
    let len = (dir[0] * dir[0] + dir[1] * dir[1] + dir[2] * dir[2]).sqrt();
    if len == 0.0 && !euler_key {
        bail!("the reference direction is zero");
    }
    let d = dir.map(|v| v / len.max(1e-300));
    let rgb: Vec<u8> = (0..n)
        .flat_map(|i| {
            let masked = mask.is_some_and(|m| m.get(i, 0) == 0.0);
            match cls[i] {
                Some(l) if !masked => {
                    let v = values(a, i);
                    if euler_key {
                        let e = if repr == Repr::Euler { [v[0], v[1], v[2]] } else { cx::quat_to_euler(repr.decode(&v)) };
                        cx::euler_color(e)
                    } else {
                        cx::ipf_color(repr.decode(&v), l, d)
                    }
                }
                _ => [0, 0, 0],
            }
        })
        .collect();
    with_array(ds, assoc, Array::new(name, 3, ArrayData::U8(rgb)))
}

/// Convert Orientations: the orientations of `input` (holding `from`) as `to`, optionally each
/// replaced by its symmetry-equivalent with the smallest rotation (its fundamental zone).
pub fn convert_orientations(ds: &DataSet, input: Option<(Association, String)>, from: Repr, to: Repr, name: &str, reduce: bool, structure: &str, phases: Option<&str>) -> Result<DataSet> {
    let (assoc, a) = orientation_array(ds, input)?;
    if a.ncomp != from.ncomp() {
        bail!("{} has {} components; {:?} needs {}", a.name, a.ncomp, from, from.ncomp());
    }
    let cls = if reduce { classes(ds, assoc, a.len(), structure, phases)? } else { Vec::new() };
    let mut out = Vec::with_capacity(a.len() * to.ncomp());
    for i in 0..a.len() {
        let mut q = from.decode(&values(a, i));
        if let Some(Some(l)) = cls.get(i) {
            q = cx::reduce_to_fundamental_zone(q, *l);
        }
        out.extend(to.encode(q));
    }
    let data = if matches!(a.data, ArrayData::F64(_)) { ArrayData::F64(out) } else { ArrayData::F32(out.iter().map(|v| *v as f32).collect()) };
    let name = if name.is_empty() { to.default_name() } else { name };
    with_array(ds, assoc, Array::new(name, to.ncomp(), data))
}

pub const FRAME_SAMPLE: &str = "Sample";
pub const FRAME_CRYSTAL: &str = "Crystal";

/// Rotate Orientations: re-express the orientations after rotating the sample frame
/// (g' = g Rᵀ: the axes the orientations refer to turn by R; the data does not move) or the
/// crystal frame (g' = R g), R being the rotation by `angle` (degrees) about `axis`. The array
/// is replaced, keeping its representation.
pub fn rotate_orientations(ds: &DataSet, input: Option<(Association, String)>, frame: &str, axis: [f64; 3], angle: f64) -> Result<DataSet> {
    let (assoc, a) = orientation_array(ds, input)?;
    let repr = Repr::guess(a.ncomp).ok_or_else(|| anyhow!("{}: not an orientation array", a.name))?;
    let r = cx::axis_angle_to_quat(axis, angle.to_radians());
    let crystal = frame == FRAME_CRYSTAL;
    let mut out = Vec::with_capacity(a.len() * a.ncomp);
    for i in 0..a.len() {
        let q = repr.decode(&values(a, i));
        let q = if crystal { cx::mul(r, q) } else { cx::mul(q, cx::conj(r)) };
        out.extend(repr.encode(cx::positive(q)));
    }
    let data = if matches!(a.data, ArrayData::F64(_)) { ArrayData::F64(out) } else { ArrayData::F32(out.iter().map(|v| *v as f32).collect()) };
    with_array(ds, assoc, Array::new(a.name.clone(), a.ncomp, data))
}

/// The IPF color key of a crystal class: the standard triangle (sector) in stereographic
/// projection, `resolution` steps along each side, colored by the key (`IPFColor` point data),
/// with its outline as lines.
pub fn ipf_key(structure: &str, resolution: usize) -> Result<PolyData> {
    let laue = Laue::from_label(structure).ok_or_else(|| anyhow!("unknown crystal structure {structure}"))?;
    let sector = cx::ipf_sector(laue);
    let n = resolution.clamp(4, 400);
    let (e0, e1) = (sector.eta_min, sector.eta_max);
    let mut pts: Vec<f64> = Vec::new();
    let mut rgb: Vec<u8> = Vec::new();
    let mut polys = Cells::new();
    let add = |eta: f64, chi: f64, pts: &mut Vec<f64>, rgb: &mut Vec<u8>| -> u32 {
        let r = (chi * 0.5).tan();
        pts.extend([r * eta.cos(), r * eta.sin(), 0.0]);
        let c = cx::ipf::sector_color(&sector, eta, chi);
        rgb.extend(c.map(|v| (v * 255.0).round() as u8));
        (pts.len() / 3 - 1) as u32
    };
    // A grid in (eta, fraction of chi_max): rows of n + 1 points, the first row at the pole.
    let mut grid = vec![vec![0u32; n + 1]; n + 1];
    for (r, row) in grid.iter_mut().enumerate() {
        for (k, id) in row.iter_mut().enumerate() {
            let eta = e0 + (e1 - e0) * k as f64 / n as f64;
            let chi = sector.chi_max(eta) * r as f64 / n as f64;
            *id = add(eta, chi, &mut pts, &mut rgb);
        }
    }
    for r in 0..n {
        for k in 0..n {
            polys.push(CellType::Quad, &[grid[r][k], grid[r + 1][k], grid[r + 1][k + 1], grid[r][k + 1]]);
        }
    }
    let mut lines = Cells::new();
    let outline: Vec<u32> = std::iter::once(grid[0][0]).chain((0..=n).map(|k| grid[n][k])).chain(std::iter::once(grid[0][n])).collect();
    lines.push(CellType::PolyLine, &outline);
    let mut pd = PolyData { points: Points::new_f64(pts), polys, lines, ..Default::default() };
    pd.point_data.insert(Array::new("IPFColor", 3, ArrayData::U8(rgb)));
    Ok(pd)
}
