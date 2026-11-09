//! Pipelines as JSON: the chain of steps that made a node, written into `.dream3d` files
//! (`/Pipeline/Pipeline`, numbered steps and a `PipelineBuilder` entry), and pipelines read
//! back: auraFX's own steps exactly, and the steps of `.dream3d` pipeline files (the version 6
//! JSON layout, as a `.json` file or inside a `.dream3d` file) mapped to auraFX filters where
//! one does the same work.

use crate::session::{NodeId, NodeKind, Session};
use anyhow::{anyhow, bail, Result};
use serde_json::{json, Map, Value};
use std::path::{Path, PathBuf};

/// What an import made, and the steps it could not map (name and reason).
#[derive(Debug, Default, Clone)]
pub struct Imported {
    pub nodes: Vec<NodeId>,
    pub skipped: Vec<(String, String)>,
}

const OWN: &str = "auraFX::";

impl Session {
    /// The nodes `id` depends on, inputs first, ending with `id`.
    fn upstream_order(&self, id: NodeId) -> Vec<NodeId> {
        fn visit(s: &Session, id: NodeId, out: &mut Vec<NodeId>) {
            if out.contains(&id) {
                return;
            }
            if let Some(n) = s.nodes.get(&id) {
                for i in n.input.iter().chain(n.extra_inputs.iter()) {
                    visit(s, *i, out);
                }
                out.push(id);
            }
        }
        let mut out = Vec::new();
        visit(self, id, &mut out);
        out
    }

    /// The pipeline that makes node `id`, as JSON text: one numbered entry per step (filter
    /// name `auraFX::<filter id>`, its label, parameters, file and inputs) and a
    /// `PipelineBuilder` entry.
    pub fn pipeline_json(&self, id: NodeId) -> String {
        let order = self.upstream_order(id);
        let pos = |i: &NodeId| order.iter().position(|x| x == i);
        let mut m = Map::new();
        for (k, nid) in order.iter().enumerate() {
            let n = &self.nodes[nid];
            let (name, label, file) = match &n.kind {
                NodeKind::Reader { path, .. } => ("open_file".to_string(), "Open File".to_string(), Some(path.display().to_string())),
                NodeKind::Remote { host, path, .. } => ("open_remote".to_string(), format!("Open on {}", host.name), Some(path.clone())),
                NodeKind::Source { def } | NodeKind::Filter { def } => (def.id.to_string(), def.label.to_string(), None),
            };
            let mut e = Map::new();
            e.insert("Filter_Enabled".into(), json!(true));
            e.insert("Filter_Human_Label".into(), json!(label));
            e.insert("Filter_Name".into(), json!(format!("{OWN}{name}")));
            e.insert("Node_Name".into(), json!(n.name));
            if let Some(f) = file {
                e.insert("InputFile".into(), json!(f));
            }
            e.insert("Parameters".into(), serde_json::to_value(&n.params).unwrap_or(Value::Null));
            e.insert("Input".into(), n.input.and_then(|i| pos(&i)).map(|p| json!(p)).unwrap_or(Value::Null));
            if !n.extra_inputs.is_empty() {
                e.insert("ExtraInputs".into(), json!(n.extra_inputs.iter().filter_map(pos).collect::<Vec<_>>()));
            }
            m.insert(k.to_string(), Value::Object(e));
        }
        m.insert("PipelineBuilder".into(), json!({"Name": "auraFX pipeline", "Number_Filters": order.len(), "Version": 6}));
        serde_json::to_string_pretty(&Value::Object(m)).unwrap_or_default()
    }

    /// Write node `id`'s data to `path` (format from the extension); `.dream3d` files also
    /// record the pipeline that made it.
    pub fn save_data(&mut self, id: NodeId, path: &Path) -> Result<()> {
        let ds = self.nodes.get(&id).and_then(|n| n.output.clone()).ok_or_else(|| anyhow!("the node has no data yet"))?;
        let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase();
        if ext == "dream3d" {
            let json = self.pipeline_json(id);
            return aurafx_io::dream3d_writer::write(&ds, path, Some(&json));
        }
        aurafx_io::writers::write_file(&ds, path)
    }

