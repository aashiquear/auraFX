//! Reading a share of a file (for parallel and streamed reading): VTK XML grids are read slab
//! by slab and unstructured grids range of cells by range of cells, from the file's bytes for
//! that share only; the shares put together are the whole file.
use aurafx_data::{Array, ArrayData, DataSet, ImageData, MultiBlock, Points, RectilinearGrid, StructuredGrid};
use aurafx_io::Loaded;
use std::path::PathBuf;

fn td(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../testdata").join(name)
}

fn read(path: &PathBuf, piece: Option<(usize, usize)>) -> DataSet {
    let Loaded::Static(d) = aurafx_io::read_file_piece(path, piece).unwrap() else { panic!("static") };
    (*d).clone()
}

/// Every array of `a` equals the same array of `b`, value for value (numbers and text), with
/// the same units.
fn same_arrays(a: &DataSet, b: &DataSet, what: &str) {
    for (x, y) in [(a.point_data(), b.point_data()), (a.cell_data(), b.cell_data())] {
        let (Some(x), Some(y)) = (x, y) else { continue };
        for arr in y.iter() {
            let got = x.get(&arr.name).unwrap_or_else(|| panic!("{what}: no {}", arr.name));
            assert_eq!(got.ncomp, arr.ncomp, "{what}: {}", arr.name);
            assert_eq!(got.scalar_type(), arr.scalar_type(), "{what}: type of {}", arr.name);
            match arr.as_slice::<String>() {
                Some(text) => assert_eq!(got.as_slice::<String>().unwrap(), text, "{what}: text of {}", arr.name),
                None => assert_eq!(got.data.to_f64_vec(), arr.data.to_f64_vec(), "{what}: values of {}", arr.name),
            }
            assert_eq!(got.units, arr.units, "{what}: units of {}", arr.name);
        }
    }
}

