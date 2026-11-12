//! Ghost layers between the processes of a parallel server.
//!
//! Each process holds one piece. A filter that looks at a cell's neighbours (a gradient, cell
//! data to point data, normals, curvature) gives wrong values on a piece's border unless the
//! piece can see the neighbouring pieces' cells there. The coordinator collects every piece's
//! border cells ([`piece_border`]) and hands each process the borders of the other pieces
//! ([`store`]); the process adds the cells that touch its piece as ghosts ([`append_ghosts`]),
//! runs the filter, and keeps only its own points and cells of the result ([`drop_ghosts`]),
//! so the pieces put together equal the result of one process.
//!
//! Explicit meshes get the ghost cells after their own cells and points (own points keep their
//! order, so splitting cells into simplices is unchanged); image, rectilinear and structured
//! grids grow by the neighbouring slabs, so their filters still see a grid. Multi-block pieces
//! are handled block by block (blocks matched by name, or by position when names repeat or are
//! empty). Meshes with polyhedron cells get no ghosts.

use crate::registry::{FilterDef, MultiInput};
use anyhow::{anyhow, bail, Result};
use aurafx_data::*;
use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};

/// Marks ghost cells (1) while a filter runs; never left in a result.
const MARK: &str = "__aurafx_ghost";

/// Start of the error a process gives when it was asked to add ghosts it does not hold (it was
/// restarted, or dropped them). The coordinator sends them again when it sees this.
pub const NOT_SENT: &str = "the ghost cells of this piece were not sent";

/// The most ghost sets a process keeps. The coordinator says when a set is no longer needed
/// ([`forget`]); this bound only matters when it never does.
const MOST: usize = 4096;

/// Ghost candidates handed to this process, by key, oldest first.
fn table() -> &'static Mutex<Vec<(String, Arc<DataSet>)>> {
    static T: OnceLock<Mutex<Vec<(String, Arc<DataSet>)>>> = OnceLock::new();
    T.get_or_init(Default::default)
}

fn lock() -> std::sync::MutexGuard<'static, Vec<(String, Arc<DataSet>)>> {
    table().lock().unwrap_or_else(|e| e.into_inner())
}

/// Keep the border cells of the other pieces under `key` until [`forget`] names it.
pub fn store(key: &str, ds: DataSet) {
    let mut t = lock();
    t.retain(|(k, _)| k != key);
    t.push((key.to_string(), Arc::new(ds)));
    let n = t.len();
    if n > MOST {
        t.drain(..n - MOST);
    }
}

/// Drop the border cells kept under these keys.
pub fn forget(keys: &[String]) {
    lock().retain(|(k, _)| !keys.contains(k));
}

/// Drop every set of border cells this process holds (as a restarted process would have).
pub fn forget_all() {
    lock().clear();
}

/// How many sets of border cells this process holds.
pub fn held() -> usize {
    lock().len()
}

fn fetch(key: &str) -> Option<Arc<DataSet>> {
    lock().iter().find(|(k, _)| k == key).map(|(_, d)| d.clone())
}

fn has_polyhedra(u: &UnstructuredGrid) -> bool {
    u.cells.polyhedron_faces.is_some() || u.cells.types.contains(&CellType::Polyhedron)
}

fn extent(ds: &DataSet) -> Option<[i64; 6]> {
    match ds {
        DataSet::ImageData(d) => Some(d.extent),
        DataSet::RectilinearGrid(d) => Some(d.extent),
        DataSet::StructuredGrid(d) => Some(d.extent),
        _ => None,
    }
}

