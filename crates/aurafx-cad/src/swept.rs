//! What [`Document::resolved`] does beyond numbers: sweeps and lofts drawn as sketches become
//! the outlines and paths the kernels build, and a sketch placed on a face of the part is put
//! back on that face as the part before it is built now.

use crate::document::{Document, Feature, LoftSection, Placement, Shape};
use crate::path::Polyline;
use crate::sketch::Sketch;
use crate::CadError;
use glam::{DMat3, DQuat, DVec2, DVec3, EulerRot};
use std::collections::HashMap;
use std::sync::Mutex;

fn bad<T>(m: impl Into<String>) -> Result<T, CadError> {
    Err(CadError::Edit(m.into()))
}

fn saved<'a>(doc: &'a Document, feature: &str, role: &str, name: &str) -> Result<&'a Sketch, CadError> {
    doc.saved_sketch(name).ok_or_else(|| {
        let have: Vec<&str> = doc.sketches.iter().map(|s| s.name.as_str()).collect();
        CadError::Edit(format!(
            "'{feature}' uses the sketch '{name}' as its {role}, and the part has no sketch of that name (its sketches are: {}): choose another in the feature panel",
            if have.is_empty() { "none".to_string() } else { have.join(", ") }
        ))
    })
}

/// The start frame of a sweep along `path`, as the kernels take it: the profile's x and y axes.
pub fn sweep_frame(path: &[DVec3]) -> Option<(DVec3, DVec3)> {
    let t0 = (*path.get(1)? - path[0]).normalize_or_zero();
    if t0 == DVec3::ZERO {
        return None;
    }
    let up = if t0.z.abs() < 0.999 { DVec3::Z } else { DVec3::Y };
    let b = (up - t0 * t0.dot(up)).normalize();
    Some((b.cross(t0), b))
}

/// The outlines and the path of a sweep drawn as two sketches.
pub fn sweep_geometry(doc: &Document, feature: &str, profile: &str, path: &str) -> Result<(Vec<Polyline>, Vec<[f64; 3]>), CadError> {
    if profile == path {
        return bad(format!("'{feature}' uses the sketch '{profile}' as both its profile and its path: a sweep needs two sketches"));
    }
    let (ps, qs) = (saved(doc, feature, "profile", profile)?, saved(doc, feature, "path", path)?);
    let open = qs.open_paths();
    let Some(line) = open.iter().max_by(|a, b| a.length().total_cmp(&b.length())) else {
        return bad(format!("'{feature}': the path sketch '{path}' has no open path (draw a line, or lines and arcs joined end to end, that does not close)"));
    };
    let pts: Vec<DVec3> = line.pts.iter().map(|p| qs.to_world(*p)).collect();
    let loops = ps.profiles();
    if loops.is_empty() {
        return bad(format!("'{feature}': the profile sketch '{profile}' has no closed outline"));
    }
    let Some((n, b)) = sweep_frame(&pts) else {
        return bad(format!("'{feature}': the path sketch '{path}' is too short"));
    };
    let (_, normal) = (ps.placement_frame().1, ps.placement_frame().0.z_axis);
    let t0 = (pts[1] - pts[0]).normalize();
    if normal.dot(t0).abs() < 0.2 {
        return bad(format!(
            "'{feature}': the profile sketch '{profile}' lies along the start of the path '{path}'; draw the profile on a plane the path leaves (at right angles is best)"
        ));
    }
    let o = pts[0];
    let outlines = loops
        .iter()
        .map(|l| {
            Polyline::closed(
                l.pts
                    .iter()
                    .map(|q| {
                        let w = ps.to_world(*q) - o;
                        DVec2::new(w.dot(n), w.dot(b))
                    })
                    .collect(),
            )
        })
        .collect();
    Ok((outlines, pts.iter().map(|p| p.to_array()).collect()))
}

/// The sections of a loft drawn as sketches on parallel planes, in the frame of the first, and
/// that frame (rotation and origin).
pub fn loft_geometry(doc: &Document, feature: &str, sections: &[String]) -> Result<(Vec<LoftSection>, DMat3, DVec3), CadError> {
    if sections.len() < 2 {
        return bad(format!("'{feature}' is a loft with {} section(s): it needs two or more sketches on different planes", sections.len()));
    }
    let first = saved(doc, feature, "first section", &sections[0])?;
    let (r0, o0) = first.placement_frame();
    let mut out: Vec<LoftSection> = Vec::new();
    for name in sections {
        let sk = saved(doc, feature, "section", name)?;
        let (r, _) = sk.placement_frame();
        if r.z_axis.dot(r0.z_axis).abs() < 1.0 - 1e-6 {
            return bad(format!(
                "'{feature}': the section '{name}' is not parallel to '{}' (the sections of a loft are drawn on parallel planes)",
                sections[0]
            ));
        }
        let loops = sk.profiles();
        if loops.is_empty() {
            return bad(format!("'{feature}': the section sketch '{name}' has no closed outline"));
        }
        let z = (sk.to_world(DVec2::ZERO) - o0).dot(r0.z_axis);
        if let Some(same) = out.iter().position(|s| (s.z - z).abs() < 1e-6) {
            return bad(format!(
                "'{feature}': the sections '{}' and '{name}' are on the same plane; move one (its sketch's offset) so they are apart",
                sections[same]
            ));
        }
        let profile = loops
            .iter()
            .map(|l| {
                Polyline::closed(
                    l.pts
                        .iter()
                        .map(|q| {
                            let w = sk.to_world(*q) - o0;
                            DVec2::new(w.dot(r0.x_axis), w.dot(r0.y_axis))
                        })
                        .collect(),
                )
            })
            .collect();
        out.push(LoftSection { profile, z });
    }
    Ok((out, r0, o0))
}

