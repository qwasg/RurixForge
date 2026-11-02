//! RFC-0052 §4.2/4.7: distinct depth domains and UV motion reference.
//! Assisted-by: Codex:gpt-6-astra.

use super::{QualityError, finite_matrix};
use crate::temporal::common::{Mat4, perspective_rh_zo};

/// Proper affine rigid transform. Static nonuniform/mirror scale must be baked into
/// asset positions/normals/TBN before this dynamic contract is constructed.
#[derive(Debug, Clone, Copy)]
pub struct RigidTransform(Mat4);

impl RigidTransform {
    pub fn new(matrix: Mat4) -> Result<Self, QualityError> {
        finite_matrix(&matrix, "rigid transform")?;
        if matrix.m[3] != [0.0, 0.0, 0.0, 1.0] {
            return Err(QualityError::InvalidView);
        }
        for r in 0..3 {
            for s in 0..3 {
                let dot: f32 = (0..3).map(|c| matrix.m[r][c] * matrix.m[s][c]).sum();
                if (dot - if r == s { 1.0 } else { 0.0 }).abs() > 1e-5 {
                    return Err(QualityError::InvalidView);
                }
            }
        }
        let a = matrix.m;
        let determinant = a[0][0] * (a[1][1] * a[2][2] - a[1][2] * a[2][1])
            - a[0][1] * (a[1][0] * a[2][2] - a[1][2] * a[2][0])
            + a[0][2] * (a[1][0] * a[2][1] - a[1][1] * a[2][0]);
        if determinant <= 0.0 {
            return Err(QualityError::InvalidView);
        }
        Ok(Self(matrix))
    }

    pub fn identity() -> Self {
        Self(Mat4::identity())
    }
    pub fn matrix(self) -> Mat4 {
        self.0
    }
    pub fn transform_point(self, local: [f32; 3]) -> Result<[f32; 3], QualityError> {
        if !local.iter().all(|v| v.is_finite()) {
            return Err(QualityError::NonFinite("local point"));
        }
        let world = self.0.transform_vec4([local[0], local[1], local[2], 1.0]);
        if !world.iter().all(|v| v.is_finite()) {
            return Err(QualityError::NonFinite("rigid world point"));
        }
        Ok([world[0], world[1], world[2]])
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Perspective {
    fov_y: f32,
    aspect: f32,
    near_m: f32,
    far_m: f32,
}

impl Perspective {
    pub fn new(fov_y: f32, aspect: f32, near_m: f32, far_m: f32) -> Result<Self, QualityError> {
        if ![fov_y, aspect, near_m, far_m].iter().all(|v| v.is_finite()) {
            return Err(QualityError::NonFinite("projection"));
        }
        if fov_y <= 0.0
            || fov_y >= std::f32::consts::PI
            || aspect <= 0.0
            || near_m <= 0.0
            || far_m <= near_m
        {
            return Err(QualityError::InvalidProjection);
        }
        let value = Self {
            fov_y,
            aspect,
            near_m,
            far_m,
        };
        finite_matrix(&value.matrix(), "projection matrix")?;
        Ok(value)
    }

    pub fn matrix(self) -> Mat4 {
        perspective_rh_zo(self.fov_y, self.aspect, self.near_m, self.far_m)
    }

    /// Configuration identity excludes the current jitter sample and camera pose.
    pub fn identity(self) -> [u32; 4] {
        [
            self.fov_y.to_bits(),
            self.aspect.to_bits(),
            self.near_m.to_bits(),
            self.far_m.to_bits(),
        ]
    }

    pub fn far_m(self) -> f32 {
        self.far_m
    }
}

/// These types cannot be passed interchangeably without an explicit conversion.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LegacySceneDepthQuirk(f32);
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DepthZo(f32);
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LinearViewDepthM(f32);

impl LegacySceneDepthQuirk {
    pub fn value(self) -> f32 {
        self.0
    }
}
impl DepthZo {
    pub fn value(self) -> f32 {
        self.0
    }
}
impl LinearViewDepthM {
    pub fn value(self) -> f32 {
        self.0
    }
}

/// A finite sentinel is never evidence of coverage; inspect `covered` first.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DepthDomains {
    pub legacy_scene_depth_quirk: LegacySceneDepthQuirk,
    pub depth_zo: DepthZo,
    pub linear_view_depth_m: LinearViewDepthM,
    pub covered: bool,
    /// True only when proven in-range physical depth required endpoint roundoff repair.
    pub depth_zo_roundoff_canonicalized: bool,
}

#[derive(Debug, Clone, Copy)]
pub struct DepthProjector {
    view: Mat4,
    vp: Mat4,
    vp_jittered: Mat4,
    projection: Perspective,
    extent: [u32; 2],
    jitter_pixels: [f32; 2],
}

impl DepthProjector {
    pub fn new(
        view: Mat4,
        projection: Perspective,
        jitter_pixels: [f32; 2],
        extent: [u32; 2],
    ) -> Result<Self, QualityError> {
        finite_matrix(&view, "view")?;
        if !jitter_pixels.iter().all(|v| v.is_finite()) {
            return Err(QualityError::NonFinite("jitter"));
        }
        if extent.contains(&0) {
            return Err(QualityError::InvalidExtent);
        }
        // View-space distances require the same rigid validation as dynamic objects.
        RigidTransform::new(view)?;
        let vp = projection.matrix().mul(&view);
        finite_matrix(&vp, "view projection")?;
        // Same convention/order as g14_3_lane_body::jittered_vp: sampled pixel + jitter.
        let mut m = vp.m;
        let sx = 2.0 * jitter_pixels[0] / extent[0] as f32;
        let sy = 2.0 * jitter_pixels[1] / extent[1] as f32;
        for c in 0..4 {
            m[0][c] -= sx * m[3][c];
            m[1][c] += sy * m[3][c];
        }
        let vp_jittered = Mat4 { m };
        finite_matrix(&vp_jittered, "jittered view projection")?;
        Ok(Self {
            view,
            vp,
            vp_jittered,
            projection,
            extent,
            jitter_pixels,
        })
    }

