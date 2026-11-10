//! Reading a share of a file (for parallel and streamed reading): VTK XML grids are read slab
//! by slab and unstructured grids range of cells by range of cells, from the file's bytes for
//! that share only; the shares put together are the whole file.
use aurafx_data::DataSet;
use aurafx_io::Loaded;
use std::path::PathBuf;

fn td(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../testdata").join(name)
}

fn read(path: &PathBuf, piece: Option<(usize, usize)>) -> DataSet {
    let Loaded::Static(d) = aurafx_io::read_file_piece(path, piece).unwrap() else { panic!("static") };
    (*d).clone()
}

/// Every array of `a` equals the same array of `b`, value for value.
fn same_arrays(a: &DataSet, b: &DataSet, what: &str) {
    for (x, y) in [(a.point_data(), b.point_data()), (a.cell_data(), b.cell_data())] {
        let (Some(x), Some(y)) = (x, y) else { continue };
        for arr in y.iter() {
            let got = x.get(&arr.name).unwrap_or_else(|| panic!("{what}: no {}", arr.name));
            assert_eq!(got.ncomp, arr.ncomp, "{what}: {}", arr.name);
            assert_eq!(got.data.to_f64_vec(), arr.data.to_f64_vec(), "{what}: values of {}", arr.name);
        }
    }
}

#[test]
fn grid_slabs_put_together_are_the_file() {
    for f in ["wavelet.vti", "rect.vtr", "struct.vts"] {
        let path = td(f);
        let whole = read(&path, None);
        for n in [2, 3, 7] {
            let parts: Vec<DataSet> = (0..n).map(|k| read(&path, Some((k, n)))).collect();
            let cells: usize = parts.iter().map(|p| p.n_cells()).sum();
            assert_eq!(cells, whole.n_cells(), "{f} in {n}: every cell once");
            let merged = aurafx_data::pieces::merge(parts);
            assert_eq!((merged.n_points(), merged.bounds()), (whole.n_points(), whole.bounds()), "{f} in {n}");
            same_arrays(&merged, &whole, &format!("{f} in {n}"));
        }
    }
}

#[test]
fn unstructured_pieces_equal_splitting_the_whole() {
    // Compressed (zlib blocks): only the blocks of each range are decompressed.
    let path = td("wavelet_tets.vtu");
    let whole = read(&path, None);
    for n in [2, 5] {
        for k in 0..n {
            let got = read(&path, Some((k, n)));
            let want = aurafx_data::pieces::split(&whole, k, n);
            let (DataSet::UnstructuredGrid(g), DataSet::UnstructuredGrid(w)) = (&got, &want) else { panic!("unstructured") };
            assert_eq!(g.cells.connectivity, w.cells.connectivity, "piece {k} of {n}: connectivity");
            assert_eq!(g.cells.types, w.cells.types, "piece {k} of {n}: types");
            assert_eq!(g.points.len(), w.points.len(), "piece {k} of {n}: points");
            assert!((0..g.points.len()).all(|i| g.points.get(i) == w.points.get(i)), "piece {k} of {n}: coordinates");
            same_arrays(&got, &want, &format!("piece {k} of {n}"));
        }
    }
}

/// Memory for one piece of a big file (set AFX_BIG to a large .vti; run with --ignored).
#[test]
#[ignore]
fn one_piece_of_a_big_file() {
    let Ok(p) = std::env::var("AFX_BIG") else { return };
    let hwm = || std::fs::read_to_string("/proc/self/status").unwrap().lines().find(|l| l.starts_with("VmHWM")).unwrap().to_string();
    let t = std::time::Instant::now();
    let d = read(&PathBuf::from(&p), Some((3, 16)));
    println!("piece: {} points, {:.1} s, {}", d.n_points(), t.elapsed().as_secs_f64(), hwm());
    let opts = aurafx_io::reader_options(&p);
    println!("reader options: {} ({})", opts.len(), hwm());
}
