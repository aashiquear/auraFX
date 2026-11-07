//! Solver mesh formats: Abaqus input (`.inp`) and Gmsh (`.msh`, version 4.1 ASCII).
//!
//! Both take an unstructured grid; element blocks come from an `ElementBlock` cell array (names
//! in the field arrays `ElementBlockIds` and `ElementBlockNames`), side sets from field arrays
//! `SideSet <name>` (cell index, Exodus side 1-6) and node sets from `NodeSet <name>`, as the
//! mesh generator makes them.

use anyhow::{bail, Result};
use aurafx_data::*;
use std::fmt::Write as _;
use std::path::Path;

/// VTK slot -> Exodus node (hexahedra with 20 and 27 nodes).
const VTK_TO_EXODUS_20: [usize; 20] = [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 16, 17, 18, 19, 12, 13, 14, 15];
const VTK_TO_EXODUS_27: [usize; 27] = [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 16, 17, 18, 19, 12, 13, 14, 15, 23, 24, 25, 26, 21, 22, 20];
/// Corners of the Exodus sides of a hexahedron.
const HEX_SIDES: [[usize; 4]; 6] = [[0, 1, 5, 4], [1, 2, 6, 5], [2, 3, 7, 6], [3, 0, 4, 7], [0, 3, 2, 1], [4, 5, 6, 7]];

/// Exodus-ordered node ids of a cell given in VTK order.
fn exodus_order(t: CellType, ids: &[u32]) -> Vec<u32> {
    match (t, ids.len()) {
        (CellType::QuadraticHexahedron, 20) => {
            let mut v = vec![0; 20];
            for (k, e) in VTK_TO_EXODUS_20.iter().enumerate() {
                v[*e] = ids[k];
            }
            v
        }
        (CellType::TriquadraticHexahedron, 27) => {
            let mut v = vec![0; 27];
            for (k, e) in VTK_TO_EXODUS_27.iter().enumerate() {
                v[*e] = ids[k];
            }
            v
        }
        (CellType::Voxel, 8) => vec![ids[0], ids[1], ids[3], ids[2], ids[4], ids[5], ids[7], ids[6]],
        _ => ids.to_vec(),
    }
}

struct Mesh<'a> {
    _ug: std::marker::PhantomData<&'a ()>,
    block: Vec<i32>,
    names: std::collections::BTreeMap<i32, String>,
    side_sets: Vec<(String, Vec<(usize, usize)>)>,
    node_sets: Vec<(String, Vec<usize>)>,
}

fn mesh_of(ds: &DataSet) -> Result<(UnstructuredGrid, Option<()>)> {
    match ds {
        DataSet::UnstructuredGrid(u) => Ok((u.clone(), None)),
        DataSet::MultiBlock(_) => bail!("write one mesh at a time (merge the blocks first)"),
        d => Ok((d.to_unstructured().ok_or_else(|| anyhow::anyhow!("cannot convert to a mesh"))?, None)),
    }
}

