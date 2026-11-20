//! Authoritative window snapshots, vanilla container geometry and predicted
//! clicks: pick up, shift-click, number keys and drops.
use horizon_protocol_47::{interaction, nbt::Item, read_string};
use std::io::{self, Cursor, Read};

/// The `GuiContainer` a window opens as.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Kind {
    #[default]
    Player,
    Chest(usize),
    Hopper,
    Dispenser,
    Crafting,
    Furnace,
    /// Windows drawn as a chest of their slots.
    Generic,
}

impl Kind {
    /// The S2D type, and its slot count where vanilla sends zero.
    fn from_type(kind: &str, slots: usize) -> (Self, usize) {
        let known = |count: usize| if slots == 0 { count } else { slots };
        match kind {
            "minecraft:chest" | "minecraft:container" => {
                (Kind::Chest(slots.div_ceil(9).max(1)), slots)
            }
            "minecraft:hopper" => (Kind::Hopper, known(5)),
            "minecraft:dispenser" | "minecraft:dropper" => (Kind::Dispenser, known(9)),
            "minecraft:crafting_table" => (Kind::Crafting, known(10)),
            "minecraft:furnace" => (Kind::Furnace, known(3)),
            "minecraft:enchanting_table" => (Kind::Generic, known(2)),
            "minecraft:anvil" | "minecraft:villager" => (Kind::Generic, known(3)),
            "minecraft:brewing_stand" => (Kind::Generic, known(4)),
            "minecraft:beacon" => (Kind::Generic, known(1)),
            _ => (Kind::Generic, slots),
        }
    }
}

/// A container screen's sheet parts, slots and labels in GUI pixels from
/// its top-left corner.
pub struct Screen {
    pub sheet: u32,
    pub size: (f32, f32),
    /// `(x, y, u, v, width, height)` blits from the sheet.
    pub parts: Vec<[f32; 6]>,
    pub slots: Vec<(i16, f32, f32)>,
    /// Title position; `None` x centres it.
    pub title: (Option<f32>, f32),
    pub inventory_label: Option<(f32, f32)>,
}

#[derive(Clone, Debug, Default)]
pub struct View {
    pub open: bool,
    pub window: u8,
    pub container_slots: usize,
    pub slots: Vec<Option<Item>>,
    pub cursor: Option<Item>,
    pub selected: u8,
    pub kind: Kind,
    /// The window title as `getUnformattedText`.
    pub title: String,
    /// S31 window properties: a furnace's burn time, item burn time, cook
    /// time and total cook time.
    pub properties: [i16; 4],
}

fn player_slots(out: &mut Vec<(i16, f32, f32)>, first: usize, main_y: f32, hotbar_y: f32) {
    for index in 0..36 {
        let (x, y) = if index < 27 {
            (
                8.0 + (index % 9) as f32 * 18.0,
                main_y + (index / 9) as f32 * 18.0,
            )
        } else {
            (8.0 + (index - 27) as f32 * 18.0, hotbar_y)
        };
        out.push(((first + index) as i16, x, y));
    }
}

impl View {
    /// The vanilla `GuiContainer` geometry for this window.
    pub fn screen(&self) -> Screen {
        use crate::ui::{CHEST, CRAFTING, DISPENSER, FURNACE, HOPPER, INVENTORY};
        let n = self.container_slots;
        let mut slots = Vec::with_capacity(n + 36);
        let generic_rows = n.div_ceil(9).max(1);
        let kind = if self.window == 0 {
            Kind::Player
        } else if self.kind == Kind::Generic {
            Kind::Chest(generic_rows)
        } else {
            self.kind
        };
        let standard = |slots: &mut Vec<(i16, f32, f32)>| player_slots(slots, n, 84.0, 142.0);
        match kind {
            Kind::Player => {
                slots.push((0, 144.0, 36.0));
                for i in 0..4 {
                    slots.push((
                        (1 + i) as i16,
                        88.0 + (i % 2) as f32 * 18.0,
                        26.0 + (i / 2) as f32 * 18.0,
                    ));
                }
                for i in 0..4 {
                    slots.push(((5 + i) as i16, 8.0, 8.0 + i as f32 * 18.0));
                }
                player_slots(&mut slots, 9, 84.0, 142.0);
                Screen {
                    sheet: INVENTORY,
                    size: (176.0, 166.0),
                    parts: vec![[0.0, 0.0, 0.0, 0.0, 176.0, 166.0]],
                    slots,
                    title: (Some(86.0), 16.0),
                    inventory_label: None,
                }
            }
            Kind::Chest(_) | Kind::Generic => {
                let rows = if let Kind::Chest(rows) = kind {
                    rows.clamp(1, 6)
                } else {
                    generic_rows
                };
                for i in 0..n {
                    slots.push((
                        i as i16,
                        8.0 + (i % 9) as f32 * 18.0,
                        18.0 + (i / 9) as f32 * 18.0,
                    ));
                }
                let offset = (rows as f32 - 4.0) * 18.0;
                player_slots(&mut slots, n, 103.0 + offset, 161.0 + offset);
                let height = 114.0 + rows as f32 * 18.0;
                Screen {
                    sheet: CHEST,
                    size: (176.0, height),
                    parts: vec![
                        [0.0, 0.0, 0.0, 0.0, 176.0, rows as f32 * 18.0 + 17.0],
                        [0.0, rows as f32 * 18.0 + 17.0, 0.0, 126.0, 176.0, 96.0],
                    ],
                    slots,
                    title: (Some(8.0), 6.0),
                    inventory_label: Some((8.0, height - 94.0)),
                }
            }
            Kind::Hopper => {
                for i in 0..n.min(5) {
                    slots.push((i as i16, 44.0 + i as f32 * 18.0, 20.0));
                }
                player_slots(&mut slots, n, 51.0, 109.0);
                Screen {
                    sheet: HOPPER,
                    size: (176.0, 133.0),
                    parts: vec![[0.0, 0.0, 0.0, 0.0, 176.0, 133.0]],
                    slots,
                    title: (Some(8.0), 6.0),
                    inventory_label: Some((8.0, 39.0)),
                }
            }
            Kind::Dispenser => {
                for i in 0..n.min(9) {
                    slots.push((
                        i as i16,
                        62.0 + (i % 3) as f32 * 18.0,
                        17.0 + (i / 3) as f32 * 18.0,
                    ));
                }
                standard(&mut slots);
                Screen {
                    sheet: DISPENSER,
                    size: (176.0, 166.0),
                    parts: vec![[0.0, 0.0, 0.0, 0.0, 176.0, 166.0]],
                    slots,
                    title: (None, 6.0),
                    inventory_label: Some((8.0, 72.0)),
                }
            }
            Kind::Crafting => {
                slots.push((0, 124.0, 35.0));
                for i in 0..9.min(n.saturating_sub(1)) {
                    slots.push((
                        (1 + i) as i16,
                        30.0 + (i % 3) as f32 * 18.0,
                        17.0 + (i / 3) as f32 * 18.0,
                    ));
                }
                standard(&mut slots);
                Screen {
                    sheet: CRAFTING,
                    size: (176.0, 166.0),
                    parts: vec![[0.0, 0.0, 0.0, 0.0, 176.0, 166.0]],
                    slots,
                    title: (Some(28.0), 6.0),
                    inventory_label: Some((8.0, 72.0)),
                }
            }
            Kind::Furnace => {
                for (i, (x, y)) in [(56.0, 17.0), (56.0, 53.0), (116.0, 35.0)]
                    .into_iter()
                    .enumerate()
                    .take(n)
                {
                    slots.push((i as i16, x, y));
                }
                standard(&mut slots);
                Screen {
                    sheet: FURNACE,
                    size: (176.0, 166.0),
                    parts: vec![[0.0, 0.0, 0.0, 0.0, 176.0, 166.0]],
                    slots,
                    title: (None, 6.0),
                    inventory_label: Some((8.0, 72.0)),
                }
            }
        }
    }

