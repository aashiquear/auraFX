//! CAD files in and out (stage C6). Documents are `.afxcad`; solids are read as STEP AP242 /
//! AP214 / AP203, IGES, 3MF, SolidCraft `.sc` designs, STL and OBJ (the file becomes the *base
//! feature* of a document, its bytes kept inside so the document is self-contained) and written
//! as STEP, IGES, 3MF, STL, OBJ and `.sc` by the vendored engine's exact writers. A part that only the
//! implicit kernel can build is written as STEP or STL from its tessellation: a faceted B-rep of
//! triangles, labelled as such in its header (IGES is written only for a part the exact kernel builds).

use crate::kernel::{brep, Evaluated, KernelChoice};
use crate::{CadError, Document, Feature, Shape};
use aurafx_data::DataSet;
use std::path::Path;

/// File extensions `import_file` reads.
pub const IMPORT_EXTENSIONS: &[&str] = &["afxcad", "step", "stp", "igs", "iges", "3mf", "sc", "stl", "obj"];

pub fn is_cad_file(path: &Path) -> bool {
    path.extension().and_then(|e| e.to_str()).is_some_and(|e| IMPORT_EXTENSIONS.contains(&e.to_lowercase().as_str()))
}

fn ioerr(path: &Path, e: impl std::fmt::Display) -> CadError {
    CadError::Edit(format!("{}: {e}", path.display()))
}

/// Read a CAD file as a document: an `.afxcad` as it is, a solid file as a base feature.
pub fn import_file(path: &Path) -> Result<Document, CadError> {
    use base64::Engine as _;
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("").to_lowercase();
    if ext == "afxcad" {
        return Document::load(path).map_err(|e| CadError::Edit(e.to_string()));
    }
    let format = match ext.as_str() {
        "step" | "stp" => "step",
        "igs" | "iges" => "igs",
        "3mf" | "sc" | "stl" | "obj" => ext.as_str(),
        other => return Err(CadError::Edit(format!("'.{other}' is not a CAD file auraFX reads (STEP, IGES, 3MF, SolidCraft .sc, STL, OBJ, .afxcad)"))),
    };
    let bytes = std::fs::read(path).map_err(|e| ioerr(path, e))?;
    let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("Import").to_string();
    let shape = Shape::Brep {
        format: format.to_string(),
        source: path.file_name().and_then(|s| s.to_str()).unwrap_or("").to_string(),
        data: base64::engine::general_purpose::STANDARD.encode(bytes),
    };
    let doc = Document::new(&stem).add(Feature::new(&stem, crate::BooleanOp::Add, shape));
    // The file must build, or the document would only show an error.
    crate::kernel::Kernel::evaluate(&brep::BrepKernel, &doc).map_err(|e| CadError::Edit(format!("{}: {e}", path.display())))?;
    Ok(doc)
}

/// What a write did.
#[derive(Debug, Clone)]
pub struct ExportReport {
    pub bytes: u64,
    /// Written by the exact kernel's own writer (false: a faceted tessellation).
    pub exact: bool,
    pub note: String,
}

/// Write the document as `.afxcad`, `.step`/`.stp`, `.igs`/`.iges`, `.3mf`, `.stl`, `.obj` or `.sc`, by the
/// path's extension.
pub fn export_file(doc: &Document, path: &Path, choice: KernelChoice) -> Result<ExportReport, CadError> {
    export_file_with(doc, path, choice, 48)
}

