//! Keyboard shortcuts: named actions bound to key combinations (`Ctrl+Shift+S`, `R`, `Space`),
//! kept in the settings and changed in Settings › Keyboard (typed, or recorded from the next key
//! pressed); optional vim-style view keys.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// The actions a key can do.
pub const ACTIONS: &[(&str, &str, &[&str])] = &[
    ("open", "Open a file", &["Ctrl+O"]),
    ("palette", "Command palette", &["Ctrl+K", "Ctrl+Space"]),
    ("save_state", "Save the state", &["Ctrl+S"]),
    ("screenshot", "Save a screenshot", &["Ctrl+Shift+S"]),
    ("undo", "Undo", &["Ctrl+Z"]),
    ("redo", "Redo", &["Ctrl+Shift+Z", "Ctrl+Y"]),
    ("reset_camera", "Reset the camera", &["R"]),
    ("delete_node", "Delete the active object", &["Delete"]),
    ("play", "Play or pause time", &["Space"]),
    ("next_step", "Next time step", &["Period"]),
    ("prev_step", "Previous time step", &["Comma"]),
    ("settings", "Settings", &["Ctrl+Comma"]),
    ("view_x", "Look along +x", &[]),
    ("view_y", "Look along +y", &[]),
    ("view_z", "Look along -z (from above)", &[]),
    ("view_iso", "Isometric view", &[]),
];

/// The user's bindings: action → key combinations (missing actions take the defaults).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Shortcuts {
    pub bindings: BTreeMap<String, Vec<String>>,
    /// h j k l turn the view, + and - zoom, 0 resets (when no text field has the keyboard).
    pub vim_keys: bool,
}

impl Shortcuts {
    /// The combinations bound to an action.
    pub fn of(&self, action: &str) -> Vec<String> {
        match self.bindings.get(action) {
            Some(v) => v.clone(),
            None => ACTIONS.iter().find(|a| a.0 == action).map(|a| a.2.iter().map(|s| s.to_string()).collect()).unwrap_or_default(),
        }
    }

    /// Bind an action to combinations (written like "Ctrl+Shift+S"; empty: no key).
    pub fn set(&mut self, action: &str, combos: Vec<String>) -> Result<(), String> {
        if !ACTIONS.iter().any(|a| a.0 == action) {
            return Err(format!("no action '{action}' (one of {})", ACTIONS.iter().map(|a| a.0).collect::<Vec<_>>().join(", ")));
        }
        for c in &combos {
            parse(c).ok_or_else(|| format!("'{c}' is not a key combination (like Ctrl+Shift+S, R, F5, Space)"))?;
        }
        self.bindings.insert(action.to_string(), combos);
        Ok(())
    }

    /// Actions whose combination is also bound to another action.
    pub fn clashes(&self) -> Vec<(String, String, String)> {
        let mut seen: BTreeMap<String, String> = BTreeMap::new();
        let mut out = Vec::new();
        for (a, _, _) in ACTIONS {
            for c in self.of(a) {
                let key = normalise(&c);
                if let Some(b) = seen.get(&key) {
                    out.push((c.clone(), b.clone(), a.to_string()));
                } else {
                    seen.insert(key, a.to_string());
                }
            }
        }
        out
    }
}

fn normalise(c: &str) -> String {
    parse(c).map(|(m, k)| format!("{}{}{}{:?}", m.ctrl || m.command, m.shift, m.alt, k)).unwrap_or_else(|| c.to_string())
}

/// "Ctrl+Shift+S" → modifiers and key (Ctrl is Cmd on macOS).
pub fn parse(combo: &str) -> Option<(egui::Modifiers, egui::Key)> {
    let mut m = egui::Modifiers::NONE;
    let parts: Vec<&str> = combo.split('+').map(str::trim).filter(|p| !p.is_empty()).collect();
    let (key, mods) = parts.split_last()?;
    for p in mods {
        match p.to_ascii_lowercase().as_str() {
            "ctrl" | "cmd" | "command" => m = m | egui::Modifiers::COMMAND,
            "shift" => m = m | egui::Modifiers::SHIFT,
            "alt" | "option" => m = m | egui::Modifiers::ALT,
            _ => return None,
        }
    }
    let k = match key.to_ascii_lowercase().as_str() {
        "." => egui::Key::Period,
        "," => egui::Key::Comma,
        "del" => egui::Key::Delete,
        "esc" => egui::Key::Escape,
        "+" | "plus" => egui::Key::Plus,
        "-" | "minus" => egui::Key::Minus,
        _ => egui::Key::from_name(key).or_else(|| {
            let mut c = key.chars();
            let first = c.next()?.to_ascii_uppercase();
            egui::Key::from_name(&format!("{first}{}", c.as_str().to_ascii_lowercase()))
        })?,
    };
    Some((m, k))
}

/// A combination as text, from a key press.
pub fn combo_text(m: egui::Modifiers, k: egui::Key) -> String {
    let mut s = String::new();
    if m.ctrl || m.command {
        s += "Ctrl+";
    }
    if m.alt {
        s += "Alt+";
    }
    if m.shift {
        s += "Shift+";
    }
    s + k.name()
}

/// Whether one of the action's combinations was pressed (and consume it). Plain keys (no Ctrl or
/// Alt) do nothing while a text field has the keyboard.
pub fn pressed(i: &mut egui::InputState, sc: &Shortcuts, action: &str, typing: bool) -> bool {
    for c in sc.of(action) {
        if let Some((m, k)) = parse(&c) {
            if typing && !(m.ctrl || m.command || m.alt) {
                continue;
            }
            if i.consume_key(m, k) {
                return true;
            }
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn combinations_parse_and_clash() {
        assert_eq!(parse("Ctrl+Shift+S"), Some((egui::Modifiers::COMMAND | egui::Modifiers::SHIFT, egui::Key::S)));
        assert_eq!(parse("r"), Some((egui::Modifiers::NONE, egui::Key::R)));
        assert_eq!(parse("F5"), Some((egui::Modifiers::NONE, egui::Key::F5)));
        assert_eq!(parse("Period"), Some((egui::Modifiers::NONE, egui::Key::Period)));
        assert!(parse("Hyper+Q").is_none());
        let mut s = Shortcuts::default();
        assert_eq!(s.of("screenshot"), vec!["Ctrl+Shift+S".to_string()]);
        assert!(s.clashes().is_empty());
        s.set("screenshot", vec!["Ctrl+P".into()]).unwrap();
        s.set("view_iso", vec!["R".into()]).unwrap();
        assert_eq!(s.clashes().len(), 1);
        assert!(s.set("nothing", vec![]).is_err());
        assert!(s.set("play", vec!["Ctrl+".into()]).is_err());
        assert_eq!(combo_text(egui::Modifiers::COMMAND | egui::Modifiers::SHIFT, egui::Key::S), "Ctrl+Shift+S");
    }
}