    /// `GuiContainer.getSlotAtPosition`, from the screen's top-left corner.
    pub fn slot_at(&self, x: f32, y: f32) -> Option<i16> {
        self.screen()
            .slots
            .into_iter()
            .find(|&(_, a, b)| x >= a - 1.0 && x < a + 17.0 && y >= b - 1.0 && y < b + 17.0)
            .map(|(slot, _, _)| slot)
    }

    /// The window index of hotbar slot `button`.
    fn hotbar(&self, button: u8) -> usize {
        if self.window == 0 {
            36 + usize::from(button)
        } else {
            self.container_slots + 27 + usize::from(button)
        }
    }
}

fn same_kind(a: &Item, b: &Item) -> bool {
    a.id == b.id && a.damage == b.damage && a.nbt == b.nbt
}

/// `Container.mergeItemStack` over `start..end`: top up matching stacks,
/// then fill the first empty slot. Returns whether anything moved.
fn merge(
    slots: &mut [Option<Item>],
    stack: &mut Item,
    start: usize,
    end: usize,
    reverse: bool,
) -> bool {
    let end = end.min(slots.len());
    if start >= end {
        return false;
    }
    let order: Vec<usize> = if reverse {
        (start..end).rev().collect()
    } else {
        (start..end).collect()
    };
    let limit = stack_limit(stack.id);
    let mut moved = false;
    if limit > 1 {
        for &i in &order {
            if stack.count <= 0 {
                break;
            }
            if let Some(target) = &mut slots[i]
                && same_kind(target, stack)
            {
                let total = target.count + stack.count;
                if total <= limit {
                    stack.count = 0;
                    target.count = total;
                    moved = true;
                } else if target.count < limit {
                    stack.count -= limit - target.count;
                    target.count = limit;
                    moved = true;
                }
            }
        }
    }
    if stack.count > 0
        && let Some(&i) = order.iter().find(|&&i| slots[i].is_none())
    {
        slots[i] = Some(stack.clone());
        stack.count = 0;
        moved = true;
    }
    moved
}

struct Drag {
    button: u8,
    slots: Vec<usize>,
}

/// Slot rules for ContainerPlayer/ContainerChest. Unknown containers do not
/// predict drag/collect: both modes return null even when the server changes
/// slots, so null alone cannot force a rejected-transaction resync.
fn slot_limit(view: &View, index: usize, item: &Item) -> i8 {
    if index >= view.slots.len() {
        return 0;
    }
    if view.window != 0 {
        return if matches!(view.kind, Kind::Chest(_)) {
            stack_limit(item.id)
        } else {
            0
        };
    }
    match index {
        0 => 0,
        5..=8 => {
            let armor = (298..=317).contains(&item.id) && (item.id - 298) % 4 == (index - 5) as i16;
            if armor || index == 5 && matches!(item.id, 86 | 397) {
                1
            } else {
                0
            }
        }
        1..=4 | 9..=44 => stack_limit(item.id),
        _ => 0,
    }
}

pub(crate) fn drag_eligible(view: &View, index: usize, item: &Item) -> bool {
    let limit = slot_limit(view, index, item);
    limit > 0
        && view.slots[index]
            .as_ref()
            .is_none_or(|s| same_kind(s, item) && s.count < limit)
}

pub fn drag_preview(view: &View, slots: &[i16], button: u8) -> View {
    let mut inv = Inventory {
        view: view.clone(),
        ..Inventory::default()
    };
    inv.drag_click(None, button * 4);
    for &slot in slots {
        inv.drag_click(usize::try_from(slot).ok(), button * 4 + 1);
    }
    inv.drag_click(None, button * 4 + 2);
    inv.view
}

pub struct Inventory {
    pub player: Vec<Option<Item>>,
    pub view: View,
    transaction: i16,
    pending: std::collections::HashMap<i16, View>,
    blocked: bool,
    drag: Option<Drag>,
}

impl Default for Inventory {
    fn default() -> Self {
        Self {
            player: vec![None; 45],
            view: View {
                slots: vec![None; 45],
                ..View::default()
            },
            transaction: 0,
            pending: Default::default(),
            blocked: false,
            drag: None,
        }
    }
}

/// `Item.getItemStackLimit` from the registry table; unknown IDs stack to 64.
pub fn stack_limit(id: i16) -> i8 {
    crate::item_data::ITEMS
        .iter()
        .find(|entry| entry.0 == id && entry.1 < 0)
        .map_or(64, |entry| entry.4 as i8)
}

impl Inventory {
    pub fn accepts_gesture(&self, window: u8, count: usize) -> bool {
        self.view.open
            && self.view.window == window
            && !self.blocked
            && self.pending.len() + count <= 128
    }

    pub fn held(&self) -> Option<&Item> {
        self.player
            .get(36 + usize::from(self.view.selected))
            .and_then(Option::as_ref)
    }
    /// S2D; `title` turns the title component into its unformatted text.
    pub fn open_packet(&mut self, fields: &[u8], title: &dyn Fn(&str) -> String) -> io::Result<()> {
        let mut input = Cursor::new(fields);
        let mut b = [0];
        input.read_exact(&mut b)?;
        let window = b[0];
        let kind = read_string(&mut input, 32)?;
        let title = title(&read_string(&mut input, 32767)?);
        input.read_exact(&mut b)?;
        let (kind, slots) = Kind::from_type(&kind, usize::from(b[0]));
        if slots > 216 {
            return Err(io::Error::other("Oversized container"));
        }
        self.drag = None;
        self.pending.clear();
        self.blocked = false;
        self.view = View {
            open: true,
            window,
            container_slots: slots,
            slots: vec![None; slots + 36],
            cursor: self.view.cursor.take(),
            selected: self.view.selected,
            kind,
            title,
            properties: [0; 4],
        };
        Ok(())
    }
    fn player_slot(&mut self, index: usize, item: Option<Item>) {
        let Some(target) = self.player.get_mut(index) else {
            return;
        };
        *target = item.clone();
        let view_index = if self.view.window == 0 {
            Some(index)
        } else if (9..45).contains(&index) {
            Some(self.view.container_slots + index - 9)
        } else {
            None
        };
        if let Some(target) = view_index.and_then(|i| self.view.slots.get_mut(i)) {
            *target = item;
        }
    }
    pub fn set_slot(&mut self, window: u8, slot: i16, item: Option<Item>) {
        if window == 255 && slot == -1 {
            self.view.cursor = item;
            return;
        }
        if window == 254 {
            let index = match slot {
                0..=8 => 36 + slot as usize,
                9..=35 => slot as usize,
                36..=39 => 44 - slot as usize,
                _ => return,
            };
            self.player_slot(index, item);
            return;
        }
        if window == 0 {
            if let Ok(index) = usize::try_from(slot) {
                self.player_slot(index, item);
            }
            return;
        }
        if window == self.view.window
            && let Ok(index) = usize::try_from(slot)
        {
            if let Some(target) = self.view.slots.get_mut(index) {
                *target = item.clone();
            }
            if index >= self.view.container_slots {
                self.player_slot(9 + index - self.view.container_slots, item);
            }
        }
    }
    pub fn items(&mut self, window: u8, items: Vec<Option<Item>>) {
        if window == 0 {
            for (index, item) in items.iter().take(self.player.len()).enumerate() {
                // InventoryPlayer is shared by the player and active container slots.
                self.player_slot(index, item.clone());
            }
        }
        if window == self.view.window {
            if window != 0 && items.len() >= 36 {
                self.view.container_slots = items.len() - 36;
            }
            self.view.slots = items;
            self.blocked = false;
            self.drag = None;
            self.pending.clear();
        }
        if window != 0 && window == self.view.window && self.view.slots.len() >= 36 {
            let start = self.view.slots.len() - 36;
            self.player[9..45].clone_from_slice(&self.view.slots[start..]);
        }
    }
    pub fn local_open(&mut self) {
        self.view.open = true;
        self.view.window = 0;
        self.view.container_slots = 0;
        self.view.kind = Kind::Player;
        self.view.title.clear();
        self.view.slots = self.player.clone();
    }

