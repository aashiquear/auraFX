//! Menus that fit on the screen, and the one way the app lists filters and sources in a menu.
//!
//! Every menu of the app opens through [`MenuExt::scroll_menu`]: its items sit in a scroll area no
//! taller than the window, so a long list (the sources, a category of filters, a list of templates)
//! can be scrolled instead of running off the screen. [`filter_menu`] and [`source_menu`] build the
//! lists from the filter registry, each filter once, under its category.

use aurafx_filters::FilterDef;
use egui::{InnerResponse, Ui};
use std::sync::Arc;

/// The tallest a menu may be: the window's height less a margin for the menu bar and the popup frame.
fn max_height(ui: &Ui) -> f32 {
    (ui.ctx().content_rect().height() - 90.0).max(160.0)
}

/// The contents of a menu in a vertical scroll area that never grows past the window.
pub fn scrolled<R>(ui: &mut Ui, add: impl FnOnce(&mut Ui) -> R) -> R {
    egui::ScrollArea::vertical().max_height(max_height(ui)).auto_shrink([true, true]).show(ui, add).inner
}

pub trait MenuExt {
    /// `menu_button` whose contents scroll when they are taller than the window.
    fn scroll_menu<'a, R>(&mut self, atoms: impl egui::IntoAtoms<'a>, add: impl FnOnce(&mut Ui) -> R) -> InnerResponse<Option<R>>;
}

impl MenuExt for Ui {
    fn scroll_menu<'a, R>(&mut self, atoms: impl egui::IntoAtoms<'a>, add: impl FnOnce(&mut Ui) -> R) -> InnerResponse<Option<R>> {
        self.menu_button(atoms, |ui| scrolled(ui, add))
    }
}

/// The categories of the registered filters (or sources), each once, in the order they first appear.
pub fn categories(defs: &[Arc<FilterDef>], sources: bool) -> Vec<&'static str> {
    let mut out: Vec<&'static str> = Vec::new();
    for f in defs.iter().filter(|f| f.is_source == sources) {
        if !out.contains(&f.category) {
            out.push(f.category);
        }
    }
    out
}

/// The filters that `keep` accepts, one submenu per category, each filter once. Returns the id of
/// the one clicked.
pub fn filter_menu(ui: &mut Ui, keep: impl Fn(&FilterDef) -> bool) -> Option<&'static str> {
    filter_menu_for(ui, None, keep)
}

/// The summary of a node's data and whether it has time steps: what the suggestions go by.
pub fn node_context(state: &crate::app::State, id: Option<aurafx_pipeline::NodeId>) -> Option<(aurafx_data::DataSetInfo, bool)> {
    let s = state.session.lock();
    let id = id.or(s.active)?;
    let info = s.nodes.get(&id)?.info.clone()?;
    let timed = s.node_times(id, 0).len() > 1;
    Some((info, timed))
}

/// The few filters that fit this data best, as buttons that say why. Returns the one clicked.
pub fn suggestion_buttons(ui: &mut Ui, info: &aurafx_data::DataSetInfo, has_time: bool) -> Option<&'static str> {
    let list = aurafx_filters::suggest::suggestions(info, has_time);
    if list.is_empty() {
        return None;
    }
    let mut picked = None;
    ui.label(egui::RichText::new(format!("{} Suggested for this data", crate::icons::SPARKLE)).weak().small());
    for s in list {
        if ui.button(crate::icons::with(s.def.label)).on_hover_text(format!("{}\n\n{}", s.reason, s.def.description)).clicked() {
            ui.close();
            picked = Some(s.def.id);
        }
    }
    ui.separator();
    picked
}

/// [`filter_menu`] for data with this summary: filters that are unlikely to work on it are shown
/// faded and say what they need (they can still be chosen).
pub fn filter_menu_for(ui: &mut Ui, info: Option<&aurafx_data::DataSetInfo>, keep: impl Fn(&FilterDef) -> bool) -> Option<&'static str> {
    let defs = aurafx_filters::registry();
    let mut picked = None;
    for cat in categories(&defs, false) {
        let items: Vec<&Arc<FilterDef>> = defs.iter().filter(|f| !f.is_source && f.category == cat && keep(f)).collect();
        if items.is_empty() {
            continue;
        }
        ui.scroll_menu(cat, |ui| {
            for f in items {
                let why_not = info.and_then(|i| aurafx_filters::suggest::applicable(f, i).err());
                let r = match &why_not {
                    Some(w) => ui.button(egui::RichText::new(f.label).weak()).on_hover_text(format!("May not work on this data: {w}.\n\n{}", f.description)),
                    None => ui.button(f.label).on_hover_text(f.description),
                };
                if r.clicked() {
                    ui.close();
                    picked = Some(f.id);
                }
            }
        });
    }
    picked
}

/// The sources, each once, in registry order. Returns the id of the one clicked.
pub fn source_menu(ui: &mut Ui) -> Option<&'static str> {
    let defs = aurafx_filters::registry();
    let mut picked = None;
    for f in defs.iter().filter(|f| f.is_source) {
        if ui.button(format!("{}…", f.label)).on_hover_text(f.description).clicked() {
            ui.close();
            picked = Some(f.id);
        }
    }
    picked
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_category_and_every_filter_appears_once() {
        let defs = aurafx_filters::registry();
        let cats = categories(&defs, false);
        let mut sorted = cats.clone();
        sorted.sort();
        sorted.dedup();
        assert_eq!(sorted.len(), cats.len(), "a category listed twice: {cats:?}");
        // Labels are unique among filters and among sources, so no menu shows two entries that read the same.
        for sources in [false, true] {
            let mut labels: Vec<&str> = defs.iter().filter(|f| f.is_source == sources).map(|f| f.label).collect();
            let n = labels.len();
            labels.sort();
            labels.dedup();
            assert_eq!(labels.len(), n, "two {} with one label", if sources { "sources" } else { "filters" });
        }
    }
}
