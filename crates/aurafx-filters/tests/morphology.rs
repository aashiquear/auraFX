//! The morphological models of microstructures (the types of the Synthetic Microstructure
//! window), checked against what each model gives in closed form: the volume fraction of
//! overlapping spheres and fibres, the hard-core distances of packings, the fractions of a
//! fractal and of osteons, the kinetics of nucleation and growth, the degree of a Delaunay
//! network.

use aurafx_data::*;
use aurafx_filters::morphology::FAMILIES;
use serde_json::json;

fn run(id: &str, input: Option<&DataSet>, set: &[(&str, serde_json::Value)]) -> DataSet {
    try_run(id, input, set).unwrap_or_else(|e| panic!("{id}: {e:#}"))
}

fn try_run(id: &str, input: Option<&DataSet>, set: &[(&str, serde_json::Value)]) -> anyhow::Result<DataSet> {
    let def = aurafx_filters::registry::find(id).unwrap_or_else(|| panic!("no filter {id}"));
    let mut p = def.default_params();
    for (k, v) in set {
        assert!(p.get(k).is_some(), "{id} has no setting {k}");
        p.set_json(k, v);
    }
    def.run(input, &p)
}

fn phases(ds: &DataSet) -> Vec<i32> {
    let a = ds.cell_data().unwrap().get("Phases").expect("Phases");
    (0..a.len()).map(|c| a.get(c, 0) as i32).collect()
}

fn share(ds: &DataSet, phase: i32) -> f64 {
    let p = phases(ds);
    p.iter().filter(|x| **x == phase).count() as f64 / p.len() as f64
}

fn field(ds: &DataSet, name: &str) -> Vec<f64> {
    let a = ds.field_data().get(name).unwrap_or_else(|| panic!("no field value {name}"));
    (0..a.len()).map(|k| a.get(k, 0)).collect()
}

fn close(what: &str, got: f64, want: f64, tolerance: f64) {
    assert!((got - want).abs() <= tolerance, "{what}: {got} for {want} (within {tolerance})");
}

#[test]
fn every_type_is_a_source_that_runs_with_its_standard_settings() {
    let mut seen = std::collections::HashSet::new();
    for (family, ids) in FAMILIES {
        for id in *ids {
            assert!(seen.insert(*id), "{id} listed twice");
            let def = aurafx_filters::registry::find(id).unwrap_or_else(|| panic!("{family}: no source {id}"));
            assert!(def.is_source, "{id} is not a source");
            assert!(aurafx_filters::morphology::is_microstructure(id));
            if matches!(*id, "voronoi_grains" | "pack_grains" | "columnar_grains") {
                continue;
            }
            let start = std::time::Instant::now();
            let out = run(id, None, &[]);
            let took = start.elapsed().as_secs_f64();
            if *id == "microstructure_study" {
                // A table: a row for every value of the setting it varies.
                let DataSet::Table(t) = &out else { panic!("the study gives a table") };
                assert_eq!(t.n_rows(), 5);
                continue;
            }
            let p = phases(&out);
            let fractions = field(&out, "PhaseFractions");
            close(&format!("{id}: the fractions sum"), fractions.iter().sum::<f64>(), 1.0, 1e-9);
            assert!(p.iter().all(|x| *x >= 1), "{id}: a cell without a phase");
            if !matches!(*id, "nucleation_growth_grains" | "microstructure_formation") {
                assert!(fractions.len() >= 3 && fractions[2] > 0.0 && fractions[1] > 0.0, "{id}: one phase only ({fractions:?})");
            }
            // The same seed gives the same structure.
            if took < 1.5 {
                assert_eq!(phases(&run(id, None, &[])), p, "{id}: not the same twice");
            }
            println!("{id:32} {took:6.2} s  fractions {:?}", fractions.iter().skip(1).map(|x| (x * 1000.0).round() / 1000.0).collect::<Vec<_>>());
            assert!(took < 30.0, "{id}: {took} s with its standard settings");
        }
    }
    assert_eq!(seen.len(), 43);
}

