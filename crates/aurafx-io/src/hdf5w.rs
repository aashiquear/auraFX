//! A small HDF5 writer: superblock version 2, version 2 object headers, groups with compact
//! link storage (hard and soft links), contiguous datasets of any numeric type, and numeric or
//! fixed-length string attributes. Enough for VTKHDF and similar formats; read back by the HDF5
//! library (h5py, VTK, ParaView) and by `hdf5-reader`.

use anyhow::{bail, Result};
use aurafx_data::ArrayData;

const UNDEF: u64 = u64::MAX;

/// Attribute values.
#[derive(Debug, Clone)]
pub enum AttrValue {
    Data(ArrayData),
    /// A fixed-length ASCII string (stored without a terminator, null-padded).
    Str(String),
}

#[derive(Debug, Clone)]
pub enum Node {
    Group {
        attrs: Vec<(String, AttrValue)>,
        children: Vec<(String, Node)>,
    },
    Dataset {
        attrs: Vec<(String, AttrValue)>,
        data: ArrayData,
        shape: Vec<u64>,
    },
    /// A soft link to an absolute path.
    SoftLink(String),
}

impl Node {
    pub fn group() -> Node {
        Node::Group { attrs: Vec::new(), children: Vec::new() }
    }

    pub fn dataset(data: ArrayData, shape: Vec<u64>) -> Node {
        Node::Dataset { attrs: Vec::new(), data, shape }
    }

    /// Add a child (groups only).
    pub fn add(&mut self, name: &str, child: Node) -> &mut Node {
        let Node::Group { children, .. } = self else { panic!("add() on a dataset") };
        children.push((name.to_string(), child));
        &mut children.last_mut().unwrap().1
    }

    pub fn attr(&mut self, name: &str, v: AttrValue) -> &mut Node {
        match self {
            Node::Group { attrs, .. } | Node::Dataset { attrs, .. } => attrs.push((name.to_string(), v)),
            Node::SoftLink(_) => {}
        }
        self
    }

    /// The child group `name`, created if missing.
    pub fn group_mut(&mut self, name: &str) -> &mut Node {
        let Node::Group { children, .. } = self else { panic!("group_mut() on a dataset") };
        if let Some(i) = children.iter().position(|(n, c)| n == name && matches!(c, Node::Group { .. })) {
            return &mut children[i].1;
        }
        children.push((name.to_string(), Node::group()));
        &mut children.last_mut().unwrap().1
    }
}

/// Datatype message bytes and element size.
fn datatype(d: &ArrayData) -> (Vec<u8>, usize) {
    let fixed = |size: u32, signed: bool| {
        let mut v = vec![0x10, if signed { 0x08 } else { 0x00 }, 0, 0];
        v.extend_from_slice(&size.to_le_bytes());
        v.extend_from_slice(&0u16.to_le_bytes()); // bit offset
        v.extend_from_slice(&((size * 8) as u16).to_le_bytes()); // precision
        (v, size as usize)
    };
    let float = |double: bool| {
        let (size, sign, exp_loc, exp_size, mant_size, bias): (u32, u8, u8, u8, u8, u32) =
            if double { (8, 63, 52, 11, 52, 1023) } else { (4, 31, 23, 8, 23, 127) };
        // Class 1, version 1; little-endian, mantissa normalized with implied 1.
        let mut v = vec![0x11, 0x20, sign, 0];
        v.extend_from_slice(&size.to_le_bytes());
        v.extend_from_slice(&0u16.to_le_bytes());
        v.extend_from_slice(&((size * 8) as u16).to_le_bytes());
        v.extend_from_slice(&[exp_loc, exp_size, 0, mant_size]);
        v.extend_from_slice(&bias.to_le_bytes());
        (v, size as usize)
    };
    match d {
        ArrayData::I8(_) => fixed(1, true),
        ArrayData::U8(_) => fixed(1, false),
        ArrayData::I16(_) => fixed(2, true),
        ArrayData::U16(_) => fixed(2, false),
        ArrayData::I32(_) => fixed(4, true),
        ArrayData::U32(_) => fixed(4, false),
        ArrayData::I64(_) => fixed(8, true),
        ArrayData::U64(_) => fixed(8, false),
        ArrayData::F32(_) | ArrayData::F16(_) => float(false),
        ArrayData::F64(_) | ArrayData::String(_) => float(true),
    }
}

