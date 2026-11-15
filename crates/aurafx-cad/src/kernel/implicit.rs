//! The implicit kernel: a document becomes one signed distance field (`kernel::sdf`), drawn by
//! marching tetrahedra over a grid, with the points moved onto the exact surface. It never fails
//! topologically; edges and faces are not exact entities, so the faces it reports are *labelled
//! regions*: every triangle belongs to the feature whose own field is nearest, and to the face
//! of that feature's shape the surface normal points along (a brick's six sides, a cylinder's
//! side and caps, an extrusion's caps and one side per outline loop, a revolve's profile
//! segments). Labels are stable across edits while the feature exists, which is what the
//! downstream check follows.

use super::sdf::{feature_sdf, grid_bounds, placement_rotation, Sdf};
use super::*;
use crate::document::{BooleanOp, Feature, Shape};
use crate::mesh::mt;
use crate::path::{dist_point_segment, Polyline};
use glam::{DMat3, DVec2, DVec3, Vec3};
use rayon::prelude::*;
use std::collections::BTreeMap;

/// The implicit kernel. `resolution` is the number of grid cells along the longest side of the
/// body; `project` moves the contour's points onto the exact surface.
#[derive(Debug, Clone)]
pub struct ImplicitKernel {
    pub resolution: usize,
    pub project: bool,
}

impl Default for ImplicitKernel {
    fn default() -> Self {
        Self { resolution: 64, project: true }
    }
}

impl ImplicitKernel {
    pub fn new(resolution: usize) -> Self {
        Self { resolution: resolution.clamp(8, 1024), project: true }
    }
}

/// How the faces of one feature's shape are told apart. The side faces of an extrusion and the
/// faces of a revolve are the drawing entities of its sketch (a rectangle's four lines are four
/// faces), so a face keeps its name when the sketch's dimensions change.
enum Classifier {
    /// Six sides of a brick: x-, x+, y-, y+, z-, z+.
    Axes6,
    /// Side, bottom, top of a round body along z.
    Round3,
    Single,
    /// An extruded outline: bottom, top, then one side per loop. `rot`/`origin` take the
    /// feature frame to the sketch frame (extrusion along its z).
    /// Each side belongs to a drawing entity and carries that entity's stable number (`ids`,
    /// one per path), so a side keeps its name when the outline gains or loses a line.
    Prism {
        paths: Vec<Polyline>,
        ids: Vec<u64>,
        rot: DMat3,
        origin: DVec3,
    },
    /// A revolved profile: one face per outline path (radius = x, axial = y), numbered like
    /// the sides of a prism.
    Revolve {
        paths: Vec<Polyline>,
        ids: Vec<u64>,
        rot: DMat3,
        origin: DVec3,
        about_z: bool,
    },
    /// A loft between `z0` and `z1`: side, bottom, top.
    Loft {
        z0: f64,
        z1: f64,
    },
    /// A sweep: side, the start cap (at `a`, looking against `ta`), the end cap (at `b`, along `tb`).
    Sweep {
        a: DVec3,
        ta: DVec3,
        b: DVec3,
        tb: DVec3,
        tol: f64,
    },
}

struct FeatureFaces {
    /// An imported solid: its faces keep their own numbers.
    import: bool,
    id: Id,
    op: BooleanOp,
    rot: DMat3,
    translate: DVec3,
    sdf: Sdf,
    class: Classifier,
    labels: Vec<(String, FaceKind)>,
}

fn round_labels() -> Vec<(String, FaceKind)> {
    vec![("side".into(), FaceKind::Cylinder), ("bottom".into(), FaceKind::Plane), ("top".into(), FaceKind::Plane)]
}

