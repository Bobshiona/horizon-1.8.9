//! Raw mouse motion on macOS. The GameController framework's `GCMouse`
//! reports each mouse's movement in device counts, before the system's pointer
//! acceleration, and on a queue of our own, so motion is summed the moment it
//! arrives rather than when the window's event loop next runs.

use block2::RcBlock;
use dispatch2::{DispatchQueue, DispatchQueueAttr, DispatchRetained};
use objc2::rc::Retained;
use objc2_game_controller::{GCDevice, GCMouse, GCMouseInput};
use std::ptr::NonNull;
use std::sync::Arc;
use std::sync::atomic::{AtomicI64, AtomicU64, Ordering};

/// Motion is summed in fixed point (1/256 count) so no fraction is lost and the
/// queue and the game never share a lock.
const UNIT: f64 = 256.0;

#[derive(Default)]
struct Totals {
    x: AtomicI64,
    y: AtomicI64,
    events: AtomicU64,
}

type Moved = dyn Fn(NonNull<GCMouseInput>, f32, f32);

/// Raw motion from every connected mouse. Lives on the main thread.
pub struct RawMouse {
    totals: Arc<Totals>,
    queue: DispatchRetained<DispatchQueue>,
    handler: RcBlock<Moved>,
    mice: Vec<Retained<GCMouse>>,
}

impl RawMouse {
    pub fn new() -> Self {
        let totals = Arc::new(Totals::default());
        let sink = totals.clone();
        let handler: RcBlock<Moved> =
            RcBlock::new(move |_mouse: NonNull<GCMouseInput>, dx: f32, dy: f32| {
                if !(dx.is_finite() && dy.is_finite()) {
                    return;
                }
                sink.x.fetch_add(fixed(f64::from(dx)), Ordering::Relaxed);
                // GameController's y grows upward; AppKit's (and the game's) downward.
                sink.y.fetch_add(fixed(-f64::from(dy)), Ordering::Relaxed);
                sink.events.fetch_add(1, Ordering::Relaxed);
            });
        let queue = DispatchQueue::new("raw mouse motion", DispatchQueueAttr::SERIAL);
        let mut raw = Self {
            totals,
            queue,
            handler,
            mice: Vec::new(),
        };
        raw.refresh();
        raw
    }

    /// Start listening to mice connected since the last call (the framework
    /// finds them asynchronously after launch). Call on the main thread.
    pub fn refresh(&mut self) {
        // SAFETY: a class property read; the array is retained.
        let mice = unsafe { GCMouse::mice() };
        for index in 0..mice.count() {
            let mouse = mice.objectAtIndex(index);
            if self
                .mice
                .iter()
                .any(|known| Retained::as_ptr(known) == Retained::as_ptr(&mouse))
            {
                continue;
            }
            // SAFETY: the queue is serial and outlives the handler's use of it
            // (the handlers are removed in `drop`); the block only touches
            // atomics and is copied by the setter.
            unsafe {
                mouse.setHandlerQueue(&self.queue);
                if let Some(input) = mouse.mouseInput() {
                    input.setMouseMovedHandler(RcBlock::as_ptr(&self.handler));
                }
            }
            self.mice.push(mouse);
        }
    }

    /// Mice being listened to.
    pub fn mice(&self) -> usize {
        self.mice.len()
    }

    /// Motion reports received so far.
    pub fn events(&self) -> u64 {
        self.totals.events.load(Ordering::Relaxed)
    }

    /// Motion since the last call, in mouse counts: x to the right, y down.
    pub fn take(&self) -> (f64, f64) {
        let x = self.totals.x.swap(0, Ordering::Relaxed);
        let y = self.totals.y.swap(0, Ordering::Relaxed);
        (x as f64 / UNIT, y as f64 / UNIT)
    }
}

impl Default for RawMouse {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for RawMouse {
    fn drop(&mut self) {
        for mouse in &self.mice {
            // SAFETY: clearing the handler this type installed.
            unsafe {
                if let Some(input) = mouse.mouseInput() {
                    input.setMouseMovedHandler(std::ptr::null_mut());
                }
            }
        }
    }
}

fn fixed(counts: f64) -> i64 {
    (counts * UNIT).round() as i64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixed_point_keeps_fractions_and_signs() {
        assert_eq!(fixed(1.0), 256);
        assert_eq!(fixed(-0.5), -128);
        assert_eq!(fixed(0.001), 0);
        let totals = Totals::default();
        totals.x.fetch_add(fixed(1.25), Ordering::Relaxed);
        totals.x.fetch_add(fixed(-0.25), Ordering::Relaxed);
        assert_eq!(totals.x.load(Ordering::Relaxed) as f64 / UNIT, 1.0);
    }
}