    pub fn jittered_vp(&self) -> Mat4 {
        self.vp_jittered
    }
    pub fn projection(&self) -> Perspective {
        self.projection
    }
    pub fn extent(&self) -> [u32; 2] {
        self.extent
    }
    pub fn jitter_pixels(&self) -> [f32; 2] {
        self.jitter_pixels
    }

    pub fn miss(&self) -> DepthDomains {
        DepthDomains {
            legacy_scene_depth_quirk: LegacySceneDepthQuirk(1.0),
            depth_zo: DepthZo(1.0),
            linear_view_depth_m: LinearViewDepthM(self.projection.far_m),
            covered: false,
            depth_zo_roundoff_canonicalized: false,
        }
    }

    pub fn project_hit(&self, position_ws: [f32; 3]) -> Result<DepthDomains, QualityError> {
        if !position_ws.iter().all(|v| v.is_finite()) {
            return Err(QualityError::NonFinite("world hit"));
        }
        let point = [position_ws[0], position_ws[1], position_ws[2], 1.0];
        let clip = self.vp_jittered.transform_vec4(point);
        let v = self.view.transform_vec4(point);
        if !clip.iter().chain(v.iter()).all(|x| x.is_finite()) {
            return Err(QualityError::NonFinite("projected hit"));
        }
        if clip[3] <= 0.0 || v[2] >= 0.0 {
            return Err(QualityError::BehindCamera);
        }
        let linear_m = -v[2];
        if linear_m < self.projection.near_m
            || linear_m > self.projection.far_m
            || clip[0].abs() > clip[3]
            || clip[1].abs() > clip[3]
        {
            return Err(QualityError::OutsideClipVolume);
        }
        // The physical near/far interval is checked independently, before any rounding
        // canonicalization. f32 z/w can round above 1 even for a far-plane-adjacent hit.
        // This never admits a hit beyond the physical planes or clamps arbitrary inputs.
        let raw_zo = clip[2] / clip[3];
        if !raw_zo.is_finite() {
            return Err(QualityError::NonFinite("ZO depth"));
        }
        let zo = if linear_m == self.projection.near_m {
            0.0
        } else if linear_m == self.projection.far_m {
            1.0
        } else {
            raw_zo.clamp(0.0, 1.0)
        };
        // Preserve the diagnostic legacy operation order, including its near-zero gate.
        // It reads unjittered VP rows 0 and 1; it is not a physical depth conversion.
        let legacy_row = |r: usize| {
            ((self.vp.m[r][0] * position_ws[0] + self.vp.m[r][1] * position_ws[1])
                + self.vp.m[r][2] * position_ws[2])
                + self.vp.m[r][3]
        };
        let numerator = legacy_row(0);
        let denominator = legacy_row(1);
        let gate = ((denominator.abs() - 0.00000001) * 1e30).clamp(0.0, 1.0);
        let safe = denominator + (1.0 - gate);
        let legacy = gate * (numerator / safe) + (1.0 - gate);
        if !legacy.is_finite() {
            return Err(QualityError::NonFinite("legacy depth"));
        }
        Ok(DepthDomains {
            legacy_scene_depth_quirk: LegacySceneDepthQuirk(legacy),
            depth_zo: DepthZo(zo),
            linear_view_depth_m: LinearViewDepthM(linear_m),
            covered: true,
            depth_zo_roundoff_canonicalized: raw_zo != zo,
        })
    }
}

/// Invalid previous projection is an explicit rejection, not a zero-motion claim.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum MotionSample {
    Valid { previous_minus_current_uv: [f32; 2] },
    RejectHistory,
}