/// The cells of a piece within `layers` cells of its outer boundary (its border with other
/// pieces lies there). Structured grids give the slabs of `layers` cells along each face, as
/// blocks of a multi-block.
pub fn piece_border(ds: &DataSet, layers: usize) -> Result<DataSet> {
    if let DataSet::MultiBlock(mb) = ds {
        let mut out = MultiBlock { field_data: mb.field_data.clone(), ..Default::default() };
        for (name, b) in &mb.blocks {
            out.blocks.push((name.clone(), b.as_ref().map(|b| piece_border(b, layers)).transpose()?));
        }
        return Ok(DataSet::MultiBlock(out));
    }
    let layers = layers.max(1);
    if let Some(e) = extent(ds) {
        let mut mb = MultiBlock::default();
        for a in 0..3 {
            let (e0, e1) = (e[2 * a], e[2 * a + 1]);
            if e1 <= e0 {
                continue;
            }
            for (lo, hi) in [(e0, (e0 + layers as i64).min(e1)), ((e1 - layers as i64).max(e0), e1)] {
                let (mut mn, mut mx) = ([f64::NAN; 3], [f64::NAN; 3]);
                for b in 0..3 {
                    mn[b] = e[2 * b] as f64;
                    mx[b] = e[2 * b + 1] as f64;
                }
                mn[a] = lo as f64;
                mx[a] = hi as f64;
                let mut s = crate::geometry::extract_subset(ds, mn, mx, [1.0; 3], false)?;
                // Back on this grid's (absolute) indices, so pieces can be matched by extent.
                let mut sub = e;
                sub[2 * a] = lo;
                sub[2 * a + 1] = hi;
                match (&mut s, ds) {
                    (DataSet::ImageData(d), DataSet::ImageData(o)) => {
                        d.extent = sub;
                        d.origin = o.origin;
                    }
                    (DataSet::RectilinearGrid(d), _) => d.extent = sub,
                    (DataSet::StructuredGrid(d), _) => d.extent = sub,
                    _ => {}
                }
                mb.blocks.push((format!("face {a} {lo}"), Some(s)));
            }
        }
        return Ok(DataSet::MultiBlock(mb));
    }
    let u = ds.to_unstructured().ok_or_else(|| anyhow!("this kind of data has no border cells"))?;
    // Facets used by one cell only are on the boundary; so are their points.
    let mut count: HashMap<Vec<u32>, u32> = HashMap::new();
    let mut all_points = vec![false; u.points.len()];
    let facets = |ty: CellType, c: &[u32], out: &mut Vec<Vec<u32>>| match ty.dimension() {
        3 => {
            let f = ty.faces();
            if f.is_empty() || ty == CellType::Polyhedron {
                out.push(Vec::new()); // unknown faces: every point counts
            }
            for face in f {
                out.push(face.iter().filter_map(|&k| c.get(k).copied()).collect());
            }
        }
        2 => {
            let n = match ty {
                CellType::Triangle | CellType::QuadraticTriangle | CellType::LagrangeTriangle => 3,
                CellType::Quad | CellType::QuadraticQuad | CellType::BiquadraticQuad | CellType::LagrangeQuadrilateral => 4,
                CellType::Pixel => 0,
                _ => c.len(),
            };
            if ty == CellType::Pixel {
                for (a, b) in [(0, 1), (1, 3), (3, 2), (2, 0)] {
                    out.push(vec![c[a], c[b]]);
                }
            } else if ty == CellType::TriangleStrip {
                out.push(Vec::new());
            } else {
                for k in 0..n.min(c.len()) {
                    out.push(vec![c[k], c[(k + 1) % n.min(c.len())]]);
                }
            }
        }
        1 => {
            if let (Some(&a), Some(&b)) = (c.first(), c.last()) {
                out.push(vec![a]);
                out.push(vec![b]);
            }
        }
        _ => out.push(c.to_vec()),
    };
    let mut fs = Vec::new();
    for ci in 0..u.cells.len() {
        let (ty, c) = (u.cells.cell_type(ci), u.cells.cell(ci));
        fs.clear();
        facets(ty, c, &mut fs);
        for f in fs.drain(..) {
            if f.is_empty() {
                for &p in c {
                    all_points[p as usize] = true;
                }
                continue;
            }
            let mut k = f;
            k.sort_unstable();
            *count.entry(k).or_insert(0) += 1;
        }
    }
    let mut on_border = all_points;
    for (k, n) in &count {
        if *n == 1 {
            for &p in k {
                on_border[p as usize] = true;
            }
        }
    }
    // Cells touching the border, then `layers - 1` more rings inwards.
    let mut keep = vec![false; u.cells.len()];
    for _ in 0..layers {
        let mut added = Vec::new();
        for (ci, k) in keep.iter().enumerate() {
            if !*k && u.cells.cell(ci).iter().any(|&p| on_border[p as usize]) {
                added.push(ci);
            }
        }
        for &ci in &added {
            keep[ci] = true;
            for &p in u.cells.cell(ci) {
                on_border[p as usize] = true;
            }
        }
    }
    let idx: Vec<usize> = (0..keep.len()).filter(|&c| keep[c]).collect();
    Ok(DataSet::UnstructuredGrid(crate::partitions::subset(&u, &idx)))
}

