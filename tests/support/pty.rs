//! A scripted terminal for lifecycle tests (docs/PORTING.md §4.C).
//!
//! [`run`] opens a pseudoterminal of an explicit size, starts a program on its
//! slave side with a cleared environment, and plays the terminal on the
//! master side: it records every byte the program writes, answers terminal
//! queries from a reply script (whole or in timed fragments), and performs a
//! scripted sequence of actions (input, resizes, signals, pauses in reading,
//! attribute snapshots), each released by an output marker or a delay after
//! the previous one. It returns the exit (code or signal), the raw
//! transcript, and the slave's attributes before, during and after.
//!
//! Everything is bounded: a run past its deadline is killed with SIGKILL and
//! reported as timed out; the child is always reaped and every descriptor
//! closed when the run returns. The harness holds the slave open for the
//! whole run, so the terminal's attributes survive the program and output
//! the program left in the terminal is drained after it exits.
//!
//! The program is not given the pseudoterminal as its controlling terminal
//! (no `setsid`; see `rbirds::platform::pty`), which no case depends on.
//!
//! [`cases`] holds the lifecycle cases, parameterized over the program under
//! test, with the behavior characterized from the C reference as assertions.

use std::collections::VecDeque;
use std::ffi::{OsStr, OsString, c_int};
use std::os::fd::{AsRawFd, OwnedFd};
use std::os::unix::process::{CommandExt, ExitStatusExt};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::time::{Duration, Instant};

use rbirds::platform::{self, PollFd, Termios, WinSize, pty::Pty};

/// A program under test and the name it is invoked as (`argv[0]`), which
/// both programs use as the prefix of their messages.
#[derive(Clone, Debug)]
pub struct Subject {
    pub exe: PathBuf,
    pub name: String,
}

/// Where one of the program's standard streams goes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stream {
    /// The pseudoterminal's slave, as in a real terminal.
    Pty,
    /// `/dev/null`.
    Null,
    /// A pipe the harness reads into [`Outcome::captured`].
    Capture,
    /// A pipe whose other end is already closed: as standard input, end of
    /// file at once; as an output, every write fails with `EPIPE`.
    ClosedPipe,
}

/// An answer to a terminal query: each time `query` appears in the output,
/// the fragments are written to the program, the first at once and each
/// later one `gap` after the previous.
#[derive(Clone, Debug)]
pub struct Reply {
    pub query: Vec<u8>,
    pub fragments: Vec<Vec<u8>>,
    pub gap: Duration,
}

impl Reply {
    pub fn whole(query: &[u8], answer: &[u8]) -> Reply {
        Reply { query: query.to_vec(), fragments: vec![answer.to_vec()], gap: Duration::ZERO }
    }
}

/// What releases a script step, measured from the previous step (or the
/// start of the run for the first).
#[derive(Clone, Debug)]
pub enum Wait {
    /// Released once `marker` appears in output written since then.
    Output(Vec<u8>),
    /// Released once this many more bytes have been written.
    Bytes(usize),
    /// Released after this long.
    Delay(Duration),
}

/// A scripted action on the terminal side.
#[derive(Clone, Debug)]
pub enum Action {
    /// Types these bytes.
    Input(Vec<u8>),
    /// Changes the window size, as an emulator does (`TIOCSWINSZ` on the master).
    Resize(WinSize),
    /// Sends a signal to the program.
    Signal(c_int),
    /// Records the slave's attributes in [`Outcome::termios_during`].
    SnapshotTermios,
    /// Stops reading the program's output for a while, so it backs up.
    PauseReading(Duration),
    /// Closes the terminal side, as a closed terminal window does.
    CloseMaster,
    /// Does nothing but log: a named point in the transcript.
    Mark(&'static str),
}

#[derive(Clone, Debug)]
pub struct Step {
    pub wait: Wait,
    pub action: Action,
}

impl Step {
    pub fn after_output(marker: &[u8], action: Action) -> Step {
        Step { wait: Wait::Output(marker.to_vec()), action }
    }
    pub fn after_bytes(bytes: usize, action: Action) -> Step {
        Step { wait: Wait::Bytes(bytes), action }
    }
    pub fn after(delay: Duration, action: Action) -> Step {
        Step { wait: Wait::Delay(delay), action }
    }
}

/// One run.
#[derive(Clone, Debug)]
pub struct Spec {
    pub program: PathBuf,
    pub arg0: OsString,
    pub args: Vec<OsString>,
    /// Added to the cleared environment, which holds only `PATH` and
    /// `TERM=xterm-256color`; an entry here may override either.
    pub env: Vec<(OsString, OsString)>,
    pub size: WinSize,
    pub stdin: Stream,
    pub stdout: Stream,
    pub stderr: Stream,
    pub replies: Vec<Reply>,
    pub script: Vec<Step>,
    pub deadline: Duration,
    /// Applied to the fresh terminal's attributes before the program starts.
    pub initial_termios: Option<fn(&mut Termios)>,
    /// Bytes of transcript kept; later output is counted but not stored.
    pub transcript_limit: usize,
}

impl Spec {
    pub fn new(subject: &Subject, args: &[&str]) -> Spec {
        Spec {
            program: subject.exe.clone(),
            arg0: OsString::from(&subject.name),
            args: args.iter().map(OsString::from).collect(),
            env: Vec::new(),
            size: WinSize { row: 24, col: 80, xpixel: 640, ypixel: 384 },
            stdin: Stream::Pty,
            stdout: Stream::Pty,
            stderr: Stream::Pty,
            replies: Vec::new(),
            script: Vec::new(),
            deadline: Duration::from_secs(30),
            initial_termios: None,
            transcript_limit: 256 << 20,
        }
    }

    pub fn env(mut self, key: &str, value: impl AsRef<OsStr>) -> Spec {
        self.env.push((key.into(), value.as_ref().to_owned()));
        self
    }

    pub fn size(mut self, size: WinSize) -> Spec {
        self.size = size;
        self
    }

    pub fn reply(mut self, reply: Reply) -> Spec {
        self.replies.push(reply);
        self
    }

    pub fn step(mut self, step: Step) -> Spec {
        self.script.push(step);
        self
    }
}

/// How the program ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Exit {
    Code(i32),
    Signal(i32),
}

