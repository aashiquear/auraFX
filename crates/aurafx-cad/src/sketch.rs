//! Ported from EZ-YX-craft (`ezyx-cad`, MIT OR Apache-2.0), the user's own project.
//!
//! 2D constrained sketcher.
//!
//! A [`Sketch`] is points + entities that reference points by index + geometric/dimensional
//! [`Constraint`]s. [`Sketch::solve`] runs a damped Gauss-Newton (Levenberg–Marquardt) solve over all
//! free scalars (coordinates of non-fixed points and circle radii) with a central-difference Jacobian;
//! sketches are small (< a few hundred unknowns) so dense linear algebra is fine. The rank of the
//! Jacobian at the solution gives the remaining degrees of freedom, so the GUI can colour a sketch
//! under/fully/over-defined.
//!
//! Profiles for features come from [`Sketch::profiles`]: non-construction lines, arcs and open
//! splines are chained by shared point indices (or `Coincident` constraints) into closed loops;
//! circles, closed splines and closed polylines are loops on their own.

use crate::document::Placement;
use crate::expr::{self, ExprError};
use crate::path::{arc_segments, shapes, Polyline, DEFAULT_TOLERANCE};
use glam::{DMat3, DQuat, DVec2, DVec3, EulerRot};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::f64::consts::TAU;

/// A sketch point (mm, sketch-local coordinates).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, Default)]
pub struct SPoint {
    pub x: f64,
    pub y: f64,
    #[serde(default)]
    pub fixed: bool,
}

impl SPoint {
    pub fn new(x: f64, y: f64) -> Self {
        Self { x, y, fixed: false }
    }
    pub fn pos(&self) -> DVec2 {
        DVec2::new(self.x, self.y)
    }
}

/// Reference plane a sketch lies on. The sketch's local +Z (its normal) is the extrude direction.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum SketchPlane {
    /// Top plane: local x → X, y → Y, normal +Z.
    #[default]
    XY,
    /// Front plane: local x → X, y → Z, normal −Y.
    XZ,
    /// Right plane: local x → Y, y → Z, normal +X.
    YZ,
}

impl SketchPlane {
    /// Local→world rotation (columns: local x, local y, normal).
    pub fn basis(self) -> DMat3 {
        match self {
            SketchPlane::XY => DMat3::IDENTITY,
            SketchPlane::XZ => DMat3::from_cols(DVec3::X, DVec3::Z, DVec3::NEG_Y),
            SketchPlane::YZ => DMat3::from_cols(DVec3::Y, DVec3::Z, DVec3::X),
        }
    }
    pub fn normal(self) -> DVec3 {
        self.basis().z_axis
    }
    /// Rotation and origin of the plane shifted `offset` mm along its normal.
    pub fn frame(self, offset: f64) -> (DMat3, DVec3) {
        (self.basis(), self.normal() * offset)
    }
    /// The same frame as a feature [`Placement`] (XYZ Euler degrees).
    pub fn placement(self, offset: f64) -> Placement {
        let (r, t) = self.frame(offset);
        let (a, b, c) = DQuat::from_mat3(&r).to_euler(EulerRot::XYZ);
        Placement { translate: t.to_array(), rotate_deg: [a.to_degrees(), b.to_degrees(), c.to_degrees()] }
    }
    /// Sketch-local 2D point → world.
    pub fn to_world(self, offset: f64, p: DVec2) -> DVec3 {
        let (r, t) = self.frame(offset);
        r * p.extend(0.0) + t
    }
}

/// A plane anywhere: an origin, the direction of the sketch's x axis in it and its normal
/// (the y axis is normal × x). Used for sketches on slanted faces and on reference planes.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct PlaneFrame {
    pub origin: [f64; 3],
    pub x_axis: [f64; 3],
    pub normal: [f64; 3],
}

impl PlaneFrame {
    /// A frame from an origin and a normal; the x axis is the given direction made
    /// perpendicular to the normal (world x, or world y when x is along the normal, if none).
    pub fn new(origin: [f64; 3], normal: [f64; 3], x_hint: Option<[f64; 3]>) -> Option<PlaneFrame> {
        let n = DVec3::from_array(normal);
        if n.length() < 1e-12 {
            return None;
        }
        let n = n.normalize();
        let pick = |v: DVec3| {
            let v = v - n * v.dot(n);
            (v.length() > 1e-6).then(|| v.normalize())
        };
        let x = x_hint.and_then(|h| pick(DVec3::from_array(h))).or_else(|| pick(DVec3::X)).or_else(|| pick(DVec3::Y))?;
        Some(PlaneFrame { origin, x_axis: x.to_array(), normal: n.to_array() })
    }
    /// The plane through three points (normal by the right-hand rule a → b → c; x along a → b).
    pub fn through(a: [f64; 3], b: [f64; 3], c: [f64; 3]) -> Option<PlaneFrame> {
        let (a3, b3, c3) = (DVec3::from_array(a), DVec3::from_array(b), DVec3::from_array(c));
        let n = (b3 - a3).cross(c3 - a3);
        if n.length() < 1e-12 * (b3 - a3).length().max(1.0).powi(2) {
            return None;
        }
        Self::new(a, n.to_array(), Some((b3 - a3).to_array()))
    }
    /// A principal plane moved `offset` along its normal, then turned `angle` degrees about its
    /// own local x (`about_x`) or y axis.
    pub fn from_principal(plane: SketchPlane, offset: f64, angle_deg: f64, about_x: bool) -> PlaneFrame {
        let (r, t) = plane.frame(offset);
        let axis = if about_x { r.x_axis } else { r.y_axis };
        let q = DQuat::from_axis_angle(axis, angle_deg.to_radians());
        PlaneFrame { origin: t.to_array(), x_axis: (q * r.x_axis).to_array(), normal: (q * r.z_axis).to_array() }
    }
    /// Local → world rotation (columns: x, y, normal), made orthonormal.
    pub fn basis(&self) -> DMat3 {
        let n = DVec3::from_array(self.normal).normalize_or(DVec3::Z);
        let x = DVec3::from_array(self.x_axis);
        let x = (x - n * x.dot(n)).normalize_or(n.any_orthonormal_vector());
        DMat3::from_cols(x, n.cross(x), n)
    }
    /// The same plane moved `offset` along its normal.
    pub fn offset(&self, offset: f64) -> PlaneFrame {
        let n = self.basis().z_axis;
        let o = DVec3::from_array(self.origin) + n * offset;
        PlaneFrame { origin: o.to_array(), ..*self }
    }
}

/// Sketch geometry. Points are referenced by index into [`Sketch::points`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum Entity {
    Line {
        a: usize,
        b: usize,
        #[serde(default)]
        construction: bool,
    },
    Circle {
        center: usize,
        radius: f64,
        #[serde(default)]
        construction: bool,
    },
    /// Counter-clockwise from `start` to `end` around `center` (radius = |start − center|).
    Arc {
        center: usize,
        start: usize,
        end: usize,
        #[serde(default)]
        construction: bool,
    },
    /// Uniform Catmull-Rom spline through the control points.
    Spline {
        ctrl: Vec<usize>,
        #[serde(default)]
        closed: bool,
        #[serde(default)]
        construction: bool,
    },
    /// Fixed imported geometry (text outlines, DXF/SVG traces). Not touched by the solver.
    Polyline {
        pts: Vec<[f64; 2]>,
        #[serde(default)]
        closed: bool,
        #[serde(default)]
        construction: bool,
    },
}

