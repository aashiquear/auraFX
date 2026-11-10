//! Rust plugins for auraFX: filters and sources compiled into a shared library (`cdylib`) that
//! auraFX loads from its plugin folders at start (and from Edit › Plugins).
//!
//! ```ignore
//! use aurafx_plugin::{export_plugin, DataSet, Filter, Param};
//!
//! fn scale(inputs: &[DataSet], p: &aurafx_plugin::Params) -> Result<DataSet, String> {
//!     let mut d = inputs[0].clone();
//!     let f = p.f64("factor", 2.0);
//!     // ... change d ...
//!     Ok(d)
//! }
//!
//! export_plugin! {
//!     name: "My filters",
//!     filters: [Filter::new("scale_points", "Scale Points", scale)
//!         .describe("Scale the points about the origin.")
//!         .param(Param::float("factor", "Factor", 2.0))],
//! }
//! ```
//!
//! The library exports one C function, `aurafx_plugin_v1`, that returns a table of C functions:
//! data crosses as postcard bytes of [`DataSet`] and settings as JSON, so the plugin and auraFX must
//! be built from the same version of `aurafx-data` (checked when the plugin loads).

pub use aurafx_data;
pub use aurafx_data::DataSet;
use serde::{Deserialize, Serialize};
pub use serde_json;
use std::ffi::{c_char, CStr, CString};

/// The version of the C table.
pub const ABI_VERSION: u32 = 1;
/// The version of the data the plugin and auraFX exchange (the `aurafx-data` crate's).
pub const DATA_VERSION: &str = concat!("aurafx-data ", env!("CARGO_PKG_VERSION"), " format 2\0");

/// Bytes handed across the boundary; freed by the side that made them.
#[repr(C)]
pub struct Buf {
    pub ptr: *mut u8,
    pub len: usize,
    pub cap: usize,
}

impl Buf {
    pub fn from_vec(v: Vec<u8>) -> Buf {
        let mut v = std::mem::ManuallyDrop::new(v);
        Buf { ptr: v.as_mut_ptr(), len: v.len(), cap: v.capacity() }
    }
    /// # Safety
    /// `self` must come from [`Buf::from_vec`] in the same library.
    pub unsafe fn into_vec(self) -> Vec<u8> {
        Vec::from_raw_parts(self.ptr, self.len, self.cap)
    }
    pub fn as_slice(&self) -> &[u8] {
        if self.ptr.is_null() {
            &[]
        } else {
            unsafe { std::slice::from_raw_parts(self.ptr, self.len) }
        }
    }
}

/// The table a plugin library exports.
#[repr(C)]
pub struct PluginTable {
    pub abi_version: u32,
    /// [`DATA_VERSION`] of the plugin's build (NUL-terminated).
    pub data_version: *const c_char,
    /// JSON describing the plugin and its filters ([`Description`]); free with `free_string`.
    pub describe: extern "C" fn() -> *mut c_char,
    /// Run filter `id` on `n` inputs (postcard bytes) with JSON settings; on success fills `out`
    /// (postcard bytes, free with `free_buf`) and returns 0, else sets `err` (free with
    /// `free_string`) and returns 1.
    pub run: extern "C" fn(id: *const c_char, params_json: *const c_char, inputs: *const Buf, n: usize, out: *mut Buf, err: *mut *mut c_char) -> i32,
    pub free_buf: extern "C" fn(Buf),
    pub free_string: extern "C" fn(*mut c_char),
}

// SAFETY: the table holds only function pointers and a pointer to a static string.
unsafe impl Sync for PluginTable {}

/// A setting of a filter, as the Properties panel shows it.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Param {
    pub key: String,
    pub label: String,
    /// "float", "int", "bool", "string", "choice", "vec3", "array".
    pub kind: String,
    pub default: serde_json::Value,
    #[serde(default)]
    pub min: Option<f64>,
    #[serde(default)]
    pub max: Option<f64>,
    #[serde(default)]
    pub choices: Vec<String>,
    #[serde(default)]
    pub help: String,
}

