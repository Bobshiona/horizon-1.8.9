//! `GuiNewChat` and `GuiChat`: wrapped, fading chat lines, scrolling, and the
//! input line with history, tab completion and clickable components. Text
//! editing follows `GuiTextField` with macOS conventions: Option moves by
//! word, Command by line.
use crate::ui::{Canvas, Resources};
use horizon_protocol_47::{
    chat::MAX_MESSAGE,
    text::{Click, Events, Formatted, strip},
};
use std::{collections::VecDeque, sync::Arc};
use winit::keyboard::{KeyCode, ModifiersState};

/// `getChatWidth` at the default chat width of 1.0.
const WIDTH: i32 = 320;
const MAX_LINES: usize = 100;
const MAX_HISTORY: usize = 100;
/// `getLineCount`: unfocused (height 90) and focused (height 180) lines.
const UNFOCUSED_LINES: usize = 10;
const FOCUSED_LINES: usize = 20;

struct Line {
    text: String,
    tags: Vec<u16>,
    events: Arc<Vec<Events>>,
    added: f64,
}

/// What the window should do after a chat key or click.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Request {
    None,
    /// The screen closed (Escape or after sending).
    Closed,
    /// Send as C01, then close if `close`.
    Send {
        text: String,
        close: bool,
    },
    /// Ask the server to complete the text left of the cursor.
    Complete(String),
    /// Put this on the clipboard.
    Copy(String),
}

#[derive(Default)]
pub struct Chat {
    lines: VecDeque<Line>,
    sent: Vec<String>,
    pub open: bool,
    opened_at: f64,
    text: Vec<char>,
    cursor: usize,
    anchor: usize,
    offset: usize,
    history: usize,
    buffer: String,
    scroll: usize,
    scrolled: bool,
    waiting: bool,
    found: Vec<String>,
    found_index: usize,
    names_found: bool,
}

fn allowed(c: char) -> bool {
    c != '§' && c >= ' ' && c != '\u{7f}'
}

impl Chat {
    /// `setChatLine`: wrap at the chat width, newest first, keep 100 lines.
    pub fn receive(&mut self, res: &Resources, message: &Formatted, tick: f64) {
        eprintln!("[CHAT] {}", strip(&message.text));
        let events = Arc::new(message.events.clone());
        for (text, tags) in res.wrap(&message.text, &message.tags, WIDTH) {
            if self.open && self.scroll > 0 {
                self.scrolled = true;
                self.scroll_by(1);
            }
            self.lines.push_front(Line {
                text,
                tags,
                events: events.clone(),
                added: tick,
            });
        }
        self.lines.truncate(MAX_LINES);
    }

    /// A client-side line, such as a copied-link notice.
    pub fn notice(&mut self, res: &Resources, text: &str, tick: f64) {
        self.receive(res, &Formatted::plain(text), tick);
    }

    /// `GuiChat(defaultText)`.
    pub fn open_with(&mut self, text: &str, tick: f64) {
        self.open = true;
        self.opened_at = tick;
        self.set_text(text);
        self.history = self.sent.len();
        self.buffer.clear();
        self.waiting = false;
        self.names_found = false;
    }

    /// `GuiChat.onGuiClosed`: `resetScroll`.
    pub fn close(&mut self) {
        self.open = false;
        self.text.clear();
        self.cursor = 0;
        self.anchor = 0;
        self.offset = 0;
        self.scroll = 0;
        self.scrolled = false;
    }

    fn line_count(&self) -> usize {
        if self.open {
            FOCUSED_LINES
        } else {
            UNFOCUSED_LINES
        }
    }

    /// `GuiNewChat.scroll`.
    pub fn scroll_by(&mut self, amount: i32) {
        let max = self.lines.len().saturating_sub(self.line_count());
        let next = (self.scroll as i64 + i64::from(amount)).clamp(0, max as i64) as usize;
        self.scroll = next;
        if next == 0 {
            self.scrolled = false;
        }
    }

    /// `GuiChat.handleMouseInput`: a wheel notch scrolls seven lines, one
    /// with Shift.
    pub fn wheel(&mut self, notches: f64, shift: bool) {
        if notches == 0.0 {
            return;
        }
        let step = if notches > 0.0 { 1 } else { -1 };
        self.scroll_by(if shift { step } else { step * 7 });
    }

