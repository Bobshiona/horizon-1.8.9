//! F3: `GuiOverlayDebug` (`avv`). The left column is `avv.b()`, the right
//! `avv.c()`; each line gets a translucent backing and grey text.

use crate::ui::Canvas;

/// Everything the overlay reports, gathered by the frame.
pub struct Info<'a> {
    /// `Minecraft.debug`: frames and section rebuilds in the last second.
    pub fps: u32,
    pub chunk_updates: u32,
    /// The frame limit, `None` when unlimited (`inf`).
    pub fps_limit: Option<u32>,
    pub vsync: bool,
    /// `RenderGlobal.getDebugInfoRenders`: sections drawn with geometry,
    /// and the view frustum's `(2D + 1)^2 * 16`.
    pub sections: u32,
    pub view_distance: u8,
    /// `pC` columns waiting or meshing, `pU` meshes waiting to upload, `aB`
    /// idle meshers.
    pub mesher: [usize; 3],
    /// Entities drawn this frame and loaded (the player included).
    pub entities: (usize, usize),
    pub particles: usize,
    pub columns: usize,
    /// The player's feet, look and the facts about the block there.
    pub feet: [f64; 3],
    pub yaw: f32,
    pub pitch: f32,
    pub here: Option<Here>,
    /// The block the crosshair is on and its registry name.
    pub looking_at: Option<([i32; 3], &'a str)>,
    /// `EntityPlayer.hasReducedDebug` (Join Game, entity status 22/23).
    pub reduced: bool,
    pub build: &'a str,
    pub cpu: &'a str,
    pub display: (u32, u32),
    pub gpu: &'a str,
}

/// The loaded column under the feet.
pub struct Here {
    pub biome: Option<u8>,
    /// Block light, then sky light.
    pub light: (u8, u8),
    pub difficulty: u8,
    pub time_of_day: i64,
}

/// `World.moonPhaseFactors`.
const MOON: [f32; 8] = [1.0, 0.75, 0.5, 0.25, 0.0, 0.25, 0.5, 0.75];

/// `DifficultyInstance.calculateAdditionalDifficulty` (`ok.a`) as the
/// client sees it: its chunks were never inhabited.
pub fn local_difficulty(difficulty: u8, time_of_day: i64) -> f32 {
    if difficulty == 0 {
        return 0.0;
    }
    let clamp = |v: f32, lo: f32, hi: f32| {
        if v < lo {
            lo
        } else if v > hi {
            hi
        } else {
            v
        }
    };
    let mut f = 0.75f32;
    let days = clamp((time_of_day as f32 + -72000.0) / 1_440_000.0, 0.0, 1.0) * 0.25;
    f += days;
    let phase = ((time_of_day / 24000 % 8 + 8) % 8) as usize;
    let mut extra = 0.0f32;
    extra += clamp(MOON[phase] * 0.25, 0.0, days);
    if difficulty == 1 {
        extra *= 0.5;
    }
    f += extra;
    f32::from(difficulty) * f
}

/// `BiomeGenBase.biomeName` by id, "Plains" for ids it doesn't register.
pub fn biome_name(id: u8) -> String {
    const BASE: [&str; 40] = [
        "Ocean",
        "Plains",
        "Desert",
        "Extreme Hills",
        "Forest",
        "Taiga",
        "Swampland",
        "River",
        "Hell",
        "The End",
        "FrozenOcean",
        "FrozenRiver",
        "Ice Plains",
        "Ice Mountains",
        "MushroomIsland",
        "MushroomIslandShore",
        "Beach",
        "DesertHills",
        "ForestHills",
        "TaigaHills",
        "Extreme Hills Edge",
        "Jungle",
        "JungleHills",
        "JungleEdge",
        "Deep Ocean",
        "Stone Beach",
        "Cold Beach",
        "Birch Forest",
        "Birch Forest Hills",
        "Roofed Forest",
        "Cold Taiga",
        "Cold Taiga Hills",
        "Mega Taiga",
        "Mega Taiga Hills",
        "Extreme Hills+",
        "Savanna",
        "Savanna Plateau",
        "Mesa",
        "Mesa Plateau F",
        "Mesa Plateau",
    ];
    match id {
        0..=39 => BASE[usize::from(id)].to_string(),
        129 => "Sunflower Plains".into(),
        132 => "Flower Forest".into(),
        140 => "Ice Plains Spikes".into(),
        160 => "Mega Spruce Taiga".into(),
        161 => "Redwood Taiga Hills M".into(),
        165 => "Mesa (Bryce)".into(),
        130 | 131 | 133 | 134 | 149 | 151 | 155..=158 | 162..=164 | 166 | 167 => {
            format!("{} M", BASE[usize::from(id - 128)])
        }
        _ => "Plains".into(),
    }
}

