//! Size-capped reads and private atomic writes for Margin's and the CLIs' files.

use std::{
    fmt,
    fs::File,
    io::{self, Read, Write},
    path::Path,
};

/// The step that failed after the path was found and opened. Errors finding or
/// opening the path carry no step and keep their own kind.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Step {
    /// The open file's metadata could not be read.
    Inspect,
    /// Not a regular file, or larger than allowed.
    Limit,
    Read,
    /// The temporary file could not be created next to the target.
    Create,
    /// The temporary file could not be made owner-only.
    Protect,
    Write,
    Sync,
    /// The temporary file could not replace the target.
    Persist,
}

impl fmt::Display for Step {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(self, formatter)
    }
}

impl std::error::Error for Step {}

pub fn step(error: &io::Error) -> Option<Step> {
    error.get_ref()?.downcast_ref::<Step>().copied()
}

fn failed(step: Step) -> io::Error {
    match step {
        Step::Limit => io::Error::new(io::ErrorKind::InvalidData, step),
        _ => io::Error::other(step),
    }
}

/// Reads a regular file of at most `max` bytes, checking it before and after opening.
pub fn read_capped(path: &Path, max: u64) -> io::Result<Vec<u8>> {
    let within = |metadata: &std::fs::Metadata| metadata.is_file() && metadata.len() <= max;
    if !within(&std::fs::metadata(path)?) {
        return Err(failed(Step::Limit));
    }
    let file = File::open(path)?;
    let metadata = file.metadata().map_err(|_| failed(Step::Inspect))?;
    if !within(&metadata) {
        return Err(failed(Step::Limit));
    }
    let mut bytes = Vec::new();
    file.take(max + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| failed(Step::Read))?;
    if bytes.len() as u64 > max {
        return Err(failed(Step::Limit));
    }
    Ok(bytes)
}

/// Replaces `path` with `bytes` through a synced temporary file in the same
/// folder, owner-only on Unix. The folder must exist.
pub fn write_private_atomic(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let parent = path.parent().ok_or_else(|| failed(Step::Create))?;
    let mut file = tempfile::NamedTempFile::new_in(parent).map_err(|_| failed(Step::Create))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        file.as_file()
            .set_permissions(std::fs::Permissions::from_mode(0o600))
            .map_err(|_| failed(Step::Protect))?;
    }
    file.write_all(bytes).map_err(|_| failed(Step::Write))?;
    file.as_file().sync_all().map_err(|_| failed(Step::Sync))?;
    file.persist(path).map_err(|_| failed(Step::Persist))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_files_up_to_the_cap() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("state.json");
        std::fs::write(&path, b"12345").unwrap();
        assert_eq!(read_capped(&path, 5).unwrap(), b"12345");
        assert_eq!(read_capped(&path, 6).unwrap(), b"12345");
        let error = read_capped(&path, 4).unwrap_err();
        assert_eq!(step(&error), Some(Step::Limit));
        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
    }

    #[test]
    fn missing_files_keep_their_kind_and_folders_are_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let error = read_capped(&dir.path().join("missing"), 10).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::NotFound);
        assert_eq!(step(&error), None);
        let error = read_capped(dir.path(), 1 << 20).unwrap_err();
        assert_eq!(step(&error), Some(Step::Limit));
    }

    #[test]
    fn writes_replace_the_file_atomically() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("state.json");
        std::fs::write(&path, b"old").unwrap();
        write_private_atomic(&path, b"new").unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"new");
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&path).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o600);
        }
    }

    #[test]
    fn writes_fail_without_a_folder() {
        let dir = tempfile::tempdir().unwrap();
        let error = write_private_atomic(&dir.path().join("missing/state.json"), b"x").unwrap_err();
        assert_eq!(step(&error), Some(Step::Create));
        assert_ne!(error.kind(), io::ErrorKind::NotFound);
    }
}