fn rotation(pl: &Placement) -> DMat3 {
    DMat3::from_quat(DQuat::from_euler(EulerRot::XYZ, pl.rotate_deg[0].to_radians(), pl.rotate_deg[1].to_radians(), pl.rotate_deg[2].to_radians()))
}

/// A sweep or a loft drawn as sketches turned into the shape the kernels build (other features
/// are left as they are).
pub(crate) fn lower(doc: &Document, f: &mut Feature) -> Result<(), CadError> {
    match &f.shape {
        Shape::SketchSweep { profile, path, twist_deg } => {
            let (outlines, pts) = sweep_geometry(doc, &f.name, profile, path)?;
            f.shape = Shape::Sweep { profile: outlines, path: pts, twist_deg: *twist_deg };
        }
        Shape::SketchLoft { sections } => {
            let (secs, r0, o0) = loft_geometry(doc, &f.name, sections)?;
            // The sections are in the first sketch's frame: the feature's own placement comes on top.
            let rf = rotation(&f.placement);
            let (r, t) = (rf * r0, rf * o0 + DVec3::from_array(f.placement.translate));
            let (a, b, c) = DQuat::from_mat3(&r).to_euler(EulerRot::XYZ);
            f.placement = Placement { translate: t.to_array(), rotate_deg: [a.to_degrees(), b.to_degrees(), c.to_degrees()] };
            f.shape = Shape::Loft { sections: secs };
        }
        _ => {}
    }
    Ok(())
}

// ------------------------------------------------------------------------- sketches on faces

type Plane = Result<([f64; 3], [f64; 3]), String>;

static FACE_PLANES: Mutex<Option<HashMap<u64, Plane>>> = Mutex::new(None);

/// The plane of the face `face` of the part `prefix` builds (a point on it and its outward
/// direction). `prefix` is resolved already. Results are remembered by the part's content, so a
/// part whose early features do not change is not rebuilt for this again.
pub fn face_plane(prefix: &Document, face: &str) -> Plane {
    let mut h = prefix.revision_hash();
    for b in face.bytes() {
        h = (h ^ b as u64).wrapping_mul(0x0100_0000_01b3);
    }
    if let Some(p) = FACE_PLANES.lock().ok().and_then(|c| c.as_ref().and_then(|m| m.get(&h).cloned())) {
        return p;
    }
    let found = (|| -> Plane {
        if !prefix.features.iter().any(|f| !f.suppressed) {
            return Err(format!("the part has no face '{face}' (nothing is built before this feature)"));
        }
        let ev =
            crate::kernel::Kernel::evaluate(&crate::kernel::implicit::ImplicitKernel { resolution: 40, project: true }, prefix).map_err(|e| e.to_string())?;
        let g = crate::analysis::face_geometry(&ev, face).ok_or_else(|| {
            let mut names: Vec<&str> = ev.faces().map(|(_, f)| f.name.as_str()).collect();
            names.truncate(12);
            format!("the part has no face '{face}' at that point of its history (its faces there: {})", names.join(", "))
        })?;
        if !g.flat {
            return Err(format!("the face '{face}' is not flat"));
        }
        Ok((g.point, g.dir))
    })();
    if let Ok(mut c) = FACE_PLANES.lock() {
        let m = c.get_or_insert_with(HashMap::new);
        if m.len() > 512 {
            m.clear();
        }
        m.insert(h, found.clone());
    }
    found
}

/// The part as built before feature `i` of `doc` (whose earlier features are resolved), on its own.
fn prefix(doc: &Document, i: usize) -> Document {
    let mut p = doc.clone();
    p.features.truncate(i);
    p.finish.clear();
    p.components.clear();
    p.mates.clear();
    p.configurations.clear();
    p.active_configuration = None;
    for f in &mut p.features {
        f.bindings.clear();
        if let Some(sk) = f.shape.sketch_mut() {
            sk.face = None;
        }
    }
    p
}

/// The plane of the face `face` of the part as it is built before feature `i` of `doc` (the
/// whole part when `i` is the number of its features).
pub fn face_plane_before(doc: &Document, i: usize, face: &str) -> Result<([f64; 3], [f64; 3]), CadError> {
    let mut pre = doc.clone();
    pre.features.truncate(i);
    let pre = pre.resolved()?;
    face_plane(&prefix(&pre, i), face).map_err(|why| CadError::Edit(format!("a sketch cannot go on '{face}': {why}")))
}

/// Put the sketch of feature `i` back on the face it was drawn on.
pub(crate) fn follow_face(doc: &mut Document, i: usize) -> Result<(), CadError> {
    let Some(face) = doc.features[i].shape.sketch().and_then(|s| s.face.clone()) else { return Ok(()) };
    let name = doc.features[i].name.clone();
    match face_plane(&prefix(doc, i), &face) {
        Ok((p, n)) => {
            if let Some(sk) = doc.features[i].shape.sketch_mut() {
                sk.follow_face(p, n);
            }
            Ok(())
        }
        Err(why) => bad(format!(
            "the sketch of '{name}' sits on the face '{face}', and {why}. Right-click '{name}' in the model tree and pick another face for its sketch, or detach it there"
        )),
    }
}
