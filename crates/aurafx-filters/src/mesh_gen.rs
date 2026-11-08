//! Structured hexahedral meshes (box, inclusions in a box), elevation to HEX20/HEX27 and the
//! hexahedron quality metrics.

use anyhow::{anyhow, bail, Result};
use aurafx_data::*;
use aurafx_mesh::hex::HexMesh;
use aurafx_mesh::inclusions::{self as inc, StarShape};
use aurafx_mesh::quality::{hex_metric, tet_metric, HexMetric, TetMetric};
use aurafx_mesh::structured::{box_mesh as box_hex, Division};

pub const ELEMENT_TYPES: [&str; 3] = ["HEX8", "HEX20", "HEX27"];

pub fn nodes_per_element(t: &str) -> usize {
    match t {
        "HEX20" => 20,
        "HEX27" => 27,
        _ => 8,
    }
}

/// Box Mesh: a box of hexahedra, `cells` along x, y, z, graded by `ratio` (largest over
/// smallest interval) towards `cluster`.
pub fn box_mesh(origin: [f64; 3], size: [f64; 3], cells: [usize; 3], ratio: [f64; 3], cluster: &str, element: &str) -> Result<DataSet> {
    if size.iter().any(|s| !(*s > 0.0)) || cells.iter().any(|c| *c == 0) {
        bail!("the size and the cell counts must be positive");
    }
    let div: Vec<Division> = (0..3).map(|a| Division { cells: cells[a], ratio: ratio[a].max(1e-9), cluster: cluster.to_lowercase() }).collect();
    let hi = std::array::from_fn(|a| origin[a] + size[a]);
    let mut m = box_hex(origin, hi, [&div[0], &div[1], &div[2]]);
    m.elevate(nodes_per_element(element), &[], &|_, p| p);
    Ok(m.to_dataset())
}

pub const SHAPES: [&str; 4] = ["Sphere", "Ellipsoid", "Superquadric", "Harmonic grain"];

/// Parameters of Inclusion Mesh.
pub struct InclusionParams {
    pub length: [f64; 3],
    pub cells: usize,
    pub count: usize,
    pub radius_mean: f64,
    pub radius_std: f64,
    pub shape: String,
    /// Semi-axes of ellipsoids and superquadrics relative to the radius.
    pub aspect: [f64; 3],
    pub exponents: [f64; 2],
    pub roughness: f64,
    pub random_orientation: bool,
    pub seed: u64,
    pub box_ratio: f64,
    pub gap_cells: usize,
    pub wall_cells: usize,
    pub n_inner: usize,
    pub n_outer: usize,
    pub inner_grading: f64,
    pub outer_grading: f64,
    pub core_fraction: f64,
    pub core_roundness: f64,
    pub patch_blend: f64,
    pub separate_blocks: bool,
    pub element: String,
    pub periodic: bool,
    /// Collective rearrangement instead of one random pass (denser packings).
    pub collective: bool,
}

fn random_rotation(rng: &mut inc::Rng) -> [[f64; 3]; 3] {
    let q = aurafx_crystal::random_quat(rng.uniform(), rng.uniform(), rng.uniform());
    aurafx_crystal::quat_to_matrix(q)
}

/// Inclusion Mesh: a box of matrix with randomly placed star-shaped inclusions (packed on the
/// background grid with whole-cell gaps), meshed with conforming hexahedra.
pub fn inclusion_mesh(p: &InclusionParams) -> Result<DataSet> {
    let h = p.length.iter().cloned().fold(f64::INFINITY, f64::min) / p.cells.max(1) as f64;
    let div: [usize; 3] = std::array::from_fn(|a| (p.length[a] / h).round().max(1.0) as usize);
    let grid = inc::Grid { div, origin: [0.0; 3], h };
    let spec = inc::PackingSpec {
        count: p.count,
        radius_mean: p.radius_mean,
        radius_std: p.radius_std,
        box_ratio: p.box_ratio,
        gap_cells: p.gap_cells,
        wall_cells: p.wall_cells,
        ..Default::default()
    };
    let mut rng = inc::Rng::new(p.seed);
    let shape = p.shape.clone();
    let (aspect, ex, rough, rot) = (p.aspect, p.exponents, p.roughness, p.random_orientation);
    let make = move |r: f64, rng: &mut inc::Rng| -> StarShape {
        let rotation = if rot { Some(random_rotation(rng)) } else { None };
        match shape.as_str() {
            "Ellipsoid" => StarShape::Ellipsoid { centre: [0.0; 3], semi_axes: aspect.map(|a| a * r), rotation },
            "Superquadric" => StarShape::Superquadric { centre: [0.0; 3], semi_axes: aspect.map(|a| a * r), exponent_xy: ex[0], exponent_z: ex[1], rotation },
            "Harmonic grain" => {
                // Coefficients decaying as 1/l^2 from degree 2 to 4, scaled by the roughness.
                let mut c = Vec::new();
                for l in 2..=4i32 {
                    for m in -l..=l {
                        c.push((l, m, rough / (l * l) as f64 * rng.normal()));
                    }
                }
                StarShape::Harmonic { centre: [0.0; 3], radius: r, coefficients: c }
            }
            _ => StarShape::Sphere { centre: [0.0; 3], radius: r },
        }
    };
    let placed = if p.collective { inc::pack_collective(&grid, &spec, &mut rng, &make, 2000, 0.5, 3) } else { inc::pack(&grid, &spec, &mut rng, &make) };
    let block = inc::BlockOptions {
        core_fraction: p.core_fraction,
        core_roundness: p.core_roundness,
        n_inner: p.n_inner.max(1),
        n_outer: p.n_outer.max(1),
        inner_grading: p.inner_grading,
        outer_grading: p.outer_grading,
        spacing: inc::Spacing::Blend(p.patch_blend),
    };
    let opt = inc::AssemblyOptions { separate_blocks: p.separate_blocks, nodes_per_element: nodes_per_element(&p.element), periodic: p.periodic };
    let m = inc::assemble(&grid, &placed, &block, &opt).map_err(|e| anyhow!(e))?;
    let mut ds = m.to_dataset();
    let fd = ds.field_data_mut_any();
    let vol: f64 = placed.iter().map(|q| q.shape.volume_estimate()).sum();
    let domain: f64 = (0..3).map(|a| div[a] as f64 * h).product();
    fd.insert(Array::scalars_f64("InclusionsPlaced", vec![placed.len() as f64]));
    fd.insert(Array::scalars_f64("InclusionsRequested", vec![p.count as f64]));
    fd.insert(Array::scalars_f64("VolumeFraction", vec![vol / domain]));
    fd.insert(Array::scalars_f64("CellSize", vec![h]));
    Ok(ds)
}

