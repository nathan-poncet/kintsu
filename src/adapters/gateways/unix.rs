//! The few calls that need the C library: leaving the terminal's session,
//! knowing who is on the other end of a socket, poking a shell with a
//! signal. The only module in the crate allowed `unsafe`.
#![allow(unsafe_code)]

use std::os::fd::AsRawFd;
use std::os::unix::net::UnixStream;

/// A session of its own and no SIGHUP when the terminal closes.
pub fn detach() {
    // SAFETY: setsid and signal take no pointers and cannot break memory safety.
    unsafe {
        libc::setsid();
        libc::signal(libc::SIGHUP, libc::SIG_IGN);
    }
}

pub fn uid() -> u32 {
    // SAFETY: getuid has no preconditions.
    unsafe { libc::getuid() }
}

/// The uid of the process on the other end of a Unix socket.
#[cfg(target_os = "linux")]
pub fn peer_uid(stream: &UnixStream) -> Option<u32> {
    let mut cred = libc::ucred {
        pid: 0,
        uid: 0,
        gid: 0,
    };
    let mut len = std::mem::size_of::<libc::ucred>() as libc::socklen_t;
    // SAFETY: the buffer and its length describe a valid ucred.
    let rc = unsafe {
        libc::getsockopt(
            stream.as_raw_fd(),
            libc::SOL_SOCKET,
            libc::SO_PEERCRED,
            (&mut cred as *mut libc::ucred).cast(),
            &mut len,
        )
    };
    (rc == 0).then_some(cred.uid)
}

/// The uid of the process on the other end of a Unix socket.
#[cfg(not(target_os = "linux"))]
pub fn peer_uid(stream: &UnixStream) -> Option<u32> {
    let mut uid: libc::uid_t = 0;
    let mut gid: libc::gid_t = 0;
    // SAFETY: both out-pointers point to live locals.
    let rc = unsafe { libc::getpeereid(stream.as_raw_fd(), &mut uid, &mut gid) };
    (rc == 0).then_some(uid)
}

/// True when `fd` has bytes to read before `timeout` runs out. `select`,
/// not `poll`: on macOS, `poll` does not work on terminal devices.
pub fn wait_readable(fd: &impl AsRawFd, timeout: std::time::Duration) -> bool {
    let fd = fd.as_raw_fd();
    if fd < 0 || fd >= libc::FD_SETSIZE as libc::c_int {
        return false;
    }
    let mut deadline = libc::timeval {
        tv_sec: libc::time_t::try_from(timeout.as_secs()).unwrap_or(libc::time_t::MAX),
        tv_usec: libc::suseconds_t::from(timeout.subsec_micros() as i32),
    };
    // SAFETY: the set is zeroed before use, fd is within FD_SETSIZE, and
    // select writes only into the set and the timeval we own.
    unsafe {
        let mut readable: libc::fd_set = std::mem::zeroed();
        libc::FD_ZERO(&mut readable);
        libc::FD_SET(fd, &mut readable);
        let ready = libc::select(
            fd + 1,
            &mut readable,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            &mut deadline,
        );
        ready > 0 && libc::FD_ISSET(fd, &readable)
    }
}

/// The device behind `fd` when it is a terminal: `/dev/ttys003`, not the
/// `/dev/tty` alias, which some event loops cannot watch on macOS.
pub fn tty_name(fd: std::os::fd::RawFd) -> Option<String> {
    let mut buffer = [0 as libc::c_char; 256];
    // SAFETY: ttyname_r writes at most `len` bytes into the buffer we own
    // and NUL-terminates on success.
    let status = unsafe { libc::ttyname_r(fd, buffer.as_mut_ptr(), buffer.len()) };
    if status != 0 {
        return None;
    }
    let bytes: Vec<u8> = buffer
        .iter()
        .take_while(|c| **c != 0)
        .map(|c| *c as u8)
        .collect();
    String::from_utf8(bytes).ok()
}

