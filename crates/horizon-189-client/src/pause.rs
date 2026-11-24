//! The game menu Escape opens in a world (`GuiIngameMenu`) and the video
//! and controls settings behind it: FOV, sensitivity and render distance
//! sliders, GUI scale, view bobbing and fullscreen. Settings apply as they
//! change and are saved by the caller.
//!
//! Two looks, switched by the "Vanilla Menu" setting: liquid-glass sheets,
//! or vanilla's own screens from widgets.png. The vanilla game menu is
//! `GuiIngameMenu` (`axp`) button for button; Achievements, Statistics and
//! Open to LAN show disabled, as this client has none of those screens. Its
//! options page lays this client's settings out on `GuiOptions`' grid.
use crate::{
    glass::Panel,
    hud::button_widget,
    menu::{button, ease, fade, switch},
    settings::{Settings, Video},
    sysfont::Weight,
    ui::WIDGETS,
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
    /// Render scale: the world's resolution, 50 to 100%.
    Scale,
}

impl Slider {
    /// Position along the track, 0 to 1.
    fn get(self, video: &Video) -> f32 {
        match self {
            Slider::Fov => (video.fov - 30.0) / 80.0,
            Slider::Sensitivity => video.sensitivity,
            Slider::Distance => f32::from(video.render_distance - 2) / 14.0,
            Slider::Scale => f32::from(video.render_scale.saturating_sub(50)) / 50.0,
        }
    }

