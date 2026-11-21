//! `RenderGlobal.drawSelectionBoundingBox`: the targeted block's selection
//! box, grown by 0.002, drawn as twelve depth-tested edges. Metal has no wide
//! lines, so each edge is a camera-facing quad of constant screen width.
use crate::terrain::Vertex;
use glam::Vec3;

/// Outline styles the menu cycles through; the last draws nothing.
pub const STYLES: [&str; 4] = ["Vanilla", "Thick", "White", "Off"];

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Style {
    /// Gamma-space colour and its alpha.
    pub color: [f32; 3],
    pub alpha: f32,
    /// Edge width in points (logical pixels).
    pub width: f32,
}

/// Vanilla is `glColor4f(0, 0, 0, 0.4)` with `glLineWidth(2.0)`.
pub fn style(index: u8) -> Option<Style> {
    match index {
        0 => Some(Style {
            color: [0.0; 3],
            alpha: 0.4,
            width: 2.0,
        }),
        1 => Some(Style {
            color: [0.0; 3],
            alpha: 0.6,
            width: 4.0,
        }),
        2 => Some(Style {
            color: [1.0; 3],
            alpha: 0.8,
            width: 2.0,
        }),
        _ => None,
    }
}

/// Corner `index` of a box: bit 0 picks max x, bit 1 max y, bit 2 max z.
fn corner(min: Vec3, max: Vec3, index: usize) -> Vec3 {
    Vec3::new(
        if index & 1 != 0 { max.x } else { min.x },
        if index & 2 != 0 { max.y } else { min.y },
        if index & 4 != 0 { max.z } else { min.z },
    )
}

const EDGES: [(usize, usize); 12] = [
    (0, 1),
    (2, 3),
    (4, 5),
    (6, 7),
    (0, 2),
    (1, 3),
    (4, 6),
    (5, 7),
    (0, 4),
    (1, 5),
    (2, 6),
    (3, 7),
];

/// Append the outline of `min..max` (mesh-origin coordinates) seen from
/// `eye`. `pixel` is the world size of one screen point one block away.
/// Edges are nudged toward the eye so the half of each quad that dips
/// behind a face at grazing angles still wins the depth test.
pub fn outline(out: &mut Vec<Vertex>, min: Vec3, max: Vec3, eye: Vec3, pixel: f32, style: Style) {
    let (min, max) = (min - Vec3::splat(0.002), max + Vec3::splat(0.002));
    edges(out, min, max, eye, pixel, style);
}

fn edges(out: &mut Vec<Vertex>, min: Vec3, max: Vec3, eye: Vec3, pixel: f32, style: Style) {
    for (a, b) in EDGES {
        segment(
            out,
            corner(min, max, a),
            corner(min, max, b),
            eye,
            pixel,
            style,
        );
    }
}

/// One line `a..b` as a camera-facing quad of the style's screen width.
fn segment(out: &mut Vec<Vertex>, a: Vec3, b: Vec3, eye: Vec3, pixel: f32, style: Style) {
    let toward = |p: Vec3| eye + (p - eye) * 0.997;
    let along = (b - a).normalize_or_zero();
    let half = |p: Vec3| (p - eye).length() * pixel * style.width * 0.5;
    let (ha, hb) = (half(a), half(b));
    // Extend each end by half a width so corners close.
    let (a, b) = (a - along * ha, b + along * hb);
    let side = |p: Vec3, h: f32| along.cross(p - eye).normalize_or_zero() * h;
    let (sa, sb) = (side(a, ha), side(b, hb));
    let quad = [a - sa, a + sa, b + sb, b - sb];
    for index in [0, 1, 2, 0, 2, 3] {
        out.push(Vertex {
            position: toward(quad[index]).to_array(),
            uv: [0.0; 2],
            layer: u32::MAX,
            color: style.color,
            hurt: style.alpha,
        });
    }
}

