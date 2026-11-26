//! `GuiScreenResourcePacks`: available packs on the left, selected ones on
//! the right (highest priority on top, the default pack last), Open
//! Resource Pack Folder and Done below. As `ResourcePackListEntry` has it,
//! only the 32-pixel icon takes clicks: an available pack's moves it across;
//! a selected pack's left half removes it, the right half's top and bottom
//! move it up and down.
use crate::{
    hud::button_widget,
    ui::{Canvas, Layout, RESOURCE_PACKS},
};
use horizon_assets_189::packs::Pack;

/// `GuiResourcePackList`: 200 wide, entries 36 tall from 32 down.
const LIST_W: f32 = 200.0;
const ENTRY_H: f32 = 36.0;
const TOP: f32 = 32.0;
/// `ResourcePackListEntry`: the text column's width.
const TEXT_W: i32 = 157;

#[derive(Clone, Debug, Default)]
pub struct Screen {
    /// Everything in the packs folder.
    packs: Vec<Pack>,
    /// Selected names, highest priority first.
    pub selected: Vec<String>,
    /// First visible row of each list.
    scroll: [usize; 2],
}

/// What a click did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    Nothing,
    Moved,
    OpenFolder,
    Done,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Hit {
    /// A list row: side, index, and where in the row (pixels from its
    /// corner).
    Row(usize, usize, f32, f32),
    OpenFolder,
    Done,
}

impl Screen {
    pub fn open(packs: Vec<Pack>, selected: &[String]) -> Self {
        let selected = selected
            .iter()
            .filter(|name| packs.iter().any(|pack| &pack.name == *name))
            .cloned()
            .collect();
        Self {
            packs,
            selected,
            scroll: [0, 0],
        }
    }

    fn available(&self) -> Vec<&Pack> {
        self.packs
            .iter()
            .filter(|pack| !self.selected.contains(&pack.name))
            .collect()
    }

    fn pack(&self, name: &str) -> Option<&Pack> {
        self.packs.iter().find(|pack| pack.name == name)
    }

    /// Rows per list: the available packs, the selected ones plus Default.
    fn count(&self, side: usize) -> usize {
        if side == 0 {
            self.available().len()
        } else {
            self.selected.len() + 1
        }
    }

    /// The lists' rectangles: left and right.
    fn lists(layout: Layout) -> [[f32; 4]; 2] {
        let (w, h) = (layout.width.floor(), layout.height.floor());
        let bottom = h - 51.0;
        let left = (w / 2.0 - 4.0 - LIST_W).floor();
        let right = (w / 2.0 + 4.0).floor();
        [
            [left, TOP, left + LIST_W, bottom],
            [right, TOP, right + LIST_W, bottom],
        ]
    }

    fn buttons(layout: Layout) -> [[f32; 4]; 2] {
        let (w, h) = (layout.width.floor(), layout.height.floor());
        [
            [(w / 2.0 - 154.0).floor(), h - 48.0, 150.0, 20.0],
            [(w / 2.0 + 4.0).floor(), h - 48.0, 150.0, 20.0],
        ]
    }

    /// Rows that fit below each list's title.
    fn rows(list: [f32; 4]) -> usize {
        ((list[3] - list[1] - 14.0) / ENTRY_H).floor().max(0.0) as usize
    }

    fn hit(&self, layout: Layout, (x, y): (f32, f32)) -> Option<Hit> {
        let [open, done] = Self::buttons(layout);
        let within = |b: [f32; 4]| x >= b[0] && x < b[0] + b[2] && y >= b[1] && y < b[1] + b[3];
        if within(open) {
            return Some(Hit::OpenFolder);
        }
        if within(done) {
            return Some(Hit::Done);
        }
        for (side, list) in Self::lists(layout).into_iter().enumerate() {
            if x < list[0] || x >= list[2] || y < list[1] + 14.0 || y >= list[3] {
                continue;
            }
            let shown = ((y - list[1] - 14.0) / ENTRY_H) as usize;
            let row = shown + self.scroll[side];
            let dy = y - list[1] - 14.0 - shown as f32 * ENTRY_H;
            return (row < self.count(side) && dy < 32.0).then_some(Hit::Row(
                side,
                row,
                x - list[0],
                dy,
            ));
        }
        None
    }

    pub fn press(&mut self, layout: Layout, mouse: (f32, f32)) -> Action {
        match self.hit(layout, mouse) {
            // ResourcePackListEntry.mousePressed: the icon only.
            Some(Hit::Row(0, row, dx, _)) if dx < 32.0 => {
                let Some(name) = self.available().get(row).map(|pack| pack.name.clone()) else {
                    return Action::Nothing;
                };
                // A newly selected pack goes on top.
                self.selected.insert(0, name);
                Action::Moved
            }
            Some(Hit::Row(1, row, dx, dy)) if dx < 32.0 && row < self.selected.len() => {
                if dx < 16.0 {
                    self.selected.remove(row);
                } else if dy < 16.0 && row > 0 {
                    self.selected.swap(row, row - 1);
                } else if dy > 16.0 && row + 1 < self.selected.len() {
                    self.selected.swap(row, row + 1);
                } else {
                    return Action::Nothing;
                }
                Action::Moved
            }
            Some(Hit::OpenFolder) => Action::OpenFolder,
            Some(Hit::Done) => Action::Done,
            _ => Action::Nothing,
        }
    }

