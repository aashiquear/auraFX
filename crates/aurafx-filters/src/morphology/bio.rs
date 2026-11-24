//! Structures of living materials: nacre, twisted plywoods of fibres, honeycombs and wood,
//! and the osteons of bone. With them, the two filters that build on any model: a coating
//! around a phase, and the combination of two microstructures.

use super::lattices::planar_germs;
use super::*;
use crate::synthetic::{Draws, Rng};
use anyhow::{anyhow, bail, Result};
use std::f64::consts::PI;

/// The germ nearest to a point of the plane and the distance from the point to the side of
/// its Voronoi cell (the bisector with the second nearest germ).
fn cell_and_wall(g: &Grid, germs: &[[f64; 3]], x: f64, y: f64) -> (usize, f64) {
    let mut best = [(usize::MAX, f64::INFINITY); 2];
    for (k, s) in germs.iter().enumerate() {
        let (mut dx, mut dy) = (x - s[0], y - s[1]);
        if g.periodic {
            dx -= g.size[0] * (dx / g.size[0]).round();
            dy -= g.size[1] * (dy / g.size[1]).round();
        }
        let d = dx * dx + dy * dy;
        if d < best[0].1 {
            best[1] = best[0];
            best[0] = (k, d);
        } else if d < best[1].1 {
            best[1] = (k, d);
        }
    }
    if best[1].0 == usize::MAX {
        return (best[0].0, f64::INFINITY);
    }
    // The distance to the bisector of two points from the difference of the squared distances.
    let (a, b) = (germs[best[0].0], germs[best[1].0]);
    let (mut ex, mut ey) = (b[0] - a[0], b[1] - a[1]);
    if g.periodic {
        ex -= g.size[0] * (ex / g.size[0]).round();
        ey -= g.size[1] * (ey / g.size[1]).round();
    }
    (best[0].0, (best[1].1 - best[0].1) / (2.0 * (ex * ex + ey * ey).sqrt().max(1e-12)))
}

pub const NACRE_BRICK: &str = "Bricks";
pub const NACRE_POLYGON: &str = "Polygons";

/// Nacre: layers of mineral tablets (phase 2) in a mortar (phase 1). The tablets are square
/// bricks, each layer shifted from the one below, or the polygons of a tessellation of
/// random points, new in every layer.
pub fn nacre(g: &Grid, arrangement: &str, size: f64, thickness: f64, mortar: f64, shift: f64, seed: u64) -> Result<DataSet> {
    if !(size > mortar && thickness > 0.0 && mortar >= 0.0) {
        bail!("the tablets must be larger than the mortar is thick, and their thickness above 0");
    }
    let period = thickness + mortar;
    let layers = (g.size[2] / period).ceil().max(1.0) as usize;
    let mut r = Rng::new(seed);
    let polygons = arrangement == NACRE_POLYGON;
    let per_layer = ((g.size[0] * g.size[1] / (size * size)).round() as usize).max(1);
    let germs: Vec<Vec<[f64; 3]>> =
        if polygons { (0..layers).map(|_| planar_germs(g, per_layer, 0.6, &mut r as &mut dyn Draws)).collect::<Result<_>>()? } else { Vec::new() };
    let cells: Vec<(i32, i32)> = per_cell(g, |x| {
        let layer = ((x[2] / period).floor() as usize).min(layers - 1);
        if x[2] - layer as f64 * period >= thickness {
            return (PHASE_MATRIX, 0);
        }
        if polygons {
            let (cell, wall) = cell_and_wall(g, &germs[layer], x[0], x[1]);
            if wall < 0.5 * mortar {
                (PHASE_MATRIX, 0)
            } else {
                (PHASE_SOLID, (layer * per_layer + cell) as i32 + 1)
            }
        } else {
            let off = shift * size * layer as f64;
            let (u, v) = ((x[0] + off) / size, (x[1] + off) / size);
            let edge = |t: f64| (t - t.floor()).min(t.ceil() - t) * size;
            if edge(u) < 0.5 * mortar || edge(v) < 0.5 * mortar {
                (PHASE_MATRIX, 0)
            } else {
                let across = (g.size[0] / size).ceil() as i64 + 2;
                (PHASE_SOLID, ((layer as i64 * across + v.floor() as i64 + 1) * across + u.floor() as i64 + 1).rem_euclid(i32::MAX as i64) as i32 + 1)
            }
        }
    });
    let (ids, _) = crate::synthetic::compact(&cells.iter().map(|c| c.1 as u32).collect::<Vec<_>>());
    // Compacting numbers the mortar too: put it back to 0 and close the gap.
    let mortar_id = cells.iter().zip(&ids).find(|(c, _)| c.1 == 0).map(|(_, i)| *i);
    let ids: Vec<i32> = ids
        .iter()
        .map(|i| match mortar_id {
            Some(m) if *i == m => 0,
            Some(m) if *i > m => *i - 1,
            _ => *i,
        })
        .collect();
    let mut img = phase_image(g, cells.iter().map(|c| c.0).collect(), Some(ids));
    note(&mut img, "Layers", vec![layers as f64]);
    Ok(DataSet::ImageData(img))
}

