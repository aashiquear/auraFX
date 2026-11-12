//! Orientation gradients inside synthetic grains, measured back by the crate's own SROD,
//! kernel-average and reference misorientation filters; and the grain realism controls
//! (relaxation, coarsening, roughness, competitive growth, layers), judged by the measures a
//! grown structure is known by: boundary length, junction angles, sizes, grains by height.

use aurafx_crystal::{self as cx, Laue, Quat};
use aurafx_data::*;
use aurafx_filters::synthetic as syn;
use serde_json::json;

fn run(id: &str, input: Option<&DataSet>, set: &[(&str, serde_json::Value)]) -> DataSet {
    let def = aurafx_filters::registry::find(id).unwrap_or_else(|| panic!("no filter {id}"));
    let mut p = def.default_params();
    for (k, v) in set {
        p.set_json(k, v);
    }
    def.run(input, &p).unwrap_or_else(|e| panic!("{id}: {e:#}"))
}

fn cells(d: &DataSet) -> [usize; 3] {
    let DataSet::ImageData(i) = d else { panic!() };
    [(i.extent[1] - i.extent[0]).max(1) as usize, (i.extent[3] - i.extent[2]).max(1) as usize, (i.extent[5] - i.extent[4]).max(1) as usize]
}

fn ids_of(d: &DataSet) -> Vec<i32> {
    let a = d.cell_data().unwrap().get("FeatureIds").unwrap();
    (0..a.len()).map(|i| a.get(i, 0) as i32).collect()
}

fn quats(d: &DataSet) -> Vec<Quat> {
    let e = d.cell_data().unwrap().get("EulerAngles").unwrap();
    (0..e.len()).map(|i| cx::euler_to_quat([e.get(i, 0), e.get(i, 1), e.get(i, 2)])).collect()
}

fn cell(d: &DataSet, name: &str) -> Vec<f64> {
    let a = d.cell_data().unwrap().get(name).unwrap_or_else(|| panic!("no {name}"));
    (0..a.len()).map(|i| a.get(i, 0)).collect()
}

fn field(d: &DataSet, name: &str) -> Vec<f64> {
    let a = d.field_data().get(name).unwrap_or_else(|| panic!("no {name}"));
    (0..a.len()).map(|i| a.get(i, 0)).collect()
}

fn median(mut v: Vec<f64>) -> f64 {
    v.sort_by(|a, b| a.total_cmp(b));
    v[v.len() / 2]
}

fn angle(a: Quat, b: Quat) -> f64 {
    cx::disorientation_angle(a, b, Laue::Triclinic).to_degrees()
}

// ------------------------------------------------------------------ orientation gradient

