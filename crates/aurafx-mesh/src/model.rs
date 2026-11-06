//! Bodies, named faces and the geometry command language.
//!
//! Commands name an entity and act on it, one per line (or separated by `;`), `#` starting a
//! comment. Numbers are bodies (`body 2`), outlines (`outline 1`) and faces (`face 5`), counted
//! from 1 in the order they are made. Sizes are in model units, angles in degrees.

use crate::outline::Outline;
use crate::shape::{FaceId, Path, Shape, V3};
use crate::trimesh::TriMesh;
use anyhow::{anyhow, bail, Context, Result};
use std::collections::BTreeMap;
use std::sync::Arc;

/// How a body was made, for the Properties tree.
#[derive(Clone, Debug)]
pub enum History {
    Leaf(String),
    Op(String, Vec<History>),
}

impl History {
    pub fn lines(&self) -> Vec<String> {
        let mut out = Vec::new();
        self.render(0, &mut out);
        out
    }
    fn render(&self, depth: usize, out: &mut Vec<String>) {
        let pad = "  ".repeat(depth);
        match self {
            History::Leaf(s) => out.push(format!("{pad}{s}")),
            History::Op(s, kids) => {
                out.push(format!("{pad}{s}"));
                for k in kids {
                    k.render(depth + 1, out);
                }
            }
        }
    }
}

#[derive(Clone)]
pub struct Body {
    pub id: u32,
    pub name: String,
    pub shape: Shape,
    pub history: History,
}

/// The geometry built by a list of commands.
#[derive(Clone, Default)]
pub struct Model {
    pub bodies: BTreeMap<u32, Body>,
    next_body: u32,
    outlines: BTreeMap<u32, Outline>,
    /// Face names; index 0 is unused (face ids start at 1).
    pub faces: Vec<String>,
    /// The commands run so far.
    pub journal: Vec<String>,
    /// Folder that relative file names are read from.
    pub base_dir: Option<std::path::PathBuf>,
}

/// Split a command into words, keeping quoted text together.
fn words(cmd: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut quote: Option<char> = None;
    let mut had = false;
    for c in cmd.chars() {
        match quote {
            Some(q) if c == q => {
                quote = None;
            }
            Some(_) => cur.push(c),
            None if c == '\'' || c == '"' => {
                quote = Some(c);
                had = true;
            }
            None if c.is_whitespace() || c == ',' => {
                if !cur.is_empty() || had {
                    out.push(std::mem::take(&mut cur));
                    had = false;
                }
            }
            None => cur.push(c),
        }
    }
    if !cur.is_empty() || had {
        out.push(cur);
    }
    out
}

/// The words of one command with helpers to read keyword values.
struct Args {
    w: Vec<String>,
}

impl Args {
    fn has(&self, k: &str) -> bool {
        self.w.iter().any(|x| x.eq_ignore_ascii_case(k))
    }
    fn pos(&self, k: &str) -> Option<usize> {
        self.w.iter().position(|x| x.eq_ignore_ascii_case(k))
    }
    /// The number after keyword `k`.
    fn num(&self, k: &str) -> Result<Option<f64>> {
        match self.pos(k) {
            Some(i) => {
                let v = self.w.get(i + 1).ok_or_else(|| anyhow!("{k} needs a number"))?;
                Ok(Some(v.parse().map_err(|_| anyhow!("{k}: '{v}' is not a number"))?))
            }
            None => Ok(None),
        }
    }
    fn need(&self, k: &str) -> Result<f64> {
        self.num(k)?.ok_or_else(|| anyhow!("missing {k}"))
    }
    /// The numbers following keyword `k` (up to the next word that is not a number).
    fn nums(&self, k: &str) -> Vec<f64> {
        let Some(i) = self.pos(k) else { return Vec::new() };
        self.w[i + 1..].iter().map_while(|x| x.parse::<f64>().ok()).collect()
    }
    fn text(&self, k: &str) -> Option<String> {
        self.pos(k).and_then(|i| self.w.get(i + 1).cloned())
    }
    /// Ids after `body` (or `bodies`), as many as there are numbers.
    fn ids(&self, k: &str) -> Vec<u32> {
        let i = match self.pos(k).or_else(|| self.pos(&format!("{k}s")).or_else(|| self.pos(&format!("{k}ies")))) {
            Some(i) => i,
            None => return Vec::new(),
        };
        self.w[i + 1..].iter().map_while(|x| x.parse::<u32>().ok()).collect()
    }
}

