//! The game menu Escape opens in a world (`GuiIngameMenu`) and the video
//! and controls settings behind it, as liquid-glass sheets: FOV,
//! sensitivity and render distance sliders, GUI scale, view bobbing and
//! fullscreen. Settings apply as they change and are saved by the caller.
use crate::{
    glass::Panel,
    menu::{button, ease, fade, switch},
    settings::{Settings, Video},
    sysfont::Weight,
    ui::{Canvas, Layout},
};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Page {
    #[default]
    Closed,
    Menu,
    Settings,
}

/// What a click asked the app to do.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Click {
    Resume,
    Mods,
    /// Disconnect (launcher sessions) or quit.
    Leave,
    /// A setting changed: apply and save.
    Changed,
    Nothing,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Slider {
    Fov,
    Sensitivity,
    Distance,
}

impl Slider {
    /// Position along the track, 0 to 1.
    fn get(self, video: &Video) -> f32 {
        match self {
            Slider::Fov => (video.fov - 30.0) / 80.0,
            Slider::Sensitivity => video.sensitivity,
            Slider::Distance => f32::from(video.render_distance - 2) / 14.0,
        }
    }

    fn set(self, video: &mut Video, t: f32) {
        let t = t.clamp(0.0, 1.0);
        match self {
            Slider::Fov => video.fov = (30.0 + t * 80.0).round(),
            Slider::Sensitivity => video.sensitivity = (t * 200.0).round() / 200.0,
            Slider::Distance => video.render_distance = (2.0 + t * 14.0).round() as u8,
        }
    }