/// [`export_file`] with the number of cells along the longest side of a part that is written from
/// its tessellation (a part the exact kernel builds is written exactly whatever this is).
pub fn export_file_with(doc: &Document, path: &Path, choice: KernelChoice, faceted_resolution: usize) -> Result<ExportReport, CadError> {
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("").to_lowercase();
    if ext == "afxcad" {
        doc.save(path).map_err(|e| CadError::Edit(e.to_string()))?;
        return Ok(ExportReport { bytes: std::fs::metadata(path).map(|m| m.len()).unwrap_or(0), exact: true, note: "the document".into() });
    }
    let exact = if choice == KernelChoice::Implicit {
        Err(CadError::KernelRefused("brep", "the implicit kernel was chosen".into()))
    } else {
        brep::export_exact(doc, path)
    };
    match exact {
        Ok(bytes) => Ok(ExportReport { bytes, exact: true, note: "exact faces and edges".into() }),
        Err(e @ (CadError::KernelRefused(..) | CadError::Kernel { .. }))
            if matches!(ext.as_str(), "step" | "stp" | "stl" | "obj") && choice != KernelChoice::Brep =>
        {
            let ev = crate::kernel::implicit::ImplicitKernel { resolution: faceted_resolution, project: true }.evaluate_doc(doc)?;
            let DataSet::PolyData(pd) = &ev.surface else { return Err(e) };
            let pts: Vec<[f64; 3]> = (0..pd.points.len()).map(|i| pd.points.get(i)).collect();
            let tris: Vec<[u32; 3]> = pd.polys.iter().filter(|(_, c)| c.len() == 3).map(|(_, c)| [c[0], c[1], c[2]]).collect();
            let text = match ext.as_str() {
                "step" | "stp" => step_faceted(&doc.name, &pts, &tris, &e.to_string()).into_bytes(),
                "stl" => stl_binary(&pts, &tris),
                _ => obj_text(&pts, &tris).into_bytes(),
            };
            std::fs::write(path, &text).map_err(|x| ioerr(path, x))?;
            Ok(ExportReport { bytes: text.len() as u64, exact: false, note: format!("faceted: {e}") })
        }
        Err(e @ (CadError::KernelRefused(..) | CadError::Kernel { .. })) if matches!(ext.as_str(), "igs" | "iges") => Err(CadError::KernelRefused(
            "brep",
            format!("an IGES file needs the exact kernel to build the part ({e}); write STEP instead, which can hold a faceted part"),
        )),
        Err(e) => Err(e),
    }
}

fn stl_binary(pts: &[[f64; 3]], tris: &[[u32; 3]]) -> Vec<u8> {
    let mut out = vec![0u8; 80];
    out[..24].copy_from_slice(b"auraFX CAD (tessellated)");
    out.extend((tris.len() as u32).to_le_bytes());
    for t in tris {
        let p: Vec<[f64; 3]> = t.iter().map(|i| pts[*i as usize]).collect();
        let (u, v) = ([p[1][0] - p[0][0], p[1][1] - p[0][1], p[1][2] - p[0][2]], [p[2][0] - p[0][0], p[2][1] - p[0][1], p[2][2] - p[0][2]]);
        let n = [u[1] * v[2] - u[2] * v[1], u[2] * v[0] - u[0] * v[2], u[0] * v[1] - u[1] * v[0]];
        let l = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt().max(1e-300);
        for x in [n[0] / l, n[1] / l, n[2] / l] {
            out.extend((x as f32).to_le_bytes());
        }
        for q in &p {
            for x in q {
                out.extend((*x as f32).to_le_bytes());
            }
        }
        out.extend([0u8, 0]);
    }
    out
}

fn obj_text(pts: &[[f64; 3]], tris: &[[u32; 3]]) -> String {
    let mut s = String::from("# auraFX CAD (tessellated)\n");
    for p in pts {
        s += &format!("v {} {} {}\n", p[0], p[1], p[2]);
    }
    for t in tris {
        s += &format!("f {} {} {}\n", t[0] + 1, t[1] + 1, t[2] + 1);
    }
    s
}