/// A requested gradient along the build direction comes back as the SROD rate of the grains.
#[test]
fn gradient_is_measured_by_srod() {
    let c = run("columnar_grains", None, &[("seed", 3.into())]);
    let rate = 0.02;
    for reference in ["Lowest point", "Centroid"] {
        let g = run(
            "orientation_gradient",
            Some(&c),
            &[("rate", rate.into()), ("rate_spread", 0.0.into()), ("amplitude", 0.0.into()), ("reference", reference.into())],
        );
        let s = run("srod", Some(&g), &[("layer", 40.0.into())]);
        let rates: Vec<f64> = field(&s, "SRODRate").into_iter().filter(|x| x.is_finite()).collect();
        let m = median(rates.clone());
        let worst = rates.iter().map(|r| (r - rate * 40.0).abs()).fold(0.0, f64::max);
        eprintln!("{reference}: {} grains with a rate, median {m:.4} degrees per layer (asked {:.4}), worst {worst:.4}", rates.len(), rate * 40.0);
        // A grain's top bin is rarely full, so its mean lies lower than a full bin's and the
        // fitted slope comes out a little under the true one.
        assert!(rates.len() > 20);
        assert!(m <= rate * 40.0 + 1e-6 && m > 0.9 * rate * 40.0, "{m}");
        assert!(worst < 0.25 * rate * 40.0, "{worst}");
    }
    // Columns through a box of eleven whole layers: every bin is full and the rate is exact.
    let tall = run("columnar_grains", None, &[("seed", 3.into()), ("size", json!([300, 300, 440])), ("height", 5000.0.into()), ("tilt", 0.0.into())]);
    let g = run("orientation_gradient", Some(&tall), &[("rate", rate.into()), ("rate_spread", 0.0.into()), ("amplitude", 0.0.into())]);
    let s = run("srod", Some(&g), &[("layer", 40.0.into())]);
    let full: Vec<f64> = field(&s, "SRODRate").into_iter().zip(field(&s, "SRODBins")).filter(|(r, b)| r.is_finite() && *b == 11.0).map(|x| x.0).collect();
    let worst = full.iter().map(|r| (r - rate * 40.0).abs()).fold(0.0, f64::max);
    eprintln!("columns through eleven whole layers: {} grains, median rate {:.5}, worst error {worst:.5}", full.len(), median(full.clone()));
    assert!(full.len() > 10 && worst < 0.01 * rate * 40.0, "{worst}");
    // A spread of rates: the median stays, the grains differ; the source's own setting gives
    // the same kind of volume.
    let g = run("orientation_gradient", Some(&c), &[("rate", rate.into()), ("rate_spread", 0.5.into()), ("amplitude", 0.3.into())]);
    let rates: Vec<f64> = field(&run("srod", Some(&g), &[("layer", 40.0.into())]), "SRODRate").into_iter().filter(|x| x.is_finite()).collect();
    let (lo, hi) = (rates.iter().copied().fold(f64::INFINITY, f64::min), rates.iter().copied().fold(0.0, f64::max));
    let m = median(rates);
    eprintln!("spread 0.5: median {m:.3}, {lo:.3} to {hi:.3}");
    assert!((m - 0.8).abs() < 0.25 && hi > 1.5 * lo.max(0.05), "{m} {lo} {hi}");
    let at_source = run("columnar_grains", None, &[("seed", 3.into()), ("gradient", rate.into())]);
    let rates: Vec<f64> = field(&run("srod", Some(&at_source), &[("layer", 40.0.into())]), "SRODRate").into_iter().filter(|x| x.is_finite()).collect();
    let m = median(rates);
    eprintln!("gradient set on the source: median {m:.3}");
    assert!((m - 0.8).abs() < 0.25, "{m}");
    assert_eq!(ids_of(&at_source), ids_of(&c));
}

