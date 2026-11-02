//! RFC-0052 §4.5/4.9: compatibility identity and successful-frame transactions.
//!
//! A reset plan describes required device work; this host oracle cannot clear GPU
//! resources. A caller must execute that plan before committing a successful frame.
//! Assisted-by: Codex:gpt-6-astra.

use super::depth::{DepthProjector, Perspective, RigidTransform};
use super::{QualityError, finite_matrix};
#[cfg(test)]
use crate::temporal::common::Mat4;

pub type Digest = [u8; 32];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ExposureMode {
    Manual,
    Automatic,
}

/// A fixed histogram represents log2 scene-linear luminance, not exposed values.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AutomaticExposureSettings {
    pub initial_ev_offset: f32,
    pub min_ev_offset: f32,
    pub max_ev_offset: f32,
    pub min_log2_luminance: f32,
    pub max_log2_luminance: f32,
    pub histogram_bins: usize,
    pub target_luminance: f32,
    pub brighten_ev_per_second: f32,
    pub darken_ev_per_second: f32,
    pub fixed_dt_seconds: f32,
}

/// Immutable, validated policy; every field participates in HistoryKey equality.
#[derive(Debug, Clone, PartialEq)]
pub struct ExposurePolicy {
    mode: ExposureMode,
    settings: AutomaticExposureSettings,
}

impl ExposurePolicy {
    /// Positive EV offset brightens (2^offset), unlike photographic EV100.
    pub fn manual(ev_offset: f32) -> Result<Self, QualityError> {
        validate_ev(ev_offset)?;
        Ok(Self {
            mode: ExposureMode::Manual,
            settings: AutomaticExposureSettings {
                initial_ev_offset: ev_offset,
                min_ev_offset: ev_offset,
                max_ev_offset: ev_offset,
                min_log2_luminance: 0.0,
                max_log2_luminance: 0.0,
                histogram_bins: 0,
                target_luminance: 1.0,
                brighten_ev_per_second: 0.0,
                darken_ev_per_second: 0.0,
                fixed_dt_seconds: 0.0,
            },
        })
    }

    pub fn automatic(settings: AutomaticExposureSettings) -> Result<Self, QualityError> {
        for ev in [
            settings.initial_ev_offset,
            settings.min_ev_offset,
            settings.max_ev_offset,
        ] {
            validate_ev(ev)?;
        }
        if ![
            settings.min_log2_luminance,
            settings.max_log2_luminance,
            settings.target_luminance,
            settings.brighten_ev_per_second,
            settings.darken_ev_per_second,
            settings.fixed_dt_seconds,
        ]
        .iter()
        .all(|v| v.is_finite())
        {
            return Err(QualityError::NonFinite("exposure policy"));
        }
        if settings.histogram_bins == 0
            || settings.min_log2_luminance >= settings.max_log2_luminance
            || settings.target_luminance <= 0.0
            || settings.brighten_ev_per_second <= 0.0
            || settings.darken_ev_per_second <= 0.0
            || settings.fixed_dt_seconds <= 0.0
            || settings.min_ev_offset > settings.max_ev_offset
            || !(settings.min_ev_offset..=settings.max_ev_offset)
                .contains(&settings.initial_ev_offset)
        {
            return Err(QualityError::InvalidExposure);
        }
        Ok(Self {
            mode: ExposureMode::Automatic,
            settings,
        })
    }

    pub fn initial_ev_offset(&self) -> f32 {
        self.settings.initial_ev_offset
    }
    pub fn histogram_bins(&self) -> usize {
        self.settings.histogram_bins
    }

    fn initial_state(&self) -> ExposureState {
        ExposureState::reset(
            self.settings.initial_ev_offset,
            self.settings.histogram_bins,
        )
    }

