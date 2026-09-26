//! Table-driven command-line options (cbirds `options.c` and `options.h`).
//!
//! A tool with forty switches cannot keep them in a hand written loop and stay
//! readable, and its `--help` cannot stay aligned by hand. Declare the options
//! once, as data, and the parser, the help text and the shell completions all
//! come off the same table.
//!
//! The C table points each row at its value through a `void *`; here a row
//! holds a typed accessor into the caller's settings struct `T`, so a row
//! cannot write the wrong type. Everything a user can see is kept byte for
//! byte: arguments and messages are bytes, not text, numbers go through the C
//! library's own `strtod`, columns are padded by bytes as `%-*s` pads them, and
//! a message is cut to the caller's buffer exactly as `snprintf` cuts it.

#![forbid(unsafe_code)]

use crate::platform::OsStrExt;
use std::ffi::{CString, OsStr, OsString};
use std::io::{self, Write};

use crate::platform;

/// What a row is, and where its value goes.
///
/// A flag's C target is an `int` the parser only ever sets to one or zero and
/// never reads, so it is a `bool` here.
pub enum Kind<T> {
    /// No argument, sets its target.
    Flag(fn(&mut T) -> &mut bool),
    /// No argument, clears its target: the off switch for something that is
    /// on by default. Named for what it does — "no-panel" — so the help lists
    /// the form that has an effect rather than the form that cannot have one.
    Off(fn(&mut T) -> &mut bool),
    /// A bounded integer. The bounds are doubles, as in the C table, and must
    /// lie within `i32`: beyond it the C's `(int)` casts are undefined, where
    /// Rust's saturate.
    Int { target: fn(&mut T) -> &mut i32, min: f64, max: f64 },
    /// A bounded double.
    Double { target: fn(&mut T) -> &mut f64, min: f64, max: f64 },
    /// One of `names`; the index goes into the target.
    Enum { target: fn(&mut T) -> &mut i32, names: &'static [&'static str] },
    /// Kept as given (the C keeps a pointer into `argv`).
    Str(fn(&mut T) -> &mut Option<OsString>),
}

impl<T> Kind<T> {
    fn takes_value(&self) -> bool {
        !matches!(self, Kind::Flag(_) | Kind::Off(_))
    }
}

/// One row of the table (`option_t`).
pub struct OptionSpec<T> {
    /// `None` when the option is long only.
    pub shorthand: Option<u8>,
    /// Long name without the dashes.
    pub name: &'static str,
    /// An older name, accepted but never advertised.
    pub alias: Option<&'static str>,
    pub kind: Kind<T>,
    /// "COUNT", "FPS", shown in `--help`.
    pub metavar: Option<&'static str>,
    /// One line, lower case, no trailing stop.
    pub help: &'static str,
    /// Section heading in `--help`.
    pub group: &'static str,
    /// Shown by `-h` as well as by `--help`.
    pub essential: bool,
}

impl<T> OptionSpec<T> {
    /// The C's `char shorthand`, where zero means none.
    fn short(&self) -> Option<u8> {
        self.shorthand.filter(|&c| c != 0)
    }
}

/// `options_status_t`, with the C's values.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    Ok = 0,
    /// `-h`: the one screen version.
    Help = 1,
    /// `--help`: everything, grouped.
    HelpFull = 2,
    /// `--version`.
    Version = 3,
    /// `--completion SHELL`, the shell left in the message.
    Completion = 4,
    /// The message says what was wrong.
    Error = 5,
}

/// What `options_parse` returned, and what it left in the caller's `error`
/// buffer: the shell for [`Status::Completion`], the complaint for
/// [`Status::Error`], and nothing otherwise. The message is cut to one byte
/// less than the buffer size, as `snprintf` cuts it, and may hold the user's
/// raw bytes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Parsed {
    pub status: Status,
    pub message: Vec<u8>,
}

/// One worked line of the help: what to type, and what it gets you. Kept as
/// two strings rather than one pre-padded line so the columns are aligned by
/// the same arithmetic that aligns the options, and stay aligned when one is
/// edited.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Example {
    pub command: &'static str,
    pub what: &'static str,
}

/// What `snprintf(error, size, ...)` leaves in a buffer of `size` bytes.
fn cut(mut message: Vec<u8>, size: usize) -> Vec<u8> {
    message.truncate(size.saturating_sub(1));
    message
}