fn pairs(v: &[f64], what: &str) -> Result<Vec<[f64; 2]>> {
    if v.len() < 2 || v.len() % 2 != 0 {
        bail!("{what} takes x y pairs");
    }
    Ok(v.chunks(2).map(|c| [c[0], c[1]]).collect())
}

fn rotation(axis: V3, deg: f64) -> [[f64; 3]; 3] {
    let n = (axis[0] * axis[0] + axis[1] * axis[1] + axis[2] * axis[2]).sqrt().max(1e-300);
    let (x, y, z) = (axis[0] / n, axis[1] / n, axis[2] / n);
    let (s, c) = deg.to_radians().sin_cos();
    let t = 1.0 - c;
    [[t * x * x + c, t * x * y - s * z, t * x * z + s * y], [t * x * y + s * z, t * y * y + c, t * y * z - s * x], [t * x * z - s * y, t * y * z + s * x, t * z * z + c]]
}

impl Model {
    pub fn new() -> Model {
        Model { next_body: 1, faces: vec![String::new()], ..Default::default() }
    }

    /// Run commands (lines or `;`-separated) and keep them in the journal.
    pub fn run(&mut self, text: &str) -> Result<()> {
        for (n, line) in text.lines().enumerate() {
            for cmd in line.split(';') {
                let cmd = cmd.split('#').next().unwrap_or("").trim();
                if cmd.is_empty() {
                    continue;
                }
                self.command(cmd).with_context(|| format!("line {}: {cmd}", n + 1))?;
                self.journal.push(cmd.to_string());
            }
        }
        Ok(())
    }

    /// A model from commands.
    pub fn from_commands(text: &str, base_dir: Option<std::path::PathBuf>) -> Result<Model> {
        let mut m = Model::new();
        m.base_dir = base_dir;
        m.run(text)?;
        Ok(m)
    }

    fn face(&mut self, name: String) -> FaceId {
        self.faces.push(name);
        (self.faces.len() - 1) as FaceId
    }

    fn add_body(&mut self, kind: &str, shape: impl FnOnce(&mut Model, u32) -> Shape, desc: String) -> u32 {
        let id = self.next_body;
        self.next_body += 1;
        let shape = shape(self, id);
        self.bodies.insert(id, Body { id, name: format!("{kind} {id}"), shape, history: History::Leaf(desc) });
        id
    }

    fn take(&mut self, id: u32) -> Result<Body> {
        self.bodies.remove(&id).ok_or_else(|| anyhow!("there is no body {id}"))
    }

    fn body_mut(&mut self, id: u32) -> Result<&mut Body> {
        self.bodies.get_mut(&id).ok_or_else(|| anyhow!("there is no body {id}"))
    }

    fn outline(&self, a: &Args) -> Result<Arc<Outline>> {
        let id = a.ids("outline").first().copied().ok_or_else(|| anyhow!("which outline? (outline N)"))?;
        let o = self.outlines.get(&id).ok_or_else(|| anyhow!("there is no outline {id}"))?;
        o.validate()?;
        // Counterclockwise, so inside is on the left of every edge.
        let o = if o.area() < 0.0 { reversed(o) } else { o.clone() };
        Ok(Arc::new(o))
    }

    /// Faces for the curves an outline uses, named after its body.
    fn curve_faces(&mut self, o: &Outline, prefix: &str) -> Vec<FaceId> {
        o.curves.iter().map(|c| if c.is_empty() { 0 } else { self.face(format!("{prefix} {c}")) }).collect()
    }

