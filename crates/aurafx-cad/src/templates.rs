//! Ported from EZ-YX-craft (`ezyx-cad`, MIT OR Apache-2.0), the user's own project.
//!
//! Starter parts for the Solid Modeler's "New part" gallery. Each shows off a few features and
//! is parameter-driven where that makes sense (edit the parameters, the part follows).

use crate::sketch::{Constraint, Dim, Sketch, SketchPlane};
use crate::document::*;
use crate::path::shapes;
use glam::DVec2;

/// `(title, part)` pairs.
pub fn templates() -> Vec<(&'static str, Document)> {
    vec![
        ("Enclosure (shell + standoffs + vents, parameter-driven width/height)", enclosure()),
        ("Flange with bolt circle (circular pattern holes, counterbore)", flange()),
        ("Knob", demo_part()),
        ("Threaded rod", threaded_rod()),
        ("Lofted vase", vase()),
        ("L-bracket (constrained sketch)", bracket()),
    ]
}

/// Open-top box shell with four drilled standoffs and a row of vent slots.
pub fn enclosure() -> Document {
    let inset = "(width/2 - wall - 5)";
    Document::new("Enclosure")
        .param("width", "80mm")
        .param("depth", "50mm")
        .param("height", "30mm")
        .param("wall", "2mm")
        .param("boss_d", "6mm")
        .add(
            Feature::new("Shell", BooleanOp::Add, Shape::Box { size: [80.0, 50.0, 30.0], corner_radius: 3.0 })
                .with(Modifier::ShellOpen { thickness: 2.0, open_top: true })
                .bind("/shape/size/0", "width")
                .bind("/shape/size/1", "depth")
                .bind("/shape/size/2", "height")
                .bind("/placement/translate/2", "height/2")
                .bind("/modifiers/0/thickness", "wall"),
        )
        .add(
            Feature::new("Standoffs", BooleanOp::Add, Shape::Cylinder { radius: 3.0, height: 12.0 })
                .with(Modifier::GridPattern { counts: [2, 2], step: [60.0, 30.0] })
                .blended(1.0)
                .bind("/shape/radius", "boss_d/2")
                .bind("/shape/height", "height*0.4")
                .bind("/placement/translate/0", format!("-{inset}"))
                .bind("/placement/translate/1", "-(depth/2 - wall - 5)")
                .bind("/placement/translate/2", "wall/2")
                .bind("/modifiers/0/step/0", format!("2*{inset}"))
                .bind("/modifiers/0/step/1", "2*(depth/2 - wall - 5)"),
        )
        .add(
            Feature::new("Screw holes", BooleanOp::Cut, Shape::Hole { kind: HoleKind::Drilled, diameter: 2.5, depth: 9.0 })
                .with(Modifier::GridPattern { counts: [2, 2], step: [60.0, 30.0] })
                .bind("/shape/depth", "height*0.4 - 3")
                .bind("/placement/translate/0", format!("-{inset}"))
                .bind("/placement/translate/1", "-(depth/2 - wall - 5)")
                .bind("/placement/translate/2", "wall/2 + height*0.4")
                .bind("/modifiers/0/step/0", format!("2*{inset}"))
                .bind("/modifiers/0/step/1", "2*(depth/2 - wall - 5)"),
        )
        .add(
            Feature::new("Vents", BooleanOp::Cut, Shape::Box { size: [8.0, 2.0, 10.0], corner_radius: 0.9 })
                .with(Modifier::LinearPattern { count: 5, step: [0.0, 5.0, 0.0] })
                .bind("/placement/translate/0", "width/2")
                .bind("/placement/translate/1", "-10")
                .bind("/placement/translate/2", "height*0.55")
                .bind("/shape/size/2", "height*0.33"),
        )
}