    fn set_text(&mut self, text: &str) {
        self.text = text
            .chars()
            .filter(|&c| allowed(c))
            .take(MAX_MESSAGE)
            .collect();
        self.cursor = self.text.len();
        self.anchor = self.cursor;
    }

    pub fn text(&self) -> String {
        self.text.iter().collect()
    }

    fn selection(&self) -> (usize, usize) {
        (self.cursor.min(self.anchor), self.cursor.max(self.anchor))
    }

    pub fn selected_text(&self) -> String {
        let (start, end) = self.selection();
        self.text[start..end].iter().collect()
    }

    pub fn select_all(&mut self) {
        self.anchor = 0;
        self.cursor = self.text.len();
    }

    /// `GuiTextField.writeText`: filtered, replacing the selection, capped.
    pub fn insert(&mut self, text: &str) {
        let (start, end) = self.selection();
        let room = MAX_MESSAGE - (self.text.len() - (end - start));
        let typed: Vec<char> = text.chars().filter(|&c| allowed(c)).take(room).collect();
        let count = typed.len();
        self.text.splice(start..end, typed);
        self.cursor = start + count;
        self.anchor = self.cursor;
    }

    fn word_left(&self, mut i: usize) -> usize {
        while i > 0 && self.text[i - 1] == ' ' {
            i -= 1;
        }
        while i > 0 && self.text[i - 1] != ' ' {
            i -= 1;
        }
        i
    }

    fn word_right(&self, mut i: usize) -> usize {
        let len = self.text.len();
        while i < len && self.text[i] != ' ' {
            i += 1;
        }
        while i < len && self.text[i] == ' ' {
            i += 1;
        }
        i
    }

    /// Delete the selection, or from the cursor to `to`.
    fn delete_to(&mut self, to: usize) {
        let (start, end) = if self.cursor != self.anchor {
            self.selection()
        } else {
            (self.cursor.min(to), self.cursor.max(to))
        };
        self.text.drain(start..end);
        self.cursor = start;
        self.anchor = start;
    }

    fn move_to(&mut self, to: usize, select: bool) {
        self.cursor = to.min(self.text.len());
        if !select {
            self.anchor = self.cursor;
        }
    }

    /// `GuiChat.getSentHistory`.
    fn recall(&mut self, step: i32) {
        let len = self.sent.len();
        let next = (self.history as i64 + i64::from(step)).clamp(0, len as i64) as usize;
        if next == self.history {
            return;
        }
        if next == len {
            self.history = len;
            let buffer = std::mem::take(&mut self.buffer);
            self.set_text(&buffer);
        } else {
            if self.history == len {
                self.buffer = self.text();
            }
            let recalled = self.sent[next].clone();
            self.set_text(&recalled);
            self.history = next;
        }
    }

    fn remember(&mut self, text: &str) {
        if self.sent.last().is_none_or(|last| last != text) {
            self.sent.push(text.to_owned());
            if self.sent.len() > MAX_HISTORY {
                self.sent.remove(0);
            }
        }
    }

    /// One key while the chat screen is open.
    pub fn key(&mut self, key: KeyCode, typed: Option<&str>, modifiers: ModifiersState) -> Request {
        let shift = modifiers.shift_key();
        let word = modifiers.alt_key();
        let line = modifiers.super_key();
        let tab = key == KeyCode::Tab;
        self.waiting = false;
        if !tab {
            self.names_found = false;
        }
        match key {
            KeyCode::Escape => {
                self.close();
                return Request::Closed;
            }
            KeyCode::Enter | KeyCode::NumpadEnter => {
                let text = self.text().trim().to_owned();
                self.close();
                if text.is_empty() {
                    return Request::Closed;
                }
                self.remember(&text);
                return Request::Send { text, close: true };
            }
            KeyCode::Tab => return self.complete(),
            KeyCode::ArrowUp => self.recall(-1),
            KeyCode::ArrowDown => self.recall(1),
            KeyCode::PageUp => self.scroll_by(FOCUSED_LINES as i32 - 1),
            KeyCode::PageDown => self.scroll_by(-(FOCUSED_LINES as i32 - 1)),
            KeyCode::Backspace => {
                let to = if line {
                    0
                } else if word {
                    self.word_left(self.cursor)
                } else {
                    self.cursor.saturating_sub(1)
                };
                self.delete_to(to);
            }
            KeyCode::Delete => {
                let to = if line {
                    self.text.len()
                } else if word {
                    self.word_right(self.cursor)
                } else {
                    (self.cursor + 1).min(self.text.len())
                };
                self.delete_to(to);
            }
            KeyCode::ArrowLeft => {
                let to = if line {
                    0
                } else if word {
                    self.word_left(self.cursor)
                } else if !shift && self.cursor != self.anchor {
                    self.selection().0
                } else {
                    self.cursor.saturating_sub(1)
                };
                self.move_to(to, shift);
            }
            KeyCode::ArrowRight => {
                let to = if line {
                    self.text.len()
                } else if word {
                    self.word_right(self.cursor)
                } else if !shift && self.cursor != self.anchor {
                    self.selection().1
                } else {
                    self.cursor + 1
                };
                self.move_to(to, shift);
            }
            KeyCode::Home => self.move_to(0, shift),
            KeyCode::End => self.move_to(self.text.len(), shift),
            _ => {
                if !modifiers.super_key()
                    && !modifiers.control_key()
                    && let Some(typed) = typed
                {
                    self.insert(typed);
                }
            }
        }
        Request::None
    }