    /// `--stackSize` on the held stack, emptied at zero (`ItemBlock.onItemUse`
    /// then `rightClickMouse`).
    pub fn consume_held(&mut self) {
        let index = 36 + usize::from(self.view.selected);
        if let Some(mut item) = self.player.get(index).cloned().flatten() {
            item.count -= 1;
            self.player_slot(index, (item.count > 0).then_some(item));
        }
    }
    pub fn close(&mut self) -> Vec<u8> {
        let packet = interaction::close_window(self.view.window);
        self.view.open = false;
        self.view.cursor = None;
        self.view.window = 0;
        self.view.container_slots = 0;
        self.view.slots = self.player.clone();
        self.drag = None;
        self.pending.clear();
        self.blocked = false;
        packet
    }
    /// A left or right pick-up click (mode 0).
    #[cfg(test)]
    pub fn click(&mut self, slot: i16, right: bool) -> io::Result<Option<Vec<u8>>> {
        self.click_mode(slot, 0, u8::from(right))
    }

    /// `Container.slotClick` predicted locally, then C0E with what it
    /// returned. Shift-clicks the client cannot predict send no clicked item,
    /// so a server that moved anything rejects it and resends the window.
    pub fn click_mode(&mut self, slot: i16, mode: u8, button: u8) -> io::Result<Option<Vec<u8>>> {
        if self.blocked || !self.view.open || self.pending.len() >= 128 || button > 8 {
            return Ok(None);
        }
        let index = usize::try_from(slot)
            .ok()
            .filter(|&i| i < self.view.slots.len());
        if mode != 5 {
            self.drag = None;
        }
        let before = self.view.clone();
        let clicked = match (mode, index) {
            (5, _)
                if button <= 6
                    && (self.view.window == 0 || matches!(self.view.kind, Kind::Chest(_))) =>
            {
                self.drag_click(index, button);
                None
            }
            (6, Some(index))
                if button <= 1
                    && (self.view.window == 0 || matches!(self.view.kind, Kind::Chest(_))) =>
            {
                self.collect(index, button != 0);
                None
            }
            (0, None) if slot == -999 && button <= 1 => {
                let Some(cursor) = &mut self.view.cursor else {
                    return Ok(None);
                };
                cursor.count = if button == 0 { 0 } else { cursor.count - 1 };
                if cursor.count <= 0 {
                    self.view.cursor = None;
                }
                None
            }
            (0, Some(index)) if button <= 1 => {
                let original = self.view.slots[index].clone();
                self.pick_up(index, button == 1);
                original
            }
            (1, Some(index)) if button <= 1 => self.shift_click(index),
            (2, Some(index)) => {
                if !self.hotbar_swap(index, button) {
                    return Ok(None);
                }
                None
            }
            (4, Some(index)) if button <= 1 && self.view.cursor.is_none() => {
                let Some(item) = &mut self.view.slots[index] else {
                    return Ok(None);
                };
                item.count = if button == 0 { item.count - 1 } else { 0 };
                if item.count <= 0 {
                    self.view.slots[index] = None;
                }
                None
            }
            _ => return Ok(None),
        };
        if let Some(index) = index
            && matches!(mode, 0 | 2 | 4)
            && self.crafting_result(index)
            && before.slots[index].is_some()
            && self.view.slots[index] != before.slots[index]
        {
            self.crafted(index);
        }
        self.transaction = self.transaction.wrapping_add(1);
        let packet = interaction::click_mode(
            self.view.window,
            slot,
            button,
            mode,
            self.transaction,
            clicked.as_ref(),
        )?;
        self.pending.insert(self.transaction, before);
        self.sync_player();
        Ok(Some(packet))
    }

    /// The view index of `InventoryPlayer.mainInventory[index]`.
    fn main_slot(&self, index: usize) -> usize {
        if index < 9 {
            self.view.hotbar(index as u8)
        } else if self.view.window == 0 {
            index
        } else {
            self.view.container_slots + index - 9
        }
    }

    /// Whether a view slot belongs to the player's own inventory.
    fn own_slot(&self, index: usize) -> bool {
        if self.view.window == 0 {
            index >= 5
        } else {
            index >= self.view.container_slots
        }
    }

    /// `Slot.isItemValid`, where this client knows it.
    fn slot_accepts(&self, index: usize, item: &Item) -> Option<bool> {
        if self.own_slot(index) {
            if self.view.window == 0 && index <= 8 {
                // ContainerPlayer's armour slots.
                let kind = index - 5;
                return Some(if (298..=317).contains(&item.id) {
                    (item.id as usize - 298) % 4 == kind
                } else {
                    matches!(item.id, 86 | 397) && kind == 0
                });
            }
            return Some(true);
        }
        match (self.view.window, self.view.kind, index) {
            (0, _, 0) | (_, Kind::Crafting, 0) | (_, Kind::Furnace, 2) => Some(false),
            (0, _, _)
            | (_, Kind::Chest(_) | Kind::Hopper | Kind::Dispenser | Kind::Crafting, _) => {
                Some(true)
            }
            (_, Kind::Furnace, 0) => Some(true),
            _ => None,
        }
    }

    /// `InventoryPlayer.getFirstEmptyStack`, as a view index.
    fn first_empty(&self) -> Option<usize> {
        (0..36)
            .map(|i| self.main_slot(i))
            .find(|&slot| self.view.slots.get(slot).is_some_and(Option::is_none))
    }

    /// `InventoryPlayer.addItemStackToInventory` for a stack known to fit.
    fn add_to_inventory(&mut self, mut stack: Item) {
        let limit = stack_limit(stack.id);
        while stack.count > 0 {
            let partial = (limit > 1)
                .then(|| {
                    (0..36).map(|i| self.main_slot(i)).find(|&slot| {
                        self.view.slots[slot]
                            .as_ref()
                            .is_some_and(|t| same_kind(t, &stack) && t.count < limit)
                    })
                })
                .flatten();
            let Some(slot) = partial.or_else(|| self.first_empty()) else {
                return;
            };
            match &mut self.view.slots[slot] {
                Some(target) => {
                    let moved = stack.count.min(limit - target.count);
                    target.count += moved;
                    stack.count -= moved;
                }
                empty => {
                    *empty = Some(stack);
                    return;
                }
            }
        }
    }

    fn crafting_result(&self, index: usize) -> bool {
        index == 0 && (self.view.window == 0 || self.view.kind == Kind::Crafting)
    }

