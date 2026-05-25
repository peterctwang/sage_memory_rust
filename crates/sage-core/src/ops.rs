//! CPU kernels reusable by future GFM message-passing.
//!
//! SPEC §B.3 marked `scatter_add` as a candle-blocker for M3. The pure-CPU
//! version lives here standalone so it can be tested in isolation and reused
//! once a tensor backend lands.

use crate::error::{Result, SageError};

/// `dst[indices[i]] += src[i]` for all `i`. Bounds-checked.
pub fn scatter_add_1d(dst: &mut [f32], indices: &[usize], src: &[f32]) -> Result<()> {
    if indices.len() != src.len() {
        return Err(SageError::Invalid(format!(
            "scatter_add_1d: indices.len()={} != src.len()={}",
            indices.len(),
            src.len()
        )));
    }
    for (idx, val) in indices.iter().zip(src.iter()) {
        let i = *idx;
        if i >= dst.len() {
            return Err(SageError::Invalid(format!(
                "scatter_add_1d: index {i} out of bounds for dst.len()={}",
                dst.len()
            )));
        }
        dst[i] += *val;
    }
    Ok(())
}

/// `dst[indices[i], :] += src[i, :]` for all `i`. Row-major. `cols` is the
/// stride of both buffers. `dst.len()` must be a multiple of `cols`, and
/// `src.len()` must equal `indices.len() * cols`.
pub fn scatter_add_rows(
    dst: &mut [f32],
    cols: usize,
    indices: &[usize],
    src: &[f32],
) -> Result<()> {
    if cols == 0 {
        return Err(SageError::Invalid(
            "scatter_add_rows: cols must be > 0".into(),
        ));
    }
    if dst.len() % cols != 0 {
        return Err(SageError::Invalid(format!(
            "scatter_add_rows: dst.len()={} not divisible by cols={cols}",
            dst.len()
        )));
    }
    if src.len() != indices.len() * cols {
        return Err(SageError::Invalid(format!(
            "scatter_add_rows: src.len()={} != indices.len()*cols={}",
            src.len(),
            indices.len() * cols
        )));
    }
    let rows = dst.len() / cols;
    for (i, &row) in indices.iter().enumerate() {
        if row >= rows {
            return Err(SageError::Invalid(format!(
                "scatter_add_rows: row index {row} out of bounds for rows={rows}"
            )));
        }
        let d_off = row * cols;
        let s_off = i * cols;
        for c in 0..cols {
            dst[d_off + c] += src[s_off + c];
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scatter_1d_basic() {
        let mut dst = vec![0.0f32; 4];
        scatter_add_1d(&mut dst, &[0, 2, 2], &[1.0, 2.0, 3.0]).unwrap();
        assert_eq!(dst, vec![1.0, 0.0, 5.0, 0.0]);
    }

    #[test]
    fn scatter_1d_empty_is_noop() {
        let mut dst = vec![0.0f32; 4];
        scatter_add_1d(&mut dst, &[], &[]).unwrap();
        assert_eq!(dst, vec![0.0; 4]);
    }

    #[test]
    fn scatter_1d_length_mismatch_errors() {
        let mut dst = vec![0.0f32; 4];
        let r = scatter_add_1d(&mut dst, &[0, 1], &[1.0]);
        assert!(matches!(r, Err(SageError::Invalid(_))));
    }

    #[test]
    fn scatter_1d_out_of_bounds_errors() {
        let mut dst = vec![0.0f32; 2];
        let r = scatter_add_1d(&mut dst, &[5], &[1.0]);
        assert!(matches!(r, Err(SageError::Invalid(_))));
    }

    #[test]
    fn scatter_rows_basic() {
        // 3 rows × 2 cols
        let mut dst = vec![0.0f32; 6];
        let src = vec![1.0, 2.0, 10.0, 20.0, 100.0, 200.0]; // 3 source rows
                                                            // scatter into rows 0, 2, 2
        scatter_add_rows(&mut dst, 2, &[0, 2, 2], &src).unwrap();
        assert_eq!(dst, vec![1.0, 2.0, 0.0, 0.0, 110.0, 220.0]);
    }

    #[test]
    fn scatter_rows_empty_is_noop() {
        let mut dst = vec![0.0f32; 4];
        scatter_add_rows(&mut dst, 2, &[], &[]).unwrap();
        assert_eq!(dst, vec![0.0; 4]);
    }

    #[test]
    fn scatter_rows_cols_zero_errors() {
        let mut dst = vec![0.0f32; 4];
        assert!(scatter_add_rows(&mut dst, 0, &[], &[]).is_err());
    }

    #[test]
    fn scatter_rows_misaligned_dst_errors() {
        let mut dst = vec![0.0f32; 5];
        let r = scatter_add_rows(&mut dst, 2, &[0], &[1.0, 1.0]);
        assert!(matches!(r, Err(SageError::Invalid(_))));
    }

    #[test]
    fn scatter_rows_src_size_mismatch_errors() {
        let mut dst = vec![0.0f32; 4];
        let r = scatter_add_rows(&mut dst, 2, &[0, 1], &[1.0]);
        assert!(matches!(r, Err(SageError::Invalid(_))));
    }

    #[test]
    fn scatter_rows_row_out_of_bounds_errors() {
        let mut dst = vec![0.0f32; 4]; // 2 rows × 2 cols
        let r = scatter_add_rows(&mut dst, 2, &[5], &[1.0, 1.0]);
        assert!(matches!(r, Err(SageError::Invalid(_))));
    }
}
