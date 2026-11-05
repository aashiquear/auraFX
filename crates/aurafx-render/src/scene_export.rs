//! Export what a 3D view shows (surfaces, lines and points with their display colors) as a
//! glTF 2.0 binary (`.glb`) or a Wavefront OBJ with vertex colors.

use crate::raytrace::Material;
use crate::{NodeDraw, Vertex};
use anyhow::{bail, Context, Result};
use aurafx_pipeline::Representation;
use std::io::Write;
use std::path::Path;

/// One object of the scene: its name and how it is drawn.
pub struct ExportObject<'a> {
    pub name: String,
    pub draw: &'a NodeDraw,
}

/// Whether an object is exported (volumes are not geometry).
fn exported(d: &NodeDraw) -> bool {
    !d.overlay && d.mesh.volume.is_none() && d.display.representation != Representation::Volume
}

fn linear_to_srgb(c: f32) -> f32 {
    let c = c.clamp(0.0, 1.0);
    if c <= 0.0031308 {
        c * 12.92
    } else {
        1.055 * c.powf(1.0 / 2.4) - 0.055
    }
}

/// Write `objects` to `path` in the format its extension names: `.glb` or `.obj`.
pub fn export(path: &Path, objects: &[ExportObject]) -> Result<()> {
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase();
    let objects: Vec<&ExportObject> = objects.iter().filter(|o| exported(o.draw)).collect();
    if objects.is_empty() {
        bail!("nothing to export: no visible surfaces, lines or points");
    }
    match ext.as_str() {
        "glb" => write_glb(path, &objects),
        "obj" => write_obj(path, &objects),
        _ => bail!("unknown scene format .{ext} (use .glb or .obj)"),
    }
}

/// Vertex colors (linear RGB) of a list of vertices.
fn colors(m: &Material, verts: &[Vertex]) -> Vec<[f32; 3]> {
    verts.iter().map(|v| m.color(v.scalar).to_array()).collect()
}

struct Bin {
    data: Vec<u8>,
    views: Vec<serde_json::Value>,
    accessors: Vec<serde_json::Value>,
}

impl Bin {
    /// Add a float accessor (VEC3) and return its index.
    fn vec3(&mut self, values: &[[f32; 3]], minmax: bool) -> usize {
        while self.data.len() % 4 != 0 {
            self.data.push(0);
        }
        let offset = self.data.len();
        for v in values {
            for c in v {
                self.data.extend_from_slice(&c.to_le_bytes());
            }
        }
        self.views.push(serde_json::json!({"buffer": 0, "byteOffset": offset, "byteLength": values.len() * 12}));
        let mut acc = serde_json::json!({"bufferView": self.views.len() - 1, "componentType": 5126, "count": values.len(), "type": "VEC3"});
        if minmax {
            let mut lo = [f32::INFINITY; 3];
            let mut hi = [f32::NEG_INFINITY; 3];
            for v in values {
                for k in 0..3 {
                    lo[k] = lo[k].min(v[k]);
                    hi[k] = hi[k].max(v[k]);
                }
            }
            acc["min"] = serde_json::json!(lo);
            acc["max"] = serde_json::json!(hi);
        }
        self.accessors.push(acc);
        self.accessors.len() - 1
    }
}

