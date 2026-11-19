//! Legacy collision operations verified against the owned 1.8.9 client's
//! aug.class methods a/b/c (Laug;D)D. Numeric geometry types reuse Horizon.
#![forbid(unsafe_code)]

pub mod player;
pub mod raycast;
pub mod shapes;
pub mod trig;

pub use horizon_core::geometry::{Aabb, Axis, Vec3};

/// Clip a moving AABB against one obstacle on one axis. Other-axis touching
/// faces permit sliding. Exact comparisons preserve arbitrarily small gaps.
pub fn clip_offset(obstacle: Aabb, moving: Aabb, axis: Axis, mut offset: f64) -> f64 {
    for other in Axis::ALL {
        if other != axis
            && (moving.max(other) <= obstacle.min(other)
                || moving.min(other) >= obstacle.max(other))
        {
            return offset;
        }
    }
    if offset > 0.0 && moving.max(axis) <= obstacle.min(axis) {
        let gap = obstacle.min(axis) - moving.max(axis);
        if gap < offset {
            offset = gap;
        }
    } else if offset < 0.0 && moving.min(axis) >= obstacle.max(axis) {
        let gap = obstacle.max(axis) - moving.min(axis);
        if gap > offset {
            offset = gap;
        }
    }
    offset
}

#[derive(Clone, Copy, Debug)]
pub struct Slide {
    pub bounds: Aabb,
    pub movement: Vec3,
    pub horizontal_collision: bool,
    pub vertical_collision: bool,
    pub on_ground: bool,
}

/// Non-stepping sweep in vanilla 1.8.9's fixed Y, X, Z order. The caller
/// supplies collision boxes gathered over the expanded movement bounds.
pub fn slide(mut bounds: Aabb, requested: Vec3, obstacles: &[Aabb]) -> Slide {
    let mut movement = requested.components();
    for axis in [Axis::Y, Axis::X, Axis::Z] {
        let index = axis as usize;
        for &obstacle in obstacles {
            movement[index] = clip_offset(obstacle, bounds, axis, movement[index]);
        }
        let mut offset = [0.0; 3];
        offset[index] = movement[index];
        bounds = bounds.move_by(Vec3::new(offset[0], offset[1], offset[2]));
    }
    let vertical_collision = movement[1] != requested.y;
    Slide {
        bounds,
        movement: Vec3::new(movement[0], movement[1], movement[2]),
        horizontal_collision: movement[0] != requested.x || movement[2] != requested.z,
        vertical_collision,
        on_ground: vertical_collision && requested.y < 0.0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clipping_preserves_sub_epsilon_motion_and_touching_faces() {
        let moving = Aabb::unit_cube(Vec3::ZERO);
        let obstacle = Aabb::unit_cube(Vec3::new(1.0 + 1e-10, 0.0, 0.0));
        let gap = obstacle.min(Axis::X) - moving.max(Axis::X);
        assert_eq!(clip_offset(obstacle, moving, Axis::X, 1e-9), gap);
        assert_eq!(clip_offset(obstacle, moving, Axis::X, 1e-12), 1e-12);
        let touching = Aabb::unit_cube(Vec3::new(1.0, 1.0, 0.0));
        assert_eq!(clip_offset(touching, moving, Axis::X, 2.0), 2.0);
        let negative = Aabb::unit_cube(Vec3::new(-2.0, 0.0, 0.0));
        assert_eq!(clip_offset(negative, moving, Axis::X, -4.0), -1.0);
    }

    #[test]
    fn downward_collision_sets_ground_and_ceiling_collision_clears_it() {
        let bounds = Aabb::unit_cube(Vec3::new(0.0, 1.0, 0.0));
        let result = slide(
            bounds,
            Vec3::new(0.0, -0.08, 0.0),
            &[Aabb::unit_cube(Vec3::ZERO)],
        );
        assert!(result.on_ground);
        assert!(result.vertical_collision);
        assert_eq!(result.movement.y, 0.0);
        let ceiling = Aabb::unit_cube(Vec3::new(0.0, 2.0, 0.0));
        let result = slide(bounds, Vec3::new(0.0, 0.42, 0.0), &[ceiling]);
        assert!(!result.on_ground);
        assert_eq!(result.movement.y, 0.0);
    }

    #[test]
    fn fixed_x_before_z_order_controls_diagonal_corner_sliding() {
        let bounds = Aabb::unit_cube(Vec3::ZERO);
        let obstacle = Aabb::unit_cube(Vec3::new(1.0, 0.0, 1.0));
        let result = slide(bounds, Vec3::new(0.5, 0.0, 1.0), &[obstacle]);
        assert_eq!(result.movement, Vec3::new(0.5, 0.0, 0.0));
        assert!(result.horizontal_collision);
    }
}