fn refuse(message: Vec<u8>, size: usize) -> Parsed {
    Parsed { status: Status::Error, message: cut(message, size) }
}

/// Joins byte strings; a function so that literals of different lengths
/// coerce to one slice type.
fn concat(parts: &[&[u8]]) -> Vec<u8> {
    parts.concat()
}

fn find_long<'a, T>(table: &'a [OptionSpec<T>], name: &[u8]) -> Option<&'a OptionSpec<T>> {
    table.iter().find(|option| {
        option.name.as_bytes() == name || option.alias.is_some_and(|alias| alias.as_bytes() == name)
    })
}

/// Levenshtein, small and iterative, so a typo can be answered with the name
/// the user probably meant instead of a bare refusal. Like the C, it works in
/// rows of 64 and calls anything longer 64 edits away.
fn edit_distance(a: &[u8], b: &[u8]) -> usize {
    let (la, lb) = (a.len(), b.len());
    if lb + 1 > 64 {
        return 64;
    }
    let mut previous: Vec<usize> = (0..=lb).collect();
    let mut current = vec![0; lb + 1];
    for i in 1..=la {
        current[0] = i;
        for j in 1..=lb {
            let cost = usize::from(a[i - 1] != b[j - 1]);
            let mut best = previous[j] + 1;
            if current[j - 1] + 1 < best {
                best = current[j - 1] + 1;
            }
            if previous[j - 1] + cost < best {
                best = previous[j - 1] + cost;
            }
            current[j] = best;
        }
        previous.copy_from_slice(&current);
    }
    previous[lb]
}

/// The first name, in table order, closest to what was typed; aliases are not
/// offered. Nothing when the word is too long to measure.
fn nearest_name<T>(table: &[OptionSpec<T>], name: &[u8]) -> Option<&'static str> {
    if name.len() + 1 > 64 {
        return None;
    }
    let mut best = None;
    let mut best_distance = 3; // Further than two edits away is a different word.
    for option in table {
        let distance = edit_distance(name, option.name.as_bytes());
        if distance < best_distance {
            best_distance = distance;
            best = Some(option.name);
        }
    }
    best
}

fn find_short<T>(table: &[OptionSpec<T>], shorthand: u8) -> Option<&OptionSpec<T>> {
    table.iter().find(|option| option.short() == Some(shorthand))
}

/// What the C would see of a string it is handed: the bytes up to the first
/// NUL, which a real `argv` never holds.
fn c_string(bytes: &[u8]) -> &[u8] {
    match bytes.iter().position(|&b| b == 0) {
        Some(end) => &bytes[..end],
        None => bytes,
    }
}

/// `strtod` must read all of the text and land on a finite number that did not
/// overflow or underflow. It reads "nan" and "inf" as numbers, and a NaN would
/// pass every range comparison by failing it.
fn number(text: &[u8]) -> Option<f64> {
    let text_c = CString::new(text).ok()?;
    let read = platform::strtod(&text_c);
    if read.erange || read.consumed == 0 || read.consumed != text.len() || !read.value.is_finite() {
        return None;
    }
    Some(read.value)
}

/// Stores `text` into a row that takes a value. Long names read better in
/// messages, so errors quote those even for `-n`.
fn assign<T>(option: &OptionSpec<T>, settings: &mut T, text: &[u8]) -> Result<(), Vec<u8>> {
    let name = option.name.as_bytes();
    let (min, max) = match &option.kind {
        Kind::Flag(target) => {
            *target(settings) = true;
            return Ok(());
        }
        Kind::Off(target) => {
            *target(settings) = false;
            return Ok(());
        }
        Kind::Str(target) => {
            *target(settings) = Some(platform::os_string_from_bytes(text));
            return Ok(());
        }
        Kind::Enum { target, names } => {
            if let Some(index) = names.iter().position(|n| n.as_bytes() == text) {
                *target(settings) = index as i32;
                return Ok(());
            }
            let mut message = concat(&[b"--", name, b" must be one of"]);
            for (i, choice) in names.iter().enumerate() {
                message.extend_from_slice(if i > 0 { b", " } else { b" " });
                message.extend_from_slice(choice.as_bytes());
            }
            return Err(message);
        }
        Kind::Int { min, max, .. } | Kind::Double { min, max, .. } => (*min, *max),
    };

    let Some(value) = number(text) else {
        return Err(concat(&[b"--", name, b" wants a number, not '", text, b"'"]));
    };
    if value < min || value > max {
        let range = match option.kind {
            Kind::Int { .. } => format!(" must be between {} and {}", min as i32, max as i32),
            _ => format!(" must be between {} and {}", format_g(min), format_g(max)),
        };
        return Err(concat(&[b"--", name, range.as_bytes()]));
    }
    match &option.kind {
        Kind::Int { target, .. } => {
            let whole = value as i32;
            if value != f64::from(whole) {
                return Err(concat(&[b"--", name, b" wants a whole number, not '", text, b"'"]));
            }
            *target(settings) = whole;
        }
        Kind::Double { target, .. } => *target(settings) = value,
        _ => unreachable!("only numeric rows get this far"),
    }
    Ok(())
}

