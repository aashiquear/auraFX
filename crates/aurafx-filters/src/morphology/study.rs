//! Models run many times: named starting points for the models, a model followed while it
//! forms, and a model run over sizes, values of a setting and seeds to see where it
//! percolates and how its volume fraction scatters.

use super::measure::{spanning, specific_surface};
use super::*;
use crate::params::*;
use crate::registry::FilterDef;
use anyhow::{anyhow, bail, Result};

/// Settings written as text, `key = value; key = value`, over the standard ones of a model.
/// A key is the name of a setting in Python or its label; lists are separated by spaces.
pub fn params_from_text(def: &FilterDef, text: &str) -> Result<Params> {
    let mut p = def.default_params();
    for part in text.split([';', '\n']).map(str::trim).filter(|s| !s.is_empty()) {
        let (key, value) = part.split_once('=').ok_or_else(|| anyhow!("cannot read \"{part}\": write settings as name = value; name = value"))?;
        let (key, value) = (key.trim(), value.trim());
        let spec = def.params.iter().find(|s| s.key == key || s.label.eq_ignore_ascii_case(key)).ok_or_else(|| {
            anyhow!("{} has no setting {key} (it has: {})", def.label, def.params.iter().map(|s| s.key.as_str()).collect::<Vec<_>>().join(", "))
        })?;
        let number = |t: &str| t.parse::<f64>().map_err(|_| anyhow!("{key}: cannot read the number {t}"));
        let list = |t: &str| t.split([' ', ',']).filter(|s| !s.is_empty()).map(number).collect::<Result<Vec<f64>>>();
        let v = match &spec.kind {
            ParamKind::Bool => ParamValue::Bool(matches!(value.to_ascii_lowercase().as_str(), "true" | "yes" | "on" | "1")),
            ParamKind::Int { .. } => ParamValue::Int(number(value)?.round() as i64),
            ParamKind::Float { .. } => ParamValue::Float(number(value)?),
            ParamKind::Floats => ParamValue::Floats(list(value)?),
            ParamKind::Vec3 => {
                let l = list(value)?;
                if l.len() != 3 {
                    bail!("{key}: three numbers");
                }
                ParamValue::Vec3([l[0], l[1], l[2]])
            }
            ParamKind::Choice(options) => {
                let found =
                    options.iter().find(|o| o.eq_ignore_ascii_case(value)).ok_or_else(|| anyhow!("{key}: {value} is not one of {}", options.join(", ")))?;
                ParamValue::Str(found.clone())
            }
            _ => ParamValue::Str(value.to_string()),
        };
        p.set(&spec.key, v);
    }
    Ok(p)
}

