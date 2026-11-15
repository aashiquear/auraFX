//! Partitioned data: split a mesh into balanced partitions (Redistribute DataSet), add layers of
//! the neighbouring partitions' cells to each partition (Ghost Cells), and take ghost cells and
//! the ghost arrays away again (Remove Ghost Information).
//!
//! Partitions are the blocks of a multi-block (a `.pvtu` read with pieces as partitions, the
//! output of Redistribute DataSet). Ghost cells carry `vtkGhostType` = 1 (a duplicate of a cell
//! owned by another partition); points used only by ghost cells carry `vtkGhostType` = 1 too.

use anyhow::{anyhow, Result};
use aurafx_data::*;
use std::collections::HashMap;

pub const GHOST: &str = "vtkGhostType";

/// The partitions of `ds` as unstructured grids (one for data that is not a multi-block).
fn partitions(ds: &DataSet) -> Result<(Vec<String>, Vec<UnstructuredGrid>)> {
    let leaves: Vec<(String, &DataSet)> = match ds {
        DataSet::MultiBlock(mb) => mb.leaves(),
        d => vec![("partition 0".to_string(), d)],
    };
    let mut names = Vec::new();
    let mut out = Vec::new();
    for (n, l) in leaves {
        let u = l.to_unstructured().ok_or_else(|| anyhow!("{n}: this kind of data cannot be partitioned"))?;
        names.push(n);
        out.push(u);
    }
    if out.is_empty() {
        return Err(anyhow!("no partitions"));
    }
    Ok((names, out))
}

/// Gather array values from several attribute sets: (set, index) per output tuple. Only arrays
/// present in every set with the same number of components are kept.
fn gather_multi(sets: &[&Attributes], picks: &[(usize, usize)]) -> Attributes {
    let mut out = Attributes::new();
    let Some(first) = sets.first() else { return out };
    for a in first.iter().filter(|a| a.scalar_type() != ScalarType::String) {
        let all: Option<Vec<&Array>> = sets.iter().map(|s| s.get(&a.name).filter(|b| b.ncomp == a.ncomp && b.scalar_type() != ScalarType::String)).collect();
        let Some(all) = all else { continue };
        let nc = a.ncomp;
        let mut v = Vec::with_capacity(picks.len() * nc);
        for &(s, i) in picks {
            for c in 0..nc {
                v.push(all[s].get(i, c));
            }
        }
        let mut arr = Array::new(a.name.clone(), nc, ArrayData::F64(v));
        arr.component_names = a.component_names.clone();
        out.insert(arr);
    }
    out
}

/// Remove Ghost Information: drop ghost cells (and points only they used) and the ghost arrays.
pub fn remove_ghosts(ds: &DataSet) -> Result<DataSet> {
    crate::registry::per_leaf(ds, &|leaf| {
        let Some(g) = leaf.cell_data().and_then(|c| c.get(GHOST)).cloned() else {
            let mut out = leaf.clone();
            if let Some(pd) = out.point_data_mut() {
                pd.remove(GHOST);
            }
            return Ok(out);
        };
        let u = leaf.to_unstructured().ok_or_else(|| anyhow!("unsupported data"))?;
        let keep: Vec<usize> = (0..u.cells.len()).filter(|&c| g.get(c, 0) == 0.0).collect();
        let mut out = subset(&u, &keep);
        out.cell_data.remove(GHOST);
        out.point_data.remove(GHOST);
        Ok(DataSet::UnstructuredGrid(out))
    })
}

/// The cells `keep` of `u`, with only the points they use.
pub fn subset(u: &UnstructuredGrid, keep: &[usize]) -> UnstructuredGrid {
    let mut map: HashMap<u32, u32> = HashMap::new();
    let mut used: Vec<usize> = Vec::new();
    let mut cells = Cells::new();
    for &c in keep {
        let ids: Vec<u32> = u
            .cells
            .cell(c)
            .iter()
            .map(|&p| {
                *map.entry(p).or_insert_with(|| {
                    used.push(p as usize);
                    (used.len() - 1) as u32
                })
            })
            .collect();
        cells.push(u.cells.cell_type(c), &ids);
    }
    UnstructuredGrid {
        points: u.points.gather(&used),
        cells,
        point_data: u.point_data.gather(&used),
        cell_data: u.cell_data.gather(keep),
        field_data: u.field_data.clone(),
    }
}

fn centre(u: &UnstructuredGrid, c: usize) -> [f64; 3] {
    let ids = u.cells.cell(c);
    let mut s = [0.0; 3];
    for &i in ids {
        let p = u.points.get(i as usize);
        for a in 0..3 {
            s[a] += p[a];
        }
    }
    s.map(|v| v / ids.len().max(1) as f64)
}

