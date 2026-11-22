//! Compact section storage. Protocol 47 sends every section as 4096 16-bit
//! states and two 2048-byte light planes (12 KiB); most sections hold a
//! handful of distinct states and uniform light, so they are kept with as few
//! bits as their contents need: one value for a uniform section, 4- or 8-bit
//! indices into a palette, or the raw array once more than 256 states appear.

/// The 4096 block states of a section, indexed `y << 8 | z << 4 | x`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum States {
    /// Every block is this state (all air, all stone).
    Uniform(u16),
    /// `bits`-wide indices (4 or 8) into `palette`, packed into 64-bit words.
    Indexed {
        bits: u32,
        palette: Vec<u16>,
        words: Box<[u64]>,
    },
    /// More than 256 distinct states.
    Direct(Box<[u16; 4096]>),
}

const fn words(bits: u32) -> usize {
    4096 * bits as usize / 64
}

impl States {
    /// Pack raw states, choosing the smallest form that holds them.
    pub fn new(raw: &[u16; 4096]) -> Self {
        let mut palette = raw.to_vec();
        palette.sort_unstable();
        palette.dedup();
        let bits = match palette.len() {
            1 => return States::Uniform(raw[0]),
            ..=16 => 4,
            ..=256 => 8,
            _ => return States::Direct(Box::new(*raw)),
        };
        // The sorted copy held all 4096 states; keep only the palette.
        palette.shrink_to_fit();
        let mut words = vec![0u64; words(bits)].into_boxed_slice();
        for (index, &state) in raw.iter().enumerate() {
            let slot = palette
                .binary_search(&state)
                .expect("state is in its palette") as u64;
            put(&mut words, bits, index, slot);
        }
        States::Indexed {
            bits,
            palette,
            words,
        }
    }

    #[inline]
    pub fn get(&self, index: usize) -> u16 {
        match self {
            States::Uniform(state) => *state,
            States::Indexed {
                bits,
                palette,
                words,
            } => palette[take(words, *bits, index)],
            States::Direct(states) => states[index],
        }
    }

    /// Change one state, widening the storage when the palette fills up.
    pub fn set(&mut self, index: usize, state: u16) {
        match self {
            States::Uniform(current) if *current == state => {}
            States::Uniform(current) => {
                let mut words = vec![0u64; words(4)].into_boxed_slice();
                put(&mut words, 4, index, 1);
                *self = States::Indexed {
                    bits: 4,
                    palette: vec![*current, state],
                    words,
                };
            }
            States::Indexed {
                bits,
                palette,
                words,
            } => {
                let slot = match palette.iter().position(|&known| known == state) {
                    Some(slot) => slot,
                    None if palette.len() < 1 << *bits => {
                        palette.push(state);
                        palette.len() - 1
                    }
                    None => {
                        let mut raw = [0u16; 4096];
                        self.unpack(&mut raw);
                        raw[index] = state;
                        *self = States::new(&raw);
                        return;
                    }
                };
                put(words, *bits, index, slot as u64);
            }
            States::Direct(states) => states[index] = state,
        }
    }

    /// All 4096 states, for loops that read every block of a section.
    pub fn unpack(&self, out: &mut [u16; 4096]) {
        match self {
            States::Uniform(state) => out.fill(*state),
            States::Indexed {
                bits,
                palette,
                words,
            } => {
                let per_word = (64 / bits) as usize;
                let mask = (1u64 << bits) - 1;
                for (chunk, &word) in out.chunks_exact_mut(per_word).zip(words.iter()) {
                    for (slot, state) in chunk.iter_mut().enumerate() {
                        *state = palette[((word >> (slot as u32 * bits)) & mask) as usize];
                    }
                }
            }
            States::Direct(states) => out.copy_from_slice(&states[..]),
        }
    }

    /// Bytes on the heap.
    pub fn heap_bytes(&self) -> usize {
        match self {
            States::Uniform(_) => 0,
            States::Indexed { palette, words, .. } => palette.capacity() * 2 + words.len() * 8,
            States::Direct(_) => 8192,
        }
    }
}

#[inline]
fn take(words: &[u64], bits: u32, index: usize) -> usize {
    let bit = index * bits as usize;
    ((words[bit / 64] >> (bit % 64)) & ((1u64 << bits) - 1)) as usize
}

