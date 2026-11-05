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

/// Write `objects` to `path` in the format its extension names: `.glb`, `.obj`, `.x3d` or
/// `.wrl` (VRML 2.0).
pub fn export(path: &Path, objects: &[ExportObject]) -> Result<()> {
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase();
    let objects: Vec<&ExportObject> = objects.iter().filter(|o| exported(o.draw)).collect();
    if objects.is_empty() {
        bail!("nothing to export: no visible surfaces, lines or points");
    }
    match ext.as_str() {
        "glb" => write_glb(path, &objects),
        "obj" => write_obj(path, &objects),
        "x3d" => write_x3d(path, &objects, false),
        "wrl" | "vrml" => write_x3d(path, &objects, true),
        _ => bail!("unknown scene format .{ext} (use .glb, .obj, .x3d or .wrl)"),
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
            "pbrMetallicRoughness": {
                "baseColorFactor": [1.0, 1.0, 1.0, opacity],
                "metallicFactor": d.display.metallic.clamp(0.0, 1.0),
                "roughnessFactor": d.display.roughness.clamp(0.0, 1.0),
            },
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

/// X3D (XML) or VRML 2.0: one shape per kind of geometry, vertex colors, absolute coordinates
/// (some readers ignore transforms). Unlit objects get no material (drawn flat).
fn write_x3d(path: &Path, objects: &[&ExportObject], vrml: bool) -> Result<()> {
    use std::fmt::Write as _;
    let mut out = String::new();
    let nums = |v: &mut String, it: &mut dyn Iterator<Item = [f64; 3]>| {
        for (i, p) in it.enumerate() {
            if i > 0 {
                v.push_str(if vrml { ", " } else { " " });
            }
            let _ = write!(v, "{} {} {}", p[0], p[1], p[2]);
        }
    };
    if vrml {
        let _ = writeln!(out, "#VRML V2.0 utf8\n# auraFX {} scene export", env!("CARGO_PKG_VERSION"));
    } else {
        let _ = writeln!(out, "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<X3D profile=\"Interchange\" version=\"3.3\">\n<head><meta name=\"generator\" content=\"auraFX {}\"/></head>\n<Scene>", env!("CARGO_PKG_VERSION"));
    }
    for o in objects {
        let d = o.draw;
        let m = Material::of(d);
        let origin = d.mesh.origin;
        let transparency = 1.0 - d.display.opacity.clamp(0.0, 1.0);
        let name: String = o.name.chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '_' }).collect();
        if vrml {
            let _ = writeln!(out, "DEF {name} Group {{\n  children [");
        } else {
            let _ = writeln!(out, "<Group DEF=\"{name}\">");
        }
        for (verts, kind) in [(&d.mesh.tris, 3usize), (&d.mesh.lines, 2), (&d.mesh.points, 1)] {
            let n = verts.len() / kind * kind;
            if n == 0 {
                continue;
            }
            let mut pts = String::new();
            nums(&mut pts, &mut verts[..n].iter().map(|v| [v.pos[0] as f64 + origin[0], v.pos[1] as f64 + origin[1], v.pos[2] as f64 + origin[2]]));
            let mut cols = String::new();
            nums(
                &mut cols,
                &mut verts[..n].iter().map(|v| {
                    let c = m.color(v.scalar);
                    [linear_to_srgb(c.x) as f64, linear_to_srgb(c.y) as f64, linear_to_srgb(c.z) as f64]
                }),
            );
            let lit = d.display.lighting && kind == 3;
            if vrml {
                let _ = writeln!(out, "    Shape {{");
                if lit {
                    let _ = writeln!(out, "      appearance Appearance {{ material Material {{ transparency {transparency} }} }}");
                }
                let geom = match kind {
                    3 => "IndexedFaceSet",
                    2 => "IndexedLineSet",
                    _ => "PointSet",
                };
                let _ = writeln!(out, "      geometry {geom} {{\n        coord Coordinate {{ point [ {pts} ] }}\n        color Color {{ color [ {cols} ] }}");
                if kind > 1 {
                    let idx: Vec<String> = (0..n / kind).map(|k| (0..kind).map(|j| (k * kind + j).to_string()).collect::<Vec<_>>().join(" ") + " -1").collect();
                    let _ = writeln!(out, "        coordIndex [ {} ]", idx.join(" "));
                }
                if kind == 3 {
                    let _ = writeln!(out, "        solid FALSE");
                }
                let _ = writeln!(out, "      }}\n    }}");
            } else {
                let _ = writeln!(out, "  <Shape>");
                if lit {
                    let _ = writeln!(out, "    <Appearance><Material transparency=\"{transparency}\"/></Appearance>");
                }
                let inner = format!("<Coordinate point=\"{pts}\"/><Color color=\"{cols}\"/>");
                match kind {
                    3 => {
                        let _ = writeln!(out, "    <TriangleSet solid=\"false\">{inner}</TriangleSet>");
                    }
                    2 => {
                        let counts = vec!["2"; n / 2].join(" ");
                        let _ = writeln!(out, "    <LineSet vertexCount=\"{counts}\">{inner}</LineSet>");
                    }
                    _ => {
                        let _ = writeln!(out, "    <PointSet>{inner}</PointSet>");
                    }
                }
                let _ = writeln!(out, "  </Shape>");
            }
        }
        let _ = writeln!(out, "{}", if vrml { "  ]\n}" } else { "</Group>" });
    }
    if !vrml {
        out.push_str("</Scene>\n</X3D>\n");
    }
    std::fs::write(path, out).with_context(|| format!("writing {}", path.display()))?;
    Ok(())
}