#[test]
fn overlapping_spheres_and_fibres_take_the_fraction_of_the_boolean_model() {
    // 1 - exp(-theta V): the mean over a few seeds, on a periodic box.
    let mean = |id: &str, set: &[(&str, serde_json::Value)]| -> f64 {
        (1..=4)
            .map(|s| {
                let mut all = set.to_vec();
                all.push(("seed", json!(s)));
                share(&run(id, None, &all), 2)
            })
            .sum::<f64>()
            / 4.0
    };
    close("spheres", mean("overlapping_spheres", &[("radius", json!(5.0)), ("fraction", json!(0.3))]), 0.3, 0.02);
    close("spheres of spread radii", mean("overlapping_spheres", &[("radius", json!(4.0)), ("radius_sigma", json!(0.3)), ("fraction", json!(0.5))]), 0.5, 0.03);
    close("cylinders", mean("overlapping_fibres", &[("radius", json!(2.5)), ("length", json!(24.0)), ("fraction", json!(0.2))]), 0.2, 0.02);
    close(
        "round fibres along Z",
        mean(
            "overlapping_fibres",
            &[("radius", json!(2.5)), ("length", json!(20.0)), ("round", json!(true)), ("directions", json!("Along Z")), ("fraction", json!(0.25))],
        ),
        0.25,
        0.025,
    );
    close("fibres without ends", mean("poisson_line_fibres", &[("radius", json!(3.0)), ("fraction", json!(0.2))]), 0.2, 0.035);
    // In one layer of cells: discs.
    close("discs", mean("overlapping_spheres", &[("cells", json!([200, 200, 1])), ("radius", json!(6.0)), ("fraction", json!(0.4))]), 0.4, 0.03);
}

fn least_gap(ds: &DataSet, size: f64) -> f64 {
    let fd = ds.field_data();
    let (c, r) = (fd.get("Centers").unwrap(), fd.get("Radii").unwrap());
    let n = r.len();
    let mut least = f64::INFINITY;
    for i in 1..n {
        for j in i + 1..n {
            let d2: f64 = (0..3)
                .map(|k| {
                    let d = c.get(i, k) - c.get(j, k);
                    (d - size * (d / size).round()).powi(2)
                })
                .sum();
            least = least.min(d2.sqrt() - r.get(i, 0) - r.get(j, 0));
        }
    }
    least
}

#[test]
fn hard_spheres_do_not_overlap() {
    let rsa = run("hard_spheres", None, &[("fraction", json!(0.3)), ("gap", json!(0.5))]);
    assert!(least_gap(&rsa, 64.0) >= 0.5 - 1e-9, "least gap {}", least_gap(&rsa, 64.0));
    close("fraction by sequential addition", share(&rsa, 2), 0.3, 0.02);
    // Sequential addition jams well below a dense packing, and says so.
    let jam = try_run("hard_spheres", None, &[("fraction", json!(0.5)), ("gap", json!(0.0))]);
    assert!(jam.unwrap_err().to_string().contains("jams"), "no jamming message");
    let dense = run("hard_spheres", None, &[("fraction", json!(0.58)), ("gap", json!(0.0)), ("method", json!("Dense packing")), ("radius", json!(5.0))]);
    assert!(least_gap(&dense, 64.0) >= -1e-3, "dense packing overlaps by {}", -least_gap(&dense, 64.0));
    close("fraction of the dense packing", share(&dense, 2), 0.58, 0.02);
    // Spread radii pack too.
    let spread = run(
        "hard_spheres",
        None,
        &[("fraction", json!(0.55)), ("gap", json!(0.0)), ("method", json!("Dense packing")), ("radius", json!(5.0)), ("radius_sigma", json!(0.2))],
    );
    assert!(least_gap(&spread, 64.0) >= -1e-3);
    // Penetrable spheres: the cores keep apart, the spheres overlap.
    let pen = run("penetrable_spheres", None, &[("radius", json!(6.0)), ("impenetrability", json!(0.5)), ("density", json!(0.6))]);
    let gap = least_gap(&pen, 64.0);
    assert!(gap >= -6.0 - 1e-9 && gap < 0.0, "cores of penetrable spheres: gap {gap}");
    assert!(share(&pen, 2) < 0.6 && share(&pen, 2) > 0.4);
    // Impenetrability 0 is the Boolean model: 1 - exp(-density).
    let free: f64 = (1..=4)
        .map(|s| share(&run("penetrable_spheres", None, &[("impenetrability", json!(0.0)), ("density", json!(0.5)), ("seed", json!(s))]), 2))
        .sum::<f64>()
        / 4.0;
    close("penetrable spheres without cores", free, 1.0 - (-0.5f64).exp(), 0.03);
}

