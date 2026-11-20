//! Connection status and deliberate retry actions. No automatic reconnects.
use crate::{
    hud::button_widget,
    ui::{Canvas, Layout},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    Reconnect,
    Quit,
}
#[derive(Default)]
enum Phase {
    #[default]
    Inactive,
    Connecting,
    Disconnected(String),
}
#[derive(Default)]
pub struct Screen {
    phase: Phase,
}
impl Screen {
    pub fn new(connecting: bool) -> Self {
        Self {
            phase: if connecting {
                Phase::Connecting
            } else {
                Phase::Inactive
            },
        }
    }
    pub fn active(&self) -> bool {
        !matches!(self.phase, Phase::Inactive)
    }
    pub fn disconnected(&self) -> bool {
        matches!(self.phase, Phase::Disconnected(_))
    }
    pub fn connecting(&mut self) {
        self.phase = Phase::Connecting;
    }
    pub fn connected(&mut self) {
        if matches!(self.phase, Phase::Connecting) {
            self.phase = Phase::Inactive;
        }
    }
    pub fn lost(&mut self, reason: String) {
        self.phase = Phase::Disconnected(reason);
    }
    fn buttons(layout: Layout) -> [(Action, [f32; 4]); 2] {
        let width = 200.0f32.min((layout.width - 20.0).max(0.0));
        let x = ((layout.width - width) / 2.0).floor();
        [
            (Action::Reconnect, [x, layout.height - 56.0, width, 20.0]),
            (Action::Quit, [x, layout.height - 30.0, width, 20.0]),
        ]
    }
    pub fn action_at(&self, layout: Layout, mouse: (f32, f32)) -> Option<Action> {
        if !self.active() {
            return None;
        }
        Self::buttons(layout)
            .into_iter()
            .find(|(action, [x, y, w, h])| {
                (*action == Action::Quit || self.disconnected())
                    && mouse.0 >= *x
                    && mouse.0 < x + w
                    && mouse.1 >= *y
                    && mouse.1 < y + h
            })
            .map(|(action, _)| action)
    }
    /// A fresh key press while the screen is shown.
    pub fn key_action(&self, key: winit::keyboard::KeyCode, command: bool) -> Option<Action> {
        use winit::keyboard::KeyCode;
        match key {
            // GuiDisconnected and GuiConnecting ignore typed keys, Escape
            // included; only Command-Q and Enter act here.
            KeyCode::KeyQ if command => Some(Action::Quit),
            KeyCode::Enter | KeyCode::NumpadEnter if self.disconnected() => Some(Action::Reconnect),
            _ => None,
        }
    }
    fn reason(&self, translate: &dyn Fn(&str) -> Option<String>) -> String {
        match &self.phase {
            Phase::Disconnected(reason) => {
                horizon_protocol_47::text::format(reason, translate).text
            }
            _ => String::new(),
        }
    }
    pub fn draw(&self, canvas: &mut Canvas<'_>, mouse: (f32, f32)) {
        if !self.active() {
            return;
        }
        let (w, h) = (canvas.layout.width, canvas.layout.height);
        canvas.gradient(0.0, 0.0, w, h, 0xFF20_2028, 0xFF08_0810);
        let key = if self.disconnected() {
            "horizon.connection.lost"
        } else {
            "connect.connecting"
        };
        let title = canvas.res.translate(key).to_owned();
        canvas.centered(&title, w / 2.0, 24.0, 0xFFFF_FFFF, true);
        let reason = self.reason(&|key| canvas.res.language.get(key).cloned());
        let lines = canvas.res.wrap(&reason, &[], (w - 30.0).max(1.0) as i32);
        let room = ((h - 110.0) / 10.0).max(1.0) as usize;
        for (index, (line, _)) in lines.iter().take(room).enumerate() {
            canvas.centered(line, w / 2.0, 48.0 + index as f32 * 10.0, 0xFFCC_CCCC, true);
        }
        for (action, rect) in Self::buttons(canvas.layout) {
            let key = match action {
                Action::Reconnect => "horizon.connection.reconnect",
                Action::Quit => "horizon.connection.quit",
            };
            let label = canvas.res.translate(key).to_owned();
            button_widget(
                canvas,
                &label,
                rect,
                action == Action::Quit || self.disconnected(),
                self.action_at(canvas.layout, mouse) == Some(action),
            );
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn retry_is_a_deliberate_action_and_loading_cannot_retry() {
        let layout = Layout::new((960, 600), 1.0, 0);
        let [(_, r), (_, q)] = Screen::buttons(layout);
        let mut s = Screen::new(true);
        assert_eq!(s.action_at(layout, (r[0] + 1.0, r[1] + 1.0)), None);
        assert_eq!(
            s.action_at(layout, (q[0] + 1.0, q[1] + 1.0)),
            Some(Action::Quit)
        );
        s.lost("Connection refused".into());
        assert_eq!(
            s.action_at(layout, (r[0] + 1.0, r[1] + 1.0)),
            Some(Action::Reconnect)
        );
        s.connected();
        assert!(
            s.disconnected(),
            "stale positioned state cannot dismiss a failure"
        );
        s.connecting();
        s.connected();
        assert!(!s.active());
    }
    #[test]
    fn escape_neither_quits_nor_retries_like_gui_disconnected() {
        use winit::keyboard::KeyCode;
        let mut s = Screen::new(true);
        for disconnected in [false, true] {
            if disconnected {
                s.lost("Kicked".into());
            }
            // GuiDisconnected and GuiConnecting ignore typed keys: an Escape
            // meant for a menu at the moment of a kick must not end the app.
            assert_eq!(s.key_action(KeyCode::Escape, false), None);
            assert_eq!(s.key_action(KeyCode::KeyQ, true), Some(Action::Quit));
            assert_eq!(s.key_action(KeyCode::KeyQ, false), None);
        }
        assert_eq!(s.key_action(KeyCode::Enter, false), Some(Action::Reconnect));
        s.connecting();
        assert_eq!(s.key_action(KeyCode::NumpadEnter, false), None);
    }
    #[test]
    fn reason_displays_components_and_plain_transport_errors() {
        let mut s = Screen::default();
        s.lost(r#"{"text":"Match ended", "color":"red"}"#.into());
        assert_eq!(s.reason(&|_| None), "§cMatch ended§r");
        s.lost("Connection refused (os error 61)".into());
        assert_eq!(s.reason(&|_| None), "Connection refused (os error 61)§r");
    }
    #[test]
    fn buttons_remain_inside_small_and_retina_layouts() {
        for (size, scale) in [((320, 240), 1.0), ((960, 600), 1.0), ((1920, 1200), 2.0)] {
            let layout = Layout::new(size, scale, 0);
            for (_, [x, y, w, h]) in Screen::buttons(layout) {
                assert!(x >= 0.0 && y >= 0.0 && x + w <= layout.width && y + h <= layout.height);
            }
        }
    }
}