/// Elevate: HEX8 elements to HEX20 or HEX27 (new nodes at edge, face and cell centres, shared
/// between neighbouring elements).
pub fn elevate(ds: &DataSet, element: &str) -> Result<DataSet> {
    let mut m = HexMesh::from_dataset(ds).map_err(|e| anyhow!(e))?;
    if m.npe != 8 {
        bail!("the mesh already has {}-node elements", m.npe);
    }
    m.elevate(nodes_per_element(element), &[], &|_, p| p);
    let mut out = m.to_dataset();
    // Keep the input's other field arrays.
    for a in ds.field_data().iter() {
        if out.field_data().get(&a.name).is_none() {
            out.field_data_mut_any().insert(a.clone());
        }
    }
    Ok(out)
}

/// Corners (Exodus/VTK order) of every hexahedral cell; None for other cells.
fn hex_corners(ds: &DataSet) -> Result<Vec<Option<[[f64; 3]; 8]>>> {
    let ug = ds.to_unstructured().ok_or_else(|| anyhow!("mesh quality needs a mesh"))?;
    Ok(ug
        .cells
        .iter()
        .map(|(t, ids)| {
            let order: Option<[usize; 8]> = match t {
                CellType::Hexahedron | CellType::QuadraticHexahedron | CellType::TriquadraticHexahedron | CellType::BiquadraticQuadraticHexahedron => {
                    Some([0, 1, 2, 3, 4, 5, 6, 7])
                }
                CellType::Voxel => Some([0, 1, 3, 2, 4, 5, 7, 6]),
                _ => None,
            };
            order.filter(|_| ids.len() >= 8).map(|o| o.map(|k| ug.points.get(ids[k] as usize)))
        })
        .collect())
}

/// The hexahedron metrics computed by Mesh Quality (the generic edge ratio is kept instead
/// of the hexahedron one).
pub fn hex_metrics() -> Vec<HexMetric> {
    HexMetric::ALL.iter().copied().filter(|m| *m != HexMetric::EdgeRatio).collect()
}

fn tet_corners(ds: &DataSet) -> Result<Vec<Option<[[f64; 3]; 4]>>> {
    let ug = ds.to_unstructured().ok_or_else(|| anyhow!("mesh quality needs a mesh"))?;
    Ok(ug
        .cells
        .iter()
        .map(|(t, ids)| matches!(t, CellType::Tetra | CellType::QuadraticTetra).then(|| std::array::from_fn(|k| ug.points.get(ids[k] as usize))))
        .collect())
}

/// The tetrahedron metrics computed by Mesh Quality (the generic edge ratio is kept instead).
pub fn tet_metrics() -> Vec<TetMetric> {
    TetMetric::ALL.iter().copied().filter(|m| *m != TetMetric::EdgeRatio).collect()
}

/// Mesh Quality: the generic measures of every cell plus, for hexahedra and tetrahedra, their
/// full metric sets (each array holds the hexahedron value for hexahedra, the tetrahedron
/// value for tetrahedra and NaN for other cells); or, with `summary`, a table of each metric
/// over the hexahedra and over the tetrahedra: its minimum, maximum and mean, the acceptable
/// range and how many cells fall outside it.
pub fn mesh_quality(ds: &DataSet, summary: bool) -> Result<DataSet> {
    let base = crate::geometry::mesh_quality(ds)?;
    let hexes = hex_corners(&base)?;
    let tets = tet_corners(&base)?;
    let mean = |v: &[f64]| if v.is_empty() { 0.0 } else { v.iter().sum::<f64>() / v.len() as f64 };
    let hex_avg = mean(&hexes.iter().flatten().map(|p| hex_metric(HexMetric::Volume, p, 0.0)).collect::<Vec<_>>());
    let tet_avg = mean(&tets.iter().flatten().map(|p| tet_metric(TetMetric::Volume, p, 0.0)).collect::<Vec<_>>());
    let hm = hex_metrics();
    let tm = tet_metrics();
    let has_hex = hexes.iter().any(|h| h.is_some());
    let has_tet = tets.iter().any(|t| t.is_some());
    let hex_values: Vec<Vec<f64>> =
        hm.iter().map(|m| hexes.iter().map(|h| h.as_ref().map(|p| hex_metric(*m, p, hex_avg)).unwrap_or(f64::NAN)).collect()).collect();
    let tet_values: Vec<Vec<f64>> =
        tm.iter().map(|m| tets.iter().map(|t| t.as_ref().map(|p| tet_metric(*m, p, tet_avg)).unwrap_or(f64::NAN)).collect()).collect();
    if summary {
        let mut t = Table::default();
        let mut rows: Vec<(String, f64, f64, f64, f64, f64, f64, f64)> = Vec::new();
        let er = base.cell_data().and_then(|c| c.get("EdgeRatio"));
        let mut add = |name: &str, v: &[f64], range: Option<(f64, f64)>| {
            let v: Vec<f64> = v.iter().copied().filter(|x| !x.is_nan()).collect();
            if v.is_empty() {
                return;
            }
            let (mn, mx) = v.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), x| (a.min(*x), b.max(*x)));
            let mean = v.iter().sum::<f64>() / v.len() as f64;
            let (lo, hi) = range.unwrap_or((f64::NAN, f64::NAN));
            // A little slack for rounding (a perfect cube scores 1 plus or minus an ulp).
            let out = if range.is_some() {
                v.iter().filter(|x| **x < lo - 1e-9 * lo.abs().max(1.0) || **x > hi + 1e-9 * hi.abs().max(1.0)).count() as f64
            } else {
                f64::NAN
            };
            rows.push((name.to_string(), mn, mx, mean, lo, hi, out, out / v.len() as f64));
        };
        if has_hex {
            let er_vals: Vec<f64> =
                er.map(|a| (0..a.len()).map(|i| a.get(i, 0)).zip(&hexes).filter(|(_, h)| h.is_some()).map(|(v, _)| v).collect()).unwrap_or_default();
            add("EdgeRatio", &er_vals, None);
            for (m, v) in hm.iter().zip(&hex_values) {
                add(m.name(), v, m.acceptable());
            }
        }
        if has_tet {
            let suffix = if has_hex { " (tetrahedra)" } else { "" };
            let er_vals: Vec<f64> =
                er.map(|a| (0..a.len()).map(|i| a.get(i, 0)).zip(&tets).filter(|(_, h)| h.is_some()).map(|(v, _)| v).collect()).unwrap_or_default();
            add(&format!("EdgeRatio{suffix}"), &er_vals, Some((1.0, 3.0)));
            for (m, v) in tm.iter().zip(&tet_values) {
                add(&format!("{}{suffix}", m.name()), v, m.acceptable());
            }
        }
        t.columns.insert(Array::new("Metric", 1, ArrayData::String(rows.iter().map(|r| r.0.clone()).collect())));
        for (k, name) in ["Min", "Max", "Mean", "AcceptableMin", "AcceptableMax", "CellsOutside", "FractionOutside"].iter().enumerate() {
            t.columns.insert(Array::scalars_f64(*name, rows.iter().map(|r| [r.1, r.2, r.3, r.4, r.5, r.6, r.7][k]).collect()));
        }
        return Ok(DataSet::Table(t));
    }
    let mut out = base;
    let cd = out.cell_data_mut().ok_or_else(|| anyhow!("no cell data"))?;
    let n = hexes.len();
    let mut arrays: std::collections::BTreeMap<&str, Vec<f64>> = std::collections::BTreeMap::new();
    if has_hex {
        for (m, v) in hm.iter().zip(hex_values) {
            arrays.insert(m.name(), v);
        }
    }
    if has_tet {
        for (m, v) in tm.iter().zip(tet_values) {
            let slot = arrays.entry(m.name()).or_insert_with(|| vec![f64::NAN; n]);
            for (i, x) in v.into_iter().enumerate() {
                if tets[i].is_some() {
                    slot[i] = x;
                }
            }
        }
    }
    for (name, v) in arrays {
        cd.insert(Array::scalars_f64(name, v));
    }
    Ok(out)
}

