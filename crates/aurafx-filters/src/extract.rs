//! Extracting parts of a dataset:
//!
//! - **Iso Volume**: the part where a point scalar lies between two values, cut through the cells
//!   at the interpolated positions of those values (two scalar clips).
//! - **Extract Cells By Region**: whole cells inside a plane's half-space, a box or a sphere (all
//!   their points inside, or any point inside: the cells the region boundary crosses too).
//! - **Extract Cells By Type**: whole cells of the chosen types or dimensions.
//! - **Clip Closed Surface**: a closed surface cut by a plane and closed again with a flat cap, so
//!   the result is still watertight.

use crate::interp::ScalarFn;
use crate::params::Association;
use crate::simplex::{as_explicit, parts};
use anyhow::{anyhow, bail, Result};
use aurafx_data::*;

/// The cells for which `keep` holds, with their points compacted and their data carried over.
fn select_cells(ds: &DataSet, keep: &(dyn Fn(usize, CellType, &[u32]) -> bool + Sync)) -> Result<UnstructuredGrid> {
    let ds = as_explicit(ds);
    let (points, pd, cd, fd) = parts(&ds).ok_or_else(|| anyhow!("unsupported dataset"))?;
    let chunks = crate::interp::par_cell_chunks(
        &ds,
        || (Cells::new(), Vec::<usize>::new()),
        |(cells, parents), cid, ty, pts, _| {
            if keep(cid, ty, pts) {
                cells.push(ty, pts);
                parents.push(cid);
            }
        },
    );
    let mut cells = Cells::new();
    let mut parents = Vec::new();
    for (c, p) in chunks {
        cells.append(&c, 0);
        parents.extend(p);
    }
    let (points, point_data) = crate::interp::compact_points(points, pd, &mut cells.connectivity);
    Ok(UnstructuredGrid { points, cells, point_data, cell_data: cd.gather(&parents), field_data: fd.clone() })
}

/// The part where `array` lies in [lo, hi], cut at the interpolated positions of lo and hi.
pub fn iso_volume(ds: &DataSet, assoc: Association, array: &str, comp: Option<usize>, lo: f64, hi: f64) -> Result<UnstructuredGrid> {
    if lo > hi {
        bail!("Iso Volume: the lower value {lo} is above the upper value {hi}");
    }
    let above = crate::clip::clip_scalar(ds, assoc, array, comp, lo, false)?;
    // The first clip carries the point array over (interpolated on the cut), so the second reads it as point data.
    crate::clip::clip_scalar(&DataSet::UnstructuredGrid(above), Association::Points, array, comp, hi, true)
}

/// How a cell counts as in a region.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RegionMode {
    /// Every point of the cell inside.
    AllInside,
    /// At least one point inside: the cells the region's boundary crosses are kept too.
    Intersecting,
}

/// The region as a function of the points, negative inside: a plane (the side its normal points
/// away from), a box or a sphere.
pub enum Region {
    Plane { origin: [f64; 3], normal: [f64; 3] },
    Box { min: [f64; 3], max: [f64; 3] },
    Sphere { center: [f64; 3], radius: f64 },
}

pub fn cells_by_region(ds: &DataSet, region: &Region, mode: RegionMode, invert: bool) -> Result<UnstructuredGrid> {
    let exp = as_explicit(ds);
    let (points, ..) = parts(&exp).ok_or_else(|| anyhow!("Extract Cells By Region: unsupported dataset"))?;
    let f = match region {
        // Plane: inside is the side the normal points to (as Clip keeps it).
        Region::Plane { origin, normal } => ScalarFn::Plane { origin: *origin, normal: crate::slice::normalize(*normal).map(|x| -x), points },
        Region::Box { min, max } => ScalarFn::Box { min: *min, max: *max, points },
        Region::Sphere { center, radius } => ScalarFn::Sphere { center: *center, radius: *radius, points },
    };
    let inside: Vec<bool> = {
        use rayon::prelude::*;
        (0..points.len()).into_par_iter().with_min_len(8192).map(|i| f.eval(i) <= 0.0).collect()
    };
    select_cells(&exp, &|_, _, pts| {
        let k = match mode {
            RegionMode::AllInside => pts.iter().all(|p| inside[*p as usize]),
            RegionMode::Intersecting => pts.iter().any(|p| inside[*p as usize]),
        };
        k != invert
    })
}

