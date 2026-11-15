//! Splitting a dataset into pieces for parallel (multi-process) work and putting pieces back
//! together.
//!
//! Structured data is cut into slabs along its longest axis (neighbouring slabs share a plane
//! of points), explicit meshes into ranges of cells, multi-block data block by block, tables
//! into ranges of rows. [`merge`] undoes [`split`]: structured pieces are reassembled exactly,
//! explicit pieces appended (points on piece borders appear once per piece).

use crate::*;

/// Piece `k` of `n` of a dataset.
pub fn split(ds: &DataSet, k: usize, n: usize) -> DataSet {
    if n <= 1 {
        return ds.clone();
    }
    match ds {
        // Cell grids are not split.
        DataSet::CellGrid(_) => ds.clone(),
        // A hyper tree grid splits by trees (cell data stays indexed globally).
        DataSet::HyperTreeGrid(h) => {
            let t = h.trees.len();
            let (lo, hi) = (t * k / n, t * (k + 1) / n);
            let mut out = h.clone();
            for (i, tree) in out.trees.iter_mut().enumerate() {
                if i < lo || i >= hi {
                    *tree = None;
                }
            }
            DataSet::HyperTreeGrid(out)
        }
        DataSet::ImageData(d) => match slab(d.extent, k, n) {
            Some((ext, pidx, cidx)) => {
                DataSet::ImageData(ImageData { extent: ext, point_data: d.point_data.gather(&pidx), cell_data: d.cell_data.gather(&cidx), ..d.clone() })
            }
            None => empty_like(ds),
        },
        DataSet::RectilinearGrid(d) => match slab(d.extent, k, n) {
            Some((ext, pidx, cidx)) => {
                let sub = |v: &Vec<f64>, a: usize| -> Vec<f64> {
                    let lo = (ext[2 * a] - d.extent[2 * a]) as usize;
                    let hi = (ext[2 * a + 1] - d.extent[2 * a]) as usize;
                    v[lo..=hi.min(v.len().saturating_sub(1))].to_vec()
                };
                DataSet::RectilinearGrid(RectilinearGrid {
                    extent: ext,
                    x: sub(&d.x, 0),
                    y: sub(&d.y, 1),
                    z: sub(&d.z, 2),
                    point_data: d.point_data.gather(&pidx),
                    cell_data: d.cell_data.gather(&cidx),
                    field_data: d.field_data.clone(),
                })
            }
            None => empty_like(ds),
        },
        DataSet::StructuredGrid(d) => match slab(d.extent, k, n) {
            Some((ext, pidx, cidx)) => DataSet::StructuredGrid(StructuredGrid {
                extent: ext,
                points: d.points.gather(&pidx),
                point_data: d.point_data.gather(&pidx),
                cell_data: d.cell_data.gather(&cidx),
                field_data: d.field_data.clone(),
            }),
            None => empty_like(ds),
        },
        DataSet::UnstructuredGrid(u) => {
            let (lo, hi) = range(u.cells.len(), k, n);
            DataSet::UnstructuredGrid(sub_grid(u, lo, hi))
        }
        DataSet::PolyData(p) => {
            let total = p.verts.len() + p.lines.len() + p.polys.len() + p.strips.len();
            let (lo, hi) = range(total, k, n);
            let mut out = PolyData { field_data: p.field_data.clone(), ..Default::default() };
            let mut parents = Vec::new();
            let mut conn = Vec::new();
            let mut base = 0;
            let mut parts: Vec<Cells> = Vec::new();
            for c in [&p.verts, &p.lines, &p.polys, &p.strips] {
                let mut part = Cells::new();
                for i in lo.max(base)..hi.min(base + c.len()) {
                    part.push(c.types[i - base], c.cell(i - base));
                    parents.push(i);
                }
                conn.extend_from_slice(&part.connectivity);
                parts.push(part);
                base += c.len();
            }
            let (pts, pidx) = compact(&p.points, &mut conn);
            let mut off = 0;
            for part in parts.iter_mut() {
                let m = part.connectivity.len();
                part.connectivity.copy_from_slice(&conn[off..off + m]);
                off += m;
            }
            out.points = pts;
            out.point_data = p.point_data.gather(&pidx);
            out.cell_data = p.cell_data.gather(&parents);
            out.strips = parts.pop().unwrap();
            out.polys = parts.pop().unwrap();
            out.lines = parts.pop().unwrap();
            out.verts = parts.pop().unwrap();
            DataSet::PolyData(out)
        }
        DataSet::MultiBlock(mb) => {
            let leaves = mb.leaves().len();
            let mut counter = 0;
            DataSet::MultiBlock(split_blocks(mb, k, n, leaves >= n, &mut counter))
        }
        DataSet::Table(t) => {
            let (lo, hi) = range(t.n_rows(), k, n);
            let idx: Vec<usize> = (lo..hi).collect();
            DataSet::Table(Table { columns: t.columns.gather(&idx), field_data: t.field_data.clone() })
        }
    }
}