/// `options_parse`.
///
/// Accepts, for an option named "birds" with shorthand 'n':
///     `-n 800   -n800   --birds 800   --birds=800`
/// Flags cluster, so `-qv` is `-q -v`, and `--no-NAME` clears a flag. A bare
/// "--" ends option parsing. Anything left over is an error: this program
/// takes no positional arguments.
///
/// `argv[0]` is the program, as in C. Values are stored left to right as they
/// are read, so an error leaves what came before it applied. `error_size` is
/// the size of the C caller's buffer; zero is refused outright, as the C
/// refuses it, before anything is read.
pub fn parse<T, A: AsRef<OsStr>>(
    table: &[OptionSpec<T>],
    settings: &mut T,
    argv: &[A],
    error_size: usize,
) -> Parsed {
    if error_size == 0 {
        return Parsed { status: Status::Error, message: Vec::new() };
    }
    let found = |status| Parsed { status, message: Vec::new() };
    #[cfg(windows)]
    if argv.iter().any(|arg| arg.as_ref().to_str().is_none()) {
        return refuse(b"arguments must be valid Unicode on Windows".to_vec(), error_size);
    }
    let args: Vec<&[u8]> = argv.iter().map(|a| c_string(a.as_ref().as_bytes())).collect();
    let argc = args.len();

    let mut i = 1;
    while i < argc {
        let argument = args[i];

        if argument == b"--" {
            if i + 1 < argc {
                return refuse(concat(&[b"unexpected argument '", args[i + 1], b"'"]), error_size);
            }
            return found(Status::Ok);
        }
        if argument == b"-h" {
            return found(Status::Help);
        }
        if argument == b"--help" {
            return found(Status::HelpFull);
        }
        if argument == b"--completion" {
            if i + 1 >= argc {
                return refuse(b"--completion wants bash, zsh or fish".to_vec(), error_size);
            }
            return Parsed {
                status: Status::Completion,
                message: cut(args[i + 1].to_vec(), error_size),
            };
        }
        if argument == b"-V" || argument == b"--version" {
            return found(Status::Version);
        }

        if argument.first() != Some(&b'-') || argument.len() == 1 {
            return refuse(concat(&[b"unexpected argument '", argument, b"'"]), error_size);
        }

        if argument[1] == b'-' {
            // Long form.
            let spelled = &argument[2..];
            let equals = spelled.iter().position(|&b| b == b'=');
            let name = &spelled[..equals.unwrap_or(spelled.len())];

            // --no-NAME clears a flag, which is how the off switches read.
            // Only a flag: an off switch or a value under that name falls
            // through to be looked up as spelled, "no-" and all.
            if name.len() > 3
                && name.starts_with(b"no-")
                && let Some(option) = find_long(table, &name[3..])
                && let Kind::Flag(target) = &option.kind
            {
                if equals.is_some() {
                    return refuse(concat(&[b"--", name, b" takes no value"]), error_size);
                }
                *target(settings) = false;
                i += 1;
                continue;
            }

            let Some(option) = find_long(table, name) else {
                let message = match nearest_name(table, name) {
                    Some(meant) => concat(&[
                        b"unknown option '--",
                        name,
                        b"', did you mean '--",
                        meant.as_bytes(),
                        b"'?",
                    ]),
                    None => concat(&[b"unknown option '--", name, b"'"]),
                };
                return refuse(message, error_size);
            };
            match &option.kind {
                Kind::Flag(target) | Kind::Off(target) => {
                    if equals.is_some() {
                        let message = concat(&[b"--", option.name.as_bytes(), b" takes no value"]);
                        return refuse(message, error_size);
                    }
                    *target(settings) = matches!(option.kind, Kind::Flag(_));
                    i += 1;
                    continue;
                }
                _ => {}
            }
            let text = match equals {
                Some(at) => Some(&spelled[at + 1..]),
                None => {
                    i += 1;
                    args.get(i).copied()
                }
            };
            let Some(text) = text else {
                return refuse(
                    concat(&[b"--", option.name.as_bytes(), b" wants a value"]),
                    error_size,
                );
            };
            if let Err(message) = assign(option, settings, text) {
                return refuse(message, error_size);
            }
            i += 1;
            continue;
        }

        // Short form, and flags cluster: -qv, -qn800, -qn 800.
        let mut c = 1;
        while c < argument.len() {
            let letter = argument[c];
            // h and V are not in the table — they are answered before any of
            // it is read — so they have to be answered here too, or `-help`,
            // which is a cluster of h e l p, is met with "unknown option '-h'":
            // the program denying its own flag over the commonest typo there is.
            if letter == b'h' {
                return found(Status::Help);
            }
            if letter == b'V' {
                return found(Status::Version);
            }
            let Some(option) = find_short(table, letter) else {
                return refuse(concat(&[b"unknown option '-", &[letter], b"'"]), error_size);
            };
            if let Kind::Flag(target) | Kind::Off(target) = &option.kind {
                *target(settings) = matches!(option.kind, Kind::Flag(_));
                c += 1;
                continue;
            }
            let text = if c + 1 < argument.len() {
                Some(&argument[c + 1..])
            } else {
                i += 1;
                args.get(i).copied()
            };
            let Some(text) = text else {
                return refuse(concat(&[b"-", &[letter], b" wants a value"]), error_size);
            };
            if let Err(message) = assign(option, settings, text) {
                return refuse(message, error_size);
            }
            break; // The rest of the cluster was the value.
        }
        i += 1;
    }
    found(Status::Ok)
}