/// The steady turn gives the kernel average misorientation and the reference misorientation
/// it should; the smooth spread has the amplitude asked for; cells have the misorientation
/// asked for across their boundaries.
#[test]
fn gradient_kam_grod_and_cells() {
    let v = run("voronoi_grains", None, &[("cells", json!([40, 40, 40])), ("grains", 30.into())]);
    let n = cells(&v);
    let ids = ids_of(&v);
    let nxy = n[0] * n[1];
    let rate = 0.5;
    let g = run("orientation_gradient", Some(&v), &[("rate", rate.into()), ("rate_spread", 0.0.into()), ("amplitude", 0.0.into())]);
    // KAM along z: the cell itself (0) and the cells above and below (rate each).
    let kam = cell(&run("kernel_average_misorientation", Some(&g), &[("kernel", json!([0, 0, 1]))]), "KernelAverageMisorientations");
    let inner: Vec<f64> = (nxy..ids.len() - nxy).filter(|c| ids[c - nxy] == ids[*c] && ids[c + nxy] == ids[*c]).map(|c| kam[c]).collect();
    let mean = inner.iter().sum::<f64>() / inner.len() as f64;
    eprintln!("KAM along z: {mean:.4} (expected {:.4})", 2.0 / 3.0 * rate);
    assert!((mean - 2.0 / 3.0 * rate).abs() < 0.005, "{mean}");
    // Reference misorientation: rate times the height above or below the grain's centroid.
    let grod = cell(&run("reference_misorientations", Some(&g), &[]), "FeatureReferenceMisorientations");
    let nf = *ids.iter().max().unwrap() as usize + 1;
    let (mut zs, mut cnt) = (vec![0.0; nf], vec![0.0; nf]);
    for (c, f) in ids.iter().enumerate() {
        zs[*f as usize] += (c / nxy) as f64 + 0.5;
        cnt[*f as usize] += 1.0;
    }
    let err = (0..ids.len()).map(|c| (grod[c] - rate * ((c / nxy) as f64 + 0.5 - zs[ids[c] as usize] / cnt[ids[c] as usize]).abs()).abs()).sum::<f64>()
        / ids.len() as f64;
    let top = grod.iter().copied().fold(0.0, f64::max);
    eprintln!("reference misorientation: mean error {err:.4} degrees, largest {top:.2}");
    assert!(err < 0.05 && top > 3.0, "{err} {top}");
    // Smooth spread only: its root mean square is the amplitude.
    let s = run("orientation_gradient", Some(&v), &[("rate", 0.0.into()), ("amplitude", 1.0.into()), ("correlation_length", 3.0.into())]);
    let grod = cell(&run("reference_misorientations", Some(&s), &[]), "FeatureReferenceMisorientations");
    let rms = (grod.iter().map(|x| x * x).sum::<f64>() / grod.len() as f64).sqrt();
    let kam = cell(&run("kernel_average_misorientation", Some(&s), &[]), "KernelAverageMisorientations");
    let kmean = kam.iter().sum::<f64>() / kam.len() as f64;
    eprintln!("smooth spread of 1 degree: RMS reference misorientation {rms:.3}, mean KAM {kmean:.3}");
    assert!((rms - 1.0).abs() < 0.15, "{rms}");
    assert!(kmean > 0.05 && kmean < 0.6, "{kmean}");
    // Cells: neighbours in one grain are either in one cell (equal) or misoriented by 1.5 on average.
    let c = run("orientation_gradient", Some(&v), &[("rate", 0.0.into()), ("amplitude", 0.0.into()), ("cell_size", 5.0.into()), ("cell_angle", 1.5.into())]);
    let q = quats(&c);
    let steps: Vec<f64> = (0..ids.len() - 1).filter(|c| (c + 1) % n[0] != 0 && ids[*c] == ids[c + 1]).map(|c| angle(q[c], q[c + 1])).collect();
    let across: Vec<f64> = steps.iter().copied().filter(|a| *a > 1e-3).collect();
    let share = across.len() as f64 / steps.len() as f64;
    let mean = across.iter().sum::<f64>() / across.len() as f64;
    eprintln!("cells of 5: {:.1} % of neighbours across a cell boundary, mean misorientation {mean:.3} (asked 1.5)", 100.0 * share);
    assert!((mean - 1.5).abs() < 0.15, "{mean}");
    assert!(share > 0.1 && share < 0.35, "{share}");
}

/// Nothing asked: nothing changed. The same seed gives the same field, another seed another;
/// the cap holds and capped grains still segment as they did.
#[test]
fn gradient_off_seed_and_cap() {
    let v = run("voronoi_grains", None, &[("cells", json!([40, 40, 40])), ("grains", 30.into())]);
    let off = run("orientation_gradient", Some(&v), &[("rate", 0.0.into()), ("amplitude", 0.0.into())]);
    assert_eq!(off.cell_data().unwrap().get("EulerAngles"), v.cell_data().unwrap().get("EulerAngles"));
    let plain = run("voronoi_grains", None, &[("cells", json!([40, 40, 40])), ("grains", 30.into()), ("gradient", 0.0.into()), ("realism", 0.0.into())]);
    assert_eq!(plain.cell_data().unwrap().get("EulerAngles"), v.cell_data().unwrap().get("EulerAngles"));
    let set = |seed: i64| run("orientation_gradient", Some(&v), &[("cell_size", 6.0.into()), ("size_exponent", 0.5.into()), ("seed", seed.into())]);
    let (a, b, c) = (set(1), set(1), set(2));
    assert_eq!(a.cell_data().unwrap().get("EulerAngles"), b.cell_data().unwrap().get("EulerAngles"));
    assert_ne!(a.cell_data().unwrap().get("EulerAngles"), c.cell_data().unwrap().get("EulerAngles"));
    // 0.5 degrees per cell over grains some 13 cells across would reach several degrees; capped at 2.
    let q0 = quats(&v);
    let turn = |d: &DataSet| quats(d).iter().zip(&q0).map(|(a, b)| angle(*a, *b)).fold(0.0, f64::max);
    let free = run("orientation_gradient", Some(&v), &[("rate", 0.5.into()), ("amplitude", 1.0.into())]);
    let capped = run("orientation_gradient", Some(&v), &[("rate", 0.5.into()), ("amplitude", 1.0.into()), ("cap", 2.0.into())]);
    let (tf, tc) = (turn(&free), turn(&capped));
    eprintln!("largest turn {tf:.2} degrees without a cap, {tc:.3} with a cap of 2");
    assert!(tf > 4.0 && tc <= 2.0 + 1e-3 && tc > 1.8, "{tf} {tc}");
    let count = |d: &DataSet| *ids_of(&run("segment_features", Some(d), &[("tolerance", 5.0.into())])).iter().max().unwrap();
    assert_eq!(count(&capped), count(&v));
}

