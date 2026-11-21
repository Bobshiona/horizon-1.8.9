//! F2: `ScreenShotHelper.saveScreenshot` (`avj`). The frame is read back on
//! the render thread; encoding and writing happen on a worker so a
//! screenshot never stalls a frame.

use std::path::{Path, PathBuf};
use std::sync::mpsc::Sender;

/// A frame read back from the GPU, tightly packed RGBA.
pub struct Pixels {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

/// `.minecraft/screenshots`, under the app's own folder.
pub fn directory() -> Option<PathBuf> {
    crate::updater::support_dir().map(|dir| dir.join("screenshots"))
}

/// `avj.a(File)`: the timestamp, then `_2`, `_3`… until the name is free.
pub fn free_name(stamp: &str, taken: impl Fn(&str) -> bool) -> String {
    (1u32..)
        .map(|n| {
            if n == 1 {
                format!("{stamp}.png")
            } else {
                format!("{stamp}_{n}.png")
            }
        })
        .find(|name| !taken(name))
        .expect("an unbounded range finds a free name")
}

/// `yyyy-MM-dd_HH.mm.ss` for a Unix time and a UTC offset in seconds.
pub fn stamp(unix: i64, offset: i64) -> String {
    let local = unix + offset;
    let (days, secs) = (local.div_euclid(86_400), local.rem_euclid(86_400));
    // Howard Hinnant's civil_from_days.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!(
        "{year:04}-{month:02}-{day:02}_{:02}.{:02}.{:02}",
        secs / 3600,
        secs / 60 % 60,
        secs % 60
    )
}

/// The local UTC offset, from `date +%z` (std has no time zones).
fn local_offset() -> i64 {
    let Ok(output) = std::process::Command::new("/bin/date").arg("+%z").output() else {
        return 0;
    };
    let text = String::from_utf8_lossy(&output.stdout);
    let text = text.trim();
    let sign = if text.starts_with('-') { -1 } else { 1 };
    let digits = text.trim_start_matches(['+', '-']);
    match (digits.get(..2), digits.get(2..4)) {
        (Some(h), Some(m)) => match (h.parse::<i64>(), m.parse::<i64>()) {
            (Ok(h), Ok(m)) => sign * (h * 3600 + m * 60),
            _ => 0,
        },
        _ => 0,
    }
}

/// Write an image as PNG, dropping alpha when `opaque` (vanilla saves RGB).
pub fn write_png(path: &Path, pixels: &Pixels, opaque: bool) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let file = std::fs::File::create(path).map_err(|e| e.to_string())?;
    let mut encoder = png::Encoder::new(std::io::BufWriter::new(file), pixels.width, pixels.height);
    encoder.set_depth(png::BitDepth::Eight);
    let rgb;
    let data = if opaque {
        encoder.set_color(png::ColorType::Rgb);
        rgb = pixels
            .rgba
            .as_chunks::<4>()
            .0
            .iter()
            .flat_map(|&[r, g, b, _]| [r, g, b])
            .collect::<Vec<u8>>();
        &rgb
    } else {
        encoder.set_color(png::ColorType::Rgba);
        &pixels.rgba
    };
    encoder
        .write_header()
        .and_then(|mut writer| writer.write_image_data(data))
        .map_err(|e| e.to_string())
}

/// Save on a worker; the chat line arrives on `done`: the file name, or why
/// it failed.
pub fn save(pixels: Pixels, dir: PathBuf, done: Sender<Result<String, String>>) {
    std::thread::spawn(move || {
        let unix = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_secs() as i64);
        let stamp = stamp(unix, local_offset());
        let name = free_name(&stamp, |name| dir.join(name).exists());
        let result = write_png(&dir.join(&name), &pixels, true).map(|()| name);
        let _ = done.send(result);
    });
}

/// `screenshot.success` / `screenshot.failure`, with the name underlined.
pub fn message(result: &Result<String, String>) -> String {
    match result {
        Ok(name) => format!("Saved screenshot as §n{name}"),
        Err(error) => format!("Couldn't save screenshot: {error}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn screenshot_names_follow_screen_shot_helper() {
        assert_eq!(stamp(0, 0), "1970-01-01_00.00.00");
        // 2026-11-21 09:27:05 UTC, shown in Pacific standard time.
        assert_eq!(stamp(1_795_253_225, -8 * 3600), "2026-11-21_01.27.05");
        assert_eq!(stamp(951_782_400, 0), "2000-02-29_00.00.00");
        let stamp = "2026-11-21_01.27.05";
        assert_eq!(free_name(stamp, |_| false), "2026-11-21_01.27.05.png");
        let taken = ["2026-11-21_01.27.05.png", "2026-11-21_01.27.05_2.png"];
        assert_eq!(
            free_name(stamp, |n| taken.contains(&n)),
            "2026-11-21_01.27.05_3.png"
        );
        assert_eq!(message(&Ok("a.png".into())), "Saved screenshot as §na.png");
    }

    #[test]
    fn screenshots_are_written_opaque() {
        let dir = std::env::temp_dir().join(format!("h189-shot-test-{}", std::process::id()));
        let path = dir.join("x.png");
        let pixels = Pixels {
            width: 2,
            height: 1,
            rgba: vec![255, 0, 0, 10, 0, 255, 0, 200],
        };
        write_png(&path, &pixels, true).unwrap();
        let decoder =
            png::Decoder::new(std::io::BufReader::new(std::fs::File::open(&path).unwrap()));
        let mut reader = decoder.read_info().unwrap();
        let mut buf = vec![0; reader.output_buffer_size().unwrap()];
        let info = reader.next_frame(&mut buf).unwrap();
        assert_eq!(info.color_type, png::ColorType::Rgb);
        assert_eq!(&buf[..info.buffer_size()], &[255, 0, 0, 0, 255, 0]);
        let _ = std::fs::remove_dir_all(dir);
    }
}