/// `own` with the cells of `cands` (the borders of the other pieces, one block each) that lie
/// within `layers` cells of it added as ghosts.
pub fn append_ghosts(own: &DataSet, cands: &DataSet, layers: usize) -> Result<DataSet> {
    let pieces: Vec<&DataSet> = match cands {
        DataSet::MultiBlock(mb) => mb.blocks.iter().filter_map(|b| b.1.as_ref()).collect(),
        d => vec![d],
    };
    append_in(own, &pieces, layers)
}

/// Whether every block has a name of its own (so blocks can be matched by name).
fn distinct_names(mb: &MultiBlock) -> bool {
    let mut seen = std::collections::HashSet::new();
    mb.blocks.iter().all(|b| !b.0.is_empty() && seen.insert(b.0.as_str()))
}

/// The block of each candidate piece that matches block `i` (called `name`) of the own piece:
/// by name when both sides name every block differently, else by position.
fn sub_blocks<'a>(pieces: &[&'a DataSet], i: usize, name: &str, by_name: bool) -> Vec<&'a DataSet> {
    pieces
        .iter()
        .filter_map(|p| match p {
            DataSet::MultiBlock(mb) if by_name && distinct_names(mb) => mb.blocks.iter().find(|b| b.0 == name).and_then(|b| b.1.as_ref()),
            DataSet::MultiBlock(mb) => mb.blocks.get(i).and_then(|b| b.1.as_ref()),
            _ => None,
        })
        .collect()
}

