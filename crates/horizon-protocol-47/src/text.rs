//! Chat components (`IChatComponent` JSON) as 1.8.9 shows them:
//! `getFormattedText` legacy text, with every visible character tagged by the
//! click and hover events of the component it came from.
use serde_json::Value;

/// `ClickEvent.Action` values a client acts on.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Click {
    OpenUrl(String),
    RunCommand(String),
    SuggestCommand(String),
}

/// The events of one component, inherited by its children as `ChatStyle`
/// getters fall back to the parent style.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Events {
    pub click: Option<Click>,
    /// `show_text` hover content as legacy formatted text.
    pub hover: Option<String>,
}

/// Legacy `§`-formatted text. `tags` has one entry per `char` of `text`:
/// zero for none, otherwise one more than an index into `events`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Formatted {
    pub text: String,
    /// `getUnformattedText`: the components' text without their styles; `§`
    /// codes inside the text remain, as the action bar shows them.
    pub unformatted: String,
    pub tags: Vec<u16>,
    pub events: Vec<Events>,
}

impl Formatted {
    pub fn plain(text: &str) -> Self {
        Self {
            text: text.to_owned(),
            unformatted: text.to_owned(),
            tags: vec![0; text.chars().count()],
            events: Vec::new(),
        }
    }

    pub fn event(&self, tag: u16) -> Option<&Events> {
        self.events.get(usize::from(tag).checked_sub(1)?)
    }
}

#[derive(Clone, Default)]
struct Style {
    color: Option<char>,
    bold: Option<bool>,
    italic: Option<bool>,
    underlined: Option<bool>,
    strikethrough: Option<bool>,
    obfuscated: Option<bool>,
    click: Option<Click>,
    hover: Option<String>,
}

impl Style {
    /// `ChatStyle.getFormattingCode` of a style whose unset fields resolved
    /// to its parents: colour, then bold, italic, underline, obfuscated and
    /// strikethrough.
    fn codes(&self) -> String {
        let mut out = String::new();
        if let Some(color) = self.color {
            out.push('§');
            out.push(color);
        }
        for (set, code) in [
            (self.bold, 'l'),
            (self.italic, 'o'),
            (self.underlined, 'n'),
            (self.obfuscated, 'k'),
            (self.strikethrough, 'm'),
        ] {
            if set == Some(true) {
                out.push('§');
                out.push(code);
            }
        }
        out
    }
}

/// `EnumChatFormatting.getValueByName` for colours (and `reset`).
fn color_code(name: &str) -> Option<char> {
    Some(match name {
        "black" => '0',
        "dark_blue" => '1',
        "dark_green" => '2',
        "dark_aqua" => '3',
        "dark_red" => '4',
        "dark_purple" => '5',
        "gold" => '6',
        "gray" => '7',
        "dark_gray" => '8',
        "blue" => '9',
        "green" => 'a',
        "aqua" => 'b',
        "red" => 'c',
        "light_purple" => 'd',
        "yellow" => 'e',
        "white" => 'f',
        "reset" => 'r',
        _ => return None,
    })
}

const MAX_DEPTH: usize = 64;

struct Builder<'a> {
    out: Formatted,
    translate: &'a dyn Fn(&str) -> Option<String>,
}

