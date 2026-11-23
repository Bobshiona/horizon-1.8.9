//! A copy of the renderer's current frustum for scheduling, never for dropping
//! world data. Origin-relative coordinates retain precision near world bounds.
use glam::{Vec3, Vec4};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct View {
    origin: [i64; 3],
    planes: [Vec4; 5],
}
impl View {
    pub fn new(origin: [i64; 3], planes: [Vec4; 5]) -> Option<Self> {
        planes
            .iter()
            .all(|p| p.is_finite())
            .then_some(Self { origin, planes })
    }
    pub fn column_visible(&self, (x, z): (i32, i32)) -> bool {
        let min = Vec3::new(
            (f64::from(x) * 16.0 - self.origin[0] as f64) as f32,
            -(self.origin[1] as f64) as f32,
            (f64::from(z) * 16.0 - self.origin[2] as f64) as f32,
        );
        let max = min + Vec3::new(16.0, 256.0, 16.0);
        self.planes.iter().all(|p| {
            let corner = Vec3::new(
                if p.x >= 0.0 { max.x } else { min.x },
                if p.y >= 0.0 { max.y } else { min.y },
                if p.z >= 0.0 { max.z } else { min.z },
            );
            p.truncate().dot(corner) + p.w >= 0.0
        })
    }
}