impl FeatureFaces {
    fn new(f: &Feature) -> Self {
        let (class, labels): (Classifier, Vec<(String, FaceKind)>) = match &f.shape {
            Shape::Box { .. } => (Classifier::Axes6, ["x-", "x+", "y-", "y+", "z-", "z+"].iter().map(|s| (s.to_string(), FaceKind::Plane)).collect()),
            Shape::Sphere { .. } => (Classifier::Single, vec![("surface".into(), FaceKind::Sphere)]),
            Shape::Torus { .. } => (Classifier::Single, vec![("surface".into(), FaceKind::Torus)]),
            Shape::Cylinder { .. } | Shape::Hole { .. } => (Classifier::Round3, round_labels()),
            Shape::Cone { .. } => {
                (Classifier::Round3, vec![("side".into(), FaceKind::Cone), ("bottom".into(), FaceKind::Plane), ("top".into(), FaceKind::Plane)])
            }
            Shape::Extrude { sketch, .. } => prism(sketch.clone(), (1..=sketch.len() as u64).collect(), DMat3::IDENTITY, DVec3::ZERO),
            Shape::SketchExtrude { sketch, .. } => {
                let (rot, origin) = sketch.placement_frame();
                prism(sketch.entity_paths(), sketch.drawing_ids(), rot, origin)
            }
            Shape::Revolve { profile, .. } => revolve(profile.clone(), (1..=profile.len() as u64).collect(), DMat3::IDENTITY, DVec3::ZERO, true),
            Shape::SketchRevolve { sketch, .. } => {
                let (rot, origin) = sketch.placement_frame();
                revolve(sketch.entity_paths(), sketch.drawing_ids(), rot, origin, false)
            }
            Shape::Loft { sections } if sections.len() >= 2 => {
                let (z0, z1) = sections.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), s| (a.min(s.z), b.max(s.z)));
                (Classifier::Loft { z0, z1 }, vec![("side".into(), FaceKind::Freeform), ("bottom".into(), FaceKind::Plane), ("top".into(), FaceKind::Plane)])
            }
            Shape::Sweep { path, .. } if path.len() >= 2 => {
                let p: Vec<DVec3> = path.iter().map(|q| DVec3::from_array(*q)).collect();
                let n = p.len();
                let len: f64 = p.windows(2).map(|w| w[0].distance(w[1])).sum();
                (
                    Classifier::Sweep {
                        a: p[0],
                        ta: (p[1] - p[0]).normalize_or(DVec3::X),
                        b: p[n - 1],
                        tb: (p[n - 1] - p[n - 2]).normalize_or(DVec3::X),
                        tol: (len * 0.01).max(1e-6),
                    },
                    vec![("side".into(), FaceKind::Freeform), ("start".into(), FaceKind::Plane), ("end".into(), FaceKind::Plane)],
                )
            }
            Shape::Sweep { .. }
            | Shape::Loft { .. }
            | Shape::SketchSweep { .. }
            | Shape::SketchLoft { .. }
            | Shape::Thread { .. }
            | Shape::Body { .. }
            | Shape::Brep { .. }
            | Shape::EdgeBlend { .. }
            | Shape::ShellFaces { .. }
            | Shape::MirrorPlane { .. }
            | Shape::Draft { .. } => (Classifier::Single, vec![("surface".into(), FaceKind::Freeform)]),
        };
        FeatureFaces {
            import: matches!(f.shape, Shape::Brep { .. }),
            id: f.id,
            op: f.op,
            rot: placement_rotation(f.placement.rotate_deg),
            translate: DVec3::from_array(f.placement.translate),
            sdf: feature_sdf(f),
            class,
            labels,
        }
    }

    /// The face of this feature at world point `p`, given the body's outward normal there.
    fn face_at(&self, p: DVec3, n_body: DVec3) -> usize {
        // The feature's own outward normal: a cutter's faces look into the body.
        let n_world = if self.op == BooleanOp::Cut { -n_body } else { n_body };
        let rt = self.rot.transpose();
        let q = rt * (p - self.translate);
        let n = rt * n_world;
        match &self.class {
            Classifier::Axes6 => {
                let a = [n.x.abs(), n.y.abs(), n.z.abs()];
                let k = if a[0] >= a[1] && a[0] >= a[2] {
                    0
                } else if a[1] >= a[2] {
                    1
                } else {
                    2
                };
                2 * k + usize::from(n[k] > 0.0)
            }
            Classifier::Round3 => {
                if n.z.abs() > 0.8 {
                    if n.z > 0.0 {
                        2
                    } else {
                        1
                    }
                } else {
                    0
                }
            }
            Classifier::Single => 0,
            Classifier::Prism { paths, ids, rot, origin } => {
                let st = rot.transpose();
                let (qs, ns) = (st * (q - *origin), st * n);
                if ns.z.abs() > 0.7071 {
                    return usize::from(ns.z > 0.0);
                }
                let c = DVec2::new(qs.x, qs.y);
                // Side faces: 2, 3, ... by the number of the entity they come from.
                1 + ids.get(nearest_path(paths, c)).copied().unwrap_or(1).max(1) as usize
            }
            Classifier::Revolve { paths, ids, rot, origin, about_z } => {
                let qs = if *about_z { q } else { rot.transpose() * (q - *origin) };
                let c = if *about_z { DVec2::new(qs.x.hypot(qs.y), qs.z) } else { DVec2::new(qs.x.hypot(qs.z), qs.y) };
                ids.get(nearest_path(paths, c)).copied().unwrap_or(1).max(1) as usize - 1
            }
            Classifier::Loft { z0, z1 } => {
                let tol = ((z1 - z0) * 0.02).max(1e-6);
                if n.z < -0.7 && (q.z - z0).abs() < tol {
                    1
                } else if n.z > 0.7 && (q.z - z1).abs() < tol {
                    2
                } else {
                    0
                }
            }
            Classifier::Sweep { a, ta, b, tb, tol } => {
                if n.dot(*ta) < -0.7 && (q - *a).dot(*ta).abs() < *tol {
                    1
                } else if n.dot(*tb) > 0.7 && (q - *b).dot(*tb).abs() < *tol {
                    2
                } else {
                    0
                }
            }
        }
    }
}

