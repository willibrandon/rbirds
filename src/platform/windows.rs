//! Native Windows console boundary. No Unix ABI or third-party crates.
//!
//! Console records are translated to the byte input understood by the shared
//! parser. A single bounded writer keeps keys serviceable during backpressure.
#![allow(unsafe_code)]

use std::collections::VecDeque;
use std::ffi::{CStr, OsStr, OsString, c_char, c_int, c_void};
use std::io::{self, Write};
use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Condvar, Mutex, OnceLock};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

#[cfg(not(all(target_env = "msvc", any(target_arch = "x86_64", target_arch = "aarch64"))))]
compile_error!("Windows support requires x86_64 or aarch64 with the MSVC toolchain");

#[path = "scan.rs"]
mod scan;
#[path = "trig.rs"]
mod trig;
pub use scan::scan_osc_rgb;
pub use trig::sin_cos;
#[cfg(test)]
#[path = "windows_tests.rs"]
mod tests;

pub type RawFd = i32;
pub const STDIN_FILENO: RawFd = 0;
pub const STDOUT_FILENO: RawFd = 1;
pub const STDERR_FILENO: RawFd = 2;
// These are Win32 error codes, not errno values. Never mix the two domains.
pub const EINTR: i32 = 995;
pub const EIO: i32 = 1117;
pub const EAGAIN: i32 = 997;
pub const EWOULDBLOCK: i32 = EAGAIN;
pub const ENOMEM: i32 = 8;
pub const POLLIN: i16 = 1;
pub const POLLOUT: i16 = 4;

pub trait OsStrExt {
    fn as_bytes(&self) -> &[u8];
}
impl OsStrExt for OsStr {
    fn as_bytes(&self) -> &[u8] {
        self.as_encoded_bytes()
    }
}
// options::parse rejects ill-formed UTF-16 before slicing its UTF-8 arguments.
pub fn os_string_from_bytes(bytes: &[u8]) -> OsString {
    OsString::from(String::from_utf8_lossy(bytes).into_owned())
}

type Handle = *mut c_void;
#[repr(C)]
#[derive(Default)]
struct FileTime {
    low: u32,
    high: u32,
}
#[repr(C)]
#[derive(Clone, Copy, Default)]
struct Coord {
    x: i16,
    y: i16,
}
#[repr(C)]
#[derive(Default)]
struct Rect {
    left: i16,
    top: i16,
    right: i16,
    bottom: i16,
}
#[repr(C)]
#[derive(Default)]
struct ScreenInfo {
    size: Coord,
    cursor: Coord,
    attributes: u16,
    window: Rect,
    maximum: Coord,
}
#[repr(C)]
#[derive(Clone, Copy, Default)]
struct KeyEvent {
    down: i32,
    repeat: u16,
    key: u16,
    scan: u16,
    character: u16,
    controls: u32,
}
#[repr(C)]
#[derive(Clone, Copy)]
struct MouseEvent {
    position: Coord,
    buttons: u32,
    controls: u32,
    flags: u32,
}
#[repr(C)]
#[derive(Clone, Copy)]
union Event {
    key: KeyEvent,
    mouse: MouseEvent,
    storage: [u32; 4],
}
#[repr(C)]
#[derive(Clone, Copy)]
struct InputRecord {
    kind: u16,
    event: Event,
}