/// `MathHelper.wrapAngleTo180_float`.
fn wrap(mut angle: f32) -> f32 {
    angle %= 360.0;
    if angle >= 180.0 {
        angle -= 360.0;
    }
    if angle < -180.0 {
        angle += 360.0;
    }
    angle
}

fn header(info: &Info<'_>) -> [String; 6] {
    let limit = info.fps_limit.map_or("inf".to_string(), |l| l.to_string());
    [
        "Minecraft 1.8.9 (1.8.9/vanilla)".into(),
        format!(
            "{} fps ({} chunk update{}) T: {limit}{}{}{}{}",
            info.fps,
            info.chunk_updates,
            if info.chunk_updates == 1 { "" } else { "s" },
            if info.vsync { " vsync" } else { "" },
            "",
            "",
            " vbo"
        ),
        format!(
            "C: {}/{} (s) D: {}, pC: {:03}, pU: {}, aB: {}",
            info.sections,
            (u32::from(info.view_distance) * 2 + 1).pow(2) * 16,
            info.view_distance,
            info.mesher[0],
            info.mesher[1],
            info.mesher[2]
        ),
        format!(
            "E: {}/{}, B: 0, I: {}",
            info.entities.0,
            info.entities.1,
            info.entities.1.saturating_sub(info.entities.0)
        ),
        format!("P: {}. T: All: {}", info.particles, info.entities.1),
        format!("MultiplayerChunkCache: {}, {}", info.columns, info.columns),
    ]
}

/// `avv.b()`.
pub fn left(info: &Info<'_>) -> Vec<String> {
    let [x, y, z] = info.feet;
    let block = [x, y, z].map(|v| v.floor() as i32);
    let mut lines: Vec<String> = header(info).into();
    lines.push(String::new());
    if info.reduced {
        lines.push(format!(
            "Chunk-relative: {} {} {}",
            block[0] & 15,
            block[1] & 15,
            block[2] & 15
        ));
        return lines;
    }
    // `Entity.getHorizontalFacing`.
    let (facing, towards) = match ((info.yaw * 4.0 / 360.0 + 0.5).floor() as i32) & 3 {
        0 => ("south", "Towards positive Z"),
        1 => ("west", "Towards negative X"),
        2 => ("north", "Towards negative Z"),
        _ => ("east", "Towards positive X"),
    };
    lines.extend([
        format!("XYZ: {x:.3} / {y:.5} / {z:.3}"),
        format!("Block: {} {} {}", block[0], block[1], block[2]),
        format!(
            "Chunk: {} {} {} in {} {} {}",
            block[0] & 15,
            block[1] & 15,
            block[2] & 15,
            block[0] >> 4,
            block[1] >> 4,
            block[2] >> 4
        ),
        format!(
            "Facing: {facing} ({towards}) ({:.1} / {:.1})",
            wrap(info.yaw),
            wrap(info.pitch)
        ),
    ]);
    if let Some(here) = &info.here {
        let (block_light, sky) = here.light;
        lines.push(format!("Biome: {}", biome_name(here.biome.unwrap_or(1))));
        lines.push(format!(
            "Light: {} ({sky} sky, {block_light} block)",
            sky.max(block_light)
        ));
        lines.push(format!(
            "Local Difficulty: {:.2} (Day {})",
            local_difficulty(here.difficulty, here.time_of_day),
            here.time_of_day / 24000
        ));
    }
    if let Some((at, _)) = info.looking_at {
        lines.push(format!("Looking at: {} {} {}", at[0], at[1], at[2]));
    }
    lines
}

/// `avv.c()`, with this client's runtime in place of the JVM's.
pub fn right(info: &Info<'_>) -> Vec<String> {
    let mut lines = vec![
        format!("Rust: Horizon {} 64bit", info.build),
        String::new(),
        format!("CPU: {}", info.cpu),
        String::new(),
        format!("Display: {}x{} (Apple)", info.display.0, info.display.1),
        info.gpu.to_string(),
        "Metal".to_string(),
    ];
    if !info.reduced
        && let Some((_, name)) = info.looking_at
    {
        lines.push(String::new());
        lines.push(format!("minecraft:{name}"));
    }
    lines
}

