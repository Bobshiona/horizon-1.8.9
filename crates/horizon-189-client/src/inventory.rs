//! Authoritative window snapshots and basic left/right pickup transactions.
use horizon_protocol_47::{interaction, nbt::Item, read_string, read_varint};
use std::io::{self, Cursor, Read};

#[derive(Clone, Debug, Default)]
pub struct View {
    pub open: bool,
    pub window: u8,
    pub container_slots: usize,
    pub slots: Vec<Option<Item>>,
    pub cursor: Option<Item>,
    pub selected: u8,
}

impl View {
    pub fn layout(&self) -> impl Iterator<Item = (i16, f32, f32)> + '_ {
        (0..self.slots.len()).filter_map(|slot| {
            let (x, y) = if self.window == 0 {
                match slot {
                    5..=8 => (-109.0, -36.0 + (slot - 5) as f32 * 18.0),
                    9..=35 => (
                        -81.0 + ((slot - 9) % 9) as f32 * 18.0,
                        -36.0 + ((slot - 9) / 9) as f32 * 18.0,
                    ),
                    36..=44 => (-81.0 + (slot - 36) as f32 * 18.0, 26.0),
                    _ => return None,
                }
            } else {
                let rows = self.container_slots.div_ceil(9);
                let top = -(rows as f32 * 18.0 + 80.0) / 2.0;
                if slot < self.container_slots {
                    (
                        -81.0 + (slot % 9) as f32 * 18.0,
                        top + (slot / 9) as f32 * 18.0,
                    )
                } else {
                    let index = slot - self.container_slots;
                    (
                        -81.0 + (index % 9) as f32 * 18.0,
                        top + rows as f32 * 18.0
                            + 8.0
                            + (index / 9) as f32 * 18.0
                            + if index >= 27 { 8.0 } else { 0.0 },
                    )
                }
            };
            Some((slot as i16, x, y))
        })
    }
    pub fn slot_at(&self, x: f32, y: f32) -> Option<i16> {
        self.layout()
            .find(|&(_, a, b)| x >= a && x < a + 18.0 && y >= b && y < b + 18.0)
            .map(|(s, _, _)| s)
    }
}

pub struct Inventory {
    pub player: Vec<Option<Item>>,
    pub view: View,
    transaction: i16,
    pending: std::collections::HashMap<i16, View>,
    blocked: bool,
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
        }
    }
}

pub fn stack_limit(id: i16) -> i8 {
    match id {
        256..=259
        | 261
        | 267..=279
        | 282..=286
        | 290..=294
        | 298..=317
        | 326..=329
        | 333
        | 335
        | 342..=343
        | 346
        | 354..=355
        | 359
        | 373
        | 386
        | 398
        | 403
        | 407..=408
        | 413
        | 417..=419
        | 422 => 1,
        323 | 325 | 332 | 344 | 368 | 387 | 416 | 425 => 16,
        _ => 64,
    }
}

impl Inventory {
    pub fn held(&self) -> Option<&Item> {
        self.player
            .get(36 + usize::from(self.view.selected))
            .and_then(Option::as_ref)
    }
    pub fn open_packet(&mut self, fields: &[u8]) -> io::Result<()> {
        let mut input = Cursor::new(fields);
        let mut b = [0];
        input.read_exact(&mut b)?;
        let window = b[0];
        let _kind = read_string(&mut input, 32)?;
        let _title = read_string(&mut input, 32767)?;
        input.read_exact(&mut b)?;
        let slots = usize::from(b[0]);
        if slots > 216 {
            return Err(io::Error::other("Oversized container"));
        }
        self.pending.clear();
        self.blocked = false;
        self.view = View {
            open: true,
            window,
            container_slots: slots,
            slots: vec![None; slots + 36],
            cursor: self.view.cursor.take(),
            selected: self.view.selected,
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
            for (to, item) in self.player.iter_mut().zip(&items) {
                *to = item.clone();
            }
        }
        if window == self.view.window {
            self.view.slots = items;
            self.blocked = false;
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
        self.view.slots = self.player.clone();
    }
    pub fn close(&mut self) -> Vec<u8> {
        let packet = interaction::close_window(self.view.window);
        self.view.open = false;
        self.view.cursor = None;
        self.view.window = 0;
        self.view.container_slots = 0;
        self.view.slots = self.player.clone();
        self.pending.clear();
        self.blocked = false;
        packet
    }
    pub fn click(&mut self, slot: i16, right: bool) -> io::Result<Option<Vec<u8>>> {
        if self.blocked || !self.view.open || self.pending.len() >= 128 {
            return Ok(None);
        }
        let Some(index) = usize::try_from(slot)
            .ok()
            .filter(|&i| i < self.view.slots.len())
        else {
            return Ok(None);
        };
        self.transaction = self.transaction.wrapping_add(1);
        let original = self.view.slots[index].clone();
        let packet = interaction::click(
            self.view.window,
            slot,
            right,
            self.transaction,
            original.as_ref(),
        )?;
        self.pending.insert(self.transaction, self.view.clone());
        let target = &mut self.view.slots[index];
        match (&mut self.view.cursor, target) {
            (cursor @ None, target) => {
                *cursor = target.take();
                if right && let Some(item) = cursor {
                    let take = (item.count + 1) / 2;
                    if item.count > take {
                        let mut rest = item.clone();
                        rest.count -= take;
                        *target = Some(rest);
                    }
                    item.count = take;
                }
            }
            (Some(cursor), target @ None) => {
                let mut placed = cursor.clone();
                placed.count = if right { 1 } else { cursor.count };
                cursor.count -= placed.count;
                *target = Some(placed);
            }
            (Some(cursor), Some(target))
                if cursor.id == target.id
                    && cursor.damage == target.damage
                    && cursor.nbt == target.nbt =>
            {
                let transfer = (if right { 1 } else { cursor.count })
                    .min(stack_limit(target.id).saturating_sub(target.count));
                target.count += transfer;
                cursor.count -= transfer;
            }
            (cursor, target) => std::mem::swap(cursor, target),
        }
        if self.view.cursor.as_ref().is_some_and(|i| i.count <= 0) {
            self.view.cursor = None;
        }
        if self.view.window == 0 {
            self.player.clone_from(&self.view.slots);
        }
        Ok(Some(packet))
    }
    pub fn confirm(&mut self, fields: &[u8]) -> io::Result<Option<Vec<u8>>> {
        if fields.len() != 4 {
            return Err(io::Error::other("Invalid transaction confirmation"));
        }
        let transaction = i16::from_be_bytes([fields[1], fields[2]]);
        if fields[0] != self.view.window {
            return Ok(None);
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
        }
        self.pending.clear();
        self.blocked = true;
        Ok(Some(interaction::confirm(fields[0], transaction)))
    }
}

/// Window property packets are read by the transport but remain available
/// for future furnace/enchantment UI implementations.
pub fn server_hotbar(fields: &[u8]) -> io::Result<u8> {
    let mut input = Cursor::new(fields);
    let value = read_varint(&mut input)?;
    if !(0..=8).contains(&value) || input.position() != fields.len() as u64 {
        return Err(io::Error::other("Invalid server hotbar"));
    }
    Ok(value as u8)
}

#[cfg(test)]
mod tests {
    use super::*;
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
}