#[test]
fn clustered_particles_sit_in_their_aggregates() {
    let c = run("clustered_spheres", None, &[]);
    let f = field(&c, "PhaseFractions");
    // Particles (2) and the rest of the aggregates (3) together are the aggregates.
    close("aggregates", f[2] + f[3], 0.25, 0.06);
    close("particles in the aggregates", f[2] / (f[2] + f[3]), 0.35, 0.05);
    let two = run("clustered_spheres", None, &[("aggregate_phase", json!(false)), ("hard", json!(false))]);
    assert_eq!(field(&two, "PhaseFractions").len(), 3);
}

#[test]
fn fibres_without_overlap() {
    let h = run("hard_fibres", None, &[]);
    close("hard fibres", share(&h, 2), 0.12, 0.015);
    // No cell is claimed twice: the volume is that of the fibres placed.
    let n = field(&h, "Fibres")[0];
    let one = std::f64::consts::PI * 4.0 * 24.0 + 4.0 / 3.0 * std::f64::consts::PI * 8.0;
    close("volume of the fibres", share(&h, 2) * 64f64.powi(3), n * one, 0.03 * n * one);
    let jam = try_run("hard_fibres", None, &[("fraction", json!(0.6))]);
    assert!(jam.is_err());
    let hollow = run("hard_fibres", None, &[("radius", json!(4.0)), ("lumen", json!(0.5)), ("fraction", json!(0.12))]);
    close("hollow fibres keep 3/4 of their section", share(&hollow, 2) / 0.12, 0.75, 0.12);
    // Bending fibres end nearly free of overlap, at their fraction.
    let b = run("bending_fibres", None, &[]);
    assert!(field(&b, "LargestOverlap")[0] < 0.25, "overlap {} radii", field(&b, "LargestOverlap")[0]);
    close("bending fibres", share(&b, 2), 0.25, 0.03);
    // Curved fibres: arcs and helices without overlap.
    for shape in ["Arc", "Helix", "Worm-like chain"] {
        let c = run("curved_fibres", None, &[("shape", json!(shape)), ("overlap", json!(false)), ("fraction", json!(0.05)), ("length", json!(40.0))]);
        close(shape, share(&c, 2), 0.05, 0.01);
    }
    // Cross-links: a third phase at a share of the contacts.
    let x = run("cross_linked_fibres", None, &[]);
    let (contacts, links) = (field(&x, "Contacts")[0], field(&x, "CrossLinks")[0]);
    assert!(contacts > 20.0 && links > 0.2 * contacts && links < 0.8 * contacts, "{links} links at {contacts} contacts");
    assert!(share(&x, 3) > 0.0);
    let all = run("cross_linked_fibres", None, &[("bonded", json!(1.0)), ("length", json!(30.0)), ("periodic", json!(true))]);
    assert_eq!(field(&all, "Contacts"), field(&all, "CrossLinks"));
}

#[test]
fn delaunay_network_has_the_degree_of_random_points() {
    // 15.54 edges meet at a point of the Delaunay triangulation of Poisson points, on average.
    let d = run("delaunay_fibre_network", None, &[("points", json!(400))]);
    close("mean degree", field(&d, "MeanDegree")[0], 15.54, 0.5);
    let regular = run("delaunay_fibre_network", None, &[("points", json!(400)), ("regularity", json!(0.9)), ("fraction", json!(0.2))]);
    close("fraction", share(&regular, 2), 0.2, 1e-3);
    assert!(field(&regular, "MeanDegree")[0] < 15.3, "kept-apart points have fewer neighbours");
}

#[test]
fn fields_are_cut_at_their_fraction() {
    for (id, set) in [
        ("gaussian_field_medium", vec![("fraction", json!(0.37))]),
        ("gaussian_field_medium", vec![("fraction", json!(0.37)), ("kind", json!("Between two levels")), ("periodic", json!(false))]),
        ("transparent_leaves_medium", vec![("fraction", json!(0.37))]),
        ("triply_periodic_surface", vec![("fraction", json!(0.37)), ("surface", json!("Schwarz D")), ("variant", json!("Network"))]),
        ("activator_inhibitor_pattern", vec![("fraction", json!(0.37))]),
        ("reaction_diffusion_pattern", vec![("fraction", json!(0.37)), ("cells", json!([40, 40, 40])), ("steps", json!(1500))]),
    ] {
        close(id, share(&run(id, None, &set), 2), 0.37, 2e-4);
    }
    // A longer correlation length gives larger features: less surface between the phases.
    let surface = |length: f64| -> usize {
        let p = phases(&run("gaussian_field_medium", None, &[("length", json!(length)), ("fraction", json!(0.5))]));
        (0..p.len() - 1).filter(|c| (c + 1) % 64 != 0 && p[*c] != p[c + 1]).count()
    };
    assert!(surface(4.0) > 2 * surface(12.0));
}