/// Disc with a hub and bore and a counterbored bolt circle.
pub fn flange() -> Document {
    Document::new("Flange")
        .param("od", "100mm")
        .param("thickness", "12mm")
        .param("hub_d", "40mm")
        .param("bore", "20mm")
        .param("bolt_circle", "75mm")
        .param("holes", "6")
        .param("bolt_d", "8.5mm")
        .add(
            Feature::new("Disc", BooleanOp::Add, Shape::Cylinder { radius: 50.0, height: 12.0 })
                .bind("/shape/radius", "od/2")
                .bind("/shape/height", "thickness"),
        )
        .add(
            Feature::new("Hub", BooleanOp::Add, Shape::Cylinder { radius: 20.0, height: 27.0 })
                .blended(3.0)
                .bind("/shape/radius", "hub_d/2")
                .bind("/shape/height", "thickness + 15"),
        )
        .add(
            Feature::new("Bore", BooleanOp::Cut, Shape::Hole { kind: HoleKind::Simple, diameter: 20.0, depth: 40.0 })
                .chamfered(1.0)
                .bind("/shape/diameter", "bore")
                .bind("/shape/depth", "thickness + 20")
                .bind("/placement/translate/2", "thickness + 15"),
        )
        .add(
            Feature::new("Bolt holes", BooleanOp::Cut, Shape::Hole { kind: HoleKind::Counterbore { diameter: 14.0, depth: 4.0 }, diameter: 8.5, depth: 20.0 })
                .with(Modifier::CircularPatternAngle { count: 6, total_deg: 360.0, axis: Axis::Z })
                .bind("/shape/diameter", "bolt_d")
                .bind("/shape/hole/diameter", "bolt_d * 1.65")
                .bind("/placement/translate/0", "bolt_circle/2")
                .bind("/placement/translate/2", "thickness")
                .bind("/modifiers/0/count", "holes"),
        )
}

/// M10 × 1.5 hex bolt: modelled thread and a chamfered hexagon head.
pub fn threaded_rod() -> Document {
    let af = 16.0; // across flats
    let rc = af / 3f64.sqrt();
    let hex: Vec<[f64; 2]> = (0..6).map(|i| (i as f64 * 60.0f64).to_radians()).map(|a| [rc * a.cos(), rc * a.sin()]).collect();
    Document::new("Threaded rod")
        .param("d", "10mm")
        .param("pitch", "1.5mm")
        .param("len", "30mm")
        .add(
            Feature::new("Thread", BooleanOp::Add, Shape::Thread { major_diameter: 10.0, pitch: 1.5, length: 30.0, internal: false, depth: 0.0 })
                .bind("/shape/major_diameter", "d")
                .bind("/shape/pitch", "pitch")
                .bind("/shape/length", "len"),
        )
        .add(
            Feature::new(
                "Head",
                BooleanOp::Add,
                Shape::SketchExtrude {
                    sketch: Sketch::polygon(&hex).on(SketchPlane::XY, 30.0),
                    depth: 6.4,
                    end: EndCondition::Blind,
                    draft_deg: 0.0,
                    top_edge: EdgeTreatment::Chamfer { distance: 1.0 },
                    bottom_edge: EdgeTreatment::None,
                    reverse: false,
                },
            )
            .bind("/shape/sketch/offset", "len - 0.5"),
        )
}

/// Circle → octagon → wider circle loft, shelled with an open top.
pub fn vase() -> Document {
    let c = |r: f64| vec![shapes::circle(DVec2::ZERO, r)];
    Document::new("Lofted vase").add(
        Feature::new(
            "Body",
            BooleanOp::Add,
            Shape::Loft {
                sections: vec![
                    LoftSection { profile: c(25.0), z: 0.0 },
                    LoftSection { profile: vec![shapes::polygon(DVec2::ZERO, 20.0, 8, 0.0)], z: 40.0 },
                    LoftSection { profile: c(30.0), z: 90.0 },
                ],
            },
        )
        .with(Modifier::ShellOpen { thickness: 2.5, open_top: true }),
    )
}

