//! DG basis functions, translated from VTK's Filters/CellGrid/Basis headers (generated from
//! Intrepid2; VTK is BSD-licensed) by scripts/gen_dg_basis.py. Do not edit by hand.

#![allow(unused_parens, unused_mut, unused_variables, unused_assignments, non_snake_case, clippy::all)]

pub type BasisFn = fn([f64; 3], &mut [f64]);

/// (function space, shape and basis kind like "HexC1", gradient?, function, number of functions)
pub const TABLE: &[(&str, &str, bool, BasisFn, usize)] = &[
    ("constant", "CellC0", false, constant_cellc0_basis, 1),
    ("constant", "CellC0", true, constant_cellc0_gradient, 1),
    ("HCURL", "HexI1", false, hcurl_hexi1_basis, 12),
    ("HCURL", "QuadI1", false, hcurl_quadi1_basis, 4),
    ("HCURL", "TetI1", false, hcurl_teti1_basis, 6),
    ("HCURL", "TriI1", false, hcurl_trii1_basis, 3),
    ("HCURL", "WdgI1", false, hcurl_wdgi1_basis, 9),
    ("HDIV", "HexI1", false, hdiv_hexi1_basis, 6),
    ("HDIV", "QuadI1", false, hdiv_quadi1_basis, 4),
    ("HDIV", "TetI1", false, hdiv_teti1_basis, 4),
    ("HDIV", "TriI1", false, hdiv_trii1_basis, 3),
    ("HDIV", "WdgI1", false, hdiv_wdgi1_basis, 5),
    ("HGRAD", "EdgeC1", false, hgrad_edgec1_basis, 2),
    ("HGRAD", "EdgeC1", true, hgrad_edgec1_gradient, 2),
    ("HGRAD", "EdgeC2", false, hgrad_edgec2_basis, 3),
    ("HGRAD", "EdgeC2", true, hgrad_edgec2_gradient, 2),
    ("HGRAD", "HexC1", false, hgrad_hexc1_basis, 8),
    ("HGRAD", "HexC1", true, hgrad_hexc1_gradient, 8),
    ("HGRAD", "HexC2", false, hgrad_hexc2_basis, 27),
    ("HGRAD", "HexC2", true, hgrad_hexc2_gradient, 27),
    ("HGRAD", "HexI2", false, hgrad_hexi2_basis, 20),
    ("HGRAD", "HexI2", true, hgrad_hexi2_gradient, 20),
    ("HGRAD", "PyrC1", false, hgrad_pyrc1_basis, 5),
    ("HGRAD", "PyrC1", true, hgrad_pyrc1_gradient, 5),
    ("HGRAD", "PyrC2", false, hgrad_pyrc2_basis, 18),
    ("HGRAD", "PyrC2", true, hgrad_pyrc2_gradient, 18),
    ("HGRAD", "PyrF2", false, hgrad_pyrf2_basis, 19),
    ("HGRAD", "PyrF2", true, hgrad_pyrf2_gradient, 19),
    ("HGRAD", "PyrI2", false, hgrad_pyri2_basis, 13),
    ("HGRAD", "PyrI2", true, hgrad_pyri2_gradient, 13),
    ("HGRAD", "QuadC1", false, hgrad_quadc1_basis, 4),
    ("HGRAD", "QuadC1", true, hgrad_quadc1_gradient, 4),
    ("HGRAD", "QuadC2", false, hgrad_quadc2_basis, 9),
    ("HGRAD", "QuadC2", true, hgrad_quadc2_gradient, 9),
    ("HGRAD", "TetC1", false, hgrad_tetc1_basis, 4),
    ("HGRAD", "TetC1", true, hgrad_tetc1_gradient, 4),
    ("HGRAD", "TetC2", false, hgrad_tetc2_basis, 10),
    ("HGRAD", "TetC2", true, hgrad_tetc2_gradient, 10),
    ("HGRAD", "TetF2", false, hgrad_tetf2_basis, 15),
    ("HGRAD", "TetF2", true, hgrad_tetf2_gradient, 15),
    ("HGRAD", "TriC1", false, hgrad_tric1_basis, 3),
    ("HGRAD", "TriC1", true, hgrad_tric1_gradient, 3),
    ("HGRAD", "TriC2", false, hgrad_tric2_basis, 6),
    ("HGRAD", "TriC2", true, hgrad_tric2_gradient, 6),
    ("HGRAD", "WdgC1", false, hgrad_wdgc1_basis, 6),
    ("HGRAD", "WdgC1", true, hgrad_wdgc1_gradient, 6),
    ("HGRAD", "WdgC2", false, hgrad_wdgc2_basis, 18),
    ("HGRAD", "WdgC2", true, hgrad_wdgc2_gradient, 18),
    ("HGRAD", "WdgF2", false, hgrad_wdgf2_basis, 21),
    ("HGRAD", "WdgF2", true, hgrad_wdgf2_gradient, 21),
    ("HGRAD", "WdgI2", false, hgrad_wdgi2_basis, 15),
    ("HGRAD", "WdgI2", true, hgrad_wdgi2_gradient, 15),
];

#[allow(unused_mut, unused_variables, clippy::all)]
pub fn constant_cellc0_basis(p: [f64; 3], b: &mut [f64]) {
    let (mut rr, mut ss, mut tt) = (p[0], p[1], p[2]);
    let eps = f64::EPSILON;
    b[0] = 1.0;
}

#[allow(unused_mut, unused_variables, clippy::all)]
pub fn constant_cellc0_gradient(p: [f64; 3], g: &mut [f64]) {
    let (mut rr, mut ss, mut tt) = (p[0], p[1], p[2]);
    let eps = f64::EPSILON;
    g[0] = 0.;
    g[1] = 0.;
    g[2] = 0.;
}

#[allow(unused_mut, unused_variables, clippy::all)]
pub fn hcurl_hexi1_basis(p: [f64; 3], b: &mut [f64]) {
    let (mut rr, mut ss, mut tt) = (p[0], p[1], p[2]);
    let eps = f64::EPSILON;
    b[0 * 3 + 0] = (1.0 - ss) * (1.0 - tt) / 4.0;
    b[0 * 3 + 1] = 0.0;
    b[0 * 3 + 2] = 0.0;
    b[1 * 3 + 0] = 0.0;
    b[1 * 3 + 1] = (1.0 + rr) * (1.0 - tt) / 4.0;
    b[1 * 3 + 2] = 0.0;
    b[2 * 3 + 0] = -(1.0 + ss) * (1.0 - tt) / 4.0;
    b[2 * 3 + 1] = 0.0;
    b[2 * 3 + 2] = 0.0;
    b[3 * 3 + 0] = 0.0;
    b[3 * 3 + 1] = -(1.0 - rr) * (1.0 - tt) / 4.0;
    b[3 * 3 + 2] = 0.0;
    b[4 * 3 + 0] = (1.0 - ss) * (1.0 + tt) / 4.0;
    b[4 * 3 + 1] = 0.0;
    b[4 * 3 + 2] = 0.0;
    b[5 * 3 + 0] = 0.0;
    b[5 * 3 + 1] = (1.0 + rr) * (1.0 + tt) / 4.0;
    b[5 * 3 + 2] = 0.0;
    b[6 * 3 + 0] = -(1.0 + ss) * (1.0 + tt) / 4.0;
    b[6 * 3 + 1] = 0.0;
    b[6 * 3 + 2] = 0.0;
    b[7 * 3 + 0] = 0.0;
    b[7 * 3 + 1] = -(1.0 - rr) * (1.0 + tt) / 4.0;
    b[7 * 3 + 2] = 0.0;
    b[8 * 3 + 0] = 0.0;
    b[8 * 3 + 1] = 0.0;
    b[8 * 3 + 2] = (1.0 - rr) * (1.0 - ss) / 4.0;
    b[9 * 3 + 0] = 0.0;
    b[9 * 3 + 1] = 0.0;
    b[9 * 3 + 2] = (1.0 + rr) * (1.0 - ss) / 4.0;
    b[10 * 3 + 0] = 0.0;
    b[10 * 3 + 1] = 0.0;
    b[10 * 3 + 2] = (1.0 + rr) * (1.0 + ss) / 4.0;
    b[11 * 3 + 0] = 0.0;
    b[11 * 3 + 1] = 0.0;
    b[11 * 3 + 2] = (1.0 - rr) * (1.0 + ss) / 4.0;
}

#[allow(unused_mut, unused_variables, clippy::all)]
pub fn hcurl_quadi1_basis(p: [f64; 3], b: &mut [f64]) {
    let (mut rr, mut ss, mut tt) = (p[0], p[1], p[2]);
    let eps = f64::EPSILON;
    b[0 * 3 + 0] = 0.5 * (1.0 - ss);
    b[0 * 3 + 1] = 0.0;
    b[0 * 3 + 2] = 0.0;
    b[1 * 3 + 0] = 0.0;
    b[1 * 3 + 1] = 0.5 * (1.0 + rr);
    b[1 * 3 + 2] = 0.0;
    b[2 * 3 + 0] = -0.5 * (1.0 + ss);
    b[2 * 3 + 1] = 0.0;
    b[2 * 3 + 2] = 0.0;
    b[3 * 3 + 0] = 0.0;
    b[3 * 3 + 1] = -0.5 * (1.0 - rr);
    b[3 * 3 + 2] = 0.0;
}

#[allow(unused_mut, unused_variables, clippy::all)]
pub fn hcurl_teti1_basis(p: [f64; 3], b: &mut [f64]) {
    let (mut rr, mut ss, mut tt) = (p[0], p[1], p[2]);
    let eps = f64::EPSILON;
    b[0 * 3 + 0] = 2.0 * (1.0 - ss - tt);
    b[0 * 3 + 1] = 2.0 * rr;
    b[0 * 3 + 2] = 2.0 * rr;
    b[1 * 3 + 0] = -2.0 * ss;
    b[1 * 3 + 1] = 2.0 * rr;
    b[1 * 3 + 2] = 0.0;
    b[2 * 3 + 0] = -2.0 * ss;
    b[2 * 3 + 1] = 2.0 * (-1.0 + rr + tt);
    b[2 * 3 + 2] = -2.0 * ss;
    b[3 * 3 + 0] = 2.0 * tt;
    b[3 * 3 + 1] = 2.0 * tt;
    b[3 * 3 + 2] = 2.0 * (1.0 - rr - ss);
    b[4 * 3 + 0] = -2.0 * tt;
    b[4 * 3 + 1] = 0.0;
    b[4 * 3 + 2] = 2.0 * rr;
    b[5 * 3 + 0] = 0.0;
    b[5 * 3 + 1] = -2.0 * tt;
    b[5 * 3 + 2] = 2.0 * ss;
}

#[allow(unused_mut, unused_variables, clippy::all)]
pub fn hcurl_trii1_basis(p: [f64; 3], b: &mut [f64]) {
    let (mut rr, mut ss, mut tt) = (p[0], p[1], p[2]);
    let eps = f64::EPSILON;
    b[0 * 3 + 0] = 2.0 * (1.0 - ss);
    b[0 * 3 + 1] = 2.0 * rr;
    b[0 * 3 + 2] = 0.0;
    b[1 * 3 + 0] = -2.0 * ss;
    b[1 * 3 + 1] = 2.0 * rr;
    b[1 * 3 + 2] = 0.0;
    b[2 * 3 + 0] = -2.0 * ss;
    b[2 * 3 + 1] = 2.0 * (-1.0 + rr);
    b[2 * 3 + 2] = 0.0;
}

#[allow(unused_mut, unused_variables, clippy::all)]
pub fn hcurl_wdgi1_basis(p: [f64; 3], b: &mut [f64]) {
    let (mut rr, mut ss, mut tt) = (p[0], p[1], p[2]);
    let eps = f64::EPSILON;
    b[0 * 3 + 0] = (1.0 - tt) * (1.0 - ss);
    b[0 * 3 + 1] = rr * (1.0 - tt);
    b[0 * 3 + 2] = 0.0;
    b[1 * 3 + 0] = ss * (tt - 1.0);
    b[1 * 3 + 1] = rr * (1.0 - tt);
    b[1 * 3 + 2] = 0.0;
    b[2 * 3 + 0] = ss * (tt - 1.0);
    b[2 * 3 + 1] = (1.0 - rr) * (tt - 1.0);
    b[2 * 3 + 2] = 0.0;
    b[3 * 3 + 0] = (1.0 - ss) * (1.0 + tt);
    b[3 * 3 + 1] = rr * (1.0 + tt);
    b[3 * 3 + 2] = 0.0;
    b[4 * 3 + 0] = -ss * (1.0 + tt);
    b[4 * 3 + 1] = rr * (1.0 + tt);
    b[4 * 3 + 2] = 0.0;
    b[5 * 3 + 0] = -ss * (1.0 + tt);
    b[5 * 3 + 1] = (rr - 1.0) * (1.0 + tt);
    b[5 * 3 + 2] = 0.0;
    b[6 * 3 + 0] = 0.0;
    b[6 * 3 + 1] = 0.0;
    b[6 * 3 + 2] = (1.0 - rr - ss);
    b[7 * 3 + 0] = 0.0;
    b[7 * 3 + 1] = 0.0;
    b[7 * 3 + 2] = rr;
    b[8 * 3 + 0] = 0.0;
    b[8 * 3 + 1] = 0.0;
    b[8 * 3 + 2] = ss;
}

#[allow(unused_mut, unused_variables, clippy::all)]
pub fn hdiv_hexi1_basis(p: [f64; 3], b: &mut [f64]) {
    let (mut rr, mut ss, mut tt) = (p[0], p[1], p[2]);
    let eps = f64::EPSILON;
    b[0 * 3 + 0] = 0.0;
    b[0 * 3 + 1] = (ss - 1.0) / 2.0;
    b[0 * 3 + 2] = 0.0;
    b[1 * 3 + 0] = (1.0 + rr) / 2.0;
    b[1 * 3 + 1] = 0.0;
    b[1 * 3 + 2] = 0.0;
    b[2 * 3 + 0] = 0.0;
    b[2 * 3 + 1] = (1.0 + ss) / 2.0;
    b[2 * 3 + 2] = 0.0;
    b[3 * 3 + 0] = (rr - 1.0) / 2.0;
    b[3 * 3 + 1] = 0.0;
    b[3 * 3 + 2] = 0.0;
    b[4 * 3 + 0] = 0.0;
    b[4 * 3 + 1] = 0.0;
    b[4 * 3 + 2] = (tt - 1.0) / 2.0;
    b[5 * 3 + 0] = 0.0;
    b[5 * 3 + 1] = 0.0;
    b[5 * 3 + 2] = (1.0 + tt) / 2.0;
}

#[allow(unused_mut, unused_variables, clippy::all)]
pub fn hdiv_quadi1_basis(p: [f64; 3], b: &mut [f64]) {
    let (mut rr, mut ss, mut tt) = (p[0], p[1], p[2]);
    let eps = f64::EPSILON;
    b[0 * 3 + 0] = 0.0;
    b[0 * 3 + 1] = 0.5 * (ss - 1.0);
    b[0 * 3 + 2] = 0.0;
    b[1 * 3 + 0] = 0.5 * (1.0 + rr);
    b[1 * 3 + 1] = 0.0;
    b[1 * 3 + 2] = 0.0;
    b[2 * 3 + 0] = 0.0;
    b[2 * 3 + 1] = 0.5 * (1.0 + ss);
    b[2 * 3 + 2] = 0.0;
    b[3 * 3 + 0] = 0.5 * (rr - 1.0);
    b[3 * 3 + 1] = 0.0;
    b[3 * 3 + 2] = 0.0;
}

#[allow(unused_mut, unused_variables, clippy::all)]
pub fn hdiv_teti1_basis(p: [f64; 3], b: &mut [f64]) {
    let (mut rr, mut ss, mut tt) = (p[0], p[1], p[2]);
    let eps = f64::EPSILON;
    b[0 * 3 + 0] = rr;
    b[0 * 3 + 1] = (ss - 1.0);
    b[0 * 3 + 2] = tt;
    b[1 * 3 + 0] = rr;
    b[1 * 3 + 1] = ss;
    b[1 * 3 + 2] = tt;
    b[2 * 3 + 0] = (rr - 1.0);
    b[2 * 3 + 1] = ss;
    b[2 * 3 + 2] = tt;
    b[3 * 3 + 0] = rr;
    b[3 * 3 + 1] = ss;
    b[3 * 3 + 2] = (tt - 1.0);
}

#[allow(unused_mut, unused_variables, clippy::all)]
pub fn hdiv_trii1_basis(p: [f64; 3], b: &mut [f64]) {
    let (mut rr, mut ss, mut tt) = (p[0], p[1], p[2]);
    let eps = f64::EPSILON;
    b[0 * 3 + 0] = 2.0 * rr;
    b[0 * 3 + 1] = 2.0 * (ss - 1.0);
    b[0 * 3 + 2] = 0.0;
    b[1 * 3 + 0] = 2.0 * rr;
    b[1 * 3 + 1] = 2.0 * ss;
    b[1 * 3 + 2] = 0.0;
    b[2 * 3 + 0] = 2.0 * (rr - 1.0);
    b[2 * 3 + 1] = 2.0 * ss;
    b[2 * 3 + 2] = 0.0;
}

