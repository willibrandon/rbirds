//! Bounded, private POSIX images for a local macOS terminal.
//!
//! The terminal unlinks each name after copying its pixels. An occupied name is
//! never overwritten: its caller can use inline output until it is consumed.
//! Names remain registered for emergency cleanup even after their writer closes.
//! Darwin's shm_unlink is a generated libsyscall stub (XNU syscalls.master), so
//! the emergency path uses only that syscall, errno and lock-free atomics.
//! This is deliberately not assumed for another operating system's libc.

use super::{os, sys};
use std::ffi::c_int;
use std::fs::File;
use std::io::{self, Read};
use std::os::fd::{AsRawFd, FromRawFd};
use std::sync::atomic::{AtomicBool, AtomicU8, AtomicU64, Ordering};
use std::sync::{Once, OnceLock};

const NAME_SIZE: usize = 32;
const SLOT_COUNT: usize = 16;
static SERIAL: AtomicU64 = AtomicU64::new(0);
static UNLINK_READY: Once = Once::new();
static NAME_SEED: OnceLock<Result<u64, c_int>> = OnceLock::new();
static SLOTS: [Slot; SLOT_COUNT] = [const { Slot::new() }; SLOT_COUNT];

struct Slot {
    claimed: AtomicBool,
    active: AtomicBool,
    name: [AtomicU8; NAME_SIZE],
}

impl Slot {
    const fn new() -> Self {
        Self {
            claimed: AtomicBool::new(false),
            active: AtomicBool::new(false),
            name: [const { AtomicU8::new(0) }; NAME_SIZE],
        }
    }

    fn clear(&self) {
        if !self.active.load(Ordering::Acquire) {
            return;
        }
        let mut name = [0_u8; NAME_SIZE];
        for (byte, stored) in name.iter_mut().zip(&self.name) {
            *byte = stored.load(Ordering::Relaxed);
        }
        // SAFETY: the name is published before active and contains a trailing
        // NUL. Darwin's generated syscall stub retains no user-space state.
        unsafe { sys::shm_unlink(name.as_ptr().cast()) };
        // Keep it active until after unlink, so an intervening signal can
        // finish the same cleanup rather than lose the resource.
        self.active.store(false, Ordering::Release);
    }
}

/// Called before terminal restoration, including from its signal handler.
pub(super) fn cleanup() {
    if !SLOTS.iter().any(|slot| slot.active.load(Ordering::Acquire)) {
        return;
    }
    let saved_errno = super::errno();
    for slot in &SLOTS {
        slot.clear();
    }
    super::set_errno(saved_errno);
}

struct SignalMask(os::sigset_t);

impl SignalMask {
    fn block() -> io::Result<Self> {
        let mut set = 0;
        let mut old = 0;
        // SAFETY: both outputs are live sigset_t values of the native layout.
        // This brief critical section covers creation and registry publication
        // on the live application's single writer thread, not pixel copying.
        unsafe {
            sys::sigfillset(&mut set);
            if sys::sigprocmask(os::SIG_BLOCK, &set, &mut old) < 0 {
                return Err(io::Error::last_os_error());
            }
        }
        Ok(Self(old))
    }
}

impl Drop for SignalMask {
    fn drop(&mut self) {
        // SAFETY: self.0 is the exact mask saved by sigprocmask; no output is
        // requested. Pending signals may run once their old mask is restored.
        unsafe { sys::sigprocmask(os::SIG_SETMASK, &self.0, std::ptr::null_mut()) };
    }
}

/// One reusable name. At most one unread image can exist under it.
#[derive(Debug)]
pub struct SharedImage {
    slot: usize,
    name: [u8; NAME_SIZE],
    seed: u64,
}

impl SharedImage {
    pub fn new() -> io::Result<Self> {
        // A PID alone can coincide with a process on an SSH client's machine.
        // Random names make the local-memory probe independent across hosts.
        let seed = (*NAME_SEED.get_or_init(|| {
            let mut bytes = [0; 8];
            File::open("/dev/urandom")
                .and_then(|mut file| file.read_exact(&mut bytes))
                .map(|()| u64::from_ne_bytes(bytes))
                .map_err(|error| error.raw_os_error().unwrap_or(super::EIO))
        }))
        .map_err(io::Error::from_raw_os_error)?;
        // Resolve the Mach-O lazy import and its errno path before any shared
        // object exists. A signal handler must never enter the dynamic loader.
        UNLINK_READY.call_once(|| {
            let saved_errno = super::errno();
            // SAFETY: an empty POSIX name is invalid and cannot name a resource.
            // This call only primes the syscall stub and its failure path.
            unsafe { sys::shm_unlink(c"".as_ptr()) };
            super::set_errno(saved_errno);
        });
        let slot = SLOTS
            .iter()
            .position(|slot| {
                slot.claimed
                    .compare_exchange(false, true, Ordering::AcqRel, Ordering::Relaxed)
                    .is_ok()
            })
            .ok_or_else(|| io::Error::from_raw_os_error(super::ENOMEM))?;
        let mut image = Self { slot, name: [0; NAME_SIZE], seed };
        image.fresh_name();
        Ok(image)
    }

