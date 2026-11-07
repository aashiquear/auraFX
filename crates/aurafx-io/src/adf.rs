//! ADF ("Advanced Data Format"), the original CGNS storage before HDF5. A file is a tree of
//! nodes in 4096-byte blocks. Each node has a 246-byte header: name, label, data type,
//! dimensions, and pointers to its child table and its data chunks. Two pointer encodings
//! exist. Old files store a block as 8 hex characters and an offset as 4. Newer files store an
//! 8-byte block number and a 4-byte offset in the file's byte order. Dimensions follow the same
//! rule: 8 hex characters each, or 8-byte integers. Counts are always hex text. Data is in the
//! file's byte order ('L' little, 'B' big IEEE). Links are nodes of type "LK" whose data reads
//! "file>path"; an empty file name means this file.

use anyhow::{anyhow, bail, Context, Result};
use aurafx_data::ArrayData;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

const BLOCK: u64 = 4096;
const HEADER: usize = 246;

pub(crate) struct AdfFile {
    f: Mutex<File>,
    path: PathBuf,
    /// Pointers and dimensions as hex text (old files) instead of binary.
    hex: bool,
    big: bool,
    root: u64,
}

#[derive(Clone)]
pub(crate) struct AdfNode {
    file: Arc<AdfFile>,
    pub name: String,
    pub label: String,
    pub dtype: String,
    /// Dimensions as stored (Fortran order: fastest first).
    pub dims: Vec<u64>,
    nsub: usize,
    table: Option<u64>,
    nchunks: usize,
    chunks: Option<u64>,
}

/// Whether `head` (the first bytes of a file) starts an ADF file.
pub(crate) fn is_adf(head: &[u8]) -> bool {
    head.len() >= 25 && &head[4..25] == b"ADF Database Version "
}

fn hexnum(b: &[u8]) -> Result<u64> {
    let s = std::str::from_utf8(b).map_err(|_| anyhow!("bad hex field"))?.trim();
    if s.is_empty() {
        return Ok(0);
    }
    u64::from_str_radix(s, 16).map_err(|_| anyhow!("bad hex field {s:?}"))
}

fn text(b: &[u8]) -> String {
    String::from_utf8_lossy(b).trim_end_matches(['\0', ' ']).trim_start().to_string()
}

impl AdfFile {
    pub(crate) fn open(path: &Path) -> Result<Arc<AdfFile>> {
        let mut f = File::open(path).with_context(|| format!("opening {}", path.display()))?;
        let mut h = [0u8; 186];
        f.read_exact(&mut h).context("ADF file header")?;
        if !is_adf(&h) || &h[32..36] != b"AdF0" || &h[130..134] != b"AdF4" {
            bail!("{} is not an ADF file", path.display());
        }
        let big = match h[100] {
            b'L' | b'l' => false,
            b'B' | b'b' => true,
            b'N' | b'n' => cfg!(target_endian = "big"),
            c => bail!("ADF numeric format {:?} (Cray or other non-IEEE data) is not supported", c as char),
        };
        let p = &h[134..146];
        // As the CGNS library decides: only version "B" files use binary pointers.
        let hex = h[25] != b'B';
        let mut file = AdfFile { f: Mutex::new(f), path: path.to_path_buf(), hex, big, root: 0 };
        file.root = file.ptr(p).ok_or_else(|| anyhow!("ADF file has no root node"))?;
        Ok(Arc::new(file))
    }

    fn read(&self, pos: u64, len: usize) -> Result<Vec<u8>> {
        let mut f = self.f.lock().unwrap();
        f.seek(SeekFrom::Start(pos))?;
        let mut b = vec![0u8; len];
        f.read_exact(&mut b).with_context(|| format!("reading {len} bytes at {pos} of {}", self.path.display()))?;
        Ok(b)
    }

    fn uint(&self, b: &[u8]) -> u64 {
        let mut v = [0u8; 8];
        if self.big {
            v[8 - b.len()..].copy_from_slice(b);
            u64::from_be_bytes(v)
        } else {
            v[..b.len()].copy_from_slice(b);
            u64::from_le_bytes(v)
        }
    }

    /// A 12-byte disk pointer as an absolute position (None for the null pointer).
    fn ptr(&self, b: &[u8]) -> Option<u64> {
        let (block, off) = if self.hex { (hexnum(&b[..8]).ok()?, hexnum(&b[8..12]).ok()?) } else { (self.uint(&b[..8]), self.uint(&b[8..12])) };
        if off >= BLOCK {
            return None;
        }
        Some(block * BLOCK + off)
    }

    fn node(self: &Arc<Self>, pos: u64) -> Result<AdfNode> {
        let h = self.read(pos, HEADER)?;
        if &h[..4] != b"NoDe" || &h[242..246] != b"TaiL" {
            bail!("no ADF node at {pos}");
        }
        let ndims = hexnum(&h[128..130])? as usize;
        if ndims > 12 {
            bail!("ADF node with {ndims} dimensions");
        }
        let dims = (0..ndims)
            .map(|i| {
                let b = &h[130 + 8 * i..138 + 8 * i];
                if self.hex {
                    hexnum(b).unwrap_or(0)
                } else {
                    self.uint(b)
                }
            })
            .collect();
        Ok(AdfNode {
            file: self.clone(),
            name: text(&h[4..36]),
            label: text(&h[36..68]),
            nsub: hexnum(&h[68..76])? as usize,
            table: self.ptr(&h[84..96]),
            dtype: text(&h[96..128]).to_ascii_uppercase(),
            dims,
            nchunks: hexnum(&h[226..230])? as usize,
            chunks: self.ptr(&h[230..242]),
        })
    }