/// Put pieces back together (in piece order).
pub fn merge(pieces: Vec<DataSet>) -> DataSet {
    let mut pieces: Vec<DataSet> = pieces;
    if pieces.len() <= 1 {
        return pieces.pop().unwrap_or(DataSet::PolyData(Default::default()));
    }
    let nonempty: Vec<&DataSet> = pieces.iter().filter(|p| !is_empty(p)).collect();
    if nonempty.is_empty() {
        return pieces.swap_remove(0);
    }
    if nonempty.len() == 1 {
        return nonempty[0].clone();
    }
    let same = |f: fn(&DataSet) -> bool| nonempty.iter().all(|p| f(p));
    if same(|p| matches!(p, DataSet::MultiBlock(_))) {
        let mbs: Vec<&MultiBlock> = nonempty.iter().filter_map(|p| if let DataSet::MultiBlock(m) = p { Some(m) } else { None }).collect();
        return DataSet::MultiBlock(merge_blocks_with(&mbs, merge));
    }
    if same(|p| matches!(p, DataSet::ImageData(_))) {
        if let Some(d) = merge_images(&nonempty) {
            return d;
        }
    }
    if same(|p| matches!(p, DataSet::RectilinearGrid(_))) {
        if let Some(d) = merge_rectilinear(&nonempty) {
            return d;
        }
    }
    if same(|p| matches!(p, DataSet::StructuredGrid(_))) {
        if let Some(d) = merge_structured(&nonempty) {
            return d;
        }
    }
    if same(|p| matches!(p, DataSet::Table(_))) {
        let cols: Vec<&Attributes> = nonempty.iter().filter_map(|p| if let DataSet::Table(t) = p { Some(&t.columns) } else { None }).collect();
        let field_data = nonempty[0].field_data().clone();
        return DataSet::Table(Table { columns: concat_attrs(&cols), field_data });
    }
    if same(|p| matches!(p, DataSet::PolyData(_))) {
        let polys: Vec<&PolyData> = nonempty.iter().filter_map(|p| if let DataSet::PolyData(d) = p { Some(d) } else { None }).collect();
        let mut out = PolyData { field_data: polys[0].field_data.clone(), ..Default::default() };
        let mut pts = Vec::new();
        let mut kinds: [Cells; 4] = Default::default();
        // Cells (and their data) in polydata order: all verts, then lines, polys, strips.
        let mut by_kind: [Vec<(usize, usize)>; 4] = Default::default();
        for (pi, p) in polys.iter().enumerate() {
            let off = (pts.len() / 3) as u32;
            pts.extend(p.points.data.to_f64_vec());
            let mut base = 0;
            for (ki, c) in [&p.verts, &p.lines, &p.polys, &p.strips].into_iter().enumerate() {
                kinds[ki].append(c, off);
                by_kind[ki].extend((0..c.len()).map(|i| (pi, base + i)));
                base += c.len();
            }
        }
        let order: Vec<(usize, usize)> = by_kind.into_iter().flatten().collect();
        out.cell_data = gather_from(&polys.iter().map(|p| &p.cell_data).collect::<Vec<_>>(), &order);
        out.point_data = concat_attrs(&polys.iter().map(|p| &p.point_data).collect::<Vec<_>>());
        out.points = Points::new_f64(pts);
        let [v, l, p, s] = kinds;
        out.verts = v;
        out.lines = l;
        out.polys = p;
        out.strips = s;
        return DataSet::PolyData(out);
    }
    // Anything else: as one unstructured grid.
    let grids: Vec<UnstructuredGrid> = nonempty.iter().filter_map(|p| p.to_unstructured()).collect();
    let mut out = UnstructuredGrid { field_data: nonempty[0].field_data().clone(), ..Default::default() };
    let mut pts = Vec::new();
    for g in &grids {
        let off = (pts.len() / 3) as u32;
        pts.extend(g.points.data.to_f64_vec());
        out.cells.append(&g.cells, off);
    }
    out.points = Points::new_f64(pts);
    out.point_data = concat_attrs(&grids.iter().map(|g| &g.point_data).collect::<Vec<_>>());
    out.cell_data = concat_attrs(&grids.iter().map(|g| &g.cell_data).collect::<Vec<_>>());
    DataSet::UnstructuredGrid(out)
}