/// A twisted plywood: layers of parallel fibres, the direction of the fibres turning by the
/// pitch angle from each layer to the next (the cuticle of arthropods, the scales of fish).
pub fn twisted_plywood(g: &Grid, radius: f64, spacing: f64, layer: f64, pitch_deg: f64, start_deg: f64) -> Result<DataSet> {
    if !(radius > 0.0 && spacing > 0.0 && layer > 0.0) {
        bail!("the fibre radius, the spacing and the layer thickness must be above 0");
    }
    let reach = (radius / layer).ceil() as i64;
    let cells: Vec<(i32, i32)> = per_cell(g, |x| {
        let here = (x[2] / layer).floor() as i64;
        for k in here - reach..=here + reach {
            let angle = (start_deg + k as f64 * pitch_deg).to_radians();
            let across = -x[0] * angle.sin() + x[1] * angle.cos();
            let du = across - (across / spacing).round() * spacing;
            let dz = x[2] - (k as f64 + 0.5) * layer;
            if du * du + dz * dz <= radius * radius {
                return (PHASE_SOLID, (k.rem_euclid(1 << 20) + 1) as i32);
            }
        }
        (PHASE_MATRIX, 0)
    });
    let mut img = phase_image(g, cells.iter().map(|c| c.0).collect(), None);
    img.cell_data.insert(Array::new("Layer", 1, ArrayData::I32(cells.iter().map(|c| c.1).collect())));
    note(&mut img, "ExpectedFraction", vec![(PI * radius * radius / (spacing * layer)).min(1.0)]);
    Ok(DataSet::ImageData(img))
}

pub const COMB_HEXAGON: &str = "Hexagons";
pub const COMB_SQUARE: &str = "Squares";
pub const COMB_RANDOM: &str = "Random cells";
pub const COMB_WOOD: &str = "Wood";

pub struct HoneycombParams {
    pub arrangement: String,
    pub cell: f64,
    pub wall: f64,
    /// Wood: the width of a growth ring along X, the share of it that is latewood, the size
    /// along X of the latewood cells and the thickness of their walls, both over those of the
    /// earlywood; the share of the volume taken by rays and their height along Z.
    pub ring: f64,
    pub latewood: f64,
    pub latewood_size: f64,
    pub latewood_wall: f64,
    pub rays: f64,
    pub ray_height: f64,
    pub seed: u64,
}

/// Prismatic cells along Z with walls of a given thickness: hexagons, squares, the cells of
/// random points, or the files of softwood (cells in rows along X whose size and walls
/// change through each growth ring, with rays between the rows).
pub fn honeycomb(g: &Grid, p: &HoneycombParams) -> Result<DataSet> {
    if !(p.cell > p.wall && p.wall > 0.0) {
        bail!("the cells must be larger than their walls are thick, and the walls above 0");
    }
    let mut r = Rng::new(p.seed);
    let d = p.cell;
    let phases: Vec<i32> = match p.arrangement.as_str() {
        COMB_SQUARE => per_cell(g, |x| {
            let edge = |t: f64| (t / d - (t / d).round()).abs() * d;
            if edge(x[0]) < 0.5 * p.wall || edge(x[1]) < 0.5 * p.wall {
                PHASE_SOLID
            } else {
                PHASE_MATRIX
            }
        }),
        COMB_RANDOM => {
            let count = ((g.size[0] * g.size[1] / (d * d)).round() as usize).max(2);
            let germs = planar_germs(g, count, 0.0, &mut r)?;
            per_cell(g, |x| if cell_and_wall(g, &germs, x[0], x[1]).1 < 0.5 * p.wall { PHASE_SOLID } else { PHASE_MATRIX })
        }
        COMB_WOOD => wood(g, p, &mut r),
        _ => {
            // Hexagons: the cells of the points of a triangular lattice, a distance `d` apart.
            let row = d * 3f64.sqrt() / 2.0;
            per_cell(g, |x| {
                let j0 = (x[1] / row).round() as i64;
                let mut best = [f64::INFINITY; 2];
                let mut at = [[0.0; 2]; 2];
                for j in j0 - 1..=j0 + 1 {
                    let off = if j.rem_euclid(2) == 1 { 0.5 * d } else { 0.0 };
                    let i0 = ((x[0] - off) / d).round() as i64;
                    for i in i0 - 1..=i0 + 1 {
                        let s = [i as f64 * d + off, j as f64 * row];
                        let dist = (x[0] - s[0]).powi(2) + (x[1] - s[1]).powi(2);
                        if dist < best[0] {
                            best[1] = best[0];
                            at[1] = at[0];
                            best[0] = dist;
                            at[0] = s;
                        } else if dist < best[1] {
                            best[1] = dist;
                            at[1] = s;
                        }
                    }
                }
                let apart = ((at[1][0] - at[0][0]).powi(2) + (at[1][1] - at[0][1]).powi(2)).sqrt();
                if (best[1] - best[0]) / (2.0 * apart) < 0.5 * p.wall {
                    PHASE_SOLID
                } else {
                    PHASE_MATRIX
                }
            })
        }
    };
    Ok(DataSet::ImageData(phase_image(g, phases, None)))
}

