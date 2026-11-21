//! HUD mods drawn as liquid-glass cards: keystrokes, CPS, FPS, armour,
//! potion effects, coordinates and ping. Each card's placement comes from
//! `Settings`; drawing returns the card rectangles for the HUD editor.
use crate::{
    glass::Panel,
    settings::{Hud, HudKind, Settings},
    sysfont::Weight,
    terrain::Registry,
    ui::Canvas,
};
use horizon_physics_189::player::Keys;
use horizon_protocol_47::nbt::Item;

/// What the HUD mods show this frame.
pub struct ModFrame<'a> {
    pub settings: &'a Settings,
    pub keys: Keys,
    /// Left and right mouse buttons held.
    pub buttons: [bool; 2],
    pub cps: [u32; 2],
    pub fps: Option<u32>,
    /// Helmet, chestplate, leggings, boots.
    pub armor: &'a [Option<Item>; 4],
    /// Effect, amplifier and remaining ticks.
    pub potions: &'a [(u8, i8, i32)],
    pub position: [f64; 3],
    pub yaw: f32,
    pub ping: Option<i32>,
}

const PAD: f32 = 4.0;
/// System-font line box, in GUI pixels.
const LINE: f32 = 8.0;
const TEXT: u32 = 0xFFF5_F7FA;
const DIM: u32 = 0xFFB8_BEC8;

/// A card's content size for `kind`, in GUI pixels at scale 1.
fn content(kind: HudKind, frame: &ModFrame<'_>, canvas: &Canvas<'_>) -> (f32, f32) {
    let width = |s: &str| canvas.system_width(s, LINE, Weight::Regular);
    match kind {
        HudKind::Keystrokes => (3.0 * KEY + 2.0 * GAP, 4.0 * KEY + 3.0 * GAP - 8.0),
        HudKind::Armor => (18.0, 4.0 * 17.0),
        HudKind::Potions => {
            let lines = potion_lines(frame, canvas);
            let w = lines
                .iter()
                .map(|(a, b)| width(a).max(width(b)))
                .fold(0.0f32, f32::max);
            (w.ceil() + 22.0, lines.len().max(1) as f32 * 22.0 - 2.0)
        }
        _ => {
            let lines = text_lines(kind, frame);
            let w = lines.iter().map(|l| width(l)).fold(0.0f32, f32::max);
            (w.ceil(), lines.len() as f32 * (LINE + 1.0) - 1.0)
        }
    }
}

const KEY: f32 = 20.0;
const GAP: f32 = 2.0;

fn text_lines(kind: HudKind, frame: &ModFrame<'_>) -> Vec<String> {
    match kind {
        HudKind::Cps => vec![format!("{} | {} CPS", frame.cps[0], frame.cps[1])],
        HudKind::Fps => vec![
            frame
                .fps
                .map_or_else(|| "-- FPS".to_owned(), |f| format!("{f} FPS")),
        ],
        HudKind::Ping => vec![
            frame
                .ping
                .map_or_else(|| "-- ms".to_owned(), |p| format!("{p} ms")),
        ],
        HudKind::Coordinates => {
            let [x, y, z] = frame.position;
            vec![
                format!("X {:.1}", x),
                format!("Y {:.1}", y),
                format!("Z {:.1}", z),
                facing(frame.yaw).to_owned(),
            ]
        }
        _ => Vec::new(),
    }
}

/// `EnumFacing` from the yaw, as the debug screen names it.
fn facing(yaw: f32) -> &'static str {
    match ((yaw.rem_euclid(360.0) + 45.0) / 90.0) as i32 % 4 {
        0 => "South (+Z)",
        1 => "West (-X)",
        2 => "North (-Z)",
        _ => "East (+X)",
    }
}

/// `Potion.getStatusIconIndex` by effect ID (inventory.png from y 198).
const ICONS: [Option<u8>; 24] = [
    None,
    Some(0),
    Some(1),
    Some(2),
    Some(3),
    Some(4),
    None,
    None,
    Some(10),
    Some(11),
    Some(7),
    Some(14),
    Some(15),
    Some(16),
    Some(8),
    Some(13),
    Some(12),
    Some(9),
    Some(5),
    Some(6),
    Some(17),
    Some(18),
    Some(18),
    None,
];

/// `Potion.getDurationString`: m:ss, or `**:**` for effects that never end.
fn duration(ticks: i32) -> String {
    if ticks >= 32767 {
        return "**:**".to_owned();
    }
    let seconds = ticks.max(0) / 20;
    format!("{}:{:02}", seconds / 60, seconds % 60)
}

