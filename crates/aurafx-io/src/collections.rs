//! Files that point to other files: parallel VTK pieces (`.pvtu`, `.pvtp`, `.pvti`, `.pvts`,
//! `.pvtr`: one header and a folder of piece files), `.pvd` time-step collections, and image
//! stacks of volumes (one PNG per slice, or one multi-page TIFF).

use anyhow::{anyhow, bail, Context, Result};
use aurafx_data::*;
use std::path::{Path, PathBuf};

/// The VTK XML extension for a kind of data.
pub fn xml_ext(ds: &DataSet) -> &'static str {
    match ds {
        DataSet::PolyData(_) => "vtp",
        DataSet::ImageData(_) => "vti",
        DataSet::RectilinearGrid(_) => "vtr",
        DataSet::StructuredGrid(_) => "vts",
        DataSet::MultiBlock(_) => "vtm",
        _ => "vtu",
    }
}

fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('"', "&quot;").replace('<', "&lt;").replace('>', "&gt;")
}

/// A folder named after `path` (without its extension) beside it, for the files it points to.
fn folder_for(path: &Path) -> Result<(PathBuf, String)> {
    let stem = path.file_stem().and_then(|s| s.to_str()).ok_or_else(|| anyhow!("{}: no file name", path.display()))?.to_string();
    let dir = path.with_file_name(&stem);
    std::fs::create_dir_all(&dir).with_context(|| format!("creating {}", dir.display()))?;
    Ok((dir, stem))
}

/// Write every step to `<stem>/<stem>_NNNN.<ext>` and a `.pvd` listing them with their times.
pub fn write_pvd_steps(steps: &[(f64, &DataSet)], path: &Path) -> Result<()> {
    if steps.is_empty() {
        bail!("no time steps to write");
    }
    let (dir, stem) = folder_for(path)?;
    let mut xml = String::from("<?xml version=\"1.0\"?>\n<VTKFile type=\"Collection\" version=\"1.0\" byte_order=\"LittleEndian\">\n  <Collection>\n");
    for (k, (t, ds)) in steps.iter().enumerate() {
        let name = format!("{stem}_{k:04}.{}", xml_ext(ds));
        crate::writers::write_file(ds, &dir.join(&name))?;
        xml += &format!("    <DataSet timestep=\"{t}\" group=\"\" part=\"0\" file=\"{}/{}\"/>\n", xml_escape(&stem), xml_escape(&name));
    }
    xml += "  </Collection>\n</VTKFile>\n";
    std::fs::write(path, xml).with_context(|| format!("writing {}", path.display()))
}

/// The pieces of `ds` for a parallel file: the blocks of a multi-block (all of one kind), or
/// `n` pieces cut from one dataset.
fn pieces_of(ds: &DataSet, n: usize) -> Result<Vec<DataSet>> {
    match ds {
        DataSet::MultiBlock(mb) => {
            let leaves: Vec<DataSet> = mb.leaves().into_iter().map(|l| l.1.clone()).collect();
            if leaves.is_empty() {
                bail!("no blocks to write");
            }
            let kind = std::mem::discriminant(&leaves[0]);
            if leaves.iter().any(|l| std::mem::discriminant(l) != kind) {
                bail!("the blocks are of different kinds; a parallel file needs pieces of one kind");
            }
            Ok(leaves)
        }
        d => Ok((0..n.max(1)).map(|k| aurafx_data::pieces::split(d, k, n.max(1))).collect()),
    }
}

fn p_arrays(tag: &str, attrs: &Attributes) -> String {
    let mut s = format!("    <{tag}>\n");
    for a in attrs.iter() {
        s += &format!(
            "      <PDataArray type=\"{}\" Name=\"{}\" NumberOfComponents=\"{}\"/>\n",
            crate::writers::xml_type_name(&a.data),
            xml_escape(&a.name),
            a.ncomp
        );
    }
    s + &format!("    </{tag}>\n")
}