/// Softwood: files of cells along X (the radial direction), a file every `cell` along Y. In
/// each growth ring the cells shorten and their walls thicken from earlywood to latewood.
/// Rays (phase 3) are ribbons between two files, across the ring, over a height along Z.
fn wood(g: &Grid, p: &HoneycombParams, r: &mut dyn Draws) -> Vec<i32> {
    let d = p.cell;
    let ring = p.ring.max(2.0 * d);
    let late = p.latewood.clamp(0.0, 0.95);
    // The walls across a ring: early cells of length d, then late cells of length d x size.
    let early_cells = (((1.0 - late) * ring / d).round() as usize).max(1);
    let late_len = (p.latewood_size.clamp(0.05, 1.0)) * d;
    let late_cells = if late > 0.0 { ((late * ring / late_len).round() as usize).max(1) } else { 0 };
    let early_len = (1.0 - late) * ring / early_cells as f64;
    let late_len = if late_cells > 0 { late * ring / late_cells as f64 } else { 0.0 };
    let files = (g.size[1] / d).ceil() as usize + 1;
    // Each file starts its cells a little differently: the walls across are staggered.
    let stagger: Vec<f64> = (0..files).map(|_| r.random()).collect();
    let ray_width = 0.5 * d;
    let ray_count = if p.rays > 0.0 && p.ray_height > 0.0 { (p.rays * g.size[1] * g.size[2] / (ray_width * p.ray_height)).round() as usize } else { 0 };
    // Rays are added one after another, none within one ray cell along Z of a ray on the same
    // boundary between two files or on the next one.
    let ray_cell = (0.5 * d).max(3.0 * g.h);
    let mut rays: Vec<(usize, f64)> = Vec::new();
    let mut tries = 0;
    while rays.len() < ray_count && tries < 200 * ray_count {
        tries += 1;
        let new = (((r.random() * files as f64) as usize).min(files - 1), r.uniform(0.0, g.size[2]));
        let apart = |z: f64| {
            let dz = new.1 - z;
            (if g.periodic { dz - g.size[2] * (dz / g.size[2]).round() } else { dz }).abs()
        };
        if rays.iter().all(|(f, z)| f.abs_diff(new.0) > 1 || apart(*z) > p.ray_height + ray_cell) {
            rays.push(new);
        }
    }
    // The walls of the cells of a ray: thinner than those of the files, never under a cell.
    let ray_wall = (0.5 * p.wall).max(g.h).min(0.2 * ray_width.max(ray_cell));
    per_cell(g, |x| {
        let file = ((x[1] / d).floor() as usize).min(files - 1);
        for (f, z) in &rays {
            let mut dz = x[2] - z;
            if g.periodic {
                dz -= g.size[2] * (dz / g.size[2]).round();
            }
            let across = (x[1] - *f as f64 * d).abs();
            if across < 0.5 * ray_width && dz.abs() < 0.5 * p.ray_height {
                // Cells in a row along X, stacked along Z, each a box with its own walls.
                let long = 2.0 * d;
                let s = (x[0] + 0.37 * long * (*f as f64)).rem_euclid(long);
                let rows = (p.ray_height / ray_cell).round().max(1.0);
                let tall = p.ray_height / rows;
                let v = (dz + 0.5 * p.ray_height).rem_euclid(tall);
                let wall = across > 0.5 * ray_width - ray_wall || s.min(long - s) < 0.5 * ray_wall || v.min(tall - v) < 0.5 * ray_wall;
                return if wall { 3 } else { PHASE_MATRIX };
            }
        }
        let u = x[0].rem_euclid(ring);
        let early_span = early_cells as f64 * early_len;
        let (len, wall, start) = if u < early_span { (early_len, p.wall, 0.0) } else { (late_len, p.wall * p.latewood_wall.max(0.1), early_span) };
        let wall = wall.min(0.9 * len).min(0.9 * d);
        // Walls along the file (between two files) and across it (between two cells).
        let side = (x[1] - file as f64 * d).min((file + 1) as f64 * d - x[1]);
        let s = if u < early_span { (u - start + stagger[file] * len).rem_euclid(len) } else { (u - start).rem_euclid(len) };
        let across = s.min(len - s);
        if side < 0.5 * wall || across < 0.5 * wall {
            PHASE_SOLID
        } else {
            PHASE_MATRIX
        }
    })
}