/// A STEP file (AP242) with one `MANIFOLD_SOLID_BREP` whose faces are the triangles of a
/// tessellation, each an `ADVANCED_FACE` on its own `PLANE` bounded by straight edges shared
/// between neighbours, in millimetres. The header says it is faceted and why; any STEP reader
/// takes it, and the exact kernel measures what the tessellation measures.
pub fn step_faceted(name: &str, pts: &[[f64; 3]], tris: &[[u32; 3]], why: &str) -> String {
    use std::collections::BTreeMap;
    let safe = |s: &str| s.replace('\'', "").replace('\n', " ");
    let f = |x: f64| format!("{x:.9}");
    let mut s = String::new();
    s += "ISO-10303-21;\nHEADER;\n";
    s += &format!("FILE_DESCRIPTION(('auraFX faceted B-rep: {}'),'2;1');\n", safe(why).chars().take(120).collect::<String>());
    s += &format!("FILE_NAME('{}.step','',(''),(''),'auraFX','auraFX','');\n", safe(name));
    s += "FILE_SCHEMA(('AP242_MANAGED_MODEL_BASED_3D_ENGINEERING_MIM_LF { 1 0 10303 442 1 1 4 }'));\nENDSEC;\nDATA;\n";
    let mut id = 0usize;
    let mut add = |s: &mut String, body: String| -> usize {
        id += 1;
        *s += &format!("#{id}={body};\n");
        id
    };
    let app = add(&mut s, "APPLICATION_CONTEXT('managed model based 3d engineering')".into());
    add(&mut s, format!("APPLICATION_PROTOCOL_DEFINITION('international standard','ap242_managed_model_based_3d_engineering',2014,#{app})"));
    let pc = add(&mut s, format!("PRODUCT_CONTEXT('',#{app},'mechanical')"));
    let pdc = add(&mut s, format!("PRODUCT_DEFINITION_CONTEXT('part definition',#{app},'design')"));
    let len = add(&mut s, "(LENGTH_UNIT()NAMED_UNIT(*)SI_UNIT(.MILLI.,.METRE.))".into());
    let ang = add(&mut s, "(NAMED_UNIT(*)PLANE_ANGLE_UNIT()SI_UNIT($,.RADIAN.))".into());
    let sa = add(&mut s, "(NAMED_UNIT(*)SI_UNIT($,.STERADIAN.)SOLID_ANGLE_UNIT())".into());
    let unc = add(&mut s, format!("UNCERTAINTY_MEASURE_WITH_UNIT(LENGTH_MEASURE(2.E-5),#{len},'distance_accuracy_value','confusion accuracy')"));
    let ctx = add(&mut s, format!("(GEOMETRIC_REPRESENTATION_CONTEXT(3)GLOBAL_UNCERTAINTY_ASSIGNED_CONTEXT((#{unc}))GLOBAL_UNIT_ASSIGNED_CONTEXT((#{len},#{ang},#{sa}))REPRESENTATION_CONTEXT('Context #1','3D Context with UNIT and UNCERTAINTY'))"));
    let org = add(&mut s, "CARTESIAN_POINT('',(0.0,0.0,0.0))".into());
    let dx = add(&mut s, "DIRECTION('',(1.0,0.0,0.0))".into());
    let dz = add(&mut s, "DIRECTION('',(0.0,0.0,1.0))".into());
    let ax = add(&mut s, format!("AXIS2_PLACEMENT_3D('',#{org},#{dz},#{dx})"));
    let prod = add(&mut s, format!("PRODUCT('{0}','{0}','',(#{pc}))", safe(name)));
    let form = add(&mut s, format!("PRODUCT_DEFINITION_FORMATION('','',#{prod})"));
    let pd = add(&mut s, format!("PRODUCT_DEFINITION('design','',#{form},#{pdc})"));
    let pds = add(&mut s, format!("PRODUCT_DEFINITION_SHAPE('','',#{pd})"));
    // Points and vertices.
    let pt: Vec<usize> = pts.iter().map(|p| add(&mut s, format!("CARTESIAN_POINT('',({},{},{}))", f(p[0]), f(p[1]), f(p[2])))).collect();
    let vx: Vec<usize> = pt.iter().map(|p| add(&mut s, format!("VERTEX_POINT('',#{p})"))).collect();
    let sub = |a: [f64; 3], b: [f64; 3]| [a[0] - b[0], a[1] - b[1], a[2] - b[2]];
    let len3 = |a: [f64; 3]| (a[0] * a[0] + a[1] * a[1] + a[2] * a[2]).sqrt();
    // Straight edges, shared by neighbouring triangles.
    let mut edges: BTreeMap<(u32, u32), usize> = BTreeMap::new();
    let mut faces: Vec<usize> = Vec::new();
    for t in tris {
        let (a, b, c) = (pts[t[0] as usize], pts[t[1] as usize], pts[t[2] as usize]);
        let (u, v) = (sub(b, a), sub(c, a));
        let n = [u[1] * v[2] - u[2] * v[1], u[2] * v[0] - u[0] * v[2], u[0] * v[1] - u[1] * v[0]];
        let (ln, lu) = (len3(n), len3(u));
        if ln < 1e-12 || lu < 1e-12 {
            continue;
        }
        let mut oriented = Vec::new();
        for (p, q) in [(t[0], t[1]), (t[1], t[2]), (t[2], t[0])] {
            let (lo, hi) = (p.min(q), p.max(q));
            let e = match edges.get(&(lo, hi)) {
                Some(e) => *e,
                None => {
                    let d = sub(pts[hi as usize], pts[lo as usize]);
                    let l = len3(d).max(1e-300);
                    let dir = add(&mut s, format!("DIRECTION('',({},{},{}))", f(d[0] / l), f(d[1] / l), f(d[2] / l)));
                    let vec = add(&mut s, format!("VECTOR('',#{dir},{})", f(l)));
                    let line = add(&mut s, format!("LINE('',#{},#{vec})", pt[lo as usize]));
                    let e = add(&mut s, format!("EDGE_CURVE('',#{},#{},#{line},.T.)", vx[lo as usize], vx[hi as usize]));
                    edges.insert((lo, hi), e);
                    e
                }
            };
            oriented.push(add(&mut s, format!("ORIENTED_EDGE('',*,*,#{e},{})", if p < q { ".T." } else { ".F." })));
        }
        let loop_id = add(&mut s, format!("EDGE_LOOP('',({}))", oriented.iter().map(|o| format!("#{o}")).collect::<Vec<_>>().join(",")));
        let bound = add(&mut s, format!("FACE_OUTER_BOUND('',#{loop_id},.T.)"));
        let nd = add(&mut s, format!("DIRECTION('',({},{},{}))", f(n[0] / ln), f(n[1] / ln), f(n[2] / ln)));
        let rd = add(&mut s, format!("DIRECTION('',({},{},{}))", f(u[0] / lu), f(u[1] / lu), f(u[2] / lu)));
        let ap = add(&mut s, format!("AXIS2_PLACEMENT_3D('',#{},#{nd},#{rd})", pt[t[0] as usize]));
        let plane = add(&mut s, format!("PLANE('',#{ap})"));
        faces.push(add(&mut s, format!("ADVANCED_FACE('',(#{bound}),#{plane},.T.)")));
    }
    let shell = add(&mut s, format!("CLOSED_SHELL('',({}))", faces.iter().map(|x| format!("#{x}")).collect::<Vec<_>>().join(",")));
    let solid = add(&mut s, format!("MANIFOLD_SOLID_BREP('{}',#{shell})", safe(name)));
    let rep = add(&mut s, format!("ADVANCED_BREP_SHAPE_REPRESENTATION('{}',(#{solid},#{ax}),#{ctx})", safe(name)));
    add(&mut s, format!("SHAPE_DEFINITION_REPRESENTATION(#{pds},#{rep})"));
    s += "ENDSEC;\nEND-ISO-10303-21;\n";
    s
}