/// Starting points: a name, the model, its settings and what it stands for.
pub const TEMPLATES: &[(&str, &str, &str, &str)] = &[
    (
        "Trabecular bone",
        "open_cell_foam",
        "fraction = 0.2; anisotropy = 1.6; node_ratio = 1.5; relaxation = 5",
        "Struts on elongated cells, thick at the nodes.",
    ),
    ("Open-cell metal foam", "level_set_foam", "fraction = 0.12", "Struts of three-sided section that thicken towards the nodes."),
    ("Closed-cell polymer foam", "level_set_foam", "fraction = 0.15; wall = 0.6; curvature = 0.5", "Cells closed by thin walls, hollow strut sides."),
    (
        "Carbon black in rubber",
        "clustered_spheres",
        "aggregate_radius = 10; aggregate_fraction = 0.3; radius = 2; local_fraction = 0.4; aggregate_phase = false",
        "Small particles gathered in aggregates.",
    ),
    (
        "Short glass fibre composite",
        "hard_fibres",
        "radius = 1.5; length = 30; directions = Beta law about Z; direction_shape = 0.3; fraction = 0.1",
        "Fibres that do not overlap, mostly along Z.",
    ),
    (
        "Nonwoven mat",
        "curved_fibres",
        "directions = Girdle about the XY plane; direction_shape = 10; persistence = 40; length = 80; fraction = 0.12",
        "Long curved fibres lying near a plane.",
    ),
    ("Paper and long-fibre mats", "poisson_line_fibres", "directions = In the XY plane; fraction = 0.2", "Fibres without ends in a plane."),
    (
        "Sintered powder",
        "hard_spheres",
        "method = Dense packing; fraction = 0.58; radius = 6; radius_sigma = 0.15; gap = 0",
        "A dense packing of spheres of several sizes.",
    ),
    ("Porous rock", "overlapping_spheres", "fraction = 0.8; radius = 5; radius_sigma = 0.3", "Grains that overlap, the pores between them."),
    ("Polymer blend", "gaussian_field_medium", "fraction = 0.5; length = 6", "Two interwoven phases in equal shares."),
    ("Partly recrystallized metal", "nucleation_growth_grains", "time = 12", "New grains growing in the old material."),
    ("Bone scaffold", "triply_periodic_surface", "surface = Gyroid; fraction = 0.25; cell_size = 32", "A gyroid sheet to print."),
    ("Printed truss", "strut_lattice", "cell = Octet; fraction = 0.15; cell_size = 32", "An octet truss at a relative density of 0.15."),
    ("Softwood", "honeycomb", "arrangement = Wood; cells = 128 96 48; cell = 8; wall = 1.5; ring = 128; rays = 0.08", "Growth rings and rays."),
    ("Cortical bone", "haversian_bone", "", "Osteons, cement lines and canals."),
    ("Polygonal nacre", "nacre", "arrangement = Polygons", "Polygonal tablets in a thin mortar."),
    ("Crustacean shell", "twisted_plywood", "pitch = 15", "Layers of fibres that turn from layer to layer."),
    ("Gel network", "delaunay_fibre_network", "regularity = 0.5; fraction = 0.05", "Thin fibres joined at nodes."),
    ("Soot aggregate", "aggregation_cluster", "seeds = 1; fraction = 0.02", "One branched fractal cluster."),
    ("Vascular tree", "branching_network", "", "A tree that thickens from the tips to the root."),
    ("Annealed metal", "voronoi_grains", "grains = 200; realism = 0.6", "Equiaxed grains with spread sizes and relaxed boundaries."),
    ("Drawn wire", "voronoi_grains", "orientations = <111> along Z; spread = 8; realism = 0.4", "Grains with a fibre texture along the wire."),
    ("Even grains", "voronoi_grains", "stealth = 0.4", "Grains without order, nearly as even in size as in a crystal."),
    ("Faceted crystals", "pack_grains", "shape = Cube-octahedron", "Grains packed as cube-octahedra."),
    ("Directionally solidified metal", "columnar_grains", "tilt = 2; height = 300; height_sigma = 0.3", "Long columns that hardly lean."),
    ("Layer by layer build", "columnar_grains", "realism = 0.7; layer = 40", "Columns that grow through the layers of an additive build."),
    ("Three-phase ceramic", "dead_leaves_mosaic", "phase_fractions = 0.5 0.3 0.2", "Grains of three phases that fill space."),
    ("Nearly hard particles", "penetrable_spheres", "impenetrability = 0.9; density = 0.4", "Particles that overlap only at their rims."),
    ("Polydisperse inclusions", "overlapping_spheres", "fraction = 0.3; radius_sigma = 0.5", "Inclusions of widely spread sizes."),
    ("Particle-filled polymer", "hard_spheres", "fraction = 0.3; radius = 5", "Filler particles apart from one another."),
    ("Fibre felt", "overlapping_fibres", "directions = In the XY plane; length = 60; fraction = 0.1", "Straight fibres lying in a plane."),
    ("Hollow fibre insulation", "overlapping_fibres", "lumen = 0.6; fraction = 0.2", "Fibres with an open core."),
    (
        "Pressed fibre board",
        "bending_fibres",
        "directions = Girdle about the XY plane; direction_shape = 10; fraction = 0.35",
        "Fibres bent around one another near a plane.",
    ),
    ("Bonded nonwoven", "cross_linked_fibres", "bonded = 0.9", "Fibres bonded at most of their contacts."),
    ("Layered rock", "random_laminate", "thickness = 3; thickness_sigma = 0.6; hole_fraction = 0.05", "Layers of uneven thickness with few holes."),
    ("Sandstone pores", "reconstructed_medium", "fraction = 0.2; length = 4", "A pore space rebuilt from its statistics."),
    ("Sheet between two phases", "gaussian_field_medium", "kind = Between two levels; fraction = 0.3", "A phase that lies as a sheet between two others."),
    ("Heat exchanger core", "triply_periodic_surface", "surface = Schwarz D; fraction = 0.3", "A sheet that separates two channel systems."),
    ("Gyroid network", "triply_periodic_surface", "variant = Network; fraction = 0.3", "One of the two channel systems of the gyroid, filled."),
    ("Kelvin cell foam", "strut_lattice", "cell = Kelvin; fraction = 0.08", "The struts of a regular foam."),
    ("Diamond lattice", "strut_lattice", "cell = Diamond; fraction = 0.2", "Four struts at every node."),
    ("Irregular truss", "disordered_strut_lattice", "perturbation = 0.3", "A truss with its nodes moved off their places."),
    ("Vicsek cross", "fractal_sponge", "pattern = Vicsek fractal", "Crosses of crosses."),
    ("Beam network", "scale_rich_network", "element = Beams", "Beams of many sizes that end on older ones."),
    ("Domains on hexagons", "cellular_automaton_structure", "lattice = Hexagons; rule = Majority", "Domains coarsened on a honeycomb of cells."),
    ("Quasicrystal pattern", "cellular_automaton_structure", "lattice = Penrose rhombi; rule = Majority", "Domains on a tiling that never repeats."),
    ("Spotted pattern", "reaction_diffusion_pattern", "pattern = Spots", "Separate spots."),
    ("Deposited film", "aggregation_cluster", "process = Ballistic; geometry = Deposit on the base; fraction = 0.3", "A porous film grown from the base."),
    ("Honeycomb core", "honeycomb", "cell = 24; wall = 1.5", "Thin-walled hexagonal cells."),
    ("Brick and mortar", "nacre", "mortar = 2", "Tablets in rows, shifted from row to row, in a thin mortar."),
];

