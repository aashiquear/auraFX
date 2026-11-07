//! Structured hexahedral meshes (box, inclusions in a box), elevation to HEX20/HEX27 and the
//! hexahedron quality metrics.

use anyhow::{anyhow, bail, Result};
use aurafx_data::*;
use aurafx_mesh::hex::HexMesh;
use aurafx_mesh::inclusions::{self as inc, StarShape};
use aurafx_mesh::quality::{hex_metric, HexMetric};
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
    let spec = inc::PackingSpec { count: p.count, radius_mean: p.radius_mean, radius_std: p.radius_std, box_ratio: p.box_ratio, gap_cells: p.gap_cells, wall_cells: p.wall_cells, ..Default::default() };
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
                CellType::Hexahedron | CellType::QuadraticHexahedron | CellType::TriquadraticHexahedron | CellType::BiquadraticQuadraticHexahedron => Some([0, 1, 2, 3, 4, 5, 6, 7]),
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

/// Mesh Quality: the generic measures of every cell plus, for hexahedra, the full metric set
/// (NaN for other cells); or, with `summary`, a table of each metric over the hexahedra: its
/// minimum, maximum and mean, the acceptable range and how many cells fall outside it.
pub fn mesh_quality(ds: &DataSet, summary: bool) -> Result<DataSet> {
    let base = crate::geometry::mesh_quality(ds)?;
    let hexes = hex_corners(&base)?;
    let vols: Vec<f64> = hexes.iter().flatten().map(|p| hex_metric(HexMetric::Volume, p, 0.0)).collect();
    let avg = if vols.is_empty() { 0.0 } else { vols.iter().sum::<f64>() / vols.len() as f64 };
    let metrics = hex_metrics();
    let values: Vec<Vec<f64>> = metrics.iter().map(|m| hexes.iter().map(|h| h.as_ref().map(|p| hex_metric(*m, p, avg)).unwrap_or(f64::NAN)).collect()).collect();
    if summary {
        let mut t = Table::default();
        let mut rows: Vec<(String, f64, f64, f64, f64, f64, f64, f64)> = Vec::new();
        let er = base.cell_data().and_then(|c| c.get("EdgeRatio"));
        let er_vals: Vec<f64> = er.map(|a| (0..a.len()).map(|i| a.get(i, 0)).zip(&hexes).filter(|(_, h)| h.is_some()).map(|(v, _)| v).collect()).unwrap_or_default();
        let mut add = |name: &str, v: &[f64], range: Option<(f64, f64)>| {
            let v: Vec<f64> = v.iter().copied().filter(|x| !x.is_nan()).collect();
            if v.is_empty() {
                return;
            }
            let (mn, mx) = v.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), x| (a.min(*x), b.max(*x)));
            let mean = v.iter().sum::<f64>() / v.len() as f64;
            let (lo, hi) = range.unwrap_or((f64::NAN, f64::NAN));
            // A little slack for rounding (a perfect cube scores 1 plus or minus an ulp).
            let out = if range.is_some() { v.iter().filter(|x| **x < lo - 1e-9 * lo.abs().max(1.0) || **x > hi + 1e-9 * hi.abs().max(1.0)).count() as f64 } else { f64::NAN };
            rows.push((name.to_string(), mn, mx, mean, lo, hi, out, out / v.len() as f64));
        };
        add("EdgeRatio", &er_vals, None);
        for (m, v) in metrics.iter().zip(&values) {
            add(m.name(), v, m.acceptable());
        }
        t.columns.insert(Array::new("Metric", 1, ArrayData::String(rows.iter().map(|r| r.0.clone()).collect())));
        for (k, name) in ["Min", "Max", "Mean", "AcceptableMin", "AcceptableMax", "CellsOutside", "FractionOutside"].iter().enumerate() {
            t.columns.insert(Array::scalars_f64(*name, rows.iter().map(|r| [r.1, r.2, r.3, r.4, r.5, r.6, r.7][k]).collect()));
        }
        return Ok(DataSet::Table(t));
    }
    let mut out = base;
    if hexes.iter().any(|h| h.is_some()) {
        let cd = out.cell_data_mut().ok_or_else(|| anyhow!("no cell data"))?;
        for (m, v) in metrics.iter().zip(values) {
            cd.insert(Array::scalars_f64(m.name(), v));
        }
    }
    Ok(out)
}

/// Extract Set: a side set's faces as a surface (with the owning element's block and element
/// variables as cell data), or a node set's nodes as points. `name` is the set's name.
pub fn extract_set(ds: &DataSet, name: &str) -> Result<DataSet> {
    let fd = ds.field_data();
    let list = || -> String {
        fd.iter().filter_map(|a| a.name.strip_prefix("SideSet ").or_else(|| a.name.strip_prefix("NodeSet "))).collect::<Vec<_>>().join(", ")
    };
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
}

/// Overlay Mesh: hexahedra on a Cartesian grid over a labelled image (each label a block, 0
/// void) or a closed surface, with pillowed and smoothed interfaces (or whole cells).
pub fn overlay_mesh(ds: &DataSet, p: &OverlayParams) -> Result<DataSet> {
    use aurafx_mesh::overlay::*;
    let smooth = p.mode != OVERLAY_STAIR;
    let mut o = OverlayOptions { pillow: smooth, project: smooth, mesh_void: p.void, periodic: p.periodic, min_cells: p.min_cells, smoothing: p.smoothing, ..Default::default() };
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
            let tm = aurafx_mesh::model::surface_mesh(ds)?;
            let shape = aurafx_mesh::shape::Shape::Surface { mesh: std::sync::Arc::new(tm), face: 1 };
            let (lo, hi) = shape.bounds();
            let size = if p.cell > 0.0 { p.cell } else { (0..3).map(|k| hi[k] - lo[k]).fold(0.0, f64::max) / 30.0 };
            o.lo = std::array::from_fn(|k| lo[k] - 1.5 * size);
            o.hi = std::array::from_fn(|k| hi[k] + 1.5 * size);
            o.cells = std::array::from_fn(|k| (((o.hi[k] - o.lo[k]) / size).round() as usize).max(1));
            o.names.insert(1, "solid".into());
            o.periodic = false;
            o.face_names = vec![String::new(), "surface".into()];
            let src = ShapeSource { shapes: vec![(1, &shape)] };
            overlay(&src, &o).map_err(|e| anyhow!(e))?
        }
        _ => bail!("Overlay Mesh works on image data with a label array, or on a closed surface"),
    };
    let mut m = r.mesh;
    m.elevate(nodes_per_element(&p.element), &[], &|_, q| q);
    let mut out = m.to_dataset();
    let fd = out.field_data_mut_any();
    fd.insert(Array::new("MinScaledJacobian", 1, ArrayData::F64(vec![r.min_sj])));
    fd.insert(Array::new("InvertedElements", 1, ArrayData::F64(vec![r.inverted as f64])));
    Ok(out)
}
