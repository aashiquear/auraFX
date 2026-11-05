//! PLOT3D grids (`.xyz`, `.x`, `.g`, `.bin`) and Q solutions (`.q`): binary with or without
//! Fortran record markers (either byte order, single/double precision) or ASCII; single or
//! multiple grids; 2-D or 3-D; IBLANK. The layout is found by trying every combination and
//! keeping the one whose size matches the file, as VTK's auto-detection does. Outputs one block
//! per grid with Density, Momentum, StagnationEnergy and Pressure (γ = 1.4), and the free-stream
//! Mach, angle of attack, Reynolds number and time as "Properties".

use anyhow::{anyhow, bail, Context, Result};
use aurafx_data::*;
use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq)]
struct Layout {
    fortran: bool,
    big: bool,
    double: bool,
    multi: bool,
    dim: usize,
    iblank: bool,
}

struct Bytes<'a> {
    b: &'a [u8],
    pos: usize,
    l: Layout,
}

impl Bytes<'_> {
    fn int(&mut self) -> Option<i64> {
        let c = self.b.get(self.pos..self.pos + 4)?;
        self.pos += 4;
        let a = [c[0], c[1], c[2], c[3]];
        Some(if self.l.big { i32::from_be_bytes(a) } else { i32::from_le_bytes(a) } as i64)
    }
    fn real(&mut self) -> Option<f64> {
        if self.l.double {
            let c = self.b.get(self.pos..self.pos + 8)?;
            self.pos += 8;
            let a: [u8; 8] = c.try_into().ok()?;
            Some(if self.l.big { f64::from_be_bytes(a) } else { f64::from_le_bytes(a) })
        } else {
            let c = self.b.get(self.pos..self.pos + 4)?;
            self.pos += 4;
            let a = [c[0], c[1], c[2], c[3]];
            Some((if self.l.big { f32::from_be_bytes(a) } else { f32::from_le_bytes(a) }) as f64)
        }
    }
    /// Start of a record: check the Fortran length marker (if any) against `len`.
    fn open(&mut self, len: usize) -> Option<()> {
        if self.l.fortran {
            (self.int()? as usize == len).then_some(())
        } else {
            Some(())
        }
    }
    fn close(&mut self, len: usize) -> Option<()> {
        self.open(len)
    }
}

/// Grid dimensions under a layout (the header), or None if it doesn't fit.
fn header(b: &mut Bytes) -> Option<Vec<[usize; 3]>> {
    let ng = if b.l.multi {
        b.open(4)?;
        let n = b.int()?;
        b.close(4)?;
        if !(1..=1_000_000).contains(&n) {
            return None;
        }
        n as usize
    } else {
        1
    };
    let len = 4 * ng * b.l.dim;
    b.open(len)?;
    let mut dims = Vec::with_capacity(ng);
    for _ in 0..ng {
        let mut d = [1usize; 3];
        for v in d.iter_mut().take(b.l.dim) {
            let x = b.int()?;
            if !(1..=100_000_000).contains(&x) {
                return None;
            }
            *v = x as usize;
        }
        dims.push(d);
    }
    b.close(len)?;
    Some(dims)
}

fn real_size(l: &Layout) -> usize {
    if l.double {
        8
    } else {
        4
    }
}

fn marker(l: &Layout) -> usize {
    if l.fortran {
        8
    } else {
        0
    }
}

/// Expected size of a grid file under a layout.
fn grid_size(l: &Layout, dims: &[[usize; 3]]) -> usize {
    let head = if l.multi { 4 + marker(l) } else { 0 } + 4 * dims.len() * l.dim + marker(l);
    head + dims.iter().map(|d| d[0] * d[1] * d[2] * (l.dim * real_size(l) + if l.iblank { 4 } else { 0 }) + marker(l)).sum::<usize>()
}

/// Expected size of a Q file under a layout.
fn q_size(l: &Layout, dims: &[[usize; 3]]) -> usize {
    let head = if l.multi { 4 + marker(l) } else { 0 } + 4 * dims.len() * l.dim + marker(l);
    head + dims.iter().map(|d| 4 * real_size(l) + marker(l) + d[0] * d[1] * d[2] * (l.dim + 2) * real_size(l) + marker(l)).sum::<usize>()
}

fn layouts() -> Vec<Layout> {
    let mut out = Vec::new();
    for fortran in [true, false] {
        for big in [false, true] {
            for double in [false, true] {
                for multi in [true, false] {
                    for dim in [3, 2] {
                        for iblank in [false, true] {
                            out.push(Layout { fortran, big, double, multi, dim, iblank });
                        }
                    }
                }
            }
        }
    }
    out
}