impl Param {
    fn new(key: &str, label: &str, kind: &str, default: serde_json::Value) -> Param {
        Param { key: key.into(), label: label.into(), kind: kind.into(), default, min: None, max: None, choices: vec![], help: String::new() }
    }
    pub fn float(key: &str, label: &str, default: f64) -> Param {
        Self::new(key, label, "float", default.into())
    }
    pub fn int(key: &str, label: &str, default: i64) -> Param {
        Self::new(key, label, "int", default.into())
    }
    pub fn bool(key: &str, label: &str, default: bool) -> Param {
        Self::new(key, label, "bool", default.into())
    }
    pub fn string(key: &str, label: &str, default: &str) -> Param {
        Self::new(key, label, "string", default.into())
    }
    pub fn vec3(key: &str, label: &str, default: [f64; 3]) -> Param {
        Self::new(key, label, "vec3", serde_json::json!(default))
    }
    /// One of `choices` (the first is the default).
    pub fn choice(key: &str, label: &str, choices: &[&str]) -> Param {
        let mut p = Self::new(key, label, "choice", choices.first().copied().unwrap_or("").into());
        p.choices = choices.iter().map(|c| c.to_string()).collect();
        p
    }
    /// The name of an array of the input (empty: the active array).
    pub fn array(key: &str, label: &str) -> Param {
        Self::new(key, label, "array", "".into())
    }
    pub fn range(mut self, min: f64, max: f64) -> Param {
        self.min = Some(min);
        self.max = Some(max);
        self
    }
    pub fn help(mut self, h: &str) -> Param {
        self.help = h.into();
        self
    }
}

/// What a plugin says about itself.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Description {
    pub name: String,
    pub filters: Vec<FilterDescription>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct FilterDescription {
    pub id: String,
    pub label: String,
    pub category: String,
    pub description: String,
    pub keywords: Vec<String>,
    /// A source makes data from nothing; a filter takes `inputs` inputs (1 or more).
    pub is_source: bool,
    pub inputs: usize,
    pub params: Vec<Param>,
}

/// The settings a filter is run with.
#[derive(Debug, Clone, Default)]
pub struct Params(pub serde_json::Map<String, serde_json::Value>);

impl Params {
    pub fn f64(&self, k: &str, default: f64) -> f64 {
        self.0.get(k).and_then(|v| v.as_f64()).unwrap_or(default)
    }
    pub fn i64(&self, k: &str, default: i64) -> i64 {
        self.0.get(k).and_then(|v| v.as_i64().or_else(|| v.as_f64().map(|f| f as i64))).unwrap_or(default)
    }
    pub fn bool(&self, k: &str, default: bool) -> bool {
        self.0.get(k).and_then(|v| v.as_bool()).unwrap_or(default)
    }
    pub fn str(&self, k: &str) -> String {
        self.0.get(k).and_then(|v| v.as_str()).unwrap_or("").to_string()
    }
    pub fn vec3(&self, k: &str, default: [f64; 3]) -> [f64; 3] {
        self.0
            .get(k)
            .and_then(|v| v.as_array())
            .filter(|a| a.len() == 3)
            .map(|a| [a[0].as_f64().unwrap_or(default[0]), a[1].as_f64().unwrap_or(default[1]), a[2].as_f64().unwrap_or(default[2])])
            .unwrap_or(default)
    }
}

pub type RunFn = fn(&[DataSet], &Params) -> Result<DataSet, String>;

/// A filter (or source) of a plugin.
pub struct Filter {
    pub desc: FilterDescription,
    pub run: RunFn,
}

impl Filter {
    /// A filter of one input.
    pub fn new(id: &str, label: &str, run: RunFn) -> Filter {
        Filter {
            desc: FilterDescription {
                id: id.into(),
                label: label.into(),
                category: "Plugins".into(),
                description: String::new(),
                keywords: vec![],
                is_source: false,
                inputs: 1,
                params: vec![],
            },
            run,
        }
    }
    /// A source (no input).
    pub fn source(id: &str, label: &str, run: RunFn) -> Filter {
        let mut f = Self::new(id, label, run);
        f.desc.is_source = true;
        f.desc.inputs = 0;
        f.desc.category = "Sources".into();
        f
    }
    pub fn describe(mut self, d: &str) -> Filter {
        self.desc.description = d.into();
        self
    }
    pub fn category(mut self, c: &str) -> Filter {
        self.desc.category = c.into();
        self
    }
    pub fn keywords(mut self, k: &[&str]) -> Filter {
        self.desc.keywords = k.iter().map(|s| s.to_string()).collect();
        self
    }
    /// Several inputs (Append-like filters).
    pub fn inputs(mut self, n: usize) -> Filter {
        self.desc.inputs = n.max(1);
        self
    }
    pub fn param(mut self, p: Param) -> Filter {
        self.desc.params.push(p);
        self
    }
}

/// Glue the macro uses: the plugin's filters, made once.
#[doc(hidden)]
pub struct Registry {
    pub name: &'static str,
    pub filters: Vec<Filter>,
}