fn tmp(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("aurafx_pieces_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir.join(name)
}

/// An image with a point array (with units), a vector point array, a cell array and a text
/// array on its cells.
fn image(extent: [i64; 6]) -> ImageData {
    let mut img = ImageData { extent, origin: [1.0, 2.0, 3.0], spacing: [0.5, 1.0, 2.0], ..Default::default() };
    let (np, nc) = (img.n_points(), img.n_cells());
    img.point_data.insert(Array::scalars_f64("p", (0..np).map(|i| i as f64 * 0.5).collect()).with_units("K"));
    img.point_data.insert(Array::new("v", 3, ArrayData::F32((0..3 * np).map(|i| i as f32).collect())));
    img.cell_data.insert(Array::new("c", 1, ArrayData::I32((0..nc as i32).collect())));
    img.cell_data.insert(Array::new("label", 1, ArrayData::String((0..nc).map(|i| format!("cell {i}")).collect())));
    img
}

/// The same grid as an image, a rectilinear grid and a structured grid.
fn three_grids(extent: [i64; 6]) -> [(&'static str, DataSet); 3] {
    let img = image(extent);
    let axis = |a: usize| -> Vec<f64> { (extent[2 * a]..=extent[2 * a + 1]).map(|i| img.origin[a] + img.spacing[a] * (i * i) as f64).collect() };
    let rect = RectilinearGrid {
        extent,
        x: axis(0),
        y: axis(1),
        z: axis(2),
        point_data: img.point_data.clone(),
        cell_data: img.cell_data.clone(),
        ..Default::default()
    };
    let mut pts = Vec::new();
    for z in &rect.z {
        for y in &rect.y {
            for x in &rect.x {
                pts.extend([x + 0.1 * y, *y, z + 0.01 * x]);
            }
        }
    }
    let st =
        StructuredGrid { extent, points: Points::new_f64(pts), point_data: img.point_data.clone(), cell_data: img.cell_data.clone(), ..Default::default() };
    [("vti", DataSet::ImageData(img)), ("vtr", DataSet::RectilinearGrid(rect)), ("vts", DataSet::StructuredGrid(st))]
}

/// Files our own writer makes (raw appended data, so only each share's bytes are read): 3-D
/// and 2-D grids, more shares than cells along the axis, and a grid thin along its last axis.
#[test]
fn written_grids_in_shares_are_the_file() {
    for (tag, extent) in [("3d", [0, 6, 0, 5, 0, 9]), ("2d", [0, 6, 0, 5, 0, 0]), ("thin", [0, 9, 0, 9, 0, 1]), ("offset", [-3, 4, 2, 6, 5, 12])] {
        for (ext, ds) in three_grids(extent) {
            let path = tmp(&format!("{tag}.{ext}"));
            aurafx_io::writers::write_file(&ds, &path).unwrap();
            let whole = read(&path, None);
            assert_eq!(whole, ds, "{tag}.{ext}: written and read back");
            for n in [2, 3, 7, 16] {
                let what = format!("{tag}.{ext} in {n}");
                let parts: Vec<DataSet> = (0..n).map(|k| read(&path, Some((k, n)))).collect();
                assert_eq!(parts.iter().map(|p| p.n_cells()).sum::<usize>(), whole.n_cells(), "{what}: every cell once");
                let merged = aurafx_data::pieces::merge(parts);
                assert_eq!((merged.n_points(), merged.n_cells(), merged.bounds()), (whole.n_points(), whole.n_cells(), whole.bounds()), "{what}");
                same_arrays(&merged, &whole, &what);
                assert_eq!(merged, whole, "{what}: the same grid");
            }
        }
    }
}

/// A mesh our own writer makes, with cell data and a text array: each share equals that share
/// of the whole mesh.
#[test]
fn written_mesh_in_shares() {
    let mut u = DataSet::ImageData(image([0, 4, 0, 3, 0, 5])).to_unstructured().unwrap();
    let np = u.points.len();
    u.point_data.insert(Array::new("name", 1, ArrayData::String((0..np).map(|i| format!("point {i}")).collect())));
    let path = tmp("mesh.vtu");
    aurafx_io::writers::write_file(&DataSet::UnstructuredGrid(u), &path).unwrap();
    let whole = read(&path, None);
    for n in [2, 3, 7] {
        for k in 0..n {
            let what = format!("piece {k} of {n}");
            let (got, want) = (read(&path, Some((k, n))), aurafx_data::pieces::split(&whole, k, n));
            let (DataSet::UnstructuredGrid(g), DataSet::UnstructuredGrid(w)) = (&got, &want) else { panic!("unstructured") };
            assert_eq!(g.cells.connectivity, w.cells.connectivity, "{what}: connectivity");
            assert_eq!(g.cells.offsets, w.cells.offsets, "{what}: offsets");
            assert_eq!(g.cells.types, w.cells.types, "{what}: types");
            assert_eq!(g.points, w.points, "{what}: points");
            assert!(g.cell_data.get("c").is_some() && g.cell_data.get("label").is_some() && g.point_data.get("name").is_some(), "{what}: arrays");
            same_arrays(&got, &want, &what);
        }
    }
}

/// Multi-block files: the blocks are read whole and shared out (or split) afterwards, so the
/// shares together hold every cell of every block once, whatever kinds of files the blocks are.
#[test]
fn multiblock_shares_hold_every_cell_once() {
    let mut mb = MultiBlock::default();
    for (name, ds) in three_grids([0, 6, 0, 5, 0, 9]) {
        mb.blocks.push((name.to_string(), Some(ds)));
    }
    let surface = read(&td("sphere.vtp"), None);
    mb.blocks.push(("surface".into(), Some(surface)));
    mb.blocks.push(("mesh".into(), Some(DataSet::UnstructuredGrid(DataSet::ImageData(image([0, 3, 0, 3, 0, 3])).to_unstructured().unwrap()))));
    let made = tmp("blocks.vtm");
    aurafx_io::writers::write_file(&DataSet::MultiBlock(mb), &made).unwrap();
    for path in [made, td("group.vtm")] {
        let whole = read(&path, None);
        let DataSet::MultiBlock(w) = &whole else { panic!("multi-block") };
        for n in [2, 3, 4, 9] {
            let parts: Vec<DataSet> = (0..n).map(|k| read(&path, Some((k, n)))).collect();
            for (b, (name, block)) in w.blocks.iter().enumerate() {
                let cells: usize = parts
                    .iter()
                    .map(|p| {
                        let DataSet::MultiBlock(m) = p else { panic!("multi-block") };
                        m.blocks[b].1.as_ref().map(|d| d.n_cells()).unwrap_or(0)
                    })
                    .sum();
                assert_eq!(cells, block.as_ref().unwrap().n_cells(), "{} in {n}: block {name}", path.display());
            }
            assert_eq!(aurafx_data::pieces::merge(parts).n_cells(), whole.n_cells(), "{} in {n}", path.display());
        }
    }
}

/// Units are written to VTK XML files and read back, also share by share.
#[test]
fn units_are_written_and_read_back() {
    let mut img = image([0, 4, 0, 4, 0, 6]);
    let v = img.point_data.get("v").unwrap().clone().with_units("m/s & <more> \"quoted\"");
    img.point_data.insert(v);
    let ug = DataSet::UnstructuredGrid(DataSet::ImageData(img.clone()).to_unstructured().unwrap());
    for (name, ds) in [("units.vti", DataSet::ImageData(img)), ("units.vtu", ug)] {
        let path = tmp(name);
        aurafx_io::writers::write_file(&ds, &path).unwrap();
        for piece in [None, Some((1, 3))] {
            let back = read(&path, piece);
            let pd = back.point_data().unwrap();
            assert_eq!(pd.get("p").unwrap().units.as_deref(), Some("K"), "{name} {piece:?}");
            assert_eq!(pd.get("v").unwrap().units.as_deref(), Some("m/s & <more> \"quoted\""), "{name} {piece:?}");
            assert_eq!(back.cell_data().unwrap().get("c").unwrap().units, None, "{name} {piece:?}");
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
            assert!(parts.iter().filter(|p| p.n_cells() > 0).count() > 1, "{f} in {n}: more than one share has cells");
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
            assert_eq!(g.cells.offsets, w.cells.offsets, "piece {k} of {n}: offsets");
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