#[test]
fn dead_leaves_make_a_mosaic_of_phases() {
    let d = run("dead_leaves_mosaic", None, &[("phase_fractions", json!([0.7, 0.3])), ("seed", json!(3))]);
    let f = field(&d, "PhaseFractions");
    close("phase 1", f[1], 0.7, 0.08);
    let ids = d.cell_data().unwrap().get("FeatureIds").unwrap();
    let top = (0..ids.len()).map(|c| ids.get(c, 0) as i32).max().unwrap();
    assert!(top > 50, "{top} leaves show");
    assert_eq!(d.cell_data().unwrap().get("EulerAngles").unwrap().ncomp, 3);
    // A finite number of leaves leaves a background: exp(-coverage) of the box.
    let part: f64 = (1..=4)
        .map(|s| {
            *field(
                &run(
                    "dead_leaves_mosaic",
                    None,
                    &[("coverage", json!(1.0)), ("phase_fractions", json!([1.0])), ("radius_sigma", json!(0.0)), ("seed", json!(s))],
                ),
                "PhaseFractions",
            )
            .last()
            .unwrap()
        })
        .sum::<f64>()
        / 4.0;
    close("background", part, (-1.0f64).exp(), 0.04);
    // Larger leaves for one phase, and its fraction all the same.
    let big = run(
        "dead_leaves_mosaic",
        None,
        &[("phase_fractions", json!([0.5, 0.5])), ("phase_radii", json!([5.0, 12.0])), ("radius_sigma", json!(0.0)), ("seed", json!(2))],
    );
    close("phases of unequal leaves", field(&big, "PhaseFractions")[1], 0.5, 0.1);
}

#[test]
fn reconstruction_comes_near_its_target() {
    let start = run("reconstructed_medium", None, &[("cells", json!([32, 32, 32])), ("sweeps", json!(0.0))]);
    let done = run("reconstructed_medium", None, &[("cells", json!([32, 32, 32])), ("sweeps", json!(20.0))]);
    let (e0, e1) = (field(&start, "Energy")[0], field(&done, "Energy")[0]);
    assert!(e1 < 0.02 * e0, "energy {e0} -> {e1}");
    let (want, got) = (field(&done, "TwoPointTarget"), field(&done, "TwoPointResult"));
    for (w, g) in want.iter().zip(&got) {
        close("two-point probability", *g, *w, 0.01);
    }
    close("fraction", share(&done, 2), 0.3, 1e-3);
}