/// The model and settings of a starting point.
pub fn template(name: &str) -> Result<(std::sync::Arc<FilterDef>, Params)> {
    let t = TEMPLATES.iter().find(|t| t.0.eq_ignore_ascii_case(name)).ok_or_else(|| anyhow!("no starting point named {name}"))?;
    let def = crate::registry::find(t.1).ok_or_else(|| anyhow!("no model {}", t.1))?;
    let p = params_from_text(&def, t.2)?;
    Ok((def, p))
}

/// Microstructure Template: the model of a starting point with its settings, on another grid
/// or with another seed when asked.
pub fn from_template(name: &str, cells: &[f64], seed: i64) -> Result<DataSet> {
    let (def, mut p) = template(name)?;
    if !cells.is_empty() {
        set_cells(&def, &mut p, cells);
    }
    if seed > 0 {
        p.set("seed", ParamValue::Int(seed));
    }
    (def.apply)(None, &p)
}

/// Give a model its number of cells. A model sized by lengths (Columnar Grains, with Size and
/// Voxel size) gets as many cells of its own size.
fn set_cells(def: &FilterDef, p: &mut Params, cells: &[f64]) {
    if def.params.iter().any(|s| s.key == "cells") {
        p.set("cells", ParamValue::Floats(cells.to_vec()));
    } else if def.params.iter().any(|s| s.key == "size") {
        let h = p.f64("voxel", 1.0);
        p.set("size", ParamValue::Vec3(std::array::from_fn(|k| cells.get(k).copied().unwrap_or(64.0) * h)));
    }
}

/// The number of cells a model is set to make along each axis.
fn cells_of(def: &FilterDef, p: &Params) -> Vec<f64> {
    if def.params.iter().any(|s| s.key == "cells") {
        p.floats("cells")
    } else if def.params.iter().any(|s| s.key == "size") {
        let h = p.f64("voxel", 1.0);
        p.vec3("size", [64.0 * h; 3]).iter().map(|x| (x / h).round().max(1.0)).collect()
    } else {
        Vec::new()
    }
}

fn model_named(label: &str) -> Result<std::sync::Arc<FilterDef>> {
    FAMILIES
        .iter()
        .flat_map(|f| f.1.iter())
        .filter_map(|id| crate::registry::find(id))
        .find(|d| d.label == label || d.id == label)
        .ok_or_else(|| anyhow!("no microstructure model named {label}"))
}

/// The models that can be followed while they form: the setting that advances them, its
/// first and last values, and the settings that make the steps follow one another.
pub const FORMING: &[(&str, &str, f64, f64, &str)] = &[
    ("nucleation_growth_grains", "time", 1.0, 24.0, ""),
    ("cellular_automaton_structure", "steps", 0.0, 40.0, ""),
    ("activator_inhibitor_pattern", "steps", 0.0, 15.0, ""),
    ("reaction_diffusion_pattern", "steps", 100.0, 3000.0, ""),
    ("aggregation_cluster", "fraction", 0.004, 0.08, ""),
    ("scale_rich_network", "ligaments", 1.0, 400.0, "fraction = 0"),
];

pub fn forming_labels() -> Vec<String> {
    FORMING.iter().filter_map(|f| crate::registry::find(f.0)).map(|d| d.label.to_string()).collect()
}

