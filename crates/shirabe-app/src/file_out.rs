//! UI state for a file path and safe, non-blocking-friendly writes.
//!
//! Every writer in the application uses one set of rules: a new file is created
//! exclusively, and a confirmed replacement writes a sibling temporary file and
//! renames it over the destination instead of truncating it.

use std::fmt;
use std::fs::{self, File};
use std::io::{self, Write};
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};

/// UI-only path and one-use overwrite authorization.
pub struct Target {
    typed: String,
    pending: Option<String>,
    revision: u64,
}

impl Target {
    pub fn new(typed: String) -> Self {
        Self {
            typed,
            pending: None,
            revision: 0,
        }
    }

    pub fn path(&self) -> &str {
        &self.typed
    }

    pub fn pending(&self) -> Option<&str> {
        self.pending.as_deref()
    }

    pub fn set_path(&mut self, path: String) {
        self.typed = path;
        self.invalidate();
    }

    pub fn invalidate(&mut self) {
        self.pending = None;
        self.revision = self.revision.wrapping_add(1);
    }

    pub fn begin(&mut self, overwrite: bool) -> Option<(String, u64)> {
        let authorized = self.pending() == Some(self.path());
        self.invalidate();
        if self.typed.trim().is_empty() || (overwrite && !authorized) {
            return None;
        }
        Some((self.typed.clone(), self.revision))
    }

    /// Only the still-current request can offer confirmation after a collision.
    pub fn offer_overwrite(&mut self, path: &str, revision: u64) -> bool {
        if self.revision != revision || self.path() != path {
            return false;
        }
        self.pending = Some(path.to_owned());
        true
    }
}

/// Why a file could not be written.
#[derive(Debug)]
pub enum WriteError {
    /// The destination exists and replacement was not confirmed.
    Exists,
    /// The file could not be written.
    Io(io::Error),
}

impl fmt::Display for WriteError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Exists => write!(f, "the file already exists"),
            Self::Io(error) => write!(f, "{error}"),
        }
    }
}

impl From<io::Error> for WriteError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

/// Writes `bytes` to `path`.
///
/// A new file uses exclusive creation, reporting [`WriteError::Exists`] on a name
/// collision; a write failure may leave a partial *new* file. A confirmed
/// replacement writes a sibling temporary file, then asks the filesystem to
/// rename it over the existing regular file; it never truncates the old file.
pub fn write(path: &Path, bytes: &[u8], overwrite: bool) -> Result<(), WriteError> {
    let dir = path
        .parent()
        .filter(|dir| !dir.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(dir)?;

    if overwrite {
        match fs::symlink_metadata(path) {
            Ok(metadata) => {
                require_regular_file(&metadata)?;
                return replace_file(dir, path, bytes);
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
    }

    let mut file = match File::create_new(path) {
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
            return Err(WriteError::Exists)
        }
        result => result?,
    };
    file.write_all(bytes)?;
    Ok(())
}

fn replace_file(dir: &Path, path: &Path, bytes: &[u8]) -> Result<(), WriteError> {
    static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);
    let (temp, mut file) = loop {
        let serial = NEXT_TEMP.fetch_add(1, Ordering::Relaxed);
        let temp = dir.join(format!(
            ".shirabe-write-{}-{serial}.tmp",
            std::process::id()
        ));
        match File::create_new(&temp) {
            Ok(file) => break (temp, file),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error.into()),
        }
    };

    let written = file.write_all(bytes).and_then(|()| file.sync_all());
    drop(file);
    // Rename the directory entry, not its referent. If replacement isn't
    // supported (e.g. a Windows sharing violation), don't fall back to truncation.
    if let Err(error) = written.and_then(|()| fs::rename(&temp, path)) {
        let _ = fs::remove_file(&temp);
        return Err(error.into());
    }
    Ok(())
}

pub(crate) fn require_regular_file(metadata: &fs::Metadata) -> io::Result<()> {
    if !metadata.is_file() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "the path must be a regular file",
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    struct TestDir(std::path::PathBuf);

    impl TestDir {
        fn new(tag: &str) -> Self {
            let serial = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos();
            let path = std::env::temp_dir().join(format!(
                "shirabe-file-out-{tag}-{}-{serial}",
                std::process::id()
            ));
            fs::create_dir_all(&path).unwrap();
            Self(path)
        }

        fn join(&self, name: &str) -> std::path::PathBuf {
            self.0.join(name)
        }
    }

    impl Drop for TestDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn an_unconfirmed_write_never_replaces_an_existing_file() {
        let dir = TestDir::new("overwrite");
        let target = dir.join("export.csv");
        fs::write(&target, b"old").unwrap();
        assert!(matches!(
            write(&target, b"new", false),
            Err(WriteError::Exists)
        ));
        assert_eq!(fs::read(&target).unwrap(), b"old");
    }

    #[test]
    fn a_new_write_creates_the_file_exclusively() {
        let dir = TestDir::new("create");
        let target = dir.join("export.json");
        write(&target, b"{\"a\":1}", false).unwrap();
        assert_eq!(fs::read(&target).unwrap(), b"{\"a\":1}");
    }

    #[test]
    fn an_unconfirmed_write_does_not_follow_a_dangling_symlink() {
        let dir = TestDir::new("dangling-link");
        let target = dir.join("export.csv");
        let outside = dir.join("outside.csv");
        std::os::unix::fs::symlink(&outside, &target).unwrap();
        assert!(matches!(
            write(&target, b"new", false),
            Err(WriteError::Exists)
        ));
        assert!(!outside.exists());
    }

    #[test]
    fn a_confirmed_write_replaces_the_file_without_modifying_its_other_hard_link() {
        let dir = TestDir::new("confirmed-hard-link");
        let target = dir.join("export.csv");
        let other = dir.join("other.csv");
        fs::write(&target, b"old").unwrap();
        fs::hard_link(&target, &other).unwrap();
        write(&target, b"new", true).unwrap();
        assert_eq!(fs::read(&target).unwrap(), b"new");
        assert_eq!(fs::read(&other).unwrap(), b"old");
    }

    #[test]
    fn a_failed_replacement_removes_its_temporary_file() {
        let dir = TestDir::new("failed-replace");
        let target = dir.join("occupied");
        // A directory cannot be replaced by a regular file, so the rename fails.
        fs::create_dir(&target).unwrap();
        assert!(matches!(
            write(&target, b"new", true),
            Err(WriteError::Io(_))
        ));
        let leftovers = fs::read_dir(&dir.0)
            .unwrap()
            .filter_map(Result::ok)
            .filter(|entry| entry.file_name().to_string_lossy().contains(".shirabe-"))
            .count();
        assert_eq!(leftovers, 0);
    }

    #[test]
    fn a_replacement_refuses_a_special_file() {
        use std::os::unix::fs::FileTypeExt;

        let dir = TestDir::new("fifo");
        let target = dir.join("export.csv");
        let status = std::process::Command::new("mkfifo")
            .arg(&target)
            .status()
            .unwrap();
        assert!(status.success());
        assert!(matches!(
            write(&target, b"new", true),
            Err(WriteError::Io(_))
        ));
        assert!(fs::symlink_metadata(&target).unwrap().file_type().is_fifo());
    }
}