/// Extract Set: a side set's faces as a surface (with the owning element's block and element
/// variables as cell data), or a node set's nodes as points. `name` is the set's name.
pub fn extract_set(ds: &DataSet, name: &str) -> Result<DataSet> {
    let fd = ds.field_data();
    let list =
        || -> String { fd.iter().filter_map(|a| a.name.strip_prefix("SideSet ").or_else(|| a.name.strip_prefix("NodeSet "))).collect::<Vec<_>>().join(", ") };
    let m = HexMesh::from_dataset(ds).map_err(|e| anyhow!(e))?;
    if let Some(faces) = m.side_set(name) {
        let mut polys = Cells::new();
        let mut used: std::collections::BTreeMap<u32, u32> = Default::default();
        let mut pts = Vec::new();
        let (mut block, mut fields): (Vec<i32>, Vec<Vec<f64>>) = (Vec::new(), vec![Vec::new(); m.elem_fields.len()]);
        for (e, s) in faces {
            let ids: Vec<u32> = m
                .side_corners(*e as usize, *s)
                .iter()
                .map(|n| {
                    *used.entry(*n).or_insert_with(|| {
                        pts.extend_from_slice(&m.points[*n as usize]);
                        (pts.len() / 3 - 1) as u32
                    })
                })
                .collect();
            polys.push(CellType::Quad, &ids);
            block.push(m.block[*e as usize]);
            for (k, (_, v)) in m.elem_fields.iter().enumerate() {
                fields[k].push(v[*e as usize]);
            }
        }
        let mut pd = PolyData { points: Points::new_f64(pts), polys, ..Default::default() };
        pd.cell_data.insert(Array::new("ElementBlock", 1, ArrayData::I32(block)));
        for ((n, _), v) in m.elem_fields.iter().zip(fields) {
            pd.cell_data.insert(Array::scalars_f64(n.clone(), v));
        }
        return Ok(DataSet::PolyData(pd));
    }
    if let Some(nodes) = m.node_set(name) {
        let pts: Vec<f64> = nodes.iter().flat_map(|n| m.points[*n as usize]).collect();
        let mut verts = Cells::new();
        for i in 0..nodes.len() as u32 {
            verts.push(CellType::Vertex, &[i]);
        }
        let mut pd = PolyData { points: Points::new_f64(pts), verts, ..Default::default() };
        pd.point_data.insert(Array::new("NodeId", 1, ArrayData::I32(nodes.iter().map(|n| *n as i32).collect())));
        return Ok(DataSet::PolyData(pd));
    }
    bail!("no set named {name:?} (the mesh has: {})", list())
}

/// Plate Mesh: a plate of `cells`-sized hexahedra (one through the thickness) with an optional
/// circular or elliptical through-hole meshed by a conforming O-grid ring.
#[allow(clippy::too_many_arguments)]
pub fn plate_mesh(size: [f64; 3], h: f64, hole: &str, a: f64, b: f64, centre: Option<[f64; 2]>, angle: f64, element: &str) -> Result<DataSet> {
    use aurafx_mesh::specimens::{plate, Hole, PlateOptions};
    let hole = match hole {
        "Circle" => Some(Hole { ellipse: false, a, b: a, centre, angle: 0.0 }),
        "Ellipse" => Some(Hole { ellipse: true, a, b, centre, angle }),
        _ => None,
    };
    let mut m = plate(&PlateOptions { width: size[0], height: size[1], thickness: size[2], h, hole, ..Default::default() }).map_err(|e| anyhow!(e))?;
    m.elevate(nodes_per_element(element), &[], &|_, p| p);
    Ok(m.to_dataset())
}

/// Dog-bone Mesh: a flat tensile bar with shoulder fillets and an optional thickness profile.
pub fn dogbone_mesh(o: aurafx_mesh::specimens::DogboneOptions, element: &str) -> Result<DataSet> {
    let mut m = aurafx_mesh::specimens::dogbone(&o).map_err(|e| anyhow!(e))?;
    m.elevate(nodes_per_element(element), &[], &|_, p| p);
    Ok(m.to_dataset())
}

