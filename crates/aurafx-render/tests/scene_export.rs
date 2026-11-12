//! Scene export: glTF binary and OBJ files that describe what the view shows.

use aurafx_data::DataSet;
use aurafx_pipeline::{ColorBy, DisplayProps, Representation};
use aurafx_render::scene_export::{export, ExportObject};
use aurafx_render::*;
use std::sync::Arc;

fn draws() -> Vec<NodeDraw> {
    // A sphere colored by its z coordinate, and a translucent wireframe box.
    let mut sphere = aurafx_filters::sources::sphere([100.0, 0.0, 0.0], 1.0, 16, 8);
    let z: Vec<f64> = (0..sphere.points.len()).map(|i| sphere.points.get(i)[2]).collect();
    sphere.point_data.insert(aurafx_data::Array::scalars_f64("z", z));
    let sphere = DataSet::PolyData(sphere);
    let mut ds = DisplayProps::default();
    ds.color_by = Some(ColorBy { assoc: aurafx_filters::Association::Points, name: "z".into(), component: None });
    let boxx = DataSet::PolyData(aurafx_filters::sources::cube([0.0; 3], [1.0, 1.0, 1.0]));
    let mut db = DisplayProps::default();
    db.representation = Representation::Wireframe;
    db.opacity = 0.5;
    db.lighting = false;
    vec![
        NodeDraw { id: 1, mesh: Arc::new(tessellate::tessellate(&sphere, &ds)), display: ds, overlay: false },
        NodeDraw { id: 2, mesh: Arc::new(tessellate::tessellate(&boxx, &db)), display: db, overlay: false },
    ]
}

