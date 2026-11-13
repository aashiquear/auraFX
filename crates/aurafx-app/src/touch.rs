//! The interface on tablets and phones: the same program, made for fingers and small screens.
//!
//! Everything here is off in the desktop interface. [`mode`] is [`Mode::Desktop`] on desktop
//! systems unless the Interface setting or `AURAFX_INTERFACE` says otherwise, and every caller
//! branches on it, so the desktop interface is drawn by the code it always was.
//!
//! - The mode: Desktop, Tablet or Phone ([`resolve`]).
//! - Safe areas: the strips the system keeps for its status bar, a camera cut-out, the home
//!   indicator or the gesture bar ([`raw_input_hook`], [`safe_area_strips`]).
//! - The touch style: bigger controls and text ([`sync_style`]).
//! - Floating windows that fit the screen and are not dragged by their body ([`FitScreen`]).
//! - The shape of the window, which the automatic layouts follow ([`layout_class`]).

use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::OnceLock;

/// The Interface setting.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub enum InterfaceChoice {
    /// Desktop on desktop systems; Tablet or Phone on iOS and Android, by the size of the screen.
    #[default]
    Automatic,
    Desktop,
    Tablet,
    Phone,
}

impl InterfaceChoice {
    pub const ALL: [InterfaceChoice; 4] = [InterfaceChoice::Automatic, InterfaceChoice::Desktop, InterfaceChoice::Tablet, InterfaceChoice::Phone];
    pub fn label(self) -> &'static str {
        match self {
            InterfaceChoice::Automatic => "Automatic",
            InterfaceChoice::Desktop => "Desktop",
            InterfaceChoice::Tablet => "Tablet",
            InterfaceChoice::Phone => "Phone",
        }
    }
}

/// The interface in use.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Desktop,
    Tablet,
    Phone,
}

impl Mode {
    pub fn is_touch(self) -> bool {
        self != Mode::Desktop
    }
    pub fn is_phone(self) -> bool {
        self == Mode::Phone
    }
    pub fn label(self) -> &'static str {
        match self {
            Mode::Desktop => "Desktop",
            Mode::Tablet => "Tablet",
            Mode::Phone => "Phone",
        }
    }
}

/// A screen whose shorter side is below this many points is a phone.
pub const PHONE_BELOW: f32 = 600.0;

static MODE: AtomicU8 = AtomicU8::new(0);

/// The interface in use this frame (set by [`resolve`] at the start of every frame).
pub fn mode() -> Mode {
    match MODE.load(Ordering::Relaxed) {
        1 => Mode::Tablet,
        2 => Mode::Phone,
        _ => Mode::Desktop,
    }
}

/// `AURAFX_INTERFACE=desktop|tablet|phone`: the interface whatever the setting is (to try the
/// touch interfaces, and to take their pictures, on a desktop machine).
fn env_mode() -> Option<Mode> {
    static M: OnceLock<Option<Mode>> = OnceLock::new();
    *M.get_or_init(|| match std::env::var("AURAFX_INTERFACE").ok()?.trim().to_ascii_lowercase().as_str() {
        "desktop" => Some(Mode::Desktop),
        "tablet" => Some(Mode::Tablet),
        "phone" => Some(Mode::Phone),
        _ => None,
    })
}

/// True when the interface is fixed by `AURAFX_INTERFACE`.
pub fn mode_from_env() -> bool {
    env_mode().is_some()
}

/// The mode for a screen of `size` points: what Automatic means on iOS and Android.
pub fn mode_for_size(size: egui::Vec2) -> Mode {
    if size.x.min(size.y) < PHONE_BELOW {
        Mode::Phone
    } else {
        Mode::Tablet
    }
}

/// Decide the interface for this frame and remember it for [`mode`].
pub fn resolve(ctx: &egui::Context, choice: InterfaceChoice) -> Mode {
    let m = env_mode().unwrap_or_else(|| match choice {
        InterfaceChoice::Desktop => Mode::Desktop,
        InterfaceChoice::Tablet => Mode::Tablet,
        InterfaceChoice::Phone => Mode::Phone,
        InterfaceChoice::Automatic => {
            if cfg!(any(target_os = "ios", target_os = "android")) {
                // The size of the screen in the system's points, whatever the UI scale is.
                mode_for_size(ctx.viewport_rect().size() * ctx.zoom_factor())
            } else {
                Mode::Desktop
            }
        }
    });
    MODE.store(
        match m {
            Mode::Desktop => 0,
            Mode::Tablet => 1,
            Mode::Phone => 2,
        },
        Ordering::Relaxed,
    );
    m
}