    /// The wheel over a list scrolls it a row a notch.
    pub fn scroll(&mut self, layout: Layout, (x, _): (f32, f32), notches: i32) {
        for (side, list) in Self::lists(layout).into_iter().enumerate() {
            if x < list[0] || x >= list[2] {
                continue;
            }
            let most = self.count(side).saturating_sub(Self::rows(list));
            let row = self.scroll[side] as i64 - i64::from(notches);
            self.scroll[side] = row.clamp(0, most as i64) as usize;
        }
    }

    pub fn draw(&self, canvas: &mut Canvas<'_>, mouse: (f32, f32)) {
        let (w, h) = (canvas.layout.width, canvas.layout.height);
        let lists = Self::lists(canvas.layout);
        // drawBackground, then each GuiSlot's darker dirt between the
        // overlays.
        canvas.dirt(0.0, 0.0, w, h, 64.0 / 255.0);
        for list in lists {
            canvas.dirt(list[0], list[1], list[2], list[3], 32.0 / 255.0);
        }
        canvas.gradient(0.0, TOP, w, TOP + 4.0, 0xFF00_0000, 0x0000_0000);
        canvas.gradient(0.0, h - 55.0, w, h - 51.0, 0x0000_0000, 0xFF00_0000);
        let res = canvas.res;
        let translate = |key: &str| res.translate(key).to_owned();
        canvas.centered(
            &translate("resourcePack.title"),
            (w / 2.0).floor(),
            16.0,
            0xFFFF_FFFF,
            true,
        );
        let hover = self.hit(canvas.layout, mouse);
        let available = self.available();
        let titles = [
            "resourcePack.available.title",
            "resourcePack.selected.title",
        ];
        for (side, list) in lists.into_iter().enumerate() {
            let title = format!("§n§l{}", translate(titles[side]));
            let centre = (list[0] + LIST_W / 2.0).floor();
            canvas.centered(&title, centre, list[1] + 2.0, 0xFFFF_FFFF, false);
            let entries: Vec<Option<&Pack>> = if side == 0 {
                available.iter().map(|pack| Some(*pack)).collect()
            } else {
                self.selected
                    .iter()
                    .map(|name| self.pack(name))
                    .chain(std::iter::once(None))
                    .collect()
            };
            let rows = Self::rows(list);
            for (index, entry) in entries
                .iter()
                .enumerate()
                .skip(self.scroll[side])
                .take(rows)
            {
                let y = list[1] + 14.0 + (index - self.scroll[side]) as f32 * ENTRY_H;
                let at = match hover {
                    Some(Hit::Row(s, row, dx, dy)) if s == side && row == index => Some((dx, dy)),
                    _ => None,
                };
                self.entry(canvas, [list[0], y], *entry, side, index, at);
            }
        }
        let [open, done] = Self::buttons(canvas.layout);
        let over = |b: [f32; 4]| {
            mouse.0 >= b[0] && mouse.0 < b[0] + b[2] && mouse.1 >= b[1] && mouse.1 < b[1] + b[3]
        };
        let label = translate("resourcePack.openFolder");
        button_widget(canvas, &label, open, true, over(open));
        button_widget(canvas, &translate("gui.done"), done, true, over(done));
        canvas.centered(
            &translate("resourcePack.folderInfo"),
            (w / 2.0 - 77.0).floor(),
            h - 26.0,
            0xFF80_8080,
            false,
        );
    }

