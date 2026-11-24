//! The filters that measure and simulate microstructures, and the variants of the models,
//! checked against what is known exactly: the surface and Euler number of a sphere and a
//! ring, the two-point probability of overlapping spheres and of a Gaussian field, the
//! conductivity and stiffness of a laminate, the wave speeds of a uniform medium, the
//! rigidity of the cubic and octet lattices, the unfolding of sections of equal spheres.

use aurafx_data::*;
use serde_json::json;
use std::f64::consts::PI;

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

/// An image of `n` cubes of edge 1 whose phase at each centre is given by `f`.
fn image(n: [usize; 3], f: impl Fn([f64; 3]) -> i32) -> DataSet {
    let mut img = ImageData { extent: [0, n[0] as i64, 0, n[1] as i64, 0, n[2] as i64], ..Default::default() };
    let phases: Vec<i32> =
        (0..n[0] * n[1] * n[2]).map(|c| f([(c % n[0]) as f64 + 0.5, ((c / n[0]) % n[1]) as f64 + 0.5, (c / (n[0] * n[1])) as f64 + 0.5])).collect();
    img.cell_data.insert(Array::new("Phases", 1, ArrayData::I32(phases)));
    DataSet::ImageData(img)
}

fn ball(c: [f64; 3], r: f64) -> impl Fn([f64; 3]) -> bool {
    move |x| (x[0] - c[0]).powi(2) + (x[1] - c[1]).powi(2) + (x[2] - c[2]).powi(2) < r * r
}

fn phases(ds: &DataSet) -> Vec<i32> {
    let a = ds.cell_data().unwrap().get("Phases").expect("Phases");
    (0..a.len()).map(|c| a.get(c, 0) as i32).collect()
}

fn share(ds: &DataSet, phase: i32) -> f64 {
    let p = phases(ds);
    p.iter().filter(|x| **x == phase).count() as f64 / p.len() as f64
}

fn cells(ds: &DataSet, name: &str) -> Vec<f64> {
    let a = ds.cell_data().unwrap().get(name).unwrap_or_else(|| panic!("no cell array {name}"));
    (0..a.len()).map(|c| a.get(c, 0)).collect()
}

fn field(ds: &DataSet, name: &str) -> Vec<f64> {
    let a = ds.field_data().get(name).unwrap_or_else(|| panic!("no field value {name}"));
    (0..a.len() * a.ncomp).map(|k| a.get(k / a.ncomp, k % a.ncomp)).collect()
}

fn column(ds: &DataSet, name: &str) -> Vec<f64> {
    let DataSet::Table(t) = ds else { panic!("not a table") };
    let a = t.columns.get(name).unwrap_or_else(|| panic!("no column {name} (there are {:?})", t.columns.names().collect::<Vec<_>>()));
    (0..a.len()).map(|k| a.get(k, 0)).collect()
}

fn close(what: &str, got: f64, want: f64, tolerance: f64) {
    assert!((got - want).abs() <= tolerance, "{what}: {got} for {want} (within {tolerance})");
}

#[test]
fn measures_of_a_sphere_a_ring_and_a_shell() {
    let (r, v) = (20.0, 64f64.powi(3));
    let sphere = image([64; 3], |x| if ball([32.0; 3], r)(x) { 2 } else { 1 });
    let t = run("phase_measures", Some(&sphere), &[]);
    assert_eq!(column(&t, "Phase"), vec![1.0, 2.0]);
    close("fraction", column(&t, "VolumeFraction")[1], 4.0 / 3.0 * PI * r.powi(3) / v, 2e-3);
    let surface = 4.0 * PI * r * r / v;
    close("surface", column(&t, "SpecificSurface")[1], surface, 0.05 * surface);
    close("the matrix has the same surface", column(&t, "SpecificSurface")[0], surface, 0.05 * surface);
    assert_eq!(column(&t, "EulerNumber")[1], 1.0);
    assert_eq!(column(&t, "ConnectedParts")[1], 1.0);
    // The mean chord of a sphere is 4/3 of its radius.
    close("mean chord", column(&t, "MeanChord")[1], 4.0 / 3.0 * r, 1.0);
    // A ring has one tunnel (Euler number 0), a shell one cavity (2); the matrix around a
    // ring has a tunnel and a cavity (1).
    let ring = image([48, 48, 24], |x| if (((x[0] - 24.0).hypot(x[1] - 24.0) - 14.0).hypot(x[2] - 12.0)) < 5.0 { 2 } else { 1 });
    assert_eq!(column(&run("phase_measures", Some(&ring), &[]), "EulerNumber"), vec![1.0, 0.0]);
    let shell = image([40; 3], |x| if ball([20.0; 3], 15.0)(x) && !ball([20.0; 3], 9.0)(x) { 2 } else { 1 });
    let t = run("phase_measures", Some(&shell), &[]);
    assert_eq!(column(&t, "EulerNumber")[1], 2.0);
    assert_eq!(column(&t, "ConnectedParts")[0], 2.0);
    // In a plane: a disc with a hole.
    let washer = image([40, 40, 1], |x| if (5.0..14.0).contains(&(x[0] - 20.0).hypot(x[1] - 20.0)) { 2 } else { 1 });
    let t = run("phase_measures", Some(&washer), &[]);
    assert_eq!(column(&t, "EulerNumber")[1], 0.0);
    let outline = 2.0 * PI * (5.0 + 14.0) / 1600.0;
    close("outline", column(&t, "SpecificSurface")[1], outline, 0.05 * outline);
}

