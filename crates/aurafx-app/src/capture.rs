//! Pictures of the open views as they are laid out: every visible 3D view and chart view in one
//! image, each where it sits in the window (File › Save Screenshot › All views, animations of
//! all views, `afx.screenshot(path, views="all")`).
//!
//! The picture is taken from the window itself: the panes' contents are cut out of a screenshot
//! of one frame and placed on a plain background, so tab bars, panels and menus are left out.
//! The chart controls are hidden in the frame that is captured.

use crate::app::{Pane, State};
use egui::{Color32, ColorImage, Rect};
use std::path::PathBuf;
use std::sync::mpsc::{Receiver, Sender};
use std::sync::Arc;

/// Which views a layout picture shows.
#[derive(Debug, Clone, PartialEq)]
pub enum Views {
    /// Every visible 3D view and chart view.
    All,
    /// Every visible chart view.
    Charts,
    /// Every visible 3D view.
    Views3d,
    /// One pane.
    One(Pane),
}

impl Views {
    /// "all", "charts", "3d", "chart" (the Charts panel), "chart N", "view N" (3D view N; 0 = the first).
    pub fn parse(s: &str) -> Option<Views> {
        let s = s.trim().to_ascii_lowercase();
        let (word, num) = match s.split_once(' ') {
            Some((w, n)) => (w.to_string(), n.trim().parse::<u32>().ok()),
            None => (s.clone(), None),
        };
        Some(match (word.as_str(), num) {
            ("all", None) => Views::All,
            ("charts", None) => Views::Charts,
            ("3d" | "views", None) => Views::Views3d,
            ("chart", None) | ("chart", Some(0) | Some(1)) => Views::One(Pane::Charts),
            // Chart views are titled from 2 ("Chart 2"), their numbers start at 1.
            ("chart", Some(n)) => Views::One(Pane::Chart(n - 1)),
            ("view", Some(0) | Some(1)) | ("view", None) => Views::One(Pane::Viewport),
            ("view", Some(n)) => Views::One(Pane::View(n - 1)),
            _ => return None,
        })
    }

    fn wants(&self, p: Pane) -> bool {
        match self {
            Views::All => is_view(p),
            Views::Charts => matches!(p, Pane::Charts | Pane::Chart(_)),
            Views::Views3d => matches!(p, Pane::Viewport | Pane::View(_)),
            Views::One(q) => *q == p,
        }
    }
}

/// Panes that are views (pictures of data) rather than tools.
pub fn is_view(p: Pane) -> bool {
    matches!(p, Pane::Viewport | Pane::View(_) | Pane::Charts | Pane::Chart(_))
}

/// Who waits for a layout picture.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Done {
    /// A message in the window.
    Toast,
    /// A script: the result goes to `Session::screenshot_results` with this id.
    Script(u64),
    /// A frame of an animation being recorded.
    Animation,
}

#[derive(Debug, Clone)]
pub struct Request {
    pub path: PathBuf,
    pub views: Views,
    pub transparent: bool,
    /// Keep the chart views' controls (their boxes and buttons) in the picture.
    pub controls: bool,
    pub done: Done,
}

#[derive(Default)]
pub struct Capture {
    queue: Vec<Request>,
    /// The request taken in this frame (its frame is the one captured).
    armed: Option<Request>,
    /// Frames to wait before arming (scripts: let the data and app commands land).
    wait: u8,
    inflight: Option<(Request, Vec<Rect>, Rect, Receiver<Arc<ColorImage>>)>,
    /// The result of the last animation frame.
    pub animation_result: Option<Result<(), String>>,
    /// The panes drawn this frame and where (refreshed every frame).
    pub pane_rects: Vec<(Pane, Rect)>,
}

impl Capture {
    pub fn request(&mut self, r: Request) {
        if matches!(r.done, Done::Script(_)) {
            self.wait = self.wait.max(2);
        }
        self.queue.push(r);
    }

