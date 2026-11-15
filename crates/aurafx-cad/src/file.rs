//! `.afxcad` files: a CAD document (and optionally an assembly of parts) as pretty-printed JSON
//! with a format version, so later versions of auraFX can read what earlier ones wrote and
//! refuse, with a clear message, what a newer one wrote.

use crate::assembly::Assembly;
use crate::document::Document;
use serde::{Deserialize, Serialize};
use std::path::Path;

/// The version written into every file. Bump it when a change would not load in an older
/// program; reading accepts every version up to this one.
pub const FORMAT_VERSION: u32 = 2;

/// What an `.afxcad` file holds.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CadFile {
    pub format: u32,
    /// Always "auraFX CAD document" (helps people who open the file in an editor).
    #[serde(default)]
    pub kind: String,
    pub document: Document,
    /// Present when the file is an assembly: the parts are the components' documents.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub assembly: Option<Assembly>,
}

#[derive(Debug, thiserror::Error)]
pub enum FileError {
    #[error("the file is not a valid .afxcad document: {0}")]
    Parse(String),
    #[error("the file was written by a newer auraFX (format {found}; this one reads up to {supported})")]
    TooNew { found: u32, supported: u32 },
    #[error("{0}")]
    Io(String),
}

impl CadFile {
    pub fn new(document: Document) -> Self {
        Self { format: FORMAT_VERSION, kind: "auraFX CAD document".into(), document, assembly: None }
    }
    pub fn with_assembly(mut self, assembly: Assembly) -> Self {
        self.assembly = Some(assembly);
        self
    }
    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self).unwrap_or_default()
    }
    pub fn from_json(text: &str) -> Result<Self, FileError> {
        // The version first, so a file from the future gets the right message instead of a
        // complaint about some field it does not know.
        let v: serde_json::Value = serde_json::from_str(text).map_err(|e| FileError::Parse(e.to_string()))?;
        let format = v.get("format").and_then(|f| f.as_u64()).ok_or_else(|| FileError::Parse("no \"format\" number".into()))? as u32;
        if format > FORMAT_VERSION {
            return Err(FileError::TooNew { found: format, supported: FORMAT_VERSION });
        }
        serde_json::from_value(v).map_err(|e| FileError::Parse(e.to_string())).map(|mut f: Self| {
            f.document.normalize_ids();
            f
        })
    }
    pub fn save(&self, path: impl AsRef<Path>) -> Result<(), FileError> {
        std::fs::write(path.as_ref(), self.to_json()).map_err(|e| FileError::Io(format!("{}: {e}", path.as_ref().display())))
    }
    pub fn load(path: impl AsRef<Path>) -> Result<Self, FileError> {
        let text = std::fs::read_to_string(path.as_ref()).map_err(|e| FileError::Io(format!("{}: {e}", path.as_ref().display())))?;
        Self::from_json(&text)
    }
}

impl Document {
    /// Write the document as an `.afxcad` file.
    pub fn save(&self, path: impl AsRef<Path>) -> Result<(), FileError> {
        CadFile::new(self.clone()).save(path)
    }
    /// Read the document of an `.afxcad` file (a bare document, as `to_json` writes it, is
    /// accepted too).
    pub fn load(path: impl AsRef<Path>) -> Result<Document, FileError> {
        let text = std::fs::read_to_string(path.as_ref()).map_err(|e| FileError::Io(format!("{}: {e}", path.as_ref().display())))?;
        Self::from_afxcad(&text)
    }
    pub fn from_afxcad(text: &str) -> Result<Document, FileError> {
        match CadFile::from_json(text) {
            Ok(f) => Ok(f.document),
            Err(FileError::Parse(_)) => Document::from_json(text).map_err(|e| FileError::Parse(e.to_string())),
            Err(e) => Err(e),
        }
    }
    pub fn to_afxcad(&self) -> String {
        CadFile::new(self.clone()).to_json()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::templates::templates;

    #[test]
    fn every_template_survives_the_file_round_trip() {
        let all = templates();
        assert!(all.len() >= 6, "{}", all.len());
        for (name, doc) in all {
            let text = doc.to_afxcad();
            let back = Document::from_afxcad(&text).unwrap_or_else(|e| panic!("{name}: {e}"));
            assert_eq!(back, doc, "{name}");
            // The shape of the geometry is unchanged too.
            let (a, b) = (doc.sdf().unwrap().bounds(), back.sdf().unwrap().bounds());
            assert_eq!(a, b, "{name}");
        }
    }

    #[test]
    fn newer_files_are_refused_with_a_message_and_bare_documents_load() {
        let doc = templates().into_iter().next().unwrap().1;
        let mut v: serde_json::Value = serde_json::from_str(&doc.to_afxcad()).unwrap();
        v["format"] = serde_json::json!(FORMAT_VERSION + 1);
        let err = CadFile::from_json(&v.to_string()).unwrap_err();
        assert!(err.to_string().contains("newer auraFX"), "{err}");
        assert_eq!(Document::from_afxcad(&doc.to_json()).unwrap(), doc);
        assert!(Document::from_afxcad("not json").is_err());
    }

    #[test]
    fn save_and_load_through_a_file_with_an_assembly() {
        use crate::assembly::Component;
        let doc = templates().into_iter().next().unwrap().1;
        let asm = Assembly::new("pair").add(Component::new("a", doc.clone())).add(Component::new("b", doc.clone()).at(50.0, 0.0, 0.0));
        let dir = std::env::temp_dir().join(format!("afxcad-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("part.afxcad");
        CadFile::new(doc.clone()).with_assembly(asm.clone()).save(&path).unwrap();
        let back = CadFile::load(&path).unwrap();
        assert_eq!(back.document, doc);
        assert_eq!(back.assembly.unwrap(), asm);
        assert_eq!(Document::load(&path).unwrap(), doc);
        std::fs::remove_dir_all(&dir).ok();
    }
}