/// [`merge`] for work that looks at how cells are connected (outer surfaces, connected
/// regions): a point that two pieces of an explicit mesh share (at exactly the same position)
/// appears once, with the values of the first piece, so cells of neighbouring pieces are
/// connected as they are in the whole. Points repeated inside one piece stay as they are.
pub fn merge_connected(pieces: Vec<DataSet>) -> DataSet {
    let nonempty: Vec<&DataSet> = pieces.iter().filter(|p| !is_empty(p)).collect();
    if nonempty.len() < 2 {
        return merge(pieces);
    }
    if nonempty.iter().all(|p| matches!(p, DataSet::MultiBlock(_))) {
        let mbs: Vec<&MultiBlock> = nonempty.iter().filter_map(|p| if let DataSet::MultiBlock(m) = p { Some(m) } else { None }).collect();
        return DataSet::MultiBlock(merge_blocks_with(&mbs, merge_connected));
    }
    // Where each piece's points start when the pieces are appended.
    let mut starts = vec![0usize];
    for p in &nonempty {
        starts.push(starts.last().unwrap() + p.n_points());
    }
    let total = *starts.last().unwrap();
    match merge(pieces) {
        DataSet::UnstructuredGrid(mut u) if u.points.len() == total && u.cells.polyhedron_faces.is_none() => {
            if let Some((keep, map)) = shared_points(&u.points, &starts) {
                u.points = u.points.gather(&keep);
                u.point_data = u.point_data.gather(&keep);
                u.cells.connectivity.iter_mut().for_each(|c| *c = map[*c as usize]);
            }
            DataSet::UnstructuredGrid(u)
        }
        DataSet::PolyData(mut p) if p.points.len() == total => {
            if let Some((keep, map)) = shared_points(&p.points, &starts) {
                p.points = p.points.gather(&keep);
                p.point_data = p.point_data.gather(&keep);
                for c in [&mut p.verts, &mut p.lines, &mut p.polys, &mut p.strips] {
                    c.connectivity.iter_mut().for_each(|c| *c = map[*c as usize]);
                }
            }
            DataSet::PolyData(p)
        }
        other => other,
    }
}

/// Points of appended pieces (piece `k` holds points `starts[k]..starts[k + 1]`) that repeat a
/// point of an earlier piece: (the points to keep, old index -> new index), or None when no
/// point is shared.
fn shared_points(points: &Points, starts: &[usize]) -> Option<(Vec<usize>, Vec<u32>)> {
    let n = points.len();
    let mut first: std::collections::HashMap<[u64; 3], (u32, usize)> = std::collections::HashMap::with_capacity(n);
    let mut keep = Vec::with_capacity(n);
    let mut map = vec![0u32; n];
    for piece in 0..starts.len().saturating_sub(1) {
        for i in starts[piece]..starts[piece + 1].min(n) {
            // (Adding zero makes -0.0 and 0.0 the same position.)
            let key = points.get(i).map(|v| (v + 0.0).to_bits());
            match first.get(&key) {
                Some(&(id, from)) if from != piece => map[i] = id,
                _ => {
                    let id = keep.len() as u32;
                    keep.push(i);
                    map[i] = id;
                    first.entry(key).or_insert((id, piece));
                }
            }
        }
    }
    (keep.len() < n).then_some((keep, map))
}

fn is_empty(ds: &DataSet) -> bool {
    match ds {
        DataSet::MultiBlock(m) => m.leaves().iter().all(|(_, d)| is_empty(d)),
        DataSet::Table(t) => t.n_rows() == 0,
        d => d.n_cells() == 0 && d.n_points() == 0,
    }
}