#[test]
fn two_point_statistics_match_the_models() {
    // Overlapping spheres of one radius: the model keeps its exact curve.
    let spheres = run("overlapping_spheres", None, &[("cells", json!([96, 96, 96])), ("radius", json!(5.0)), ("fraction", json!(0.3))]);
    let t = run("two_point_statistics", Some(&spheres), &[("periodic", json!(true)), ("reach", json!(24))]);
    let (s2, expected) = (column(&t, "S2"), column(&t, "Expected"));
    let f = share(&spheres, 2);
    close("at no distance: the fraction", s2[0], f, 1e-9);
    for d in 0..=24 {
        close(&format!("spheres at {d}"), s2[d], expected[d], 0.012);
        assert!(column(&t, "Cluster X")[d] <= column(&t, "S2 X")[d] + 1e-9 && column(&t, "LinealPath X")[d] <= column(&t, "S2 X")[d] + 1e-9);
    }
    close("far apart: the fraction squared", s2[24], f * f, 0.01);
    // The pores between overlapping spheres: a segment of length z lies in them with the
    // probability q exp(-theta pi R^2 z).
    let pores = run("two_point_statistics", Some(&spheres), &[("phase", json!(1)), ("reach", json!(12))]);
    let q = 1.0 - 0.3f64;
    let theta = -q.ln() / (4.0 / 3.0 * PI * 125.0);
    for d in [2usize, 6, 10] {
        close(&format!("lineal path at {d}"), column(&pores, "LinealPath X")[d], q * (-theta * PI * 25.0 * d as f64).exp(), 0.03);
    }
    let measures = run("phase_measures", Some(&spheres), &[("periodic", json!(true))]);
    let (got, want) = (column(&measures, "SpecificSurface")[1], column(&measures, "ExpectedSpecificSurface")[1]);
    close("surface of overlapping spheres", got, want, 0.1 * want);
    close("expected fraction", column(&measures, "ExpectedFraction")[1], 0.3, 1e-12);
    // A Gaussian field cut at a level.
    let gauss = run("gaussian_field_medium", None, &[("cells", json!([96, 96, 96])), ("length", json!(6.0)), ("fraction", json!(0.35))]);
    let t = run("two_point_statistics", Some(&gauss), &[("periodic", json!(true)), ("reach", json!(20))]);
    for d in 0..=20 {
        close(&format!("field at {d}"), column(&t, "S2")[d], column(&t, "Expected")[d], 0.015);
    }
    let measures = run("phase_measures", Some(&gauss), &[("periodic", json!(true))]);
    let (got, want) = (column(&measures, "SpecificSurface")[1], column(&measures, "ExpectedSpecificSurface")[1]);
    close("surface of the level set", got, want, 0.08 * want);
    // Fibres on lines.
    let lines = run("poisson_line_fibres", None, &[("cells", json!([96, 96, 96])), ("radius", json!(4.0))]);
    let measures = run("phase_measures", Some(&lines), &[]);
    let (got, want) = (column(&measures, "SpecificSurface")[1], column(&measures, "ExpectedSpecificSurface")[1]);
    close("surface of fibres", got, want, 0.1 * want);
}

#[test]
fn chords_thickness_and_paths() {
    // Layers 4 thick every 10 along X.
    let layers = image([60, 20, 20], |x| if (x[0] as usize + 3) % 10 < 4 { 2 } else { 1 });
    let t = run("phase_chord_lengths", Some(&layers), &[]);
    close("chords across the layers", field(&t, "MeanChord")[0], 4.0, 1e-9);
    let t = run("phase_chord_lengths", Some(&layers), &[("phase", json!(1))]);
    close("chords between the layers", field(&t, "MeanChord")[0], 6.0, 1e-9);
    // A wall 9 thick, a sphere of radius 10.
    let wall = image([40, 30, 30], |x| if (15.0..24.0).contains(&x[0]) { 2 } else { 1 });
    let t = run("local_thickness", Some(&wall), &[]);
    close("wall", field(&t, "MeanThickness")[0], 9.0, 1.0);
    let sphere = image([40; 3], |x| if ball([20.0; 3], 10.0)(x) { 2 } else { 1 });
    let t = run("local_thickness", Some(&sphere), &[]);
    // To about a cell: the balls are centred on cells.
    close("sphere", field(&t, "LargestThickness")[0], 20.0, 2.6);
    assert!(cells(&t, "LocalThickness").iter().zip(phases(&sphere)).all(|(v, p)| (p == 2) == (*v > 0.0)));
    let dist = run("local_thickness", Some(&sphere), &[("output", json!("Size distribution"))]);
    close("the shares sum to 1", column(&dist, "ShareBelow").last().copied().unwrap(), 1.0, 1e-9);
    // A straight channel, and one that runs along the diagonal of a face.
    let straight = image([20, 20, 30], |x| if (x[0] - 10.0).abs() < 3.0 && (x[1] - 10.0).abs() < 3.0 { 1 } else { 2 });
    close("straight", field(&run("tortuosity", Some(&straight), &[]), "Tortuosity")[0], 1.0, 0.01);
    let slanted = image([40, 12, 40], |x| if (x[0] - x[2]).abs() < 1.2 { 1 } else { 2 });
    let t = run("tortuosity", Some(&slanted), &[]);
    close("along a diagonal", field(&t, "ShortestPath")[0], 2f64.sqrt(), 0.05);
    assert!(field(&t, "ReachedShare")[0] > 0.99);
    // No way through.
    let closed = image([10, 10, 20], |x| if x[2] < 8.0 || x[2] > 12.0 { 1 } else { 2 });
    assert!(field(&run("tortuosity", Some(&closed), &[]), "Tortuosity")[0].is_nan());
}

