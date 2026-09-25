//! The panel on the user's terminal: raw mode, a ratatui viewport pinned
//! under the prompt, keys and wheel through crossterm. It opens the
//! terminal itself and, for its lifetime, points stdin and stdout at it:
//! the shell hook captures stdout and, in zsh, gives a widget no stdin,
//! while crossterm reads keys from stdin and asks its questions through
//! stdout. Once the panel closes, stdout is the shell's again and receives
//! only the text to insert. The hook puts the cursor on a fresh line
//! before, and climbs back to the prompt after.

use std::fs::{File, OpenOptions};
use std::io::{self, Read, Write};
use std::sync::mpsc::Receiver;
use std::time::Duration;

use crossterm::Command;
use crossterm::event::{self, DisableMouseCapture, EnableMouseCapture};
use crossterm::terminal::{disable_raw_mode, enable_raw_mode};
use ratatui::backend::CrosstermBackend;
use ratatui::layout::Rect;
use ratatui::{Terminal, TerminalOptions, Viewport};

use crate::adapters::controllers::key_for;
use crate::adapters::gateways::unix;
use crate::adapters::presenters::panel::{Arrival, Effect, Panel};
use crate::adapters::presenters::{Style, screen_rows};

/// Handles the effects the terminal cannot: asking models, silencing.
pub type Outside<'a> = &'a mut dyn FnMut(Effect, &mut Panel);

const POLL: Duration = Duration::from_millis(50);
const CURSOR_ANSWER: Duration = Duration::from_secs(2);

/// Draws until the user closes the panel or takes a suggestion; the text to
/// insert, when they did. The panel takes the place of the bubble: the
/// cursor climbs `above` rows to the prompt's first line, then over the
/// bubble's rows, and everything below is cleared before drawing. On
/// close the bubble is printed again and the cursor is left where the
/// prompt's first line goes, so the hook only has to redraw the prompt.
pub fn run(
    panel: &mut Panel,
    style: &Style,
    arrivals: &Receiver<Arrival>,
    outside: Outside<'_>,
    above: u16,
    bubble: Option<&str>,
) -> io::Result<Option<String>> {
    let mut tty = OpenOptions::new()
        .read(true)
        .write(true)
        .open(terminal_path())?;
    let _stdin = Diversion::of(libc_stdin(), &tty);
    let _stdout = Diversion::of(libc_stdout(), &tty);
    let _guard = TtyGuard::enter(tty.try_clone()?)?;
    let (columns, rows) = crossterm::terminal::size()?;
    let climb = above.saturating_add(bubble.map_or(0, |text| screen_rows(text, columns)));
    climb_and_clear(&mut tty, climb)?;
    let height = panel
        .height(columns, style)
        .min(rows.saturating_sub(1))
        .max(1);
    let top = place_viewport(&mut tty, height, rows)?;
    let viewport = Rect::new(0, top, columns, height);
    let mut terminal = Terminal::with_options(
        CrosstermBackend::new(tty.try_clone()?),
        TerminalOptions {
            viewport: Viewport::Fixed(viewport),
        },
    )?;
    terminal.hide_cursor()?;
    send(&tty, EnableMouseCapture)?;
    let taken = loop {
        terminal.draw(|frame| panel.render(frame, style))?;
        while let Ok(arrival) = arrivals.try_recv() {
            panel.receive(arrival);
        }
        if !event::poll(POLL)? {
            continue;
        }
        let Some(key) = key_for(&event::read()?) else {
            continue;
        };
        match panel.press(key) {
            Effect::Nothing => {}
            Effect::Close => break None,
            Effect::Insert(text) => break Some(text),
            Effect::Copy(text) => {
                copy_to_clipboard(&mut tty, &text)?;
                panel.receive(Arrival::Note("copied".into()));
            }
            other => outside(other, panel),
        }
    };
    send(&tty, DisableMouseCapture)?;
    terminal.show_cursor()?;
    write!(tty, "\x1b[{};1H\x1b[J", top + 1)?;
    for line in bubble.into_iter().flat_map(str::lines) {
        write!(tty, "{line}\r\n")?;
    }
    tty.flush()?;
    Ok(taken)
}

