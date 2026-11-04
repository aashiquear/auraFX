//! Per-node display properties.

use aurafx_filters::Association;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum Representation {
    #[default]
    Surface,
    SurfaceWithEdges,
    Wireframe,
    Points,
    Outline,
    /// GPU ray-cast volume (image data directly; other meshes are resampled).
    Volume,
}

impl Representation {
    pub const ALL: [Representation; 6] = [
        Representation::Surface,
        Representation::SurfaceWithEdges,
        Representation::Wireframe,
        Representation::Points,
        Representation::Outline,
        Representation::Volume,
    ];
    pub fn label(self) -> &'static str {
        match self {
            Representation::Surface => "Surface",
            Representation::SurfaceWithEdges => "Surface With Edges",
            Representation::Wireframe => "Wireframe",
            Representation::Points => "Points",
            Representation::Outline => "Outline",
            Representation::Volume => "Volume",
        }
    }
    pub fn parse(s: &str) -> Option<Self> {
        let l = s.to_ascii_lowercase().replace(['_', '-'], " ");
        Self::ALL.into_iter().find(|r| r.label().to_ascii_lowercase() == l)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ColorBy {
    pub assoc: Association,
    pub name: String,
    /// `None` = magnitude for vectors.
    pub component: Option<usize>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct DisplayProps {
    pub representation: Representation,
    /// `None` = solid color.
    pub color_by: Option<ColorBy>,
    pub colormap: String,
    /// `None` = automatic from data.
    pub range: Option<(f64, f64)>,
    pub log_scale: bool,
    pub solid_color: [f32; 3],
    pub opacity: f32,
    pub point_size: f32,
    pub points_as_spheres: bool,
    pub line_width: f32,
    pub show_scalar_bar: bool,
    pub lighting: bool,
    pub edge_color: [f32; 3],
    /// Volume rendering: overall opacity scale (higher = more opaque).
    pub volume_opacity: f32,
    /// Volume rendering: values below this fraction of the range are fully transparent.
    pub volume_threshold: f32,
    /// Draw a labelled axes grid around the object's bounds.
    pub axes_grid: bool,
}

impl Default for DisplayProps {
    fn default() -> Self {
        Self {
            representation: Representation::Surface,
            color_by: None,
            colormap: "Viridis".into(),
            range: None,
            log_scale: false,
            solid_color: [0.85, 0.85, 0.85],
            opacity: 1.0,
            point_size: 4.0,
            points_as_spheres: true,
            line_width: 1.0,
            show_scalar_bar: true,
            lighting: true,
            edge_color: [0.0, 0.0, 0.0],
            volume_opacity: 1.0,
            volume_threshold: 0.0,
            axes_grid: false,
        }
    }
}
