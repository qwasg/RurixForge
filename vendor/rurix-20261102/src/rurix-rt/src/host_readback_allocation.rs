//! Private fallible host storage used by readback and the HQ capacity witness.
//! Existing Result failure semantics; no public API or C ABI change.
//! Assisted-by: Codex:gpt-6-astra.

pub(crate) fn zeroed(size: usize) -> Result<Vec<u8>, std::collections::TryReserveError> {
    let mut bytes = Vec::new();
    bytes.try_reserve_exact(size)?;
    bytes.resize(size, 0);
    Ok(bytes)
}

/// Release already collected large payloads before formatting an allocation error.
/// The caller must obtain this storage before mapping a device allocation.
pub(crate) fn next(previous: &mut Vec<Vec<u8>>, size: usize) -> Result<Vec<u8>, String> {
    match zeroed(size) {
        Ok(bytes) => Ok(bytes),
        Err(error) => {
            previous.clear();
            Err(format!("host readback allocation failed: requested {size} bytes: {error}"))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exact_zero_initialized_storage_and_no_prior_payload_change() {
        let mut previous = vec![vec![3; 17], vec![9; 31]];
        for size in [0, 1, 64, 4097, 1 << 20] {
            let bytes = next(&mut previous, size).unwrap();
            assert_eq!(bytes.len(), size);
            assert!(bytes.iter().all(|&v| v == 0));
            assert_eq!(previous, [vec![3; 17], vec![9; 31]]);
        }
    }

    #[test]
    fn capacity_overflow_releases_partial_results_before_error() {
        let mut previous = vec![vec![1; 1 << 20], vec![2; 2 << 20]];
        let error = next(&mut previous, usize::MAX).unwrap_err();
        assert!(previous.is_empty());
        assert!(error.contains("host readback allocation failed"));
        assert!(error.contains(&usize::MAX.to_string()));
    }
}