#[test]
fn fabric_of_stretched_spheres_and_stretching() {
    let spheres = run("hard_spheres", None, &[("cells", json!([96, 96, 96])), ("radius", json!(9.0)), ("fraction", json!(0.15))]);
    let round = run("fabric_tensor", Some(&spheres), &[("periodic", json!(true))]);
    close("round", field(&round, "DegreeOfAnisotropy")[0], 1.0, 0.06);
    let long = run("stretch_microstructure", Some(&spheres), &[("stretch", json!([1.0, 1.0, 2.0]))]);
    close("the fraction is kept", share(&long, 2), share(&spheres, 2), 0.02);
    assert!(long.field_data().get("Shapes").is_none(), "the objects no longer describe the structure");
    // Stretched, the structure no longer joins up through the faces.
    let t = run("fabric_tensor", Some(&long), &[]);
    // The layers of cells repeated by the stretch make steps that the slanted lines of the
    // fabric meet more than once: it reads a little under 2. Along the axes the chords are exact.
    let fabric = field(&t, "DegreeOfAnisotropy")[0];
    assert!(fabric > 1.55 && fabric < 2.1, "fabric of spheres stretched twice: {fabric}");
    let chords = field(&run("phase_chord_lengths", Some(&long), &[]), "MeanChord");
    close("chords twice as long along Z", chords[2] / chords[0], 2.0, 0.12);
    assert!(column(&t, "DirectionZ")[0].abs() > 0.98, "the long axis is Z");
    // Turned by 90 degrees about X (Bunge: about Z, X, Z), the long axis is Y.
    let turned = run("stretch_microstructure", Some(&spheres), &[("stretch", json!([1.0, 1.0, 2.0])), ("rotation", json!([0.0, 90.0, 0.0]))]);
    let t = run("fabric_tensor", Some(&turned), &[]);
    assert!(column(&t, "DirectionY")[0].abs() > 0.98, "{:?}", column(&t, "DirectionY"));
}

#[test]
fn percolation_and_studies() {
    let slab = image([20, 20, 20], |x| if (8.0..12.0).contains(&x[2]) || ball([4.0, 4.0, 16.0], 2.5)(x) { 2 } else { 1 });
    let p = run("percolation", Some(&slab), &[]);
    assert_eq!(field(&p, "Percolates"), vec![1.0, 1.0, 0.0]);
    assert_eq!(field(&p, "Clusters")[0], 2.0);
    let ids = cells(&p, "ClusterIds");
    assert_eq!(ids[10 + 20 * (10 + 20 * 10)], 1.0, "the largest part is number 1");
    assert_eq!(ids[4 + 20 * (4 + 20 * 16)], 2.0);
    assert!(field(&p, "SpanningShare")[0] > 0.9 && field(&p, "SpanningShare")[0] < 1.0);
    // Overlapping spheres percolate near a volume fraction of 0.29.
    let t = run(
        "microstructure_study",
        None,
        &[
            ("settings", json!("radius = 3")),
            ("values", json!([0.2, 0.24, 0.28, 0.32, 0.36, 0.4])),
            ("sizes", json!([40.0, 56.0])),
            ("realizations", json!(10)),
        ],
    );
    assert_eq!(column(&t, "Size").len(), 12);
    let fractions = column(&t, "MeanFraction");
    close("the fraction asked", fractions[2], 0.28, 0.02);
    let share = column(&t, "Percolates");
    assert!(share[0] < 0.4 && share[5] > 0.9 && share[6] < 0.3 && share[11] > 0.9, "{share:?}");
    for threshold in field(&t, "Thresholds") {
        close("threshold", threshold, 0.29, 0.05);
    }
    close("carried to an endless box", field(&t, "ThresholdOfAnEndlessBox")[0], 0.29, 0.08);
    // Without a setting varied: the scatter of the fraction gives the integral range. For
    // overlapping spheres it is the volume of a sphere times a number near 1.
    let t = run("microstructure_study", None, &[("settings", json!("radius = 3")), ("vary", json!("")), ("sizes", json!([32.0])), ("realizations", json!(40))]);
    let range = column(&t, "IntegralRange")[0];
    assert!(range > 0.4 * 113.0 && range < 2.5 * 113.0, "integral range {range}");
}

#[test]
fn representative_volume_of_noise() {
    // Cells drawn one by one: nothing is correlated beyond a cell, so the variance of the
    // mean over a block is that of a cell over the number of cells of the block.
    let mut s = 12345u64;
    let mut draw = move || {
        s ^= s << 13;
        s ^= s >> 7;
        s ^= s << 17;
        (s >> 11) as f64 / (1u64 << 53) as f64
    };
    let noise: Vec<i32> = (0..64 * 64 * 64).map(|_| if draw() < 0.3 { 2 } else { 1 }).collect();
    let mut img = ImageData { extent: [0, 64, 0, 64, 0, 64], ..Default::default() };
    img.cell_data.insert(Array::new("Phases", 1, ArrayData::I32(noise)));
    let t = run("representative_volume", Some(&DataSet::ImageData(img)), &[("precision", json!(0.01))]);
    close("exponent", field(&t, "Exponent")[0], 1.0, 0.08);
    close("integral range: one cell", field(&t, "IntegralRange")[0], 1.0, 0.25);
    // 4 x 0.21 / (0.01 x 0.3)^2 cells for one percent on the fraction.
    let want = 4.0 * 0.21 / (0.01f64 * 0.3).powi(2);
    close("volume", field(&t, "RepresentativeVolume")[0], want, 0.3 * want);
    let v = column(&t, "Variance");
    assert!(v.windows(2).all(|w| w[1] < w[0]), "the variance falls with the size of the blocks: {v:?}");
}

#[test]
fn sections_of_equal_spheres_unfold_to_one_size() {
    // Spheres of diameter 10, 0.001 per unit volume, cut by a plane: a sphere is met when its
    // centre lies within its radius of the plane, at a height drawn evenly.
    let (diameter, per_volume, sections) = (10.0f64, 1e-3, 40_000usize);
    let d: Vec<f64> = (0..sections).map(|k| diameter * (1.0 - ((k as f64 + 0.5) / sections as f64).powi(2)).sqrt()).collect();
    let mut t = Table::default();
    t.columns.insert(Array::scalars_f64("Diameters", d));
    let area = sections as f64 / (per_volume * diameter);
    let out = run("unfold_section_sizes", Some(&DataSet::Table(t)), &[("array", json!("Diameters")), ("classes", json!(10)), ("area", json!(area))]);
    close("spheres per volume", field(&out, "SpheresPerVolume")[0], per_volume, 0.03 * per_volume);
    let share = column(&out, "Share");
    assert!(share[9] > 0.97, "all in the largest class: {share:?}");
    assert!(column(&out, "SectionsPerArea").iter().all(|v| *v > 0.0));
}