    fn command(&mut self, cmd: &str) -> Result<()> {
        let w = words(cmd);
        let a = Args { w: w.iter().map(|s| s.to_string()).collect() };
        let verb = a.w[0].to_ascii_lowercase();
        let next = self.next_body;
        match verb.as_str() {
            "brick" | "box" | "block" => {
                let x = a.num("x")?.or(a.num("width")?);
                let y = a.num("y")?.or(a.num("depth")?);
                let z = a.num("z")?.or(a.num("height")?);
                let cube = a.w.get(1).and_then(|v| v.parse::<f64>().ok());
                let (x, y, z) = match (x, cube) {
                    (Some(x), _) => (x, y.unwrap_or(x), z.unwrap_or(x)),
                    (None, Some(c)) => (c, c, c),
                    _ => bail!("brick x <width> y <depth> z <height>, or brick <size>"),
                };
                positive(&[x, y, z])?;
                self.add_body(
                    "brick",
                    |m, id| Shape::Brick { half: [x / 2.0, y / 2.0, z / 2.0], faces: ["x-", "x+", "y-", "y+", "z-", "z+"].map(|f| m.face(format!("brick {id} {f}"))) },
                    format!("brick {x} x {y} x {z}"),
                );
            }
            "sphere" => {
                let r = a.need("radius")?;
                positive(&[r])?;
                self.add_body("sphere", |m, id| Shape::Sphere { r, face: m.face(format!("sphere {id} surface")) }, format!("sphere radius {r}"));
            }
            "cylinder" => {
                let r = a.need("radius")?;
                let h = a.num("height")?.or(a.num("z")?).ok_or_else(|| anyhow!("missing height"))?;
                positive(&[r, h])?;
                self.add_body("cylinder", |m, id| Shape::Cylinder { r, h, faces: ["side", "bottom", "top"].map(|f| m.face(format!("cylinder {id} {f}"))) }, format!("cylinder radius {r} height {h}"));
            }
            "cone" | "frustum" => {
                let r1 = a.need("radius")?;
                let r2 = a.num("top")?.unwrap_or(0.0);
                let h = a.num("height")?.or(a.num("z")?).ok_or_else(|| anyhow!("missing height"))?;
                positive(&[r1, h])?;
                if r2 < 0.0 {
                    bail!("the top radius cannot be negative");
                }
                self.add_body("cone", |m, id| Shape::Cone { r1, r2, h, faces: ["side", "bottom", "top"].map(|f| m.face(format!("cone {id} {f}"))) }, format!("cone radius {r1} top {r2} height {h}"));
            }
            "torus" => {
                let major = a.need("major")?;
                let minor = a.need("minor")?;
                positive(&[major, minor])?;
                if minor > major {
                    bail!("the minor radius must not exceed the major radius");
                }
                self.add_body("torus", |m, id| Shape::Torus { major, minor, face: m.face(format!("torus {id} surface")) }, format!("torus major {major} minor {minor}"));
            }
            "prism" => {
                let n = a.need("sides")? as usize;
                let r = a.need("radius")?;
                let h = a.num("height")?.or(a.num("z")?).ok_or_else(|| anyhow!("missing height"))?;
                if n < 3 {
                    bail!("a prism needs at least 3 sides");
                }
                positive(&[r, h])?;
                let o = Outline::regular(n, r);
                let id = self.next_body;
                let cf = o.curves.iter().map(|c| self.face(format!("prism {id} side {}", c.trim_start_matches("edge ")))).collect();
                let (bottom, top) = (self.face(format!("prism {id} bottom")), self.face(format!("prism {id} top")));
                // Centred on z like the cylinder.
                let shape = Shape::Transform {
                    inner: Box::new(Shape::Extrude { outline: Arc::new(o), h, curve_faces: cf, bottom, top }),
                    rot: rotation([0.0, 0.0, 1.0], 0.0),
                    offset: [0.0, 0.0, -h / 2.0],
                    scale: [1.0; 3],
                };
                self.add_body("prism", |_, _| shape, format!("prism {n} sides radius {r} height {h}"));
            }
            "superquadric" | "superellipsoid" => {
                let x = a.need("x")?;
                let y = a.num("y")?.unwrap_or(x);
                let z = a.num("z")?.unwrap_or(x);
                let e1 = a.num("e1")?.unwrap_or(1.0);
                let e2 = a.num("e2")?.unwrap_or(1.0);
                positive(&[x, y, z, e1, e2])?;
                self.add_body("superquadric", |m, id| Shape::Superquadric { abc: [x, y, z], e1, e2, face: m.face(format!("superquadric {id} surface")) }, format!("superquadric {x} x {y} x {z}, exponents {e1} {e2}"));
            }
            "harmonic" => {
                let r = a.need("radius")?;
                positive(&[r])?;
                let mut coef = Vec::new();
                for (i, w) in a.w.iter().enumerate() {
                    if w.eq_ignore_ascii_case("coefficient") {
                        let v: Vec<f64> = a.w[i + 1..].iter().take(3).map(|x| x.parse::<f64>()).collect::<std::result::Result<_, _>>().map_err(|_| anyhow!("coefficient l m value"))?;
                        if v.len() != 3 || v[0] < 0.0 || v[1].abs() > v[0] {
                            bail!("coefficient l m value with l >= 0 and |m| <= l");
                        }
                        coef.push((v[0] as i32, v[1] as i32, v[2]));
                    }
                }
                let desc = format!("harmonic radius {r}, {} coefficients", coef.len());
                self.add_body("harmonic", |m, id| Shape::Harmonic { r, coef, face: m.face(format!("harmonic {id} surface")) }, desc);
            }
            "outline" => self.outline_cmd(&a)?,
            "extrude" => {
                let o = self.outline(&a)?;
                let h = a.num("height")?.or(a.num("distance")?).ok_or_else(|| anyhow!("missing height"))?;
                positive(&[h])?;
                let id = next;
                let cf = self.curve_faces(&o, &format!("extrusion {id}"));
                let (bottom, top) = (self.face(format!("extrusion {id} bottom")), self.face(format!("extrusion {id} top")));
                let desc = format!("extrude outline {} height {h}", a.ids("outline")[0]);
                self.add_body("extrusion", |_, _| Shape::Extrude { outline: o, h, curve_faces: cf, bottom, top }, desc);
            }
            "revolve" => {
                let o = self.outline(&a)?;
                let angle = a.num("angle")?.unwrap_or(360.0);
                if !(angle > 0.0 && angle <= 360.0) {
                    bail!("the angle must be in (0, 360] degrees");
                }
                if o.bounds().0[0] < -1e-12 {
                    bail!("a revolved outline must lie at x >= 0 (x is the distance from the axis)");
                }
                let id = next;
                let cf = self.curve_faces(&o, &format!("revolution {id}"));
                let (start, end) = (self.face(format!("revolution {id} start")), self.face(format!("revolution {id} end")));
                let desc = format!("revolve outline {} angle {angle}", a.ids("outline")[0]);
                self.add_body("revolution", |_, _| Shape::Revolve { outline: o, angle: angle.to_radians(), curve_faces: cf, start, end }, desc);
            }
            "sweep" => {
                let o = self.outline(&a)?;
                let v = a.nums("path");
                if v.len() < 6 || v.len() % 3 != 0 {
                    bail!("sweep outline N path x y z x y z ... (at least two points)");
                }
                let pts: Vec<V3> = v.chunks(3).map(|c| [c[0], c[1], c[2]]).collect();
                let id = next;
                let cf = self.curve_faces(&o, &format!("sweep {id}"));
                let (start, end) = (self.face(format!("sweep {id} start")), self.face(format!("sweep {id} end")));
                let desc = format!("sweep outline {} along {} points", a.ids("outline")[0], pts.len());
                self.add_body("sweep", |_, _| Shape::Sweep { outline: o, path: Arc::new(Path::new(pts)), curve_faces: cf, start, end }, desc);
            }
            "import" => {
                let file = a.w.get(1).ok_or_else(|| anyhow!("import '<file>'"))?.clone();
                let mut path = std::path::PathBuf::from(&file);
                if path.is_relative() {
                    if let Some(b) = &self.base_dir {
                        path = b.join(path);
                    }
                }
                let mesh = read_surface(&path)?;
                let s = a.num("scale")?.unwrap_or(1.0);
                let stem = path.file_name().and_then(|f| f.to_str()).unwrap_or("surface").to_string();
                let inner = Shape::Surface { mesh: Arc::new(mesh), face: 0 };
                let id = self.add_body("surface", |m, id| {
                    let f = m.face(format!("surface {id} {stem}"));
                    let inner = match inner {
                        Shape::Surface { mesh, .. } => Shape::Surface { mesh, face: f },
                        x => x,
                    };
                    if s == 1.0 {
                        inner
                    } else {
                        Shape::Transform { inner: Box::new(inner), rot: rotation([0.0, 0.0, 1.0], 0.0), offset: [0.0; 3], scale: [s; 3] }
                    }
                }, format!("import {file}"));
                let _ = id;
            }
            "move" | "translate" => {
                let ids = a.ids("body");
                let d = [a.num("x")?.unwrap_or(0.0), a.num("y")?.unwrap_or(0.0), a.num("z")?.unwrap_or(0.0)];
                for id in ids_or_err(ids)? {
                    let b = self.body_mut(id)?;
                    let s = std::mem::replace(&mut b.shape, Shape::Union(vec![]));
                    b.shape = Shape::Transform { inner: Box::new(s), rot: rotation([0.0, 0.0, 1.0], 0.0), offset: d, scale: [1.0; 3] };
                    b.history = History::Op(format!("move {} {} {}", d[0], d[1], d[2]), vec![b.history.clone()]);
                }
            }
            "rotate" => {
                let ids = a.ids("body");
                let ang = a.need("angle")?;
                let axis = match a.text("about").map(|s| s.to_ascii_lowercase()) {
                    Some(s) if s == "x" => [1.0, 0.0, 0.0],
                    Some(s) if s == "y" => [0.0, 1.0, 0.0],
                    Some(s) if s == "z" => [0.0, 0.0, 1.0],
                    Some(_) => {
                        let v = a.nums("about");
                        if v.len() < 3 {
                            bail!("about x|y|z or about ax ay az");
                        }
                        [v[0], v[1], v[2]]
                    }
                    None => [0.0, 0.0, 1.0],
                };
                let c = a.nums("center");
                let c = if c.len() >= 3 { [c[0], c[1], c[2]] } else { [0.0; 3] };
                let r = rotation(axis, ang);
                // p -> R (p - c) + c
                let rc: V3 = std::array::from_fn(|i| r[i][0] * c[0] + r[i][1] * c[1] + r[i][2] * c[2]);
                let off = [c[0] - rc[0], c[1] - rc[1], c[2] - rc[2]];
                for id in ids_or_err(ids)? {
                    let b = self.body_mut(id)?;
                    let s = std::mem::replace(&mut b.shape, Shape::Union(vec![]));
                    b.shape = Shape::Transform { inner: Box::new(s), rot: r, offset: off, scale: [1.0; 3] };
                    b.history = History::Op(format!("rotate {ang} about {} {} {}", axis[0], axis[1], axis[2]), vec![b.history.clone()]);
                }
            }
            "scale" => {
                let mut ids = a.ids("body");
                let s = match (a.num("x")?, a.num("y")?, a.num("z")?) {
                    // scale body N [M ...] <factor>: the last number is the factor.
                    (None, None, None) => {
                        let v = a.nums("body");
                        if v.len() < 2 {
                            bail!("scale body N <factor>, or x/y/z factors");
                        }
                        ids.truncate(v.len() - 1);
                        [v[v.len() - 1]; 3]
                    }
                    (x, y, z) => [x.unwrap_or(1.0), y.unwrap_or(1.0), z.unwrap_or(1.0)],
                };
                positive(&s)?;
                for id in ids_or_err(ids)? {
                    let b = self.body_mut(id)?;
                    let sh = std::mem::replace(&mut b.shape, Shape::Union(vec![]));
                    b.shape = Shape::Transform { inner: Box::new(sh), rot: rotation([0.0, 0.0, 1.0], 0.0), offset: [0.0; 3], scale: s };
                    b.history = History::Op(format!("scale {} {} {}", s[0], s[1], s[2]), vec![b.history.clone()]);
                }
            }
            "unite" | "union" => {
                let ids = a.ids("body");
                if ids.len() < 2 {
                    bail!("unite body A B ... (at least two bodies)");
                }
                let mut parts = Vec::new();
                let mut hist = Vec::new();
                for id in &ids {
                    let b = self.take(*id)?;
                    parts.push(b.shape);
                    hist.push(b.history);
                }
                let first = ids[0];
                self.bodies.insert(first, Body { id: first, name: format!("body {first}"), shape: Shape::Union(parts), history: History::Op("unite".into(), hist) });
            }
            "intersect" => {
                let ids = a.ids("body");
                if ids.len() < 2 {
                    bail!("intersect body A B ... (at least two bodies)");
                }
                let mut parts = Vec::new();
                let mut hist = Vec::new();
                for id in &ids {
                    let b = self.take(*id)?;
                    parts.push(b.shape);
                    hist.push(b.history);
                }
                let first = ids[0];
                self.bodies.insert(first, Body { id: first, name: format!("body {first}"), shape: Shape::Intersect(parts), history: History::Op("intersect".into(), hist) });
            }
            "subtract" => {
                let f = a.pos("from").ok_or_else(|| anyhow!("subtract body B from body A"))?;
                let tools = Args { w: a.w[..f].to_vec() }.ids("body");
                let target = Args { w: a.w[f..].to_vec() }.ids("body");
                let (Some(&t), false) = (target.first(), tools.is_empty()) else { bail!("subtract body B from body A") };
                let keep = a.has("keep");
                let mut tool_shapes = Vec::new();
                let mut hist = Vec::new();
                for id in &tools {
                    if *id == t {
                        bail!("a body cannot be subtracted from itself");
                    }
                    let b = if keep { self.bodies.get(id).cloned().ok_or_else(|| anyhow!("there is no body {id}"))? } else { self.take(*id)? };
                    tool_shapes.push(b.shape);
                    hist.push(b.history);
                }
                let b = self.body_mut(t)?;
                let s = std::mem::replace(&mut b.shape, Shape::Union(vec![]));
                let tool = if tool_shapes.len() == 1 { tool_shapes.pop().unwrap() } else { Shape::Union(tool_shapes) };
                b.shape = Shape::Subtract(Box::new(s), Box::new(tool));
                let mut kids = vec![b.history.clone()];
                kids.extend(hist);
                b.history = History::Op("subtract".into(), kids);
            }
            "copy" => {
                for id in ids_or_err(a.ids("body"))? {
                    let b = self.bodies.get(&id).cloned().ok_or_else(|| anyhow!("there is no body {id}"))?;
                    let nid = self.next_body;
                    self.next_body += 1;
                    self.bodies.insert(nid, Body { id: nid, name: format!("{} copy", b.name), shape: b.shape, history: History::Op(format!("copy of body {id}"), vec![b.history]) });
                }
            }
            "delete" => {
                for id in ids_or_err(a.ids("body"))? {
                    self.take(id)?;
                }
            }
            "body" => {
                let id = a.w.get(1).and_then(|x| x.parse::<u32>().ok()).ok_or_else(|| anyhow!("body N name '<name>'"))?;
                let name = a.text("name").ok_or_else(|| anyhow!("body N name '<name>'"))?;
                self.body_mut(id)?.name = name;
            }
            "face" => {
                let id = a.w.get(1).and_then(|x| x.parse::<usize>().ok()).ok_or_else(|| anyhow!("face N name '<name>'"))?;
                let name = a.text("name").ok_or_else(|| anyhow!("face N name '<name>'"))?;
                if id == 0 || id >= self.faces.len() {
                    bail!("there is no face {id}");
                }
                self.faces[id] = name;
            }
            other => bail!("unknown command '{other}'"),
        }
        Ok(())
    }

