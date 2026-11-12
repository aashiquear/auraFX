//! The CAD document: feature timeline, parameters with unit-aware expressions, bindings and
//! configurations (ported from EZ-YX-craft's `Document`, MIT OR Apache-2.0), with stable ids on
//! every feature so the downstream check and the B-rep mirror can follow an object across
//! edits. The document is plain serde data and never depends on a kernel.

use crate::path::Polyline;
use crate::sketch::Sketch;
use crate::Id;
use glam::DVec2;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum BooleanOp {
    #[default]
    Add,
    Cut,
    Intersect,
}

/// Placement of a feature: translation (mm) and rotation (XYZ Euler, degrees).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct Placement {
    pub translate: [f64; 3],
    pub rotate_deg: [f64; 3],
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind")]
pub enum Shape {
    Box {
        size: [f64; 3],
        corner_radius: f64,
    },
    Sphere {
        radius: f64,
    },
    Cylinder {
        radius: f64,
        height: f64,
    },
    Cone {
        r_bottom: f64,
        r_top: f64,
        height: f64,
    },
    Torus {
        major: f64,
        minor: f64,
    },
    /// Extrude closed sketch loops (even-odd) along +Z, with optional draft (degrees).
    Extrude {
        sketch: Vec<Polyline>,
        height: f64,
        draft_deg: f64,
    },
    /// Revolve a sketch drawn in the XZ half-plane (x ≥ 0 = radius) around Z.
    Revolve {
        profile: Vec<Polyline>,
        angle_deg: f64,
    },
    /// Extrude the closed profiles of a constrained sketch along its plane normal.
    SketchExtrude {
        sketch: Sketch,
        depth: f64,
        #[serde(default)]
        end: EndCondition,
        #[serde(default)]
        draft_deg: f64,
        #[serde(default)]
        top_edge: EdgeTreatment,
        #[serde(default)]
        bottom_edge: EdgeTreatment,
        /// Extrude against the normal (Blind only).
        #[serde(default)]
        reverse: bool,
    },
    /// Revolve sketch profiles (local x = radius ≥ 0, local y = axial) about the sketch's local Y
    /// axis — on the XZ plane that is world Z. Partial angles start at local +X.
    SketchRevolve {
        sketch: Sketch,
        angle_deg: f64,
    },
    /// Sweep closed profiles (in the path's local normal plane) along a 3D polyline. The profile's
    /// local y follows world Z where possible; frames are parallel-transported (rotation-minimising).
    Sweep {
        profile: Vec<Polyline>,
        path: Vec<[f64; 3]>,
        #[serde(default)]
        twist_deg: f64,
    },
    /// Loft between horizontal sections (piecewise linear along Z, capped).
    Loft {
        sections: Vec<LoftSection>,
    },
    /// Hole from the placement origin going DOWN (−Z): place at a top face and use `BooleanOp::Cut`.
    /// Cutters extend `r + 1 mm` (r = largest radius) above the origin so they break cleanly
    /// through the face.
    Hole {
        #[serde(rename = "hole")]
        kind: HoleKind,
        diameter: f64,
        depth: f64,
    },
    /// Helical 60° V thread along +Z from 0 to `length`. `internal = true` models the *void* of an
    /// internal thread of that nominal size (cut it from a part); `depth` 0 = ISO default
    /// (0.6134·P external, 0.5413·P internal).
    Thread {
        major_diameter: f64,
        pitch: f64,
        length: f64,
        #[serde(default)]
        internal: bool,
        #[serde(default)]
        depth: f64,
    },
    /// An imported solid (STEP, IGES, 3MF, a SolidCraft `.sc` design, STL or OBJ) as the base of the
    /// part: the file's bytes are kept in the document (base64), so it is self-contained.
    Brep {
        /// "step", "igs", "3mf", "sc", "stl" or "obj": what the data is.
        format: String,
        /// The file's name it came from.
        #[serde(default)]
        source: String,
        data: String,
    },
    /// Rounds (`chamfer: false`) or chamfers picked edges of the part built so far. Each edge is
    /// named by a point on it (the nearest edge is meant; `cad.edges()` lists them). The B-rep kernel
    /// builds it exactly; the implicit kernel leaves the edges sharp and says so.
    EdgeBlend {
        #[serde(default)]
        chamfer: bool,
        size: f64,
        edges: Vec<[f64; 3]>,
    },
    /// Hollows the part built so far with this wall thickness, removing the faces picked by a point
    /// on each (no pick: a closed hollow body). The implicit kernel approximates it and says so.
    ShellFaces {
        thickness: f64,
        #[serde(default)]
        faces: Vec<[f64; 3]>,
    },
    /// Mirrors the part so far about a plane (a flat face is the usual choice: its centroid and
    /// normal) and joins the reflection to it.
    MirrorPlane {
        origin: [f64; 3],
        normal: [f64; 3],
    },
    /// Tapers the picked side faces by an angle about a neutral plane (the plane they stay fixed on),
    /// pulled along its normal. The B-rep kernel builds it exactly; the implicit kernel leaves the
    /// faces vertical and says so.
    Draft {
        angle_deg: f64,
        faces: Vec<[f64; 3]>,
        neutral_origin: [f64; 3],
        neutral_normal: [f64; 3],
    },
    /// Another part used as a body (a combine: add / subtract / intersect bodies).
    Body {
        part: Box<Document>,
    },
}