/// Find the binary layout of a grid file.
fn detect_grid(data: &[u8]) -> Option<(Layout, Vec<[usize; 3]>)> {
    layouts().into_iter().find_map(|l| {
        let mut b = Bytes { b: data, pos: 0, l };
        let dims = header(&mut b)?;
        (grid_size(&l, &dims) == data.len()).then_some((l, dims))
    })
}

struct Grid {
    dims: [usize; 3],
    pts: Vec<f64>,
    iblank: Option<Vec<i32>>,
}

fn read_binary_grid(data: &[u8], l: Layout, dims: &[[usize; 3]]) -> Result<Vec<Grid>> {
    let mut b = Bytes { b: data, pos: 0, l };
    header(&mut b).ok_or_else(|| anyhow!("bad header"))?;
    let mut out = Vec::new();
    for d in dims {
        let n = d[0] * d[1] * d[2];
        let len = n * (l.dim * real_size(&l) + if l.iblank { 4 } else { 0 });
        b.open(len).ok_or_else(|| anyhow!("bad record"))?;
        let mut comps = vec![vec![0.0; n]; 3];
        for c in comps.iter_mut().take(l.dim) {
            for v in c.iter_mut() {
                *v = b.real().ok_or_else(|| anyhow!("truncated grid"))?;
            }
        }
        let iblank = if l.iblank {
            Some((0..n).map(|_| b.int().map(|v| v as i32)).collect::<Option<Vec<i32>>>().ok_or_else(|| anyhow!("truncated IBLANK"))?)
        } else {
            None
        };
        b.close(len).ok_or_else(|| anyhow!("bad record end"))?;
        out.push(Grid { dims: *d, pts: (0..n).flat_map(|i| [comps[0][i], comps[1][i], comps[2][i]]).collect(), iblank });
    }
    Ok(out)
}

/// ASCII grids: decide the layout from the token count.
fn read_ascii_grid(text: &str) -> Result<(Layout, Vec<Grid>)> {
    let toks: Vec<f64> =
        text.split_whitespace().map(|t| t.replace(['d', 'D'], "e").parse::<f64>().map_err(|_| anyhow!("not a PLOT3D file ({t:?})"))).collect::<Result<_>>()?;
    for multi in [true, false] {
        for dim in [3, 2] {
            for iblank in [false, true] {
                let mut k = 0;
                let ng = if multi {
                    k = 1;
                    *toks.first().ok_or_else(|| anyhow!("empty file"))? as usize
                } else {
                    1
                };
                if ng == 0 || toks.len() < k + ng * dim || toks[k..k + ng * dim].iter().any(|v| *v < 1.0 || v.fract() != 0.0) {
                    continue;
                }
                let dims: Vec<[usize; 3]> = (0..ng)
                    .map(|g| {
                        let mut d = [1usize; 3];
                        for c in 0..dim {
                            d[c] = toks[k + g * dim + c] as usize;
                        }
                        d
                    })
                    .collect();
                let need = k + ng * dim + dims.iter().map(|d| d[0] * d[1] * d[2] * (dim + iblank as usize)).sum::<usize>();
                if need != toks.len() {
                    continue;
                }
                let mut p = k + ng * dim;
                let mut grids = Vec::new();
                for d in &dims {
                    let n = d[0] * d[1] * d[2];
                    let mut comps = vec![vec![0.0; n]; 3];
                    for c in comps.iter_mut().take(dim) {
                        c.copy_from_slice(&toks[p..p + n]);
                        p += n;
                    }
                    let ib = iblank.then(|| {
                        let v = toks[p..p + n].iter().map(|v| *v as i32).collect();
                        p += n;
                        v
                    });
                    grids.push(Grid { dims: *d, pts: (0..n).flat_map(|i| [comps[0][i], comps[1][i], comps[2][i]]).collect(), iblank: ib });
                }
                return Ok((Layout { fortran: false, big: false, double: true, multi, dim, iblank }, grids));
            }
        }
    }
    bail!("not a PLOT3D grid: the numbers don't fit any layout")
}

