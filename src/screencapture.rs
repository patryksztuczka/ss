//! Thin wrapper around macOS' own `/usr/sbin/screencapture` (port of
//! `Screencapture.ts`). It gives us the native selection UI (crosshair,
//! Space for window mode, Esc to cancel) while ss decides where files go.
//! Every call blocks; run it on the background executor.

use std::fmt;
use std::io;
use std::path::{Path, PathBuf};
use std::process::{ChildStdin, Command, Stdio};
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

use crate::displays::Rect;

const SCREENCAPTURE: &str = "/usr/sbin/screencapture";

#[derive(Debug)]
pub enum CaptureError {
    /// macOS refused to give us pixels: almost always a missing Screen
    /// Recording permission. Carries screencapture's stderr.
    Permission(String),
    Failed(String),
    Spawn(io::Error),
}

impl fmt::Display for CaptureError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Permission(detail) => write!(formatter, "Screen Recording permission: {detail}"),
            Self::Failed(message) => formatter.write_str(message),
            Self::Spawn(error) => write!(formatter, "Cannot run screencapture: {error}"),
        }
    }
}

impl std::error::Error for CaptureError {}

/// Native interactive selection (drag for area, Space for window, Esc to
/// cancel). `Ok(None)` means the user cancelled.
pub fn interactive(file: &Path) -> Result<Option<PathBuf>, CaptureError> {
    // -i interactive, -x silent, -o no window shadow (the editor adds its own).
    run(&["-i".into(), "-x".into(), "-o".into()], file)
}

/// Captures a rectangle in top-left global points, e.g. a whole display.
pub fn rect(file: &Path, area: Rect) -> Result<PathBuf, CaptureError> {
    run(&["-x".into(), rect_arg(area)], file)?
        .ok_or_else(|| CaptureError::Failed("Nothing was captured".into()))
}

fn run(args: &[String], file: &Path) -> Result<Option<PathBuf>, CaptureError> {
    let output = Command::new(SCREENCAPTURE)
        .args(args)
        .arg(file)
        .output()
        .map_err(CaptureError::Spawn)?;
    let stderr = String::from_utf8_lossy(&output.stderr);
    interpret(file, file.exists(), stderr.trim())
}

/// Starts `screencapture -v -x -R… <file>` recording `area`.
pub fn record(file: &Path, area: Rect) -> io::Result<RecordingProcess> {
    let mut command = Command::new(SCREENCAPTURE);
    command
        .args(["-v".into(), "-x".into(), rect_arg(area)])
        .arg(file);
    RecordingProcess::spawn(command)
}

/// How long `stop` waits after SIGINT, and then after SIGTERM.
#[derive(Clone, Copy, Debug)]
pub struct StopTimeouts {
    pub interrupt: Duration,
    pub terminate: Duration,
}

impl Default for StopTimeouts {
    fn default() -> Self {
        Self {
            interrupt: Duration::from_secs(5),
            terminate: Duration::from_secs(2),
        }
    }
}

/// The exit status of a child, published by its waiter thread.
#[derive(Clone, Default)]
pub struct ExitWatch(Arc<(Mutex<Option<i32>>, Condvar)>);

impl ExitWatch {
    /// Blocks until the process exits; returns its exit code (−1 if signalled).
    pub fn wait(&self) -> i32 {
        let (code, changed) = &*self.0;
        let mut code = code.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        loop {
            if let Some(value) = *code {
                return value;
            }
            code = changed
                .wait(code)
                .unwrap_or_else(|poisoned| poisoned.into_inner());
        }
    }

    /// Like [`wait`](Self::wait), but gives up after `timeout`.
    pub fn wait_timeout(&self, timeout: Duration) -> Option<i32> {
        let (code, changed) = &*self.0;
        let code = code.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        let (code, _) = changed
            .wait_timeout_while(code, timeout, |code| code.is_none())
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        *code
    }

    fn publish(&self, value: i32) {
        let (code, changed) = &*self.0;
        *code.lock().unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(value);
        changed.notify_all();
    }
}

/// A running recording. Its stdin stays open: screencapture treats input or
/// EOF on stdin as "stop recording".
pub struct RecordingProcess {
    pid: i32,
    _stdin: Option<ChildStdin>,
    exited: ExitWatch,
}