/// Sweep Quads: a surface of quadrilaterals swept into hexahedra along `direction`.
pub fn sweep(ds: &DataSet, direction: [f64; 3], distance: f64, layers: usize, ratio: f64, cluster: &str) -> Result<DataSet> {
    let pd = match ds {
        DataSet::PolyData(p) => p.clone(),
        other => crate::surface::extract_surface(other)?,
    };
    let pts: Vec<[f64; 3]> = (0..pd.points.len()).map(|i| pd.points.get(i)).collect();
    let mut quads = Vec::new();
    let mut blocks = Vec::new();
    let eb = pd.cell_data.get("ElementBlock");
    let nv = pd.verts.len() + pd.lines.len();
    for (i, (_, c)) in pd.polys.iter().enumerate() {
        if c.len() != 4 {
            bail!("the surface has a polygon with {} corners: only quadrilaterals can be swept", c.len());
        }
        quads.push([c[0], c[1], c[2], c[3]]);
        blocks.push(eb.map(|a| a.get(nv + i, 0) as i32).unwrap_or(1));
    }
    if quads.is_empty() {
        bail!("no quadrilaterals to sweep");
    }
    let div = Division { cells: layers.max(1), ratio, cluster: cluster.to_lowercase() };
    let m = aurafx_mesh::structured::sweep_quads(&pts, &quads, &blocks, direction, distance, &div).map_err(|e| anyhow!(e))?;
    Ok(m.to_dataset())
}

/// The hexahedral mesh of the bodies named by `mesh body` commands.
pub fn geometry_mesh(commands: &str) -> Result<DataSet> {
    let model = aurafx_mesh::Model::from_commands(commands, None)?;
    let m = aurafx_mesh::bodymesh::mesh_model(&model).map_err(|e| anyhow!(e))?;
    let mut ds = m.to_dataset();
    ds.field_data_mut_any().insert(Array::new("FaceNames", 1, ArrayData::String(model.faces.clone())));
    Ok(ds)
}

pub const OVERLAY_SMOOTH: &str = "Smooth";
pub const OVERLAY_STAIR: &str = "Stair-step";

/// Settings of the Overlay Mesh filter.
pub struct OverlayParams {
    pub array: String,
    pub cell: f64,
    pub mode: String,
    pub void: bool,
    pub periodic: bool,
    pub min_cells: usize,
    pub smoothing: usize,
    pub element: String,
    pub sharp: bool,
}

/// Overlay Mesh: hexahedra on a Cartesian grid over a labelled image (each label a block, 0
/// void) or a closed surface, with pillowed and smoothed interfaces (or whole cells).
pub fn overlay_mesh(ds: &DataSet, p: &OverlayParams) -> Result<DataSet> {
    use aurafx_mesh::overlay::*;
    let smooth = p.mode != OVERLAY_STAIR;
    let mut o = OverlayOptions {
        pillow: smooth,
        project: smooth,
        mesh_void: p.void,
        periodic: p.periodic,
        min_cells: p.min_cells,
        smoothing: p.smoothing,
        curves: p.sharp,
        ..Default::default()
    };
    let r = match ds {
        DataSet::ImageData(img) => {
            let v = crate::features::Vox::of(img);
            let a = img.cell_data.get(&p.array).ok_or_else(|| anyhow!("no cell array {} (choose the array of labels)", p.array))?;
            if a.ncomp != 1 {
                bail!("{} has {} components: the labels must be one number per cell", p.array, a.ncomp);
            }
            let labels: Vec<u32> = (0..a.len()).map(|i| a.get(i, 0).max(0.0) as u32).collect();
            let n = v.n;
            let hi: [f64; 3] = std::array::from_fn(|k| v.corner[k] + n[k] as f64 * v.sp[k]);
            o.lo = v.corner;
            o.hi = hi;
            o.cells = if p.cell > 0.0 { std::array::from_fn(|k| (((hi[k] - v.corner[k]) / p.cell).round() as usize).max(1)) } else { n };
            o.samples = if o.cells == n { 1 } else { 3 };
            let grains = p.array.eq_ignore_ascii_case("FeatureIds") || p.array.eq_ignore_ascii_case("grain_id");
            for l in labels.iter().copied().collect::<std::collections::BTreeSet<u32>>() {
                if l > 0 {
                    o.names.insert(l, if grains { format!("grain {l}") } else { format!("{} {l}", p.array) });
                }
            }
            let src = LabelSource { n, origin: v.corner, spacing: v.sp, labels };
            overlay(&src, &o).map_err(|e| anyhow!(e))?
        }
        DataSet::PolyData(_) => {
            // A CAD Model's (or Geometry's) surface keeps its faces: one side set per face, by name.
            let (tm, cad_names) = match aurafx_mesh::model::surface_mesh_with_faces(ds) {
                Some((m, names)) => (m, Some(names)),
                None => (aurafx_mesh::model::surface_mesh(ds)?, None),
            };
            let np = tm.n_patches;
            let shape = aurafx_mesh::shape::Shape::Surface { mesh: std::sync::Arc::new(tm), face: 1 };
            let (lo, hi) = shape.bounds();
            let size = if p.cell > 0.0 { p.cell } else { (0..3).map(|k| hi[k] - lo[k]).fold(0.0, f64::max) / 30.0 };
            o.lo = std::array::from_fn(|k| lo[k] - 1.5 * size);
            o.hi = std::array::from_fn(|k| hi[k] + 1.5 * size);
            o.cells = std::array::from_fn(|k| (((o.hi[k] - o.lo[k]) / size).round() as usize).max(1));
            o.names.insert(1, "solid".into());
            o.periodic = false;
            // One side set per patch of the surface between sharp edges.
            o.face_names = match &cad_names {
                // Face f of the surface is numbered 1 + f here: the first name is the empty one.
                Some(names) => std::iter::once(String::new()).chain((0..np as usize).map(|k| names.get(k).cloned().unwrap_or_else(|| format!("face {k}")))).collect(),
                None => std::iter::once(String::new()).chain((1..=np).map(|k| if np == 1 { "surface".to_string() } else { format!("surface {k}") })).collect(),
            };
            let src = ShapeSource { shapes: vec![(1, &shape)] };
            overlay(&src, &o).map_err(|e| anyhow!(e))?
        }
        _ => bail!("Overlay Mesh works on image data with a label array, or on a closed surface"),
    };
    let mut m = r.mesh;
    m.elevate(nodes_per_element(&p.element), &[], &|_, q| q);
    // Grains of a labelled volume with orientations: each element's grain and its mean
    // orientation, ready for orientation files.
    let grain_q: Option<Vec<Option<aurafx_crystal::Quat>>> = match ds {
        DataSet::ImageData(img) => crate::features::Orients::of(ds, None, crate::crystal::FROM_PHASES, None).ok().and_then(|o| {
            let a = img.cell_data.get(&p.array)?;
            let fid: Vec<i32> = (0..a.len()).map(|i| a.get(i, 0) as i32).collect();
            let nf = fid.iter().copied().max().unwrap_or(0).max(0) as usize + 1;
            Some(crate::features::average_quats(&o, &fid, nf))
        }),
        _ => None,
    };
    let grain_of: Vec<i32> = m.block.iter().map(|b| if m.block_names.get(b).is_some_and(|n| n == "void") { 0 } else { *b }).collect();
    let mut out = m.to_dataset();
    if let Some(gq) = grain_q {
        let eul: Vec<f64> =
            grain_of.iter().flat_map(|g| gq.get(*g as usize).copied().flatten().map(aurafx_crystal::quat_to_euler).unwrap_or([f64::NAN; 3])).collect();
        let rows: Vec<f64> = gq.iter().flat_map(|q| q.map(aurafx_crystal::quat_to_euler).unwrap_or([f64::NAN; 3])).collect();
        if let Some(cd) = out.cell_data_mut() {
            cd.insert(Array::new("grain_id", 1, ArrayData::I32(grain_of.clone())));
            cd.insert(Array::new("EulerAngles", 3, ArrayData::F64(eul)));
        }
        out.field_data_mut_any().insert(Array::new("GrainEulerAngles", 3, ArrayData::F64(rows)));
    }
    let fd = out.field_data_mut_any();
    fd.insert(Array::new("MinScaledJacobian", 1, ArrayData::F64(vec![r.min_sj])));
    fd.insert(Array::new("InvertedElements", 1, ArrayData::F64(vec![r.inverted as f64])));
    Ok(out)
}