    /// The slider's text, as `GameSettings.getKeyBinding` words it.
    fn label(self, video: &Video, translate: &dyn Fn(&str) -> String) -> String {
        match self {
            Slider::Fov => {
                let fov = video.fov.round() as i32;
                let value = match fov {
                    70 => translate("options.fov.min"),
                    110 => translate("options.fov.max"),
                    _ => fov.to_string(),
                };
                format!("{}: {value}", translate("options.fov"))
            }
            Slider::Sensitivity => {
                let value = match video.sensitivity {
                    s if s <= 0.0 => translate("options.sensitivity.min"),
                    s if s >= 1.0 => translate("options.sensitivity.max"),
                    s => format!("{}%", (s * 200.0).round() as i32),
                };
                format!("{}: {value}", translate("options.sensitivity"))
            }
            Slider::Distance => format!(
                "{}: {} chunks",
                translate("options.renderDistance"),
                video.render_distance
            ),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Target {
    Resume,
    Settings,
    Mods,
    Leave,
    Slider(Slider),
    GuiScale,
    Bobbing,
    Fullscreen,
    Done,
}

#[derive(Debug)]
struct Geometry {
    sheet: [f32; 4],
    targets: Vec<(Target, [f32; 4])>,
}

const BUTTON_H: f32 = 20.0;
const ROW_H: f32 = 24.0;
const PAD: f32 = 12.0;
const TITLE: f32 = 9.5;
const BODY: f32 = 7.5;
const WHITE: u32 = 0xFFF7_F8FA;
const LABEL: u32 = 0xFFE6_E9EF;
const TRACK: f32 = 120.0;

fn centred(layout: Layout, width: f32, height: f32) -> [f32; 4] {
    let x0 = ((layout.width - width) / 2.0).floor().max(0.0);
    let y0 = ((layout.height - height) / 2.0).floor().max(0.0);
    [x0, y0, x0 + width, y0 + height]
}

fn geometry(page: Page, layout: Layout) -> Geometry {
    match page {
        Page::Settings => {
            let rows = [
                Target::Slider(Slider::Fov),
                Target::Slider(Slider::Sensitivity),
                Target::Slider(Slider::Distance),
                Target::GuiScale,
                Target::Bobbing,
                Target::Fullscreen,
            ];
            let width = 300.0f32.min(layout.width - 8.0);
            let height = 34.0 + rows.len() as f32 * ROW_H + 8.0 + BUTTON_H + PAD;
            let sheet = centred(layout, width, height);
            let mut targets = Vec::new();
            for (index, target) in rows.into_iter().enumerate() {
                let y = sheet[1] + 34.0 + index as f32 * ROW_H;
                let rect = match target {
                    Target::Slider(_) => {
                        [sheet[2] - PAD - TRACK, y + 4.0, sheet[2] - PAD, y + 18.0]
                    }
                    Target::GuiScale => [sheet[2] - PAD - 70.0, y + 2.0, sheet[2] - PAD, y + 20.0],
                    _ => [sheet[2] - PAD - 18.0, y + 5.0, sheet[2] - PAD, y + 16.0],
                };
                targets.push((target, rect));
            }
            let bottom = sheet[3] - PAD - BUTTON_H;
            let mid = (sheet[0] + sheet[2]) / 2.0;
            targets.push((
                Target::Done,
                [mid - 60.0, bottom, mid + 60.0, bottom + BUTTON_H],
            ));
            Geometry { sheet, targets }
        }
        _ => {
            let buttons = [
                Target::Resume,
                Target::Settings,
                Target::Mods,
                Target::Leave,
            ];
            let width = 200.0f32.min(layout.width - 8.0);
            let height = 34.0 + buttons.len() as f32 * (BUTTON_H + 6.0) - 6.0 + PAD;
            let sheet = centred(layout, width, height);
            let targets = buttons
                .into_iter()
                .enumerate()
                .map(|(index, target)| {
                    let y = sheet[1] + 34.0 + index as f32 * (BUTTON_H + 6.0);
                    (target, [sheet[0] + PAD, y, sheet[2] - PAD, y + BUTTON_H])
                })
                .collect();
            Geometry { sheet, targets }
        }
    }
}

fn inside(rect: [f32; 4], (x, y): (f32, f32)) -> bool {
    x >= rect[0] && x < rect[2] && y >= rect[1] && y < rect[3]
}

#[derive(Default)]
pub struct Pause {
    page: Page,
    /// Open animation, 0 to 1.
    shown: f32,
    drag: Option<Slider>,
    /// The second option leaves for the launcher rather than quitting.
    pub leave_to_launcher: bool,
}

impl Pause {
    pub fn active(&self) -> bool {
        self.page != Page::Closed
    }

    pub fn open(&mut self) {
        self.page = Page::Menu;
        self.shown = 0.0;
    }

    /// Straight to the settings page (validation captures).
    pub fn open_settings(&mut self) {
        self.page = Page::Settings;
        self.shown = 0.0;
    }

    pub fn close(&mut self) {
        self.page = Page::Closed;
        self.drag = None;
    }

    /// Escape: settings back to the menu, the menu back to the game.
    pub fn back(&mut self) {
        self.drag = None;
        self.page = match self.page {
            Page::Settings => Page::Menu,
            _ => Page::Closed,
        };
    }

    pub fn step(&mut self, dt: f32) {
        self.shown = (self.shown + dt.clamp(0.0, 0.1) / 0.18).min(1.0);
    }

    pub fn press(&mut self, layout: Layout, mouse: (f32, f32), settings: &mut Settings) -> Click {
        let g = geometry(self.page, layout);
        let Some(target) = g
            .targets
            .iter()
            .find(|(_, rect)| inside(*rect, mouse))
            .map(|(target, _)| *target)
        else {
            return Click::Nothing;
        };
        let video = &mut settings.video;
        match target {
            Target::Resume => {
                self.close();
                Click::Resume
            }
            Target::Settings => {
                self.page = Page::Settings;
                Click::Nothing
            }
            Target::Mods => {
                self.close();
                Click::Mods
            }
            Target::Leave => {
                self.close();
                Click::Leave
            }
            Target::Done => {
                self.page = Page::Menu;
                Click::Nothing
            }
            Target::Slider(slider) => {
                self.drag = Some(slider);
                self.moved(layout, mouse, settings)
            }
            Target::GuiScale => {
                video.gui_scale = (video.gui_scale + 1) % 5;
                Click::Changed
            }
            Target::Bobbing => {
                video.view_bobbing = !video.view_bobbing;
                Click::Changed
            }
            Target::Fullscreen => {
                video.fullscreen = !video.fullscreen;
                Click::Changed
            }
        }
    }

    /// Dragging a slider.
    pub fn moved(&mut self, layout: Layout, mouse: (f32, f32), settings: &mut Settings) -> Click {
        let Some(slider) = self.drag else {
            return Click::Nothing;
        };
        let g = geometry(self.page, layout);
        let Some(&(_, rect)) = g.targets.iter().find(|(t, _)| *t == Target::Slider(slider)) else {
            return Click::Nothing;
        };
        let before = settings.video;
        slider.set(
            &mut settings.video,
            (mouse.0 - rect[0]) / (rect[2] - rect[0]),
        );
        if settings.video == before {
            Click::Nothing
        } else {
            Click::Changed
        }
    }

    /// True when a slider drag ended (save now).
    pub fn release(&mut self) -> bool {
        self.drag.take().is_some()
    }

    pub fn draw(&self, canvas: &mut Canvas<'_>, settings: &Settings, mouse: (f32, f32)) {
        if !self.active() {
            return;
        }
        canvas.overlay();
        let e = ease(self.shown);
        let (w, h) = (canvas.layout.width, canvas.layout.height);
        canvas.rect(0.0, 0.0, w, h, fade(0x5000_0000, e));
        let g = geometry(self.page, canvas.layout);
        let mut sheet = Panel::new(g.sheet[0], g.sheet[1], g.sheet[2], g.sheet[3]);
        sheet.radius = 14.0;
        sheet.shadow = 0.4;
        sheet.tint = [0.05, 0.06, 0.08, 0.46];
        sheet.opacity = e;
        canvas.glass(sheet);
        let res = canvas.res;
        let translate = |key: &str| res.translate(key).to_owned();
        let title = match self.page {
            Page::Settings => translate("options.title"),
            _ => translate("menu.game"),
        };
        canvas.system_text(
            &title,
            g.sheet[0] + PAD,
            g.sheet[1] + 10.0,
            TITLE,
            Weight::Semibold,
            fade(WHITE, e),
        );
        let video = &settings.video;
        for &(target, rect) in &g.targets {
            let hover = inside(rect, mouse);
            match target {
                Target::Resume => button(
                    canvas,
                    rect,
                    &translate("menu.returnToGame"),
                    hover,
                    true,
                    e,
                ),
                Target::Settings => {
                    button(canvas, rect, &translate("menu.options"), hover, false, e)
                }
                Target::Mods => button(canvas, rect, "Mods", hover, false, e),
                Target::Leave => {
                    let label = if self.leave_to_launcher {
                        translate("menu.disconnect")
                    } else {
                        translate("menu.quit")
                    };
                    button(canvas, rect, &label, hover, false, e);
                }
                Target::Done => button(canvas, rect, &translate("gui.done"), hover, true, e),
                Target::Slider(slider) => {
                    let label = slider.label(video, &translate);
                    row_label(canvas, g.sheet[0] + PAD, rect, &label, e);
                    let mut track = Panel::new(rect[0], rect[1] + 4.0, rect[2], rect[3] - 4.0);
                    track.radius = 3.0;
                    track.shadow = 0.0;
                    track.tint = [0.42, 0.44, 0.50, 0.70];
                    track.opacity = e;
                    canvas.glass(track);
                    let t = slider.get(video).clamp(0.0, 1.0);
                    let x = rect[0] + t * (rect[2] - rect[0]);
                    let mut fill =
                        Panel::new(rect[0], rect[1] + 4.0, x.max(rect[0] + 6.0), rect[3] - 4.0);
                    fill.radius = 3.0;
                    fill.shadow = 0.0;
                    fill.tint = [0.04, 0.47, 1.0, 0.9];
                    fill.opacity = e;
                    canvas.glass(fill);
                    let mut knob = Panel::new(x - 5.0, rect[1] - 1.0, x + 5.0, rect[3] + 1.0);
                    knob.radius = 5.0;
                    knob.shadow = 0.25;
                    knob.highlight = 0.2;
                    knob.tint = [1.0, 1.0, 1.0, 0.96];
                    knob.opacity = e;
                    canvas.glass(knob);
                }
                Target::GuiScale => {
                    row_label(
                        canvas,
                        g.sheet[0] + PAD,
                        rect,
                        &translate("options.guiScale"),
                        e,
                    );
                    let value = match video.gui_scale {
                        0 => translate("options.guiScale.auto"),
                        1 => translate("options.guiScale.small"),
                        2 => translate("options.guiScale.normal"),
                        3 => translate("options.guiScale.large"),
                        _ => "Huge".to_owned(),
                    };
                    button(canvas, rect, &value, hover, false, e);
                }
                Target::Bobbing | Target::Fullscreen => {
                    let (key, on) = if target == Target::Bobbing {
                        ("options.viewBobbing", video.view_bobbing)
                    } else {
                        ("options.fullscreen", video.fullscreen)
                    };
                    row_label(canvas, g.sheet[0] + PAD, rect, &translate(key), e);
                    switch(canvas, rect[0], rect[1], if on { 1.0 } else { 0.0 }, e);
                }
            }
        }
    }
}

fn row_label(canvas: &mut Canvas<'_>, x: f32, rect: [f32; 4], label: &str, opacity: f32) {
    let y = (rect[1] + rect[3]) / 2.0 - BODY / 2.0;
    canvas.system_text(label, x, y, BODY, Weight::Regular, fade(LABEL, opacity));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn centre(r: [f32; 4]) -> (f32, f32) {
        ((r[0] + r[2]) / 2.0, (r[1] + r[3]) / 2.0)
    }

    fn rect(page: Page, layout: Layout, target: Target) -> [f32; 4] {
        geometry(page, layout)
            .targets
            .into_iter()
            .find(|(t, _)| *t == target)
            .unwrap()
            .1
    }

    #[test]
    fn escape_walks_back_and_buttons_route() {
        let layout = Layout::new((1440, 900), 1.0, 2);
        let mut settings = Settings::default();
        let mut pause = Pause::default();
        assert!(!pause.active());
        pause.open();
        let settings_button = rect(Page::Menu, layout, Target::Settings);
        assert_eq!(
            pause.press(layout, centre(settings_button), &mut settings),
            Click::Nothing
        );
        assert_eq!(pause.page, Page::Settings);
        pause.back();
        assert_eq!(pause.page, Page::Menu);
        let leave = rect(Page::Menu, layout, Target::Leave);
        assert_eq!(
            pause.press(layout, centre(leave), &mut settings),
            Click::Leave
        );
        assert!(!pause.active());
        pause.open();
        pause.back();
        assert!(!pause.active());
    }

    #[test]
    fn sliders_and_switches_change_video_settings() {
        let layout = Layout::new((1440, 900), 1.0, 2);
        let mut settings = Settings::default();
        let mut pause = Pause {
            page: Page::Settings,
            ..Pause::default()
        };
        let fov = rect(Page::Settings, layout, Target::Slider(Slider::Fov));
        // Dragging to the far right is Quake Pro (110), the left end 30.
        assert_eq!(
            pause.press(layout, (fov[0] + 1.0, fov[1] + 2.0), &mut settings),
            Click::Changed
        );
        assert_eq!(settings.video.fov, 31.0);
        assert_eq!(
            pause.moved(layout, (fov[2] + 40.0, fov[1]), &mut settings),
            Click::Changed
        );
        assert_eq!(settings.video.fov, 110.0);
        assert!(pause.release());
        assert_eq!(
            pause.moved(layout, (fov[0], fov[1]), &mut settings),
            Click::Nothing
        );
        let distance = rect(Page::Settings, layout, Target::Slider(Slider::Distance));
        pause.press(layout, (distance[0], distance[1] + 2.0), &mut settings);
        assert_eq!(settings.video.render_distance, 2);
        pause.release();
        let bobbing = rect(Page::Settings, layout, Target::Bobbing);
        pause.press(layout, centre(bobbing), &mut settings);
        assert!(!settings.video.view_bobbing);
        let scale = rect(Page::Settings, layout, Target::GuiScale);
        for expected in [1, 2, 3, 4, 0] {
            pause.press(layout, centre(scale), &mut settings);
            assert_eq!(settings.video.gui_scale, expected);
        }
        let words = |key: &str| key.to_owned();
        let mut video = Video::default();
        assert_eq!(
            Slider::Fov.label(&video, &words),
            "options.fov: options.fov.min"
        );
        assert_eq!(
            Slider::Sensitivity.label(&video, &words),
            "options.sensitivity: 100%"
        );
        video.sensitivity = 1.0;
        assert_eq!(
            Slider::Sensitivity.label(&video, &words),
            "options.sensitivity: options.sensitivity.max"
        );
    }

    #[test]
    fn sheets_fit_small_and_large_windows() {
        for size in [(640, 480), (1440, 900), (2880, 1864)] {
            let layout = Layout::new(size, 1.0, 0);
            for page in [Page::Menu, Page::Settings] {
                let g = geometry(page, layout);
                assert!(g.sheet[2] <= layout.width + 0.5 && g.sheet[3] <= layout.height + 0.5);
                for (_, r) in &g.targets {
                    assert!(r[0] >= g.sheet[0] && r[2] <= g.sheet[2] && r[3] <= g.sheet[3]);
                }
            }
        }
    }
}
