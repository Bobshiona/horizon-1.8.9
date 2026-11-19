//! `MathHelper.sin`/`cos`: a 65536-entry float table indexed with float
//! multiplication and Java's truncating, saturating int cast.
use std::sync::LazyLock;

static SIN_TABLE: LazyLock<Box<[f32; 65536]>> = LazyLock::new(|| {
    let mut table = Box::new([0.0f32; 65536]);
    for (index, value) in table.iter_mut().enumerate() {
        *value = (index as f64 * std::f64::consts::PI * 2.0 / 65536.0).sin() as f32;
    }
    table
});

pub fn sin(value: f32) -> f32 {
    SIN_TABLE[((value * 10430.378f32) as i32 & 65535) as usize]
}

pub fn cos(value: f32) -> f32 {
    SIN_TABLE[((value * 10430.378f32 + 16384.0f32) as i32 & 65535) as usize]
}

/// `MathHelper.sqrt_float`.
pub fn sqrt_float(value: f32) -> f32 {
    f64::from(value).sqrt() as f32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn table_lookup_quantizes_like_the_legacy_helper() {
        assert_eq!(sin(0.0), 0.0);
        assert_eq!(cos(0.0), 1.0);
        // 90 degrees as the jump code computes it: yaw * 0.017453292F.
        let quarter = 90.0f32 * 0.017_453_292f32;
        assert_eq!(sin(quarter), 1.0);
        // Quantized, not exact: the index truncates toward zero.
        let near = 0.000_05f32;
        assert_eq!(sin(near), 0.0);
        assert_ne!(f64::from(sin(1.0)), 1.0f64.sin());
        // Negative angles wrap through the 16-bit mask; NaN indexes zero.
        assert_eq!(sin(-quarter), -1.0);
        assert_eq!(sin(f32::NAN), 0.0);
    }
}