/// What the hook needs when there is nothing to expand: the cursor on the
/// prompt's first line, the screen clear below it.
pub fn clear_above(above: u16) -> io::Result<()> {
    let mut tty = OpenOptions::new().write(true).open(terminal_path())?;
    climb_and_clear(&mut tty, above)
}

fn climb_and_clear(tty: &mut File, rows: u16) -> io::Result<()> {
    if rows > 0 {
        write!(tty, "\x1b[{rows}A")?;
    }
    write!(tty, "\r\x1b[J")?;
    tty.flush()
}

/// Where the viewport goes: the cursor's row, after scrolling the screen
/// enough for `height` rows to fit below it.
fn place_viewport(tty: &mut File, height: u16, rows: u16) -> io::Result<u16> {
    let row = cursor_row(tty)?;
    let overflow = (row + height).saturating_sub(rows);
    if overflow > 0 {
        write!(tty, "{}", "\n".repeat(overflow as usize))?;
        tty.flush()?;
    }
    Ok(row - overflow)
}

/// Asks the terminal where the cursor is; raw mode must be on. The query
/// goes to the tty, not to stdout, which the shell is capturing.
fn cursor_row(tty: &mut File) -> io::Result<u16> {
    tty.write_all(b"\x1b[6n")?;
    tty.flush()?;
    let mut bytes = Vec::new();
    let mut byte = [0u8; 1];
    loop {
        if !unix::wait_readable(tty, CURSOR_ANSWER) {
            return Err(io::Error::other(
                "the terminal did not report the cursor position",
            ));
        }
        tty.read_exact(&mut byte)?;
        bytes.push(byte[0]);
        if byte[0] == b'R' && bytes.contains(&b'[') {
            break;
        }
        if bytes.len() > 64 {
            return Err(io::Error::other("unreadable cursor report"));
        }
    }
    parse_cursor_report(&bytes).ok_or_else(|| io::Error::other("unreadable cursor report"))
}

/// `ESC [ row ; column R`, possibly after typed-ahead bytes: the row, zero-based.
fn parse_cursor_report(bytes: &[u8]) -> Option<u16> {
    let text = String::from_utf8_lossy(bytes);
    let start = text.rfind("\x1b[")?;
    let body = text[start + 2..].strip_suffix('R')?;
    let row: u16 = body.split(';').next()?.parse().ok()?;
    Some(row.saturating_sub(1))
}

/// A crossterm command, written to the tty rather than to stdout.
fn send(tty: &File, command: impl Command) -> io::Result<()> {
    let mut sequence = String::new();
    command
        .write_ansi(&mut sequence)
        .map_err(|_| io::Error::other("cannot encode a terminal command"))?;
    let mut writer = tty;
    writer.write_all(sequence.as_bytes())?;
    writer.flush()
}

/// OSC 52: the terminal puts the text in the system clipboard, over SSH too.
fn copy_to_clipboard(tty: &mut File, text: &str) -> io::Result<()> {
    write!(tty, "\x1b]52;c;{}\x07", base64(text.as_bytes()))?;
    tty.flush()
}

