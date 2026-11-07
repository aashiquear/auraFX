//! Crystal Mechanics: Schmid factor, Taylor factor and directional Young's modulus of every
//! orientation for a loading direction, from a material of the table (or one described in
//! the parameters), with their volume means and a yield-strength estimate.

use crate::crystal_texture::{selected, Selection};
use crate::Association;
use anyhow::{anyhow, bail, Result};
use aurafx_crystal::mechanics::{self as mech, Lattice, Material, MATERIALS};
use aurafx_data::*;
use rayon::prelude::*;

pub const CUSTOM: &str = "Custom";

pub fn material_names() -> Vec<&'static str> {
    MATERIALS.iter().map(|m| m.name).chain(std::iter::once(CUSTOM)).collect()
}

/// A material described in the parameters.
pub struct CustomMaterial {
    pub lattice: String,
    pub elastic: Vec<f64>,
    pub shear_modulus_gpa: f64,
    pub burgers_m: f64,
    pub c_over_a: f64,
    /// Slip families as `name=CRSS, name=CRSS` (CRSS relative; 1 when left out).
    pub families: String,
    pub sigma_base_mpa: f64,
}

struct Resolved {
    lattice: Lattice,
    elastic: Vec<f64>,
    mu: f64,
    b: f64,
    ca: f64,
    families: Vec<(usize, f64)>,
    sigma_base: f64,
    alpha: f64,
}

fn resolve(name: &str, custom: &CustomMaterial) -> Result<Resolved> {
    if let Some(m) = MATERIALS.iter().find(|m| m.name == name) {
        let Material { lattice, elastic, shear_modulus_gpa, burgers_m, c_over_a, families, sigma_base_mpa, taylor_alpha, .. } = m.clone();
        return Ok(Resolved {
            lattice,
            elastic: elastic.to_vec(),
            mu: shear_modulus_gpa,
            b: burgers_m,
            ca: c_over_a,
            families: families.to_vec(),
            sigma_base: sigma_base_mpa,
            alpha: taylor_alpha,
        });
    }
    let lattice = match custom.lattice.to_ascii_uppercase().as_str() {
        "FCC" => Lattice::Fcc,
        "BCC" => Lattice::Bcc,
        "HCP" => Lattice::Hcp,
        l => bail!("unknown lattice {l} (FCC, BCC, HCP)"),
    };
    let need = if lattice == Lattice::Hcp { 5 } else { 3 };
    if custom.elastic.len() < need {
        bail!("give {need} elastic constants (GPa): {}", if need == 5 { "C11 C12 C13 C33 C44" } else { "C11 C12 C44" });
    }
    let mut families = Vec::new();
    for part in custom.families.split(',').map(str::trim).filter(|p| !p.is_empty()) {
        let (n, w) = match part.split_once('=') {
            Some((n, w)) => (n.trim(), w.trim().parse::<f64>().map_err(|_| anyhow!("{part}: the resolved shear stress is not a number"))?),
            None => (part, 1.0),
        };
        let k = mech::FAMILIES.iter().position(|f| f.name.eq_ignore_ascii_case(n) && f.lattice == lattice).ok_or_else(|| {
            anyhow!(
                "{n}: not a slip family of this lattice ({})",
                mech::FAMILIES.iter().filter(|f| f.lattice == lattice).map(|f| f.name).collect::<Vec<_>>().join(", ")
            )
        })?;
        families.push((k, w));
    }
    if families.is_empty() {
        families = mech::FAMILIES.iter().enumerate().filter(|(_, f)| f.lattice == lattice).take(1).map(|(k, _)| (k, 1.0)).collect();
    }
    Ok(Resolved {
        lattice,
        elastic: custom.elastic.clone(),
        mu: custom.shear_modulus_gpa,
        b: custom.burgers_m,
        ca: custom.c_over_a,
        families,
        sigma_base: custom.sigma_base_mpa,
        alpha: 0.3,
    })
}

