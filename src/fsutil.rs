//! Small filesystem helpers shared by every code path that writes a file.
//!
//! mboxshell is read-only toward the mailboxes it opens, so anything that
//! writes (exports, merges, the index sidecar) goes through these: a write
//! target is never allowed to be one of the inputs, and temp files are created
//! fresh instead of opened through whatever already sits at their name.

use std::fs::{File, OpenOptions};
use std::io;
use std::path::{Path, PathBuf};

/// True when `a` and `b` name the same existing file.
///
/// Compares device and inode on Unix, so hard links and differently spelled
/// paths (`./x.mbox`, `../dir/x.mbox`, a symlink) all match; elsewhere it
/// compares canonical paths. A path that does not exist matches nothing.
pub fn same_file(a: &Path, b: &Path) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if let (Ok(ma), Ok(mb)) = (std::fs::metadata(a), std::fs::metadata(b)) {
            return ma.dev() == mb.dev() && ma.ino() == mb.ino();
        }
        false
    }
    #[cfg(not(unix))]
    {
        match (a.canonicalize(), b.canonicalize()) {
            (Ok(ca), Ok(cb)) => ca == cb,
            _ => false,
        }
    }
}

/// Create a new, empty file at `path`, or at `stem_1.ext`, `stem_2.ext`… if
/// that name is taken. Returns the path actually created and the open file.
///
/// The file is opened with `create_new`, so an existing file is never
/// overwritten and a planted symlink is never followed.
pub fn create_unique(path: &Path) -> io::Result<(PathBuf, File)> {
    let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("file");
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
    let parent = path.parent().unwrap_or(Path::new("."));

    let mut i: u64 = 0;
    loop {
        let candidate = match (i, ext.is_empty()) {
            (0, _) => path.to_path_buf(),
            (_, true) => parent.join(format!("{stem}_{i}")),
            (_, false) => parent.join(format!("{stem}_{i}.{ext}")),
        };
        match OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&candidate)
        {
            Ok(file) => return Ok((candidate, file)),
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => i += 1,
            Err(e) => return Err(e),
        }
    }
}

/// Create the directory `path`, or accept the one already there — but only
/// a real directory, never a symlink to one.
///
/// Export subfolders have predictable names (`{date}_{subject}`, `cur`,
/// `0007`). In an output directory someone else can write to, a symlink
/// planted under that name sent the exported files wherever it pointed.
/// The parent must exist.
pub fn create_dir_no_follow(path: &Path) -> io::Result<()> {
    match std::fs::create_dir(path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {
            if std::fs::symlink_metadata(path)?.file_type().is_dir() {
                Ok(())
            } else {
                Err(io::Error::new(
                    io::ErrorKind::AlreadyExists,
                    format!("{} exists and is not a directory", path.display()),
                ))
            }
        }
        Err(e) => Err(e),
    }
}

/// The first of `inputs` that `output` would overwrite, if any.
pub fn output_clobbers_input<'a>(output: &Path, inputs: &[&'a Path]) -> Option<&'a Path> {
    inputs
        .iter()
        .copied()
        .find(|input| same_file(output, input))
}

/// Create a new, empty temp file next to `dest`, for writing and then
/// renaming over it.
///
/// The name is hidden and carries the process id, and the file is opened
/// with `create_new`: a stale temp — or a symlink someone planted at the
/// predictable `dest.tmp` name — is never opened and written through.
pub fn create_temp_beside(dest: &Path) -> io::Result<(PathBuf, File)> {
    create_temp_beside_with(dest, false)
}

/// [`create_temp_beside`] for a file only its owner may read (mode `0600` on
/// Unix), whatever the umask says.
///
/// The index holds senders, recipients and subjects: created with the
/// default mode it was world-readable next to a mailbox that was not.
pub fn create_private_temp_beside(dest: &Path) -> io::Result<(PathBuf, File)> {
    create_temp_beside_with(dest, true)
}

fn create_temp_beside_with(dest: &Path, private: bool) -> io::Result<(PathBuf, File)> {
    let dir = match dest.parent() {
        Some(p) if !p.as_os_str().is_empty() => p,
        _ => Path::new("."),
    };
    let name = dest
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "mboxshell".to_string());
    let pid = std::process::id();

    let mut attempt = 0u32;
    loop {
        let tmp = dir.join(format!(".{name}.{pid}.{attempt}.tmp"));
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        if private {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        #[cfg(not(unix))]
        let _ = private;
        match options.open(&tmp) {
            Ok(file) => return Ok((tmp, file)),
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists && attempt < 100 => {
                attempt += 1;
            }
            Err(e) => return Err(e),
        }
    }
}