/// A flat-shaded 2D primitive of a vector picture (pixel coordinates, sRGB color).
#[derive(Debug, Clone)]
pub struct VectorPrim {
    pub points: Vec<[f32; 2]>,
    pub color: [u8; 3],
    pub alpha: f32,
    /// Points: radius; lines: width (pixels).
    pub size: f32,
    depth: f32,
}

/// The visible surfaces, lines and points projected to a `w`×`h` picture, each triangle shaded
/// flat with the view's lights, sorted back to front (painter's algorithm).
pub fn vector_scene(draws: &[NodeDraw], camera: &crate::Camera, lighting: &aurafx_pipeline::Lighting, w: u32, h: u32) -> Vec<VectorPrim> {
    use glam::{DVec3, DVec4, Vec3};
    let aspect = w as f64 / h.max(1) as f64;
    let (lights, ambient) = lighting.effective_lights();
    let f = (camera.eye - camera.target).normalize_or_zero();
    let r = camera.right();
    let u = r.cross(-f).normalize_or_zero();
    let lights: Vec<(Vec3, Vec3)> = lights
        .iter()
        .map(|l| {
            let d = DVec3::new(l.direction[0] as f64, l.direction[1] as f64, l.direction[2] as f64);
            let wd = if l.camera_relative { r * d.x + u * d.y + f * d.z } else { d };
            (wd.normalize_or_zero().as_vec3(), Vec3::from_array(l.color) * l.intensity)
        })
        .collect();
    let mut out = Vec::new();
    for d in draws.iter().filter(|d| exported(d)) {
        let m = Material::of(d);
        let origin = DVec3::from_array(d.mesh.origin);
        let vp = camera.proj_matrix(aspect) * camera.view_matrix();
        let eye = camera.eye;
        // Pixel position and depth, None when behind the camera or outside the picture.
        let project = |p: [f32; 3]| -> Option<([f32; 2], f32)> {
            let wp = origin + DVec3::new(p[0] as f64, p[1] as f64, p[2] as f64);
            let c = vp * DVec4::new(wp.x, wp.y, wp.z, 1.0);
            if c.w <= 0.0 {
                return None;
            }
            let n = c.truncate() / c.w;
            Some(([((n.x + 1.0) * 0.5 * w as f64) as f32, ((1.0 - n.y) * 0.5 * h as f64) as f32], n.z as f32))
        };
        let inside = |pts: &[[f32; 2]]| {
            let (lo, hi) = pts
                .iter()
                .fold(([f32::INFINITY; 2], [f32::NEG_INFINITY; 2]), |(lo, hi), p| ([lo[0].min(p[0]), lo[1].min(p[1])], [hi[0].max(p[0]), hi[1].max(p[1])]));
            hi[0] >= 0.0 && hi[1] >= 0.0 && lo[0] <= w as f32 && lo[1] <= h as f32
        };
        let alpha = d.display.opacity.clamp(0.0, 1.0);
        let srgb = |c: Vec3| [(linear_to_srgb(c.x) * 255.0 + 0.5) as u8, (linear_to_srgb(c.y) * 255.0 + 0.5) as u8, (linear_to_srgb(c.z) * 255.0 + 0.5) as u8];
        for t in d.mesh.tris.chunks_exact(3) {
            let (Some(a), Some(b), Some(c)) = (project(t[0].pos), project(t[1].pos), project(t[2].pos)) else { continue };
            let pts = vec![a.0, b.0, c.0];
            let area = (b.0[0] - a.0[0]) * (c.0[1] - a.0[1]) - (c.0[0] - a.0[0]) * (b.0[1] - a.0[1]);
            if area.abs() < 1e-3 || !inside(&pts) {
                continue;
            }
            let albedo = (m.color(t[0].scalar) + m.color(t[1].scalar) + m.color(t[2].scalar)) / 3.0;
            let col = if m.lit {
                let mut n = (Vec3::from_array(t[0].normal) + Vec3::from_array(t[1].normal) + Vec3::from_array(t[2].normal)).normalize_or_zero();
                let center = origin + DVec3::new(t[0].pos[0] as f64, t[0].pos[1] as f64, t[0].pos[2] as f64);
                let view = (eye - center).normalize_or_zero().as_vec3();
                if n.dot(view) < 0.0 {
                    n = -n;
                }
                let mut light = Vec3::splat(ambient);
                for (l, c) in &lights {
                    light += *c * n.dot(*l).max(0.0);
                }
                albedo * light * (1.0 - m.metallic)
            } else {
                albedo
            };
            out.push(VectorPrim { points: pts, color: srgb(col), alpha, size: 0.0, depth: (a.1 + b.1 + c.1) / 3.0 });
        }
        for l in d.mesh.lines.chunks_exact(2) {
            let (Some(a), Some(b)) = (project(l[0].pos), project(l[1].pos)) else { continue };
            if !inside(&[a.0, b.0]) {
                continue;
            }
            let col = (m.color(l[0].scalar) + m.color(l[1].scalar)) * 0.5;
            // Lines a hair in front of the surfaces they lie on.
            out.push(VectorPrim { points: vec![a.0, b.0], color: srgb(col), alpha, size: d.display.line_width.max(0.5), depth: (a.1 + b.1) * 0.5 - 1e-5 });
        }
        for p in &d.mesh.points {
            let Some(a) = project(p.pos) else { continue };
            if !inside(&[a.0]) {
                continue;
            }
            out.push(VectorPrim { points: vec![a.0], color: srgb(m.color(p.scalar)), alpha, size: d.display.point_size * 0.5, depth: a.1 - 1e-5 });
        }
    }
    out.sort_by(|a, b| b.depth.total_cmp(&a.depth));
    out
}