    fn advance(
        &self,
        start: &ExposureState,
        histogram: &[u64],
    ) -> Result<ExposureState, QualityError> {
        if self.mode == ExposureMode::Manual {
            if !histogram.is_empty() {
                return Err(QualityError::InvalidExposure);
            }
            return Ok(self.initial_state());
        }
        let s = self.settings;
        if histogram.len() != s.histogram_bins {
            return Err(QualityError::InvalidExposure);
        }
        let count = histogram
            .iter()
            .try_fold(0u64, |sum, &n| sum.checked_add(n))
            .ok_or(QualityError::InvalidExposure)?;
        if count == 0 {
            return Err(QualityError::InvalidExposure);
        }
        // Fixed increasing-bin reduction, f64 host oracle. Device parity remains untested.
        let width =
            (s.max_log2_luminance as f64 - s.min_log2_luminance as f64) / s.histogram_bins as f64;
        let mean_log2 = histogram
            .iter()
            .enumerate()
            .map(|(i, &n)| (s.min_log2_luminance as f64 + (i as f64 + 0.5) * width) * n as f64)
            .sum::<f64>()
            / count as f64;
        let target = ((s.target_luminance as f64).log2() - mean_log2)
            .clamp(s.min_ev_offset as f64, s.max_ev_offset as f64);
        let current = start.current_ev as f64;
        let rate = if target >= current {
            s.brighten_ev_per_second
        } else {
            s.darken_ev_per_second
        } as f64;
        let step = rate * s.fixed_dt_seconds as f64;
        let next = current + (target - current).clamp(-step, step);
        let out = ExposureState {
            current_ev: next as f32,
            target_ev: target as f32,
            histogram: histogram.to_vec(),
            sample_count: count,
            statistics_valid: true,
        };
        out.validate(s.histogram_bins)?;
        Ok(out)
    }
}

/// History compatibility only. Continuously changing samples belong in DynamicFrame.
#[derive(Debug, Clone, PartialEq)]
pub struct HistoryKey {
    pub profile_formula_digest: Digest,
    pub scene_contract_digest: Digest,
    pub asset_digest: Digest,
    pub camera_id: u64,
    pub camera_cut_epoch: u64,
    pub projection: Perspective,
    pub jitter_sequence_id: u64,
    pub input_extent: [u32; 2],
    pub output_extent: [u32; 2],
    pub tier_percent: u32,
    pub material_generation: u64,
    pub texture_generation: u64,
    pub lighting_generation: u64,
    pub blas_geometry_generation: u64,
    pub tlas_identity_generation: u64,
    /// Includes TSR formula, MV sign/units and depth-domain convention.
    pub temporal_convention_digest: Digest,
    pub exposure_policy_generation: u64,
    pub exposure_policy: ExposurePolicy,
    pub view_transform_digest: Digest,
    pub lut_digest: Digest,
    pub cloud_weather_digest: Digest,
    pub cloud_noise_digest: Digest,
    pub cloud_parameter_generation: u64,
    pub water_domain_digest: Digest,
    pub water_obstacle_generation: u64,
    pub water_simulation_epoch: u64,
    pub rain_configuration_digest: Digest,
    pub particle_simulation_epoch: u64,
}

impl HistoryKey {
    pub fn validate(&self) -> Result<(), QualityError> {
        if self.input_extent.contains(&0)
            || self.output_extent.contains(&0)
            || (0..2).any(|i| self.input_extent[i] > self.output_extent[i])
            || ![50, 67, 100].contains(&self.tier_percent)
        {
            return Err(QualityError::InvalidExtent);
        }
        Ok(())
    }
}

/// Frame samples are deliberately absent from HistoryKey.
#[derive(Debug, Clone)]
pub struct DynamicFrame {
    pub frame_index: u64,
    pub camera: DepthProjector,
    pub jitter_pixels: [f32; 2],
    pub object_transforms: Vec<RigidTransform>,
    pub tlas_refit_serial: u64,
    pub cloud_wind_offset: [f32; 2],
    pub water_step: u64,
    pub particle_step: u64,
}