    /// Hide the chart controls and the 3D views' tools in this frame (a picture without them
    /// is being taken). The window's screenshot may be of the frame after the request, so they
    /// stay hidden until the picture has arrived.
    pub fn hide_controls(&self) -> bool {
        self.armed.as_ref().is_some_and(|r| !r.controls) || self.inflight.as_ref().is_some_and(|f| !f.0.controls)
    }

    pub fn busy(&self) -> bool {
        !self.queue.is_empty() || self.armed.is_some() || self.inflight.is_some()
    }
}

impl State {
    /// Before the panes are drawn: forget last frame's pane positions, take the next request.
    pub fn capture_begin_frame(&mut self) {
        self.capture.pane_rects.clear();
        let c = &mut self.capture;
        if c.armed.is_none() && c.inflight.is_none() && !c.queue.is_empty() {
            let busy = self.session.lock().is_busy();
            if busy {
                return;
            }
            if c.wait > 0 {
                c.wait -= 1;
                return;
            }
            c.armed = Some(c.queue.remove(0));
        }
    }

    /// After the panes are drawn: send the screenshot for this frame, finish pictures that arrived.
    pub fn capture_end_frame(&mut self, ctx: &egui::Context) {
        if self.capture.busy() {
            ctx.request_repaint();
        }
        if let Some(req) = self.capture.armed.take() {
            let rects: Vec<Rect> = self.capture.pane_rects.iter().filter(|(p, _)| req.views.wants(*p)).map(|(_, r)| *r).collect();
            if rects.is_empty() {
                self.capture_finished(&req, Err(format!("no {} is open and visible", describe(&req.views))));
            } else {
                let (tx, rx): (Sender<Arc<ColorImage>>, Receiver<Arc<ColorImage>>) = std::sync::mpsc::channel();
                let wake = ctx.clone();
                ctx.request_screenshot(move |img| {
                    let _ = tx.send(img);
                    wake.request_repaint();
                });
                self.capture.inflight = Some((req, rects, ctx.viewport_rect(), rx));
            }
        }
        let got = self.capture.inflight.as_ref().and_then(|(_, _, _, rx)| rx.try_recv().ok());
        if let Some(img) = got {
            let (req, rects, screen, _) = self.capture.inflight.take().unwrap();
            let bg = if req.transparent { Color32::TRANSPARENT } else { ctx.global_style().visuals.panel_fill };
            let out = compose(&img, screen, &rects, bg);
            let r = out.save(&req.path).map_err(|e| format!("cannot write {}: {e}", req.path.display()));
            self.capture_finished(&req, r);
        }
    }

    fn capture_finished(&mut self, req: &Request, r: Result<(), String>) {
        match req.done {
            Done::Toast => match &r {
                Ok(()) => self.toast(format!("Saved {}", req.path.display()), false),
                Err(e) => self.toast(format!("Screenshot failed: {e}"), true),
            },
            Done::Script(id) => self.session.lock().screenshot_results.push((id, r)),
            Done::Animation => self.capture.animation_result = Some(r),
        }
    }

    /// Requests from scripts (`afx.screenshot(path, views=...)`).
    pub fn capture_poll_session(&mut self) {
        let req = self.session.lock().layout_request.take();
        if let Some(r) = req {
            match Views::parse(&r.views) {
                Some(views) => {
                    self.capture.request(Request { path: r.path, views, transparent: r.transparent, controls: r.controls, done: Done::Script(r.id) })
                }
                None => self
                    .session
                    .lock()
                    .screenshot_results
                    .push((r.id, Err(format!("views must be 'all', 'charts', '3d', 'chart', 'chart N' or 'view N', not {:?}", r.views)))),
            }
        }
    }
}

fn describe(v: &Views) -> String {
    match v {
        Views::All => "3D view or chart view".into(),
        Views::Charts => "chart view".into(),
        Views::Views3d => "3D view".into(),
        Views::One(p) => format!("{p:?} pane"),
    }
}