/// Haversian bone: osteons, cylinders along Z with a canal on their axis and a cement line
/// around them, arrive one after another at random places and cover what was there; the
/// bone they leave uncovered is the interstitial bone. Phases: 1 interstitial bone,
/// 2 the lamellar bone of the osteons, 3 cement lines, 4 canals.
pub fn haversian_bone(g: &Grid, radius: f64, radius_sd: f64, coverage: f64, canal: f64, cement: f64, seed: u64) -> Result<DataSet> {
    if !(0.0..1.0).contains(&coverage) {
        bail!("the coverage must be at least 0 and below 1");
    }
    if !(radius > 0.0) {
        bail!("the radius must be above 0");
    }
    let mut r = Rng::new(seed);
    let mean_square = radius * radius + radius_sd * radius_sd;
    let reach = radius + 3.0 * radius_sd;
    let m = if g.periodic { 0.0 } else { reach };
    let count = poisson(-(1.0 - coverage).ln() / (PI * mean_square) * (g.size[0] + 2.0 * m) * (g.size[1] + 2.0 * m), &mut r);
    let mut phases = vec![PHASE_MATRIX; g.len()];
    let mut ids = vec![0i32; g.len()];
    let mut expected = [0.0f64; 2];
    // From the last osteon to the first: a cell belongs to the first that reaches it.
    for k in 0..count {
        let c = [r.uniform(-m, g.size[0] + m), r.uniform(-m, g.size[1] + m)];
        let rad = (radius + radius_sd * r.std_normal().clamp(-3.0, 3.0)).max(0.2 * radius);
        let line = (rad - cement).max(canal * rad);
        expected[0] += rad * rad - line * line;
        expected[1] += rad * rad;
        g.stamp([c[0] - rad, c[1] - rad, 0.0], [c[0] + rad, c[1] + rad, g.size[2]], |cell, x| {
            let d = ((x[0] - c[0]).powi(2) + (x[1] - c[1]).powi(2)).sqrt();
            if d <= rad && ids[cell] == 0 {
                ids[cell] = k as i32 + 1;
                phases[cell] = if d < canal * rad {
                    4
                } else if d > line {
                    3
                } else {
                    2
                };
            }
        });
    }
    let mut img = phase_image(g, phases, Some(ids));
    // What the model gives exactly: the top osteon over a point is drawn by its area.
    let cement_share = if expected[1] > 0.0 { expected[0] / expected[1] } else { 0.0 };
    let canals = coverage * canal * canal;
    note(&mut img, "ExpectedFractions", vec![0.0, 1.0 - coverage, coverage * (1.0 - cement_share) - canals, coverage * cement_share, canals]);
    note(&mut img, "Osteons", vec![count as f64]);
    Ok(DataSet::ImageData(img))
}

pub(crate) fn phases_of(ds: &DataSet, array: &str) -> Result<(ImageData, Vec<i32>)> {
    let DataSet::ImageData(img) = ds else { bail!("the input must be an image (a grid of cells)") };
    let name = if array.is_empty() { "Phases" } else { array };
    let a = img.cell_data.get(name).ok_or_else(|| anyhow!("the input has no cell array {name}"))?;
    let phases = (0..a.len()).map(|c| a.get(c, 0) as i32).collect();
    Ok((img.clone(), phases))
}

pub(crate) fn refresh_fractions(img: &mut ImageData, phases: Vec<i32>) {
    let top = phases.iter().copied().max().unwrap_or(1).max(1) as usize;
    let mut count = vec![0usize; top + 1];
    phases.iter().for_each(|p| count[(*p).max(0) as usize] += 1);
    let total = phases.len().max(1) as f64;
    img.field_data.insert(Array::scalars_f64("PhaseFractions", count.iter().map(|c| *c as f64 / total).collect()));
    img.cell_data.insert(Array::new("Phases", 1, ArrayData::I32(phases)));
}