// ---------------------------------------------------------------------------------------------
// Safe areas

/// Insets as `[top, bottom, left, right]`.
type Insets = [f32; 4];

/// Where the system's insets come from on Android: a function that returns them in pixels
/// (`[top, bottom, left, right]`), or `None` when it cannot tell.
type InsetsSource = Box<dyn Fn() -> Option<Insets> + Send + Sync>;

static INSETS_SOURCE: OnceLock<InsetsSource> = OnceLock::new();

/// Tell the app where to ask for the system's insets (status bar, cut-out, navigation or gesture
/// bar, and the on-screen keyboard when the system counts it), in pixels as `[top, bottom, left,
/// right]`. The Android entry point sets this; iOS reports its safe area through the windowing
/// library.
pub fn set_insets_source(f: impl Fn() -> Option<[f32; 4]> + Send + Sync + 'static) {
    let _ = INSETS_SOURCE.set(Box::new(f));
}

/// `AURAFX_SAFE_AREA=top,bottom,left,right` (points): pretend the system keeps these strips, so
/// the padding of a tablet or phone can be seen and pictured on a desktop machine.
fn env_insets() -> Option<Insets> {
    static I: OnceLock<Option<Insets>> = OnceLock::new();
    *I.get_or_init(|| parse_insets(&std::env::var("AURAFX_SAFE_AREA").ok()?))
}

/// `top,bottom,left,right`; missing numbers are 0.
pub fn parse_insets(text: &str) -> Option<Insets> {
    let mut out = [0.0f32; 4];
    let mut any = false;
    for (k, part) in text.split(',').take(4).enumerate() {
        let v: f32 = part.trim().parse().ok()?;
        if !(0.0..=2000.0).contains(&v) {
            return None;
        }
        out[k] = v;
        any = true;
    }
    any.then_some(out)
}

/// `AURAFX_KEYBOARD=points`: pretend an on-screen keyboard of this height is up while a text box
/// has the focus (to try that case on a desktop machine).
fn env_keyboard() -> Option<f32> {
    static K: OnceLock<Option<f32>> = OnceLock::new();
    *K.get_or_init(|| std::env::var("AURAFX_KEYBOARD").ok()?.trim().parse().ok())
}

/// The share of the screen's height an on-screen keyboard takes where the system does not say
/// (iOS): about a third of a tall screen, half of a wide one.
pub fn keyboard_guess(screen: egui::Vec2, phone: bool) -> f32 {
    let tall = screen.y > screen.x;
    let share = match (phone, tall) {
        (true, true) => 0.40,
        (true, false) => 0.55,
        (false, true) => 0.28,
        (false, false) => 0.50,
    };
    (screen.y * share).round()
}

#[derive(Default)]
struct InsetState {
    /// What the system last reported (points), without a keyboard guess.
    system: Insets,
    /// When the Android source was last asked (seconds of the app's clock).
    asked: f64,
    /// What was last given to egui.
    sent: Insets,
}

static INSET_STATE: parking_lot::Mutex<InsetState> = parking_lot::Mutex::new(InsetState { system: [0.0; 4], asked: -1.0, sent: [0.0; 4] });

