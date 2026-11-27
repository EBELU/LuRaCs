use ndarray::Array2;
use rayon::prelude::*;

pub const FWHM_C: f64 = 2.354_820_045_030_949_3;

pub const SPARSE_CUTOFF: f64 = 1e-12;
pub const PARALLEL_ROW_THRESHOLD: usize = 512;

// =============================================================================
// CSR sparse matrix
// =============================================================================

#[derive(Clone, Debug)]
pub struct CsrMatrix {
    pub offsets: Vec<usize>,
    pub indices: Vec<usize>,
    pub values: Vec<f64>,
    nrows: usize,
    ncols: usize,
}

impl CsrMatrix {
    /// Construct a CSR matrix from raw parts. Panics on invalid input.
    pub fn from_parts(
        offsets: Vec<usize>,
        indices: Vec<usize>,
        values: Vec<f64>,
        ncols: usize,
    ) -> Self {
        assert!(
            offsets.len() >= 2,
            "CSR offsets must contain at least two entries"
        );

        let nrows = offsets.len() - 1;

        Self::new(offsets, indices, values, nrows, ncols).expect("invalid CSR matrix")
    }

    /// Construct a CSR matrix with validation.
    pub fn new(
        offsets: Vec<usize>,
        indices: Vec<usize>,
        values: Vec<f64>,
        nrows: usize,
        ncols: usize,
    ) -> Result<Self, &'static str> {
        if offsets.len() != nrows + 1 {
            return Err("offsets length must be nrows + 1");
        }

        if indices.len() != values.len() {
            return Err("indices and values must have the same length");
        }

        if offsets.first() != Some(&0) || offsets.last() != Some(&values.len()) {
            return Err("offsets must start at 0 and end at nnz");
        }

        if offsets.windows(2).any(|w| w[0] > w[1]) {
            return Err("offsets must be non-decreasing");
        }

        if indices.iter().any(|&col| col >= ncols) {
            return Err("column index out of bounds");
        }

        Ok(Self {
            offsets,
            indices,
            values,
            nrows,
            ncols,
        })
    }

    #[inline]
    pub const fn nrows(&self) -> usize {
        self.nrows
    }

    #[inline]
    pub const fn ncols(&self) -> usize {
        self.ncols
    }

    #[inline]
    pub const fn nnz(&self) -> usize {
        self.values.len()
    }

    /// Convert CSR buffers to serialization-friendly integer types.
    pub fn to_i64_i32(&self) -> (Vec<i64>, Vec<i32>, Vec<f64>) {
        (
            self.offsets.iter().map(|&v| v as i64).collect(),
            self.indices.iter().map(|&v| v as i32).collect(),
            self.values.clone(),
        )
    }

    /// Construct a CSR matrix from a dense ndarray.
    pub fn from_dense(dense: &Array2<f64>) -> Self {
        let (nrows, ncols) = dense.dim();

        let mut offsets = Vec::with_capacity(nrows + 1);
        let mut indices = Vec::new();
        let mut values = Vec::new();

        offsets.push(0);

        for row in dense.rows() {
            for (col, &value) in row.iter().enumerate() {
                if value.abs() > SPARSE_CUTOFF {
                    indices.push(col);
                    values.push(value);
                }
            }

            offsets.push(values.len());
        }

        Self {
            offsets,
            indices,
            values,
            nrows,
            ncols,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_csr_matrix_construction_validation_and_conversions() {
        let m = CsrMatrix::from_parts(vec![0, 1, 2], vec![0, 1], vec![2.0, 3.0], 2);
        assert_eq!(m.nrows(), 2);
        assert_eq!(m.ncols(), 2);
        assert_eq!(m.nnz(), 2);
        let (offs, idxs, vals) = m.to_i64_i32();
        assert_eq!(offs, vec![0, 1, 2]);
        assert_eq!(idxs, vec![0, 1]);
        assert_eq!(vals, vec![2.0, 3.0]);

        let dense = Array2::from_shape_vec((2, 2), vec![1.0, 0.0, 0.0, 4.0]).unwrap();
        let from_d = CsrMatrix::from_dense(&dense);
        assert_eq!(from_d.nnz(), 2);

        // Validation errors
        assert!(CsrMatrix::new(vec![0], vec![], vec![], 1, 1).is_err());
        assert!(CsrMatrix::new(vec![0, 1], vec![0], vec![], 1, 1).is_err());
        assert!(CsrMatrix::new(vec![1, 1], vec![], vec![], 1, 1).is_err());
        assert!(CsrMatrix::new(vec![0, 2, 1], vec![0], vec![1.0], 2, 2).is_err());
        assert!(CsrMatrix::new(vec![0, 1], vec![5], vec![1.0], 1, 2).is_err());
    }
}