#[test]
fn glb_and_obj() {
    let draws = draws();
    let objects: Vec<ExportObject> = draws.iter().zip(["sphere", "box"]).map(|(d, n)| ExportObject { name: n.into(), draw: d }).collect();
    let dir = std::env::temp_dir().join(format!("aurafx_export_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();

    // glTF binary: header, JSON and BIN chunks; accessors fit in the buffer.
    let glb = dir.join("scene.glb");
    export(&glb, &objects).unwrap();
    let bytes = std::fs::read(&glb).unwrap();
    assert_eq!(&bytes[0..4], b"glTF");
    assert_eq!(u32::from_le_bytes(bytes[8..12].try_into().unwrap()) as usize, bytes.len());
    let jlen = u32::from_le_bytes(bytes[12..16].try_into().unwrap()) as usize;
    assert_eq!(&bytes[16..20], b"JSON");
    let doc: serde_json::Value = serde_json::from_slice(&bytes[20..20 + jlen]).unwrap();
    let blen = u32::from_le_bytes(bytes[20 + jlen..24 + jlen].try_into().unwrap()) as usize;
    assert_eq!(&bytes[24 + jlen..28 + jlen], b"BIN\0");
    assert_eq!(doc["buffers"][0]["byteLength"].as_u64().unwrap() as usize, blen);
    for v in doc["bufferViews"].as_array().unwrap() {
        assert!(v["byteOffset"].as_u64().unwrap() + v["byteLength"].as_u64().unwrap() <= blen as u64);
    }
    let meshes = doc["meshes"].as_array().unwrap();
    assert_eq!(meshes.len(), 2);
    let tri = &meshes[0]["primitives"][0];
    assert_eq!(tri["mode"], 4);
    let count = doc["accessors"][tri["attributes"]["POSITION"].as_u64().unwrap() as usize]["count"].as_u64().unwrap();
    assert_eq!(count as usize, draws[0].mesh.tris.len());
    assert_eq!(doc["nodes"][0]["translation"][0].as_f64().unwrap(), 100.0, "big coordinates kept in the node");
    assert_eq!(meshes[1]["primitives"][0]["mode"], 1, "wireframe as lines");
    assert_eq!(doc["materials"][1]["alphaMode"], "BLEND");
    assert!(doc["materials"][1]["extensions"]["KHR_materials_unlit"].is_object());
    // Colors follow the color map: the bottom of the sphere is dark (viridis), the top light.
    let colors = doc["accessors"][tri["attributes"]["COLOR_0"].as_u64().unwrap() as usize]["bufferView"].as_u64().unwrap() as usize;
    let off = 28 + jlen + doc["bufferViews"][colors]["byteOffset"].as_u64().unwrap() as usize;
    let f = |i: usize| f32::from_le_bytes(bytes[off + 4 * i..off + 4 * i + 4].try_into().unwrap());
    let lum: Vec<f32> = (0..count as usize).map(|v| f(3 * v) + f(3 * v + 1) + f(3 * v + 2)).collect();
    let (lo, hi) = lum.iter().fold((f32::INFINITY, 0.0f32), |(a, b), &x| (a.min(x), b.max(x)));
    assert!(hi > 4.0 * lo, "color map spans dark to light: {lo} {hi}");

    // OBJ: vertices with colors, normals, faces and lines.
    let obj = dir.join("scene.obj");
    export(&obj, &objects).unwrap();
    let text = std::fs::read_to_string(&obj).unwrap();
    let n = |p: &str| text.lines().filter(|l| l.starts_with(p)).count();
    assert_eq!(n("v "), draws[0].mesh.tris.len() + draws[1].mesh.lines.len());
    assert_eq!(n("f "), draws[0].mesh.tris.len() / 3);
    assert_eq!(n("l "), draws[1].mesh.lines.len() / 2);
    assert!(text.lines().find(|l| l.starts_with("v ")).unwrap().split_whitespace().count() == 7, "x y z r g b");
    // X3D and VRML.
    export(&dir.join("scene.x3d"), &objects).unwrap();
    let x3d = std::fs::read_to_string(dir.join("scene.x3d")).unwrap();
    assert!(x3d.contains("<TriangleSet") && x3d.contains("<LineSet") && x3d.contains("100.97"));
    export(&dir.join("scene.wrl"), &objects).unwrap();
    let wrl = std::fs::read_to_string(dir.join("scene.wrl")).unwrap();
    assert!(wrl.starts_with("#VRML V2.0 utf8") && wrl.contains("IndexedFaceSet") && wrl.contains("IndexedLineSet"));
    assert_eq!(wrl.matches('{').count(), wrl.matches('}').count(), "balanced braces");
    assert!(export(&dir.join("scene.xyz"), &objects).is_err());
    if std::env::var("AURAFX_KEEP").is_err() {
        let _ = std::fs::remove_dir_all(&dir);
    } else {
        eprintln!("kept {}", dir.display());
    }
}

/// The web page export: one file holding the vertices (10 floats each) of the scene, with the
/// viewer's script. When Chrome is on this machine the page is opened headless and must draw
/// (a picture that is not one flat color, and the script reports the counts it loaded).
#[test]
fn html_page_holds_the_scene_and_draws() {
    use base64::Engine;
    let draws = draws();
    let objects: Vec<ExportObject> = draws.iter().zip(["sphere", "box"]).map(|(d, n)| ExportObject { name: n.into(), draw: d }).collect();
    let dir = std::env::temp_dir().join(format!("aurafx_export_html_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let page = dir.join("scene.html");
    export(&page, &objects).unwrap();
    let text = std::fs::read_to_string(&page).unwrap();
    assert!(
        text.starts_with("<!doctype html>")
            && text.contains("2 objects")
            && !text.contains("@TRIS@")
            && !text.contains("http://")
            && !text.contains("https://")
    );
    // The triangle array: the sphere's triangles, 10 floats a vertex, positions within the unit ball.
    let b64 = text.split("tris: \"").nth(1).unwrap().split('"').next().unwrap();
    let bytes = base64::engine::general_purpose::STANDARD.decode(b64).unwrap();
    let f: Vec<f32> = bytes.chunks_exact(4).map(|c| f32::from_le_bytes(c.try_into().unwrap())).collect();
    assert_eq!(f.len(), draws[0].mesh.tris.len() * 10);
    assert!(f.chunks_exact(10).all(|v| v[0].abs() <= 1.001 && v[1].abs() <= 1.001 && v[2].abs() <= 1.001 && (0.0..=1.0).contains(&v[6])));
    // In a browser, when there is one.
    let chrome = ["google-chrome", "chromium", "chromium-browser"].into_iter().find(|c| std::process::Command::new(c).arg("--version").output().is_ok());
    if let Some(chrome) = chrome {
        let png = dir.join("page.png");
        let out = std::process::Command::new(chrome)
            .args([
                "--headless=new",
                "--no-sandbox",
                "--use-angle=swiftshader",
                "--enable-unsafe-swiftshader",
                "--window-size=640,480",
                "--virtual-time-budget=4000",
            ])
            .arg(format!("--screenshot={}", png.display()))
            .arg(format!("file://{}", page.display()))
            .output()
            .unwrap();
        if let Ok(img) = image::open(&png) {
            let img = img.to_rgb8();
            let first = *img.get_pixel(5, 5);
            let other = img.pixels().filter(|p| (0..3).any(|k| p[k].abs_diff(first[k]) > 24)).count();
            eprintln!("the page drew {other} pixels that differ from the background");
            assert!(other > 2000, "the page is blank ({other} pixels drawn)\n{}", String::from_utf8_lossy(&out.stderr));
        } else {
            eprintln!("no screenshot from {chrome}: skipped the browser check\n{}", String::from_utf8_lossy(&out.stderr));
        }
    }
    let _ = std::fs::remove_dir_all(&dir);
}
