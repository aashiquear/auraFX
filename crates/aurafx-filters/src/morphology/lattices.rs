//! Architectures: triply periodic surfaces, lattices of struts (regular, disordered,
//! hierarchical), fractal sponges, foams (struts on the edges of cells, and level sets of
//! the distances to packed seeds) and scale-rich networks.

use super::fibres::periodic_images;
use super::particles::{dense_packing, sequential_addition};
use super::*;
use crate::synthetic::{Draws, Rng};
use anyhow::{bail, Result};
use std::f64::consts::{PI, TAU};

pub const SURFACES: &[&str] = &["Gyroid", "Schwarz P", "Schwarz D", "Neovius", "I-WP"];
pub const VARIANT_SHEET: &str = "Sheet";
pub const VARIANT_NETWORK: &str = "Network";

/// A triply periodic surface as a solid: the sheet about the surface, or the network on one
/// side of it, at a volume fraction.
pub fn triply_periodic_surface(g: &Grid, surface: &str, variant: &str, cell: f64, fraction: f64) -> Result<DataSet> {
    if !(0.0..=1.0).contains(&fraction) {
        bail!("the volume fraction must be between 0 and 1");
    }
    let kind = SURFACES.iter().position(|s| *s == surface).unwrap_or(0);
    let field: Vec<f32> = per_cell(g, |p| {
        let [x, y, z] = p.map(|v| TAU * v / cell);
        let (sx, sy, sz, cx, cy, cz) = (x.sin(), y.sin(), z.sin(), x.cos(), y.cos(), z.cos());
        (match kind {
            1 => cx + cy + cz,
            2 => sx * sy * sz + sx * cy * cz + cx * sy * cz + cx * cy * sz,
            3 => 3.0 * (cx + cy + cz) + 4.0 * cx * cy * cz,
            4 => 2.0 * (cx * cy + cy * cz + cz * cx) - ((2.0 * x).cos() + (2.0 * y).cos() + (2.0 * z).cos()),
            _ => sx * cy + sy * cz + sz * cx,
        }) as f32
    });
    let phases =
        if variant == VARIANT_SHEET { below_quantile(&field.iter().map(|x| x.abs()).collect::<Vec<_>>(), fraction) } else { below_quantile(&field, fraction) };
    Ok(DataSet::ImageData(phase_image(g, phases, None)))
}

pub const CELLS: &[&str] = &["Cubic", "Body-centred cubic", "Face-centred cubic", "Octet", "Kelvin", "Diamond", "Pyrochlore"];

/// The nodes of a unit cell and its struts: (node, node, cell of the second node).
type UnitCell = (Vec<[f64; 3]>, Vec<(usize, usize, [i32; 3])>);

/// Nodes in the unit cube joined to their neighbours at the distance `reach`, each strut once.
fn joined(nodes: Vec<[f64; 3]>, reach: f64, keep: impl Fn(usize, usize) -> bool) -> UnitCell {
    let mut struts = Vec::new();
    for i in 0..nodes.len() {
        for j in i..nodes.len() {
            for sz in -1..=1i32 {
                for sy in -1..=1i32 {
                    for sx in -1..=1i32 {
                        let s = [sx, sy, sz];
                        // A strut and its opposite are the same: the one going "up" is kept.
                        if i == j && s <= [0, 0, 0] {
                            continue;
                        }
                        let d: [f64; 3] = std::array::from_fn(|k| nodes[j][k] + s[k] as f64 - nodes[i][k]);
                        if (norm(d) - reach).abs() < 1e-6 && keep(i, j) {
                            struts.push((i, j, s));
                        }
                    }
                }
            }
        }
    }
    (nodes, struts)
}

fn unit_cell(kind: &str) -> UnitCell {
    let fcc = vec![[0.0, 0.0, 0.0], [0.5, 0.5, 0.0], [0.5, 0.0, 0.5], [0.0, 0.5, 0.5]];
    match kind {
        "Body-centred cubic" => joined(vec![[0.0; 3], [0.5; 3]], 0.75f64.sqrt(), |_, _| true),
        // Corners joined to the centres of the faces.
        "Face-centred cubic" => joined(fcc, 0.5f64.sqrt(), |i, j| (i == 0) != (j == 0)),
        "Octet" => joined(fcc, 0.5f64.sqrt(), |_, _| true),
        "Kelvin" => {
            let mut nodes = Vec::new();
            for q in [0.25, 0.75] {
                for perm in [[0, 1, 2], [0, 2, 1], [1, 0, 2], [1, 2, 0], [2, 0, 1], [2, 1, 0]] {
                    let v = [0.0, q, 0.5];
                    nodes.push([v[perm[0]], v[perm[1]], v[perm[2]]]);
                }
            }
            joined(nodes, 0.125f64.sqrt(), |_, _| true)
        }
        "Diamond" => {
            let mut nodes = fcc.clone();
            nodes.extend(fcc.iter().map(|p| p.map(|x| x + 0.25)));
            joined(nodes, 0.1875f64.sqrt(), |_, _| true)
        }
        "Pyrochlore" => {
            // Tetrahedra joined by their corners: every node belongs to two of them.
            let corners = [[0.0, 0.0, 0.0], [0.25, 0.25, 0.0], [0.25, 0.0, 0.25], [0.0, 0.25, 0.25]];
            let nodes: Vec<[f64; 3]> = fcc.iter().flat_map(|p| corners.iter().map(move |c| add(*p, *c))).collect();
            joined(nodes, 0.125f64.sqrt(), |_, _| true)
        }
        _ => joined(vec![[0.0; 3]], 1.0, |_, _| true),
    }
}