#[allow(unused_mut, unused_variables, clippy::all)]
pub fn hdiv_wdgi1_basis(p: [f64; 3], b: &mut [f64]) {
    let (mut rr, mut ss, mut tt) = (p[0], p[1], p[2]);
    let eps = f64::EPSILON;
    b[0 * 3 + 0] = rr * 2.0;
    b[0 * 3 + 1] = (ss - 1.0) * 2.0;
    b[0 * 3 + 2] = 0.0;
    b[1 * 3 + 0] = rr * 2.0;
    b[1 * 3 + 1] = ss * 2.0;
    b[1 * 3 + 2] = 0.0;
    b[2 * 3 + 0] = (rr - 1.0) * 2.0;
    b[2 * 3 + 1] = ss * 2.0;
    b[2 * 3 + 2] = 0.0;
    b[3 * 3 + 0] = 0.0;
    b[3 * 3 + 1] = 0.0;
    b[3 * 3 + 2] = (tt - 1.0) / 2.0;
    b[4 * 3 + 0] = 0.0;
    b[4 * 3 + 1] = 0.0;
    b[4 * 3 + 2] = (1.0 + tt) / 2.0;
}

#[allow(unused_mut, unused_variables, clippy::all)]
pub fn hgrad_edgec1_basis(p: [f64; 3], b: &mut [f64]) {
    let (mut rr, mut ss, mut tt) = (p[0], p[1], p[2]);
    let eps = f64::EPSILON;
    b[0] = 0.5 * (1.0 - rr);
    b[1] = 0.5 * (1.0 + rr);
}

#[allow(unused_mut, unused_variables, clippy::all)]
pub fn hgrad_edgec1_gradient(p: [f64; 3], g: &mut [f64]) {
    let (mut rr, mut ss, mut tt) = (p[0], p[1], p[2]);
    let eps = f64::EPSILON;
    g[0] = -0.5;
    g[1] = 0.;
    g[2] = 0.;
    g[3] = 0.5;
    g[4] = 0.;
    g[5] = 0.;
}

#[allow(unused_mut, unused_variables, clippy::all)]
pub fn hgrad_edgec2_basis(p: [f64; 3], b: &mut [f64]) {
    let (mut rr, mut ss, mut tt) = (p[0], p[1], p[2]);
    let eps = f64::EPSILON;
    b[0] = 0.5 * (1.0 - rr) * rr;
    b[1] = 0.5 * (1.0 + rr) * rr;
    b[2] = (1.0 - rr) * (1.0 + rr);
}

#[allow(unused_mut, unused_variables, clippy::all)]
pub fn hgrad_edgec2_gradient(p: [f64; 3], g: &mut [f64]) {
    let (mut rr, mut ss, mut tt) = (p[0], p[1], p[2]);
    let eps = f64::EPSILON;
    g[0] = 0.5 - rr;
    g[1] = 0.;
    g[2] = 0.;
    g[3] = 0.5 + rr;
    g[4] = 0.;
    g[5] = 0.;
    g[3] = 2.0;
    g[4] = 0.;
    g[5] = 0.;
}

#[allow(unused_mut, unused_variables, clippy::all)]
pub fn hgrad_hexc1_basis(p: [f64; 3], b: &mut [f64]) {
    let (mut rr, mut ss, mut tt) = (p[0], p[1], p[2]);
    let eps = f64::EPSILON;
    b[0] = (1.0 - rr) * (1.0 - ss) * (1.0 - tt) / 8.0;
    b[1] = (1.0 + rr) * (1.0 - ss) * (1.0 - tt) / 8.0;
    b[2] = (1.0 + rr) * (1.0 + ss) * (1.0 - tt) / 8.0;
    b[3] = (1.0 - rr) * (1.0 + ss) * (1.0 - tt) / 8.0;
    b[4] = (1.0 - rr) * (1.0 - ss) * (1.0 + tt) / 8.0;
    b[5] = (1.0 + rr) * (1.0 - ss) * (1.0 + tt) / 8.0;
    b[6] = (1.0 + rr) * (1.0 + ss) * (1.0 + tt) / 8.0;
    b[7] = (1.0 - rr) * (1.0 + ss) * (1.0 + tt) / 8.0;
}

#[allow(unused_mut, unused_variables, clippy::all)]
pub fn hgrad_hexc1_gradient(p: [f64; 3], g: &mut [f64]) {
    let (mut rr, mut ss, mut tt) = (p[0], p[1], p[2]);
    let eps = f64::EPSILON;
    g[0] = -(1.0 - ss) * (1.0 - tt) / 8.0;
    g[1] = -(1.0 - rr) * (1.0 - tt) / 8.0;
    g[2] = -(1.0 - rr) * (1.0 - ss) / 8.0;
    g[3] = (1.0 - ss) * (1.0 - tt) / 8.0;
    g[4] = -(1.0 + rr) * (1.0 - tt) / 8.0;
    g[5] = -(1.0 + rr) * (1.0 - ss) / 8.0;
    g[6] = (1.0 + ss) * (1.0 - tt) / 8.0;
    g[7] = (1.0 + rr) * (1.0 - tt) / 8.0;
    g[8] = -(1.0 + rr) * (1.0 + ss) / 8.0;
    g[9] = -(1.0 + ss) * (1.0 - tt) / 8.0;
    g[10] = (1.0 - rr) * (1.0 - tt) / 8.0;
    g[11] = -(1.0 - rr) * (1.0 + ss) / 8.0;
    g[12] = -(1.0 - ss) * (1.0 + tt) / 8.0;
    g[13] = -(1.0 - rr) * (1.0 + tt) / 8.0;
    g[14] = (1.0 - rr) * (1.0 - ss) / 8.0;
    g[15] = (1.0 - ss) * (1.0 + tt) / 8.0;
    g[16] = -(1.0 + rr) * (1.0 + tt) / 8.0;
    g[17] = (1.0 + rr) * (1.0 - ss) / 8.0;
    g[18] = (1.0 + ss) * (1.0 + tt) / 8.0;
    g[19] = (1.0 + rr) * (1.0 + tt) / 8.0;
    g[20] = (1.0 + rr) * (1.0 + ss) / 8.0;
    g[21] = -(1.0 + ss) * (1.0 + tt) / 8.0;
    g[22] = (1.0 - rr) * (1.0 + tt) / 8.0;
    g[23] = (1.0 - rr) * (1.0 + ss) / 8.0;
}

#[allow(unused_mut, unused_variables, clippy::all)]
pub fn hgrad_hexc2_basis(p: [f64; 3], b: &mut [f64]) {
    let (mut rr, mut ss, mut tt) = (p[0], p[1], p[2]);
    let eps = f64::EPSILON;
    b[0] = 0.125 * (-1. + rr) * rr * (-1. + ss) * ss * (-1. + tt) * tt;
    b[1] = 0.125 * rr * (1. + rr) * (-1. + ss) * ss * (-1. + tt) * tt;
    b[2] = 0.125 * rr * (1. + rr) * ss * (1. + ss) * (-1. + tt) * tt;
    b[3] = 0.125 * (-1. + rr) * rr * ss * (1. + ss) * (-1. + tt) * tt;
    b[4] = 0.125 * (-1. + rr) * rr * (-1. + ss) * ss * tt * (1. + tt);
    b[5] = 0.125 * rr * (1. + rr) * (-1. + ss) * ss * tt * (1. + tt);
    b[6] = 0.125 * rr * (1. + rr) * ss * (1. + ss) * tt * (1. + tt);
    b[7] = 0.125 * (-1. + rr) * rr * ss * (1. + ss) * tt * (1. + tt);
    b[8] = 0.25 * (1. - rr) * (1. + rr) * (-1. + ss) * ss * (-1. + tt) * tt;
    b[9] = 0.25 * rr * (1. + rr) * (1. - ss) * (1. + ss) * (-1. + tt) * tt;
    b[10] = 0.25 * (1. - rr) * (1. + rr) * ss * (1. + ss) * (-1. + tt) * tt;
    b[11] = 0.25 * (-1. + rr) * rr * (1. - ss) * (1. + ss) * (-1. + tt) * tt;
    b[12] = 0.25 * (-1. + rr) * rr * (-1. + ss) * ss * (1. - tt) * (1. + tt);
    b[13] = 0.25 * rr * (1. + rr) * (-1. + ss) * ss * (1. - tt) * (1. + tt);
    b[14] = 0.25 * rr * (1. + rr) * ss * (1. + ss) * (1. - tt) * (1. + tt);
    b[15] = 0.25 * (-1. + rr) * rr * ss * (1. + ss) * (1. - tt) * (1. + tt);
    b[16] = 0.25 * (1. - rr) * (1. + rr) * (-1. + ss) * ss * tt * (1. + tt);
    b[17] = 0.25 * rr * (1. + rr) * (1. - ss) * (1. + ss) * tt * (1. + tt);
    b[18] = 0.25 * (1. - rr) * (1. + rr) * ss * (1. + ss) * tt * (1. + tt);
    b[19] = 0.25 * (-1. + rr) * rr * (1. - ss) * (1. + ss) * tt * (1. + tt);
    b[20] = (1. - rr) * (1. + rr) * (1. - ss) * (1. + ss) * (1. - tt) * (1. + tt);
    b[21] = 0.5 * (1. - rr) * (1. + rr) * (1. - ss) * (1. + ss) * (-1. + tt) * tt;
    b[22] = 0.5 * (1. - rr) * (1. + rr) * (1. - ss) * (1. + ss) * tt * (1. + tt);
    b[23] = 0.5 * (-1. + rr) * rr * (1. - ss) * (1. + ss) * (1. - tt) * (1. + tt);
    b[24] = 0.5 * rr * (1. + rr) * (1. - ss) * (1. + ss) * (1. - tt) * (1. + tt);
    b[25] = 0.5 * (1. - rr) * (1. + rr) * (-1. + ss) * ss * (1. - tt) * (1. + tt);
    b[26] = 0.5 * (1. - rr) * (1. + rr) * ss * (1. + ss) * (1. - tt) * (1. + tt);
}

#[allow(unused_mut, unused_variables, clippy::all)]
pub fn hgrad_hexc2_gradient(p: [f64; 3], g: &mut [f64]) {
    let (mut rr, mut ss, mut tt) = (p[0], p[1], p[2]);
    let eps = f64::EPSILON;
    g[0 * 3 + 0] = (-0.125 + 0.25 * rr) * (-1. + ss) * ss * (-1. + tt) * tt;
    g[0 * 3 + 1] = (-1. + rr) * rr * (-0.125 + 0.25 * ss) * (-1. + tt) * tt;
    g[0 * 3 + 2] = (-1. + rr) * rr * (-1. + ss) * ss * (-0.125 + 0.25 * tt);
    g[1 * 3 + 0] = (0.125 + 0.25 * rr) * (-1. + ss) * ss * (-1. + tt) * tt;
    g[1 * 3 + 1] = rr * (1. + rr) * (-0.125 + 0.25 * ss) * (-1. + tt) * tt;
    g[1 * 3 + 2] = rr * (1. + rr) * (-1. + ss) * ss * (-0.125 + 0.25 * tt);
    g[2 * 3 + 0] = (0.125 + 0.25 * rr) * ss * (1. + ss) * (-1. + tt) * tt;
    g[2 * 3 + 1] = rr * (1. + rr) * (0.125 + 0.25 * ss) * (-1. + tt) * tt;
    g[2 * 3 + 2] = rr * (1. + rr) * ss * (1. + ss) * (-0.125 + 0.25 * tt);
    g[3 * 3 + 0] = (-0.125 + 0.25 * rr) * ss * (1. + ss) * (-1. + tt) * tt;
    g[3 * 3 + 1] = (-1. + rr) * rr * (0.125 + 0.25 * ss) * (-1. + tt) * tt;
    g[3 * 3 + 2] = (-1. + rr) * rr * ss * (1. + ss) * (-0.125 + 0.25 * tt);
    g[4 * 3 + 0] = (-0.125 + 0.25 * rr) * (-1. + ss) * ss * tt * (1. + tt);
    g[4 * 3 + 1] = (-1. + rr) * rr * (-0.125 + 0.25 * ss) * tt * (1. + tt);
    g[4 * 3 + 2] = (-1. + rr) * rr * (-1. + ss) * ss * (0.125 + 0.25 * tt);
    g[5 * 3 + 0] = (0.125 + 0.25 * rr) * (-1. + ss) * ss * tt * (1. + tt);
    g[5 * 3 + 1] = rr * (1. + rr) * (-0.125 + 0.25 * ss) * tt * (1. + tt);
    g[5 * 3 + 2] = rr * (1. + rr) * (-1. + ss) * ss * (0.125 + 0.25 * tt);
    g[6 * 3 + 0] = (0.125 + 0.25 * rr) * ss * (1. + ss) * tt * (1. + tt);
    g[6 * 3 + 1] = rr * (1. + rr) * (0.125 + 0.25 * ss) * tt * (1. + tt);
    g[6 * 3 + 2] = rr * (1. + rr) * ss * (1. + ss) * (0.125 + 0.25 * tt);
    g[7 * 3 + 0] = (-0.125 + 0.25 * rr) * ss * (1. + ss) * tt * (1. + tt);
    g[7 * 3 + 1] = (-1. + rr) * rr * (0.125 + 0.25 * ss) * tt * (1. + tt);
    g[7 * 3 + 2] = (-1. + rr) * rr * ss * (1. + ss) * (0.125 + 0.25 * tt);
    g[8 * 3 + 0] = -0.5 * rr * (-1. + ss) * ss * (-1. + tt) * tt;
    g[8 * 3 + 1] = (1. - rr) * (1. + rr) * (-0.25 + 0.5 * ss) * (-1. + tt) * tt;
    g[8 * 3 + 2] = (1. - rr) * (1. + rr) * (-1. + ss) * ss * (-0.25 + 0.5 * tt);
    g[9 * 3 + 0] = (0.25 + 0.5 * rr) * (1. - ss) * (1. + ss) * (-1. + tt) * tt;
    g[9 * 3 + 1] = rr * (1. + rr) * (-0.5 * ss) * (-1. + tt) * tt;
    g[9 * 3 + 2] = rr * (1. + rr) * (1. - ss) * (1. + ss) * (-0.25 + 0.5 * tt);
    g[10 * 3 + 0] = -0.5 * rr * ss * (1. + ss) * (-1. + tt) * tt;
    g[10 * 3 + 1] = (1. - rr) * (1. + rr) * (0.25 + 0.5 * ss) * (-1. + tt) * tt;
    g[10 * 3 + 2] = (1. - rr) * (1. + rr) * ss * (1. + ss) * (-0.25 + 0.5 * tt);
    g[11 * 3 + 0] = (-0.25 + 0.5 * rr) * (1. - ss) * (1. + ss) * (-1. + tt) * tt;
    g[11 * 3 + 1] = (-1. + rr) * rr * (-0.5 * ss) * (-1. + tt) * tt;
    g[11 * 3 + 2] = (-1. + rr) * rr * (1. - ss) * (1. + ss) * (-0.25 + 0.5 * tt);
    g[12 * 3 + 0] = (-0.25 + 0.5 * rr) * (-1. + ss) * ss * (1. - tt) * (1. + tt);
    g[12 * 3 + 1] = (-1. + rr) * rr * (-0.25 + 0.5 * ss) * (1. - tt) * (1. + tt);
    g[12 * 3 + 2] = (-1. + rr) * rr * (-1. + ss) * ss * (-0.5 * tt);
    g[13 * 3 + 0] = (0.25 + 0.5 * rr) * (-1. + ss) * ss * (1. - tt) * (1. + tt);
    g[13 * 3 + 1] = rr * (1. + rr) * (-0.25 + 0.5 * ss) * (1. - tt) * (1. + tt);
    g[13 * 3 + 2] = rr * (1. + rr) * (-1. + ss) * ss * (-0.5 * tt);
    g[14 * 3 + 0] = (0.25 + 0.5 * rr) * ss * (1. + ss) * (1. - tt) * (1. + tt);
    g[14 * 3 + 1] = rr * (1. + rr) * (0.25 + 0.5 * ss) * (1. - tt) * (1. + tt);
    g[14 * 3 + 2] = rr * (1. + rr) * ss * (1. + ss) * (-0.5 * tt);
    g[15 * 3 + 0] = (-0.25 + 0.5 * rr) * ss * (1. + ss) * (1. - tt) * (1. + tt);
    g[15 * 3 + 1] = (-1. + rr) * rr * (0.25 + 0.5 * ss) * (1. - tt) * (1. + tt);
    g[15 * 3 + 2] = (-1. + rr) * rr * ss * (1. + ss) * (-0.5 * tt);
    g[16 * 3 + 0] = -0.5 * rr * (-1. + ss) * ss * tt * (1. + tt);
    g[16 * 3 + 1] = (1. - rr) * (1. + rr) * (-0.25 + 0.5 * ss) * tt * (1. + tt);
    g[16 * 3 + 2] = (1. - rr) * (1. + rr) * (-1. + ss) * ss * (0.25 + 0.5 * tt);
    g[17 * 3 + 0] = (0.25 + 0.5 * rr) * (1. - ss) * (1. + ss) * tt * (1. + tt);
    g[17 * 3 + 1] = rr * (1. + rr) * (-0.5 * ss) * tt * (1. + tt);
    g[17 * 3 + 2] = rr * (1. + rr) * (1. - ss) * (1. + ss) * (0.25 + 0.5 * tt);
    g[18 * 3 + 0] = -0.5 * rr * ss * (1. + ss) * tt * (1. + tt);
    g[18 * 3 + 1] = (1. - rr) * (1. + rr) * (0.25 + 0.5 * ss) * tt * (1. + tt);
    g[18 * 3 + 2] = (1. - rr) * (1. + rr) * ss * (1. + ss) * (0.25 + 0.5 * tt);
    g[19 * 3 + 0] = (-0.25 + 0.5 * rr) * (1. - ss) * (1. + ss) * tt * (1. + tt);
    g[19 * 3 + 1] = (-1. + rr) * rr * (-0.5 * ss) * tt * (1. + tt);
    g[19 * 3 + 2] = (-1. + rr) * rr * (1. - ss) * (1. + ss) * (0.25 + 0.5 * tt);
    g[20 * 3 + 0] = -2. * rr * (1. - ss) * (1. + ss) * (1. - tt) * (1. + tt);
    g[20 * 3 + 1] = (1. - rr) * (1. + rr) * (-2. * ss) * (1. - tt) * (1. + tt);
    g[20 * 3 + 2] = (1. - rr) * (1. + rr) * (1. - ss) * (1. + ss) * (-2. * tt);
    g[21 * 3 + 0] = -rr * (1. - ss) * (1. + ss) * (-1. + tt) * tt;
    g[21 * 3 + 1] = (1. - rr) * (1. + rr) * (-ss) * (-1. + tt) * tt;
    g[21 * 3 + 2] = (1. - rr) * (1. + rr) * (1. - ss) * (1. + ss) * (-0.5 + tt);
    g[22 * 3 + 0] = -rr * (1. - ss) * (1. + ss) * tt * (1. + tt);
    g[22 * 3 + 1] = (1. - rr) * (1. + rr) * (-ss) * tt * (1. + tt);
    g[22 * 3 + 2] = (1. - rr) * (1. + rr) * (1. - ss) * (1. + ss) * (0.5 + tt);
    g[23 * 3 + 0] = (-0.5 + rr) * (1. - ss) * (1. + ss) * (1. - tt) * (1. + tt);
    g[23 * 3 + 1] = (-1. + rr) * rr * (-ss) * (1. - tt) * (1. + tt);
    g[23 * 3 + 2] = (-1. + rr) * rr * (1. - ss) * (1. + ss) * (-tt);
    g[24 * 3 + 0] = (0.5 + rr) * (1. - ss) * (1. + ss) * (1. - tt) * (1. + tt);
    g[24 * 3 + 1] = rr * (1. + rr) * (-ss) * (1. - tt) * (1. + tt);
    g[24 * 3 + 2] = rr * (1. + rr) * (1. - ss) * (1. + ss) * (-tt);
    g[25 * 3 + 0] = -rr * (-1. + ss) * ss * (1. - tt) * (1. + tt);
    g[25 * 3 + 1] = (1. - rr) * (1. + rr) * (-0.5 + ss) * (1. - tt) * (1. + tt);
    g[25 * 3 + 2] = (1. - rr) * (1. + rr) * (-1. + ss) * ss * (-tt);
    g[26 * 3 + 0] = -rr * ss * (1. + ss) * (1. - tt) * (1. + tt);
    g[26 * 3 + 1] = (1. - rr) * (1. + rr) * (0.5 + ss) * (1. - tt) * (1. + tt);
    g[26 * 3 + 2] = (1. - rr) * (1. + rr) * ss * (1. + ss) * (-tt);
}