fn empty_like(ds: &DataSet) -> DataSet {
    match ds {
        DataSet::PolyData(_) => DataSet::PolyData(Default::default()),
        _ => DataSet::UnstructuredGrid(UnstructuredGrid { field_data: ds.field_data().clone(), ..Default::default() }),
    }
}

fn range(total: usize, k: usize, n: usize) -> (usize, usize) {
    (k * total / n, (k + 1) * total / n)
}

/// Slab `k` of `n` of a structured extent along its longest axis: (extent, point indices,
/// cell indices) into the whole grid, or None when the slab is empty.
fn slab(e: [i64; 6], k: usize, n: usize) -> Option<([i64; 6], Vec<usize>, Vec<usize>)> {
    let d: [usize; 3] = std::array::from_fn(|a| (e[2 * a + 1] - e[2 * a] + 1).max(0) as usize);
    let c: [usize; 3] = std::array::from_fn(|a| d[a].saturating_sub(1));
    let a = (0..3).max_by_key(|&a| c[a])?;
    if c[a] == 0 {
        return if k == 0 { Some((e, (0..d[0] * d[1] * d[2]).collect(), (0..1).collect())) } else { None };
    }
    let (lo, hi) = range(c[a], k, n);
    if lo == hi {
        return None;
    }
    let mut ext = e;
    ext[2 * a] = e[2 * a] + lo as i64;
    ext[2 * a + 1] = e[2 * a] + hi as i64;
    let pr: [std::ops::Range<usize>; 3] = std::array::from_fn(|b| if b == a { lo..hi + 1 } else { 0..d[b] });
    let cr: [std::ops::Range<usize>; 3] = std::array::from_fn(|b| if b == a { lo..hi } else { 0..c[b].max(if d[b] > 0 { 1 } else { 0 }) });
    let cd: [usize; 3] = std::array::from_fn(|b| c[b].max(1));
    let mut pidx = Vec::new();
    for z in pr[2].clone() {
        for y in pr[1].clone() {
            for x in pr[0].clone() {
                pidx.push(x + d[0] * (y + d[1] * z));
            }
        }
    }
    let mut cidx = Vec::new();
    for z in cr[2].clone() {
        for y in cr[1].clone() {
            for x in cr[0].clone() {
                cidx.push(x + cd[0] * (y + cd[1] * z));
            }
        }
    }
    Some((ext, pidx, cidx))
}

/// Keep only the points `conn` uses, in their original order (cells are then split into
/// tetrahedra exactly as in the whole dataset, which depends on the order of point ids).
fn compact(points: &Points, conn: &mut [u32]) -> (Points, Vec<usize>) {
    let mut used = vec![false; points.len()];
    for &c in conn.iter() {
        used[c as usize] = true;
    }
    let mut map = vec![u32::MAX; points.len()];
    let mut order = Vec::new();
    for (i, u) in used.iter().enumerate() {
        if *u {
            map[i] = order.len() as u32;
            order.push(i);
        }
    }
    for c in conn.iter_mut() {
        *c = map[*c as usize];
    }
    (points.gather(&order), order)
}

fn sub_grid(u: &UnstructuredGrid, lo: usize, hi: usize) -> UnstructuredGrid {
    let mut cells = Cells::new();
    let mut faces = Vec::new();
    for i in lo..hi {
        cells.push(u.cells.types[i], u.cells.cell(i));
        if let Some(f) = &u.cells.polyhedron_faces {
            faces.push(f.get(i).cloned().unwrap_or_default());
        }
    }
    let parents: Vec<usize> = (lo..hi).collect();
    if u.cells.polyhedron_faces.is_some() {
        // Polyhedron faces use global point ids: keep all points.
        cells.polyhedron_faces = Some(faces);
        return UnstructuredGrid {
            points: u.points.clone(),
            cells,
            point_data: u.point_data.clone(),
            cell_data: u.cell_data.gather(&parents),
            field_data: u.field_data.clone(),
        };
    }
    let (points, pidx) = compact(&u.points, &mut cells.connectivity);
    UnstructuredGrid { points, cells, point_data: u.point_data.gather(&pidx), cell_data: u.cell_data.gather(&parents), field_data: u.field_data.clone() }
}