/// The cell types a list names: VTK type names ("Tetra", "Hexahedron", "QuadraticTetra", …, any
/// case, spaces ignored) or dimensions ("3D", "2D", "1D", "0D").
pub fn parse_types(list: &str) -> Result<(Vec<CellType>, Vec<u8>)> {
    let mut types = Vec::new();
    let mut dims = Vec::new();
    let all: Vec<CellType> = (0u8..=100).filter_map(CellType::from_vtk).collect();
    for word in list.split([',', ';']).map(|w| w.trim()).filter(|w| !w.is_empty()) {
        let w = word.to_ascii_lowercase().replace([' ', '_', '-'], "");
        if let Some(d) = w.strip_suffix('d').and_then(|d| d.parse::<u8>().ok()).filter(|d| *d <= 3) {
            dims.push(d);
            continue;
        }
        let t = all.iter().find(|t| t.name().to_ascii_lowercase().replace([' ', '_', '-'], "") == w).ok_or_else(|| {
            anyhow!("Extract Cells By Type: no cell type '{word}' (names such as Tetra, Hexahedron, Wedge, Pyramid, Triangle, Quad, Line, Vertex, or 3D, 2D, 1D, 0D)")
        })?;
        types.push(*t);
    }
    if types.is_empty() && dims.is_empty() {
        bail!("Extract Cells By Type: name at least one cell type or dimension");
    }
    Ok((types, dims))
}

pub fn cells_by_type(ds: &DataSet, list: &str, invert: bool) -> Result<UnstructuredGrid> {
    let (types, dims) = parse_types(list)?;
    select_cells(ds, &|_, ty, _| (types.contains(&ty) || dims.contains(&ty.dimension())) != invert)
}

// ------------------------------------------------------------------ the capped clip

/// Orientation of a 2-D triangle (twice its signed area).
fn cross2(o: [f64; 2], a: [f64; 2], b: [f64; 2]) -> f64 {
    (a[0] - o[0]) * (b[1] - o[1]) - (a[1] - o[1]) * (b[0] - o[0])
}

fn area2(poly: &[[f64; 2]]) -> f64 {
    (0..poly.len())
        .map(|i| {
            let (a, b) = (poly[i], poly[(i + 1) % poly.len()]);
            a[0] * b[1] - b[0] * a[1]
        })
        .sum::<f64>()
}

fn inside_poly(p: [f64; 2], poly: &[[f64; 2]]) -> bool {
    let mut c = false;
    for i in 0..poly.len() {
        let (a, b) = (poly[i], poly[(i + 1) % poly.len()]);
        if (a[1] > p[1]) != (b[1] > p[1]) && p[0] < a[0] + (p[1] - a[1]) / (b[1] - a[1]) * (b[0] - a[0]) {
            c = !c;
        }
    }
    c
}

fn in_triangle(p: [f64; 2], a: [f64; 2], b: [f64; 2], c: [f64; 2]) -> bool {
    let (d1, d2, d3) = (cross2(a, b, p), cross2(b, c, p), cross2(c, a, p));
    !((d1 < 0.0 || d2 < 0.0 || d3 < 0.0) && (d1 > 0.0 || d2 > 0.0 || d3 > 0.0))
}

