//! Ported from EZ-YX-craft (`ezyx-cad`, MIT OR Apache-2.0), the user's own project.
//!
//! Parameters, configurations and bindings: turning a parametric [`Document`] into concrete values.

use crate::document::{Document, Parameter, Shape};
use crate::expr::{self, ExprError};
use crate::CadError;
use std::collections::BTreeMap;

impl Document {
    /// Set (or add) parameter `name` to `expr`.
    pub fn set_parameter(&mut self, name: &str, expr: &str) {
        match self.parameters.iter_mut().find(|p| p.name == name) {
            Some(p) => p.expr = expr.to_string(),
            None => self.parameters.push(Parameter::new(name, expr)),
        }
    }

    /// Builder form of [`Document::set_parameter`].
    pub fn param(mut self, name: &str, expr: &str) -> Self {
        self.set_parameter(name, expr);
        self
    }

    /// Evaluate all parameters (active configuration overrides applied) in dependency order.
    pub fn parameter_values(&self) -> Result<BTreeMap<String, f64>, CadError> {
        let mut exprs: BTreeMap<&str, &str> = self.parameters.iter().map(|p| (p.name.as_str(), p.expr.as_str())).collect();
        if let Some(name) = &self.active_configuration {
            let cfg = self.configurations.iter().find(|c| &c.name == name).ok_or_else(|| CadError::UnknownConfiguration(name.clone()))?;
            for o in &cfg.overrides {
                exprs.insert(&o.name, &o.expr);
            }
        }
        let mut done: BTreeMap<String, f64> = BTreeMap::new();
        let mut stack: Vec<&str> = Vec::new();
        fn visit<'a>(n: &'a str, exprs: &BTreeMap<&'a str, &'a str>, done: &mut BTreeMap<String, f64>, stack: &mut Vec<&'a str>) -> Result<(), CadError> {
            if done.contains_key(n) {
                return Ok(());
            }
            if let Some(k) = stack.iter().position(|s| *s == n) {
                let mut cyc: Vec<&str> = stack[k..].to_vec();
                cyc.push(n);
                return Err(CadError::Cycle(cyc.join(" → ")));
            }
            let e = exprs[n];
            stack.push(n);
            for dep in expr::identifiers(e) {
                if let Some((k, _)) = exprs.get_key_value(dep.as_str()) {
                    visit(k, exprs, done, stack)?;
                }
            }
            stack.pop();
            let v = expr::eval(e, &|id| done.get(id).copied()).map_err(|source| CadError::Expr { name: n.to_string(), source })?;
            done.insert(n.to_string(), v);
            Ok(())
        }
        for n in exprs.keys() {
            visit(n, &exprs, &mut done, &mut stack)?;
        }
        Ok(done)
    }

    /// True when [`Document::resolved`] can change anything (bindings or constrained sketches present).
    pub fn needs_resolution(&self) -> bool {
        self.features.iter().any(|f| {
            !f.bindings.is_empty()
                || matches!(f.shape, Shape::SketchExtrude { .. } | Shape::SketchRevolve { .. } | Shape::SketchSweep { .. } | Shape::SketchLoft { .. })
        })
    }

    /// A copy with every binding evaluated and written into its feature, parameter-driven sketch
    /// dimensions evaluated and every sketch solved.
    pub fn resolved(&self) -> Result<Document, CadError> {
        let vals = self.parameter_values()?;
        let vars = |n: &str| vals.get(n).copied();
        let mut out = self.clone();
        // The sketches kept with the part (profiles, paths, sections) follow the parameters too.
        for s in &mut out.sketches {
            let report = s.sketch.solve_with(&vals).map_err(|source: ExprError| CadError::Expr { name: format!("sketch {}", s.name), source })?;
            if !report.converged && report.max_residual > 1e-6 {
                return Err(CadError::Edit(format!(
                    "the sketch '{}' cannot be solved: its constraints and dimensions contradict each other (off by {:.3})",
                    s.name, report.max_residual
                )));
            }
        }
        for m in &mut out.mates {
            if let Some(e) = &m.expr {
                m.value = expr::eval(e, &vars).map_err(|source| CadError::Expr { name: format!("mate {}", m.name), source })?;
            }
        }
        for i in 0..out.features.len() {
            let f = &mut out.features[i];
            // A switched-off feature is not built: a problem in it must not stop the part.
            if f.suppressed {
                continue;
            }
            if !f.bindings.is_empty() {
                let err = |path: &str, reason: String| CadError::Binding { feature: f.name.clone(), path: path.to_string(), reason };
                let mut v = serde_json::to_value(&*f).map_err(|e| err("", e.to_string()))?;
                for b in &f.bindings {
                    let x = expr::eval(&b.expr, &vars).map_err(|source| CadError::Expr { name: format!("{}{}", f.name, b.path), source })?;
                    let slot = v.pointer_mut(&b.path).ok_or_else(|| err(&b.path, "no such field".into()))?;
                    *slot = if slot.is_u64() {
                        serde_json::json!(x.round().max(0.0) as u64)
                    } else if slot.is_i64() {
                        serde_json::json!(x.round() as i64)
                    } else if slot.is_number() {
                        serde_json::json!(x)
                    } else {
                        return Err(err(&b.path, format!("not a number: {slot}")));
                    };
                }
                let name = f.name.clone();
                *f = serde_json::from_value(v).map_err(|e| CadError::Binding { feature: name, path: String::new(), reason: e.to_string() })?;
            }
            if let Some(sk) = f.shape.sketch_mut() {
                let report = sk.solve_with(&vals).map_err(|source: ExprError| CadError::Expr { name: format!("{} sketch", f.name), source })?;
                // Constraints or dimensions that contradict each other leave the outline wherever
                // the solver stopped: refuse, instead of building a shape nobody asked for.
                if !report.converged && report.max_residual > 1e-6 {
                    return Err(CadError::Edit(format!(
                        "the sketch of '{}' cannot be solved: its constraints and dimensions contradict each other (off by {:.3}); edit the sketch or the dimension changed last",
                        f.name, report.max_residual
                    )));
                }
            }
            // A sketch drawn on a face goes where that face is now; sweeps and lofts drawn as
            // sketches become the outlines the kernels build.
            crate::swept::follow_face(&mut out, i)?;
            let mut f = out.features[i].clone();
            crate::swept::lower(&out, &mut f)?;
            out.features[i] = f;
        }
        Ok(out)
    }

    /// Stable 64-bit hash (FNV-1a) of the part's JSON — cheap change detection / cache keys.
    pub fn revision_hash(&self) -> u64 {
        let json = serde_json::to_vec(self).unwrap_or_default();
        json.iter().fold(0xcbf2_9ce4_8422_2325u64, |h, b| (h ^ *b as u64).wrapping_mul(0x0100_0000_01b3))
    }
}

