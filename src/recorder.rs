//! Screen recording state (port of `Recorder.ts`). One recording at a time;
//! a single finalisation path runs when the process exits, however it ended.

use std::fmt;
use std::io;
use std::path::Path;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::SystemTime;

use futures::channel::oneshot;

use crate::capture_store::Capture;
use crate::screencapture::{RecordingProcess, StopTimeouts};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RecorderState {
    Idle,
    Recording { started_at: SystemTime },
}

#[derive(Debug)]
pub enum RecorderError {
    AlreadyRecording,
    Spawn(io::Error),
}

impl fmt::Display for RecorderError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::AlreadyRecording => formatter.write_str("Already recording"),
            Self::Spawn(error) => write!(formatter, "Cannot start recording: {error}"),
        }
    }
}

impl std::error::Error for RecorderError {}

/// How a recording ended. The recorder is already idle when this arrives.
#[derive(Debug)]
pub struct Finished {
    pub capture: Capture,
    /// Whether the movie file exists ("Recording produced no file" otherwise).
    pub produced_file: bool,
    pub exit_code: i32,
}

struct Active {
    process: Arc<RecordingProcess>,
    started_at: SystemTime,
}

pub struct Recorder {
    active: Arc<Mutex<Option<Active>>>,
    timeouts: StopTimeouts,
}

impl Recorder {
    pub fn new(timeouts: StopTimeouts) -> Self {
        Self {
            active: Arc::default(),
            timeouts,
        }
    }

    pub fn state(&self) -> RecorderState {
        match &*lock(&self.active) {
            Some(active) => RecorderState::Recording {
                started_at: active.started_at,
            },
            None => RecorderState::Idle,
        }
    }

    /// Starts recording into `capture.path` with the process `spawn` makes.
    /// The receiver resolves once the process has exited.
    pub fn start(
        &self,
        capture: Capture,
        spawn: impl FnOnce(&Path) -> io::Result<RecordingProcess>,
    ) -> Result<oneshot::Receiver<Finished>, RecorderError> {
        let mut active = lock(&self.active);
        if active.is_some() {
            return Err(RecorderError::AlreadyRecording);
        }
        let process = Arc::new(spawn(&capture.path).map_err(RecorderError::Spawn)?);
        let exited = process.exited();
        *active = Some(Active {
            process,
            started_at: SystemTime::now(),
        });
        drop(active);
        tracing::info!(path = %capture.path.display(), "recording started");

        let (sender, receiver) = oneshot::channel();
        let slot = self.active.clone();
        std::thread::Builder::new()
            .name("recording-finaliser".into())
            .spawn(move || {
                let exit_code = exited.wait();
                lock(&slot).take();
                let produced_file = capture.path.exists();
                if !produced_file {
                    tracing::warn!("Recording produced no file");
                }
                let _ = sender.send(Finished {
                    capture,
                    produced_file,
                    exit_code,
                });
            })
            .map_err(RecorderError::Spawn)?;
        Ok(receiver)
    }

    /// Stops the current recording; a no-op when idle. Blocks for at most
    /// the stop timeouts (5 s + 2 s by default).
    pub fn stop(&self) {
        let process = lock(&self.active)
            .as_ref()
            .map(|active| active.process.clone());
        if let Some(process) = process {
            process.stop(self.timeouts);
        }
    }
}

