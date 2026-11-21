//! Small 3D math: vectors, 3×3 matrices and affine transforms, plus GoldSrc's
//! Euler convention (`AngleMatrix`: R = Rz(z) · Ry(y) · Rx(x), in radians).

pub type V3 = [f32; 3];

pub fn add(a: V3, b: V3) -> V3 {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}
pub fn sub(a: V3, b: V3) -> V3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
pub fn mul(a: V3, s: f32) -> V3 {
    [a[0] * s, a[1] * s, a[2] * s]
}
pub fn dot(a: V3, b: V3) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
pub fn cross(a: V3, b: V3) -> V3 {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}
pub fn len(a: V3) -> f32 {
    dot(a, a).sqrt()
}
pub fn norm(a: V3) -> V3 {
    let l = len(a);
    if l > 1e-12 { mul(a, 1.0 / l) } else { [0.0; 3] }
}

/// Row-major 3×3 matrix: `m[row][col]`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct M3(pub [[f32; 3]; 3]);

impl M3 {
    pub const IDENTITY: M3 = M3([[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]]);

    pub fn rows(r0: V3, r1: V3, r2: V3) -> M3 {
        M3([r0, r1, r2])
    }
    pub fn mul(&self, o: &M3) -> M3 {
        let mut m = [[0.0; 3]; 3];
        for (r, row) in m.iter_mut().enumerate() {
            for (c, v) in row.iter_mut().enumerate() {
                *v = (0..3).map(|k| self.0[r][k] * o.0[k][c]).sum();
            }
        }
        M3(m)
    }
    pub fn apply(&self, v: V3) -> V3 {
        [dot(self.0[0], v), dot(self.0[1], v), dot(self.0[2], v)]
    }
    pub fn det(&self) -> f32 {
        let m = self.0;
        m[0][0] * (m[1][1] * m[2][2] - m[1][2] * m[2][1])
            - m[0][1] * (m[1][0] * m[2][2] - m[1][2] * m[2][0])
            + m[0][2] * (m[1][0] * m[2][1] - m[1][1] * m[2][0])
    }

    /// The matrix for transforming normals: the inverse transpose up to a
    /// positive scale (cofactors, signed by the determinant, and scaled so
    /// the largest is 1, so tiny scales don't underflow). Normalise after.
    pub fn normal_matrix(&self) -> M3 {
        let m = self.0;
        let c = |r: usize, k: usize| {
            let (r1, r2, k1, k2) = ((r + 1) % 3, (r + 2) % 3, (k + 1) % 3, (k + 2) % 3);
            m[r1][k1] * m[r2][k2] - m[r1][k2] * m[r2][k1]
        };
        let det = m[0][0] * c(0, 0) + m[0][1] * c(0, 1) + m[0][2] * c(0, 2);
        let big = (0..9).map(|i| c(i / 3, i % 3).abs()).fold(0.0f32, f32::max);
        if big == 0.0 || !big.is_finite() {
            return M3([[0.0; 3]; 3]);
        }
        let s = if det < 0.0 { -1.0 } else { 1.0 } / big;
        M3(std::array::from_fn(|r| {
            std::array::from_fn(|k| s * c(r, k))
        }))
    }
    pub fn transpose(&self) -> M3 {
        let m = self.0;
        M3([
            [m[0][0], m[1][0], m[2][0]],
            [m[0][1], m[1][1], m[2][1]],
            [m[0][2], m[1][2], m[2][2]],
        ])
    }
    pub fn scaled(&self, s: f32) -> M3 {
        M3(self.0.map(|r| r.map(|v| v * s)))
    }
    pub fn plus(&self, o: &M3) -> M3 {
        let mut m = self.0;
        for (r, row) in m.iter_mut().enumerate() {
            for (c, v) in row.iter_mut().enumerate() {
                *v += o.0[r][c];
            }
        }
        M3(m)
    }

