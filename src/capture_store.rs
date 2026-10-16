//! Capture history (port of `CaptureStore.ts`). Screenshots live in the
//! throwaway `captures/` folder as `<id>.png` (+ `<id>.ocr.json`), pruned to
//! the newest [`KEEP_IMAGES`]. Ids sort chronologically, so the history can be
//! rebuilt from file names and "Edit Last Capture" survives restarts.

use std::collections::HashMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

const KEEP_IMAGES: usize = 60;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CaptureKind {
    Image,
    Video,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Capture {
    pub id: String,
    pub kind: CaptureKind,
    /// The image in the history folder, or the recording.
    pub path: PathBuf,
}

#[derive(Default)]
struct State {
    by_id: HashMap<String, Capture>,
    latest: Option<String>,
}

pub struct CaptureStore {
    directory: PathBuf,
    state: Mutex<State>,
}

impl CaptureStore {
    /// Creates `captures/` if needed and rebuilds the image history from disk.
    pub fn open(directory: PathBuf) -> io::Result<Self> {
        fs::create_dir_all(&directory)?;
        let images = image_ids(&directory).unwrap_or_default();
        let latest = images.last().cloned();
        let by_id = images
            .into_iter()
            .map(|id| {
                let capture = Capture {
                    path: directory.join(format!("{id}.png")),
                    id: id.clone(),
                    kind: CaptureKind::Image,
                };
                (id, capture)
            })
            .collect();
        Ok(Self {
            directory,
            state: Mutex::new(State { by_id, latest }),
        })
    }

    /// Reserves a path for a new screenshot; [`register`](Self::register) it once captured.
    pub fn new_image(&self) -> Capture {
        let id = new_id();
        Capture {
            path: self.directory.join(format!("{id}.png")),
            id,
            kind: CaptureKind::Image,
        }
    }

    /// Reserves `<directory>/Recording <date> at <time>.mov`.
    pub fn new_recording(&self, directory: &Path) -> io::Result<Capture> {
        fs::create_dir_all(directory)?;
        let name = timestamp_name("Recording", &jiff::Zoned::now());
        Ok(Capture {
            id: new_id(),
            kind: CaptureKind::Video,
            path: directory.join(format!("{name}.mov")),
        })
    }

    pub fn directory(&self) -> &Path {
        &self.directory
    }

    /// Adds the capture and makes it the latest. Callers prune images afterwards.
    pub fn register(&self, capture: Capture) {
        let mut state = self.state();
        state.latest = Some(capture.id.clone());
        state.by_id.insert(capture.id.clone(), capture);
    }

    #[allow(
        dead_code,
        reason = "preview and editor look captures up by id (M5/M6)"
    )]
    pub fn get(&self, id: &str) -> Option<Capture> {
        self.state().by_id.get(id).cloned()
    }

    pub fn latest(&self) -> Option<Capture> {
        let state = self.state();
        state
            .latest
            .as_ref()
            .and_then(|id| state.by_id.get(id))
            .cloned()
    }

    /// Deletes all but the newest [`KEEP_IMAGES`] screenshots and their OCR
    /// caches. Blocking; run on the background executor.
    pub fn prune(&self) -> io::Result<usize> {
        let images = image_ids(&self.directory)?;
        let stale = &images[..images.len().saturating_sub(KEEP_IMAGES)];
        for id in stale {
            remove_if_present(&self.directory.join(format!("{id}.png")))?;
            remove_if_present(&self.directory.join(format!("{id}.ocr.json")))?;
            self.state().by_id.remove(id);
        }
        Ok(stale.len())
    }

    fn state(&self) -> std::sync::MutexGuard<'_, State> {
        self.state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

/// Ids of `*.png` files, oldest first.
fn image_ids(directory: &Path) -> io::Result<Vec<String>> {
    let mut ids: Vec<String> = fs::read_dir(directory)?
        .filter_map(|entry| entry.ok())
        .filter_map(|entry| {
            let name = entry.file_name().into_string().ok()?;
            name.strip_suffix(".png").map(str::to_owned)
        })
        .collect();
    ids.sort();
    Ok(ids)
}

fn remove_if_present(path: &Path) -> io::Result<()> {
    match fs::remove_file(path) {
        Err(error) if error.kind() != io::ErrorKind::NotFound => Err(error),
        _ => Ok(()),
    }
}

/// Sortable ids: base36 epoch milliseconds, `-`, three random base36 digits.
pub fn new_id() -> String {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_millis() as u64);
    format_id(millis, fastrand::u32(..36 * 36 * 36))
}

fn format_id(millis: u64, suffix: u32) -> String {
    format!("{}-{:0>3}", base36(millis), base36(u64::from(suffix)))
}

fn base36(mut value: u64) -> String {
    const DIGITS: &[u8; 36] = b"0123456789abcdefghijklmnopqrstuvwxyz";
    let mut digits = Vec::new();
    loop {
        digits.push(DIGITS[(value % 36) as usize]);
        value /= 36;
        if value == 0 {
            break;
        }
    }
    digits.reverse();
    String::from_utf8(digits).expect("base36 digits are ASCII")
}

/// Human-friendly local-time name, e.g. `"ss 2026-10-12 at 20.45.03"`.
pub fn timestamp_name(prefix: &str, now: &jiff::Zoned) -> String {
    format!("{prefix} {}", now.strftime("%Y-%m-%d at %H.%M.%S"))
}

