//! Vector pictures (SVG): the visible geometry as flat-shaded polygons sorted back to front,
//! and the overlays (color legend, labels, axes) from the same egui shapes as on screen.

use aurafx_render::scene_export::VectorPrim;
use egui::epaint::Shape;
use std::fmt::Write as _;

fn hex(c: [u8; 3]) -> String {
    format!("#{:02x}{:02x}{:02x}", c[0], c[1], c[2])
}

fn color32(c: egui::Color32) -> (String, f32) {
    let [r, g, b, a] = c.to_srgba_unmultiplied();
    (hex([r, g, b]), a as f32 / 255.0)
}

fn esc(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

/// Append egui shapes (in points) as SVG, scaled by `k` pixels per point.
fn shapes(out: &mut String, shapes: &[Shape], k: f32) {
    let p = |p: egui::Pos2| format!("{:.2} {:.2}", p.x * k, p.y * k);
    for s in shapes {
        match s {
            Shape::Vec(v) => self::shapes(out, v, k),
            Shape::LineSegment { points, stroke } => {
                let (c, a) = color32(stroke.color);
                let _ = writeln!(
                    out,
                    r#"<line x1="{:.2}" y1="{:.2}" x2="{:.2}" y2="{:.2}" stroke="{c}" stroke-opacity="{a:.3}" stroke-width="{:.2}"/>"#,
                    points[0].x * k,
                    points[0].y * k,
                    points[1].x * k,
                    points[1].y * k,
                    stroke.width * k
                );
            }
            Shape::Rect(r) => {
                let rr = r.rect;
                let (fc, fa) = color32(r.fill);
                let (sc, sa) = color32(r.stroke.color);
                let radius = r.corner_radius.nw as f32 * k;
                let _ = writeln!(
                    out,
                    r#"<rect x="{:.2}" y="{:.2}" width="{:.2}" height="{:.2}" rx="{radius:.2}" fill="{fc}" fill-opacity="{fa:.3}" stroke="{sc}" stroke-opacity="{sa:.3}" stroke-width="{:.2}"/>"#,
                    rr.min.x * k,
                    rr.min.y * k,
                    rr.width() * k,
                    rr.height() * k,
                    r.stroke.width * k
                );
            }
            Shape::Circle(c) => {
                let (fc, fa) = color32(c.fill);
                let (sc, sa) = color32(c.stroke.color);
                let _ = writeln!(
                    out,
                    r#"<circle cx="{:.2}" cy="{:.2}" r="{:.2}" fill="{fc}" fill-opacity="{fa:.3}" stroke="{sc}" stroke-opacity="{sa:.3}" stroke-width="{:.2}"/>"#,
                    c.center.x * k,
                    c.center.y * k,
                    c.radius * k,
                    c.stroke.width * k
                );
            }
            Shape::Path(path) => {
                let d: Vec<String> = path.points.iter().map(|q| p(*q)).collect();
                let (fc, fa) = color32(path.fill);
                let stroke_color = match &path.stroke.color {
                    egui::epaint::ColorMode::Solid(c) => *c,
                    _ => egui::Color32::TRANSPARENT,
                };
                let (sc, sa) = color32(stroke_color);
                let _ = writeln!(
                    out,
                    r#"<path d="M {}{}" fill="{fc}" fill-opacity="{fa:.3}" stroke="{sc}" stroke-opacity="{sa:.3}" stroke-width="{:.2}"/>"#,
                    d.join(" L "),
                    if path.closed { " Z" } else { "" },
                    path.stroke.width * k
                );
            }
            Shape::Text(t) => {
                let g = &t.galley;
                let (size, color) = g.job.sections.first().map(|s| (s.format.font_id.size, s.format.color)).unwrap_or((12.0, t.fallback_color));
                let color = t.override_text_color.unwrap_or(if color == egui::Color32::PLACEHOLDER { t.fallback_color } else { color });
                let (c, a) = color32(color);
                for row in &g.rows {
                    let text: String = row.glyphs.iter().map(|gl| gl.chr).collect();
                    if text.trim().is_empty() {
                        continue;
                    }
                    let x = t.pos.x + row.pos.x;
                    // Baseline: the row's top plus the font's ascent (about 0.8 em).
                    let y = t.pos.y + row.pos.y + size * 0.8;
                    let _ = writeln!(
                        out,
                        r#"<text x="{:.2}" y="{:.2}" font-family="Ubuntu, 'Noto Sans', 'DejaVu Sans', sans-serif" font-size="{:.2}" fill="{c}" fill-opacity="{a:.3}">{}</text>"#,
                        x * k,
                        y * k,
                        size * k,
                        esc(&text)
                    );
                }
            }
            _ => {}
        }
    }
}

/// Write an SVG picture of `w`×`h` pixels.
pub fn write(
    path: &std::path::Path,
    prims: &[VectorPrim],
    w: u32,
    h: u32,
    background: Option<[f32; 3]>,
    overlays: Option<(&[Shape], f32)>,
) -> anyhow::Result<()> {
    let mut out = String::new();
    let _ = writeln!(out, r#"<?xml version="1.0" encoding="UTF-8"?>"#);
    let _ = writeln!(out, r#"<svg xmlns="http://www.w3.org/2000/svg" width="{w}" height="{h}" viewBox="0 0 {w} {h}">"#);
    let _ = writeln!(out, "<!-- auraFX {} -->", env!("CARGO_PKG_VERSION"));
    if let Some(b) = background {
        let c = [(b[0] * 255.0) as u8, (b[1] * 255.0) as u8, (b[2] * 255.0) as u8];
        let _ = writeln!(out, r#"<rect width="{w}" height="{h}" fill="{}"/>"#, hex(c));
    }
    let _ = writeln!(out, r#"<g stroke-linejoin="round" stroke-linecap="round">"#);
    for p in prims {
        let col = hex(p.color);
        let op = if p.alpha < 1.0 { format!(r#" opacity="{:.3}""#, p.alpha) } else { String::new() };
        match p.points.len() {
            1 => {
                let _ = writeln!(out, r#"<circle cx="{:.2}" cy="{:.2}" r="{:.2}" fill="{col}"{op}/>"#, p.points[0][0], p.points[0][1], p.size.max(0.5));
            }
            2 => {
                let _ = writeln!(
                    out,
                    r#"<line x1="{:.2}" y1="{:.2}" x2="{:.2}" y2="{:.2}" stroke="{col}" stroke-width="{:.2}"{op}/>"#,
                    p.points[0][0], p.points[0][1], p.points[1][0], p.points[1][1], p.size
                );
            }
            _ => {
                // A thin outline in the same color hides hairline seams between triangles.
                let d: Vec<String> = p.points.iter().map(|q| format!("{:.2} {:.2}", q[0], q[1])).collect();
                let _ = writeln!(out, r#"<path d="M {} Z" fill="{col}" stroke="{col}" stroke-width="0.5"{op}/>"#, d.join(" L "));
            }
        }
    }
    let _ = writeln!(out, "</g>");
    if let Some((s, k)) = overlays {
        let _ = writeln!(out, "<g>");
        shapes(&mut out, s, k);
        let _ = writeln!(out, "</g>");
    }
    let _ = writeln!(out, "</svg>");
    std::fs::write(path, out)?;
    Ok(())
}