/// A parallel VTK file (`.pvtu`, `.pvtp`, `.pvti`, `.pvts`, `.pvtr`) with its pieces in a folder
/// named after it: the blocks of a multi-block, or `pieces` pieces cut from the data. Returns the
/// number of pieces.
pub fn write_parallel(ds: &DataSet, path: &Path, pieces: usize) -> Result<usize> {
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase();
    let parts = pieces_of(ds, pieces)?;
    let first = &parts[0];
    let want = match first {
        DataSet::UnstructuredGrid(_) => "pvtu",
        DataSet::PolyData(_) => "pvtp",
        DataSet::ImageData(_) => "pvti",
        DataSet::StructuredGrid(_) => "pvts",
        DataSet::RectilinearGrid(_) => "pvtr",
        other => bail!("{} cannot be written as parallel pieces", other.type_name()),
    };
    if ext != want {
        bail!("this data is written as .{want} (not .{ext})");
    }
    let (dir, stem) = folder_for(path)?;
    let inner = &want[1..];
    let mut sources = Vec::new();
    for (k, p) in parts.iter().enumerate() {
        let name = format!("{stem}_{k}.{inner}");
        crate::writers::write_xml(p, &dir.join(&name))?;
        let extent = match p {
            DataSet::ImageData(i) => Some(i.extent),
            DataSet::RectilinearGrid(r) => Some(r.extent),
            DataSet::StructuredGrid(s) => Some(s.extent),
            _ => None,
        };
        sources.push((format!("{stem}/{name}"), extent));
    }
    let pd = first.point_data().cloned().unwrap_or_default();
    let cd = first.cell_data().cloned().unwrap_or_default();
    let ty = match first {
        DataSet::UnstructuredGrid(_) => "PUnstructuredGrid",
        DataSet::PolyData(_) => "PPolyData",
        DataSet::ImageData(_) => "PImageData",
        DataSet::StructuredGrid(_) => "PStructuredGrid",
        _ => "PRectilinearGrid",
    };
    let ext_str = |e: [i64; 6]| e.iter().map(|v| v.to_string()).collect::<Vec<_>>().join(" ");
    let whole = {
        let mut w = [i64::MAX, i64::MIN, i64::MAX, i64::MIN, i64::MAX, i64::MIN];
        for (_, e) in &sources {
            if let Some(e) = e {
                for a in 0..3 {
                    w[2 * a] = w[2 * a].min(e[2 * a]);
                    w[2 * a + 1] = w[2 * a + 1].max(e[2 * a + 1]);
                }
            }
        }
        w
    };
    let mut x = format!("<?xml version=\"1.0\"?>\n<VTKFile type=\"{ty}\" version=\"1.0\" byte_order=\"LittleEndian\" header_type=\"UInt64\">\n");
    let ghost = if first.cell_data().is_some_and(|c| c.get("vtkGhostType").is_some()) { 1 } else { 0 };
    match first {
        DataSet::ImageData(im) => {
            let d = im.direction.iter().map(|v| v.to_string()).collect::<Vec<_>>().join(" ");
            x += &format!(
                "  <PImageData WholeExtent=\"{}\" GhostLevel=\"{ghost}\" Origin=\"{} {} {}\" Spacing=\"{} {} {}\" Direction=\"{d}\">\n",
                ext_str(whole),
                im.origin[0],
                im.origin[1],
                im.origin[2],
                im.spacing[0],
                im.spacing[1],
                im.spacing[2]
            );
        }
        DataSet::StructuredGrid(_) | DataSet::RectilinearGrid(_) => x += &format!("  <{ty} WholeExtent=\"{}\" GhostLevel=\"{ghost}\">\n", ext_str(whole)),
        _ => x += &format!("  <{ty} GhostLevel=\"{ghost}\">\n"),
    }
    x += &p_arrays("PPointData", &pd);
    x += &p_arrays("PCellData", &cd);
    match first {
        DataSet::UnstructuredGrid(u) => {
            x += &format!(
                "    <PPoints>\n      <PDataArray type=\"{}\" NumberOfComponents=\"3\"/>\n    </PPoints>\n",
                crate::writers::xml_type_name(&u.points.data)
            )
        }
        DataSet::PolyData(p) => {
            x += &format!(
                "    <PPoints>\n      <PDataArray type=\"{}\" NumberOfComponents=\"3\"/>\n    </PPoints>\n",
                crate::writers::xml_type_name(&p.points.data)
            )
        }
        DataSet::StructuredGrid(s) => {
            x += &format!(
                "    <PPoints>\n      <PDataArray type=\"{}\" NumberOfComponents=\"3\"/>\n    </PPoints>\n",
                crate::writers::xml_type_name(&s.points.data)
            )
        }
        DataSet::RectilinearGrid(_) => {
            x += "    <PCoordinates>\n";
            for _ in 0..3 {
                x += "      <PDataArray type=\"Float64\" NumberOfComponents=\"1\"/>\n";
            }
            x += "    </PCoordinates>\n";
        }
        _ => {}
    }
    for (src, e) in &sources {
        match e {
            Some(e) => x += &format!("    <Piece Extent=\"{}\" Source=\"{}\"/>\n", ext_str(*e), xml_escape(src)),
            None => x += &format!("    <Piece Source=\"{}\"/>\n", xml_escape(src)),
        }
    }
    x += &format!("  </{ty}>\n</VTKFile>\n");
    std::fs::write(path, x).with_context(|| format!("writing {}", path.display()))?;
    Ok(parts.len())
}