impl Exit {
    fn of(status: ExitStatus) -> Exit {
        match (status.code(), status.signal()) {
            (Some(code), _) => Exit::Code(code),
            (None, Some(signal)) => Exit::Signal(signal),
            (None, None) => unreachable!("an exit status is a code or a signal"),
        }
    }
}

/// Something the harness did, with when and where in the transcript.
#[derive(Clone, Debug)]
pub struct Event {
    pub at: Duration,
    /// Transcript length at the time.
    pub offset: usize,
    pub what: String,
}

/// What a run produced.
#[derive(Debug)]
pub struct Outcome {
    /// `None` when the run timed out and was killed.
    pub exit: Option<Exit>,
    pub timed_out: bool,
    pub transcript: Vec<u8>,
    /// Bytes read from the terminal, including any beyond the stored limit.
    pub total_output: usize,
    /// What a [`Stream::Capture`] stream received.
    pub captured: Vec<u8>,
    pub termios_before: Termios,
    pub termios_during: Vec<Termios>,
    /// `None` only if the attributes could not be read after the run.
    pub termios_after: Option<Termios>,
    pub events: Vec<Event>,
    /// From spawn to exit.
    pub run_time: Duration,
    pub steps_done: usize,
}

const QUIET_AFTER_EXIT: Duration = Duration::from_millis(250);
const TICK: Duration = Duration::from_millis(5);

struct Pending {
    due: Instant,
    order: u64,
    bytes: Vec<u8>,
    label: String,
}

struct ReplyState {
    scanned: usize,
}

fn stdio_for(stream: Stream, slave: &OwnedFd, input: bool) -> std::io::Result<Stdio> {
    Ok(match stream {
        Stream::Pty => Stdio::from(slave.try_clone()?),
        Stream::Null => Stdio::null(),
        Stream::Capture => Stdio::piped(),
        Stream::ClosedPipe => {
            let (reader, writer) = std::io::pipe()?;
            if input {
                drop(writer);
                Stdio::from(reader)
            } else {
                drop(reader);
                Stdio::from(writer)
            }
        }
    })
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() {
        return Some(0);
    }
    haystack.windows(needle.len()).position(|w| w == needle)
}

struct Runner<'a> {
    spec: &'a Spec,
    start: Instant,
    master: Option<OwnedFd>,
    transcript: Vec<u8>,
    total_output: usize,
    events: Vec<Event>,
    pending: Vec<Pending>,
    order: u64,
    replies: Vec<ReplyState>,
    termios_during: Vec<Termios>,
    paused_until: Option<Instant>,
    last_data: Instant,
}

impl Runner<'_> {
    fn log(&mut self, what: String) {
        let event = Event { at: self.start.elapsed(), offset: self.total_output, what };
        self.events.push(event);
    }

    fn queue(&mut self, due: Instant, bytes: Vec<u8>, label: String) {
        self.order += 1;
        self.pending.push(Pending { due, order: self.order, bytes, label });
    }

    /// Reads whatever the program has written; true if anything arrived.
    fn read_available(&mut self) -> bool {
        let Some(master) = &self.master else { return false };
        let fd = master.as_raw_fd();
        let mut buffer = [0u8; 65536];
        let mut any = false;
        // Bounded per call so writes and the script stay serviced.
        for _ in 0..64 {
            match platform::read(fd, &mut buffer) {
                Ok(0) => break,
                Ok(n) => {
                    any = true;
                    self.total_output += n;
                    let room = self.spec.transcript_limit.saturating_sub(self.transcript.len());
                    self.transcript.extend_from_slice(&buffer[..n.min(room)]);
                }
                // EAGAIN: drained. EIO: Linux's answer once the slave side
                // is gone, which cannot happen while the harness holds it.
                Err(_) => break,
            }
        }
        if any {
            self.last_data = Instant::now();
        }
        any
    }

    fn answer_queries(&mut self) {
        for index in 0..self.spec.replies.len() {
            let reply = &self.spec.replies[index];
            loop {
                let from = self.replies[index].scanned;
                let Some(at) = find(&self.transcript[from..], &reply.query) else {
                    // Keep a tail that could be the start of a split query.
                    let keep = reply.query.len().saturating_sub(1);
                    self.replies[index].scanned =
                        from.max(self.transcript.len().saturating_sub(keep));
                    break;
                };
                self.replies[index].scanned = from + at + reply.query.len();
                let now = Instant::now();
                let fragments = reply.fragments.clone();
                let gap = reply.gap;
                let label = format!("reply to {}", escape(&reply.query));
                self.log(format!("query seen: {}", escape(&reply.query)));
                for (i, fragment) in fragments.into_iter().enumerate() {
                    self.queue(now + gap * i as u32, fragment, format!("{label} [{i}]"));
                }
            }
        }
    }

    /// Writes due input, earliest first; a partial write keeps the rest.
    fn write_due(&mut self) {
        let Some(master) = &self.master else { return };
        let fd = master.as_raw_fd();
        let now = Instant::now();
        loop {
            let next = self
                .pending
                .iter()
                .enumerate()
                .filter(|(_, p)| p.due <= now)
                .min_by_key(|(_, p)| (p.due, p.order))
                .map(|(i, _)| i);
            let Some(i) = next else { break };
            match platform::write(fd, &self.pending[i].bytes) {
                Ok(n) if n == self.pending[i].bytes.len() => {
                    let done = self.pending.remove(i);
                    self.log(format!("wrote {} ({})", escape(&done.bytes), done.label));
                }
                Ok(n) => {
                    self.pending[i].bytes.drain(..n);
                    break;
                }
                Err(_) => break,
            }
        }
    }

    fn next_due(&self) -> Option<Instant> {
        self.pending.iter().map(|p| p.due).min()
    }
}

/// The child, killed and reaped however the run ends, a harness panic
/// included.
struct Reaper(Child);