impl<'a> Mesh<'a> {
    fn new(ug: &'a UnstructuredGrid) -> Mesh<'a> {
        let n = ug.cells.len();
        let block: Vec<i32> = match ug.cell_data.get("ElementBlock") {
            Some(a) => (0..n).map(|i| a.get(i, 0) as i32).collect(),
            None => vec![1; n],
        };
        let mut names = std::collections::BTreeMap::new();
        if let (Some(ids), Some(ArrayData::String(nm))) = (ug.field_data.get("ElementBlockIds"), ug.field_data.get("ElementBlockNames").map(|a| &a.data)) {
            for i in 0..ids.len() {
                names.insert(ids.get(i, 0) as i32, nm.get(i).cloned().unwrap_or_default());
            }
        }
        for b in &block {
            names.entry(*b).or_insert_with(|| format!("block_{b}"));
        }
        let mut side_sets = Vec::new();
        let mut node_sets = Vec::new();
        for a in ug.field_data.iter() {
            if let Some(nm) = a.name.strip_prefix("SideSet ") {
                side_sets.push((nm.to_string(), (0..a.len()).map(|i| (a.get(i, 0) as usize, a.get(i, 1) as usize)).collect()));
            } else if let Some(nm) = a.name.strip_prefix("NodeSet ") {
                node_sets.push((nm.to_string(), (0..a.len()).map(|i| a.get(i, 0) as usize).collect()));
            }
        }
        Mesh { _ug: std::marker::PhantomData, block, names, side_sets, node_sets }
    }
}

/// A name usable as an Abaqus set or Gmsh group name.
fn clean(name: &str) -> String {
    let s: String = name.chars().map(|c| if c.is_ascii_alphanumeric() || c == '_' || c == '-' || c == '+' { c } else { '_' }).collect();
    let s = s.replace('+', "p").replace('-', "m");
    if s.chars().next().is_some_and(|c| c.is_ascii_alphabetic()) {
        s
    } else {
        format!("S_{s}")
    }
}

fn abaqus_type(t: CellType, n: usize) -> Option<(&'static str, Vec<usize>)> {
    let first = |k: usize| (0..k).collect::<Vec<_>>();
    Some(match (t, n) {
        (CellType::Hexahedron, 8) | (CellType::Voxel, 8) => ("C3D8", first(8)),
        // Abaqus C3D20: corners, bottom edges, top edges, vertical edges (VTK's order).
        (CellType::QuadraticHexahedron, 20) => ("C3D20", first(20)),
        // C3D27: then the face centres of faces S1..S6 and the centre.
        (CellType::TriquadraticHexahedron, 27) => ("C3D27", [first(20), vec![24, 25, 22, 21, 23, 20, 26]].concat()),
        (CellType::Tetra, 4) => ("C3D4", first(4)),
        (CellType::QuadraticTetra, 10) => ("C3D10", first(10)),
        (CellType::Wedge, 6) => ("C3D6", first(6)),
        (CellType::Pyramid, 5) => ("C3D5", first(5)),
        (CellType::Quad, 4) => ("S4", first(4)),
        (CellType::Triangle, 3) => ("S3", first(3)),
        _ => return None,
    })
}

/// Abaqus input file: nodes, elements by block (element set named after the block), node
/// sets, and side sets as element-based surfaces.
pub fn write_abaqus(ds: &DataSet, path: &Path) -> Result<()> {
    let (ug, _) = mesh_of(ds)?;
    let m = Mesh::new(&ug);
    let mut s = String::new();
    writeln!(s, "*Heading\n** Written by auraFX {}", env!("CARGO_PKG_VERSION"))?;
    writeln!(s, "*Node")?;
    for i in 0..ug.points.len() {
        let p = ug.points.get(i);
        writeln!(s, "{}, {:.17e}, {:.17e}, {:.17e}", i + 1, p[0], p[1], p[2])?;
    }
    // Elements grouped by (block, type), numbered in cell order.
    let mut groups: Vec<((i32, &'static str), Vec<(usize, Vec<u32>)>)> = Vec::new();
    for (c, (t, ids)) in ug.cells.iter().enumerate() {
        let Some((name, order)) = abaqus_type(t, ids.len()) else { bail!("{t:?} cells cannot be written to an Abaqus file") };
        let key = (m.block[c], name);
        let row: Vec<u32> = order.iter().map(|k| ids[*k] + 1).collect();
        match groups.iter_mut().find(|(k, _)| *k == key) {
            Some((_, v)) => v.push((c, row)),
            None => groups.push((key, vec![(c, row)])),
        }
    }
    let multi_type = |b: i32| groups.iter().filter(|((bb, _), _)| *bb == b).count() > 1;
    for ((b, ty), rows) in &groups {
        let base = clean(&m.names[b]);
        let set = if multi_type(*b) { format!("{base}_{ty}") } else { base };
        writeln!(s, "*Element, type={ty}, elset={set}")?;
        for (c, row) in rows {
            let mut line = format!("{}", c + 1);
            for (k, n) in row.iter().enumerate() {
                // Lines of at most 16 entries.
                if (k + 1) % 15 == 0 {
                    line.push_str(",\n");
                    write!(line, "{n}")?;
                } else {
                    write!(line, ", {n}")?;
                }
            }
            writeln!(s, "{line}")?;
        }
    }
    let list = |s: &mut String, ids: &[usize]| -> Result<()> {
        for chunk in ids.chunks(16) {
            writeln!(s, "{}", chunk.iter().map(|i| (i + 1).to_string()).collect::<Vec<_>>().join(", "))?;
        }
        Ok(())
    };
    for (name, nodes) in &m.node_sets {
        writeln!(s, "*Nset, nset={}", clean(name))?;
        list(&mut s, nodes)?;
    }
    // Exodus side -> Abaqus face of a hexahedron.
    const FACE: [usize; 7] = [0, 3, 4, 5, 6, 1, 2];
    for (name, faces) in &m.side_sets {
        let base = clean(name);
        let mut by_face: [Vec<usize>; 7] = Default::default();
        for (c, side) in faces {
            if (1..=6).contains(side) {
                by_face[FACE[*side]].push(*c);
            }
        }
        for (f, cells) in by_face.iter().enumerate() {
            if !cells.is_empty() {
                writeln!(s, "*Elset, elset={base}_S{f}")?;
                list(&mut s, cells)?;
            }
        }
        writeln!(s, "*Surface, type=ELEMENT, name={base}")?;
        for (f, cells) in by_face.iter().enumerate() {
            if !cells.is_empty() {
                writeln!(s, "{base}_S{f}, S{f}")?;
            }
        }
    }
    std::fs::write(path, s)?;
    Ok(())
}

fn gmsh_type(t: CellType, n: usize) -> Option<(u32, Vec<usize>)> {
    // Gmsh hexahedron edges: 0-1, 0-3, 0-4, 1-2, 1-5, 2-3, 2-6, 3-7, 4-5, 4-7, 5-6, 6-7;
    // faces 0321, 0154, 0473, 1265, 2376, 4567, then the centre (in Exodus numbering).
    const HEX20: [usize; 20] = [0, 1, 2, 3, 4, 5, 6, 7, 8, 11, 12, 9, 13, 10, 14, 15, 16, 19, 17, 18];
    const HEX27: [usize; 27] = [0, 1, 2, 3, 4, 5, 6, 7, 8, 11, 12, 9, 13, 10, 14, 15, 16, 19, 17, 18, 21, 25, 23, 24, 26, 22, 20];
    let first = |k: usize| (0..k).collect::<Vec<_>>();
    Some(match (t, n) {
        (CellType::Hexahedron, 8) | (CellType::Voxel, 8) => (5, first(8)),
        (CellType::QuadraticHexahedron, 20) => (17, HEX20.to_vec()),
        (CellType::TriquadraticHexahedron, 27) => (12, HEX27.to_vec()),
        (CellType::Tetra, 4) => (4, first(4)),
        (CellType::QuadraticTetra, 10) => (11, vec![0, 1, 2, 3, 4, 5, 6, 7, 9, 8]),
        (CellType::Wedge, 6) => (6, first(6)),
        (CellType::Pyramid, 5) => (7, first(5)),
        (CellType::Quad, 4) => (3, first(4)),
        (CellType::Triangle, 3) => (2, first(3)),
        _ => return None,
    })
}

/// Gmsh 4.1 ASCII mesh: a volume entity and physical group per element block, a surface
/// entity and physical group of quadrilaterals per side set (node sets are not part of the
/// format and are left out).
pub fn write_gmsh(ds: &DataSet, path: &Path) -> Result<()> {
    let (ug, _) = mesh_of(ds)?;
    let m = Mesh::new(&ug);
    let n = ug.points.len();
    let (mut lo, mut hi) = ([f64::INFINITY; 3], [f64::NEG_INFINITY; 3]);
    for i in 0..n {
        let p = ug.points.get(i);
        for k in 0..3 {
            lo[k] = lo[k].min(p[k]);
            hi[k] = hi[k].max(p[k]);
        }
    }
    let blocks: Vec<i32> = m.names.keys().copied().filter(|b| m.block.contains(b)).collect();
    // Surface quads of every side set (corner nodes, Exodus side order).
    let mut surfaces: Vec<(String, Vec<[u32; 4]>)> = Vec::new();
    for (name, faces) in &m.side_sets {
        let mut quads = Vec::new();
        for (c, side) in faces {
            let Some((t, ids)) = ug.cells.iter().nth(*c) else { continue };
            if !matches!(t, CellType::Hexahedron | CellType::Voxel | CellType::QuadraticHexahedron | CellType::TriquadraticHexahedron) || !(1..=6).contains(side) {
                continue;
            }
            let e = exodus_order(t, ids);
            quads.push(HEX_SIDES[side - 1].map(|k| e[k] + 1));
        }
        if !quads.is_empty() {
            surfaces.push((name.clone(), quads));
        }
    }
    let mut s = String::new();
    writeln!(s, "$MeshFormat\n4.1 0 8\n$EndMeshFormat")?;
    writeln!(s, "$PhysicalNames\n{}", blocks.len() + surfaces.len())?;
    for (i, (name, _)) in surfaces.iter().enumerate() {
        writeln!(s, "2 {} \"{}\"", i + 1, name)?;
    }
    for b in &blocks {
        writeln!(s, "3 {} \"{}\"", b, m.names[b])?;
    }
    writeln!(s, "$EndPhysicalNames")?;
    writeln!(s, "$Entities\n0 0 {} {}", surfaces.len(), blocks.len())?;
    let bbox = format!("{} {} {} {} {} {}", lo[0], lo[1], lo[2], hi[0], hi[1], hi[2]);
    for i in 0..surfaces.len() {
        writeln!(s, "{} {bbox} 1 {} 0", i + 1, i + 1)?;
    }
    for b in &blocks {
        writeln!(s, "{b} {bbox} 1 {b} 0")?;
    }
    writeln!(s, "$EndEntities")?;
    // All nodes belong to the first volume entity.
    let first_block = *blocks.first().ok_or_else(|| anyhow::anyhow!("no cells"))?;
    writeln!(s, "$Nodes\n1 {n} 1 {n}\n3 {first_block} 0 {n}")?;
    for i in 0..n {
        writeln!(s, "{}", i + 1)?;
    }
    for i in 0..n {
        let p = ug.points.get(i);
        writeln!(s, "{:.17e} {:.17e} {:.17e}", p[0], p[1], p[2])?;
    }
    writeln!(s, "$EndNodes")?;
    // Elements: per side set (quads), then per block and type.
    let mut sections: Vec<(u32, i32, u32, Vec<Vec<u32>>)> = Vec::new();
    for (i, (_, quads)) in surfaces.iter().enumerate() {
        sections.push((2, i as i32 + 1, 3, quads.iter().map(|q| q.to_vec()).collect()));
    }
    for b in &blocks {
        let mut by_type: Vec<(u32, Vec<Vec<u32>>)> = Vec::new();
        for (c, (t, ids)) in ug.cells.iter().enumerate() {
            if m.block[c] != *b {
                continue;
            }
            let Some((gt, order)) = gmsh_type(t, ids.len()) else { bail!("{t:?} cells cannot be written to a Gmsh file") };
            let e = exodus_order(t, ids);
            let row: Vec<u32> = order.iter().map(|k| e[*k] + 1).collect();
            match by_type.iter_mut().find(|(g, _)| *g == gt) {
                Some((_, v)) => v.push(row),
                None => by_type.push((gt, vec![row])),
            }
        }
        for (gt, rows) in by_type {
            sections.push((3, *b, gt, rows));
        }
    }
    let total: usize = sections.iter().map(|x| x.3.len()).sum();
    writeln!(s, "$Elements\n{} {total} 1 {total}", sections.len())?;
    let mut tag = 1usize;
    for (dim, ent, gt, rows) in &sections {
        writeln!(s, "{dim} {ent} {gt} {}", rows.len())?;
        for r in rows {
            writeln!(s, "{tag} {}", r.iter().map(|x| x.to_string()).collect::<Vec<_>>().join(" "))?;
            tag += 1;
        }
    }
    writeln!(s, "$EndElements")?;
    std::fs::write(path, s)?;
    Ok(())
}