fn lock(active: &Mutex<Option<Active>>) -> MutexGuard<'_, Option<Active>> {
    active
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::process::Command;
    use std::time::{Duration, Instant};

    use futures::executor::block_on;

    use super::*;
    use crate::capture_store::CaptureKind;

    fn capture(directory: &Path) -> Capture {
        Capture {
            id: "test".into(),
            kind: CaptureKind::Video,
            path: directory.join("Recording.mov"),
        }
    }

    /// A stand-in for screencapture: `script` runs with the movie path as `$1`.
    fn fake(script: &'static str) -> impl FnOnce(&Path) -> io::Result<RecordingProcess> {
        move |path: &Path| {
            let mut command = Command::new("/bin/sh");
            command.args(["-c", script, "sh"]).arg(path);
            RecordingProcess::spawn(command)
        }
    }

    const FINALISES_ON_INT: &str =
        r#"trap 'echo movie > "$1"; exit 0' INT; while :; do sleep 0.02; done"#;
    const NO_FILE_ON_INT: &str = r#"trap 'exit 0' INT; while :; do sleep 0.02; done"#;
    const IGNORES_INT: &str = r#"trap '' INT; while :; do sleep 0.02; done"#;

    fn short() -> StopTimeouts {
        StopTimeouts {
            interrupt: Duration::from_millis(300),
            terminate: Duration::from_secs(2),
        }
    }

    fn wait_started() {
        // Let the shell install its trap before signalling it.
        std::thread::sleep(Duration::from_millis(150));
    }

    #[test]
    fn stopping_finalises_and_registers_the_file() {
        let directory = tempfile::tempdir().unwrap();
        let recorder = Recorder::new(short());
        let finished = recorder
            .start(capture(directory.path()), fake(FINALISES_ON_INT))
            .unwrap();
        assert!(matches!(recorder.state(), RecorderState::Recording { .. }));
        wait_started();
        recorder.stop();
        let finished = block_on(finished).unwrap();
        assert!(finished.produced_file);
        assert_eq!(finished.exit_code, 0);
        assert_eq!(
            finished.capture.path,
            directory.path().join("Recording.mov")
        );
        assert_eq!(recorder.state(), RecorderState::Idle);
    }

    #[test]
    fn a_recording_without_a_file_still_returns_to_idle() {
        let directory = tempfile::tempdir().unwrap();
        let recorder = Recorder::new(short());
        let finished = recorder
            .start(capture(directory.path()), fake(NO_FILE_ON_INT))
            .unwrap();
        wait_started();
        recorder.stop();
        let finished = block_on(finished).unwrap();
        assert!(!finished.produced_file);
        assert_eq!(recorder.state(), RecorderState::Idle);
    }

    #[test]
    fn only_one_recording_at_a_time() {
        let directory = tempfile::tempdir().unwrap();
        let recorder = Recorder::new(short());
        let finished = recorder
            .start(capture(directory.path()), fake(FINALISES_ON_INT))
            .unwrap();
        let second = recorder.start(capture(directory.path()), fake(FINALISES_ON_INT));
        assert!(matches!(second, Err(RecorderError::AlreadyRecording)));
        assert_eq!(second.unwrap_err().to_string(), "Already recording");
        wait_started();
        recorder.stop();
        block_on(finished).unwrap();
        // Idle again: a new recording may start.
        let again = recorder
            .start(capture(directory.path()), fake(FINALISES_ON_INT))
            .unwrap();
        wait_started();
        recorder.stop();
        block_on(again).unwrap();
    }

    #[test]
    fn stopping_while_idle_does_nothing() {
        let recorder = Recorder::new(StopTimeouts::default());
        let started = Instant::now();
        recorder.stop();
        assert!(started.elapsed() < Duration::from_millis(50));
        assert_eq!(recorder.state(), RecorderState::Idle);
    }

    #[test]
    fn ignoring_sigint_escalates_to_sigterm() {
        let directory = tempfile::tempdir().unwrap();
        let recorder = Recorder::new(short());
        let finished = recorder
            .start(capture(directory.path()), fake(IGNORES_INT))
            .unwrap();
        wait_started();
        let started = Instant::now();
        recorder.stop();
        let elapsed = started.elapsed();
        assert!(
            elapsed >= Duration::from_millis(300),
            "waited {elapsed:?} for SIGINT"
        );
        assert!(
            elapsed < Duration::from_millis(1500),
            "SIGTERM took {elapsed:?}"
        );
        let finished = block_on(finished).unwrap();
        assert_eq!(finished.exit_code, -1, "terminated by a signal");
        assert_eq!(recorder.state(), RecorderState::Idle);
    }

    #[test]
    fn a_process_that_ends_on_its_own_is_finalised() {
        let directory = tempfile::tempdir().unwrap();
        let recorder = Recorder::new(short());
        let finished = recorder
            .start(
                capture(directory.path()),
                fake(r#"echo movie > "$1"; exit 0"#),
            )
            .unwrap();
        let finished = block_on(finished).unwrap();
        assert!(finished.produced_file);
        assert_eq!(recorder.state(), RecorderState::Idle);
    }

    #[test]
    fn spawn_failures_leave_the_recorder_idle() {
        let recorder = Recorder::new(short());
        let result = recorder.start(capture(&PathBuf::from("/tmp")), |_| {
            RecordingProcess::spawn(Command::new("/nonexistent/screencapture"))
        });
        assert!(matches!(result, Err(RecorderError::Spawn(_))));
        assert_eq!(recorder.state(), RecorderState::Idle);
    }
}