/// `RenderManager.renderDebugBoundingBox` (F3+B) for an entity standing at
/// `feet` (mesh-origin coordinates, interpolated): its box in white, for a
/// living entity a red box 0.02 tall at eye height, and its look two blocks
/// out in blue from the eye. Vanilla's lines are depth tested without
/// writing depth, at the `glLineWidth(2)` the block outline leaves set.
pub fn hitbox(
    out: &mut Vec<Vertex>,
    feet: Vec3,
    (width, height): (f32, f32),
    eye_height: Option<f32>,
    look: Vec3,
    eye: Vec3,
    pixel: f32,
) {
    let line = |color: [f32; 3]| Style {
        color,
        alpha: 1.0,
        width: 2.0,
    };
    let f = width / 2.0;
    let min = feet - Vec3::new(f, 0.0, f);
    edges(
        out,
        min,
        min + Vec3::new(width, height, width),
        eye,
        pixel,
        line([1.0; 3]),
    );
    let eyes = feet.y + eye_height.unwrap_or(height * 0.85);
    if eye_height.is_some() {
        edges(
            out,
            Vec3::new(feet.x - f, eyes - 0.01, feet.z - f),
            Vec3::new(feet.x + f, eyes + 0.01, feet.z + f),
            eye,
            pixel,
            line([1.0, 0.0, 0.0]),
        );
    }
    let from = Vec3::new(feet.x, eyes, feet.z);
    segment(
        out,
        from,
        from + look * 2.0,
        eye,
        pixel,
        line([0.0, 0.0, 1.0]),
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn edges_hug_the_grown_box_at_a_constant_screen_width() {
        let vanilla = style(0).unwrap();
        let mut out = Vec::new();
        let eye = Vec3::new(0.5, 1.62, -3.0);
        outline(&mut out, Vec3::ZERO, Vec3::ONE, eye, 0.002, vanilla);
        assert_eq!(out.len(), 12 * 6);
        assert!(out.iter().all(|v| v.layer == u32::MAX && v.hurt == 0.4));
        for quad in out.chunks(6) {
            let points: Vec<Vec3> = quad.iter().map(|v| Vec3::from_array(v.position)).collect();
            // Every corner stays within a few hundredths of the box.
            for p in &points {
                assert!(p.cmpge(Vec3::splat(-0.05)).all() && p.cmple(Vec3::splat(1.05)).all());
            }
            // Width across the edge, relative to distance, is the style's.
            let (a, b) = (points[0], points[1]);
            let mid = (a + b) * 0.5;
            let width = (a - b).length() / ((mid - eye).length() * 0.002 / 0.997);
            assert!((width - vanilla.width).abs() < 0.05, "{width}");
        }
        assert_eq!(style(3), None);
        // A standing player's hitbox: twelve white edges, twelve red at
        // the eyes (1.62) and one blue look line ending two blocks out.
        let mut out = Vec::new();
        let look = Vec3::new(0.0, 0.0, 1.0);
        hitbox(
            &mut out,
            Vec3::ZERO,
            (0.6, 1.8),
            Some(1.62),
            look,
            eye,
            0.002,
        );
        assert_eq!(out.len(), 25 * 6);
        let colors: Vec<[f32; 3]> = out.chunks(6).map(|q| q[0].color).collect();
        assert!(colors[..12].iter().all(|&c| c == [1.0; 3]));
        assert!(colors[12..24].iter().all(|&c| c == [1.0, 0.0, 0.0]));
        assert_eq!(colors[24], [0.0, 0.0, 1.0]);
        let red: Vec<f32> = out[72..144].iter().map(|v| v.position[1]).collect();
        assert!(red.iter().all(|&y| (y - 1.62).abs() < 0.04), "{red:?}");
        let blue_z = out[144..]
            .iter()
            .map(|v| v.position[2])
            .fold(f32::MIN, f32::max);
        // Within the 0.3% nudge toward the eye.
        assert!((1.98..2.1).contains(&blue_z), "{blue_z}");
        let white_y = out[..72]
            .iter()
            .map(|v| v.position[1])
            .fold(f32::MIN, f32::max);
        assert!((1.8..1.85).contains(&white_y), "{white_y}");
        assert_eq!(STYLES.len(), 4);
    }
}