    /// Unit quaternion (x, y, z, w) as a rotation matrix.
    pub fn from_quat([x, y, z, w]: [f32; 4]) -> M3 {
        M3([
            [
                1.0 - 2.0 * (y * y + z * z),
                2.0 * (x * y - z * w),
                2.0 * (x * z + y * w),
            ],
            [
                2.0 * (x * y + z * w),
                1.0 - 2.0 * (x * x + z * z),
                2.0 * (y * z - x * w),
            ],
            [
                2.0 * (x * z - y * w),
                2.0 * (y * z + x * w),
                1.0 - 2.0 * (x * x + y * y),
            ],
        ])
    }

    /// GoldSrc bone angles (x, y, z) in radians, as `AngleMatrix` builds them.
    pub fn from_gs_angles([x, y, z]: V3) -> M3 {
        let (sr, cr) = x.sin_cos();
        let (sp, cp) = y.sin_cos();
        let (sy, cy) = z.sin_cos();
        M3([
            [cp * cy, sr * sp * cy - cr * sy, cr * sp * cy + sr * sy],
            [cp * sy, sr * sp * sy + cr * cy, cr * sp * sy - sr * cy],
            [-sp, sr * cp, cr * cp],
        ])
    }

    /// The inverse of `from_gs_angles` for a rotation matrix.
    pub fn to_gs_angles(&self) -> V3 {
        let m = self.0;
        // Pitch from atan2 (cos pitch from the yaw column), so roll and yaw
        // stay recoverable until the matrix truly loses them.
        let cp = m[0][0].hypot(m[1][0]);
        let y = (-m[2][0]).atan2(cp);
        if cp > 1e-6 {
            [m[2][1].atan2(m[2][2]), y, m[1][0].atan2(m[0][0])]
        } else {
            // Gimbal lock: put all of the remaining turn on z.
            [0.0, y, (-m[0][1]).atan2(m[1][1])]
        }
    }

    /// The smallest rotation taking unit vector `a` onto unit vector `b`.
    pub fn align(a: V3, b: V3) -> M3 {
        let c = dot(a, b);
        if c > 1.0 - 1e-7 {
            return M3::IDENTITY;
        }
        let v = cross(a, b);
        let sn = len(v);
        // Near a half turn the axis is still resolvable from the cross
        // product: rotate about it by atan2(|a×b|, a·b) (Rodrigues).
        if c < 0.0 && sn > 1e-7 {
            let k = mul(v, 1.0 / sn);
            let km = M3([[0.0, -k[2], k[1]], [k[2], 0.0, -k[0]], [-k[1], k[0], 0.0]]);
            let t = sn.atan2(c);
            return M3::IDENTITY
                .plus(&km.scaled(t.sin()))
                .plus(&km.mul(&km).scaled(1.0 - t.cos()));
        }
        if c < 0.0 {
            let helper = if a[0].abs() < 0.9 {
                [1.0, 0.0, 0.0]
            } else {
                [0.0, 1.0, 0.0]
            };
            let k = norm(cross(a, helper));
            let outer = M3([mul(k, k[0]), mul(k, k[1]), mul(k, k[2])]);
            return outer.scaled(2.0).plus(&M3::IDENTITY.scaled(-1.0));
        }
        let k = M3([[0.0, -v[2], v[1]], [v[2], 0.0, -v[0]], [-v[1], v[0], 0.0]]);
        M3::IDENTITY
            .plus(&k)
            .plus(&k.mul(&k).scaled(1.0 / (1.0 + c)))
    }
}

/// Affine transform `p ↦ r·p + t` (`r` is a pure rotation for skeleton frames).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Xf {
    pub r: M3,
    pub t: V3,
}

impl Xf {
    pub const IDENTITY: Xf = Xf {
        r: M3::IDENTITY,
        t: [0.0; 3],
    };