#[allow(unused_mut, unused_variables, clippy::all)]
pub fn hgrad_hexi2_basis(p: [f64; 3], b: &mut [f64]) {
    let (mut rr, mut ss, mut tt) = (p[0], p[1], p[2]);
    let eps = f64::EPSILON;
    b[0] = 0.125 * (1.0 - rr) * (1.0 - ss) * (1.0 - tt) * (-rr - ss - tt - 2.0);
    b[1] = 0.125 * (1.0 + rr) * (1.0 - ss) * (1.0 - tt) * (rr - ss - tt - 2.0);
    b[2] = 0.125 * (1.0 + rr) * (1.0 + ss) * (1.0 - tt) * (rr + ss - tt - 2.0);
    b[3] = 0.125 * (1.0 - rr) * (1.0 + ss) * (1.0 - tt) * (-rr + ss - tt - 2.0);
    b[4] = 0.125 * (1.0 - rr) * (1.0 - ss) * (1.0 + tt) * (-rr - ss + tt - 2.0);
    b[5] = 0.125 * (1.0 + rr) * (1.0 - ss) * (1.0 + tt) * (rr - ss + tt - 2.0);
    b[6] = 0.125 * (1.0 + rr) * (1.0 + ss) * (1.0 + tt) * (rr + ss + tt - 2.0);
    b[7] = 0.125 * (1.0 - rr) * (1.0 + ss) * (1.0 + tt) * (-rr + ss + tt - 2.0);
    b[8] = 0.25 * (1.0 - rr * rr) * (1.0 - ss) * (1.0 - tt);
    b[9] = 0.25 * (1.0 + rr) * (1.0 - ss * ss) * (1.0 - tt);
    b[10] = 0.25 * (1.0 - rr * rr) * (1.0 + ss) * (1.0 - tt);
    b[11] = 0.25 * (1.0 - rr) * (1.0 - ss * ss) * (1.0 - tt);
    b[12] = 0.25 * (1.0 - rr) * (1.0 - ss) * (1.0 - tt * tt);
    b[13] = 0.25 * (1.0 + rr) * (1.0 - ss) * (1.0 - tt * tt);
    b[14] = 0.25 * (1.0 + rr) * (1.0 + ss) * (1.0 - tt * tt);
    b[15] = 0.25 * (1.0 - rr) * (1.0 + ss) * (1.0 - tt * tt);
    b[16] = 0.25 * (1.0 - rr * rr) * (1.0 - ss) * (1.0 + tt);
    b[17] = 0.25 * (1.0 + rr) * (1.0 - ss * ss) * (1.0 + tt);
    b[18] = 0.25 * (1.0 - rr * rr) * (1.0 + ss) * (1.0 + tt);
    b[19] = 0.25 * (1.0 - rr) * (1.0 - ss * ss) * (1.0 + tt);
}

#[allow(unused_mut, unused_variables, clippy::all)]
pub fn hgrad_hexi2_gradient(p: [f64; 3], g: &mut [f64]) {
    let (mut rr, mut ss, mut tt) = (p[0], p[1], p[2]);
    let eps = f64::EPSILON;
    g[0 * 3 + 0] = -0.125 * (1.0 - ss) * (1.0 - tt) * (-rr - ss - tt - 2.0) - 0.125 * (1.0 - rr) * (1.0 - ss) * (1.0 - tt);
    g[0 * 3 + 1] = -0.125 * (1.0 - rr) * (1.0 - tt) * (-rr - ss - tt - 2.0) - 0.125 * (1.0 - rr) * (1.0 - ss) * (1.0 - tt);
    g[0 * 3 + 2] = -0.125 * (1.0 - rr) * (1.0 - ss) * (-rr - ss - tt - 2.0) - 0.125 * (1.0 - rr) * (1.0 - ss) * (1.0 - tt);
    g[1 * 3 + 0] = 0.125 * (1.0 - ss) * (1.0 - tt) * (rr - ss - tt - 2.0) + 0.125 * (1.0 + rr) * (1.0 - ss) * (1.0 - tt);
    g[1 * 3 + 1] = -0.125 * (1.0 + rr) * (1.0 - tt) * (rr - ss - tt - 2.0) - 0.125 * (1.0 + rr) * (1.0 - ss) * (1.0 - tt);
    g[1 * 3 + 2] = -0.125 * (1.0 + rr) * (1.0 - ss) * (rr - ss - tt - 2.0) - 0.125 * (1.0 + rr) * (1.0 - ss) * (1.0 - tt);
    g[2 * 3 + 0] = 0.125 * (1.0 + ss) * (1.0 - tt) * (rr + ss - tt - 2.0) + 0.125 * (1.0 + rr) * (1.0 + ss) * (1.0 - tt);
    g[2 * 3 + 1] = 0.125 * (1.0 + rr) * (1.0 - tt) * (rr + ss - tt - 2.0) + 0.125 * (1.0 + rr) * (1.0 + ss) * (1.0 - tt);
    g[2 * 3 + 2] = -0.125 * (1.0 + rr) * (1.0 + ss) * (rr + ss - tt - 2.0) - 0.125 * (1.0 + rr) * (1.0 + ss) * (1.0 - tt);
    g[3 * 3 + 0] = -0.125 * (1.0 + ss) * (1.0 - tt) * (-rr + ss - tt - 2.0) - 0.125 * (1.0 - rr) * (1.0 + ss) * (1.0 - tt);
    g[3 * 3 + 1] = 0.125 * (1.0 - rr) * (1.0 - tt) * (-rr + ss - tt - 2.0) + 0.125 * (1.0 - rr) * (1.0 + ss) * (1.0 - tt);
    g[3 * 3 + 2] = -0.125 * (1.0 - rr) * (1.0 + ss) * (-rr + ss - tt - 2.0) - 0.125 * (1.0 - rr) * (1.0 + ss) * (1.0 - tt);
    g[4 * 3 + 0] = -0.125 * (1.0 - ss) * (1.0 + tt) * (-rr - ss + tt - 2.0) - 0.125 * (1.0 - rr) * (1.0 - ss) * (1.0 + tt);
    g[4 * 3 + 1] = -0.125 * (1.0 - rr) * (1.0 + tt) * (-rr - ss + tt - 2.0) - 0.125 * (1.0 - rr) * (1.0 - ss) * (1.0 + tt);
    g[4 * 3 + 2] = 0.125 * (1.0 - rr) * (1.0 - ss) * (-rr - ss + tt - 2.0) + 0.125 * (1.0 - rr) * (1.0 - ss) * (1.0 + tt);
    g[5 * 3 + 0] = 0.125 * (1.0 - ss) * (1.0 + tt) * (rr - ss + tt - 2.0) + 0.125 * (1.0 + rr) * (1.0 - ss) * (1.0 + tt);
    g[5 * 3 + 1] = -0.125 * (1.0 + rr) * (1.0 + tt) * (rr - ss + tt - 2.0) - 0.125 * (1.0 + rr) * (1.0 - ss) * (1.0 + tt);
    g[5 * 3 + 2] = 0.125 * (1.0 + rr) * (1.0 - ss) * (rr - ss + tt - 2.0) + 0.125 * (1.0 + rr) * (1.0 - ss) * (1.0 + tt);
    g[6 * 3 + 0] = 0.125 * (1.0 + ss) * (1.0 + tt) * (rr + ss + tt - 2.0) + 0.125 * (1.0 + rr) * (1.0 + ss) * (1.0 + tt);
    g[6 * 3 + 1] = 0.125 * (1.0 + rr) * (1.0 + tt) * (rr + ss + tt - 2.0) + 0.125 * (1.0 + rr) * (1.0 + ss) * (1.0 + tt);
    g[6 * 3 + 2] = 0.125 * (1.0 + rr) * (1.0 + ss) * (rr + ss + tt - 2.0) + 0.125 * (1.0 + rr) * (1.0 + ss) * (1.0 + tt);
    g[7 * 3 + 0] = -0.125 * (1.0 + ss) * (1.0 + tt) * (-rr + ss + tt - 2.0) - 0.125 * (1.0 - rr) * (1.0 + ss) * (1.0 + tt);
    g[7 * 3 + 1] = 0.125 * (1.0 - rr) * (1.0 + tt) * (-rr + ss + tt - 2.0) + 0.125 * (1.0 - rr) * (1.0 + ss) * (1.0 + tt);
    g[7 * 3 + 2] = 0.125 * (1.0 - rr) * (1.0 + ss) * (-rr + ss + tt - 2.0) + 0.125 * (1.0 - rr) * (1.0 + ss) * (1.0 + tt);
    g[8 * 3 + 0] = -0.5 * rr * (1.0 - ss) * (1.0 - tt);
    g[8 * 3 + 1] = -0.25 * (1.0 - rr * rr) * (1.0 - tt);
    g[8 * 3 + 2] = -0.25 * (1.0 - rr * rr) * (1.0 - ss);
    g[9 * 3 + 0] = 0.25 * (1.0 - ss * ss) * (1.0 - tt);
    g[9 * 3 + 1] = -0.5 * ss * (1.0 + rr) * (1.0 - tt);
    g[9 * 3 + 2] = -0.25 * (1.0 + rr) * (1.0 - ss * ss);
    g[10 * 3 + 0] = -0.5 * rr * (1.0 + ss) * (1.0 - tt);
    g[10 * 3 + 1] = 0.25 * (1.0 - rr * rr) * (1.0 - tt);
    g[10 * 3 + 2] = -0.25 * (1.0 - rr * rr) * (1.0 + ss);
    g[11 * 3 + 0] = -0.25 * (1.0 - ss * ss) * (1.0 - tt);
    g[11 * 3 + 1] = -0.5 * ss * (1.0 - rr) * (1.0 - tt);
    g[11 * 3 + 2] = -0.25 * (1.0 - rr) * (1.0 - ss * ss);
    g[12 * 3 + 0] = -0.25 * (1.0 - ss) * (1.0 - tt * tt);
    g[12 * 3 + 1] = -0.25 * (1.0 - rr) * (1.0 - tt * tt);
    g[12 * 3 + 2] = -0.5 * tt * (1.0 - rr) * (1.0 - ss);
    g[13 * 3 + 0] = 0.25 * (1.0 - ss) * (1.0 - tt * tt);
    g[13 * 3 + 1] = -0.25 * (1.0 + rr) * (1.0 - tt * tt);
    g[13 * 3 + 2] = -0.5 * tt * (1.0 + rr) * (1.0 - ss);
    g[14 * 3 + 0] = 0.25 * (1.0 + ss) * (1.0 - tt * tt);
    g[14 * 3 + 1] = 0.25 * (1.0 + rr) * (1.0 - tt * tt);
    g[14 * 3 + 2] = -0.5 * tt * (1.0 + rr) * (1.0 + ss);
    g[15 * 3 + 0] = -0.25 * (1.0 + ss) * (1.0 - tt * tt);
    g[15 * 3 + 1] = 0.25 * (1.0 - rr) * (1.0 - tt * tt);
    g[15 * 3 + 2] = -0.5 * tt * (1.0 - rr) * (1.0 + ss);
    g[16 * 3 + 0] = -0.5 * rr * (1.0 - ss) * (1.0 + tt);
    g[16 * 3 + 1] = -0.25 * (1.0 - rr * rr) * (1.0 + tt);
    g[16 * 3 + 2] = 0.25 * (1.0 - rr * rr) * (1.0 - ss);
    g[17 * 3 + 0] = 0.25 * (1.0 - ss * ss) * (1.0 + tt);
    g[17 * 3 + 1] = -0.5 * ss * (1.0 + rr) * (1.0 + tt);
    g[17 * 3 + 2] = 0.25 * (1.0 + rr) * (1.0 - ss * ss);
    g[18 * 3 + 0] = -0.5 * rr * (1.0 + ss) * (1.0 + tt);
    g[18 * 3 + 1] = 0.25 * (1.0 - rr * rr) * (1.0 + tt);
    g[18 * 3 + 2] = 0.25 * (1.0 - rr * rr) * (1.0 + ss);
    g[19 * 3 + 0] = -0.25 * (1.0 - ss * ss) * (1.0 + tt);
    g[19 * 3 + 1] = -0.5 * ss * (1.0 - rr) * (1.0 + tt);
    g[19 * 3 + 2] = 0.25 * (1.0 - rr) * (1.0 - ss * ss);
}

#[allow(unused_mut, unused_variables, clippy::all)]
pub fn hgrad_pyrc1_basis(p: [f64; 3], b: &mut [f64]) {
    let (mut rr, mut ss, mut tt) = (p[0], p[1], p[2]);
    let eps = f64::EPSILON;
    if f64::abs(tt - 1.0) < eps {
        if tt <= 1.0 {
            tt = 1.0 - eps;
        } else {
            tt = 1.0 + eps;
        }
    }
    let mut ttTerm: f64 = 0.25 / (1.0 - tt);
    b[0] = (1.0 - rr - tt) * (1.0 - ss - tt) * ttTerm;
    b[1] = (1.0 + rr - tt) * (1.0 - ss - tt) * ttTerm;
    b[2] = (1.0 + rr - tt) * (1.0 + ss - tt) * ttTerm;
    b[3] = (1.0 - rr - tt) * (1.0 + ss - tt) * ttTerm;
    b[4] = tt;
}

#[allow(unused_mut, unused_variables, clippy::all)]
pub fn hgrad_pyrc1_gradient(p: [f64; 3], g: &mut [f64]) {
    let (mut rr, mut ss, mut tt) = (p[0], p[1], p[2]);
    let eps = f64::EPSILON;
    if f64::abs(tt - 1.0) < eps {
        if tt <= 1.0 {
            tt = 1.0 - eps;
        } else {
            tt = 1.0 + eps;
        }
    }
    let mut ttTerm: f64 = 0.25 / (1.0 - tt);
    let mut ttTerm2: f64 = 4.0 * ttTerm * ttTerm;
    g[0] = (ss + tt - 1.0) * ttTerm;
    g[1] = (rr + tt - 1.0) * ttTerm;
    g[2] = rr * ss * ttTerm2 - 0.25;
    g[3] = (1.0 - ss - tt) * ttTerm;
    g[4] = (tt - rr - 1.0) * ttTerm;
    g[5] = rr * ss * ttTerm2 - 0.25;
    g[6] = (1.0 + ss - tt) * ttTerm;
    g[7] = (1.0 + rr - tt) * ttTerm;
    g[8] = rr * ss * ttTerm2 - 0.25;
    g[9] = (tt - ss - 1.0) * ttTerm;
    g[10] = (1.0 - rr - tt) * ttTerm;
    g[11] = rr * ss * ttTerm2 - 0.25;
    g[12] = 0.0;
    g[13] = 0.0;
    g[14] = 1.0;
}

