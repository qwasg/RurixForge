//! RFC-0052 internal HQ host contracts and reference oracles.
//!
//! These types do not implement a GPU lane or claim material/visual acceptance.
//! Frozen temporal mathematics remains in `temporal::common`.
//! Assisted-by: Codex:gpt-6-astra.

pub mod depth;
pub mod effects;
pub mod history;
pub mod hit_policy;
pub mod material;

/// Strict assembly/input failures; no silent value repair.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum QualityError {
    NonFinite(&'static str),
    InvalidProjection,
    InvalidExtent,
    InvalidView,
    OutsideClipVolume,
    BehindCamera,
    InvalidExposure,
}

impl std::fmt::Display for QualityError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "HQ contract violation: {self:?}")
    }
}

impl std::error::Error for QualityError {}

pub(crate) fn finite_matrix(
    matrix: &crate::temporal::common::Mat4,
    name: &'static str,
) -> Result<(), QualityError> {
    if matrix.m.iter().flatten().all(|v| v.is_finite()) {
        Ok(())
    } else {
        Err(QualityError::NonFinite(name))
    }
}