#[cfg(test)]
mod tests {
    use crate::document::*;
    use crate::CadError;
    use crate::ExprError;

    fn box_part() -> Document {
        Document::new("p").param("width", "80mm").param("depth", "width / 2").param("height", "1in").add(
            Feature::new("b", BooleanOp::Add, Shape::Box { size: [1.0, 1.0, 1.0], corner_radius: 0.0 })
                .bind("/shape/size/0", "width")
                .bind("/shape/size/1", "depth")
                .bind("/shape/size/2", "height")
                .bind("/placement/translate/2", "height / 2"),
        )
    }

    #[test]
    fn parameters_in_dependency_order() {
        let p = Document::new("p").param("c", "a + b").param("b", "2 * a").param("a", "3cm");
        let v = p.parameter_values().unwrap();
        assert_eq!(v["a"], 30.0);
        assert_eq!(v["b"], 60.0);
        assert_eq!(v["c"], 90.0);
    }

    #[test]
    fn cycles_and_unknowns_are_errors() {
        let p = Document::new("p").param("a", "b + 1").param("b", "a * 2");
        assert!(matches!(p.parameter_values(), Err(CadError::Cycle(_))));
        let p = Document::new("p").param("a", "nope + 1");
        assert!(matches!(p.parameter_values(), Err(CadError::Expr { source: ExprError::UnknownIdentifier(_), .. })));
        let mut p = Document::new("p").param("a", "1");
        p.active_configuration = Some("missing".into());
        assert!(matches!(p.parameter_values(), Err(CadError::UnknownConfiguration(_))));
    }

    #[test]
    fn bindings_drive_geometry_and_configurations_override() {
        let mut p = box_part();
        let r = p.resolved().unwrap();
        let Shape::Box { size, .. } = &r.features[0].shape else { panic!() };
        assert_eq!(*size, [80.0, 40.0, 25.4]);
        assert!((r.features[0].placement.translate[2] - 12.7).abs() < 1e-12);
        let v = p.mesh(1.0).volume() as f64;
        assert!((v - 80.0 * 40.0 * 25.4).abs() / (80.0 * 40.0 * 25.4) < 0.03, "{v}");

        p.configurations.push(Configuration { name: "small".into(), overrides: vec![Parameter::new("width", "40")] });
        p.active_configuration = Some("small".into());
        let v = p.parameter_values().unwrap();
        assert_eq!((v["width"], v["depth"]), (40.0, 20.0));
        let (lo, hi) = p.sdf().unwrap().bounds();
        assert!(((hi.x - lo.x) - 40.0).abs() < 1e-9 && ((hi.y - lo.y) - 20.0).abs() < 1e-9);

        p.active_configuration = None;
        p.set_parameter("height", "10");
        let (lo, hi) = p.sdf().unwrap().bounds();
        assert!(((hi.z - lo.z) - 10.0).abs() < 1e-9 && lo.z.abs() < 1e-9);
    }

    #[test]
    fn integer_bindings_and_bad_paths() {
        let p = Document::new("p").param("n", "2.6").add(
            Feature::new("h", BooleanOp::Add, Shape::Cylinder { radius: 1.0, height: 1.0 })
                .with(Modifier::LinearPattern { count: 1, step: [5.0, 0.0, 0.0] })
                .bind("/modifiers/0/count", "n"),
        );
        let r = p.resolved().unwrap();
        assert_eq!(r.features[0].modifiers[0], Modifier::LinearPattern { count: 3, step: [5.0, 0.0, 0.0] });
        let mut bad = p.clone();
        bad.features[0].bindings[0].path = "/shape/nope".into();
        assert!(matches!(bad.resolved(), Err(CadError::Binding { .. })));
        // Fallback: the raw part still builds an SDF.
        assert!(bad.sdf().is_some());
    }

    #[test]
    fn revision_hash_tracks_changes() {
        let a = box_part();
        let mut b = a.clone();
        assert_eq!(a.revision_hash(), b.revision_hash());
        b.set_parameter("width", "81");
        assert_ne!(a.revision_hash(), b.revision_hash());
    }
}