/// Removes a temp output file on drop unless disarmed, so an export or merge
/// that fails half-way does not leave a stray temp file next to the output.
///
/// Declare it *before* the writer on that file: locals drop in reverse order,
/// so the file handle is closed before the removal (required on Windows).
pub(crate) struct TempFileGuard {
    path: PathBuf,
    armed: bool,
}

impl TempFileGuard {
    /// Guard the temp file at `path`.
    pub(crate) fn new(path: &Path) -> Self {
        Self {
            path: path.to_path_buf(),
            armed: true,
        }
    }

    /// The file was renamed into place: leave it alone.
    pub(crate) fn disarm(&mut self) {
        self.armed = false;
    }
}

impl Drop for TempFileGuard {
    fn drop(&mut self) {
        if self.armed {
            let _ = std::fs::remove_file(&self.path);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_unique_never_reuses_a_taken_name() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("mboxshell_export.csv");
        std::fs::write(&target, "first").unwrap();

        let (second, _) = create_unique(&target).unwrap();
        let (third, _) = create_unique(&target).unwrap();
        assert_eq!(second, dir.path().join("mboxshell_export_1.csv"));
        assert_eq!(third, dir.path().join("mboxshell_export_2.csv"));
        assert_eq!(std::fs::read_to_string(&target).unwrap(), "first");
    }

    #[test]
    fn test_same_file_sees_through_path_spelling() {
        let dir = tempfile::tempdir().unwrap();
        let a = dir.path().join("a.mbox");
        std::fs::write(&a, b"x").unwrap();
        let spelled = dir.path().join(".").join("a.mbox");
        assert!(same_file(&a, &spelled));
        assert!(!same_file(&a, &dir.path().join("missing.mbox")));
    }

    #[cfg(unix)]
    #[test]
    fn test_same_file_sees_through_symlinks() {
        let dir = tempfile::tempdir().unwrap();
        let a = dir.path().join("a.mbox");
        let link = dir.path().join("link.mbox");
        std::fs::write(&a, b"x").unwrap();
        std::os::unix::fs::symlink(&a, &link).unwrap();
        assert!(same_file(&a, &link));
    }

    #[test]
    fn test_create_dir_no_follow_accepts_only_real_directories() {
        let dir = tempfile::tempdir().unwrap();
        let sub = dir.path().join("sub");
        create_dir_no_follow(&sub).unwrap();
        create_dir_no_follow(&sub).unwrap(); // already there: fine

        let file = dir.path().join("file");
        std::fs::write(&file, b"x").unwrap();
        assert!(create_dir_no_follow(&file).is_err());

        #[cfg(unix)]
        {
            let elsewhere = dir.path().join("elsewhere");
            std::fs::create_dir(&elsewhere).unwrap();
            let link = dir.path().join("link");
            std::os::unix::fs::symlink(&elsewhere, &link).unwrap();
            assert!(create_dir_no_follow(&link).is_err());
        }
    }

    #[test]
    fn test_create_temp_beside_never_reuses_a_name() {
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("out.mbox");
        let (t1, _f1) = create_temp_beside(&dest).unwrap();
        let (t2, _f2) = create_temp_beside(&dest).unwrap();
        assert_ne!(t1, t2);
        assert_eq!(t1.parent(), Some(dir.path()));
    }

    #[cfg(unix)]
    #[test]
    fn test_create_temp_beside_does_not_follow_a_planted_symlink() {
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("out.mbox");
        let victim = dir.path().join("victim");
        std::fs::write(&victim, b"keep me").unwrap();
        let pid = std::process::id();
        let planted = dir.path().join(format!(".out.mbox.{pid}.0.tmp"));
        std::os::unix::fs::symlink(&victim, &planted).unwrap();

        let (tmp, _f) = create_temp_beside(&dest).unwrap();
        assert_ne!(tmp, planted, "the planted name must be skipped");
        assert_eq!(std::fs::read(&victim).unwrap(), b"keep me");
    }
}