#[test]
fn touching_particles_are_split() {
    let pair = image([48, 32, 32], |x| if ball([16.0, 16.0, 16.0], 9.0)(x) || ball([30.0, 16.0, 16.0], 9.0)(x) { 2 } else { 1 });
    let out = run("split_particles", Some(&pair), &[]);
    assert_eq!(field(&out, "Particles")[0], 2.0);
    let ids = cells(&out, "FeatureIds");
    let at = |x: usize| ids[x + 48 * (16 + 32 * 16)];
    assert!(at(10) != at(36) && at(10) > 0.0 && at(36) > 0.0 && at(2) == 0.0);
    // The cut lies half way.
    assert!(at(20) == at(10) && at(26) == at(36));
    // A neck that is hardly one: with a larger depth the pair stays whole.
    assert_eq!(field(&run("split_particles", Some(&pair), &[("depth", json!(6.0))]), "Particles")[0], 1.0);
    let one = image([32; 3], |x| if ball([16.0; 3], 9.0)(x) { 2 } else { 1 });
    assert_eq!(field(&run("split_particles", Some(&one), &[]), "Particles")[0], 1.0);
}

#[test]
fn reconstruction_from_an_image() {
    let source = run("overlapping_spheres", None, &[("cells", json!([48, 48, 48])), ("radius", json!(4.0)), ("fraction", json!(0.3))]);
    let copy = run("reconstruct_microstructure", Some(&source), &[("cells", json!([32, 32, 32])), ("sweeps", json!(12.0))]);
    close("fraction", share(&copy, 2), share(&source, 2), 1e-3);
    let (want, got) = (field(&copy, "TwoPointTarget"), field(&copy, "TwoPointResult"));
    for d in 0..want.len() {
        close(&format!("two-point at {}", d + 1), got[d], want[d], 0.01);
    }
    // With the lineal path matched, the straight stretches of the phase are those of the image.
    let a = run("two_point_statistics", Some(&source), &[("reach", json!(8))]);
    let b = run("two_point_statistics", Some(&copy), &[("reach", json!(8)), ("periodic", json!(true))]);
    for d in [1usize, 3, 5] {
        close(&format!("lineal path at {d}"), column(&b, "LinealPath X")[d], column(&a, "LinealPath X")[d], 0.03);
    }
    // From a section: a volume.
    let section = run("overlapping_spheres", None, &[("cells", json!([96, 96, 1])), ("radius", json!(4.0)), ("fraction", json!(0.3))]);
    let volume = run("reconstruct_microstructure", Some(&section), &[("cells", json!([24, 24, 24])), ("sweeps", json!(8.0))]);
    close("fraction of the volume", share(&volume, 2), share(&section, 2), 2e-3);
}

fn area_of(pd: &PolyData) -> f64 {
    pd.polys
        .iter()
        .map(|(_, c)| {
            let p: Vec<[f64; 3]> = c.iter().map(|i| pd.points.get(*i as usize)).collect();
            (1..p.len() - 1)
                .map(|k| {
                    let (u, v) =
                        ([p[k][0] - p[0][0], p[k][1] - p[0][1], p[k][2] - p[0][2]], [p[k + 1][0] - p[0][0], p[k + 1][1] - p[0][1], p[k + 1][2] - p[0][2]]);
                    0.5 * ((u[1] * v[2] - u[2] * v[1]).powi(2) + (u[2] * v[0] - u[0] * v[2]).powi(2) + (u[0] * v[1] - u[1] * v[0]).powi(2)).sqrt()
                })
                .sum::<f64>()
        })
        .sum()
}

#[test]
fn objects_and_smooth_surfaces() {
    let spheres = run("hard_spheres", None, &[("radius", json!(6.0)), ("fraction", json!(0.2)), ("periodic", json!(false))]);
    let DataSet::PolyData(objects) = run("microstructure_objects", Some(&spheres), &[]) else { panic!() };
    let count = field(&spheres, "Radii").len() - 1;
    assert_eq!(objects.verts.len(), count);
    assert_eq!(objects.point_data.get("Radius").unwrap().get(0, 0), 6.0);
    // One sphere well inside the box: the surface from its description is that of the sphere.
    let mut one = image([32; 3], |x| if ball([16.0; 3], 10.0)(x) { 2 } else { 1 });
    one.field_data_mut_any().insert(Array::new("Shapes", 8, ArrayData::F64(vec![16.0, 16.0, 16.0, 16.0, 16.0, 16.0, 10.0, 10.0])));
    let DataSet::PolyData(exact) = run("microstructure_surface", Some(&one), &[("refine", json!(2))]) else { panic!() };
    let want = 4.0 * PI * 100.0;
    close("sphere from its description", area_of(&exact), want, 0.01 * want);
    let DataSet::PolyData(smooth) = run("microstructure_surface", Some(&one), &[("from_cells", json!(true))]) else { panic!() };
    close("sphere from its cells", area_of(&smooth), want, 0.05 * want);
    // Struts and fibres keep their description too; a field does not, and falls back on its cells.
    let lattice = run("strut_lattice", None, &[]);
    let DataSet::PolyData(struts) = run("microstructure_objects", Some(&lattice), &[]) else { panic!() };
    assert_eq!(struts.lines.len(), 8 * 24, "the octet truss has 24 struts per cell");
    let DataSet::PolyData(skin) = run("microstructure_surface", Some(&lattice), &[]) else { panic!() };
    assert!(skin.polys.len() > 10_000);
    let field_medium = run("gaussian_field_medium", None, &[("cells", json!([32, 32, 32]))]);
    assert!(try_run("microstructure_objects", Some(&field_medium), &[]).is_err());
    assert!(matches!(run("microstructure_surface", Some(&field_medium), &[]), DataSet::PolyData(p) if p.polys.len() > 1000));
}

