//! Ported from EZ-YX-craft (`ezyx-cad`, MIT OR Apache-2.0), the user's own project.
//!
//! Assemblies (lite): placed part instances, combined meshing, interference and mass properties.
//!
//! Components are rigidly placed with a [`Placement`]. Mates/joints, motion studies and exploded
//! views are out of scope for now; the GUI positions components directly. The combined solid is
//! what gets exported into the CNC pipeline (heightmap or mesh), so an assembly can be carved as a
//! single relief or used to check fits before cutting.

use crate::analysis::{interference_sdf, MassProperties};
use crate::document::{Document, Placement};
use crate::kernel::sdf::Sdf;
use crate::mesh::{mt, TriMesh};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Component {
    pub name: String,
    pub part: Document,
    #[serde(default)]
    pub placement: Placement,
    /// Grounded component (informational until mates exist).
    #[serde(default)]
    pub fixed: bool,
    #[serde(default = "yes")]
    pub visible: bool,
}

fn yes() -> bool {
    true
}

impl Component {
    pub fn new(name: impl Into<String>, part: Document) -> Self {
        Self { name: name.into(), part, placement: Placement::default(), fixed: false, visible: true }
    }
    pub fn at(mut self, x: f64, y: f64, z: f64) -> Self {
        self.placement.translate = [x, y, z];
        self
    }
    pub fn rotated(mut self, rx: f64, ry: f64, rz: f64) -> Self {
        self.placement.rotate_deg = [rx, ry, rz];
        self
    }
    /// The part's SDF in assembly coordinates.
    pub fn sdf(&self) -> Option<Sdf> {
        let s = self.part.sdf()?;
        Some(Sdf::Transform { inner: Box::new(s), translate: self.placement.translate, rotate_deg: self.placement.rotate_deg })
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct Assembly {
    pub name: String,
    #[serde(default)]
    pub components: Vec<Component>,
}

impl Assembly {
    pub fn new(name: impl Into<String>) -> Self {
        Self { name: name.into(), components: vec![] }
    }
    #[allow(clippy::should_implement_trait)]
    pub fn add(mut self, c: Component) -> Self {
        self.components.push(c);
        self
    }

    /// Union of all visible components.
    pub fn combined_sdf(&self) -> Option<Sdf> {
        self.components.iter().filter(|c| c.visible).filter_map(|c| c.sdf()).reduce(|a, b| Sdf::Union(Box::new(a), Box::new(b), 0.0))
    }

    /// Triangulate the combined solid at resolution `h` (mm).
    pub fn mesh(&self, h: f64) -> TriMesh {
        let Some(s) = self.combined_sdf() else { return TriMesh::default() };
        let (lo, hi) = s.bounds();
        let g = mt::Grid::sample(lo.as_vec3(), hi.as_vec3(), h as f32, |p| s.eval(p.as_dvec3()) as f32);
        let mut m = mt::extract(&g, 0.0);
        m.smooth_taubin(2, 0.4, -0.42);
        m
    }

    /// Pairs of visible components that overlap, with the overlap volume (mm³) at resolution `h`.
    pub fn interference_report(&self, h: f64) -> Vec<(usize, usize, f64)> {
        let sdfs: Vec<Option<Sdf>> = self.components.iter().map(|c| if c.visible { c.sdf() } else { None }).collect();
        let mut out = Vec::new();
        for i in 0..sdfs.len() {
            for j in i + 1..sdfs.len() {
                if let (Some(a), Some(b)) = (&sdfs[i], &sdfs[j]) {
                    let v = interference_sdf(a, b, h);
                    if v > 0.0 {
                        out.push((i, j, v));
                    }
                }
            }
        }
        out
    }

    /// Mass properties of the combined solid (overlaps counted once).
    pub fn mass_properties(&self, h: f64, density_g_cm3: f64) -> MassProperties {
        MassProperties::from_mesh(&self.mesh(h), density_g_cm3)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::*;

    fn cube(s: f64) -> Document {
        Document::new("cube").add(Feature::new("c", BooleanOp::Add, Shape::Box { size: [s; 3], corner_radius: 0.0 }))
    }

    #[test]
    fn assembly_interference_and_mass() {
        let asm = Assembly::new("a")
            .add(Component::new("base", cube(10.0)))
            .add(Component::new("overlapping", cube(10.0)).at(6.0, 0.0, 0.0))
            .add(Component::new("separate", cube(10.0)).at(40.0, 0.0, 0.0));
        let rep = asm.interference_report(0.5);
        assert_eq!(rep.len(), 1);
        let (i, j, v) = rep[0];
        assert_eq!((i, j), (0, 1));
        assert!((v - 400.0).abs() / 400.0 < 0.08, "{v}");
        let mp = asm.mass_properties(0.5, 1.0);
        let expect = 1000.0 + 1000.0 + 1000.0 - 400.0;
        assert!((mp.volume_mm3 - expect).abs() / expect < 0.04, "{}", mp.volume_mm3);
        let json = serde_json::to_string(&asm).unwrap();
        let back: Assembly = serde_json::from_str(&json).unwrap();
        assert_eq!(back, asm);
        let mut hidden = asm.clone();
        hidden.components[1].visible = false;
        assert!(hidden.interference_report(0.5).is_empty());
    }
}