/// The array an image stack shows: the active (else first) point array, else cell array.
fn stack_array(im: &ImageData) -> Result<(&Array, [usize; 3])> {
    if let Some(a) = im.point_data.scalars().or_else(|| im.point_data.iter().find(|x| x.scalar_type() != ScalarType::String)) {
        return Ok((a, im.dims()));
    }
    let a = im
        .cell_data
        .scalars()
        .or_else(|| im.cell_data.iter().find(|x| x.scalar_type() != ScalarType::String))
        .ok_or_else(|| anyhow!("the image has no array to write"))?;
    Ok((a, im.dims().map(|d| (d - 1).max(1))))
}

/// The slices of a volume as pictures: one PNG per slice (`name_0000.png`, ... ; a single slice
/// is written to `path` itself) or one multi-page TIFF. 8-bit arrays with 1, 3 or 4 values and
/// 16-bit arrays with one value are written as they are; TIFF also keeps 32-bit floats; other
/// values are scaled to 16 bits between their minimum and maximum. Returns the number of slices.
pub fn write_image_stack(ds: &DataSet, path: &Path) -> Result<usize> {
    let DataSet::ImageData(im) = ds else { bail!("an image stack is written from image data (a uniform grid); resample first") };
    let (arr, dims) = stack_array(im)?;
    let [w, h, nz] = dims;
    let nc = arr.ncomp;
    let plane = w * h * nc;
    // Rows of a picture run top to bottom: flip each slice.
    let slice_rows = |k: usize, vals: &dyn Fn(usize) -> f64| -> Vec<f64> {
        let mut out = Vec::with_capacity(plane);
        for y in (0..h).rev() {
            for x in 0..w {
                for c in 0..nc {
                    out.push(vals(((k * h + y) * w + x) * nc + c));
                }
            }
        }
        out
    };
    let flat = arr.data.to_f64_vec();
    let get = |i: usize| flat[i];
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase();
    let (lo, hi) = flat.iter().filter(|v| v.is_finite()).fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), &v| (a.min(v), b.max(v)));
    let to16 = |v: f64| if hi > lo { (((v - lo) / (hi - lo)) * 65535.0).round().clamp(0.0, 65535.0) as u16 } else { 0 };
    let is8 = matches!(arr.data, ArrayData::U8(_)) && matches!(nc, 1 | 3 | 4);
    let is16 = matches!(arr.data, ArrayData::U16(_)) && nc == 1;
    if ext == "tif" || ext == "tiff" {
        use tiff::encoder::{colortype, TiffEncoder};
        let f = std::fs::File::create(path).with_context(|| format!("writing {}", path.display()))?;
        let mut enc = TiffEncoder::new(std::io::BufWriter::new(f))?;
        for k in 0..nz {
            let v = slice_rows(k, &get);
            let (w32, h32) = (w as u32, h as u32);
            match (&arr.data, nc) {
                (ArrayData::U8(_), 1) => enc.write_image::<colortype::Gray8>(w32, h32, &v.iter().map(|x| *x as u8).collect::<Vec<_>>())?,
                (ArrayData::U8(_), 3) => enc.write_image::<colortype::RGB8>(w32, h32, &v.iter().map(|x| *x as u8).collect::<Vec<_>>())?,
                (ArrayData::U8(_), 4) => enc.write_image::<colortype::RGBA8>(w32, h32, &v.iter().map(|x| *x as u8).collect::<Vec<_>>())?,
                (ArrayData::U16(_), 1) => enc.write_image::<colortype::Gray16>(w32, h32, &v.iter().map(|x| *x as u16).collect::<Vec<_>>())?,
                (ArrayData::F32(_) | ArrayData::F64(_), 1) => {
                    enc.write_image::<colortype::Gray32Float>(w32, h32, &v.iter().map(|x| *x as f32).collect::<Vec<_>>())?
                }
                (_, 1) => enc.write_image::<colortype::Gray16>(w32, h32, &v.iter().map(|x| to16(*x)).collect::<Vec<_>>())?,
                _ => bail!("TIFF stacks take arrays with one value (or 8-bit RGB/RGBA)"),
            }
        }
        return Ok(nz);
    }
    if ext != "png" {
        bail!("image stacks are written as .png or .tif");
    }
    let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("slice").to_string();
    for k in 0..nz {
        let v = slice_rows(k, &get);
        let p = if nz == 1 { path.to_path_buf() } else { path.with_file_name(format!("{stem}_{k:04}.png")) };
        let (w32, h32) = (w as u32, h as u32);
        let r = if is8 {
            let bytes: Vec<u8> = v.iter().map(|x| *x as u8).collect();
            let ct = match nc {
                1 => image::ColorType::L8,
                3 => image::ColorType::Rgb8,
                _ => image::ColorType::Rgba8,
            };
            image::save_buffer(&p, &bytes, w32, h32, ct)
        } else {
            let px: Vec<u16> = if is16 {
                v.iter().map(|x| *x as u16).collect()
            } else if nc == 1 {
                v.iter().map(|x| to16(*x)).collect()
            } else {
                bail!("PNG stacks take arrays with one value (or 8-bit RGB/RGBA)")
            };
            let buf: image::ImageBuffer<image::Luma<u16>, Vec<u16>> = image::ImageBuffer::from_raw(w32, h32, px).ok_or_else(|| anyhow!("image size"))?;
            buf.save(&p)
        };
        r.with_context(|| format!("writing {}", p.display()))?;
    }
    Ok(nz)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("afx-coll-{}-{name}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn cube_image(n: usize) -> ImageData {
        let mut im =
            ImageData { extent: [0, n as i64 - 1, 0, n as i64 - 1, 0, n as i64 - 1], origin: [1.0, 2.0, 3.0], spacing: [0.5, 0.5, 0.5], ..Default::default() };
        im.point_data.insert_active(AttributeKind::Scalars, Array::new("v", 1, ArrayData::U16((0..n * n * n).map(|i| i as u16).collect())));
        im
    }

    #[test]
    fn parallel_pieces_read_back() {
        let d = tmp("par");
        let im = DataSet::ImageData(cube_image(6));
        let n = write_parallel(&im, &d.join("cube.pvti"), 3).unwrap();
        assert_eq!(n, 3);
        let back = crate::read_file(&d.join("cube.pvti")).unwrap();
        let crate::Loaded::Static(b) = back else { panic!() };
        assert_eq!(b.n_points(), 216);
        let u = DataSet::UnstructuredGrid(im.to_unstructured().unwrap());
        write_parallel(&u, &d.join("cube.pvtu"), 4).unwrap();
        let crate::Loaded::Static(b) = crate::read_file(&d.join("cube.pvtu")).unwrap() else { panic!() };
        assert_eq!(b.n_cells(), 125);
        assert!(write_parallel(&u, &d.join("cube.pvti"), 2).is_err(), "wrong extension");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn pvd_and_stacks() {
        let d = tmp("pvd");
        let a = DataSet::ImageData(cube_image(3));
        let b = DataSet::ImageData(cube_image(3));
        write_pvd_steps(&[(0.0, &a), (0.5, &b)], &d.join("run.pvd")).unwrap();
        let crate::Loaded::Series(s) = crate::read_file(&d.join("run.pvd")).unwrap() else { panic!() };
        assert_eq!(s.times, vec![0.0, 0.5]);
        // A 16-bit volume as PNG slices and as one TIFF, read back as volumes.
        assert_eq!(write_image_stack(&a, &d.join("slice.png")).unwrap(), 3);
        let crate::Loaded::Static(st) = crate::read_file(&d.join("slice_*.png")).unwrap() else { panic!() };
        assert_eq!(st.n_points(), 27);
        let v = st.point_data().unwrap().scalars().unwrap();
        assert_eq!(v.get(26, 0), 26.0);
        write_image_stack(&a, &d.join("vol.tif")).unwrap();
        let crate::Loaded::Static(t) = crate::read_file(&d.join("vol.tif")).unwrap() else { panic!() };
        assert_eq!(t.point_data().unwrap().scalars().unwrap().get(13, 0), 13.0);
        let _ = std::fs::remove_dir_all(&d);
    }
}
