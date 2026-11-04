//! GPU discovery for NVIDIA (nvidia-smi) and AMD (rocm-smi / amd-smi), locally or over SSH.

use crate::Host;
use serde::{Deserialize, Serialize};
use std::time::Duration;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum GpuVendor {
    Nvidia,
    Amd,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GpuInfo {
    pub vendor: GpuVendor,
    pub index: u32,
    pub name: String,
    pub memory_total_mb: f64,
    pub memory_used_mb: f64,
    pub utilization: f64,
    pub temperature_c: Option<f64>,
}

/// Query GPUs on a host. Never fails: missing tools simply yield no GPUs of that vendor.
pub fn query(host: &Host) -> Vec<GpuInfo> {
    let script = "command -v nvidia-smi >/dev/null && nvidia-smi --query-gpu=index,name,memory.total,memory.used,utilization.gpu,temperature.gpu --format=csv,noheader,nounits | sed 's/^/NV,/'; \
                  command -v rocm-smi >/dev/null && rocm-smi --showproductname --showmeminfo vram --showuse --showtemp --json 2>/dev/null | sed 's/^/AMDJSON,/'; true";
    let out = host.run_capture(script, Duration::from_secs(20)).unwrap_or_default();
    let mut gpus = Vec::new();
    for line in out.lines() {
        if let Some(rest) = line.strip_prefix("NV,") {
            let f: Vec<&str> = rest.split(',').map(|s| s.trim()).collect();
            if f.len() >= 5 {
                gpus.push(GpuInfo {
                    vendor: GpuVendor::Nvidia,
                    index: f[0].parse().unwrap_or(0),
                    name: f[1].to_string(),
                    memory_total_mb: f[2].parse().unwrap_or(0.0),
                    memory_used_mb: f[3].parse().unwrap_or(0.0),
                    utilization: f[4].parse().unwrap_or(0.0),
                    temperature_c: f.get(5).and_then(|t| t.parse().ok()),
                });
            }
        } else if let Some(json) = line.strip_prefix("AMDJSON,") {
            gpus.extend(parse_rocm_smi(json));
        }
    }
    gpus
}

fn num(v: Option<&serde_json::Value>) -> Option<f64> {
    v.and_then(|x| x.as_f64().or_else(|| x.as_str().and_then(|s| s.trim().trim_end_matches('%').parse().ok())))
}

pub fn parse_rocm_smi(json: &str) -> Vec<GpuInfo> {
    let Ok(v) = serde_json::from_str::<serde_json::Value>(json) else { return vec![] };
    let Some(obj) = v.as_object() else { return vec![] };
    let mut out = Vec::new();
    for (k, card) in obj {
        let Some(idx) = k.strip_prefix("card").and_then(|s| s.parse::<u32>().ok()) else { continue };
        let name = card
            .get("Card Series")
            .or_else(|| card.get("Card series"))
            .or_else(|| card.get("Card Model"))
            .and_then(|s| s.as_str())
            .unwrap_or("AMD GPU")
            .to_string();
        let total = num(card.get("VRAM Total Memory (B)")).unwrap_or(0.0) / 1048576.0;
        let used = num(card.get("VRAM Total Used Memory (B)")).unwrap_or(0.0) / 1048576.0;
        let util = num(card.get("GPU use (%)")).unwrap_or(0.0);
        let temp = num(card.get("Temperature (Sensor edge) (C)")).or_else(|| num(card.get("Temperature (Sensor junction) (C)")));
        out.push(GpuInfo { vendor: GpuVendor::Amd, index: idx, name, memory_total_mb: total, memory_used_mb: used, utilization: util, temperature_c: temp });
    }
    out.sort_by_key(|g| g.index);
    out
}

#[cfg(test)]
mod tests {
    #[test]
    fn rocm_json() {
        let j = r#"{"card0": {"Card Series": "Navi 14 [Radeon RX 5500M]", "VRAM Total Memory (B)": "4278190080", "VRAM Total Used Memory (B)": "123731968", "GPU use (%)": "3", "Temperature (Sensor edge) (C)": "48.0"}}"#;
        let g = super::parse_rocm_smi(j);
        assert_eq!(g.len(), 1);
        assert_eq!(g[0].index, 0);
        assert!((g[0].memory_total_mb - 4080.0).abs() < 1.0);
        assert_eq!(g[0].utilization, 3.0);
    }
}