/// `printf("%g", value)`: six significant digits, the shorter of fixed and
/// exponent notation by the C rule, trailing zeros dropped. Rust's exact
/// `{:e}` formatting rounds ties to even, as the C libraries do. A NaN bound
/// is printed without its sign by Darwin's libc (checked against it) and with
/// it by glibc.
fn format_g(value: f64) -> String {
    const PRECISION: i32 = 6;
    if value.is_nan() {
        return if value.is_sign_negative() && cfg!(target_os = "linux") {
            "-nan".to_owned()
        } else {
            "nan".to_owned()
        };
    }
    let sign = if value.is_sign_negative() { "-" } else { "" };
    if value.is_infinite() {
        return format!("{sign}inf");
    }
    if value == 0.0 {
        return format!("{sign}0");
    }
    let scientific = format!("{:.*e}", (PRECISION - 1) as usize, value.abs());
    let (mantissa, exponent) = scientific.split_once('e').expect("exponent notation");
    let exponent: i32 = exponent.parse().expect("decimal exponent");
    let digits: String = mantissa.chars().filter(char::is_ascii_digit).collect();
    let trimmed = |text: String| -> String {
        if text.contains('.') {
            text.trim_end_matches('0').trim_end_matches('.').to_owned()
        } else {
            text
        }
    };
    if !(-4..PRECISION).contains(&exponent) {
        let mantissa = trimmed(format!("{}.{}", &digits[..1], &digits[1..]));
        let exponent_sign = if exponent < 0 { '-' } else { '+' };
        format!("{sign}{mantissa}e{exponent_sign}{:02}", exponent.unsigned_abs())
    } else if exponent >= 0 {
        let point = exponent as usize + 1;
        format!("{sign}{}", trimmed(format!("{}.{}", &digits[..point], &digits[point..])))
    } else {
        let zeros = "0".repeat((-exponent - 1) as usize);
        format!("{sign}{}", trimmed(format!("0.{zeros}{digits}")))
    }
}