impl Entity {
    pub fn construction(&self) -> bool {
        match self {
            Entity::Line { construction, .. }
            | Entity::Circle { construction, .. }
            | Entity::Arc { construction, .. }
            | Entity::Spline { construction, .. }
            | Entity::Polyline { construction, .. } => *construction,
        }
    }
    pub fn set_construction(&mut self, v: bool) {
        match self {
            Entity::Line { construction, .. }
            | Entity::Circle { construction, .. }
            | Entity::Arc { construction, .. }
            | Entity::Spline { construction, .. }
            | Entity::Polyline { construction, .. } => *construction = v,
        }
    }
}

/// A driving dimension: a value (mm or degrees) optionally driven by an expression of parameters.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct Dim {
    pub value: f64,
    #[serde(default)]
    pub expr: Option<String>,
    /// Where the label sits, as an offset from its usual place (sketch units); None: the usual place.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub at: Option<[f64; 2]>,
}

impl Dim {
    pub fn new(value: f64) -> Self {
        Self { value, expr: None, at: None }
    }
    /// Expression-driven dimension (value filled in by [`Sketch::solve_with`]).
    pub fn expr(expr: impl Into<String>) -> Self {
        Self { value: 0.0, expr: Some(expr.into()), at: None }
    }
}

impl From<f64> for Dim {
    fn from(v: f64) -> Self {
        Dim::new(v)
    }
}

/// Sketch constraints. `p`/`q` are point indices; lines/circles/arcs are entity indices.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Constraint {
    Coincident(usize, usize),
    Horizontal(usize),
    Vertical(usize),
    /// Point–point distance.
    Distance(usize, usize, Dim),
    /// |Δx| between two points.
    HorizontalDistance(usize, usize, Dim),
    /// |Δy| between two points.
    VerticalDistance(usize, usize, Dim),
    Length(usize, Dim),
    /// Circle or arc radius.
    Radius(usize, Dim),
    Diameter(usize, Dim),
    /// Unsigned angle between two lines' directions, degrees (0..180).
    Angle(usize, usize, Dim),
    Parallel(usize, usize),
    Perpendicular(usize, usize),
    /// Equal line lengths or equal radii (a line and a circle: length = radius).
    Equal(usize, usize),
    /// Line tangent to circle/arc, or two circles/arcs externally tangent.
    Tangent(usize, usize),
    Concentric(usize, usize),
    /// Point on the (infinite) line through a line entity.
    PointOnLine(usize, usize),
    PointOnCircle(usize, usize),
    /// Point at the midpoint of a line.
    Midpoint(usize, usize),
    /// The perpendicular distance from a point to a line (also between parallel lines, through
    /// an end of one).
    PointLineDistance(usize, usize, Dim),
    /// Points `p` and `q` symmetric about a line.
    Symmetric(usize, usize, usize),
    Fix(usize),
}