/// Join holes (clockwise) into an outer polygon (counter-clockwise) by bridges from each hole's
/// rightmost vertex to a visible vertex of the outer one (the classic method of ear clipping
/// with holes). Indices refer to `xy`.
fn bridge_holes(outer: Vec<usize>, mut holes: Vec<Vec<usize>>, xy: &[[f64; 2]]) -> Vec<usize> {
    let mut poly = outer;
    holes.sort_by(|a, b| {
        let mx = |h: &Vec<usize>| h.iter().map(|i| xy[*i][0]).fold(f64::NEG_INFINITY, f64::max);
        mx(b).total_cmp(&mx(a))
    });
    for hole in holes {
        let (hk, &m) = hole.iter().enumerate().max_by(|a, b| xy[*a.1][0].total_cmp(&xy[*b.1][0])).unwrap();
        let mp = xy[m];
        // The nearest edge of the polygon crossed by a ray from M along +x.
        let mut best: Option<(f64, usize)> = None;
        for i in 0..poly.len() {
            let (a, b) = (xy[poly[i]], xy[poly[(i + 1) % poly.len()]]);
            if (a[1] > mp[1]) != (b[1] > mp[1]) || a[1] == mp[1] && b[1] == mp[1] {
                if a[1] == b[1] {
                    continue;
                }
                let x = a[0] + (mp[1] - a[1]) / (b[1] - a[1]) * (b[0] - a[0]);
                if x >= mp[0] && best.is_none_or(|(bx, _)| x < bx) {
                    // The endpoint with the larger x of the crossed edge.
                    let k = if a[0] > b[0] { i } else { (i + 1) % poly.len() };
                    best = Some((x, k));
                }
            }
        }
        let Some((ix, mut k)) = best else { continue };
        // A reflex vertex inside the triangle (M, I, P) is the visible one instead (smallest angle).
        let (ip, p) = ([ix, mp[1]], xy[poly[k]]);
        let mut best_angle = f64::INFINITY;
        for (j, &v) in poly.iter().enumerate() {
            let q = xy[v];
            if v == poly[k] || !in_triangle(q, mp, ip, p) {
                continue;
            }
            let prev = xy[poly[(j + poly.len() - 1) % poly.len()]];
            let next = xy[poly[(j + 1) % poly.len()]];
            if cross2(prev, q, next) < 0.0 {
                let ang = (q[1] - mp[1]).abs().atan2(q[0] - mp[0]);
                if ang < best_angle {
                    best_angle = ang;
                    k = j;
                }
            }
        }
        // poly[..=k], the hole from M around back to M, then poly[k] again and the rest.
        let mut merged: Vec<usize> = poly[..=k].to_vec();
        merged.extend((0..=hole.len()).map(|t| hole[(hk + t) % hole.len()]));
        merged.extend(&poly[k..]);
        poly = merged;
    }
    poly
}

/// Ear clipping of a simple counter-clockwise polygon (possibly with bridge edges).
fn ear_clip(poly: &[usize], xy: &[[f64; 2]]) -> Vec<[usize; 3]> {
    let mut v: Vec<usize> = poly.to_vec();
    let mut out = Vec::new();
    let mut guard = 0;
    while v.len() > 3 && guard < 10 * poly.len() * poly.len() + 100 {
        guard += 1;
        let n = v.len();
        let mut clipped = false;
        for i in 0..n {
            let (a, b, c) = (v[(i + n - 1) % n], v[i], v[(i + 1) % n]);
            if cross2(xy[a], xy[b], xy[c]) <= 1e-18 {
                continue;
            }
            let blocked =
                v.iter().any(|&q| q != a && q != b && q != c && xy[q] != xy[a] && xy[q] != xy[b] && xy[q] != xy[c] && in_triangle(xy[q], xy[a], xy[b], xy[c]));
            if !blocked {
                out.push([a, b, c]);
                v.remove(i);
                clipped = true;
                break;
            }
        }
        if !clipped {
            // Degenerate rest (collinear points): fan it.
            for i in 1..v.len() - 1 {
                out.push([v[0], v[i], v[i + 1]]);
            }
            return out;
        }
    }
    if v.len() == 3 {
        out.push([v[0], v[1], v[2]]);
    }
    out
}

/// Merge points closer than `tol` (a cut through existing vertices makes a new point on top of each)
/// and drop the polygons that collapse; the first point's data is kept.
fn weld(pd: &mut PolyData, tol: f64) {
    let n = pd.points.len();
    let key = |p: [f64; 3]| -> [i64; 3] { p.map(|x| (x / tol).round() as i64) };
    let mut first: std::collections::HashMap<[i64; 3], u32> = std::collections::HashMap::new();
    let mut map = vec![0u32; n];
    let mut keep: Vec<usize> = Vec::new();
    for i in 0..n {
        let k = key(pd.points.get(i));
        let id = *first.entry(k).or_insert_with(|| {
            keep.push(i);
            (keep.len() - 1) as u32
        });
        map[i] = id;
    }
    if keep.len() == n {
        return;
    }
    let mut polys = Cells::new();
    let mut parents = Vec::new();
    for (c, (t, pts)) in pd.polys.iter().enumerate() {
        let mut v: Vec<u32> = pts.iter().map(|p| map[*p as usize]).collect();
        v.dedup();
        while v.len() > 1 && v.first() == v.last() {
            v.pop();
        }
        if v.len() >= 3 {
            polys.push(
                if v.len() == 3 {
                    CellType::Triangle
                } else if v.len() == 4 && t == CellType::Quad {
                    CellType::Quad
                } else {
                    CellType::Polygon
                },
                &v,
            );
            parents.push(c);
        }
    }
    pd.points = pd.points.gather(&keep);
    pd.point_data = pd.point_data.gather(&keep);
    pd.cell_data = pd.cell_data.gather(&parents);
    pd.polys = polys;
}

