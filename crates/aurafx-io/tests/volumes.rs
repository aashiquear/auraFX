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