    /// `GuiChat.autocompletePlayerNames`: the first Tab asks the server;
    /// further Tabs cycle the matches.
    fn complete(&mut self) -> Request {
        if self.names_found {
            let start = self.word_left_no_skip(self.cursor);
            self.delete_to(start);
            if self.found_index >= self.found.len() {
                self.found_index = 0;
            }
        } else {
            self.found.clear();
            self.found_index = 0;
            self.waiting = true;
            return Request::Complete(self.text[..self.cursor].iter().collect());
        }
        if let Some(name) = self.found.get(self.found_index).cloned() {
            self.found_index += 1;
            self.insert(&strip(&name));
        }
        Request::None
    }

    /// `func_146197_a(-1, cursor, false)`: start of the word at the cursor.
    fn word_left_no_skip(&self, mut i: usize) -> usize {
        while i > 0 && self.text[i - 1] != ' ' {
            i -= 1;
        }
        i
    }

    /// `GuiChat.onAutocompleteResponse`; returns a list line to print when
    /// there are several matches.
    pub fn completions(&mut self, matches: Vec<String>) -> Option<String> {
        if !self.waiting || !self.open {
            return None;
        }
        self.waiting = false;
        self.found = matches.into_iter().filter(|m| !m.is_empty()).collect();
        let start = self.word_left_no_skip(self.cursor);
        let current: String = self.text[start..self.cursor].iter().collect();
        let prefix = common_prefix(&self.found);
        if !prefix.is_empty() && !current.eq_ignore_ascii_case(&prefix) {
            self.delete_to(start);
            self.insert(&prefix);
            return None;
        }
        if self.found.is_empty() {
            return None;
        }
        self.names_found = true;
        let listing = (self.found.len() > 1).then(|| self.found.join(", "));
        let start = self.word_left_no_skip(self.cursor);
        self.delete_to(start);
        if let Some(name) = self.found.first().cloned() {
            self.found_index = 1;
            self.insert(&strip(&name));
        }
        listing
    }

    /// `GuiNewChat.getChatComponent`: the events under the mouse.
    pub fn events_at(&self, res: &Resources, height: f32, mouse: (f32, f32)) -> Option<&Events> {
        if !self.open {
            return None;
        }
        let x = mouse.0 - 3.0;
        let y = height - mouse.1 - 27.0;
        if x < 0.0 || y < 0.0 || x > WIDTH as f32 {
            return None;
        }
        let shown = self.line_count().min(self.lines.len());
        if y >= (9 * shown + shown) as f32 {
            return None;
        }
        let line = self.lines.get(y as usize / 9 + self.scroll)?;
        let mut width = 0;
        let mut chars = line.text.chars().zip(line.tags.iter().copied());
        while let Some((c, tag)) = chars.next() {
            if c == '§' {
                chars.next();
                continue;
            }
            width += res.char_width(c).max(0);
            if width as f32 > x {
                return line.events.get(usize::from(tag).checked_sub(1)?);
            }
        }
        None
    }

    /// `GuiScreen.handleComponentClick` for a left click.
    pub fn click(&mut self, res: &Resources, height: f32, mouse: (f32, f32)) -> Request {
        let Some(click) = self
            .events_at(res, height, mouse)
            .and_then(|e| e.click.clone())
        else {
            self.click_input(res, mouse);
            return Request::None;
        };
        match click {
            Click::RunCommand(command) => Request::Send {
                text: command,
                close: false,
            },
            Click::SuggestCommand(text) => {
                self.set_text(&text);
                Request::None
            }
            Click::OpenUrl(url) => Request::Copy(url),
        }
    }