fn string_type(len: usize) -> Vec<u8> {
    // Class 3 (string), version 1, null-padded ASCII.
    let mut v = vec![0x13, 0x01, 0, 0];
    v.extend_from_slice(&(len.max(1) as u32).to_le_bytes());
    v
}

/// Little-endian bytes of the values (F16 widened to f32; strings are not stored as data).
fn bytes(d: &ArrayData) -> Vec<u8> {
    let mut out = Vec::new();
    macro_rules! put {
        ($v:expr) => {
            for x in $v {
                out.extend_from_slice(&x.to_le_bytes());
            }
        };
    }
    match d {
        ArrayData::I8(v) => put!(v),
        ArrayData::U8(v) => put!(v),
        ArrayData::I16(v) => put!(v),
        ArrayData::U16(v) => put!(v),
        ArrayData::I32(v) => put!(v),
        ArrayData::U32(v) => put!(v),
        ArrayData::I64(v) => put!(v),
        ArrayData::U64(v) => put!(v),
        ArrayData::F16(v) => put!(v.iter().map(|x| x.to_f32())),
        ArrayData::F32(v) => put!(v),
        ArrayData::F64(v) => put!(v),
        ArrayData::String(_) => {}
    }
    out
}

fn dataspace(shape: &[u64]) -> Vec<u8> {
    // Version 2: simple (or scalar for an empty shape).
    let mut v = vec![2, shape.len() as u8, 0, if shape.is_empty() { 0 } else { 1 }];
    for d in shape {
        v.extend_from_slice(&d.to_le_bytes());
    }
    v
}

fn attribute(name: &str, value: &AttrValue) -> Vec<u8> {
    let (dt, space, data) = match value {
        AttrValue::Data(d) => (datatype(d).0, dataspace(&[d.len() as u64]), bytes(d)),
        AttrValue::Str(s) => (string_type(s.len()), dataspace(&[]), {
            let mut b = s.as_bytes().to_vec();
            if b.is_empty() {
                b.push(0);
            }
            b
        }),
    };
    let mut v = vec![3, 0];
    v.extend_from_slice(&((name.len() + 1) as u16).to_le_bytes());
    v.extend_from_slice(&(dt.len() as u16).to_le_bytes());
    v.extend_from_slice(&(space.len() as u16).to_le_bytes());
    v.push(0); // ASCII name
    v.extend_from_slice(name.as_bytes());
    v.push(0);
    v.extend_from_slice(&dt);
    v.extend_from_slice(&space);
    v.extend_from_slice(&data);
    v
}

fn link(name: &str, target: &LinkTarget) -> Vec<u8> {
    let long = name.len() > 255;
    let soft = matches!(target, LinkTarget::Soft(_));
    let mut flags = if long { 1u8 } else { 0 };
    if soft {
        flags |= 0x08; // link type present
    }
    let mut v = vec![1, flags];
    if soft {
        v.push(1);
    }
    if long {
        v.extend_from_slice(&(name.len() as u16).to_le_bytes());
    } else {
        v.push(name.len() as u8);
    }
    v.extend_from_slice(name.as_bytes());
    match target {
        LinkTarget::Hard(a) => v.extend_from_slice(&a.to_le_bytes()),
        LinkTarget::Soft(p) => {
            v.extend_from_slice(&(p.len() as u16).to_le_bytes());
            v.extend_from_slice(p.as_bytes());
        }
    }
    v
}

enum LinkTarget {
    Hard(u64),
    Soft(String),
}