fn potion_lines(frame: &ModFrame<'_>, canvas: &Canvas<'_>) -> Vec<(String, String)> {
    frame
        .potions
        .iter()
        .map(|&(effect, amplifier, ticks)| {
            let name = crate::tooltip::EFFECTS
                .get(usize::from(effect))
                .map_or_else(String::new, |key| {
                    canvas.res.translate(&format!("potion.{key}")).to_owned()
                });
            let level = match amplifier {
                1..=3 => format!(
                    " {}",
                    canvas
                        .res
                        .translate(&format!("enchantment.level.{}", amplifier + 1))
                ),
                _ => String::new(),
            };
            (format!("{name}{level}"), duration(ticks))
        })
        .collect()
}

fn card(canvas: &mut Canvas<'_>, rect: [f32; 4], editing: bool) {
    let mut panel = Panel::new(rect[0], rect[1], rect[2], rect[3]);
    panel.radius = 5.0;
    if editing {
        panel.tint = [0.35, 0.55, 1.0, 0.45];
    }
    canvas.glass(panel);
}

/// Where a card of unscaled `size` sits on a `screen`-sized interface:
/// its saved corner, scaled, kept fully on screen and on whole pixels.
fn place(hud: Hud, size: (f32, f32), screen: (f32, f32)) -> [f32; 4] {
    let scale = hud.scale.clamp(0.5, 3.0);
    let (w, h) = (size.0 * scale, size.1 * scale);
    let x = (hud.x * screen.0)
        .clamp(0.0, (screen.0 - w).max(0.0))
        .floor();
    let y = (hud.y * screen.1)
        .clamp(0.0, (screen.1 - h).max(0.0))
        .floor();
    [x, y, x + w, y + h]
}

/// Draw every enabled card (all of them while editing) at its own scale;
/// returns their on-screen rectangles in GUI pixels.
pub fn draw(
    canvas: &mut Canvas<'_>,
    registry: &Registry,
    frame: &ModFrame<'_>,
    editing: bool,
) -> Vec<(HudKind, [f32; 4])> {
    let mut rects = Vec::new();
    let (screen_w, screen_h) = (canvas.layout.width, canvas.layout.height);
    for kind in HudKind::ALL {
        let hud = frame.settings.hud(kind);
        if !hud.enabled && !editing {
            continue;
        }
        if kind == HudKind::Potions && frame.potions.is_empty() && !editing {
            continue;
        }
        if kind == HudKind::Armor && frame.armor.iter().all(Option::is_none) && !editing {
            continue;
        }
        let (w, h) = content(kind, frame, canvas);
        let keys = kind == HudKind::Keystrokes;
        let (cw, ch) = if keys {
            (w, h)
        } else {
            (w + 2.0 * PAD, h + 2.0 * PAD)
        };
        let placed = place(hud, (cw, ch), (screen_w, screen_h));
        rects.push((kind, placed));
        // Draw in the card's own units under `GlStateManager.scale`.
        let scale = hud.scale.clamp(0.5, 3.0);
        canvas.scale = scale;
        let (x, y) = (placed[0] / scale, placed[1] / scale);
        let rect = [x, y, x + cw, y + ch];
        let disabled = editing && !hud.enabled;
        match kind {
            HudKind::Keystrokes => keystrokes(canvas, frame, x, y, editing),
            HudKind::Armor => {
                card(canvas, rect, editing);
                for (i, item) in frame.armor.iter().enumerate() {
                    if let Some(item) = item {
                        crate::hud::item_slot(
                            canvas,
                            registry,
                            item,
                            x + PAD + 1.0,
                            y + PAD + i as f32 * 17.0,
                        );
                    }
                }
            }
            HudKind::Potions => {
                card(canvas, rect, editing);
                let lines = potion_lines(frame, canvas);
                if lines.is_empty() {
                    canvas.system_text(
                        "Potion Effects",
                        x + PAD,
                        y + PAD,
                        LINE,
                        Weight::Regular,
                        DIM,
                    );
                }
                for (i, (name, time)) in lines.iter().enumerate() {
                    let ly = y + PAD + i as f32 * 22.0;
                    let icon = frame.potions.get(i).and_then(|&(effect, _, _)| {
                        ICONS.get(usize::from(effect)).copied().flatten()
                    });
                    if let Some(index) = icon {
                        let (u, v) = (
                            f32::from(index % 8) * 18.0,
                            198.0 + f32::from(index / 8) * 18.0,
                        );
                        canvas.sprite(
                            crate::ui::INVENTORY,
                            x + PAD,
                            ly,
                            u,
                            v,
                            18.0,
                            18.0,
                            [1.0; 4],
                        );
                    }
                    canvas.system_text(name, x + PAD + 20.0, ly, LINE, Weight::Semibold, TEXT);
                    canvas.system_text(time, x + PAD + 20.0, ly + 10.0, LINE, Weight::Regular, DIM);
                }
            }
            _ => {
                card(canvas, rect, editing);
                for (i, line) in text_lines(kind, frame).iter().enumerate() {
                    let color = if disabled { DIM } else { TEXT };
                    let ly = y + PAD + i as f32 * (LINE + 1.0);
                    canvas.system_text(line, x + PAD, ly, LINE, Weight::Semibold, color);
                }
            }
        }
        canvas.scale = 1.0;
    }
    rects
}

