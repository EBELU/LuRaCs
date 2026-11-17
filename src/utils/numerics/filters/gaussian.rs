use super::edge::EdgeMode;
use super::simple::correlate1d;

pub fn gaussian_kernel(window_size: usize, sigma: f64) -> Option<Vec<f64>> {
    if window_size == 0 || window_size % 2 == 0 || sigma <= 0.0 || !sigma.is_finite() {
        return None;
    }

    let radius = window_size / 2;
    let inv_sigma = 1.0 / sigma;
    let mut kernel = vec![0.0; window_size];

    // Compute half the kernel and mirror (cuts exp() calls in half & avoids sigma^2 overflow)
    kernel[radius] = 1.0;
    let mut sum = 1.0;

    for offset in 1..=radius {
        let z = (offset as f64) * inv_sigma;
        let val = (-0.5 * z * z).exp();
        kernel[radius - offset] = val;
        kernel[radius + offset] = val;
        sum = 2.0f64.mul_add(val, sum);
    }

    let inv_sum = 1.0 / sum;
    for coeff in &mut kernel {
        *coeff *= inv_sum;
    }

    Some(kernel)
}

pub fn gaussian_filter_1d(
    signal: &[f64],
    window_size: usize,
    sigma: f64,
    edge_mode: EdgeMode,
) -> Vec<f64> {
    let Some(kernel) = gaussian_kernel(window_size, sigma) else {
        return Vec::new();
    };

    // Symmetric kernel: correlate1d skips cloning and reversing `kernel`
    correlate1d(signal, &kernel, edge_mode)
}