    pub(crate) fn root(self: &Arc<Self>) -> Result<AdfNode> {
        self.node(self.root)
    }
}

impl AdfNode {
    /// The children, with links replaced by the nodes they point to (named as the link).
    pub(crate) fn children(&self) -> Vec<AdfNode> {
        self.children_depth(0)
    }

    fn children_depth(&self, depth: usize) -> Vec<AdfNode> {
        let (Some(table), true) = (self.table, self.nsub > 0) else { return Vec::new() };
        let Ok(head) = self.file.read(table, 16) else { return Vec::new() };
        let Some(end) = self.file.ptr(&head[4..16]) else { return Vec::new() };
        let len = end.saturating_sub(table + 16) as usize;
        let Ok(t) = self.file.read(table + 16, len.min(44 * 1_000_000)) else { return Vec::new() };
        let mut out = Vec::new();
        for e in t.chunks_exact(44) {
            if out.len() == self.nsub {
                break;
            }
            if text(&e[..32]) == "unused entry in sub-node-table" {
                continue;
            }
            let Some(pos) = self.file.ptr(&e[32..44]) else { continue };
            match self.file.node(pos) {
                Ok(n) if n.dtype == "LK" => match n.follow(depth) {
                    Ok(mut t) => {
                        t.name = n.name;
                        out.push(t)
                    }
                    Err(e) => tracing::warn!("ADF link {}: {e:#}", n.name),
                },
                Ok(n) => out.push(n),
                Err(e) => tracing::warn!("ADF: {e:#}"),
            }
        }
        out
    }

    /// The node a link points to.
    fn follow(&self, depth: usize) -> Result<AdfNode> {
        if depth > 16 {
            bail!("links nested too deeply");
        }
        let raw = self.raw()?;
        let s = String::from_utf8_lossy(&raw).trim_end_matches(['\0', ' ']).to_string();
        let (file, path) = s.split_once('>').unwrap_or(("", s.as_str()));
        let start = if file.trim().is_empty() {
            self.file.root()?
        } else {
            let p = self.file.path.parent().unwrap_or(Path::new(".")).join(file.trim());
            AdfFile::open(&p).with_context(|| format!("linked file {}", p.display()))?.root()?
        };
        let mut n = start;
        for part in path.split('/').filter(|p| !p.is_empty()) {
            let next = n.children_depth(depth + 1).into_iter().find(|c| c.name == part);
            n = next.ok_or_else(|| anyhow!("{path}: no node {part}"))?;
        }
        if n.dtype == "LK" {
            return n.follow(depth + 1);
        }
        Ok(n)
    }

    /// The node's bytes, joined across its data chunks.
    fn raw(&self) -> Result<Vec<u8>> {
        let Some(at) = self.chunks.filter(|_| self.nchunks > 0) else { bail!("{} has no data", self.name) };
        let chunk = |start: u64| -> Result<Vec<u8>> {
            let head = self.file.read(start, 16)?;
            if &head[..4] != b"DaTa" {
                bail!("{}: bad data chunk", self.name);
            }
            let end = self.file.ptr(&head[4..16]).ok_or_else(|| anyhow!("{}: bad data chunk end", self.name))?;
            self.file.read(start + 16, end.saturating_sub(start + 16) as usize)
        };
        if self.nchunks == 1 {
            return chunk(at);
        }
        // A table of (start, end) pointers to the chunks, after a tag and an end pointer.
        let t = self.file.read(at + 16, 24 * self.nchunks)?;
        let mut out = Vec::new();
        for e in t.chunks_exact(24) {
            let start = self.file.ptr(&e[..12]).ok_or_else(|| anyhow!("{}: bad chunk table", self.name))?;
            out.extend(chunk(start)?);
        }
        Ok(out)
    }

    /// The values and the shape in C order (slowest first), as HDF5 storage would give them.
    pub(crate) fn data(&self) -> Result<(ArrayData, Vec<usize>)> {
        let size = match self.dtype.as_str() {
            "I4" | "U4" | "R4" => 4,
            "I8" | "U8" | "R8" => 8,
            "C1" | "B1" => 1,
            "MT" => bail!("{} has no data", self.name),
            t => bail!("{}: ADF data type {t} is not supported", self.name),
        };
        let n: u64 = self.dims.iter().product();
        let mut raw = self.raw()?;
        let want = (n as usize) * size;
        if raw.len() < want {
            bail!("{}: {} bytes of data, expected {want}", self.name, raw.len());
        }
        raw.truncate(want);
        let big = self.file.big;
        macro_rules! nums {
            ($t:ty, $v:ident) => {{
                const W: usize = std::mem::size_of::<$t>();
                ArrayData::$v(
                    raw.chunks_exact(W)
                        .map(|c| {
                            let b: [u8; W] = c.try_into().unwrap();
                            if big {
                                <$t>::from_be_bytes(b)
                            } else {
                                <$t>::from_le_bytes(b)
                            }
                        })
                        .collect(),
                )
            }};
        }
        let d = match self.dtype.as_str() {
            "I4" => nums!(i32, I32),
            "U4" => nums!(u32, U32),
            "R4" => nums!(f32, F32),
            "I8" => nums!(i64, I64),
            "U8" => nums!(u64, U64),
            "R8" => nums!(f64, F64),
            _ => ArrayData::U8(raw),
        };
        Ok((d, self.dims.iter().rev().map(|&d| d as usize).collect()))
    }
}