#[inline]
fn put(words: &mut [u64], bits: u32, index: usize, slot: u64) {
    let bit = index * bits as usize;
    let mask = ((1u64 << bits) - 1) << (bit % 64);
    let word = &mut words[bit / 64];
    *word = (*word & !mask) | (slot << (bit % 64));
}

/// A 4-bit light plane (2048 bytes, low nibble first), or one level for all of it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Nibbles {
    Uniform(u8),
    Full(Box<[u8; 2048]>),
}

impl Nibbles {
    pub fn new(raw: &[u8; 2048]) -> Self {
        let first = raw[0];
        if first & 15 == first >> 4 && raw.iter().all(|&byte| byte == first) {
            Nibbles::Uniform(first & 15)
        } else {
            Nibbles::Full(Box::new(*raw))
        }
    }

    #[inline]
    pub fn get(&self, index: usize) -> u8 {
        match self {
            Nibbles::Uniform(level) => *level,
            Nibbles::Full(raw) => (raw[index >> 1] >> ((index & 1) * 4)) & 15,
        }
    }

    pub fn heap_bytes(&self) -> usize {
        match self {
            Nibbles::Uniform(_) => 0,
            Nibbles::Full(_) => 2048,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn raw(f: impl Fn(usize) -> u16) -> [u16; 4096] {
        std::array::from_fn(f)
    }

    fn all(states: &States) -> [u16; 4096] {
        let mut out = [0; 4096];
        states.unpack(&mut out);
        for (index, &state) in out.iter().enumerate() {
            assert_eq!(states.get(index), state, "index {index}");
        }
        out
    }

    #[test]
    fn each_form_holds_exactly_what_it_was_given() {
        let uniform = raw(|_| 1 << 4);
        assert_eq!(States::new(&uniform), States::Uniform(1 << 4));
        assert_eq!(all(&States::new(&uniform)), uniform);

        let few = raw(|i| [0, 16, 32, 48, 0x123][i % 5]);
        let packed = States::new(&few);
        assert!(matches!(packed, States::Indexed { bits: 4, .. }));
        assert_eq!(packed.heap_bytes(), 2048 + 5 * 2);
        assert_eq!(all(&packed), few);

        let many = raw(|i| (i % 200) as u16 * 16);
        let packed = States::new(&many);
        assert!(matches!(packed, States::Indexed { bits: 8, .. }));
        assert_eq!(all(&packed), many);

        let every = raw(|i| i as u16);
        let packed = States::new(&every);
        assert!(matches!(packed, States::Direct(_)));
        assert_eq!(all(&packed), every);
    }

    #[test]
    fn setting_states_widens_the_palette_and_keeps_every_other_block() {
        let mut expected = raw(|_| 0);
        let mut states = States::new(&expected);
        // Uniform -> 4 bits -> 8 bits -> direct, checking every block at each step.
        for (step, index) in (0..300).map(|n| (n, n * 13 % 4096)) {
            let state = (step as u16 + 1) * 16;
            states.set(index, state);
            expected[index] = state;
            let form = match &states {
                States::Uniform(_) => 0,
                States::Indexed { bits, .. } => *bits,
                States::Direct(_) => 16,
            };
            let distinct = step + 2;
            let want = if distinct <= 16 {
                4
            } else if distinct <= 256 {
                8
            } else {
                16
            };
            assert_eq!(form, want, "after {distinct} distinct states");
            if step % 37 == 0 || distinct == 17 || distinct == 257 {
                assert_eq!(all(&states), expected);
            }
        }
        assert_eq!(all(&states), expected);
        // Setting a block to its own state changes nothing.
        let mut uniform = States::Uniform(7);
        uniform.set(5, 7);
        assert_eq!(uniform, States::Uniform(7));
    }

    #[test]
    fn light_planes_keep_both_nibbles_and_collapse_only_when_uniform() {
        assert_eq!(Nibbles::new(&[0xff; 2048]), Nibbles::Uniform(15));
        assert_eq!(Nibbles::new(&[0; 2048]), Nibbles::Uniform(0));
        // 0xf0 is two different levels per byte: not uniform.
        let mixed = Nibbles::new(&[0xf0; 2048]);
        assert_eq!((mixed.get(0), mixed.get(1)), (0, 15));
        let mut raw = [0x33; 2048];
        raw[2047] = 0x43;
        let almost = Nibbles::new(&raw);
        assert_eq!(
            (almost.get(4094), almost.get(4095), almost.get(0)),
            (3, 4, 3)
        );
        assert_eq!(almost.heap_bytes(), 2048);
    }
}