/// The index of the path nearest to `c`.
fn nearest_path(paths: &[Polyline], c: DVec2) -> usize {
    paths
        .iter()
        .enumerate()
        .map(|(i, l)| (i, l.segments().map(|(a, b)| dist_point_segment(c, a, b)).fold(f64::INFINITY, f64::min)))
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(i, _)| i)
        .unwrap_or(0)
}

fn prism(paths: Vec<Polyline>, ids: Vec<u64>, rot: DMat3, origin: DVec3) -> (Classifier, Vec<(String, FaceKind)>) {
    let mut labels = vec![("bottom".to_string(), FaceKind::Plane), ("top".to_string(), FaceKind::Plane)];
    // The label of side `id` is at 1 + id: the list has a place for every number up to the largest.
    for id in 1..=ids.iter().copied().max().unwrap_or(1).max(1) {
        labels.push((format!("side {id}"), FaceKind::Freeform));
    }
    (Classifier::Prism { paths, ids, rot, origin }, labels)
}

fn revolve(paths: Vec<Polyline>, ids: Vec<u64>, rot: DMat3, origin: DVec3, about_z: bool) -> (Classifier, Vec<(String, FaceKind)>) {
    let labels = (1..=ids.iter().copied().max().unwrap_or(1).max(1)).map(|id| (format!("profile {id}"), FaceKind::Freeform)).collect();
    (Classifier::Revolve { paths, ids, rot, origin, about_z }, labels)
}

/// Gradient of the field by central differences.
fn gradient(sdf: &Sdf, p: DVec3, e: f64) -> DVec3 {
    DVec3::new(
        sdf.eval(p + DVec3::X * e) - sdf.eval(p - DVec3::X * e),
        sdf.eval(p + DVec3::Y * e) - sdf.eval(p - DVec3::Y * e),
        sdf.eval(p + DVec3::Z * e) - sdf.eval(p - DVec3::Z * e),
    ) / (2.0 * e)
}