/// Make the winding of a surface's polygons coherent (neighbours run each shared edge in opposite
/// directions) by flipping polygons, region by region, from the first polygon of each.
fn orient_coherently(polys: &mut Cells) {
    let n = polys.len();
    let cells: Vec<Vec<u32>> = polys.iter().map(|(_, c)| c.to_vec()).collect();
    let types: Vec<CellType> = polys.iter().map(|(t, _)| t).collect();
    let mut by_edge: std::collections::HashMap<(u32, u32), Vec<usize>> = std::collections::HashMap::new();
    for (i, c) in cells.iter().enumerate() {
        for k in 0..c.len() {
            let (a, b) = (c[k], c[(k + 1) % c.len()]);
            by_edge.entry((a.min(b), a.max(b))).or_default().push(i);
        }
    }
    let mut flip = vec![false; n];
    let mut seen = vec![false; n];
    let runs = |c: &Vec<u32>, f: bool, a: u32, b: u32| -> bool {
        // Does polygon c (flipped when f) run from a to b?
        let l = c.len();
        (0..l).any(|k| {
            let (x, y) = (c[k], c[(k + 1) % l]);
            if f {
                (y, x) == (a, b)
            } else {
                (x, y) == (a, b)
            }
        })
    };
    for start in 0..n {
        if seen[start] {
            continue;
        }
        seen[start] = true;
        let mut todo = vec![start];
        while let Some(i) = todo.pop() {
            let c = &cells[i];
            for k in 0..c.len() {
                let (a, b) = if flip[i] { (c[(k + 1) % c.len()], c[k]) } else { (c[k], c[(k + 1) % c.len()]) };
                for &j in &by_edge[&(a.min(b), a.max(b))] {
                    if j == i || seen[j] {
                        continue;
                    }
                    seen[j] = true;
                    // j must run b -> a.
                    flip[j] = runs(&cells[j], false, a, b);
                    todo.push(j);
                }
            }
        }
    }
    let mut out = Cells::new();
    for (i, c) in cells.iter().enumerate() {
        if flip[i] {
            let r: Vec<u32> = c.iter().rev().copied().collect();
            out.push(types[i], &r);
        } else {
            out.push(types[i], c);
        }
    }
    *polys = out;
}

