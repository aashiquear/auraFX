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
    /// Bodies to mesh: (body, element size, nodes per element).
    pub mesh_requests: Vec<(u32, f64, usize)>,
    /// Meshing scheme per body when not automatic: structured, overlay or stairstep.
    pub mesh_schemes: BTreeMap<u32, String>,
    /// Structured divisions per body (`intervals`, `bias`, `cluster` of `mesh body`): pieces
    /// along the body's own x, y, z (a cylinder: around a quarter, across the radius, along the
    /// height), largest over smallest piece per axis, and where the small pieces go.
    pub mesh_divisions: BTreeMap<u32, (Vec<usize>, Vec<f64>, String)>,
    /// Element blocks: (id, name, bodies).
    pub user_blocks: Vec<(i32, String, Vec<u32>)>,
    /// Side sets and node sets: (id, name, faces).
    pub user_sidesets: Vec<(i32, String, Vec<FaceId>)>,
    pub user_nodesets: Vec<(i32, String, Vec<FaceId>)>,
    /// Groups of bodies imprinted together (`imprint`): meshed with one element size, with a side
    /// set on each shared face.
    pub imprints: Vec<Vec<u32>>,
    /// Groups of bodies merged (`merge`): their meshes must meet node to node on shared faces.
    pub merges: Vec<Vec<u32>>,
    /// Webcuts made so far (numbers the cut faces).
    webcuts: u32,
    /// Groups: (id, name, bodies, faces). Commands name them as `group N` or `group <name>`.
    pub groups: Vec<(u32, String, Vec<u32>, Vec<FaceId>)>,
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
    [
        [t * x * x + c, t * x * y - s * z, t * x * z + s * y],
        [t * x * y + s * z, t * y * y + c, t * y * z - s * x],
        [t * x * z - s * y, t * y * z + s * x, t * z * z + c],
    ]
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

    /// `webcut body N with plane xplane|yplane|zplane [offset d]`, `with plane normal x y z [offset d]`,
    /// `with cylinder radius r [axis x|y|z] [center x y z]` or `with body M`: the body is cut in two
    /// (the part on the plane's negative side, inside the cylinder or inside body M keeps the id; the
    /// rest is a new body), both sharing the cut face `webcut K`. A brick cut along one of its own
    /// axes gives two bricks, so both stay structured (sweepable).
    fn webcut(&mut self, a: &Args) -> Result<()> {
        let w = a.pos("with").ok_or_else(|| anyhow!("webcut body N with plane xplane offset 0 (or with cylinder radius r, with body M)"))?;
        let ids = Args { w: a.w[..w].to_vec() }.ids("body");
        let ids = ids_or_err(ids)?;
        let rest = Args { w: a.w[w..].to_vec() };
        self.webcuts += 1;
        let k = self.webcuts;
        let face = self.face(format!("webcut {k}"));
        // The tool: a shape whose inside is the part that keeps the body's id.
        let (tool, plane): (Shape, Option<(V3, f64)>) = if rest.has("plane") {
            let mut n = if rest.has("xplane") {
                [1.0, 0.0, 0.0]
            } else if rest.has("yplane") {
                [0.0, 1.0, 0.0]
            } else if rest.has("zplane") {
                [0.0, 0.0, 1.0]
            } else {
                let v = rest.nums("normal");
                if v.len() != 3 {
                    bail!("webcut ... with plane xplane|yplane|zplane, or with plane normal x y z");
                }
                [v[0], v[1], v[2]]
            };
            let l = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
            if l < 1e-12 {
                bail!("the plane's normal is zero");
            }
            n = n.map(|x| x / l);
            let d = rest.num("offset")?.unwrap_or(0.0);
            (Shape::HalfSpace { normal: n, offset: d, face }, Some((n, d)))
        } else if rest.has("cylinder") {
            let r = rest.need("radius")?;
            positive(&[r])?;
            let axis = match rest.text("axis").as_deref() {
                Some("x") => [1.0, 0.0, 0.0],
                Some("y") => [0.0, 1.0, 0.0],
                Some("z") | None => [0.0, 0.0, 1.0],
                Some(o) => bail!("axis is x, y or z, not {o}"),
            };
            let c = rest.nums("center");
            let center = if c.len() == 3 { [c[0], c[1], c[2]] } else { [0.0; 3] };
            // A cylinder long enough to cross any body, turned from z onto the axis.
            let rot = if axis[2] == 1.0 {
                rotation([0.0, 0.0, 1.0], 0.0)
            } else if axis[0] == 1.0 {
                rotation([0.0, 1.0, 0.0], 90.0)
            } else {
                rotation([1.0, 0.0, 0.0], -90.0)
            };
            let cyl = Shape::Cylinder { r, h: 1.0e9, faces: [face, face, face] };
            (Shape::Transform { inner: Box::new(cyl), rot, offset: center, scale: [1.0; 3] }, None)
        } else if rest.has("sheet") {
            // An open surface from a file: the side its normals point away from keeps the id.
            let path = rest.text("sheet").ok_or_else(|| anyhow!("webcut body N with sheet 'surface.stl'"))?;
            let mesh = read_surface(std::path::Path::new(&path))?;
            (Shape::Sheet { mesh: Arc::new(mesh), face }, None)
        } else if rest.has("sweep") || rest.has("curve") {
            // A curve in the xy plane (an outline's points, not closed) swept along a direction,
            // and continued past its ends, so that it crosses the bodies.
            let oid = rest.ids("outline").first().copied().ok_or_else(|| anyhow!("webcut body N with sweep outline M [direction x y z]"))?;
            let o = self.outlines.get(&oid).ok_or_else(|| anyhow!("there is no outline {oid}"))?;
            let pts: Vec<[f64; 2]> = o.points().to_vec();
            if pts.len() < 2 {
                bail!("outline {oid} needs at least two points to cut with");
            }
            let dv = rest.nums("direction");
            let d = if dv.len() == 3 { [dv[0], dv[1], dv[2]] } else { [0.0, 0.0, 1.0] };
            let dl = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt();
            if dl < 1e-12 {
                bail!("the sweep direction is zero");
            }
            let d = d.map(|x| x / dl);
            let mut size = 1.0f64;
            for id in &ids {
                if let Some(b) = self.bodies.get(id) {
                    let (lo, hi) = b.shape.bounds();
                    for k in 0..3 {
                        if lo[k].is_finite() && hi[k].is_finite() {
                            size = size.max(hi[k].abs()).max(lo[k].abs()).max(hi[k] - lo[k]);
                        }
                    }
                }
            }
            let big = 10.0 * size;
            let unit2 = |a: [f64; 2], b: [f64; 2]| {
                let (x, y) = (b[0] - a[0], b[1] - a[1]);
                let l = (x * x + y * y).sqrt().max(1e-300);
                [x / l, y / l]
            };
            let (t0, t1) = (unit2(pts[1], pts[0]), unit2(pts[pts.len() - 2], pts[pts.len() - 1]));
            let mut line = vec![[pts[0][0] + big * t0[0], pts[0][1] + big * t0[1]]];
            line.extend(pts.iter().copied());
            line.push([pts[pts.len() - 1][0] + big * t1[0], pts[pts.len() - 1][1] + big * t1[1]]);
            let mut vs = Vec::new();
            for q in &line {
                for s in [-big, big] {
                    vs.push([q[0] + s * d[0], q[1] + s * d[1], s * d[2]]);
                }
            }
            let mut tris = Vec::new();
            for i in 0..line.len() as u32 - 1 {
                let (a0, a1, b0, b1) = (2 * i, 2 * i + 1, 2 * i + 2, 2 * i + 3);
                tris.push([a0, b0, b1]);
                tris.push([a0, b1, a1]);
            }
            (Shape::Sheet { mesh: Arc::new(TriMesh::new(&vs, &tris)?), face }, None)
        } else if rest.has("body") {
            let tid = rest.ids("body").first().copied().ok_or_else(|| anyhow!("webcut body N with body M"))?;
            let t = self.bodies.get(&tid).ok_or_else(|| anyhow!("there is no body {tid}"))?;
            (t.shape.clone(), None)
        } else {
            bail!("webcut body N with plane xplane|yplane|zplane [offset d], with plane normal x y z [offset d], with cylinder radius r, or with body M");
        };
        for id in ids {
            let b = self.bodies.get(&id).cloned().ok_or_else(|| anyhow!("there is no body {id}"))?;
            let (inner, outer) = match plane.and_then(|(n, d)| split_brick(&b.shape, n, d, face)) {
                Some(pair) => pair,
                None => (Shape::Intersect(vec![b.shape.clone(), tool.clone()]), Shape::Subtract(Box::new(b.shape.clone()), Box::new(tool.clone()))),
            };
            let nid = self.next_body;
            self.next_body += 1;
            let desc = format!("webcut {k} of body {id}");
            self.bodies.insert(id, Body { id, name: b.name.clone(), shape: inner, history: History::Op(desc.clone(), vec![b.history.clone()]) });
            self.bodies.insert(nid, Body { id: nid, name: format!("{} cut {k}", b.name), shape: outer, history: History::Op(desc, vec![b.history]) });
        }
        Ok(())
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

    /// Replace `group N` (or `group <name>`) in a command by its bodies (`body ...`), or for side
    /// and node sets by its faces and its bodies' faces (`face ...`).
    fn expand_groups(&self, w: Vec<String>) -> Result<Vec<String>> {
        let verb = w[0].to_ascii_lowercase();
        if verb == "group" {
            return Ok(w);
        }
        let faces_wanted = verb == "sideset" || verb == "nodeset";
        let mut out: Vec<String> = Vec::new();
        let mut add: Vec<u32> = Vec::new();
        let mut i = 0;
        while i < w.len() {
            if i > 0 && w[i].eq_ignore_ascii_case("group") && i + 1 < w.len() {
                let key = &w[i + 1];
                let g = self
                    .groups
                    .iter()
                    .find(|g| key.parse::<u32>().ok() == Some(g.0) || g.1.eq_ignore_ascii_case(key))
                    .ok_or_else(|| anyhow!("there is no group {key}"))?;
                if faces_wanted {
                    add.extend(g.3.iter().copied());
                    for b in &g.2 {
                        if let Some(body) = self.bodies.get(b) {
                            add.extend(body.shape.faces());
                        }
                    }
                } else {
                    add.extend(g.2.iter().copied());
                }
                i += 2;
                continue;
            }
            out.push(w[i].clone());
            i += 1;
        }
        if add.is_empty() {
            return Ok(out);
        }
        let kw = if faces_wanted { "face" } else { "body" };
        let ids: Vec<String> = add.iter().map(|x| x.to_string()).collect();
        match out.iter().position(|t| t.eq_ignore_ascii_case(kw) || t.eq_ignore_ascii_case(&format!("{kw}s"))) {
            Some(p) => {
                for (k, id) in ids.into_iter().enumerate() {
                    out.insert(p + 1 + k, id);
                }
            }
            None => {
                // After `add` for sets and blocks, else after the verb.
                let p = out.iter().position(|t| t.eq_ignore_ascii_case("add")).map(|p| p + 1).unwrap_or(1);
                out.insert(p, kw.to_string());
                for (k, id) in ids.into_iter().enumerate() {
                    out.insert(p + 1 + k, id);
                }
            }
        }
        Ok(out)
    }

    fn command(&mut self, cmd: &str) -> Result<()> {
        let w = self.expand_groups(words(cmd).iter().map(|s| s.to_string()).collect())?;
        let a = Args { w };
        let verb = a.w[0].to_ascii_lowercase();
        let next = self.next_body;
        match verb.as_str() {
            "brick" | "box" => {
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
                    |m, id| Shape::Brick {
                        half: [x / 2.0, y / 2.0, z / 2.0],
                        faces: ["x-", "x+", "y-", "y+", "z-", "z+"].map(|f| m.face(format!("brick {id} {f}"))),
                    },
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
                self.add_body(
                    "cylinder",
                    |m, id| Shape::Cylinder { r, h, faces: ["side", "bottom", "top"].map(|f| m.face(format!("cylinder {id} {f}"))) },
                    format!("cylinder radius {r} height {h}"),
                );
            }
            "cone" | "frustum" => {
                let r1 = a.need("radius")?;
                let r2 = a.num("top")?.unwrap_or(0.0);
                let h = a.num("height")?.or(a.num("z")?).ok_or_else(|| anyhow!("missing height"))?;
                positive(&[r1, h])?;
                if r2 < 0.0 {
                    bail!("the top radius cannot be negative");
                }
                self.add_body(
                    "cone",
                    |m, id| Shape::Cone { r1, r2, h, faces: ["side", "bottom", "top"].map(|f| m.face(format!("cone {id} {f}"))) },
                    format!("cone radius {r1} top {r2} height {h}"),
                );
            }
            "torus" => {
                let major = a.need("major")?;
                let minor = a.need("minor")?;
                positive(&[major, minor])?;
                if minor > major {
                    bail!("the minor radius must not exceed the major radius");
                }
                self.add_body(
                    "torus",
                    |m, id| Shape::Torus { major, minor, face: m.face(format!("torus {id} surface")) },
                    format!("torus major {major} minor {minor}"),
                );
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
                self.add_body(
                    "superquadric",
                    |m, id| Shape::Superquadric { abc: [x, y, z], e1, e2, face: m.face(format!("superquadric {id} surface")) },
                    format!("superquadric {x} x {y} x {z}, exponents {e1} {e2}"),
                );
            }
            "harmonic" => {
                let r = a.need("radius")?;
                positive(&[r])?;
                let mut coef = Vec::new();
                for (i, w) in a.w.iter().enumerate() {
                    if w.eq_ignore_ascii_case("coefficient") {
                        let v: Vec<f64> = a.w[i + 1..]
                            .iter()
                            .take(3)
                            .map(|x| x.parse::<f64>())
                            .collect::<std::result::Result<_, _>>()
                            .map_err(|_| anyhow!("coefficient l m value"))?;
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
            "thicken" => {
                let file = a.w.get(1).ok_or_else(|| anyhow!("thicken '<surface file>' thickness t"))?.clone();
                let t = a.need("thickness")?;
                positive(&[t])?;
                let mesh = read_surface(&self.resolve(&file))?;
                self.add_body(
                    "thickened",
                    |m, id| Shape::Thick { mesh: Arc::new(mesh), half: t / 2.0, face: m.face(format!("thickened {id} surface")) },
                    format!("{file} thickened {t}"),
                );
            }
            "loft" => {
                let ids = a.ids("outline");
                let (Some(&o1), Some(&o2)) = (
                    ids.first(),
                    a.w.iter().rposition(|x| x.eq_ignore_ascii_case("outline")).and_then(|i| a.w.get(i + 1)).and_then(|x| x.parse::<u32>().ok()).as_ref(),
                ) else {
                    bail!("loft outline 1 outline 2 height h");
                };
                let h = a.need("height")?;
                positive(&[h])?;
                let get = |id: u32| -> Result<Arc<Outline>> {
                    let o = self.outlines.get(&id).ok_or_else(|| anyhow!("there is no outline {id}"))?;
                    o.validate()?;
                    Ok(Arc::new(o.clone()))
                };
                let (oa, ob) = (get(o1)?, get(o2)?);
                self.add_body(
                    "loft",
                    |m, id| Shape::Loft { a: oa, b: ob, h, faces: ["side", "bottom", "top"].map(|f| m.face(format!("loft {id} {f}"))) },
                    format!("loft from outline {o1} to outline {o2}, height {h}"),
                );
            }
            "sew" => {
                // Surface patches from files, their corners joined within a tolerance, into one closed surface.
                let tol = a.num("tolerance")?.unwrap_or(1e-6);
                let files: Vec<String> = a.w[1..].iter().take_while(|x| !x.eq_ignore_ascii_case("tolerance")).cloned().collect();
                if files.is_empty() {
                    bail!("sew 'a.stl' 'b.stl' ... [tolerance t]");
                }
                let (mut pts, mut tris): (Vec<V3>, Vec<[u32; 3]>) = (Vec::new(), Vec::new());
                for f in &files {
                    let m = read_surface_open(&self.resolve(f))?;
                    let o = pts.len() as u32;
                    pts.extend(m.0);
                    tris.extend(m.1.iter().map(|t| t.map(|i| i + o)));
                }
                let (pts, tris) = weld(&pts, &tris, tol);
                let mut edges: std::collections::HashMap<(u32, u32), u32> = Default::default();
                for t in &tris {
                    for k in 0..3 {
                        let (x, y) = (t[k].min(t[(k + 1) % 3]), t[k].max(t[(k + 1) % 3]));
                        *edges.entry((x, y)).or_default() += 1;
                    }
                }
                let open = edges.values().filter(|c| **c != 2).count();
                if open > 0 {
                    bail!("the patches do not close up: {open} edges are not shared by exactly two triangles (raise the tolerance, or a patch is missing)");
                }
                let mesh = TriMesh::new(&pts, &tris)?;
                if mesh.volume() < 0.0 {
                    bail!("the sewn surface faces inwards: flip the patches' triangles");
                }
                self.add_body(
                    "sewn",
                    |m, id| Shape::Surface { mesh: Arc::new(mesh), face: m.face(format!("sewn {id} surface")) },
                    format!("{} sewn", files.join(", ")),
                );
            }
            "import" => {
                let file = a.w.get(1).ok_or_else(|| anyhow!("import '<file>'"))?.clone();
                let mut path = std::path::PathBuf::from(&file);
                if path.is_relative() {
                    if let Some(b) = &self.base_dir {
                        path = b.join(path);
                    }
                }
                let (mesh, face_names) = read_surface_named(&path)?;
                let s = a.num("scale")?.unwrap_or(1.0);
                let stem = path.file_name().and_then(|f| f.to_str()).unwrap_or("surface").to_string();
                let patches = mesh.n_patches;
                let inner = Shape::Surface { mesh: Arc::new(mesh), face: 0 };
                let id = self.add_body(
                    "surface",
                    |m, id| {
                        // A CAD file's faces keep their names; other surfaces are numbered patches.
                        let label = |k: u32| match &face_names {
                            Some(n) => format!("{id} {}", n.get(k as usize).cloned().unwrap_or_else(|| format!("face {}", k + 1))),
                            None if k == 0 => format!("surface {id} {stem}"),
                            None => format!("surface {id} {stem} {}", k + 1),
                        };
                        let f = m.face(label(0));
                        for k in 1..patches {
                            m.face(label(k));
                        }
                        let inner = match inner {
                            Shape::Surface { mesh, .. } => Shape::Surface { mesh, face: f },
                            x => x,
                        };
                        if s == 1.0 {
                            inner
                        } else {
                            Shape::Transform { inner: Box::new(inner), rot: rotation([0.0, 0.0, 1.0], 0.0), offset: [0.0; 3], scale: [s; 3] }
                        }
                    },
                    format!("import {file}"),
                );
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
                self.bodies
                    .insert(first, Body { id: first, name: format!("body {first}"), shape: Shape::Union(parts), history: History::Op("unite".into(), hist) });
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
                self.bodies.insert(
                    first,
                    Body { id: first, name: format!("body {first}"), shape: Shape::Intersect(parts), history: History::Op("intersect".into(), hist) },
                );
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
            "webcut" => self.webcut(&a)?,
            "imprint" | "merge" => {
                let ids = if a.has("all") { self.bodies.keys().copied().collect() } else { a.ids("body") };
                if ids.len() < 2 {
                    bail!("{verb} body A B ... (or {verb} all): at least two bodies");
                }
                for id in &ids {
                    if !self.bodies.contains_key(id) {
                        bail!("there is no body {id}");
                    }
                }
                if verb == "imprint" {
                    self.imprints.push(ids);
                } else {
                    self.merges.push(ids);
                }
            }
            "copy" => {
                for id in ids_or_err(a.ids("body"))? {
                    let b = self.bodies.get(&id).cloned().ok_or_else(|| anyhow!("there is no body {id}"))?;
                    let nid = self.next_body;
                    self.next_body += 1;
                    self.bodies.insert(
                        nid,
                        Body { id: nid, name: format!("{} copy", b.name), shape: b.shape, history: History::Op(format!("copy of body {id}"), vec![b.history]) },
                    );
                }
            }
            "delete" => {
                for id in ids_or_err(a.ids("body"))? {
                    self.take(id)?;
                }
            }
            "mesh" => {
                let mut ids = a.ids("body");
                if ids.is_empty() {
                    ids = a.ids("volume");
                }
                let ids = ids_or_err(ids)?;
                let size = a.need("size")?;
                positive(&[size])?;
                let npe = match a.text("element").map(|s| s.to_ascii_uppercase()) {
                    Some(s) if s == "HEX20" => 20,
                    Some(s) if s == "HEX27" => 27,
                    Some(s) if s == "HEX8" || s == "HEX" => 8,
                    Some(s) => bail!("unknown element {s} (HEX8, HEX20, HEX27)"),
                    None => 8,
                };
                let scheme = match a.text("scheme").map(|s| s.to_ascii_lowercase()) {
                    Some(s) if ["structured", "overlay", "stairstep", "mapped", "auto"].contains(&s.as_str()) => s,
                    Some(s) => bail!("unknown scheme {s} (auto, structured, mapped, overlay, stairstep)"),
                    None => "auto".into(),
                };
                let intervals: Vec<usize> = a.nums("intervals").into_iter().map(|x| x.max(1.0) as usize).collect();
                let bias: Vec<f64> = a.nums("bias");
                if bias.iter().any(|b| *b < 1.0) {
                    bail!("bias is the largest piece over the smallest: 1 or more");
                }
                let cluster = a.text("cluster").map(|s| s.to_ascii_lowercase()).unwrap_or_else(|| "start".into());
                if !["start", "end", "both", "none"].contains(&cluster.as_str()) {
                    bail!("cluster start, end or both");
                }
                for id in ids {
                    if !self.bodies.contains_key(&id) {
                        bail!("there is no body {id}");
                    }
                    if intervals.is_empty() && bias.is_empty() {
                        self.mesh_divisions.remove(&id);
                    } else {
                        self.mesh_divisions.insert(id, (intervals.clone(), bias.clone(), cluster.clone()));
                    }
                    self.mesh_requests.retain(|r| r.0 != id);
                    self.mesh_requests.push((id, size, npe));
                    if scheme == "auto" {
                        self.mesh_schemes.remove(&id);
                    } else {
                        self.mesh_schemes.insert(id, scheme.clone());
                    }
                }
            }
            "block" | "sideset" | "nodeset" => {
                let id = a.w.get(1).and_then(|x| x.parse::<i32>().ok()).ok_or_else(|| anyhow!("{verb} N add ..."))?;
                if !a.has("add") && !a.has("name") {
                    bail!("{verb} {id} add ... [name '<name>']");
                }
                let name = a.text("name");
                if verb == "block" {
                    let mut bodies = a.ids("body");
                    if bodies.is_empty() {
                        bodies = a.ids("volume");
                    }
                    let e = match self.user_blocks.iter_mut().find(|b| b.0 == id) {
                        Some(e) => e,
                        None => {
                            self.user_blocks.push((id, format!("block_{id}"), Vec::new()));
                            self.user_blocks.last_mut().unwrap()
                        }
                    };
                    e.2.extend(bodies);
                    if let Some(n) = name {
                        e.1 = n;
                    }
                } else {
                    let mut faces = a.ids("face");
                    if faces.is_empty() {
                        faces = a.ids("surface");
                    }
                    for f in &faces {
                        if *f == 0 || *f as usize >= self.faces.len() {
                            bail!("there is no face {f}");
                        }
                    }
                    let list = if verb == "sideset" { &mut self.user_sidesets } else { &mut self.user_nodesets };
                    let e = match list.iter_mut().find(|b| b.0 == id) {
                        Some(e) => e,
                        None => {
                            list.push((id, format!("{verb}_{id}"), Vec::new()));
                            list.last_mut().unwrap()
                        }
                    };
                    e.2.extend(faces);
                    if let Some(n) = name {
                        e.1 = n;
                    }
                }
            }
            "group" => {
                let id = a.w.get(1).and_then(|x| x.parse::<u32>().ok()).ok_or_else(|| anyhow!("group N add body 1 2 face 5 [name '<name>']"))?;
                let bodies = a.ids("body");
                let faces = a.ids("face");
                for b in &bodies {
                    if !self.bodies.contains_key(b) {
                        bail!("there is no body {b}");
                    }
                }
                for f in &faces {
                    if *f == 0 || *f as usize >= self.faces.len() {
                        bail!("there is no face {f}");
                    }
                }
                let e = match self.groups.iter_mut().find(|g| g.0 == id) {
                    Some(e) => e,
                    None => {
                        self.groups.push((id, format!("group_{id}"), Vec::new(), Vec::new()));
                        self.groups.last_mut().unwrap()
                    }
                };
                for b in bodies {
                    if !e.2.contains(&b) {
                        e.2.push(b);
                    }
                }
                for f in faces {
                    if !e.3.contains(&f) {
                        e.3.push(f);
                    }
                }
                if let Some(n) = a.text("name") {
                    e.1 = n;
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
        let kind =
            a.w.get(2)
                .map(|s| s.to_ascii_lowercase())
                .ok_or_else(|| anyhow!("outline {id} what? (polyline, rectangle, circle, ellipse, spline, regular, add, fillet)"))?;
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
                    Some("line") | Some("lines") | Some("polyline") => {
                        o.add_lines(&pairs(&a.w[4..].iter().filter_map(|x| x.parse().ok()).collect::<Vec<f64>>(), "line")?)
                    }
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

    /// An outline made by `outline N ...` commands.
    pub fn outline_points(&self, id: u32) -> Option<Vec<[f64; 2]>> {
        self.outlines.get(&id).map(|o| o.points().to_vec())
    }

    /// The bodies with their face ids, for listings.
    pub fn describe(&self) -> Vec<(u32, String, Vec<String>, Vec<(FaceId, String)>)> {
        self.bodies
            .values()
            .map(|b| (b.id, b.name.clone(), b.history.lines(), b.shape.faces().into_iter().map(|f| (f, self.faces[f as usize].clone())).collect()))
            .collect()
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

impl Model {
    fn resolve(&self, file: &str) -> std::path::PathBuf {
        let p = std::path::PathBuf::from(file);
        match (&self.base_dir, p.is_relative()) {
            (Some(b), true) => b.join(p),
            _ => p,
        }
    }
}

/// The points and triangles of a surface file as they are (open surfaces too).
fn read_surface_open(path: &std::path::Path) -> Result<(Vec<V3>, Vec<[u32; 3]>)> {
    let ds = match aurafx_io::read_file(path)? {
        aurafx_io::Loaded::Static(d) => d,
        aurafx_io::Loaded::Series(s) => match s.steps.first() {
            Some(st) => aurafx_io::load_step(st)?.into(),
            None => bail!("{}: no data", path.display()),
        },
    };
    let aurafx_data::DataSet::PolyData(pd) = &*ds else { bail!("{}: not a surface", path.display()) };
    let pts = (0..pd.points.len()).map(|i| pd.points.get(i)).collect();
    let mut tris = Vec::new();
    for (_, c) in pd.polys.iter() {
        for k in 1..c.len().saturating_sub(1) {
            tris.push([c[0], c[k], c[k + 1]]);
        }
    }
    Ok((pts, tris))
}

/// Join points closer than `tol` (each to the first of them) and drop triangles that collapse.
fn weld(pts: &[V3], tris: &[[u32; 3]], tol: f64) -> (Vec<V3>, Vec<[u32; 3]>) {
    let tree = aurafx_data::kdtree::KdTree::new(pts);
    let mut to: Vec<u32> = (0..pts.len() as u32).collect();
    for i in 0..pts.len() {
        if to[i] as usize != i {
            continue;
        }
        for (j, _) in tree.within(pts[i], tol) {
            if j > i && to[j] as usize == j {
                to[j] = i as u32;
            }
        }
    }
    let mut keep = vec![u32::MAX; pts.len()];
    let mut out = Vec::new();
    for i in 0..pts.len() {
        if to[i] as usize == i {
            keep[i] = out.len() as u32;
            out.push(pts[i]);
        }
    }
    let t = tris.iter().map(|t| t.map(|v| keep[to[v as usize] as usize])).filter(|t| t[0] != t[1] && t[1] != t[2] && t[0] != t[2]).collect();
    (out, t)
}

/// A closed surface from a file (STL, PLY, OBJ, VTK polygonal data ...).
/// A surface file as a triangle mesh and, when its triangles carry named faces (a CAD file), their names.
pub fn read_surface_named(path: &std::path::Path) -> Result<(TriMesh, Option<Vec<String>>)> {
    let ds = match aurafx_io::read_file(path)? {
        aurafx_io::Loaded::Static(d) => d,
        aurafx_io::Loaded::Series(s) => match s.steps.first() {
            Some(st) => aurafx_io::load_step(st)?.into(),
            None => bail!("{}: no data", path.display()),
        },
    };
    match surface_mesh_with_faces(&ds) {
        Some((m, names)) => Ok((m, Some(names))),
        None => Ok((surface_mesh(&ds)?, None)),
    }
}

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
/// A surface whose triangles carry face numbers (the cell array `FaceId` of a CAD Model or Geometry
/// surface): the mesh's patches are those faces, and their names are the field array `FaceNames`.
pub fn surface_mesh_with_faces(ds: &aurafx_data::DataSet) -> Option<(TriMesh, Vec<String>)> {
    let aurafx_data::DataSet::PolyData(pd) = ds else { return None };
    let ids = pd.cell_data.get("FaceId")?;
    let names = match &pd.field_data.get("FaceNames")?.data {
        aurafx_data::ArrayData::String(v) => v.clone(),
        _ => return None,
    };
    let pts: Vec<V3> = (0..pd.points.len()).map(|i| pd.points.get(i)).collect();
    let (mut tris, mut labels) = (Vec::new(), Vec::new());
    for (ci, (_, c)) in pd.polys.iter().enumerate() {
        for k in 1..c.len().saturating_sub(1) {
            tris.push([c[0], c[k], c[k + 1]]);
            labels.push(ids.get(ci, 0).max(0.0) as u32);
        }
    }
    TriMesh::new_labeled(&pts, &tris, Some(&labels)).ok().map(|m| (m, names))
}

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

/// A brick (under moves, turns and scales) cut by the plane n·p = d along one of its own axes: the
/// two bricks, each placed in the original's frame, the low one first (the side n points away from).
fn split_brick(shape: &Shape, n: V3, d: f64, cut: FaceId) -> Option<(Shape, Shape)> {
    // The chain of transforms over a brick, outermost first.
    let mut chain: Vec<([[f64; 3]; 3], V3, V3)> = Vec::new();
    let mut s = shape;
    loop {
        match s {
            Shape::Transform { inner, rot, offset, scale } => {
                chain.push((*rot, *offset, *scale));
                s = inner;
            }
            Shape::Brick { .. } => break,
            _ => return None,
        }
    }
    let Shape::Brick { half, faces } = s else { return None };
    // World = R S local + t, composed from the outermost transform inward.
    let (mut m, mut t) = ([[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]], [0.0f64; 3]);
    for (rot, off, sc) in &chain {
        let a: [[f64; 3]; 3] = std::array::from_fn(|i| std::array::from_fn(|j| rot[i][j] * sc[j]));
        let nt: V3 = std::array::from_fn(|i| t[i] + (0..3).map(|k| m[i][k] * off[k]).sum::<f64>());
        let nm: [[f64; 3]; 3] = std::array::from_fn(|i| std::array::from_fn(|j| (0..3).map(|k| m[i][k] * a[k][j]).sum()));
        m = nm;
        t = nt;
    }
    // The plane in the brick's own coordinates: (Mᵀ n)·l = d - n·t.
    let nl: V3 = std::array::from_fn(|j| (0..3).map(|i| m[i][j] * n[i]).sum());
    let dl = d - (0..3).map(|i| n[i] * t[i]).sum::<f64>();
    let len = (nl[0] * nl[0] + nl[1] * nl[1] + nl[2] * nl[2]).sqrt();
    let ax = (0..3).max_by(|a, b| nl[*a].abs().total_cmp(&nl[*b].abs()))?;
    if (0..3).any(|j| j != ax && nl[j].abs() > 1e-9 * len) {
        return None;
    }
    let c = dl / nl[ax];
    if c <= -half[ax] + 1e-12 * half[ax] || c >= half[ax] - 1e-12 * half[ax] {
        return None;
    }
    let piece = |lo: f64, hi: f64, low_side: bool| -> Shape {
        let mut h = *half;
        h[ax] = (hi - lo) / 2.0;
        let mut f = *faces;
        // The cut face replaces the face the cut stands in for.
        f[2 * ax + usize::from(low_side)] = cut;
        let mut off = [0.0; 3];
        off[ax] = (lo + hi) / 2.0;
        let brick = Shape::Transform { inner: Box::new(Shape::Brick { half: h, faces: f }), rot: rotation([0.0, 0.0, 1.0], 0.0), offset: off, scale: [1.0; 3] };
        // Back inside the original chain of transforms.
        chain.iter().rev().fold(brick, |acc, (rot, off, sc)| Shape::Transform { inner: Box::new(acc), rot: *rot, offset: *off, scale: *sc })
    };
    let low = piece(-half[ax], c, true);
    let high = piece(c, half[ax], false);
    // Which piece is on the side n points away from (n·p < d)?
    if nl[ax] > 0.0 {
        Some((low, high))
    } else {
        Some((high, low))
    }
}
