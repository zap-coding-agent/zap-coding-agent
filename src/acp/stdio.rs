//! Process-level stdio isolation for ACP mode.
//!
//! ACP speaks newline-delimited JSON-RPC over stdin/stdout. zap has hundreds of
//! `println!` call sites (commands, hooks, CLI permission prompts) and spawns
//! child processes (shell tool, LSP servers, MCP servers) that inherit fd 0/1.
//! Any one of them writing to stdout — or reading stdin — corrupts the protocol.
//!
//! Instead of auditing every call site, take private duplicates of fd 0/1 for
//! the protocol, then point fd 1 at stderr and fd 0 at the null device. Every
//! other writer in the process (and every child) now lands on stderr, which ACP
//! clients treat as a log stream.

use std::fs::File;
use std::io;

/// The protocol's private ends of the original stdin/stdout.
pub struct ProtocolStdio {
    pub stdin: File,
    pub stdout: File,
}

/// Must be called before anything else writes to stdout or spawns a child.
pub fn isolate() -> io::Result<ProtocolStdio> {
    imp::isolate()
}

#[cfg(unix)]
mod imp {
    use super::ProtocolStdio;
    use std::fs::File;
    use std::io;
    use std::os::fd::FromRawFd;

    fn check(rc: libc::c_int) -> io::Result<libc::c_int> {
        if rc < 0 { Err(io::Error::last_os_error()) } else { Ok(rc) }
    }

    pub fn isolate() -> io::Result<ProtocolStdio> {
        // SAFETY: plain fd syscalls on the process's own standard descriptors;
        // each duplicated fd is owned by exactly one `File`.
        unsafe {
            let proto_in = check(libc::fcntl(libc::STDIN_FILENO, libc::F_DUPFD_CLOEXEC, 3))?;
            let proto_out = check(libc::fcntl(libc::STDOUT_FILENO, libc::F_DUPFD_CLOEXEC, 3))?;

            check(libc::dup2(libc::STDERR_FILENO, libc::STDOUT_FILENO))?;

            let null = check(libc::open(c"/dev/null".as_ptr(), libc::O_RDONLY))?;
            check(libc::dup2(null, libc::STDIN_FILENO))?;
            libc::close(null);

            Ok(ProtocolStdio {
                stdin: File::from_raw_fd(proto_in),
                stdout: File::from_raw_fd(proto_out),
            })
        }
    }
}

#[cfg(windows)]
mod imp {
    use super::ProtocolStdio;
    use std::fs::File;
    use std::io;
    use std::os::windows::io::FromRawHandle;

    fn check(rc: libc::c_int) -> io::Result<libc::c_int> {
        if rc < 0 { Err(io::Error::last_os_error()) } else { Ok(rc) }
    }

    pub fn isolate() -> io::Result<ProtocolStdio> {
        // SAFETY: CRT fd calls on the process's own standard descriptors. The
        // UCRT's `_dup2` onto fds 0-2 also updates the Win32 std handles, which
        // is what Rust's `std::io::stdout()` writes through.
        unsafe {
            let proto_in = check(libc::dup(0))?;
            let proto_out = check(libc::dup(1))?;

            check(libc::dup2(2, 1))?;

            let null = check(libc::open(c"NUL".as_ptr(), libc::O_RDONLY))?;
            check(libc::dup2(null, 0))?;
            libc::close(null);

            let h_in = libc::get_osfhandle(proto_in);
            let h_out = libc::get_osfhandle(proto_out);
            if h_in == -1 || h_out == -1 {
                return Err(io::Error::last_os_error());
            }
            Ok(ProtocolStdio {
                stdin: File::from_raw_handle(h_in as _),
                stdout: File::from_raw_handle(h_out as _),
            })
        }
    }
}
