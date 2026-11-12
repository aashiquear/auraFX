//! Mates: relations between the parts of an assembly (two flat faces in one plane, two holes
//! on one axis, a distance, an angle, a part that stays put), solved into the parts'
//! placements whenever the assembly is built. The solver moves every part as little as it can
//! from where it is placed (a damped least-squares solve over each free part's position and
//! turn), so a part keeps the freedom its mates leave it, and a mate's distance or angle can
//! be changed to move the assembly.

use crate::document::{Document, Mate, MateKind, Placement};
use crate::sketch::{matrix_rank, solve_dense};
use glam::{DMat3, DQuat, DVec3, EulerRot};

/// A face as a mate sees it, in its own part's coordinates.
#[derive(Debug, Clone, Copy)]
pub struct MateFace {
    /// Flat (a point on it and its outward normal) or round (a point on its axis and the axis).
    pub flat: bool,
    pub point: DVec3,
    pub dir: DVec3,
}

/// How one mate came out.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct MateStatus {
    pub name: String,
    /// Why it is not met: a face that is gone, faces of the wrong kind, or mates that
    /// contradict each other.
    pub error: Option<String>,
}

/// The placements of the components with the mates applied.
#[derive(Debug, Clone)]
pub struct MateSolution {
    /// One per component, in the document's order.
    pub placements: Vec<Placement>,
    /// One per mate, in the document's order.
    pub status: Vec<MateStatus>,
    /// Ways the mated parts can still move (0: fully held).
    pub dof: usize,
    /// Every mate is met.
    pub satisfied: bool,
}

impl MateSolution {
    /// A sentence for the model tree and scripts.
    pub fn describe(&self) -> String {
        let bad = self.status.iter().filter(|s| s.error.is_some()).count();
        if bad > 0 {
            format!("{bad} mate(s) cannot be met")
        } else if self.dof == 0 {
            "fully held by its mates".to_string()
        } else {
            format!("{} way(s) left to move", self.dof)
        }
    }
}

fn rot(pl: &Placement) -> DMat3 {
    DMat3::from_quat(DQuat::from_euler(EulerRot::XYZ, pl.rotate_deg[0].to_radians(), pl.rotate_deg[1].to_radians(), pl.rotate_deg[2].to_radians()))
}

/// One mate ready for the solver: the two parts (None: the assembly itself, which does not
/// move) and the faces in their own coordinates.
struct Row {
    mate: usize,
    kind: MateKind,
    a: (Option<usize>, MateFace),
    b: (Option<usize>, MateFace),
    value: f64,
    flip: bool,
}

struct Problem {
    rows: Vec<Row>,
    /// Component index → its place among the unknowns (None: it does not move).
    slot: Vec<Option<usize>>,
    r0: Vec<DMat3>,
    t0: Vec<DVec3>,
    /// A length to compare distances with directions (mm).
    scale: f64,
}

impl Problem {
    fn pose(&self, c: Option<usize>, x: &[f64]) -> (DMat3, DVec3) {
        let Some(c) = c else { return (DMat3::IDENTITY, DVec3::ZERO) };
        match self.slot[c] {
            Some(k) => {
                let w = DVec3::new(x[6 * k + 3], x[6 * k + 4], x[6 * k + 5]);
                let dr = if w.length() > 0.0 { DMat3::from_axis_angle(w.normalize(), w.length()) } else { DMat3::IDENTITY };
                (dr * self.r0[c], self.t0[c] + DVec3::new(x[6 * k], x[6 * k + 1], x[6 * k + 2]))
            }
            None => (self.r0[c], self.t0[c]),
        }
    }

    /// The faces of a row in assembly coordinates: (point, direction) of each.
    fn world(&self, r: &Row, x: &[f64]) -> (DVec3, DVec3, DVec3, DVec3) {
        let (ra, ta) = self.pose(r.a.0, x);
        let (rb, tb) = self.pose(r.b.0, x);
        (ra * r.a.1.point + ta, (ra * r.a.1.dir).normalize_or(DVec3::Z), rb * r.b.1.point + tb, (rb * r.b.1.dir).normalize_or(DVec3::Z))
    }