/// Width and height from a PNG's IHDR chunk.
pub fn png_size(bytes: &[u8]) -> Option<(u32, u32)> {
    const SIGNATURE: &[u8] = b"\x89PNG\r\n\x1a\n";
    if bytes.len() < 24 || !bytes.starts_with(SIGNATURE) || &bytes[12..16] != b"IHDR" {
        return None;
    }
    let read = |offset: usize| u32::from_be_bytes(bytes[offset..offset + 4].try_into().unwrap());
    Some((read(16), read(20)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn touch(directory: &Path, name: &str) {
        fs::write(directory.join(name), b"").unwrap();
    }

    #[test]
    fn ids_match_the_electron_format_and_sort_chronologically() {
        // A real Electron capture id: 2026-10-12 21:19:14.935 local, suffix "gxv".
        assert_eq!(format_id(1_791_832_754_935, 21_955), "mv5myi9j-gxv");
        assert_eq!(format_id(0, 0), "0-000");
        assert_eq!(format_id(35, 35), "z-00z");
        assert_eq!(format_id(1, 46_655), "1-zzz");
        let earlier = format_id(1_791_832_754_935, 46_655);
        let later = format_id(1_791_832_754_936, 0);
        assert!(earlier < later);
        let id = new_id();
        let (time, suffix) = id.split_once('-').unwrap();
        assert!(time.len() >= 8 && suffix.len() == 3, "{id}");
    }

    #[test]
    fn rebuilds_history_from_png_files() {
        let directory = tempfile::tempdir().unwrap();
        for name in [
            "mv5myi9j-gxv.png",
            "mv5myi9j-gxv.ocr.json",
            "mv5no66m-qn7.png",
            "notes.txt",
        ] {
            touch(directory.path(), name);
        }
        let store = CaptureStore::open(directory.path().to_path_buf()).unwrap();
        let latest = store.latest().unwrap();
        assert_eq!(latest.id, "mv5no66m-qn7");
        assert_eq!(latest.kind, CaptureKind::Image);
        assert_eq!(latest.path, directory.path().join("mv5no66m-qn7.png"));
        assert!(store.get("mv5myi9j-gxv").is_some());
        assert!(store.get("notes").is_none());
    }

    #[test]
    fn empty_or_missing_folder_has_no_latest() {
        let directory = tempfile::tempdir().unwrap();
        let captures = directory.path().join("captures");
        let store = CaptureStore::open(captures.clone()).unwrap();
        assert!(captures.is_dir());
        assert_eq!(store.latest(), None);
    }

    #[test]
    fn register_makes_a_capture_the_latest() {
        let directory = tempfile::tempdir().unwrap();
        let store = CaptureStore::open(directory.path().to_path_buf()).unwrap();
        let capture = store.new_image();
        assert_eq!(
            capture.path,
            directory.path().join(format!("{}.png", capture.id))
        );
        assert_eq!(store.get(&capture.id), None);
        store.register(capture.clone());
        assert_eq!(store.latest(), Some(capture.clone()));
        assert_eq!(store.get(&capture.id), Some(capture));
    }

    #[test]
    fn recordings_get_timestamp_names_in_their_folder() {
        let directory = tempfile::tempdir().unwrap();
        let store = CaptureStore::open(directory.path().join("captures")).unwrap();
        let movies = directory.path().join("Movies").join("ss");
        let recording = store.new_recording(&movies).unwrap();
        assert!(movies.is_dir());
        assert_eq!(recording.kind, CaptureKind::Video);
        let name = recording.path.file_name().unwrap().to_str().unwrap();
        assert!(
            name.starts_with("Recording ") && name.ends_with(".mov"),
            "{name}"
        );
    }

    #[test]
    fn prune_keeps_the_newest_sixty_images_and_their_ocr() {
        let directory = tempfile::tempdir().unwrap();
        for index in 0..63 {
            touch(directory.path(), &format!("id{index:03}.png"));
            touch(directory.path(), &format!("id{index:03}.ocr.json"));
        }
        touch(directory.path(), "orphan.ocr.json");
        let store = CaptureStore::open(directory.path().to_path_buf()).unwrap();
        assert_eq!(store.prune().unwrap(), 3);
        for stale in ["id000", "id001", "id002"] {
            assert!(!directory.path().join(format!("{stale}.png")).exists());
            assert!(!directory.path().join(format!("{stale}.ocr.json")).exists());
            assert_eq!(store.get(stale), None);
        }
        assert!(directory.path().join("id003.png").exists());
        assert!(directory.path().join("id003.ocr.json").exists());
        assert!(directory.path().join("orphan.ocr.json").exists());
        assert_eq!(store.latest().unwrap().id, "id062");
        assert_eq!(store.prune().unwrap(), 0);
    }

    #[test]
    fn timestamp_names_use_local_wall_time() {
        let now: jiff::Zoned = "2026-10-12T20:45:03+02:00[Europe/Warsaw]".parse().unwrap();
        assert_eq!(timestamp_name("ss", &now), "ss 2026-10-12 at 20.45.03");
        let now: jiff::Zoned = "2026-01-02T03:04:05-05:00[America/New_York]"
            .parse()
            .unwrap();
        assert_eq!(
            timestamp_name("Recording", &now),
            "Recording 2026-01-02 at 03.04.05"
        );
    }

    #[test]
    fn reads_png_dimensions() {
        let mut png = b"\x89PNG\r\n\x1a\n\0\0\0\x0dIHDR".to_vec();
        png.extend_from_slice(&3024u32.to_be_bytes());
        png.extend_from_slice(&1964u32.to_be_bytes());
        assert_eq!(png_size(&png), Some((3024, 1964)));
        assert_eq!(png_size(b"GIF89a"), None);
    }
}