/// A closed surface cut by a plane, keeping the side the normal points to (the other with
/// `invert`), and closed with flat caps on the plane. The cap triangles get the cell array `Cap` = 1.
pub fn clip_closed_surface(ds: &DataSet, origin: [f64; 3], normal: [f64; 3], invert: bool) -> Result<PolyData> {
    let tri = crate::geometry::triangulate(ds)?;
    let n = crate::slice::normalize(if invert { normal.map(|x| -x) } else { normal });
    let cut = crate::clip::clip_plane(&DataSet::PolyData(tri), origin, n, false)?;
    if cut.cells.types.iter().any(|t| t.dimension() != 2) {
        bail!("Clip Closed Surface works on a closed surface (polygons only)");
    }
    let mut pd = PolyData {
        points: cut.points,
        polys: cut.cells,
        point_data: cut.point_data,
        cell_data: cut.cell_data,
        field_data: cut.field_data,
        ..Default::default()
    };
    weld(&mut pd, 1e-9 * ds.bounds().diagonal().max(1e-12));
    orient_coherently(&mut pd.polys);
    let pts: Vec<[f64; 3]> = (0..pd.points.len()).map(|i| pd.points.get(i)).collect();
    let size = ds.bounds().diagonal().max(1e-12);
    let on_plane = |i: u32| -> bool {
        let p = pts[i as usize];
        ((p[0] - origin[0]) * n[0] + (p[1] - origin[1]) * n[1] + (p[2] - origin[2]) * n[2]).abs() < 1e-7 * size
    };
    // Boundary edges (used by one triangle) that lie on the plane.
    let mut count: std::collections::HashMap<(u32, u32), i32> = std::collections::HashMap::new();
    for (_, c) in pd.polys.iter() {
        for k in 0..c.len() {
            let (a, b) = (c[k], c[(k + 1) % c.len()]);
            *count.entry((a.min(b), a.max(b))).or_insert(0) += 1;
        }
    }
    let mut next: std::collections::HashMap<u32, Vec<u32>> = std::collections::HashMap::new();
    for (&(a, b), &k) in &count {
        if k == 1 && on_plane(a) && on_plane(b) {
            next.entry(a).or_default().push(b);
            next.entry(b).or_default().push(a);
        }
    }
    // The surface's own direction along its boundary edges.
    let surface_dir: std::collections::HashSet<(u32, u32)> = pd
        .polys
        .iter()
        .flat_map(|(_, c)| (0..c.len()).map(move |k| (c[k], c[(k + 1) % c.len()])))
        .filter(|(a, b)| count.get(&(*a.min(b), *a.max(b))) == Some(&1))
        .collect();
    // Chain the edges into closed loops.
    let mut used: std::collections::HashSet<(u32, u32)> = std::collections::HashSet::new();
    let mut loops: Vec<Vec<u32>> = Vec::new();
    let mut starts: Vec<u32> = next.keys().copied().collect();
    starts.sort_unstable();
    for s in starts {
        for &first in next[&s].clone().iter() {
            if used.contains(&(s.min(first), s.max(first))) {
                continue;
            }
            let mut lp = vec![s];
            let (mut prev, mut cur) = (s, first);
            used.insert((s.min(first), s.max(first)));
            while cur != s && lp.len() <= next.len() + 1 {
                lp.push(cur);
                let Some(nx) = next[&cur].iter().copied().find(|x| *x != prev && !used.contains(&(cur.min(*x), cur.max(*x)))) else { break };
                used.insert((cur.min(nx), cur.max(nx)));
                prev = cur;
                cur = nx;
            }
            if cur == s && lp.len() >= 3 {
                loops.push(lp);
            }
        }
    }
    // Plane coordinates in which the caps are triangulated (counter-clockwise outer loops).
    let helper = if n[0].abs() < 0.9 { [1.0, 0.0, 0.0] } else { [0.0, 1.0, 0.0] };
    let cr = |a: [f64; 3], b: [f64; 3]| [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]];
    let mn = n.map(|x| -x);
    let u = crate::slice::normalize(cr(helper, mn));
    let v = cr(mn, u);
    let mut xy = vec![[0.0f64; 2]; pts.len()];
    for lp in &loops {
        for &i in lp {
            let p = pts[i as usize];
            let d = [p[0] - origin[0], p[1] - origin[1], p[2] - origin[2]];
            xy[i as usize] = [d[0] * u[0] + d[1] * u[1] + d[2] * u[2], d[0] * v[0] + d[1] * v[1] + d[2] * v[2]];
        }
    }
    // Outer loops (inside an even number of others) counter-clockwise, holes clockwise.
    let poly2 = |lp: &Vec<u32>| lp.iter().map(|i| xy[*i as usize]).collect::<Vec<_>>();
    let depth: Vec<usize> = loops.iter().map(|l| loops.iter().filter(|o| !std::ptr::eq(*o, l) && inside_poly(xy[l[0] as usize], &poly2(o))).count()).collect();
    let mut caps: Vec<[usize; 3]> = Vec::new();
    for (i, lp) in loops.iter().enumerate().filter(|(i, _)| depth[*i] % 2 == 0) {
        let mut outer: Vec<usize> = lp.iter().map(|x| *x as usize).collect();
        if area2(&poly2(lp)) < 0.0 {
            outer.reverse();
        }
        let holes: Vec<Vec<usize>> = loops
            .iter()
            .enumerate()
            .filter(|(j, h)| depth[*j] == depth[i] + 1 && inside_poly(xy[h[0] as usize], &poly2(lp)))
            .map(|(_, h)| {
                let mut h: Vec<usize> = h.iter().map(|x| *x as usize).collect();
                if area2(&h.iter().map(|k| xy[*k]).collect::<Vec<_>>()) > 0.0 {
                    h.reverse();
                }
                h
            })
            .collect();
        let merged = bridge_holes(outer, holes, &xy);
        let mut group = ear_clip(&merged, &xy);
        // The cap follows the surface's winding: along the cut its edges run opposite to the surface's.
        let flip = group.iter().any(|t| (0..3).any(|k| surface_dir.contains(&(t[k] as u32, t[(k + 1) % 3] as u32))));
        if flip {
            for t in &mut group {
                t.swap(1, 2);
            }
        }
        caps.extend(group);
    }
    let n_surface = pd.polys.len();
    for t in &caps {
        pd.polys.push(CellType::Triangle, &[t[0] as u32, t[1] as u32, t[2] as u32]);
    }
    // Outward: the enclosed volume (divergence theorem over fans) is positive.
    let signed: f64 = pd
        .polys
        .iter()
        .map(|(_, c)| {
            (1..c.len() - 1)
                .map(|k| {
                    let (a, b, d) = (pts[c[0] as usize], pts[c[k] as usize], pts[c[k + 1] as usize]);
                    a[0] * (b[1] * d[2] - b[2] * d[1]) - a[1] * (b[0] * d[2] - b[2] * d[0]) + a[2] * (b[0] * d[1] - b[1] * d[0])
                })
                .sum::<f64>()
        })
        .sum();
    if signed < 0.0 {
        let cells: Vec<(CellType, Vec<u32>)> = pd.polys.iter().map(|(t, c)| (t, c.iter().rev().copied().collect())).collect();
        let mut out = Cells::new();
        for (t, c) in cells {
            out.push(t, &c);
        }
        pd.polys = out;
    }
    // Cell arrays: the surface's values, 0 on the caps; and Cap = 1 on the caps.
    let mut cd = Attributes::new();
    for a in pd.cell_data.iter() {
        let mut vals: Vec<f64> = (0..n_surface).flat_map(|c| (0..a.ncomp).map(move |k| a.get(c, k))).collect();
        vals.extend(std::iter::repeat_n(0.0, caps.len() * a.ncomp));
        cd.insert(Array::new(a.name.clone(), a.ncomp, ArrayData::F64(vals)));
    }
    cd.insert(Array::new("Cap", 1, ArrayData::I32((0..n_surface + caps.len()).map(|c| i32::from(c >= n_surface)).collect())));
    pd.cell_data = cd;
    Ok(pd)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Volume of a closed triangle surface (divergence theorem) and whether every edge has two triangles.
    fn closed_volume(pd: &PolyData) -> (f64, bool) {
        let p = |i: u32| pd.points.get(i as usize);
        let mut v = 0.0;
        let mut edges: std::collections::HashMap<(u32, u32), i32> = std::collections::HashMap::new();
        for (_, c) in pd.polys.iter() {
            // Polygons as fans of triangles for the volume; every polygon edge for watertightness.
            for k in 1..c.len() - 1 {
                let (a, b, cc) = (p(c[0]), p(c[k]), p(c[k + 1]));
                v += (a[0] * (b[1] * cc[2] - b[2] * cc[1]) - a[1] * (b[0] * cc[2] - b[2] * cc[0]) + a[2] * (b[0] * cc[1] - b[1] * cc[0])) / 6.0;
            }
            for k in 0..c.len() {
                let (x, y) = (c[k], c[(k + 1) % c.len()]);
                *edges.entry((x.min(y), x.max(y))).or_insert(0) += 1;
            }
        }
        (v, edges.values().all(|k| *k == 2))
    }

    fn cube() -> DataSet {
        DataSet::PolyData(crate::sources::cube([0.5; 3], [1.0; 3]))
    }

    /// A closed torus surface (quads) about z.
    fn torus(r: f64, a: f64, nu: usize, nv: usize) -> DataSet {
        let mut pts = Vec::new();
        for i in 0..nu {
            for j in 0..nv {
                let (u, v) = (std::f64::consts::TAU * i as f64 / nu as f64, std::f64::consts::TAU * j as f64 / nv as f64);
                pts.extend([(r + a * v.cos()) * u.cos(), (r + a * v.cos()) * u.sin(), a * v.sin()]);
            }
        }
        let mut polys = Cells::new();
        let id = |i: usize, j: usize| ((i % nu) * nv + (j % nv)) as u32;
        for i in 0..nu {
            for j in 0..nv {
                polys.push(CellType::Quad, &[id(i, j), id(i + 1, j), id(i + 1, j + 1), id(i, j + 1)]);
            }
        }
        DataSet::PolyData(PolyData { points: Points::new_f64(pts), polys, ..Default::default() })
    }

    fn grid(n: i64, h: f64) -> ImageData {
        ImageData {
            extent: [0, n, 0, n, 0, n],
            origin: [0.0; 3],
            spacing: [h; 3],
            direction: [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0],
            point_data: Attributes::new(),
            cell_data: Attributes::new(),
            field_data: Attributes::new(),
        }
    }

    /// The volume of the 3-D cells of an unstructured grid.
    fn cells_volume(u: &UnstructuredGrid) -> f64 {
        let mut v = 0.0;
        for (ty, pts) in u.cells.iter() {
            crate::simplex::decompose(&u.points, ty, pts, None, |s| {
                if let crate::simplex::Simplex::Tet(t) = s {
                    let p = t.map(|i| u.points.get(i as usize));
                    let e = |k: usize| [p[k][0] - p[0][0], p[k][1] - p[0][1], p[k][2] - p[0][2]];
                    let (a, b, c) = (e(1), e(2), e(3));
                    v += (a[0] * (b[1] * c[2] - b[2] * c[1]) - a[1] * (b[0] * c[2] - b[2] * c[0]) + a[2] * (b[0] * c[1] - b[1] * c[0])).abs() / 6.0;
                }
            });
        }
        v
    }

    #[test]
    fn a_capped_clip_of_a_cube_is_watertight_and_holds_half_its_volume() {
        let c = cube();
        let out = clip_closed_surface(&c, [0.5, 0.5, 0.37], [0.0, 0.0, 1.0], false).unwrap();
        let (v, closed) = closed_volume(&out);
        assert!(closed, "every edge has two triangles");
        assert!((v.abs() - 0.63).abs() < 1e-9, "volume {v}");
        let caps = out.cell_data.get("Cap").unwrap();
        assert!((0..caps.len()).any(|i| caps.get(i, 0) == 1.0));
        // An oblique plane through the center keeps exactly half.
        let out = clip_closed_surface(&c, [0.5, 0.5, 0.5], [1.0, 2.0, 0.5], true).unwrap();
        let (v, closed) = closed_volume(&out);
        assert!(closed && (v.abs() - 0.5).abs() < 1e-9, "{v} {closed}");
    }

    #[test]
    fn a_capped_clip_of_a_hollow_ring_keeps_the_hole_in_its_cap() {
        // A torus cut through its middle plane: the cap is an annulus (an outer loop and a hole).
        let t = torus(1.0, 0.3, 48, 24);
        let half = clip_closed_surface(&t, [0.0, 0.0, 0.0], [0.0, 0.0, 1.0], false).unwrap();
        let (v, closed) = closed_volume(&half);
        let (whole, _) = closed_volume(&crate::geometry::triangulate(&t).unwrap());
        assert!(closed, "watertight");
        assert!((v.abs() - whole.abs() / 2.0).abs() < 1e-6 * whole.abs(), "half {v} of {whole}");
        // The cap is an annulus: its area is pi (R_out^2 - R_in^2) of the polygonal circles.
        let caps = half.cell_data.get("Cap").unwrap();
        let area: f64 = half
            .polys
            .iter()
            .enumerate()
            .filter(|(c, _)| caps.get(*c, 0) == 1.0)
            .map(|(_, (_, t))| {
                let p = t.iter().map(|i| half.points.get(*i as usize)).collect::<Vec<_>>();
                0.5 * ((p[1][0] - p[0][0]) * (p[2][1] - p[0][1]) - (p[1][1] - p[0][1]) * (p[2][0] - p[0][0])).abs()
            })
            .sum();
        let ring = |r: f64| 0.5 * 48.0 * r * r * (std::f64::consts::TAU / 48.0).sin();
        assert!((area - (ring(1.3) - ring(0.7))).abs() < 1e-9, "cap area {area}");
    }

    #[test]
    fn iso_volume_of_a_linear_field_cuts_at_the_exact_values() {
        let mut img = grid(10, 0.1);
        let x: Vec<f64> = (0..=10).flat_map(|_k| (0..=10).flat_map(|_j| (0..=10).map(|i| i as f64 * 0.1))).collect();
        img.point_data.insert(Array::scalars_f64("x", x));
        let ds = DataSet::ImageData(img);
        let iv = iso_volume(&ds, Association::Points, "x", None, 0.25, 0.75).unwrap();
        let vol = cells_volume(&iv);
        assert!((vol - 0.5).abs() < 1e-9, "{vol}");
        let xs = iv.point_data.get("x").unwrap();
        let (lo, hi) = (0..xs.len()).fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), i| (a.min(xs.get(i, 0)), b.max(xs.get(i, 0))));
        assert!((lo - 0.25).abs() < 1e-12 && (hi - 0.75).abs() < 1e-12);
    }

    #[test]
    fn cells_by_region_and_by_type_keep_whole_cells() {
        let ds = DataSet::ImageData(grid(10, 0.1));
        let sphere = Region::Sphere { center: [0.5; 3], radius: 0.3 };
        let inside = cells_by_region(&ds, &sphere, RegionMode::AllInside, false).unwrap();
        let cross = cells_by_region(&ds, &sphere, RegionMode::Intersecting, false).unwrap();
        let ball = 4.0 / 3.0 * std::f64::consts::PI * 0.027;
        let (vi, vc) = (inside.cells.len() as f64 * 1e-3, cross.cells.len() as f64 * 1e-3);
        assert!(vi < ball && ball < vc, "{vi} < {ball} < {vc}");
        let outside = cells_by_region(&ds, &sphere, RegionMode::AllInside, true).unwrap();
        assert_eq!(outside.cells.len() + inside.cells.len(), 1000);
        // A half-space: the side the normal points to, as Clip keeps it.
        let half = cells_by_region(&ds, &Region::Plane { origin: [0.5; 3], normal: [1.0, 0.0, 0.0] }, RegionMode::AllInside, false).unwrap();
        assert_eq!(half.cells.len(), 500);
        assert!((0..half.points.len()).all(|i| half.points.get(i)[0] >= 0.5 - 1e-12));
        // Types: the mixed cells of the cell-types source.
        let mut mixed = crate::sources::unstructured_cell_types(CellType::Hexahedron, [2, 2, 2], 1);
        let tets = crate::sources::unstructured_cell_types(CellType::Tetra, [2, 2, 2], 1);
        let base = mixed.points.len() as u32;
        let mut pts: Vec<f64> = (0..mixed.points.len()).flat_map(|i| mixed.points.get(i)).collect();
        pts.extend((0..tets.points.len()).flat_map(|i| tets.points.get(i)));
        mixed.points = Points::new_f64(pts);
        for (t, c) in tets.cells.iter() {
            mixed.cells.push(t, &c.iter().map(|i| i + base).collect::<Vec<_>>());
        }
        mixed.point_data = Attributes::new();
        mixed.cell_data = Attributes::new();
        let mixed = DataSet::UnstructuredGrid(mixed);
        let hexes = cells_by_type(&mixed, "Hexahedron", false).unwrap();
        assert!(!hexes.cells.is_empty() && hexes.cells.iter().all(|(t, _)| t == CellType::Hexahedron));
        let solids = cells_by_type(&mixed, "3D", false).unwrap();
        assert!(solids.cells.iter().all(|(t, _)| t.dimension() == 3) && solids.cells.len() > hexes.cells.len());
        let not_hex = cells_by_type(&mixed, "Hexahedron", true).unwrap();
        assert!(not_hex.cells.iter().all(|(t, _)| t == CellType::Tetra) && not_hex.cells.len() + hexes.cells.len() == mixed.n_cells());
        assert!(cells_by_type(&mixed, "Bogus", false).unwrap_err().to_string().contains("no cell type"));
    }
}