/// Move the contour's points onto the exact surface (Newton steps along the gradient, kept only
/// while they bring the point closer and never more than a fifth of a cell in all: near edges the
/// gradient jumps between faces and longer moves fold the triangles).
fn project_points(sdf: &Sdf, pts: &mut [DVec3], tris: &[[u32; 3]], h: f64) {
    let before = pts.to_vec();
    pts.par_iter_mut().for_each(|p| {
        let start = *p;
        let g0 = gradient(sdf, start, h * 1e-3);
        for _ in 0..4 {
            let d = sdf.eval(*p);
            if d.abs() < 1e-12 * h {
                break;
            }
            let g = gradient(sdf, *p, h * 1e-3);
            let gg = g.length_squared();
            if gg < 1e-12 {
                break;
            }
            let q = *p - g * (d / gg);
            if q.distance(start) > 0.2 * h || sdf.eval(q).abs() >= d.abs() {
                break;
            }
            *p = q;
        }
        // Near an edge the surface turns: leave such points where the contour put them.
        if *p != start {
            let g1 = gradient(sdf, *p, h * 1e-3);
            if g0.length() == 0.0 || g0.dot(g1) < 0.9 * g0.length() * g1.length() {
                *p = start;
            }
        }
    });
    // Snapping must not turn a triangle over: put such corners back, until none turns.
    let normal = |p: &[DVec3], t: &[u32; 3]| (p[t[1] as usize] - p[t[0] as usize]).cross(p[t[2] as usize] - p[t[0] as usize]);
    for _ in 0..8 {
        let mut undo = Vec::new();
        for t in tris {
            if normal(&before, t).dot(normal(pts, t)) <= 0.0 {
                undo.extend(t.iter().copied());
            }
        }
        if undo.is_empty() {
            break;
        }
        for i in undo {
            pts[i as usize] = before[i as usize];
        }
    }
}

fn unique_names(doc: &Document) -> Vec<String> {
    let mut seen: BTreeMap<String, usize> = BTreeMap::new();
    doc.features
        .iter()
        .map(|f| {
            let n = seen.entry(f.name.clone()).or_insert(0);
            *n += 1;
            if *n == 1 {
                f.name.clone()
            } else {
                format!("{} {}", f.name, n)
            }
        })
        .collect()
}

/// Names the faces of any surface of this document the way the implicit kernel does: the owner
/// of a point on the surface is the feature whose own field is nearest, the face is the one its
/// normal points along. The B-rep kernel labels its exact faces with it, so both kernels agree
/// on names and ids.
pub struct Labeller {
    ffs: Vec<FeatureFaces>,
    names: Vec<String>,
    ids: Vec<Id>,
    sdf: Sdf,
    h: f64,
}