    /// `GuiTextField.mouseClicked`: place the cursor in the input line.
    fn click_input(&mut self, res: &Resources, mouse: (f32, f32)) {
        let visible: String = self.text[self.offset.min(self.text.len())..]
            .iter()
            .collect();
        let x = (mouse.0 - 4.0).max(0.0) as i32;
        let fitted = res.trim(&visible, x).chars().count();
        self.move_to(self.offset + fitted, false);
    }

    pub fn draw(&mut self, canvas: &mut Canvas<'_>, tick: f64, mouse: (f32, f32)) {
        let (w, h) = (canvas.layout.width, canvas.layout.height);
        let base = h - 28.0;
        let visible = self.line_count();
        let mut drawn = 0;
        for (index, line) in self
            .lines
            .iter()
            .skip(self.scroll)
            .take(visible)
            .enumerate()
        {
            let age = tick - line.added;
            if age >= 200.0 && !self.open {
                continue;
            }
            let fade = ((1.0 - age / 200.0) * 10.0).clamp(0.0, 1.0);
            let alpha = if self.open {
                255
            } else {
                (255.0 * fade * fade) as u32
            };
            drawn += 1;
            if alpha > 3 {
                let y = base - index as f32 * 9.0;
                canvas.rect(2.0, y - 9.0, 2.0 + WIDTH as f32 + 4.0, y, (alpha / 2) << 24);
                canvas.text(&line.text, 2.0, y - 8.0, 0x00FF_FFFF | alpha << 24, true);
            }
        }
        if self.open {
            let total = self.lines.len();
            let full = (total * 9 + total) as f32;
            let shown = (drawn * 9 + drawn) as f32;
            if total > 0 && full != shown {
                let offset = self.scroll as f32 * shown / total as f32;
                let length = shown * shown / full;
                let alpha: u32 = if offset > 0.0 { 170 } else { 96 };
                let color = if self.scrolled {
                    0x00CC_3333
                } else {
                    0x0033_33AA
                };
                canvas.rect(
                    -1.0,
                    base - offset,
                    1.0,
                    base - offset - length,
                    color | alpha << 24,
                );
                canvas.rect(
                    1.0,
                    base - offset,
                    0.0,
                    base - offset - length,
                    0x00CC_CCCC | alpha << 24,
                );
            }
            self.draw_input(canvas, tick, w, h);
            if let Some(hover) = self
                .events_at(canvas.res, h, mouse)
                .and_then(|events| events.hover.clone())
            {
                let lines: Vec<String> = hover.split('\n').map(str::to_owned).collect();
                canvas.tooltip(&lines, mouse);
            }
        }
    }

    /// `GuiChat.drawScreen` and `GuiTextField.drawTextBox` without a border.
    fn draw_input(&mut self, canvas: &mut Canvas<'_>, tick: f64, w: f32, h: f32) {
        canvas.rect(2.0, h - 14.0, w - 2.0, h - 2.0, 0x8000_0000);
        let field = (w - 4.0) as i32;
        if self.cursor < self.offset {
            self.offset = self.cursor;
        }
        while self.offset < self.cursor
            && canvas.res.width(
                &self.text[self.offset..self.cursor]
                    .iter()
                    .collect::<String>(),
            ) > field
        {
            self.offset += 1;
        }
        let tail: String = self.text[self.offset..].iter().collect();
        let shown = canvas.res.trim(&tail, field);
        let shown: Vec<char> = shown.chars().collect();
        let caret = self.cursor - self.offset;
        let (x, y) = (4.0, h - 12.0);
        let color = 0xFFE0_E0E0;
        let before: String = shown[..caret.min(shown.len())].iter().collect();
        let mut end = if before.is_empty() {
            x
        } else {
            canvas.text(&before, x, y, color, true)
        };
        let inside = self.cursor < self.text.len() || self.text.len() >= MAX_MESSAGE;
        let mut caret_x = end;
        if inside {
            caret_x = end - 1.0;
            end -= 1.0;
        }
        if caret < shown.len() {
            let after: String = shown[caret..].iter().collect();
            canvas.text(&after, end, y, color, true);
        }
        let blink = ((tick - self.opened_at).max(0.0) as u64 / 6).is_multiple_of(2);
        if blink {
            if inside {
                canvas.rect(caret_x, y - 1.0, caret_x + 1.0, y + 10.0, 0xFFD0_D0D0);
            } else {
                canvas.text("_", caret_x, y, color, true);
            }
        }
        if self.cursor != self.anchor {
            let (start, stop) = self.selection();
            let from = start.saturating_sub(self.offset).min(shown.len());
            let to = stop.saturating_sub(self.offset).min(shown.len());
            let left = x + canvas.res.width(&shown[..from].iter().collect::<String>()) as f32;
            let right = x + canvas.res.width(&shown[..to].iter().collect::<String>()) as f32;
            canvas.rect(left - 1.0, y - 1.0, right, y + 10.0, 0x8000_00FF);
        }
    }
}

