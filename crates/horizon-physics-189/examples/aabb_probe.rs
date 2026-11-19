//! Numeric differential probe. Each input line is [axis, obstacle, moving,
//! offset], where boxes and offset use lossless f64 bit patterns as u64 values.
use horizon_physics_189::{Aabb, Axis, Vec3, clip_offset};
use std::{
    error::Error,
    io::{self, BufRead},
};

fn bounds(values: [u64; 6]) -> Aabb {
    let values = values.map(f64::from_bits);
    Aabb::new(
        Vec3::new(values[0], values[1], values[2]),
        Vec3::new(values[3], values[4], values[5]),
    )
}

fn main() -> Result<(), Box<dyn Error>> {
    for line in io::stdin().lock().lines() {
        let (axis, obstacle, moving, offset): (usize, [u64; 6], [u64; 6], u64) =
            serde_json::from_str(&line?)?;
        let axis = *Axis::ALL.get(axis).ok_or("Invalid axis")?;
        let result = clip_offset(
            bounds(obstacle),
            bounds(moving),
            axis,
            f64::from_bits(offset),
        );
        println!("{}", serde_json::to_string(&result.to_bits())?);
    }
    Ok(())
}