    /// Read a pipeline (a `.json` pipeline file or the pipeline inside a `.dream3d` file) and
    /// build it. Steps that read files open them when they exist; otherwise the pipeline
    /// starts from `input` (for example the data the pipeline was made for, opened already).
    pub fn import_pipeline(&mut self, path: &Path, input: Option<NodeId>) -> Result<Imported> {
        let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase();
        let text = if ext == "dream3d" {
            aurafx_io::ebsd_h5::dream3d_pipeline(path)?.ok_or_else(|| anyhow!("{} records no pipeline", path.display()))?
        } else {
            std::fs::read_to_string(path)?
        };
        self.import_pipeline_text(&text, input, path.parent())
    }

    pub fn import_pipeline_text(&mut self, text: &str, input: Option<NodeId>, base: Option<&Path>) -> Result<Imported> {
        let v: Value = serde_json::from_str(text).map_err(|e| anyhow!("not a pipeline JSON file: {e}"))?;
        let obj = v.as_object().ok_or_else(|| anyhow!("not a pipeline JSON file"))?;
        if obj.get("pipeline").is_some_and(|p| p.is_array()) {
            bail!("this pipeline file uses the newer layout (a \"pipeline\" list); pipelines with numbered steps (version 6, as .dream3d files record them) are read");
        }
        let mut keys: Vec<usize> = obj.keys().filter_map(|k| k.parse().ok()).collect();
        keys.sort_unstable();
        if keys.is_empty() {
            bail!("the pipeline has no steps");
        }
        let steps: Vec<&Map<String, Value>> = keys.iter().filter_map(|k| obj[&k.to_string()].as_object()).collect();
        let own = steps.iter().all(|s| s.get("Filter_Name").and_then(|n| n.as_str()).is_some_and(|n| n.starts_with(OWN)));
        let resolve = |f: &str| -> PathBuf {
            let p = PathBuf::from(f);
            match (p.is_relative(), base) {
                (true, Some(b)) if b.join(&p).exists() => b.join(p),
                _ => p,
            }
        };
        let mut out = Imported::default();
        if own {
            // auraFX steps: the same graph, the same parameters.
            let mut made: Vec<Option<NodeId>> = Vec::new();
            for s in &steps {
                let name = s["Filter_Name"].as_str().unwrap()[OWN.len()..].to_string();
                let inp = s.get("Input").and_then(|i| i.as_u64()).and_then(|i| made.get(i as usize).copied().flatten());
                let extra: Vec<NodeId> = s
                    .get("ExtraInputs")
                    .and_then(|e| e.as_array())
                    .map(|a| a.iter().filter_map(|i| i.as_u64()).filter_map(|i| made.get(i as usize).copied().flatten()).collect())
                    .unwrap_or_default();
                let id = match name.as_str() {
                    "open_file" | "open_remote" => {
                        let f = s.get("InputFile").and_then(|f| f.as_str()).unwrap_or("");
                        let p = resolve(f);
                        if name == "open_file" && p.exists() {
                            Some(self.open_file(&p)?)
                        } else {
                            out.skipped.push((
                                format!("open {f}"),
                                if input.is_some() { "not found: the pipeline starts from the selected data".into() } else { "not found".into() },
                            ));
                            input
                        }
                    }
                    id => {
                        let def = aurafx_filters::find(id).ok_or_else(|| anyhow!("unknown filter {id}"))?;
                        let nid = if def.is_source {
                            Some(self.add_source(id)?)
                        } else if let Some(i) = inp.or(if made.is_empty() { input } else { None }) {
                            Some(if extra.is_empty() { self.add_filter(id, i)? } else { self.add_filter_multi(id, &[&[i][..], &extra[..]].concat())? })
                        } else {
                            out.skipped.push((def.label.to_string(), "its input was not made".into()));
                            None
                        };
                        if let (Some(n), Some(p)) = (nid, s.get("Parameters")) {
                            if let Ok(params) = serde_json::from_value::<aurafx_filters::Params>(p.clone()) {
                                self.set_params(n, params);
                            }
                        }
                        if let (Some(n), Some(nm)) = (nid, s.get("Node_Name").and_then(|x| x.as_str())) {
                            self.rename(n, nm.to_string());
                        }
                        nid
                    }
                };
                if let Some(i) = id {
                    if !out.nodes.contains(&i) && Some(i) != input {
                        out.nodes.push(i);
                    }
                }
                made.push(id);
            }
            return Ok(out);
        }
        // Steps of other programs' pipeline files: a chain.
        let mut cur = input;
        for s in steps {
            let name = s.get("Filter_Name").and_then(|n| n.as_str()).unwrap_or("?").to_string();
            let label = s.get("Filter_Human_Label").and_then(|n| n.as_str()).unwrap_or(&name).to_string();
            if s.get("Filter_Enabled").and_then(|e| e.as_bool()) == Some(false) {
                out.skipped.push((label, "disabled in the pipeline".into()));
                continue;
            }
            match map_step(&name, s) {
                Step::Open(f) => {
                    let p = resolve(&f);
                    if p.exists() {
                        let id = self.open_file(&p)?;
                        out.nodes.push(id);
                        cur = Some(id);
                    } else {
                        out.skipped
                            .push((label, format!("{f} not found{}", if cur.is_some() { ": the pipeline continues from the selected data" } else { "" })));
                    }
                }
                Step::Filter(id, params) => {
                    let Some(i) = cur else {
                        out.skipped.push((label, "no data to work on: open the data first, then import with it selected".into()));
                        continue;
                    };
                    let nid = self.add_filter(id, i)?;
                    for (k, v) in params {
                        self.set_param_json(nid, &k, &v);
                    }
                    out.nodes.push(nid);
                    cur = Some(nid);
                }
                Step::Skip(why) => out.skipped.push((label, why)),
            }
        }
        Ok(out)
    }
}

