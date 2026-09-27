//! Canonical Huffman codes for live frame transport. Raising the minimum
//! positive weight when necessary bounds code lengths without changing data.

use std::{cmp::Reverse, collections::BinaryHeap};

#[derive(Debug)]
pub(super) struct Codes<const N: usize> {
    pub(super) frequencies: [u32; N],
    pub(super) lengths: [u8; N],
    pub(super) codes: [u16; N],
}

impl<const N: usize> Default for Codes<N> {
    fn default() -> Self {
        Self { frequencies: [0; N], lengths: [0; N], codes: [0; N] }
    }
}

impl<const N: usize> Codes<N> {
    pub(super) fn build(&mut self, limit: u8, heap: &mut BinaryHeap<Reverse<(u64, usize)>>) {
        debug_assert!((2..=286).contains(&N));
        debug_assert!((7..=15).contains(&limit));
        let active = self.frequencies.iter().filter(|&&n| n > 0).count();
        let dummy = if self.frequencies[0] == 0 { 0 } else { 1 };
        let mut floor = 1u64;
        loop {
            heap.clear();
            let mut parents = [usize::MAX; 572];
            // Two leaves give a complete tree even for empty or constant data.
            for i in 0..N {
                if self.frequencies[i] > 0 || (active == 0 && i < 2) || (active == 1 && i == dummy)
                {
                    heap.push(Reverse((u64::from(self.frequencies[i]).max(floor), i)));
                }
            }
            let mut next = N;
            while heap.len() > 1 {
                let Reverse((a, i)) = heap.pop().unwrap();
                let Reverse((b, j)) = heap.pop().unwrap();
                parents[i] = next;
                parents[j] = next;
                heap.push(Reverse((a + b, next)));
                next += 1;
            }
            self.lengths.fill(0);
            for i in 0..N {
                let mut node = i;
                while parents[node] != usize::MAX {
                    self.lengths[i] += 1;
                    node = parents[node];
                }
            }
            if *self.lengths.iter().max().unwrap() <= limit {
                break;
            }
            // Rare, heavily skewed inputs may exceed DEFLATE's length limit.
            // Raising only positive weights eventually yields a balanced tree.
            floor *= 2;
        }
        let mut counts = [0u32; 16];
        for &len in &self.lengths {
            if len > 0 {
                counts[len as usize] += 1;
            }
        }
        let mut next = [0u32; 16];
        let mut code = 0;
        for bits in 1..16 {
            code = (code + counts[bits - 1]) << 1;
            next[bits] = code;
        }
        self.codes.fill(0);
        for i in 0..N {
            let len = self.lengths[i] as usize;
            if len > 0 {
                self.codes[i] = (next[len].reverse_bits() >> (32 - len)) as u16;
                next[len] += 1;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn complete<const N: usize>(codes: &Codes<N>, limit: u8) {
        let mut space = 0;
        for (a, &length) in codes.lengths.iter().enumerate() {
            assert!(length <= limit);
            if length == 0 {
                assert_eq!(codes.frequencies[a], 0);
                continue;
            }
            space += 1u32 << (limit - length);
            for (b, &other) in codes.lengths.iter().enumerate() {
                if a != b && other >= length {
                    assert_ne!(codes.codes[a], codes.codes[b] & ((1 << length) - 1));
                }
            }
        }
        assert_eq!(space, 1 << limit);
    }

    #[test]
    fn empty_single_uniform_and_skewed_alphabets_have_bounded_complete_codes() {
        let mut heap = BinaryHeap::new();
        let mut codes = Codes::<286>::default();
        for mode in 0..4 {
            codes.frequencies.fill(0);
            match mode {
                0 => {}
                1 => codes.frequencies[285] = u32::MAX,
                2 => codes.frequencies.fill(1),
                _ => {
                    codes.frequencies[0] = 1;
                    codes.frequencies[1] = 1;
                    for i in 2..45 {
                        codes.frequencies[i] = codes.frequencies[i - 1] + codes.frequencies[i - 2];
                    }
                }
            }
            let frequencies = codes.frequencies;
            codes.build(15, &mut heap);
            complete(&codes, 15);
            assert_eq!(codes.frequencies, frequencies);
        }
        let mut lengths = Codes::<19>::default();
        for i in 0..19 {
            lengths.frequencies[i] = 1 << i;
        }
        lengths.build(7, &mut heap);
        complete(&lengths, 7);
    }
}