#[test]
fn lattices_and_fractals_have_their_exact_fractions() {
    // Cubic struts of radius r in a cell of 1: 3 pi r^2 - 8 sqrt(2) r^3.
    let r: f64 = 0.15;
    let cubic = run(
        "strut_lattice",
        None,
        &[("cells", json!([80, 80, 80])), ("cell", json!("Cubic")), ("cell_size", json!(40.0)), ("fraction", json!(0.0)), ("radius", json!(r * 40.0))],
    );
    close("cubic lattice", share(&cubic, 2), 3.0 * std::f64::consts::PI * r * r - 8.0 * 2f64.sqrt() * r.powi(3), 0.006);
    assert_eq!(field(&cubic, "MeanCoordination")[0], 6.0);
    for (cell, z) in [("Body-centred cubic", 8.0), ("Face-centred cubic", 6.0), ("Octet", 12.0), ("Kelvin", 4.0), ("Diamond", 4.0)] {
        let l = run("strut_lattice", None, &[("cell", json!(cell)), ("fraction", json!(0.2))]);
        // Whole shells of cells at one distance from the struts come in together.
        close(cell, share(&l, 2), 0.2, 0.015);
        assert_eq!(field(&l, "MeanCoordination")[0], z, "{cell}");
    }
    // Hollow struts hold less, a graded lattice more at its far face, a turned one the same.
    let hollow = run("strut_lattice", None, &[("fraction", json!(0.0)), ("radius", json!(4.0)), ("wall", json!(1.5))]);
    let solid = run("strut_lattice", None, &[("fraction", json!(0.0)), ("radius", json!(4.0))]);
    assert!(share(&hollow, 2) < 0.8 * share(&solid, 2));
    let graded = phases(&run("strut_lattice", None, &[("fraction", json!(0.0)), ("radius", json!(1.5)), ("radius_top", json!(4.5))]));
    let half = graded.len() / 2;
    let count = |s: &[i32]| s.iter().filter(|x| **x == 2).count();
    assert!(count(&graded[half..]) > 2 * count(&graded[..half]));
    let turned =
        run("strut_lattice", None, &[("fraction", json!(0.0)), ("radius", json!(3.0)), ("rotation", json!([30.0, 40.0, 10.0])), ("periodic", json!(false))]);
    let straight = run("strut_lattice", None, &[("fraction", json!(0.0)), ("radius", json!(3.0)), ("periodic", json!(false))]);
    close("turned lattice", share(&turned, 2), share(&straight, 2), 0.04);
    // Disorder: the coordination asked, and Maxwell's count below 0 under 6.
    let d = run("disordered_strut_lattice", None, &[("coordination", json!(5.0))]);
    close("coordination", field(&d, "MeanCoordination")[0], 5.0, 0.4);
    // A radius that single precision rounds down must not fill the box.
    for radius in [1.3, 1.2, 1.7] {
        let l = run("disordered_strut_lattice", None, &[("radius", json!(radius))]);
        assert!(share(&l, 2) < 0.5, "radius {radius}: {}", share(&l, 2));
        let n = run("delaunay_fibre_network", None, &[("radius", json!(radius)), ("points", json!(100))]);
        assert!(share(&n, 2) < 0.6, "network radius {radius}: {}", share(&n, 2));
    }
    assert!(field(&d, "MaxwellNumber")[0] < 0.0);
    // The Menger sponge at level 3 on 27 cells: (20/27)^3 exactly.
    for (pattern, kept) in [("Menger sponge", 20.0f64), ("Vicsek fractal", 7.0), ("Cantor dust", 8.0)] {
        let s = run("fractal_sponge", None, &[("cells", json!([27, 27, 27])), ("cell_size", json!(27.0)), ("pattern", json!(pattern))]);
        close(pattern, share(&s, 2), (kept / 27.0).powi(3), 1e-12);
        close(pattern, field(&s, "ExpectedFraction")[0], (kept / 27.0).powi(3), 1e-12);
    }
    // Frames of octahedra: 36 tubes for each of the 36 struts of a cell.
    let h = run("hierarchical_lattice", None, &[("cells", json!([64, 64, 64])), ("cell_size", json!(64.0)), ("radius", json!(1.2))]);
    close("tubes per cell", field(&h, "TubesPerCell")[0], 1296.0, 1e-9);
    let plain = run(
        "hierarchical_lattice",
        None,
        &[("cells", json!([64, 64, 64])), ("cell_size", json!(64.0)), ("radius", json!(1.2)), ("generations", json!(0)), ("cube_edges", json!(false))],
    );
    close("struts per cell of the octet truss", field(&plain, "TubesPerCell")[0], 24.0, 1e-9);
}

#[test]
fn foams() {
    let f = run("open_cell_foam", None, &[("fraction", json!(0.12))]);
    close("foam fraction", share(&f, 2), 0.12, 1e-3);
    // A cell with F faces has 3 (F - 2) edges, each shared by three cells: 13.5 struts per cell
    // for the 15.5 faces of random cells, fewer for the more even cells after relaxation.
    let per_cell = field(&f, "Struts")[0] / 64.0;
    assert!(per_cell > 11.5 && per_cell < 13.6, "{per_cell} struts per cell");
    let raw = run("open_cell_foam", None, &[("relaxation", json!(0)), ("foam_cells", json!(300))]);
    close("struts per cell of random cells", field(&raw, "Struts")[0] / 300.0, 13.54, 0.6);
    let long = run("open_cell_foam", None, &[("fraction", json!(0.0)), ("radius", json!(1.5)), ("anisotropy", json!(2.0))]);
    assert!(share(&long, 2) > 0.02);
    let l = run("level_set_foam", None, &[("fraction", json!(0.2))]);
    close("level-set foam fraction", share(&l, 2), 0.2, 2e-3);
    let closed = run("level_set_foam", None, &[("fraction", json!(0.2)), ("wall", json!(1.0))]);
    close("closed cells", share(&closed, 2), 0.2, 2e-3);
    assert!(field(&closed, "Thickness")[0] < field(&l, "Thickness")[0], "walls take part of the solid");
}