    fn outline_cmd(&mut self, a: &Args) -> Result<()> {
        let id = a.w.get(1).and_then(|x| x.parse::<u32>().ok()).ok_or_else(|| anyhow!("outline N <shape> ..."))?;
        let kind = a.w.get(2).map(|s| s.to_ascii_lowercase()).ok_or_else(|| anyhow!("outline {id} what? (polyline, rectangle, circle, ellipse, spline, regular, add, fillet)"))?;
        let center = {
            let c = a.nums("center");
            if c.len() >= 2 {
                [c[0], c[1]]
            } else {
                [0.0, 0.0]
            }
        };
        let o = match kind.as_str() {
            "polyline" | "polygon" => {
                let mut o = Outline::default();
                o.add_lines(&pairs(&a.nums(&kind), "polyline")?);
                o
            }
            "rectangle" => Outline::rectangle(center, a.need("x")?, a.need("y")?),
            "circle" => Outline::circle(center, a.need("radius")?),
            "ellipse" => Outline::ellipse(center, a.need("x")?, a.need("y")?),
            "spline" => Outline::spline(&pairs(&a.nums("spline"), "spline")?),
            "regular" => Outline::regular(a.need("sides")? as usize, a.need("radius")?),
            "add" => {
                let mut o = self.outlines.get(&id).cloned().unwrap_or_default();
                match a.w.get(3).map(|s| s.to_ascii_lowercase()).as_deref() {
                    Some("line") | Some("lines") | Some("polyline") => o.add_lines(&pairs(&a.w[4..].iter().filter_map(|x| x.parse().ok()).collect::<Vec<f64>>(), "line")?),
                    Some("arc") => {
                        let r = a.need("radius")?;
                        let (s, e) = (a.need("start")?, a.need("end")?);
                        o.add_arc(center, r, s.to_radians(), e.to_radians());
                    }
                    Some("spline") => o.add_spline(&pairs(&a.w[4..].iter().filter_map(|x| x.parse().ok()).collect::<Vec<f64>>(), "spline")?, false),
                    _ => bail!("outline {id} add line|arc|spline ..."),
                }
                o
            }
            "fillet" => {
                let mut o = self.outlines.get(&id).cloned().ok_or_else(|| anyhow!("there is no outline {id}"))?;
                o.fillet(a.need("radius")?)?;
                o
            }
            other => bail!("unknown outline kind '{other}'"),
        };
        self.outlines.insert(id, o);
        Ok(())
    }