#[allow(unused_mut, unused_variables, clippy::all)]
pub fn hgrad_pyrc2_basis(p: [f64; 3], b: &mut [f64]) {
    let (mut rr, mut ss, mut tt) = (p[0], p[1], p[2]);
    let eps = f64::EPSILON;
    let mut ZERO: f64 = 0.;
    let mut HALF: f64 = 0.5;
    let mut FOURTH: f64 = 0.25;
    let mut NINTH: f64 = 1. / 9.;
    let mut FOURNINTHS: f64 = 4. / 9.;
    let mut ONE: f64 = 1.;
    let mut TWO: f64 = 2.;
    let mut EIGHT: f64 = 9.;
    let mut TWENTYSEVEN: f64 = 27.;
    let mut K1: f64 = -FOURTH;
    let mut K5: f64 = ONE;
    let mut K6: f64 = -HALF;
    let mut K10: f64 = -ONE;
    let mut K14: f64 = ONE;
    let mut K15A: f64 = TWENTYSEVEN / EIGHT;
    let mut K15B: f64 = -TWENTYSEVEN / EIGHT;
    let mut r2: f64 = rr * rr;
    let mut s2: f64 = ss * ss;
    let mut t2: f64 = tt * tt;
    let mut rs: f64 = rr * ss;
    let mut rt: f64 = rr * tt;
    let mut st: f64 = ss * tt;
    let mut r2s: f64 = r2 * ss;
    let mut r2t: f64 = r2 * tt;
    let mut rs2: f64 = rr * s2;
    let mut s2t: f64 = s2 * tt;
    let mut rt2: f64 = rr * t2;
    let mut st2: f64 = ss * t2;
    let mut r2s2: f64 = r2 * s2;
    let mut tm: f64 = tt - ONE;
    let mut tm2: f64 = tm * tm;
    let mut mt: f64 = if f64::abs(tm) > eps { ONE / tm } else { ZERO };
    let mut m2t: f64 = if f64::abs(tm2) > eps { ONE / tm2 } else { ZERO };
    b[0] = K1 * (rr + ss - ONE) * (rs * mt - rr - ss + tt - ONE);
    b[1] = K1 * (-rr + ss - ONE) * (-rs * mt + rr - ss + tt - ONE);
    b[2] = K1 * (-rr - ss - ONE) * (rs * mt + rr + ss + tt - ONE);
    b[3] = K1 * (rr - ss - ONE) * (-rs * mt - rr + ss + tt - ONE);
    b[4] = K5 * (-tt + TWO * t2);
    b[5] = K6 * (-r2s * mt + r2 + st - ss - tm2);
    b[6] = K6 * (rs2 * mt - rt + rr + s2 - tm2);
    b[7] = K6 * (r2s * mt + r2 - st + ss - tm2);
    b[8] = K6 * (-rs2 * mt + rt - rr + s2 - tm2);
    b[9] = K10 * (rs + rs * mt - rt - st - tt + t2);
    b[10] = K10 * (-rs - rs * mt + rt - st - tt + t2);
    b[11] = K10 * (rs + rs * mt + rt + st - tt + t2);
    b[12] = K10 * (-rs - rs * mt - rt + st - tt + t2);
    b[13] = K14 * (r2s2 * m2t - r2 - s2 + tm2);
    b[14] = K15A * (r2s + r2s * mt - r2s2 * mt - r2s2 * m2t + st - st2 + s2t);
    b[15] = K15B * (rs2 + rs2 * mt + rt - rt2 + r2s2 * mt + r2s2 * m2t - r2t);
    b[16] = K15B * (r2s + r2s * mt + r2s2 * mt + r2s2 * m2t + st - st2 - s2t);
    b[17] = K15A * (rs2 + rs2 * mt + rt - rt2 - r2s2 * mt - r2s2 * m2t + r2t);
    b[0] = b[0] + FOURTH * b[13] + NINTH * (b[14] + b[17]);
    b[1] = b[1] + FOURTH * b[13] + NINTH * (b[14] + b[15]);
    b[2] = b[2] + FOURTH * b[13] + NINTH * (b[15] + b[16]);
    b[3] = b[3] + FOURTH * b[13] + NINTH * (b[16] + b[17]);
    b[4] = b[4] + NINTH * (b[14] + b[15] + b[16] + b[17]);
    b[5] = b[5] - FOURNINTHS * b[14] - b[13] * HALF;
    b[6] = b[6] - FOURNINTHS * b[15] - b[13] * HALF;
    b[7] = b[7] - FOURNINTHS * b[16] - b[13] * HALF;
    b[8] = b[8] - FOURNINTHS * b[17] - b[13] * HALF;
    b[9] = b[9] - FOURNINTHS * (b[14] + b[17]);
    b[10] = b[10] - FOURNINTHS * (b[14] + b[15]);
    b[11] = b[11] - FOURNINTHS * (b[15] + b[16]);
    b[12] = b[12] - FOURNINTHS * (b[16] + b[17]);
}

#[allow(unused_mut, unused_variables, clippy::all)]
pub fn hgrad_pyrc2_gradient(p: [f64; 3], g: &mut [f64]) {
    let (mut rr, mut ss, mut tt) = (p[0], p[1], p[2]);
    let eps = f64::EPSILON;
    let mut ZERO: f64 = 0.;
    let mut HALF: f64 = 0.5;
    let mut FOURTH: f64 = 0.25;
    let mut NINTH: f64 = 1. / 9.;
    let mut FOURNINTHS: f64 = 4. / 9.;
    let mut ONE: f64 = 1.;
    let mut TWO: f64 = 2.;
    let mut FOUR: f64 = 4.;
    let mut EIGHT: f64 = 9.;
    let mut TWENTYSEVEN: f64 = 27.;
    let mut K1: f64 = -FOURTH;
    let mut K5: f64 = ONE;
    let mut K6: f64 = -HALF;
    let mut K10: f64 = -ONE;
    let mut K14: f64 = ONE;
    let mut K15A: f64 = TWENTYSEVEN / EIGHT;
    let mut K15B: f64 = -TWENTYSEVEN / EIGHT;
    let mut r2: f64 = rr * rr;
    let mut s2: f64 = ss * ss;
    let mut t2: f64 = tt * tt;
    let mut rs: f64 = rr * ss;
    let mut rt: f64 = rr * tt;
    let mut st: f64 = ss * tt;
    let mut r2s: f64 = r2 * ss;
    let mut rs2: f64 = rr * s2;
    let mut r2s2: f64 = r2 * s2;
    let mut tm: f64 = tt - ONE;
    let mut tm2: f64 = tm * tm;
    let mut tm3: f64 = tm2 * tm;
    let mut tm4: f64 = tm3 * tm;
    let mut mt: f64 = if f64::abs(tm) > eps { ONE / tm } else { ZERO };
    let mut m2t: f64 = if f64::abs(tm2) > eps { ONE / tm2 } else { ZERO };
    let mut m3t: f64 = if f64::abs(tm3) > eps { ONE / tm3 } else { ZERO };
    let mut m4t: f64 = if f64::abs(tm4) > eps { ONE / tm4 } else { ZERO };
    g[0] = K1 * (rs * mt - rr - ss + tt + (ss * mt - ONE) * (rr + ss - ONE) - ONE);
    g[3] = K1 * (rs * mt - rr + ss - tt + (-ss * mt + ONE) * (-rr + ss - ONE) + ONE);
    g[6] = K1 * (-rs * mt - rr - ss - tt + (ss * mt + ONE) * (-rr - ss - ONE) + ONE);
    g[9] = K1 * (-rs * mt - rr + ss + tt + (-ss * mt - ONE) * (rr - ss - ONE) - ONE);
    g[12] = ZERO;
    g[15] = K6 * (-TWO * rs * mt + TWO * rr);
    g[18] = K6 * (s2 * mt - tt + ONE);
    g[21] = K6 * (TWO * rs * mt + TWO * rr);
    g[24] = K6 * (-s2 * mt + tt - ONE);
    g[27] = K10 * (ss + ss * mt - tt);
    g[30] = K10 * (-ss - ss * mt + tt);
    g[33] = K10 * (ss + ss * mt + tt);
    g[36] = K10 * (-ss - ss * mt - tt);
    g[39] = K14 * (TWO * rs2 * m2t - TWO * rr);
    g[42] = K15A * (TWO * rs + TWO * rs * mt - TWO * rs2 * mt - TWO * rs2 * m2t);
    g[45] = K15B * (TWO * rs2 * mt + TWO * rs2 * m2t - TWO * rt + s2 + s2 * mt + tt - t2);
    g[48] = K15B * (TWO * rs + TWO * rs * mt + TWO * rs2 * mt + TWO * rs2 * m2t);
    g[51] = K15A * (-TWO * rs2 * mt - TWO * rs2 * m2t + TWO * rt + s2 + s2 * mt + tt - t2);
    g[1] = K1 * (rs * mt - rr - ss + tt + (rr * mt - ONE) * (rr + ss - ONE) - ONE);
    g[4] = K1 * (-rs * mt + rr - ss + tt + (-rr * mt - ONE) * (-rr + ss - ONE) - ONE);
    g[7] = K1 * (-rs * mt - rr - ss - tt + (rr * mt + ONE) * (-rr - ss - ONE) + ONE);
    g[10] = K1 * (rs * mt + rr - ss - tt + (-rr * mt + ONE) * (rr - ss - ONE) + ONE);
    g[13] = ZERO;
    g[16] = K6 * (-r2 * mt + tt - ONE);
    g[19] = K6 * (TWO * rs * mt + TWO * ss);
    g[22] = K6 * (r2 * mt - tt + ONE);
    g[25] = K6 * (-TWO * rs * mt + TWO * ss);
    g[28] = K10 * (rr + rr * mt - tt);
    g[31] = K10 * (-rr - rr * mt - tt);
    g[34] = K10 * (rr + rr * mt + tt);
    g[37] = K10 * (-rr - rr * mt + tt);
    g[40] = K14 * (TWO * r2s * m2t - TWO * ss);
    g[43] = K15A * (-TWO * r2s * mt - TWO * r2s * m2t + r2 + r2 * mt + TWO * st + tt - t2);
    g[46] = K15B * (TWO * rs + TWO * rs * mt + TWO * r2s * mt + TWO * r2s * m2t);
    g[49] = K15B * (TWO * r2s * mt + TWO * r2s * m2t + r2 + r2 * mt - TWO * st + tt - t2);
    g[52] = K15A * (TWO * rs + TWO * rs * mt - TWO * r2s * mt - TWO * r2s * m2t);
    g[2] = K1 * (-rs * m2t + ONE) * (rr + ss - ONE);
    g[5] = K1 * (rs * m2t + ONE) * (-rr + ss - ONE);
    g[8] = K1 * (-rs * m2t + ONE) * (-rr - ss - ONE);
    g[11] = K1 * (rs * m2t + ONE) * (rr - ss - ONE);
    g[14] = K5 * (FOUR * tt - ONE);
    g[17] = K6 * (r2s * m2t + ss - TWO * tt + TWO);
    g[20] = K6 * (-rs2 * m2t - rr - TWO * tt + TWO);
    g[23] = K6 * (-r2s * m2t - ss - TWO * tt + TWO);
    g[26] = K6 * (rs2 * m2t + rr - TWO * tt + TWO);
    g[29] = K10 * (-rs * m2t - rr - ss + TWO * tt - ONE);
    g[32] = K10 * (rs * m2t + rr - ss + TWO * tt - ONE);
    g[35] = K10 * (-rs * m2t + rr + ss + TWO * tt - ONE);
    g[38] = K10 * (rs * m2t - rr + ss + TWO * tt - ONE);
    g[41] = K14 * (-TWO * r2s2 * m3t + TWO * tt - TWO);
    g[44] = K15A * (-r2s * m2t - r2s2 * (-TWO * tt + TWO) * m4t + r2s2 * m2t - TWO * st + ss + s2);
    g[47] = K15B * (-rs2 * m2t - TWO * rt + rr + r2s2 * (-TWO * tt + TWO) * m4t - r2s2 * m2t - r2);
    g[50] = K15B * (-r2s * m2t + r2s2 * (-TWO * tt + TWO) * m4t - r2s2 * m2t - TWO * st + ss - s2);
    g[53] = K15A * (-rs2 * m2t - TWO * rt + rr - r2s2 * (-TWO * tt + TWO) * m4t + r2s2 * m2t + r2);
    g[0] = g[0] + g[39] * FOURTH + NINTH * (g[42] + g[51]);
    g[3] = g[3] + g[39] * FOURTH + NINTH * (g[42] + g[45]);
    g[6] = g[6] + g[39] * FOURTH + NINTH * (g[45] + g[48]);
    g[9] = g[9] + g[39] * FOURTH + NINTH * (g[48] + g[51]);
    g[12] = g[12] + NINTH * (g[42] + g[45] + g[48] + g[51]);
    g[15] = g[15] - g[39] * HALF - g[42] * FOURNINTHS;
    g[18] = g[18] - g[39] * HALF - g[45] * FOURNINTHS;
    g[21] = g[21] - g[39] * HALF - g[48] * FOURNINTHS;
    g[24] = g[24] - g[39] * HALF - g[51] * FOURNINTHS;
    g[27] = g[27] + FOURNINTHS * (-g[42] - g[51]);
    g[30] = g[30] + FOURNINTHS * (-g[42] - g[45]);
    g[33] = g[33] + FOURNINTHS * (-g[45] - g[48]);
    g[36] = g[36] + FOURNINTHS * (-g[48] - g[51]);
    g[1] = g[1] + g[40] * FOURTH + NINTH * (g[43] + g[52]);
    g[4] = g[4] + g[40] * FOURTH + NINTH * (g[43] + g[46]);
    g[7] = g[7] + g[40] * FOURTH + NINTH * (g[46] + g[49]);
    g[10] = g[10] + g[40] * FOURTH + NINTH * (g[49] + g[52]);
    g[13] = g[13] + NINTH * (g[43] + g[46] + g[49] + g[52]);
    g[16] = g[16] - g[40] * HALF - g[43] * FOURNINTHS;
    g[19] = g[19] - g[40] * HALF - g[46] * FOURNINTHS;
    g[22] = g[22] - g[40] * HALF - g[49] * FOURNINTHS;
    g[25] = g[25] - g[40] * HALF - g[52] * FOURNINTHS;
    g[28] = g[28] + FOURNINTHS * (-g[43] - g[52]);
    g[31] = g[31] + FOURNINTHS * (-g[43] - g[46]);
    g[34] = g[34] + FOURNINTHS * (-g[46] - g[49]);
    g[37] = g[37] + FOURNINTHS * (-g[49] - g[52]);
    g[2] = g[2] + g[41] * FOURTH + NINTH * (g[44] + g[53]);
    g[5] = g[5] + g[41] * FOURTH + NINTH * (g[44] + g[47]);
    g[8] = g[8] + g[41] * FOURTH + NINTH * (g[47] + g[50]);
    g[11] = g[11] + g[41] * FOURTH + NINTH * (g[50] + g[53]);
    g[14] = g[14] + NINTH * (g[44] + g[47] + g[50] + g[53]);
    g[17] = g[17] - g[41] * HALF - g[44] * FOURNINTHS;
    g[20] = g[20] - g[41] * HALF - g[47] * FOURNINTHS;
    g[23] = g[23] - g[41] * HALF - g[50] * FOURNINTHS;
    g[26] = g[26] - g[41] * HALF - g[53] * FOURNINTHS;
    g[29] = g[29] + FOURNINTHS * (-g[44] - g[53]);
    g[32] = g[32] + FOURNINTHS * (-g[44] - g[47]);
    g[35] = g[35] + FOURNINTHS * (-g[47] - g[50]);
    g[38] = g[38] + FOURNINTHS * (-g[50] - g[53]);
}