#[test]
fn scale_rich_networks() {
    let s = run("scale_rich_network", None, &[]);
    close("fraction in the plane", share(&s, 2), 0.4, 0.01);
    assert!(field(&s, "Ligaments")[0] > 300.0);
    let aligned =
        phases(&run("scale_rich_network", None, &[("angles", json!([0.0, 90.0])), ("fraction", json!(0.0)), ("first", json!(10.0)), ("ligaments", json!(50))]));
    // Ligaments along X and Y: rows and columns of cells that are solid from end to end of a void.
    assert!(aligned.iter().filter(|x| **x == 2).count() > 1000);
    let plates = run("scale_rich_network", None, &[("cells", json!([48, 48, 48])), ("ligaments", json!(200)), ("fraction", json!(0.3))]);
    close("plates", share(&plates, 2), 0.3, 0.015);
    let beams =
        run("scale_rich_network", None, &[("cells", json!([48, 48, 48])), ("ligaments", json!(120)), ("fraction", json!(0.1)), ("element", json!("Beams"))]);
    close("beams", share(&beams, 2), 0.1, 0.02);
    let framed = phases(&run("scale_rich_network", None, &[("frame", json!(4.0)), ("ligaments", json!(20))]));
    assert!(framed[..256 * 4].iter().all(|x| *x == 2), "the frame is solid");
}

#[test]
fn nucleation_and_growth_follows_its_kinetics() {
    // Stopped at a time: the transformed fraction of the Johnson-Mehl-Avrami-Kolmogorov law.
    for (nucleation, time) in [("At a constant rate", 14.0), ("All at the start", 5.0)] {
        let mean: Vec<(f64, f64)> = (1..=4)
            .map(|s| {
                let g = run(
                    "nucleation_growth_grains",
                    None,
                    &[("nucleation", json!(nucleation)), ("time", json!(time)), ("periodic", json!(true)), ("seed", json!(s))],
                );
                (field(&g, "TransformedFraction")[0], field(&g, "ExpectedFraction")[0])
            })
            .collect();
        let got = mean.iter().map(|m| m.0).sum::<f64>() / 4.0;
        let want = mean.iter().map(|m| m.1).sum::<f64>() / 4.0;
        assert!(want > 0.15 && want < 0.9, "{nucleation}: {want}");
        close(nucleation, got, want, 0.04);
    }
    // To the end: every cell in a grain, grains of one piece with orientations.
    let full = run("nucleation_growth_grains", None, &[]);
    assert_eq!(field(&full, "TransformedFraction")[0], 1.0);
    let ids = full.cell_data().unwrap().get("FeatureIds").unwrap();
    let grains = (0..ids.len()).map(|c| ids.get(c, 0) as i32).max().unwrap();
    assert!((60..400).contains(&grains), "{grains} grains");
    assert!(full.cell_data().unwrap().get("TransformationTime").is_some());
    // Nuclei all at the start are Voronoi cells: as many grains as nuclei.
    let v = run("nucleation_growth_grains", None, &[("nucleation", json!("All at the start")), ("nuclei", json!(80))]);
    let ids = v.cell_data().unwrap().get("FeatureIds").unwrap();
    assert_eq!((0..ids.len()).map(|c| ids.get(c, 0) as i32).max().unwrap(), 80);
    let faceted = run("nucleation_growth_grains", None, &[("front", json!("Cubes")), ("nuclei", json!(40)), ("nucleation", json!("All at the start"))]);
    assert_ne!(phases(&faceted).len(), 0);
}

