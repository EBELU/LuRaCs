use nalgebra::{DMatrix, DVector};
use ndarray::Array2;

/// Moore–Penrose pseudoinverse via SVD for `nalgebra::DMatrix`.
/// `None` only if the SVD fails to converge (degenerate inputs like `NaN`
/// in `a` are the usual culprits).
pub fn pseudo_inverse(a: &DMatrix<f64>) -> Option<DMatrix<f64>> {
    let svd = a.clone().svd(true, true);
    let u = svd.u?;
    let vt = svd.v_t?;
    let s = svd.singular_values;
    let tol = s.iter().cloned().fold(0.0_f64, f64::max) * 1e-10;
    let mut s_inv = DVector::zeros(s.len());
    for i in 0..s.len() {
        if s[i] > tol {
            s_inv[i] = 1.0 / s[i];
        }
    }
    Some(vt.transpose() * DMatrix::from_diagonal(&s_inv) * u.transpose())
}

/// Moore–Penrose pseudoinverse for `ndarray::Array2`.
pub fn pseudo_inverse_array(a: &Array2<f64>) -> Option<Array2<f64>> {
    let (nrows, ncols) = (a.nrows(), a.ncols());
    let flat: Vec<f64> = a.iter().copied().collect();
    let dm = DMatrix::from_row_slice(nrows, ncols, &flat);
    let inv_dm = pseudo_inverse(&dm)?;
    let mut out = Array2::zeros((ncols, nrows));
    for r in 0..ncols {
        for c in 0..nrows {
            out[(r, c)] = inv_dm[(r, c)];
        }
    }
    Some(out)
}