#[doc(hidden)]
pub fn describe_json(r: &Registry) -> *mut c_char {
    let d = Description { name: r.name.into(), filters: r.filters.iter().map(|f| f.desc.clone()).collect() };
    CString::new(serde_json::to_string(&d).unwrap_or_default()).unwrap_or_default().into_raw()
}

#[doc(hidden)]
pub fn run_filter(r: &Registry, id: *const c_char, params: *const c_char, inputs: *const Buf, n: usize, out: *mut Buf, err: *mut *mut c_char) -> i32 {
    let fail = |msg: String| {
        unsafe { *err = CString::new(msg.replace('\0', " ")).unwrap_or_default().into_raw() };
        1
    };
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| -> Result<Vec<u8>, String> {
        let id = unsafe { CStr::from_ptr(id) }.to_string_lossy().into_owned();
        let pj = unsafe { CStr::from_ptr(params) }.to_string_lossy().into_owned();
        let f = r.filters.iter().find(|f| f.desc.id == id).ok_or_else(|| format!("no filter {id} in this plugin"))?;
        let map = match serde_json::from_str::<serde_json::Value>(&pj) {
            Ok(serde_json::Value::Object(m)) => m,
            _ => Default::default(),
        };
        let bufs = if n == 0 { &[][..] } else { unsafe { std::slice::from_raw_parts(inputs, n) } };
        let data: Vec<DataSet> =
            bufs.iter().map(|b| postcard::from_bytes::<DataSet>(b.as_slice()).map_err(|e| format!("reading the input: {e}"))).collect::<Result<_, _>>()?;
        let d = (f.run)(&data, &Params(map))?;
        postcard::to_allocvec(&d).map_err(|e| format!("writing the output: {e}"))
    }));
    match result {
        Ok(Ok(bytes)) => {
            unsafe { *out = Buf::from_vec(bytes) };
            0
        }
        Ok(Err(e)) => fail(e),
        Err(p) => fail(format!(
            "the plugin panicked: {}",
            p.downcast_ref::<String>().cloned().or_else(|| p.downcast_ref::<&str>().map(|s| s.to_string())).unwrap_or_default()
        )),
    }
}

#[doc(hidden)]
pub extern "C" fn free_buf(b: Buf) {
    if !b.ptr.is_null() {
        drop(unsafe { b.into_vec() });
    }
}

#[doc(hidden)]
pub extern "C" fn free_string(s: *mut c_char) {
    if !s.is_null() {
        drop(unsafe { CString::from_raw(s) });
    }
}

/// Postcard bytes of a dataset (the host side of the exchange).
pub fn encode(d: &DataSet) -> Result<Vec<u8>, String> {
    postcard::to_allocvec(d).map_err(|e| e.to_string())
}

pub fn decode(b: &[u8]) -> Result<DataSet, String> {
    postcard::from_bytes(b).map_err(|e| e.to_string())
}

/// Export a plugin: its name and its filters (see the crate's documentation).
#[macro_export]
macro_rules! export_plugin {
    (name: $name:expr, filters: [$($f:expr),* $(,)?] $(,)?) => {
        fn __aurafx_registry() -> &'static $crate::Registry {
            static R: std::sync::OnceLock<$crate::Registry> = std::sync::OnceLock::new();
            R.get_or_init(|| $crate::Registry { name: $name, filters: vec![$($f),*] })
        }
        extern "C" fn __aurafx_describe() -> *mut std::ffi::c_char {
            $crate::describe_json(__aurafx_registry())
        }
        extern "C" fn __aurafx_run(
            id: *const std::ffi::c_char,
            params: *const std::ffi::c_char,
            inputs: *const $crate::Buf,
            n: usize,
            out: *mut $crate::Buf,
            err: *mut *mut std::ffi::c_char,
        ) -> i32 {
            $crate::run_filter(__aurafx_registry(), id, params, inputs, n, out, err)
        }
        static __AURAFX_TABLE: $crate::PluginTable = $crate::PluginTable {
            abi_version: $crate::ABI_VERSION,
            data_version: $crate::DATA_VERSION.as_ptr() as *const std::ffi::c_char,
            describe: __aurafx_describe,
            run: __aurafx_run,
            free_buf: $crate::free_buf,
            free_string: $crate::free_string,
        };
        /// The plugin's table (looked up by auraFX).
        #[no_mangle]
        pub extern "C" fn aurafx_plugin_v1() -> *const $crate::PluginTable {
            &__AURAFX_TABLE
        }
    };
}