#[allow(unused_mut, unused_variables, clippy::all)]
pub fn hgrad_pyrf2_basis(p: [f64; 3], b: &mut [f64]) {
    let (mut rr, mut ss, mut tt) = (p[0], p[1], p[2]);
    let eps = f64::EPSILON;
    let mut HALF: f64 = 0.5;
    let mut ONE: f64 = 1.;
    let mut ZERO: f64 = 0.;
    let mut TWO: f64 = 2.;
    let mut SIXTEENTHIRDS: f64 = 16. / 3.;
    let mut EIGHT: f64 = 8.;
    let mut TWENTYSEVEN: f64 = 27.;
    let mut FOURTH: f64 = 0.25;
    let mut NINTH: f64 = 1. / 9.;
    let mut THREESIXTYFOURTHS: f64 = 3. / 64.;
    let mut EIGHTH: f64 = 1. / 8.;
    let mut FOURNINTHS: f64 = 4. / 9.;
    let mut THREESIXTEENTHS: f64 = 3. / 16.;
    let mut NINESIXTEENTHS: f64 = 9. / 16.;
    let mut K1: f64 = -0.25;
    let mut K5: f64 = ONE;
    let mut K6: f64 = -HALF;
    let mut K10: f64 = -ONE;
    let mut K14: f64 = ONE;
    let mut K15A: f64 = TWENTYSEVEN / EIGHT;
    let mut K15B: f64 = -TWENTYSEVEN / EIGHT;
    let mut K19: f64 = -SIXTEENTHIRDS;
    let mut r2: f64 = rr * rr;
    let mut s2: f64 = ss * ss;
    let mut t2: f64 = tt * tt;
    let mut rs: f64 = rr * ss;
    let mut rt: f64 = rr * tt;
    let mut st: f64 = ss * tt;
    let mut r2s: f64 = r2 * ss;
    let mut r2t: f64 = r2 * tt;
    let mut rs2: f64 = rr * s2;
    let mut s2t: f64 = s2 * tt;
    let mut rt2: f64 = rr * t2;
    let mut st2: f64 = ss * t2;
    let mut r2s2: f64 = r2 * s2;
    let mut tm: f64 = tt - ONE;
    let mut tm2: f64 = tm * tm;
    let mut tm3: f64 = tm2 * tm;
    let mut mt: f64 = if f64::abs(tm) > eps { ONE / tm } else { ZERO };
    let mut m2t: f64 = if f64::abs(tm2) > eps { ONE / tm2 } else { ZERO };
    let mut m3t: f64 = if f64::abs(tm3) > eps { ONE / tm3 } else { ZERO };
    b[0] = K1 * (rr + ss - ONE) * (rs * mt - rr - ss + tt - ONE);
    b[1] = K1 * (-rr + ss - ONE) * (-rs * mt + rr - ss + tt - ONE);
    b[2] = K1 * (-rr - ss - ONE) * (rs * mt + rr + ss + tt - ONE);
    b[3] = K1 * (rr - ss - ONE) * (-rs * mt - rr + ss + tt - ONE);
    b[4] = K5 * (-tt + TWO * t2);
    b[5] = K6 * (-r2s * mt + r2 + st - ss - tm2);
    b[6] = K6 * (rs2 * mt - rt + rr + s2 - tm2);
    b[7] = K6 * (r2s * mt + r2 - st + ss - tm2);
    b[8] = K6 * (-rs2 * mt + rt - rr + s2 - tm2);
    b[9] = K10 * (rs + rs * mt - rt - st - tt + t2);
    b[10] = K10 * (-rs - rs * mt + rt - st - tt + t2);
    b[11] = K10 * (rs + rs * mt + rt + st - tt + t2);
    b[12] = K10 * (-rs - rs * mt - rt + st - tt + t2);
    b[13] = K14 * (r2s2 * m2t - r2 - s2 + tm2);
    b[14] = K15A * (r2s + r2s * mt - r2s2 * mt - r2s2 * m2t + st - st2 + s2t);
    b[15] = K15B * (rs2 + rs2 * mt + rt - rt2 + r2s2 * mt + r2s2 * m2t - r2t);
    b[16] = K15B * (r2s + r2s * mt + r2s2 * mt + r2s2 * m2t + st - st2 - s2t);
    b[17] = K15A * (rs2 + rs2 * mt + rt - rt2 - r2s2 * mt - r2s2 * m2t + r2t);
    b[18] = K19 * (r2s2 * m2t + r2s2 * m3t - r2 - r2 * mt - s2 - s2 * mt - tt + t2);
    b[0] = b[0] + FOURTH * b[13] + NINTH * (b[14] + b[17]) + b[18] * THREESIXTYFOURTHS;
    b[1] = b[1] + FOURTH * b[13] + NINTH * (b[14] + b[15]) + b[18] * THREESIXTYFOURTHS;
    b[2] = b[2] + FOURTH * b[13] + NINTH * (b[15] + b[16]) + b[18] * THREESIXTYFOURTHS;
    b[3] = b[3] + FOURTH * b[13] + NINTH * (b[16] + b[17]) + b[18] * THREESIXTYFOURTHS;
    b[4] = b[4] + EIGHTH * b[18] + NINTH * (b[14] + b[15] + b[16] + b[17]);
    b[5] = b[5] - FOURNINTHS * b[14] - b[13] * HALF;
    b[6] = b[6] - FOURNINTHS * b[15] - b[13] * HALF;
    b[7] = b[7] - FOURNINTHS * b[16] - b[13] * HALF;
    b[8] = b[8] - FOURNINTHS * b[17] - b[13] * HALF;
    b[9] = b[9] - FOURNINTHS * (b[14] + b[17]) - b[18] * THREESIXTEENTHS;
    b[10] = b[10] - FOURNINTHS * (b[14] + b[15]) - b[18] * THREESIXTEENTHS;
    b[11] = b[11] - FOURNINTHS * (b[15] + b[16]) - b[18] * THREESIXTEENTHS;
    b[12] = b[12] - FOURNINTHS * (b[16] + b[17]) - b[18] * THREESIXTEENTHS;
    b[13] = b[13] - b[18] * NINESIXTEENTHS;
}

#[allow(unused_mut, unused_variables, clippy::all)]
pub fn hgrad_pyrf2_gradient(p: [f64; 3], g: &mut [f64]) {
    let (mut rr, mut ss, mut tt) = (p[0], p[1], p[2]);
    let eps = f64::EPSILON;
    let mut HALF: f64 = 0.5;
    let mut ONE: f64 = 1.;
    let mut ZERO: f64 = 0.;
    let mut TWO: f64 = 2.;
    let mut THREE: f64 = 3.;
    let mut FOUR: f64 = 4.;
    let mut SIXTEENTHIRDS: f64 = 16. / 3.;
    let mut EIGHT: f64 = 8.;
    let mut TWENTYSEVEN: f64 = 27.;
    let mut FOURTH: f64 = 0.25;
    let mut NINTH: f64 = 1. / 9.;
    let mut THREESIXTYFOURTHS: f64 = 3. / 64.;
    let mut EIGHTH: f64 = 1. / 8.;
    let mut FOURNINTHS: f64 = 4. / 9.;
    let mut THREESIXTEENTHS: f64 = 3. / 16.;
    let mut NINESIXTEENTHS: f64 = 9. / 16.;
    let mut K1: f64 = -0.25;
    let mut K5: f64 = ONE;
    let mut K6: f64 = -HALF;
    let mut K10: f64 = -ONE;
    let mut K14: f64 = ONE;
    let mut K15A: f64 = TWENTYSEVEN / EIGHT;
    let mut K15B: f64 = -TWENTYSEVEN / EIGHT;
    let mut K19: f64 = -SIXTEENTHIRDS;
    let mut r2: f64 = rr * rr;
    let mut s2: f64 = ss * ss;
    let mut t2: f64 = tt * tt;
    let mut rs: f64 = rr * ss;
    let mut rt: f64 = rr * tt;
    let mut st: f64 = ss * tt;
    let mut r2s: f64 = r2 * ss;
    let mut rs2: f64 = rr * s2;
    let mut r2s2: f64 = r2 * s2;
    let mut tm: f64 = tt - ONE;
    let mut tm2: f64 = tm * tm;
    let mut tm3: f64 = tm2 * tm;
    let mut tm4: f64 = tm3 * tm;
    let mut mt: f64 = if f64::abs(tm) > eps { ONE / tm } else { ZERO };
    let mut m2t: f64 = if f64::abs(tm2) > eps { ONE / tm2 } else { ZERO };
    let mut m3t: f64 = if f64::abs(tm3) > eps { ONE / tm3 } else { ZERO };
    let mut m4t: f64 = if f64::abs(tm4) > eps { ONE / tm4 } else { ZERO };
    g[0] = K1 * (rs * mt - rr - ss + tt + (ss * mt - ONE) * (rr + ss - ONE) - ONE);
    g[1] = K1 * (rs * mt - rr - ss + tt + (rr * mt - ONE) * (rr + ss - ONE) - ONE);
    g[2] = K1 * (-rs * m2t + ONE) * (rr + ss - ONE);
    g[3] = K1 * (rs * mt - rr + ss - tt + (-ss * mt + ONE) * (-rr + ss - ONE) + ONE);
    g[4] = K1 * (-rs * mt + rr - ss + tt + (-rr * mt - ONE) * (-rr + ss - ONE) - ONE);
    g[5] = K1 * (rs * m2t + ONE) * (-rr + ss - ONE);
    g[6] = K1 * (-rs * mt - rr - ss - tt + (ss * mt + ONE) * (-rr - ss - ONE) + ONE);
    g[7] = K1 * (-rs * mt - rr - ss - tt + (rr * mt + ONE) * (-rr - ss - ONE) + ONE);
    g[8] = K1 * (-rs * m2t + ONE) * (-rr - ss - ONE);
    g[9] = K1 * (-rs * mt - rr + ss + tt + (-ss * mt - ONE) * (rr - ss - ONE) - ONE);
    g[10] = K1 * (rs * mt + rr - ss - tt + (-rr * mt + ONE) * (rr - ss - ONE) + ONE);
    g[11] = K1 * (rs * m2t + ONE) * (rr - ss - ONE);
    g[12] = ZERO;
    g[13] = ZERO;
    g[14] = K5 * (FOUR * tt - ONE);
    g[15] = K6 * (-TWO * rs * mt + TWO * rr);
    g[16] = K6 * (-r2 * mt + tt - ONE);
    g[17] = K6 * (r2s * m2t + ss - TWO * tt + TWO);
    g[18] = K6 * (s2 * mt - tt + ONE);
    g[19] = K6 * (TWO * rs * mt + TWO * ss);
    g[20] = K6 * (-rs2 * m2t - rr - TWO * tt + TWO);
    g[21] = K6 * (TWO * rs * mt + TWO * rr);
    g[22] = K6 * (r2 * mt - tt + ONE);
    g[23] = K6 * (-r2s * m2t - ss - TWO * tt + TWO);
    g[24] = K6 * (-s2 * mt + tt - ONE);
    g[25] = K6 * (-TWO * rs * mt + TWO * ss);
    g[26] = K6 * (rs2 * m2t + rr - TWO * tt + TWO);
    g[27] = K10 * (ss + ss * mt - tt);
    g[28] = K10 * (rr + rr * mt - tt);
    g[29] = K10 * (-rs * m2t - rr - ss + TWO * tt - ONE);
    g[30] = K10 * (-ss - ss * mt + tt);
    g[31] = K10 * (-rr - rr * mt - tt);
    g[32] = K10 * (rs * m2t + rr - ss + TWO * tt - ONE);
    g[33] = K10 * (ss + ss * mt + tt);
    g[34] = K10 * (rr + rr * mt + tt);
    g[35] = K10 * (-rs * m2t + rr + ss + TWO * tt - ONE);
    g[36] = K10 * (-ss - ss * mt - tt);
    g[37] = K10 * (-rr - rr * mt + tt);
    g[38] = K10 * (rs * m2t - rr + ss + TWO * tt - ONE);
    g[39] = K14 * (TWO * rs2 * m2t - TWO * rr);
    g[40] = K14 * (TWO * r2s * m2t - TWO * ss);
    g[41] = K14 * (-TWO * r2s2 * m3t + TWO * tt - TWO);
    g[42] = K15A * (TWO * rs + TWO * rs * mt - TWO * rs2 * mt - TWO * rs2 * m2t);
    g[43] = K15A * (-TWO * r2s * mt - TWO * r2s * m2t + r2 + r2 * mt + TWO * st + tt - t2);
    g[44] = K15A * (-r2s * m2t - r2s2 * (-TWO * tt + TWO) * m4t + r2s2 * m2t - TWO * st + ss + s2);
    g[45] = K15B * (TWO * rs2 * mt + TWO * rs2 * m2t - TWO * rt + s2 + s2 * mt + tt - t2);
    g[46] = K15B * (TWO * rs + TWO * rs * mt + TWO * r2s * mt + TWO * r2s * m2t);
    g[47] = K15B * (-rs2 * m2t - TWO * rt + rr + r2s2 * (-TWO * tt + TWO) * m4t - r2s2 * m2t - r2);
    g[48] = K15B * (TWO * rs + TWO * rs * mt + TWO * rs2 * mt + TWO * rs2 * m2t);
    g[49] = K15B * (TWO * r2s * mt + TWO * r2s * m2t + r2 + r2 * mt - TWO * st + tt - t2);
    g[50] = K15B * (-r2s * m2t + r2s2 * (-TWO * tt + TWO) * m4t - r2s2 * m2t - TWO * st + ss - s2);
    g[51] = K15A * (-TWO * rs2 * mt - TWO * rs2 * m2t + TWO * rt + s2 + s2 * mt + tt - t2);
    g[52] = K15A * (TWO * rs + TWO * rs * mt - TWO * r2s * mt - TWO * r2s * m2t);
    g[53] = K15A * (-rs2 * m2t - TWO * rt + rr - r2s2 * (-TWO * tt + TWO) * m4t + r2s2 * m2t + r2);
    g[54] = K19 * (TWO * rs2 * m2t + TWO * rs2 * m3t - TWO * rr - TWO * rr * mt);
    g[55] = K19 * (TWO * r2s * m2t + TWO * r2s * m3t - TWO * ss - TWO * ss * mt);
    g[56] = K19 * (-TWO * r2s2 * m3t - THREE * r2s2 * m4t + r2 * m2t + s2 * m2t + TWO * tt - ONE);
    g[0] = g[0] + g[39] * FOURTH + g[54] * THREESIXTYFOURTHS + NINTH * (g[42] + g[51]);
    g[3] = g[3] + g[39] * FOURTH + g[54] * THREESIXTYFOURTHS + NINTH * (g[42] + g[45]);
    g[6] = g[6] + g[39] * FOURTH + g[54] * THREESIXTYFOURTHS + NINTH * (g[45] + g[48]);
    g[9] = g[9] + g[39] * FOURTH + g[54] * THREESIXTYFOURTHS + NINTH * (g[48] + g[51]);
    g[12] = g[12] + g[54] * EIGHTH + NINTH * (g[42] + g[45] + g[48] + g[51]);
    g[15] = g[15] - g[39] * HALF - g[42] * FOURNINTHS;
    g[18] = g[18] - g[39] * HALF - g[45] * FOURNINTHS;
    g[21] = g[21] - g[39] * HALF - g[48] * FOURNINTHS;
    g[24] = g[24] - g[39] * HALF - g[51] * FOURNINTHS;
    g[27] = g[27] - g[54] * THREESIXTEENTHS + FOURNINTHS * (-g[42] - g[51]);
    g[30] = g[30] - g[54] * THREESIXTEENTHS + FOURNINTHS * (-g[42] - g[45]);
    g[33] = g[33] - g[54] * THREESIXTEENTHS + FOURNINTHS * (-g[45] - g[48]);
    g[36] = g[36] - g[54] * THREESIXTEENTHS + FOURNINTHS * (-g[48] - g[51]);
    g[39] = g[39] - g[54] * NINESIXTEENTHS;
    g[1] = g[1] + g[40] * FOURTH + g[55] * THREESIXTYFOURTHS + NINTH * (g[43] + g[52]);
    g[4] = g[4] + g[40] * FOURTH + g[55] * THREESIXTYFOURTHS + NINTH * (g[43] + g[46]);
    g[7] = g[7] + g[40] * FOURTH + g[55] * THREESIXTYFOURTHS + NINTH * (g[46] + g[49]);
    g[10] = g[10] + g[40] * FOURTH + g[55] * THREESIXTYFOURTHS + NINTH * (g[49] + g[52]);
    g[13] = g[13] + g[55] * EIGHTH + NINTH * (g[43] + g[46] + g[49] + g[52]);
    g[16] = g[16] - g[40] * HALF - g[43] * FOURNINTHS;
    g[19] = g[19] - g[40] * HALF - g[46] * FOURNINTHS;
    g[22] = g[22] - g[40] * HALF - g[49] * FOURNINTHS;
    g[25] = g[25] - g[40] * HALF - g[52] * FOURNINTHS;
    g[28] = g[28] - g[55] * THREESIXTEENTHS + FOURNINTHS * (-g[43] - g[52]);
    g[31] = g[31] - g[55] * THREESIXTEENTHS + FOURNINTHS * (-g[43] - g[46]);
    g[34] = g[34] - g[55] * THREESIXTEENTHS + FOURNINTHS * (-g[46] - g[49]);
    g[37] = g[37] - g[55] * THREESIXTEENTHS + FOURNINTHS * (-g[49] - g[52]);
    g[40] = g[40] - g[55] * NINESIXTEENTHS;
    g[2] = g[2] + g[41] * FOURTH + g[56] * THREESIXTYFOURTHS + NINTH * (g[44] + g[53]);
    g[5] = g[5] + g[41] * FOURTH + g[56] * THREESIXTYFOURTHS + NINTH * (g[44] + g[47]);
    g[8] = g[8] + g[41] * FOURTH + g[56] * THREESIXTYFOURTHS + NINTH * (g[47] + g[50]);
    g[11] = g[11] + g[41] * FOURTH + g[56] * THREESIXTYFOURTHS + NINTH * (g[50] + g[53]);
    g[14] = g[14] + g[56] * EIGHTH + NINTH * (g[44] + g[47] + g[50] + g[53]);
    g[17] = g[17] - g[41] * HALF - g[44] * FOURNINTHS;
    g[20] = g[20] - g[41] * HALF - g[47] * FOURNINTHS;
    g[23] = g[23] - g[41] * HALF - g[50] * FOURNINTHS;
    g[26] = g[26] - g[41] * HALF - g[53] * FOURNINTHS;
    g[29] = g[29] - g[56] * THREESIXTEENTHS + FOURNINTHS * (-g[44] - g[53]);
    g[32] = g[32] - g[56] * THREESIXTEENTHS + FOURNINTHS * (-g[44] - g[47]);
    g[35] = g[35] - g[56] * THREESIXTEENTHS + FOURNINTHS * (-g[47] - g[50]);
    g[38] = g[38] - g[56] * THREESIXTEENTHS + FOURNINTHS * (-g[50] - g[53]);
    g[41] = g[41] - g[56] * NINESIXTEENTHS;
}