impl Shape {
    /// The constrained sketch inside this shape, if any.
    pub fn sketch_mut(&mut self) -> Option<&mut Sketch> {
        match self {
            Shape::SketchExtrude { sketch, .. } | Shape::SketchRevolve { sketch, .. } => Some(sketch),
            _ => None,
        }
    }
}

/// How far an extrusion goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum EndCondition {
    #[default]
    Blind,
    /// Symmetric about the sketch plane.
    MidPlane,
    /// Through everything in both directions (±5 m) — for cuts.
    ThroughAll,
}

/// Treatment of the cap edges of an extrusion.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(tag = "type")]
pub enum EdgeTreatment {
    #[default]
    None,
    Fillet {
        radius: f64,
    },
    Chamfer {
        distance: f64,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LoftSection {
    pub profile: Vec<Polyline>,
    pub z: f64,
}

/// Hole Wizard types.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(tag = "type")]
pub enum HoleKind {
    #[default]
    Simple,
    Counterbore {
        diameter: f64,
        depth: f64,
    },
    Countersink {
        diameter: f64,
        angle_deg: f64,
    },
    /// `diameter` is the nominal (major) size. Cosmetic = plain tap-drill hole; otherwise a modelled
    /// internal thread.
    Tapped {
        pitch: f64,
        #[serde(default)]
        cosmetic: bool,
    },
    /// Drill with a 118° point below `depth` (depth measured to the shoulder).
    Drilled,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum Axis {
    X,
    Y,
    #[default]
    Z,
}

/// Junction style of a feature's `blend` with the body.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum BlendKind {
    #[default]
    Round,
    Chamfer,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind")]
pub enum Modifier {
    /// Round all edges of the result so far by radius (offset trick).
    Fillet {
        radius: f64,
    },
    /// Hollow with wall thickness.
    Shell {
        thickness: f64,
    },
    LinearPattern {
        count: usize,
        step: [f64; 3],
    },
    CircularPattern {
        count: usize,
        axis_z: bool,
    },
    /// Keeps the original and adds its reflection.
    Mirror {
        plane: MirrorPlane,
    },
    /// Keep one side of a principal plane at `offset` (e.g. YZ + keep_positive → x ≥ offset).
    Split {
        plane: MirrorPlane,
        offset: f64,
        keep_positive: bool,
    },
    /// Hollow inward with wall `thickness`; `open_top` removes the top face (enclosure shell).
    ShellOpen {
        thickness: f64,
        open_top: bool,
    },
    /// `counts[0] × counts[1]` copies stepped along X and Y.
    GridPattern {
        counts: [usize; 2],
        step: [f64; 2],
    },
    /// `count` copies over `total_deg` about a world axis through the part origin (360 = full
    /// circle, otherwise first and last copy at 0 and `total_deg`). Unlike the other feature
    /// modifiers it applies *after* the feature placement, so a hole placed at (r, 0, z) with this
    /// modifier gives a bolt circle of radius r.
    CircularPatternAngle {
        count: usize,
        total_deg: f64,
        #[serde(default)]
        axis: Axis,
    },
    /// Uniform scale about the origin.
    Scale {
        factor: f64,
    },
    /// Moves the copies made so far by `by` in the feature's own frame: the first step of a bolt
    /// circle on a face (offset by the radius, then a circular pattern about the face's normal).
    Offset {
        by: [f64; 3],
    },
}

impl Modifier {
    /// Applied in part (world) coordinates after the feature placement rather than in the
    /// feature's local frame.
    pub fn is_world(&self) -> bool {
        matches!(self, Modifier::CircularPatternAngle { .. })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MirrorPlane {
    YZ,
    XZ,
    XY,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Feature {
    /// Stable identity, never reused within the document (assigned by [`Document::add`]).
    #[serde(default)]
    pub id: Id,
    pub name: String,
    pub op: BooleanOp,
    pub shape: Shape,
    pub placement: Placement,
    /// Blend radius with the existing body (0 = sharp boolean).
    pub blend: f64,
    pub modifiers: Vec<Modifier>,
    pub suppressed: bool,
    /// Rounded (default) or chamfered junction for `blend`.
    #[serde(default)]
    pub blend_kind: BlendKind,
    /// Expressions written into numeric fields of this feature (see [`Binding`]).
    #[serde(default)]
    pub bindings: Vec<Binding>,
}

/// A part placed in an assembly: its own document, kept inside, and where it sits.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ComponentRef {
    pub id: Id,
    pub name: String,
    pub document: Box<Document>,
    #[serde(default)]
    pub placement: Placement,
    #[serde(default = "yes")]
    pub visible: bool,
}

fn yes() -> bool {
    true
}

/// A named, expression-valued part parameter (e.g. `width = 80mm`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct Parameter {
    pub name: String,
    pub expr: String,
    #[serde(default)]
    pub comment: String,
}

impl Parameter {
    pub fn new(name: impl Into<String>, expr: impl Into<String>) -> Self {
        Self { name: name.into(), expr: expr.into(), comment: String::new() }
    }
}

/// Drive a numeric field of a feature by an expression. `path` is a JSON pointer inside the
/// serialized [`Feature`], e.g. `/shape/size/0`, `/placement/translate/2`, `/modifiers/0/count`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Binding {
    pub path: String,
    pub expr: String,
}

/// A named set of parameter overrides: a variant of the part, like a row of a design table.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct Configuration {
    pub name: String,
    #[serde(default)]
    pub overrides: Vec<Parameter>,
}

impl Feature {
    pub fn new(name: impl Into<String>, op: BooleanOp, shape: Shape) -> Self {
        Self {
            id: Id(0),
            name: name.into(),
            op,
            shape,
            placement: Placement::default(),
            blend: 0.0,
            modifiers: vec![],
            suppressed: false,
            blend_kind: BlendKind::Round,
            bindings: vec![],
        }
    }
    /// Chamfered junction of size `d` with the body.
    pub fn chamfered(mut self, d: f64) -> Self {
        self.blend = d;
        self.blend_kind = BlendKind::Chamfer;
        self
    }
    pub fn with(mut self, m: Modifier) -> Self {
        self.modifiers.push(m);
        self
    }
    /// Bind the numeric field at JSON pointer `path` to `expr`.
    pub fn bind(mut self, path: impl Into<String>, expr: impl Into<String>) -> Self {
        self.bindings.push(Binding { path: path.into(), expr: expr.into() });
        self
    }
    pub fn at(mut self, x: f64, y: f64, z: f64) -> Self {
        self.placement.translate = [x, y, z];
        self
    }
    pub fn rotated(mut self, rx: f64, ry: f64, rz: f64) -> Self {
        self.placement.rotate_deg = [rx, ry, rz];
        self
    }
    pub fn blended(mut self, r: f64) -> Self {
        self.blend = r;
        self
    }
}

/// A named plane to sketch on (made by offset, angle or three points), kept with the part.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RefPlane {
    pub name: String,
    pub frame: crate::sketch::PlaneFrame,
    /// How it was made, for display ("Top offset 20", "through 3 points").
    #[serde(default)]
    pub how: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct Document {
    pub name: String,
    pub features: Vec<Feature>,
    /// Modifiers applied to the whole part (e.g. a global fillet).
    pub finish: Vec<Modifier>,
    #[serde(default)]
    pub parameters: Vec<Parameter>,
    #[serde(default)]
    pub configurations: Vec<Configuration>,
    #[serde(default)]
    pub active_configuration: Option<String>,
    /// The next id to hand out (never decreases).
    #[serde(default)]
    pub next_id: u64,
    /// Parts placed beside this one (an assembly): each keeps its own document.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub components: Vec<ComponentRef>,
    /// Reference planes to sketch on.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub planes: Vec<RefPlane>,
}

impl Document {
    pub fn new(name: impl Into<String>) -> Self {
        Self { name: name.into(), ..Default::default() }
    }
    /// A fresh stable id.
    pub fn new_id(&mut self) -> Id {
        self.next_id = self.next_id.max(self.features.iter().map(|f| f.id.0).max().unwrap_or(0)) + 1;
        Id(self.next_id)
    }
    /// Append a feature to the timeline, giving it a stable id when it has none.
    #[allow(clippy::should_implement_trait)]
    pub fn add(mut self, f: Feature) -> Self {
        self.push(f);
        self
    }
    /// The same as [`Document::add`] on a mutable document; returns the feature's id.
    pub fn push(&mut self, mut f: Feature) -> Id {
        if f.id.0 == 0 || self.features.iter().any(|g| g.id == f.id) {
            f.id = self.new_id();
        } else {
            self.next_id = self.next_id.max(f.id.0);
        }
        let id = f.id;
        self.features.push(f);
        id
    }
    /// Give every feature an id of its own: files written by hand or by older versions may
    /// have none (0) or the same one twice, and faces, selections and the comparison of two
    /// versions of a part all go by id. Features that already have a unique id keep it.
    pub fn normalize_ids(&mut self) {
        let mut seen = std::collections::HashSet::new();
        let mut next = self.next_id.max(self.features.iter().map(|f| f.id.0).max().unwrap_or(0)).max(self.components.iter().map(|c| c.id.0).max().unwrap_or(0));
        for f in &mut self.features {
            if f.id.0 == 0 || !seen.insert(f.id) {
                next += 1;
                f.id = Id(next);
                seen.insert(f.id);
            }
            if let Shape::Body { part } = &mut f.shape {
                part.normalize_ids();
            }
        }
        self.next_id = next;
        for c in &mut self.components {
            c.document.normalize_ids();
        }
    }

    /// The feature with the given id.
    pub fn feature(&self, id: Id) -> Option<&Feature> {
        self.features.iter().find(|f| f.id == id)
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self).unwrap_or_default()
    }
    pub fn from_json(s: &str) -> Result<Document, serde_json::Error> {
        serde_json::from_str(s).map(|mut d: Document| {
            d.normalize_ids();
            d
        })
    }
}

/// A ready-made demo part (used by the "New 3D part" template and tests): a rounded plaque with a
/// revolved knob and four counterbored holes.
pub fn demo_part() -> Document {
    let mut p = Document::new("Demo knob plate")
        .add(Feature::new("Base", BooleanOp::Add, Shape::Box { size: [80.0, 50.0, 8.0], corner_radius: 3.0 }).at(0.0, 0.0, 4.0))
        .add(
            Feature::new(
                "Knob",
                BooleanOp::Add,
                Shape::Revolve {
                    profile: vec![Polyline::closed(vec![
                        DVec2::new(0.0, 0.0),
                        DVec2::new(15.0, 0.0),
                        DVec2::new(12.0, 10.0),
                        DVec2::new(6.0, 16.0),
                        DVec2::new(0.0, 17.0),
                    ])],
                    angle_deg: 360.0,
                },
            )
            .at(0.0, 0.0, 8.0)
            .blended(2.0),
        );
    let mut hole = Feature::new("Hole", BooleanOp::Cut, Shape::Cylinder { radius: 2.5, height: 30.0 }).at(-32.0, -17.0, 0.0);
    hole.modifiers.push(Modifier::LinearPattern { count: 2, step: [64.0, 0.0, 0.0] });
    let mut hole2 = hole.clone();
    hole2.name = "Hole row 2".into();
    hole2.placement.translate[1] = 17.0;
    p.push(hole);
    p.push(hole2);
    p.finish.push(Modifier::Fillet { radius: 0.8 });
    p
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sketch::SketchPlane;
    use glam::DVec3;

    #[test]
    fn box_volume() {
        let p = Document::new("b").add(Feature::new("b", BooleanOp::Add, Shape::Box { size: [10.0, 20.0, 5.0], corner_radius: 0.0 }));
        let m = p.mesh(0.5);
        assert_eq!(m.boundary_edge_count(), 0);
        assert!((m.volume() - 1000.0).abs() / 1000.0 < 0.03, "{}", m.volume());
    }

    #[test]
    fn cut_reduces_volume() {
        let base = Document::new("b").add(Feature::new("b", BooleanOp::Add, Shape::Box { size: [20.0, 20.0, 10.0], corner_radius: 0.0 }));
        let cut = base.clone().add(Feature::new("h", BooleanOp::Cut, Shape::Cylinder { radius: 4.0, height: 30.0 }).at(0.0, 0.0, -15.0));
        let v0 = base.mesh(0.5).volume();
        let v1 = cut.mesh(0.5).volume();
        let hole = std::f32::consts::PI * 16.0 * 10.0;
        assert!(((v0 - v1) - hole).abs() / hole < 0.1, "{} vs {}", v0 - v1, hole);
    }

    #[test]
    fn extrude_sketch() {
        let sq = crate::path::shapes::rectangle(DVec2::new(-5.0, -5.0), 10.0, 10.0, 0.0);
        let p = Document::new("e").add(Feature::new("e", BooleanOp::Add, Shape::Extrude { sketch: vec![sq], height: 4.0, draft_deg: 0.0 }));
        let v = p.mesh(0.4).volume();
        assert!((v - 400.0).abs() / 400.0 < 0.05, "{v}");
    }

    // ---- solid-modeler features -------------------------------------------------------------
    use std::f64::consts::PI;

    fn part(shape: Shape) -> Document {
        Document::new("t").add(Feature::new("f", BooleanOp::Add, shape))
    }
    fn vol(p: &Document, h: f64) -> f64 {
        p.mesh(h).volume() as f64
    }
    fn close(v: f64, e: f64, tol: f64) -> bool {
        (v - e).abs() / e < tol
    }
    fn extrude(sketch: Sketch, depth: f64, end: EndCondition) -> Shape {
        Shape::SketchExtrude { sketch, depth, end, draft_deg: 0.0, top_edge: EdgeTreatment::None, bottom_edge: EdgeTreatment::None, reverse: false }
    }
    fn block() -> Document {
        part(Shape::Box { size: [40.0, 40.0, 20.0], corner_radius: 0.0 }).add(Feature::new("noop", BooleanOp::Add, Shape::Sphere { radius: 0.1 })).tap(|p| {
            p.features[0].placement.translate = [0.0, 0.0, 10.0];
        })
    }
    trait Tap: Sized {
        fn tap(mut self, f: impl FnOnce(&mut Self)) -> Self {
            f(&mut self);
            self
        }
    }
    impl Tap for Document {}

    #[test]
    fn sketch_extrude_end_conditions() {
        let p = part(extrude(Sketch::rectangle(20.0, 10.0), 5.0, EndCondition::Blind));
        assert!(close(vol(&p, 0.4), 1000.0, 0.03), "{}", vol(&p, 0.4));
        let (lo, hi) = p.sdf().unwrap().bounds();
        assert!((lo.z, hi.z) == (0.0, 5.0) && (hi.x - 10.0).abs() < 1e-6);
        let mid = part(extrude(Sketch::rectangle(20.0, 10.0), 5.0, EndCondition::MidPlane));
        let (lo, hi) = mid.sdf().unwrap().bounds();
        assert!((lo.z + 2.5).abs() < 1e-9 && (hi.z - 2.5).abs() < 1e-9);
        let mut rev = extrude(Sketch::rectangle(20.0, 10.0), 5.0, EndCondition::Blind);
        if let Shape::SketchExtrude { reverse, .. } = &mut rev {
            *reverse = true;
        }
        assert!(part(rev).sdf().unwrap().eval(DVec3::new(0.0, 0.0, -2.5)) < -2.0);
        // On the YZ plane the extrusion runs along +X from the plane offset.
        let yz = part(extrude(Sketch::rectangle(20.0, 10.0).on(SketchPlane::YZ, 3.0), 5.0, EndCondition::Blind));
        let (lo, hi) = yz.sdf().unwrap().bounds();
        assert!((lo.x - 3.0).abs() < 1e-9 && (hi.x - 8.0).abs() < 1e-9 && (hi.y - 10.0).abs() < 1e-6 && (hi.z - 5.0).abs() < 1e-6);
        // Through-all cut removes a full cylinder from a block.
        let base = block();
        let cut = base.clone().add(Feature::new("c", BooleanOp::Cut, extrude(Sketch::circle(3.0), 1.0, EndCondition::ThroughAll)));
        let removed = vol(&base, 0.5) - vol(&cut, 0.5);
        assert!(close(removed, PI * 9.0 * 20.0, 0.1), "{removed}");
    }

    #[test]
    fn extrude_edge_treatments_and_draft() {
        let mk = |top: EdgeTreatment, draft: f64| {
            part(Shape::SketchExtrude {
                sketch: Sketch::rectangle(20.0, 10.0),
                depth: 5.0,
                end: EndCondition::Blind,
                draft_deg: draft,
                top_edge: top,
                bottom_edge: EdgeTreatment::None,
                reverse: false,
            })
            .sdf()
            .unwrap()
        };
        let edge = DVec3::new(10.0, 0.0, 5.0); // the sharp top edge
        let f = mk(EdgeTreatment::Fillet { radius: 2.0 }, 0.0);
        assert!((f.eval(edge) - (2f64.sqrt() - 1.0) * 2.0).abs() < 1e-9);
        assert!(f.eval(DVec3::new(10.0, 0.0, 0.0)).abs() < 1e-9, "bottom edge stays sharp");
        let c = mk(EdgeTreatment::Chamfer { distance: 2.0 }, 0.0);
        assert!((c.eval(edge) - 2.0 * std::f64::consts::FRAC_1_SQRT_2).abs() < 1e-9);
        assert!(c.eval(DVec3::new(9.0, 0.0, 4.0)).abs() < 1e-9, "chamfer face passes through (9, 4)");
        let d = mk(EdgeTreatment::None, 10.0);
        let top_x = 10.0 - 5.0 * 10f64.to_radians().tan();
        assert!(d.eval(DVec3::new(top_x, 0.0, 4.9999)).abs() < 1e-3);
        assert!(d.eval(DVec3::new(10.0, 0.0, 0.0)).abs() < 1e-9);
    }

    #[test]
    fn revolves_full_and_partial() {
        let mut sk = Sketch::polygon(&[[5.0, 0.0], [10.0, 0.0], [10.0, 4.0], [5.0, 4.0]]);
        sk.plane = SketchPlane::XZ;
        let full = part(Shape::SketchRevolve { sketch: sk.clone(), angle_deg: 360.0 });
        let washer = PI * (100.0 - 25.0) * 4.0;
        assert!(close(vol(&full, 0.35), washer, 0.04), "{}", vol(&full, 0.35));
        let (lo, hi) = full.sdf().unwrap().bounds();
        assert!((hi.z - 4.0).abs() < 1e-6 && lo.z.abs() < 1e-6, "XZ sketch revolves about world Z");
        let quarter = part(Shape::SketchRevolve { sketch: sk, angle_deg: 90.0 });
        assert!(close(vol(&quarter, 0.35), washer / 4.0, 0.05), "{}", vol(&quarter, 0.35));
        // Legacy polyline revolve: half angle gives half the volume.
        let prof = vec![Polyline::closed(vec![DVec2::new(0.0, 0.0), DVec2::new(10.0, 0.0), DVec2::new(10.0, 5.0), DVec2::new(0.0, 5.0)])];
        let half = vol(&part(Shape::Revolve { profile: prof.clone(), angle_deg: 180.0 }), 0.4);
        let whole = vol(&part(Shape::Revolve { profile: prof, angle_deg: 360.0 }), 0.4);
        assert!(close(half, whole / 2.0, 0.04), "{half} vs {whole}");
        assert!(close(whole, PI * 100.0 * 5.0, 0.03));
    }

    #[test]
    fn sweeps() {
        let circle = vec![crate::path::shapes::circle(DVec2::ZERO, 2.0)];
        let straight = part(Shape::Sweep { profile: circle.clone(), path: vec![[0.0, 0.0, 0.0], [50.0, 0.0, 0.0]], twist_deg: 0.0 });
        assert!(close(vol(&straight, 0.25), PI * 4.0 * 50.0, 0.05), "{}", vol(&straight, 0.25));
        let l = part(Shape::Sweep { profile: circle, path: vec![[0.0, 0.0, 0.0], [30.0, 0.0, 0.0], [30.0, 30.0, 0.0]], twist_deg: 0.0 });
        let m = l.mesh(0.3);
        assert_eq!(m.boundary_edge_count(), 0, "watertight");
        assert!(close(m.volume() as f64, PI * 4.0 * 60.0, 0.06), "{}", m.volume());
        // No interior seam at the mitre joint (matters when the sweep is used as a cut).
        let s = l.sdf().unwrap();
        let joint = DVec3::new(30.0, 0.0, 0.0) + DVec3::new(-1.0, 1.0, 0.0).normalize() * 0.5;
        assert!(s.eval(joint) < -1.0, "{}", s.eval(joint));
        // Twist: a square rotated 45° by the end of the path.
        let sq = vec![crate::path::shapes::rectangle(DVec2::new(-2.0, -2.0), 4.0, 4.0, 0.0)];
        let tw = part(Shape::Sweep { profile: sq, path: vec![[0.0, 0.0, 0.0], [40.0, 0.0, 0.0]], twist_deg: 45.0 }).sdf().unwrap();
        assert!(tw.eval(DVec3::new(39.99, 2.7, 0.0)) < 0.0 && tw.eval(DVec3::new(0.01, 2.7, 0.0)) > 0.0);
        assert!(tw.eval(DVec3::new(0.01, 1.9, 1.9)) < 0.0 && tw.eval(DVec3::new(39.99, 1.9, 1.9)) > 0.0);
    }

    #[test]
    fn loft_frustum_volume() {
        let sq = |s: f64| vec![crate::path::shapes::rectangle(DVec2::splat(-s / 2.0), s, s, 0.0)];
        let p = part(Shape::Loft { sections: vec![LoftSection { profile: sq(20.0), z: 0.0 }, LoftSection { profile: sq(10.0), z: 10.0 }] });
        let (a1, a2): (f64, f64) = (400.0, 100.0);
        let frustum = 10.0 / 3.0 * (a1 + a2 + (a1 * a2).sqrt());
        assert!(close(vol(&p, 0.3), frustum, 0.06), "{} vs {frustum}", vol(&p, 0.3));
        let s = p.sdf().unwrap();
        assert!(s.eval(DVec3::new(7.5, 0.0, 5.0)).abs() < 0.05, "mid section half-width 7.5");
    }

    #[test]
    fn holes() {
        let base = block();
        let v0 = vol(&base, 0.4);
        let with = |kind: HoleKind, d: f64, depth: f64| {
            base.clone().add(Feature::new("h", BooleanOp::Cut, Shape::Hole { kind, diameter: d, depth }).at(0.0, 0.0, 20.0))
        };
        let simple = with(HoleKind::Simple, 6.0, 10.0);
        assert!(close(v0 - vol(&simple, 0.4), PI * 9.0 * 10.0, 0.1), "{}", v0 - vol(&simple, 0.4));
        let s = simple.sdf().unwrap();
        assert!(s.eval(DVec3::new(0.0, 0.0, 10.5)) > 0.0 && s.eval(DVec3::new(0.0, 0.0, 9.5)) < 0.0, "blind depth 10");

        let cb = with(HoleKind::Counterbore { diameter: 11.0, depth: 6.0 }, 6.0, 15.0).sdf().unwrap();
        assert!(cb.eval(DVec3::new(5.0, 0.0, 15.0)) > 0.0 && cb.eval(DVec3::new(5.0, 0.0, 13.0)) < 0.0 && cb.eval(DVec3::new(2.5, 0.0, 7.0)) > 0.0);

        let cs = with(HoleKind::Countersink { diameter: 12.0, angle_deg: 90.0 }, 6.0, 15.0).sdf().unwrap();
        // 90° sink: radius 6 at the face, 3 at 3 mm below.
        assert!(cs.eval(DVec3::new(5.8, 0.0, 19.9)) > 0.0 && cs.eval(DVec3::new(4.0, 0.0, 17.5)) < 0.0 && cs.eval(DVec3::new(4.0, 0.0, 18.5)) > 0.0);

        let dr = with(HoleKind::Drilled, 6.0, 10.0).sdf().unwrap();
        let tip = 3.0 / 59f64.to_radians().tan();
        assert!(dr.eval(DVec3::new(0.0, 0.0, 10.0 - tip * 0.5)) > 0.0 && dr.eval(DVec3::new(2.4, 0.0, 10.0 - tip * 0.9)) < 0.0);

        let cosmetic = with(HoleKind::Tapped { pitch: 1.5, cosmetic: true }, 10.0, 12.0);
        let minor = 5.0 - 0.5413 * 1.5;
        assert!(close(v0 - vol(&cosmetic, 0.4), PI * minor * minor * 12.0, 0.1));
        let tapped = with(HoleKind::Tapped { pitch: 1.5, cosmetic: false }, 10.0, 12.0);
        let removed = v0 - vol(&tapped, 0.25);
        assert!(removed > PI * minor * minor * 12.0 && removed < PI * 25.0 * 12.0, "{removed}");
    }

    #[test]
    fn thread_between_minor_and_major() {
        let p = part(Shape::Thread { major_diameter: 10.0, pitch: 1.5, length: 12.0, internal: false, depth: 0.0 });
        let v = vol(&p, 0.15);
        let rmin = 5.0 - 0.6134 * 1.5;
        let (vmin, vmax) = (PI * rmin * rmin * 12.0, PI * 25.0 * 12.0);
        assert!(v > vmin * 1.05 && v < vmax * 0.97, "{vmin} < {v} < {vmax}");
        // Helix: along a line just inside the major radius the field alternates twice per pitch.
        let s = p.sdf().unwrap();
        let signs: Vec<bool> = (0..1200).map(|i| s.eval(DVec3::new(4.8, 0.0, 0.5 + i as f64 * 0.009)) < 0.0).collect();
        let flips = signs.windows(2).filter(|w| w[0] != w[1]).count();
        assert!((13..=15).contains(&flips), "{flips}");
        // Advances one pitch per turn (right hand): +90° ⇒ +P/4 in z.
        let z0 = 3.0;
        let a = s.eval(DVec3::new(4.8, 0.0, z0));
        let b = s.eval(DVec3::new(0.0, 4.8, z0 + 0.375));
        assert!((a - b).abs() < 1e-9);
    }

    #[test]
    fn body_combine() {
        let tool = part(Shape::Sphere { radius: 5.0 });
        let p = Document::new("combine")
            .add(Feature::new("a", BooleanOp::Add, Shape::Body { part: Box::new(part(Shape::Box { size: [20.0; 3], corner_radius: 0.0 })) }))
            .add(Feature::new("b", BooleanOp::Cut, Shape::Body { part: Box::new(tool) }).at(10.0, 10.0, 10.0));
        let e = 8000.0 - 4.0 / 3.0 * PI * 125.0 / 8.0;
        assert!(close(vol(&p, 0.4), e, 0.03), "{}", vol(&p, 0.4));
        assert!(part(Shape::Body { part: Box::new(Document::new("empty")) }).mesh(1.0).is_empty());
    }

    #[test]
    fn chamfer_blend_vs_round() {
        let mk = |kind: BlendKind| {
            let mut boss = Feature::new("boss", BooleanOp::Add, Shape::Box { size: [6.0; 3], corner_radius: 0.0 }).at(0.0, 0.0, 13.0).blended(2.0);
            boss.blend_kind = kind;
            part(Shape::Box { size: [20.0, 20.0, 10.0], corner_radius: 0.0 })
                .tap(|p| p.features[0].placement.translate = [0.0, 0.0, 5.0])
                .add(boss)
                .sdf()
                .unwrap()
        };
        let q = DVec3::new(4.0, 0.0, 11.0); // 1 mm from both faces at the concave edge
        assert!(mk(BlendKind::Chamfer).eval(q).abs() < 1e-9, "on the 45° chamfer face");
        assert!((mk(BlendKind::Round).eval(q) - 0.5).abs() < 1e-9);
        let cut = block().add(Feature::new("h", BooleanOp::Cut, Shape::Cylinder { radius: 3.0, height: 30.0 }).chamfered(1.0)).sdf().unwrap();
        assert!(cut.eval(DVec3::new(3.3, 0.0, 19.7)) > 0.0, "chamfered hole rim");
        assert!(cut.eval(DVec3::new(3.5, 0.0, 15.0)) < 0.0);
    }

    #[test]
    fn modifiers_split_shell_patterns_scale_mirror() {
        let sphere = |m: Modifier| part(Shape::Sphere { radius: 10.0 }).tap(|p| p.features[0].modifiers.push(m));
        let v_sphere = 4.0 / 3.0 * PI * 1000.0;
        let half = sphere(Modifier::Split { plane: MirrorPlane::YZ, offset: 0.0, keep_positive: true });
        assert!(close(vol(&half, 0.4), v_sphere / 2.0, 0.04));
        assert!(half.sdf().unwrap().bounds().0.x >= 0.0);

        let shelled = |open| {
            part(Shape::Box { size: [40.0, 30.0, 20.0], corner_radius: 0.0 })
                .tap(|p| p.features[0].modifiers.push(Modifier::ShellOpen { thickness: 2.0, open_top: open }))
        };
        assert!(close(vol(&shelled(true), 0.4), 24000.0 - 36.0 * 26.0 * 18.0, 0.06), "{}", vol(&shelled(true), 0.4));
        assert!(close(vol(&shelled(false), 0.4), 24000.0 - 36.0 * 26.0 * 16.0, 0.06), "{}", vol(&shelled(false), 0.4));
        assert!(shelled(true).sdf().unwrap().eval(DVec3::new(0.0, 0.0, 9.9)) > 0.0 && shelled(false).sdf().unwrap().eval(DVec3::new(0.0, 0.0, 9.9)) < 0.0);

        let cyl = |m: Modifier| part(Shape::Cylinder { radius: 2.0, height: 5.0 }).tap(|p| p.features[0].modifiers.push(m));
        let one = PI * 4.0 * 5.0;
        assert!(close(vol(&cyl(Modifier::GridPattern { counts: [3, 2], step: [10.0, 8.0] }), 0.3), 6.0 * one, 0.05));

        let arc = part(Shape::Box { size: [4.0; 3], corner_radius: 0.0 })
            .tap(|p| p.features[0].placement.translate = [20.0, 0.0, 0.0])
            .tap(|p| p.features[0].modifiers.push(Modifier::CircularPatternAngle { count: 4, total_deg: 90.0, axis: Axis::Z }))
            .sdf()
            .unwrap();
        for deg in [0.0f64, 30.0, 60.0, 90.0] {
            let a = deg.to_radians();
            assert!(arc.eval(DVec3::new(20.0 * a.cos(), 20.0 * a.sin(), 0.0)) < -1.5, "{deg}");
        }
        assert!(arc.eval(DVec3::new(20.0 * 2.1f64.cos(), 20.0 * 2.1f64.sin(), 0.0)) > 0.0);

        let scaled = sphere(Modifier::Scale { factor: 2.0 });
        assert!(scaled.sdf().unwrap().eval(DVec3::new(20.0, 0.0, 0.0)).abs() < 1e-9);

        // Mirror keeps the original (here entirely on the −X side) and adds the copy.
        let m = part(Shape::Box { size: [4.0; 3], corner_radius: 0.0 }).tap(|p| p.finish.push(Modifier::Mirror { plane: MirrorPlane::YZ }));
        let m = m.tap(|p| p.features[0].placement.translate = [-10.0, 0.0, 0.0]);
        let s = m.sdf().unwrap();
        assert!(s.eval(DVec3::new(-10.0, 0.0, 0.0)) < -1.9 && s.eval(DVec3::new(10.0, 0.0, 0.0)) < -1.9);
        assert!(close(vol(&m, 0.25), 128.0, 0.04), "{}", vol(&m, 0.25));
    }

    #[test]
    fn old_json_still_loads() {
        let mut v = serde_json::to_value(demo_part()).unwrap();
        let o = v.as_object_mut().unwrap();
        for k in ["parameters", "configurations", "active_configuration"] {
            o.remove(k);
        }
        for f in o["features"].as_array_mut().unwrap() {
            f.as_object_mut().unwrap().remove("blend_kind");
            f.as_object_mut().unwrap().remove("bindings");
        }
        let back: Document = serde_json::from_value(v).unwrap();
        assert_eq!(back.features.len(), demo_part().features.len());
        assert!(back.parameters.is_empty() && back.features.iter().all(|f| f.blend_kind == BlendKind::Round));
    }
}