/// Points `fd` (stdin or stdout) at `target` and hands back the original,
/// to give to `restore`. For libraries that talk to the terminal through
/// the standard descriptors no matter what those are.
pub fn divert(fd: std::os::fd::RawFd, target: &impl AsRawFd) -> Option<std::os::fd::OwnedFd> {
    use std::os::fd::FromRawFd;
    // SAFETY: dup returns a fresh descriptor we own; dup2 replaces `fd`
    // with a copy of a descriptor the caller keeps alive.
    unsafe {
        let saved = libc::dup(fd);
        if saved < 0 || libc::dup2(target.as_raw_fd(), fd) < 0 {
            if saved >= 0 {
                libc::close(saved);
            }
            return None;
        }
        Some(std::os::fd::OwnedFd::from_raw_fd(saved))
    }
}

pub fn restore(fd: std::os::fd::RawFd, saved: std::os::fd::OwnedFd) {
    // SAFETY: dup2 onto `fd` from a descriptor we own; the owned fd is
    // closed when it drops.
    unsafe {
        libc::dup2(saved.as_raw_fd(), fd);
    }
}

/// A pseudo-terminal pair (master, slave) with a raw slave, for tests that
/// need a terminal to talk to.
#[cfg(test)]
pub fn open_pty() -> Option<(std::os::fd::OwnedFd, std::os::fd::OwnedFd)> {
    use std::os::fd::FromRawFd;
    let (mut master, mut slave) = (-1, -1);
    // SAFETY: openpty writes two descriptors into the ints we own; the
    // name, termios and winsize pointers may be null.
    let status = unsafe {
        libc::openpty(
            &mut master,
            &mut slave,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        )
    };
    if status != 0 || master < 0 || slave < 0 {
        return None;
    }
    // SAFETY: tcgetattr and tcsetattr read and write a termios we own, on a
    // descriptor we just received; both descriptors are then owned by
    // nobody else.
    unsafe {
        let mut termios: libc::termios = std::mem::zeroed();
        libc::tcgetattr(slave, &mut termios);
        libc::cfmakeraw(&mut termios);
        libc::tcsetattr(slave, libc::TCSANOW, &termios);
        Some((
            std::os::fd::OwnedFd::from_raw_fd(master),
            std::os::fd::OwnedFd::from_raw_fd(slave),
        ))
    }
}

/// True when the signal was sent; false when the process is gone.
pub fn signal_usr1(pid: u32) -> bool {
    let Ok(pid) = libc::pid_t::try_from(pid) else {
        return false;
    };
    // SAFETY: kill with a valid signal number has no memory effects.
    unsafe { libc::kill(pid, libc::SIGUSR1) == 0 }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_diverted_descriptor_writes_elsewhere_until_restored() {
        use std::io::{Read, Write};
        let (mut original_reader, mut writer) = std::io::pipe().unwrap();
        let (mut other_reader, other_writer) = std::io::pipe().unwrap();
        let saved = divert(writer.as_raw_fd(), &other_writer).unwrap();
        writer.write_all(b"diverted").unwrap();
        let mut got = [0u8; 8];
        other_reader.read_exact(&mut got).unwrap();
        assert_eq!(&got, b"diverted");
        restore(writer.as_raw_fd(), saved);
        writer.write_all(b"back").unwrap();
        let mut got = [0u8; 4];
        original_reader.read_exact(&mut got).unwrap();
        assert_eq!(&got, b"back");
        drop(other_writer);
        assert!(
            divert(-1, &other_reader).is_none(),
            "a bad descriptor cannot be diverted"
        );
    }

    #[test]
    fn a_socket_has_no_tty_name_and_a_pty_slave_has_one() {
        let (a, _b) = UnixStream::pair().unwrap();
        assert_eq!(tty_name(a.as_raw_fd()), None);
        let (_master, slave) = open_pty().expect("a pseudo-terminal");
        let name = tty_name(slave.as_raw_fd()).expect("the slave's device");
        assert!(name.starts_with("/dev/"), "{name}");
    }

    #[test]
    fn a_readable_end_is_seen_and_a_quiet_one_times_out() {
        let (mut a, b) = UnixStream::pair().unwrap();
        let short = std::time::Duration::from_millis(20);
        assert!(!wait_readable(&b, short));
        std::io::Write::write_all(&mut a, b"x").unwrap();
        assert!(wait_readable(&b, short));
    }

    #[test]
    fn a_socket_to_ourselves_belongs_to_us() {
        let (a, _b) = UnixStream::pair().unwrap();
        assert_eq!(peer_uid(&a), Some(uid()));
        assert!(!signal_usr1(u32::MAX), "no such process");
    }
}