fn append_in(own: &DataSet, pieces: &[&DataSet], layers: usize) -> Result<DataSet> {
    if let DataSet::MultiBlock(mb) = own {
        let mut out = MultiBlock { field_data: mb.field_data.clone(), ..Default::default() };
        let by_name = distinct_names(mb);
        for (i, (name, b)) in mb.blocks.iter().enumerate() {
            let sub = sub_blocks(pieces, i, name, by_name);
            out.blocks.push((name.clone(), b.as_ref().map(|b| append_in(b, &sub, layers)).transpose()?));
        }
        return Ok(DataSet::MultiBlock(out));
    }
    // A structured border is a multi-block of face slabs; an explicit one a mesh.
    let leaves: Vec<&DataSet> = pieces
        .iter()
        .flat_map(|p| match p {
            DataSet::MultiBlock(mb) => mb.blocks.iter().filter_map(|b| b.1.as_ref()).collect::<Vec<_>>(),
            d => vec![*d],
        })
        .collect();
    if let Some(e) = extent(own) {
        // Slabs of the same kind of grid that sit against one face of this piece and cover it.
        let mut parts = vec![own.clone()];
        for l in leaves {
            let Some(c) = extent(l) else { continue };
            if std::mem::discriminant(l) != std::mem::discriminant(own) {
                continue;
            }
            let touches = (0..3).any(|a| {
                (c[2 * a] == e[2 * a + 1] || c[2 * a + 1] == e[2 * a])
                    && c[2 * a + 1] > c[2 * a]
                    && (0..3).filter(|&b| b != a).all(|b| c[2 * b] == e[2 * b] && c[2 * b + 1] == e[2 * b + 1])
            });
            if touches {
                parts.push(l.clone());
            }
        }
        if parts.len() == 1 {
            return Ok(own.clone());
        }
        let merged = aurafx_data::pieces::merge(parts);
        if std::mem::discriminant(&merged) != std::mem::discriminant(own) {
            return Ok(own.clone());
        }
        return Ok(merged);
    }
    let Some(u) = own.to_unstructured() else { return Ok(own.clone()) };
    let others: Vec<UnstructuredGrid> = leaves.iter().filter_map(|l| l.to_unstructured()).filter(|g| !g.cells.is_empty()).collect();
    if others.is_empty() {
        return Ok(own.clone());
    }
    // Polyhedron faces index points on their own; such pieces run without ghosts.
    if has_polyhedra(&u) || others.iter().any(has_polyhedra) {
        return Ok(own.clone());
    }
    let mut b = own.bounds();
    for g in &others {
        let gb = DataSet::UnstructuredGrid(g.clone()).bounds();
        for a in 0..3 {
            b.min[a] = b.min[a].min(gb.min[a]);
            b.max[a] = b.max[a].max(gb.max[a]);
        }
    }
    // Positions are compared relative to the corner of the box around everything, so a small
    // part far from the origin still has distinct keys (at most 1e12 along an axis).
    let tol = (b.diagonal() * 1e-12).max(1e-300);
    let o = b.min;
    let key = |q: [f64; 3]| -> [i64; 3] { std::array::from_fn(|a| ((q[a] - o[a]) / tol).round() as i64) };
    let mut index: HashMap<[i64; 3], u32> = HashMap::with_capacity(u.points.len());
    for i in 0..u.points.len() {
        index.entry(key(u.points.get(i))).or_insert(i as u32);
    }
    let n_own = u.points.len();
    // Candidate points: an own point, or a new point (numbered after the own points).
    let mut new_points: Vec<(usize, usize)> = Vec::new(); // (candidate, point)
    let mut cand_map: Vec<Vec<u32>> = Vec::new();
    for (g, cu) in others.iter().enumerate() {
        let mut m = Vec::with_capacity(cu.points.len());
        for i in 0..cu.points.len() {
            let next = (n_own + new_points.len()) as u32;
            let id = *index.entry(key(cu.points.get(i))).or_insert_with(|| {
                new_points.push((g, i));
                next
            });
            m.push(id);
        }
        cand_map.push(m);
    }
    let total = n_own + new_points.len();
    let mut point_cells: Vec<Vec<(usize, usize)>> = vec![Vec::new(); total];
    for (g, cu) in others.iter().enumerate() {
        for c in 0..cu.cells.len() {
            for &p in cu.cells.cell(c) {
                point_cells[cand_map[g][p as usize] as usize].push((g, c));
            }
        }
    }
    let mut picked: Vec<(usize, usize)> = Vec::new();
    let mut seen: std::collections::HashSet<(usize, usize)> = Default::default();
    let mut frontier: Vec<usize> = (0..n_own).filter(|&p| !point_cells[p].is_empty()).collect();
    for _ in 0..layers.max(1) {
        let mut next = Vec::new();
        for &p in &frontier {
            for &(g, c) in &point_cells[p] {
                if seen.insert((g, c)) {
                    picked.push((g, c));
                    next.extend(others[g].cells.cell(c).iter().map(|&q| cand_map[g][q as usize] as usize));
                }
            }
        }
        next.sort_unstable();
        next.dedup();
        frontier = next;
    }
    if picked.is_empty() {
        return Ok(own.clone());
    }
    // Ghost points: only those the picked cells use, after the own points.
    let mut renum: HashMap<u32, u32> = HashMap::new();
    let mut ghost_pts: Vec<(usize, usize)> = Vec::new();
    let mut cells_of: Vec<(CellType, Vec<u32>)> = Vec::with_capacity(picked.len());
    for &(g, c) in &picked {
        let ids = others[g]
            .cells
            .cell(c)
            .iter()
            .map(|&q| {
                let id = cand_map[g][q as usize];
                if (id as usize) < n_own {
                    id
                } else {
                    *renum.entry(id).or_insert_with(|| {
                        ghost_pts.push(new_points[id as usize - n_own]);
                        (n_own + ghost_pts.len() - 1) as u32
                    })
                }
            })
            .collect();
        cells_of.push((others[g].cells.cell_type(c), ids));
    }
    let mut points = u.points.clone();
    for &(g, i) in &ghost_pts {
        points.push(others[g].points.get(i));
    }
    // Arrays: the own values, then the ghosts' (per candidate, gathered in order).
    let gather_multi = |own_attrs: &Attributes, sets: Vec<&Attributes>, picks: &[(usize, usize)]| -> Attributes {
        let mut out = own_attrs.clone();
        let names: Vec<String> = own_attrs.names().map(|s| s.to_string()).collect();
        for name in names {
            let a = own_attrs.get(&name).unwrap();
            if a.scalar_type() == ScalarType::String {
                // Text: the ghosts' own, or empty where a neighbour has none.
                let mut vals: Vec<String> = Vec::with_capacity(picks.len() * a.ncomp);
                for &(g, i) in picks {
                    match sets[g].get(&name).filter(|o| o.ncomp == a.ncomp).and_then(|o| o.as_slice::<String>()) {
                        Some(o) => vals.extend_from_slice(&o[i * a.ncomp..(i + 1) * a.ncomp]),
                        None => vals.extend(std::iter::repeat_n(String::new(), a.ncomp)),
                    }
                }
                let mut b = a.clone();
                let _ = b.data.append(&ArrayData::String(vals));
                out.insert(b);
                continue;
            }
            let mut vals = Vec::with_capacity(picks.len() * a.ncomp);
            for &(g, i) in picks {
                match sets[g].get(&name).filter(|o| o.ncomp == a.ncomp) {
                    Some(o) => vals.extend((0..a.ncomp).map(|c| o.get(i, c))),
                    None => vals.extend(std::iter::repeat_n(0.0, a.ncomp)),
                }
            }
            let mut b = a.clone();
            let _ = b.data.append(&ArrayData::from_f64(a.scalar_type(), &vals));
            out.insert(b);
        }
        out
    };
    let point_data = gather_multi(&u.point_data, others.iter().map(|g| &g.point_data).collect(), &ghost_pts);
    let mut cell_data = gather_multi(&u.cell_data, others.iter().map(|g| &g.cell_data).collect(), &picked);
    let n_own_cells = u.cells.len();
    match own {
        DataSet::PolyData(p) => {
            // Keep the verts, lines, polys, strips order: own cells of each kind, then ghosts.
            let mut out = PolyData { points, point_data, field_data: p.field_data.clone(), ..Default::default() };
            let kind = |ty: CellType| match ty.dimension() {
                0 => 0,
                1 => 1,
                _ if ty == CellType::TriangleStrip => 3,
                _ => 2,
            };
            let mut order: Vec<(usize, bool, usize)> = Vec::new(); // (kind, ghost, source index)
            for c in 0..n_own_cells {
                order.push((kind(u.cells.cell_type(c)), false, c));
            }
            for (k, (ty, _)) in cells_of.iter().enumerate() {
                order.push((kind(*ty), true, k));
            }
            order.sort_by_key(|&(k, g, i)| (k, g, i));
            let mut cd_idx = Vec::with_capacity(order.len());
            let mut mark = Vec::with_capacity(order.len());
            for &(k, g, i) in &order {
                let (ty, ids): (CellType, &[u32]) = if g { (cells_of[i].0, &cells_of[i].1) } else { (u.cells.cell_type(i), u.cells.cell(i)) };
                match k {
                    0 => out.verts.push(ty, ids),
                    1 => out.lines.push(ty, ids),
                    2 => out.polys.push(ty, ids),
                    _ => out.strips.push(ty, ids),
                }
                cd_idx.push(if g { n_own_cells + i } else { i });
                mark.push(u8::from(g));
            }
            let mut reordered = Attributes::new();
            for a in cell_data.iter() {
                reordered.insert(a.gather(&cd_idx));
            }
            cell_data = reordered;
            cell_data.insert(Array::new(MARK, 1, ArrayData::U8(mark)));
            out.cell_data = cell_data;
            Ok(DataSet::PolyData(out))
        }
        _ => {
            let mut cells = u.cells.clone();
            for (ty, ids) in &cells_of {
                cells.push(*ty, ids);
            }
            let mut mark = vec![0u8; n_own_cells];
            mark.resize(n_own_cells + cells_of.len(), 1);
            cell_data.insert(Array::new(MARK, 1, ArrayData::U8(mark)));
            Ok(DataSet::UnstructuredGrid(UnstructuredGrid { points, cells, point_data, cell_data, field_data: u.field_data.clone() }))
        }
    }
}