#[test]
fn rigidity_of_lattices() {
    let of = |cell: &str| {
        let lattice = run("strut_lattice", None, &[("cell", json!(cell)), ("fraction", json!(0.0)), ("radius", json!(1.5))]);
        run("framework_rigidity", Some(&lattice), &[])
    };
    // The cubic lattice cannot resist its three shears; the octet truss resists every strain.
    let cubic = of("Cubic");
    assert_eq!(column(&cubic, "Nodes")[0], 8.0);
    assert_eq!(column(&cubic, "Struts")[0], 24.0);
    assert_eq!(column(&cubic, "FloppyStrains")[0], 3.0);
    let octet = of("Octet");
    assert_eq!((column(&octet, "Nodes")[0], column(&octet, "Struts")[0]), (32.0, 192.0));
    assert_eq!(column(&octet, "FloppyStrains")[0], 0.0);
    assert_eq!(column(&octet, "Mechanisms")[0], 0.0);
    // Struts less freedoms: states of self-stress less mechanisms.
    for t in [&cubic, &octet, &of("Body-centred cubic"), &of("Kelvin"), &of("Pyrochlore")] {
        assert_eq!(column(t, "MaxwellNumber")[0], column(t, "StatesOfSelfStress")[0] - column(t, "Mechanisms")[0]);
    }
    assert!(column(&of("Kelvin"), "FloppyStrains")[0] > 0.0, "the Kelvin cell bends");
    // Every node of the pyrochlore lattice holds six struts.
    close("pyrochlore", column(&of("Pyrochlore"), "StrutsPerNode")[0], 6.0, 1e-9);
}

#[test]
fn conductivity_and_stiffness_of_laminates() {
    // Layers across X, half and half: in series along X, in parallel along Y and Z.
    let layers = image([16, 8, 8], |x| if (x[0] as usize / 4) % 2 == 0 { 1 } else { 2 });
    let out = run("effective_conductivity", Some(&layers), &[("conductivities", json!([1.0, 10.0])), ("tolerance", json!(1e-8))]);
    let k = field(&out, "EffectiveConductivity");
    close("in series", k[0], 1.0 / (0.5 / 1.0 + 0.5 / 10.0), 1e-5);
    close("in parallel", k[4], 5.5, 1e-5);
    close("in parallel", k[8], 5.5, 1e-5);
    close("no cross term", k[1], 0.0, 1e-6);
    let bounds = field(&out, "ConductivityBounds");
    assert!(bounds[0] <= bounds[1] && bounds[1] <= bounds[2] && bounds[2] <= bounds[3]);
    // The flux along X is the same in both phases.
    let flux = out.cell_data().unwrap().get("Flux").unwrap();
    assert!((0..flux.len()).all(|c| (flux.get(c, 0) - k[0]).abs() < 1e-4));
    // Elastic layers: stretched across them, with no strain along them, the stiffnesses add in series.
    let (e1, e2, nu) = (1.0, 5.0, 0.25);
    let stiff = |e: f64| e * (1.0 - nu) / ((1.0 + nu) * (1.0 - 2.0 * nu));
    let out = run("effective_stiffness", Some(&layers), &[("youngs", json!([e1, e2])), ("poisson", json!([nu, nu])), ("tolerance", json!(1e-8))]);
    let c = field(&out, "EffectiveStiffness");
    close("C11", c[0], 1.0 / (0.5 / stiff(e1) + 0.5 / stiff(e2)), 1e-4);
    // Shear along the layers (YZ): the shear moduli in parallel; across them (XY): in series.
    let shear = |e: f64| e / (2.0 * (1.0 + nu));
    close("C44", c[3 * 6 + 3], 0.5 * (shear(e1) + shear(e2)), 1e-4);
    close("C66", c[5 * 6 + 5], 1.0 / (0.5 / shear(e1) + 0.5 / shear(e2)), 1e-4);
    assert!((0..6).all(|i| (0..6).all(|j| (c[i * 6 + j] - c[j * 6 + i]).abs() < 1e-4)), "the stiffness is symmetric");
    // One material: its own moduli, whatever the phases.
    let out = run("effective_stiffness", Some(&layers), &[("youngs", json!([2.0, 2.0])), ("poisson", json!([0.3, 0.3]))]);
    let moduli = field(&out, "EffectiveModuli");
    close("Young's modulus", moduli[0], 2.0, 1e-9);
    close("shear modulus", moduli[3], 2.0 / 2.6, 1e-9);
    close("Poisson's ratio", field(&out, "PoissonRatios")[0], 0.3, 1e-9);
    close("isotropic", field(&out, "AnisotropyRatio")[0], 1.0, 1e-9);
}

#[test]
fn properties_of_spheres_lie_between_their_bounds() {
    let spheres = run("hard_spheres", None, &[("cells", json!([32, 32, 32])), ("radius", json!(5.0)), ("fraction", json!(0.25))]);
    let out = run("effective_conductivity", Some(&spheres), &[("conductivities", json!([1.0, 20.0]))]);
    let (k, b) = (field(&out, "EffectiveConductivity"), field(&out, "ConductivityBounds"));
    for a in [0, 4, 8] {
        assert!(k[a] >= b[1] * 0.995 && k[a] <= b[2], "conductivity {} outside {b:?}", k[a]);
    }
    // Well-separated spheres in a matrix sit near the lower bound.
    assert!(k[0] < b[1] + 0.25 * (b[2] - b[1]));
    let out = run("effective_stiffness", Some(&spheres), &[("youngs", json!([1.0, 20.0]))]);
    let (bulk, shear) = (field(&out, "BulkModulus")[0], field(&out, "ShearModulus")[0]);
    let (kb, gb) = (field(&out, "BulkBounds"), field(&out, "ShearBounds"));
    assert!(bulk >= kb[1] * 0.995 && bulk <= kb[2], "bulk {bulk} outside {kb:?}");
    assert!(shear >= gb[1] * 0.995 && shear <= gb[2], "shear {shear} outside {gb:?}");
    assert!(cells(&out, "VonMisesStress").iter().all(|v| v.is_finite() && *v >= 0.0));
    // The variance of the stress over blocks gives the volume for a precision on its mean.
    let mut with = out.clone();
    let stress = out.cell_data().unwrap().get("Stress").unwrap();
    with.cell_data_mut().unwrap().insert(Array::scalars_f64("StressXX", (0..stress.len()).map(|c| stress.get(c, 0)).collect()));
    let t = run("representative_volume", Some(&with), &[("property", json!(["Cells", "StressXX"]))]);
    assert!(field(&t, "RepresentativeVolume")[0] > 0.0 && field(&t, "Exponent")[0] > 0.3);
}