impl Labeller {
    /// For a resolved document.
    pub fn new(doc: &Document) -> Self {
        let ffs: Vec<FeatureFaces> = doc.features.iter().filter(|f| !f.suppressed).map(FeatureFaces::new).collect();
        let all = unique_names(doc);
        let (names, ids): (Vec<String>, Vec<Id>) = doc.features.iter().enumerate().filter(|(_, f)| !f.suppressed).map(|(i, f)| (all[i].clone(), f.id)).unzip();
        let sdf = doc.raw_sdf().unwrap_or(Sdf::Empty);
        let (lo, hi) = grid_bounds(&sdf);
        Self { ffs, names, ids, h: ((hi - lo).max_element() / 64.0).max(1e-6), sdf }
    }
    /// `(feature index, local face)` of the surface point `p` whose outward normal is `n`.
    pub fn label(&self, p: DVec3, n: DVec3) -> (usize, usize) {
        let owner = self.ffs.iter().enumerate().map(|(i, f)| (i, f.sdf.eval(p).abs())).min_by(|a, b| a.1.total_cmp(&b.1)).map(|(i, _)| i).unwrap_or(0);
        let _ = self.h;
        let _ = &self.sdf;
        (owner, self.ffs.get(owner).map(|f| f.face_at(p, n)).unwrap_or(0))
    }
    /// Like [`Labeller::label`] for a triangle of the exact face number `face`: the faces of an
    /// imported solid are told apart by it (`part face 3`).
    pub fn label_in(&self, p: DVec3, n: DVec3, face: u32) -> (usize, usize) {
        let (owner, local) = self.label(p, n);
        if self.ffs.get(owner).is_some_and(|f| f.import) {
            (owner, face as usize)
        } else {
            (owner, local)
        }
    }
    /// Name, kind and owning feature id of a label.
    pub fn describe(&self, feature: usize, local: usize) -> (String, FaceKind, Id) {
        if self.ffs[feature].import {
            return (format!("{} face {}", self.names[feature], local + 1), FaceKind::Freeform, self.ids[feature]);
        }
        let (label, kind) = self.ffs[feature].labels.get(local).cloned().unwrap_or(("surface".into(), FaceKind::Freeform));
        (format!("{} {}", self.names[feature], label), kind, self.ids[feature])
    }
}