#[test]
fn automata_and_aggregates() {
    // Annealing coarsens: far fewer changes of phase along a row than in the noise it starts from.
    let changes = |steps: i64| -> usize {
        let p = phases(&run("cellular_automaton_structure", None, &[("steps", json!(steps)), ("cells", json!([48, 48, 48]))]));
        (0..p.len() - 1).filter(|c| (c + 1) % 48 != 0 && p[*c] != p[c + 1]).count()
    };
    assert!(changes(30) * 4 < changes(0), "{} -> {}", changes(0), changes(30));
    let kept = run(
        "cellular_automaton_structure",
        None,
        &[("rule", json!("Majority at a constant fraction")), ("initial_fraction", json!(0.3)), ("steps", json!(10))],
    );
    close("fraction kept", share(&kept, 2), 0.3, 0.01);
    let life = run(
        "cellular_automaton_structure",
        None,
        &[
            ("cells", json!([64, 64, 1])),
            ("rule", json!("Births and survivals")),
            ("births", json!("3")),
            ("survivals", json!("2, 3")),
            ("initial_fraction", json!(0.3)),
            ("steps", json!(5)),
        ],
    );
    assert!(share(&life, 2) > 0.02 && share(&life, 2) < 0.5);
    assert!(try_run("cellular_automaton_structure", None, &[("rule", json!("Births and survivals")), ("births", json!("many"))]).is_err());
    for process in ["Diffusion-limited", "Eden growth", "Ballistic"] {
        for geometry in ["Clusters", "Deposit on the base"] {
            let a = run(
                "aggregation_cluster",
                None,
                &[("cells", json!([40, 40, 40])), ("process", json!(process)), ("geometry", json!(geometry)), ("fraction", json!(0.06))],
            );
            close(&format!("{process}, {geometry}"), share(&a, 2), 0.06, 0.004);
        }
    }
    // Diffusion-limited clusters are more open than Eden ones: more of their cells lie on the surface.
    let surface = |process: &str| -> f64 {
        let a = run(
            "aggregation_cluster",
            None,
            &[("cells", json!([48, 48, 48])), ("process", json!(process)), ("seeds", json!(1)), ("fraction", json!(0.03)), ("periodic", json!(true))],
        );
        let p = phases(&a);
        let solid = p.iter().filter(|x| **x == 2).count() as f64;
        (0..p.len() - 1).filter(|c| (c + 1) % 48 != 0 && p[*c] != p[c + 1]).count() as f64 / solid
    };
    assert!(surface("Diffusion-limited") > 1.5 * surface("Eden growth"));
    // Pipes: without 1 among the open sides allowed, no pipe ends in the box.
    let pipes = run("pipe_network", None, &[]);
    assert!(field(&pipes, "Pipes")[0] > 100.0);
    assert!(try_run("pipe_network", None, &[("degrees", json!([]))]).is_err());
    // A tree: one root, radii by Murray's law.
    let t = run("branching_network", None, &[]);
    let (tips, root) = (field(&t, "Tips")[0], field(&t, "RootRadius")[0]);
    close("root radius from the tips", root, 0.6 * tips.cbrt(), 1e-6);
    assert!(tips > 50.0 && share(&t, 2) > 0.01);
}

#[test]
fn bio_structures_have_their_fractions() {
    // Bricks of 24 with a mortar of 4 in layers of 8 + 4: (20/24)^2 x 8/12.
    let n = run("nacre", None, &[("cells", json!([96, 96, 48])), ("shift", json!(0.0))]);
    close("nacre", share(&n, 2), (20.0f64 / 24.0).powi(2) * 8.0 / 12.0, 1e-9);
    let poly = run("nacre", None, &[("arrangement", json!("Polygons"))]);
    assert!(share(&poly, 2) > 0.45 && share(&poly, 2) < 0.75, "{}", share(&poly, 2));
    let p = run("twisted_plywood", None, &[("radius", json!(5.0)), ("fibre_spacing", json!(14.0)), ("layer", json!(13.0))]);
    close("plywood", share(&p, 2), field(&p, "ExpectedFraction")[0], 0.02);
    // Hexagons and squares of 16 with walls of 2: 1 - (1 - 2/16)^2.
    for cells in ["Hexagons", "Squares"] {
        let h = run("honeycomb", None, &[("arrangement", json!(cells)), ("cells", json!([160, 160, 2]))]);
        close(cells, share(&h, 2), 1.0 - (1.0 - 2.0f64 / 16.0).powi(2), 0.012);
    }
    let random = run("honeycomb", None, &[("arrangement", json!("Random cells"))]);
    assert!(share(&random, 2) > 0.15 && share(&random, 2) < 0.35);
    let wood = run("honeycomb", None, &[("arrangement", json!("Wood")), ("cells", json!([128, 64, 32])), ("rays", json!(0.15))]);
    let w = phases(&wood);
    assert!(share(&wood, 3) > 0.01, "rays");
    // Latewood (the end of each ring of 64 along X) is denser than earlywood.
    let dense = |from: usize, to: usize| (0..w.len()).filter(|c| (from..to).contains(&(c % 64)) && w[*c] == 2).count() as f64 / (to - from) as f64;
    assert!(dense(46, 64) > 1.5 * dense(0, 40));
    // Osteons: the fractions the dead-leaves model gives exactly.
    let mut got = [0.0; 5];
    let mut want = [0.0; 5];
    for s in 1..=6 {
        let o = run("haversian_bone", None, &[("cells", json!([400, 400, 1])), ("seed", json!(s))]);
        for k in 1..5 {
            got[k] += field(&o, "PhaseFractions")[k] / 6.0;
            want[k] += field(&o, "ExpectedFractions")[k] / 6.0;
        }
    }
    for k in 1..5 {
        close(&format!("bone phase {k}"), got[k], want[k], 0.02);
    }
    close("canals", want[4], 0.65 * 0.04, 1e-9);
}