#[test]
fn a_unit_cell_is_optimized_towards_its_bound() {
    let cell = run("optimized_cell", None, &[("cells", json!([16, 16, 16])), ("iterations", json!(25)), ("fraction", json!(0.4))]);
    close("fraction", share(&cell, 2), 0.4, 1e-3);
    let (bulk, bound) = (field(&cell, "BulkModulus")[0], field(&cell, "UpperBound")[0]);
    assert!(bulk <= bound * 1.001 && bulk > 0.6 * bound, "bulk modulus {bulk}, bound {bound}");
    // Material spread evenly (a Gaussian field at the same fraction) is far less stiff.
    let random = run("gaussian_field_medium", None, &[("cells", json!([16, 16, 16])), ("fraction", json!(0.4)), ("length", json!(4.0))]);
    let out = run("effective_stiffness", Some(&random), &[("youngs", json!([1e-3, 1.0]))]);
    assert!(field(&out, "BulkModulus")[0] < 0.8 * bulk);
    // In the plane, for conduction.
    let sheet = run("optimized_cell", None, &[("cells", json!([32, 32, 1])), ("goal", json!("Conductivity")), ("iterations", json!(25))]);
    let (k, bound) = (field(&sheet, "Conductivity")[0], field(&sheet, "UpperBound")[0]);
    assert!(k <= bound * 1.001 && k > 0.7 * bound, "conductivity {k}, bound {bound}");
}

#[test]
fn waves_in_a_uniform_medium_travel_at_its_two_speeds() {
    let (e, nu, rho) = (2.0f64, 0.25f64, 4.0f64);
    let uniform = image([8, 8, 8], |_| 1);
    let t = run(
        "wave_band_structure",
        Some(&uniform),
        &[("youngs", json!([e])), ("poisson", json!([nu])), ("density", json!([rho])), ("waves", json!(1)), ("points", json!(4))],
    );
    let (shear, long) = ((e / (2.0 * (1.0 + nu)) / rho).sqrt(), (e * (1.0 - nu) / ((1.0 + nu) * (1.0 - 2.0 * nu)) / rho).sqrt());
    // The second point of the path: a quarter of the way to the face of the zone, pi / 8.
    let k = column(&t, "Kx")[1];
    close("the wave vector", k, PI / 8.0 / 4.0, 1e-12);
    close("shear wave", column(&t, "Band 1")[1], shear * k / (2.0 * PI), 1e-9);
    close("shear wave", column(&t, "Band 2")[1], shear * k / (2.0 * PI), 1e-9);
    close("pressure wave", column(&t, "Band 3")[1], long * k / (2.0 * PI), 1e-9);
    assert!(field(&t, "BandGaps").is_empty(), "no gap in a uniform medium");
    // Layers of two materials stop the waves that travel across them in some ranges of frequency.
    let layers = image([16, 1, 1], |x| if x[0] < 8.0 { 1 } else { 2 });
    let t =
        run("wave_band_structure", Some(&layers), &[("youngs", json!([1.0, 30.0])), ("density", json!([1.0, 6.0])), ("waves", json!(3)), ("bands", json!(6))]);
    assert!(column(&t, "Band 1")[0].abs() < 1e-9);
    assert!(column(&t, "Band 4").iter().zip(column(&t, "Band 3")).all(|(a, b)| *a >= b - 1e-12));
}

#[test]
fn stealthy_seeds_make_even_grains() {
    let volumes = |stealth: f64| -> f64 {
        let g = run("voronoi_grains", None, &[("cells", json!([48, 48, 48])), ("grains", json!(150)), ("periodic", json!(true)), ("stealth", json!(stealth))]);
        let ids = cells(&g, "FeatureIds");
        let mut count = vec![0.0; 151];
        ids.iter().for_each(|i| count[*i as usize] += 1.0);
        let mean = count[1..].iter().sum::<f64>() / 150.0;
        (count[1..].iter().map(|c| (c - mean).powi(2)).sum::<f64>() / 150.0).sqrt() / mean
    };
    let (random, even) = (volumes(0.0), volumes(0.3));
    assert!(random > 0.33 && random < 0.52, "random seeds: {random}");
    assert!(even < 0.12, "stealthy seeds: {even}");
    // No density wave is left at the wave vectors that were constrained.
    let size = [10.0, 10.0, 10.0];
    let mut s = 7u64;
    let start: Vec<[f64; 3]> = (0..60)
        .map(|_| {
            std::array::from_fn(|_| {
                s = s.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
                (s >> 11) as f64 / (1u64 << 53) as f64 * 10.0
            })
        })
        .collect();
    let wave = |pts: &[[f64; 3]]| -> f64 {
        let k = 2.0 * PI / 10.0;
        let (c, s) = pts.iter().fold((0.0, 0.0), |a, p| (a.0 + (k * p[0]).cos(), a.1 + (k * p[0]).sin()));
        (c * c + s * s) / pts.len() as f64
    };
    let moved = aurafx_filters::morphology::stealthy(size, false, start.clone(), 0.3).unwrap();
    assert!(wave(&start) > 0.05 && wave(&moved) < 1e-8, "{} then {}", wave(&start), wave(&moved));
    // Foams and networks take them too.
    let net = run("delaunay_fibre_network", None, &[("stealth", json!(0.3))]);
    assert!(share(&net, 2) > 0.0);
}

