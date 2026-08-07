//! Human-facing mailbox names.
//!
//! Apple Mail exports a mailbox as a *directory* named `Inbox.mbox` holding a
//! single file literally called `mbox`. Taking `file_name()` of the path we
//! actually read therefore yields `"mbox"` for every such mailbox — useless as
//! a label, and actively misleading in the `X-Mbox-Source` header written by a
//! merge. These helpers name a mailbox the way the user sees it.

use std::collections::HashMap;
use std::path::{Component, Path, PathBuf};

/// Maximum number of trailing path components used to tell two mailboxes apart.
/// Without a cap, identical paths would climb to the filesystem root forever.
const MAX_LEVELS: usize = 4;

/// The path that represents the mailbox *to the user*: the containing
/// `Name.mbox` package when `path` is Apple Mail's inner file, `path` itself
/// otherwise.
///
/// A file called `mbox` that is *not* inside a `.mbox` package keeps its own
/// name — it is a mailbox in its own right (Thunderbird stores look like this).
pub fn presentation_path(path: &Path) -> &Path {
    let is_inner = path
        .file_name()
        .is_some_and(|n| n.eq_ignore_ascii_case("mbox"));
    if !is_inner {
        return path;
    }
    match path.parent() {
        Some(parent)
            if parent
                .extension()
                .is_some_and(|e| e.eq_ignore_ascii_case("mbox")) =>
        {
            parent
        }
        _ => path,
    }
}

/// Visible name of a single mailbox: `Inbox.mbox` for `…/Account/Inbox.mbox/mbox`.
pub fn display_name(path: &Path) -> String {
    name_at_level(presentation_path(path), 1)
}

/// Visible names for a set of mailboxes, disambiguated *against each other*.
///
/// Mailboxes that would share a name get their parent directory prepended
/// (`Work/Inbox.mbox` vs `Personal/Inbox.mbox`), repeatedly, up to
/// [`MAX_LEVELS`]. Names that can no longer grow — the filesystem root is
/// reached, or two entries are genuinely the same path — are left as they are
/// instead of looping.
pub fn unique_display_names(paths: &[PathBuf]) -> Vec<String> {
    let presented: Vec<&Path> = paths.iter().map(|p| presentation_path(p)).collect();
    let mut levels = vec![1usize; paths.len()];
    let mut names: Vec<String> = presented.iter().map(|p| name_at_level(p, 1)).collect();

    // Each round lets every still-colliding name grow by one component.
    for _ in 1..MAX_LEVELS {
        let mut groups: HashMap<String, Vec<usize>> = HashMap::new();
        for (i, name) in names.iter().enumerate() {
            groups.entry(name.clone()).or_default().push(i);
        }
        let colliding: Vec<usize> = groups
            .into_values()
            .filter(|idx| idx.len() > 1)
            .flatten()
            .collect();
        if colliding.is_empty() {
            break;
        }

        let mut grew = false;
        for i in colliding {
            let next = levels[i] + 1;
            let candidate = name_at_level(presented[i], next);
            if candidate != names[i] {
                names[i] = candidate;
                levels[i] = next;
                grew = true;
            }
        }
        if !grew {
            break; // identical paths, or nothing left to prepend
        }
    }

    names
}

/// The last `level` path components, joined with `/`.
///
/// Only `Normal` components count, so drive prefixes and root separators never
/// end up in a user-facing name. A path with no normal components at all (a
/// bare root) falls back to its own string form.
fn name_at_level(path: &Path, level: usize) -> String {
    let components: Vec<String> = path
        .components()
        .filter_map(|c| match c {
            Component::Normal(part) => Some(part.to_string_lossy().to_string()),
            _ => None,
        })
        .collect();

    if components.is_empty() {
        return path.to_string_lossy().to_string();
    }
    let start = components.len().saturating_sub(level);
    components[start..].join("/")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_display_name_apple_mail_package() {
        // The real case: the path we read is the inner file, the name the user
        // knows is the package around it.
        let path = Path::new("/tmp/Account/Inbox.mbox/mbox");
        assert_eq!(display_name(path), "Inbox.mbox");
        assert_eq!(
            presentation_path(path),
            Path::new("/tmp/Account/Inbox.mbox"),
        );
    }

    #[test]
    fn test_display_name_plain_files() {
        assert_eq!(display_name(Path::new("/tmp/archive.mbox")), "archive.mbox");
        // A file called "mbox" outside a .mbox package is its own mailbox.
        assert_eq!(display_name(Path::new("/tmp/backup/mbox")), "mbox");
    }

    #[test]
    fn test_unique_display_names_disambiguates_by_parent() {
        let paths = vec![
            PathBuf::from("/tmp/Work/Inbox.mbox/mbox"),
            PathBuf::from("/tmp/Personal/Inbox.mbox/mbox"),
        ];
        assert_eq!(
            unique_display_names(&paths),
            vec!["Work/Inbox.mbox", "Personal/Inbox.mbox"],
        );
    }

    #[test]
    fn test_unique_display_names_identical_paths_terminate() {
        // Two identical paths can never be told apart: the loop must stop
        // instead of climbing to the root.
        let paths = vec![
            PathBuf::from("/tmp/Work/Inbox.mbox/mbox"),
            PathBuf::from("/tmp/Work/Inbox.mbox/mbox"),
        ];
        let names = unique_display_names(&paths);
        assert_eq!(names.len(), 2);
        assert_eq!(names[0], names[1]);
    }

    #[test]
    fn test_unique_display_names_leaves_distinct_names_short() {
        let paths = vec![
            PathBuf::from("/tmp/Work/Inbox.mbox/mbox"),
            PathBuf::from("/tmp/Work/Sent.mbox/mbox"),
            PathBuf::from("/tmp/other/archive.mbox"),
        ];
        assert_eq!(
            unique_display_names(&paths),
            vec!["Inbox.mbox", "Sent.mbox", "archive.mbox"],
        );
    }
}
