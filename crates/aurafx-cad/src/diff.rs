//! What changed between two builds of a part, for the downstream check: bodies and faces
//! matched by stable ids, then by name, then by their place (kind, direction, centroid, area),
//! with the successor of every face that went away, or the candidates when several fit.

use crate::kernel::Evaluated;
use crate::Document;
use serde::{Deserialize, Serialize};

/// One face as the check sees it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FaceSnap {
    /// `(feature id, index in the feature)`: stable while the owning feature exists.
    pub id: (u64, u32),
    /// The `FaceId` in the surface (what downstream arrays hold).
    pub face_id: u32,
    pub name: String,
    pub kind: String,
    pub area: f64,
    pub centroid: [f64; 3],
    pub direction: [f64; 3],
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BodySnap {
    pub id: u64,
    pub name: String,
    pub volume: f64,
    pub centroid: [f64; 3],
}

/// A build reduced to what the check compares.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct GeometrySnapshot {
    pub bodies: Vec<BodySnap>,
    pub faces: Vec<FaceSnap>,
    pub bounds: ([f64; 3], [f64; 3]),
    pub volume: f64,
    /// The names of the document's parameters.
    pub parameters: Vec<String>,
    /// The names of the document's features by id (a renamed hole keeps its id).
    pub features: Vec<(u64, String)>,
}

impl GeometrySnapshot {
    pub fn of(ev: &Evaluated, doc: &Document) -> Self {
        let mut lo = [f64::INFINITY; 3];
        let mut hi = [f64::NEG_INFINITY; 3];
        for b in &ev.bodies {
            for k in 0..3 {
                lo[k] = lo[k].min(b.bounds.0[k]);
                hi[k] = hi[k].max(b.bounds.1[k]);
            }
        }
        Self {
            bodies: ev.bodies.iter().map(|b| BodySnap { id: b.id.0, name: b.name.clone(), volume: b.volume, centroid: b.centroid }).collect(),
            faces: ev
                .faces()
                .map(|(_, f)| FaceSnap {
                    id: ((f.id.0).0, f.id.1),
                    face_id: f.face_id,
                    name: f.name.clone(),
                    kind: f.kind.label().to_string(),
                    area: f.area,
                    centroid: f.centroid,
                    direction: f.direction,
                })
                .collect(),
            bounds: (lo, hi),
            volume: ev.volume(),
            parameters: doc.parameters.iter().map(|p| p.name.clone()).collect(),
            features: doc.features.iter().map(|f| (f.id.0, f.name.clone())).collect(),
        }
    }

    pub fn size(&self) -> f64 {
        (0..3).map(|k| self.bounds.1[k] - self.bounds.0[k]).fold(0.0, f64::max)
    }
}

/// What happened to a face of the old build.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum FaceFate {
    /// Still there under the same name.
    Same,
    /// Still there (the same feature and index, or the matching place) under another name or number.
    Renamed { to: FaceSnap },
    /// Gone, with one plausible successor (a face of the same kind in the same place).
    Replaced { by: FaceSnap },
    /// Gone, with several equally plausible successors.
    Ambiguous { candidates: Vec<FaceSnap> },
    /// Gone.
    Removed,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FaceChange {
    pub old: FaceSnap,
    pub fate: FaceFate,
    /// The new face's numbers differ (area or place moved by more than a few percent).
    pub moved: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct GeometryDiff {
    pub faces: Vec<FaceChange>,
    pub faces_added: Vec<FaceSnap>,
    pub bodies_added: Vec<BodySnap>,
    pub bodies_removed: Vec<BodySnap>,
    pub parameters_added: Vec<String>,
    pub parameters_removed: Vec<String>,
    pub old_bounds: ([f64; 3], [f64; 3]),
    pub new_bounds: ([f64; 3], [f64; 3]),
    pub old_volume: f64,
    pub new_volume: f64,
}

impl GeometryDiff {
    /// Whether anything a downstream node could depend on changed.
    pub fn is_empty(&self) -> bool {
        self.faces.iter().all(|f| matches!(f.fate, FaceFate::Same) && !f.moved)
            && self.faces_added.is_empty()
            && self.bodies_added.is_empty()
            && self.bodies_removed.is_empty()
            && self.parameters_removed.is_empty()
            && self.old_bounds == self.new_bounds
    }

    /// A plain-text description for people and for the assistant's prompt.
    pub fn describe(&self) -> String {
        let mut out = String::new();
        let b = |b: &([f64; 3], [f64; 3])| format!("({:.2}, {:.2}, {:.2}) to ({:.2}, {:.2}, {:.2})", b.0[0], b.0[1], b.0[2], b.1[0], b.1[1], b.1[2]);
        out += &format!("bounds {} -> {}; volume {:.1} -> {:.1}\n", b(&self.old_bounds), b(&self.new_bounds), self.old_volume, self.new_volume);
        for f in &self.faces {
            match &f.fate {
                FaceFate::Same if !f.moved => {}
                FaceFate::Same => out += &format!("face '{}' (number {}) changed size or place\n", f.old.name, f.old.face_id),
                FaceFate::Renamed { to } => out += &format!("face '{}' (number {}) is now '{}' (number {})\n", f.old.name, f.old.face_id, to.name, to.face_id),
                FaceFate::Replaced { by } => out += &format!("face '{}' is gone; '{}' is the nearest successor\n", f.old.name, by.name),
                FaceFate::Ambiguous { candidates } => {
                    out += &format!("face '{}' is gone; it may now be: {}\n", f.old.name, candidates.iter().map(|c| format!("'{}'", c.name)).collect::<Vec<_>>().join(", "))
                }
                FaceFate::Removed => out += &format!("face '{}' is gone\n", f.old.name),
            }
        }
        for f in &self.faces_added {
            out += &format!("new face '{}' ({}, area {:.1})\n", f.name, f.kind, f.area);
        }
        for p in &self.parameters_removed {
            out += &format!("dimension '{p}' was removed\n");
        }
        for p in &self.parameters_added {
            out += &format!("dimension '{p}' was added\n");
        }
        for body in &self.bodies_removed {
            out += &format!("body '{}' is gone\n", body.name);
        }
        for body in &self.bodies_added {
            out += &format!("new body '{}'\n", body.name);
        }
        out
    }
}

fn dist(a: [f64; 3], b: [f64; 3]) -> f64 {
    ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)).sqrt()
}
fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