/// Motion of corresponding surface points; previous geometry may differ from current.
/// UV origin/sign matches temporal::common::compute_camera_mv.
fn surface_motion(
    current_ws: [f32; 3],
    previous_ws: [f32; 3],
    current_projector: &DepthProjector,
    previous_projector: Option<&DepthProjector>,
) -> Result<MotionSample, QualityError> {
    if !current_ws
        .iter()
        .chain(previous_ws.iter())
        .all(|v| v.is_finite())
    {
        return Err(QualityError::NonFinite("motion point"));
    }
    let Some(previous_projector) = previous_projector else {
        return Ok(MotionSample::RejectHistory);
    };
    let uv = |p: [f32; 3], projector: &DepthProjector| -> Result<Option<[f32; 2]>, QualityError> {
        // Preserve the same physical clipping as GBuffer, including valid far-plane
        // roundoff; an untyped raw matrix cannot identify its intended near/far planes.
        match projector.project_hit(p) {
            Ok(_) => {}
            Err(QualityError::OutsideClipVolume | QualityError::BehindCamera) => return Ok(None),
            Err(error) => return Err(error),
        }
        let c = projector
            .vp_jittered
            .transform_vec4([p[0], p[1], p[2], 1.0]);
        if !c.iter().all(|v| v.is_finite()) {
            return Err(QualityError::NonFinite("motion clip"));
        }
        if c[3] <= 1e-8 {
            return Ok(None);
        }
        let out = [0.5 * (c[0] / c[3] + 1.0), 0.5 * (1.0 - c[1] / c[3])];
        if !out.iter().all(|v| (0.0..=1.0).contains(v)) {
            return Ok(None);
        }
        Ok(Some(out))
    };
    match (
        uv(current_ws, current_projector)?,
        uv(previous_ws, previous_projector)?,
    ) {
        (Some(current), Some(previous)) => Ok(MotionSample::Valid {
            previous_minus_current_uv: [previous[0] - current[0], previous[1] - current[1]],
        }),
        _ => Ok(MotionSample::RejectHistory),
    }
}