/// L-bracket from a fully-defined constrained sketch on the front plane, with two mounting holes.
pub fn bracket() -> Document {
    let mut s = Sketch::new(SketchPlane::XZ);
    let pts = [[0.0, 0.0], [60.0, 0.0], [60.0, 6.0], [6.0, 6.0], [6.0, 40.0], [0.0, 40.0]];
    let ids: Vec<usize> = pts.iter().map(|p| s.add_point(p[0], p[1])).collect();
    s.constrain(Constraint::Fix(ids[0]));
    let l: Vec<usize> = (0..6).map(|i| s.add_line(ids[i], ids[(i + 1) % 6])).collect();
    for (i, e) in l.iter().enumerate() {
        s.constrain(if i % 2 == 0 { Constraint::Horizontal(*e) } else { Constraint::Vertical(*e) });
    }
    s.constrain(Constraint::Length(l[0], Dim::expr("length")));
    s.constrain(Constraint::Length(l[5], Dim::expr("height")));
    s.constrain(Constraint::Length(l[1], Dim::expr("t")));
    s.constrain(Constraint::Length(l[4], Dim::expr("t")));
    Document::new("L-bracket")
        .param("length", "60mm")
        .param("height", "40mm")
        .param("t", "6mm")
        .param("width", "30mm")
        .add(
            Feature::new(
                "Bracket",
                BooleanOp::Add,
                Shape::SketchExtrude {
                    sketch: s,
                    depth: 30.0,
                    end: EndCondition::MidPlane,
                    draft_deg: 0.0,
                    top_edge: EdgeTreatment::Chamfer { distance: 0.5 },
                    bottom_edge: EdgeTreatment::Chamfer { distance: 0.5 },
                    reverse: false,
                },
            )
            .bind("/shape/depth", "width"),
        )
        .add(
            Feature::new(
                "Mount holes",
                BooleanOp::Cut,
                Shape::Hole { kind: HoleKind::Countersink { diameter: 10.0, angle_deg: 90.0 }, diameter: 5.0, depth: 10.0 },
            )
            .with(Modifier::GridPattern { counts: [1, 2], step: [0.0, 14.0] })
            .at(40.0, -7.0, 6.0)
            .bind("/placement/translate/0", "length*2/3")
            .bind("/placement/translate/2", "t"),
        )
}

#[cfg(test)]
mod tests {
    use super::*;
    use glam::DVec3;

    #[test]
    fn all_templates_resolve_and_mesh() {
        for (name, p) in templates() {
            p.resolved().unwrap_or_else(|e| panic!("{name}: {e}"));
            let m = p.mesh(2.0);
            assert!(!m.is_empty(), "{name}");
            assert!(m.volume() > 100.0, "{name}: {}", m.volume());
            let back = Document::from_json(&p.to_json()).unwrap();
            assert_eq!(back.features.len(), p.features.len(), "{name} JSON round trip");
            assert_eq!(back.parameters, p.parameters);
            let (a, b) = (p.sdf().unwrap(), back.sdf().unwrap());
            let q = DVec3::new(1.3, -2.1, 4.7);
            assert!((a.eval(q) - b.eval(q)).abs() < 1e-6, "{name}");
        }
    }

    #[test]
    fn enclosure_is_open_and_parameter_driven() {
        let mut p = enclosure();
        let s = p.sdf().unwrap();
        // Inside the cavity, just below the rim: empty. Floor and walls: solid.
        assert!(s.eval(DVec3::new(0.0, 0.0, 29.5)) > 0.0, "open top");
        assert!(s.eval(DVec3::new(0.0, 0.0, 1.0)) < 0.0, "floor");
        assert!(s.eval(DVec3::new(39.0, 20.0, 15.0)) < 0.0 || s.eval(DVec3::new(-39.0, 0.0, 15.0)) < 0.0, "wall");
        p.set_parameter("width", "120");
        let (lo, hi) = p.sdf().unwrap().bounds();
        assert!(((hi.x - lo.x) - 120.0).abs() <= 2.0 + 1e-9, "{}", hi.x - lo.x); // blend pads bounds by 1 mm
        assert!(p.sdf().unwrap().eval(DVec3::new(-59.0, 0.0, 15.0)) < 0.0, "wall moved with width");
    }

    #[test]
    fn flange_has_bolt_circle() {
        let s = flange().sdf().unwrap();
        // Bolt hole centres on the 75 mm circle every 60°; material between them.
        for k in 0..6 {
            let a = (k as f64 * 60.0f64).to_radians();
            assert!(s.eval(DVec3::new(37.5 * a.cos(), 37.5 * a.sin(), 6.0)) > 0.0, "hole {k}");
            let b = a + 30f64.to_radians();
            assert!(s.eval(DVec3::new(37.5 * b.cos(), 37.5 * b.sin(), 6.0)) < 0.0, "web {k}");
        }
        // Counterbore: wider near the top face.
        assert!(s.eval(DVec3::new(37.5 + 5.5, 0.0, 11.0)) > 0.0 && s.eval(DVec3::new(37.5 + 5.5, 0.0, 5.0)) < 0.0);
        // Bore through the hub.
        assert!(s.eval(DVec3::new(0.0, 0.0, 20.0)) > 0.0 && s.eval(DVec3::new(15.0, 0.0, 20.0)) < 0.0);
    }
}