/// Q solutions: per grid (Mach, alpha, Re, time) and Density, Momentum, StagnationEnergy.
fn read_q(data: &[u8], grid: &Layout, dims: &[[usize; 3]]) -> Result<Vec<([f64; 4], Vec<f64>)>> {
    // Same layout as the grid, else any binary layout that fits.
    let candidates: Vec<Layout> = std::iter::once(Layout { iblank: false, ..*grid })
        .chain(layouts().into_iter().filter(|l| !l.iblank && l.dim == grid.dim && l.multi == grid.multi))
        .collect();
    let l = candidates.into_iter().find(|l| q_size(l, dims) == data.len()).ok_or_else(|| anyhow!("the Q file does not match the grid"))?;
    let mut b = Bytes { b: data, pos: 0, l };
    let qdims = header(&mut b).ok_or_else(|| anyhow!("bad Q header"))?;
    if qdims != dims {
        bail!("the Q file's grids differ from the grid file's");
    }
    let mut out = Vec::new();
    for d in dims {
        let n = d[0] * d[1] * d[2];
        let rs = real_size(&l);
        b.open(4 * rs).ok_or_else(|| anyhow!("bad Q record"))?;
        let props = [b.real().unwrap_or(0.0), b.real().unwrap_or(0.0), b.real().unwrap_or(0.0), b.real().unwrap_or(0.0)];
        b.close(4 * rs).ok_or_else(|| anyhow!("bad Q record"))?;
        let len = n * (l.dim + 2) * rs;
        b.open(len).ok_or_else(|| anyhow!("bad Q record"))?;
        let q: Vec<f64> = (0..n * (l.dim + 2)).map(|_| b.real()).collect::<Option<_>>().ok_or_else(|| anyhow!("truncated Q"))?;
        b.close(len).ok_or_else(|| anyhow!("bad Q record"))?;
        out.push((props, q));
    }
    Ok(out)
}

/// Whether a file is a binary PLOT3D grid (its size fits a layout exactly).
pub fn is_binary_grid(path: &Path) -> bool {
    std::fs::read(path).map(|d| detect_grid(&d).is_some()).unwrap_or(false)
}

/// Read a grid file, and a Q file when given (or found next to it with the same stem).
pub fn read(path: &Path, q: Option<&Path>) -> Result<DataSet> {
    let data = std::fs::read(path).with_context(|| format!("reading {}", path.display()))?;
    let (layout, grids) = match detect_grid(&data) {
        Some((l, dims)) => (l, read_binary_grid(&data, l, &dims)?),
        None => {
            let text = std::str::from_utf8(&data).map_err(|_| anyhow!("{}: not a PLOT3D grid (no binary layout fits its size)", path.display()))?;
            read_ascii_grid(text)?
        }
    };
    let q_path = q.map(|p| p.to_path_buf()).or_else(|| {
        let p = path.with_extension("q");
        p.exists().then_some(p)
    });
    let solution = match &q_path {
        Some(qp) => {
            let qd = std::fs::read(qp).with_context(|| format!("reading {}", qp.display()))?;
            let dims: Vec<[usize; 3]> = grids.iter().map(|g| g.dims).collect();
            Some(read_q(&qd, &layout, &dims).with_context(|| format!("{}", qp.display()))?)
        }
        None => None,
    };
    let mut mb = MultiBlock { blocks: Vec::new(), field_data: Attributes::new() };
    for (gi, g) in grids.into_iter().enumerate() {
        let n = g.dims[0] * g.dims[1] * g.dims[2];
        let mut sg = StructuredGrid {
            extent: [0, g.dims[0] as i64 - 1, 0, g.dims[1] as i64 - 1, 0, g.dims[2] as i64 - 1],
            points: Points::new_f64(g.pts),
            point_data: Attributes::new(),
            cell_data: Attributes::new(),
            field_data: Attributes::new(),
        };
        if let Some(ib) = g.iblank {
            sg.point_data.insert(Array::new("IBlank", 1, ArrayData::I32(ib)));
        }
        if let Some(sol) = &solution {
            let (props, q) = &sol[gi];
            let dim = layout.dim;
            let rho: Vec<f64> = q[..n].to_vec();
            let m: Vec<f64> = (0..n).flat_map(|i| [q[n + i], q[2 * n + i], if dim == 3 { q[3 * n + i] } else { 0.0 }]).collect();
            let e: Vec<f64> = q[(dim + 1) * n..(dim + 2) * n].to_vec();
            let p: Vec<f64> = (0..n)
                .map(|i| {
                    let m2 = m[3 * i].powi(2) + m[3 * i + 1].powi(2) + m[3 * i + 2].powi(2);
                    let r = if rho[i] != 0.0 { rho[i] } else { 1.0 };
                    0.4 * (e[i] - 0.5 * m2 / r)
                })
                .collect();
            sg.point_data.insert_active(AttributeKind::Scalars, Array::scalars_f64("Density", rho));
            sg.point_data.insert(Array::new("Momentum", 3, ArrayData::F64(m)));
            sg.point_data.insert(Array::scalars_f64("StagnationEnergy", e));
            sg.point_data.insert(Array::scalars_f64("Pressure", p));
            sg.field_data.insert(Array::new("Properties", 4, ArrayData::F64(props.to_vec())));
        }
        mb.blocks.push((format!("Grid {}", gi + 1), Some(DataSet::StructuredGrid(sg))));
    }
    Ok(DataSet::MultiBlock(mb))
}