/// The result `gx` of a filter run on `own` with ghosts, back on `own`: its points and cells,
/// with the arrays the filter left (values of arrays it did not change keep their own type).
pub fn drop_ghosts(gx: &DataSet, own: &DataSet) -> Result<DataSet> {
    if let (DataSet::MultiBlock(g), DataSet::MultiBlock(o)) = (gx, own) {
        if g.blocks.len() != o.blocks.len() {
            bail!("the filter changed the blocks; run it on one server process");
        }
        let mut out = MultiBlock { field_data: g.field_data.clone(), ..Default::default() };
        for ((name, gb), (_, ob)) in g.blocks.iter().zip(&o.blocks) {
            let b = match (gb, ob) {
                (Some(gb), Some(ob)) => Some(drop_ghosts(gb, ob)?),
                (gb, _) => gb.clone(),
            };
            out.blocks.push((name.clone(), b));
        }
        return Ok(DataSet::MultiBlock(out));
    }
    if own.to_unstructured().is_none() && extent(own).is_none() {
        return Ok(gx.clone());
    }
    let (pts, cells): (Vec<usize>, Vec<usize>) = if let (Some(e), Some(g)) = (extent(own), extent(gx)) {
        if (0..3).any(|a| g[2 * a] > e[2 * a] || g[2 * a + 1] < e[2 * a + 1]) {
            bail!("the filter changed the grid; run it on one server process");
        }
        let dims = |x: &[i64; 6]| -> [i64; 3] { std::array::from_fn(|a| x[2 * a + 1] - x[2 * a] + 1) };
        let (dg, de) = (dims(&g), dims(&e));
        let cg: [i64; 3] = dg.map(|d| (d - 1).max(1));
        let ce: [i64; 3] = de.map(|d| (d - 1).max(1));
        let o: [i64; 3] = std::array::from_fn(|a| e[2 * a] - g[2 * a]);
        let mut p = Vec::with_capacity((de[0] * de[1] * de[2]) as usize);
        for z in 0..de[2] {
            for y in 0..de[1] {
                for x in 0..de[0] {
                    p.push(((o[0] + x) + dg[0] * ((o[1] + y) + dg[1] * (o[2] + z))) as usize);
                }
            }
        }
        let mut c = Vec::with_capacity((ce[0] * ce[1] * ce[2]) as usize);
        for z in 0..ce[2] {
            for y in 0..ce[1] {
                for x in 0..ce[0] {
                    c.push((((o[0] + x).min(cg[0] - 1)) + cg[0] * (((o[1] + y).min(cg[1] - 1)) + cg[1] * ((o[2] + z).min(cg[2] - 1)))) as usize);
                }
            }
        }
        (p, c)
    } else {
        let (np, nc) = (own.n_points(), own.n_cells());
        let cells: Vec<usize> = match gx.cell_data().and_then(|c| c.get(MARK)) {
            Some(m) => (0..m.len()).filter(|&c| m.get(c, 0) == 0.0).collect(),
            None => (0..nc).collect(),
        };
        if gx.n_points() < np || cells.len() != nc {
            bail!("the filter changed the mesh; run it on one server process");
        }
        ((0..np).collect(), cells)
    };
    let mut out = own.clone();
    let pick = |from: Option<&Attributes>, mine: Option<&Attributes>, idx: &[usize]| -> Attributes {
        let mut res = Attributes::new();
        let Some(from) = from else { return res };
        let need = idx.iter().copied().max().map_or(0, |m| m + 1);
        for a in from.iter().filter(|a| !a.name.starts_with("__aurafx")) {
            if a.len() < need {
                continue; // not one value per point or cell of the piece with ghosts
            }
            let g = a.gather(idx);
            let same = mine.and_then(|m| m.get(&a.name)).filter(|m| m.ncomp == g.ncomp && m.len() == g.len() && m.data.to_f64_vec() == g.data.to_f64_vec());
            res.insert(match same {
                Some(m) => m.clone(),
                None => g,
            });
        }
        res
    };
    let pd = pick(gx.point_data(), own.point_data(), &pts);
    let cd = pick(gx.cell_data(), own.cell_data(), &cells);
    if let Some(p) = out.point_data_mut() {
        *p = pd;
    }
    if let Some(c) = out.cell_data_mut() {
        *c = cd;
    }
    Ok(out)
}

