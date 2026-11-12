//! Rust plugins: shared libraries (`.so`, `.dylib`, `.dll`) built with the `aurafx-plugin` crate,
//! loaded from the plugin folders. Each filter or source they describe joins the registry; running
//! it sends the inputs (postcard bytes) and settings (JSON) to the library and reads its output.

use crate::params::*;
use crate::registry::{register, unregister_plugin, Extension, FilterDef, MultiInput};
use anyhow::{anyhow, bail, Context, Result};
use aurafx_data::DataSet;
use aurafx_plugin::{Buf, Description, PluginTable};
use std::ffi::{c_char, CStr, CString};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

/// Loaded libraries stay loaded for the rest of the run (their functions are in the registry).
static LIBS: Mutex<Vec<(PathBuf, &'static libloading::Library)>> = Mutex::new(Vec::new());

/// Whether a file name is a shared library of this system.
pub fn is_library(p: &Path) -> bool {
    let e = p.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase();
    e == std::env::consts::DLL_EXTENSION
}

/// Whether a shared library found in a plugin folder is taken for an auraFX plugin: its file
/// name says so (it contains "aurafx", as `libaurafx_my_filters.so`). Loading a library runs
/// its code, so other libraries that happen to lie in the folder (what a plugin itself links
/// to, for one) are left alone; a library with another name is loaded when it is named itself.
pub fn named_as_plugin(p: &Path) -> bool {
    p.file_name().and_then(|n| n.to_str()).is_some_and(|n| n.to_ascii_lowercase().contains("aurafx"))
}

/// The plugin libraries of these paths, sorted: a file is taken as it is; of a folder, the
/// shared libraries whose name says they are auraFX plugins (see [`named_as_plugin`]).
pub fn discover(paths: &[PathBuf]) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for p in paths {
        if p.is_file() && is_library(p) {
            out.push(p.clone());
        } else if let Ok(rd) = std::fs::read_dir(p) {
            let mut v: Vec<PathBuf> = rd.filter_map(|e| e.ok()).map(|e| e.path()).filter(|q| q.is_file() && is_library(q) && named_as_plugin(q)).collect();
            v.sort();
            out.extend(v);
        }
    }
    out
}

fn to_json(v: &ParamValue) -> serde_json::Value {
    match v {
        ParamValue::Bool(b) => (*b).into(),
        ParamValue::Int(i) => (*i).into(),
        ParamValue::Float(f) => serde_json::json!(f),
        ParamValue::Vec3(a) => serde_json::json!(a),
        ParamValue::Str(s) => s.clone().into(),
        ParamValue::Array { name, .. } => name.clone().into(),
        ParamValue::Floats(v) => serde_json::json!(v),
    }
}

fn spec_of(p: &aurafx_plugin::Param) -> ParamSpec {
    let num = |v: &serde_json::Value| v.as_f64().unwrap_or(0.0);
    let s = match p.kind.as_str() {
        "int" => ParamSpec::new(
            &p.key,
            &p.label,
            ParamKind::Int { min: p.min.map(|x| x as i64).unwrap_or(i64::MIN), max: p.max.map(|x| x as i64).unwrap_or(i64::MAX) },
            ParamValue::Int(p.default.as_i64().unwrap_or(0)),
        ),
        "bool" => ParamSpec::new(&p.key, &p.label, ParamKind::Bool, ParamValue::Bool(p.default.as_bool().unwrap_or(false))),
        "string" => ParamSpec::new(&p.key, &p.label, ParamKind::Str, ParamValue::Str(p.default.as_str().unwrap_or("").into())),
        "choice" => ParamSpec::new(&p.key, &p.label, ParamKind::Choice(p.choices.clone()), ParamValue::Str(p.default.as_str().unwrap_or("").into())),
        "vec3" => {
            let a = p.default.as_array().cloned().unwrap_or_default();
            let g = |k: usize| a.get(k).map(num).unwrap_or(0.0);
            ParamSpec::new(&p.key, &p.label, ParamKind::Vec3, ParamValue::Vec3([g(0), g(1), g(2)]))
        }
        "array" => ParamSpec::new(
            &p.key,
            &p.label,
            ParamKind::Array { scalar_only: false, vector_only: false },
            ParamValue::Array { assoc: crate::Association::Points, name: String::new() },
        ),
        _ => ParamSpec::new(
            &p.key,
            &p.label,
            ParamKind::Float { min: p.min.unwrap_or(f64::NEG_INFINITY), max: p.max.unwrap_or(f64::INFINITY), log: false },
            ParamValue::Float(num(&p.default)),
        ),
    };
    if p.help.is_empty() {
        s
    } else {
        s.help(&p.help)
    }
}