/// Exactly as wide as `render_option` writes it, so the two cannot disagree:
/// two of indent, four for the shorthand slot whether or not there is one, the
/// two dashes, the name, and the metavar with its space.
fn option_width<T>(option: &OptionSpec<T>) -> usize {
    let mut width = 2 + 4 + 2 + option.name.len();
    if option.kind.takes_value()
        && let Some(metavar) = option.metavar
    {
        width += 1 + metavar.len();
    }
    width
}

/// One line of the help. The C assembles the left column in a 96 byte buffer,
/// so it is cut at 95 bytes, and then pads it by bytes with `%-*s`. (A name
/// over 88 bytes with a metavar overruns that buffer in the C; here the line
/// is cut the same way.)
fn render_option(
    out: &mut dyn Write,
    column: usize,
    shorthand: Option<u8>,
    name: &str,
    metavar: Option<&str>,
    help: &str,
) -> io::Result<()> {
    let mut left = b"  ".to_vec();
    match shorthand {
        Some(letter) => left.extend_from_slice(&[b'-', letter, b',', b' ']),
        None => left.extend_from_slice(b"    "),
    }
    left.extend_from_slice(b"--");
    left.extend_from_slice(name.as_bytes());
    if let Some(metavar) = metavar {
        left.push(b' ');
        left.extend_from_slice(metavar.as_bytes());
    }
    left.truncate(95);
    out.write_all(&left)?;
    out.write_all(&b" ".repeat(column.saturating_sub(left.len())))?;
    out.write_all(help.as_bytes())?;
    out.write_all(b"\n")
}

/// `options_usage`: groups in table order, columns aligned to the widest
/// option. With `everything` false only the rows marked essential are shown,
/// which is what `-h` is for: one screen a newcomer can read, against the full
/// list for someone looking for a particular switch.
///
/// `tagline` and `examples` are the C's nullable pointers: `None` prints no
/// tagline and no Examples section, where an empty list still prints the
/// heading. Examples are shown only with `everything`. `program` is printed
/// as the bytes it is, as `%s` prints them.
///
/// The C ignores failed writes and prints on; this stops at the first and
/// returns it, and what the caller does with it is the caller's to match.
pub fn usage<T>(
    out: &mut dyn Write,
    program: &[u8],
    tagline: Option<&str>,
    examples: Option<&[Example]>,
    table: &[OptionSpec<T>],
    everything: bool,
) -> io::Result<()> {
    let mut column = "  -V, --version".len();
    for option in table {
        column = column.max(option_width(option));
    }
    column += 2;

    if let Some(tagline) = tagline {
        out.write_all(tagline.as_bytes())?;
        out.write_all(b"\n\n")?;
    }
    out.write_all(b"Usage: ")?;
    out.write_all(c_string(program))?;
    out.write_all(b" [OPTIONS]\n")?;

    // The heading changes whenever the group does, and the switches the table
    // does not hold close the list under "General".
    let mut group: Option<&str> = None;
    for i in 0..=table.len() {
        let option = table.get(i);
        if option.is_some_and(|option| !everything && !option.essential) {
            continue;
        }
        let next = option.map_or("General", |option| option.group);
        if group != Some(next) {
            write!(out, "\n{next}\n")?;
            group = Some(next);
        }
        let Some(option) = option else { break };
        let metavar = if option.kind.takes_value() { option.metavar } else { None };
        render_option(out, column, option.short(), option.name, metavar, option.help)?;
    }
    if everything {
        render_option(out, column, Some(b'h'), "help", None, "the one-screen help")?;
        render_option(out, column, None, "completion", Some("SHELL"), COMPLETION_HELP)?;
        render_option(out, column, Some(b'V'), "version", None, VERSION_HELP)?;
    } else {
        render_option(out, column, None, "help", None, "every option, grouped")?;
        render_option(out, column, Some(b'V'), "version", None, VERSION_HELP)?;
    }

    if let (Some(examples), true) = (examples, everything) {
        let widest = examples.iter().map(|example| example.command.len()).max().unwrap_or(0);
        out.write_all(b"\nExamples\n")?;
        for example in examples {
            let pad = " ".repeat(widest.saturating_sub(example.command.len()));
            writeln!(out, "  {}{pad}  {}", example.command, example.what)?;
        }
    }
    Ok(())
}