    /// The residuals of one row; `angles` overrides the angle of angle mates (while stepping
    /// towards the one asked for).
    fn row(&self, r: &Row, x: &[f64], angle: Option<f64>, out: &mut Vec<f64>) {
        let (pa, da, pb, db) = self.world(r, x);
        let l = self.scale;
        // Faces that touch look at each other; `flip` has them look the same way.
        let facing = if r.flip { 1.0 } else { -1.0 };
        match r.kind {
            MateKind::Coincident | MateKind::Distance if r.a.1.flat => {
                out.extend((da * facing - db).to_array());
                let gap = if r.kind == MateKind::Distance { r.value } else { 0.0 };
                out.push(((pb - pa).dot(da) - gap) / l);
            }
            MateKind::Distance => {
                out.extend(da.cross(db).to_array());
                let d = pb - pa;
                out.push(((d - da * d.dot(da)).length() - r.value) / l);
            }
            MateKind::Concentric | MateKind::Coincident => {
                out.extend(da.cross(db).to_array());
                out.extend(((pb - pa).cross(da) / l).to_array());
            }
            MateKind::Angle => {
                let want = angle.unwrap_or(r.value).to_radians();
                // Between two faces 0 is how a coincident mate leaves them (looking at each other).
                let between = if r.a.1.flat && r.b.1.flat && !r.flip { std::f64::consts::PI - want } else { want };
                if between.sin().abs() < 1e-9 {
                    out.extend((da * between.cos() - db).to_array());
                } else {
                    out.push(da.cross(db).length().atan2(da.dot(db)) - between);
                }
            }
            MateKind::Fixed => {}
        }
    }

    fn residuals(&self, x: &[f64], angles: &[Option<f64>], out: &mut Vec<f64>) {
        out.clear();
        for (r, a) in self.rows.iter().zip(angles) {
            self.row(r, x, *a, out);
        }
    }

    fn jacobian(&self, x: &[f64], angles: &[Option<f64>], m: usize) -> Vec<f64> {
        let n = x.len();
        let mut j = vec![0.0; m * n];
        let (mut xp, mut rp, mut rm) = (x.to_vec(), Vec::new(), Vec::new());
        for k in 0..n {
            let h = 1e-6;
            xp[k] = x[k] + h;
            self.residuals(&xp, angles, &mut rp);
            xp[k] = x[k] - h;
            self.residuals(&xp, angles, &mut rm);
            xp[k] = x[k];
            for row in 0..m {
                j[row * n + k] = (rp[row] - rm[row]) / (2.0 * h);
            }
        }
        j
    }

    /// Damped least squares from `x`: the nearest arrangement that meets the mates.
    fn solve(&self, x: &mut Vec<f64>, angles: &[Option<f64>]) -> f64 {
        let n = x.len();
        let mut r = Vec::new();
        self.residuals(x, angles, &mut r);
        let m = r.len();
        let maxabs = |r: &[f64]| r.iter().fold(0.0f64, |a, v| a.max(v.abs()));
        let cost = |r: &[f64]| r.iter().map(|v| v * v).sum::<f64>();
        let (mut lambda, mut c0, mut rn) = (1e-4, cost(&r), Vec::new());
        for _ in 0..200 {
            if maxabs(&r) < 1e-11 || n == 0 {
                break;
            }
            let j = self.jacobian(x, angles, m);
            let (mut jtj, mut g) = (vec![0.0; n * n], vec![0.0; n]);
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
                    self.residuals(&xn, angles, &mut rn);
                    let c1 = cost(&rn);
                    if c1 < c0 {
                        *x = xn;
                        std::mem::swap(&mut r, &mut rn);
                        c0 = c1;
                        lambda = (lambda * 0.2).max(1e-12);
                        accepted = true;
                        break;
                    }
                }
                lambda *= 8.0;
            }
            if !accepted {
                break;
            }
        }
        maxabs(&r)
    }
}

/// `(point, normal)` of a principal plane of the assembly.
fn principal(face: &str) -> Option<MateFace> {
    let dir = match face.to_uppercase().as_str() {
        "XY" | "TOP" => DVec3::Z,
        "XZ" | "FRONT" => DVec3::NEG_Y,
        "YZ" | "RIGHT" => DVec3::X,
        _ => return None,
    };
    Some(MateFace { flat: true, point: DVec3::ZERO, dir })
}