/// Public MV construction projects the same local surface point through both rigid
/// transforms. Separate arbitrary world points cannot be passed as matching geometry.
pub fn rigid_surface_motion(
    local_point: [f32; 3],
    current_object: RigidTransform,
    previous_object: Option<RigidTransform>,
    current_projector: &DepthProjector,
    previous_projector: Option<&DepthProjector>,
) -> Result<MotionSample, QualityError> {
    let current_world = current_object.transform_point(local_point)?;
    let Some(previous_object) = previous_object else {
        return Ok(MotionSample::RejectHistory);
    };
    surface_motion(
        current_world,
        previous_object.transform_point(local_point)?,
        current_projector,
        previous_projector,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    fn projector() -> DepthProjector {
        DepthProjector::new(
            Mat4::identity(),
            Perspective::new(std::f32::consts::FRAC_PI_2, 1.0, 1.0, 9.0).unwrap(),
            [0.0; 2],
            [128, 128],
        )
        .unwrap()
    }
    fn close(a: f32, b: f32) {
        assert!((a - b).abs() < 1e-6, "{a} != {b}");
    }

    #[test]
    fn known_planes_separate_three_domains() {
        let p = projector();
        let a = p.project_hit([0.25, 0.5, -2.0]).unwrap();
        let b = p.project_hit([0.5, 1.0, -4.0]).unwrap();
        // Analytic right-handed projection: z = far/(far-near) - far*near/((far-near)*distance).
        close(a.depth_zo.value(), 0.5625);
        close(b.depth_zo.value(), 0.84375);
        close(a.linear_view_depth_m.value(), 2.0);
        close(b.linear_view_depth_m.value(), 4.0);
        close(a.legacy_scene_depth_quirk.value(), 0.5);
        close(b.legacy_scene_depth_quirk.value(), 0.5);
        // Off-axis ray length is not view depth.
        assert!((0.25f32 * 0.25 + 0.5 * 0.5 + 4.0).sqrt() > a.linear_view_depth_m.value());
    }

    #[test]
    fn far_hit_and_miss_have_equal_sentinels_but_distinct_coverage() {
        let p = projector();
        let hit = p.project_hit([0.0, 0.0, -9.0]).unwrap();
        let miss = p.miss();
        assert_eq!(hit.depth_zo, miss.depth_zo);
        assert_eq!(hit.linear_view_depth_m, miss.linear_view_depth_m);
        assert!(hit.covered && !miss.covered);
    }

    #[test]
    fn rigid_view_translation_preserves_meter_depth() {
        let mut view = Mat4::identity();
        view.m[2][3] = 3.0;
        let p = DepthProjector::new(view, projector().projection, [0.0; 2], [128; 2]).unwrap();
        close(
            p.project_hit([0.0, 0.0, -5.0])
                .unwrap()
                .linear_view_depth_m
                .value(),
            2.0,
        );
        view.m[0][0] = 2.0;
        assert!(matches!(
            DepthProjector::new(view, projector().projection, [0.0; 2], [128; 2]),
            Err(QualityError::InvalidView)
        ));
    }

    #[test]
    fn object_and_camera_translation_motion_have_correct_uv_sign() {
        let p = projector();
        let MotionSample::Valid {
            previous_minus_current_uv: mv,
        } = surface_motion([0.5, 0.0, -2.0], [0.0, 0.0, -2.0], &p, Some(&p)).unwrap()
        else {
            panic!()
        };
        close(mv[0], -0.125);
        close(mv[1], 0.0);
        let mut moved = Mat4::identity();
        moved.m[0][3] = -0.5;
        let current = DepthProjector::new(moved, p.projection, [0.0; 2], [128; 2]).unwrap();
        let MotionSample::Valid {
            previous_minus_current_uv: mv,
        } = surface_motion([0.0, 0.0, -2.0], [0.0, 0.0, -2.0], &current, Some(&p)).unwrap()
        else {
            panic!()
        };
        close(mv[0], 0.125);
    }

    #[test]
    fn jitter_changes_uv_but_not_physical_depth() {
        let plain = projector();
        let jittered =
            DepthProjector::new(Mat4::identity(), plain.projection, [0.25, -0.25], [100; 2])
                .unwrap();
        let position = [0.0, 0.0, -2.0];
        assert_eq!(
            plain.project_hit(position).unwrap().depth_zo,
            jittered.project_hit(position).unwrap().depth_zo
        );
        let MotionSample::Valid {
            previous_minus_current_uv: mv,
        } = surface_motion(position, position, &jittered, Some(&plain)).unwrap()
        else {
            panic!()
        };
        close(mv[0], 0.0025);
        close(mv[1], -0.0025);
    }

    #[test]
    fn invalid_input_and_unavailable_history_are_not_silent_values() {
        let p = projector();
        assert!(p.project_hit([f32::NAN, 0.0, -2.0]).is_err());
        assert!(p.project_hit([0.0, 0.0, 2.0]).is_err());
        assert!(p.project_hit([0.0, 0.0, -0.5]).is_err());
        assert!(Perspective::new(1.0, 1.0, 1.0, f32::INFINITY).is_err());
        assert_eq!(
            surface_motion([0.0, 0.0, -2.0], [0.0, 0.0, -2.0], &p, None).unwrap(),
            MotionSample::RejectHistory
        );
        assert_eq!(
            surface_motion([0.0, 0.0, -2.0], [0.0, 0.0, 2.0], &p, Some(&p)).unwrap(),
            MotionSample::RejectHistory
        );
    }

    #[test]
    fn near_far_roundoff_does_not_reject_valid_planes_or_accept_outside() {
        for (near, far) in [(0.1f32, 10000.0f32), (0.0001, 100.0)] {
            let p = DepthProjector::new(
                Mat4::identity(),
                Perspective::new(1.0, 1.0, near, far).unwrap(),
                [0.0; 2],
                [128; 2],
            )
            .unwrap();
            assert_eq!(
                p.project_hit([0.0, 0.0, -near]).unwrap().depth_zo.value(),
                0.0
            );
            assert_eq!(
                p.project_hit([0.0, 0.0, -far]).unwrap().depth_zo.value(),
                1.0
            );
            let inside_far = f32::from_bits(far.to_bits() - 1);
            assert!(p.project_hit([0.0, 0.0, -inside_far]).unwrap().covered);
            let before_near = f32::from_bits(near.to_bits() - 1);
            let beyond_far = f32::from_bits(far.to_bits() + 1);
            assert_eq!(
                p.project_hit([0.0, 0.0, -before_near]),
                Err(QualityError::OutsideClipVolume)
            );
            assert_eq!(
                p.project_hit([0.0, 0.0, -beyond_far]),
                Err(QualityError::OutsideClipVolume)
            );
            assert_eq!(
                surface_motion([0.0, 0.0, -far], [0.0, 0.0, -far], &p, Some(&p)).unwrap(),
                MotionSample::Valid {
                    previous_minus_current_uv: [0.0; 2]
                }
            );
        }
    }

    #[test]
    fn rigid_motion_enforces_same_local_point_and_dynamic_transform_subset() {
        let mut transform = Mat4::identity();
        transform.m[0][3] = 0.5;
        let p = projector();
        let MotionSample::Valid {
            previous_minus_current_uv: mv,
        } = rigid_surface_motion(
            [0.0, 0.0, -2.0],
            RigidTransform::new(transform).unwrap(),
            Some(RigidTransform::identity()),
            &p,
            Some(&p),
        )
        .unwrap()
        else {
            panic!()
        };
        close(mv[0], -0.125);
        for (row, col, value) in [(0, 0, 2.0), (0, 0, -1.0), (0, 1, 0.25), (3, 0, 0.1)] {
            let mut invalid = Mat4::identity();
            invalid.m[row][col] = value;
            assert!(RigidTransform::new(invalid).is_err());
        }
    }
}