impl Constraint {
    pub fn dim(&self) -> Option<&Dim> {
        match self {
            Constraint::Distance(_, _, d)
            | Constraint::HorizontalDistance(_, _, d)
            | Constraint::VerticalDistance(_, _, d)
            | Constraint::Length(_, d)
            | Constraint::Radius(_, d)
            | Constraint::Diameter(_, d)
            | Constraint::Angle(_, _, d)
            | Constraint::PointLineDistance(_, _, d) => Some(d),
            _ => None,
        }
    }
    pub fn dim_mut(&mut self) -> Option<&mut Dim> {
        match self {
            Constraint::Distance(_, _, d)
            | Constraint::HorizontalDistance(_, _, d)
            | Constraint::VerticalDistance(_, _, d)
            | Constraint::Length(_, d)
            | Constraint::Radius(_, d)
            | Constraint::Diameter(_, d)
            | Constraint::Angle(_, _, d)
            | Constraint::PointLineDistance(_, _, d) => Some(d),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SolveStatus {
    FullyDefined,
    /// Converged with this many remaining degrees of freedom.
    UnderDefined(usize),
    /// Redundant or conflicting constraints.
    OverDefined,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SolveReport {
    pub converged: bool,
    pub max_residual: f64,
    pub iterations: usize,
    /// Remaining degrees of freedom (unknowns − Jacobian rank).
    pub dof: usize,
    pub status: SolveStatus,
    /// Constraints skipped because they reference missing/wrong-type geometry.
    pub ignored: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct Sketch {
    #[serde(default)]
    pub points: Vec<SPoint>,
    #[serde(default)]
    pub entities: Vec<Entity>,
    #[serde(default)]
    pub constraints: Vec<Constraint>,
    #[serde(default)]
    pub plane: SketchPlane,
    /// Plane offset along its normal (mm).
    #[serde(default)]
    pub offset: f64,
    /// A plane anywhere (a slanted face, a reference plane): replaces `plane` when set.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub frame: Option<PlaneFrame>,
    /// The name of the reference plane or face the sketch was started on (for display).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plane_name: Option<String>,
    /// The face of the part the sketch sits on, by its name: when the part is rebuilt the sketch
    /// plane is put back on that face (`plane`, `offset` and `frame` hold where it was last).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub face: Option<String>,
    /// A stable number for every entity (same order as `entities`): it stays with its entity
    /// when others are added or removed, and the faces a feature makes from the sketch are named
    /// by it (`side 3`). Entities without one yet get theirs from [`Sketch::normalize_entity_ids`].
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub entity_ids: Vec<u64>,
    /// The largest entity number handed out so far (never reused).
    #[serde(default, skip_serializing_if = "is_zero")]
    pub next_entity_id: u64,
}

fn is_zero(v: &u64) -> bool {
    *v == 0
}

/// Residual tolerance (mm) for convergence.
const TOL: f64 = 1e-9;

impl Sketch {
    pub fn new(plane: SketchPlane) -> Self {
        Self { plane, ..Default::default() }
    }
    pub fn on(mut self, plane: SketchPlane, offset: f64) -> Self {
        self.plane = plane;
        self.offset = offset;
        self
    }
    /// The sketch on a plane anywhere.
    pub fn on_frame(mut self, frame: PlaneFrame, name: Option<String>) -> Self {
        self.frame = Some(frame);
        self.plane_name = name;
        self
    }
    /// Rotation (columns: local x, local y, normal) and origin of the sketch plane, offset included.
    pub fn placement_frame(&self) -> (DMat3, DVec3) {
        match &self.frame {
            Some(f) => {
                let r = f.basis();
                (r, DVec3::from_array(f.origin) + r.z_axis * self.offset)
            }
            None => self.plane.frame(self.offset),
        }
    }
    /// Sketch-local point → world.
    pub fn to_world(&self, p: DVec2) -> DVec3 {
        let (r, t) = self.placement_frame();
        r * p.extend(0.0) + t
    }
    /// A short name of the plane the sketch is on.
    pub fn plane_label(&self) -> String {
        match (&self.plane_name, &self.frame) {
            (Some(n), _) => n.clone(),
            (None, Some(_)) => "a custom plane".into(),
            (None, None) => format!("the {:?} plane", self.plane),
        }
    }

    // ---- entity numbers -------------------------------------------------------------------
    /// The number of every entity as [`Sketch::normalize_entity_ids`] would leave it, and the
    /// largest one. Entities that have none yet are numbered after the largest: first the
    /// drawing entities in order, then the construction ones, so that a sketch that never had
    /// numbers (an older file) names its faces as before: `side 1` is its first drawing entity.
    fn assigned_ids(&self) -> (Vec<u64>, u64) {
        let n = self.entities.len();
        let mut ids: Vec<u64> = (0..n).map(|i| self.entity_ids.get(i).copied().unwrap_or(0)).collect();
        let mut seen = std::collections::HashSet::new();
        for id in ids.iter_mut() {
            if *id != 0 && !seen.insert(*id) {
                *id = 0;
            }
        }
        let mut next = self.next_entity_id.max(ids.iter().copied().max().unwrap_or(0));
        for construction in [false, true] {
            for (i, e) in self.entities.iter().enumerate() {
                if ids[i] == 0 && e.construction() == construction {
                    next += 1;
                    ids[i] = next;
                }
            }
        }
        (ids, next)
    }
    /// Give every entity a number of its own (kept when it has one already).
    pub fn normalize_entity_ids(&mut self) {
        let (ids, next) = self.assigned_ids();
        self.entity_ids = ids;
        self.next_entity_id = next;
    }
    /// The stable number of entity `i`.
    pub fn entity_id(&self, i: usize) -> u64 {
        self.assigned_ids().0.get(i).copied().unwrap_or(0)
    }
    /// The index of the entity with this number.
    pub fn entity_by_id(&self, id: u64) -> Option<usize> {
        self.assigned_ids().0.iter().position(|x| *x == id)
    }
    /// The numbers of the drawing (non-construction) entities, in the order of [`Sketch::entity_paths`].
    pub fn drawing_ids(&self) -> Vec<u64> {
        let ids = self.assigned_ids().0;
        self.entities.iter().enumerate().filter(|(_, e)| !e.construction()).map(|(i, _)| ids[i]).collect()
    }
    /// Remove entity `i` (its number goes with it; the others keep theirs) and every constraint
    /// that names it. Its points stay.
    pub fn remove_entity(&mut self, i: usize) {
        if i >= self.entities.len() {
            return;
        }
        self.normalize_entity_ids();
        self.entities.remove(i);
        self.entity_ids.remove(i);
        let shift = |e: usize| if e > i { e - 1 } else { e };
        use Constraint::*;
        self.constraints = std::mem::take(&mut self.constraints)
            .into_iter()
            .filter_map(|c| {
                Some(match c {
                    Horizontal(e) if e != i => Horizontal(shift(e)),
                    Vertical(e) if e != i => Vertical(shift(e)),
                    Length(e, d) if e != i => Length(shift(e), d),
                    Radius(e, d) if e != i => Radius(shift(e), d),
                    Diameter(e, d) if e != i => Diameter(shift(e), d),
                    Angle(a, b, d) if a != i && b != i => Angle(shift(a), shift(b), d),
                    Parallel(a, b) if a != i && b != i => Parallel(shift(a), shift(b)),
                    Perpendicular(a, b) if a != i && b != i => Perpendicular(shift(a), shift(b)),
                    Equal(a, b) if a != i && b != i => Equal(shift(a), shift(b)),
                    Tangent(a, b) if a != i && b != i => Tangent(shift(a), shift(b)),
                    Concentric(a, b) if a != i && b != i => Concentric(shift(a), shift(b)),
                    PointOnLine(p, e) if e != i => PointOnLine(p, shift(e)),
                    PointOnCircle(p, e) if e != i => PointOnCircle(p, shift(e)),
                    Midpoint(p, e) if e != i => Midpoint(p, shift(e)),
                    PointLineDistance(p, e, d) if e != i => PointLineDistance(p, shift(e), d),
                    Symmetric(p, q, e) if e != i => Symmetric(p, q, shift(e)),
                    c @ (Coincident(..) | Distance(..) | HorizontalDistance(..) | VerticalDistance(..) | Fix(..)) => c,
                    _ => return None,
                })
            })
            .collect();
    }

    // ---- placing on a face ----------------------------------------------------------------
    /// A new sketch on a flat face given by a point on it and its outward direction: on the
    /// matching principal plane when the face looks along x, y or z, otherwise on the face's own
    /// plane. The second value says the face looks against the sketch plane's normal (a boss
    /// then extrudes with `reverse`).
    pub fn on_face(point: [f64; 3], direction: [f64; 3], name: &str) -> Option<(Sketch, bool)> {
        let a = [direction[0].abs(), direction[1].abs(), direction[2].abs()];
        let k = (0..3).max_by(|i, j| a[*i].total_cmp(&a[*j]))?;
        let len = (a[0] * a[0] + a[1] * a[1] + a[2] * a[2]).sqrt();
        if len < 1e-12 {
            return None;
        }
        // Nearly along an axis is along it (a face's direction read off a rough build is not exact;
        // the sketch is put on the face's true plane when the part is built).
        if a[k] / len >= 0.999 {
            let plane = [SketchPlane::YZ, SketchPlane::XZ, SketchPlane::XY][k];
            let n = plane.normal().to_array();
            let offset = (0..3).map(|i| point[i] * n[i]).sum::<f64>();
            let reverse = (0..3).map(|i| direction[i] * n[i]).sum::<f64>() < 0.0;
            let mut s = Sketch::new(plane);
            s.offset = offset;
            s.plane_name = Some(name.to_string());
            s.face = Some(name.to_string());
            return Some((s, reverse));
        }
        let mut s = Sketch::new(SketchPlane::XY).on_frame(PlaneFrame::new(point, direction, None)?, Some(name.to_string()));
        s.face = Some(name.to_string());
        Some((s, false))
    }
    /// Put the sketch plane back on its face, now through `point` with the outward direction
    /// `direction`: a face that only moved along its normal moves the sketch with it; one that
    /// turned takes the sketch plane with it, the sketch's origin and x axis staying as near to
    /// where they were as the new plane allows.
    pub fn follow_face(&mut self, point: [f64; 3], direction: [f64; 3]) {
        let (r, o) = self.placement_frame();
        let n = DVec3::from_array(direction).normalize_or_zero();
        if n == DVec3::ZERO {
            return;
        }
        let p = DVec3::from_array(point);
        let along = r.z_axis.dot(n);
        if along.abs() > 1.0 - 1e-9 {
            let moved = (p - o).dot(r.z_axis);
            if moved.abs() > 1e-9 {
                self.offset += moved;
            }
            return;
        }
        // The side of the face the sketch's normal was on stays the same.
        let normal = if along < 0.0 { -n } else { n };
        let origin = o - n * (o - p).dot(n);
        if let Some(f) = PlaneFrame::new(origin.to_array(), normal.to_array(), Some(r.x_axis.to_array())) {
            self.frame = Some(f);
            self.offset = 0.0;
        }
    }

    // ---- builders -------------------------------------------------------------------------
    pub fn add_point(&mut self, x: f64, y: f64) -> usize {
        self.points.push(SPoint::new(x, y));
        self.points.len() - 1
    }
    pub fn add_fixed_point(&mut self, x: f64, y: f64) -> usize {
        self.points.push(SPoint { x, y, fixed: true });
        self.points.len() - 1
    }
    pub fn push(&mut self, e: Entity) -> usize {
        self.entities.push(e);
        self.entities.len() - 1
    }
    /// Line between existing points.
    pub fn add_line(&mut self, a: usize, b: usize) -> usize {
        self.push(Entity::Line { a, b, construction: false })
    }
    /// Line with two new end points.
    pub fn add_line_xy(&mut self, a: [f64; 2], b: [f64; 2]) -> usize {
        let pa = self.add_point(a[0], a[1]);
        let pb = self.add_point(b[0], b[1]);
        self.add_line(pa, pb)
    }
    /// Circle with a new centre point.
    pub fn add_circle(&mut self, cx: f64, cy: f64, radius: f64) -> usize {
        let c = self.add_point(cx, cy);
        self.push(Entity::Circle { center: c, radius, construction: false })
    }
    pub fn add_arc(&mut self, center: usize, start: usize, end: usize) -> usize {
        self.push(Entity::Arc { center, start, end, construction: false })
    }
    pub fn add_spline(&mut self, ctrl: Vec<usize>, closed: bool) -> usize {
        self.push(Entity::Spline { ctrl, closed, construction: false })
    }
    pub fn add_polyline(&mut self, pl: &Polyline) -> usize {
        self.push(Entity::Polyline { pts: pl.pts.iter().map(|p| p.to_array()).collect(), closed: pl.closed, construction: false })
    }
    pub fn add_construction_line(&mut self, a: usize, b: usize) -> usize {
        self.push(Entity::Line { a, b, construction: true })
    }
    pub fn constrain(&mut self, c: Constraint) -> usize {
        self.constraints.push(c);
        self.constraints.len() - 1
    }

    /// Centre rectangle `w × h` around the origin: 4 lines sharing corner points, horizontal /
    /// vertical relations, width/height dimensions (constraints 4 and 5) and a construction diagonal
    /// whose midpoint is the fixed origin — fully defined.
    pub fn rectangle(w: f64, h: f64) -> Sketch {
        let mut s = Sketch::default();
        let p: Vec<usize> = [(-w, -h), (w, -h), (w, h), (-w, h)].iter().map(|(x, y)| s.add_point(x * 0.5, y * 0.5)).collect();
        let c = s.add_fixed_point(0.0, 0.0);
        let l: Vec<usize> = (0..4).map(|i| s.add_line(p[i], p[(i + 1) % 4])).collect();
        let diag = s.add_construction_line(p[0], p[2]);
        s.constrain(Constraint::Horizontal(l[0]));
        s.constrain(Constraint::Vertical(l[1]));
        s.constrain(Constraint::Horizontal(l[2]));
        s.constrain(Constraint::Vertical(l[3]));
        s.constrain(Constraint::Length(l[0], Dim::new(w)));
        s.constrain(Constraint::Length(l[1], Dim::new(h)));
        s.constrain(Constraint::Midpoint(c, diag));
        s
    }
    /// Circle of `radius` centred on the fixed origin, with a radius dimension (constraint 0).
    pub fn circle(radius: f64) -> Sketch {
        let mut s = Sketch::default();
        let c = s.add_fixed_point(0.0, 0.0);
        let e = s.push(Entity::Circle { center: c, radius, construction: false });
        s.constrain(Constraint::Radius(e, Dim::new(radius)));
        s
    }
    /// Closed polygon of lines through `pts` (shared corners, no constraints).
    pub fn polygon(pts: &[[f64; 2]]) -> Sketch {
        let mut s = Sketch::default();
        let ids: Vec<usize> = pts.iter().map(|p| s.add_point(p[0], p[1])).collect();
        for i in 0..ids.len() {
            s.add_line(ids[i], ids[(i + 1) % ids.len()]);
        }
        s
    }
    /// Wrap fixed polylines (e.g. from the 2D design module or text) as a sketch.
    pub fn from_polylines(pls: &[Polyline]) -> Sketch {
        let mut s = Sketch::default();
        for p in pls {
            s.add_polyline(p);
        }
        s
    }

    pub fn point(&self, i: usize) -> DVec2 {
        self.points.get(i).map(|p| p.pos()).unwrap_or_default()
    }
    /// Set the value of dimension constraint `c` (clears its expression).
    pub fn set_dim(&mut self, c: usize, value: f64) {
        if let Some(d) = self.constraints.get_mut(c).and_then(|c| c.dim_mut()) {
            d.value = value;
            d.expr = None;
        }
    }

    // ---- solving --------------------------------------------------------------------------
    /// Evaluate every dimension expression with `vars`.
    pub fn apply_dim_exprs(&mut self, vars: &dyn Fn(&str) -> Option<f64>) -> Result<(), ExprError> {
        for c in &mut self.constraints {
            if let Some(d) = c.dim_mut() {
                if let Some(e) = &d.expr {
                    d.value = expr::eval(e, vars)?;
                }
            }
        }
        Ok(())
    }

    /// Evaluate parameter-driven dimensions, then solve.
    pub fn solve_with(&mut self, vars: &BTreeMap<String, f64>) -> Result<SolveReport, ExprError> {
        self.apply_dim_exprs(&|n| vars.get(n).copied())?;
        Ok(self.solve())
    }

    /// Solve the constraints in place (Levenberg–Marquardt). Free geometry stays near where it was.
    pub fn solve(&mut self) -> SolveReport {
        let (ctx, mut x) = Ctx::new(self);
        let mut r = Vec::new();
        let ignored = ctx.residuals(&x, &mut r);
        let (m, n) = (r.len(), x.len());
        let maxabs = |r: &[f64]| r.iter().fold(0.0f64, |a, v| a.max(v.abs()));
        let cost = |r: &[f64]| r.iter().map(|v| v * v).sum::<f64>();
        let mut iterations = 0;
        let mut lambda = 1e-3;
        let mut c0 = cost(&r);
        let mut rn = Vec::with_capacity(m);
        while iterations < 500 && maxabs(&r) > TOL && n > 0 {
            iterations += 1;
            let j = ctx.jacobian(&x, m);
            // Normal equations: (JᵀJ + λI) δ = −Jᵀr
            let mut jtj = vec![0.0; n * n];
            let mut g = vec![0.0; n];
            for row in 0..m {
                let jr = &j[row * n..(row + 1) * n];
                for a in 0..n {
                    if jr[a] == 0.0 {
                        continue;
                    }
                    g[a] += jr[a] * r[row];
                    for b in 0..n {
                        jtj[a * n + b] += jr[a] * jr[b];
                    }
                }
            }
            let mut accepted = false;
            while lambda < 1e12 {
                let mut a = jtj.clone();
                for k in 0..n {
                    a[k * n + k] += lambda;
                }
                let mut d: Vec<f64> = g.iter().map(|v| -v).collect();
                if solve_dense(&mut a, &mut d, n) {
                    let xn: Vec<f64> = x.iter().zip(&d).map(|(a, b)| a + b).collect();
                    ctx.residuals(&xn, &mut rn);
                    let c1 = cost(&rn);
                    if c1 < c0 {
                        x = xn;
                        std::mem::swap(&mut r, &mut rn);
                        c0 = c1;
                        lambda = (lambda * 0.2).max(1e-10);
                        accepted = true;
                        break;
                    }
                }
                lambda *= 8.0;
            }
            if !accepted {
                break; // stalled at a least-squares minimum (conflicting constraints)
            }
        }
        let max_residual = maxabs(&r);
        let converged = max_residual <= TOL * 10.0;
        let rank = if n > 0 && m > 0 { matrix_rank(&mut ctx.jacobian(&x, m), m, n) } else { 0 };
        let dof = n - rank;
        let status = if !converged {
            if rank < m {
                SolveStatus::OverDefined
            } else {
                SolveStatus::Failed
            }
        } else if rank < m {
            SolveStatus::OverDefined
        } else if dof > 0 {
            SolveStatus::UnderDefined(dof)
        } else {
            SolveStatus::FullyDefined
        };
        let (pvar, rvar) = (ctx.pvar, ctx.rvar);
        write_back(&pvar, &rvar, &x, self);
        SolveReport { converged, max_residual, iterations, dof, status, ignored }
    }

    // ---- profiles -------------------------------------------------------------------------
    /// Closed loops for features (even-odd: inner loops are holes).
    pub fn profiles(&self) -> Vec<Polyline> {
        self.chains().0
    }
    /// Open chains (e.g. sweep paths, engraving).
    /// The flattened outline of every drawing (non-construction) entity, in entity order: what
    /// the kernels use to tell the side faces of an extrusion apart (one face per entity).
    pub fn entity_paths(&self) -> Vec<Polyline> {
        self.entities
            .iter()
            .filter(|e| !e.construction())
            .map(|e| match e {
                Entity::Line { a, b, .. } => Polyline::open(vec![self.point(*a), self.point(*b)]),
                Entity::Circle { center, radius, .. } => shapes::circle(self.point(*center), *radius),
                Entity::Arc { center, start, end, .. } => Polyline::open(arc_points(self.point(*center), self.point(*start), self.point(*end))),
                Entity::Spline { ctrl, closed, .. } => {
                    let p: Vec<DVec2> = ctrl.iter().map(|i| self.point(*i)).collect();
                    Polyline::new(catmull_rom(&p, *closed), *closed)
                }
                Entity::Polyline { pts, closed, .. } => Polyline::new(pts.iter().map(|p| DVec2::new(p[0], p[1])).collect(), *closed),
            })
            .collect()
    }

    pub fn open_paths(&self) -> Vec<Polyline> {
        self.chains().1
    }

    fn chains(&self) -> (Vec<Polyline>, Vec<Polyline>) {
        let mut closed = Vec::new();
        let mut open = Vec::new();
        // Node identity: union of coincident constraints and points at the same position.
        let np = self.points.len();
        let mut uf: Vec<usize> = (0..np).collect();
        fn find(uf: &mut [usize], mut i: usize) -> usize {
            while uf[i] != i {
                uf[i] = uf[uf[i]];
                i = uf[i];
            }
            i
        }
        let join = |uf: &mut Vec<usize>, a: usize, b: usize| {
            let (ra, rb) = (find(uf, a), find(uf, b));
            uf[ra] = rb;
        };
        for c in &self.constraints {
            if let Constraint::Coincident(a, b) = c {
                if *a < np && *b < np {
                    join(&mut uf, *a, *b);
                }
            }
        }
        for a in 0..np {
            for b in a + 1..np {
                if self.point(a).distance(self.point(b)) < 1e-7 {
                    join(&mut uf, a, b);
                }
            }
        }
        // Chainable edges: (start node, end node, points).
        let mut edges: Vec<(usize, usize, Vec<DVec2>)> = Vec::new();
        for e in self.entities.iter().filter(|e| !e.construction()) {
            match e {
                Entity::Line { a, b, .. } if *a < np && *b < np => edges.push((*a, *b, vec![self.point(*a), self.point(*b)])),
                Entity::Arc { center, start, end, .. } if *center < np && *start < np && *end < np => {
                    edges.push((*start, *end, arc_points(self.point(*center), self.point(*start), self.point(*end))))
                }
                Entity::Circle { center, radius, .. } if *center < np => closed.push(shapes::circle(self.point(*center), radius.abs())),
                Entity::Spline { ctrl, closed: c, .. } if ctrl.iter().all(|i| *i < np) && ctrl.len() >= 2 => {
                    let pts: Vec<DVec2> = ctrl.iter().map(|i| self.point(*i)).collect();
                    if *c {
                        closed.push(Polyline::closed(catmull_rom(&pts, true)));
                    } else {
                        edges.push((ctrl[0], *ctrl.last().unwrap(), catmull_rom(&pts, false)));
                    }
                }
                Entity::Polyline { pts, closed: c, .. } => {
                    let pl = Polyline::new(pts.iter().map(|p| DVec2::from_array(*p)).collect(), *c);
                    if *c {
                        closed.push(pl);
                    } else {
                        open.push(pl);
                    }
                }
                _ => {}
            }
        }
        let edges: Vec<(usize, usize, Vec<DVec2>)> = edges.into_iter().map(|(a, b, p)| (find(&mut uf, a), find(&mut uf, b), p)).collect();
        let mut used = vec![false; edges.len()];
        for s in 0..edges.len() {
            if used[s] {
                continue;
            }
            used[s] = true;
            let (start, mut end, first) = edges[s].clone();
            let mut pts = first;
            let mut is_closed = start == end && pts.len() > 2;
            // Extend forward.
            while !is_closed {
                let Some(k) = (0..edges.len()).find(|&k| !used[k] && (edges[k].0 == end || edges[k].1 == end)) else { break };
                used[k] = true;
                let (a, b, p) = &edges[k];
                let (next, seg): (usize, Vec<DVec2>) = if *a == end { (*b, p.clone()) } else { (*a, p.iter().rev().copied().collect()) };
                pts.extend_from_slice(&seg[1..]);
                end = next;
                is_closed = end == start;
            }
            if is_closed {
                pts.pop(); // last point duplicates the first
                closed.push(Polyline::closed(pts));
                continue;
            }
            // Extend backward from the start for open chains.
            let mut head = start;
            while let Some(k) = (0..edges.len()).find(|&k| !used[k] && (edges[k].0 == head || edges[k].1 == head)) {
                used[k] = true;
                let (a, b, p) = &edges[k];
                let (prev, seg): (usize, Vec<DVec2>) = if *b == head { (*a, p.clone()) } else { (*b, p.iter().rev().copied().collect()) };
                let mut np2 = seg;
                np2.pop();
                np2.extend_from_slice(&pts);
                pts = np2;
                head = prev;
            }
            open.push(Polyline::open(pts));
        }
        (closed, open)
    }
}

/// Tessellate a CCW arc (end point kept exact so chains close).
fn arc_points(c: DVec2, s: DVec2, e: DVec2) -> Vec<DVec2> {
    let r = (s - c).length();
    let a0 = (s - c).y.atan2((s - c).x);
    let a1 = (e - c).y.atan2((e - c).x);
    let mut sweep = (a1 - a0).rem_euclid(TAU);
    if sweep < 1e-9 {
        sweep = TAU;
    }
    let n = arc_segments(r, sweep, DEFAULT_TOLERANCE);
    let mut pts: Vec<DVec2> = (0..n).map(|i| c + DVec2::from_angle(a0 + sweep * i as f64 / n as f64) * r).collect();
    pts.push(e);
    pts
}

/// Uniform Catmull-Rom through `p` (endpoints interpolated; open curves start/end on p[0]/p[n-1]).
fn catmull_rom(p: &[DVec2], closed: bool) -> Vec<DVec2> {
    let n = p.len();
    let get = |i: isize| -> DVec2 {
        if closed {
            p[i.rem_euclid(n as isize) as usize]
        } else if i < 0 {
            p[0] * 2.0 - p[1]
        } else if i as usize >= n {
            p[n - 1] * 2.0 - p[n - 2]
        } else {
            p[i as usize]
        }
    };
    let segs = if closed { n } else { n - 1 };
    let mut out = Vec::new();
    for i in 0..segs as isize {
        let (p0, p1, p2, p3) = (get(i - 1), get(i), get(i + 1), get(i + 2));
        let k = ((p1.distance(p2) / 0.5).ceil() as usize).clamp(8, 64);
        for j in 0..k {
            let t = j as f64 / k as f64;
            let (t2, t3) = (t * t, t * t * t);
            out.push(0.5 * (2.0 * p1 + (p2 - p0) * t + (2.0 * p0 - 5.0 * p1 + 4.0 * p2 - p3) * t2 + (3.0 * p1 - p0 - 3.0 * p2 + p3) * t3));
        }
    }
    if !closed {
        out.push(p[n - 1]);
    }
    out
}

/// Maps sketch geometry to the solver's unknown vector.
struct Ctx<'a> {
    sk: &'a Sketch,
    pvar: Vec<Option<usize>>,
    rvar: Vec<Option<usize>>,
    lref: f64,
}

impl<'a> Ctx<'a> {
    fn new(sk: &'a Sketch) -> (Self, Vec<f64>) {
        let mut fixed: Vec<bool> = sk.points.iter().map(|p| p.fixed).collect();
        for c in &sk.constraints {
            if let Constraint::Fix(p) = c {
                if let Some(f) = fixed.get_mut(*p) {
                    *f = true;
                }
            }
        }
        let mut x = Vec::new();
        let pvar = sk
            .points
            .iter()
            .zip(&fixed)
            .map(|(p, f)| {
                (!f).then(|| {
                    x.push(p.x);
                    x.push(p.y);
                    x.len() - 2
                })
            })
            .collect();
        let rvar = sk
            .entities
            .iter()
            .map(|e| match e {
                Entity::Circle { radius, .. } => {
                    x.push(*radius);
                    Some(x.len() - 1)
                }
                _ => None,
            })
            .collect();
        let b = crate::path::Bounds2::from_points(sk.points.iter().map(|p| p.pos()).collect::<Vec<_>>().iter());
        let lref = if b.is_empty() { 1.0 } else { (b.size().length() * 0.5).max(1.0) };
        (Self { sk, pvar, rvar, lref }, x)
    }
    fn p(&self, x: &[f64], i: usize) -> Option<DVec2> {
        let sp = self.sk.points.get(i)?;
        Some(match self.pvar[i] {
            Some(k) => DVec2::new(x[k], x[k + 1]),
            None => sp.pos(),
        })
    }
    fn line(&self, x: &[f64], e: usize) -> Option<(DVec2, DVec2)> {
        match self.sk.entities.get(e)? {
            Entity::Line { a, b, .. } => Some((self.p(x, *a)?, self.p(x, *b)?)),
            _ => None,
        }
    }
    /// Centre and radius of a circle or arc.
    fn circ(&self, x: &[f64], e: usize) -> Option<(DVec2, f64)> {
        match self.sk.entities.get(e)? {
            Entity::Circle { center, radius, .. } => Some((self.p(x, *center)?, self.rvar[e].map(|k| x[k]).unwrap_or(*radius))),
            Entity::Arc { center, start, .. } => {
                let c = self.p(x, *center)?;
                Some((c, (self.p(x, *start)? - c).length()))
            }
            _ => None,
        }
    }
    fn size(&self, x: &[f64], e: usize) -> Option<f64> {
        self.line(x, e).map(|(a, b)| a.distance(b)).or_else(|| self.circ(x, e).map(|c| c.1))
    }

    /// Append residuals; returns the number of ignored (invalid) constraints. The residual count
    /// depends only on topology, never on `x`.
    fn residuals(&self, x: &[f64], out: &mut Vec<f64>) -> usize {
        out.clear();
        let mut ignored = 0;
        let l = self.lref;
        let dir = |a: DVec2, b: DVec2| (b - a).normalize_or_zero();
        for c_kind in &self.sk.constraints {
            let ok = (|| -> Option<()> {
                match c_kind {
                    Constraint::Coincident(p, q) => {
                        let d = self.p(x, *p)? - self.p(x, *q)?;
                        out.extend([d.x, d.y]);
                    }
                    Constraint::Horizontal(e) => {
                        let (a, b) = self.line(x, *e)?;
                        out.push(a.y - b.y);
                    }
                    Constraint::Vertical(e) => {
                        let (a, b) = self.line(x, *e)?;
                        out.push(a.x - b.x);
                    }
                    Constraint::Distance(p, q, d) => out.push(self.p(x, *p)?.distance(self.p(x, *q)?) - d.value),
                    Constraint::HorizontalDistance(p, q, d) => out.push((self.p(x, *q)?.x - self.p(x, *p)?.x).abs() - d.value),
                    Constraint::VerticalDistance(p, q, d) => out.push((self.p(x, *q)?.y - self.p(x, *p)?.y).abs() - d.value),
                    Constraint::Length(e, d) => {
                        let (a, b) = self.line(x, *e)?;
                        out.push(a.distance(b) - d.value);
                    }
                    Constraint::Radius(e, d) => out.push(self.circ(x, *e)?.1 - d.value),
                    Constraint::Diameter(e, d) => out.push(2.0 * self.circ(x, *e)?.1 - d.value),
                    Constraint::Angle(e1, e2, d) => {
                        let (a, b) = self.line(x, *e1)?;
                        let (c, dd) = self.line(x, *e2)?;
                        let (u, v) = (dir(a, b), dir(c, dd));
                        out.push((u.perp_dot(v).abs().atan2(u.dot(v)) - d.value.to_radians()) * l);
                    }
                    Constraint::Parallel(e1, e2) | Constraint::Perpendicular(e1, e2) => {
                        let (a, b) = self.line(x, *e1)?;
                        let (c, d) = self.line(x, *e2)?;
                        let (u, v) = (dir(a, b), dir(c, d));
                        let parallel = matches!(c_kind, Constraint::Parallel(..));
                        out.push(if parallel { u.perp_dot(v) } else { u.dot(v) } * l);
                    }
                    Constraint::Equal(e1, e2) => out.push(self.size(x, *e1)? - self.size(x, *e2)?),
                    Constraint::Tangent(e1, e2) => {
                        if let (Some((a, b)), Some((c, r))) = (self.line(x, *e1), self.circ(x, *e2)) {
                            out.push(dir(a, b).perp_dot(c - a).abs() - r.abs());
                        } else if let (Some((c, r)), Some((a, b))) = (self.circ(x, *e1), self.line(x, *e2)) {
                            out.push(dir(a, b).perp_dot(c - a).abs() - r.abs());
                        } else {
                            let ((c1, r1), (c2, r2)) = (self.circ(x, *e1)?, self.circ(x, *e2)?);
                            out.push(c1.distance(c2) - (r1.abs() + r2.abs()));
                        }
                    }
                    Constraint::Concentric(e1, e2) => {
                        let d = self.circ(x, *e1)?.0 - self.circ(x, *e2)?.0;
                        out.extend([d.x, d.y]);
                    }
                    Constraint::PointOnLine(p, e) => {
                        let (a, b) = self.line(x, *e)?;
                        out.push(dir(a, b).perp_dot(self.p(x, *p)? - a));
                    }
                    Constraint::PointOnCircle(p, e) => {
                        let (c, r) = self.circ(x, *e)?;
                        out.push(self.p(x, *p)?.distance(c) - r.abs());
                    }
                    Constraint::Midpoint(p, e) => {
                        let (a, b) = self.line(x, *e)?;
                        let d = self.p(x, *p)? - (a + b) * 0.5;
                        out.extend([d.x, d.y]);
                    }
                    Constraint::PointLineDistance(p, e, d) => {
                        let (a, b) = self.line(x, *e)?;
                        out.push(dir(a, b).perp_dot(self.p(x, *p)? - a).abs() - d.value);
                    }
                    Constraint::Symmetric(p, q, e) => {
                        let (a, b) = self.line(x, *e)?;
                        let (pp, qq) = (self.p(x, *p)?, self.p(x, *q)?);
                        let u = dir(a, b);
                        out.extend([u.perp_dot((pp + qq) * 0.5 - a), u.dot(qq - pp)]);
                    }
                    Constraint::Fix(p) => {
                        self.sk.points.get(*p)?; // handled by removing the unknowns
                    }
                }
                Some(())
            })();
            if ok.is_none() {
                ignored += 1;
            }
        }
        // Implicit: arcs keep |end − centre| = |start − centre|.
        for e in &self.sk.entities {
            if let Entity::Arc { center, start, end, .. } = e {
                if let (Some(c), Some(s), Some(t)) = (self.p(x, *center), self.p(x, *start), self.p(x, *end)) {
                    out.push(t.distance(c) - s.distance(c));
                }
            }
        }
        ignored
    }

    fn jacobian(&self, x: &[f64], m: usize) -> Vec<f64> {
        let n = x.len();
        let mut j = vec![0.0; m * n];
        let mut xp = x.to_vec();
        let (mut rp, mut rm) = (Vec::with_capacity(m), Vec::with_capacity(m));
        for k in 0..n {
            let h = 1e-6 * x[k].abs().max(1.0);
            xp[k] = x[k] + h;
            self.residuals(&xp, &mut rp);
            xp[k] = x[k] - h;
            self.residuals(&xp, &mut rm);
            xp[k] = x[k];
            for row in 0..m {
                j[row * n + k] = (rp[row] - rm[row]) / (2.0 * h);
            }
        }
        j
    }
}

fn write_back(pvar: &[Option<usize>], rvar: &[Option<usize>], x: &[f64], sk: &mut Sketch) {
    for (i, v) in pvar.iter().enumerate() {
        if let Some(k) = v {
            sk.points[i].x = x[*k];
            sk.points[i].y = x[*k + 1];
        }
    }
    for (e, v) in rvar.iter().enumerate() {
        if let (Some(k), Entity::Circle { radius, .. }) = (v, &mut sk.entities[e]) {
            *radius = x[*k].abs();
        }
    }
}

/// Solve a dense n×n system in place (Gaussian elimination, partial pivoting). `false` if singular.
pub(crate) fn solve_dense(a: &mut [f64], b: &mut [f64], n: usize) -> bool {
    for col in 0..n {
        let piv = (col..n).max_by(|&i, &j| a[i * n + col].abs().total_cmp(&a[j * n + col].abs())).unwrap();
        if a[piv * n + col].abs() < 1e-300 {
            return false;
        }
        if piv != col {
            for k in 0..n {
                a.swap(piv * n + k, col * n + k);
            }
            b.swap(piv, col);
        }
        let d = a[col * n + col];
        for row in col + 1..n {
            let f = a[row * n + col] / d;
            if f == 0.0 {
                continue;
            }
            for k in col..n {
                a[row * n + k] -= f * a[col * n + k];
            }
            b[row] -= f * b[col];
        }
    }
    for col in (0..n).rev() {
        let mut s = b[col];
        for k in col + 1..n {
            s -= a[col * n + k] * b[k];
        }
        b[col] = s / a[col * n + col];
    }
    b.iter().all(|v| v.is_finite())
}

/// Numerical rank of an m×n row-major matrix (destroys it).
pub(crate) fn matrix_rank(a: &mut [f64], m: usize, n: usize) -> usize {
    let tol = 1e-7 * a.iter().fold(1.0f64, |acc, v| acc.max(v.abs()));
    let mut rank = 0;
    for col in 0..n {
        if rank == m {
            break;
        }
        let piv = (rank..m).max_by(|&i, &j| a[i * n + col].abs().total_cmp(&a[j * n + col].abs())).unwrap();
        if a[piv * n + col].abs() <= tol {
            continue;
        }
        for k in 0..n {
            a.swap(piv * n + k, rank * n + k);
        }
        for row in rank + 1..m {
            let f = a[row * n + col] / a[rank * n + col];
            for k in col..n {
                a[row * n + k] -= f * a[rank * n + k];
            }
        }
        rank += 1;
    }
    rank
}

#[cfg(test)]
mod tests {
    use super::*;

    fn area(pls: &[Polyline]) -> f64 {
        pls.iter().map(|p| p.signed_area().abs()).sum()
    }

    fn jiggle(s: &mut Sketch) {
        for (i, p) in s.points.iter_mut().enumerate().filter(|(_, p)| !p.fixed) {
            p.x += 0.7 * ((i * 7 % 5) as f64 - 2.0);
            p.y -= 0.4 * ((i * 3 % 4) as f64 - 1.5);
        }
    }

    #[test]
    fn rectangle_solves_to_exact_size() {
        let mut s = Sketch::rectangle(40.0, 25.0);
        jiggle(&mut s);
        let r = s.solve();
        assert!(r.converged, "{r:?}");
        assert_eq!(r.status, SolveStatus::FullyDefined, "{r:?}");
        assert_eq!(r.dof, 0);
        let prof = s.profiles();
        assert_eq!(prof.len(), 1, "construction diagonal must not split the loop");
        assert!((area(&prof) - 1000.0).abs() < 1e-6, "{}", area(&prof));
        // Centred on the fixed origin.
        assert!(s.point(0).distance(DVec2::new(-20.0, -12.5)) < 1e-6);
        // Change a dimension and re-solve.
        s.set_dim(4, 60.0);
        let r = s.solve();
        assert!(r.converged && r.status == SolveStatus::FullyDefined);
        assert!((area(&s.profiles()) - 1500.0).abs() < 1e-6);
        assert!(s.open_paths().is_empty());
    }

    #[test]
    fn triangle_length_angle_matches_trig() {
        let mut s = Sketch::default();
        let a = s.add_fixed_point(0.0, 0.0);
        let b = s.add_point(25.0, 3.0);
        let c = s.add_point(10.0, 15.0);
        let ab = s.add_line(a, b);
        s.add_line(b, c);
        let ac = s.add_line(a, c);
        s.constrain(Constraint::Horizontal(ab));
        s.constrain(Constraint::Length(ab, 30.0.into()));
        s.constrain(Constraint::Length(ac, 20.0.into()));
        s.constrain(Constraint::Angle(ab, ac, 40.0.into()));
        let r = s.solve();
        assert!(r.converged && r.status == SolveStatus::FullyDefined, "{r:?}");
        let t = 40f64.to_radians();
        assert!(s.point(c).distance(DVec2::new(20.0 * t.cos(), 20.0 * t.sin())) < 1e-7, "{:?}", s.point(c));
        let bc = (30f64.powi(2) + 20f64.powi(2) - 2.0 * 30.0 * 20.0 * t.cos()).sqrt();
        assert!((s.point(b).distance(s.point(c)) - bc).abs() < 1e-7);
        let tri = s.profiles();
        assert_eq!(tri.len(), 1);
        assert!((area(&tri) - 0.5 * 30.0 * 20.0 * t.sin()).abs() < 1e-6);
    }

    #[test]
    fn under_and_over_defined() {
        let mut s = Sketch::default();
        let l = s.add_line_xy([0.0, 0.0], [10.0, 1.0]);
        s.constrain(Constraint::Horizontal(l));
        let r = s.solve();
        assert!(r.converged);
        assert_eq!(r.status, SolveStatus::UnderDefined(3));
        assert_eq!(r.dof, 3);
        assert!((s.point(0).y - s.point(1).y).abs() < 1e-9);

        // Conflicting: a diagonal shorter than the width.
        let mut s = Sketch::rectangle(10.0, 5.0);
        s.constrain(Constraint::Distance(0, 2, 5.0.into()));
        let r = s.solve();
        assert!(!r.converged);
        assert_eq!(r.status, SolveStatus::OverDefined);

        // Redundant but consistent.
        let mut s = Sketch::rectangle(10.0, 5.0);
        s.constrain(Constraint::Parallel(0, 2));
        let r = s.solve();
        assert!(r.converged);
        assert_eq!(r.status, SolveStatus::OverDefined);
    }

    #[test]
    fn parameter_driven_dimension() {
        let mut s = Sketch::rectangle(10.0, 10.0);
        s.constraints[4] = Constraint::Length(0, Dim::expr("w * 2"));
        s.constraints[5] = Constraint::Length(1, Dim::expr("w + 1cm"));
        let vars: BTreeMap<String, f64> = [("w".to_string(), 15.0)].into();
        let r = s.solve_with(&vars).unwrap();
        assert!(r.converged && r.status == SolveStatus::FullyDefined);
        assert!((area(&s.profiles()) - 30.0 * 25.0).abs() < 1e-6);
        assert!(s.solve_with(&BTreeMap::new()).is_err());
    }

    #[test]
    fn circles_arcs_tangent_and_concentric() {
        let mut s = Sketch::default();
        let c = s.add_circle(1.0, 2.0, 3.0);
        let center = 0;
        s.constrain(Constraint::Fix(center));
        s.constrain(Constraint::Diameter(c, 20.0.into()));
        let c2 = s.add_circle(0.0, 0.0, 2.0);
        s.constrain(Constraint::Concentric(c, c2));
        s.constrain(Constraint::Radius(c2, 4.0.into()));
        // Horizontal line tangent to the big circle (above it).
        let l = s.add_line_xy([-5.0, 13.0], [5.0, 13.5]);
        s.constrain(Constraint::Horizontal(l));
        s.constrain(Constraint::Tangent(l, c));
        let r = s.solve();
        assert!(r.converged, "{r:?}");
        let Entity::Circle { radius, .. } = s.entities[c] else { panic!() };
        assert!((radius - 10.0).abs() < 1e-9);
        assert!(s.point(1).distance(DVec2::new(1.0, 2.0)) < 1e-9, "concentric centre");
        assert!((s.point(2).y - 12.0).abs() < 1e-7 && (s.point(3).y - 12.0).abs() < 1e-7, "tangent line at y = 2 + 10, got {}", s.point(3).y);
        assert_eq!(r.status, SolveStatus::UnderDefined(2), "line can slide along x and stretch");
        // Two loops (circles); the open line is an open path.
        assert_eq!(s.profiles().len(), 2);
        assert_eq!(s.open_paths().len(), 1);
    }

    #[test]
    fn slot_from_lines_and_arcs() {
        // Obround slot: two lines + two semicircular arcs, chained by shared points.
        let mut s = Sketch::default();
        let p = [s.add_point(0.0, -5.0), s.add_point(20.0, -5.0), s.add_point(20.0, 5.0), s.add_point(0.0, 5.0)];
        let c1 = s.add_point(20.0, 0.0);
        let c0 = s.add_point(0.0, 0.0);
        let l0 = s.add_line(p[0], p[1]);
        s.add_arc(c1, p[1], p[2]);
        let l1 = s.add_line(p[2], p[3]);
        s.add_arc(c0, p[3], p[0]);
        s.constrain(Constraint::Fix(c0));
        s.constrain(Constraint::Horizontal(l0));
        s.constrain(Constraint::Parallel(l0, l1));
        s.constrain(Constraint::Midpoint(c1, s.entities.len() + 99)); // invalid → ignored
        let r = s.solve();
        assert_eq!(r.ignored, 1);
        let prof = s.profiles();
        assert_eq!(prof.len(), 1);
        let expect = 20.0 * 10.0 + std::f64::consts::PI * 25.0;
        assert!((area(&prof) - expect).abs() / expect < 1e-3, "{}", area(&prof));
    }

    #[test]
    fn symmetric_point_on_and_equal() {
        let mut s = Sketch::default();
        let axis = s.add_line_xy([0.0, -10.0], [0.0, 10.0]);
        s.constrain(Constraint::Fix(0));
        s.constrain(Constraint::Fix(1));
        let a = s.add_point(3.0, 1.0);
        let b = s.add_point(-5.0, 2.0);
        s.constrain(Constraint::Symmetric(a, b, axis));
        s.constrain(Constraint::HorizontalDistance(a, b, 8.0.into()));
        s.constrain(Constraint::VerticalDistance(0, a, 12.0.into()));
        let r = s.solve();
        assert!(r.converged && r.status == SolveStatus::FullyDefined, "{r:?}");
        assert!(s.point(a).distance(DVec2::new(4.0, 2.0)) < 1e-7, "{:?}", s.point(a));
        assert!(s.point(b).distance(DVec2::new(-4.0, 2.0)) < 1e-7);

        let mut s = Sketch::default();
        let l0 = s.add_line_xy([0.0, 0.0], [10.0, 0.0]);
        let l1 = s.add_line_xy([0.0, 5.0], [3.0, 9.0]);
        s.constrain(Constraint::Equal(l0, l1));
        s.constrain(Constraint::Perpendicular(l0, l1));
        let p = s.add_point(4.0, 1.0);
        s.constrain(Constraint::PointOnLine(p, l0));
        let r = s.solve();
        assert!(r.converged);
        let (a, b) = (s.point(2), s.point(3));
        assert!(((b - a).length() - s.point(0).distance(s.point(1))).abs() < 1e-8);
        assert!((b - a).dot(s.point(1) - s.point(0)).abs() < 1e-6);
        assert!((s.point(1) - s.point(0)).perp_dot(s.point(p) - s.point(0)).abs() < 1e-6);
    }

    #[test]
    fn spline_and_polyline_profiles() {
        let mut s = Sketch::default();
        let ids: Vec<usize> = [[10.0, 0.0], [0.0, 10.0], [-10.0, 0.0], [0.0, -10.0]].iter().map(|p| s.add_point(p[0], p[1])).collect();
        s.add_spline(ids, true);
        s.add_polyline(&crate::path::shapes::rectangle(DVec2::new(30.0, 0.0), 4.0, 2.0, 0.0));
        let prof = s.profiles();
        assert_eq!(prof.len(), 2);
        // Catmull-Rom through a diamond bulges towards a circle: between the diamond and r=10 disc.
        let a = prof[0].signed_area().abs();
        assert!(a > 200.0 && a < 314.2, "{a}");
        assert!((prof[1].signed_area().abs() - 8.0).abs() < 1e-9);
    }

    #[test]
    fn sketch_planes() {
        let p = SketchPlane::YZ;
        assert!(p.to_world(5.0, DVec2::new(1.0, 2.0)).distance(DVec3::new(5.0, 1.0, 2.0)) < 1e-12);
        assert!(SketchPlane::XZ.to_world(0.0, DVec2::new(1.0, 2.0)).distance(DVec3::new(1.0, 0.0, 2.0)) < 1e-12);
        // The Placement form reproduces the same rotation.
        for pl in [SketchPlane::XY, SketchPlane::XZ, SketchPlane::YZ] {
            let pm = pl.placement(3.0);
            let r =
                DMat3::from_quat(DQuat::from_euler(EulerRot::XYZ, pm.rotate_deg[0].to_radians(), pm.rotate_deg[1].to_radians(), pm.rotate_deg[2].to_radians()));
            let w = r * DVec3::new(1.0, 2.0, 0.0) + DVec3::from_array(pm.translate);
            assert!(w.distance(pl.to_world(3.0, DVec2::new(1.0, 2.0))) < 1e-9, "{pl:?}");
        }
        let json = serde_json::to_string(&Sketch::rectangle(2.0, 3.0)).unwrap();
        let back: Sketch = serde_json::from_str(&json).unwrap();
        assert_eq!(back, Sketch::rectangle(2.0, 3.0));
    }
}