/// Redistribute DataSet: `n` partitions of nearly equal cell counts, cut by recursive bisection
/// of the cell centres along the longest side; `ghost_layers` layers of ghost cells added.
pub fn redistribute(ds: &DataSet, n: usize, ghost_layers: usize) -> Result<DataSet> {
    let (_, parts) = partitions(ds)?;
    // One mesh first.
    let u = if parts.len() == 1 {
        parts.into_iter().next().unwrap()
    } else {
        let refs: Vec<DataSet> = parts.into_iter().map(DataSet::UnstructuredGrid).collect();
        let r: Vec<&DataSet> = refs.iter().collect();
        crate::append::append_datasets(&r)?
    };
    let centres: Vec<[f64; 3]> = (0..u.cells.len()).map(|c| centre(&u, c)).collect();
    let mut groups: Vec<Vec<usize>> = Vec::new();
    bisect((0..u.cells.len()).collect(), n.max(1), &centres, &mut groups);
    let mut mb = MultiBlock::default();
    for (k, g) in groups.iter().enumerate() {
        let mut part = subset(&u, g);
        part.field_data.insert(Array::scalars_f64("PartitionId", vec![k as f64]));
        // Also per cell, to color by (ghost cells added later keep their own partition's number).
        part.cell_data.insert(Array::scalars_f64("PartitionId", vec![k as f64; g.len()]));
        mb.blocks.push((format!("partition {k}"), Some(DataSet::UnstructuredGrid(part))));
    }
    let out = DataSet::MultiBlock(mb);
    if ghost_layers > 0 {
        ghost_cells(&out, ghost_layers)
    } else {
        Ok(out)
    }
}

fn bisect(cells: Vec<usize>, n: usize, centres: &[[f64; 3]], out: &mut Vec<Vec<usize>>) {
    if n <= 1 || cells.len() <= 1 {
        out.push(cells);
        for _ in 1..n {
            out.push(Vec::new());
        }
        return;
    }
    let mut lo = [f64::INFINITY; 3];
    let mut hi = [f64::NEG_INFINITY; 3];
    for &c in &cells {
        for a in 0..3 {
            lo[a] = lo[a].min(centres[c][a]);
            hi[a] = hi[a].max(centres[c][a]);
        }
    }
    let ax = (0..3).max_by(|&a, &b| (hi[a] - lo[a]).total_cmp(&(hi[b] - lo[b]))).unwrap();
    let left_n = n / 2;
    let split = cells.len() * left_n / n;
    let mut cells = cells;
    // Sort by the axis, ties by cell number, so the cut is the same every time.
    cells.sort_by(|&a, &b| centres[a][ax].total_cmp(&centres[b][ax]).then(a.cmp(&b)));
    let right = cells.split_off(split);
    bisect(cells, left_n, centres, out);
    bisect(right, n - left_n, centres, out);
}