impl DynamicFrame {
    fn validate(&self) -> Result<(), QualityError> {
        finite_matrix(&self.camera.jittered_vp(), "history view projection")?;
        for transform in &self.object_transforms {
            finite_matrix(&transform.matrix(), "history object transform")?;
        }
        if !self
            .jitter_pixels
            .iter()
            .chain(self.cloud_wind_offset.iter())
            .all(|v| v.is_finite())
        {
            return Err(QualityError::NonFinite("dynamic sample"));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FrameEvent {
    Continuous,
    CameraCut,
    MissingPrevious,
    Rollback,
    SimulationRestart,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResetReason {
    FirstFrame,
    CompatibilityChanged,
    CameraCut,
    MissingPrevious,
    Rollback,
    SimulationRestart,
    FailedFrame,
    ObjectLayoutChanged,
}

/// ALL means both ping/pong sides, not merely the next write target.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ResetPlan {
    pub color_depth_luma_sign_flicker: bool,
    pub history_validity: bool,
    pub previous_motion: bool,
    pub merge_winner: bool,
    pub exposure_histogram_and_adaptation: bool,
    pub render_parities: bool,
}

impl ResetPlan {
    pub const ALL: Self = Self {
        color_depth_luma_sign_flicker: true,
        history_validity: true,
        previous_motion: true,
        merge_winner: true,
        exposure_histogram_and_adaptation: true,
        render_parities: true,
    };
    pub const NONE: Self = Self {
        color_depth_luma_sign_flicker: false,
        history_validity: false,
        previous_motion: false,
        merge_winner: false,
        exposure_histogram_and_adaptation: false,
        render_parities: false,
    };
}

#[derive(Debug, Clone, PartialEq)]
pub struct ExposureState {
    /// EV used to expose the next frame, independent of TSR's fixed exposure=1.
    pub current_ev: f32,
    pub target_ev: f32,
    pub histogram: Vec<u64>,
    pub sample_count: u64,
    pub statistics_valid: bool,
}

fn validate_ev(ev: f32) -> Result<(), QualityError> {
    if !ev.is_finite() {
        return Err(QualityError::NonFinite("exposure EV"));
    }
    let multiplier = ev.exp2();
    if !multiplier.is_finite() || multiplier <= 0.0 {
        return Err(QualityError::InvalidExposure);
    }
    Ok(())
}

impl ExposureState {
    fn reset(initial_ev: f32, histogram_bins: usize) -> Self {
        Self {
            current_ev: initial_ev,
            target_ev: initial_ev,
            histogram: vec![0; histogram_bins],
            sample_count: 0,
            statistics_valid: false,
        }
    }

    fn validate(&self, expected_bins: usize) -> Result<(), QualityError> {
        validate_ev(self.current_ev)?;
        validate_ev(self.target_ev)?;
        let count = self
            .histogram
            .iter()
            .try_fold(0u64, |sum, &n| sum.checked_add(n));
        if self.histogram.len() != expected_bins
            || count != Some(self.sample_count)
            || (self.statistics_valid && self.sample_count == 0)
            || (!self.statistics_valid && self.sample_count != 0)
        {
            return Err(QualityError::InvalidExposure);
        }
        Ok(())
    }
}

#[derive(Debug, Clone)]
struct CommittedFrame {
    key: HistoryKey,
    dynamic: DynamicFrame,
    exposure: ExposureState,
    parity: u8,
}

#[derive(Debug, Clone)]
pub struct FrameDecision {
    pub reasons: Vec<ResetReason>,
    pub reset: ResetPlan,
    /// Next write target. Simulation ring state is owned separately by its epoch.
    pub render_parity: u8,
    pub exposure_at_start: ExposureState,
}

/// Fields are private: values can only be derived by the current frame transaction.
pub struct ExposureCandidate {
    next: ExposureState,
    key: HistoryKey,
    frame_index: u64,
    start: ExposureState,
    serial: u64,
}

impl ExposureCandidate {
    pub fn next_state(&self) -> &ExposureState {
        &self.next
    }
}

/// A mutable borrow prevents a second frame from starting before commit/abort.
#[derive(Debug)]
pub struct HistoryState {
    committed: Option<CommittedFrame>,
    failed_since_commit: bool,
    transaction_serial: u64,
}

impl HistoryState {
    pub fn new() -> Self {
        Self {
            committed: None,
            failed_since_commit: false,
            transaction_serial: 0,
        }
    }

    pub fn committed_frame_index(&self) -> Option<u64> {
        self.committed.as_ref().map(|v| v.dynamic.frame_index)
    }

    pub fn history_available(&self) -> bool {
        self.committed.is_some() && !self.failed_since_commit
    }

    /// Invalid input invalidates reuse just like an aborted frame; it never commits state.
    pub fn begin_frame(
        &mut self,
        key: HistoryKey,
        dynamic: DynamicFrame,
        event: FrameEvent,
    ) -> Result<FrameTransaction<'_>, QualityError> {
        if let Err(error) = key.validate().and_then(|_| dynamic.validate()) {
            self.failed_since_commit = true;
            return Err(error);
        }
        if dynamic.camera.projection() != key.projection
            || dynamic.camera.extent() != key.input_extent
            || dynamic.camera.jitter_pixels() != dynamic.jitter_pixels
        {
            self.failed_since_commit = true;
            return Err(QualityError::InvalidProjection);
        }
        self.transaction_serial = self
            .transaction_serial
            .checked_add(1)
            .ok_or(QualityError::InvalidExposure)?;
        let mut reasons = Vec::new();
        let mut add = |reason| {
            if !reasons.contains(&reason) {
                reasons.push(reason);
            }
        };
        if self.failed_since_commit {
            add(ResetReason::FailedFrame);
        }
        match &self.committed {
            None => add(ResetReason::FirstFrame),
            Some(last) => {
                if last.key != key {
                    add(ResetReason::CompatibilityChanged);
                }
                if last.dynamic.object_transforms.len() != dynamic.object_transforms.len() {
                    add(ResetReason::ObjectLayoutChanged);
                }
                if dynamic.frame_index <= last.dynamic.frame_index
                    || dynamic.water_step < last.dynamic.water_step
                    || dynamic.particle_step < last.dynamic.particle_step
                    || dynamic.tlas_refit_serial < last.dynamic.tlas_refit_serial
                {
                    add(ResetReason::Rollback);
                } else if last.dynamic.frame_index.checked_add(1) != Some(dynamic.frame_index) {
                    add(ResetReason::MissingPrevious);
                }
            }
        }
        match event {
            FrameEvent::Continuous => {}
            FrameEvent::CameraCut => add(ResetReason::CameraCut),
            FrameEvent::MissingPrevious => add(ResetReason::MissingPrevious),
            FrameEvent::Rollback => add(ResetReason::Rollback),
            FrameEvent::SimulationRestart => add(ResetReason::SimulationRestart),
        }
        let reset = !reasons.is_empty();
        let (render_parity, exposure_at_start) = if reset {
            (0, key.exposure_policy.initial_state())
        } else {
            let last = self
                .committed
                .as_ref()
                .expect("no reset implies committed history");
            (last.parity ^ 1, last.exposure.clone())
        };
        let decision = FrameDecision {
            reasons,
            reset: if reset {
                ResetPlan::ALL
            } else {
                ResetPlan::NONE
            },
            render_parity,
            exposure_at_start,
        };
        Ok(FrameTransaction {
            owner: self,
            key,
            dynamic,
            decision,
            completed: false,
        })
    }
}

pub struct FrameTransaction<'a> {
    owner: &'a mut HistoryState,
    key: HistoryKey,
    dynamic: DynamicFrame,
    decision: FrameDecision,
    completed: bool,
}

impl FrameTransaction<'_> {
    pub fn decision(&self) -> &FrameDecision {
        &self.decision
    }

