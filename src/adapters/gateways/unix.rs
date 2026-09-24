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
    fn a_socket_to_ourselves_belongs_to_us() {
        let (a, _b) = UnixStream::pair().unwrap();
        assert_eq!(peer_uid(&a), Some(uid()));
        assert!(!signal_usr1(u32::MAX), "no such process");
    }
}
