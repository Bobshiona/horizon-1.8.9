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
    let toward = |p: Vec3| eye + (p - eye) * 0.997;
    for (a, b) in EDGES {
        let (a, b) = (corner(min, max, a), corner(min, max, b));
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
        assert_eq!(STYLES.len(), 4);
    }
}