    /// `SlotCrafting.onPickupFromSlot`: one of each ingredient is used and a
    /// bucket's container goes to the inventory, or back to its emptied grid
    /// slot. The new result is the server's to send.
    fn crafted(&mut self, result: usize) {
        self.view.slots[result] = None;
        let grid = if self.view.window == 0 { 1..=4 } else { 1..=9 };
        for slot in grid {
            let Some(item) = &mut self.view.slots[slot] else {
                continue;
            };
            let container = matches!(item.id, 326 | 327 | 335);
            item.count -= 1;
            if item.count <= 0 {
                self.view.slots[slot] = None;
            }
            if container {
                let bucket = Item {
                    id: 325,
                    count: 1,
                    damage: 0,
                    nbt: None,
                };
                let fits = self.first_empty().is_some()
                    || (0..36).map(|i| self.main_slot(i)).any(|i| {
                        self.view.slots[i]
                            .as_ref()
                            .is_some_and(|t| same_kind(t, &bucket) && t.count < stack_limit(325))
                    });
                if fits {
                    self.add_to_inventory(bucket);
                } else if self.view.slots[slot].is_none() {
                    self.view.slots[slot] = Some(bucket);
                }
            }
        }
    }

    /// `Container.slotClick` mode 2: a number key over a slot. False when the
    /// result cannot be predicted, and no click is sent.
    fn hotbar_swap(&mut self, index: usize, button: u8) -> bool {
        let hotbar = self.view.hotbar(button);
        if button > 8 || hotbar >= self.view.slots.len() {
            return false;
        }
        let hot = self.view.slots[hotbar].clone();
        let current = self.view.slots[index].clone();
        let hot_valid = match &hot {
            Some(item) => match self.slot_accepts(index, item) {
                Some(valid) => valid,
                None => return false,
            },
            None => false,
        };
        let swaps = hot.is_none() || (self.own_slot(index) && hot_valid);
        let empty = if swaps { None } else { self.first_empty() };
        match current {
            Some(stack) if swaps || empty.is_some() => {
                self.view.slots[hotbar] = Some(stack);
                if swaps {
                    self.view.slots[index] = hot;
                } else if let Some(hot) = hot {
                    self.add_to_inventory(hot);
                    self.view.slots[index] = None;
                }
            }
            None if hot_valid => {
                self.view.slots[hotbar] = None;
                self.view.slots[index] = hot;
            }
            _ => {}
        }
        true
    }

    /// Vanilla quick-craft stage = button & 3, type = (button >> 2) & 3.
    fn drag_click(&mut self, index: Option<usize>, button: u8) {
        let stage = button & 3;
        let kind = button >> 2;
        if kind > 1 {
            self.drag = None;
            return;
        }
        if stage == 0 {
            // A fresh start while already dragging resets the server state.
            if self.drag.is_some() {
                self.drag = None;
                return;
            }
            if self.view.cursor.is_some() {
                self.drag = Some(Drag {
                    button: kind,
                    slots: Vec::new(),
                });
            }
            return;
        }
        let Some(mut drag) = self.drag.take() else {
            return;
        };
        if drag.button != kind {
            return;
        }
        let Some(cursor) = self.view.cursor.as_ref() else {
            return;
        };
        if stage == 1 {
            if let Some(index) = index
                && cursor.count as usize > drag.slots.len()
                && !drag.slots.contains(&index)
                && drag_eligible(&self.view, index, cursor)
            {
                drag.slots.push(index);
            }
            self.drag = Some(drag);
        } else if stage == 2 && !drag.slots.is_empty() {
            let cursor = cursor.clone();
            let share = if kind == 0 {
                cursor.count as usize / drag.slots.len()
            } else {
                1
            };
            let mut remaining = i32::from(cursor.count);
            for index in drag.slots.iter().copied() {
                if cursor.count as usize >= drag.slots.len()
                    && drag_eligible(&self.view, index, &cursor)
                {
                    let before = self.view.slots[index].as_ref().map_or(0, |s| s.count);
                    let count = (i32::from(before) + share as i32)
                        .min(i32::from(slot_limit(&self.view, index, &cursor)));
                    remaining -= count - i32::from(before);
                    let mut item = cursor.clone();
                    item.count = count as i8;
                    self.view.slots[index] = Some(item);
                }
            }
            self.view.cursor = (remaining > 0).then(|| {
                let mut item = cursor;
                item.count = remaining as i8;
                item
            });
        }
    }

    /// Container.slotClick mode 6: partial stacks before full stacks, and
    /// button 1 reverses each pass. The clicked slot must be empty.
    fn collect(&mut self, index: usize, reverse: bool) {
        if self.view.slots[index].is_some() {
            return;
        }
        let Some(mut cursor) = self.view.cursor.clone() else {
            return;
        };
        let limit = stack_limit(cursor.id);
        let order: Vec<usize> = if reverse {
            (0..self.view.slots.len()).rev().collect()
        } else {
            (0..self.view.slots.len()).collect()
        };
        for pass in 0..2 {
            for &slot in &order {
                if cursor.count >= limit {
                    break;
                }
                // ContainerPlayer.canMergeSlot excludes the crafting result.
                if self.view.window == 0 && slot == 0 {
                    continue;
                }
                let Some(item) = &mut self.view.slots[slot] else {
                    continue;
                };
                if !same_kind(item, &cursor) || pass == 0 && item.count == stack_limit(item.id) {
                    continue;
                }
                let take = (limit - cursor.count).min(item.count);
                cursor.count += take;
                item.count -= take;
                if item.count <= 0 {
                    self.view.slots[slot] = None;
                }
            }
        }
        self.view.cursor = Some(cursor);
    }

    /// `Container.slotClick` mode 0 on a slot, with `Slot.isItemValid` and
    /// the armour slots' limit of one. Slots of windows this client does not
    /// model accept anything.
    fn pick_up(&mut self, index: usize, right: bool) {
        let limit: i8 = if self.view.window == 0 && (5..=8).contains(&index) {
            1
        } else {
            64
        };
        let valid = self
            .view
            .cursor
            .as_ref()
            .is_some_and(|held| self.slot_accepts(index, held).unwrap_or(true));
        let (slots, cursor) = (&mut self.view.slots, &mut self.view.cursor);
        match (cursor.take(), slots[index].take()) {
            (None, None) => {}
            (None, Some(mut item)) => {
                let mut taken = item.clone();
                taken.count = if right {
                    (item.count + 1) / 2
                } else {
                    item.count
                };
                item.count -= taken.count;
                *cursor = Some(taken);
                slots[index] = (item.count > 0).then_some(item);
            }
            (Some(mut held), None) => {
                let count = (if right { 1 } else { held.count }).min(limit);
                if valid && held.count >= count {
                    let mut placed = held.clone();
                    placed.count = count;
                    held.count -= count;
                    slots[index] = Some(placed);
                }
                *cursor = (held.count > 0).then_some(held);
            }
            (Some(mut held), Some(mut item)) => {
                let max = stack_limit(held.id);
                if valid && same_kind(&held, &item) {
                    let count = (if right { 1 } else { held.count })
                        .min(limit - item.count)
                        .min(max - item.count)
                        .max(0);
                    held.count -= count;
                    item.count += count;
                    slots[index] = Some(item);
                    *cursor = (held.count > 0).then_some(held);
                } else if valid && held.count <= limit {
                    slots[index] = Some(held);
                    *cursor = Some(item);
                } else if !valid
                    && same_kind(&held, &item)
                    && max > 1
                    && item.count > 0
                    && item.count + held.count <= max
                {
                    // An output slot empties into a matching cursor.
                    held.count += item.count;
                    *cursor = Some(held);
                } else {
                    slots[index] = Some(item);
                    *cursor = Some(held);
                }
            }
        }
    }