// The Win32 layouts these mirror (CONSOLE_SCREEN_BUFFER_INFO, KEY_EVENT_RECORD,
// MOUSE_EVENT_RECORD and INPUT_RECORD in wincontypes.h), checked when compiling.
const _: () = {
    use std::mem::{align_of, offset_of, size_of};
    assert!(size_of::<Coord>() == 4 && align_of::<Coord>() == 2);
    assert!(size_of::<Rect>() == 8 && align_of::<Rect>() == 2);
    assert!(size_of::<ScreenInfo>() == 22 && align_of::<ScreenInfo>() == 2);
    assert!(offset_of!(ScreenInfo, cursor) == 4 && offset_of!(ScreenInfo, attributes) == 8);
    assert!(offset_of!(ScreenInfo, window) == 10 && offset_of!(ScreenInfo, maximum) == 18);
    assert!(size_of::<KeyEvent>() == 16 && align_of::<KeyEvent>() == 4);
    assert!(offset_of!(KeyEvent, repeat) == 4 && offset_of!(KeyEvent, key) == 6);
    assert!(offset_of!(KeyEvent, scan) == 8 && offset_of!(KeyEvent, character) == 10);
    assert!(offset_of!(KeyEvent, controls) == 12);
    assert!(size_of::<MouseEvent>() == 16 && align_of::<MouseEvent>() == 4);
    assert!(offset_of!(MouseEvent, buttons) == 4 && offset_of!(MouseEvent, controls) == 8);
    assert!(offset_of!(MouseEvent, flags) == 12);
    assert!(size_of::<Event>() == 16 && align_of::<Event>() == 4);
    assert!(size_of::<InputRecord>() == 20 && align_of::<InputRecord>() == 4);
    assert!(offset_of!(InputRecord, event) == 4);
};

#[link(name = "kernel32")]
unsafe extern "system" {
    fn CreateEventW(
        attributes: *const c_void,
        manual: i32,
        initial: i32,
        name: *const u16,
    ) -> Handle;
    fn SetEvent(event: Handle) -> i32;
    fn ResetEvent(event: Handle) -> i32;
    fn WaitForMultipleObjects(count: u32, handles: *const Handle, all: i32, timeout: u32) -> u32;
    fn GetCurrentProcess() -> Handle;
    fn GetProcessTimes(
        process: Handle,
        created: *mut FileTime,
        exited: *mut FileTime,
        kernel: *mut FileTime,
        user: *mut FileTime,
    ) -> i32;
    fn GetStdHandle(which: u32) -> Handle;
    fn GetConsoleMode(handle: Handle, mode: *mut u32) -> i32;
    fn SetConsoleMode(handle: Handle, mode: u32) -> i32;
    fn GetConsoleCP() -> u32;
    fn GetConsoleOutputCP() -> u32;
    fn SetConsoleCP(codepage: u32) -> i32;
    fn SetConsoleOutputCP(codepage: u32) -> i32;
    fn GetConsoleScreenBufferInfo(handle: Handle, info: *mut ScreenInfo) -> i32;
    fn GetNumberOfConsoleInputEvents(handle: Handle, count: *mut u32) -> i32;
    fn ReadConsoleInputW(
        handle: Handle,
        records: *mut InputRecord,
        count: u32,
        read: *mut u32,
    ) -> i32;
    fn WriteFile(
        handle: Handle,
        data: *const c_void,
        count: u32,
        written: *mut u32,
        overlapped: *mut c_void,
    ) -> i32;
    fn ReadFile(
        handle: Handle,
        data: *mut c_void,
        count: u32,
        read: *mut u32,
        overlapped: *mut c_void,
    ) -> i32;
    fn SetConsoleCtrlHandler(handler: Option<extern "system" fn(u32) -> i32>, add: i32) -> i32;
    fn CancelSynchronousIo(thread: Handle) -> i32;
    fn GetLastError() -> u32;
    fn SetLastError(error: u32);
}
unsafe extern "C" {
    #[link_name = "strtod"]
    fn c_strtod(text: *const c_char, end: *mut *mut c_char) -> f64;
    fn _errno() -> *mut c_int;
}