/// `avv.a(avr)`: each non-empty line on a `0x90505050` backing in `0xE0E0E0`
/// without shadow, nine pixels apart from the top corners.
pub fn draw(canvas: &mut Canvas<'_>, left: &[String], right: &[String]) {
    let width = canvas.layout.width;
    for (i, line) in left.iter().enumerate() {
        if line.is_empty() {
            continue;
        }
        let top = 2.0 + 9.0 * i as f32;
        let w = canvas.res.width(line) as f32;
        canvas.rect(1.0, top - 1.0, 2.0 + w + 1.0, top + 8.0, 0x9050_5050);
        canvas.text(line, 2.0, top, 0xFFE0_E0E0, false);
    }
    for (i, line) in right.iter().enumerate() {
        if line.is_empty() {
            continue;
        }
        let top = 2.0 + 9.0 * i as f32;
        let w = canvas.res.width(line) as f32;
        let x = width - 2.0 - w;
        canvas.rect(x - 1.0, top - 1.0, x + w + 1.0, top + 8.0, 0x9050_5050);
        canvas.text(line, x, top, 0xFFE0_E0E0, false);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn info() -> Info<'static> {
        Info {
            fps: 60,
            chunk_updates: 1,
            fps_limit: None,
            vsync: true,
            sections: 812,
            view_distance: 12,
            mesher: [3, 0, 7],
            entities: (4, 9),
            particles: 12,
            columns: 625,
            feet: [-12.5, 64.0, 3.25],
            yaw: 450.0,
            pitch: 12.34,
            here: Some(Here {
                biome: Some(132),
                light: (3, 12),
                difficulty: 2,
                time_of_day: 6000,
            }),
            looking_at: Some(([-13, 63, 3], "grass")),
            reduced: false,
            build: "abc1234",
            cpu: "10x Apple M5",
            display: (2880, 1864),
            gpu: "Apple M5",
        }
    }

    #[test]
    fn the_left_column_follows_gui_overlay_debug() {
        assert_eq!(
            left(&info()),
            [
                "Minecraft 1.8.9 (1.8.9/vanilla)",
                "60 fps (1 chunk update) T: inf vsync vbo",
                "C: 812/10000 (s) D: 12, pC: 003, pU: 0, aB: 7",
                "E: 4/9, B: 0, I: 5",
                "P: 12. T: All: 9",
                "MultiplayerChunkCache: 625, 625",
                "",
                "XYZ: -12.500 / 64.00000 / 3.250",
                "Block: -13 64 3",
                "Chunk: 3 0 3 in -1 4 0",
                "Facing: west (Towards negative X) (90.0 / 12.3)",
                "Biome: Flower Forest",
                "Light: 12 (12 sky, 3 block)",
                "Local Difficulty: 1.50 (Day 0)",
                "Looking at: -13 63 3",
            ]
        );
        let mut reduced = info();
        reduced.reduced = true;
        assert_eq!(left(&reduced)[7], "Chunk-relative: 3 0 3");
        assert_eq!(left(&reduced).len(), 8);
        assert!(!right(&reduced).iter().any(|l| l.starts_with("minecraft:")));
        assert_eq!(right(&info()).last().unwrap(), "minecraft:grass");
    }

    #[test]
    fn local_difficulty_and_biomes_match_the_jar() {
        assert_eq!(local_difficulty(0, 10_000_000), 0.0);
        // Normal on day 0: 2 * 0.75.
        assert_eq!(local_difficulty(2, 0), 1.5);
        // Hard after 60 days at full moon: 3 * (0.75 + 0.25 + 0.25).
        assert_eq!(local_difficulty(3, 24000 * 64), 3.75);
        // Easy halves the moon's part: day 3 + 8 * 8 has phase 3 (0.25).
        let t = 24000 * 67;
        assert!((local_difficulty(1, t) - (0.75 + 0.25 + 0.0625 * 0.5)).abs() < 1e-6);
        assert_eq!(biome_name(1), "Plains");
        assert_eq!(biome_name(39), "Mesa Plateau");
        assert_eq!(biome_name(130), "Desert M");
        assert_eq!(biome_name(140), "Ice Plains Spikes");
        assert_eq!(biome_name(200), "Plains");
    }
}
