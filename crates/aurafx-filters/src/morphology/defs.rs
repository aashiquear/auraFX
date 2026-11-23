//! The morphological models as sources of the registry (and the two filters that build on
//! them), with their settings and standard values.

use super::*;
use crate::params::*;
use crate::registry::{b, choice, f, i, FilterDef, MultiInput};
use crate::synthetic::{ORIENT_FIBER100, ORIENT_FIBER111, ORIENT_RANDOM};
use anyhow::{anyhow, Result};

fn floats(key: &str, label: &str, default: &[f64]) -> ParamSpec {
    ParamSpec::new(key, label, ParamKind::Floats, ParamValue::Floats(default.to_vec()))
}

/// The grid settings: cells, cell size and, when the model can wrap, periodic.
fn grid_params(cells: [f64; 3], periodic: Option<bool>) -> Vec<ParamSpec> {
    let mut v = vec![
        floats("cells", "Cells", &cells).help("Cells along X, Y and Z. One cell along Z: the model in the plane."),
        f("spacing", "Cell size", 1.0, 1e-9, f64::INFINITY).help("The lengths below are in the same unit."),
    ];
    if let Some(p) = periodic {
        v.push(b("periodic", "Periodic", p).help("The structure continues through opposite faces of the box."));
    }
    v
}

fn seed() -> ParamSpec {
    i("seed", "Random seed", 1, 0, i64::MAX).advanced()
}

fn fraction(default: f64) -> ParamSpec {
    f("fraction", "Volume fraction", default, 0.0, 1.0)
}

fn directions(default: &str, shape: f64) -> Vec<ParamSpec> {
    vec![
        choice("directions", "Directions", DIRECTIONS, default),
        f("direction_shape", "Beta, or girdle spread (degrees)", shape, 0.0, 1000.0)
            .help("For the beta law: 1 is isotropic, towards 0 the fibres line up with Z, large values lay them in the XY plane. For the girdle: the spread of the angle from the XY plane, in degrees.")
            .advanced(),
    ]
}

fn grid(p: &Params) -> Result<Grid> {
    Grid::new(&p.floats("cells"), p.f64("spacing", 1.0), p.bool("periodic", false))
}

fn dirs(p: &Params, g: &Grid) -> Directions {
    Directions { law: p.str("directions"), shape: p.f64("direction_shape", 1.0), planar: g.planar() }
}

fn seed_of(p: &Params) -> u64 {
    p.i64("seed", 1) as u64
}

fn axis_of(p: &Params, key: &str) -> usize {
    match p.str(key).as_str() {
        "X" => 0,
        "Y" => 1,
        _ => 2,
    }
}

fn crystal_params() -> Vec<ParamSpec> {
    vec![
        choice("orientations", "Orientations", &[ORIENT_RANDOM, ORIENT_FIBER100, ORIENT_FIBER111], ORIENT_RANDOM),
        f("spread", "Fibre spread (degrees)", 10.0, 0.0, 90.0).advanced(),
        choice("structure", "Crystal structure", &crate::crystal::structure_choices()[1..], "Cubic m-3m"),
    ]
}

fn source(
    id: &'static str,
    label: &'static str,
    description: &'static str,
    keywords: &'static [&'static str],
    params: Vec<ParamSpec>,
    apply: crate::registry::ApplyFn,
) -> FilterDef {
    FilterDef { id, label, category: "Sources", description, keywords, is_source: true, ext: None, temporal: None, multi: None, params, apply }
}

fn join(parts: Vec<Vec<ParamSpec>>) -> Vec<ParamSpec> {
    parts.into_iter().flatten().collect()
}