    pub fn previous_dynamic(&self) -> Option<&DynamicFrame> {
        if self.decision.reset != ResetPlan::NONE {
            return None;
        }
        self.owner.committed.as_ref().map(|v| &v.dynamic)
    }

    /// Computes next-frame adaptation using the immutable policy's contract dt.
    /// No caller-provided EV/target/dt can enter a successful commit.
    pub fn advance_exposure(&self, histogram: &[u64]) -> Result<ExposureCandidate, QualityError> {
        Ok(ExposureCandidate {
            next: self
                .key
                .exposure_policy
                .advance(&self.decision.exposure_at_start, histogram)?,
            key: self.key.clone(),
            frame_index: self.dynamic.frame_index,
            start: self.decision.exposure_at_start.clone(),
            serial: self.owner.transaction_serial,
        })
    }

    /// Call only after the entire frame and all required resets/dependencies succeeded.
    /// A public ExposureState cannot be passed here; only a sealed derived candidate.
    pub fn commit_success(mut self, candidate: ExposureCandidate) -> Result<(), QualityError> {
        if candidate.key != self.key
            || candidate.frame_index != self.dynamic.frame_index
            || candidate.start != self.decision.exposure_at_start
            || candidate.serial != self.owner.transaction_serial
        {
            return Err(QualityError::InvalidExposure);
        }
        self.owner.committed = Some(CommittedFrame {
            key: self.key.clone(),
            dynamic: self.dynamic.clone(),
            exposure: candidate.next,
            parity: self.decision.render_parity,
        });
        self.owner.failed_since_commit = false;
        self.completed = true;
        Ok(())
    }

    /// Failure preserves the last completed identity for diagnostics but invalidates reuse.
    pub fn abort(self) {
        drop(self);
    }
}