// ------------------------------------------------------------------ grain realism

/// Boundary length of a 2-D label map by the four-direction Crofton estimate, in cells.
fn boundary_length(ids: &[i32], nx: usize, ny: usize) -> f64 {
    let at = |i: usize, j: usize| ids[j * nx + i];
    let (mut nxc, mut nyc, mut nd) = (0usize, 0usize, 0usize);
    for j in 0..ny {
        for i in 0..nx {
            if i + 1 < nx && at(i, j) != at(i + 1, j) {
                nxc += 1;
            }
            if j + 1 < ny && at(i, j) != at(i, j + 1) {
                nyc += 1;
            }
            if i + 1 < nx && j + 1 < ny {
                nd += (at(i, j) != at(i + 1, j + 1)) as usize + (at(i + 1, j) != at(i, j + 1)) as usize;
            }
        }
    }
    std::f64::consts::FRAC_PI_2 * (nxc as f64 + nyc as f64 + nd as f64 / 2f64.sqrt()) / 4.0
}

/// Angles between the three boundaries at every triple junction of a 2-D label map, from the
/// boundary points within `radius` cells of the junction: (junctions, RMS deviation from 120).
fn junction_spread(ids: &[i32], nx: usize, ny: usize, radius: f64) -> (usize, f64) {
    let at = |i: usize, j: usize| ids[j * nx + i];
    let mut found: Vec<(f64, f64, [i32; 3])> = Vec::new();
    for j in 0..ny - 1 {
        for i in 0..nx - 1 {
            let mut u = vec![at(i, j), at(i + 1, j), at(i, j + 1), at(i + 1, j + 1)];
            u.sort();
            u.dedup();
            if u.len() != 3 {
                continue;
            }
            let (x, y, key) = (i as f64 + 0.5, j as f64 + 0.5, [u[0], u[1], u[2]]);
            if !found.iter().any(|f| f.2 == key && (f.0 - x).powi(2) + (f.1 - y).powi(2) < radius * radius) {
                found.push((x, y, key));
            }
        }
    }
    let (mut sum, mut count, mut junctions) = (0.0, 0usize, 0usize);
    for (x, y, key) in &found {
        // Junctions too near the edge have their boundaries cut short.
        if *x < radius || *y < radius || *x > nx as f64 - 1.0 - radius || *y > ny as f64 - 1.0 - radius {
            continue;
        }
        let (i0, i1, j0, j1) = ((x - radius) as usize, (x + radius) as usize + 1, (y - radius) as usize, (y + radius) as usize + 1);
        let mut dirs = Vec::new();
        for pair in [(key[0], key[1]), (key[1], key[2]), (key[0], key[2])] {
            let is = |a: i32, b: i32| (a, b) == pair || (b, a) == pair;
            let (mut sx, mut sy, mut m) = (0.0, 0.0, 0usize);
            for j in j0..=j1.min(ny - 1) {
                for i in i0..=i1.min(nx - 1) {
                    for (pi, pj, ok) in [
                        (i as f64 + 0.5, j as f64, i + 1 < nx && is(at(i, j), at((i + 1).min(nx - 1), j))),
                        (i as f64, j as f64 + 0.5, j + 1 < ny && is(at(i, j), at(i, (j + 1).min(ny - 1)))),
                    ] {
                        if ok && (pi - x).powi(2) + (pj - y).powi(2) <= radius * radius {
                            sx += pi - x;
                            sy += pj - y;
                            m += 1;
                        }
                    }
                }
            }
            if m >= 3 && sx.hypot(sy) > 1e-9 {
                dirs.push(sy.atan2(sx).to_degrees().rem_euclid(360.0));
            }
        }
        if dirs.len() != 3 {
            continue;
        }
        dirs.sort_by(|a, b| a.total_cmp(b));
        for a in [dirs[1] - dirs[0], dirs[2] - dirs[1], 360.0 - dirs[2] + dirs[0]] {
            sum += (a - 120.0) * (a - 120.0);
            count += 1;
        }
        junctions += 1;
    }
    (junctions, (sum / count.max(1) as f64).sqrt())
}