#[test]
fn spheres_pack_densely_by_collisions() {
    let packed = run(
        "hard_spheres",
        None,
        &[
            ("cells", json!([40, 40, 40])),
            ("radius", json!(5.0)),
            ("fraction", json!(0.56)),
            ("gap", json!(0.0)),
            ("method", json!("Dense packing by collisions")),
            ("growth_rate", json!(0.02)),
        ],
    );
    let (centres, radii) = (field(&packed, "Centers"), field(&packed, "Radii"));
    let n = radii.len() - 1;
    let fraction = n as f64 * 4.0 / 3.0 * PI * 125.0 / 64000.0;
    assert!(fraction > 0.54, "{n} spheres: {fraction}");
    let mut least = f64::INFINITY;
    for i in 1..=n {
        for j in i + 1..=n {
            let d: f64 = (0..3)
                .map(|k| {
                    let d = centres[3 * i + k] - centres[3 * j + k];
                    (d - 40.0 * (d / 40.0).round()).powi(2)
                })
                .sum::<f64>()
                .sqrt();
            least = least.min(d - radii[i] - radii[j]);
        }
    }
    assert!(least > -1e-6, "two spheres overlap by {}", -least);
    // In a box with walls, and discs in a plane.
    let boxed = run(
        "hard_spheres",
        None,
        &[
            ("cells", json!([32, 32, 32])),
            ("radius", json!(4.0)),
            ("fraction", json!(0.45)),
            ("periodic", json!(false)),
            ("method", json!("Dense packing by collisions")),
        ],
    );
    assert!(field(&boxed, "Centers").iter().all(|x| (0.0..=32.0).contains(x)));
    let discs = run(
        "hard_spheres",
        None,
        &[
            ("cells", json!([96, 96, 1])),
            ("radius", json!(5.0)),
            ("fraction", json!(0.7)),
            ("gap", json!(0.0)),
            ("method", json!("Dense packing by collisions")),
        ],
    );
    close("discs", share(&discs, 2), 0.7, 0.03);
}

#[test]
fn automata_on_other_cells_than_those_of_the_grid() {
    let on = |lattice: &str, cells: [usize; 3], neighbours: &str| {
        run(
            "cellular_automaton_structure",
            None,
            &[("cells", json!(cells)), ("lattice", json!(lattice)), ("tile", json!(8.0)), ("neighbours", json!(neighbours)), ("steps", json!(10))],
        )
    };
    // Away from the faces of the box, a hexagon has 6 neighbours, a triangle 3 across its
    // sides, a rhombus 4, a truncated octahedron 14 and a rhombic dodecahedron 12.
    for (lattice, cells, least, most) in [
        ("Hexagons", [160, 160, 1], 5.3, 6.0),
        ("Triangles", [160, 160, 1], 2.6, 3.05),
        ("Penrose rhombi", [160, 160, 1], 3.5, 4.1),
        ("Squares and rhombi (eightfold)", [160, 160, 1], 3.5, 4.1),
        ("Truncated octahedra", [64, 64, 64], 9.0, 14.0),
        ("Rhombic dodecahedra", [64, 64, 64], 8.5, 12.0),
    ] {
        let out = on(lattice, cells, "Faces");
        let near = field(&out, "NeighboursPerTile")[0];
        assert!(near >= least && near <= most, "{lattice}: {near} neighbours per tile");
        assert!(share(&out, 2) > 0.05 && share(&out, 1) > 0.05, "{lattice}: one phase");
        // Every tile is one phase.
        let (ids, p) = (cells_of(&out), phases(&out));
        let mut phase_of = std::collections::HashMap::new();
        assert!(ids.iter().zip(&p).all(|(i, x)| *phase_of.entry(*i).or_insert(*x) == *x), "{lattice}: a tile of two phases");
    }
    // The two rhombi of the fivefold tiling: areas in the golden ratio, and more thick ones.
    let out = on("Penrose rhombi", [240, 240, 1], "Faces, edges and corners");
    let ids = cells_of(&out);
    let mut area = std::collections::HashMap::new();
    ids.iter().for_each(|i| *area.entry(*i).or_insert(0.0) += 1.0);
    let whole: Vec<f64> = area.values().copied().filter(|a| *a > 30.0).collect();
    let (thin, thick) = (64.0 * (PI / 5.0).sin(), 64.0 * (2.0 * PI / 5.0).sin());
    let near = |a: f64, b: f64| (a - b).abs() < 7.0;
    let (thins, thicks) = (whole.iter().filter(|a| near(**a, thin)).count() as f64, whole.iter().filter(|a| near(**a, thick)).count() as f64);
    close("thick over thin rhombi: the golden ratio", thicks / thins, 1.618, 0.15);
    // In a volume, the tilings of the plane are stacked.
    assert!(share(&on("Hexagons", [48, 48, 24], "Faces"), 2) > 0.0);
}

fn cells_of(ds: &DataSet) -> Vec<i64> {
    cells(ds, "FeatureIds").iter().map(|v| *v as i64).collect()
}

#[test]
fn grains_coarsen_and_blocks_fit() {
    let grains = |sweeps: usize| {
        run(
            "nucleation_growth_grains",
            None,
            &[("cells", json!([32, 32, 32])), ("nucleation", json!("All at the start")), ("nuclei", json!(300)), ("coarsening", json!(sweeps))],
        )
    };
    let count = |ds: &DataSet| cells(ds, "FeatureIds").iter().cloned().fold(0.0, f64::max);
    let (before, after) = (grains(0), grains(100));
    assert_eq!(count(&before), 300.0);
    assert!(count(&after) < 200.0 && count(&after) > 10.0, "{} grains left", count(&after));
    let ids = cells(&after, "FeatureIds");
    assert!(ids.iter().all(|i| *i >= 1.0));
    assert_eq!(
        after.field_data().get("AvgQuats").map(|a| a.len()).or(after.field_data().get("AvgEulerAngles").map(|a| a.len())).unwrap_or(count(&after) as usize + 1),
        count(&after) as usize + 1
    );
    // Blocks: along every line the motion alternates, so two neighbours never both push out
    // through the face they share.
    let blocks = run("polarized_blocks", None, &[]);
    let (axis, sign) = (cells(&blocks, "BlockAxis"), cells(&blocks, "BlockSign"));
    let motion = |b: [usize; 3], along: usize| -> f64 {
        let c = (b[0] * 16 + 8) + 64 * ((b[1] * 16 + 8) + 64 * (b[2] * 16 + 8));
        if axis[c] as usize == along {
            sign[c]
        } else {
            -sign[c]
        }
    };
    for along in 0..3 {
        for b in 0..64usize {
            let at = [b % 4, (b / 4) % 4, b / 16];
            let mut next = at;
            next[along] = (at[along] + 1) % 4;
            assert_eq!(motion(at, along), -motion(next, along), "blocks {at:?} and {next:?} along {along}");
        }
    }
    assert!(share(&blocks, 1) > 0.02 && share(&blocks, 2) > 0.5);
    assert!(try_run("polarized_blocks", None, &[("cells", json!([48, 64, 64]))]).is_err(), "an odd number of blocks in a periodic box");
}