fn handle(fd: RawFd) -> io::Result<Handle> {
    let which = match fd {
        0 => -10i32,
        1 => -11,
        2 => -12,
        _ => return Err(io::Error::from_raw_os_error(6)),
    };
    // SAFETY: these are the three documented standard-handle selectors.
    let value = unsafe { GetStdHandle(which as u32) };
    if value.is_null() || value as isize == -1 {
        Err(io::Error::from_raw_os_error(6))
    } else {
        Ok(value)
    }
}
fn check(result: i32) -> io::Result<()> {
    if result == 0 { Err(io::Error::last_os_error()) } else { Ok(()) }
}
pub fn errno() -> i32 {
    // SAFETY: GetLastError reads only the calling thread's error slot.
    unsafe { GetLastError() as i32 }
}
pub fn set_errno(error: i32) {
    // SAFETY: SetLastError writes only the calling thread's error slot.
    unsafe { SetLastError(error as u32) }
}
pub fn strerror(error: i32) -> Vec<u8> {
    io::Error::from_raw_os_error(error).to_string().into_bytes()
}
pub fn perror_message(prefix: &[u8], error: i32) -> Vec<u8> {
    let mut result = prefix.to_vec();
    if !result.is_empty() {
        result.extend_from_slice(b": ");
    }
    result.extend_from_slice(&strerror(error));
    result.push(b'\n');
    result
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Strtod {
    pub value: f64,
    pub consumed: usize,
    pub erange: bool,
}
pub fn strtod(text: &CStr) -> Strtod {
    let mut end = std::ptr::null_mut();
    // SAFETY: the C string and out pointer live throughout the call. _errno
    // points to this thread's CRT slot; end points within this same C string.
    unsafe {
        *_errno() = 0;
        let value = c_strtod(text.as_ptr(), &mut end);
        Strtod {
            value,
            consumed: end.cast_const().offset_from(text.as_ptr()) as usize,
            erange: *_errno() == 34,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub struct Timespec {
    pub tv_sec: i64,
    pub tv_nsec: i64,
}
pub fn monotonic_now() -> Timespec {
    static EPOCH: OnceLock<Instant> = OnceLock::new();
    let elapsed = EPOCH.get_or_init(Instant::now).elapsed();
    Timespec { tv_sec: elapsed.as_secs() as i64, tv_nsec: i64::from(elapsed.subsec_nanos()) }
}
pub fn nanosleep(delay: &Timespec) -> io::Result<()> {
    if delay.tv_sec < 0 || !(0..1_000_000_000).contains(&delay.tv_nsec) {
        return Err(io::Error::from_raw_os_error(87));
    }
    std::thread::sleep(Duration::new(delay.tv_sec as u64, delay.tv_nsec as u32));
    Ok(())
}

#[derive(Default)]
pub struct FrameSleeper;
impl FrameSleeper {
    pub fn new() -> Self {
        Self
    }
    pub fn sleep(&mut self, delay: Duration) {
        std::thread::sleep(delay);
    }
}

pub fn process_cpu_time() -> io::Result<Duration> {
    let mut created = FileTime::default();
    let mut exited = FileTime::default();
    let mut kernel = FileTime::default();
    let mut user = FileTime::default();
    // SAFETY: the current-process pseudo-handle is valid, and every output
    // points to a writable FILETIME (two DWORDs, alignment four).
    check(unsafe {
        GetProcessTimes(GetCurrentProcess(), &mut created, &mut exited, &mut kernel, &mut user)
    })?;
    let ticks = |time: FileTime| (u64::from(time.high) << 32) | u64::from(time.low);
    let total = ticks(kernel) + ticks(user);
    Ok(Duration::new(total / 10_000_000, (total % 10_000_000) as u32 * 100))
}
pub fn time_now() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs() as i64
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct WinSize {
    pub row: u16,
    pub col: u16,
    pub xpixel: u16,
    pub ypixel: u16,
}
pub fn window_size(fd: RawFd) -> io::Result<WinSize> {
    let mut info = ScreenInfo::default();
    // SAFETY: info matches CONSOLE_SCREEN_BUFFER_INFO and is writable.
    check(unsafe { GetConsoleScreenBufferInfo(handle(fd)?, &mut info) })?;
    Ok(WinSize {
        row: (info.window.bottom - info.window.top + 1) as u16,
        col: (info.window.right - info.window.left + 1) as u16,
        ..WinSize::default()
    })
}
/// The buffer cell at the visible window's top left. Mouse records give
/// buffer positions, and the classic console's window can be scrolled down its
/// buffer; Windows Terminal's buffer is the window, so this is (0, 0) there.
fn window_origin() -> Coord {
    let mut info = ScreenInfo::default();
    let Ok(output) = handle(STDOUT_FILENO) else { return Coord::default() };
    // SAFETY: info matches CONSOLE_SCREEN_BUFFER_INFO and is writable.
    if unsafe { GetConsoleScreenBufferInfo(output, &mut info) } == 0 {
        return Coord::default();
    }
    Coord { x: info.window.left, y: info.window.top }
}
pub fn window_size_or_zero(fd: RawFd) -> WinSize {
    window_size(fd).unwrap_or_default()
}
pub fn is_terminal(fd: RawFd) -> bool {
    let Ok(handle) = handle(fd) else { return false };
    let mut mode = 0;
    // SAFETY: mode is a valid writable DWORD.
    unsafe { GetConsoleMode(handle, &mut mode) != 0 }
}
pub fn write_file(path: &OsStr, data: &[u8]) -> io::Result<()> {
    let mut file = std::fs::File::create(path)?;
    file.write_all(data)?;
    file.sync_all()
}
pub fn write(fd: RawFd, data: &[u8]) -> io::Result<usize> {
    let mut written = 0;
    let count = data.len().min(u32::MAX as usize) as u32;
    // SAFETY: the input slice is readable for count bytes; the synchronous
    // call retains neither it nor the writable DWORD out parameter.
    check(unsafe {
        WriteFile(handle(fd)?, data.as_ptr().cast(), count, &mut written, std::ptr::null_mut())
    })?;
    Ok(written as usize)
}
pub fn write_all_quietly(fd: RawFd, mut data: &[u8]) {
    while !data.is_empty() {
        match write(fd, data) {
            Ok(0) | Err(_) => break,
            Ok(n) => data = &data[n..],
        }
    }
}

/// Rust writes console text through the Unicode API. Pipes keep the original
/// bytes, and graphics output continues to use the raw writer above.
pub fn write_text_all_quietly(fd: RawFd, data: &[u8]) {
    if !is_terminal(fd) {
        write_all_quietly(fd, data);
        return;
    }
    match fd {
        STDOUT_FILENO => {
            let mut output = io::stdout().lock();
            let _ = output.write_all(data);
            let _ = output.flush();
        }
        STDERR_FILENO => {
            let _ = io::stderr().lock().write_all(data);
        }
        _ => write_all_quietly(fd, data),
    }
}

#[derive(Default)]
struct Input {
    bytes: VecDeque<u8>,
    surrogate: Option<u16>,
    buttons: u32,
}
static INPUT: Mutex<Input> =
    Mutex::new(Input { bytes: VecDeque::new(), surrogate: None, buttons: 0 });
fn event_count() -> io::Result<u32> {
    let mut count = 0;
    // SAFETY: count is a writable DWORD; the input handle is not retained.
    check(unsafe { GetNumberOfConsoleInputEvents(handle(0)?, &mut count) })?;
    Ok(count)
}
impl Input {
    fn key(&mut self, key: KeyEvent) {
        if key.down == 0 {
            return;
        }
        for _ in 0..key.repeat {
            if key.character != 0 {
                if (0xd800..=0xdbff).contains(&key.character) {
                    self.surrogate = Some(key.character);
                    continue;
                }
                let cp = if let Some(high) = self.surrogate.take() {
                    if (0xdc00..=0xdfff).contains(&key.character) {
                        0x10000 + ((u32::from(high) - 0xd800) << 10) + u32::from(key.character)
                            - 0xdc00
                    } else {
                        u32::from(key.character)
                    }
                } else {
                    u32::from(key.character)
                };
                let mut utf8 = [0; 4];
                self.bytes.extend(
                    char::from_u32(cp).unwrap_or('\u{fffd}').encode_utf8(&mut utf8).as_bytes(),
                );
            } else {
                self.bytes.extend(match key.key {
                    0x25 => &b"\x1b[D"[..],
                    0x26 => b"\x1b[A",
                    0x27 => b"\x1b[C",
                    0x28 => b"\x1b[B",
                    0x24 => b"\x1b[H",
                    0x23 => b"\x1b[F",
                    0x2e => b"\x1b[3~",
                    _ => b"",
                });
            }
        }
    }
    fn mouse(&mut self, mouse: MouseEvent, origin: Coord) {
        let down = mouse.buttons & 0xffff;
        let released = down == 0 && self.buttons != 0 && mouse.flags & 4 == 0;
        let mut button = if mouse.flags & 4 != 0 {
            if (mouse.buttons >> 16) as i16 > 0 { 64 } else { 65 }
        } else if down & 1 != 0 || released && self.buttons & 1 != 0 {
            0
        } else if down & 4 != 0 || released && self.buttons & 4 != 0 {
            1
        } else if down & 2 != 0 || released && self.buttons & 2 != 0 {
            2
        } else {
            3
        };
        if mouse.flags & 1 != 0 {
            button += 32;
        }
        if mouse.controls & 0x10 != 0 {
            button += 4;
        }
        if mouse.controls & 3 != 0 {
            button += 8;
        }
        if mouse.controls & 12 != 0 {
            button += 16;
        }
        self.bytes.extend(
            format!(
                "\x1b[<{button};{};{}{}",
                (i32::from(mouse.position.x) - i32::from(origin.x) + 1).max(1),
                (i32::from(mouse.position.y) - i32::from(origin.y) + 1).max(1),
                if released { 'm' } else { 'M' }
            )
            .bytes(),
        );
        self.buttons = down;
    }
}
pub fn read(fd: RawFd, data: &mut [u8]) -> io::Result<usize> {
    if data.is_empty() {
        return Ok(0);
    }
    if fd != STDIN_FILENO || !is_terminal(fd) {
        let mut got = 0;
        // SAFETY: data is writable for the supplied bounded count.
        check(unsafe {
            ReadFile(
                handle(fd)?,
                data.as_mut_ptr().cast(),
                data.len().min(u32::MAX as usize) as u32,
                &mut got,
                std::ptr::null_mut(),
            )
        })?;
        return Ok(got as usize);
    }
    let mut input = INPUT.lock().unwrap_or_else(|e| e.into_inner());
    if input.bytes.is_empty() {
        let count = event_count()?.min(64);
        if count != 0 {
            let mut records = [InputRecord { kind: 0, event: Event { storage: [0; 4] } }; 64];
            let mut got = 0;
            // SAFETY: records has room for count INPUT_RECORDs (20-byte ABI).
            check(unsafe {
                ReadConsoleInputW(handle(fd)?, records.as_mut_ptr(), count, &mut got)
            })?;
            let records = &records[..got as usize];
            let origin = if records.iter().any(|record| record.kind == 2) {
                window_origin()
            } else {
                Coord::default()
            };
            for record in records {
                // SAFETY: the discriminator determines the initialized union member.
                match record.kind {
                    1 => input.key(unsafe { record.event.key }),
                    2 => input.mouse(unsafe { record.event.mouse }, origin),
                    _ => {}
                }
            }
        }
    }
    let n = data.len().min(input.bytes.len());
    for byte in &mut data[..n] {
        *byte = input.bytes.pop_front().unwrap();
    }
    Ok(n)
}

#[derive(Clone, Copy, Debug)]
pub struct PollFd {
    pub fd: RawFd,
    pub events: i16,
    pub revents: i16,
}
impl PollFd {
    pub fn new(fd: RawFd, events: i16) -> Self {
        Self { fd, events, revents: 0 }
    }
}
pub fn poll(fds: &mut [PollFd], timeout_ms: i32) -> io::Result<i32> {
    let start = Instant::now();
    let cancellation = cancellation_event()?;
    loop {
        let mut ready = 0;
        for fd in fds.iter_mut() {
            fd.revents = 0;
            if fd.events & POLLIN != 0
                && (exit_requested()
                    || !INPUT.lock().unwrap_or_else(|e| e.into_inner()).bytes.is_empty()
                    || event_count()? > 0)
            {
                fd.revents |= POLLIN;
            }
            if fd.events & POLLOUT != 0
                && active_writer()
                    .is_none_or(|w| !w.state.0.lock().unwrap_or_else(|e| e.into_inner()).busy)
            {
                fd.revents |= POLLOUT;
            }
            ready += i32::from(fd.revents != 0);
        }
        if ready > 0
            || timeout_ms >= 0 && start.elapsed() >= Duration::from_millis(timeout_ms as u64)
        {
            return Ok(ready);
        }
        let mut handles =
            [cancellation.as_raw_handle(), std::ptr::null_mut(), std::ptr::null_mut()];
        let mut count = 1;
        if fds.iter().any(|fd| fd.events & POLLIN != 0) {
            handles[count] = handle(STDIN_FILENO)?;
            count += 1;
        }
        if fds.iter().any(|fd| fd.events & POLLOUT != 0)
            && let Some(writer) = active_writer()
        {
            handles[count] = writer.completed.as_raw_handle();
            count += 1;
        }
        let timeout = if timeout_ms < 0 {
            u32::MAX
        } else {
            let remaining =
                Duration::from_millis(timeout_ms as u64).saturating_sub(start.elapsed());
            remaining.as_nanos().div_ceil(1_000_000).min(u128::from(u32::MAX - 1)) as u32
        };
        // SAFETY: distinct, live console/event handles owned throughout the
        // call; count bounds the initialized part of the array. No polling
        // timer is needed: console input, write completion or Ctrl+C wakes us.
        let result = unsafe { WaitForMultipleObjects(count as u32, handles.as_ptr(), 0, timeout) };
        if result == u32::MAX {
            return Err(io::Error::last_os_error());
        }
        if result == 258 {
            return Ok(0);
        }
        if exit_requested() {
            return Ok(0);
        }
    }
}

fn new_event() -> io::Result<OwnedHandle> {
    // SAFETY: no name or security attributes; returns a new manual-reset event.
    let event = unsafe { CreateEventW(std::ptr::null(), 1, 0, std::ptr::null()) };
    if event.is_null() {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: ownership of this fresh handle transfers to OwnedHandle.
    Ok(unsafe { OwnedHandle::from_raw_handle(event) })
}

static CANCELLATION_EVENT: OnceLock<Result<OwnedHandle, i32>> = OnceLock::new();
fn cancellation_event() -> io::Result<&'static OwnedHandle> {
    CANCELLATION_EVENT
        .get_or_init(|| new_event().map_err(|e| e.raw_os_error().unwrap_or(EIO)))
        .as_ref()
        .map_err(|&code| io::Error::from_raw_os_error(code))
}

#[derive(Default)]
struct WriteState {
    job: Option<(RawFd, Vec<u8>)>,
    spare: Vec<u8>,
    result: Option<Result<usize, i32>>,
    busy: bool,
    stop: bool,
}
struct Writer {
    state: Arc<(Mutex<WriteState>, Condvar)>,
    completed: Arc<OwnedHandle>,
    thread: std::thread::JoinHandle<()>,
}
static WRITER: OnceLock<Result<Writer, i32>> = OnceLock::new();
fn active_writer() -> Option<&'static Writer> {
    WRITER.get().and_then(|result| result.as_ref().ok())
}
impl Writer {
    fn new() -> io::Result<Self> {
        let state = Arc::new((Mutex::new(WriteState::default()), Condvar::new()));
        let completed = Arc::new(new_event()?);
        let done = completed.clone();
        let worker = state.clone();
        let thread = std::thread::Builder::new().name("rbirds-output".into()).spawn(move || {
            loop {
                let (lock, wake) = &*worker;
                let mut state = lock.lock().unwrap_or_else(|e| e.into_inner());
                while state.job.is_none() && !state.stop {
                    state = wake.wait(state).unwrap_or_else(|e| e.into_inner());
                }
                if state.stop {
                    return;
                }
                let (fd, mut bytes) = state.job.take().unwrap();
                drop(state);
                let result = write(fd, &bytes).map_err(|e| e.raw_os_error().unwrap_or(EIO));
                let mut state = lock.lock().unwrap_or_else(|e| e.into_inner());
                state.result = Some(result);
                state.busy = false;
                bytes.clear();
                state.spare = bytes;
                // SAFETY: the shared event is live until this worker exits.
                unsafe {
                    SetEvent(done.as_raw_handle());
                }
                wake.notify_all();
            }
        })?;
        Ok(Self { state, completed, thread })
    }
    fn stop(&self) -> bool {
        let start = Instant::now();
        let drained;
        loop {
            let mut state = self.state.0.lock().unwrap_or_else(|e| e.into_inner());
            if !state.busy || start.elapsed() >= Duration::from_millis(250) {
                drained = !state.busy;
                state.stop = true;
                self.state.1.notify_all();
                drop(state);
                break;
            }
            drop(state);
            std::thread::sleep(Duration::from_millis(1));
        }
        // Cancellation is retried until the worker exits, covering the race
        // between taking a job and entering the synchronous OS write. A write
        // that can't be cancelled is left behind after a second, so teardown
        // still restores the console modes; the cleanup sequences are skipped,
        // since they would queue behind it.
        let deadline = Instant::now() + Duration::from_secs(1);
        while !self.thread.is_finished() && Instant::now() < deadline {
            // SAFETY: JoinHandle owns a live thread handle throughout this call.
            unsafe {
                CancelSynchronousIo(self.thread.as_raw_handle());
            }
            std::thread::sleep(Duration::from_millis(1));
        }
        drained && self.thread.is_finished()
    }
}
pub fn write_nonblocking(fd: RawFd, data: &[u8]) -> io::Result<usize> {
    let writer = WRITER
        .get_or_init(|| Writer::new().map_err(|e| e.raw_os_error().unwrap_or(EIO)))
        .as_ref()
        .map_err(|&code| io::Error::from_raw_os_error(code))?;
    let mut state = writer.state.0.lock().unwrap_or_else(|e| e.into_inner());
    // Stopped, the writer takes no more work, and a write it had in hand may
    // have been cancelled with ERROR_OPERATION_ABORTED, which is the EINTR
    // alias here. Callers retry EINTR, so both report EIO instead.
    if state.stop {
        return Err(io::Error::from_raw_os_error(EIO));
    }
    if let Some(result) = state.result.take() {
        return result.map_err(io::Error::from_raw_os_error);
    }
    if !state.busy {
        let count = data.len().min(64 * 1024);
        let mut bytes = std::mem::take(&mut state.spare);
        bytes.try_reserve_exact(count).map_err(|_| io::Error::from_raw_os_error(ENOMEM))?;
        bytes.extend_from_slice(&data[..count]);
        // SAFETY: this writer owns the event; the state lock orders reset
        // before the worker can complete the next job and signal it again.
        check(unsafe { ResetEvent(writer.completed.as_raw_handle()) })?;
        state.job = Some((fd, bytes));
        state.busy = true;
        writer.state.1.notify_one();
    }
    Err(io::Error::from_raw_os_error(EAGAIN))
}

pub const ALT_SCREEN_ON: &[u8] = b"\x1b[?1049h";
pub const ALT_SCREEN_OFF: &[u8] = b"\x1b[?1049l";
pub const CURSOR_HIDE: &[u8] = b"\x1b[?25l";
pub const CURSOR_SHOW: &[u8] = b"\x1b[?25h";
pub const SYNC_UPDATE_END: &[u8] = b"\x1b[?2026l";
pub const MOUSE_REPORT_ON: &[u8] = b"\x1b[?1003h\x1b[?1006h";
pub const MOUSE_REPORT_OFF: &[u8] = b"\x1b[?1006l\x1b[?1003l";
pub const KITTY_FREE_IMAGES: &[u8] = b"\x1b_Ga=d,d=A,q=2\x1b\\";
static RAW: AtomicBool = AtomicBool::new(false);
static RESTORED: AtomicBool = AtomicBool::new(false);
static ALT: AtomicBool = AtomicBool::new(false);
static SPRITES: AtomicBool = AtomicBool::new(false);
static CANCELLED: AtomicBool = AtomicBool::new(false);
static SIXEL_MODE: AtomicU32 = AtomicU32::new(0);
static INPUT_MODE: AtomicU32 = AtomicU32::new(0);
static OUTPUT_MODE: AtomicU32 = AtomicU32::new(0);
static INPUT_CP: AtomicU32 = AtomicU32::new(0);
static OUTPUT_CP: AtomicU32 = AtomicU32::new(0);
extern "system" fn control(kind: u32) -> i32 {
    if kind <= 1 {
        CANCELLED.store(true, Ordering::Release);
        if let Some(Ok(event)) = CANCELLATION_EVENT.get() {
            // SAFETY: the static event remains live for the process lifetime.
            unsafe {
                SetEvent(event.as_raw_handle());
            }
        }
        1
    } else {
        0
    }
}
pub fn install_signal_handlers() {
    let _ = cancellation_event();
    // SAFETY: control is a static system-ABI callback; it sets an atomic and
    // signals an already-created event without acquiring a mutex.
    unsafe {
        SetConsoleCtrlHandler(Some(control), 1);
    }
}
pub fn default_sigpipe() {}
pub fn exit_requested() -> bool {
    CANCELLED.load(Ordering::Acquire)
}
pub fn mark_sprites_uploaded() {
    SPRITES.store(true, Ordering::Release);
}
pub fn enable_sixel_mode(was_enabled: bool) {
    SIXEL_MODE.store(if was_enabled { 1 } else { 2 }, Ordering::Release);
    write_all_quietly(1, b"\x1b[?80h");
}
pub fn enter_terminal() -> io::Result<()> {
    let input = handle(0)?;
    let output = handle(1)?;
    let (mut imode, mut omode) = (0, 0);
    // SAFETY: valid DWORD out parameters, console handles, and documented flags.
    unsafe {
        check(GetConsoleMode(input, &mut imode))?;
        check(GetConsoleMode(output, &mut omode))?;
        INPUT_MODE.store(imode, Ordering::Relaxed);
        OUTPUT_MODE.store(omode, Ordering::Relaxed);
        INPUT_CP.store(GetConsoleCP(), Ordering::Relaxed);
        OUTPUT_CP.store(GetConsoleOutputCP(), Ordering::Relaxed);
        // Preserve processed input (Ctrl+C), disable line/echo/Quick Edit,
        // and enable mouse/window records. ReadConsoleInputW is Unicode.
        check(SetConsoleMode(input, (imode & !(2 | 4 | 0x40 | 0x200)) | 1 | 8 | 0x10 | 0x80))?;
        RAW.store(true, Ordering::Release);
        let configured = check(SetConsoleMode(output, omode | 1 | 4))
            .and_then(|_| check(SetConsoleCP(65001)))
            .and_then(|_| check(SetConsoleOutputCP(65001)));
        if let Err(error) = configured {
            restore_terminal();
            return Err(error);
        }
    }
    Ok(())
}
pub fn enter_alt_screen() {
    ALT.store(true, Ordering::Release);
    write_all_quietly(1, ALT_SCREEN_ON);
    write_all_quietly(1, CURSOR_HIDE);
    write_all_quietly(1, MOUSE_REPORT_ON);
}
pub fn restore_terminal() {
    if RESTORED.swap(true, Ordering::AcqRel) {
        return;
    }
    let drained = active_writer().is_none_or(Writer::stop);
    if SIXEL_MODE.swap(0, Ordering::AcqRel) == 2 && drained {
        write_all_quietly(1, b"\x1b[?80l");
    }
    if ALT.load(Ordering::Acquire) && drained {
        if SPRITES.load(Ordering::Acquire) {
            write_all_quietly(1, KITTY_FREE_IMAGES);
        }
        write_all_quietly(1, MOUSE_REPORT_OFF);
        write_all_quietly(1, SYNC_UPDATE_END);
        write_all_quietly(1, CURSOR_SHOW);
        write_all_quietly(1, ALT_SCREEN_OFF);
    }
    if RAW.swap(false, Ordering::AcqRel) {
        // SAFETY: restore the exact saved modes/code pages on the standard
        // handles. Failures during teardown cannot usefully be recovered.
        unsafe {
            if let Ok(input) = handle(0) {
                SetConsoleMode(input, INPUT_MODE.load(Ordering::Relaxed));
            }
            if let Ok(output) = handle(1) {
                SetConsoleMode(output, OUTPUT_MODE.load(Ordering::Relaxed));
            }
            SetConsoleCP(INPUT_CP.load(Ordering::Relaxed));
            SetConsoleOutputCP(OUTPUT_CP.load(Ordering::Relaxed));
        }
    }
}