#[allow(unused_mut, unused_variables, clippy::all)]
pub fn hgrad_pyri2_basis(p: [f64; 3], b: &mut [f64]) {
    let (mut rr, mut ss, mut tt) = (p[0], p[1], p[2]);
    let eps = f64::EPSILON;
    if f64::abs(tt - 1.0) < eps {
        if tt <= 1.0 {
            tt = 1.0 - eps;
        } else {
            tt = 1.0 + eps;
        }
    }
    let mut ww: f64 = 1.0 / (1.0 - tt);
    b[0] = 0.25 * (-rr - ss - 1.0) * ((1.0 - rr) * (1.0 - ss) - tt + rr * ss * tt * ww);
    b[1] = 0.25 * (rr - ss - 1.0) * ((1.0 + rr) * (1.0 - ss) - tt - rr * ss * tt * ww);
    b[2] = 0.25 * (rr + ss - 1.0) * ((1.0 + rr) * (1.0 + ss) - tt + rr * ss * tt * ww);
    b[3] = 0.25 * (-rr + ss - 1.0) * ((1.0 - rr) * (1.0 + ss) - tt - rr * ss * tt * ww);
    b[4] = tt * (2.0 * tt - 1.0);
    b[5] = 0.5 * (1.0 + rr - tt) * (1.0 - rr - tt) * (1.0 - ss - tt) * ww;
    b[6] = 0.5 * (1.0 + ss - tt) * (1.0 - ss - tt) * (1.0 + rr - tt) * ww;
    b[7] = 0.5 * (1.0 + rr - tt) * (1.0 - rr - tt) * (1.0 + ss - tt) * ww;
    b[8] = 0.5 * (1.0 + ss - tt) * (1.0 - ss - tt) * (1.0 - rr - tt) * ww;
    b[9] = tt * (1.0 - rr - tt) * (1.0 - ss - tt) * ww;
    b[10] = tt * (1.0 + rr - tt) * (1.0 - ss - tt) * ww;
    b[11] = tt * (1.0 + rr - tt) * (1.0 + ss - tt) * ww;
    b[12] = tt * (1.0 - rr - tt) * (1.0 + ss - tt) * ww;
}

#[allow(unused_mut, unused_variables, clippy::all)]
pub fn hgrad_pyri2_gradient(p: [f64; 3], g: &mut [f64]) {
    let (mut rr, mut ss, mut tt) = (p[0], p[1], p[2]);
    let eps = f64::EPSILON;
    if f64::abs(tt - 1.0) < eps {
        if tt <= 1.0 {
            tt = 1.0 - eps;
        } else {
            tt = 1.0 + eps;
        }
    }
    let mut ww: f64 = 1.0 / (1.0 - tt);
    g[0] = 0.25 * (-1.0 - rr - ss) * (-1.0 + ss + ss * tt * ww) - 0.25 * ((1.0 - rr) * (1.0 - ss) - tt + rr * ss * tt * ww);
    g[1] = 0.25 * (-1.0 - rr - ss) * (-1.0 + rr + rr * tt * ww) - 0.25 * ((1.0 - rr) * (1.0 - ss) - tt + rr * ss * tt * ww);
    g[2] = 0.25 * (-1.0 - rr - ss) * (-1.0 + rr * ss * ww + rr * ss * tt * ww * ww);
    g[3] = 0.25 * (-1.0 + rr - ss) * (1.0 - ss - ss * tt * ww) + 0.25 * ((1.0 + rr) * (1.0 - ss) - tt - rr * ss * tt * ww);
    g[4] = 0.25 * (-1.0 + rr - ss) * (-1.0 - rr - rr * tt * ww) - 0.25 * ((1.0 + rr) * (1.0 - ss) - tt - rr * ss * tt * ww);
    g[5] = 0.25 * (-1.0 + rr - ss) * (-1.0 - rr * ss * ww - rr * ss * tt * ww * ww);
    g[6] = 0.25 * (-1.0 + rr + ss) * (1.0 + ss + ss * tt * ww) + 0.25 * ((1.0 + rr) * (1.0 + ss) - tt + rr * ss * tt * ww);
    g[7] = 0.25 * (-1.0 + rr + ss) * (1.0 + rr + rr * tt * ww) + 0.25 * ((1.0 + rr) * (1.0 + ss) - tt + rr * ss * tt * ww);
    g[8] = 0.25 * (-1.0 + rr + ss) * (-1.0 + rr * ss * ww + rr * ss * tt * ww * ww);
    g[9] = 0.25 * (-1.0 - rr + ss) * (-1.0 - ss - ss * tt * ww) - 0.25 * ((1.0 - rr) * (1.0 + ss) - tt - rr * ss * tt * ww);
    g[10] = 0.25 * (-1.0 - rr + ss) * (1.0 - rr - rr * tt * ww) + 0.25 * ((1.0 - rr) * (1.0 + ss) - tt - rr * ss * tt * ww);
    g[11] = 0.25 * (-1.0 - rr + ss) * (-1.0 - rr * ss * ww - rr * ss * tt * ww * ww);
    g[12] = 0.0;
    g[13] = 0.0;
    g[14] = -1.0 + 4.0 * tt;
    g[15] = -rr * ww * (1.0 - ss - tt);
    g[16] = -0.5 * (1.0 - rr - tt) * (1.0 + rr - tt) * ww;
    g[17] = 0.5 * ss * rr * rr * ww * ww + 0.5 * ss - 1.0 + tt;
    g[18] = 0.5 * (1.0 - ss - tt) * (1.0 + ss - tt) * ww;
    g[19] = -ss * ww * (1.0 + rr - tt);
    g[20] = -0.5 * rr * ss * ss * ww * ww - 0.5 * rr - 1.0 + tt;
    g[21] = -rr * ww * (1.0 + ss - tt);
    g[22] = 0.5 * (1.0 - rr - tt) * (1.0 + rr - tt) * ww;
    g[23] = -0.5 * ss * rr * rr * ww * ww - 0.5 * ss - 1.0 + tt;
    g[24] = -0.5 * (1.0 - ss - tt) * (1.0 + ss - tt) * ww;
    g[25] = -ss * ww * (1.0 - rr - tt);
    g[26] = 0.5 * rr * ss * ss * ww * ww + 0.5 * rr - 1.0 + tt;
    g[27] = -(1.0 - ss - tt) * tt * ww;
    g[28] = -(1.0 - rr - tt) * tt * ww;
    g[29] = rr * ss * ww * ww + 1.0 - rr - ss - 2.0 * tt;
    g[30] = (1.0 - ss - tt) * tt * ww;
    g[31] = -(1.0 + rr - tt) * tt * ww;
    g[32] = -rr * ss * ww * ww + 1.0 + rr - ss - 2.0 * tt;
    g[33] = (1.0 + ss - tt) * tt * ww;
    g[34] = (1.0 + rr - tt) * tt * ww;
    g[35] = rr * ss * ww * ww + 1.0 + rr + ss - 2.0 * tt;
    g[36] = -(1.0 + ss - tt) * tt * ww;
    g[37] = (1.0 - rr - tt) * tt * ww;
    g[38] = -rr * ss * ww * ww + 1.0 - rr + ss - 2.0 * tt;
}

#[allow(unused_mut, unused_variables, clippy::all)]
pub fn hgrad_quadc1_basis(p: [f64; 3], b: &mut [f64]) {
    let (mut rr, mut ss, mut tt) = (p[0], p[1], p[2]);
    let eps = f64::EPSILON;
    b[0] = (1.0 - rr) * (1.0 - ss) / 4.0;
    b[1] = (1.0 + rr) * (1.0 - ss) / 4.0;
    b[2] = (1.0 + rr) * (1.0 + ss) / 4.0;
    b[3] = (1.0 - rr) * (1.0 + ss) / 4.0;
}

#[allow(unused_mut, unused_variables, clippy::all)]
pub fn hgrad_quadc1_gradient(p: [f64; 3], g: &mut [f64]) {
    let (mut rr, mut ss, mut tt) = (p[0], p[1], p[2]);
    let eps = f64::EPSILON;
    g[0] = -(1.0 - ss) / 4.0;
    g[1] = -(1.0 - rr) / 4.0;
    g[2] = 0.0;
    g[3] = (1.0 - ss) / 4.0;
    g[4] = -(1.0 + rr) / 4.0;
    g[5] = 0.0;
    g[6] = (1.0 + ss) / 4.0;
    g[7] = (1.0 + rr) / 4.0;
    g[8] = 0.0;
    g[9] = -(1.0 + ss) / 4.0;
    g[10] = (1.0 - rr) / 4.0;
    g[11] = 0.0;
}

#[allow(unused_mut, unused_variables, clippy::all)]
pub fn hgrad_quadc2_basis(p: [f64; 3], b: &mut [f64]) {
    let (mut rr, mut ss, mut tt) = (p[0], p[1], p[2]);
    let eps = f64::EPSILON;
    b[0] = rr * (rr - 1.0) * ss * (ss - 1.0) / 4.0;
    b[1] = rr * (rr + 1.0) * ss * (ss - 1.0) / 4.0;
    b[2] = rr * (rr + 1.0) * ss * (ss + 1.0) / 4.0;
    b[3] = rr * (rr - 1.0) * ss * (ss + 1.0) / 4.0;
    b[4] = (1.0 - rr) * (1.0 + rr) * ss * (ss - 1.0) / 2.0;
    b[5] = rr * (rr + 1.0) * (1.0 - ss) * (1.0 + ss) / 2.0;
    b[6] = (1.0 - rr) * (1.0 + rr) * ss * (ss + 1.0) / 2.0;
    b[7] = rr * (rr - 1.0) * (1.0 - ss) * (1.0 + ss) / 2.0;
    b[8] = (1.0 - rr) * (1.0 + rr) * (1.0 - ss) * (1.0 + ss);
}

#[allow(unused_mut, unused_variables, clippy::all)]
pub fn hgrad_quadc2_gradient(p: [f64; 3], g: &mut [f64]) {
    let (mut rr, mut ss, mut tt) = (p[0], p[1], p[2]);
    let eps = f64::EPSILON;
    g[0] = (-0.25 + 0.5 * rr) * (-1. + ss) * ss;
    g[1] = (-1.0 + rr) * rr * (-0.25 + 0.5 * ss);
    g[2] = 0.0;
    g[3] = (0.25 + 0.5 * rr) * (-1. + ss) * ss;
    g[4] = rr * (1. + rr) * (-0.25 + 0.5 * ss);
    g[5] = 0.0;
    g[6] = (0.25 + 0.5 * rr) * ss * (1. + ss);
    g[7] = rr * (1. + rr) * (0.25 + 0.5 * ss);
    g[8] = 0.0;
    g[9] = (-0.25 + 0.5 * rr) * ss * (1. + ss);
    g[10] = (-1. + rr) * rr * (0.25 + 0.5 * ss);
    g[11] = 0.0;
    g[12] = rr * (1.0 - ss) * ss;
    g[13] = 0.5 * (1.0 - rr) * (1.0 + rr) * (-1.0 + 2.0 * ss);
    g[14] = 0.0;
    g[15] = 0.5 * (1.0 - ss) * (1.0 + ss) * (1.0 + 2.0 * rr);
    g[16] = -rr * (1.0 + rr) * ss;
    g[17] = 0.0;
    g[18] = -ss * (1.0 + ss) * rr;
    g[19] = 0.5 * (1.0 - rr) * (1.0 + rr) * (1.0 + 2.0 * ss);
    g[20] = 0.0;
    g[21] = 0.5 * (1.0 - ss) * (1.0 + ss) * (-1.0 + 2.0 * rr);
    g[22] = (1.0 - rr) * rr * ss;
    g[23] = 0.0;
    g[24] = -2.0 * (1.0 - ss) * (1.0 + ss) * rr;
    g[25] = -2.0 * (1.0 - rr) * (1.0 + rr) * ss;
    g[26] = 0.0;
}

#[allow(unused_mut, unused_variables, clippy::all)]
pub fn hgrad_tetc1_basis(p: [f64; 3], b: &mut [f64]) {
    let (mut rr, mut ss, mut tt) = (p[0], p[1], p[2]);
    let eps = f64::EPSILON;
    b[0] = 1.0 - rr - ss - tt;
    b[1] = rr;
    b[2] = ss;
    b[3] = tt;
}

#[allow(unused_mut, unused_variables, clippy::all)]
pub fn hgrad_tetc1_gradient(p: [f64; 3], g: &mut [f64]) {
    let (mut rr, mut ss, mut tt) = (p[0], p[1], p[2]);
    let eps = f64::EPSILON;
    g[0] = -1.;
    g[1] = -1.;
    g[2] = -1.;
    g[3] = 1.;
    g[4] = 0.;
    g[5] = 0.;
    g[6] = 0.;
    g[7] = 1.;
    g[8] = 0.;
    g[9] = 0.;
    g[10] = 0.;
    g[11] = 1.;
}

#[allow(unused_mut, unused_variables, clippy::all)]
pub fn hgrad_tetc2_basis(p: [f64; 3], b: &mut [f64]) {
    let (mut rr, mut ss, mut tt) = (p[0], p[1], p[2]);
    let eps = f64::EPSILON;
    b[0] = (-1. + rr + ss + tt) * (-1. + 2. * rr + 2. * ss + 2. * tt);
    b[1] = rr * (-1. + 2. * rr);
    b[2] = ss * (-1. + 2. * ss);
    b[3] = tt * (-1. + 2. * tt);
    b[4] = -4. * rr * (-1. + rr + ss + tt);
    b[5] = 4. * rr * ss;
    b[6] = -4. * ss * (-1. + rr + ss + tt);
    b[7] = -4. * tt * (-1. + rr + ss + tt);
    b[8] = 4. * rr * tt;
    b[9] = 4. * ss * tt;
}

#[allow(unused_mut, unused_variables, clippy::all)]
pub fn hgrad_tetc2_gradient(p: [f64; 3], g: &mut [f64]) {
    let (mut rr, mut ss, mut tt) = (p[0], p[1], p[2]);
    let eps = f64::EPSILON;
    g[0] = -3. + 4. * rr + 4. * ss + 4. * tt;
    g[1] = -3. + 4. * rr + 4. * ss + 4. * tt;
    g[2] = -3. + 4. * rr + 4. * ss + 4. * tt;
    g[3] = -1. + 4. * rr;
    g[4] = 0.;
    g[5] = 0.;
    g[6] = 0.;
    g[7] = -1. + 4. * ss;
    g[8] = 0.;
    g[9] = 0.;
    g[10] = 0.;
    g[11] = -1. + 4. * tt;
    g[12] = -4. * (-1. + 2.0 * rr + ss + tt);
    g[13] = -4. * rr;
    g[14] = -4. * rr;
    g[15] = 4. * ss;
    g[16] = 4. * rr;
    g[17] = 0.;
    g[18] = -4. * ss;
    g[19] = -4. * (-1. + rr + 2.0 * ss + tt);
    g[20] = -4. * ss;
    g[21] = -4. * tt;
    g[22] = -4. * tt;
    g[23] = -4. * (-1. + rr + ss + 2.0 * tt);
    g[24] = 4. * tt;
    g[25] = 0.;
    g[26] = 4. * rr;
    g[27] = 0.;
    g[28] = 4. * tt;
    g[29] = 4. * ss;
}

#[allow(unused_mut, unused_variables, clippy::all)]
pub fn hgrad_tetf2_basis(p: [f64; 3], b: &mut [f64]) {
    let (mut rr, mut ss, mut tt) = (p[0], p[1], p[2]);
    let eps = f64::EPSILON;
    let mut uu: f64 = 1. - rr - ss - tt;
    let mut ur: f64 = uu * rr;
    let mut us: f64 = uu * ss;
    let mut ut: f64 = uu * tt;
    let mut rs: f64 = rr * ss;
    let mut rt: f64 = rr * tt;
    let mut st: f64 = ss * tt;
    let mut urs: f64 = ur * ss;
    let mut urt: f64 = ur * tt;
    let mut ust: f64 = us * tt;
    let mut rst: f64 = rs * tt;
    let mut urst: f64 = urs * tt;
    b[0] = uu - 2.0 * (ur + us + ut) + 3.0 * (urs + urt + ust) - 4.0 * urst;
    b[1] = rr - 2.0 * (ur + rs + rt) + 3.0 * (urs + urt + rst) - 4.0 * urst;
    b[2] = ss - 2.0 * (rs + us + st) + 3.0 * (urs + rst + ust) - 4.0 * urst;
    b[3] = tt - 2.0 * (ut + rt + st) + 3.0 * (urt + ust + rst) - 4.0 * urst;
    b[4] = 4.0 * ur - 12.0 * (urs + urt) + 32.0 * urst;
    b[5] = 4.0 * rs - 12.0 * (urs + rst) + 32.0 * urst;
    b[6] = 4.0 * us - 12.0 * (urs + ust) + 32.0 * urst;
    b[7] = 4.0 * ut - 12.0 * (urt + ust) + 32.0 * urst;
    b[8] = 4.0 * rt - 12.0 * (urt + rst) + 32.0 * urst;
    b[9] = 4.0 * st - 12.0 * (rst + ust) + 32.0 * urst;
    b[10] = 27.0 * urs - 108.0 * urst;
    b[11] = 27.0 * urt - 108.0 * urst;
    b[12] = 27.0 * rst - 108.0 * urst;
    b[13] = 27.0 * ust - 108.0 * urst;
    b[14] = 256.0 * urst;
}

