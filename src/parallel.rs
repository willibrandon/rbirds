//! Work spread over threads for big-flock mode (docs/DEVIATIONS.md D-006).
//!
//! cbirds runs on one thread, and so does rbirds unless `--big-flock` asks
//! for more. The work handed out here writes each item from shared, read-only
//! inputs and never depends on the order items are done in, so the result is
//! exactly the same on any number of threads.

#![forbid(unsafe_code)]

use std::num::NonZero;
use std::sync::{Mutex, PoisonError};
use std::thread;

/// The threads big-flock mode uses: half the cores the system offers. The
/// terminal needs the rest to keep up. With every core busy, a terminal
/// drawing tens of thousands of birds falls behind and the flock stalls.
pub fn big_flock_threads() -> usize {
    (thread::available_parallelism().map_or(1, NonZero::get) / 2).max(1)
}

/// Calls `work(first, block)` for consecutive blocks of `block` items, where
/// `first` is the index of the block's first item. With more than one thread
/// the blocks are handed out to whichever thread is free, the caller's
/// included; with one, or with a single block, they run in order on the
/// caller's thread and nothing is spawned. A thread the system refuses leaves
/// the work to the others.
pub fn for_each_block<T, F>(threads: usize, items: &mut [T], block: usize, work: F)
where
    T: Send,
    F: Fn(usize, &mut [T]) + Sync,
{
    let mut nothing = vec![(); threads.max(1)];
    for_each_block_with(&mut nothing, items, block, |(), first, chunk| work(first, chunk));
}

/// [`for_each_block`] on one thread for each of `scratch`'s entries, each
/// thread passing its own to `work` with every block it takes: storage a
/// thread needs while it works, kept by the caller from one call to the next.
pub fn for_each_block_with<S, T, F>(scratch: &mut [S], items: &mut [T], block: usize, work: F)
where
    S: Send,
    T: Send,
    F: Fn(&mut S, usize, &mut [T]) + Sync,
{
    let block = block.max(1);
    let threads = scratch.len().min(items.len().div_ceil(block));
    if threads <= 1 {
        if let Some(own) = scratch.first_mut() {
            for (k, chunk) in items.chunks_mut(block).enumerate() {
                work(own, k * block, chunk);
            }
        }
        return;
    }
    let queue = Mutex::new(items.chunks_mut(block).enumerate());
    let run = |own: &mut S| {
        loop {
            // No work runs while the lock is held, so it is never poisoned.
            let next = queue.lock().unwrap_or_else(PoisonError::into_inner).next();
            let Some((k, chunk)) = next else { break };
            work(own, k * block, chunk);
        }
    };
    let (own, others) = scratch[..threads].split_first_mut().expect("at least two threads");
    thread::scope(|scope| {
        for theirs in others {
            if thread::Builder::new().spawn_scoped(scope, move || run(theirs)).is_err() {
                break;
            }
        }
        run(own);
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_item_is_visited_once_with_its_index() {
        for threads in [1, 2, 3, 8] {
            for block in [1, 3, 64] {
                let mut items = vec![0_usize; 1000];
                for_each_block(threads, &mut items, block, |first, chunk| {
                    for (offset, item) in chunk.iter_mut().enumerate() {
                        *item += first + offset + 1;
                    }
                });
                assert!(items.iter().enumerate().all(|(i, &v)| v == i + 1), "{threads} {block}");
            }
        }
    }

    #[test]
    fn each_thread_keeps_its_own_scratch() {
        let mut scratch = vec![0_usize; 4];
        let mut items = vec![1_usize; 999];
        for_each_block_with(&mut scratch, &mut items, 10, |taken, _, chunk| {
            *taken += chunk.len();
            chunk.iter_mut().for_each(|item| *item = 2);
        });
        assert_eq!(scratch.iter().sum::<usize>(), 999);
        assert!(items.iter().all(|&item| item == 2));
    }

    #[test]
    fn nothing_to_do_is_fine() {
        let mut items: [u8; 0] = [];
        for_each_block(4, &mut items, 16, |_, _| panic!("no blocks"));
    }
}
