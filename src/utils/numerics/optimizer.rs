use nalgebra::{DMatrix, DVector};

/// Moore–Penrose pseudoinverse via SVD. `None` only if the SVD fails to
/// converge (very rare; degenerate inputs like `NaN` in `a` are the usual
/// culprits).
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