#[allow(unused_mut, unused_variables, clippy::all)]
pub fn hgrad_tetf2_gradient(p: [f64; 3], g: &mut [f64]) {
    let (mut rr, mut ss, mut tt) = (p[0], p[1], p[2]);
    let eps = f64::EPSILON;
    let mut uu: f64 = 1. - rr - ss - tt;
    let mut rs: f64 = rr * ss;
    let mut rt: f64 = rr * tt;
    let mut st: f64 = ss * tt;
    let mut umr: f64 = uu - rr;
    let mut ums: f64 = uu - ss;
    let mut umt: f64 = uu - tt;
    g[0] = 1.0 - 4.0 * uu + 3.0 * ((ss + tt) * umr - st) - 4.0 * st * umr;
    g[1] = 1.0 - 4.0 * uu + 3.0 * ((rr + tt) * ums - rt) - 4.0 * rt * ums;
    g[2] = 1.0 - 4.0 * uu + 3.0 * ((rr + ss) * umt - rs) - 4.0 * rs * umt;
    g[3] = 1.0 - 2.0 * (umr + ss + tt) + 3.0 * ((ss + tt) * umr + st) - 4.0 * st * umr;
    g[4] = 3.0 * rr * ums - 4.0 * rt * ums;
    g[5] = 3.0 * rr * umt - 4.0 * rs * umt;
    g[6] = 3.0 * ss * umr - 4.0 * st * umr;
    g[7] = 1.0 - 2.0 * (rr + ums + tt) + 3.0 * ((rr + tt) * ums + rt) - 4.0 * rt * ums;
    g[8] = 3.0 * ss * umt - 4.0 * rs * umt;
    g[9] = 3.0 * tt * umr - 4.0 * st * umr;
    g[10] = 3.0 * tt * ums - 4.0 * rt * ums;
    g[11] = 1.0 - 2.0 * (umt + rr + ss) + 3.0 * ((rr + ss) * umt + rs) - 4.0 * rs * umt;
    g[12] = 4.0 * umr - 12.0 * umr * (ss + tt) + 32.0 * st * umr;
    g[13] = -4.0 * rr - 12.0 * rr * (ums - tt) + 32.0 * rt * ums;
    g[14] = -4.0 * rr - 12.0 * rr * (umt - ss) + 32.0 * rs * umt;
    g[15] = 4.0 * ss - 12.0 * ss * (umr + tt) + 32.0 * st * umr;
    g[16] = 4.0 * rr - 12.0 * rr * (ums + tt) + 32.0 * rt * ums;
    g[17] = 32.0 * rs * umt;
    g[18] = -4.0 * ss - 12.0 * ss * (umr - tt) + 32.0 * st * umr;
    g[19] = 4.0 * ums - 12.0 * ums * (rr + tt) + 32.0 * rt * ums;
    g[20] = -4.0 * ss - 12.0 * ss * (umt - rr) + 32.0 * rs * umt;
    g[21] = -4.0 * tt - 12.0 * tt * (umr - ss) + 32.0 * st * umr;
    g[22] = -4.0 * tt - 12.0 * tt * (ums - rr) + 32.0 * rt * ums;
    g[23] = 4.0 * umt - 12.0 * umt * (rr + ss) + 32.0 * rs * umt;
    g[24] = 4.0 * tt - 12.0 * tt * (umr + ss) + 32.0 * st * umr;
    g[25] = 32.0 * rt * ums;
    g[26] = 4.0 * rr - 12.0 * rr * (umt + ss) + 32.0 * rs * umt;
    g[27] = 32.0 * st * umr;
    g[28] = 4.0 * tt - 12.0 * tt * (rr + ums) + 32.0 * rt * ums;
    g[29] = 4.0 * ss - 12.0 * ss * (rr + umt) + 32.0 * rs * umt;
    g[30] = 27.0 * ss * umr - 108.0 * st * umr;
    g[31] = 27.0 * rr * ums - 108.0 * rt * ums;
    g[32] = -27.0 * rs - 108.0 * rs * umt;
    g[33] = 27.0 * tt * umr - 108.0 * st * umr;
    g[34] = -27.0 * rt - 108.0 * rt * ums;
    g[35] = 27.0 * rr * umt - 108.0 * rs * umt;
    g[36] = 27.0 * st - 108.0 * st * umr;
    g[37] = 27.0 * rt - 108.0 * rt * ums;
    g[38] = 27.0 * rs - 108.0 * rs * umt;
    g[39] = -27.0 * st - 108.0 * st * umr;
    g[40] = 27.0 * tt * ums - 108.0 * rt * ums;
    g[41] = 27.0 * ss * umt - 108.0 * rs * umt;
    g[42] = 256.0 * st * umr;
    g[43] = 256.0 * rt * ums;
    g[44] = 256.0 * rs * umt;
}

#[allow(unused_mut, unused_variables, clippy::all)]
pub fn hgrad_tric1_basis(p: [f64; 3], b: &mut [f64]) {
    let (mut rr, mut ss, mut tt) = (p[0], p[1], p[2]);
    let eps = f64::EPSILON;
    b[0] = 1.0 - rr - ss;
    b[1] = rr;
    b[2] = ss;
}

#[allow(unused_mut, unused_variables, clippy::all)]
pub fn hgrad_tric1_gradient(p: [f64; 3], g: &mut [f64]) {
    let (mut rr, mut ss, mut tt) = (p[0], p[1], p[2]);
    let eps = f64::EPSILON;
    g[0] = -1.;
    g[1] = -1.;
    g[2] = 0.;
    g[3] = 1.;
    g[4] = 0.;
    g[5] = 0.;
    g[6] = 0.;
    g[7] = 1.;
    g[8] = 0.;
}

#[allow(unused_mut, unused_variables, clippy::all)]
pub fn hgrad_tric2_basis(p: [f64; 3], b: &mut [f64]) {
    let (mut rr, mut ss, mut tt) = (p[0], p[1], p[2]);
    let eps = f64::EPSILON;
    b[0] = (rr + ss - 1.0) * (2.0 * rr + 2.0 * ss - 1.0);
    b[1] = rr * (2.0 * rr - 1.0);
    b[2] = ss * (2.0 * ss - 1.0);
    b[3] = -4.0 * rr * (rr + ss - 1.0);
    b[4] = 4.0 * rr * ss;
    b[5] = -4.0 * ss * (rr + ss - 1.0);
}

#[allow(unused_mut, unused_variables, clippy::all)]
pub fn hgrad_tric2_gradient(p: [f64; 3], g: &mut [f64]) {
    let (mut rr, mut ss, mut tt) = (p[0], p[1], p[2]);
    let eps = f64::EPSILON;
    g[0] = 4.0 * rr + 4.0 * ss - 3.0;
    g[1] = 4.0 * rr + 4.0 * ss - 3.0;
    g[2] = 0.0;
    g[3] = 4.0 * rr - 1.0;
    g[4] = 0.0;
    g[5] = 0.0;
    g[6] = 0.0;
    g[7] = 4.0 * ss - 1.0;
    g[8] = 0.0;
    g[9] = -4.0 * (2.0 * rr + ss - 1.0);
    g[10] = -4.0 * rr;
    g[11] = 0.0;
    g[12] = 4.0 * ss;
    g[13] = 4.0 * rr;
    g[14] = 0.0;
    g[15] = -4.0 * ss;
    g[16] = -4.0 * (rr + 2.0 * ss - 1.0);
    g[17] = 0.0;
}

#[allow(unused_mut, unused_variables, clippy::all)]
pub fn hgrad_wdgc1_basis(p: [f64; 3], b: &mut [f64]) {
    let (mut rr, mut ss, mut tt) = (p[0], p[1], p[2]);
    let eps = f64::EPSILON;
    b[0] = (1.0 - rr - ss) * (1.0 - tt) / 2.0;
    b[1] = rr * (1.0 - tt) / 2.0;
    b[2] = ss * (1.0 - tt) / 2.0;
    b[3] = (1.0 - rr - ss) * (1.0 + tt) / 2.0;
    b[4] = rr * (1.0 + tt) / 2.0;
    b[5] = ss * (1.0 + tt) / 2.0;
}

#[allow(unused_mut, unused_variables, clippy::all)]
pub fn hgrad_wdgc1_gradient(p: [f64; 3], g: &mut [f64]) {
    let (mut rr, mut ss, mut tt) = (p[0], p[1], p[2]);
    let eps = f64::EPSILON;
    g[0] = -(1.0 - tt) / 2.0;
    g[1] = -(1.0 - tt) / 2.0;
    g[2] = -(1.0 - rr - ss) / 2.0;
    g[3] = (1.0 - tt) / 2.0;
    g[4] = 0.0;
    g[5] = -rr / 2.0;
    g[6] = 0.0;
    g[7] = (1.0 - tt) / 2.0;
    g[8] = -ss / 2.0;
    g[9] = -(1.0 + tt) / 2.0;
    g[10] = -(1.0 + tt) / 2.0;
    g[11] = (1.0 - rr - ss) / 2.0;
    g[12] = (1.0 + tt) / 2.0;
    g[13] = 0.0;
    g[14] = rr / 2.0;
    g[15] = 0.0;
    g[16] = (1.0 + tt) / 2.0;
    g[17] = ss / 2.0;
}

#[allow(unused_mut, unused_variables, clippy::all)]
pub fn hgrad_wdgc2_basis(p: [f64; 3], b: &mut [f64]) {
    let (mut rr, mut ss, mut tt) = (p[0], p[1], p[2]);
    let eps = f64::EPSILON;
    b[0] = ((-1. + rr + ss) * (-1. + 2. * rr + 2. * ss) * (-1. + tt) * tt) / 2.;
    b[1] = (rr * (-1. + 2. * rr) * (-1. + tt) * tt) / 2.;
    b[2] = (ss * (-1. + 2. * ss) * (-1. + tt) * tt) / 2.;
    b[3] = ((-1. + rr + ss) * (-1. + 2. * rr + 2. * ss) * tt * (1. + tt)) / 2.;
    b[4] = (rr * (-1. + 2. * rr) * tt * (1. + tt)) / 2.;
    b[5] = (ss * (-1. + 2. * ss) * tt * (1. + tt)) / 2.;
    b[6] = -2. * rr * (-1. + rr + ss) * (-1. + tt) * tt;
    b[7] = 2. * rr * ss * (-1. + tt) * tt;
    b[8] = -2. * ss * (-1. + rr + ss) * (-1. + tt) * tt;
    b[9] = -((-1. + rr + ss) * (-1. + 2. * rr + 2. * ss) * (-1. + tt) * (1. + tt));
    b[10] = -(rr * (-1. + 2. * rr) * (-1. + tt) * (1. + tt));
    b[11] = -(ss * (-1. + 2. * ss) * (-1. + tt) * (1. + tt));
    b[12] = -2. * rr * (-1. + rr + ss) * tt * (1. + tt);
    b[13] = 2. * rr * ss * tt * (1. + tt);
    b[14] = -2. * ss * (-1. + rr + ss) * tt * (1. + tt);
    b[15] = 4. * rr * (-1. + rr + ss) * (-1. + tt) * (1. + tt);
    b[16] = -4. * rr * ss * (-1. + tt) * (1. + tt);
    b[17] = 4. * ss * (-1. + rr + ss) * (-1. + tt) * (1. + tt);
}

#[allow(unused_mut, unused_variables, clippy::all)]
pub fn hgrad_wdgc2_gradient(p: [f64; 3], g: &mut [f64]) {
    let (mut rr, mut ss, mut tt) = (p[0], p[1], p[2]);
    let eps = f64::EPSILON;
    g[0] = ((-3.0 + 4.0 * rr + 4.0 * ss) * (-1.0 + tt) * tt) / 2.;
    g[1] = ((-3.0 + 4.0 * rr + 4.0 * ss) * (-1.0 + tt) * tt) / 2.;
    g[2] = ((-1.0 + rr + ss) * (-1.0 + 2.0 * rr + 2.0 * ss) * (-1.0 + 2.0 * tt)) / 2.;
    g[3] = ((-1.0 + 4.0 * rr) * (-1.0 + tt) * tt) / 2.;
    g[4] = 0.;
    g[5] = (rr * (-1.0 + 2.0 * rr) * (-1.0 + 2.0 * tt)) / 2.;
    g[6] = 0.;
    g[7] = ((-1.0 + 4.0 * ss) * (-1.0 + tt) * tt) / 2.;
    g[8] = (ss * (-1.0 + 2.0 * ss) * (-1.0 + 2.0 * tt)) / 2.;
    g[9] = ((-3.0 + 4.0 * rr + 4.0 * ss) * tt * (1.0 + tt)) / 2.;
    g[10] = ((-3.0 + 4.0 * rr + 4.0 * ss) * tt * (1.0 + tt)) / 2.;
    g[11] = ((-1.0 + rr + ss) * (-1.0 + 2.0 * rr + 2.0 * ss) * (1.0 + 2.0 * tt)) / 2.;
    g[12] = ((-1.0 + 4.0 * rr) * tt * (1.0 + tt)) / 2.;
    g[13] = 0.;
    g[14] = (rr * (-1.0 + 2.0 * rr) * (1.0 + 2.0 * tt)) / 2.;
    g[15] = 0.;
    g[16] = ((-1.0 + 4.0 * ss) * tt * (1.0 + tt)) / 2.;
    g[17] = (ss * (-1.0 + 2.0 * ss) * (1.0 + 2.0 * tt)) / 2.;
    g[18] = -2.0 * (-1.0 + 2.0 * rr + ss) * (-1.0 + tt) * tt;
    g[19] = -2.0 * rr * (-1.0 + tt) * tt;
    g[20] = 2.0 * rr * (-1.0 + rr + ss) * (1.0 - 2.0 * tt);
    g[21] = 2.0 * ss * (-1.0 + tt) * tt;
    g[22] = 2.0 * rr * (-1.0 + tt) * tt;
    g[23] = 2.0 * rr * ss * (-1.0 + 2.0 * tt);
    g[24] = -2.0 * ss * (-1.0 + tt) * tt;
    g[25] = -2.0 * (-1.0 + rr + 2.0 * ss) * (-1.0 + tt) * tt;
    g[26] = 2.0 * ss * (-1.0 + rr + ss) * (1.0 - 2.0 * tt);
    g[27] = -(-3.0 + 4.0 * rr + 4.0 * ss) * (-1.0 + tt * tt);
    g[28] = -(-3.0 + 4.0 * rr + 4.0 * ss) * (-1.0 + tt * tt);
    g[29] = -2.0 * (1.0 + 2.0 * rr * rr - 3.0 * ss + 2.0 * ss * ss + rr * (-3.0 + 4.0 * ss)) * tt;
    g[30] = -(-1.0 + 4.0 * rr) * (-1.0 + tt * tt);
    g[31] = 0.0;
    g[32] = 2.0 * (1.0 - 2.0 * rr) * rr * tt;
    g[33] = 0.0;
    g[34] = -(-1.0 + 4.0 * ss) * (-1.0 + tt * tt);
    g[35] = 2.0 * (1.0 - 2.0 * ss) * ss * tt;
    g[36] = -2.0 * (-1.0 + 2.0 * rr + ss) * tt * (1.0 + tt);
    g[37] = -2.0 * rr * tt * (1.0 + tt);
    g[38] = -2.0 * rr * (-1.0 + rr + ss) * (1.0 + 2.0 * tt);
    g[39] = 2.0 * ss * tt * (1.0 + tt);
    g[40] = 2.0 * rr * tt * (1.0 + tt);
    g[41] = 2.0 * rr * ss * (1.0 + 2.0 * tt);
    g[42] = -2.0 * ss * tt * (1.0 + tt);
    g[43] = -2.0 * (-1.0 + rr + 2.0 * ss) * tt * (1.0 + tt);
    g[44] = -2.0 * ss * (-1.0 + rr + ss) * (1.0 + 2.0 * tt);
    g[45] = 4.0 * (-1.0 + 2.0 * rr + ss) * (-1.0 + tt * tt);
    g[46] = 4.0 * rr * (-1.0 + tt) * (1.0 + tt);
    g[47] = 8.0 * rr * (-1.0 + rr + ss) * tt;
    g[48] = -4.0 * ss * (-1.0 + tt) * (1.0 + tt);
    g[49] = -4.0 * rr * (-1.0 + tt) * (1.0 + tt);
    g[50] = -8.0 * rr * ss * tt;
    g[51] = 4.0 * ss * (-1.0 + tt) * (1.0 + tt);
    g[52] = 4.0 * (-1.0 + rr + 2.0 * ss) * (-1.0 + tt * tt);
    g[53] = 8.0 * ss * (-1.0 + rr + ss) * tt;
}

