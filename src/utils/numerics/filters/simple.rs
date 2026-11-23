use super::edge::{EdgeMode, edge_buffers, for_each_window, full_padded_buffer};

/// Re-anchor sliding sum periodically to eliminate floating-point drift and NaN/Inf lockup.
const DRIFT_RESET_INTERVAL: usize = 1024;

pub fn mean_filter(signal: &[f64], window_size: usize, edge_mode: EdgeMode) -> Vec<f64> {
    if signal.is_empty() || window_size == 0 || window_size.is_multiple_of(2) {
        return Vec::new();
    }

    let n = signal.len();
    let radius = window_size / 2;
    let inv_w = 1.0 / window_size as f64;
    let mut out = Vec::with_capacity(n);

    // Helper for running sum over a contiguous slice `src` producing `count` outputs
    let mut push_sliding_means = |src: &[f64], count: usize| {
        if count == 0 {
            return;
        }
        let mut sum: f64 = src[..window_size].iter().sum();
        out.push(sum * inv_w);

        for i in 1..count {
            if i % DRIFT_RESET_INTERVAL == 0 {
                sum = src[i..i + window_size].iter().sum();
            } else {
                sum += src[i + window_size - 1] - src[i - 1];
            }
            out.push(sum * inv_w);
        }
    };

    if n <= 2 * radius {
        let buf = full_padded_buffer(signal, radius, edge_mode);
        push_sliding_means(&buf, n);
        return out;
    }

    let (left, right) = edge_buffers(signal, radius, edge_mode);
    push_sliding_means(&left, radius);
    push_sliding_means(signal, n - 2 * radius);
    push_sliding_means(&right, radius);

    out
}

pub fn median_filter(signal: &[f64], window_size: usize, edge_mode: EdgeMode) -> Vec<f64> {
    if signal.is_empty() || window_size == 0 || window_size.is_multiple_of(2) {
        return Vec::new();
    }

    let radius = window_size / 2;
    let mut out = Vec::with_capacity(signal.len());
    let mut scratch = vec![0.0; window_size];

    for_each_window(signal, window_size, edge_mode, |window| {
        scratch.copy_from_slice(window);
        let (_, median, _) = scratch.select_nth_unstable_by(radius, f64::total_cmp);
        out.push(*median);
    });

    out
}

/// Cross-correlation without reversing `kernel` (used directly by symmetric kernels).
pub fn correlate1d(signal: &[f64], kernel: &[f64], edge_mode: EdgeMode) -> Vec<f64> {
    if signal.is_empty() || kernel.is_empty() || kernel.len().is_multiple_of(2) {
        return Vec::new();
    }

    let mut out = Vec::with_capacity(signal.len());
    for_each_window(signal, kernel.len(), edge_mode, |window| {
        let acc = window.iter().zip(kernel.iter()).map(|(&s, &w)| s * w).sum();
        out.push(acc);
    });
    out
}

pub fn convolve1d(signal: &[f64], kernel: &[f64], edge_mode: EdgeMode) -> Vec<f64> {
    if signal.is_empty() || kernel.is_empty() || kernel.len().is_multiple_of(2) {
        return Vec::new();
    }

    let rev_kernel: Vec<f64> = kernel.iter().rev().copied().collect();
    correlate1d(signal, &rev_kernel, edge_mode)
}