pub fn defs() -> Vec<FilterDef> {
    const C64: [f64; 3] = [64.0, 64.0, 64.0];
    vec![
        // ---------------- Grains and polycrystals ----------------
        source(
            "nucleation_growth_grains",
            "Nucleation and Growth Grains",
            "Grains by nucleation and growth: nuclei appear at random, all at the start or at a constant rate, and grow at one speed until they meet. Stopped early: a partly transformed material.",
            &["jmak", "avrami", "johnson-mehl", "recrystallization", "crystallization", "solidification", "kinetics", "synthetic microstructure", "polycrystal", "grains"],
            join(vec![
                grid_params(C64, Some(false)),
                vec![
                    choice("nucleation", "Nucleation", &[growth::NUCLEATION_CONTINUOUS, growth::NUCLEATION_START], growth::NUCLEATION_CONTINUOUS),
                    f("rate", "Nucleation rate (per volume and time)", 5e-5, 0.0, f64::INFINITY).help("For nucleation at a constant rate."),
                    i("nuclei", "Nuclei at the start", 150, 1, 1_000_000).help("For nucleation all at the start (the grains are then Voronoi cells)."),
                    f("growth", "Growth speed", 1.0, 1e-12, f64::INFINITY),
                    f("time", "Stop at time (0: when full)", 0.0, 0.0, f64::INFINITY).help("Above 0, the cells not yet reached are the last feature, of phase 2; the transformed fraction is kept with the result beside the one the kinetics law gives."),
                    choice("front", "Growth front", &[growth::FRONT_ROUND, growth::FRONT_CUBE, growth::FRONT_OCTAHEDRON], growth::FRONT_ROUND).help("Cubes and octahedra: faceted grains, the facets along the crystal axes of each grain."),
                ],
                crystal_params(),
                vec![seed()],
            ]),
            |_, p| {
                growth::nucleation_growth(
                    &grid(p)?,
                    &growth::NucleationParams {
                        nucleation: p.str("nucleation"),
                        nuclei: p.i64("nuclei", 150).max(1) as usize,
                        rate: p.f64("rate", 5e-5),
                        growth: p.f64("growth", 1.0),
                        time: p.f64("time", 0.0),
                        front: p.str("front"),
                        orientations: p.str("orientations"),
                        spread_deg: p.f64("spread", 10.0),
                        structure: p.str("structure"),
                        seed: seed_of(p),
                    },
                )
            },
        ),
        source(
            "dead_leaves_mosaic",
            "Dead Leaves Mosaic",
            "A mosaic of grains that are not convex: spheres or spheroids fall one after another and hide what lies under them. One or several phases, an orientation for every grain, or a finite number of leaves over a background.",
            &["dead leaves", "mosaic", "random tessellation", "non-convex grains", "multiphase", "synthetic microstructure", "sequential"],
            join(vec![
                grid_params(C64, Some(true)),
                vec![
                    f("radius", "Radius", 8.0, 1e-9, f64::INFINITY),
                    f("radius_sigma", "Radius spread (log sigma)", 0.3, 0.0, 3.0),
                    f("aspect", "Aspect ratio", 1.0, 0.05, 20.0).help("1: spheres. Above 1 elongated spheroids, below 1 flat ones, turned at random."),
                    floats("phase_fractions", "Phase fractions", &[0.5, 0.5]).help("The volume fraction of each phase of leaves (they are scaled to sum to 1)."),
                    floats("phase_radii", "Radius of each phase", &[]).help("Empty: Radius for every phase.").advanced(),
                    f("coverage", "Coverage (0: until full)", 0.0, 0.0, 50.0).help("Above 0: as many leaves as cover a point this many times on average; the cells no leaf reached are a background, the last feature."),
                ],
                crystal_params(),
                vec![seed()],
            ]),
            |_, p| {
                fields::dead_leaves(
                    &grid(p)?,
                    &fields::LeavesParams {
                        radius: p.f64("radius", 8.0),
                        sigma: p.f64("radius_sigma", 0.3),
                        aspect: p.f64("aspect", 1.0),
                        phase_fractions: p.floats("phase_fractions"),
                        phase_radii: p.floats("phase_radii"),
                        coverage: p.f64("coverage", 0.0),
                        orientations: p.str("orientations"),
                        spread_deg: p.f64("spread", 10.0),
                        structure: p.str("structure"),
                        seed: seed_of(p),
                    },
                )
            },
        ),
        // ---------------- Particles ----------------
        source(
            "overlapping_spheres",
            "Overlapping Spheres",
            "Spheres at random points, free to overlap (the Boolean model): porous and granular media. The number of spheres follows from the volume fraction of their union.",
            &["boolean model", "poisson", "porous medium", "granular", "random spheres", "synthetic microstructure", "pores"],
            join(vec![
                grid_params(C64, Some(true)),
                vec![f("radius", "Radius", 6.0, 1e-9, f64::INFINITY), f("radius_sigma", "Radius spread (log sigma)", 0.0, 0.0, 3.0), fraction(0.3), seed()],
            ]),
            |_, p| particles::overlapping_spheres(&grid(p)?, p.f64("radius", 6.0), p.f64("radius_sigma", 0.0), p.f64("fraction", 0.3), seed_of(p)),
        ),
        source(
            "penetrable_spheres",
            "Penetrable Spheres",
            "Spheres that may overlap down to a hard core (the cherry-pit model): from freely overlapping spheres (impenetrability 0) to hard spheres (1).",
            &["cherry-pit", "penetrable concentric shell", "hard core", "interpenetrable", "synthetic microstructure", "particles"],
            join(vec![
                grid_params(C64, Some(true)),
                vec![
                    f("radius", "Radius", 6.0, 1e-9, f64::INFINITY),
                    f("impenetrability", "Impenetrability", 0.5, 0.0, 1.0).help("The radius of the hard core over that of the sphere."),
                    f("density", "Reduced density", 0.6, 0.0, 10.0).help("Spheres per unit volume times the volume of one: the fraction they would take without overlap."),
                    seed(),
                ],
            ]),
            |_, p| particles::penetrable_spheres(&grid(p)?, p.f64("radius", 6.0), p.f64("impenetrability", 0.5), p.f64("density", 0.6), seed_of(p)),
        ),
        source(
            "hard_spheres",
            "Hard Spheres",
            "Spheres that do not overlap: placed one after another at random (up to a fraction near 0.38), or packed densely by growing them while pushing them apart (up to about 0.6).",
            &["random sequential addition", "rsa", "packing", "dense random packing", "inclusions", "composite", "synthetic microstructure", "particles", "non-overlapping"],
            join(vec![
                grid_params(C64, Some(true)),
                vec![
                    f("radius", "Radius", 6.0, 1e-9, f64::INFINITY),
                    f("radius_sigma", "Radius spread (log sigma)", 0.0, 0.0, 3.0),
                    fraction(0.3),
                    f("gap", "Least gap", 0.5, 0.0, f64::INFINITY).help("No two spheres come nearer than this."),
                    choice("method", "Method", &[particles::PACK_RSA, particles::PACK_DENSE], particles::PACK_RSA),
                    f("growth_rate", "Growth per step (dense packing)", 0.01, 1e-4, 0.2).help("Slower growth packs denser (and takes longer).").advanced(),
                    seed(),
                ],
            ]),
            |_, p| particles::hard_spheres(&grid(p)?, p.f64("radius", 6.0), p.f64("radius_sigma", 0.0), p.f64("fraction", 0.3), p.f64("gap", 0.5), &p.str("method"), p.f64("growth_rate", 0.01), seed_of(p)),
        ),
        source(
            "clustered_spheres",
            "Clustered Spheres",
            "Small particles gathered in aggregates: the aggregates are overlapping spheres, and the particles fall inside them (fillers in a rubber, particles in agglomerates). The aggregates can be a phase of their own.",
            &["cox process", "aggregates", "agglomerates", "nanocomposite", "carbon black", "clusters", "fillers", "synthetic microstructure"],
            join(vec![
                grid_params(C64, Some(true)),
                vec![
                    f("aggregate_radius", "Aggregate radius", 12.0, 1e-9, f64::INFINITY),
                    f("aggregate_fraction", "Aggregate volume fraction", 0.25, 0.0, 0.99),
                    f("radius", "Particle radius", 2.5, 1e-9, f64::INFINITY),
                    f("local_fraction", "Particle fraction inside the aggregates", 0.35, 0.0, 0.99),
                    f("outside_fraction", "Particle fraction outside", 0.0, 0.0, 0.99).advanced(),
                    b("hard", "Particles do not overlap", true),
                    b("aggregate_phase", "Aggregates as a third phase", true),
                    seed(),
                ],
            ]),
            |_, p| {
                particles::clustered_spheres(
                    &grid(p)?,
                    &particles::ClusterParams {
                        aggregate_radius: p.f64("aggregate_radius", 12.0),
                        aggregate_fraction: p.f64("aggregate_fraction", 0.25),
                        radius: p.f64("radius", 2.5),
                        local_fraction: p.f64("local_fraction", 0.35),
                        outside_fraction: p.f64("outside_fraction", 0.0),
                        hard: p.bool("hard", true),
                        aggregate_phase: p.bool("aggregate_phase", true),
                        seed: seed_of(p),
                    },
                )
            },
        ),
        // ---------------- Fibres and layers ----------------
        source(
            "overlapping_fibres",
            "Overlapping Fibres",
            "Straight fibres of a given length at random places, free to overlap: cylinders or fibres with round ends, in any direction, along Z, in a plane or in between.",
            &["boolean model", "cylinders", "spherocylinders", "fibers", "fibrous", "nonwoven", "short fibres", "synthetic microstructure"],
            join(vec![
                grid_params(C64, Some(true)),
                vec![f("radius", "Radius", 2.0, 1e-9, f64::INFINITY), f("length", "Length", 30.0, 0.0, f64::INFINITY)],
                directions(DIR_ISOTROPIC, 1.0),
                vec![b("round", "Round ends", false), fraction(0.15), f("lumen", "Hollow (inner radius over the radius)", 0.0, 0.0, 0.95).advanced(), seed()],
            ]),
            |_, p| {
                let g = grid(p)?;
                fibres::overlapping_fibres(&g, p.f64("radius", 2.0), p.f64("length", 30.0), &dirs(p, &g), p.bool("round", false), p.f64("fraction", 0.15), p.f64("lumen", 0.0), seed_of(p))
            },
        ),
        source(
            "poisson_line_fibres",
            "Poisson Line Fibres",
            "Fibres without ends: cylinders about random straight lines that cross the whole box (long-fibre mats, fibrous networks that always percolate).",
            &["poisson lines", "infinite fibres", "fibers", "long fibres", "fibrous network", "percolation", "synthetic microstructure"],
            join(vec![grid_params(C64, None), vec![f("radius", "Radius", 2.5, 1e-9, f64::INFINITY)], directions(DIR_ISOTROPIC, 1.0), vec![fraction(0.16), seed()]]),
            |_, p| {
                let g = grid(p)?;
                fibres::poisson_line_fibres(&g, p.f64("radius", 2.5), &dirs(p, &g), p.f64("fraction", 0.16), seed_of(p))
            },
        ),
        source(
            "curved_fibres",
            "Curved Fibres",
            "Curved fibres: chains that forget their direction along their length, arcs of circles or helices; overlapping, or placed one after another without overlap; solid or hollow.",
            &["worm-like chain", "persistence length", "arcs", "helix", "crimp", "nonwoven", "entangled", "fibers", "synthetic microstructure"],
            join(vec![
                grid_params(C64, Some(true)),
                vec![
                    choice("shape", "Shape", &[fibres::CURVE_CHAIN, fibres::CURVE_ARC, fibres::CURVE_HELIX], fibres::CURVE_CHAIN),
                    f("radius", "Radius", 1.5, 1e-9, f64::INFINITY),
                    f("length", "Length along the fibre", 60.0, 0.0, f64::INFINITY),
                    f("persistence", "Persistence length (chains)", 30.0, 1e-9, f64::INFINITY).help("The length over which a chain forgets its direction: short for tangled fibres, long for nearly straight ones."),
                    f("curvature_radius", "Radius of curvature (arcs), helix radius", 30.0, 0.0, f64::INFINITY),
                    f("pitch", "Pitch (helices)", 20.0, 0.0, f64::INFINITY).advanced(),
                    i("segments", "Straight pieces per fibre", 16, 1, 2000).advanced(),
                ],
                directions(DIR_ISOTROPIC, 1.0),
                vec![
                    fraction(0.1),
                    b("overlap", "Fibres may overlap", true),
                    f("gap", "Least gap (without overlap)", 0.5, 0.0, f64::INFINITY).advanced(),
                    f("lumen", "Hollow (inner radius over the radius)", 0.0, 0.0, 0.95).advanced(),
                    seed(),
                ],
            ]),
            |_, p| {
                let g = grid(p)?;
                let c = fibres::CurveParams {
                    shape: p.str("shape"),
                    radius: p.f64("radius", 1.5),
                    length: p.f64("length", 60.0),
                    persistence: p.f64("persistence", 30.0),
                    curvature_radius: p.f64("curvature_radius", 30.0),
                    pitch: p.f64("pitch", 20.0),
                    segments: p.i64("segments", 16).max(1) as usize,
                };
                fibres::curved_fibres(&g, &c, &dirs(p, &g), p.f64("fraction", 0.1), p.bool("overlap", true), p.f64("gap", 0.5), p.f64("lumen", 0.0), seed_of(p))
            },
        ),
        source(
            "hard_fibres",
            "Hard Fibres",
            "Straight fibres with round ends that do not overlap, placed one after another at random with a least gap between them: short-fibre composites.",
            &["random sequential addition", "rsa", "spherocylinders", "short fibre composite", "fibers", "non-overlapping", "synthetic microstructure"],
            join(vec![
                grid_params(C64, Some(true)),
                vec![f("radius", "Radius", 2.0, 1e-9, f64::INFINITY), f("length", "Length of the straight part", 24.0, 0.0, f64::INFINITY)],
                directions(DIR_ISOTROPIC, 1.0),
                vec![fraction(0.12), f("gap", "Least gap", 0.5, 0.0, f64::INFINITY), f("lumen", "Hollow (inner radius over the radius)", 0.0, 0.0, 0.95).advanced(), seed()],
            ]),
            |_, p| {
                let g = grid(p)?;
                fibres::hard_fibres(&g, p.f64("radius", 2.0), p.f64("length", 24.0), &dirs(p, &g), p.f64("fraction", 0.12), p.f64("gap", 0.5), p.f64("lumen", 0.0), seed_of(p))
            },
        ),
        source(
            "bending_fibres",
            "Bending Fibres",
            "Long bending fibres packed without overlap to high volume fractions: each fibre is a random walk that keeps near its main direction, and the fibres push one another apart until they no longer overlap.",
            &["random walk", "von mises-fisher", "force-biased packing", "glass fibre", "fibers", "dense fibre packing", "contact", "synthetic microstructure"],
            join(vec![
                grid_params(C64, Some(true)),
                vec![f("radius", "Radius", 2.0, 1e-9, f64::INFINITY), f("length", "Length along the fibre", 40.0, 0.0, f64::INFINITY)],
                directions(DIR_ISOTROPIC, 1.0),
                vec![
                    f("kappa_main", "Hold to the main direction", 10.0, 0.0, 10_000.0).help("How closely each step of the walk follows the main direction of its fibre (0: not at all)."),
                    f("kappa_step", "Hold to the previous step", 100.0, 0.0, 10_000.0).help("How closely each step follows the one before: high values give smooth, stiff fibres."),
                    fraction(0.25),
                    i("sweeps", "Packing sweeps at most", 400, 0, 100_000).advanced(),
                    f("contact_distance", "Draw into contact within", 0.0, 0.0, f64::INFINITY).help("Above 0, fibres nearer than this to one another are pulled until they touch.").advanced(),
                    seed(),
                ],
            ]),
            |_, p| {
                let g = grid(p)?;
                fibres::bending_fibres(
                    &g,
                    &fibres::BendingParams {
                        radius: p.f64("radius", 2.0),
                        length: p.f64("length", 40.0),
                        kappa_main: p.f64("kappa_main", 10.0),
                        kappa_step: p.f64("kappa_step", 100.0),
                        fraction: p.f64("fraction", 0.25),
                        sweeps: p.i64("sweeps", 400).max(0) as usize,
                        contact_distance: p.f64("contact_distance", 0.0),
                        seed: seed_of(p),
                    },
                    &dirs(p, &g),
                )
            },
        ),
        source(
            "cross_linked_fibres",
            "Cross-Linked Fibres",
            "Entangled straight fibres bonded where they touch: a fraction of the contacts carries a cross-link, a ball of a third phase (moulded fibre networks bonded by a resin).",
            &["entangled", "cross-links", "contacts", "bonded fibres", "epoxy", "fibers", "network", "synthetic microstructure"],
            join(vec![
                grid_params(C64, Some(false)),
                vec![f("radius", "Radius", 1.5, 1e-9, f64::INFINITY), f("length", "Length (0: fibres without ends)", 0.0, 0.0, f64::INFINITY).help("0: the fibres cross the whole box, which is then not periodic.")],
                directions(DIR_GIRDLE, 20.0),
                vec![fraction(0.085), f("bonded", "Contacts with a cross-link (fraction)", 0.5, 0.0, 1.0), f("link_radius", "Cross-link radius", 3.0, 0.0, f64::INFINITY), seed()],
            ]),
            |_, p| {
                let g = grid(p)?;
                fibres::cross_linked_fibres(
                    &g,
                    &fibres::CrossLinkParams { radius: p.f64("radius", 1.5), length: p.f64("length", 0.0), fraction: p.f64("fraction", 0.085), bonded: p.f64("bonded", 0.5), link_radius: p.f64("link_radius", 3.0), seed: seed_of(p) },
                    &dirs(p, &g),
                )
            },
        ),
        source(
            "delaunay_fibre_network",
            "Delaunay Fibre Network",
            "A network of fibres on the edges of the Delaunay triangulation of random points, joined at every point (gels, biological networks).",
            &["delaunay", "network", "cross-linked network", "gel", "collagen", "fibers", "beams", "synthetic microstructure"],
            join(vec![
                grid_params(C64, Some(true)),
                vec![
                    i("points", "Points", 200, 8, 200_000),
                    f("radius", "Fibre radius", 1.2, 1e-9, f64::INFINITY),
                    f("fraction", "Volume fraction (0: use the radius)", 0.0, 0.0, 0.95),
                    f("regularity", "Regularity of the points", 0.0, 0.0, 1.0).help("0: points at random. Towards 1 the points keep apart, and the fibres are of more even lengths."),
                    seed(),
                ],
            ]),
            |_, p| fibres::delaunay_fibre_network(&grid(p)?, p.i64("points", 200).max(8) as usize, p.f64("radius", 1.2), p.f64("fraction", 0.0), p.f64("regularity", 0.0), seed_of(p)),
        ),
        source(
            "random_laminate",
            "Random Laminate",
            "Layers of random thicknesses a random distance apart, with round holes through the layers or through the thinner ones only (multilayer films).",
            &["layers", "multilayer", "lamellae", "nanolayered film", "stack", "holes", "synthetic microstructure"],
            join(vec![
                grid_params(C64, Some(true)),
                vec![
                    f("thickness", "Layer thickness", 4.0, 1e-9, f64::INFINITY),
                    f("thickness_sigma", "Thickness spread (log sigma)", 0.4, 0.0, 3.0),
                    f("layer_spacing", "Distance between layers", 8.0, 1e-9, f64::INFINITY),
                    f("spacing_sigma", "Distance spread (log sigma)", 0.3, 0.0, 3.0),
                    choice("axis", "Stacked along", &["X", "Y", "Z"], "Z"),
                    f("hole_fraction", "Holes (fraction of a layer)", 0.2, 0.0, 0.99),
                    f("hole_radius", "Hole radius", 5.0, 0.0, f64::INFINITY),
                    b("thin_only", "Holes in the thinner layers only", true),
                    seed(),
                ],
            ]),
            |_, p| {
                fibres::random_laminate(
                    &grid(p)?,
                    &fibres::LaminateParams {
                        thickness: p.f64("thickness", 4.0),
                        thickness_sigma: p.f64("thickness_sigma", 0.4),
                        spacing: p.f64("layer_spacing", 8.0),
                        spacing_sigma: p.f64("spacing_sigma", 0.3),
                        axis: axis_of(p, "axis"),
                        hole_fraction: p.f64("hole_fraction", 0.2),
                        hole_radius: p.f64("hole_radius", 5.0),
                        thin_only: p.bool("thin_only", true),
                        seed: seed_of(p),
                    },
                )
            },
        ),
        // ---------------- Random fields ----------------
        source(
            "gaussian_field_medium",
            "Gaussian Field Medium",
            "A smooth random field cut at a level: two interwoven phases with rounded shapes (porous media, phase-separated blends). The level follows from the volume fraction.",
            &["gaussian random field", "excursion set", "level cut", "bicontinuous", "spinodal", "porous medium", "synthetic microstructure"],
            join(vec![
                grid_params(C64, Some(true)),
                vec![
                    f("length", "Correlation length", 8.0, 1e-9, f64::INFINITY).help("The distance over which the field loses its memory: the size of the features."),
                    fraction(0.4),
                    choice("kind", "Solid", &[fields::EXCURSION_ABOVE, fields::EXCURSION_BAND], fields::EXCURSION_ABOVE).help("Between two levels: sheets that follow the mean level of the field."),
                    seed(),
                ],
            ]),
            |_, p| fields::gaussian_field_medium(&grid(p)?, p.f64("length", 8.0), p.f64("fraction", 0.4), &p.str("kind"), seed_of(p)),
        ),
        source(
            "transparent_leaves_medium",
            "Transparent Leaves Medium",
            "A random function made of leaves that let part of what lies under them show through, cut at a level: softer, layered shapes than opaque leaves give.",
            &["transparent dead leaves", "random function", "level cut", "random texture", "synthetic microstructure"],
            join(vec![
                grid_params(C64, Some(true)),
                vec![
                    f("radius", "Radius", 5.0, 1e-9, f64::INFINITY),
                    f("radius_sigma", "Radius spread (log sigma)", 0.3, 0.0, 3.0),
                    f("transparency", "Transparency", 0.4, 0.0, 0.99).help("The share of what lies under a leaf that shows through it (0: opaque leaves)."),
                    fraction(0.35),
                    seed(),
                ],
            ]),
            |_, p| fields::transparent_leaves_medium(&grid(p)?, p.f64("radius", 5.0), p.f64("radius_sigma", 0.3), p.f64("transparency", 0.4), p.f64("fraction", 0.35), seed_of(p)),
        ),
        source(
            "reconstructed_medium",
            "Reconstructed Medium",
            "A two-phase medium rebuilt from a statistic alone: cells are swapped at random and the swaps kept when they bring the probability that two points a distance apart are both solid nearer to a target (simulated annealing).",
            &["stochastic reconstruction", "simulated annealing", "two-point probability", "correlation function", "debye medium", "synthetic microstructure"],
            join(vec![
                grid_params([40.0, 40.0, 40.0], None),
                vec![
                    fraction(0.3),
                    f("length", "Correlation length", 3.0, 1e-9, f64::INFINITY).help("The target falls off as exp(-distance / length)."),
                    f("sweeps", "Swaps tried per cell", 15.0, 0.0, 10_000.0).help("More swaps come nearer to the target (and take longer)."),
                    seed(),
                ],
            ]),
            |_, p| fields::reconstructed_medium(&Grid::new(&p.floats("cells"), p.f64("spacing", 1.0), true)?, p.f64("fraction", 0.3), p.f64("length", 3.0), p.f64("sweeps", 15.0), seed_of(p)),
        ),
        // ---------------- Lattices and foams ----------------
        source(
            "triply_periodic_surface",
            "Triply Periodic Surface",
            "A solid built on a triply periodic minimal surface (gyroid, Schwarz P and D, Neovius, I-WP): the sheet about the surface or the network on one side of it, at a volume fraction.",
            &["tpms", "gyroid", "schwarz", "primitive", "diamond", "neovius", "iwp", "minimal surface", "lattice", "additive manufacturing", "scaffold"],
            join(vec![
                grid_params(C64, None),
                vec![
                    choice("surface", "Surface", lattices::SURFACES, "Gyroid"),
                    choice("variant", "Solid", &[lattices::VARIANT_SHEET, lattices::VARIANT_NETWORK], lattices::VARIANT_SHEET),
                    f("cell_size", "Cell size of the surface", 32.0, 1e-9, f64::INFINITY).help("One period of the surface. The structure is periodic when the box holds a whole number of them."),
                    fraction(0.2),
                ],
            ]),
            |_, p| lattices::triply_periodic_surface(&grid(p)?, &p.str("surface"), &p.str("variant"), p.f64("cell_size", 32.0), p.f64("fraction", 0.2)),
        ),
        source("strut_lattice", "Strut Lattice", "A lattice of struts: cubic, body-centred, face-centred, octet, Kelvin or diamond cells, with solid or hollow struts, a radius graded along an axis, the lattice turned.", &["truss", "lattice", "octet", "bcc", "fcc", "kelvin", "diamond", "architected material", "metamaterial", "additive manufacturing", "graded lattice", "hollow struts"], lattice_params(false), |_, p| lattices::strut_lattice(&grid(p)?, &lattice_of(p))),
        source(
            "disordered_strut_lattice",
            "Disordered Strut Lattice",
            "A lattice of struts with struts missing and nodes moved at random: how a lattice loses its stiffness with its connectivity. The mean number of struts per node and Maxwell's count are kept with the result.",
            &["random lattice", "missing struts", "defects", "rigidity", "maxwell", "coordination", "truss", "perturbed lattice", "metamaterial"],
            lattice_params(true),
            |_, p| lattices::strut_lattice(&grid(p)?, &lattice_of(p)),
        ),
        source(
            "hierarchical_lattice",
            "Hierarchical Lattice",
            "A lattice whose struts are themselves frames: columns of octahedra made of tubes, solid or hollow, on the octet truss (and the cube edges), over one or two generations.",
            &["hierarchical", "frames of frames", "octahedra", "hollow tubes", "ultralight", "truss", "metamaterial", "octet"],
            join(vec![
                grid_params([128.0, 128.0, 128.0], None),
                vec![
                    f("cell_size", "Cell size of the lattice", 128.0, 1e-9, f64::INFINITY),
                    i("generations", "Generations", 1, 0, 2).help("0: plain struts. 1: every strut a frame of octahedra. 2: the members of those frames are frames too (needs a fine grid)."),
                    i("octahedra", "Octahedra per frame", 3, 1, 20),
                    f("radius", "Tube radius", 2.0, 1e-9, f64::INFINITY),
                    f("wall", "Wall thickness (0: solid tubes)", 0.0, 0.0, f64::INFINITY),
                    b("cube_edges", "Frames on the cube edges too", true),
                ],
            ]),
            |_, p| lattices::hierarchical_lattice(&grid(p)?, p.f64("cell_size", 128.0), p.i64("generations", 1).max(0) as usize, p.i64("octahedra", 3).max(1) as usize, p.f64("radius", 2.0), p.f64("wall", 0.0), p.bool("cube_edges", true)),
        ),
        source(
            "fractal_sponge",
            "Fractal Sponge",
            "A fractal on a cubic grid: the Menger sponge, the Vicsek fractal or the Cantor dust, to a number of levels. The solid fraction and the fractal dimension are kept with the result.",
            &["menger", "vicsek", "cantor dust", "fractal", "self-similar", "iterated function system", "sierpinski"],
            join(vec![
                grid_params([81.0, 81.0, 81.0], None),
                vec![choice("pattern", "Pattern", lattices::SPONGES, "Menger sponge"), i("level", "Levels", 3, 0, 12), f("cell_size", "Size of the fractal", 81.0, 1e-9, f64::INFINITY).help("The edge of the cube that is cut; the grid shows it best with 3 to the power of the levels cells (or a multiple) along it.")],
            ]),
            |_, p| lattices::fractal_sponge(&grid(p)?, &p.str("pattern"), p.i64("level", 3).max(0) as usize, p.f64("cell_size", 81.0)),
        ),
        source(
            "open_cell_foam",
            "Open-Cell Foam",
            "An open-cell foam: struts on the edges of the cells of random points, the cells made even by relaxation and elongated along an axis, thicker at the nodes (metal and polymer foams, trabecular bone).",
            &["foam", "voronoi edges", "struts", "trabecular bone", "cancellous", "porous", "relaxed cells", "lloyd", "synthetic microstructure"],
            join(vec![
                grid_params(C64, None),
                vec![
                    i("foam_cells", "Cells of the foam", 64, 8, 100_000),
                    f("fraction", "Volume fraction (0: use the radius)", 0.15, 0.0, 0.9),
                    f("radius", "Strut radius", 1.5, 1e-9, f64::INFINITY),
                    i("relaxation", "Relaxation steps", 10, 0, 200).help("Each step moves the points to the centres of their cells: more steps, more even cells."),
                    f("anisotropy", "Elongation of the cells", 1.0, 0.1, 10.0),
                    choice("axis", "Elongated along", &["X", "Y", "Z"], "Z"),
                    f("node_ratio", "Node radius over the strut radius", 1.3, 1.0, 5.0),
                    seed(),
                ],
            ]),
            |_, p| {
                lattices::open_cell_foam(
                    &grid(p)?,
                    &lattices::FoamParams {
                        cells: p.i64("foam_cells", 64).max(8) as usize,
                        radius: p.f64("radius", 1.5),
                        fraction: p.f64("fraction", 0.15),
                        relaxation: p.i64("relaxation", 10).max(0) as usize,
                        anisotropy: p.f64("anisotropy", 1.0),
                        axis: axis_of(p, "axis"),
                        node_ratio: p.f64("node_ratio", 1.3),
                        seed: seed_of(p),
                    },
                )
            },
        ),
        source(
            "level_set_foam",
            "Level-Set Foam",
            "A foam drawn from the distances to packed seeds: struts with three-sided sections that thicken towards the nodes, their sides hollow or bulging, in open cells or cells closed by walls; the seeds can be elongated.",
            &["foam", "plateau borders", "level set", "distance field", "packed ellipsoids", "closed cells", "open cells", "metal foam", "synthetic microstructure"],
            join(vec![
                grid_params(C64, Some(true)),
                vec![
                    floats("seed_radius", "Seed radius (from, to)", &[6.0, 9.0]).help("The short half-axis of the seeds, drawn evenly between the two values."),
                    floats("aspect", "Seed shape (Y and Z over X)", &[1.0, 1.5]),
                    f("packing", "Packing of the seeds", 0.55, 0.05, 0.64).advanced(),
                    fraction(0.2),
                    f("curvature", "Curvature of the strut sides", -0.7, -5.0, 0.99).help("Between 0 and 1: hollow sides, as in a drained liquid foam. Below 0: bulging sides."),
                    f("c1", "Thickening towards the nodes (c1)", 50.0, 0.0, 1000.0).help("The section of a strut at the position x along it (0 at mid-span, 1/2 at the nodes) is c1 x^4 + c2 x^2 + 1 times its section at mid-span.").advanced(),
                    f("c2", "Thickening towards the nodes (c2)", 1.0, 0.0, 1000.0).advanced(),
                    f("wall", "Closed cells: wall over strut thickness (0: open)", 0.0, 0.0, 10.0),
                    seed(),
                ],
            ]),
            |_, p| {
                let pair = |k: &str, d: [f64; 2]| {
                    let v = p.floats(k);
                    [v.first().copied().unwrap_or(d[0]), v.get(1).copied().unwrap_or(v.first().copied().unwrap_or(d[1]))]
                };
                lattices::level_set_foam(
                    &grid(p)?,
                    &lattices::LevelSetFoamParams {
                        seed_radius: pair("seed_radius", [6.0, 9.0]),
                        aspect: pair("aspect", [1.0, 1.5]),
                        packing: p.f64("packing", 0.55),
                        fraction: p.f64("fraction", 0.2),
                        curvature: p.f64("curvature", -0.7),
                        c1: p.f64("c1", 50.0),
                        c2: p.f64("c2", 1.0),
                        wall: p.f64("wall", 0.0),
                        seed: seed_of(p),
                    },
                )
            },
        ),
        source(
            "scale_rich_network",
            "Scale-Rich Network",
            "Ligaments grown one after another, each thinner than the one before, from a random point of the voids until they meet a ligament or the box: thicknesses and lengths over orders of magnitude in one sample. Strips in a plane; plates or beams in space.",
            &["scale-rich", "hierarchical network", "power law", "ligaments", "sequential partition", "metamaterial", "plates", "beams", "closed cells"],
            join(vec![
                grid_params([256.0, 256.0, 1.0], None),
                vec![
                    i("ligaments", "Ligaments", 500, 1, 1_000_000),
                    f("decay", "Thinning exponent", 0.5, 0.0, 3.0).help("The ligament number t is t to the power minus this exponent as thick as the first: 0 for equal thicknesses, near 1 for a few thick ligaments and many thin ones."),
                    f("fraction", "Volume fraction (0: use the first thickness)", 0.4, 0.0, 0.99),
                    f("first", "First thickness", 12.0, 0.0, f64::INFINITY),
                    choice("element", "In space", &[lattices::ELEMENT_PLATES, lattices::ELEMENT_BEAMS], lattices::ELEMENT_PLATES).help("With more than one layer of cells: plates that close cells, or beams that leave them open."),
                    floats("angles", "Directions in the plane (degrees)", &[]).help("Empty: any direction. 0, 90: ligaments along X and Y only."),
                    b("aligned", "Aligned in space (layers across Z, beams along Z)", false).advanced(),
                    f("min_spacing", "Least room beside a new ligament", 1.0, 0.0, f64::INFINITY).advanced(),
                    f("frame", "Frame thickness", 0.0, 0.0, f64::INFINITY).help("Above 0, a frame of material closes the box.").advanced(),
                    i("attempts", "Tries before the network is jammed", 200, 1, 100_000).advanced(),
                    seed(),
                ],
            ]),
            |_, p| {
                lattices::scale_rich_network(
                    &grid(p)?,
                    &lattices::ScaleRichParams {
                        ligaments: p.i64("ligaments", 500).max(1) as usize,
                        decay: p.f64("decay", 0.5),
                        fraction: p.f64("fraction", 0.4),
                        first: p.f64("first", 12.0),
                        element: p.str("element"),
                        angles: p.floats("angles"),
                        aligned: p.bool("aligned", false),
                        min_spacing: p.f64("min_spacing", 1.0),
                        frame: p.f64("frame", 0.0),
                        attempts: p.i64("attempts", 200).max(1) as usize,
                        seed: seed_of(p),
                    },
                )
            },
        ),
        // ---------------- Automata and growth ----------------
        source(
            "cellular_automaton_structure",
            "Cellular Automaton Structure",
            "A two-phase structure grown by a cellular automaton from random cells: annealing coarsens the noise into two interwoven phases; majority rules and any rule of births and survivals are there too.",
            &["cellular automaton", "vichniac", "annealing rule", "majority rule", "life-like", "bicontinuous", "coarsening", "synthetic microstructure"],
            join(vec![
                grid_params(C64, Some(true)),
                vec![
                    choice("rule", "Rule", &[growth::RULE_ANNEAL, growth::RULE_MAJORITY, growth::RULE_CONSERVED, growth::RULE_CUSTOM], growth::RULE_ANNEAL),
                    choice("neighbours", "Neighbours", &[growth::NEIGHBOURS_MOORE, growth::NEIGHBOURS_FACES], growth::NEIGHBOURS_MOORE),
                    i("steps", "Steps", 40, 0, 100_000),
                    f("initial_fraction", "Live cells at the start (fraction)", 0.5, 0.0, 1.0),
                    crate::registry::s("births", "Births (live neighbours)", "14-26").help("For the rule of births and survivals: a dead cell comes alive with this many live neighbours (5-8, or 4, 6, 7).").advanced(),
                    crate::registry::s("survivals", "Survivals (live neighbours)", "13-26").advanced(),
                    seed(),
                ],
            ]),
            |_, p| {
                growth::cellular_automaton(
                    &grid(p)?,
                    &growth::AutomatonParams {
                        rule: p.str("rule"),
                        neighbours: p.str("neighbours"),
                        steps: p.i64("steps", 40).max(0) as usize,
                        initial_fraction: p.f64("initial_fraction", 0.5),
                        births: p.str("births"),
                        survivals: p.str("survivals"),
                        seed: seed_of(p),
                    },
                )
            },
        ),
        source(
            "activator_inhibitor_pattern",
            "Activator-Inhibitor Pattern",
            "Spots, stripes and labyrinths from short-range activation and longer-range inhibition between cells; the volume fraction chooses between them.",
            &["turing pattern", "activator inhibitor", "spots", "stripes", "labyrinth", "morphogenesis", "synthetic microstructure"],
            join(vec![
                grid_params(C64, Some(true)),
                vec![
                    f("activation", "Activation radius", 3.0, 1e-9, f64::INFINITY),
                    f("inhibition", "Inhibition radius", 7.0, 1e-9, f64::INFINITY),
                    fraction(0.35).help("Low: spots. Near one half: stripes and labyrinths."),
                    i("steps", "Steps", 15, 0, 10_000),
                    seed(),
                ],
            ]),
            |_, p| growth::activator_inhibitor(&grid(p)?, p.f64("activation", 3.0), p.f64("inhibition", 7.0), p.f64("fraction", 0.35), p.i64("steps", 15).max(0) as usize, seed_of(p)),
        ),
        source(
            "reaction_diffusion_pattern",
            "Reaction-Diffusion Pattern",
            "Patterns of two species that diffuse and react (Gray-Scott): spots, worms, labyrinths and holes, cut at a volume fraction. The concentration is kept as an array.",
            &["gray-scott", "reaction diffusion", "turing", "spots", "worms", "labyrinth", "pattern formation", "synthetic microstructure"],
            join(vec![
                grid_params(C64, Some(true)),
                vec![
                    choice("pattern", "Pattern", &growth::PATTERNS.iter().map(|x| x.0).collect::<Vec<_>>(), "Labyrinth"),
                    i("steps", "Steps", 3000, 1, 1_000_000),
                    fraction(0.4),
                    f("feed", "Feed rate (Custom)", 0.034, 0.0, 0.2).advanced(),
                    f("kill", "Kill rate (Custom)", 0.060, 0.0, 0.2).advanced(),
                    seed(),
                ],
            ]),
            |_, p| growth::reaction_diffusion(&grid(p)?, &p.str("pattern"), p.f64("feed", 0.034), p.f64("kill", 0.06), p.i64("steps", 3000).max(1) as usize, p.f64("fraction", 0.4), seed_of(p)),
        ),
        source(
            "aggregation_cluster",
            "Aggregation Cluster",
            "Aggregates grown one cell at a time: diffusion-limited (branched, fractal), Eden (compact with a rough surface) or ballistic (porous deposits); from seeds in the box or as a deposit on its base.",
            &["dla", "diffusion-limited aggregation", "eden", "ballistic deposition", "fractal cluster", "soot", "colloid", "deposit", "synthetic microstructure"],
            join(vec![
                grid_params(C64, Some(true)),
                vec![
                    choice("process", "Process", &[growth::PROCESS_DLA, growth::PROCESS_EDEN, growth::PROCESS_BALLISTIC], growth::PROCESS_DLA),
                    choice("geometry", "Grown from", &[growth::GEOMETRY_CLUSTER, growth::GEOMETRY_DEPOSIT], growth::GEOMETRY_CLUSTER),
                    i("seeds", "Seeds", 20, 1, 1_000_000),
                    fraction(0.08),
                    seed(),
                ],
            ]),
            |_, p| growth::aggregation(&grid(p)?, &p.str("process"), &p.str("geometry"), p.i64("seeds", 20).max(1) as usize, p.f64("fraction", 0.08), seed_of(p)),
        ),
        source(
            "pipe_network",
            "Pipe Network",
            "A network of pipes assembled from tiles on a cubic lattice: every tile opens towards some of its neighbours, and only tiles with an allowed number of open sides are used, so that pipes never end where they should not.",
            &["tile assembly", "building blocks", "compatible tiles", "pipes", "channels", "network", "combinatorial", "synthetic microstructure"],
            join(vec![
                grid_params(C64, Some(true)),
                vec![
                    i("tile_cells", "Cells per tile", 8, 2, 1000),
                    f("pipe_radius", "Pipe radius (tiles)", 0.18, 0.01, 0.5),
                    floats("degrees", "Open sides allowed", &[0.0, 2.0, 3.0, 4.0]).help("A tile may have this many open sides: without 1, no pipe ends in a dead end."),
                    f("open", "Open sides at the start (fraction)", 0.6, 0.0, 1.0),
                    seed(),
                ],
            ]),
            |_, p| growth::pipe_network(&grid(p)?, p.i64("tile_cells", 8).max(2) as usize, p.f64("pipe_radius", 0.18), &p.floats("degrees"), p.f64("open", 0.6), seed_of(p)),
        ),
        source(
            "branching_network",
            "Branching Network",
            "A branching tree grown towards points scattered in the box (vessels, roots, corals), its radii thickening from the tips to the root. The radius of each branch is kept as an array.",
            &["space colonization", "vascular tree", "murray's law", "roots", "coral", "dendritic", "blood vessels", "tree", "synthetic microstructure"],
            join(vec![
                grid_params(C64, None),
                vec![
                    i("attractors", "Points to reach", 3000, 1, 2_000_000),
                    f("step", "Segment length", 1.5, 1e-9, f64::INFINITY),
                    f("influence", "Reach of a point (0: any distance)", 0.0, 0.0, f64::INFINITY).advanced(),
                    f("kill", "A point is reached within (0: two segments)", 0.0, 0.0, f64::INFINITY).advanced(),
                    i("roots", "Roots", 1, 1, 1000),
                    choice("root", "First root at", &[growth::ROOT_BASE, growth::ROOT_CENTRE], growth::ROOT_BASE),
                    f("tip_radius", "Radius at the tips", 0.6, 1e-9, f64::INFINITY),
                    f("exponent", "Branching exponent", 3.0, 0.5, 10.0).help("The radius r of a branch and those of its children: r^n is the sum of theirs. 3 for vessels (least work of the flow), 2 for plants (the area is kept)."),
                    seed(),
                ],
            ]),
            |_, p| {
                growth::branching_network(
                    &grid(p)?,
                    &growth::BranchingParams {
                        attractors: p.i64("attractors", 3000).max(1) as usize,
                        step: p.f64("step", 1.5),
                        influence: p.f64("influence", 0.0),
                        kill: p.f64("kill", 0.0),
                        roots: p.i64("roots", 1).max(1) as usize,
                        root: p.str("root"),
                        tip_radius: p.f64("tip_radius", 0.6),
                        exponent: p.f64("exponent", 3.0),
                        seed: seed_of(p),
                    },
                )
            },
        ),
        // ---------------- Bio-inspired ----------------
        source(
            "nacre",
            "Nacre",
            "Brick and mortar: layers of mineral tablets in a thin mortar, the tablets square bricks shifted from layer to layer or the polygons of random points.",
            &["brick and mortar", "mother of pearl", "tablets", "staggered", "layered composite", "bio-inspired", "synthetic microstructure"],
            join(vec![
                grid_params([96.0, 96.0, 48.0], Some(true)),
                vec![
                    choice("arrangement", "Tablets", &[bio::NACRE_BRICK, bio::NACRE_POLYGON], bio::NACRE_BRICK),
                    f("tablet_size", "Tablet size", 24.0, 1e-9, f64::INFINITY),
                    f("tablet_thickness", "Tablet thickness", 8.0, 1e-9, f64::INFINITY),
                    f("mortar", "Mortar thickness", 4.0, 0.0, f64::INFINITY),
                    f("shift", "Shift from layer to layer (tablets)", 0.5, 0.0, 1.0),
                    seed(),
                ],
            ]),
            |_, p| bio::nacre(&grid(p)?, &p.str("arrangement"), p.f64("tablet_size", 24.0), p.f64("tablet_thickness", 8.0), p.f64("mortar", 4.0), p.f64("shift", 0.5), seed_of(p)),
        ),
        source(
            "twisted_plywood",
            "Twisted Plywood",
            "Layers of parallel fibres whose direction turns a little from each layer to the next, as in the shells of crustaceans and the scales of fish.",
            &["bouligand", "helicoidal", "plywood", "chitin", "cuticle", "twisted fibres", "bio-inspired", "laminate", "synthetic microstructure"],
            join(vec![
                grid_params(C64, None),
                vec![
                    f("radius", "Fibre radius", 2.5, 1e-9, f64::INFINITY),
                    f("fibre_spacing", "Distance between fibres", 7.0, 1e-9, f64::INFINITY),
                    f("layer", "Layer thickness", 7.0, 1e-9, f64::INFINITY),
                    f("pitch", "Turn per layer (degrees)", 15.0, -180.0, 180.0),
                    f("start", "Direction of the first layer (degrees)", 0.0, -360.0, 360.0).advanced(),
                ],
            ]),
            |_, p| bio::twisted_plywood(&grid(p)?, p.f64("radius", 2.5), p.f64("fibre_spacing", 7.0), p.f64("layer", 7.0), p.f64("pitch", 15.0), p.f64("start", 0.0)),
        ),
        source(
            "honeycomb",
            "Honeycomb",
            "Prismatic cells along Z with thin walls: hexagons, squares, random cells, or wood (rows of cells whose size and walls change through the growth rings, with rays between the rows).",
            &["honeycomb", "hexagonal cells", "prismatic", "wood", "softwood", "growth rings", "tracheids", "rays", "cellular solid", "bio-inspired"],
            join(vec![
                grid_params(C64, Some(true)),
                vec![
                    choice("arrangement", "Cells", &[bio::COMB_HEXAGON, bio::COMB_SQUARE, bio::COMB_RANDOM, bio::COMB_WOOD], bio::COMB_HEXAGON),
                    f("cell", "Cell size", 16.0, 1e-9, f64::INFINITY),
                    f("wall", "Wall thickness", 2.0, 1e-9, f64::INFINITY),
                    f("ring", "Wood: width of a growth ring", 64.0, 1e-9, f64::INFINITY).advanced(),
                    f("latewood", "Wood: latewood share of a ring", 0.3, 0.0, 0.95).advanced(),
                    f("latewood_size", "Wood: latewood cell size over earlywood", 0.4, 0.05, 1.0).advanced(),
                    f("latewood_wall", "Wood: latewood wall over earlywood", 2.0, 0.1, 10.0).advanced(),
                    f("rays", "Wood: rays (volume fraction)", 0.05, 0.0, 0.9).advanced(),
                    f("ray_height", "Wood: height of a ray", 12.0, 0.0, f64::INFINITY).advanced(),
                    seed(),
                ],
            ]),
            |_, p| {
                bio::honeycomb(
                    &grid(p)?,
                    &bio::HoneycombParams {
                        arrangement: p.str("arrangement"),
                        cell: p.f64("cell", 16.0),
                        wall: p.f64("wall", 2.0),
                        ring: p.f64("ring", 64.0),
                        latewood: p.f64("latewood", 0.3),
                        latewood_size: p.f64("latewood_size", 0.4),
                        latewood_wall: p.f64("latewood_wall", 2.0),
                        rays: p.f64("rays", 0.05),
                        ray_height: p.f64("ray_height", 12.0),
                        seed: seed_of(p),
                    },
                )
            },
        ),
        source(
            "haversian_bone",
            "Haversian Bone",
            "The cross-section of cortical bone: osteons with a canal on their axis and a cement line around them, each laid over the older ones, with interstitial bone between them. The fractions the model gives exactly are kept with the result.",
            &["osteons", "cortical bone", "haversian canal", "cement line", "bone remodelling", "dead leaves", "bio-inspired", "synthetic microstructure"],
            join(vec![
                grid_params([192.0, 192.0, 4.0], Some(true)),
                vec![
                    f("radius", "Osteon radius", 20.0, 1e-9, f64::INFINITY),
                    f("radius_sd", "Radius standard deviation", 3.0, 0.0, f64::INFINITY),
                    f("coverage", "Area covered by osteons", 0.65, 0.0, 0.999),
                    f("canal", "Canal radius over the osteon radius", 0.2, 0.0, 0.9),
                    f("cement", "Cement line thickness", 1.5, 0.0, f64::INFINITY),
                    seed(),
                ],
            ]),
            |_, p| bio::haversian_bone(&grid(p)?, p.f64("radius", 20.0), p.f64("radius_sd", 3.0), p.f64("coverage", 0.65), p.f64("canal", 0.2), p.f64("cement", 1.5), seed_of(p)),
        ),
        // ---------------- Filters on microstructures ----------------
        FilterDef {
            id: "coat_phase",
            label: "Coat Phase",
            category: "Grains",
            description: "A layer of a new phase around a phase of a microstructure, or just inside it: coatings and interphases around particles and fibres, hollow spheres, tubes and struts.",
            keywords: &["coating", "interphase", "shell", "core-shell", "hollow", "dilate phase", "layer around particles"],
            is_source: false,
            ext: None,
            temporal: None,
            multi: None,
            params: vec![
                ParamSpec::new("phases", "Phases", ParamKind::Array { scalar_only: true, vector_only: false }, ParamValue::Array { assoc: Association::Cells, name: String::new() }).help("Empty: Phases.").advanced(),
                i("phase", "Phase to coat", 2, 0, 1_000_000),
                f("thickness", "Thickness", 2.0, 0.0, f64::INFINITY),
                choice("side", "Side", &["Outside", "Inside"], "Outside").help("Outside: a coating around the phase. Inside: a shell, the outer layer of the phase itself."),
                i("new_phase", "Number of the new phase (0: the next free one)", 0, 0, 1_000_000).advanced(),
                b("periodic", "Through periodic faces", false).help("On when the microstructure was made periodic."),
            ],
            apply: |input, p| {
                let name = p.array("phases").map(|a| a.1).unwrap_or_default();
                bio::coat_phase(input.ok_or_else(|| anyhow!("Coat Phase needs an input"))?, &name, p.i64("phase", 2) as i32, p.f64("thickness", 2.0), p.str("side") == "Inside", p.i64("new_phase", 0) as i32, p.bool("periodic", false))
            },
        },
        FilterDef {
            id: "combine_microstructures",
            label: "Combine Microstructures",
            category: "Grains",
            description: "Two microstructures on the same grid combined through a phase of each: union, intersection, difference, the second as inclusions over the first, or the second inside the phase of the first.",
            keywords: &["union", "intersection", "difference", "nested", "inclusions in a matrix", "boolean operation", "combine phases", "multiscale"],
            is_source: false,
            ext: None,
            temporal: None,
            multi: Some(MultiInput {
                label: "Second microstructure",
                min: 2,
                max: Some(2),
                apply: |inputs, p| {
                    if inputs.len() < 2 {
                        return Err(anyhow!("Combine Microstructures needs a second microstructure"));
                    }
                    bio::combine(inputs[0], inputs[1], &p.str("operation"), p.i64("phase", 2) as i32, p.i64("second_phase", 2) as i32)
                },
            }),
            params: vec![
                choice("operation", "Operation", &[bio::COMBINE_UNION, bio::COMBINE_INTERSECTION, bio::COMBINE_DIFFERENCE, bio::COMBINE_OVERLAY, bio::COMBINE_NESTED], bio::COMBINE_UNION),
                i("phase", "Phase of the first", 2, 0, 1_000_000),
                i("second_phase", "Phase of the second", 2, 0, 1_000_000),
            ],
            apply: |_, _| Err(anyhow!("Combine Microstructures needs a second microstructure")),
        },
    ]
}