enum Step {
    Open(String),
    Filter(&'static str, Vec<(String, Value)>),
    Skip(String),
}

/// One step of a version-6 pipeline file as an auraFX filter.
fn map_step(name: &str, s: &Map<String, Value>) -> Step {
    let num = |k: &str| s.get(k).and_then(|v| v.as_f64());
    let vec3 = |k: &str| s.get(k).and_then(|v| Some(json!([v.get("x")?.as_f64()?, v.get("y")?.as_f64()?, v.get("z")?.as_f64()?])));
    let array = |k: &str| s.get(k).and_then(|v| v.get("Data Array Name")).and_then(|v| v.as_str()).filter(|n| !n.trim().is_empty()).map(str::to_string);
    let flag = |k: &str| num(k).map(|x| x != 0.0).or_else(|| s.get(k).and_then(|v| v.as_bool()));
    let cells = |n: String| json!(["CELLS", n]);
    let mut p: Vec<(String, Value)> = Vec::new();
    let mut put = |k: &str, v: Option<Value>| {
        if let Some(v) = v {
            p.push((k.to_string(), v));
        }
    };
    let fids = array("FeatureIdsArrayPath").map(Value::String);
    let mask = || if flag("UseGoodVoxels").unwrap_or(true) { array("GoodVoxelsArrayPath").map(cells) } else { None };
    let id: &'static str = match name {
        "ReadH5Ebsd"
        | "ReadAngData"
        | "ReadCtfData"
        | "DataContainerReader"
        | "ReadEDAXH5Data"
        | "ImportH5OimData"
        | "ImportH5EspritData"
        | "ITKImageReader" => {
            return match s.get("InputFile").and_then(|f| f.as_str()) {
                Some(f) => Step::Open(f.to_string()),
                None => Step::Skip("no file named".into()),
            }
        }
        "DataContainerWriter" | "FeatureDataCSVWriter" | "VtkRectilinearGridWriter" | "AbaqusHexahedronWriter" => {
            return Step::Skip("writes a file: save the result with File › Save Data".into())
        }
        "RemoveArrays" | "RenameAttributeArray" | "CopyObject" | "ConvertData" => return Step::Skip("array bookkeeping, not needed".into()),
        "RotateEulerRefFrame" => {
            put("frame", Some(json!("Sample")));
            put("axis", vec3("RotationAxis"));
            put("angle", num("RotationAngle").map(|a| json!(a)));
            "rotate_orientations"
        }
        "CropImageGeometry" => {
            put("units", Some(json!("Cell indices")));
            put("min", Some(json!([num("XMin").unwrap_or(0.0), num("YMin").unwrap_or(0.0), num("ZMin").unwrap_or(0.0)])));
            put("max", Some(json!([num("XMax").unwrap_or(1e30), num("YMax").unwrap_or(1e30), num("ZMax").unwrap_or(1e30)])));
            put("renumber_features", flag("RenumberFeatures").map(|b| json!(b)));
            put("feature_ids", fids);
            "crop_volume"
        }
        "ResampleImageGeom" | "ChangeResolution" => {
            put("spacing", vec3("Spacing").or_else(|| vec3("Resolution")));
            put("renumber_features", flag("RenumberFeatures").map(|b| json!(b)));
            put("feature_ids", fids);
            "change_resolution"
        }
        "ConvertOrientations" => {
            const R: [&str; 7] = ["Euler angles", "Orientation matrices", "Quaternions", "Axis-angle", "Rodrigues vectors", "Homochoric vectors", ""];
            let r = |k: &str| num(k).and_then(|i| R.get(i as usize)).filter(|x| !x.is_empty()).map(|x| json!(x));
            let (Some(a), Some(b)) = (r("InputType"), r("OutputType")) else { return Step::Skip("cubochoric orientations are not converted".into()) };
            put("input_type", Some(a));
            put("output_type", Some(b));
            put("orientations", array("InputOrientationArrayPath").map(cells));
            put("name", s.get("OutputOrientationArrayName").cloned());
            "convert_orientations"
        }
        "EBSDSegmentFeatures" | "CAxisSegmentFeatures" | "ScalarSegmentFeatures" => {
            put(
                "method",
                Some(json!(match name {
                    "CAxisSegmentFeatures" => "C-axis alignment",
                    "ScalarSegmentFeatures" => "Scalar",
                    _ => "Misorientation",
                })),
            );
            put("tolerance", num("MisorientationTolerance").or(num("ScalarTolerance")).map(|t| json!(t)));
            put("mask", mask());
            put("scalar", array("ScalarArrayPath").map(cells));
            put("shuffle", flag("RandomizeFeatureIds").map(|b| json!(b)));
            "segment_features"
        }
        "MultiThresholdObjects" => {
            let Some(list) = s.get("SelectedThresholds").and_then(|v| v.as_array()) else { return Step::Skip("no thresholds".into()) };
            let mut terms = Vec::new();
            for t in list {
                let (Some(a), Some(op), Some(v)) = (
                    t.get("Attribute Array Name").and_then(|x| x.as_str()),
                    t.get("Comparison Operator").and_then(|x| x.as_i64()),
                    t.get("Comparison Value").and_then(|x| x.as_f64()),
                ) else {
                    return Step::Skip("thresholds in sets are not mapped".into());
                };
                let op = match op {
                    0 => "<",
                    1 => ">",
                    2 => "==",
                    3 => "!=",
                    _ => return Step::Skip("unknown comparison".into()),
                };
                terms.push(format!("if({a} {op} {v}, 1, 0)"));
            }
            put("attribute", Some(json!("Cells")));
            put("expression", Some(json!(terms.join(" * "))));
            put("result", s.get("DestinationArrayName").cloned());
            "calculator"
        }
        "FillBadData" => {
            put("min_defect", num("MinAllowedDefectSize").map(|x| json!(x as i64)));
            put("feature_ids", fids);
            "fill_bad_data"
        }
        "MinSize" => {
            put("min_cells", num("MinAllowedFeatureSize").map(|x| json!(x as i64)));
            put("feature_ids", fids);
            "minimum_size"
        }
        "FindSizes" => {
            put("feature_ids", fids);
            "feature_sizes"
        }
        "FindFeatureCentroids" => {
            put("feature_ids", fids);
            "feature_centroids"
        }
        "FindShapes" => {
            put("feature_ids", fids);
            "feature_shapes"
        }
        "FindNeighbors" | "FindSurfaceFeatures" | "FindBoundaryCells" => {
            put("feature_ids", fids);
            "feature_neighbors"
        }
        "FindEuclideanDistMap" => {
            put("feature_ids", fids);
            "distance_to_boundaries"
        }
        "FindAvgOrientations" => {
            put("feature_ids", fids);
            "average_orientations"
        }
        "FindFeatureReferenceMisorientations" => {
            put("feature_ids", fids);
            "reference_misorientations"
        }
        "FindKernelAvgMisorientations" => {
            put("kernel", vec3("KernelSize"));
            put("feature_ids", fids);
            "kernel_average_misorientation"
        }
        "MergeTwins" => {
            put("axis_tolerance", num("AxisTolerance").map(|x| json!(x)));
            put("angle_tolerance", num("AngleTolerance").map(|x| json!(x)));
            put("feature_ids", fids);
            "merge_twins"
        }
        "GenerateIPFColors" => {
            put("key", Some(json!("Inverse pole figure")));
            put("direction", vec3("ReferenceDir"));
            put("orientations", array("CellEulerAnglesArrayPath").map(cells));
            put("mask", mask());
            put("name", s.get("CellIPFColorsArrayName").cloned());
            "orientation_colors"
        }
        "AlignSectionsMisorientation" | "AlignSectionsFeatureCentroid" | "AlignSectionsFeature" => {
            put("method", Some(json!(if name == "AlignSectionsMisorientation" { "Misorientation" } else { "Feature ids" })));
            put("tolerance", num("MisorientationTolerance").map(|x| json!(x)));
            put("mask", mask());
            "align_sections"
        }
        "QuickSurfaceMesh" => {
            put("feature_ids", fids);
            "quick_surface_mesh"
        }
        "LaplacianSmoothing" => {
            let lambda = num("Lambda").unwrap_or(0.1);
            let taubin = flag("UseTaubinSmoothing").unwrap_or(false);
            put("iterations", num("IterationSteps").map(|x| json!(x as i64)));
            put("lambda", Some(json!(lambda)));
            put("mu", Some(json!(if taubin { num("MuFactor").unwrap_or(-1.03) * lambda } else { 0.0 })));
            put("triple_lambda", num("TripleLineLambda").map(|x| json!(x)));
            put("quad_lambda", num("QuadPointLambda").map(|x| json!(x)));
            put("smooth_outer", num("SurfacePointLambda").map(|x| json!(x > 0.0)));
            "smooth_boundaries"
        }
        "RotateSampleRefFrame" => {
            put("axis", vec3("RotationAxis"));
            put("angle", num("RotationAngle").map(|a| json!(a)));
            "rotate_sample_reference_frame"
        }
        "ApplyTransformationToGeometry" => {
            // 2 manual matrix, 3 rotation, 4 translation, 5 scale (1 = a matrix another step computed).
            match num("TransformationMatrixType").map(|x| x as i64) {
                Some(3) => {
                    put("axis", vec3("RotationAxis"));
                    put("angle", num("RotationAngle").map(|a| json!(a)));
                }
                Some(4) => put("translation", vec3("Translation")),
                Some(5) => put("scale", vec3("Scale")),
                Some(2) => {
                    let m = s.get("ManualTransformationMatrix").and_then(|t| t.get("Table Data")).and_then(|t| t.as_array()).cloned().unwrap_or_default();
                    let at = |r: usize, c: usize| m.get(r).and_then(|row| row.get(c)).and_then(|v| v.as_f64()).unwrap_or(if r == c { 1.0 } else { 0.0 });
                    put("matrix", Some(json!((0..4).flat_map(|r| (0..4).map(move |c| (r, c))).map(|(r, c)| at(r, c)).collect::<Vec<_>>())));
                }
                _ => return Step::Skip("the transformation comes from another step (Iterative Closest Point)".into()),
            }
            "apply_transformation_to_geometry"
        }
        "SetOriginResolutionImageGeom" => {
            put("change_origin", flag("ChangeOrigin").map(|b| json!(b)));
            put("origin", vec3("Origin"));
            put("change_spacing", flag("ChangeResolution").map(|b| json!(b)));
            put("spacing", vec3("Spacing").or_else(|| vec3("Resolution")));
            "set_origin_spacing"
        }
        "IdentifySample" => {
            put("mask", array("GoodVoxelsArrayPath").map(Value::String));
            put("fill_holes", flag("FillHoles").map(|b| json!(b)));
            "identify_sample"
        }
        "ErodeDilateBadData" => {
            put("operation", num("Direction").map(|d| json!(if d == 0.0 { "Dilate" } else { "Erode" })));
            put("iterations", num("NumIterations").map(|x| json!(x as i64)));
            put("x", flag("XDirOn").map(|b| json!(b)));
            put("y", flag("YDirOn").map(|b| json!(b)));
            put("z", flag("ZDirOn").map(|b| json!(b)));
            put("feature_ids", fids);
            "erode_dilate_bad_data"
        }
        "ErodeDilateCoordinationNumber" => {
            put("coordination", num("CoordinationNumber").map(|x| json!(x as i64)));
            put("repeat", flag("Loop").map(|b| json!(b)));
            put("feature_ids", fids);
            "erode_dilate_coordination_number"
        }
        "ReplaceElementAttributesWithNeighborValues" => {
            put("confidence", array("ConfidenceIndexArrayPath").map(Value::String));
            put("min", num("MinConfidence").map(|x| json!(x)));
            put("greater", num("SelectedComparison").map(|x| json!(x != 0.0)));
            put("repeat", flag("Loop").map(|b| json!(b)));
            "replace_element_attributes_with_neighbor"
        }
        "NeighborOrientationCorrelation" => {
            put("confidence", array("ConfidenceIndexArrayPath").map(Value::String));
            put("min_confidence", num("MinConfidence").map(|x| json!(x)));
            put("tolerance", num("MisorientationTolerance").map(|x| json!(x)));
            put("level", num("Level").map(|x| json!(x as i64)));
            "neighbor_orientation_correlation"
        }
        "BadDataNeighborOrientationCheck" => {
            put("mask", array("GoodVoxelsArrayPath").map(Value::String));
            put("tolerance", num("MisorientationTolerance").map(|x| json!(x)));
            put("neighbors", num("NumberOfNeighbors").map(|x| json!(x as i64)));
            "bad_data_neighbor_orientation_check"
        }
        "FindNeighborhoods" => {
            put("multiples", num("MultiplesOfAverage").map(|x| json!(x)));
            put("feature_ids", fids);
            "find_neighborhoods"
        }
        "FindLargestCrossSections" => {
            // Plane 0 XY, 1 XZ, 2 YZ.
            put(
                "axis",
                num("Plane").map(|x| {
                    json!(match x as i64 {
                        1 => "Y",
                        2 => "X",
                        _ => "Z",
                    })
                }),
            );
            put("feature_ids", fids);
            "find_largest_cross_sections"
        }
        "FindFeatureClustering" => {
            put("bins", num("NumberOfBins").map(|x| json!(x as i64)));
            put("phase", num("PhaseNumber").map(|x| json!(x as i64)));
            put("feature_ids", fids);
            "find_feature_clustering"
        }
        "FindBoundaryStrengths" | "FindSlipTransmissionMetrics" => {
            put("direction", vec3("Loading"));
            put("feature_ids", fids);
            "find_boundary_strengths"
        }
        "FindTwinBoundarySchmidFactors" => {
            put("direction", vec3("LoadingDir"));
            put("feature_ids", fids);
            "find_twin_boundary_schmid_factors"
        }
        "FindCAxisLocations" => "find_c_axis_locations",
        "FindFeatureReferenceCAxisMisorientations" => {
            put("feature_ids", fids);
            "find_feature_reference_c_axis_misorientations"
        }
        "IdentifyMicroTextureRegions" => {
            put("tolerance", num("CAxisTolerance").map(|x| json!(x)));
            put("min_features", num("MinMTRSize").map(|x| json!((x as i64).max(2))));
            put("feature_ids", fids);
            "identify_micro_texture_regions"
        }
        "CopyFeatureArrayToElementArray" => {
            put("array", array("SelectedFeatureArrayPath").map(Value::String));
            put("result", s.get("CreatedArrayName").cloned());
            put("feature_ids", fids);
            "copy_feature_array_to_cells"
        }
        "ITKMedianImage" => {
            put("array", array("SelectedCellArrayPath").map(cells));
            put("radius", vec3("Radius"));
            put("result", s.get("NewCellArrayName").cloned());
            "median_image"
        }
        "ITKOtsuMultipleThresholdsImage" => {
            put("array", array("SelectedCellArrayPath").map(cells));
            put("thresholds", num("NumberOfThresholds").map(|x| json!(x as i64)));
            put("bins", num("NumberOfHistogramBins").map(|x| json!(x as i64)));
            put("result", s.get("NewCellArrayName").cloned());
            "otsu_threshold"
        }
        "ITKDiscreteGaussianImage" | "ITKSmoothingRecursiveGaussianImage" => {
            put("array", array("SelectedCellArrayPath").map(cells));
            put(
                "sigma",
                vec3("Variance")
                    .map(|v| json!(v.as_array().unwrap().iter().map(|x| x.as_f64().unwrap_or(1.0).sqrt()).collect::<Vec<_>>()))
                    .or_else(|| vec3("Sigma")),
            );
            put("result", s.get("NewCellArrayName").cloned());
            "gaussian_image"
        }
        "ITKBinaryErodeImage" | "ITKBinaryDilateImage" | "ITKBinaryOpeningByReconstructionImage" | "ITKBinaryClosingByReconstructionImage" => {
            put("array", array("SelectedCellArrayPath").map(cells));
            put(
                "operation",
                Some(json!(match name {
                    "ITKBinaryErodeImage" => "Erode",
                    "ITKBinaryDilateImage" => "Dilate",
                    "ITKBinaryOpeningByReconstructionImage" => "Open",
                    _ => "Close",
                })),
            );
            put("radius", s.get("KernelRadius").and_then(|v| v.get("x")).and_then(|v| v.as_f64()).map(|x| json!(x as i64)));
            put("result", s.get("NewCellArrayName").cloned());
            "binary_morphology"
        }
        "TriangleAreaFilter" => "triangle_geometry",
        "PointSampleTriangleGeometry" => {
            put("samples", num("NumberOfSamples").map(|x| json!(x as i64)));
            "point_sample_surface"
        }
        "CropVertexGeometry" => {
            put("min", Some(json!([num("XMin").unwrap_or(f64::MIN), num("YMin").unwrap_or(f64::MIN), num("ZMin").unwrap_or(f64::MIN)])));
            put("max", Some(json!([num("XMax").unwrap_or(f64::MAX), num("YMax").unwrap_or(f64::MAX), num("ZMax").unwrap_or(f64::MAX)])));
            "crop_points"
        }
        "IterativeClosestPoint" => return Step::Skip("aligns two point sets of different data: add Iterative Closest Point with both inputs".into()),
        _ => return Step::Skip("no auraFX filter does this step".into()),
    };
    Step::Filter(id, p)
}
