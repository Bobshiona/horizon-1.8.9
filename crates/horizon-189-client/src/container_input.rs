//! Mouse gestures for GuiContainer. Drag packets are queued as one bounded
//! batch on release, so network slot updates cannot interleave its stages.
use crate::inventory::{self, Kind, View};
use std::time::{Duration, Instant};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Click {
    pub slot: i16,
    pub mode: u8,
    pub button: u8,
}
struct Gesture {
    window: u8,
    start: i16,
    button: u8,
    slots: Vec<i16>,
    collect: bool,
}
#[derive(Default)]
pub struct Pointer {
    active: Option<Gesture>,
    last: Option<(u8, i16, u8, Instant)>,
}
impl Pointer {
    pub fn preview(&self, view: &View) -> Option<View> {
        let g = self.active.as_ref()?;
        if g.window != view.window || g.collect || g.slots.is_empty() {
            return None;
        }
        Some(inventory::drag_preview(view, &g.slots, g.button))
    }

    pub fn clear(&mut self) {
        self.active = None;
        self.last = None;
    }
    pub fn press(
        &mut self,
        view: &View,
        slot: Option<i16>,
        button: u8,
        shift: bool,
        now: Instant,
    ) -> Vec<Click> {
        self.active = None;
        let Some(slot) = slot else {
            self.last = None;
            return vec![];
        };
        let double = self.last.is_some_and(|(w, s, b, t)| {
            w == view.window
                && s == slot
                && b == button
                && now.saturating_duration_since(t) < Duration::from_millis(250)
        });
        self.last = Some((view.window, slot, button, now));
        if shift && slot >= 0 {
            return vec![Click {
                slot,
                mode: 1,
                button,
            }];
        }
        if view.cursor.is_none()
            || slot == -999
            || !(view.window == 0 || matches!(view.kind, Kind::Chest(_)))
        {
            return vec![Click {
                slot,
                mode: 0,
                button,
            }];
        }
        self.active = Some(Gesture {
            window: view.window,
            start: slot,
            button,
            slots: vec![],
            collect: double && button == 0,
        });
        vec![]
    }
    pub fn moved(&mut self, view: &View, slot: Option<i16>) {
        let Some(g) = &mut self.active else {
            return;
        };
        if view.window != g.window || !view.open {
            self.active = None;
            return;
        }
        if g.collect {
            return;
        }
        if let (Some(slot), Some(cursor)) = (slot.filter(|s| *s >= 0), &view.cursor)
            && cursor.count as usize > g.slots.len()
            && !g.slots.contains(&slot)
            && inventory::drag_eligible(view, slot as usize, cursor)
        {
            g.slots.push(slot);
        }
    }
    pub fn release(&mut self, view: &View, slot: Option<i16>, button: u8) -> Vec<Click> {
        let Some(g) = self.active.take() else {
            return vec![];
        };
        if view.window != g.window || !view.open || button != g.button {
            return vec![];
        }
        if g.collect {
            return if slot == Some(g.start) {
                vec![Click {
                    slot: g.start,
                    mode: 6,
                    button: 0,
                }]
            } else {
                vec![]
            };
        }
        if g.slots.is_empty() {
            return slot
                .map(|slot| {
                    vec![Click {
                        slot,
                        mode: 0,
                        button,
                    }]
                })
                .unwrap_or_default();
        }
        let base = button * 4;
        let mut out = Vec::with_capacity(g.slots.len() + 2);
        out.push(Click {
            slot: -999,
            mode: 5,
            button: base,
        });
        out.extend(g.slots.into_iter().map(|slot| Click {
            slot,
            mode: 5,
            button: base + 1,
        }));
        out.push(Click {
            slot: -999,
            mode: 5,
            button: base + 2,
        });
        out
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use horizon_protocol_47::nbt::Item;
    fn view() -> View {
        View {
            open: true,
            slots: vec![None; 45],
            cursor: Some(Item {
                id: 264,
                count: 8,
                damage: 0,
                nbt: None,
            }),
            ..View::default()
        }
    }
    #[test]
    fn drag_batches_stages_and_focus_loss_cancels() {
        let v = view();
        let mut p = Pointer::default();
        assert!(p.press(&v, Some(9), 0, false, Instant::now()).is_empty());
        p.moved(&v, Some(9));
        p.moved(&v, Some(10));
        p.moved(&v, Some(9));
        let clicks = p.release(&v, Some(10), 0);
        assert_eq!(
            clicks
                .iter()
                .map(|c| (c.slot, c.mode, c.button))
                .collect::<Vec<_>>(),
            [(-999, 5, 0), (9, 5, 1), (10, 5, 1), (-999, 5, 2)]
        );
        p.press(&v, Some(9), 1, false, Instant::now());
        p.moved(&v, Some(10));
        p.clear();
        assert!(p.release(&v, Some(10), 1).is_empty());
    }
    #[test]
    fn collect_requires_same_window_button_slot_and_time() {
        let mut p = Pointer::default();
        let mut v = view();
        v.cursor = None;
        let t = Instant::now();
        assert_eq!(p.press(&v, Some(9), 0, false, t)[0].mode, 0);
        v.cursor = view().cursor;
        p.press(&v, Some(9), 0, false, t + Duration::from_millis(100));
        assert_eq!(p.release(&v, Some(9), 0)[0].mode, 6);
        p.press(&v, Some(9), 0, false, t + Duration::from_secs(1));
        assert_eq!(p.release(&v, Some(9), 0)[0].mode, 0);
        p.press(&v, Some(9), 0, false, t + Duration::from_secs(2));
        v.window = 3;
        assert!(p.release(&v, Some(9), 0).is_empty());
    }
}