fn write_glb(path: &Path, objects: &[&ExportObject]) -> Result<()> {
    let mut bin = Bin { data: Vec::new(), views: Vec::new(), accessors: Vec::new() };
    let (mut meshes, mut nodes, mut materials) = (Vec::new(), Vec::new(), Vec::new());
    let mut unlit_used = false;
    for o in objects {
        let d = o.draw;
        let m = Material::of(d);
        let opacity = d.display.opacity.clamp(0.0, 1.0);
        let mut mat = serde_json::json!({
            "name": o.name,
            "pbrMetallicRoughness": {"baseColorFactor": [1.0, 1.0, 1.0, opacity], "metallicFactor": 0.0, "roughnessFactor": 0.8},
            "doubleSided": true,
        });
        if opacity < 1.0 {
            mat["alphaMode"] = "BLEND".into();
        }
        if !d.display.lighting {
            mat["extensions"] = serde_json::json!({"KHR_materials_unlit": {}});
            unlit_used = true;
        }
        materials.push(mat);
        let material = materials.len() - 1;
        let mut prims = Vec::new();
        // (vertices, glTF mode, with normals)
        for (verts, mode, normals) in [(&d.mesh.tris, 4, true), (&d.mesh.lines, 1, false), (&d.mesh.points, 0, false)] {
            if verts.is_empty() {
                continue;
            }
            let pos: Vec<[f32; 3]> = verts.iter().map(|v| v.pos).collect();
            let p = bin.vec3(&pos, true);
            let c = bin.vec3(&colors(&m, verts), false);
            let mut attrs = serde_json::json!({"POSITION": p, "COLOR_0": c});
            if normals {
                let nrm: Vec<[f32; 3]> = verts
                    .iter()
                    .map(|v| {
                        let n = glam::Vec3::from_array(v.normal).normalize_or_zero();
                        if n == glam::Vec3::ZERO {
                            [0.0, 0.0, 1.0]
                        } else {
                            n.to_array()
                        }
                    })
                    .collect();
                attrs["NORMAL"] = bin.vec3(&nrm, false).into();
            }
            prims.push(serde_json::json!({"attributes": attrs, "mode": mode, "material": material}));
        }
        if prims.is_empty() {
            continue;
        }
        meshes.push(serde_json::json!({"name": o.name, "primitives": prims}));
        // Positions are relative to the mesh origin (precision); the node puts it back.
        nodes.push(serde_json::json!({"name": o.name, "mesh": meshes.len() - 1, "translation": d.mesh.origin}));
    }
    while bin.data.len() % 4 != 0 {
        bin.data.push(0);
    }
    let mut doc = serde_json::json!({
        "asset": {"version": "2.0", "generator": format!("auraFX {}", env!("CARGO_PKG_VERSION"))},
        "scene": 0,
        "scenes": [{"nodes": (0..nodes.len()).collect::<Vec<_>>()}],
        "nodes": nodes,
        "meshes": meshes,
        "materials": materials,
        "accessors": bin.accessors,
        "bufferViews": bin.views,
        "buffers": [{"byteLength": bin.data.len()}],
    });
    if unlit_used {
        doc["extensionsUsed"] = serde_json::json!(["KHR_materials_unlit"]);
    }
    let mut json = serde_json::to_vec(&doc)?;
    while json.len() % 4 != 0 {
        json.push(b' ');
    }
    let total = 12 + 8 + json.len() + 8 + bin.data.len();
    let mut f = std::io::BufWriter::new(std::fs::File::create(path).with_context(|| format!("creating {}", path.display()))?);
    f.write_all(b"glTF")?;
    f.write_all(&2u32.to_le_bytes())?;
    f.write_all(&(total as u32).to_le_bytes())?;
    f.write_all(&(json.len() as u32).to_le_bytes())?;
    f.write_all(b"JSON")?;
    f.write_all(&json)?;
    f.write_all(&(bin.data.len() as u32).to_le_bytes())?;
    f.write_all(b"BIN\0")?;
    f.write_all(&bin.data)?;
    f.flush()?;
    Ok(())
}

fn write_obj(path: &Path, objects: &[&ExportObject]) -> Result<()> {
    let mut f = std::io::BufWriter::new(std::fs::File::create(path).with_context(|| format!("creating {}", path.display()))?);
    writeln!(f, "# auraFX {} scene export: vertex colors are sRGB after the coordinates", env!("CARGO_PKG_VERSION"))?;
    let mut base = 1usize;
    let mut nbase = 1usize;
    for o in objects {
        let d = o.draw;
        let m = Material::of(d);
        let origin = d.mesh.origin;
        writeln!(f, "o {}", o.name.replace(char::is_whitespace, "_"))?;
        for verts in [&d.mesh.tris, &d.mesh.lines, &d.mesh.points] {
            for v in verts.iter() {
                let c = m.color(v.scalar);
                writeln!(
                    f,
                    "v {} {} {} {:.4} {:.4} {:.4}",
                    v.pos[0] as f64 + origin[0],
                    v.pos[1] as f64 + origin[1],
                    v.pos[2] as f64 + origin[2],
                    linear_to_srgb(c.x),
                    linear_to_srgb(c.y),
                    linear_to_srgb(c.z)
                )?;
            }
        }
        for v in &d.mesh.tris {
            writeln!(f, "vn {} {} {}", v.normal[0], v.normal[1], v.normal[2])?;
        }
        let (nt, nl, np) = (d.mesh.tris.len() / 3 * 3, d.mesh.lines.len() / 2 * 2, d.mesh.points.len());
        for t in (0..nt).step_by(3) {
            let (a, b, c) = (base + t, base + t + 1, base + t + 2);
            let (na, nb, nc) = (nbase + t, nbase + t + 1, nbase + t + 2);
            writeln!(f, "f {a}//{na} {b}//{nb} {c}//{nc}")?;
        }
        let lb = base + d.mesh.tris.len();
        for l in (0..nl).step_by(2) {
            writeln!(f, "l {} {}", lb + l, lb + l + 1)?;
        }
        let pb = lb + d.mesh.lines.len();
        for p in 0..np {
            writeln!(f, "p {}", pb + p)?;
        }
        base = pb + d.mesh.points.len();
        nbase += d.mesh.tris.len();
    }
    f.flush()?;
    Ok(())
}