impl Kernel for ImplicitKernel {
    fn name(&self) -> &'static str {
        "implicit"
    }

    fn evaluate(&self, document: &Document) -> Result<Evaluated, CadError> {
        let err = |reason: String| CadError::Kernel { kernel: "implicit", reason };
        let doc = document.resolved()?;
        let sdf = doc.raw_sdf().ok_or_else(|| err("the document has no solid yet: add a feature that adds material".into()))?;
        let (lo, hi) = grid_bounds(&sdf);
        let size = (hi - lo).max_element();
        if !(size.is_finite() && size > 1e-9) {
            return Err(err("the solid is empty or has no size".into()));
        }
        let h = size / self.resolution.clamp(8, 1024) as f64;
        // Off round coordinates and no exact zeros: faces lying on grid points make the contour
        // degenerate (slivers and holes).
        let shift = DVec3::new(0.0137, 0.0219, 0.0071) * h;
        let grid = mt::Grid::sample((lo - shift).as_vec3(), (hi - shift).as_vec3(), h as f32, |p| {
            let d = sdf.eval(p.as_dvec3()) as f32;
            if d == 0.0 {
                1e-9 * h as f32
            } else {
                d
            }
        });
        let mesh = mt::extract(&grid, 0.0);
        let mut pts: Vec<DVec3> = mesh.positions.iter().map(|p: &Vec3| p.as_dvec3()).collect();
        let mut tris: Vec<[u32; 3]> = mesh.indices.clone();
        if self.project {
            project_points(&sdf, &mut pts, &tris, h);
        }
        let area = |p: &[DVec3], t: &[u32; 3]| 0.5 * (p[t[1] as usize] - p[t[0] as usize]).cross(p[t[2] as usize] - p[t[0] as usize]).length();
        tris.retain(|t| area(&pts, t) > 1e-10 * h * h);
        // Turn the surface outward (positive enclosed volume).
        let vol6 = |p: &[DVec3], t: &[u32; 3]| p[t[0] as usize].dot(p[t[1] as usize].cross(p[t[2] as usize]));
        if tris.iter().map(|t| vol6(&pts, t)).sum::<f64>() < 0.0 {
            tris.iter_mut().for_each(|t| t.swap(1, 2));
        }

        // Which feature and which face of it every triangle belongs to.
        let ffs: Vec<FeatureFaces> = doc.features.iter().filter(|f| !f.suppressed).map(FeatureFaces::new).collect();
        let names = unique_names(&doc);
        let name_of = |id: Id| doc.features.iter().position(|f| f.id == id).map(|i| names[i].clone()).unwrap_or_default();
        let assigned: Vec<(usize, usize)> = tris
            .par_iter()
            .map(|t| {
                let c = (pts[t[0] as usize] + pts[t[1] as usize] + pts[t[2] as usize]) / 3.0;
                let owner = ffs.iter().enumerate().map(|(i, f)| (i, f.sdf.eval(c).abs())).min_by(|a, b| a.1.total_cmp(&b.1)).map(|(i, _)| i).unwrap_or(0);
                let n = gradient(&sdf, c, h * 0.1);
                let n = if n.length() > 0.0 { n.normalize() } else { DVec3::Z };
                (owner, ffs.get(owner).map(|f| f.face_at(c, n)).unwrap_or(0))
            })
            .collect();

        // Global face ids in (feature, local face) order.
        let mut keys: Vec<(usize, usize)> = assigned.clone();
        keys.sort_unstable();
        keys.dedup();
        let face_id: BTreeMap<(usize, usize), u32> = keys.iter().enumerate().map(|(i, k)| (*k, i as u32)).collect();
        let face: Vec<u32> = assigned.iter().map(|k| face_id[k]).collect();

        let mut faces: Vec<EvaluatedFace> = keys
            .iter()
            .map(|&(fi, li)| {
                let ff = &ffs[fi];
                let (label, kind) = ff.labels.get(li).cloned().unwrap_or(("surface".into(), FaceKind::Freeform));
                EvaluatedFace {
                    id: (ff.id, li as u32),
                    face_id: face_id[&(fi, li)],
                    name: format!("{} {}", name_of(ff.id), label),
                    kind,
                    area: 0.0,
                    centroid: [0.0; 3],
                    direction: [0.0; 3],
                }
            })
            .collect();
        let (mut vol, mut total_area, mut cen) = (0.0, 0.0, DVec3::ZERO);
        let mut acc: Vec<(f64, DVec3, DVec3)> = vec![(0.0, DVec3::ZERO, DVec3::ZERO); faces.len()];
        for (t, f) in tris.iter().zip(&face) {
            let (a, b, c) = (pts[t[0] as usize], pts[t[1] as usize], pts[t[2] as usize]);
            let nrm = (b - a).cross(c - a);
            let ar = 0.5 * nrm.length();
            let v6 = a.dot(b.cross(c));
            vol += v6 / 6.0;
            cen += (a + b + c) * v6 / 24.0;
            total_area += ar;
            let e = &mut acc[*f as usize];
            e.0 += ar;
            e.1 += (a + b + c) / 3.0 * ar;
            e.2 += nrm * 0.5;
        }
        for (fc, (ar, wc, wn)) in faces.iter_mut().zip(&acc) {
            fc.area = *ar;
            if *ar > 0.0 {
                fc.centroid = (*wc / *ar).to_array();
            }
            let l = wn.length();
            if l > 0.0 {
                fc.direction = (*wn / l).to_array();
            }
        }
        let (mut blo, mut bhi) = (DVec3::splat(f64::INFINITY), DVec3::splat(f64::NEG_INFINITY));
        for t in &tris {
            for &i in t {
                blo = blo.min(pts[i as usize]);
                bhi = bhi.max(pts[i as usize]);
            }
        }
        let body_id = doc.features.first().map(|f| f.id).unwrap_or(Id(1));
        let body_name = if doc.name.is_empty() { "Body".to_string() } else { doc.name.clone() };
        let face_names: Vec<String> = faces.iter().map(|f| f.name.clone()).collect();
        let surface = surface_dataset(
            &[BodyMesh { id: body_id, name: body_name.clone(), points: pts.iter().map(|p| p.to_array()).collect(), tris, face }],
            &face_names,
            &[vol],
            &[total_area],
        );
        let sdf = std::sync::Arc::new(sdf);
        Ok(Evaluated {
            bodies: vec![EvaluatedBody {
                id: body_id,
                name: body_name,
                faces,
                volume: vol,
                area: total_area,
                centroid: if vol.abs() > 1e-12 { (cen / vol).to_array() } else { [0.0; 3] },
                bounds: (blo.to_array(), bhi.to_array()),
            }],
            surface,
            distance: Some(std::sync::Arc::new(move |p| sdf.eval(DVec3::from_array(p)))),
            kernel: "implicit",
            approximate: doc
                .features
                .iter()
                .filter(|f| !f.suppressed)
                .filter_map(|f| match &f.shape {
                    Shape::EdgeBlend { .. } => {
                        Some(format!("'{}' is not built by the implicit kernel: the picked edges stay sharp (choose the B-rep kernel)", f.name))
                    }
                    Shape::ShellFaces { .. } => Some(format!("'{}' hollows the part with an approximate open face on the implicit kernel", f.name)),
                    Shape::Draft { .. } => {
                        Some(format!("'{}' is not built by the implicit kernel: the picked faces stay vertical (choose the B-rep kernel)", f.name))
                    }
                    // An open shell of a rounded box: the exact kernel offsets the rounded rim, this one cuts the opening straight.
                    Shape::Box { corner_radius, .. }
                        if *corner_radius > 0.0 && f.modifiers.iter().any(|m| matches!(m, crate::document::Modifier::ShellOpen { open_top: true, .. })) =>
                    {
                        Some(format!(
                            "'{}': the rim of the open shell is approximate on the implicit kernel (the exact kernel offsets the rounded rim)",
                            f.name
                        ))
                    }
                    _ => None,
                })
                .collect(),
            mates: Vec::new(),
            mate_dof: None,
            placements: Vec::new(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::*;
    use crate::sketch::Sketch;
    use std::f64::consts::PI;

    fn one(shape: Shape) -> Document {
        Document::new("part").add(Feature::new("F", BooleanOp::Add, shape))
    }
    fn close(v: f64, e: f64, tol: f64) -> bool {
        (v - e).abs() / e.abs() < tol
    }
    fn eval(doc: &Document) -> Evaluated {
        ImplicitKernel::new(96).evaluate(doc).unwrap()
    }

    #[test]
    fn primitives_match_their_analytic_volumes() {
        let b = eval(&one(Shape::Box { size: [40.0, 20.0, 10.0], corner_radius: 0.0 }));
        assert!(close(b.volume(), 8000.0, 0.01), "box {}", b.volume());
        assert_eq!(b.bodies[0].faces.len(), 6);
        let c = eval(&one(Shape::Cylinder { radius: 10.0, height: 30.0 }));
        assert!(close(c.volume(), PI * 100.0 * 30.0, 0.01), "cylinder {}", c.volume());
        assert_eq!(c.bodies[0].faces.len(), 3);
        let s = eval(&one(Shape::Sphere { radius: 12.0 }));
        assert!(close(s.volume(), 4.0 / 3.0 * PI * 1728.0, 0.01), "sphere {}", s.volume());
        let t = eval(&one(Shape::Torus { major: 20.0, minor: 5.0 }));
        assert!(close(t.volume(), 2.0 * PI * PI * 20.0 * 25.0, 0.015), "torus {}", t.volume());
    }

    #[test]
    fn extrude_and_revolve_of_sketches() {
        let ex = one(Shape::SketchExtrude {
            sketch: Sketch::rectangle(30.0, 12.0),
            depth: 8.0,
            end: EndCondition::Blind,
            draft_deg: 0.0,
            top_edge: EdgeTreatment::None,
            bottom_edge: EdgeTreatment::None,
            reverse: false,
        });
        let e = eval(&ex);
        assert!(close(e.volume(), 30.0 * 12.0 * 8.0, 0.01), "extrude {}", e.volume());
        let names: Vec<_> = e.bodies[0].faces.iter().map(|f| f.name.as_str()).collect();
        assert!(names.contains(&"F bottom") && names.contains(&"F top") && names.contains(&"F side 1"), "{names:?}");
        // A rectangle revolved about its local y axis gives a cylinder of the width as diameter.
        let mut sk = Sketch::polygon(&[[0.0, 0.0], [10.0, 0.0], [10.0, 25.0], [0.0, 25.0]]);
        sk.plane = crate::sketch::SketchPlane::XZ;
        let rv = eval(&one(Shape::SketchRevolve { sketch: sk, angle_deg: 360.0 }));
        assert!(close(rv.volume(), PI * 100.0 * 25.0, 0.015), "revolve {}", rv.volume());
    }

    #[test]
    fn a_hole_adds_a_named_face_and_removes_volume() {
        let doc = Document::new("plate")
            .add(Feature::new("Plate", BooleanOp::Add, Shape::Box { size: [40.0, 40.0, 10.0], corner_radius: 0.0 }))
            .add(Feature::new("Bore", BooleanOp::Cut, Shape::Cylinder { radius: 5.0, height: 30.0 }).at(0.0, 0.0, -10.0));
        let e = eval(&doc);
        let expect = 40.0 * 40.0 * 10.0 - PI * 25.0 * 10.0;
        assert!(close(e.volume(), expect, 0.01), "{} vs {expect}", e.volume());
        let wall = e.face_by_name("Bore side").expect("the hole's wall is a face");
        assert!(close(wall.area, 2.0 * PI * 5.0 * 10.0, 0.04), "wall area {}", wall.area);
        assert_eq!(wall.kind, FaceKind::Cylinder);
        // The plate's top and bottom each lose the hole's disc.
        let top = e.face_by_name("Plate z+").unwrap();
        assert!(close(top.area, 1600.0 - PI * 25.0, 0.02), "{}", top.area);
        assert!(top.direction[2] > 0.99);
        // Face ids are dense and the surface lists the names in that order.
        let DataSet::PolyData(pd) = &e.surface else { panic!() };
        let names = pd.field_data.get("FaceNames").unwrap();
        assert_eq!(names.len(), e.bodies[0].faces.len());
        let ids = pd.cell_data.get("FaceId").unwrap();
        assert_eq!(ids.len(), pd.polys.len());
    }

    #[test]
    fn faces_keep_their_identity_when_a_parameter_changes() {
        let mut doc = Document::new("p")
            .param("w", "40")
            .add(Feature::new("Base", BooleanOp::Add, Shape::Box { size: [1.0, 20.0, 10.0], corner_radius: 0.0 }).bind("/shape/size/0", "w"));
        let a = eval(&doc);
        doc.set_parameter("w", "60");
        let b = eval(&doc);
        assert!(close(b.volume(), 60.0 * 200.0, 0.01), "{}", b.volume());
        let ids = |e: &Evaluated| e.faces().map(|(_, f)| f.id).collect::<Vec<_>>();
        assert_eq!(ids(&a), ids(&b));
        let (fa, fb) = (a.face_by_name("Base x+").unwrap(), b.face_by_name("Base x+").unwrap());
        assert!(fb.centroid[0] > fa.centroid[0] + 9.0);
    }

    #[test]
    fn empty_and_cut_only_documents_are_errors_with_a_message() {
        let e = ImplicitKernel::default().evaluate(&Document::new("empty")).unwrap_err();
        assert!(e.to_string().contains("no solid"), "{e}");
        let only_cut = Document::new("c").add(Feature::new("Cut", BooleanOp::Cut, Shape::Sphere { radius: 3.0 }));
        assert!(ImplicitKernel::default().evaluate(&only_cut).is_err());
    }

    #[test]
    fn every_template_evaluates_with_positive_volume() {
        for (name, doc) in crate::templates::templates() {
            let e = ImplicitKernel::new(48).evaluate(&doc).unwrap_or_else(|e| panic!("{name}: {e}"));
            assert!(e.volume() > 0.0, "{name}");
            assert!(!e.bodies[0].faces.is_empty(), "{name}");
            let (d, v) = (e.distance.as_ref().unwrap(), e.bodies[0].centroid);
            let _ = (d(v), v);
        }
    }
}