/// Give egui the safe area before each frame: the system's insets (iOS reports them itself;
/// Android through [`set_insets_source`]), `AURAFX_SAFE_AREA`, and room for the on-screen
/// keyboard where it is only known that one is probably up. On a desktop system without the
/// variables this changes nothing.
pub fn raw_input_hook(ctx: &egui::Context, raw: &mut egui::RawInput, keyboard_room: bool) {
    let mut st = INSET_STATE.lock();
    // iOS: the windowing library reports the safe area when it changes.
    if let Some(s) = raw.safe_area_insets {
        st.system = [s.0.top, s.0.bottom, s.0.left, s.0.right];
    }
    // iOS: the window tells the windowing library it has no insets, so that the picture covers
    // the whole screen (see `ios_safe_area`); the real ones come from there.
    #[cfg(target_os = "ios")]
    if crate::ios_safe_area::installed() {
        let zoom = ctx.zoom_factor().max(0.1);
        st.system = crate::ios_safe_area::insets().map(|v| v / zoom);
    }
    if let Some(src) = INSETS_SOURCE.get() {
        let now = raw.time.unwrap_or(0.0);
        if st.asked < 0.0 || now - st.asked > 0.25 || now < st.asked {
            st.asked = now;
            match src() {
                Some(px) => {
                    let ppp = ctx.pixels_per_point().max(0.1);
                    st.system = px.map(|v| (v / ppp).max(0.0));
                }
                // Not known (the window is not up yet, or the system would not say): room for a
                // status bar and a gesture bar rather than none.
                None => st.system = [28.0, 20.0, 0.0, 0.0],
            }
        }
    }
    let mut want = env_insets().unwrap_or(st.system);
    // The keyboard: Android counts it in the bottom inset. Elsewhere it is only known that a text
    // box has the focus, so keep the usual room for it (Settings > Interface turns this off, for
    // a tablet with a hardware keyboard).
    if ctx.text_edit_focused() {
        let screen = ctx.viewport_rect().size();
        let guess = if let Some(k) = env_keyboard() {
            k
        } else if cfg!(target_os = "ios") && keyboard_room {
            keyboard_guess(screen, mode().is_phone())
        } else {
            0.0
        };
        if guess > 0.0 {
            want[1] = want[1].max(guess).min(screen.y * 0.7);
        }
    }
    if want != st.sent || raw.safe_area_insets.is_some() {
        if want != st.sent {
            ctx.request_repaint();
        }
        st.sent = want;
        raw.safe_area_insets = Some(egui::SafeAreaInsets(egui::epaint::MarginF32 { top: want[0], bottom: want[1], left: want[2], right: want[3] }));
    }
}

/// Reserve the system's strips at the edges of the window, in the panel color, so that nothing of
/// the app sits under the status bar, a camera cut-out, the home indicator or the gesture bar.
/// Call before any other panel. Without insets (every desktop system) it adds nothing.
pub fn safe_area_strips(ui: &mut egui::Ui) {
    let m = ui.ctx().input(|i| i.safe_area_insets()).0;
    if m.top <= 0.0 && m.bottom <= 0.0 && m.left <= 0.0 && m.right <= 0.0 {
        return;
    }
    let frame = egui::Frame::NONE.fill(ui.visuals().panel_fill);
    let strip = |p: egui::Panel, size: f32| p.exact_size(size).resizable(false).show_separator_line(false).frame(frame);
    if m.top > 0.0 {
        strip(egui::Panel::top("safe area top"), m.top).show(ui, |_| {});
    }
    if m.bottom > 0.0 {
        strip(egui::Panel::bottom("safe area bottom"), m.bottom).show(ui, |_| {});
    }
    if m.left > 0.0 {
        strip(egui::Panel::left("safe area left"), m.left).show(ui, |_| {});
    }
    if m.right > 0.0 {
        strip(egui::Panel::right("safe area right"), m.right).show(ui, |_| {});
    }
}

// ---------------------------------------------------------------------------------------------
// The touch style

/// The height of a control in the touch interfaces (points).
pub const TOUCH_HEIGHT: f32 = 40.0;

static STYLED: AtomicU8 = AtomicU8::new(0);

