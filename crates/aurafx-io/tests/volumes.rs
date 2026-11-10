//! The volume readers on testdata/volumes (written by scripts/gen_volumes.py, where every value
//! v = 100 k + 10 j + i tells its position). Spacings, origins and values agree with ParaView
//! 6.1.1's readers for local.mha, zipped.mhd, float.mrc, vector.nhdr and the DICOM spacing and
//! row order (see docs/reference/verification.md for where they differ).
use aurafx_data::*;
use aurafx_io::{read_file, Loaded};

fn root() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../testdata/volumes")
}

fn image(name: &str) -> ImageData {
    match read_file(root().join(name)).unwrap_or_else(|e| panic!("{name}: {e:#}")) {
        Loaded::Static(d) => match &*d {
            DataSet::ImageData(i) => i.clone(),
            other => panic!("{name}: {}", other.type_name()),
        },
        _ => panic!("{name}: a series"),
    }
}

fn v(i: usize, j: usize, k: usize) -> f64 {
    (100 * k + 10 * j + i) as f64
}

fn check_values(im: &ImageData, name: &str, f: impl Fn(f64) -> f64) {
    let a = im.point_data.get(name).unwrap_or_else(|| panic!("no array {name}"));
    let [nx, ny, nz] = im.dims();
    for k in 0..nz {
        for j in 0..ny {
            for i in 0..nx {
                let got = a.get(i + nx * (j + ny * k), 0);
                assert_eq!(got, f(v(i, j, k)), "{name} at {i} {j} {k}");
            }
        }
    }
}

#[test]
fn nrrd() {
    let im = image("raw.nrrd");
    assert_eq!((im.dims(), im.spacing, im.origin), ([4, 3, 2], [0.5, 1.0, 2.0], [1.0, 2.0, 3.0]));
    assert!(matches!(im.point_data.get("NRRDImage").unwrap().data, ArrayData::I16(_)));
    check_values(&im, "NRRDImage", |x| x);
    // Space directions: the file's i axis along +y, j along -x; laid out along +x +y +z (x is the
    // file's j reversed, y its i), big endian gzip floats.
    let g = image("gzip.nrrd");
    assert_eq!((g.dims(), g.spacing, g.origin), ([3, 4, 2], [1.0, 0.5, 2.0], [8.0, 20.0, 30.0]));
    assert_eq!(g.direction, [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0]);
    let a = g.point_data.get("NRRDImage").unwrap();
    for k in 0..2 {
        for y in 0..4 {
            for x in 0..3 {
                assert_eq!(a.get(x + 3 * (y + 4 * k), 0), v(y, 2 - x, k));
            }
        }
    }
    // A detached header with a vector on the first axis.
    let vec = image("vector.nhdr");
    assert_eq!(vec.dims(), [4, 3, 2]);
    let a = vec.point_data.get("NRRDImage").unwrap();
    assert_eq!(a.ncomp, 3);
    assert_eq!((a.get(23, 0), a.get(23, 1), a.get(23, 2)), (v(3, 2, 1), -v(3, 2, 1), 1.0));
}

#[test]
fn metaimage_and_mrc() {
    let m = image("local.mha");
    assert_eq!((m.dims(), m.spacing, m.origin), ([4, 3, 2], [0.25, 0.5, 1.0], [-1.0, -2.0, -3.0]));
    check_values(&m, "MetaImage", |x| x);
    let z = image("zipped.mhd");
    assert!(matches!(z.point_data.get("MetaImage").unwrap().data, ArrayData::F32(_)));
    check_values(&z, "MetaImage", |x| x);
    let f = image("float.mrc");
    assert_eq!((f.dims(), f.spacing, f.origin), ([4, 3, 2], [1.5, 1.5, 1.5], [3.0, 4.0, 5.0]));
    check_values(&f, "Scalars", |x| x);
    // Columns along y and rows along x: put back in x-fastest order.
    let p = image("permuted.mrc");
    assert_eq!(p.dims(), [4, 3, 2]);
    check_values(&p, "Scalars", |x| x);
}

#[test]
fn dicom_series() {
    // The folder (or any of its slices) opens the series; the file of another series is left out.
    for name in ["dicom", "dicom/slice1.dcm"] {
        let d = image(name);
        assert_eq!((d.dims(), d.spacing), ([4, 3, 2], [0.5, 0.75, 2.5]), "{name}");
        // Rows along +y from the first pixel's position.
        assert_eq!(d.origin, [-5.0, -6.0, 7.0]);
        assert_eq!(d.direction, [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0]);
        check_values(&d, "DICOMImage", |x| 2.0 * x - 1024.0);
    }
}