fn split_blocks(mb: &MultiBlock, k: usize, n: usize, by_block: bool, counter: &mut usize) -> MultiBlock {
    let blocks = mb
        .blocks
        .iter()
        .map(|(name, b)| {
            let b = b.as_ref().map(|d| match d {
                DataSet::MultiBlock(inner) => DataSet::MultiBlock(split_blocks(inner, k, n, by_block, counter)),
                leaf if by_block => {
                    let mine = *counter % n == k;
                    *counter += 1;
                    if mine {
                        leaf.clone()
                    } else {
                        empty_like(leaf)
                    }
                }
                leaf => split(leaf, k, n),
            });
            (name.clone(), b)
        })
        .collect();
    MultiBlock { blocks, field_data: mb.field_data.clone() }
}

fn merge_blocks_with(mbs: &[&MultiBlock], merge: fn(Vec<DataSet>) -> DataSet) -> MultiBlock {
    let first = mbs[0];
    let blocks = (0..first.blocks.len())
        .map(|i| {
            let parts: Vec<DataSet> = mbs.iter().filter_map(|m| m.blocks.get(i).and_then(|b| b.1.clone())).collect();
            let merged = if parts.is_empty() { None } else { Some(merge(parts)) };
            (first.blocks[i].0.clone(), merged)
        })
        .collect();
    MultiBlock { blocks, field_data: first.field_data.clone() }
}

/// Arrays present in every set, concatenated.
fn concat_attrs(sets: &[&Attributes]) -> Attributes {
    let order: Vec<(usize, usize)> =
        sets.iter().enumerate().flat_map(|(s, a)| (0..a.iter().next().map(|x| x.len()).unwrap_or(0)).map(move |i| (s, i))).collect();
    let mut out = gather_from(sets, &order);
    out.active = sets.first().map(|a| a.active.clone()).unwrap_or_default();
    out.active.retain(|_, n| out.arrays.contains_key(n));
    out
}

/// Arrays present in every set, with tuple `order[i] = (set, index)`.
fn gather_from(sets: &[&Attributes], order: &[(usize, usize)]) -> Attributes {
    let mut out = Attributes::new();
    let Some(first) = sets.first() else { return out };
    for arr in first.iter() {
        let all: Option<Vec<&Array>> = sets.iter().map(|s| s.get(&arr.name).filter(|a| a.ncomp == arr.ncomp)).collect();
        let Some(all) = all else { continue };
        let nc = arr.ncomp;
        let data = if arr.scalar_type() == ScalarType::String {
            // Text is copied as it is (pieces holding another type under this name: skipped).
            let Some(text) = all.iter().map(|a| a.as_slice::<String>()).collect::<Option<Vec<_>>>() else { continue };
            let mut v = Vec::with_capacity(order.len() * nc);
            for &(s, i) in order {
                v.extend_from_slice(&text[s][i * nc..(i + 1) * nc]);
            }
            ArrayData::String(v)
        } else {
            let mut v = Vec::with_capacity(order.len() * nc);
            for &(s, i) in order {
                for c in 0..nc {
                    v.push(all[s].get(i, c));
                }
            }
            ArrayData::from_f64(arr.scalar_type(), &v)
        };
        let mut a = Array::new(arr.name.clone(), nc, data);
        a.component_names = arr.component_names.clone();
        // The units of the first piece that states them.
        a.units = all.iter().find_map(|a| a.units.clone());
        out.insert(a);
    }
    out.active = first.active.clone();
    out.active.retain(|_, n| out.arrays.contains_key(n));
    out
}