    fn fresh_name(&mut self) {
        self.name[..3].copy_from_slice(b"/rb");
        let values = [
            (u64::from(std::process::id()), 8),
            (self.seed ^ SERIAL.fetch_add(1, Ordering::Relaxed), 16),
        ];
        let mut at = 3;
        for (value, digits) in values {
            for shift in (0..digits).rev() {
                self.name[at] = b"0123456789abcdef"[((value >> (shift * 4)) & 15) as usize];
                at += 1;
            }
        }
        for (stored, byte) in SLOTS[self.slot].name.iter().zip(self.name) {
            stored.store(byte, Ordering::Relaxed);
        }
    }

    pub fn name(&self) -> &[u8] {
        &self.name[..27]
    }

    /// Whether the terminal still has an unread object. A successful Kitty
    /// query must also consume it before shared transport is enabled.
    pub fn pending(&self) -> io::Result<bool> {
        // SAFETY: the fixed name is NUL terminated; O_RDONLY creates nothing.
        let fd = unsafe { sys::shm_open(self.name.as_ptr().cast(), os::O_RDONLY) };
        if fd < 0 {
            let error = io::Error::last_os_error();
            return if error.raw_os_error() == Some(os::ENOENT) { Ok(false) } else { Err(error) };
        }
        // SAFETY: shm_open returned this fresh descriptor; File closes it once.
        drop(unsafe { File::from_raw_fd(fd) });
        Ok(true)
    }

    /// Copies packed rows from a checked source region. False means that the
    /// preceding object has not been consumed; the caller must retain it and
    /// use inline transport for this frame. No heap allocation is required.
    pub fn stage(
        &mut self,
        pixels: &[u8],
        stride: usize,
        offset: usize,
        row_bytes: usize,
        rows: usize,
    ) -> io::Result<bool> {
        let invalid = || io::Error::from_raw_os_error(super::EINVAL);
        let length = row_bytes
            .checked_mul(rows)
            .filter(|n| *n > 0 && *n <= isize::MAX as usize)
            .ok_or_else(invalid)?;
        let end = rows
            .checked_sub(1)
            .and_then(|n| n.checked_mul(stride))
            .and_then(|n| n.checked_add(offset))
            .and_then(|n| n.checked_add(row_bytes))
            .ok_or_else(invalid)?;
        if row_bytes > stride || end > pixels.len() {
            return Err(invalid());
        }
        let file = {
            let _mask = SignalMask::block()?;
            let mut file = None;
            for _ in 0..16 {
                // SAFETY: name is terminated, O_EXCL never opens an existing
                // object, and mode_t is promoted to int in Darwin's varargs.
                let fd = unsafe {
                    sys::shm_open(
                        self.name.as_ptr().cast(),
                        os::O_CREAT | os::O_EXCL | os::O_RDWR,
                        0o600 as c_int,
                    )
                };
                if fd >= 0 {
                    SLOTS[self.slot].active.store(true, Ordering::Release);
                    // SAFETY: this newly created descriptor has one owner.
                    file = Some(unsafe { File::from_raw_fd(fd) });
                    break;
                }
                let error = io::Error::last_os_error();
                if error.raw_os_error() != Some(os::EEXIST) {
                    return Err(error);
                }
                if SLOTS[self.slot].active.load(Ordering::Acquire) {
                    return Ok(false);
                }
                // A stale name from an earlier process is never removed.
                self.fresh_name();
            }
            file.ok_or_else(|| io::Error::from_raw_os_error(os::EEXIST))?
        };
        let result = Self::copy_rows(&file, length, pixels, stride, offset, row_bytes, rows);
        if result.is_err() {
            SLOTS[self.slot].clear();
        }
        result.map(|()| true)
    }

    fn copy_rows(
        file: &File,
        length: usize,
        pixels: &[u8],
        stride: usize,
        offset: usize,
        row_bytes: usize,
        rows: usize,
    ) -> io::Result<()> {
        file.set_len(length as u64)?;
        // SAFETY: the file was resized to length and remains open during mmap.
        // The returned mapping is kept private, written once, then unmapped.
        let address = unsafe {
            sys::mmap(
                std::ptr::null_mut(),
                length,
                os::PROT_READ | os::PROT_WRITE,
                os::MAP_SHARED,
                file.as_raw_fd(),
                0,
            )
        };
        if address as usize == usize::MAX {
            return Err(io::Error::last_os_error());
        }
        if !address.is_null() {
            // SAFETY: the successful mapping covers length writable bytes.
            // All source and destination ranges were checked by stage; the
            // private mapping cannot alias pixels or another Rust reference.
            let target = unsafe { std::slice::from_raw_parts_mut(address.cast::<u8>(), length) };
            for row in 0..rows {
                let from = offset + row * stride;
                target[row * row_bytes..(row + 1) * row_bytes]
                    .copy_from_slice(&pixels[from..from + row_bytes]);
            }
        }
        // SAFETY: address/length are the exact successful mapping. No reference
        // into it escapes the preceding scope, and it is unmapped only once.
        let status = unsafe { sys::munmap(address, length) };
        if status < 0 {
            return Err(io::Error::last_os_error());
        }
        if address.is_null() {
            return Err(io::Error::from_raw_os_error(super::ENOMEM));
        }
        Ok(())
    }
}

impl Drop for SharedImage {
    fn drop(&mut self) {
        SLOTS[self.slot].clear();
        SLOTS[self.slot].claimed.store(false, Ordering::Release);
    }
}
