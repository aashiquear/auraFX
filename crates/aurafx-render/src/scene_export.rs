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
        "html" | "htm" => write_html(path, &objects),
        _ => bail!("unknown scene format .{ext} (use .glb, .obj, .x3d, .wrl or .html)"),
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

/// A web page that shows the scene and lets it be turned, moved and zoomed with the mouse or
/// a finger: one file with the geometry inside (no server, no other files, nothing loaded from
/// the network), drawn with WebGL by a small viewer written for this purpose.
fn write_html(path: &Path, objects: &[&ExportObject]) -> Result<()> {
    use base64::Engine;
    // Everything in one set of arrays per kind: positions about the scene's centre, colors in sRGB.
    let (mut lo, mut hi) = ([f64::INFINITY; 3], [f64::NEG_INFINITY; 3]);
    for o in objects {
        let m = &o.draw.mesh;
        for v in m.tris.iter().chain(&m.lines).chain(&m.points) {
            for k in 0..3 {
                let x = v.pos[k] as f64 + m.origin[k];
                lo[k] = lo[k].min(x);
                hi[k] = hi[k].max(x);
            }
        }
    }
    if lo[0] > hi[0] {
        bail!("nothing to export");
    }
    let centre: [f64; 3] = std::array::from_fn(|k| (lo[k] + hi[k]) / 2.0);
    let radius = (0..3).map(|k| (hi[k] - lo[k]).powi(2)).sum::<f64>().sqrt().max(1e-30) / 2.0;
    let mut bufs: [Vec<f32>; 3] = Default::default(); // triangles, lines, points: x y z nx ny nz r g b lit
    for o in objects {
        let d = o.draw;
        let mat = Material::of(d);
        let lit = if d.display.lighting { 1.0 } else { 0.0 };
        for (k, verts) in [&d.mesh.tris, &d.mesh.lines, &d.mesh.points].into_iter().enumerate() {
            for v in verts.iter() {
                let c = mat.color(v.scalar);
                let n = if k == 0 { v.normal } else { [0.0; 3] };
                bufs[k].extend((0..3).map(|a| ((v.pos[a] as f64 + d.mesh.origin[a] - centre[a]) / radius) as f32));
                bufs[k].extend(n);
                bufs[k].extend([linear_to_srgb(c.x), linear_to_srgb(c.y), linear_to_srgb(c.z), if k == 0 { lit } else { 0.0 }]);
            }
        }
    }
    let b64 = |v: &[f32]| base64::engine::general_purpose::STANDARD.encode(bytemuck::cast_slice::<f32, u8>(v));
    let names: Vec<String> = objects.iter().map(|o| o.name.replace(['<', '>', '&'], " ")).collect();
    let title = if names.len() == 1 { names[0].clone() } else { format!("{} objects", names.len()) };
    let page = HTML_VIEWER
        .replace("@TITLE@", &title)
        .replace("@VERSION@", env!("CARGO_PKG_VERSION"))
        .replace("@RADIUS@", &format!("{:.4}", radius * 2.0))
        .replace("@TRIS@", &b64(&bufs[0]))
        .replace("@LINES@", &b64(&bufs[1]))
        .replace("@POINTS@", &b64(&bufs[2]));
    std::fs::write(path, page).with_context(|| format!("writing {}", path.display()))
}

