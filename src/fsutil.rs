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
        match OpenOptions::new().write(true).create_new(true).open(&tmp) {
            Ok(file) => return Ok((tmp, file)),
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists && attempt < 100 => {
                attempt += 1;
            }
            Err(e) => return Err(e),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