/// Microstructure Formation at the step `step` of `frames`: the model with the setting that
/// advances it at that stage.
pub fn formation(model: &str, settings: &str, range: &[f64], frames: usize, step: f64) -> Result<DataSet> {
    let def = model_named(model)?;
    let f = FORMING.iter().find(|f| f.0 == def.id).ok_or_else(|| anyhow!("{} cannot be followed while it forms", def.label))?;
    let mut p = params_from_text(&def, &format!("{}; {settings}", f.4))?;
    let (from, to) = (range.first().copied().unwrap_or(f.2), range.get(1).copied().unwrap_or(f.3));
    let t = if frames > 1 { (step / (frames - 1) as f64).clamp(0.0, 1.0) } else { 1.0 };
    let value = from + (to - from) * t;
    let spec = def.params.iter().find(|s| s.key == f.1).ok_or_else(|| anyhow!("no setting {}", f.1))?;
    p.set(f.1, if matches!(spec.kind, ParamKind::Int { .. }) { ParamValue::Int(value.round() as i64) } else { ParamValue::Float(value) });
    let mut out = (def.apply)(None, &p)?;
    out.field_data_mut_any().insert(Array::scalars_f64("FormationValue", vec![value]));
    Ok(out)
}

pub struct StudyParams {
    pub model: String,
    pub settings: String,
    /// The setting that is varied (none: only the sizes and the seeds change) and its values.
    pub vary: String,
    pub values: Vec<f64>,
    /// Cells along every edge of the box (none: the size of the model).
    pub sizes: Vec<f64>,
    pub realizations: usize,
    pub phase: i32,
    pub connectivity: usize,
    pub seed: u64,
}