/// How well `new` could be the place of `old`: `None` when it cannot, else a cost (smaller is
/// better). Same kind; the direction within 25 degrees when both have one; the centroid within
/// `reach` (a fraction of the part's size); the area within a factor of three.
fn place_cost(old: &FaceSnap, new: &FaceSnap, reach: f64) -> Option<f64> {
    if old.kind != new.kind {
        return None;
    }
    let (ln, lo) = (dot(new.direction, new.direction).sqrt(), dot(old.direction, old.direction).sqrt());
    if ln > 0.5 && lo > 0.5 && dot(old.direction, new.direction) / (ln * lo) < 0.9 {
        return None;
    }
    let d = dist(old.centroid, new.centroid);
    if d > reach {
        return None;
    }
    let ratio = if old.area > 1e-12 && new.area > 1e-12 { (old.area / new.area).max(new.area / old.area) } else { 1.0 };
    if ratio > 3.0 {
        return None;
    }
    Some(d / reach.max(1e-12) + (ratio - 1.0) * 0.25)
}

/// Compare two builds.
pub fn diff(old: &GeometrySnapshot, new: &GeometrySnapshot) -> GeometryDiff {
    let reach = 0.35 * old.size().max(new.size()).max(1e-9);
    let mut taken = vec![false; new.faces.len()];
    let mut out = GeometryDiff {
        old_bounds: old.bounds,
        new_bounds: new.bounds,
        old_volume: old.volume,
        new_volume: new.volume,
        parameters_removed: old.parameters.iter().filter(|p| !new.parameters.contains(p)).cloned().collect(),
        parameters_added: new.parameters.iter().filter(|p| !old.parameters.contains(p)).cloned().collect(),
        ..Default::default()
    };
    let changed_place = |o: &FaceSnap, n: &FaceSnap| {
        let tol = 0.03 * old.size().max(1e-9);
        dist(o.centroid, n.centroid) > tol || (o.area - n.area).abs() > 0.05 * o.area.max(1e-9)
    };
    // 1. the same feature and index; 2. the same name.
    let mut fates: Vec<Option<(usize, bool)>> = vec![None; old.faces.len()];
    for (i, o) in old.faces.iter().enumerate() {
        if let Some(j) = new.faces.iter().enumerate().position(|(j, n)| n.id == o.id && !taken[j]) {
            taken[j] = true;
            fates[i] = Some((j, true));
        }
    }
    for (i, o) in old.faces.iter().enumerate() {
        if fates[i].is_some() {
            continue;
        }
        if let Some(j) = new.faces.iter().enumerate().position(|(j, n)| !taken[j] && n.name == o.name) {
            taken[j] = true;
            fates[i] = Some((j, false));
        }
    }
    for (i, o) in old.faces.iter().enumerate() {
        let fate = match fates[i] {
            Some((j, _)) => {
                let n = &new.faces[j];
                if n.name == o.name {
                    FaceFate::Same
                } else {
                    FaceFate::Renamed { to: n.clone() }
                }
            }
            None => {
                // 3. by place among the faces nobody has taken.
                let mut cands: Vec<(usize, f64)> = new.faces.iter().enumerate().filter(|(j, _)| !taken[*j]).filter_map(|(j, n)| place_cost(o, n, reach).map(|c| (j, c))).collect();
                cands.sort_by(|a, b| a.1.total_cmp(&b.1));
                match cands.as_slice() {
                    [] => FaceFate::Removed,
                    [(j, _)] => FaceFate::Replaced { by: new.faces[*j].clone() },
                    [(j, c0), (_, c1), ..] if *c1 > 2.0 * *c0 + 0.05 => FaceFate::Replaced { by: new.faces[*j].clone() },
                    many => FaceFate::Ambiguous { candidates: many.iter().take(4).map(|(j, _)| new.faces[*j].clone()).collect() },
                }
            }
        };
        let moved = match &fate {
            FaceFate::Same => new.faces.iter().find(|n| n.name == o.name).is_some_and(|n| changed_place(o, n)),
            FaceFate::Renamed { to } => changed_place(o, to),
            _ => false,
        };
        out.faces.push(FaceChange { old: o.clone(), fate, moved });
    }
    out.faces_added = new.faces.iter().enumerate().filter(|(j, _)| !taken[*j]).map(|(_, f)| f.clone()).filter(|f| !out.faces.iter().any(|c| matches!(&c.fate, FaceFate::Replaced { by } if by == f))).collect();
    out.bodies_removed = old.bodies.iter().filter(|b| !new.bodies.iter().any(|n| n.id == b.id || n.name == b.name)).cloned().collect();
    out.bodies_added = new.bodies.iter().filter(|n| !old.bodies.iter().any(|b| n.id == b.id || n.name == b.name)).cloned().collect();
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::edit::apply_op;
    use crate::kernel::implicit::ImplicitKernel;
    use crate::kernel::Kernel;
    use serde_json::json;

    fn plate() -> Document {
        let mut d = Document::new("plate");
        apply_op(&mut d, "sketch_extrude", &json!({"name": "Plate", "depth": 10, "sketch": {"plane": "XY", "items": [{"rectangle": {"center": [0, 0], "w": 40, "h": 40}}]}})).unwrap();
        apply_op(&mut d, "add", &json!({"kind": "hole", "name": "Bore", "diameter": 8, "depth": 20, "at": [0, 0, 12]})).unwrap();
        d
    }
    fn snap(d: &Document) -> GeometrySnapshot {
        GeometrySnapshot::of(&ImplicitKernel::new(64).evaluate(d).unwrap(), d)
    }

    #[test]
    fn nothing_changed_is_an_empty_diff_and_a_size_change_is_a_move() {
        let a = plate();
        let d = diff(&snap(&a), &snap(&a));
        assert!(d.is_empty(), "{}", d.describe());
        let mut b = a.clone();
        apply_op(&mut b, "set", &json!({"feature": "Bore", "path": "/shape/diameter", "value": 12})).unwrap();
        let d = diff(&snap(&a), &snap(&b));
        let bore = d.faces.iter().find(|f| f.old.name == "Bore side").unwrap();
        assert_eq!(bore.fate, FaceFate::Same);
        assert!(bore.moved, "the bore's wall grew: {}", d.describe());
    }

    #[test]
    fn a_renamed_feature_renames_its_faces_by_stable_id() {
        let a = plate();
        let mut b = a.clone();
        apply_op(&mut b, "rename", &json!({"feature": "Bore", "to": "Pin hole"})).unwrap();
        let d = diff(&snap(&a), &snap(&b));
        let c = d.faces.iter().find(|f| f.old.name == "Bore side").unwrap();
        match &c.fate {
            FaceFate::Renamed { to } => assert_eq!(to.name, "Pin hole side"),
            f => panic!("{f:?}: {}", d.describe()),
        }
    }

    #[test]
    fn a_removed_feature_removes_its_faces_and_a_replaced_one_names_its_successor() {
        let a = plate();
        let mut b = a.clone();
        apply_op(&mut b, "remove", &json!({"feature": "Bore"})).unwrap();
        let d = diff(&snap(&a), &snap(&b));
        assert_eq!(d.faces.iter().find(|f| f.old.name == "Bore side").unwrap().fate, FaceFate::Removed);
        assert!(d.faces.iter().filter(|f| f.old.name.starts_with("Plate")).all(|f| matches!(f.fate, FaceFate::Same)), "{}", d.describe());
        // Remove and add a hole of another name in the same place: the nearest successor.
        let mut c = a.clone();
        apply_op(&mut c, "remove", &json!({"feature": "Bore"})).unwrap();
        apply_op(&mut c, "add", &json!({"kind": "hole", "name": "Drill", "diameter": 8, "depth": 20, "at": [0, 0, 12]})).unwrap();
        let d = diff(&snap(&a), &snap(&c));
        match &d.faces.iter().find(|f| f.old.name == "Bore side").unwrap().fate {
            FaceFate::Replaced { by } => assert_eq!(by.name, "Drill side"),
            f => panic!("{f:?}"),
        }
        // Dimensions are tracked too.
        let mut e = a.clone();
        apply_op(&mut e, "parameter", &json!({"name": "w", "expr": 3})).unwrap();
        let d2 = diff(&snap(&e), &snap(&a));
        assert_eq!(d2.parameters_removed, vec!["w".to_string()]);
    }
}
