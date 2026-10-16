use std::path::{Path, PathBuf};

use color_eyre::eyre::{Result, eyre};

/// Overrides the Application Support folder. Used by tests and development runs
/// so they never touch `~/Library/Application Support/ss`.
pub const SUPPORT_DIR_ENV: &str = "SS_APPLICATION_SUPPORT_DIR";

/// Every location ss reads or writes. The data folder is shared with the
/// Electron build and read in place.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Paths {
    pub data: PathBuf,
    /// Throwaway capture history (pruned). Nothing is ever written to the Desktop.
    pub captures: PathBuf,
    pub settings_file: PathBuf,
    pub logs: PathBuf,
    pub log_file: PathBuf,
    pub lock_file: PathBuf,
    pub default_images: PathBuf,
    pub default_recordings: PathBuf,
}

impl Paths {
    pub fn resolve() -> Result<Self> {
        let data = match std::env::var_os(SUPPORT_DIR_ENV) {
            Some(path) if !path.is_empty() => PathBuf::from(path),
            _ => dirs::data_dir()
                .ok_or_else(|| eyre!("application data directory is unavailable"))?
                .join("ss"),
        };
        let pictures =
            dirs::picture_dir().ok_or_else(|| eyre!("Pictures folder is unavailable"))?;
        let movies = dirs::video_dir().ok_or_else(|| eyre!("Movies folder is unavailable"))?;
        Ok(Self::new(data, &pictures, &movies))
    }

    pub fn new(data: PathBuf, pictures: &Path, movies: &Path) -> Self {
        let logs = data.join("logs");
        Self {
            captures: data.join("captures"),
            settings_file: data.join("settings.json"),
            log_file: logs.join("ss.log"),
            logs,
            lock_file: data.join("ss.lock"),
            default_images: pictures.join("ss"),
            default_recordings: movies.join("ss"),
            data,
        }
    }
}

/// `/Users/name/Pictures/ss` → `~/Pictures/ss`, as the Settings window shows it.
pub fn tildify(path: &Path) -> String {
    let text = path.to_string_lossy();
    if let Some(rest) = text.strip_prefix("/Users/") {
        let end = rest.find('/').unwrap_or(rest.len());
        if end > 0 {
            return format!("~{}", &rest[end..]);
        }
    }
    text.into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layout_matches_the_electron_data_folder() {
        let paths = Paths::new(
            PathBuf::from("/data/ss"),
            Path::new("/home/Pictures"),
            Path::new("/home/Movies"),
        );
        assert_eq!(paths.captures, Path::new("/data/ss/captures"));
        assert_eq!(paths.settings_file, Path::new("/data/ss/settings.json"));
        assert_eq!(paths.log_file, Path::new("/data/ss/logs/ss.log"));
        assert_eq!(paths.lock_file, Path::new("/data/ss/ss.lock"));
        assert_eq!(paths.default_images, Path::new("/home/Pictures/ss"));
        assert_eq!(paths.default_recordings, Path::new("/home/Movies/ss"));
    }

    #[test]
    fn tildify_replaces_only_the_home_prefix() {
        assert_eq!(tildify(Path::new("/Users/me/Pictures/ss")), "~/Pictures/ss");
        assert_eq!(tildify(Path::new("/Users/me")), "~");
        assert_eq!(tildify(Path::new("/Volumes/Users/me")), "/Volumes/Users/me");
        assert_eq!(tildify(Path::new("/Users/")), "/Users/");
    }
}