impl crate::kernel::implicit::ImplicitKernel {
    /// Evaluate with this kernel (a convenience that does not need the trait in scope).
    pub fn evaluate_doc(&self, doc: &Document) -> Result<Evaluated, CadError> {
        crate::kernel::Kernel::evaluate(self, doc)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::edit::apply_op;
    use crate::kernel::Kernel;
    use serde_json::json;

    fn tmp(name: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("afxcad-io-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        d.join(name)
    }
    fn part() -> Document {
        let mut d = Document::new("plate");
        apply_op(
            &mut d,
            "sketch_extrude",
            &json!({"name": "Plate", "depth": 10, "sketch": {"plane": "XY", "items": [{"rectangle": {"center": [0, 0], "w": 40, "h": 30}}]}}),
        )
        .unwrap();
        apply_op(&mut d, "add", &json!({"kind": "hole", "name": "Bore", "diameter": 8, "depth": 10, "at": [5, 3, 10]})).unwrap();
        d
    }
    fn volume(d: &Document) -> f64 {
        brep::BrepKernel.evaluate(d).unwrap().volume()
    }

    #[test]
    fn step_written_here_reads_back_with_the_same_volume_and_becomes_a_base_feature() {
        let d = part();
        let p = tmp("plate.step");
        let r = export_file(&d, &p, KernelChoice::Auto).unwrap();
        assert!(r.exact && r.bytes > 1000, "{r:?}");
        let text = std::fs::read_to_string(&p).unwrap();
        assert!(text.starts_with("ISO-10303-21;") && text.contains("ADVANCED_FACE") || text.contains("MANIFOLD_SOLID_BREP"), "a STEP B-rep");
        let back = import_file(&p).unwrap();
        assert!(matches!(back.features[0].shape, Shape::Brep { .. }));
        let (v0, v1) = (volume(&d), volume(&back));
        assert!((v0 - v1).abs() / v0 < 1e-3, "{v0} vs {v1}");
        // The imported solid is the base of a part: cut a hole through it.
        let mut cut = back.clone();
        apply_op(&mut cut, "add", &json!({"kind": "hole", "name": "Pin hole", "diameter": 6, "depth": 10, "at": [-10, 0, 10]})).unwrap();
        let v2 = volume(&cut);
        assert!((v1 - v2 - std::f64::consts::PI * 9.0 * 10.0).abs() < 2.0, "{v1} -> {v2}");
        // The implicit kernel reads it through its tessellation.
        let imp = crate::kernel::implicit::ImplicitKernel::new(64).evaluate(&back).unwrap();
        assert!((imp.volume() - v1).abs() / v1 < 0.02, "{} vs {v1}", imp.volume());
    }

    #[test]
    fn three_mf_sc_stl_and_afxcad_round_trip() {
        let d = part();
        let v = volume(&d);
        for ext in ["3mf", "sc", "stl"] {
            let p = tmp(&format!("plate.{ext}"));
            let r = export_file(&d, &p, KernelChoice::Auto).unwrap();
            assert!(r.exact && r.bytes > 100, "{ext}: {r:?}");
            let back = import_file(&p).unwrap_or_else(|e| panic!("{ext}: {e}"));
            let vb = volume(&back);
            assert!((vb - v).abs() / v < 0.01, "{ext}: {vb} vs {v}");
        }
        let p = tmp("plate.afxcad");
        export_file(&d, &p, KernelChoice::Auto).unwrap();
        assert_eq!(import_file(&p).unwrap(), d);
        // IGES is written too (see an_exact_part_is_written_as_iges_and_reads_back_the_same).
        assert!(export_file(&d, &tmp("plate.igs"), KernelChoice::Auto).unwrap().exact);
        assert!(import_file(&tmp("plate.xyz")).unwrap_err().to_string().contains("not a CAD file"));
    }

    #[test]
    fn a_part_only_the_implicit_kernel_builds_is_written_as_a_faceted_step_that_reads_back() {
        let mut d = Document::new("rod");
        apply_op(&mut d, "add", &json!({"kind": "box", "name": "Base", "size": [30, 30, 6], "at": [0, 0, 3]})).unwrap();
        // A threaded stub on the base: a modelled thread the exact kernel does not build.
        apply_op(&mut d, "add", &json!({"kind": "thread", "name": "Stub", "major_diameter": 8, "pitch": 2.5, "length": 10, "at": [0, 0, 6]})).unwrap();
        apply_op(&mut d, "add", &json!({"kind": "cylinder", "name": "Core", "radius": 3.0, "height": 10, "at": [0, 0, 6]})).unwrap();
        let p = tmp("rod.step");
        let r = export_file_with(&d, &p, KernelChoice::Auto, 14).unwrap();
        assert!(!r.exact && r.note.contains("faceted"), "{r:?}");
        let text = std::fs::read_to_string(&p).unwrap();
        assert!(text.contains("MANIFOLD_SOLID_BREP") && text.contains("faceted B-rep"));
        // The exact kernel reads the file and measures what the tessellation measures.
        let back = import_file(&p).unwrap();
        let imp = crate::kernel::implicit::ImplicitKernel::new(14).evaluate(&d).unwrap();
        let vb = volume(&back);
        assert!((vb - imp.volume()).abs() / imp.volume() < 0.01, "{vb} vs {}", imp.volume());
        let fine = crate::kernel::implicit::ImplicitKernel::new(96).evaluate(&d).unwrap().volume();
        assert!((vb - fine).abs() / fine < 0.08, "coarse tessellation {vb} vs fine {fine}");
        // Choosing the exact kernel for a part it cannot build is an error, not a faceted file.
        assert!(export_file(&d, &tmp("rod2.step"), KernelChoice::Brep).is_err());
    }

    /// `testdata/cad/`: small files written by this code (`cargo test -p aurafx-cad io::tests::write_testdata -- --ignored`).
    fn testdata_dir() -> std::path::PathBuf {
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../testdata/cad")
    }

    #[test]
    #[ignore = "writes testdata/cad (see testdata/README.md)"]
    fn write_testdata() {
        let dir = testdata_dir();
        std::fs::create_dir_all(&dir).unwrap();
        let d = part();
        for ext in ["afxcad", "step", "3mf", "sc"] {
            export_file(&d, &dir.join(format!("plate_with_hole.{ext}")), KernelChoice::Auto).unwrap();
        }
    }

    #[test]
    fn the_committed_files_read_back_to_the_same_part() {
        let dir = testdata_dir();
        let v = volume(&part());
        for ext in ["step", "3mf", "sc"] {
            let p = dir.join(format!("plate_with_hole.{ext}"));
            if !p.exists() {
                continue; // tests that need a missing file skip themselves
            }
            let back = import_file(&p).unwrap_or_else(|e| panic!("{ext}: {e}"));
            assert!((volume(&back) - v).abs() / v < 0.01, "{ext}");
        }
        let p = dir.join("plate_with_hole.afxcad");
        if p.exists() {
            assert_eq!(import_file(&p).unwrap(), part());
        }
    }

    /// A Python with OCP (OpenCASCADE), if the environment names one: an independent STEP reader.
    fn ocp_python() -> Option<String> {
        let py = std::env::var("AURAFX_OCP_PYTHON").ok()?;
        std::process::Command::new(&py).args(["-c", "import OCP"]).output().ok().filter(|o| o.status.success()).map(|_| py)
    }

    /// The JSON line of the checker's output (the IGES reader also prints its own log lines).
    fn checker_json(out: &std::process::Output) -> serde_json::Value {
        let text = String::from_utf8_lossy(&out.stdout);
        let line = text.lines().rev().find(|l| l.trim_start().starts_with('{')).unwrap_or_else(|| panic!("{text} {}", String::from_utf8_lossy(&out.stderr)));
        serde_json::from_str(line).unwrap()
    }

    #[test]
    fn an_independent_step_reader_measures_the_same_part() {
        let Some(py) = ocp_python() else { return }; // skipped where no OCP is installed
        let script = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../scripts/cad/step_check.py");
        let d = part();
        let v = volume(&d);
        let p = tmp("independent.step");
        export_file(&d, &p, KernelChoice::Auto).unwrap();
        let out = std::process::Command::new(&py).arg(&script).arg(&p).output().unwrap();
        let j = checker_json(&out);
        assert_eq!(j["solids"], 1, "{j}");
        assert_eq!(j["faces"], 7, "a plate with a hole has seven faces: {j}");
        let vo = j["volume"].as_f64().unwrap();
        assert!((vo - v).abs() / v < 1e-3, "OpenCASCADE measures {vo}, the exact kernel {v}");
        // The IGES file of the same part reads back through the independent reader too.
        let pi = tmp("independent.igs");
        export_file(&d, &pi, KernelChoice::Auto).unwrap();
        let out = std::process::Command::new(&py).arg(&script).arg(&pi).output().unwrap();
        let j = checker_json(&out);
        assert_eq!(j["faces"], 7, "{j}");
        assert!((j["volume"].as_f64().unwrap() - v).abs() / v < 1e-3, "OpenCASCADE reads {j}, the exact kernel {v}");
        // The faceted file of a part only the implicit kernel builds is a valid solid for it too.
        let mut rod = Document::new("rod");
        apply_op(&mut rod, "add", &json!({"kind": "thread", "name": "T", "major_diameter": 8, "pitch": 2.5, "length": 10})).unwrap();
        apply_op(&mut rod, "add", &json!({"kind": "cylinder", "name": "Core", "radius": 3.0, "height": 10})).unwrap();
        let pf = tmp("independent_faceted.step");
        let r = export_file_with(&rod, &pf, KernelChoice::Auto, 14).unwrap();
        assert!(!r.exact);
        let out = std::process::Command::new(&py).arg(&script).arg(&pf).output().unwrap();
        let j = checker_json(&out);
        assert_eq!(j["solids"], 1, "{j}");
        assert!(j["volume"].as_f64().unwrap() > 100.0, "{j}");
    }

    #[test]
    fn an_exact_part_is_written_as_iges_and_reads_back_the_same() {
        let d = part();
        let v = volume(&d);
        let p = tmp("plate.igs");
        let r = export_file(&d, &p, KernelChoice::Auto).unwrap();
        assert!(r.exact, "{r:?}");
        let text = std::fs::read_to_string(&p).unwrap();
        // An IGES file is 80-column records ending in the section letters S, G, D, P and T.
        assert!(text.lines().all(|l| l.len() == 80), "IGES records are 80 columns wide");
        assert!(text.lines().last().unwrap().contains('T') && text.lines().any(|l| l.as_bytes()[72] == b'G'));
        let back = import_file(&p).unwrap();
        let ev = crate::kernel::evaluate_with(KernelChoice::Brep, &back, 64, true).unwrap();
        assert!((ev.volume() - v).abs() / v < 1e-3, "{} vs {v}", ev.volume());
        // A part only the implicit kernel builds cannot be written as IGES: the message says to write STEP.
        let mut rod = Document::new("rod");
        apply_op(&mut rod, "add", &json!({"kind": "thread", "name": "T", "major_diameter": 8, "pitch": 2.5, "length": 10})).unwrap();
        let e = export_file(&rod, &tmp("rod.igs"), KernelChoice::Auto).unwrap_err().to_string();
        assert!(e.contains("write STEP"), "{e}");
    }
}