impl Builder<'_> {
    fn emit(&mut self, style: &Style, text: &str) {
        let codes = style.codes();
        let tag = if style.click.is_some() || style.hover.is_some() {
            let events = Events {
                click: style.click.clone(),
                hover: style.hover.clone(),
            };
            match self.out.events.iter().position(|e| *e == events) {
                Some(index) => index + 1,
                None if self.out.events.len() < usize::from(u16::MAX) - 1 => {
                    self.out.events.push(events);
                    self.out.events.len()
                }
                None => 0,
            }
        } else {
            0
        };
        self.out.text.push_str(&codes);
        self.out
            .tags
            .extend(std::iter::repeat_n(0, codes.chars().count()));
        self.out.text.push_str(text);
        self.out.unformatted.push_str(text);
        self.out
            .tags
            .extend(std::iter::repeat_n(tag as u16, text.chars().count()));
        self.out.text.push_str("§r");
        self.out.tags.extend([0, 0]);
    }

    fn style(&self, value: &Value, parent: &Style, depth: usize) -> Style {
        let mut style = parent.clone();
        let Some(object) = value.as_object() else {
            return style;
        };
        if let Some(code) = object
            .get("color")
            .and_then(Value::as_str)
            .and_then(color_code)
        {
            style.color = Some(code);
        }
        for (key, field) in [
            ("bold", &mut style.bold),
            ("italic", &mut style.italic),
            ("underlined", &mut style.underlined),
            ("strikethrough", &mut style.strikethrough),
            ("obfuscated", &mut style.obfuscated),
        ] {
            if let Some(set) = object.get(key).and_then(Value::as_bool) {
                *field = Some(set);
            }
        }
        if let Some(click) = object.get("clickEvent") {
            let value = click
                .get("value")
                .and_then(Value::as_str)
                .map(str::to_owned);
            style.click = match (click.get("action").and_then(Value::as_str), value) {
                (Some("open_url"), Some(url)) => Some(Click::OpenUrl(url)),
                (Some("run_command"), Some(command)) => Some(Click::RunCommand(command)),
                (Some("suggest_command"), Some(command)) => Some(Click::SuggestCommand(command)),
                _ => style.click,
            };
        }
        if let Some(hover) = object.get("hoverEvent")
            && hover.get("action").and_then(Value::as_str) == Some("show_text")
            && let Some(content) = hover.get("value")
            && depth < MAX_DEPTH
        {
            let mut inner = Builder {
                out: Formatted::default(),
                translate: self.translate,
            };
            inner.walk(content, &Style::default(), depth + 1);
            style.hover = Some(inner.out.text);
        }
        style
    }

    fn walk(&mut self, value: &Value, parent: &Style, depth: usize) {
        if depth > MAX_DEPTH {
            return;
        }
        match value {
            Value::Array(items) => {
                let Some((first, rest)) = items.split_first() else {
                    return;
                };
                let style = self.style(first, parent, depth);
                self.node(first, &style, depth);
                for sibling in rest {
                    self.walk(sibling, &style, depth + 1);
                }
            }
            Value::Object(_) => {
                let style = self.style(value, parent, depth);
                self.node(value, &style, depth);
            }
            Value::String(text) => self.emit(parent, text),
            Value::Number(number) => self.emit(parent, &number.to_string()),
            Value::Bool(set) => self.emit(parent, if *set { "true" } else { "false" }),
            Value::Null => {}
        }
    }

    /// One object or the head of an array: its own text, then `extra`.
    fn node(&mut self, value: &Value, style: &Style, depth: usize) {
        if let Some(object) = value.as_object() {
            if let Some(text) = object.get("text") {
                let text = match text {
                    Value::String(text) => text.clone(),
                    Value::Null => String::new(),
                    other => other.to_string(),
                };
                self.emit(style, &text);
            } else if let Some(key) = object.get("translate").and_then(Value::as_str) {
                let with = object
                    .get("with")
                    .and_then(Value::as_array)
                    .map(Vec::as_slice)
                    .unwrap_or_default();
                self.translation(key, with, style, depth);
            } else if let Some(score) = object.get("score") {
                let text = score.get("value").and_then(Value::as_str).unwrap_or("");
                self.emit(style, text);
            } else if let Some(selector) = object.get("selector").and_then(Value::as_str) {
                self.emit(style, selector);
            }
            if let Some(extra) = object.get("extra").and_then(Value::as_array) {
                for sibling in extra {
                    self.walk(sibling, style, depth + 1);
                }
            }
        } else {
            self.walk(value, style, depth + 1);
        }
    }

    /// `ChatComponentTranslation.initializeFromFormat`: `%s`, `%n$s` and
    /// `%%`; any other conversion shows the format as plain text.
    fn translation(&mut self, key: &str, with: &[Value], style: &Style, depth: usize) {
        let format = (self.translate)(key).unwrap_or_else(|| key.to_owned());
        let mut pieces: Vec<Result<String, usize>> = Vec::new();
        let mut literal = String::new();
        let mut next = 0;
        let mut chars = format.chars().peekable();
        while let Some(c) = chars.next() {
            if c != '%' {
                literal.push(c);
                continue;
            }
            let mut digits = String::new();
            while let Some(d) = chars.peek().filter(|d| d.is_ascii_digit()) {
                digits.push(*d);
                chars.next();
            }
            let explicit = if !digits.is_empty() && chars.peek() == Some(&'$') {
                chars.next();
                digits.parse::<usize>().ok()
            } else if digits.is_empty() {
                None
            } else {
                self.emit(style, &format);
                return;
            };
            match chars.next() {
                Some('%') if explicit.is_none() => literal.push('%'),
                Some('s') => {
                    let index = match explicit {
                        Some(n) => n.saturating_sub(1),
                        None => {
                            next += 1;
                            next - 1
                        }
                    };
                    if index >= with.len() {
                        self.emit(style, &format);
                        return;
                    }
                    pieces.push(Ok(std::mem::take(&mut literal)));
                    pieces.push(Err(index));
                }
                _ => {
                    self.emit(style, &format);
                    return;
                }
            }
        }
        pieces.push(Ok(literal));
        for piece in pieces {
            match piece {
                Ok(text) if !text.is_empty() => self.emit(style, &text),
                Ok(_) => {}
                Err(index) => self.walk(&with[index], style, depth + 1),
            }
        }
    }
}