/// The tetrahedral or hexahedral mesh of a dataset (with its blocks and sets).
enum AnyMesh {
    Hex(HexMesh),
    Tet(aurafx_mesh::tet::TetMesh),
}

fn any_mesh(ds: &DataSet) -> Result<AnyMesh> {
    match HexMesh::from_dataset(ds) {
        Ok(m) => Ok(AnyMesh::Hex(m)),
        Err(e1) => aurafx_mesh::tet::TetMesh::from_dataset(ds)
            .map(AnyMesh::Tet)
            .map_err(|e2| anyhow!("the mesh must be all hexahedra ({e1}) or all tetrahedra ({e2})")),
    }
}

/// Refine Mesh: every element into eight, `levels` times (hexahedra: HEX8 meshes).
pub fn refine_mesh(ds: &DataSet, levels: usize) -> Result<DataSet> {
    let mut m = any_mesh(ds)?;
    for _ in 0..levels {
        m = match m {
            AnyMesh::Hex(h) => AnyMesh::Hex(h.refine().map_err(|e| anyhow!(e))?),
            AnyMesh::Tet(t) => AnyMesh::Tet(t.refine()),
        };
    }
    Ok(match m {
        AnyMesh::Hex(h) => h.to_dataset(),
        AnyMesh::Tet(t) => t.to_dataset(),
    })
}

pub const TO_TETS: &str = "Hexahedra to tetrahedra";
pub const TO_HEXES: &str = "Tetrahedra to hexahedra";

/// Convert Elements: hexahedra into 12 tetrahedra each, or tetrahedra into 4 hexahedra each.
pub fn convert_elements(ds: &DataSet, to: &str) -> Result<DataSet> {
    match (any_mesh(ds)?, to) {
        (AnyMesh::Hex(h), TO_TETS) => Ok(aurafx_mesh::tet::hexes_to_tets(&h).map_err(|e| anyhow!(e))?.to_dataset()),
        (AnyMesh::Tet(t), TO_HEXES) => Ok(t.to_hexes().to_dataset()),
        (AnyMesh::Hex(_), _) => bail!("the mesh is already hexahedral"),
        (AnyMesh::Tet(_), _) => bail!("the mesh is already tetrahedral"),
    }
}

/// Smooth Mesh: the inner nodes of a hexahedral or tetrahedral mesh moved by one of the
/// schemes, the boundary and the nodes of `fixed_set` held.
pub fn smooth_mesh(ds: &DataSet, scheme: &str, iterations: usize, fixed_set: &str) -> Result<DataSet> {
    let (mut points, cells, fixed, rebuild): (Vec<[f64; 3]>, Vec<Vec<u32>>, std::collections::HashSet<u32>, Box<dyn Fn(Vec<[f64; 3]>) -> DataSet>) =
        match any_mesh(ds)? {
            AnyMesh::Hex(h) => {
                if h.npe != 8 {
                    bail!("smooth a HEX8 mesh (elevate afterwards)");
                }
                let fixed = if fixed_set.is_empty() {
                    Default::default()
                } else {
                    h.node_set(fixed_set).ok_or_else(|| anyhow!("no node set {fixed_set}"))?.iter().copied().collect()
                };
                let cells = (0..h.n_elements()).map(|e| h.cell(e).to_vec()).collect();
                let pts = h.points.clone();
                (
                    pts,
                    cells,
                    fixed,
                    Box::new(move |p| {
                        let mut m = h.clone();
                        m.points = p;
                        m.to_dataset()
                    }),
                )
            }
            AnyMesh::Tet(t) => {
                let fixed = if fixed_set.is_empty() {
                    Default::default()
                } else {
                    t.node_set(fixed_set).ok_or_else(|| anyhow!("no node set {fixed_set}"))?.iter().copied().collect()
                };
                let cells = t.tets.iter().map(|c| c.to_vec()).collect();
                let pts = t.points.clone();
                (
                    pts,
                    cells,
                    fixed,
                    Box::new(move |p| {
                        let mut m = t.clone();
                        m.points = p;
                        m.to_dataset()
                    }),
                )
            }
        };
    let c3 = aurafx_mesh::improve::Cells3 { cells };
    let min_sj = aurafx_mesh::improve::smooth(&mut points, &c3, scheme, iterations, &fixed);
    let mut out = rebuild(points);
    out.field_data_mut_any().insert(Array::scalars_f64("MinScaledJacobian", vec![min_sj]));
    Ok(out)
}

/// Boundary Layers: thin layers of hexahedra under the faces of a side set.
pub fn boundary_layers(ds: &DataSet, side_set: &str, layers: usize, first: f64, growth: f64) -> Result<DataSet> {
    let m = HexMesh::from_dataset(ds).map_err(|e| anyhow!(e))?;
    let faces = m.side_set(side_set).ok_or_else(|| anyhow!("no side set {side_set}"))?.clone();
    Ok(aurafx_mesh::improve::boundary_layers(&m, &faces, layers, first, growth).map_err(|e| anyhow!(e))?.to_dataset())
}