/// Coat Phase: a layer of a new phase around a phase of a microstructure (the cells within
/// `thickness` of it), or inside it (a shell; the cells of the phase within `thickness` of
/// its surface).
pub fn coat_phase(ds: &DataSet, array: &str, phase: i32, thickness: f64, inside: bool, new_phase: i32, periodic: bool) -> Result<DataSet> {
    let (mut img, phases) = phases_of(ds, array)?;
    if !phases.contains(&phase) {
        bail!("no cell of phase {phase}");
    }
    let v = crate::features::Vox::of(&img);
    // Through periodic faces: the image is wrapped by the thickness on every side.
    let pad: [usize; 3] = std::array::from_fn(|k| if periodic && v.n[k] > 1 { ((thickness / v.sp[k]).ceil() as usize + 1).min(v.n[k]) } else { 0 });
    let n: [usize; 3] = std::array::from_fn(|k| v.n[k] + 2 * pad[k]);
    let wide = crate::features::Vox { n, sp: v.sp, corner: v.corner, flat: v.flat };
    let source = |c: usize| -> usize {
        let p = [c % n[0], (c / n[0]) % n[1], c / (n[0] * n[1])];
        let q: [usize; 3] = std::array::from_fn(|k| (p[k] + v.n[k] - pad[k]) % v.n[k]);
        q[0] + v.n[0] * (q[1] + v.n[1] * q[2])
    };
    // The distance to the phase (outside), or to what is not the phase (inside).
    let seed: Vec<bool> = (0..n[0] * n[1] * n[2]).map(|c| (phases[source(c)] == phase) != inside).collect();
    let d2 = crate::features::distance_transform(&wide, &seed);
    let new_phase = if new_phase > 0 { new_phase } else { phases.iter().copied().max().unwrap_or(1) + 1 };
    let mut out = phases.clone();
    let limit = thickness * thickness;
    for c in 0..n[0] * n[1] * n[2] {
        let p = [c % n[0], (c / n[0]) % n[1], c / (n[0] * n[1])];
        if (0..3).any(|k| p[k] < pad[k] || p[k] >= pad[k] + v.n[k]) {
            continue;
        }
        let s = source(c);
        if (phases[s] == phase) == inside && d2[c] > 0.0 && d2[c] <= limit {
            out[s] = new_phase;
        }
    }
    refresh_fractions(&mut img, out);
    Ok(DataSet::ImageData(img))
}

pub const COMBINE_UNION: &str = "Union";
pub const COMBINE_INTERSECTION: &str = "Intersection";
pub const COMBINE_DIFFERENCE: &str = "Difference";
pub const COMBINE_OVERLAY: &str = "Inclusions over the first";
pub const COMBINE_NESTED: &str = "Second inside the first";

/// Combine Microstructures: two images on the same grid, a phase of each. Union,
/// intersection and difference give two phases (2 where the operation holds). Inclusions
/// over the first keeps the first microstructure and puts the phase of the second over it as
/// a new phase. Second inside the first keeps the phase of the first only where the second
/// has its phase too (a lattice made of a foam, spheres within struts).
pub fn combine(a: &DataSet, b: &DataSet, operation: &str, phase_a: i32, phase_b: i32) -> Result<DataSet> {
    let (mut img, pa) = phases_of(a, "")?;
    let (other, pb) = phases_of(b, "")?;
    if img.extent != other.extent {
        bail!("the two microstructures are not on the same grid ({:?} and {:?} cells)", crate::features::Vox::of(&img).n, crate::features::Vox::of(&other).n);
    }
    let new_phase = pa.iter().copied().max().unwrap_or(1) + 1;
    let out: Vec<i32> = pa
        .iter()
        .zip(&pb)
        .map(|(x, y)| {
            let (ina, inb) = (*x == phase_a, *y == phase_b);
            let two = |on: bool| if on { PHASE_SOLID } else { PHASE_MATRIX };
            match operation {
                COMBINE_INTERSECTION => two(ina && inb),
                COMBINE_DIFFERENCE => two(ina && !inb),
                COMBINE_OVERLAY => {
                    if inb {
                        new_phase
                    } else {
                        *x
                    }
                }
                COMBINE_NESTED => {
                    if ina && !inb {
                        PHASE_MATRIX
                    } else {
                        *x
                    }
                }
                _ => two(ina || inb),
            }
        })
        .collect();
    if operation != COMBINE_OVERLAY && operation != COMBINE_NESTED {
        // The objects of the first no longer match the phases.
        img.cell_data.remove("FeatureIds");
    }
    refresh_fractions(&mut img, out);
    Ok(DataSet::ImageData(img))
}