    pub fn new(r: M3, t: V3) -> Xf {
        Xf { r, t }
    }
    /// `self ∘ o`: apply `o` first.
    pub fn then(&self, o: &Xf) -> Xf {
        Xf {
            r: self.r.mul(&o.r),
            t: add(self.r.apply(o.t), self.t),
        }
    }
    pub fn apply(&self, p: V3) -> V3 {
        add(self.r.apply(p), self.t)
    }
    /// Inverse of a rigid transform (rotation + translation).
    pub fn inverse_rigid(&self) -> Xf {
        let rt = self.r.transpose();
        Xf {
            r: rt,
            t: mul(rt.apply(self.t), -1.0),
        }
    }
    pub fn from_trs(t: V3, q: [f32; 4], s: V3) -> Xf {
        let r = M3::from_quat(q);
        let r = M3(std::array::from_fn(|row| {
            std::array::from_fn(|c| r.0[row][c] * s[c])
        }));
        Xf { r, t }
    }
    /// A glTF column-major 4×4 matrix.
    pub fn from_gltf_matrix(a: &[f32]) -> Xf {
        Xf {
            r: M3(std::array::from_fn(|row| {
                std::array::from_fn(|c| a[c * 4 + row])
            })),
            t: [a[12], a[13], a[14]],
        }
    }
    /// A GoldSrc bone's local transform from `value` (position, angles).
    pub fn from_gs(v: &[f32; 6]) -> Xf {
        Xf {
            r: M3::from_gs_angles([v[3], v[4], v[5]]),
            t: [v[0], v[1], v[2]],
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn align_resolves_near_half_turns() {
        let a = [1.0, 0.0, 0.0];
        let b = [-(0.001f32).cos(), (0.001f32).sin(), 0.0];
        let r = M3::align(a, b).apply(a);
        assert!(len(sub(r, b)) < 1e-4, "{r:?}");
    }

    #[test]
    fn tiny_scales_keep_normals() {
        // Uniform scale 1e-7: cofactors of 1e-14 must not underflow to zero.
        let m = M3([[1e-7, 0.0, 0.0], [0.0, 1e-7, 0.0], [0.0, 0.0, 1e-7]]);
        let n = norm(m.normal_matrix().apply([0.0, 0.0, 1.0]));
        assert!((n[2] - 1.0).abs() < 1e-6, "{n:?}");
        // A mirror flips the sign so the normal stays outside.
        let m = M3([[-1e-7, 0.0, 0.0], [0.0, 1e-7, 0.0], [0.0, 0.0, 1e-7]]);
        assert!(m.normal_matrix().apply([1.0, 0.0, 0.0])[0] < 0.0);
        assert!(m.det() < 0.0);
    }

    #[test]
    fn euler_keeps_roll_near_gimbal_lock() {
        let x = [std::f32::consts::FRAC_PI_2, 1.567, 0.0];
        let m = M3::from_gs_angles(x);
        let back = M3::from_gs_angles(m.to_gs_angles());
        for r in 0..3 {
            assert!(len(sub(m.0[r], back.0[r])) < 1e-4, "{:?} vs {:?}", m, back);
        }
    }

    use super::*;

    fn close(a: &M3, b: &M3) -> bool {
        (0..3).all(|r| (0..3).all(|c| (a.0[r][c] - b.0[r][c]).abs() < 1e-5))
    }

    #[test]
    fn gs_angles_round_trip() {
        for a in [
            [0.1, 0.2, 0.3],
            [-1.2, 0.7, 2.9],
            [3.0, -1.5, -0.4],
            [0.0, 0.0, -1.571],
        ] {
            let m = M3::from_gs_angles(a);
            assert!(close(&M3::from_gs_angles(m.to_gs_angles()), &m), "{a:?}");
        }
    }

    #[test]
    fn gs_angles_axes() {
        // A yaw of +90° turns +X (forward) to +Y (left).
        let m = M3::from_gs_angles([0.0, 0.0, std::f32::consts::FRAC_PI_2]);
        let v = m.apply([1.0, 0.0, 0.0]);
        assert!((v[1] - 1.0).abs() < 1e-6 && v[0].abs() < 1e-6);
    }

    #[test]
    fn align_maps_a_to_b() {
        for (a, b) in [
            ([1.0, 0.0, 0.0], [0.0, 1.0, 0.0]),
            ([0.0, 0.0, 1.0], [0.0, 0.0, -1.0]),
            ([0.6, 0.8, 0.0], [0.0, 0.6, 0.8]),
        ] {
            let v = M3::align(a, b).apply(a);
            assert!(len(sub(v, b)) < 1e-5, "{a:?} -> {b:?} gave {v:?}");
        }
    }
}
