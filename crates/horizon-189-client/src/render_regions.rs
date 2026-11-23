//! Persistent terrain pages. Allocation math is portable; wgpu owns GPU lifetime
//! and queue ordering on the native backend. Leases keep live ranges disjoint.
use std::{cell::RefCell, ops::Range, rc::Rc};

const QUAD_BYTES: u64 = 48;
const PAGE_QUADS: u32 = 65_536;

struct Ranges {
    free: Vec<Range<u32>>,
}
impl Ranges {
    fn new(capacity: u32) -> Self {
        Self {
            free: std::iter::once(0..capacity).collect(),
        }
    }
    fn take(&mut self, count: u32) -> Option<Range<u32>> {
        if count == 0 {
            return None;
        }
        let index = self.free.iter().position(|r| r.end - r.start >= count)?;
        let start = self.free[index].start;
        let end = start + count;
        if end == self.free[index].end {
            self.free.remove(index);
        } else {
            self.free[index].start = end;
        }
        Some(start..end)
    }
    fn release(&mut self, mut range: Range<u32>) {
        let mut index = self.free.partition_point(|r| r.start < range.start);
        if index > 0 && self.free[index - 1].end == range.start {
            range.start = self.free.remove(index - 1).start;
            index -= 1;
        }
        if index < self.free.len() && self.free[index].start == range.end {
            range.end = self.free.remove(index).end;
        }
        self.free.insert(index, range);
    }
}
struct Page {
    buffer: wgpu::Buffer,
    ranges: RefCell<Ranges>,
    capacity: u32,
}
pub struct Lease {
    page: Rc<Page>,
    range: Range<u32>,
}
impl Lease {
    pub fn buffer(&self) -> &wgpu::Buffer {
        &self.page.buffer
    }
    pub fn page_id(&self) -> usize {
        Rc::as_ptr(&self.page) as usize
    }
    pub fn first_vertex(&self) -> i32 {
        (self.range.start * 4) as i32
    }
}
impl Drop for Lease {
    fn drop(&mut self) {
        self.page.ranges.borrow_mut().release(self.range.clone());
    }
}
pub struct Pool {
    pages: Vec<Rc<Page>>,
    page_quads: u32,
}
impl Default for Pool {
    fn default() -> Self {
        Self {
            pages: Vec::new(),
            page_quads: PAGE_QUADS,
        }
    }
}
impl Pool {
    /// Writes must precede encoding the next terrain pass. Earlier frames have
    /// already been submitted, so queue ordering protects their old contents.
    pub fn upload(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        bytes: &[u8],
    ) -> Result<Lease, &'static str> {
        if bytes.is_empty() || !(bytes.len() as u64).is_multiple_of(QUAD_BYTES) {
            return Err("Invalid terrain quad payload");
        }
        let count = u32::try_from(bytes.len() as u64 / QUAD_BYTES)
            .map_err(|_| "Terrain region exceeds address range")?;
        let max_quads =
            (device.limits().max_buffer_size / QUAD_BYTES).min((i32::MAX as u64 + 1) / 4) as u32;
        if count > max_quads {
            return Err("Terrain region exceeds device buffer limit");
        }
        for page in &self.pages {
            if let Some(range) = page.ranges.borrow_mut().take(count) {
                queue.write_buffer(&page.buffer, u64::from(range.start) * QUAD_BYTES, bytes);
                return Ok(Lease {
                    page: Rc::clone(page),
                    range,
                });
            }
        }
        let capacity = self
            .page_quads
            .max(count.next_power_of_two())
            .min(max_quads);
        let buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Persistent terrain region"),
            size: u64::from(capacity) * QUAD_BYTES,
            usage: wgpu::BufferUsages::VERTEX
                | wgpu::BufferUsages::COPY_DST
                | wgpu::BufferUsages::COPY_SRC,
            mapped_at_creation: false,
        });
        let page = Rc::new(Page {
            buffer,
            ranges: RefCell::new(Ranges::new(capacity)),
            capacity,
        });
        let range = page
            .ranges
            .borrow_mut()
            .take(count)
            .expect("new page fits payload");
        queue.write_buffer(&page.buffer, 0, bytes);
        self.pages.push(Rc::clone(&page));
        Ok(Lease { page, range })
    }
    /// Retain one empty standard page for the next rebuild; release the rest.
    pub fn trim(&mut self) {
        let mut spare = false;
        self.pages.retain(|page| {
            if Rc::strong_count(page) > 1 {
                return true;
            }
            if !spare && page.capacity == self.page_quads {
                spare = true;
                true
            } else {
                false
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn fragmented_ranges_coalesce_and_live_allocations_never_overlap() {
        let mut ranges = Ranges::new(128);
        let mut live: Vec<Range<u32>> = Vec::new();
        let mut random = 0x189u32;
        for _ in 0..10_000 {
            random = random.wrapping_mul(1664525).wrapping_add(1013904223);
            if random & 3 == 0 && !live.is_empty() {
                let index = random as usize % live.len();
                ranges.release(live.swap_remove(index));
            } else if let Some(range) = ranges.take(1 + (random >> 16) % 19) {
                assert!(
                    live.iter()
                        .all(|old| old.end <= range.start || range.end <= old.start)
                );
                live.push(range);
            }
            let mut all = live.clone();
            all.extend(ranges.free.iter().cloned());
            all.sort_by_key(|r| r.start);
            let mut end = 0;
            for range in all {
                assert_eq!(range.start, end);
                assert!(range.end > range.start);
                end = range.end;
            }
            assert_eq!(end, 128);
        }
        for range in live {
            ranges.release(range);
        }
        assert_eq!(ranges.free.len(), 1);
        assert_eq!(ranges.free[0], 0..128);
        assert_eq!(ranges.take(0), None);
        assert_eq!(ranges.take(129), None);
        assert_eq!(ranges.take(128), Some(0..128));
        assert_eq!(ranges.take(1), None);
    }
}

#[cfg(test)]
mod metal_tests {
    use super::*;
    use std::{error::Error, time::Duration};
    #[test]
    #[ignore = "Requires a native Metal GPU"]
    fn reused_regions_preserve_submitted_data_and_neighboring_live_ranges_on_metal()
    -> Result<(), Box<dyn Error>> {
        let mut descriptor = wgpu::InstanceDescriptor::new_without_display_handle();
        descriptor.backends = wgpu::Backends::METAL;
        let instance = wgpu::Instance::new(descriptor);
        let adapter = pollster::block_on(instance.request_adapter(&Default::default()))?;
        assert_eq!(adapter.get_info().backend, wgpu::Backend::Metal);
        let (device, queue) = pollster::block_on(adapter.request_device(&Default::default()))?;
        let mut pool = Pool {
            pages: Vec::new(),
            page_quads: 8,
        };
        assert!(pool.upload(&device, &queue, &[]).is_err());
        assert!(pool.upload(&device, &queue, &[0; 49]).is_err());
        let first = pool.upload(&device, &queue, &[0x11; 96])?;
        let neighbor = pool.upload(&device, &queue, &[0x22; 144])?;
        let spill = pool.upload(&device, &queue, &[0x33; 192])?;
        assert_eq!(first.page_id(), neighbor.page_id());
        assert_ne!(first.page_id(), spill.page_id());
        assert_eq!(neighbor.first_vertex(), 8);
        let old_page = first.page_id();
        let readback = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Terrain region lifetime readback"),
            size: 336,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut encoder = device.create_command_encoder(&Default::default());
        encoder.copy_buffer_to_buffer(first.buffer(), 0, &readback, 0, 96);
        queue.submit([encoder.finish()]);
        drop(first);
        let replacement = pool.upload(&device, &queue, &[0x44; 96])?;
        assert_eq!(replacement.page_id(), old_page);
        assert_eq!(replacement.first_vertex(), 0);
        let mut encoder = device.create_command_encoder(&Default::default());
        encoder.copy_buffer_to_buffer(replacement.buffer(), 0, &readback, 96, 96);
        encoder.copy_buffer_to_buffer(neighbor.buffer(), 96, &readback, 192, 144);
        let submission = queue.submit([encoder.finish()]);
        let (send, receive) = std::sync::mpsc::channel();
        readback.map_async(wgpu::MapMode::Read, .., move |result| {
            let _ = send.send(result);
        });
        device.poll(wgpu::PollType::Wait {
            submission_index: Some(submission),
            timeout: Some(Duration::from_secs(5)),
        })?;
        receive.recv_timeout(Duration::from_secs(5))??;
        let bytes = readback.get_mapped_range(..)?;
        assert!(
            bytes[..96].iter().all(|&v| v == 0x11),
            "submitted copy lost old data"
        );
        assert!(
            bytes[96..192].iter().all(|&v| v == 0x44),
            "reuse did not upload replacement"
        );
        assert!(
            bytes[192..].iter().all(|&v| v == 0x22),
            "live neighbor was overwritten"
        );
        drop(bytes);
        readback.unmap();
        pool.trim();
        assert_eq!(pool.pages.len(), 2);
        drop((replacement, neighbor, spill));
        pool.trim();
        assert_eq!(pool.pages.len(), 1);
        let whole = pool.upload(&device, &queue, &[0x55; 384])?;
        assert_eq!(whole.first_vertex(), 0);
        assert_eq!(pool.pages.len(), 1);
        Ok(())
    }
}