#[test]
fn networks_turned_and_graded_and_in_the_plane() {
    // Ligaments start four times as often at the far end along X: more walls there.
    let graded = run(
        "scale_rich_network",
        None,
        &[("nucleation_gradient", json!([1.0, 8.0])), ("fraction", json!(0.0)), ("first", json!(3.0)), ("decay", json!(0.2)), ("ligaments", json!(300))],
    );
    let p = phases(&graded);
    let walls = |from: usize, to: usize| (0..256).flat_map(|j| (from..to).map(move |i| (i, j))).filter(|(i, j)| p[i + 256 * j] != p[i + 1 + 256 * j]).count();
    assert!(walls(128, 255) as f64 > 1.3 * walls(0, 127) as f64, "{} then {}", walls(0, 127), walls(128, 255));
    // Around a pole: along a ray from the pole the ligaments are crossed, along a circle they are followed.
    let around = run(
        "scale_rich_network",
        None,
        &[
            ("polarized", json!(1.0)),
            ("pole_angle", json!(90.0)),
            ("fraction", json!(0.0)),
            ("first", json!(4.0)),
            ("decay", json!(0.3)),
            ("ligaments", json!(120)),
        ],
    );
    let radial = run(
        "scale_rich_network",
        None,
        &[
            ("polarized", json!(1.0)),
            ("pole_angle", json!(0.0)),
            ("fraction", json!(0.0)),
            ("first", json!(4.0)),
            ("decay", json!(0.3)),
            ("ligaments", json!(120)),
        ],
    );
    let crossed = |ds: &DataSet| {
        let p = phases(ds);
        // The row through the pole, right of it: a ray.
        (140..250).filter(|i| p[i + 256 * 128] != p[i + 1 + 256 * 128]).count()
    };
    assert!(crossed(&around) > crossed(&radial) + 4, "{} around, {} radial", crossed(&around), crossed(&radial));
    // A Delaunay network in the plane: six fibres at a node.
    let flat = run("delaunay_fibre_network", None, &[("cells", json!([128, 128, 1])), ("points", json!(200))]);
    close("degree in the plane", field(&flat, "MeanDegree")[0], 6.0, 1e-9);
    assert!(share(&flat, 2) > 0.05);
}

#[test]
fn templates_and_formation() {
    for t in aurafx_filters::morphology::study::TEMPLATES {
        let (def, p) = aurafx_filters::morphology::study::template(t.0).unwrap_or_else(|e| panic!("{}: {e:#}", t.0));
        assert_eq!(def.id, t.1);
        assert!(p.get("cells").is_some() || p.get("size").is_some(), "{}", t.0);
    }
    // Every starting point makes a structure with its own settings.
    for t in aurafx_filters::morphology::study::TEMPLATES {
        let start = std::time::Instant::now();
        let out = try_run("microstructure_template", None, &[("template", json!(t.0))]).unwrap_or_else(|e| panic!("{}: {e:#}", t.0));
        assert!(!phases(&out).is_empty(), "{}", t.0);
        println!("{:32} {:6.2} s", t.0, start.elapsed().as_secs_f64());
    }
    let bone = run("microstructure_template", None, &[("cells", json!([40, 40, 40]))]);
    close("trabecular bone", share(&bone, 2), 0.2, 0.01);
    assert!(try_run("microstructure_template", None, &[("template", json!("Softwood")), ("cells", json!([64, 48, 16]))]).is_ok());
    // Grains that nucleate and grow: more is transformed at each step.
    let def = aurafx_filters::registry::find("microstructure_formation").unwrap();
    let mut p = def.default_params();
    p.set_json("settings", &json!("cells = 32 32 32"));
    p.set_json("frames", &json!(6));
    let temporal = def.temporal.as_ref().unwrap();
    assert_eq!((temporal.output_times)(&p, &[]), vec![0.0, 1.0, 2.0, 3.0, 4.0, 5.0]);
    let shares: Vec<f64> = (0..6)
        .map(|k| share(&aurafx_filters::morphology::study::formation("Nucleation and Growth Grains", "cells = 32 32 32", &[], 6, k as f64).unwrap(), 1))
        .collect();
    assert!(shares.windows(2).all(|w| w[1] >= w[0]) && shares[0] < 0.05 && shares[5] > 0.95, "{shares:?}");
    let grown = run("microstructure_formation", None, &[("model", json!("Aggregation Cluster")), ("settings", json!("cells = 48 48 1")), ("frames", json!(4))]);
    close("the last step", share(&grown, 2), 0.08, 0.01);
    // Settings as text.
    let def = aurafx_filters::registry::find("hard_fibres").unwrap();
    let p = aurafx_filters::morphology::study::params_from_text(&def, "radius = 3; Directions = along z; cells = 32 32 16").unwrap();
    assert_eq!((p.f64("radius", 0.0), p.str("directions"), p.floats("cells")), (3.0, "Along Z".to_string(), vec![32.0, 32.0, 16.0]));
    assert!(aurafx_filters::morphology::study::params_from_text(&def, "radios = 3").is_err());
}
