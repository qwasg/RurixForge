//! 2D 仿射矩阵(列向量):x' = a·x + c·y + tx;y' = b·x + d·y + ty。

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Affine {
    pub a: f32,
    pub b: f32,
    pub c: f32,
    pub d: f32,
    pub tx: f32,
    pub ty: f32,
}

impl Default for Affine {
    fn default() -> Self {
        Self::IDENTITY
    }
}

impl Affine {
    pub const IDENTITY: Affine = Affine { a: 1.0, b: 0.0, c: 0.0, d: 1.0, tx: 0.0, ty: 0.0 };

    pub fn translate(x: f32, y: f32) -> Affine {
        Affine { tx: x, ty: y, ..Self::IDENTITY }
    }

    /// 旋转(度,画布坐标标准旋转)。
    pub fn rotate_deg(deg: f32) -> Affine {
        let (s, c) = deg.to_radians().sin_cos();
        Affine { a: c, b: s, c: -s, d: c, tx: 0.0, ty: 0.0 }
    }

    pub fn scale(sx: f32, sy: f32) -> Affine {
        Affine { a: sx, d: sy, ..Self::IDENTITY }
    }

    /// T · R · S。
    pub fn trs(pos: [f32; 2], deg: f32, scale: [f32; 2]) -> Affine {
        Self::translate(pos[0], pos[1]).mul(&Self::rotate_deg(deg)).mul(&Self::scale(scale[0], scale[1]))
    }

    /// self · o(先 o 后 self)。
    pub fn mul(&self, o: &Affine) -> Affine {
        Affine {
            a: self.a * o.a + self.c * o.b,
            b: self.b * o.a + self.d * o.b,
            c: self.a * o.c + self.c * o.d,
            d: self.b * o.c + self.d * o.d,
            tx: self.a * o.tx + self.c * o.ty + self.tx,
            ty: self.b * o.tx + self.d * o.ty + self.ty,
        }
    }

    pub fn apply(&self, p: [f32; 2]) -> [f32; 2] {
        [self.a * p[0] + self.c * p[1] + self.tx, self.b * p[0] + self.d * p[1] + self.ty]
    }

    pub fn inverse(&self) -> Affine {
        let det = self.a * self.d - self.b * self.c;
        if det.abs() < 1e-12 {
            return Self::IDENTITY;
        }
        let inv = 1.0 / det;
        let a = self.d * inv;
        let b = -self.b * inv;
        let c = -self.c * inv;
        let d = self.a * inv;
        Affine { a, b, c, d, tx: -(a * self.tx + c * self.ty), ty: -(b * self.tx + d * self.ty) }
    }

    /// 加权和(LBS 用:Σ wᵢ·Mᵢ)。
    pub fn scaled_add(&self, o: &Affine, w: f32) -> Affine {
        Affine {
            a: self.a + o.a * w,
            b: self.b + o.b * w,
            c: self.c + o.c * w,
            d: self.d + o.d * w,
            tx: self.tx + o.tx * w,
            ty: self.ty + o.ty * w,
        }
    }

    pub const ZERO: Affine = Affine { a: 0.0, b: 0.0, c: 0.0, d: 0.0, tx: 0.0, ty: 0.0 };
}

pub fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

pub fn lerp2(a: [f32; 2], b: [f32; 2], t: f32) -> [f32; 2] {
    [lerp(a[0], b[0], t), lerp(a[1], b[1], t)]
}

/// 点到线段距离。
pub fn dist_point_segment(p: [f32; 2], a: [f32; 2], b: [f32; 2]) -> f32 {
    let ab = [b[0] - a[0], b[1] - a[1]];
    let ap = [p[0] - a[0], p[1] - a[1]];
    let len2 = ab[0] * ab[0] + ab[1] * ab[1];
    let t = if len2 <= 1e-12 { 0.0 } else { ((ap[0] * ab[0] + ap[1] * ab[1]) / len2).clamp(0.0, 1.0) };
    let q = [a[0] + ab[0] * t - p[0], a[1] + ab[1] * t - p[1]];
    (q[0] * q[0] + q[1] * q[1]).sqrt()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: [f32; 2], b: [f32; 2]) -> bool {
        (a[0] - b[0]).abs() < 1e-4 && (a[1] - b[1]).abs() < 1e-4
    }

    #[test]
    fn trs_and_inverse_roundtrip() {
        let m = Affine::trs([10.0, 5.0], 30.0, [2.0, 0.5]);
        let p = [3.0, -7.0];
        assert!(close(m.inverse().apply(m.apply(p)), p));
    }

    #[test]
    fn rotate_90_maps_x_to_y() {
        assert!(close(Affine::rotate_deg(90.0).apply([1.0, 0.0]), [0.0, 1.0]));
    }

    #[test]
    fn segment_distance() {
        assert!((dist_point_segment([0.0, 1.0], [-1.0, 0.0], [1.0, 0.0]) - 1.0).abs() < 1e-6);
        assert!((dist_point_segment([3.0, 0.0], [-1.0, 0.0], [1.0, 0.0]) - 2.0).abs() < 1e-6);
    }
}