/// Filters run by the processes of a parallel server around a neighbour filter. Not listed in
/// menus or the catalog; found by id only.
pub fn internal_defs() -> Vec<FilterDef> {
    let layers = || crate::registry::i("layers", "Layers", 1, 1, 8);
    vec![
        FilterDef {
            id: "__piece_border",
            label: "Piece Border",
            category: "Internal",
            description: "The cells of a piece near its border (for ghost layers between processes).",
            keywords: &[],
            is_source: false,
            ext: None,
            temporal: None,
            multi: None,
            params: vec![layers()],
            apply: |input, p| piece_border(input.unwrap(), p.i64("layers", 1) as usize),
        },
        FilterDef {
            id: "__append_ghosts",
            label: "Append Ghosts",
            category: "Internal",
            description: "A piece with the neighbouring pieces' cells next to it as ghosts.",
            keywords: &[],
            is_source: false,
            ext: None,
            temporal: None,
            multi: None,
            params: vec![layers(), crate::registry::s("key", "Key", "")],
            apply: |input, p| {
                let key = p.str("key");
                let cands = fetch(&key).ok_or_else(|| anyhow!("{NOT_SENT} ({key}); try again"))?;
                append_ghosts(input.unwrap(), &cands, p.i64("layers", 1) as usize)
            },
        },
        FilterDef {
            id: "__drop_ghosts",
            label: "Drop Ghosts",
            category: "Internal",
            description: "A filter's result on a piece with ghosts, back on the piece.",
            keywords: &[],
            is_source: false,
            ext: None,
            temporal: None,
            multi: Some(MultiInput { label: "Piece", min: 2, max: Some(2), apply: |inputs, _| drop_ghosts(inputs[0], inputs[1]) }),
            params: vec![],
            apply: |_, _| Err(anyhow!("needs the piece as its second input")),
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Run `id` on every piece of `whole` with ghosts from the others, and compare each point
    /// with the filter run on the whole: the largest relative difference.
    fn worst(whole: &DataSet, n: usize, id: &str, set: &[(&str, crate::ParamValue)]) -> f64 {
        let def = crate::find(id).unwrap();
        let mut p = def.default_params();
        for (k, v) in set {
            p.0.insert(k.to_string(), v.clone());
        }
        let want = def.run(Some(whole), &p).unwrap();
        let pieces: Vec<DataSet> = (0..n).map(|k| aurafx_data::pieces::split(whole, k, n)).collect();
        let borders: Vec<DataSet> = pieces.iter().map(|d| piece_border(d, 1).unwrap()).collect();
        let wu = want.to_unstructured().unwrap();
        let mut at: HashMap<(String, [i64; 3]), Vec<f64>> = HashMap::new();
        for a in wu.point_data.iter() {
            for i in 0..wu.points.len() {
                at.insert((a.name.clone(), wu.points.get(i).map(|v| (v * 1e9).round() as i64)), (0..a.ncomp).map(|c| a.get(i, c)).collect());
            }
        }
        let mut worst = 0.0f64;
        for k in 0..n {
            let mut mb = MultiBlock::default();
            for (j, b) in borders.iter().enumerate() {
                if j != k {
                    mb.blocks.push((format!("piece {j}"), Some(b.clone())));
                }
            }
            let g = append_ghosts(&pieces[k], &DataSet::MultiBlock(mb), 1).unwrap();
            assert!(g.n_cells() > pieces[k].n_cells(), "piece {k} got ghosts");
            let r = drop_ghosts(&def.run(Some(&g), &p).unwrap(), &pieces[k]).unwrap();
            assert_eq!((r.n_points(), r.n_cells()), (pieces[k].n_points(), pieces[k].n_cells()));
            let ru = r.to_unstructured().unwrap();
            for a in ru.point_data.iter() {
                for i in 0..ru.points.len() {
                    let w = &at[&(a.name.clone(), ru.points.get(i).map(|v| (v * 1e9).round() as i64))];
                    for c in 0..a.ncomp {
                        worst = worst.max((a.get(i, c) - w[c]).abs() / w[c].abs().max(1.0));
                    }
                }
            }
        }
        worst
    }

    #[test]
    fn pieces_with_ghosts_match_the_whole() {
        let src = crate::find("wavelet").unwrap();
        let img = src.run(None, &src.default_params()).unwrap();
        let rt = crate::ParamValue::Array { assoc: crate::Association::Points, name: "RTData".into() };
        assert!(worst(&img, 4, "gradient", &[("array", rt.clone())]) < 1e-9, "image gradient");
        let ug = DataSet::UnstructuredGrid(img.to_unstructured().unwrap());
        assert!(worst(&ug, 4, "gradient", &[("array", rt)]) < 1e-9, "mesh gradient");
        let pc = crate::find("point_data_to_cell_data").unwrap();
        let cells = pc.run(Some(&ug), &pc.default_params()).unwrap();
        assert!(worst(&cells, 3, "cell_data_to_point_data", &[]) < 1e-9, "cell to point");
    }

    fn wavelet_mesh() -> UnstructuredGrid {
        let src = crate::find("wavelet").unwrap();
        src.run(None, &src.default_params()).unwrap().to_unstructured().unwrap()
    }

    fn moved(u: &UnstructuredGrid, by: f64) -> UnstructuredGrid {
        let mut u = u.clone();
        u.points = Points::new_f64(u.points.data.to_f64_vec().chunks(3).flat_map(|p| [p[0] + by, p[1], p[2]]).collect());
        u
    }

    /// A small part far from the origin: its points are still told apart.
    #[test]
    fn far_from_the_origin() {
        let ug = DataSet::UnstructuredGrid(moved(&wavelet_mesh(), 2e9));
        let rt = crate::ParamValue::Array { assoc: crate::Association::Points, name: "RTData".into() };
        assert!(worst(&ug, 4, "gradient", &[("array", rt)]) < 1e-6);
    }

    /// Text arrays and units come through; blocks with the same name are matched by position;
    /// polyhedra get no ghosts.
    #[test]
    fn text_units_blocks_and_polyhedra() {
        let mut u = wavelet_mesh();
        let rt = u.point_data.get("RTData").unwrap().clone().with_units("K");
        u.point_data.insert(rt);
        let n = u.points.len();
        u.point_data.insert(Array::new("label", 1, ArrayData::String((0..n).map(|i| format!("p{i}")).collect())));
        let whole = DataSet::UnstructuredGrid(u.clone());
        let pieces: Vec<DataSet> = (0..3).map(|k| aurafx_data::pieces::split(&whole, k, 3)).collect();
        let border = |skip: usize, of: &[DataSet]| {
            let mut mb = MultiBlock::default();
            for (j, p) in of.iter().enumerate().filter(|(j, _)| *j != skip) {
                mb.blocks.push((format!("piece {j}"), Some(piece_border(p, 1).unwrap())));
            }
            DataSet::MultiBlock(mb)
        };
        let g = append_ghosts(&pieces[1], &border(1, &pieces), 1).unwrap();
        assert!(g.n_cells() > pieces[1].n_cells());
        let labels = g.point_data().unwrap().get("label").expect("the text array is kept with ghosts");
        assert_eq!(labels.len(), g.n_points());
        let def = crate::find("gradient").unwrap();
        let mut p = def.default_params();
        p.0.insert("array".into(), crate::ParamValue::Array { assoc: crate::Association::Points, name: "RTData".into() });
        let r = drop_ghosts(&def.run(Some(&g), &p).unwrap(), &pieces[1]).unwrap();
        let (got, own) = (r.point_data().unwrap(), pieces[1].point_data().unwrap());
        assert_eq!(got.get("label"), own.get("label"), "text array");
        assert_eq!(got.get("RTData").unwrap().units.as_deref(), Some("K"), "units");

        // Two blocks without names, the second one elsewhere: each gets ghosts from its own.
        let mut mb = MultiBlock::default();
        mb.blocks.push((String::new(), Some(whole.clone())));
        mb.blocks.push((String::new(), Some(DataSet::UnstructuredGrid(moved(&u, 1000.0)))));
        let mb = DataSet::MultiBlock(mb);
        let parts: Vec<DataSet> = (0..3).map(|k| aurafx_data::pieces::split(&mb, k, 3)).collect();
        let g = append_ghosts(&parts[1], &border(1, &parts), 1).unwrap();
        let (DataSet::MultiBlock(g), DataSet::MultiBlock(own)) = (&g, &parts[1]) else { panic!() };
        for b in 0..2 {
            let (with, without) = (g.blocks[b].1.as_ref().unwrap(), own.blocks[b].1.as_ref().unwrap());
            assert!(with.n_cells() > without.n_cells(), "block {b} got ghosts");
            assert_eq!(with.n_cells(), g.blocks[0].1.as_ref().unwrap().n_cells(), "block {b}: as many as the first");
        }

        // Polyhedra: the piece is left alone.
        let DataSet::UnstructuredGrid(mut poly) = pieces[1].clone() else { panic!() };
        poly.cells.polyhedron_faces = Some(vec![Vec::new(); poly.cells.len()]);
        let poly = DataSet::UnstructuredGrid(poly);
        assert_eq!(append_ghosts(&poly, &border(1, &pieces), 1).unwrap().n_cells(), poly.n_cells());
    }
}
