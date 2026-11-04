//! Filters that combine several inputs: Append Datasets, Append Geometry, Append Attributes
//! and Resample With Dataset. Behaviour follows ParaView 6.1.1 (checked with pvpython).

use crate::probe::probe_points;
use anyhow::{anyhow, Result};
use aurafx_data::*;

/// All non-composite pieces of `inputs`, in order (multi-blocks are flattened).
fn pieces<'a>(inputs: &[&'a DataSet]) -> Vec<&'a DataSet> {
    let mut out = Vec::new();
    for d in inputs {
        match d {
            DataSet::MultiBlock(mb) => out.extend(mb.leaves().into_iter().map(|(_, l)| l)),
            other => out.push(*other),
        }
    }
    out
}

/// Append the arrays that every piece has (same name and component count), as ParaView
/// does. Values are converted to f64 when the pieces store different scalar types.
fn append_attrs(parts: &[&Attributes]) -> Attributes {
    let Some(first) = parts.first() else { return Attributes::new() };
    let mut out = Attributes::new();
    for a in first.iter() {
        let all: Option<Vec<&Array>> = parts.iter().map(|p| p.get(&a.name).filter(|b| b.ncomp == a.ncomp)).collect();
        let Some(all) = all else { continue };
        let mut data = a.data.clone();
        let mut ok = true;
        for b in &all[1..] {
            if data.append(&b.data).is_err() {
                if a.scalar_type() == ScalarType::String || b.scalar_type() == ScalarType::String {
                    ok = false;
                    break;
                }
                let mut v = data.to_f64_vec();
                v.extend(b.data.to_f64_vec());
                data = ArrayData::F64(v);
            }
        }
        if !ok {
            continue;
        }
        let mut arr = Array::new(a.name.clone(), a.ncomp, data);
        arr.component_names = a.component_names.clone();
        out.insert(arr);
    }
    for (kind, name) in &first.active {
        if parts.iter().all(|p| p.active.get(kind) == Some(name)) {
            out.set_active(*kind, name);
        }
    }
    out
}

/// Append Datasets: one unstructured grid with every cell of every input.
pub fn append_datasets(inputs: &[&DataSet]) -> Result<UnstructuredGrid> {
    let grids: Vec<UnstructuredGrid> = pieces(inputs).into_iter().filter(|d| d.n_points() > 0).filter_map(|d| d.to_unstructured()).collect();
    if grids.is_empty() {
        return Ok(UnstructuredGrid::default());
    }
    let mut out = UnstructuredGrid { points: grids[0].points.clone(), cells: grids[0].cells.clone(), ..Default::default() };
    for g in &grids[1..] {
        let off = out.points.len() as u32;
        out.points.append(&g.points);
        out.cells.append(&g.cells, off);
    }
    out.point_data = append_attrs(&grids.iter().map(|g| &g.point_data).collect::<Vec<_>>());
    out.cell_data = append_attrs(&grids.iter().map(|g| &g.cell_data).collect::<Vec<_>>());
    out.field_data = grids[0].field_data.clone();
    Ok(out)
}

/// Append Geometry: polygonal inputs combined into one polygonal mesh. Non-polygonal
/// inputs are rejected so that the result never silently drops cells.
pub fn append_geometry(inputs: &[&DataSet]) -> Result<PolyData> {
    let polys: Vec<&PolyData> = pieces(inputs)
        .into_iter()
        .filter(|d| d.n_points() > 0)
        .map(|d| match d {
            DataSet::PolyData(p) => Ok(p),
            other => Err(anyhow!("Append Geometry needs polygonal inputs; got {} (use Append Datasets or Extract Surface first)", other.type_name())),
        })
        .collect::<Result<_>>()?;
    let Some(first) = polys.first() else { return Ok(PolyData::default()) };
    let mut out = PolyData { points: first.points.clone(), ..Default::default() };
    for p in &polys[1..] {
        out.points.append(&p.points);
    }
    // Cells keep VTK's verts, lines, polys, strips order, so cell data is regrouped the same way.
    let mut order: Vec<(usize, usize)> = Vec::new(); // (input, cell index within that input)
    let mut off = 0u32;
    let mut offsets = Vec::new();
    for p in &polys {
        offsets.push(off);
        off += p.points.len() as u32;
    }
    for group in 0..4 {
        for (k, p) in polys.iter().enumerate() {
            let (cells, base) = match group {
                0 => (&p.verts, 0),
                1 => (&p.lines, p.verts.len()),
                2 => (&p.polys, p.verts.len() + p.lines.len()),
                _ => (&p.strips, p.verts.len() + p.lines.len() + p.polys.len()),
            };
            let dst = match group {
                0 => &mut out.verts,
                1 => &mut out.lines,
                2 => &mut out.polys,
                _ => &mut out.strips,
            };
            dst.append(cells, offsets[k]);
            order.extend((0..cells.len()).map(|c| (k, base + c)));
        }
    }
    out.point_data = append_attrs(&polys.iter().map(|p| &p.point_data).collect::<Vec<_>>());
    // Cell data: append per input, then gather into the regrouped order.
    let cd = append_attrs(&polys.iter().map(|p| &p.cell_data).collect::<Vec<_>>());
    if !cd.is_empty() {
        let mut start = Vec::new();
        let mut s = 0;
        for p in &polys {
            start.push(s);
            s += p.n_cells();
        }
        let idx: Vec<usize> = order.iter().map(|&(k, c)| start[k] + c).collect();
        out.cell_data = cd.gather(&idx);
    }
    out.field_data = first.field_data.clone();
    Ok(out)
}