/// Tet Mesh: tetrahedra filling a closed surface (isosurface stuffing at `cell`, 0: a
/// thirtieth of its largest size), a side set per patch of the surface between creases.
pub fn tet_mesh(ds: &DataSet, cell: f64) -> Result<DataSet> {
    let tm = aurafx_mesh::model::surface_mesh(ds)?;
    let np = tm.n_patches;
    let shape = aurafx_mesh::shape::Shape::Surface { mesh: std::sync::Arc::new(tm), face: 1 };
    let (lo, hi) = shape.bounds();
    let h = if cell > 0.0 { cell } else { (0..3).map(|k| hi[k] - lo[k]).fold(0.0, f64::max) / 30.0 };
    let lo: [f64; 3] = std::array::from_fn(|a| lo[a] - 1.5 * h);
    let hi: [f64; 3] = std::array::from_fn(|a| hi[a] + 1.5 * h);
    let mut m = aurafx_mesh::stuffing::stuff(&|p| shape.eval(p).0, lo, hi, h).map_err(|e| anyhow!(e))?;
    let mut sets: std::collections::BTreeMap<u32, Vec<(u32, u8)>> = Default::default();
    for (e, s) in m.boundary_faces() {
        let c = m.side_corners(e as usize, s);
        let cen: [f64; 3] = std::array::from_fn(|a| c.iter().map(|k| m.points[*k as usize][a]).sum::<f64>() / 3.0);
        sets.entry(shape.eval(cen).1).or_default().push((e, s));
    }
    for (f, faces) in sets {
        m.side_sets.push((if np == 1 { "surface".to_string() } else { format!("surface {f}") }, faces));
    }
    Ok(m.to_dataset())
}

/// Refine Region: the tetrahedra whose centre lies in the box `lo`..`hi` (or, with `array`,
/// whose cell value is at least `value`) split into eight, their neighbours closed.
pub fn refine_region(ds: &DataSet, lo: [f64; 3], hi: [f64; 3], array: &str, value: f64) -> Result<DataSet> {
    let m = aurafx_mesh::tet::TetMesh::from_dataset(ds).map_err(|e| anyhow!("{e} (Refine Region works on tetrahedral meshes; Convert Elements first)"))?;
    let marked: Vec<bool> = if array.is_empty() {
        m.tets
            .iter()
            .map(|t| {
                let c: [f64; 3] = std::array::from_fn(|a| t.iter().map(|k| m.points[*k as usize][a]).sum::<f64>() / 4.0);
                (0..3).all(|a| c[a] >= lo[a] && c[a] <= hi[a])
            })
            .collect()
    } else {
        let a = ds.cell_data().and_then(|c| c.get(array)).ok_or_else(|| anyhow!("no cell array {array}"))?;
        (0..m.tets.len()).map(|i| a.get(i, 0) >= value).collect()
    };
    if !marked.iter().any(|x| *x) {
        bail!("no element to refine");
    }
    Ok(aurafx_mesh::stuffing::refine_marked(&m, &marked, &|p| p).to_dataset())
}

// ------------------------------------------------------------------ grains on meshes

fn corner_count(t: CellType, n: usize) -> usize {
    match t {
        CellType::QuadraticHexahedron | CellType::TriquadraticHexahedron | CellType::BiquadraticQuadraticHexahedron => 8,
        CellType::QuadraticTetra => 4,
        CellType::QuadraticWedge => 6,
        CellType::QuadraticPyramid => 5,
        _ => n,
    }
}

/// Element centres: the mean of each element's corner nodes.
fn centroids(ds: &DataSet) -> Result<Vec<[f64; 3]>> {
    let ug = ds.to_unstructured().ok_or_else(|| anyhow!("Assign Grains works on meshes"))?;
    Ok(ug
        .cells
        .iter()
        .map(|(t, ids)| {
            let k = corner_count(t, ids.len()).min(ids.len()).max(1);
            let mut c = [0.0; 3];
            for id in &ids[..k] {
                let p = ug.points.get(*id as usize);
                for a in 0..3 {
                    c[a] += p[a];
                }
            }
            c.map(|x| x / k as f64)
        })
        .collect())
}

/// Nearest of a set of points (with optional power weights) by buckets.
struct Nearest {
    pts: Vec<[f64; 3]>,
    w: Vec<f64>,
    lo: [f64; 3],
    bw: [f64; 3],
    nb: [usize; 3],
    bucket: Vec<Vec<u32>>,
    wmax: f64,
}

impl Nearest {
    fn new(pts: Vec<[f64; 3]>, w: Vec<f64>) -> Nearest {
        let (mut lo, mut hi) = ([f64::INFINITY; 3], [f64::NEG_INFINITY; 3]);
        for p in &pts {
            for a in 0..3 {
                lo[a] = lo[a].min(p[a]);
                hi[a] = hi[a].max(p[a]);
            }
        }
        let ext: [f64; 3] = std::array::from_fn(|a| (hi[a] - lo[a]).max(1e-12));
        let per = (pts.len() as f64).cbrt().max(1.0);
        let nb: [usize; 3] = std::array::from_fn(|a| ((per * ext[a] / ext.iter().cloned().fold(0.0, f64::max)).ceil() as usize).clamp(1, 128));
        let bw: [f64; 3] = std::array::from_fn(|a| ext[a] / nb[a] as f64 * (1.0 + 1e-9));
        let mut bucket = vec![Vec::new(); nb[0] * nb[1] * nb[2]];
        for (i, p) in pts.iter().enumerate() {
            let b: [usize; 3] = std::array::from_fn(|a| (((p[a] - lo[a]) / bw[a]) as usize).min(nb[a] - 1));
            bucket[b[0] + nb[0] * (b[1] + nb[1] * b[2])].push(i as u32);
        }
        let wmax = w.iter().cloned().fold(0.0, f64::max);
        Nearest { pts, w, lo, bw, nb, bucket, wmax }
    }
    /// Index of the point of least |x - p|^2 - w (ties: the lowest index).
    fn query(&self, x: [f64; 3]) -> usize {
        let pb: [i64; 3] = std::array::from_fn(|a| (((x[a] - self.lo[a]) / self.bw[a]).floor() as i64).clamp(0, self.nb[a] as i64 - 1));
        let mut best = (f64::INFINITY, usize::MAX);
        let bmin = self.bw.iter().cloned().fold(f64::INFINITY, f64::min);
        for ring in 0i64.. {
            for dz in -ring..=ring {
                for dy in -ring..=ring {
                    for dx in -ring..=ring {
                        if dx.abs().max(dy.abs()).max(dz.abs()) != ring {
                            continue;
                        }
                        let q = [pb[0] + dx, pb[1] + dy, pb[2] + dz];
                        if (0..3).any(|a| q[a] < 0 || q[a] >= self.nb[a] as i64) {
                            continue;
                        }
                        for &i in &self.bucket[q[0] as usize + self.nb[0] * (q[1] as usize + self.nb[1] * q[2] as usize)] {
                            let p = self.pts[i as usize];
                            let d = (0..3).map(|a| (x[a] - p[a]).powi(2)).sum::<f64>() - self.w.get(i as usize).copied().unwrap_or(0.0);
                            if d < best.0 || (d == best.0 && (i as usize) < best.1) {
                                best = (d, i as usize);
                            }
                        }
                    }
                }
            }
            // Beyond this ring every point is at least `ring` bucket widths away (the query
            // point may lie outside the buckets: measure from its clamped bucket).
            let outside: f64 =
                (0..3).map(|a| ((self.lo[a] - x[a]).max(0.0).max(x[a] - (self.lo[a] + self.nb[a] as f64 * self.bw[a]))).powi(2)).sum::<f64>().sqrt();
            let reach = (ring as f64 * bmin + outside).max(0.0);
            let all = (0..3).all(|a| ring as usize >= self.nb[a]);
            if all || (best.1 != usize::MAX && reach * reach - self.wmax > best.0 + 1e-12) {
                break;
            }
        }
        best.1
    }
}