    /// `ContainerPlayer`/`ContainerChest.transferStackInSlot`, retried while
    /// the slot keeps the same item, as `slotClick` mode 1 does.
    fn shift_click(&mut self, index: usize) -> Option<Item> {
        let original = self.view.slots[index].clone()?;
        let predictable = self.view.window == 0 || matches!(self.view.kind, Kind::Chest(_));
        // Each craft recomputes the result from recipes this client lacks.
        if !predictable || self.crafting_result(index) {
            return None;
        }
        let mut moved_any = false;
        for _ in 0..64 {
            let Some(mut stack) = self.view.slots[index].take() else {
                break;
            };
            let before = stack.count;
            let moved = self.transfer(index, &mut stack);
            let left = stack.count;
            if left > 0 {
                self.view.slots[index] = Some(stack);
            }
            if !moved || left == before {
                break;
            }
            moved_any = true;
            if self.view.slots[index]
                .as_ref()
                .is_none_or(|item| item.id != original.id)
            {
                break;
            }
        }
        moved_any.then_some(original)
    }

    fn transfer(&mut self, index: usize, stack: &mut Item) -> bool {
        let slots = &mut self.view.slots;
        if self.view.window != 0 {
            let n = self.view.container_slots;
            let total = slots.len();
            return if index < n {
                merge(slots, stack, n, total, true)
            } else {
                merge(slots, stack, 0, n, false)
            };
        }
        match index {
            0 => merge(slots, stack, 9, 45, true),
            1..=8 => merge(slots, stack, 9, 45, false),
            _ if (298..=317).contains(&stack.id)
                && slots[5 + (stack.id as usize - 298) % 4].is_none() =>
            {
                let armor = 5 + (stack.id as usize - 298) % 4;
                merge(slots, stack, armor, armor + 1, false)
            }
            9..=35 => merge(slots, stack, 36, 45, false),
            36..=44 => merge(slots, stack, 9, 36, false),
            _ => merge(slots, stack, 9, 45, false),
        }
    }

    /// Mirror the open window's player rows into the player inventory.
    fn sync_player(&mut self) {
        if self.view.window == 0 {
            self.player.clone_from(&self.view.slots);
        } else if self.view.slots.len() >= self.view.container_slots + 36 {
            let start = self.view.container_slots;
            self.player[9..45].clone_from_slice(&self.view.slots[start..start + 36]);
        }
    }

    pub fn confirm(&mut self, fields: &[u8]) -> io::Result<Option<Vec<u8>>> {
        if fields.len() != 4 {
            return Err(io::Error::other("Invalid transaction confirmation"));
        }
        let transaction = i16::from_be_bytes([fields[1], fields[2]]);
        // handleConfirmTransaction: window 0 is always the player's own
        // container; any other window must be the open one.
        let open = fields[0] == self.view.window;
        if !open && fields[0] != 0 {
            return Ok(None);
        }
        // Only a transaction this client numbered can undo a predicted click;
        // server pings (often negative numbers, or window 0 under a chest)
        // are answered and change nothing.
        if !open || !self.pending.contains_key(&transaction) {
            return Ok((fields[3] == 0).then(|| interaction::confirm(fields[0], transaction)));
        }
        let saved = self.pending.remove(&transaction);
        if fields[3] != 0 {
            if self.view.window != 0 && self.view.slots.len() >= self.view.container_slots + 36 {
                self.player[9..45].clone_from_slice(
                    &self.view.slots[self.view.container_slots..self.view.container_slots + 36],
                );
            }
            return Ok(None);
        }
        if let Some(saved) = saved {
            self.view = saved;
            self.sync_player();
        }
        self.drag = None;
        self.pending.clear();
        self.blocked = true;
        Ok(Some(interaction::confirm(fields[0], transaction)))
    }
}

/// S31 `Window Property`: (window, property, value).
pub fn window_property(fields: &[u8]) -> io::Result<(u8, i16, i16)> {
    let [window, a, b, c, d] = fields else {
        return Err(io::Error::other("Invalid window property"));
    };
    Ok((
        *window,
        i16::from_be_bytes([*a, *b]),
        i16::from_be_bytes([*c, *d]),
    ))
}