/// Faces between cells of different grains.
fn boundary_faces(ids: &[i32], n: [usize; 3]) -> usize {
    let mut faces = 0;
    for c in 0..ids.len() {
        let (i, j, k) = (c % n[0], (c / n[0]) % n[1], c / (n[0] * n[1]));
        faces += (i + 1 < n[0] && ids[c] != ids[c + 1]) as usize
            + (j + 1 < n[1] && ids[c] != ids[c + n[0]]) as usize
            + (k + 1 < n[2] && ids[c] != ids[c + n[0] * n[1]]) as usize;
    }
    faces
}

fn contiguous(ids: &[i32]) -> usize {
    let top = *ids.iter().max().unwrap() as usize;
    let mut seen = vec![false; top + 1];
    for i in ids {
        assert!(*i >= 1);
        seen[*i as usize] = true;
    }
    assert!(seen[1..].iter().all(|s| *s), "ids are not contiguous from 1");
    top
}

/// Relaxation shortens the boundaries of a 2-D Voronoi map sweep by sweep and brings its
/// triple junctions towards 120 degrees.
#[test]
fn relaxation_shortens_boundaries_and_opens_junctions() {
    let (mut l0, mut l4, mut s0, mut s4, mut junctions) = (0.0, 0.0, 0.0, 0.0, 0usize);
    for seed in 1..=4 {
        let v = run("voronoi_grains", None, &[("cells", json!([160, 160, 1])), ("grains", 45.into()), ("seed", seed.into())]);
        let (nx, ny) = (160, 160);
        let before = ids_of(&v);
        let (j0, r0) = junction_spread(&before, nx, ny, 8.0);
        let mut last = boundary_length(&before, nx, ny);
        l0 += last;
        s0 += r0 * r0 * j0 as f64;
        junctions += j0;
        for sweeps in [1, 2, 4] {
            let r = run("realistic_grains", Some(&v), &[("relax", sweeps.into()), ("width", 3.0.into())]);
            let ids = ids_of(&r);
            let len = boundary_length(&ids, nx, ny);
            assert!(len < last, "seed {seed}, {sweeps} sweeps: {len} is not below {last}");
            last = len;
            if sweeps == 4 {
                let (j4, r4) = junction_spread(&ids, nx, ny, 8.0);
                l4 += len;
                s4 += r4 * r4 * j4 as f64;
                contiguous(&ids);
            }
        }
    }
    let (r0, r4) = ((s0 / junctions as f64).sqrt(), (s4 / junctions as f64).sqrt());
    eprintln!(
        "{junctions} junctions: RMS deviation from 120 degrees {r0:.1} -> {r4:.1}; boundary length {l0:.0} -> {l4:.0} cells ({:.1} %)",
        100.0 * (l4 / l0 - 1.0)
    );
    assert!(r4 < 0.7 * r0, "{r0} {r4}");
    assert!(l4 < 0.98 * l0);
}