/// Microstructure Study: a model run again and again, over sizes of the box, values of one
/// setting and seeds. A row for every size and value: the mean and the variance of the
/// volume fraction of a phase, its surface, and how often the phase joins opposite faces.
/// With a setting varied, the value at which half the samples percolate is found for every
/// size and carried to an endless box; with several sizes, the variance gives the integral
/// range.
pub fn study(p: &StudyParams) -> Result<DataSet> {
    let def = model_named(&p.model)?;
    let base = params_from_text(&def, &p.settings)?;
    let key = p.vary.trim();
    let spec = if key.is_empty() {
        None
    } else {
        Some(def.params.iter().find(|s| s.key == key || s.label.eq_ignore_ascii_case(key)).ok_or_else(|| anyhow!("{} has no setting {key}", def.label))?)
    };
    let values: Vec<f64> = if spec.is_some() { p.values.clone() } else { vec![f64::NAN] };
    if values.is_empty() {
        bail!("give the values of {key}");
    }
    let own = cells_of(&def, &base);
    let planar = own.get(2).is_some_and(|z| *z <= 1.0);
    let sizes: Vec<f64> = if p.sizes.is_empty() { vec![own.first().copied().unwrap_or(64.0)] } else { p.sizes.clone() };
    let runs = p.realizations.max(1);
    if sizes.len() * values.len() * runs > 5000 {
        bail!("more than 5000 runs: fewer sizes, values or samples");
    }
    let mut cases: Vec<(f64, f64)> = Vec::new();
    for s in &sizes {
        for v in &values {
            cases.push((*s, *v));
        }
    }
    type Row = (f64, f64, f64, [f64; 3], f64, f64);
    let rows: Vec<Result<Row>> = cases
        .par_iter()
        .map(|(size, value)| {
            let mut q = base.clone();
            if !p.sizes.is_empty() {
                set_cells(&def, &mut q, &[*size, *size, if planar { 1.0 } else { *size }]);
            }
            if let Some(s) = spec {
                q.set(&s.key, if matches!(s.kind, ParamKind::Int { .. }) { ParamValue::Int(value.round() as i64) } else { ParamValue::Float(*value) });
            }
            let (mut fractions, mut joins, mut any, mut surface) = (Vec::new(), [0.0; 3], 0.0, 0.0);
            for k in 0..runs {
                q.set("seed", ParamValue::Int((p.seed + k as u64) as i64));
                let out = (def.apply)(None, &q)?;
                let m = measure::Cells::of(&out, "")?;
                let mask: Vec<bool> = m.phases.iter().map(|x| *x == p.phase).collect();
                fractions.push(mask.iter().filter(|x| **x).count() as f64 / mask.len() as f64);
                let (j, _) = spanning(&mask, m.n, p.connectivity);
                (0..3).for_each(|a| joins[a] += j[a] as u8 as f64 / runs as f64);
                any += j.iter().any(|x| *x) as u8 as f64 / runs as f64;
                surface += specific_surface(&mask, m.n, m.sp, false) / runs as f64;
            }
            let mean = fractions.iter().sum::<f64>() / runs as f64;
            let variance = if runs > 1 { fractions.iter().map(|f| (f - mean).powi(2)).sum::<f64>() / (runs - 1) as f64 } else { f64::NAN };
            Ok((mean, variance, surface, joins, any, *size))
        })
        .collect();
    let mut done: Vec<Row> = Vec::new();
    for r in rows {
        done.push(r?);
    }
    let spacing = base.f64("spacing", base.f64("voxel", 1.0));
    let dim = if planar { 2 } else { 3 };
    let mut t = Table::default();
    let mut col = |name: &str, v: Vec<f64>| {
        t.columns.insert(Array::scalars_f64(name, v));
    };
    col("Size", cases.iter().map(|c| c.0).collect());
    if spec.is_some() {
        col("Value", cases.iter().map(|c| c.1).collect());
    }
    col("MeanFraction", done.iter().map(|d| d.0).collect());
    col("VarianceOfFraction", done.iter().map(|d| d.1).collect());
    // The volume over which the phase is correlated, from the scatter of its fraction.
    col("IntegralRange", done.iter().map(|d| d.1 * (d.5 * spacing).powi(dim) / (d.0 * (1.0 - d.0)).max(1e-300)).collect());
    col("SpecificSurface", done.iter().map(|d| d.2).collect());
    for (a, name) in ["PercolatesX", "PercolatesY", "PercolatesZ"].iter().enumerate().take(dim as usize) {
        col(name, done.iter().map(|d| d.3[a]).collect());
    }
    col("Percolates", done.iter().map(|d| d.4).collect());
    if spec.is_some() && values.len() > 1 {
        // For every size, the value at which half the samples percolate along an axis.
        let mut thresholds = Vec::new();
        for (k, _) in sizes.iter().enumerate() {
            let share: Vec<f64> = (0..values.len()).map(|v| done[k * values.len() + v].3.iter().take(dim as usize).sum::<f64>() / dim as f64).collect();
            let mut found = f64::NAN;
            for v in 0..values.len() - 1 {
                let (a, b) = (share[v] - 0.5, share[v + 1] - 0.5);
                if a == 0.0 || a * b < 0.0 {
                    found = values[v] + (values[v + 1] - values[v]) * if a == b { 0.0 } else { a / (a - b) };
                    break;
                }
            }
            thresholds.push(found);
        }
        t.field_data.insert(Array::scalars_f64("ThresholdSizes", sizes.clone()));
        t.field_data.insert(Array::scalars_f64("Thresholds", thresholds.clone()));
        // Towards an endless box: the threshold moves as the size to the power -1 / nu.
        let nu = if planar { 4.0 / 3.0 } else { 0.8765 };
        let pts: Vec<(f64, f64)> = sizes.iter().zip(&thresholds).filter(|(_, t)| t.is_finite()).map(|(s, t)| (s.powf(-1.0 / nu), *t)).collect();
        if pts.len() >= 2 {
            let n = pts.len() as f64;
            let (sx, sy) = (pts.iter().map(|p| p.0).sum::<f64>(), pts.iter().map(|p| p.1).sum::<f64>());
            let (sxx, sxy) = (pts.iter().map(|p| p.0 * p.0).sum::<f64>(), pts.iter().map(|p| p.0 * p.1).sum::<f64>());
            let slope = (n * sxy - sx * sy) / (n * sxx - sx * sx);
            t.field_data.insert(Array::scalars_f64("ThresholdOfAnEndlessBox", vec![(sy - slope * sx) / n]));
        }
    }
    t.field_data.insert(Array::scalars_f64("Samples", vec![runs as f64]));
    Ok(DataSet::Table(t))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_model_sized_by_lengths_takes_cells() {
        let def = crate::registry::find("columnar_grains").unwrap();
        let mut p = def.default_params();
        p.set("voxel", ParamValue::Float(5.0));
        set_cells(&def, &mut p, &[12.0, 10.0, 8.0]);
        assert_eq!(p.vec3("size", [0.0; 3]), [60.0, 50.0, 40.0]);
        assert_eq!(cells_of(&def, &p), vec![12.0, 10.0, 8.0]);
        let out = (def.apply)(None, &p).unwrap();
        let DataSet::ImageData(img) = out else { panic!("not an image") };
        assert_eq!([img.extent[1] - img.extent[0], img.extent[3] - img.extent[2], img.extent[5] - img.extent[4]], [12, 10, 8]);
        // A model with Cells keeps them as given.
        let spheres = crate::registry::find("overlapping_spheres").unwrap();
        let mut q = spheres.default_params();
        set_cells(&spheres, &mut q, &[20.0, 20.0, 1.0]);
        assert_eq!(cells_of(&spheres, &q), vec![20.0, 20.0, 1.0]);
    }
}