fn touch_style(style: &mut egui::Style) {
    use egui::{FontFamily::Monospace, FontFamily::Proportional, FontId, TextStyle};
    let s = &mut style.spacing;
    s.interact_size = egui::vec2(48.0, TOUCH_HEIGHT);
    s.button_padding = egui::vec2(12.0, 8.0);
    s.item_spacing = egui::vec2(10.0, 8.0);
    s.icon_width = 24.0;
    s.icon_width_inner = 14.0;
    s.icon_spacing = 8.0;
    s.slider_width = 160.0;
    s.slider_rail_height = 10.0;
    s.combo_width = 140.0;
    s.indent = 24.0;
    s.scroll.bar_width = 14.0;
    s.scroll.floating_width = 5.0;
    s.scroll.handle_min_length = 36.0;
    s.tooltip_width = 320.0;
    s.menu_margin = egui::Margin::same(8);
    let i = &mut style.interaction;
    i.interact_radius = 8.0;
    i.resize_grab_radius_side = 12.0;
    i.resize_grab_radius_corner = 18.0;
    // Press and hold shows what a control does (and why one is disabled).
    i.tooltip_delay = 0.35;
    i.show_tooltips_only_when_still = true;
    style.text_styles = [
        (TextStyle::Small, FontId::new(11.5, Proportional)),
        (TextStyle::Body, FontId::new(14.5, Proportional)),
        (TextStyle::Button, FontId::new(14.5, Proportional)),
        (TextStyle::Heading, FontId::new(20.0, Proportional)),
        (TextStyle::Monospace, FontId::new(13.5, Monospace)),
    ]
    .into();
}

/// Keep egui's style in step with the mode: the touch style in Tablet and Phone, the desktop
/// one (untouched when the app never left Desktop) otherwise. Cheap; call every frame.
pub fn sync_style(ctx: &egui::Context, mode: Mode, theme: crate::theme::ThemeMode, scale: f32) {
    if mode.is_touch() {
        // The theme sets its own (desktop) spacing whenever it is applied: put ours back on top.
        if ctx.global_style().spacing.interact_size.y != TOUCH_HEIGHT {
            ctx.all_styles_mut(touch_style);
        }
        STYLED.store(1, Ordering::Relaxed);
    } else if STYLED.swap(0, Ordering::Relaxed) == 1 {
        // Back from a touch interface: the style of a fresh start.
        ctx.all_styles_mut(|style| {
            let visuals = style.visuals.clone();
            *style = egui::Style::default();
            style.visuals = visuals;
        });
        crate::theme::apply(ctx, theme, scale);
    }
}

// ---------------------------------------------------------------------------------------------
// Floating windows

/// The part of the screen a floating window may use: inside the safe area, with a small margin.
pub fn window_area(ctx: &egui::Context) -> egui::Rect {
    ctx.content_rect().shrink(if mode().is_phone() { 4.0 } else { 12.0 })
}

/// Floating windows in the touch interfaces. In Desktop these do exactly what `show` does.
pub trait FitScreen<'open>: Sized {
    /// `show`, and in the touch interfaces: the window is placed by the app inside the safe area
    /// (so a finger on its body or on a text box in it does not drag it away), is no larger than
    /// the screen, and its contents scroll when they do not fit. A phone's windows are as wide
    /// as the screen.
    fn show_fit<R>(self, ctx: &egui::Context, add_contents: impl FnOnce(&mut egui::Ui) -> R) -> Option<egui::InnerResponse<Option<R>>>;
    /// The placing and the limits of [`Self::show_fit`] without the scrolling, for a window whose
    /// contents scroll by themselves. Call last, just before `show`.
    fn fit_own_scroll(self, ctx: &egui::Context) -> Self;
}

/// The largest contents a floating window may have.
fn window_room(ctx: &egui::Context) -> egui::Vec2 {
    let area = window_area(ctx);
    let style = ctx.global_style();
    // The window's frame and title bar around the contents.
    let chrome_x = 2.0 * (style.spacing.window_margin.leftf() + 1.0);
    let chrome_y = style.spacing.interact_size.y + 2.0 * (style.spacing.window_margin.topf() + 1.0) + style.spacing.item_spacing.y;
    egui::vec2((area.width() - chrome_x).max(120.0), (area.height() - chrome_y).max(100.0))
}

fn fit_window<'a>(w: egui::Window<'a>, ctx: &egui::Context) -> egui::Window<'a> {
    let room = window_room(ctx);
    // Anchored: a window is where the screen has room for it (above the keyboard when one is up),
    // and is not moved by a drag.
    let mut w = w.anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO).constrain_to(window_area(ctx)).max_width(room.x).max_height(room.y);
    if mode().is_phone() {
        w = w.min_width(room.x).collapsible(false);
    }
    w
}

