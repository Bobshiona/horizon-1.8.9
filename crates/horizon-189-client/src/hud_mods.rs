//! HUD mods drawn as translucent graphite cards (no blur, so the world
//! reads through and they cost nothing extra): keystrokes, CPS, FPS,
//! armour, potion effects, coordinates and ping. The HUD editor marks each
//! card with the ember corner brackets. Each card's placement comes from
//! `Settings`; drawing returns the card rectangles for the HUD editor.
use crate::{
    settings::{Hud, HudKind, Settings},
    style,
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
    /// Smoothed CPU work per frame in milliseconds, without display waits.
    pub frame_ms: Option<f32>,
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
const TEXT: u32 = style::TEXT;
const DIM: u32 = style::MUTED;

/// A card's content size for `kind`, in GUI pixels at scale 1.
fn content(kind: HudKind, frame: &ModFrame<'_>, canvas: &Canvas<'_>) -> (f32, f32) {
    let width = |s: &str| canvas.system_width(s, LINE, Weight::Regular);
    match kind {
        HudKind::Keystrokes => (3.0 * KEY + 2.0 * GAP, 4.0 * KEY + 3.0 * GAP - 8.0),
        HudKind::Armor => (18.0, 4.0 * 17.0),
        HudKind::Potions => {
            let lines = potion_lines(frame, canvas);
            if lines.is_empty() {
                // The editor's placeholder.
                return (width(PLACEHOLDER).ceil(), LINE);
            }
            let w = lines
                .iter()
                .map(|(a, b)| width(a).max(width(b)))
                .fold(0.0f32, f32::max);
            (w.ceil() + 22.0, lines.len() as f32 * 22.0 - 2.0)
        }
        _ => {
            let lines = text_lines(kind, frame);
            let w = lines.iter().map(|l| width(l)).fold(0.0f32, f32::max);
            (w.ceil(), lines.len() as f32 * (LINE + 1.0) - 1.0)
        }
    }
}

const KEY: f32 = 20.0;
/// What the potion card says in the editor with no effects active.
const PLACEHOLDER: &str = "Potion Effects";
const GAP: f32 = 2.0;

fn text_lines(kind: HudKind, frame: &ModFrame<'_>) -> Vec<String> {
    match kind {
        HudKind::Cps => vec![format!("{} | {} CPS", frame.cps[0], frame.cps[1])],
        HudKind::Fps => vec![fps_line(frame.fps, frame.frame_ms)],
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

/// Shown frames per second, and how long each takes to make: the display
/// caps the first, the second is the client's own cost.
fn fps_line(fps: Option<u32>, ms: Option<f32>) -> String {
    let fps = fps.map_or_else(|| "--".to_owned(), |f| f.to_string());
    match ms {
        Some(ms) => format!("{fps} FPS · {ms:.1} ms"),
        None => format!("{fps} FPS"),
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

/// How a card is marked in the HUD editor.
#[derive(Clone, Copy, PartialEq)]
enum Mark {
    /// Playing: no marks.
    None,
    /// Editing: muted brackets; dimmed when the mod is off.
    Idle { off: bool },
    /// The card being dragged: ember brackets.
    Active,
}

impl Mark {
    fn fill(self) -> u32 {
        match self {
            Mark::Idle { off: true } => style::fade(style::HUD, 0.75),
            _ => style::HUD,
        }
    }

    fn brackets(self, canvas: &mut Canvas<'_>, rect: [f32; 4]) {
        match self {
            Mark::None => {}
            Mark::Idle { .. } => style::brackets(canvas, rect, style::MUTED),
            Mark::Active => style::brackets(canvas, rect, style::ACCENT),
        }
    }
}

fn card(canvas: &mut Canvas<'_>, rect: [f32; 4], mark: Mark) {
    style::panel(canvas, rect, mark.fill(), style::fade(style::EDGE, 0.7));
    mark.brackets(canvas, rect);
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
    dragging: Option<HudKind>,
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
        let mark = if dragging == Some(kind) {
            Mark::Active
        } else if editing {
            Mark::Idle { off: disabled }
        } else {
            Mark::None
        };
        match kind {
            HudKind::Keystrokes => keystrokes(canvas, frame, x, y, mark),
            HudKind::Armor => {
                card(canvas, rect, mark);
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
                card(canvas, rect, mark);
                let lines = potion_lines(frame, canvas);
                if lines.is_empty() {
                    canvas.system_text(PLACEHOLDER, x + PAD, y + PAD, LINE, Weight::Regular, DIM);
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
                card(canvas, rect, mark);
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

fn keystrokes(canvas: &mut Canvas<'_>, frame: &ModFrame<'_>, x: f32, y: f32, mark: Mark) {
    let key =
        |canvas: &mut Canvas<'_>, kx: f32, ky: f32, w: f32, h: f32, label: &str, down: bool| {
            let rect = [kx, ky, kx + w, ky + h];
            if down {
                canvas.rect(kx, ky, kx + w, ky + h, style::HUD_DOWN);
            } else {
                style::panel(canvas, rect, mark.fill(), style::fade(style::EDGE, 0.7));
            }
            let color = if down { style::BASE } else { TEXT };
            if label.is_empty() {
                // The space bar: a short drawn bar rather than dashes.
                let (bw, bh) = (w * 0.3, 1.0);
                let (bx, by) = (kx + (w - bw) / 2.0, ky + (h - bh) / 2.0);
                canvas.rect(bx, by, bx + bw, by + bh, color);
                return;
            }
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
        "",
        k.jump,
    );
    let size = (3.0 * KEY + 2.0 * GAP, 4.0 * step - 8.0 - GAP);
    mark.brackets(canvas, [x, y, x + size.0, y + size.1]);
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
    fn default_cards_do_not_overlap_in_the_editor() {
        // Generous card sizes; a 1080p window at GUI scale 4 is 480x270.
        let size = |kind| match kind {
            HudKind::Keystrokes => (66.0, 78.0),
            HudKind::Cps => (70.0, 16.0),
            HudKind::Fps => (80.0, 16.0),
            HudKind::Armor => (26.0, 76.0),
            HudKind::Potions => (110.0, 42.0),
            HudKind::Coordinates => (80.0, 43.0),
            HudKind::Ping => (40.0, 16.0),
        };
        let settings = Settings::default();
        let rects: Vec<_> = HudKind::ALL
            .iter()
            .map(|&kind| (kind, place(settings.hud(kind), size(kind), (480.0, 270.0))))
            .collect();
        for (i, (a, ra)) in rects.iter().enumerate() {
            for (b, rb) in &rects[i + 1..] {
                let apart = ra[2] <= rb[0] || rb[2] <= ra[0] || ra[3] <= rb[1] || rb[3] <= ra[1];
                assert!(apart, "{a:?} {ra:?} overlaps {b:?} {rb:?}");
            }
        }
    }

    #[test]
    fn the_fps_card_shows_frame_cost() {
        assert_eq!(fps_line(Some(60), Some(1.26)), "60 FPS · 1.3 ms");
        assert_eq!(fps_line(None, None), "-- FPS");
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
