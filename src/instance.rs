use std::fs::{self, File, OpenOptions};
use std::path::Path;

use color_eyre::eyre::{Result, WrapErr};
use fs2::FileExt;

/// Held for the life of the app; the OS releases the lock when the process exits.
pub struct InstanceLock {
    _file: File,
}

/// Takes the single-instance lock, or returns `None` when another ss owns it.
pub fn acquire(path: &Path) -> Result<Option<InstanceLock>> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let file = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(path)
        .wrap_err_with(|| format!("could not open instance lock {}", path.display()))?;
    match file.try_lock_exclusive() {
        Ok(()) => Ok(Some(InstanceLock { _file: file })),
        Err(error) if error.raw_os_error() == fs2::lock_contended_error().raw_os_error() => {
            Ok(None)
        }
        Err(error) => Err(error).wrap_err_with(|| format!("could not lock {}", path.display())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_one_owner_at_a_time() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("ss.lock");
        let first = acquire(&path).unwrap();
        assert!(first.is_some());
        assert!(acquire(&path).unwrap().is_none());
        drop(first);
        assert!(acquire(&path).unwrap().is_some());
    }
}