fn keystrokes(canvas: &mut Canvas<'_>, frame: &ModFrame<'_>, x: f32, y: f32, editing: bool) {
    let key =
        |canvas: &mut Canvas<'_>, kx: f32, ky: f32, w: f32, h: f32, label: &str, down: bool| {
            let mut panel = Panel::new(kx, ky, kx + w, ky + h);
            panel.radius = 4.0;
            panel.shadow = 0.18;
            if down {
                panel.tint = [0.96, 0.97, 1.0, 0.82];
            } else if editing {
                panel.tint = [0.35, 0.55, 1.0, 0.45];
            }
            canvas.glass(panel);
            let color = if down { 0xFF1A_1C22 } else { TEXT };
            let size = if label.len() > 2 { 6.5 } else { LINE };
            let tw = canvas.system_width(label, size, Weight::Semibold);
            canvas.system_text(
                label,
                kx + (w - tw) / 2.0,
                ky + (h - size) / 2.0,
                size,
                Weight::Semibold,
                color,
            );
        };
    let k = frame.keys;
    let step = KEY + GAP;
    key(canvas, x + step, y, KEY, KEY, "W", k.forward);
    key(canvas, x, y + step, KEY, KEY, "A", k.left);
    key(canvas, x + step, y + step, KEY, KEY, "S", k.back);
    key(canvas, x + 2.0 * step, y + step, KEY, KEY, "D", k.right);
    let half = (3.0 * KEY + GAP) / 2.0;
    let left = format!("{} CPS", frame.cps[0]);
    let right = format!("{} CPS", frame.cps[1]);
    key(
        canvas,
        x,
        y + 2.0 * step,
        half,
        KEY,
        &left,
        frame.buttons[0],
    );
    key(
        canvas,
        x + half + GAP,
        y + 2.0 * step,
        half,
        KEY,
        &right,
        frame.buttons[1],
    );
    key(
        canvas,
        x,
        y + 3.0 * step,
        3.0 * KEY + 2.0 * GAP,
        KEY - 8.0,
        "———",
        k.jump,
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scaled_cards_stay_on_screen_at_their_corner() {
        let hud = |x, y, scale| Hud {
            enabled: true,
            x,
            y,
            scale,
        };
        let screen = (480.0, 270.0);
        assert_eq!(
            place(hud(0.1, 0.2, 1.0), (40.0, 10.0), screen),
            [48.0, 54.0, 88.0, 64.0]
        );
        assert_eq!(
            place(hud(0.1, 0.2, 2.0), (40.0, 10.0), screen),
            [48.0, 54.0, 128.0, 74.0]
        );
        // Pushed back inside at the far edge, and out-of-range scales clamp.
        assert_eq!(
            place(hud(1.0, 1.0, 2.0), (40.0, 10.0), screen),
            [400.0, 250.0, 480.0, 270.0]
        );
        assert_eq!(
            place(hud(0.0, 0.0, 9.0), (40.0, 10.0), screen),
            [0.0, 0.0, 120.0, 30.0]
        );
    }

    #[test]
    fn durations_and_facing_follow_vanilla_formatting() {
        assert_eq!(duration(20 * 75), "1:15");
        assert_eq!(duration(19), "0:00");
        assert_eq!(duration(40000), "**:**");
        assert_eq!(facing(0.0), "South (+Z)");
        assert_eq!(facing(90.0), "West (-X)");
        assert_eq!(facing(-90.0), "East (+X)");
        assert_eq!(facing(180.0), "North (-Z)");
    }
}