const COMPLETION_HELP: &str = "completions for bash, zsh or fish";
const VERSION_HELP: &str = "print the version and quit";

/// Help text goes inside a single quoted zsh spec, as the [description]: a
/// quote would end the spec, as "each other's" did, and a bracket would end
/// the description.
fn zsh_description(out: &mut Vec<u8>, text: &str) {
    for &byte in text.as_bytes() {
        match byte {
            b'\'' => out.extend_from_slice(b"'\\''"),
            b'[' | b']' => out.extend_from_slice(&[b'\\', byte]),
            _ => out.push(byte),
        }
    }
}

/// And inside a double quoted fish string, where these four mean something.
fn fish_description(out: &mut Vec<u8>, text: &str) {
    for &byte in text.as_bytes() {
        if matches!(byte, b'"' | b'\\' | b'$') {
            out.push(b'\\');
        }
        out.push(byte);
    }
}

/// What follows an option that takes a value in a zsh spec: its names when it
/// is one of a list, a file when it is a file, and a bare message otherwise.
fn zsh_value<T>(out: &mut Vec<u8>, option: &OptionSpec<T>) {
    if !option.kind.takes_value() {
        return;
    }
    if let Kind::Enum { names, .. } = &option.kind {
        out.extend_from_slice(b":name:(");
        out.extend_from_slice(names.join(" ").as_bytes());
        out.push(b')');
    } else if option.metavar == Some("FILE") {
        out.extend_from_slice(b":file:_files");
    } else {
        out.extend_from_slice(b":value:");
    }
}

/// The switches every table gets without being in it, as the parser answers
/// them and `--help` lists them: one list, so no shell can be left without one
/// again.
struct Special {
    shorthand: Option<u8>,
    name: Option<&'static str>,
    help: &'static str,
}

const SPECIAL: [Special; 4] = [
    Special { shorthand: Some(b'h'), name: None, help: "the one-screen help" },
    Special { shorthand: None, name: Some("help"), help: "every option, grouped" },
    Special { shorthand: None, name: Some("completion"), help: COMPLETION_HELP },
    Special { shorthand: Some(b'V'), name: Some("version"), help: VERSION_HELP },
];

fn short_text(letter: u8) -> [u8; 1] {
    [letter]
}