#[allow(unused_mut, unused_variables, clippy::all)]
pub fn hgrad_wdgf2_basis(p: [f64; 3], b: &mut [f64]) {
    let (mut rr, mut ss, mut tt) = (p[0], p[1], p[2]);
    let eps = f64::EPSILON;
    let mut uu: f64 = 1.0 - rr - ss;
    let mut rs: f64 = rr * ss;
    let mut tp: f64 = 1. + tt;
    let mut tm: f64 = 1. - tt;
    b[0] = -0.5 * tt * tm * uu * (1.0 - 2.0 * (rr + ss) + 3.0 * rs);
    b[1] = -0.5 * tt * tm * (rr - 2.0 * (uu * rr + rs) + 3.0 * uu * rs);
    b[2] = -0.5 * tt * tm * (ss - 2.0 * (uu * ss + rs) + 3.0 * uu * rs);
    b[3] = 0.5 * tt * tp * uu * (1.0 - 2.0 * (rr + ss) + 3.0 * rs);
    b[4] = 0.5 * tt * tp * (rr - 2.0 * (uu * rr + rs) + 3.0 * uu * rs);
    b[5] = 0.5 * tt * tp * (ss - 2.0 * (uu * ss + rs) + 3.0 * uu * rs);
    b[6] = -0.5 * tt * tm * uu * (4.0 * rr - 12.0 * rs);
    b[7] = -0.5 * tt * tm * (4.0 * rs - 12.0 * uu * rs);
    b[8] = -0.5 * tt * tm * uu * (4.0 * ss - 12.0 * rs);
    b[9] = 0.5 * tt * tp * uu * (4.0 * rr - 12.0 * rs);
    b[10] = 0.5 * tt * tp * (4.0 * rs - 12.0 * uu * rs);
    b[11] = 0.5 * tt * tp * uu * (4.0 * ss - 12.0 * rs);
    b[12] = tp * tm * uu * (1.0 - 2.0 * (rr + ss) + 3.0 * rs);
    b[13] = tp * tm * (rr - 2.0 * (uu * rr + rs) + 3.0 * uu * rs);
    b[14] = tp * tm * (ss - 2.0 * (uu * ss + rs) + 3.0 * uu * rs);
    b[15] = -0.5 * 27.0 * tt * tm * uu * rs;
    b[16] = 0.5 * 27.0 * tt * tp * uu * rs;
    b[17] = tp * tm * uu * (4.0 * rr - 12.0 * rs);
    b[18] = tp * tm * (4.0 * rs - 12.0 * uu * rs);
    b[19] = tp * tm * uu * (4.0 * ss - 12.0 * rs);
    b[20] = 27.0 * tp * tm * uu * rs;
}

#[allow(unused_mut, unused_variables, clippy::all)]
pub fn hgrad_wdgf2_gradient(p: [f64; 3], g: &mut [f64]) {
    let (mut rr, mut ss, mut tt) = (p[0], p[1], p[2]);
    let eps = f64::EPSILON;
    let mut uu: f64 = 1.0 - rr - ss;
    let mut rs: f64 = rr * ss;
    let mut tp: f64 = 1. + tt;
    let mut tm: f64 = 1. - tt;
    g[0] = -0.5 * tt * tm * (3.0 * ss - 2.0) * uu + 0.5 * tt * tm * (3.0 * rs - 2.0 * rr - 2.0 * ss + 1.0);
    g[1] = -0.5 * tt * tm * (3.0 * rr - 2.0) * uu + 0.5 * tt * tm * (3.0 * rs - 2.0 * rr - 2.0 * ss + 1.0);
    g[2] = 0.5 * tt * uu * (3.0 * rs - 2.0 * rr - 2.0 * ss + 1.0) - 0.5 * tm * uu * (3.0 * rs - 2.0 * rr - 2.0 * ss + 1.0);
    g[3] = -0.5 * tt * tm * (-3.0 * rs + 4.0 * rr + ss * (-3.0 * rr - 3.0 * ss + 3.0) - 1.0);
    g[4] = -0.5 * tt * tm * (-3.0 * rs + rr * (-3.0 * rr - 3.0 * ss + 3.0));
    g[5] = 0.5 * tt * (rs * (-3.0 * rr - 3.0 * ss + 3.0) - 2.0 * rs - 2.0 * rr * uu + rr)
        - 0.5 * tm * (rs * (-3.0 * rr - 3.0 * ss + 3.0) - 2.0 * rs - 2.0 * rr * uu + rr);
    g[6] = -0.5 * tt * tm * (-3.0 * rs + ss * (-3.0 * rr - 3.0 * ss + 3.0));
    g[7] = -0.5 * tt * tm * (-3.0 * rs + rr * (-3.0 * rr - 3.0 * ss + 3.0) + 4.0 * ss - 1.0);
    g[8] = 0.5 * tt * (rs * (-3.0 * rr - 3.0 * ss + 3.0) - 2.0 * rs - 2.0 * ss * uu + ss)
        - 0.5 * tm * (rs * (-3.0 * rr - 3.0 * ss + 3.0) - 2.0 * rs - 2.0 * ss * uu + ss);
    g[9] = 0.5 * tt * (3.0 * ss - 2.0) * tp * uu - 0.5 * tt * tp * (3.0 * rs - 2.0 * rr - 2.0 * ss + 1.0);
    g[10] = 0.5 * tt * (3.0 * rr - 2.0) * tp * uu - 0.5 * tt * tp * (3.0 * rs - 2.0 * rr - 2.0 * ss + 1.0);
    g[11] = 0.5 * tt * uu * (3.0 * rs - 2.0 * rr - 2.0 * ss + 1.0) + 0.5 * tp * uu * (3.0 * rs - 2.0 * rr - 2.0 * ss + 1.0);
    g[12] = 0.5 * tt * tp * (-3.0 * rs + 4.0 * rr + ss * (-3.0 * rr - 3.0 * ss + 3.0) - 1.0);
    g[13] = 0.5 * tt * tp * (-3.0 * rs + rr * (-3.0 * rr - 3.0 * ss + 3.0));
    g[14] = 0.5 * tt * (rs * (-3.0 * rr - 3.0 * ss + 3.0) - 2.0 * rs - 2.0 * rr * uu + rr)
        + 0.5 * tp * (rs * (-3.0 * rr - 3.0 * ss + 3.0) - 2.0 * rs - 2.0 * rr * uu + rr);
    g[15] = 0.5 * tt * tp * (-3.0 * rs + ss * (-3.0 * rr - 3.0 * ss + 3.0));
    g[16] = 0.5 * tt * tp * (-3.0 * rs + rr * (-3.0 * rr - 3.0 * ss + 3.0) + 4.0 * ss - 1.0);
    g[17] = 0.5 * tt * (rs * (-3.0 * rr - 3.0 * ss + 3.0) - 2.0 * rs - 2.0 * ss * uu + ss)
        + 0.5 * tp * (rs * (-3.0 * rr - 3.0 * ss + 3.0) - 2.0 * rs - 2.0 * ss * uu + ss);
    g[18] = -0.5 * tt * tm * (4.0 - 12.0 * ss) * uu + 0.5 * tt * tm * (-12.0 * rs + 4.0 * rr);
    g[19] = 6.0 * rr * tt * tm * uu + 0.5 * tt * tm * (-12.0 * rs + 4.0 * rr);
    g[20] = 0.5 * tt * (-12.0 * rs + 4.0 * rr) * uu - 0.5 * tm * (-12.0 * rs + 4.0 * rr) * uu;
    g[21] = -0.5 * tt * tm * (12.0 * rs - ss * (-12.0 * rr - 12.0 * ss + 12.0) + 4.0 * ss);
    g[22] = -0.5 * tt * tm * (12.0 * rs - rr * (-12.0 * rr - 12.0 * ss + 12.0) + 4.0 * rr);
    g[23] = 0.5 * tt * (-rs * (-12.0 * rr - 12.0 * ss + 12.0) + 4.0 * rs) - 0.5 * tm * (-rs * (-12.0 * rr - 12.0 * ss + 12.0) + 4.0 * rs);
    g[24] = 6.0 * ss * tt * tm * uu + 0.5 * tt * tm * (-12.0 * rs + 4.0 * ss);
    g[25] = -0.5 * tt * tm * (4.0 - 12.0 * rr) * uu + 0.5 * tt * tm * (-12.0 * rs + 4.0 * ss);
    g[26] = 0.5 * tt * (-12.0 * rs + 4.0 * ss) * uu - 0.5 * tm * (-12.0 * rs + 4.0 * ss) * uu;
    g[27] = 0.5 * tt * (4.0 - 12.0 * ss) * tp * uu - 0.5 * tt * tp * (-12.0 * rs + 4.0 * rr);
    g[28] = -6.0 * rr * tt * tp * uu - 0.5 * tt * tp * (-12.0 * rs + 4.0 * rr);
    g[29] = 0.5 * tt * (-12.0 * rs + 4.0 * rr) * uu + 0.5 * tp * (-12.0 * rs + 4.0 * rr) * uu;
    g[30] = 0.5 * tt * tp * (12.0 * rs - ss * (-12.0 * rr - 12.0 * ss + 12.0) + 4.0 * ss);
    g[31] = 0.5 * tt * tp * (12.0 * rs - rr * (-12.0 * rr - 12.0 * ss + 12.0) + 4.0 * rr);
    g[32] = 0.5 * tt * (-rs * (-12.0 * rr - 12.0 * ss + 12.0) + 4.0 * rs) + 0.5 * tp * (-rs * (-12.0 * rr - 12.0 * ss + 12.0) + 4.0 * rs);
    g[33] = -6.0 * ss * tt * tp * uu - 0.5 * tt * tp * (-12.0 * rs + 4.0 * ss);
    g[34] = 0.5 * tt * (4.0 - 12.0 * rr) * tp * uu - 0.5 * tt * tp * (-12.0 * rs + 4.0 * ss);
    g[35] = 0.5 * tt * (-12.0 * rs + 4.0 * ss) * uu + 0.5 * tp * (-12.0 * rs + 4.0 * ss) * uu;
    g[36] = tm * (3.0 * ss - 2.0) * tp * uu - tm * tp * (3.0 * rs - 2.0 * rr - 2.0 * ss + 1.0);
    g[37] = tm * (3.0 * rr - 2.0) * tp * uu - tm * tp * (3.0 * rs - 2.0 * rr - 2.0 * ss + 1.0);
    g[38] = tm * uu * (3.0 * rs - 2.0 * rr - 2.0 * ss + 1.0) - tp * uu * (3.0 * rs - 2.0 * rr - 2.0 * ss + 1.0);
    g[39] = tm * tp * (-3.0 * rs + 4.0 * rr + ss * (-3.0 * rr - 3.0 * ss + 3.0) - 1.0);
    g[40] = tm * tp * (-3.0 * rs + rr * (-3.0 * rr - 3.0 * ss + 3.0));
    g[41] = tm * (rs * (-3.0 * rr - 3.0 * ss + 3.0) - 2.0 * rs - 2.0 * rr * uu + rr) - tp * (rs * (-3.0 * rr - 3.0 * ss + 3.0) - 2.0 * rs - 2.0 * rr * uu + rr);
    g[42] = tm * tp * (-3.0 * rs + ss * (-3.0 * rr - 3.0 * ss + 3.0));
    g[43] = tm * tp * (-3.0 * rs + rr * (-3.0 * rr - 3.0 * ss + 3.0) + 4.0 * ss - 1.0);
    g[44] = tm * (rs * (-3.0 * rr - 3.0 * ss + 3.0) - 2.0 * rs - 2.0 * ss * uu + ss) - tp * (rs * (-3.0 * rr - 3.0 * ss + 3.0) - 2.0 * rs - 2.0 * ss * uu + ss);
    g[45] = 13.5 * rs * tt * tm - 13.5 * ss * tt * tm * uu;
    g[46] = 13.5 * rs * tt * tm - 13.5 * rr * tt * tm * uu;
    g[47] = 13.5 * rs * tt * uu - 13.5 * rs * tm * uu;
    g[48] = -13.5 * rs * tt * tp + 13.5 * ss * tt * tp * uu;
    g[49] = -13.5 * rs * tt * tp + 13.5 * rr * tt * tp * uu;
    g[50] = 13.5 * rs * tt * uu + 13.5 * rs * tp * uu;
    g[51] = tm * (4.0 - 12.0 * ss) * tp * uu - tm * tp * (-12.0 * rs + 4.0 * rr);
    g[52] = -12.0 * rr * tm * tp * uu - tm * tp * (-12.0 * rs + 4.0 * rr);
    g[53] = tm * (-12.0 * rs + 4.0 * rr) * uu - tp * (-12.0 * rs + 4.0 * rr) * uu;
    g[54] = tm * tp * (12.0 * rs - ss * (-12.0 * rr - 12.0 * ss + 12.0) + 4.0 * ss);
    g[55] = tm * tp * (12.0 * rs - rr * (-12.0 * rr - 12.0 * ss + 12.0) + 4.0 * rr);
    g[56] = tm * (-rs * (-12.0 * rr - 12.0 * ss + 12.0) + 4.0 * rs) - tp * (-rs * (-12.0 * rr - 12.0 * ss + 12.0) + 4.0 * rs);
    g[57] = -12.0 * ss * tm * tp * uu - tm * tp * (-12.0 * rs + 4.0 * ss);
    g[58] = tm * (4.0 - 12.0 * rr) * tp * uu - tm * tp * (-12.0 * rs + 4.0 * ss);
    g[59] = tm * (-12.0 * rs + 4.0 * ss) * uu - tp * (-12.0 * rs + 4.0 * ss) * uu;
    g[60] = -rs * tm * (27.0 * tt + 27.0) + ss * tm * (27.0 * tt + 27.0) * uu;
    g[61] = -rs * tm * (27.0 * tt + 27.0) + rr * tm * (27.0 * tt + 27.0) * uu;
    g[62] = 27.0 * rs * tm * uu - rs * (27.0 * tt + 27.0) * uu;
}

#[allow(unused_mut, unused_variables, clippy::all)]
pub fn hgrad_wdgi2_basis(p: [f64; 3], b: &mut [f64]) {
    let (mut rr, mut ss, mut tt) = (p[0], p[1], p[2]);
    let eps = f64::EPSILON;
    let mut ww: f64 = 1.0 - rr - ss;
    b[0] = 0.5 * ww * (2.0 * ww - 2.0 - tt) * (1.0 - tt);
    b[1] = 0.5 * rr * (2.0 * rr - 2.0 - tt) * (1.0 - tt);
    b[2] = 0.5 * ss * (2.0 * ss - 2.0 - tt) * (1.0 - tt);
    b[3] = 0.5 * ww * (2.0 * ww - 2.0 + tt) * (1.0 + tt);
    b[4] = 0.5 * rr * (2.0 * rr - 2.0 + tt) * (1.0 + tt);
    b[5] = 0.5 * ss * (2.0 * ss - 2.0 + tt) * (1.0 + tt);
    b[6] = 2.0 * ww * rr * (1.0 - tt);
    b[7] = 2.0 * rr * ss * (1.0 - tt);
    b[8] = 2.0 * ss * ww * (1.0 - tt);
    b[9] = ww * (1.0 - tt * tt);
    b[10] = rr * (1.0 - tt * tt);
    b[11] = ss * (1.0 - tt * tt);
    b[12] = 2.0 * ww * rr * (1.0 + tt);
    b[13] = 2.0 * rr * ss * (1.0 + tt);
    b[14] = 2.0 * ss * ww * (1.0 + tt);
}

#[allow(unused_mut, unused_variables, clippy::all)]
pub fn hgrad_wdgi2_gradient(p: [f64; 3], g: &mut [f64]) {
    let (mut rr, mut ss, mut tt) = (p[0], p[1], p[2]);
    let eps = f64::EPSILON;
    let mut ww: f64 = 1.0 - rr - ss;
    g[0] = -(2.0 * ww - 1.0 - 0.5 * tt) * (1.0 - tt);
    g[1] = -(2.0 * ww - 1.0 - 0.5 * tt) * (1.0 - tt);
    g[2] = -0.5 * ww * (2.0 * ww - 1.0 - 2.0 * tt);
    g[3] = (2.0 * rr - 1.0 - 0.5 * tt) * (1.0 - tt);
    g[4] = 0.0;
    g[5] = -0.5 * rr * (2.0 * rr - 1.0 - 2.0 * tt);
    g[6] = 0.0;
    g[7] = (2.0 * ss - 1.0 - 0.5 * tt) * (1.0 - tt);
    g[8] = -0.5 * ss * (2.0 * ss - 1.0 - 2.0 * tt);
    g[9] = -(2.0 * ww - 1.0 + 0.5 * tt) * (1.0 + tt);
    g[10] = -(2.0 * ww - 1.0 + 0.5 * tt) * (1.0 + tt);
    g[11] = 0.5 * ww * (2.0 * ww - 1.0 + 2.0 * tt);
    g[12] = (2.0 * rr - 1.0 + 0.5 * tt) * (1.0 + tt);
    g[13] = 0.0;
    g[14] = 0.5 * rr * (2.0 * rr - 1.0 + 2.0 * tt);
    g[15] = 0.0;
    g[16] = (2.0 * ss - 1.0 + 0.5 * tt) * (1.0 + tt);
    g[17] = 0.5 * ss * (2.0 * ss - 1.0 + 2.0 * tt);
    g[18] = 2.0 * (ww - rr) * (1.0 - tt);
    g[19] = -2.0 * rr * (1.0 - tt);
    g[20] = -2.0 * rr * ww;
    g[21] = 2.0 * ss * (1.0 - tt);
    g[22] = 2.0 * rr * (1.0 - tt);
    g[23] = -2.0 * rr * ss;
    g[24] = -2.0 * ss * (1.0 - tt);
    g[25] = 2.0 * (ww - ss) * (1.0 - tt);
    g[26] = -2.0 * ww * ss;
    g[27] = -(1.0 - tt * tt);
    g[28] = -(1.0 - tt * tt);
    g[29] = -2.0 * tt * ww;
    g[30] = (1.0 - tt * tt);
    g[31] = 0.0;
    g[32] = -2.0 * tt * rr;
    g[33] = 0.0;
    g[34] = (1.0 - tt * tt);
    g[35] = -2.0 * tt * ss;
    g[36] = 2.0 * (ww - rr) * (1.0 + tt);
    g[37] = -2.0 * rr * (1.0 + tt);
    g[38] = 2.0 * rr * ww;
    g[39] = 2.0 * ss * (1.0 + tt);
    g[40] = 2.0 * rr * (1.0 + tt);
    g[41] = 2.0 * rr * ss;
    g[42] = -2.0 * ss * (1.0 + tt);
    g[43] = 2.0 * (ww - ss) * (1.0 + tt);
    g[44] = 2.0 * ww * ss;
}