    /// `ResourcePackListEntry.drawEntry`.
    fn entry(
        &self,
        canvas: &mut Canvas<'_>,
        [x, y]: [f32; 2],
        pack: Option<&Pack>,
        side: usize,
        index: usize,
        hover: Option<(f32, f32)>,
    ) {
        let res = canvas.res;
        let format = pack.map_or(1, |pack| pack.format);
        if format != 1 {
            canvas.rect(x - 1.0, y - 1.0, x + LIST_W - 9.0, y + 33.0, 0xFF77_0000);
        }
        // No pack.png textures yet: a tile with the pack's initial.
        canvas.rect(x, y, x + 32.0, y + 32.0, 0xFF3A_3A3A);
        canvas.rect(x + 1.0, y + 1.0, x + 31.0, y + 31.0, 0xFF5A_5A5A);
        let (mut name, mut description) = match pack {
            Some(pack) => (
                pack.name.trim_end_matches(".zip").to_owned(),
                horizon_protocol_47::text::format(&pack.description, &|_| None).text,
            ),
            None => (
                "Default".to_owned(),
                "The default look of Minecraft".to_owned(),
            ),
        };
        let initial = name.chars().take(1).collect::<String>().to_uppercase();
        canvas.centered(&initial, x + 16.0, y + 12.0, 0xFFFF_FFFF, true);
        if let Some((dx, dy)) = hover
            && pack.is_some()
        {
            canvas.rect(x, y, x + 32.0, y + 32.0, 0xA090_9090);
            if format != 1 {
                name = res.translate("resourcePack.incompatible").to_owned();
                let key = if format < 1 {
                    "resourcePack.incompatible.old"
                } else {
                    "resourcePack.incompatible.new"
                };
                description = res.translate(key).to_owned();
            }
            let on_icon = dx < 32.0;
            let white = [1.0; 4];
            let mut arrow = |u: f32, lit: bool| {
                let v = if lit { 32.0 } else { 0.0 };
                canvas.sprite_region(
                    RESOURCE_PACKS,
                    [x, y, 32.0, 32.0],
                    [u, v, 32.0, 32.0],
                    white,
                );
            };
            if side == 0 {
                arrow(0.0, on_icon);
            } else {
                arrow(32.0, on_icon && dx < 16.0);
                if index > 0 {
                    arrow(96.0, on_icon && dx > 16.0 && dy < 16.0);
                }
                if index + 1 < self.selected.len() {
                    arrow(64.0, on_icon && dx > 16.0 && dy > 16.0);
                }
            }
        }
        let name = trim(canvas, &name, TEXT_W);
        canvas.text(&name, x + 34.0, y + 1.0, 0xFFFF_FFFF, true);
        let tags = vec![0; description.chars().count()];
        let lines = res.wrap(&description, &tags, TEXT_W);
        for (line, (text, _)) in lines.iter().take(2).enumerate() {
            canvas.text(
                text,
                x + 34.0,
                y + 12.0 + 10.0 * line as f32,
                0xFF80_8080,
                true,
            );
        }
    }
}

/// `trimStringToWidth` with "..." when cut, as the entry does names.
fn trim(canvas: &Canvas<'_>, text: &str, width: i32) -> String {
    if canvas.res.width(text) <= width {
        return text.to_owned();
    }
    let room = width - canvas.res.width("...");
    let mut out = String::new();
    for c in text.chars() {
        out.push(c);
        if canvas.res.width(&out) > room {
            out.pop();
            break;
        }
    }
    format!("{out}...")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pack(name: &str) -> Pack {
        Pack {
            name: name.to_owned(),
            path: name.into(),
            description: "\"x\"".into(),
            format: 1,
            icon: None,
        }
    }

    #[test]
    fn icons_move_packs_across_and_reorder() {
        let layout = Layout::new((854, 480), 1.0, 2);
        let mut screen = Screen::open(vec![pack("a"), pack("b"), pack("c")], &["gone".into()]);
        assert!(screen.selected.is_empty(), "missing packs drop out");
        let [left, right] = Screen::lists(layout);
        let row = |list: [f32; 4], index: usize, dx: f32, dy: f32| {
            (list[0] + dx, list[1] + 14.0 + index as f32 * ENTRY_H + dy)
        };
        // The name doesn't take clicks; the icon does.
        assert_eq!(
            screen.press(layout, row(left, 1, 100.0, 10.0)),
            Action::Nothing
        );
        // Select b, then a: the newest goes on top.
        assert_eq!(
            screen.press(layout, row(left, 1, 10.0, 10.0)),
            Action::Moved
        );
        assert_eq!(
            screen.press(layout, row(left, 0, 10.0, 10.0)),
            Action::Moved
        );
        assert_eq!(screen.selected, ["a", "b"]);
        // The right half's bottom moves a below b; b's top moves it back.
        assert_eq!(
            screen.press(layout, row(right, 0, 24.0, 24.0)),
            Action::Moved
        );
        assert_eq!(screen.selected, ["b", "a"]);
        assert_eq!(
            screen.press(layout, row(right, 1, 24.0, 4.0)),
            Action::Moved
        );
        assert_eq!(screen.selected, ["a", "b"]);
        // The top can't go up; the default pack row does nothing.
        assert_eq!(
            screen.press(layout, row(right, 0, 24.0, 4.0)),
            Action::Nothing
        );
        assert_eq!(
            screen.press(layout, row(right, 2, 10.0, 10.0)),
            Action::Nothing
        );
        // The left half removes.
        assert_eq!(
            screen.press(layout, row(right, 1, 8.0, 10.0)),
            Action::Moved
        );
        assert_eq!(screen.selected, ["a"]);
        let [open, done] = Screen::buttons(layout);
        assert_eq!(
            screen.press(layout, (open[0] + 5.0, open[1] + 5.0)),
            Action::OpenFolder
        );
        assert_eq!(
            screen.press(layout, (done[0] + 5.0, done[1] + 5.0)),
            Action::Done
        );
    }
}
