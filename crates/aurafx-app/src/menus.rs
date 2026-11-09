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
    let defs = aurafx_filters::registry();
    let mut picked = None;
    for cat in categories(&defs, false) {
        let items: Vec<&Arc<FilterDef>> = defs.iter().filter(|f| !f.is_source && f.category == cat && keep(f)).collect();
        if items.is_empty() {
            continue;
        }
        ui.scroll_menu(cat, |ui| {
            for f in items {
                if ui.button(f.label).on_hover_text(f.description).clicked() {
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