/// In 3-D: roughness lengthens the boundaries, relaxation shortens them again; coarsening
/// removes the share of grains asked for; ids stay contiguous, per-feature arrays keep one
/// row per grain and every cell carries its grain's orientation.
#[test]
fn realism_filter_keeps_the_volume_consistent() {
    let v = run("voronoi_grains", None, &[("cells", json!([48, 48, 48])), ("grains", 60.into())]);
    let n = cells(&v);
    let f0 = boundary_faces(&ids_of(&v), n);
    let rough = run("realistic_grains", Some(&v), &[("relax", 0.into()), ("roughness", 1.5.into()), ("roughness_length", 5.0.into())]);
    let fr = boundary_faces(&ids_of(&rough), n);
    let relaxed = run("realistic_grains", Some(&rough), &[("relax", 3.into())]);
    let fx = boundary_faces(&ids_of(&relaxed), n);
    eprintln!("boundary faces: {f0} tessellated, {fr} roughened, {fx} relaxed");
    assert!(fr > f0 + f0 / 50 && fx < fr - fr / 50, "{f0} {fr} {fx}");
    let all = run("realistic_grains", Some(&v), &[("relax", 4.into()), ("coarsen", 0.25.into()), ("roughness", 1.0.into())]);
    let ids = ids_of(&all);
    let grains = contiguous(&ids);
    eprintln!("coarsened by a quarter and relaxed: {grains} of 60 grains");
    assert!(grains <= 45 && grains >= 40, "{grains}");
    assert_eq!(all.field_data().get("Seeds").unwrap().len(), grains + 1);
    let q = quats(&all);
    let mut first: Vec<Option<Quat>> = vec![None; grains + 1];
    for (c, f) in ids.iter().enumerate() {
        let o = first[*f as usize].get_or_insert(q[c]);
        assert!(angle(*o, q[c]) < 1e-3, "grain {f} has more than one orientation");
    }
    // The same seed gives the same volume, another seed another roughness.
    let again = run("realistic_grains", Some(&v), &[("relax", 4.into()), ("coarsen", 0.25.into()), ("roughness", 1.0.into())]);
    assert_eq!(ids_of(&again), ids);
    let other = run("realistic_grains", Some(&v), &[("relax", 4.into()), ("coarsen", 0.25.into()), ("roughness", 1.0.into()), ("seed", 2.into())]);
    assert_ne!(ids_of(&other), ids);
    // Nothing asked: nothing changed.
    let off = run("realistic_grains", Some(&v), &[("relax", 0.into())]);
    assert_eq!(ids_of(&off), ids_of(&v));
    // Periodic: a periodic volume shifted round by some cells, coarsened and relaxed, is the
    // coarsened and relaxed volume shifted round (ids apart: compare which cells share a grain).
    let p = run("voronoi_grains", None, &[("cells", json!([40, 40, 40])), ("grains", 40.into()), ("periodic", true.into())]);
    let DataSet::ImageData(pi) = &p else { panic!() };
    let roll: Vec<usize> = (0..40 * 40 * 40).map(|c| (c / 1600 + 5) % 40 * 1600 + ((c / 40) % 40 + 11) % 40 * 40 + (c % 40 + 7) % 40).collect();
    let mut shifted = pi.clone();
    shifted.cell_data = pi.cell_data.gather(&roll);
    let opts = [("relax", 3.into()), ("width", 2.0.into()), ("coarsen", 0.2.into()), ("periodic", true.into())];
    let (a, b) = (ids_of(&run("realistic_grains", Some(&p), &opts)), ids_of(&run("realistic_grains", Some(&DataSet::ImageData(shifted)), &opts)));
    let mut pair = std::collections::HashMap::new();
    let consistent = roll.iter().enumerate().all(|(c, from)| *pair.entry(b[c]).or_insert(a[*from]) == a[*from]);
    assert!(consistent, "relaxing a periodic volume depends on where it is cut");
    assert_ne!(a, ids_of(&p));
    let real = run("voronoi_grains", None, &[("cells", json!([40, 40, 40])), ("grains", 40.into()), ("periodic", true.into()), ("realism", 1.0.into())]);
    let ids = ids_of(&real);
    contiguous(&ids);
    let same = |a: usize, b: usize| (0..1600).filter(|r| ids[r * 40 + a] == ids[r * 40 + b]).count() as f64 / 1600.0;
    let inside = (1..40).map(|i| same(i, i - 1)).sum::<f64>() / 39.0;
    eprintln!("periodic with realism 1: {:.2} of the cells continue across the x faces, {inside:.2} across planes inside", same(0, 39));
    assert!(same(0, 39) > inside - 0.1);
}

