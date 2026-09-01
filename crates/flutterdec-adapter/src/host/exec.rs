//! Starting, bounding, and reaping one adapter child.
//!
//! Three things have to be true at once and each of them can break the other
//! two. The host must not block forever on a child that never exits, must not
//! grow without bound on a child that never stops writing, and must not leave a
//! process behind when it gives up on either. So the output is drained by two
//! threads that never stop reading (a reader that stops reading is a child that
//! blocks on `write` and never notices it is being killed), the wait is a poll
//! against a deadline rather than a blocking `wait`, and every exit path ends
//! with one signal to the whole process group.
//!
//! The group signal is sent even when the child exited on its own. A child that
//! forked and abandoned a grandchild is the normal case for a backend that
//! shells out, and that grandchild holds the inherited pipe open: without the
//! sweep the drain threads would never see end of file and the host would hang
//! after a perfectly successful run.

use super::image::ExecImage;
use super::{HostError, OutputStream};
use crate::sandbox::{kill_tree, Containment, ContainmentReport, Limits};
use std::io::Read;
use std::os::unix::process::{CommandExt, ExitStatusExt};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

/// How often the deadline loop looks at the child.
///
/// Short enough that the overshoot past a deadline is noise next to any real
/// adapter, long enough that a slow adapter does not cost a busy loop.
const POLL_INTERVAL: Duration = Duration::from_millis(2);

/// How one invocation ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Completion {
    Exited { code: i32 },
    Signalled { signal: i32 },
    Timeout { after: Duration },
    OutputLimit { stream: OutputStream, limit: u64 },
}

pub(crate) struct Execution {
    pub(crate) completion: Completion,
    pub(crate) stdout: Vec<u8>,
    pub(crate) stderr: Vec<u8>,
    pub(crate) containment: ContainmentReport,
}

/// Read a stream to end of file, keeping at most `limit` bytes.
///
/// Reading continues past the limit and discards. That is the point: the caller
/// wants to *terminate* a flooding child, and it cannot do that if the child is
/// blocked writing into a pipe nobody is emptying.
fn drain<R: Read + Send + 'static>(
    mut source: R,
    limit: u64,
    seen: Arc<AtomicU64>,
) -> thread::JoinHandle<Vec<u8>> {
    thread::spawn(move || {
        let mut kept = Vec::new();
        let mut buffer = [0u8; 16 * 1024];
        loop {
            let read = match source.read(&mut buffer) {
                Ok(0) => break,
                Ok(read) => read,
                Err(ref err) if err.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(_) => break,
            };
            let total = seen.fetch_add(read as u64, Ordering::Relaxed) + read as u64;
            let room = limit.saturating_sub(total.saturating_sub(read as u64));
            if room > 0 {
                let take = usize::try_from(room).unwrap_or(read).min(read);
                kept.extend_from_slice(&buffer[..take]);
            }
        }
        kept
    })
}

/// Spawn, tolerating a file the kernel still considers open for writing.
///
/// Where the image is an ordinary inode rather than an anonymous one, the host
/// writes it and executes it immediately. The descriptor it wrote through is
/// closed by then, but any other thread that forked while it was open holds a
/// copy until that child execs, and the kernel answers `ETXTBSY` for as long as
/// one exists. That is a property of the host being multi-threaded, not of the
/// artifact, so it is waited out rather than reported.
fn spawn_retrying_busy_text(command: &mut Command) -> std::io::Result<std::process::Child> {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        match command.spawn() {
            Err(err) if err.raw_os_error() == Some(libc::ETXTBSY) && Instant::now() < deadline => {
                thread::sleep(POLL_INTERVAL);
            }
            other => return other,
        }
    }
}

/// Start the command, hold it to its limits, and reap it.
///
/// `command` carries only what the standard library sets up before a pre-exec
/// hook runs: the working directory and the three standard streams. What the
/// child actually becomes is `image`, executed from its descriptor by the last
/// hook registered here, with the arguments and environment the image was built
/// with. The program named by `command` is never reached.
pub(crate) fn run(
    mut command: Command,
    limits: &Limits,
    image: Arc<ExecImage>,
) -> Result<Execution, HostError> {
    let containment = Containment::prepare(limits)
        .map_err(|err| HostError::Spawn(format!("create the containment status pipe: {err}")))?;
    containment.install(&mut command);
    // Registered after the containment hook so it runs after it: every limit,
    // every isolation step and the status record all have to be in place before
    // the image replaces the child, and this call does not return.
    let refusal = Arc::clone(&image);
    unsafe {
        command.pre_exec(move || Err(image.exec()));
    }

    let mut child = match spawn_retrying_busy_text(&mut command) {
        Ok(child) => child,
        // Nothing was created, so there is nothing to describe. The image knows
        // whether this is a failure to start a process or its own refusal to
        // execute something that is no longer the verified bytes.
        Err(err) => return Err(refusal.spawn_refusal(err)),
    };
    let pid = child.id();

    let stdout_seen = Arc::new(AtomicU64::new(0));
    let stderr_seen = Arc::new(AtomicU64::new(0));
    let stdout = drain(
        child.stdout.take().expect("stdout is piped"),
        limits.max_stdout_bytes,
        Arc::clone(&stdout_seen),
    );
    let stderr = drain(
        child.stderr.take().expect("stderr is piped"),
        limits.max_stderr_bytes,
        Arc::clone(&stderr_seen),
    );

    let started = Instant::now();
    let mut completion = None;
    let mut terminated = false;
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                // Reaped. Sweep the group before anything else so an abandoned
                // grandchild cannot outlive the run or hold the pipes open.
                kill_tree(pid);
                completion = Some(match status.code() {
                    Some(code) => Completion::Exited { code },
                    None => Completion::Signalled {
                        signal: status.signal().unwrap_or(0),
                    },
                });
                break;
            }
            Ok(None) => {}
            Err(err) => {
                kill_tree(pid);
                let _ = child.kill();
                let _ = child.wait();
                return Err(HostError::Io(format!("wait for the adapter: {err}")));
            }
        }

        if stdout_seen.load(Ordering::Relaxed) > limits.max_stdout_bytes {
            completion = Some(Completion::OutputLimit {
                stream: OutputStream::Stdout,
                limit: limits.max_stdout_bytes,
            });
        } else if stderr_seen.load(Ordering::Relaxed) > limits.max_stderr_bytes {
            completion = Some(Completion::OutputLimit {
                stream: OutputStream::Stderr,
                limit: limits.max_stderr_bytes,
            });
        } else if started.elapsed() >= limits.wall_clock {
            completion = Some(Completion::Timeout {
                after: limits.wall_clock,
            });
        }

        if completion.is_some() {
            // The group first, so a fork bomb loses its members before the
            // leader is reaped and the pid is free to be reused.
            terminated = true;
            kill_tree(pid);
            let _ = child.kill();
            let _ = child.wait();
            break;
        }

        thread::sleep(POLL_INTERVAL);
    }

    // Both ends of both pipes are closed once the tree is gone, so these joins
    // terminate.
    let stdout = stdout.join().unwrap_or_default();
    let stderr = stderr.join().unwrap_or_default();

    Ok(Execution {
        completion: completion.expect("the loop only exits with a completion"),
        stdout,
        stderr,
        containment: containment.collect(terminated),
    })
}
