//! Feature (grain) filters against DREAM.3D's own results on a window of NIST's AM Bench 2022
//! IN718 1 um EBSD volume (testdata/dream3d/am_bench_features.dream3d, scripts/gen_dream3d_window.py),
//! and against closed forms on synthetic shapes.

use aurafx_crystal::{self as cx, Laue};
use aurafx_data::*;
use aurafx_filters::features::{self as ft, Moments, Orients, Vox};
use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

fn load(name: &str) -> DataSet {
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../testdata").join(name);
    match aurafx_io::read_file(p).unwrap() {
        aurafx_io::Loaded::Static(d) => (*d).clone(),
        _ => panic!(),
    }
}

fn img(d: &DataSet) -> &ImageData {
    match d {
        DataSet::ImageData(i) => i,
        _ => panic!("not image data"),
    }
}

fn ids_of(i: &ImageData, name: &str) -> Vec<i32> {
    let a = i.cell_data.get(name).unwrap();
    (0..a.len()).map(|k| a.get(k, 0) as i32).collect()
}

/// DREAM.3D's features that lie wholly inside the window (their statistics are complete).
fn interior(v: &Vox, ids: &[i32]) -> BTreeSet<i32> {
    let mut touch = BTreeSet::new();
    for (c, f) in ids.iter().enumerate() {
        let p = v.ijk(c);
        if (0..3).any(|a| p[a] == 0 || p[a] + 1 == v.n[a]) {
            touch.insert(*f);
        }
    }
    ids.iter().copied().filter(|f| *f > 0 && !touch.contains(f)).collect()
}