/// Solve the mates of `doc`. `face(component, face name)` gives a face of a component in that
/// part's own coordinates (an empty component: the assembly's own part), or why it cannot.
pub fn solve(doc: &Document, face: &dyn Fn(&str, &str) -> Result<MateFace, String>) -> MateSolution {
    let nc = doc.components.len();
    let index = |name: &str| doc.components.iter().position(|c| c.name == name);
    let mut status: Vec<MateStatus> = doc.mates.iter().map(|m| MateStatus { name: m.name.clone(), error: None }).collect();
    let mut fixed = vec![false; nc];
    let mut rows: Vec<Row> = Vec::new();
    let side = |r: &crate::document::MateRef| -> Result<(Option<usize>, MateFace), String> {
        if r.component.is_empty() {
            return match principal(&r.face) {
                Some(f) => Ok((None, f)),
                None => face("", &r.face).map(|f| (None, f)),
            };
        }
        let c = index(&r.component).ok_or_else(|| format!("the assembly has no part '{}'", r.component))?;
        face(&r.component, &r.face).map(|f| (Some(c), f))
    };
    for (mi, m) in doc.mates.iter().enumerate() {
        if m.suppressed {
            continue;
        }
        if m.kind == MateKind::Fixed {
            match index(&m.a.component) {
                Some(c) => fixed[c] = true,
                None => status[mi].error = Some(format!("the assembly has no part '{}'", m.a.component)),
            }
            continue;
        }
        let made = (|| -> Result<Row, String> {
            let (a, b) = (side(&m.a)?, side(&m.b)?);
            if a.0 == b.0 {
                return Err(if a.0.is_none() { "neither face is on a part that can move".to_string() } else { "both faces are on the same part".to_string() });
            }
            let kinds = |f: &MateFace| if f.flat { "flat" } else { "round" };
            match m.kind {
                MateKind::Coincident if !(a.1.flat && b.1.flat) => {
                    return Err(format!(
                        "a coincident mate joins two flat faces ({} is {}, {} is {}); for holes and pins use concentric",
                        m.a.label(),
                        kinds(&a.1),
                        m.b.label(),
                        kinds(&b.1)
                    ))
                }
                MateKind::Concentric if a.1.flat || b.1.flat => {
                    return Err(format!("a concentric mate joins two round faces ({} is {}, {} is {})", m.a.label(), kinds(&a.1), m.b.label(), kinds(&b.1)))
                }
                MateKind::Distance if a.1.flat != b.1.flat => return Err("a distance is between two flat faces or between two round ones".to_string()),
                _ => {}
            }
            Ok(Row { mate: mi, kind: m.kind, a, b, value: m.value, flip: m.flip })
        })();
        match made {
            Ok(r) => rows.push(r),
            Err(e) => status[mi].error = Some(e),
        }
    }
    // The unknowns: every part a mate touches that is not fixed.
    let mut slot: Vec<Option<usize>> = vec![None; nc];
    let mut n = 0;
    for r in &rows {
        for c in [r.a.0, r.b.0].into_iter().flatten() {
            if !fixed[c] && slot[c].is_none() {
                slot[c] = Some(n);
                n += 1;
            }
        }
    }
    let r0: Vec<DMat3> = doc.components.iter().map(|c| rot(&c.placement)).collect();
    let t0: Vec<DVec3> = doc.components.iter().map(|c| DVec3::from_array(c.placement.translate)).collect();
    let scale = rows.iter().flat_map(|r| [r.a.1.point.length(), r.b.1.point.length(), r.value.abs()]).chain(t0.iter().map(|t| t.length())).fold(10.0, f64::max);
    let p = Problem { rows, slot, r0, t0, scale };
    let mut x = vec![0.0; 6 * n];
    let unchanged = MateSolution {
        placements: doc.components.iter().map(|c| c.placement.clone()).collect(),
        status: status.clone(),
        dof: 0,
        satisfied: status.iter().all(|s| s.error.is_none()),
    };
    if p.rows.is_empty() || n == 0 {
        // Nothing can move: say which mates are not met as the parts stand.
        let mut out = unchanged;
        let mut r = Vec::new();
        for row in &p.rows {
            r.clear();
            p.row(row, &x, None, &mut r);
            if r.iter().any(|v| v.abs() > 1e-6) {
                out.status[row.mate].error = Some("not met, and neither part can move (both are fixed)".to_string());
                out.satisfied = false;
            }
        }
        return out;
    }
    // An angle is reached in steps from the angle the parts make now, so the parts turn the
    // short way from where they are (and the same way every time).
    let start: Vec<Option<f64>> = p
        .rows
        .iter()
        .map(|r| {
            (r.kind == MateKind::Angle).then(|| {
                let (_, da, _, db) = p.world(r, &x);
                let now = da.cross(db).length().atan2(da.dot(db)).to_degrees();
                if r.a.1.flat && r.b.1.flat && !r.flip {
                    180.0 - now
                } else {
                    now
                }
            })
        })
        .collect();
    let steps = p.rows.iter().zip(&start).filter_map(|(r, s)| s.map(|s| ((r.value - s).abs() / 10.0).ceil() as usize)).max().unwrap_or(1).clamp(1, 36);
    // Parts exactly in line cannot tell which way to turn: nudge the moving one about an axis
    // across the faces (a hinge between the two parts gives that axis).
    for (r, s) in p.rows.iter().zip(&start) {
        let Some(s) = s else { continue };
        let (_, da, _, db) = p.world(r, &x);
        if da.cross(db).length() < 1e-7 && (r.value - s).abs() > 1e-9 {
            let hinge = p
                .rows
                .iter()
                .filter(|h| h.kind == MateKind::Concentric && ((h.a.0, h.b.0) == (r.a.0, r.b.0) || (h.a.0, h.b.0) == (r.b.0, r.a.0)))
                .map(|h| p.world(h, &x).1)
                .find(|ax| ax.cross(da).length() > 1e-3)
                .unwrap_or_else(|| da.any_orthonormal_vector());
            if let Some(k) = r.b.0.and_then(|c| p.slot[c]).or_else(|| r.a.0.and_then(|c| p.slot[c])) {
                // Of the two ways to turn, the one that opens the faces (takes them apart), not
                // the one that pushes one part into the other.
                let gap = |x: &[f64]| {
                    let (pa, da, pb, _) = p.world(r, x);
                    (pb - pa).dot(da)
                };
                let turned = |sign: f64| {
                    let mut y = x.clone();
                    let w = hinge.normalize() * 1e-3 * sign;
                    y[6 * k + 3] += w.x;
                    y[6 * k + 4] += w.y;
                    y[6 * k + 5] += w.z;
                    // The part turns about its own origin: about the hinge, when there is one, by solving it back on.
                    let hinges: Vec<Option<f64>> = vec![None; p.rows.len()];
                    let only_hinges = Problem {
                        rows: p
                            .rows
                            .iter()
                            .filter(|h| h.kind == MateKind::Concentric)
                            .map(|h| Row { mate: h.mate, kind: h.kind, a: h.a, b: h.b, value: h.value, flip: h.flip })
                            .collect(),
                        slot: p.slot.clone(),
                        r0: p.r0.clone(),
                        t0: p.t0.clone(),
                        scale: p.scale,
                    };
                    only_hinges.solve(&mut y, &hinges[..only_hinges.rows.len()]);
                    y
                };
                let (plus, minus) = (turned(1.0), turned(-1.0));
                x = if gap(&minus) > gap(&plus) + 1e-12 { minus } else { plus };
            }
        }
    }
    let mut worst = 0.0;
    for step in 1..=steps {
        let f = step as f64 / steps as f64;
        let angles: Vec<Option<f64>> = p.rows.iter().zip(&start).map(|(r, s)| s.map(|s| s + (r.value - s) * f)).collect();
        worst = p.solve(&mut x, &angles);
    }
    let none: Vec<Option<f64>> = vec![None; p.rows.len()];
    let mut r = Vec::new();
    p.residuals(&x, &none, &mut r);
    let m = r.len();
    let rank = if m > 0 { matrix_rank(&mut p.jacobian(&x, &none, m), m, x.len()) } else { 0 };
    let mut out = unchanged;
    out.dof = x.len() - rank.min(x.len());
    // Which mates are left unmet.
    if worst > 1e-7 {
        let mut one = Vec::new();
        for row in &p.rows {
            one.clear();
            p.row(row, &x, None, &mut one);
            let off = one.iter().fold(0.0f64, |a, v| a.max(v.abs()));
            if off > 1e-7 {
                out.status[row.mate].error = Some("cannot be met together with the other mates: they contradict each other (remove or change one)".to_string());
            }
        }
    }
    out.satisfied = out.status.iter().all(|s| s.error.is_none());
    for (c, slot) in p.slot.iter().enumerate() {
        if slot.is_some() {
            let (rm, t) = p.pose(Some(c), &x);
            let (a, b, g) = DQuat::from_mat3(&rm).to_euler(EulerRot::XYZ);
            let clean = |v: f64| if v.abs() < 1e-9 { 0.0 } else { v };
            out.placements[c] = Placement { translate: t.to_array().map(clean), rotate_deg: [a.to_degrees(), b.to_degrees(), g.to_degrees()].map(clean) };
        }
    }
    out
}

/// A mate from an operation's arguments is checked against the parts: a short description of it.
pub fn describe(m: &Mate) -> String {
    match m.kind {
        MateKind::Fixed => format!("{} stays in place", m.a.component),
        k if k.has_value() => format!("{} {} between {} and {}", k.label(), m.value, m.a.label(), m.b.label()),
        k => format!("{} of {} and {}", k.label(), m.a.label(), m.b.label()),
    }
}