fn base64(input: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(input.len().div_ceil(3) * 4);
    for chunk in input.chunks(3) {
        let bits = chunk
            .iter()
            .enumerate()
            .fold(0u32, |acc, (i, b)| acc | (u32::from(*b) << (16 - 8 * i)));
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(TABLE[((bits >> (18 - 6 * i)) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

/// The terminal's own device, found through stderr or stdin; `/dev/tty`
/// only as a last resort, since macOS cannot watch that alias for input.
fn terminal_path() -> String {
    unix::tty_name(2)
        .or_else(|| unix::tty_name(0))
        .unwrap_or_else(|| "/dev/tty".to_string())
}

fn libc_stdin() -> std::os::fd::RawFd {
    0
}

fn libc_stdout() -> std::os::fd::RawFd {
    1
}

/// A standard descriptor pointed at the tty while the panel runs; put back
/// on drop, so the text to insert reaches the shell.
struct Diversion {
    fd: std::os::fd::RawFd,
    saved: Option<std::os::fd::OwnedFd>,
}

impl Diversion {
    fn of(fd: std::os::fd::RawFd, tty: &File) -> Self {
        Self {
            fd,
            saved: unix::divert(fd, tty),
        }
    }
}

impl Drop for Diversion {
    fn drop(&mut self) {
        if let Some(saved) = self.saved.take() {
            unix::restore(self.fd, saved);
        }
    }
}

/// Raw mode for the panel's lifetime, restored even when drawing fails.
struct TtyGuard {
    tty: File,
}

impl TtyGuard {
    fn enter(tty: File) -> io::Result<Self> {
        enable_raw_mode()?;
        Ok(Self { tty })
    }
}

impl Drop for TtyGuard {
    fn drop(&mut self) {
        let _ = send(&self.tty, DisableMouseCapture);
        let _ = disable_raw_mode();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::fd::{AsRawFd, OwnedFd};

    /// A terminal to talk to: the slave as a `File`, the master to play the
    /// terminal emulator.
    fn pty() -> (File, File) {
        let (master, slave) = unix::open_pty().expect("a pseudo-terminal");
        (File::from(master), File::from(slave))
    }

    fn read_available(master: &mut File) -> Vec<u8> {
        let mut out = Vec::new();
        let mut byte = [0u8; 1];
        while unix::wait_readable(master, Duration::from_millis(50)) {
            if master.read(&mut byte).unwrap_or(0) == 0 {
                break;
            }
            out.push(byte[0]);
        }
        out
    }

    #[test]
    fn the_cursor_row_is_asked_on_the_tty_and_the_screen_scrolled_to_fit() {
        let (mut master, mut slave) = pty();
        master.write_all(b"\x1b[5;1R").unwrap();
        assert_eq!(cursor_row(&mut slave).unwrap(), 4);
        assert_eq!(read_available(&mut master), b"\x1b[6n");

        master.write_all(b"\x1b[20;3R").unwrap();
        let top = place_viewport(&mut slave, 8, 24).unwrap();
        assert_eq!(
            top, 16,
            "row 19 with 8 rows on a 24-row screen: scrolled by 3"
        );
        let written = read_available(&mut master);
        assert!(written.starts_with(b"\x1b[6n"), "{written:?}");
        assert_eq!(written.iter().filter(|b| **b == b'\n').count(), 3);

        let silent = cursor_row(&mut slave);
        assert!(silent.is_err(), "no answer within the wait is an error");
        let _ = OwnedFd::from(master);
    }

    #[test]
    fn the_clipboard_and_the_mouse_go_through_the_tty() {
        let (mut master, mut slave) = pty();
        copy_to_clipboard(&mut slave, "git status").unwrap();
        assert_eq!(
            read_available(&mut master),
            b"\x1b]52;c;Z2l0IHN0YXR1cw==\x07"
        );
        send(&slave, EnableMouseCapture).unwrap();
        let bytes = read_available(&mut master);
        assert!(bytes.starts_with(b"\x1b[?100"), "{bytes:?}");
        assert!(unix::tty_name(slave.as_raw_fd()).is_some());
    }

    #[test]
    fn the_cursor_report_is_read_even_after_typed_ahead_bytes() {
        assert_eq!(parse_cursor_report(b"\x1b[12;1R"), Some(11));
        assert_eq!(parse_cursor_report(b"abc\x1b[3;40R"), Some(2));
        assert_eq!(parse_cursor_report(b"\x1b[1;1R"), Some(0));
        assert_eq!(parse_cursor_report(b"\x1b[x;1R"), None);
        assert_eq!(parse_cursor_report(b"nothing"), None);
    }

    #[test]
    fn base64_matches_the_standard_alphabet_and_padding() {
        assert_eq!(base64(b""), "");
        assert_eq!(base64(b"f"), "Zg==");
        assert_eq!(base64(b"fo"), "Zm8=");
        assert_eq!(base64(b"foo"), "Zm9v");
        assert_eq!(base64(b"git status"), "Z2l0IHN0YXR1cw==");
    }
}