fn lattice_params(disordered: bool) -> Vec<ParamSpec> {
    let mut v = grid_params([64.0, 64.0, 64.0], Some(true));
    v.extend([
        choice("cell", "Cell", lattices::CELLS, "Octet"),
        f("cell_size", "Cell size of the lattice", if disordered { 16.0 } else { 32.0 }, 1e-9, f64::INFINITY)
            .help("The lattice is periodic when the box holds a whole number of cells."),
        f("fraction", "Volume fraction (0: use the radius)", if disordered { 0.0 } else { 0.15 }, 0.0, 0.95),
        f("radius", "Strut radius", if disordered { 1.2 } else { 2.5 }, 1e-9, f64::INFINITY),
        f("wall", "Wall thickness (0: solid struts)", 0.0, 0.0, f64::INFINITY),
    ]);
    if disordered {
        v.extend([
            f("coordination", "Struts per node (0: all)", 7.0, 0.0, 100.0).help("Struts are removed at random until a node keeps this many on average."),
            f("perturbation", "Node displacement (fraction of the cell)", 0.1, 0.0, 0.5),
            seed(),
        ]);
    } else {
        v.extend([
            f("radius_top", "Radius at the far face (graded)", f64::NAN, 0.0, f64::INFINITY)
                .help("Empty: the same radius everywhere. A value: the radius changes evenly from Strut radius at the near face to this at the far face.")
                .advanced(),
            choice("gradient_axis", "Graded along", &["X", "Y", "Z"], "Z").advanced(),
            ParamSpec::new("rotation", "Rotation (Bunge angles, degrees)", ParamKind::Vec3, ParamValue::Vec3([0.0; 3])).advanced(),
        ]);
    }
    v
}

fn lattice_of(p: &Params) -> lattices::LatticeParams {
    lattices::LatticeParams {
        cell: p.str("cell"),
        cell_size: p.f64("cell_size", 32.0),
        radius: p.f64("radius", 2.5),
        fraction: p.f64("fraction", 0.0),
        wall: p.f64("wall", 0.0),
        radius_top: p.f64("radius_top", f64::NAN),
        gradient_axis: axis_of(p, "gradient_axis"),
        rotation: p.vec3("rotation", [0.0; 3]),
        coordination: p.f64("coordination", 0.0),
        perturbation: p.f64("perturbation", 0.0),
        seed: seed_of(p),
    }
}
