//! The Horizon look, shared by the launcher, the mod menu and the HUD:
//! graphite surfaces with hairline edges and square corners, one ember
//! accent kept for the single most important thing on a screen, and corner
//! brackets, like a hitbox's, marking whatever is selected.
use crate::ui::Canvas;

/// The window behind everything.
pub const BASE: u32 = 0xFF0D_0E11;
/// Hairline edges and dividers.
pub const EDGE: u32 = 0xFF2C_3038;
pub const TEXT: u32 = 0xFFEC_EEF2;
pub const MUTED: u32 = 0xFF8C_93A0;
/// Ember: the one accent.
pub const ACCENT: u32 = 0xFFFF_5A1F;
/// HUD cards: graphite at 66% so the world reads through without blur.
pub const HUD: u32 = 0xA80D_0E11;
/// A pressed key on the HUD.
pub const HUD_DOWN: u32 = 0xE6EC_EEF2;

/// One physical pixel in GUI pixels.
pub fn hairline(canvas: &Canvas<'_>) -> f32 {
    1.0 / canvas.layout.factor.max(1.0) as f32 / canvas.scale.max(0.01)
}

/// A filled rectangle with a hairline edge inside it.
pub fn panel(canvas: &mut Canvas<'_>, [x0, y0, x1, y1]: [f32; 4], fill: u32, edge: u32) {
    canvas.rect(x0, y0, x1, y1, fill);
    let t = hairline(canvas);
    canvas.rect(x0, y0, x1, y0 + t, edge);
    canvas.rect(x0, y1 - t, x1, y1, edge);
    canvas.rect(x0, y0 + t, x0 + t, y1 - t, edge);
    canvas.rect(x1 - t, y0 + t, x1, y1 - t, edge);
}

/// The signature mark: four corner brackets just outside `rect`.
pub fn brackets(canvas: &mut Canvas<'_>, [x0, y0, x1, y1]: [f32; 4], color: u32) {
    let t = (hairline(canvas) * 2.0).max(0.5);
    let len = ((x1 - x0).min(y1 - y0) * 0.3).clamp(2.0, 6.0);
    let (x0, y0, x1, y1) = (x0 - t, y0 - t, x1 + t, y1 + t);
    for (cx, cy, sx, sy) in [
        (x0, y0, 1.0, 1.0),
        (x1, y0, -1.0, 1.0),
        (x0, y1, 1.0, -1.0),
        (x1, y1, -1.0, -1.0),
    ] {
        // The horizontal arm, then the vertical one.
        let (hx0, hx1) = if sx > 0.0 {
            (cx, cx + len)
        } else {
            (cx - len, cx)
        };
        let (hy0, hy1) = if sy > 0.0 { (cy, cy + t) } else { (cy - t, cy) };
        canvas.rect(hx0, hy0, hx1, hy1, color);
        let (vx0, vx1) = if sx > 0.0 { (cx, cx + t) } else { (cx - t, cx) };
        let (vy0, vy1) = if sy > 0.0 {
            (cy, cy + len)
        } else {
            (cy - len, cy)
        };
        canvas.rect(vx0, vy0, vx1, vy1, color);
    }
}

/// `color` at `opacity` of its alpha.
pub fn fade(color: u32, opacity: f32) -> u32 {
    let alpha = ((color >> 24) as f32 * opacity.clamp(0.0, 1.0)).round() as u32;
    (alpha << 24) | (color & 0x00FF_FFFF)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fade_scales_only_the_alpha() {
        assert_eq!(fade(ACCENT, 0.5), 0x80FF_5A1F);
        assert_eq!(fade(HUD, 1.0), HUD);
        assert_eq!(fade(TEXT, 0.0), 0x00EC_EEF2);
    }
}