    fn set(self, video: &mut Video, t: f32) {
        let t = t.clamp(0.0, 1.0);
        match self {
            Slider::Fov => video.fov = (30.0 + t * 80.0).round(),
            Slider::Sensitivity => video.sensitivity = (t * 200.0).round() / 200.0,
            Slider::Distance => video.render_distance = (2.0 + t * 14.0).round() as u8,
            Slider::Scale => {
                video.render_scale = crate::upscale::percent(50.0 + f64::from(t) * 50.0);
            }
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
            Slider::Scale => format!("Render Scale: {}%", video.render_scale),
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
    /// Graphics: Fancy / Fast.
    Graphics,
    /// Glass or vanilla menus.
    MenuStyle,
    /// A vanilla button this client has nothing behind (translation key).
    Inert(&'static str),
    Done,
}

#[derive(Debug)]
struct Geometry {
    sheet: [f32; 4],
    targets: Vec<(Target, [f32; 4])>,
}

const BUTTON_H: f32 = 20.0;
const ROW_H: f32 = 22.0;
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

/// `GuiScreen` sizes are whole GUI pixels.
fn gui_size(layout: Layout) -> (i32, i32) {
    (layout.width as i32, layout.height as i32)
}

fn vanilla_geometry(page: Page, layout: Layout) -> Geometry {
    let (w, h) = gui_size(layout);
    let rect =
        |x: i32, y: i32, width: i32| [x as f32, y as f32, (x + width) as f32, (y + 20) as f32];
    let mut targets = Vec::new();
    match page {
        Page::Settings => {
            // GuiOptions: two columns of 150, 24 apart, from height/6 - 12.
            let rows = [
                Target::Slider(Slider::Fov),
                Target::Slider(Slider::Sensitivity),
                Target::Slider(Slider::Distance),
                Target::GuiScale,
                Target::Bobbing,
                Target::Fullscreen,
                Target::Graphics,
                Target::MenuStyle,
                Target::Slider(Slider::Scale),
            ];
            for (i, target) in rows.into_iter().enumerate() {
                let i = i as i32;
                targets.push((
                    target,
                    rect(w / 2 - 155 + i % 2 * 160, h / 6 - 12 + 24 * (i >> 1), 150),
                ));
            }
            targets.push((Target::Done, rect(w / 2 - 100, h / 6 + 168, 200)));
        }
        _ => {
            // GuiIngameMenu.initGui, with its -16 offset.
            let (left, right, top) = (w / 2 - 100, w / 2 + 2, h / 4 - 16);
            targets.push((Target::Resume, rect(left, top + 24, 200)));
            targets.push((Target::Inert("gui.achievements"), rect(left, top + 48, 98)));
            targets.push((Target::Inert("gui.stats"), rect(right, top + 48, 98)));
            targets.push((Target::Settings, rect(left, top + 96, 98)));
            targets.push((Target::Inert("menu.shareToLan"), rect(right, top + 96, 98)));
            targets.push((Target::Leave, rect(left, top + 120, 200)));
        }
    }
    Geometry {
        sheet: [0.0, 0.0, w as f32, h as f32],
        targets,
    }
}

fn geometry(page: Page, layout: Layout, vanilla: bool) -> Geometry {
    if vanilla {
        return vanilla_geometry(page, layout);
    }
    match page {
        Page::Settings => {
            let rows = [
                Target::Slider(Slider::Fov),
                Target::Slider(Slider::Sensitivity),
                Target::Slider(Slider::Distance),
                Target::GuiScale,
                Target::Bobbing,
                Target::Fullscreen,
                Target::Graphics,
                Target::MenuStyle,
                Target::Slider(Slider::Scale),
            ];
            let width = 300.0f32.min(layout.width - 8.0);
            // Rows tighten (never below the GUI-scale picker's 18) to fit short
            // windows; each control stays centred in its row.
            let fixed = 34.0 + 8.0 + BUTTON_H + PAD;
            let row_h = ROW_H
                .min((layout.height - 4.0 - fixed) / rows.len() as f32)
                .max(18.0);
            let inset = (ROW_H - row_h) / 2.0;
            let height = fixed + rows.len() as f32 * row_h;
            let sheet = centred(layout, width, height);
            let mut targets = Vec::new();
            for (index, target) in rows.into_iter().enumerate() {
                let y = sheet[1] + 34.0 + index as f32 * row_h;
                let y = y - inset;
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
        let g = geometry(self.page, layout, settings.toggles.vanilla_menu);
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
            Target::Graphics => {
                video.fancy_graphics = !video.fancy_graphics;
                Click::Changed
            }
            Target::MenuStyle => {
                settings.toggles.vanilla_menu = !settings.toggles.vanilla_menu;
                Click::Changed
            }
            Target::Inert(_) => Click::Nothing,
        }
    }

    /// Dragging a slider.
    pub fn moved(&mut self, layout: Layout, mouse: (f32, f32), settings: &mut Settings) -> Click {
        let Some(slider) = self.drag else {
            return Click::Nothing;
        };
        let vanilla = settings.toggles.vanilla_menu;
        let g = geometry(self.page, layout, vanilla);
        let Some(&(_, rect)) = g.targets.iter().find(|(t, _)| *t == Target::Slider(slider)) else {
            return Click::Nothing;
        };
        let before = settings.video;
        // GuiOptionSlider: the knob's 8 pixels ride inside the button.
        let t = if vanilla {
            (mouse.0 - (rect[0] + 4.0)) / (rect[2] - rect[0] - 8.0)
        } else {
            (mouse.0 - rect[0]) / (rect[2] - rect[0])
        };
        slider.set(&mut settings.video, t);
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
        if settings.toggles.vanilla_menu {
            self.draw_vanilla(canvas, settings, mouse);
            return;
        }
        let e = ease(self.shown);
        let (w, h) = (canvas.layout.width, canvas.layout.height);
        canvas.rect(0.0, 0.0, w, h, fade(0x5000_0000, e));
        let g = geometry(self.page, canvas.layout, false);
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
                Target::Bobbing | Target::Fullscreen | Target::Graphics | Target::MenuStyle => {
                    let (label, on) = match target {
                        Target::Bobbing => (translate("options.viewBobbing"), video.view_bobbing),
                        Target::Fullscreen => (translate("options.fullscreen"), video.fullscreen),
                        Target::Graphics => (
                            format!(
                                "{}: {}",
                                translate("options.graphics"),
                                translate(if video.fancy_graphics {
                                    "options.graphics.fancy"
                                } else {
                                    "options.graphics.fast"
                                })
                            ),
                            video.fancy_graphics,
                        ),
                        _ => ("Vanilla Menu".to_owned(), settings.toggles.vanilla_menu),
                    };
                    row_label(canvas, g.sheet[0] + PAD, rect, &label, e);
                    switch(canvas, rect[0], rect[1], if on { 1.0 } else { 0.0 }, e);
                }
                Target::Inert(_) => {}
            }
        }
    }

    /// `GuiIngameMenu` and `GuiOptions` as vanilla draws them: the dark
    /// default background, a white title, widgets.png buttons and sliders.
    fn draw_vanilla(&self, canvas: &mut Canvas<'_>, settings: &Settings, mouse: (f32, f32)) {
        let (w, h) = (canvas.layout.width, canvas.layout.height);
        canvas.gradient(0.0, 0.0, w, h, 0xC010_1010, 0xD010_1010);
        let res = canvas.res;
        let translate = |key: &str| res.translate(key).to_owned();
        let (title, title_y) = match self.page {
            Page::Settings => (translate("options.title"), 15.0),
            _ => (translate("menu.game"), 40.0),
        };
        let (gw, _) = gui_size(canvas.layout);
        canvas.centered(&title, (gw / 2) as f32, title_y, 0xFFFF_FFFF, true);
        let on_off = |on: bool| translate(if on { "options.on" } else { "options.off" });
        let video = &settings.video;
        let g = geometry(self.page, canvas.layout, true);
        for &(target, rect) in &g.targets {
            let hover = inside(rect, mouse);
            let size = [rect[0], rect[1], rect[2] - rect[0], rect[3] - rect[1]];
            let label = match target {
                Target::Resume => translate("menu.returnToGame"),
                Target::Settings => translate("menu.options"),
                Target::Mods => "Mods".to_owned(),
                Target::Leave if self.leave_to_launcher => translate("menu.disconnect"),
                Target::Leave => translate("menu.quit"),
                Target::Done => translate("gui.done"),
                Target::Inert(key) => translate(key),
                Target::GuiScale => {
                    let value = match video.gui_scale {
                        0 => translate("options.guiScale.auto"),
                        1 => translate("options.guiScale.small"),
                        2 => translate("options.guiScale.normal"),
                        3 => translate("options.guiScale.large"),
                        _ => "Huge".to_owned(),
                    };
                    format!("{}: {value}", translate("options.guiScale"))
                }
                Target::Bobbing => format!(
                    "{}: {}",
                    translate("options.viewBobbing"),
                    on_off(video.view_bobbing)
                ),
                Target::Fullscreen => format!(
                    "{}: {}",
                    translate("options.fullscreen"),
                    on_off(video.fullscreen)
                ),
                Target::Graphics => format!(
                    "{}: {}",
                    translate("options.graphics"),
                    translate(if video.fancy_graphics {
                        "options.graphics.fancy"
                    } else {
                        "options.graphics.fast"
                    })
                ),
                Target::MenuStyle => "Menu Style: Vanilla".to_owned(),
                Target::Slider(slider) => {
                    // GuiOptionSlider: the disabled texture behind, the knob,
                    // then the text.
                    let [x, y, width, height] = size;
                    let half = (width / 2.0).floor();
                    let white = [1.0; 4];
                    canvas.sprite(WIDGETS, x, y, 0.0, 46.0, half, height, white);
                    canvas.sprite(
                        WIDGETS,
                        x + half,
                        y,
                        200.0 - half,
                        46.0,
                        half,
                        height,
                        white,
                    );
                    let knob = x + (slider.get(video).clamp(0.0, 1.0) * (width - 8.0)).floor();
                    canvas.sprite(WIDGETS, knob, y, 0.0, 66.0, 4.0, 20.0, white);
                    canvas.sprite(WIDGETS, knob + 4.0, y, 196.0, 66.0, 4.0, 20.0, white);
                    let color = if hover { 0xFFFF_FFA0 } else { 0xFFE0_E0E0 };
                    let text = slider.label(video, &translate);
                    canvas.centered(&text, x + half, y + 6.0, color, true);
                    continue;
                }
            };
            let enabled = !matches!(target, Target::Inert(_));
            button_widget(canvas, &label, size, enabled, hover && enabled);
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
        geometry(page, layout, false)
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
        // Render scale: the left end is 50%, the middle 75%, steps of 5.
        let world = rect(Page::Settings, layout, Target::Slider(Slider::Scale));
        pause.press(layout, (world[0], world[1] + 2.0), &mut settings);
        assert_eq!(settings.video.render_scale, 50);
        pause.moved(
            layout,
            ((world[0] + world[2]) / 2.0, world[1]),
            &mut settings,
        );
        assert_eq!(settings.video.render_scale, 75);
        pause.release();
        assert_eq!(
            Slider::Scale.label(&settings.video, &|key: &str| key.to_owned()),
            "Render Scale: 75%"
        );
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
    fn the_vanilla_menu_is_gui_ingame_menu_and_switches_back() {
        // 1920x1080 at GUI scale 4: a 480 x 270 screen.
        let layout = Layout::new((1920, 1080), 1.0, 4);
        let mut settings = Settings::default();
        settings.toggles.vanilla_menu = true;
        let g = geometry(Page::Menu, layout, true);
        let at = |target| g.targets.iter().find(|(t, _)| *t == target).unwrap().1;
        // height/4 - 16 = 51; width/2 - 100 = 140.
        assert_eq!(at(Target::Resume), [140.0, 75.0, 340.0, 95.0]);
        assert_eq!(
            at(Target::Inert("gui.achievements")),
            [140.0, 99.0, 238.0, 119.0]
        );
        assert_eq!(at(Target::Inert("gui.stats")), [242.0, 99.0, 340.0, 119.0]);
        assert_eq!(at(Target::Settings), [140.0, 147.0, 238.0, 167.0]);
        assert_eq!(
            at(Target::Inert("menu.shareToLan")),
            [242.0, 147.0, 340.0, 167.0]
        );
        assert_eq!(at(Target::Leave), [140.0, 171.0, 340.0, 191.0]);
        let mut pause = Pause::default();
        pause.open();
        let lan = at(Target::Inert("menu.shareToLan"));
        assert_eq!(
            pause.press(layout, centre(lan), &mut settings),
            Click::Nothing
        );
        assert!(pause.active(), "disabled buttons do nothing");
        pause.press(layout, centre(at(Target::Settings)), &mut settings);
        assert_eq!(pause.page, Page::Settings);
        // GuiOptions' grid: the 7th row starts the 4th line, left column.
        let options = geometry(Page::Settings, layout, true);
        let row = |target| {
            options
                .targets
                .iter()
                .find(|(t, _)| *t == target)
                .unwrap()
                .1
        };
        assert_eq!(row(Target::Slider(Slider::Fov)), [85.0, 33.0, 235.0, 53.0]);
        assert_eq!(
            row(Target::Slider(Slider::Sensitivity)),
            [245.0, 33.0, 395.0, 53.0]
        );
        assert_eq!(row(Target::Graphics), [85.0, 105.0, 235.0, 125.0]);
        assert_eq!(row(Target::MenuStyle), [245.0, 105.0, 395.0, 125.0]);
        // Graphics flips Fancy and Fast.
        assert!(settings.video.fancy_graphics);
        assert_eq!(
            pause.press(layout, centre(row(Target::Graphics)), &mut settings),
            Click::Changed
        );
        assert!(!settings.video.fancy_graphics);
        assert_eq!(row(Target::Done), [140.0, 213.0, 340.0, 233.0]);
        // The vanilla slider's knob rides 4 pixels in from each end.
        let fov = row(Target::Slider(Slider::Fov));
        pause.press(layout, (fov[0] + 4.0, fov[1] + 5.0), &mut settings);
        assert_eq!(settings.video.fov, 30.0);
        pause.moved(layout, (fov[2] - 4.0, fov[1]), &mut settings);
        assert_eq!(settings.video.fov, 110.0);
        pause.release();
        // Menu Style switches back to the glass sheets, which can switch again.
        assert_eq!(
            pause.press(layout, centre(row(Target::MenuStyle)), &mut settings),
            Click::Changed
        );
        assert!(!settings.toggles.vanilla_menu);
        let glass = rect(Page::Settings, layout, Target::MenuStyle);
        pause.press(layout, centre(glass), &mut settings);
        assert!(settings.toggles.vanilla_menu);
    }

    #[test]
    fn sheets_fit_small_and_large_windows() {
        for size in [(640, 480), (1440, 900), (2880, 1864)] {
            let layout = Layout::new(size, 1.0, 0);
            for page in [Page::Menu, Page::Settings] {
                let g = geometry(page, layout, false);
                assert!(g.sheet[2] <= layout.width + 0.5 && g.sheet[3] <= layout.height + 0.5);
                for (_, r) in &g.targets {
                    assert!(r[0] >= g.sheet[0] && r[2] <= g.sheet[2] && r[3] <= g.sheet[3]);
                }
            }
        }
    }
}