    /// The bodies with their face ids, for listings.
    pub fn describe(&self) -> Vec<(u32, String, Vec<String>, Vec<(FaceId, String)>)> {
        self.bodies.values().map(|b| (b.id, b.name.clone(), b.history.lines(), b.shape.faces().into_iter().map(|f| (f, self.faces[f as usize].clone())).collect())).collect()
    }
}

fn reversed(o: &Outline) -> Outline {
    // Rebuild through the points in the other order, keeping curve names per segment.
    let mut r = o.clone();
    r.reverse_in_place();
    r
}

fn ids_or_err(ids: Vec<u32>) -> Result<Vec<u32>> {
    if ids.is_empty() {
        bail!("which body? (body N)");
    }
    Ok(ids)
}

fn positive(v: &[f64]) -> Result<()> {
    if v.iter().any(|x| !(*x > 0.0) || !x.is_finite()) {
        bail!("sizes must be positive");
    }
    Ok(())
}

/// A closed surface from a file (STL, PLY, OBJ, VTK polygonal data ...).
pub fn read_surface(path: &std::path::Path) -> Result<TriMesh> {
    let ds = match aurafx_io::read_file(path)? {
        aurafx_io::Loaded::Static(d) => d,
        aurafx_io::Loaded::Series(s) => match s.steps.first() {
            Some(st) => aurafx_io::load_step(st)?.into(),
            None => bail!("{}: no data", path.display()),
        },
    };
    surface_mesh(&ds)
}

/// The triangles of a surface dataset (polygons split into fans).
pub fn surface_mesh(ds: &aurafx_data::DataSet) -> Result<TriMesh> {
    let aurafx_data::DataSet::PolyData(pd) = ds else { bail!("not a surface (polygonal data)") };
    let pts: Vec<V3> = (0..pd.points.len()).map(|i| pd.points.get(i)).collect();
    let mut tris = Vec::new();
    for (_, c) in pd.polys.iter() {
        for k in 1..c.len().saturating_sub(1) {
            tris.push([c[0], c[k], c[k + 1]]);
        }
    }
    TriMesh::new(&pts, &tris)
}