/// `options_completion`: completions for bash, zsh or fish, off the same
/// table. `Ok(false)`, with nothing written, for any other shell. An error
/// can only come from writing, so only for a shell the C would have answered
/// with 1, whatever became of its writes.
pub fn completion<T>(
    out: &mut dyn Write,
    shell: &[u8],
    program: &str,
    table: &[OptionSpec<T>],
) -> io::Result<bool> {
    let program = program.as_bytes();
    let mut text: Vec<u8> = Vec::new();
    match c_string(shell) {
        b"bash" => {
            text.extend_from_slice(&concat(&[b"# ", program, b" completions for bash\n"]));
            text.extend_from_slice(b"complete -W \"");
            for option in table {
                text.extend_from_slice(&concat(&[b"--", option.name.as_bytes(), b" "]));
                if let Some(letter) = option.short() {
                    text.extend_from_slice(&[b'-', letter, b' ']);
                }
            }
            for special in &SPECIAL {
                if let Some(letter) = special.shorthand {
                    text.extend_from_slice(&[b'-', letter, b' ']);
                }
                if let Some(name) = special.name {
                    text.extend_from_slice(&concat(&[b"--", name.as_bytes(), b" "]));
                }
            }
            text.extend_from_slice(&concat(&[b"\" ", program, b"\n"]));
        }
        b"zsh" => {
            text.extend_from_slice(&concat(&[b"#compdef ", program, b"\n_arguments \\\n"]));
            for option in table {
                let spellings: [Option<Vec<u8>>; 2] = [
                    Some(concat(&[b"--", option.name.as_bytes()])),
                    option.short().map(|letter| concat(&[b"-", &short_text(letter)])),
                ];
                for spelling in spellings.into_iter().flatten() {
                    text.extend_from_slice(b"  '");
                    text.extend_from_slice(&spelling);
                    text.push(b'[');
                    zsh_description(&mut text, option.help);
                    text.push(b']');
                    zsh_value(&mut text, option);
                    text.extend_from_slice(b"' \\\n");
                }
            }
            for (i, special) in SPECIAL.iter().enumerate() {
                let more: &[u8] = if i + 1 < SPECIAL.len() { b" \\" } else { b"" };
                let help = special.help.as_bytes();
                if let Some(letter) = special.shorthand {
                    let after: &[u8] = if special.name.is_some() { b" \\" } else { more };
                    text.extend_from_slice(&concat(&[
                        b"  '-",
                        &short_text(letter),
                        b"[",
                        help,
                        b"]'",
                        after,
                        b"\n",
                    ]));
                }
                if let Some(name) = special.name {
                    let value: &[u8] =
                        if name == "completion" { b":shell:(bash zsh fish)" } else { b"" };
                    text.extend_from_slice(&concat(&[
                        b"  '--",
                        name.as_bytes(),
                        b"[",
                        help,
                        b"]",
                        value,
                        b"'",
                        more,
                        b"\n",
                    ]));
                }
            }
        }
        b"fish" => {
            for option in table {
                text.extend_from_slice(&concat(&[
                    b"complete -c ",
                    program,
                    b" -l ",
                    option.name.as_bytes(),
                ]));
                if let Some(letter) = option.short() {
                    text.extend_from_slice(&[b' ', b'-', b's', b' ', letter]);
                }
                if option.kind.takes_value() {
                    text.extend_from_slice(b" -r");
                }
                if let Kind::Enum { names, .. } = &option.kind {
                    text.extend_from_slice(b" -f -a \"");
                    text.extend_from_slice(names.join(" ").as_bytes());
                    text.push(b'"');
                }
                text.extend_from_slice(b" -d \"");
                fish_description(&mut text, option.help);
                text.extend_from_slice(b"\"\n");
            }
            for special in &SPECIAL {
                text.extend_from_slice(&concat(&[b"complete -c ", program]));
                if let Some(name) = special.name {
                    text.extend_from_slice(&concat(&[b" -l ", name.as_bytes()]));
                }
                if let Some(letter) = special.shorthand {
                    text.extend_from_slice(&[b' ', b'-', b's', b' ', letter]);
                }
                if special.name == Some("completion") {
                    text.extend_from_slice(b" -r -f -a \"bash zsh fish\"");
                }
                text.extend_from_slice(&concat(&[b" -d \"", special.help.as_bytes(), b"\"\n"]));
            }
        }
        _ => return Ok(false),
    }
    out.write_all(&text)?;
    Ok(true)
}

/// `options_status_string`.
pub fn options_status_string(status: Status) -> &'static str {
    match status {
        Status::Ok => "ok",
        Status::Help | Status::HelpFull => "help requested",
        Status::Completion => "completions requested",
        Status::Version => "version requested",
        Status::Error => "invalid arguments",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_g_follows_printf() {
        let cases: &[(f64, &str)] = &[
            (0.0, "0"),
            (-0.0, "-0"),
            (1.0, "1"),
            (0.5, "0.5"),
            (4096.0, "4096"),
            (1e6, "1e+06"),
            (123456.0, "123456"),
            (1234567.0, "1.23457e+06"),
            (1234565.0, "1.23456e+06"),
            (999999.5, "1e+06"),
            (0.0001, "0.0001"),
            (0.00001, "1e-05"),
            (0.000099999999, "0.0001"),
            (-2.5, "-2.5"),
            (1e100, "1e+100"),
            (5e-324, "4.94066e-324"),
            (f64::INFINITY, "inf"),
            (f64::NEG_INFINITY, "-inf"),
            (1.0 / 3.0, "0.333333"),
            (100000.0, "100000"),
        ];
        for &(value, expected) in cases {
            assert_eq!(format_g(value), expected, "{value:e}");
        }
    }

    #[test]
    fn edit_distance_is_levenshtein() {
        assert_eq!(edit_distance(b"birdz", b"birds"), 1);
        assert_eq!(edit_distance(b"", b"size"), 4);
        assert_eq!(edit_distance(b"kitten", b"sitting"), 3);
        assert_eq!(edit_distance(b"x", &[b'a'; 64]), 64);
        assert_eq!(edit_distance(b"x", &[b'a'; 63]), 63);
    }
}