/// A number in [0, 1) from a seed and four integers (the same for the same integers).
fn hash01(seed: u64, a: i64, b: i64, c: i64, d: i64) -> f64 {
    let mut x = seed ^ 0x9E37_79B9_7F4A_7C15;
    for v in [a, b, c, d] {
        x = (x ^ (v as u64).wrapping_mul(0xBF58_476D_1CE4_E5B9)).wrapping_mul(0x94D0_49BB_1331_11EB);
        x ^= x >> 29;
    }
    x = (x ^ (x >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    x = (x ^ (x >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    ((x ^ (x >> 31)) >> 11) as f64 / (1u64 << 53) as f64
}

/// The rotation of the Bunge angles (degrees): about Z, then X, then Z.
fn bunge(e: [f64; 3]) -> [[f64; 3]; 3] {
    let [a, b, c] = e.map(f64::to_radians);
    let (s1, c1, s, cc, s2, c2) = (a.sin(), a.cos(), b.sin(), b.cos(), c.sin(), c.cos());
    [[c1 * c2 - s1 * s2 * cc, -c1 * s2 - s1 * c2 * cc, s1 * s], [s1 * c2 + c1 * s2 * cc, -s1 * s2 + c1 * c2 * cc, -c1 * s], [s2 * s, c2 * s, cc]]
}

fn rotate(m: &[[f64; 3]; 3], v: [f64; 3]) -> [f64; 3] {
    std::array::from_fn(|k| dot(m[k], v))
}

pub struct LatticeParams {
    pub cell: String,
    pub cell_size: f64,
    pub radius: f64,
    /// Above 0: the radius is the one that gives this volume fraction.
    pub fraction: f64,
    /// Above 0: hollow struts with walls this thick.
    pub wall: f64,
    /// The radius at the upper face along `gradient_axis` (not a number: the same everywhere).
    pub radius_top: f64,
    pub gradient_axis: usize,
    /// Bunge angles of the lattice, degrees.
    pub rotation: [f64; 3],
    /// Mean number of struts kept at a node (0: all), and how far the nodes are moved, as a
    /// fraction of the cell.
    pub coordination: f64,
    pub perturbation: f64,
    pub seed: u64,
}

/// The struts of a lattice over the box, as segments, with the numbers of nodes and struts
/// of the cells of the box.
fn lattice_struts(g: &Grid, p: &LatticeParams) -> (Vec<([f64; 3], [f64; 3])>, usize, usize, bool) {
    let (nodes, struts) = unit_cell(&p.cell);
    let a = p.cell_size;
    let turned = p.rotation != [0.0; 3];
    let m = bunge(p.rotation);
    let centre = g.size.map(|s| 0.5 * s);
    let cells: [i64; 3] = std::array::from_fn(|k| (g.size[k] / a).round().max(1.0) as i64);
    // A whole number of cells in a periodic box, not turned: the lattice wraps.
    let wraps = g.periodic && !turned && (0..3).all(|k| (cells[k] as f64 * a - g.size[k]).abs() < 1e-6 * a);
    let range: [(i64, i64); 3] = if turned {
        let half = (norm(g.size) / a / 2.0).ceil() as i64 + 1;
        [(-half, half); 3]
    } else if wraps {
        std::array::from_fn(|k| (0, cells[k] - 1))
    } else {
        std::array::from_fn(|k| (-1, (g.size[k] / a).ceil() as i64))
    };
    let full = 2.0 * struts.len() as f64 / nodes.len() as f64;
    let keep = if p.coordination > 0.0 { (p.coordination / full).min(1.0) } else { 1.0 };
    let node_at = |c: [i64; 3], j: usize| -> [f64; 3] {
        let w: [i64; 3] = if wraps { std::array::from_fn(|k| c[k].rem_euclid(cells[k])) } else { c };
        let mut x: [f64; 3] = std::array::from_fn(|k| (c[k] as f64 + nodes[j][k]) * a);
        if p.perturbation > 0.0 {
            // A point uniform in a ball, the same for a node whichever cell names it.
            let (u, v, t) = (
                hash01(p.seed, w[0], w[1], w[2], j as i64 * 4 + 1),
                hash01(p.seed, w[0], w[1], w[2], j as i64 * 4 + 2),
                hash01(p.seed, w[0], w[1], w[2], j as i64 * 4 + 3),
            );
            let (z, phi, rad) = (2.0 * u - 1.0, TAU * v, p.perturbation * a * t.cbrt());
            let s = (1.0 - z * z).sqrt();
            x = add(x, [rad * s * phi.cos(), rad * s * phi.sin(), rad * z]);
        }
        if turned {
            x = add(rotate(&m, sub(x, centre)), centre);
        }
        x
    };
    let mut out = Vec::new();
    let mut kept = 0usize;
    let in_box = |c: [i64; 3]| (0..3).all(|k| c[k] >= 0 && c[k] < cells[k]);
    for cz in range[2].0..=range[2].1 {
        for cy in range[1].0..=range[1].1 {
            for cx in range[0].0..=range[0].1 {
                let c = [cx, cy, cz];
                for (k, (i, j, s)) in struts.iter().enumerate() {
                    let w: [i64; 3] = if wraps { std::array::from_fn(|q| c[q].rem_euclid(cells[q])) } else { c };
                    if keep < 1.0 && hash01(p.seed, w[0], w[1], w[2], -(k as i64) - 1) >= keep {
                        continue;
                    }
                    if in_box(c) {
                        kept += 1;
                    }
                    out.push((node_at(c, *i), node_at([cx + s[0] as i64, cy + s[1] as i64, cz + s[2] as i64], *j)));
                }
            }
        }
    }
    (out, nodes.len() * (cells[0] * cells[1] * cells[2]) as usize, kept, wraps)
}

/// The solid of struts from the distance to their axes: where the distance is below the
/// radius (and above the radius less the wall for hollow struts). `base` gives the radius at
/// a cell; with a fraction, the radii are scaled together until the solid takes it.
fn strut_solid(g: &Grid, dist: &[f32], base: impl Fn(usize) -> f64 + Sync, fraction: f64, wall: f64, reach: f64) -> (Vec<i32>, f64) {
    let solid = |c: usize, s: f64| -> bool {
        let (d, rad) = (dist[c] as f64, s * base(c));
        d < rad.min(reach) && (wall <= 0.0 || d >= rad - wall)
    };
    let mut s = 1.0;
    if fraction > 0.0 {
        let top = (0..g.len()).into_par_iter().map(&base).reduce(|| 0.0, f64::max).max(1e-12);
        let share = |s: f64| (0..g.len()).into_par_iter().filter(|c| solid(*c, s)).count() as f64 / g.len() as f64;
        let (mut lo, mut hi) = (0.0, reach / top);
        for _ in 0..40 {
            s = 0.5 * (lo + hi);
            if share(s) < fraction {
                lo = s;
            } else {
                hi = s;
            }
        }
        // On a lattice lined up with the grid whole shells of cells come in at one radius:
        // the fraction moves in steps, and the side of the step nearer the goal is taken.
        s = if (share(lo) - fraction).abs() < (share(hi) - fraction).abs() { lo } else { hi };
    }
    ((0..g.len()).into_par_iter().map(|c| if solid(c, s) { PHASE_SOLID } else { PHASE_MATRIX }).collect(), s)
}

/// A lattice of struts: one of the unit cells repeated over the box, with solid or hollow
/// struts of a given radius or volume fraction, the radius graded along an axis, the lattice
/// turned; with a coordination or a perturbation, struts are missing and nodes moved.
pub fn strut_lattice(g: &Grid, p: &LatticeParams) -> Result<DataSet> {
    if !(p.cell_size > 0.0) {
        bail!("the cell size must be above 0");
    }
    if p.cell_size < 3.0 * g.h {
        bail!("the cells of the lattice are smaller than three cells of the grid: raise the cell size");
    }
    let graded = p.radius_top.is_finite();
    let top = if graded { p.radius_top } else { p.radius };
    let reach = if p.fraction > 0.0 { 0.5 * p.cell_size } else { p.radius.max(top) };
    let (segments, nodes, struts, wraps) = lattice_struts(g, p);
    let dist = segment_distance(g, &segments, reach);
    let ax = p.gradient_axis.min(2);
    let base = |c: usize| -> f64 {
        let u = g.pos(c)[ax] / g.size[ax];
        p.radius + (top - p.radius) * u
    };
    let (phases, s) = strut_solid(g, &dist, base, p.fraction, p.wall, reach);
    let mut img = phase_image(g, phases, None);
    let at = |x: [f64; 3]| s * (p.radius + (top - p.radius) * (x[ax] / g.size[ax]).clamp(0.0, 1.0));
    shapes::keep(&mut img, &segments.iter().map(|(a, b)| [a[0], a[1], a[2], b[0], b[1], b[2], at(*a), at(*b)]).collect::<Vec<_>>(), wraps);
    note(&mut img, "StrutRadius", vec![s * p.radius, s * top]);
    note(&mut img, "Nodes", vec![nodes as f64]);
    note(&mut img, "Struts", vec![struts as f64]);
    note(&mut img, "MeanCoordination", vec![2.0 * struts as f64 / nodes.max(1) as f64]);
    // Maxwell's count: struts less three times the nodes (below 0: the frame has mechanisms).
    note(&mut img, "MaxwellNumber", vec![struts as f64 - 3.0 * nodes as f64]);
    Ok(DataSet::ImageData(img))
}

/// A frame between `a` and `b`: a tube at generation 0; above, a column of `n` regular
/// octahedra closed by two tetrahedra, whose edges under compression (those that leave the
/// planes of the triangles) are frames of the generation below.
fn frame_members(a: [f64; 3], b: [f64; 3], generation: usize, n: usize, out: &mut Vec<([f64; 3], [f64; 3])>) {
    if generation == 0 {
        out.push((a, b));
        return;
    }
    let axis = sub(b, a);
    let length = norm(axis);
    let u = scale(axis, 1.0 / length);
    let (v, w) = frame(u);
    let rise = (2.0f64 / 3.0).sqrt();
    let edge = length / (rise * (n as f64 + 2.0));
    let ring = edge / 3f64.sqrt();
    let corner = |k: usize, m: usize| -> [f64; 3] {
        let t = k as f64 * PI / 3.0 + m as f64 * TAU / 3.0;
        add(a, add(scale(u, rise * edge * (k as f64 + 1.0)), add(scale(v, ring * t.cos()), scale(w, ring * t.sin()))))
    };
    for k in 0..=n {
        for m in 0..3 {
            out.push((corner(k, m), corner(k, (m + 1) % 3)));
            if k < n {
                frame_members(corner(k, m), corner(k + 1, m), generation - 1, n, out);
                frame_members(corner(k, m), corner(k + 1, (m + 2) % 3), generation - 1, n, out);
            }
        }
    }
    for m in 0..3 {
        frame_members(a, corner(0, m), generation - 1, n, out);
        frame_members(b, corner(n, m), generation - 1, n, out);
    }
}

/// A hierarchical lattice: the nodes of a face-centred cubic lattice joined to their nearest
/// neighbours (and, with `cube_edges`, to the next ones) by frames of octahedra made of
/// tubes, themselves frames at the generation above 1.
pub fn hierarchical_lattice(g: &Grid, cell_size: f64, generations: usize, octahedra: usize, radius: f64, wall: f64, cube_edges: bool) -> Result<DataSet> {
    if generations > 2 {
        bail!("at most 2 generations");
    }
    if !(cell_size > 0.0 && radius > 0.0) {
        bail!("the cell size and the tube radius must be above 0");
    }
    let fcc = vec![[0.0, 0.0, 0.0], [0.5, 0.5, 0.0], [0.5, 0.0, 0.5], [0.0, 0.5, 0.5]];
    let (nodes, mut struts) = joined(fcc.clone(), 0.5f64.sqrt(), |_, _| true);
    if cube_edges {
        struts.extend(joined(fcc, 1.0, |_, _| true).1);
    }
    let cells: [i64; 3] = std::array::from_fn(|k| (g.size[k] / cell_size).ceil() as i64);
    if (cells[0] * cells[1] * cells[2]) as usize * struts.len() * 900usize.pow(generations.saturating_sub(1) as u32) > 60_000_000 {
        bail!("too many tubes: raise the cell size or lower the generations");
    }
    let mut members = Vec::new();
    for cz in -1..=cells[2] {
        for cy in -1..=cells[1] {
            for cx in -1..=cells[0] {
                for (i, j, s) in &struts {
                    let a: [f64; 3] = std::array::from_fn(|k| ([cx, cy, cz][k] as f64 + nodes[*i][k]) * cell_size);
                    let b: [f64; 3] = std::array::from_fn(|k| ([cx, cy, cz][k] as f64 + s[k] as f64 + nodes[*j][k]) * cell_size);
                    frame_members(a, b, generations, octahedra.max(1), &mut members);
                }
            }
        }
    }
    let open = Grid { periodic: false, ..g.clone() };
    let dist = segment_distance(&open, &members, radius);
    let (phases, _) = strut_solid(g, &dist, |_| radius, 0.0, wall, radius);
    let mut img = phase_image(g, phases, None);
    shapes::keep(&mut img, &members.iter().map(|(a, b)| shapes::capsule(*a, *b, radius)).collect::<Vec<_>>(), false);
    note(&mut img, "TubesPerCell", vec![members.len() as f64 / ((cells[0] + 2) * (cells[1] + 2) * (cells[2] + 2)) as f64]);
    Ok(DataSet::ImageData(img))
}

pub const SPONGES: &[&str] = &["Menger sponge", "Vicsek fractal", "Cantor dust"];

/// A fractal on a cubic grid: a cube is cut in 27, some of the small cubes are kept, and each
/// kept cube is cut again, `level` times. A point is solid when its small cube is kept at
/// every level.
pub fn fractal_sponge(g: &Grid, pattern: &str, level: usize, cell: f64) -> Result<DataSet> {
    if level > 12 {
        bail!("at most 12 levels");
    }
    let kept = |d: [u64; 3]| -> bool {
        let middle = d.iter().filter(|x| **x == 1).count();
        match pattern {
            "Vicsek fractal" => middle >= 2,
            "Cantor dust" => middle == 0,
            _ => middle < 2,
        }
    };
    let side = 3u64.pow(level as u32);
    let phases: Vec<i32> = per_cell(g, |p| {
        let mut i: [u64; 3] = p.map(|x| ((x / cell).rem_euclid(1.0) * side as f64) as u64 % side.max(1));
        for _ in 0..level {
            if !kept(i.map(|x| x % 3)) {
                return PHASE_MATRIX;
            }
            i = i.map(|x| x / 3);
        }
        PHASE_SOLID
    });
    let per_level = match pattern {
        "Vicsek fractal" => 7.0f64,
        "Cantor dust" => 8.0,
        _ => 20.0,
    };
    let mut img = phase_image(g, phases, None);
    note(&mut img, "ExpectedFraction", vec![(per_level / 27.0).powi(level as i32)]);
    note(&mut img, "FractalDimension", vec![per_level.ln() / 3f64.ln()]);
    Ok(DataSet::ImageData(img))
}

/// The centre of the sphere through four points.
fn circumcentre(p: [[f64; 3]; 4]) -> Option<[f64; 3]> {
    let (u, v, w) = (sub(p[1], p[0]), sub(p[2], p[0]), sub(p[3], p[0]));
    let det = 2.0 * dot(u, cross(v, w));
    if det.abs() < 1e-300 {
        return None;
    }
    let o = add(add(scale(cross(v, w), dot(u, u)), scale(cross(w, u), dot(v, v))), scale(cross(u, v), dot(w, w)));
    Some(add(p[0], scale(o, 1.0 / det)))
}

/// The edges of the Voronoi cells of `points` in a periodic box of size `size`: every edge
/// once (the one whose middle lies in the box), as its two ends.
pub fn voronoi_edges(size: [f64; 3], points: &[[f64; 3]]) -> Result<Vec<([f64; 3], [f64; 3])>> {
    let all = periodic_images(size, points);
    let tets = crate::delaunay3d::tetrahedralize(&all, 0.0)?;
    let centres: Vec<Option<[f64; 3]>> = tets.iter().map(|t| circumcentre(t.map(|i| all[i]))).collect();
    // Two tetrahedra that share a face: their circumcentres are the ends of a Voronoi edge.
    let mut faces: std::collections::HashMap<[usize; 3], usize> = std::collections::HashMap::new();
    let mut edges = Vec::new();
    for (k, t) in tets.iter().enumerate() {
        for skip in 0..4 {
            let mut f: Vec<usize> = (0..4).filter(|i| *i != skip).map(|i| t[i]).collect();
            f.sort_unstable();
            let key = [f[0], f[1], f[2]];
            if let Some(other) = faces.remove(&key) {
                if let (Some(a), Some(b)) = (centres[k], centres[other]) {
                    let mid = scale(add(a, b), 0.5);
                    if (0..3).all(|q| mid[q] >= 0.0 && mid[q] < size[q]) {
                        edges.push((a, b));
                    }
                }
            } else {
                faces.insert(key, k);
            }
        }
    }
    Ok(edges)
}

pub struct FoamParams {
    pub cells: usize,
    pub radius: f64,
    /// Above 0: the strut radius is the one that gives this volume fraction.
    pub fraction: f64,
    /// Times the germs are moved to the centroids of their cells (even cells).
    pub relaxation: usize,
    /// Elongation of the cells along `axis` (1: none).
    pub anisotropy: f64,
    pub axis: usize,
    /// Radius of the balls at the nodes over the strut radius (1 or less: none).
    pub node_ratio: f64,
    /// Above 0: the points are made stealthy (see `stealthy`) before the relaxation.
    pub stealth: f64,
    pub seed: u64,
}

/// An open-cell foam: struts on the edges of the Voronoi cells of random germs, the germs
/// moved to the centroids of their cells for even cells, the cells elongated along an axis
/// (the foam is made in a box shortened along that axis, then stretched back).
pub fn open_cell_foam(g: &Grid, p: &FoamParams) -> Result<DataSet> {
    if g.planar() {
        bail!("the foam needs more than one layer of cells");
    }
    if p.cells < 8 {
        bail!("at least 8 cells");
    }
    let stretch = p.anisotropy.max(1e-3);
    let ax = p.axis.min(2);
    let mut size = g.size;
    size[ax] /= stretch;
    let mut r = Rng::new(p.seed);
    let mut germs: Vec<[f64; 3]> = (0..p.cells).map(|_| std::array::from_fn(|k| r.uniform(0.0, size[k]))).collect();
    if p.stealth > 0.0 {
        germs = stealthy(size, false, germs, p.stealth)?;
    }
    // Lloyd's relaxation on a coarse grid: each germ goes to the centroid of its cell.
    let coarse: [usize; 3] = std::array::from_fn(|k| g.n[k].clamp(2, 40));
    let hc: [f64; 3] = std::array::from_fn(|k| size[k] / coarse[k] as f64);
    let zero = vec![0.0; germs.len()];
    for _ in 0..p.relaxation {
        let labels = crate::synthetic::power_labels(coarse, hc, &germs, &zero, true);
        let mut sum = vec![[0.0; 3]; germs.len()];
        let mut count = vec![0usize; germs.len()];
        for (c, l) in labels.iter().enumerate() {
            let ijk = [c % coarse[0], (c / coarse[0]) % coarse[1], c / (coarse[0] * coarse[1])];
            let s = germs[*l as usize];
            for k in 0..3 {
                let d = (ijk[k] as f64 + 0.5) * hc[k] - s[k];
                sum[*l as usize][k] += d - size[k] * (d / size[k]).round();
            }
            count[*l as usize] += 1;
        }
        for (i, s) in germs.iter_mut().enumerate() {
            if count[i] > 0 {
                for k in 0..3 {
                    s[k] = (s[k] + sum[i][k] / count[i] as f64).rem_euclid(size[k]);
                }
            }
        }
    }
    let mut edges = voronoi_edges(size, &germs)?;
    for e in edges.iter_mut() {
        e.0[ax] *= stretch;
        e.1[ax] *= stretch;
    }
    let spacing = (g.volume() / p.cells as f64).cbrt();
    let reach = if p.fraction > 0.0 { 0.4 * spacing } else { p.radius * p.node_ratio.max(1.0) };
    let wrapped = Grid { periodic: true, ..g.clone() };
    let mut dist = segment_distance(&wrapped, &edges, reach);
    if p.node_ratio > 1.0 {
        // A ball at each node: as a strut of radius 1 at the distance divided by the ratio.
        for e in &edges {
            for node in [e.0, e.1] {
                wrapped.stamp_sphere(node, reach, |c, d2| {
                    let d = (d2.sqrt() / p.node_ratio) as f32;
                    if d < dist[c] {
                        dist[c] = d;
                    }
                });
            }
        }
    }
    let (phases, radius) = if p.fraction > 0.0 {
        let ph = below_quantile(&dist, p.fraction.min(0.9));
        let rad = quantile(&dist, p.fraction.min(0.9)) as f64;
        (ph, rad)
    } else {
        (dist.iter().map(|d| if (*d as f64) < p.radius { PHASE_SOLID } else { PHASE_MATRIX }).collect(), p.radius)
    };
    let mut img = phase_image(g, phases, None);
    let mut all: Vec<shapes::Shape> = edges.iter().map(|e| shapes::capsule(e.0, e.1, radius)).collect();
    if p.node_ratio > 1.0 {
        all.extend(edges.iter().flat_map(|e| [shapes::sphere(e.0, radius * p.node_ratio), shapes::sphere(e.1, radius * p.node_ratio)]));
    }
    shapes::keep(&mut img, &all, true);
    note(&mut img, "StrutRadius", vec![radius]);
    note(&mut img, "Struts", vec![edges.len() as f64]);
    Ok(DataSet::ImageData(img))
}

pub struct LevelSetFoamParams {
    /// The short semi-axis of the seeds, between two values.
    pub seed_radius: [f64; 2],
    /// The two other semi-axes over the short one (along Y and Z).
    pub aspect: [f64; 2],
    /// Volume fraction of the packed seeds.
    pub packing: f64,
    pub fraction: f64,
    /// Bends the sides of the struts: between 0 and 1 hollow sides, below 0 bulging ones.
    pub curvature: f64,
    /// The area of a strut section along the strut, over its value at mid-span, is
    /// `c1 x^4 + c2 x^2 + 1` with x from 0 at mid-span to 1/2 at the nodes.
    pub c1: f64,
    pub c2: f64,
    /// Above 0: closed cells, with walls of this thickness over that of the struts.
    pub wall: f64,
    pub seed: u64,
}

/// A foam as a level set of the distances to packed seeds. Aligned ellipsoids are packed
/// without overlap; every seed defines a distance function (-1 at its centre, 0 on its
/// surface), and at each point the smallest ones are compared: the struts lie where the
/// three smallest are nearly equal, the nodes where four are, the walls of closed cells where
/// two are. The thickness is the one that gives the volume fraction.
pub fn level_set_foam(g: &Grid, p: &LevelSetFoamParams) -> Result<DataSet> {
    if g.planar() {
        bail!("the foam needs more than one layer of cells");
    }
    if !(p.fraction > 0.0 && p.fraction < 1.0) {
        bail!("the volume fraction must be between 0 and 1");
    }
    let mut r = Rng::new(p.seed);
    let shape = [1.0, p.aspect[0].max(0.05), p.aspect[1].max(0.05)];
    // Packed as spheres in the box squeezed by the shape of the seeds, then stretched back.
    let squeezed = Grid { size: std::array::from_fn(|k| g.size[k] / shape[k]), ..g.clone() };
    let (lo, hi) = (p.seed_radius[0].min(p.seed_radius[1]), p.seed_radius[0].max(p.seed_radius[1]));
    if !(lo > 0.0) {
        bail!("the seed radius must be above 0");
    }
    let goal = p.packing.clamp(0.05, 0.64) * squeezed.volume();
    let mut radii = Vec::new();
    let mut total = 0.0;
    while total < goal {
        let a = r.uniform(lo, hi);
        total += 4.0 / 3.0 * PI * a.powi(3);
        radii.push(a);
    }
    if radii.len() < 8 {
        bail!("fewer than 8 seeds fit in the box: lower the seed radius");
    }
    if radii.len() as f64 * g.len() as f64 > 3e10 {
        bail!("too many seeds ({}) for this grid: raise the seed radius", radii.len());
    }
    radii.sort_by(|a, b| b.total_cmp(a));
    let centres = if p.packing > 0.3 {
        dense_packing(&squeezed, &radii, 0.0, 0.02, &mut r)?
    } else {
        let c = sequential_addition(&squeezed, &radii, 0.0, 20_000, &mut r);
        if c.len() < radii.len() {
            bail!("the seeds jam: lower the packing");
        }
        c
    };
    let seeds: Vec<([f64; 3], [f64; 3])> =
        centres.iter().zip(&radii).map(|(c, a)| (std::array::from_fn(|k| c[k] * shape[k]), std::array::from_fn(|k| a * shape[k]))).collect();
    // The four smallest distance functions at every cell.
    let nearest: Vec<([f32; 4], [u32; 3])> = per_cell(g, |x| {
        let mut best = [(f64::INFINITY, 0u32); 4];
        for (i, (c, semi)) in seeds.iter().enumerate() {
            let d = g.delta(x, *c);
            let v = (d[0] / semi[0]).powi(2) + (d[1] / semi[1]).powi(2) + (d[2] / semi[2]).powi(2) - 1.0;
            if v < best[3].0 {
                best[3] = (v, i as u32);
                let mut k = 3;
                while k > 0 && best[k].0 < best[k - 1].0 {
                    best.swap(k, k - 1);
                    k -= 1;
                }
            }
        }
        let mut ids = [best[0].1, best[1].1, best[2].1];
        ids.sort_unstable();
        (best.map(|v| v.0 as f32), ids)
    });
    // Along a strut (the line between three seeds), the gap between the third and the fourth
    // distance is largest at mid-span: the largest gap near the axis of each strut measures it.
    let mut spans: std::collections::HashMap<[u32; 3], f32> = std::collections::HashMap::new();
    for (d, ids) in &nearest {
        if 0.5 * (d[2] + d[1]) - d[0] < 0.1 {
            let e = spans.entry(*ids).or_insert(0.0);
            *e = e.max(d[3] - d[2]);
        }
    }
    let near: Vec<f32> = nearest.iter().filter(|(d, _)| 0.5 * (d[2] + d[1]) - d[0] < 0.1).map(|(d, _)| d[3] - d[2]).collect();
    let overall = quantile(&near, 0.9).max(1e-6);
    // The four distances with, in place of the fourth, the gap over the span of the strut.
    let four: Vec<[f32; 4]> =
        nearest.iter().map(|(d, ids)| [d[0], d[1], d[2], (d[3] - d[2]) / spans.get(ids).copied().filter(|s| *s > 1e-6).unwrap_or(overall)]).collect();
    drop(nearest);
    let level = |d: &[f32; 4], t: f64| -> bool {
        let [d1, d2, d3, gap] = d.map(|v| v as f64);
        let xi = (0.5 * (1.0 - gap)).clamp(0.0, 0.5);
        let width = t * (p.c1 * xi.powi(4) + p.c2 * xi * xi + 1.0).max(0.0).sqrt();
        let across = 0.5 * (d3 - d2);
        let bend = if width > 0.0 { ((across * across - width * width) / (2.0 * width)).min(0.0) } else { 0.0 };
        let strut = 0.5 * (d3 + d2) - d1 - width - p.curvature * bend <= 0.0;
        strut || (p.wall > 0.0 && 0.5 * (d2 - d1) <= p.wall * t)
    };
    let (mut a, mut b) = (0.0, 4.0);
    let mut t = 0.0;
    for _ in 0..36 {
        t = 0.5 * (a + b);
        let f = four.par_iter().filter(|d| level(d, t)).count() as f64 / g.len() as f64;
        if f < p.fraction {
            a = t;
        } else {
            b = t;
        }
    }
    let phases: Vec<i32> = four.par_iter().map(|d| if level(d, t) { PHASE_SOLID } else { PHASE_MATRIX }).collect();
    let mut img = phase_image(g, phases, None);
    note(&mut img, "Seeds", vec![seeds.len() as f64]);
    note(&mut img, "Thickness", vec![t]);
    Ok(DataSet::ImageData(img))
}

pub const ELEMENT_PLATES: &str = "Plates";
pub const ELEMENT_BEAMS: &str = "Beams";

pub struct ScaleRichParams {
    pub ligaments: usize,
    /// The ligament number t is `first x t^-decay` thick.
    pub decay: f64,
    /// Above 0: the first thickness is the one that gives this volume fraction.
    pub fraction: f64,
    pub first: f64,
    pub element: String,
    /// Directions of the ligaments in the plane (degrees); empty: any.
    pub angles: Vec<f64>,
    /// In space: the plates lie across Z (layers), the beams along Z.
    pub aligned: bool,
    /// Least room left on either side of a new ligament.
    pub min_spacing: f64,
    /// Thickness of a frame of material around the box (0: none).
    pub frame: f64,
    pub attempts: usize,
    /// The share of the ligaments, the first ones, that are turned towards a pole: a point of
    /// the box (as fractions of its size), and the angle of the ligaments (plates: of their
    /// planes) to the direction of the pole, 0 radial, 90 around it.
    pub polarized: f64,
    pub pole: [f64; 3],
    pub pole_angle: f64,
    /// A normal spread of the directions about the ones given or polarized (degrees).
    pub angle_spread: f64,
    /// The density of the starting points from one end to the other (two weights; empty:
    /// even), along X, Y or Z (0, 1, 2) or the distance to the pole (3).
    pub gradient: Vec<f64>,
    pub gradient_along: usize,
    pub seed: u64,
}

impl ScaleRichParams {
    fn pole_at(&self, g: &Grid) -> [f64; 3] {
        let mut c: [f64; 3] = std::array::from_fn(|k| self.pole[k] * g.size[k]);
        if g.planar() {
            c[2] = 0.5 * g.h;
        }
        c
    }
    /// A starting point: uniform, or kept with the weight of the gradient where it falls.
    fn start(&self, g: &Grid, r: &mut Rng) -> [f64; 3] {
        loop {
            let mut x: [f64; 3] = std::array::from_fn(|k| r.uniform(self.frame, (g.size[k] - self.frame).max(self.frame)));
            if g.planar() {
                x[2] = 0.5 * g.h;
            }
            if self.gradient.len() < 2 {
                return x;
            }
            let u = if self.gradient_along < 3 {
                x[self.gradient_along] / g.size[self.gradient_along]
            } else {
                let pole = self.pole_at(g);
                let far = (0..8).map(|k| norm(sub(std::array::from_fn(|a| if k >> a & 1 == 1 { g.size[a] } else { 0.0 }), pole))).fold(0.0, f64::max);
                norm(sub(x, pole)) / far.max(1e-300)
            };
            let (a, b) = (self.gradient[0].max(0.0), self.gradient[1].max(0.0));
            if r.random() * a.max(b).max(1e-300) <= a + (b - a) * u {
                return x;
            }
        }
    }
    fn is_polarized(&self, t: usize) -> bool {
        self.polarized > 0.0 && (t as f64) <= (self.polarized * self.ligaments as f64).ceil()
    }
}

/// The box cut again and again by slabs: a ligament is a slab across the convex void it
/// starts in, which it splits in two.
enum Cut {
    Void,
    Slab { normal: [f64; 3], offset: f64, half: f64, below: usize, above: usize },
}

struct Cuts(Vec<Cut>);

impl Cuts {
    /// The void that holds `x`, or none when `x` lies in a ligament.
    fn void_at(&self, x: [f64; 3]) -> Option<usize> {
        let mut k = 0;
        loop {
            match &self.0[k] {
                Cut::Void => return Some(k),
                Cut::Slab { normal, offset, half, below, above } => {
                    let s = dot(*normal, x) - offset;
                    if s < -half {
                        k = *below;
                    } else if s > *half {
                        k = *above;
                    } else {
                        return None;
                    }
                }
            }
        }
    }
}

fn ligament_rng(seed: u64, t: usize) -> Rng {
    Rng::new(seed.wrapping_mul(0x0100_0000_01B3).wrapping_add(t as u64).wrapping_mul(0x9E37_79B9) ^ 0x5851_F42D)
}

/// Ligaments that span their voids: strips in the plane, plates in space.
fn grow_plates(g: &Grid, p: &ScaleRichParams, first: f64) -> (Cuts, usize) {
    let mut cuts = Cuts(vec![Cut::Void]);
    let planar = g.planar();
    let mut placed = 0;
    for t in 1..=p.ligaments {
        let mut r = ligament_rng(p.seed, t);
        let half = 0.5 * first * (t as f64).powf(-p.decay);
        let room = half + 0.5 * p.min_spacing;
        let mut done = false;
        for _ in 0..p.attempts.max(1) {
            let x = p.start(g, &mut r);
            let spread = p.angle_spread.to_radians();
            let away = unit(sub(x, p.pole_at(g)));
            let normal = if planar {
                let a = if p.is_polarized(t) {
                    away[1].atan2(away[0]) + p.pole_angle.to_radians()
                } else if p.angles.is_empty() {
                    r.uniform(0.0, PI)
                } else {
                    p.angles[((r.random() * p.angles.len() as f64) as usize).min(p.angles.len() - 1)].to_radians()
                } + if spread > 0.0 { spread * r.std_normal() } else { 0.0 };
                [-a.sin(), a.cos(), 0.0]
            } else {
                let n = if p.is_polarized(t) {
                    // At the angle to the direction of the pole, any way around it.
                    let (u, w) = frame(away);
                    let (b, phi) = (p.pole_angle.to_radians(), r.uniform(0.0, 2.0 * PI));
                    add(scale(away, b.sin()), scale(add(scale(u, phi.cos()), scale(w, phi.sin())), b.cos()))
                } else if p.aligned {
                    [0.0, 0.0, 1.0]
                } else {
                    isotropic(&mut r)
                };
                if spread > 0.0 {
                    let (u, w) = frame(n);
                    unit(add(n, add(scale(u, spread * r.std_normal()), scale(w, spread * r.std_normal()))))
                } else {
                    n
                }
            };
            let Some(void) = cuts.void_at(x) else { continue };
            let side = |s: f64| cuts.void_at(add(x, scale(normal, s))) == Some(void);
            if !(side(room) && side(-room)) {
                continue;
            }
            let (below, above) = (cuts.0.len(), cuts.0.len() + 1);
            cuts.0.push(Cut::Void);
            cuts.0.push(Cut::Void);
            cuts.0[void] = Cut::Slab { normal, offset: dot(normal, x), half, below, above };
            done = true;
            break;
        }
        if !done {
            // No room for this ligament anywhere it was tried: the network is jammed.
            break;
        }
        placed += 1;
    }
    (cuts, placed)
}

/// Beams in space: each grows both ways from its start until it touches another beam or
/// the box. Returns the axes and the radii.
fn grow_beams(g: &Grid, p: &ScaleRichParams, first: f64) -> Vec<([f64; 3], [f64; 3], f64)> {
    let mut beams: Vec<([f64; 3], [f64; 3], f64)> = Vec::new();
    let step = 0.5 * g.h;
    let inside = |x: [f64; 3]| (0..3).all(|k| x[k] >= 0.0 && x[k] <= g.size[k]);
    let point_gap = |x: [f64; 3], b: &([f64; 3], [f64; 3], f64)| -> f64 {
        let ab = sub(b.1, b.0);
        let t = (dot(sub(x, b.0), ab) / dot(ab, ab).max(1e-300)).clamp(0.0, 1.0);
        norm(sub(x, add(b.0, scale(ab, t)))) - b.2
    };
    for t in 1..=p.ligaments {
        let mut r = ligament_rng(p.seed, t);
        let rad = 0.5 * first * (t as f64).powf(-p.decay);
        let mut done = false;
        for _ in 0..p.attempts.max(1) {
            let x = p.start(g, &mut r);
            let d = if p.is_polarized(t) {
                let away = unit(sub(x, p.pole_at(g)));
                let (u, w) = frame(away);
                let (b, phi) = (p.pole_angle.to_radians(), r.uniform(0.0, 2.0 * PI));
                add(scale(away, b.cos()), scale(add(scale(u, phi.cos()), scale(w, phi.sin())), b.sin()))
            } else if p.aligned {
                [0.0, 0.0, 1.0]
            } else {
                isotropic(&mut r)
            };
            if beams.iter().any(|b| point_gap(x, b) < rad + 0.5 * p.min_spacing) {
                continue;
            }
            let reach = |sign: f64| -> [f64; 3] {
                let mut s = 0.0;
                loop {
                    let q = add(x, scale(d, sign * (s + step)));
                    if !inside(q) || beams.iter().any(|b| point_gap(q, b) < rad) {
                        return add(x, scale(d, sign * s));
                    }
                    s += step;
                }
            };
            beams.push((reach(-1.0), reach(1.0), rad));
            done = true;
            break;
        }
        if !done {
            break;
        }
    }
    beams
}

/// A scale-rich network: ligaments grown one after another, each thinner than the one
/// before (`first x t^-decay`), from a random point of the voids until they meet a ligament
/// or the box. Their thicknesses and lengths span orders of magnitude in one sample.
pub fn scale_rich_network(g: &Grid, p: &ScaleRichParams) -> Result<DataSet> {
    if p.ligaments == 0 {
        bail!("at least one ligament");
    }
    let beams = p.element == ELEMENT_BEAMS && !g.planar();
    let in_frame = |x: [f64; 3]| p.frame > 0.0 && (0..3).any(|k| g.n[k] > 1 && (x[k] < p.frame || x[k] > g.size[k] - p.frame));
    let build = |first: f64| -> (Vec<i32>, usize) {
        if beams {
            let grown = grow_beams(g, p, first);
            let open = Grid { periodic: false, ..g.clone() };
            let mut phases: Vec<i32> = per_cell(g, |x| if in_frame(x) { PHASE_SOLID } else { PHASE_MATRIX });
            for (a, b, rad) in &grown {
                open.stamp_segment(*a, *b, *rad, |c, _, _| phases[c] = PHASE_SOLID);
            }
            (phases, grown.len())
        } else {
            let (cuts, placed) = grow_plates(g, p, first);
            (per_cell(g, |x| if in_frame(x) || cuts.void_at(x).is_none() { PHASE_SOLID } else { PHASE_MATRIX }), placed)
        }
    };
    let least = (0..3).filter(|k| g.n[*k] > 1).map(|k| g.size[k]).fold(f64::INFINITY, f64::min);
    let mut first = p.first;
    if p.fraction > 0.0 {
        let (mut lo, mut hi) = (0.0, 0.5 * least);
        for _ in 0..if beams { 14 } else { 22 } {
            first = 0.5 * (lo + hi);
            if fraction_of(&build(first).0, PHASE_SOLID) < p.fraction {
                lo = first;
            } else {
                hi = first;
            }
        }
    }
    if !(first > 0.0) {
        bail!("the first thickness must be above 0 (or give a volume fraction)");
    }
    let (phases, placed) = build(first);
    let mut img = phase_image(g, phases, None);
    note(&mut img, "Ligaments", vec![placed as f64]);
    note(&mut img, "FirstThickness", vec![first]);
    Ok(DataSet::ImageData(img))
}

/// Used by the nacre and honeycomb models: germs kept apart in the plane.
pub fn planar_germs(g: &Grid, count: usize, regularity: f64, r: &mut dyn Draws) -> Result<Vec<[f64; 3]>> {
    let flat = Grid { n: [g.n[0], g.n[1], 1], size: [g.size[0], g.size[1], g.h], ..g.clone() };
    if regularity <= 0.0 {
        return Ok((0..count).map(|_| flat.random_point(r)).collect());
    }
    // Discs placed one after another jam near an area fraction of 0.547.
    let jam = (0.547 * 4.0 * flat.size[0] * flat.size[1] / (PI * count.max(1) as f64)).sqrt();
    let pts = sequential_addition(&flat, &vec![0.5 * jam * regularity.min(1.0); count], 0.0, 50_000, r);
    if pts.len() < count {
        bail!("the points jam after {} of {}: lower the regularity", pts.len(), count);
    }
    Ok(pts)
}