#[test]
fn features_match_dream3d() {
    let d = load("dream3d/am_bench_features.dream3d");
    let im = img(&d);
    let v = Vox::of(im);
    let ref_ids = ids_of(im, "FeatureIds");
    let inner = interior(&v, &ref_ids);
    assert!(inner.len() >= 40, "{}", inner.len());
    let fd = &im.field_data;

    // Segmentation (2 degrees within SampleVoxels), Fill Bad Data 7, Minimum Size 7 give the
    // same cells for every feature wholly inside the window.
    let s = ft::segment(&d, "Misorientation", 2.0, Some("SampleVoxels"), None, "From the phases", None, None, None).unwrap();
    let s = ft::fill_bad_data(&s, 7, "FeatureIds").unwrap();
    let s = ft::minimum_size(&s, 7, "FeatureIds").unwrap();
    let ours = ids_of(img(&s), "FeatureIds");
    let mut cells: BTreeMap<i32, Vec<usize>> = BTreeMap::new();
    for (c, f) in ref_ids.iter().enumerate() {
        cells.entry(*f).or_default().push(c);
    }
    let mut ours_count: BTreeMap<i32, usize> = BTreeMap::new();
    for f in &ours {
        *ours_count.entry(*f).or_default() += 1;
    }
    let mut same = 0;
    let mut joined = 0;
    for f in &inner {
        let cs = &cells[f];
        let g = ours[cs[0]];
        if g > 0 && cs.iter().all(|c| ours[*c] == g) && ours_count[&g] == cs.len() {
            same += 1;
        } else {
            let o = Orients::of(&d, None, "From the phases", None).unwrap();
            let mut ws = Vec::new();
            for c in cs {
                for nb in v.nbrs(*c).into_iter().flatten() {
                    if ref_ids[nb] != *f && ours[nb] == g {
                        let (q1, l) = o.get(*c).unwrap();
                        let (q2, _) = o.get(nb).unwrap();
                        ws.push((ft::miso(q1, q2, l).to_degrees(), ref_ids[nb]));
                    }
                }
            }
            ws.sort_by(|a, b| a.0.total_cmp(&b.0));
            // Accepted when the stored orientations themselves join the two features (the
            // reference segmented before its clean-up copied orientations into the joining
            // cells).
            if ws.first().is_some_and(|w| w.0 < 2.0) {
                joined += 1;
            }
        }
    }
    assert_eq!(same + joined, inner.len(), "features with the same cells");
    assert!(joined * 20 <= inner.len(), "{joined} joined");

    // Statistics on DREAM.3D's own feature ids.
    let st = ft::feature_sizes(&d, "FeatureIds").unwrap();
    let st = ft::feature_centroids(&st, "FeatureIds").unwrap();
    let sf = &img(&st).field_data;
    let close = |what: &str, f: i32, a: f64, b: f64, tol: f64| assert!((a - b).abs() <= tol * b.abs().max(1.0), "{what} of feature {f}: {a} vs {b}");
    for &f in &inner {
        let u = f as usize;
        close("NumElements", f, sf.get("NumElements").unwrap().get(u, 0), fd.get("NumElements").unwrap().get(u, 0), 0.0);
        close("Volumes", f, sf.get("Volumes").unwrap().get(u, 0), fd.get("Volumes").unwrap().get(u, 0), 1e-6);
        close("EquivalentDiameters", f, sf.get("EquivalentDiameters").unwrap().get(u, 0), fd.get("EquivalentDiameters").unwrap().get(u, 0), 1e-6);
        for k in 0..3 {
            close("Centroids", f, sf.get("Centroids").unwrap().get(u, k), fd.get("Centroids").unwrap().get(u, k), 1e-6);
        }
    }
    // Shapes with the moment scheme the reference used (cells half a cell low, sampled at
    // quarter-cell points), to check the axes, Omega3 and axis directions.
    let nf = (ref_ids.iter().copied().max().unwrap() + 1) as usize;
    let cents: Vec<[f64; 3]> = (0..nf).map(|f| std::array::from_fn(|k| fd.get("Centroids").unwrap().get(f, k))).collect();
    let sh = ft::shapes(&v, &ref_ids, Moments::CornerQuarter, &cents);
    for &f in &inner {
        let u = f as usize;
        let s = sh[u].as_ref().unwrap();
        for k in 0..3 {
            close("AxisLengths", f, s.axes[k], fd.get("AxisLengths").unwrap().get(u, k), 2e-3);
        }
        close("Omega3s", f, s.omega3, fd.get("Omega3s").unwrap().get(u, 0), 2e-3);
        let e = fd.get("AxisEulerAngles").unwrap();
        let g = cx::euler_to_matrix([e.get(u, 0), e.get(u, 1), e.get(u, 2)]);
        // The shortest axis's direction agrees for every feature (the stored directions of the
        // two longer axes are turned within their plane for about half the features, while
        // their lengths agree; ours pass the rotated-ellipsoid test below).
        let dot2 = (0..3).map(|k| g[2][k] * s.dirs[2][k]).sum::<f64>().abs();
        assert!(dot2 > 0.9999, "shortest axis of feature {f}: {dot2}");
    }

    // Average orientations, reference misorientations, KAM.
    let o = Orients::of(&d, None, "From the phases", None).unwrap();
    let avg = ft::average_quats(&o, &ref_ids, nf);
    let aq = fd.get("AvgQuats").unwrap();
    for &f in &inner {
        let u = f as usize;
        let theirs = cx::quat_from_xyzw([aq.get(u, 0), aq.get(u, 1), aq.get(u, 2), aq.get(u, 3)]);
        let w = ft::miso(avg[u].unwrap(), theirs, Laue::CubicHigh).to_degrees();
        // Ours fit the cells at least as well (mean squared misorientation of the feature's
        // cells to the average); the stored averages lie up to 1.6 degrees away.
        let (mut ss_ours, mut ss_theirs) = (0.0f64, 0.0);
        for &c in &cells[&f] {
            let (q, l) = o.get(c).unwrap();
            ss_ours += ft::miso(q, avg[u].unwrap(), l).powi(2);
            ss_theirs += ft::miso(q, theirs, l).powi(2);
        }
        assert!(ss_ours <= ss_theirs * (1.0 + 1e-9), "AvgQuats of feature {f}: ours fit worse");
        assert!(w < 2.0, "AvgQuats of feature {f}: {w} degrees apart");
    }
    // Reference misorientations against the stored averages (the input carries them).
    let r = ft::reference_misorientations(&d, "FeatureIds", None, "From the phases", None).unwrap();
    let (rc, rf) = (img(&r).cell_data.get("FeatureReferenceMisorientations").unwrap(), img(&r).field_data.get("FeatureAvgMisorientations").unwrap());
    let theirs_c = im.cell_data.get("FeatureReferenceMisorientations").unwrap();
    for &f in &inner {
        close("FeatureAvgMisorientations", f, rf.get(f as usize, 0), fd.get("FeatureAvgMisorientations").unwrap().get(f as usize, 0), 1e-2);
        for &c in &cells[&f] {
            assert!((rc.get(c, 0) - theirs_c.get(c, 0)).abs() < 0.05, "reference misorientation of cell {c}: {} vs {}", rc.get(c, 0), theirs_c.get(c, 0));
        }
    }
    // KAM with the kernel the reference ran (2 2 1, the cell itself counted), away from the
    // window's edge.
    let kam = ft::kam_kernel(&v, &o, &ref_ids, [-2, -2, -1], [2, 2, 1], true);
    let theirs_k = im.cell_data.get("KernelAverageMisorientations").unwrap();
    let (mut n, mut off) = (0, 0);
    for c in 0..ref_ids.len() {
        let p = v.ijk(c);
        if p[0] >= 2 && p[0] + 2 < v.n[0] && p[1] >= 2 && p[1] + 2 < v.n[1] && p[2] >= 1 && p[2] + 1 < v.n[2] {
            if (kam[c] - theirs_k.get(c, 0)).abs() >= 0.01 {
                off += 1;
            }
            n += 1;
        }
    }
    assert!(n > 50_000 && off * 1000 <= n, "KAM differs on {off} of {n} cells");
    // Surface features: wholly-inside features touching the sample's outside (feature 0).
    let nb = ft::feature_neighbors(&d, "FeatureIds").unwrap();
    let sfl = img(&nb).field_data.get("SurfaceFeatures").unwrap();
    for &f in &inner {
        assert_eq!(sfl.get(f as usize, 0), fd.get("SurfaceFeatures").unwrap().get(f as usize, 0), "SurfaceFeatures of {f}");
    }
}