fn leak(s: String) -> &'static str {
    Box::leak(s.into_boxed_str())
}

/// Load one plugin library and register its filters; returns their ids.
pub fn load(path: &Path) -> Result<Vec<String>> {
    if LIBS.lock().unwrap().iter().any(|(p, _)| p == path) {
        bail!("{} is already loaded (restart auraFX to load a new build of it)", path.display());
    }
    // SAFETY: loading a library runs its initialisers; plugin libraries are code the user chose to install.
    let lib: &'static libloading::Library =
        Box::leak(Box::new(unsafe { libloading::Library::new(path) }.with_context(|| format!("loading {}", path.display()))?));
    let entry: libloading::Symbol<extern "C" fn() -> *const PluginTable> =
        unsafe { lib.get(b"aurafx_plugin_v1") }.map_err(|_| anyhow!("{} is not an auraFX plugin (no aurafx_plugin_v1)", path.display()))?;
    let table = entry();
    if table.is_null() {
        bail!("{} gave no plugin table", path.display());
    }
    // SAFETY: not null, and a plugin's table is a static of the library, which stays loaded.
    let table: &'static PluginTable = unsafe { &*table };
    if table.abi_version != aurafx_plugin::ABI_VERSION {
        bail!("{}: plugin interface {} (this auraFX: {})", path.display(), table.abi_version, aurafx_plugin::ABI_VERSION);
    }
    if table.data_version.is_null() {
        bail!("{} does not say which data version it was built for", path.display());
    }
    let theirs = unsafe { CStr::from_ptr(table.data_version) }.to_string_lossy().into_owned();
    let ours = aurafx_plugin::DATA_VERSION.trim_end_matches('\0');
    if theirs != ours {
        bail!("{} was built for {theirs}; this auraFX has {ours}: rebuild the plugin against this version", path.display());
    }
    let raw = (table.describe)();
    if raw.is_null() {
        bail!("{} could not describe its filters (the plugin failed while starting)", path.display());
    }
    let text = unsafe { CStr::from_ptr(raw) }.to_string_lossy().into_owned();
    (table.free_string)(raw);
    let desc: Description = serde_json::from_str(&text).context("the plugin's description")?;
    let plugin = path.display().to_string();
    let name = desc.name.clone();
    // All of its filters, or none: what was registered before one fails is taken out again.
    let ids = register_filters(desc, table, &plugin).inspect_err(|_| {
        unregister_plugin(&plugin);
    })?;
    LIBS.lock().unwrap().push((path.to_path_buf(), lib));
    tracing::info!("Rust plugin {} ({name}): {}", path.display(), ids.join(", "));
    Ok(ids)
}

/// The id a plugin's filter gets in the registry.
fn registry_id(id: &str) -> Result<String> {
    let rid = id.to_ascii_lowercase().replace([' ', '-'], "_");
    if rid.is_empty() || !rid.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
        bail!("filter id {id:?} must be letters, digits and _");
    }
    Ok(rid)
}