/// Realism on the Voronoi source: off, it is the plain tessellation cell for cell; on, the
/// sizes spread (the log-sigma of the diameters grows) and fewer grains are left.
#[test]
fn voronoi_realism() {
    let set = |realism: f64| {
        run("voronoi_grains", None, &[("cells", json!([60, 60, 60])), ("grains", 200.into()), ("periodic", true.into()), ("realism", realism.into())])
    };
    let plain = set(0.0);
    // The tessellation itself, from its parts.
    let mut r = syn::Rng::new(1);
    use syn::Draws;
    let seeds: Vec<[f64; 3]> = (0..200).map(|_| std::array::from_fn(|_| r.uniform(0.0, 60.0))).collect();
    let labels = syn::power_labels([60, 60, 60], [1.0; 3], &seeds, &vec![0.0; 200], true);
    let mut map = std::collections::HashMap::new();
    let expect: Vec<i32> = labels
        .iter()
        .map(|l| {
            let k = map.len() as i32 + 1;
            *map.entry(*l).or_insert(k)
        })
        .collect();
    assert_eq!(ids_of(&plain), expect);
    let orient = syn::grain_orientations_pub(map.len(), "Random", 0.0, &mut r);
    let q = quats(&plain);
    assert!((0..q.len()).step_by(101).all(|c| angle(q[c], orient[(expect[c] - 1) as usize]) < 1e-3));
    let sigma = |d: &DataSet| {
        let st = run("ensemble_statistics", Some(d), &[("include_surface", true.into())]);
        (st.field_data().get("Features").unwrap().get(0, 0), st.field_data().get("ESD sigma").unwrap().get(0, 0))
    };
    let real = set(1.0);
    let ((n0, s0), (n1, s1)) = (sigma(&plain), sigma(&real));
    eprintln!("realism 0: {n0} grains, ESD sigma {s0:.3}; realism 1: {n1} grains, ESD sigma {s1:.3}");
    assert!(s1 > s0 + 0.03 && n1 < n0, "{s0} {s1}");
    contiguous(&ids_of(&real));
    assert_eq!(real.field_data().get("Seeds").unwrap().len(), n1 as usize + 1);
    assert_eq!(ids_of(&set(1.0)), ids_of(&real));
}

fn grains_in_slice(ids: &[i32], n: [usize; 3], k: usize) -> usize {
    let mut s: Vec<i32> = ids[k * n[0] * n[1]..(k + 1) * n[0] * n[1]].to_vec();
    s.sort_unstable();
    s.dedup();
    s.len()
}

/// Mean angle (degrees) between the build direction and the nearest <100> over a slice.
fn misalignment(d: &DataSet, n: [usize; 3], k: usize) -> f64 {
    let q = quats(d);
    let cells = k * n[0] * n[1]..(k + 1) * n[0] * n[1];
    cells.clone().map(|c| cx::rotate(q[c], [0.0, 0.0, 1.0]).iter().fold(0.0f64, |m, x| m.max(x.abs())).min(1.0).acos().to_degrees()).sum::<f64>()
        / cells.len() as f64
}