/// The page of [`write_html`]: the arrays are interleaved 32-bit floats (10 per vertex).
const HTML_VIEWER: &str = r##"<!doctype html>
<html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width, initial-scale=1">
<title>@TITLE@</title>
<style>
html, body { margin: 0; height: 100%; background: #f4f5f7; font: 13px system-ui, sans-serif; color: #333; overflow: hidden; }
canvas { width: 100%; height: 100%; display: block; touch-action: none; cursor: grab; }
#note { position: fixed; left: 10px; bottom: 8px; opacity: .7; user-select: none; }
</style></head><body>
<canvas id="c"></canvas>
<div id="note">@TITLE@ · drag to turn, Shift+drag or two fingers to move, wheel or pinch to zoom, double-click to reset · made with auraFX @VERSION@ (the scene is @RADIUS@ across)</div>
<script>
"use strict";
const data = { tris: "@TRIS@", lines: "@LINES@", points: "@POINTS@" };
function floats(b64) {
  const s = atob(b64), bytes = new Uint8Array(s.length);
  for (let i = 0; i < s.length; i++) bytes[i] = s.charCodeAt(i);
  return new Float32Array(bytes.buffer);
}
const canvas = document.getElementById("c");
const gl = canvas.getContext("webgl", { antialias: true });
if (!gl) { document.getElementById("note").textContent = "This browser cannot draw 3D (WebGL is not available)."; throw new Error("no WebGL"); }
const vs = `attribute vec3 p; attribute vec3 n; attribute vec4 c; uniform mat4 mvp; uniform mat3 rot; uniform float size;
varying vec3 vn; varying vec4 vc;
void main() { vn = rot * n; vc = c; gl_Position = mvp * vec4(p, 1.0); gl_PointSize = size; }`;
const fs = `precision mediump float; varying vec3 vn; varying vec4 vc;
void main() {
  vec3 col = vc.rgb;
  if (vc.a > 0.5) {
    vec3 nn = normalize(vn);
    float d = abs(dot(nn, normalize(vec3(0.3, 0.4, 0.86))));
    col = col * (0.35 + 0.65 * d) + vec3(0.12) * pow(d, 24.0);
  }
  gl_FragColor = vec4(col, 1.0);
}`;
function shader(type, src) {
  const s = gl.createShader(type); gl.shaderSource(s, src); gl.compileShader(s);
  if (!gl.getShaderParameter(s, gl.COMPILE_STATUS)) throw new Error(gl.getShaderInfoLog(s));
  return s;
}
const prog = gl.createProgram();
gl.attachShader(prog, shader(gl.VERTEX_SHADER, vs)); gl.attachShader(prog, shader(gl.FRAGMENT_SHADER, fs));
gl.linkProgram(prog); gl.useProgram(prog);
const loc = { p: gl.getAttribLocation(prog, "p"), n: gl.getAttribLocation(prog, "n"), c: gl.getAttribLocation(prog, "c"),
  mvp: gl.getUniformLocation(prog, "mvp"), rot: gl.getUniformLocation(prog, "rot"), size: gl.getUniformLocation(prog, "size") };
const sets = [["tris", gl.TRIANGLES], ["lines", gl.LINES], ["points", gl.POINTS]].map(([k, mode]) => {
  const a = floats(data[k]), buf = gl.createBuffer();
  gl.bindBuffer(gl.ARRAY_BUFFER, buf); gl.bufferData(gl.ARRAY_BUFFER, a, gl.STATIC_DRAW);
  return { buf, mode, count: a.length / 10 };
});
window.sceneCounts = sets.map(s => s.count);
// The view: a rotation (a 3x3 matrix), a pan and a distance.
let R = [1, 0, 0, 0, 1, 0, 0, 0, 1], pan = [0, 0], dist = 3.2;
function mul3(a, b) { const o = new Array(9); for (let i = 0; i < 3; i++) for (let j = 0; j < 3; j++) o[3 * i + j] = a[3 * i] * b[j] + a[3 * i + 1] * b[3 + j] + a[3 * i + 2] * b[6 + j]; return o; }
function rotX(t) { const c = Math.cos(t), s = Math.sin(t); return [1, 0, 0, 0, c, -s, 0, s, c]; }
function rotY(t) { const c = Math.cos(t), s = Math.sin(t); return [c, 0, s, 0, 1, 0, -s, 0, c]; }
function reset() { R = mul3(rotX(-1.05), [0.7071, 0.7071, 0, -0.7071, 0.7071, 0, 0, 0, 1]); pan = [0, 0]; dist = 3.2; draw(); }
function draw() {
  const dpr = window.devicePixelRatio || 1, w = Math.round(canvas.clientWidth * dpr), h = Math.round(canvas.clientHeight * dpr);
  if (canvas.width !== w || canvas.height !== h) { canvas.width = w; canvas.height = h; }
  gl.viewport(0, 0, w, h); gl.clearColor(0.957, 0.961, 0.969, 1); gl.clear(gl.COLOR_BUFFER_BIT | gl.DEPTH_BUFFER_BIT); gl.enable(gl.DEPTH_TEST);
  const f = 1 / Math.tan(0.35), asp = w / Math.max(h, 1), near = Math.max(dist - 2.5, 0.01), far = dist + 2.5;
  // Column-major: projection * translate(pan, -dist) * rotation.
  const m = new Float32Array(16);
  for (let j = 0; j < 3; j++) {
    m[4 * j] = f / asp * R[j]; m[4 * j + 1] = f * R[3 + j];
    m[4 * j + 2] = (far + near) / (near - far) * R[6 + j]; m[4 * j + 3] = -R[6 + j];
  }
  m[12] = f / asp * pan[0]; m[13] = f * pan[1]; m[14] = (far + near) / (near - far) * (-dist) + 2 * far * near / (near - far); m[15] = dist;
  gl.uniformMatrix4fv(loc.mvp, false, m);
  gl.uniformMatrix3fv(loc.rot, false, new Float32Array([R[0], R[3], R[6], R[1], R[4], R[7], R[2], R[5], R[8]]));
  gl.uniform1f(loc.size, 4 * dpr);
  for (const s of sets) {
    if (!s.count) continue;
    gl.bindBuffer(gl.ARRAY_BUFFER, s.buf);
    gl.enableVertexAttribArray(loc.p); gl.vertexAttribPointer(loc.p, 3, gl.FLOAT, false, 40, 0);
    gl.enableVertexAttribArray(loc.n); gl.vertexAttribPointer(loc.n, 3, gl.FLOAT, false, 40, 12);
    gl.enableVertexAttribArray(loc.c); gl.vertexAttribPointer(loc.c, 4, gl.FLOAT, false, 40, 24);
    gl.drawArrays(s.mode, 0, s.count);
  }
}
const ptrs = new Map(); let pinch = 0;
canvas.addEventListener("pointerdown", e => { canvas.setPointerCapture(e.pointerId); ptrs.set(e.pointerId, [e.clientX, e.clientY]); pinch = 0; });
canvas.addEventListener("pointerup", e => { ptrs.delete(e.pointerId); pinch = 0; });
canvas.addEventListener("pointermove", e => {
  const last = ptrs.get(e.pointerId); if (!last) return;
  const dx = e.clientX - last[0], dy = e.clientY - last[1]; ptrs.set(e.pointerId, [e.clientX, e.clientY]);
  if (ptrs.size >= 2) {
    const [a, b] = [...ptrs.values()], d = Math.hypot(a[0] - b[0], a[1] - b[1]);
    if (pinch) dist = Math.min(40, Math.max(0.2, dist * pinch / d));
    pinch = d; pan[0] += dx * dist / canvas.clientHeight * 0.35; pan[1] -= dy * dist / canvas.clientHeight * 0.35;
  } else if (e.shiftKey || e.buttons === 4) {
    pan[0] += dx * dist / canvas.clientHeight * 0.7; pan[1] -= dy * dist / canvas.clientHeight * 0.7;
  } else {
    R = mul3(mul3(rotX(dy * 0.008), rotY(dx * 0.008)), R);
  }
  draw();
});
canvas.addEventListener("wheel", e => { e.preventDefault(); dist = Math.min(40, Math.max(0.2, dist * Math.exp(e.deltaY * 0.001))); draw(); }, { passive: false });
canvas.addEventListener("dblclick", reset);
window.addEventListener("resize", draw);
reset();
</script></body></html>
"##;

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
