//! Owned native test processes must not outlive a failed Cargo test. Nonblocking
//! pipes keep both diagnostics and the deadline bounded, including noisy children.
use std::{
    io::{self, Read},
    os::{fd::AsRawFd, unix::process::CommandExt},
    process::{Child, Command, Output, Stdio},
    thread,
    time::{Duration, Instant},
};

const OUTPUT_LIMIT: usize = 1024 * 1024;
const POLL_INTERVAL: Duration = Duration::from_millis(10);

pub(super) struct OwnedChild(pub(super) Child);

impl Drop for OwnedChild {
    fn drop(&mut self) {
        // SAFETY: process_group(0) creates a group owned by this child. A
        // negative PID targets only that group, including the leaks wrapper's
        // test child. Always reap our direct child, even after an I/O failure.
        if let Ok(pid) = i32::try_from(self.0.id()) {
            unsafe { libc::kill(-pid, libc::SIGKILL) };
        }
        let _ = self.0.wait();
    }
}

pub(super) fn nonblocking(pipe: &impl AsRawFd) -> io::Result<()> {
    // SAFETY: the borrowed pipe owns a valid descriptor for both calls. We only
    // add O_NONBLOCK and preserve every other descriptor status flag.
    let flags = unsafe { libc::fcntl(pipe.as_raw_fd(), libc::F_GETFL) };
    if flags < 0
        || unsafe { libc::fcntl(pipe.as_raw_fd(), libc::F_SETFL, flags | libc::O_NONBLOCK) } < 0
    {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

pub(super) fn drain(pipe: &mut impl Read, output: &mut Vec<u8>) -> io::Result<()> {
    // A continuously writing child cannot starve the next deadline check.
    let mut chunk = [0; 8192];
    for _ in 0..8 {
        match pipe.read(&mut chunk) {
            Ok(0) => break,
            Ok(count) => {
                if output.len() + count > OUTPUT_LIMIT {
                    return Err(io::Error::other(
                        "native test diagnostic output exceeded 1 MiB",
                    ));
                }
                output.extend_from_slice(&chunk[..count]);
            }
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => break,
            Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
            Err(error) => return Err(error),
        }
    }
    Ok(())
}

pub(super) fn run(command: &mut Command, timeout: Duration) -> Result<Output, String> {
    let label = format!("{command:?}");
    let mut child = OwnedChild(
        command
            .process_group(0)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|error| format!("cannot launch {label}: {error}"))?,
    );
    let mut stdout_pipe = child.0.stdout.take().expect("requested stdout pipe");
    let mut stderr_pipe = child.0.stderr.take().expect("requested stderr pipe");
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    let result = (|| {
        nonblocking(&stdout_pipe)?;
        nonblocking(&stderr_pipe)?;
        let started = Instant::now();
        loop {
            drain(&mut stdout_pipe, &mut stdout)?;
            drain(&mut stderr_pipe, &mut stderr)?;
            if let Some(status) = child.0.try_wait()? {
                // The child has exited: drain the bounded remaining pipe data.
                // Descendants holding a pipe open cannot stall nonblocking reads.
                for _ in 0..=(OUTPUT_LIMIT / 65536) {
                    drain(&mut stdout_pipe, &mut stdout)?;
                    drain(&mut stderr_pipe, &mut stderr)?;
                }
                return Ok(status);
            }
            if started.elapsed() >= timeout {
                return Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "native test deadline exceeded",
                ));
            }
            thread::sleep(POLL_INTERVAL.min(timeout.saturating_sub(started.elapsed())));
        }
    })();
    match result {
        Ok(status) => Ok(Output {
            status,
            stdout,
            stderr,
        }),
        Err(error) => Err(format!(
            "{label}: {error}\nstdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&stdout),
            String::from_utf8_lossy(&stderr)
        )),
    }
}