#[test]
fn coating_and_combining() {
    // One sphere of radius 12 coated by 3: the shell between the two radii.
    let mut img = ImageData { extent: [0, 48, 0, 48, 0, 48], origin: [0.0; 3], spacing: [1.0; 3], ..Default::default() };
    let ph: Vec<i32> = (0..48 * 48 * 48)
        .map(|c| {
            let p = [c % 48, (c / 48) % 48, c / (48 * 48)].map(|x| x as f64 + 0.5 - 24.0);
            if p[0] * p[0] + p[1] * p[1] + p[2] * p[2] < 144.0 {
                2
            } else {
                1
            }
        })
        .collect();
    img.cell_data.insert(Array::new("Phases", 1, ArrayData::I32(ph)));
    let ball = DataSet::ImageData(img);
    let sphere = |r: f64| 4.0 / 3.0 * std::f64::consts::PI * r.powi(3) / 48f64.powi(3);
    let coated = run("coat_phase", Some(&ball), &[("thickness", json!(3.0))]);
    close("coating", share(&coated, 3), sphere(15.0) - sphere(12.0), 0.004);
    close("core", share(&coated, 2), sphere(12.0), 0.002);
    let shell = run("coat_phase", Some(&ball), &[("thickness", json!(3.0)), ("side", json!("Inside"))]);
    close("shell", share(&shell, 3), sphere(12.0) - sphere(9.0), 0.004);
    // Through the faces of a periodic structure.
    let spheres = run("hard_spheres", None, &[("cells", json!([40, 40, 40])), ("fraction", json!(0.1)), ("gap", json!(4.0))]);
    let wrapped = run("coat_phase", Some(&spheres), &[("thickness", json!(2.0)), ("periodic", json!(true))]);
    let open = run("coat_phase", Some(&spheres), &[("thickness", json!(2.0))]);
    assert!(share(&wrapped, 3) >= share(&open, 3));
    // The thickness is counted between cell centres: a little under (8/6)^3 - 1 of the spheres.
    let ratio = share(&wrapped, 3) / share(&wrapped, 2);
    assert!(ratio > 0.95 && ratio < 1.4, "coating over core {ratio}");
    // Combinations of a gyroid and a lattice.
    let a = run("triply_periodic_surface", None, &[("fraction", json!(0.5)), ("variant", json!("Network"))]);
    let b = run("strut_lattice", None, &[("fraction", json!(0.3))]);
    let def = aurafx_filters::registry::find("combine_microstructures").unwrap();
    let op = |name: &str| -> DataSet {
        let mut p = def.default_params();
        p.set_json("operation", &json!(name));
        def.run_multi(&[&a, &b], &p).unwrap()
    };
    let (u, i, d) = (share(&op("Union"), 2), share(&op("Intersection"), 2), share(&op("Difference"), 2));
    close("union and intersection", u + i, share(&a, 2) + share(&b, 2), 1e-9);
    close("difference", d, share(&a, 2) - i, 1e-9);
    close("second inside the first", share(&op("Second inside the first"), 2), i, 1e-9);
    let over = op("Inclusions over the first");
    close("inclusions", share(&over, 3), share(&b, 2), 1e-9);
    let small = run("strut_lattice", None, &[("cells", json!([32, 32, 32]))]);
    assert!(def.run_multi(&[&a, &small], &def.default_params()).is_err());
}