/// A version 2 object header holding `messages` (type, body).
fn object_header(messages: &[(u8, Vec<u8>)]) -> Result<Vec<u8>> {
    let mut body = Vec::new();
    for (ty, m) in messages {
        if m.len() > u16::MAX as usize {
            bail!("HDF5 object header message too large ({} bytes)", m.len());
        }
        body.push(*ty);
        body.extend_from_slice(&(m.len() as u16).to_le_bytes());
        body.push(0);
        body.extend_from_slice(m);
    }
    let mut v = b"OHDR".to_vec();
    v.push(2);
    v.push(0x02); // chunk 0 size stored in 4 bytes
    v.extend_from_slice(&(body.len() as u32).to_le_bytes());
    v.extend_from_slice(&body);
    let sum = hdf5_reader::checksum::jenkins_lookup3(&v);
    v.extend_from_slice(&sum.to_le_bytes());
    Ok(v)
}

struct Writer {
    out: Vec<u8>,
}

impl Writer {
    fn put(&mut self, b: &[u8]) -> u64 {
        let a = self.out.len() as u64;
        self.out.extend_from_slice(b);
        a
    }

    /// Write a node (children first) and return its object header address.
    fn node(&mut self, n: &Node) -> Result<u64> {
        match n {
            Node::Dataset { attrs, data, shape } => {
                let raw = bytes(data);
                let addr = if raw.is_empty() { UNDEF } else { self.put(&raw) };
                let mut layout = vec![3, 1];
                layout.extend_from_slice(&addr.to_le_bytes());
                layout.extend_from_slice(&(raw.len() as u64).to_le_bytes());
                let mut msgs = vec![
                    (0x01, dataspace(shape)),
                    (0x03, datatype(data).0),
                    // Fill value: allocated early, never written, none defined.
                    (0x05, vec![3, 0x05]),
                    (0x08, layout),
                ];
                for (k, v) in attrs {
                    msgs.push((0x0C, attribute(k, v)));
                }
                let oh = object_header(&msgs)?;
                Ok(self.put(&oh))
            }
            Node::Group { attrs, children } => {
                let mut links = Vec::new();
                for (name, c) in children {
                    let t = match c {
                        Node::SoftLink(p) => LinkTarget::Soft(p.clone()),
                        c => LinkTarget::Hard(self.node(c)?),
                    };
                    links.push(link(name, &t));
                }
                let mut li = vec![0, 0];
                li.extend_from_slice(&UNDEF.to_le_bytes());
                li.extend_from_slice(&UNDEF.to_le_bytes());
                let mut msgs = vec![(0x02, li), (0x0A, vec![0, 0])];
                for l in links {
                    msgs.push((0x06, l));
                }
                for (k, v) in attrs {
                    msgs.push((0x0C, attribute(k, v)));
                }
                let oh = object_header(&msgs)?;
                Ok(self.put(&oh))
            }
            Node::SoftLink(_) => bail!("a soft link can't be written on its own"),
        }
    }
}

/// Write a file whose root group is `root`.
pub fn write(path: &std::path::Path, root: &Node) -> Result<()> {
    let mut w = Writer { out: vec![0u8; 48] };
    let root_addr = w.node(root)?;
    let eof = w.out.len() as u64;
    let mut sb = b"\x89HDF\r\n\x1a\n".to_vec();
    sb.extend_from_slice(&[2, 8, 8, 0]);
    sb.extend_from_slice(&0u64.to_le_bytes()); // base address
    sb.extend_from_slice(&UNDEF.to_le_bytes()); // no superblock extension
    sb.extend_from_slice(&eof.to_le_bytes());
    sb.extend_from_slice(&root_addr.to_le_bytes());
    let sum = hdf5_reader::checksum::jenkins_lookup3(&sb);
    sb.extend_from_slice(&sum.to_le_bytes());
    w.out[..48].copy_from_slice(&sb);
    std::fs::write(path, &w.out)?;
    Ok(())
}