/// Append Attributes: the first input's mesh with the arrays of every input. Inputs must have
/// the same number of points (point arrays) or cells (cell arrays); a repeated name from input
/// `k` becomes `name_input_k`, as in ParaView.
pub fn append_attributes(inputs: &[&DataSet]) -> Result<DataSet> {
    let Some(first) = inputs.first() else { return Err(anyhow!("Append Attributes needs an input")) };
    if let DataSet::MultiBlock(mb) = first {
        // Block-wise: block i takes arrays from block i of every multi-block input.
        let leaves: Vec<Vec<&DataSet>> = inputs
            .iter()
            .map(|d| match d {
                DataSet::MultiBlock(m) => m.leaves().into_iter().map(|(_, l)| l).collect(),
                other => vec![*other],
            })
            .collect();
        let mut out = mb.clone();
        let mut k = 0;
        fn walk(mb: &mut MultiBlock, f: &mut dyn FnMut(&mut DataSet) -> Result<()>) -> Result<()> {
            for (_, b) in mb.blocks.iter_mut() {
                match b {
                    Some(DataSet::MultiBlock(inner)) => walk(inner, f)?,
                    Some(d) => f(d)?,
                    None => {}
                }
            }
            Ok(())
        }
        walk(&mut out, &mut |d| {
            let mine: Vec<&DataSet> = leaves.iter().filter_map(|l| l.get(k)).copied().collect();
            k += 1;
            *d = append_attributes(&mine)?;
            Ok(())
        })?;
        return Ok(DataSet::MultiBlock(out));
    }
    let mut out = (*first).clone();
    let (np, nc) = (first.n_points(), first.n_cells());
    for (k, d) in inputs.iter().enumerate().skip(1) {
        let add = |dst: Option<&mut Attributes>, src: Option<&Attributes>, n: usize, n_src: usize| {
            let (Some(dst), Some(src)) = (dst, src) else { return };
            if n_src != n {
                return;
            }
            for a in src.iter() {
                let mut a = a.clone();
                if dst.get(&a.name).is_some() {
                    a.name = format!("{}_input_{k}", a.name);
                }
                dst.insert(a);
            }
        };
        if d.n_points() != np && d.n_cells() != nc {
            return Err(anyhow!(
                "Append Attributes: input {} has {} points and {} cells but the first input has {np} points and {nc} cells",
                k + 1,
                d.n_points(),
                d.n_cells()
            ));
        }
        add(out.point_data_mut(), d.point_data(), np, d.n_points());
        add(out.cell_data_mut(), d.cell_data(), nc, d.n_cells());
    }
    Ok(out)
}

/// Resample With Dataset: sample the arrays of `source` at the points of `destination`.
/// Output is the destination mesh carrying the sampled arrays (source cell arrays become point
/// arrays) and `vtkValidPointMask`; points outside the source get 0, as in ParaView.
/// `pass_point`/`pass_cell` keep the destination's own arrays.
pub fn resample_with_dataset(source: &DataSet, destination: &DataSet, pass_point: bool, pass_cell: bool) -> Result<DataSet> {
    if let DataSet::MultiBlock(mb) = destination {
        let mut out = MultiBlock { field_data: mb.field_data.clone(), ..Default::default() };
        for (name, b) in &mb.blocks {
            let r = match b {
                Some(d) => Some(resample_with_dataset(source, d, pass_point, pass_cell)?),
                None => None,
            };
            out.blocks.push((name.clone(), r));
        }
        return Ok(DataSet::MultiBlock(out));
    }
    let pts = destination.points().ok_or_else(|| anyhow!("Resample With Dataset: the destination has no points"))?;
    let pts: Vec<[f64; 3]> = (0..pts.len()).map(|i| pts.get(i)).collect();
    let mut probed = probe_points(source, &pts)?;
    for a in probed.arrays.values_mut() {
        if let ArrayData::F64(v) = &mut a.data {
            for x in v.iter_mut().filter(|x| x.is_nan()) {
                *x = 0.0;
            }
        }
    }
    if let Some(m) = probed.remove("vtkValidPointMask") {
        let mask: Vec<u8> = m.data.to_f64_vec().iter().map(|&x| (x > 0.5) as u8).collect();
        probed.insert(Array::from_vec("vtkValidPointMask", 1, mask));
    }
    let mut out = destination.clone();
    if let Some(pd) = out.point_data_mut() {
        let mut keep = if pass_point { std::mem::take(pd) } else { Attributes::new() };
        for a in probed.iter() {
            keep.insert(a.clone());
        }
        for (k, n) in &probed.active {
            keep.set_active(*k, n);
        }
        *pd = keep;
    }
    if !pass_cell {
        if let Some(cd) = out.cell_data_mut() {
            *cd = Attributes::new();
        }
    }
    Ok(out)
}