#[test]
fn ome_tiff() {
    let Loaded::Series(s) = read_file(root().join("cells.ome.tif")).unwrap() else { panic!("two time steps") };
    assert_eq!(s.times, vec![0.0, 2.0]);
    for (t, step) in s.steps.iter().enumerate() {
        let TimeStepRef::Loaded(d) = step else { panic!() };
        let DataSet::ImageData(im) = &**d else { panic!() };
        assert_eq!((im.dims(), im.spacing), ([4, 3, 3], [0.2, 0.3, 1.5]));
        for (c, name) in ["DAPI", "GFP"].iter().enumerate() {
            check_values(im, name, |x| x + 1000.0 * c as f64 + 5000.0 * t as f64);
        }
    }
}

fn nifti_value(ds: &aurafx_data::DataSet, p: [f64; 3], c: usize) -> f64 {
    let aurafx_data::DataSet::ImageData(im) = ds else { panic!("not image data") };
    let a = im.point_data.get("NIFTI").unwrap();
    let ijk: Vec<usize> = (0..3).map(|k| ((p[k] - im.origin[k]) / im.spacing[k]).round() as usize).collect();
    let d = im.dims();
    a.get(ijk[0] + d[0] * (ijk[1] + d[1] * ijk[2]), c)
}

#[test]
fn nifti_sform_qform_scaling_vectors_and_pairs() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../testdata/volumes");
    // sform: x = 30 - 2 i, y = 1.5 k, z = 3 j + 4; values 2 (i + 10 j + 100 k) - 5.
    let h = aurafx_io::volumes::read_nifti(&dir.join("head.nii.gz")).unwrap();
    let aurafx_data::DataSet::ImageData(im) = &h else { panic!() };
    assert_eq!(im.dims(), [6, 4, 5]);
    assert_eq!((im.origin, im.spacing), ([20.0, 0.0, 4.0], [2.0, 1.5, 3.0]));
    for (i, j, k) in [(0, 0, 0), (5, 4, 3), (2, 1, 3)] {
        let p = [30.0 - 2.0 * i as f64, 1.5 * k as f64, 3.0 * j as f64 + 4.0];
        assert_eq!(nifti_value(&h, p, 0), 2.0 * (i + 10 * j + 100 * k) as f64 - 5.0, "{i} {j} {k}");
    }
    // NIfTI-2 with three components from the 5th dimension.
    let v = aurafx_io::volumes::read_nifti(&dir.join("vectors.nii")).unwrap();
    let aurafx_data::DataSet::ImageData(vm) = &v else { panic!() };
    assert_eq!((vm.dims(), vm.spacing), ([4, 3, 2], [0.5, 0.5, 1.0]));
    assert_eq!(vm.point_data.get("NIFTI").unwrap().ncomp, 3);
    assert_eq!(nifti_value(&v, [1.5, 1.0, 1.0], 2), (2 + 3 + 20 + 100) as f64);
    // A big-endian .hdr/.img pair with a qform turning 90 degrees about z, offset (10, 0, 0).
    let p = aurafx_io::read_file(dir.join("pair.hdr")).unwrap();
    let aurafx_io::Loaded::Static(p) = p else { panic!() };
    let aurafx_data::DataSet::ImageData(pm) = &*p else { panic!() };
    assert_eq!((pm.dims(), pm.origin), ([2, 3, 2], [8.0, 0.0, 0.0]));
    for (i, j, k) in [(0, 0, 0), (2, 1, 1), (1, 0, 1)] {
        let w = [10.0 - 2.0 * j as f64, 2.0 * i as f64, 2.0 * k as f64];
        assert_eq!(nifti_value(&p, w, 0), (1 + i + 3 * j + 6 * k) as f64, "{i} {j} {k}");
    }
}

#[test]
fn pts_scans_with_intensity_and_colours() {
    let f = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../testdata/lidar/scan.pts");
    let aurafx_io::Loaded::Static(d) = aurafx_io::read_file(&f).unwrap() else { panic!() };
    assert_eq!(d.n_points(), 5);
    let pd = d.point_data().unwrap();
    assert_eq!(pd.get("Intensity").unwrap().get(4, 0), 50.0);
    assert_eq!(pd.get("Colors").unwrap().get(3, 2), 30.0);
    assert_eq!((0..5).map(|i| pd.get("ScanId").unwrap().get(i, 0)).collect::<Vec<_>>(), vec![0.0, 0.0, 0.0, 1.0, 1.0]);
}