/// Realism on the columnar source: off, the plain columns cell for cell; with competitive
/// growth the grains thin out with height and the survivors are the well-aligned ones;
/// layers start new grains at their boundaries.
#[test]
fn columnar_realism() {
    let plain = run("columnar_grains", None, &[("realism", 0.0.into())]);
    let n = cells(&plain);
    let mut r = syn::Rng::new(1);
    let p = syn::ColumnParams {
        size: [300.0, 300.0, 450.0],
        voxel: 5.0,
        width: 50.0,
        height: 150.0,
        height_sigma: 0.5,
        tilt_sd_deg: 6.0,
        width_sigma: 0.0,
        width_height_exponent: 0.0,
    };
    let col = syn::columns(&p, &mut r).unwrap();
    assert_eq!(ids_of(&plain), col.labels.iter().map(|l| *l as i32 + 1).collect::<Vec<_>>());
    let orient = syn::grain_orientations_pub(col.base.len(), "<100> along Z", 10f64.to_radians(), &mut r);
    let q = quats(&plain);
    assert!((0..q.len()).step_by(101).all(|c| angle(q[c], orient[col.labels[c] as usize]) < 1e-3));
    let (b0, t0) = (grains_in_slice(&ids_of(&plain), n, 1), grains_in_slice(&ids_of(&plain), n, n[2] - 2));
    // Competitive growth alone.
    let comp = run("columnar_grains", None, &[("competitive", 1.0.into()), ("height", 2000.0.into())]);
    let ids = ids_of(&comp);
    contiguous(&ids);
    let counts: Vec<usize> = [1, n[2] / 3, 2 * n[2] / 3, n[2] - 2].iter().map(|k| grains_in_slice(&ids, n, *k)).collect();
    let (ab, at) = (misalignment(&comp, n, 1), misalignment(&comp, n, n[2] - 2));
    eprintln!("plain columns: {b0} grains at the bottom, {t0} at the top; competitive: {counts:?} by height, <100> off the build direction by {ab:.1} degrees at the bottom, {at:.1} at the top");
    assert!(t0 * 10 >= b0 * 6);
    assert!(counts[3] * 2 < counts[0] && counts[1] <= counts[0] && counts[3] <= counts[1], "{counts:?}");
    assert!(at < 0.7 * ab, "{ab} {at}");
    // Layers: grains that start on a layer boundary.
    let starts = |d: &DataSet| field(d, "GrainBase").iter().filter(|b| **b > 0.0 && (*b / 50.0).fract() == 0.0).count();
    let layered = run("columnar_grains", None, &[("layer", 50.0.into()), ("epitaxial", 0.5.into())]);
    eprintln!("grains starting on a multiple of 50: {} plain, {} with half the columns stopping at each layer", starts(&plain), starts(&layered));
    assert!(starts(&layered) > starts(&plain) + 30);
    // Fine grains at the layer boundaries add grains; a lean changes the columns.
    let fine = run("columnar_grains", None, &[("layer", 50.0.into()), ("equiaxed", 0.5.into())]);
    let lean = run("columnar_grains", None, &[("layer", 50.0.into()), ("scan_tilt", 15.0.into())]);
    assert!(contiguous(&ids_of(&fine)) > contiguous(&ids_of(&plain)) + 20);
    assert_ne!(ids_of(&lean), ids_of(&plain));
    // Everything together: consistent arrays, the same again with the same seed.
    let real = run("columnar_grains", None, &[("realism", 0.8.into())]);
    let ids = ids_of(&real);
    let grains = contiguous(&ids);
    for name in ["GrainBase", "GrainTop", "GrainTilt"] {
        assert_eq!(field(&real, name).len(), grains + 1, "{name}");
    }
    let (base, top) = (field(&real, "GrainBase"), field(&real, "GrainTop"));
    for (c, f) in ids.iter().enumerate().step_by(53) {
        let z = ((c / (n[0] * n[1])) as f64 + 0.5) * 5.0;
        assert!(z > base[*f as usize] && z < top[*f as usize]);
    }
    assert_eq!(ids_of(&run("columnar_grains", None, &[("realism", 0.8.into())])), ids);
    eprintln!("realism 0.8: {grains} grains, {} at the bottom, {} at the top", grains_in_slice(&ids, n, 1), grains_in_slice(&ids, n, n[2] - 2));
}