/// A voxelised ellipsoid: exact axes, Omega3 = 1 and the axis directions.
#[test]
fn shapes_of_an_ellipsoid() {
    let (a, b, c) = (24.0, 14.0, 8.0);
    let rot = cx::euler_to_matrix([0.4, 0.7, 1.1]);
    let n = 56usize;
    let mut ids = vec![0i32; n * n * n];
    for k in 0..n {
        for j in 0..n {
            for i in 0..n {
                let p = [i as f64 + 0.5 - 28.0, j as f64 + 0.5 - 28.0, k as f64 + 0.5 - 28.0];
                // Body coordinates: rows of rot are the body axes.
                let q: Vec<f64> = (0..3).map(|r| (0..3).map(|s| rot[r][s] * p[s]).sum()).collect();
                if (q[0] / a).powi(2) + (q[1] / b).powi(2) + (q[2] / c).powi(2) <= 1.0 {
                    ids[(k * n + j) * n + i] = 1;
                }
            }
        }
    }
    let mut im = ImageData { extent: [0, n as i64, 0, n as i64, 0, n as i64], ..Default::default() };
    im.cell_data.insert(Array::new("FeatureIds", 1, ArrayData::I32(ids)));
    let out = ft::feature_shapes(&DataSet::ImageData(im), "FeatureIds").unwrap();
    let fd = &img(&out).field_data;
    let ax = fd.get("AxisLengths").unwrap();
    for (k, want) in [a, b, c].iter().enumerate() {
        assert!((ax.get(1, k) - want).abs() / want < 0.02, "axis {k}: {}", ax.get(1, k));
    }
    assert!((fd.get("Omega3s").unwrap().get(1, 0) - 1.0).abs() < 0.01);
    let e = fd.get("AxisEulerAngles").unwrap();
    let g = cx::euler_to_matrix([e.get(1, 0), e.get(1, 1), e.get(1, 2)]);
    for r in 0..3 {
        let dot: f64 = (0..3).map(|k| g[r][k] * rot[r][k]).sum();
        assert!(dot.abs() > 0.999, "axis {r}: {dot}");
    }
    let ar = fd.get("AspectRatios").unwrap();
    assert!((ar.get(1, 0) - b / a).abs() < 0.02 && (ar.get(1, 1) - c / a).abs() < 0.02);
}