/// A chat component as legacy formatted text. Malformed JSON shows as its
/// raw text, as vanilla falls back to a plain text component.
pub fn format(json: &str, translate: &dyn Fn(&str) -> Option<String>) -> Formatted {
    let mut builder = Builder {
        out: Formatted::default(),
        translate,
    };
    match serde_json::from_str::<Value>(json) {
        Ok(value) => builder.walk(&value, &Style::default(), 0),
        Err(_) => builder.emit(&Style::default(), json),
    }
    builder.out
}

/// `EnumChatFormatting.getTextWithoutFormattingCodes`.
pub fn strip(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars();
    while let Some(c) = chars.next() {
        if c == '§' {
            chars.next();
        } else {
            out.push(c);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lang(key: &str) -> Option<String> {
        match key {
            "chat.type.text" => Some("<%s> %s".into()),
            "chat.type.announcement" => Some("[%s] %s".into()),
            "commands.generic.usage" => Some("Usage: %1$s (%%)".into()),
            _ => None,
        }
    }

    #[test]
    fn styles_inherit_and_close_with_reset_as_get_formatted_text() {
        let out = format(
            r#"{"text":"A","color":"gold","bold":true,"extra":[{"text":"B","color":"red"},"C",{"text":"D","bold":false}]}"#,
            &lang,
        );
        assert_eq!(out.text, "§6§lA§r§c§lB§r§6§lC§r§6D§r");
        assert_eq!(strip(&out.text), "ABCD");
        assert_eq!(out.tags.len(), out.text.chars().count());
        assert_eq!(format(r#""§aHypixel""#, &lang).text, "§aHypixel§r");
        assert_eq!(format("not json {", &lang).text, "not json {§r");
    }

    #[test]
    fn translations_substitute_components_and_keep_unknown_keys() {
        let out = format(
            r#"{"translate":"chat.type.text","with":[{"text":"Steve","color":"aqua"},"hi"]}"#,
            &lang,
        );
        assert_eq!(out.text, "<§r§bSteve§r> §rhi§r");
        assert_eq!(out.unformatted, "<Steve> hi");
        let usage = format(
            r#"{"translate":"commands.generic.usage","with":["/l"]}"#,
            &lang,
        );
        assert_eq!(strip(&usage.text), "Usage: /l (%)");
        assert_eq!(strip(&format(r#"{"translate":"x.y"}"#, &lang).text), "x.y");
        // An argument the format cannot reach shows the format itself.
        assert_eq!(
            strip(&format(r#"{"translate":"chat.type.text","with":["a"]}"#, &lang).text),
            "<%s> %s"
        );
    }

    #[test]
    fn click_and_hover_events_tag_only_their_characters() {
        let out = format(
            r#"["",{"text":"Join ","color":"yellow"},{"text":"[ACCEPT]","clickEvent":{"action":"run_command","value":"/party accept Bob"},"hoverEvent":{"action":"show_text","value":{"text":"Click","color":"green"}}}]"#,
            &lang,
        );
        let visible: Vec<(char, u16)> = out
            .text
            .chars()
            .zip(out.tags.iter().copied())
            .filter(|(_, tag)| *tag != 0)
            .collect();
        assert_eq!(
            visible.iter().map(|(c, _)| c).collect::<String>(),
            "[ACCEPT]"
        );
        let events = out.event(visible[0].1).unwrap();
        assert_eq!(
            events.click,
            Some(Click::RunCommand("/party accept Bob".into()))
        );
        assert_eq!(events.hover.as_deref(), Some("§aClick§r"));
    }

    #[test]
    fn hostile_nesting_is_bounded() {
        let nested = |levels: usize| {
            let mut json = String::new();
            for _ in 0..levels {
                json.push_str(r#"{"text":"x","extra":["#);
            }
            json.push_str("\"y\"");
            for _ in 0..levels {
                json.push_str("]}");
            }
            json
        };
        // Within the parser's limit, every level shows.
        assert_eq!(strip(&format(&nested(40), &lang).text).len(), 41);
        // Beyond it the message degrades to its raw text instead of recursing.
        let deep = nested(500);
        assert_eq!(format(&deep, &lang).text.len(), deep.len() + 3);
    }
}