/// S09 carries a signed byte; vanilla ignores selections outside 0..9.
pub fn server_hotbar(fields: &[u8]) -> io::Result<Option<u8>> {
    let [value] = fields else {
        return Err(io::Error::other("Invalid server hotbar packet length"));
    };
    Ok((*value < 9).then_some(*value))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn server_hotbar_uses_one_signed_byte_and_ignores_out_of_range_slots() {
        for value in 0..=255u8 {
            assert_eq!(
                server_hotbar(&[value]).unwrap(),
                (value < 9).then_some(value)
            );
        }
        assert!(server_hotbar(&[]).is_err());
        assert!(server_hotbar(&[0, 0]).is_err());
        assert!(server_hotbar(&[0x80, 0]).is_err());
    }

    #[test]
    fn window_properties_decode_big_endian_shorts() {
        assert_eq!(window_property(&[3, 0, 2, 0, 200]).unwrap(), (3, 2, 200));
        assert_eq!(window_property(&[1, 255, 255, 0, 1]).unwrap(), (1, -1, 1));
        assert!(window_property(&[1, 0, 2]).is_err());
    }
    #[test]
    fn direct_inventory_indices_and_container_hotbar_stay_synchronized() {
        let mut inv = Inventory::default();
        let item = Some(Item {
            id: 276,
            count: 1,
            damage: 0,
            nbt: None,
        });
        inv.set_slot(254, 0, item.clone());
        assert_eq!(inv.held(), item.as_ref());
        inv.set_slot(
            254,
            39,
            Some(Item {
                id: 310,
                count: 1,
                damage: 0,
                nbt: None,
            }),
        );
        assert_eq!(inv.player[5].as_ref().unwrap().id, 310);
        inv.view.window = 1;
        inv.view.container_slots = 27;
        inv.view.slots = vec![None; 63];
        inv.set_slot(1, 54, item.clone());
        assert_eq!(inv.player[36], item);
        inv.set_slot(
            0,
            37,
            Some(Item {
                id: 4,
                count: 16,
                damage: 0,
                nbt: None,
            }),
        );
        assert_eq!(inv.view.slots[55], inv.player[37]);
    }

    #[test]
    fn pickup_split_merge_and_rejected_transactions_restore_snapshot() {
        let mut inv = Inventory::default();
        inv.player[36] = Some(Item {
            id: 1,
            count: 9,
            damage: 0,
            nbt: None,
        });
        inv.local_open();
        inv.click(36, true).unwrap();
        assert_eq!(inv.view.cursor.as_ref().unwrap().count, 5);
        assert_eq!(inv.view.slots[36].as_ref().unwrap().count, 4);
        inv.click(37, true).unwrap();
        assert_eq!(inv.view.slots[37].as_ref().unwrap().count, 1);
        assert_eq!(inv.view.cursor.as_ref().unwrap().count, 4);
        assert_eq!(
            inv.confirm(&[0, 0, 2, 0]).unwrap().unwrap(),
            [15, 0, 0, 2, 1]
        );
        assert!(inv.view.slots[37].is_none());
        assert_eq!(inv.view.cursor.as_ref().unwrap().count, 5);
        assert!(inv.click(38, false).unwrap().is_none());
    }

    /// NetHandlerPlayClient.handleConfirmTransaction answers an unaccepted
    /// S32 for window 0 (the player inventory, whatever is open) or the open
    /// window. Server pings that match none of our clicks must not discard
    /// pending clicks or block the next ones.
    #[test]
    fn transaction_pings_are_answered_without_touching_clicks() {
        let mut inv = Inventory::default();
        inv.player[36] = stack(1, 9);
        inv.local_open();
        inv.click(36, false).unwrap().unwrap();
        let ours = inv.transaction;
        // A ping on window 0 with a foreign (negative) action number.
        assert_eq!(
            inv.confirm(&[0, 0xff, 0xfe, 0]).unwrap().unwrap(),
            [0x0f, 0, 0xff, 0xfe, 1]
        );
        assert!(inv.view.cursor.is_some(), "the ping undid our click");
        assert!(
            inv.click(37, false).unwrap().is_some(),
            "the ping blocked clicks"
        );
        assert!(inv.pending.contains_key(&ours));
        // With a chest open, window 0 pings are still answered.
        let mut chest = Inventory {
            view: View {
                open: true,
                window: 5,
                container_slots: 27,
                kind: Kind::Chest(3),
                slots: vec![None; 63],
                ..View::default()
            },
            ..Inventory::default()
        };
        chest.view.slots[54] = stack(1, 9);
        chest.click(54, false).unwrap().unwrap();
        assert_eq!(
            chest.confirm(&[0, 0x80, 0x00, 0]).unwrap().unwrap(),
            [0x0f, 0, 0x80, 0x00, 1]
        );
        assert!(chest.view.cursor.is_some());
        assert_eq!(chest.view.window, 5);
        // Accepted confirmations and other windows get no reply.
        assert!(chest.confirm(&[0, 0, 9, 1]).unwrap().is_none());
        assert!(chest.confirm(&[9, 0, 9, 0]).unwrap().is_none());
    }

    fn stack(id: i16, count: i8) -> Option<Item> {
        Some(Item {
            id,
            count,
            damage: 0,
            nbt: None,
        })
    }

    #[test]
    fn full_player_update_reaches_visible_rows_of_an_open_chest() {
        let mut inv = Inventory {
            view: View {
                open: true,
                window: 1,
                container_slots: 27,
                kind: Kind::Chest(3),
                slots: vec![None; 63],
                ..View::default()
            },
            ..Inventory::default()
        };
        let mut chest = vec![None; 63];
        chest[0] = stack(264, 3);
        chest[54] = stack(276, 1);
        inv.items(1, chest);
        let mut player = vec![None; 45];
        player[9] = stack(264, 7);
        player[36] = stack(260, 5);
        player[5] = stack(310, 1);
        inv.items(0, player);
        assert_eq!(
            inv.view.slots[54],
            stack(260, 5),
            "hotbar view retained the old sword"
        );
        assert_eq!(inv.view.slots[27], stack(264, 7));
        assert_eq!(
            inv.view.slots[0],
            stack(264, 3),
            "player update must preserve chest contents"
        );
        assert_eq!(inv.held().unwrap().id, 260);
        assert_eq!(inv.player[5], stack(310, 1));
        assert_eq!(inv.view.window, 1);
    }

    #[test]
    fn rejected_pickup_restores_the_held_item_mirror_before_full_resync() {
        for chest in [false, true] {
            let mut inv = Inventory::default();
            inv.player[36] = stack(1, 9);
            inv.local_open();
            let slot = if chest {
                inv.view = View {
                    open: true,
                    window: 1,
                    container_slots: 27,
                    kind: Kind::Chest(3),
                    slots: vec![None; 63],
                    ..View::default()
                };
                inv.view.slots[54] = stack(1, 9);
                54
            } else {
                36
            };
            inv.click(slot, false).unwrap().unwrap();
            assert!(inv.held().is_none());
            let [a, b] = inv.transaction.to_be_bytes();
            let window = inv.view.window;
            inv.confirm(&[window, a, b, 0]).unwrap().unwrap();
            assert_eq!(
                inv.held().map(|i| (i.id, i.count)),
                Some((1, 9)),
                "rejected click left a ghost held item"
            );
            assert!(inv.view.cursor.is_none());
            assert!(
                inv.click(slot, false).unwrap().is_none(),
                "clicks stay blocked pending authoritative slots"
            );
        }
    }

    fn quick_craft(inv: &mut Inventory, slots: &[i16], right: bool) {
        let base = u8::from(right) * 4;
        for (slot, button) in std::iter::once((-999, base))
            .chain(slots.iter().map(|s| (*s, base + 1)))
            .chain(std::iter::once((-999, base + 2)))
        {
            let packet = inv.click_mode(slot, 5, button).unwrap().unwrap();
            assert_eq!(
                &packet[packet.len() - 2..],
                &[255, 255],
                "mode 5 returns null"
            );
        }
    }

    #[test]
    fn left_drag_shares_original_count_clamps_and_preserves_remainder() {
        let mut inv = Inventory::default();
        inv.local_open();
        inv.view.cursor = stack(1, 10);
        inv.view.slots[9] = stack(1, 63);
        quick_craft(&mut inv, &[9, 10, 10, 11], false);
        assert_eq!(inv.view.slots[9].as_ref().unwrap().count, 64);
        assert_eq!(inv.view.slots[10].as_ref().unwrap().count, 3);
        assert_eq!(inv.view.slots[11].as_ref().unwrap().count, 3);
        assert_eq!(inv.view.cursor.as_ref().unwrap().count, 3);
        assert_eq!(inv.player[10], inv.view.slots[10]);
        assert_eq!(inv.transaction, 6);
    }
    #[test]
    fn right_drag_respects_slot_rules_count_and_single_item_limits() {
        let mut inv = Inventory::default();
        inv.local_open();
        inv.view.cursor = stack(1, 2);
        quick_craft(&mut inv, &[0, 5, 9, 10, 11], true);
        assert!(inv.view.slots[0].is_none() && inv.view.slots[5].is_none());
        assert_eq!(inv.view.slots[9].as_ref().unwrap().count, 1);
        assert_eq!(inv.view.slots[10].as_ref().unwrap().count, 1);
        assert!(inv.view.slots[11].is_none() && inv.view.cursor.is_none());
        inv.view.cursor = stack(310, 1);
        quick_craft(&mut inv, &[5], false);
        assert_eq!(inv.view.slots[5].as_ref().unwrap().id, 310);
        assert!(inv.view.cursor.is_none());
    }
    #[test]
    fn collect_takes_partial_stacks_first_and_obeys_direction_and_nbt() {
        let mut inv = Inventory::default();
        inv.local_open();
        inv.view.cursor = stack(1, 1);
        inv.view.slots[9] = stack(1, 64);
        inv.view.slots[10] = stack(1, 4);
        inv.view.slots[11] = stack(1, 10);
        inv.view.slots[11].as_mut().unwrap().damage = 1;
        inv.view.slots[12] = stack(1, 5);
        inv.view.slots[15] = stack(1, 8);
        inv.view.slots[15].as_mut().unwrap().nbt = Some(horizon_protocol_47::nbt::Tag::Compound(
            vec![("marker".to_owned(), horizon_protocol_47::nbt::Tag::Int(1))],
        ));
        inv.click_mode(13, 6, 1).unwrap().unwrap();
        assert_eq!(inv.view.cursor.as_ref().unwrap().count, 64);
        assert_eq!(inv.view.slots[9].as_ref().unwrap().count, 10);
        assert!(inv.view.slots[10].is_none() && inv.view.slots[12].is_none());
        assert_eq!(inv.view.slots[11].as_ref().unwrap().count, 10);
        assert_eq!(
            inv.view.slots[15].as_ref().unwrap().count,
            8,
            "distinct NBT cannot collect"
        );
        inv.view.cursor = stack(1, 2);
        inv.view.slots[14] = stack(1, 4);
        let before = inv.view.cursor.clone();
        inv.click_mode(14, 6, 0).unwrap();
        assert_eq!(
            inv.view.cursor, before,
            "nonempty clicked slot cannot collect"
        );
    }
    #[test]
    fn rejected_drag_blocks_until_resync_and_clears_gesture_state() {
        let mut inv = Inventory::default();
        inv.local_open();
        inv.view.cursor = stack(264, 8);
        quick_craft(&mut inv, &[9, 10], false);
        let transaction = inv.transaction;
        let [a, b] = transaction.to_be_bytes();
        assert!(inv.confirm(&[0, a, b, 0]).unwrap().is_some());
        assert!(inv.drag.is_none() && inv.blocked);
        assert!(!inv.accepts_gesture(0, 4));
        assert!(inv.click_mode(9, 6, 0).unwrap().is_none());
        inv.items(0, vec![None; 45]);
        inv.set_slot(255, -1, stack(264, 8));
        assert!(inv.accepts_gesture(0, 4));
        quick_craft(&mut inv, &[9, 10], false);
        assert_eq!(inv.view.cursor, None);
    }
    #[test]
    fn unsupported_windows_never_assume_null_will_force_resync() {
        let mut inv = Inventory::default();
        inv.local_open();
        inv.view.window = 1;
        inv.view.kind = Kind::Furnace;
        inv.view.cursor = stack(264, 8);
        assert!(inv.click_mode(-999, 5, 0).unwrap().is_none());
        assert!(inv.click_mode(9, 6, 0).unwrap().is_none());
        assert_eq!(inv.view.cursor.as_ref().unwrap().count, 8);
    }
    #[test]
    fn drag_preview_and_commit_agree_without_mutating_authoritative_view() {
        let mut inv = Inventory::default();
        inv.local_open();
        inv.view.cursor = stack(264, 7);
        let preview = drag_preview(&inv.view, &[9, 10, 11], 0);
        assert!(inv.view.slots[9].is_none());
        quick_craft(&mut inv, &[9, 10, 11], false);
        assert_eq!(preview.cursor, inv.view.cursor);
        assert_eq!(preview.slots, inv.view.slots);
    }

    #[test]
    fn shift_clicks_follow_transfer_stack_in_slot() {
        let mut inv = Inventory::default();
        inv.player[9] = stack(1, 40);
        inv.player[37] = stack(1, 50);
        inv.player[20] = stack(310, 1);
        inv.local_open();
        // Main inventory to the hotbar: top up the 50, then the first empty slot.
        let packet = inv.click_mode(9, 1, 0).unwrap().unwrap();
        assert_eq!(packet[7], 1, "mode byte");
        assert_eq!(inv.view.slots[37].as_ref().unwrap().count, 64);
        assert_eq!(inv.view.slots[36].as_ref().unwrap().count, 26);
        assert!(inv.view.slots[9].is_none());
        assert_eq!(inv.player[36], inv.view.slots[36]);
        // A diamond helmet goes to its empty armour slot.
        inv.click_mode(20, 1, 0).unwrap();
        assert_eq!(inv.view.slots[5].as_ref().unwrap().id, 310);

        // A chest: container to the player rows (last first), and back.
        let mut chest = Inventory::default();
        let mut fields = vec![1];
        horizon_protocol_47::write_string(&mut fields, "minecraft:chest", 32).unwrap();
        horizon_protocol_47::write_string(&mut fields, r#"{"text":"Shop"}"#, 32767).unwrap();
        fields.push(27);
        chest.open_packet(&fields, &|_| "Item Shop".into()).unwrap();
        assert_eq!(chest.view.kind, Kind::Chest(3));
        chest.view.slots[4] = stack(35, 16);
        chest.click_mode(4, 1, 0).unwrap();
        assert_eq!(chest.view.slots[62].as_ref().unwrap().count, 16);
        assert_eq!(chest.player[44].as_ref().unwrap().count, 16);
        let screen = chest.view.screen();
        assert_eq!(screen.size, (176.0, 168.0));
        assert_eq!(screen.slots[27], (27, 8.0, 85.0));
        assert_eq!(chest.view.slot_at(6.5, 18.0), None);
        assert_eq!(chest.view.slot_at(25.5, 18.0), Some(1));
        assert_eq!(chest.view.slot_at(7.5, 17.5), Some(0));
    }

    /// SlotCrafting.onPickupFromSlot: taking the result (click, number key
    /// or Q) uses one of each ingredient and returns a bucket's container.
    /// Shift-clicks repeat crafts the client cannot recompute, so they are
    /// left to the server's correction.
    #[test]
    fn taking_a_crafting_result_uses_the_grid() {
        let mut inv = Inventory::default();
        for grid in 1..=4 {
            inv.player[grid] = stack(5, 2);
        }
        inv.player[0] = stack(58, 1);
        inv.local_open();
        assert!(inv.click_mode(0, 0, 0).unwrap().is_some());
        assert_eq!(inv.view.cursor, stack(58, 1));
        assert!(inv.view.slots[0].is_none());
        for grid in 1..=4 {
            assert_eq!(inv.view.slots[grid], stack(5, 1), "grid {grid}");
        }
        // The server's S2F shows the next result; a number key takes it.
        inv.view.cursor = None;
        inv.view.slots[0] = stack(58, 1);
        inv.click_mode(0, 2, 0).unwrap();
        assert_eq!(inv.view.slots[36], stack(58, 1));
        assert!((1..=4).all(|grid| inv.view.slots[grid].is_none()));
        // A shift-click predicts nothing and sends no item, so a server
        // that crafted anything rejects it and resends the window.
        inv.view.slots[1] = stack(5, 4);
        inv.view.slots[0] = stack(5, 4);
        let before = inv.view.clone();
        let packet = inv.click_mode(0, 1, 0).unwrap().unwrap();
        assert_eq!(packet.last(), Some(&0xff), "no clicked item");
        assert_eq!(inv.view.slots, before.slots);

        // A crafting table: a milk bucket returns its bucket to the hotbar,
        // or to its grid slot when the inventory is full. Q drops the result.
        let mut fields = vec![4];
        horizon_protocol_47::write_string(&mut fields, "minecraft:crafting_table", 32).unwrap();
        horizon_protocol_47::write_string(&mut fields, r#"{"text":"Crafting"}"#, 32767).unwrap();
        fields.push(0);
        let mut table = Inventory::default();
        table.open_packet(&fields, &|_| "Crafting".into()).unwrap();
        table.view.slots[1] = stack(335, 1);
        table.view.slots[2] = stack(353, 3);
        table.view.slots[0] = stack(354, 1);
        table.click_mode(0, 4, 0).unwrap();
        assert!(table.view.slots[0].is_none());
        assert!(table.view.slots[1].is_none());
        assert_eq!(table.view.slots[2], stack(353, 2));
        assert_eq!(table.view.slots[37], stack(325, 1), "first empty: hotbar 0");
        for slot in 10..46 {
            table.view.slots[slot] = stack(4, 64);
        }
        table.view.slots[1] = stack(335, 1);
        table.view.slots[0] = stack(354, 1);
        table.click_mode(0, 0, 0).unwrap();
        assert_eq!(table.view.slots[1], stack(325, 1));
        assert_eq!(table.view.cursor, stack(354, 1));
    }

    /// Container.slotClick mode 0 respects Slot.isItemValid and the armour
    /// slots' stack limit of one; the server accepts the click either way.
    #[test]
    fn pickup_clicks_respect_slot_validity_and_limits() {
        let mut inv = Inventory::default();
        inv.player[6] = stack(307, 1);
        inv.local_open();
        inv.view.cursor = stack(3, 64);
        for slot in [5, 6, 0] {
            let before = inv.view.clone();
            assert!(inv.click_mode(slot, 0, 0).unwrap().is_some());
            assert_eq!(inv.view.slots, before.slots, "slot {slot}");
            assert_eq!(inv.view.cursor, before.cursor, "slot {slot}");
        }
        inv.view.cursor = stack(86, 64);
        inv.click_mode(5, 0, 0).unwrap();
        assert_eq!(inv.view.slots[5], stack(86, 1));
        assert_eq!(inv.view.cursor, stack(86, 63));
        // Same item over a full armour slot: nothing more fits.
        inv.click_mode(5, 0, 0).unwrap();
        assert_eq!(inv.view.slots[5], stack(86, 1));
        // A valid chestplate swaps with the worn one.
        inv.view.cursor = stack(311, 1);
        inv.click_mode(6, 0, 0).unwrap();
        assert_eq!(inv.view.slots[6], stack(311, 1));
        assert_eq!(inv.view.cursor, stack(307, 1));

        // A furnace output takes nothing, but merges into a matching cursor.
        let mut fields = vec![2];
        horizon_protocol_47::write_string(&mut fields, "minecraft:furnace", 32).unwrap();
        horizon_protocol_47::write_string(&mut fields, r#"{"text":"Furnace"}"#, 32767).unwrap();
        fields.push(3);
        let mut furnace = Inventory::default();
        furnace.open_packet(&fields, &|_| "Furnace".into()).unwrap();
        furnace.view.slots[2] = stack(265, 5);
        furnace.view.cursor = stack(3, 1);
        furnace.click_mode(2, 0, 0).unwrap();
        assert_eq!(furnace.view.slots[2], stack(265, 5));
        assert_eq!(furnace.view.cursor, stack(3, 1));
        furnace.view.cursor = stack(265, 10);
        furnace.click_mode(2, 0, 1).unwrap();
        assert!(furnace.view.slots[2].is_none());
        assert_eq!(furnace.view.cursor, stack(265, 15));
    }

    /// Container.slotClick mode 2: a hotbar stack that may not go into the
    /// hovered slot moves to the player's inventory (first partial stacks,
    /// then the first empty slot, hotbar first) instead of swapping.
    #[test]
    fn number_keys_follow_slot_click_mode_two() {
        let mut fields = vec![1];
        horizon_protocol_47::write_string(&mut fields, "minecraft:chest", 32).unwrap();
        horizon_protocol_47::write_string(&mut fields, r#"{"text":"Loot"}"#, 32767).unwrap();
        fields.push(27);
        let mut chest = Inventory::default();
        chest.open_packet(&fields, &|_| "Loot".into()).unwrap();
        for hotbar in 54..63 {
            chest.view.slots[hotbar] = stack(1, 1);
        }
        chest.view.slots[54] = stack(262, 10);
        chest.view.slots[27] = stack(262, 60);
        chest.view.slots[0] = stack(276, 1);
        // Hotbar slot 0 (view 54) takes the sword; the arrows top up main
        // slot 9 (view 27) to 64 and the other 6 go to the first empty slot,
        // main slot 10 (view 28). The chest slot empties.
        assert!(chest.click_mode(0, 2, 0).unwrap().is_some());
        assert_eq!(chest.view.slots[54], stack(276, 1));
        assert_eq!(chest.view.slots[27], stack(262, 64));
        assert_eq!(chest.view.slots[28], stack(262, 6));
        assert!(chest.view.slots[0].is_none());
        assert_eq!(chest.player[36], stack(276, 1));
        // A full inventory: the packet is sent and nothing moves.
        for slot in 27..54 {
            chest.view.slots[slot] = stack(4, 64);
        }
        chest.view.slots[1] = stack(264, 2);
        let before = chest.view.slots.clone();
        assert!(chest.click_mode(1, 2, 1).unwrap().is_some());
        assert_eq!(chest.view.slots, before);

        // Window 0: a sword cannot be worn, so the helmet comes to the hotbar
        // and the sword goes to the first empty slot, the hotbar's slot 1.
        let mut inv = Inventory::default();
        inv.player[5] = stack(310, 1);
        inv.player[36] = stack(276, 1);
        inv.local_open();
        assert!(inv.click_mode(5, 2, 0).unwrap().is_some());
        assert_eq!(inv.view.slots[36], stack(310, 1));
        assert_eq!(inv.view.slots[37], stack(276, 1));
        assert!(inv.view.slots[5].is_none());
        // Boots over the boots slot swap; a pumpkin stack fits the helmet.
        inv.view.slots[8] = stack(313, 1);
        inv.view.slots[38] = stack(301, 1);
        inv.click_mode(8, 2, 2).unwrap();
        assert_eq!(inv.view.slots[8], stack(301, 1));
        assert_eq!(inv.view.slots[38], stack(313, 1));
        inv.view.slots[39] = stack(86, 64);
        inv.click_mode(5, 2, 3).unwrap();
        assert_eq!(inv.view.slots[5], stack(86, 64));
        assert!(inv.view.slots[39].is_none());
        // A crafting result comes to the empty hotbar slot.
        inv.view.slots[0] = stack(5, 4);
        assert!(inv.click_mode(0, 2, 4).unwrap().is_some());
        assert_eq!(inv.view.slots[40], stack(5, 4));
    }

    #[test]
    fn number_keys_drops_and_outside_clicks_predict_vanilla_results() {
        let mut inv = Inventory::default();
        inv.player[10] = stack(264, 5);
        inv.player[38] = stack(1, 3);
        inv.local_open();
        inv.click_mode(10, 2, 2).unwrap();
        assert_eq!(inv.view.slots[38].as_ref().unwrap().id, 264);
        assert_eq!(inv.view.slots[10].as_ref().unwrap().id, 1);
        // An empty armour slot and an empty hotbar slot: C0E, no change.
        let before = inv.view.slots.clone();
        assert!(inv.click_mode(5, 2, 0).unwrap().is_some());
        assert_eq!(inv.view.slots, before);
        inv.click_mode(38, 4, 0).unwrap();
        assert_eq!(inv.view.slots[38].as_ref().unwrap().count, 4);
        inv.click_mode(38, 4, 1).unwrap();
        assert!(inv.view.slots[38].is_none());
        inv.click_mode(10, 0, 0).unwrap();
        inv.click_mode(-999, 0, 1).unwrap();
        assert_eq!(inv.view.cursor.as_ref().unwrap().count, 2);
        inv.click_mode(-999, 0, 0).unwrap();
        assert!(inv.view.cursor.is_none());
    }
}