impl RecordingProcess {
    pub fn spawn(mut command: Command) -> io::Result<Self> {
        let mut child = command
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()?;
        let stdin = child.stdin.take();
        let pid = child.id() as i32;
        let exited = ExitWatch::default();
        let watch = exited.clone();
        std::thread::Builder::new()
            .name("recording-waiter".into())
            .spawn(move || {
                let code = child
                    .wait()
                    .ok()
                    .and_then(|status| status.code())
                    .unwrap_or(-1);
                watch.publish(code);
            })?;
        Ok(Self {
            pid,
            _stdin: stdin,
            exited,
        })
    }

    pub fn exited(&self) -> ExitWatch {
        self.exited.clone()
    }

    /// Asks the recording to finish (SIGINT, which finalises the movie),
    /// escalating to SIGTERM if it is ignored. Blocks for at most the sum
    /// of both timeouts.
    pub fn stop(&self, timeouts: StopTimeouts) {
        if self.exited.wait_timeout(Duration::ZERO).is_some() {
            return;
        }
        tracing::info!("stopping recording");
        // SAFETY: plain signal delivery to our own child.
        unsafe { libc::kill(self.pid, libc::SIGINT) };
        if self.exited.wait_timeout(timeouts.interrupt).is_some() {
            return;
        }
        tracing::warn!("screencapture ignored SIGINT, terminating");
        // SAFETY: as above.
        unsafe { libc::kill(self.pid, libc::SIGTERM) };
        self.exited.wait_timeout(timeouts.terminate);
    }
}

/// The file decides success; stderr only explains a missing file.
fn interpret(file: &Path, exists: bool, stderr: &str) -> Result<Option<PathBuf>, CaptureError> {
    if exists {
        Ok(Some(file.to_path_buf()))
    } else if looks_like_permission_error(stderr) {
        Err(CaptureError::Permission(stderr.into()))
    } else if !stderr.is_empty() {
        Err(CaptureError::Failed(stderr.into()))
    } else {
        Ok(None)
    }
}

/// `/could not create image|not authorized|permission|declined/i`
pub fn looks_like_permission_error(stderr: &str) -> bool {
    let stderr = stderr.to_lowercase();
    [
        "could not create image",
        "not authorized",
        "permission",
        "declined",
    ]
    .iter()
    .any(|needle| stderr.contains(needle))
}

/// `-R<x>,<y>,<w>,<h>` rounded like JavaScript's `Math.round`.
pub fn rect_arg(area: Rect) -> String {
    let round = |value: f64| (value + 0.5).floor() as i64;
    format!(
        "-R{},{},{},{}",
        round(area.x),
        round(area.y),
        round(area.width),
        round(area.height)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rect_argument_rounds_like_javascript() {
        assert_eq!(
            rect_arg(Rect::new(0.0, 0.0, 1512.0, 982.0)),
            "-R0,0,1512,982"
        );
        assert_eq!(
            rect_arg(Rect::new(-1920.4, -97.5, 1919.6, 1080.5)),
            "-R-1920,-97,1920,1081"
        );
        assert_eq!(rect_arg(Rect::new(-0.5, 2.5, 10.49, 0.0)), "-R0,3,10,0");
    }

    #[test]
    fn permission_errors_are_recognised() {
        for stderr in [
            "could not create image from display 1",
            "screencapture: Not Authorized",
            "Permission denied",
            "The user declined screen recording",
        ] {
            assert!(looks_like_permission_error(stderr), "{stderr}");
        }
        assert!(!looks_like_permission_error("invalid rect"));
        assert!(!looks_like_permission_error(""));
    }

    #[test]
    fn the_file_decides_success() {
        let file = Path::new("/tmp/capture.png");
        assert_eq!(
            interpret(file, true, "could not create image").unwrap(),
            Some(file.to_path_buf())
        );
        assert!(matches!(
            interpret(file, false, "could not create image from display"),
            Err(CaptureError::Permission(_))
        ));
        assert!(matches!(
            interpret(file, false, "invalid display"),
            Err(CaptureError::Failed(message)) if message == "invalid display"
        ));
        // Esc during interactive selection: no file, no output.
        assert_eq!(interpret(file, false, "").unwrap(), None);
    }
}