impl Drop for FrameTransaction<'_> {
    fn drop(&mut self) {
        if !self.completed {
            self.owner.failed_since_commit = true;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn settings() -> AutomaticExposureSettings {
        AutomaticExposureSettings {
            initial_ev_offset: -2.0,
            min_ev_offset: -10.0,
            max_ev_offset: 10.0,
            min_log2_luminance: -2.0,
            max_log2_luminance: 2.0,
            histogram_bins: 2,
            target_luminance: 1.0,
            brighten_ev_per_second: 1.0,
            darken_ev_per_second: 2.0,
            fixed_dt_seconds: 0.25,
        }
    }
    fn key() -> HistoryKey {
        HistoryKey {
            profile_formula_digest: [1; 32],
            scene_contract_digest: [2; 32],
            asset_digest: [3; 32],
            camera_id: 0,
            camera_cut_epoch: 0,
            projection: Perspective::new(1.0, 1.0, 0.1, 100.0).unwrap(),
            jitter_sequence_id: 1,
            input_extent: [64; 2],
            output_extent: [64; 2],
            tier_percent: 100,
            material_generation: 0,
            texture_generation: 0,
            lighting_generation: 0,
            blas_geometry_generation: 0,
            tlas_identity_generation: 0,
            temporal_convention_digest: [4; 32],
            exposure_policy_generation: 0,
            exposure_policy: ExposurePolicy::automatic(settings()).unwrap(),
            view_transform_digest: [5; 32],
            lut_digest: [6; 32],
            cloud_weather_digest: [7; 32],
            cloud_noise_digest: [8; 32],
            cloud_parameter_generation: 0,
            water_domain_digest: [9; 32],
            water_obstacle_generation: 0,
            water_simulation_epoch: 0,
            rain_configuration_digest: [10; 32],
            particle_simulation_epoch: 0,
        }
    }
    fn frame_for(index: u64, k: &HistoryKey) -> DynamicFrame {
        DynamicFrame {
            frame_index: index,
            camera: DepthProjector::new(Mat4::identity(), k.projection, [0.0; 2], k.input_extent)
                .unwrap(),
            jitter_pixels: [0.0; 2],
            object_transforms: vec![RigidTransform::identity()],
            tlas_refit_serial: index,
            cloud_wind_offset: [index as f32, 0.0],
            water_step: index,
            particle_step: index,
        }
    }
    fn frame(index: u64) -> DynamicFrame {
        frame_for(index, &key())
    }
    fn commit(tx: FrameTransaction<'_>) {
        let candidate = tx.advance_exposure(&[2, 3]).unwrap();
        tx.commit_success(candidate).unwrap();
    }
    fn started() -> HistoryState {
        let mut state = HistoryState::new();
        commit(
            state
                .begin_frame(key(), frame(0), FrameEvent::Continuous)
                .unwrap(),
        );
        state
    }

    #[test]
    fn continuous_pose_jitter_refit_wave_particle_steps_preserve_history() {
        let mut state = started();
        for index in 1..5 {
            let mut dynamic = frame(index);
            let mut camera = Mat4::identity();
            camera.m[0][3] = index as f32 * 0.1;
            dynamic.jitter_pixels = [0.25, -0.25];
            dynamic.camera =
                DepthProjector::new(camera, key().projection, dynamic.jitter_pixels, [64; 2])
                    .unwrap();
            let mut object = Mat4::identity();
            object.m[1][3] = index as f32;
            dynamic.object_transforms[0] = RigidTransform::new(object).unwrap();
            let tx = state
                .begin_frame(key(), dynamic, FrameEvent::Continuous)
                .unwrap();
            assert_eq!(tx.decision().reset, ResetPlan::NONE);
            assert_eq!(tx.decision().render_parity, (index % 2) as u8);
            assert!(tx.decision().exposure_at_start.statistics_valid);
            assert_eq!(tx.previous_dynamic().unwrap().frame_index, index - 1);
            commit(tx);
        }
    }

    #[test]
    fn compatibility_changes_reset_all_including_exposure() {
        let mut variants = Vec::new();
        macro_rules! change {
            ($field:ident, $value:expr) => {{
                let mut k = key();
                k.$field = $value;
                variants.push(k);
            }};
        }
        change!(profile_formula_digest, [11; 32]);
        change!(scene_contract_digest, [11; 32]);
        change!(asset_digest, [11; 32]);
        change!(camera_id, 1);
        change!(camera_cut_epoch, 1);
        change!(projection, Perspective::new(1.1, 1.0, 0.1, 100.0).unwrap());
        change!(jitter_sequence_id, 2);
        change!(input_extent, [32; 2]);
        change!(output_extent, [128; 2]);
        change!(tier_percent, 67);
        change!(material_generation, 1);
        change!(texture_generation, 1);
        change!(lighting_generation, 1);
        change!(blas_geometry_generation, 1);
        change!(tlas_identity_generation, 1);
        change!(temporal_convention_digest, [11; 32]);
        change!(exposure_policy_generation, 1);
        change!(exposure_policy, ExposurePolicy::manual(-1.0).unwrap());
        change!(view_transform_digest, [11; 32]);
        change!(lut_digest, [11; 32]);
        change!(cloud_weather_digest, [11; 32]);
        change!(cloud_noise_digest, [11; 32]);
        change!(cloud_parameter_generation, 1);
        change!(water_domain_digest, [11; 32]);
        change!(water_obstacle_generation, 1);
        change!(water_simulation_epoch, 1);
        change!(rain_configuration_digest, [11; 32]);
        change!(particle_simulation_epoch, 1);
        for k in variants {
            let mut state = started();
            let initial = k.exposure_policy.initial_state();
            let dynamic = frame_for(1, &k);
            let tx = state
                .begin_frame(k, dynamic, FrameEvent::Continuous)
                .unwrap();
            assert!(
                tx.decision()
                    .reasons
                    .contains(&ResetReason::CompatibilityChanged)
            );
            assert_eq!(tx.decision().reset, ResetPlan::ALL);
            assert_eq!(tx.decision().render_parity, 0);
            assert_eq!(tx.decision().exposure_at_start, initial);
            assert!(tx.previous_dynamic().is_none());
        }
    }

    #[test]
    fn cut_equals_clean_start_and_next_exposure_is_not_applied_to_current_frame() {
        let mut continuing = started();
        let mut clean = HistoryState::new();
        let cut = continuing
            .begin_frame(key(), frame(1), FrameEvent::CameraCut)
            .unwrap();
        let first = clean
            .begin_frame(key(), frame(1), FrameEvent::Continuous)
            .unwrap();
        assert_eq!(cut.decision().reset, first.decision().reset);
        assert_eq!(
            cut.decision().exposure_at_start,
            first.decision().exposure_at_start
        );
        let candidate = cut.advance_exposure(&[4, 0]).unwrap();
        // Bin 0 center is -1 => target EV=+1. Brighten at 1EV/s for dt .25 => -1.75.
        assert_eq!(candidate.next_state().target_ev, 1.0);
        assert_eq!(candidate.next_state().current_ev, -1.75);
        assert_eq!(cut.decision().exposure_at_start.current_ev, -2.0);
        assert!(cut.previous_dynamic().is_none());
    }

    #[test]
    fn failed_or_dropped_transaction_never_advances_committed_state() {
        let mut state = started();
        state
            .begin_frame(key(), frame(1), FrameEvent::Continuous)
            .unwrap()
            .abort();
        assert_eq!(state.committed_frame_index(), Some(0));
        assert!(!state.history_available());
        let tx = state
            .begin_frame(key(), frame(1), FrameEvent::Continuous)
            .unwrap();
        assert!(tx.decision().reasons.contains(&ResetReason::FailedFrame));
        assert_eq!(tx.decision().exposure_at_start.current_ev, -2.0);
        commit(tx);
        assert!(state.history_available());
        drop(
            state
                .begin_frame(key(), frame(2), FrameEvent::Continuous)
                .unwrap(),
        );
        assert!(!state.history_available());
        assert_eq!(state.committed_frame_index(), Some(1));
    }

    #[test]
    fn rollback_missing_frame_object_layout_and_serial_rewind_reset() {
        let mut state = started();
        commit(
            state
                .begin_frame(key(), frame(1), FrameEvent::Continuous)
                .unwrap(),
        );
        for kind in 0..5 {
            let mut dynamic = frame(2);
            let expected = match kind {
                0 => {
                    dynamic.frame_index = 1;
                    ResetReason::Rollback
                }
                1 => {
                    dynamic.frame_index = 4;
                    ResetReason::MissingPrevious
                }
                2 => {
                    dynamic.water_step = 0;
                    ResetReason::Rollback
                }
                3 => {
                    dynamic.tlas_refit_serial = 0;
                    ResetReason::Rollback
                }
                _ => {
                    dynamic.object_transforms.push(RigidTransform::identity());
                    ResetReason::ObjectLayoutChanged
                }
            };
            let tx = state
                .begin_frame(key(), dynamic, FrameEvent::Continuous)
                .unwrap();
            assert!(tx.decision().reasons.contains(&expected));
            assert_eq!(tx.decision().reset, ResetPlan::ALL);
        }
        let mut state = started();
        let mut equal_serial = frame(1);
        equal_serial.tlas_refit_serial = 0;
        let tx = state
            .begin_frame(key(), equal_serial, FrameEvent::Continuous)
            .unwrap();
        assert_eq!(tx.decision().reset, ResetPlan::NONE);
    }

    #[test]
    fn policy_validates_range_rates_target_and_dt_and_all_fields_invalidate_identity() {
        for field in 0..7 {
            let mut s = settings();
            match field {
                0 => s.min_log2_luminance = 3.0,
                1 => s.histogram_bins = 0,
                2 => s.target_luminance = 0.0,
                3 => s.fixed_dt_seconds = 0.0,
                4 => s.brighten_ev_per_second = -1.0,
                5 => s.darken_ev_per_second = f32::NAN,
                _ => s.initial_ev_offset = f32::INFINITY,
            }
            assert!(ExposurePolicy::automatic(s).is_err());
        }
        for field in 0..5 {
            let mut s = settings();
            match field {
                0 => s.histogram_bins = 4,
                1 => s.fixed_dt_seconds = 0.5,
                2 => s.brighten_ev_per_second = 3.0,
                3 => s.target_luminance = 0.5,
                _ => s.max_log2_luminance = 4.0,
            }
            let mut k = key();
            k.exposure_policy = ExposurePolicy::automatic(s).unwrap();
            let mut state = started();
            assert!(
                state
                    .begin_frame(k, frame(1), FrameEvent::Continuous)
                    .unwrap()
                    .decision()
                    .reasons
                    .contains(&ResetReason::CompatibilityChanged)
            );
        }
    }

    #[test]
    fn invalid_statistics_and_nonfinite_dynamic_cannot_commit() {
        let mut state = started();
        let tx = state
            .begin_frame(key(), frame(1), FrameEvent::Continuous)
            .unwrap();
        for histogram in [vec![], vec![1], vec![0, 0], vec![u64::MAX, 1]] {
            assert!(tx.advance_exposure(&histogram).is_err());
        }
        drop(tx);
        assert!(!state.history_available());
        assert_eq!(state.committed_frame_index(), Some(0));
        let mut dynamic = frame(1);
        dynamic.jitter_pixels[0] = f32::NAN;
        assert!(
            state
                .begin_frame(key(), dynamic, FrameEvent::Continuous)
                .is_err()
        );
        let mut k = key();
        k.input_extent[0] = 0;
        assert!(
            state
                .begin_frame(k, frame(1), FrameEvent::Continuous)
                .is_err()
        );
    }

    #[test]
    fn stale_candidate_is_rejected_and_manual_candidate_is_sealed() {
        let mut state = started();
        let tx = state
            .begin_frame(key(), frame(1), FrameEvent::Continuous)
            .unwrap();
        let stale = tx.advance_exposure(&[1, 2]).unwrap();
        drop(tx);
        let retry = state
            .begin_frame(key(), frame(1), FrameEvent::Continuous)
            .unwrap();
        assert_eq!(
            retry.commit_success(stale),
            Err(QualityError::InvalidExposure)
        );
        let mut manual = key();
        manual.exposure_policy = ExposurePolicy::manual(2.0).unwrap();
        let tx = state
            .begin_frame(manual, frame(1), FrameEvent::Continuous)
            .unwrap();
        assert!(tx.advance_exposure(&[1, 2]).is_err());
        let candidate = tx.advance_exposure(&[]).unwrap();
        assert_eq!(candidate.next_state().current_ev, 2.0);
        assert_eq!(candidate.next_state().current_ev.exp2(), 4.0);
        tx.commit_success(candidate).unwrap();
        assert!(state.history_available());
    }
}
