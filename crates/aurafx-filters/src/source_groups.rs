//! The sources gathered in groups: what the Sources menu shows as one entry each, with the
//! types of a group listed in the window that adds one. A source outside every group is an
//! entry of its own. Python and the assistant call every source by its own name whatever its group.

/// A group of sources: its name in commands, its label in the menu, what it holds, and its
/// types in families (a family without a name is listed without a heading).
pub struct SourceGroup {
    pub id: &'static str,
    pub label: &'static str,
    pub hint: &'static str,
    pub families: &'static [(&'static str, &'static [&'static str])],
}

pub const MICROSTRUCTURE: &str = "synthetic_microstructure";

pub static SOURCE_GROUPS: &[SourceGroup] = &[
    SourceGroup {
        id: "geometric_shape",
        label: "Geometric Shape",
        hint: "A sphere, cone, cylinder, box, plane, line or cloud of points, as a surface.",
        families: &[("Solids", &["sphere", "box", "cylinder", "cone"]), ("Flat and thin", &["plane", "line", "point_source"])],
    },
    SourceGroup {
        id: "solid_model",
        label: "Solid Model",
        hint: "Solids with named faces: built from geometry commands, or a parametric part with sketches and features.",
        families: &[("", &["geometry", "cad_model"])],
    },
    SourceGroup {
        id: "mesh",
        label: "Mesh",
        hint: "Meshes ready for a solver: a box, a plate with a hole, a dog-bone specimen, a block with inclusions, or a face drawn with outline commands.",
        families: &[("Blocks and specimens", &["box_mesh", "plate_mesh", "dogbone_mesh", "inclusion_mesh"]), ("Drawn outlines", &["face_mesh"])],
    },
    SourceGroup {
        id: "sample_data",
        label: "Sample Data",
        hint: "Data with fields to try filters on: an image, an unstructured grid of every cell type, a grid in time, adaptive grids.",
        families: &[
            ("Images", &["wavelet"]),
            ("Unstructured grids", &["unstructured_cell_types", "cell_grid"]),
            ("In time", &["spatio_temporal_harmonics_grid"]),
            ("Adaptive grids", &["amr_gaussian_pulse", "hyper_tree_grid", "random_hyper_tree_grid"]),
        ],
    },
    SourceGroup {
        id: MICROSTRUCTURE,
        label: "Synthetic Microstructure",
        hint: "Grains, particles, fibres, random fields, lattices, foams, growths and bio-inspired structures, each with its standard settings.",
        families: crate::morphology::FAMILIES,
    },
];

impl SourceGroup {
    pub fn ids(&self) -> impl Iterator<Item = &'static str> + '_ {
        self.families.iter().flat_map(|f| f.1.iter().copied())
    }
    pub fn len(&self) -> usize {
        self.ids().count()
    }
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// The group with this name.
pub fn source_group(id: &str) -> Option<&'static SourceGroup> {
    SOURCE_GROUPS.iter().find(|g| g.id == id)
}

/// The group a source is listed in.
pub fn group_of_source(id: &str) -> Option<&'static SourceGroup> {
    SOURCE_GROUPS.iter().find(|g| g.ids().any(|i| i == id))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_grouped_source_exists_once() {
        let mut seen = Vec::new();
        for g in SOURCE_GROUPS {
            assert!(crate::registry::find(g.id).is_none(), "{} is also a source", g.id);
            for id in g.ids() {
                let def = crate::registry::find(id).unwrap_or_else(|| panic!("no source {id}"));
                assert!(def.is_source, "{id}");
                assert!(!seen.contains(&id), "{id} in two groups");
                seen.push(id);
            }
        }
    }
}
