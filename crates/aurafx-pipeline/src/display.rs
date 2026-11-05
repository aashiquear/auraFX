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
    /// Edited color map control points; when set they replace the named preset.
    pub colormap_stops: Option<Vec<(f32, [f32; 3])>>,
    /// Run the color map backwards.
    pub colormap_reverse: bool,
    /// Number of flat color bands (0 = continuous).
    pub colormap_bands: u32,
    /// Curved (higher-order) cells: 0 = straight through the corners, 1 = through the nodes,
    /// each further level halves the pieces (ParaView's "Nonlinear Subdivision Level").
    pub nonlinear_subdivision: u32,
    /// For objects on a server: draw them there (images come back) or here (geometry comes back).
    pub draw_on: DrawOn,
}

/// Where an object on a server is drawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum DrawOn {
    /// Here, unless even its surface is too big to transfer.
    #[default]
    Auto,
    /// Always here (its surface comes over, however big).
    Local,
    /// Always on the server (only images come over).
    Server,
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
            colormap_stops: None,
            colormap_reverse: false,
            colormap_bands: 0,
            nonlinear_subdivision: 1,
            draw_on: DrawOn::Auto,
        }
    }
}

impl DisplayProps {
    /// The color map actually used: the preset or edited stops, reversed and banded as asked.
    pub fn effective_colormap(&self) -> crate::colormap::ColorMap {
        let mut m = match &self.colormap_stops {
            Some(stops) if stops.len() >= 2 => crate::colormap::ColorMap::new(self.colormap.clone(), stops.clone()),
            _ => crate::colormap::get(&self.colormap),
        };
        if self.colormap_reverse {
            m = m.reversed();
        }
        if self.colormap_bands >= 2 {
            m = m.banded(self.colormap_bands);
        }
        m
    }

    /// Changes whenever the effective color map does (for caching lookup textures).
    pub fn colormap_key(&self) -> String {
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        for (t, c) in self.effective_colormap().stops {
            t.to_bits().hash(&mut h);
            c.map(|x| x.to_bits()).hash(&mut h);
        }
        format!("{}#{:x}", self.colormap, h.finish())
    }
}

/// A directional light of the 3D views.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Light {
    /// Direction towards the light. With `camera_relative`, in the camera's frame (x right,
    /// y up, z towards the viewer), so the light moves with the view; else in world coordinates.
    pub direction: [f32; 3],
    pub color: [f32; 3],
    pub intensity: f32,
    pub camera_relative: bool,
    /// Casts shadows (when shadows are on; the first such light is used).
    pub shadows: bool,
}

impl Default for Light {
    fn default() -> Self {
        Self { direction: [0.3, 0.5, 1.0], color: [1.0, 1.0, 1.0], intensity: 0.6, camera_relative: true, shadows: true }
    }
}

impl Light {
    /// ParaView-like light kit: key light (upper right), fill (lower left), head light.
    pub fn kit() -> Vec<Light> {
        vec![
            Light::default(),
            Light { direction: [-0.6, -0.3, 1.0], intensity: 0.25, shadows: false, ..Default::default() },
            Light { direction: [0.0, 0.0, 1.0], intensity: 0.2, shadows: false, ..Default::default() },
        ]
    }

    /// The single light of earlier versions.
    pub fn single() -> Vec<Light> {
        vec![Light { intensity: 0.7, ..Default::default() }]
    }
}

/// Tone mapping of the final picture.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ToneMapping {
    #[default]
    None,
    Reinhard,
    Aces,
}

/// Lighting and screen effects of the 3D views.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Lighting {
    /// Key, fill and head lights (else one light). Ignored when `lights` is set.
    pub light_kit: bool,
    /// Custom lights (empty: the light kit or the single light).
    pub lights: Vec<Light>,
    /// Ambient light (None: 0.15 with several lights, 0.3 with one).
    pub ambient: Option<f32>,
    /// The first light with `shadows` casts shadows.
    pub shadows: bool,
    /// Eye-Dome Lighting: shade by depth differences (point clouds, dense meshes).
    pub edl: bool,
    pub edl_strength: f32,
    /// Screen-space ambient occlusion: darken creases and cavities.
    pub ssao: bool,
    pub ssao_strength: f32,
    /// Occlusion radius as a fraction of the size of the visible objects.
    pub ssao_radius: f32,
    /// Fast approximate anti-aliasing of the final picture.
    pub fxaa: bool,
    pub tone_mapping: ToneMapping,
    pub exposure: f32,
}

impl Default for Lighting {
    fn default() -> Self {
        Self {
            light_kit: true,
            lights: Vec::new(),
            ambient: None,
            shadows: false,
            edl: false,
            edl_strength: 1.0,
            ssao: false,
            ssao_strength: 1.0,
            ssao_radius: 0.06,
            fxaa: false,
            tone_mapping: ToneMapping::None,
            exposure: 1.0,
        }
    }
}

impl Lighting {
    /// The lights in effect and the ambient term.
    pub fn effective_lights(&self) -> (Vec<Light>, f32) {
        let lights = if !self.lights.is_empty() {
            self.lights.clone()
        } else if self.light_kit {
            Light::kit()
        } else {
            Light::single()
        };
        let ambient = self.ambient.unwrap_or(if lights.len() > 1 { 0.15 } else { 0.3 });
        (lights, ambient)
    }

    /// What a server applies to the pictures it draws: lights and shadows only (the screen
    /// effects run here, over the composited picture).
    pub fn for_server(&self) -> Lighting {
        Lighting {
            light_kit: self.light_kit,
            lights: self.lights.clone(),
            ambient: self.ambient,
            shadows: self.shadows,
            ..Lighting { edl: false, ssao: false, fxaa: false, tone_mapping: ToneMapping::None, exposure: 1.0, ..Default::default() }
        }
    }
}