/// Reassemble structured pieces into the extent covering them all: (extent, point source,
/// cell source) where each source maps a global index to (piece, local index).
#[allow(clippy::type_complexity)]
fn assemble(extents: &[[i64; 6]]) -> ([i64; 6], Vec<(usize, usize)>, Vec<(usize, usize)>) {
    let mut e = extents[0];
    for x in extents {
        for a in 0..3 {
            e[2 * a] = e[2 * a].min(x[2 * a]);
            e[2 * a + 1] = e[2 * a + 1].max(x[2 * a + 1]);
        }
    }
    let dims = |x: &[i64; 6]| -> [usize; 3] { std::array::from_fn(|a| (x[2 * a + 1] - x[2 * a] + 1).max(0) as usize) };
    let d = dims(&e);
    let cd: [usize; 3] = std::array::from_fn(|a| d[a].saturating_sub(1).max(1));
    let mut psrc = vec![(0usize, 0usize); d[0] * d[1] * d[2]];
    let mut csrc = vec![(0usize, 0usize); cd[0] * cd[1] * cd[2]];
    for (p, x) in extents.iter().enumerate() {
        let pd = dims(x);
        let pc: [usize; 3] = std::array::from_fn(|a| pd[a].saturating_sub(1).max(1));
        let o: [usize; 3] = std::array::from_fn(|a| (x[2 * a] - e[2 * a]) as usize);
        for z in 0..pd[2] {
            for y in 0..pd[1] {
                for xx in 0..pd[0] {
                    psrc[(o[0] + xx) + d[0] * ((o[1] + y) + d[1] * (o[2] + z))] = (p, xx + pd[0] * (y + pd[1] * z));
                }
            }
        }
        for z in 0..pc[2].min(cd[2] - o[2].min(cd[2])) {
            for y in 0..pc[1].min(cd[1] - o[1].min(cd[1])) {
                for xx in 0..pc[0].min(cd[0] - o[0].min(cd[0])) {
                    csrc[(o[0] + xx) + cd[0] * ((o[1] + y) + cd[1] * (o[2] + z))] = (p, xx + pc[0] * (y + pc[1] * z));
                }
            }
        }
    }
    (e, psrc, csrc)
}

fn merge_images(pieces: &[&DataSet]) -> Option<DataSet> {
    let imgs: Vec<&ImageData> = pieces.iter().filter_map(|p| if let DataSet::ImageData(d) = p { Some(d) } else { None }).collect();
    let f = imgs[0];
    if imgs.iter().any(|d| d.origin != f.origin || d.spacing != f.spacing || d.direction != f.direction) {
        return None;
    }
    let (e, psrc, csrc) = assemble(&imgs.iter().map(|d| d.extent).collect::<Vec<_>>());
    Some(DataSet::ImageData(ImageData {
        extent: e,
        point_data: gather_from(&imgs.iter().map(|d| &d.point_data).collect::<Vec<_>>(), &psrc),
        cell_data: gather_from(&imgs.iter().map(|d| &d.cell_data).collect::<Vec<_>>(), &csrc),
        ..f.clone()
    }))
}

fn merge_rectilinear(pieces: &[&DataSet]) -> Option<DataSet> {
    let gs: Vec<&RectilinearGrid> = pieces.iter().filter_map(|p| if let DataSet::RectilinearGrid(d) = p { Some(d) } else { None }).collect();
    let (e, psrc, csrc) = assemble(&gs.iter().map(|d| d.extent).collect::<Vec<_>>());
    // Coordinates along each axis, from whichever piece covers each index.
    let coords = |a: usize| -> Vec<f64> {
        let n = (e[2 * a + 1] - e[2 * a] + 1).max(0) as usize;
        (0..n)
            .map(|i| {
                let g = e[2 * a] + i as i64;
                gs.iter()
                    .find(|d| g >= d.extent[2 * a] && g <= d.extent[2 * a + 1])
                    .map(|d| {
                        let v = [&d.x, &d.y, &d.z][a];
                        v[(g - d.extent[2 * a]) as usize]
                    })
                    .unwrap_or(0.0)
            })
            .collect()
    };
    Some(DataSet::RectilinearGrid(RectilinearGrid {
        extent: e,
        x: coords(0),
        y: coords(1),
        z: coords(2),
        point_data: gather_from(&gs.iter().map(|d| &d.point_data).collect::<Vec<_>>(), &psrc),
        cell_data: gather_from(&gs.iter().map(|d| &d.cell_data).collect::<Vec<_>>(), &csrc),
        field_data: gs[0].field_data.clone(),
    }))
}