/// Twins, scalar and c-axis segmentation, clean-up and the table on small synthetic grids.
#[test]
fn twins_segmentation_and_cleanup() {
    // Three slabs along x: A, its twin (60 degrees about [111]) and a random orientation.
    let a = cx::euler_to_quat([0.3, 0.5, 0.2]);
    // A twin: 60 degrees about [111] in the crystal frame (on the left).
    let twin = cx::mul(cx::axis_angle_to_quat([1.0, 1.0, 1.0], 60f64.to_radians()), a);
    let other = cx::euler_to_quat([1.9, 0.8, 2.6]);
    let (nx, ny, nz) = (12usize, 4usize, 3usize);
    let mut eul = Vec::new();
    let mut val = Vec::new();
    for _k in 0..nz {
        for _j in 0..ny {
            for i in 0..nx {
                let q = if i < 4 {
                    a
                } else if i < 8 {
                    twin
                } else {
                    other
                };
                eul.extend(cx::quat_to_euler(q).map(|v| v as f32));
                val.push(if i < 6 { 1.0 } else { 5.0 });
            }
        }
    }
    let mut im = ImageData { extent: [0, nx as i64, 0, ny as i64, 0, nz as i64], ..Default::default() };
    im.cell_data.insert(Array::new("EulerAngles", 3, ArrayData::F32(eul)));
    im.cell_data.insert(Array::scalars_f64("T", val));
    im.field_data.insert(Array::new("CrystalStructures", 1, ArrayData::U32(vec![999, 1])));
    let d = DataSet::ImageData(im);
    let s = ft::segment(&d, "Misorientation", 5.0, None, None, "From the phases", None, None, None).unwrap();
    let ids = ids_of(img(&s), "FeatureIds");
    assert_eq!((ids[0], ids[5], ids[10]), (1, 2, 3));
    let s = ft::average_orientations(&s, "FeatureIds", None, "From the phases", None).unwrap();
    let m = ft::merge_twins(&s, "FeatureIds", 3.0, 2.0).unwrap();
    let p = img(&m).field_data.get("ParentIds").unwrap();
    assert_eq!((p.get(1, 0), p.get(2, 0), p.get(3, 0)), (1.0, 1.0, 2.0));
    let gb = ft::feature_boundaries(&s, "FeatureIds", false, 3.0, 2.0, None, "From the phases", None).unwrap();
    let DataSet::PolyData(pd) = &gb else { panic!() };
    assert_eq!(pd.polys.len(), 2 * ny * nz);
    let (fa, tw, mi) = (pd.cell_data.get("FeatureIdA").unwrap(), pd.cell_data.get("Twin").unwrap(), pd.cell_data.get("Misorientation").unwrap());
    for k in 0..pd.polys.len() {
        let twin_face = fa.get(k, 0) == 1.0;
        assert_eq!(tw.get(k, 0) == 1.0, twin_face);
        if twin_face {
            assert!((mi.get(k, 0) - 60.0).abs() < 1e-3);
        }
    }
    // Scalar segmentation: two features (values 1 and 5).
    let sc = ft::segment(&d, "Scalar", 0.5, None, None, "From the phases", None, Some((aurafx_filters::Association::Cells, "T".into())), None).unwrap();
    let ids = ids_of(img(&sc), "FeatureIds");
    assert_eq!(ids.iter().copied().max(), Some(2));
    // C-axis: the twin's c axis differs, so three features again; a huge tolerance merges all.
    let ca = ft::segment(&d, "C-axis alignment", 179.0, None, None, "From the phases", None, None, None).unwrap();
    assert_eq!(ids_of(img(&ca), "FeatureIds").iter().copied().max(), Some(1));
    // Fill Bad Data: a hole of 2 cells inside feature 1 is filled with feature 1's values.
    let mut hole = img(&s).clone();
    let mut fid = ids_of(&hole, "FeatureIds");
    fid[(nx) + 1] = 0;
    fid[(nx) + 2] = 0;
    hole.cell_data.insert(Array::new("FeatureIds", 1, ArrayData::I32(fid)));
    let filled = ft::fill_bad_data(&DataSet::ImageData(hole.clone()), 3, "FeatureIds").unwrap();
    assert_eq!(ids_of(img(&filled), "FeatureIds")[nx + 1], 1);
    let kept = ft::fill_bad_data(&DataSet::ImageData(hole), 2, "FeatureIds").unwrap();
    assert_eq!(ids_of(img(&kept), "FeatureIds")[nx + 1], 0);
    // Minimum size removes nothing here (16 cells each) but 17 removes all but... every feature
    // has 48 cells: 49 is an error, 48 keeps all.
    assert!(ft::minimum_size(&s, 49, "FeatureIds").is_err());
    let ms = ft::minimum_size(&s, 48, "FeatureIds").unwrap();
    assert_eq!(img(&ms).field_data.get("AvgQuats").unwrap().len(), 4);
    // Table: a row per feature.
    let t = ft::feature_table(&s, "FeatureIds").unwrap();
    let DataSet::Table(t) = t else { panic!() };
    assert_eq!(t.columns.get("FeatureId").unwrap().len(), 3);
    assert!(t.columns.get("AvgQuats").is_some());
}