impl Drop for Reaper {
    fn drop(&mut self) {
        if let Ok(None) = self.0.try_wait() {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
}

/// Runs `spec` to completion or its deadline.
pub fn run(spec: &Spec) -> Outcome {
    let Pty { master, slave, .. } = platform::pty::open_pty(&spec.size).expect("open a pty");
    if let Some(adjust) = spec.initial_termios {
        let mut termios = platform::tcgetattr(slave.as_raw_fd()).expect("read the attributes");
        adjust(&mut termios);
        platform::tcsetattr(slave.as_raw_fd(), platform::TCSANOW, &termios)
            .expect("set the initial attributes");
    }
    let termios_before = platform::tcgetattr(slave.as_raw_fd()).expect("read the attributes");
    platform::set_nonblocking(master.as_raw_fd(), true).expect("nonblocking master");

    let mut command = Command::new(&spec.program);
    command
        .arg0(&spec.arg0)
        .args(&spec.args)
        .env_clear()
        .env("PATH", std::env::var_os("PATH").unwrap_or_else(|| "/usr/bin:/bin".into()))
        .env("TERM", "xterm-256color");
    for (key, value) in &spec.env {
        command.env(key, value);
    }
    command
        .stdin(stdio_for(spec.stdin, &slave, true).expect("stdin"))
        .stdout(stdio_for(spec.stdout, &slave, false).expect("stdout"))
        .stderr(stdio_for(spec.stderr, &slave, false).expect("stderr"));
    let started = Instant::now();
    let mut reaper = Reaper(
        command.spawn().unwrap_or_else(|e| panic!("cannot start {}: {e}", spec.program.display())),
    );
    // The Command holds the child's copies of the descriptors; let them go.
    drop(command);
    let child = &mut reaper.0;
    let capture = capture_thread(child);

    let mut runner = Runner {
        spec,
        start: started,
        master: Some(master),
        transcript: Vec::new(),
        total_output: 0,
        events: Vec::new(),
        pending: Vec::new(),
        order: 0,
        replies: spec.replies.iter().map(|_| ReplyState { scanned: 0 }).collect(),
        termios_during: Vec::new(),
        paused_until: None,
        last_data: started,
    };
    let mut script: VecDeque<Step> = spec.script.iter().cloned().collect();
    let mut step_began = started;
    // Where the current step's wait began, in bytes read and in transcript.
    let mut step_output = 0usize;
    let mut step_offset = 0usize;
    let mut steps_done = 0usize;
    let mut exited: Option<(ExitStatus, Instant)> = None;
    let mut timed_out = false;
    let deadline = started + spec.deadline;

    loop {
        let now = Instant::now();
        if now >= deadline {
            timed_out = true;
            if exited.is_none() {
                let _ = child.kill();
                let status = child.wait().expect("reap the killed child");
                exited = Some((status, Instant::now()));
            }
            runner.log("deadline passed: killed".into());
            break;
        }
        if exited.is_none()
            && let Some(status) = child.try_wait().expect("wait for the child")
        {
            exited = Some((status, now));
            runner.log(format!("exited: {status}"));
        }
        if runner.paused_until.is_some_and(|until| now >= until) {
            runner.paused_until = None;
            // The quiet period that ends a run counts from here, not from
            // output that arrived before the pause.
            runner.last_data = now;
            runner.log("reading resumed".into());
        }
        if let Some((_, at)) = exited {
            // Drain what the program left in the terminal: after any pause in
            // reading has run its course, until the output has been quiet.
            let quiet_since = runner.last_data.max(at);
            let closed = runner.master.is_none();
            let paused = runner.paused_until.is_some();
            if closed || (!paused && now.duration_since(quiet_since) >= QUIET_AFTER_EXIT) {
                break;
            }
        }

        // Wait for output, writability or the next timed thing, briefly.
        let mut timeout = TICK;
        if let Some(due) = runner.next_due() {
            timeout = timeout.min(due.saturating_duration_since(now));
        }
        if let Some(Step { wait: Wait::Delay(d), .. }) = script.front() {
            timeout = timeout.min((step_began + *d).saturating_duration_since(now));
        }
        if let Some(master) = &runner.master {
            let mut events = 0;
            if runner.paused_until.is_none() {
                events |= platform::POLLIN;
            }
            if runner.next_due().is_some_and(|due| due <= now) {
                events |= platform::POLLOUT;
            }
            let mut fds = [PollFd::new(master.as_raw_fd(), events)];
            let _ = platform::poll(&mut fds, timeout.as_millis() as c_int);
        } else {
            std::thread::sleep(timeout);
        }

        if runner.paused_until.is_none() && runner.read_available() {
            runner.answer_queries();
        }
        runner.write_due();

        // Release every step whose wait is over, in order.
        while let Some(step) = script.front() {
            let released = match &step.wait {
                Wait::Output(marker) => {
                    find(&runner.transcript[step_offset.min(runner.transcript.len())..], marker)
                        .is_some()
                }
                Wait::Bytes(n) => runner.total_output >= step_output + n,
                Wait::Delay(d) => Instant::now() >= step_began + *d,
            };
            if !released {
                break;
            }
            let step = script.pop_front().expect("front exists");
            perform(&mut runner, child, &slave, step.action);
            steps_done += 1;
            step_began = Instant::now();
            step_output = runner.total_output;
            step_offset = runner.transcript.len();
        }
    }

    let (status, ended) = exited.expect("the loop ends only after an exit");
    let captured = capture.map(|t| t.join().unwrap_or_default()).unwrap_or_default();
    let termios_after = platform::tcgetattr(slave.as_raw_fd()).ok();
    Outcome {
        exit: if timed_out && runner.events.iter().all(|e| !e.what.starts_with("exited")) {
            None
        } else {
            Some(Exit::of(status))
        },
        timed_out,
        transcript: runner.transcript,
        total_output: runner.total_output,
        captured,
        termios_before,
        termios_during: runner.termios_during,
        termios_after,
        events: runner.events,
        run_time: ended.duration_since(started),
        steps_done,
    }
    // The slave, the master and the child's pipes close here; the child has
    // been reaped.
}

fn perform(runner: &mut Runner<'_>, child: &Child, slave: &OwnedFd, action: Action) {
    match action {
        Action::Input(bytes) => {
            let label = "script input".to_string();
            runner.queue(Instant::now(), bytes, label);
            runner.write_due();
        }
        Action::Resize(size) => {
            if let Some(master) = &runner.master {
                platform::set_window_size(master.as_raw_fd(), &size).expect("resize");
            }
            runner.log(format!(
                "resized to {}x{} ({}x{} px)",
                size.col, size.row, size.xpixel, size.ypixel
            ));
        }
        Action::Signal(signal) => {
            // The child may already have exited, which is itself a result.
            let result = platform::send_signal(child.id() as i32, signal);
            runner.log(format!("sent signal {signal}: {result:?}"));
        }
        Action::SnapshotTermios => {
            let termios = platform::tcgetattr(slave.as_raw_fd()).expect("read the attributes");
            runner.termios_during.push(termios);
            runner.log("attributes recorded".into());
        }
        Action::PauseReading(duration) => {
            runner.paused_until = Some(Instant::now() + duration);
            runner.log(format!("reading paused for {duration:?}"));
        }
        Action::CloseMaster => {
            runner.master = None;
            runner.log("terminal closed".into());
        }
        Action::Mark(name) => runner.log(format!("mark {name}")),
    }
}

fn capture_thread(child: &mut Child) -> Option<std::thread::JoinHandle<Vec<u8>>> {
    use std::io::Read;
    let mut pipes: Vec<Box<dyn Read + Send>> = Vec::new();
    if let Some(out) = child.stdout.take() {
        pipes.push(Box::new(out));
    }
    if let Some(err) = child.stderr.take() {
        pipes.push(Box::new(err));
    }
    if pipes.is_empty() {
        return None;
    }
    Some(std::thread::spawn(move || {
        let mut all = Vec::new();
        for mut pipe in pipes {
            let _ = pipe.read_to_end(&mut all);
        }
        all
    }))
}

/// Non-overlapping occurrences of `needle` in `haystack`.
pub fn find_all(haystack: &[u8], needle: &[u8]) -> usize {
    if needle.is_empty() {
        return 0;
    }
    let mut count = 0;
    let mut rest = haystack;
    while let Some(at) = find(rest, needle) {
        count += 1;
        rest = &rest[at + needle.len()..];
    }
    count
}

impl Outcome {
    pub fn count(&self, needle: &[u8]) -> usize {
        find_all(&self.transcript, needle)
    }

    pub fn contains(&self, needle: &[u8]) -> bool {
        find(&self.transcript, needle).is_some()
    }

    pub fn position(&self, needle: &[u8]) -> Option<usize> {
        find(&self.transcript, needle)
    }

    /// The transcript offset of the first event whose description starts
    /// with `prefix`.
    pub fn offset_of(&self, prefix: &str) -> Option<usize> {
        self.events.iter().find(|e| e.what.starts_with(prefix)).map(|e| e.offset)
    }

    pub fn event(&self, prefix: &str) -> Option<&Event> {
        self.events.iter().find(|e| e.what.starts_with(prefix))
    }

    /// A readable summary for failure messages and the characterization log.
    pub fn describe(&self) -> String {
        let head = &self.transcript[..self.transcript.len().min(400)];
        let tail = &self.transcript[self.transcript.len().saturating_sub(200)..];
        let mut text = format!(
            "exit {:?}, timed out {}, run {:?}, {} bytes, {} steps\n  head: {}\n  tail: {}\n",
            self.exit,
            self.timed_out,
            self.run_time,
            self.total_output,
            self.steps_done,
            escape(head),
            escape(tail),
        );
        if !self.captured.is_empty() {
            text.push_str(&format!("  captured: {}\n", escape(&self.captured)));
        }
        for event in &self.events {
            text.push_str(&format!("  {:>9.3?} @{:<8} {}\n", event.at, event.offset, event.what));
        }
        text
    }

    /// The program ended with `_exit`/`exit(code)`, not by a signal, in time.
    pub fn assert_exit_code(&self, code: i32) {
        assert!(!self.timed_out, "timed out:\n{}", self.describe());
        assert_eq!(self.exit, Some(Exit::Code(code)), "{}", self.describe());
    }

    /// The terminal's attributes after the run are exactly those before it
    /// (every named field; padding is never compared).
    pub fn assert_attributes_restored(&self) {
        let after = self.termios_after.expect("attributes readable after the run");
        assert_eq!(
            after.raw_mode_view(),
            self.termios_before.raw_mode_view(),
            "raw-mode fields not restored\n{}",
            self.describe()
        );
        assert_eq!(after, self.termios_before, "attributes not restored\n{}", self.describe());
    }
}

/// Bytes with control characters escaped, for messages.
pub fn escape(bytes: &[u8]) -> String {
    let mut out = String::new();
    for &b in bytes {
        match b {
            0x1b => out.push_str("\\e"),
            b'\n' => out.push_str("\\n"),
            b'\r' => out.push_str("\\r"),
            b'\t' => out.push_str("\\t"),
            b'\\' => out.push_str("\\\\"),
            0x20..=0x7e => out.push(b as char),
            _ => out.push_str(&format!("\\x{b:02x}")),
        }
    }
    out
}

pub mod cases {
    //! Lifecycle cases for a program with cbirds' terminal behavior, each run
    //! against a [`Subject`] and asserting what the C reference was observed
    //! to do (tests/pty_reference.rs runs them on cbirds). Every assertion is
    //! a byte sequence, an exit status or an attribute comparison; timings
    //! appear only as generous lower bounds.

    use super::*;
    use rbirds::platform::{
        ALT_SCREEN_OFF, ALT_SCREEN_ON, CURSOR_HIDE, CURSOR_SHOW, KITTY_FREE_IMAGES,
        MOUSE_REPORT_OFF, MOUSE_REPORT_ON, SYNC_UPDATE_END,
    };

    /// The theme queries a default ("theme" palette) live run asks, in order,
    /// each answered or abandoned after 60 ms before the next is asked.
    pub fn theme_queries() -> Vec<Vec<u8>> {
        let mut queries = vec![b"\x1b]11;?\x1b\\".to_vec()];
        for entry in 1..=6 {
            queries.push(format!("\x1b]4;{entry};?\x1b\\").into_bytes());
        }
        queries
    }

    /// What a live run writes on taking the screen, after any queries:
    /// `enter_alt_screen` then the erase below the cursor.
    pub fn screen_taken() -> Vec<u8> {
        [ALT_SCREEN_ON, CURSOR_HIDE, MOUSE_REPORT_ON, b"\x1b[J"].concat()
    }

    /// What `restore_terminal` writes once the alternate screen is on.
    pub fn screen_restored(sprites: bool) -> Vec<u8> {
        let mut out = Vec::new();
        if sprites {
            out.extend_from_slice(KITTY_FREE_IMAGES);
        }
        for part in [MOUSE_REPORT_OFF, SYNC_UPDATE_END, CURSOR_SHOW, ALT_SCREEN_OFF] {
            out.extend_from_slice(part);
        }
        out
    }

    /// Every frame, whatever the renderer, is one synchronized update.
    pub const FRAME_BEGIN: &[u8] = b"\x1b[?2026h";

    /// Asserts a run drew exactly `frames` frames: that many synchronized
    /// updates begun, and each ended (plus the restore's own end).
    pub fn assert_frames(outcome: &Outcome, frames: usize) {
        assert_eq!(outcome.count(FRAME_BEGIN), frames, "frames drawn\n{}", outcome.describe());
        assert_eq!(outcome.count(SYNC_UPDATE_END), frames + 1, "{}", outcome.describe());
    }

    /// Replies making a known theme: background `bg` and palette entries 1-6.
    pub fn theme_replies(bg: [u16; 3], entries: [[u16; 3]; 6]) -> Vec<Reply> {
        let colour = |c: [u16; 3]| format!("rgb:{:04x}/{:04x}/{:04x}", c[0], c[1], c[2]);
        let mut replies = vec![Reply::whole(
            b"\x1b]11;?\x1b\\",
            format!("\x1b]11;{}\x1b\\", colour(bg)).as_bytes(),
        )];
        for (i, entry) in entries.iter().enumerate() {
            let n = i + 1;
            replies.push(Reply::whole(
                format!("\x1b]4;{n};?\x1b\\").as_bytes(),
                format!("\x1b]4;{n};{}\x1b\\", colour(*entry)).as_bytes(),
            ));
        }
        replies
    }

    /// Asserts the frame of every successful live run: exit 0, attributes
    /// back, raw mode seen while running (if recorded), and the transcript
    /// ending with the screen restored.
    pub fn assert_clean_exit(outcome: &Outcome, sprites: bool) {
        outcome.assert_exit_code(0);
        outcome.assert_attributes_restored();
        for during in &outcome.termios_during {
            assert_eq!(*during, outcome.termios_before.raw_mode(), "raw mode while running");
        }
        assert!(
            outcome.transcript.ends_with(&screen_restored(sprites)),
            "the screen is not restored at the end\n{}",
            outcome.describe()
        );
        assert_eq!(outcome.count(ALT_SCREEN_OFF), 1, "restored once\n{}", outcome.describe());
    }

    /// Attributes a fresh terminal would not have, so both raw mode and its
    /// undoing are visible: every bit raw mode clears set, a 7-bit character
    /// size, and distinctive VMIN/VTIME.
    pub fn cooked_with_everything(t: &mut Termios) {
        t.c_iflag |= platform::BRKINT
            | platform::ICRNL
            | platform::INPCK
            | platform::ISTRIP
            | platform::IXON;
        t.c_oflag |= platform::OPOST;
        t.c_lflag |= platform::ECHO | platform::ICANON | platform::IEXTEN | platform::ISIG;
        t.c_cflag &= !platform::CSIZE;
        #[cfg(target_os = "macos")]
        {
            t.c_cflag |= 0x0000_0200; // CS7
        }
        #[cfg(target_os = "linux")]
        {
            t.c_cflag |= 0o040; // CS7
        }
        t.c_cc[platform::VMIN] = 3;
        t.c_cc[platform::VTIME] = 7;
    }

    /// `--frames 20 --seed 1`, default palette, a terminal that answers no
    /// query: the seven theme queries come first, exactly, then the
    /// screen is taken; the run ends by itself with exit 0, the attributes
    /// exactly restored, and the restore sequence last.
    pub fn frames_run(subject: &Subject) -> Outcome {
        let spec = Spec::new(subject, &["--frames", "20", "--seed", "1"])
            .step(Step::after_output(ALT_SCREEN_ON, Action::SnapshotTermios));
        let spec = Spec { initial_termios: Some(cooked_with_everything), ..spec };
        let outcome = run(&spec);
        assert_clean_exit(&outcome, false);
        let mut expected = theme_queries().concat();
        expected.extend_from_slice(&screen_taken());
        assert!(
            outcome.transcript.starts_with(&expected),
            "queries then the screen\n{}",
            outcome.describe()
        );
        assert_eq!(outcome.termios_during.len(), 1, "{}", outcome.describe());
        assert_frames(&outcome, 20);
        outcome
    }

    /// `--color ember`: no palette to learn, so no query at all; the
    /// transcript starts with the screen being taken.
    pub fn fixed_palette(subject: &Subject) -> Outcome {
        let outcome =
            run(&Spec::new(subject, &["--color", "ember", "--frames", "10", "--seed", "1"]));
        assert_clean_exit(&outcome, false);
        assert!(outcome.transcript.starts_with(&screen_taken()), "{}", outcome.describe());
        assert!(!outcome.contains(b"\x1b]"), "no OSC query\n{}", outcome.describe());
        assert_frames(&outcome, 10);
        outcome
    }

    /// The accent `learn_the_theme` picks from these replies and the five
    /// tints it ramps from it towards the background (boids.c
    /// `ramp_between`), as 24-bit SGR foregrounds.
    pub fn theme_tints(bg: [u8; 3], accent: [u8; 3]) -> Vec<Vec<u8>> {
        (0..5)
            .map(|i| {
                let c =
                    |k: usize| (accent[k] as i32 + (bg[k] as i32 - accent[k] as i32) * i / 7) as u8;
                format!("\x1b[38;2;{};{};{}m", c(0), c(1), c(2)).into_bytes()
            })
            .collect()
    }

    /// Distinct 24-bit foreground sequences in a transcript.
    pub fn truecolor_foregrounds(transcript: &[u8]) -> Vec<Vec<u8>> {
        let mut found: Vec<Vec<u8>> = Vec::new();
        let mut rest = transcript;
        while let Some(at) = find(rest, b"\x1b[38;2;") {
            let tail = &rest[at..];
            let end = tail.iter().position(|&b| b == b'm').map(|e| e + 1).unwrap_or(tail.len());
            let sequence = tail[..end].to_vec();
            if !found.contains(&sequence) {
                found.push(sequence);
            }
            rest = &tail[end..];
        }
        found
    }

    /// The theme a terminal describes in [`theme_run`]: a dark background and
    /// a red that is by far the most saturated and visible entry.
    pub const THEME_BG: [u8; 3] = [0x10, 0x10, 0x20];
    pub const THEME_ACCENT: [u8; 3] = [0xe0, 0x30, 0x30];

    fn theme_spec(subject: &Subject, fragmented: bool) -> Spec {
        let wide = |c: [u8; 3]| [c[0] as u16 * 0x101, c[1] as u16 * 0x101, c[2] as u16 * 0x101];
        let grey = [0x5050, 0x5050, 0x5050];
        let entries = [wide(THEME_ACCENT), grey, grey, grey, grey, grey];
        let mut replies = theme_replies(wide(THEME_BG), entries);
        if fragmented {
            // The background answer split in three reads, 15 ms apart.
            let whole = replies[0].fragments[0].clone();
            replies[0].fragments =
                vec![whole[..6].to_vec(), whole[6..17].to_vec(), whole[17..].to_vec()];
            replies[0].gap = Duration::from_millis(15);
        }
        let mut spec =
            Spec::new(subject, &["--frames", "30", "--seed", "1"]).env("COLORTERM", "truecolor");
        spec.replies = replies;
        spec
    }

    /// A terminal that answers every theme query: the flock is drawn only in
    /// the five tints between the chosen accent and the background.
    pub fn theme_run(subject: &Subject) -> Outcome {
        let outcome = run(&theme_spec(subject, false));
        assert_theme_learned(&outcome);
        outcome
    }

    /// The same with the background reply fragmented across reads: the reply
    /// is accumulated until its terminator, and the theme is learned alike.
    pub fn fragmented_theme_run(subject: &Subject) -> Outcome {
        let outcome = run(&theme_spec(subject, true));
        assert_eq!(
            outcome.events.iter().filter(|e| e.what.contains("reply to \\e]11;?")).count(),
            3,
            "three fragments written\n{}",
            outcome.describe()
        );
        assert_theme_learned(&outcome);
        outcome
    }

    fn assert_theme_learned(outcome: &Outcome) {
        assert_clean_exit(outcome, false);
        let mut expected = Vec::new();
        for query in theme_queries() {
            expected.extend_from_slice(&query);
        }
        expected.extend_from_slice(&screen_taken());
        assert!(outcome.transcript.starts_with(&expected), "{}", outcome.describe());
        let tints = theme_tints(THEME_BG, THEME_ACCENT);
        let used = truecolor_foregrounds(&outcome.transcript);
        assert!(!used.is_empty(), "the flock is drawn in 24-bit colour\n{}", outcome.describe());
        for colour in &used {
            assert!(
                tints.contains(colour),
                "{} is not a theme tint {:?}\n{}",
                escape(colour),
                tints.iter().map(|t| escape(t)).collect::<Vec<_>>(),
                outcome.describe()
            );
        }
    }

    /// `q` while flying. The reference has two quit paths, and which one a
    /// keypress takes depends on when it is read: at the top of the loop,
    /// the flock flies out for 40/60 s of frames, then the program exits 0;
    /// while the program waits on output the terminal has not yet taken (its
    /// frames are larger than a PTY's buffer, so it often is), it leaves at
    /// once with no outro. Both end with the terminal restored. This case
    /// types `q` once twenty frames have been drawn and accepts either path,
    /// asserting whichever it took; `quit_key_while_output_blocked` forces
    /// the second, and the injected-time oracle tests the first exactly.
    pub fn quit_key(subject: &Subject) -> Outcome {
        let mut spec = Spec::new(subject, &["--color", "ember", "--seed", "1"])
            .step(Step::after_output(ALT_SCREEN_ON, Action::Mark("screen taken")));
        for _ in 0..20 {
            spec = spec.step(Step::after_output(FRAME_BEGIN, Action::Mark("frame")));
        }
        let spec = spec.step(Step::after(Duration::from_millis(20), Action::Input(b"q".to_vec())));
        let outcome = run(&spec);
        assert_clean_exit(&outcome, false);
        let quit = outcome.event("wrote q").expect("q was typed");
        let flight = outcome.run_time.saturating_sub(quit.at);
        let after_quit = find_all(&outcome.transcript[quit.offset..], FRAME_BEGIN);
        // The flight is drawn: about 40 frames at 60 Hz; the bound allows a
        // slow host. Or the quit was read while output was blocked: at most
        // the frame being written finishes.
        let flew_out = flight >= Duration::from_millis(600) && after_quit >= 10;
        let left_at_once = after_quit <= 1;
        assert!(
            flew_out || left_at_once,
            "neither quit path: {flight:?} and {after_quit} frames after q\n{}",
            outcome.describe()
        );
        outcome
    }

    /// `q` typed while the terminal has stopped reading, so the program is
    /// waiting on its output: it reads the key there, leaves the loop with no
    /// outro, and exits 0 once its restore sequence can be written.
    pub fn quit_key_while_output_blocked(subject: &Subject) -> Outcome {
        let mut spec = Spec::new(subject, &["--color", "ember", "--seed", "1"])
            .step(Step::after_output(ALT_SCREEN_ON, Action::Mark("screen taken")));
        for _ in 0..10 {
            spec = spec.step(Step::after_output(FRAME_BEGIN, Action::Mark("frame")));
        }
        // Long enough for any PTY's buffer to fill at ~150 KB a second (a
        // Darwin PTY holds a few KB, Linux's several tens), so the program
        // is waiting on its output when the key arrives.
        let spec = spec
            .step(Step::after(Duration::ZERO, Action::PauseReading(Duration::from_millis(4000))))
            .step(Step::after(Duration::from_millis(2500), Action::Input(b"q".to_vec())));
        let outcome = run(&spec);
        assert_clean_exit(&outcome, false);
        // Frames written before the program blocked may still sit in the
        // PTY and arrive after the pause, so frames are not counted from the
        // q. What tells the paths apart is when it ends: the outro could only
        // begin once a write completed, after reading resumed, and would fly
        // for 40/60 s from there; the blocked quit ends as soon as its
        // restore sequence drains.
        outcome.event("wrote q").expect("q was typed");
        let resumed = outcome.event("reading resumed").expect("reading resumed");
        let lingered = outcome.run_time.saturating_sub(resumed.at);
        assert!(
            lingered < Duration::from_millis(400),
            "ran {lingered:?} after reading resumed: an outro\n{}",
            outcome.describe()
        );
        outcome
    }

    /// A handled signal while flying: the handler restores the terminal and
    /// leaves with `_exit(128 + signal)`, an exit code, not a signal death.
    pub fn signal_while_running(subject: &Subject, signal: c_int) -> Outcome {
        let spec = Spec::new(subject, &["--color", "ember", "--seed", "1"])
            .step(Step::after_output(ALT_SCREEN_ON, Action::Mark("screen taken")))
            .step(Step::after(Duration::from_millis(250), Action::Signal(signal)));
        let outcome = run(&spec);
        assert_signal_exit(&outcome, signal, false);
        outcome
    }

    /// The emergency path's result: exit code 128 + signal, attributes back,
    /// the restore sequence last and only once.
    pub fn assert_signal_exit(outcome: &Outcome, signal: c_int, sprites: bool) {
        outcome.assert_exit_code(128 + signal);
        outcome.assert_attributes_restored();
        assert!(
            outcome.transcript.ends_with(&screen_restored(sprites)),
            "restore sequence last\n{}",
            outcome.describe()
        );
        assert_eq!(outcome.count(ALT_SCREEN_OFF), 1, "{}", outcome.describe());
    }

    /// A signal while the program is blocked (in `poll`) on a terminal that
    /// stopped reading. On both OSes the exit is still 128 + signal, the
    /// attributes come back, and once the terminal reads again the restore
    /// sequence ends the transcript, after the backlog. When the program
    /// exits differs, by kernel, not by program: on Darwin the handler's
    /// `tcsetattr(TCSAFLUSH)` waits for the PTY's output to drain, so the exit
    /// follows the terminal's resumption (asserted); on Linux a PTY has no
    /// output queue for `TCSAFLUSH` to wait on and the few restore bytes still
    /// fit, so the program exits at once, before the terminal reads again
    /// (observed on aarch64 and x86_64; recorded, not asserted, since it
    /// depends on the room left in the kernel's buffers).
    pub fn signal_while_output_blocked(subject: &Subject, signal: c_int) -> Outcome {
        let spec = Spec::new(subject, &["--color", "ember", "--seed", "1"])
            .size(WinSize { row: 50, col: 200, xpixel: 1600, ypixel: 800 })
            .step(Step::after_output(ALT_SCREEN_ON, Action::Mark("screen taken")))
            .step(Step::after(
                Duration::from_millis(200),
                Action::PauseReading(Duration::from_millis(1500)),
            ))
            .step(Step::after(Duration::from_millis(500), Action::Signal(signal)));
        let outcome = run(&spec);
        assert_signal_exit(&outcome, signal, false);
        let signalled = outcome.event("sent signal").expect("signalled").at;
        let resumed = outcome.event("reading resumed").expect("resumed").at;
        let exited = outcome.event("exited").expect("exited").at;
        assert!(signalled < resumed, "signalled while blocked\n{}", outcome.describe());
        if cfg!(target_os = "macos") {
            assert!(resumed <= exited, "exit waited for the drain\n{}", outcome.describe());
        }
        outcome
    }

    /// SIGPIPE is ignored: sent while flying, it changes nothing, and the run
    /// ends at its frame limit with exit 0.
    pub fn sigpipe_ignored(subject: &Subject) -> Outcome {
        let spec = Spec::new(subject, &["--color", "ember", "--seed", "1", "--frames", "60"])
            .step(Step::after_output(ALT_SCREEN_ON, Action::Mark("screen taken")))
            .step(Step::after(Duration::from_millis(100), Action::Signal(platform::SIGPIPE)));
        let outcome = run(&spec);
        assert_clean_exit(&outcome, false);
        outcome
    }

    /// Standard output a pipe nobody reads: with SIGPIPE ignored the first
    /// frame's write fails with EPIPE, reported on standard error (the
    /// terminal, still raw, so no CR is added) before `exit(1)`, whose
    /// restore puts the attributes back.
    pub fn broken_pipe_stdout(subject: &Subject) -> Outcome {
        let mut spec = Spec::new(subject, &["--color", "ember", "--seed", "1"]);
        spec.stdout = Stream::ClosedPipe;
        let outcome = run(&spec);
        outcome.assert_exit_code(1);
        outcome.assert_attributes_restored();
        let expected = format!(
            "{}: cannot write to the terminal: {}\n",
            subject.name,
            String::from_utf8_lossy(&platform::strerror(platform::EPIPE))
        );
        assert_eq!(outcome.transcript, expected.as_bytes(), "{}", outcome.describe());
        outcome
    }

    /// `--render kitty`: the sprites are uploaded as Kitty graphics
    /// (`a=t` transmissions) and placed (`a=p`); the restore frees every
    /// image before the rest of the sequence.
    pub fn kitty_run(subject: &Subject) -> Outcome {
        let outcome = run(&Spec::new(
            subject,
            &["--render", "kitty", "--frames", "10", "--seed", "1", "--color", "ember"],
        ));
        assert_clean_exit(&outcome, true);
        assert!(outcome.transcript.starts_with(&screen_taken()), "{}", outcome.describe());
        assert!(outcome.contains(b"\x1b_Ga=t,q=2,f=100,I="), "uploads\n{}", outcome.describe());
        assert!(outcome.contains(b"\x1b_Ga=p,I="), "placements\n{}", outcome.describe());
        assert_eq!(outcome.count(KITTY_FREE_IMAGES), 1, "{}", outcome.describe());
        assert_frames(&outcome, 10);
        outcome
    }

    /// The errno `tcgetattr` gives on this target for standard input of this
    /// kind: `ENOTTY` for a pipe; for `/dev/null`, `ENOTTY` on Linux but
    /// `ENODEV` on Darwin.
    pub fn tcgetattr_errno(stdin: Stream) -> i32 {
        let error = match stdin {
            Stream::Null => {
                let null = std::fs::File::open("/dev/null").expect("open /dev/null");
                platform::tcgetattr(null.as_raw_fd()).expect_err("/dev/null is no terminal")
            }
            Stream::ClosedPipe => {
                let (reader, _writer) = std::io::pipe().expect("pipe");
                platform::tcgetattr(reader.as_raw_fd()).expect_err("a pipe is no terminal")
            }
            other => panic!("{other:?} is not a non-terminal input"),
        };
        error.raw_os_error().expect("an OS error")
    }

    /// Standard input not a terminal: `perror("Can't enable raw mode")` with
    /// `tcgetattr`'s errno, then exit 1, before anything else is written to
    /// the terminal (which, never made raw, turns the newline into CR LF).
    pub fn stdin_not_a_terminal(subject: &Subject, stdin: Stream) -> Outcome {
        let mut spec = Spec::new(subject, &[]);
        spec.stdin = stdin;
        let outcome = run(&spec);
        outcome.assert_exit_code(1);
        outcome.assert_attributes_restored();
        let mut expected =
            platform::perror_message(b"Can't enable raw mode", tcgetattr_errno(stdin));
        expected.pop();
        expected.extend_from_slice(b"\r\n");
        assert_eq!(escape(&outcome.transcript), escape(&expected), "{}", outcome.describe());
        outcome
    }

    /// Cursor-position rows (`ESC [ row ; col H`) in a transcript slice.
    pub fn cursor_rows(transcript: &[u8]) -> Vec<u32> {
        let mut rows = Vec::new();
        let mut rest = transcript;
        while let Some(at) = find(rest, b"\x1b[") {
            rest = &rest[at + 2..];
            let digits = rest.iter().take_while(|b| b.is_ascii_digit()).count();
            if digits > 0 && rest.get(digits) == Some(&b';') {
                let cols = rest[digits + 1..].iter().take_while(|b| b.is_ascii_digit()).count();
                if rest.get(digits + 1 + cols) == Some(&b'H')
                    && let Ok(row) = std::str::from_utf8(&rest[..digits]).unwrap_or("").parse()
                {
                    rows.push(row);
                }
            }
        }
        rows
    }

    /// A 10x5 window without pixel sizes, grown to 100x40 and shrunk to 6x3
    /// while flying: drawing follows the size each frame (no row beyond the
    /// window's before the growth, rows beyond 5 after it), and the run still
    /// ends cleanly.
    pub fn tiny_window_and_resizes(subject: &Subject) -> Outcome {
        let spec = Spec::new(subject, &["--color", "ember", "--seed", "1", "--frames", "90"])
            .size(WinSize { row: 5, col: 10, xpixel: 0, ypixel: 0 })
            .step(Step::after_output(ALT_SCREEN_ON, Action::Mark("screen taken")))
            .step(Step::after(
                Duration::from_millis(400),
                Action::Resize(WinSize { row: 40, col: 100, xpixel: 800, ypixel: 640 }),
            ))
            .step(Step::after(
                Duration::from_millis(500),
                Action::Resize(WinSize { row: 3, col: 6, xpixel: 0, ypixel: 0 }),
            ));
        let outcome = run(&spec);
        assert_clean_exit(&outcome, false);
        assert_eq!(
            outcome.steps_done,
            3,
            "both resizes happened in the run\n{}",
            outcome.describe()
        );
        let grown = outcome.offset_of("resized to 100x40").expect("grown");
        let before = cursor_rows(&outcome.transcript[..grown]);
        assert!(before.iter().all(|&r| r <= 5), "rows {before:?} in a 5-row window");
        let shrunk = outcome.offset_of("resized to 6x3").expect("shrunk");
        let during = cursor_rows(&outcome.transcript[grown..shrunk]);
        assert!(during.iter().any(|&r| r > 5), "the grown window is used: {during:?}");
        outcome
    }

    /// `--snapshot PATH`: at the end the terminal is restored first, then the
    /// PNG is written and `<name>: wrote PATH` printed on standard error,
    /// after the restore sequence, with the newline cooked again to CR LF.
    pub fn snapshot_run(subject: &Subject, path: &Path) -> Outcome {
        let path_text = path.to_str().expect("a UTF-8 scratch path");
        let outcome = run(&Spec::new(
            subject,
            &["--color", "ember", "--seed", "1", "--frames", "10", "--snapshot", path_text],
        ));
        outcome.assert_exit_code(0);
        outcome.assert_attributes_restored();
        let mut expected_tail = screen_restored(false);
        expected_tail
            .extend_from_slice(format!("{}: wrote {path_text}\r\n", subject.name).as_bytes());
        assert!(outcome.transcript.ends_with(&expected_tail), "{}", outcome.describe());
        assert_frames(&outcome, 10);
        let png = std::fs::read(path).expect("the snapshot exists");
        assert!(png.starts_with(b"\x89PNG\r\n\x1a\n"), "a PNG file");
        outcome
    }

    /// `--snapshot` into a directory that does not exist: restored, then
    /// `<name>: could not write PATH` and exit 1.
    pub fn snapshot_failure(subject: &Subject, path: &Path) -> Outcome {
        let path_text = path.to_str().expect("a UTF-8 scratch path");
        let outcome = run(&Spec::new(
            subject,
            &["--color", "ember", "--seed", "1", "--frames", "5", "--snapshot", path_text],
        ));
        outcome.assert_exit_code(1);
        outcome.assert_attributes_restored();
        let mut expected_tail = screen_restored(false);
        expected_tail.extend_from_slice(
            format!("{}: could not write {path_text}\r\n", subject.name).as_bytes(),
        );
        assert!(outcome.transcript.ends_with(&expected_tail), "{}", outcome.describe());
        outcome
    }

    /// The terminal closing under the program. Only the exit is observable
    /// (its messages go to the closed terminal); recorded, not asserted
    /// beyond finishing in time without a signal death.
    pub fn terminal_closed(subject: &Subject) -> Outcome {
        let spec = Spec::new(subject, &["--color", "ember", "--seed", "1"])
            .step(Step::after_output(ALT_SCREEN_ON, Action::Mark("screen taken")))
            .step(Step::after(Duration::from_millis(200), Action::CloseMaster));
        let outcome = run(&spec);
        assert!(!outcome.timed_out, "{}", outcome.describe());
        outcome
    }
}