fn merge_structured(pieces: &[&DataSet]) -> Option<DataSet> {
    let gs: Vec<&StructuredGrid> = pieces.iter().filter_map(|p| if let DataSet::StructuredGrid(d) = p { Some(d) } else { None }).collect();
    let (e, psrc, csrc) = assemble(&gs.iter().map(|d| d.extent).collect::<Vec<_>>());
    let mut pts = Vec::with_capacity(3 * psrc.len());
    for &(p, i) in &psrc {
        pts.extend_from_slice(&gs[p].points.get(i));
    }
    Some(DataSet::StructuredGrid(StructuredGrid {
        extent: e,
        points: Points::new_f64(pts),
        point_data: gather_from(&gs.iter().map(|d| &d.point_data).collect::<Vec<_>>(), &psrc),
        cell_data: gather_from(&gs.iter().map(|d| &d.cell_data).collect::<Vec<_>>(), &csrc),
        field_data: gs[0].field_data.clone(),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn image() -> DataSet {
        let mut img = ImageData { extent: [0, 9, 0, 4, 0, 2], origin: [1.0, 2.0, 3.0], spacing: [0.5, 1.0, 2.0], ..Default::default() };
        let n = img.n_points();
        img.point_data.insert(Array::scalars_f64("p", (0..n).map(|i| i as f64).collect()));
        let c = img.n_cells();
        img.cell_data.insert(Array::scalars_f64("c", (0..c).map(|i| i as f64 * 2.0).collect()));
        DataSet::ImageData(img)
    }

    /// Units and text arrays survive splitting and putting the pieces back together.
    #[test]
    fn merge_keeps_units_and_text() {
        let DataSet::ImageData(mut img) = image() else { panic!() };
        let p = img.point_data.get("p").unwrap().clone().with_units("K");
        img.point_data.insert(p);
        let c = img.cell_data.get("c").unwrap().clone().with_units("m/s");
        img.cell_data.insert(c);
        let n = img.n_points();
        img.point_data.insert(Array::new("label", 1, ArrayData::String((0..n).map(|i| format!("p{i}")).collect())));
        let img = DataSet::ImageData(img);
        let ug = DataSet::UnstructuredGrid(img.to_unstructured().unwrap());
        for whole in [&img, &ug] {
            let m = merge((0..3).map(|k| split(whole, k, 3)).collect());
            assert_eq!(m.point_data().unwrap().get("p").unwrap().units.as_deref(), Some("K"));
            assert_eq!(m.cell_data().unwrap().get("c").unwrap().units.as_deref(), Some("m/s"));
            let text = m.point_data().unwrap().get("label").expect("the text array is kept");
            assert_eq!(text.len(), m.n_points());
            assert!(text.as_slice::<String>().unwrap().iter().all(|t| t.starts_with('p')));
        }
        assert_eq!(merge((0..3).map(|k| split(&img, k, 3)).collect()), img, "an image is reassembled exactly");
    }

    #[test]
    fn split_and_merge_round_trip() {
        let img = image();
        for n in [1, 2, 3, 4, 9, 12] {
            let pieces: Vec<DataSet> = (0..n).map(|k| split(&img, k, n)).collect();
            let cells: usize = pieces.iter().map(|p| p.n_cells()).sum();
            assert_eq!(cells, img.n_cells(), "{n} pieces cover every cell once");
            assert_eq!(merge(pieces), img, "{n} pieces reassemble the image");
        }
        let ug = DataSet::UnstructuredGrid(img.to_unstructured().unwrap());
        let pieces: Vec<DataSet> = (0..3).map(|k| split(&ug, k, 3)).collect();
        assert_eq!(pieces.iter().map(|p| p.n_cells()).sum::<usize>(), ug.n_cells());
        // Appended, the points on the borders between pieces repeat; connected, they do not.
        let c = merge_connected(pieces.clone());
        assert_eq!((c.n_points(), c.n_cells()), (ug.n_points(), ug.n_cells()));
        assert_eq!(c.bounds(), ug.bounds());
        let m = merge(pieces);
        assert!(m.n_points() > ug.n_points());
        assert_eq!(m.n_cells(), ug.n_cells());
        assert_eq!(m.cell_data().unwrap().get("c").unwrap(), ug.cell_data().unwrap().get("c").unwrap());
        assert_eq!(m.bounds(), ug.bounds());
        let mut mb = MultiBlock::default();
        mb.blocks.push(("a".into(), Some(img.clone())));
        mb.blocks.push(("b".into(), Some(ug.clone())));
        let mb = DataSet::MultiBlock(mb);
        for n in [2, 3] {
            let pieces: Vec<DataSet> = (0..n).map(|k| split(&mb, k, n)).collect();
            let m = merge(pieces);
            let DataSet::MultiBlock(m) = m else { panic!() };
            assert_eq!(m.blocks[0].1.as_ref().unwrap(), &img);
            assert_eq!(m.blocks[1].1.as_ref().unwrap().n_cells(), ug.n_cells());
        }
    }
}