/// `StringUtils.getCommonPrefix`.
fn common_prefix(items: &[String]) -> String {
    let Some(first) = items.first() else {
        return String::new();
    };
    let mut prefix: Vec<char> = first.chars().collect();
    for item in &items[1..] {
        let shared = prefix
            .iter()
            .zip(item.chars())
            .take_while(|(a, b)| **a == *b)
            .count();
        prefix.truncate(shared);
    }
    prefix.into_iter().collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn typing(chat: &mut Chat, text: &str) {
        for c in text.chars() {
            chat.key(KeyCode::KeyA, Some(&c.to_string()), ModifiersState::empty());
        }
    }

    #[test]
    fn editing_follows_text_field_rules_with_mac_shortcuts() {
        let mut chat = Chat::default();
        chat.open_with("/", 0.0);
        typing(&mut chat, "party §invite Bob");
        assert_eq!(chat.text(), "/party invite Bob");
        chat.key(KeyCode::Backspace, None, ModifiersState::ALT);
        assert_eq!(chat.text(), "/party invite ");
        chat.key(KeyCode::ArrowLeft, None, ModifiersState::SUPER);
        typing(&mut chat, ">");
        assert_eq!(chat.text(), ">/party invite ");
        chat.select_all();
        chat.insert("hi");
        assert_eq!(chat.text(), "hi");
        chat.insert(&"x".repeat(200));
        assert_eq!(chat.text().chars().count(), MAX_MESSAGE);
        // Command shortcuts never type their letter.
        chat.key(KeyCode::KeyV, Some("v"), ModifiersState::SUPER);
        assert_eq!(chat.text().chars().count(), MAX_MESSAGE);
    }

    #[test]
    fn sending_trims_remembers_and_recalls_history() {
        let mut chat = Chat::default();
        chat.open_with("", 0.0);
        typing(&mut chat, "  /l  ");
        assert_eq!(
            chat.key(KeyCode::Enter, None, ModifiersState::empty()),
            Request::Send {
                text: "/l".into(),
                close: true
            }
        );
        assert!(!chat.open);
        chat.open_with("", 0.0);
        typing(&mut chat, "draft");
        chat.key(KeyCode::ArrowUp, None, ModifiersState::empty());
        assert_eq!(chat.text(), "/l");
        chat.key(KeyCode::ArrowDown, None, ModifiersState::empty());
        assert_eq!(chat.text(), "draft");
        chat.close();
        chat.open_with("", 0.0);
        assert_eq!(
            chat.key(KeyCode::Enter, None, ModifiersState::empty()),
            Request::Closed
        );
    }

    #[test]
    fn tab_completion_extends_the_common_prefix_then_cycles() {
        let mut chat = Chat::default();
        chat.open_with("/party invite bo", 0.0);
        assert_eq!(
            chat.key(KeyCode::Tab, None, ModifiersState::empty()),
            Request::Complete("/party invite bo".into())
        );
        assert_eq!(
            chat.completions(vec!["Bobby".into(), "Bobcat".into()]),
            None
        );
        assert_eq!(chat.text(), "/party invite Bob");
        chat.key(KeyCode::Tab, None, ModifiersState::empty());
        assert_eq!(
            chat.completions(vec!["Bobby".into(), "Bobcat".into()]),
            Some("Bobby, Bobcat".into())
        );
        assert_eq!(chat.text(), "/party invite Bobby");
        chat.key(KeyCode::Tab, None, ModifiersState::empty());
        assert_eq!(chat.text(), "/party invite Bobcat");
        // Late answers after another key are ignored.
        chat.key(KeyCode::KeyA, Some("x"), ModifiersState::empty());
        assert_eq!(chat.completions(vec!["Zed".into()]), None);
    }
}