pub const GRAINS_VORONOI: &str = "Voronoi";
pub const GRAINS_POINTS: &str = "Nearest point";
pub const GRAINS_VOLUME: &str = "From a volume";

pub struct AssignParams {
    pub method: String,
    pub grains: usize,
    pub size_sigma: f64,
    pub orientations: String,
    pub spread_deg: f64,
    pub seed: u64,
    pub grain_array: String,
    pub blocks: bool,
    pub boundaries: bool,
}

/// Assign Grains: a grain id (`grain_id`) and an orientation (`EulerAngles`, radians) for
/// every element of a mesh, from Voronoi (or Laguerre) seeds placed at random, from the
/// nearest point of a point set or table carrying grain ids (and orientations), or from the
/// feature of a microstructure volume at the element's centre. Optionally one element block
/// per grain and a side set on the grain boundaries; the grains' orientations are field data
/// (`GrainEulerAngles`, one row per grain id) for orientation files.
pub fn assign_grains(mesh: &DataSet, source: Option<&DataSet>, p: &AssignParams) -> Result<DataSet> {
    use aurafx_crystal::{self as cx, Quat};
    let cen = centroids(mesh)?;
    if cen.is_empty() {
        bail!("the mesh has no elements");
    }
    let mut seeds_out: Option<Vec<[f64; 3]>> = None;
    let (gid, mut grain_q): (Vec<i32>, std::collections::BTreeMap<i32, Quat>) = match p.method.as_str() {
        GRAINS_POINTS => {
            let s = source.ok_or_else(|| anyhow!("Nearest point needs a second input: points or a table (x, y, z and a grain id column)"))?;
            let (pts, cols): (Vec<[f64; 3]>, &Attributes) = match s {
                DataSet::Table(t) => {
                    let col = |n: &str| t.columns.get(n).ok_or_else(|| anyhow!("the table has no column {n}"));
                    let (x, y) = (col("x")?, col("y")?);
                    let z = t.columns.get("z");
                    ((0..x.len()).map(|i| [x.get(i, 0), y.get(i, 0), z.map(|a| a.get(i, 0)).unwrap_or(0.0)]).collect(), &t.columns)
                }
                d => {
                    let pts = d.points().ok_or_else(|| anyhow!("the second input has no points"))?;
                    ((0..pts.len()).map(|i| pts.get(i)).collect(), d.point_data().ok_or_else(|| anyhow!("no point data"))?)
                }
            };
            let g = cols.get(&p.grain_array).ok_or_else(|| anyhow!("the points have no {} array", p.grain_array))?;
            // Orientations: EulerAngles (radians) or phi1/Phi/phi2 columns (degrees).
            let eul: Option<Box<dyn Fn(usize) -> Quat>> = if let Some(e) = cols.get("EulerAngles").filter(|e| e.ncomp == 3) {
                Some(Box::new(move |i| cx::euler_to_quat([e.get(i, 0), e.get(i, 1), e.get(i, 2)])))
            } else if let (Some(a), Some(b), Some(c)) = (cols.get("phi1"), cols.get("Phi"), cols.get("phi2")) {
                Some(Box::new(move |i| cx::euler_to_quat([a.get(i, 0).to_radians(), b.get(i, 0).to_radians(), c.get(i, 0).to_radians()])))
            } else {
                None
            };
            let near = Nearest::new(pts, Vec::new());
            let pick: Vec<usize> = cen.iter().map(|c| near.query(*c)).collect();
            let gid: Vec<i32> = pick.iter().map(|i| g.get(*i, 0) as i32).collect();
            let mut q = std::collections::BTreeMap::new();
            if let Some(f) = &eul {
                for (k, i) in pick.iter().enumerate() {
                    q.entry(gid[k]).or_insert_with(|| f(*i));
                }
            }
            (gid, q)
        }
        GRAINS_VOLUME => {
            let s = source.ok_or_else(|| anyhow!("From a volume needs a second input: a microstructure with FeatureIds"))?;
            let img = crate::features::image(s)?;
            let v = crate::features::Vox::of(img);
            let ids = img.cell_data.get(&p.grain_array).ok_or_else(|| anyhow!("the volume has no {} array", p.grain_array))?;
            let gid: Vec<i32> = cen
                .iter()
                .map(|c| {
                    let ix: [usize; 3] =
                        std::array::from_fn(|a| if v.n[a] <= 1 { 0 } else { (((c[a] - v.corner[a]) / v.sp[a]).floor().max(0.0) as usize).min(v.n[a] - 1) });
                    ids.get(ix[0] + v.n[0] * (ix[1] + v.n[1] * ix[2]), 0) as i32
                })
                .collect();
            let mut q = std::collections::BTreeMap::new();
            if let Ok(o) = crate::features::Orients::of(s, None, crate::crystal::FROM_PHASES, None) {
                let fid: Vec<i32> = (0..ids.len()).map(|i| ids.get(i, 0) as i32).collect();
                let nf = fid.iter().copied().max().unwrap_or(0).max(0) as usize + 1;
                for (f, a) in crate::features::average_quats(&o, &fid, nf).into_iter().enumerate() {
                    if let Some(a) = a {
                        q.insert(f as i32, a);
                    }
                }
            }
            (gid, q)
        }
        _ => {
            // Voronoi / Laguerre seeds in the mesh's bounding box.
            let (mut lo, mut hi) = ([f64::INFINITY; 3], [f64::NEG_INFINITY; 3]);
            for c in &cen {
                for a in 0..3 {
                    lo[a] = lo[a].min(c[a]);
                    hi[a] = hi[a].max(c[a]);
                }
            }
            let mut r = crate::synthetic::Rng::new(p.seed);
            use crate::synthetic::Draws;
            let n = p.grains.max(1);
            let seeds: Vec<[f64; 3]> = (0..n).map(|_| std::array::from_fn(|a| r.uniform(lo[a], hi[a]))).collect();
            let vol: f64 = (0..3).map(|a| (hi[a] - lo[a]).max(1e-12)).product();
            let spacing = (vol / n as f64).cbrt();
            let w: Vec<f64> = (0..n)
                .map(|_| if p.size_sigma > 0.0 { (0.5 * spacing * (p.size_sigma * r.std_normal().clamp(-3.0, 3.0)).exp()).powi(2) } else { 0.0 })
                .collect();
            seeds_out = Some(seeds.clone());
            let near = Nearest::new(seeds, w);
            let gid: Vec<i32> = cen.iter().map(|c| near.query(*c) as i32 + 1).collect();
            let orients = crate::synthetic::grain_orientations_pub(n, &p.orientations, p.spread_deg.to_radians(), &mut r);
            let q = (1..=n as i32).zip(orients).collect();
            (gid, q)
        }
    };
    // Every grain present gets an orientation (random when the source had none).
    let mut r = crate::synthetic::Rng::new(p.seed ^ 0x5eed);
    for g in gid.iter() {
        grain_q.entry(*g).or_insert_with(|| crate::synthetic::random_quat(&mut r));
    }
    let mut out = mesh.clone();
    if p.blocks || p.boundaries {
        // Through the mesh types that know blocks and sides.
        out = match any_mesh(mesh)? {
            AnyMesh::Hex(mut h) => {
                if p.blocks {
                    h.block = gid.clone();
                    h.block_names = gid.iter().map(|g| (*g, format!("grain {g}"))).collect();
                }
                if p.boundaries {
                    let mut faces: std::collections::HashMap<Vec<u32>, Vec<(u32, u8)>> = Default::default();
                    for e in 0..h.n_elements() {
                        for s in 1..=6u8 {
                            let mut k = h.side_corners(e, s).to_vec();
                            k.sort_unstable();
                            faces.entry(k).or_default().push((e as u32, s));
                        }
                    }
                    let mut set: Vec<(u32, u8)> = faces
                        .values()
                        .filter(|f| f.len() == 2 && gid[f[0].0 as usize] != gid[f[1].0 as usize])
                        .map(|f| if gid[f[0].0 as usize] < gid[f[1].0 as usize] { f[0] } else { f[1] })
                        .collect();
                    set.sort_unstable();
                    let nodes: Vec<u32> = set.iter().flat_map(|(e, s)| h.side_corners(*e as usize, *s)).collect();
                    h.add_side_set("grain boundaries", set);
                    h.add_node_set("grain boundaries", nodes);
                }
                h.to_dataset()
            }
            AnyMesh::Tet(mut t) => {
                if p.blocks {
                    t.block = gid.clone();
                    t.block_names = gid.iter().map(|g| (*g, format!("grain {g}"))).collect();
                }
                if p.boundaries {
                    let mut faces: std::collections::HashMap<Vec<u32>, Vec<(u32, u8)>> = Default::default();
                    for e in 0..t.n_elements() {
                        for s in 1..=4u8 {
                            let mut k = t.side_corners(e, s).to_vec();
                            k.sort_unstable();
                            faces.entry(k).or_default().push((e as u32, s));
                        }
                    }
                    let mut set: Vec<(u32, u8)> = faces
                        .values()
                        .filter(|f| f.len() == 2 && gid[f[0].0 as usize] != gid[f[1].0 as usize])
                        .map(|f| if gid[f[0].0 as usize] < gid[f[1].0 as usize] { f[0] } else { f[1] })
                        .collect();
                    set.sort_unstable();
                    let mut nodes: Vec<u32> = set.iter().flat_map(|(e, s)| t.side_corners(*e as usize, *s)).collect();
                    nodes.sort_unstable();
                    nodes.dedup();
                    t.side_sets.push(("grain boundaries".into(), set));
                    t.node_sets.push(("grain boundaries".into(), nodes));
                }
                t.to_dataset()
            }
        };
    }
    // The mesh types rebuild the dataset from the elements; the input's point arrays
    // (displacements, results) come along since the points keep their order.
    if p.blocks || p.boundaries {
        if let (Some(src), Some(dst)) = (mesh.point_data(), out.point_data_mut()) {
            if dst.iter().next().is_none() {
                for a in src.iter() {
                    dst.insert(a.clone());
                }
            }
        }
    }
    let cd = out.cell_data_mut().ok_or_else(|| anyhow!("the mesh cannot hold cell arrays"))?;
    cd.insert(Array::new("grain_id", 1, ArrayData::I32(gid.clone())));
    let eul: Vec<f64> = gid.iter().flat_map(|g| cx::quat_to_euler(grain_q[g])).collect();
    cd.insert(Array::new("EulerAngles", 3, ArrayData::F64(eul)));
    let gmax = grain_q.keys().copied().max().unwrap_or(0).max(0) as usize;
    let rows: Vec<f64> = (0..=gmax).flat_map(|g| grain_q.get(&(g as i32)).map(|q| cx::quat_to_euler(*q)).unwrap_or([f64::NAN; 3])).collect();
    let fd = out.field_data_mut_any();
    fd.insert(Array::new("GrainEulerAngles", 3, ArrayData::F64(rows)));
    fd.insert(Array::scalars_f64("Grains", vec![grain_q.len() as f64]));
    if let Some(s) = seeds_out {
        let rows: Vec<f64> = std::iter::once([f64::NAN; 3]).chain(s).flatten().collect();
        fd.insert(Array::new("GrainSeeds", 3, ArrayData::F64(rows)));
    }
    Ok(out)
}
