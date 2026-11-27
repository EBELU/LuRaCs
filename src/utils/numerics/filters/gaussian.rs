use super::edge::EdgeMode;
use super::simple::correlate1d;

pub fn gaussian_kernel(window_size: usize, sigma: f64) -> Option<Vec<f64>> {
    if window_size == 0 || window_size.is_multiple_of(2) || sigma <= 0.0 || !sigma.is_finite() {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_gaussian_kernel_and_filter() {
        assert!(gaussian_kernel(0, 1.0).is_none());
        assert!(gaussian_kernel(4, 1.0).is_none());
        assert!(gaussian_kernel(3, 0.0).is_none());
        assert!(gaussian_kernel(3, f64::NAN).is_none());

        let k = gaussian_kernel(5, 1.0).unwrap();
        assert_eq!(k.len(), 5);
        assert!((k.iter().sum::<f64>() - 1.0).abs() < 1e-12);
        assert!((k[0] - k[4]).abs() < 1e-12);

        assert!(gaussian_filter_1d(&[1.0, 2.0, 3.0], 4, 1.0, EdgeMode::Zeros).is_empty());
        let smoothed = gaussian_filter_1d(&[1.0, 1.0, 1.0, 1.0, 1.0], 3, 1.0, EdgeMode::Mirror);
        assert_eq!(smoothed.len(), 5);
        for v in smoothed {
            assert!((v - 1.0).abs() < 1e-12);
        }
    }
}
