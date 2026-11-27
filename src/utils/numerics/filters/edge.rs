#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EdgeMode {
    Mirror,
    Wrap,
    Zeros,
}

#[inline]
pub const fn map_index(index: isize, len: usize, mode: EdgeMode) -> Option<usize> {
    let n = len as isize;
    match mode {
        EdgeMode::Zeros => {
            if index < 0 || index >= n {
                None
            } else {
                Some(index as usize)
            }
        }
        EdgeMode::Wrap => Some(index.rem_euclid(n) as usize),
        EdgeMode::Mirror => {
            if len == 1 {
                return Some(0);
            }
            let period = 2 * (n - 1);
            let i = index.rem_euclid(period);
            Some((if i < n { i } else { period - i }) as usize)
        }
    }
}

pub fn full_padded_buffer(signal: &[f64], radius: usize, edge_mode: EdgeMode) -> Vec<f64> {
    let n = signal.len();
    let mut buf = Vec::with_capacity(n + 2 * radius);
    for i in -(radius as isize)..(n as isize + radius as isize) {
        buf.push(map_index(i, n, edge_mode).map(|j| signal[j]).unwrap_or(0.0));
    }
    buf
}

pub fn edge_buffers(signal: &[f64], radius: usize, edge_mode: EdgeMode) -> (Vec<f64>, Vec<f64>) {
    let n = signal.len();
    debug_assert!(n > 2 * radius);

    let mut left = Vec::with_capacity(3 * radius);
    for i in -(radius as isize)..(2 * radius) as isize {
        left.push(map_index(i, n, edge_mode).map(|j| signal[j]).unwrap_or(0.0));
    }

    let right_start = n - 2 * radius;
    let mut right = Vec::with_capacity(3 * radius);
    for i in (right_start as isize)..(n as isize + radius as isize) {
        right.push(map_index(i, n, edge_mode).map(|j| signal[j]).unwrap_or(0.0));
    }

    (left, right)
}

#[inline]
pub fn for_each_window<F>(signal: &[f64], window_size: usize, edge_mode: EdgeMode, mut f: F)
where
    F: FnMut(&[f64]),
{
    let n = signal.len();
    let radius = window_size / 2;

    if n <= 2 * radius {
        let buf = full_padded_buffer(signal, radius, edge_mode);
        for w in buf.windows(window_size).take(n) {
            f(w);
        }
        return;
    }

    let (left, right) = edge_buffers(signal, radius, edge_mode);
    for w in left.windows(window_size).take(radius) {
        f(w);
    }
    for w in signal.windows(window_size) {
        f(w);
    }
    for w in right.windows(window_size).take(radius) {
        f(w);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_edge_modes_and_for_each_window() {
        assert_eq!(map_index(-1, 4, EdgeMode::Zeros), None);
        assert_eq!(map_index(4, 4, EdgeMode::Zeros), None);
        assert_eq!(map_index(2, 4, EdgeMode::Zeros), Some(2));
        assert_eq!(map_index(-1, 4, EdgeMode::Wrap), Some(3));
        assert_eq!(map_index(4, 4, EdgeMode::Wrap), Some(0));
        assert_eq!(map_index(-1, 4, EdgeMode::Mirror), Some(1));
        assert_eq!(map_index(4, 4, EdgeMode::Mirror), Some(2));
        assert_eq!(map_index(5, 1, EdgeMode::Mirror), Some(0));

        // Short signal path (n <= 2 * radius)
        let mut windows_short = Vec::new();
        for_each_window(&[10.0, 20.0], 3, EdgeMode::Zeros, |w| {
            windows_short.push(w.to_vec());
        });
        assert_eq!(windows_short, vec![vec![0.0, 10.0, 20.0], vec![10.0, 20.0, 0.0]]);

        // Normal signal path (n > 2 * radius)
        let mut windows_long = Vec::new();
        for_each_window(&[1.0, 2.0, 3.0, 4.0], 3, EdgeMode::Mirror, |w| {
            windows_long.push(w.to_vec());
        });
        assert_eq!(windows_long.len(), 4);
    }
}