impl<'open> FitScreen<'open> for egui::Window<'open> {
    fn show_fit<R>(self, ctx: &egui::Context, add_contents: impl FnOnce(&mut egui::Ui) -> R) -> Option<egui::InnerResponse<Option<R>>> {
        if !mode().is_touch() {
            return self.show(ctx, add_contents);
        }
        let room = window_room(ctx);
        // Sized by its contents up to the screen, then they scroll (sideways too: a table wider
        // than a phone can still be reached).
        fit_window(self, ctx).resizable(false).show(ctx, |ui| {
            let width = ui.available_width().min(room.x);
            egui::ScrollArea::both()
                .auto_shrink([true, true])
                .max_width(room.x)
                .max_height(room.y)
                // As tall as the screen allows before the contents scroll (not just as tall as
                // a window is to begin with).
                .min_scrolled_height(room.y)
                .show(ui, |ui| {
                    // Lay out to the window's width, not to the endless width of a scrolling area.
                    ui.set_max_width(width);
                    if mode().is_phone() {
                        ui.set_min_width(width);
                    }
                    let r = add_contents(ui);
                    keep_focus_in_view(ui);
                    r
                })
                .inner
        })
    }

    fn fit_own_scroll(self, ctx: &egui::Context) -> Self {
        if !mode().is_touch() {
            return self;
        }
        fit_window(self, ctx)
    }
}

/// When the room on the screen has just shrunk (the on-screen keyboard came up), scroll the
/// text box that has the focus into view if it is in this window.
fn keep_focus_in_view(ui: &mut egui::Ui) {
    let height = ui.ctx().content_rect().height();
    let id = egui::Id::new("aurafx touch: room last frame");
    let before: Option<(u64, f32, f32)> = ui.data(|d| d.get_temp(id));
    let frame = ui.ctx().cumulative_frame_nr();
    // (The height of the frame before this one, whichever window asks first in a frame.)
    let (last, now) = match before {
        Some((f, last, now)) if f == frame => (last, now),
        Some((_, _, now)) => (now, height),
        None => (height, height),
    };
    ui.data_mut(|d| d.insert_temp(id, (frame, last, now)));
    if now < last - 40.0 {
        if let Some(r) = ui.memory(|m| m.focused()).and_then(|f| ui.ctx().read_response(f)) {
            if r.layer_id == ui.layer_id() {
                ui.scroll_to_rect(r.rect.expand(24.0), None);
            }
        }
    }
}

/// A row of controls: `ui.horizontal` in the desktop interface. In the touch interfaces, where
/// the controls are larger and the panels narrower, the row scrolls sideways when it is wider
/// than its panel instead of being cut off (and of widening everything below it).
pub fn row<R>(ui: &mut egui::Ui, id: &str, add_contents: impl FnOnce(&mut egui::Ui) -> R) -> R {
    if !mode().is_touch() {
        return ui.horizontal(add_contents).inner;
    }
    let width = ui.available_width();
    egui::ScrollArea::horizontal()
        .id_salt(id)
        .show(ui, |ui| {
            // What is laid out from the right stays at the panel's right edge.
            ui.set_max_width(width);
            ui.horizontal(add_contents).inner
        })
        .inner
}

/// A small button: as it is in the desktop interface, a button of the usual touch size in the
/// touch interfaces (a small one is too low for a finger).
pub fn small_button(ui: &mut egui::Ui, text: impl Into<egui::WidgetText>) -> egui::Response {
    if mode().is_touch() {
        ui.button(text)
    } else {
        ui.small_button(text)
    }
}

// ---------------------------------------------------------------------------------------------
// The shape of the window

/// What the automatic layouts go by: the interface and whether the window is wide or tall.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LayoutClass {
    /// A desktop window, wide: the full workbench.
    Workbench,
    /// A tablet held wide: the workbench with the panels that work there.
    TabletWide,
    /// A tablet held tall, or a tall desktop window: the view on top, two tab groups below.
    TabletTall,
    /// A phone held wide: the view on the left, one tab group on the right.
    PhoneWide,
    /// A phone held tall: the view on top, one tab group below.
    PhoneTall,
}