/// Ghost Cells: each partition gains `layers` layers of the cells of other partitions that touch
/// it (sharing a point; points matched by position), marked in `vtkGhostType`.
pub fn ghost_cells(ds: &DataSet, layers: usize) -> Result<DataSet> {
    let (names, parts) = partitions(ds)?;
    let b = ds.bounds();
    let tol = (b.diagonal() * 1e-9).max(1e-300);
    // Points shared between partitions get one global number.
    let mut gid: HashMap<[i64; 3], usize> = HashMap::new();
    let mut local_to_global: Vec<Vec<usize>> = Vec::new();
    let mut rep: Vec<(usize, usize)> = Vec::new();
    for (p, u) in parts.iter().enumerate() {
        let mut m = Vec::with_capacity(u.points.len());
        for i in 0..u.points.len() {
            let q = u.points.get(i);
            let k = [(q[0] / tol).round() as i64, (q[1] / tol).round() as i64, (q[2] / tol).round() as i64];
            let next = gid.len();
            let g = *gid.entry(k).or_insert(next);
            if g == rep.len() {
                rep.push((p, i));
            }
            m.push(g);
        }
        local_to_global.push(m);
    }
    let mut point_cells: Vec<Vec<(usize, usize)>> = vec![Vec::new(); rep.len()];
    for (p, u) in parts.iter().enumerate() {
        for c in 0..u.cells.len() {
            for &i in u.cells.cell(c) {
                point_cells[local_to_global[p][i as usize]].push((p, c));
            }
        }
    }
    let cell_sets: Vec<&Attributes> = parts.iter().map(|u| &u.cell_data).collect();
    let point_sets: Vec<&Attributes> = parts.iter().map(|u| &u.point_data).collect();
    let mut mb = MultiBlock { field_data: ds.field_data().clone(), ..Default::default() };
    for (p, u) in parts.iter().enumerate() {
        let mut picked: Vec<(usize, usize)> = (0..u.cells.len()).map(|c| (p, c)).collect();
        let mut seen: std::collections::HashSet<(usize, usize)> = picked.iter().copied().collect();
        let mut frontier: Vec<usize> = local_to_global[p].clone();
        for _ in 0..layers {
            let mut next = Vec::new();
            for &g in &frontier {
                for &(q, c) in &point_cells[g] {
                    if q != p && seen.insert((q, c)) {
                        picked.push((q, c));
                        next.extend(parts[q].cells.cell(c).iter().map(|&i| local_to_global[q][i as usize]));
                    }
                }
            }
            next.sort_unstable();
            next.dedup();
            frontier = next;
        }
        // Points of the partition and its ghosts, numbered in order of use.
        let mut map: HashMap<usize, u32> = HashMap::new();
        let mut pts: Vec<usize> = Vec::new();
        let mut cells = Cells::new();
        for &(q, c) in &picked {
            let ids: Vec<u32> = parts[q]
                .cells
                .cell(c)
                .iter()
                .map(|&i| {
                    let g = local_to_global[q][i as usize];
                    *map.entry(g).or_insert_with(|| {
                        pts.push(g);
                        (pts.len() - 1) as u32
                    })
                })
                .collect();
            cells.push(parts[q].cells.cell_type(c), &ids);
        }
        let owned_pts: std::collections::HashSet<usize> = local_to_global[p].iter().copied().collect();
        let mut points = Points::empty();
        for &g in &pts {
            let (q, i) = rep[g];
            points.push(parts[q].points.get(i));
        }
        let mut point_data = gather_multi(&point_sets, &pts.iter().map(|&g| rep[g]).collect::<Vec<_>>());
        let mut cell_data = gather_multi(&cell_sets, &picked);
        cell_data.insert(Array::new(GHOST, 1, ArrayData::U8(picked.iter().map(|&(q, _)| u8::from(q != p)).collect())));
        point_data.insert(Array::new(GHOST, 1, ArrayData::U8(pts.iter().map(|g| u8::from(!owned_pts.contains(g))).collect())));
        let out = UnstructuredGrid { points, cells, point_data, cell_data, field_data: u.field_data.clone() };
        mb.blocks.push((names[p].clone(), Some(DataSet::UnstructuredGrid(out))));
    }
    Ok(DataSet::MultiBlock(mb))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn box_mesh(n: usize) -> DataSet {
        let mut im = ImageData { extent: [0, n as i64, 0, n as i64, 0, n as i64], ..Default::default() };
        im.cell_data.insert(Array::scalars_f64("id", (0..n * n * n).map(|i| i as f64).collect()));
        DataSet::ImageData(im)
    }

    fn ghosts(u: &DataSet) -> (usize, usize) {
        let g = u.cell_data().unwrap().get(GHOST).unwrap();
        let n = u.n_cells();
        (n, (0..n).filter(|&i| g.get(i, 0) == 1.0).count())
    }

    #[test]
    fn split_ghost_and_remove() {
        let ds = box_mesh(4);
        let DataSet::MultiBlock(mb) = redistribute(&ds, 2, 0).unwrap() else { panic!() };
        assert_eq!(mb.blocks.len(), 2);
        let sizes: Vec<usize> = mb.blocks.iter().map(|b| b.1.as_ref().unwrap().n_cells()).collect();
        assert_eq!(sizes, vec![32, 32]);
        // One layer: the 16 cells across the cut; two layers: 32.
        let g1 = ghost_cells(&DataSet::MultiBlock(mb.clone()), 1).unwrap();
        let DataSet::MultiBlock(g1) = g1 else { panic!() };
        for b in &g1.blocks {
            assert_eq!(ghosts(b.1.as_ref().unwrap()), (48, 16));
        }
        let DataSet::MultiBlock(g2) = ghost_cells(&DataSet::MultiBlock(mb.clone()), 2).unwrap() else { panic!() };
        assert_eq!(ghosts(g2.blocks[0].1.as_ref().unwrap()), (64, 32));
        // Cell arrays travel with the ghost cells.
        let p0 = g1.blocks[0].1.as_ref().unwrap();
        let ids = p0.cell_data().unwrap().get("id").unwrap();
        let mut all: Vec<i64> = (0..p0.n_cells()).map(|i| ids.get(i, 0) as i64).collect();
        all.sort_unstable();
        all.dedup();
        assert_eq!(all.len(), 48);
        // Removing the ghosts gives the partitions back.
        let DataSet::MultiBlock(r) = remove_ghosts(&DataSet::MultiBlock(g1)).unwrap() else { panic!() };
        assert_eq!(r.blocks[1].1.as_ref().unwrap().n_cells(), 32);
        assert!(r.blocks[1].1.as_ref().unwrap().cell_data().unwrap().get(GHOST).is_none());
        // Three partitions of 64 cells: 21, 21, 22.
        let DataSet::MultiBlock(m3) = redistribute(&ds, 3, 0).unwrap() else { panic!() };
        let mut s3: Vec<usize> = m3.blocks.iter().map(|b| b.1.as_ref().unwrap().n_cells()).collect();
        s3.sort();
        assert_eq!(s3, vec![21, 21, 22]);
    }
}