/// Cut `rects` (in points, inside `screen`) out of the window picture and place them, at their
/// places, on a background as large as the box around them.
pub fn compose(img: &ColorImage, screen: Rect, rects: &[Rect], bg: Color32) -> image::RgbaImage {
    let sx = img.size[0] as f32 / screen.width().max(1.0);
    let sy = img.size[1] as f32 / screen.height().max(1.0);
    let px = |r: Rect| -> [u32; 4] {
        let x0 = ((r.min.x - screen.min.x) * sx).round().clamp(0.0, img.size[0] as f32) as u32;
        let y0 = ((r.min.y - screen.min.y) * sy).round().clamp(0.0, img.size[1] as f32) as u32;
        let x1 = ((r.max.x - screen.min.x) * sx).round().clamp(0.0, img.size[0] as f32) as u32;
        let y1 = ((r.max.y - screen.min.y) * sy).round().clamp(0.0, img.size[1] as f32) as u32;
        [x0, y0, x1.max(x0), y1.max(y0)]
    };
    let boxes: Vec<[u32; 4]> = rects.iter().map(|r| px(*r)).filter(|b| b[2] > b[0] && b[3] > b[1]).collect();
    if boxes.is_empty() {
        return image::RgbaImage::new(1, 1);
    }
    let ux0 = boxes.iter().map(|b| b[0]).min().unwrap();
    let uy0 = boxes.iter().map(|b| b[1]).min().unwrap();
    let ux1 = boxes.iter().map(|b| b[2]).max().unwrap();
    let uy1 = boxes.iter().map(|b| b[3]).max().unwrap();
    let bgc = bg.to_srgba_unmultiplied();
    let mut out = image::RgbaImage::from_pixel(ux1 - ux0, uy1 - uy0, image::Rgba(bgc));
    for b in &boxes {
        for y in b[1]..b[3] {
            for x in b[0]..b[2] {
                let c = img.pixels[(y as usize) * img.size[0] + x as usize].to_srgba_unmultiplied();
                out.put_pixel(x - ux0, y - uy0, image::Rgba([c[0], c[1], c[2], 255]));
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_views() {
        assert_eq!(Views::parse("all"), Some(Views::All));
        assert_eq!(Views::parse("Charts"), Some(Views::Charts));
        assert_eq!(Views::parse("chart"), Some(Views::One(Pane::Charts)));
        assert_eq!(Views::parse("chart 3"), Some(Views::One(Pane::Chart(2))));
        assert_eq!(Views::parse("view 2"), Some(Views::One(Pane::View(1))));
        assert_eq!(Views::parse("3d"), Some(Views::Views3d));
        assert_eq!(Views::parse("everything"), None);
    }

    #[test]
    fn compose_places_panes() {
        // A 2x window picture (8 x 4 points): left pane red, right pane blue, a gap between.
        let mut img = ColorImage::filled([16, 8], Color32::GREEN);
        for y in 0..8 {
            for x in 0..16 {
                if x < 6 {
                    img.pixels[y * 16 + x] = Color32::RED;
                } else if x >= 8 {
                    img.pixels[y * 16 + x] = Color32::BLUE;
                }
            }
        }
        let screen = Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(8.0, 4.0));
        let left = Rect::from_min_max(egui::pos2(0.0, 1.0), egui::pos2(3.0, 4.0));
        let right = Rect::from_min_max(egui::pos2(4.0, 0.0), egui::pos2(8.0, 4.0));
        let out = compose(&img, screen, &[left, right], Color32::WHITE);
        assert_eq!(out.dimensions(), (16, 8));
        assert_eq!(out.get_pixel(0, 0).0, [255, 255, 255, 255], "above the left pane: background");
        assert_eq!(out.get_pixel(0, 4).0, [255, 0, 0, 255]);
        assert_eq!(out.get_pixel(7, 4).0, [255, 255, 255, 255], "the gap between the panes");
        assert_eq!(out.get_pixel(12, 0).0, [0, 0, 255, 255]);
    }
}