/// The layout class for a window of `size` points in `mode`.
pub fn layout_class(mode: Mode, size: egui::Vec2) -> LayoutClass {
    let tall = size.y > size.x;
    match mode {
        Mode::Phone => {
            if tall {
                LayoutClass::PhoneTall
            } else {
                LayoutClass::PhoneWide
            }
        }
        Mode::Tablet => {
            if tall {
                LayoutClass::TabletTall
            } else {
                LayoutClass::TabletWide
            }
        }
        // A desktop window chosen to follow its shape: small ones get the phone's layouts.
        Mode::Desktop => {
            if size.x.min(size.y) < PHONE_BELOW {
                if tall {
                    LayoutClass::PhoneTall
                } else {
                    LayoutClass::PhoneWide
                }
            } else if tall {
                LayoutClass::TabletTall
            } else {
                LayoutClass::Workbench
            }
        }
    }
}

/// What the touch interfaces keep between frames.
#[derive(Default)]
pub struct TouchUi {
    /// The phone's menu list is open.
    pub menu_open: bool,
    /// The top menu whose items are shown in the phone's menu list.
    pub menu_section: Option<&'static str>,
    /// The 3D view has the whole screen (the panels and the tool rows are put away).
    pub view_maximized: bool,
    /// The CAD environment was open last frame (a phone says once that it wants a larger screen).
    pub cad_open: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_phone_is_a_screen_with_a_short_side_below_600_points() {
        assert_eq!(mode_for_size(egui::vec2(390.0, 844.0)), Mode::Phone);
        assert_eq!(mode_for_size(egui::vec2(844.0, 390.0)), Mode::Phone);
        assert_eq!(mode_for_size(egui::vec2(820.0, 1180.0)), Mode::Tablet);
        assert_eq!(mode_for_size(egui::vec2(1366.0, 1024.0)), Mode::Tablet);
        assert_eq!(mode_for_size(egui::vec2(600.0, 960.0)), Mode::Tablet);
    }

    #[test]
    fn layouts_follow_the_shape() {
        assert_eq!(layout_class(Mode::Phone, egui::vec2(390.0, 844.0)), LayoutClass::PhoneTall);
        assert_eq!(layout_class(Mode::Phone, egui::vec2(844.0, 390.0)), LayoutClass::PhoneWide);
        assert_eq!(layout_class(Mode::Tablet, egui::vec2(820.0, 1180.0)), LayoutClass::TabletTall);
        assert_eq!(layout_class(Mode::Tablet, egui::vec2(1180.0, 820.0)), LayoutClass::TabletWide);
        assert_eq!(layout_class(Mode::Desktop, egui::vec2(1600.0, 950.0)), LayoutClass::Workbench);
        assert_eq!(layout_class(Mode::Desktop, egui::vec2(900.0, 1400.0)), LayoutClass::TabletTall);
        assert_eq!(layout_class(Mode::Desktop, egui::vec2(420.0, 800.0)), LayoutClass::PhoneTall);
    }

    #[test]
    fn safe_area_text() {
        assert_eq!(parse_insets("47,34,0,0"), Some([47.0, 34.0, 0.0, 0.0]));
        assert_eq!(parse_insets(" 24 , 20 "), Some([24.0, 20.0, 0.0, 0.0]));
        assert_eq!(parse_insets("x"), None);
        assert_eq!(parse_insets(""), None);
        assert_eq!(parse_insets("-3"), None);
    }

    #[test]
    fn the_keyboard_guess_leaves_room_to_see_the_text() {
        for (screen, phone) in
            [(egui::vec2(390.0, 844.0), true), (egui::vec2(844.0, 390.0), true), (egui::vec2(820.0, 1180.0), false), (egui::vec2(1180.0, 820.0), false)]
        {
            let k = keyboard_guess(screen, phone);
            assert!(k > 0.2 * screen.y && k < 0.6 * screen.y, "{screen:?}: {k}");
        }
    }

    #[test]
    fn the_interface_setting_round_trips() {
        #[derive(serde::Serialize, serde::Deserialize)]
        struct S {
            interface: InterfaceChoice,
        }
        for c in InterfaceChoice::ALL {
            let t = toml::to_string(&S { interface: c }).unwrap();
            assert_eq!(toml::from_str::<S>(&t).unwrap().interface, c);
        }
    }
}