#[allow(clippy::too_many_arguments)]
pub fn crystal_mechanics(
    ds: &DataSet,
    sel: &Selection,
    material: &str,
    custom: &CustomMaterial,
    dir: [f64; 3],
    taylor: bool,
    gnd: Option<&str>,
) -> Result<DataSet> {
    let m = resolve(material, custom)?;
    let (q, _laue) = selected(ds, sel)?;
    let fam_idx: Vec<usize> = m.families.iter().map(|f| f.0).collect();
    let systems = mech::slip_systems(&fam_idx, m.ca);
    let weights: Vec<f64> = systems.iter().map(|s| m.families.iter().find(|f| f.0 == s.family).map(|f| f.1).unwrap_or(1.0)).collect();
    let bases = if taylor { Some(mech::TaylorBases::new(&systems, &weights).map_err(|e| anyhow!(e))?) } else { None };
    let s = mech::compliance(&mech::stiffness(m.lattice, &m.elastic)).ok_or_else(|| anyhow!("the elastic constants give no compliance"))?;
    let l = (dir[0] * dir[0] + dir[1] * dir[1] + dir[2] * dir[2]).sqrt();
    if l == 0.0 {
        bail!("the loading direction is zero");
    }
    let rows: Vec<(f64, i32, f64, f64)> = q
        .par_iter()
        .map(|o| match o {
            Some(g) => {
                let (sf, k) = mech::schmid(*g, dir, &systems);
                let t = bases.as_ref().map(|b| b.taylor(*g, dir)).unwrap_or(f64::NAN);
                let e = mech::youngs_modulus(&s, aurafx_crystal::rotate(*g, dir.map(|x| x / l)));
                (sf, k as i32, t, e)
            }
            None => (f64::NAN, -1, f64::NAN, f64::NAN),
        })
        .collect();
    let (assoc, _) = crate::crystal::orientation_array(ds, sel.orientations.clone())?;
    let mut out = ds.clone();
    let mean = |v: &mut dyn Iterator<Item = f64>| {
        let (s, n) = v.filter(|x| x.is_finite()).fold((0.0, 0usize), |(s, n), x| (s + x, n + 1));
        if n > 0 {
            s / n as f64
        } else {
            f64::NAN
        }
    };
    let e_voigt = mean(&mut rows.iter().map(|r| r.3));
    let e_reuss = 1.0 / mean(&mut rows.iter().map(|r| 1.0 / r.3));
    let m_mean = mean(&mut rows.iter().map(|r| r.2));
    let rho = match gnd.filter(|g| !g.is_empty()) {
        Some(name) => {
            let a = match assoc {
                Association::Cells => ds.cell_data(),
                Association::Points => ds.point_data(),
            }
            .and_then(|x| x.get(name))
            .ok_or_else(|| anyhow!("no array {name}"))?;
            mean(&mut (0..a.len()).map(|i| a.get(i, 0)))
        }
        None => 0.0,
    };
    {
        let attrs = match assoc {
            Association::Cells => out.cell_data_mut(),
            Association::Points => out.point_data_mut(),
        }
        .ok_or_else(|| anyhow!("this data cannot hold arrays"))?;
        attrs.insert(Array::scalars_f64("SchmidFactor", rows.iter().map(|r| r.0).collect()));
        attrs.insert(Array::new("SlipSystem", 1, ArrayData::I32(rows.iter().map(|r| r.1).collect())));
        if taylor {
            attrs.insert(Array::scalars_f64("TaylorFactor", rows.iter().map(|r| r.2).collect()));
        }
        attrs.insert(Array::scalars_f64("YoungsModulus", rows.iter().map(|r| r.3).collect()));
    }
    let fd = out.field_data_mut_any();
    fd.insert(Array::scalars_f64("MeanSchmidFactor", vec![mean(&mut rows.iter().map(|r| r.0))]));
    fd.insert(Array::scalars_f64("YoungsModulusVoigt", vec![e_voigt]));
    fd.insert(Array::scalars_f64("YoungsModulusReuss", vec![e_reuss]));
    fd.insert(Array::scalars_f64("YoungsModulusHill", vec![0.5 * (e_voigt + e_reuss)]));
    if taylor {
        fd.insert(Array::scalars_f64("MeanTaylorFactor", vec![m_mean]));
        if m.sigma_base > 0.0 && m.lattice == Lattice::Fcc {
            // The toolkit's estimate: the random-texture base strength scaled by the Taylor
            // factor, plus the Taylor hardening of the dislocation density.
            let texture = m.sigma_base * m_mean / mech::M_RANDOM_FCC;
            let disl = m.alpha * m.mu * 1e9 * m.b * m_mean * rho.max(0.0).sqrt() / 1e6;
            fd.insert(Array::scalars_f64("YieldTextureMPa", vec![texture]));
            fd.insert(Array::scalars_f64("YieldDislocationMPa", vec![disl]));
            fd.insert(Array::scalars_f64("YieldEstimateMPa", vec![texture + disl]));
        }
    }
    let names: Vec<String> = systems
        .iter()
        .map(|s| format!("{} n({:.3} {:.3} {:.3}) s({:.3} {:.3} {:.3})", mech::FAMILIES[s.family].name, s.n[0], s.n[1], s.n[2], s.s[0], s.s[1], s.s[2]))
        .collect();
    fd.insert(Array::new("SlipSystemNames", 1, ArrayData::String(names)));
    Ok(out)
}