/// Register the filters a plugin describes; returns their ids.
fn register_filters(desc: Description, table: &'static PluginTable, plugin: &str) -> Result<Vec<String>> {
    // Checked before any is registered.
    for f in &desc.filters {
        registry_id(&f.id)?;
    }
    let mut ids = Vec::new();
    for f in desc.filters {
        let id = registry_id(&f.id)?;
        let fid = CString::new(f.id.clone())?;
        let specs: Vec<ParamSpec> = f.params.iter().map(spec_of).collect();
        let label = f.label.clone();
        let apply: crate::registry::CustomFn = Arc::new(move |inputs: &[&DataSet], p: &Params| -> Result<DataSet> {
            let mut map = serde_json::Map::new();
            for (k, v) in p.0.iter() {
                map.insert(k.clone(), to_json(v));
            }
            let pj = CString::new(serde_json::Value::Object(map).to_string())?;
            let encoded: Vec<Vec<u8>> = inputs.iter().map(|d| aurafx_plugin::encode(d).map_err(|e| anyhow!(e))).collect::<Result<_>>()?;
            let bufs: Vec<Buf> = encoded.iter().map(|v| Buf { ptr: v.as_ptr() as *mut u8, len: v.len(), cap: v.len() }).collect();
            let mut out = Buf { ptr: std::ptr::null_mut(), len: 0, cap: 0 };
            let mut err: *mut c_char = std::ptr::null_mut();
            let rc = (table.run)(fid.as_ptr(), pj.as_ptr(), bufs.as_ptr(), bufs.len(), &mut out, &mut err);
            if rc != 0 {
                let msg = if err.is_null() { "failed".to_string() } else { unsafe { CStr::from_ptr(err) }.to_string_lossy().into_owned() };
                (table.free_string)(err);
                bail!("{label}: {msg}");
            }
            let d = aurafx_plugin::decode(out.as_slice()).map_err(|e| anyhow!("{label}: reading its output: {e}"));
            (table.free_buf)(out);
            d
        });
        let def = FilterDef {
            id: leak(id.clone()),
            label: leak(f.label),
            category: leak(f.category),
            description: leak(f.description),
            keywords: Box::leak(f.keywords.into_iter().map(leak).collect::<Vec<_>>().into_boxed_slice()),
            is_source: f.is_source,
            ext: Some(Arc::new(Extension { apply, plugin: plugin.to_string() })),
            temporal: None,
            multi: (!f.is_source && f.inputs > 1).then_some(MultiInput {
                label: "Inputs",
                min: f.inputs,
                max: Some(f.inputs),
                apply: |_, _| Err(anyhow!("plugin filter")),
            }),
            params: specs,
            apply: |_, _| Err(anyhow!("plugin filter")),
        };
        register(def)?;
        ids.push(id);
    }
    Ok(ids)
}

/// Load every plugin library of these paths (see [`discover`]) that is not loaded yet; messages
/// for the log.
pub fn load_all(paths: &[PathBuf], disabled: &[PathBuf]) -> Vec<String> {
    let mut msgs = Vec::new();
    for p in discover(paths) {
        if disabled.contains(&p) || LIBS.lock().unwrap().iter().any(|(q, _)| *q == p) {
            continue;
        }
        match load(&p) {
            Ok(ids) => msgs.push(format!("Loaded the Rust plugin {}: {}", p.display(), ids.join(", "))),
            Err(e) => msgs.push(format!("The Rust plugin {} failed to load: {e:#}", p.display())),
        }
    }
    msgs
}

/// The loaded plugin libraries.
pub fn loaded() -> Vec<PathBuf> {
    LIBS.lock().unwrap().iter().map(|(p, _)| p.clone()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Of a folder, only libraries named as auraFX plugins are taken; a file named itself is
    /// taken whatever it is called.
    #[test]
    fn folders_give_only_libraries_named_as_plugins() {
        let dir = std::env::temp_dir().join(format!("aurafx_discover_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let ext = std::env::consts::DLL_EXTENSION;
        let names = [format!("libaurafx_mine.{ext}"), format!("libother.{ext}"), format!("My-AuraFX-Filters.{ext}"), "aurafx_notes.txt".to_string()];
        for n in &names {
            std::fs::write(dir.join(n), b"").unwrap();
        }
        let found = discover(std::slice::from_ref(&dir));
        assert_eq!(found, vec![dir.join(&names[2]), dir.join(&names[0])]);
        let other = dir.join(&names[1]);
        assert_eq!(discover(&[other.clone(), dir.join(&names[3])]), vec![other.clone()]);
        // What is not a plugin is refused when loaded, with nothing left behind.
        assert!(load(&other).is_err());
        assert!(loaded().is_empty() || !loaded().contains(&other));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
